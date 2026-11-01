use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub language: crate::i18n::Language,
    pub theme: crate::theme::Theme,
    pub restore_on_start: bool,
    pub close_to_tray: bool,
    pub step: u8,
    pub monitors: BTreeMap<String, MonitorMemory>,
    pub schedule: Schedule,
    pub hotkeys: Hotkeys,
    pub check_updates_automatically: bool,
    pub last_update_check: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: crate::i18n::Language::default(),
            theme: crate::theme::Theme::default(),
            restore_on_start: true,
            close_to_tray: true,
            step: 5,
            monitors: BTreeMap::new(),
            schedule: Schedule::default(),
            hotkeys: Hotkeys::default(),
            check_updates_automatically: true,
            last_update_check: 0,
        }
    }
}

impl Settings {
    pub fn normalize(&mut self) {
        self.step = self.step.clamp(1, 25);
        for memory in self.monitors.values_mut() {
            memory.brightness = memory.brightness.min(100);
        }
        self.schedule.start %= 1440;
        self.schedule.morning %= 1440;
        self.schedule.duration = self.schedule.duration.clamp(1, 240);
        self.schedule.target = self.schedule.target.min(100);
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct MonitorMemory {
    pub brightness: u8,
    pub included: bool,
}

impl Default for MonitorMemory {
    fn default() -> Self {
        Self {
            brightness: 50,
            included: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Schedule {
    pub enabled: bool,
    pub start: u16,
    pub morning: u16,
    pub duration: u16,
    pub target: u8,
}

impl Default for Schedule {
    fn default() -> Self {
        Self {
            enabled: false,
            start: 20 * 60,
            morning: 8 * 60,
            duration: 60,
            target: 25,
        }
    }
}

impl Schedule {
    pub fn phase(&self, day: i64, minute: u16) -> Option<(i64, f32)> {
        if !self.enabled {
            return None;
        }
        let span = (self.morning + 1440 - self.start) % 1440;
        let elapsed = (minute + 1440 - self.start) % 1440;
        if span == 0 || elapsed >= span {
            return None;
        }
        let cycle = if minute < self.start { day - 1 } else { day };
        let duration = self.duration.max(1).min(span);
        Some((cycle, (elapsed as f32 / duration as f32).min(1.0)))
    }
}

pub fn dimmed(base: u8, target: u8, progress: f32) -> u8 {
    let base = base.min(100) as f32;
    let target = (target.min(100) as f32).min(base);
    (base + (target - base) * progress.clamp(0.0, 1.0)).round() as u8
}

pub fn adjusted(value: u8, delta: i16) -> u8 {
    (value as i16 + delta).clamp(0, 100) as u8
}

pub fn percent(raw: u32, min: u32, max: u32) -> Option<u8> {
    if max <= min {
        return None;
    }
    Some(((raw.clamp(min, max) - min) as f64 * 100.0 / (max - min) as f64).round() as u8)
}

pub fn raw_value(value: u8, min: u32, max: u32) -> u32 {
    min + ((max.saturating_sub(min)) as f64 * value.min(100) as f64 / 100.0).round() as u32
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: String,
}

impl Default for Hotkey {
    fn default() -> Self {
        Self {
            ctrl: true,
            alt: true,
            shift: false,
            key: "Up".into(),
        }
    }
}

impl Hotkey {
    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push("Alt");
        }
        if self.shift {
            parts.push("Shift");
        }
        parts.push(&self.key);
        parts.join(" + ")
    }

    pub fn virtual_key(&self) -> Option<u32> {
        match self.key.as_str() {
            "Up" => Some(0x26),
            "Down" => Some(0x28),
            "Left" => Some(0x25),
            "Right" => Some(0x27),
            "Page Up" => Some(0x21),
            "Page Down" => Some(0x22),
            "Home" => Some(0x24),
            "End" => Some(0x23),
            key if key.len() == 1 && key.as_bytes()[0].is_ascii_alphanumeric() => {
                Some(key.as_bytes()[0].to_ascii_uppercase() as u32)
            }
            key => key
                .strip_prefix('F')
                .and_then(|s| s.parse::<u32>().ok())
                .filter(|n| (1..=11).contains(n))
                .map(|n| 0x6F + n),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Hotkeys {
    pub enabled: bool,
    pub up: Hotkey,
    pub down: Hotkey,
    pub show: Hotkey,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            enabled: true,
            up: Hotkey::default(),
            down: Hotkey {
                key: "Down".into(),
                ..Hotkey::default()
            },
            show: Hotkey {
                key: "B".into(),
                ..Hotkey::default()
            },
        }
    }
}

impl Hotkeys {
    pub fn validate(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        let keys = [&self.up, &self.down, &self.show];
        for key in &keys {
            if !(key.ctrl || key.alt || key.shift) {
                return Err("Use at least one modifier for every shortcut.".into());
            }
            if key.virtual_key().is_none() {
                return Err(format!("Unsupported shortcut key: {}", key.key));
            }
        }
        for i in 0..keys.len() {
            for j in i + 1..keys.len() {
                if keys[i].ctrl == keys[j].ctrl
                    && keys[i].alt == keys[j].alt
                    && keys[i].shift == keys[j].shift
                    && keys[i].virtual_key() == keys[j].virtual_key()
                {
                    return Err("Each action needs a different shortcut.".into());
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_settings_keep_brightness_and_new_appearance_preferences_round_trip() {
        let old: Settings = serde_json::from_str(r#"{"step":9,"restore_on_start":false}"#).unwrap();
        assert_eq!(old.language, crate::i18n::Language::English);
        assert_eq!(old.theme, crate::theme::Theme::Dark);
        assert_eq!(old.step, 9);
        assert!(!old.restore_on_start);
        for language in crate::i18n::Language::ALL {
            let settings = Settings {
                language,
                theme: crate::theme::Theme::Light,
                ..old.clone()
            };
            let loaded: Settings =
                serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
            assert_eq!(loaded.language, language);
            assert_eq!(loaded.theme, crate::theme::Theme::Light);
            assert_eq!(loaded.step, 9);
        }
    }

    #[test]
    fn startup_update_opt_out_survives_saved_settings_and_older_preferences() {
        let old: Settings = serde_json::from_str(r#"{"step":10}"#).unwrap();
        assert!(old.check_updates_automatically);
        let disabled: Settings = serde_json::from_str(
            r#"{"check_updates_automatically":false,"last_update_check":123}"#,
        )
        .unwrap();
        let loaded: Settings =
            serde_json::from_str(&serde_json::to_string(&disabled).unwrap()).unwrap();
        assert!(!loaded.check_updates_automatically);
        assert_eq!(loaded.last_update_check, 123);
    }

    #[test]
    fn brightness_uses_the_hardware_range() {
        assert_eq!(percent(60, 20, 220), Some(20));
        assert_eq!(raw_value(20, 20, 220), 60);
        assert_eq!(percent(0, 20, 220), Some(0));
        assert_eq!(percent(u32::MAX, 0, u32::MAX), Some(100));
        assert_eq!(percent(0, 0, 0), None);
        assert_eq!(adjusted(98, 5), 100);
        assert_eq!(adjusted(2, -5), 0);
    }

    #[test]
    fn evening_crosses_midnight_and_restores_in_the_morning() {
        let s = Schedule {
            enabled: true,
            ..Schedule::default()
        };
        assert_eq!(s.phase(100, 19 * 60), None);
        assert_eq!(s.phase(100, 20 * 60), Some((100, 0.0)));
        assert_eq!(s.phase(100, 20 * 60 + 30), Some((100, 0.5)));
        assert_eq!(s.phase(101, 60), Some((100, 1.0)));
        assert_eq!(s.phase(101, 8 * 60), None);
        assert_eq!(s.phase(101, 20 * 60), Some((101, 0.0)));
    }

    #[test]
    fn daytime_schedules_and_short_windows_work() {
        let s = Schedule {
            enabled: true,
            start: 600,
            morning: 630,
            duration: 60,
            ..Schedule::default()
        };
        assert_eq!(s.phase(1, 615), Some((1, 0.5)));
        assert_eq!(s.phase(1, 630), None);
        assert_eq!(Schedule { morning: 600, ..s }.phase(1, 615), None);
    }

    #[test]
    fn dimming_preserves_darker_displays_and_manual_memory() {
        assert_eq!(dimmed(80, 20, 0.5), 50);
        assert_eq!(dimmed(10, 20, 1.0), 10);
        assert_eq!(dimmed(80, 20, 2.0), 20);
    }

    #[test]
    fn hotkey_validation_catches_conflicts_and_invalid_config() {
        let mut h = Hotkeys::default();
        assert!(h.validate().is_ok());
        h.down = h.up.clone();
        assert!(h.validate().is_err());
        h.down.key = "F12".into();
        assert!(h.validate().is_err());
        h.enabled = false;
        assert!(h.validate().is_ok());
    }
}
