//! Named setting bundles for common play styles.

use crate::config::{AppConfig, KeepaliveAction, MonitorStyle};

pub const PRESET_NAMES: &[&str] = &["Walking simulator", "Long interval (Space)", "Camera AFK"];

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
        "Long interval (Space)" | "Roblox" => {
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
        "Camera AFK" => {
            config.interval = 180;
            config.action = KeepaliveAction::CameraNudge;
            config.adaptive_actions = false;
            config.send_without_focus = false;
            config.hold_while_playing = false;
            config.auto_fallback = true;
        }
        _ => {
            return Err(
                "Couldn't apply preset - choose Walking simulator, Long interval (Space), or Camera AFK to fix this."
                    .to_string(),
            );
        }
    }
    Ok(())
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
        assert_eq!(config.action, KeepaliveAction::SpaceTap);
        assert_eq!(config.interval, 540);
    }
}
