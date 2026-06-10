use crate::{
    config::{
        self, AppConfig, KeepaliveAction, NotificationLevel, OverrideVerdict, Sensitivity,
        UpdateChannel,
    },
    engine::{EngineStatus, GameSnapshot, SharedEngine},
    flyout,
    stats::StatsSnapshot,
    updates,
};
use serde::Serialize;
use serde_json::Value;
use std::{fs, thread, time::Duration};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::DialogExt;

const STATE_EVENT: &str = "omnafk://state";

#[derive(Debug, Clone, Serialize)]
pub struct StatePayload {
    pub engine: EngineStatus,
    pub next_tick: Option<u64>,
    pub games: Vec<GameSnapshot>,
    pub stats: StatsSnapshot,
    pub config: ConfigPayload,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigPayload {
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
}

impl From<&AppConfig> for ConfigPayload {
    fn from(config: &AppConfig) -> Self {
        Self {
            interval: config.interval,
            randomize: config.randomize,
            action: config.action,
            send_without_focus: config.send_without_focus,
            hold_while_playing: config.hold_while_playing,
            manual_mode: config.manual_mode,
            sensitivity: config.sensitivity,
            autostart: config.autostart,
            show_on_launch: config.show_on_launch,
            remember_pin: config.remember_pin,
            notifications: config.notifications,
            hotkey: config.hotkey.clone(),
            github_repo: config.github_repo.clone(),
            update_channel: config.update_channel,
            check_updates_on_launch: config.check_updates_on_launch,
            pinned: config.pinned,
        }
    }
}

#[tauri::command]
pub fn get_state(engine: State<'_, SharedEngine>) -> StatePayload {
    state_payload(engine.inner())
}

#[tauri::command]
pub fn set_config(
    key: String,
    value: Value,
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    let payload = mutate_config(&app, engine.inner(), |config| {
        apply_config_value(config, &key, value)
    })?;
    apply_live_config(&app, &key, &payload)?;
    Ok(payload)
}

#[tauri::command]
pub fn cycle_override(
    exe: String,
    wclass: String,
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    mutate_config(&app, engine.inner(), |config| {
        let next = match config.override_for(&exe, &wclass) {
            None => Some(OverrideVerdict::Game),
            Some(OverrideVerdict::Game) => Some(OverrideVerdict::Ignored),
            Some(OverrideVerdict::Ignored) => None,
        };
        config.set_override(&exe, &wclass, next);
        Ok(())
    })
}

#[tauri::command]
pub fn rescan(app: AppHandle, engine: State<'_, SharedEngine>) -> Result<StatePayload, String> {
    engine.run_detection_cycle();
    emit_and_return(&app, engine.inner())
}

#[tauri::command]
pub fn set_suspended(
    suspended: bool,
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    mutate_config(&app, engine.inner(), |config| {
        config.suspended = suspended;
        Ok(())
    })
}

#[tauri::command]
pub fn set_pinned(
    pinned: bool,
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    mutate_config(&app, engine.inner(), |config| {
        config.pinned = pinned;
        Ok(())
    })
}

#[tauri::command]
pub fn hide_flyout(
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    if let Some(window) = app.get_webview_window("flyout") {
        window.hide().map_err(|error| {
            format!("Couldn't hide the flyout - restart OMNAFK to fix this: {error}")
        })?;
    }
    emit_and_return(&app, engine.inner())
}

#[tauri::command]
pub fn set_hotkey(
    hotkey: String,
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    let payload = mutate_config(&app, engine.inner(), |config| {
        if hotkey.trim().is_empty() {
            return Err(
                "Couldn't set the hotkey - press a valid key combination to fix this.".to_string(),
            );
        }
        config.hotkey = hotkey.trim().to_ascii_uppercase();
        Ok(())
    })?;
    flyout::register_hotkey(&app, &payload.config.hotkey)?;
    Ok(payload)
}

#[tauri::command]
pub fn reset_stats(
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    engine.reset_stats();
    emit_and_return(&app, engine.inner())
}

#[tauri::command]
pub fn import_settings(
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    let Some(path) = app
        .dialog()
        .file()
        .add_filter("JSON", &["json"])
        .blocking_pick_file()
    else {
        return Ok(state_payload(engine.inner()));
    };
    let path = path.into_path().map_err(|error| {
        format!("Couldn't import settings - choose a local JSON file to fix this: {error}")
    })?;
    let imported = config::load_from_path(&path).map_err(|error| error.to_string())?;

    engine.replace_config(imported);
    persist_config(engine.inner())?;
    emit_and_return(&app, engine.inner())
}

#[tauri::command]
pub fn export_settings(
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    let Some(path) = app
        .dialog()
        .file()
        .add_filter("JSON", &["json"])
        .set_file_name("omnafk-settings.json")
        .blocking_save_file()
    else {
        return Ok(state_payload(engine.inner()));
    };
    let path = path.into_path().map_err(|error| {
        format!("Couldn't export settings - choose a local file path to fix this: {error}")
    })?;
    let config = engine.snapshot().config;
    let json = serde_json::to_vec_pretty(&config).map_err(|error| {
        format!("Couldn't export settings - fix the current config to continue: {error}")
    })?;
    fs::write(&path, json).map_err(|error| {
        format!("Couldn't export settings - choose a writable folder to fix this: {error}")
    })?;
    emit_and_return(&app, engine.inner())
}

#[tauri::command]
pub fn check_updates(engine: State<'_, SharedEngine>) -> Result<updates::UpdateCheck, String> {
    let config = engine.snapshot().config;
    updates::check(
        &config.github_repo,
        config.update_channel,
        env!("CARGO_PKG_VERSION"),
    )
}

#[tauri::command]
pub fn open_github(engine: State<'_, SharedEngine>) -> Result<(), String> {
    let config = engine.snapshot().config;
    updates::repo_url(&config.github_repo).and_then(|url| updates::open_url(&url))
}

#[tauri::command]
pub fn open_github_releases(engine: State<'_, SharedEngine>) -> Result<(), String> {
    let config = engine.snapshot().config;
    updates::releases_url(&config.github_repo).and_then(|url| updates::open_url(&url))
}

#[tauri::command]
pub fn open_github_issue(engine: State<'_, SharedEngine>) -> Result<(), String> {
    let config = engine.snapshot().config;
    updates::issues_url(&config.github_repo).and_then(|url| updates::open_url(&url))
}

#[tauri::command]
pub fn open_github_url(url: String) -> Result<(), String> {
    updates::open_url(&url)
}

pub fn spawn_state_pump(app: AppHandle, engine: SharedEngine) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(1));
        let Some(window) = app.get_webview_window("flyout") else {
            continue;
        };
        if window.is_visible().unwrap_or(false) {
            let _ = emit_state(&app, &engine);
        }
    });
}

