use crate::config::Sensitivity;
use serde::Serialize;
use std::{
    mem::{size_of, MaybeUninit},
    path::Path,
};
use windows::{
    core::{BOOL, PWSTR},
    Win32::{
        Foundation::{CloseHandle, HWND, LPARAM, RECT},
        Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        },
        Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY},
        System::{
            ProcessStatus::{K32EnumProcessModulesEx, K32GetModuleBaseNameW, LIST_MODULES_ALL},
            Threading::{
                GetCurrentProcess, OpenProcess, OpenProcessToken, QueryFullProcessImageNameW,
                PROCESS_NAME_FORMAT, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
                PROCESS_VM_READ,
            },
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GetClassNameW, GetWindow, GetWindowLongW, GetWindowRect,
            GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
            GWL_EXSTYLE, GW_OWNER, WS_EX_TOOLWINDOW,
        },
    },
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WindowFacts {
    pub title: String,
    pub exe: String,
    pub wclass: String,
    pub pid: u32,
    pub fullscreen: bool,
    pub borderless: bool,
    pub gfx_dll: bool,
    pub platform_path: bool,
    pub known_game: bool,
    pub negative_class: bool,
    pub gpu_active: bool,
    /// The target process runs elevated (None when we couldn't query it).
    pub elevated: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Game,
    Ignored,
}

#[derive(Debug, Clone)]
pub struct DetectedWindow {
    pub hwnd: isize,
    pub facts: WindowFacts,
    pub score: i32,
    pub verdict: Verdict,
}

pub trait GpuUsageProbe: Send + Sync {
    fn usage_for_pid(&self, pid: u32) -> Option<f32>;
}

#[derive(Debug, Default)]
pub struct NoGpuUsageProbe;

impl GpuUsageProbe for NoGpuUsageProbe {
    fn usage_for_pid(&self, _pid: u32) -> Option<f32> {
        // v1 does not wire PDH GPU Engine counters yet; the trait keeps the detector API ready.
        None
    }
}

pub fn scan_windows(
    sensitivity: Sensitivity,
    gpu: &dyn GpuUsageProbe,
    always_mark_exes: &[String],
    supplement: Option<&crate::community::DetectionSupplement>,
) -> Vec<DetectedWindow> {
    let mut hwnds = Vec::new();
    unsafe {
        let _ = EnumWindows(
            Some(enum_window),
            LPARAM((&mut hwnds as *mut Vec<HWND>) as isize),
        );
    }

    hwnds
        .into_iter()
        .filter_map(|hwnd| gather_window_facts(hwnd, gpu, always_mark_exes, supplement))
        .map(|seed| {
            let score = score(&seed.facts);
            let verdict = verdict_for_score(score, sensitivity);
            DetectedWindow {
                hwnd: seed.hwnd,
                facts: seed.facts,
                score,
                verdict,
            }
        })
        .collect()
}

pub fn score(facts: &WindowFacts) -> i32 {
    let mut score = 0;

    if facts.fullscreen {
        score += 45;
    }
    if facts.borderless {
        score += 35;
    }
    if facts.gfx_dll {
        score += 35;
    }
    if facts.platform_path {
        score += 55;
    }
    if facts.known_game {
        score += 80;
    }
    if facts.gpu_active {
        score += 20;
    }
    if facts.negative_class {
        score -= 60;
    }

    score
}

pub fn verdict(facts: &WindowFacts, sensitivity: Sensitivity) -> Verdict {
    verdict_for_score(score(facts), sensitivity)
}

pub fn threshold(sensitivity: Sensitivity) -> i32 {
    match sensitivity {
        Sensitivity::Strict => 80,
        Sensitivity::Standard => 55,
        Sensitivity::Broad => 30,
    }
}

pub fn verdict_for_score(score: i32, sensitivity: Sensitivity) -> Verdict {
    if score >= threshold(sensitivity) {
        Verdict::Game
    } else {
        Verdict::Ignored
    }
}

unsafe extern "system" fn enum_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    if !is_candidate_window(hwnd) {
        return true.into();
    }

    let hwnds = &mut *(lparam.0 as *mut Vec<HWND>);
    hwnds.push(hwnd);
    true.into()
}

