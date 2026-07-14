mod config_set;

use crate::{
    config::{
        self, validate_key_sequence, AppConfig, KeepaliveAction, MonitorStyle, MonitorWhen,
        NotificationLevel, OverrideVerdict, SafetyNoteDisplay, Sensitivity, TabLabelMode,
        TargetDensity, TargetSort, TargetView, Theme, UpdatePromptMode, VersionDisplay,
    },
    engine::{ActivityEvent, EngineStatus, GameSnapshot, SharedEngine},
    flyout, monitor,
    notifications::{self, ToastAction},
    presets::{self, PRESET_NAMES},
    startup,
    stats::StatsSnapshot,
    updates,
};
use serde::Serialize;
use serde_json::Value;
use std::{
    fs,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use windows::{
    core::HSTRING,
    Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
};

const STATE_EVENT: &str = "omnafk://state";
const UPDATE_CHECK_EVENT: &str = "omnafk://update-check";
static UPDATE_CHECK_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize)]
pub struct UpdateCheckStarted {
    pub started: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UpdateCheckEvent {
    Result { check: Box<updates::UpdateCheck> },
    Error { message: String },
}

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub community_last_error: Option<String>,
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
    pub auto_fallback: bool,
    pub adaptive_min_samples: u64,
    pub adaptive_learn_sequences: bool,
    pub adaptive_learn_actions: bool,
    pub burst_detection: bool,
    pub headless: bool,
    pub community_intelligence: bool,
    pub presence_log_enabled: bool,
    pub presence_screen_enabled: bool,
    pub presence_memory_enabled: bool,
    pub respect_presence: bool,
    pub auto_elevate: bool,
    pub always_mark_exes: Vec<String>,
    pub always_ignore_exes: Vec<String>,
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
    pub autostart_status: String,
    pub user_presets: Vec<String>,
    pub show_on_launch: bool,
    pub remember_pin: bool,
    pub notifications: NotificationLevel,
    pub remote_alerts: bool,
    pub ntfy_topic: String,
    pub discord_webhook: String,
    pub hotkey: String,
    pub suspend_hotkey: String,
    pub github_repo: String,
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
    pub favorite_targets: Vec<String>,
    pub tab_label_mode: TabLabelMode,
    pub theme: Theme,
    pub version_display: VersionDisplay,
    pub safety_note_display: SafetyNoteDisplay,
    pub update_prompt_mode: UpdatePromptMode,
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
            auto_fallback: config.auto_fallback,
            adaptive_min_samples: config.adaptive_min_samples,
            adaptive_learn_sequences: config.adaptive_learn_sequences,
            adaptive_learn_actions: config.adaptive_learn_actions,
            burst_detection: config.burst_detection,
            headless: config.headless,
            community_intelligence: config.community_intelligence,
            presence_log_enabled: config.presence_log_enabled,
            presence_screen_enabled: config.presence_screen_enabled,
            presence_memory_enabled: config.presence_memory_enabled,
            respect_presence: config.respect_presence,
            auto_elevate: config.auto_elevate,
            always_mark_exes: config.always_mark_exes.clone(),
            always_ignore_exes: config.always_ignore_exes.clone(),
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
            autostart_status: autostart_status_string(config),
            user_presets: presets::user_preset_names(config),
            show_on_launch: config.show_on_launch,
            remember_pin: config.remember_pin,
            notifications: config.notifications,
            remote_alerts: config.remote_alerts,
            ntfy_topic: config.ntfy_topic.clone(),
            discord_webhook: config.discord_webhook.clone(),
            hotkey: config.hotkey.clone(),
            suspend_hotkey: config.suspend_hotkey.clone(),
            github_repo: config.github_repo.clone(),
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
            favorite_targets: config.favorite_targets.clone(),
            tab_label_mode: config.tab_label_mode,
            theme: config.theme,
            version_display: config.version_display,
            safety_note_display: config.safety_note_display,
            update_prompt_mode: config.update_prompt_mode,
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
pub fn list_presets() -> Vec<&'static str> {
    PRESET_NAMES.to_vec()
}

