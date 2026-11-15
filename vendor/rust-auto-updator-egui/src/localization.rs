//! Built-in panel translations. Hosts own and persist the selected language.

/// Supported interface languages. Unknown language codes fall back to English.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Language {
    #[default]
    English,
    Russian,
    French,
    German,
    Spanish,
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

    /// Stable value suitable for the host application's settings file.
    pub const fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Russian => "ru",
            Self::French => "fr",
            Self::German => "de",
            Self::Spanish => "es",
            Self::Italian => "it",
        }
    }

    /// Accept a language code, including regional forms such as `fr-CA`.
    pub fn from_code(code: &str) -> Self {
        match code
            .split(['-', '_'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str()
        {
            "ru" => Self::Russian,
            "fr" => Self::French,
            "de" => Self::German,
            "es" => Self::Spanish,
            "it" => Self::Italian,
            _ => Self::English,
        }
    }

    /// Show native names so a user can recover from an unfamiliar interface.
    pub const fn native_name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Russian => "Русский",
            Self::French => "Français",
            Self::German => "Deutsch",
            Self::Spanish => "Español",
            Self::Italian => "Italiano",
        }
    }

    pub const fn text(self) -> &'static PanelText<'static> {
        match self {
            Self::English => &ENGLISH,
            Self::Russian => &RUSSIAN,
            Self::French => &FRENCH,
            Self::German => &GERMAN,
            Self::Spanish => &SPANISH,
            Self::Italian => &ITALIAN,
        }
    }
}

/// Text supplied to the optional panel. Hosts may provide their own catalog.
/// `installed_version` and `available_version` contain one `{version}` token.
/// Release notes and technical error details remain their original text.
#[derive(Clone, Copy, Debug)]
pub struct PanelText<'a> {
    pub heading: &'a str,
    pub check: &'a str,
    pub installed_version: &'a str,
    pub source: &'a str,
    pub checking: &'a str,
    pub current: &'a str,
    pub available_version: &'a str,
    pub download_install: &'a str,
    pub release_notes: &'a str,
    pub restart_notice: &'a str,
    pub whats_new: &'a str,
    pub downloading: &'a str,
    pub installing: &'a str,
    pub error: &'a str,
    pub disabled: &'a str,
    pub startup: &'a str,
    pub language: &'a str,
    pub theme: &'a str,
    pub light: &'a str,
    pub dark: &'a str,
}

pub const ENGLISH: PanelText<'static> = PanelText {
    heading: "Updates",
    check: "Check for updates",
    installed_version: "Installed version {version}",
    source: "Updates are downloaded from the official GitHub releases.",
    checking: "Checking GitHub…",
    current: "You're up to date.",
    available_version: "Version {version} is available",
    download_install: "Download and install",
    release_notes: "Release notes on GitHub",
    restart_notice: "The app will restart. Your settings will be kept.",
    whats_new: "What's new",
    downloading: "Downloading and verifying SHA-256…",
    installing: "Restarting to finish the update…",
    error: "Update failed",
    disabled: "Updates unavailable",
    startup: "Check for updates on startup",
    language: "Language",
    theme: "Theme",
    light: "Light",
    dark: "Dark",
};

pub const RUSSIAN: PanelText<'static> = PanelText {
    heading: "Обновления",
    check: "Проверить обновления",
    installed_version: "Установлена версия {version}",
    source: "Обновления загружаются из официальных выпусков GitHub.",
    checking: "Проверяем GitHub…",
    current: "Установлена последняя версия.",
    available_version: "Доступна версия {version}",
    download_install: "Скачать и установить",
    release_notes: "Описание выпуска на GitHub",
    restart_notice: "Приложение перезапустится. Настройки сохранятся.",
    whats_new: "Что нового",
    downloading: "Загрузка и проверка SHA-256…",
    installing: "Перезапуск для завершения обновления…",
    error: "Не удалось обновить",
    disabled: "Обновления недоступны",
    startup: "Проверять обновления при запуске",
    language: "Язык",
    theme: "Тема",
    light: "Светлая",
    dark: "Тёмная",
};

pub const FRENCH: PanelText<'static> = PanelText {
    heading: "Mises à jour",
    check: "Rechercher des mises à jour",
    installed_version: "Version installée : {version}",
    source: "Les mises à jour proviennent des versions officielles sur GitHub.",
    checking: "Vérification sur GitHub…",
    current: "Votre application est à jour.",
    available_version: "La version {version} est disponible",
    download_install: "Télécharger et installer",
    release_notes: "Notes de version sur GitHub",
    restart_notice: "L'application redémarrera. Vos paramètres seront conservés.",
    whats_new: "Nouveautés",
    downloading: "Téléchargement et vérification SHA-256…",
    installing: "Redémarrage pour terminer la mise à jour…",
    error: "Échec de la mise à jour",
    disabled: "Mises à jour indisponibles",
    startup: "Rechercher des mises à jour au démarrage",
    language: "Langue",
    theme: "Thème",
    light: "Clair",
    dark: "Sombre",
};

