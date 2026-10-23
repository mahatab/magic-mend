//! Channel detection from the active window, and the excluded app check
//! (spec, "Channel detection rules" and "Excluded apps").

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChannelRule {
    /// Matched against the process name (case-insensitive substring) when set.
    #[serde(default)]
    pub process: Vec<String>,
    /// Matched against the window title (case-insensitive substring) when set.
    #[serde(default)]
    pub title: Vec<String>,
    /// When true, the title words only count if the process is a web browser.
    #[serde(default)]
    pub browser_only: bool,
    pub channel: String,
}

pub const BROWSERS: [&str; 10] = [
    "chrome", "msedge", "firefox", "brave", "opera", "vivaldi", "arc", "safari", "chromium", "zen",
];

pub fn default_rules() -> Vec<ChannelRule> {
    let r = |process: &[&str], title: &[&str], browser_only: bool, channel: &str| ChannelRule {
        process: process.iter().map(|s| s.to_string()).collect(),
        title: title.iter().map(|s| s.to_string()).collect(),
        browser_only,
        channel: channel.to_string(),
    };
    vec![
        r(&["outlook", "olk", "thunderbird", "hxoutlook", "mail"], &["Outlook", "Thunderbird", "Mail"], false, "Email"),
        r(&[], &["Gmail"], true, "Email"),
        r(&["whatsapp"], &["WhatsApp"], false, "WhatsApp"),
        r(&[], &["LinkedIn"], true, "LinkedIn"),
        r(&[], &["Messenger", "Facebook"], true, "Text"),
        r(&["messages", "telegram", "signal"], &[], false, "Text"),
    ]
}

pub fn is_browser(process: &str) -> bool {
    let p = process.to_lowercase();
    BROWSERS.iter().any(|b| p.trim_end_matches(".exe") == *b || p.starts_with(b))
}

fn contains_ci(hay: &str, needle: &str) -> bool {
    !needle.is_empty() && hay.to_lowercase().contains(&needle.to_lowercase())
}

/// Process name first, then window title. Anything else is Text.
pub fn detect(rules: &[ChannelRule], process: &str, title: &str) -> String {
    let browser = is_browser(process);
    let proc_stem = process.to_lowercase();
    let proc_stem = proc_stem.trim_end_matches(".exe").trim_end_matches(".app");
    // Pass 1: process names.
    for rule in rules {
        if rule.process.iter().any(|p| contains_ci(proc_stem, p)) {
            return rule.channel.clone();
        }
    }
    // Pass 2: window titles.
    for rule in rules {
        if rule.browser_only && !browser {
            continue;
        }
        if rule.title.iter().any(|t| title_has_word(title, t)) {
            return rule.channel.clone();
        }
    }
    "Text".to_string()
}

/// Title words match on word boundaries so "Mail" does not match "Mailchimp pricing".
fn title_has_word(title: &str, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    let t = title.to_lowercase();
    let w = word.to_lowercase();
    let mut start = 0;
    while let Some(pos) = t[start..].find(&w) {
        let a = start + pos;
        let b = a + w.len();
        let before_ok = a == 0 || !t[..a].chars().next_back().unwrap().is_alphanumeric();
        let after_ok = b >= t.len() || !t[b..].chars().next().unwrap().is_alphanumeric();
        if before_ok && after_ok {
            return true;
        }
        start = a + w.len().max(1);
    }
    false
}

/// Password managers, remote desktop clients, and terminals.
pub fn default_excluded_apps() -> Vec<String> {
    [
        // Terminals: a simulated Ctrl+C can stop a program, and a pasted line break can run a command.
        "WindowsTerminal", "OpenConsole", "conhost", "cmd", "powershell", "pwsh", "wt", "mintty",
        "alacritty", "wezterm-gui", "kitty", "Hyper", "Terminal", "iTerm2", "Warp", "Tabby",
        // Password managers.
        "1Password", "Bitwarden", "KeePass", "KeePassXC", "LastPass", "Dashlane", "Enpass",
        "Keeper", "NordPass", "RoboForm", "Proton Pass",
        // Remote desktop clients.
        "mstsc", "msrdc", "Windows App", "TeamViewer", "AnyDesk", "vncviewer", "RustDesk",
        "parsec", "Microsoft Remote Desktop", "Screen Sharing", "Chrome Remote Desktop",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Exact match on the process name without its extension, ignoring case.
pub fn is_excluded(excluded: &[String], process: &str) -> bool {
    let p = process.to_lowercase();
    let stem = p.trim_end_matches(".exe").trim_end_matches(".app");
    excluded.iter().any(|e| {
        let e = e.to_lowercase();
        let e = e.trim_end_matches(".exe").trim_end_matches(".app");
        !e.is_empty() && e == stem
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(p: &str, t: &str) -> String {
        detect(&default_rules(), p, t)
    }

    #[test]
    fn email() {
        assert_eq!(d("OUTLOOK.EXE", "Inbox - me@x.com - Outlook"), "Email");
        assert_eq!(d("olk.exe", "Mail"), "Email");
        assert_eq!(d("thunderbird.exe", "Write: hi"), "Email");
        assert_eq!(d("chrome.exe", "Inbox (3) - me@gmail.com - Gmail - Google Chrome"), "Email");
        assert_eq!(d("msedge.exe", "Compose Mail - Microsoft Edge"), "Email");
    }

    #[test]
    fn whatsapp_linkedin_text() {
        assert_eq!(d("WhatsApp.exe", "WhatsApp"), "WhatsApp");
        assert_eq!(d("ApplicationFrameHost.exe", "WhatsApp"), "WhatsApp");
        assert_eq!(d("chrome.exe", "WhatsApp - Google Chrome"), "WhatsApp");
        assert_eq!(d("chrome.exe", "(2) Messaging | LinkedIn - Google Chrome"), "LinkedIn");
        assert_eq!(d("firefox.exe", "Messenger | Facebook — Mozilla Firefox"), "Text");
        assert_eq!(d("Telegram.exe", "Telegram"), "Text");
        assert_eq!(d("Signal.exe", "Signal"), "Text");
        assert_eq!(d("notepad.exe", "Untitled - Notepad"), "Text");
    }

    #[test]
    fn title_words_need_browser_and_boundaries() {
        // A document named LinkedIn in Word is not the LinkedIn site.
        assert_eq!(d("WINWORD.EXE", "LinkedIn post ideas - Word"), "Text");
        assert_eq!(d("chrome.exe", "Mailchimp pricing - Google Chrome"), "Text");
        assert_eq!(d("notepad.exe", "Outlook notes - Notepad"), "Email"); // Outlook title rule is not browser-only
    }

    #[test]
    fn exclusions() {
        let ex = default_excluded_apps();
        for p in ["WindowsTerminal.exe", "powershell.exe", "pwsh.exe", "cmd.exe", "1Password.exe", "mstsc.exe", "KeePassXC.exe", "Terminal", "iTerm2"] {
            assert!(is_excluded(&ex, p), "{p}");
        }
        for p in ["notepad.exe", "chrome.exe", "WhatsApp.exe", "Code.exe"] {
            assert!(!is_excluded(&ex, p), "{p}");
        }
    }

    #[test]
    fn browser_names() {
        assert!(is_browser("chrome.exe"));
        assert!(is_browser("msedge.exe"));
        assert!(is_browser("Safari"));
        assert!(!is_browser("notepad.exe"));
    }
}
