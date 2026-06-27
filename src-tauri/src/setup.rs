use crate::{config, installer, updates};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    env, fs, io,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    thread,
    time::Duration,
};
use tauri::{Emitter, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;
use winreg::{enums::*, RegKey};

#[cfg(windows)]
use std::os::windows::{ffi::OsStrExt, process::CommandExt};

// The payload is gzip-compressed at build time (see build.rs) to roughly halve the setup size.
#[cfg(omnafk_embed_payload)]
const OMNAFK_PAYLOAD_GZ: &[u8] = include_bytes!(env!("OMNAFK_PAYLOAD_GZ"));
#[cfg(not(omnafk_embed_payload))]
const OMNAFK_PAYLOAD_GZ: &[u8] = &[];

const PAYLOAD_EMBEDDED: bool = cfg!(omnafk_embed_payload);

// Optional bundled ViGEmBus installer for the Gamepad nudge action.
#[cfg(omnafk_embed_vigem)]
const OMNAFK_VIGEM_GZ: &[u8] = include_bytes!(env!("OMNAFK_VIGEM_GZ"));
#[cfg(not(omnafk_embed_vigem))]
const OMNAFK_VIGEM_GZ: &[u8] = &[];

const VIGEM_EMBEDDED: bool = cfg!(omnafk_embed_vigem);

fn decompress_vigem() -> Result<Vec<u8>, String> {
    use std::io::Read;
    if OMNAFK_VIGEM_GZ.is_empty() {
        return Err("This setup build does not bundle the ViGEmBus driver.".to_string());
    }
    let mut decoder = flate2::read::GzDecoder::new(OMNAFK_VIGEM_GZ);
    let mut raw = Vec::new();
    decoder
        .read_to_end(&mut raw)
        .map_err(|error| format!("Couldn't unpack the ViGEmBus installer: {error}"))?;
    Ok(raw)
}

fn payload_raw_len() -> u64 {
    #[cfg(omnafk_embed_payload)]
    {
        env!("OMNAFK_PAYLOAD_RAW_LEN").parse().unwrap_or(0)
    }
    #[cfg(not(omnafk_embed_payload))]
    {
        0
    }
}

fn decompress_payload() -> Result<Vec<u8>, String> {
    use std::io::Read;
    if OMNAFK_PAYLOAD_GZ.is_empty() {
        return Err("This setup build does not contain omnafk.exe. Rebuild with scripts/build-custom-installer.ps1.".to_string());
    }
    let mut decoder = flate2::read::GzDecoder::new(OMNAFK_PAYLOAD_GZ);
    let mut raw = Vec::with_capacity(payload_raw_len() as usize);
    decoder
        .read_to_end(&mut raw)
        .map_err(|error| format!("Couldn't unpack omnafk.exe from setup: {error}"))?;
    Ok(raw)
}

const APP_NAME: &str = "OMNAFK";
const APP_EXE: &str = "omnafk.exe";
const SETUP_EXE: &str = "omnafk-setup.exe";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\OMNAFK";
const PROGRESS_EVENT: &str = "setup://progress";
const PUBLISHER: &str = "OMNHZN";
const HELP_URL: &str = "https://github.com/OMNHZN/OMNAFK";
const LICENSE_URL: &str = "https://github.com/OMNHZN/OMNAFK/blob/main/LICENSE";
const CREATE_NO_WINDOW: u32 = 0x08000000;
const SETUP_WIDTH: f64 = 560.0;
const SETUP_HEIGHT: f64 = 380.0;

#[derive(Debug, Clone, Serialize)]
pub struct SetupState {
    pub mode: String,
    pub version: String,
    pub install_dir: String,
    pub payload_size: String,
    pub payload_embedded: bool,
    pub installed: bool,
    pub installed_version: Option<String>,
    /// `fresh`, `update`, `reinstall`, or `downgrade` (install mode only).
    pub install_kind: String,
    pub is_downgrade: bool,
    pub changelog_excerpt: Option<String>,
    pub webview2_ok: bool,
    pub license_url: String,
    pub help_url: String,
    pub start_with_windows: bool,
    pub desktop_shortcut: bool,
    /// True when this setup bundles the ViGEmBus driver (gamepad keepalives).
    pub vigem_available: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetupProgress {
    percent: u8,
    message: String,
    done: bool,
    error: Option<String>,
    #[serde(default)]
    warnings: Vec<String>,
}

impl Default for SetupProgress {
    fn default() -> Self {
        Self {
            percent: 0,
            message: "Ready.".to_string(),
            done: false,
            error: None,
            warnings: Vec::new(),
        }
    }
}

#[derive(Clone, Default)]
pub struct ProgressState(Arc<Mutex<SetupProgress>>);

impl ProgressState {
    fn snapshot(&self) -> SetupProgress {
        self.0.lock().clone()
    }

    fn set(&self, progress: SetupProgress) {
        *self.0.lock() = progress;
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct InstallOptions {
    install_dir: String,
    start_with_windows: bool,
    desktop_shortcut: bool,
    #[serde(default)]
    allow_downgrade: bool,
    #[serde(default)]
    install_gamepad_driver: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UninstallOptions {
    install_dir: String,
    keep_settings: bool,
}

pub fn run() {
    let args: Vec<String> = env::args().collect();
    let cli = installer::parse_setup_cli(&args);
    installer::append_install_log(&format!(
        "Setup started (silent={}, uninstall={})",
        cli.silent, cli.uninstall
    ));

    if !acquire_single_instance() {
        installer::show_info_message(
            "OMNAFK Setup",
            "Another copy of OMNAFK Setup is already running.",
        );
        std::process::exit(1);
    }

    if cli.silent {
        let ok = run_silent(&cli);
        std::process::exit(if ok { 0 } else { 1 });
    }

    if !installer::webview2_installed() {
        installer::show_info_message(
            "OMNAFK Setup",
            "Microsoft Edge WebView2 Runtime is required.\n\nInstall it from Microsoft, then run setup again.",
        );
        std::process::exit(1);
    }

    tauri::Builder::default()
        .manage(ProgressState::default())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let window =
                WebviewWindowBuilder::new(app, "setup", WebviewUrl::App("setup.html".into()))
                    .title("OMNAFK Setup")
                    .inner_size(SETUP_WIDTH, SETUP_HEIGHT)
                    .min_inner_size(SETUP_WIDTH, SETUP_HEIGHT)
                    .max_inner_size(SETUP_WIDTH, SETUP_HEIGHT)
                    .resizable(false)
                    .decorations(false)
                    .transparent(true)
                    .skip_taskbar(false)
                    .shadow(false)
                    .visible(false)
                    .build()?;
            window.center()?;
            window.show()?;
            window.set_focus()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            setup_state,
            setup_progress,
            browse_install_dir,
            validate_install_dir,
            start_install,
            finish_install,
            start_uninstall,
            exit_setup,
            open_setup_url
        ])
        .run(tauri::generate_context!("tauri.setup.conf.json"))
        .expect("failed to run OMNAFK Setup");
}

pub fn handoff_uninstaller_if_needed() {
    let args: Vec<String> = env::args().collect();
    if !args.iter().any(|arg| arg == "--uninstall") || args.iter().any(|arg| arg == "--handoff") {
        return;
    }

    let Ok(current_exe) = env::current_exe() else {
        return;
    };
    let Some(install_dir) = registered_install_dir() else {
        return;
    };
    if !current_exe.starts_with(&install_dir) {
        return;
    }

    let temp_exe = env::temp_dir().join("OMNAFK-Uninstall.exe");
    if fs::copy(&current_exe, &temp_exe).is_ok() {
        let mut command = hidden_command(&temp_exe);
        command.arg("--uninstall").arg("--handoff");
        if args.iter().any(|arg| arg == "--silent" || arg == "/S") {
            command.arg("--silent");
        }
        let _ = command.spawn();
        std::process::exit(0);
    }
}

/// Headless install/uninstall for `--silent` runs (e.g. quiet uninstall from
/// Windows Settings, or a future in-app updater). Uses the registered install
/// location and the user's existing preferences unless CLI flags override them.
fn run_silent(cli: &installer::SetupCli) -> bool {
    let report = |_: u8, _: &str| {};
    let install_dir = cli
        .install_dir
        .as_ref()
        .map(|path| PathBuf::from(path.trim()))
        .or_else(registered_install_dir)
        .unwrap_or_else(default_install_dir);

    if cli.uninstall {
        let keep_settings = cli.keep_settings.unwrap_or(true);
        let options = UninstallOptions {
            install_dir: display_path(&install_dir),
            keep_settings,
        };
        let ok = uninstall(&report, &options).is_ok();
        if ok {
            installer::append_install_log("Silent uninstall finished.");
        } else {
            installer::append_install_log("Silent uninstall failed.");
        }
        if ok && cli.handoff {
            schedule_self_delete();
        }
        return ok;
    }

    let start_with_windows = if cli.no_autostart {
        false
    } else {
        config::load().map(|cfg| cfg.autostart).unwrap_or(true)
    };
    let desktop_shortcut = cli.desktop_shortcut.unwrap_or_else(|| {
        dirs::desktop_dir()
            .map(|desktop| desktop.join("OMNAFK.lnk").exists())
            .unwrap_or(false)
    });
    let options = InstallOptions {
        install_dir: display_path(&install_dir),
        start_with_windows,
        desktop_shortcut,
        allow_downgrade: cli.allow_downgrade,
        // Unattended installs don't silently add a kernel driver; the
        // interactive installer offers it, or install ViGEmBus separately.
        install_gamepad_driver: false,
    };
    match install(&report, &options) {
        Ok(_) => {
            installer::append_install_log("Silent install finished.");
            true
        }
        Err(error) => {
            installer::append_install_log(&format!("Silent install failed: {error}"));
            false
        }
    }
}

#[tauri::command]
pub fn setup_state() -> SetupState {
    let mode = if env::args().any(|arg| arg == "--uninstall") {
        "uninstall"
    } else {
        "install"
    };
    let registered = registered_install_dir();
    let installed = registered.is_some();
    let install_dir = registered.unwrap_or_else(default_install_dir);
    let installed_version = registered_version();
    let install_kind = if mode == "uninstall" {
        "uninstall".to_string()
    } else if !installed {
        "fresh".to_string()
    } else {
        match installed_version.as_deref() {
            Some(installed) => {
                match updates::compare_versions(env!("CARGO_PKG_VERSION"), installed) {
                    Ordering::Greater => "update".to_string(),
                    Ordering::Equal => "reinstall".to_string(),
                    Ordering::Less => "downgrade".to_string(),
                }
            }
            None => "reinstall".to_string(),
        }
    };
    let is_downgrade = install_kind == "downgrade";
    let changelog_excerpt = if install_kind == "update" || is_downgrade {
        let repo = config::load()
            .map(|cfg| cfg.github_repo)
            .unwrap_or_else(|_| "OMNHZN/OMNAFK".to_string());
        updates::release_notes_excerpt(
            &repo,
            &format!("v{}", env!("CARGO_PKG_VERSION")),
            env!("CARGO_PKG_VERSION"),
        )
    } else {
        None
    };
    SetupState {
        mode: mode.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        install_dir: display_path(&install_dir),
        payload_size: payload_size_label(),
        payload_embedded: PAYLOAD_EMBEDDED,
        installed,
        installed_version,
        install_kind,
        is_downgrade,
        changelog_excerpt,
        webview2_ok: installer::webview2_installed(),
        license_url: LICENSE_URL.to_string(),
        help_url: HELP_URL.to_string(),
        // Pre-fill toggles from the existing install so an update doesn't
        // silently reset the user's choices.
        start_with_windows: if installed {
            config::load().map(|cfg| cfg.autostart).unwrap_or(true)
        } else {
            true
        },
        desktop_shortcut: dirs::desktop_dir()
            .map(|desktop| desktop.join("OMNAFK.lnk").exists())
            .unwrap_or(false),
        vigem_available: VIGEM_EMBEDDED,
    }
}

#[tauri::command]
pub fn validate_install_dir(path: String) -> Option<String> {
    install_dir_problem(Path::new(path.trim()))
}

#[tauri::command]
pub fn setup_progress(progress: State<ProgressState>) -> SetupProgress {
    progress.snapshot()
}

#[tauri::command]
pub fn browse_install_dir(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let Some(folder) = app.dialog().file().blocking_pick_folder() else {
        return Ok(None);
    };
    folder
        .into_path()
        .map(|path| Some(display_path(&path)))
        .map_err(|error| {
            format!("Couldn't use that folder - choose a local install location: {error}")
        })
}

#[tauri::command]
pub fn start_install(
    app: tauri::AppHandle,
    progress: State<ProgressState>,
    options: InstallOptions,
) -> Result<(), String> {
    let progress = progress.inner().clone();
    emit_progress(
        &app,
        &progress,
        0,
        "Preparing install...",
        false,
        None,
        vec![],
    );
    thread::spawn(move || {
        let report = |percent: u8, message: &str| {
            emit_progress(&app, &progress, percent, message, false, None, vec![]);
        };
        match install(&report, &options) {
            Ok(warnings) => emit_progress(
                &app,
                &progress,
                100,
                "Install complete.",
                true,
                None,
                warnings,
            ),
            Err(error) => emit_progress(
                &app,
                &progress,
                100,
                "Install failed.",
                true,
                Some(error),
                vec![],
            ),
        }
    });
    Ok(())
}

#[tauri::command]
pub fn finish_install(
    app: tauri::AppHandle,
    install_dir: String,
    launch: bool,
) -> Result<(), String> {
    if launch {
        let exe = PathBuf::from(install_dir).join(APP_EXE);
        hidden_command(&exe)
            .arg("--post-install")
            .spawn()
            .map_err(|error| {
                format!("Couldn't launch OMNAFK - open it from the Start menu to fix this: {error}")
            })?;
        let mut started = false;
        for _ in 0..50 {
            if app_running() {
                started = true;
                break;
            }
            thread::sleep(Duration::from_millis(200));
        }
        if !started {
            return Err(
                "OMNAFK didn't start after install. Launch it from the Start menu.".to_string(),
            );
        }
        installer::append_install_log("Launch verified after install.");
    }
    app.exit(0);
    Ok(())
}

#[tauri::command]
pub fn start_uninstall(
    app: tauri::AppHandle,
    progress: State<ProgressState>,
    options: UninstallOptions,
) -> Result<(), String> {
    let progress = progress.inner().clone();
    emit_progress(
        &app,
        &progress,
        0,
        "Preparing removal...",
        false,
        None,
        vec![],
    );
    thread::spawn(move || {
        let report = |percent: u8, message: &str| {
            emit_progress(&app, &progress, percent, message, false, None, vec![]);
        };
        match uninstall(&report, &options) {
            Ok(()) => emit_progress(
                &app,
                &progress,
                100,
                "Uninstall complete.",
                true,
                None,
                vec![],
            ),
            Err(error) => emit_progress(
                &app,
                &progress,
                100,
                "Uninstall failed.",
                true,
                Some(error),
                vec![],
            ),
        }
    });
    Ok(())
}

#[tauri::command]
pub fn open_setup_url(url: String) -> Result<(), String> {
    crate::updates::open_url(&url)
}

#[tauri::command]
pub fn exit_setup(app: tauri::AppHandle) {
    // The hand-off uninstaller lives in %TEMP%; clean it up once we're gone.
    if env::args().any(|arg| arg == "--handoff") {
        schedule_self_delete();
    }
    app.exit(0);
}

fn install(report: &dyn Fn(u8, &str), options: &InstallOptions) -> Result<Vec<String>, String> {
    installer::append_install_log(&format!(
        "Install to {} (allow_downgrade={})",
        options.install_dir.trim(),
        options.allow_downgrade
    ));

    if let Some(installed) = registered_version() {
        let ordering = updates::compare_versions(env!("CARGO_PKG_VERSION"), &installed);
        if ordering == Ordering::Less && !options.allow_downgrade {
            return Err(
                "This setup is older than the installed version. Confirm the downgrade to continue."
                    .to_string(),
            );
        }
    }

    let install_dir = PathBuf::from(options.install_dir.trim());
    if let Some(problem) = install_dir_problem(&install_dir) {
        return Err(problem);
    }
    let app_exe = install_dir.join(APP_EXE);
    let setup_exe = install_dir.join(SETUP_EXE);
    // Non-fatal steps collect warnings instead of aborting a half-finished install.
    let mut warnings: Vec<String> = Vec::new();

    report(4, "Unpacking omnafk.exe...");
    let payload = decompress_payload()?;
    verify_payload_integrity(&payload)?;

    report(12, "Stopping OMNAFK...");
    ensure_app_stopped()?;

    report(20, "Preparing install folder...");
    fs::create_dir_all(&install_dir).map_err(|error| {
        format!("Couldn't create the install folder - choose another location to fix this: {error}")
    })?;

    report(34, "Copying omnafk.exe...");
    write_file_atomic(&app_exe, &payload).map_err(|error| {
        format!("Couldn't copy omnafk.exe - close OMNAFK and try again: {error}")
    })?;

    report(48, "Copying uninstaller...");
    let current = env::current_exe().map_err(|error| {
        format!("Couldn't locate setup.exe - restart setup to fix this: {error}")
    })?;
    retry_io(|| fs::copy(&current, &setup_exe).map(|_| ())).map_err(|error| {
        format!("Couldn't copy the uninstaller - choose a writable folder: {error}")
    })?;

    report(60, "Writing config...");
    if let Err(error) = write_initial_config(options.start_with_windows) {
        warnings.push(format!("Settings weren't written ({error})."));
    }

    report(72, "Registering tray startup...");
    if let Err(error) = crate::startup::set_start_with_windows(options.start_with_windows, &app_exe)
    {
        warnings.push(format!("Start with Windows wasn't updated ({error})."));
    }

    report(84, "Creating shortcuts...");
    if let Err(error) = create_start_menu_shortcut(&app_exe, &install_dir) {
        warnings.push(format!("The Start menu shortcut wasn't created ({error})."));
    }
    if options.desktop_shortcut {
        if let Err(error) = create_desktop_shortcut(&app_exe, &install_dir) {
            warnings.push(format!("The desktop shortcut wasn't created ({error})."));
        }
    } else {
        delete_desktop_shortcut();
    }

    if options.install_gamepad_driver && VIGEM_EMBEDDED {
        report(90, "Installing gamepad driver (ViGEmBus)...");
        if let Err(error) = install_gamepad_driver() {
            warnings.push(format!("The gamepad driver wasn't installed ({error})."));
        }
    }

    report(94, "Registering uninstaller...");
    write_uninstall_key(&install_dir, &app_exe, &setup_exe)?;

    installer::append_install_log("Install finished.");
    Ok(warnings)
}

/// Extract the bundled ViGEmBus installer to a temp file and run it silently.
/// The WiX bundle elevates itself (a UAC prompt may appear) and is a no-op when
/// a current-or-newer driver is already present.
fn install_gamepad_driver() -> Result<(), String> {
    let installer_bytes = decompress_vigem()?;
    let temp = env::temp_dir().join("OMNAFK-ViGEmBus-Setup.exe");
    write_file_atomic(&temp, &installer_bytes)
        .map_err(|error| format!("couldn't unpack the installer: {error}"))?;

    installer::append_install_log("Running bundled ViGEmBus installer.");
    let status = hidden_command(&temp)
        .args(["/quiet", "/norestart"])
        .status()
        .map_err(|error| format!("couldn't launch the installer: {error}"))?;

    let _ = fs::remove_file(&temp);

    // 0 = installed, 3010 = success but a reboot is recommended.
    match status.code() {
        Some(0) | Some(3010) => Ok(()),
        Some(code) => Err(format!("installer exited with code {code}")),
        None => Err("installer was terminated".to_string()),
    }
}

fn uninstall(report: &dyn Fn(u8, &str), options: &UninstallOptions) -> Result<(), String> {
    installer::append_install_log(&format!(
        "Uninstall from {} (keep_settings={})",
        options.install_dir.trim(),
        options.keep_settings
    ));
    let install_dir = PathBuf::from(options.install_dir.trim());
    report(15, "Stopping OMNAFK...");
    ensure_app_stopped()?;

    report(35, "Removing shortcuts...");
    delete_start_menu_shortcut();
    delete_desktop_shortcut();
    crate::startup::set_start_with_windows(false, &install_dir.join(APP_EXE))?;

    report(55, "Removing app files...");
    remove_file_if_exists(&install_dir.join(APP_EXE))?;
    remove_file_if_exists(&install_dir.join(SETUP_EXE))?;
    let _ = fs::remove_dir(&install_dir);

    report(75, "Removing uninstall registration...");
    delete_uninstall_key();

    if !options.keep_settings {
        report(90, "Removing settings...");
        if let Some(config_dir) = dirs::config_dir() {
            let _ = fs::remove_dir_all(config_dir.join(APP_NAME));
        }
    } else {
        report(90, "Keeping settings...");
    }

    installer::append_install_log("Uninstall finished.");
    Ok(())
}

fn emit_progress(
    app: &tauri::AppHandle,
    progress: &ProgressState,
    percent: u8,
    message: &str,
    done: bool,
    error: Option<String>,
    warnings: Vec<String>,
) {
    let payload = SetupProgress {
        percent,
        message: message.to_string(),
        done,
        error,
        warnings,
    };
    progress.set(payload.clone());
    let _ = app.emit(PROGRESS_EVENT, payload);
    thread::sleep(Duration::from_millis(40));
}

fn payload_size_label() -> String {
    if !PAYLOAD_EMBEDDED {
        "payload missing".to_string()
    } else {
        let mb = payload_raw_len() as f64 / (1024.0 * 1024.0);
        format!("~{mb:.1} MB on disk")
    }
}

/// Why the given directory can't be installed to, or None when it's fine.
fn install_dir_problem(path: &Path) -> Option<String> {
    let raw = path.as_os_str().to_string_lossy();
    if raw.trim().is_empty() {
        return Some("Choose an install location to continue.".to_string());
    }
    if raw.starts_with(r"\\") {
        return Some("Network folders aren't supported - pick a folder on this PC.".to_string());
    }
    if !path.is_absolute() {
        return Some(r"Use a full path like C:\Users\you\Apps\OMNAFK.".to_string());
    }
    if path.parent().is_none() {
        return Some("Pick a folder, not a drive root.".to_string());
    }
    for var in [
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramW6432",
        "windir",
    ] {
        if let Ok(protected) = env::var(var) {
            if !protected.is_empty() && path_starts_with_ignore_case(path, Path::new(&protected)) {
                return Some(
                    "That folder needs administrator rights - pick a per-user folder like the default."
                        .to_string(),
                );
            }
        }
    }
    if let Some(free) = free_disk_bytes(path) {
        // App exe + bundled uninstaller + headroom for temp files.
        let needed = payload_raw_len() + OMNAFK_PAYLOAD_GZ.len() as u64 + 50 * 1024 * 1024;
        if free < needed {
            return Some(format!(
                "Not enough free space on that drive (about {} MB needed).",
                needed / (1024 * 1024)
            ));
        }
    }
    None
}

fn path_starts_with_ignore_case(path: &Path, base: &Path) -> bool {
    let path = path.to_string_lossy().to_lowercase();
    let base = base.to_string_lossy().to_lowercase();
    path == base || path.starts_with(&format!("{base}\\"))
}

#[cfg(windows)]
fn free_disk_bytes(path: &Path) -> Option<u64> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    // The target folder may not exist yet; probe the closest existing ancestor.
    let mut probe = path;
    while !probe.exists() {
        probe = probe.parent()?;
    }
    let wide = wide_path(probe);
    let mut free = 0u64;
    unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut free), None, None).ok()? };
    Some(free)
}

