//! Windows implementations: clipboard, key simulation, foreground window, process details,
//! elevation, app icons, and a job object that ends the claude process with the app.

#![allow(clippy::missing_safety_doc)]

use super::{ClipboardSnapshot, SavedFormat, WindowInfo};
use std::ffi::c_void;
use std::time::{Duration, Instant};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, GlobalFree, HANDLE, HGLOBAL, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetMonitorInfoW, GetObjectW, MonitorFromPoint, MonitorFromWindow, ReleaseDC,
    BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HGDIOBJ, MONITORINFO,
    MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardSequenceNumber,
    IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId, OpenProcess, OpenProcessToken,
    QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::System::WindowsProgramming::QueryUnbiasedInterruptTime;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, VIRTUAL_KEY, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU,
    VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, CreateWindowExW, DestroyIcon, DestroyWindow, EnumChildWindows, GetAncestor, GetCursorPos,
    GetForegroundWindow, GetIconInfo, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow,
    IsWindowVisible, SetForegroundWindow, ShowWindow, GA_ROOT, HICON, HWND_MESSAGE, ICONINFO, SW_RESTORE, SW_SHOWNOACTIVATE,
    WINDOW_EX_STYLE, WINDOW_STYLE,
};

// Standard clipboard formats that are not plain memory blocks.
const CF_BITMAP: u32 = 2;
const CF_METAFILEPICT: u32 = 3;
const CF_PALETTE: u32 = 9;
const CF_ENHMETAFILE: u32 = 14;
const CF_UNICODETEXT: u32 = 13;
const CF_DIB: u32 = 8;
const CF_DIBV5: u32 = 17;
const CF_OWNERDISPLAY: u32 = 0x0080;
const CF_DSPBITMAP: u32 = 0x0082;
const CF_DSPMETAFILEPICT: u32 = 0x0083;
const CF_DSPENHMETAFILE: u32 = 0x008E;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn hwnd_from(h: isize) -> HWND {
    HWND(h as *mut c_void)
}

/// A hidden message-only window that owns the clipboard while we write to it. Without an
/// owner window, SetClipboardData can fail after EmptyClipboard.
struct ClipOwner(HWND);

impl ClipOwner {
    fn new() -> Option<Self> {
        let class = wide("STATIC");
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                PCWSTR(class.as_ptr()),
                PCWSTR::null(),
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
        }
        .ok()?;
        Some(ClipOwner(hwnd))
    }
}

impl Drop for ClipOwner {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}

/// Opens the clipboard, retrying while another app holds it.
struct OpenClip;

