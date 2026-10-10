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
    // Child exit can precede the reader draining the final PTY output.
    wait_for(&session, |screen| {
        screen.exited && screen.text().contains("filler 119")
    });
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

#[test]
fn enhanced_search_counts_navigates_and_preserves_manual_selection() {
    use opterm::search::{Case, Mode, Options};
    let session = Session::spawn(
        &[
            "/bin/sh".into(),
            "-c".into(),
            "printf 'Foo foo FOO selected\\r\\n'; sleep 10".into(),
        ],
        || {},
    )
    .unwrap();
    wait_for(&session, |screen| screen.text().contains("selected"));
    session.select(0, 12, true);
    session.select(0, 19, false);
    let selected = session.selected_text();
    assert_eq!(selected.as_deref(), Some("selected"));
    let options = Options {
        mode: Mode::Literal,
        case: Case::Insensitive,
    };
    for ordinal in [1, 2, 3, 1] {
        let result = session
            .search_advanced(
                "foo",
                options,
                false,
                ordinal == 1 && session.screen().cells.iter().all(|cell| !cell.search_match),
            )
            .unwrap();
        assert_eq!(result.count, 3);
        assert_eq!(result.current, Some(ordinal));
        assert!(!result.truncated);
        assert_eq!(session.selected_text(), selected);
        let screen = session.screen();
        assert_eq!(
            screen.cells.iter().filter(|cell| cell.search_match).count(),
            9
        );
        assert_eq!(
            screen
                .cells
                .iter()
                .filter(|cell| cell.search_current)
                .count(),
            3
        );
        assert_eq!(screen.cells.iter().filter(|cell| cell.selected).count(), 8);
    }
    let previous = session
        .search_advanced("foo", options, true, false)
        .unwrap();
    assert_eq!(previous.current, Some(3));
    let result = session
        .search_advanced(
            "F..",
            Options {
                mode: Mode::Regex,
                case: Case::Sensitive,
            },
            false,
            true,
        )
        .unwrap();
    assert_eq!(result.count, 2);
    assert!(session.screen().cells.iter().any(|cell| cell.search_match));
    let generation = session
        .generation
        .load(std::sync::atomic::Ordering::Relaxed);
    assert!(
        session
            .search_advanced(
                "[",
                Options {
                    mode: Mode::Regex,
                    case: Case::Sensitive
                },
                false,
                true
            )
            .is_err()
    );
    assert_eq!(session.selected_text(), selected);
    assert!(
        session
            .screen()
            .cells
            .iter()
            .all(|cell| !cell.search_match && !cell.search_current)
    );
    assert!(
        session
            .generation
            .load(std::sync::atomic::Ordering::Relaxed)
            > generation
    );
    assert_eq!(
        session
            .search_advanced("", options, false, true)
            .unwrap()
            .count,
        0
    );
    assert!(session.screen().cells.iter().all(|cell| !cell.search_match));
    assert_eq!(session.selected_text(), selected);
}

