//! Application text uses explicit English catalog keys; user data is never translated.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    #[serde(rename = "en")]
    English,
    #[serde(rename = "ru")]
    Russian,
    #[serde(rename = "fr")]
    French,
    #[serde(rename = "de")]
    German,
    #[serde(rename = "es")]
    Spanish,
    #[serde(rename = "it")]
    Italian,
}

impl Language {
    pub const ALL: [Self; 6] = [
        Self::English,
        Self::Russian,
        Self::French,
        Self::German,
        Self::Spanish,
        Self::Italian,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Russian => "Русский",
            Self::French => "Français",
            Self::German => "Deutsch",
            Self::Spanish => "Español",
            Self::Italian => "Italiano",
        }
    }

    #[cfg(feature = "ui-capture")]
    pub fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|language| {
            serde_json::to_string(language).ok().as_deref() == Some(&format!("\"{code}\""))
        })
    }

    pub fn text<'a>(self, key: &'a str) -> &'a str {
        let index = match self {
            Self::English => return key,
            Self::Russian => 0,
            Self::French => 1,
            Self::German => 2,
            Self::Spanish => 3,
            Self::Italian => 4,
        };
        catalog()
            .get(key)
            .map_or(key, |values| values[index].as_str())
    }

    pub fn install(self, ctx: &egui::Context) {
        ctx.data_mut(|data| data.insert_temp(language_id(), self));
    }
}

fn catalog() -> &'static BTreeMap<String, [String; 5]> {
    static CATALOG: OnceLock<BTreeMap<String, [String; 5]>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        serde_json::from_str(include_str!("locales.json")).expect("validated translation catalog")
    })
}

fn language_id() -> egui::Id {
    egui::Id::unique("application-language")
}

pub fn current(ctx: &egui::Context) -> Language {
    ctx.data(|data| data.get_temp(language_id()).unwrap_or_default())
}

pub fn tr<'a>(ui: &egui::Ui, key: &'a str) -> &'a str {
    current(ui.ctx()).text(key)
}

pub fn format(ui: &egui::Ui, key: &str, arguments: &[(&str, String)]) -> String {
    let mut text = tr(ui, key).to_owned();
    for (name, value) in arguments {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn placeholders(text: &str) -> Vec<&str> {
        let mut result: Vec<_> = text
            .split('{')
            .skip(1)
            .filter_map(|part| part.split_once('}').map(|(name, _)| name))
            .collect();
        result.sort_unstable();
        result
    }

    #[test]
    fn every_translation_is_complete_and_preserves_message_arguments() {
        assert!(catalog().len() > 80);
        for (english, translations) in catalog() {
            for translation in translations {
                assert!(!translation.trim().is_empty(), "{english}");
                assert_eq!(
                    placeholders(english),
                    placeholders(translation),
                    "{english}: {translation}"
                );
            }
        }
        assert_eq!(
            Language::German.text("Technical detail from Windows"),
            "Technical detail from Windows"
        );
    }
}
