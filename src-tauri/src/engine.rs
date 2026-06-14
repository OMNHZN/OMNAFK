use crate::{
    community::{self, CommunityGameSnapshot, SharedCommunity},
    config::{AppConfig, OverrideVerdict, ResolvedAction, ResolvedMonitor, TargetProfile},
    detector::{self, DetectedWindow, Verdict, WindowFacts},
    gpu::PdhGpuProbe,
    health::{KeepaliveHealth, FAILURE_THRESHOLD},
    keepalive::{
        self, KeepaliveOptions, KeepaliveTarget, TickDecision, TickTimer, Win32ActivityProbe,
    },
    learn::{self, AdaptivePick, LearnedSnapshot},
    monitor::{self, MonitorInfo, PlacementOptions, PlacementResult},
    stats::{PersistedStats, Stats, StatsSnapshot},
    updates::UpdateCheck,
};
use parking_lot::{Mutex, RwLock};
use rand::thread_rng;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::Write,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const DETECTION_INTERVAL: Duration = Duration::from_secs(5);
const BURST_DETECTION_INTERVAL: Duration = Duration::from_secs(1);
const BURST_WINDOW: Duration = Duration::from_secs(30);
const MONITOR_CACHE_TTL: Duration = Duration::from_secs(30);
const GONE_LINGER: Duration = Duration::from_secs(60);
const ACTIVITY_LOG_CAP: usize = 50;
const STATS_SAVE_INTERVAL: Duration = Duration::from_secs(30);

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

pub type SharedEngine = Arc<Engine>;