unsafe fn is_candidate_window(hwnd: HWND) -> bool {
    if !IsWindowVisible(hwnd).as_bool() {
        return false;
    }

    // GetWindow returns Err when the window has no owner (NULL result),
    // which is the normal case for top-level app/game windows.
    if let Ok(owner) = GetWindow(hwnd, GW_OWNER) {
        if !owner.is_invalid() {
            return false;
        }
    }

    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    if ex_style & WS_EX_TOOLWINDOW.0 != 0 {
        return false;
    }

    true
}

fn gather_window_facts(
    hwnd: HWND,
    gpu: &dyn GpuUsageProbe,
    always_mark_exes: &[String],
    supplement: Option<&crate::community::DetectionSupplement>,
) -> Option<DetectedWindowSeed> {
    let raw_title = window_text(hwnd);
    let wclass = window_class(hwnd)?;
    let pid = window_pid(hwnd)?;
    let path = process_path(pid);
    let exe = path
        .as_deref()
        .and_then(|path| Path::new(path).file_name())
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| format!("pid-{pid}"));
    let platform_path = path.as_deref().map(is_game_platform_path).unwrap_or(false);
    let (fullscreen, borderless) = window_screen_coverage(hwnd).unwrap_or((false, false));
    let known_game = is_known_game_window(&raw_title, &exe, &wclass, path.as_deref(), supplement)
        || always_mark_exes
            .iter()
            .any(|entry| entry.trim().eq_ignore_ascii_case(&exe));

    if raw_title.is_empty() && !platform_path && !known_game {
        return None;
    }

    let modules = process_modules(pid);
    let gfx_dll = modules.iter().any(|module| is_graphics_module(module));
    let negative_class = !known_game && is_negative_window(&exe, &wclass, supplement);
    let gpu_active = gpu.usage_for_pid(pid).is_some();
    let title = if raw_title.is_empty() {
        fallback_title(&exe, path.as_deref())
    } else {
        raw_title
    };

    Some(DetectedWindowSeed {
        hwnd: hwnd.0 as isize,
        facts: WindowFacts {
            title,
            exe,
            wclass,
            pid,
            fullscreen,
            borderless,
            gfx_dll,
            platform_path,
            known_game,
            negative_class,
            gpu_active,
            elevated: process_elevated(pid),
        },
    })
}

/// Whether the process behind `pid` runs with an elevated token.
fn process_elevated(pid: u32) -> Option<bool> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()? };
    let result = token_elevated(handle);
    unsafe {
        let _ = CloseHandle(handle);
    }
    result
}

/// Whether OMNAFK itself runs elevated.
pub fn current_process_elevated() -> bool {
    token_elevated(unsafe { GetCurrentProcess() }).unwrap_or(false)
}

fn token_elevated(process: windows::Win32::Foundation::HANDLE) -> Option<bool> {
    let mut token = windows::Win32::Foundation::HANDLE::default();
    unsafe {
        OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
    }
    let mut elevation = TOKEN_ELEVATION::default();
    let mut len = 0u32;
    let queried = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut std::ffi::c_void),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
    };
    unsafe {
        let _ = CloseHandle(token);
    }
    queried.ok()?;
    Some(elevation.TokenIsElevated != 0)
}

#[derive(Debug, Clone)]
struct DetectedWindowSeed {
    hwnd: isize,
    facts: WindowFacts,
}

fn window_text(hwnd: HWND) -> String {
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    if len <= 0 {
        return String::new();
    }

    let mut buf = vec![0; len as usize + 1];
    let read = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if read <= 0 {
        return String::new();
    }

    String::from_utf16_lossy(&buf[..read as usize])
        .trim()
        .to_string()
}

fn fallback_title(exe: &str, path: Option<&str>) -> String {
    path.and_then(|path| Path::new(path).file_stem())
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| exe.to_string())
}