#[cfg(not(windows))]
fn free_disk_bytes(_path: &Path) -> Option<u64> {
    None
}

/// Refuses to start a second copy of setup (two installers racing each other
/// can corrupt the install). The mutex handle is never closed, so it lives
/// for the whole process.
#[cfg(windows)]
fn acquire_single_instance() -> bool {
    use windows::core::w;
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;

    unsafe {
        match CreateMutexW(None, true, w!("Local\\OMNAFK-Setup-Singleton")) {
            Ok(_handle) => GetLastError() != ERROR_ALREADY_EXISTS,
            Err(_) => true,
        }
    }
}

#[cfg(not(windows))]
fn acquire_single_instance() -> bool {
    true
}

/// Deletes this exe a few seconds after the process exits (used by the
/// temp-copied uninstaller so it doesn't linger in %TEMP% forever).
fn schedule_self_delete() {
    let Ok(exe) = env::current_exe() else {
        return;
    };
    let _ = hidden_command("cmd")
        .args([
            "/C",
            &format!(
                "ping 127.0.0.1 -n 4 > nul & del /F /Q \"{}\"",
                display_path(&exe)
            ),
        ])
        .spawn();
}

fn default_install_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(env::temp_dir)
        .join(APP_NAME)
}

fn write_initial_config(start_with_windows: bool) -> Result<(), String> {
    let mut cfg = config::load().unwrap_or_default();
    cfg.autostart = start_with_windows;
    let path = dirs::config_dir()
        .ok_or_else(|| {
            "Couldn't find %APPDATA% - choose a normal Windows user account to fix this."
                .to_string()
        })?
        .join(APP_NAME)
        .join("config.json");
    config::save_to_path(&cfg, &path)
        .map_err(|error| format!("Couldn't write config - check %APPDATA% permissions: {error}"))
}

