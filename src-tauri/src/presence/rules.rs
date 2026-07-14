//! Manifest-driven presence rules (deserialized from community `GameEntry`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PresenceRules {
    pub log: Option<LogPresenceRules>,
    pub screen: Option<ScreenPresenceRules>,
    pub memory: Option<MemoryPresenceRules>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LogPresenceRules {
    /// Glob paths; `%VAR%` env expansion supported.
    pub paths: Vec<String>,
    pub in_game: Vec<String>,
    pub menu: Vec<String>,
    #[serde(default = "default_poll_secs")]
    pub poll_secs: u64,
}

fn default_poll_secs() -> u64 {
    2
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ScreenPresenceRules {
    #[serde(default = "default_sample_w")]
    pub sample_w: u32,
    #[serde(default = "default_sample_h")]
    pub sample_h: u32,
    #[serde(default = "default_interval_secs")]
    pub interval_secs: u64,
    /// Mean per-pixel delta below this → static/menu-like (0.0–1.0).
    #[serde(default = "default_variance_max_menu")]
    pub variance_max_menu: f32,
    /// Mean per-pixel delta above this → active gameplay (0.0–1.0).
    #[serde(default = "default_variance_min_game")]
    pub variance_min_game: f32,
}

fn default_sample_w() -> u32 {
    96
}
fn default_sample_h() -> u32 {
    54
}
fn default_interval_secs() -> u64 {
    8
}
fn default_variance_max_menu() -> f32 {
    0.018
}
fn default_variance_min_game() -> f32 {
    0.045
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryPresenceRules {
    pub reads: Vec<MemoryReadRule>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryReadRule {
    /// Module name (e.g. `game.exe`) or empty for main module.
    pub module: String,
    /// Fixed offset from module base when `signature` is empty.
    pub offset: u64,
    /// Optional `48 8B ?? 05` style pattern; when set, `offset` is added after match.
    pub signature: Option<String>,
    #[serde(default)]
    pub offset_from_match: u64,
    #[serde(default = "default_read_size")]
    pub size: usize,
    #[serde(default)]
    pub in_game_values: Vec<u32>,
    #[serde(default)]
    pub menu_values: Vec<u32>,
}

fn default_read_size() -> usize {
    4
}
