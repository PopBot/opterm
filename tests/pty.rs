//! These exercise an actual native PTY, not a mock process or parser-only fixture.
#![cfg(unix)]
use opterm::terminal::{Screen, Session};
use std::{
    thread,
    time::{Duration, Instant},
};

fn wait_for(session: &Session, predicate: impl Fn(&Screen) -> bool) -> Screen {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let screen = session.screen();
        if predicate(&screen) {
            return screen;
        }
        assert!(
            Instant::now() < deadline,
            "PTY timed out: {}",
            screen.text()
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn native_shell_title_color_and_resize() {
    let session = Session::spawn(&["/bin/sh".into(), "-c".into(),
        "stty -echo; printf 'READY\\n'; read line; stty size; printf '\\033]0;PTY fixture\\007\\033[31mRED\\033[0m\\n'; printf 'DONE\\n'".into()], || {}).unwrap();
    wait_for(&session, |s| s.text().contains("READY"));
    session.resize(91, 27, 8.0, 17.0).unwrap();
    session.write(b"continue\r").unwrap();
    let screen = wait_for(&session, |s| s.text().contains("DONE"));
    assert!(screen.text().contains("27 91"));
    assert_eq!(screen.title, "PTY fixture");
    let red = &screen
        .cells
        .windows(3)
        .find(|cells| cells[0].text == "R" && cells[1].text == "E" && cells[2].text == "D")
        .unwrap()[0];
    assert_eq!(red.fg, opterm::terminal::palette(1));
    wait_for(&session, |s| s.exited);
}

#[test]
fn alternate_screen_restores_main_screen() {
    let session = Session::spawn(&["/bin/sh".into(), "-c".into(),
        "printf 'MAIN\\r\\n\\033[?1049hALT\\r\\n'; sleep 0.2; printf '\\033[?1049lRESTORED\\r\\n'".into()], || {}).unwrap();
    wait_for(&session, |s| s.text().contains("ALT"));
    let screen = wait_for(&session, |s| s.text().contains("RESTORED"));
    assert!(screen.text().contains("MAIN"));
    assert!(!screen.text().contains("ALT"));
}

#[test]
fn paste_adds_no_enter_and_preserves_bracketed_framing() {
    let session = Session::spawn(&["/bin/sh".into(), "-c".into(),
        "stty raw -echo; printf '\\033[?2004hREADY'; dd bs=1 count=15 2>/dev/null | od -An -tx1".into()], || {}).unwrap();
    wait_for(&session, |s| s.text().contains("READY"));
    session.paste("a\nb").unwrap();
    let screen = wait_for(&session, |s| s.text().contains("1b 5b 32 30 31 7e"));
    assert!(
        screen
            .text()
            .contains("1b 5b 32 30 30 7e 61 0a 62 1b 5b 32 30 31 7e")
    );
}

#[test]
fn terminal_status_queries_receive_replies() {
    let session = Session::spawn(
        &[
            "/bin/sh".into(),
            "-c".into(),
            "stty raw -echo; printf '\\033[6n'; dd bs=1 count=6 2>/dev/null | od -An -tx1".into(),
        ],
        || {},
    )
    .unwrap();
    wait_for(&session, |s| s.text().contains("1b 5b 31 3b 31 52"));
}

#[test]
#[ignore = "requires locally installed Codex and Claude CLIs; performs only help launches"]
fn existing_agent_clis_start_in_native_pty() {
    for program in ["codex", "claude"] {
        let session = Session::spawn(&[program.into(), "--help".into()], || {}).unwrap();
        wait_for(&session, |s| s.exited);
        session.scroll(i32::MAX);
        let screen = wait_for(&session, |s| s.text().contains("Usage"));
        assert!(screen.error.is_none(), "{program}: {:?}", screen.error);
    }
}

#[test]
fn profile_theme_queries_match_theme_and_program_overrides() {
    use opterm::profiles::{Launch, Theme};
    let mut launch = Launch::legacy(&[]);
    launch.theme = Theme::light();
    launch.profile.executable = "/bin/sh".into();
    let reply = "\x1b]11;rgb:f8f8/f8f8/f9f9\x1b\\";
    let script = format!(
        r"stty raw -echo; printf '\033]11;?\033\\'; dd bs=1 count={} 2>/dev/null | od -An -tx1; printf '\033]11;rgb:01/02/03\007\033]11;?\033\\'; dd bs=1 count={} 2>/dev/null | od -An -tx1",
        reply.len(),
        reply.len()
    );
    launch.profile.arguments = vec!["-c".into(), script];
    let session = Session::spawn_configured(&launch, || {}).unwrap();
    let screen = wait_for(&session, |s| s.exited);
    let hex: String = screen
        .text()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    for expected in [reply, "\x1b]11;rgb:0101/0202/0303\x1b\\"] {
        let expected: String = expected
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert!(hex.contains(&expected), "Missing {expected}: {hex}");
    }
    assert!(screen.error.is_none());
}
