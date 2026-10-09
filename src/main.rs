#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod input;
mod layout;
use layout::{Axis, Layout};

use eframe::egui::{self, Align2, Color32, FontId, Key, Pos2, Rect, Sense, Vec2};
use opterm::{
    service::{
        self, Request,
        client::{Client, RemoteSession},
    },
    terminal::BACKGROUND,
};
use std::{
    collections::{BTreeMap, VecDeque},
    process::{Command, Stdio},
    sync::Arc,
};

type PendingInput = Option<(u64, Vec<u8>)>;
fn flush_input(pending: &mut PendingInput, client: &Client) -> anyhow::Result<()> {
    if let Some((id, bytes)) = pending.take() {
        client.send(Request::Write { id, bytes })?;
    }
    Ok(())
}
fn queue_input(
    pending: &mut PendingInput,
    client: &Client,
    id: u64,
    bytes: &[u8],
) -> anyhow::Result<()> {
    if let Some((previous, buffer)) = pending
        && *previous == id
        && buffer.len() + bytes.len() <= 65_536
    {
        buffer.extend_from_slice(bytes);
        return Ok(());
    }
    flush_input(pending, client)?;
    *pending = Some((id, bytes.to_vec()));
    Ok(())
}

struct Tab {
    key: u64,
    layout: Layout,
    focused: u64,
    name: Option<String>,
    maximized: bool,
}
#[derive(Clone, Copy)]
enum Placement {
    Tab,
    Split { tab: u64, pane: u64, axis: Axis },
}

