//! Live-window regression: use the installed tray menu callback while the real HWND is hidden.
//! Opt-in only. Simulated monitors ensure this never changes physical screen brightness.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Stage {
    Ready,
    PopupOpened,
    ToggleClosed,
    ToggleOpened,
    Reopened,
    Settings,
    Schedule,
    Hidden,
    Up,
    Down,
    Minimized,
    Restored,
    HiddenAgain,
    Scheduled,
    Morning,
    Quit,
}

pub struct SmokeTest {
    observe: Box<dyn Fn() -> (bool, bool)>,
    report: PathBuf,
    started: Instant,
    stage: Stage,
    baseline: Vec<(String, u8)>,
    passed: Vec<&'static str>,
    started_hidden: bool,
}

impl SmokeTest {
    pub fn new(cc: &crate::host::CreationContext, demo: bool) -> Option<Self> {
        let report = std::env::var_os("MMB_TRAY_SMOKE_REPORT")?;
        assert!(demo, "Tray regression must be launched with --demo");
        let window = cc.window.clone();
        Some(Self {
            observe: Box::new(move || {
                (
                    window.is_visible().unwrap_or(false),
                    window.is_minimized().unwrap_or(false),
                )
            }),
            report: report.into(),
            started: Instant::now(),
            stage: Stage::Ready,
            baseline: Vec::new(),
            passed: Vec::new(),
            started_hidden: std::env::args().any(|arg| arg == "--startup"),
        })
    }

    pub fn tick(&mut self, app: &mut MultiMonitorBrightness, ctx: &egui::Context) {
        if self.started.elapsed() > Duration::from_secs(15) {
            eprintln!(
                "Tray regression timed out at {:?}; passed: {:?}",
                self.stage, self.passed
            );
            std::process::exit(1);
        }
        let (preferences, minimized) = (self.observe)();
        let (visible, _) = platform::popup_window_state();
        let settled = !app.displays.iter().any(|d| d.queued.is_some() || d.writing);
        let matches = |delta| {
            self.baseline.iter().all(|(id, value)| {
                app.displays
                    .iter()
                    .any(|d| &d.info.id == id && d.value == model::adjusted(*value, delta))
            })
        };
        let dispatch = |action| {
            app.tray
                .as_ref()
                .expect("system tray")
                .dispatch_menu_for_test(action)
        };
        match self.stage {
            Stage::Ready
                if !app.scanning && self.started.elapsed() > Duration::from_millis(600) =>
            {
                assert!(
                    !preferences,
                    "Preferences must not open for a brightness tray click"
                );
                self.baseline = app
                    .displays
                    .iter()
                    .filter(|d| d.info.brightness.is_some())
                    .map(|d| (d.info.id.clone(), d.value))
                    .collect();
                assert_eq!(self.baseline.len(), 2);
                if self.started_hidden {
                    assert!(!visible);
                    self.pass("startup_stays_in_tray");
                    dispatch(Action::Show);
                }
                self.stage = Stage::PopupOpened;
            }
            Stage::PopupOpened if visible && !preferences => {
                self.pass("tray_opens_brightness_popup_only");
                app.tray.as_ref().unwrap().dispatch_click_for_test();
                self.stage = Stage::ToggleClosed;
            }
            Stage::ToggleClosed if !visible && !preferences => {
                self.pass("left_click_closes_popup");
                app.tray.as_ref().unwrap().dispatch_click_for_test();
                self.stage = Stage::ToggleOpened;
            }
            Stage::ToggleOpened if visible && !preferences => {
                self.pass("left_click_opens_popup");
                app.hide_popup(ctx);
                self.stage = Stage::Hidden;
            }
            Stage::Hidden if !visible && !preferences => {
                self.pass("close_to_tray");
                dispatch(Action::Up);
                self.stage = Stage::Up;
            }
            Stage::Up if settled && matches(app.settings.step as i16) => {
                assert!(!visible, "brightness commands must not reveal the window");
                self.pass("brightness_up_while_hidden");
                dispatch(Action::Down);
                self.stage = Stage::Down;
            }
            Stage::Down if settled && matches(0) => {
                assert!(!visible);
                self.pass("brightness_down_while_hidden");
                dispatch(Action::Show);
                self.stage = Stage::Reopened;
            }
            Stage::Reopened if visible && !preferences => {
                self.pass("popup_reopens_from_tray");
                dispatch(Action::Settings);
                self.stage = Stage::Settings;
            }
            Stage::Settings if preferences && !visible => {
                assert!(app.tab == Tab::Settings);
                self.pass("settings_in_separate_window");
                dispatch(Action::Schedule);
                self.stage = Stage::Schedule;
            }
            Stage::Schedule if preferences && app.tab == Tab::Evening => {
                self.pass("schedule_in_preferences_window");
                ctx.send_viewport_cmd_to(
                    egui::ViewportId::ROOT,
                    egui::ViewportCommand::Minimized(true),
                );
                self.stage = Stage::Minimized;
            }
            Stage::Minimized if minimized => {
                self.pass("minimize_preferences");
                dispatch(Action::Settings);
                self.stage = Stage::Restored;
            }
            Stage::Restored if preferences && !minimized => {
                self.pass("restore_preferences_from_tray");
                ctx.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
                self.stage = Stage::HiddenAgain;
            }
            Stage::HiddenAgain if !visible && !preferences => {
                self.pass("close_to_tray_again");
                let (_, minute) = platform::local_clock();
                app.settings.schedule = model::Schedule {
                    enabled: true,
                    start: (minute + 1439) % 1440,
                    morning: (minute + 60) % 1440,
                    duration: 1,
                    target: 25,
                };
                app.last_schedule = Instant::now() - Duration::from_secs(2);
                self.stage = Stage::Scheduled;
            }
            Stage::Scheduled
                if settled
                    && app
                        .displays
                        .iter()
                        .filter(|d| d.info.brightness.is_some())
                        .all(|d| d.value == 25) =>
            {
                assert!(!visible);
                for (id, value) in &self.baseline {
                    assert_eq!(app.settings.monitors[id].brightness, *value);
                }
                self.pass("evening_schedule_while_hidden");
                app.settings.schedule.enabled = false;
                app.last_schedule = Instant::now() - Duration::from_secs(2);
                self.stage = Stage::Morning;
            }
            Stage::Morning if settled && matches(0) => {
                assert!(!visible);
                self.pass("schedule_restores_manual_levels_while_hidden");
                dispatch(Action::Quit);
                self.stage = Stage::Quit;
            }
            _ => {}
        }
        ctx.request_repaint_after(Duration::from_millis(40));
    }

    fn pass(&mut self, step: &'static str) {
        eprintln!("PASS {step}");
        self.passed.push(step);
    }

    pub fn finish(&self) {
        assert_eq!(
            self.stage,
            Stage::Quit,
            "unexpected application exit during tray regression"
        );
        let mut steps = self.passed.clone();
        steps.push("quit_while_hidden");
        let report = serde_json::json!({ "status": "passed", "steps": steps, "hardware": "simulated; no physical brightness changes" });
        std::fs::write(&self.report, serde_json::to_vec_pretty(&report).unwrap())
            .expect("write tray regression report");
        eprintln!("PASS quit_while_hidden");
    }
}
