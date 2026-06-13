//! Named setting bundles for common play styles.

use crate::config::{AppConfig, KeepaliveAction, MonitorStyle};

pub const PRESET_NAMES: &[&str] = &["Roblox", "Walking simulator", "Camera AFK"];

pub fn apply_preset(config: &mut AppConfig, name: &str) -> Result<(), String> {
    match name {
        "Roblox" => {
            config.interval = 540;
            config.action = KeepaliveAction::SpaceTap;
            config.adaptive_actions = true;
            config.adaptive_min_samples = 50;
            config.adaptive_learn_sequences = true;
            config.adaptive_learn_actions = true;
            config.send_without_focus = true;
            config.hold_while_playing = true;
            config.auto_fallback = true;
            config.monitor_style = MonitorStyle::Preserve;
        }
        "Walking simulator" => {
            config.interval = 120;
            config.action = KeepaliveAction::WTap;
            config.adaptive_actions = true;
            config.adaptive_min_samples = 30;
            config.adaptive_learn_sequences = true;
            config.adaptive_learn_actions = true;
            config.send_without_focus = true;
            config.hold_while_playing = true;
            config.auto_fallback = true;
        }
        "Camera AFK" => {
            config.interval = 180;
            config.action = KeepaliveAction::CameraNudge;
            config.adaptive_actions = false;
            config.send_without_focus = true;
            config.hold_while_playing = false;
            config.auto_fallback = true;
        }
        _ => {
            return Err(
                "Couldn't apply preset - choose Roblox, Walking simulator, or Camera AFK to fix this."
                    .to_string(),
            );
        }
    }
    Ok(())
}