fn verify_payload_integrity(payload: &[u8]) -> Result<(), String> {
    #[cfg(omnafk_embed_payload)]
    {
        let expected = env!("OMNAFK_PAYLOAD_SHA256");
        installer::verify_sha256(payload, expected)?;
    }
    #[cfg(not(omnafk_embed_payload))]
    {
        let _ = payload;
    }
    Ok(())
}

fn ensure_app_stopped() -> Result<(), String> {
    stop_running_app();
    if app_running() {
        return Err("OMNAFK is still running. Close it from the tray and try again.".to_string());
    }
    Ok(())
}

fn write_file_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let backup = path.with_file_name(format!(
        "{}.bak",
        path.file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_default()
    ));
    if path.exists() {
        let _ = fs::remove_file(&backup);
        fs::copy(path, &backup)?;
    }

    let temp = path.with_extension("tmp");
    let result = (|| {
        fs::write(&temp, bytes)?;
        retry_io(|| {
            if path.exists() {
                fs::remove_file(path)?;
            }
            fs::rename(&temp, path)
        })
    })();

    match result {
        Ok(()) => {
            let _ = fs::remove_file(&backup);
            Ok(())
        }
        Err(error) => {
            if backup.exists() {
                let _ = fs::remove_file(path);
                let _ = fs::rename(&backup, path);
            }
            let _ = fs::remove_file(&temp);
            Err(error)
        }
    }
}

