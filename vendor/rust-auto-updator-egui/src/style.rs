//! Optional application-wide theme, tokens and shared containers.
//! The update panel never changes the host context theme.
use egui::{Color32, FontId, Stroke, Vec2};
use std::path::PathBuf;

pub const BG: Color32 = Color32::from_rgb(23, 24, 27);
pub const CARD: Color32 = Color32::from_rgb(32, 33, 38);
pub const BORDER: Color32 = Color32::from_rgb(51, 53, 60);
pub const ACCENT: Color32 = Color32::from_rgb(137, 167, 237);
pub const TRACK: Color32 = Color32::from_rgb(66, 68, 77);
pub const TEXT: Color32 = Color32::from_rgb(239, 239, 242);
pub const MUTED: Color32 = Color32::from_rgb(177, 180, 193);
pub const WARM: Color32 = Color32::from_rgb(224, 181, 132);

/// Optional application theme. The host owns the persisted preference.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Theme {
    Light,
    #[default]
    Dark,
}

impl Theme {
    pub const ALL: [Self; 2] = [Self::Light, Self::Dark];

    pub const fn code(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    pub fn from_code(code: &str) -> Self {
        if code.eq_ignore_ascii_case("light") {
            Self::Light
        } else {
            Self::Dark
        }
    }

    pub const fn label(self, language: crate::Language) -> &'static str {
        match self {
            Self::Light => language.text().light,
            Self::Dark => language.text().dark,
        }
    }

    const fn egui_theme(self) -> egui::Theme {
        match self {
            Self::Light => egui::Theme::Light,
            Self::Dark => egui::Theme::Dark,
        }
    }
}

/// Semantic color roles for applications opting into the shared theme.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub background: Color32,
    pub surface: Color32,
    pub border: Color32,
    pub accent: Color32,
    pub track: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub warning: Color32,
    pub selection: Color32,
    pub control: Color32,
    pub hover: Color32,
    pub active: Color32,
}

pub const fn palette(theme: Theme) -> Palette {
    match theme {
        Theme::Dark => Palette {
            background: BG,
            surface: CARD,
            border: BORDER,
            accent: ACCENT,
            track: TRACK,
            text: TEXT,
            muted: MUTED,
            warning: WARM,
            selection: Color32::from_rgb(51, 66, 96),
            control: Color32::from_rgb(43, 44, 51),
            hover: Color32::from_rgb(53, 55, 64),
            active: Color32::from_rgb(58, 66, 88),
        },
        Theme::Light => Palette {
            background: Color32::from_rgb(245, 246, 249),
            surface: Color32::WHITE,
            border: Color32::from_rgb(205, 210, 221),
            accent: Color32::from_rgb(40, 80, 158),
            track: Color32::from_rgb(208, 214, 225),
            text: Color32::from_rgb(30, 34, 43),
            muted: Color32::from_rgb(86, 94, 111),
            warning: Color32::from_rgb(143, 83, 20),
            selection: Color32::from_rgb(216, 228, 252),
            control: Color32::from_rgb(235, 238, 244),
            hover: Color32::from_rgb(225, 231, 242),
            active: Color32::from_rgb(210, 223, 247),
        },
    }
}

