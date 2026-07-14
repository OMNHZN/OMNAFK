use serde::{Deserialize, Deserializer, Serialize};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

pub const DEFAULT_GITHUB_REPO: &str = "OMNHZN/OMNAFK";
pub const MAX_KEY_SEQUENCE_LEN: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KeepaliveAction {
    #[serde(rename = "Space tap")]
    SpaceTap,
    #[serde(rename = "W tap")]
    WTap,
    #[serde(rename = "Camera nudge")]
    CameraNudge,
    #[serde(rename = "Mouse wiggle")]
    MouseWiggle,
    #[serde(rename = "Scroll tick")]
    ScrollTick,
    #[serde(rename = "Right click")]
    RightClick,
    #[serde(rename = "Gamepad nudge")]
    GamepadNudge,
    #[serde(rename = "Key sequence…")]
    KeySequence,
    #[serde(rename = "Per-target…")]
    PerTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetAction {
    #[serde(rename = "Space tap")]
    SpaceTap,
    #[serde(rename = "W tap")]
    WTap,
    #[serde(rename = "Camera nudge")]
    CameraNudge,
    #[serde(rename = "Mouse wiggle")]
    MouseWiggle,
    #[serde(rename = "Scroll tick")]
    ScrollTick,
    #[serde(rename = "Right click")]
    RightClick,
    #[serde(rename = "Gamepad nudge")]
    GamepadNudge,
    #[serde(rename = "Key sequence…")]
    KeySequence,
}

impl TargetAction {
    pub const fn label(self) -> &'static str {
        match self {
            Self::SpaceTap => "Space tap",
            Self::WTap => "W tap",
            Self::CameraNudge => "Camera nudge",
            Self::MouseWiggle => "Mouse wiggle",
            Self::ScrollTick => "Scroll tick",
            Self::RightClick => "Right click",
            Self::GamepadNudge => "Gamepad nudge",
            Self::KeySequence => "Key sequence…",
        }
    }
}

