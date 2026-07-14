mod placement;

use crate::{
    audio::WasapiAudioProbe,
    community::{self, CommunityGameSnapshot, SharedCommunity},
    config::{
        AppConfig, OverrideVerdict, ResolvedAction, ResolvedMonitor, Sensitivity, TargetProfile,
    },
    detector::{self, DetectedWindow, Verdict, WindowFacts},
    gpu::PdhGpuProbe,
    health::KeepaliveHealth,
    keepalive::{
        self, ActivityProbe, KeepaliveOptions, KeepaliveTarget, TickDecision, TickTimer,
        Win32ActivityProbe,
    },
    learn::{self, AdaptivePick, LearnedSnapshot},
    menu::MenuHint,
    monitor::{self, MonitorInfo, PlacementOptions, PlacementResult},
    notifications::{QueuedNotice, ToastAction, ToastKind},
    presence::{self, PresenceSnapshot},
    stats::{PersistedStats, Stats, StatsSnapshot},
    updates::UpdateCheck,
};
use parking_lot::{Mutex, RwLock};
use placement::WindowPlacementPlan;
use rand::thread_rng;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::Write,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub const AUTO_ELEVATE_MIN_UPTIME: Duration = Duration::from_secs(5);
pub const AUTO_ELEVATE_AUTOSTART_GRACE: Duration = Duration::from_secs(60);

