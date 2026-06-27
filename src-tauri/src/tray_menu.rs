use crate::engine::SharedEngine;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Rect, WindowEvent};

pub const TRAY_MENU_STATE_EVENT: &str = "omnafk://tray-menu-state";

#[derive(Debug, Clone, Serialize)]
pub struct TrayMenuState {
    pub state: String,
    pub next: String,
    pub targets: String,
    pub suspend_label: String,
}

pub fn setup_window_events(app: &AppHandle) {
    let Some(window) = app.get_webview_window("tray-menu") else {
        return;
    };
    let app = app.clone();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Focused(false)) {
            let _ = hide(&app);
        }
    });
}

pub fn toggle_at_tray_rect(app: &AppHandle, rect: Rect) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("tray-menu") else {
        return Ok(());
    };

    if window.is_visible()? {
        return hide(app);
    }

    // Close flyout when opening the tray menu so only one popup is visible.
    if let Some(flyout) = app.get_webview_window("flyout") {
        if flyout.is_visible().unwrap_or(false) {
            let _ = flyout.hide();
        }
    }

    crate::flyout::position_window_near_tray(app, &window, rect)?;
    window.show()?;
    window.set_focus()?;
    if let Some(engine) = app.try_state::<SharedEngine>() {
        let _ = emit_state(app, engine.inner());
    }
    Ok(())
}

pub fn hide(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window("tray-menu") {
        window.hide()?;
    }
    Ok(())
}

pub fn emit_state(app: &AppHandle, engine: &SharedEngine) -> tauri::Result<()> {
    let payload = tray_menu_state(&engine.snapshot());
    app.emit(TRAY_MENU_STATE_EVENT, payload)
}

pub fn tray_menu_state(snapshot: &crate::engine::EngineSnapshot) -> TrayMenuState {
    let (state, next, targets) = crate::tray::menu_summaries(snapshot);
    TrayMenuState {
        state: state.trim_start_matches("State: ").to_string(),
        next,
        targets,
        suspend_label: if snapshot.config.suspended {
            "Resume watching".to_string()
        } else {
            "Suspend watching".to_string()
        },
    }
}
