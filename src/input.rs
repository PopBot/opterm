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

pub fn app_shortcut(mods: Modifiers) -> bool {
    if cfg!(target_os = "macos") {
        mods.mac_cmd && !mods.ctrl && !mods.alt
    } else {
        mods.ctrl && mods.shift && !mods.alt
    }
}

pub fn encode_key(key: Key, mods: Modifiers, mode: TermMode) -> Option<Vec<u8>> {
    if mods.mac_cmd {
        return None;
    }
    let text = match key {
        Key::Enter => "\r".into(),
        Key::Backspace => "\x7f".into(),
        Key::Escape => "\x1b".into(),
        Key::Tab if mods.shift => "\x1b[Z".into(),
        Key::Tab => "\t".into(),
        Key::ArrowUp | Key::ArrowDown | Key::ArrowRight | Key::ArrowLeft | Key::Home | Key::End => {
            let suffix = match key {
                Key::ArrowUp => 'A',
                Key::ArrowDown => 'B',
                Key::ArrowRight => 'C',
                Key::ArrowLeft => 'D',
                Key::Home => 'H',
                _ => 'F',
            };
            let modifier =
                1 + u8::from(mods.shift) + 2 * u8::from(mods.alt) + 4 * u8::from(mods.ctrl);
            if modifier > 1 {
                format!("\x1b[1;{modifier}{suffix}")
            } else if mode.contains(TermMode::APP_CURSOR) {
                format!("\x1bO{suffix}")
            } else {
                format!("\x1b[{suffix}")
            }
        }
        Key::Insert | Key::Delete | Key::PageUp | Key::PageDown => {
            let number = match key {
                Key::Insert => 2,
                Key::Delete => 3,
                Key::PageUp => 5,
                _ => 6,
            };
            let modifier =
                1 + u8::from(mods.shift) + 2 * u8::from(mods.alt) + 4 * u8::from(mods.ctrl);
            if modifier > 1 {
                format!("\x1b[{number};{modifier}~")
            } else {
                format!("\x1b[{number}~")
            }
        }
        Key::Space if mods.ctrl => "\0".into(),
        _ if mods.ctrl => {
            let name = key.name().as_bytes();
            if name.len() == 1 && name[0].is_ascii_alphabetic() {
                return Some(vec![name[0].to_ascii_uppercase() - b'@']);
            }
            return None;
        }
        _ => return None,
    };
    Some(text.into_bytes())
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
}
