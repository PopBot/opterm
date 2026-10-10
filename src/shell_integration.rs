//! Opt-in OSC 133 boundaries and local OSC 7 metadata. Bounded live-grid state only.
use alacritty_terminal::{
    event::EventListener,
    grid::{Dimensions, Scroll},
    index::{Column, Line, Point},
    term::{Term, TermMode, cell::Flags},
    vte::ansi::{self, Handler, Processor},
};
use anyhow::{Result, ensure};
use std::{collections::VecDeque, path::Path};

const MAX_OSC_BYTES: usize = 4096;
const MAX_PROMPTS: usize = 128;
const MAX_OUTPUT_CELLS: usize = 262_144;
const MAX_OUTPUT_BYTES: usize = 1024 * 1024;

#[derive(Default)]
pub struct Integration {
    pub cwd: Option<String>,
    pub seen: bool,
    prompts: VecDeque<Point>,
    input: Option<Point>,
    output: Option<Point>,
    completed: Option<(Point, Point)>,
    navigation: Option<Point>,
    region: Option<(usize, usize)>,
}
impl Integration {
    /// Geometry changes invalidate annotations, never the last validated cwd.
    pub fn invalidate(&mut self) {
        self.prompts.clear();
        self.input = None;
        self.output = None;
        self.completed = None;
        self.navigation = None;
        self.region = None;
    }
    fn discard_annotations(&mut self) {
        let region = self.region;
        self.invalidate();
        // Most destructive ANSI edits leave the kernel's scrolling region intact.
        self.region = region;
    }
    pub fn trim<T: EventListener>(&mut self, term: &Term<T>) {
        let top = term.grid().topmost_line();
        self.prompts.retain(|point| point.line >= top);
        if self.input.is_some_and(|point| point.line < top) {
            self.input = None;
        }
        if self.output.is_some_and(|point| point.line < top) {
            self.output = None;
        }
        if self.completed.is_some_and(|(start, _)| start.line < top) {
            self.completed = None;
        }
        if self.navigation.is_some_and(|point| point.line < top) {
            self.navigation = None;
        }
    }
    fn scroll(&mut self, lines: usize) {
        let shift = |point: &mut Point| {
            point.line.0 = point
                .line
                .0
                .saturating_sub(lines.min(i32::MAX as usize) as i32)
        };
        for point in &mut self.prompts {
            shift(point);
        }
        for point in [&mut self.input, &mut self.output, &mut self.navigation]
            .into_iter()
            .flatten()
        {
            shift(point);
        }
        if let Some((start, end)) = &mut self.completed {
            shift(start);
            shift(end);
        }
    }
    fn osc<T: EventListener>(&mut self, payload: &[u8], term: &Term<T>) {
        if let Some(uri) = payload.strip_prefix(b"7;") {
            self.cwd = std::str::from_utf8(uri).ok().and_then(local_directory);
            return;
        }
        let Ok(text) = std::str::from_utf8(payload) else {
            return;
        };
        if term.mode().contains(TermMode::ALT_SCREEN) {
            return;
        }
        let Some(marker) = text.strip_prefix("133;") else {
            return;
        };
        let (kind, parameters) = marker.split_once(';').unwrap_or((marker, ""));
        if !matches!(kind, "A" | "B" | "C" | "D") || parameters.chars().any(char::is_control) {
            return;
        }
        let point = term.grid().cursor.point;
        match kind {
            "A" => {
                self.seen = true;
                self.input = None;
                self.output = None;
                self.navigation = None;
                self.prompts.push_back(point);
                while self.prompts.len() > MAX_PROMPTS {
                    self.prompts.pop_front();
                }
            }
            "B" if self.prompts.back().is_some_and(|prompt| *prompt <= point) => {
                self.input = Some(point)
            }
            "C" if self.input.is_some() && self.output.is_none() => {
                self.output = Some(point);
                self.completed = None;
            }
            "D" => {
                if let Some(start) = self.output.take()
                    && start <= point
                {
                    self.completed = Some((start, point));
                }
                self.input = None;
            }
            _ => {}
        }
        self.trim(term);
    }
    pub fn navigate<T: EventListener>(
        &mut self,
        term: &mut Term<T>,
        backwards: bool,
    ) -> Result<()> {
        ensure!(
            !term.mode().contains(TermMode::ALT_SCREEN),
            "Prompt navigation is unavailable in an alternate-screen program"
        );
        self.trim(term);
        ensure!(
            self.seen,
            "No shell integration markers. Source Opterm's Bash or zsh integration script first."
        );
        let origin = self.navigation.unwrap_or_else(|| {
            if term.grid().display_offset() > 0 {
                Point::new(Line(-(term.grid().display_offset() as i32)), Column(0))
            } else if self.input.is_some() && self.output.is_none() {
                self.prompts
                    .back()
                    .copied()
                    .unwrap_or(term.grid().cursor.point)
            } else {
                term.grid().cursor.point
            }
        });
        let point = if backwards {
            self.prompts.iter().rev().find(|point| **point < origin)
        } else {
            self.prompts.iter().find(|point| **point > origin)
        }
        .copied()
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No {} retained prompt",
                if backwards { "previous" } else { "next" }
            )
        })?;
        self.navigation = Some(point);
        let desired = (-point.line.0).max(0);
        term.scroll_display(Scroll::Delta(desired - term.grid().display_offset() as i32));
        Ok(())
    }
    pub fn reset_navigation(&mut self) {
        self.navigation = None;
    }
    pub fn target_row<T: EventListener>(&self, term: &Term<T>) -> Option<usize> {
        let line = self.navigation?.line.0 + term.grid().display_offset() as i32;
        (line >= 0 && (line as usize) < term.screen_lines()).then_some(line as usize)
    }
    pub fn last_output<T: EventListener>(&mut self, term: &Term<T>) -> Result<String> {
        ensure!(
            !term.mode().contains(TermMode::ALT_SCREEN),
            "Last command output is unavailable in an alternate-screen program"
        );
        self.trim(term);
        ensure!(
            self.seen,
            "No shell integration markers. Source Opterm's Bash or zsh integration script first."
        );
        let (start, end) = self.completed.ok_or_else(|| {
            anyhow::anyhow!("No completed command output remains in live scrollback")
        })?;
        let columns = term.columns();
        let span = (end.line.0 - start.line.0).max(0) as usize;
        ensure!(
            span.saturating_mul(columns).saturating_add(end.column.0) <= MAX_OUTPUT_CELLS,
            "Last command output exceeds the live copy limit"
        );
        if start == end {
            return Ok(String::new());
        }
        let mut text = String::new();
        for row in start.line.0..=end.line.0 {
            let first = if row == start.line.0 {
                start.column.0
            } else {
                0
            };
            let last = if row == end.line.0 {
                end.column.0
            } else {
                columns
            };
            if first >= last {
                continue;
            }
            let beginning = text.len();
            for column in first..last {
                let cell = &term.grid()[Line(row)][Column(column)];
                if cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                for character in
                    std::iter::once(&cell.c).chain(cell.zerowidth().into_iter().flatten())
                {
                    ensure!(
                        text.len() + character.len_utf8() <= MAX_OUTPUT_BYTES,
                        "Last command output exceeds the live copy byte limit"
                    );
                    text.push(*character);
                }
            }
            if last == columns
                && !term.grid()[Line(row)][Column(columns - 1)]
                    .flags
                    .contains(Flags::WRAPLINE)
            {
                let trimmed = text[beginning..].trim_end_matches(' ').len() + beginning;
                text.truncate(trimmed);
                if row < end.line.0 {
                    ensure!(
                        text.len() < MAX_OUTPUT_BYTES,
                        "Last command output exceeds the live copy byte limit"
                    );
                    text.push('\n');
                }
            }
        }
        Ok(text)
    }
}

