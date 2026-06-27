use crate::config::DEFAULT_GITHUB_REPO;
use reqwest::{blocking::Client, Url};
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, path::PathBuf, time::Duration};

const GITHUB_API_ROOT: &str = "https://api.github.com/repos";
const GITHUB_WEB_ROOT: &str = "https://github.com";

#[derive(Debug, Clone, Serialize)]
pub struct UpdateCheck {
    pub repo: String,
    pub current_version: String,
    pub latest_version: String,
    pub latest_tag: String,
    pub title: String,
    pub url: String,
    pub published_at: Option<String>,
    pub prerelease: bool,
    pub update_available: bool,
    pub asset_name: Option<String>,
    pub asset_url: Option<String>,
    pub notes_excerpt: Option<String>,
    pub release_notes: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    tag_name: String,
    name: Option<String>,
    html_url: String,
    body: Option<String>,
    draft: bool,
    prerelease: bool,
    published_at: Option<String>,
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

pub fn check(repo: &str, current_version: &str) -> Result<UpdateCheck, String> {
    let repo = normalize_repo(repo)?;
    let releases = fetch_releases(&repo, current_version)?;

    let release = releases
        .into_iter()
        .find(|release| !release.draft && !release.prerelease)
        .ok_or_else(|| format!("Couldn't find a public stable release for {repo}."))?;

    let latest_version = release_version(&release.tag_name);
    let update_available = compare_versions(&latest_version, current_version) == Ordering::Greater;
    let asset = release
        .assets
        .into_iter()
        .find(|asset| asset.name.ends_with("-setup.exe") || asset.name.ends_with(".exe"));

    Ok(UpdateCheck {
        repo,
        current_version: current_version.to_string(),
        latest_version,
        latest_tag: release.tag_name.clone(),
        title: release.name.unwrap_or(release.tag_name),
        url: release.html_url,
        published_at: release.published_at,
        prerelease: release.prerelease,
        update_available,
        asset_name: asset.as_ref().map(|asset| asset.name.clone()),
        asset_url: asset.map(|asset| asset.browser_download_url),
        notes_excerpt: release.body.as_ref().and_then(|body| notes_excerpt(body)),
        release_notes: release
            .body
            .as_ref()
            .map(|body| truncate_notes(body.clone(), 800)),
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct ReleaseNotes {
    pub tag: String,
    pub title: String,
    pub published_at: Option<String>,
    pub body: String,
}

/// Full release notes for the latest few stable releases.
pub fn changelog(repo: &str, current_version: &str) -> Result<Vec<ReleaseNotes>, String> {
    let repo = normalize_repo(repo)?;
    let releases = fetch_releases(&repo, current_version)?;
    Ok(releases
        .into_iter()
        .filter(|release| !release.draft && !release.prerelease)
        .take(5)
        .map(|release| ReleaseNotes {
            title: release
                .name
                .clone()
                .unwrap_or_else(|| release.tag_name.clone()),
            tag: release.tag_name,
            published_at: release.published_at,
            body: truncate_notes(release.body.unwrap_or_default(), 1200),
        })
        .collect())
}

pub fn release_notes_excerpt(repo: &str, tag: &str, current_version: &str) -> Option<String> {
    let repo = normalize_repo(repo).ok()?;
    let releases = fetch_releases(&repo, current_version).ok()?;
    let tag_norm = release_version(tag);
    releases
        .into_iter()
        .find(|release| release_version(&release.tag_name) == tag_norm)
        .and_then(|release| {
            release
                .body
                .as_ref()
                .map(|body| truncate_notes(body.clone(), 800))
        })
}

fn truncate_notes(body: String, max: usize) -> String {
    let trimmed = body.trim();
    if trimmed.chars().count() <= max {
        trimmed.to_string()
    } else {
        format!("{}...", trimmed.chars().take(max).collect::<String>())
    }
}

fn fetch_releases(repo: &str, current_version: &str) -> Result<Vec<GithubRelease>, String> {
    let url = format!("{GITHUB_API_ROOT}/{repo}/releases?per_page=20");
    let client = Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent(format!("OMNAFK/{current_version}"))
        .build()
        .map_err(|error| {
            format!(
                "Couldn't prepare the GitHub update check - restart OMNAFK to fix this: {error}"
            )
        })?;

    client
        .get(url)
        .send()
        .map_err(|error| format!("Couldn't reach GitHub Releases - check your internet connection to fix this: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Couldn't read GitHub Releases for {repo} - check the repository name to fix this: {error}"))?
        .json::<Vec<GithubRelease>>()
        .map_err(|error| format!("Couldn't parse GitHub Releases for {repo} - try again later: {error}"))
}

pub fn repo_url(repo: &str) -> Result<String, String> {
    normalize_repo(repo).map(|repo| format!("{GITHUB_WEB_ROOT}/{repo}"))
}

pub fn releases_url(repo: &str) -> Result<String, String> {
    normalize_repo(repo).map(|repo| format!("{GITHUB_WEB_ROOT}/{repo}/releases"))
}

pub fn issues_url(repo: &str) -> Result<String, String> {
    normalize_repo(repo)
        .map(|repo| format!("{GITHUB_WEB_ROOT}/{repo}/issues/new?template=bug_report.yml"))
}

/// Build a prefilled "Community profile suggestion" issue URL. `fields` maps
/// each template field id to the value to drop into the form.
pub fn community_profile_url(repo: &str, fields: &[(&str, &str)]) -> Result<String, String> {
    let repo = normalize_repo(repo)?;
    let mut url = format!("{GITHUB_WEB_ROOT}/{repo}/issues/new?template=community_profile.yml");
    for (key, value) in fields {
        if value.trim().is_empty() {
            continue;
        }
        url.push('&');
        url.push_str(&encode_query_component(key));
        url.push('=');
        url.push_str(&encode_query_component(value));
    }
    Ok(url)
}

/// Percent-encode a query component (RFC 3986 unreserved set kept as-is).
fn encode_query_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

pub fn normalize_repo(repo: &str) -> Result<String, String> {
    let mut value = repo.trim().trim_end_matches('/').trim_end_matches(".git");
    for prefix in [
        "https://github.com/",
        "http://github.com/",
        "git@github.com:",
        "github.com/",
    ] {
        if let Some(stripped) = value.strip_prefix(prefix) {
            value = stripped;
            break;
        }
    }
    let parts: Vec<_> = value.split('/').filter(|part| !part.is_empty()).collect();
    if parts.len() < 2 {
        return Err("Set GitHub repository to owner/repo before checking for updates.".to_string());
    }
    let owner = parts[0];
    let name = parts[1];
    if !valid_repo_part(owner) || !valid_repo_part(name) {
        return Err(
            "Set GitHub repository with only letters, numbers, '.', '_' or '-'.".to_string(),
        );
    }
    Ok(format!("{owner}/{name}"))
}

pub fn open_url(url: &str) -> Result<(), String> {
    if !(url.starts_with("https://github.com/")
        || url.starts_with("https://api.github.com/")
        || url.starts_with("https://objects.githubusercontent.com/")
        || url.starts_with("https://release-assets.githubusercontent.com/"))
    {
        return Err(
            "Couldn't open link - OMNAFK only opens trusted GitHub download URLs from this control."
                .to_string(),
        );
    }
    open_url_impl(url)
}

pub fn download_setup_installer(
    repo: &str,
    asset_url: &str,
    tag: &str,
    current_version: &str,
) -> Result<PathBuf, String> {
    use crate::installer::{
        downloaded_setup_path, parse_sha256_sidecar, setup_sha256_url, verify_sha256,
    };

    require_official_repo(repo)?;
    if !trusted_download_url(asset_url) {
        return Err(
            "Couldn't download the update installer - the release asset host is not trusted."
                .to_string(),
        );
    }

    let client = Client::builder()
        .timeout(Duration::from_secs(120))
        .user_agent(format!("OMNAFK/{current_version}"))
        .build()
        .map_err(|error| format!("Couldn't prepare the update download: {error}"))?;

    let response = client
        .get(asset_url)
        .send()
        .map_err(|error| format!("Couldn't download the update installer: {error}"))?;
    if !trusted_download_url(response.url().as_str()) {
        return Err(
            "Couldn't download the update installer - the download redirected to an untrusted host."
                .to_string(),
        );
    }
    let bytes = response
        .error_for_status()
        .map_err(|error| format!("Couldn't download the update installer: {error}"))?
        .bytes()
        .map_err(|error| format!("Couldn't read the update installer: {error}"))?;

    let sidecar_url = setup_sha256_url(asset_url);
    if !trusted_download_url(&sidecar_url) {
        return Err(
            "Couldn't verify the update installer - the checksum host is not trusted.".to_string(),
        );
    }
    let sidecar_response = client
        .get(&sidecar_url)
        .send()
        .map_err(|error| format!("Couldn't download the update checksum: {error}"))?;
    if !trusted_download_url(sidecar_response.url().as_str()) {
        return Err(
            "Couldn't verify the update installer - the checksum redirected to an untrusted host."
                .to_string(),
        );
    }
    let text = sidecar_response
        .error_for_status()
        .map_err(|error| format!("Couldn't download the update checksum: {error}"))?
        .text()
        .map_err(|error| format!("Couldn't read the update checksum: {error}"))?;
    let expected = parse_sha256_sidecar(&text).ok_or_else(|| {
        "Couldn't verify the update installer - the checksum file is missing or invalid."
            .to_string()
    })?;
    verify_sha256(&bytes, &expected)?;

    let path = downloaded_setup_path(tag);
    std::fs::write(&path, &bytes).map_err(|error| {
        format!(
            "Couldn't save the update installer to {}: {error}",
            path.display()
        )
    })?;
    Ok(path)
}

pub fn launch_setup_installer(path: &std::path::Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        std::process::Command::new(path)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ())
            .map_err(|error| format!("Couldn't launch the update installer: {error}"))
    }
    #[cfg(not(windows))]
    {
        Err("Launching the update installer is only supported on Windows.".to_string())
    }
}

fn valid_repo_part(part: &str) -> bool {
    !part.is_empty()
        && part
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

pub fn require_official_repo(repo: &str) -> Result<(), String> {
    let repo = normalize_repo(repo)?;
    if repo.eq_ignore_ascii_case(DEFAULT_GITHUB_REPO) {
        Ok(())
    } else {
        Err("App updates can only install builds from the official OMNAFK repository.".to_string())
    }
}

fn trusted_download_url(url: &str) -> bool {
    let Ok(url) = Url::parse(url) else {
        return false;
    };
    if url.scheme() != "https" {
        return false;
    }
    matches!(
        url.host_str(),
        Some(
            "github.com" | "objects.githubusercontent.com" | "release-assets.githubusercontent.com"
        )
    )
}

fn release_version(tag: &str) -> String {
    tag.trim_start_matches(['v', 'V']).to_string()
}

pub fn compare_versions(left: &str, right: &str) -> Ordering {
    let left = version_numbers(left);
    let right = version_numbers(right);
    for index in 0..left.len().max(right.len()) {
        let ordering = left
            .get(index)
            .unwrap_or(&0)
            .cmp(right.get(index).unwrap_or(&0));
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

fn version_numbers(value: &str) -> Vec<u64> {
    value
        .trim_start_matches(['v', 'V'])
        .split(['.', '-', '+'])
        .map(|part| {
            part.bytes()
                .take_while(u8::is_ascii_digit)
                .fold(0_u64, |acc, byte| {
                    acc.saturating_mul(10).saturating_add((byte - b'0') as u64)
                })
        })
        .collect()
}

fn notes_excerpt(body: &str) -> Option<String> {
    let normalized = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(3)
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.is_empty() {
        None
    } else if normalized.len() > 180 {
        Some(format!(
            "{}...",
            normalized.chars().take(177).collect::<String>()
        ))
    } else {
        Some(normalized)
    }
}

#[cfg(windows)]
fn open_url_impl(url: &str) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::PCWSTR,
        Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_SHOWNORMAL},
    };

    let wide_url: Vec<u16> = std::ffi::OsStr::new(url)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let result = unsafe {
        ShellExecuteW(
            None,
            None,
            PCWSTR(wide_url.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        )
    };
    if result.0 as usize <= 32 {
        Err(
            "Couldn't open GitHub in your browser - open the link manually from Settings."
                .to_string(),
        )
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn open_url_impl(_url: &str) -> Result<(), String> {
    Err("Opening GitHub links is only wired for Windows builds.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_common_github_repo_inputs() {
        assert_eq!(normalize_repo("owner/repo").unwrap(), "owner/repo");
        assert_eq!(
            normalize_repo("https://github.com/owner/repo.git").unwrap(),
            "owner/repo"
        );
        assert_eq!(
            normalize_repo("git@github.com:owner/repo/releases").unwrap(),
            "owner/repo"
        );
    }

    #[test]
    fn compares_basic_semver_tags() {
        assert_eq!(compare_versions("v0.2.0", "0.1.9"), Ordering::Greater);
        assert_eq!(compare_versions("0.1.0", "0.1.0"), Ordering::Equal);
        assert_eq!(compare_versions("0.1.0", "0.1.1"), Ordering::Less);
    }

    #[test]
    fn issue_url_opens_bug_report_form() {
        assert_eq!(
            issues_url("OMNHZN/OMNAFK").unwrap(),
            "https://github.com/OMNHZN/OMNAFK/issues/new?template=bug_report.yml"
        );
    }

    #[test]
    fn community_profile_url_prefills_and_encodes_fields() {
        let url = community_profile_url(
            "OMNHZN/OMNAFK",
            &[
                ("game", "Elden Ring"),
                ("exe", "eldenring.exe"),
                ("action", "W tap"),
                ("notes", ""),
            ],
        )
        .unwrap();
        assert!(url.starts_with(
            "https://github.com/OMNHZN/OMNAFK/issues/new?template=community_profile.yml"
        ));
        assert!(url.contains("&game=Elden%20Ring"));
        assert!(url.contains("&exe=eldenring.exe"));
        assert!(url.contains("&action=W%20tap"));
        // Empty values are skipped entirely.
        assert!(!url.contains("notes="));
    }

    #[test]
    fn update_installer_hosts_are_allowlisted() {
        assert!(trusted_download_url(
            "https://github.com/OMNHZN/OMNAFK/releases/download/v0.1.12/OMNAFK-Setup.exe"
        ));
        assert!(trusted_download_url(
            "https://release-assets.githubusercontent.com/github-production-release-asset/file"
        ));
        assert!(!trusted_download_url(
            "http://github.com/OMNHZN/OMNAFK/file.exe"
        ));
        assert!(!trusted_download_url(
            "https://example.com/OMNAFK-Setup.exe"
        ));
    }

    #[test]
    fn installer_updates_require_official_repo() {
        assert!(require_official_repo("OMNHZN/OMNAFK").is_ok());
        assert!(require_official_repo("someone/else").is_err());
    }
}
