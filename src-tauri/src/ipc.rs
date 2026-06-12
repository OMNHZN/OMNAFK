use crate::{
    config::{
        self, parse_hhmm, validate_key_sequence, Accent, AppConfig, KeepaliveAction, MonitorStyle,
        MonitorWhen, NotificationLevel, OverrideVerdict, SafetyNoteDisplay, Sensitivity,
        TabLabelMode, TargetAction, TargetDensity, TargetSort, TargetView, UpdateChannel,
        UpdatePromptMode, VersionDisplay,
    },
    engine::{ActivityEvent, EngineStatus, GameSnapshot, SharedEngine},
    flyout, monitor,
    stats::StatsSnapshot,
    updates,
};
use serde::Serialize;
use serde_json::Value;
use std::{fs, thread, time::Duration};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_notification::NotificationExt;

const STATE_EVENT: &str = "omnafk://state";

#[derive(Debug, Clone, Serialize)]
pub struct StatePayload {
    pub version: String,
    pub engine: EngineStatus,
    pub next_tick: Option<u64>,
    pub games: Vec<GameSnapshot>,
    pub stats: StatsSnapshot,
    pub config: ConfigPayload,
    pub error: Option<String>,
    pub update: Option<updates::UpdateCheck>,
    pub paused_reason: Option<String>,
    pub snooze_remaining: Option<u64>,
    pub log: Vec<ActivityEvent>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArmedOverride {
    pub exe: String,
    pub wclass: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigPayload {
    pub interval: u64,
    pub randomize: bool,
    pub jitter_pct: u8,
    pub action: KeepaliveAction,
    pub adaptive_actions: bool,
    pub key_sequence: Vec<String>,
    pub send_without_focus: bool,
    pub hold_while_playing: bool,
    pub hold_window_secs: u64,
    pub idle_threshold_mins: u64,
    pub pause_on_battery: bool,
    pub pause_when_locked: bool,
    pub max_session_hours: u64,
    pub max_session_actions: u64,
    pub quiet_hours_enabled: bool,
    pub quiet_start: String,
    pub quiet_end: String,
    pub manual_mode: bool,
    pub sensitivity: Sensitivity,
    pub autostart: bool,
    pub show_on_launch: bool,
    pub remember_pin: bool,
    pub notifications: NotificationLevel,
    pub hotkey: String,
    pub suspend_hotkey: String,
    pub github_repo: String,
    pub update_channel: UpdateChannel,
    pub check_updates_on_launch: bool,
    pub ignored_update_tag: Option<String>,
    pub pinned: bool,
    pub last_tab: String,
    pub settings_interface_collapsed: bool,
    pub settings_updates_collapsed: bool,
    pub general_advanced_collapsed: bool,
    pub target_view: TargetView,
    pub target_density: TargetDensity,
    pub target_sort: TargetSort,
    pub tab_label_mode: TabLabelMode,
    pub version_display: VersionDisplay,
    pub safety_note_display: SafetyNoteDisplay,
    pub update_prompt_mode: UpdatePromptMode,
    pub accent: Accent,
    pub file_logging: bool,
    pub monitor_placement: bool,
    pub monitor_device: Option<String>,
    pub monitor_when: MonitorWhen,
    pub monitor_style: MonitorStyle,
    pub monitor_skip_active: bool,
    pub monitor_skip_active_secs: u64,
    pub tour_done: bool,
    pub armed_overrides: Vec<ArmedOverride>,
}

impl From<&AppConfig> for ConfigPayload {
    fn from(config: &AppConfig) -> Self {
        let armed_overrides = config
            .overrides
            .iter()
            .flat_map(|(exe, classes)| {
                classes
                    .iter()
                    .filter(|(_, verdict)| **verdict == OverrideVerdict::Game)
                    .map(move |(wclass, _)| ArmedOverride {
                        exe: exe.clone(),
                        wclass: wclass.clone(),
                    })
            })
            .collect();
        Self {
            interval: config.interval,
            randomize: config.randomize,
            jitter_pct: config.jitter_pct,
            action: config.action,
            adaptive_actions: config.adaptive_actions,
            key_sequence: config.key_sequence.clone(),
            send_without_focus: config.send_without_focus,
            hold_while_playing: config.hold_while_playing,
            hold_window_secs: config.hold_window_secs,
            idle_threshold_mins: config.idle_threshold_mins,
            pause_on_battery: config.pause_on_battery,
            pause_when_locked: config.pause_when_locked,
            max_session_hours: config.max_session_hours,
            max_session_actions: config.max_session_actions,
            quiet_hours_enabled: config.quiet_hours_enabled,
            quiet_start: config.quiet_start.clone(),
            quiet_end: config.quiet_end.clone(),
            manual_mode: config.manual_mode,
            sensitivity: config.sensitivity,
            autostart: config.autostart,
            show_on_launch: config.show_on_launch,
            remember_pin: config.remember_pin,
            notifications: config.notifications,
            hotkey: config.hotkey.clone(),
            suspend_hotkey: config.suspend_hotkey.clone(),
            github_repo: config.github_repo.clone(),
            update_channel: config.update_channel,
            check_updates_on_launch: config.check_updates_on_launch,
            ignored_update_tag: config.ignored_update_tag.clone(),
            pinned: config.pinned,
            last_tab: config.last_tab.clone(),
            settings_interface_collapsed: config.settings_interface_collapsed,
            settings_updates_collapsed: config.settings_updates_collapsed,
            general_advanced_collapsed: config.general_advanced_collapsed,
            target_view: config.target_view,
            target_density: config.target_density,
            target_sort: config.target_sort,
            tab_label_mode: config.tab_label_mode,
            version_display: config.version_display,
            safety_note_display: config.safety_note_display,
            update_prompt_mode: config.update_prompt_mode,
            accent: config.accent,
            file_logging: config.file_logging,
            monitor_placement: config.monitor_placement,
            monitor_device: config.monitor_device.clone(),
            monitor_when: config.monitor_when,
            monitor_style: config.monitor_style,
            monitor_skip_active: config.monitor_skip_active,
            monitor_skip_active_secs: config.monitor_skip_active_secs,
            tour_done: config.tour_done,
            armed_overrides,
        }
    }
}

#[tauri::command]
pub fn list_monitors() -> Vec<monitor::MonitorInfo> {
    monitor::list_monitors()
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
    let payload = mutate_config_with_reschedule(
        &app,
        engine.inner(),
        config_key_reschedules(&key),
        |config| apply_config_value(config, &key, value),
    )?;
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
pub fn set_override(
    exe: String,
    wclass: String,
    verdict: Option<String>,
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    let parsed = match verdict.as_deref() {
        None | Some("") | Some("auto") => None,
        Some("game") => Some(OverrideVerdict::Game),
        Some("ignored") => Some(OverrideVerdict::Ignored),
        Some(other) => {
            return Err(format!(
                "Couldn't set override '{other}' - use game, ignored, or auto to fix this."
            ));
        }
    };
    mutate_config(&app, engine.inner(), |config| {
        config.set_override(&exe, &wclass, parsed);
        Ok(())
    })
}

#[tauri::command]
pub fn clear_overrides(
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    mutate_config(&app, engine.inner(), |config| {
        config.overrides.clear();
        Ok(())
    })
}

#[tauri::command]
pub fn pause_target(
    exe: String,
    wclass: String,
    paused: bool,
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    mutate_config(&app, engine.inner(), |config| {
        config.set_paused(&exe, &wclass, paused);
        Ok(())
    })
}

#[tauri::command]
pub fn test_target(
    exe: String,
    wclass: String,
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    engine.test_target(&exe, &wclass)?;
    emit_and_return(&app, engine.inner())
}

#[tauri::command]
pub fn reset_learning(
    exe: String,
    wclass: String,
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    engine.reset_learning(&exe, &wclass);
    emit_and_return(&app, engine.inner())
}

#[tauri::command]
pub fn snooze(
    minutes: u64,
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    if minutes > 24 * 60 {
        return Err("Couldn't snooze - choose up to 24 hours to fix this.".to_string());
    }
    engine.snooze(minutes);
    emit_and_return(&app, engine.inner())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn set_target_profile(
    exe: String,
    wclass: String,
    action: Option<String>,
    interval: Option<u64>,
    key_sequence: Option<Vec<String>>,
    monitor: Option<String>,
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    mutate_config(&app, engine.inner(), |config| {
        let mut profile = config
            .profile_for(&exe, &wclass)
            .cloned()
            .unwrap_or_default();

        profile.action = match action.as_deref() {
            None | Some("") | Some("Use global") => None,
            Some(label) => Some(parse_target_action(label)?),
        };

        profile.interval = match interval {
            Some(secs) if (10..=3600).contains(&secs) => Some(secs),
            Some(_) => {
                return Err(
                    "Couldn't set profile interval - choose 10 to 3600 seconds to fix this."
                        .to_string(),
                );
            }
            None => None,
        };

        if let Some(keys) = key_sequence {
            validate_key_sequence(&keys)?;
            profile.key_sequence = keys;
        }

        profile.monitor = match monitor.as_deref() {
            None | Some("") | Some("Use global") => None,
            Some("Don't move") => Some("Don't move".to_string()),
            Some(device) => Some(device.to_string()),
        };

        config.set_profile(&exe, &wclass, profile);
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
    let payload = emit_and_return(&app, engine.inner())?;
    flyout::register_hotkeys(&app, &payload.config.hotkey, &payload.config.suspend_hotkey)?;
    apply_live_config(&app, "autostart", &payload)?;
    Ok(payload)
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
pub fn export_stats(
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    let Some(path) = app
        .dialog()
        .file()
        .add_filter("CSV", &["csv"])
        .set_file_name("omnafk-stats.csv")
        .blocking_save_file()
    else {
        return Ok(state_payload(engine.inner()));
    };
    let path = path.into_path().map_err(|error| {
        format!("Couldn't export stats - choose a local file path to fix this: {error}")
    })?;

    let snapshot = engine.snapshot();
    let mut csv = String::from("section,key,title,kept_seconds,actions\n");
    csv.push_str(&format!(
        "session,totals,,{},{}\n",
        snapshot.stats.kept, snapshot.stats.actions
    ));
    csv.push_str(&format!(
        "lifetime,totals,,{},{}\n",
        snapshot.stats.lifetime_kept, snapshot.stats.lifetime_actions
    ));
    for day in &snapshot.stats.daily {
        csv.push_str(&format!(
            "daily,{},seen={},{},{}\n",
            day.date, day.seen, day.kept, day.actions
        ));
    }
    for game in &snapshot.stats.lifetime_games {
        let title = game.title.replace([',', '\n'], " ");
        csv.push_str(&format!(
            "game,{},{},{},{}\n",
            game.identity.replace('\u{1f}', "|"),
            title,
            game.kept,
            game.actions
        ));
    }
    for (label, count) in &snapshot.stats.actions_by_type {
        csv.push_str(&format!("action_type,{label},,,{count}\n"));
    }

    fs::write(&path, csv).map_err(|error| {
        format!("Couldn't export stats - choose a writable folder to fix this: {error}")
    })?;
    emit_and_return(&app, engine.inner())
}

#[tauri::command]
pub fn reset_settings(
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    let mut defaults = AppConfig::default();
    // Keep one-time flags so resets don't replay first-run UX.
    {
        let current = engine.snapshot().config;
        defaults.first_run_notified = current.first_run_notified;
        defaults.tour_done = current.tour_done;
    }
    engine.replace_config(defaults);
    persist_config(engine.inner())?;
    let payload = emit_and_return(&app, engine.inner())?;
    let _ = flyout::register_hotkey(&app, &payload.config.hotkey);
    apply_live_config(&app, "autostart", &payload)?;
    Ok(payload)
}

#[tauri::command]
pub fn open_config_dir() -> Result<(), String> {
    let path = config::config_path()
        .map_err(|error| error.to_string())?
        .parent()
        .map(|parent| parent.to_path_buf())
        .ok_or_else(|| "Couldn't locate the OMNAFK config folder.".to_string())?;
    open_in_explorer(&path)
}

#[tauri::command]
pub fn open_log_file() -> Result<(), String> {
    let path = crate::engine::log_file_path()
        .ok_or_else(|| "Couldn't locate the OMNAFK log file.".to_string())?;
    if !path.exists() {
        return Err(
            "No log file yet - enable activity logging in Settings and let OMNAFK run first."
                .to_string(),
        );
    }
    open_in_explorer(&path)
}

#[tauri::command]
pub fn diagnostics(engine: State<'_, SharedEngine>) -> Result<String, String> {
    let snapshot = engine.snapshot();
    let config = &snapshot.config;
    let games = snapshot
        .games
        .iter()
        .map(|game| {
            format!(
                "  {} [{}] verdict={:?} effective={:?} score={} gone={} paused={}",
                game.exe,
                game.wclass,
                game.verdict,
                game.effective,
                game.score,
                game.gone,
                game.paused
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!(
        "OMNAFK diagnostics\n\
         version: {}\n\
         os: {} {}\n\
         engine: {:?}\n\
         elevated: {}\n\
         suspended: {} | snooze: {:?} | gate: {:?}\n\
         last error: {}\n\
         config: interval={}s randomize={} jitter={}% action={:?} sensitivity={:?} manual={}\n\
         gates: battery={} locked={} quiet={}({}-{}) idle={}m caps={}h/{} actions\n\
         session: kept={}s actions={} | lifetime: kept={}s actions={}\n\
         windows:\n{}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        snapshot.engine,
        crate::detector::current_process_elevated(),
        config.suspended,
        snapshot.snooze_remaining,
        snapshot.paused_reason,
        snapshot.error.as_deref().unwrap_or("none"),
        config.interval,
        config.randomize,
        config.jitter_pct,
        config.action,
        config.sensitivity,
        config.manual_mode,
        config.pause_on_battery,
        config.pause_when_locked,
        config.quiet_hours_enabled,
        config.quiet_start,
        config.quiet_end,
        config.idle_threshold_mins,
        config.max_session_hours,
        config.max_session_actions,
        snapshot.stats.kept,
        snapshot.stats.actions,
        snapshot.stats.lifetime_kept,
        snapshot.stats.lifetime_actions,
        if games.is_empty() {
            "  none".to_string()
        } else {
            games
        },
    ))
}

#[tauri::command]
pub fn get_changelog(
    engine: State<'_, SharedEngine>,
) -> Result<Vec<updates::ReleaseNotes>, String> {
    let config = engine.snapshot().config;
    updates::changelog(
        &config.github_repo,
        config.update_channel,
        env!("CARGO_PKG_VERSION"),
    )
}

fn open_in_explorer(path: &std::path::Path) -> Result<(), String> {
    std::process::Command::new("explorer")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("Couldn't open {path:?} - open it manually to fix this: {error}"))
}

#[tauri::command]
pub fn check_updates(
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<updates::UpdateCheck, String> {
    let config = engine.snapshot().config;
    let check = updates::check(
        &config.github_repo,
        config.update_channel,
        env!("CARGO_PKG_VERSION"),
    )?;
    if check.update_available && !config.ignores_update(&check.latest_tag) {
        engine.set_update_prompt(Some(check.clone()));
    } else {
        engine.set_update_prompt(None);
    }
    let _ = emit_state(&app, engine.inner());
    Ok(check)
}

#[tauri::command]
pub fn ignore_update(
    tag: String,
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    let tag = tag.trim();
    if tag.is_empty() {
        return Err("Couldn't ignore update - check for updates again to fix this.".to_string());
    }
    let tag = tag.to_string();
    engine.set_update_prompt(None);
    mutate_config_with_reschedule(&app, engine.inner(), false, |config| {
        config.ignored_update_tag = Some(tag);
        Ok(())
    })
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

        // Surface engine notices as Windows toasts regardless of flyout visibility.
        for notice in engine.take_notices() {
            if let Err(error) = app
                .notification()
                .builder()
                .title("OMNAFK")
                .body(notice)
                .show()
            {
                tracing::warn!("Couldn't show notification - enable Windows notifications to fix this: {error}");
            }
        }

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
    mutate_config_with_reschedule(app, engine, true, update)
}

fn mutate_config_with_reschedule(
    app: &AppHandle,
    engine: &SharedEngine,
    reschedule: bool,
    update: impl FnOnce(&mut AppConfig) -> Result<(), String>,
) -> Result<StatePayload, String> {
    let mut config = engine.snapshot().config;
    update(&mut config)?;
    if reschedule {
        engine.replace_config(config);
    } else {
        engine.update_config_without_reschedule(|current| *current = config);
    }
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
        "hotkey" | "suspend_hotkey" => {
            flyout::register_hotkeys(app, &payload.config.hotkey, &payload.config.suspend_hotkey)?
        }
        _ => {}
    }
    Ok(())
}

fn state_payload(engine: &SharedEngine) -> StatePayload {
    let snapshot = engine.snapshot();
    StatePayload {
        version: env!("CARGO_PKG_VERSION").to_string(),
        engine: snapshot.engine,
        next_tick: snapshot.next_tick,
        games: snapshot.games,
        stats: snapshot.stats,
        config: ConfigPayload::from(&snapshot.config),
        error: snapshot.error,
        update: snapshot.update,
        paused_reason: snapshot.paused_reason,
        snooze_remaining: snapshot.snooze_remaining,
        log: snapshot.log,
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
        "adaptive_actions" => config.adaptive_actions = bool_value(value, key)?,
        "monitor_placement" => config.monitor_placement = bool_value(value, key)?,
        "monitor_skip_active" => config.monitor_skip_active = bool_value(value, key)?,
        "monitor_device" => {
            let raw = string_value(value, key)?;
            config.monitor_device = if raw.is_empty() || raw == "Off" {
                None
            } else {
                Some(raw)
            };
        }
        "monitor_when" => {
            config.monitor_when = parse_monitor_when(string_value(value, key)?.as_str())?
        }
        "monitor_style" => {
            config.monitor_style = parse_monitor_style(string_value(value, key)?.as_str())?
        }
        "monitor_skip_active_secs" => {
            let secs = value.as_u64().ok_or_else(|| {
                "Couldn't set monitor skip window - choose a number of seconds to fix this."
                    .to_string()
            })?;
            if !(1..=60).contains(&secs) {
                return Err(
                    "Couldn't set monitor skip window - choose 1 to 60 seconds to fix this."
                        .to_string(),
                );
            }
            config.monitor_skip_active_secs = secs;
        }
        "jitter_pct" => {
            let pct = value.as_u64().ok_or_else(|| {
                "Couldn't set jitter - choose a percentage to fix this.".to_string()
            })?;
            if !(1..=50).contains(&pct) {
                return Err("Couldn't set jitter - choose 1 to 50 percent to fix this.".to_string());
            }
            config.jitter_pct = pct as u8;
        }
        "hold_window_secs" => {
            let secs = value.as_u64().ok_or_else(|| {
                "Couldn't set hold window - choose a number of seconds to fix this.".to_string()
            })?;
            if !(10..=600).contains(&secs) {
                return Err(
                    "Couldn't set hold window - choose 10 to 600 seconds to fix this.".to_string(),
                );
            }
            config.hold_window_secs = secs;
        }
        "idle_threshold_mins" => {
            let mins = value.as_u64().ok_or_else(|| {
                "Couldn't set idle threshold - choose a number of minutes to fix this.".to_string()
            })?;
            if mins > 120 {
                return Err(
                    "Couldn't set idle threshold - choose up to 120 minutes to fix this."
                        .to_string(),
                );
            }
            config.idle_threshold_mins = mins;
        }
        "max_session_hours" => {
            let hours = value.as_u64().ok_or_else(|| {
                "Couldn't set the safety cap - choose a number of hours to fix this.".to_string()
            })?;
            if hours > 168 {
                return Err(
                    "Couldn't set the safety cap - choose up to 168 hours to fix this.".to_string(),
                );
            }
            config.max_session_hours = hours;
        }
        "max_session_actions" => {
            let actions = value.as_u64().ok_or_else(|| {
                "Couldn't set the safety cap - choose a number of actions to fix this.".to_string()
            })?;
            if actions > 100_000 {
                return Err(
                    "Couldn't set the safety cap - choose up to 100000 actions to fix this."
                        .to_string(),
                );
            }
            config.max_session_actions = actions;
        }
        "quiet_hours_enabled" => config.quiet_hours_enabled = bool_value(value, key)?,
        "quiet_start" => config.quiet_start = hhmm_value(value, key)?,
        "quiet_end" => config.quiet_end = hhmm_value(value, key)?,
        "pause_on_battery" => config.pause_on_battery = bool_value(value, key)?,
        "pause_when_locked" => config.pause_when_locked = bool_value(value, key)?,
        "file_logging" => config.file_logging = bool_value(value, key)?,
        "tour_done" => config.tour_done = bool_value(value, key)?,
        "send_without_focus" => config.send_without_focus = bool_value(value, key)?,
        "hold_while_playing" => config.hold_while_playing = bool_value(value, key)?,
        "manual_mode" => config.manual_mode = bool_value(value, key)?,
        "autostart" => config.autostart = bool_value(value, key)?,
        "show_on_launch" => config.show_on_launch = bool_value(value, key)?,
        "remember_pin" => config.remember_pin = bool_value(value, key)?,
        "check_updates_on_launch" => config.check_updates_on_launch = bool_value(value, key)?,
        "pinned" => config.pinned = bool_value(value, key)?,
        "settings_interface_collapsed" => {
            config.settings_interface_collapsed = bool_value(value, key)?
        }
        "settings_updates_collapsed" => config.settings_updates_collapsed = bool_value(value, key)?,
        "general_advanced_collapsed" => config.general_advanced_collapsed = bool_value(value, key)?,
        "target_sort" => {
            config.target_sort = parse_target_sort(string_value(value, key)?.as_str())?
        }
        "accent" => config.accent = parse_accent(string_value(value, key)?.as_str())?,
        "suspend_hotkey" => {
            config.suspend_hotkey = string_value(value, key)?.trim().to_ascii_uppercase();
        }
        "target_view" => {
            config.target_view = parse_target_view(string_value(value, key)?.as_str())?
        }
        "target_density" => {
            config.target_density = parse_target_density(string_value(value, key)?.as_str())?
        }
        "tab_label_mode" => {
            config.tab_label_mode = parse_tab_label_mode(string_value(value, key)?.as_str())?
        }
        "version_display" => {
            config.version_display = parse_version_display(string_value(value, key)?.as_str())?
        }
        "safety_note_display" => {
            config.safety_note_display =
                parse_safety_note_display(string_value(value, key)?.as_str())?
        }
        "update_prompt_mode" => {
            config.update_prompt_mode =
                parse_update_prompt_mode(string_value(value, key)?.as_str())?
        }
        "action" => config.action = parse_action(string_value(value, key)?.as_str())?,
        "key_sequence" => {
            let keys = value
                .as_array()
                .ok_or_else(|| {
                    "Couldn't save key sequence - use a list of key names to fix this.".to_string()
                })?
                .iter()
                .filter_map(|item| item.as_str().map(|s| s.trim().to_ascii_uppercase()))
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>();
            validate_key_sequence(&keys)?;
            config.key_sequence = keys;
        }
        "sensitivity" => {
            config.sensitivity = parse_sensitivity(string_value(value, key)?.as_str())?
        }
        "update_channel" => {
            config.update_channel = parse_update_channel(string_value(value, key)?.as_str())?;
            config.ignored_update_tag = None;
        }
        "github_repo" => {
            let repo = string_value(value, key)?;
            config.github_repo = if repo.trim().is_empty() {
                String::new()
            } else {
                updates::normalize_repo(&repo)?
            };
            config.ignored_update_tag = None;
        }
        "ignored_update_tag" => {
            if value.is_null() {
                config.ignored_update_tag = None;
            } else {
                let tag = string_value(value, key)?;
                let tag = tag.trim();
                config.ignored_update_tag = (!tag.is_empty()).then(|| tag.to_string());
            }
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
        "last_tab" => {
            let tab = string_value(value, key)?;
            if is_valid_tab(&tab) {
                config.last_tab = tab;
            }
        }
        other => {
            return Err(format!(
                "Couldn't save setting '{other}' - update the frontend to use a supported config key."
            ));
        }
    }
    Ok(())
}

fn config_key_reschedules(key: &str) -> bool {
    matches!(
        key,
        "interval"
            | "randomize"
            | "jitter_pct"
            | "hold_window_secs"
            | "send_without_focus"
            | "hold_while_playing"
            | "manual_mode"
            | "action"
            | "key_sequence"
            | "sensitivity"
    )
}

fn is_valid_tab(tab: &str) -> bool {
    matches!(tab, "general" | "targets" | "stats" | "settings" | "about")
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

fn hhmm_value(value: Value, key: &str) -> Result<String, String> {
    let raw = string_value(value, key)?;
    let raw = raw.trim().to_string();
    if parse_hhmm(&raw).is_none() {
        return Err(format!(
            "Couldn't save setting '{key}' - use a 24-hour HH:MM time to fix this."
        ));
    }
    Ok(raw)
}

fn parse_action(value: &str) -> Result<KeepaliveAction, String> {
    match value {
        "Space tap" => Ok(KeepaliveAction::SpaceTap),
        "W tap" => Ok(KeepaliveAction::WTap),
        "Camera nudge" => Ok(KeepaliveAction::CameraNudge),
        "Mouse wiggle" => Ok(KeepaliveAction::MouseWiggle),
        "Scroll tick" => Ok(KeepaliveAction::ScrollTick),
        "Right click" => Ok(KeepaliveAction::RightClick),
        "Key sequence…" => Ok(KeepaliveAction::KeySequence),
        "Per-target…" => Ok(KeepaliveAction::PerTarget),
        _ => Err(
            "Couldn't set action - choose Space tap, W tap, Camera nudge, Mouse wiggle, Scroll tick, Right click, Key sequence…, or Per-target… to fix this."
                .to_string(),
        ),
    }
}

fn parse_target_action(value: &str) -> Result<TargetAction, String> {
    match value {
        "Space tap" => Ok(TargetAction::SpaceTap),
        "W tap" => Ok(TargetAction::WTap),
        "Camera nudge" => Ok(TargetAction::CameraNudge),
        "Mouse wiggle" => Ok(TargetAction::MouseWiggle),
        "Scroll tick" => Ok(TargetAction::ScrollTick),
        "Right click" => Ok(TargetAction::RightClick),
        "Key sequence…" => Ok(TargetAction::KeySequence),
        _ => Err(
            "Couldn't set profile action - choose Space tap, W tap, Camera nudge, Mouse wiggle, Scroll tick, Right click, or Key sequence… to fix this."
                .to_string(),
        ),
    }
}

fn parse_target_sort(value: &str) -> Result<TargetSort, String> {
    match value {
        "Status" => Ok(TargetSort::Status),
        "Name" => Ok(TargetSort::Name),
        _ => Err("Couldn't set targets sort - choose Status or Name to fix this.".to_string()),
    }
}

fn parse_monitor_when(value: &str) -> Result<MonitorWhen, String> {
    match value {
        "Always" => Ok(MonitorWhen::Always),
        "On launch" => Ok(MonitorWhen::OnLaunch),
        _ => {
            Err("Couldn't set monitor timing - choose Always or On launch to fix this.".to_string())
        }
    }
}

fn parse_monitor_style(value: &str) -> Result<MonitorStyle, String> {
    match value {
        "Preserve size" => Ok(MonitorStyle::Preserve),
        "Maximize" => Ok(MonitorStyle::Maximize),
        "Fill work area" => Ok(MonitorStyle::FillWorkArea),
        "Fill monitor" => Ok(MonitorStyle::FillMonitor),
        _ => Err(
            "Couldn't set monitor placement style - choose Preserve size, Maximize, Fill work area, or Fill monitor to fix this."
                .to_string(),
        ),
    }
}

fn parse_accent(value: &str) -> Result<Accent, String> {
    match value {
        "Mono" => Ok(Accent::Mono),
        "Ice" => Ok(Accent::Ice),
        "Ember" => Ok(Accent::Ember),
        "Acid" => Ok(Accent::Acid),
        "Violet" => Ok(Accent::Violet),
        _ => Err(
            "Couldn't set accent - choose Mono, Ice, Ember, Acid, or Violet to fix this."
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

fn parse_target_view(value: &str) -> Result<TargetView, String> {
    match value {
        "Clean" => Ok(TargetView::Clean),
        "All" => Ok(TargetView::All),
        "Games only" => Ok(TargetView::GamesOnly),
        _ => Err(
            "Couldn't set targets view - choose Clean, All, or Games only to fix this.".to_string(),
        ),
    }
}

fn parse_target_density(value: &str) -> Result<TargetDensity, String> {
    match value {
        "Compact" => Ok(TargetDensity::Compact),
        "Comfortable" => Ok(TargetDensity::Comfortable),
        _ => Err(
            "Couldn't set targets density - choose Compact or Comfortable to fix this.".to_string(),
        ),
    }
}

fn parse_tab_label_mode(value: &str) -> Result<TabLabelMode, String> {
    match value {
        "Active only" => Ok(TabLabelMode::ActiveOnly),
        "Always" => Ok(TabLabelMode::Always),
        "Icons only" => Ok(TabLabelMode::IconsOnly),
        _ => Err(
            "Couldn't set tab labels - choose Active only, Always, or Icons only to fix this."
                .to_string(),
        ),
    }
}

fn parse_version_display(value: &str) -> Result<VersionDisplay, String> {
    match value {
        "Title + About" => Ok(VersionDisplay::TitleAndAbout),
        "About only" => Ok(VersionDisplay::AboutOnly),
        "Hidden" => Ok(VersionDisplay::Hidden),
        _ => Err(
            "Couldn't set version display - choose Title + About, About only, or Hidden to fix this."
                .to_string(),
        ),
    }
}

fn parse_safety_note_display(value: &str) -> Result<SafetyNoteDisplay, String> {
    match value {
        "Compact" => Ok(SafetyNoteDisplay::Compact),
        "Full" => Ok(SafetyNoteDisplay::Full),
        "Hidden" => Ok(SafetyNoteDisplay::Hidden),
        _ => Err(
            "Couldn't set safety note - choose Compact, Full, or Hidden to fix this.".to_string(),
        ),
    }
}

fn parse_update_prompt_mode(value: &str) -> Result<UpdatePromptMode, String> {
    match value {
        "Card + toast" => Ok(UpdatePromptMode::CardAndToast),
        "Card only" => Ok(UpdatePromptMode::CardOnly),
        "Manual only" => Ok(UpdatePromptMode::ManualOnly),
        _ => Err(
            "Couldn't set update prompts - choose Card + toast, Card only, or Manual only to fix this."
                .to_string(),
        ),
    }
}

fn parse_update_channel(value: &str) -> Result<UpdateChannel, String> {
    match value {
        "Stable" => Ok(UpdateChannel::Stable),
        _ => Err("Couldn't set update channel - OMNAFK only uses Stable releases.".to_string()),
    }
}
