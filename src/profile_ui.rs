use super::*;
use std::collections::BTreeMap;

impl MultiMonitorBrightness {
    fn connected(&self) -> Vec<profiles::Connected<'_>> {
        self.displays
            .iter()
            .map(|d| profiles::Connected {
                id: &d.info.id,
                supported: d.info.brightness.is_some(),
            })
            .collect()
    }

    pub(super) fn can_capture_profile(&self) -> bool {
        !self.save_blocked
            && self.quitting.is_none()
            && !self.editor_changed()
            && self.displays.iter().any(|d| d.info.brightness.is_some())
            && self
                .displays
                .iter()
                .all(|d| d.queued.is_none() && !d.writing && d.error.is_none())
            && self.displays.iter().all(|d| {
                self.displays
                    .iter()
                    .filter(|other| other.info.id == d.info.id)
                    .count()
                    == 1
            })
    }

    pub(super) fn save_current_profile(&mut self, name: &str, preset: Option<u8>) {
        if !self.can_capture_profile() {
            self.profile_error = Some("Wait for brightness changes to finish, then save again.");
            return;
        }
        let name = match profiles::validate_name(name, &self.settings.profiles, None) {
            Ok(name) => name,
            Err(error) => {
                self.profile_error = Some(error);
                return;
            }
        };
        let profile = profiles::Profile {
            id: profiles::next_id(&self.settings.profiles),
            name,
            monitors: self
                .displays
                .iter()
                .filter(|d| d.info.brightness.is_some())
                .map(|d| {
                    (
                        d.info.id.clone(),
                        profiles::Level {
                            name: d.info.name.clone(),
                            brightness: preset.unwrap_or(d.confirmed),
                            included: self
                                .settings
                                .monitors
                                .get(&d.info.id)
                                .is_none_or(|m| m.included),
                        },
                    )
                })
                .collect(),
        };
        self.selected_profile = Some(profile.id);
        self.profile_editor = Some(profile.clone());
        self.settings.profiles.push(profile);
        self.profile_name.clear();
        self.profile_error = None;
        self.profile_feedback = Some("Profile saved. Brightness has not changed.");
        self.profile_delete_pending = None;
        self.dirty();
    }

    fn editor_changed(&self) -> bool {
        self.profile_editor.as_ref().is_some_and(|draft| {
            self.settings.profiles.iter().find(|p| p.id == draft.id) != Some(draft)
        })
    }

    fn select_profile(&mut self, id: u64) {
        if self.editor_changed() {
            self.profile_error = Some("Save or discard your edits first.");
            return;
        }
        self.selected_profile = Some(id);
        self.profile_editor = self.settings.profiles.iter().find(|p| p.id == id).cloned();
        self.profile_error = None;
        self.profile_feedback = None;
        self.profile_delete_pending = None;
    }

    pub(super) fn save_profile_edits(&mut self, rename_only: bool) {
        let Some(mut draft) = self.profile_editor.clone() else {
            return;
        };
        draft.name =
            match profiles::validate_name(&draft.name, &self.settings.profiles, Some(draft.id)) {
                Ok(name) => name,
                Err(error) => {
                    self.profile_error = Some(error);
                    return;
                }
            };
        if let Some(saved) = self.settings.profiles.iter_mut().find(|p| p.id == draft.id) {
            if rename_only {
                saved.name.clone_from(&draft.name);
            } else {
                *saved = draft.clone();
            }
            self.profile_editor = Some(draft);
            self.profile_error = None;
            self.profile_feedback = Some("Profile saved. Brightness has not changed.");
            self.dirty();
        }
    }

    pub(super) fn apply_profile_at(&mut self, id: u64, day: i64, minute: u16) {
        if self.quitting.is_some() {
            return;
        }
        let Some(profile) = self.settings.profiles.iter().find(|p| p.id == id).cloned() else {
            return;
        };
        let connected = self.connected();
        let preview = profile.preview(&connected);
        if preview.count(Availability::Ready) == 0 {
            self.profile_error = Some("No matching displays are ready. Connect them and refresh.");
            return;
        }
        let rows: Vec<_> = preview
            .rows
            .iter()
            .map(|(id, level, availability)| ((*id).to_owned(), (*level).clone(), *availability))
            .collect();
        // Applying is a manual action, including when the levels already match. Pause
        // the current evening so the schedule cannot immediately overwrite the choice.
        if let Some((cycle, _)) = self.settings.schedule.phase(day, minute) {
            self.paused_cycle = Some(cycle);
        }
        self.profile_application = None;
        let mut outcomes = BTreeMap::new();
        for (id, level, availability) in rows {
            let outcome = match availability {
                Availability::Missing => Outcome::Missing,
                Availability::Unsupported => Outcome::Unsupported,
                Availability::Ambiguous => Outcome::Ambiguous,
                Availability::Ready => {
                    let index = self.displays.iter().position(|d| d.info.id == id).unwrap();
                    self.queue(index, level.brightness, true);
                    let display = &mut self.displays[index];
                    if display.queued.is_some() || display.writing {
                        // An existing in-flight schedule write with the same target still
                        // needs manual intent recorded for its eventual successful result.
                        display.manual.insert(display.revision);
                        Outcome::Pending {
                            revision: display.revision,
                        }
                    } else {
                        let memory = self.settings.monitors.entry(id.clone()).or_default();
                        memory.brightness = level.brightness;
                        memory.included = level.included;
                        Outcome::Applied
                    }
                }
            };
            outcomes.insert(id, (level, outcome));
        }
        self.profile_application = Some(profiles::Application {
            name: profile.name,
            outcomes,
        });
        self.profile_error = None;
        self.profile_feedback = None;
        self.dirty();
    }

    fn apply_selected_profile(&mut self) {
        if let Some(id) = self.selected_profile {
            let (day, minute) = platform::local_clock();
            self.apply_profile_at(id, day, minute);
        }
    }

    pub(super) fn profile_written(&mut self, id: &str, revision: u64, success: bool) {
        if let Some(application) = &mut self.profile_application
            && let Some((level, outcome)) = application.outcomes.get_mut(id)
            && *outcome == (Outcome::Pending { revision })
        {
            *outcome = if success {
                Outcome::Applied
            } else {
                Outcome::Failed
            };
            if success {
                self.settings
                    .monitors
                    .entry(id.into())
                    .or_default()
                    .included = level.included;
                self.dirty();
            }
        }
    }

    pub(super) fn quick_profiles_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(tr(ui, "Profiles"))
                    .size(12.0)
                    .color(palette(ui).muted),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button(tr(ui, "Manage")).clicked() {
                    self.open_preferences(Tab::Profiles, ui.ctx());
                }
            });
        });
        if self.settings.profiles.is_empty() {
            if ui.button(tr(ui, "Save a brightness profile")).clicked() {
                self.open_preferences(Tab::Profiles, ui.ctx());
            }
        } else {
            let selected = self
                .settings
                .profiles
                .iter()
                .find(|p| Some(p.id) == self.selected_profile);
            let name =
                selected.map_or_else(|| tr(ui, "Choose a profile").to_owned(), |p| p.name.clone());
            let ready = !self.editor_changed()
                && selected
                    .is_some_and(|p| p.preview(&self.connected()).count(Availability::Ready) > 0);
            let mut selection = self.selected_profile;
            ui.horizontal(|ui| {
                let combo_width = (ui.available_width() - 104.0).max(80.0);
                ui.allocate_ui_with_layout(
                    Vec2::new(combo_width, 25.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_max_width(combo_width);
                        if self.editor_changed() {
                            ui.disable();
                        }
                        egui::ComboBox::from_id_salt("quick-profile")
                            .width(combo_width)
                            .selected_text(name)
                            .truncate()
                            .show_ui(ui, |ui| {
                                for profile in &self.settings.profiles {
                                    ui.selectable_value(
                                        &mut selection,
                                        Some(profile.id),
                                        &profile.name,
                                    );
                                }
                            });
                    },
                );
                if ui
                    .add_enabled(ready, egui::Button::new(tr(ui, "Apply")))
                    .clicked()
                {
                    self.apply_selected_profile();
                }
            });
            if selection != self.selected_profile {
                self.selected_profile = selection;
                self.profile_editor = None;
                self.profile_error = None;
            }
            if let Some(profile) = self
                .settings
                .profiles
                .iter()
                .find(|p| Some(p.id) == self.selected_profile)
            {
                let preview = profile.preview(&self.connected());
                let ready = preview.count(Availability::Ready);
                let skipped = preview.rows.len() - ready;
                ui.label(RichText::new(i18n::format(ui, "{ready} ready · {skipped} skipped", &[
                    ("ready", ready.to_string()), ("skipped", skipped.to_string())
                ])).size(11.0).color(palette(ui).muted))
                    .on_hover_text(tr(ui, "Only matching connected displays will change. Other displays stay as they are."));
            }
        }
        if let Some(error) = self.profile_error {
            ui.label(
                RichText::new(tr(ui, error))
                    .size(11.0)
                    .color(palette(ui).warm),
            );
        }
        self.profile_result_ui(ui);
        ui.add_space(6.0);
        ui.separator();
        ui.add_space(8.0);
    }

    fn profile_result_ui(&self, ui: &mut egui::Ui) {
        if let Some(application) = &self.profile_application {
            let pending = application.pending();
            ui.label(
                RichText::new(i18n::format(
                    ui,
                    if pending > 0 {
                        "{name}: applying…"
                    } else {
                        "{name}: {applied} applied · {skipped} skipped or failed"
                    },
                    &[
                        ("name", application.name.clone()),
                        ("applied", application.applied().to_string()),
                        ("skipped", application.skipped().to_string()),
                    ],
                ))
                .size(11.0)
                .color(if application.skipped() > 0 {
                    palette(ui).warm
                } else {
                    palette(ui).accent
                }),
            );
            let progress = (application.outcomes.len() - pending) as f32
                / application.outcomes.len().max(1) as f32;
            let animated = crate::motion::value(
                ui,
                ui.scope_id().with("profile-progress"),
                progress,
                self.settings.reduced_motion || !ui.is_rect_visible(ui.clip_rect()),
            );
            ui.add(
                egui::ProgressBar::new(animated)
                    .desired_height(3.0)
                    .animate(false),
            );
        }
    }

    pub(super) fn profiles_ui(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new(tr(ui, "Brightness profiles"))
                .size(20.0)
                .strong(),
        );
        ui.label(RichText::new(tr(ui, "Keep a brightness setup for work, reading or evenings. Choose a profile, preview it, then apply."))
            .size(12.0).color(palette(ui).muted));
        ui.add_space(16.0);
        card(ui, |ui| {
            ui.label(RichText::new(tr(ui, "Save current")).size(16.0).strong());
            ui.label(
                RichText::new(tr(
                    ui,
                    "Saves confirmed levels and linked displays. Saving never changes brightness.",
                ))
                .size(11.0)
                .color(palette(ui).muted),
            );
            ui.horizontal_wrapped(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.profile_name)
                        .hint_text(tr(ui, "New profile name"))
                        .char_limit(profiles::MAX_NAME_CHARS)
                        .desired_width((ui.available_width() - 120.0).max(100.0)),
                );
                if ui
                    .add_enabled(
                        self.can_capture_profile()
                            && self.settings.profiles.len() < profiles::MAX_PROFILES,
                        egui::Button::new(tr(ui, "Save current")),
                    )
                    .on_hover_text(tr(
                        ui,
                        "Wait for brightness changes to finish before saving.",
                    ))
                    .clicked()
                {
                    self.save_current_profile(&self.profile_name.clone(), None);
                }
            });
            if self.settings.profiles.is_empty() {
                ui.add_space(8.0);
                ui.label(tr(
                    ui,
                    "Or create a starting point. These buttons save a profile without applying it.",
                ));
                ui.horizontal_wrapped(|ui| {
                    for (name, value) in [("Work", 75), ("Relax", 45), ("Night", 20)] {
                        if ui
                            .add_enabled(
                                self.can_capture_profile(),
                                egui::Button::new(format!("{} · {value}%", tr(ui, name))),
                            )
                            .clicked()
                        {
                            self.save_current_profile(tr(ui, name), Some(value));
                        }
                    }
                });
            }
        });
        if let Some(error) = self.profile_error {
            ui.label(RichText::new(tr(ui, error)).color(palette(ui).warm));
        }
        if let Some(feedback) = self.profile_feedback {
            ui.label(RichText::new(tr(ui, feedback)).color(palette(ui).accent));
        }
        ui.add_space(12.0);
        let mut select = None;
        ui.horizontal_wrapped(|ui| {
            for profile in &self.settings.profiles {
                let active = Some(profile.id) == self.selected_profile;
                let fade = crate::motion::value(
                    ui,
                    ui.scope_id().with(("profile-selection", profile.id)),
                    if active { 1.0 } else { 0.0 },
                    self.settings.reduced_motion,
                );
                let fill = palette(ui).card.lerp_to_gamma(palette(ui).track, fade);
                if ui
                    .add(
                        egui::Button::new(&profile.name)
                            .truncate()
                            .fill(fill)
                            .stroke(Stroke::new(
                                1.0,
                                if active {
                                    palette(ui).accent
                                } else {
                                    palette(ui).border
                                },
                            )),
                    )
                    .clicked()
                {
                    select = Some(profile.id);
                }
            }
        });
        if let Some(id) = select {
            self.select_profile(id);
        }
        if self.profile_editor.is_none() {
            self.profile_editor = self
                .settings
                .profiles
                .iter()
                .find(|p| Some(p.id) == self.selected_profile)
                .cloned();
        }
        if let Some(mut draft) = self.profile_editor.take() {
            ui.add_space(12.0);
            card(ui, |ui| {
                ui.label(
                    RichText::new(tr(ui, "Preview and edit"))
                        .size(16.0)
                        .strong(),
                );
                ui.horizontal(|ui| {
                    ui.label(tr(ui, "Profile name"));
                    ui.add(
                        egui::TextEdit::singleline(&mut draft.name)
                            .char_limit(profiles::MAX_NAME_CHARS)
                            .desired_width((ui.available_width() - 100.0).max(80.0)),
                    );
                });
                ui.add_space(10.0);
                let connected = self.connected();
                let preview = draft.preview(&connected);
                let statuses: BTreeMap<String, _> = preview
                    .rows
                    .iter()
                    .map(|(id, _, status)| ((*id).into(), *status))
                    .collect();
                let untouched = preview.untouched;
                let name_width = (ui.available_width() - 160.0).max(100.0);
                egui::Grid::new("profile-levels")
                    .num_columns(3)
                    .spacing([12.0, 10.0])
                    .show(ui, |ui| {
                        for (id, level) in &mut draft.monitors {
                            ui.push_id(id, |ui| {
                                ui.vertical(|ui| {
                                    ui.set_width(name_width);
                                    ui.add(egui::Label::new(&level.name).truncate());
                                    let label = match statuses[id.as_str()] {
                                        Availability::Ready => "Ready",
                                        Availability::Missing => "Disconnected · skipped",
                                        Availability::Unsupported => "Unavailable · skipped",
                                        Availability::Ambiguous => "Ambiguous identity · skipped",
                                    };
                                    ui.label(
                                        RichText::new(tr(ui, label))
                                            .size(10.0)
                                            .color(palette(ui).muted),
                                    );
                                });
                            });
                            ui.add(
                                egui::DragValue::new(&mut level.brightness)
                                    .range(0..=100)
                                    .suffix("%"),
                            );
                            ui.checkbox(&mut level.included, tr(ui, "Link"));
                            ui.end_row();
                        }
                    });
                if untouched > 0 {
                    ui.label(
                        RichText::new(i18n::format(
                            ui,
                            "{count} other displays will stay unchanged.",
                            &[("count", untouched.to_string())],
                        ))
                        .size(11.0)
                        .color(palette(ui).muted),
                    );
                }
                ui.add_space(10.0);
                ui.label(RichText::new(tr(ui, "Applying pauses the current evening schedule. Missing displays are skipped; apply again after reconnecting."))
                    .size(11.0).color(palette(ui).muted));
            });
            self.profile_editor = Some(draft);
            let changed = self.editor_changed();
            let name_changed = self.profile_editor.as_ref().is_some_and(|draft| {
                self.settings
                    .profiles
                    .iter()
                    .find(|p| p.id == draft.id)
                    .is_some_and(|saved| saved.name != draft.name)
            });
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        !self.save_blocked && changed,
                        egui::Button::new(tr(ui, "Save changes")),
                    )
                    .clicked()
                {
                    self.save_profile_edits(false);
                }
                if ui
                    .add_enabled(
                        !self.save_blocked && name_changed,
                        egui::Button::new(tr(ui, "Rename")),
                    )
                    .clicked()
                {
                    self.save_profile_edits(true);
                }
                if ui
                    .add_enabled(changed, egui::Button::new(tr(ui, "Discard edits")))
                    .clicked()
                {
                    self.profile_editor = self
                        .settings
                        .profiles
                        .iter()
                        .find(|p| Some(p.id) == self.selected_profile)
                        .cloned();
                    self.profile_error = None;
                }
                let ready = self
                    .settings
                    .profiles
                    .iter()
                    .find(|p| Some(p.id) == self.selected_profile)
                    .is_some_and(|p| p.preview(&self.connected()).count(Availability::Ready) > 0);
                if ui
                    .add_enabled(
                        !changed && ready,
                        egui::Button::new(tr(ui, "Apply")).fill(palette(ui).track),
                    )
                    .clicked()
                {
                    self.apply_selected_profile();
                }
                if ui
                    .add_enabled(!self.save_blocked, egui::Button::new(tr(ui, "Delete")))
                    .clicked()
                {
                    self.profile_delete_pending = self.selected_profile;
                }
            });
            if let Some(id) = self.profile_delete_pending {
                ui.horizontal_wrapped(|ui| {
                    ui.label(tr(
                        ui,
                        "Delete this saved profile? Monitor brightness will stay unchanged.",
                    ));
                    if ui.button(tr(ui, "Delete profile")).clicked() {
                        self.settings.profiles.retain(|p| p.id != id);
                        self.selected_profile = None;
                        self.profile_editor = None;
                        self.profile_delete_pending = None;
                        self.profile_feedback = None;
                        self.profile_error = None;
                        self.dirty();
                    }
                    if ui.button(tr(ui, "Cancel")).clicked() {
                        self.profile_delete_pending = None;
                    }
                });
            }
        }
        ui.add_space(12.0);
        self.profile_result_ui(ui);
        if let Some(application) = &self.profile_application {
            for (level, outcome) in application.outcomes.values() {
                let label = match outcome {
                    Outcome::Pending { .. } => "Applying…",
                    Outcome::Applied => "Applied",
                    Outcome::Failed => "Failed · previous level kept",
                    Outcome::Missing => "Disconnected · skipped",
                    Outcome::Unsupported => "Unavailable · skipped",
                    Outcome::Ambiguous => "Ambiguous identity · skipped",
                    Outcome::Superseded => "Changed manually · profile cancelled",
                };
                ui.label(format!("{} · {}", level.name, tr(ui, label)));
            }
        }
    }
}
