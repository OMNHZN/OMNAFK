//! Per-process audio activity probe via WASAPI session metering.
//!
//! Enumerates audio sessions on the default render endpoint and reports which
//! PIDs are actively emitting sound (peak above a small threshold). A live game
//! almost always renders audio, which helps separate a running game from a
//! backgrounded launcher window. Fails closed (no PIDs active) when WASAPI is
//! unavailable, e.g. on a headless box with no audio endpoint.

use crate::detector::AudioActivityProbe;
use parking_lot::Mutex;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};
use windows::core::Interface;
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioSessionControl2, IAudioSessionEnumerator, IAudioSessionManager2,
    IMMDeviceEnumerator, MMDeviceEnumerator,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
};

const REFRESH_INTERVAL: Duration = Duration::from_secs(4);
/// Linear peak amplitude above which a session counts as actively playing.
const ACTIVE_PEAK: f32 = 0.0015;

pub struct WasapiAudioProbe {
    inner: Mutex<ProbeInner>,
}

struct ProbeInner {
    active: BTreeSet<u32>,
    last_refresh: Instant,
}

thread_local! {
    // Tracks COM init per OS thread. The detection worker can be restarted on a
    // fresh thread by the watchdog, so a process-wide flag would leave the new
    // thread uninitialized; a thread-local re-inits correctly after a restart.
    static COM_INITIALIZED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn ensure_com_initialized() {
    COM_INITIALIZED.with(|flag| {
        if !flag.get() {
            // COINIT_MULTITHREADED matches the worker thread. A repeat call on an
            // already-initialized thread just bumps a refcount; we never need to
            // uninitialize a long-lived worker thread.
            unsafe {
                let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            }
            flag.set(true);
        }
    });
}

impl Default for WasapiAudioProbe {
    fn default() -> Self {
        Self {
            inner: Mutex::new(ProbeInner {
                active: BTreeSet::new(),
                last_refresh: crate::time_util::instant_ttl_ago(REFRESH_INTERVAL),
            }),
        }
    }
}

impl AudioActivityProbe for WasapiAudioProbe {
    fn is_active(&self, pid: u32) -> bool {
        let mut inner = self.inner.lock();
        inner.refresh_if_due();
        inner.active.contains(&pid)
    }
}

impl ProbeInner {
    fn refresh_if_due(&mut self) {
        if self.last_refresh.elapsed() < REFRESH_INTERVAL {
            return;
        }
        self.last_refresh = Instant::now();
        ensure_com_initialized();
        if let Ok(active) = collect_active_pids() {
            self.active = active;
        }
    }
}

fn collect_active_pids() -> windows::core::Result<BTreeSet<u32>> {
    let mut out = BTreeSet::new();
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let manager: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None)?;
        let sessions: IAudioSessionEnumerator = manager.GetSessionEnumerator()?;
        let count = sessions.GetCount()?;
        for index in 0..count {
            let Ok(control) = sessions.GetSession(index) else {
                continue;
            };
            let Ok(control2) = control.cast::<IAudioSessionControl2>() else {
                continue;
            };
            let pid = control2.GetProcessId().unwrap_or(0);
            if pid == 0 {
                continue;
            }
            if let Ok(meter) = control2.cast::<IAudioMeterInformation>() {
                if let Ok(peak) = meter.GetPeakValue() {
                    if peak >= ACTIVE_PEAK {
                        out.insert(pid);
                    }
                }
            }
        }
    }
    Ok(out)
}
