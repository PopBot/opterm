use alacritty_terminal::term::TermMode;
use eframe::egui::{Event, Key, Modifiers};

// Clipboard events omit their modifiers. A matching key-release in the same
// batch retains them even when the modifier has been released before repaint.
pub fn clipboard_modifiers(
    events: &[Event],
    index: usize,
    key: Key,
    fallback: Modifiers,
) -> Modifiers {
    events[index + 1..]
        .iter()
        .find_map(|event| match event {
            Event::Key {
                key: released,
                pressed: false,
                modifiers,
                ..
            } if *released == key => Some(*modifiers),
            _ => None,
        })
        .unwrap_or(fallback)
}

/// Encode the conventional xterm key protocol advertised by TERM=xterm-256color.
/// Printable text is handled separately so keyboard layout and IME text survive.
pub fn encode_key(key: Key, mods: Modifiers, mode: TermMode) -> Option<Vec<u8>> {
    if mods.mac_cmd {
        return None;
    }
    let modifier = 1 + u8::from(mods.shift) + 2 * u8::from(mods.alt) + 4 * u8::from(mods.ctrl);
    let mut prefix_alt = true;
    let text: String = match key {
        Key::Enter => "\r".into(),
        Key::Backspace if mods.ctrl => "\x08".into(),
        Key::Backspace => "\x7f".into(),
        Key::Escape => "\x1b".into(),
        Key::Tab if mods.shift => "\x1b[Z".into(),
        Key::Tab => "\t".into(),
        Key::ArrowUp | Key::ArrowDown | Key::ArrowRight | Key::ArrowLeft | Key::Home | Key::End => {
            prefix_alt = false;
            let suffix = match key {
                Key::ArrowUp => 'A',
                Key::ArrowDown => 'B',
                Key::ArrowRight => 'C',
                Key::ArrowLeft => 'D',
                Key::Home => 'H',
                _ => 'F',
            };
            if modifier > 1 {
                format!("\x1b[1;{modifier}{suffix}")
            } else if mode.contains(TermMode::APP_CURSOR) {
                format!("\x1bO{suffix}")
            } else {
                format!("\x1b[{suffix}")
            }
        }
        Key::F1 | Key::F2 | Key::F3 | Key::F4 => {
            prefix_alt = false;
            let suffix = match key {
                Key::F1 => 'P',
                Key::F2 => 'Q',
                Key::F3 => 'R',
                _ => 'S',
            };
            if modifier > 1 {
                format!("\x1b[1;{modifier}{suffix}")
            } else {
                format!("\x1bO{suffix}")
            }
        }
        Key::Insert
        | Key::Delete
        | Key::PageUp
        | Key::PageDown
        | Key::F5
        | Key::F6
        | Key::F7
        | Key::F8
        | Key::F9
        | Key::F10
        | Key::F11
        | Key::F12
        | Key::F13
        | Key::F14
        | Key::F15
        | Key::F16
        | Key::F17
        | Key::F18
        | Key::F19
        | Key::F20 => {
            prefix_alt = false;
            let number = match key {
                Key::Insert => 2,
                Key::Delete => 3,
                Key::PageUp => 5,
                Key::PageDown => 6,
                Key::F5 => 15,
                Key::F6 => 17,
                Key::F7 => 18,
                Key::F8 => 19,
                Key::F9 => 20,
                Key::F10 => 21,
                Key::F11 => 23,
                Key::F12 => 24,
                Key::F13 => 25,
                Key::F14 => 26,
                Key::F15 => 28,
                Key::F16 => 29,
                Key::F17 => 31,
                Key::F18 => 32,
                Key::F19 => 33,
                _ => 34,
            };
            if modifier > 1 {
                format!("\x1b[{number};{modifier}~")
            } else {
                format!("\x1b[{number}~")
            }
        }
        _ if mods.ctrl => {
            let code = match key {
                Key::Space | Key::Num2 => 0,
                Key::OpenBracket | Key::Num3 => 27,
                Key::Backslash | Key::Pipe | Key::Num4 => 28,
                Key::CloseBracket | Key::Num5 => 29,
                Key::Num6 => 30,
                Key::Minus | Key::Slash | Key::Num7 => 31,
                Key::Questionmark | Key::Num8 => 127,
                _ => {
                    let name = key.name().as_bytes();
                    if name.len() != 1 || !name[0].is_ascii_alphabetic() {
                        return None;
                    }
                    name[0].to_ascii_uppercase() - b'@'
                }
            };
            String::from(char::from(code))
        }
        _ => return None,
    };
    let mut bytes = text.into_bytes();
    if mods.alt && prefix_alt {
        bytes.insert(0, 27);
    }
    Some(bytes)
}

