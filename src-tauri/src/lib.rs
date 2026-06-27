use tauri::Manager;

static POST_INSTALL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub mod alerts;
pub mod audio;
pub mod community;
pub mod config;
pub mod detector;
pub mod elevation;
pub mod engine;
pub mod flyout;
pub mod gamepad;
pub mod gamepad_send;
pub mod gpu;
pub mod health;
pub mod installer;
pub mod ipc;
pub mod keepalive;
pub mod learn;
pub mod monitor;
pub mod notifications;
pub mod persist;
pub mod presets;
pub mod setup;
pub mod startup;
pub mod startup_log;
pub mod stats;
pub mod time_util;
pub mod tray;
pub mod tray_menu;
pub mod updates;

pub fn run() {
    startup_log::init();
    startup_log::install_panic_hook();
    startup_log::info(format!(
        "OMNAFK {} starting (pid={}, args={:?})",
        env!("CARGO_PKG_VERSION"),
        std::process::id(),
        std::env::args().collect::<Vec<_>>()
    ));

    if std::env::args().any(|arg| arg == "--post-install") {
        POST_INSTALL.store(true, std::sync::atomic::Ordering::SeqCst);
        startup_log::info("post-install launch detected");
    }

    if startup::is_autostart_launch() {
        startup_log::info("Windows autostart launch detected (--autostart)");
    }

    if elevation::is_elevation_handoff(&std::env::args().collect::<Vec<_>>()) {
        startup_log::info("elevated relaunch handoff requested");
    }

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let result = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(handle_duplicate_launch))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            startup_log::info("setup begin");

            let (mut config, config_load_error) = match config::load() {
                Ok(config) => {
                    startup_log::info("config loaded");
                    (config, None)
                }
                Err(error) => {
                    startup_log::warn(format!("config load failed, using defaults: {error}"));
                    (config::AppConfig::default(), Some(error.to_string()))
                }
            };

            let community = community::shared_runtime();
            if config.community_intelligence {
                community::ensure_client_id(&mut config);
                if let Err(error) = config::save(&config) {
                    startup_log::warn(format!("couldn't save community client id: {error}"));
                }
                community::refresh_on_launch(&community, &config.github_repo);
                startup_log::info("community manifest refresh queued on launch");
            }

            startup_log::info("community runtime ready");

            gamepad_send::set_kind(config.gamepad_kind);

            let show_on_launch = config.show_on_launch && !config.headless;
            let first_run_notified = config.first_run_notified;
            let notifications = config.notifications;
            let update_options = LaunchUpdateCheckOptions {
                github_repo: config.github_repo.clone(),
                enabled: config.check_updates_on_launch,
                ignored_update_tag: config.ignored_update_tag.clone(),
                update_prompt_mode: config.update_prompt_mode,
                notifications,
            };

            let persisted = stats::load_persisted();
            startup_log::info("stats loaded");

            let engine = engine::Engine::with_launch_context(
                config,
                persisted,
                community.clone(),
                startup::is_autostart_launch(),
            );
            app.manage(engine.clone());
            startup_log::info("engine registered");

            engine.start();
            startup_log::info("engine started");

            if let Some(message) = config_load_error {
                engine.note_runtime_warning(message, true);
            }

            community::spawn_sync_loop(community, engine.clone());

            ipc::spawn_state_pump(app.handle().clone(), engine.clone());
            startup_log::info("state pump started");

            flyout::setup_window_events(
                app.handle(),
                app.state::<engine::SharedEngine>().inner().clone(),
            );
            tray_menu::setup_window_events(app.handle());

            match tray::install(
                app.handle(),
                app.state::<engine::SharedEngine>().inner().clone(),
            ) {
                Ok(()) => startup_log::info("tray installed"),
                Err(error) => startup_log::warn(format!(
                    "tray install failed on first attempt (will retry): {error}"
                )),
            }

            if let Err(error) = flyout::register_hotkey(
                app.handle(),
                &app.state::<engine::SharedEngine>().snapshot().config.hotkey,
            ) {
                startup_log::warn(format!("hotkey registration failed: {error}"));
            } else {
                startup_log::info("hotkeys registered");
            }

            apply_autostart_preference(app.state::<engine::SharedEngine>().inner().clone());
            startup_log::info("autostart preference synced");

            maybe_show_first_run_notification(
                app.handle(),
                app.state::<engine::SharedEngine>().inner().clone(),
                first_run_notified,
                notifications,
            );

            if update_options.enabled
                && !update_options.github_repo.trim().is_empty()
                && !matches!(
                    update_options.update_prompt_mode,
                    config::UpdatePromptMode::ManualOnly
                )
            {
                startup_log::info("update check queued for launch");
            } else {
                startup_log::info("launch update check skipped");
            }

            maybe_check_updates_on_launch(
                app.handle().clone(),
                update_options,
                app.state::<engine::SharedEngine>().inner().clone(),
            );

            if POST_INSTALL.load(std::sync::atomic::Ordering::SeqCst) {
                tray::request_attention();
                if !matches!(notifications, config::NotificationLevel::None) {
                    notifications::deliver(
                        app.handle(),
                        &notifications::QueuedNotice::info(
                            "Update complete. OMNAFK is back in your tray.",
                        ),
                    );
                }
            }

            if cfg!(debug_assertions) || show_on_launch {
                if let Some(window) = app.get_webview_window("flyout") {
                    engine.mark_user_ui_opened();
                    window.show()?;
                    window.set_focus()?;
                    startup_log::info("flyout shown on launch");
                }
            } else {
                startup_log::info("tray-only launch (show_on_launch=false)");
            }

            startup_log::info("setup complete");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::list_presets,
            ipc::apply_preset,
            ipc::save_user_preset,
            ipc::apply_user_preset,
            ipc::delete_user_preset,
            ipc::dismiss_community_profile,
            ipc::apply_community_profile,
            ipc::community_feedback,
            ipc::share_community_profile,
            ipc::test_all_targets,
            ipc::move_target,
            ipc::restart_as_admin,
            ipc::list_monitors,
            ipc::get_state,
            ipc::set_config,
            ipc::cycle_override,
            ipc::set_override,
            ipc::clear_overrides,
            ipc::pause_target,
            ipc::test_target,
            ipc::explain_detection,
            ipc::test_alert,
            ipc::reset_learning,
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
            ipc::run_app_update,
            ipc::ignore_update,
            ipc::open_github,
            ipc::open_github_releases,
            ipc::open_github_issue,
            ipc::open_github_url,
            ipc::get_tray_menu_state,
            ipc::tray_menu_action,
            ipc::hide_tray_menu,
            ipc::toast_action,
        ])
        .run(tauri::generate_context!());

    match result {
        Ok(()) => startup_log::info("Tauri event loop exited normally"),
        Err(error) => {
            startup_log::error(format!("Tauri run failed: {error}"));
            std::process::exit(1);
        }
    }
}

