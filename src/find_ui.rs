use eframe::egui::{self, Key};
use opterm::{
    search::{Case, Mode, Options, SessionResult},
    service::client::Client,
};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Focused,
    All,
}

pub struct Search {
    pub session: u64,
    pub query: String,
    pub status: String,
    pub token: u64,
    pub options: Options,
    pub scope: Scope,
    pub pending_session: Option<u64>,
    results: Vec<SessionResult>,
    changed: Option<Instant>,
    focus_query: bool,
}

pub enum Action {
    Find { backwards: bool, reset: bool },
    Activate(u64),
    Close,
}

impl Search {
    pub fn new(session: u64) -> Self {
        Self {
            session,
            query: String::new(),
            status: String::new(),
            token: 0,
            options: Options {
                mode: Mode::Literal,
                case: Case::Smart,
            },
            scope: Scope::Focused,
            pending_session: None,
            results: Vec::new(),
            changed: None,
            focus_query: true,
        }
    }

    pub fn receive(&mut self, client: &Client) {
        for result in client.take_search_results() {
            if self.changed.is_none() && result.id == self.session && result.token == self.token {
                self.status = match result.result {
                    Ok(true) => "Match".into(),
                    Ok(false) => "No match".into(),
                    Err(error) => error,
                };
            }
        }
        for result in client.take_advanced_search_results() {
            if self.changed.is_none() && result.id == self.session && result.token == self.token {
                self.status = match result.result {
                    Ok(summary) if self.query.is_empty() => {
                        let _ = summary;
                        String::new()
                    }
                    Ok(summary) => format!(
                        "{} / {}{}",
                        summary.current.unwrap_or(0),
                        summary.count,
                        if summary.truncated {
                            "+ · bounded"
                        } else {
                            ""
                        }
                    ),
                    Err(error) => error,
                };
            }
        }
        for result in client.take_all_search_results() {
            if self.changed.is_none() && result.token == self.token {
                match result.result {
                    Ok(summary) => {
                        if self.scope == Scope::Focused {
                            let result = summary
                                .sessions
                                .iter()
                                .find(|result| result.id == self.session);
                            let count = result.map_or(0, |result| result.summary.count);
                            let bounded = summary.truncated
                                || result.is_some_and(|result| result.summary.truncated);
                            self.status = format!(
                                "{count}{} matches · observed · Take control to navigate",
                                if bounded { "+ · bounded" } else { "" }
                            );
                            self.results.clear();
                            continue;
                        }
                        self.status = format!(
                            "{} matching sessions{}",
                            summary.sessions.len(),
                            if summary.truncated { " · bounded" } else { "" }
                        );
                        self.results = summary.sessions;
                    }
                    Err(error) => {
                        self.status = error;
                        self.results.clear();
                    }
                }
            }
        }
    }

    pub fn send(
        &mut self,
        client: &std::sync::Arc<Client>,
        token: u64,
        backwards: bool,
        reset: bool,
    ) {
        self.token = token;
        self.changed = None;
        let result = if client.advanced_search_supported {
            if self.scope == Scope::All || !client.session(self.session).controlled() {
                client.search_all(&self.query, self.options, token)
            } else {
                client.search_advanced(
                    self.session,
                    &self.query,
                    self.options,
                    backwards,
                    reset,
                    token,
                )
            }
        } else {
            if !client.session(self.session).controlled() {
                self.status = "Observed session · Take control to navigate its scrollback".into();
                return;
            }
            client.search(self.session, &self.query, backwards, reset, token)
        };
        self.status = match result {
            Ok(()) => "Searching…".into(),
            Err(error) => error.to_string(),
        };
    }

    pub fn show(&mut self, ctx: &egui::Context, client: &std::sync::Arc<Client>) -> Option<Action> {
        self.receive(client);
        let mut action = None;
        let (enter, escape) = ctx.input(|input| {
            (
                input.events.iter().find_map(|event| match event {
                    egui::Event::Key {
                        key: Key::Enter,
                        pressed: true,
                        modifiers,
                        ..
                    } if !modifiers.ctrl && !modifiers.alt && !modifiers.mac_cmd => {
                        Some(modifiers.shift)
                    }
                    _ => None,
                }),
                input.key_pressed(Key::Escape),
            )
        });
        if escape {
            action = Some(Action::Close);
        }
        egui::TopBottomPanel::top("find").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Find");
                let response = ui.add(egui::TextEdit::singleline(&mut self.query).desired_width(230.0).char_limit(1024).hint_text("Search terminal output"));
                if self.focus_query { response.request_focus(); self.focus_query = false; }
                if response.changed() { self.changed = Some(Instant::now()); self.results.clear(); }
                ui.add_enabled_ui(client.advanced_search_supported, |ui| {
                    let previous = (self.options, self.scope);
                    let mut regex = self.options.mode == Mode::Regex;
                    if ui.checkbox(&mut regex, "Regex").on_hover_text("Use a regular expression; turn off for literal text").changed() { self.options.mode = if regex { Mode::Regex } else { Mode::Literal }; }
                    egui::ComboBox::from_id_salt("search_case").selected_text(match self.options.case { Case::Sensitive => "Case sensitive", Case::Insensitive => "Ignore case", Case::Smart => "Smart case" }).show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.options.case, Case::Smart, "Smart case");
                        ui.selectable_value(&mut self.options.case, Case::Sensitive, "Case sensitive");
                        ui.selectable_value(&mut self.options.case, Case::Insensitive, "Ignore case");
                    });
                    egui::ComboBox::from_id_salt("search_scope").selected_text(if self.scope == Scope::Focused { "This session" } else { "All sessions" }).show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.scope, Scope::Focused, "This session");
                        ui.selectable_value(&mut self.scope, Scope::All, "All sessions");
                    });
                    if previous != (self.options, self.scope) { self.changed = Some(Instant::now()); self.results.clear(); }
                });
                if self.scope == Scope::Focused {
                    let controlled = client.session(self.session).controlled();
                    if ui.add_enabled(controlled, egui::Button::new("Previous")).clicked() { action = Some(Action::Find { backwards: true, reset: false }); }
                    if ui.add_enabled(controlled, egui::Button::new("Next")).clicked() { action = Some(Action::Find { backwards: false, reset: false }); }
                }
                let close = ui.button("×").on_hover_text("Close find (Esc)");
                close.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Close find"));
                if close.clicked() { action = Some(Action::Close); }
                ui.label(&self.status);
            });
            if !client.advanced_search_supported { ui.weak("This service supports literal, case-sensitive search. Enhanced search is available after the service is restarted."); }
            if self.scope == Scope::All && !self.results.is_empty() {
                egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                    let infos = client.infos();
                    for result in &self.results {
                        let title = infos.iter().find(|info| info.id == result.id).map(|info| info.name.as_deref().unwrap_or(&info.title)).unwrap_or("Session");
                        let text = format!("{} · {}{} matches · {}", title, result.summary.count, if result.summary.truncated { "+" } else { "" }, result.preview);
                        if ui.selectable_label(false, text).clicked() { action = Some(Action::Activate(result.id)); }
                    }
                });
            }
        });
        if action.is_none() {
            if let Some(backwards) = enter {
                action = Some(Action::Find {
                    backwards,
                    reset: self.scope == Scope::All,
                });
            } else if let Some(changed) = self.changed {
                let elapsed = changed.elapsed();
                if elapsed >= Duration::from_millis(120) {
                    action = Some(Action::Find {
                        backwards: false,
                        reset: true,
                    });
                } else {
                    ctx.request_repaint_after(Duration::from_millis(120) - elapsed);
                }
            }
        }
        action
    }
}
