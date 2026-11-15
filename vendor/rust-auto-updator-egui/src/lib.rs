//! Drop-in egui update panel. Rendering/backend and persistent settings stay with
//! the application. `style` is optional and never applied by `show` or `start`.
pub mod localization;
pub mod style;

pub use localization::{Language, PanelText};

use egui::RichText;
use rust_auto_updator::{Config, Controller, State};

/// Connect worker events to egui's existing event loop, without polling frames.
pub fn start(config: Config, ctx: egui::Context) -> Result<Controller, String> {
    Controller::new(config, move || {
        ctx.request_repaint_of(egui::ViewportId::ROOT)
    })
}

#[derive(Default)]
pub struct Response {
    pub preference_changed: bool,
}

/// Draw using the host's current egui theme. The host persists `check_on_startup`
/// when the returned response reports a change. Call `Controller::poll` in the
/// host logic loop, including while this panel is closed.
pub fn show(ui: &mut egui::Ui, updates: &mut Controller, check_on_startup: &mut bool) -> Response {
    show_localized(ui, updates, check_on_startup, Language::English)
}

/// Draw with one of the built-in languages, without changing the host theme.
/// The host persists the language code and requests a frame when it changes.
pub fn show_localized(
    ui: &mut egui::Ui,
    updates: &mut Controller,
    check_on_startup: &mut bool,
    language: Language,
) -> Response {
    show_with_text(ui, updates, check_on_startup, language.text())
}

/// Draw with host-supplied translations. Technical errors and release notes are
/// displayed verbatim; translate product-specific disabled reasons in the host.
pub fn show_with_text(
    ui: &mut egui::Ui,
    updates: &mut Controller,
    check_on_startup: &mut bool,
    text: &PanelText<'_>,
) -> Response {
    let mut response = Response::default();
    egui::Frame::new()
        .fill(ui.visuals().window_fill)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(9)
        .inner_margin(16)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(text.heading).size(17.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            updates.enabled() && !updates.state().busy(),
                            egui::Button::new(text.check),
                        )
                        .clicked()
                    {
                        updates.check();
                    }
                });
            });
            ui.label(
                RichText::new(
                    text.installed_version
                        .replace("{version}", updates.installed_version()),
                )
                .size(11.0)
                .color(ui.visuals().weak_text_color()),
            );
            ui.add_space(8.0);
            let muted = ui.visuals().weak_text_color();
            match updates.state() {
                State::Idle => {
                    if let Some(reason) = updates.disabled_reason() {
                        ui.label(RichText::new(text.disabled).size(12.0).color(muted));
                        ui.label(RichText::new(reason).size(12.0).color(muted));
                    } else {
                        ui.label(RichText::new(text.source).size(12.0).color(muted));
                    }
                }
                State::Checking => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(text.checking);
                    });
                }
                State::Current => {
                    ui.label(RichText::new(text.current).size(13.0).color(muted));
                }
                State::Available(release) => {
                    let version = release.version.to_string();
                    let page = release.page.clone();
                    let notes = release.notes.clone();
                    ui.label(text.available_version.replace("{version}", &version));
                    ui.horizontal(|ui| {
                        if ui.button(text.download_install).clicked() {
                            updates.download_available();
                        }
                        ui.hyperlink_to(text.release_notes, page);
                    });
                    ui.label(RichText::new(text.restart_notice).size(11.0).color(muted));
                    if !notes.is_empty() {
                        ui.collapsing(text.whats_new, |ui| {
                            ui.label(notes);
                        });
                    }
                }
                State::Downloading { received, total } => {
                    let progress = if *total > 0 {
                        *received as f32 / *total as f32
                    } else {
                        0.0
                    };
                    ui.add(
                        egui::ProgressBar::new(progress)
                            .show_percentage()
                            .animate(true),
                    );
                    ui.label(RichText::new(text.downloading).size(12.0).color(muted));
                }
                State::Installing => {
                    ui.label(text.installing);
                }
                State::Error(error) => {
                    ui.label(RichText::new(text.error).color(ui.visuals().warn_fg_color));
                    ui.label(
                        RichText::new(error)
                            .size(12.0)
                            .color(ui.visuals().warn_fg_color),
                    );
                }
            }
            ui.add_space(8.0);
            response.preference_changed = ui
                .add_enabled(
                    updates.enabled(),
                    egui::Checkbox::new(check_on_startup, text.startup),
                )
                .changed();
        });
    response
}