/// OSC 7 is accepted only for this host and an existing absolute local directory.
/// This validates emitted metadata; it is not a cryptographic assertion of cwd.
pub fn local_directory(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    if rest.len() > MAX_OSC_BYTES || rest.chars().any(char::is_control) {
        return None;
    }
    let (host, path) = rest.split_once('/')?;
    let local = sysinfo::System::host_name();
    if !(host.is_empty()
        || host.eq_ignore_ascii_case("localhost")
        || local
            .as_deref()
            .is_some_and(|local| host.eq_ignore_ascii_case(local)))
    {
        return None;
    }
    if path.contains(['?', '#']) {
        return None;
    }
    let bytes = format!("/{path}").into_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = (*bytes.get(index + 1)? as char).to_digit(16)?;
            let low = (*bytes.get(index + 2)? as char).to_digit(16)?;
            decoded.push((high * 16 + low) as u8);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    let path = String::from_utf8(decoded).ok()?;
    if path.chars().any(char::is_control) {
        return None;
    }
    #[cfg(windows)]
    let path = if path.as_bytes().get(2) == Some(&b':') {
        path[1..].to_owned()
    } else {
        path
    };
    (Path::new(&path).is_absolute() && Path::new(&path).is_dir()).then_some(path)
}

/// Bounded OSC framing preserves original bytes and records markers at their
/// exact position in the ANSI stream, including sequences split across reads.
#[derive(Default)]
pub struct Stream {
    escape: bool,
    osc: bool,
    osc_escape: bool,
    oversized: bool,
    payload: Vec<u8>,
}
impl Stream {
    fn event(&mut self, byte: u8) -> Option<Vec<u8>> {
        if self.osc {
            if byte == 7 || (self.osc_escape && byte == b'\\') {
                if self.osc_escape && self.payload.last() == Some(&27) {
                    self.payload.pop();
                }
                self.osc = false;
                self.osc_escape = false;
                if self.oversized {
                    return self.payload.starts_with(b"7;").then(|| b"7;".to_vec());
                }
                return Some(std::mem::take(&mut self.payload));
            }
            if self.payload.len() < MAX_OSC_BYTES {
                self.payload.push(byte);
            } else {
                self.oversized = true;
            }
            self.osc_escape = byte == 27;
        } else if self.escape && byte == b']' {
            self.osc = true;
            self.osc_escape = false;
            self.oversized = false;
            self.payload.clear();
            self.escape = false;
        } else {
            self.escape = byte == 27;
        }
        None
    }
    pub fn advance<T: EventListener>(
        &mut self,
        processor: &mut Processor,
        term: &mut Term<T>,
        state: &mut Integration,
        bytes: &[u8],
    ) {
        if !self.osc && !self.escape && !bytes.contains(&27) {
            // Most PTY chunks are plain text. Keep ANSI forwarding (including
            // a CSI started in the previous chunk), but skip OSC byte framing.
            processor.advance(&mut Observed { term, state }, bytes);
            if state.seen {
                state.trim(term);
            }
            return;
        }
        let mut start = 0;
        for (index, &byte) in bytes.iter().enumerate() {
            if let Some(payload) = self.event(byte) {
                let mut handler = Observed { term, state };
                processor.advance(&mut handler, &bytes[start..=index]);
                if processor.sync_bytes_count() > 0 {
                    // Buffered ANSI has not reached the grid yet. Never stamp
                    // boundaries against an earlier cursor or retain a stale cwd.
                    state.discard_annotations();
                    if payload.starts_with(b"7;") {
                        state.cwd = None;
                    }
                } else {
                    state.osc(&payload, term);
                }
                start = index + 1;
            }
        }
        if start < bytes.len() {
            processor.advance(&mut Observed { term, state }, &bytes[start..]);
        }
        state.trim(term);
    }
}

