//! Magic Mend: select text, press the hotkey, pick a tone, click Replace.
//! The app never sends a message. Its only key output into another app is the paste shortcut
//! after the user clicks Replace.

pub mod actions;
pub mod channel;
pub mod checker;
pub mod claude_path;
pub mod drafts;
pub mod engine;
pub mod engine_manager;
pub mod hotkey;
pub mod logging;
pub mod platform;
pub mod protocol;
pub mod selection;
pub mod settings;
pub mod window_tracker;

mod app;

pub use app::run;

/// The system prompt from the spec, built into the app.
pub const SYSTEM_PROMPT: &str = include_str!("../../prompts/system.txt");

#[cfg(test)]
mod prompt_tests {
    #[test]
    fn prompt_has_the_rules() {
        let p = super::SYSTEM_PROMPT;
        for rule in ["Never use contractions", "Never use em dashes", "never ask questions", "Never put the same word twice"] {
            assert!(p.contains(rule), "missing: {rule}");
        }
    }
}
