//! Optional rendering adjustments; themes and PTY contents are never rewritten.
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Readability {
    /// 1.0 disables text contrast adjustment; range 1.0..=7.0.
    pub minimum_contrast: f32,
    pub automatic_cursor_contrast: bool,
    pub cursor_line_guide: bool,
    /// 0.0 disables inactive pane dimming; range 0.0..=0.7.
    pub inactive_pane_dim: f32,
}
impl Default for Readability {
    fn default() -> Self {
        Self {
            minimum_contrast: 1.0,
            automatic_cursor_contrast: false,
            cursor_line_guide: false,
            inactive_pane_dim: 0.0,
        }
    }
}
impl Readability {
    pub fn validate(self) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.minimum_contrast.is_finite() && (1.0..=7.0).contains(&self.minimum_contrast),
            "Minimum text contrast must be between 1 and 7"
        );
        anyhow::ensure!(
            self.inactive_pane_dim.is_finite() && (0.0..=0.7).contains(&self.inactive_pane_dim),
            "Inactive pane dimming must be between 0 and 70 percent"
        );
        Ok(())
    }
}
fn luminance(color: [u8; 3]) -> f32 {
    let linear = color.map(|component| {
        let value = f32::from(component) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    });
    linear[0] * 0.2126 + linear[1] * 0.7152 + linear[2] * 0.0722
}
pub fn contrast_ratio(fg: [u8; 3], bg: [u8; 3]) -> f32 {
    let (a, b) = (luminance(fg), luminance(bg));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}
fn mix(color: [u8; 3], target: [u8; 3], amount: f32) -> [u8; 3] {
    std::array::from_fn(|index| {
        (f32::from(color[index]) * (1.0 - amount) + f32::from(target[index]) * amount).round() as u8
    })
}
/// If the requested ratio is impossible, choose the strongest available contrast.
pub fn ensure_contrast(fg: [u8; 3], bg: [u8; 3], minimum: f32) -> [u8; 3] {
    if !minimum.is_finite() || minimum <= 1.0 || contrast_ratio(fg, bg) >= minimum {
        return fg;
    }
    let black = [0; 3];
    let white = [255; 3];
    let target = if contrast_ratio(black, bg) >= contrast_ratio(white, bg) {
        black
    } else {
        white
    };
    let mut low = 0.0;
    let mut high = 1.0;
    for _ in 0..12 {
        let amount = (low + high) * 0.5;
        if contrast_ratio(mix(fg, target, amount), bg) >= minimum {
            high = amount;
        } else {
            low = amount;
        }
    }
    mix(fg, target, high)
}
pub fn cursor_color(configured: [u8; 3], bg: [u8; 3], automatic: bool) -> [u8; 3] {
    if automatic {
        ensure_contrast(configured, bg, 4.5)
    } else {
        configured
    }
}
pub fn dim(color: [u8; 3], bg: [u8; 3], amount: f32) -> [u8; 3] {
    if !amount.is_finite() || amount <= 0.0 {
        return color;
    }
    mix(color, bg, amount.clamp(0.0, 0.7))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_preserve_theme_colors_and_settings_are_bounded() {
        let settings = Readability::default();
        settings.validate().unwrap();
        assert_eq!(
            ensure_contrast([120, 100, 80], [118, 99, 77], settings.minimum_contrast),
            [120, 100, 80]
        );
        assert_eq!(cursor_color([70; 3], [75; 3], false), [70; 3]);
        assert_eq!(dim([200, 100, 50], [20; 3], 0.0), [200, 100, 50]);
        assert!(
            Readability {
                minimum_contrast: f32::NAN,
                ..settings
            }
            .validate()
            .is_err()
        );
        assert!(
            Readability {
                minimum_contrast: 7.1,
                ..settings
            }
            .validate()
            .is_err()
        );
        assert!(
            Readability {
                inactive_pane_dim: 0.71,
                ..settings
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn contrast_improves_on_light_dark_and_midtones_without_changing_good_colors() {
        assert!((contrast_ratio([255; 3], [0; 3]) - 21.0).abs() < 0.01);
        for background in [[10; 3], [240; 3], [120, 80, 170]] {
            let adjusted = ensure_contrast(background, background, 4.5);
            assert!(contrast_ratio(adjusted, background) >= 4.5);
            assert_eq!(ensure_contrast(adjusted, background, 4.5), adjusted);
            assert!(contrast_ratio(cursor_color(background, background, true), background) >= 4.5);
        }
        assert_eq!(ensure_contrast([255; 3], [0; 3], 4.5), [255; 3]);
        assert_eq!(dim([220; 3], [20; 3], 0.5), [120; 3]);
    }
}