fn retry_io<T>(mut operation: impl FnMut() -> io::Result<T>) -> io::Result<T> {
    let mut last_error = None;
    for attempt in 0..10 {
        if attempt > 0 {
            thread::sleep(Duration::from_millis(250));
        }
        match operation() {
            Ok(value) => return Ok(value),
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.unwrap_or_else(|| io::Error::other("retry failed")))
}

fn write_uninstall_key(install_dir: &Path, app_exe: &Path, setup_exe: &Path) -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(UNINSTALL_KEY)
        .map_err(|error| format!("Couldn't register the uninstaller: {error}"))?;
    key.set_value("DisplayName", &APP_NAME).map_err(reg_error)?;
    key.set_value("DisplayVersion", &env!("CARGO_PKG_VERSION"))
        .map_err(reg_error)?;
    key.set_value("Publisher", &PUBLISHER).map_err(reg_error)?;
    key.set_value("HelpLink", &HELP_URL).map_err(reg_error)?;
    key.set_value("URLInfoAbout", &HELP_URL)
        .map_err(reg_error)?;
    key.set_value("InstallLocation", &display_path(install_dir))
        .map_err(reg_error)?;
    key.set_value("DisplayIcon", &display_path(app_exe))
        .map_err(reg_error)?;
    key.set_value(
        "UninstallString",
        &format!("{} --uninstall", quote_path(setup_exe)),
    )
    .map_err(reg_error)?;
    key.set_value(
        "QuietUninstallString",
        &format!("{} --uninstall --silent", quote_path(setup_exe)),
    )
    .map_err(reg_error)?;
    // Shown as the app size in Windows Settings -> Apps (value is in KB).
    let setup_len = fs::metadata(setup_exe).map(|meta| meta.len()).unwrap_or(0);
    let estimated_kb = ((payload_raw_len() + setup_len) / 1024) as u32;
    key.set_value("EstimatedSize", &estimated_kb)
        .map_err(reg_error)?;
    key.set_value(
        "InstallDate",
        &chrono::Local::now().format("%Y%m%d").to_string(),
    )
    .map_err(reg_error)?;
    key.set_value("NoModify", &1u32).map_err(reg_error)?;
    key.set_value("NoRepair", &1u32).map_err(reg_error)?;
    Ok(())
}

