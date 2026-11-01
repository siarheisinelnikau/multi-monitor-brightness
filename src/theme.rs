//! Coherent application colors for both themes, independent of the renderer.
use egui::{Color32, FontId, Stroke, Vec2};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    #[default]
    Dark,
}

pub struct Palette {
    pub bg: Color32,
    pub card: Color32,
    pub border: Color32,
    pub accent: Color32,
    pub track: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub warm: Color32,
    pub warning_bg: Color32,
}

impl Theme {
    pub fn palette(self) -> Palette {
        if self == Self::Dark {
            Palette {
                bg: Color32::from_rgb(23, 24, 27),
                card: Color32::from_rgb(32, 33, 38),
                border: Color32::from_rgb(51, 53, 60),
                accent: Color32::from_rgb(137, 167, 237),
                track: Color32::from_rgb(66, 68, 77),
                text: Color32::from_rgb(239, 239, 242),
                muted: Color32::from_rgb(162, 164, 175),
                warm: Color32::from_rgb(224, 181, 132),
                warning_bg: Color32::from_rgb(45, 39, 34),
            }
        } else {
            Palette {
                bg: Color32::from_rgb(245, 246, 249),
                card: Color32::WHITE,
                border: Color32::from_rgb(207, 212, 223),
                accent: Color32::from_rgb(43, 82, 162),
                track: Color32::from_rgb(204, 211, 225),
                text: Color32::from_rgb(28, 34, 46),
                muted: Color32::from_rgb(82, 93, 112),
                warm: Color32::from_rgb(133, 77, 19),
                warning_bg: Color32::from_rgb(255, 243, 222),
            }
        }
    }

    pub fn apply(self, ctx: &egui::Context) {
        ctx.set_theme(if self == Self::Dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        });
    }
}

pub fn initialize(ctx: &egui::Context, selected: Theme) {
    // Use the installed UI font so Cyrillic and Western European text share metrics.
    if let Some(windows) = std::env::var_os("WINDIR")
        && let Ok(bytes) =
            std::fs::read(std::path::PathBuf::from(windows).join("Fonts/segoeui.ttf"))
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
    for theme in [Theme::Light, Theme::Dark] {
        let p = theme.palette();
        let egui_theme = if theme == Theme::Dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        };
        let mut style = (*ctx.style_of(egui_theme)).clone();
        style.visuals = if theme == Theme::Dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        style.visuals.override_text_color = Some(p.text);
        style.visuals.panel_fill = p.bg;
        style.visuals.window_fill = p.card;
        style.visuals.extreme_bg_color = p.bg;
        style.visuals.faint_bg_color = p.card;
        style.visuals.hyperlink_color = p.accent;
        style.visuals.warn_fg_color = p.warm;
        style.visuals.selection.bg_fill = if theme == Theme::Dark {
            Color32::from_rgb(51, 66, 96)
        } else {
            Color32::from_rgb(215, 229, 255)
        };
        style.visuals.selection.stroke = Stroke::new(1.0, p.accent);
        style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
        style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
        for widgets in [
            &mut style.visuals.widgets.inactive,
            &mut style.visuals.widgets.hovered,
            &mut style.visuals.widgets.active,
        ] {
            widgets.corner_radius = 5.into();
            widgets.bg_stroke = Stroke::new(1.0, p.border);
            widgets.fg_stroke = Stroke::new(1.0, p.text);
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
        ctx.set_style_of(egui_theme, style);
    }
    selected.apply(ctx);
}

pub fn palette(ui: &egui::Ui) -> Palette {
    if ui.visuals().dark_mode {
        Theme::Dark
    } else {
        Theme::Light
    }
    .palette()
}

pub fn card(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    let p = palette(ui);
    egui::Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(9)
        .inner_margin(16)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            contents(ui);
        });
}
