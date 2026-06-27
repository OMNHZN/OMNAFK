use crate::{
    config::{
        parse_hhmm, validate_key_sequence, AppConfig, GamepadKind, KeepaliveAction, MonitorStyle,
        MonitorWhen, NotificationLevel, QuietDays, SafetyNoteDisplay, Sensitivity, TabLabelMode,
        TargetAction, TargetDensity, TargetSort, TargetView, Theme, UpdatePromptMode,
        VersionDisplay,
    },
    updates,
};
use serde_json::Value;

pub fn apply_config_value(config: &mut AppConfig, key: &str, value: Value) -> Result<(), String> {
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
        "auto_fallback" => config.auto_fallback = bool_value(value, key)?,
        "adaptive_learn_sequences" => config.adaptive_learn_sequences = bool_value(value, key)?,
        "adaptive_learn_actions" => config.adaptive_learn_actions = bool_value(value, key)?,
        "adaptive_interval" => config.adaptive_interval = bool_value(value, key)?,
        "burst_detection" => config.burst_detection = bool_value(value, key)?,
        "keep_all_instances" => config.keep_all_instances = bool_value(value, key)?,
        "rotate_actions" => config.rotate_actions = bool_value(value, key)?,
        "gamepad_kind" => {
            config.gamepad_kind = parse_gamepad_kind(string_value(value, key)?.as_str())?;
            crate::gamepad_send::set_kind(config.gamepad_kind);
        }
        "headless" => config.headless = bool_value(value, key)?,
        "community_intelligence" => {
            config.community_intelligence = bool_value(value, key)?;
            if config.community_intelligence {
                crate::community::ensure_client_id(config);
            }
        }
        "auto_elevate" => config.auto_elevate = bool_value(value, key)?,
        "adaptive_min_samples" => {
            let samples = value.as_u64().ok_or_else(|| {
                "Couldn't set adaptive sample threshold - choose a number to fix this.".to_string()
            })?;
            if !(10..=500).contains(&samples) {
                return Err(
                    "Couldn't set adaptive sample threshold - choose 10 to 500 to fix this."
                        .to_string(),
                );
            }
            config.adaptive_min_samples = samples;
        }
        "favorite_targets" => config.favorite_targets = parse_string_list(value, key)?,
        "always_mark_exes" => config.always_mark_exes = parse_lowercase_list(value, key)?,
        "always_ignore_exes" => config.always_ignore_exes = parse_lowercase_list(value, key)?,
        "mark_title_contains" => config.mark_title_contains = parse_lowercase_list(value, key)?,
        "ignore_title_contains" => config.ignore_title_contains = parse_lowercase_list(value, key)?,
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
        "quiet_days" => config.quiet_days = parse_quiet_days(string_value(value, key)?.as_str())?,
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
        "theme" => config.theme = parse_theme(string_value(value, key)?.as_str())?,
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
        "remote_alerts" => config.remote_alerts = bool_value(value, key)?,
        "ntfy_topic" => config.ntfy_topic = string_value(value, key)?.trim().to_string(),
        "discord_webhook" => config.discord_webhook = string_value(value, key)?.trim().to_string(),
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

pub fn config_key_reschedules(key: &str) -> bool {
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
            | "adaptive_interval"
            | "keep_all_instances"
    )
}

fn is_valid_tab(tab: &str) -> bool {
    matches!(tab, "general" | "targets" | "stats" | "settings" | "about")
}

/// Parse a JSON array of strings, preserving case (used for target identity
/// keys, which are case-sensitive `exe\u{1f}wclass` pairs).
fn parse_string_list(value: Value, key: &str) -> Result<Vec<String>, String> {
    let items = value
        .as_array()
        .ok_or_else(|| format!("Couldn't set {key} - expected a list."))?;
    Ok(items
        .iter()
        .filter_map(|item| item.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

/// Parse a JSON array or comma-separated string into trimmed, lowercased,
/// non-empty entries (used for exe and title-rule lists).
fn parse_lowercase_list(value: Value, key: &str) -> Result<Vec<String>, String> {
    if let Some(items) = value.as_array() {
        Ok(items
            .iter()
            .filter_map(|item| item.as_str())
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect())
    } else {
        Ok(string_value(value, key)?
            .split(',')
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect())
    }
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
        "Gamepad nudge" => Ok(KeepaliveAction::GamepadNudge),
        "Key sequence…" => Ok(KeepaliveAction::KeySequence),
        "Per-target…" => Ok(KeepaliveAction::PerTarget),
        _ => Err(
            "Couldn't set action - choose Space tap, W tap, Camera nudge, Mouse wiggle, Scroll tick, Right click, Gamepad nudge, Key sequence…, or Per-target… to fix this."
                .to_string(),
        ),
    }
}

pub fn parse_target_action(value: &str) -> Result<TargetAction, String> {
    match value {
        "Space tap" => Ok(TargetAction::SpaceTap),
        "W tap" => Ok(TargetAction::WTap),
        "Camera nudge" => Ok(TargetAction::CameraNudge),
        "Mouse wiggle" => Ok(TargetAction::MouseWiggle),
        "Scroll tick" => Ok(TargetAction::ScrollTick),
        "Right click" => Ok(TargetAction::RightClick),
        "Gamepad nudge" => Ok(TargetAction::GamepadNudge),
        "Key sequence…" => Ok(TargetAction::KeySequence),
        _ => Err(
            "Couldn't set profile action - choose Space tap, W tap, Camera nudge, Mouse wiggle, Scroll tick, Right click, Gamepad nudge, or Key sequence… to fix this."
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

/// Parse a sensitivity label from the per-target profile editor.
pub fn parse_sensitivity_label(value: &str) -> Result<Sensitivity, String> {
    parse_sensitivity(value)
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

fn parse_gamepad_kind(value: &str) -> Result<GamepadKind, String> {
    match value {
        "Xbox 360" => Ok(GamepadKind::Xbox360),
        "DualShock 4" => Ok(GamepadKind::DualShock4),
        _ => Err(
            "Couldn't set gamepad type - choose Xbox 360 or DualShock 4 to fix this.".to_string(),
        ),
    }
}

fn parse_quiet_days(value: &str) -> Result<QuietDays, String> {
    match value {
        "Every day" => Ok(QuietDays::EveryDay),
        "Weekdays" => Ok(QuietDays::Weekdays),
        "Weekends" => Ok(QuietDays::Weekends),
        _ => Err(
            "Couldn't set quiet days - choose Every day, Weekdays, or Weekends to fix this."
                .to_string(),
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

fn parse_theme(value: &str) -> Result<Theme, String> {
    match value {
        "Dark" => Ok(Theme::Dark),
        "High contrast" => Ok(Theme::HighContrast),
        _ => Err("Couldn't set theme - choose Dark or High contrast to fix this.".to_string()),
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
        "Automatic" => Ok(UpdatePromptMode::Automatic),
        _ => Err(
            "Couldn't set update prompts - choose Card + toast, Card only, Manual only, or Automatic to fix this."
                .to_string(),
        ),
    }
}
