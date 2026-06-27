use chrono::Local;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::Mutex,
};

static LOG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

pub fn init() {
    let path = log_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = OpenOptions::new().create(true).append(true).open(&path);
    *LOG_PATH.lock().unwrap() = Some(path);
}

fn log_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("OMNAFK")
        .join("startup.log")
}

pub fn log(level: &str, message: impl AsRef<str>) {
    let line = format!(
        "{} [{level}] {}\n",
        Local::now().format("%Y-%m-%d %H:%M:%S"),
        message.as_ref()
    );
    let path = LOG_PATH.lock().unwrap().clone().unwrap_or_else(log_path);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(line.as_bytes());
    }
}

pub fn info(message: impl AsRef<str>) {
    log("INFO", message);
}

pub fn warn(message: impl AsRef<str>) {
    let message = message.as_ref();
    log("WARN", message);
    tracing::warn!("{message}");
}

pub fn error(message: impl AsRef<str>) {
    let message = message.as_ref();
    log("ERROR", message);
    tracing::error!("{message}");
}

pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let message = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic payload".to_string()
        };
        let location = info
            .location()
            .map(|loc| format!("{}:{}", loc.file(), loc.line()))
            .unwrap_or_else(|| "unknown location".to_string());
        error(format!("panic at {location}: {message}"));
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_path_ends_with_startup_log() {
        let path = log_path();
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("startup.log")
        );
    }
}
