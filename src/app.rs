use crate::{
    config,
    model::{self, Hotkey, Hotkeys, MonitorMemory, Settings},
    monitors::{self, Command, MonitorInfo},
    platform::{self, Action, HotkeyEvent},
    profiles::{self, Availability, Outcome},
    startup, updater,
};
use egui::{self, Color32, FontId, RichText, Stroke, Vec2};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    time::{Duration, Instant},
};

#[cfg(feature = "tray-smoke-test")]
#[path = "tray_smoke.rs"]
mod tray_smoke;

#[path = "profile_ui.rs"]
mod profile_ui;

use crate::{
    i18n::{self, Language, tr},
    theme::{self, Theme, card, palette},
};

struct Display {
    info: MonitorInfo,
    value: u8,
    confirmed: u8,
    revision: u64,
    queued: Option<Instant>,
    writing: bool,
    manual: BTreeSet<u64>,
    error: Option<String>,
}

#[derive(PartialEq)]
enum Tab {
    Profiles,
    Evening,
    Settings,
}

fn popup_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("brightness-popup")
}

pub struct MultiMonitorBrightness {
    settings: Settings,
    hotkey_draft: Hotkeys,
    config_path: Option<PathBuf>,
    save_blocked: bool,
    dirty: Option<Instant>,
    demo: bool,
    portable: bool,
    startup_status: startup::Status,
    startup_error: Option<String>,
    preferences_open: bool,
    preferences_position_pending: bool,
    popup_open: bool,
    popup_had_focus: bool,
    popup_focus_pending: bool,
    popup_anchor: Option<platform::Anchor>,
    popup_details: bool,
    selected_profile: Option<u64>,
    profile_editor: Option<profiles::Profile>,
    profile_name: String,
    profile_error: Option<&'static str>,
    profile_feedback: Option<&'static str>,
    profile_delete_pending: Option<u64>,
    profile_application: Option<profiles::Application>,
    updates: updater::Controller,
    prepared_update: Option<updater::Prepared>,
    monitors: Option<monitors::Controller>,
    hotkeys: Option<platform::HotkeyService>,
    tray: Option<platform::Tray>,
    displays: Vec<Display>,
    scanning: bool,
    tab: Tab,
    notice: Option<String>,
    shortcut_error: Option<String>,
    shortcuts_pending: bool,
    revision: u64,
    last_schedule: Instant,
    schedule_was_active: bool,
    paused_cycle: Option<i64>,
    quitting: Option<Instant>,
    #[cfg(feature = "tray-smoke-test")]
    smoke: Option<tray_smoke::SmokeTest>,
}

