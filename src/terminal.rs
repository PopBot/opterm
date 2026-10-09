//! PTY ownership and terminal state, separate from the provisional renderer.
use std::{
    io::{Read, Write},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread,
};

use alacritty_terminal::{
    event::{Event, EventListener, WindowSize},
    grid::{Dimensions, Scroll},
    index::{Column, Point, Side},
    selection::{Selection, SelectionType},
    term::{Config, Osc52, Term, TermMode, cell::Flags, viewport_to_point},
    vte::ansi::{Color, Processor, Rgb},
};
use anyhow::{Context, Result};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

pub const BACKGROUND: [u8; 3] = [31, 31, 33];
pub const FOREGROUND: [u8; 3] = [225, 225, 226];
const ANSI: [[u8; 3]; 16] = [
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
type Writer = Arc<Mutex<Box<dyn Write + Send>>>;
type Wake = Arc<dyn Fn() + Send + Sync>;

#[derive(Clone)]
struct Listener {
    writer: Writer,
    status: Arc<Mutex<Status>>,
    size: Arc<Mutex<PtySize>>,
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
                let [r, g, b] = palette(index);
                Some(format(Rgb { r, g, b }))
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
            && let Err(error) = self.writer.lock().unwrap().write_all(reply.as_bytes())
        {
            self.status.lock().unwrap().error = Some(format!("Terminal reply failed: {error}"));
        }
    }
}

#[derive(Clone)]
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
pub struct Screen {
    pub cells: Vec<Cell>,
    pub cursor: Option<(usize, usize)>,
    pub mode: TermMode,
    pub title: String,
    pub exited: bool,
    pub error: Option<String>,
}
impl Screen {
    pub fn text(&self) -> String {
        let mut result = String::new();
        let mut row = 0;
        for cell in &self.cells {
            while row < cell.row {
                result.push('\n');
                row += 1;
            }
            result.push_str(&cell.text);
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
}
impl Session {
    pub fn spawn(program: &[String], wake: impl Fn() + Send + Sync + 'static) -> Result<Self> {
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
        let mut child = pair
            .slave
            .spawn_command(command)
            .context("Start shell/program")?;
        let killer = child.clone_killer();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader()?;
        let writer = Arc::new(Mutex::new(pair.master.take_writer()?));
        let size = Arc::new(Mutex::new(size));
        let status = Arc::new(Mutex::new(Status::default()));
        let listener = Listener {
            writer: writer.clone(),
            status: status.clone(),
            size: size.clone(),
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
        let term = terminal.clone();
        let read_status = status.clone();
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
                            processor.advance(&mut *term.lock().unwrap(), &buffer[..count]);
                            changed.fetch_add(1, Ordering::Relaxed);
                            notify();
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        // Linux PTYs return EIO at EOF after the slave closes.
                        Err(error) if cfg!(unix) && error.raw_os_error() == Some(5) => break,
                        Err(error) => {
                            read_status.lock().unwrap().error =
                                Some(format!("PTY read failed: {error}"));
                            notify();
                            break;
                        }
                    }
                }
            })?;
        let wait_status = status.clone();
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
        })
    }

    pub fn write(&self, bytes: &[u8]) -> Result<()> {
        self.terminal.lock().unwrap().scroll_display(Scroll::Bottom);
        self.writer
            .lock()
            .unwrap()
            .write_all(bytes)
            .context("Write terminal input")
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
        (self.wake)();
        Ok(())
    }

    pub fn scroll(&self, lines: i32) {
        self.terminal
            .lock()
            .unwrap()
            .scroll_display(Scroll::Delta(lines));
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
        (self.wake)();
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

    pub fn screen(&self) -> Screen {
        let term = self.terminal.lock().unwrap();
        let content = term.renderable_content();
        let color = |color: Color| match color {
            Color::Spec(rgb) => [rgb.r, rgb.g, rgb.b],
            Color::Indexed(index) => content.colors[index as usize]
                .map(|rgb| [rgb.r, rgb.g, rgb.b])
                .unwrap_or_else(|| palette(index as usize)),
            Color::Named(name) => content.colors[name]
                .map(|rgb| [rgb.r, rgb.g, rgb.b])
                .unwrap_or_else(|| palette(name as usize)),
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
        // First development slice only. Move ownership into the independent
        // service before promising detach/reattach or desktop-crash survival.
        let _ = self.killer.kill();
    }
}
