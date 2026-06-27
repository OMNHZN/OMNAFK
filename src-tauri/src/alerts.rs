//! Best-effort remote alerts for away-from-keyboard events.
//!
//! When the keepalive stops while you're away (battery, session lock, safety
//! cap) or hits an error, OMNAFK can push a notification to your phone via
//! [ntfy](https://ntfy.sh) and/or a Discord webhook. This is purely a
//! notification side-channel: sends run on a detached thread with a short
//! timeout and only log on failure, so they can never block or break the
//! keepalive itself.

use crate::config::AppConfig;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);
const TEST_BODY: &str = "Test alert — OMNAFK notifications are working.";

/// Fire an alert for a notable engine event, if a channel is configured.
/// Non-blocking: spawns a thread and returns immediately.
pub fn send(config: &AppConfig, title: &str, message: &str) {
    if !config.remote_alerts {
        return;
    }
    let ntfy = ntfy_url(&config.ntfy_topic);
    let discord = discord_url(&config.discord_webhook);
    if ntfy.is_none() && discord.is_none() {
        return;
    }
    let title = title.to_string();
    let message = message.to_string();
    std::thread::spawn(move || {
        let client = match build_client() {
            Ok(client) => client,
            Err(error) => {
                crate::startup_log::warn(format!("alert client build failed: {error}"));
                return;
            }
        };
        if let Some(url) = ntfy {
            if let Err(error) = post_ntfy(&client, &url, &title, &message) {
                crate::startup_log::warn(format!("ntfy alert failed: {error}"));
            }
        }
        if let Some(url) = discord {
            if let Err(error) = post_discord(&client, &url, &title, &message) {
                crate::startup_log::warn(format!("discord alert failed: {error}"));
            }
        }
    });
}

/// Synchronously send a test alert to every configured channel, returning a
/// human summary or the first error. Used by the "Send test" button.
pub fn send_test(config: &AppConfig) -> Result<String, String> {
    let ntfy = ntfy_url(&config.ntfy_topic);
    let discord = discord_url(&config.discord_webhook);
    if ntfy.is_none() && discord.is_none() {
        return Err("Add an ntfy topic or a Discord webhook URL first.".to_string());
    }
    let client =
        build_client().map_err(|error| format!("Couldn't prepare the request: {error}"))?;
    let mut sent = Vec::new();
    if let Some(url) = ntfy {
        post_ntfy(&client, &url, "OMNAFK", TEST_BODY)
            .map_err(|error| format!("ntfy test failed: {error}"))?;
        sent.push("ntfy");
    }
    if let Some(url) = discord {
        post_discord(&client, &url, "OMNAFK", TEST_BODY)
            .map_err(|error| format!("Discord test failed: {error}"))?;
        sent.push("Discord");
    }
    Ok(format!("Test alert sent to {}.", sent.join(" and ")))
}

fn build_client() -> reqwest::Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(TIMEOUT)
        .user_agent(concat!("OMNAFK/", env!("CARGO_PKG_VERSION")))
        .build()
}

fn post_ntfy(
    client: &reqwest::blocking::Client,
    url: &str,
    title: &str,
    message: &str,
) -> reqwest::Result<()> {
    client
        .post(url)
        .header("Title", title)
        .body(message.to_string())
        .send()?
        .error_for_status()?;
    Ok(())
}

fn post_discord(
    client: &reqwest::blocking::Client,
    url: &str,
    title: &str,
    message: &str,
) -> reqwest::Result<()> {
    let payload = serde_json::json!({ "content": format!("**{title}**\n{message}") });
    client.post(url).json(&payload).send()?.error_for_status()?;
    Ok(())
}

/// Resolve the ntfy target. A full `https://…` value is used as-is (so a
/// self-hosted ntfy server works); a bare topic posts to the public
/// `https://ntfy.sh`. `http://` is rejected so alerts always go over TLS.
fn ntfy_url(topic: &str) -> Option<String> {
    let topic = topic.trim();
    if topic.is_empty() {
        return None;
    }
    if topic.starts_with("https://") {
        let parsed = reqwest::Url::parse(topic).ok()?;
        return parsed.host_str().is_some().then(|| topic.to_string());
    }
    if topic.starts_with("http://") {
        return None;
    }
    Some(format!("https://ntfy.sh/{topic}"))
}

/// Validate a Discord webhook URL: must be an `https` Discord host.
fn discord_url(url: &str) -> Option<String> {
    let url = url.trim();
    if url.is_empty() {
        return None;
    }
    let parsed = reqwest::Url::parse(url).ok()?;
    if parsed.scheme() != "https" {
        return None;
    }
    matches!(
        parsed.host_str(),
        Some("discord.com" | "discordapp.com" | "ptb.discord.com" | "canary.discord.com")
    )
    .then(|| url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_topic_targets_public_ntfy() {
        assert_eq!(
            ntfy_url("omnafk-abc123"),
            Some("https://ntfy.sh/omnafk-abc123".to_string())
        );
    }

    #[test]
    fn full_https_topic_used_as_is() {
        assert_eq!(
            ntfy_url("https://ntfy.example.com/mytopic"),
            Some("https://ntfy.example.com/mytopic".to_string())
        );
    }

    #[test]
    fn blank_and_insecure_topics_are_none() {
        assert_eq!(ntfy_url("  "), None);
        assert_eq!(ntfy_url("http://ntfy.sh/topic"), None);
    }

    #[test]
    fn discord_requires_https_discord_host() {
        assert!(discord_url("https://discord.com/api/webhooks/123/abc").is_some());
        assert!(discord_url("https://discordapp.com/api/webhooks/123/abc").is_some());
        assert!(discord_url("https://evil.example.com/api/webhooks/123/abc").is_none());
        assert!(discord_url("http://discord.com/api/webhooks/123/abc").is_none());
        assert!(discord_url("").is_none());
    }
}
