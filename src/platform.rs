use crate::model::Hotkeys;
use std::{ptr, sync::mpsc, thread, time::Duration};
use windows_sys::Win32::{
    Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, FILETIME, GetLastError, HANDLE, SYSTEMTIME},
    System::{
        SystemInformation::GetLocalTime, Threading::CreateMutexW, Time::SystemTimeToFileTime,
    },
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub fn message(text: &str) {
    // SAFETY: Strings remain alive for the duration of the modal call.
    unsafe {
        MessageBoxW(
            ptr::null_mut(),
            wide(text).as_ptr(),
            wide("Multi-Monitor Brightness").as_ptr(),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

pub struct Instance(HANDLE);

impl Instance {
    pub fn acquire(demo: bool) -> Result<Option<Self>, String> {
        let name = wide(if demo {
            "Local\\MultiMonitorBrightness.Demo"
        } else {
            "Local\\MultiMonitorBrightness.App"
        });
        // SAFETY: No security descriptor; the name is NUL-terminated.
        let handle = unsafe { CreateMutexW(ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return Err(format!(
                "Could not create application lock: {}",
                std::io::Error::last_os_error()
            ));
        }
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            unsafe {
                CloseHandle(handle);
            }
            return Ok(None);
        }
        Ok(Some(Self(handle)))
    }
}

impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    Show,
    TogglePopup(Anchor),
    Settings,
    Schedule,
    CheckUpdates,
    Quit,
}

pub const PREFERENCES_TITLE: &str = "Multi-Monitor Brightness — Preferences";
pub const POPUP_TITLE: &str = "Multi-Monitor Brightness — Brightness";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl From<tray_icon::Rect> for Anchor {
    fn from(rect: tray_icon::Rect) -> Self {
        Self {
            left: rect.position.x as i32,
            top: rect.position.y as i32,
            right: (rect.position.x + rect.size.width as f64) as i32,
            bottom: (rect.position.y + rect.size.height as f64) as i32,
        }
    }
}

pub fn cursor_in(anchor: Anchor) -> bool {
    let mut point = windows_sys::Win32::Foundation::POINT::default();
    unsafe {
        GetCursorPos(&mut point) != 0
            && point.x >= anchor.left
            && point.x <= anchor.right
            && point.y >= anchor.top
            && point.y <= anchor.bottom
    }
}

pub fn popup_position(
    anchor: Option<Anchor>,
    size: eframe::egui::Vec2,
    ctx: &eframe::egui::Context,
) -> eframe::egui::Pos2 {
    use windows_sys::Win32::{Foundation::POINT, Graphics::Gdi::*, UI::HiDpi::*};
    let mut point = POINT::default();
    unsafe {
        GetCursorPos(&mut point);
    }
    if let Some(a) = anchor {
        point = POINT {
            x: (a.left + a.right) / 2,
            y: (a.top + a.bottom) / 2,
        };
    }
    let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    let mut dpi_x = 96;
    let mut dpi_y = 96;
    // SAFETY: The monitor comes from Windows and output structs have their documented layouts.
    unsafe {
        GetMonitorInfoW(monitor, &mut info);
        GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y);
    }
    let work = info.rcWork;
    let scale = dpi_x.max(96) as f32 / 96.0;
    let anchor = anchor.unwrap_or(Anchor {
        left: work.right - 30,
        top: work.bottom + 8,
        right: work.right - 8,
        bottom: work.bottom + 30,
    });
    let (x, y) = place_popup(
        [work.left, work.top, work.right, work.bottom],
        anchor,
        (size.x * scale) as i32,
        (size.y * scale) as i32,
    );
    let current_scale = ctx.native_pixels_per_point().unwrap_or(scale);
    eframe::egui::pos2(x as f32 / current_scale, y as f32 / current_scale)
}

fn place_popup(work: [i32; 4], anchor: Anchor, width: i32, height: i32) -> (i32, i32) {
    let x = ((anchor.left + anchor.right - width) / 2)
        .clamp(work[0] + 8, (work[2] - width - 8).max(work[0] + 8));
    let above = anchor.top - height - 8;
    let y = if above >= work[1] + 8 {
        above
    } else {
        anchor.bottom + 8
    };
    (
        x,
        y.clamp(work[1] + 8, (work[3] - height - 8).max(work[1] + 8)),
    )
}

#[cfg(feature = "tray-smoke-test")]
pub fn popup_window_state() -> (bool, bool) {
    unsafe extern "system" fn find(
        window: windows_sys::Win32::Foundation::HWND,
        data: isize,
    ) -> i32 {
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(window, &mut pid);
        }
        if pid != std::process::id() {
            return 1;
        }
        let mut text = [0u16; 256];
        let n = unsafe { GetWindowTextW(window, text.as_mut_ptr(), text.len() as i32) };
        if String::from_utf16_lossy(&text[..n.max(0) as usize]) == POPUP_TITLE {
            unsafe {
                *(data as *mut (bool, bool)) =
                    (IsWindowVisible(window) != 0, IsIconic(window) != 0);
            }
        }
        1
    }
    let mut state = (false, false);
    unsafe {
        EnumWindows(Some(find), &mut state as *mut _ as isize);
    }
    state
}

