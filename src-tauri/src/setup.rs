use crate::config;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::{
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
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\OMNAFK";
const PROGRESS_EVENT: &str = "setup://progress";
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
    pub start_with_windows: bool,
    pub desktop_shortcut: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SetupProgress {
    percent: u8,
    message: String,
    done: bool,
    error: Option<String>,
}

impl Default for SetupProgress {
    fn default() -> Self {
        Self {
            percent: 0,
            message: "Ready.".to_string(),
            done: false,
            error: None,
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
}

#[derive(Debug, Clone, Deserialize)]
pub struct UninstallOptions {
    install_dir: String,
    keep_settings: bool,
}

pub fn run() {
    if !acquire_single_instance() {
        return;
    }
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|arg| arg == "--silent" || arg == "/S") {
        run_silent(args.iter().any(|arg| arg == "--uninstall"));
        return;
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
            exit_setup
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
/// location and the user's existing preferences.
fn run_silent(uninstall: bool) {
    let report = |_: u8, _: &str| {};
    let install_dir = registered_install_dir().unwrap_or_else(default_install_dir);
    if uninstall {
        let options = UninstallOptions {
            install_dir: display_path(&install_dir),
            keep_settings: true,
        };
        let _ = self::uninstall(&report, &options);
        if env::args().any(|arg| arg == "--handoff") {
            schedule_self_delete();
        }
    } else {
        let options = InstallOptions {
            install_dir: display_path(&install_dir),
            start_with_windows: config::load().map(|cfg| cfg.autostart).unwrap_or(true),
            desktop_shortcut: dirs::desktop_dir()
                .map(|desktop| desktop.join("OMNAFK.lnk").exists())
                .unwrap_or(false),
        };
        let _ = install(&report, &options);
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
    SetupState {
        mode: mode.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        install_dir: display_path(&install_dir),
        payload_size: payload_size_label(),
        payload_embedded: PAYLOAD_EMBEDDED,
        installed,
        installed_version: registered_version(),
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
    emit_progress(&app, &progress, 0, "Preparing install...", false, None);
    thread::spawn(move || {
        let report = |percent: u8, message: &str| {
            emit_progress(&app, &progress, percent, message, false, None);
        };
        match install(&report, &options) {
            Ok(warnings) => {
                let message = if warnings.is_empty() {
                    "Install complete.".to_string()
                } else {
                    format!("Installed, but: {}", warnings.join(" "))
                };
                emit_progress(&app, &progress, 100, &message, true, None);
            }
            Err(error) => emit_progress(&app, &progress, 100, "Install failed.", true, Some(error)),
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
        hidden_command(&exe).spawn().map_err(|error| {
            format!("Couldn't launch OMNAFK - open it from the Start menu to fix this: {error}")
        })?;
        thread::sleep(Duration::from_millis(150));
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
    emit_progress(&app, &progress, 0, "Preparing removal...", false, None);
    thread::spawn(move || {
        let report = |percent: u8, message: &str| {
            emit_progress(&app, &progress, percent, message, false, None);
        };
        match uninstall(&report, &options) {
            Ok(()) => emit_progress(&app, &progress, 100, "Uninstall complete.", true, None),
            Err(error) => {
                emit_progress(&app, &progress, 100, "Uninstall failed.", true, Some(error))
            }
        }
    });
    Ok(())
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

    report(12, "Stopping OMNAFK...");
    stop_running_app();

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
    if options.start_with_windows {
        if let Err(error) = write_run_key(&app_exe) {
            warnings.push(format!("Start with Windows wasn't enabled ({error})."));
        }
    } else {
        delete_run_key();
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

    report(94, "Registering uninstaller...");
    write_uninstall_key(&install_dir, &app_exe, &setup_exe)?;

    Ok(warnings)
}

fn uninstall(report: &dyn Fn(u8, &str), options: &UninstallOptions) -> Result<(), String> {
    let install_dir = PathBuf::from(options.install_dir.trim());
    report(15, "Stopping OMNAFK...");
    stop_running_app();

    report(35, "Removing shortcuts...");
    delete_start_menu_shortcut();
    delete_desktop_shortcut();
    delete_run_key();

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

    Ok(())
}

fn emit_progress(
    app: &tauri::AppHandle,
    progress: &ProgressState,
    percent: u8,
    message: &str,
    done: bool,
    error: Option<String>,
) {
    let payload = SetupProgress {
        percent,
        message: message.to_string(),
        done,
        error,
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

fn write_file_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temp = path.with_extension("tmp");
    fs::write(&temp, bytes)?;
    // Windows can keep the old exe locked for a moment after taskkill,
    // so the swap is retried instead of failing the whole install.
    retry_io(|| {
        if path.exists() {
            fs::remove_file(path)?;
        }
        fs::rename(&temp, path)
    })
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

fn write_run_key(app_exe: &Path) -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(RUN_KEY)
        .map_err(|error| format!("Couldn't open Windows startup settings: {error}"))?;
    key.set_value(APP_NAME, &quote_path(app_exe))
        .map_err(|error| format!("Couldn't register Start with Windows: {error}"))
}

fn delete_run_key() {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE) {
        let _ = key.delete_value(APP_NAME);
    }
}

fn write_uninstall_key(install_dir: &Path, app_exe: &Path, setup_exe: &Path) -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu
        .create_subkey(UNINSTALL_KEY)
        .map_err(|error| format!("Couldn't register the uninstaller: {error}"))?;
    key.set_value("DisplayName", &APP_NAME).map_err(reg_error)?;
    key.set_value("DisplayVersion", &env!("CARGO_PKG_VERSION"))
        .map_err(reg_error)?;
    key.set_value("Publisher", &APP_NAME).map_err(reg_error)?;
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
