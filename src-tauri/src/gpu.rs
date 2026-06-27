//! GPU activity probe via PDH `GPU Engine` counters (Windows 10+).
//!
//! Enumerates GPU Engine instances, parses the owning PID from each instance
//! name, and reports utilization. Falls back gracefully when PDH is unavailable.

use crate::detector::GpuUsageProbe;
use parking_lot::Mutex;
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    os::windows::ffi::OsStrExt,
    time::{Duration, Instant},
};
use windows::core::PCWSTR;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Performance::{
    PdhAddCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterValue, PdhOpenQueryW,
    PDH_FMT_COUNTERVALUE, PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PERF_DETAIL,
};

const REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const ACTIVE_THRESHOLD: f32 = 1.0;

pub struct PdhGpuProbe {
    inner: Mutex<ProbeInner>,
}

struct ProbeInner {
    query: Option<isize>,
    counters: BTreeMap<u32, isize>,
    warming: BTreeSet<u32>,
    last_refresh: Instant,
}

impl Default for PdhGpuProbe {
    fn default() -> Self {
        Self {
            inner: Mutex::new(ProbeInner {
                query: None,
                counters: BTreeMap::new(),
                warming: BTreeSet::new(),
                last_refresh: crate::time_util::instant_ttl_ago(REFRESH_INTERVAL),
            }),
        }
    }
}

impl GpuUsageProbe for PdhGpuProbe {
    fn usage_for_pid(&self, pid: u32) -> Option<f32> {
        let mut inner = self.inner.lock();
        inner.refresh_if_due();
        inner.read_usage(pid)
    }
}

impl ProbeInner {
    fn refresh_if_due(&mut self) {
        if self.last_refresh.elapsed() < REFRESH_INTERVAL {
            return;
        }
        self.warming.clear();
        self.last_refresh = Instant::now();
        let _ = self.refresh();
    }

    fn refresh(&mut self) -> Result<(), ()> {
        let instances = enumerate_gpu_engine_instances()?;
        let query = self.query.get_or_insert_with(|| open_query().unwrap_or(0));
        if *query == 0 {
            return Err(());
        }

        // Drop counters for PIDs that disappeared.
        self.counters.retain(|pid, _| {
            instances
                .iter()
                .any(|(instance_pid, _)| instance_pid == pid)
        });
        self.warming.retain(|pid| self.counters.contains_key(pid));

        let mut added = Vec::new();
        for (pid, instance) in instances {
            if self.counters.contains_key(&pid) {
                continue;
            }
            let path = format!(r"\GPU Engine({instance})\Utilization Percentage");
            if let Ok(counter) = add_counter(*query, &path) {
                self.counters.insert(pid, counter);
                added.push(pid);
            }
        }

        unsafe {
            let _ = PdhCollectQueryData(PDH_HQUERY(*query as *mut _));
        }
        if !added.is_empty() {
            unsafe {
                let _ = PdhCollectQueryData(PDH_HQUERY(*query as *mut _));
            }
            self.warming.extend(added);
        }
        Ok(())
    }

    fn read_usage(&self, pid: u32) -> Option<f32> {
        if self.warming.contains(&pid) {
            return None;
        }
        let query = self.query?;
        if query == 0 {
            return None;
        }
        let counter = self.counters.get(&pid)?;
        let mut value = PDH_FMT_COUNTERVALUE::default();
        let status = unsafe {
            PdhGetFormattedCounterValue(
                PDH_HCOUNTER(*counter as *mut _),
                PDH_FMT_DOUBLE,
                None,
                &mut value,
            )
        };
        if status != ERROR_SUCCESS.0 {
            return None;
        }
        let usage = unsafe { value.Anonymous.doubleValue } as f32;
        (usage >= ACTIVE_THRESHOLD).then_some(usage)
    }
}

fn open_query() -> Result<isize, ()> {
    let mut query = PDH_HQUERY::default();
    let status = unsafe { PdhOpenQueryW(None, 0, &mut query) };
    if status != ERROR_SUCCESS.0 {
        return Err(());
    }
    Ok(query.0 as isize)
}

fn add_counter(query: isize, path: &str) -> Result<isize, ()> {
    let wide = wide(path);
    let mut counter = PDH_HCOUNTER::default();
    let status = unsafe {
        PdhAddCounterW(
            PDH_HQUERY(query as *mut _),
            PCWSTR(wide.as_ptr()),
            0,
            &mut counter,
        )
    };
    if status != ERROR_SUCCESS.0 {
        return Err(());
    }
    Ok(counter.0 as isize)
}

fn enumerate_gpu_engine_instances() -> Result<Vec<(u32, String)>, ()> {
    let mut out = Vec::new();
    let items = pdh_enum_object_items("GPU Engine")?;
    for instance in items {
        if let Some(pid) = parse_pid_from_gpu_instance(&instance) {
            out.push((pid, instance));
        }
    }
    Ok(out)
}

fn parse_pid_from_gpu_instance(instance: &str) -> Option<u32> {
    // Instances look like: pid_12345_type_0_Graphical_0
    let rest = instance.strip_prefix("pid_")?;
    let pid_str = rest.split('_').next()?;
    pid_str.parse().ok()
}

fn pdh_enum_object_items(object: &str) -> Result<Vec<String>, ()> {
    use windows::Win32::System::Performance::PdhEnumObjectItemsW;

    let mut buf_len: u32 = 0;
    let mut count: u32 = 0;
    let object_wide = wide(object);
    let status = unsafe {
        PdhEnumObjectItemsW(
            None,
            None,
            PCWSTR(object_wide.as_ptr()),
            None,
            &mut buf_len,
            None,
            &mut count,
            PERF_DETAIL(PERF_DETAIL_WIZARD),
            0,
        )
    };
    if status != ERROR_SUCCESS.0 && status != windows::Win32::Foundation::ERROR_MORE_DATA.0 {
        return Err(());
    }
    if buf_len == 0 {
        return Ok(Vec::new());
    }
    let mut buffer = vec![0u16; buf_len as usize];
    let status = unsafe {
        PdhEnumObjectItemsW(
            None,
            None,
            PCWSTR(object_wide.as_ptr()),
            Some(windows::core::PWSTR(buffer.as_mut_ptr())),
            &mut buf_len,
            None,
            &mut count,
            PERF_DETAIL(PERF_DETAIL_WIZARD),
            0,
        )
    };
    if status != ERROR_SUCCESS.0 {
        return Err(());
    }
    Ok(split_double_null_wide(&buffer))
}

const PERF_DETAIL_WIZARD: u32 = 100;

fn split_double_null_wide(buffer: &[u16]) -> Vec<String> {
    let mut items = Vec::new();
    let mut start = 0usize;
    for (index, &ch) in buffer.iter().enumerate() {
        if ch == 0 {
            if index > start {
                items.push(String::from_utf16_lossy(&buffer[start..index]));
            }
            if index + 1 < buffer.len() && buffer[index + 1] == 0 {
                break;
            }
            start = index + 1;
        }
    }
    items
}

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

impl Drop for PdhGpuProbe {
    fn drop(&mut self) {
        let inner = self.inner.lock();
        if let Some(query) = inner.query.filter(|&q| q != 0) {
            unsafe {
                let _ = PdhCloseQuery(PDH_HQUERY(query as *mut _));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pid_from_gpu_engine_instance_names() {
        assert_eq!(
            parse_pid_from_gpu_instance("pid_4242_type_0_Graphical_0"),
            Some(4242)
        );
        assert_eq!(parse_pid_from_gpu_instance("not_a_pid"), None);
    }
}
