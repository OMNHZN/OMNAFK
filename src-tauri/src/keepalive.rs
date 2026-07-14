use crate::config::{key_name_to_vk, AppConfig, ResolvedAction};
use rand::Rng;
use std::{
    ffi::c_void,
    fmt,
    mem::size_of,
    thread,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{HWND, LPARAM, WPARAM},
    System::{
        Power::GetSystemPowerStatus,
        StationsAndDesktops::{
            CloseDesktop, OpenInputDesktop, DESKTOP_ACCESS_FLAGS, DESKTOP_CONTROL_FLAGS,
        },
        SystemInformation::GetTickCount64,
        Threading::{AttachThreadInput, GetCurrentThreadId},
    },
    UI::{
        Input::KeyboardAndMouse::{
            GetLastInputInfo, MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD,
            INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, LASTINPUTINFO,
            MAPVK_VK_TO_VSC, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
            MOUSEEVENTF_WHEEL, MOUSEINPUT, VIRTUAL_KEY,
        },
        WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId, PostMessageW, SetForegroundWindow,
            WHEEL_DELTA, WM_KEYDOWN, WM_KEYUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN,
            WM_RBUTTONUP,
        },
    },
};

const KEY_SPACE: u16 = 0x20;
const KEY_W: u16 = 0x57;
const KEY_ALT: u16 = 0x12;
const KEY_HOLD_BASE_MS: u64 = 50;
const MOUSE_STEP_MS: u64 = 25;
const FOCUS_SETTLE_MS: u64 = 80;

#[derive(Debug, Clone)]
pub struct KeepaliveOptions {
    pub interval: u64,
    pub randomize: bool,
    pub jitter_pct: u8,
    pub action: ResolvedAction,
    pub send_without_focus: bool,
    pub hold_while_playing: bool,
    pub hold_window_secs: u64,
}

impl KeepaliveOptions {
    pub fn from_config(config: &AppConfig, exe: &str, wclass: &str) -> Self {
        let resolved = config.resolve_keepalive(exe, wclass);
        let profile = config.profile_for(exe, wclass);
        Self {
            interval: resolved.interval,
            randomize: config.randomize,
            jitter_pct: config.jitter_pct,
            action: resolved.action,
            send_without_focus: profile
                .and_then(|p| p.send_without_focus)
                .unwrap_or(config.send_without_focus),
            hold_while_playing: profile
                .and_then(|p| p.hold_while_playing)
                .unwrap_or(config.hold_while_playing),
            hold_window_secs: profile
                .and_then(|p| p.hold_window_secs)
                .unwrap_or(config.hold_window_secs),
        }
    }

    pub fn global_from_config(config: &AppConfig) -> Self {
        Self::from_config(config, "", "")
    }
}

#[derive(Debug, Clone)]
pub struct KeepaliveTarget {
    pub hwnd: isize,
    pub exe: String,
    pub wclass: String,
}

#[derive(Debug, Clone)]
pub struct TickTimer {
    next_due: Instant,
}

impl TickTimer {
    pub fn new(now: Instant, options: &KeepaliveOptions, rng: &mut impl Rng) -> Self {
        Self {
            next_due: now
                + next_delay(options.interval, options.randomize, options.jitter_pct, rng),
        }
    }

    pub fn due(&self, now: Instant) -> bool {
        now >= self.next_due
    }

    pub fn seconds_until(&self, now: Instant) -> u64 {
        self.next_due
            .checked_duration_since(now)
            .unwrap_or_default()
            .as_secs()
    }

    pub fn reschedule(&mut self, now: Instant, options: &KeepaliveOptions, rng: &mut impl Rng) {
        self.reschedule_scaled(now, options, rng, 1);
    }

    /// Push the due time back a short amount (polite defer) without starting
    /// a whole new interval.
    pub fn defer(&mut self, now: Instant, by: Duration) {
        self.next_due = now + by;
    }

