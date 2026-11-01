//! Product-local translations around the shared updater state machine.
use crate::{
    i18n::{self, tr},
    theme::{card, palette},
};
use egui::RichText;
use rust_auto_updator::{Controller, State};

pub fn show(ui: &mut egui::Ui, updates: &mut Controller, check_on_startup: &mut bool) -> bool {
    let mut changed = false;
    card(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(tr(ui, "Updates")).size(17.0).strong());
            if ui
                .add_enabled(
                    updates.enabled() && !updates.state().busy(),
                    egui::Button::new(tr(ui, "Check for updates")),
                )
                .clicked()
            {
                updates.check();
            }
        });
        ui.label(
            RichText::new(i18n::format(
                ui,
                "Installed version {version}",
                &[("version", updates.installed_version().into())],
            ))
            .size(11.0)
            .color(palette(ui).muted),
        );
        ui.add_space(8.0);
        match updates.state() {
            State::Idle => {
                ui.label(
                    RichText::new(tr(
                        ui,
                        updates
                            .disabled_reason()
                            .unwrap_or("Updates are downloaded from the official GitHub releases."),
                    ))
                    .size(12.0)
                    .color(palette(ui).muted),
                );
            }
            State::Checking => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tr(ui, "Checking GitHub…"));
                });
            }
            State::Current => {
                ui.label(RichText::new(tr(ui, "You're up to date.")).color(palette(ui).muted));
            }
            State::Available(release) => {
                let version = release.version.to_string();
                let page = release.page.clone();
                let notes = release.notes.clone();
                ui.label(i18n::format(
                    ui,
                    "Version {version} is available",
                    &[("version", version)],
                ));
                ui.horizontal_wrapped(|ui| {
                    if ui.button(tr(ui, "Download and install")).clicked() {
                        updates.download_available();
                    }
                    ui.hyperlink_to(tr(ui, "Release notes on GitHub"), page);
                });
                ui.label(
                    RichText::new(tr(ui, "The app will restart. Your settings will be kept."))
                        .size(11.0)
                        .color(palette(ui).muted),
                );
                if !notes.is_empty() {
                    ui.collapsing(tr(ui, "What's new"), |ui| {
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
                ui.label(tr(ui, "Downloading and verifying SHA-256…"));
            }
            State::Installing => {
                ui.label(tr(ui, "Restarting to finish the update…"));
            }
            State::Error(error) => {
                ui.label(
                    RichText::new(tr(ui, "Update failed. Try checking again."))
                        .color(palette(ui).warm),
                );
                ui.collapsing(tr(ui, "Details"), |ui| {
                    ui.label(error);
                });
            }
        }
        ui.add_space(8.0);
        changed = ui
            .add_enabled(
                updates.enabled(),
                egui::Checkbox::new(check_on_startup, tr(ui, "Check for updates on startup")),
            )
            .changed();
    });
    changed
}