fn registered_version() -> Option<String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu.open_subkey_with_flags(UNINSTALL_KEY, KEY_READ).ok()?;
    key.get_value("DisplayVersion").ok()
}

fn delete_uninstall_key() {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let _ = hkcu.delete_subkey_all(UNINSTALL_KEY);
}

fn registered_install_dir() -> Option<PathBuf> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu.open_subkey_with_flags(UNINSTALL_KEY, KEY_READ).ok()?;
    let value: String = key.get_value("InstallLocation").ok()?;
    Some(PathBuf::from(value))
}

fn create_start_menu_shortcut(app_exe: &Path, working_dir: &Path) -> Result<(), String> {
    let dir = start_menu_dir();
    fs::create_dir_all(&dir)
        .map_err(|error| format!("Couldn't create the Start menu folder: {error}"))?;
    create_shortcut(&dir.join("OMNAFK.lnk"), app_exe, working_dir)
}

fn delete_start_menu_shortcut() {
    let dir = start_menu_dir();
    let _ = fs::remove_file(dir.join("OMNAFK.lnk"));
    let _ = fs::remove_dir(dir);
}

fn create_desktop_shortcut(app_exe: &Path, working_dir: &Path) -> Result<(), String> {
    let Some(desktop) = dirs::desktop_dir() else {
        return Ok(());
    };
    create_shortcut(&desktop.join("OMNAFK.lnk"), app_exe, working_dir)
}

