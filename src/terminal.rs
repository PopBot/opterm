//! PTY ownership and terminal state, separate from the provisional renderer.
use std::{
    io::{Read, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::{SyncSender, sync_channel},
    },
    thread,
};

use crate::profiles::{Launch, Theme};
use alacritty_terminal::{
    event::{Event, EventListener, WindowSize},
    grid::{Dimensions, Scroll},
    index::{Boundary, Column, Direction, Point, Side},
    selection::{Selection, SelectionType},
    term::{Config, Osc52, Term, TermMode, cell::Flags, search::RegexSearch, viewport_to_point},
    vte::ansi::{Color, Processor, Rgb},
};
use anyhow::{Context, Result};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

pub const BACKGROUND: [u8; 3] = [31, 31, 33];
pub const FOREGROUND: [u8; 3] = [225, 225, 226];
pub const ANSI: [[u8; 3]; 16] = [
    [25, 25, 27],
    [205, 85, 85],
    [110, 185, 120],
    [215, 185, 110],
    [115, 155, 215],
    [180, 135, 200],
    [110, 185, 195],
    [205, 205, 207],
    [115, 115, 120],
    [240, 120, 120],
    [150, 215, 155],
    [235, 210, 145],
    [155, 185, 240],
    [210, 170, 235],
    [145, 215, 220],
    [245, 245, 246],
];

pub fn palette(index: usize) -> [u8; 3] {
    match index {
        0..16 => ANSI[index],
        16..232 => {
            let n = index - 16;
            let level = |v: usize| if v == 0 { 0 } else { (55 + v * 40) as u8 };
            [level(n / 36), level(n / 6 % 6), level(n % 6)]
        }
        232..256 => [8 + (index as u8 - 232) * 10; 3],
        256 => FOREGROUND,
        257 => BACKGROUND,
        258 | 267 => [245, 245, 246],
        259..267 => ANSI[index - 259].map(|v| (f32::from(v) * 0.66) as u8),
        268 => FOREGROUND.map(|v| (f32::from(v) * 0.66) as u8),
        _ => FOREGROUND,
    }
}

#[derive(Clone, Copy)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
}
impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows as usize
    }
    fn screen_lines(&self) -> usize {
        self.rows as usize
    }
    fn columns(&self) -> usize {
        self.cols as usize
    }
}

#[derive(Default)]
struct Status {
    title: String,
    exited: bool,
    error: Option<String>,
}
type Writer = SyncSender<Vec<u8>>;
type Wake = Arc<dyn Fn() + Send + Sync>;
type ColorReply = (usize, Arc<dyn Fn(Rgb) -> String + Send + Sync>);
type ColorReplies = Arc<Mutex<Vec<ColorReply>>>;