/// Button codes follow xterm: left=0, middle=1, right=2.
#[derive(Clone, Copy, Debug)]
pub enum Mouse {
    Press(u8),
    Release(u8),
    Motion(Option<u8>),
    /// True scrolls up; false scrolls down.
    Wheel(bool),
}

pub fn mouse_reporting(mode: TermMode) -> bool {
    mode.intersects(TermMode::MOUSE_MODE)
}

/// Mouse coordinates are zero based terminal cells, never window pixels.
/// Shift overrides application reporting so selection and scrollback stay usable.
pub fn encode_mouse(
    event: Mouse,
    row: usize,
    col: usize,
    mods: Modifiers,
    mode: TermMode,
) -> Option<Vec<u8>> {
    if !mouse_reporting(mode) || mods.shift || mods.mac_cmd {
        return None;
    }
    let (mut button, release) = match event {
        Mouse::Press(button) if button < 3 => (button, false),
        Mouse::Release(button) if button < 3 => (button, true),
        Mouse::Motion(button) => {
            if !mode.contains(TermMode::MOUSE_MOTION)
                && !(mode.contains(TermMode::MOUSE_DRAG) && button.is_some())
            {
                return None;
            }
            let button = button.unwrap_or(3);
            if button > 3 {
                return None;
            }
            (button + 32, false)
        }
        Mouse::Wheel(up) => (if up { 64 } else { 65 }, false),
        _ => return None,
    };
    button += 8 * u8::from(mods.alt) + 16 * u8::from(mods.ctrl);
    if mode.contains(TermMode::SGR_MOUSE) {
        let suffix = if release { 'm' } else { 'M' };
        let col = col.checked_add(1)?;
        let row = row.checked_add(1)?;
        return Some(format!("\x1b[<{button};{col};{row}{suffix}").into_bytes());
    }
    if release {
        button = 3 + 8 * u8::from(mods.alt) + 16 * u8::from(mods.ctrl);
    }
    let mut bytes = b"\x1b[M".to_vec();
    bytes.push(button + 32);
    if mode.contains(TermMode::UTF8_MOUSE) {
        // UTF-8 mouse coordinates are restricted to the two-byte range.
        if row > 2014 || col > 2014 {
            return None;
        }
        for coordinate in [col, row] {
            let mut utf8 = [0; 4];
            bytes.extend_from_slice(
                char::from_u32(coordinate as u32 + 33)?
                    .encode_utf8(&mut utf8)
                    .as_bytes(),
            );
        }
    } else {
        if row > 222 || col > 222 {
            return None;
        }
        bytes.extend_from_slice(&[col as u8 + 33, row as u8 + 33]);
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shell_interrupt_and_cursor_modes() {
        assert_eq!(
            encode_key(Key::C, Modifiers::CTRL, TermMode::empty()),
            Some(vec![3])
        );
        assert_eq!(
            encode_key(Key::ArrowUp, Modifiers::NONE, TermMode::APP_CURSOR),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            encode_key(Key::ArrowUp, Modifiers::CTRL, TermMode::empty()),
            Some(b"\x1b[1;5A".to_vec())
        );
    }

    #[test]
    fn clipboard_keeps_modifiers_when_keys_are_batched() {
        let events = vec![
            Event::Copy,
            Event::Key {
                key: Key::C,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: Modifiers::CTRL,
            },
        ];
        assert_eq!(
            clipboard_modifiers(&events, 0, Key::C, Modifiers::NONE),
            Modifiers::CTRL
        );
    }

    #[test]
    fn function_keys_control_punctuation_and_alt_sequences() {
        let mode = TermMode::empty();
        for (key, mods, expected) in [
            (Key::F1, Modifiers::NONE, &b"\x1bOP"[..]),
            (Key::F4, Modifiers::CTRL, &b"\x1b[1;5S"[..]),
            (Key::F12, Modifiers::NONE, &b"\x1b[24~"[..]),
            (
                Key::F12,
                Modifiers::ALT | Modifiers::SHIFT,
                &b"\x1b[24;4~"[..],
            ),
            (Key::Tab, Modifiers::SHIFT, &b"\x1b[Z"[..]),
            (Key::Enter, Modifiers::ALT, &b"\x1b\r"[..]),
            (Key::Backspace, Modifiers::CTRL, &b"\x08"[..]),
            (Key::C, Modifiers::CTRL | Modifiers::ALT, &b"\x1b\x03"[..]),
            (Key::Backslash, Modifiers::CTRL, &b"\x1c"[..]),
            (Key::CloseBracket, Modifiers::CTRL, &b"\x1d"[..]),
            (Key::Num6, Modifiers::CTRL, &b"\x1e"[..]),
            (Key::Minus, Modifiers::CTRL, &b"\x1f"[..]),
        ] {
            assert_eq!(
                encode_key(key, mods, mode),
                Some(expected.to_vec()),
                "{key:?} {mods:?}"
            );
        }
        assert!(encode_key(Key::A, Modifiers::NONE, mode).is_none());
        assert!(encode_key(Key::F1, Modifiers::MAC_CMD, mode).is_none());
    }

    #[test]
    fn mouse_protocols_distinguish_release_drag_and_hover() {
        let click = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        assert_eq!(
            encode_mouse(Mouse::Press(0), 4, 8, Modifiers::NONE, click),
            Some(b"\x1b[<0;9;5M".to_vec())
        );
        assert_eq!(
            encode_mouse(Mouse::Release(2), 4, 8, Modifiers::CTRL, click),
            Some(b"\x1b[<18;9;5m".to_vec())
        );
        assert_eq!(
            encode_mouse(Mouse::Wheel(true), 4, 8, Modifiers::NONE, click),
            Some(b"\x1b[<64;9;5M".to_vec())
        );
        assert!(encode_mouse(Mouse::Motion(Some(0)), 4, 8, Modifiers::NONE, click).is_none());
        let drag = TermMode::MOUSE_DRAG | TermMode::SGR_MOUSE;
        assert_eq!(
            encode_mouse(Mouse::Motion(Some(0)), 4, 8, Modifiers::NONE, drag),
            Some(b"\x1b[<32;9;5M".to_vec())
        );
        assert!(encode_mouse(Mouse::Motion(None), 4, 8, Modifiers::NONE, drag).is_none());
        let motion = TermMode::MOUSE_MOTION | TermMode::SGR_MOUSE;
        assert_eq!(
            encode_mouse(Mouse::Motion(None), 4, 8, Modifiers::NONE, motion),
            Some(b"\x1b[<35;9;5M".to_vec())
        );
        assert!(encode_mouse(Mouse::Press(0), 4, 8, Modifiers::SHIFT, motion).is_none());
        assert!(encode_mouse(Mouse::Press(0), 4, 8, Modifiers::NONE, TermMode::empty()).is_none());
    }

    #[test]
    fn legacy_mouse_coordinates_are_bounded_and_utf8_mode_is_honored() {
        let mode = TermMode::MOUSE_REPORT_CLICK;
        assert_eq!(
            encode_mouse(Mouse::Press(0), 0, 0, Modifiers::NONE, mode),
            Some(b"\x1b[M !!".to_vec())
        );
        assert_eq!(
            encode_mouse(Mouse::Release(0), 0, 0, Modifiers::NONE, mode),
            Some(b"\x1b[M#!!".to_vec())
        );
        assert!(encode_mouse(Mouse::Press(0), 223, 0, Modifiers::NONE, mode).is_none());
        let utf8 = mode | TermMode::UTF8_MOUSE;
        assert_eq!(
            encode_mouse(Mouse::Press(0), 0, 223, Modifiers::NONE, utf8),
            Some(b"\x1b[M \xc4\x80!".to_vec())
        );
        assert!(encode_mouse(Mouse::Press(0), 2015, 0, Modifiers::NONE, utf8).is_none());
        assert!(
            encode_mouse(
                Mouse::Press(0),
                usize::MAX,
                0,
                Modifiers::NONE,
                mode | TermMode::SGR_MOUSE
            )
            .is_none()
        );
    }
}
