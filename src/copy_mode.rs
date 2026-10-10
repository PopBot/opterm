//! Keyboard selection over a frozen visible snapshot; never sends terminal input.
use crate::terminal::{Cell, Screen};
use eframe::egui::{Key, Modifiers};
use std::sync::Arc;

const MAX_CELLS: usize = 65_536;
const MAX_TEXT_BYTES: usize = 1024 * 1024;
const MAX_COLUMNS: usize = 2048;

pub enum Outcome {
    Continue,
    Exit,
    Copy(String),
}
pub struct CopyMode {
    screen: Arc<Screen>,
    cursor: (usize, usize),
    anchor: Option<(usize, usize)>,
    rows: usize,
    cols: usize,
    pub truncated: bool,
}
impl CopyMode {
    pub fn new(source: Arc<Screen>) -> Self {
        let cols = source
            .cells
            .iter()
            .map(|cell| cell.col.saturating_add(if cell.wide { 2 } else { 1 }))
            .chain(source.cursor.map(|(_, col)| col.saturating_add(1)))
            .max()
            .unwrap_or(1)
            .clamp(1, MAX_COLUMNS);
        let source_rows = source
            .cells
            .iter()
            .map(|cell| cell.row.saturating_add(1))
            .chain(source.cursor.map(|(row, _)| row.saturating_add(1)))
            .chain(source.wrapped_rows.iter().map(|row| row.saturating_add(1)))
            .max()
            .unwrap_or(1);
        let rows = source_rows.clamp(1, (MAX_CELLS / cols).clamp(1, 1024));
        let mut screen = Screen::empty();
        screen.background = source.background;
        screen.cursor_color = source.cursor_color;
        screen.selection_color = source.selection_color;
        screen.mode = source.mode;
        screen.title = source.title.chars().take(128).collect();
        screen.exited = source.exited;
        screen.cursor = source.cursor.filter(|&(row, col)| row < rows && col < cols);
        screen.wrapped_rows = source
            .wrapped_rows
            .iter()
            .copied()
            .filter(|&row| row < rows)
            .take(rows)
            .collect();
        let mut bytes = 0;
        let mut truncated = source_rows > rows;
        for cell in &source.cells {
            if cell.row >= rows || cell.col >= cols {
                truncated = true;
                continue;
            }
            if screen.cells.len() == MAX_CELLS || bytes + cell.text.len() > MAX_TEXT_BYTES {
                truncated = true;
                break;
            }
            bytes += cell.text.len();
            screen.cells.push(Cell {
                row: cell.row,
                col: cell.col,
                text: cell.text.clone(),
                fg: cell.fg,
                bg: cell.bg,
                wide: cell.wide,
                underline: cell.underline,
                selected: false,
                search_match: false,
                search_current: false,
                hyperlink: None,
            });
        }
        screen.cells.sort_by_key(|cell| (cell.row, cell.col));
        let cursor = screen.cursor.unwrap_or((0, 0));
        let mut result = Self {
            screen: Arc::new(screen),
            cursor,
            anchor: None,
            rows,
            cols,
            truncated,
        };
        result.cursor = result.normalize(result.cursor);
        result
    }
    pub fn screen(&self) -> Arc<Screen> {
        self.screen.clone()
    }
    pub fn cursor(&self) -> (usize, usize) {
        self.cursor
    }
    fn cell(&self, row: usize, col: usize) -> Option<&Cell> {
        let index = self
            .screen
            .cells
            .partition_point(|cell| (cell.row, cell.col) <= (row, col));
        self.screen
            .cells
            .get(index.checked_sub(1)?)
            .filter(|cell| cell.row == row && col < cell.col + if cell.wide { 2 } else { 1 })
    }
    fn normalize(&self, point: (usize, usize)) -> (usize, usize) {
        let (row, col) = (point.0.min(self.rows - 1), point.1.min(self.cols - 1));
        (row, self.cell(row, col).map_or(col, |cell| cell.col))
    }
    pub fn contains(&self, row: usize, col: usize) -> bool {
        if row >= self.rows || col >= self.cols {
            return false;
        }
        self.anchor.is_some_and(|anchor| {
            let point = self.normalize((row, col));
            anchor.min(self.cursor) <= point && point <= anchor.max(self.cursor)
        })
    }
    pub fn text(&self) -> Option<String> {
        let anchor = self.anchor?;
        let (start, end) = (anchor.min(self.cursor), anchor.max(self.cursor));
        let mut text = String::new();
        for row in start.0..=end.0 {
            let from = if row == start.0 { start.1 } else { 0 };
            let to = if row == end.0 { end.1 } else { self.cols - 1 };
            let row_start = text.len();
            let mut col = from;
            while col <= to && text.len() < MAX_TEXT_BYTES {
                if let Some(cell) = self.cell(row, col) {
                    if text.len() + cell.text.len() > MAX_TEXT_BYTES {
                        return Some(text);
                    }
                    text.push_str(&cell.text);
                    col = cell.col + if cell.wide { 2 } else { 1 };
                } else {
                    text.push(' ');
                    col += 1;
                }
            }
            if !self.screen.wrapped_rows.contains(&row) {
                while text.len() > row_start && text.ends_with(' ') {
                    text.pop();
                }
                if row < end.0 {
                    text.push('\n');
                }
            }
        }
        Some(text)
    }
    fn step(&self, point: (usize, usize), right: bool) -> (usize, usize) {
        let width = self
            .cell(point.0, point.1)
            .map_or(1, |cell| if cell.wide { 2 } else { 1 });
        self.normalize(if right {
            if point.1 + width < self.cols {
                (point.0, point.1 + width)
            } else if point.0 + 1 < self.rows {
                (point.0 + 1, 0)
            } else {
                point
            }
        } else if point.1 > 0 {
            (point.0, point.1 - 1)
        } else if point.0 > 0 {
            (point.0 - 1, self.cols - 1)
        } else {
            point
        })
    }
    fn category(&self, point: (usize, usize)) -> u8 {
        match self
            .cell(point.0, point.1)
            .and_then(|cell| cell.text.chars().next())
            .unwrap_or(' ')
        {
            c if c.is_whitespace() => 0,
            c if c.is_alphanumeric() || c == '_' => 1,
            _ => 2,
        }
    }
    fn word(&self, right: bool) -> (usize, usize) {
        let mut point = self.cursor;
        if right {
            let category = self.category(point);
            while self.category(point) == category {
                let next = self.step(point, true);
                if next == point {
                    return point;
                }
                point = next;
            }
            while self.category(point) == 0 {
                let next = self.step(point, true);
                if next == point {
                    break;
                }
                point = next;
            }
        } else {
            point = self.step(point, false);
            while self.category(point) == 0 {
                let previous = self.step(point, false);
                if previous == point {
                    return point;
                }
                point = previous;
            }
            let category = self.category(point);
            loop {
                let previous = self.step(point, false);
                if previous == point || self.category(previous) != category {
                    break;
                }
                point = previous;
            }
        }
        point
    }
    /// Parent consumes all key/text events while active; ignored keys never reach the PTY.
    pub fn handle_key(&mut self, key: Key, mods: Modifiers) -> Outcome {
        if key == Key::Escape {
            return Outcome::Exit;
        }
        if key == Key::Enter || (key == Key::C && ((mods.ctrl && mods.shift) || mods.mac_cmd)) {
            let text = if let Some(text) = self.text() {
                text
            } else {
                let previous = self.anchor;
                self.anchor = Some((self.cursor.0, 0));
                let cursor = self.cursor;
                self.cursor.1 = self.cols - 1;
                let text = self.text().unwrap_or_default();
                self.cursor = cursor;
                self.anchor = previous;
                text
            };
            return Outcome::Copy(text);
        }
        if key == Key::V && !mods.ctrl && !mods.mac_cmd && !mods.alt {
            self.anchor = if self.anchor.is_some() {
                None
            } else {
                Some(self.cursor)
            };
            return Outcome::Continue;
        }
        let old = self.cursor;
        let word = mods.ctrl || mods.alt;
        let point = match key {
            Key::ArrowLeft | Key::H => {
                if word {
                    self.word(false)
                } else {
                    self.step(old, false)
                }
            }
            Key::ArrowRight | Key::L => {
                if word {
                    self.word(true)
                } else {
                    self.step(old, true)
                }
            }
            Key::ArrowUp | Key::K => (old.0.saturating_sub(1), old.1),
            Key::ArrowDown | Key::J => ((old.0 + 1).min(self.rows - 1), old.1),
            Key::Home => {
                if mods.ctrl || mods.mac_cmd {
                    (0, 0)
                } else {
                    (old.0, 0)
                }
            }
            Key::End => {
                if mods.ctrl || mods.mac_cmd {
                    (self.rows - 1, self.cols - 1)
                } else {
                    (old.0, self.cols - 1)
                }
            }
            Key::PageUp => (
                old.0.saturating_sub(self.rows.saturating_sub(1).max(1)),
                old.1,
            ),
            Key::PageDown => (
                (old.0 + self.rows.saturating_sub(1).max(1)).min(self.rows - 1),
                old.1,
            ),
            _ => return Outcome::Continue,
        };
        if mods.shift && self.anchor.is_none() {
            self.anchor = Some(old);
        }
        self.cursor = self.normalize(point);
        Outcome::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot(rows: &[&str], cols: usize, wraps: &[usize]) -> Arc<Screen> {
        let mut screen = Screen::empty();
        screen.cursor = Some((0, 0));
        screen.wrapped_rows = wraps.to_vec();
        for (row, text) in rows.iter().enumerate() {
            for (col, c) in text.chars().enumerate() {
                screen.cells.push(Cell {
                    row,
                    col,
                    text: c.to_string(),
                    fg: [255; 3],
                    bg: [0; 3],
                    wide: false,
                    underline: false,
                    selected: false,
                    search_match: false,
                    search_current: false,
                    hyperlink: None,
                });
            }
        }
        if cols > 0
            && !screen
                .cells
                .iter()
                .any(|cell| cell.row == rows.len().saturating_sub(1) && cell.col == cols - 1)
        {
            screen.cells.push(Cell {
                row: rows.len().saturating_sub(1),
                col: cols - 1,
                text: " ".into(),
                fg: [255; 3],
                bg: [0; 3],
                wide: false,
                underline: false,
                selected: false,
                search_match: false,
                search_current: false,
                hyperlink: None,
            });
        }
        Arc::new(screen)
    }
    #[test]
    fn word_motion_shift_selection_and_copy_do_not_mutate_source() {
        let source = snapshot(&["hello world"], 12, &[]);
        let mut mode = CopyMode::new(source.clone());
        mode.handle_key(Key::ArrowRight, Modifiers::CTRL | Modifiers::SHIFT);
        assert_eq!(mode.cursor(), (0, 6));
        assert_eq!(mode.text().as_deref(), Some("hello w"));
        assert!(mode.contains(0, 0));
        assert!(!mode.contains(0, 7));
        match mode.handle_key(Key::Enter, Modifiers::NONE) {
            Outcome::Copy(text) => assert_eq!(text, "hello w"),
            _ => panic!("Expected copy"),
        }
        assert!(source.cells.iter().all(|cell| !cell.selected));
        mode.handle_key(Key::ArrowLeft, Modifiers::CTRL);
        assert_eq!(mode.cursor(), (0, 0));
        assert!(matches!(
            mode.handle_key(Key::Escape, Modifiers::NONE),
            Outcome::Exit
        ));
    }
    #[test]
    fn soft_wrapping_joins_but_hard_line_breaks_are_preserved() {
        let mut mode = CopyMode::new(snapshot(&["abcdef", "ghij", "next"], 6, &[0]));
        mode.handle_key(Key::V, Modifiers::NONE);
        mode.handle_key(Key::End, Modifiers::CTRL);
        assert_eq!(mode.text().as_deref(), Some("abcdefghij\nnext"));
        mode.handle_key(Key::PageUp, Modifiers::NONE);
        assert_eq!(mode.cursor().0, 0);
    }
    #[test]
    fn wide_and_combining_glyphs_are_selected_as_complete_cells() {
        let mut source = (*snapshot(&["a界e"], 5, &[])).clone();
        source.cells[1].wide = true;
        source.cells[2].col = 3;
        source.cells[2].text = "e\u{301}".into();
        source.cursor = Some((0, 1));
        let mut mode = CopyMode::new(Arc::new(source));
        mode.handle_key(Key::ArrowRight, Modifiers::SHIFT);
        assert_eq!(mode.cursor(), (0, 3));
        assert_eq!(mode.text().as_deref(), Some("界e\u{301}"));
        mode.handle_key(Key::ArrowLeft, Modifiers::NONE);
        assert_eq!(mode.cursor(), (0, 1));
        assert!(mode.contains(0, 2));
    }
    #[test]
    fn snapshot_is_frozen_bounded_and_plain_enter_copies_current_line() {
        let mut source = (*snapshot(&["current line"], 12, &[])).clone();
        source.cells.push(Cell {
            row: usize::MAX,
            col: usize::MAX,
            text: "out of bounds".into(),
            fg: [0; 3],
            bg: [0; 3],
            wide: false,
            underline: false,
            selected: false,
            search_match: false,
            search_current: false,
            hyperlink: None,
        });
        let mut mode = CopyMode::new(Arc::new(source));
        assert!(mode.truncated);
        assert!(mode.screen().cells.len() <= MAX_CELLS);
        match mode.handle_key(Key::Enter, Modifiers::NONE) {
            Outcome::Copy(text) => assert_eq!(text, "current line"),
            _ => panic!("Expected copy"),
        }
        assert!(matches!(
            mode.handle_key(Key::A, Modifiers::NONE),
            Outcome::Continue
        ));
        assert_eq!(mode.cursor(), (0, 0));
    }
}
