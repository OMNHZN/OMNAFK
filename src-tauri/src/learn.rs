//! Adaptive keepalive learning.

use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SAMPLE_INTERVAL_MS: u64 = 200;
pub const DEFAULT_MIN_SAMPLES: u64 = 50;
pub const ACTIVE_INPUT_MS: u64 = 1500;
const TOP_KEYS: usize = 4;

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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LearnedProfile {
    pub counts: BTreeMap<String, u64>,
    pub sequences: BTreeMap<String, u64>,
    pub successful_actions: BTreeMap<String, u64>,
    pub total: u64,
    pub last_decay: String,
}

impl LearnedProfile {
    pub fn note(
        &mut self,
        keys: &[&str],
        week: &str,
        min_samples: u64,
        learn_sequences: bool,
    ) -> bool {
        self.decay_if_due(week);
        let was_confident = self.confident(min_samples);
        for key in keys {
            *self.counts.entry((*key).to_string()).or_default() += 1;
            self.total += 1;
        }
        if learn_sequences && keys.len() >= 2 {
            let seq = keys.join("+");
            *self.sequences.entry(seq).or_default() += 1;
        }
        !was_confident && self.confident(min_samples)
    }

    pub fn note_action_success(&mut self, action_label: &str) {
        *self
            .successful_actions
            .entry(action_label.to_string())
            .or_default() += 1;
    }

    fn decay_if_due(&mut self, week: &str) {
        if self.last_decay == week {
            return;
        }
        if !self.last_decay.is_empty() {
            for count in self.counts.values_mut() {
                *count /= 2;
            }
            for count in self.sequences.values_mut() {
                *count /= 2;
            }
            for count in self.successful_actions.values_mut() {
                *count /= 2;
            }
            self.counts.retain(|_, count| *count > 0);
            self.sequences.retain(|_, count| *count > 0);
            self.successful_actions.retain(|_, count| *count > 0);
            self.total = self.counts.values().sum();
        }
        self.last_decay = week.to_string();
    }

    pub fn confident(&self, min_samples: u64) -> bool {
        self.total >= min_samples.max(1)
    }

    pub fn pick(
        &self,
        rng: &mut impl Rng,
        learn_sequences: bool,
        learn_actions: bool,
    ) -> AdaptivePick {
        if learn_actions {
            if let Some(action) = self.pick_action(rng) {
                return AdaptivePick::Action(action);
            }
        }
        if learn_sequences {
            if let Some(keys) = self.pick_sequence(rng) {
                return AdaptivePick::Keys(keys);
            }
        }
        self.pick_key(rng)
            .map(|key| AdaptivePick::Keys(vec![key]))
            .unwrap_or(AdaptivePick::None)
    }

    fn pick_action(&self, rng: &mut impl Rng) -> Option<String> {
        let total: u64 = self.successful_actions.values().sum();
        if total == 0 {
            return None;
        }
        let mut roll = rng.gen_range(0..total);
        for (action, count) in &self.successful_actions {
            if roll < *count {
                return Some(action.clone());
            }
            roll -= count;
        }
        self.successful_actions.keys().next_back().cloned()
    }

    fn pick_sequence(&self, rng: &mut impl Rng) -> Option<Vec<String>> {
        let total: u64 = self.sequences.values().sum();
        if total < 10 {
            return None;
        }
        let mut roll = rng.gen_range(0..total);
        let seq = self.sequences.iter().find_map(|(seq, count)| {
            if roll < *count {
                Some(seq.clone())
            } else {
                roll -= count;
                None
            }
        })?;
        Some(seq.split('+').map(str::to_string).collect())
    }

    fn pick_key(&self, rng: &mut impl Rng) -> Option<String> {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdaptivePick {
    None,
    Keys(Vec<String>),
    Action(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct TopKey {
    pub key: String,
    pub pct: u8,
}

#[derive(Debug, Clone, Serialize)]
pub struct LearnedSnapshot {
    pub samples: u64,
    pub needed: u64,
    pub active: bool,
    pub top: Vec<TopKey>,
}

pub fn snapshot(profile: &LearnedProfile, active: bool, needed: u64) -> LearnedSnapshot {
    LearnedSnapshot {
        samples: profile.total,
        needed,
        active,
        top: profile.top(),
    }
}

pub fn current_week_key() -> String {
    use chrono::Datelike;
    let week = chrono::Local::now().date_naive().iso_week();
    format!("{}-W{:02}", week.year(), week.week())
}

#[cfg(windows)]
pub fn pressed_keys() -> Vec<&'static str> {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    WHITELIST
        .iter()
        .filter(|(vk, _)| (unsafe { GetAsyncKeyState(*vk as i32) } as u16) & 0x8000 != 0)
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
    fn confidence_uses_configurable_threshold() {
        let mut profile = LearnedProfile::default();
        for _ in 0..29 {
            profile.note(&["W"], "2026-W24", 30, false);
        }
        assert!(!profile.confident(30));
        assert!(profile.note(&["W"], "2026-W24", 30, false));
    }

    #[test]
    fn sequences_record_when_enabled() {
        let mut profile = LearnedProfile::default();
        profile.note(&["W", "SPACE"], "2026-W24", 1, true);
        assert_eq!(profile.sequences.get("W+SPACE"), Some(&1));
    }

    #[test]
    fn pick_sequence_after_enough_samples() {
        let mut profile = LearnedProfile::default();
        for _ in 0..20 {
            profile.note(&["W", "SPACE"], "2026-W24", 1, true);
        }
        let mut rng = StdRng::seed_from_u64(3);
        match profile.pick(&mut rng, true, false) {
            AdaptivePick::Keys(keys) => {
                assert_eq!(keys, vec!["W".to_string(), "SPACE".to_string()])
            }
            other => panic!("expected sequence pick, got {other:?}"),
        }
    }
}