pub const GERMAN: PanelText<'static> = PanelText {
    heading: "Updates",
    check: "Nach Updates suchen",
    installed_version: "Installierte Version: {version}",
    source: "Updates werden aus den offiziellen GitHub-Releases geladen.",
    checking: "GitHub wird geprüft…",
    current: "Die Anwendung ist auf dem neuesten Stand.",
    available_version: "Version {version} ist verfügbar",
    download_install: "Herunterladen und installieren",
    release_notes: "Versionshinweise auf GitHub",
    restart_notice: "Die Anwendung wird neu gestartet. Ihre Einstellungen bleiben erhalten.",
    whats_new: "Neuerungen",
    downloading: "Download und SHA-256-Prüfung…",
    installing: "Neustart zum Abschließen des Updates…",
    error: "Update fehlgeschlagen",
    disabled: "Updates nicht verfügbar",
    startup: "Beim Start nach Updates suchen",
    language: "Sprache",
    theme: "Design",
    light: "Hell",
    dark: "Dunkel",
};

pub const SPANISH: PanelText<'static> = PanelText {
    heading: "Actualizaciones",
    check: "Buscar actualizaciones",
    installed_version: "Versión instalada: {version}",
    source: "Las actualizaciones se descargan de las versiones oficiales de GitHub.",
    checking: "Comprobando GitHub…",
    current: "La aplicación está actualizada.",
    available_version: "La versión {version} está disponible",
    download_install: "Descargar e instalar",
    release_notes: "Notas de la versión en GitHub",
    restart_notice: "La aplicación se reiniciará. Se conservará la configuración.",
    whats_new: "Novedades",
    downloading: "Descargando y verificando SHA-256…",
    installing: "Reiniciando para terminar la actualización…",
    error: "Error al actualizar",
    disabled: "Actualizaciones no disponibles",
    startup: "Buscar actualizaciones al iniciar",
    language: "Idioma",
    theme: "Tema",
    light: "Claro",
    dark: "Oscuro",
};

pub const ITALIAN: PanelText<'static> = PanelText {
    heading: "Aggiornamenti",
    check: "Cerca aggiornamenti",
    installed_version: "Versione installata: {version}",
    source: "Gli aggiornamenti provengono dalle versioni ufficiali su GitHub.",
    checking: "Verifica su GitHub…",
    current: "L'applicazione è aggiornata.",
    available_version: "La versione {version} è disponibile",
    download_install: "Scarica e installa",
    release_notes: "Note di rilascio su GitHub",
    restart_notice: "L'applicazione si riavvierà. Le impostazioni verranno mantenute.",
    whats_new: "Novità",
    downloading: "Download e verifica SHA-256…",
    installing: "Riavvio per completare l'aggiornamento…",
    error: "Aggiornamento non riuscito",
    disabled: "Aggiornamenti non disponibili",
    startup: "Cerca aggiornamenti all'avvio",
    language: "Lingua",
    theme: "Tema",
    light: "Chiaro",
    dark: "Scuro",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_codes_round_trip_and_regional_codes_resolve() {
        for language in Language::ALL {
            assert_eq!(Language::from_code(language.code()), language);
        }
        assert_eq!(Language::from_code("FR-ca"), Language::French);
        assert_eq!(Language::from_code("ru_RU"), Language::Russian);
        assert_eq!(Language::from_code("unknown"), Language::English);
        assert_eq!(Language::from_code(""), Language::English);
    }

    #[test]
    fn all_catalogs_are_complete_and_preserve_version_placeholders() {
        for language in Language::ALL {
            let text = language.text();
            for value in [
                text.heading,
                text.check,
                text.installed_version,
                text.source,
                text.checking,
                text.current,
                text.available_version,
                text.download_install,
                text.release_notes,
                text.restart_notice,
                text.whats_new,
                text.downloading,
                text.installing,
                text.error,
                text.disabled,
                text.startup,
                text.language,
                text.theme,
                text.light,
                text.dark,
            ] {
                assert!(
                    !value.trim().is_empty(),
                    "{} has an empty message",
                    language.code()
                );
            }
            for template in [text.installed_version, text.available_version] {
                assert_eq!(template.matches("{version}").count(), 1);
                let formatted = template.replace("{version}", "1.2.3");
                assert!(formatted.contains("1.2.3"));
                assert!(!formatted.contains(['{', '}']));
            }
        }
    }
}