fn wait_advanced(client: &Client, token: u64) -> opterm::service::client::AdvancedSearchResult {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        for result in client.take_advanced_search_results() {
            if result.token == token {
                return result;
            }
        }
        assert!(Instant::now() < deadline, "Enhanced search result timeout");
        thread::sleep(Duration::from_millis(10));
    }
}
fn wait_all(client: &Client, token: u64) -> opterm::search::AllSummary {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        for result in client.take_all_search_results() {
            if result.token == token {
                return result.result.unwrap();
            }
        }
        assert!(
            Instant::now() < deadline,
            "All-session search result timeout"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn enhanced_ipc_errors_are_local_and_all_session_scan_does_not_take_control() {
    use opterm::search::{Case, Mode, Options};
    let service = Service::start();
    let mut owner = Connection::connect(service.directory.path()).unwrap();
    assert!(owner.advanced_search_supported);
    assert!(owner.smart_selection_supported);
    let program = vec![
        "/bin/sh".into(),
        "-c".into(),
        "printf 'target TARGET target\\r\\n'; sleep 10".into(),
    ];
    let mut ids = Vec::new();
    for _ in 0..2 {
        let Reply::Snapshot(snapshot) = owner.call(Request::Create(program.clone())).unwrap()
        else {
            panic!("Expected new session");
        };
        ids.push(snapshot.info.id);
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    for id in &ids {
        loop {
            let Reply::Snapshot(snapshot) = owner
                .call(Request::View {
                    id: *id,
                    since: None,
                })
                .unwrap()
            else {
                panic!("Expected snapshot");
            };
            if snapshot.screen.text().contains("target TARGET target") {
                break;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(10));
        }
    }
    owner
        .call(Request::SelectMode {
            id: ids[0],
            row: 0,
            col: 1,
            start: true,
            mode: opterm::selection::SelectionMode::Semantic,
        })
        .unwrap();
    let Reply::Snapshot(before) = owner
        .call(Request::View {
            id: ids[0],
            since: None,
        })
        .unwrap()
    else {
        panic!("Expected snapshot");
    };
    assert_eq!(before.selected_text.as_deref(), Some("target"));
    owner.call(Request::Detach(ids[1])).unwrap();
    let observer = Client::open_with(
        service.directory.path(),
        Launch::legacy(&[]),
        false,
        Reconnect::Fresh,
        false,
        || {},
    )
    .unwrap();
    assert!(observer.view_updates().0.is_empty());
    observer
        .search_all(
            "target",
            Options {
                case: Case::Insensitive,
                mode: Mode::Literal,
            },
            50,
        )
        .unwrap();
    let results = wait_all(&observer, 50);
    assert!(!results.truncated);
    assert_eq!(
        results
            .sessions
            .iter()
            .map(|session| session.id)
            .collect::<Vec<_>>(),
        ids
    );
    assert!(
        results
            .sessions
            .iter()
            .all(|session| session.summary.count == 3
                && session.summary.current.is_none()
                && session.preview.contains("TARGET"))
    );
    let Reply::Snapshot(after) = owner
        .call(Request::View {
            id: ids[0],
            since: None,
        })
        .unwrap()
    else {
        panic!("Expected snapshot");
    };
    assert_eq!(after.selected_text, before.selected_text);
    assert_eq!(after.screen.text(), before.screen.text());
    assert_eq!(after.revision, before.revision);
    assert!(after.info.controlled);
    let Reply::List(infos) = owner.call(Request::List).unwrap() else {
        panic!("Expected list");
    };
    assert!(
        !infos
            .iter()
            .find(|info| info.id == ids[1])
            .unwrap()
            .attached
    );
    assert!(observer.view_updates().0.is_empty());
    observer
        .search_advanced(ids[0], "target", Options::default(), false, true, 51)
        .unwrap();
    assert!(
        wait_advanced(&observer, 51)
            .result
            .unwrap_err()
            .contains("controlled in another window")
    );
    observer.send(Request::Attach(ids[0])).unwrap();
    // FIFO guarantees attachment finishes before search; it must not steal ownership.
    observer
        .search_advanced(ids[0], "target", Options::default(), false, true, 52)
        .unwrap();
    assert!(
        wait_advanced(&observer, 52)
            .result
            .unwrap_err()
            .contains("controlled in another window")
    );
    // A controlled client reports invalid regex as a request-scoped asynchronous error.
    let controller = Client::open_with(
        service.directory.path(),
        Launch::legacy(&program),
        true,
        Reconnect::Fresh,
        true,
        || {},
    )
    .unwrap();
    let id = controller
        .infos()
        .iter()
        .find(|info| info.controlled)
        .unwrap()
        .id;
    controller
        .search_advanced(
            id,
            "[",
            Options {
                mode: Mode::Regex,
                case: Case::Sensitive,
            },
            false,
            true,
            53,
        )
        .unwrap();
    assert!(
        wait_advanced(&controller, 53)
            .result
            .unwrap_err()
            .contains("Invalid search expression")
    );
    assert!(controller.take_error().is_none());
    assert!(observer.take_error().is_none());
}

#[test]
fn regex_navigation_distinguishes_combining_char_matches_in_the_same_cell() {
    use opterm::search::{Case, Mode, Options};
    let session = Session::spawn(
        &["/bin/sh".into(), "-c".into(), "printf 'é'; sleep 10".into()],
        || {},
    )
    .unwrap();
    wait_for(&session, |screen| screen.text().contains("é"));
    let options = Options {
        mode: Mode::Regex,
        case: Case::Sensitive,
    };
    for (reset, ordinal) in [(true, 1), (false, 2), (false, 1)] {
        let summary = session
            .search_advanced("e|\\p{M}", options, false, reset)
            .unwrap();
        assert_eq!(summary.count, 2);
        assert_eq!(summary.current, Some(ordinal));
    }
}

#[test]
fn ipc_snapshot_preserves_search_and_osc8_spaces_and_wrapped_gaps() {
    use opterm::search::Options;
    let service = Service::start();
    let mut owner = Connection::connect(service.directory.path()).unwrap();
    let script = "printf 'a b\\r\\n'; printf '\\033]8;;https://example.com/blank\\033\\\\ \\033]8;;\\033\\\\\\r\\n'; printf '%-100s' 'https://example.com/'; printf 'suffix\\r\\n'; sleep 10";
    let Reply::Snapshot(created) = owner
        .call(Request::Create(vec![
            "/bin/sh".into(),
            "-c".into(),
            script.into(),
        ]))
        .unwrap()
    else {
        panic!("Expected snapshot");
    };
    let id = created.info.id;
    let deadline = Instant::now() + Duration::from_secs(5);
    let screen = loop {
        let Reply::Snapshot(snapshot) = owner.call(Request::View { id, since: None }).unwrap()
        else {
            panic!("Expected snapshot");
        };
        if snapshot.screen.text().contains("suffix") {
            break snapshot.screen;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(10));
    };
    let blank_link = screen
        .cells
        .iter()
        .find(|cell| cell.hyperlink.as_deref() == Some("https://example.com/blank"))
        .expect("OSC8 blank label must survive sparse IPC");
    assert_eq!(blank_link.text, " ");
    assert!(screen.wrapped_rows.contains(&2));
    assert!(
        screen
            .cells
            .iter()
            .any(|cell| cell.row == 2 && cell.col == 99 && cell.text == " ")
    );
    let link = opterm::links::link_at(&screen, 2, 5, None).unwrap();
    assert_eq!(link.target, "https://example.com/");
    owner
        .call(Request::SearchAdvanced {
            id,
            query: "a b".into(),
            options: Options::default(),
            backwards: false,
            reset: true,
            token: 1,
        })
        .unwrap();
    let Reply::Snapshot(snapshot) = owner.call(Request::View { id, since: None }).unwrap() else {
        panic!("Expected snapshot");
    };
    assert!(snapshot.screen.cells.iter().any(|cell| cell.row == 0
        && cell.col == 1
        && cell.text == " "
        && cell.search_match
        && cell.search_current));
}
