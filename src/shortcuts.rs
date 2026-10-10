//! User-editable application bindings. An empty binding deliberately disables it.
use anyhow::{Result, bail, ensure};
use eframe::egui::{Key, Modifiers};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    NewTab,
    NewWindow,
    Close,
    Copy,
    Paste,
    Search,
    Settings,
    SplitRight,
    SplitBelow,
    Maximize,
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
    ResizeLeft,
    ResizeRight,
    ResizeUp,
    ResizeDown,
    NextTab,
    PreviousTab,
}
impl Action {
    pub const ALL: [Self; 20] = [
        Self::NewTab,
        Self::NewWindow,
        Self::Close,
        Self::Copy,
        Self::Paste,
        Self::Search,
        Self::Settings,
        Self::SplitRight,
        Self::SplitBelow,
        Self::Maximize,
        Self::FocusLeft,
        Self::FocusRight,
        Self::FocusUp,
        Self::FocusDown,
        Self::ResizeLeft,
        Self::ResizeRight,
        Self::ResizeUp,
        Self::ResizeDown,
        Self::NextTab,
        Self::PreviousTab,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::NewTab => "New tab",
            Self::NewWindow => "New window",
            Self::Close => "Close current view",
            Self::Copy => "Copy",
            Self::Paste => "Paste",
            Self::Search => "Search scrollback",
            Self::Settings => "Settings",
            Self::SplitRight => "Split right",
            Self::SplitBelow => "Split below",
            Self::Maximize => "Maximize / restore pane",
            Self::FocusLeft => "Focus left",
            Self::FocusRight => "Focus right",
            Self::FocusUp => "Focus up",
            Self::FocusDown => "Focus down",
            Self::ResizeLeft => "Resize left",
            Self::ResizeRight => "Resize right",
            Self::ResizeUp => "Resize up",
            Self::ResizeDown => "Resize down",
            Self::NextTab => "Next tab",
            Self::PreviousTab => "Previous tab",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Shortcuts(pub BTreeMap<Action, String>);
impl Default for Shortcuts {
    fn default() -> Self {
        Self::for_platform(cfg!(target_os = "macos"))
    }
}
impl Shortcuts {
    pub fn for_platform(mac: bool) -> Self {
        use Action::*;
        let primary = if mac { "Cmd" } else { "Ctrl+Shift" };
        let mut bindings = BTreeMap::new();
        for (action, key) in [
            (NewTab, "T"),
            (NewWindow, "N"),
            (Close, "W"),
            (Copy, "C"),
            (Paste, "V"),
            (Search, "F"),
            (Settings, "Comma"),
        ] {
            bindings.insert(action, format!("{primary}+{key}"));
        }
        bindings.insert(
            SplitRight,
            if mac { "Cmd+D" } else { "Ctrl+Shift+E" }.into(),
        );
        bindings.insert(
            SplitBelow,
            if mac { "Cmd+Shift+D" } else { "Ctrl+Shift+O" }.into(),
        );
        bindings.insert(
            Maximize,
            if mac {
                "Cmd+Shift+Enter"
            } else {
                "Ctrl+Shift+Enter"
            }
            .into(),
        );
        for (focus, resize, direction) in [
            (FocusLeft, ResizeLeft, "ArrowLeft"),
            (FocusRight, ResizeRight, "ArrowRight"),
            (FocusUp, ResizeUp, "ArrowUp"),
            (FocusDown, ResizeDown, "ArrowDown"),
        ] {
            bindings.insert(
                focus,
                format!("{}+{direction}", if mac { "Cmd+Alt" } else { "Ctrl+Shift" }),
            );
            bindings.insert(
                resize,
                format!(
                    "{}+{direction}",
                    if mac {
                        "Cmd+Alt+Shift"
                    } else {
                        "Ctrl+Shift+Alt"
                    }
                ),
            );
        }
        bindings.insert(NextTab, String::new());
        bindings.insert(PreviousTab, String::new());
        Self(bindings)
    }
    pub fn validate(&self) -> Result<()> {
        let mut used = BTreeMap::new();
        for (&action, binding) in &self.0 {
            if let Some(chord) = Chord::parse(binding)? {
                let identity = (format!("{:?}", chord.key), chord.bits);
                if let Some(other) = used.insert(identity, action) {
                    bail!(
                        "{} and {} use the same shortcut. Unbind one before reassigning it.",
                        other.label(),
                        action.label()
                    );
                }
            }
        }
        Ok(())
    }
    pub fn matches(&self, action: Action, key: Key, mods: Modifiers) -> bool {
        self.0
            .get(&action)
            .and_then(|text| Chord::parse(text).ok().flatten())
            .is_some_and(|chord| chord.key == key && chord.bits == modifier_bits(mods))
    }
    pub fn action(&self, key: Key, mods: Modifiers) -> Option<Action> {
        Action::ALL
            .into_iter()
            .find(|&action| self.matches(action, key, mods))
    }
}

struct Chord {
    key: Key,
    bits: u8,
}
fn modifier_bits(mods: Modifiers) -> u8 {
    u8::from(mods.ctrl)
        | (u8::from(mods.shift) << 1)
        | (u8::from(mods.alt) << 2)
        | (u8::from(mods.mac_cmd) << 3)
}
impl Chord {
    fn parse(text: &str) -> Result<Option<Self>> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        ensure!(text.len() <= 80, "Shortcut is too long");
        let (modifiers, name) = text.rsplit_once('+').unwrap_or(("", text));
        let mut bits = 0;
        if !modifiers.is_empty() {
            for name in modifiers.split('+').map(str::trim) {
                let bit = if ["ctrl", "control"]
                    .iter()
                    .any(|v| name.eq_ignore_ascii_case(v))
                {
                    1
                } else if name.eq_ignore_ascii_case("shift") {
                    2
                } else if ["alt", "option"]
                    .iter()
                    .any(|v| name.eq_ignore_ascii_case(v))
                {
                    4
                } else if ["cmd", "command", "super"]
                    .iter()
                    .any(|v| name.eq_ignore_ascii_case(v))
                {
                    8
                } else {
                    bail!("Unknown shortcut modifier {name}; use Ctrl, Shift, Alt or Cmd");
                };
                ensure!(bits & bit == 0, "Shortcut repeats modifier {name}");
                bits |= bit;
            }
        }
        let name = name.trim();
        let key = Key::from_name(name)
            .or_else(|| {
                Key::ALL
                    .iter()
                    .copied()
                    .find(|key| key.name().eq_ignore_ascii_case(name))
            })
            .ok_or_else(|| anyhow::anyhow!("Unknown shortcut key {name}"))?;
        Ok(Some(Self { key, bits }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_preserve_interrupt_and_distinguish_navigation_from_resize() {
        for mac in [false, true] {
            Shortcuts::for_platform(mac).validate().unwrap();
        }
        let bindings = Shortcuts::for_platform(false);
        assert_eq!(bindings.action(Key::C, Modifiers::CTRL), None);
        assert_eq!(
            bindings.action(Key::C, Modifiers::CTRL | Modifiers::SHIFT),
            Some(Action::Copy)
        );
        assert_eq!(
            bindings.action(Key::ArrowLeft, Modifiers::CTRL | Modifiers::SHIFT),
            Some(Action::FocusLeft)
        );
        assert_eq!(
            bindings.action(
                Key::ArrowLeft,
                Modifiers::CTRL | Modifiers::SHIFT | Modifiers::ALT
            ),
            Some(Action::ResizeLeft)
        );
        assert_eq!(
            Shortcuts::for_platform(true).action(Key::D, Modifiers::MAC_CMD | Modifiers::SHIFT),
            Some(Action::SplitBelow)
        );
    }
    #[test]
    fn duplicate_aliases_require_explicit_unbind_and_reassignment() {
        let mut bindings = Shortcuts::for_platform(false);
        bindings.0.insert(Action::NewTab, "shift+control+c".into());
        assert!(
            bindings
                .validate()
                .unwrap_err()
                .to_string()
                .contains("Unbind")
        );
        bindings.0.insert(Action::Copy, String::new());
        bindings.validate().unwrap();
        assert_eq!(
            bindings.action(Key::C, Modifiers::CTRL | Modifiers::SHIFT),
            Some(Action::NewTab)
        );
        for text in ["Ctrl+Ctrl+C", "Hyper+C", "Ctrl+", "Ctrl+NotAKey"] {
            bindings.0.insert(Action::NewTab, text.into());
            assert!(bindings.validate().is_err(), "{text}");
        }
    }
}
