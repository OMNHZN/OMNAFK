//! Named setting bundles for common play styles.

use crate::config::{AppConfig, KeepaliveAction, MonitorStyle, UserPreset, MAX_USER_PRESETS};

pub const PRESET_NAMES: &[&str] = &[
    "Walking simulator",
    "Long interval (Space)",
    "Camera AFK",
    "Mouse only",
];

pub fn apply_preset(config: &mut AppConfig, name: &str) -> Result<(), String> {
    match name {
        "Walking simulator" => {
            config.interval = 120;
            config.action = KeepaliveAction::WTap;
            config.adaptive_actions = true;
            config.adaptive_min_samples = 30;
            config.adaptive_learn_sequences = true;
            config.adaptive_learn_actions = true;
            config.send_without_focus = false;
            config.hold_while_playing = true;
            config.auto_fallback = true;
        }
        "Long interval (Space)" => {
            config.interval = 540;
            config.action = KeepaliveAction::SpaceTap;
            config.adaptive_actions = true;
            config.adaptive_min_samples = 50;
            config.adaptive_learn_sequences = true;
            config.adaptive_learn_actions = true;
            config.send_without_focus = false;
            config.hold_while_playing = true;
            config.auto_fallback = true;
            config.monitor_style = MonitorStyle::Preserve;
        }
        // Legacy alias. Roblox's 20-minute AFK timer resets on any input, but
        // key taps move the character and cancel emotes — so this preset now
        // sticks to a pointer-only wiggle instead of Space.
        "Roblox" => {
            config.interval = 540;
            config.action = KeepaliveAction::MouseWiggle;
            config.adaptive_actions = false;
            config.send_without_focus = false;
            config.hold_while_playing = true;
            config.auto_fallback = true;
            config.monitor_style = MonitorStyle::Preserve;
        }
        "Camera AFK" => {
            config.interval = 180;
            config.action = KeepaliveAction::CameraNudge;
            config.adaptive_actions = false;
            config.send_without_focus = false;
            config.hold_while_playing = false;
            config.auto_fallback = true;
        }
        "Mouse only" => {
            // Mouse jiggle for menus, lobbies, and idle games that ignore
            // keyboard input but register a tiny pointer move.
            config.interval = 180;
            config.action = KeepaliveAction::MouseWiggle;
            config.adaptive_actions = false;
            config.send_without_focus = false;
            config.hold_while_playing = true;
            config.auto_fallback = true;
        }
        _ => {
            return Err(
                "Couldn't apply preset - choose Walking simulator, Long interval (Space), Camera AFK, or Mouse only to fix this."
                    .to_string(),
            );
        }
    }
    Ok(())
}

pub fn snapshot_user_preset(config: &AppConfig, name: &str) -> UserPreset {
    UserPreset {
        name: name.to_string(),
        interval: Some(config.interval),
        action: Some(config.action),
        key_sequence: config.key_sequence.clone(),
        randomize: Some(config.randomize),
        jitter_pct: Some(config.jitter_pct),
        hold_while_playing: Some(config.hold_while_playing),
        hold_window_secs: Some(config.hold_window_secs),
        adaptive_actions: Some(config.adaptive_actions),
        auto_fallback: Some(config.auto_fallback),
        send_without_focus: Some(config.send_without_focus),
        monitor_placement: Some(config.monitor_placement),
        monitor_style: Some(config.monitor_style),
        monitor_when: Some(config.monitor_when),
        monitor_skip_active: Some(config.monitor_skip_active),
        monitor_skip_active_secs: Some(config.monitor_skip_active_secs),
    }
}

pub fn apply_user_preset(config: &mut AppConfig, preset: &UserPreset) -> Result<(), String> {
    if let Some(interval) = preset.interval {
        if !(10..=3600).contains(&interval) {
            return Err(
                "Couldn't apply preset - interval must be between 10 and 3600 seconds.".to_string(),
            );
        }
        config.interval = interval;
    }
    if let Some(action) = preset.action {
        config.action = action;
    }
    config.key_sequence = preset.key_sequence.clone();
    if let Some(randomize) = preset.randomize {
        config.randomize = randomize;
    }
    if let Some(jitter_pct) = preset.jitter_pct {
        config.jitter_pct = jitter_pct;
    }
    if let Some(hold_while_playing) = preset.hold_while_playing {
        config.hold_while_playing = hold_while_playing;
    }
    if let Some(hold_window_secs) = preset.hold_window_secs {
        config.hold_window_secs = hold_window_secs;
    }
    if let Some(adaptive_actions) = preset.adaptive_actions {
        config.adaptive_actions = adaptive_actions;
    }
    if let Some(auto_fallback) = preset.auto_fallback {
        config.auto_fallback = auto_fallback;
    }
    if let Some(send_without_focus) = preset.send_without_focus {
        config.send_without_focus = send_without_focus;
    }
    if let Some(monitor_placement) = preset.monitor_placement {
        config.monitor_placement = monitor_placement;
    }
    if let Some(monitor_style) = preset.monitor_style {
        config.monitor_style = monitor_style;
    }
    if let Some(monitor_when) = preset.monitor_when {
        config.monitor_when = monitor_when;
    }
    if let Some(monitor_skip_active) = preset.monitor_skip_active {
        config.monitor_skip_active = monitor_skip_active;
    }
    if let Some(secs) = preset.monitor_skip_active_secs {
        if (1..=60).contains(&secs) {
            config.monitor_skip_active_secs = secs;
        }
    }
    Ok(())
}

