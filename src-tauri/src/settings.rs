//! Settings saved as a local JSON file. No cloud sync (spec, "Settings").

use crate::channel::{default_excluded_apps, default_rules, ChannelRule};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Bump when the setup wizard gains a step, so only the new step is shown.
pub const SETUP_VERSION: u32 = 1;

pub const WIZARD_STEPS: [&str; 7] = ["welcome", "connect", "access", "hotkey", "defaults", "try", "done"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub hotkey: String,
    pub default_tone: String,
    pub default_length: String,
    pub default_toggles: Vec<String>,
    pub drafts: u8,
    pub model: String,
    pub warm_mode: bool,
    pub priming: bool,
    pub theme: String,
    pub start_at_login: bool,
    pub popup_position: String,
    pub excluded_apps: Vec<String>,
    pub channel_rules: Vec<ChannelRule>,
    pub claude_path: Option<String>,
    pub no_contractions: bool,
    pub no_em_dashes: bool,
    pub paused: bool,
    pub setup: SetupProgress,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct SetupProgress {
    pub version: u32,
    pub completed: Vec<String>,
    /// Steps added in a later version that the user still has to see.
    pub pending_new: Vec<String>,
}

pub fn default_hotkey() -> String {
    // Ctrl+Option+R on Mac is the same chord, since Option is Alt.
    "Ctrl+Alt+R".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            hotkey: default_hotkey(),
            default_tone: "Simple".into(),
            default_length: "Same".into(),
            default_toggles: vec![],
            drafts: 2,
            model: "haiku".into(),
            warm_mode: true,
            priming: false,
            theme: "system".into(),
            start_at_login: true,
            popup_position: "center".into(),
            excluded_apps: default_excluded_apps(),
            channel_rules: default_rules(),
            claude_path: None,
            no_contractions: true,
            no_em_dashes: true,
            paused: false,
            setup: SetupProgress::default(),
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        let mut s: Settings = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        s.normalize();
        s
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).expect("settings serialize"))?;
        std::fs::rename(tmp, path)
    }

    fn normalize(&mut self) {
        use crate::protocol::{LENGTHS, TONES, TOGGLES};
        if !TONES.contains(&self.default_tone.as_str()) {
            self.default_tone = "Simple".into();
        }
        if !LENGTHS.contains(&self.default_length.as_str()) {
            self.default_length = "Same".into();
        }
        self.default_toggles.retain(|t| TOGGLES.contains(&t.as_str()));
        self.drafts = self.drafts.clamp(1, 4);
        if self.hotkey.trim().is_empty() {
            self.hotkey = default_hotkey();
        }
        if self.model.trim().is_empty() {
            self.model = "haiku".into();
        }
        if !["center", "cursor"].contains(&self.popup_position.as_str()) {
            self.popup_position = "center".into();
        }
        if !["system", "light", "dark"].contains(&self.theme.as_str()) {
            self.theme = "system".into();
        }
        // A newer version added steps: ask only for those.
        if self.setup.version > 0 && self.setup.version < SETUP_VERSION {
            for step in new_steps_since(self.setup.version) {
                if !self.setup.pending_new.contains(&step.to_string()) {
                    self.setup.pending_new.push(step.to_string());
                }
            }
        }
    }

    /// Steps that must pass before the hotkey turns on: Connect Claude, and Allow access on Mac.
    pub fn required_steps() -> Vec<&'static str> {
        if cfg!(target_os = "macos") {
            vec!["connect", "access"]
        } else {
            vec!["connect"]
        }
    }

    pub fn steps_for_platform() -> Vec<&'static str> {
        WIZARD_STEPS
            .iter()
            .copied()
            .filter(|s| *s != "access" || cfg!(target_os = "macos"))
            .collect()
    }

    pub fn required_done(&self) -> bool {
        Self::required_steps().iter().all(|s| self.setup.completed.iter().any(|c| c == s))
            && self.setup.pending_new.is_empty()
    }

    pub fn setup_finished(&self) -> bool {
        self.setup.version >= SETUP_VERSION && self.setup.pending_new.is_empty()
    }

    /// The first step the user has not completed (or skipped), for "Finish setup".
    pub fn first_unfinished_step(&self) -> &'static str {
        if let Some(p) = self.setup.pending_new.first() {
            if let Some(s) = WIZARD_STEPS.iter().find(|s| *s == p) {
                return s;
            }
        }
        Self::steps_for_platform()
            .into_iter()
            .find(|s| !self.setup.completed.iter().any(|c| c == s))
            .unwrap_or("done")
    }

    pub fn checker_rules(&self) -> crate::checker::Rules {
        crate::checker::Rules { no_contractions: self.no_contractions, no_em_dashes: self.no_em_dashes }
    }
}

/// Steps introduced after a given setup version. Version 1 is the first release.
pub fn new_steps_since(_version: u32) -> Vec<&'static str> {
    Vec::new()
}

pub fn settings_path(config_dir: &Path) -> PathBuf {
    config_dir.join("settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mm-settings-test-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d.join("settings.json")
    }

    #[test]
    fn defaults_match_spec() {
        let s = Settings::default();
        assert_eq!(s.hotkey, "Ctrl+Alt+R");
        assert_eq!((s.default_tone.as_str(), s.default_length.as_str()), ("Simple", "Same"));
        assert!(s.default_toggles.is_empty());
        assert_eq!(s.drafts, 2);
        assert!(s.warm_mode && s.start_at_login && s.no_contractions && s.no_em_dashes);
        assert_eq!(s.theme, "system");
        assert!(s.excluded_apps.iter().any(|a| a == "WindowsTerminal"));
    }

    #[test]
    fn round_trip_and_missing_file() {
        let p = tmp("rt");
        assert_eq!(Settings::load(&p), Settings::default());
        let mut s = Settings::default();
        s.default_tone = "Warm".into();
        s.setup.completed = vec!["connect".into()];
        s.save(&p).unwrap();
        assert_eq!(Settings::load(&p), s);
    }

    #[test]
    fn bad_values_are_normalized() {
        let p = tmp("bad");
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, r#"{"default_tone":"Pirate","drafts":99,"hotkey":"","theme":"neon"}"#).unwrap();
        let s = Settings::load(&p);
        assert_eq!(s.default_tone, "Simple");
        assert_eq!(s.drafts, 4);
        assert_eq!(s.hotkey, "Ctrl+Alt+R");
        assert_eq!(s.theme, "system");
        std::fs::write(&p, "not json").unwrap();
        assert_eq!(Settings::load(&p), Settings::default());
    }

    #[test]
    fn wizard_progress() {
        let mut s = Settings::default();
        assert!(!s.required_done());
        assert_eq!(s.first_unfinished_step(), "welcome");
        s.setup.completed = vec!["welcome".into(), "connect".into()];
        if cfg!(target_os = "macos") {
            assert!(!s.required_done());
            assert_eq!(s.first_unfinished_step(), "access");
        } else {
            assert!(s.required_done());
            assert_eq!(s.first_unfinished_step(), "hotkey");
        }
        s.setup.pending_new = vec!["try".into()];
        assert!(!s.required_done());
        assert_eq!(s.first_unfinished_step(), "try");
    }
}
