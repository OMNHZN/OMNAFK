use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
};

pub const DEFAULT_GITHUB_REPO: &str = "OMNHZN/OMNAFK";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeepaliveAction {
    #[serde(rename = "Space tap")]
    SpaceTap,
    #[serde(rename = "W tap")]
    WTap,
    #[serde(rename = "Camera nudge")]
    CameraNudge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sensitivity {
    Strict,
    Standard,
    Broad,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotificationLevel {
    All,
    #[serde(rename = "Errors only")]
    ErrorsOnly,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdateChannel {
    Stable,
    Prerelease,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OverrideVerdict {
    Game,
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub interval: u64,
    pub randomize: bool,
    pub action: KeepaliveAction,
    pub send_without_focus: bool,
    pub hold_while_playing: bool,
    pub manual_mode: bool,
    pub sensitivity: Sensitivity,
    pub autostart: bool,
    pub show_on_launch: bool,
    pub remember_pin: bool,
    pub notifications: NotificationLevel,
    pub hotkey: String,
    pub github_repo: String,
    pub update_channel: UpdateChannel,
    pub check_updates_on_launch: bool,
    pub pinned: bool,

    pub suspended: bool,
    pub pin_position: Option<PinPosition>,
    pub first_run_notified: bool,
    pub overrides: BTreeMap<String, BTreeMap<String, OverrideVerdict>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinPosition {
    pub x: i32,
    pub y: i32,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            interval: 540,
            randomize: true,
            action: KeepaliveAction::SpaceTap,
            send_without_focus: true,
            hold_while_playing: true,
            manual_mode: false,
            sensitivity: Sensitivity::Standard,
            autostart: true,
            show_on_launch: false,
            remember_pin: true,
            notifications: NotificationLevel::ErrorsOnly,
            hotkey: "CTRL+ALT+K".to_string(),
            github_repo: DEFAULT_GITHUB_REPO.to_string(),
            update_channel: UpdateChannel::Stable,
            check_updates_on_launch: false,
            pinned: false,
            suspended: false,
            pin_position: None,
            first_run_notified: false,
            overrides: BTreeMap::new(),
        }
    }
}

impl AppConfig {
    pub fn override_for(&self, exe: &str, wclass: &str) -> Option<OverrideVerdict> {
        self.overrides
            .get(&identity_exe_key(exe))
            .and_then(|classes| classes.get(wclass).copied())
    }

    pub fn set_override(&mut self, exe: &str, wclass: &str, verdict: Option<OverrideVerdict>) {
        let exe_key = identity_exe_key(exe);
        match verdict {
            Some(verdict) => {
                self.overrides
                    .entry(exe_key)
                    .or_default()
                    .insert(wclass.to_string(), verdict);
            }
            None => {
                if let Some(classes) = self.overrides.get_mut(&exe_key) {
                    classes.remove(wclass);
                    if classes.is_empty() {
                        self.overrides.remove(&exe_key);
                    }
                }
            }
        }
    }
}

pub fn config_path() -> io::Result<PathBuf> {
    let appdata = dirs::config_dir().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Couldn't find %APPDATA% - restore your Windows profile folders to fix this.",
        )
    })?;
    Ok(appdata.join("OMNAFK").join("config.json"))
}

pub fn load() -> io::Result<AppConfig> {
    load_from_path(&config_path()?)
}

pub fn save(config: &AppConfig) -> io::Result<()> {
    save_to_path(config, &config_path()?)
}

pub fn load_from_path(path: &Path) -> io::Result<AppConfig> {
    match fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw).map_err(invalid_config),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(AppConfig::default()),
        Err(error) => Err(error),
    }
}

pub fn save_to_path(config: &AppConfig, path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp_path = path.with_extension("json.tmp");
    let json = serde_json::to_vec_pretty(config).map_err(invalid_config)?;

    {
        let mut tmp = File::create(&tmp_path)?;
        tmp.write_all(&json)?;
        tmp.write_all(b"\n")?;
        tmp.sync_all()?;
    }

    replace_file(&tmp_path, path)
}

fn identity_exe_key(exe: &str) -> String {
    exe.to_ascii_lowercase()
}

fn invalid_config(error: serde_json::Error) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("Couldn't read OMNAFK settings - fix config.json JSON syntax to continue: {error}"),
    )
}

#[cfg(windows)]
fn replace_file(src: &Path, dst: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::PCWSTR,
        Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        },
    };

    let src_w: Vec<u16> = src.as_os_str().encode_wide().chain(Some(0)).collect();
    let dst_w: Vec<u16> = dst.as_os_str().encode_wide().chain(Some(0)).collect();

    unsafe {
        MoveFileExW(
            PCWSTR(src_w.as_ptr()),
            PCWSTR(dst_w.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
        .map_err(|_| io::Error::last_os_error())
    }
}

#[cfg(not(windows))]
fn replace_file(src: &Path, dst: &Path) -> io::Result<()> {
    fs::rename(src, dst)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_config_path(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "omnafk-config-test-{}-{}.json",
            std::process::id(),
            name
        ));
        let _ = fs::remove_file(&path);
        path
    }

    #[test]
    fn defaults_match_ipc_contract() {
        let config = AppConfig::default();

        assert_eq!(config.interval, 540);
        assert!(config.randomize);
        assert_eq!(config.action, KeepaliveAction::SpaceTap);
        assert!(config.send_without_focus);
        assert!(config.hold_while_playing);
        assert!(!config.manual_mode);
        assert_eq!(config.sensitivity, Sensitivity::Standard);
        assert!(config.autostart);
        assert!(!config.show_on_launch);
        assert!(config.remember_pin);
        assert_eq!(config.notifications, NotificationLevel::ErrorsOnly);
        assert_eq!(config.hotkey, "CTRL+ALT+K");
        assert_eq!(config.github_repo, DEFAULT_GITHUB_REPO);
        assert_eq!(config.update_channel, UpdateChannel::Stable);
        assert!(!config.check_updates_on_launch);
        assert!(!config.pinned);
        assert!(!config.suspended);
        assert!(config.pin_position.is_none());
        assert!(!config.first_run_notified);
        assert!(config.overrides.is_empty());
    }

    #[test]
    fn roundtrips_config_json() {
        let path = temp_config_path("roundtrip");
        let mut config = AppConfig {
            interval: 120,
            action: KeepaliveAction::CameraNudge,
            sensitivity: Sensitivity::Broad,
            pinned: true,
            suspended: true,
            pin_position: Some(PinPosition { x: 12, y: 34 }),
            ..AppConfig::default()
        };
        config.set_override(
            "RobloxPlayerBeta.exe",
            "WINDOWSCLIENT",
            Some(OverrideVerdict::Game),
        );

        save_to_path(&config, &path).expect("save config");
        let loaded = load_from_path(&path).expect("load config");

        assert_eq!(loaded, config);
        assert_eq!(
            loaded.override_for("robloxplayerbeta.exe", "WINDOWSCLIENT"),
            Some(OverrideVerdict::Game)
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn tolerates_unknown_fields() {
        let path = temp_config_path("unknown");
        fs::write(
            &path,
            r#"{
              "interval": 30,
              "future_field": true,
              "action": "W tap",
              "notifications": "All"
            }"#,
        )
        .expect("write config");

        let loaded = load_from_path(&path).expect("load config");

        assert_eq!(loaded.interval, 30);
        assert_eq!(loaded.action, KeepaliveAction::WTap);
        assert_eq!(loaded.notifications, NotificationLevel::All);
        assert!(loaded.randomize);

        let _ = fs::remove_file(path);
    }
}
