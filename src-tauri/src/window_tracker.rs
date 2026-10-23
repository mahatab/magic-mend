//! The window that was active at hotkey time: process name, title, handle, and app icon.
//! Nothing else is read from the window (spec, "Privacy and safety").

use crate::platform::{imp, WindowInfo};
use base64::Engine as _;
use std::collections::HashMap;
use std::sync::Mutex;

static ICONS: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);

pub fn active() -> Option<WindowInfo> {
    imp::foreground_window()
}

/// The app icon as a data URL, cached per executable.
pub fn icon_data_url(path: &str) -> Option<String> {
    let mut guard = ICONS.lock().ok()?;
    let cache = guard.get_or_insert_with(HashMap::new);
    if let Some(v) = cache.get(path) {
        return v.clone();
    }
    let v = imp::app_icon_png(path)
        .map(|png| format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)));
    cache.insert(path.to_string(), v.clone());
    v
}
