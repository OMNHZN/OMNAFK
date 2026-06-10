use crate::{
    config::{AppConfig, OverrideVerdict, TargetProfile},
    detector::{self, DetectedWindow, NoGpuUsageProbe, Verdict, WindowFacts},
    keepalive::{
        self, KeepaliveOptions, KeepaliveTarget, TickDecision, TickTimer, Win32ActivityProbe,
    },
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
    pub profile: GameProfileSnapshot,
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
    worker: Mutex<Option<JoinHandle<()>>>,
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
}

impl Engine {
    pub fn new(config: AppConfig) -> SharedEngine {
        Self::with_stats(config, PersistedStats::default())
    }

    pub fn with_stats(config: AppConfig, persisted: PersistedStats) -> SharedEngine {
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
            }),
            stop: AtomicBool::new(false),
            worker: Mutex::new(None),
        })
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
                sleep_until_next_cycle(&engine.stop);
            }
            engine.persist_stats(true);
        }));
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.lock().take() {
            let _ = worker.join();
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

        let sensitivity = self.inner.read().config.sensitivity;
        let detected = detector::scan_windows(sensitivity, &NoGpuUsageProbe);
        {
            let mut inner = self.inner.write();
            inner.apply_detection(detected, Instant::now());
        }
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
        let now = Instant::now();
        let mut games: Vec<_> = inner
            .windows
            .values()
            .map(|window| window.snapshot(&inner.config, now, inner.current_elevated))
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
        let Some(window) = inner
            .windows
            .get(&identity)
            .filter(|w| w.gone_since.is_none())
        else {
            return Err(format!(
                "Couldn't test {exe} - the window is not currently visible."
            ));
        };
        let options = KeepaliveOptions::from_config(&inner.config, &window.exe, &window.wclass);
        let target = KeepaliveTarget {
            hwnd: window.hwnd,
            exe: window.exe.clone(),
            wclass: window.wclass.clone(),
        };
        let label = options.action.label();
        let title = window.title.clone();
        match keepalive::send_keepalive(&target, &options) {
            Ok(()) => {
                let now = Instant::now();
                if let Some(window) = inner.windows.get_mut(&identity) {
                    window.actions = window.actions.saturating_add(1);
                    window.last_action_at = Some(now);
                    window.last_action_ok = Some(true);
                }
                inner.stats.note_action(&identity, &title, &label);
                inner.push_log("action", format!("Test: {label} → {title}"));
                Ok(label)
            }
            Err(error) => {
                if let Some(window) = inner.windows.get_mut(&identity) {
                    window.last_action_ok = Some(false);
                }
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
            if window.effective == Verdict::Game && window.gone_since.is_none() && !paused {
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

    fn apply_detection(&mut self, detected: Vec<DetectedWindow>, now: Instant) {
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
            let effective = effective_verdict(&self.config, &detected);
            let overridden = self
                .config
                .override_for(&detected.facts.exe, &detected.facts.wclass)
                .is_some();

            self.windows
                .entry(identity.clone())
                .and_modify(|window| {
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
                });

            if effective == Verdict::Game {
                self.stats.note_seen_today(&identity);
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
        self.note_armed_transitions();
        self.drive_keepalives(now, elapsed, &activity, &mut rng);
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
                    format!("Armed: {}", window.title)
                } else {
                    format!("Disarmed: {}", window.title)
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
                Some(reason) => self.push_log("info", format!("Paused — {reason}")),
                None => {
                    if self.gate_reason.is_some() {
                        self.push_log("info", "Resumed — gate cleared".to_string());
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

            active_count += 1;
            let title = window.title.clone();
            let options = KeepaliveOptions::from_config(&self.config, &window.exe, &window.wclass);
            let target = KeepaliveTarget {
                hwnd: window.hwnd,
                exe: window.exe.clone(),
                wclass: window.wclass.clone(),
            };

            if gate.is_none() && elapsed > 0 {
                self.stats.note_kept(&identity, &title, elapsed);
                if let Some(window) = self.windows.get_mut(&identity) {
                    window.uptime = window.uptime.saturating_add(elapsed);
                }
            }
            self.stats.note_seen_today(&identity);

            if keepalive::should_hold(&target, &options, now, activity) {
                holding = true;
            }

            let Some(window) = self.windows.get_mut(&identity) else {
                continue;
            };
            let Some(timer) = window.timer.as_mut() else {
                continue;
            };

            match keepalive::tick_decision(timer, &target, &options, now, activity) {
                TickDecision::Waiting => {}
                TickDecision::Held => {
                    holding = true;
                    timer.reschedule(now, &options, rng);
                }
                TickDecision::Send if gate.is_some() => {
                    // A global gate (quiet hours, battery, lock, idle, cap) blocks the send.
                    timer.reschedule(now, &options, rng);
                }
                TickDecision::Send => {
                    let label = options.action.label();
                    match keepalive::send_keepalive(&target, &options) {
                        Ok(()) => {
                            let first = window.actions == 0;
                            window.actions = window.actions.saturating_add(1);
                            window.last_action_at = Some(now);
                            window.last_action_ok = Some(true);
                            self.stats.note_action(&identity, &title, &label);
                            self.last_error = None;
                            log_entries.push(("action".into(), format!("{label} → {title}")));
                            if first && notify_all {
                                notices.push(format!("First keepalive sent to {title}"));
                            }
                        }
                        Err(error) => {
                            window.last_action_ok = Some(false);
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
    fn snapshot(&self, config: &AppConfig, now: Instant, current_elevated: bool) -> GameSnapshot {
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
            profile: profile_snapshot(&profile),
        }
    }
}

fn profile_snapshot(profile: &TargetProfile) -> GameProfileSnapshot {
    GameProfileSnapshot {
        action: profile.action.map(|action| action.label().to_string()),
        interval: profile.interval,
        key_sequence: profile.key_sequence.clone(),
    }
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

fn sleep_until_next_cycle(stop: &AtomicBool) {
    let start = Instant::now();
    while !stop.load(Ordering::SeqCst) && start.elapsed() < DETECTION_INTERVAL {
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