impl OpenClip {
    fn open(owner: Option<HWND>) -> Option<Self> {
        let deadline = Instant::now() + Duration::from_millis(500);
        loop {
            if unsafe { OpenClipboard(owner) }.is_ok() {
                return Some(OpenClip);
            }
            if Instant::now() > deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for OpenClip {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

/// Waits until no other program has the clipboard open. Clipboard history and other monitors
/// open it to read each new value; an app that pastes at that moment finds it locked and
/// silently pastes nothing. Being able to open it ourselves (twice, a moment apart) means the
/// readers are done.
pub fn wait_clipboard_free(timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    let mut free_in_a_row = 0;
    while Instant::now() < deadline {
        if unsafe { OpenClipboard(None) }.is_ok() {
            unsafe {
                let _ = CloseClipboard();
            }
            free_in_a_row += 1;
            if free_in_a_row >= 2 {
                return true;
            }
        } else {
            free_in_a_row = 0;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
    false
}

pub fn clipboard_seq() -> u64 {
    unsafe { GetClipboardSequenceNumber() as u64 }
}

unsafe fn global_bytes(h: HANDLE) -> Option<Vec<u8>> {
    let hg = HGLOBAL(h.0);
    let size = GlobalSize(hg);
    if size == 0 {
        return None;
    }
    let p = GlobalLock(hg);
    if p.is_null() {
        return None;
    }
    let bytes = std::slice::from_raw_parts(p as *const u8, size).to_vec();
    let _ = GlobalUnlock(hg);
    Some(bytes)
}

unsafe fn alloc_global(bytes: &[u8]) -> Option<HGLOBAL> {
    let hg = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)).ok()?;
    let p = GlobalLock(hg);
    if p.is_null() {
        let _ = GlobalFree(Some(hg));
        return None;
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), p as *mut u8, bytes.len());
    let _ = GlobalUnlock(hg);
    Some(hg)
}

/// Saves every clipboard format that can be restored. Formats that cannot be copied are
/// counted so the user can be told before Replace runs.
pub fn save_clipboard() -> ClipboardSnapshot {
    let mut snap = ClipboardSnapshot::default();
    let Some(_open) = OpenClip::open(None) else {
        snap.unrestorable = 1;
        return snap;
    };
    unsafe {
        let has_dib = IsClipboardFormatAvailable(CF_DIB).is_ok() || IsClipboardFormatAvailable(CF_DIBV5).is_ok();
        let mut fmt = 0u32;
        loop {
            fmt = EnumClipboardFormats(fmt);
            if fmt == 0 {
                break;
            }
            match fmt {
                // Windows makes a bitmap from the DIB on its own, so skip it when a DIB exists.
                CF_BITMAP if has_dib => continue,
                CF_BITMAP | CF_PALETTE | CF_METAFILEPICT | CF_OWNERDISPLAY | CF_DSPBITMAP | CF_DSPMETAFILEPICT
                | CF_DSPENHMETAFILE => {
                    snap.unrestorable += 1;
                    continue;
                }
                CF_ENHMETAFILE => {
                    // Rare (Office drawings). Office also offers other formats that restore fine.
                    snap.unrestorable += 1;
                    continue;
                }
                _ => {}
            }
            let Ok(h) = GetClipboardData(fmt) else {
                snap.unrestorable += 1;
                continue;
            };
            match global_bytes(h) {
                Some(bytes) => snap.formats.push(SavedFormat { format: fmt, bytes }),
                None => snap.unrestorable += 1,
            }
        }
    }
    snap
}

/// Puts the saved formats back. The restored copy is kept out of clipboard history so the
/// user's history does not get a duplicate entry.
pub fn restore_clipboard(snap: &ClipboardSnapshot) -> bool {
    let owner = ClipOwner::new();
    let Some(_open) = OpenClip::open(owner.as_ref().map(|o| o.0)) else { return false };
    unsafe {
        if EmptyClipboard().is_err() {
            return false;
        }
        let mut ok = true;
        for f in &snap.formats {
            if let Some(hg) = alloc_global(&f.bytes) {
                if SetClipboardData(f.format, Some(HANDLE(hg.0))).is_err() {
                    let _ = GlobalFree(Some(hg));
                    ok = false;
                }
            }
        }
        if !snap.formats.is_empty() {
            set_history_flags(false);
        }
        ok
    }
}

/// Marks the current clipboard content as private to clipboard history and cloud sync.
/// When `exclude_monitors` is true, clipboard monitors are asked to skip it as well.
unsafe fn set_history_flags(exclude_monitors: bool) {
    let zero = 0u32.to_le_bytes();
    for name in ["CanIncludeInClipboardHistory", "CanUploadToCloudClipboard"] {
        let id = RegisterClipboardFormatW(PCWSTR(wide(name).as_ptr()));
        if id != 0 {
            if let Some(hg) = alloc_global(&zero) {
                let _ = SetClipboardData(id, Some(HANDLE(hg.0)));
            }
        }
    }
    if exclude_monitors {
        let id = RegisterClipboardFormatW(PCWSTR(wide("ExcludeClipboardContentFromMonitorProcessing").as_ptr()));
        if id != 0 {
            if let Some(hg) = alloc_global(&[0u8]) {
                let _ = SetClipboardData(id, Some(HANDLE(hg.0)));
            }
        }
    }
}

pub fn read_clipboard_text() -> Option<String> {
    let _open = OpenClip::open(None)?;
    unsafe {
        let h = GetClipboardData(CF_UNICODETEXT).ok()?;
        let bytes = global_bytes(h)?;
        let words: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
        let end = words.iter().position(|w| *w == 0).unwrap_or(words.len());
        Some(String::from_utf16_lossy(&words[..end]))
    }
}

/// Writes text. Private values (the draft used by Replace) are hidden from clipboard history
/// and monitors. The Copy button writes a normal value.
pub fn write_clipboard_text(text: &str, private: bool) -> bool {
    let owner = ClipOwner::new();
    let Some(_open) = OpenClip::open(owner.as_ref().map(|o| o.0)) else { return false };
    // Windows apps expect CRLF line breaks.
    let normalized = text.replace("\r\n", "\n").replace('\n', "\r\n");
    let mut bytes: Vec<u8> = normalized.encode_utf16().flat_map(|w| w.to_le_bytes()).collect();
    bytes.extend_from_slice(&[0, 0]);
    unsafe {
        if EmptyClipboard().is_err() {
            return false;
        }
        let Some(hg) = alloc_global(&bytes) else { return false };
        if SetClipboardData(CF_UNICODETEXT, Some(HANDLE(hg.0))).is_err() {
            let _ = GlobalFree(Some(hg));
            return false;
        }
        if private {
            set_history_flags(true);
        }
    }
    true
}

fn key_input(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    let scan = unsafe { MapVirtualKeyW(vk.0 as u32, MAPVK_VK_TO_VSC) } as u16;
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: super::INJECTED_TAG,
            },
        },
    }
}

