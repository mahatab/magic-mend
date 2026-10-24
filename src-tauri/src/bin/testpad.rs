//! Test helper for the Windows end-to-end tests. Not shipped.
//!
//!   mm-testpad window <dir> [text]   a window with a text box; writes <dir>/text.txt and logs
//!                                    every key it receives to <dir>/keys.log
//!   mm-testpad keys <combo> [hold_ms] sends a key combination, for example ctrl+alt+r
//!   mm-testpad clip-set <text>        puts text on the clipboard (plus a private test format)
//!   mm-testpad clip-get               prints the clipboard text and whether the test format is there
//!   mm-testpad clip-seq               prints the clipboard sequence number
//!   mm-testpad fg                     prints the foreground window's process and title
//!   mm-testpad focus <title>          brings a window with this title to the front

#[cfg(windows)]
fn main() {
    win::main();
}

#[cfg(not(windows))]
fn main() {
    eprintln!("mm-testpad runs on Windows only");
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{GlobalFree, HANDLE, HWND, LPARAM, LRESULT, WPARAM};
        use windows::Win32::System::DataExchange::*;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    static OUT: Mutex<Option<PathBuf>> = Mutex::new(None);
    static mut OLD_PROC: isize = 0;
    static mut EDIT: isize = 0;

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn log(line: &str) {
        if let Some(dir) = OUT.lock().unwrap().clone() {
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("keys.log")) {
                let _ = writeln!(f, "{line}");
            }
        }
    }

    fn mods() -> String {
        let down = |vk: VIRTUAL_KEY| unsafe { (GetKeyState(vk.0 as i32) as u16 & 0x8000) != 0 };
        let mut m = String::new();
        if down(VK_CONTROL) {
            m.push_str("ctrl+");
        }
        if down(VK_MENU) {
            m.push_str("alt+");
        }
        if down(VK_SHIFT) {
            m.push_str("shift+");
        }
        m
    }

    unsafe extern "system" fn edit_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_KEYDOWN | WM_SYSKEYDOWN => log(&format!("keydown {}0x{:02X}", mods(), wp.0)),
            WM_CHAR => log(&format!("char 0x{:02X}", wp.0)),
            WM_PASTE => log("paste"),
            WM_COPY => log("copy"),
            _ => {}
        }
        let old: WNDPROC = std::mem::transmute(OLD_PROC);
        CallWindowProcW(old, h, msg, wp, lp)
    }

    fn edit_text(h: HWND) -> String {
        unsafe {
            let len = GetWindowTextLengthW(h);
            let mut buf = vec![0u16; len as usize + 1];
            let n = GetWindowTextW(h, &mut buf);
            String::from_utf16_lossy(&buf[..n as usize])
        }
    }

    unsafe extern "system" fn wnd_proc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        match msg {
            WM_SIZE => {
                let w = (lp.0 & 0xFFFF) as i32;
                let hgt = ((lp.0 >> 16) & 0xFFFF) as i32;
                let _ = MoveWindow(HWND(EDIT as *mut c_void), 0, 0, w, hgt, true);
                LRESULT(0)
            }
            WM_TIMER => {
                if let Some(dir) = OUT.lock().unwrap().clone() {
                    let t = edit_text(HWND(EDIT as *mut c_void));
                    let _ = std::fs::write(dir.join("text.tmp"), t.replace("\r\n", "\n"));
                    let _ = std::fs::rename(dir.join("text.tmp"), dir.join("text.txt"));
                }
                LRESULT(0)
            }
            WM_SETFOCUS => {
                let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(HWND(EDIT as *mut c_void)));
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(h, msg, wp, lp),
        }
    }

    fn window(dir: PathBuf, text: String, title: String) {
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(dir.join("keys.log"));
        *OUT.lock().unwrap() = Some(dir);
        unsafe {
            let inst = GetModuleHandleW(None).unwrap();
            let class = w!("MMTestpad");
            let wc = WNDCLASSW {
                lpfnWndProc: Some(wnd_proc),
                hInstance: inst.into(),
                lpszClassName: class,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap(),
                ..Default::default()
            };
            RegisterClassW(&wc);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                PCWSTR(wide(&title).as_ptr()),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                200,
                200,
                640,
                300,
                None,
                None,
                Some(inst.into()),
                None,
            )
            .unwrap();
            let edit = CreateWindowExW(
                WS_EX_CLIENTEDGE,
                w!("EDIT"),
                PCWSTR::null(),
                WS_CHILD | WS_VISIBLE | WS_VSCROLL | WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN) as u32),
                0,
                0,
                620,
                260,
                Some(hwnd),
                None,
                Some(inst.into()),
                None,
            )
            .unwrap();
            EDIT = edit.0 as isize;
            let t = wide(&text.replace('\n', "\r\n"));
            let _ = SetWindowTextW(edit, PCWSTR(t.as_ptr()));
            OLD_PROC = SetWindowLongPtrW(edit, GWLP_WNDPROC, edit_proc as *const () as usize as isize);
            SendMessageW(edit, 0x00B1 /* EM_SETSEL */, Some(WPARAM(0)), Some(LPARAM(-1)));
            let _ = SetForegroundWindow(hwnd);
            let _ = windows::Win32::UI::Input::KeyboardAndMouse::SetFocus(Some(edit));
            SetTimer(Some(hwnd), 1, 50, None);
            println!("ready {}", hwnd.0 as isize);
            let _ = std::io::stdout().flush();
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    fn vk_of(name: &str) -> VIRTUAL_KEY {
        match name.to_lowercase().as_str() {
            "ctrl" | "control" => VK_CONTROL,
            "alt" => VK_MENU,
            "shift" => VK_SHIFT,
            "win" => VK_LWIN,
            "enter" | "return" => VK_RETURN,
            "space" => VK_SPACE,
            "esc" | "escape" => VK_ESCAPE,
            "tab" => VK_TAB,
            "end" => VK_END,
            "home" => VK_HOME,
            k if k.len() == 1 => VIRTUAL_KEY(k.to_uppercase().as_bytes()[0] as u16),
            k if k.starts_with('f') => VIRTUAL_KEY(0x70 + k[1..].parse::<u16>().unwrap() - 1),
            other => panic!("unknown key {other}"),
        }
    }

    fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: unsafe { MapVirtualKeyW(vk.0 as u32, MAPVK_VK_TO_VSC) } as u16,
                    dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    fn keys(combo: &str, hold_ms: u64) {
        let vks: Vec<VIRTUAL_KEY> = combo.split('+').map(vk_of).collect();
        let down: Vec<INPUT> = vks.iter().map(|v| key(*v, false)).collect();
        let up: Vec<INPUT> = vks.iter().rev().map(|v| key(*v, true)).collect();
        unsafe {
            SendInput(&down, std::mem::size_of::<INPUT>() as i32);
            std::thread::sleep(std::time::Duration::from_millis(hold_ms));
            SendInput(&up, std::mem::size_of::<INPUT>() as i32);
        }
    }

    fn clip_set(text: &str) {
        unsafe {
            let class = w!("STATIC");
            let owner = CreateWindowExW(WINDOW_EX_STYLE(0), class, PCWSTR::null(), WINDOW_STYLE(0), 0, 0, 0, 0, Some(HWND_MESSAGE), None, None, None).ok();
            for _ in 0..50 {
                if OpenClipboard(owner).is_ok() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let _ = EmptyClipboard();
            let put = |fmt: u32, bytes: &[u8]| {
                let hg = GlobalAlloc(GMEM_MOVEABLE, bytes.len()).unwrap();
                let p = GlobalLock(hg);
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), p as *mut u8, bytes.len());
                let _ = GlobalUnlock(hg);
                if SetClipboardData(fmt, Some(HANDLE(hg.0))).is_err() {
                    let _ = GlobalFree(Some(hg));
                }
            };
            let mut utf16: Vec<u8> = text.encode_utf16().flat_map(|w| w.to_le_bytes()).collect();
            utf16.extend_from_slice(&[0, 0]);
            put(13, &utf16);
            // A custom format proves that more than plain text is restored.
            let custom = RegisterClipboardFormatW(w!("MagicMendTestFormat"));
            put(custom, b"custom-payload-42");
            let _ = CloseClipboard();
            if let Some(o) = owner {
                let _ = DestroyWindow(o);
            }
        }
    }

    fn clip_get() {
        unsafe {
            for _ in 0..50 {
                if OpenClipboard(None).is_ok() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let read = |fmt: u32| -> Option<Vec<u8>> {
                let h = GetClipboardData(fmt).ok()?;
                let hg = windows::Win32::Foundation::HGLOBAL(h.0);
                let size = windows::Win32::System::Memory::GlobalSize(hg);
                let p = GlobalLock(hg);
                let v = std::slice::from_raw_parts(p as *const u8, size).to_vec();
                let _ = GlobalUnlock(hg);
                Some(v)
            };
            let text = read(13)
                .map(|b| {
                    let w: Vec<u16> = b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
                    let end = w.iter().position(|x| *x == 0).unwrap_or(w.len());
                    String::from_utf16_lossy(&w[..end])
                })
                .unwrap_or_default();
            let custom = RegisterClipboardFormatW(w!("MagicMendTestFormat"));
            let has_custom = read(custom).map(|b| b.starts_with(b"custom-payload-42")).unwrap_or(false);
            let _ = CloseClipboard();
            println!("{}", serde_json::json!({ "text": text, "custom": has_custom }));
        }
    }

    fn fg() {
        let info = magic_mend_lib::platform::imp::foreground_window();
        println!("{}", serde_json::to_string(&info).unwrap());
    }

    fn focus(title: &str) {
        unsafe {
            let t = wide(title);
            if let Ok(h) = FindWindowW(PCWSTR::null(), PCWSTR(t.as_ptr())) {
                let ok = magic_mend_lib::platform::imp::focus_window(h.0 as isize);
                println!("{ok}");
            } else {
                println!("false");
            }
        }
    }

    pub fn main() {
        let args: Vec<String> = std::env::args().collect();
        match args.get(1).map(String::as_str) {
            Some("window") => window(PathBuf::from(&args[2]), args.get(3).cloned().unwrap_or_default(), args.get(4).cloned().unwrap_or_else(|| "MM Testpad".into())),
            Some("keys") => keys(&args[2], args.get(3).and_then(|s| s.parse().ok()).unwrap_or(20)),
            Some("clip-set") => clip_set(&args[2]),
            Some("clip-get") => clip_get(),
            Some("clip-seq") => println!("{}", unsafe { GetClipboardSequenceNumber() }),
            Some("fg") => fg(),
            Some("release") => {
                // Sends key-up events for every key Windows still reports as held.
                let ups: Vec<INPUT> = (8..255u16)
                    .filter(|vk| unsafe { (GetAsyncKeyState(*vk as i32) as u16 & 0x8000) != 0 })
                    .filter(|vk| ![0x01u16, 0x02, 0x04].contains(vk))
                    .map(|vk| key(VIRTUAL_KEY(vk), true))
                    .collect();
                if !ups.is_empty() {
                    unsafe { SendInput(&ups, std::mem::size_of::<INPUT>() as i32) };
                }
                println!("{}", ups.len());
            }
            Some("held") => {
                let held: Vec<String> = (1..255u16)
                    .filter(|vk| unsafe { (GetAsyncKeyState(*vk as i32) as u16 & 0x8000) != 0 })
                    .map(|vk| format!("0x{vk:02X}"))
                    .collect();
                println!("{}", held.join(" "));
            }
            Some("focus") => focus(&args[2]),
            _ => eprintln!("see the comment at the top of testpad.rs"),
        }
    }
}
