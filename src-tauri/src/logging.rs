//! The log holds error categories, timings, and the app version only. Never message text.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static LOG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
const MAX_BYTES: u64 = 256 * 1024;

pub fn init(dir: PathBuf) {
    let _ = std::fs::create_dir_all(&dir);
    *LOG_PATH.lock().unwrap() = Some(dir.join("magic-mend.log"));
    event("app_start", &[("version", env!("CARGO_PKG_VERSION"))]);
}

pub fn path() -> Option<PathBuf> {
    LOG_PATH.lock().ok()?.clone()
}

/// Writes one line. Values must be categories or numbers, never user text, so keys and values
/// are restricted to a safe character set.
pub fn event(name: &str, fields: &[(&str, &str)]) {
    let Some(path) = path() else { return };
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > MAX_BYTES {
            let _ = std::fs::rename(&path, path.with_extension("log.old"));
        }
    }
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let mut line = format!("{ts} {}", safe(name));
    for (k, v) in fields {
        line.push_str(&format!(" {}={}", safe(k), safe(v)));
    }
    line.push('\n');
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(line.as_bytes());
    }
}

pub fn timing(name: &str, ms: u128) {
    event(name, &[("ms", &ms.to_string())]);
}

/// Keeps only short identifiers: letters, digits, dot, dash, underscore. Anything longer than
/// 40 characters is cut, so a sentence can never end up in the log by mistake.
pub fn safe(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .take(40)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_strips_sentences() {
        assert_eq!(safe("not_signed_in"), "not_signed_in");
        assert_eq!(safe("hi chi, we had a great stay"), "hichiwehadagreatstay");
        assert!(safe(&"x".repeat(200)).len() <= 40);
    }
}