impl MultiMonitorBrightness {
    pub fn new(
        cc: &crate::host::CreationContext,
        demo: bool,
        portable: bool,
        start_in_tray: bool,
        open_settings: bool,
        updated: bool,
    ) -> Self {
        #[cfg(feature = "ui-capture")]
        if demo {
            crate::capture::configure_scale(&cc.egui_ctx);
        }
        let mut notice = None;
        let mut save_blocked = false;
        let config_path = if demo {
            None
        } else {
            match config::path(portable) {
                Ok(path) => Some(path),
                Err(e) => {
                    notice = Some(e.to_string());
                    save_blocked = true;
                    None
                }
            }
        };
        let settings = match config_path.as_ref().map(|p| config::load(p)) {
            Some(Ok(settings)) => settings,
            Some(Err(error)) => {
                notice = Some(error);
                save_blocked = true;
                Settings::default()
            }
            None => Settings::default(),
        };
        #[cfg(feature = "ui-capture")]
        let settings = {
            let mut settings = settings;
            if demo && std::env::var_os("MMB_CAPTURE_TO").is_some() {
                if let Ok(code) = std::env::var("MMB_CAPTURE_LANGUAGE") {
                    settings.language =
                        Language::from_code(&code).expect("supported capture language");
                }
                if let Ok(value) = std::env::var("MMB_CAPTURE_THEME") {
                    settings.theme = match value.as_str() {
                        "light" => Theme::Light,
                        "dark" => Theme::Dark,
                        _ => panic!("unsupported capture theme"),
                    };
                }
            }
            settings
        };
        settings.language.install(&cc.egui_ctx);
        cc.egui_ctx
            .send_viewport_cmd(egui::ViewportCommand::Title(format!(
                "Multi-Monitor Brightness — {}",
                settings.language.text("Preferences")
            )));
        theme::initialize(&cc.egui_ctx, settings.theme);
        theme::motion(&cc.egui_ctx, settings.reduced_motion);
        let monitors = match monitors::Controller::new(cc.egui_ctx.clone(), demo) {
            Ok(service) => Some(service),
            Err(error) => {
                notice = Some(error);
                None
            }
        };
        let hotkeys = if demo {
            None
        } else {
            match platform::HotkeyService::new(cc.egui_ctx.clone()) {
                Ok(service) => {
                    service.configure(&settings.hotkeys);
                    Some(service)
                }
                Err(error) => {
                    notice = Some(error);
                    None
                }
            }
        };
        let tray = match platform::Tray::new(cc.egui_ctx.clone(), settings.language) {
            Ok(tray) => Some(tray),
            Err(error) => {
                notice = Some(format!("System tray unavailable: {error}"));
                None
            }
        };
        if start_in_tray && tray.is_none() {
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::Visible(true));
        }
        let (startup_status, startup_error) = if demo {
            (startup::Status::default(), None)
        } else {
            match startup::status(portable) {
                Ok(status) => (status, None),
                Err(error) => (startup::Status::default(), Some(error)),
            }
        };
        let scanning = monitors.is_some();
        let shortcuts_pending = hotkeys.is_some();
        let preferences_open = open_settings || tray.is_none();
        let popup_open = !start_in_tray && !open_settings && tray.is_some();
        let popup_anchor = tray.as_ref().and_then(|tray| tray.anchor());
        let updates = if demo {
            updater::Controller::disabled(
                crate::VERSION,
                "Update checks are disabled in preview mode.",
            )
        } else {
            match updater::start(cc.egui_ctx.clone(), portable) {
                Ok(service) => service,
                Err(error) => updater::Controller::disabled(crate::VERSION, &error),
            }
        };
        if updated {
            notice = Some(format!("Updated to version {}.", crate::VERSION));
        }
        if popup_open {
            cc.egui_ctx.send_viewport_cmd_to(
                egui::ViewportId::ROOT,
                egui::ViewportCommand::RequestPaintWhileHidden,
            );
        }
        let mut app = Self {
            hotkey_draft: settings.hotkeys.clone(),
            settings,
            config_path,
            save_blocked,
            dirty: None,
            demo,
            portable,
            startup_status,
            startup_error,
            preferences_open,
            preferences_position_pending: preferences_open,
            popup_open,
            popup_had_focus: false,
            popup_focus_pending: popup_open && std::env::var_os("MMB_CAPTURE_TO").is_none(),
            popup_anchor,
            popup_details: false,
            selected_profile: None,
            profile_editor: None,
            profile_name: String::new(),
            profile_error: None,
            profile_feedback: None,
            profile_delete_pending: None,
            profile_application: None,
            updates,
            prepared_update: None,
            monitors,
            hotkeys,
            tray,
            displays: Vec::new(),
            scanning,
            tab: Tab::Settings,
            notice,
            shortcut_error: None,
            shortcuts_pending,
            revision: 0,
            last_schedule: Instant::now() - Duration::from_secs(2),
            schedule_was_active: false,
            paused_cycle: None,
            quitting: None,
            #[cfg(feature = "tray-smoke-test")]
            smoke: tray_smoke::SmokeTest::new(cc, demo),
        };
        // Capture populated screens for visual QA without sending mouse/keyboard input.
        if cfg!(feature = "ui-capture") && std::env::var_os("MMB_CAPTURE_TO").is_some() {
            if let Some(controller) = &app.monitors
                && let Ok(event) = controller.events.recv_timeout(Duration::from_secs(5))
            {
                app.monitor_event(event);
            }
            match std::env::var("MMB_CAPTURE_TAB").as_deref() {
                Ok("profiles") => {
                    app.save_current_profile("Reading", Some(38));
                    app.tab = Tab::Profiles;
                    app.preferences_open = true;
                    app.popup_open = false;
                }
                Ok("schedule") => {
                    app.tab = Tab::Evening;
                    app.preferences_open = true;
                    app.popup_open = false;
                }
                Ok("settings") => {
                    app.tab = Tab::Settings;
                    app.preferences_open = true;
                    app.popup_open = false;
                }
                _ => {}
            }
        }
        app
    }

    fn dirty(&mut self) {
        self.dirty = Some(Instant::now());
    }

    fn set_startup(&mut self, enabled: bool) {
        match startup::set_enabled(enabled, self.portable) {
            Ok(status) => {
                self.startup_status = status;
                self.startup_error = None;
            }
            Err(error) => self.startup_error = Some(error),
        }
    }

    fn save(&mut self) {
        if self.save_blocked || self.demo {
            self.dirty = None;
            return;
        }
        if let Some(path) = &self.config_path {
            match config::save(path, &self.settings) {
                Ok(()) => self.dirty = None,
                Err(error) => {
                    self.notice = Some(error);
                    self.dirty = None;
                }
            }
        }
    }

    fn included(&self, index: usize) -> bool {
        self.displays[index].info.brightness.is_some()
            && self
                .settings
                .monitors
                .get(&self.displays[index].info.id)
                .is_some_and(|m| m.included)
    }

    fn queue(&mut self, index: usize, value: u8, manual: bool) {
        if self.quitting.is_some() {
            return;
        }
        let display = &mut self.displays[index];
        if display.info.brightness.is_none() || (display.value == value && display.error.is_none())
        {
            return;
        }
        if let Some(application) = &mut self.profile_application
            && let Some((_, outcome)) = application.outcomes.get_mut(&display.info.id)
            && matches!(outcome, Outcome::Pending { .. })
        {
            *outcome = Outcome::Superseded;
        }
        self.revision += 1;
        display.value = value;
        display.revision = self.revision;
        display.queued.get_or_insert_with(Instant::now);
        display.error = None;
        if manual {
            display.manual.insert(self.revision);
        }
    }

    fn pause_schedule(&mut self) {
        let (day, minute) = platform::local_clock();
        if let Some((cycle, _)) = self.settings.schedule.phase(day, minute) {
            self.paused_cycle = Some(cycle);
        }
    }

    fn adjust(&mut self, delta: i16) {
        self.pause_schedule();
        for index in 0..self.displays.len() {
            if self.included(index) {
                self.queue(
                    index,
                    model::adjusted(self.displays[index].value, delta),
                    true,
                );
            }
        }
    }

    fn set_all(&mut self, value: u8) {
        self.pause_schedule();
        for index in 0..self.displays.len() {
            if self.included(index) {
                self.queue(index, value, true);
            }
        }
    }

    fn flush(&mut self, force: bool) {
        let Some(controller) = &self.monitors else {
            return;
        };
        let mut failures = Vec::new();
        for display in &mut self.displays {
            if display
                .queued
                .is_some_and(|at| force || at.elapsed() >= Duration::from_millis(120))
            {
                let command = Command::Set {
                    id: display.info.id.clone(),
                    value: display.value,
                    revision: display.revision,
                };
                match controller.commands.send(command) {
                    Ok(()) => {
                        display.queued = None;
                        display.writing = true;
                    }
                    Err(_) => {
                        failures.push((display.info.id.clone(), display.revision));
                        display.queued = None;
                        display.writing = false;
                        display.error = Some(
                            "Monitor service stopped. Restart Multi-Monitor Brightness.".into(),
                        );
                        display.value = display.confirmed;
                    }
                }
            }
        }
        for (id, revision) in failures {
            self.profile_written(&id, revision, false);
        }
    }

    fn receive(&mut self, ctx: &egui::Context) {
        let events: Vec<_> = self
            .monitors
            .as_ref()
            .map(|m| m.events.try_iter().collect())
            .unwrap_or_default();
        if !events.is_empty() {
            ctx.request_repaint();
        }
        for event in events {
            self.monitor_event(event);
        }
        let events: Vec<_> = self
            .hotkeys
            .as_ref()
            .map(|h| h.events.try_iter().collect())
            .unwrap_or_default();
        if !events.is_empty() {
            ctx.request_repaint();
        }
        for event in events {
            match event {
                HotkeyEvent::Action(action) => self.action(action, ctx),
                HotkeyEvent::Registered(result) => {
                    self.shortcuts_pending = false;
                    self.shortcut_error = result.err();
                }
            }
        }
        let actions: Vec<_> = self
            .tray
            .as_ref()
            .map(|t| t.events.try_iter().collect())
            .unwrap_or_default();
        if !actions.is_empty() {
            ctx.request_repaint();
        }
        for action in actions {
            self.action(action, ctx);
        }
    }

    fn monitor_event(&mut self, event: monitors::Event) {
        match event {
            monitors::Event::Scanned(result) => {
                self.scanning = false;
                match result {
                    Ok(infos) => self.merge_displays(infos),
                    Err(error) => self.notice = Some(error),
                }
            }
            monitors::Event::Written {
                id,
                value,
                revision,
                result,
            } => {
                self.profile_written(&id, revision, result.is_ok());
                if let Some(display) = self.displays.iter_mut().find(|d| d.info.id == id) {
                    let manual = display.manual.contains(&revision);
                    display.manual.retain(|r| *r > revision);
                    match result {
                        Ok(()) => {
                            display.confirmed = value;
                            if manual {
                                self.settings.monitors.entry(id).or_default().brightness = value;
                                self.dirty = Some(Instant::now());
                            }
                            if display.revision == revision {
                                display.writing = false;
                                display.error = None;
                            }
                        }
                        Err(error) => {
                            if display.revision == revision {
                                display.writing = false;
                                display.value = display.confirmed;
                                display.error = Some(error);
                            }
                        }
                    }
                }
            }
        }
    }

    fn merge_displays(&mut self, infos: Vec<MonitorInfo>) {
        if let Some(application) = &mut self.profile_application {
            for (id, (_, outcome)) in &mut application.outcomes {
                if matches!(outcome, Outcome::Pending { .. })
                    && !infos
                        .iter()
                        .any(|info| info.id == *id && info.brightness.is_some())
                {
                    *outcome = Outcome::Missing;
                }
            }
        }
        let mut old = std::mem::take(&mut self.displays);
        let mut restore = Vec::new();
        for info in infos {
            let index = self.displays.len();
            let brightness = info.brightness.unwrap_or(0);
            if let Some(old_index) = old.iter().position(|d| d.info.id == info.id) {
                let mut display = old.remove(old_index);
                if display.queued.is_none() && !display.writing {
                    display.value = brightness;
                    display.confirmed = brightness;
                    display.error = None;
                }
                display.info = info;
                self.displays.push(display);
            } else {
                let saved = self.settings.monitors.get(&info.id).cloned();
                if info.brightness.is_some() {
                    self.settings
                        .monitors
                        .entry(info.id.clone())
                        .or_insert(MonitorMemory {
                            brightness,
                            included: true,
                        });
                    if self.settings.restore_on_start
                        && let Some(memory) = saved
                    {
                        restore.push((index, memory.brightness));
                    }
                }
                self.displays.push(Display {
                    info,
                    value: brightness,
                    confirmed: brightness,
                    revision: 0,
                    queued: None,
                    writing: false,
                    manual: BTreeSet::new(),
                    error: None,
                });
            }
        }
        for (index, value) in restore {
            self.queue(index, value, false);
        }
        self.last_schedule = Instant::now() - Duration::from_secs(2);
        self.dirty();
    }

    fn show_popup(&mut self, anchor: Option<platform::Anchor>, ctx: &egui::Context) {
        self.popup_anchor = anchor.or_else(|| self.tray.as_ref().and_then(|t| t.anchor()));
        self.popup_open = true;
        self.popup_had_focus = false;
        self.popup_focus_pending = true;
        ctx.send_viewport_cmd_to(
            egui::ViewportId::ROOT,
            egui::ViewportCommand::RequestPaintWhileHidden,
        );
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }

    fn hide_popup(&mut self, ctx: &egui::Context) {
        self.popup_open = false;
        self.flush(true);
        ctx.send_viewport_cmd_to(popup_id(), egui::ViewportCommand::Visible(false));
        ctx.send_viewport_cmd_to(
            egui::ViewportId::ROOT,
            egui::ViewportCommand::RequestPaintWhileHidden,
        );
    }

    fn open_preferences(&mut self, tab: Tab, ctx: &egui::Context) {
        let minimized = ctx.input(|input| {
            input
                .raw
                .viewports
                .get(&egui::ViewportId::ROOT)
                .is_some_and(|viewport| viewport.minimized == Some(true))
        });
        self.preferences_position_pending |= !self.preferences_open || minimized;
        self.hide_popup(ctx);
        self.preferences_open = true;
        self.tab = tab;
        for command in [
            egui::ViewportCommand::Visible(true),
            egui::ViewportCommand::Minimized(false),
            egui::ViewportCommand::Focus,
        ] {
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, command);
        }
    }

    fn check_updates(&mut self) {
        if !self.demo && self.quitting.is_none() {
            self.updates.check();
        }
    }

    fn action(&mut self, action: Action, ctx: &egui::Context) {
        match action {
            Action::Up => self.adjust(self.settings.step as i16),
            Action::Down => self.adjust(-(self.settings.step as i16)),
            Action::Show => self.show_popup(None, ctx),
            Action::TogglePopup(anchor) => {
                if self.popup_open {
                    self.hide_popup(ctx);
                } else {
                    self.show_popup(Some(anchor), ctx);
                }
            }
            Action::Settings => self.open_preferences(Tab::Settings, ctx),
            Action::Schedule => self.open_preferences(Tab::Evening, ctx),
            Action::CheckUpdates => {
                self.open_preferences(Tab::Settings, ctx);
                self.check_updates();
            }
            Action::Quit => {
                self.prepared_update = None;
                self.popup_open = false;
                self.quitting = Some(Instant::now());
                self.flush(true);
            }
        }
    }

    fn schedule_tick(&mut self) {
        if self.last_schedule.elapsed() < Duration::from_secs(1) {
            return;
        }
        self.last_schedule = Instant::now();
        let (day, minute) = platform::local_clock();
        self.schedule_at(day, minute);
    }

    fn schedule_at(&mut self, day: i64, minute: u16) {
        let phase = self.settings.schedule.phase(day, minute);
        if let Some((cycle, progress)) = phase {
            if self.paused_cycle != Some(cycle) {
                self.paused_cycle = None;
                for i in 0..self.displays.len() {
                    if self.included(i) && self.displays[i].error.is_none() {
                        let base = self.settings.monitors[&self.displays[i].info.id].brightness;
                        self.queue(
                            i,
                            model::dimmed(base, self.settings.schedule.target, progress),
                            false,
                        );
                    }
                }
            }
        } else {
            self.paused_cycle = None;
            if self.schedule_was_active {
                for i in 0..self.displays.len() {
                    if self.included(i) && self.displays[i].error.is_none() {
                        self.queue(
                            i,
                            self.settings.monitors[&self.displays[i].info.id].brightness,
                            false,
                        );
                    }
                }
            }
        }
        self.schedule_was_active = phase.is_some();
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(36.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 9, palette(ui).track);
            sun(ui.painter(), rect.center(), 9.0, palette(ui).accent);
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.label(RichText::new(tr(ui, "Preferences")).size(21.0).strong());
                ui.label(
                    RichText::new("Multi-Monitor Brightness")
                        .color(palette(ui).muted)
                        .size(12.0),
                );
            });
        });
        ui.add_space(18.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (tab, label) in [
                (Tab::Profiles, tr(ui, "Profiles")),
                (Tab::Evening, tr(ui, "Schedule")),
                (Tab::Settings, tr(ui, "Settings")),
            ] {
                let active = self.tab == tab;
                let response = ui.add(
                    egui::Button::new(RichText::new(label).size(13.0).color(if active {
                        palette(ui).text
                    } else {
                        palette(ui).muted
                    }))
                    .fill(if active {
                        palette(ui).card
                    } else {
                        Color32::TRANSPARENT
                    })
                    .stroke(Stroke::NONE)
                    .corner_radius(6)
                    .min_size(Vec2::new(92.0, 32.0)),
                );
                if active {
                    let line = response.rect.shrink2(Vec2::new(13.0, 0.0));
                    ui.painter().line_segment(
                        [line.left_bottom(), line.right_bottom()],
                        Stroke::new(2.0, palette(ui).accent),
                    );
                }
                if response.clicked() {
                    self.tab = tab;
                }
            }
        });
        ui.add_space(18.0);
    }

    fn displays_ui(&mut self, ui: &mut egui::Ui) {
        let count = (0..self.displays.len())
            .filter(|i| self.included(*i))
            .count();
        if count > 1 {
            let average = ((0..self.displays.len())
                .filter(|i| self.included(*i))
                .map(|i| self.displays[i].value as u32)
                .sum::<u32>()
                / count as u32) as u8;
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(tr(ui, "All linked displays"))
                        .size(12.0)
                        .color(palette(ui).muted),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("{average}%")).size(20.0));
                });
            });
            let mut value = average;
            let response = brightness_slider(
                ui,
                &mut value,
                ui.available_width(),
                self.settings.reduced_motion,
            );
            #[cfg(feature = "render-smoke-test")]
            {
                let viewport = ui.ctx().viewport_id();
                ui.ctx().data_mut(|data| {
                    data.insert_temp(
                        egui::Id::unique("render-probe-slider"),
                        (viewport, response.rect, value),
                    )
                });
            }
            if response.changed() {
                self.set_all(value);
            }
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(8.0);
        }
        if self.displays.is_empty() {
            if self.scanning {
                ui.horizontal(|ui| {
                    if !self.settings.reduced_motion {
                        ui.spinner();
                    }
                    ui.label(tr(ui, "Finding displays…"));
                });
            } else {
                ui.label(tr(ui, "No displays detected"));
            }
        }
        let supported = self
            .displays
            .iter()
            .filter(|d| d.info.brightness.is_some())
            .count();
        for i in 0..self.displays.len() {
            if self.displays[i].info.brightness.is_none() {
                continue;
            }
            ui.push_id(self.displays[i].info.id.clone(), |ui| {
                ui.horizontal(|ui| {
                    let width = (ui.available_width()
                        - if supported > 1 || !self.included(i) {
                            112.0
                        } else {
                            58.0
                        })
                    .max(60.0);
                    ui.allocate_ui_with_layout(
                        Vec2::new(width, 24.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(if self.demo {
                                        tr(ui, &self.displays[i].info.name)
                                    } else {
                                        &self.displays[i].info.name
                                    })
                                    .size(13.0),
                                )
                                .truncate(),
                            )
                        },
                    )
                    .inner
                    .on_hover_text(i18n::display_detail(ui, &self.displays[i].info.detail));
                    if supported > 1 || !self.included(i) {
                        let memory = self
                            .settings
                            .monitors
                            .entry(self.displays[i].info.id.clone())
                            .or_default();
                        if ui
                            .checkbox(
                                &mut memory.included,
                                RichText::new(tr(ui, "Link"))
                                    .size(10.0)
                                    .color(palette(ui).muted),
                            )
                            .on_hover_text(tr(
                                ui,
                                "Include in shared controls, hotkeys and the evening schedule",
                            ))
                            .changed()
                        {
                            if let Some(application) = &mut self.profile_application
                                && let Some((_, outcome)) =
                                    application.outcomes.get_mut(&self.displays[i].info.id)
                                && matches!(outcome, Outcome::Pending { .. })
                            {
                                *outcome = Outcome::Superseded;
                            }
                            self.dirty();
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(format!("{}%", self.displays[i].value)).size(20.0));
                    });
                });
                let mut value = self.displays[i].value;
                if brightness_slider(
                    ui,
                    &mut value,
                    ui.available_width(),
                    self.settings.reduced_motion,
                )
                .changed()
                {
                    self.pause_schedule();
                    self.queue(i, value, true);
                }
                if let Some(error) = &self.displays[i].error {
                    ui.label(
                        RichText::new(i18n::message(ui, error))
                            .size(11.0)
                            .color(palette(ui).warm),
                    );
                } else if self.displays[i].queued.is_some() || self.displays[i].writing {
                    ui.label(
                        RichText::new(tr(ui, "Changing brightness…"))
                            .size(11.0)
                            .color(palette(ui).accent),
                    );
                }
                ui.add_space(10.0);
            });
        }
        let unavailable = self.displays.len() - supported;
        if unavailable > 0 {
            if ui
                .small_button(i18n::format(
                    ui,
                    if unavailable == 1 {
                        "{count} display unavailable"
                    } else {
                        "{count} displays unavailable"
                    },
                    &[("count", unavailable.to_string())],
                ))
                .clicked()
            {
                self.popup_details = !self.popup_details;
            }
            if self.popup_details {
                for d in self.displays.iter().filter(|d| d.info.brightness.is_none()) {
                    ui.label(
                        RichText::new(if self.demo {
                            tr(ui, &d.info.name)
                        } else {
                            &d.info.name
                        })
                        .size(11.0)
                        .color(palette(ui).muted),
                    );
                }
                ui.label(RichText::new(tr(ui, "Enable DDC/CI in the monitor's menu, then refresh. Some built-in screens and docks are not supported.")).size(11.0).color(palette(ui).muted));
            }
        }
        if self.settings.schedule.enabled {
            ui.label(
                RichText::new(if self.paused_cycle.is_some() {
                    tr(ui, "Evening dimming paused until morning")
                } else {
                    tr(ui, "Evening dimming enabled")
                })
                .color(palette(ui).muted)
                .size(10.0),
            );
        }
    }

    fn popup_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if self.popup_focus_pending {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            let size = self.popup_size();
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                platform::popup_geometry(self.popup_anchor, size, &ctx).0,
            ));
            self.popup_focus_pending = false;
        }
        let focused = ctx.input(|i| i.viewport().focused);
        if focused == Some(true) {
            self.popup_had_focus = true;
        }
        let over_tray = self.popup_anchor.is_some_and(platform::cursor_in);
        if ctx.input(|i| i.key_pressed(egui::Key::Escape) || i.viewport().close_requested())
            || (self.popup_had_focus && focused == Some(false) && !over_tray)
        {
            self.hide_popup(&ctx);
            return;
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(palette(ui).bg)
                    .stroke(Stroke::new(1.0, palette(ui).border))
                    .inner_margin(16)
                    .corner_radius(10),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tr(ui, "Brightness")).size(17.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button("×")
                            .on_hover_text(tr(ui, "Close · Esc"))
                            .clicked()
                        {
                            self.hide_popup(&ctx);
                        }
                        if ui.small_button(tr(ui, "Settings")).clicked() {
                            self.open_preferences(Tab::Settings, &ctx);
                        }
                    });
                });
                ui.add_space(12.0);
                let footer = 32.0;
                egui::ScrollArea::vertical()
                    .max_height((ui.available_height() - footer).max(40.0))
                    .show(ui, |ui| {
                        self.quick_profiles_ui(ui);
                        self.displays_ui(ui);
                    });
                ui.add_space(4.0);
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !self.scanning,
                            egui::Button::new(RichText::new(tr(ui, "Refresh")).size(11.0))
                                .frame(false),
                        )
                        .clicked()
                        && let Some(service) = &self.monitors
                    {
                        let _ = service.commands.send(Command::Refresh);
                        self.scanning = true;
                    }
                    if ui
                        .add(
                            egui::Button::new(RichText::new(tr(ui, "Schedule")).size(11.0))
                                .frame(false),
                        )
                        .clicked()
                    {
                        self.open_preferences(Tab::Evening, &ctx);
                    }
                    if self.updates.update_available()
                        && ui.small_button(tr(ui, "Update available")).clicked()
                    {
                        self.open_preferences(Tab::Settings, &ctx);
                    }
                    if self.demo {
                        ui.label(
                            RichText::new(tr(ui, "Preview"))
                                .size(10.0)
                                .color(palette(ui).muted),
                        );
                    }
                });
            });
        #[cfg(feature = "ui-capture")]
        if !matches!(
            std::env::var("MMB_CAPTURE_TAB").as_deref(),
            Ok("settings" | "schedule" | "profiles")
        ) {
            self.capture(&ctx);
        }
    }

    fn popup_size(&self) -> Vec2 {
        let supported = self
            .displays
            .iter()
            .filter(|d| d.info.brightness.is_some())
            .count();
        let linked = (0..self.displays.len())
            .filter(|i| self.included(*i))
            .count();
        let mut height = 206.0 + supported.max(1) as f32 * if supported > 1 { 96.0 } else { 70.0 };
        if self.profile_application.is_some() {
            height += 56.0;
        }
        if linked > 1 {
            height += 84.0;
        }
        if supported < self.displays.len() {
            height += if self.popup_details { 110.0 } else { 28.0 };
        }
        height += self.displays.iter().filter(|d| d.error.is_some()).count() as f32 * 55.0;
        if self.settings.schedule.enabled {
            height += 18.0;
        }
        Vec2::new(360.0, height.clamp(190.0, 640.0))
    }

    fn evening_ui(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new(tr(ui, "Evening schedule"))
                .size(20.0)
                .strong(),
        );
        ui.add_space(5.0);
        ui.label(
            RichText::new(tr(
                ui,
                "Lower brightness gradually, then restore it in the morning.",
            ))
            .color(palette(ui).muted),
        );
        ui.add_space(22.0);
        let previous = self.settings.schedule.clone();
        card(ui, |ui| {
            ui.checkbox(
                &mut self.settings.schedule.enabled,
                RichText::new(tr(ui, "Smooth evening dimming"))
                    .size(16.0)
                    .strong(),
            );
            ui.add_space(8.0);
            ui.label(
                RichText::new(tr(
                    ui,
                    "Gradually dim linked displays. Your usual brightness returns in the morning.",
                ))
                .color(palette(ui).muted)
                .size(13.0),
            );
            ui.add_space(20.0);
            egui::Grid::new("schedule")
                .num_columns(2)
                .spacing([28.0, 18.0])
                .show(ui, |ui| {
                    ui.label(tr(ui, "Start dimming"));
                    time_editor(ui, &mut self.settings.schedule.start, "evening_start");
                    ui.end_row();
                    ui.label(tr(ui, "Restore brightness"));
                    time_editor(ui, &mut self.settings.schedule.morning, "morning_start");
                    ui.end_row();
                    ui.label(tr(ui, "Fade duration"));
                    ui.add(
                        egui::DragValue::new(&mut self.settings.schedule.duration)
                            .range(1..=240)
                            .suffix(tr(ui, " min")),
                    );
                    ui.end_row();
                    ui.label(tr(ui, "Evening brightness"));
                    ui.add(
                        egui::DragValue::new(&mut self.settings.schedule.target)
                            .range(0..=100)
                            .suffix("%"),
                    );
                    ui.end_row();
                });
            ui.add_space(23.0);
            schedule_graph(ui, &self.settings.schedule);
        });
        if previous != self.settings.schedule {
            self.paused_cycle = None;
            self.last_schedule = Instant::now() - Duration::from_secs(2);
            self.dirty();
        }
        if self.settings.schedule.start == self.settings.schedule.morning {
            ui.add_space(12.0);
            ui.label(
                RichText::new(tr(
                    ui,
                    "Choose different evening and morning times to activate the schedule.",
                ))
                .color(palette(ui).warm),
            );
        }
        ui.add_space(16.0);
        ui.label(
            RichText::new(tr(ui, "Manual adjustments"))
                .size(13.0)
                .strong(),
        );
        ui.label(RichText::new(tr(ui, "Changing brightness pauses the schedule until morning. Displays below the target stay at their lower level. Times follow your computer's local clock.")).color(palette(ui).muted).size(12.0));
        if self.paused_cycle.is_some() {
            ui.add_space(8.0);
            if ui.button(tr(ui, "Resume evening dimming")).clicked() {
                self.paused_cycle = None;
            }
        }
    }

    fn appearance_ui(&mut self, ui: &mut egui::Ui) {
        let previous_language = self.settings.language;
        let previous_theme = self.settings.theme;
        let previous_motion = self.settings.reduced_motion;
        card(ui, |ui| {
            ui.label(RichText::new(tr(ui, "Appearance")).size(17.0).strong());
            ui.add_space(10.0);
            egui::Grid::new("appearance")
                .spacing([24.0, 10.0])
                .show(ui, |ui| {
                    ui.label(tr(ui, "Language"));
                    egui::ComboBox::from_id_salt("language")
                        .selected_text(self.settings.language.name())
                        .show_ui(ui, |ui| {
                            for language in Language::ALL {
                                ui.selectable_value(
                                    &mut self.settings.language,
                                    language,
                                    language.name(),
                                );
                            }
                        });
                    ui.end_row();
                    ui.label(tr(ui, "Theme"));
                    ui.horizontal(|ui| {
                        ui.selectable_value(
                            &mut self.settings.theme,
                            Theme::Light,
                            tr(ui, "Light"),
                        );
                        ui.selectable_value(&mut self.settings.theme, Theme::Dark, tr(ui, "Dark"));
                    });
                    ui.end_row();
                });
            ui.add_space(8.0);
            ui.checkbox(&mut self.settings.reduced_motion, tr(ui, "Reduce motion"))
                .on_hover_text(tr(
                    ui,
                    "Use immediate transitions instead of slider and selection animations.",
                ));
            ui.add_space(8.0);
            ui.label(
                RichText::new(tr(ui, "Language and theme changes apply immediately."))
                    .size(11.0)
                    .color(palette(ui).muted),
            );
        });
        if previous_language != self.settings.language {
            self.settings.language.install(ui.ctx());
            if let Some(tray) = &self.tray {
                tray.set_language(self.settings.language);
            }
            ui.ctx().send_viewport_cmd_to(
                egui::ViewportId::ROOT,
                egui::ViewportCommand::Title(format!(
                    "Multi-Monitor Brightness — {}",
                    self.settings.language.text("Preferences")
                )),
            );
            ui.ctx().request_repaint();
        }
        if previous_theme != self.settings.theme {
            self.settings.theme.apply(ui.ctx());
        }
        if previous_motion != self.settings.reduced_motion {
            theme::motion(ui.ctx(), self.settings.reduced_motion);
        }
        if previous_language != self.settings.language
            || previous_theme != self.settings.theme
            || previous_motion != self.settings.reduced_motion
        {
            self.dirty();
        }
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr(ui, "Settings")).size(20.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(i18n::format(
                        ui,
                        "Version {version}",
                        &[("version", crate::VERSION.into())],
                    ))
                    .size(12.0)
                    .color(palette(ui).muted),
                );
            });
        });
        ui.add_space(18.0);
        self.appearance_ui(ui);
        ui.add_space(12.0);
        self.updates_ui(ui);
        ui.add_space(12.0);
        card(ui, |ui| {
            ui.label(RichText::new(tr(ui, "Application")).size(17.0).strong());
            ui.add_space(10.0);
            let mut enabled = self.startup_status.enabled;
            if ui
                .add_enabled(
                    !self.demo,
                    egui::Checkbox::new(&mut enabled, tr(ui, "Start with Windows")),
                )
                .on_hover_text(tr(ui, "Start quietly in the system tray when you sign in"))
                .changed()
            {
                self.set_startup(enabled);
            }
            ui.label(
                RichText::new(tr(ui, "Starts in the tray when you sign in."))
                    .size(11.0)
                    .color(palette(ui).muted),
            );
            if self.startup_status.enabled && !self.startup_status.uses_current_copy {
                ui.label(
                    RichText::new(tr(ui, "Autostart currently uses another copy of the app."))
                        .size(12.0)
                        .color(palette(ui).muted),
                );
                if ui.button(tr(ui, "Use this executable")).clicked() {
                    self.set_startup(true);
                }
            }
            if let Some(error) = &self.startup_error {
                ui.label(
                    RichText::new(i18n::message(ui, error))
                        .size(12.0)
                        .color(palette(ui).warm),
                );
            }
            ui.add_space(6.0);
            if ui
                .checkbox(
                    &mut self.settings.restore_on_start,
                    tr(
                        ui,
                        "Restore each display's brightness on launch / reconnect",
                    ),
                )
                .changed()
            {
                self.dirty();
            }
            ui.add_space(12.0);
            if let Some(path) = &self.config_path {
                ui.label(
                    RichText::new(tr(ui, "Settings are saved automatically"))
                        .size(12.0)
                        .color(palette(ui).muted),
                );
                ui.label(
                    RichText::new(path.display().to_string())
                        .size(11.0)
                        .color(palette(ui).muted),
                );
            } else if self.demo {
                ui.label(
                    RichText::new(tr(ui, "Demo settings are temporary."))
                        .size(12.0)
                        .color(palette(ui).muted),
                );
            }
            if self.save_blocked {
                ui.label(RichText::new(tr(ui, "Saving is paused to preserve your original settings. Fix or move the settings file and restart.")).color(palette(ui).warm).size(12.0));
            }
        });
        ui.add_space(12.0);
        card(ui, |ui| {
            ui.label(
                RichText::new(tr(ui, "Keyboard shortcuts"))
                    .size(17.0)
                    .strong(),
            );
            ui.add_space(10.0);
            ui.checkbox(
                &mut self.hotkey_draft.enabled,
                tr(ui, "Enable global shortcuts"),
            );
            ui.add_space(10.0);
            hotkey_editor(
                ui,
                tr(ui, "Increase brightness"),
                "key_up",
                &mut self.hotkey_draft.up,
            );
            hotkey_editor(
                ui,
                tr(ui, "Decrease brightness"),
                "key_down",
                &mut self.hotkey_draft.down,
            );
            hotkey_editor(
                ui,
                tr(ui, "Open brightness"),
                "key_show",
                &mut self.hotkey_draft.show,
            );
            ui.add_space(13.0);
            ui.horizontal(|ui| {
                ui.label(tr(ui, "Brightness step"));
                if ui
                    .add(
                        egui::DragValue::new(&mut self.settings.step)
                            .range(1..=25)
                            .suffix("%"),
                    )
                    .changed()
                {
                    self.dirty();
                }
            });
            ui.add_space(13.0);
            if ui
                .add_enabled(
                    !self.shortcuts_pending,
                    egui::Button::new(tr(ui, "Apply shortcuts")),
                )
                .clicked()
            {
                match self.hotkey_draft.validate() {
                    Ok(()) => {
                        self.settings.hotkeys = self.hotkey_draft.clone();
                        self.shortcut_error = None;
                        if let Some(service) = &self.hotkeys {
                            service.configure(&self.settings.hotkeys);
                            self.shortcuts_pending = true;
                        }
                        self.dirty();
                    }
                    Err(error) => self.shortcut_error = Some(error),
                }
            }
            ui.add_space(7.0);
            let (text, color) = if self.demo {
                (
                    tr(ui, "Shortcuts are preview-only in demo mode."),
                    palette(ui).muted,
                )
            } else if let Some(error) = self.shortcut_error.as_deref() {
                (error, palette(ui).warm)
            } else if self.shortcuts_pending {
                (tr(ui, "Registering shortcuts…"), palette(ui).muted)
            } else if self.hotkeys.is_none() {
                (
                    tr(ui, "Shortcut service unavailable. Restart the application."),
                    palette(ui).warm,
                )
            } else if self.settings.hotkeys.enabled {
                (
                    tr(
                        ui,
                        "Shortcuts are active, including while the window is hidden.",
                    ),
                    palette(ui).muted,
                )
            } else {
                (tr(ui, "Global shortcuts are disabled."), palette(ui).muted)
            };
            ui.label(
                RichText::new(i18n::message(ui, text))
                    .size(12.0)
                    .color(color),
            );
        });
        ui.add_space(15.0);
        ui.label(
            RichText::new(tr(ui, "GPLv3 License"))
                .color(palette(ui).muted)
                .size(12.0),
        );
    }
}

