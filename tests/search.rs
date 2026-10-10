//! Real PTY search over the bounded live terminal grid, plus asynchronous IPC.
#![cfg(unix)]
use opterm::{
    profiles::Launch,
    service::{
        Connection, Reply, Request,
        client::{Client, Reconnect, SearchResult},
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
        assert!(Instant::now() < deadline, "Timed out: {}", screen.text());
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn literal_search_visits_offscreen_matches_in_both_directions_and_wraps() {
    let script = "printf 'TOP a+b[0] λ\\r\\n'; i=0; while [ $i -lt 120 ]; do if [ $i -eq 40 ]; then printf 'MID a+b[0] λ\\r\\n'; elif [ $i -eq 80 ]; then printf 'BOTTOM a+b[0] λ\\r\\n'; else printf 'filler %s\\r\\n' $i; fi; i=$((i+1)); done";
    let session = Session::spawn(&["/bin/sh".into(), "-c".into(), script.into()], || {}).unwrap();
    wait_for(&session, |screen| screen.exited);
    assert!(!session.screen().text().contains("a+b"));
    let query = "a+b[0] λ";
    for (backwards, reset, label) in [
        (true, true, "BOTTOM"),
        (true, false, "MID"),
        (true, false, "TOP"),
        (true, false, "BOTTOM"),
        (false, false, "TOP"),
        (false, false, "MID"),
        (false, false, "BOTTOM"),
    ] {
        assert!(session.search(query, backwards, reset).unwrap());
        assert_eq!(session.selected_text().as_deref(), Some(query));
        assert!(session.screen().text().contains(label), "Missing {label}");
        assert!(session.screen().cells.iter().any(|cell| cell.selected));
    }
    assert!(
        !session.search("A+B[0] λ", false, true).unwrap(),
        "Search must be case-sensitive"
    );
    assert!(session.selected_text().is_none());
    assert!(!session.search("", false, true).unwrap());
    assert!(session.search(&"x".repeat(4097), false, true).is_err());
}

#[test]
fn literal_search_crosses_soft_wrapping_and_missing_query_preserves_process() {
    let session = Session::spawn(&["/bin/sh".into(), "-c".into(), "stty -echo; printf 'READY'; read start; printf '\\r\\ncross-boundary\\r\\n'; read finish; printf 'STILL ALIVE'".into()], || {}).unwrap();
    wait_for(&session, |screen| screen.text().contains("READY"));
    session.resize(10, 5, 8.0, 17.0).unwrap();
    session.write(b"start\r").unwrap();
    wait_for(&session, |screen| screen.text().contains("dary"));
    assert!(session.search("cross-boundary", false, true).unwrap());
    assert_eq!(session.selected_text().as_deref(), Some("cross-boundary"));
    assert!(!session.search("absent", true, false).unwrap());
    assert!(!session.exited());
    session.resize(100, 32, 8.0, 17.0).unwrap();
    session.write(b"finish\r").unwrap();
    wait_for(&session, |screen| screen.text().contains("STILL ALIVE"));
}

struct Service {
    directory: TempDir,
    child: Child,
}
impl Service {
    fn start() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::Builder::new()
            .prefix("opterm-search-")
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
fn wait_search(client: &Client, token: u64) -> SearchResult {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        for result in client.take_search_results() {
            if result.token == token {
                return result;
            }
        }
        assert!(Instant::now() < deadline, "Search result timeout");
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn service_search_reports_no_match_and_control_errors_without_global_errors() {
    let service = Service::start();
    let mut launch = Launch::legacy(&[]);
    launch.profile.executable = "/bin/sh".into();
    launch.profile.arguments = vec!["-c".into(), "printf 'target'; sleep 10".into()];
    let client = Client::open_with(
        service.directory.path(),
        launch.clone(),
        false,
        Reconnect::Fresh,
        true,
        || {},
    )
    .unwrap();
    assert!(client.search_supported);
    let id = client.infos()[0].id;
    let fresh = Client::open_with(
        service.directory.path(),
        launch.clone(),
        false,
        Reconnect::Fresh,
        false,
        || {},
    )
    .unwrap();
    assert_eq!(
        fresh.infos().iter().map(|info| info.id).collect::<Vec<_>>(),
        vec![id]
    );
    assert!(fresh.view_updates().0.is_empty());
    let session = client.session(id);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !session.screen().text().contains("target") {
        assert!(Instant::now() < deadline, "PTY startup timeout");
        thread::sleep(Duration::from_millis(10));
    }
    client.search(id, "target", false, true, 1).unwrap();
    let result = wait_search(&client, 1);
    assert_eq!(result.id, id);
    assert!(result.result.unwrap());
    client.search(id, "absent", true, true, 2).unwrap();
    assert!(!wait_search(&client, 2).result.unwrap());
    assert!(client.take_error().is_none());
    let other = Client::open_with(
        service.directory.path(),
        launch,
        false,
        Reconnect::All,
        false,
        || {},
    )
    .unwrap();
    other.search(id, "target", false, true, 3).unwrap();
    assert!(
        wait_search(&other, 3)
            .result
            .unwrap_err()
            .contains("controlled in another window")
    );
    assert!(other.take_error().is_none());
    client.search(id, "target", false, true, 4).unwrap();
    assert!(wait_search(&client, 4).result.unwrap());
}

#[test]
fn older_hello_deserializes_without_search_capability() {
    let reply: Reply = serde_json::from_str(
        r#"{"Hello":{"version":1,"pid":1,"profiles":true,"instance":"legacy"}}"#,
    )
    .unwrap();
    assert!(matches!(reply, Reply::Hello { search: false, .. }));
}
