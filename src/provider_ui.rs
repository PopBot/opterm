use eframe::egui::{self, Color32, Vec2};
use opterm::agents::Provider;

pub fn image(ui: &egui::Ui, provider: Provider, size: f32) -> egui::Image<'static> {
    let source = match provider {
        Provider::Claude => egui::include_image!("../assets/providers/claude.svg"),
        Provider::Codex => egui::include_image!("../assets/providers/codex.svg"),
        Provider::Copilot => egui::include_image!("../assets/providers/copilot.svg"),
        Provider::Antigravity => egui::include_image!("../assets/providers/antigravity.svg"),
    };
    let tint = match provider {
        Provider::Antigravity => Color32::WHITE,
        Provider::Codex => ui.visuals().text_color(),
        _ => {
            let [r, g, b] = provider.accent();
            Color32::from_rgb(r, g, b)
        }
    };
    egui::Image::new(source)
        .fit_to_exact_size(Vec2::splat(size))
        .tint(tint)
}
pub fn button<'a>(
    ui: &egui::Ui,
    provider: Option<Provider>,
    text: impl Into<egui::WidgetText>,
) -> egui::Button<'a> {
    egui::Button::opt_image_and_text(provider.map(|p| image(ui, p, 14.0)), Some(text.into()))
}
pub fn profile_provider(profile: &opterm::profiles::Profile) -> Option<Provider> {
    profile
        .provider
        .or_else(|| Provider::from_executable(&profile.executable))
}