impl MultiMonitorBrightness {
    fn tick(&mut self, ctx: &egui::Context) {
        let before = self.background_visual_state();
        self.receive(ctx);
        let update = self.updates.poll(
            !self.demo && self.quitting.is_none() && self.settings.check_updates_automatically,
        );
        if update.checked {
            self.settings.last_update_check = updater::now();
            self.dirty();
            ctx.request_repaint();
        }
        if update.ready_to_install {
            let prepared = self.updates.take_prepared();
            if self.quitting.is_none() {
                self.prepared_update = prepared;
                self.quitting = Some(Instant::now());
                self.popup_open = false;
            }
        }
        #[cfg(feature = "tray-smoke-test")]
        if let Some(mut smoke) = self.smoke.take() {
            smoke.tick(self, ctx);
            self.smoke = Some(smoke);
        }
        if self.quitting.is_none() {
            self.schedule_tick();
        }
        self.flush(false);
        if self
            .dirty
            .is_some_and(|at| at.elapsed() > Duration::from_millis(600))
        {
            self.save();
        }
        if let Some(since) = self.quitting
            && (!self
                .displays
                .iter()
                .any(|d| d.queued.is_some() || d.writing)
                || since.elapsed() > Duration::from_secs(5))
        {
            self.save();
            if let Some(mut prepared) = self.prepared_update.take()
                && let Err(error) = updater::handoff(&mut prepared)
            {
                self.updates.installation_failed(error);
                self.quitting = None;
                ctx.request_repaint();
                return;
            }
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
        }
        if before != self.background_visual_state() {
            ctx.request_repaint();
        }
    }

