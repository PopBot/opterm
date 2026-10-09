#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod input;

use eframe::egui::{self, Align2, Color32, FontId, Key, Pos2, Rect, Sense, Vec2};
use opterm::terminal::{BACKGROUND, Session};

struct Tab {
    session: Session,
    name: Option<String>,
}
struct App {
    tabs: Vec<Tab>,
    active: usize,
    program: Vec<String>,
    error: Option<String>,
    rename: Option<usize>,
    rename_text: String,
    paste_requested: bool,
}

fn install_font(ctx: &egui::Context) {
    let mut database = fontdb::Database::new();
    database.load_system_fonts();
    let preferred = if cfg!(target_os = "macos") {
        "Menlo"
    } else if cfg!(target_os = "windows") {
        "Consolas"
    } else {
        "DejaVu Sans Mono"
    };
    if let Some(id) = database.query(&fontdb::Query {
        families: &[fontdb::Family::Name(preferred), fontdb::Family::Monospace],
        ..fontdb::Query::default()
    }) {
        database.with_face_data(id, |bytes, _| {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "system-monospace".into(),
                egui::FontData::from_owned(bytes.to_vec()).into(),
            );
            fonts
                .families
                .get_mut(&egui::FontFamily::Monospace)
                .unwrap()
                .insert(0, "system-monospace".into());
            ctx.set_fonts(fonts);
        });
    }
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, program: Vec<String>) -> Self {
        install_font(&cc.egui_ctx);
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::light();
        style.spacing.button_padding = Vec2::new(10.0, 3.0);
        style.spacing.item_spacing = Vec2::new(3.0, 0.0);
        style.visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
        style.visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
        cc.egui_ctx.set_style(style);
        let mut app = Self {
            tabs: Vec::new(),
            active: 0,
            program,
            error: None,
            rename: None,
            rename_text: String::new(),
            paste_requested: false,
        };
        app.new_tab(&cc.egui_ctx);
        app
    }

    fn new_tab(&mut self, ctx: &egui::Context) {
        let notify = ctx.clone();
        match Session::spawn(&self.program, move || notify.request_repaint()) {
            Ok(session) => {
                self.tabs.push(Tab {
                    session,
                    name: None,
                });
                self.active = self.tabs.len() - 1;
            }
            Err(error) => self.error = Some(format!("{error:#}")),
        }
    }

    fn copy(&self, ctx: &egui::Context) {
        if let Some(tab) = self.tabs.get(self.active)
            && let Some(text) = tab.session.selected_text()
        {
            ctx.copy_text(text);
        }
    }

    fn tab_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("tabs")
            .exact_height(30.0)
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(196, 196, 198))
                    .inner_margin(egui::Margin::symmetric(6, 3)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (index, tab) in self.tabs.iter().enumerate() {
                        let automatic_title = tab.session.title();
                        let title = tab
                            .name
                            .as_deref()
                            .unwrap_or(if automatic_title.is_empty() {
                                "Shell"
                            } else {
                                &automatic_title
                            });
                        let title: String =
                            title.chars().filter(|c| !c.is_control()).take(30).collect();
                        let response = ui.add(
                            egui::Button::new(title)
                                .fill(if index == self.active {
                                    Color32::from_rgb(248, 248, 249)
                                } else {
                                    Color32::TRANSPARENT
                                })
                                .corner_radius(4),
                        );
                        if response.clicked() {
                            self.active = index;
                        }
                        if response.double_clicked() {
                            self.rename = Some(index);
                            self.rename_text = tab.name.clone().unwrap_or_default();
                        }
                    }
                    if ui.button("+").on_hover_text("New shell tab").clicked() {
                        self.new_tab(ctx);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.menu_button("...", |ui| {
                            if ui.button("New tab").clicked() {
                                self.new_tab(ctx);
                                ui.close();
                            }
                            if ui.button("Rename tab…").clicked() {
                                self.rename = Some(self.active);
                                self.rename_text = self
                                    .tabs
                                    .get(self.active)
                                    .and_then(|t| t.name.clone())
                                    .unwrap_or_default();
                                ui.close();
                            }
                            if ui.button("Copy selection").clicked() {
                                self.copy(ctx);
                                ui.close();
                            }
                            if ui.button("Paste").clicked() {
                                self.paste_requested = true;
                                ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                                ui.close();
                            }
                        });
                    });
                });
            });
    }

    fn terminal(&mut self, ctx: &egui::Context) {
        let bg = Color32::from_rgb(BACKGROUND[0], BACKGROUND[1], BACKGROUND[2]);
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(bg).inner_margin(6))
            .show(ctx, |ui| {
                let (rect, response) =
                    ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
                if self.tabs.is_empty() {
                    ui.painter().text(
                        rect.min,
                        Align2::LEFT_TOP,
                        "Shell could not start. Use + to retry.",
                        FontId::monospace(14.0),
                        Color32::LIGHT_GRAY,
                    );
                    return;
                }
                let session = &self.tabs[self.active].session;
                let font = FontId::monospace(14.0);
                let width = ui.fonts_mut(|fonts| fonts.glyph_width(&font, 'M'));
                let height = 14.0 * 1.2;
                if let Err(error) = session.resize(
                    (rect.width() / width) as u16,
                    (rect.height() / height) as u16,
                    width,
                    height,
                ) {
                    self.error = Some(format!("{error:#}"));
                }
                let point = |pos: Pos2| {
                    (
                        ((pos.y - rect.top()).max(0.0) / height) as usize,
                        ((pos.x - rect.left()).max(0.0) / width) as usize,
                    )
                };
                if response.drag_started()
                    && let Some(pos) = ctx.input(|i| i.pointer.press_origin())
                {
                    let (row, col) = point(pos);
                    session.select(row, col, true);
                }
                if response.dragged()
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    let (row, col) = point(pos);
                    session.select(row, col, false);
                }
                if response.clicked()
                    && let Some(pos) = response.interact_pointer_pos()
                {
                    let (row, col) = point(pos);
                    session.select(row, col, true);
                }
                let screen = session.screen();
                if let Some(error) = screen.error {
                    self.error = Some(error);
                }
                let painter = ui.painter().with_clip_rect(rect);
                for cell in screen.cells {
                    let position =
                        rect.min + Vec2::new(cell.col as f32 * width, cell.row as f32 * height);
                    let cell_rect = Rect::from_min_size(
                        position,
                        Vec2::new(width * if cell.wide { 2.0 } else { 1.0 }, height),
                    );
                    let cell_bg = if cell.selected {
                        Color32::from_rgb(65, 83, 112)
                    } else {
                        Color32::from_rgb(cell.bg[0], cell.bg[1], cell.bg[2])
                    };
                    if cell_bg != bg {
                        painter.rect_filled(cell_rect, 0, cell_bg);
                    }
                    let fg = Color32::from_rgb(cell.fg[0], cell.fg[1], cell.fg[2]);
                    if cell.text != " " {
                        painter.text(position, Align2::LEFT_TOP, cell.text, font.clone(), fg);
                    }
                    if cell.underline {
                        painter.line_segment(
                            [cell_rect.left_bottom(), cell_rect.right_bottom()],
                            (1.0, fg),
                        );
                    }
                }
                if let Some((row, col)) = screen.cursor
                    && ctx.input(|i| i.focused)
                    && !screen.exited
                {
                    let pos = rect.min + Vec2::new(col as f32 * width, row as f32 * height);
                    painter.rect_stroke(
                        Rect::from_min_size(pos, Vec2::new(width, height)),
                        0,
                        (1.0, Color32::from_gray(235)),
                        egui::StrokeKind::Inside,
                    );
                }
                if screen.exited {
                    painter.text(
                        rect.right_bottom(),
                        Align2::RIGHT_BOTTOM,
                        "Process exited",
                        FontId::monospace(12.0),
                        Color32::GRAY,
                    );
                }
                // A native title bar, one small tab row, and an uninterrupted terminal.
                let title =
                    self.tabs[self.active]
                        .name
                        .as_deref()
                        .unwrap_or(if screen.title.is_empty() {
                            "Shell"
                        } else {
                            &screen.title
                        });
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("Opterm — {title}")));
            });
    }

    fn events(&mut self, ctx: &egui::Context) {
        if self.rename.is_some() || egui::Popup::is_any_open(ctx) {
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        let modifiers = ctx.input(|i| i.modifiers);
        for (index, event) in events.iter().cloned().enumerate() {
            if let egui::Event::Key {
                key: Key::T,
                pressed: true,
                modifiers,
                ..
            } = &event
                && input::app_shortcut(*modifiers)
            {
                self.new_tab(ctx);
                continue;
            }
            let Some(tab) = self.tabs.get(self.active) else {
                continue;
            };
            let result = match event {
                egui::Event::Copy => {
                    let modifiers = input::clipboard_modifiers(&events, index, Key::C, modifiers);
                    if !cfg!(target_os = "macos") && modifiers.ctrl && !modifiers.shift {
                        tab.session.write(b"\x03")
                    } else {
                        self.copy(ctx);
                        Ok(())
                    }
                }
                egui::Event::Paste(text) => {
                    let modifiers = input::clipboard_modifiers(&events, index, Key::V, modifiers);
                    if !cfg!(target_os = "macos")
                        && modifiers.ctrl
                        && !modifiers.shift
                        && !self.paste_requested
                    {
                        tab.session.write(b"\x16")
                    } else {
                        self.paste_requested = false;
                        tab.session.paste(&text)
                    }
                }
                egui::Event::Text(text) => {
                    if modifiers.ctrl || modifiers.mac_cmd {
                        Ok(())
                    } else if modifiers.alt {
                        tab.session.write(format!("\x1b{text}").as_bytes())
                    } else {
                        tab.session.write(text.as_bytes())
                    }
                }
                egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                    tab.session.write(text.as_bytes())
                }
                egui::Event::Cut => {
                    let modifiers = input::clipboard_modifiers(&events, index, Key::X, modifiers);
                    if !cfg!(target_os = "macos") && modifiers.ctrl {
                        tab.session.write(b"\x18")
                    } else {
                        self.copy(ctx);
                        Ok(())
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    let mode = tab.session.mode();
                    match input::encode_key(key, modifiers, mode) {
                        Some(bytes) => tab.session.write(&bytes),
                        None => Ok(()),
                    }
                }
                egui::Event::MouseWheel { unit, delta, .. } => {
                    let lines = match unit {
                        egui::MouseWheelUnit::Line => delta.y * 3.0,
                        egui::MouseWheelUnit::Point => delta.y / (14.0 * 1.2),
                        egui::MouseWheelUnit::Page => delta.y * 30.0,
                    };
                    tab.session.scroll(lines.round() as i32);
                    Ok(())
                }
                _ => Ok(()),
            };
            if let Err(error) = result {
                self.error = Some(format!("{error:#}"));
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.tab_bar(ctx);
        self.terminal(ctx);
        self.events(ctx);
        if let Some(index) = self.rename {
            egui::Window::new("Rename tab")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.rename_text)
                            .hint_text("Blank restores automatic title"),
                    );
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            if let Some(tab) = self.tabs.get_mut(index) {
                                tab.name = (!self.rename_text.trim().is_empty())
                                    .then(|| self.rename_text.trim().to_owned());
                            }
                            self.rename = None;
                        }
                        if ui.button("Cancel").clicked() {
                            self.rename = None;
                        }
                    });
                });
        }
        if let Some(error) = self.error.clone() {
            egui::Window::new("Terminal error")
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label(error);
                    if ui.button("Dismiss").clicked() {
                        self.error = None;
                    }
                });
        }
    }
}

fn main() -> eframe::Result {
    let mut program: Vec<String> = std::env::args().skip(1).collect();
    if program.first().is_some_and(|s| s == "--") {
        program.remove(0);
    }
    eframe::run_native(
        "Opterm",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("Opterm")
                .with_inner_size([1000.0, 680.0])
                .with_min_inner_size([320.0, 160.0]),
            ..Default::default()
        },
        Box::new(move |cc| Ok(Box::new(App::new(cc, program)))),
    )
}
