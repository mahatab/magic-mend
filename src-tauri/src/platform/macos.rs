//! Mac implementations: pasteboard, key simulation with Cmd, the frontmost app, and the
//! Accessibility permission. The "window handle" on Mac is the app's process id, because
//! Replace returns focus by activating the app.

use super::{ClipboardSnapshot, WindowInfo};
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use objc2::rc::Retained;
use objc2::AnyThread;
use objc2_app_kit::{
    NSApplicationActivationOptions, NSBitmapImageFileType, NSBitmapImageRep, NSPasteboard, NSPasteboardItem,
    NSRunningApplication, NSWorkspace,
};
use objc2_foundation::{NSArray, NSData, NSDictionary, NSString};
use std::ffi::c_void;
use std::sync::OnceLock;
use std::time::{Instant, SystemTime};

const KVK_ANSI_C: u16 = 8;
const KVK_ANSI_V: u16 = 9;
const CONCEALED: &str = "org.nspasteboard.ConcealedType";
const TRANSIENT: &str = "org.nspasteboard.TransientType";
const STRING_TYPE: &str = "public.utf8-plain-text";

// Accessibility API (ApplicationServices).
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> *const c_void;
    fn AXUIElementCopyAttributeValue(element: *const c_void, attribute: *const c_void, value: *mut *const c_void) -> i32;
}
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceFlagsState(state: i32) -> u64;
}
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const c_void);
}

fn pasteboard() -> Retained<NSPasteboard> {
    NSPasteboard::generalPasteboard()
}

pub fn clipboard_seq() -> u64 {
    pasteboard().changeCount() as u64
}

/// Saves every item and every type on the pasteboard.
pub fn save_clipboard() -> ClipboardSnapshot {
    let mut snap = ClipboardSnapshot::default();
    let pb = pasteboard();
    let Some(items) = pb.pasteboardItems() else { return snap };
    for item in items.iter() {
        let mut saved = Vec::new();
        for t in item.types().iter() {
            match item.dataForType(&t) {
                Some(d) => saved.push((t.to_string(), d.to_vec())),
                None => snap.unrestorable += 1,
            }
        }
        snap.items.push(saved);
    }
    snap
}

pub fn restore_clipboard(snap: &ClipboardSnapshot) -> bool {
    let pb = pasteboard();
    pb.clearContents();
    if snap.items.is_empty() {
        return true;
    }
    let mut objs = Vec::new();
    for saved in &snap.items {
        let item = NSPasteboardItem::new();
        for (t, bytes) in saved {
            let data = NSData::with_bytes(bytes);
            item.setData_forType(&data, &NSString::from_str(t));
        }
        objs.push(item);
    }
    let refs: Vec<&NSPasteboardItem> = objs.iter().map(|o| &**o).collect();
    let array = NSArray::from_slice(&refs);
    // SAFETY: NSPasteboardItem conforms to NSPasteboardWriting.
    unsafe { pb.writeObjects(std::mem::transmute::<&NSArray<NSPasteboardItem>, &NSArray<objc2::runtime::ProtocolObject<dyn objc2_app_kit::NSPasteboardWriting>>>(&array)) }
}

pub fn read_clipboard_text() -> Option<String> {
    pasteboard().stringForType(&NSString::from_str(STRING_TYPE)).map(|s| s.to_string())
}

/// Private values carry the concealed type, so clipboard history managers skip them.
pub fn write_clipboard_text(text: &str, private: bool) -> bool {
    let pb = pasteboard();
    pb.clearContents();
    let ok = pb.setString_forType(&NSString::from_str(text), &NSString::from_str(STRING_TYPE));
    if private {
        let empty = NSData::new();
        pb.setData_forType(Some(&empty), &NSString::from_str(CONCEALED));
        pb.setData_forType(Some(&empty), &NSString::from_str(TRANSIENT));
    }
    ok
}

fn send_cmd(key: u16, letter: super::Letter) -> bool {
    let Ok(src) = CGEventSource::new(CGEventSourceStateID::HIDSystemState) else { return false };
    let (Ok(down), Ok(up)) = (CGEvent::new_keyboard_event(src.clone(), key, true), CGEvent::new_keyboard_event(src, key, false)) else {
        return false;
    };
    down.set_flags(CGEventFlags::CGEventFlagCommand);
    up.set_flags(CGEventFlags::CGEventFlagCommand);
    super::record_injected(letter);
    down.post(CGEventTapLocation::HID);
    up.post(CGEventTapLocation::HID);
    true
}

pub fn send_copy() -> bool {
    send_cmd(KVK_ANSI_C, super::Letter::C)
}

pub fn send_paste() -> bool {
    send_cmd(KVK_ANSI_V, super::Letter::V)
}

pub fn modifiers_held(_extra_vk: Option<u16>) -> bool {
    // 1 is kCGEventSourceStateHIDSystemState.
    let flags = CGEventFlags::from_bits_truncate(unsafe { CGEventSourceFlagsState(1) });
    let mods = CGEventFlags::CGEventFlagCommand
        | CGEventFlags::CGEventFlagControl
        | CGEventFlags::CGEventFlagAlternate
        | CGEventFlags::CGEventFlagShift;
    flags.intersects(mods)
}