/// Sends Ctrl plus one letter. These two shortcuts (Ctrl+C and Ctrl+V) are the only keys the
/// app ever sends to another app.
fn send_ctrl_letter(letter: super::Letter) -> bool {
    let vk = VIRTUAL_KEY(match letter {
        super::Letter::C => b'C',
        super::Letter::V => b'V',
    } as u16);
    let inputs = [
        key_input(VK_CONTROL, false),
        key_input(vk, false),
        key_input(vk, true),
        key_input(VK_CONTROL, true),
    ];
    super::record_injected(letter);
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    sent as usize == inputs.len()
}

pub fn send_copy() -> bool {
    send_ctrl_letter(super::Letter::C)
}

pub fn send_paste() -> bool {
    send_ctrl_letter(super::Letter::V)
}

fn key_down(vk: i32) -> bool {
    (unsafe { GetAsyncKeyState(vk) } as u16 & 0x8000) != 0
}

/// True while any modifier (or the given key) is held.
pub fn modifiers_held(extra_vk: Option<u16>) -> bool {
    let mods = [
        VK_CONTROL, VK_LCONTROL, VK_RCONTROL, VK_MENU, VK_LMENU, VK_RMENU, VK_SHIFT, VK_LSHIFT, VK_RSHIFT, VK_LWIN,
        VK_RWIN,
    ];
    mods.iter().any(|k| key_down(k.0 as i32)) || extra_vk.map(|k| key_down(k as i32)).unwrap_or(false)
}

/// Shows a window without taking focus from the app in front, so the selection can still be
/// copied from it while the popup is already on screen.
pub fn show_without_focus(h: isize) -> bool {
    unsafe {
        let _ = ShowWindow(hwnd_from(h), SW_SHOWNOACTIVATE);
    }
    true
}

pub fn foreground_handle() -> isize {
    unsafe { GetForegroundWindow().0 as isize }
}

/// The top-level window that owns the given window.
pub fn root_handle(h: isize) -> isize {
    unsafe { GetAncestor(hwnd_from(h), GA_ROOT).0 as isize }
}

pub fn is_window_alive(h: isize) -> bool {
    h != 0 && unsafe { IsWindow(Some(hwnd_from(h))).as_bool() }
}

fn window_pid(h: HWND) -> u32 {
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
    pid
}