#[tauri::command]
pub fn apply_preset(
    name: String,
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    let payload = mutate_config_with_reschedule(&app, engine.inner(), true, |config| {
        presets::apply_preset(config, &name)
    })?;
    Ok(payload)
}

#[tauri::command]
pub fn save_user_preset(
    name: String,
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    mutate_config(&app, engine.inner(), |config| {
        presets::save_user_preset(config, &name)
    })
}

#[tauri::command]
pub fn apply_user_preset(
    name: String,
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    mutate_config_with_reschedule(&app, engine.inner(), true, |config| {
        presets::apply_user_preset_by_name(config, &name)
    })
}

#[tauri::command]
pub fn delete_user_preset(
    name: String,
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    mutate_config(&app, engine.inner(), |config| {
        presets::delete_user_preset(config, &name)
    })
}

#[tauri::command]
pub fn dismiss_community_profile(
    exe: String,
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    mutate_config(&app, engine.inner(), |config| {
        let exe_key = exe.trim().to_ascii_lowercase();
        if exe_key.is_empty() {
            return Err("Couldn't dismiss community profile — exe is required.".to_string());
        }
        if !config
            .community_dismissed_exes
            .iter()
            .any(|d| d == &exe_key)
        {
            config.community_dismissed_exes.push(exe_key);
        }
        Ok(())
    })
}

#[tauri::command]
pub fn apply_community_profile(
    exe: String,
    wclass: String,
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    let exe_key = exe.trim().to_ascii_lowercase();
    if exe_key.is_empty() {
        return Err("Couldn't apply community profile - exe is required.".to_string());
    }
    let entry = {
        let community = engine.community().read();
        crate::community::game_entry(&community, &exe_key)
            .cloned()
            .ok_or_else(|| "No community profile is available for this target.".to_string())?
    };
    engine
        .community()
        .write()
        .applied_exes
        .insert(exe_key.clone());
    let payload = mutate_config(&app, engine.inner(), |config| {
        crate::community::set_game_profile(config, &exe_key, &wclass, &entry);
        crate::community::apply_global_hints(config, &entry);
        config.community_dismissed_exes.retain(|d| d != &exe_key);
        Ok(())
    })?;
    Ok(payload)
}

#[tauri::command]
pub fn community_feedback(
    exe: String,
    feedback: String,
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    let exe_key = exe.trim().to_ascii_lowercase();
    if exe_key.is_empty() {
        return Err("Couldn't record feedback - exe is required.".to_string());
    }
    crate::community::record_feedback(&exe_key, feedback.trim());
    emit_and_return(&app, engine.inner())
}

/// Open a prefilled "Community profile suggestion" issue for a game whose
/// settings worked, so users can contribute back the shared profile DB.
#[tauri::command]
pub fn share_community_profile(
    exe: String,
    game: String,
    action: Option<String>,
    interval: Option<String>,
    notes: Option<String>,
    engine: State<'_, SharedEngine>,
) -> Result<(), String> {
    let exe = exe.trim();
    let game = game.trim();
    if exe.is_empty() || game.is_empty() {
        return Err("Couldn't share profile - the game name and exe are required.".to_string());
    }
    let repo = engine.snapshot().config.github_repo;
    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    let action = action.unwrap_or_default();
    let interval = interval.unwrap_or_default();
    let notes = notes.unwrap_or_default();
    let url = updates::community_profile_url(
        &repo,
        &[
            ("game", game),
            ("exe", exe),
            ("action", action.trim()),
            ("interval", interval.trim()),
            ("notes", notes.trim()),
            ("version", &version),
        ],
    )?;
    updates::open_url(&url)
}

