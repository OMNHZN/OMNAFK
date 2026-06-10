use crate::{
    config,
    engine::{EngineStatus, SharedEngine},
    flyout, ipc, updates,
};
use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Wry,
};

const MENU_TOGGLE_SUSPEND: &str = "toggle_suspend";
const MENU_OPEN: &str = "open_omnafk";
const MENU_SETTINGS: &str = "open_settings";
const MENU_CHECK_UPDATES: &str = "check_updates";
const MENU_REPORT_BUG: &str = "report_bug";
const MENU_QUIT: &str = "quit_omnafk";

pub fn install(app: &AppHandle, engine: SharedEngine) -> tauri::Result<()> {
    let suspend_label = if engine.snapshot().config.suspended {
        "Resume watching"
    } else {
        "Suspend watching"
    };
    let suspend_item =
        MenuItem::with_id(app, MENU_TOGGLE_SUSPEND, suspend_label, true, None::<&str>)?;
    let open_item = MenuItem::with_id(app, MENU_OPEN, "Open flyout", true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, MENU_SETTINGS, "Settings", true, None::<&str>)?;
    let update_item = MenuItem::with_id(
        app,
        MENU_CHECK_UPDATES,
        "Check for updates",
        true,
        None::<&str>,
    )?;
    let bug_item = MenuItem::with_id(app, MENU_REPORT_BUG, "Report a bug", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, MENU_QUIT, "Quit OMNAFK", true, None::<&str>)?;
    let separator_one = PredefinedMenuItem::separator(app)?;
    let separator_two = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &open_item,
            &settings_item,
            &update_item,
            &bug_item,
            &separator_one,
            &suspend_item,
            &separator_two,
            &quit_item,
        ],
    )?;

    let tray = TrayIconBuilder::with_id("omnafk-tray")
        .icon(icon_for(EngineStatus::Dormant, false)?)
        .tooltip("OMNAFK - DORMANT")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event({
            let engine = engine.clone();
            move |tray, event| handle_tray_event(tray, event, &engine)
        })
        .on_menu_event({
            let engine = engine.clone();
            let suspend_item = suspend_item.clone();
            move |app, event| handle_menu_event(app, event.id().0.as_str(), &engine, &suspend_item)
        })
        .build(app)?;

    spawn_tray_state_loop(tray, suspend_item, engine);
    Ok(())
}

fn handle_tray_event(tray: &TrayIcon<Wry>, event: TrayIconEvent, _engine: &SharedEngine) {
    if let TrayIconEvent::Click {
        rect,
        button: MouseButton::Left,
        button_state: MouseButtonState::Up,
        ..
    } = event
    {
        let _ = flyout::toggle_at_tray_rect(tray.app_handle(), rect);
    }
}

fn handle_menu_event(
    app: &AppHandle,
    id: &str,
    engine: &SharedEngine,
    suspend_item: &MenuItem<Wry>,
) {
    match id {
        MENU_TOGGLE_SUSPEND => {
            let suspended = !engine.snapshot().config.suspended;
            engine.update_config(|config| config.suspended = suspended);
            if let Err(error) = config::save(&engine.snapshot().config) {
                tracing::warn!("{error}");
            }
            let _ = suspend_item.set_text(if suspended {
                "Resume watching"
            } else {
                "Suspend watching"
            });
            let _ = ipc::emit_state(app, engine);
        }
        MENU_OPEN => {
            let _ = flyout::open_default(app);
        }
        MENU_SETTINGS => {
            let _ = flyout::open_default(app);
            let _ = app.emit("omnafk://open-settings", "settings");
        }
        MENU_CHECK_UPDATES => {
            let _ = flyout::open_default(app);
            let _ = app.emit("omnafk://open-settings", "updates");
        }
        MENU_REPORT_BUG => {
            let repo = engine.snapshot().config.github_repo;
            match updates::issues_url(&repo).and_then(|url| updates::open_url(&url)) {
                Ok(()) => {}
                Err(error) => tracing::warn!("{error}"),
            }
        }
        MENU_QUIT => {
            engine.stop();
            app.exit(0);
        }
        _ => {}
    }
}

fn spawn_tray_state_loop(tray: TrayIcon<Wry>, suspend_item: MenuItem<Wry>, engine: SharedEngine) {
    thread::spawn(move || {
        let blink = AtomicBool::new(false);
        loop {
            thread::sleep(Duration::from_secs(1));
            let snapshot = engine.snapshot();
            let blink_on = blink.fetch_xor(true, Ordering::SeqCst);
            let state = snapshot.engine;
            let suspended = snapshot.config.suspended;

            if let Ok(icon) = icon_for(state, blink_on) {
                let _ = tray.set_icon(Some(icon));
            }
            let _ = tray.set_tooltip(Some(tooltip_for(state, snapshot.next_tick)));
            let _ = suspend_item.set_text(if suspended {
                "Resume watching"
            } else {
                "Suspend watching"
            });
        }
    });
}

fn tooltip_for(state: EngineStatus, next_tick: Option<u64>) -> String {
    match state {
        EngineStatus::Dormant => "OMNAFK - DORMANT".to_string(),
        EngineStatus::Suspended => "OMNAFK - SUSPENDED".to_string(),
        EngineStatus::Holding => "OMNAFK - HOLDING".to_string(),
        EngineStatus::Active => {
            let next = next_tick.unwrap_or_default();
            format!(
                "OMNAFK - ACTIVE - NEXT TICK {:02}:{:02}",
                next / 60,
                next % 60
            )
        }
    }
}

fn icon_for(state: EngineStatus, _blink_on: bool) -> tauri::Result<Image<'static>> {
    match state {
        EngineStatus::Active | EngineStatus::Holding => {
            Image::from_bytes(include_bytes!("../icons/sentinel-active.png"))
        }
        EngineStatus::Suspended => {
            Image::from_bytes(include_bytes!("../icons/sentinel-suspended.png"))
        }
        _ => Image::from_bytes(include_bytes!("../icons/sentinel-dormant.png")),
    }
}
