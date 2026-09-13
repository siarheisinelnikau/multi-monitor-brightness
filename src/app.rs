use crate::{
    config,
    model::{self, Hotkey, Hotkeys, MonitorMemory, Settings},
    monitors::{self, Command, MonitorInfo},
    platform::{self, Action, HotkeyEvent},
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

const BG: Color32 = Color32::from_rgb(23, 24, 27);
const CARD: Color32 = Color32::from_rgb(32, 33, 38);
const BORDER: Color32 = Color32::from_rgb(51, 53, 60);
const ACCENT: Color32 = Color32::from_rgb(137, 167, 237);
const TRACK: Color32 = Color32::from_rgb(66, 68, 77);
const TEXT: Color32 = Color32::from_rgb(239, 239, 242);
const MUTED: Color32 = Color32::from_rgb(162, 164, 175);
const WARM: Color32 = Color32::from_rgb(224, 181, 132);

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
    Evening,
    Settings,
}

enum UpdateState {
    Idle,
    Checking,
    Current,
    Available(Box<updater::Release>),
    Downloading { received: u64, total: u64 },
    Installing,
    Error(String),
}
impl UpdateState {
    fn busy(&self) -> bool {
        matches!(
            self,
            Self::Checking | Self::Downloading { .. } | Self::Installing
        )
    }
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
    updates: Option<updater::Service>,
    update_state: UpdateState,
    prepared_update: Option<updater::Prepared>,
    started: Instant,
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
        theme(&cc.egui_ctx);
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
        let tray = match platform::Tray::new(cc.egui_ctx.clone()) {
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
            None
        } else {
            match updater::Service::new(cc.egui_ctx.clone(), portable) {
                Ok(service) => Some(service),
                Err(error) => {
                    notice = Some(error);
                    None
                }
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
            popup_focus_pending: popup_open,
            popup_anchor,
            popup_details: false,
            updates,
            update_state: UpdateState::Idle,
            prepared_update: None,
            started: Instant::now(),
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
    }

    fn receive(&mut self, ctx: &egui::Context) {
        let events: Vec<_> = self
            .monitors
            .as_ref()
            .map(|m| m.events.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            self.monitor_event(event);
        }
        let events: Vec<_> = self
            .hotkeys
            .as_ref()
            .map(|h| h.events.try_iter().collect())
            .unwrap_or_default();
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
        if self.update_state.busy() || self.demo {
            return;
        }
        if let Some(service) = &self.updates {
            service.check();
            self.update_state = UpdateState::Checking;
        } else {
            self.update_state =
                UpdateState::Error("Update service is unavailable. Restart the app.".into());
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
            ui.painter()
                .rect_filled(rect, 9, Color32::from_rgb(44, 48, 59));
            sun(ui.painter(), rect.center(), 9.0, ACCENT);
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.label(RichText::new("Preferences").size(21.0).strong());
                ui.label(
                    RichText::new("Multi-Monitor Brightness")
                        .color(MUTED)
                        .size(12.0),
                );
            });
        });
        ui.add_space(18.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (tab, label) in [(Tab::Evening, "Schedule"), (Tab::Settings, "Settings")] {
                let active = self.tab == tab;
                let response = ui.add(
                    egui::Button::new(RichText::new(label).size(13.0).color(if active {
                        TEXT
                    } else {
                        MUTED
                    }))
                    .fill(if active { CARD } else { Color32::TRANSPARENT })
                    .stroke(Stroke::NONE)
                    .corner_radius(6)
                    .min_size(Vec2::new(92.0, 32.0)),
                );
                if active {
                    let line = response.rect.shrink2(Vec2::new(13.0, 0.0));
                    ui.painter().line_segment(
                        [line.left_bottom(), line.right_bottom()],
                        Stroke::new(2.0, ACCENT),
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
                ui.label(RichText::new("All linked displays").size(12.0).color(MUTED));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("{average}%")).size(20.0));
                });
            });
            let mut value = average;
            let response = brightness_slider(ui, &mut value, ui.available_width());
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
                    ui.spinner();
                    ui.label("Finding displays…");
                });
            } else {
                ui.label("No displays detected");
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
                                    RichText::new(&self.displays[i].info.name).size(13.0),
                                )
                                .truncate(),
                            )
                        },
                    )
                    .inner
                    .on_hover_text(&self.displays[i].info.detail);
                    if supported > 1 || !self.included(i) {
                        let memory = self
                            .settings
                            .monitors
                            .entry(self.displays[i].info.id.clone())
                            .or_default();
                        if ui
                            .checkbox(
                                &mut memory.included,
                                RichText::new("Link").size(10.0).color(MUTED),
                            )
                            .on_hover_text(
                                "Include in shared controls, hotkeys and the evening schedule",
                            )
                            .changed()
                        {
                            self.dirty();
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(format!("{}%", self.displays[i].value)).size(20.0));
                    });
                });
                let mut value = self.displays[i].value;
                if brightness_slider(ui, &mut value, ui.available_width()).changed() {
                    self.pause_schedule();
                    self.queue(i, value, true);
                }
                if let Some(error) = &self.displays[i].error {
                    ui.label(RichText::new(error).size(11.0).color(WARM));
                }
                ui.add_space(10.0);
            });
        }
        let unavailable = self.displays.len() - supported;
        if unavailable > 0 {
            if ui
                .small_button(format!(
                    "{unavailable} {} unavailable",
                    if unavailable == 1 {
                        "display"
                    } else {
                        "displays"
                    }
                ))
                .clicked()
            {
                self.popup_details = !self.popup_details;
            }
            if self.popup_details {
                for d in self.displays.iter().filter(|d| d.info.brightness.is_none()) {
                    ui.label(RichText::new(&d.info.name).size(11.0).color(MUTED));
                }
                ui.label(RichText::new("Enable DDC/CI in the monitor's menu, then refresh. Some built-in screens and docks are not supported.").size(11.0).color(MUTED));
            }
        }
        if self.settings.schedule.enabled {
            ui.label(
                RichText::new(if self.paused_cycle.is_some() {
                    "Evening dimming paused until morning"
                } else {
                    "Evening dimming enabled"
                })
                .color(MUTED)
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
                    .fill(BG)
                    .stroke(Stroke::new(1.0, BORDER))
                    .inner_margin(16)
                    .corner_radius(10),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Brightness").size(17.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("×").on_hover_text("Close · Esc").clicked() {
                            self.hide_popup(&ctx);
                        }
                        if ui.small_button("Settings").clicked() {
                            self.open_preferences(Tab::Settings, &ctx);
                        }
                    });
                });
                ui.add_space(12.0);
                let footer = 32.0;
                egui::ScrollArea::vertical()
                    .max_height((ui.available_height() - footer).max(40.0))
                    .show(ui, |ui| self.displays_ui(ui));
                ui.add_space(4.0);
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !self.scanning,
                            egui::Button::new(RichText::new("Refresh").size(11.0)).frame(false),
                        )
                        .clicked()
                        && let Some(service) = &self.monitors
                    {
                        let _ = service.commands.send(Command::Refresh);
                        self.scanning = true;
                    }
                    if ui
                        .add(egui::Button::new(RichText::new("Schedule").size(11.0)).frame(false))
                        .clicked()
                    {
                        self.open_preferences(Tab::Evening, &ctx);
                    }
                    if matches!(self.update_state, UpdateState::Available(_))
                        && ui.small_button("Update available").clicked()
                    {
                        self.open_preferences(Tab::Settings, &ctx);
                    }
                    if self.demo {
                        ui.label(RichText::new("Preview").size(10.0).color(MUTED));
                    }
                });
            });
        #[cfg(feature = "ui-capture")]
        if !matches!(
            std::env::var("MMB_CAPTURE_TAB").as_deref(),
            Ok("settings" | "schedule")
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
        let mut height = 108.0 + supported.max(1) as f32 * if supported > 1 { 96.0 } else { 70.0 };
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
        ui.label(RichText::new("Evening schedule").size(20.0).strong());
        ui.add_space(5.0);
        ui.label(
            RichText::new("Lower brightness gradually, then restore it in the morning.")
                .color(MUTED),
        );
        ui.add_space(22.0);
        let previous = self.settings.schedule.clone();
        card(ui, |ui| {
            ui.checkbox(
                &mut self.settings.schedule.enabled,
                RichText::new("Smooth evening dimming").size(16.0).strong(),
            );
            ui.add_space(8.0);
            ui.label(
                RichText::new(
                    "Gradually dim linked displays. Your usual brightness returns in the morning.",
                )
                .color(MUTED)
                .size(13.0),
            );
            ui.add_space(20.0);
            egui::Grid::new("schedule")
                .num_columns(2)
                .spacing([28.0, 18.0])
                .show(ui, |ui| {
                    ui.label("Start dimming");
                    time_editor(ui, &mut self.settings.schedule.start, "evening_start");
                    ui.end_row();
                    ui.label("Restore brightness");
                    time_editor(ui, &mut self.settings.schedule.morning, "morning_start");
                    ui.end_row();
                    ui.label("Fade duration");
                    ui.add(
                        egui::DragValue::new(&mut self.settings.schedule.duration)
                            .range(1..=240)
                            .suffix(" min"),
                    );
                    ui.end_row();
                    ui.label("Evening brightness");
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
                RichText::new(
                    "Choose different evening and morning times to activate the schedule.",
                )
                .color(WARM),
            );
        }
        ui.add_space(16.0);
        ui.label(RichText::new("Manual adjustments").size(13.0).strong());
        ui.label(RichText::new("Changing brightness pauses the schedule until morning. Displays below the target stay at their lower level. Times follow your computer's local clock.").color(MUTED).size(12.0));
        if self.paused_cycle.is_some() {
            ui.add_space(8.0);
            if ui.button("Resume evening dimming").clicked() {
                self.paused_cycle = None;
            }
        }
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Settings").size(20.0).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("Version {}", crate::VERSION))
                        .size(12.0)
                        .color(MUTED),
                );
            });
        });
        ui.add_space(18.0);
        self.updates_ui(ui);
        ui.add_space(12.0);
        card(ui, |ui| {
            ui.label(RichText::new("Application").size(17.0).strong());
            ui.add_space(10.0);
            let mut enabled = self.startup_status.enabled;
            if ui
                .add_enabled(
                    !self.demo,
                    egui::Checkbox::new(&mut enabled, "Start with Windows"),
                )
                .on_hover_text("Start quietly in the system tray when you sign in")
                .changed()
            {
                self.set_startup(enabled);
            }
            ui.label(
                RichText::new("Starts in the tray when you sign in.")
                    .size(11.0)
                    .color(MUTED),
            );
            if self.startup_status.enabled && !self.startup_status.uses_current_copy {
                ui.label(
                    RichText::new("Autostart currently uses another copy of the app.")
                        .size(12.0)
                        .color(MUTED),
                );
                if ui.button("Use this executable").clicked() {
                    self.set_startup(true);
                }
            }
            if let Some(error) = &self.startup_error {
                ui.label(RichText::new(error).size(12.0).color(WARM));
            }
            ui.add_space(6.0);
            if ui
                .checkbox(
                    &mut self.settings.restore_on_start,
                    "Restore each display's brightness on launch / reconnect",
                )
                .changed()
            {
                self.dirty();
            }
            ui.add_space(12.0);
            if let Some(path) = &self.config_path {
                ui.label(
                    RichText::new("Settings are saved automatically")
                        .size(12.0)
                        .color(MUTED),
                );
                ui.label(
                    RichText::new(path.display().to_string())
                        .size(11.0)
                        .color(MUTED),
                );
            } else if self.demo {
                ui.label(
                    RichText::new("Demo settings are temporary.")
                        .size(12.0)
                        .color(MUTED),
                );
            }
            if self.save_blocked {
                ui.label(RichText::new("Saving is paused to preserve your original settings. Fix or move the settings file and restart.").color(WARM).size(12.0));
            }
        });
        ui.add_space(12.0);
        card(ui, |ui| {
            ui.label(RichText::new("Keyboard shortcuts").size(17.0).strong());
            ui.add_space(10.0);
            ui.checkbox(&mut self.hotkey_draft.enabled, "Enable global shortcuts");
            ui.add_space(10.0);
            hotkey_editor(
                ui,
                "Increase brightness",
                "key_up",
                &mut self.hotkey_draft.up,
            );
            hotkey_editor(
                ui,
                "Decrease brightness",
                "key_down",
                &mut self.hotkey_draft.down,
            );
            hotkey_editor(
                ui,
                "Open brightness",
                "key_show",
                &mut self.hotkey_draft.show,
            );
            ui.add_space(13.0);
            ui.horizontal(|ui| {
                ui.label("Brightness step");
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
                    egui::Button::new("Apply shortcuts"),
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
                ("Shortcuts are preview-only in demo mode.", MUTED)
            } else if let Some(error) = self.shortcut_error.as_deref() {
                (error, WARM)
            } else if self.shortcuts_pending {
                ("Registering shortcuts…", MUTED)
            } else if self.hotkeys.is_none() {
                (
                    "Shortcut service unavailable. Restart the application.",
                    WARM,
                )
            } else if self.settings.hotkeys.enabled {
                (
                    "Shortcuts are active, including while the window is hidden.",
                    MUTED,
                )
            } else {
                ("Global shortcuts are disabled.", MUTED)
            };
            ui.label(RichText::new(text).size(12.0).color(color));
        });
        ui.add_space(15.0);
        ui.label(RichText::new("MIT License").color(MUTED).size(12.0));
    }
}

