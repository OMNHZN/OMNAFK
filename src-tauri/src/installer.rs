use std::{fs::OpenOptions, io::Write, path::PathBuf};

pub const WEBVIEW2_INSTALL_URL: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";
const WEBVIEW2_CLIENT_KEY: &str =
    r"Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";

#[derive(Debug, Clone, Default)]
pub struct SetupCli {
    pub silent: bool,
    pub uninstall: bool,
    pub handoff: bool,
    pub install_dir: Option<String>,
    pub no_autostart: bool,
    pub desktop_shortcut: Option<bool>,
    pub keep_settings: Option<bool>,
    pub allow_downgrade: bool,
}

pub fn parse_setup_cli(args: &[String]) -> SetupCli {
    let mut cli = SetupCli::default();
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--silent" | "/S" => cli.silent = true,
            "--uninstall" => cli.uninstall = true,
            "--handoff" => cli.handoff = true,
            "--no-autostart" => cli.no_autostart = true,
            "--desktop-shortcut" => {
                cli.desktop_shortcut = Some(true);
            }
            "--no-desktop-shortcut" => {
                cli.desktop_shortcut = Some(false);
            }
            "--keep-settings" => {
                cli.keep_settings = Some(true);
            }
            "--no-keep-settings" => {
                cli.keep_settings = Some(false);
            }
            "--allow-downgrade" => {
                cli.allow_downgrade = true;
            }
            "--install-dir" if index + 1 < args.len() => {
                index += 1;
                cli.install_dir = Some(args[index].clone());
            }
            _ => {}
        }
        index += 1;
    }
    cli
}

pub fn append_install_log(message: &str) {
    let Some(config_dir) = dirs::config_dir() else {
        return;
    };
    let path = config_dir.join("OMNAFK").join("install.log");
    if std::fs::create_dir_all(path.parent().unwrap()).is_err() {
        return;
    }
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(file, "[{timestamp}] {message}");
    }
}

#[cfg(windows)]
pub fn webview2_installed() -> bool {
    use winreg::{enums::*, RegKey};

    let roots = [
        RegKey::predef(HKEY_LOCAL_MACHINE),
        RegKey::predef(HKEY_CURRENT_USER),
    ];
    for root in roots {
        for subkey in [
            WEBVIEW2_CLIENT_KEY,
            r"Software\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}",
        ] {
            if let Ok(key) = root.open_subkey_with_flags(subkey, KEY_READ) {
                if key.get_value::<String, _>("pv").is_ok() {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(not(windows))]
pub fn webview2_installed() -> bool {
    true
}

#[cfg(windows)]
pub fn show_info_message(title: &str, text: &str) {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};

    let title_w: Vec<u16> = std::ffi::OsStr::new(title)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let text_w: Vec<u16> = std::ffi::OsStr::new(text)
        .encode_wide()
        .chain(Some(0))
        .collect();
    unsafe {
        let _ = MessageBoxW(
            None,
            PCWSTR(text_w.as_ptr()),
            PCWSTR(title_w.as_ptr()),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}

#[cfg(not(windows))]
pub fn show_info_message(_title: &str, text: &str) {
    eprintln!("{text}");
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn verify_sha256(bytes: &[u8], expected_hex: &str) -> Result<(), String> {
    let actual = sha256_hex(bytes);
    let expected = expected_hex.trim().to_ascii_lowercase();
    if actual == expected {
        Ok(())
    } else {
        Err("Downloaded installer failed the integrity check.".to_string())
    }
}

pub fn parse_sha256_sidecar(text: &str) -> Option<String> {
    text.lines()
        .next()
        .and_then(|line| line.split_whitespace().next())
        .map(|hash| hash.trim().to_ascii_lowercase())
        .filter(|hash| hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()))
}

pub fn setup_sha256_url(asset_url: &str) -> String {
    format!("{asset_url}.sha256")
}

pub fn downloaded_setup_path(tag: &str) -> PathBuf {
    let safe_tag = tag
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    std::env::temp_dir().join(format!("OMNAFK-Setup-{safe_tag}.exe"))
}
