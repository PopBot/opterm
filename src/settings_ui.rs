use eframe::egui::{self, Color32, FontId, Vec2};
use opterm::settings::{Appearance, Chrome, Cursor, Revision, Settings, Store};

pub struct Dialog {
    pub draft: Settings,
    pub baseline: Revision,
    section: usize,
    pub message: Option<String>,
    pub conflict: bool,
    profiles: crate::profiles_ui::Editor,
}
#[derive(Clone, Copy)]
pub enum Action {
    Save,
    Cancel,
    Reload,
    Replace,
    Restore,
    Edit,
    BackupFolder,
}
impl Dialog {
    pub fn new(store: &Store) -> Self {
        Self {
            draft: store.current.clone(),
            baseline: store.disk_revision(),
            section: 0,
            message: None,
            conflict: false,
            profiles: crate::profiles_ui::Editor::default(),
        }
    }
    pub fn show(
        &mut self,
        root: &egui::Context,
        store: &Store,
        fonts: &[String],
    ) -> Option<Action> {
        let mut action = None;
        root.show_viewport_immediate(
            egui::ViewportId::from_hash_of("opterm-settings"),
            egui::ViewportBuilder::default()
                .with_title("Opterm Settings")
                .with_inner_size([620.0, 540.0])
                .with_min_inner_size([580.0, 460.0]),
            |ctx, class| {
                if ctx.input(|i| i.viewport().close_requested()) {
                    action = Some(Action::Cancel);
                }
                let content = |ui: &mut egui::Ui| {
                    self.content(ui, store, fonts, &mut action);
                };
                if class == egui::ViewportClass::Embedded {
                    egui::Window::new("Opterm Settings").show(ctx, content);
                } else {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::central_panel(&ctx.style()).inner_margin(12))
                        .show(ctx, content);
                }
            },
        );
        action
    }
    fn content(
        &mut self,
        ui: &mut egui::Ui,
        store: &Store,
        fonts: &[String],
        action: &mut Option<Action>,
    ) {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        ui.horizontal(|ui| {
            for (index, name) in [
                "Appearance",
                "Shortcuts & clipboard",
                "Profiles",
                "Sessions & history",
            ]
            .iter()
            .enumerate()
            {
                ui.selectable_value(&mut self.section, index, *name);
            }
        });
        ui.separator();
        let footer = 136.0;
        egui::ScrollArea::vertical().max_height((ui.available_height() - footer).max(160.0)).show(ui, |ui| {
            match self.section {
                0 => appearance(ui, &mut self.draft.appearance, fonts),
                1 => {
                    shortcuts(ui, &mut self.draft);

                }
                2 => {
                    if let Some(message) = self.profiles.show(ui, &mut self.draft) { self.message = Some(message); }
                }
                _ => {
                    ui.checkbox(&mut self.draft.restore_layout, "Restore saved windows, tabs and splits on startup");
                    ui.label("Closing the desktop leaves shells and agents running in the session service.");
                    ui.label("Live scrollback: 10,000 lines per session. Saved history is not implemented yet.");
                    ui.weak("Appearance saves do not restart sessions or the service.");
                }
            }
            if let Some(message) = self.message.as_ref().or(store.notice.as_ref()) {
                ui.separator(); ui.label(egui::RichText::new(message).color(if ui.visuals().dark_mode { Color32::from_rgb(245, 195, 110) } else { Color32::from_rgb(135, 65, 0) }));
            }
        });
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    *action = Some(Action::Save);
                }
                if ui.button("Cancel").clicked() {
                    *action = Some(Action::Cancel);
                }
                if self.conflict {
                    if ui.button("Reload file").clicked() {
                        *action = Some(Action::Reload);
                    }
                    if ui.button("Replace file with my draft").clicked() {
                        *action = Some(Action::Replace);
                    }
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Restore last valid settings").clicked() {
                    *action = Some(Action::Restore);
                }
                if ui.button("Open backup folder").clicked() {
                    *action = Some(Action::BackupFolder);
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Edit configuration…").clicked() {
                    *action = Some(Action::Edit);
                }
                ui.label("External edits are validated automatically.");
            });
            ui.label(egui::RichText::new(store.paths.config.display().to_string()).small())
                .on_hover_text("Settings file");
            ui.separator();
        });
    }
}
fn appearance(ui: &mut egui::Ui, a: &mut Appearance, fonts: &[String]) {
    egui::Grid::new("appearance-form")
        .min_col_width(110.0)
        .spacing([28.0, 7.0])
        .show(ui, |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            ui.label("App appearance");
            egui::ComboBox::from_id_salt("chrome")
                .selected_text(format!("{:?}", a.chrome))
                .show_ui(ui, |ui| {
                    for value in [Chrome::System, Chrome::Dark, Chrome::Light] {
                        ui.selectable_value(&mut a.chrome, value, format!("{value:?}"));
                    }
                });
            ui.end_row();
            ui.label("Terminal font");
            egui::ComboBox::from_id_salt("font")
                .selected_text(&a.font_family)
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut a.font_family,
                        "System monospace".into(),
                        "System monospace",
                    );
                    for name in fonts {
                        ui.selectable_value(&mut a.font_family, name.clone(), name);
                    }
                });
            ui.end_row();
            for (label, value, range, speed, suffix) in [
                ("Font size", &mut a.font_size, 8.0..=40.0, 1.0, " px"),
                (
                    "Horizontal padding",
                    &mut a.padding_x,
                    0.0..=32.0,
                    1.0,
                    " px",
                ),
                ("Vertical padding", &mut a.padding_y, 0.0..=32.0, 1.0, " px"),
                ("Line height", &mut a.line_height, 1.0..=2.0, 0.05, " ×"),
            ] {
                ui.label(label);
                ui.add(
                    egui::DragValue::new(value)
                        .range(range)
                        .speed(speed)
                        .suffix(suffix),
                );
                ui.end_row();
            }
            ui.label("Cursor");
            egui::ComboBox::from_id_salt("cursor")
                .selected_text(format!("{:?}", a.cursor))
                .show_ui(ui, |ui| {
                    for value in [Cursor::Outline, Cursor::Bar, Cursor::Underline] {
                        ui.selectable_value(&mut a.cursor, value, format!("{value:?}"));
                    }
                });
            ui.end_row();
        });
    ui.add_space(10.0);
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 82.0), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 3, Color32::from_rgb(31, 31, 33));
    let origin = rect.min + Vec2::new(a.padding_x, a.padding_y);
    let font = FontId::monospace(a.font_size);
    for (index, (text, color)) in [
        ("$ cargo build", Color32::from_rgb(225, 225, 226)),
        ("Finished successfully", Color32::from_rgb(117, 183, 114)),
        ("$ ▏", Color32::from_rgb(225, 225, 226)),
    ]
    .into_iter()
    .enumerate()
    {
        painter.text(
            origin + Vec2::new(0.0, index as f32 * a.font_size * a.line_height),
            egui::Align2::LEFT_TOP,
            text,
            font.clone(),
            color,
        );
    }
    ui.weak("Terminal color themes are selected in Profiles.");
}

fn shortcuts(ui: &mut egui::Ui, draft: &mut Settings) {
    ui.checkbox(
        &mut draft.copy_on_select,
        "Copy selected text automatically",
    );
    ui.weak(
        "Ctrl+C interrupts the terminal unless deliberately rebound. Empty bindings are disabled.",
    );
    ui.horizontal(|ui| {
        if ui.button("Reset shortcuts").clicked() {
            draft.shortcuts = opterm::shortcuts::Shortcuts::default();
        }
        ui.weak("Example: Ctrl+Shift+T or Cmd+T");
    });
    egui::Grid::new("editable-shortcuts")
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            for action in opterm::shortcuts::Action::ALL {
                ui.label(action.label());
                let binding = draft.shortcuts.0.entry(action).or_default();
                ui.add(egui::TextEdit::singleline(binding).desired_width(190.0));
                if ui.small_button("Unbind").clicked() {
                    binding.clear();
                }
                ui.end_row();
            }
        });
    if let Err(error) = draft.shortcuts.validate() {
        ui.colored_label(Color32::from_rgb(190, 85, 55), error.to_string());
    }
    ui.weak("To reassign an occupied shortcut, explicitly Unbind its previous action before Save.");
}
