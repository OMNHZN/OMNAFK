use tauri::Manager;
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_notification::NotificationExt;

pub mod config;
pub mod detector;
pub mod engine;
pub mod flyout;
pub mod ipc;
pub mod keepalive;
pub mod setup;
pub mod stats;
pub mod tray;
pub mod updates;

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            let _ = flyout::open_default(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let config = match config::load() {
                Ok(config) => config,
                Err(error) => {
                    tracing::warn!("{error}");
                    config::AppConfig::default()
                }
            };
            let show_on_launch = config.show_on_launch;
            let first_run_notified = config.first_run_notified;
            let notifications = config.notifications;
            let github_repo = config.github_repo.clone();
            let update_channel = config.update_channel;
            let check_updates_on_launch = config.check_updates_on_launch;
            let engine = engine::Engine::new(config);
            engine.start();
            app.manage(engine.clone());
            ipc::spawn_state_pump(app.handle().clone(), engine);
            flyout::setup_window_events(
                app.handle(),
                app.state::<engine::SharedEngine>().inner().clone(),
            );
            tray::install(
                app.handle(),
                app.state::<engine::SharedEngine>().inner().clone(),
            )?;
            let _ = flyout::register_hotkey(
                app.handle(),
                &app.state::<engine::SharedEngine>().snapshot().config.hotkey,
            );
            apply_autostart_preference(
                app.handle(),
                app.state::<engine::SharedEngine>()
                    .snapshot()
                    .config
                    .autostart,
            );
            maybe_show_first_run_notification(
                app.handle(),
                app.state::<engine::SharedEngine>().inner().clone(),
                first_run_notified,
                notifications,
            );
            maybe_check_updates_on_launch(
                app.handle().clone(),
                github_repo,
                update_channel,
                check_updates_on_launch,
                notifications,
            );

            if cfg!(debug_assertions) || show_on_launch {
                if let Some(window) = app.get_webview_window("flyout") {
                    window.show()?;
                    window.set_focus()?;
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::get_state,
            ipc::set_config,
            ipc::cycle_override,
            ipc::rescan,
            ipc::set_suspended,
            ipc::set_pinned,
            ipc::hide_flyout,
            ipc::set_hotkey,
            ipc::reset_stats,
            ipc::import_settings,
            ipc::export_settings,
            ipc::check_updates,
            ipc::open_github,
            ipc::open_github_releases,
            ipc::open_github_issue,
            ipc::open_github_url
        ])
        .run(tauri::generate_context!())
        .expect("failed to run OMNAFK");
}

fn apply_autostart_preference(app: &tauri::AppHandle, enabled: bool) {
    use tauri_plugin_autostart::ManagerExt;

    let result = if enabled {
        app.autolaunch().enable()
    } else {
        app.autolaunch().disable()
    };
    if let Err(error) = result {
        tracing::warn!("Couldn't update Start with Windows - check Windows startup permissions to fix this: {error}");
    }
}

fn maybe_show_first_run_notification(
    app: &tauri::AppHandle,
    engine: engine::SharedEngine,
    first_run_notified: bool,
    notifications: config::NotificationLevel,
) {
    if first_run_notified || matches!(notifications, config::NotificationLevel::None) {
        return;
    }

    if let Err(error) = app
        .notification()
        .builder()
        .title("OMNAFK")
        .body("OMNAFK is in your tray. It wakes when a game does.")
        .show()
    {
        tracing::warn!("Couldn't show the first-run notification - enable Windows notifications to fix this: {error}");
    }

    engine.update_config_without_reschedule(|config| config.first_run_notified = true);
    if let Err(error) = config::save(&engine.snapshot().config) {
        tracing::warn!("{error}");
    }
}

fn maybe_check_updates_on_launch(
    app: tauri::AppHandle,
    github_repo: String,
    update_channel: config::UpdateChannel,
    enabled: bool,
    notifications: config::NotificationLevel,
) {
    if !enabled
        || github_repo.trim().is_empty()
        || matches!(notifications, config::NotificationLevel::None)
    {
        return;
    }

    std::thread::spawn(move || {
        match updates::check(&github_repo, update_channel, env!("CARGO_PKG_VERSION")) {
            Ok(check) if check.update_available => {
                let body = format!(
                    "{} is available. Open Settings to view the GitHub release.",
                    check.latest_tag
                );
                if let Err(error) = app
                    .notification()
                    .builder()
                    .title("OMNAFK update")
                    .body(body)
                    .show()
                {
                    tracing::warn!("Couldn't show update notification - enable Windows notifications to fix this: {error}");
                }
            }
            Ok(_) => {}
            Err(error) => tracing::warn!("{error}"),
        }
    });
}
