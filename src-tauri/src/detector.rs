use crate::config::Sensitivity;
use serde::Serialize;
use std::{
    mem::{size_of, MaybeUninit},
    path::Path,
};
use windows::{
    core::{PWSTR, BOOL},
    Win32::{
        Foundation::{CloseHandle, HWND, LPARAM, RECT},
        Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST},
        System::{
            ProcessStatus::{
                K32EnumProcessModulesEx, K32GetModuleBaseNameW, LIST_MODULES_ALL,
            },
            Threading::{
                OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
                PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
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
    pub negative_class: bool,
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

pub fn scan_windows(sensitivity: Sensitivity, gpu: &dyn GpuUsageProbe) -> Vec<DetectedWindow> {
    let mut hwnds = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(enum_window), LPARAM((&mut hwnds as *mut Vec<HWND>) as isize));
    }

    hwnds
        .into_iter()
        .filter_map(|hwnd| gather_window_facts(hwnd, gpu))
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
        score += 30;
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

    let Ok(owner) = GetWindow(hwnd, GW_OWNER) else {
        return false;
    };
    if !owner.is_invalid() {
        return false;
    }

    let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
    if ex_style & WS_EX_TOOLWINDOW.0 != 0 {
        return false;
    }

    GetWindowTextLengthW(hwnd) > 0
}

fn gather_window_facts(hwnd: HWND, gpu: &dyn GpuUsageProbe) -> Option<DetectedWindowSeed> {
    let title = window_text(hwnd)?;
    let wclass = window_class(hwnd)?;
    let pid = window_pid(hwnd)?;
    let path = process_path(pid);
    let exe = path
        .as_deref()
        .and_then(|path| Path::new(path).file_name())
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| format!("pid-{pid}"));
    let modules = process_modules(pid);
    let gfx_dll = modules.iter().any(|module| is_graphics_module(module));
    let platform_path = path
        .as_deref()
        .map(is_game_platform_path)
        .unwrap_or(false);
    let (fullscreen, borderless) = window_screen_coverage(hwnd).unwrap_or((false, false));
    let negative_class = is_negative_window(&exe, &wclass);
    let _gpu_usage = gpu.usage_for_pid(pid);

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
            negative_class,
        },
    })
}

#[derive(Debug, Clone)]
struct DetectedWindowSeed {
    hwnd: isize,
    facts: WindowFacts,
}

fn window_text(hwnd: HWND) -> Option<String> {
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    if len <= 0 {
        return None;
    }

    let mut buf = vec![0; len as usize + 1];
    let read = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if read <= 0 {
        return None;
    }

    Some(String::from_utf16_lossy(&buf[..read as usize]).trim().to_string())
        .filter(|title| !title.is_empty())
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
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ,
            false,
            pid,
        )
        .ok()?
    };
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

fn module_name(handle: windows::Win32::Foundation::HANDLE, module: windows::Win32::Foundation::HMODULE) -> Option<String> {
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
        r"\riot games\",
        r"\xboxgames\",
        r"\windowsapps\",
        r"\roblox\",
        r"\battle.net\",
        r"\gog galaxy\",
    ]
    .iter()
    .any(|needle| path.contains(needle))
}

fn is_negative_window(exe: &str, wclass: &str) -> bool {
    let exe = exe.to_ascii_lowercase();
    let wclass = wclass.to_ascii_lowercase();

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
        || negative_class
            .iter()
            .any(|needle| wclass.contains(needle))
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
            negative_class: false,
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
    fn steam_path_windowed_depends_on_sensitivity() {
        let facts = WindowFacts {
            platform_path: true,
            ..facts()
        };

        assert_eq!(score(&facts), 30);
        assert_eq!(verdict(&facts, Sensitivity::Broad), Verdict::Game);
        assert_eq!(verdict(&facts, Sensitivity::Strict), Verdict::Ignored);
    }
}