impl MultiMonitorBrightness {
    fn tick(&mut self, ctx: &egui::Context) {
        self.receive(ctx);
        let updates: Vec<_> = self
            .updates
            .as_ref()
            .map(|s| s.events.try_iter().collect())
            .unwrap_or_default();
        for event in updates {
            match event {
                updater::Event::Checked(result) => {
                    self.settings.last_update_check = updater::now();
                    self.dirty();
                    self.update_state = match result {
                        Ok(Some(release)) => UpdateState::Available(release),
                        Ok(None) => UpdateState::Current,
                        Err(e) => UpdateState::Error(e),
                    };
                }
                updater::Event::Progress { received, total } => {
                    self.update_state = UpdateState::Downloading { received, total }
                }
                updater::Event::Ready(result) => {
                    if self.quitting.is_some() {
                        continue;
                    }
                    match result {
                        Ok(prepared) => {
                            self.prepared_update = Some(prepared);
                            self.update_state = UpdateState::Installing;
                            self.quitting = Some(Instant::now());
                            self.popup_open = false;
                        }
                        Err(error) => self.update_state = UpdateState::Error(error),
                    }
                }
            }
        }
        if !self.demo
            && self.quitting.is_none()
            && self.settings.check_updates_automatically
            && self.started.elapsed() > Duration::from_secs(5)
            && matches!(self.update_state, UpdateState::Idle)
        {
            self.check_updates();
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
                self.update_state = UpdateState::Error(error);
                self.quitting = None;
                return;
            }
            ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
        }
        // A quiet 1 Hz timer keeps schedules running while hidden; active drags get faster feedback.
        let pending = self
            .displays
            .iter()
            .any(|d| d.queued.is_some() || d.writing);
        ctx.request_repaint_after_for(
            if pending || self.quitting.is_some() {
                Duration::from_millis(60)
            } else {
                Duration::from_secs(1)
            },
            egui::ViewportId::ROOT,
        );
    }
}

