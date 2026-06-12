//! Adaptive keepalive learning.
//!
//! While the user is actively playing a tracked game, a sampler polls a small
//! whitelist of game-safe keys and builds a per-game frequency histogram.
//! Once a game has enough samples, keepalives draw a weighted-random key from
//! that histogram instead of the global default action — so each game gets
//! inputs shaped like the player's own habits.
//!
//! Privacy: only the whitelisted keys below are ever polled. Typing, chat,
//! and every other key are invisible to the sampler by construction.

use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How often the engine sampler polls held keys while the user plays.
pub const SAMPLE_INTERVAL_MS: u64 = 200;
/// Samples required before a learned profile takes over from the default action.
pub const MIN_SAMPLES: u64 = 50;
/// Only consider the user "playing" when input happened this recently.
pub const ACTIVE_INPUT_MS: u64 = 1500;
const TOP_KEYS: usize = 4;

/// Game-safe keys the sampler is allowed to observe. Names must round-trip
/// through `config::key_name_to_vk` so learned keys can be replayed.
pub const WHITELIST: &[(u16, &str)] = &[
    (0x20, "SPACE"),
    (0x57, "W"),
    (0x41, "A"),
    (0x53, "S"),
    (0x44, "D"),
    (0x10, "SHIFT"),
    (0x45, "E"),
    (0x46, "F"),
    (0x52, "R"),
    (0x51, "Q"),
    (0x26, "UP"),
    (0x28, "DOWN"),
    (0x25, "LEFT"),
    (0x27, "RIGHT"),
];

/// Decaying key-frequency histogram for one game.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LearnedProfile {
    pub counts: BTreeMap<String, u64>,
    pub total: u64,
    /// ISO week of the last decay pass; counts halve when the week changes.
    pub last_decay: String,
}

impl LearnedProfile {
    /// Record one observation of `keys` being held. Returns true when this
    /// sample pushed the profile across the confidence threshold.
    pub fn note(&mut self, keys: &[&str], week: &str) -> bool {
        self.decay_if_due(week);
        let was_confident = self.confident();
        for key in keys {
            *self.counts.entry((*key).to_string()).or_default() += 1;
            self.total += 1;
        }
        !was_confident && self.confident()
    }

    /// Halve all counts when the ISO week changes, so old habits fade and
    /// rebinds or playstyle changes win out over time.
    fn decay_if_due(&mut self, week: &str) {
        if self.last_decay == week {
            return;
        }
        if !self.last_decay.is_empty() {
            for count in self.counts.values_mut() {
                *count /= 2;
            }
            self.counts.retain(|_, count| *count > 0);
            self.total = self.counts.values().sum();
        }
        self.last_decay = week.to_string();
    }

    pub fn confident(&self) -> bool {
        self.total >= MIN_SAMPLES
    }

    /// Weighted-random key draw from the learned distribution.
    pub fn pick(&self, rng: &mut impl Rng) -> Option<String> {
        if self.total == 0 {
            return None;
        }
        let mut roll = rng.gen_range(0..self.total);
        for (key, count) in &self.counts {
            if roll < *count {
                return Some(key.clone());
            }
            roll -= count;
        }
        self.counts.keys().next_back().cloned()
    }

    /// Most-used keys with their share in percent, for the UI.
    pub fn top(&self) -> Vec<TopKey> {
        if self.total == 0 {
            return Vec::new();
        }
        let mut entries: Vec<_> = self.counts.iter().collect();
        entries.sort_by_key(|(key, count)| (std::cmp::Reverse(**count), (*key).clone()));
        entries
            .into_iter()
            .take(TOP_KEYS)
            .map(|(key, count)| TopKey {
                key: key.clone(),
                pct: ((count * 100) / self.total.max(1)) as u8,
            })
            .collect()
    }
}

