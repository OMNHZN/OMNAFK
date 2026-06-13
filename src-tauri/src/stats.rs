use crate::learn::LearnedProfile;
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
};

const DAILY_HISTORY_DAYS: usize = 14;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GameTotals {
    pub title: String,
    pub kept: u64,
    pub actions: u64,
    pub actions_ok: u64,
    pub actions_fail: u64,
}

/// Slice of stats that survives restarts (written to stats.json).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PersistedStats {
    pub lifetime_kept: u64,
    pub lifetime_actions: u64,
    pub longest_streak: u64,
    pub seen_by_date: BTreeMap<String, BTreeSet<String>>,
    pub actions_by_date: BTreeMap<String, u64>,
    pub kept_by_date: BTreeMap<String, u64>,
    pub per_game: BTreeMap<String, GameTotals>,
    pub learned: BTreeMap<String, LearnedProfile>,
}

#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub kept: u64,
    pub actions: u64,
    current_streak: u64,
    longest_streak: u64,
    actions_by_type: BTreeMap<String, u64>,
    persisted: PersistedStats,
    dirty: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct DayStat {
    pub date: String,
    pub seen: usize,
    pub actions: u64,
    pub kept: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct GameTotalsSnapshot {
    pub identity: String,
    pub title: String,
    pub kept: u64,
    pub actions: u64,
    pub actions_ok: u64,
    pub actions_fail: u64,
    pub success_rate: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatsSnapshot {
    pub kept: u64,
    pub actions: u64,
    pub seen: usize,
    pub current_streak: u64,
    pub longest_streak: u64,
    pub lifetime_kept: u64,
    pub lifetime_actions: u64,
    pub actions_by_type: BTreeMap<String, u64>,
    pub daily: Vec<DayStat>,
    pub lifetime_games: Vec<GameTotalsSnapshot>,
}

impl Stats {
    pub fn with_persisted(persisted: PersistedStats) -> Self {
        Self {
            longest_streak: persisted.longest_streak,
            persisted,
            ..Self::default()
        }
    }

    pub fn note_kept(&mut self, identity: &str, title: &str, seconds: u64) {
        self.kept = self.kept.saturating_add(seconds);
        self.current_streak = self.current_streak.saturating_add(seconds);
        if self.current_streak > self.longest_streak {
            self.longest_streak = self.current_streak;
            self.persisted.longest_streak = self.longest_streak;
        }
        self.persisted.lifetime_kept = self.persisted.lifetime_kept.saturating_add(seconds);
        let day = self.persisted.kept_by_date.entry(today_key()).or_default();
        *day = day.saturating_add(seconds);
        let game = self
            .persisted
            .per_game
            .entry(identity.to_string())
            .or_default();
        game.title = title.to_string();
        game.kept = game.kept.saturating_add(seconds);
        self.dirty = true;
    }

    pub fn note_action(&mut self, identity: &str, title: &str, action_label: &str) {
        self.note_action_result(identity, title, action_label, true);
    }

    pub fn note_action_result(
        &mut self,
        identity: &str,
        title: &str,
        action_label: &str,
        ok: bool,
    ) {
        self.actions = self.actions.saturating_add(1);
        *self
            .actions_by_type
            .entry(action_label.to_string())
            .or_default() += 1;
        self.persisted.lifetime_actions = self.persisted.lifetime_actions.saturating_add(1);
        *self
            .persisted
            .actions_by_date
            .entry(today_key())
            .or_default() += 1;
        let game = self
            .persisted
            .per_game
            .entry(identity.to_string())
            .or_default();
        game.title = title.to_string();
        game.actions = game.actions.saturating_add(1);
        if ok {
            game.actions_ok = game.actions_ok.saturating_add(1);
        } else {
            game.actions_fail = game.actions_fail.saturating_add(1);
        }
        self.dirty = true;
    }

    pub fn game_success_rate(&self, identity: &str) -> Option<u8> {
        let game = self.persisted.per_game.get(identity)?;
        if game.actions == 0 {
            return None;
        }
        Some(((game.actions_ok * 100) / game.actions.max(1)) as u8)
    }

    pub fn note_seen_today(&mut self, identity: &str) {
        let today = today_key();
        let inserted = self
            .persisted
            .seen_by_date
            .entry(today)
            .or_default()
            .insert(identity.to_string());
        if inserted {
            self.dirty = true;
        }
    }

    pub fn note_dormant(&mut self) {
        self.current_streak = 0;
    }

    /// Record one adaptive-learning observation. Returns true when this
    /// sample made the game's profile confident for the first time.
    pub fn note_learned_sample(
        &mut self,
        identity: &str,
        keys: &[&str],
        week: &str,
        min_samples: u64,
        learn_sequences: bool,
    ) -> bool {
        if keys.is_empty() {
            return false;
        }
        let crossed = self
            .persisted
            .learned
            .entry(identity.to_string())
            .or_default()
            .note(keys, week, min_samples, learn_sequences);
        self.dirty = true;
        crossed
    }

    pub fn learned_profile(&self, identity: &str) -> Option<&LearnedProfile> {
        self.persisted.learned.get(identity)
    }

    pub fn reset_learned(&mut self, identity: &str) {
        if self.persisted.learned.remove(identity).is_some() {
            self.dirty = true;
        }
    }

    pub fn note_learned_action_success(&mut self, identity: &str, action_label: &str) {
        if let Some(profile) = self.persisted.learned.get_mut(identity) {
            profile.note_action_success(action_label);
            self.dirty = true;
        }
    }

    pub fn reset_session(&mut self) {
        self.kept = 0;
        self.actions = 0;
        self.current_streak = 0;
        self.longest_streak = self.persisted.longest_streak;
        self.actions_by_type.clear();
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    pub fn persisted(&self) -> &PersistedStats {
        &self.persisted
    }

    pub fn snapshot(&self) -> StatsSnapshot {
        let today = today_key();
        let daily = self
            .persisted
            .seen_by_date
            .keys()
            .chain(self.persisted.actions_by_date.keys())
            .chain(self.persisted.kept_by_date.keys())
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .rev()
            .take(DAILY_HISTORY_DAYS)
            .map(|date| DayStat {
                seen: self
                    .persisted
                    .seen_by_date
                    .get(&date)
                    .map(BTreeSet::len)
                    .unwrap_or_default(),
                actions: self
                    .persisted
                    .actions_by_date
                    .get(&date)
                    .copied()
                    .unwrap_or_default(),
                kept: self
                    .persisted
                    .kept_by_date
                    .get(&date)
                    .copied()
                    .unwrap_or_default(),
                date,
            })
            .collect();

        let mut lifetime_games: Vec<_> = self
            .persisted
            .per_game
            .iter()
            .map(|(identity, totals)| GameTotalsSnapshot {
                identity: identity.clone(),
                title: totals.title.clone(),
                kept: totals.kept,
                actions: totals.actions,
                actions_ok: totals.actions_ok,
                actions_fail: totals.actions_fail,
                success_rate: (totals.actions > 0)
                    .then_some(((totals.actions_ok * 100) / totals.actions.max(1)) as u8),
            })
            .collect();
        lifetime_games.sort_by_key(|game| Reverse(game.kept));
        lifetime_games.truncate(25);

        StatsSnapshot {
            kept: self.kept,
            actions: self.actions,
            seen: self
                .persisted
                .seen_by_date
                .get(&today)
                .map(BTreeSet::len)
                .unwrap_or_default(),
            current_streak: self.current_streak,
            longest_streak: self.longest_streak,
            lifetime_kept: self.persisted.lifetime_kept,
            lifetime_actions: self.persisted.lifetime_actions,
            actions_by_type: self.actions_by_type.clone(),
            daily,
            lifetime_games,
        }
    }
}

pub fn today_key() -> String {
    Local::now().date_naive().format("%Y-%m-%d").to_string()
}

pub fn stats_path() -> io::Result<PathBuf> {
    let appdata = dirs::config_dir().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Couldn't find %APPDATA% - restore your Windows profile folders to fix this.",
        )
    })?;
    Ok(appdata.join("OMNAFK").join("stats.json"))
}

