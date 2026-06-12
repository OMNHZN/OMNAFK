//! Per-monitor game window placement for Windows.
//!
//! Enumerates displays by stable device name, detects where a window lives,
//! and moves it onto a user-chosen monitor with several placement styles.

use crate::{
    config::{MonitorStyle, MonitorWhen},
    detector::WindowFacts,
    keepalive::ActivityProbe,
};
use serde::Serialize;
use std::{
    ffi::c_void,
    fmt,
    time::{Duration, Instant},
};
use windows::core::BOOL;
use windows::Win32::{
    Foundation::{HWND, LPARAM, RECT},
    Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, MonitorFromWindow, HDC, HMONITOR, MONITORINFOEXW,
        MONITOR_DEFAULTTONEAREST,
    },
    UI::WindowsAndMessaging::{
        GetWindowRect, IsIconic, IsZoomed, SetWindowPos, ShowWindow, HWND_TOP, SWP_NOACTIVATE,
        SWP_NOZORDER, SWP_SHOWWINDOW, SW_MAXIMIZE, SW_RESTORE,
    },
};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MonitorInfo {
    pub device: String,
    pub label: String,
    pub primary: bool,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorRects {
    pub monitor: RECT,
    pub work: RECT,
}

#[derive(Debug, Clone, Copy)]
pub struct PlacementOptions {
    pub when: MonitorWhen,
    pub style: MonitorStyle,
    pub skip_active: bool,
    pub skip_active_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlacementResult {
    Moved,
    AlreadyOnTarget,
    SkippedOff,
    SkippedActive,
    SkippedLaunchDone,
    MonitorMissing,
    Failed(String),
}

impl PlacementResult {
    pub fn status_label(&self) -> &'static str {
        match self {
            Self::Moved => "Moved",
            Self::AlreadyOnTarget => "On target",
            Self::SkippedOff => "Off",
            Self::SkippedActive => "Waiting (active)",
            Self::SkippedLaunchDone => "On target",
            Self::MonitorMissing => "Monitor disconnected",
            Self::Failed(_) => "Move failed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlacementError {
    message: String,
}

impl fmt::Display for PlacementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

/// List connected monitors, primary first.
pub fn list_monitors() -> Vec<MonitorInfo> {
    let mut raw = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(collect_monitor),
            LPARAM(&mut raw as *mut _ as isize),
        );
    }
    raw.sort_by(|a: &RawMonitor, b| {
        b.primary
            .cmp(&a.primary)
            .then_with(|| a.device.cmp(&b.device))
    });
    raw.into_iter()
        .enumerate()
        .map(|(index, entry)| MonitorInfo {
            label: format_monitor_label(index + 1, &entry),
            device: entry.device,
            primary: entry.primary,
            width: rect_width(entry.monitor),
            height: rect_height(entry.monitor),
        })
        .collect()
}

pub fn monitor_by_device(device: &str) -> Option<MonitorInfo> {
    list_monitors().into_iter().find(|m| m.device == device)
}

pub fn window_monitor_device(hwnd: isize) -> Option<String> {
    let hwnd = hwnd_from_isize(hwnd);
    let hmon = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
    if hmon.0.is_null() {
        return None;
    }
    monitor_device(hmon)
}

pub fn try_place_window(
    hwnd: isize,
    target_device: &str,
    facts: &WindowFacts,
    options: &PlacementOptions,
    already_placed: bool,
    now: Instant,
    activity: &dyn ActivityProbe,
) -> PlacementResult {
    if options.when == MonitorWhen::OnLaunch && already_placed {
        return PlacementResult::SkippedLaunchDone;
    }

    let Some(target) = monitor_rects(target_device) else {
        return PlacementResult::MonitorMissing;
    };

    let current = window_monitor_device(hwnd);
    if current.as_deref() == Some(target_device) {
        return PlacementResult::AlreadyOnTarget;
    }

    if options.skip_active && user_recently_active(hwnd, options.skip_active_secs, now, activity) {
        return PlacementResult::SkippedActive;
    }

    match move_window(hwnd, &target, facts, options.style) {
        Ok(()) => PlacementResult::Moved,
        Err(error) => PlacementResult::Failed(error.to_string()),
    }
}

fn move_window(
    hwnd: isize,
    target: &MonitorRects,
    facts: &WindowFacts,
    style: MonitorStyle,
) -> Result<(), PlacementError> {
    let hwnd = hwnd_from_isize(hwnd);
    if hwnd.is_invalid() {
        return Err(PlacementError {
            message: "Window handle is no longer valid.".to_string(),
        });
    }

    let mut rect = RECT::default();
    unsafe {
        GetWindowRect(hwnd, &mut rect).map_err(|_| PlacementError {
            message: "Couldn't read the window position.".to_string(),
        })?;
    }

    let maximized = unsafe { IsZoomed(hwnd).as_bool() };
    let minimized = unsafe { IsIconic(hwnd).as_bool() };
    if minimized {
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
    } else if maximized && !matches!(style, MonitorStyle::Maximize) {
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        unsafe {
            GetWindowRect(hwnd, &mut rect).map_err(|_| PlacementError {
                message: "Couldn't read the window position.".to_string(),
            })?;
        }
    }

    let (x, y, w, h) = target_geometry(&rect, target, facts, style);
    unsafe {
        SetWindowPos(
            hwnd,
            Some(HWND_TOP),
            x,
            y,
            w,
            h,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        )
        .map_err(|_| PlacementError {
            message:
                "Couldn't move the window — it may be running as administrator or in exclusive fullscreen."
                    .to_string(),
        })?;
    }

    if matches!(style, MonitorStyle::Maximize) || maximized {
        unsafe {
            let _ = ShowWindow(hwnd, SW_MAXIMIZE);
        }
    }

    Ok(())
}

fn target_geometry(
    window: &RECT,
    target: &MonitorRects,
    facts: &WindowFacts,
    style: MonitorStyle,
) -> (i32, i32, i32, i32) {
    match style {
        MonitorStyle::FillMonitor | MonitorStyle::FillWorkArea
            if facts.fullscreen || facts.borderless =>
        {
            let area = placement_area(target, style);
            (area.left, area.top, rect_width(area), rect_height(area))
        }
        MonitorStyle::FillMonitor => {
            let area = target.monitor;
            (area.left, area.top, rect_width(area), rect_height(area))
        }
        MonitorStyle::FillWorkArea => {
            let area = target.work;
            (area.left, area.top, rect_width(area), rect_height(area))
        }
        MonitorStyle::Maximize => {
            let area = target.work;
            (area.left, area.top, rect_width(area), rect_height(area))
        }
        MonitorStyle::Preserve => {
            let area = target.work;
            let width = rect_width(*window).clamp(200, rect_width(area));
            let height = rect_height(*window).clamp(150, rect_height(area));
            let x = area.left + (rect_width(area).saturating_sub(width) / 2);
            let y = area.top + (rect_height(area).saturating_sub(height) / 2);
            (x, y, width, height)
        }
    }
}

fn placement_area(target: &MonitorRects, style: MonitorStyle) -> RECT {
    match style {
        MonitorStyle::FillMonitor => target.monitor,
        _ => target.work,
    }
}

fn user_recently_active(
    hwnd: isize,
    skip_secs: u64,
    now: Instant,
    activity: &dyn ActivityProbe,
) -> bool {
    let threshold = Duration::from_secs(skip_secs.max(1));
    activity.foreground_window().is_some_and(|fg| fg == hwnd)
        && activity
            .last_input_age(now)
            .is_some_and(|age| age <= threshold)
}

fn monitor_rects(device: &str) -> Option<MonitorRects> {
    let mut found = None;
    unsafe {
        let _ = EnumDisplayMonitors(
            None,
            None,
            Some(find_monitor),
            LPARAM(&mut FindCtx {
                device,
                out: &mut found,
            } as *mut _ as isize),
        );
    }
    found
}

#[derive(Debug, Clone)]
struct RawMonitor {
    device: String,
    primary: bool,
    monitor: RECT,
    work: RECT,
}

struct FindCtx<'a> {
    device: &'a str,
    out: &'a mut Option<MonitorRects>,
}