pub fn user_preset_names(config: &AppConfig) -> Vec<String> {
    config
        .user_presets
        .iter()
        .map(|preset| preset.name.clone())
        .collect()
}

pub fn save_user_preset(config: &mut AppConfig, name: &str) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Couldn't save preset - enter a name.".to_string());
    }
    if PRESET_NAMES
        .iter()
        .any(|built_in| built_in.eq_ignore_ascii_case(name))
    {
        return Err(
            "Couldn't save preset - that name is reserved for a built-in preset.".to_string(),
        );
    }
    let snapshot = snapshot_user_preset(config, name);
    if let Some(existing) = config
        .user_presets
        .iter_mut()
        .find(|preset| preset.name.eq_ignore_ascii_case(name))
    {
        *existing = snapshot;
        return Ok(());
    }
    if config.user_presets.len() >= MAX_USER_PRESETS {
        return Err(format!(
            "Couldn't save preset - delete one first (max {MAX_USER_PRESETS})."
        ));
    }
    config.user_presets.push(snapshot);
    Ok(())
}

pub fn delete_user_preset(config: &mut AppConfig, name: &str) -> Result<(), String> {
    let before = config.user_presets.len();
    config
        .user_presets
        .retain(|preset| !preset.name.eq_ignore_ascii_case(name.trim()));
    if config.user_presets.len() == before {
        return Err("Couldn't delete preset - name not found.".to_string());
    }
    Ok(())
}

pub fn apply_user_preset_by_name(config: &mut AppConfig, name: &str) -> Result<(), String> {
    let preset = config
        .user_presets
        .iter()
        .find(|preset| preset.name.eq_ignore_ascii_case(name.trim()))
        .cloned()
        .ok_or_else(|| "Couldn't apply preset - name not found.".to_string())?;
    apply_user_preset(config, &preset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preset_names_are_behavior_not_game_titles() {
        assert!(!PRESET_NAMES
            .iter()
            .any(|name| name.eq_ignore_ascii_case("roblox")));
    }

    #[test]
    fn legacy_roblox_preset_alias_still_applies() {
        let mut config = AppConfig::default();
        apply_preset(&mut config, "Roblox").expect("legacy alias");
        // Pointer-only: key taps move/emote a Roblox character.
        assert_eq!(config.action, KeepaliveAction::MouseWiggle);
        assert_eq!(config.interval, 540);
        assert!(!config.adaptive_actions);
    }

    #[test]
    fn mouse_only_preset_uses_cursor_wiggle() {
        let mut config = AppConfig::default();
        apply_preset(&mut config, "Mouse only").expect("apply mouse only");
        assert_eq!(config.action, KeepaliveAction::MouseWiggle);
        assert_eq!(config.interval, 180);
        assert!(!config.adaptive_actions);
        assert!(config.hold_while_playing);
    }

    #[test]
    fn user_preset_roundtrip() {
        let mut config = AppConfig {
            interval: 180,
            action: KeepaliveAction::CameraNudge,
            ..Default::default()
        };
        save_user_preset(&mut config, "Test").expect("save");
        config.interval = 540;
        apply_user_preset_by_name(&mut config, "Test").expect("apply");
        assert_eq!(config.interval, 180);
        assert_eq!(config.action, KeepaliveAction::CameraNudge);
    }

    #[test]
    fn user_preset_captures_monitor_placement() {
        use crate::config::{MonitorStyle, MonitorWhen};
        let mut config = AppConfig {
            monitor_placement: true,
            monitor_style: MonitorStyle::FillMonitor,
            monitor_when: MonitorWhen::OnLaunch,
            monitor_skip_active: false,
            monitor_skip_active_secs: 30,
            ..Default::default()
        };
        save_user_preset(&mut config, "Big screen").expect("save");

        // Mutate, then re-apply the preset and confirm the monitor block returns.
        config.monitor_placement = false;
        config.monitor_style = MonitorStyle::Preserve;
        config.monitor_when = MonitorWhen::Always;
        config.monitor_skip_active = true;
        config.monitor_skip_active_secs = 5;
        apply_user_preset_by_name(&mut config, "Big screen").expect("apply");

        assert!(config.monitor_placement);
        assert_eq!(config.monitor_style, MonitorStyle::FillMonitor);
        assert_eq!(config.monitor_when, MonitorWhen::OnLaunch);
        assert!(!config.monitor_skip_active);
        assert_eq!(config.monitor_skip_active_secs, 30);
    }

    #[test]
    fn user_preset_can_clear_key_sequence() {
        let mut config = AppConfig::default();
        save_user_preset(&mut config, "No sequence").expect("save");
        config.key_sequence = vec!["W".to_string(), "SPACE".to_string()];
        apply_user_preset_by_name(&mut config, "No sequence").expect("apply");
        assert!(config.key_sequence.is_empty());
    }
}
