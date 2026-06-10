use crate::config::{AppConfig, KeepaliveAction};
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
    System::SystemInformation::GetTickCount64,
    UI::{
        Input::KeyboardAndMouse::{
            GetLastInputInfo, MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD,
            INPUT_MOUSE, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, LASTINPUTINFO,
            MAPVK_VK_TO_VSC, MOUSEINPUT, MOUSEEVENTF_MOVE, VIRTUAL_KEY,
        },
        WindowsAndMessaging::{
            GetForegroundWindow, PostMessageW, SetForegroundWindow, WM_KEYDOWN, WM_KEYUP,
            WM_MOUSEMOVE,
        },
    },
};

const HOLD_RECENT_INPUT: Duration = Duration::from_secs(60);
const KEY_SPACE: u16 = 0x20;
const KEY_W: u16 = 0x57;

#[derive(Debug, Clone)]
pub struct KeepaliveOptions {
    pub interval: u64,
    pub randomize: bool,
    pub action: KeepaliveAction,
    pub send_without_focus: bool,
    pub hold_while_playing: bool,
}

impl From<&AppConfig> for KeepaliveOptions {
    fn from(config: &AppConfig) -> Self {
        Self {
            interval: config.interval,
            randomize: config.randomize,
            action: config.action,
            send_without_focus: config.send_without_focus,
            hold_while_playing: config.hold_while_playing,
        }
    }
}

#[derive(Debug, Clone)]
pub struct KeepaliveTarget {
    pub hwnd: isize,
    pub exe: String,
}

#[derive(Debug, Clone)]
pub struct TickTimer {
    next_due: Instant,
}