#[tauri::command]
pub fn move_target(
    exe: String,
    wclass: String,
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<StatePayload, String> {
    engine.move_target(&exe, &wclass)?;
    emit_and_return(&app, engine.inner())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElevationMode {
    Manual,
    Auto,
}

#[tauri::command]
pub fn restart_as_admin(app: AppHandle, engine: State<'_, SharedEngine>) -> Result<(), String> {
    if crate::detector::current_process_elevated() {
        return Err("OMNAFK is already running as administrator.".to_string());
    }
    restart_elevated(&app, &engine, ElevationMode::Manual)
}

pub fn restart_elevated(
    app: &AppHandle,
    engine: &SharedEngine,
    mode: ElevationMode,
) -> Result<(), String> {
    if crate::detector::current_process_elevated() {
        return Ok(());
    }

    if mode == ElevationMode::Auto && !engine.can_auto_elevate_now() {
        crate::startup_log::info(
            "auto-elevation deferred during startup grace (tray-only autostart launch)",
        );
        engine.clear_elevation_request();
        return Ok(());
    }

    let exe = std::env::current_exe().map_err(|error| {
        format!("Couldn't locate OMNAFK - reinstall the app to fix this: {error}")
    })?;
    let verb = HSTRING::from("runas");
    let file = HSTRING::from(exe.to_string_lossy().as_ref());
    let params = HSTRING::from(crate::elevation::elevation_command_line());
    let result = unsafe { ShellExecuteW(None, &verb, &file, &params, None, SW_SHOWNORMAL) };
    if result.0 as isize <= 32 {
        engine.clear_elevation_request();
        let message =
            "Couldn't restart as administrator - approve the UAC prompt or run OMNAFK as admin manually.";
        crate::startup_log::warn(format!("ShellExecuteW runas failed: {message}"));
        return Err(message.to_string());
    }

    crate::startup_log::info(format!(
        "requested elevation relaunch ({mode:?}); waiting for handoff instance"
    ));

    if mode == ElevationMode::Manual {
        let app = app.clone();
        let engine = engine.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(30));
            if !crate::detector::current_process_elevated() {
                engine.note_runtime_warning(
                    "Administrator restart was cancelled or didn't complete.",
                    true,
                );
                let _ = emit_state(&app, &engine);
            }
        });
    }

    Ok(())
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
        config_set::config_key_reschedules(&key),
        |config| config_set::apply_config_value(config, &key, value),
    )?;
    apply_live_config(&app, engine.inner(), &key, &payload)?;
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

/// Explain why a window is or isn't kept awake: the weighted score breakdown
/// plus the precedence rule behind its effective verdict. Computed on demand so
/// it adds no cost to the per-tick state pump.
#[tauri::command]
pub fn explain_detection(
    exe: String,
    wclass: String,
    engine: State<'_, SharedEngine>,
) -> Result<crate::engine::DetectionExplanation, String> {
    engine
        .explain_detection(&exe, &wclass)
        .ok_or_else(|| format!("Couldn't explain {exe} - the window is no longer being tracked."))
}

/// Send a test away-from-keyboard alert to the configured ntfy/Discord
/// channels, so the user can confirm delivery works before relying on it.
#[tauri::command]
pub fn test_alert(engine: State<'_, SharedEngine>) -> Result<String, String> {
    crate::alerts::send_test(&engine.snapshot().config)
}

