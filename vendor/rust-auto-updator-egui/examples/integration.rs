//! Compiling integration skeleton. The host supplies its window and event loop.
use rust_auto_updator::{Config, Controller, Prepared};
use rust_auto_updator_egui::{Language, style::Theme};

struct Updates {
    controller: Controller,
    check_on_startup: bool,
    pending: Option<Prepared>,
    settings_dirty: bool,
    language: Language,
    theme: Theme,
}

impl Updates {
    fn new(
        ctx: egui::Context,
        saved_preference: Option<bool>,
        saved_language: Option<&str>,
        saved_theme: Option<&str>,
    ) -> Result<Self, String> {
        let language = Language::from_code(saved_language.unwrap_or("en"));
        let theme = Theme::from_code(saved_theme.unwrap_or("dark"));
        rust_auto_updator_egui::style::apply_theme(&ctx, theme);
        let mut config = Config::new(
            "your-account",
            "your-repository",
            env!("CARGO_PKG_VERSION"),
            "YourProduct",
        )?;
        config.install.restart_args = vec!["--updated".into()];
        Ok(Self {
            controller: rust_auto_updator_egui::start(config, ctx)?,
            check_on_startup: saved_preference.unwrap_or(true),
            pending: None,
            settings_dirty: false,
            language,
            theme,
        })
    }

    fn logic(&mut self) {
        let event = self.controller.poll(self.check_on_startup);
        if event.ready_to_install {
            self.pending = self.controller.take_prepared();
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        let old_language = self.language;
        let old_theme = self.theme;
        ui.horizontal(|ui| {
            ui.label(self.language.text().language);
            egui::ComboBox::from_id_salt("interface-language")
                .selected_text(self.language.native_name())
                .show_ui(ui, |ui| {
                    for language in Language::ALL {
                        ui.selectable_value(&mut self.language, language, language.native_name());
                    }
                });
            ui.label(self.language.text().theme);
            egui::ComboBox::from_id_salt("interface-theme")
                .selected_text(self.theme.label(self.language))
                .show_ui(ui, |ui| {
                    for theme in Theme::ALL {
                        ui.selectable_value(&mut self.theme, theme, theme.label(self.language));
                    }
                });
        });
        if self.theme != old_theme {
            rust_auto_updator_egui::style::apply_theme(ui.ctx(), self.theme);
        }
        if self.language != old_language || self.theme != old_theme {
            // Persist language.code() and theme.code() in the host's settings.
            self.settings_dirty = true;
        }
        if rust_auto_updator_egui::show_localized(
            ui,
            &mut self.controller,
            &mut self.check_on_startup,
            self.language,
        )
        .preference_changed
        {
            self.settings_dirty = true;
        }
    }

    /// Call only after saving user data and finishing pending host operations.
    /// Return true to let the host exit; never exit on a failed handoff.
    fn handoff_after_saving(&mut self) -> bool {
        let Some(mut prepared) = self.pending.take() else {
            return false;
        };
        match rust_auto_updator::handoff(&mut prepared) {
            Ok(()) => true,
            Err(error) => {
                self.controller.installation_failed(error);
                false
            }
        }
    }
}

fn main() {
    // Check the integration signatures without creating a UI or making requests.
    let _ = Updates::new;
    let _ = Updates::logic;
    let _ = Updates::ui;
    let _ = Updates::handoff_after_saving;
    println!(
        "Integrate Updates into the host event loop; see the workspace README for CLI routing."
    );
}
