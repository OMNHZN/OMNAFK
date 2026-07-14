//! Layered in-game vs menu presence detection.

mod log;
mod memory;
mod rules;
mod screen;

pub use rules::*;

use crate::menu::{MenuHint, MenuSense};
use log::{LogTailState, LogVote};
use memory::{MemoryReadState, MemoryVote};
use screen::{ScreenSampleState, ScreenVote};
use serde::Serialize;
use std::time::{Duration, Instant};

pub const SINGLE_SOURCE_HIGH: u8 = 90;
pub const COMBINED_THRESHOLD: u8 = 85;
pub const RESPECT_ACT_THRESHOLD: u8 = 85;
pub const DEBOUNCE: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PresenceState {
    #[default]
    Unknown,
    InGame,
    LikelyMenu,
}

#[derive(Debug, Clone, Serialize)]
pub struct PresenceSourceVote {
    pub layer: String,
    pub state: PresenceState,
    pub confidence: u8,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PresenceSnapshot {
    pub state: PresenceState,
    pub confidence: u8,
    pub reason: String,
    pub sources: Vec<PresenceSourceVote>,
    /// When true and `respect_presence` is on, keepalives are held for this target.
    pub hold_keepalives: bool,
}

impl Default for PresenceSnapshot {
    fn default() -> Self {
        Self {
            state: PresenceState::Unknown,
            confidence: 0,
            reason: String::new(),
            sources: Vec::new(),
            hold_keepalives: false,
        }
    }
}

pub struct PresenceInputs<'a> {
    pub gpu_usage: Option<u8>,
    pub title: &'a str,
    pub hwnd: isize,
    pub pid: u32,
    pub rules: Option<&'a PresenceRules>,
    pub log_enabled: bool,
    pub screen_enabled: bool,
    pub memory_enabled: bool,
    pub respect_presence: bool,
    pub now: Instant,
}

#[derive(Debug, Default)]
pub struct PresenceTracker {
    menu: MenuSense,
    log: LogTailState,
    screen: ScreenSampleState,
    memory: MemoryReadState,
    candidate: PresenceState,
    candidate_confidence: u8,
    candidate_reason: String,
    candidate_sources: Vec<PresenceSourceVote>,
    candidate_since: Option<Instant>,
    stable: PresenceSnapshot,
}