pub fn emit_state(app: &AppHandle, engine: &SharedEngine) -> Result<(), String> {
    app.emit(STATE_EVENT, state_payload(engine))
        .map_err(|error| format!("Couldn't update the flyout - reopen OMNAFK to fix this: {error}"))
}

fn emit_and_return(app: &AppHandle, engine: &SharedEngine) -> Result<StatePayload, String> {
    let payload = state_payload(engine);
    app.emit(STATE_EVENT, payload.clone()).map_err(|error| {
        format!("Couldn't update the flyout - reopen OMNAFK to fix this: {error}")
    })?;
    Ok(payload)
}

fn mutate_config(
    app: &AppHandle,
    engine: &SharedEngine,
    update: impl FnOnce(&mut AppConfig) -> Result<(), String>,
) -> Result<StatePayload, String> {
    let mut config = engine.snapshot().config;
    update(&mut config)?;
    engine.replace_config(config);
    persist_config(engine)?;
    emit_and_return(app, engine)
}

fn persist_config(engine: &SharedEngine) -> Result<(), String> {
    config::save(&engine.snapshot().config).map_err(|error| {
        format!("Couldn't save settings - check %APPDATA% permissions to fix this: {error}")
    })
}

fn apply_live_config(app: &AppHandle, key: &str, payload: &StatePayload) -> Result<(), String> {
    match key {
        "autostart" => {
            let manager = app.autolaunch();
            if payload.config.autostart {
                manager.enable()
            } else {
                manager.disable()
            }
            .map_err(|error| format!("Couldn't update Start with Windows - check Windows startup permissions to fix this: {error}"))?;
        }
        "hotkey" => flyout::register_hotkey(app, &payload.config.hotkey)?,
        _ => {}
    }
    Ok(())
}

