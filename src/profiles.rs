//! Saved definitions and identity-based previews. This module never touches hardware.
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_PROFILES: usize = 24;
pub const MAX_NAME_CHARS: usize = 40;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub id: u64,
    pub name: String,
    pub monitors: BTreeMap<String, Level>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Level {
    pub name: String,
    pub brightness: u8,
    pub included: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    Ready,
    Missing,
    Unsupported,
    Ambiguous,
}

pub struct Connected<'a> {
    pub id: &'a str,
    pub supported: bool,
}

pub struct Preview<'a> {
    pub rows: Vec<(&'a str, &'a Level, Availability)>,
    pub untouched: usize,
}

impl Preview<'_> {
    pub fn count(&self, availability: Availability) -> usize {
        self.rows.iter().filter(|row| row.2 == availability).count()
    }
}

impl Profile {
    pub fn preview<'a>(&'a self, connected: &[Connected<'_>]) -> Preview<'a> {
        Preview {
            rows: self
                .monitors
                .iter()
                .map(|(id, level)| {
                    let mut matches = connected.iter().filter(|d| d.id == id);
                    let availability = match (matches.next(), matches.next()) {
                        (_, Some(_)) => Availability::Ambiguous,
                        (None, None) => Availability::Missing,
                        (Some(_), None) if id.contains("/physical-") => Availability::Ambiguous,
                        (Some(display), None) if display.supported => Availability::Ready,
                        _ => Availability::Unsupported,
                    };
                    (id.as_str(), level, availability)
                })
                .collect(),
            untouched: connected
                .iter()
                .filter(|d| !self.monitors.contains_key(d.id))
                .count(),
        }
    }
}

pub fn validate_name(
    name: &str,
    profiles: &[Profile],
    editing: Option<u64>,
) -> Result<String, &'static str> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Enter a profile name.");
    }
    if name.chars().count() > MAX_NAME_CHARS || name.chars().any(char::is_control) {
        return Err("Use up to 40 characters without line breaks.");
    }
    if profiles
        .iter()
        .any(|p| Some(p.id) != editing && p.name.to_lowercase() == name.to_lowercase())
    {
        return Err("A profile with this name already exists.");
    }
    if editing.is_none() && profiles.len() >= MAX_PROFILES {
        return Err("You can save up to 24 profiles. Delete one to add another.");
    }
    Ok(name.into())
}

pub fn next_id(profiles: &[Profile]) -> u64 {
    (1..)
        .find(|id| profiles.iter().all(|p| p.id != *id))
        .unwrap()
}

/// Migrate additive preferences without discarding valid profiles. Malformed duplicate
/// identifiers are repaired deterministically; definitions never depend on list position.
pub fn normalize(profiles: &mut [Profile]) {
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    for profile in profiles {
        if profile.id == 0 || !ids.insert(profile.id) {
            profile.id = (1..).find(|id| !ids.contains(id)).unwrap();
            ids.insert(profile.id);
        }
        let base: String = profile
            .name
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_NAME_CHARS)
            .collect();
        let base = if base.is_empty() {
            "Profile".into()
        } else {
            base
        };
        let mut name = base.clone();
        let mut suffix = 2;
        while !names.insert(name.to_lowercase()) {
            let ending = format!(" ({suffix})");
            name = base
                .chars()
                .take(MAX_NAME_CHARS - ending.len())
                .collect::<String>()
                + &ending;
            suffix += 1;
        }
        profile.name = name;
        profile.monitors.retain(|id, _| !id.is_empty());
        for level in profile.monitors.values_mut() {
            level.brightness = level.brightness.min(100);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Pending { revision: u64 },
    Applied,
    Failed,
    Missing,
    Unsupported,
    Ambiguous,
    Superseded,
}

#[derive(Clone, Debug)]
pub struct Application {
    pub name: String,
    pub outcomes: BTreeMap<String, (Level, Outcome)>,
}

impl Application {
    pub fn pending(&self) -> usize {
        self.outcomes
            .values()
            .filter(|(_, o)| matches!(o, Outcome::Pending { .. }))
            .count()
    }

    pub fn applied(&self) -> usize {
        self.outcomes
            .values()
            .filter(|(_, o)| *o == Outcome::Applied)
            .count()
    }

    pub fn skipped(&self) -> usize {
        self.outcomes.len() - self.pending() - self.applied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn fixture() -> Profile {
        Profile {
            id: 7,
            name: "Reading".into(),
            monitors: [
                (
                    "path-a".into(),
                    Level {
                        name: "A".into(),
                        brightness: 40,
                        included: true,
                    },
                ),
                (
                    "path-b".into(),
                    Level {
                        name: "B".into(),
                        brightness: 20,
                        included: false,
                    },
                ),
            ]
            .into(),
        }
    }

    #[test]
    fn preview_matches_identities_after_reorder_and_never_substitutes_a_new_display() {
        let profile = fixture();
        let preview = profile.preview(&[
            Connected {
                id: "path-b",
                supported: false,
            },
            Connected {
                id: "new-in-slot-a",
                supported: true,
            },
        ]);
        assert_eq!(preview.rows[0].2, Availability::Missing);
        assert_eq!(preview.rows[1].2, Availability::Unsupported);
        assert_eq!(preview.untouched, 1);
        let preview = profile.preview(&[
            Connected {
                id: "path-b",
                supported: true,
            },
            Connected {
                id: "path-a",
                supported: true,
            },
        ]);
        assert_eq!(preview.count(Availability::Ready), 2);
        assert_eq!(preview.rows[0].1.brightness, 40);
    }

    #[test]
    fn duplicate_live_identities_are_never_safe_to_apply() {
        let profile = fixture();
        let preview = profile.preview(&[
            Connected {
                id: "path-a",
                supported: true,
            },
            Connected {
                id: "path-a",
                supported: true,
            },
        ]);
        assert_eq!(preview.rows[0].2, Availability::Ambiguous);
        assert_eq!(preview.count(Availability::Ready), 0);
    }

    #[test]
    fn windows_slot_fallback_is_not_a_monitor_identity() {
        let mut profile = fixture();
        profile.monitors.insert(
            r"\\.\DISPLAY1/physical-0".into(),
            profile.monitors["path-a"].clone(),
        );
        let preview = profile.preview(&[Connected {
            id: r"\\.\DISPLAY1/physical-0",
            supported: true,
        }]);
        assert_eq!(preview.count(Availability::Ambiguous), 1);
        assert_eq!(
            preview.count(Availability::Ready),
            0,
            "a replacement in the same slot must never inherit a profile"
        );
    }

    #[test]
    fn names_are_deliberate_unique_and_bounded_with_unicode() {
        let profiles = vec![fixture()];
        assert!(validate_name(" reading ", &profiles, None).is_err());
        assert_eq!(
            validate_name(" Reading ", &profiles, Some(7)),
            Ok("Reading".into())
        );
        assert!(validate_name("\n", &profiles, None).is_err());
        assert!(validate_name(&"я".repeat(41), &profiles, None).is_err());
        assert!(validate_name("Écran", &profiles, None).is_ok());
    }

    #[test]
    fn normalization_repairs_imported_definitions_without_losing_monitor_levels() {
        let mut profiles = vec![fixture(), fixture()];
        profiles[0].monitors.get_mut("path-a").unwrap().brightness = 255;
        normalize(&mut profiles);
        assert_ne!(profiles[0].id, profiles[1].id);
        assert_ne!(profiles[0].name, profiles[1].name);
        assert_eq!(profiles[0].monitors["path-a"].brightness, 100);
        assert!(!profiles[1].monitors["path-b"].included);
    }
}