    fn background_visual_state(&self) -> BackgroundVisualState {
        BackgroundVisualState {
            revision: self.revision,
            schedule_active: self.schedule_was_active,
            paused_cycle: self.paused_cycle,
            notice: self.notice.clone(),
            displays: self
                .displays
                .iter()
                .map(|d| (d.value, d.confirmed, d.writing, d.error.clone()))
                .collect(),
        }
    }
}

#[derive(PartialEq)]
struct BackgroundVisualState {
    revision: u64,
    schedule_active: bool,
    paused_cycle: Option<i64>,
    notice: Option<String>,
    displays: Vec<(u8, u8, bool, Option<String>)>,
}

impl MultiMonitorBrightness {
    fn updates_ui(&mut self, ui: &mut egui::Ui) {
        if crate::update_panel::show(
            ui,
            &mut self.updates,
            &mut self.settings.check_updates_automatically,
            self.settings.reduced_motion,
        ) {
            self.dirty();
        }
    }

    #[cfg(feature = "ui-capture")]
    fn capture(&mut self, ctx: &egui::Context) {
        match crate::capture::update(ctx) {
            Ok(true) => self.quitting = Some(Instant::now()),
            Ok(false) => {}
            Err(error) => {
                eprintln!("UI capture failed: {error}");
                std::process::exit(1);
            }
        }
    }
}

