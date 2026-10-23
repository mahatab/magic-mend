//! One interface per platform feature (clipboard, keys, window tracking), so the Mac port
//! only adds implementations.

use serde::Serialize;
use std::sync::Mutex;

#[cfg(windows)]
pub mod windows;
#[cfg(windows)]
pub use self::windows as imp;

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub use self::macos as imp;

#[cfg(not(any(windows, target_os = "macos")))]
pub mod other;
#[cfg(not(any(windows, target_os = "macos")))]
pub use self::other as imp;

/// Marks key events the app injects, so its own hooks and tests can tell them apart.
pub const INJECTED_TAG: usize = 0x4D4D_4D4D; // "MMMM"

#[derive(Debug, Clone, Default, Serialize)]
pub struct WindowInfo {
    pub handle: isize,
    pub pid: u32,
    pub process: String,
    pub path: String,
    pub title: String,
    pub app_name: String,
    /// The window belongs to an app running as administrator while Magic Mend is not, so
    /// Windows blocks our keys from reaching it.
    pub elevated: bool,
}

#[derive(Debug, Clone)]
pub struct SavedFormat {
    pub format: u32,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct ClipboardSnapshot {
    pub formats: Vec<SavedFormat>,
    /// How many formats could not be saved.
    pub unrestorable: usize,
    /// Mac: the pasteboard items, stored as type and data pairs per item.
    pub items: Vec<Vec<(String, Vec<u8>)>>,
}

impl ClipboardSnapshot {
    pub fn is_empty(&self) -> bool {
        self.formats.is_empty() && self.items.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Letter {
    C,
    V,
}

/// Every shortcut the app has sent to another app. Tests read this to prove that only the
/// copy and paste shortcuts are ever sent.
static INJECTED: Mutex<Vec<Letter>> = Mutex::new(Vec::new());

pub fn record_injected(l: Letter) {
    if let Ok(mut v) = INJECTED.lock() {
        v.push(l);
        let len = v.len();
        if len > 100 {
            v.drain(..len - 100);
        }
    }
}

pub fn injected_log() -> Vec<Letter> {
    INJECTED.lock().map(|v| v.clone()).unwrap_or_default()
}

pub fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().ok()?;
        writer.write_image_data(rgba).ok()?;
    }
    Some(out)
}