    /// Reschedule with the interval multiplied by `backoff` (>=1). Used to space
    /// out retries for a target whose keepalives keep failing.
    pub fn reschedule_scaled(
        &mut self,
        now: Instant,
        options: &KeepaliveOptions,
        rng: &mut impl Rng,
        backoff: u32,
    ) {
        let interval = options.interval.saturating_mul(backoff.max(1) as u64);
        self.next_due = now + next_delay(interval, options.randomize, options.jitter_pct, rng);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickDecision {
    Waiting,
    Held,
    /// Target is backgrounded and the user is mid-input: wait a few seconds
    /// for a quiet moment before the focus flick, instead of holding.
    DeferBriefly,
    Send,
}

pub trait ActivityProbe {
    fn foreground_window(&self) -> Option<isize>;
    fn last_input_age(&self, now: Instant) -> Option<Duration>;
    fn last_input_tick(&self) -> Option<u32> {
        None
    }
}

#[derive(Debug, Default)]
pub struct Win32ActivityProbe;

impl ActivityProbe for Win32ActivityProbe {
    fn foreground_window(&self) -> Option<isize> {
        let hwnd = unsafe { GetForegroundWindow() };
        (!hwnd.is_invalid()).then_some(hwnd.0 as isize)
    }

    fn last_input_age(&self, _now: Instant) -> Option<Duration> {
        last_input_tick().map(|tick| {
            let now = unsafe { GetTickCount64() };
            Duration::from_millis(now.saturating_sub(tick as u64))
        })
    }

    fn last_input_tick(&self) -> Option<u32> {
        last_input_tick()
    }
}

/// Raw `GetLastInputInfo` tick — used to distinguish OMNAFK injections from real user input.
pub fn last_input_tick() -> Option<u32> {
    let mut info = LASTINPUTINFO {
        cbSize: size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    unsafe { GetLastInputInfo(&mut info).as_bool() }.then_some(info.dwTime)
}

pub fn tick_decision(
    timer: &TickTimer,
    target: &KeepaliveTarget,
    options: &KeepaliveOptions,
    now: Instant,
    activity: &dyn ActivityProbe,
    ignore_input_tick: Option<u32>,
) -> TickDecision {
    if !timer.due(now) {
        return TickDecision::Waiting;
    }

    if should_hold(target, options, now, activity, ignore_input_tick) {
        return TickDecision::Held;
    }

    // The target is backgrounded, so delivery would be a focus flick. If the
    // user is typing or mousing right now, wait for a short input gap so the
    // flick doesn't steal focus mid-keystroke. This delays the tick by
    // seconds, never holds it.
    if !options.send_without_focus
        && activity.foreground_window() != Some(target.hwnd)
        && genuine_recent_user_input(
            now,
            activity,
            Duration::from_secs(POLITE_GAP_SECS),
            ignore_input_tick,
        )
    {
        return TickDecision::DeferBriefly;
    }

    TickDecision::Send
}

/// Hold only while the user is actively playing this game: the target window
/// is foreground and real input arrived within the hold window. Input given
/// to other apps never holds a background game's keepalives.
pub fn should_hold(
    target: &KeepaliveTarget,
    options: &KeepaliveOptions,
    now: Instant,
    activity: &dyn ActivityProbe,
    ignore_input_tick: Option<u32>,
) -> bool {
    if activity.foreground_window() != Some(target.hwnd) {
        return false;
    }
    recent_user_input(options, now, activity, ignore_input_tick)
}

/// True when the user (not OMNAFK) recently gave keyboard/mouse input.
pub fn recent_user_input(
    options: &KeepaliveOptions,
    now: Instant,
    activity: &dyn ActivityProbe,
    ignore_input_tick: Option<u32>,
) -> bool {
    options.hold_while_playing
        && genuine_recent_user_input(
            now,
            activity,
            Duration::from_secs(options.hold_window_secs.max(1)),
            ignore_input_tick,
        )
}

/// Recent input that is not solely OMNAFK's last injected tick.
pub fn genuine_recent_user_input(
    now: Instant,
    activity: &dyn ActivityProbe,
    max_age: Duration,
    ignore_input_tick: Option<u32>,
) -> bool {
    let Some(age) = activity.last_input_age(now) else {
        return false;
    };
    if age > max_age {
        return false;
    }
    if let Some(ignored) = ignore_input_tick {
        if activity
            .last_input_tick()
            .is_some_and(|tick| tick == ignored)
        {
            return false;
        }
    }
    true
}

/// Number of confirmed sends before a target relaxes to its full interval.
pub const WARMUP_TICKS: u32 = 3;
/// Recent-input gap (seconds) required before a background focus flick fires.
pub const POLITE_GAP_SECS: u64 = 3;
/// How long (seconds) a due background tick is pushed back while the user is mid-input.
pub const POLITE_DEFER_SECS: u64 = 5;
/// Longest interval used during warm-up, so a freshly detected game is kept
/// awake (and its action confirmed) quickly before relaxing to the full cadence.
pub const WARMUP_MAX_INTERVAL: u64 = 60;

/// Ease-in interval: cap the cadence during the first few ticks, then use the
/// configured interval. Never longer than `base`, so it can only keep a game
/// more awake, never less.
pub fn warmup_interval(base: u64, confirmed_sends: u32, enabled: bool) -> u64 {
    if !enabled || confirmed_sends >= WARMUP_TICKS {
        base
    } else {
        base.min(WARMUP_MAX_INTERVAL)
    }
}

pub fn next_delay(
    interval_secs: u64,
    randomize: bool,
    jitter_pct: u8,
    rng: &mut impl Rng,
) -> Duration {
    if !randomize || jitter_pct == 0 {
        return Duration::from_secs(interval_secs);
    }

    let spread = interval_secs as f64 * (jitter_pct.min(50) as f64 / 100.0);
    let jitter = rng.gen_range(-spread..=spread);
    let jittered = (interval_secs as f64 + jitter).round().max(1.0) as u64;
    Duration::from_secs(jittered)
}

/// True when the machine is running on battery power.
pub fn on_battery() -> bool {
    let mut status = unsafe { std::mem::zeroed() };
    if unsafe { GetSystemPowerStatus(&mut status) }.is_err() {
        return false;
    }
    // 0 = offline, 1 = online, 255 = unknown.
    status.ACLineStatus == 0
}

/// True when the workstation is locked (the input desktop can't be opened).
pub fn session_locked() -> bool {
    // DESKTOP_SWITCHDESKTOP (0x0100): fails while the workstation is locked.
    let desktop = unsafe {
        OpenInputDesktop(
            DESKTOP_CONTROL_FLAGS(0),
            false,
            DESKTOP_ACCESS_FLAGS(0x0100),
        )
    };
    match desktop {
        Ok(handle) => {
            unsafe {
                let _ = CloseDesktop(handle);
            }
            false
        }
        Err(_) => true,
    }
}

/// How keepalive input reaches the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryStrategy {
    /// Target is already foreground — inject with `SendInput` (no focus change).
    DirectSendInput,
    /// Target is in the background — brief focus flick, then `SendInput`.
    FocusFlick,
    /// Legacy background `PostMessage` path (opt-in only; many games ignore it).
    PostMessage,
}

/// Pick the delivery path for a target HWND vs the current foreground window.
pub fn delivery_strategy(target_hwnd: isize, foreground_hwnd: Option<isize>) -> DeliveryStrategy {
    if foreground_hwnd == Some(target_hwnd) {
        DeliveryStrategy::DirectSendInput
    } else {
        DeliveryStrategy::FocusFlick
    }
}

pub fn send_keepalive(
    target: &KeepaliveTarget,
    options: &KeepaliveOptions,
) -> Result<Option<u32>, KeepaliveError> {
    let hold_ms = key_hold_ms(options.randomize);

    // The virtual gamepad is global and focus-independent — no window flick.
    if matches!(options.action, ResolvedAction::GamepadNudge) {
        crate::gamepad_send::nudge().map_err(KeepaliveError::other)?;
        return Ok(None);
    }

    let foreground = current_foreground_hwnd();

    if options.send_without_focus {
        // Expert opt-in: background PostMessage only. Most games ignore this.
        post_action(target, &options.action, hold_ms)?;
        return Ok(None);
    }

    match delivery_strategy(target.hwnd, foreground) {
        DeliveryStrategy::DirectSendInput => {
            send_input_action(&options.action, &target.exe, hold_ms)?;
        }
        DeliveryStrategy::FocusFlick => {
            focus_flick_action(target, &options.action, hold_ms)?;
        }
        DeliveryStrategy::PostMessage => {
            post_action(target, &options.action, hold_ms)?;
            return Ok(None);
        }
    }

    Ok(last_input_tick())
}

fn current_foreground_hwnd() -> Option<isize> {
    let hwnd = unsafe { GetForegroundWindow() };
    (!hwnd.is_invalid()).then_some(hwnd.0 as isize)
}

/// Vary the down->up delay so taps don't look machine-perfect.
fn key_hold_ms(randomize: bool) -> u64 {
    if randomize {
        rand::thread_rng().gen_range(50..=120)
    } else {
        KEY_HOLD_BASE_MS
    }
}

#[derive(Debug, Clone)]
pub struct KeepaliveError {
    message: String,
    /// True for temporary conditions (e.g. Windows refused the focus switch)
    /// that should be retried without counting toward failure escalation.
    transient: bool,
}

impl KeepaliveError {
    fn admin_hint(exe: &str) -> Self {
        Self {
            message: format!(
                "Couldn't send input to {exe} - it may be running as administrator. Restart OMNAFK as administrator to fix this."
            ),
            transient: false,
        }
    }

    fn focus_refused(exe: &str) -> Self {
        Self {
            message: format!(
                "Windows blocked the focus switch to {exe} — retrying at the next tick."
            ),
            transient: true,
        }
    }

    fn other(message: String) -> Self {
        Self {
            message,
            transient: false,
        }
    }

    pub fn is_transient(&self) -> bool {
        self.transient
    }
}

impl fmt::Display for KeepaliveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for KeepaliveError {}

fn post_action(
    target: &KeepaliveTarget,
    action: &ResolvedAction,
    hold_ms: u64,
) -> Result<(), KeepaliveError> {
    let hwnd = hwnd_from_isize(target.hwnd);
    match action {
        ResolvedAction::SpaceTap => post_key_tap(hwnd, KEY_SPACE, &target.exe, hold_ms),
        ResolvedAction::WTap => post_key_tap(hwnd, KEY_W, &target.exe, hold_ms),
        ResolvedAction::CameraNudge => {
            post_mouse_move(hwnd, 2, 0, &target.exe)?;
            thread::sleep(Duration::from_millis(25));
            post_mouse_move(hwnd, -2, 0, &target.exe)
        }
        ResolvedAction::MouseWiggle => {
            post_mouse_move(hwnd, 1, 1, &target.exe)?;
            thread::sleep(Duration::from_millis(25));
            post_mouse_move(hwnd, -1, -1, &target.exe)
        }
        ResolvedAction::ScrollTick => {
            post_mouse_wheel(hwnd, WHEEL_DELTA as i16, &target.exe)?;
            thread::sleep(Duration::from_millis(40));
            post_mouse_wheel(hwnd, -(WHEEL_DELTA as i16), &target.exe)
        }
        ResolvedAction::RightClick => {
            unsafe {
                PostMessageW(Some(hwnd), WM_RBUTTONDOWN, WPARAM(0x0002), LPARAM(0))
                    .map_err(|_| KeepaliveError::admin_hint(&target.exe))?;
            }
            thread::sleep(Duration::from_millis(hold_ms));
            unsafe {
                PostMessageW(Some(hwnd), WM_RBUTTONUP, WPARAM(0), LPARAM(0))
                    .map_err(|_| KeepaliveError::admin_hint(&target.exe))
            }
        }
        ResolvedAction::GamepadNudge => {
            // Focus-independent; PostMessage delivery doesn't apply.
            crate::gamepad_send::nudge().map_err(KeepaliveError::other)
        }
        ResolvedAction::KeySequence(keys) => {
            for key in keys {
                let vk = key_name_to_vk(key).ok_or_else(|| KeepaliveError::other(format!(
                    "Couldn't send key sequence - record supported keys in Settings to fix this: unsupported key '{key}'."
                )))?;
                post_key_tap(hwnd, vk, &target.exe, hold_ms)?;
                thread::sleep(Duration::from_millis(40));
            }
            Ok(())
        }
    }
}

fn focus_flick_action(
    target: &KeepaliveTarget,
    action: &ResolvedAction,
    hold_ms: u64,
) -> Result<(), KeepaliveError> {
    let target_hwnd = hwnd_from_isize(target.hwnd);
    let previous = unsafe { GetForegroundWindow() };

    if previous != target_hwnd {
        if !bring_to_foreground(target_hwnd) {
            return Err(KeepaliveError::focus_refused(&target.exe));
        }
        thread::sleep(Duration::from_millis(FOCUS_SETTLE_MS));
        // Re-verify after settling: if the user (or another app) grabbed
        // focus back mid-flick, abort rather than typing into their window.
        if unsafe { GetForegroundWindow() } != target_hwnd {
            return Err(KeepaliveError::focus_refused(&target.exe));
        }
    }

    let result = send_input_action(action, &target.exe, hold_ms);

    if !previous.is_invalid() && previous != target_hwnd {
        let _ = bring_to_foreground(previous);
    }

    result
}

/// `SetForegroundWindow` with the standard foreground-lock workarounds.
/// Windows refuses bare calls from background processes, so escalate:
/// direct call → `AttachThreadInput` to the foreground thread → synthetic
/// ALT release (makes this process the last input source). Each attempt is
/// verified; returns false only when every route was denied.
fn bring_to_foreground(hwnd: HWND) -> bool {
    unsafe {
        if SetForegroundWindow(hwnd).as_bool() && GetForegroundWindow() == hwnd {
            return true;
        }

        let foreground = GetForegroundWindow();
        if !foreground.is_invalid() && foreground != hwnd {
            let foreground_thread = GetWindowThreadProcessId(foreground, None);
            let our_thread = GetCurrentThreadId();
            if foreground_thread != 0 && foreground_thread != our_thread {
                let attached = AttachThreadInput(our_thread, foreground_thread, true).as_bool();
                let switched = SetForegroundWindow(hwnd).as_bool();
                if attached {
                    let _ = AttachThreadInput(our_thread, foreground_thread, false);
                }
                if switched && GetForegroundWindow() == hwnd {
                    return true;
                }
            }
        }

        // A lone ALT key-up is inert for the receiving app but registers this
        // process as the last input source, which unlocks SetForegroundWindow.
        let alt_up = key_input(KEY_ALT, KEYEVENTF_KEYUP);
        SendInput(&[alt_up], size_of::<INPUT>() as i32);
        let _ = SetForegroundWindow(hwnd);
        GetForegroundWindow() == hwnd
    }
}

fn post_key_tap(hwnd: HWND, vk: u16, exe: &str, hold_ms: u64) -> Result<(), KeepaliveError> {
    unsafe {
        PostMessageW(
            Some(hwnd),
            WM_KEYDOWN,
            WPARAM(vk as usize),
            key_lparam(vk, false),
        )
        .map_err(|_| KeepaliveError::admin_hint(exe))?;
    }
    thread::sleep(Duration::from_millis(hold_ms));
    unsafe {
        PostMessageW(
            Some(hwnd),
            WM_KEYUP,
            WPARAM(vk as usize),
            key_lparam(vk, true),
        )
        .map_err(|_| KeepaliveError::admin_hint(exe))
    }
}

fn post_mouse_move(hwnd: HWND, x: i16, y: i16, exe: &str) -> Result<(), KeepaliveError> {
    unsafe {
        PostMessageW(
            Some(hwnd),
            WM_MOUSEMOVE,
            WPARAM(0),
            packed_mouse_lparam(x, y),
        )
        .map_err(|_| KeepaliveError::admin_hint(exe))
    }
}

fn post_mouse_wheel(hwnd: HWND, delta: i16, exe: &str) -> Result<(), KeepaliveError> {
    let wparam = (delta as u16 as usize) << 16;
    unsafe {
        PostMessageW(Some(hwnd), WM_MOUSEWHEEL, WPARAM(wparam), LPARAM(0))
            .map_err(|_| KeepaliveError::admin_hint(exe))
    }
}

fn send_input_action(
    action: &ResolvedAction,
    exe: &str,
    hold_ms: u64,
) -> Result<(), KeepaliveError> {
    match action {
        ResolvedAction::SpaceTap => send_key_tap(KEY_SPACE, exe, hold_ms),
        ResolvedAction::WTap => send_key_tap(KEY_W, exe, hold_ms),
        ResolvedAction::CameraNudge => {
            send_single_input(&mouse_input(2, 0), exe)?;
            thread::sleep(Duration::from_millis(MOUSE_STEP_MS));
            send_single_input(&mouse_input(-2, 0), exe)
        }
        ResolvedAction::MouseWiggle => {
            send_single_input(&mouse_input(1, 1), exe)?;
            thread::sleep(Duration::from_millis(MOUSE_STEP_MS));
            send_single_input(&mouse_input(-1, -1), exe)
        }
        ResolvedAction::ScrollTick => {
            send_single_input(&wheel_input(WHEEL_DELTA as i32), exe)?;
            thread::sleep(Duration::from_millis(MOUSE_STEP_MS));
            send_single_input(&wheel_input(-(WHEEL_DELTA as i32)), exe)
        }
        ResolvedAction::RightClick => {
            send_single_input(&mouse_button_input(MOUSEEVENTF_RIGHTDOWN), exe)?;
            thread::sleep(Duration::from_millis(hold_ms));
            send_single_input(&mouse_button_input(MOUSEEVENTF_RIGHTUP), exe)
        }
        ResolvedAction::GamepadNudge => crate::gamepad_send::nudge().map_err(KeepaliveError::other),
        ResolvedAction::KeySequence(keys) => {
            for (index, key) in keys.iter().enumerate() {
                let vk = key_name_to_vk(key).ok_or_else(|| KeepaliveError::other(format!(
                    "Couldn't send key sequence - record supported keys in Settings to fix this: unsupported key '{key}'."
                )))?;
                send_key_tap(vk, exe, hold_ms)?;
                if index + 1 < keys.len() {
                    thread::sleep(Duration::from_millis(MOUSE_STEP_MS));
                }
            }
            Ok(())
        }
    }
}

fn send_key_tap(vk: u16, exe: &str, hold_ms: u64) -> Result<(), KeepaliveError> {
    send_single_input(&key_input(vk, KEYBD_EVENT_FLAGS(0)), exe)?;
    thread::sleep(Duration::from_millis(hold_ms));
    send_single_input(&key_input(vk, KEYEVENTF_KEYUP), exe)
}

fn send_single_input(input: &INPUT, exe: &str) -> Result<(), KeepaliveError> {
    let sent = unsafe { SendInput(std::slice::from_ref(input), size_of::<INPUT>() as i32) };
    if sent == 1 {
        Ok(())
    } else {
        Err(KeepaliveError::admin_hint(exe))
    }
}

fn key_input(vk: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: scan_code(vk),
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn mouse_input(x: i32, y: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: x,
                dy: y,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn wheel_input(delta: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: delta as u32,
                dwFlags: MOUSEEVENTF_WHEEL,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn mouse_button_input(
    flags: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS,
) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn key_lparam(vk: u16, key_up: bool) -> LPARAM {
    let scan = scan_code(vk) as isize;
    let mut value = 1 | (scan << 16);
    if key_up {
        value |= 1 << 30;
        value |= 1 << 31;
    }
    LPARAM(value)
}

fn scan_code(vk: u16) -> u16 {
    unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) as u16 }
}

fn packed_mouse_lparam(x: i16, y: i16) -> LPARAM {
    let lo = x as u16 as u32;
    let hi = y as u16 as u32;
    LPARAM(((hi << 16) | lo) as isize)
}

fn hwnd_from_isize(hwnd: isize) -> HWND {
    HWND(hwnd as *mut c_void)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{rngs::StdRng, SeedableRng};

    struct FakeActivity {
        foreground: Option<isize>,
        last_input_at: Option<Instant>,
        input_tick: Option<u32>,
    }

    impl ActivityProbe for FakeActivity {
        fn foreground_window(&self) -> Option<isize> {
            self.foreground
        }

        fn last_input_age(&self, now: Instant) -> Option<Duration> {
            self.last_input_at.map(|last| now.duration_since(last))
        }

        fn last_input_tick(&self) -> Option<u32> {
            self.input_tick
        }
    }

    fn options() -> KeepaliveOptions {
        KeepaliveOptions {
            interval: 540,
            randomize: true,
            jitter_pct: 15,
            action: ResolvedAction::SpaceTap,
            send_without_focus: false,
            hold_while_playing: true,
            hold_window_secs: 60,
        }
    }

    fn target() -> KeepaliveTarget {
        KeepaliveTarget {
            hwnd: 77,
            exe: "game.exe".to_string(),
            wclass: "CLASS".to_string(),
        }
    }

    fn mouse(input: &INPUT) -> MOUSEINPUT {
        assert_eq!(input.r#type, INPUT_MOUSE);
        unsafe { input.Anonymous.mi }
    }

    fn key(input: &INPUT) -> KEYBDINPUT {
        assert_eq!(input.r#type, INPUT_KEYBOARD);
        unsafe { input.Anonymous.ki }
    }

    #[test]
    fn warmup_caps_early_interval_then_relaxes() {
        // Long interval is shortened during warm-up, never lengthened.
        assert_eq!(warmup_interval(540, 0, true), WARMUP_MAX_INTERVAL);
        assert_eq!(
            warmup_interval(540, WARMUP_TICKS - 1, true),
            WARMUP_MAX_INTERVAL
        );
        assert_eq!(warmup_interval(540, WARMUP_TICKS, true), 540);
        // A short interval is left alone.
        assert_eq!(warmup_interval(30, 0, true), 30);
        // Disabled passes through unchanged.
        assert_eq!(warmup_interval(540, 0, false), 540);
    }

    #[test]
    fn jitter_stays_inside_configured_bounds() {
        let mut rng = StdRng::seed_from_u64(7);

        for _ in 0..200 {
            let delay = next_delay(540, true, 15, &mut rng).as_secs();
            assert!((459..=621).contains(&delay), "{delay}");
        }
        for _ in 0..200 {
            let delay = next_delay(540, true, 30, &mut rng).as_secs();
            assert!((378..=702).contains(&delay), "{delay}");
        }

        assert_eq!(next_delay(540, false, 15, &mut rng).as_secs(), 540);
        assert_eq!(next_delay(540, true, 0, &mut rng).as_secs(), 540);
    }

    #[test]
    fn timer_uses_injected_clock_for_due_state() {
        let now = Instant::now();
        let mut rng = StdRng::seed_from_u64(3);
        let mut options = options();
        options.randomize = false;

        let mut timer = TickTimer::new(now, &options, &mut rng);
        assert_eq!(timer.seconds_until(now), 540);
        assert!(!timer.due(now + Duration::from_secs(539)));
        assert!(timer.due(now + Duration::from_secs(540)));

        timer.reschedule(now + Duration::from_secs(540), &options, &mut rng);
        assert_eq!(timer.seconds_until(now + Duration::from_secs(540)), 540);
    }

    #[test]
    fn delivery_strategy_prefers_direct_input_when_target_is_foreground() {
        assert_eq!(
            delivery_strategy(42, Some(42)),
            DeliveryStrategy::DirectSendInput
        );
        assert_eq!(
            delivery_strategy(42, Some(99)),
            DeliveryStrategy::FocusFlick
        );
        assert_eq!(delivery_strategy(42, None), DeliveryStrategy::FocusFlick);
    }

    #[test]
    fn mouse_input_builds_relative_camera_and_wiggle_steps() {
        let right = mouse(&mouse_input(2, 0));
        assert_eq!(right.dx, 2);
        assert_eq!(right.dy, 0);
        assert_eq!(right.mouseData, 0);
        assert_eq!(right.dwFlags, MOUSEEVENTF_MOVE);

        let left = mouse(&mouse_input(-2, 0));
        assert_eq!(left.dx, -2);
        assert_eq!(left.dy, 0);
        assert_eq!(left.dwFlags, MOUSEEVENTF_MOVE);

        let down_right = mouse(&mouse_input(1, 1));
        assert_eq!(down_right.dx, 1);
        assert_eq!(down_right.dy, 1);
        assert_eq!(down_right.dwFlags, MOUSEEVENTF_MOVE);

        let up_left = mouse(&mouse_input(-1, -1));
        assert_eq!(up_left.dx, -1);
        assert_eq!(up_left.dy, -1);
        assert_eq!(up_left.dwFlags, MOUSEEVENTF_MOVE);
    }

    #[test]
    fn wheel_and_right_click_inputs_use_expected_flags() {
        let wheel_up = mouse(&wheel_input(WHEEL_DELTA as i32));
        assert_eq!(wheel_up.dx, 0);
        assert_eq!(wheel_up.dy, 0);
        assert_eq!(wheel_up.mouseData, WHEEL_DELTA);
        assert_eq!(wheel_up.dwFlags, MOUSEEVENTF_WHEEL);

        let wheel_down = mouse(&wheel_input(-(WHEEL_DELTA as i32)));
        assert_eq!(wheel_down.mouseData, (-(WHEEL_DELTA as i32)) as u32);
        assert_eq!(wheel_down.dwFlags, MOUSEEVENTF_WHEEL);

        let right_down = mouse(&mouse_button_input(MOUSEEVENTF_RIGHTDOWN));
        assert_eq!(right_down.dwFlags, MOUSEEVENTF_RIGHTDOWN);
        assert_eq!(right_down.mouseData, 0);

        let right_up = mouse(&mouse_button_input(MOUSEEVENTF_RIGHTUP));
        assert_eq!(right_up.dwFlags, MOUSEEVENTF_RIGHTUP);
        assert_eq!(right_up.mouseData, 0);
    }

    #[test]
    fn key_inputs_press_and_release_expected_virtual_keys() {
        let down = key(&key_input(KEY_W, KEYBD_EVENT_FLAGS(0)));
        assert_eq!(down.wVk, VIRTUAL_KEY(KEY_W));
        assert_eq!(down.dwFlags, KEYBD_EVENT_FLAGS(0));
        assert_ne!(down.wScan, 0);

        let up = key(&key_input(KEY_SPACE, KEYEVENTF_KEYUP));
        assert_eq!(up.wVk, VIRTUAL_KEY(KEY_SPACE));
        assert_eq!(up.dwFlags, KEYEVENTF_KEYUP);
        assert_ne!(up.wScan, 0);
    }

    #[test]
    fn hold_skip_logic_uses_recent_input_conservatively() {
        let now = Instant::now();
        let target = target();
        let options = options();

        let recent = FakeActivity {
            foreground: Some(target.hwnd),
            last_input_at: Some(now - Duration::from_secs(30)),
            input_tick: Some(1000),
        };
        assert!(should_hold(&target, &options, now, &recent, None));

        let stale = FakeActivity {
            foreground: Some(target.hwnd),
            last_input_at: Some(now - Duration::from_secs(61)),
            input_tick: Some(1000),
        };
        assert!(!should_hold(&target, &options, now, &stale, None));

        let other_window = FakeActivity {
            foreground: Some(11),
            last_input_at: Some(now - Duration::from_secs(10)),
            input_tick: Some(2000),
        };
        // Input in another app must NOT hold a background game's keepalives.
        assert!(!should_hold(&target, &options, now, &other_window, None));

        // OMNAFK's own injection tick must not trigger hold.
        let self_injected = FakeActivity {
            foreground: Some(target.hwnd),
            last_input_at: Some(now - Duration::from_millis(500)),
            input_tick: Some(4242),
        };
        assert!(!should_hold(
            &target,
            &options,
            now,
            &self_injected,
            Some(4242)
        ));

        // Real user input after OMNAFK send uses a newer tick.
        let user_after_inject = FakeActivity {
            foreground: Some(target.hwnd),
            last_input_at: Some(now - Duration::from_millis(200)),
            input_tick: Some(5000),
        };
        assert!(should_hold(
            &target,
            &options,
            now,
            &user_after_inject,
            Some(4242)
        ));
    }

    #[test]
    fn background_tick_defers_briefly_while_user_is_mid_input() {
        let now = Instant::now();
        let target = target();
        let mut options = options();
        options.randomize = false;
        options.interval = 1;
        let mut rng = StdRng::seed_from_u64(1);
        let timer = TickTimer::new(now, &options, &mut rng);
        let due = now + Duration::from_secs(2);

        // Typing in another app right now: wait for a quiet moment.
        let typing_elsewhere = FakeActivity {
            foreground: Some(11),
            last_input_at: Some(due - Duration::from_secs(1)),
            input_tick: Some(2000),
        };
        assert_eq!(
            tick_decision(&timer, &target, &options, due, &typing_elsewhere, None),
            TickDecision::DeferBriefly
        );

        // Quiet gap reached: send via focus flick.
        let quiet = FakeActivity {
            foreground: Some(11),
            last_input_at: Some(due - Duration::from_secs(10)),
            input_tick: Some(2000),
        };
        assert_eq!(
            tick_decision(&timer, &target, &options, due, &quiet, None),
            TickDecision::Send
        );

        // OMNAFK's own injected input never defers the next tick.
        let self_injected = FakeActivity {
            foreground: Some(11),
            last_input_at: Some(due - Duration::from_millis(500)),
            input_tick: Some(4242),
        };
        assert_eq!(
            tick_decision(&timer, &target, &options, due, &self_injected, Some(4242)),
            TickDecision::Send
        );

        // Background-only mode never touches focus, so no defer is needed.
        options.send_without_focus = true;
        assert_eq!(
            tick_decision(&timer, &target, &options, due, &typing_elsewhere, None),
            TickDecision::Send
        );
    }

    #[test]
    fn timer_defer_pushes_due_time_back_briefly() {
        let now = Instant::now();
        let mut options = options();
        options.randomize = false;
        let mut rng = StdRng::seed_from_u64(1);
        let mut timer = TickTimer::new(now, &options, &mut rng);

        let due = now + Duration::from_secs(540);
        assert!(timer.due(due));
        timer.defer(due, Duration::from_secs(POLITE_DEFER_SECS));
        assert!(!timer.due(due));
        assert!(timer.due(due + Duration::from_secs(POLITE_DEFER_SECS)));
    }
}
