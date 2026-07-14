//! Tail log files for in-game vs menu line patterns.

use super::rules::LogPresenceRules;
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogVote {
    InGame,
    Menu,
    #[default]
    None,
}

#[derive(Debug, Default)]
pub struct LogTailState {
    last_poll: Option<Instant>,
    last_path: Option<PathBuf>,
    last_offset: u64,
    last_vote: LogVote,
}

impl LogTailState {
    pub fn poll(
        &mut self,
        rules: &LogPresenceRules,
        poll_interval: Duration,
        now: Instant,
    ) -> LogVote {
        if self
            .last_poll
            .is_some_and(|at| now.duration_since(at) < poll_interval)
        {
            return self.last_vote;
        }
        self.last_poll = Some(now);

        let Some(path) = resolve_newest_log(&rules.paths) else {
            self.last_vote = LogVote::None;
            return LogVote::None;
        };

        if self.last_path.as_ref() != Some(&path) {
            self.last_path = Some(path.clone());
            self.last_offset = 0;
        }

        let vote = tail_file(&path, &mut self.last_offset, rules);
        self.last_vote = vote;
        vote
    }
}

fn expand_path(raw: &str) -> String {
    let mut out = raw.to_string();
    for (key, value) in std::env::vars() {
        let needle = format!("%{key}%");
        out = out.replace(&needle, &value);
    }
    out
}

fn resolve_newest_log(patterns: &[String]) -> Option<PathBuf> {
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for pattern in patterns {
        let expanded = expand_path(pattern);
        let path = Path::new(&expanded);
        if path.is_file() {
            if let Ok(meta) = fs::metadata(path) {
                if let Ok(modified) = meta.modified() {
                    match &best {
                        Some((best_time, _)) if modified <= *best_time => {}
                        _ => best = Some((modified, path.to_path_buf())),
                    }
                }
            }
            continue;
        }
        let parent = path.parent()?;
        let glob_part = path.file_name()?.to_str()?;
        if !parent.is_dir() {
            continue;
        }
        let entries = fs::read_dir(parent).ok()?;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !glob_match(glob_part, &name) {
                continue;
            }
            let file_path = entry.path();
            if !file_path.is_file() {
                continue;
            }
            if let Ok(meta) = fs::metadata(&file_path) {
                if let Ok(modified) = meta.modified() {
                    match &best {
                        Some((best_time, _)) if modified <= *best_time => {}
                        _ => best = Some((modified, file_path)),
                    }
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

fn glob_match(pattern: &str, name: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return name.starts_with(prefix);
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return name.ends_with(suffix);
    }
    name == pattern
}

fn tail_file(path: &Path, offset: &mut u64, rules: &LogPresenceRules) -> LogVote {
    let Ok(mut file) = fs::File::open(path) else {
        return LogVote::None;
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    if *offset > len {
        *offset = 0;
    }
    if file.seek(SeekFrom::Start(*offset)).is_err() {
        return LogVote::None;
    }
    let mut chunk = vec![0u8; 64 * 1024];
    let read = file.read(&mut chunk).unwrap_or(0);
    *offset = offset.saturating_add(read as u64);

    if read == 0 {
        return LogVote::None;
    }
    let text = String::from_utf8_lossy(&chunk[..read]);
    let mut last_menu = None;
    let mut last_in_game = None;
    for line in text.lines() {
        let line_lower = line.to_ascii_lowercase();
        for needle in &rules.menu {
            if line_lower.contains(&needle.to_ascii_lowercase()) {
                last_menu = Some(line.trim().to_string());
            }
        }
        for needle in &rules.in_game {
            if line_lower.contains(&needle.to_ascii_lowercase()) {
                last_in_game = Some(line.trim().to_string());
            }
        }
    }
    match (last_in_game, last_menu) {
        (Some(ig), Some(m)) => {
            if text.rfind(&ig).unwrap_or(0) > text.rfind(&m).unwrap_or(0) {
                LogVote::InGame
            } else {
                LogVote::Menu
            }
        }
        (Some(_), None) => LogVote::InGame,
        (None, Some(_)) => LogVote::Menu,
        (None, None) => LogVote::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_star_suffix() {
        assert!(glob_match("*.log", "game.log"));
        assert!(!glob_match("*.log", "game.txt"));
    }
}