// Deferred native viewports can render independently while preferences stay hidden.
// Workers communicate through channels; only UI callbacks access this shared state.
pub struct Application(std::rc::Rc<std::cell::RefCell<MultiMonitorBrightness>>);

thread_local! {
    // Native window/tray handles stay on the event-loop thread. The deferred callback
    // captures no handles and accesses state only on that same thread.
    static VIEWPORT_STATE: std::cell::RefCell<std::rc::Weak<std::cell::RefCell<MultiMonitorBrightness>>> = const { std::cell::RefCell::new(std::rc::Weak::new()) };
}

impl Application {
    pub fn new(state: MultiMonitorBrightness) -> Self {
        let state = std::rc::Rc::new(std::cell::RefCell::new(state));
        VIEWPORT_STATE.with(|slot| *slot.borrow_mut() = std::rc::Rc::downgrade(&state));
        Self(state)
    }
}

impl crate::host::App for Application {
    fn logic(&mut self, ctx: &egui::Context) {
        self.0.borrow_mut().tick(ctx);
    }

    fn logic_delay(&self) -> Duration {
        let app = self.0.borrow();
        if app.quitting.is_some() || app.displays.iter().any(|d| d.queued.is_some() || d.writing) {
            Duration::from_millis(60)
        } else {
            Duration::from_secs(1)
        }
    }

