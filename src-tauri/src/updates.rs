use crate::config::UpdateChannel;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, time::Duration};

const GITHUB_API_ROOT: &str = "https://api.github.com/repos";
const GITHUB_WEB_ROOT: &str = "https://github.com";

#[derive(Debug, Clone, Serialize)]
pub struct UpdateCheck {
    pub repo: String,
    pub channel: UpdateChannel,
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

pub fn check(
    repo: &str,
    channel: UpdateChannel,
    current_version: &str,
) -> Result<UpdateCheck, String> {
    let repo = normalize_repo(repo)?;
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

    let releases = client
        .get(url)
        .send()
        .map_err(|error| format!("Couldn't reach GitHub Releases - check your internet connection to fix this: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Couldn't read GitHub Releases for {repo} - check the repository name to fix this: {error}"))?
        .json::<Vec<GithubRelease>>()
        .map_err(|error| format!("Couldn't parse GitHub Releases for {repo} - try again later: {error}"))?;

    let release = releases
        .into_iter()
        .find(|release| {
            !release.draft && (matches!(channel, UpdateChannel::Prerelease) || !release.prerelease)
        })
        .ok_or_else(|| {
            format!("Couldn't find a public release for {repo} on the selected update channel.")
        })?;

    let latest_version = release_version(&release.tag_name);
    let update_available = compare_versions(&latest_version, current_version) == Ordering::Greater;
    let asset = release
        .assets
        .into_iter()
        .find(|asset| asset.name.ends_with("-setup.exe") || asset.name.ends_with(".exe"));

    Ok(UpdateCheck {
        repo,
        channel,
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
        notes_excerpt: release.body.and_then(|body| notes_excerpt(&body)),
    })
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
    if !(url.starts_with("https://github.com/") || url.starts_with("https://api.github.com/")) {
        return Err(
            "Couldn't open link - OMNAFK only opens GitHub URLs from this control.".to_string(),
        );
    }
    open_url_impl(url)
}

fn valid_repo_part(part: &str) -> bool {
    !part.is_empty()
        && part
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn release_version(tag: &str) -> String {
    tag.trim_start_matches(['v', 'V']).to_string()
}

fn compare_versions(left: &str, right: &str) -> Ordering {
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
}