fn window_class(hwnd: HWND) -> Option<String> {
    let mut buf = vec![0; 256];
    let read = unsafe { GetClassNameW(hwnd, &mut buf) };
    if read <= 0 {
        return None;
    }

    Some(String::from_utf16_lossy(&buf[..read as usize]))
}

fn window_pid(hwnd: HWND) -> Option<u32> {
    let mut pid = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    (pid != 0).then_some(pid)
}

fn process_path(pid: u32) -> Option<String> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()? };
    let mut buf = vec![0; 32_768];
    let mut len = buf.len() as u32;
    let result = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_FORMAT(0),
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    };
    unsafe {
        let _ = CloseHandle(handle);
    }

    result
        .ok()
        .map(|_| String::from_utf16_lossy(&buf[..len as usize]))
}

fn process_modules(pid: u32) -> Vec<String> {
    let handle = unsafe {
        match OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid) {
            Ok(handle) => handle,
            Err(_) => return Vec::new(),
        }
    };

    let mut needed = 0;
    unsafe {
        let _ = K32EnumProcessModulesEx(
            handle,
            std::ptr::null_mut(),
            0,
            &mut needed,
            LIST_MODULES_ALL.0,
        );
    }

    if needed == 0 {
        unsafe {
            let _ = CloseHandle(handle);
        }
        return Vec::new();
    }

    let count = needed as usize / size_of::<windows::Win32::Foundation::HMODULE>();
    let mut modules = vec![windows::Win32::Foundation::HMODULE::default(); count];
    let ok = unsafe {
        K32EnumProcessModulesEx(
            handle,
            modules.as_mut_ptr(),
            (modules.len() * size_of::<windows::Win32::Foundation::HMODULE>()) as u32,
            &mut needed,
            LIST_MODULES_ALL.0,
        )
        .as_bool()
    };

    let names = if ok {
        modules
            .into_iter()
            .filter_map(|module| module_name(handle, module))
            .collect()
    } else {
        Vec::new()
    };

    unsafe {
        let _ = CloseHandle(handle);
    }
    names
}

fn module_name(
    handle: windows::Win32::Foundation::HANDLE,
    module: windows::Win32::Foundation::HMODULE,
) -> Option<String> {
    let mut buf = vec![0; 260];
    let read = unsafe { K32GetModuleBaseNameW(handle, Some(module), &mut buf) };
    if read == 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..read as usize]))
}

fn window_screen_coverage(hwnd: HWND) -> Option<(bool, bool)> {
    let mut rect = RECT::default();
    unsafe {
        GetWindowRect(hwnd, &mut rect).ok()?;
    }

    let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_invalid() {
        return None;
    }

    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..unsafe { MaybeUninit::zeroed().assume_init() }
    };
    let ok = unsafe { GetMonitorInfoW(monitor, &mut info).as_bool() };
    if !ok {
        return None;
    }

    let fullscreen = rect_covers(rect, info.rcMonitor);
    let borderless = !fullscreen && rect_covers(rect, info.rcWork);
    Some((fullscreen, borderless))
}

fn rect_covers(window: RECT, target: RECT) -> bool {
    const TOLERANCE: i32 = 2;
    window.left <= target.left + TOLERANCE
        && window.top <= target.top + TOLERANCE
        && window.right >= target.right - TOLERANCE
        && window.bottom >= target.bottom - TOLERANCE
}

fn is_graphics_module(module: &str) -> bool {
    matches!(
        module.to_ascii_lowercase().as_str(),
        "d3d9.dll"
            | "d3d10.dll"
            | "d3d11.dll"
            | "d3d12.dll"
            | "dxgi.dll"
            | "vulkan-1.dll"
            | "opengl32.dll"
            | "xinput1_4.dll"
    )
}

fn is_game_platform_path(path: &str) -> bool {
    let path = path.to_ascii_lowercase();
    [
        r"\steamapps\common\",
        r"\epic games\",
        r"\gog games\",
        r"\riot games\",
        r"\xboxgames\",
        r"\windowsapps\",
        r"\roblox\",
        r"\bloxstrap\",
        r"\battle.net\",
        r"\ea games\",
        r"\gog galaxy\",
        r"\ubisoft game launcher\games\",
        r"\ubisoft connect\games\",
        r"\itch\apps\",
    ]
    .iter()
    .any(|needle| path.contains(needle))
}