fn delete_desktop_shortcut() {
    if let Some(desktop) = dirs::desktop_dir() {
        let _ = fs::remove_file(desktop.join("OMNAFK.lnk"));
    }
}

fn start_menu_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(env::temp_dir)
        .join(r"Microsoft\Windows\Start Menu\Programs\OMNAFK")
}

#[cfg(windows)]
fn create_shortcut(shortcut: &Path, target: &Path, working_dir: &Path) -> Result<(), String> {
    use windows::{
        core::{Interface, PCWSTR},
        Win32::{
            System::Com::{
                CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile,
                CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED,
            },
            UI::Shell::{IShellLinkW, ShellLink},
        },
    };

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
            .map_err(|error| format!("Couldn't create a Windows shortcut object: {error}"))?;
        let target_w = wide_path(target);
        let work_w = wide_path(working_dir);
        let desc_w = wide_str("OMNAFK");
        link.SetPath(PCWSTR(target_w.as_ptr()))
            .map_err(|error| format!("Couldn't set shortcut target: {error}"))?;
        link.SetWorkingDirectory(PCWSTR(work_w.as_ptr()))
            .map_err(|error| format!("Couldn't set shortcut folder: {error}"))?;
        link.SetDescription(PCWSTR(desc_w.as_ptr()))
            .map_err(|error| format!("Couldn't set shortcut description: {error}"))?;
        link.SetIconLocation(PCWSTR(target_w.as_ptr()), 0)
            .map_err(|error| format!("Couldn't set shortcut icon: {error}"))?;

        let persist: IPersistFile = link
            .cast()
            .map_err(|error| format!("Couldn't save the shortcut: {error}"))?;
        let shortcut_w = wide_path(shortcut);
        let result = persist.Save(PCWSTR(shortcut_w.as_ptr()), true);
        CoUninitialize();
        result.map_err(|error| format!("Couldn't write the shortcut: {error}"))
    }
}

