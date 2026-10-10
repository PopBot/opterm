//! Verify that the bundled fallback has real outline glyphs in the desktop renderer.
use eframe::egui::{Color32, Context, FontData, FontDefinitions, FontFamily, FontId, RawInput};

#[test]
fn bundled_emoji_outline_is_supported_and_rasterizes_modern_glyphs() {
    let mut fonts = FontDefinitions::empty();
    fonts.font_data.insert(
        "opterm-emoji".into(),
        FontData::from_static(include_bytes!("../assets/fonts/NotoEmoji-wght.ttf")).into(),
    );
    fonts
        .families
        .insert(FontFamily::Monospace, vec!["opterm-emoji".into()]);
    let ctx = Context::default();
    ctx.set_fonts(fonts);
    let _ = ctx.run(RawInput::default(), |ctx| {
        ctx.fonts_mut(|fonts| {
            let font = FontId::monospace(18.0);
            let text = "🙂🚀🧑";
            assert!(fonts.has_glyphs(&font, text));
            let galley = fonts.layout_no_wrap(text.into(), font, Color32::WHITE);
            assert_eq!(galley.rows[0].glyphs.len(), 3);
            assert!(
                galley.rows[0]
                    .glyphs
                    .iter()
                    .all(|glyph| glyph.uv_rect.size.x > 0.0 && glyph.uv_rect.size.y > 0.0)
            );
        });
    });
}
