//! IME composition is local preview state; only a final commit becomes PTY input.
use eframe::egui::ImeEvent;

const MAX_PREVIEW_CHARS: usize = 128;

#[derive(Default)]
pub struct Composition {
    session: Option<u64>,
    text: String,
    cancelled: bool,
}
impl Composition {
    pub fn handle(&mut self, session: u64, event: &ImeEvent) -> Option<Vec<u8>> {
        match event {
            ImeEvent::Enabled => {
                self.session = Some(session);
                self.text.clear();
                self.cancelled = false;
                None
            }
            ImeEvent::Preedit(text) => {
                self.session = Some(session);
                self.text = text.chars().take(MAX_PREVIEW_CHARS).collect();
                self.cancelled = false;
                None
            }
            ImeEvent::Commit(text) => {
                let valid = !self.cancelled && self.session.is_none_or(|owner| owner == session);
                self.session = None;
                self.text.clear();
                // Keep cancellation until a fresh composition starts. A delayed
                // commit and its following Disabled event must not revive it.
                if valid {
                    Some(text.as_bytes().to_vec())
                } else {
                    self.cancelled = true;
                    None
                }
            }
            ImeEvent::Disabled => {
                self.session = None;
                self.text.clear();
                None
            }
        }
    }
    pub fn is_composing(&self, session: u64) -> bool {
        self.session == Some(session) && !self.text.is_empty()
    }
    pub fn clear_other_session(&mut self, session: Option<u64>) {
        if self.session.is_some() && self.session != session {
            self.session = None;
            self.text.clear();
            self.cancelled = true;
        }
    }
    pub fn text(&self, session: u64) -> Option<&str> {
        (self.is_composing(session) && !self.text.is_empty()).then_some(self.text.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preedit_is_preview_only_and_commit_is_one_exact_utf8_packet() {
        let mut composition = Composition::default();
        assert!(composition.handle(1, &ImeEvent::Enabled).is_none());
        assert!(!composition.is_composing(1));
        assert!(
            composition
                .handle(1, &ImeEvent::Preedit("仮入力🙂".into()))
                .is_none()
        );
        assert_eq!(composition.text(1), Some("仮入力🙂"));
        assert_eq!(composition.text(2), None);
        let commit = "確定🙂\r\nλ";
        assert_eq!(
            composition.handle(1, &ImeEvent::Commit(commit.into())),
            Some(commit.as_bytes().to_vec())
        );
        assert!(!composition.is_composing(1));
        assert!(composition.text(1).is_none());
    }
    #[test]
    fn preview_limit_never_splits_utf8_and_does_not_truncate_commit() {
        let mut composition = Composition::default();
        let text = "漢🙂".repeat(100);
        composition.handle(1, &ImeEvent::Preedit(text.clone()));
        let preview = composition.text(1).unwrap();
        assert_eq!(preview.chars().count(), MAX_PREVIEW_CHARS);
        assert!(preview.len() <= MAX_PREVIEW_CHARS * 4);
        assert_eq!(
            composition.handle(1, &ImeEvent::Commit(text.clone())),
            Some(text.into_bytes())
        );
    }
    #[test]
    fn focus_change_discards_delayed_commit_until_fresh_composition() {
        let mut composition = Composition::default();
        composition.handle(1, &ImeEvent::Preedit("old".into()));
        composition.clear_other_session(Some(2));
        assert!(composition.text(1).is_none());
        assert!(
            composition
                .handle(2, &ImeEvent::Commit("old".into()))
                .is_none()
        );
        composition.handle(2, &ImeEvent::Disabled);
        assert!(
            composition
                .handle(2, &ImeEvent::Commit("late".into()))
                .is_none()
        );
        composition.handle(2, &ImeEvent::Enabled);
        assert_eq!(
            composition.handle(2, &ImeEvent::Commit("new".into())),
            Some(b"new".to_vec())
        );
    }
    #[test]
    fn global_cancel_and_wrong_session_commit_never_forward_preedit() {
        let mut composition = Composition::default();
        composition.handle(1, &ImeEvent::Preedit("discard".into()));
        composition.clear_other_session(None);
        assert!(
            composition
                .handle(1, &ImeEvent::Commit("discard".into()))
                .is_none()
        );
        composition.handle(1, &ImeEvent::Preedit("replacement".into()));
        assert!(
            composition
                .handle(2, &ImeEvent::Commit("replacement".into()))
                .is_none()
        );
        assert!(!composition.is_composing(1));
    }
    #[test]
    fn same_focus_is_retained_and_empty_preedit_can_be_cancelled() {
        let mut composition = Composition::default();
        composition.handle(1, &ImeEvent::Preedit(String::new()));
        composition.clear_other_session(Some(1));
        assert!(!composition.is_composing(1));
        assert!(composition.text(1).is_none());
        composition.handle(1, &ImeEvent::Disabled);
        assert!(!composition.is_composing(1));
        // Direct commits are supported before any focus cancellation.
        assert_eq!(
            Composition::default().handle(1, &ImeEvent::Commit("é".into())),
            Some("é".as_bytes().to_vec())
        );
    }
}