/// What the frontend sees about one game's learned profile.
#[derive(Debug, Clone, Serialize)]
pub struct TopKey {
    pub key: String,
    pub pct: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct LearnedSnapshot {
    pub samples: u64,
    pub needed: u64,
    /// True when keepalives for this game currently use the learned profile.
    pub active: bool,
    pub top: Vec<TopKey>,
}

pub fn snapshot(profile: &LearnedProfile, active: bool) -> LearnedSnapshot {
    LearnedSnapshot {
        samples: profile.total,
        needed: MIN_SAMPLES,
        active,
        top: profile.top(),
    }
}

/// ISO year-week key used for the decay schedule, e.g. "2026-W24".
pub fn current_week_key() -> String {
    use chrono::Datelike;
    let week = chrono::Local::now().date_naive().iso_week();
    format!("{}-W{:02}", week.year(), week.week())
}

/// Whitelisted keys currently held down (Windows only).
#[cfg(windows)]
pub fn pressed_keys() -> Vec<&'static str> {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    WHITELIST
        .iter()
        .filter(|(vk, _)| {
            // High bit set => key is currently down.
            (unsafe { GetAsyncKeyState(*vk as i32) } as u16) & 0x8000 != 0
        })
        .map(|(_, name)| *name)
        .collect()
}

#[cfg(not(windows))]
pub fn pressed_keys() -> Vec<&'static str> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::key_name_to_vk;
    use rand::{rngs::StdRng, SeedableRng};

    #[test]
    fn whitelist_keys_replay_through_key_sequences() {
        for (vk, name) in WHITELIST {
            assert_eq!(key_name_to_vk(name), Some(*vk), "{name}");
        }
    }

    #[test]
    fn confidence_requires_min_samples() {
        let mut profile = LearnedProfile::default();
        for _ in 0..MIN_SAMPLES - 1 {
            assert!(!profile.note(&["W"], "2026-W24"));
        }
        assert!(!profile.confident());
        // The crossing sample reports the transition exactly once.
        assert!(profile.note(&["SPACE"], "2026-W24"));
        assert!(profile.confident());
        assert!(!profile.note(&["SPACE"], "2026-W24"));
    }

    #[test]
    fn pick_is_weighted_and_within_learned_keys() {
        let mut profile = LearnedProfile::default();
        for _ in 0..90 {
            profile.note(&["W"], "2026-W24");
        }
        for _ in 0..10 {
            profile.note(&["E"], "2026-W24");
        }

        let mut rng = StdRng::seed_from_u64(11);
        let mut w_hits = 0;
        for _ in 0..200 {
            let key = profile.pick(&mut rng).expect("pick");
            assert!(key == "W" || key == "E");
            if key == "W" {
                w_hits += 1;
            }
        }
        // ~90% of draws should be W; leave generous slack for randomness.
        assert!(w_hits > 140, "{w_hits}");
    }

    #[test]
    fn weekly_decay_halves_counts_and_prunes_zeroes() {
        let mut profile = LearnedProfile::default();
        for _ in 0..60 {
            profile.note(&["W"], "2026-W24");
        }
        profile.note(&["E"], "2026-W24");
        assert_eq!(profile.total, 61);

        profile.note(&["W"], "2026-W25");
        // 60/2 + 1/2(=0, pruned) + the new sample.
        assert_eq!(profile.counts.get("W"), Some(&31));
        assert_eq!(profile.counts.get("E"), None);
        assert_eq!(profile.total, 31);
    }

    #[test]
    fn top_reports_percent_shares() {
        let mut profile = LearnedProfile::default();
        for _ in 0..75 {
            profile.note(&["SPACE"], "2026-W24");
        }
        for _ in 0..25 {
            profile.note(&["D"], "2026-W24");
        }
        let top = profile.top();
        assert_eq!(top[0].key, "SPACE");
        assert_eq!(top[0].pct, 75);
        assert_eq!(top[1].key, "D");
        assert_eq!(top[1].pct, 25);
    }
}