pub struct Engine {
    inner: RwLock<EngineInner>,
    stop: AtomicBool,
    pending_elevation: AtomicBool,
    worker: Mutex<Option<JoinHandle<()>>>,
    sampler: Mutex<Option<JoinHandle<()>>>,
    gpu: Mutex<PdhGpuProbe>,
    community: SharedCommunity,
}

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
    notices: Vec<String>,
    last_stats_save: Instant,
    current_elevated: bool,
    burst_until: Option<Instant>,
    monitors_cache: Vec<MonitorInfo>,
    monitors_cached_at: Instant,
    elevation_requested: bool,
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
    /// `GetLastInputInfo` tick from OMNAFK's last successful SendInput keepalive.
    last_injected_input_tick: Option<u32>,
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
                monitors_cached_at: Instant::now() - MONITOR_CACHE_TTL,
                elevation_requested: false,
            }),
            stop: AtomicBool::new(false),
            pending_elevation: AtomicBool::new(false),
            worker: Mutex::new(None),
            sampler: Mutex::new(None),
            gpu: Mutex::new(PdhGpuProbe::default()),
            community,
        })
    }

    pub fn community(&self) -> &SharedCommunity {
        &self.community
    }

    pub fn take_pending_elevation(&self) -> bool {
        self.pending_elevation.swap(false, Ordering::SeqCst)
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
        inner.push_notice(
            "Elevated game detected — approve the UAC prompt so OMNAFK can send input.".to_string(),
        );
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
                engine.run_detection_cycle();
                sleep_until_next_cycle(&engine.stop, engine.detection_interval());
            }
            engine.persist_stats(true);
        }));

        // Adaptive-learning sampler: observes which whitelisted keys the user
        // holds while actively playing a tracked game.
        let engine = Arc::clone(self);
        *self.sampler.lock() = Some(thread::spawn(move || {
            while !engine.stop.load(Ordering::SeqCst) {
                engine.run_learn_sample();
                thread::sleep(Duration::from_millis(learn::SAMPLE_INTERVAL_MS));
            }
        }));
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
        let mut inner = self.inner.write();
        let identity = identity_key(exe, wclass);
        let (hwnd, title, fullscreen, facts) = {
            let Some(window) = inner
                .windows
                .get(&identity)
                .filter(|w| w.gone_since.is_none())
            else {
                return Err(format!(
                    "Couldn't move {exe} - the window is not currently visible."
                ));
            };
            (
                window.hwnd,
                window.title.clone(),
                window.facts.fullscreen,
                window.facts.clone(),
            )
        };
        let monitors = inner.cached_monitors(Instant::now());
        let options = PlacementOptions {
            when: inner.config.monitor_when,
            style: inner.config.monitor_style,
            skip_active: false,
            skip_active_secs: inner.config.monitor_skip_active_secs,
        };
        let (target_device, target_label) = match inner.config.resolve_monitor(exe, wclass) {
            ResolvedMonitor::Off => {
                return Err(
                    "Couldn't move window - monitor placement is off for this target.".to_string(),
                );
            }
            ResolvedMonitor::Device(device) => {
                let label = monitor_label(&monitors, &device);
                (device, label)
            }
        };
        let result = monitor::try_place_window(
            hwnd,
            &target_device,
            &facts,
            &options,
            false,
            Instant::now(),
            &Win32ActivityProbe,
        );
        if let Some(window) = inner.windows.get_mut(&identity) {
            window.monitor_status = Some(result.status_label().to_string());
            if matches!(
                result,
                PlacementResult::Moved | PlacementResult::AlreadyOnTarget
            ) {
                window.monitor_placed = true;
                window.monitor_move_failures = 0;
            } else if let PlacementResult::Failed(_) = result {
                window.monitor_move_failures = window.monitor_move_failures.saturating_add(1);
            }
        }
        match result {
            PlacementResult::Moved => {
                let label = target_label.unwrap_or(target_device);
                inner.push_log("info", format!("Moved {title} to {label}"));
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
                let hint = if fullscreen {
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
        let detected =
            detector::scan_windows(sensitivity, &*gpu, &always_mark, supplement.as_ref());
        drop(gpu);
        {
            let mut inner = self.inner.write();
            inner.apply_detection(detected, Instant::now(), &self.community);
        }
        self.maybe_request_elevation();
        self.persist_stats(false);
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
            tracing::warn!(
                "Couldn't save stats - check %APPDATA% permissions to fix this: {error}"
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

    /// Fire one keepalive at a specific tracked window right now.
    pub fn test_target(&self, exe: &str, wclass: &str) -> Result<String, String> {
        let mut inner = self.inner.write();
        let identity = identity_key(exe, wclass);
        let (hwnd, title, exe_c, wclass_c, health) = {
            let Some(window) = inner
                .windows
                .get(&identity)
                .filter(|w| w.gone_since.is_none())
            else {
                return Err(format!(
                    "Couldn't test {exe} - the window is not currently visible."
                ));
            };
            (
                window.hwnd,
                window.title.clone(),
                window.exe.clone(),
                window.wclass.clone(),
                window.health.clone(),
            )
        };
        let base_action = KeepaliveOptions::from_config(&inner.config, &exe_c, &wclass_c).action;
        let (action, send_without_focus, label, _log_label) = resolve_keepalive_action(
            KeepaliveResolveContext {
                config: &inner.config,
                stats: &inner.stats,
                identity: &identity,
                exe: &exe_c,
                wclass: &wclass_c,
                base: &base_action,
                health: &health,
                community_entry: None,
            },
            &mut thread_rng(),
        );
        let mut options = KeepaliveOptions::from_config(&inner.config, &exe_c, &wclass_c);
        options.action = action;
        options.send_without_focus = send_without_focus;
        let target = KeepaliveTarget {
            hwnd,
            exe: exe_c,
            wclass: wclass_c,
        };
        match keepalive::send_keepalive(&target, &options) {
            Ok(injected_tick) => {
                let now = Instant::now();
                if let Some(window) = inner.windows.get_mut(&identity) {
                    window.actions = window.actions.saturating_add(1);
                    window.last_action_at = Some(now);
                    window.last_action_ok = Some(true);
                    window.last_injected_input_tick = injected_tick;
                    window.health.note_success();
                }
                inner
                    .stats
                    .note_action_result(&identity, &title, &label, true);
                inner.push_log("action", format!("Test: {label} → {title}"));
                Ok(label)
            }
            Err(error) => {
                let auto_fallback = inner.config.auto_fallback;
                if let Some(window) = inner.windows.get_mut(&identity) {
                    window.last_action_ok = Some(false);
                    if let Some(warning) = window.health.note_failure(auto_fallback) {
                        inner.push_log("error", warning);
                    }
                }
                inner
                    .stats
                    .note_action_result(&identity, &title, &label, false);
                inner.push_log("error", error.to_string());
                Err(error.to_string())
            }
        }
    }

    /// Drain toast notices queued by the engine for the notification pump.
    pub fn take_notices(&self) -> Vec<String> {
        std::mem::take(&mut self.inner.write().notices)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
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

    fn push_notice(&mut self, text: String) {
        self.notices.push(text);
        if self.notices.len() > 8 {
            self.notices.remove(0);
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
            window.effective = match (self.config.manual_mode, override_verdict) {
                (true, Some(OverrideVerdict::Game)) => Verdict::Game,
                (true, _) => Verdict::Ignored,
                (false, Some(OverrideVerdict::Game)) => Verdict::Game,
                (false, Some(OverrideVerdict::Ignored)) => Verdict::Ignored,
                (false, None) => window.verdict,
            };

            let paused = self.config.is_paused(&window.exe, &window.wclass);
            if window.effective == Verdict::Game
                && window.gone_since.is_none()
                && !paused
                && window.primary_keepalive
            {
                if window.timer.is_none() {
                    let options =
                        KeepaliveOptions::from_config(&self.config, &window.exe, &window.wclass);
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
    ) {
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
                    facts: detected.facts,
                    last_action_at: None,
                    last_action_ok: None,
                    was_armed: false,
                    monitor_placed: false,
                    monitor_status: None,
                    monitor_move_failures: 0,
                    health: KeepaliveHealth::default(),
                    primary_keepalive: true,
                    last_injected_input_tick: None,
                });

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
                        tracing::warn!("Couldn't save community profile: {error}");
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
        self.place_windows(now, &activity);
        self.drive_keepalives(now, elapsed, &activity, &mut rng, community);
    }

    fn assign_primary_keepalives(&mut self) {
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

    fn place_windows(&mut self, now: Instant, activity: &Win32ActivityProbe) {
        if !self.config.monitor_placement {
            return;
        }

        let monitors = self.cached_monitors(now);
        let options = PlacementOptions {
            when: self.config.monitor_when,
            style: self.config.monitor_style,
            skip_active: self.config.monitor_skip_active,
            skip_active_secs: self.config.monitor_skip_active_secs,
        };

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

            let exe = window.exe.clone();
            let wclass = window.wclass.clone();
            let hwnd = window.hwnd;
            let facts = window.facts.clone();
            let title = window.title.clone();
            let placed = window.monitor_placed;

            let (target_device, target_label) = match self.config.resolve_monitor(&exe, &wclass) {
                ResolvedMonitor::Off => {
                    if let Some(window) = self.windows.get_mut(&identity) {
                        window.monitor_status =
                            Some(PlacementResult::SkippedOff.status_label().to_string());
                    }
                    continue;
                }
                ResolvedMonitor::Device(device) => {
                    let label = monitor_label(&monitors, &device);
                    (device, label)
                }
            };

            let result = monitor::try_place_window(
                hwnd,
                &target_device,
                &facts,
                &options,
                placed,
                now,
                activity,
            );

            if let Some(window) = self.windows.get_mut(&identity) {
                window.monitor_status = Some(result.status_label().to_string());
                if matches!(
                    result,
                    PlacementResult::Moved | PlacementResult::AlreadyOnTarget
                ) {
                    window.monitor_placed = true;
                    window.monitor_move_failures = 0;
                    if self.config.community_intelligence {
                        community::record_monitor_result(&exe, true);
                    }
                } else if matches!(result, PlacementResult::Failed(_)) {
                    window.monitor_move_failures = window.monitor_move_failures.saturating_add(1);
                    if self.config.community_intelligence {
                        community::record_monitor_result(&exe, false);
                    }
                    if window.facts.fullscreen && window.monitor_move_failures >= FAILURE_THRESHOLD
                    {
                        window.monitor_status = Some("Try borderless/windowed".to_string());
                    }
                }
            }

            if result == PlacementResult::Moved {
                self.push_log(
                    "info",
                    format!(
                        "Moved {title} to {}",
                        target_label.unwrap_or_else(|| target_device.clone())
                    ),
                );
            } else if let PlacementResult::Failed(reason) = result {
                self.push_log(
                    "error",
                    format!("Monitor move failed for {title}: {reason}"),
                );
                self.push_notice(format!("Monitor move failed for {title}"));
            }
        }
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
                self.push_notice(text);
            }
        }
    }

    /// Why the engine is currently holding fire across all targets, if at all.
    fn compute_gate(&self, now: Instant, activity: &Win32ActivityProbe) -> Option<String> {
        use crate::keepalive::ActivityProbe;

        if self.config.pause_on_battery && keepalive::on_battery() {
            return Some("ON BATTERY".to_string());
        }
        if self.config.pause_when_locked && keepalive::session_locked() {
            return Some("SESSION LOCKED".to_string());
        }
        let minutes_now = {
            use chrono::Timelike;
            let t = chrono::Local::now();
            t.hour() * 60 + t.minute()
        };
        if self.config.in_quiet_hours(minutes_now) {
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
    ) {
        if self.config.suspended || self.snooze_until.is_some() {
            self.status = EngineStatus::Suspended;
            self.stats.note_dormant();
            self.last_error = None;
            return;
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
            self.gate_reason = gate.clone();
        }

        let notify_all = matches!(
            self.config.notifications,
            crate::config::NotificationLevel::All
        );
        let notify_errors = !matches!(
            self.config.notifications,
            crate::config::NotificationLevel::None
        );

        let mut active_count = 0;
        let mut holding = false;
        let mut log_entries: Vec<(String, String)> = Vec::new();
        let mut notices: Vec<String> = Vec::new();

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
            let base_action = KeepaliveOptions::from_config(&self.config, &exe, &wclass).action;
            let health = window.health.clone();
            let community_entry = if self.config.community_intelligence {
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
                    base: &base_action,
                    health: &health,
                    community_entry: community_entry.as_ref(),
                },
                rng,
            );
            let mut options = KeepaliveOptions::from_config(&self.config, &exe, &wclass);
            options.action = action;
            options.send_without_focus = send_without_focus;
            let target = KeepaliveTarget {
                hwnd,
                exe: exe.clone(),
                wclass,
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

            if keepalive::should_hold(&target, &options, now, activity, ignore_input_tick) {
                holding = true;
            }

            let Some(window) = self.windows.get_mut(&identity) else {
                continue;
            };
            let Some(timer) = window.timer.as_mut() else {
                continue;
            };

            match keepalive::tick_decision(
                timer,
                &target,
                &options,
                now,
                activity,
                ignore_input_tick,
            ) {
                TickDecision::Waiting => {}
                TickDecision::Held => {
                    holding = true;
                    log_entries.push((
                        "info".into(),
                        format!("Held tick: recent input for {title}"),
                    ));
                    timer.reschedule(now, &options, rng);
                }
                TickDecision::Send if gate.is_some() => {
                    // A global gate (quiet hours, battery, lock, idle, cap) blocks the send.
                    timer.reschedule(now, &options, rng);
                }
                TickDecision::Send => {
                    match keepalive::send_keepalive(&target, &options) {
                        Ok(injected_tick) => {
                            let first = window.actions == 0;
                            window.actions = window.actions.saturating_add(1);
                            window.last_action_at = Some(now);
                            window.last_action_ok = Some(true);
                            window.last_injected_input_tick = injected_tick;
                            window.health.note_success();
                            self.stats
                                .note_action_result(&identity, &title, &label, true);
                            if self.config.adaptive_learn_actions {
                                self.stats.note_learned_action_success(&identity, &label);
                            }
                            if self.config.community_intelligence {
                                let top_keys = self
                                    .stats
                                    .learned_profile(&identity)
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
                                    &exe,
                                    &label,
                                    true,
                                    send_without_focus,
                                    &top_keys,
                                );
                            }
                            self.last_error = None;
                            log_entries
                                .push(("action".into(), format!("Sent {log_label} to {title}")));
                            if first && notify_all {
                                notices.push(format!("First keepalive sent to {title}"));
                            }
                        }
                        Err(error) => {
                            window.last_action_ok = Some(false);
                            if let Some(warning) =
                                window.health.note_failure(self.config.auto_fallback)
                            {
                                log_entries.push(("error".into(), warning.clone()));
                                notices.push(warning);
                            }
                            self.stats
                                .note_action_result(&identity, &title, &label, false);
                            if self.config.community_intelligence {
                                community::record_keepalive(
                                    &exe,
                                    &label,
                                    false,
                                    send_without_focus,
                                    &[],
                                );
                            }
                            self.last_error = Some(error.to_string());
                            log_entries.push(("error".into(), error.to_string()));
                            if notify_errors {
                                notices.push(error.to_string());
                            }
                            tracing::warn!("{error}");
                        }
                    }
                    if let Some(window) = self.windows.get_mut(&identity) {
                        if let Some(timer) = window.timer.as_mut() {
                            timer.reschedule(now, &options, rng);
                        }
                    }
                }
            }
        }

        for (kind, text) in log_entries {
            self.push_log(&kind, text);
        }
        for notice in notices {
            self.push_notice(notice);
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
            threshold: detector::threshold(config.sensitivity),
            facts: self.facts.clone(),
            next_tick: self.timer.as_ref().map(|timer| timer.seconds_until(now)),
            last_action_secs: self
                .last_action_at
                .map(|at| now.saturating_duration_since(at).as_secs()),
            last_action_ok: self.last_action_ok,
            elevated_mismatch: self.facts.elevated == Some(true) && !current_elevated,
            learned,
            monitor: GameMonitorSnapshot {
                target: monitor_target_label(config, &self.exe, &self.wclass),
                status: self.monitor_status.clone(),
            },
            profile: profile_snapshot(&profile, config),
            health_warning: self.health.warning(),
            consecutive_failures: self.health.consecutive_failures,
            success_rate,
            primary_keepalive: self.primary_keepalive,
            community,
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
    let (mut action, send_without_focus) =
        effective_health.apply_to_options(base, config.send_without_focus);

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
    }
}

fn profile_monitor_global_label(config: &AppConfig) -> Option<String> {
    if config.monitor_placement {
        Some("Use global".to_string())
    } else {
        None
    }
}

fn monitor_target_label(config: &AppConfig, exe: &str, wclass: &str) -> Option<String> {
    match config.resolve_monitor(exe, wclass) {
        ResolvedMonitor::Off => None,
        ResolvedMonitor::Device(device) => {
            monitor::monitor_by_device(&device).map(|info| info.label)
        }
    }
}

fn monitor_label(monitors: &[MonitorInfo], device: &str) -> Option<String> {
    monitors
        .iter()
        .find(|monitor| monitor.device == device)
        .map(|monitor| monitor.label.clone())
}

fn effective_verdict(config: &AppConfig, detected: &DetectedWindow) -> Verdict {
    match (
        config.manual_mode,
        config.override_for(&detected.facts.exe, &detected.facts.wclass),
    ) {
        (true, Some(OverrideVerdict::Game)) => Verdict::Game,
        (true, _) => Verdict::Ignored,
        (false, Some(OverrideVerdict::Game)) => Verdict::Game,
        (false, Some(OverrideVerdict::Ignored)) => Verdict::Ignored,
        (false, None) => detected.verdict,
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
        thread::sleep(Duration::from_millis(250));
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
}