fn handle_duplicate_launch(app: &tauri::AppHandle, args: Vec<String>, _cwd: String) {
    if elevation::is_elevation_handoff(&args) {
        startup_log::info(
            "elevation handoff: exiting unelevated instance so elevated copy can start",
        );
        if let Some(engine) = app.try_state::<engine::SharedEngine>() {
            engine.stop();
        }
        app.exit(0);
        return;
    }

    let Some(engine) = app.try_state::<engine::SharedEngine>() else {
        startup_log::info("duplicate launch while primary is still starting; ignoring");
        return;
    };

    if startup::is_autostart_args(&args) {
        startup_log::info("duplicate autostart launch; keeping tray-only");
        return;
    }

    // A second launch carrying control flags acts on the running instance
    // (for Stream Deck / scripts) instead of opening the window.
    if handle_cli_control(app, &engine, &args) {
        return;
    }

    engine.mark_user_ui_opened();
    if !engine.snapshot().config.headless {
        let _ = flyout::open_default(app);
    }
}

/// Apply control-flag args from a second launch to the running engine.
/// Returns true when at least one control command was handled.
fn handle_cli_control(
    app: &tauri::AppHandle,
    engine: &engine::SharedEngine,
    args: &[String],
) -> bool {
    let set_suspended = |value: bool| {
        engine.update_config(|config| config.suspended = value);
        let _ = config::save(&engine.snapshot().config);
    };
    // Clamp --snooze so a typo can't suppress keepalives for an absurd span.
    const MAX_SNOOZE_MINS: u64 = 24 * 60;
    let mut handled = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--suspend" => set_suspended(true),
            "--resume" => set_suspended(false),
            "--toggle-suspend" => set_suspended(!engine.snapshot().config.suspended),
            "--rescan" => engine.run_detection_cycle(),
            "--snooze" => {
                if let Some(mins) = args.get(i + 1).and_then(|v| v.parse::<u64>().ok()) {
                    engine.snooze(mins.min(MAX_SNOOZE_MINS));
                    i += 1;
                }
            }
            other if other.starts_with("--snooze=") => {
                if let Some(Ok(mins)) = other.strip_prefix("--snooze=").map(str::parse::<u64>) {
                    engine.snooze(mins.min(MAX_SNOOZE_MINS));
                }
            }
            _ => {
                i += 1;
                continue;
            }
        }
        handled = true;
        i += 1;
    }
    if handled {
        let _ = ipc::emit_state(app, engine);
        startup_log::info("applied CLI control command");
    }
    handled
}