struct App {
    tabs: Vec<Tab>,
    client: Option<Arc<Client>>,
    panes: BTreeMap<u64, RemoteSession>,
    pending: VecDeque<Placement>,
    pane_rects: Vec<(u64, Rect)>,
    end_session: Option<u64>,
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
    fn new(cc: &eframe::CreationContext<'_>, program: Vec<String>, restore: bool) -> Self {
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
            client: None,
            panes: BTreeMap::new(),
            pending: VecDeque::new(),
            pane_rects: Vec::new(),
            end_session: None,
            active: 0,
            program,
            error: None,
            rename: None,
            rename_text: String::new(),
            paste_requested: false,
        };
        let notify = cc.egui_ctx.clone();
        match service::state_dir().and_then(|path| {
            Client::open(&path, &app.program, restore, move || {
                notify.request_repaint()
            })
        }) {
            Ok(client) => app.client = Some(client),
            Err(error) => app.error = Some(format!("{error:#}")),
        }
        app.sync_tabs();
        app
    }

    fn sync_tabs(&mut self) {
        let Some(client) = &self.client else {
            return;
        };
        let (views, created) = client.view_updates();
        self.panes.retain(|id, _| views.contains(id));
        for &id in &views {
            self.panes.entry(id).or_insert_with(|| client.session(id));
        }
        let previous = self.tabs.get(self.active).map(|tab| tab.key);
        self.tabs = self
            .tabs
            .drain(..)
            .filter_map(|mut tab| {
                if tab.layout.ids().iter().any(|id| !views.contains(id)) {
                    tab.layout = tab.layout.retain(&views)?;
                }
                if !tab.layout.contains(tab.focused) {
                    tab.focused = tab.layout.ids()[0];
                    tab.maximized = false;
                }
                Some(tab)
            })
            .collect();
        self.active = previous
            .and_then(|key| self.tabs.iter().position(|t| t.key == key))
            .unwrap_or(0);
        for created in created {
            let placement = self.pending.pop_front().unwrap_or(Placement::Tab);
            if let Some(id) = created {
                if !views.contains(&id) {
                    continue;
                }
                let split = if let Placement::Split { tab, pane, axis } = placement {
                    if let Some(index) = self.tabs.iter().position(|t| t.key == tab)
                        && self.tabs[index].layout.split(pane, id, axis)
                    {
                        self.tabs[index].focused = id;
                        self.tabs[index].maximized = false;
                        self.active = index;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };
                if !split {
                    self.tabs
                        .push(Self::tab(id, self.panes.get(&id).and_then(|p| p.name())));
                    self.active = self.tabs.len() - 1;
                }
            }
        }
        for id in views {
            if !self.tabs.iter().any(|t| t.layout.contains(id)) {
                self.tabs.push(Self::tab(id, self.panes[&id].name()));
                self.active = self.tabs.len() - 1;
            }
        }
        if let Some(error) = client.take_error() {
            self.error = Some(error);
        }
    }
    fn tab(id: u64, name: Option<String>) -> Tab {
        Tab {
            key: id,
            layout: Layout::Pane(id),
            focused: id,
            name,
            maximized: false,
        }
    }
    fn focused_session(&self) -> Option<&RemoteSession> {
        self.tabs
            .get(self.active)
            .and_then(|tab| self.panes.get(&tab.focused))
    }
    fn create(&mut self, placement: Placement) {
        if let Some(client) = &self.client {
            match client.send(Request::Create(Vec::new())) {
                Ok(()) => self.pending.push_back(placement),
                Err(error) => self.error = Some(format!("{error:#}")),
            }
        }
    }
    fn new_tab(&mut self) {
        self.create(Placement::Tab);
    }
    fn split(&mut self, axis: Axis) {
        if let Some(tab) = self.tabs.get(self.active) {
            self.create(Placement::Split {
                tab: tab.key,
                pane: tab.focused,
                axis,
            });
        } else {
            self.new_tab();
        }
    }
    fn new_window(&mut self) {
        let result = std::env::current_exe().and_then(|exe| {
            Command::new(exe)
                .arg("--new-window")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .spawn()
        });
        match result {
            Ok(mut child) => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(error) => self.error = Some(format!("Open new window: {error}")),
        }
    }
    fn detach(&mut self) {
        if let Some(session) = self.focused_session()
            && let Some(client) = &self.client
            && let Err(error) = client.send(Request::Detach(session.id))
        {
            self.error = Some(format!("{error:#}"));
        }
    }
    fn detach_tab(&mut self) {
        if let Some(tab) = self.tabs.get(self.active)
            && let Some(client) = &self.client
        {
            for id in tab.layout.ids() {
                if let Err(error) = client.send(Request::Detach(id)) {
                    self.error = Some(format!("{error:#}"));
                    break;
                }
            }
        }
    }
    fn maximize(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.maximized = !tab.maximized;
        }
    }
    fn navigate(&mut self, key: Key, resize: bool) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        if resize {
            let (axis, delta) = match key {
                Key::ArrowLeft => (Axis::Right, -0.05),
                Key::ArrowRight => (Axis::Right, 0.05),
                Key::ArrowUp => (Axis::Below, -0.05),
                _ => (Axis::Below, 0.05),
            };
            tab.layout.resize(tab.focused, axis, delta);
            return;
        }
        if tab.maximized {
            let ids = tab.layout.ids();
            let current = ids.iter().position(|id| *id == tab.focused).unwrap_or(0);
            let previous = matches!(key, Key::ArrowLeft | Key::ArrowUp);
            tab.focused = ids[if previous {
                (current + ids.len() - 1) % ids.len()
            } else {
                (current + 1) % ids.len()
            }];
            return;
        }
        let Some((_, current)) = self.pane_rects.iter().find(|(id, _)| *id == tab.focused) else {
            return;
        };
        let center = current.center();
        let candidate = self
            .pane_rects
            .iter()
            .filter_map(|(id, rect)| {
                if *id == tab.focused {
                    return None;
                }
                let offset = rect.center() - center;
                let (along, across) = match key {
                    Key::ArrowLeft => (-offset.x, offset.y),
                    Key::ArrowRight => (offset.x, offset.y),
                    Key::ArrowUp => (-offset.y, offset.x),
                    _ => (offset.y, offset.x),
                };
                (along > 0.0).then_some((*id, along + across.abs() * 2.0))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((id, _)) = candidate {
            tab.focused = id;
        }
    }

    fn copy(&self, ctx: &egui::Context) {
        if let Some(session) = self.focused_session()
            && let Some(text) = session.selected_text()
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
                        let automatic_title = self
                            .panes
                            .get(&tab.focused)
                            .map(|s| s.title())
                            .unwrap_or_default();
                        let manual_name = tab.name.clone();
                        let title =
                            manual_name
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
                        self.new_tab();
                    }
                    ui.menu_button("Sessions", |ui| {
                        if let Some(client) = &self.client {
                            for info in client.infos() {
                                let name =
                                    info.name.as_deref().unwrap_or(if info.title.is_empty() {
                                        "Shell"
                                    } else {
                                        &info.title
                                    });
                                let state = if info.exited {
                                    "ended"
                                } else if info.attached {
                                    "live"
                                } else {
                                    "detached"
                                };
                                if ui
                                    .button(format!(
                                        "{} · {} · {}",
                                        info.id,
                                        name.chars().take(30).collect::<String>(),
                                        state
                                    ))
                                    .clicked()
                                {
                                    if self.tabs.iter().any(|tab| tab.layout.contains(info.id)) {
                                        self.active = self
                                            .tabs
                                            .iter()
                                            .position(|t| t.layout.contains(info.id))
                                            .unwrap();
                                        self.tabs[self.active].focused = info.id;
                                    } else if let Err(error) = client.send(Request::Attach(info.id))
                                    {
                                        self.error = Some(format!("{error:#}"));
                                    }
                                    ui.close();
                                }
                            }
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.menu_button("...", |ui| {
                            if ui.button("New tab").clicked() {
                                self.new_tab();
                                ui.close();
                            }
                            if ui.button("New window").clicked() {
                                self.new_window();
                                ui.close();
                            }
                            if ui.button("Split right").clicked() {
                                self.split(Axis::Right);
                                ui.close();
                            }
                            if ui.button("Split below").clicked() {
                                self.split(Axis::Below);
                                ui.close();
                            }
                            if ui.button("Maximize / restore pane").clicked() {
                                self.maximize();
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
                            if ui.button("Detach tab").clicked() {
                                self.detach_tab();
                                ui.close();
                            }
                            if ui.button("Detach pane").clicked() {
                                self.detach();
                                ui.close();
                            }
                            if let Some(session) = self.focused_session() {
                                let id = session.id;
                                let controlled = session.controlled();
                                if !controlled && ui.button("Take control").clicked() {
                                    if let Some(client) = &self.client
                                        && let Err(error) = client.send(Request::TakeControl(id))
                                    {
                                        self.error = Some(format!("{error:#}"));
                                    }
                                    ui.close();
                                }
                                if ui
                                    .add_enabled(controlled, egui::Button::new("End session…"))
                                    .clicked()
                                {
                                    self.end_session = Some(id);
                                    ui.close();
                                }
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
        self.pane_rects.clear();
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(bg))
            .show(ctx, |ui| {
                let rect = ui.available_rect_before_wrap();
                let Some(tab) = self.tabs.get_mut(self.active) else {
                    ui.painter().text(
                        rect.min + Vec2::splat(6.0),
                        Align2::LEFT_TOP,
                        "No attached sessions. Use + or the Sessions menu.",
                        FontId::monospace(14.0),
                        Color32::LIGHT_GRAY,
                    );
                    return;
                };
                if tab.maximized {
                    self.pane_rects.push((tab.focused, rect));
                } else {
                    tab.layout.show(ui, rect, &mut self.pane_rects);
                }
                let panes = self.pane_rects.clone();
                for (id, rect) in panes {
                    let focused = self.tabs[self.active].focused == id;
                    if self.pane_rects.len() > 1 && focused {
                        ui.painter().rect_stroke(
                            rect.shrink(1.0),
                            0,
                            (1.0, Color32::from_gray(80)),
                            egui::StrokeKind::Inside,
                        );
                    }
                    self.paint_pane(ui, ctx, id, rect.shrink(6.0), focused);
                }
            });
        if let Some(client) = &self.client {
            client.visible(self.pane_rects.iter().map(|(id, _)| *id).collect());
        }
        if let Some(tab) = self.tabs.get(self.active)
            && let Some(session) = self.panes.get(&tab.focused)
        {
            let automatic = session.title();
            let title = tab.name.as_deref().unwrap_or(if automatic.is_empty() {
                "Shell"
            } else {
                &automatic
            });
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("Opterm — {title}")));
        }
    }
    fn paint_pane(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        id: u64,
        rect: Rect,
        focused: bool,
    ) {
        let bg = Color32::from_rgb(BACKGROUND[0], BACKGROUND[1], BACKGROUND[2]);
        let session = &self.panes[&id];
        let response = ui.interact(rect, egui::Id::new(("pane", id)), Sense::click_and_drag());
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
            if session.controlled()
                && let Err(error) = session.select(row, col, true)
            {
                self.error = Some(format!("{error:#}"));
            }
        }
        if response.dragged()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let (row, col) = point(pos);
            if session.controlled()
                && let Err(error) = session.select(row, col, false)
            {
                self.error = Some(format!("{error:#}"));
            }
        }
        if response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let (row, col) = point(pos);
            if session.controlled()
                && let Err(error) = session.select(row, col, true)
            {
                self.error = Some(format!("{error:#}"));
            }
        }
        let screen = session.screen();
        if let Some(error) = &screen.error {
            self.error = Some(error.clone());
        }
        let painter = ui.painter().with_clip_rect(rect);
        for cell in &screen.cells {
            let position = rect.min + Vec2::new(cell.col as f32 * width, cell.row as f32 * height);
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
                painter.text(position, Align2::LEFT_TOP, &cell.text, font.clone(), fg);
            }
            if cell.underline {
                painter.line_segment(
                    [cell_rect.left_bottom(), cell_rect.right_bottom()],
                    (1.0, fg),
                );
            }
        }
        if let Some((row, col)) = screen.cursor
            && focused
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
        if !session.controlled() && !screen.exited {
            painter.text(
                rect.right_bottom(),
                Align2::RIGHT_BOTTOM,
                "Read-only · Take control in menu",
                FontId::monospace(12.0),
                Color32::GRAY,
            );
        }
    }

    fn events(&mut self, ctx: &egui::Context) {
        if self.rename.is_some()
            || self.end_session.is_some()
            || self.error.is_some()
            || egui::Popup::is_any_open(ctx)
        {
            return;
        }
        let Some(client) = self.client.clone() else {
            return;
        };
        let mut pending = None;
        let events = ctx.input(|i| i.events.clone());
        let modifiers = ctx.input(|i| i.modifiers);
        for (index, event) in events.iter().cloned().enumerate() {
            let boundary = matches!(
                &event,
                egui::Event::PointerButton { .. }
                    | egui::Event::Paste(_)
                    | egui::Event::MouseWheel { .. }
            ) || matches!(&event, egui::Event::Key { pressed: true, modifiers, .. } if input::app_shortcut(*modifiers) || input::pane_navigation(*modifiers));
            if boundary && let Err(error) = flush_input(&mut pending, &client) {
                self.error = Some(format!("{error:#}"));
                continue;
            }

            // Process focus changes in event order. A later click in the same
            // frame must not redirect text that was typed before that click.
            if let egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                ..
            } = &event
            {
                if let Some((id, _)) = self.pane_rects.iter().find(|(_, rect)| rect.contains(*pos))
                    && let Some(tab) = self.tabs.get_mut(self.active)
                {
                    tab.focused = *id;
                    ctx.request_repaint();
                }
                continue;
            }

            if let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = &event
            {
                if input::pane_navigation(*modifiers)
                    && matches!(
                        key,
                        Key::ArrowLeft | Key::ArrowRight | Key::ArrowUp | Key::ArrowDown
                    )
                {
                    self.navigate(
                        *key,
                        if cfg!(target_os = "macos") {
                            modifiers.shift
                        } else {
                            modifiers.alt
                        },
                    );
                    ctx.request_repaint();
                    continue;
                }
                if input::app_shortcut(*modifiers) {
                    match key {
                        Key::N => {
                            self.new_window();
                            continue;
                        }
                        Key::E if !cfg!(target_os = "macos") => {
                            self.split(Axis::Right);
                            continue;
                        }
                        Key::O if !cfg!(target_os = "macos") => {
                            self.split(Axis::Below);
                            continue;
                        }
                        Key::D if cfg!(target_os = "macos") => {
                            self.split(if modifiers.shift {
                                Axis::Below
                            } else {
                                Axis::Right
                            });
                            continue;
                        }
                        Key::Enter if modifiers.shift => {
                            self.maximize();
                            ctx.request_repaint();
                            continue;
                        }
                        _ => {}
                    }
                }
            }

            if let egui::Event::Key {
                key: Key::T,
                pressed: true,
                modifiers,
                ..
            } = &event
                && input::app_shortcut(*modifiers)
            {
                self.new_tab();
                continue;
            }
            if let egui::Event::Key {
                key: Key::W,
                pressed: true,
                modifiers,
                ..
            } = &event
                && input::app_shortcut(*modifiers)
            {
                self.detach();
                continue;
            }
            let Some(session) = self
                .tabs
                .get(self.active)
                .and_then(|tab| self.panes.get(&tab.focused))
            else {
                continue;
            };
            let result = match event {
                egui::Event::Copy => {
                    let modifiers = input::clipboard_modifiers(&events, index, Key::C, modifiers);
                    if !cfg!(target_os = "macos") && modifiers.ctrl && !modifiers.shift {
                        queue_input(&mut pending, &client, session.id, b"\x03")
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
                        queue_input(&mut pending, &client, session.id, b"\x16")
                    } else {
                        self.paste_requested = false;
                        session.paste(&text)
                    }
                }
                egui::Event::Text(text) => {
                    if modifiers.ctrl || modifiers.mac_cmd {
                        Ok(())
                    } else if modifiers.alt {
                        queue_input(
                            &mut pending,
                            &client,
                            session.id,
                            format!("\x1b{text}").as_bytes(),
                        )
                    } else {
                        queue_input(&mut pending, &client, session.id, text.as_bytes())
                    }
                }
                egui::Event::Ime(egui::ImeEvent::Commit(text)) => {
                    queue_input(&mut pending, &client, session.id, text.as_bytes())
                }
                egui::Event::Cut => {
                    let modifiers = input::clipboard_modifiers(&events, index, Key::X, modifiers);
                    if !cfg!(target_os = "macos") && modifiers.ctrl {
                        queue_input(&mut pending, &client, session.id, b"\x18")
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
                    let mode = session.mode();
                    match input::encode_key(key, modifiers, mode) {
                        Some(bytes) => queue_input(&mut pending, &client, session.id, &bytes),
                        None => Ok(()),
                    }
                }
                egui::Event::MouseWheel { unit, delta, .. } => {
                    let lines = match unit {
                        egui::MouseWheelUnit::Line => delta.y * 3.0,
                        egui::MouseWheelUnit::Point => delta.y / (14.0 * 1.2),
                        egui::MouseWheelUnit::Page => delta.y * 30.0,
                    };
                    let hovered = ctx
                        .input(|i| i.pointer.hover_pos())
                        .and_then(|pos| self.pane_rects.iter().find(|(_, rect)| rect.contains(pos)))
                        .and_then(|(id, _)| self.panes.get(id))
                        .unwrap_or(session);
                    if hovered.controlled() {
                        hovered.scroll(lines.round() as i32)
                    } else {
                        Ok(())
                    }
                }
                _ => Ok(()),
            };
            if let Err(error) = result {
                self.error = Some(format!("{error:#}"));
            }
        }
        if let Err(error) = flush_input(&mut pending, &client) {
            self.error = Some(format!("{error:#}"));
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.sync_tabs();
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
                                let name = (!self.rename_text.trim().is_empty())
                                    .then(|| self.rename_text.trim().to_owned());
                                if let Some(session) = self.panes.get(&tab.layout.ids()[0]) {
                                    match session.rename(name.clone()) {
                                        Ok(()) => tab.name = name,
                                        Err(error) => self.error = Some(format!("{error:#}")),
                                    }
                                }
                            }
                            self.rename = None;
                        }
                        if ui.button("Cancel").clicked() {
                            self.rename = None;
                        }
                    });
                });
        }
        if let Some(id) = self.end_session {
            egui::Window::new("End session?")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("This stops the shell or agent. Closing a tab only detaches it.");
                    ui.horizontal(|ui| {
                        if ui.button("End session").clicked() {
                            if let Some(client) = &self.client
                                && let Err(error) = client.send(Request::End(id))
                            {
                                self.error = Some(format!("{error:#}"));
                            }
                            self.end_session = None;
                        }
                        if ui.button("Cancel").clicked() {
                            self.end_session = None;
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

fn main() {
    let mut program: Vec<String> = std::env::args().skip(1).collect();
    if program.first().is_some_and(|s| s == "--session-service") {
        if let Err(error) = service::state_dir().and_then(|path| service::serve(&path)) {
            eprintln!("{error:#}");
            std::process::exit(1);
        }
        return;
    }
    if program.first().is_some_and(|s| s == "--stop-service") {
        let result = service::state_dir()
            .and_then(|path| service::Connection::connect(&path))
            .and_then(|mut connection| connection.call(Request::Shutdown));
        if let Err(error) = result {
            eprintln!("Could not stop session service: {error:#}");
            std::process::exit(1);
        }
        println!("Session service stopped; its sessions were ended.");
        return;
    }
    let restore = !program.first().is_some_and(|s| s == "--new-window");
    if !restore {
        program.remove(0);
    }
    if program.first().is_some_and(|s| s == "--") {
        program.remove(0);
    }
    if let Err(error) = eframe::run_native(
        "Opterm",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("Opterm")
                .with_inner_size([1000.0, 680.0])
                .with_min_inner_size([320.0, 160.0]),
            ..Default::default()
        },
        Box::new(move |cc| Ok(Box::new(App::new(cc, program, restore)))),
    ) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