unsafe extern "system" fn collect_monitor(
    hmon: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let bucket = &mut *(data.0 as *mut Vec<RawMonitor>);
    if let Some(entry) = read_monitor(hmon) {
        bucket.push(entry);
    }
    true.into()
}

unsafe extern "system" fn find_monitor(
    hmon: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let ctx = &mut *(data.0 as *mut FindCtx<'_>);
    if let Some(entry) = read_monitor(hmon) {
        if entry.device == ctx.device {
            *ctx.out = Some(MonitorRects {
                monitor: entry.monitor,
                work: entry.work,
            });
            return false.into();
        }
    }
    true.into()
}

unsafe fn read_monitor(hmon: HMONITOR) -> Option<RawMonitor> {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if !unsafe { GetMonitorInfoW(hmon, &mut info.monitorInfo).as_bool() } {
        return None;
    }
    Some(RawMonitor {
        device: wchar_device(&info.szDevice),
        primary: info.monitorInfo.dwFlags & 1 != 0,
        monitor: info.monitorInfo.rcMonitor,
        work: info.monitorInfo.rcWork,
    })
}

fn monitor_device(hmon: HMONITOR) -> Option<String> {
    unsafe { read_monitor(hmon).map(|entry| entry.device) }
}

fn format_monitor_label(index: usize, entry: &RawMonitor) -> String {
    let suffix = if entry.primary { ", primary" } else { "" };
    format!(
        "Monitor {} ({}×{}{})",
        index,
        rect_width(entry.monitor),
        rect_height(entry.monitor),
        suffix
    )
}

fn wchar_device(raw: &[u16; 32]) -> String {
    let len = raw.iter().position(|&ch| ch == 0).unwrap_or(raw.len());
    String::from_utf16_lossy(&raw[..len])
}

fn rect_width(rect: RECT) -> i32 {
    rect.right.saturating_sub(rect.left)
}

fn rect_height(rect: RECT) -> i32 {
    rect.bottom.saturating_sub(rect.top)
}

fn hwnd_from_isize(hwnd: isize) -> HWND {
    HWND(hwnd as *mut c_void)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserve_geometry_centers_on_work_area() {
        let window = RECT {
            left: 100,
            top: 100,
            right: 900,
            bottom: 700,
        };
        let target = MonitorRects {
            monitor: RECT {
                left: 1920,
                top: 0,
                right: 3840,
                bottom: 1080,
            },
            work: RECT {
                left: 1920,
                top: 0,
                right: 3840,
                bottom: 1040,
            },
        };
        let facts = WindowFacts {
            title: String::new(),
            exe: String::new(),
            wclass: String::new(),
            pid: 0,
            fullscreen: false,
            borderless: false,
            gfx_dll: false,
            platform_path: false,
            known_game: false,
            negative_class: false,
            elevated: None,
        };
        let (x, y, w, h) = target_geometry(&window, &target, &facts, MonitorStyle::Preserve);
        assert_eq!(w, 800);
        assert_eq!(h, 600);
        assert_eq!(x, 1920 + (1920 - 800) / 2);
        assert_eq!(y, (1040 - 600) / 2);
    }
}