fn apply_autostart_preference(engine: engine::SharedEngine) {
    let enabled = engine.snapshot().config.autostart;
    if let Err(error) = ipc::sync_autostart(&engine, enabled) {
        startup_log::warn(format!("autostart sync failed: {error}"));
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

    notifications::deliver(
        app,
        &notifications::QueuedNotice::info("OMNAFK is in your tray. It wakes when a game does."),
    );

    engine.update_config_without_reschedule(|config| config.first_run_notified = true);
    if let Err(error) = config::save(&engine.snapshot().config) {
        startup_log::warn(format!("couldn't save first-run flag: {error}"));
    }
}

struct LaunchUpdateCheckOptions {
    github_repo: String,
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
        match updates::check(&options.github_repo, env!("CARGO_PKG_VERSION")) {
            Ok(check)
                if check.update_available
                    && options.ignored_update_tag.as_deref() != Some(check.latest_tag.as_str()) =>
            {
                engine.set_update_prompt(Some(check.clone()));
                let _ = ipc::emit_state(&app, &engine);

                if matches!(
                    options.update_prompt_mode,
                    config::UpdatePromptMode::Automatic
                ) {
                    // Never yank the app out from under an active keepalive
                    // session; leave the prompt up and install on a later launch.
                    let busy = matches!(
                        engine.snapshot().engine,
                        engine::EngineStatus::Active | engine::EngineStatus::Holding
                    );
                    if busy {
                        startup_log::info(format!(
                            "auto-update {} deferred — keepalive session active",
                            check.latest_tag
                        ));
                    } else {
                        startup_log::info(format!("auto-update installing {}", check.latest_tag));
                        if let Err(error) = ipc::install_pending_update(&app, &engine) {
                            startup_log::warn(format!("auto-update failed: {error}"));
                        }
                    }
                    return;
                }

                let body = format!(
                    "{} is available. Open OMNAFK to update or ignore it.",
                    check.latest_tag
                );
                if matches!(
                    options.update_prompt_mode,
                    config::UpdatePromptMode::CardAndToast
                ) && !matches!(options.notifications, config::NotificationLevel::None)
                {
                    notifications::deliver(&app, &notifications::QueuedNotice::info(body));
                }
            }
            Ok(_) => {}
            Err(error) => startup_log::warn(format!("launch update check failed: {error}")),
        }
    });
}
