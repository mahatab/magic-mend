//! Selection capture: save the clipboard, wait for the hotkey to be released, simulate copy,
//! wait for the clipboard to change, read the text, and restore the clipboard
//! (spec, "User flow" step 3).

use crate::platform::imp;
use serde::Serialize;
use std::time::{Duration, Instant};

pub const CLIPBOARD_WAIT: Duration = Duration::from_millis(400);
/// How long we wait for the user to let go of the hotkey before giving up.
pub const RELEASE_WAIT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Capture {
    Text { text: String },
    /// The clipboard did not change, so nothing was selected.
    Nothing,
    /// The selection cannot be read (password field, locked or elevated app).
    Unreadable { reason: String },
}

/// Waits until no modifier key (and not the hotkey letter) is held, so a simulated Ctrl+C is
/// never combined with Alt or Shift into a different shortcut.
pub fn wait_for_release(extra_vk: Option<u16>, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    // A short settle time lets the key-up events arrive.
    std::thread::sleep(Duration::from_millis(15));
    while imp::modifiers_held(extra_vk) {
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    true
}

/// Waits for the clipboard sequence number to move past `before`.
pub fn wait_for_change(before: u64, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if imp::clipboard_seq() != before {
            return true;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    imp::clipboard_seq() != before
}

/// Runs the full capture. The original clipboard is always restored before returning.
pub fn capture(extra_vk: Option<u16>) -> (Capture, CaptureTimings) {
    let mut t = CaptureTimings::default();
    let start = Instant::now();
    if !wait_for_release(extra_vk, RELEASE_WAIT) {
        return (Capture::Unreadable { reason: "The hotkey keys were held down too long.".into() }, t);
    }
    t.release_ms = start.elapsed().as_millis() as u64;
    let snapshot = imp::save_clipboard();
    let before = imp::clipboard_seq();
    if !imp::send_copy() {
        imp::restore_clipboard(&snapshot);
        return (Capture::Unreadable { reason: "Magic Mend could not copy the selection from this app.".into() }, t);
    }
    let changed = wait_for_change(before, CLIPBOARD_WAIT);
    t.copy_ms = start.elapsed().as_millis() as u64 - t.release_ms;
    let result = if changed {
        // Give the app a moment to finish writing every format.
        std::thread::sleep(Duration::from_millis(20));
        match imp::read_clipboard_text() {
            Some(text) if !text.trim().is_empty() => Capture::Text { text: text.trim_end_matches(['\r', '\n']).replace("\r\n", "\n") },
            Some(_) => Capture::Nothing,
            None => Capture::Unreadable { reason: "The selection is not text.".into() },
        }
    } else {
        Capture::Nothing
    };
    imp::restore_clipboard(&snapshot);
    t.total_ms = start.elapsed().as_millis() as u64;
    (result, t)
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CaptureTimings {
    pub release_ms: u64,
    pub copy_ms: u64,
    pub total_ms: u64,
}
