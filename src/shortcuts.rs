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
    QuickSwitch,
    UndoClose,
    CopyMode,
    FindCursor,
    PreviousPrompt,
    NextPrompt,
    CopyLastOutput,
}
impl Action {
    pub const ALL: [Self; 27] = [
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
        Self::QuickSwitch,
        Self::UndoClose,
        Self::CopyMode,
        Self::FindCursor,
        Self::PreviousPrompt,
        Self::NextPrompt,
        Self::CopyLastOutput,
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
            Self::QuickSwitch => "Quick session switcher",
            Self::UndoClose => "Undo close",
            Self::CopyMode => "Keyboard copy mode",
            Self::FindCursor => "Find cursor",
            Self::PreviousPrompt => "Previous prompt",
            Self::NextPrompt => "Next prompt",
            Self::CopyLastOutput => "Copy last command output",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Shortcuts(pub BTreeMap<Action, String>);
impl<'de> Deserialize<'de> for Shortcuts {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let mut shortcuts = Self(BTreeMap::<Action, String>::deserialize(deserializer)?);
        // Older configuration files predate these actions. Add their defaults
        // only when unclaimed; preserve explicit empty and custom bindings.
        let defaults = Self::default();
        for action in [
            Action::QuickSwitch,
            Action::UndoClose,
            Action::CopyMode,
            Action::FindCursor,
            Action::PreviousPrompt,
            Action::NextPrompt,
            Action::CopyLastOutput,
        ] {
            if !shortcuts.0.contains_key(&action) {
                shortcuts.0.insert(action, defaults.0[&action].clone());
                if shortcuts.validate().is_err() {
                    shortcuts.0.insert(action, String::new());
                }
            }
        }
        Ok(shortcuts)
    }
}
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
        bindings.insert(
            QuickSwitch,
            if mac { "Cmd+P" } else { "Ctrl+Shift+P" }.into(),
        );
        bindings.insert(
            UndoClose,
            if mac { "Cmd+Shift+T" } else { "Ctrl+Shift+Z" }.into(),
        );
        bindings.insert(
            CopyMode,
            if mac {
                "Cmd+Shift+Space"
            } else {
                "Ctrl+Shift+Space"
            }
            .into(),
        );
        bindings.insert(
            FindCursor,
            if mac { "Cmd+Shift+J" } else { "Ctrl+Shift+J" }.into(),
        );
        bindings.insert(
            PreviousPrompt,
            if mac {
                "Cmd+Alt+PageUp"
            } else {
                "Ctrl+Alt+PageUp"
            }
            .into(),
        );
        bindings.insert(
            NextPrompt,
            if mac {
                "Cmd+Alt+PageDown"
            } else {
                "Ctrl+Alt+PageDown"
            }
            .into(),
        );
        bindings.insert(
            CopyLastOutput,
            if mac {
                "Cmd+Shift+Alt+C"
            } else {
                "Ctrl+Shift+Alt+C"
            }
            .into(),
        );
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
    fn new_shortcuts_migrate_without_overwriting_custom_or_disabled_bindings() {
        let mut previous = Shortcuts::for_platform(false);
        previous.0.remove(&Action::QuickSwitch);
        previous.0.remove(&Action::UndoClose);
        let migrated: Shortcuts =
            serde_json::from_str(&serde_json::to_string(&previous).unwrap()).unwrap();
        assert_eq!(
            migrated.0[&Action::QuickSwitch],
            Shortcuts::default().0[&Action::QuickSwitch]
        );
        assert_eq!(
            migrated.0[&Action::UndoClose],
            Shortcuts::default().0[&Action::UndoClose]
        );
        previous.0.insert(
            Action::Copy,
            Shortcuts::default().0[&Action::QuickSwitch].clone(),
        );
        let migrated: Shortcuts =
            serde_json::from_str(&serde_json::to_string(&previous).unwrap()).unwrap();
        assert!(migrated.0[&Action::QuickSwitch].is_empty());
        previous.0.insert(Action::QuickSwitch, String::new());
        let migrated: Shortcuts =
            serde_json::from_str(&serde_json::to_string(&previous).unwrap()).unwrap();
        assert!(migrated.0[&Action::QuickSwitch].is_empty());
        assert_eq!(
            Shortcuts::for_platform(false).action(Key::P, Modifiers::CTRL | Modifiers::SHIFT),
            Some(Action::QuickSwitch)
        );
        assert_eq!(
            Shortcuts::for_platform(false).action(Key::Z, Modifiers::CTRL | Modifiers::SHIFT),
            Some(Action::UndoClose)
        );
        assert_eq!(
            Shortcuts::for_platform(true).action(Key::P, Modifiers::MAC_CMD),
            Some(Action::QuickSwitch)
        );
        assert_eq!(
            Shortcuts::for_platform(true).action(Key::T, Modifiers::MAC_CMD | Modifiers::SHIFT),
            Some(Action::UndoClose)
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
    #[test]
    fn p1_defaults_migrate_without_conflicts_or_overwriting_user_bindings() {
        for mac in [false, true] {
            Shortcuts::for_platform(mac).validate().unwrap();
        }
        let actions = [
            Action::CopyMode,
            Action::FindCursor,
            Action::PreviousPrompt,
            Action::NextPrompt,
            Action::CopyLastOutput,
        ];
        let mut legacy = Shortcuts::default();
        for action in actions {
            legacy.0.remove(&action);
        }
        legacy.0.insert(
            Action::Search,
            Shortcuts::default().0[&Action::CopyMode].clone(),
        );
        let migrated: Shortcuts =
            serde_json::from_str(&serde_json::to_string(&legacy).unwrap()).unwrap();
        migrated.validate().unwrap();
        assert!(migrated.0[&Action::CopyMode].is_empty());
        assert_eq!(
            migrated.0[&Action::FindCursor],
            Shortcuts::default().0[&Action::FindCursor]
        );
        let mut custom = migrated;
        custom.0.insert(Action::FindCursor, "Alt+J".into());
        custom.0.insert(Action::CopyMode, String::new());
        let roundtrip: Shortcuts =
            serde_json::from_str(&serde_json::to_string(&custom).unwrap()).unwrap();
        assert_eq!(roundtrip.0[&Action::FindCursor], "Alt+J");
        assert!(roundtrip.0[&Action::CopyMode].is_empty());
    }
}