impl TickTimer {
    pub fn new(now: Instant, options: &KeepaliveOptions, rng: &mut impl Rng) -> Self {
        Self {
            next_due: now + next_delay(options.interval, options.randomize, rng),
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
        self.next_due = now + next_delay(options.interval, options.randomize, rng);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickDecision {
    Waiting,
    Held,
    Send,
}

pub trait ActivityProbe {
    fn foreground_window(&self) -> Option<isize>;
    fn last_input_age(&self, now: Instant) -> Option<Duration>;
}

#[derive(Debug, Default)]
pub struct Win32ActivityProbe;

impl ActivityProbe for Win32ActivityProbe {
    fn foreground_window(&self) -> Option<isize> {
        let hwnd = unsafe { GetForegroundWindow() };
        (!hwnd.is_invalid()).then_some(hwnd.0 as isize)
    }

    fn last_input_age(&self, _now: Instant) -> Option<Duration> {
        let mut info = LASTINPUTINFO {
            cbSize: size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };

        let ok = unsafe { GetLastInputInfo(&mut info).as_bool() };
        if !ok {
            return None;
        }

        let now = unsafe { GetTickCount64() };
        let elapsed = now.saturating_sub(info.dwTime as u64);
        Some(Duration::from_millis(elapsed))
    }
}

pub fn tick_decision(
    timer: &TickTimer,
    target: &KeepaliveTarget,
    options: &KeepaliveOptions,
    now: Instant,
    activity: &dyn ActivityProbe,
) -> TickDecision {
    if !timer.due(now) {
        return TickDecision::Waiting;
    }

    if should_hold(target, options, now, activity) {
        TickDecision::Held
    } else {
        TickDecision::Send
    }
}

pub fn should_hold(
    target: &KeepaliveTarget,
    options: &KeepaliveOptions,
    now: Instant,
    activity: &dyn ActivityProbe,
) -> bool {
    options.hold_while_playing
        && activity.foreground_window() == Some(target.hwnd)
        && activity
            .last_input_age(now)
            .is_some_and(|age| age <= HOLD_RECENT_INPUT)
}

pub fn next_delay(interval_secs: u64, randomize: bool, rng: &mut impl Rng) -> Duration {
    if !randomize {
        return Duration::from_secs(interval_secs);
    }

    let spread = interval_secs as f64 * 0.15;
    let jitter = rng.gen_range(-spread..=spread);
    let jittered = (interval_secs as f64 + jitter).round().max(1.0) as u64;
    Duration::from_secs(jittered)
}

pub fn send_keepalive(target: &KeepaliveTarget, options: &KeepaliveOptions) -> Result<(), KeepaliveError> {
    if options.send_without_focus {
        post_action(target, options.action)
    } else {
        focus_flick_action(target, options.action)
    }
}

#[derive(Debug, Clone)]
pub struct KeepaliveError {
    message: String,
}

impl KeepaliveError {
    fn admin_hint(exe: &str) -> Self {
        Self {
            message: format!(
                "Couldn't send input to {exe} - it may be running as administrator. Restart OMNAFK as administrator to fix this."
            ),
        }
    }
}

impl fmt::Display for KeepaliveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for KeepaliveError {}

fn post_action(target: &KeepaliveTarget, action: KeepaliveAction) -> Result<(), KeepaliveError> {
    let hwnd = hwnd_from_isize(target.hwnd);
    match action {
        KeepaliveAction::SpaceTap => post_key_tap(hwnd, KEY_SPACE, &target.exe),
        KeepaliveAction::WTap => post_key_tap(hwnd, KEY_W, &target.exe),
        KeepaliveAction::CameraNudge => {
            post_mouse_move(hwnd, 2, 0, &target.exe)?;
            thread::sleep(Duration::from_millis(25));
            post_mouse_move(hwnd, -2, 0, &target.exe)
        }
    }
}

fn focus_flick_action(target: &KeepaliveTarget, action: KeepaliveAction) -> Result<(), KeepaliveError> {
    let target_hwnd = hwnd_from_isize(target.hwnd);
    let previous = unsafe { GetForegroundWindow() };

    if previous != target_hwnd && !unsafe { SetForegroundWindow(target_hwnd).as_bool() } {
        return Err(KeepaliveError::admin_hint(&target.exe));
    }

    thread::sleep(Duration::from_millis(50));
    let result = send_input_action(action, &target.exe);

    if !previous.is_invalid() && previous != target_hwnd {
        unsafe {
            let _ = SetForegroundWindow(previous);
        }
    }

    result
}

fn post_key_tap(hwnd: HWND, vk: u16, exe: &str) -> Result<(), KeepaliveError> {
    unsafe {
        PostMessageW(
            Some(hwnd),
            WM_KEYDOWN,
            WPARAM(vk as usize),
            key_lparam(vk, false),
        )
        .map_err(|_| KeepaliveError::admin_hint(exe))?;
    }
    thread::sleep(Duration::from_millis(25));
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

fn send_input_action(action: KeepaliveAction, exe: &str) -> Result<(), KeepaliveError> {
    let inputs = match action {
        KeepaliveAction::SpaceTap => key_inputs(KEY_SPACE),
        KeepaliveAction::WTap => key_inputs(KEY_W),
        KeepaliveAction::CameraNudge => vec![mouse_input(2, 0), mouse_input(-2, 0)],
    };

    let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
    if sent == inputs.len() as u32 {
        Ok(())
    } else {
        Err(KeepaliveError::admin_hint(exe))
    }
}

fn key_inputs(vk: u16) -> Vec<INPUT> {
    vec![key_input(vk, KEYBD_EVENT_FLAGS(0)), key_input(vk, KEYEVENTF_KEYUP)]
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
    }

    impl ActivityProbe for FakeActivity {
        fn foreground_window(&self) -> Option<isize> {
            self.foreground
        }

        fn last_input_age(&self, now: Instant) -> Option<Duration> {
            self.last_input_at.map(|last| now.duration_since(last))
        }
    }

    fn options() -> KeepaliveOptions {
        KeepaliveOptions {
            interval: 540,
            randomize: true,
            action: KeepaliveAction::SpaceTap,
            send_without_focus: true,
            hold_while_playing: true,
        }
    }

    fn target() -> KeepaliveTarget {
        KeepaliveTarget {
            hwnd: 77,
            exe: "game.exe".to_string(),
        }
    }

    #[test]
    fn jitter_stays_inside_fifteen_percent_bounds() {
        let mut rng = StdRng::seed_from_u64(7);

        for _ in 0..200 {
            let delay = next_delay(540, true, &mut rng).as_secs();
            assert!((459..=621).contains(&delay), "{delay}");
        }

        assert_eq!(next_delay(540, false, &mut rng).as_secs(), 540);
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
    fn hold_skip_logic_uses_foreground_and_recent_input() {
        let now = Instant::now();
        let target = target();
        let options = options();

        let recent = FakeActivity {
            foreground: Some(target.hwnd),
            last_input_at: Some(now - Duration::from_secs(30)),
        };
        assert!(should_hold(&target, &options, now, &recent));

        let stale = FakeActivity {
            foreground: Some(target.hwnd),
            last_input_at: Some(now - Duration::from_secs(61)),
        };
        assert!(!should_hold(&target, &options, now, &stale));

        let other_window = FakeActivity {
            foreground: Some(11),
            last_input_at: Some(now - Duration::from_secs(10)),
        };
        assert!(!should_hold(&target, &options, now, &other_window));
    }
}
