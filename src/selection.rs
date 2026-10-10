//! Selection gestures delegate Unicode, wide-cell and wrapped-line rules to the kernel.
use alacritty_terminal::{
    event::EventListener,
    grid::Dimensions,
    index::{Column, Point, Side},
    selection::{Selection, SelectionType},
    term::{Term, viewport_to_point},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SelectionMode {
    #[default]
    Simple,
    Semantic,
    Lines,
    Block,
}

impl SelectionMode {
    /// Alt-drag selects a rectangle; double/triple clicks select words/logical lines.
    pub fn gesture(clicks: u8, alt: bool) -> Self {
        match (clicks, alt) {
            (_, true) => Self::Block,
            (3.., false) => Self::Lines,
            (2, false) => Self::Semantic,
            _ => Self::Simple,
        }
    }
}

/// Mouse-aware programs receive gestures unless Shift explicitly requests selection.
pub fn uses_local_selection(mouse_reporting: bool, shift: bool) -> bool {
    !mouse_reporting || shift
}

pub fn apply<T: EventListener>(
    term: &mut Term<T>,
    row: usize,
    col: usize,
    start: bool,
    mode: SelectionMode,
) {
    let point = viewport_to_point(
        term.grid().display_offset(),
        Point::new(
            row.min(term.screen_lines().saturating_sub(1)),
            Column(col.min(term.columns().saturating_sub(1))),
        ),
    );
    if start {
        let ty = match mode {
            SelectionMode::Simple => SelectionType::Simple,
            SelectionMode::Semantic => SelectionType::Semantic,
            SelectionMode::Lines => SelectionType::Lines,
            SelectionMode::Block => SelectionType::Block,
        };
        term.selection = Some(Selection::new(ty, point, Side::Left));
    } else if let Some(selection) = &mut term.selection {
        selection.update(point, Side::Right);
        // This API receives whole-cell coordinates, so either drag direction includes
        // both endpoint cells. Kernel expansion keeps wide-character pairs intact.
        selection.include_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::{event::Event, term::Config, vte::ansi::Processor};

    struct Listener;
    impl EventListener for Listener {
        fn send_event(&self, _: Event) {}
    }
    struct Size;
    impl Dimensions for Size {
        fn total_lines(&self) -> usize {
            4
        }
        fn screen_lines(&self) -> usize {
            4
        }
        fn columns(&self) -> usize {
            12
        }
    }
    fn terminal(text: &str) -> Term<Listener> {
        let mut term = Term::new(Config::default(), &Size, Listener);
        let mut processor: Processor = Processor::new();
        processor.advance(&mut term, text.as_bytes());
        term
    }

    #[test]
    fn semantic_selection_preserves_wide_and_combining_characters() {
        let mut term = terminal("hello 界e\u{301} test");
        apply(&mut term, 0, 7, true, SelectionMode::Semantic);
        assert_eq!(term.selection_to_string().as_deref(), Some("界e\u{301}"));
    }

    #[test]
    fn line_selection_includes_soft_wrapped_line_without_inserted_newline() {
        let mut term = terminal("abcdefghijklmnop\r\nsecond");
        apply(&mut term, 1, 2, true, SelectionMode::Lines);
        assert_eq!(
            term.selection_to_string().as_deref(),
            Some("abcdefghijklmnop\n")
        );
    }

    #[test]
    fn block_drag_keeps_column_boundaries_and_direction() {
        let mut term = terminal("abcdef\r\nghijkl");
        apply(&mut term, 1, 3, true, SelectionMode::Block);
        apply(&mut term, 0, 1, false, SelectionMode::Simple);
        assert_eq!(term.selection.as_ref().unwrap().ty, SelectionType::Block);
        assert_eq!(term.selection_to_string().as_deref(), Some("bcd\nhij"));
        apply(&mut term, 0, 1, true, SelectionMode::Block);
        apply(&mut term, 1, 3, false, SelectionMode::Simple);
        assert_eq!(term.selection_to_string().as_deref(), Some("bcd\nhij"));
    }

    #[test]
    fn gestures_and_mouse_override_are_explicit() {
        assert_eq!(SelectionMode::gesture(1, false), SelectionMode::Simple);
        assert_eq!(SelectionMode::gesture(2, false), SelectionMode::Semantic);
        assert_eq!(SelectionMode::gesture(3, false), SelectionMode::Lines);
        assert_eq!(SelectionMode::gesture(1, true), SelectionMode::Block);
        assert!(!uses_local_selection(true, false));
        assert!(uses_local_selection(true, true));
        assert!(uses_local_selection(false, false));
    }
}