fn process_path(pid: u32) -> Option<String> {
    unsafe {
        let hp = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        let r = QueryFullProcessImageNameW(hp, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(hp);
        r.ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// Universal Windows apps run inside ApplicationFrameHost. The real app owns a child window.
fn uwp_child_pid(h: HWND, host_pid: u32) -> Option<u32> {
    struct Ctx {
        host: u32,
        found: u32,
    }
    unsafe extern "system" fn cb(child: HWND, lp: LPARAM) -> windows::core::BOOL {
        let ctx = &mut *(lp.0 as *mut Ctx);
        let pid = window_pid(child);
        if pid != ctx.host && pid != 0 {
            ctx.found = pid;
            return false.into();
        }
        true.into()
    }
    let mut ctx = Ctx { host: host_pid, found: 0 };
    unsafe {
        let _ = EnumChildWindows(Some(h), Some(cb), LPARAM(&mut ctx as *mut Ctx as isize));
    }
    (ctx.found != 0).then_some(ctx.found)
}

/// The friendly name from the file's version info, such as "Notepad" or "WhatsApp".
fn file_description(path: &str) -> Option<String> {
    unsafe {
        let wpath = wide(path);
        let size = GetFileVersionInfoSizeW(PCWSTR(wpath.as_ptr()), None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(PCWSTR(wpath.as_ptr()), Some(0), size, data.as_mut_ptr() as *mut c_void).ok()?;
        let mut ptr: *mut c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let q = wide("\\VarFileInfo\\Translation");
        if !VerQueryValueW(data.as_ptr() as *const c_void, PCWSTR(q.as_ptr()), &mut ptr, &mut len).as_bool() || len < 4 {
            return None;
        }
        let lang = *(ptr as *const u16);
        let cp = *((ptr as *const u16).add(1));
        let q = wide(&format!("\\StringFileInfo\\{lang:04x}{cp:04x}\\FileDescription"));
        if !VerQueryValueW(data.as_ptr() as *const c_void, PCWSTR(q.as_ptr()), &mut ptr, &mut len).as_bool() || len == 0 {
            return None;
        }
        let s = std::slice::from_raw_parts(ptr as *const u16, len as usize);
        let end = s.iter().position(|w| *w == 0).unwrap_or(s.len());
        let out = String::from_utf16_lossy(&s[..end]).trim().to_string();
        (!out.is_empty()).then_some(out)
    }
}

pub fn process_elevated(pid: u32) -> bool {
    unsafe {
        let Ok(hp) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
        let r = token_elevated(hp);
        let _ = CloseHandle(hp);
        // When we cannot even read the token, the process is higher than us.
        r.unwrap_or(true)
    }
}

unsafe fn token_elevated(process: HANDLE) -> Option<bool> {
    let mut token = HANDLE::default();
    OpenProcessToken(process, TOKEN_QUERY, &mut token).ok()?;
    let mut elev = TOKEN_ELEVATION::default();
    let mut ret = 0u32;
    let r = GetTokenInformation(
        token,
        TokenElevation,
        Some(&mut elev as *mut _ as *mut c_void),
        std::mem::size_of::<TOKEN_ELEVATION>() as u32,
        &mut ret,
    );
    let _ = CloseHandle(token);
    r.ok()?;
    Some(elev.TokenIsElevated != 0)
}

pub fn self_elevated() -> bool {
    unsafe { token_elevated(GetCurrentProcess()).unwrap_or(false) }
}

pub fn self_pid() -> u32 {
    unsafe { GetCurrentProcessId() }
}

pub fn window_info(h: isize) -> Option<WindowInfo> {
    if h == 0 {
        return None;
    }
    let hwnd = hwnd_from(h);
    let mut pid = window_pid(hwnd);
    let mut title_buf = vec![0u16; 512];
    let n = unsafe { GetWindowTextW(hwnd, &mut title_buf) };
    let title = String::from_utf16_lossy(&title_buf[..n.max(0) as usize]);
    let mut path = process_path(pid).unwrap_or_default();
    if path.to_lowercase().ends_with("applicationframehost.exe") {
        if let Some(child) = uwp_child_pid(hwnd, pid) {
            pid = child;
            path = process_path(pid).unwrap_or(path);
        }
    }
    let process = std::path::Path::new(&path)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let app_name = file_description(&path).unwrap_or_else(|| process.trim_end_matches(".exe").to_string());
    let elevated = !self_elevated() && process_elevated(pid);
    Some(WindowInfo { handle: h, pid, process, path, title, app_name, elevated })
}

pub fn foreground_window() -> Option<WindowInfo> {
    window_info(foreground_handle())
}

/// Brings a window back to the front. Windows limits which process may take focus; we are
/// allowed because the user just clicked our popup. The thread-attach step covers the rest.
pub fn focus_window(h: isize) -> bool {
    if !is_window_alive(h) {
        return false;
    }
    let hwnd = hwnd_from(h);
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        if SetForegroundWindow(hwnd).as_bool() && same_root(foreground_handle(), h) {
            return true;
        }
        let fg = GetForegroundWindow();
        let fg_thread = GetWindowThreadProcessId(fg, None);
        let me = GetCurrentThreadId();
        let attached = fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, true).as_bool();
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
        if attached {
            let _ = AttachThreadInput(me, fg_thread, false);
        }
    }
    same_root(foreground_handle(), h)
}

fn same_root(a: isize, b: isize) -> bool {
    a != 0 && (a == b || root_handle(a) == root_handle(b))
}

pub fn window_visible(h: isize) -> bool {
    is_window_alive(h) && unsafe { IsWindowVisible(hwnd_from(h)).as_bool() }
}

/// Work area (left, top, right, bottom) in physical pixels of the monitor showing the window,
/// or of the monitor under the cursor when there is no window.
pub fn work_area(h: Option<isize>) -> (i32, i32, i32, i32) {
    unsafe {
        let mon = match h.filter(|h| is_window_alive(*h)) {
            Some(h) => MonitorFromWindow(hwnd_from(h), MONITOR_DEFAULTTONEAREST),
            None => MonitorFromPoint(cursor_pos_point(), MONITOR_DEFAULTTONEAREST),
        };
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(mon, &mut mi).as_bool() {
            let r: RECT = mi.rcWork;
            (r.left, r.top, r.right, r.bottom)
        } else {
            (0, 0, 1920, 1080)
        }
    }
}

fn cursor_pos_point() -> POINT {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    p
}

pub fn cursor_pos() -> (i32, i32) {
    let p = cursor_pos_point();
    (p.x, p.y)
}

/// Milliseconds the computer spent asleep since boot. It grows when the machine sleeps, which
/// is how the app knows to recycle the warm process after waking.
pub fn sleep_ms_total() -> u64 {
    unsafe {
        let tick = GetTickCount64();
        let mut unbiased = 0u64;
        if !QueryUnbiasedInterruptTime(&mut unbiased).as_bool() {
            return 0;
        }
        tick.saturating_sub(unbiased / 10_000)
    }
}

/// The app's icon as a PNG, so the popup header can show where Replace will paste.
pub fn app_icon_png(path: &str) -> Option<Vec<u8>> {
    if path.is_empty() {
        return None;
    }
    unsafe {
        let wpath = wide(path);
        let mut info = SHFILEINFOW::default();
        let r = SHGetFileInfoW(
            PCWSTR(wpath.as_ptr()),
            windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if r == 0 || info.hIcon.is_invalid() {
            return None;
        }
        let png = icon_to_png(info.hIcon);
        let _ = DestroyIcon(info.hIcon);
        png
    }
}

unsafe fn icon_to_png(icon: HICON) -> Option<Vec<u8>> {
    let mut ii = ICONINFO::default();
    GetIconInfo(icon, &mut ii).ok()?;
    let color: HBITMAP = ii.hbmColor;
    let mask: HBITMAP = ii.hbmMask;
    let cleanup = |c: HBITMAP, m: HBITMAP| {
        if !c.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(c.0));
        }
        if !m.is_invalid() {
            let _ = DeleteObject(HGDIOBJ(m.0));
        }
    };
    if color.is_invalid() {
        cleanup(color, mask);
        return None;
    }
    let mut bm = BITMAP::default();
    if GetObjectW(HGDIOBJ(color.0), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut c_void)) == 0 {
        cleanup(color, mask);
        return None;
    }
    let (w, h) = (bm.bmWidth, bm.bmHeight);
    let mut bi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut px = vec![0u8; (w * h * 4) as usize];
    let dc = GetDC(None);
    let lines = GetDIBits(dc, color, 0, h as u32, Some(px.as_mut_ptr() as *mut c_void), &mut bi, DIB_RGB_COLORS);
    ReleaseDC(None, dc);
    cleanup(color, mask);
    if lines == 0 {
        return None;
    }
    let has_alpha = px.as_chunks::<4>().0.iter().any(|p| p[3] != 0);
    for p in px.as_chunks_mut::<4>().0 {
        p.swap(0, 2); // BGRA to RGBA
        if !has_alpha {
            p[3] = 255;
        }
    }
    super::encode_png(w as u32, h as u32, &px)
}

