//! Native PTY and local-transport coverage on each CI operating system.
//! This does not exercise desktop IME, accessibility or display-server behavior.
use opterm::{
    history::Options,
    service::{Connection, Reply, Request, Snapshot},
    terminal::{Screen, Session},
};
use std::{
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const TIMEOUT: Duration = Duration::from_secs(15);

fn interrupt_program() -> Vec<String> {
    program(
        "trap 'printf INTERRUPTED; exit 0' INT; printf 'READY\\r\\n'; sleep 30",
        "[Console]::WriteLine('READY'); try { Start-Sleep -Seconds 30 } finally { [Console]::WriteLine('INTERRUPTED') }",
    )
}

fn program(unix: &str, windows: &str) -> Vec<String> {
    if cfg!(windows) {
        vec![
            "pwsh.exe".into(),
            "-NoLogo".into(),
            "-NoProfile".into(),
            "-Command".into(),
            windows.into(),
        ]
    } else {
        vec!["/bin/sh".into(), "-c".into(), unix.into()]
    }
}
fn wait_screen(session: &Session, predicate: impl Fn(&Screen) -> bool) -> Screen {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let screen = session.screen();
        if predicate(&screen) {
            return screen;
        }
        assert!(
            Instant::now() < deadline,
            "Native PTY timeout: {:?}, {}",
            screen.error,
            screen.text()
        );
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn native_pty_colors_unicode_and_resize() {
    let command = program(
        "stty -echo; printf 'READY\\r\\n'; read line; printf 'SIZE:'; stty size; printf '\\033[31mRED\\033[0m λ 漢字 🙂\\r\\n'",
        "[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false); [Console]::WriteLine('READY'); $null=[Console]::ReadLine(); [Console]::WriteLine('SIZE:'+ [Console]::WindowHeight +' '+ [Console]::WindowWidth); [Console]::Write(([string][char]27)+'[31mRED'+([string][char]27)+'[0m λ 漢字 🙂'+\"`r`n\")",
    );
    let session = Session::spawn(&command, || {}).unwrap();
    wait_screen(&session, |screen| screen.text().contains("READY"));
    session.resize(42, 17, 8.0, 17.0).unwrap();
    session.write(b"continue\r").unwrap();
    let screen = wait_screen(&session, |screen| screen.text().contains("漢字 🙂"));
    assert!(
        screen.text().contains("SIZE:17 42"),
        "Native resize not reflected: {}",
        screen.text()
    );
    let red = screen
        .cells
        .windows(3)
        .find(|cells| cells[0].text == "R" && cells[1].text == "E" && cells[2].text == "D")
        .unwrap();
    assert_eq!(red[0].fg, opterm::terminal::palette(1));
    assert!(
        screen
            .cells
            .iter()
            .any(|cell| cell.text == "漢" && cell.wide)
    );
    assert!(
        screen
            .cells
            .iter()
            .all(|cell| cell.col < 42 && cell.row < 17)
    );
    assert!(screen.error.is_none());
}

#[test]
fn native_ctrl_c_interrupts_a_foreground_command() {
    #[cfg(windows)]
    {
        // Simulate an inheritable ignore flag left by a console launcher or
        // CREATE_NEW_PROCESS_GROUP. Session launch must clear it for the child.
        // SAFETY: this null handler changes only the test process's attribute.
        unsafe {
            windows_sys::Win32::System::Console::SetConsoleCtrlHandler(None, 1);
        }
    }
    let command = interrupt_program();
    let session = Session::spawn(&command, || {}).unwrap();
    wait_screen(&session, |screen| screen.text().contains("READY"));
    let started = Instant::now();
    session.write(b"\x03").unwrap();
    wait_screen(&session, |screen| screen.text().contains("INTERRUPTED"));
    assert!(
        started.elapsed() < TIMEOUT,
        "Ctrl+C did not interrupt the child"
    );
}

struct Service {
    directory: tempfile::TempDir,
    child: Child,
}
impl Service {
    fn start() -> Self {
        let directory = tempfile::Builder::new()
            .prefix("opterm-native-")
            .tempdir()
            .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let mut command = Command::new(env!("CARGO_BIN_EXE_opterm"));
        command
            .arg("--session-service")
            .env("OPTERM_STATE_DIR", directory.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0000_0008); // Match the detached production service.
        }
        let child = command.spawn().unwrap();
        let service = Self { directory, child };
        service.connect();
        service
    }
    fn connect(&self) -> Connection {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Ok(connection) = Connection::connect(self.directory.path()) {
                return connection;
            }
            assert!(
                Instant::now() < deadline,
                "Native local transport startup timed out"
            );
            thread::sleep(Duration::from_millis(20));
        }
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
fn snapshot(reply: Reply) -> Snapshot {
    let Reply::Snapshot(snapshot) = reply else {
        panic!("Expected session snapshot");
    };
    *snapshot
}
fn wait_remote(connection: &mut Connection, id: u64, text: &str) -> Snapshot {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let current = snapshot(connection.call(Request::View { id, since: None }).unwrap());
        if current.screen.text().contains(text) {
            return current;
        }
        assert!(
            Instant::now() < deadline,
            "Remote native PTY timeout: {}",
            current.screen.text()
        );
        thread::sleep(Duration::from_millis(20));
    }
}
fn pid(screen: &Screen) -> String {
    let text = screen.text();
    let after = text
        .split("READY_PID:")
        .nth(1)
        .expect("Fixture did not report PID");
    after.chars().take_while(char::is_ascii_digit).collect()
}
fn no_output_files(path: &Path) -> bool {
    !path.join("history").exists()
        || std::fs::read_dir(path.join("history"))
            .unwrap()
            .next()
            .is_none()
}

#[test]
fn native_local_service_reattaches_same_process_and_state_without_recording() {
    let service = Service::start();
    let mut first = service.connect();
    assert!(first.search_supported && first.history_supported && first.profiles);
    let instance = first.instance.clone();
    // Accepted transports must stay open while the desktop is idle. On BSD
    // inherited O_NONBLOCK otherwise closes a peer immediately after Hello.
    thread::sleep(Duration::from_millis(100));
    first
        .call(Request::ConfigHistory(Options {
            enabled: false,
            ..Options::default()
        }))
        .unwrap();
    let command = program(
        "stty -echo; count=0; printf 'READY_PID:%s\\r\\n' $$; while read line; do count=$((count+1)); printf 'COUNT:%s:%s λ 漢字 🙂\\r\\n' $count \"$line\"; done",
        "[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false); $count=0; [Console]::WriteLine('READY_PID:'+$PID); while ($null -ne ($line=[Console]::ReadLine())) { $count++; [Console]::WriteLine('COUNT:'+$count+':'+$line+' λ 漢字 🙂') }",
    );
    let created = snapshot(first.call(Request::Create(command)).unwrap());
    let id = created.info.id;
    let ready = wait_remote(&mut first, id, "READY_PID:");
    let original_pid = pid(&ready.screen);
    assert!(!original_pid.is_empty());
    first
        .call(Request::Write {
            id,
            bytes: b"before\r".to_vec(),
        })
        .unwrap();
    wait_remote(&mut first, id, "COUNT:1:before λ 漢字 🙂");
    assert!(
        no_output_files(service.directory.path()),
        "Global opt-out must apply to initial bytes"
    );
    drop(first);
    let mut reopened = service.connect();
    assert_eq!(reopened.instance, instance);
    let deadline = Instant::now() + TIMEOUT;
    loop {
        let Reply::List(infos) = reopened.call(Request::List).unwrap() else {
            panic!("Expected session list");
        };
        if infos.iter().any(|info| info.id == id && !info.attached) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "Closed transport did not release session ownership"
        );
        thread::sleep(Duration::from_millis(20));
    }
    let attached = snapshot(reopened.call(Request::Attach(id)).unwrap());
    assert_eq!(pid(&attached.screen), original_pid);
    assert!(attached.screen.text().contains("COUNT:1:before"));
    assert!(!attached.info.recording);
    assert!(!attached.info.exited);
    let result = reopened
        .call(Request::Search {
            id,
            query: "漢字".into(),
            backwards: false,
            reset: true,
            token: 1,
        })
        .unwrap();
    assert!(
        matches!(result, Reply::Search { id: found_id, token: 1, found: true } if found_id == id)
    );
    let searched = snapshot(reopened.call(Request::View { id, since: None }).unwrap());
    assert_eq!(searched.selected_text.as_deref(), Some("漢字"));
    reopened
        .call(Request::Write {
            id,
            bytes: b"after\r".to_vec(),
        })
        .unwrap();
    let current = wait_remote(&mut reopened, id, "COUNT:2:after λ 漢字 🙂");
    assert_eq!(pid(&current.screen), original_pid);
    assert!(current.screen.error.is_none());
    assert!(no_output_files(service.directory.path()));
    let Reply::List(infos) = reopened.call(Request::List).unwrap() else {
        panic!("Expected session list");
    };
    assert_eq!(
        infos.len(),
        1,
        "Reattachment must not replace or duplicate work"
    );
    // Interruption must also work through the service's detached launch, whose
    // parent may have no console for SetConsoleCtrlHandler to reset.
    let interrupt = snapshot(reopened.call(Request::Create(interrupt_program())).unwrap());
    let interrupt_id = interrupt.info.id;
    wait_remote(&mut reopened, interrupt_id, "READY");
    let started = Instant::now();
    reopened
        .call(Request::Write {
            id: interrupt_id,
            bytes: b"\x03".to_vec(),
        })
        .unwrap();
    wait_remote(&mut reopened, interrupt_id, "INTERRUPTED");
    assert!(started.elapsed() < TIMEOUT);
}
