use crate::model::{percent, raw_value};
use std::{
    collections::BTreeMap,
    ptr,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Devices::Display::*,
    Foundation::{HANDLE, LPARAM, RECT},
    Graphics::Gdi::*,
};

#[derive(Clone, Debug)]
pub struct MonitorInfo {
    pub id: String,
    pub name: String,
    pub detail: String,
    pub brightness: Option<u8>,
}

pub enum Command {
    Set {
        id: String,
        value: u8,
        revision: u64,
    },
    Refresh,
    Shutdown,
}

pub enum Event {
    Scanned(Result<Vec<MonitorInfo>, String>),
    Written {
        id: String,
        value: u8,
        revision: u64,
        result: Result<(), String>,
    },
}

pub struct Controller {
    pub commands: mpsc::Sender<Command>,
    pub events: mpsc::Receiver<Event>,
}

impl Controller {
    pub fn new(ctx: egui::Context, demo: bool) -> Result<Self, String> {
        let (commands, receiver) = mpsc::channel();
        let (sender, events) = mpsc::channel();
        thread::Builder::new()
            .name("monitor-ddc".into())
            .spawn(move || {
                let emit = |event| {
                    let _ = sender.send(event);
                    ctx.request_repaint();
                };
                let mut devices = Vec::new();
                let mut signature = Vec::new();
                let mut refresh = true;
                let mut last_probe = Instant::now();
                let mut last_loop = Instant::now();
                loop {
                    if refresh {
                        devices.clear(); // Release old physical handles before rescanning.
                        match if demo { Ok(demo_devices()) } else { discover() } {
                            Ok(found) => {
                                devices = found;
                                signature = topology().unwrap_or_default();
                                emit(Event::Scanned(Ok(devices
                                    .iter()
                                    .map(|d| d.info.clone())
                                    .collect())));
                            }
                            Err(error) => emit(Event::Scanned(Err(error))),
                        }
                        refresh = false;
                        last_probe = Instant::now();
                    }
                    let first = match receiver.recv_timeout(Duration::from_millis(100)) {
                        Ok(command) => Some(command),
                        Err(mpsc::RecvTimeoutError::Timeout) => None,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    };
                    let mut pending = BTreeMap::new();
                    for command in first.into_iter().chain(receiver.try_iter()) {
                        match command {
                            Command::Shutdown => return,
                            Command::Refresh => refresh = true,
                            Command::Set {
                                id,
                                value,
                                revision,
                            } => {
                                pending.insert(id, (value, revision));
                            }
                        }
                    }
                    // Keep only the newest queued value per monitor. DDC calls can take 50+ ms.
                    for (id, (value, revision)) in pending {
                        let result = match devices.iter_mut().find(|d| d.info.id == id) {
                            Some(device) => device.set(value),
                            None => {
                                Err("Display disconnected. Refresh displays and try again.".into())
                            }
                        };
                        emit(Event::Written {
                            id,
                            value,
                            revision,
                            result,
                        });
                    }
                    if !demo && last_probe.elapsed() >= Duration::from_secs(5) {
                        if let Ok(current) = topology() {
                            refresh |= current != signature;
                        }
                        last_probe = Instant::now();
                    }
                    // Refresh stale DDC handles after a suspend/resume or a long driver stall.
                    if !demo && last_loop.elapsed() > Duration::from_secs(20) {
                        refresh = true;
                    }
                    last_loop = Instant::now();
                }
            })
            .map_err(|e| format!("Could not start monitor service: {e}"))?;
        Ok(Self { commands, events })
    }
}

impl Drop for Controller {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
    }
}

enum BrightnessApi {
    HighLevel,
    Vcp,
    Demo,
    Unsupported,
}

struct Device {
    info: MonitorInfo,
    handle: HANDLE,
    api: BrightnessApi,
    min: u32,
    max: u32,
}

