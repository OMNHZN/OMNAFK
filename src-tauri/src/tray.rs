use crate::{
    config,
    engine::{EngineSnapshot, EngineStatus, SharedEngine},
    flyout, ipc, tray_menu, updates,
};
use std::{
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{
    image::Image,
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, Wry,
};

static ATTENTION_UNTIL: AtomicU64 = AtomicU64::new(0);
static TRAY_INSTALLED: AtomicBool = AtomicBool::new(false);

pub fn is_installed() -> bool {
    TRAY_INSTALLED.load(Ordering::SeqCst)
}

pub fn ensure_installed(app: &AppHandle, engine: &SharedEngine) {
    if TRAY_INSTALLED.load(Ordering::SeqCst) {
        return;
    }
    match install(app, engine.clone()) {
        Ok(()) => {
            TRAY_INSTALLED.store(true, Ordering::SeqCst);
            crate::startup_log::info("tray icon installed on retry");
        }
        Err(error) => {
            crate::startup_log::warn(format!("tray install retry failed: {error}"));
        }
    }
}

pub fn request_attention() {
    let until = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().saturating_add(30))
        .unwrap_or(0);
    ATTENTION_UNTIL.store(until, Ordering::SeqCst);
}

fn attention_active() -> bool {
    let until = ATTENTION_UNTIL.load(Ordering::SeqCst);
    until > 0
        && SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0)
            < until
}

pub fn install(app: &AppHandle, engine: SharedEngine) -> tauri::Result<()> {
    let app_handle = app.clone();
    let tray = TrayIconBuilder::with_id("omnafk-tray")
        .icon(icon_for(EngineStatus::Dormant, false, false)?)
        .tooltip("OMNAFK - DORMANT")
        .show_menu_on_left_click(false)
        .on_tray_icon_event({
            let engine = engine.clone();
            move |tray, event| handle_tray_event(tray, event, &engine)
        })
        .build(app)?;

    spawn_tray_state_loop(tray, engine, app_handle);
    TRAY_INSTALLED.store(true, Ordering::SeqCst);
    Ok(())
}

pub fn execute_action(app: &AppHandle, engine: &SharedEngine, action: &str) {
    match action {
        "toggle_suspend" => {
            let suspended = !engine.snapshot().config.suspended;
            engine.update_config(|config| config.suspended = suspended);
            if let Err(error) = config::save(&engine.snapshot().config) {
                tracing::warn!("{error}");
            }
            let _ = ipc::emit_state(app, engine);
            let _ = tray_menu::emit_state(app, engine);
        }
        "open" => {
            engine.mark_user_ui_opened();
            let _ = tray_menu::hide(app);
            let _ = flyout::open_default(app);
        }
        "settings" => {
            engine.mark_user_ui_opened();
            let _ = tray_menu::hide(app);
            let _ = flyout::open_default(app);
            let _ = app.emit("omnafk://open-settings", "settings");
        }
        "updates" => {
            engine.mark_user_ui_opened();
            let _ = tray_menu::hide(app);
            let _ = flyout::open_default(app);
            let _ = app.emit("omnafk://open-settings", "updates");
        }
        "bug" => {
            let repo = engine.snapshot().config.github_repo;
            match updates::issues_url(&repo).and_then(|url| updates::open_url(&url)) {
                Ok(()) => {}
                Err(error) => tracing::warn!("{error}"),
            }
        }
        "quit" => {
            engine.stop();
            app.exit(0);
        }
        _ => {}
    }
}

fn handle_tray_event(tray: &TrayIcon<Wry>, event: TrayIconEvent, _engine: &SharedEngine) {
    match event {
        TrayIconEvent::Click {
            rect,
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } => {
            let app = tray.app_handle();
            if let Some(engine) = app.try_state::<SharedEngine>() {
                engine.mark_user_ui_opened();
            }
            let _ = tray_menu::hide(app);
            let _ = flyout::toggle_at_tray_rect(app, rect);
        }
        TrayIconEvent::Click {
            rect,
            button: MouseButton::Right,
            button_state: MouseButtonState::Up,
            ..
        } => {
            let _ = tray_menu::toggle_at_tray_rect(tray.app_handle(), rect);
        }
        _ => {}
    }
}