fn state_payload(engine: &SharedEngine) -> StatePayload {
    let snapshot = engine.snapshot();
    StatePayload {
        engine: snapshot.engine,
        next_tick: snapshot.next_tick,
        games: snapshot.games,
        stats: snapshot.stats,
        config: ConfigPayload::from(&snapshot.config),
    }
}

fn apply_config_value(config: &mut AppConfig, key: &str, value: Value) -> Result<(), String> {
    match key {
        "interval" => {
            let interval = value.as_u64().ok_or_else(|| {
                "Couldn't set interval - choose a number of seconds to fix this.".to_string()
            })?;
            if !(10..=3600).contains(&interval) {
                return Err(
                    "Couldn't set interval - choose 10 to 3600 seconds to fix this.".to_string(),
                );
            }
            config.interval = interval;
        }
        "randomize" => config.randomize = bool_value(value, key)?,
        "send_without_focus" => config.send_without_focus = bool_value(value, key)?,
        "hold_while_playing" => config.hold_while_playing = bool_value(value, key)?,
        "manual_mode" => config.manual_mode = bool_value(value, key)?,
        "autostart" => config.autostart = bool_value(value, key)?,
        "show_on_launch" => config.show_on_launch = bool_value(value, key)?,
        "remember_pin" => config.remember_pin = bool_value(value, key)?,
        "check_updates_on_launch" => config.check_updates_on_launch = bool_value(value, key)?,
        "pinned" => config.pinned = bool_value(value, key)?,
        "action" => config.action = parse_action(string_value(value, key)?.as_str())?,
        "sensitivity" => {
            config.sensitivity = parse_sensitivity(string_value(value, key)?.as_str())?
        }
        "update_channel" => {
            config.update_channel = parse_update_channel(string_value(value, key)?.as_str())?
        }
        "github_repo" => {
            let repo = string_value(value, key)?;
            config.github_repo = if repo.trim().is_empty() {
                String::new()
            } else {
                updates::normalize_repo(&repo)?
            };
        }
        "notifications" => {
            config.notifications = parse_notifications(string_value(value, key)?.as_str())?;
        }
        "hotkey" => {
            let hotkey = string_value(value, key)?;
            if hotkey.trim().is_empty() {
                return Err(
                    "Couldn't set hotkey - press a valid key combination to fix this.".to_string(),
                );
            }
            config.hotkey = hotkey.trim().to_ascii_uppercase();
        }
        other => {
            return Err(format!(
                "Couldn't save setting '{other}' - update the frontend to use a supported config key."
            ));
        }
    }
    Ok(())
}

fn bool_value(value: Value, key: &str) -> Result<bool, String> {
    value
        .as_bool()
        .ok_or_else(|| format!("Couldn't save setting '{key}' - use true or false to fix this."))
}

fn string_value(value: Value, key: &str) -> Result<String, String> {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| format!("Couldn't save setting '{key}' - use text to fix this."))
}

fn parse_action(value: &str) -> Result<KeepaliveAction, String> {
    match value {
        "Space tap" => Ok(KeepaliveAction::SpaceTap),
        "W tap" => Ok(KeepaliveAction::WTap),
        "Camera nudge" => Ok(KeepaliveAction::CameraNudge),
        _ => Err(
            "Couldn't set action - choose Space tap, W tap, or Camera nudge to fix this."
                .to_string(),
        ),
    }
}

fn parse_sensitivity(value: &str) -> Result<Sensitivity, String> {
    match value {
        "Strict" => Ok(Sensitivity::Strict),
        "Standard" => Ok(Sensitivity::Standard),
        "Broad" => Ok(Sensitivity::Broad),
        _ => Err(
            "Couldn't set sensitivity - choose Strict, Standard, or Broad to fix this.".to_string(),
        ),
    }
}

fn parse_notifications(value: &str) -> Result<NotificationLevel, String> {
    match value {
        "All" => Ok(NotificationLevel::All),
        "Errors only" => Ok(NotificationLevel::ErrorsOnly),
        "None" => Ok(NotificationLevel::None),
        _ => Err(
            "Couldn't set notifications - choose All, Errors only, or None to fix this."
                .to_string(),
        ),
    }
}

fn parse_update_channel(value: &str) -> Result<UpdateChannel, String> {
    match value {
        "Stable" => Ok(UpdateChannel::Stable),
        "Prerelease" => Ok(UpdateChannel::Prerelease),
        _ => Err(
            "Couldn't set update channel - choose Stable or Prerelease to fix this.".to_string(),
        ),
    }
}