pub enum HotkeyEvent {
    Action(Action),
    Registered(Result<(), String>),
}
enum HotkeyCommand {
    Configure(Hotkeys),
    Shutdown,
}

pub struct HotkeyService {
    commands: mpsc::Sender<HotkeyCommand>,
    pub events: mpsc::Receiver<HotkeyEvent>,
}

impl HotkeyService {
    pub fn new(ctx: eframe::egui::Context) -> Result<Self, String> {
        let (commands, receiver) = mpsc::channel();
        let (sender, events) = mpsc::channel();
        thread::Builder::new()
            .name("global-hotkeys".into())
            .spawn(move || {
                let emit = |event| {
                    let _ = sender.send(event);
                    ctx.request_repaint();
                };
                loop {
                    match receiver.recv_timeout(Duration::from_millis(30)) {
                        Ok(HotkeyCommand::Configure(hotkeys)) => {
                            unregister();
                            emit(HotkeyEvent::Registered(register(&hotkeys)));
                        }
                        Ok(HotkeyCommand::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                            break;
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                    let mut msg = MSG::default();
                    // SAFETY: Only this thread registers these thread-scoped hotkeys and drains its queue.
                    while unsafe {
                        PeekMessageW(&mut msg, ptr::null_mut(), WM_HOTKEY, WM_HOTKEY, PM_REMOVE)
                    } != 0
                    {
                        let action = match msg.wParam {
                            1 => Action::Up,
                            2 => Action::Down,
                            3 => Action::Show,
                            _ => continue,
                        };
                        emit(HotkeyEvent::Action(action));
                    }
                }
                unregister();
            })
            .map_err(|e| format!("Could not start shortcuts: {e}"))?;
        Ok(Self { commands, events })
    }

    pub fn configure(&self, hotkeys: &Hotkeys) {
        let _ = self
            .commands
            .send(HotkeyCommand::Configure(hotkeys.clone()));
    }
}

impl Drop for HotkeyService {
    fn drop(&mut self) {
        let _ = self.commands.send(HotkeyCommand::Shutdown);
    }
}

fn unregister() {
    for id in 1..=3 {
        unsafe {
            UnregisterHotKey(ptr::null_mut(), id);
        }
    }
}

fn register(hotkeys: &Hotkeys) -> Result<(), String> {
    hotkeys.validate()?;
    if !hotkeys.enabled {
        return Ok(());
    }
    for (index, key) in [&hotkeys.up, &hotkeys.down, &hotkeys.show]
        .iter()
        .enumerate()
    {
        let modifiers = if key.ctrl { MOD_CONTROL } else { 0 }
            | if key.alt { MOD_ALT } else { 0 }
            | if key.shift { MOD_SHIFT } else { 0 }
            | MOD_NOREPEAT;
        // SAFETY: Thread-scoped registration; the message queue is drained by this same thread.
        if unsafe {
            RegisterHotKey(
                ptr::null_mut(),
                index as i32 + 1,
                modifiers,
                key.virtual_key().unwrap(),
            )
        } == 0
        {
            let error = std::io::Error::last_os_error();
            unregister(); // Avoid leaving only some of the configured keys registered.
            return Err(format!(
                "Could not register {} ({error}). Choose another shortcut and apply again.",
                key.label()
            ));
        }
    }
    Ok(())
}

pub fn local_clock() -> (i64, u16) {
    let mut time = SYSTEMTIME::default();
    let mut file = FILETIME::default();
    // Treat the local wall-clock date as UTC only to obtain a consecutive calendar-day key.
    // SAFETY: Both output structs are valid and have the exact Windows ABI layout.
    unsafe {
        GetLocalTime(&mut time);
        SystemTimeToFileTime(&time, &mut file);
    }
    let ticks = (file.dwHighDateTime as u64) << 32 | file.dwLowDateTime as u64;
    (
        (ticks / 864_000_000_000) as i64,
        time.wHour * 60 + time.wMinute,
    )
}

pub struct Tray {
    _icon: tray_icon::TrayIcon,
    pub events: mpsc::Receiver<Action>,
    #[cfg(feature = "tray-smoke-test")]
    menu_handler: std::sync::Arc<dyn Fn(tray_icon::menu::MenuEvent) + Send + Sync>,
    #[cfg(feature = "tray-smoke-test")]
    menu_ids: [tray_icon::menu::MenuId; 7],
    #[cfg(feature = "tray-smoke-test")]
    click_handler: std::sync::Arc<dyn Fn(tray_icon::TrayIconEvent) + Send + Sync>,
}

impl Tray {
    pub fn new(ctx: eframe::egui::Context) -> Result<Self, String> {
        use tray_icon::{
            Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent,
            menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
        };
        let show = MenuItem::new("Brightness", true, None);
        let settings = MenuItem::new("Settings", true, None);
        let schedule = MenuItem::new("Schedule", true, None);
        let updates = MenuItem::new("Check for updates", true, None);
        let up = MenuItem::new("Brightness +", true, None);
        let down = MenuItem::new("Brightness -", true, None);
        let quit = MenuItem::new("Quit Multi-Monitor Brightness", true, None);
        let menu = Menu::with_items(&[
            &show,
            &settings,
            &schedule,
            &updates,
            &PredefinedMenuItem::separator(),
            &up,
            &down,
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .map_err(|e| e.to_string())?;
        let ids = [
            show.id().clone(),
            settings.id().clone(),
            schedule.id().clone(),
            updates.id().clone(),
            up.id().clone(),
            down.id().clone(),
            quit.id().clone(),
        ];
        #[cfg(feature = "tray-smoke-test")]
        let menu_ids = ids.clone();
        let (sender, events) = mpsc::channel();
        let menu_sender = sender.clone();
        let menu_ctx = ctx.clone();
        let menu_handler: std::sync::Arc<dyn Fn(MenuEvent) + Send + Sync> =
            std::sync::Arc::new(move |event: MenuEvent| {
                let action = if event.id == ids[0] {
                    Action::Show
                } else if event.id == ids[1] {
                    Action::Settings
                } else if event.id == ids[2] {
                    Action::Schedule
                } else if event.id == ids[3] {
                    Action::CheckUpdates
                } else if event.id == ids[4] {
                    Action::Up
                } else if event.id == ids[5] {
                    Action::Down
                } else if event.id == ids[6] {
                    Action::Quit
                } else {
                    return;
                };
                let _ = menu_sender.send(action);
                menu_ctx.request_repaint();
            });
        let handler = menu_handler.clone();
        MenuEvent::set_event_handler(Some(move |event| handler(event)));
        let click_handler: std::sync::Arc<dyn Fn(TrayIconEvent) + Send + Sync> =
            std::sync::Arc::new(move |event| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    rect,
                    ..
                } = event
                {
                    let _ = sender.send(Action::TogglePopup(rect.into()));
                    ctx.request_repaint();
                }
            });
        let handler = click_handler.clone();
        TrayIconEvent::set_event_handler(Some(move |event| handler(event)));
        let icon = Icon::from_rgba(crate::icon::rgba(32), 32, 32).map_err(|e| e.to_string())?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip("Multi-Monitor Brightness")
            .with_icon(icon)
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            _icon: tray,
            events,
            #[cfg(feature = "tray-smoke-test")]
            menu_handler,
            #[cfg(feature = "tray-smoke-test")]
            menu_ids,
            #[cfg(feature = "tray-smoke-test")]
            click_handler,
        })
    }

