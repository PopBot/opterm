#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod ime;
mod input;
mod profiles_ui;
mod provider_ui;
mod settings_ui;
use opterm::{
    layout::{Axis, Layout},
    layouts::{self, Node, SavedTab, SessionRef},
    shortcuts::Action as Shortcut,
    transfers,
};

use eframe::egui::{self, Align2, Color32, FontId, Key, Pos2, Rect, Sense, Vec2};
use opterm::{
    profiles::Theme,
    service::{
        self, Request,
        client::{Client, Reconnect, RemoteSession},
    },
    settings::{Appearance, Chrome, Cursor, Paths, Store},
    terminal::BACKGROUND,
};
use std::{
    collections::{BTreeMap, VecDeque},
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
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

struct Missing {
    session: SessionRef,
    title: String,
}

enum Startup {
    Existing,
    Fresh,
    Receive(String),
}

struct Search {
    session: u64,
    query: String,
    status: String,
    token: u64,
}

fn restore_node(
    node: &Node,
    instance: &str,
    views: &[u64],
    missing: &mut BTreeMap<u64, Missing>,
    next: &mut u64,
) -> Layout {
    let local = *next;
    *next -= 1;
    match node {
        Node::Pane { session, title } => {
            if session.matches(instance, views) {
                Layout::Pane(session.id)
            } else {
                missing.insert(
                    local,
                    Missing {
                        session: session.clone(),
                        title: title.clone(),
                    },
                );
                Layout::Pane(local)
            }
        }
        Node::Split {
            axis,
            ratio,
            first,
            second,
        } => Layout::Split {
            id: local,
            axis: *axis,
            ratio: *ratio,
            first: Box::new(restore_node(first, instance, views, missing, next)),
            second: Box::new(restore_node(second, instance, views, missing, next)),
        },
    }
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
    settings: Store,
    settings_dialog: Option<settings_ui::Dialog>,
    settings_poll: Instant,
    font_database: fontdb::Database,
    font_names: Vec<String>,
    applied_appearance: Appearance,
    applied_themes: Vec<Theme>,
    saved_layout: Option<layouts::Store>,
    missing: BTreeMap<u64, Missing>,
    layout_poll: Instant,
    mouse_button: Option<(u64, u8, Rect)>,
    pending_copy: Option<(u64, Instant)>,
    search: Option<Search>,
    search_token: u64,
    dragged_tab: Option<u64>,
    drag_hover: Option<(u64, Instant)>,
    outgoing: Option<(u64, transfers::Pending)>,
    incoming: Vec<transfers::Offer>,
    transfer_poll: Instant,
    executable: std::path::PathBuf,
    next_local: u64,
    leaving: Vec<u64>,
    ui_input_consumed: bool,
    destinations: Vec<layouts::Window>,
    window_poll: Instant,
    ime: ime::Composition,
    window_title: String,
    caret_activity: BTreeMap<u64, (Option<(usize, usize)>, Instant)>,
    click_edit: Option<(u64, usize, usize)>,
}

fn app_icon() -> Arc<egui::IconData> {
    static ICON: std::sync::OnceLock<Arc<egui::IconData>> = std::sync::OnceLock::new();
    ICON.get_or_init(|| {
        Arc::new(
            eframe::icon_data::from_png_bytes(include_bytes!("../assets/icons/window-icon.png"))
                .expect("bundled application icon"),
        )
    })
    .clone()
}

fn install_font(ctx: &egui::Context, database: &fontdb::Database, family: &str) -> bool {
    let preferred = if family != "System monospace" {
        family
    } else if cfg!(target_os = "macos") {
        "Menlo"
    } else if cfg!(target_os = "windows") {
        "Consolas"
    } else {
        "DejaVu Sans Mono"
    };
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "terminal-emoji".into(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/NotoEmoji-wght.ttf")).into(),
    );
    let mono = fonts
        .families
        .get_mut(&egui::FontFamily::Monospace)
        .unwrap();
    let position = mono
        .iter()
        .position(|name| name == "NotoEmoji-Regular")
        .unwrap_or(mono.len());
    mono.insert(position, "terminal-emoji".into());
    let primary = database.query(&fontdb::Query {
        families: &[fontdb::Family::Name(preferred), fontdb::Family::Monospace],
        ..fontdb::Query::default()
    });
    let fallback = cjk_fallback(database);
    for (name, id) in [("system-monospace", primary), ("system-cjk", fallback)] {
        if let Some(id) = id {
            database.with_face_data(id, |bytes, face_index| {
                fonts.font_data.insert(
                    name.into(),
                    egui::FontData {
                        index: face_index,
                        ..egui::FontData::from_owned(bytes.to_vec())
                    }
                    .into(),
                );
                let family = fonts
                    .families
                    .get_mut(&egui::FontFamily::Monospace)
                    .unwrap();
                if name == "system-monospace" {
                    family.insert(0, name.into());
                } else {
                    family.push(name.into());
                }
            });
        }
    }
    ctx.set_fonts(fonts);
    primary.is_some()
}

fn cjk_fallback(database: &fontdb::Database) -> Option<fontdb::ID> {
    // Load one installed outline face rather than every system font.
    [
        "Noto Sans CJK SC",
        "Noto Sans CJK JP",
        "PingFang SC",
        "Hiragino Sans",
        "Microsoft YaHei",
        "Yu Gothic",
        "WenQuanYi Zen Hei",
        "Droid Sans Fallback",
    ]
    .iter()
    .find_map(|name| {
        database.query(&fontdb::Query {
            families: &[fontdb::Family::Name(name)],
            ..fontdb::Query::default()
        })
    })
}

impl App {
    fn new(
        cc: &eframe::CreationContext<'_>,
        program: Vec<String>,
        settings: Store,
        saved_layout: Option<layouts::Store>,
        restored: bool,
        startup: Startup,
        notice: Option<String>,
    ) -> anyhow::Result<Self> {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let restored_window = if restored {
            saved_layout.as_ref().map(|store| store.window.clone())
        } else {
            None
        };
        let mut font_database = fontdb::Database::new();
        font_database.load_system_fonts();
        let mut font_names: Vec<String> = font_database
            .faces()
            .filter(|face| face.monospaced)
            .flat_map(|face| face.families.iter().map(|(name, _)| name.clone()))
            .collect();
        font_names.sort();
        font_names.dedup();
        install_font(
            &cc.egui_ctx,
            &font_database,
            &settings.current.appearance.font_family,
        );
        let mut style = (*cc.egui_ctx.style()).clone();

        style.spacing.button_padding = Vec2::new(10.0, 3.0);
        style.spacing.item_spacing = Vec2::new(3.0, 0.0);
        style.visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
        style.visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
        cc.egui_ctx.set_style(style);
        cc.egui_ctx
            .send_viewport_cmd(egui::ViewportCommand::IMEPurpose(
                egui::IMEPurpose::Terminal,
            ));
        let mut app = Self {
            caret_activity: BTreeMap::new(),
            click_edit: None,
            tabs: Vec::new(),
            client: None,
            panes: BTreeMap::new(),
            pending: VecDeque::new(),
            pane_rects: Vec::new(),
            end_session: None,
            active: 0,
            program,
            error: notice,
            rename: None,
            rename_text: String::new(),
            paste_requested: false,
            applied_appearance: settings.current.appearance.clone(),
            applied_themes: Vec::new(),
            saved_layout,
            missing: BTreeMap::new(),
            layout_poll: Instant::now(),
            mouse_button: None,
            pending_copy: None,
            search: None,
            search_token: 0,
            dragged_tab: None,
            drag_hover: None,
            outgoing: None,
            incoming: Vec::new(),
            transfer_poll: Instant::now(),
            executable: std::env::current_exe()?,
            next_local: u64::MAX - 10000,
            leaving: Vec::new(),
            ui_input_consumed: false,
            destinations: Vec::new(),
            window_poll: Instant::now(),
            ime: ime::Composition::default(),
            window_title: String::new(),
            settings_dialog: None,
            settings_poll: Instant::now(),
            settings,
            font_database,
            font_names,
        };
        app.apply_appearance(&cc.egui_ctx);
        if !matches!(app.settings.observed, opterm::settings::Revision::Missing)
            && app.settings.notice.is_some()
        {
            app.open_settings();
        }
        let notify = cc.egui_ctx.clone();
        match service::state_dir().and_then(|path| {
            let mut launch = app
                .settings
                .current
                .launch(&app.settings.current.default_profile().id)?;
            if let Some(program) = app.program.first() {
                launch.profile.executable = program.clone();
                launch.profile.arguments = app.program[1..].to_vec();
            }
            let reconnect = if let Some(window) = &restored_window {
                Reconnect::Saved(window.sessions())
            } else if !matches!(startup, Startup::Existing) {
                Reconnect::Fresh
            } else {
                Reconnect::Detached
            };
            Client::open_with_history(
                &path,
                launch,
                !app.program.is_empty(),
                reconnect,
                restored_window.is_none() && !matches!(startup, Startup::Receive(_)),
                Some(&app.settings.current.history),
                move || notify.request_repaint(),
            )
        }) {
            Ok(client) => app.client = Some(client),
            Err(error) => app.error = Some(format!("{error:#}")),
        }
        app.apply_appearance(&cc.egui_ctx);
        if app
            .client
            .as_ref()
            .is_some_and(|client| !client.history_supported)
        {
            let previous = app.settings.notice.take().unwrap_or_default();
            app.settings.notice = Some(format!(
                "{previous} Running service predates saved output controls. Existing work stays running; finish its sessions before deliberately upgrading the service."
            ));
        }
        if app
            .client
            .as_ref()
            .is_some_and(|client| !client.profiles || client.instance.is_empty())
        {
            let previous = app.settings.notice.take().unwrap_or_default();
            app.settings.notice = Some(format!(
                "{previous} Running service predates saved layouts or profiles/themes; existing work stays running. Finish its sessions before stopping the service, or use an isolated OPTERM_STATE_DIR to try the new features."
            ));
        }
        if let Some(window) = &restored_window {
            app.restore_tabs(window);
        }
        app.sync_tabs();
        if let Startup::Receive(id) = startup {
            let result = service::state_dir().and_then(|state| {
                let receiver = app
                    .saved_layout
                    .as_ref()
                    .ok_or_else(|| {
                        anyhow::anyhow!("A saved-window file is required for tab movement")
                    })?
                    .window
                    .id
                    .clone();
                let offer = transfers::receive(&state, &id, &receiver)?;
                app.begin_receive(offer)
            });
            if let Err(error) = result {
                app.error = Some(format!(
                    "Receive tab: {error:#}. The source tab stays available."
                ));
            }
        }
        Ok(app)
    }

    fn restore_tabs(&mut self, window: &layouts::Window) {
        let (instance, views) = self
            .client
            .as_ref()
            .map(|client| (client.instance.clone(), client.view_updates().0))
            .unwrap_or_default();
        let mut next = u64::MAX;
        for tab in &window.tabs {
            let layout = restore_node(&tab.layout, &instance, &views, &mut self.missing, &mut next);
            let ids = layout.ids();
            self.tabs.push(Tab {
                key: ids[0],
                layout,
                focused: ids[tab.focused],
                name: tab.name.clone(),
                maximized: tab.maximized,
            });
        }
        self.active = window.active;
    }
    fn pane_title(&self, id: u64) -> String {
        self.panes
            .get(&id)
            .map(|pane| pane.title())
            .unwrap_or_else(|| {
                self.missing
                    .get(&id)
                    .map(|pane| format!("{} · unavailable", pane.title))
                    .unwrap_or_default()
            })
    }
    fn capture_node(&self, layout: &Layout, instance: &str) -> Node {
        match layout {
            Layout::Pane(id) => {
                let (session, title) = match self.missing.get(id) {
                    Some(pane) => (pane.session.clone(), pane.title.clone()),
                    None => (
                        SessionRef {
                            instance: instance.into(),
                            id: *id,
                        },
                        self.pane_title(*id)
                            .chars()
                            .filter(|c| !c.is_control())
                            .take(128)
                            .collect(),
                    ),
                };
                Node::Pane { session, title }
            }
            Layout::Split {
                axis,
                ratio,
                first,
                second,
                ..
            } => Node::Split {
                axis: *axis,
                ratio: *ratio,
                first: Box::new(self.capture_node(first, instance)),
                second: Box::new(self.capture_node(second, instance)),
            },
        }
    }
    fn save_layout(&mut self, ctx: &egui::Context) -> anyhow::Result<()> {
        let Some(store) = &self.saved_layout else {
            return Ok(());
        };
        let mut window = store.window.clone();
        let instance = self
            .client
            .as_ref()
            .map(|client| client.instance.as_str())
            .unwrap_or_default();
        window.active = self.active.min(self.tabs.len().saturating_sub(1));
        window.tabs = self
            .tabs
            .iter()
            .map(|tab| SavedTab {
                name: tab.name.clone(),
                focused: tab
                    .layout
                    .ids()
                    .iter()
                    .position(|id| *id == tab.focused)
                    .unwrap_or(0),
                maximized: tab.maximized,
                layout: self.capture_node(&tab.layout, instance),
            })
            .collect();
        // Wayland does not supply an absolute native inner rectangle. Read the
        // fallback outside input(), which already holds the context lock.
        let fallback_rect = ctx.viewport_rect();
        ctx.input(|input| {
            let viewport = input.viewport();
            window.maximized = viewport.maximized.unwrap_or(false);
            if !window.maximized && !viewport.minimized.unwrap_or(false) {
                let rect = viewport.inner_rect.unwrap_or(fallback_rect);
                window.size = [
                    rect.width().clamp(320.0, 16384.0),
                    rect.height().clamp(160.0, 16384.0),
                ];
                if let Some(rect) = viewport.outer_rect {
                    window.position = Some([rect.min.x, rect.min.y]);
                }
            }
        });
        self.saved_layout.as_mut().unwrap().save(window)
    }

    fn open_settings(&mut self) {
        if self.settings_dialog.is_none() {
            self.settings_dialog = Some(settings_ui::Dialog::new(&self.settings));
        }
    }
    fn apply_appearance(&mut self, ctx: &egui::Context) {
        let appearance = &self.settings.current.appearance;
        ctx.set_theme(match appearance.chrome {
            Chrome::System => egui::ThemePreference::System,
            Chrome::Dark => egui::ThemePreference::Dark,
            Chrome::Light => egui::ThemePreference::Light,
        });
        if self.applied_appearance.font_family != appearance.font_family {
            install_font(ctx, &self.font_database, &appearance.font_family);
        }
        self.applied_appearance = appearance.clone();
        let themes = self.settings.current.all_themes();
        if let Some(client) = &self.client
            && client.profiles
            && self.applied_themes != themes
        {
            match client.send(Request::Themes(themes.clone())) {
                Ok(()) => self.applied_themes = themes,
                Err(error) => self.error = Some(format!("Apply themes: {error:#}")),
            }
        }
        if let Some(client) = &self.client
            && client.history_supported
            && let Err(error) = client.configure_history(&self.settings.current.history)
        {
            self.error = Some(format!("Apply output settings: {error:#}"));
        }
        ctx.request_repaint();
    }
    fn show_settings(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.settings_dialog else {
            return;
        };
        let action = dialog.show(ctx, &self.settings, &self.font_names);
        use settings_ui::Action;
        match action {
            Some(Action::Cancel) => self.settings_dialog = None,
            Some(Action::Save | Action::Replace) => {
                let previous_revision = self.settings.observed.clone();
                let result = self.settings.save(
                    &dialog.draft,
                    &dialog.baseline,
                    matches!(action, Some(Action::Replace)),
                );
                match result {
                    Ok(()) => self.settings_dialog = None,
                    Err(error) => {
                        dialog.conflict = dialog.baseline != self.settings.disk_revision();
                        dialog.message = Some(format!("{error:#}"));
                        // Only advance the baseline after our own config commit
                        // (e.g. backup replacement failed). A conflict must keep
                        // requiring explicit replacement, even for an unchanged draft.
                        if self.settings.observed != previous_revision {
                            dialog.baseline = self.settings.observed.clone();
                        }
                    }
                }
                self.apply_appearance(ctx);
            }
            Some(Action::Restore) => match self.settings.recovery() {
                Ok(recovery) => {
                    dialog.draft = recovery;
                    dialog.message = Some("Recovery copy loaded into the draft. Save to restore; Cancel leaves the file untouched.".into());
                }
                Err(error) => dialog.message = Some(format!("{error:#}")),
            },
            Some(Action::Reload) => {
                self.settings.poll();
                self.settings_dialog = Some(settings_ui::Dialog::new(&self.settings));
                self.apply_appearance(ctx);
            }
            Some(Action::Edit) => {
                if !self.settings.paths.config.exists() {
                    dialog.message = Some("Save first to create the configuration file.".into());
                } else if let Err(error) = open::that_detached(&self.settings.paths.config) {
                    dialog.message = Some(format!("Open configuration: {error}"));
                }
            }
            Some(Action::BackupFolder) => {
                if let Err(error) =
                    open::that_detached(self.settings.paths.backup.parent().unwrap())
                {
                    dialog.message = Some(format!("Open recovery folder: {error}"));
                }
            }
            Some(Action::StageStorage) => {
                dialog.message = Some(
                    match self
                        .settings
                        .stage_location(std::path::Path::new(&dialog.storage_root))
                    {
                        Ok(path) => format!(
                            "Saved settings copied to {}. Close and reopen the desktop to use this folder. Your sessions keep running.",
                            path.display()
                        ),
                        Err(error) => format!("Change folder: {error:#}"),
                    },
                );
            }
            Some(Action::ClearCache) => {
                dialog.message = Some(match opterm::storage::clear_cache(&self.settings.paths) {
                    Ok(bytes) => format!(
                        "Cleared {bytes} bytes of disposable cache. Settings, saved output, layouts and recovery are retained."
                    ),
                    Err(error) => format!("Clear cache: {error:#}"),
                });
            }
            Some(Action::HistoryFolder) => {
                let result = service::state_dir().and_then(|path| {
                    let path = path.join("history");
                    anyhow::ensure!(path.is_dir(), "No saved output folder yet. Saving output requires a compatible session service.");
                    open::that_detached(path).map_err(Into::into)
                });
                if let Err(error) = result {
                    dialog.message = Some(format!("Open saved output: {error:#}"));
                }
            }
            None => {}
        }
    }

    fn sync_tabs(&mut self) {
        let Some(client) = &self.client else {
            return;
        };
        let (mut views, created) = client.view_updates();
        self.leaving.retain(|id| views.contains(id));
        views.retain(|id| !self.leaving.contains(id));
        let retained: Vec<u64> = views
            .iter()
            .copied()
            .chain(self.missing.keys().copied())
            .collect();
        self.panes.retain(|id, _| views.contains(id));
        self.caret_activity.retain(|id, _| views.contains(id));
        for &id in &views {
            self.panes.entry(id).or_insert_with(|| client.session(id));
        }
        let previous = self.tabs.get(self.active).map(|tab| tab.key);
        self.tabs = self
            .tabs
            .drain(..)
            .filter_map(|mut tab| {
                if tab.layout.ids().iter().any(|id| !retained.contains(id)) {
                    tab.layout = tab.layout.retain(&retained)?;
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
        self.create_with_profile(
            placement,
            &self.settings.current.default_profile().id.clone(),
        );
    }
    fn create_with_profile(&mut self, placement: Placement, profile: &str) {
        if matches!(placement, Placement::Split {tab,..} if self.outgoing.as_ref().is_some_and(|(key,_)| *key == tab))
        {
            return;
        }
        if let Some(client) = &self.client {
            let request = self.settings.current.launch(profile).and_then(|launch| {
                if client.profiles { Ok(Request::CreateProfile(Box::new(launch))) }
                else if launch.legacy_compatible() { Ok(Request::Create(launch.program())) }
                else { anyhow::bail!("Running service predates launch profiles; existing work stays running. Finish its sessions before stopping it, or use an isolated OPTERM_STATE_DIR.") }
            });
            match request.and_then(|request| client.send(request)) {
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
    fn split_with_profile(&mut self, axis: Axis, profile: &str) {
        let placement = self
            .tabs
            .get(self.active)
            .map(|tab| Placement::Split {
                tab: tab.key,
                pane: tab.focused,
                axis,
            })
            .unwrap_or(Placement::Tab);
        self.create_with_profile(placement, profile);
    }
    fn profile_menu(&mut self, ui: &mut egui::Ui, axis: Option<Axis>) {
        for profile in self.settings.current.all_profiles() {
            if ui
                .add(provider_ui::button(
                    ui,
                    provider_ui::profile_provider(&profile),
                    &profile.name,
                ))
                .clicked()
            {
                if let Some(axis) = axis {
                    self.split_with_profile(axis, &profile.id);
                } else {
                    self.create_with_profile(Placement::Tab, &profile.id);
                }
                ui.close();
            }
        }
    }
    fn new_window(&mut self) {
        let result = {
            Command::new(&self.executable)
                .arg("--new-window")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .spawn()
        };
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
        if self.outgoing.as_ref().is_some_and(|(key, _)| {
            self.tabs
                .get(self.active)
                .is_some_and(|tab| tab.key == *key)
        }) {
            return;
        }
        if let Some(tab) = self.tabs.get(self.active)
            && self.missing.remove(&tab.focused).is_some()
        {
            let ids: Vec<u64> = tab
                .layout
                .ids()
                .into_iter()
                .filter(|id| *id != tab.focused)
                .collect();
            if let Some(layout) = tab.layout.clone().retain(&ids) {
                let tab = &mut self.tabs[self.active];
                tab.focused = ids[0];
                tab.layout = layout;
                tab.maximized = false;
            } else {
                self.tabs.remove(self.active);
                self.active = self.active.min(self.tabs.len().saturating_sub(1));
            }
            return;
        }
        if let Some(session) = self.focused_session()
            && let Some(client) = &self.client
            && let Err(error) = client.send(Request::Detach(session.id))
        {
            self.error = Some(format!("{error:#}"));
        }
    }
    fn detach_tab(&mut self) {
        if let Some(tab) = self.tabs.get(self.active) {
            self.close_tab(tab.key);
        }
    }
    fn close_tab(&mut self, key: u64) {
        if self
            .outgoing
            .as_ref()
            .is_some_and(|(moving, _)| *moving == key)
        {
            return;
        }
        let Some(index) = self.tabs.iter().position(|tab| tab.key == key) else {
            return;
        };
        let ids = self.tabs[index].layout.ids();
        for id in &ids {
            if self.panes.contains_key(id) {
                let Some(client) = &self.client else { return };
                if let Err(error) = client.send(Request::Detach(*id)) {
                    self.error = Some(format!("Close tab: {error:#}"));
                    return;
                }
                self.leaving.push(*id);
            }
        }
        let active = self.tabs.get(self.active).map(|tab| tab.key);
        self.tabs.remove(index);
        for id in ids {
            self.missing.remove(&id);
            self.panes.remove(&id);
        }
        self.active = active
            .and_then(|key| self.tabs.iter().position(|tab| tab.key == key))
            .unwrap_or(index.min(self.tabs.len().saturating_sub(1)));
        self.search = None;
        self.rename = None;
    }
    fn maximize(&mut self) {
        if self.outgoing.as_ref().is_some_and(|(key, _)| {
            self.tabs
                .get(self.active)
                .is_some_and(|tab| tab.key == *key)
        }) {
            return;
        }
        if let Some(tab) = self.tabs.get_mut(self.active) {
            tab.maximized = !tab.maximized;
        }
    }
    fn navigate(&mut self, key: Key, resize: bool) {
        if self.outgoing.as_ref().is_some_and(|(moving, _)| {
            self.tabs
                .get(self.active)
                .is_some_and(|tab| tab.key == *moving)
        }) {
            return;
        }
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

    fn tab_bar(&mut self, ctx: &egui::Context) -> Vec<(u64, Rect)> {
        let destinations = self.destinations.clone();
        let own = self
            .saved_layout
            .as_ref()
            .map(|store| store.window.id.clone())
            .unwrap_or_default();
        let mut move_request = None;
        let mut close_request = None;
        let mut split_request = None;
        let mut tab_rects = Vec::new();
        egui::TopBottomPanel::top("tabs")
            .exact_height(30.0)
            .frame(
                egui::Frame::new()
                    .fill(if ctx.style().visuals.dark_mode {
                        Color32::from_rgb(44, 44, 46)
                    } else {
                        Color32::from_rgb(196, 196, 198)
                    })
                    .inner_margin(egui::Margin::symmetric(6, 3)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    egui::ScrollArea::horizontal()
                        .id_salt("terminal-tab-strip")
                        .max_width((ui.available_width() - 180.0).max(80.0))
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for (index, tab) in self.tabs.iter().enumerate() {
                                    let automatic_title = self.pane_title(tab.focused);
                                    let provider = self
                                        .panes
                                        .get(&tab.focused)
                                        .and_then(|pane| pane.info())
                                        .and_then(|info| info.provider);
                                    let manual_name = tab.name.clone();
                                    let title =
                                        manual_name
                                            .as_deref()
                                            .unwrap_or(if automatic_title.is_empty() {
                                                "Shell"
                                            } else {
                                                &automatic_title
                                            });
                                    let mut title: String =
                                        title.chars().filter(|c| !c.is_control()).take(24).collect();
                                    if self
                                        .outgoing
                                        .as_ref()
                                        .is_some_and(|(key, _)| *key == tab.key)
                                    {
                                        title.push_str(" · moving");
                                    }
                                    let moving = self.outgoing.as_ref().is_some_and(|(key, _)| *key == tab.key);
                                    let fill = if index == self.active {
                                        if ctx.style().visuals.dark_mode { Color32::from_rgb(66, 66, 68) }
                                        else { Color32::from_rgb(248, 248, 249) }
                                    } else { Color32::TRANSPARENT };
                                    let group = ui.push_id(tab.key, |ui| {
                                        egui::Frame::new().fill(fill).corner_radius(4).show(ui, |ui| {
                                            ui.spacing_mut().item_spacing.x = 0.0;
                                            ui.horizontal(|ui| {
                                                let label = ui.add(provider_ui::button(ui, provider, title)
                                                    .frame(false).sense(Sense::click_and_drag()));
                                                let close = ui.add_enabled(!moving, egui::Button::new("×")
                                                    .frame(false).min_size(Vec2::new(22.0, 20.0)))
                                                    .on_hover_text("Close tab · sessions stay available in Sessions");
                                                if close.clicked() { close_request = Some(tab.key); }
                                                (label, close)
                                            }).inner
                                        })
                                    }).inner;
                                    let (response, close_response) = group.inner;
                                    let menu_response = response.clone().union(close_response);
                                    tab_rects.push((tab.key, group.response.rect));
                                    if response.drag_started() && self.outgoing.is_none() {
                                        self.dragged_tab = Some(tab.key);
                                    }
                                    if response.clicked() {
                                        self.active = index;
                                        self.search = None;
                                    }
                                    menu_response.context_menu(|ui| {
                                        if ui
                                            .add_enabled(
                                                self.outgoing.is_none(),
                                                egui::Button::new("Move to new window"),
                                            )
                                            .clicked()
                                        {
                                            move_request = Some((tab.key, None));
                                            ui.close();
                                        }
                                        for window in destinations.iter().filter(|window| window.id != own) {
                                            let name = window
                                                .tabs
                                                .get(window.active)
                                                .and_then(|tab| tab.name.clone())
                                                .unwrap_or_else(|| {
                                                    format!("Window {}", &window.id[window.id.len() - 6..])
                                                });
                                            if ui
                                                .add_enabled(
                                                    self.outgoing.is_none(),
                                                    egui::Button::new(format!("Move to {name}")),
                                                )
                                                .clicked()
                                            {
                                                move_request = Some((tab.key, Some(window.id.clone())));
                                                ui.close();
                                            }
                                        }
                                        ui.separator();
                                        if ui.add_enabled(!moving, egui::Button::new("Rename tab…")).clicked() {
                                            self.rename = Some(index);
                                            self.rename_text = tab.name.clone().unwrap_or_default();
                                            ui.close();
                                        }
                                        for (label, axis) in [("Split right", Axis::Right), ("Split below", Axis::Below)] {
                                            if ui.add_enabled(!moving, egui::Button::new(label)).clicked() {
                                                split_request = Some((index, axis));
                                                ui.close();
                                            }
                                        }
                                        ui.separator();
                                        if ui.add_enabled(!moving, egui::Button::new("Close tab")).clicked() {
                                            close_request = Some(tab.key);
                                            ui.close();
                                        }
                                        ui.weak("Closed sessions stay available in Sessions.");
                                    });
                                    if response.double_clicked()
                                        && self
                                            .outgoing
                                            .as_ref()
                                            .is_none_or(|(key, _)| *key != tab.key)
                                    {
                                        self.rename = Some(index);
                                        self.rename_text = tab.name.clone().unwrap_or_default();
                                    }
                                }
                            });
                        });
                    let plus = ui
                        .button("+")
                        .on_hover_text("New default-profile tab; right-click to choose a profile");
                    if plus.clicked() {
                        self.new_tab();
                    }
                    plus.context_menu(|ui| self.profile_menu(ui, None));
                    if let Some(notice) = &self.settings.notice
                        && ui.button("Settings notice").on_hover_text(notice).clicked()
                    {
                        self.open_settings();
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
                                    .add(provider_ui::button(
                                        ui,
                                        info.provider,
                                        format!(
                                            "{} · {} · {}",
                                            info.id,
                                            name.chars().take(30).collect::<String>(),
                                            state
                                        ),
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
                            if ui.button("Settings…").clicked() {
                                self.open_settings();
                                ui.close();
                            }
                            ui.separator();
                            if ui.button("New tab").clicked() {
                                self.new_tab();
                                ui.close();
                            }
                            ui.menu_button("New tab with profile", |ui| {
                                self.profile_menu(ui, None)
                            });
                            ui.menu_button("Split right with profile", |ui| {
                                self.profile_menu(ui, Some(Axis::Right))
                            });
                            ui.menu_button("Split below with profile", |ui| {
                                self.profile_menu(ui, Some(Axis::Below))
                            });
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
                            if ui
                                .add_enabled(
                                    self.outgoing.as_ref().is_none_or(|(key, _)| {
                                        self.tabs.get(self.active).is_none_or(|tab| tab.key != *key)
                                    }),
                                    egui::Button::new("Rename tab…"),
                                )
                                .clicked()
                            {
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
                                if let Some(client) = &self.client
                                    && client.history_supported
                                    && let Some(info) =
                                        client.infos().into_iter().find(|info| info.id == id)
                                {
                                    let mut recording = info.recording;
                                    if ui
                                        .add_enabled(
                                            controlled && self.settings.current.history.enabled,
                                            egui::Checkbox::new(
                                                &mut recording,
                                                "Save this session's output",
                                            ),
                                        )
                                        .changed()
                                        && let Err(error) = client.send(Request::Recording {
                                            id,
                                            enabled: recording,
                                        })
                                    {
                                        self.error = Some(format!("Output recording: {error:#}"));
                                    }
                                    if let Some(error) = info.history_error {
                                        ui.weak(error);
                                    }
                                }
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
        if let Some((index, axis)) = split_request {
            self.active = index;
            self.split(axis);
        }
        if let Some(key) = close_request {
            self.close_tab(key);
        }
        if let Some((key, target)) = move_request {
            self.move_tab(key, target, ctx);
        }
        tab_rects
    }

    fn tab_drag(&mut self, ctx: &egui::Context, tab_rects: &[(u64, Rect)]) {
        let Some(key) = self.dragged_tab else { return };
        let Some(pos) = ctx.input(|i| i.pointer.interact_pos().or(i.pointer.latest_pos())) else {
            return;
        };
        let hover = tab_rects
            .iter()
            .find(|(other, rect)| *other != key && rect.contains(pos))
            .map(|(key, _)| *key);
        if let Some(hover) = hover {
            if self.drag_hover.is_none_or(|(key, _)| key != hover) {
                self.drag_hover = Some((hover, Instant::now()));
            }
            if self
                .drag_hover
                .is_some_and(|(_, since)| since.elapsed() >= Duration::from_millis(450))
            {
                if let Some(index) = self.tabs.iter().position(|tab| tab.key == hover)
                    && self.active != index
                {
                    self.active = index;
                    ctx.request_repaint();
                    if !ctx.input(|input| input.pointer.any_released()) {
                        return;
                    }
                }
            } else {
                ctx.request_repaint_after(Duration::from_millis(50));
            }
        } else {
            self.drag_hover = None;
        }
        let active = self.tabs.get(self.active).map(|tab| tab.key);
        let snap = if active != Some(key) {
            self.pane_rects.iter().find_map(|(id, rect)| {
                if !rect.contains(pos) {
                    return None;
                }
                let edges = [
                    (pos.x - rect.left(), Axis::Right, true),
                    (rect.right() - pos.x, Axis::Right, false),
                    (pos.y - rect.top(), Axis::Below, true),
                    (rect.bottom() - pos.y, Axis::Below, false),
                ];
                let (distance, axis, before) =
                    edges.into_iter().min_by(|a, b| a.0.total_cmp(&b.0))?;
                (distance < rect.width().min(rect.height()) * 0.25)
                    .then_some((*id, *rect, axis, before))
            })
        } else {
            None
        };
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("tab-drop-preview"),
        ));
        if let Some((_, mut rect, axis, before)) = snap {
            match (axis, before) {
                (Axis::Right, true) => rect.max.x = rect.center().x,
                (Axis::Right, false) => rect.min.x = rect.center().x,
                (Axis::Below, true) => rect.max.y = rect.center().y,
                (Axis::Below, false) => rect.min.y = rect.center().y,
            }
            painter.rect_filled(rect, 4, Color32::from_rgba_unmultiplied(90, 160, 230, 65));
            painter.rect_stroke(
                rect,
                4,
                (2.0, Color32::from_rgb(90, 160, 230)),
                egui::StrokeKind::Inside,
            );
        } else if pos.y <= 30.0 && pos.x >= 0.0 {
            let slot = tab_rects
                .iter()
                .filter(|(id, _)| *id != key)
                .find(|(_, rect)| pos.x < rect.center().x);
            let x = slot
                .map(|(_, rect)| rect.left())
                .unwrap_or_else(|| tab_rects.last().map_or(6.0, |(_, rect)| rect.right()));
            painter.line_segment(
                [Pos2::new(x, 4.0), Pos2::new(x, 26.0)],
                (2.0, Color32::from_rgb(90, 160, 230)),
            );
        }
        if !ctx.input(|i| i.pointer.any_released()) {
            return;
        }
        self.dragged_tab = None;
        self.drag_hover = None;
        if let Some((pane, _, axis, before)) = snap {
            if let Some(source) = self.tabs.iter().position(|tab| tab.key == key) {
                let layout = self.tabs[source].layout.clone();
                let focused = self.tabs[source].focused;
                if self.tabs[self.active]
                    .layout
                    .dock(pane, &layout, axis, before, self.next_local)
                {
                    self.next_local -= 1;
                    self.tabs[self.active].focused = focused;
                    self.tabs[self.active].maximized = false;
                    self.tabs.remove(source);
                    self.active = self
                        .tabs
                        .iter()
                        .position(|tab| Some(tab.key) == active)
                        .unwrap_or(0);
                }
            }
        } else if pos.y <= 30.0 && ctx.viewport_rect().contains(pos) {
            if let Some(source) = self.tabs.iter().position(|tab| tab.key == key) {
                let tab = self.tabs.remove(source);
                let target = self
                    .tabs
                    .iter()
                    .position(|tab| {
                        tab_rects
                            .iter()
                            .find(|(key, _)| *key == tab.key)
                            .is_some_and(|(_, rect)| pos.x < rect.center().x)
                    })
                    .unwrap_or(self.tabs.len());
                self.tabs.insert(target, tab);
                self.active = self
                    .tabs
                    .iter()
                    .position(|tab| Some(tab.key) == active)
                    .unwrap_or(0);
            }
        } else if !ctx.viewport_rect().contains(pos) {
            let own = self
                .saved_layout
                .as_ref()
                .map(|store| store.window.id.as_str())
                .unwrap_or_default();
            let origin = ctx
                .input(|i| i.viewport().inner_rect.map(|rect| rect.min))
                .unwrap_or(Pos2::ZERO);
            let world = origin + pos.to_vec2();
            let target = self
                .destinations
                .iter()
                .filter(|window| window.id != own)
                .find(|window| {
                    window.position.is_some_and(|position| {
                        Rect::from_min_size(
                            Pos2::new(position[0], position[1]),
                            Vec2::new(window.size[0], window.size[1]),
                        )
                        .contains(world)
                    })
                })
                .map(|window| window.id.clone());
            self.move_tab(key, target, ctx);
        }
        if let Err(error) = self.save_layout(ctx) {
            self.error = Some(error.to_string());
        }
        self.ui_input_consumed = true;
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
                if self.outgoing.as_ref().is_some_and(|(key,_)| *key == tab.key) { ui.disable(); }
                if tab.maximized {
                    self.pane_rects.push((tab.focused, rect));
                } else {
                    tab.layout.show(ui, rect, &mut self.pane_rects);
                }
                let panes = self.pane_rects.clone();
                for (id, rect) in panes {
                    let focused = self.tabs[self.active].focused == id;
                    let background = self.panes.get(&id).map(|pane| pane.screen().background).unwrap_or(BACKGROUND);
                    ui.painter().rect_filled(
                        rect,
                        0,
                        Color32::from_rgb(background[0], background[1], background[2]),
                    );
                    if self.pane_rects.len() > 1 && focused {
                        ui.painter().rect_stroke(
                            rect.shrink(1.0),
                            0,
                            (1.0, Color32::from_gray(80)),
                            egui::StrokeKind::Inside,
                        );
                    }
                    let a = &self.settings.current.appearance;
                    let inset = Vec2::new(
                        a.padding_x.min((rect.width() - 1.0).max(0.0) / 2.0),
                        a.padding_y.min((rect.height() - 1.0).max(0.0) / 2.0),
                    );
                    if self.panes.contains_key(&id) { self.paint_pane(ui, ctx, id, rect.shrink2(inset), focused); }
                    else { ui.painter().text(rect.min + Vec2::splat(12.0), Align2::LEFT_TOP,
                        "Session unavailable. Its commands were not rerun.\nUse + for new work, or close this pane.", FontId::monospace(14.0), Color32::LIGHT_GRAY); }
                }
            });
        if let Some(client) = &self.client {
            client.visible(self.pane_rects.iter().map(|(id, _)| *id).collect());
        }
        if let Some(tab) = self.tabs.get(self.active) {
            let automatic = self.pane_title(tab.focused);
            let title = tab.name.as_deref().unwrap_or(if automatic.is_empty() {
                "Shell"
            } else {
                &automatic
            });
            let title = format!("Opterm — {title}");
            if title != self.window_title {
                // Viewport commands request another frame, even when the title
                // is unchanged. Sending this every frame keeps an idle terminal
                // continuously rendering.
                ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
                self.window_title = title;
            }
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
        let session = &self.panes[&id];
        let response = ui.interact(rect, egui::Id::new(("pane", id)), Sense::click_and_drag());
        if self
            .tabs
            .get(self.active)
            .is_some_and(|tab| tab.focused == id)
            && self.settings_dialog.is_none()
            && self.search.is_none()
            && self.rename.is_none()
            && self.error.is_none()
            && self.end_session.is_none()
            && !egui::Popup::is_any_open(ctx)
        {
            ctx.memory_mut(|memory| {
                memory.request_focus(response.id);
                memory.set_focus_lock_filter(
                    response.id,
                    egui::EventFilter {
                        tab: true,
                        horizontal_arrows: true,
                        vertical_arrows: true,
                        escape: true,
                    },
                );
            });
        }
        let appearance = &self.settings.current.appearance;
        let font = FontId::monospace(appearance.font_size);
        let width = ui.fonts_mut(|fonts| fonts.glyph_width(&font, 'M'));
        let height = appearance.font_size * appearance.line_height;
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
        let local_selection =
            !input::mouse_reporting(session.mode()) || ctx.input(|i| i.modifiers.shift);
        if local_selection
            && response.drag_started()
            && let Some(pos) = ctx.input(|i| i.pointer.press_origin())
        {
            let (row, col) = point(pos);
            if session.controlled()
                && let Err(error) = session.select(row, col, true)
            {
                self.error = Some(format!("{error:#}"));
            }
        }
        if local_selection
            && response.dragged()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let (row, col) = point(pos);
            if session.controlled()
                && let Err(error) = session.select(row, col, false)
            {
                self.error = Some(format!("{error:#}"));
            }
        }
        if local_selection
            && response.clicked()
            && let Some(pos) = response.interact_pointer_pos()
        {
            let (row, col) = point(pos);
            if session.controlled() {
                let result = if ctx.input(|i| i.modifiers.is_none())
                    && !response.double_clicked()
                    && !response.triple_clicked()
                {
                    self.click_edit = Some((id, row, col));
                    Ok(())
                } else {
                    session.select(row, col, true)
                };
                if let Err(error) = result {
                    self.error = Some(format!("{error:#}"));
                }
            }
        }
        if local_selection && response.drag_stopped() && self.settings.current.copy_on_select {
            self.pending_copy = Some((id, Instant::now()));
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        let screen = session.screen();
        let caret = session
            .info()
            .and_then(|info| info.profile)
            .map(|profile| {
                self.settings
                    .current
                    .profile(&profile.id)
                    .map(|p| p.caret)
                    .unwrap_or(profile.caret)
            })
            .unwrap_or(self.settings.current.shell_caret);
        let activity = self
            .caret_activity
            .entry(id)
            .or_insert((screen.cursor, Instant::now()));
        if activity.0 != screen.cursor {
            *activity = (screen.cursor, Instant::now());
        }
        let blink_phase = activity.1.elapsed().as_millis() % 1000;
        let caret_visible = !caret.blink || blink_phase < 500 || self.ime.text(id).is_some();
        if caret.blink
            && focused
            && ctx.input(|i| i.focused)
            && screen.cursor.is_some()
            && !screen.exited
        {
            ctx.request_repaint_after(Duration::from_millis(500 - (blink_phase % 500) as u64));
        }
        let bg = Color32::from_rgb(
            screen.background[0],
            screen.background[1],
            screen.background[2],
        );
        let cursor_color = Color32::from_rgb(
            screen.cursor_color[0],
            screen.cursor_color[1],
            screen.cursor_color[2],
        );
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
                Color32::from_rgb(
                    screen.selection_color[0],
                    screen.selection_color[1],
                    screen.selection_color[2],
                )
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
            let cursor_rect = Rect::from_min_size(pos, Vec2::new(width, height));
            if session.controlled()
                && self.search.is_none()
                && self.settings_dialog.is_none()
                && self.rename.is_none()
                && self.end_session.is_none()
                && self.error.is_none()
                && !egui::Popup::is_any_open(ctx)
                && self.outgoing.is_none()
            {
                ctx.output_mut(|output| {
                    output.ime = Some(egui::output::IMEOutput { rect, cursor_rect })
                });
                if let Some(text) = self.ime.text(id) {
                    let galley = painter.layout_no_wrap(text.into(), font.clone(), cursor_color);
                    let preview = Rect::from_min_size(pos, galley.size());
                    painter.rect_filled(preview, 0, bg);
                    painter.galley(pos, galley, cursor_color);
                    painter.line_segment(
                        [preview.left_bottom(), preview.right_bottom()],
                        (1.0, cursor_color),
                    );
                }
            }
            if caret_visible {
                match if self.ime.text(id).is_some() {
                    Cursor::Bar
                } else {
                    caret.shape.unwrap_or(appearance.cursor)
                } {
                    Cursor::Block => {
                        let cell = screen
                            .cells
                            .iter()
                            .find(|cell| cell.row == row && cell.col == col);
                        let block = Rect::from_min_size(
                            pos,
                            Vec2::new(
                                width
                                    * if cell.is_some_and(|c| c.wide) {
                                        2.0
                                    } else {
                                        1.0
                                    },
                                height,
                            ),
                        );
                        painter.rect_filled(block, 0, cursor_color);
                        if let Some(cell) = cell {
                            painter.text(pos, Align2::LEFT_TOP, &cell.text, font.clone(), bg);
                        }
                    }
                    Cursor::Outline => {
                        painter.rect_stroke(
                            cursor_rect,
                            0,
                            (1.0, cursor_color),
                            egui::StrokeKind::Inside,
                        );
                    }
                    Cursor::Bar => {
                        painter.rect_filled(
                            Rect::from_min_size(pos, Vec2::new(2.0, height)),
                            0,
                            cursor_color,
                        );
                    }
                    Cursor::Underline => {
                        painter.line_segment(
                            [cursor_rect.left_bottom(), cursor_rect.right_bottom()],
                            (2.0, cursor_color),
                        );
                    }
                }
            }
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

    fn open_search(&mut self) {
        if let Some(session) = self.focused_session() {
            self.search = Some(Search {
                session: session.id,
                query: String::new(),
                status: "Literal, case-sensitive search".into(),
                token: 0,
            });
        }
    }
    fn close_search(&mut self) {
        if let Some(search) = self.search.take()
            && let Some(client) = &self.client
        {
            self.search_token += 1;
            let _ = client.search(search.session, "", false, true, self.search_token);
        }
    }
    fn find(&mut self, backwards: bool, reset: bool) {
        if let Some(search) = &mut self.search
            && let Some(client) = &self.client
        {
            self.search_token += 1;
            search.token = self.search_token;
            match client.search(
                search.session,
                &search.query,
                backwards,
                reset,
                search.token,
            ) {
                Ok(()) => {
                    search.status = if search.query.is_empty() {
                        String::new()
                    } else {
                        "Searching…".into()
                    }
                }
                Err(error) => search.status = error.to_string(),
            }
        }
    }
    fn search_bar(&mut self, ctx: &egui::Context) {
        if let Some(client) = &self.client {
            for result in client.take_search_results() {
                if let Some(search) = &mut self.search
                    && result.id == search.session
                    && result.token == search.token
                {
                    search.status = match result.result {
                        Ok(true) => "Match".into(),
                        Ok(false) => "No match".into(),
                        Err(error) => error,
                    };
                }
            }
        }
        let Some(search) = &mut self.search else {
            return;
        };
        let mut command = None;
        let (enter, shift, escape) = ctx.input(|input| {
            let enter = input.events.iter().find_map(|event| match event {
                egui::Event::Key {
                    key: Key::Enter,
                    pressed: true,
                    modifiers,
                    ..
                } if !modifiers.ctrl && !modifiers.alt && !modifiers.mac_cmd => {
                    Some(modifiers.shift)
                }
                _ => None,
            });
            (
                enter.is_some(),
                enter.unwrap_or(false),
                input.key_pressed(Key::Escape),
            )
        });
        let mut close = escape;
        egui::TopBottomPanel::top("find")
            .exact_height(32.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Find");
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut search.query)
                            .desired_width(230.0)
                            .char_limit(1024),
                    );
                    if search.token == 0 || enter {
                        response.request_focus();
                    }
                    if response.changed() {
                        command = Some((false, true));
                    }
                    if ui.button("Next").clicked() || (enter && !shift) {
                        command = Some((false, false));
                    }
                    if ui.button("Previous").clicked() || (enter && shift) {
                        command = Some((true, false));
                    }
                    if ui.button("×").clicked() || ctx.input(|i| i.key_pressed(Key::Escape)) {
                        close = true;
                    }
                    ui.label(&search.status);
                });
            });
        if let Some((backwards, reset)) = command {
            self.find(backwards, reset);
        }
        if close {
            self.close_search();
            self.ui_input_consumed = true;
        }
    }

    fn saved_tab(&self, tab: &Tab) -> SavedTab {
        SavedTab {
            name: tab.name.clone(),
            focused: tab
                .layout
                .ids()
                .iter()
                .position(|id| *id == tab.focused)
                .unwrap_or(0),
            maximized: tab.maximized,
            layout: self.capture_node(
                &tab.layout,
                self.client
                    .as_ref()
                    .map(|client| client.instance.as_str())
                    .unwrap_or_default(),
            ),
        }
    }
    fn move_tab(&mut self, key: u64, target: Option<String>, ctx: &egui::Context) {
        if self.outgoing.is_some() {
            return;
        }
        let result = (|| -> anyhow::Result<transfers::Pending> {
            let source = &self
                .saved_layout
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Saved layouts are required for tab movement"))?
                .window
                .id;
            anyhow::ensure!(
                self.client
                    .as_ref()
                    .is_some_and(|client| !client.instance.is_empty()),
                "Finish work before upgrading the old service to enable tab movement"
            );
            let tab = self
                .tabs
                .iter()
                .find(|tab| tab.key == key)
                .ok_or_else(|| anyhow::anyhow!("Tab is no longer available"))?;
            let pending = transfers::Offer::send(
                &service::state_dir()?,
                source,
                target.as_deref(),
                self.saved_tab(tab),
            )?;
            if target.is_none() {
                let mut child = Command::new(&self.executable)
                    .args(["--receive-tab", pending.id()])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .spawn()?;
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Ok(pending)
        })();
        match result {
            Ok(pending) => {
                self.outgoing = Some((key, pending));
                self.close_search();
                ctx.request_repaint();
            }
            Err(error) => {
                self.error = Some(format!(
                    "Move tab: {error:#}. The source tab stays available."
                ))
            }
        }
    }
    fn begin_receive(&mut self, offer: transfers::Offer) -> anyhow::Result<()> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Session service is unavailable"))?;
        let ids: Vec<_> = client.infos().iter().map(|info| info.id).collect();
        let references = offer.tab.layout.sessions();
        let reserved: Vec<_> = self
            .tabs
            .iter()
            .flat_map(|tab| self.saved_tab(tab).layout.sessions())
            .chain(
                self.incoming
                    .iter()
                    .flat_map(|offer| offer.tab.layout.sessions()),
            )
            .collect();
        anyhow::ensure!(
            !references
                .iter()
                .any(|reference| reserved.contains(reference)),
            "This window already displays or is receiving a session from the tab"
        );
        let mut attached = Vec::new();
        for reference in references {
            if reference.matches(&client.instance, &ids) {
                if let Err(error) = client.send(Request::Attach(reference.id)) {
                    for id in attached {
                        self.leaving.push(id);
                        let _ = client.send(Request::Detach(id));
                    }
                    return Err(error);
                }
                attached.push(reference.id);
            }
        }
        self.incoming.push(offer);
        Ok(())
    }
    fn poll_transfers(&mut self, ctx: &egui::Context) {
        if self.transfer_poll.elapsed() < Duration::from_millis(100) {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        self.transfer_poll = Instant::now();
        let result = (|| -> anyhow::Result<()> {
            let state = service::state_dir()?;
            if self.window_poll.elapsed() >= Duration::from_secs(1) {
                self.window_poll = Instant::now();
                self.destinations = transfers::live_windows(&state)?;
            }
            let Some(receiver) = self
                .saved_layout
                .as_ref()
                .map(|store| store.window.id.clone())
            else {
                return Ok(());
            };
            if let Some((key, pending)) = &mut self.outgoing {
                match pending.poll()? {
                    transfers::Status::Pending => {}
                    transfers::Status::Accepted { .. } => {
                        let key = *key;
                        if let Some(index) = self.tabs.iter().position(|tab| tab.key == key) {
                            let active_key = self.tabs.get(self.active).map(|tab| tab.key);
                            let tab = self.tabs.remove(index);
                            if let Some(client) = &self.client {
                                for id in tab.layout.ids() {
                                    if self.panes.contains_key(&id) {
                                        self.leaving.push(id);
                                        client.send(Request::Detach(id))?;
                                    } else {
                                        self.missing.remove(&id);
                                    }
                                }
                            }
                            self.active = active_key
                                .and_then(|key| self.tabs.iter().position(|tab| tab.key == key))
                                .unwrap_or_else(|| {
                                    self.active.min(self.tabs.len().saturating_sub(1))
                                });
                        }
                        self.outgoing = None;
                        self.save_layout(ctx)?;
                    }
                    transfers::Status::Expired => {
                        self.outgoing = None;
                        anyhow::bail!(
                            "The receiving window did not accept the tab. Its original view is still available."
                        );
                    }
                }
            }
            for offer in transfers::incoming(&state, &receiver)? {
                if !self.incoming.iter().any(|pending| pending.id == offer.id) {
                    self.begin_receive(offer)?;
                }
            }
            let Some(client) = self.client.clone() else {
                return Ok(());
            };
            let ids: Vec<_> = client.infos().iter().map(|info| info.id).collect();
            let mut ready = Vec::new();
            for offer in &self.incoming {
                let live: Vec<_> = offer
                    .tab
                    .layout
                    .sessions()
                    .into_iter()
                    .filter(|reference| reference.matches(&client.instance, &ids))
                    .map(|reference| reference.id)
                    .collect();
                if live.iter().all(|id| self.panes.contains_key(id)) {
                    ready.push(offer.id.clone());
                }
            }
            for id in ready {
                let index = self
                    .incoming
                    .iter()
                    .position(|offer| offer.id == id)
                    .unwrap();
                let offer = self.incoming.remove(index);
                let live: Vec<_> = offer
                    .tab
                    .layout
                    .sessions()
                    .into_iter()
                    .filter(|reference| reference.matches(&client.instance, &ids))
                    .map(|reference| reference.id)
                    .collect();
                // The automatic tabs created by attachment become one transferred split tab.
                self.tabs
                    .retain(|tab| !tab.layout.ids().iter().any(|pane| live.contains(pane)));
                let layout = restore_node(
                    &offer.tab.layout,
                    &client.instance,
                    &ids,
                    &mut self.missing,
                    &mut self.next_local,
                );
                let panes = layout.ids();
                let key = panes[0];
                self.tabs.push(Tab {
                    key,
                    focused: panes[offer.tab.focused],
                    layout,
                    name: offer.tab.name.clone(),
                    maximized: offer.tab.maximized,
                });
                self.active = self.tabs.len() - 1;
                let accepted = self
                    .save_layout(ctx)
                    .and_then(|()| offer.acknowledge(&state, &receiver));
                if let Err(error) = accepted {
                    if let Some(tab) = self.tabs.pop() {
                        for id in tab.layout.ids() {
                            self.missing.remove(&id);
                        }
                    }
                    for id in live {
                        self.leaving.push(id);
                        client.send(Request::Detach(id))?;
                    }
                    self.active = self.active.min(self.tabs.len().saturating_sub(1));
                    self.save_layout(ctx)?;
                    return Err(error);
                }
                for id in live {
                    client.send(Request::TakeControl(id))?;
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.error = Some(format!("Tab transfer: {error:#}"));
        }
    }

    fn shortcut(&mut self, action: Shortcut, ctx: &egui::Context) {
        match action {
            Shortcut::NewTab => self.new_tab(),
            Shortcut::NewWindow => self.new_window(),
            Shortcut::Close => self.detach(),
            Shortcut::Copy => self.copy(ctx),
            Shortcut::Paste => {
                self.paste_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            }
            Shortcut::Settings => self.open_settings(),
            Shortcut::Search => self.open_search(),
            Shortcut::SplitRight => self.split(Axis::Right),
            Shortcut::SplitBelow => self.split(Axis::Below),
            Shortcut::Maximize => self.maximize(),
            Shortcut::FocusLeft => self.navigate(Key::ArrowLeft, false),
            Shortcut::FocusRight => self.navigate(Key::ArrowRight, false),
            Shortcut::FocusUp => self.navigate(Key::ArrowUp, false),
            Shortcut::FocusDown => self.navigate(Key::ArrowDown, false),
            Shortcut::ResizeLeft => self.navigate(Key::ArrowLeft, true),
            Shortcut::ResizeRight => self.navigate(Key::ArrowRight, true),
            Shortcut::ResizeUp => self.navigate(Key::ArrowUp, true),
            Shortcut::ResizeDown => self.navigate(Key::ArrowDown, true),
            Shortcut::NextTab if !self.tabs.is_empty() => {
                self.active = (self.active + 1) % self.tabs.len()
            }
            Shortcut::PreviousTab if !self.tabs.is_empty() => {
                self.active = (self.active + self.tabs.len() - 1) % self.tabs.len()
            }
            _ => {}
        }
        ctx.request_repaint();
    }

    fn mouse_input(
        &self,
        ctx: &egui::Context,
        pos: Pos2,
        event: input::Mouse,
        mods: egui::Modifiers,
    ) -> Option<(u64, Vec<u8>)> {
        let captured = if matches!(event, input::Mouse::Motion(_) | input::Mouse::Release(_)) {
            self.mouse_button
        } else {
            None
        };
        let (id, rect) = if let Some((id, _, saved_rect)) = captured {
            (
                id,
                self.pane_rects
                    .iter()
                    .find(|(pane, _)| *pane == id)
                    .map(|(_, rect)| *rect)
                    .unwrap_or(saved_rect),
            )
        } else {
            self.pane_rects
                .iter()
                .find(|(_, rect)| rect.contains(pos))
                .map(|(id, rect)| (*id, *rect))?
        };
        let session = self.panes.get(&id)?;
        if !session.controlled() {
            return None;
        }
        let mut mods = mods;
        if captured.is_some() && matches!(event, input::Mouse::Release(_)) {
            mods.shift = false;
        }
        let appearance = &self.settings.current.appearance;
        let width =
            ctx.fonts_mut(|fonts| fonts.glyph_width(&FontId::monospace(appearance.font_size), 'M'));
        let height = appearance.font_size * appearance.line_height;
        let rect = rect.shrink2(Vec2::new(
            appearance
                .padding_x
                .min((rect.width() - 1.0).max(0.0) / 2.0),
            appearance
                .padding_y
                .min((rect.height() - 1.0).max(0.0) / 2.0),
        ));
        let row = (((pos.y - rect.top()).max(0.0) / height) as usize)
            .min(((rect.height() / height) as usize).saturating_sub(1));
        let col = (((pos.x - rect.left()).max(0.0) / width) as usize)
            .min(((rect.width() / width) as usize).saturating_sub(1));
        input::encode_mouse(event, row, col, mods, session.mode()).map(|bytes| (id, bytes))
    }

    fn events(&mut self, ctx: &egui::Context) {
        let mut click_edit = self.click_edit.take();
        let focus = self
            .focused_session()
            .filter(|session| session.controlled())
            .map(|session| session.id);
        self.ime.clear_other_session(
            if ctx.input(|i| i.focused)
                && self.search.is_none()
                && self.settings_dialog.is_none()
                && self.rename.is_none()
                && self.error.is_none()
                && self.end_session.is_none()
            {
                focus
            } else {
                None
            },
        );
        if self.ui_input_consumed || self.dragged_tab.is_some() {
            return;
        }
        if self.search.is_some() {
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.close_search();
            }
            return;
        }
        if self.outgoing.as_ref().is_some_and(|(key, _)| {
            self.tabs
                .get(self.active)
                .is_some_and(|tab| tab.key == *key)
        }) {
            return;
        }
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
        let mut shortcut_text = false;
        let events = ctx.input(|i| i.events.clone());
        let mut composition_frame = focus.is_some_and(|id| self.ime.is_composing(id));
        let modifiers = ctx.input(|i| i.modifiers);
        for (index, event) in events.iter().cloned().enumerate() {
            let boundary = matches!(
                &event,
                egui::Event::PointerButton { .. }
                    | egui::Event::Paste(_)
                    | egui::Event::MouseWheel { .. }
            ) || matches!(&event, egui::Event::Key { key, pressed: true, modifiers, .. } if self.settings.current.shortcuts.action(*key, *modifiers).is_some());
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
                && let Some((id, _)) = self.pane_rects.iter().find(|(_, rect)| rect.contains(*pos))
                && let Some(tab) = self.tabs.get_mut(self.active)
            {
                tab.focused = *id;
                self.ime.clear_other_session(Some(*id));
                ctx.request_repaint();
            }

            if composition_frame && matches!(&event, egui::Event::Key { .. } | egui::Event::Text(_))
            {
                continue;
            }

            if matches!(&event, egui::Event::Key { pressed: true, .. }) {
                shortcut_text = false;
            }
            let clipboard_key = match &event {
                egui::Event::Copy => Some(Key::C),
                egui::Event::Cut => Some(Key::X),
                egui::Event::Paste(_) if !self.paste_requested => Some(Key::V),
                _ => None,
            };
            if let Some(key) = clipboard_key {
                let mods = input::clipboard_modifiers(&events, index, key, modifiers);
                if let Some(action) = self.settings.current.shortcuts.action(key, mods)
                    && !(key == Key::V && action == Shortcut::Paste)
                {
                    self.shortcut(action, ctx);
                    continue;
                }
            }
            if let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = &event
                && let Some(action) = self.settings.current.shortcuts.action(*key, *modifiers)
            {
                self.shortcut(action, ctx);
                shortcut_text = true;
                continue;
            }
            let mouse = match &event {
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    modifiers,
                } => {
                    let button = match button {
                        egui::PointerButton::Primary => Some(0),
                        egui::PointerButton::Middle => Some(1),
                        egui::PointerButton::Secondary => Some(2),
                        _ => None,
                    };
                    button.map(|button| {
                        (
                            *pos,
                            if *pressed {
                                input::Mouse::Press(button)
                            } else {
                                input::Mouse::Release(button)
                            },
                            *modifiers,
                        )
                    })
                }
                egui::Event::PointerMoved(pos) => Some((
                    *pos,
                    input::Mouse::Motion(self.mouse_button.map(|(_, button, _)| button)),
                    modifiers,
                )),
                _ => None,
            };
            if let Some((pos, event, mods)) = mouse
                && let Some((id, bytes)) = self.mouse_input(ctx, pos, event, mods)
            {
                if let input::Mouse::Press(button) = event
                    && let Some((_, rect)) = self.pane_rects.iter().find(|(pane, _)| *pane == id)
                {
                    self.mouse_button = Some((id, button, *rect));
                }
                if matches!(event, input::Mouse::Release(_)) {
                    self.mouse_button = None;
                }
                if let Err(error) = queue_input(&mut pending, &client, id, &bytes) {
                    self.error = Some(format!("{error:#}"));
                }
                continue;
            }
            if matches!(&event, egui::Event::PointerButton { pressed: false, .. }) {
                self.mouse_button = None;
            }
            if matches!(
                &event,
                egui::Event::PointerButton {
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    ..
                }
            ) && let Some((id, row, col)) = click_edit.take()
            {
                if let Err(error) = client.session(id).click_edit(row, col) {
                    self.error = Some(format!("{error:#}"));
                }
                if let Some(activity) = self.caret_activity.get_mut(&id) {
                    activity.1 = Instant::now();
                }
                continue;
            }
            let Some(id) = self
                .tabs
                .get(self.active)
                .map(|tab| tab.focused)
                .filter(|id| self.panes.contains_key(id))
            else {
                continue;
            };
            let session = client.session(id);
            if matches!(
                &event,
                egui::Event::Text(_)
                    | egui::Event::Paste(_)
                    | egui::Event::Key { pressed: true, .. }
            ) && let Some(activity) = self.caret_activity.get_mut(&id)
            {
                activity.1 = Instant::now();
            }
            let result = (|| -> anyhow::Result<()> {
                match event {
                    egui::Event::Copy => {
                        let modifiers =
                            input::clipboard_modifiers(&events, index, Key::C, modifiers);
                        if !self.settings.current.shortcuts.matches(
                            Shortcut::Copy,
                            Key::C,
                            modifiers,
                        ) && !cfg!(target_os = "macos")
                            && modifiers.ctrl
                        {
                            queue_input(&mut pending, &client, session.id, b"\x03")
                        } else {
                            self.copy(ctx);
                            Ok(())
                        }
                    }
                    egui::Event::Paste(text) => {
                        let modifiers =
                            input::clipboard_modifiers(&events, index, Key::V, modifiers);
                        if !self.settings.current.shortcuts.matches(
                            Shortcut::Paste,
                            Key::V,
                            modifiers,
                        ) && !self.paste_requested
                        {
                            queue_input(&mut pending, &client, session.id, b"\x16")
                        } else {
                            self.paste_requested = false;
                            // egui normalizes CRLF; use the native clipboard's
                            // original text so terminal paste preserves every byte.
                            let original = arboard::Clipboard::new()
                                .and_then(|mut clipboard| clipboard.get_text())
                                .unwrap_or(text);
                            session.paste(&original)
                        }
                    }
                    egui::Event::Text(text) => {
                        if shortcut_text {
                            shortcut_text = false;
                            Ok(())
                        } else if modifiers.ctrl || modifiers.mac_cmd {
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
                    egui::Event::Ime(event) => {
                        if let Some(bytes) = self.ime.handle(session.id, &event) {
                            composition_frame = true;
                            queue_input(&mut pending, &client, session.id, &bytes)
                        } else {
                            composition_frame |= self.ime.is_composing(session.id);
                            Ok(())
                        }
                    }
                    egui::Event::Cut => {
                        let modifiers =
                            input::clipboard_modifiers(&events, index, Key::X, modifiers);
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
                    egui::Event::MouseWheel {
                        unit,
                        delta,
                        modifiers,
                    } => {
                        if delta.y == 0.0 {
                            return Ok(());
                        }
                        if let Some(pos) = ctx.input(|i| i.pointer.hover_pos())
                            && let Some((id, bytes)) = self.mouse_input(
                                ctx,
                                pos,
                                input::Mouse::Wheel(delta.y > 0.0),
                                modifiers,
                            )
                        {
                            let steps = match unit {
                                egui::MouseWheelUnit::Line => delta.y.abs().ceil(),
                                _ => (delta.y.abs()
                                    / (self.settings.current.appearance.font_size
                                        * self.settings.current.appearance.line_height))
                                    .ceil(),
                            }
                            .clamp(1.0, 30.0) as usize;
                            for _ in 0..steps {
                                queue_input(&mut pending, &client, id, &bytes)?;
                            }
                            return Ok(());
                        }
                        let lines = match unit {
                            egui::MouseWheelUnit::Line => delta.y * 3.0,
                            egui::MouseWheelUnit::Point => {
                                delta.y
                                    / (self.settings.current.appearance.font_size
                                        * self.settings.current.appearance.line_height)
                            }
                            egui::MouseWheelUnit::Page => delta.y * 30.0,
                        };
                        let hovered = ctx
                            .input(|i| i.pointer.hover_pos())
                            .and_then(|pos| {
                                self.pane_rects.iter().find(|(_, rect)| rect.contains(pos))
                            })
                            .and_then(|(id, _)| self.panes.get(id))
                            .unwrap_or(&session);
                        if hovered.controlled() {
                            hovered.scroll(lines.round() as i32)
                        } else {
                            Ok(())
                        }
                    }
                    _ => Ok(()),
                }
            })();
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
        if self.settings_poll.elapsed() >= Duration::from_secs(1) {
            self.settings_poll = Instant::now();
            let previous = self.settings.observed.clone();
            if self.settings.poll() {
                self.apply_appearance(ctx);
            }
            if previous != self.settings.observed && self.settings.notice.is_some() {
                self.open_settings();
            }
        }
        if let Some((id, since)) = self.pending_copy
            && since.elapsed() >= Duration::from_millis(100)
        {
            if let Some(text) = self
                .panes
                .get(&id)
                .and_then(|session| session.selected_text())
            {
                ctx.copy_text(text);
            }
            self.pending_copy = None;
        }
        ctx.request_repaint_after(Duration::from_secs(1));
        self.ui_input_consumed = false;
        self.sync_tabs();
        self.poll_transfers(ctx);
        let tab_rects = self.tab_bar(ctx);
        self.search_bar(ctx);
        self.terminal(ctx);
        self.tab_drag(ctx, &tab_rects);
        self.events(ctx);
        self.show_settings(ctx);
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
                                        Ok(()) => tab.name = name.clone(),
                                        Err(error) => self.error = Some(format!("{error:#}")),
                                    }
                                } else {
                                    tab.name = name;
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
        if self.layout_poll.elapsed() >= Duration::from_secs(1)
            || ctx.input(|i| i.viewport().close_requested())
        {
            self.layout_poll = Instant::now();
            if let Err(error) = self.save_layout(ctx) {
                self.error = Some(format!(
                    "Save layout: {error:#}. Running work is unchanged; retry after fixing the state folder."
                ));
            }
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
    let receive = if program.first().is_some_and(|arg| arg == "--receive-tab") {
        program.remove(0);
        if program.is_empty() {
            eprintln!("Missing tab transfer ID");
            std::process::exit(1);
        }
        Some(program.remove(0))
    } else {
        None
    };
    let mut fresh = program
        .first()
        .is_some_and(|s| s == "--new-window" || s == "--no-restore");
    if fresh {
        program.remove(0);
    }
    let requested = if program.first().is_some_and(|s| s == "--restore-window") {
        program.remove(0);
        if program.is_empty() {
            eprintln!("Missing saved window ID");
            std::process::exit(1);
        }
        Some(program.remove(0))
    } else {
        None
    };
    if program.first().is_some_and(|s| s == "--") {
        program.remove(0);
    }
    fresh |= !program.is_empty() || receive.is_some();
    let settings = match Paths::discover() {
        Ok(paths) => Store::load(paths),
        Err(error) => {
            eprintln!("{error:#}");
            std::process::exit(1);
        }
    };
    let restore = settings.current.restore_layout && !fresh;
    let boot = service::state_dir()
        .and_then(|path| layouts::Store::open(&path, requested.as_deref(), restore));
    let (saved_layout, restored, mut notice, others) = match boot {
        Ok(Some(boot)) => (Some(boot.store), boot.restored, boot.notice, boot.others),
        Ok(None) => return,
        Err(error) => (
            None,
            false,
            Some(format!(
                "Could not restore/save layouts: {error:#}. Saved files were left untouched."
            )),
            Vec::new(),
        ),
    };
    for id in others {
        let result = std::env::current_exe().and_then(|exe| {
            Command::new(exe)
                .args(["--restore-window", &id])
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
            Err(error) => {
                notice = Some(format!(
                    "Could not reopen a saved window: {error}. Its layout is still saved."
                ))
            }
        }
    }
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Opterm")
        .with_app_id("dev.opterm.Opterm")
        .with_icon(app_icon())
        .with_inner_size([1000.0, 680.0])
        .with_min_inner_size([320.0, 160.0]);
    if restored && let Some(store) = &saved_layout {
        viewport = viewport
            .with_inner_size(store.window.size)
            .with_maximized(store.window.maximized);
        if let Some(position) = store.window.position {
            viewport = viewport.with_position(position);
        }
    }
    if let Err(error) = eframe::run_native(
        "Opterm",
        eframe::NativeOptions {
            viewport,
            ..Default::default()
        },
        Box::new(move |cc| {
            Ok(Box::new(App::new(
                cc,
                program,
                settings,
                saved_layout,
                restored,
                match receive {
                    Some(id) => Startup::Receive(id),
                    None if fresh || !restore => Startup::Fresh,
                    None => Startup::Existing,
                },
                notice,
            )?))
        }),
    ) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod font_tests {
    use super::*;
    #[test]
    fn terminal_has_modern_outline_emoji_without_system_fonts() {
        let ctx = egui::Context::default();
        install_font(&ctx, &fontdb::Database::new(), "System monospace");
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            ctx.fonts_mut(|fonts| assert!(fonts.has_glyphs(&FontId::monospace(14.0), "🙂👩💻🦀")));
        });
    }
    #[test]
    fn installed_cjk_fallback_supplies_terminal_glyphs() {
        let mut database = fontdb::Database::new();
        database.load_system_fonts();
        if cjk_fallback(&database).is_none() {
            return;
        }
        let ctx = egui::Context::default();
        install_font(&ctx, &database, "System monospace");
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            ctx.fonts_mut(|fonts| {
                assert!(fonts.has_glyphs(&FontId::monospace(14.0), "中文日本語"));
            });
        });
    }
}
