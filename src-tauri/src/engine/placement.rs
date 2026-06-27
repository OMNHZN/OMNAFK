use super::EngineInner;
use crate::keepalive::Win32ActivityProbe;
use crate::{
    config::{AppConfig, ResolvedMonitor},
    detector::Verdict,
    health::FAILURE_THRESHOLD,
    monitor::{self, MonitorInfo, PlacementOptions, PlacementResult},
    notifications::QueuedNotice,
};
use std::time::Instant;

pub(crate) struct WindowPlacementPlan {
    pub identity: String,
    pub title: String,
    pub exe: String,
    pub hwnd: isize,
    pub facts: crate::detector::WindowFacts,
    pub target_device: String,
    pub target_label: Option<String>,
    pub placed: bool,
    pub options: PlacementOptions,
    pub prior_failures: u32,
    pub community_enabled: bool,
}

pub(crate) fn monitor_label(monitors: &[MonitorInfo], device: &str) -> Option<String> {
    monitors
        .iter()
        .find(|monitor| monitor.device == device)
        .map(|monitor| monitor.label.clone())
}

impl EngineInner {
    pub(crate) fn plan_window_placements(
        &mut self,
        now: Instant,
        _activity: &Win32ActivityProbe,
    ) -> Vec<WindowPlacementPlan> {
        if !self.config.monitor_placement {
            return Vec::new();
        }

        let monitors = self.cached_monitors(now);
        let options = PlacementOptions {
            when: self.config.monitor_when,
            style: self.config.monitor_style,
            skip_active: self.config.monitor_skip_active,
            skip_active_secs: self.config.monitor_skip_active_secs,
        };
        let community_enabled = self.config.community_intelligence;
        let mut plans = Vec::new();

        let identities: Vec<String> = self.windows.keys().cloned().collect();
        for identity in identities {
            let Some(window) = self.windows.get(&identity) else {
                continue;
            };
            if window.effective != Verdict::Game || window.gone_since.is_some() {
                continue;
            }
            if !window.primary_keepalive {
                continue;
            }
            if self.config.is_paused(&window.exe, &window.wclass) {
                continue;
            }

            match self.config.resolve_monitor(&window.exe, &window.wclass) {
                ResolvedMonitor::Off => {
                    if let Some(window) = self.windows.get_mut(&identity) {
                        window.monitor_status =
                            Some(PlacementResult::SkippedOff.status_label().to_string());
                    }
                    continue;
                }
                ResolvedMonitor::Device(device) => {
                    let label = monitor_label(&monitors, &device);
                    plans.push(WindowPlacementPlan {
                        identity: identity.clone(),
                        title: window.title.clone(),
                        exe: window.exe.clone(),
                        hwnd: window.hwnd,
                        facts: window.facts.clone(),
                        target_device: device,
                        target_label: label,
                        placed: window.monitor_placed,
                        options,
                        prior_failures: window.monitor_move_failures,
                        community_enabled,
                    });
                }
            }
        }

        plans
    }

    pub(crate) fn apply_placement_results(
        &mut self,
        results: Vec<(WindowPlacementPlan, PlacementResult)>,
    ) {
        for (plan, result) in results {
            self.apply_single_placement(&plan, result);
        }
    }

    pub(crate) fn apply_single_placement(
        &mut self,
        plan: &WindowPlacementPlan,
        result: PlacementResult,
    ) {
        if let Some(window) = self.windows.get_mut(&plan.identity) {
            window.monitor_status = Some(result.status_label().to_string());
            if matches!(
                result,
                PlacementResult::Moved | PlacementResult::AlreadyOnTarget
            ) {
                window.monitor_placed = true;
                window.monitor_move_failures = 0;
                if plan.community_enabled {
                    crate::community::record_monitor_result(&plan.exe, true);
                }
            } else if matches!(result, PlacementResult::Failed(_)) {
                window.monitor_move_failures = plan.prior_failures.saturating_add(1);
                if plan.community_enabled {
                    crate::community::record_monitor_result(&plan.exe, false);
                }
                if let Some(hint) =
                    fullscreen_placement_hint(&window.facts, window.monitor_move_failures)
                {
                    window.monitor_status = Some(hint);
                }
            }
        }

        match result {
            PlacementResult::Moved => {
                self.push_log(
                    "info",
                    format!(
                        "Moved {} to {}",
                        plan.title,
                        plan.target_label
                            .clone()
                            .unwrap_or_else(|| plan.target_device.clone())
                    ),
                );
            }
            PlacementResult::Failed(reason) => {
                self.push_log(
                    "error",
                    format!("Monitor move failed for {}: {reason}", plan.title),
                );
                self.push_notice(QueuedNotice::error(
                    format!("Monitor move failed for {}", plan.title),
                    None,
                ));
            }
            _ => {}
        }
    }
}

/// Hint shown when a fullscreen game keeps failing to move. Exclusive
/// fullscreen (covers the screen but isn't a borderless window) almost never
/// accepts a move, so surface the actionable fix after the *first* failure
/// instead of waiting for three wasted attempts.
pub(crate) fn fullscreen_placement_hint(
    facts: &crate::detector::WindowFacts,
    failures: u32,
) -> Option<String> {
    if !facts.fullscreen {
        return None;
    }
    let exclusive = !facts.borderless;
    let threshold = if exclusive { 1 } else { FAILURE_THRESHOLD };
    if failures < threshold {
        return None;
    }
    Some(if exclusive {
        "Exclusive fullscreen resists moving — switch the game to borderless/windowed".to_string()
    } else {
        "Window keeps resisting the move — try a different monitor or placement style".to_string()
    })
}

pub(crate) fn monitor_target_label(config: &AppConfig, exe: &str, wclass: &str) -> Option<String> {
    match config.resolve_monitor(exe, wclass) {
        ResolvedMonitor::Off => None,
        ResolvedMonitor::Device(device) => {
            monitor::monitor_by_device(&device).map(|info| info.label)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detector::WindowFacts;

    fn facts(fullscreen: bool, borderless: bool) -> WindowFacts {
        WindowFacts {
            title: "Game".into(),
            exe: "game.exe".into(),
            wclass: "CLASS".into(),
            pid: 1,
            fullscreen,
            borderless,
            gfx_dll: false,
            platform_path: false,
            known_game: false,
            negative_class: false,
            gpu_active: false,
            audio_active: false,
            elevated: None,
        }
    }

    #[test]
    fn exclusive_fullscreen_hints_after_first_failure() {
        let f = facts(true, false);
        assert!(fullscreen_placement_hint(&f, 0).is_none());
        let hint = fullscreen_placement_hint(&f, 1).expect("hint");
        assert!(hint.contains("borderless/windowed"));
    }

    #[test]
    fn borderless_fullscreen_waits_for_threshold() {
        let f = facts(true, true);
        assert!(fullscreen_placement_hint(&f, 1).is_none());
        assert!(fullscreen_placement_hint(&f, FAILURE_THRESHOLD).is_some());
    }

    #[test]
    fn windowed_never_hints() {
        assert!(fullscreen_placement_hint(&facts(false, false), 99).is_none());
    }
}
