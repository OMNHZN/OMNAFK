use chrono::Local;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub kept: u64,
    pub actions: u64,
    current_streak: u64,
    longest_streak: u64,
    seen_by_date: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct StatsSnapshot {
    pub kept: u64,
    pub actions: u64,
    pub seen: usize,
    pub longest_streak: u64,
}

impl Stats {
    pub fn note_kept(&mut self, seconds: u64) {
        self.kept = self.kept.saturating_add(seconds);
        self.current_streak = self.current_streak.saturating_add(seconds);
        if self.current_streak > self.longest_streak {
            self.longest_streak = self.current_streak;
        }
    }

    pub fn note_action(&mut self) {
        self.actions = self.actions.saturating_add(1);
    }

    pub fn note_seen_today(&mut self, identity: &str) {
        let today = today_key();
        self.seen_by_date
            .entry(today)
            .or_default()
            .insert(identity.to_string());
    }

    pub fn note_dormant(&mut self) {
        self.current_streak = 0;
    }

    pub fn reset_session(&mut self) {
        self.kept = 0;
        self.actions = 0;
        self.current_streak = 0;
        self.longest_streak = 0;
    }

    pub fn snapshot(&self) -> StatsSnapshot {
        let today = today_key();
        StatsSnapshot {
            kept: self.kept,
            actions: self.actions,
            seen: self
                .seen_by_date
                .get(&today)
                .map(BTreeSet::len)
                .unwrap_or_default(),
            longest_streak: self.longest_streak,
        }
    }
}

pub fn today_key() -> String {
    Local::now().date_naive().format("%Y-%m-%d").to_string()
}
