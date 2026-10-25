//! Replace and Copy (spec, "User flow" step 7). Replace never presses Enter: its only key
//! output is the paste shortcut (classic Windows edit boxes get a paste message and no key at
//! all), and the pasted text never ends with a line break.

use crate::platform::imp;
use crate::protocol::clean_draft;
use serde::Serialize;
use std::time::Duration;

pub const FOCUS_SETTLE: Duration = Duration::from_millis(80);
pub const PASTE_SETTLE: Duration = Duration::from_millis(500);
/// How long to wait for other programs to let go of the clipboard before pasting.
pub const CLIPBOARD_FREE_WAIT: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ReplaceOutcome {
    Pasted,
    /// The window is gone or would not take focus. The draft was left on the clipboard instead.
    WindowLost,
    /// The target runs as administrator and ignores our keys.
    Elevated,
    Failed { reason: String },
}

/// Pastes the draft into the target window. `hide_popup` is called after the target has focus.
pub fn replace(target: isize, target_elevated: bool, draft: &str, hide_popup: impl FnOnce()) -> ReplaceOutcome {
    let text = clean_draft(draft);
    if target_elevated {
        copy(&text);
        return ReplaceOutcome::Elevated;
    }
    if !imp::is_window_alive(target) {
        copy(&text);
        return ReplaceOutcome::WindowLost;
    }
    let snapshot = imp::save_clipboard();
    if !imp::write_clipboard_text(&text, true) {
        imp::restore_clipboard(&snapshot);
        return ReplaceOutcome::Failed { reason: "Magic Mend could not use the clipboard.".into() };
    }
    // Let clipboard monitors finish reading the draft before the target app needs it.
    std::thread::sleep(Duration::from_millis(30));
    imp::wait_clipboard_free(CLIPBOARD_FREE_WAIT);
    if !imp::focus_window(target) {
        // Leave the draft on the clipboard as a plain copy so the user can paste by hand.
        copy(&text);
        return ReplaceOutcome::WindowLost;
    }
    hide_popup();
    std::thread::sleep(FOCUS_SETTLE);
    // Check again: focus must still be on the target before the paste shortcut is sent.
    if imp::root_handle(imp::foreground_handle()) != imp::root_handle(target) {
        copy(&text);
        return ReplaceOutcome::WindowLost;
    }
    imp::wait_clipboard_free(CLIPBOARD_FREE_WAIT);
    // A paste message returns after the edit box has pasted, so no settle time is needed.
    let ok = imp::paste_without_keys(target).unwrap_or_else(|| {
        let ok = imp::send_paste();
        // Slow apps read the clipboard late.
        std::thread::sleep(PASTE_SETTLE);
        ok
    });
    imp::restore_clipboard(&snapshot);
    if ok {
        ReplaceOutcome::Pasted
    } else {
        ReplaceOutcome::Failed { reason: "Windows blocked the paste.".into() }
    }
}

/// The Copy button: intentionally leaves the draft on the clipboard.
pub fn copy(text: &str) -> bool {
    imp::write_clipboard_text(&clean_draft(text), false)
}