/// Fire a test keepalive at every active target and return pass/fail counts,
/// so the user can confirm everything works before going AFK.
#[tauri::command]
pub fn test_all_targets(
    app: AppHandle,
    engine: State<'_, SharedEngine>,
) -> Result<crate::engine::TestAllResult, String> {
    let result = engine.test_all_targets();
    let _ = emit_state(&app, engine.inner());
    Ok(result)
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
    adaptive: Option<bool>,
    hold_while_playing: Option<bool>,
    hold_window_secs: Option<u64>,
    send_without_focus: Option<bool>,
    auto_fallback: Option<bool>,
    sensitivity: Option<String>,
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
            Some(label) => Some(config_set::parse_target_action(label)?),
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

        profile.sensitivity = match sensitivity.as_deref() {
            None | Some("") | Some("Use global") => None,
            Some(label) => Some(config_set::parse_sensitivity_label(label)?),
        };

        profile.adaptive = adaptive;
        profile.hold_while_playing = hold_while_playing;
        profile.send_without_focus = send_without_focus;
        profile.auto_fallback = auto_fallback;
        profile.hold_window_secs = match hold_window_secs {
            Some(secs) if (10..=3600).contains(&secs) => Some(secs),
            Some(_) => {
                return Err(
                    "Couldn't set hold window - choose 10 to 3600 seconds to fix this.".to_string(),
                );
            }
            None => None,
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
pub fn get_tray_menu_state(engine: State<'_, SharedEngine>) -> crate::tray_menu::TrayMenuState {
    crate::tray_menu::tray_menu_state(&engine.snapshot())
}

#[tauri::command]
pub fn tray_menu_action(action: String, app: AppHandle, engine: State<'_, SharedEngine>) {
    crate::tray::execute_action(&app, engine.inner(), &action);
}

#[tauri::command]
pub fn hide_tray_menu(app: AppHandle) -> Result<(), String> {
    crate::tray_menu::hide(&app).map_err(|error| {
        format!("Couldn't hide the tray menu - restart OMNAFK to fix this: {error}")
    })
}

#[tauri::command]
pub fn toast_action(action: ToastAction, app: AppHandle) -> Result<(), String> {
    notifications::run_toast_action(&app, action)
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
    apply_all_live_config(&app, engine.inner(), &payload)?;
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
    apply_all_live_config(&app, engine.inner(), &payload)?;
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
    updates::changelog(&config.github_repo, env!("CARGO_PKG_VERSION"))
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
) -> Result<UpdateCheckStarted, String> {
    if UPDATE_CHECK_IN_FLIGHT.swap(true, Ordering::SeqCst) {
        return Ok(UpdateCheckStarted { started: false });
    }

    let shared = engine.inner().clone();
    let config = shared.snapshot().config;
    let repo = config.github_repo.clone();
    let ignored = config.ignored_update_tag.clone();
    let current = env!("CARGO_PKG_VERSION").to_string();

    thread::spawn(move || {
        let event = match updates::check(&repo, &current) {
            Ok(check) => {
                if check.update_available && ignored.as_deref() != Some(check.latest_tag.as_str()) {
                    shared.set_update_prompt(Some(check.clone()));
                } else {
                    shared.set_update_prompt(None);
                }
                UpdateCheckEvent::Result {
                    check: Box::new(check),
                }
            }
            Err(message) => UpdateCheckEvent::Error { message },
        };
        UPDATE_CHECK_IN_FLIGHT.store(false, Ordering::SeqCst);
        let _ = app.emit(UPDATE_CHECK_EVENT, event);
        let _ = emit_state(&app, &shared);
    });

    Ok(UpdateCheckStarted { started: true })
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

#[tauri::command]
pub fn run_app_update(app: AppHandle, engine: State<'_, SharedEngine>) -> Result<(), String> {
    install_pending_update(&app, engine.inner())
}

static INSTALL_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

/// Download, verify, install, and relaunch the pending update, then exit.
/// Shared by the manual "Update now" button and the automatic update path.
/// Requires the update prompt to be set (so `snapshot.update` carries the
/// release to install). Guarded so periodic checks and a manual click cannot
/// launch two installers for the same release.
pub(crate) fn install_pending_update(app: &AppHandle, engine: &SharedEngine) -> Result<(), String> {
    if INSTALL_IN_FLIGHT.swap(true, Ordering::SeqCst) {
        return Ok(());
    }

    let outcome = run_pending_update_install(app, engine);
    if outcome.is_err() {
        INSTALL_IN_FLIGHT.store(false, Ordering::SeqCst);
    }
    outcome
}

fn run_pending_update_install(app: &AppHandle, engine: &SharedEngine) -> Result<(), String> {
    let snapshot = engine.snapshot();
    let check = snapshot
        .update
        .filter(|check| check.update_available)
        .ok_or_else(|| "No pending update. Check for updates in Settings first.".to_string())?;
    let asset_url = check
        .asset_url
        .as_deref()
        .ok_or_else(|| "This release has no installer download.".to_string())?;

    let path = updates::download_setup_installer(
        &check.repo,
        asset_url,
        &check.latest_tag,
        env!("CARGO_PKG_VERSION"),
    )?;
    // Launch the installer *before* stopping the engine: if the launch fails
    // (AV block, exec error), we return the error with keepalives still running
    // instead of leaving a live app with a dead engine.
    updates::launch_setup_installer(&path)?;
    engine.stop();
    app.exit(0);
    Ok(())
}

pub fn spawn_state_pump(app: AppHandle, engine: SharedEngine) {
    thread::spawn(move || loop {
        thread::sleep(Duration::from_secs(1));

        if catch_unwind(AssertUnwindSafe(|| {
            if engine.detection_stale() {
                let snapshot = engine.snapshot();
                if !snapshot.config.suspended && snapshot.snooze_remaining.is_none() {
                    engine.note_runtime_warning(
                        "Detection loop stalled — restarting the engine worker.",
                        true,
                    );
                    engine.ensure_worker_running();
                }
            }

            if engine.take_pending_elevation() {
                if let Err(error) = restart_elevated(&app, &engine, ElevationMode::Auto) {
                    engine.note_runtime_warning(format!("Auto-elevation failed: {error}"), true);
                }
            }

            crate::tray::ensure_installed(&app, &engine);

            for notice in engine.take_notices() {
                notifications::deliver(&app, &notice);
            }

            let Some(window) = app.get_webview_window("flyout") else {
                return;
            };
            if window.is_visible().unwrap_or(false) {
                let _ = emit_state(&app, &engine);
            }
        }))
        .is_err()
        {
            engine.note_runtime_warning("UI refresh recovered after an internal error.", false);
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

fn autostart_status_string(config: &AppConfig) -> String {
    std::env::current_exe()
        .map(|exe| {
            startup::autostart_status(config.autostart, &exe)
                .as_str()
                .to_string()
        })
        .unwrap_or_else(|_| startup::AutostartStatus::Missing.as_str().to_string())
}

pub fn sync_autostart(engine: &SharedEngine, enabled: bool) -> Result<(), String> {
    let exe = std::env::current_exe()
        .map_err(|error| format!("Couldn't find OMNAFK executable: {error}"))?;
    match startup::ensure_autostart(enabled, &exe) {
        Err(error) => {
            engine
                .note_runtime_warning(format!("Couldn't update Start with Windows: {error}"), true);
            Err(format!(
                "Couldn't update Start with Windows - check Windows startup permissions to fix this: {error}"
            ))
        }
        Ok(startup::AutostartStatus::Mismatch) => {
            engine.note_runtime_warning(
                "Start with Windows path mismatch — OMNAFK retried registration.".to_string(),
                false,
            );
            Ok(())
        }
        Ok(_) => Ok(()),
    }
}

fn apply_all_live_config(
    app: &AppHandle,
    engine: &SharedEngine,
    payload: &StatePayload,
) -> Result<(), String> {
    sync_autostart(engine, payload.config.autostart)?;
    flyout::register_hotkeys(app, &payload.config.hotkey, &payload.config.suspend_hotkey)?;
    Ok(())
}

fn apply_live_config(
    app: &AppHandle,
    engine: &SharedEngine,
    key: &str,
    payload: &StatePayload,
) -> Result<(), String> {
    match key {
        "autostart" => sync_autostart(engine, payload.config.autostart)?,
        "hotkey" | "suspend_hotkey" => {
            flyout::register_hotkeys(app, &payload.config.hotkey, &payload.config.suspend_hotkey)?
        }
        _ => {}
    }
    Ok(())
}

fn state_payload(engine: &SharedEngine) -> StatePayload {
    let snapshot = engine.snapshot();
    let community_last_error = if snapshot.config.community_intelligence {
        engine.community().read().last_error.clone()
    } else {
        None
    };
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
        community_last_error,
    }
}
