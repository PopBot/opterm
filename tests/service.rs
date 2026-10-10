//! Real service processes and PTYs, isolated from the user's live sessions.
#![cfg(unix)]
use opterm::service::{Connection, Reply, Request, Snapshot};
use std::os::unix::fs::PermissionsExt;
use std::{
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

struct Service {
    directory: TempDir,
    child: Child,
}
impl Service {
    fn start() -> Self {
        let directory = tempfile::Builder::new()
            .prefix("opterm-test-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let child = Self::spawn(directory.path());
        let service = Self { directory, child };
        service.connect();
        service
    }
    fn spawn(path: &Path) -> Child {
        Command::new(env!("CARGO_BIN_EXE_opterm"))
            .arg("--session-service")
            .env("OPTERM_STATE_DIR", path)
            .env("NO_COLOR", "1") // Simulate a development/tool launcher.
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .unwrap()
    }
    fn connect(&self) -> Connection {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Ok(connection) = Connection::connect(self.directory.path()) {
                return connection;
            }
            assert!(Instant::now() < deadline, "Service startup timed out");
            thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        if let Ok(mut connection) = Connection::connect(self.directory.path()) {
            let _ = connection.call(Request::Shutdown);
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while self.child.try_wait().unwrap().is_none() {
            if Instant::now() > deadline {
                let _ = self.child.kill();
                let _ = self.child.wait();
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}
fn snapshot(reply: Reply) -> Snapshot {
    let Reply::Snapshot(snapshot) = reply else {
        panic!("Expected terminal snapshot")
    };
    *snapshot
}
fn list(connection: &mut Connection) -> Vec<opterm::service::Info> {
    let Reply::List(infos) = connection.call(Request::List).unwrap() else {
        panic!("Expected session list")
    };
    infos
}
fn write(connection: &mut Connection, id: u64, command: &str) {
    connection
        .call(Request::Write {
            id,
            bytes: command.as_bytes().to_vec(),
        })
        .unwrap();
}
fn wait_screen(connection: &mut Connection, id: u64, text: &str) -> Snapshot {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let current = snapshot(connection.call(Request::View { id, since: None }).unwrap());
        if current.screen.text().contains(text) {
            return current;
        }
        assert!(
            Instant::now() < deadline,
            "Missing {text}: {}",
            current.screen.text()
        );
        thread::sleep(Duration::from_millis(10));
    }
}
fn shell(connection: &mut Connection) -> u64 {
    let id = snapshot(
        connection
            .call(Request::Create(vec!["/bin/sh".into()]))
            .unwrap(),
    )
    .info
    .id;
    // Readiness must come from execution, never the terminal's initial echo.
    write(connection, id, "stty -echo; printf 'REA%s\\n' DY\n");
    wait_screen(connection, id, "READY");
    id
}

#[test]
fn shell_and_scrollback_survive_view_disconnect_and_detach() {
    let service = Service::start();
    let mut connection = service.connect();
    let id = shell(&mut connection);
    let pid_file = service.directory.path().join("shell-pid");
    write(
        &mut connection,
        id,
        &format!(
            "export OPTERM_TEST=still-here; printf '%s' $$ > '{}'; printf 'BEFORE_CLOSE\\n'; (sleep 0.3; printf 'OUTPUT_WHILE_DETACHED\\n') &\n",
            pid_file.display()
        ),
    );
    wait_screen(&mut connection, id, "BEFORE_CLOSE");
    let pid = std::fs::read_to_string(&pid_file).unwrap();
    connection
        .call(Request::Rename {
            id,
            name: Some("Persistent shell".into()),
        })
        .unwrap();
    drop(connection); // Desktop EOF/crash never owns or kills the shell.
    thread::sleep(Duration::from_millis(400));
    let mut reopened = service.connect();
    let infos = list(&mut reopened);
    assert_eq!(infos.len(), 1);
    assert_eq!(infos[0].id, id);
    assert!(!infos[0].attached);
    assert!(!infos[0].exited);
    let restored = snapshot(reopened.call(Request::Attach(id)).unwrap());
    assert!(restored.info.controlled);
    assert_eq!(restored.info.name.as_deref(), Some("Persistent shell"));
    assert!(restored.screen.text().contains("OUTPUT_WHILE_DETACHED"));
    write(
        &mut reopened,
        id,
        "printf 'RESTORED:%s:%s\\n' $$ \"$OPTERM_TEST\"\n",
    );
    wait_screen(&mut reopened, id, &format!("RESTORED:{pid}:still-here"));

    // Headless parsing continues beyond the visible screen; no command replay.
    write(
        &mut reopened,
        id,
        "printf 'HISTORY_ANCHOR\\n'; i=0; while [ $i -lt 100 ]; do printf 'line %s\\n' $i; i=$((i+1)); done; printf 'HISTORY_DONE\\n'\n",
    );
    wait_screen(&mut reopened, id, "HISTORY_DONE");
    reopened.call(Request::Detach(id)).unwrap();
    assert!(!list(&mut reopened)[0].attached);
    reopened.call(Request::Attach(id)).unwrap();
    reopened
        .call(Request::Scroll {
            id,
            lines: i32::MAX,
        })
        .unwrap();
    wait_screen(&mut reopened, id, "HISTORY_ANCHOR");
    reopened.call(Request::End(id)).unwrap();
    assert!(list(&mut reopened).is_empty());
}

#[test]
fn one_controller_and_duplicate_service_preserve_existing_work() {
    let service = Service::start();
    let mut first = service.connect();
    let id = shell(&mut first);
    let mut second = service.connect();
    assert!(
        !snapshot(second.call(Request::Attach(id)).unwrap())
            .info
            .controlled
    );
    assert!(
        second
            .call(Request::Write {
                id,
                bytes: b"echo UNAUTHORIZED\n".to_vec()
            })
            .is_err()
    );
    assert!(
        second
            .call(Request::Resize {
                id,
                cols: 80,
                rows: 24,
                width: 8.0,
                height: 17.0
            })
            .is_err()
    );
    assert!(
        snapshot(second.call(Request::TakeControl(id)).unwrap())
            .info
            .controlled
    );
    assert!(
        first
            .call(Request::Write {
                id,
                bytes: b"echo STALE\n".to_vec()
            })
            .is_err()
    );
    assert!(first.call(Request::End(id)).is_err());
    write(&mut second, id, "printf 'CONTROL_OK\\n'\n");
    wait_screen(&mut second, id, "CONTROL_OK");
    let mut duplicate = Service::spawn(service.directory.path());
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = duplicate.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if Instant::now() > deadline {
            duplicate.kill().unwrap();
            duplicate.wait().unwrap();
            panic!("Duplicate service failed to exit");
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(list(&mut second)[0].id, id);
    let Reply::Hello { pid, .. } = second.call(Request::Hello(1)).unwrap() else {
        panic!("Expected hello")
    };
    assert_eq!(pid, service.child.id());
    drop(second);
    thread::sleep(Duration::from_millis(50));
    assert!(
        snapshot(first.call(Request::Attach(id)).unwrap())
            .info
            .controlled
    );
    let screen = wait_screen(&mut first, id, "CONTROL_OK");
    assert!(!screen.screen.text().contains("UNAUTHORIZED"));
    assert!(!screen.screen.text().contains("STALE"));
}

#[test]
fn service_failure_never_replays_old_work() {
    let mut service = Service::start();
    let mut connection = service.connect();
    let id = shell(&mut connection);
    write(&mut connection, id, "printf 'OLD_WORK\\n'\n");
    wait_screen(&mut connection, id, "OLD_WORK");
    service.child.kill().unwrap();
    service.child.wait().unwrap();
    assert!(connection.call(Request::List).is_err());
    service.child = Service::spawn(service.directory.path());
    let mut restarted = service.connect();
    assert!(list(&mut restarted).is_empty());
    assert!(restarted.call(Request::Attach(id)).is_err());
}

#[test]
fn foreground_agent_fixture_keeps_alternate_screen_and_process_state() {
    let service = Service::start();
    let mut first = service.connect();
    let pid_file = service.directory.path().join("agent-pid");
    let script = format!(
        "stty -echo; printf '%s' $$ > '{}'; printf '\\033[?1049hAGENT_READY\\n'; (sleep 0.3; printf 'AGENT_BACKGROUND\\n') & n=0; while IFS= read -r line; do n=$((n+1)); printf 'STEP:%s:%s\\n' $n \"$line\"; done",
        pid_file.display()
    );
    let id = snapshot(
        first
            .call(Request::Create(vec!["/bin/sh".into(), "-c".into(), script]))
            .unwrap(),
    )
    .info
    .id;
    wait_screen(&mut first, id, "AGENT_READY");
    let pid = std::fs::read_to_string(&pid_file).unwrap();
    write(&mut first, id, "first\n");
    wait_screen(&mut first, id, "STEP:1:first");
    drop(first);
    thread::sleep(Duration::from_millis(400));
    let mut reopened = service.connect();
    let restored = snapshot(reopened.call(Request::Attach(id)).unwrap());
    assert!(
        restored
            .screen
            .mode
            .contains(alacritty_terminal::term::TermMode::ALT_SCREEN)
    );
    assert!(restored.screen.text().contains("AGENT_BACKGROUND"));
    assert_eq!(std::fs::read_to_string(pid_file).unwrap(), pid);
    write(&mut reopened, id, "second\n");
    wait_screen(&mut reopened, id, "STEP:2:second");
}

#[test]
fn interactive_sessions_enable_color_and_keep_palette_and_truecolor() {
    let service = Service::start();
    let mut connection = service.connect();
    let id = shell(&mut connection);
    write(
        &mut connection,
        id,
        "stty -echo; printf '\\033[2J\\033[H'; printf 'COLOR_ENV:%s:%s:%s\\n' \"$TERM\" \"$COLORTERM\" \"${NO_COLOR-unset}\"; printf '\\033[31mRED\\033[91mBRIGHT\\033[38;5;208mINDEXED\\033[38;2;12;170;240mRGB\\033[0m\\n'; printf '%s_%s\\n' COLOR DONE\n",
    );
    let current = wait_screen(&mut connection, id, "COLOR_DONE");
    assert!(
        current
            .screen
            .text()
            .contains("COLOR_ENV:xterm-256color:truecolor:unset")
    );
    for (word, expected) in [
        ("RED", opterm::terminal::palette(1)),
        ("BRIGHT", opterm::terminal::palette(9)),
        ("INDEXED", opterm::terminal::palette(208)),
        ("RGB", [12, 170, 240]),
    ] {
        let cells = current
            .screen
            .cells
            .windows(word.len())
            .find(|cells| cells.iter().map(|c| c.text.as_str()).collect::<String>() == word)
            .unwrap();
        assert!(
            cells.iter().all(|cell| cell.fg == expected),
            "{word} lost its color through IPC"
        );
    }
}

#[test]
fn configured_profiles_capture_launch_and_live_themes_preserve_program_colors() {
    use opterm::profiles::{Environment, Launch, Profile, Rgb, Theme};
    let service = Service::start();
    let mut connection = service.connect();
    assert!(connection.profiles);
    let theme = Theme {
        id: "custom".into(),
        name: "Custom".into(),
        ..Theme::light()
    };
    let profile = Profile {
        id: "agent".into(),
        name: "Agent shell".into(),
        executable: "/bin/sh".into(),
        arguments: vec!["-i".into()],
        directory: service.directory.path().display().to_string(),
        theme: theme.id.clone(),
        environment: vec![Environment {
            name: "PROFILE_VALUE".into(),
            value: "one value with spaces".into(),
            secret: false,
        }],
    };
    let launch = Launch {
        profile,
        workspace_directory: "/this/workspace/fallback/is/unavailable".into(),
        theme,
    };
    let initial = snapshot(
        connection
            .call(Request::CreateProfile(Box::new(launch.clone())))
            .unwrap(),
    );
    let id = initial.info.id;
    assert_eq!(initial.info.profile.as_ref().unwrap().name, "Agent shell");
    assert_eq!(initial.screen.background, Theme::light().background.0);
    write(
        &mut connection,
        id,
        "stty -echo; printf '%s|%s|%s' $$ \"$PWD\" \"$PROFILE_VALUE\" > captured; printf 'REA%s\\n' DY\n",
    );
    wait_screen(&mut connection, id, "READY");
    let captured = std::fs::read_to_string(service.directory.path().join("captured")).unwrap();
    assert!(captured.ends_with("|one value with spaces"));
    let directory = captured.split('|').nth(1).unwrap();
    assert_eq!(
        std::fs::canonicalize(directory).unwrap(),
        std::fs::canonicalize(service.directory.path()).unwrap()
    );
    write(
        &mut connection,
        id,
        "printf '\\033[2J\\033[H\\033[31mA\\033[38;2;9;8;7mB\\033[0m'; printf '\\033]4;2;rgb:04/05/06\\007\\033[32mC\\033[0m'; printf '\\033]11;rgb:07/08/09\\007'\n",
    );
    wait_screen(&mut connection, id, "ABC");
    let mut changed = launch.theme.clone();
    changed.ansi[1] = Rgb([1, 2, 3]);
    changed.foreground = Rgb([44, 55, 66]);
    connection
        .call(Request::Themes(vec![changed.clone()]))
        .unwrap();
    let updated = snapshot(connection.call(Request::View { id, since: None }).unwrap());
    let cell = |c| {
        updated
            .screen
            .cells
            .iter()
            .find(|cell| cell.text == c)
            .unwrap()
    };
    assert_eq!(cell("A").fg, [1, 2, 3]);
    assert_eq!(cell("B").fg, [9, 8, 7]);
    assert_eq!(cell("C").fg, [4, 5, 6]);
    assert_eq!(updated.screen.background, [7, 8, 9]);
    // Removing a theme falls back without changing the process or captured launch environment.
    connection.call(Request::Themes(vec![])).unwrap();
    write(
        &mut connection,
        id,
        "printf '%s|%s|%s' $$ \"$PWD\" \"$PROFILE_VALUE\" > after; printf 'STILL_RUNNING\\n'\n",
    );
    wait_screen(&mut connection, id, "STILL_RUNNING");
    assert_eq!(
        std::fs::read_to_string(service.directory.path().join("after")).unwrap(),
        captured
    );
    drop(connection);
    let mut reopened = service.connect();
    let restored = snapshot(reopened.call(Request::Attach(id)).unwrap());
    assert_eq!(restored.info.profile.as_ref().unwrap().id, "agent");
    let before = list(&mut reopened).len();
    let mut bad = launch.clone();
    bad.profile.directory = "/this/profile/folder/is/unavailable".into();
    assert!(
        reopened
            .call(Request::CreateProfile(Box::new(bad)))
            .err()
            .unwrap()
            .to_string()
            .contains("unavailable")
    );
    let mut bad = launch;
    bad.profile.executable = "/this/program/does/not/exist".into();
    assert!(
        reopened
            .call(Request::CreateProfile(Box::new(bad)))
            .is_err()
    );
    assert_eq!(list(&mut reopened).len(), before);
}

#[test]
#[ignore = "requires a preserved pre-profile Opterm binary in OPTERM_TEST_OLD_BINARY"]
fn older_service_keeps_existing_work_when_profiles_are_rejected() {
    use opterm::{profiles::Launch, service::client::Client};
    let directory = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let child = Command::new(std::env::var_os("OPTERM_TEST_OLD_BINARY").unwrap())
        .arg("--session-service")
        .env("OPTERM_STATE_DIR", directory.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let service = Service { directory, child };
    let mut connection = service.connect();
    assert!(!connection.profiles);
    let id = shell(&mut connection);
    let original = service.directory.path().join("old-pid");
    write(
        &mut connection,
        id,
        &format!(
            "printf '%s' $$ > '{}'; printf 'OLD_READY\\n'\n",
            original.display()
        ),
    );
    wait_screen(&mut connection, id, "OLD_READY");
    drop(connection);
    let client = Client::open(
        service.directory.path(),
        Launch::legacy(&[]),
        false,
        true,
        || {},
    )
    .unwrap();
    assert!(!client.profiles);
    assert!(
        client
            .send(Request::CreateProfile(Box::new(Launch::legacy(&[]))))
            .is_err()
    );
    let after = service.directory.path().join("still-alive");
    client
        .session(id)
        .write(format!("printf '%s' $$ > '{}'\n", after.display()).as_bytes())
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !after.exists() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        std::fs::read_to_string(&after).unwrap(),
        std::fs::read_to_string(&original).unwrap()
    );
    assert!(client.take_error().is_none());
}

#[test]
fn saved_references_reattach_only_selected_work_and_never_reuse_ids_after_restart() {
    use opterm::{
        layouts::SessionRef,
        profiles::Launch,
        service::client::{Client, Reconnect},
    };
    let mut service = Service::start();
    let mut connection = service.connect();
    let original = connection.instance.clone();
    assert!(!original.is_empty());
    assert_eq!(service.connect().instance, original);
    let first = shell(&mut connection);
    let second = shell(&mut connection);
    write(
        &mut connection,
        first,
        "export LAYOUT_STATE=retained; printf 'LAYOUT_READY\\n'\n",
    );
    wait_screen(&mut connection, first, "LAYOUT_READY");
    connection.call(Request::Detach(first)).unwrap();
    let refs = vec![SessionRef {
        instance: original.clone(),
        id: first,
    }];
    let client = Client::open_with(
        service.directory.path(),
        Launch::legacy(&[]),
        false,
        Reconnect::Saved(refs.clone()),
        false,
        || {},
    )
    .unwrap();
    assert_eq!(client.view_updates().0, vec![first]);
    assert!(client.session(first).controlled());
    assert_eq!(list(&mut connection).len(), 2);
    assert!(!client.view_updates().0.contains(&second));
    client
        .send(Request::Write {
            id: first,
            bytes: b"printf 'LAYOUT:%s\\n' \"$LAYOUT_STATE\"\n".to_vec(),
        })
        .unwrap();
    connection.call(Request::Attach(first)).unwrap();
    wait_screen(&mut connection, first, "LAYOUT:retained");
    drop(client);
    service.child.kill().unwrap();
    service.child.wait().unwrap();
    service.child = Service::spawn(service.directory.path());
    let mut restarted = service.connect();
    assert_ne!(restarted.instance, original);
    let reused = shell(&mut restarted);
    assert_eq!(reused, first);
    let client = Client::open_with(
        service.directory.path(),
        Launch::legacy(&[]),
        false,
        Reconnect::Saved(refs),
        false,
        || {},
    )
    .unwrap();
    assert!(client.view_updates().0.is_empty());
    assert_eq!(list(&mut restarted).len(), 1);
    let screen = snapshot(
        restarted
            .call(Request::View {
                id: reused,
                since: None,
            })
            .unwrap(),
    );
    assert!(!screen.screen.text().contains("LAYOUT_READY"));
}
