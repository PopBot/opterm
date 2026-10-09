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
    write(connection, id, "stty -echo; printf 'READY\\n'\n");
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
        "printf 'COLOR_ENV:%s:%s:%s\\n' \"$TERM\" \"$COLORTERM\" \"${NO_COLOR-unset}\"; printf '\\033[31mRED\\033[91mBRIGHT\\033[38;5;208mINDEXED\\033[38;2;12;170;240mRGB\\033[0m\\n'\n",
    );
    let current = wait_screen(&mut connection, id, "RGB");
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
