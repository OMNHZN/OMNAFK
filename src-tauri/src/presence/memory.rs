//! Read manifest-defined offsets or signatures from the target process.

use super::rules::{MemoryPresenceRules, MemoryReadRule};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows::Win32::System::ProcessStatus::{
    K32EnumProcessModulesEx, K32GetModuleBaseNameW, LIST_MODULES_ALL,
};
use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_READ};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MemoryVote {
    InGame,
    Menu,
    #[default]
    None,
}

#[derive(Debug, Default)]
pub struct MemoryReadState {
    last_poll: Option<Instant>,
    last_vote: MemoryVote,
}

impl MemoryReadState {
    pub fn poll(
        &mut self,
        pid: u32,
        rules: &MemoryPresenceRules,
        interval: Duration,
        now: Instant,
    ) -> MemoryVote {
        if self
            .last_poll
            .is_some_and(|at| now.duration_since(at) < interval)
        {
            return self.last_vote;
        }
        self.last_poll = Some(now);

        let mut best: Option<(MemoryVote, u8)> = None;
        for rule in &rules.reads {
            if let Some((vote, confidence)) = read_rule(pid, rule) {
                if best.as_ref().is_none_or(|(_, c)| confidence > *c) {
                    best = Some((vote, confidence));
                }
            }
        }
        self.last_vote = best.map(|(v, _)| v).unwrap_or(MemoryVote::None);
        self.last_vote
    }
}

fn read_rule(pid: u32, rule: &MemoryReadRule) -> Option<(MemoryVote, u8)> {
    let handle =
        unsafe { OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid).ok()? };
    let result = read_rule_inner(handle, pid, rule);
    unsafe {
        let _ = CloseHandle(handle);
    }
    result
}

fn read_rule_inner(handle: HANDLE, pid: u32, rule: &MemoryReadRule) -> Option<(MemoryVote, u8)> {
    let base = module_base(handle, pid, &rule.module)?;
    let address = if let Some(sig) = rule.signature.as_ref().filter(|s| !s.is_empty()) {
        let bytes = parse_signature(sig)?;
        let hit = scan_module(handle, base, &bytes)?;
        base.saturating_add(hit)
            .saturating_add(rule.offset_from_match)
    } else {
        base.saturating_add(rule.offset)
    };

    let mut buf = vec![0u8; rule.size.clamp(1, 8)];
    let mut read = 0usize;
    let ok = unsafe {
        ReadProcessMemory(
            handle,
            address as *const _,
            buf.as_mut_ptr() as *mut _,
            buf.len(),
            Some(&mut read),
        )
        .is_ok()
    };
    if !ok || read == 0 {
        return None;
    }
    let value = u32::from_le_bytes([
        buf[0],
        buf.get(1).copied().unwrap_or(0),
        buf.get(2).copied().unwrap_or(0),
        buf.get(3).copied().unwrap_or(0),
    ]);

    if rule.in_game_values.contains(&value) {
        Some((MemoryVote::InGame, 92))
    } else if rule.menu_values.contains(&value) {
        Some((MemoryVote::Menu, 92))
    } else {
        None
    }
}

fn module_base(handle: HANDLE, _pid: u32, module_name: &str) -> Option<u64> {
    let mut modules = [windows::Win32::Foundation::HMODULE::default(); 256];
    let mut needed = 0u32;
    let ok = unsafe {
        K32EnumProcessModulesEx(
            handle,
            modules.as_mut_ptr(),
            (modules.len() * std::mem::size_of::<windows::Win32::Foundation::HMODULE>()) as u32,
            &mut needed,
            LIST_MODULES_ALL.0,
        )
    }
    .as_bool();
    if !ok {
        return None;
    }
    let count = (needed as usize) / std::mem::size_of::<windows::Win32::Foundation::HMODULE>();
    let want = module_name.trim().to_ascii_lowercase();
    for &module in &modules[..count.min(modules.len())] {
        let mut name = [0u16; 260];
        let len = unsafe { K32GetModuleBaseNameW(handle, Some(module), &mut name) };
        if len == 0 {
            continue;
        }
        let base_name = String::from_utf16_lossy(&name[..len as usize]).to_ascii_lowercase();
        if want.is_empty() || base_name == want {
            return Some(module.0 as u64);
        }
    }
    None
}

fn parse_signature(sig: &str) -> Option<Vec<Option<u8>>> {
    let mut out = Vec::new();
    for token in sig.split_whitespace() {
        if token == "??" || token == "?" {
            out.push(None);
        } else {
            let byte = u8::from_str_radix(token, 16).ok()?;
            out.push(Some(byte));
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn scan_module(handle: HANDLE, base: u64, pattern: &[Option<u8>]) -> Option<u64> {
    const CHUNK: usize = 64 * 1024;
    let mut offset = 0u64;
    let max_scan = 8 * 1024 * 1024u64;
    while offset < max_scan {
        let mut buf = vec![0u8; CHUNK];
        let mut read = 0usize;
        let ok = unsafe {
            ReadProcessMemory(
                handle,
                (base + offset) as *const _,
                buf.as_mut_ptr() as *mut _,
                buf.len(),
                Some(&mut read),
            )
            .is_ok()
        };
        if !ok || read < pattern.len() {
            break;
        }
        for i in 0..=read.saturating_sub(pattern.len()) {
            if pattern_match(&buf[i..], pattern) {
                return Some(offset + i as u64);
            }
        }
        offset = offset.saturating_add(read as u64);
    }
    None
}

fn pattern_match(data: &[u8], pattern: &[Option<u8>]) -> bool {
    if data.len() < pattern.len() {
        return false;
    }
    for (i, slot) in pattern.iter().enumerate() {
        if let Some(byte) = slot {
            if data[i] != *byte {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_signature_wildcards() {
        let p = parse_signature("48 8B ?? 05").unwrap();
        assert_eq!(p.len(), 4);
        assert_eq!(p[0], Some(0x48));
        assert_eq!(p[2], None);
    }
}