fn frontmost() -> Option<Retained<NSRunningApplication>> {
    NSWorkspace::sharedWorkspace().frontmostApplication()
}

pub fn foreground_handle() -> isize {
    frontmost().map(|a| a.processIdentifier() as isize).unwrap_or(0)
}

pub fn root_handle(h: isize) -> isize {
    h
}

fn app_for(pid: isize) -> Option<Retained<NSRunningApplication>> {
    NSRunningApplication::runningApplicationWithProcessIdentifier(pid as i32)
}

pub fn is_window_alive(h: isize) -> bool {
    h != 0 && app_for(h).map(|a| !a.isTerminated()).unwrap_or(false)
}

pub fn window_visible(h: isize) -> bool {
    is_window_alive(h)
}

/// The focused window's title through the Accessibility API (needs the permission).
fn focused_title(pid: i32) -> String {
    unsafe {
        let app = AXUIElementCreateApplication(pid);
        if app.is_null() {
            return String::new();
        }
        let focused_attr = NSString::from_str("AXFocusedWindow");
        let title_attr = NSString::from_str("AXTitle");
        let mut win: *const c_void = std::ptr::null();
        let mut out = String::new();
        if AXUIElementCopyAttributeValue(app, Retained::as_ptr(&focused_attr) as *const c_void, &mut win) == 0 && !win.is_null() {
            let mut title: *const c_void = std::ptr::null();
            if AXUIElementCopyAttributeValue(win, Retained::as_ptr(&title_attr) as *const c_void, &mut title) == 0 && !title.is_null() {
                let s: &NSString = &*(title as *const NSString);
                out = s.to_string();
                CFRelease(title);
            }
            CFRelease(win);
        }
        CFRelease(app);
        out
    }
}

pub fn window_info(h: isize) -> Option<WindowInfo> {
    let app = app_for(h)?;
    let pid = app.processIdentifier() as u32;
    let path = app.bundleURL().and_then(|u| u.path()).map(|p| p.to_string()).unwrap_or_default();
    let process = app
        .executableURL()
        .and_then(|u| u.lastPathComponent())
        .map(|s| s.to_string())
        .unwrap_or_default();
    let app_name = app.localizedName().map(|s| s.to_string()).unwrap_or_else(|| process.clone());
    Some(WindowInfo { handle: h, pid, process, path, title: focused_title(pid as i32), app_name, elevated: false })
}

pub fn foreground_window() -> Option<WindowInfo> {
    window_info(foreground_handle())
}

/// Re-activates the original app. Replace calls this before the paste shortcut.
pub fn focus_window(h: isize) -> bool {
    let Some(app) = app_for(h) else { return false };
    #[allow(deprecated)]
    let ok = app.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps);
    let deadline = Instant::now() + std::time::Duration::from_millis(400);
    while Instant::now() < deadline {
        if foreground_handle() == h {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    ok && foreground_handle() == h
}

pub fn self_elevated() -> bool {
    false
}

pub fn process_elevated(_pid: u32) -> bool {
    false
}

pub fn self_pid() -> u32 {
    std::process::id()
}

/// Mac popups are placed with the monitor APIs in the app layer, so this returns an empty area.
pub fn work_area(_h: Option<isize>) -> (i32, i32, i32, i32) {
    (0, 0, 0, 0)
}

pub fn cursor_pos() -> (i32, i32) {
    CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .ok()
        .and_then(|s| CGEvent::new(s).ok())
        .map(|e| {
            let p = e.location();
            (p.x as i32, p.y as i32)
        })
        .unwrap_or((0, 0))
}

/// Time asleep: the wall clock keeps running during sleep while the monotonic clock stops.
pub fn sleep_ms_total() -> u64 {
    static START: OnceLock<(Instant, SystemTime)> = OnceLock::new();
    let (i0, s0) = START.get_or_init(|| (Instant::now(), SystemTime::now()));
    let wall = SystemTime::now().duration_since(*s0).map(|d| d.as_millis() as u64).unwrap_or(0);
    wall.saturating_sub(i0.elapsed().as_millis() as u64)
}

pub fn app_icon_png(path: &str) -> Option<Vec<u8>> {
    if path.is_empty() {
        return None;
    }
    let image = NSWorkspace::sharedWorkspace().iconForFile(&NSString::from_str(path));
    let tiff = image.TIFFRepresentation()?;
    let rep = NSBitmapImageRep::initWithData(NSBitmapImageRep::alloc(), &tiff)?;
    let props = NSDictionary::new();
    let png = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &props) }?;
    Some(png.to_vec())
}

pub fn accessibility_granted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

pub fn open_accessibility_settings() {
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn();
}

pub fn windows_build() -> u32 {
    0
}

pub fn round_corners(_h: isize) {}

pub fn apply_vibrancy(w: &tauri::WebviewWindow) {
    use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};
    let _ = apply_vibrancy(w, NSVisualEffectMaterial::Popover, Some(NSVisualEffectState::Active), Some(16.0));
}
