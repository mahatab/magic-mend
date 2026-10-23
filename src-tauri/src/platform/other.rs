//! Placeholder for platforms other than Windows and Mac, so the crate still compiles.
//! Nothing here simulates keys or touches the clipboard.

use super::{ClipboardSnapshot, WindowInfo};

pub fn clipboard_seq() -> u64 { 0 }
pub fn save_clipboard() -> ClipboardSnapshot { ClipboardSnapshot::default() }
pub fn restore_clipboard(_s: &ClipboardSnapshot) -> bool { true }
pub fn read_clipboard_text() -> Option<String> { None }
pub fn write_clipboard_text(_t: &str, _private: bool) -> bool { false }
pub fn send_copy() -> bool { false }
pub fn send_paste() -> bool { false }
pub fn modifiers_held(_v: Option<u16>) -> bool { false }
pub fn foreground_handle() -> isize { 0 }
pub fn root_handle(h: isize) -> isize { h }
pub fn is_window_alive(_h: isize) -> bool { false }
pub fn window_visible(_h: isize) -> bool { false }
pub fn window_info(_h: isize) -> Option<WindowInfo> { None }
pub fn foreground_window() -> Option<WindowInfo> { None }
pub fn focus_window(_h: isize) -> bool { false }
pub fn self_elevated() -> bool { false }
pub fn process_elevated(_p: u32) -> bool { false }
pub fn self_pid() -> u32 { std::process::id() }
pub fn work_area(_h: Option<isize>) -> (i32, i32, i32, i32) { (0, 0, 0, 0) }
pub fn cursor_pos() -> (i32, i32) { (0, 0) }
pub fn sleep_ms_total() -> u64 { 0 }
pub fn app_icon_png(_p: &str) -> Option<Vec<u8>> { None }
pub fn accessibility_granted() -> bool { true }
pub fn open_accessibility_settings() {}
pub fn windows_build() -> u32 { 0 }
pub fn round_corners(_h: isize) {}
