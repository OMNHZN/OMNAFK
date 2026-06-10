use crate::{config, engine::SharedEngine, ipc};
use tauri::{
    AppHandle, Manager, PhysicalPosition, Position, Rect, Size, WebviewWindow, WindowEvent,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

const GAP: f64 = 12.0;

pub fn setup_window_events(app: &AppHandle, engine: SharedEngine) {
    let Some(window) = app.get_webview_window("flyout") else {
        return;
    };

    let app = app.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Focused(false) => {
            let snapshot = engine.snapshot();
            if !snapshot.config.pinned {
                if let Some(window) = app.get_webview_window("flyout") {
                    let _ = window.hide();
                }
            }
        }
        WindowEvent::Moved(position) => {
            let snapshot = engine.snapshot();
            if snapshot.config.pinned && snapshot.config.remember_pin {
                engine.update_config_without_reschedule(|config| {
                    config.pin_position = Some(config::PinPosition {
                        x: position.x,
                        y: position.y,
                    });
                });
                let _ = config::save(&engine.snapshot().config);
            }
        }
        _ => {}
    });
}

pub fn toggle_at_tray_rect(app: &AppHandle, rect: Rect) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("flyout") else {
        return Ok(());
    };

    if window.is_visible()? {
        window.hide()?;
        return Ok(());
    }

    position_near_tray(app, &window, rect)?;
    show_window(&window)
}

pub fn open_default(app: &AppHandle) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("flyout") else {
        return Ok(());
    };
    show_window(&window)
}

pub fn register_hotkey(app: &AppHandle, hotkey: &str) -> Result<(), String> {
    let suspend_hotkey = app
        .try_state::<SharedEngine>()
        .map(|engine| engine.snapshot().config.suspend_hotkey)
        .unwrap_or_default();
    register_hotkeys(app, hotkey, &suspend_hotkey)
}

pub fn register_hotkeys(
    app: &AppHandle,
    open_hotkey: &str,
    suspend_hotkey: &str,
) -> Result<(), String> {
    app.global_shortcut().unregister_all().map_err(|error| {
        format!("Couldn't update the hotkey - restart OMNAFK to fix this: {error}")
    })?;
    app.global_shortcut()
        .on_shortcut(open_hotkey, |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                let _ = open_default(app);
            }
        })
        .map_err(|error| {
            format!("Couldn't register the hotkey - choose another shortcut to fix this: {error}")
        })?;

    let suspend_hotkey = suspend_hotkey.trim();
    if suspend_hotkey.is_empty() || suspend_hotkey.eq_ignore_ascii_case(open_hotkey) {
        return Ok(());
    }
    app.global_shortcut()
        .on_shortcut(suspend_hotkey, |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                if let Some(engine) = app.try_state::<SharedEngine>() {
                    let suspended = !engine.snapshot().config.suspended;
                    engine.update_config(|config| config.suspended = suspended);
                    if let Err(error) = config::save(&engine.snapshot().config) {
                        tracing::warn!("{error}");
                    }
                    let _ = ipc::emit_state(app, engine.inner());
                }
            }
        })
        .map_err(|error| {
            format!(
                "Couldn't register the suspend hotkey - choose another shortcut to fix this: {error}"
            )
        })
}

fn show_window(window: &WebviewWindow) -> tauri::Result<()> {
    window.show()?;
    window.set_focus()
}

fn position_near_tray(app: &AppHandle, window: &WebviewWindow, rect: Rect) -> tauri::Result<()> {
    let size = window.outer_size()?;
    let work = work_area_for_rect(app, &rect).or_else(|| {
        window
            .current_monitor()
            .ok()
            .flatten()
            .map(|monitor| *monitor.work_area())
    });
    let Some(work) = work else {
        return Ok(());
    };

    let win_w = size.width as f64;
    let win_h = size.height as f64;
    let left = work.position.x as f64;
    let top = work.position.y as f64;
    let right = left + work.size.width as f64;
    let bottom = top + work.size.height as f64;

    let (icon_left, icon_top, icon_width, icon_height) = rect_parts(&rect);
    let icon_right = icon_left + icon_width;
    let icon_bottom = icon_top + icon_height;
    let center_x = icon_left + icon_width / 2.0;
    let center_y = icon_top + icon_height / 2.0;

    let distances = [
        (TrayEdge::Bottom, (bottom - center_y).abs()),
        (TrayEdge::Top, (center_y - top).abs()),
        (TrayEdge::Left, (center_x - left).abs()),
        (TrayEdge::Right, (right - center_x).abs()),
    ];
    let edge = distances
        .into_iter()
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(edge, _)| edge)
        .unwrap_or(TrayEdge::Bottom);

    let (mut x, mut y) = match edge {
        TrayEdge::Bottom => (icon_right - win_w, icon_top - win_h - GAP),
        TrayEdge::Top => (icon_right - win_w, icon_bottom + GAP),
        TrayEdge::Left => (icon_right + GAP, icon_bottom - win_h),
        TrayEdge::Right => (icon_left - win_w - GAP, icon_bottom - win_h),
    };

    x = x.clamp(left, right - win_w);
    y = y.clamp(top, bottom - win_h);
    window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))
}

fn work_area_for_rect(app: &AppHandle, rect: &Rect) -> Option<tauri::PhysicalRect<i32, u32>> {
    let (x, y, width, height) = rect_parts(rect);
    let center_x = x + width / 2.0;
    let center_y = y + height / 2.0;

    app.available_monitors()
        .ok()?
        .into_iter()
        .find_map(|monitor| {
            let work = *monitor.work_area();
            let left = work.position.x as f64;
            let top = work.position.y as f64;
            let right = left + work.size.width as f64;
            let bottom = top + work.size.height as f64;
            (center_x >= left && center_x <= right && center_y >= top && center_y <= bottom)
                .then_some(work)
        })
}

fn rect_parts(rect: &Rect) -> (f64, f64, f64, f64) {
    let (x, y) = match rect.position {
        Position::Physical(position) => (position.x as f64, position.y as f64),
        Position::Logical(position) => (position.x, position.y),
    };
    let (width, height) = match rect.size {
        Size::Physical(size) => (size.width as f64, size.height as f64),
        Size::Logical(size) => (size.width, size.height),
    };
    (x, y, width, height)
}

#[derive(Debug, Clone, Copy)]
enum TrayEdge {
    Bottom,
    Top,
    Left,
    Right,
}
