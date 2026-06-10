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
            let update_options = LaunchUpdateCheckOptions {
                github_repo: config.github_repo.clone(),
                update_channel: config.update_channel,
                enabled: config.check_updates_on_launch,
                ignored_update_tag: config.ignored_update_tag.clone(),
                update_prompt_mode: config.update_prompt_mode,
                notifications,
            };
            let engine = engine::Engine::with_stats(config, stats::load_persisted());
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
                update_options,
                app.state::<engine::SharedEngine>().inner().clone(),
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
            ipc::set_override,
            ipc::clear_overrides,
            ipc::pause_target,
            ipc::test_target,
            ipc::snooze,
            ipc::set_target_profile,
            ipc::rescan,
            ipc::set_suspended,
            ipc::set_pinned,
            ipc::hide_flyout,
            ipc::set_hotkey,
            ipc::reset_stats,
            ipc::export_stats,
            ipc::reset_settings,
            ipc::import_settings,
            ipc::export_settings,
            ipc::open_config_dir,
            ipc::open_log_file,
            ipc::diagnostics,
            ipc::get_changelog,
            ipc::check_updates,
            ipc::ignore_update,
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

struct LaunchUpdateCheckOptions {
    github_repo: String,
    update_channel: config::UpdateChannel,
    enabled: bool,
    ignored_update_tag: Option<String>,
    update_prompt_mode: config::UpdatePromptMode,
    notifications: config::NotificationLevel,
}

fn maybe_check_updates_on_launch(
    app: tauri::AppHandle,
    options: LaunchUpdateCheckOptions,
    engine: engine::SharedEngine,
) {
    if !options.enabled
        || options.github_repo.trim().is_empty()
        || matches!(
            options.update_prompt_mode,
            config::UpdatePromptMode::ManualOnly
        )
    {
        return;
    }

    std::thread::spawn(move || {
        match updates::check(
            &options.github_repo,
            options.update_channel,
            env!("CARGO_PKG_VERSION"),
        ) {
            Ok(check)
                if check.update_available
                    && options.ignored_update_tag.as_deref() != Some(check.latest_tag.as_str()) =>
            {
                engine.set_update_prompt(Some(check.clone()));
                let _ = ipc::emit_state(&app, &engine);
                let body = format!(
                    "{} is available. Open OMNAFK to update or ignore it.",
                    check.latest_tag
                );
                if matches!(
                    options.update_prompt_mode,
                    config::UpdatePromptMode::CardAndToast
                ) && !matches!(options.notifications, config::NotificationLevel::None)
                {
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
            }
            Ok(_) => {}
            Err(error) => tracing::warn!("{error}"),
        }
    });
}