/// A job object that ends the claude process (and anything it starts) when Magic Mend exits,
/// even after a crash.
pub struct KillJob(HANDLE);

unsafe impl Send for KillJob {}

impl KillJob {
    pub fn attach(child: &std::process::Child) -> Option<KillJob> {
        use std::os::windows::io::AsRawHandle;
        unsafe {
            let job = CreateJobObjectW(None, PCWSTR::null()).ok()?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .is_err()
            {
                let _ = CloseHandle(job);
                return None;
            }
            let ph = HANDLE(child.as_raw_handle());
            if AssignProcessToJobObject(job, ph).is_err() {
                let _ = CloseHandle(job);
                return None;
            }
            Some(KillJob(job))
        }
    }
}

impl Drop for KillJob {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// The Windows build number, read from the registry (11 is build 22000 and later).
pub fn windows_build() -> u32 {
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    let key = wide(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion");
    let val = wide("CurrentBuildNumber");
    let mut buf = [0u16; 32];
    let mut len = (buf.len() * 2) as u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR(key.as_ptr()),
            PCWSTR(val.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut c_void),
            Some(&mut len),
        )
    };
    if r.is_err() {
        return 0;
    }
    let end = buf.iter().position(|w| *w == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end]).trim().parse().unwrap_or(0)
}

/// Asks Windows 11 for rounded corners on a frameless window.
pub fn round_corners(h: isize) {
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
    let pref = DWMWCP_ROUND;
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd_from(h),
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &pref as *const _ as *const c_void,
            std::mem::size_of_val(&pref) as u32,
        );
    }
}

/// Windows needs no extra permission for key simulation.
pub fn accessibility_granted() -> bool {
    true
}

pub fn open_accessibility_settings() {}
