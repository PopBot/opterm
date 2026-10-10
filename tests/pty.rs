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
#[cfg(target_os = "linux")]
fn clicks_edit_real_readline_input_and_leave_prompt_output_and_stale_views_alone() {
    use opterm::profiles::{Environment, Launch};
    let mut launch = Launch::legacy(&[
        "/bin/bash".into(),
        "--noprofile".into(),
        "--norc".into(),
        "-i".into(),
    ]);
    launch.profile.environment = vec![
        Environment {
            name: "PS1".into(),
            value: "edit> ".into(),
            secret: false,
        },
        Environment {
            name: "LC_ALL".into(),
            value: "C.UTF-8".into(),
            secret: false,
        },
    ];
    let session = Session::spawn_configured(&launch, || {}).unwrap();
    wait_for(&session, |s| s.text().contains("edit> "));
    session.write(b"printf 'RESULT:%s\\n' abcdef").unwrap();
    let screen = wait_for(&session, |s| s.text().contains("abcdef"));
    let (row, col) = screen.cursor.unwrap();
    for (target_row, target_col, expected) in [
        (row + 1, col - 3, screen.cursor),
        (row, 1, screen.cursor),
        (row, col - 3, Some((row, col - 1))),
    ] {
        session
            .click_edit(target_row, target_col, expected)
            .unwrap();
        thread::sleep(Duration::from_millis(30));
        assert_eq!(session.screen().cursor, screen.cursor);
    }
    session.click_edit(row, col - 3, screen.cursor).unwrap();
    wait_for(&session, |s| s.cursor == Some((row, col - 3)));
    session.write(b"X").unwrap();
    let moved = wait_for(&session, |s| s.text().contains("abcXdef"));
    // Move back to the end by clicking, then insert again and execute.
    session.click_edit(row, col + 1, moved.cursor).unwrap();
    wait_for(&session, |s| s.cursor == Some((row, col + 1)));
    session.write(b"Y\r").unwrap();
    wait_for(&session, |s| s.text().contains("RESULT:abcXdefY"));
    wait_for(&session, |s| {
        s.cursor.is_some_and(|(r, c)| r > row && c == 6)
    });
    session
        .write("printf 'WIDE:%s\\n' a界bc".as_bytes())
        .unwrap();
    let screen = wait_for(&session, |s| s.text().contains("a界bc"));
    let (row, col) = screen.cursor.unwrap();
    // The second display cell of 界 still targets its character boundary.
    session.click_edit(row, col - 3, screen.cursor).unwrap();
    wait_for(&session, |s| s.cursor == Some((row, col - 4)));
    session.write(b"X\x1b[F\r").unwrap();
    wait_for(&session, |s| s.text().contains("WIDE:aX界bc"));
    wait_for(&session, |s| s.cursor.is_some_and(|(_, c)| c == 6));
    session
        .write("printf 'COMBINING:%s\\n' ae\u{301}bc".as_bytes())
        .unwrap();
    let screen = wait_for(&session, |s| s.text().contains("ae\u{301}bc"));
    let (row, col) = screen.cursor.unwrap();
    session.click_edit(row, col - 3, screen.cursor).unwrap();
    wait_for(&session, |s| s.cursor == Some((row, col - 3)));
    session.write(b"X\x1b[F\r").unwrap();
    wait_for(&session, |s| s.text().contains("COMBINING:aXe\u{301}bc"));
    wait_for(&session, |s| s.cursor.is_some_and(|(_, c)| c == 6));
    session.resize(32, 4, 8.0, 17.0).unwrap();
    session
        .write(b"printf '\\033[2J\\033[HWRAPREADY\\n\\n\\n'\r")
        .unwrap();
    // Resize can leave the previous prompt at the same cursor coordinates.
    // Wait for the clear command and its fresh Readline prompt, rather than
    // sending wrapped input while that foreground command is still running.
    wait_for(&session, |s| {
        s.cursor == Some((3, 6)) && s.text().starts_with("WRAPREADY")
    });
    let prefix = "printf 'WRAPPED:%s\\n' ";
    let value = "abcdef0123456789012345678901234567890123456789";
    session
        .write(format!("{prefix}{value}").as_bytes())
        .unwrap();
    let screen = wait_for(&session, |s| s.text().replace('\n', "").contains(value));
    let (row, _) = screen.cursor.unwrap();
    let prompt_row = screen
        .cells
        .windows(5)
        .rev()
        .find(|cells| cells.iter().map(|c| c.text.as_str()).collect::<String>() == "edit>")
        .unwrap()[0]
        .row;
    let target_row = row - 1;
    let insertion = target_row * 32 + 2 - (prompt_row * 32 + 6 + prefix.len());
    session.click_edit(target_row, 2, screen.cursor).unwrap();
    wait_for(&session, |s| s.cursor == Some((target_row, 2)));
    session.write(b"X\x1b[F\r").unwrap();
    let expected = format!("WRAPPED:{}X{}", &value[..insertion], &value[insertion..]);
    wait_for(&session, |s| s.text().replace('\n', "").contains(&expected));
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
fn paste_preserves_exact_bytes_with_and_without_bracketed_framing() {
    let text = "a\r\nb\nc\rλ\t";
    for bracketed in [false, true] {
        let mut expected = Vec::new();
        if bracketed {
            expected.extend_from_slice(b"\x1b[200~");
        }
        expected.extend_from_slice(text.as_bytes());
        if bracketed {
            expected.extend_from_slice(b"\x1b[201~");
        }
        let mode = if bracketed { "\\033[?2004h" } else { "" };
        let script = format!(
            "stty raw -echo; printf '{mode}READY'; dd bs=1 count={} 2>/dev/null | od -An -tx1; printf 'DONE'",
            expected.len()
        );
        let session = Session::spawn(&["/bin/sh".into(), "-c".into(), script], || {}).unwrap();
        wait_for(&session, |s| s.text().contains("READY"));
        session.paste(text).unwrap();
        let screen = wait_for(&session, |s| s.text().contains("DONE"));
        let actual: String = screen
            .text()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let hex: String = expected.iter().map(|byte| format!("{byte:02x}")).collect();
        assert!(actual.contains(&hex), "bracketed={bracketed}: {actual}");
    }
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
    // BSD and GNU od use different spacing for the same byte sequence.
    wait_for(&session, |s| {
        s.text()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .contains("1b5b313b3152")
    });
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
