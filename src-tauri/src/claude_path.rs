//! Finds the official `claude` command. Apps started from the desktop do not inherit the
//! terminal PATH, so common install folders are searched first (spec, "finding claude").

use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(windows)]
const NAMES: [&str; 3] = ["claude.exe", "claude.cmd", "claude"];
#[cfg(not(windows))]
const NAMES: [&str; 1] = ["claude"];

fn home() -> Option<PathBuf> {
    dirs::home_dir()
}

/// Install folders to search, most likely first.
pub fn candidate_dirs() -> Vec<PathBuf> {
    let mut dirs_out = Vec::new();
    if let Some(h) = home() {
        dirs_out.push(h.join(".local").join("bin"));
        dirs_out.push(h.join(".claude").join("local"));
        dirs_out.push(h.join(".npm-global").join("bin"));
        dirs_out.push(h.join(".bun").join("bin"));
        dirs_out.push(h.join(".volta").join("bin"));
    }
    #[cfg(windows)]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            dirs_out.push(PathBuf::from(appdata).join("npm"));
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            dirs_out.push(PathBuf::from(&local).join("Programs").join("claude"));
            dirs_out.push(PathBuf::from(&local).join("Microsoft").join("WinGet").join("Links"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        dirs_out.push(PathBuf::from("/opt/homebrew/bin"));
        dirs_out.push(PathBuf::from("/usr/local/bin"));
    }
    #[cfg(unix)]
    {
        dirs_out.push(PathBuf::from("/usr/bin"));
    }
    // Then whatever PATH the app did inherit.
    if let Some(path) = std::env::var_os("PATH") {
        dirs_out.extend(std::env::split_paths(&path));
    }
    dirs_out
}

pub fn find_in(dirs_list: &[PathBuf]) -> Option<PathBuf> {
    for d in dirs_list {
        for n in NAMES {
            let p = d.join(n);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// Asks a login shell, as the last step (Mac and Linux).
#[cfg(unix)]
fn from_login_shell() -> Option<PathBuf> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let out = Command::new(shell).args(["-l", "-c", "command -v claude"]).output().ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let p = PathBuf::from(s);
    p.is_file().then_some(p)
}

#[cfg(windows)]
fn from_login_shell() -> Option<PathBuf> {
    None
}

/// The user's own path wins when it exists.
pub fn resolve(user_path: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = user_path.map(str::trim).filter(|p| !p.is_empty()) {
        let p = PathBuf::from(p);
        return p.is_file().then_some(p);
    }
    find_in(&candidate_dirs()).or_else(from_login_shell)
}

/// Builds a command for the claude path. A .cmd shim runs through cmd.exe, still with no window.
pub fn command(path: &Path) -> Command {
    #[allow(unused_mut)]
    let mut cmd = if is_cmd_shim(path) {
        let mut c = Command::new("cmd.exe");
        c.arg("/d").arg("/s").arg("/c").arg(path);
        c
    } else {
        Command::new(path)
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn is_cmd_shim(path: &Path) -> bool {
    path.extension()
        .map(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"))
        .unwrap_or(false)
}

/// Runs `claude --version`.
pub fn version(path: &Path) -> Result<String, String> {
    let out = command(path).arg("--version").output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Asks `claude auth status` whether the user is signed in. The command reports only a yes or
/// no and the method; the app never reads credential files.
pub fn signed_in(path: &Path) -> Option<bool> {
    let mut cmd = command(path);
    cmd.args(["auth", "status"]);
    crate::engine::scrub_env(&mut cmd);
    let out = cmd.output().ok()?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    v.get("loggedIn").and_then(|b| b.as_bool())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_file_in_given_dirs() {
        let d = std::env::temp_dir().join(format!("mm-claude-path-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let empty = d.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert_eq!(find_in(&[empty.clone()]), None);
        let f = d.join(NAMES[0]);
        std::fs::write(&f, b"x").unwrap();
        assert_eq!(find_in(&[empty, d.clone()]), Some(f.clone()));
        assert_eq!(resolve(Some(f.to_str().unwrap())), Some(f));
        assert_eq!(resolve(Some("C:/definitely/not/here/claude.exe")), None);
    }

    #[test]
    fn cmd_shims_detected() {
        assert!(is_cmd_shim(Path::new("C:/x/claude.cmd")));
        assert!(!is_cmd_shim(Path::new("C:/x/claude.exe")));
    }
}