pub fn load_persisted() -> PersistedStats {
    stats_path()
        .and_then(|path| load_from_path(&path))
        .unwrap_or_default()
}

pub fn save_persisted(persisted: &PersistedStats) -> io::Result<()> {
    save_to_path(persisted, &stats_path()?)
}

fn load_from_path(path: &Path) -> io::Result<PersistedStats> {
    match fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(PersistedStats::default()),
        Err(error) => Err(error),
    }
}

fn save_to_path(persisted: &PersistedStats, path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_vec_pretty(persisted)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    fs::write(path, json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_session_and_lifetime_totals() {
        let mut stats = Stats::default();
        stats.note_kept("game.exe\u{1f}CLASS", "Game", 30);
        stats.note_action("game.exe\u{1f}CLASS", "Game", "Space tap");
        stats.note_action("game.exe\u{1f}CLASS", "Game", "Space tap");

        let snap = stats.snapshot();
        assert_eq!(snap.kept, 30);
        assert_eq!(snap.actions, 2);
        assert_eq!(snap.lifetime_kept, 30);
        assert_eq!(snap.lifetime_actions, 2);
        assert_eq!(snap.actions_by_type.get("Space tap"), Some(&2));
        assert_eq!(snap.current_streak, 30);
        assert_eq!(snap.longest_streak, 30);
        assert_eq!(snap.lifetime_games.len(), 1);
        assert_eq!(snap.lifetime_games[0].kept, 30);
        assert_eq!(snap.daily.len(), 1);
        assert_eq!(snap.daily[0].actions, 2);

        stats.note_dormant();
        stats.reset_session();
        let snap = stats.snapshot();
        assert_eq!(snap.kept, 0);
        assert_eq!(snap.actions, 0);
        assert_eq!(snap.current_streak, 0);
        // Lifetime survives a session reset.
        assert_eq!(snap.lifetime_kept, 30);
        assert_eq!(snap.longest_streak, 30);
    }

    #[test]
    fn learned_profiles_persist_and_reset() {
        use crate::learn::DEFAULT_MIN_SAMPLES;

        let mut stats = Stats::default();
        for _ in 0..49 {
            assert!(!stats.note_learned_sample(
                "game.exe\u{1f}CLASS",
                &["W"],
                "2026-W24",
                DEFAULT_MIN_SAMPLES,
                false,
            ));
        }
        assert!(stats.note_learned_sample(
            "game.exe\u{1f}CLASS",
            &["SPACE"],
            "2026-W24",
            DEFAULT_MIN_SAMPLES,
            false,
        ));

        let json = serde_json::to_string(stats.persisted()).expect("serialize");
        let restored: PersistedStats = serde_json::from_str(&json).expect("deserialize");
        let revived = Stats::with_persisted(restored);
        let profile = revived
            .learned_profile("game.exe\u{1f}CLASS")
            .expect("profile");
        assert!(profile.confident(DEFAULT_MIN_SAMPLES));
        assert_eq!(profile.counts.get("W"), Some(&49));

        let mut revived = revived;
        revived.reset_learned("game.exe\u{1f}CLASS");
        assert!(revived.learned_profile("game.exe\u{1f}CLASS").is_none());
    }

    #[test]
    fn persisted_roundtrips_through_disk_format() {
        let mut stats = Stats::default();
        stats.note_kept("a", "A", 10);
        stats.note_action("a", "A", "W tap");
        assert!(stats.take_dirty());
        assert!(!stats.take_dirty());

        let json = serde_json::to_string(stats.persisted()).expect("serialize");
        let restored: PersistedStats = serde_json::from_str(&json).expect("deserialize");
        let revived = Stats::with_persisted(restored);
        let snap = revived.snapshot();
        assert_eq!(snap.lifetime_kept, 10);
        assert_eq!(snap.lifetime_actions, 1);
        assert_eq!(snap.longest_streak, 10);
        // Session counters start fresh.
        assert_eq!(snap.kept, 0);
    }
}
