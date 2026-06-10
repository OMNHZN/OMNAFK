use crate::{
    config::{AppConfig, OverrideVerdict},
    detector::{self, DetectedWindow, NoGpuUsageProbe, Verdict},
    keepalive::{
        self, KeepaliveOptions, KeepaliveTarget, TickDecision, TickTimer, Win32ActivityProbe,
    },
    stats::{Stats, StatsSnapshot},
};
use parking_lot::{Mutex, RwLock};
use rand::thread_rng;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const DETECTION_INTERVAL: Duration = Duration::from_secs(5);
const GONE_LINGER: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EngineStatus {
    Dormant,
    Active,
    Holding,
    Suspended,
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
    pub uptime: u64,
    pub actions: u64,
}

#[derive(Debug, Clone)]
pub struct EngineSnapshot {
    pub engine: EngineStatus,
    pub next_tick: Option<u64>,
    pub games: Vec<GameSnapshot>,
    pub stats: StatsSnapshot,
    pub config: AppConfig,
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
}

impl Engine {
    pub fn new(config: AppConfig) -> SharedEngine {
        Arc::new(Self {
            inner: RwLock::new(EngineInner {
                config,
                windows: BTreeMap::new(),
                stats: Stats::default(),
                status: EngineStatus::Dormant,
                last_cycle: Instant::now(),
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
        }));
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.lock().take() {
            let _ = worker.join();
        }
    }

    pub fn run_detection_cycle(&self) {
        {
            let mut inner = self.inner.write();
            if inner.config.suspended {
                inner.status = EngineStatus::Suspended;
                return;
            }
        }

        let sensitivity = self.inner.read().config.sensitivity;
        let detected = detector::scan_windows(sensitivity, &NoGpuUsageProbe);
        self.apply_detection(detected, Instant::now());
    }

    pub fn snapshot(&self) -> EngineSnapshot {
        let inner = self.inner.read();
        let mut games: Vec<_> = inner.windows.values().map(TrackedWindow::snapshot).collect();
        games.sort_by_key(|game| (game.effective != Verdict::Game, game.gone, game.exe.clone()));

        EngineSnapshot {
            engine: inner.status,
            next_tick: inner.next_tick(Instant::now()),
            games,
            stats: inner.stats.snapshot(),
            config: inner.config.clone(),
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

    fn apply_detection(&self, detected: Vec<DetectedWindow>, now: Instant) {
        let activity = Win32ActivityProbe;
        let mut rng = thread_rng();
        let mut inner = self.inner.write();
        let elapsed = now
            .checked_duration_since(inner.last_cycle)
            .unwrap_or_default()
            .as_secs();
        inner.last_cycle = now;

        let mut seen = BTreeSet::new();
        for detected in detected {
            let identity = identity_key(&detected.facts.exe, &detected.facts.wclass);
            seen.insert(identity.clone());
            let effective = effective_verdict(&inner.config, &detected);
            let overridden = inner
                .config
                .override_for(&detected.facts.exe, &detected.facts.wclass)
                .is_some();

            inner
                .windows
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
                })
                .or_insert_with(|| TrackedWindow {
                    title: detected.facts.title,
                    exe: detected.facts.exe,
                    wclass: detected.facts.wclass,
                    pid: detected.facts.pid,
                    hwnd: detected.hwnd,
                    verdict: detected.verdict,
                    effective,
                    overridden,
                    gone_since: None,
                    uptime: 0,
                    actions: 0,
                    timer: None,
                });

            if effective == Verdict::Game {
                inner.stats.note_seen_today(&identity);
            }
        }

        for (identity, window) in inner.windows.iter_mut() {
            if !seen.contains(identity) && window.gone_since.is_none() {
                window.gone_since = Some(now);
            }
        }

        inner
            .windows
            .retain(|_, window| window.gone_since.is_none_or(|gone| now.duration_since(gone) <= GONE_LINGER));

        inner.recompute_effective(now);
        inner.drive_keepalives(now, elapsed, &activity, &mut rng);
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl EngineInner {
    fn clear_timers(&mut self) {
        for window in self.windows.values_mut() {
            window.timer = None;
        }
    }

    fn apply_suspended_status(&mut self) {
        if self.config.suspended {
            self.status = EngineStatus::Suspended;
        }
    }

    fn recompute_effective(&mut self, now: Instant) {
        let options = KeepaliveOptions::from(&self.config);
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

            if window.effective == Verdict::Game && window.gone_since.is_none() {
                if window.timer.is_none() {
                    window.timer = Some(TickTimer::new(now, &options, &mut rng));
                }
            } else {
                window.timer = None;
            }
        }
    }

    fn drive_keepalives(
        &mut self,
        now: Instant,
        elapsed: u64,
        activity: &Win32ActivityProbe,
        rng: &mut impl rand::Rng,
    ) {
        if self.config.suspended {
            self.status = EngineStatus::Suspended;
            return;
        }

        let options = KeepaliveOptions::from(&self.config);
        let mut active_count = 0;
        let mut holding = false;

        for (identity, window) in self.windows.iter_mut() {
            if window.effective != Verdict::Game || window.gone_since.is_some() {
                continue;
            }

            active_count += 1;
            if elapsed > 0 {
                window.uptime = window.uptime.saturating_add(elapsed);
                self.stats.note_kept(elapsed);
            }
            self.stats.note_seen_today(identity);

            let target = KeepaliveTarget {
                hwnd: window.hwnd,
                exe: window.exe.clone(),
            };

            if keepalive::should_hold(&target, &options, now, activity) {
                holding = true;
            }

            let Some(timer) = window.timer.as_mut() else {
                continue;
            };

            match keepalive::tick_decision(timer, &target, &options, now, activity) {
                TickDecision::Waiting => {}
                TickDecision::Held => {
                    holding = true;
                    timer.reschedule(now, &options, rng);
                }
                TickDecision::Send => {
                    match keepalive::send_keepalive(&target, &options) {
                        Ok(()) => {
                            window.actions = window.actions.saturating_add(1);
                            self.stats.note_action();
                        }
                        Err(error) => tracing::warn!("{error}"),
                    }
                    timer.reschedule(now, &options, rng);
                }
            }
        }

        self.status = if active_count == 0 {
            EngineStatus::Dormant
        } else if holding {
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
    fn snapshot(&self) -> GameSnapshot {
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
            uptime: self.uptime,
            actions: self.actions,
        }
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

fn identity_key(exe: &str, wclass: &str) -> String {
    format!("{}\u{1f}{wclass}", exe.to_ascii_lowercase())
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
}