fn spawn_tray_state_loop(tray: TrayIcon<Wry>, engine: SharedEngine, app: AppHandle) {
    thread::spawn(move || {
        let blink = AtomicBool::new(false);
        loop {
            let fast = attention_active();
            thread::sleep(if fast {
                Duration::from_millis(500)
            } else {
                Duration::from_secs(1)
            });
            let snapshot = engine.snapshot();
            let blink_on = blink.fetch_xor(true, Ordering::SeqCst);
            let state = snapshot.engine;

            if let Ok(icon) = icon_for(state, blink_on, fast) {
                let _ = tray.set_icon(Some(icon));
            }
            let _ = tray.set_tooltip(Some(tooltip_for(&snapshot)));

            if app
                .get_webview_window("tray-menu")
                .is_some_and(|window| window.is_visible().unwrap_or(false))
            {
                let _ = tray_menu::emit_state(&app, &engine);
            }
        }
    });
}

pub fn menu_summaries(snapshot: &EngineSnapshot) -> (String, String, String) {
    let active: Vec<_> = snapshot
        .games
        .iter()
        .filter(|game| {
            game.effective == crate::detector::Verdict::Game && !game.gone && !game.paused
        })
        .collect();
    let ignored = snapshot
        .games
        .iter()
        .filter(|game| game.effective == crate::detector::Verdict::Ignored && !game.gone)
        .count();
    let state = match snapshot.engine {
        EngineStatus::Dormant => "OMNAFK - DORMANT".to_string(),
        EngineStatus::Suspended => {
            if let Some(remaining) = snapshot.snooze_remaining {
                format!("OMNAFK - SNOOZED - BACK IN {}", mmss(remaining))
            } else {
                "OMNAFK - SUSPENDED".to_string()
            }
        }
        EngineStatus::Holding => snapshot
            .paused_reason
            .as_ref()
            .map(|reason| format!("OMNAFK - HOLDING - {reason}"))
            .unwrap_or_else(|| "OMNAFK - HOLDING - RECENT INPUT".to_string()),
        EngineStatus::Active => {
            if active.len() == 1 {
                format!("OMNAFK - ACTIVE - {}", compact_title(&active[0].title))
            } else {
                format!("OMNAFK - ACTIVE - {} TARGETS", active.len())
            }
        }
    };
    let next = match snapshot.engine {
        EngineStatus::Active => snapshot
            .next_tick
            .map(|seconds| format!("Next tick: {}", mmss(seconds)))
            .unwrap_or_else(|| "Next tick: --".to_string()),
        EngineStatus::Holding => "Next tick: held".to_string(),
        _ => "Next tick: --".to_string(),
    };
    let mut targets = format!("Targets: {} active, {} ignored", active.len(), ignored);
    if snapshot.config.monitor_placement {
        let placed = active
            .iter()
            .filter(|game| {
                game.monitor
                    .status
                    .as_deref()
                    .is_some_and(|s| s.contains("target") || s == "Moved")
            })
            .count();
        if placed > 0 {
            targets.push_str(&format!(", {placed} on monitor"));
        }
    }
    (
        format!("State: {}", state.trim_start_matches("OMNAFK - ")),
        next,
        targets,
    )
}

fn tooltip_for(snapshot: &EngineSnapshot) -> String {
    let (state, next, targets) = menu_summaries(snapshot);
    format!("OMNAFK\n{state}\n{next}\n{targets}")
}

fn mmss(seconds: u64) -> String {
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

fn compact_title(title: &str) -> String {
    const MAX: usize = 28;
    let mut out = String::new();
    for ch in title.chars().take(MAX) {
        out.push(ch);
    }
    if title.chars().count() > MAX {
        out.push('…');
    }
    out
}

fn icon_for(state: EngineStatus, blink_on: bool, attention: bool) -> tauri::Result<Image<'static>> {
    match state {
        EngineStatus::Active => Image::from_bytes(include_bytes!("../icons/sentinel-active.png")),
        EngineStatus::Holding if blink_on => {
            Image::from_bytes(include_bytes!("../icons/sentinel-active.png"))
        }
        EngineStatus::Holding => Image::from_bytes(include_bytes!("../icons/sentinel-dormant.png")),
        EngineStatus::Suspended => {
            Image::from_bytes(include_bytes!("../icons/sentinel-suspended.png"))
        }
        EngineStatus::Dormant if attention && blink_on => {
            Image::from_bytes(include_bytes!("../icons/sentinel-active.png"))
        }
        _ => Image::from_bytes(include_bytes!("../icons/sentinel-dormant.png")),
    }
}