impl MultiMonitorBrightness {
    fn updates_ui(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Updates").size(17.0).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(
                            !self.demo && !self.update_state.busy(),
                            egui::Button::new("Check for updates"),
                        )
                        .clicked()
                    {
                        self.check_updates();
                    }
                });
            });
            ui.add_space(8.0);
            match &self.update_state {
                UpdateState::Idle => {
                    ui.label(
                        RichText::new(if self.demo {
                            "Update checks are disabled in preview mode."
                        } else {
                            "Updates are downloaded from the official GitHub releases."
                        })
                        .size(12.0)
                        .color(MUTED),
                    );
                }
                UpdateState::Checking => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Checking GitHub…");
                    });
                }
                UpdateState::Current => {
                    ui.label(RichText::new("You're up to date.").size(13.0).color(MUTED));
                }
                UpdateState::Available(release) => {
                    let release = release.clone();
                    ui.label(format!("Version {} is available", release.version));
                    ui.horizontal(|ui| {
                        if ui.button("Download and install").clicked()
                            && let Some(service) = &self.updates
                        {
                            service.download(release.clone());
                            self.update_state = UpdateState::Downloading {
                                received: 0,
                                total: 0,
                            };
                        }
                        ui.hyperlink_to("Release notes on GitHub", &release.page);
                    });
                    ui.label(
                        RichText::new("The app will restart. Your settings will be kept.")
                            .size(11.0)
                            .color(MUTED),
                    );
                    if !release.notes.is_empty() {
                        ui.collapsing("What's new", |ui| {
                            ui.label(&release.notes);
                        });
                    }
                }
                UpdateState::Downloading { received, total } => {
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
                    ui.label(
                        RichText::new("Downloading and verifying SHA-256…")
                            .size(12.0)
                            .color(MUTED),
                    );
                }
                UpdateState::Installing => {
                    ui.label("Restarting to finish the update…");
                }
                UpdateState::Error(error) => {
                    ui.label(RichText::new(error).size(12.0).color(WARM));
                }
            }
            ui.add_space(8.0);
            if ui
                .add_enabled(
                    !self.demo,
                    egui::Checkbox::new(
                        &mut self.settings.check_updates_automatically,
                        "Check for updates on startup",
                    ),
                )
                .changed()
            {
                self.dirty();
            }
        });
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

    fn ui(&mut self, root: &mut egui::Ui) {
        let ctx = root.ctx().clone();
        let builder = {
            let mut app = self.0.borrow_mut();
            app.ui(root);
            (app.popup_open && app.quitting.is_none()).then(|| {
                let (position, size) =
                    platform::popup_geometry(app.popup_anchor, app.popup_size(), &ctx);
                egui::ViewportBuilder::default()
                    .with_title(platform::POPUP_TITLE)
                    .with_visible(true)
                    .with_decorations(false)
                    .with_resizable(false)
                    .with_taskbar(false)
                    .with_active(true)
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
                    .fill(BG)
                    .inner_margin(egui::Margin::symmetric(24, 11)),
            )
            .show(root, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(if self.quitting.is_some() {
                            "Finishing brightness changes…"
                        } else if self.demo {
                            "Preview · Simulated displays"
                        } else {
                            "Changes are saved automatically"
                        })
                        .size(11.0)
                        .color(MUTED),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("Quit").clicked() {
                            self.action(Action::Quit, ctx);
                        }
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BG)
                    .inner_margin(egui::Margin::symmetric(24, 22)),
            )
            .show(root, |ui| {
                self.header(ui);
                if let Some(notice) = self.notice.clone() {
                    egui::Frame::new()
                        .fill(Color32::from_rgb(45, 39, 34))
                        .inner_margin(12)
                        .corner_radius(8)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(notice).color(WARM).size(12.0));
                                if !self.save_blocked && ui.small_button("Dismiss").clicked() {
                                    self.notice = None;
                                }
                            });
                        });
                    ui.add_space(12.0);
                }
                if self.shortcut_error.is_some() && self.tab != Tab::Settings {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("A keyboard shortcut is unavailable.")
                                .color(WARM)
                                .size(12.0),
                        );
                        if ui.small_button("Fix in Settings").clicked() {
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
                            Tab::Evening => self.evening_ui(ui),
                            Tab::Settings => self.settings_ui(ui),
                        }
                    });
            });
        #[cfg(feature = "ui-capture")]
        if matches!(
            std::env::var("MMB_CAPTURE_TAB").as_deref(),
            Ok("settings" | "schedule")
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

fn theme(ctx: &egui::Context) {
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
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = CARD;
    style.visuals.extreme_bg_color = BG;
    style.visuals.faint_bg_color = CARD;
    style.visuals.hyperlink_color = ACCENT;
    style.visuals.selection.bg_fill = Color32::from_rgb(51, 66, 96);
    style.visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(43, 44, 51);
    style.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(43, 44, 51);
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, MUTED);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(53, 55, 64);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(53, 55, 64);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(81, 86, 103));
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(58, 66, 88);
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.2, TEXT);
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
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_style_of(egui::Theme::Dark, style);
}

