use crate::{flyout, ipc};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

pub const TOAST_EVENT: &str = "omnafk://toast";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToastKind {
    Info,
    Error,
    Success,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToastAction {
    OpenFlyout,
    RestartAdmin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToastPayload {
    pub text: String,
    pub kind: ToastKind,
    pub action: Option<ToastAction>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone)]
pub struct QueuedNotice {
    pub text: String,
    pub kind: ToastKind,
    pub action: Option<ToastAction>,
}

impl QueuedNotice {
    pub fn info(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: ToastKind::Info,
            action: None,
        }
    }

    pub fn success(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: ToastKind::Success,
            action: None,
        }
    }

    pub fn error(text: impl Into<String>, action: Option<ToastAction>) -> Self {
        Self {
            text: text.into(),
            kind: ToastKind::Error,
            action,
        }
    }

    pub fn to_payload(&self) -> ToastPayload {
        let duration_ms = match self.kind {
            ToastKind::Error => 0,
            ToastKind::Success => 4_000,
            ToastKind::Info => 5_000,
        };
        ToastPayload {
            text: self.text.clone(),
            kind: self.kind,
            action: self.action,
            duration_ms,
        }
    }
}

/// Surface a notice to the user as a native Windows notification, so every
/// engine event lands in the OS Action Center regardless of flyout state.
/// In-app toasts remain only as direct feedback for buttons clicked in the UI.
pub fn deliver(app: &AppHandle, notice: &QueuedNotice) {
    show_windows_notification(app, notice);
}

/// Run the button action from an in-flyout toast.
pub fn run_toast_action(app: &AppHandle, action: ToastAction) -> Result<(), String> {
    match action {
        ToastAction::OpenFlyout => flyout::open_default(app).map_err(|error| error.to_string()),
        ToastAction::RestartAdmin => {
            let engine = app
                .try_state::<crate::engine::SharedEngine>()
                .ok_or_else(|| "Engine not ready.".to_string())?;
            ipc::restart_elevated(app, engine.inner(), ipc::ElevationMode::Manual)
        }
    }
}

fn show_windows_notification(app: &AppHandle, notice: &QueuedNotice) {
    use tauri_plugin_notification::NotificationExt;
    let title = if matches!(notice.kind, ToastKind::Error) {
        "OMNAFK — Problem"
    } else {
        "OMNAFK"
    };
    if let Err(error) = app
        .notification()
        .builder()
        .title(title)
        .body(&notice.text)
        .show()
    {
        tracing::warn!(
            "Couldn't show notification - enable Windows notifications for OMNAFK to fix this: {error}"
        );
    }
}
