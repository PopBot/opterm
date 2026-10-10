//! Opt-in shell hooks through real PTYs, plus observer-only output queries.
#![cfg(unix)]
use opterm::{
    profiles::Launch,
    service::{
        Connection, Reply, Request,
        client::{Client, OutputResult, Reconnect},
    },
    terminal::{Screen, Session},
};
use std::{
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

fn wait_for(session: &Session, predicate: impl Fn(&Screen) -> bool) -> Screen {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let screen = session.screen();
        if predicate(&screen) {
            return screen;
        }
        assert!(Instant::now() < deadline, "PTY timeout: {}", screen.text());
        thread::sleep(Duration::from_millis(10));
    }
}

fn wait_output(session: &Session, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if session
            .last_command_output()
            .is_ok_and(|output| output == expected)
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "Output timeout: {:?}; {}",
            session.last_command_output(),
            session.screen().text()
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn bash_and_zsh_sourceable_hooks_track_output_cwd_and_wrapped_commands() {
    for (shell, args, extension) in [
        ("/bin/bash", vec!["--noprofile", "--norc", "-i"], "bash"),
        ("/usr/bin/zsh", vec!["-f", "-i"], "zsh"),
    ] {
        if !std::path::Path::new(shell).is_file() {
            continue;
        }
        let mut argv = vec![shell.into()];
        argv.extend(args.into_iter().map(String::from));
        let session = Session::spawn(&argv, || {}).unwrap();
        // Set a deterministic prompt and avoid startup state from user configs.
        wait_for(&session, |screen| !screen.text().trim().is_empty());
        let script = format!(
            "{}/scripts/shell-integration/opterm.{extension}",
            env!("CARGO_MANIFEST_DIR")
        );
        let existing_hook = if extension == "bash" {
            "PROMPT_COMMAND=(\"printf 'hook-output\\n'\"); "
        } else {
            "__existing_prompt() { printf 'hook-output\\n'; }; precmd_functions=(__existing_prompt); "
        };
        session
            .write(format!("{existing_hook}PS1='opterm> '; source '{script}'\r").as_bytes())
            .unwrap();
        wait_for(&session, |screen| {
            screen.shell_integration && screen.cwd.is_some()
        });
        session.resize(22, 6, 8.0, 17.0).unwrap();
        // The echo wraps and scrolls before the C boundary; it must not enter output.
        session
            .write(b"\rprintf 'long-wrapped-command-output\\n'; printf 'last line\\n'\r")
            .unwrap();
        wait_output(&session, "long-wrapped-command-output\nlast line\n");
        let screen = session.screen();
        assert!(screen.shell_integration);
        assert!(session.navigate_prompt(true).is_ok());
        assert!(session.screen().prompt_navigation.is_some());
        session.scroll(-1000);
        assert!(session.screen().prompt_navigation.is_none());
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("space # λ folder");
        std::fs::create_dir(&target).unwrap();
        session
            .write(format!("cd '{}'; printf 'cwd updated\\n'\r", target.display()).as_bytes())
            .unwrap();
        wait_for(&session, |screen| screen.cwd.as_deref() == target.to_str());
        wait_output(&session, "cwd updated\n");
        session.resize(40, 8, 8.0, 17.0).unwrap();
        assert_eq!(session.screen().cwd.as_deref(), target.to_str());
        assert!(session.last_command_output().is_err());
        session.write(b"\rprintf 'after resize\\n'\r").unwrap();
        wait_output(&session, "after resize\n");
        session.write(b"exit\r").unwrap();
        wait_for(&session, |screen| screen.exited);
    }
}

struct Service {
    directory: TempDir,
    child: Child,
}
impl Service {
    fn start() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::Builder::new()
            .prefix("opterm-shell-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_opterm"))
            .arg("--session-service")
            .env("OPTERM_STATE_DIR", directory.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let service = Self { directory, child };
        let deadline = Instant::now() + Duration::from_secs(5);
        while Connection::connect(service.directory.path()).is_err() {
            assert!(Instant::now() < deadline, "Service startup timeout");
            thread::sleep(Duration::from_millis(10));
        }
        service
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        if let Ok(mut connection) = Connection::connect(self.directory.path()) {
            let _ = connection.call(Request::Shutdown);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn wait_result(client: &Client, token: u64) -> OutputResult {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        for result in client.take_output_results() {
            if result.token == token {
                return result;
            }
        }
        assert!(Instant::now() < deadline, "Output query timeout");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn observed_output_copy_is_readonly_and_errors_are_query_specific() {
    let service = Service::start();
    let mut launch = Launch::legacy(&[]);
    launch.profile.executable = "/bin/sh".into();
    launch.profile.arguments = vec!["-c".into(), r#"printf '\033]133;A\007$ \033]133;B\007command\r\n\033]133;C\007only output\r\n\033]133;D;0\007\033]133;A\007$ \033]133;B\007'; sleep 10"#.into()];
    let owner = Client::open_with(
        service.directory.path(),
        launch.clone(),
        false,
        Reconnect::Fresh,
        true,
        || {},
    )
    .unwrap();
    assert!(owner.shell_integration_supported);
    assert!(owner.take_error().is_none());
    let id = owner.infos()[0].id;
    let observer = Client::open_with(
        service.directory.path(),
        launch,
        false,
        Reconnect::All,
        false,
        || {},
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !observer.session(id).screen().shell_integration {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    observer.copy_last_output(id, 101).unwrap();
    let result = wait_result(&observer, 101);
    assert_eq!(result.id, id);
    assert_eq!(result.result.unwrap(), "only output\n");
    assert!(!observer.infos()[0].controlled);
    assert!(owner.infos()[0].controlled);
    assert!(observer.take_error().is_none());
    observer.copy_last_output(u64::MAX, 102).unwrap();
    assert!(wait_result(&observer, 102).result.is_err());
    assert!(observer.take_error().is_none());
    let mut readonly = Connection::connect(service.directory.path()).unwrap();
    readonly.call(Request::Attach(id)).unwrap();
    assert!(
        readonly
            .call(Request::Prompt {
                id,
                backwards: true
            })
            .err()
            .unwrap()
            .to_string()
            .contains("controlled in another window")
    );
}

#[test]
fn legacy_service_has_no_shell_integration_capability() {
    let reply: Reply = serde_json::from_str(
        r#"{"Hello":{"version":1,"pid":1,"profiles":true,"instance":"legacy"}}"#,
    )
    .unwrap();
    assert!(matches!(
        reply,
        Reply::Hello {
            shell_integration: false,
            ..
        }
    ));
}
