//! Finds OpenAI's official `codex` command, used for ChatGPT. It ships in several places: the
//! npm package, the Codex desktop app, and the ChatGPT extension for VS Code. Copies differ in
//! age, and old ones cannot use current ChatGPT models, so every copy found is asked for its
//! version and the newest wins.
//!
//! Only commands that print a version, a sign-in yes or no, MCP server names, or the model list
//! are run here. No credential file is ever opened.

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[cfg(windows)]
const NAMES: [&str; 3] = ["codex.exe", "codex.cmd", "codex"];
#[cfg(not(windows))]
const NAMES: [&str; 1] = ["codex"];

/// A version such as "codex-cli 0.163.0-alpha.11.2" as comparable numbers.
pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let v = s.split_whitespace().find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()))?;
    let mut parts = v.split(['.', '-']).map(|p| p.parse::<u32>().ok());
    Some((parts.next()??, parts.next()??, parts.next().flatten().unwrap_or(0)))
}

fn push_dir_names(out: &mut Vec<PathBuf>, dir: &Path) {
    for n in NAMES {
        let p = dir.join(n);
        if p.is_file() {
            out.push(p);
        }
    }
}

/// Every place an official codex binary may live.
pub fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let home = dirs::home_dir();
    if let Some(h) = &home {
        push_dir_names(&mut out, &h.join(".local").join("bin"));
        push_dir_names(&mut out, &h.join(".npm-global").join("bin"));
        // ChatGPT extension for VS Code and compatible editors.
        for editor in [".vscode", ".vscode-insiders", ".cursor", ".windsurf"] {
            let ext = h.join(editor).join("extensions");
            if let Ok(rd) = std::fs::read_dir(&ext) {
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().to_lowercase();
                    if !name.starts_with("openai.chatgpt") {
                        continue;
                    }
                    if let Ok(bins) = std::fs::read_dir(e.path().join("bin")) {
                        for b in bins.flatten() {
                            push_dir_names(&mut out, &b.path());
                        }
                    }
                }
            }
        }
    }
    #[cfg(windows)]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            push_dir_names(&mut out, &PathBuf::from(appdata).join("npm"));
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            // The Codex desktop app keeps its command in a versioned folder.
            if let Ok(rd) = std::fs::read_dir(PathBuf::from(&local).join("OpenAI").join("Codex").join("bin")) {
                for e in rd.flatten() {
                    push_dir_names(&mut out, &e.path());
                }
            }
            push_dir_names(&mut out, &PathBuf::from(&local).join("Programs").join("codex"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        for d in ["/opt/homebrew/bin", "/usr/local/bin", "/Applications/Codex.app/Contents/Resources"] {
            push_dir_names(&mut out, Path::new(d));
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for d in std::env::split_paths(&path) {
            push_dir_names(&mut out, &d);
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|p| seen.insert(p.to_string_lossy().to_lowercase()));
    out
}

pub fn version(path: &Path) -> Option<String> {
    let out = crate::claude_path::command(path).arg("--version").output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Found {
    pub path: PathBuf,
    pub version: String,
}

/// The newest copy that runs. The user's own path wins when set and valid.
pub fn resolve(user_path: Option<&str>) -> Option<Found> {
    if let Some(p) = user_path.map(str::trim).filter(|p| !p.is_empty()) {
        let p = PathBuf::from(p);
        return version(&p).map(|v| Found { path: p, version: v });
    }
    static CACHE: Mutex<Option<(std::time::Instant, Option<Found>)>> = Mutex::new(None);
    if let Ok(c) = CACHE.lock() {
        if let Some((at, f)) = c.as_ref() {
            // Desktop app updates move the binary, so look again after a few minutes.
            if at.elapsed() < std::time::Duration::from_secs(300) && f.as_ref().is_some_and(|f| f.path.is_file()) {
                return f.clone();
            }
        }
    }
    let best = candidates()
        .into_iter()
        .filter_map(|p| version(&p).map(|v| Found { path: p, version: v }))
        .max_by_key(|f| parse_version(&f.version).unwrap_or((0, 0, 0)));
    if let Ok(mut c) = CACHE.lock() {
        *c = Some((std::time::Instant::now(), best.clone()));
    }
    best
}

/// `codex login status` prints "Logged in using ChatGPT" (or "using an API key"). Returns the
/// method, or None when signed out.
pub fn sign_in(path: &Path) -> Option<String> {
    let out = crate::claude_path::command(path).args(["login", "status"]).output().ok()?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let line = text.lines().find(|l| l.to_lowercase().contains("logged in"))?;
    if line.to_lowercase().contains("not logged in") || !out.status.success() {
        return None;
    }
    Some(line.trim().to_string())
}

/// Names of the MCP servers in the user's Codex setup, so the engine can switch each one off.
/// Only the names are kept.
pub fn mcp_server_names(path: &Path) -> Vec<String> {
    let Ok(out) = crate::claude_path::command(path).args(["mcp", "list", "--json"]).output() else { return Vec::new() };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else { return Vec::new() };
    let items: Vec<serde_json::Value> = match v {
        serde_json::Value::Array(a) => a,
        serde_json::Value::Object(o) => o.into_values().collect(),
        _ => Vec::new(),
    };
    items
        .iter()
        .filter(|s| s.get("enabled").and_then(|e| e.as_bool()).unwrap_or(true))
        .filter_map(|s| s.get("name").and_then(|n| n.as_str()).map(str::to_string))
        .filter(|n| n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .collect()
}

/// Overrides that switch off the user's MCP servers. A server that comes from a desktop app
/// feature (switched off separately) cannot be overridden by name, so each override is checked
/// with a harmless command first and kept only when Codex accepts it.
pub fn mcp_overrides(path: &Path) -> Vec<String> {
    static CACHE: Mutex<Option<(PathBuf, Vec<String>)>> = Mutex::new(None);
    if let Ok(c) = CACHE.lock() {
        if let Some((p, v)) = c.as_ref() {
            if p == path {
                return v.clone();
            }
        }
    }
    let mut keep = Vec::new();
    for name in mcp_server_names(path) {
        let kv = format!("mcp_servers.{name}.enabled=false");
        let mut trial: Vec<String> = keep.iter().flat_map(|k: &String| ["-c".to_string(), k.clone()]).collect();
        trial.extend(["-c".to_string(), kv.clone()]);
        let ok = crate::claude_path::command(path)
            .args(["mcp", "list", "--json"])
            .args(&trial)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if ok {
            keep.push(kv);
        }
    }
    if let Ok(mut c) = CACHE.lock() {
        *c = Some((path.to_path_buf(), keep.clone()));
    }
    keep
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Model {
    pub slug: String,
    pub name: String,
}

/// The models this ChatGPT account can use, from `codex debug models`.
pub fn models(path: &Path) -> Vec<Model> {
    let Ok(out) = crate::claude_path::command(path).args(["debug", "models"]).output() else { return Vec::new() };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(&out.stdout) else { return Vec::new() };
    let list = v.get("models").cloned().unwrap_or(v);
    let items: Vec<serde_json::Value> = match list {
        serde_json::Value::Array(a) => a,
        serde_json::Value::Object(o) => o.into_values().collect(),
        _ => Vec::new(),
    };
    items
        .iter()
        .filter(|m| m.get("visibility").and_then(|x| x.as_str()).map(|x| x == "list").unwrap_or(true))
        .filter_map(|m| {
            let slug = m.get("slug").or_else(|| m.get("id")).and_then(|x| x.as_str())?.to_string();
            let name = m.get("display_name").and_then(|x| x.as_str()).unwrap_or(&slug).to_string();
            Some(Model { slug, name })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_codex_versions() {
        assert_eq!(parse_version("codex-cli 0.163.0-alpha.11.2"), Some((0, 163, 0)));
        assert_eq!(parse_version("codex-cli 0.122.0-alpha.1"), Some((0, 122, 0)));
        assert_eq!(parse_version("codex-cli 1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("nonsense"), None);
        assert!(parse_version("codex-cli 0.163.0").unwrap() > parse_version("codex-cli 0.122.0").unwrap());
    }

    #[test]
    fn missing_user_path_is_none() {
        assert_eq!(resolve(Some("C:/definitely/not/here/codex.exe")), None);
    }
}
