//! Controller activity sense via XInput.
//!
//! `GetLastInputInfo` does not see gamepad input, so a controller-only player
//! would have keepalives fired mid-game. This polls the four XInput slots and
//! reports when a pad was last active — a button/trigger/stick beyond its
//! deadzone, or a state-packet change since the previous poll — so
//! hold-while-playing can respect controller play too. Sense only; no input is
//! injected here.

use parking_lot::Mutex;
use std::time::{Duration, Instant};
use windows::Win32::UI::Input::XboxController::{XInputGetState, XINPUT_GAMEPAD, XINPUT_STATE};

const TRIGGER_THRESHOLD: i32 = 30;
// Stick deadzones from the XInput SDK (sticks range -32768..=32767). Deflection
// beyond these is treated as deliberate input; the left/right values differ
// because Microsoft specifies a larger recommended deadzone for the right stick.
const LEFT_THUMB_DEADZONE: i32 = 7849;
const RIGHT_THUMB_DEADZONE: i32 = 8689;
const MAX_SLOTS: u32 = 4;

#[derive(Debug, Default)]
pub struct XInputProbe {
    inner: Mutex<ProbeState>,
}

#[derive(Debug, Default)]
struct ProbeState {
    last_packet: [u32; 4],
    connected: [bool; 4],
    last_active: Option<Instant>,
}

impl XInputProbe {
    /// Poll all slots, updating the last-active timestamp when any controller
    /// shows current deflection or a state change since the previous poll.
    pub fn poll(&self, now: Instant) {
        let mut state = self.inner.lock();
        let excluded = crate::gamepad_send::excluded_slot();
        for slot in 0..MAX_SLOTS {
            // Ignore OMNAFK's own virtual pad so its nudges don't look like play.
            if excluded == Some(slot) {
                continue;
            }
            let mut xstate = XINPUT_STATE::default();
            let res = unsafe { XInputGetState(slot, &mut xstate) };
            let idx = slot as usize;
            if res != 0 {
                // ERROR_DEVICE_NOT_CONNECTED (or any error): slot is empty.
                state.connected[idx] = false;
                continue;
            }
            let packet = xstate.dwPacketNumber;
            // Only count a packet change as activity if the pad was already
            // connected last poll — a fresh plug-in shouldn't read as play.
            let changed = state.connected[idx] && packet != state.last_packet[idx];
            state.connected[idx] = true;
            state.last_packet[idx] = packet;
            if changed || gamepad_active(&xstate.Gamepad) {
                state.last_active = Some(now);
            }
        }
    }

    /// Time since the most recent controller activity, or `None` if no pad has
    /// been active since the probe started.
    pub fn last_active_age(&self, now: Instant) -> Option<Duration> {
        self.inner
            .lock()
            .last_active
            .map(|at| now.saturating_duration_since(at))
    }
}

/// Whether a gamepad snapshot shows the user actively holding an input.
fn gamepad_active(gamepad: &XINPUT_GAMEPAD) -> bool {
    if gamepad.wButtons.0 != 0 {
        return true;
    }
    if gamepad.bLeftTrigger as i32 > TRIGGER_THRESHOLD
        || gamepad.bRightTrigger as i32 > TRIGGER_THRESHOLD
    {
        return true;
    }
    beyond(gamepad.sThumbLX, LEFT_THUMB_DEADZONE)
        || beyond(gamepad.sThumbLY, LEFT_THUMB_DEADZONE)
        || beyond(gamepad.sThumbRX, RIGHT_THUMB_DEADZONE)
        || beyond(gamepad.sThumbRY, RIGHT_THUMB_DEADZONE)
}

/// Whether a stick axis is deflected past its deadzone in either direction.
fn beyond(axis: i16, deadzone: i32) -> bool {
    (axis as i32).abs() > deadzone
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::UI::Input::XboxController::XINPUT_GAMEPAD_BUTTON_FLAGS;

    #[test]
    fn neutral_pad_is_inactive() {
        assert!(!gamepad_active(&XINPUT_GAMEPAD::default()));
    }

    #[test]
    fn button_press_is_active() {
        let pad = XINPUT_GAMEPAD {
            wButtons: XINPUT_GAMEPAD_BUTTON_FLAGS(0x1000),
            ..Default::default()
        };
        assert!(gamepad_active(&pad));
    }

    #[test]
    fn stick_beyond_deadzone_is_active() {
        let inside = XINPUT_GAMEPAD {
            sThumbLX: 4000,
            ..Default::default()
        };
        assert!(!gamepad_active(&inside));
        let outside = XINPUT_GAMEPAD {
            sThumbLX: 20000,
            ..Default::default()
        };
        assert!(gamepad_active(&outside));
    }

    #[test]
    fn trigger_pull_is_active() {
        let pad = XINPUT_GAMEPAD {
            bRightTrigger: 200,
            ..Default::default()
        };
        assert!(gamepad_active(&pad));
    }
}