fn is_known_game_window(
    title: &str,
    exe: &str,
    wclass: &str,
    path: Option<&str>,
    supplement: Option<&crate::community::DetectionSupplement>,
) -> bool {
    let title = title.to_ascii_lowercase();
    let exe = exe.to_ascii_lowercase();
    let wclass = wclass.to_ascii_lowercase();
    let path = path.unwrap_or_default().to_ascii_lowercase();

    if let Some(sup) = supplement {
        if sup.known_exes.contains(&exe) {
            return true;
        }
        if sup.path_patterns.iter().any(|needle| path.contains(needle)) {
            return true;
        }
    }

    if exe == "robloxplayerbeta.exe" || exe == "robloxplayer.exe" {
        return true;
    }

    const KNOWN_EXES: &[&str] = &[
        "javaw.exe",
        "minecraft.windows.exe",
        "minecraftlauncher.exe",
        "fortniteclient-win64-shipping.exe",
        "valorant-win64-shipping.exe",
        "cs2.exe",
        "csgo.exe",
        "gta5.exe",
        "eldenring.exe",
        "darksoulsiii.exe",
        "rocketleague.exe",
        "amongus.exe",
        "fallguys_client_game.exe",
        "destiny2.exe",
        "overwatch.exe",
    ];
    if KNOWN_EXES.contains(&exe.as_str()) {
        return true;
    }

    if exe == "windows10universal.exe"
        && (title.contains("roblox") || path.contains("roblox") || wclass.contains("roblox"))
    {
        return true;
    }

    path.contains(r"\roblox\")
        || path.contains(r"\bloxstrap\")
        || (wclass == "windowsclient"
            && (title.contains("roblox") || exe.contains("roblox") || path.contains("roblox")))
        || wclass.contains("roblox")
}

fn is_negative_window(
    exe: &str,
    wclass: &str,
    supplement: Option<&crate::community::DetectionSupplement>,
) -> bool {
    let exe = exe.to_ascii_lowercase();
    let wclass = wclass.to_ascii_lowercase();

    if let Some(sup) = supplement {
        if sup.negative_exes.contains(&exe) {
            return true;
        }
        if sup
            .negative_classes
            .iter()
            .any(|needle| wclass.contains(needle))
        {
            return true;
        }
    }

    let negative_exe = [
        "chrome.exe",
        "msedge.exe",
        "firefox.exe",
        "code.exe",
        "devenv.exe",
        "winword.exe",
        "excel.exe",
        "powerpnt.exe",
        "vlc.exe",
        "wmplayer.exe",
    ];
    let negative_class = [
        "chrome_widgetwin",
        "mozillawindowclass",
        "applicationframewindow",
        "cabinetwclass",
        "opusapp",
        "xlmain",
        "pptframeclass",
        "media player",
        "sunawtframe",
        "qt",
    ];

    negative_exe.iter().any(|name| exe == *name)
        || negative_class.iter().any(|needle| wclass.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> WindowFacts {
        WindowFacts {
            title: "Window".to_string(),
            exe: "game.exe".to_string(),
            wclass: "GameWindow".to_string(),
            pid: 42,
            fullscreen: false,
            borderless: false,
            gfx_dll: false,
            platform_path: false,
            known_game: false,
            negative_class: false,
            gpu_active: false,
            elevated: None,
        }
    }

    #[test]
    fn fullscreen_d3d_scores_game_at_standard() {
        let facts = WindowFacts {
            fullscreen: true,
            gfx_dll: true,
            ..facts()
        };

        assert_eq!(score(&facts), 80);
        assert_eq!(verdict(&facts, Sensitivity::Standard), Verdict::Game);
    }

    #[test]
    fn browser_windowed_scores_ignored() {
        let facts = WindowFacts {
            title: "Docs".to_string(),
            exe: "chrome.exe".to_string(),
            wclass: "Chrome_WidgetWin_1".to_string(),
            negative_class: true,
            ..facts()
        };

        assert_eq!(score(&facts), -60);
        assert_eq!(verdict(&facts, Sensitivity::Broad), Verdict::Ignored);
    }

    #[test]
    fn platform_path_windowed_scores_game_at_standard() {
        let facts = WindowFacts {
            platform_path: true,
            ..facts()
        };

        assert_eq!(score(&facts), 55);
        assert_eq!(verdict(&facts, Sensitivity::Standard), Verdict::Game);
        assert_eq!(verdict(&facts, Sensitivity::Strict), Verdict::Ignored);
    }

    #[test]
    fn negative_class_still_beats_platform_path() {
        let facts = WindowFacts {
            title: "Settings".to_string(),
            exe: "SystemSettings.exe".to_string(),
            wclass: "ApplicationFrameWindow".to_string(),
            platform_path: true,
            negative_class: true,
            ..facts()
        };

        assert_eq!(score(&facts), -5);
        assert_eq!(verdict(&facts, Sensitivity::Broad), Verdict::Ignored);
        assert_eq!(verdict(&facts, Sensitivity::Strict), Verdict::Ignored);
    }

    #[test]
    fn known_roblox_player_scores_game_at_strict() {
        let facts = WindowFacts {
            title: "Roblox".to_string(),
            exe: "RobloxPlayerBeta.exe".to_string(),
            wclass: "WINDOWSCLIENT".to_string(),
            known_game: true,
            ..facts()
        };

        assert_eq!(score(&facts), 80);
        assert_eq!(verdict(&facts, Sensitivity::Standard), Verdict::Game);
        assert_eq!(verdict(&facts, Sensitivity::Strict), Verdict::Game);
    }

    #[test]
    fn titleless_roblox_player_still_scores_game() {
        let facts = WindowFacts {
            title: "RobloxPlayerBeta".to_string(),
            exe: "RobloxPlayerBeta.exe".to_string(),
            wclass: "WINDOWSCLIENT".to_string(),
            known_game: true,
            ..facts()
        };

        assert!(is_known_game_window(
            "",
            "RobloxPlayerBeta.exe",
            "WINDOWSCLIENT",
            None,
            None
        ));
        assert_eq!(score(&facts), 80);
        assert_eq!(verdict(&facts, Sensitivity::Strict), Verdict::Game);
    }

    #[test]
    fn known_roblox_path_scores_game_at_strict() {
        let facts = WindowFacts {
            title: "Roblox".to_string(),
            exe: "RobloxPlayerBeta.exe".to_string(),
            wclass: "WINDOWSCLIENT".to_string(),
            platform_path: true,
            known_game: true,
            ..facts()
        };

        assert_eq!(score(&facts), 135);
        assert_eq!(verdict(&facts, Sensitivity::Strict), Verdict::Game);
    }

    #[test]
    fn recognizes_bloxstrap_roblox_paths() {
        assert!(is_game_platform_path(
            r"C:\Users\Player\AppData\Local\Bloxstrap\Versions\version-abc\RobloxPlayerBeta.exe"
        ));
        assert!(is_known_game_window(
            "",
            "RobloxPlayerBeta.exe",
            "WINDOWSCLIENT",
            Some(
                r"C:\Users\Player\AppData\Local\Bloxstrap\Versions\version-abc\RobloxPlayerBeta.exe"
            ),
            None
        ));
    }

    #[test]
    fn recognizes_roblox_store_wrapper_without_negative_class() {
        assert!(is_known_game_window(
            "Roblox",
            "Windows10Universal.exe",
            "ApplicationFrameWindow",
            Some(r"C:\Program Files\WindowsApps\ROBLOXCORPORATION.ROBLOX"),
            None
        ));
    }

    #[test]
    fn generic_application_frame_remains_negative() {
        assert!(!is_known_game_window(
            "Settings",
            "SystemSettings.exe",
            "ApplicationFrameWindow",
            None,
            None
        ));
        assert!(is_negative_window(
            "SystemSettings.exe",
            "ApplicationFrameWindow",
            None
        ));
    }
}