    fn ui(&mut self, root: &mut egui::Ui) {
        let ctx = root.ctx().clone();
        let builder = {
            let mut app = self.0.borrow_mut();
            app.ui(root);
            (app.popup_open && app.quitting.is_none()).then(|| {
                let (position, size) =
                    platform::popup_geometry(app.popup_anchor, app.popup_size(), &ctx);
                egui::ViewportBuilder::default()
                    .with_title(format!(
                        "Multi-Monitor Brightness — {}",
                        app.settings.language.text("Brightness")
                    ))
                    .with_visible(true)
                    .with_decorations(false)
                    .with_resizable(false)
                    .with_taskbar(false)
                    .with_active(std::env::var_os("MMB_CAPTURE_TO").is_none())
                    .with_window_level(egui::WindowLevel::AlwaysOnTop)
                    .with_inner_size(size)
                    .with_position(position)
            })
        };
        if let Some(builder) = builder {
            ctx.show_viewport_deferred(popup_id(), builder, |ui, _| {
                let state = VIEWPORT_STATE.with(|slot| slot.borrow().upgrade());
                if let Some(state) = state {
                    state.borrow_mut().popup_ui(ui);
                }
            });
        }
    }

    fn shutdown(&mut self) {
        self.0.borrow_mut().shutdown();
    }
}

impl MultiMonitorBrightness {
    fn ui(&mut self, root: &mut egui::Ui) {
        let context = root.ctx().clone();
        let ctx = &context;
        #[cfg(feature = "ui-capture")]
        crate::capture::configure_viewport(ctx);
        if !self.preferences_open {
            return;
        }
        if self.preferences_position_pending {
            self.preferences_position_pending = false;
            if !ctx.input(|input| input.viewport().maximized == Some(true)) {
                let anchor = self
                    .tray
                    .as_ref()
                    .and_then(|tray| tray.anchor())
                    .or(self.popup_anchor);
                let (position, size) = platform::preferences_geometry(anchor, ctx);
                for command in [
                    egui::ViewportCommand::OuterPosition(position),
                    egui::ViewportCommand::MinInnerSize(size.min(egui::vec2(500.0, 360.0))),
                    egui::ViewportCommand::InnerSize(size),
                ] {
                    ctx.send_viewport_cmd(command);
                }
            }
        }
        if self.preferences_open
            && self.quitting.is_none()
            && ctx.input(|i| i.viewport().close_requested())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.preferences_open = false;
            self.flush(true);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        egui::Panel::bottom("footer")
            .frame(
                egui::Frame::new()
                    .fill(palette(root).bg)
                    .inner_margin(egui::Margin::symmetric(24, 11)),
            )
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(if self.quitting.is_some() {
                            tr(ui, "Finishing brightness changes…")
                        } else if self.demo {
                            tr(ui, "Preview · Simulated displays")
                        } else {
                            tr(ui, "Changes are saved automatically")
                        })
                        .size(11.0)
                        .color(palette(ui).muted),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button(tr(ui, "Quit")).clicked() {
                            self.action(Action::Quit, ctx);
                        }
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(palette(root).bg)
                    .inner_margin(egui::Margin::symmetric(24, 22)),
            )
            .show(root, |ui| {
                self.header(ui);
                if let Some(notice) = self.notice.clone() {
                    egui::Frame::new()
                        .fill(palette(ui).warning_bg)
                        .inner_margin(12)
                        .corner_radius(8)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(i18n::message(ui, &notice))
                                        .color(palette(ui).warm)
                                        .size(12.0),
                                );
                                if !self.save_blocked
                                    && ui.small_button(tr(ui, "Dismiss")).clicked()
                                {
                                    self.notice = None;
                                }
                            });
                        });
                    ui.add_space(12.0);
                }
                if self.shortcut_error.is_some() && self.tab != Tab::Settings {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(tr(ui, "A keyboard shortcut is unavailable."))
                                .color(palette(ui).warm)
                                .size(12.0),
                        );
                        if ui.small_button(tr(ui, "Fix in Settings")).clicked() {
                            self.tab = Tab::Settings;
                        }
                    });
                    ui.add_space(12.0);
                }
                egui::ScrollArea::vertical()
                    .id_salt("content")
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        match self.tab {
                            Tab::Profiles => self.profiles_ui(ui),
                            Tab::Evening => self.evening_ui(ui),
                            Tab::Settings => self.settings_ui(ui),
                        }
                    });
            });
        #[cfg(feature = "ui-capture")]
        if matches!(
            std::env::var("MMB_CAPTURE_TAB").as_deref(),
            Ok("settings" | "schedule" | "profiles")
        ) {
            self.capture(ctx);
        }
    }

    fn shutdown(&mut self) {
        #[cfg(feature = "tray-smoke-test")]
        if let Some(smoke) = &self.smoke {
            smoke.finish();
        }
        if self.dirty.is_some() {
            self.save();
        }
    }
}

fn sun(painter: &egui::Painter, center: egui::Pos2, radius: f32, color: Color32) {
    painter.circle_stroke(center, radius * 0.5, Stroke::new(1.7, color));
    for i in 0..8 {
        let dir = Vec2::angled(i as f32 * std::f32::consts::FRAC_PI_4);
        painter.line_segment(
            [center + dir * radius * 0.8, center + dir * radius * 1.12],
            Stroke::new(1.7, color),
        );
    }
}

fn brightness_slider(
    ui: &mut egui::Ui,
    value: &mut u8,
    width: f32,
    reduced_motion: bool,
) -> egui::Response {
    let (rect, mut response) = ui.allocate_exact_size(
        Vec2::new(width.max(60.0), 28.0),
        egui::Sense::click_and_drag(),
    );
    let track = rect.shrink2(Vec2::new(10.0, 0.0));
    if response.clicked() {
        response.request_focus();
    }
    let old = *value;
    if ui.is_enabled()
        && (response.clicked() || response.dragged())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        *value =
            (((pointer.x - track.left()) / track.width()).clamp(0.0, 1.0) * 100.0).round() as u8;
    }
    if response.has_focus() && ui.is_enabled() {
        ui.input(|input| {
            if input.key_pressed(egui::Key::ArrowRight) || input.key_pressed(egui::Key::ArrowUp) {
                *value = model::adjusted(*value, 1);
            }
            if input.key_pressed(egui::Key::ArrowLeft) || input.key_pressed(egui::Key::ArrowDown) {
                *value = model::adjusted(*value, -1);
            }
            if input.key_pressed(egui::Key::Home) {
                *value = 0;
            }
            if input.key_pressed(egui::Key::End) {
                *value = 100;
            }
        });
    }
    if old != *value {
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::slider(ui.is_enabled(), *value as f64, tr(ui, "Brightness"))
    });
    let y = track.center().y;
    let visible = ui.is_rect_visible(rect);
    let position = crate::motion::value(
        ui,
        response.id.with("position"),
        *value as f32,
        reduced_motion || response.dragged() || !visible,
    );
    let hover = crate::motion::value(
        ui,
        response.id.with("hover"),
        if response.hovered() || response.has_focus() {
            1.0
        } else {
            0.0
        },
        reduced_motion || !visible,
    );
    let x = egui::lerp(track.x_range(), position / 100.0);
    ui.painter().line_segment(
        [egui::pos2(track.left(), y), egui::pos2(track.right(), y)],
        Stroke::new(4.0, palette(ui).track),
    );
    ui.painter().line_segment(
        [egui::pos2(track.left(), y), egui::pos2(x, y)],
        Stroke::new(4.0 + hover, palette(ui).accent),
    );
    ui.painter().circle_filled(
        egui::pos2(x, y),
        if response.dragged() { 7.0 } else { 6.0 + hover },
        palette(ui).text,
    );
    if response.has_focus() {
        ui.painter()
            .circle_stroke(egui::pos2(x, y), 10.0, Stroke::new(1.0, palette(ui).accent));
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn time_editor(ui: &mut egui::Ui, minute: &mut u16, id: &str) {
    ui.push_id(id, |ui| {
        let (mut hour, mut minutes) = (*minute / 60, *minute % 60);
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut hour)
                    .range(0..=23)
                    .min_decimals(0)
                    .custom_formatter(|v, _| format!("{:02}", v as u16)),
            );
            ui.label(":");
            ui.add(
                egui::DragValue::new(&mut minutes)
                    .range(0..=59)
                    .min_decimals(0)
                    .custom_formatter(|v, _| format!("{:02}", v as u16)),
            );
        });
        *minute = hour * 60 + minutes;
    });
}

fn hotkey_editor(ui: &mut egui::Ui, label: &str, id: &str, key: &mut Hotkey) {
    ui.push_id(id, |ui| {
        ui.horizontal(|ui| {
            ui.add_sized(
                [126.0, 25.0],
                egui::Label::new(RichText::new(label).size(12.0).color(palette(ui).muted)),
            );
            ui.checkbox(&mut key.ctrl, "Ctrl");
            ui.checkbox(&mut key.alt, "Alt");
            ui.checkbox(&mut key.shift, "Shift");
            egui::ComboBox::from_id_salt("key")
                .width(96.0)
                .selected_text(tr(ui, &key.key))
                .show_ui(ui, |ui| {
                    for name in [
                        "Up",
                        "Down",
                        "Left",
                        "Right",
                        "Page Up",
                        "Page Down",
                        "Home",
                        "End",
                    ] {
                        ui.selectable_value(&mut key.key, name.into(), tr(ui, name));
                    }
                    for n in 1..=11 {
                        let name = format!("F{n}");
                        ui.selectable_value(&mut key.key, name.clone(), name);
                    }
                    for c in ('A'..='Z').chain('0'..='9') {
                        let name = c.to_string();
                        ui.selectable_value(&mut key.key, name.clone(), name);
                    }
                });
        });
    });
}

