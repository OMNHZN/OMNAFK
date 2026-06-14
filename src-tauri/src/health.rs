//! Per-target keepalive health and session-only fallback tiers.

use crate::config::ResolvedAction;

pub const FAILURE_THRESHOLD: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FallbackTier {
    #[default]
    Normal,
    FocusFlick,
    CameraNudge,
}

impl FallbackTier {
    pub fn next(self, auto_fallback: bool) -> Self {
        if !auto_fallback {
            return self;
        }
        match self {
            Self::Normal => Self::FocusFlick,
            Self::FocusFlick => Self::CameraNudge,
            Self::CameraNudge => Self::CameraNudge,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct KeepaliveHealth {
    pub consecutive_failures: u32,
    pub fallback_tier: FallbackTier,
}

impl KeepaliveHealth {
    pub fn note_success(&mut self) {
        self.consecutive_failures = 0;
        self.fallback_tier = FallbackTier::Normal;
    }

    pub fn note_failure(&mut self, auto_fallback: bool) -> Option<String> {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.consecutive_failures < FAILURE_THRESHOLD {
            return None;
        }
        let previous = self.fallback_tier;
        self.fallback_tier = self.fallback_tier.next(auto_fallback);
        if self.fallback_tier == previous {
            return Some(
                "Keepalives keep failing — try a different action or run OMNAFK as administrator."
                    .to_string(),
            );
        }
        Some(
            match self.fallback_tier {
                FallbackTier::FocusFlick => {
                    "Keepalives failing — trying camera nudge for this game."
                }
                FallbackTier::CameraNudge => {
                    "Keepalives still failing — trying mouse wiggle for this game."
                }
                FallbackTier::Normal => return None,
            }
            .to_string(),
        )
    }

    pub fn warning(&self) -> Option<String> {
        if self.consecutive_failures >= FAILURE_THRESHOLD {
            Some(format!(
                "Keepalive failing ({}x) — {}",
                self.consecutive_failures,
                match self.fallback_tier {
                    FallbackTier::Normal => "try another action or run as administrator",
                    FallbackTier::FocusFlick => "using camera nudge",
                    FallbackTier::CameraNudge => "using mouse wiggle",
                }
            ))
        } else if self.consecutive_failures > 0 {
            Some(format!(
                "Last keepalive failed ({}/{})",
                self.consecutive_failures, FAILURE_THRESHOLD
            ))
        } else {
            None
        }
    }

    pub fn apply_to_options(
        &self,
        action: &ResolvedAction,
        _send_without_focus: bool,
    ) -> (ResolvedAction, bool) {
        match self.fallback_tier {
            FallbackTier::Normal => (action.clone(), false),
            FallbackTier::FocusFlick => (ResolvedAction::CameraNudge, false),
            FallbackTier::CameraNudge => (ResolvedAction::MouseWiggle, false),
        }
    }
}
