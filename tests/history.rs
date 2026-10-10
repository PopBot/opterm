//! Saved output is isolated from live shells and disposable cache.
#![cfg(unix)]
use opterm::{
    history::{MAX_HISTORY_CELLS, Options, Recorder, cleanup},
    profiles::Launch,
    terminal::{Screen, Session},
};
use std::{
    fs::{self, FileTimes, OpenOptions},
    os::unix::fs::PermissionsExt,
    path::Path,
    thread,
    time::{Duration, Instant, SystemTime},
};

fn wait(predicate: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !predicate() {
        assert!(Instant::now() < deadline, "Saved output timed out");
        thread::sleep(Duration::from_millis(10));
    }
}
fn contents(directory: &Path) -> Vec<u8> {
    fs::read_dir(directory)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "ansi")
        })
        .flat_map(|entry| fs::read(entry.path()).unwrap())
        .collect()
}
fn shell(directory: &Path, options: Options) -> Session {
    let mut launch = Launch::legacy(&[]);
    launch.profile.executable = "/bin/sh".into();
    launch.profile.arguments = vec!["-c".into(), "stty -echo; printf '\\033[31mFIRST\\033[0m PID:%s\\r\\n' $$; while read line; do printf '%s\\r\\n' \"$line\"; done".into()];
    Session::spawn_with_history(&launch, directory.to_owned(), options, || {}).unwrap()
}
fn wait_text(session: &Session, text: &str) -> Screen {
    wait(|| session.screen().text().contains(text));
    session.screen()
}
#[test]
fn pause_resume_keeps_shell_alive_and_preserves_raw_output_privately() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("history");
    let session = shell(&directory, Options::default());
    let first = wait_text(&session, "FIRST").text();
    wait(|| {
        contents(&directory)
            .windows(5)
            .any(|window| window == b"FIRST")
    });
    session.set_recording(false);
    session.write(b"PAUSED\r").unwrap();
    wait_text(&session, "PAUSED");
    session.configure_history(Options {
        retention_days: 31,
        ..Options::default()
    });
    assert!(
        !session.recording(),
        "Retention edits must preserve a manual pause"
    );
    session.set_recording(true);
    session.write(b"RESUMED\r").unwrap();
    wait_text(&session, "RESUMED");
    wait(|| {
        contents(&directory)
            .windows(7)
            .any(|window| window == b"RESUMED")
    });
    let output = contents(&directory);
    assert!(output.windows(10).any(|window| window == b"\x1b[31mFIRST"));
    assert!(!output.windows(6).any(|window| window == b"PAUSED"));
    assert!(
        session.screen().text().contains(first.trim()),
        "Original PID/output survives recording changes"
    );
    assert!(!session.exited());
    assert_eq!(
        fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
        0o700
    );
    for entry in fs::read_dir(&directory).unwrap().flatten() {
        assert_eq!(
            entry.metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
#[test]
fn startup_opt_out_and_failed_disk_write_leave_live_work_usable() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("disabled");
    let session = shell(
        &directory,
        Options {
            enabled: false,
            ..Options::default()
        },
    );
    wait_text(&session, "FIRST");
    assert!(
        !directory.exists(),
        "Disabled capture must not write initial output"
    );
    session.set_recording(true);
    assert!(
        !session.recording(),
        "A session cannot override global saved output off"
    );
    session.write(b"RUNNING\r").unwrap();
    wait_text(&session, "RUNNING");
    assert!(session.history_error().is_none());
    let blocked = temp.path().join("blocked");
    fs::write(&blocked, "file prevents directory creation").unwrap();
    let failed = shell(&blocked, Options::default());
    wait_text(&failed, "FIRST");
    wait(|| failed.history_error().is_some());
    let error = failed.history_error().unwrap();
    assert!(error.contains("Live work continues"));
    assert!(error.contains("gap"));
    assert!(!failed.recording());
    failed.write(b"STILL_RUNNING\r").unwrap();
    wait_text(&failed, "STILL_RUNNING");
    assert!(!failed.exited());
    assert!(failed.screen().error.is_none());
}
fn saved(directory: &Path, name: &str, size: usize, age_days: u64) {
    let path = directory.join(format!("output-{name}.ansi"));
    fs::write(&path, vec![b'x'; size]).unwrap();
    let old = SystemTime::now() - Duration::from_secs(age_days * 86400);
    OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(FileTimes::new().set_modified(old))
        .unwrap();
}
#[test]
fn retention_removes_expired_then_oldest_output_and_preserves_other_files() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("history");
    cleanup(&directory, &Options::default()).unwrap();
    saved(&directory, "expired", 1, 31);
    saved(&directory, "older", 600 * 1024, 2);
    saved(&directory, "newer", 600 * 1024, 1);
    fs::write(directory.join("settings-last-valid.toml"), b"keep").unwrap();
    cleanup(
        &directory,
        &Options {
            total_mib: 1,
            stream_mib: 1,
            ..Options::default()
        },
    )
    .unwrap();
    assert!(!directory.join("output-expired.ansi").exists());
    assert!(!directory.join("output-older.ansi").exists());
    assert!(directory.join("output-newer.ansi").exists());
    assert_eq!(
        fs::read(directory.join("settings-last-valid.toml")).unwrap(),
        b"keep"
    );
}
#[test]
fn stream_limit_reports_gap_and_live_scrollback_has_a_cell_budget() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("history");
    let options = Options {
        stream_mib: 1,
        total_mib: 2,
        ..Options::default()
    };
    let recorder = Recorder::new(directory.clone(), options.clone());
    recorder.record(&vec![b'x'; 900 * 1024]);
    wait(|| contents(&directory).len() == 900 * 1024);
    recorder.record(&vec![b'y'; 256 * 1024]);
    wait(|| recorder.error().is_some());
    assert!(recorder.error().unwrap().contains("gap"));
    assert!(contents(&directory).len() <= 1024 * 1024);
    assert!(!recorder.enabled());
    for cols in [2, 100, 500] {
        assert!(options.lines(cols) * cols <= MAX_HISTORY_CELLS);
    }
    assert_eq!(options.lines(500), 2000);
    assert!(
        Options {
            scrollback_lines: 100_001,
            ..options.clone()
        }
        .validate()
        .is_err()
    );
}