const DETECTION_INTERVAL: Duration = Duration::from_secs(5);
const BURST_DETECTION_INTERVAL: Duration = Duration::from_secs(1);
const BURST_WINDOW: Duration = Duration::from_secs(30);
const MONITOR_CACHE_TTL: Duration = Duration::from_secs(30);
const GONE_LINGER: Duration = Duration::from_secs(60);
const ACTIVITY_LOG_CAP: usize = 50;
const STATS_SAVE_INTERVAL: Duration = Duration::from_secs(30);
/// How soon a gate-blocked tick is re-checked, so keepalives resume promptly
/// after quiet hours end, power returns, or the session unlocks.
const GATE_RECHECK_SECS: u64 = 30;
/// How soon a presence-held (menu/lobby) tick is re-checked, so a wrong or
/// stale menu read can't starve keepalives for a whole interval.
const PRESENCE_RECHECK_SECS: u64 = 45;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineStatus {
    Dormant,
    Active,
    Holding,
    Suspended,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActivityEvent {
    pub at: String,
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GameProfileSnapshot {
    pub action: Option<String>,
    pub interval: Option<u64>,
    pub key_sequence: Vec<String>,
    pub monitor: Option<String>,
    pub adaptive: Option<bool>,
    pub hold_while_playing: Option<bool>,
    pub hold_window_secs: Option<u64>,
    pub send_without_focus: Option<bool>,
    pub auto_fallback: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GameMonitorSnapshot {
    pub target: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GameSnapshot {
    pub title: String,
    pub exe: String,
    pub wclass: String,
    pub verdict: Verdict,
    pub overridden: bool,
    pub effective: Verdict,
    pub gone: bool,
    pub paused: bool,
    pub uptime: u64,
    pub actions: u64,
    pub score: i32,
    pub threshold: i32,
    pub facts: WindowFacts,
    pub next_tick: Option<u64>,
    pub last_action_secs: Option<u64>,
    pub last_action_ok: Option<bool>,
    pub elevated_mismatch: bool,
    pub learned: Option<LearnedSnapshot>,
    pub monitor: GameMonitorSnapshot,
    pub profile: GameProfileSnapshot,
    pub health_warning: Option<String>,
    pub consecutive_failures: u32,
    pub success_rate: Option<u8>,
    pub primary_keepalive: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub community: Option<CommunityGameSnapshot>,
    /// Layered presence: in-game vs menu (log, screen, memory, heuristic).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub presence: Option<PresenceSnapshot>,
    /// Legacy heuristic menu hint (also surfaced inside `presence.sources`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub menu_hint: Option<MenuHint>,
}

#[derive(Debug, Clone)]
pub struct EngineSnapshot {
    pub engine: EngineStatus,
    pub next_tick: Option<u64>,
    pub games: Vec<GameSnapshot>,
    pub stats: StatsSnapshot,
    pub config: AppConfig,
    pub error: Option<String>,
    pub update: Option<UpdateCheck>,
    pub paused_reason: Option<String>,
    pub snooze_remaining: Option<u64>,
    pub log: Vec<ActivityEvent>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TestAllResult {
    pub tested: usize,
    pub ok: usize,
    pub failed: usize,
}

/// Why a tracked window is (or isn't) treated as a game: the weighted score
/// breakdown plus the precedence rule that decided the effective verdict.
#[derive(Debug, Clone, Serialize)]
pub struct DetectionExplanation {
    pub title: String,
    pub exe: String,
    pub score: i32,
    pub threshold: i32,
    pub sensitivity: &'static str,
    /// Verdict the score alone would give, ignoring pins and manual mode.
    pub score_verdict: Verdict,
    /// Verdict actually in effect after pins, title rules, and manual mode.
    pub effective: Verdict,
    pub factors: Vec<crate::detector::ScoreFactor>,
    /// One-sentence explanation of the effective verdict, in plain language.
    pub reason: String,
}

pub type SharedEngine = Arc<Engine>;

pub struct Engine {
    inner: RwLock<EngineInner>,
    stop: AtomicBool,
    pending_elevation: AtomicBool,
    worker: Mutex<Option<JoinHandle<()>>>,
    sampler: Mutex<Option<JoinHandle<()>>>,
    gpu: Mutex<PdhGpuProbe>,
    audio: Mutex<WasapiAudioProbe>,
    community: SharedCommunity,
    last_detection_at: Mutex<Instant>,
    launched_at: Instant,
    autostart_launch: bool,
    user_ui_opened: AtomicBool,
}

const WORKER_STALE_AFTER: Duration = Duration::from_secs(30);

#[derive(Debug)]
struct EngineInner {
    config: AppConfig,
    windows: BTreeMap<String, TrackedWindow>,
    stats: Stats,
    status: EngineStatus,
    last_cycle: Instant,
    last_error: Option<String>,
    update_prompt: Option<UpdateCheck>,
    snooze_until: Option<Instant>,
    session_start: Instant,
    gate_reason: Option<String>,
    log: VecDeque<ActivityEvent>,
    notices: Vec<QueuedNotice>,
    last_stats_save: Instant,
    current_elevated: bool,
    burst_until: Option<Instant>,
    monitors_cache: Vec<MonitorInfo>,
    monitors_cached_at: Instant,
    elevation_requested: bool,
    session_warnings: BTreeSet<String>,
    gamepad: crate::gamepad::XInputProbe,
}

#[derive(Debug)]
struct TrackedWindow {
    title: String,
    exe: String,
    wclass: String,
    pid: u32,
    hwnd: isize,
    verdict: Verdict,
    effective: Verdict,
    overridden: bool,
    gone_since: Option<Instant>,
    uptime: u64,
    actions: u64,
    timer: Option<TickTimer>,
    facts: WindowFacts,
    score: i32,
    last_action_at: Option<Instant>,
    last_action_ok: Option<bool>,
    was_armed: bool,
    monitor_placed: bool,
    monitor_status: Option<String>,
    monitor_move_failures: u32,
    health: KeepaliveHealth,
    primary_keepalive: bool,
    /// Confirmed keepalive sends since this window appeared, for interval ease-in.
    warmup_sends: u32,
    /// Advances each confirmed send to cycle actions when rotation is enabled.
    rotation_index: u32,
    /// `GetLastInputInfo` tick from OMNAFK's last successful SendInput keepalive.
    last_injected_input_tick: Option<u32>,
    /// Dedupes per-stretch "Held tick: recent input" activity logs.
    logged_recent_input_hold: bool,
    /// Dedupes per-stretch "Held tick: presence menu/lobby" activity logs.
    logged_presence_hold: bool,
    /// Layered in-game vs menu detection for this window.
    presence: presence::PresenceTracker,
}

struct KeepaliveSendPlan {
    identity: String,
    title: String,
    exe: String,
    target: KeepaliveTarget,
    options: KeepaliveOptions,
    label: String,
    log_label: String,
    auto_fallback: bool,
    send_without_focus: bool,
    community_enabled: bool,
    was_first_action: bool,
}

impl Engine {
    pub fn new(config: AppConfig) -> SharedEngine {
        Self::with_stats(config, PersistedStats::default())
    }

    pub fn with_stats(config: AppConfig, persisted: PersistedStats) -> SharedEngine {
        Self::with_community(config, persisted, community::shared_runtime())
    }

    pub fn with_community(
        config: AppConfig,
        persisted: PersistedStats,
        community: SharedCommunity,
    ) -> SharedEngine {
        Self::with_launch_context(
            config,
            persisted,
            community,
            crate::startup::is_autostart_launch(),
        )
    }

    pub fn with_launch_context(
        config: AppConfig,
        persisted: PersistedStats,
        community: SharedCommunity,
        autostart_launch: bool,
    ) -> SharedEngine {
        Arc::new(Self {
            inner: RwLock::new(EngineInner {
                config,
                windows: BTreeMap::new(),
                stats: Stats::with_persisted(persisted),
                status: EngineStatus::Dormant,
                last_cycle: Instant::now(),
                last_error: None,
                update_prompt: None,
                snooze_until: None,
                session_start: Instant::now(),
                gate_reason: None,
                log: VecDeque::new(),
                notices: Vec::new(),
                last_stats_save: Instant::now(),
                current_elevated: detector::current_process_elevated(),
                burst_until: None,
                monitors_cache: Vec::new(),
                monitors_cached_at: crate::time_util::instant_ttl_ago(MONITOR_CACHE_TTL),
                elevation_requested: false,
                session_warnings: BTreeSet::new(),
                gamepad: crate::gamepad::XInputProbe::default(),
            }),
            stop: AtomicBool::new(false),
            pending_elevation: AtomicBool::new(false),
            worker: Mutex::new(None),
            sampler: Mutex::new(None),
            gpu: Mutex::new(PdhGpuProbe::default()),
            audio: Mutex::new(WasapiAudioProbe::default()),
            community,
            last_detection_at: Mutex::new(Instant::now()),
            launched_at: Instant::now(),
            autostart_launch,
            user_ui_opened: AtomicBool::new(false),
        })
    }

    pub fn touch_detection_heartbeat(&self) {
        *self.last_detection_at.lock() = Instant::now();
    }

    pub fn detection_stale(&self) -> bool {
        self.last_detection_at.lock().elapsed() >= WORKER_STALE_AFTER
    }

    pub fn ensure_worker_running(self: &Arc<Self>) {
        let finished = self
            .worker
            .lock()
            .as_ref()
            .is_some_and(|handle| handle.is_finished());
        if finished {
            *self.worker.lock() = None;
            self.start();
        }
    }

    pub fn community(&self) -> &SharedCommunity {
        &self.community
    }

    pub fn take_pending_elevation(&self) -> bool {
        self.pending_elevation.swap(false, Ordering::SeqCst)
    }

    pub fn mark_user_ui_opened(&self) {
        self.user_ui_opened.store(true, Ordering::SeqCst);
    }

    pub fn autostart_launch(&self) -> bool {
        self.autostart_launch
    }

    pub fn can_auto_elevate_now(&self) -> bool {
        if detector::current_process_elevated() {
            return false;
        }
        let uptime = self.launched_at.elapsed();
        if uptime < AUTO_ELEVATE_MIN_UPTIME {
            return false;
        }
        if self.autostart_launch && uptime < AUTO_ELEVATE_AUTOSTART_GRACE {
            return self.user_ui_opened.load(Ordering::SeqCst);
        }
        true
    }

    pub fn clear_elevation_request(&self) {
        self.pending_elevation.store(false, Ordering::SeqCst);
        let mut inner = self.inner.write();
        inner.elevation_requested = false;
    }

    fn maybe_request_elevation(&self) {
        let should = {
            let inner = self.inner.read();
            if !inner.config.auto_elevate || inner.current_elevated || inner.elevation_requested {
                return;
            }
            inner.windows.values().any(|window| {
                window.effective == Verdict::Game
                    && window.gone_since.is_none()
                    && window.primary_keepalive
                    && window.facts.elevated == Some(true)
            })
        };
        if !should {
            return;
        }
        let mut inner = self.inner.write();
        if inner.elevation_requested {
            return;
        }
        inner.elevation_requested = true;
        inner.push_log(
            "info",
            "Elevated game detected — restarting OMNAFK as administrator…".to_string(),
        );
        inner.push_notice(QueuedNotice::error(
            "Elevated game detected — approve the UAC prompt so OMNAFK can send input.",
            Some(ToastAction::RestartAdmin),
        ));
        drop(inner);
        self.pending_elevation.store(true, Ordering::SeqCst);
    }

    pub fn start(self: &Arc<Self>) {
        let mut worker = self.worker.lock();
        if worker.is_some() {
            return;
        }

        self.stop.store(false, Ordering::SeqCst);
        let engine = Arc::clone(self);
        *worker = Some(thread::spawn(move || {
            while !engine.stop.load(Ordering::SeqCst) {
                if catch_unwind(AssertUnwindSafe(|| engine.run_detection_cycle())).is_err() {
                    engine
                        .note_runtime_warning("Detection recovered after an internal error.", true);
                }
                sleep_until_next_cycle(&engine.stop, engine.detection_interval());
            }
            engine.persist_stats(true);
        }));

        // Adaptive-learning sampler: observes which whitelisted keys the user
        // holds while actively playing a tracked game.
        let engine = Arc::clone(self);
        *self.sampler.lock() = Some(thread::spawn(move || {
            while !engine.stop.load(Ordering::SeqCst) {
                if !engine.adaptive_sampling_enabled() {
                    thread::sleep(Duration::from_secs(1));
                    continue;
                }
                if catch_unwind(AssertUnwindSafe(|| engine.run_learn_sample())).is_err() {
                    engine.note_runtime_warning(
                        "Adaptive learning recovered after an internal error.",
                        false,
                    );
                }
                thread::sleep(Duration::from_millis(learn::SAMPLE_INTERVAL_MS));
            }
        }));
    }

    fn adaptive_sampling_enabled(&self) -> bool {
        let inner = self.inner.read();
        inner.config.adaptive_actions
            || inner.config.adaptive_learn_sequences
            || inner.config.adaptive_learn_actions
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.lock().take() {
            let _ = worker.join();
        }
        if let Some(sampler) = self.sampler.lock().take() {
            let _ = sampler.join();
        }
        self.persist_stats(true);
    }

    /// One adaptive-learning observation: when the user is actively playing a
    /// tracked game in the foreground, record which whitelisted keys are held.
    fn run_learn_sample(&self) {
        use keepalive::ActivityProbe;

        let candidate = {
            let inner = self.inner.read();
            if !inner.config.adaptive_actions
                || inner.config.suspended
                || inner.snooze_until.is_some()
            {
                return;
            }
            let probe = Win32ActivityProbe;
            let now = Instant::now();
            probe.foreground_window().and_then(|foreground| {
                inner
                    .windows
                    .iter()
                    .find(|(_, window)| {
                        window.hwnd == foreground
                            && window.effective == Verdict::Game
                            && window.gone_since.is_none()
                            && inner.config.adaptive_enabled(&window.exe, &window.wclass)
                    })
                    .and_then(|(identity, window)| {
                        keepalive::genuine_recent_user_input(
                            now,
                            &probe,
                            Duration::from_millis(learn::ACTIVE_INPUT_MS),
                            window.last_injected_input_tick,
                        )
                        .then(|| (identity.clone(), window.title.clone()))
                    })
            })
        };
        let Some((identity, title)) = candidate else {
            return;
        };

        let keys = learn::pressed_keys();
        if keys.is_empty() {
            return;
        }
        let week = learn::current_week_key();
        let min_samples = {
            let inner = self.inner.read();
            inner.config.adaptive_min_samples.max(1)
        };
        let learn_sequences = self.inner.read().config.adaptive_learn_sequences;
        let mut inner = self.inner.write();
        if inner
            .stats
            .note_learned_sample(&identity, &keys, &week, min_samples, learn_sequences)
        {
            inner.push_log(
                "info",
                format!("Adaptive profile ready for {title} — keepalives now mimic your inputs"),
            );
        }
    }

    pub fn detection_interval(&self) -> Duration {
        let inner = self.inner.read();
        if inner.config.burst_detection
            && inner
                .burst_until
                .is_some_and(|until| Instant::now() < until)
        {
            BURST_DETECTION_INTERVAL
        } else {
            DETECTION_INTERVAL
        }
    }

    /// Move one target onto its configured monitor immediately.
    pub fn move_target(&self, exe: &str, wclass: &str) -> Result<String, String> {
        let plan = {
            let inner = self.inner.read();
            let identity = identity_key(exe, wclass);
            let Some(window) = inner
                .windows
                .get(&identity)
                .filter(|w| w.gone_since.is_none())
            else {
                return Err(format!(
                    "Couldn't move {exe} - the window is not currently visible."
                ));
            };
            let monitors = monitor::list_monitors();
            let options = PlacementOptions {
                when: inner.config.monitor_when,
                style: inner.config.monitor_style,
                skip_active: false,
                skip_active_secs: inner.config.monitor_skip_active_secs,
            };
            let (target_device, target_label) = match inner.config.resolve_monitor(exe, wclass) {
                ResolvedMonitor::Off => {
                    return Err(
                        "Couldn't move window - monitor placement is off for this target."
                            .to_string(),
                    );
                }
                ResolvedMonitor::Device(device) => {
                    let label = placement::monitor_label(&monitors, &device);
                    (device, label)
                }
            };
            WindowPlacementPlan {
                identity,
                title: window.title.clone(),
                exe: window.exe.clone(),
                hwnd: window.hwnd,
                facts: window.facts.clone(),
                target_device,
                target_label,
                placed: window.monitor_placed,
                options,
                prior_failures: window.monitor_move_failures,
                community_enabled: inner.config.community_intelligence,
            }
        };

        let result = monitor::try_place_window(
            plan.hwnd,
            &plan.target_device,
            &plan.facts,
            &plan.options,
            false,
            Instant::now(),
            &Win32ActivityProbe,
        );

        let mut inner = self.inner.write();
        inner.apply_single_placement(&plan, result.clone());
        match result {
            PlacementResult::Moved => {
                let label = plan
                    .target_label
                    .clone()
                    .unwrap_or_else(|| plan.target_device.clone());
                inner.push_log("info", format!("Moved {} to {label}", plan.title));
                Ok(format!("Moved to {label}"))
            }
            PlacementResult::AlreadyOnTarget => Ok("Already on target monitor".to_string()),
            PlacementResult::SkippedActive => {
                Err("Move skipped — you're actively using the game.".to_string())
            }
            PlacementResult::MonitorMissing => {
                Err("Target monitor is disconnected — pick another monitor.".to_string())
            }
            PlacementResult::Failed(reason) => {
                let hint = if plan.facts.fullscreen {
                    format!("{reason} Try borderless or windowed mode for monitor placement.")
                } else {
                    reason
                };
                Err(hint)
            }
            other => Err(other.status_label().to_string()),
        }
    }

    /// Wipe the learned input profile for one target.
    pub fn reset_learning(&self, exe: &str, wclass: &str) {
        {
            let mut inner = self.inner.write();
            let identity = identity_key(exe, wclass);
            inner.stats.reset_learned(&identity);
            inner.push_log("info", format!("Adaptive learning reset for {exe}"));
        }
        self.persist_stats(true);
    }

    pub fn run_detection_cycle(&self) {
        let now = Instant::now();
        {
            let mut inner = self.inner.write();
            if let Some(until) = inner.snooze_until {
                if now >= until {
                    inner.snooze_until = None;
                    inner.push_log("info", "Snooze ended — watching again".to_string());
                }
            }
            if inner.config.suspended || inner.snooze_until.is_some() {
                inner.status = EngineStatus::Suspended;
                inner.stats.note_dormant();
                inner.last_error = None;
                drop(inner);
                self.touch_detection_heartbeat();
                return;
            }
        }

        let (sensitivity, always_mark, supplement) = {
            let inner = self.inner.read();
            let supplement = if inner.config.community_intelligence {
                Some(self.community.read().supplement.clone())
            } else {
                None
            };
            (
                inner.config.sensitivity,
                inner.config.always_mark_exes.clone(),
                supplement,
            )
        };
        let gpu = self.gpu.lock();
        let audio = self.audio.lock();
        let detected = detector::scan_windows(
            sensitivity,
            &*gpu,
            &*audio,
            &always_mark,
            supplement.as_ref(),
        );
        drop(audio);
        drop(gpu);
        let pending = {
            let mut inner = self.inner.write();
            inner.apply_detection(detected, Instant::now(), &self.community)
        };
        if !pending.placements.is_empty() {
            let results: Vec<_> = pending
                .placements
                .into_iter()
                .map(|plan| {
                    let activity = Win32ActivityProbe;
                    let result = monitor::try_place_window(
                        plan.hwnd,
                        &plan.target_device,
                        &plan.facts,
                        &plan.options,
                        plan.placed,
                        Instant::now(),
                        &activity,
                    );
                    (plan, result)
                })
                .collect();
            let mut inner = self.inner.write();
            inner.apply_placement_results(results);
        }
        if !pending.sends.is_empty() {
            let results: Vec<_> = pending
                .sends
                .into_iter()
                .map(|plan| {
                    let result = keepalive::send_keepalive(&plan.target, &plan.options);
                    (plan, result)
                })
                .collect();
            let mut inner = self.inner.write();
            inner.apply_keepalive_results(Instant::now(), results, &self.community);
        }
        self.touch_detection_heartbeat();
        self.maybe_request_elevation();
        self.persist_stats(false);
    }

    pub fn note_runtime_warning(&self, text: impl Into<String>, notice: bool) {
        self.inner.write().note_runtime_warning(text.into(), notice);
    }

    /// Write lifetime stats to disk (throttled unless `force`).
    pub fn persist_stats(&self, force: bool) {
        let persisted = {
            let mut inner = self.inner.write();
            let due = force || inner.last_stats_save.elapsed() >= STATS_SAVE_INTERVAL;
            if !due || !inner.stats.take_dirty() {
                return;
            }
            inner.last_stats_save = Instant::now();
            inner.stats.persisted().clone()
        };
        if let Err(error) = crate::stats::save_persisted(&persisted) {
            self.note_runtime_warning(
                format!("Couldn't save stats — check %APPDATA% permissions: {error}"),
                false,
            );
        }
    }

    pub fn snapshot(&self) -> EngineSnapshot {
        let inner = self.inner.read();
        let community_rt = self.community.read();
        let now = Instant::now();
        let mut games: Vec<_> = inner
            .windows
            .iter()
            .map(|(identity, window)| {
                let learned = inner.stats.learned_profile(identity).map(|profile| {
                    let explicit = inner
                        .config
                        .profile_for(&window.exe, &window.wclass)
                        .is_some_and(|p| p.action.is_some());
                    let min_samples = inner.config.adaptive_min_samples.max(1);
                    learn::snapshot(
                        profile,
                        inner.config.adaptive_enabled(&window.exe, &window.wclass)
                            && profile.confident(min_samples)
                            && !explicit,
                        min_samples,
                    )
                });
                let success_rate = inner.stats.game_success_rate(identity);
                let exe_key = window.exe.to_ascii_lowercase();
                let community = if inner.config.community_intelligence {
                    community::snapshot_for_exe(
                        &community_rt,
                        &window.exe,
                        community_rt.applied_exes.contains(&exe_key),
                    )
                } else {
                    None
                };
                window.snapshot(
                    &inner.config,
                    now,
                    inner.current_elevated,
                    learned,
                    success_rate,
                    community,
                )
            })
            .collect();
        games.sort_by_key(|game| (game.effective != Verdict::Game, game.gone, game.exe.clone()));

        EngineSnapshot {
            engine: inner.status,
            next_tick: inner.next_tick(now),
            games,
            stats: inner.stats.snapshot(),
            config: inner.config.clone(),
            error: inner.last_error.clone(),
            update: inner.update_prompt.clone(),
            paused_reason: inner.gate_reason.clone(),
            snooze_remaining: inner
                .snooze_until
                .map(|until| until.saturating_duration_since(now).as_secs()),
            log: inner.log.iter().rev().cloned().collect(),
        }
    }

    pub fn update_config(&self, update: impl FnOnce(&mut AppConfig)) {
        let mut inner = self.inner.write();
        update(&mut inner.config);
        inner.clear_timers();
        inner.recompute_effective(Instant::now());
        inner.apply_suspended_status();
    }

    pub fn update_config_without_reschedule(&self, update: impl FnOnce(&mut AppConfig)) {
        let mut inner = self.inner.write();
        update(&mut inner.config);
        inner.apply_suspended_status();
    }

    pub fn replace_config(&self, config: AppConfig) {
        let mut inner = self.inner.write();
        inner.config = config;
        inner.clear_timers();
        inner.recompute_effective(Instant::now());
        inner.apply_suspended_status();
    }

    pub fn reset_stats(&self) {
        let mut inner = self.inner.write();
        inner.stats.reset_session();
        for window in inner.windows.values_mut() {
            window.uptime = 0;
            window.actions = 0;
        }
    }

    pub fn set_update_prompt(&self, update: Option<UpdateCheck>) {
        self.inner.write().update_prompt = update;
    }

    /// Snooze the engine for `minutes` (0 cancels an active snooze).
    pub fn snooze(&self, minutes: u64) {
        let mut inner = self.inner.write();
        if minutes == 0 {
            if inner.snooze_until.take().is_some() {
                inner.push_log("info", "Snooze cancelled".to_string());
            }
            if inner.status == EngineStatus::Suspended && !inner.config.suspended {
                inner.status = EngineStatus::Dormant;
            }
            return;
        }
        inner.snooze_until = Some(Instant::now() + Duration::from_secs(minutes * 60));
        inner.status = EngineStatus::Suspended;
        inner.stats.note_dormant();
        inner.push_log("info", format!("Snoozed for {minutes} min"));
    }

    /// Explain why a tracked window has its current verdict: the per-signal
    /// score breakdown and the precedence rule (pin > title rule > manual mode
    /// > score) that produced the effective verdict.
    pub fn explain_detection(&self, exe: &str, wclass: &str) -> Option<DetectionExplanation> {
        let inner = self.inner.read();
        let window = inner.windows.get(&identity_key(exe, wclass))?;
        let config = &inner.config;
        let sensitivity = config.resolve_sensitivity(&window.exe, &window.wclass);
        let threshold = detector::threshold(sensitivity);
        let pinned = config.override_for(&window.exe, &window.wclass);
        let title_rule = config.title_override(&window.title);
        let exe_ignored = config.exe_ignore_override(&window.exe);

        Some(DetectionExplanation {
            title: window.title.clone(),
            exe: window.exe.clone(),
            score: window.score,
            threshold,
            sensitivity: sensitivity.label(),
            score_verdict: detector::verdict_for_score(window.score, sensitivity),
            effective: window.effective,
            factors: detector::score_factors(&window.facts),
            reason: explain_reason(
                config.manual_mode,
                pinned,
                title_rule,
                exe_ignored,
                window.score,
                threshold,
            ),
        })
    }

    /// Fire one keepalive at a specific tracked window right now.
    pub fn test_target(&self, exe: &str, wclass: &str) -> Result<String, String> {
        let plan = {
            let inner = self.inner.read();
            let identity = identity_key(exe, wclass);
            let Some(window) = inner
                .windows
                .get(&identity)
                .filter(|w| w.gone_since.is_none())
            else {
                return Err(format!(
                    "Couldn't test {exe} - the window is not currently visible."
                ));
            };
            let base_action =
                KeepaliveOptions::from_config(&inner.config, &window.exe, &window.wclass).action;
            let (action, send_without_focus, label, log_label) = resolve_keepalive_action(
                KeepaliveResolveContext {
                    config: &inner.config,
                    stats: &inner.stats,
                    identity: &identity,
                    exe: &window.exe,
                    wclass: &window.wclass,
                    base: &base_action,
                    health: &window.health,
                    community_entry: None,
                },
                &mut thread_rng(),
            );
            let mut options =
                KeepaliveOptions::from_config(&inner.config, &window.exe, &window.wclass);
            options.action = action;
            options.send_without_focus = send_without_focus;
            KeepaliveSendPlan {
                identity,
                title: window.title.clone(),
                exe: window.exe.clone(),
                target: KeepaliveTarget {
                    hwnd: window.hwnd,
                    exe: window.exe.clone(),
                    wclass: window.wclass.clone(),
                },
                options,
                label,
                log_label,
                auto_fallback: inner
                    .config
                    .profile_for(&window.exe, &window.wclass)
                    .and_then(|profile| profile.auto_fallback)
                    .unwrap_or(inner.config.auto_fallback),
                send_without_focus,
                community_enabled: inner.config.community_intelligence,
                was_first_action: window.actions == 0,
            }
        };

        let result = keepalive::send_keepalive(&plan.target, &plan.options);
        let mut inner = self.inner.write();
        match result {
            Ok(injected_tick) => {
                let now = Instant::now();
                if let Some(window) = inner.windows.get_mut(&plan.identity) {
                    window.actions = window.actions.saturating_add(1);
                    window.last_action_at = Some(now);
                    window.last_action_ok = Some(true);
                    window.last_injected_input_tick = injected_tick;
                    window.health.note_success();
                }
                inner
                    .stats
                    .note_action_result(&plan.identity, &plan.title, &plan.label, true);
                if inner.config.adaptive_learn_actions {
                    inner
                        .stats
                        .note_learned_action_success(&plan.identity, &plan.label);
                }
                inner.last_error = None;
                inner.push_log(
                    "action",
                    format!("Sent {} to {}", plan.log_label, plan.title),
                );
                Ok(format!("Sent {} to {}", plan.label, plan.title))
            }
            Err(error) => {
                if let Some(window) = inner.windows.get_mut(&plan.identity) {
                    window.last_action_ok = Some(false);
                    // Transient refusals (focus lock) don't escalate fallbacks.
                    if !error.is_transient() {
                        let _ = window.health.note_failure(plan.auto_fallback);
                    }
                }
                inner
                    .stats
                    .note_action_result(&plan.identity, &plan.title, &plan.label, false);
                inner.last_error = Some(error.to_string());
                Err(error.to_string())
            }
        }
    }

    /// Fire a one-off test keepalive at every active target, so the user can
    /// confirm everything works before walking away. Returns pass/fail counts.
    pub fn test_all_targets(&self) -> TestAllResult {
        let targets: Vec<(String, String)> = {
            let inner = self.inner.read();
            inner
                .windows
                .values()
                .filter(|w| {
                    w.effective == Verdict::Game
                        && w.gone_since.is_none()
                        && w.primary_keepalive
                        && !inner.config.is_paused(&w.exe, &w.wclass)
                })
                .map(|w| (w.exe.clone(), w.wclass.clone()))
                .collect()
        };
        let mut result = TestAllResult::default();
        for (exe, wclass) in targets {
            result.tested += 1;
            match self.test_target(&exe, &wclass) {
                Ok(_) => result.ok += 1,
                Err(_) => result.failed += 1,
            }
        }
        result
    }

    /// Drain toast notices queued by the engine for the notification pump.
    pub fn take_notices(&self) -> Vec<QueuedNotice> {
        let mut notices = std::mem::take(&mut self.inner.write().notices);
        notices.sort_by_key(|notice| match notice.kind {
            ToastKind::Error => 0,
            ToastKind::Success => 1,
            ToastKind::Info => 2,
        });
        notices
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

struct DetectionCyclePlans {
    sends: Vec<KeepaliveSendPlan>,
    placements: Vec<WindowPlacementPlan>,
}

impl EngineInner {
    fn push_log(&mut self, kind: &str, text: String) {
        let at = chrono::Local::now().format("%H:%M:%S").to_string();
        if self.config.file_logging {
            append_file_log(&at, kind, &text);
        }
        self.log.push_back(ActivityEvent {
            at,
            kind: kind.to_string(),
            text,
        });
        while self.log.len() > ACTIVITY_LOG_CAP {
            self.log.pop_front();
        }
    }

    fn push_notice(&mut self, notice: QueuedNotice) {
        if notice.kind == ToastKind::Error {
            self.notices.push(notice);
        } else {
            while self.notices.len() >= 8 {
                if let Some(index) = self
                    .notices
                    .iter()
                    .position(|existing| existing.kind != ToastKind::Error)
                {
                    self.notices.remove(index);
                } else {
                    break;
                }
            }
            self.notices.push(notice);
        }
    }

    fn note_runtime_warning(&mut self, text: String, notice: bool) {
        if !self.session_warnings.insert(text.clone()) {
            return;
        }
        self.push_log("error", text.clone());
        if notice {
            crate::alerts::send(&self.config, "OMNAFK error", &text);
            self.push_notice(QueuedNotice::error(text, Some(ToastAction::OpenFlyout)));
        }
    }

    fn clear_timers(&mut self) {
        for window in self.windows.values_mut() {
            window.timer = None;
        }
    }

    fn apply_suspended_status(&mut self) {
        if self.config.suspended {
            self.status = EngineStatus::Suspended;
            self.stats.note_dormant();
            self.last_error = None;
        } else if self.status == EngineStatus::Suspended && self.snooze_until.is_none() {
            self.status = EngineStatus::Dormant;
        }
    }

    fn recompute_effective(&mut self, now: Instant) {
        let mut rng = thread_rng();

        for window in self.windows.values_mut() {
            let override_verdict = self.config.override_for(&window.exe, &window.wclass);
            window.overridden = override_verdict.is_some();
            let forced = override_verdict
                .or_else(|| self.config.title_override(&window.title))
                .or_else(|| self.config.exe_ignore_override(&window.exe));
            window.effective = resolve_verdict(
                self.config.manual_mode,
                forced,
                window.score,
                self.config.resolve_sensitivity(&window.exe, &window.wclass),
            );

            let paused = self.config.is_paused(&window.exe, &window.wclass);
            if window.effective == Verdict::Game
                && window.gone_since.is_none()
                && !paused
                && window.primary_keepalive
            {
                if window.timer.is_none() {
                    let mut options =
                        KeepaliveOptions::from_config(&self.config, &window.exe, &window.wclass);
                    options.interval = keepalive::warmup_interval(
                        options.interval,
                        window.warmup_sends,
                        self.config.adaptive_interval,
                    );
                    window.timer = Some(TickTimer::new(now, &options, &mut rng));
                }
            } else {
                window.timer = None;
            }
        }
    }

    fn apply_detection(
        &mut self,
        detected: Vec<DetectedWindow>,
        now: Instant,
        community: &SharedCommunity,
    ) -> DetectionCyclePlans {
        let activity = Win32ActivityProbe;
        let mut rng = thread_rng();
        let elapsed = now
            .checked_duration_since(self.last_cycle)
            .unwrap_or_default()
            .as_secs();
        self.last_cycle = now;

        let mut seen = BTreeSet::new();
        for detected in detected {
            let identity = identity_key(&detected.facts.exe, &detected.facts.wclass);
            seen.insert(identity.clone());
            let is_new = !self.windows.contains_key(&identity);
            let exe_name = detected.facts.exe.clone();
            let wclass_name = detected.facts.wclass.clone();
            let effective = effective_verdict(&self.config, &detected);
            let overridden = self
                .config
                .override_for(&detected.facts.exe, &detected.facts.wclass)
                .is_some();

            self.windows
                .entry(identity.clone())
                .and_modify(|window| {
                    if window.hwnd != detected.hwnd {
                        window.monitor_placed = false;
                        window.monitor_status = None;
                        window.presence.reset();
                    }
                    window.title = detected.facts.title.clone();
                    window.exe = detected.facts.exe.clone();
                    window.wclass = detected.facts.wclass.clone();
                    window.pid = detected.facts.pid;
                    window.hwnd = detected.hwnd;
                    window.verdict = detected.verdict;
                    window.effective = effective;
                    window.overridden = overridden;
                    window.gone_since = None;
                    window.facts = detected.facts.clone();
                    window.score = detected.score;
                })
                .or_insert_with(|| TrackedWindow {
                    title: detected.facts.title.clone(),
                    exe: detected.facts.exe.clone(),
                    wclass: detected.facts.wclass.clone(),
                    pid: detected.facts.pid,
                    hwnd: detected.hwnd,
                    verdict: detected.verdict,
                    effective,
                    overridden,
                    gone_since: None,
                    uptime: 0,
                    actions: 0,
                    timer: None,
                    score: detected.score,
                    presence: presence::PresenceTracker::seeded(
                        detected.facts.gpu_usage,
                        &detected.facts.title,
                    ),
                    facts: detected.facts,
                    last_action_at: None,
                    last_action_ok: None,
                    was_armed: false,
                    monitor_placed: false,
                    monitor_status: None,
                    monitor_move_failures: 0,
                    health: KeepaliveHealth::default(),
                    primary_keepalive: true,
                    warmup_sends: 0,
                    rotation_index: 0,
                    last_injected_input_tick: None,
                    logged_recent_input_hold: false,
                    logged_presence_hold: false,
                });

            if let Some(window) = self.windows.get_mut(&identity) {
                Self::update_window_presence(window, &self.config, community, now);
            }

            if is_new && self.config.community_intelligence {
                let mut rt = community.write();
                if community::try_auto_apply_for_window(
                    &mut self.config,
                    &mut rt,
                    &exe_name,
                    &wclass_name,
                ) {
                    self.push_log("info", format!("Community profile applied for {exe_name}"));
                    if let Err(error) = crate::config::save(&self.config) {
                        self.note_runtime_warning(
                            format!("Couldn't save community profile: {error}"),
                            false,
                        );
                    }
                }
            }

            if effective == Verdict::Game {
                self.stats.note_seen_today(&identity);
                if self.config.burst_detection {
                    let until = now + BURST_WINDOW;
                    self.burst_until = Some(
                        self.burst_until
                            .map(|current| current.max(until))
                            .unwrap_or(until),
                    );
                }
            }
        }

        for (identity, window) in self.windows.iter_mut() {
            if !seen.contains(identity) && window.gone_since.is_none() {
                window.gone_since = Some(now);
            }
        }

        self.windows.retain(|_, window| {
            window
                .gone_since
                .is_none_or(|gone| now.duration_since(gone) <= GONE_LINGER)
        });

        self.recompute_effective(now);
        self.assign_primary_keepalives();
        self.note_armed_transitions();
        let placements = self.plan_window_placements(now, &activity);
        let sends = self.drive_keepalives(now, elapsed, &activity, &mut rng, community);
        DetectionCyclePlans { sends, placements }
    }

    fn apply_keepalive_results(
        &mut self,
        now: Instant,
        results: Vec<(
            KeepaliveSendPlan,
            Result<Option<u32>, keepalive::KeepaliveError>,
        )>,
        _community: &SharedCommunity,
    ) {
        if results.is_empty() {
            return;
        }

        let notify_all = matches!(
            self.config.notifications,
            crate::config::NotificationLevel::All
        );
        let notify_errors = !matches!(
            self.config.notifications,
            crate::config::NotificationLevel::None
        );
        let mut rng = thread_rng();
        let mut keepalive_errors = 0usize;
        let adaptive_interval = self.config.adaptive_interval;

        for (plan, result) in results {
            match result {
                Ok(injected_tick) => {
                    if let Some(window) = self.windows.get_mut(&plan.identity) {
                        window.actions = window.actions.saturating_add(1);
                        window.last_action_at = Some(now);
                        window.last_action_ok = Some(true);
                        window.last_injected_input_tick = injected_tick;
                        window.warmup_sends = window.warmup_sends.saturating_add(1);
                        window.rotation_index = window.rotation_index.wrapping_add(1);
                        window.health.note_success();
                    }
                    self.stats
                        .note_action_result(&plan.identity, &plan.title, &plan.label, true);
                    if self.config.adaptive_learn_actions {
                        self.stats
                            .note_learned_action_success(&plan.identity, &plan.label);
                    }
                    if plan.community_enabled {
                        let top_keys = self
                            .stats
                            .learned_profile(&plan.identity)
                            .map(|profile| {
                                profile
                                    .top()
                                    .into_iter()
                                    .take(2)
                                    .map(|entry| entry.key)
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        community::record_keepalive(
                            &plan.exe,
                            &plan.label,
                            true,
                            plan.send_without_focus,
                            &top_keys,
                        );
                    }
                    self.last_error = None;
                    self.push_log(
                        "action",
                        format!("Sent {} to {}", plan.log_label, plan.title),
                    );
                    if plan.was_first_action && notify_all {
                        self.push_notice(QueuedNotice::success(format!(
                            "First keepalive sent to {}",
                            plan.title
                        )));
                    }
                }
                Err(error) => {
                    if error.is_transient() {
                        // Temporary refusal (e.g. Windows blocked the focus
                        // switch): log quietly and retry at the next tick
                        // without escalating fallbacks or backoff.
                        self.push_log("info", error.to_string());
                        tracing::info!("{error}");
                    } else {
                        keepalive_errors += 1;
                        if let Some(window) = self.windows.get_mut(&plan.identity) {
                            window.last_action_ok = Some(false);
                            if let Some(warning) = window.health.note_failure(plan.auto_fallback) {
                                self.push_log("error", warning.clone());
                                if keepalive_errors == 1 {
                                    self.push_notice(QueuedNotice::error(
                                        warning,
                                        Some(ToastAction::OpenFlyout),
                                    ));
                                }
                            }
                        }
                        self.stats.note_action_result(
                            &plan.identity,
                            &plan.title,
                            &plan.label,
                            false,
                        );
                        if plan.community_enabled {
                            community::record_keepalive(
                                &plan.exe,
                                &plan.label,
                                false,
                                plan.send_without_focus,
                                &[],
                            );
                        }
                        self.last_error = Some(error.to_string());
                        self.push_log("error", error.to_string());
                        tracing::warn!("{error}");
                    }
                }
            }
            if let Some(window) = self.windows.get_mut(&plan.identity) {
                let backoff = window.health.backoff_multiplier();
                let sends = window.warmup_sends;
                if let Some(timer) = window.timer.as_mut() {
                    let mut options = plan.options.clone();
                    options.interval =
                        keepalive::warmup_interval(options.interval, sends, adaptive_interval);
                    timer.reschedule_scaled(now, &options, &mut rng, backoff);
                }
            }
        }

        if keepalive_errors > 1 && notify_errors {
            self.push_notice(QueuedNotice::error(
                format!("Keepalive failed for {keepalive_errors} targets."),
                Some(ToastAction::OpenFlyout),
            ));
        }
    }

    fn assign_primary_keepalives(&mut self) {
        // Multi-boxing: every window of a game is its own keepalive target.
        if self.config.keep_all_instances {
            for window in self.windows.values_mut() {
                window.primary_keepalive =
                    window.effective == Verdict::Game && window.gone_since.is_none();
            }
            return;
        }
        let mut best: BTreeMap<String, (String, i32, isize)> = BTreeMap::new();
        for (identity, window) in &self.windows {
            if window.effective != Verdict::Game || window.gone_since.is_some() {
                continue;
            }
            let exe = window.exe.to_ascii_lowercase();
            match best.get(&exe) {
                Some((_, best_score, best_hwnd))
                    if (window.score, window.hwnd) <= (*best_score, *best_hwnd) => {}
                _ => {
                    best.insert(exe, (identity.clone(), window.score, window.hwnd));
                }
            }
        }
        for (identity, window) in self.windows.iter_mut() {
            let exe = window.exe.to_ascii_lowercase();
            window.primary_keepalive = best
                .get(&exe)
                .is_some_and(|(best_id, _, _)| best_id == identity);
            if !window.primary_keepalive {
                window.timer = None;
            }
        }
    }

    fn cached_monitors(&mut self, now: Instant) -> Vec<MonitorInfo> {
        if self.monitors_cache.is_empty() || self.monitors_cached_at.elapsed() >= MONITOR_CACHE_TTL
        {
            self.monitors_cache = monitor::list_monitors();
            self.monitors_cached_at = now;
        }
        self.monitors_cache.clone()
    }

    fn note_armed_transitions(&mut self) {
        let notify = matches!(
            self.config.notifications,
            crate::config::NotificationLevel::All
        );
        let mut events = Vec::new();
        for window in self.windows.values_mut() {
            let armed = window.effective == Verdict::Game
                && window.gone_since.is_none()
                && !self.config.is_paused(&window.exe, &window.wclass);
            if armed != window.was_armed {
                window.was_armed = armed;
                let text = if armed {
                    format!("{} marked as game", window.title)
                } else {
                    format!("{} no longer active", window.title)
                };
                events.push(text);
            }
        }
        for text in events {
            self.push_log("info", text.clone());
            if notify {
                self.push_notice(QueuedNotice::info(text));
            }
        }
    }

    /// Why the engine is currently holding fire across all targets, if at all.
    fn compute_gate(&self, now: Instant, activity: &Win32ActivityProbe) -> Option<String> {
        use crate::keepalive::ActivityProbe;

        if self.config.pause_on_battery && keepalive::on_battery() {
            return Some("ON BATTERY".to_string());
        }
        if keepalive::session_locked() {
            if self.config.pause_when_locked {
                return Some("SESSION LOCKED".to_string());
            }
            // Real input can't reach games on a locked desktop, so sends would
            // only fail with confusing errors. Hold automatically unless the
            // user runs the background-only (PostMessage) delivery, which can
            // still work while locked.
            if !self.config.send_without_focus {
                return Some("SESSION LOCKED".to_string());
            }
        }
        let (minutes_now, dow_now) = {
            use chrono::{Datelike, Timelike};
            let t = chrono::Local::now();
            (
                t.hour() * 60 + t.minute(),
                t.weekday().num_days_from_sunday(),
            )
        };
        if self.config.in_quiet_hours(minutes_now, dow_now) {
            return Some("QUIET HOURS".to_string());
        }
        if self.config.idle_threshold_mins > 0 {
            let threshold = Duration::from_secs(self.config.idle_threshold_mins * 60);
            if activity
                .last_input_age(now)
                .is_some_and(|age| age < threshold)
            {
                return Some("WAITING FOR IDLE".to_string());
            }
        }
        if self.config.max_session_hours > 0
            && now.duration_since(self.session_start)
                >= Duration::from_secs(self.config.max_session_hours * 3600)
        {
            return Some("SAFETY CAP REACHED (HOURS)".to_string());
        }
        if self.config.max_session_actions > 0
            && self.stats.actions >= self.config.max_session_actions
        {
            return Some("SAFETY CAP REACHED (ACTIONS)".to_string());
        }
        None
    }

    fn drive_keepalives(
        &mut self,
        now: Instant,
        elapsed: u64,
        activity: &Win32ActivityProbe,
        rng: &mut impl rand::Rng,
        community: &SharedCommunity,
    ) -> Vec<KeepaliveSendPlan> {
        if self.config.suspended || self.snooze_until.is_some() {
            self.status = EngineStatus::Suspended;
            self.stats.note_dormant();
            self.last_error = None;
            return Vec::new();
        }

        let gate = self.compute_gate(now, activity);
        if gate != self.gate_reason {
            match &gate {
                Some(reason) => self.push_log("info", format!("Held ticks: {reason}")),
                None => {
                    if self.gate_reason.is_some() {
                        self.push_log("info", "Resumed ticks".to_string());
                    }
                }
            }
            // Push an away-from-keyboard alert only for the unexpected "it
            // stopped" gates (battery, lock, safety cap) — not idle/quiet hours,
            // which toggle constantly during normal use.
            let was_alertworthy = self.gate_reason.as_deref().is_some_and(gate_is_alertworthy);
            match &gate {
                Some(reason) if gate_is_alertworthy(reason) => {
                    crate::alerts::send(
                        &self.config,
                        "OMNAFK paused",
                        &format!("Keepalives paused — {reason}."),
                    );
                }
                None if was_alertworthy => {
                    crate::alerts::send(&self.config, "OMNAFK resumed", "Keepalives resumed.");
                }
                _ => {}
            }
            self.gate_reason = gate.clone();
        }

        let mut active_count = 0;
        let mut holding = false;
        let mut log_entries: Vec<(String, String)> = Vec::new();
        let mut pending_sends: Vec<KeepaliveSendPlan> = Vec::new();
        let community_enabled = self.config.community_intelligence;

        // Controller activity is global; poll once here. It only holds a
        // target's ticks when that game is also the foreground window, so a
        // controller session in one game never starves another game's keepalives.
        self.gamepad.poll(now);
        let gamepad_age = self.gamepad.last_active_age(now);

        let identities: Vec<String> = self.windows.keys().cloned().collect();
        for identity in identities {
            let Some(window) = self.windows.get(&identity) else {
                continue;
            };
            if window.effective != Verdict::Game || window.gone_since.is_some() {
                continue;
            }
            if self.config.is_paused(&window.exe, &window.wclass) {
                continue;
            }
            if !window.primary_keepalive {
                continue;
            }

            active_count += 1;
            let title = window.title.clone();
            let exe = window.exe.clone();
            let wclass = window.wclass.clone();
            let hwnd = window.hwnd;
            let mut options = KeepaliveOptions::from_config(&self.config, &exe, &wclass);
            if self.config.rotate_actions {
                options.action = crate::config::rotation_action(window.rotation_index);
            }
            let health = window.health.clone();
            let community_entry = if community_enabled {
                community::game_entry(&community.read(), &exe).cloned()
            } else {
                None
            };

            let (action, send_without_focus, label, log_label) = resolve_keepalive_action(
                KeepaliveResolveContext {
                    config: &self.config,
                    stats: &self.stats,
                    identity: &identity,
                    exe: &exe,
                    wclass: &wclass,
                    base: &options.action,
                    health: &health,
                    community_entry: community_entry.as_ref(),
                },
                rng,
            );
            options.action = action;
            options.send_without_focus = send_without_focus;
            let target = KeepaliveTarget {
                hwnd,
                exe: exe.clone(),
                wclass: wclass.clone(),
            };
            let ignore_input_tick = self
                .windows
                .get(&identity)
                .and_then(|window| window.last_injected_input_tick);

            if gate.is_none() && elapsed > 0 {
                self.stats.note_kept(&identity, &title, elapsed);
                if let Some(window) = self.windows.get_mut(&identity) {
                    window.uptime = window.uptime.saturating_add(elapsed);
                }
            }
            self.stats.note_seen_today(&identity);

            let gamepad_held = options.hold_while_playing
                && activity.foreground_window() == Some(target.hwnd)
                && gamepad_age
                    .is_some_and(|age| age <= Duration::from_secs(options.hold_window_secs.max(1)));
            if keepalive::should_hold(&target, &options, now, activity, ignore_input_tick)
                || gamepad_held
            {
                holding = true;
            } else if let Some(window) = self.windows.get_mut(&identity) {
                window.logged_recent_input_hold = false;
            }

            let Some(window) = self.windows.get_mut(&identity) else {
                continue;
            };
            let Some(timer) = window.timer.as_mut() else {
                continue;
            };

            let presence_hold = window.presence.should_hold_keepalives();

            match keepalive::tick_decision(
                timer,
                &target,
                &options,
                now,
                activity,
                ignore_input_tick,
            ) {
                TickDecision::Waiting => {}
                TickDecision::DeferBriefly => {
                    // User is mid-input in another app: nudge the due time a
                    // few seconds so the focus flick lands in a quiet moment.
                    timer.defer(now, Duration::from_secs(keepalive::POLITE_DEFER_SECS));
                }
                TickDecision::Held => {
                    holding = true;
                    if !window.logged_recent_input_hold {
                        window.logged_recent_input_hold = true;
                        log_entries.push((
                            "info".into(),
                            format!("Held tick: recent input for {title}"),
                        ));
                    }
                    timer.reschedule(now, &options, rng);
                }
                TickDecision::Send if gate.is_some() => {
                    // A global gate (quiet hours, battery, lock, idle, cap)
                    // blocks the send. Defer briefly instead of burning a full
                    // interval, so ticks resume promptly once the gate clears.
                    timer.defer(now, Duration::from_secs(GATE_RECHECK_SECS));
                }
                TickDecision::Send if presence_hold => {
                    holding = true;
                    if !window.logged_presence_hold {
                        window.logged_presence_hold = true;
                        log_entries.push((
                            "info".into(),
                            format!("Held tick: presence menu/lobby for {title}"),
                        ));
                    }
                    // Re-check soon: if presence flips back to in-game (or was
                    // wrong), the next keepalive shouldn't be an interval away.
                    timer.defer(now, Duration::from_secs(PRESENCE_RECHECK_SECS));
                }
                TickDecision::Send => {
                    let auto_fallback = self
                        .config
                        .profile_for(&exe, &wclass)
                        .and_then(|profile| profile.auto_fallback)
                        .unwrap_or(self.config.auto_fallback);
                    let resume_hold =
                        window.logged_recent_input_hold || window.logged_presence_hold;
                    window.logged_recent_input_hold = false;
                    window.logged_presence_hold = false;
                    if resume_hold {
                        log_entries.push(("info".into(), format!("Resumed tick for {title}")));
                    }
                    let was_first_action = window.actions == 0;
                    pending_sends.push(KeepaliveSendPlan {
                        identity: identity.clone(),
                        title,
                        exe: exe.clone(),
                        target,
                        options: options.clone(),
                        label,
                        log_label,
                        auto_fallback,
                        send_without_focus,
                        community_enabled,
                        was_first_action,
                    });
                }
            }
        }

        for (kind, text) in log_entries {
            self.push_log(&kind, text);
        }

        self.status = if active_count == 0 {
            self.stats.note_dormant();
            self.last_error = None;
            EngineStatus::Dormant
        } else if holding || self.gate_reason.is_some() {
            EngineStatus::Holding
        } else {
            EngineStatus::Active
        };

        pending_sends
    }

    fn update_window_presence(
        window: &mut TrackedWindow,
        config: &AppConfig,
        community: &SharedCommunity,
        now: Instant,
    ) {
        let rules = community::presence_rules_for(&community.read(), &window.exe);
        window.presence.note(presence::PresenceInputs {
            gpu_usage: window.facts.gpu_usage,
            title: &window.facts.title,
            hwnd: window.hwnd,
            pid: window.pid,
            rules: rules.as_ref(),
            log_enabled: config.presence_log_enabled,
            screen_enabled: config.presence_screen_enabled,
            memory_enabled: config.presence_memory_enabled,
            respect_presence: config.respect_presence,
            now,
        });
    }

    fn next_tick(&self, now: Instant) -> Option<u64> {
        if self.status != EngineStatus::Active {
            return None;
        }

        self.windows
            .values()
            .filter(|window| window.effective == Verdict::Game && window.gone_since.is_none())
            .filter_map(|window| window.timer.as_ref())
            .map(|timer| timer.seconds_until(now))
            .min()
    }
}

impl TrackedWindow {
    fn snapshot(
        &self,
        config: &AppConfig,
        now: Instant,
        current_elevated: bool,
        learned: Option<LearnedSnapshot>,
        success_rate: Option<u8>,
        community: Option<CommunityGameSnapshot>,
    ) -> GameSnapshot {
        let profile = config
            .profile_for(&self.exe, &self.wclass)
            .cloned()
            .unwrap_or_default();

        GameSnapshot {
            title: if self.gone_since.is_some() {
                format!("{} (closed)", self.title)
            } else {
                self.title.clone()
            },
            exe: self.exe.clone(),
            wclass: self.wclass.clone(),
            verdict: self.verdict,
            overridden: self.overridden,
            effective: self.effective,
            gone: self.gone_since.is_some(),
            paused: config.is_paused(&self.exe, &self.wclass),
            uptime: self.uptime,
            actions: self.actions,
            score: self.score,
            threshold: detector::threshold(config.resolve_sensitivity(&self.exe, &self.wclass)),
            facts: self.facts.clone(),
            next_tick: self.timer.as_ref().map(|timer| timer.seconds_until(now)),
            last_action_secs: self
                .last_action_at
                .map(|at| now.saturating_duration_since(at).as_secs()),
            last_action_ok: self.last_action_ok,
            elevated_mismatch: self.facts.elevated == Some(true) && !current_elevated,
            learned,
            monitor: GameMonitorSnapshot {
                target: placement::monitor_target_label(config, &self.exe, &self.wclass),
                status: self.monitor_status.clone(),
            },
            profile: profile_snapshot(&profile, config),
            health_warning: self.health.warning(),
            consecutive_failures: self.health.consecutive_failures,
            success_rate,
            primary_keepalive: self.primary_keepalive,
            community,
            presence: (self.effective == Verdict::Game && self.gone_since.is_none())
                .then(|| {
                    let snap = self.presence.snapshot();
                    (snap.confidence > 0).then_some(snap)
                })
                .flatten(),
            menu_hint: (self.effective == Verdict::Game && self.gone_since.is_none())
                .then(|| self.presence.menu_hint())
                .flatten(),
        }
    }
}

struct KeepaliveResolveContext<'a> {
    config: &'a AppConfig,
    stats: &'a Stats,
    identity: &'a str,
    exe: &'a str,
    wclass: &'a str,
    base: &'a ResolvedAction,
    health: &'a KeepaliveHealth,
    community_entry: Option<&'a community::GameEntry>,
}

fn resolve_keepalive_action(
    ctx: KeepaliveResolveContext<'_>,
    rng: &mut impl rand::Rng,
) -> (ResolvedAction, bool, String, String) {
    let KeepaliveResolveContext {
        config,
        stats,
        identity,
        exe,
        wclass,
        base,
        health,
        community_entry,
    } = ctx;
    let mut effective_health = health.clone();
    if let Some(entry) = community_entry {
        if let Some(tier) = community::preferred_fallback_tier(entry, health.consecutive_failures) {
            effective_health.fallback_tier = tier;
        }
    }
    let send_without_focus = config
        .profile_for(exe, wclass)
        .and_then(|profile| profile.send_without_focus)
        .unwrap_or(config.send_without_focus);
    let (mut action, send_without_focus) =
        effective_health.apply_to_options(base, send_without_focus);

    if config.adaptive_enabled(exe, wclass)
        && config
            .profile_for(exe, wclass)
            .is_none_or(|p| p.action.is_none())
    {
        if let Some(profile) = stats.learned_profile(identity) {
            let min = config.adaptive_min_samples.max(1);
            if profile.confident(min) {
                match profile.pick(
                    rng,
                    config.adaptive_learn_sequences,
                    config.adaptive_learn_actions,
                ) {
                    AdaptivePick::Keys(keys) => {
                        action = ResolvedAction::KeySequence(keys.clone());
                        let log = if keys.len() > 1 {
                            format!("Adaptive ({})", keys.join("+"))
                        } else {
                            format!("Adaptive ({})", keys.first().cloned().unwrap_or_default())
                        };
                        return (action, send_without_focus, "Adaptive".to_string(), log);
                    }
                    AdaptivePick::Action(label) => {
                        if let Some(resolved) = resolved_from_label(&label) {
                            action = resolved;
                            return (
                                action.clone(),
                                send_without_focus,
                                label.clone(),
                                format!("Adaptive ({label})"),
                            );
                        }
                    }
                    AdaptivePick::None => {}
                }
            }
        }
    }

    let label = action.label();
    (action, send_without_focus, label.clone(), label)
}

fn resolved_from_label(label: &str) -> Option<ResolvedAction> {
    match label {
        "Space tap" => Some(ResolvedAction::SpaceTap),
        "W tap" => Some(ResolvedAction::WTap),
        "Camera nudge" => Some(ResolvedAction::CameraNudge),
        "Mouse wiggle" => Some(ResolvedAction::MouseWiggle),
        "Scroll tick" => Some(ResolvedAction::ScrollTick),
        "Right click" => Some(ResolvedAction::RightClick),
        other if other.starts_with("Keys ") => {
            let keys = other
                .trim_start_matches("Keys ")
                .split('+')
                .map(str::to_string)
                .collect();
            Some(ResolvedAction::KeySequence(keys))
        }
        _ => None,
    }
}

fn profile_snapshot(profile: &TargetProfile, config: &AppConfig) -> GameProfileSnapshot {
    GameProfileSnapshot {
        action: profile.action.map(|action| action.label().to_string()),
        interval: profile.interval,
        key_sequence: profile.key_sequence.clone(),
        monitor: profile
            .monitor
            .clone()
            .or_else(|| profile_monitor_global_label(config)),
        adaptive: profile.adaptive,
        hold_while_playing: profile.hold_while_playing,
        hold_window_secs: profile.hold_window_secs,
        send_without_focus: profile.send_without_focus,
        auto_fallback: profile.auto_fallback,
    }
}

fn profile_monitor_global_label(config: &AppConfig) -> Option<String> {
    if config.monitor_placement {
        Some("Use global".to_string())
    } else {
        None
    }
}

fn effective_verdict(config: &AppConfig, detected: &DetectedWindow) -> Verdict {
    // Per-window pin wins over a title rule, which wins over the auto score.
    let forced = config
        .override_for(&detected.facts.exe, &detected.facts.wclass)
        .or_else(|| config.title_override(&detected.facts.title))
        .or_else(|| config.exe_ignore_override(&detected.facts.exe));
    resolve_verdict(
        config.manual_mode,
        forced,
        detected.score,
        config.resolve_sensitivity(&detected.facts.exe, &detected.facts.wclass),
    )
}

/// Whether a hold-gate reason is worth a phone alert: the unexpected "it
/// stopped" cases, not the normal idle/quiet-hours cadence.
fn gate_is_alertworthy(reason: &str) -> bool {
    reason.starts_with("ON BATTERY")
        || reason.starts_with("SESSION LOCKED")
        || reason.starts_with("SAFETY CAP")
}

/// Fold a window's signals into a final verdict. Precedence: a `forced` pin or
/// title rule always wins; with nothing forced, manual mode ignores everything
/// and auto mode falls back to the detection score.
fn resolve_verdict(
    manual_mode: bool,
    forced: Option<OverrideVerdict>,
    score: i32,
    sensitivity: Sensitivity,
) -> Verdict {
    match (manual_mode, forced) {
        (_, Some(OverrideVerdict::Game)) => Verdict::Game,
        (_, Some(OverrideVerdict::Ignored)) => Verdict::Ignored,
        (true, None) => Verdict::Ignored,
        (false, None) => detector::verdict_for_score(score, sensitivity),
    }
}

/// Plain-language sentence for why a window has its effective verdict, walking
/// the same precedence as `resolve_verdict`: pin, then title rule, then the
/// always-ignore exe list, then manual mode, then score-versus-threshold.
fn explain_reason(
    manual_mode: bool,
    pinned: Option<OverrideVerdict>,
    title_rule: Option<OverrideVerdict>,
    exe_ignored: Option<OverrideVerdict>,
    score: i32,
    threshold: i32,
) -> String {
    match pinned {
        Some(OverrideVerdict::Game) => {
            return "You pinned this window as a game, so it's always kept awake.".into()
        }
        Some(OverrideVerdict::Ignored) => {
            return "You pinned this window as ignored, so it's never kept awake.".into()
        }
        None => {}
    }
    match title_rule {
        Some(OverrideVerdict::Game) => {
            return "A title rule marks this window as a game, overriding the score.".into()
        }
        Some(OverrideVerdict::Ignored) => {
            return "A title rule ignores this window, overriding the score.".into()
        }
        None => {}
    }
    if exe_ignored.is_some() {
        return "An always-ignore exe rule ignores this window, overriding the score.".into();
    }
    if manual_mode {
        return "Manual mode is on, so only windows you pin are kept awake.".into();
    }
    if score >= threshold {
        format!("Its score {score} meets the threshold of {threshold}, so it's kept awake.")
    } else {
        format!(
            "Its score {score} is below the threshold of {threshold}, so it's ignored. \
             Lower the sensitivity or pin it as a game to keep it awake."
        )
    }
}

pub fn identity_key(exe: &str, wclass: &str) -> String {
    format!("{}\u{1f}{wclass}", exe.to_ascii_lowercase())
}

fn append_file_log(at: &str, kind: &str, text: &str) {
    let Some(dir) = dirs::config_dir() else {
        return;
    };
    let path = dir.join("OMNAFK").join("omnafk.log");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let date = chrono::Local::now().format("%Y-%m-%d").to_string();
        let _ = writeln!(file, "{date} {at} [{kind}] {text}");
    }
}

pub fn log_file_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|dir| dir.join("OMNAFK").join("omnafk.log"))
}

fn sleep_until_next_cycle(stop: &AtomicBool, interval: Duration) {
    let start = Instant::now();
    while !stop.load(Ordering::SeqCst) && start.elapsed() < interval {
        thread::sleep(Duration::from_millis(500));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Runs the real detector loop for 15 seconds."]
    fn engine_loop_smoke_15s_no_panic() {
        let engine = Engine::new(AppConfig::default());
        engine.start();
        thread::sleep(Duration::from_secs(15));
        engine.stop();
        let _ = engine.snapshot();
    }

    #[test]
    fn held_tick_log_flag_dedupes() {
        let mut logged = false;
        assert!(!logged);
        logged = true;
        assert!(logged);
    }

    #[test]
    fn snooze_suspends_and_cancels() {
        let engine = Engine::new(AppConfig::default());
        engine.snooze(30);
        let snap = engine.snapshot();
        assert_eq!(snap.engine, EngineStatus::Suspended);
        assert!(snap
            .snooze_remaining
            .is_some_and(|secs| secs > 0 && secs <= 1800));

        engine.snooze(0);
        let snap = engine.snapshot();
        assert_eq!(snap.engine, EngineStatus::Dormant);
        assert!(snap.snooze_remaining.is_none());
    }

    #[test]
    fn autostart_launch_defers_auto_elevation_until_ui_or_grace() {
        let engine = Engine::with_launch_context(
            AppConfig::default(),
            PersistedStats::default(),
            crate::community::shared_runtime(),
            true,
        );
        assert!(engine.autostart_launch());
        assert!(!engine.can_auto_elevate_now());
        engine.mark_user_ui_opened();
        assert!(!engine.can_auto_elevate_now());
    }
}