impl Device {
    fn set(&mut self, value: u8) -> Result<(), String> {
        let raw = raw_value(value, self.min, self.max);
        // SAFETY: Each handle is created and exclusively used by this worker until Drop.
        let success = unsafe {
            match self.api {
                BrightnessApi::HighLevel => SetMonitorBrightness(self.handle, raw) != 0,
                BrightnessApi::Vcp => SetVCPFeature(self.handle, 0x10, raw) != 0,
                BrightnessApi::Demo => true,
                BrightnessApi::Unsupported => {
                    return Err("Brightness control is unavailable on this display.".into());
                }
            }
        };
        if !success {
            return Err(format!(
                "{} did not accept the brightness change ({}). Check DDC/CI, then refresh displays.",
                self.info.name,
                std::io::Error::last_os_error()
            ));
        }
        self.info.brightness = Some(value);
        Ok(())
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: This object owns exactly one physical monitor handle.
            unsafe {
                DestroyPhysicalMonitor(self.handle);
            }
        }
    }
}

struct Logical {
    handle: HMONITOR,
    info: MONITORINFOEXW,
}

unsafe extern "system" fn enumerate(handle: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> i32 {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    // SAFETY: EnumDisplayMonitors calls synchronously; data points to our live Vec.
    unsafe {
        if GetMonitorInfoW(handle, &mut info.monitorInfo) != 0 {
            (*(data as *mut Vec<Logical>)).push(Logical { handle, info });
        }
    }
    1
}

fn logical_displays() -> Result<Vec<Logical>, String> {
    let mut displays: Vec<Logical> = Vec::new();
    // SAFETY: Callback and data outlive synchronous enumeration; no panics cross the ABI.
    if unsafe {
        EnumDisplayMonitors(
            ptr::null_mut(),
            ptr::null(),
            Some(enumerate),
            &mut displays as *mut _ as LPARAM,
        )
    } == 0
    {
        return Err(format!(
            "Could not enumerate displays: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(displays)
}

fn topology() -> Result<Vec<String>, String> {
    let mut keys: Vec<_> = logical_displays()?
        .iter()
        .map(|d| {
            let r = d.info.monitorInfo.rcMonitor;
            format!(
                "{}:{}:{}:{}:{}",
                wide(&d.info.szDevice),
                r.left,
                r.top,
                r.right,
                r.bottom
            )
        })
        .collect();
    keys.sort();
    Ok(keys)
}

fn discover() -> Result<Vec<Device>, String> {
    let mut devices = Vec::new();
    for logical in logical_displays()? {
        let display_name = wide(&logical.info.szDevice);
        let rect = logical.info.monitorInfo.rcMonitor;
        let detail = format!(
            "{} × {}{}",
            rect.right - rect.left,
            rect.bottom - rect.top,
            if logical.info.monitorInfo.dwFlags & 1 != 0 {
                "  ·  Main display"
            } else {
                ""
            }
        );
        let mut count = 0;
        // SAFETY: HMONITOR comes from enumeration; output storage is initialized.
        let available =
            unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(logical.handle, &mut count) } != 0;
        let mut physical =
            vec![PHYSICAL_MONITOR::default(); if available { count as usize } else { 0 }];
        let enumerated = !physical.is_empty()
            && unsafe {
                GetPhysicalMonitorsFromHMONITOR(logical.handle, count, physical.as_mut_ptr()) != 0
            };
        if !enumerated {
            let (id, name) = identity(&logical.info.szDevice, 0, &display_name);
            devices.push(Device {
                info: MonitorInfo {
                    id,
                    name,
                    detail,
                    brightness: None,
                },
                handle: ptr::null_mut(),
                api: BrightnessApi::Unsupported,
                min: 0,
                max: 100,
            });
            continue;
        }
        for (index, p) in physical.into_iter().enumerate() {
            // PHYSICAL_MONITOR is packed. Copy its description before taking a reference.
            let description = p.szPhysicalMonitorDescription;
            let physical_name = wide(&description);
            let (id, mut name) = identity(&logical.info.szDevice, index as u32, &display_name);
            if !physical_name.is_empty() && physical_name != "Generic PnP Monitor" {
                name = physical_name;
            }
            let mut device = Device {
                info: MonitorInfo {
                    id,
                    name,
                    detail: detail.clone(),
                    brightness: None,
                },
                handle: p.hPhysicalMonitor,
                api: BrightnessApi::Unsupported,
                min: 0,
                max: 100,
            };
            let (mut min, mut current, mut max) = (0, 0, 0);
            // SAFETY: device owns the live handle; all output pointers refer to local u32s.
            if unsafe { GetMonitorBrightness(device.handle, &mut min, &mut current, &mut max) } != 0
                && max > min
            {
                device.api = BrightnessApi::HighLevel;
            } else {
                min = 0;
                if unsafe {
                    GetVCPFeatureAndVCPFeatureReply(
                        device.handle,
                        0x10,
                        ptr::null_mut(),
                        &mut current,
                        &mut max,
                    )
                } != 0
                    && max > 0
                {
                    device.api = BrightnessApi::Vcp;
                }
            }
            if !matches!(device.api, BrightnessApi::Unsupported) {
                device.min = min;
                device.max = max;
                device.info.brightness = percent(current, min, max);
            }
            devices.push(device);
        }
    }
    Ok(devices)
}

fn identity(adapter: &[u16; 32], index: u32, fallback: &str) -> (String, String) {
    let mut device = DISPLAY_DEVICEW {
        cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
        ..Default::default()
    };
    // EDD_GET_DEVICE_INTERFACE_NAME yields a per-monitor interface path rather than DISPLAY1.
    // SAFETY: The adapter name is NUL-terminated and device is valid output storage.
    if unsafe { EnumDisplayDevicesW(adapter.as_ptr(), index, &mut device, 1) } != 0 {
        let id = wide(&device.DeviceID);
        let name = wide(&device.DeviceString);
        if !id.is_empty() {
            return (
                id,
                if name.is_empty() {
                    fallback.into()
                } else {
                    name
                },
            );
        }
    }
    (
        format!("{fallback}/physical-{index}"),
        format!("Display {}", index + 1),
    )
}

fn wide(value: &[u16]) -> String {
    String::from_utf16_lossy(&value[..value.iter().position(|v| *v == 0).unwrap_or(value.len())])
}

fn demo_devices() -> Vec<Device> {
    [
        (
            "demo-studio",
            "Studio display",
            "2560 × 1440  ·  Main display",
            Some(72),
        ),
        ("demo-portrait", "Portrait display", "1920 × 1080", Some(58)),
        ("demo-unsupported", "Built-in display", "1920 × 1200", None),
    ]
    .into_iter()
    .map(|(id, name, detail, brightness)| Device {
        info: MonitorInfo {
            id: id.into(),
            name: name.into(),
            detail: detail.into(),
            brightness,
        },
        handle: ptr::null_mut(),
        api: if brightness.is_some() {
            BrightnessApi::Demo
        } else {
            BrightnessApi::Unsupported
        },
        min: 0,
        max: 100,
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Requires a live Windows desktop; reads DDC/CI without changing brightness"]
    fn read_hardware_brightness() {
        let devices = discover().expect("enumerate physical monitors");
        for device in &devices {
            println!(
                "{}: {:?}% ({})",
                device.info.name, device.info.brightness, device.info.detail
            );
        }
        assert!(
            !devices.is_empty(),
            "No displays found in this desktop session"
        );
    }

    #[test]
    fn worker_coalesces_commands_and_returns_latest_revision() {
        let c = Controller::new(egui::Context::default(), true).unwrap();
        assert!(matches!(
            c.events.recv_timeout(Duration::from_secs(3)).unwrap(),
            Event::Scanned(Ok(_))
        ));
        for revision in 1..=30 {
            c.commands
                .send(Command::Set {
                    id: "demo-studio".into(),
                    value: revision as u8,
                    revision,
                })
                .unwrap();
        }
        loop {
            match c.events.recv_timeout(Duration::from_secs(3)).unwrap() {
                Event::Written {
                    value,
                    revision: 30,
                    result,
                    ..
                } => {
                    assert_eq!(value, 30);
                    assert!(result.is_ok());
                    break;
                }
                Event::Written { result, .. } => assert!(result.is_ok()),
                _ => panic!("unexpected worker event"),
            }
        }
        c.commands.send(Command::Refresh).unwrap();
        assert!(matches!(
            c.events.recv_timeout(Duration::from_secs(3)).unwrap(),
            Event::Scanned(Ok(_))
        ));
    }
}