    pub fn anchor(&self) -> Option<Anchor> {
        self._icon.rect().map(Into::into)
    }

    #[cfg(feature = "tray-smoke-test")]
    pub fn dispatch_click_for_test(&self) {
        let rect = self._icon.rect().expect("tray icon rectangle");
        (self.click_handler)(tray_icon::TrayIconEvent::Click {
            id: self._icon.id().clone(),
            position: rect.position,
            rect,
            button: tray_icon::MouseButton::Left,
            button_state: tray_icon::MouseButtonState::Up,
        });
    }

    /// Exercise the exact installed menu callback and IDs in a live-window regression test.
    #[cfg(feature = "tray-smoke-test")]
    pub fn dispatch_menu_for_test(&self, action: Action) {
        let index = match action {
            Action::Show => 0,
            Action::Settings => 1,
            Action::Schedule => 2,
            Action::CheckUpdates => 3,
            Action::Up => 4,
            Action::Down => 5,
            Action::Quit => 6,
            Action::TogglePopup(_) => 0,
        };
        (self.menu_handler)(tray_icon::menu::MenuEvent {
            id: self.menu_ids[index].clone(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_stays_in_work_area_for_each_taskbar_edge_and_negative_monitors() {
        for work in [[0, 0, 1920, 1040], [-2560, -300, 0, 1100]] {
            for (left, top) in [
                (work[0], work[1]),
                (work[2] - 25, work[1]),
                (work[0], work[3]),
                (work[2] - 25, work[3]),
            ] {
                // Sizes include a 150% DPI popup.
                for (width, height) in [(360, 412), (540, 618)] {
                    let (x, y) = place_popup(
                        work,
                        Anchor {
                            left,
                            top,
                            right: left + 25,
                            bottom: top + 25,
                        },
                        width,
                        height,
                    );
                    assert!(x >= work[0] + 8 && x + width <= work[2] - 8);
                    assert!(y >= work[1] + 8 && y + height <= work[3] - 8);
                    if top == work[1] {
                        assert!(y > top);
                    } else {
                        assert!(y + height < top);
                    }
                }
            }
        }
    }
}
