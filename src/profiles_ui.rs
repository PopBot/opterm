use eframe::egui::{self, Color32, FontId, Vec2};
use opterm::{
    profiles::{DARK, Environment, LIGHT, Profile, SHELL, Theme, fresh_id},
    settings::Settings,
};
use std::path::Path;

pub struct Editor {
    profile: String,
    theme: String,
    themes: bool,
    file: String,
    message: Option<String>,
}
impl Default for Editor {
    fn default() -> Self {
        Self {
            profile: SHELL.into(),
            theme: DARK.into(),
            themes: false,
            file: String::new(),
            message: None,
        }
    }
}
fn unique(base: &str, names: impl IntoIterator<Item = String>) -> String {
    let names: Vec<_> = names.into_iter().map(|s| s.to_lowercase()).collect();
    if !names.contains(&base.to_lowercase()) {
        return base.into();
    }
    (2..)
        .map(|n| format!("{base} {n}"))
        .find(|s| !names.contains(&s.to_lowercase()))
        .unwrap()
}
impl Editor {
    pub fn show(&mut self, ui: &mut egui::Ui, settings: &mut Settings) -> Option<String> {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.themes, false, "Launch profiles");
            ui.selectable_value(&mut self.themes, true, "Terminal themes");
        });
        if self.themes {
            self.themes(ui, settings);
        } else {
            self.profiles(ui, settings);
        }
        self.message.take()
    }
    fn add_profile(&mut self, settings: &mut Settings, mut profile: Profile) {
        match fresh_id("profile") {
            Ok(id) => {
                profile.id = id.clone();
                profile.name = unique(
                    &profile.name,
                    settings.all_profiles().into_iter().map(|p| p.name),
                );
                self.profile = id;
                settings.profiles.push(profile);
            }
            Err(error) => self.message = Some(format!("{error:#}")),
        }
    }
    fn profiles(&mut self, ui: &mut egui::Ui, settings: &mut Settings) {
        if settings.profile(&self.profile).is_none() {
            self.profile = SHELL.into();
        }
        let selected = settings.profile(&self.profile).unwrap();
        ui.horizontal(|ui| {
            if let Some(provider) = crate::provider_ui::profile_provider(&selected) {
                ui.add(crate::provider_ui::image(ui, provider, 18.0));
            }
            egui::ComboBox::from_id_salt("profile-choice")
                .width(190.0)
                .selected_text(&selected.name)
                .show_ui(ui, |ui| {
                    for p in settings.all_profiles() {
                        let response = ui.add(
                            crate::provider_ui::button(
                                ui,
                                crate::provider_ui::profile_provider(&p),
                                &p.name,
                            )
                            .selected(self.profile == p.id),
                        );
                        if response.clicked() {
                            self.theme = p.theme;
                            self.profile = p.id;
                            ui.close();
                        }
                    }
                });
            if ui.button("Add").clicked() {
                self.add_profile(
                    settings,
                    Profile {
                        name: "Profile".into(),
                        ..Profile::default()
                    },
                );
            }
            if ui.button("Duplicate").clicked() {
                let mut p = selected.clone();
                p.name = format!("{} copy", p.name);
                self.add_profile(settings, p);
            }
            if ui
                .add_enabled(self.profile != SHELL, egui::Button::new("Delete"))
                .clicked()
            {
                settings.profiles.retain(|p| p.id != self.profile);
                if settings.default_profile == self.profile {
                    settings.default_profile = SHELL.into();
                    self.message = Some(
                        "Default profile reverted to Shell. Running sessions keep working.".into(),
                    );
                } else {
                    self.message = Some(
                        "Profile removed from the draft. Running sessions keep working.".into(),
                    );
                }
                self.profile = SHELL.into();
            }
        });
        ui.horizontal(|ui| {
            ui.label("Workspace folder");
            ui.add(
                egui::TextEdit::singleline(&mut settings.workspace_directory)
                    .hint_text("Blank uses home")
                    .desired_width(360.0),
            );
        });
        ui.horizontal(|ui| {
            let default = settings.default_profile == self.profile;
            if ui
                .add_enabled(!default, egui::Button::new("Use as default"))
                .clicked()
            {
                settings.default_profile = self.profile.clone();
            }
            ui.weak(if default {
                "Default for new tabs, splits and windows"
            } else {
                "Launch changes affect new sessions"
            });
        });
        let themes = settings.all_themes();
        if self.profile == SHELL {
            ui.weak("Built-in fallback. Duplicate to customize.");
            ui.add_enabled_ui(false, |ui| {
                profile_form(ui, &mut Profile::default(), &themes)
            });
        } else if let Some(profile) = settings.profiles.iter_mut().find(|p| p.id == self.profile) {
            profile_form(ui, profile, &themes);
        }
        if ui.small_button("Add missing AI profiles").clicked() {
            settings.add_provider_presets();
        }
        ui.weak("Provider colors apply to the terminal theme. Font follows Appearance.");
    }
    fn add_theme(&mut self, settings: &mut Settings, mut theme: Theme) {
        match fresh_id("theme") {
            Ok(id) => {
                theme.id = id.clone();
                theme.name = unique(
                    &theme.name,
                    settings.all_themes().into_iter().map(|t| t.name),
                );
                self.theme = id;
                settings.themes.push(theme);
            }
            Err(error) => self.message = Some(format!("{error:#}")),
        }
    }
    fn themes(&mut self, ui: &mut egui::Ui, settings: &mut Settings) {
        if self.theme != DARK
            && self.theme != LIGHT
            && !settings.themes.iter().any(|t| t.id == self.theme)
        {
            self.theme = DARK.into();
        }
        let selected = settings.theme(&self.theme);
        let provider_for_theme = |id: &str| {
            if matches!(id, DARK | LIGHT) {
                return None;
            }
            settings
                .profiles
                .iter()
                .find(|p| p.theme == id)
                .and_then(crate::provider_ui::profile_provider)
        };
        let provider = provider_for_theme(&selected.id);
        ui.horizontal(|ui| {
            if let Some(provider) = provider { ui.add(crate::provider_ui::image(ui, provider, 18.0)); }
            egui::ComboBox::from_id_salt("theme-choice").width(190.0).selected_text(&selected.name).show_ui(ui, |ui| {
                for t in settings.all_themes() { ui.selectable_value(&mut self.theme, t.id, t.name); }
            });
            if ui.button("Add").clicked() { self.add_theme(settings, Theme { name: "Theme".into(), ..Theme::dark() }); }
            if ui.button("Duplicate").clicked() { let mut t = selected.clone(); t.name = format!("{} copy", t.name); self.add_theme(settings, t); }
            if ui.add_enabled(self.theme != DARK && self.theme != LIGHT, egui::Button::new("Delete")).clicked() {
                settings.themes.retain(|t| t.id != self.theme);
                for p in &mut settings.profiles { if p.theme == self.theme { p.theme = DARK.into(); } }
                self.theme = DARK.into(); self.message = Some("Deleted theme. Profiles and live views using it fall back to Dark after Save; programs keep running.".into());
            }
        });
        let builtin = self.theme == DARK || self.theme == LIGHT;
        if builtin {
            ui.weak("Built-in theme. Duplicate to customize.");
        }
        let mut fallback = settings.theme(&self.theme);
        let theme = settings
            .themes
            .iter_mut()
            .find(|t| t.id == self.theme)
            .unwrap_or(&mut fallback);
        ui.add_enabled_ui(!builtin, |ui| {
            ui.horizontal(|ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut theme.name);
            });
            ui.horizontal(|ui| {
                for (label, color) in [
                    ("Text", &mut theme.foreground),
                    ("Background", &mut theme.background),
                    ("Cursor", &mut theme.cursor),
                    ("Selection", &mut theme.selection),
                ] {
                    ui.label(label);
                    ui.color_edit_button_srgb(&mut color.0);
                }
            });
            egui::Grid::new("ansi-colors")
                .spacing([8.0, 4.0])
                .show(ui, |ui| {
                    for row in 0..2 {
                        for col in 0..8 {
                            let index = row * 8 + col;
                            ui.vertical(|ui| {
                                ui.small(index.to_string());
                                ui.color_edit_button_srgb(&mut theme.ansi[index].0);
                            });
                        }
                        ui.end_row();
                    }
                });
        });
        preview(ui, theme);
        ui.collapsing("Import / export TOML theme", |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.file)
                    .hint_text("Full path to a .toml theme file")
                    .desired_width(ui.available_width()),
            );
            ui.horizontal(|ui| {
                if ui.button("Import into draft").clicked() {
                    match Theme::import(Path::new(&self.file)) {
                        Ok(theme) => {
                            self.add_theme(settings, theme);
                            self.message =
                                Some("Theme imported into the draft. Save to apply it.".into());
                        }
                        Err(error) => self.message = Some(format!("Import theme: {error:#}")),
                    }
                }
                if ui.button("Export selected theme").clicked() {
                    self.message = Some(
                        match settings.theme(&self.theme).export(Path::new(&self.file)) {
                            Ok(()) => format!("Theme exported to {}", self.file),
                            Err(error) => format!("Export theme: {error:#}"),
                        },
                    );
                }
            });
        });
    }
}
fn profile_form(ui: &mut egui::Ui, profile: &mut Profile, themes: &[Theme]) {
    egui::Grid::new("profile-form")
        .min_col_width(95.0)
        .spacing([12.0, 6.0])
        .show(ui, |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            for (label, value, hint) in [
                ("Name", &mut profile.name, "Profile name"),
                (
                    "Executable",
                    &mut profile.executable,
                    "Blank uses system shell",
                ),
                (
                    "Starting folder",
                    &mut profile.directory,
                    "Blank uses workspace folder, then home",
                ),
            ] {
                ui.label(label);
                ui.add_sized(
                    [370.0, 20.0],
                    egui::TextEdit::singleline(value).hint_text(hint),
                );
                ui.end_row();
            }
            ui.label("Terminal theme");
            let name = themes
                .iter()
                .find(|t| t.id == profile.theme)
                .map(|t| t.name.as_str())
                .unwrap_or("Dark (removed theme)");
            egui::ComboBox::from_id_salt("profile-theme")
                .selected_text(name)
                .show_ui(ui, |ui| {
                    for theme in themes {
                        ui.selectable_value(&mut profile.theme, theme.id.clone(), &theme.name);
                    }
                });
            ui.end_row();
            ui.label("AI provider");
            ui.horizontal(|ui| {
                if let Some(provider) = crate::provider_ui::profile_provider(profile) {
                    ui.add(crate::provider_ui::image(ui, provider, 18.0));
                }
                egui::ComboBox::from_id_salt("profile-provider")
                    .selected_text(profile.provider.map(|p| p.name()).unwrap_or("Automatic"))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut profile.provider, None, "Automatic");
                        for provider in opterm::agents::Provider::ALL {
                            if ui
                                .add(
                                    crate::provider_ui::button(ui, Some(provider), provider.name())
                                        .selected(profile.provider == Some(provider)),
                                )
                                .clicked()
                            {
                                profile.provider = Some(provider);
                                ui.close();
                            }
                        }
                    });
            });
            ui.end_row();
        });
    ui.collapsing("Arguments", |ui| {
        let mut remove = None;
        for (i, argument) in profile.arguments.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.small((i + 1).to_string());
                ui.add(egui::TextEdit::singleline(argument).desired_width(420.0));
                if ui.small_button("×").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            profile.arguments.remove(i);
        }
        if ui.button("Add argument").clicked() {
            profile.arguments.push(String::new());
        }
        ui.weak("Each row is one argument. Spaces are preserved; no shell parsing.");
    });
    ui.collapsing("Environment overrides", |ui| {
        let mut remove = None;
        for (i, env) in profile.environment.iter_mut().enumerate() { ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut env.name).hint_text("NAME").desired_width(120.0));
            ui.add(egui::TextEdit::singleline(&mut env.value).password(env.secret).hint_text("Value").desired_width(215.0));
            ui.checkbox(&mut env.secret, "Secret"); if ui.small_button("×").clicked() { remove = Some(i); }
        }); }
        if let Some(i) = remove { profile.environment.remove(i); }
        if ui.button("Add variable").clicked() { profile.environment.push(Environment::default()); }
        ui.weak("Marked secrets are hidden here and excluded from recovery. Active TOML stores their values; reenter them after recovery.");
    });
}
fn color(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}
fn preview(ui: &mut egui::Ui, theme: &Theme) {
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 66.0), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 3, color(theme.background.0));
    painter.text(
        rect.min + Vec2::new(6.0, 6.0),
        egui::Align2::LEFT_TOP,
        "$ terminal preview",
        FontId::monospace(14.0),
        color(theme.foreground.0),
    );
    for (i, rgb) in theme.ansi.iter().enumerate() {
        let pos = rect.min + Vec2::new(6.0 + i as f32 * 30.0, 30.0);
        painter.text(
            pos,
            egui::Align2::LEFT_TOP,
            "Aa",
            FontId::monospace(14.0),
            color(rgb.0),
        );
    }
}
