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

#[cfg(omnafk_embed_payload)]
const OMNAFK_PAYLOAD: &[u8] = include_bytes!(env!("OMNAFK_PAYLOAD_EXE"));
#[cfg(not(omnafk_embed_payload))]
const OMNAFK_PAYLOAD: &[u8] = &[];

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
        let _ = hidden_command(&temp_exe)
            .arg("--uninstall")
            .arg("--handoff")
            .spawn();
        std::process::exit(0);
    }
}

#[tauri::command]
pub fn setup_state() -> SetupState {
    let mode = if env::args().any(|arg| arg == "--uninstall") {
        "uninstall"
    } else {
        "install"
    };
    let install_dir = registered_install_dir().unwrap_or_else(default_install_dir);
    SetupState {
        mode: mode.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        install_dir: display_path(&install_dir),
        payload_size: payload_size_label(),
        payload_embedded: !OMNAFK_PAYLOAD.is_empty(),
    }
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
        let result = install(&app, &progress, &options);
        match result {
            Ok(()) => emit_progress(&app, &progress, 100, "Install complete.", true, None),
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
        let result = uninstall(&app, &progress, &options);
        match result {
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
    app.exit(0);
}

fn install(
    app: &tauri::AppHandle,
    progress: &ProgressState,
    options: &InstallOptions,
) -> Result<(), String> {
    if OMNAFK_PAYLOAD.is_empty() {
        return Err("This setup build does not contain omnafk.exe. Rebuild with scripts/build-custom-installer.ps1.".to_string());
    }

    let install_dir = PathBuf::from(options.install_dir.trim());
    if install_dir.as_os_str().is_empty() {
        return Err("Choose an install location to continue.".to_string());
    }
    let app_exe = install_dir.join(APP_EXE);
    let setup_exe = install_dir.join(SETUP_EXE);

    emit_progress(app, progress, 8, "Preparing install folder...", false, None);
    stop_running_app();
    fs::create_dir_all(&install_dir).map_err(|error| {
        format!("Couldn't create the install folder - choose another location to fix this: {error}")
    })?;

    emit_progress(app, progress, 28, "Copying omnafk.exe...", false, None);
    write_file_atomic(&app_exe, OMNAFK_PAYLOAD).map_err(|error| {
        format!("Couldn't copy omnafk.exe - close OMNAFK and try again: {error}")
    })?;

    emit_progress(app, progress, 45, "Copying uninstaller...", false, None);
    let current = env::current_exe().map_err(|error| {
        format!("Couldn't locate setup.exe - restart setup to fix this: {error}")
    })?;
    fs::copy(current, &setup_exe).map_err(|error| {
        format!("Couldn't copy the uninstaller - choose a writable folder: {error}")
    })?;

    emit_progress(app, progress, 60, "Writing config...", false, None);
    write_initial_config(options.start_with_windows)?;

    emit_progress(
        app,
        progress,
        72,
        "Registering tray startup...",
        false,
        None,
    );
    if options.start_with_windows {
        write_run_key(&app_exe)?;
    } else {
        delete_run_key();
    }

    emit_progress(app, progress, 84, "Creating shortcuts...", false, None);
    create_start_menu_shortcut(&app_exe, &install_dir)?;
    if options.desktop_shortcut {
        create_desktop_shortcut(&app_exe, &install_dir)?;
    } else {
        delete_desktop_shortcut();
    }

    emit_progress(app, progress, 94, "Registering uninstaller...", false, None);
    write_uninstall_key(&install_dir, &app_exe, &setup_exe)?;

    Ok(())
}

fn uninstall(
    app: &tauri::AppHandle,
    progress: &ProgressState,
    options: &UninstallOptions,
) -> Result<(), String> {
    let install_dir = PathBuf::from(options.install_dir.trim());
    emit_progress(app, progress, 15, "Stopping OMNAFK...", false, None);
    stop_running_app();

    emit_progress(app, progress, 35, "Removing shortcuts...", false, None);
    delete_start_menu_shortcut();
    delete_desktop_shortcut();
    delete_run_key();

    emit_progress(app, progress, 55, "Removing app files...", false, None);
    remove_file_if_exists(&install_dir.join(APP_EXE))?;
    remove_file_if_exists(&install_dir.join(SETUP_EXE))?;
    let _ = fs::remove_dir(&install_dir);

    emit_progress(
        app,
        progress,
        75,
        "Removing uninstall registration...",
        false,
        None,
    );
    delete_uninstall_key();

    if !options.keep_settings {
        emit_progress(app, progress, 90, "Removing settings...", false, None);
        if let Some(config_dir) = dirs::config_dir() {
            let _ = fs::remove_dir_all(config_dir.join(APP_NAME));
        }
    } else {
        emit_progress(app, progress, 90, "Keeping settings...", false, None);
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
    if OMNAFK_PAYLOAD.is_empty() {
        "payload missing".to_string()
    } else {
        let mb = OMNAFK_PAYLOAD.len() as f64 / (1024.0 * 1024.0);
        format!("~{mb:.1} MB on disk")
    }
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
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temp, path)
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
        &format!("{} --uninstall", quote_path(setup_exe)),
    )
    .map_err(reg_error)?;
    key.set_value("NoModify", &1u32).map_err(reg_error)?;
    key.set_value("NoRepair", &1u32).map_err(reg_error)?;
    Ok(())
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
    let _ = hidden_command("taskkill")
        .args(["/IM", APP_EXE, "/F"])
        .status();
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