fn card(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(9)
        .inner_margin(16)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            contents(ui);
        });
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

fn brightness_slider(ui: &mut egui::Ui, value: &mut u8, width: f32) -> egui::Response {
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
    response.widget_info(|| egui::WidgetInfo::slider(ui.is_enabled(), *value as f64, "Brightness"));
    let y = track.center().y;
    let x = egui::lerp(track.x_range(), *value as f32 / 100.0);
    ui.painter().line_segment(
        [egui::pos2(track.left(), y), egui::pos2(track.right(), y)],
        Stroke::new(4.0, TRACK),
    );
    ui.painter().line_segment(
        [egui::pos2(track.left(), y), egui::pos2(x, y)],
        Stroke::new(4.0, ACCENT),
    );
    ui.painter().circle_filled(
        egui::pos2(x, y),
        if response.dragged() { 7.0 } else { 6.0 },
        TEXT,
    );
    if response.has_focus() {
        ui.painter()
            .circle_stroke(egui::pos2(x, y), 10.0, Stroke::new(1.0, ACCENT));
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
                egui::Label::new(RichText::new(label).size(12.0).color(MUTED)),
            );
            ui.checkbox(&mut key.ctrl, "Ctrl");
            ui.checkbox(&mut key.alt, "Alt");
            ui.checkbox(&mut key.shift, "Shift");
            egui::ComboBox::from_id_salt("key")
                .width(96.0)
                .selected_text(&key.key)
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
                        ui.selectable_value(&mut key.key, name.into(), name);
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
        Stroke::new(1.0, BORDER),
    );
    let points = vec![from, to];
    ui.painter()
        .add(egui::Shape::line(points, Stroke::new(2.0, ACCENT)));
    ui.painter().text(
        rect.left_bottom(),
        egui::Align2::LEFT_BOTTOM,
        format!("{:02}:{:02}", schedule.start / 60, schedule.start % 60),
        FontId::proportional(11.0),
        MUTED,
    );
    ui.painter().text(
        rect.right_bottom(),
        egui::Align2::RIGHT_BOTTOM,
        format!("{} min → {}%", schedule.duration, schedule.target),
        FontId::proportional(11.0),
        MUTED,
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
            updates: None,
            update_state: UpdateState::Idle,
            prepared_update: None,
            started: Instant::now(),
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
    fn startup_update_requests_respect_opt_out_and_manual_checks_remain_available() {
        let mut app = app();
        app.demo = false;
        app.started = Instant::now() - Duration::from_secs(6);
        app.settings.check_updates_automatically = false;
        let (service, requests) = updater::Service::test_channel();
        app.updates = Some(service);
        let ctx = egui::Context::default();
        app.tick(&ctx);
        assert_eq!(
            requests(),
            0,
            "disabled startup checks must make no request"
        );
        app.check_updates();
        assert_eq!(requests(), 1, "manual checks remain available");
        app.update_state = UpdateState::Idle;
        app.settings.check_updates_automatically = true;
        app.settings.last_update_check = updater::now();
        app.tick(&ctx);
        assert_eq!(
            requests(),
            1,
            "each launch checks even after a recent result"
        );
        app.tick(&ctx);
        assert_eq!(requests(), 0, "pending checks must not be duplicated");
    }

    #[test]
    fn rendering_controls_does_not_change_brightness() {
        let mut app = app();
        let ctx = egui::Context::default();
        theme(&ctx);
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
}