#[derive(Clone)]
struct Listener {
    writer: Writer,
    status: Arc<Mutex<Status>>,
    size: Arc<Mutex<PtySize>>,
    color_replies: ColorReplies,
}
impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let reply = match event {
            Event::Title(title) => {
                self.status.lock().unwrap().title = title;
                None
            }
            Event::ResetTitle => {
                self.status.lock().unwrap().title.clear();
                None
            }
            Event::PtyWrite(text) => Some(text),
            Event::ColorRequest(index, format) => {
                let mut replies = self.color_replies.lock().unwrap();
                if replies.len() < 64 {
                    replies.push((index, format));
                } else {
                    self.status.lock().unwrap().error =
                        Some("Too many pending terminal color queries".into());
                }
                None
            }
            Event::TextAreaSizeRequest(format) => {
                let size = self.size.lock().unwrap();
                Some(format(WindowSize {
                    num_lines: size.rows,
                    num_cols: size.cols,
                    cell_width: size.pixel_width / size.cols,
                    cell_height: size.pixel_height / size.rows,
                }))
            }
            // No output-driven clipboard or desktop side effects in this slice.
            _ => None,
        };
        if let Some(reply) = reply
            && let Err(error) = self.writer.try_send(reply.into_bytes())
        {
            self.status.lock().unwrap().error = Some(format!("Terminal reply failed: {error}"));
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Cell {
    pub row: usize,
    pub col: usize,
    pub text: String,
    pub fg: [u8; 3],
    pub bg: [u8; 3],
    pub wide: bool,
    pub underline: bool,
    pub selected: bool,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Screen {
    pub cells: Vec<Cell>,
    #[serde(default = "default_background")]
    pub background: [u8; 3],
    #[serde(default = "default_cursor_color")]
    pub cursor_color: [u8; 3],
    #[serde(default = "default_selection_color")]
    pub selection_color: [u8; 3],
    pub cursor: Option<(usize, usize)>,
    #[serde(with = "mode_bits")]
    pub mode: TermMode,
    pub title: String,
    pub exited: bool,
    pub error: Option<String>,
}
fn default_background() -> [u8; 3] {
    BACKGROUND
}
fn default_cursor_color() -> [u8; 3] {
    [235, 235, 235]
}
fn default_selection_color() -> [u8; 3] {
    [65, 83, 112]
}
impl Screen {
    pub fn empty() -> Self {
        Self {
            cells: Vec::new(),
            background: BACKGROUND,
            cursor_color: default_cursor_color(),
            selection_color: default_selection_color(),
            cursor: None,
            mode: TermMode::empty(),
            title: String::new(),
            exited: false,
            error: None,
        }
    }
    pub fn text(&self) -> String {
        let mut result = String::new();
        let mut row = 0;
        let mut column = 0;
        for cell in &self.cells {
            while row < cell.row {
                result.push('\n');
                row += 1;
                column = 0;
            }
            while column < cell.col {
                result.push(' ');
                column += 1;
            }
            result.push_str(&cell.text);
            column = cell.col + if cell.wide { 2 } else { 1 };
        }
        result
    }
}

pub struct Session {
    terminal: Arc<Mutex<Term<Listener>>>,
    writer: Writer,
    status: Arc<Mutex<Status>>,
    size: Arc<Mutex<PtySize>>,
    master: Box<dyn MasterPty + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    pub generation: Arc<AtomicU64>,
    wake: Wake,
    theme: Arc<Mutex<Theme>>,
}
impl Session {
    pub fn spawn(program: &[String], wake: impl Fn() + Send + Sync + 'static) -> Result<Self> {
        Self::spawn_configured(&Launch::legacy(program), wake)
    }
    pub fn spawn_configured(
        launch: &Launch,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Result<Self> {
        let directory = launch.directory()?;
        let program = launch.program();
        let size = PtySize {
            rows: 32,
            cols: 100,
            pixel_width: 800,
            pixel_height: 540,
        };
        let pair = native_pty_system()
            .openpty(size)
            .context("Open native PTY")?;
        let mut command = if let Some(program) = program.first() {
            CommandBuilder::new(program)
        } else {
            CommandBuilder::new_default_prog()
        };
        if !program.is_empty() {
            command.args(&program[1..]);
        }
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        // Tool launchers commonly suppress color for captured output. A real
        // interactive PTY should advertise its own color capability instead.
        command.env_remove("NO_COLOR");
        command.cwd(directory);
        for env in &launch.profile.environment {
            command.env(&env.name, &env.value);
        }
        let mut child = pair
            .slave
            .spawn_command(command)
            .context("Start shell/program")?;
        let killer = child.clone_killer();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let mut pty_writer = pair.master.take_writer()?;
        let (writer, input) = sync_channel::<Vec<u8>>(64);
        let size = Arc::new(Mutex::new(size));
        let status = Arc::new(Mutex::new(Status::default()));
        let theme = Arc::new(Mutex::new(launch.theme.clone()));
        let color_replies: ColorReplies = Arc::default();
        let listener = Listener {
            writer: writer.clone(),
            status: status.clone(),
            size: size.clone(),
            color_replies: color_replies.clone(),
        };
        let terminal = Arc::new(Mutex::new(Term::new(
            Config {
                scrolling_history: 10_000,
                osc52: Osc52::Disabled,
                ..Config::default()
            },
            &Size {
                cols: 100,
                rows: 32,
            },
            listener,
        )));
        let generation = Arc::new(AtomicU64::new(0));
        let wake: Wake = Arc::new(wake);
        let write_status = status.clone();
        let write_changed = generation.clone();
        let notify = wake.clone();
        thread::Builder::new()
            .name("opterm-pty-write".into())
            .spawn(move || {
                for bytes in input {
                    if let Err(error) = pty_writer.write_all(&bytes) {
                        write_status.lock().unwrap().error =
                            Some(format!("PTY write failed: {error}"));
                        write_changed.fetch_add(1, Ordering::Relaxed);
                        notify();
                        break;
                    }
                }
            })?;
        let term = terminal.clone();
        let read_status = status.clone();
        let read_theme = theme.clone();
        let read_writer = writer.clone();
        let changed = generation.clone();
        let notify = wake.clone();
        thread::Builder::new()
            .name("opterm-pty-read".into())
            .spawn(move || {
                let mut processor: Processor = Processor::new();
                let mut buffer = [0; 8192];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(count) => {
                            let replies = {
                                let mut terminal = term.lock().unwrap();
                                processor.advance(&mut *terminal, &buffer[..count]);
                                let pending = std::mem::take(&mut *color_replies.lock().unwrap());
                                let content = terminal.renderable_content();
                                let theme = read_theme.lock().unwrap();
                                pending
                                    .into_iter()
                                    .map(|(index, format)| {
                                        let [r, g, b] = content.colors[index]
                                            .map(|rgb| [rgb.r, rgb.g, rgb.b])
                                            .unwrap_or_else(|| theme.palette(index));
                                        format(Rgb { r, g, b }).into_bytes()
                                    })
                                    .collect::<Vec<_>>()
                            };
                            for reply in replies {
                                if let Err(error) = read_writer.try_send(reply) {
                                    read_status.lock().unwrap().error =
                                        Some(format!("Terminal color reply failed: {error}"));
                                }
                            }
                            changed.fetch_add(1, Ordering::Relaxed);
                            notify();
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        // Linux PTYs return EIO at EOF after the slave closes.
                        Err(error) if cfg!(unix) && error.raw_os_error() == Some(5) => break,
                        Err(error) => {
                            read_status.lock().unwrap().error =
                                Some(format!("PTY read failed: {error}"));
                            changed.fetch_add(1, Ordering::Relaxed);
                            notify();
                            break;
                        }
                    }
                }
            })?;
        let wait_status = status.clone();
        let changed = generation.clone();
        let notify = wake.clone();
        thread::Builder::new()
            .name("opterm-child-wait".into())
            .spawn(move || {
                let result = child.wait();
                let mut status = wait_status.lock().unwrap();
                status.exited = true;
                if let Err(error) = result {
                    status.error = Some(format!("Wait failed: {error}"));
                }
                drop(status);
                changed.fetch_add(1, Ordering::Relaxed);
                notify();
            })?;
        Ok(Self {
            terminal,
            writer,
            status,
            size,
            master: pair.master,
            killer,
            generation,
            wake,
            theme,
        })
    }

    pub fn set_theme(&self, theme: Theme) {
        let mut current = self.theme.lock().unwrap();
        if *current != theme {
            *current = theme;
            self.generation.fetch_add(1, Ordering::Relaxed);
            (self.wake)();
        }
    }

    pub fn write(&self, bytes: &[u8]) -> Result<()> {
        anyhow::ensure!(
            bytes.len() <= 1024 * 1024,
            "Input exceeds the 1 MiB prototype limit"
        );
        self.terminal.lock().unwrap().scroll_display(Scroll::Bottom);
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.writer
            .try_send(bytes.to_vec())
            .context("Queue terminal input")
    }

    pub fn paste(&self, text: &str) -> Result<()> {
        let bracketed = self
            .terminal
            .lock()
            .unwrap()
            .mode()
            .contains(TermMode::BRACKETED_PASTE);
        let mut bytes = Vec::with_capacity(text.len() + 12);
        if bracketed {
            bytes.extend_from_slice(b"\x1b[200~");
        }
        bytes.extend_from_slice(text.as_bytes());
        if bracketed {
            bytes.extend_from_slice(b"\x1b[201~");
        }
        self.write(&bytes)
    }

    pub fn resize(&self, cols: u16, rows: u16, cell_width: f32, cell_height: f32) -> Result<()> {
        let cols = cols.clamp(2, 500);
        let rows = rows.clamp(1, 200);
        let new = PtySize {
            cols,
            rows,
            pixel_width: (cols as f32 * cell_width).min(u16::MAX as f32) as u16,
            pixel_height: (rows as f32 * cell_height).min(u16::MAX as f32) as u16,
        };
        if *self.size.lock().unwrap() == new {
            return Ok(());
        }
        self.master.resize(new).context("Resize native PTY")?;
        self.terminal.lock().unwrap().resize(Size { cols, rows });
        *self.size.lock().unwrap() = new;
        self.generation.fetch_add(1, Ordering::Relaxed);
        (self.wake)();
        Ok(())
    }

    pub fn scroll(&self, lines: i32) {
        self.terminal
            .lock()
            .unwrap()
            .scroll_display(Scroll::Delta(lines));
        self.generation.fetch_add(1, Ordering::Relaxed);
        (self.wake)();
    }

    pub fn select(&self, row: usize, col: usize, start: bool) {
        let mut term = self.terminal.lock().unwrap();
        let point = viewport_to_point(
            term.grid().display_offset(),
            Point::new(
                row.min(term.screen_lines() - 1),
                Column(col.min(term.columns() - 1)),
            ),
        );
        if start {
            term.selection = Some(Selection::new(SelectionType::Simple, point, Side::Left));
        } else if let Some(selection) = &mut term.selection {
            selection.update(point, Side::Right);
        }
        self.generation.fetch_add(1, Ordering::Relaxed);
        (self.wake)();
    }

    /// Search only the terminal's bounded live grid; no transcript or index is built.
    pub fn search(&self, query: &str, backwards: bool, reset: bool) -> Result<bool> {
        anyhow::ensure!(query.len() <= 4096, "Search text exceeds 4096 bytes");
        let mut term = self.terminal.lock().unwrap();
        let found = if query.is_empty() {
            term.selection = None;
            false
        } else {
            // Explicit case sensitivity avoids the parser's smart-case default.
            let mut literal = String::with_capacity(query.len() * 2 + 7);
            literal.push_str("(?-i:");
            for c in query.chars() {
                if "\\.+*?()|[]{}^$".contains(c) {
                    literal.push('\\');
                }
                literal.push(c);
            }
            literal.push(')');
            let mut regex = RegexSearch::new(&literal)
                .map_err(|error| anyhow::anyhow!("Build search: {error}"))?;
            let direction = if backwards {
                Direction::Left
            } else {
                Direction::Right
            };
            let selected = (!reset)
                .then(|| {
                    term.selection
                        .as_ref()
                        .and_then(|selection| selection.to_range(&term))
                })
                .flatten();
            let origin = if let Some(selected) = selected {
                if backwards {
                    selected.start.sub(&*term, Boundary::None, 1)
                } else {
                    selected.end.add(&*term, Boundary::None, 1)
                }
            } else {
                let row = if backwards {
                    term.screen_lines() - 1
                } else {
                    0
                };
                let col = if backwards { term.columns() - 1 } else { 0 };
                viewport_to_point(term.grid().display_offset(), Point::new(row, Column(col)))
            };
            let side = if backwards { Side::Right } else { Side::Left };
            if let Some(found) = term.search_next(&mut regex, origin, direction, side, None) {
                let mut selection =
                    Selection::new(SelectionType::Simple, *found.start(), Side::Left);
                selection.update(*found.end(), Side::Right);
                term.selection = Some(selection);
                term.scroll_to_point(*found.start());
                true
            } else {
                term.selection = None;
                false
            }
        };
        drop(term);
        self.generation.fetch_add(1, Ordering::Relaxed);
        (self.wake)();
        Ok(found)
    }

    pub fn selected_text(&self) -> Option<String> {
        self.terminal.lock().unwrap().selection_to_string()
    }

    pub fn title(&self) -> String {
        self.status.lock().unwrap().title.clone()
    }

    pub fn mode(&self) -> TermMode {
        *self.terminal.lock().unwrap().mode()
    }

    pub fn exited(&self) -> bool {
        self.status.lock().unwrap().exited
    }

    pub fn screen(&self) -> Screen {
        let term = self.terminal.lock().unwrap();
        let content = term.renderable_content();
        let theme = self.theme.lock().unwrap().clone();
        let color = |color: Color| match color {
            Color::Spec(rgb) => [rgb.r, rgb.g, rgb.b],
            Color::Indexed(index) => content.colors[index as usize]
                .map(|rgb| [rgb.r, rgb.g, rgb.b])
                .unwrap_or_else(|| theme.palette(index as usize)),
            Color::Named(name) => content.colors[name]
                .map(|rgb| [rgb.r, rgb.g, rgb.b])
                .unwrap_or_else(|| theme.palette(name as usize)),
        };
        let cursor = content
            .mode
            .contains(TermMode::SHOW_CURSOR)
            .then(|| {
                alacritty_terminal::term::point_to_viewport(
                    content.display_offset,
                    content.cursor.point,
                )
            })
            .flatten()
            .filter(|p| p.line < term.screen_lines())
            .map(|p| (p.line, p.column.0));
        let mode = content.mode;
        let cells = content
            .display_iter
            .filter_map(|indexed| {
                let point = alacritty_terminal::term::point_to_viewport(
                    content.display_offset,
                    indexed.point,
                )?;
                let cell = indexed.cell;
                if cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    return None;
                }
                let mut fg = color(cell.fg);
                let mut bg = color(cell.bg);
                if cell.flags.contains(Flags::INVERSE) {
                    std::mem::swap(&mut fg, &mut bg);
                }
                if cell.flags.contains(Flags::DIM) {
                    fg = fg.map(|v| (f32::from(v) * 0.66) as u8);
                }
                let mut text = if cell.flags.contains(Flags::HIDDEN) {
                    " ".into()
                } else {
                    cell.c.to_string()
                };
                if let Some(chars) = cell.zerowidth() {
                    text.extend(chars);
                }
                Some(Cell {
                    row: point.line,
                    col: point.column.0,
                    text,
                    fg,
                    bg,
                    wide: cell.flags.contains(Flags::WIDE_CHAR),
                    underline: cell.flags.intersects(Flags::ALL_UNDERLINES),
                    selected: content
                        .selection
                        .is_some_and(|selection| selection.contains(indexed.point)),
                })
            })
            .collect();
        let status = self.status.lock().unwrap();
        Screen {
            cells,
            background: color(Color::Named(
                alacritty_terminal::vte::ansi::NamedColor::Background,
            )),
            cursor_color: color(Color::Named(
                alacritty_terminal::vte::ansi::NamedColor::Cursor,
            )),
            selection_color: theme.selection.0,
            cursor,
            mode,
            title: status.title.clone(),
            exited: status.exited,
            error: status.error.clone(),
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Only the background service owns these sessions in the desktop path.
        // Dropping an explicit service-owned session ends its child.
        let _ = self.killer.kill();
    }
}

mod mode_bits {
    use alacritty_terminal::term::TermMode;
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(mode: &TermMode, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(mode.bits())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<TermMode, D::Error> {
        Ok(TermMode::from_bits_retain(u32::deserialize(deserializer)?))
    }
}
