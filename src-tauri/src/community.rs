//! Community game intelligence: fetch shared manifests, apply hints, and upload opt-in stats.

use crate::{
    config::{AppConfig, TargetAction, TargetProfile},
    updates,
};
use parking_lot::RwLock;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::PathBuf,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

const MANIFEST_PATH: &str = "community/manifest.json";
const REFRESH_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const MIN_AUTO_APPLY_CONFIDENCE: f32 = 0.7;
const MIN_AUTO_APPLY_REPORTS: u32 = 30;
const MIN_CONTRIBUTE_ATTEMPTS: u32 = 20;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CommunityManifest {
    pub version: u32,
    pub updated: String,
    pub ingest_url: Option<String>,
    pub games: BTreeMap<String, GameEntry>,
    pub detection: DetectionFeed,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GameEntry {
    pub action: Option<String>,
    pub interval: Option<u64>,
    pub send_without_focus: Option<bool>,
    pub auto_fallback: Option<bool>,
    pub adaptive: Option<bool>,
    pub confidence: f32,
    pub reports: u32,
    pub top_keys: Vec<String>,
    pub fallback_order: Vec<String>,
    pub monitor_style: Option<String>,
    pub monitor_note: Option<String>,
    pub status: Option<String>,
    pub status_note: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DetectionFeed {
    pub known_exes: Vec<String>,
    pub path_patterns: Vec<String>,
    pub negative_exes: Vec<String>,
    pub negative_classes: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct DetectionSupplement {
    pub known_exes: BTreeSet<String>,
    pub path_patterns: Vec<String>,
    pub negative_exes: BTreeSet<String>,
    pub negative_classes: Vec<String>,
}

impl DetectionSupplement {
    pub fn from_manifest(manifest: &CommunityManifest) -> Self {
        Self {
            known_exes: manifest
                .detection
                .known_exes
                .iter()
                .map(|s| s.to_ascii_lowercase())
                .collect(),
            path_patterns: manifest
                .detection
                .path_patterns
                .iter()
                .map(|s| s.to_ascii_lowercase())
                .collect(),
            negative_exes: manifest
                .detection
                .negative_exes
                .iter()
                .map(|s| s.to_ascii_lowercase())
                .collect(),
            negative_classes: manifest
                .detection
                .negative_classes
                .iter()
                .map(|s| s.to_ascii_lowercase())
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommunityGameSnapshot {
    pub label: Option<String>,
    pub confidence: Option<f32>,
    pub reports: Option<u32>,
    pub degraded: Option<String>,
    pub applied: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CommunityRuntime {
    pub manifest: Option<CommunityManifest>,
    pub supplement: DetectionSupplement,
    pub last_fetch: Option<Instant>,
    pub last_error: Option<String>,
    pub applied_exes: BTreeSet<String>,
}

pub type SharedCommunity = Arc<RwLock<CommunityRuntime>>;

pub fn shared_runtime() -> SharedCommunity {
    Arc::new(RwLock::new(CommunityRuntime {
        manifest: load_cached_manifest(),
        supplement: load_cached_manifest()
            .as_ref()
            .map(DetectionSupplement::from_manifest)
            .unwrap_or_default(),
        last_fetch: None,
        last_error: None,
        applied_exes: BTreeSet::new(),
    }))
}

pub fn spawn_sync_loop(community: SharedCommunity, engine: crate::engine::SharedEngine) {
    thread::spawn(move || loop {
        let (enabled, repo, client_id, version) = {
            let snap = engine.snapshot();
            (
                snap.config.community_intelligence,
                snap.config.github_repo.clone(),
                snap.config.community_client_id.clone(),
                env!("CARGO_PKG_VERSION").to_string(),
            )
        };
        if enabled {
            if !client_id.is_empty() {
                if let Err(error) = refresh_manifest(&community, &repo) {
                    community.write().last_error = Some(error);
                }
                let _ = flush_contributions(&client_id, &version, &community);
            } else {
                let _ = refresh_manifest(&community, &repo);
            }
        }
        thread::sleep(REFRESH_INTERVAL);
    });
}

pub fn refresh_on_launch(community: &SharedCommunity, repo: &str) {
    if let Err(error) = refresh_manifest(community, repo) {
        community.write().last_error = Some(error);
    }
}

fn refresh_manifest(community: &SharedCommunity, repo: &str) -> Result<(), String> {
    let repo = updates::normalize_repo(repo)?;
    let url = format!("https://raw.githubusercontent.com/{repo}/main/{MANIFEST_PATH}");
    let body = fetch_text(&url)?;
    let manifest: CommunityManifest = serde_json::from_str(&body)
        .map_err(|error| format!("Couldn't parse community manifest: {error}"))?;
    save_cached_manifest(&manifest)
        .map_err(|error| format!("Couldn't cache community manifest: {error}"))?;
    let supplement = DetectionSupplement::from_manifest(&manifest);
    let mut rt = community.write();
    rt.manifest = Some(manifest);
    rt.supplement = supplement;
    rt.last_fetch = Some(Instant::now());
    rt.last_error = None;
    Ok(())
}

fn fetch_text(url: &str) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("OMNAFK-community")
        .build()
        .map_err(|error| format!("Couldn't prepare community fetch: {error}"))?;
    client
        .get(url)
        .send()
        .map_err(|error| format!("Couldn't reach community manifest: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Couldn't download community manifest: {error}"))?
        .text()
        .map_err(|error| format!("Couldn't read community manifest: {error}"))
}

pub fn cache_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("OMNAFK").join("community_cache.json"))
}

pub fn queue_path() -> Option<PathBuf> {
    dirs::config_dir().map(|dir| dir.join("OMNAFK").join("community_queue.json"))
}

fn load_cached_manifest() -> Option<CommunityManifest> {
    let path = cache_path()?;
    let bytes = fs::read(&path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn save_cached_manifest(manifest: &CommunityManifest) -> io::Result<()> {
    let path = cache_path().ok_or_else(|| io::Error::other("no config dir"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_vec_pretty(manifest).map_err(io::Error::other)?;
    fs::write(path, json)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct ContributionQueue {
    pub reports: Vec<ContributionReport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContributionReport {
    pub exe: String,
    pub action: String,
    pub attempts: u32,
    pub successes: u32,
    pub send_without_focus: bool,
    pub top_keys: Vec<String>,
    pub monitor_ok: Option<u32>,
    pub monitor_fail: Option<u32>,
    pub stability_event: Option<String>,
}

pub fn record_keepalive(
    exe: &str,
    action: &str,
    success: bool,
    send_without_focus: bool,
    top_keys: &[String],
) {
    let exe = exe.to_ascii_lowercase();
    let mut queue = load_queue();
    if let Some(row) = queue
        .reports
        .iter_mut()
        .find(|r| r.exe == exe && r.action == action)
    {
        row.attempts = row.attempts.saturating_add(1);
        if success {
            row.successes = row.successes.saturating_add(1);
        }
        if !top_keys.is_empty() {
            row.top_keys = top_keys.iter().take(2).cloned().collect();
        }
        row.send_without_focus = send_without_focus;
    } else {
        queue.reports.push(ContributionReport {
            exe,
            action: action.to_string(),
            attempts: 1,
            successes: u32::from(success),
            send_without_focus,
            top_keys: top_keys.iter().take(2).cloned().collect(),
            monitor_ok: None,
            monitor_fail: None,
            stability_event: None,
        });
    }
    let _ = save_queue(&queue);
}

pub fn record_monitor_result(exe: &str, ok: bool) {
    let exe = exe.to_ascii_lowercase();
    let mut queue = load_queue();
    if let Some(row) = queue.reports.iter_mut().find(|r| r.exe == exe) {
        if ok {
            row.monitor_ok = Some(row.monitor_ok.unwrap_or(0).saturating_add(1));
        } else {
            row.monitor_fail = Some(row.monitor_fail.unwrap_or(0).saturating_add(1));
        }
    } else {
        queue.reports.push(ContributionReport {
            exe,
            action: String::new(),
            attempts: 0,
            successes: 0,
            send_without_focus: true,
            top_keys: Vec::new(),
            monitor_ok: Some(u32::from(ok)),
            monitor_fail: Some(u32::from(!ok)),
            stability_event: None,
        });
    }
    let _ = save_queue(&queue);
}

pub fn record_stability(event: &str) {
    let mut queue = load_queue();
    queue.reports.push(ContributionReport {
        exe: String::new(),
        action: String::new(),
        attempts: 0,
        successes: 0,
        send_without_focus: false,
        top_keys: Vec::new(),
        monitor_ok: None,
        monitor_fail: None,
        stability_event: Some(event.to_string()),
    });
    let _ = save_queue(&queue);
}

fn load_queue() -> ContributionQueue {
    let Some(path) = queue_path() else {
        return ContributionQueue::default();
    };
    fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save_queue(queue: &ContributionQueue) -> io::Result<()> {
    let path = queue_path().ok_or_else(|| io::Error::other("no config dir"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_vec_pretty(queue).map_err(io::Error::other)?;
    fs::write(path, json)
}

#[derive(Debug, Serialize)]
struct UploadPayload {
    schema: u32,
    client_id: String,
    app_version: String,
    reports: Vec<ContributionReport>,
}

fn flush_contributions(
    client_id: &str,
    app_version: &str,
    community: &SharedCommunity,
) -> Result<(), String> {
    let ingest_url = community
        .read()
        .manifest
        .as_ref()
        .and_then(|m| m.ingest_url.clone())
        .filter(|url| !url.is_empty());

    let mut queue = load_queue();
    queue.reports.retain(|r| {
        r.attempts >= MIN_CONTRIBUTE_ATTEMPTS
            || r.stability_event.is_some()
            || r.monitor_ok.is_some()
            || r.monitor_fail.is_some()
    });
    if queue.reports.is_empty() {
        return Ok(());
    }

    let Some(url) = ingest_url else {
        return Ok(());
    };

    let payload = UploadPayload {
        schema: 1,
        client_id: client_id.to_string(),
        app_version: app_version.to_string(),
        reports: queue.reports.clone(),
    };

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(format!("OMNAFK/{app_version}"))
        .build()
        .map_err(|error| format!("Couldn't prepare community upload: {error}"))?;

    client
        .post(&url)
        .json(&payload)
        .send()
        .map_err(|error| format!("Couldn't upload community stats: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Community upload rejected: {error}"))?;

    queue.reports.clear();
    let _ = save_queue(&queue);
    Ok(())
}

pub fn ensure_client_id(config: &mut AppConfig) {
    if config.community_client_id.is_empty() {
        config.community_client_id = generate_client_id();
    }
}

fn generate_client_id() -> String {
    let mut rng = rand::thread_rng();
    (0..16)
        .map(|_| format!("{:02x}", rng.gen_range(0..=255)))
        .collect()
}

pub fn game_entry<'a>(runtime: &'a CommunityRuntime, exe: &str) -> Option<&'a GameEntry> {
    let exe = exe.to_ascii_lowercase();
    runtime.manifest.as_ref()?.games.get(&exe)
}

pub fn snapshot_for_exe(
    runtime: &CommunityRuntime,
    exe: &str,
    applied: bool,
) -> Option<CommunityGameSnapshot> {
    let entry = game_entry(runtime, exe)?;
    let label = entry
        .action
        .as_ref()
        .map(|action| format!("Community · {action} · {:.0}%", entry.confidence * 100.0));
    Some(CommunityGameSnapshot {
        label,
        confidence: Some(entry.confidence),
        reports: Some(entry.reports),
        degraded: entry
            .status
            .as_deref()
            .filter(|s| *s == "degraded")
            .and_then(|_| entry.status_note.clone()),
        applied,
    })
}

pub fn should_auto_apply(entry: &GameEntry, exe: &str, config: &AppConfig) -> bool {
    if !config.community_intelligence {
        return false;
    }
    if config.community_dismissed_exes.iter().any(|d| d == exe) {
        return false;
    }
    if entry.confidence < MIN_AUTO_APPLY_CONFIDENCE || entry.reports < MIN_AUTO_APPLY_REPORTS {
        return false;
    }
    true
}

pub fn apply_game_profile(config: &mut AppConfig, exe: &str, wclass: &str, entry: &GameEntry) {
    if config.profile_for(exe, wclass).is_some() {
        return;
    }
    let mut profile = TargetProfile::default();
    if let Some(action) = &entry.action {
        if let Ok(parsed) = parse_target_action(action) {
            profile.action = Some(parsed);
        }
    }
    profile.interval = entry.interval;
    if let Some(adaptive) = entry.adaptive {
        profile.adaptive = Some(adaptive);
    }
    config.set_profile(exe, wclass, profile);
}

pub fn apply_global_hints(config: &mut AppConfig, entry: &GameEntry) {
    if let Some(fallback) = entry.auto_fallback {
        if fallback {
            config.auto_fallback = true;
        }
    }
    if let Some(adaptive) = entry.adaptive {
        if adaptive {
            config.adaptive_actions = true;
        }
    }
}

fn parse_target_action(value: &str) -> Result<TargetAction, ()> {
    Ok(match value {
        "Space tap" | "SpaceTap" => TargetAction::SpaceTap,
        "W tap" | "WTap" => TargetAction::WTap,
        "Camera nudge" | "CameraNudge" => TargetAction::CameraNudge,
        "Mouse wiggle" | "MouseWiggle" => TargetAction::MouseWiggle,
        "Scroll tick" | "ScrollTick" => TargetAction::ScrollTick,
        "Right click" | "RightClick" => TargetAction::RightClick,
        _ => return Err(()),
    })
}

pub fn preferred_fallback_tier(
    entry: &GameEntry,
    failures: u32,
) -> Option<crate::health::FallbackTier> {
    use crate::health::FallbackTier;
    if entry.fallback_order.is_empty() || failures < crate::health::FAILURE_THRESHOLD {
        return None;
    }
    let index = ((failures / crate::health::FAILURE_THRESHOLD) as usize).saturating_sub(1);
    let label = entry.fallback_order.get(index)?;
    Some(match label.as_str() {
        "FocusFlick" | "focus_flick" => FallbackTier::FocusFlick,
        "CameraNudge" | "camera_nudge" => FallbackTier::CameraNudge,
        _ => FallbackTier::Normal,
    })
}

pub fn try_auto_apply_for_window(
    config: &mut AppConfig,
    runtime: &mut CommunityRuntime,
    exe: &str,
    wclass: &str,
) -> bool {
    if !config.community_intelligence {
        return false;
    }
    let exe_key = exe.to_ascii_lowercase();
    if runtime.applied_exes.contains(&exe_key) {
        return false;
    }
    let Some(entry) = game_entry(runtime, exe).cloned() else {
        return false;
    };
    if !should_auto_apply(&entry, &exe_key, config) {
        return false;
    }
    if config.profile_for(exe, wclass).is_some() {
        return false;
    }
    apply_game_profile(config, exe, wclass, &entry);
    apply_global_hints(config, &entry);
    runtime.applied_exes.insert(exe_key);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_deserializes_minimal() {
        let json = r#"{
            "version": 1,
            "updated": "2026-06-12",
            "games": {
                "robloxplayerbeta.exe": {
                    "action": "Space tap",
                    "interval": 540,
                    "confidence": 0.95,
                    "reports": 100
                }
            }
        }"#;
        let manifest: CommunityManifest = serde_json::from_str(json).unwrap();
        assert_eq!(manifest.version, 1);
        assert!(manifest.games.contains_key("robloxplayerbeta.exe"));
    }

    #[test]
    fn auto_apply_respects_thresholds() {
        let entry = GameEntry {
            confidence: 0.95,
            reports: 100,
            ..Default::default()
        };
        let config = AppConfig {
            community_intelligence: true,
            ..Default::default()
        };
        assert!(should_auto_apply(&entry, "game.exe", &config));
        let low = GameEntry {
            confidence: 0.5,
            reports: 5,
            ..Default::default()
        };
        assert!(!should_auto_apply(&low, "game.exe", &config));
    }
}
