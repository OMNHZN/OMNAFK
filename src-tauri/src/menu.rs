//! Heuristic "probably idling at a menu" sensing.
//!
//! Watches each tracked window's GPU load history and window title. Gameplay
//! establishes a per-window load baseline (the peak); a sustained drop to a
//! small fraction of that peak reads as a menu or pause screen. A window title
//! that reverts to its launch-time value after having changed (many games show
//! the bare game name at menus and append map/world names in-game) boosts
//! confidence. This is a *hint* only — the engine never gates keepalives on it.

use serde::Serialize;
use std::collections::VecDeque;

/// Samples arrive on the ~5s detection cadence; 12 samples ≈ one minute.
const HISTORY_CAP: usize = 12;
/// Recent samples averaged when judging the current load level.
const RECENT_WINDOW: usize = 6;
/// The window must have hit at least this GPU load before menu sensing arms —
/// without a gameplay baseline there is nothing to compare against.
const MIN_PEAK: u8 = 25;
/// Recent load at or below this fraction of the peak reads as menu-like.
const MENU_RATIO: f32 = 0.35;
/// Hints below this confidence are suppressed as noise.
const MIN_CONFIDENCE: u8 = 55;

/// Rolling per-window menu detector fed by the detection loop.
#[derive(Debug, Default)]
pub struct MenuSense {
    samples: VecDeque<u8>,
    peak: u8,
    base_title: Option<String>,
    saw_other_title: bool,
    current_is_base: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct MenuHint {
    /// 0-100 confidence that the game is idling at a menu or pause screen.
    pub confidence: u8,
    pub reason: String,
}

impl MenuSense {
    pub fn seeded(gpu_usage: Option<u8>, title: &str) -> Self {
        let mut sense = Self::default();
        sense.note(gpu_usage, title);
        sense
    }

    /// Record one detection-cycle observation.
    pub fn note(&mut self, gpu_usage: Option<u8>, title: &str) {
        match &self.base_title {
            None => self.base_title = Some(title.to_string()),
            Some(base) => {
                if title != base {
                    self.saw_other_title = true;
                }
            }
        }
        self.current_is_base = self.base_title.as_deref() == Some(title);

        if let Some(usage) = gpu_usage {
            self.peak = self.peak.max(usage);
            self.samples.push_back(usage);
            while self.samples.len() > HISTORY_CAP {
                self.samples.pop_front();
            }
        }
    }

    /// The window handle changed (relaunch/new session) — old baseline is stale.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Current menu hint, `None` until the signal is both armed and confident.
    pub fn hint(&self) -> Option<MenuHint> {
        if self.samples.len() < RECENT_WINDOW || self.peak < MIN_PEAK {
            return None;
        }
        let recent = self.samples.iter().rev().take(RECENT_WINDOW);
        let avg = recent.map(|&v| f32::from(v)).sum::<f32>() / RECENT_WINDOW as f32;
        let ratio = avg / f32::from(self.peak);
        if ratio > MENU_RATIO {
            return None;
        }

        let mut confidence = 55.0 + (MENU_RATIO - ratio) / MENU_RATIO * 30.0;
        let mut reason = format!("GPU load {:.0}% vs {}% gameplay peak", avg, self.peak);
        if self.saw_other_title && self.current_is_base {
            confidence += 15.0;
            reason.push_str(", title back to launch state");
        }

        let confidence = confidence.min(95.0) as u8;
        if confidence < MIN_CONFIDENCE {
            return None;
        }
        Some(MenuHint { confidence, reason })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(sense: &mut MenuSense, usages: &[u8], title: &str) {
        for &usage in usages {
            sense.note(Some(usage), title);
        }
    }

    #[test]
    fn no_hint_without_gameplay_baseline() {
        let mut sense = MenuSense::default();
        // Low load the whole time — could be a menu, but we never saw gameplay.
        feed(&mut sense, &[5, 6, 4, 5, 6, 5, 4, 5], "Game");
        assert!(sense.hint().is_none());
    }

    #[test]
    fn no_hint_while_load_stays_high() {
        let mut sense = MenuSense::default();
        feed(&mut sense, &[60, 65, 70, 62, 68, 64, 66, 63], "Game");
        assert!(sense.hint().is_none());
    }

    #[test]
    fn sustained_low_load_after_gameplay_reads_as_menu() {
        let mut sense = MenuSense::default();
        feed(&mut sense, &[70, 75, 72], "Game");
        feed(&mut sense, &[8, 6, 7, 5, 8, 6], "Game");
        let hint = sense.hint().expect("menu hint");
        assert!(hint.confidence >= MIN_CONFIDENCE);
        assert!(hint.reason.contains("gameplay peak"));
    }

    #[test]
    fn brief_dip_does_not_trigger() {
        let mut sense = MenuSense::default();
        feed(&mut sense, &[70, 75, 72, 68, 71], "Game");
        // One low sample (loading screen) mixed with high load.
        feed(&mut sense, &[6, 70, 72, 69, 74, 71], "Game");
        assert!(sense.hint().is_none());
    }

    #[test]
    fn title_reverting_to_launch_state_boosts_confidence() {
        let mut plain = MenuSense::default();
        feed(&mut plain, &[70, 75, 72], "Game");
        feed(&mut plain, &[8, 6, 7, 5, 8, 6], "Game");
        let base = plain.hint().expect("hint").confidence;

        let mut reverted = MenuSense::default();
        feed(&mut reverted, &[70], "Game");
        feed(&mut reverted, &[75, 72], "Game - Ashen Keep");
        feed(&mut reverted, &[8, 6, 7, 5, 8, 6], "Game");
        let boosted = reverted.hint().expect("hint");
        assert!(boosted.confidence > base);
        assert!(boosted.reason.contains("title back to launch state"));
    }

    #[test]
    fn reset_clears_the_baseline() {
        let mut sense = MenuSense::default();
        feed(&mut sense, &[70, 75, 72], "Game");
        feed(&mut sense, &[8, 6, 7, 5, 8, 6], "Game");
        assert!(sense.hint().is_some());
        sense.reset();
        assert!(sense.hint().is_none());
    }
}
