//! Global hotkey helpers: parsing, the key to wait on, and conflict checks.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ParsedHotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
    pub key: String,
}

/// Parses strings such as "Ctrl+Alt+R" or "Ctrl+Option+R".
pub fn parse(s: &str) -> Option<ParsedHotkey> {
    let mut h = ParsedHotkey { ctrl: false, alt: false, shift: false, meta: false, key: String::new() };
    for part in s.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        match part.to_lowercase().as_str() {
            "ctrl" | "control" => h.ctrl = true,
            "alt" | "option" | "opt" => h.alt = true,
            "shift" => h.shift = true,
            "cmd" | "command" | "super" | "meta" | "win" => h.meta = true,
            k => {
                if !h.key.is_empty() {
                    return None;
                }
                h.key = normalize_key(k)?;
            }
        }
    }
    if h.key.is_empty() || !(h.ctrl || h.alt || h.meta) {
        // A hotkey needs at least one of Ctrl, Alt, or Cmd so normal typing never triggers it.
        return None;
    }
    Some(h)
}

fn normalize_key(k: &str) -> Option<String> {
    let up = k.to_uppercase();
    if up.len() == 1 && up.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Some(up);
    }
    if let Some(n) = up.strip_prefix('F').and_then(|n| n.parse::<u8>().ok()) {
        if (1..=24).contains(&n) {
            return Some(format!("F{n}"));
        }
    }
    match up.as_str() {
        "SPACE" => Some("Space".into()),
        _ => None,
    }
}

impl ParsedHotkey {
    /// The string the global shortcut plugin understands.
    pub fn to_plugin_string(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Control".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        if self.meta {
            parts.push("Super".to_string());
        }
        parts.push(self.key.clone());
        parts.join("+")
    }

    /// How the hotkey is shown to the user on this platform.
    pub fn display(&self) -> String {
        let mac = cfg!(target_os = "macos");
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push(if mac { "Option" } else { "Alt" });
        }
        if self.shift {
            parts.push("Shift");
        }
        if self.meta {
            parts.push(if mac { "Cmd" } else { "Win" });
        }
        let mut s = parts.join("+");
        s.push('+');
        s.push_str(&self.key);
        s
    }

    /// The Windows virtual key code of the main key, to wait for its release.
    pub fn virtual_key(&self) -> Option<u16> {
        let k = self.key.as_str();
        if k.len() == 1 {
            return Some(k.as_bytes()[0] as u16);
        }
        if let Some(n) = k.strip_prefix('F').and_then(|n| n.parse::<u16>().ok()) {
            return Some(0x70 + n - 1);
        }
        (k == "Space").then_some(0x20)
    }
}

/// Plain warnings about a hotkey choice. Registration failures are reported separately.
pub fn warnings(h: &ParsedHotkey) -> Vec<String> {
    let mut w = Vec::new();
    if h.ctrl && h.alt && !h.meta && altgr_types_character(h) {
        w.push(format!(
            "On your keyboard layout, {} types a character (Ctrl+Alt works like AltGr). Pick another combination, for example Ctrl+Shift+Space.",
            h.display()
        ));
    }
    let common: &[(&str, &str)] = &[
        ("Control+C", "copy"), ("Control+V", "paste"), ("Control+X", "cut"), ("Control+Z", "undo"),
        ("Control+A", "select all"), ("Control+S", "save"), ("Control+R", "reload in browsers"),
        ("Control+Shift+R", "hard reload in browsers"), ("Control+Shift+V", "paste as plain text"),
        ("Control+Alt+Delete", "the Windows security screen"), ("Super+V", "clipboard history"),
        ("Control+Shift+N", "a new private window in browsers"), ("Control+Shift+T", "reopen closed tab in browsers"),
    ];
    let p = h.to_plugin_string();
    for (combo, what) in common {
        if p.eq_ignore_ascii_case(combo) {
            w.push(format!("{} is already used for {}. Pick another combination.", h.display(), what));
        }
    }
    w
}

#[cfg(windows)]
fn altgr_types_character(h: &ParsedHotkey) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardLayout, MapVirtualKeyExW, ToUnicodeEx, MAPVK_VK_TO_VSC};
    let Some(vk) = h.virtual_key() else { return false };
    unsafe {
        let layout = GetKeyboardLayout(0);
        let mut state = [0u8; 256];
        state[0x11] = 0x80; // VK_CONTROL
        state[0x12] = 0x80; // VK_MENU
        state[0xA2] = 0x80; // VK_LCONTROL
        state[0xA5] = 0x80; // VK_RMENU, the AltGr key
        if h.shift {
            state[0x10] = 0x80;
        }
        let scan = MapVirtualKeyExW(vk as u32, MAPVK_VK_TO_VSC, Some(layout));
        let mut buf = [0u16; 8];
        // Flag 4: do not change the keyboard state (Windows 10 1607 and later).
        let n = ToUnicodeEx(vk as u32, scan, &state, &mut buf, 4, Some(layout));
        n != 0 && buf[0] >= 0x20
    }
}

#[cfg(not(windows))]
fn altgr_types_character(_h: &ParsedHotkey) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_and_mac_names() {
        let h = parse("Ctrl+Alt+R").unwrap();
        assert!(h.ctrl && h.alt && !h.shift && !h.meta);
        assert_eq!(h.key, "R");
        assert_eq!(h.to_plugin_string(), "Control+Alt+R");
        assert_eq!(parse("ctrl+option+r"), Some(h.clone()));
        assert_eq!(h.virtual_key(), Some(b'R' as u16));
        assert_eq!(parse("Ctrl+Shift+F9").unwrap().virtual_key(), Some(0x78));
        assert_eq!(parse("Ctrl+Shift+Space").unwrap().virtual_key(), Some(0x20));
    }

    #[test]
    fn rejects_bad_hotkeys() {
        assert!(parse("R").is_none());
        assert!(parse("Shift+R").is_none());
        assert!(parse("Ctrl+R+T").is_none());
        assert!(parse("Ctrl+").is_none());
        assert!(parse("Ctrl+Enter").is_none());
    }

    #[test]
    fn warns_on_common_shortcuts() {
        assert!(!warnings(&parse("Ctrl+C").unwrap()).is_empty());
        assert!(!warnings(&parse("Ctrl+R").unwrap()).is_empty());
        assert!(warnings(&parse("Ctrl+Shift+Space").unwrap()).is_empty());
    }

    #[test]
    fn warning_text_follows_output_rules() {
        for w in warnings(&parse("Ctrl+V").unwrap()) {
            assert!(crate::checker::check(&w, Default::default()).is_clean(), "{w}");
        }
    }
}
