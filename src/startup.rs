use std::{path::Path, ptr};
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS},
    System::Registry::*,
};

const RUN_KEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE_NAME: &str = "MultiMonitorBrightness";

#[derive(Clone, Copy, Default)]
pub struct Status {
    pub enabled: bool,
    pub uses_current_copy: bool,
}

pub fn status(portable: bool) -> Result<Status, String> {
    let command = command_line(
        &std::env::current_exe().map_err(|e| e.to_string())?,
        portable,
    )?;
    let existing = read(RUN_KEY)?;
    Ok(Status {
        enabled: existing.is_some(),
        uses_current_copy: existing.as_deref() == Some(&command),
    })
}

pub fn set_enabled(enabled: bool, portable: bool) -> Result<Status, String> {
    let command = if enabled {
        let command = command_line(
            &std::env::current_exe().map_err(|e| e.to_string())?,
            portable,
        )?;
        validate_length(&command)?;
        Some(command)
    } else {
        None
    };
    write(RUN_KEY, command.as_deref())?;
    status(portable)
}

fn command_line(executable: &Path, portable: bool) -> Result<String, String> {
    let path = executable
        .to_str()
        .ok_or("The executable path is not valid Unicode.")?;
    if path.contains(['"', '\0']) {
        return Err("The executable path cannot be used for autostart.".into());
    }
    let command = format!(
        "\"{path}\" --startup{}",
        if portable { " --portable" } else { "" }
    );
    Ok(command)
}

fn validate_length(command: &str) -> Result<(), String> {
    // Windows Run values are limited to 260 UTF-16 characters, including the terminator.
    if command.encode_utf16().count() + 1 > 260 {
        return Err(
            "The autostart path is too long. Move the executable to a shorter folder.".into(),
        );
    }
    Ok(())
}

fn read(subkey: &str) -> Result<Option<String>, String> {
    let key = crate::platform::wide(subkey);
    let name = crate::platform::wide(VALUE_NAME);
    let mut size = 0;
    // SAFETY: Strings are NUL-terminated; the first call only queries the buffer size.
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut size,
        )
    };
    if missing(result) {
        return Ok(None);
    }
    check(result)?;
    if size > 8192 || size % 2 != 0 {
        return Err("The Windows autostart entry could not be read.".into());
    }
    let mut buffer = vec![0u16; (size / 2) as usize + 1];
    // SAFETY: Buffer capacity is at least the byte count returned above.
    let result = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut size,
        )
    };
    if missing(result) {
        return Ok(None);
    }
    check(result)?;
    let length = buffer.iter().position(|v| *v == 0).unwrap_or(buffer.len());
    let command = String::from_utf16(&buffer[..length])
        .map_err(|_| "The Windows autostart entry contains invalid text.")?;
    Ok(if command.is_empty() {
        None
    } else {
        Some(command)
    })
}

fn write(subkey: &str, command: Option<&str>) -> Result<(), String> {
    let key_name = crate::platform::wide(subkey);
    let value_name = crate::platform::wide(VALUE_NAME);
    let Some(command) = command else {
        // SAFETY: Delete only this application's named value; no other startup items are touched.
        let result = unsafe {
            RegDeleteKeyValueW(HKEY_CURRENT_USER, key_name.as_ptr(), value_name.as_ptr())
        };
        return if missing(result) {
            Ok(())
        } else {
            check(result)
        };
    };
    let mut key = ptr::null_mut();
    // SAFETY: Output handle is initialized; this opens/creates a per-user key without elevation.
    check(unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            key_name.as_ptr(),
            0,
            ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            ptr::null(),
            &mut key,
            ptr::null_mut(),
        )
    })?;
    let data = crate::platform::wide(command);
    // SAFETY: data includes its terminating NUL; the key handle is live until RegCloseKey below.
    let result = unsafe {
        RegSetValueExW(
            key,
            value_name.as_ptr(),
            0,
            REG_SZ,
            data.as_ptr().cast(),
            (data.len() * 2) as u32,
        )
    };
    unsafe {
        RegCloseKey(key);
    }
    check(result)
}

fn missing(code: u32) -> bool {
    code == ERROR_FILE_NOT_FOUND || code == ERROR_PATH_NOT_FOUND
}

fn check(code: u32) -> Result<(), String> {
    if code == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!(
            "Could not access Windows startup settings: {}",
            std::io::Error::from_raw_os_error(code as i32)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_quotes_paths_and_preserves_portable_mode() {
        let path = Path::new("C:\\Apps\\Display Tools\\MultiMonitorBrightness.exe");
        assert_eq!(
            command_line(path, false).unwrap(),
            "\"C:\\Apps\\Display Tools\\MultiMonitorBrightness.exe\" --startup"
        );
        assert!(
            command_line(path, true)
                .unwrap()
                .ends_with(" --startup --portable")
        );
        assert!(!command_line(path, true).unwrap().contains("--demo"));
        assert!(command_line(Path::new("C:\\Монитор\\яркость.exe"), false).is_ok());
        assert!(
            validate_length(&command_line(Path::new(&"x".repeat(260)), false).unwrap()).is_err()
        );
    }

    #[test]
    #[ignore = "Writes an isolated test key in HKCU; never changes the Windows Run key"]
    fn registry_registration_round_trip() {
        let key = format!(
            "Software\\MultiMonitorBrightness\\Tests\\{}",
            std::process::id()
        );
        assert!(read(&key).unwrap().is_none());
        let command = command_line(
            Path::new("C:\\Программы\\Display Tools\\brightness.exe"),
            true,
        )
        .unwrap();
        write(&key, Some(&command)).unwrap();
        assert_eq!(read(&key).unwrap(), Some(command));
        write(&key, None).unwrap();
        write(&key, None).unwrap();
        assert_eq!(read(&key).unwrap(), None);
        // Delete only the empty, process-specific key created by this test.
        check(unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, crate::platform::wide(&key).as_ptr()) })
            .unwrap();
    }
}