impl KeepaliveAction {
    pub const fn label(self) -> &'static str {
        match self {
            Self::SpaceTap => "Space tap",
            Self::WTap => "W tap",
            Self::CameraNudge => "Camera nudge",
            Self::MouseWiggle => "Mouse wiggle",
            Self::ScrollTick => "Scroll tick",
            Self::RightClick => "Right click",
            Self::GamepadNudge => "Gamepad nudge",
            Self::KeySequence => "Key sequence…",
            Self::PerTarget => "Per-target…",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TargetProfile {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<TargetAction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub key_sequence: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adaptive: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hold_while_playing: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hold_window_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub send_without_focus: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_fallback: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sensitivity: Option<Sensitivity>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UserPreset {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<KeepaliveAction>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub key_sequence: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub randomize: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jitter_pct: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hold_while_playing: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hold_window_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adaptive_actions: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_fallback: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub send_without_focus: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor_placement: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor_style: Option<MonitorStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor_when: Option<MonitorWhen>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor_skip_active: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor_skip_active_secs: Option<u64>,
}

pub const MAX_USER_PRESETS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MonitorWhen {
    #[serde(rename = "Always")]
    Always,
    #[serde(rename = "On launch")]
    OnLaunch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MonitorStyle {
    #[serde(rename = "Preserve size")]
    Preserve,
    #[serde(rename = "Maximize")]
    Maximize,
    #[serde(rename = "Fill work area")]
    FillWorkArea,
    #[serde(rename = "Fill monitor")]
    FillMonitor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedMonitor {
    Off,
    Device(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sensitivity {
    Strict,
    Standard,
    Broad,
}

impl Sensitivity {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Strict => "Strict",
            Self::Standard => "Standard",
            Self::Broad => "Broad",
        }
    }
}

/// Which virtual controller the Gamepad nudge action emulates via ViGEmBus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GamepadKind {
    #[serde(rename = "Xbox 360")]
    Xbox360,
    #[serde(rename = "DualShock 4")]
    DualShock4,
}

/// Which days the quiet-hours window applies to. Membership is evaluated for
/// the current local day, so a window that wraps past midnight ends when the
/// new day no longer matches (e.g. a Weekdays window ends at Friday midnight).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuietDays {
    #[serde(rename = "Every day")]
    EveryDay,
    #[serde(rename = "Weekdays")]
    Weekdays,
    #[serde(rename = "Weekends")]
    Weekends,
}

impl QuietDays {
    /// `dow_from_sunday`: 0 = Sunday .. 6 = Saturday (matches chrono's
    /// `num_days_from_sunday`).
    pub fn includes(self, dow_from_sunday: u32) -> bool {
        let is_weekday = (1..=5).contains(&dow_from_sunday);
        match self {
            Self::EveryDay => true,
            Self::Weekdays => is_weekday,
            Self::Weekends => !is_weekday,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotificationLevel {
    All,
    #[serde(rename = "Errors only")]
    ErrorsOnly,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetView {
    Clean,
    All,
    #[serde(rename = "Games only")]
    GamesOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetDensity {
    Compact,
    Comfortable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TargetSort {
    Status,
    Name,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TabLabelMode {
    #[serde(rename = "Active only")]
    ActiveOnly,
    Always,
    #[serde(rename = "Icons only")]
    IconsOnly,
}

/// Visual theme for the flyout. `Dark` is the default; `HighContrast` keeps the
/// dark base but brightens text and borders for legibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Theme {
    Dark,
    #[serde(rename = "High contrast")]
    HighContrast,
}

impl<'de> Deserialize<'de> for Theme {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "Dark" | "Light" => Ok(Theme::Dark),
            "High contrast" => Ok(Theme::HighContrast),
            _ => Err(serde::de::Error::custom(format!(
                "unknown theme value: {value}"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VersionDisplay {
    #[serde(rename = "Title + About")]
    TitleAndAbout,
    #[serde(rename = "About only")]
    AboutOnly,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SafetyNoteDisplay {
    Compact,
    Full,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdatePromptMode {
    #[serde(rename = "Card + toast")]
    CardAndToast,
    #[serde(rename = "Card only")]
    CardOnly,
    #[serde(rename = "Manual only")]
    ManualOnly,
    /// Download, verify, and install a new release automatically on the launch
    /// update check (skipped while a keepalive session is active).
    #[serde(rename = "Automatic")]
    Automatic,
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
    pub jitter_pct: u8,
    pub action: KeepaliveAction,
    pub adaptive_actions: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub key_sequence: Vec<String>,
    pub send_without_focus: bool,
    pub background_delivery_migrated: bool,
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
    #[serde(default = "default_quiet_days")]
    pub quiet_days: QuietDays,
    pub manual_mode: bool,
    pub sensitivity: Sensitivity,
    pub autostart: bool,
    pub show_on_launch: bool,
    pub remember_pin: bool,
    pub notifications: NotificationLevel,
    /// Push away-from-keyboard alerts (keepalive paused/resumed, errors) to a
    /// phone via ntfy and/or a Discord webhook. Off by default.
    #[serde(default)]
    pub remote_alerts: bool,
    /// ntfy topic (posts to https://ntfy.sh) or a full https ntfy server URL.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ntfy_topic: String,
    /// Discord webhook URL for alerts.
    #[serde(default, skip_serializing_if = "String::is_empty")]
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
    /// Target identities (exe + window class) the user starred to float to the
    /// top of Sightline, most-recent first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub favorite_targets: Vec<String>,
    pub tab_label_mode: TabLabelMode,
    #[serde(default = "default_theme")]
    pub theme: Theme,
    pub version_display: VersionDisplay,
    pub safety_note_display: SafetyNoteDisplay,
    pub update_prompt_mode: UpdatePromptMode,
    pub file_logging: bool,
    pub monitor_placement: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monitor_device: Option<String>,
    pub monitor_when: MonitorWhen,
    pub monitor_style: MonitorStyle,
    pub monitor_skip_active: bool,
    pub monitor_skip_active_secs: u64,
    pub auto_fallback: bool,
    pub adaptive_min_samples: u64,
    pub adaptive_learn_sequences: bool,
    pub adaptive_learn_actions: bool,
    #[serde(default = "default_true")]
    pub adaptive_interval: bool,
    pub burst_detection: bool,
    /// Keep every window of a game awake, not just the highest-scoring one
    /// (for multi-boxing). Off by default.
    #[serde(default)]
    pub keep_all_instances: bool,
    /// Cycle through a varied set of actions each tick instead of repeating one,
    /// so input looks less mechanical. Off by default.
    #[serde(default)]
    pub rotate_actions: bool,
    /// Virtual controller type for the Gamepad nudge action.
    #[serde(default = "default_gamepad_kind")]
    pub gamepad_kind: GamepadKind,
    pub headless: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub always_mark_exes: Vec<String>,
    /// Window-title substrings (lowercased) that force a window to be marked.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mark_title_contains: Vec<String>,
    /// Window-title substrings (lowercased) that force a window to be ignored.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ignore_title_contains: Vec<String>,
    /// Executable names (lowercased) that are always treated as non-games.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub always_ignore_exes: Vec<String>,
    pub community_intelligence: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub community_client_id: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub community_dismissed_exes: Vec<String>,
    /// Tail log files from community manifest for in-game vs menu detection.
    #[serde(default = "default_true")]
    pub presence_log_enabled: bool,
    /// Sample game window pixels for static vs active frames (local only).
    #[serde(default = "default_true")]
    pub presence_screen_enabled: bool,
    /// Read manifest-defined memory patterns (expert; use at your own risk).
    #[serde(default)]
    pub presence_memory_enabled: bool,
    /// Hold keepalives when high-confidence presence says menu/lobby.
    #[serde(default = "default_true")]
    pub respect_presence: bool,
    #[serde(default = "default_true")]
    pub auto_elevate: bool,
    #[serde(default)]
    pub zero_config_migrated: bool,
    /// One-time flag for moving users from the old default prompt to automatic updates.
    #[serde(default)]
    pub auto_update_migrated: bool,

    pub suspended: bool,
    pub pin_position: Option<PinPosition>,
    pub first_run_notified: bool,
    pub tour_done: bool,
    pub overrides: BTreeMap<String, BTreeMap<String, OverrideVerdict>>,
    pub profiles: BTreeMap<String, BTreeMap<String, TargetProfile>>,
    pub paused: BTreeMap<String, BTreeMap<String, bool>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub user_presets: Vec<UserPreset>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinPosition {
    pub x: i32,
    pub y: i32,
}

/// Resolved keepalive settings for one target window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedKeepalive {
    pub interval: u64,
    pub action: ResolvedAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedAction {
    SpaceTap,
    WTap,
    CameraNudge,
    MouseWiggle,
    ScrollTick,
    RightClick,
    GamepadNudge,
    KeySequence(Vec<String>),
}

impl ResolvedAction {
    pub fn label(&self) -> String {
        match self {
            Self::SpaceTap => "Space tap".to_string(),
            Self::WTap => "W tap".to_string(),
            Self::CameraNudge => "Camera nudge".to_string(),
            Self::MouseWiggle => "Mouse wiggle".to_string(),
            Self::ScrollTick => "Scroll tick".to_string(),
            Self::RightClick => "Right click".to_string(),
            Self::GamepadNudge => "Gamepad nudge".to_string(),
            Self::KeySequence(keys) => format!("Keys {}", keys.join("+")),
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            interval: 540,
            randomize: true,
            jitter_pct: 15,
            action: KeepaliveAction::WTap,
            adaptive_actions: true,
            key_sequence: Vec::new(),
            send_without_focus: false,
            background_delivery_migrated: true,
            auto_elevate: true,
            zero_config_migrated: true,
            auto_update_migrated: true,
            hold_while_playing: true,
            hold_window_secs: 60,
            idle_threshold_mins: 0,
            pause_on_battery: false,
            pause_when_locked: false,
            max_session_hours: 0,
            max_session_actions: 0,
            quiet_hours_enabled: false,
            quiet_start: "23:00".to_string(),
            quiet_end: "07:00".to_string(),
            quiet_days: QuietDays::EveryDay,
            manual_mode: false,
            sensitivity: Sensitivity::Standard,
            autostart: true,
            show_on_launch: false,
            remember_pin: true,
            notifications: NotificationLevel::ErrorsOnly,
            remote_alerts: false,
            ntfy_topic: String::new(),
            discord_webhook: String::new(),
            hotkey: "CTRL+ALT+K".to_string(),
            suspend_hotkey: String::new(),
            github_repo: DEFAULT_GITHUB_REPO.to_string(),
            check_updates_on_launch: true,
            ignored_update_tag: None,
            pinned: false,
            last_tab: "targets".to_string(),
            settings_interface_collapsed: true,
            settings_updates_collapsed: false,
            general_advanced_collapsed: true,
            target_view: TargetView::All,
            target_density: TargetDensity::Compact,
            target_sort: TargetSort::Status,
            favorite_targets: Vec::new(),
            tab_label_mode: TabLabelMode::ActiveOnly,
            theme: default_theme(),
            version_display: VersionDisplay::TitleAndAbout,
            safety_note_display: SafetyNoteDisplay::Compact,
            update_prompt_mode: UpdatePromptMode::Automatic,
            file_logging: false,
            monitor_placement: false,
            monitor_device: None,
            monitor_when: MonitorWhen::Always,
            monitor_style: MonitorStyle::Preserve,
            monitor_skip_active: true,
            monitor_skip_active_secs: 5,
            auto_fallback: true,
            adaptive_min_samples: 20,
            adaptive_learn_sequences: true,
            adaptive_learn_actions: true,
            adaptive_interval: true,
            burst_detection: true,
            keep_all_instances: false,
            rotate_actions: false,
            gamepad_kind: GamepadKind::Xbox360,
            headless: false,
            always_mark_exes: Vec::new(),
            always_ignore_exes: Vec::new(),
            mark_title_contains: Vec::new(),
            ignore_title_contains: Vec::new(),
            community_intelligence: false,
            community_client_id: String::new(),
            community_dismissed_exes: Vec::new(),
            presence_log_enabled: true,
            presence_screen_enabled: true,
            presence_memory_enabled: false,
            respect_presence: true,
            suspended: false,
            pin_position: None,
            first_run_notified: false,
            tour_done: false,
            overrides: BTreeMap::new(),
            profiles: BTreeMap::new(),
            paused: BTreeMap::new(),
            user_presets: Vec::new(),
        }
    }
}

impl AppConfig {
    /// One-time migrations for saved settings. Returns true when the file should be rewritten.
    pub fn migrate(&mut self) -> bool {
        let mut changed = false;
        // Older builds defaulted to PostMessage background delivery, which most games ignore.
        if !self.background_delivery_migrated {
            if self.send_without_focus {
                self.send_without_focus = false;
            }
            self.background_delivery_migrated = true;
            changed = true;
        }
        if !self.zero_config_migrated {
            if self.action == KeepaliveAction::SpaceTap {
                self.action = KeepaliveAction::WTap;
            }
            if self.adaptive_min_samples >= 50 {
                self.adaptive_min_samples = 20;
            }
            self.auto_elevate = true;
            self.zero_config_migrated = true;
            changed = true;
        }
        if !self.auto_update_migrated {
            if self.update_prompt_mode == UpdatePromptMode::CardAndToast {
                self.update_prompt_mode = UpdatePromptMode::Automatic;
            }
            self.auto_update_migrated = true;
            changed = true;
        }
        changed
    }

    /// Clamp disk-loaded values to the same safe ranges enforced by the UI.
    pub fn sanitize_loaded(&mut self) -> bool {
        let mut changed = false;
        changed |= clamp_u64(&mut self.interval, 10, 3600);
        changed |= clamp_u8(&mut self.jitter_pct, 1, 50);
        changed |= clamp_u64(&mut self.hold_window_secs, 10, 600);
        changed |= clamp_u64(&mut self.idle_threshold_mins, 0, 120);
        changed |= clamp_u64(&mut self.max_session_hours, 0, 168);
        changed |= clamp_u64(&mut self.max_session_actions, 0, 100_000);
        changed |= clamp_u64(&mut self.adaptive_min_samples, 10, 500);
        changed |= clamp_u64(&mut self.monitor_skip_active_secs, 1, 60);
        changed |= sanitize_key_sequence(&mut self.key_sequence);
        if self.user_presets.len() > MAX_USER_PRESETS {
            self.user_presets.truncate(MAX_USER_PRESETS);
            changed = true;
        }
        for preset in &mut self.user_presets {
            if let Some(interval) = &mut preset.interval {
                changed |= clamp_u64(interval, 10, 3600);
            }
            if let Some(jitter) = &mut preset.jitter_pct {
                changed |= clamp_u8(jitter, 1, 50);
            }
            if let Some(hold) = &mut preset.hold_window_secs {
                changed |= clamp_u64(hold, 10, 600);
            }
            changed |= sanitize_key_sequence(&mut preset.key_sequence);
        }
        for classes in self.profiles.values_mut() {
            for profile in classes.values_mut() {
                if let Some(interval) = &mut profile.interval {
                    changed |= clamp_u64(interval, 10, 3600);
                }
                if let Some(hold) = &mut profile.hold_window_secs {
                    changed |= clamp_u64(hold, 10, 600);
                }
                changed |= sanitize_key_sequence(&mut profile.key_sequence);
            }
        }
        changed
    }

    /// Built-in keepalive hints for well-known game executables (no user setup required).
    pub fn known_exe_keepalive(exe: &str) -> Option<ResolvedKeepalive> {
        match identity_exe_key(exe).as_str() {
            "robloxplayerbeta.exe" | "robloxplayer.exe" => Some(ResolvedKeepalive {
                interval: 540,
                action: ResolvedAction::SpaceTap,
            }),
            "gta5.exe" => Some(ResolvedKeepalive {
                interval: 540,
                action: ResolvedAction::WTap,
            }),
            "minecraft.windows.exe" | "javaw.exe" => Some(ResolvedKeepalive {
                interval: 180,
                action: ResolvedAction::WTap,
            }),
            "fortniteclient-win64-shipping.exe"
            | "valorant-win64-shipping.exe"
            | "cs2.exe"
            | "csgo.exe"
            | "eldenring.exe"
            | "darksoulsiii.exe"
            | "rocketleague.exe"
            | "destiny2.exe" => Some(ResolvedKeepalive {
                interval: 540,
                action: ResolvedAction::WTap,
            }),
            _ => None,
        }
    }

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

    /// Verdict forced by a window-title rule, if any. Mark rules win over
    /// ignore rules when both match.
    pub fn title_override(&self, title: &str) -> Option<OverrideVerdict> {
        if self.mark_title_contains.is_empty() && self.ignore_title_contains.is_empty() {
            return None;
        }
        let title = title.to_ascii_lowercase();
        let any_rule_matches = |rules: &[String]| {
            rules
                .iter()
                .any(|rule| !rule.is_empty() && title.contains(rule.as_str()))
        };
        if any_rule_matches(&self.mark_title_contains) {
            Some(OverrideVerdict::Game)
        } else if any_rule_matches(&self.ignore_title_contains) {
            Some(OverrideVerdict::Ignored)
        } else {
            None
        }
    }

    /// Verdict forced by the user's always-ignore exe list, if the window's exe
    /// matches. Case-insensitive, exact exe-name match.
    pub fn exe_ignore_override(&self, exe: &str) -> Option<OverrideVerdict> {
        let exe = exe.trim();
        self.always_ignore_exes
            .iter()
            .any(|entry| entry.trim().eq_ignore_ascii_case(exe))
            .then_some(OverrideVerdict::Ignored)
    }

    pub fn profile_for(&self, exe: &str, wclass: &str) -> Option<&TargetProfile> {
        self.profiles
            .get(&identity_exe_key(exe))
            .and_then(|classes| classes.get(wclass))
    }

    pub fn profile_for_mut(&mut self, exe: &str, wclass: &str) -> &mut TargetProfile {
        self.profiles
            .entry(identity_exe_key(exe))
            .or_default()
            .entry(wclass.to_string())
            .or_default()
    }

    pub fn set_profile(&mut self, exe: &str, wclass: &str, profile: TargetProfile) {
        let exe_key = identity_exe_key(exe);
        if profile.action.is_none()
            && profile.interval.is_none()
            && profile.key_sequence.is_empty()
            && profile.monitor.is_none()
            && profile.adaptive.is_none()
            && profile.hold_while_playing.is_none()
            && profile.hold_window_secs.is_none()
            && profile.send_without_focus.is_none()
            && profile.auto_fallback.is_none()
            && profile.sensitivity.is_none()
        {
            if let Some(classes) = self.profiles.get_mut(&exe_key) {
                classes.remove(wclass);
                if classes.is_empty() {
                    self.profiles.remove(&exe_key);
                }
            }
            return;
        }
        self.profiles
            .entry(exe_key)
            .or_default()
            .insert(wclass.to_string(), profile);
    }

    pub fn resolve_keepalive(&self, exe: &str, wclass: &str) -> ResolvedKeepalive {
        let profile = self.profile_for(exe, wclass);
        let known = Self::known_exe_keepalive(exe);
        let interval = profile
            .and_then(|p| p.interval)
            .or_else(|| known.as_ref().map(|k| k.interval))
            .unwrap_or(self.interval);

        let per_target_fallback = known
            .as_ref()
            .map(|k| k.action.clone())
            .unwrap_or(ResolvedAction::WTap);

        let action = match self.action {
            KeepaliveAction::PerTarget => profile
                .and_then(|p| {
                    p.action
                        .map(|a| resolved_from_target_action(a, &p.key_sequence))
                })
                .unwrap_or(per_target_fallback),
            KeepaliveAction::KeySequence => resolved_key_sequence(&self.key_sequence),
            _ => known
                .as_ref()
                .map(|k| k.action.clone())
                .unwrap_or_else(|| resolved_from_keepalive_action(self.action)),
        };

        ResolvedKeepalive { interval, action }
    }

    /// Resolve which monitor a target should live on, if any.
    pub fn resolve_monitor(&self, exe: &str, wclass: &str) -> ResolvedMonitor {
        if !self.monitor_placement {
            return ResolvedMonitor::Off;
        }

        if let Some(profile) = self.profile_for(exe, wclass) {
            if let Some(monitor) = &profile.monitor {
                if monitor == "Don't move" {
                    return ResolvedMonitor::Off;
                }
                return ResolvedMonitor::Device(monitor.clone());
            }
        }

        self.monitor_device
            .as_ref()
            .filter(|device| !device.is_empty())
            .cloned()
            .map(ResolvedMonitor::Device)
            .unwrap_or(ResolvedMonitor::Off)
    }

    /// Detection sensitivity for this target, honoring any per-target override.
    pub fn resolve_sensitivity(&self, exe: &str, wclass: &str) -> Sensitivity {
        self.profile_for(exe, wclass)
            .and_then(|profile| profile.sensitivity)
            .unwrap_or(self.sensitivity)
    }

    /// Whether adaptive learning applies to this target.
    pub fn adaptive_enabled(&self, exe: &str, wclass: &str) -> bool {
        if let Some(profile) = self.profile_for(exe, wclass) {
            if let Some(enabled) = profile.adaptive {
                return enabled;
            }
        }
        self.adaptive_actions
    }

    pub fn is_always_marked_exe(&self, exe: &str) -> bool {
        let exe = exe.to_ascii_lowercase();
        self.always_mark_exes
            .iter()
            .any(|entry| entry.trim().eq_ignore_ascii_case(&exe))
    }

    pub fn is_paused(&self, exe: &str, wclass: &str) -> bool {
        self.paused
            .get(&identity_exe_key(exe))
            .and_then(|classes| classes.get(wclass).copied())
            .unwrap_or(false)
    }

    pub fn set_paused(&mut self, exe: &str, wclass: &str, paused: bool) {
        let exe_key = identity_exe_key(exe);
        if paused {
            self.paused
                .entry(exe_key)
                .or_default()
                .insert(wclass.to_string(), true);
        } else if let Some(classes) = self.paused.get_mut(&exe_key) {
            classes.remove(wclass);
            if classes.is_empty() {
                self.paused.remove(&exe_key);
            }
        }
    }

    /// True when the current local time falls inside the configured quiet window
    /// and the current day is in scope. `dow_from_sunday`: 0 = Sunday .. 6 = Sat.
    pub fn in_quiet_hours(&self, now_minutes: u32, dow_from_sunday: u32) -> bool {
        if !self.quiet_hours_enabled {
            return false;
        }
        if !self.quiet_days.includes(dow_from_sunday) {
            return false;
        }
        let (Some(start), Some(end)) = (parse_hhmm(&self.quiet_start), parse_hhmm(&self.quiet_end))
        else {
            return false;
        };
        if start == end {
            return false;
        }
        if start < end {
            (start..end).contains(&now_minutes)
        } else {
            // Window wraps past midnight, e.g. 23:00 -> 07:00.
            now_minutes >= start || now_minutes < end
        }
    }

    pub fn ignores_update(&self, tag: &str) -> bool {
        self.ignored_update_tag.as_deref() == Some(tag)
    }
}

fn default_true() -> bool {
    true
}

fn default_quiet_days() -> QuietDays {
    QuietDays::EveryDay
}

fn default_gamepad_kind() -> GamepadKind {
    GamepadKind::Xbox360
}

fn default_theme() -> Theme {
    Theme::Dark
}

/// Number of distinct actions cycled through when `rotate_actions` is on.
pub const ACTION_ROTATION_LEN: u32 = 3;

/// Varied action for this tick when `rotate_actions` is on, so repeated
/// keepalives don't look perfectly mechanical.
pub fn rotation_action(index: u32) -> ResolvedAction {
    match index % ACTION_ROTATION_LEN {
        0 => ResolvedAction::WTap,
        1 => ResolvedAction::SpaceTap,
        _ => ResolvedAction::CameraNudge,
    }
}

pub fn resolved_from_keepalive_action(action: KeepaliveAction) -> ResolvedAction {
    match action {
        KeepaliveAction::SpaceTap => ResolvedAction::SpaceTap,
        KeepaliveAction::WTap => ResolvedAction::WTap,
        KeepaliveAction::CameraNudge => ResolvedAction::CameraNudge,
        KeepaliveAction::MouseWiggle => ResolvedAction::MouseWiggle,
        KeepaliveAction::ScrollTick => ResolvedAction::ScrollTick,
        KeepaliveAction::RightClick => ResolvedAction::RightClick,
        KeepaliveAction::GamepadNudge => ResolvedAction::GamepadNudge,
        KeepaliveAction::KeySequence | KeepaliveAction::PerTarget => ResolvedAction::WTap,
    }
}

pub fn parse_hhmm(value: &str) -> Option<u32> {
    let (h, m) = value.trim().split_once(':')?;
    let h: u32 = h.parse().ok()?;
    let m: u32 = m.parse().ok()?;
    (h < 24 && m < 60).then_some(h * 60 + m)
}

pub fn resolved_from_target_action(action: TargetAction, keys: &[String]) -> ResolvedAction {
    match action {
        TargetAction::SpaceTap => ResolvedAction::SpaceTap,
        TargetAction::WTap => ResolvedAction::WTap,
        TargetAction::CameraNudge => ResolvedAction::CameraNudge,
        TargetAction::MouseWiggle => ResolvedAction::MouseWiggle,
        TargetAction::ScrollTick => ResolvedAction::ScrollTick,
        TargetAction::RightClick => ResolvedAction::RightClick,
        TargetAction::GamepadNudge => ResolvedAction::GamepadNudge,
        TargetAction::KeySequence => resolved_key_sequence(keys),
    }
}

fn resolved_key_sequence(keys: &[String]) -> ResolvedAction {
    if keys.is_empty() {
        ResolvedAction::SpaceTap
    } else {
        ResolvedAction::KeySequence(keys.to_vec())
    }
}

pub fn validate_key_sequence(keys: &[String]) -> Result<(), String> {
    if keys.len() > MAX_KEY_SEQUENCE_LEN {
        return Err(format!(
            "Couldn't save key sequence - use at most {MAX_KEY_SEQUENCE_LEN} keys to fix this."
        ));
    }
    for key in keys {
        if key_name_to_vk(key).is_none() {
            return Err(format!(
                "Couldn't save key sequence - unsupported key '{key}' to fix this."
            ));
        }
    }
    Ok(())
}

pub fn key_name_to_vk(name: &str) -> Option<u16> {
    let upper = name.trim().to_ascii_uppercase();
    if upper.len() == 1 {
        let ch = upper.as_bytes()[0];
        if ch.is_ascii_alphanumeric() {
            return Some(ch as u16);
        }
    }
    match upper.as_str() {
        "SPACE" => Some(0x20),
        "ENTER" | "RETURN" => Some(0x0D),
        "TAB" => Some(0x09),
        "ESC" | "ESCAPE" => Some(0x1B),
        "UP" => Some(0x26),
        "DOWN" => Some(0x28),
        "LEFT" => Some(0x25),
        "RIGHT" => Some(0x27),
        "SHIFT" => Some(0x10),
        "CTRL" | "CONTROL" => Some(0x11),
        "ALT" => Some(0x12),
        _ => None,
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
    let mut config: AppConfig = match fs::read_to_string(path) {
        Ok(raw) => match serde_json::from_str(&raw) {
            Ok(config) => config,
            Err(error) => {
                let backup = backup_corrupt_config(path)?;
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) {
                    let recovered = recover_config_from_value(value);
                    tracing::warn!(
                        "Recovered partial settings after load error (backup at {}): {error}",
                        backup.display()
                    );
                    recovered
                } else {
                    return Err(invalid_config_with_backup(error, &backup));
                }
            }
        },
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(AppConfig::default()),
        Err(error) => return Err(error),
    };
    let migrated = config.migrate();
    let sanitized = config.sanitize_loaded();
    if migrated || sanitized {
        let _ = save_to_path(&config, path);
    }
    Ok(config)
}

pub fn save_to_path(config: &AppConfig, path: &Path) -> io::Result<()> {
    let json = serde_json::to_vec_pretty(config).map_err(invalid_config)?;
    crate::persist::atomic_write(path, &json)
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

fn backup_corrupt_config(path: &Path) -> io::Result<PathBuf> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("config");
    let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let mut backup = parent.join(format!("{stem}.corrupt-{timestamp}.json"));
    let mut suffix = 1;
    while backup.exists() {
        backup = parent.join(format!("{stem}.corrupt-{timestamp}-{suffix}.json"));
        suffix += 1;
    }
    fs::copy(path, &backup)?;
    Ok(backup)
}

fn invalid_config_with_backup(error: serde_json::Error, backup: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "Couldn't read OMNAFK settings, so the damaged config was backed up to {}: {error}",
            backup.display()
        ),
    )
}

fn recover_config_from_value(value: serde_json::Value) -> AppConfig {
    let Some(obj) = value.as_object() else {
        return AppConfig::default();
    };
    let mut config = AppConfig::default();
    for (key, field) in obj {
        merge_config_field(&mut config, key, field.clone());
    }
    config
}

fn merge_config_field(config: &mut AppConfig, key: &str, value: serde_json::Value) {
    macro_rules! merge {
        ($field:ident: $ty:ty) => {
            if key == stringify!($field) {
                if let Ok(parsed) = serde_json::from_value::<$ty>(value) {
                    config.$field = parsed;
                }
                return;
            }
        };
    }

    merge!(interval: u64);
    merge!(randomize: bool);
    merge!(jitter_pct: u8);
    merge!(action: KeepaliveAction);
    merge!(adaptive_actions: bool);
    merge!(key_sequence: Vec<String>);
    merge!(send_without_focus: bool);
    merge!(background_delivery_migrated: bool);
    merge!(hold_while_playing: bool);
    merge!(hold_window_secs: u64);
    merge!(idle_threshold_mins: u64);
    merge!(pause_on_battery: bool);
    merge!(pause_when_locked: bool);
    merge!(max_session_hours: u64);
    merge!(max_session_actions: u64);
    merge!(quiet_hours_enabled: bool);
    merge!(quiet_start: String);
    merge!(quiet_end: String);
    merge!(quiet_days: QuietDays);
    merge!(manual_mode: bool);
    merge!(sensitivity: Sensitivity);
    merge!(autostart: bool);
    merge!(show_on_launch: bool);
    merge!(remember_pin: bool);
    merge!(notifications: NotificationLevel);
    merge!(remote_alerts: bool);
    merge!(ntfy_topic: String);
    merge!(discord_webhook: String);
    merge!(hotkey: String);
    merge!(suspend_hotkey: String);
    merge!(github_repo: String);
    merge!(check_updates_on_launch: bool);
    merge!(ignored_update_tag: Option<String>);
    merge!(pinned: bool);
    merge!(last_tab: String);
    merge!(settings_interface_collapsed: bool);
    merge!(settings_updates_collapsed: bool);
    merge!(general_advanced_collapsed: bool);
    merge!(target_view: TargetView);
    merge!(target_density: TargetDensity);
    merge!(target_sort: TargetSort);
    merge!(favorite_targets: Vec<String>);
    merge!(tab_label_mode: TabLabelMode);
    merge!(theme: Theme);
    merge!(version_display: VersionDisplay);
    merge!(safety_note_display: SafetyNoteDisplay);
    merge!(update_prompt_mode: UpdatePromptMode);
    merge!(file_logging: bool);
    merge!(monitor_placement: bool);
    merge!(monitor_device: Option<String>);
    merge!(monitor_when: MonitorWhen);
    merge!(monitor_style: MonitorStyle);
    merge!(monitor_skip_active: bool);
    merge!(monitor_skip_active_secs: u64);
    merge!(auto_fallback: bool);
    merge!(adaptive_min_samples: u64);
    merge!(adaptive_learn_sequences: bool);
    merge!(adaptive_learn_actions: bool);
    merge!(adaptive_interval: bool);
    merge!(burst_detection: bool);
    merge!(keep_all_instances: bool);
    merge!(rotate_actions: bool);
    merge!(gamepad_kind: GamepadKind);
    merge!(headless: bool);
    merge!(always_mark_exes: Vec<String>);
    merge!(always_ignore_exes: Vec<String>);
    merge!(mark_title_contains: Vec<String>);
    merge!(ignore_title_contains: Vec<String>);
    merge!(community_intelligence: bool);
    merge!(community_client_id: String);
    merge!(community_dismissed_exes: Vec<String>);
    merge!(presence_log_enabled: bool);
    merge!(presence_screen_enabled: bool);
    merge!(presence_memory_enabled: bool);
    merge!(respect_presence: bool);
    merge!(auto_elevate: bool);
    merge!(zero_config_migrated: bool);
    merge!(auto_update_migrated: bool);
    merge!(suspended: bool);
    merge!(pin_position: Option<PinPosition>);
    merge!(first_run_notified: bool);
    merge!(tour_done: bool);
    merge!(user_presets: Vec<UserPreset>);

    match key {
        "profiles" => merge_profiles(config, value),
        "overrides" => merge_overrides(config, value),
        "paused" => merge_paused(config, value),
        _ => {}
    }
}

fn merge_profiles(config: &mut AppConfig, value: serde_json::Value) {
    let Some(exes) = value.as_object() else {
        return;
    };
    for (exe, classes) in exes {
        let Some(classes) = classes.as_object() else {
            continue;
        };
        for (wclass, profile_value) in classes {
            let cleaned = sanitize_target_profile_value(profile_value.clone());
            if let Ok(profile) = serde_json::from_value::<TargetProfile>(cleaned) {
                config
                    .profiles
                    .entry(exe.clone())
                    .or_default()
                    .insert(wclass.clone(), profile);
            }
        }
    }
}

fn merge_overrides(config: &mut AppConfig, value: serde_json::Value) {
    let Some(exes) = value.as_object() else {
        return;
    };
    for (exe, classes) in exes {
        let Some(classes) = classes.as_object() else {
            continue;
        };
        for (wclass, verdict_value) in classes {
            if let Ok(verdict) = serde_json::from_value::<OverrideVerdict>(verdict_value.clone()) {
                config
                    .overrides
                    .entry(exe.clone())
                    .or_default()
                    .insert(wclass.clone(), verdict);
            }
        }
    }
}

fn merge_paused(config: &mut AppConfig, value: serde_json::Value) {
    let Some(exes) = value.as_object() else {
        return;
    };
    for (exe, classes) in exes {
        let Some(classes) = classes.as_object() else {
            continue;
        };
        for (wclass, paused_value) in classes {
            if let Some(paused) = paused_value.as_bool() {
                config
                    .paused
                    .entry(exe.clone())
                    .or_default()
                    .insert(wclass.clone(), paused);
            }
        }
    }
}

fn sanitize_target_profile_value(mut value: serde_json::Value) -> serde_json::Value {
    let Some(obj) = value.as_object_mut() else {
        return value;
    };
    if let Some(action) = obj.get("action").cloned() {
        if serde_json::from_value::<TargetAction>(action).is_err() {
            obj.remove("action");
        }
    }
    if let Some(sensitivity) = obj.get("sensitivity").cloned() {
        if serde_json::from_value::<Sensitivity>(sensitivity).is_err() {
            obj.remove("sensitivity");
        }
    }
    value
}

fn clamp_u64(value: &mut u64, min: u64, max: u64) -> bool {
    let old = *value;
    *value = (*value).clamp(min, max);
    old != *value
}

fn clamp_u8(value: &mut u8, min: u8, max: u8) -> bool {
    let old = *value;
    *value = (*value).clamp(min, max);
    old != *value
}

fn sanitize_key_sequence(keys: &mut Vec<String>) -> bool {
    let old = keys.clone();
    keys.retain(|key| key_name_to_vk(key).is_some());
    keys.truncate(MAX_KEY_SEQUENCE_LEN);
    *keys != old
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
        assert_eq!(config.jitter_pct, 15);
        assert_eq!(config.hold_window_secs, 60);
        assert_eq!(config.idle_threshold_mins, 0);
        assert!(!config.pause_on_battery);
        assert!(!config.pause_when_locked);
        assert_eq!(config.max_session_hours, 0);
        assert_eq!(config.max_session_actions, 0);
        assert!(!config.quiet_hours_enabled);
        assert_eq!(config.quiet_start, "23:00");
        assert_eq!(config.quiet_end, "07:00");
        assert_eq!(config.target_sort, TargetSort::Status);
        assert!(!config.file_logging);
        assert!(!config.monitor_placement);
        assert!(config.monitor_device.is_none());
        assert_eq!(config.monitor_when, MonitorWhen::Always);
        assert_eq!(config.monitor_style, MonitorStyle::Preserve);
        assert!(config.monitor_skip_active);
        assert_eq!(config.monitor_skip_active_secs, 5);
        assert!(config.suspend_hotkey.is_empty());
        assert!(config.general_advanced_collapsed);
        assert!(!config.tour_done);
        assert!(config.paused.is_empty());
        assert_eq!(config.action, KeepaliveAction::WTap);
        assert!(config.adaptive_actions);
        assert!(config.auto_elevate);
        assert!(config.zero_config_migrated);
        assert!(config.adaptive_interval);
        assert!(!config.keep_all_instances);
        assert!(!config.rotate_actions);
        assert_eq!(config.gamepad_kind, GamepadKind::Xbox360);
        assert!(!config.community_intelligence);
        assert!(config.presence_log_enabled);
        assert!(config.presence_screen_enabled);
        assert!(!config.presence_memory_enabled);
        assert!(config.respect_presence);
        assert_eq!(config.adaptive_min_samples, 20);
        assert!(config.key_sequence.is_empty());
        assert!(!config.send_without_focus);
        assert!(config.hold_while_playing);
        assert!(!config.manual_mode);
        assert_eq!(config.sensitivity, Sensitivity::Standard);
        assert!(config.autostart);
        assert!(!config.show_on_launch);
        assert!(config.remember_pin);
        assert_eq!(config.notifications, NotificationLevel::ErrorsOnly);
        assert!(!config.remote_alerts);
        assert!(config.ntfy_topic.is_empty());
        assert!(config.discord_webhook.is_empty());
        assert_eq!(config.hotkey, "CTRL+ALT+K");
        assert_eq!(config.github_repo, DEFAULT_GITHUB_REPO);
        assert!(config.check_updates_on_launch);
        assert!(config.ignored_update_tag.is_none());
        assert!(!config.pinned);
        assert_eq!(config.last_tab, "targets");
        assert!(config.settings_interface_collapsed);
        assert_eq!(config.target_view, TargetView::All);
        assert_eq!(config.target_density, TargetDensity::Compact);
        assert_eq!(config.tab_label_mode, TabLabelMode::ActiveOnly);
        assert_eq!(config.theme, Theme::Dark);
        assert_eq!(config.version_display, VersionDisplay::TitleAndAbout);
        assert_eq!(config.safety_note_display, SafetyNoteDisplay::Compact);
        assert_eq!(config.update_prompt_mode, UpdatePromptMode::Automatic);
        assert!(!config.suspended);
        assert!(config.pin_position.is_none());
        assert!(!config.first_run_notified);
        assert!(config.overrides.is_empty());
        assert!(config.profiles.is_empty());
    }

    #[test]
    fn exe_ignore_override_forces_ignored_case_insensitively() {
        let mut config = AppConfig::default();
        assert!(config.exe_ignore_override("Zoom.exe").is_none());
        config.always_ignore_exes = vec!["zoom.exe".to_string()];
        assert_eq!(
            config.exe_ignore_override("Zoom.exe"),
            Some(OverrideVerdict::Ignored)
        );
        assert!(config.exe_ignore_override("game.exe").is_none());
    }

    #[test]
    fn update_prompt_mode_serde_has_automatic() {
        assert_eq!(
            serde_json::to_string(&UpdatePromptMode::Automatic).unwrap(),
            "\"Automatic\""
        );
        let parsed: UpdatePromptMode = serde_json::from_str("\"Automatic\"").unwrap();
        assert_eq!(parsed, UpdatePromptMode::Automatic);
    }

    #[test]
    fn migrate_moves_old_default_update_prompt_to_automatic() {
        let mut config = AppConfig {
            update_prompt_mode: UpdatePromptMode::CardAndToast,
            auto_update_migrated: false,
            ..AppConfig::default()
        };

        assert!(config.migrate());
        assert_eq!(config.update_prompt_mode, UpdatePromptMode::Automatic);
        assert!(config.auto_update_migrated);
        assert!(!config.migrate());
    }

    #[test]
    fn migrate_preserves_reduced_update_prompt_choices() {
        for mode in [UpdatePromptMode::CardOnly, UpdatePromptMode::ManualOnly] {
            let mut config = AppConfig {
                update_prompt_mode: mode,
                auto_update_migrated: false,
                ..AppConfig::default()
            };

            assert!(config.migrate());
            assert_eq!(config.update_prompt_mode, mode);
            assert!(config.auto_update_migrated);
        }
    }

    #[test]
    fn theme_serde_uses_friendly_labels() {
        assert_eq!(
            serde_json::to_string(&Theme::HighContrast).unwrap(),
            "\"High contrast\""
        );
        let parsed: Theme = serde_json::from_str("\"Dark\"").unwrap();
        assert_eq!(parsed, Theme::Dark);
        let legacy_light: Theme = serde_json::from_str("\"Light\"").unwrap();
        assert_eq!(legacy_light, Theme::Dark);
    }

    #[test]
    fn migrate_applies_zero_config_defaults_for_legacy_users() {
        let mut config = AppConfig {
            action: KeepaliveAction::SpaceTap,
            adaptive_min_samples: 50,
            community_intelligence: false,
            auto_elevate: false,
            zero_config_migrated: false,
            background_delivery_migrated: true,
            ..AppConfig::default()
        };
        assert!(config.migrate());
        assert_eq!(config.action, KeepaliveAction::WTap);
        assert_eq!(config.adaptive_min_samples, 20);
        assert!(!config.community_intelligence);
        assert!(config.auto_elevate);
        assert!(config.zero_config_migrated);
        assert!(!config.migrate());
    }

    #[test]
    fn known_exe_keepalive_overrides_for_gta5() {
        let resolved = AppConfig::known_exe_keepalive("GTA5.exe").unwrap();
        assert_eq!(resolved.action, ResolvedAction::WTap);
        assert_eq!(resolved.interval, 540);
    }

    #[test]
    fn migrate_clears_legacy_background_only_default() {
        let mut config = AppConfig {
            send_without_focus: true,
            background_delivery_migrated: false,
            ..AppConfig::default()
        };
        assert!(config.migrate());
        assert!(!config.send_without_focus);
        assert!(!config.migrate());
    }

    #[test]
    fn migrate_keeps_explicit_background_only_after_marker_is_set() {
        let mut config = AppConfig {
            send_without_focus: true,
            background_delivery_migrated: true,
            ..AppConfig::default()
        };
        assert!(!config.migrate());
        assert!(config.send_without_focus);
    }

    #[test]
    fn roundtrips_config_json() {
        let path = temp_config_path("roundtrip");
        let mut config = AppConfig {
            interval: 120,
            action: KeepaliveAction::CameraNudge,
            key_sequence: vec!["SPACE".into(), "W".into()],
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
        config.set_profile(
            "eldenring.exe",
            "FLUX",
            TargetProfile {
                action: Some(TargetAction::WTap),
                interval: Some(60),
                key_sequence: vec![],
                ..TargetProfile::default()
            },
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
    fn recovers_invalid_top_level_enum_and_keeps_valid_fields() {
        let path = temp_config_path("enum-recover");
        fs::write(
            &path,
            r#"{
              "interval": 120,
              "action": "Not a real action",
              "notifications": "All",
              "sensitivity": "Broad"
            }"#,
        )
        .expect("write config");

        let loaded = load_from_path(&path).expect("load config");

        assert_eq!(loaded.interval, 120);
        assert_eq!(loaded.notifications, NotificationLevel::All);
        assert_eq!(loaded.sensitivity, Sensitivity::Broad);
        assert_eq!(loaded.action, KeepaliveAction::WTap);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn skips_invalid_scalar_types_during_recovery() {
        let path = temp_config_path("type-recover");
        fs::write(
            &path,
            r#"{
              "interval": "not-a-number",
              "action": "W tap",
              "jitter_pct": 15
            }"#,
        )
        .expect("write config");

        let loaded = load_from_path(&path).expect("load config");

        assert_eq!(loaded.interval, 540);
        assert_eq!(loaded.action, KeepaliveAction::WTap);
        assert_eq!(loaded.jitter_pct, 15);

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

    #[test]
    fn backs_up_corrupt_config_before_falling_back() {
        let path = temp_config_path("corrupt");
        fs::write(&path, r#"{"interval": "#).expect("write config");

        let error = load_from_path(&path).expect_err("corrupt config should error");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        let parent = path.parent().unwrap();
        let stem = path.file_stem().unwrap().to_string_lossy();
        let backups = fs::read_dir(parent)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(&format!("{stem}.corrupt-"))
            })
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        assert!(!backups.is_empty());

        let _ = fs::remove_file(&path);
        for backup in backups {
            let _ = fs::remove_file(backup);
        }
    }

    #[test]
    fn clamps_loaded_config_ranges() {
        let path = temp_config_path("clamp");
        fs::write(
            &path,
            r#"{
              "interval": 0,
              "jitter_pct": 99,
              "hold_window_secs": 1,
              "adaptive_min_samples": 999,
              "monitor_skip_active_secs": 0,
              "background_delivery_migrated": true,
              "zero_config_migrated": true,
              "key_sequence": ["SPACE", "NOPE", "W", "A", "S", "D"]
            }"#,
        )
        .expect("write config");

        let loaded = load_from_path(&path).expect("load config");

        assert_eq!(loaded.interval, 10);
        assert_eq!(loaded.jitter_pct, 50);
        assert_eq!(loaded.hold_window_secs, 10);
        assert_eq!(loaded.adaptive_min_samples, 500);
        assert_eq!(loaded.monitor_skip_active_secs, 1);
        assert_eq!(loaded.key_sequence, vec!["SPACE", "W", "A", "S"]);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn per_target_profile_overrides_global() {
        let mut config = AppConfig {
            action: KeepaliveAction::PerTarget,
            interval: 540,
            ..AppConfig::default()
        };
        config.set_profile(
            "game.exe",
            "CLASS",
            TargetProfile {
                action: Some(TargetAction::CameraNudge),
                interval: Some(30),
                key_sequence: vec![],
                ..TargetProfile::default()
            },
        );

        let resolved = config.resolve_keepalive("game.exe", "CLASS");
        assert_eq!(resolved.interval, 30);
        assert_eq!(resolved.action, ResolvedAction::CameraNudge);
    }

    #[test]
    fn title_rules_force_verdict_case_insensitively() {
        let mut config = AppConfig {
            mark_title_contains: vec!["my game".to_string()],
            ignore_title_contains: vec!["launcher".to_string()],
            ..AppConfig::default()
        };
        assert_eq!(
            config.title_override("MY GAME - Level 1"),
            Some(OverrideVerdict::Game)
        );
        assert_eq!(
            config.title_override("Epic Launcher"),
            Some(OverrideVerdict::Ignored)
        );
        assert_eq!(config.title_override("Notepad"), None);
        // Mark wins when both match.
        config.ignore_title_contains.push("my".to_string());
        assert_eq!(
            config.title_override("My Game"),
            Some(OverrideVerdict::Game)
        );
    }

    #[test]
    fn rotation_cycles_through_distinct_actions() {
        assert_eq!(rotation_action(0), ResolvedAction::WTap);
        assert_eq!(rotation_action(1), ResolvedAction::SpaceTap);
        assert_eq!(rotation_action(2), ResolvedAction::CameraNudge);
        // Wraps around.
        assert_eq!(rotation_action(3), ResolvedAction::WTap);
        assert_eq!(rotation_action(u32::MAX), rotation_action(u32::MAX % 3));
    }

    #[test]
    fn resolve_sensitivity_honors_per_target_override() {
        let mut config = AppConfig {
            sensitivity: Sensitivity::Standard,
            ..AppConfig::default()
        };
        assert_eq!(
            config.resolve_sensitivity("a.exe", "X"),
            Sensitivity::Standard
        );

        config.set_profile(
            "a.exe",
            "X",
            TargetProfile {
                sensitivity: Some(Sensitivity::Broad),
                ..TargetProfile::default()
            },
        );
        assert_eq!(config.resolve_sensitivity("a.exe", "X"), Sensitivity::Broad);
        // Other windows keep the global sensitivity.
        assert_eq!(
            config.resolve_sensitivity("b.exe", "Y"),
            Sensitivity::Standard
        );
    }

    #[test]
    fn quiet_hours_handles_wraparound_windows() {
        let mut config = AppConfig {
            quiet_hours_enabled: true,
            quiet_start: "23:00".to_string(),
            quiet_end: "07:00".to_string(),
            ..AppConfig::default()
        };

        // Wednesday (dow 3) — in scope for EveryDay.
        assert!(config.in_quiet_hours(23 * 60 + 30, 3));
        assert!(config.in_quiet_hours(3 * 60, 3));
        assert!(!config.in_quiet_hours(12 * 60, 3));

        config.quiet_start = "09:00".to_string();
        config.quiet_end = "17:00".to_string();
        assert!(config.in_quiet_hours(12 * 60, 3));
        assert!(!config.in_quiet_hours(18 * 60, 3));

        config.quiet_hours_enabled = false;
        assert!(!config.in_quiet_hours(12 * 60, 3));
    }

    #[test]
    fn quiet_days_scopes_to_weekdays_or_weekends() {
        let mut config = AppConfig {
            quiet_hours_enabled: true,
            quiet_start: "09:00".to_string(),
            quiet_end: "17:00".to_string(),
            quiet_days: QuietDays::Weekdays,
            ..AppConfig::default()
        };
        // Wednesday (3) active, Saturday (6) and Sunday (0) skipped.
        assert!(config.in_quiet_hours(12 * 60, 3));
        assert!(!config.in_quiet_hours(12 * 60, 6));
        assert!(!config.in_quiet_hours(12 * 60, 0));

        config.quiet_days = QuietDays::Weekends;
        assert!(!config.in_quiet_hours(12 * 60, 3));
        assert!(config.in_quiet_hours(12 * 60, 6));
        assert!(config.in_quiet_hours(12 * 60, 0));
    }

    #[test]
    fn paused_targets_roundtrip() {
        let mut config = AppConfig::default();
        assert!(!config.is_paused("Game.exe", "CLASS"));
        config.set_paused("Game.exe", "CLASS", true);
        assert!(config.is_paused("game.exe", "CLASS"));
        config.set_paused("GAME.EXE", "CLASS", false);
        assert!(!config.is_paused("game.exe", "CLASS"));
        assert!(config.paused.is_empty());
    }

    #[test]
    fn empty_key_sequence_clears_and_falls_back_to_space_tap() {
        let mut config = AppConfig {
            action: KeepaliveAction::KeySequence,
            key_sequence: Vec::new(),
            ..AppConfig::default()
        };

        assert!(validate_key_sequence(&[]).is_ok());
        assert_eq!(
            config.resolve_keepalive("game.exe", "CLASS").action,
            ResolvedAction::SpaceTap
        );

        config.action = KeepaliveAction::PerTarget;
        config.set_profile(
            "game.exe",
            "CLASS",
            TargetProfile {
                action: Some(TargetAction::KeySequence),
                interval: None,
                key_sequence: vec![],
                ..TargetProfile::default()
            },
        );

        assert_eq!(
            config.resolve_keepalive("game.exe", "CLASS").action,
            ResolvedAction::SpaceTap
        );
    }

    #[test]
    fn resolve_monitor_honors_global_per_target_and_off() {
        let mut config = AppConfig::default();
        assert!(matches!(
            config.resolve_monitor("a.exe", "X"),
            ResolvedMonitor::Off
        ));

        config.monitor_placement = true;
        config.monitor_device = Some(r"\\.\DISPLAY2".to_string());
        assert_eq!(
            config.resolve_monitor("a.exe", "X"),
            ResolvedMonitor::Device(r"\\.\DISPLAY2".to_string())
        );

        config.set_profile(
            "a.exe",
            "X",
            TargetProfile {
                monitor: Some("Don't move".to_string()),
                ..TargetProfile::default()
            },
        );
        assert!(matches!(
            config.resolve_monitor("a.exe", "X"),
            ResolvedMonitor::Off
        ));

        config.set_profile(
            "a.exe",
            "X",
            TargetProfile {
                monitor: Some(r"\\.\DISPLAY3".to_string()),
                ..TargetProfile::default()
            },
        );
        assert_eq!(
            config.resolve_monitor("a.exe", "X"),
            ResolvedMonitor::Device(r"\\.\DISPLAY3".to_string())
        );
    }
}