impl PresenceTracker {
    pub fn seeded(gpu_usage: Option<u8>, title: &str) -> Self {
        let mut tracker = Self::default();
        tracker.menu.note(gpu_usage, title);
        tracker
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn note(&mut self, inputs: PresenceInputs<'_>) {
        self.menu.note(inputs.gpu_usage, inputs.title);
        let mut votes = Vec::new();

        if let Some(rules) = inputs.rules {
            if inputs.log_enabled {
                if let Some(log_rules) = rules.log.as_ref() {
                    let poll = Duration::from_secs(log_rules.poll_secs.max(1));
                    match self.log.poll(log_rules, poll, inputs.now) {
                        LogVote::InGame => votes.push(PresenceSourceVote {
                            layer: "log".into(),
                            state: PresenceState::InGame,
                            confidence: 93,
                            detail: "log pattern: in session".into(),
                        }),
                        LogVote::Menu => votes.push(PresenceSourceVote {
                            layer: "log".into(),
                            state: PresenceState::LikelyMenu,
                            confidence: 93,
                            detail: "log pattern: menu/lobby".into(),
                        }),
                        LogVote::None => {}
                    }
                }
            }

            if inputs.memory_enabled {
                if let Some(mem_rules) = rules.memory.as_ref() {
                    let interval = Duration::from_secs(3);
                    match self
                        .memory
                        .poll(inputs.pid, mem_rules, interval, inputs.now)
                    {
                        MemoryVote::InGame => votes.push(PresenceSourceVote {
                            layer: "memory".into(),
                            state: PresenceState::InGame,
                            confidence: 92,
                            detail: "memory read: in session".into(),
                        }),
                        MemoryVote::Menu => votes.push(PresenceSourceVote {
                            layer: "memory".into(),
                            state: PresenceState::LikelyMenu,
                            confidence: 92,
                            detail: "memory read: menu state".into(),
                        }),
                        MemoryVote::None => {}
                    }
                }
            }
        }

        if inputs.screen_enabled {
            let default_screen = ScreenPresenceRules::default();
            let screen_rules = inputs
                .rules
                .and_then(|r| r.screen.as_ref())
                .unwrap_or(&default_screen);
            let interval = Duration::from_secs(screen_rules.interval_secs.max(2));
            match self
                .screen
                .sample(inputs.hwnd, screen_rules, interval, inputs.now)
            {
                ScreenVote::InGame => {
                    let var = self
                        .screen
                        .last_variance()
                        .map(|v| format!("{v:.3}"))
                        .unwrap_or_else(|| "?".into());
                    votes.push(PresenceSourceVote {
                        layer: "screen".into(),
                        state: PresenceState::InGame,
                        confidence: 88,
                        detail: format!("frame variance {var}"),
                    });
                }
                ScreenVote::Menu => {
                    let var = self
                        .screen
                        .last_variance()
                        .map(|v| format!("{v:.3}"))
                        .unwrap_or_else(|| "?".into());
                    votes.push(PresenceSourceVote {
                        layer: "screen".into(),
                        state: PresenceState::LikelyMenu,
                        confidence: 88,
                        detail: format!("static frame {var}"),
                    });
                }
                ScreenVote::None => {}
            }
        }

        if let Some(hint) = self.menu.hint() {
            votes.push(PresenceSourceVote {
                layer: "heuristic".into(),
                state: PresenceState::LikelyMenu,
                confidence: hint.confidence,
                detail: hint.reason.clone(),
            });
        }

        let merged = merge_votes(&votes);
        self.apply_debounce(merged, inputs.now, inputs.respect_presence);
    }

    pub fn snapshot(&self) -> PresenceSnapshot {
        self.stable.clone()
    }

    pub fn menu_hint(&self) -> Option<MenuHint> {
        self.menu.hint()
    }

    pub fn should_hold_keepalives(&self) -> bool {
        self.stable.hold_keepalives
    }

    fn apply_debounce(&mut self, merged: MergedVote, now: Instant, respect_presence: bool) {
        if merged.state != self.candidate
            || merged.confidence != self.candidate_confidence
            || merged.reason != self.candidate_reason
        {
            self.candidate = merged.state;
            self.candidate_confidence = merged.confidence;
            self.candidate_reason = merged.reason;
            self.candidate_sources = merged.sources;
            self.candidate_since = Some(now);
        }

        let stable = self
            .candidate_since
            .is_some_and(|since| now.duration_since(since) >= DEBOUNCE)
            || merged.confidence >= SINGLE_SOURCE_HIGH;

        if stable {
            let hold = respect_presence
                && merged.state == PresenceState::LikelyMenu
                && merged.confidence >= RESPECT_ACT_THRESHOLD
                && merged.hold_eligible;
            self.stable = PresenceSnapshot {
                state: merged.state,
                confidence: merged.confidence,
                reason: self.candidate_reason.clone(),
                sources: self.candidate_sources.clone(),
                hold_keepalives: hold,
            };
        } else if merged.confidence == 0 {
            self.stable = PresenceSnapshot::default();
        }
    }
}

struct MergedVote {
    state: PresenceState,
    confidence: u8,
    reason: String,
    sources: Vec<PresenceSourceVote>,
    hold_eligible: bool,
}

fn merge_votes(votes: &[PresenceSourceVote]) -> MergedVote {
    if votes.is_empty() {
        return MergedVote {
            state: PresenceState::Unknown,
            confidence: 0,
            reason: String::new(),
            sources: Vec::new(),
            hold_eligible: false,
        };
    }

    if let Some(top) = votes.iter().find(|v| v.confidence >= SINGLE_SOURCE_HIGH) {
        let hold_eligible = matches!(top.layer.as_str(), "log" | "memory")
            && top.state == PresenceState::LikelyMenu;
        return MergedVote {
            state: top.state,
            confidence: top.confidence,
            reason: format!("{}: {}", top.layer, top.detail),
            sources: votes.to_vec(),
            hold_eligible,
        };
    }

    let mut in_game = 0u32;
    let mut menu = 0u32;
    let mut in_game_conf = 0u32;
    let mut menu_conf = 0u32;
    let mut in_game_best = 0u8;
    let mut menu_best = 0u8;
    for vote in votes {
        match vote.state {
            PresenceState::InGame => {
                in_game += 1;
                in_game_conf = in_game_conf.saturating_add(vote.confidence as u32);
                in_game_best = in_game_best.max(vote.confidence);
            }
            PresenceState::LikelyMenu => {
                menu += 1;
                menu_conf = menu_conf.saturating_add(vote.confidence as u32);
                menu_best = menu_best.max(vote.confidence);
            }
            PresenceState::Unknown => {}
        }
    }

    if in_game >= 2 && menu == 0 {
        let confidence = ((in_game_conf / in_game).min(95) as u8).max(in_game_best);
        if confidence >= COMBINED_THRESHOLD {
            return MergedVote {
                state: PresenceState::InGame,
                confidence,
                reason: format!("{in_game} layers agree: in session"),
                sources: votes.to_vec(),
                hold_eligible: false,
            };
        }
    }
    if menu >= 2 && in_game == 0 {
        let confidence = ((menu_conf / menu).min(95) as u8).max(menu_best);
        if confidence >= COMBINED_THRESHOLD {
            return MergedVote {
                state: PresenceState::LikelyMenu,
                confidence,
                reason: format!("{menu} layers agree: menu/idle"),
                sources: votes.to_vec(),
                hold_eligible: true,
            };
        }
    }

    if let Some(best) = votes.iter().max_by_key(|v| v.confidence) {
        if best.confidence >= 70 {
            return MergedVote {
                state: best.state,
                confidence: best.confidence,
                reason: format!("{}: {}", best.layer, best.detail),
                sources: votes.to_vec(),
                hold_eligible: false,
            };
        }
    }

    MergedVote {
        state: PresenceState::Unknown,
        confidence: 0,
        reason: String::new(),
        sources: votes.to_vec(),
        hold_eligible: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_high_trust_log_wins() {
        let votes = vec![PresenceSourceVote {
            layer: "log".into(),
            state: PresenceState::InGame,
            confidence: 93,
            detail: "joined".into(),
        }];
        let merged = merge_votes(&votes);
        assert_eq!(merged.state, PresenceState::InGame);
        assert!(merged.confidence >= SINGLE_SOURCE_HIGH);
        assert!(!merged.hold_eligible);
    }

    #[test]
    fn two_layers_must_agree_for_combined() {
        let votes = vec![
            PresenceSourceVote {
                layer: "screen".into(),
                state: PresenceState::LikelyMenu,
                confidence: 88,
                detail: "low variance".into(),
            },
            PresenceSourceVote {
                layer: "heuristic".into(),
                state: PresenceState::LikelyMenu,
                confidence: 78,
                detail: "gpu drop".into(),
            },
        ];
        let merged = merge_votes(&votes);
        assert_eq!(merged.state, PresenceState::LikelyMenu);
        assert!(merged.confidence >= COMBINED_THRESHOLD);
        assert!(merged.hold_eligible);
    }

    #[test]
    fn screen_alone_can_hint_but_cannot_hold() {
        let votes = vec![PresenceSourceVote {
            layer: "screen".into(),
            state: PresenceState::LikelyMenu,
            confidence: 88,
            detail: "static frame".into(),
        }];
        let merged = merge_votes(&votes);
        assert_eq!(merged.state, PresenceState::LikelyMenu);
        assert!(!merged.hold_eligible);
    }
}