#[test]
fn client_applies_global_opt_out_before_creating_a_shell() {
    use opterm::service::{
        Connection, Request,
        client::{Client, Reconnect},
    };
    use std::process::{Child, Command, Stdio};
    struct Guard {
        directory: tempfile::TempDir,
        child: Child,
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            if let Ok(mut connection) = Connection::connect(self.directory.path()) {
                let _ = connection.call(Request::Shutdown);
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
    let directory = tempfile::tempdir().unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let child = Command::new(env!("CARGO_BIN_EXE_opterm"))
        .arg("--session-service")
        .env("OPTERM_STATE_DIR", directory.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let guard = Guard { directory, child };
    wait(|| Connection::connect(guard.directory.path()).is_ok());
    let mut launch = Launch::legacy(&[]);
    launch.profile.executable = "/bin/sh".into();
    launch.profile.arguments = vec!["-c".into(), "stty -echo; printf 'PRIVATE_FIRST\\r\\n'; while read line; do printf '%s\\r\\n' \"$line\"; done".into()];
    let options = Options {
        enabled: false,
        ..Options::default()
    };
    let client = Client::open_with_history(
        guard.directory.path(),
        launch,
        false,
        Reconnect::Fresh,
        true,
        Some(&options),
        || {},
    )
    .unwrap();
    assert!(client.history_supported);
    let session = client.session(client.infos()[0].id);
    wait(|| session.screen().text().contains("PRIVATE_FIRST"));
    assert!(!guard.directory.path().join("history").exists());
    session.recording(true).unwrap();
    session.write(b"PRIVATE_SECOND\r").unwrap();
    wait(|| session.screen().text().contains("PRIVATE_SECOND"));
    assert!(!guard.directory.path().join("history").exists());
    client.configure_history(&Options::default()).unwrap();
    session.write(b"PUBLIC_THIRD\r").unwrap();
    wait(|| {
        contents(&guard.directory.path().join("history"))
            .windows(12)
            .any(|window| window == b"PUBLIC_THIRD")
    });
    let output = contents(&guard.directory.path().join("history"));
    assert!(!output.windows(7).any(|window| window == b"PRIVATE"));
}
