use serde::Serialize;
use std::path::{Path, PathBuf};
use winreg::{
    enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_BINARY},
    RegKey, RegValue,
};

const APP_NAME: &str = "OMNAFK";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const STARTUP_APPROVED_RUN_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

/// Legacy Run value names from older builds (tauri-plugin-autostart used app identifier).
const LEGACY_RUN_NAMES: &[&str] = &["io.omnafk.app", "com.omnafk.app"];

pub const AUTOSTART_ARG: &str = "--autostart";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AutostartStatus {
    Disabled,
    Ok,
    Missing,
    Mismatch,
}

impl AutostartStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Ok => "ok",
            Self::Missing => "missing",
            Self::Mismatch => "mismatch",
        }
    }
}

pub fn read_run_entry() -> Option<String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    hkcu.open_subkey_with_flags(RUN_KEY, KEY_READ)
        .ok()
        .and_then(|key| key.get_value(APP_NAME).ok())
}

pub fn is_autostart_launch() -> bool {
    is_autostart_args(&std::env::args().collect::<Vec<_>>())
}

pub fn is_autostart_args(args: &[String]) -> bool {
    args.iter().any(|arg| arg == AUTOSTART_ARG)
}

pub fn autostart_run_command(app_exe: &Path) -> String {
    format!("{} {}", quote_path(app_exe), AUTOSTART_ARG)
}

pub fn normalize_run_path(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let exe_part = if let Some(rest) = trimmed.strip_prefix('"') {
        rest.split_once('"')
            .map(|(path, _)| path)
            .unwrap_or(rest)
            .trim()
    } else {
        trimmed.split_whitespace().next().unwrap_or(trimmed).trim()
    };
    if exe_part.is_empty() {
        None
    } else {
        Some(PathBuf::from(exe_part))
    }
}

pub fn paths_match(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => a
            .to_string_lossy()
            .eq_ignore_ascii_case(&b.to_string_lossy()),
    }
}

pub fn is_registered_for(exe: &Path) -> bool {
    read_run_entry()
        .and_then(|registered| normalize_run_path(&registered))
        .is_some_and(|registered| paths_match(&registered, exe))
}

pub fn autostart_status(config_enabled: bool, exe: &Path) -> AutostartStatus {
    if !config_enabled {
        return AutostartStatus::Disabled;
    }
    match read_run_entry() {
        None => AutostartStatus::Missing,
        Some(value) => {
            if normalize_run_path(&value).is_some_and(|registered| paths_match(&registered, exe)) {
                AutostartStatus::Ok
            } else {
                AutostartStatus::Mismatch
            }
        }
    }
}

pub fn cleanup_legacy_run_entries() {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(key) = hkcu.open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE) else {
        return;
    };
    for name in LEGACY_RUN_NAMES {
        let _ = key.delete_value(name);
    }
}

pub fn set_start_with_windows(enabled: bool, app_exe: &Path) -> Result<(), String> {
    if enabled {
        write_run_key(app_exe)?;
        mark_startup_approved(true);
    } else {
        delete_run_key();
        mark_startup_approved(false);
    }
    Ok(())
}

/// Register or unregister autostart and verify the Run key when enabled.
pub fn ensure_autostart(config_enabled: bool, exe: &Path) -> Result<AutostartStatus, String> {
    cleanup_legacy_run_entries();
    set_start_with_windows(config_enabled, exe)?;

    if !config_enabled {
        return Ok(AutostartStatus::Disabled);
    }

    if is_registered_for(exe) {
        return Ok(AutostartStatus::Ok);
    }

    set_start_with_windows(true, exe)?;
    if is_registered_for(exe) {
        Ok(AutostartStatus::Ok)
    } else {
        Ok(AutostartStatus::Mismatch)
    }
}

fn write_run_key(app_exe: &Path) -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(RUN_KEY)
        .map_err(|error| format!("Couldn't open Windows startup settings: {error}"))?;
    key.set_value(APP_NAME, &autostart_run_command(app_exe))
        .map_err(|error| format!("Couldn't register Start with Windows: {error}"))
}

fn delete_run_key() {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE) {
        let _ = key.delete_value(APP_NAME);
    }
}

fn mark_startup_approved(enabled: bool) {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok((key, _)) = hkcu.create_subkey(STARTUP_APPROVED_RUN_KEY) else {
        return;
    };
    if enabled {
        let mut bytes = key
            .get_raw_value(APP_NAME)
            .map(|value| value.bytes)
            .unwrap_or_else(|_| vec![0; 12]);
        if bytes.is_empty() {
            bytes.resize(12, 0);
        }
        bytes[0] = 0x02;
        let _ = key.set_raw_value(
            APP_NAME,
            &RegValue {
                vtype: REG_BINARY,
                bytes,
            },
        );
    } else {
        let _ = key.delete_value(APP_NAME);
    }
}

fn quote_path(path: &Path) -> String {
    format!("\"{}\"", path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn paths_match_case_insensitive_fallback() {
        assert!(paths_match(
            Path::new(r"C:\Games\OMNAFK.exe"),
            Path::new(r"c:\games\omnafk.exe")
        ));
    }

    #[test]
    fn normalize_run_path_strips_quotes_and_autostart_arg() {
        assert_eq!(
            normalize_run_path(r#""C:\OMNAFK\OMNAFK.exe" --autostart"#),
            Some(PathBuf::from(r"C:\OMNAFK\OMNAFK.exe"))
        );
    }

    #[test]
    fn normalize_run_path_keeps_quoted_spaces() {
        assert_eq!(
            normalize_run_path(r#""C:\Program Files\OMNAFK\OMNAFK.exe" --autostart"#),
            Some(PathBuf::from(r"C:\Program Files\OMNAFK\OMNAFK.exe"))
        );
    }

    #[test]
    fn autostart_run_command_includes_flag() {
        assert_eq!(
            autostart_run_command(Path::new(r"C:\OMNAFK\omnafk.exe")),
            r#""C:\OMNAFK\omnafk.exe" --autostart"#
        );
    }

    #[test]
    fn normalize_run_path_strips_quotes() {
        assert_eq!(
            normalize_run_path(r#""C:\OMNAFK\OMNAFK.exe""#),
            Some(PathBuf::from(r"C:\OMNAFK\OMNAFK.exe"))
        );
    }

    #[test]
    fn autostart_status_disabled_when_config_off() {
        assert_eq!(
            autostart_status(false, Path::new(r"C:\OMNAFK.exe")),
            AutostartStatus::Disabled
        );
    }
}