struct Observed<'a, T: EventListener> {
    term: &'a mut Term<T>,
    state: &'a mut Integration,
}
impl<T: EventListener> Observed<'_, T> {
    fn region(&self) -> (usize, usize) {
        self.state.region.unwrap_or((0, self.term.screen_lines()))
    }
    fn before_linefeed(&mut self) {
        if !self.state.seen || self.term.mode().contains(TermMode::ALT_SCREEN) {
            return;
        }
        let (top, bottom) = self.region();
        if self.term.grid().cursor.point.line.0 + 1 == bottom as i32 {
            if top == 0 && bottom == self.term.screen_lines() {
                self.state.scroll(1);
            } else {
                self.state.discard_annotations();
            }
        }
    }
}

// Every unobserved ANSI operation forwards unchanged to the terminal kernel.
impl<T: EventListener> Handler for Observed<'_, T> {
    fn set_title(&mut self, arg0: Option<String>) {
        self.term.set_title(arg0);
    }
    fn set_cursor_style(&mut self, arg0: Option<ansi::CursorStyle>) {
        self.term.set_cursor_style(arg0);
    }
    fn set_cursor_shape(&mut self, arg0: ansi::CursorShape) {
        self.term.set_cursor_shape(arg0);
    }
    fn goto(&mut self, arg0: i32, arg1: usize) {
        self.term.goto(arg0, arg1);
    }
    fn goto_line(&mut self, arg0: i32) {
        self.term.goto_line(arg0);
    }
    fn goto_col(&mut self, arg0: usize) {
        self.term.goto_col(arg0);
    }
    fn insert_blank(&mut self, arg0: usize) {
        self.term.insert_blank(arg0);
    }
    fn move_up(&mut self, arg0: usize) {
        self.term.move_up(arg0);
    }
    fn move_down(&mut self, arg0: usize) {
        self.term.move_down(arg0);
    }
    fn identify_terminal(&mut self, arg0: Option<char>) {
        self.term.identify_terminal(arg0);
    }
    fn device_status(&mut self, arg0: usize) {
        self.term.device_status(arg0);
    }
    fn move_forward(&mut self, arg0: usize) {
        self.term.move_forward(arg0);
    }
    fn move_backward(&mut self, arg0: usize) {
        self.term.move_backward(arg0);
    }
    fn move_down_and_cr(&mut self, arg0: usize) {
        self.term.move_down_and_cr(arg0);
    }
    fn move_up_and_cr(&mut self, arg0: usize) {
        self.term.move_up_and_cr(arg0);
    }
    fn put_tab(&mut self, arg0: u16) {
        if self.term.grid().cursor.input_needs_wrap
            && self.term.mode().contains(TermMode::LINE_WRAP)
        {
            self.before_linefeed();
        }
        self.term.put_tab(arg0);
    }
    fn backspace(&mut self) {
        self.term.backspace();
    }
    fn carriage_return(&mut self) {
        self.term.carriage_return();
    }
    fn bell(&mut self) {
        self.term.bell();
    }
    fn substitute(&mut self) {
        self.term.substitute();
    }
    fn set_horizontal_tabstop(&mut self) {
        self.term.set_horizontal_tabstop();
    }
    fn erase_chars(&mut self, arg0: usize) {
        self.term.erase_chars(arg0);
    }
    fn delete_chars(&mut self, arg0: usize) {
        self.term.delete_chars(arg0);
    }
    fn move_backward_tabs(&mut self, arg0: u16) {
        self.term.move_backward_tabs(arg0);
    }
    fn move_forward_tabs(&mut self, arg0: u16) {
        self.term.move_forward_tabs(arg0);
    }
    fn save_cursor_position(&mut self) {
        self.term.save_cursor_position();
    }
    fn restore_cursor_position(&mut self) {
        self.term.restore_cursor_position();
    }
    fn clear_line(&mut self, arg0: ansi::LineClearMode) {
        self.term.clear_line(arg0);
    }
    fn clear_tabs(&mut self, arg0: ansi::TabulationClearMode) {
        self.term.clear_tabs(arg0);
    }
    fn set_tabs(&mut self, arg0: u16) {
        self.term.set_tabs(arg0);
    }
    fn terminal_attribute(&mut self, arg0: ansi::Attr) {
        self.term.terminal_attribute(arg0);
    }
    fn set_mode(&mut self, arg0: ansi::Mode) {
        self.term.set_mode(arg0);
    }
    fn unset_mode(&mut self, arg0: ansi::Mode) {
        self.term.unset_mode(arg0);
    }
    fn report_mode(&mut self, arg0: ansi::Mode) {
        self.term.report_mode(arg0);
    }
    fn set_private_mode(&mut self, arg0: ansi::PrivateMode) {
        if arg0 == ansi::PrivateMode::Named(ansi::NamedPrivateMode::ColumnMode) {
            self.state.invalidate();
        }
        self.term.set_private_mode(arg0);
    }
    fn unset_private_mode(&mut self, arg0: ansi::PrivateMode) {
        if arg0 == ansi::PrivateMode::Named(ansi::NamedPrivateMode::ColumnMode) {
            self.state.invalidate();
        }
        self.term.unset_private_mode(arg0);
    }
    fn report_private_mode(&mut self, arg0: ansi::PrivateMode) {
        self.term.report_private_mode(arg0);
    }
    fn set_keypad_application_mode(&mut self) {
        self.term.set_keypad_application_mode();
    }
    fn unset_keypad_application_mode(&mut self) {
        self.term.unset_keypad_application_mode();
    }
    fn set_active_charset(&mut self, arg0: ansi::CharsetIndex) {
        self.term.set_active_charset(arg0);
    }
    fn configure_charset(&mut self, arg0: ansi::CharsetIndex, arg1: ansi::StandardCharset) {
        self.term.configure_charset(arg0, arg1);
    }
    fn set_color(&mut self, arg0: usize, arg1: ansi::Rgb) {
        self.term.set_color(arg0, arg1);
    }
    fn dynamic_color_sequence(&mut self, arg0: String, arg1: usize, arg2: &str) {
        self.term.dynamic_color_sequence(arg0, arg1, arg2);
    }
    fn reset_color(&mut self, arg0: usize) {
        self.term.reset_color(arg0);
    }
    fn clipboard_store(&mut self, arg0: u8, arg1: &[u8]) {
        self.term.clipboard_store(arg0, arg1);
    }
    fn clipboard_load(&mut self, arg0: u8, arg1: &str) {
        self.term.clipboard_load(arg0, arg1);
    }
    fn push_title(&mut self) {
        self.term.push_title();
    }
    fn pop_title(&mut self) {
        self.term.pop_title();
    }
    fn text_area_size_pixels(&mut self) {
        self.term.text_area_size_pixels();
    }
    fn text_area_size_chars(&mut self) {
        self.term.text_area_size_chars();
    }
    fn set_hyperlink(&mut self, arg0: Option<ansi::Hyperlink>) {
        self.term.set_hyperlink(arg0);
    }
    fn report_keyboard_mode(&mut self) {
        self.term.report_keyboard_mode();
    }
    fn push_keyboard_mode(&mut self, arg0: ansi::KeyboardModes) {
        self.term.push_keyboard_mode(arg0);
    }
    fn pop_keyboard_modes(&mut self, arg0: u16) {
        self.term.pop_keyboard_modes(arg0);
    }
    fn set_keyboard_mode(
        &mut self,
        arg0: ansi::KeyboardModes,
        arg1: ansi::KeyboardModesApplyBehavior,
    ) {
        self.term.set_keyboard_mode(arg0, arg1);
    }
    fn set_modify_other_keys(&mut self, arg0: ansi::ModifyOtherKeys) {
        self.term.set_modify_other_keys(arg0);
    }
    fn report_modify_other_keys(&mut self) {
        self.term.report_modify_other_keys();
    }
    fn set_scp(&mut self, arg0: ansi::ScpCharPath, arg1: ansi::ScpUpdateMode) {
        self.term.set_scp(arg0, arg1);
    }
    fn input(&mut self, character: char) {
        if !self.state.seen {
            self.term.input(character);
            return;
        }
        let before = self.term.grid().cursor.point;
        let wrapping = self.term.grid().cursor.input_needs_wrap;
        self.term.input(character);
        if !self.term.mode().contains(TermMode::ALT_SCREEN)
            && before.line.0 + 1 == self.region().1 as i32
        {
            let after = self.term.grid().cursor.point;
            let scrolled = after.line == before.line
                && (after.column < before.column
                    || (wrapping && !self.term.grid().cursor.input_needs_wrap));
            if scrolled {
                if self.region() == (0, self.term.screen_lines()) {
                    self.state.scroll(1);
                } else {
                    self.state.discard_annotations();
                }
            }
            if self.term.columns() <= 3 && (wrapping || before.column.0 + 1 >= self.term.columns())
            {
                self.state.discard_annotations();
            }
        }
    }
    fn linefeed(&mut self) {
        self.before_linefeed();
        self.term.linefeed();
    }
    fn newline(&mut self) {
        self.before_linefeed();
        self.term.newline();
    }
    fn scroll_up(&mut self, rows: usize) {
        if self.state.seen && !self.term.mode().contains(TermMode::ALT_SCREEN) {
            if self.region() == (0, self.term.screen_lines()) {
                self.state.scroll(rows.min(self.term.screen_lines()));
            } else {
                self.state.discard_annotations();
            }
        }
        self.term.scroll_up(rows);
    }
    fn scroll_down(&mut self, rows: usize) {
        if !self.term.mode().contains(TermMode::ALT_SCREEN) {
            self.state.discard_annotations();
        }
        self.term.scroll_down(rows);
    }
    fn insert_blank_lines(&mut self, rows: usize) {
        if !self.term.mode().contains(TermMode::ALT_SCREEN) {
            self.state.discard_annotations();
        }
        self.term.insert_blank_lines(rows);
    }
    fn delete_lines(&mut self, rows: usize) {
        if !self.term.mode().contains(TermMode::ALT_SCREEN) {
            self.state.discard_annotations();
        }
        self.term.delete_lines(rows);
    }
    fn clear_screen(&mut self, mode: ansi::ClearMode) {
        if !self.term.mode().contains(TermMode::ALT_SCREEN)
            && matches!(mode, ansi::ClearMode::All | ansi::ClearMode::Saved)
        {
            self.state.discard_annotations();
        }
        self.term.clear_screen(mode);
    }
    fn reset_state(&mut self) {
        self.state.invalidate();
        self.state.cwd = None;
        self.term.reset_state();
    }
    fn reverse_index(&mut self) {
        if !self.term.mode().contains(TermMode::ALT_SCREEN)
            && self.term.grid().cursor.point.line.0 == self.region().0 as i32
        {
            self.state.discard_annotations();
        }
        self.term.reverse_index();
    }
    fn set_scrolling_region(&mut self, top: usize, bottom: Option<usize>) {
        self.term.set_scrolling_region(top, bottom);
        let bottom = bottom
            .unwrap_or(self.term.screen_lines())
            .min(self.term.screen_lines());
        let top = top.saturating_sub(1).min(self.term.screen_lines());
        if top < bottom {
            self.state.region = Some((top, bottom));
        }
    }
    fn decaln(&mut self) {
        self.state.discard_annotations();
        self.term.decaln();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::{event::Event, term::Config};
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
    fn terminal() -> Term<Listener> {
        Term::new(
            Config {
                scrolling_history: 6,
                ..Config::default()
            },
            &Size,
            Listener,
        )
    }
    fn feed(term: &mut Term<Listener>, state: &mut Integration, text: &[u8]) {
        let mut stream = Stream::default();
        let mut processor: Processor = Processor::new();
        // Deliberately fragment every marker and UTF-8 codepoint across reads.
        for byte in text {
            stream.advance(&mut processor, term, state, &[*byte]);
        }
    }
    #[test]
    fn explicit_boundaries_copy_rendered_output_without_prompt_or_command() {
        let mut term = terminal();
        let mut state = Integration::default();
        feed(&mut term, &mut state, b"\x1b]133;A\x07$ \x1b]133;B\x07cmd\r\n\x1b]133;C\x07out one\r\nout two\r\n\x1b]133;D;0\x07\x1b]133;A\x07$ \x1b]133;B\x07");
        assert_eq!(state.last_output(&term).unwrap(), "out one\nout two\n");
        assert!(state.navigate(&mut term, true).is_ok());
        assert!(state.navigate(&mut term, false).is_ok());
    }
    #[test]
    fn bounded_markers_and_scrollback_trim_never_copy_unrelated_content() {
        let mut term = terminal();
        let mut state = Integration::default();
        let mut text = String::from("\x1b]133;A\x07$ \x1b]133;B\x07run\r\n\x1b]133;C\x07");
        text.push_str(&"a long line that wraps\r\n".repeat(30));
        text.push_str("\x1b]133;D;0\x07\x1b]133;A\x07$ \x1b]133;B\x07");
        feed(&mut term, &mut state, text.as_bytes());
        assert!(state.last_output(&term).is_err());
        assert!(
            state
                .prompts
                .iter()
                .all(|point| point.line >= term.grid().topmost_line())
        );
        let text = "\x1b]133;A\x07$ \x1b]133;B\x07\r\n".repeat(200);
        feed(&mut term, &mut state, text.as_bytes());
        assert!(state.prompts.len() <= MAX_PROMPTS);
    }
    #[test]
    fn no_markers_and_malformed_markers_have_explicit_fallback() {
        let mut term = terminal();
        let mut state = Integration::default();
        feed(
            &mut term,
            &mut state,
            b"plain\r\n\x1b]133;C\x07unmarked\r\n\x1b]133;D\x07\x1b]133;BAD\x07",
        );
        assert!(!state.seen);
        assert!(
            state
                .last_output(&term)
                .unwrap_err()
                .to_string()
                .contains("Source Opterm")
        );
        assert!(state.navigate(&mut term, true).is_err());
    }
    #[test]
    fn cwd_metadata_requires_local_host_absolute_existing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let child = dir.path().join("space folder");
        std::fs::create_dir(&child).unwrap();
        let path = child.to_string_lossy().replace(' ', "%20");
        assert_eq!(
            local_directory(&format!("file://localhost{path}")),
            Some(child.to_string_lossy().into_owned())
        );
        assert!(local_directory(&format!("file://unrelated.invalid{path}")).is_none());
        assert!(local_directory("file://localhost/does-not-exist-opterm-test").is_none());
        assert!(local_directory("file://localhost/tmp%00").is_none());
        assert!(local_directory("file://localhost/tmp%XZ").is_none());
        assert!(local_directory("file://user@localhost/tmp").is_none());
        let mut term = terminal();
        let mut state = Integration::default();
        feed(
            &mut term,
            &mut state,
            format!("\x1b]7;file://localhost{path}\x1b\\").as_bytes(),
        );
        assert!(state.cwd.is_some());
        state.invalidate();
        assert!(state.cwd.is_some());
        feed(
            &mut term,
            &mut state,
            b"\x1b]7;file://remote.invalid/tmp\x07",
        );
        assert!(state.cwd.is_none());
    }
    #[test]
    fn wrapped_unicode_output_and_new_command_boundaries_are_exact() {
        let mut term = terminal();
        let mut state = Integration::default();
        feed(&mut term, &mut state, "\x1b]133;A\x07$ \x1b]133;B\x07x\r\n\x1b]133;C\x07abcdefghijk界e\u{301}\r\n\x1b]133;D\x07".as_bytes());
        assert_eq!(state.last_output(&term).unwrap(), "abcdefghijk界e\u{301}\n");
        feed(
            &mut term,
            &mut state,
            b"\x1b]133;A\x07$ \x1b]133;B\x07x\r\n\x1b]133;C\x07",
        );
        assert!(state.last_output(&term).is_err());
        feed(&mut term, &mut state, b"\x1b]133;D;0\x07");
        assert_eq!(state.last_output(&term).unwrap(), "");
    }
    #[test]
    fn observed_ansi_preserves_kernel_grid_modes_and_cursor() {
        let mut plain = terminal();
        let mut observed = terminal();
        let mut processor: Processor = Processor::new();
        let mut observed_processor = Processor::new();
        let mut stream = Stream::default();
        let mut state = Integration::default();
        for text in [
            "abc\tdef\r\nwide 界e\u{301}\r\n",
            "\x1b[31;1mcolor\x1b[0m\x1b]8;id=conformance;https://example.com\x07link\x1b]8;;\x07",
            "\x1b[2;3H\x1b[2@XX\x1b[P\x1b[2X\x1b[K\x1b7\x1b[4;1Hlast column!\t\x1b8",
            "\x1b[2;4r\x1b[4;1Hscroll\r\n\x1b[1S\x1b[1T\x1b[1L\x1b[1M\x1bM\x1b[r",
            "\x1b[?1049h\x1b[2Jalternate\x1b[?1000h\x1b[?1006h\x1b[?2004h\x1b[?1049l",
            "\x1b[?3h\x1b[?3l\x1b[?6h\x1b[?6l\x1b[20h\n\x1b[20l",
            "\x1b[4munder\x1b[0m\x1b[2J\x1b[3J\x1b#8\x1bcfinal",
        ] {
            for byte in text.as_bytes() {
                processor.advance(&mut plain, &[*byte]);
                stream.advance(&mut observed_processor, &mut observed, &mut state, &[*byte]);
            }
            assert_eq!(plain.mode(), observed.mode());
            assert_eq!(plain.grid().cursor.point, observed.grid().cursor.point);
            assert_eq!(
                plain.grid().cursor.input_needs_wrap,
                observed.grid().cursor.input_needs_wrap
            );
            assert_eq!(plain.grid().topmost_line(), observed.grid().topmost_line());
            for row in plain.grid().topmost_line().0..plain.screen_lines() as i32 {
                for column in 0..plain.columns() {
                    assert_eq!(
                        plain.grid()[Line(row)][Column(column)],
                        observed.grid()[Line(row)][Column(column)]
                    );
                }
            }
        }
    }
    #[test]
    fn wrapped_input_and_tab_scroll_preserve_output_boundaries() {
        let mut term = terminal();
        let mut state = Integration::default();
        feed(&mut term, &mut state, b"\r\n\r\n\x1b]133;A\x07$ \x1b]133;B\x07a wrapped command that scrolls\r\n\x1b]133;C\x07abcdefghijkl\tmore\r\n\x1b]133;D\x07\x1b]133;A\x07$ \x1b]133;B\x07");
        assert_eq!(state.last_output(&term).unwrap(), "abcdefghijklmore\n");
        assert!(state.navigate(&mut term, true).is_ok());
        assert!(state.navigate(&mut term, false).is_ok());
    }
    #[test]
    fn synchronized_markers_never_copy_output_from_an_earlier_cursor() {
        let mut term = terminal();
        let mut state = Integration::default();
        feed(&mut term, &mut state, b"\x1b[?2026h\x1b]133;A\x07$ \x1b]133;B\x07cmd\r\n\x1b]133;C\x07buffered\r\n\x1b]133;D\x07\x1b[?2026l\x1b]133;A\x07$ \x1b]133;B\x07");
        assert!(state.last_output(&term).is_err());
        feed(
            &mut term,
            &mut state,
            b"next\r\n\x1b]133;C\x07recovered\r\n\x1b]133;D\x07",
        );
        assert_eq!(state.last_output(&term).unwrap(), "recovered\n");
    }
    #[test]
    fn destructive_partial_scroll_keeps_tracking_the_actual_kernel_region() {
        let mut term = terminal();
        let mut state = Integration::default();
        feed(
            &mut term,
            &mut state,
            b"\x1b[2;3r\x1b[3;1H\x1b]133;A\x07$ \x1b]133;B\x07x\r\n",
        );
        assert_eq!(state.region, Some((1, 3)));
        assert!(state.prompts.is_empty());
        feed(
            &mut term,
            &mut state,
            b"\x1b]133;A\x07$ \x1b]133;B\x07x\r\n\x1b]133;C\x07output\r\n\x1b]133;D\x07",
        );
        assert_eq!(state.region, Some((1, 3)));
        assert!(state.last_output(&term).is_err());
    }
}
