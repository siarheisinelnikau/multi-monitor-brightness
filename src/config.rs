use crate::model::Settings;
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

pub fn path(portable: bool) -> io::Result<PathBuf> {
    if portable {
        Ok(std::env::current_exe()?
            .parent()
            .ok_or_else(|| io::Error::other("Cannot locate the executable folder"))?
            .join("multi-monitor-brightness.json"))
    } else {
        let appdata = std::env::var_os("APPDATA")
            .ok_or_else(|| io::Error::other("APPDATA is unavailable; launch with --portable"))?;
        Ok(PathBuf::from(appdata)
            .join("MultiMonitorBrightness")
            .join("settings.json"))
    }
}

pub fn load(path: &Path) -> Result<Settings, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Settings::default()),
        Err(e) => return Err(format!("Could not read settings: {e}")),
    };
    let mut settings: Settings = serde_json::from_slice(&bytes)
        .map_err(|e| format!("Settings could not be read. The original file is preserved. {e}"))?;
    settings.normalize();
    Ok(settings)
}

pub fn save(path: &Path, settings: &Settings) -> Result<(), String> {
    save_inner(path, settings).map_err(|e| format!("Could not save settings: {e}"))
}

fn save_inner(path: &Path, settings: &Settings) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("json.tmp");
    let mut file = fs::File::create(&temp)?;
    file.write_all(&serde_json::to_vec_pretty(settings)?)?;
    file.sync_all()?;
    drop(file);
    // Same-directory replacement keeps the previous settings intact until the new file is ready.
    fs::rename(&temp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_replace_atomically_and_keep_invalid_files() {
        let dir = std::env::current_dir()
            .unwrap()
            .join("target")
            .join(format!(
                "multi-monitor-brightness-test-{}",
                std::process::id()
            ));
        let path = dir.join("settings.json");
        let mut s = Settings::default();
        save(&path, &s).unwrap();
        s.step = 9;
        save(&path, &s).unwrap();
        assert_eq!(load(&path).unwrap().step, 9);
        fs::write(&path, b"broken JSON").unwrap();
        assert!(load(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"broken JSON");
        fs::remove_file(&path).unwrap();
        fs::remove_dir(&dir).unwrap();
    }
}
