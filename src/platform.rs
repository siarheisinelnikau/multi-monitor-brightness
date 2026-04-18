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
    Quit,
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
    menu_ids: [tray_icon::menu::MenuId; 4],
}

impl Tray {
    pub fn new(ctx: eframe::egui::Context) -> Result<Self, String> {
        use tray_icon::{
            Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent,
            menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
        };
        let show = MenuItem::new("Open Multi-Monitor Brightness", true, None);
        let up = MenuItem::new("Brightness +", true, None);
        let down = MenuItem::new("Brightness -", true, None);
        let quit = MenuItem::new("Quit Multi-Monitor Brightness", true, None);
        let menu = Menu::with_items(&[
            &show,
            &PredefinedMenuItem::separator(),
            &up,
            &down,
            &PredefinedMenuItem::separator(),
            &quit,
        ])
        .map_err(|e| e.to_string())?;
        let ids = [
            show.id().clone(),
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
                    Action::Up
                } else if event.id == ids[2] {
                    Action::Down
                } else if event.id == ids[3] {
                    Action::Quit
                } else {
                    return;
                };
                let _ = menu_sender.send(action);
                menu_ctx.request_repaint();
            });
        let handler = menu_handler.clone();
        MenuEvent::set_event_handler(Some(move |event| handler(event)));
        TrayIconEvent::set_event_handler(Some(move |event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                let _ = sender.send(Action::Show);
                ctx.request_repaint();
            }
        }));
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
        })
    }

    /// Exercise the exact installed menu callback and IDs in a live-window regression test.
    #[cfg(feature = "tray-smoke-test")]
    pub fn dispatch_menu_for_test(&self, action: Action) {
        let index = match action {
            Action::Show => 0,
            Action::Up => 1,
            Action::Down => 2,
            Action::Quit => 3,
        };
        (self.menu_handler)(tray_icon::menu::MenuEvent {
            id: self.menu_ids[index].clone(),
        });
    }
}