/// Install either shared theme. Call at initialization or preference change,
/// rather than on every frame. This makes no repaint timer or settings file.
pub fn apply_theme(ctx: &egui::Context, theme: Theme) {
    let colors = palette(theme);
    // Use the installed Windows UI font without bundling a proprietary font file.
    if let Some(windows) = std::env::var_os("WINDIR")
        && let Ok(bytes) = std::fs::read(PathBuf::from(windows).join("Fonts").join("segoeui.ttf"))
    {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "windows-ui".into(),
            egui::FontData::from_owned(bytes).into(),
        );
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "windows-ui".into());
        ctx.set_fonts(fonts);
    }
    let mut style = (*ctx.style_of(theme.egui_theme())).clone();
    style.visuals = match theme {
        Theme::Light => egui::Visuals::light(),
        Theme::Dark => egui::Visuals::dark(),
    };
    style.visuals.override_text_color = Some(colors.text);
    style.visuals.weak_text_color = Some(colors.muted);
    style.visuals.panel_fill = colors.background;
    style.visuals.window_fill = colors.surface;
    style.visuals.window_stroke = Stroke::new(1.0, colors.border);
    style.visuals.extreme_bg_color = colors.background;
    style.visuals.faint_bg_color = colors.surface;
    style.visuals.hyperlink_color = colors.accent;
    style.visuals.warn_fg_color = colors.warning;
    style.visuals.selection.bg_fill = colors.selection;
    style.visuals.selection.stroke = Stroke::new(1.0, colors.accent);
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, colors.text);
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, colors.border);
    style.visuals.widgets.inactive.bg_fill = colors.control;
    style.visuals.widgets.inactive.weak_bg_fill = colors.control;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, colors.border);
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, colors.muted);
    style.visuals.widgets.hovered.bg_fill = colors.hover;
    style.visuals.widgets.hovered.weak_bg_fill = colors.hover;
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, colors.accent);
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, colors.text);
    style.visuals.widgets.active.bg_fill = colors.active;
    style.visuals.widgets.active.weak_bg_fill = colors.active;
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, colors.accent);
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.2, colors.text);
    for widgets in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
    ] {
        widgets.corner_radius = 5.into();
    }
    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.button_padding = Vec2::new(10.0, 5.0);
    style.spacing.interact_size.y = 25.0;
    style.spacing.icon_width = 15.0;
    style.spacing.scroll = egui::style::ScrollStyle::solid();
    style.spacing.scroll.bar_width = 6.0;
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(13.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(12.0));
    ctx.set_theme(theme.egui_theme());
    ctx.set_style_of(theme.egui_theme(), style);
}

/// Backward-compatible dark theme entry point.
pub fn apply_dark_theme(ctx: &egui::Context) {
    apply_theme(ctx, Theme::Dark);
}

pub fn apply_light_theme(ctx: &egui::Context) {
    apply_theme(ctx, Theme::Light);
}

pub fn card(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(ui.visuals().window_fill)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(9)
        .inner_margin(16)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            contents(ui);
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(color: Color32) -> f64 {
        let channel = |value: u8| {
            let value = f64::from(value) / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
    }

    fn contrast(first: Color32, second: Color32) -> f64 {
        let first = luminance(first);
        let second = luminance(second);
        (first.max(second) + 0.05) / (first.min(second) + 0.05)
    }

    #[test]
    fn both_palettes_keep_body_and_secondary_text_readable() {
        for theme in Theme::ALL {
            let colors = palette(theme);
            for background in [
                colors.background,
                colors.surface,
                colors.control,
                colors.hover,
                colors.active,
            ] {
                assert!(
                    contrast(colors.text, background) >= 4.5,
                    "{theme:?} body contrast"
                );
                assert!(
                    contrast(colors.muted, background) >= 4.5,
                    "{theme:?} secondary contrast"
                );
            }
            assert!(contrast(colors.accent, colors.surface) >= 4.5);
            assert!(contrast(colors.warning, colors.surface) >= 4.5);
        }
    }

    #[test]
    fn switching_themes_updates_actual_egui_visuals_and_keeps_codes_stable() {
        let context = egui::Context::default();
        for theme in [Theme::Light, Theme::Dark, Theme::Light] {
            apply_theme(&context, theme);
            let visuals = context.style_of(theme.egui_theme()).visuals.clone();
            assert_eq!(visuals.dark_mode, theme == Theme::Dark);
            assert_eq!(visuals.panel_fill, palette(theme).background);
            assert_eq!(visuals.window_fill, palette(theme).surface);
            assert_eq!(Theme::from_code(theme.code()), theme);
        }
        assert_eq!(Theme::from_code("unknown"), Theme::Dark);
    }
}
