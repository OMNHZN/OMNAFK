//! Virtual gamepad keepalive via ViGEmBus.
//!
//! Creates a virtual Xbox 360 or DualShock 4 pad through the ViGEmBus kernel
//! driver and sends a brief left-stick nudge. This keeps controller-gated games
//! awake — games that ignore synthetic keyboard/mouse input. Xbox 360 pads are
//! read via XInput; DualShock 4 pads are read by DirectInput/HID games (and are
//! invisible to XInput, so the DS4 nudge never trips OMNAFK's own sense probe).
//!
//! Safety: this is opt-in. The virtual pad is only created the first time a
//! gamepad nudge actually runs (i.e. the user selected the Gamepad nudge
//! action for a target), never just because the driver is installed. Sending
//! synthetic controller input may violate some games' terms of service — the
//! same caveat that applies to OMNAFK's keyboard/mouse keepalives.

use crate::config::GamepadKind;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicI32, AtomicU8, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

const NUDGE_HOLD: Duration = Duration::from_millis(40);
const XBOX_DEFLECTION: i16 = 16_000;
const DS4_CENTER: u8 = 0x80;
const DS4_DEFLECTION: u8 = 0xC8;

/// XInput slot occupied by our virtual pad, or -1 when not plugged in (or when
/// the pad is a DS4, which XInput can't see). The activity-sense probe reads
/// this so OMNAFK's own Xbox nudges don't look like the user playing.
static EXCLUDED_SLOT: AtomicI32 = AtomicI32::new(-1);
/// Requested pad kind, mirrored from config (see `KIND_XBOX360`/`KIND_DUALSHOCK4`).
static KIND: AtomicU8 = AtomicU8::new(KIND_XBOX360);
const KIND_XBOX360: u8 = 0;
const KIND_DUALSHOCK4: u8 = 1;

enum Pad {
    Xbox(vigem_client::Xbox360Wired<vigem_client::Client>),
    Ds4(vigem_client::DualShock4Wired<vigem_client::Client>),
}

struct Active {
    pad: Pad,
    kind: GamepadKind,
}

// vigem_client targets are not Send by default; the injector is only ever
// touched while holding the mutex on the keepalive execution thread.
unsafe impl Send for Active {}

fn cell() -> &'static Mutex<Option<Active>> {
    static INJECTOR: OnceLock<Mutex<Option<Active>>> = OnceLock::new();
    INJECTOR.get_or_init(|| Mutex::new(None))
}

/// Mirror the configured pad kind so the next nudge uses it.
pub fn set_kind(kind: GamepadKind) {
    let encoded = match kind {
        GamepadKind::Xbox360 => KIND_XBOX360,
        GamepadKind::DualShock4 => KIND_DUALSHOCK4,
    };
    KIND.store(encoded, Ordering::Relaxed);
}

fn requested_kind() -> GamepadKind {
    match KIND.load(Ordering::Relaxed) {
        KIND_DUALSHOCK4 => GamepadKind::DualShock4,
        _ => GamepadKind::Xbox360,
    }
}

/// The XInput slot the virtual pad occupies, if currently plugged in.
pub fn excluded_slot() -> Option<u32> {
    let slot = EXCLUDED_SLOT.load(Ordering::Relaxed);
    (slot >= 0).then_some(slot as u32)
}

/// Send one left-stick nudge, lazily connecting (and re-plugging if the pad
/// kind changed) on first use. Returns a user-facing error if ViGEmBus is
/// unavailable.
pub fn nudge() -> Result<(), String> {
    let kind = requested_kind();
    let mut guard = cell().lock();
    if guard.as_ref().map(|active| active.kind) != Some(kind) {
        *guard = Some(connect(kind)?);
    }
    let active = guard.as_mut().expect("pad just connected");

    match &mut active.pad {
        Pad::Xbox(target) => {
            let deflect = vigem_client::XGamepad {
                thumb_lx: XBOX_DEFLECTION,
                ..Default::default()
            };
            target.update(&deflect).map_err(send_error)?;
            std::thread::sleep(NUDGE_HOLD);
            target
                .update(&vigem_client::XGamepad::default())
                .map_err(send_error)?;
        }
        Pad::Ds4(target) => {
            let deflect = vigem_client::DS4Report {
                thumb_lx: DS4_DEFLECTION,
                ..Default::default()
            };
            target.update(&deflect).map_err(send_error)?;
            std::thread::sleep(NUDGE_HOLD);
            // Recenter the stick. DS4 axes are unsigned (0x80 is center), unlike
            // the Xbox pad whose neutral state is the all-zero default report.
            target
                .update(&vigem_client::DS4Report {
                    thumb_lx: DS4_CENTER,
                    ..Default::default()
                })
                .map_err(send_error)?;
        }
    }
    Ok(())
}

fn connect(kind: GamepadKind) -> Result<Active, String> {
    let client = vigem_client::Client::connect().map_err(driver_error)?;
    let pad = match kind {
        GamepadKind::Xbox360 => {
            let mut target =
                vigem_client::Xbox360Wired::new(client, vigem_client::TargetId::XBOX360_WIRED);
            target.plugin().map_err(driver_error)?;
            target.wait_ready().map_err(driver_error)?;
            if let Ok(index) = target.get_user_index() {
                EXCLUDED_SLOT.store(index as i32, Ordering::Relaxed);
            }
            Pad::Xbox(target)
        }
        GamepadKind::DualShock4 => {
            let mut target = vigem_client::DualShock4Wired::new(
                client,
                vigem_client::TargetId::DUALSHOCK4_WIRED,
            );
            target.plugin().map_err(driver_error)?;
            target.wait_ready().map_err(driver_error)?;
            // A DS4 is not an XInput device, so nothing to exclude from the probe.
            EXCLUDED_SLOT.store(-1, Ordering::Relaxed);
            Pad::Ds4(target)
        }
    };
    Ok(Active { pad, kind })
}

fn driver_error(_err: vigem_client::Error) -> String {
    "Couldn't reach the gamepad driver — install ViGEmBus from \
     https://github.com/nefarius/ViGEmBus/releases to use gamepad nudges."
        .to_string()
}

fn send_error(_err: vigem_client::Error) -> String {
    "Couldn't send to the virtual gamepad — reconnect the ViGEmBus driver or restart OMNAFK."
        .to_string()
}
