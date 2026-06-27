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

    /// Stretch factor for the next retry once a target keeps failing past the
    /// action-escalation tiers. Caps at 4x so a recovering game is still
    /// retried within a reasonable window instead of hammered every cycle.
    pub const MAX_BACKOFF: u32 = 4;

    pub fn backoff_multiplier(&self) -> u32 {
        let over = self.consecutive_failures.saturating_sub(FAILURE_THRESHOLD);
        (1 + over).min(Self::MAX_BACKOFF)
    }

    pub fn warning(&self) -> Option<String> {
        if self.consecutive_failures >= FAILURE_THRESHOLD {
            let backoff = self.backoff_multiplier();
            let tail = if backoff > 1 {
                format!(", retrying {backoff}x slower")
            } else {
                String::new()
            };
            Some(format!(
                "Keepalive failing ({}x) — {}{tail}",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_then_caps_and_resets_on_success() {
        let mut health = KeepaliveHealth::default();
        // No backoff until we exceed the failure threshold.
        for _ in 0..FAILURE_THRESHOLD {
            health.note_failure(false);
        }
        assert_eq!(health.backoff_multiplier(), 1);

        health.note_failure(false); // threshold + 1
        assert_eq!(health.backoff_multiplier(), 2);

        for _ in 0..20 {
            health.note_failure(false);
        }
        assert_eq!(health.backoff_multiplier(), KeepaliveHealth::MAX_BACKOFF);

        health.note_success();
        assert_eq!(health.backoff_multiplier(), 1);
    }
}