#[cfg(not(windows))]
fn create_shortcut(_shortcut: &Path, _target: &Path, _working_dir: &Path) -> Result<(), String> {
    Ok(())
}

fn stop_running_app() {
    // Polite close first (lets OMNAFK flush stats/config), force only if it
    // doesn't exit within the grace period.
    let _ = hidden_command("taskkill").args(["/IM", APP_EXE]).status();
    for _ in 0..10 {
        if !app_running() {
            return;
        }
        thread::sleep(Duration::from_millis(200));
    }
    let _ = hidden_command("taskkill")
        .args(["/IM", APP_EXE, "/F"])
        .status();
    thread::sleep(Duration::from_millis(300));
}

fn app_running() -> bool {
    hidden_command("tasklist")
        .args(["/FI", &format!("IMAGENAME eq {APP_EXE}"), "/NH"])
        .output()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .to_lowercase()
                .contains(APP_EXE)
        })
        .unwrap_or(false)
}

fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    command
}

fn remove_file_if_exists(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Couldn't remove {}: {error}", display_path(path))),
    }
}

fn reg_error(error: io::Error) -> String {
    format!("Couldn't write uninstall registration: {error}")
}

fn quote_path(path: &Path) -> String {
    format!("\"{}\"", display_path(path))
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

#[cfg(windows)]
fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(windows)]
fn wide_str(value: &str) -> Vec<u16> {
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(Some(0))
        .collect()
}