fn schedule_graph(ui: &mut egui::Ui, schedule: &model::Schedule) {
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 76.0), egui::Sense::hover());
    let chart = rect.shrink2(Vec2::new(4.0, 16.0));
    let upper = chart.top() + 5.0;
    let lower = egui::lerp(
        upper..=(chart.bottom() - 5.0),
        1.0 - schedule.target as f32 / 100.0,
    );
    let from = egui::pos2(chart.left(), upper);
    let to = egui::pos2(chart.right(), lower);
    ui.painter().line_segment(
        [egui::pos2(chart.left(), lower), to],
        Stroke::new(1.0, palette(ui).border),
    );
    let points = vec![from, to];
    ui.painter().add(egui::Shape::line(
        points,
        Stroke::new(2.0, palette(ui).accent),
    ));
    ui.painter().text(
        rect.left_bottom(),
        egui::Align2::LEFT_BOTTOM,
        format!("{:02}:{:02}", schedule.start / 60, schedule.start % 60),
        FontId::proportional(11.0),
        palette(ui).muted,
    );
    ui.painter().text(
        rect.right_bottom(),
        egui::Align2::RIGHT_BOTTOM,
        i18n::format(
            ui,
            "{minutes} min → {target}%",
            &[
                ("minutes", schedule.duration.to_string()),
                ("target", schedule.target.to_string()),
            ],
        ),
        FontId::proportional(11.0),
        palette(ui).muted,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> MultiMonitorBrightness {
        let settings = Settings::default();
        let mut app = MultiMonitorBrightness {
            hotkey_draft: settings.hotkeys.clone(),
            settings,
            config_path: None,
            save_blocked: false,
            dirty: None,
            demo: true,
            portable: false,
            startup_status: startup::Status::default(),
            startup_error: None,
            preferences_open: false,
            preferences_position_pending: false,
            popup_open: false,
            popup_had_focus: false,
            popup_focus_pending: false,
            popup_anchor: None,
            popup_details: false,
            selected_profile: None,
            profile_editor: None,
            profile_name: String::new(),
            profile_error: None,
            profile_feedback: None,
            profile_delete_pending: None,
            profile_application: None,
            updates: updater::Controller::disabled(crate::VERSION, "Preview mode"),
            prepared_update: None,
            monitors: None,
            hotkeys: None,
            tray: None,
            displays: Vec::new(),
            scanning: false,
            tab: Tab::Settings,
            notice: None,
            shortcut_error: None,
            shortcuts_pending: false,
            revision: 0,
            last_schedule: Instant::now(),
            schedule_was_active: false,
            paused_cycle: None,
            quitting: None,
            #[cfg(feature = "tray-smoke-test")]
            smoke: None,
        };
        app.merge_displays(vec![
            MonitorInfo {
                id: "a".into(),
                name: "A".into(),
                detail: "Main display".into(),
                brightness: Some(72),
            },
            MonitorInfo {
                id: "b".into(),
                name: "B".into(),
                detail: "Second display".into(),
                brightness: Some(58),
            },
            MonitorInfo {
                id: "c".into(),
                name: "C".into(),
                detail: "Unsupported".into(),
                brightness: None,
            },
        ]);
        app
    }

    #[test]
    fn rendering_controls_does_not_change_brightness() {
        let mut app = app();
        let ctx = egui::Context::default();
        theme::initialize(&ctx, Theme::Dark);
        for _ in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(640.0, 800.0),
                    )),
                    ..Default::default()
                },
                |root| {
                    egui::CentralPanel::default().show(root, |ui| app.displays_ui(ui));
                },
            );
            // This headless layout check deliberately has no renderer or texture uploads.
            output.textures_delta.clear();
        }
        assert_eq!((app.displays[0].value, app.displays[1].value), (72, 58));
        assert_eq!(app.revision, 0);
    }

    #[test]
    fn hotkeys_preserve_offsets_and_honor_link_selection() {
        let mut app = app();
        app.adjust(5);
        assert_eq!((app.displays[0].value, app.displays[1].value), (77, 63));
        app.settings.monitors.get_mut("b").unwrap().included = false;
        app.set_all(30);
        assert_eq!((app.displays[0].value, app.displays[1].value), (30, 63));
        assert!(app.displays[2].queued.is_none());
    }

    #[test]
    fn delayed_responses_do_not_overwrite_newer_input_or_save_failed_values() {
        let mut app = app();
        app.queue(0, 60, true);
        let first = app.displays[0].revision;
        app.queue(0, 40, true);
        let last = app.displays[0].revision;
        app.displays[0].writing = true;
        app.displays[0].queued = None;
        app.monitor_event(monitors::Event::Written {
            id: "a".into(),
            value: 60,
            revision: first,
            result: Ok(()),
        });
        assert_eq!(app.displays[0].value, 40);
        assert!(app.displays[0].writing);
        app.monitor_event(monitors::Event::Written {
            id: "a".into(),
            value: 40,
            revision: last,
            result: Err("Disconnected".into()),
        });
        assert_eq!(app.displays[0].value, 60);
        assert_eq!(app.settings.monitors["a"].brightness, 60);
        assert!(!app.displays[0].writing);
        assert!(app.displays[0].error.is_some());
    }

    #[test]
    fn schedule_preserves_manual_memory_and_pause_expires_next_evening() {
        let mut app = app();
        app.settings.schedule.enabled = true;
        app.schedule_at(10, 20 * 60 + 30);
        assert_eq!((app.displays[0].value, app.displays[1].value), (49, 42));
        assert_eq!(app.settings.monitors["a"].brightness, 72);
        let rev = app.displays[0].revision;
        app.monitor_event(monitors::Event::Written {
            id: "a".into(),
            value: 49,
            revision: rev,
            result: Ok(()),
        });
        assert_eq!(app.settings.monitors["a"].brightness, 72);
        app.queue(0, 61, true);
        app.paused_cycle = Some(10);
        app.schedule_at(10, 22 * 60);
        assert_eq!(app.displays[0].value, 61);
        app.schedule_at(11, 8 * 60);
        assert_eq!(app.displays[0].value, 72);
        assert_eq!(app.paused_cycle, None);
        app.schedule_at(11, 21 * 60);
        assert_eq!(app.displays[0].value, 25);
    }

    #[test]
    fn reconnect_restores_only_the_matching_monitor() {
        let mut app = app();
        app.displays.clear();
        app.settings.monitors.get_mut("a").unwrap().brightness = 33;
        app.merge_displays(vec![
            MonitorInfo {
                id: "new".into(),
                name: "New screen".into(),
                detail: "".into(),
                brightness: Some(90),
            },
            MonitorInfo {
                id: "a".into(),
                name: "A".into(),
                detail: "".into(),
                brightness: Some(90),
            },
        ]);
        assert_eq!(app.displays[0].value, 90);
        assert!(app.displays[0].queued.is_none());
        assert_eq!(app.displays[1].value, 33);
        assert!(app.displays[1].queued.is_some());
    }

    fn saved(app: &mut MultiMonitorBrightness) -> u64 {
        app.save_current_profile("Reading", Some(35));
        assert!(app.profile_error.is_none());
        app.selected_profile.unwrap()
    }

    fn acknowledge(app: &mut MultiMonitorBrightness, index: usize, result: Result<(), String>) {
        let d = &mut app.displays[index];
        d.queued = None;
        let event = monitors::Event::Written {
            id: d.info.id.clone(),
            value: d.value,
            revision: d.revision,
            result,
        };
        app.monitor_event(event);
    }

    #[test]
    fn saving_and_editing_profiles_never_queue_hardware_changes() {
        let mut app = app();
        app.settings.monitors.get_mut("b").unwrap().included = false;
        app.save_current_profile("  Reading  ", None);
        let profile = &app.settings.profiles[0];
        assert_eq!(profile.name, "Reading");
        assert_eq!(profile.monitors["a"].brightness, 72);
        assert!(!profile.monitors["b"].included);
        assert!(!profile.monitors.contains_key("c"));
        assert_eq!(app.revision, 0);
        app.profile_editor.as_mut().unwrap().name = "Evening".into();
        app.profile_editor
            .as_mut()
            .unwrap()
            .monitors
            .get_mut("a")
            .unwrap()
            .brightness = 18;
        app.save_profile_edits(false);
        assert_eq!(app.settings.profiles[0].name, "Evening");
        assert_eq!(app.settings.profiles[0].monitors["a"].brightness, 18);
        assert_eq!(app.revision, 0);
        app.queue(0, 30, true);
        app.save_current_profile("Pending", None);
        assert_eq!(app.settings.profiles.len(), 1);
        assert!(app.profile_error.is_some());
    }

    #[test]
    fn profile_application_handles_reorder_partial_failure_and_confirmed_link_membership() {
        let mut app = app();
        let id = saved(&mut app);
        app.settings.profiles[0]
            .monitors
            .get_mut("b")
            .unwrap()
            .included = false;
        app.displays.swap(0, 1);
        app.apply_profile_at(id, 10, 12 * 60);
        assert_eq!(app.displays[0].info.id, "b");
        assert_eq!(app.displays[0].value, 35);
        assert!(
            app.settings.monitors["b"].included,
            "link changes wait for successful writes"
        );
        acknowledge(&mut app, 0, Ok(()));
        acknowledge(&mut app, 1, Err("Fake DDC failure".into()));
        assert_eq!(app.settings.monitors["b"].brightness, 35);
        assert!(!app.settings.monitors["b"].included);
        assert_eq!(app.settings.monitors["a"].brightness, 72);
        assert_eq!(app.displays[1].value, 72);
        let result = app.profile_application.as_ref().unwrap();
        assert_eq!(
            (result.applied(), result.skipped(), result.pending()),
            (1, 1, 0)
        );
    }

    #[test]
    fn profiles_skip_missing_unsupported_and_unrelated_displays_without_deferred_application() {
        let mut app = app();
        let id = saved(&mut app);
        app.settings.profiles[0].monitors.insert(
            "c".into(),
            profiles::Level {
                name: "Unsupported".into(),
                brightness: 10,
                included: true,
            },
        );
        let old_a = app.displays[0].info.clone();
        let b = app.displays[1].info.clone();
        let c = app.displays[2].info.clone();
        app.merge_displays(vec![
            MonitorInfo {
                id: "new".into(),
                name: "New".into(),
                detail: String::new(),
                brightness: Some(91),
            },
            b,
            c,
        ]);
        app.apply_profile_at(id, 10, 12 * 60);
        assert_eq!(app.displays[0].value, 91);
        assert!(app.displays[0].queued.is_none());
        let result = app.profile_application.as_ref().unwrap();
        assert_eq!(result.outcomes["a"].1, Outcome::Missing);
        assert_eq!(result.outcomes["c"].1, Outcome::Unsupported);
        let mut reconnect: Vec<_> = app.displays.iter().map(|d| d.info.clone()).collect();
        reconnect.push(old_a);
        app.merge_displays(reconnect);
        assert_eq!(
            app.displays.last().unwrap().value,
            72,
            "reconnection uses existing manual memory, not a skipped profile"
        );
    }

    #[test]
    fn manual_override_cancels_pending_profile_link_changes() {
        let mut app = app();
        let id = saved(&mut app);
        app.settings.profiles[0]
            .monitors
            .get_mut("a")
            .unwrap()
            .included = false;
        app.apply_profile_at(id, 10, 12 * 60);
        let revision = app.displays[0].revision;
        app.queue(0, 65, true);
        app.monitor_event(monitors::Event::Written {
            id: "a".into(),
            value: 35,
            revision,
            result: Ok(()),
        });
        assert_eq!(app.displays[0].value, 65);
        assert!(app.settings.monitors["a"].included);
        assert_eq!(
            app.profile_application.as_ref().unwrap().outcomes["a"].1,
            Outcome::Superseded
        );
    }

    #[test]
    fn applying_identical_profile_levels_still_sets_manual_memory_and_pauses_the_evening() {
        let mut app = app();
        app.settings.schedule.enabled = true;
        app.schedule_at(10, 21 * 60);
        acknowledge(&mut app, 0, Ok(()));
        acknowledge(&mut app, 1, Ok(()));
        assert_eq!(app.settings.monitors["a"].brightness, 72);
        app.save_current_profile("Night", None);
        let id = app.selected_profile.unwrap();
        let revision = app.revision;
        app.apply_profile_at(id, 10, 22 * 60);
        assert_eq!(app.paused_cycle, Some(10));
        assert_eq!(
            app.revision, revision,
            "matching confirmed targets require no extra writes"
        );
        assert_eq!(app.settings.monitors["a"].brightness, 25);
        app.schedule_at(11, 8 * 60);
        assert_eq!(app.displays[0].value, 25);
        assert!(app.settings.schedule.enabled);
    }

    #[test]
    fn profile_commands_coalesce_and_visual_animation_never_generates_ddc_writes() {
        let mut app = app();
        let id = saved(&mut app);
        let (commands, received) = std::sync::mpsc::channel();
        let (_, events) = std::sync::mpsc::channel();
        app.monitors = Some(monitors::Controller { commands, events });
        app.apply_profile_at(id, 10, 12 * 60);
        app.settings.profiles[0]
            .monitors
            .get_mut("a")
            .unwrap()
            .brightness = 15;
        app.apply_profile_at(id, 10, 12 * 60);
        app.flush(true);
        let commands: Vec<_> = received.try_iter().collect();
        assert_eq!(commands.len(), 2);
        assert!(matches!(&commands[0], Command::Set { id, value: 15, .. } if id == "a"));
        let revision = app.revision;
        let ctx = egui::Context::default();
        theme::initialize(&ctx, Theme::Dark);
        for frame in 0..25 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    time: Some(frame as f64 / 60.0),
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(360.0, 640.0),
                    )),
                    ..Default::default()
                },
                |root| {
                    egui::CentralPanel::default().show(root, |ui| app.displays_ui(ui));
                },
            );
            output.textures_delta.clear();
        }
        assert_eq!(app.revision, revision);
        assert_eq!(received.try_iter().count(), 0);
    }

    #[test]
    fn stopped_worker_finishes_profile_with_failure_and_keeps_previous_memory() {
        let mut app = app();
        let id = saved(&mut app);
        let (commands, receiver) = std::sync::mpsc::channel();
        drop(receiver);
        let (_, events) = std::sync::mpsc::channel();
        app.monitors = Some(monitors::Controller { commands, events });
        app.apply_profile_at(id, 10, 12 * 60);
        app.flush(true);
        let result = app.profile_application.as_ref().unwrap();
        assert_eq!(
            (result.pending(), result.applied(), result.skipped()),
            (0, 0, 2)
        );
        assert_eq!(app.settings.monitors["a"].brightness, 72);
    }

    #[test]
    fn profile_editor_and_popup_layout_are_safe_in_every_language_and_theme() {
        for language in Language::ALL {
            for selected_theme in [Theme::Light, Theme::Dark] {
                let mut app = app();
                saved(&mut app);
                app.settings.profiles[0].name = "W".repeat(40);
                app.profile_editor = None;
                let ctx = egui::Context::default();
                language.install(&ctx);
                theme::initialize(&ctx, selected_theme);
                for frame in 0..3 {
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            time: Some(frame as f64 / 60.0),
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(500.0, 760.0),
                            )),
                            ..Default::default()
                        },
                        |root| {
                            egui::CentralPanel::default().show(root, |ui| {
                                let width = ui.available_width();
                                app.profiles_ui(ui);
                                assert!(ui.min_rect().width() <= width + 1.0,
                                    "profile editor overflow: {language:?} {selected_theme:?} {} > {width}", ui.min_rect().width());
                            });
                        },
                    );
                    output.textures_delta.clear();
                }
                for frame in 3..6 {
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            time: Some(frame as f64 / 60.0),
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                Vec2::new(360.0, 640.0),
                            )),
                            ..Default::default()
                        },
                        |root| {
                            egui::CentralPanel::default().show(root, |ui| {
                                let width = ui.available_width();
                                app.quick_profiles_ui(ui);
                                app.displays_ui(ui);
                                assert!(
                                    ui.min_rect().width() <= width + 1.0,
                                    "popup overflow: {language:?} {selected_theme:?} {} > {width}",
                                    ui.min_rect().width()
                                );
                            });
                        },
                    );
                    output.textures_delta.clear();
                }
                assert_eq!(
                    app.revision, 0,
                    "headless layout must never change hardware"
                );
            }
        }
    }

    #[test]
    fn cosmetic_slider_frames_end_after_the_value_and_hover_settle() {
        for reduced in [false, true] {
            let ctx = egui::Context::default();
            theme::initialize(&ctx, Theme::Dark);
            theme::motion(&ctx, reduced);
            let mut value = 20;
            let mut animated_frames = 0;
            for frame in 0..40 {
                if frame == 2 {
                    value = 80;
                }
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        time: Some(frame as f64 / 60.0),
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(360.0, 100.0),
                        )),
                        ..Default::default()
                    },
                    |root| {
                        egui::CentralPanel::default().show(root, |ui| {
                            brightness_slider(ui, &mut value, 320.0, reduced);
                        });
                    },
                );
                let repaint = output.viewport_output[&egui::ViewportId::ROOT].repaint_delay;
                if frame > 2 && frame < 12 && repaint != Duration::MAX {
                    animated_frames += 1;
                }
                if frame >= 30 {
                    assert_eq!(
                        repaint,
                        Duration::MAX,
                        "settled widgets must stop requesting frames"
                    );
                }
                output.textures_delta.clear();
            }
            if !reduced {
                assert!(animated_frames > 0);
            }
            assert_eq!(
                value, 80,
                "visual interpolation must never modify the target"
            );
        }
    }
}
