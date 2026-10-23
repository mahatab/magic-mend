//! The Claude engine: one official `claude` process in print mode with stream-json input and
//! output, kept warm so the hotkey does not pay the startup delay (spec, "Claude engine").
//!
//! The engine never reads credentials. It only starts the user's own signed-in command.

use serde::Serialize;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::claude_path;
use crate::protocol::user_message_line;

pub const PRIMING_TEXT: &str = "Reply with ok";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    NotFound,
    NotSignedIn,
    LimitReached,
    Offline,
    ProcessExit,
    Timeout,
    BadReply,
    Other,
}

impl ErrorKind {
    /// A plain message that names the fix. Never a stack trace.
    pub fn message(self) -> &'static str {
        match self {
            ErrorKind::NotFound => "Magic Mend cannot find the claude command. Install Claude Code, or set the path in Settings.",
            ErrorKind::NotSignedIn => "Claude is not signed in. Open a terminal, run claude, sign in, and then click Retry.",
            ErrorKind::LimitReached => "Your Claude plan limit has been reached. Try again when your limit resets.",
            ErrorKind::Offline => "Claude cannot reach the internet. Check your connection and click Retry.",
            ErrorKind::ProcessExit => "The claude process stopped unexpectedly. Click Retry.",
            ErrorKind::Timeout => "Claude took longer than 30 seconds. Click Retry.",
            ErrorKind::BadReply => "Claude sent a reply Magic Mend could not read. Click Retry.",
            ErrorKind::Other => "Something went wrong with Claude. Click Retry.",
        }
    }

    pub fn category(self) -> &'static str {
        match self {
            ErrorKind::NotFound => "not_found",
            ErrorKind::NotSignedIn => "not_signed_in",
            ErrorKind::LimitReached => "limit_reached",
            ErrorKind::Offline => "offline",
            ErrorKind::ProcessExit => "process_exit",
            ErrorKind::Timeout => "timeout",
            ErrorKind::BadReply => "bad_reply",
            ErrorKind::Other => "other",
        }
    }
}

/// Maps the CLI's error code and text to a category.
pub fn classify(error_code: Option<&str>, text: &str) -> ErrorKind {
    let t = text.to_lowercase();
    match error_code {
        Some("authentication_failed") | Some("oauth_org_not_allowed") => return ErrorKind::NotSignedIn,
        Some("rate_limit") | Some("billing_error") => return ErrorKind::LimitReached,
        _ => {}
    }
    if t.contains("authenticate") || t.contains("/login") || t.contains("not logged in")
        || t.contains("invalid api key") || t.contains("please run claude") || t.contains("log in")
    {
        ErrorKind::NotSignedIn
    } else if t.contains("usage limit") || t.contains("limit reached") || t.contains("hit your limit")
        || t.contains("rate limit") || t.contains("out of extra usage") || t.contains("credit balance")
    {
        ErrorKind::LimitReached
    } else if t.contains("enotfound") || t.contains("econnrefused") || t.contains("econnreset")
        || t.contains("etimedout") || t.contains("connection error") || t.contains("network")
        || t.contains("fetch failed") || t.contains("offline") || t.contains("getaddrinfo")
        || t.contains("unable to connect")
    {
        ErrorKind::Offline
    } else {
        ErrorKind::Other
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum EngineEvent {
    /// The spare finished its priming exchange (only sent when priming is on).
    Primed { ms: u128 },
    /// Text of the current turn so far.
    Delta { turn: u64, text: String },
    /// A turn finished.
    TurnDone { turn: u64, text: String, error: Option<ErrorKind> },
    /// The process ended.
    Exited { error: ErrorKind },
}

#[derive(Debug, Clone)]
pub struct Tagged {
    pub engine: u64,
    pub event: EngineEvent,
}

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub claude_path: PathBuf,
    pub model: String,
    pub system_prompt_file: PathBuf,
    /// A dedicated empty folder, so no project settings load.
    pub workdir: PathBuf,
    pub priming: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EngineError(pub ErrorKind);

/// The small engine interface, so a second backend can be added later.
pub trait Engine: Send {
    fn start(&mut self) -> Result<(), EngineError>;
    /// Sends one user turn and returns its turn number (1, 2, 3...).
    fn send(&mut self, text: &str) -> Result<u64, EngineError>;
    fn stop(&mut self);
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub struct ClaudeEngine {
    pub id: u64,
    cfg: EngineConfig,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    sent: u64,
    tx: Sender<Tagged>,
    pub spawned_at: Option<Instant>,
    #[cfg(windows)]
    job: Option<crate::platform::windows::KillJob>,
}

/// The flags, verified against `claude --help` for version 2.1.296. `--system-prompt-file`
/// and `--safe-mode` keep the personal setup (CLAUDE.md, hooks, plugins, output styles, MCP)
/// from loading.
pub fn args(cfg: &EngineConfig) -> Vec<String> {
    let mut a: Vec<String> = [
        "-p",
        "--input-format", "stream-json",
        "--output-format", "stream-json",
        "--verbose",
        "--include-partial-messages",
        "--model",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    a.push(cfg.model.clone());
    a.push("--system-prompt-file".into());
    a.push(cfg.system_prompt_file.to_string_lossy().into_owned());
    for s in [
        "--tools", "",
        "--strict-mcp-config",
        "--disable-slash-commands",
        "--no-session-persistence",
        "--safe-mode",
        "--setting-sources", "",
    ] {
        a.push(s.to_string());
    }
    a
}

/// Removes variables a parent Claude Code session may have set, so the child runs exactly as
/// the user's own standalone command would. Then turns off auto-update and extra traffic.
pub fn scrub_env(cmd: &mut std::process::Command) {
    for (k, _) in std::env::vars_os() {
        let k = k.to_string_lossy().to_string();
        if k == "CLAUDECODE"
            || k.starts_with("CLAUDE_CODE_")
            || k.starts_with("CLAUDE_AGENT_SDK")
            || k == "CLAUDE_PID"
            || k == "CLAUDE_EFFORT"
            || k == "CLAUDE_PREVIEW_CLASSIFIER_FLOOR"
        {
            cmd.env_remove(&k);
        }
    }
    cmd.env("DISABLE_AUTOUPDATER", "1");
    cmd.env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1");
}

impl ClaudeEngine {
    pub fn new(cfg: EngineConfig, tx: Sender<Tagged>) -> Self {
        ClaudeEngine {
            id: NEXT_ID.fetch_add(1, Ordering::SeqCst),
            cfg,
            child: None,
            stdin: None,
            sent: 0,
            tx,
            spawned_at: None,
            #[cfg(windows)]
            job: None,
        }
    }

    pub fn is_running(&mut self) -> bool {
        match self.child.as_mut() {
            Some(c) => matches!(c.try_wait(), Ok(None)),
            None => false,
        }
    }

    pub fn turns_sent(&self) -> u64 {
        self.sent
    }

    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().map(|c| c.id())
    }
}

impl Engine for ClaudeEngine {
    fn start(&mut self) -> Result<(), EngineError> {
        if !self.cfg.claude_path.is_file() {
            return Err(EngineError(ErrorKind::NotFound));
        }
        std::fs::create_dir_all(&self.cfg.workdir).map_err(|_| EngineError(ErrorKind::Other))?;
        let mut cmd = claude_path::command(&self.cfg.claude_path);
        cmd.args(args(&self.cfg))
            .current_dir(&self.cfg.workdir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        scrub_env(&mut cmd);
        let mut child = cmd.spawn().map_err(|e| {
            EngineError(if e.kind() == std::io::ErrorKind::NotFound { ErrorKind::NotFound } else { ErrorKind::ProcessExit })
        })?;
        self.spawned_at = Some(Instant::now());
        #[cfg(windows)]
        {
            self.job = crate::platform::windows::KillJob::attach(&child);
        }
        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");
        self.stdin = child.stdin.take();
        self.child = Some(child);

        let stderr_tail = Arc::new(Mutex::new(String::new()));
        {
            let tail = stderr_tail.clone();
            std::thread::spawn(move || {
                let mut r = BufReader::new(stderr);
                let mut buf = [0u8; 4096];
                while let Ok(n) = r.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let mut t = tail.lock().unwrap();
                    t.push_str(&String::from_utf8_lossy(&buf[..n]));
                    let len = t.len();
                    if len > 4000 {
                        let cut = t.char_indices().map(|(i, _)| i).find(|i| *i >= len - 4000).unwrap_or(0);
                        t.drain(..cut);
                    }
                }
            });
        }

        let tx = self.tx.clone();
        let id = self.id;
        let priming = self.cfg.priming;
        let spawned = Instant::now();
        std::thread::spawn(move || {
            let mut reader = ReplyReader::new(priming);
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                for event in reader.feed(&line, spawned) {
                    let _ = tx.send(Tagged { engine: id, event });
                }
            }
            let tail = stderr_tail.lock().unwrap().clone();
            let error = match classify(None, &tail) {
                ErrorKind::Other => ErrorKind::ProcessExit,
                k => k,
            };
            let _ = tx.send(Tagged { engine: id, event: EngineEvent::Exited { error } });
        });

        if self.cfg.priming {
            self.write_line(PRIMING_TEXT)?;
        }
        Ok(())
    }

    fn send(&mut self, text: &str) -> Result<u64, EngineError> {
        self.write_line(text)?;
        self.sent += 1;
        Ok(self.sent)
    }

    fn stop(&mut self) {
        self.stdin = None;
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        #[cfg(windows)]
        {
            self.job = None;
        }
    }
}

impl ClaudeEngine {
    fn write_line(&mut self, text: &str) -> Result<(), EngineError> {
        let stdin = self.stdin.as_mut().ok_or(EngineError(ErrorKind::ProcessExit))?;
        let mut line = user_message_line(text);
        line.push('\n');
        stdin
            .write_all(line.as_bytes())
            .and_then(|_| stdin.flush())
            .map_err(|_| EngineError(ErrorKind::ProcessExit))
    }
}

impl Drop for ClaudeEngine {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Turns stream-json output lines into engine events. Separate from the process so it can be
/// tested with recorded lines.
pub struct ReplyReader {
    priming_pending: bool,
    priming_used: bool,
    results_seen: u64,
    acc: String,
    error_code: Option<String>,
}

impl ReplyReader {
    pub fn new(priming: bool) -> Self {
        ReplyReader { priming_pending: priming, priming_used: false, results_seen: 0, acc: String::new(), error_code: None }
    }

    fn user_turn(&self) -> u64 {
        // The priming exchange is turn zero and never reaches the caller.
        self.results_seen + 1 - u64::from(self.priming_used)
    }

    pub fn feed(&mut self, line: &str, spawned: Instant) -> Vec<EngineEvent> {
        let mut out = Vec::new();
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { return out };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("stream_event") => {
                let ev = &v["event"];
                if ev["type"] == "content_block_delta" && ev["delta"]["type"] == "text_delta" {
                    if let Some(t) = ev["delta"]["text"].as_str() {
                        self.acc.push_str(t);
                        if !self.priming_pending {
                            out.push(EngineEvent::Delta { turn: self.user_turn(), text: self.acc.clone() });
                        }
                    }
                }
            }
            Some("assistant") => {
                if let Some(code) = v.get("error").and_then(|e| e.as_str()) {
                    self.error_code = Some(code.to_string());
                }
                // When partial messages are missing, the full message still carries the text.
                if self.acc.is_empty() {
                    if let Some(parts) = v["message"]["content"].as_array() {
                        for p in parts {
                            if p["type"] == "text" {
                                self.acc.push_str(p["text"].as_str().unwrap_or(""));
                            }
                        }
                    }
                }
            }
            Some("result") => {
                let is_error = v["is_error"].as_bool().unwrap_or(false) || self.error_code.is_some();
                let text = v["result"].as_str().map(str::to_string).unwrap_or_else(|| self.acc.clone());
                let error = is_error.then(|| classify(self.error_code.as_deref(), &text));
                if self.priming_pending {
                    self.priming_pending = false;
                    self.priming_used = true;
                    self.results_seen += 1;
                    match error {
                        None => out.push(EngineEvent::Primed { ms: spawned.elapsed().as_millis() }),
                        Some(e) => out.push(EngineEvent::TurnDone { turn: 0, text: String::new(), error: Some(e) }),
                    }
                } else {
                    let turn = self.user_turn();
                    self.results_seen += 1;
                    out.push(EngineEvent::TurnDone { turn, text, error });
                }
                self.acc.clear();
                self.error_code = None;
            }
            _ => {}
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn delta(t: &str) -> String {
        serde_json::json!({"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":t}}}).to_string()
    }
    fn result(t: &str, is_error: bool) -> String {
        serde_json::json!({"type":"result","subtype":"success","is_error":is_error,"result":t}).to_string()
    }

    #[test]
    fn reads_turns_and_deltas() {
        let now = Instant::now();
        let mut r = ReplyReader::new(false);
        assert!(r.feed(r#"{"type":"system","subtype":"init"}"#, now).is_empty());
        assert_eq!(r.feed(&delta("Hel"), now), vec![EngineEvent::Delta { turn: 1, text: "Hel".into() }]);
        assert_eq!(r.feed(&delta("lo"), now), vec![EngineEvent::Delta { turn: 1, text: "Hello".into() }]);
        assert_eq!(r.feed(&result("Hello", false), now), vec![EngineEvent::TurnDone { turn: 1, text: "Hello".into(), error: None }]);
        assert_eq!(r.feed(&delta("B"), now), vec![EngineEvent::Delta { turn: 2, text: "B".into() }]);
        assert_eq!(r.feed(&result("B", false), now)[0], EngineEvent::TurnDone { turn: 2, text: "B".into(), error: None });
        assert!(r.feed("not json", now).is_empty());
    }

    #[test]
    fn priming_turn_is_hidden() {
        let now = Instant::now();
        let mut r = ReplyReader::new(true);
        assert!(r.feed(&delta("ok"), now).is_empty());
        assert!(matches!(r.feed(&result("ok", false), now)[0], EngineEvent::Primed { .. }));
        assert_eq!(r.feed(&delta("X"), now), vec![EngineEvent::Delta { turn: 1, text: "X".into() }]);
        assert_eq!(r.feed(&result("X", false), now)[0], EngineEvent::TurnDone { turn: 1, text: "X".into(), error: None });
    }

    #[test]
    fn auth_error_from_real_output() {
        let now = Instant::now();
        let mut r = ReplyReader::new(false);
        r.feed(r#"{"type":"assistant","error":"authentication_failed","is_api_error_message":true,"message":{"content":[{"type":"text","text":"Failed to authenticate: OAuth session expired and could not be refreshed"}]}}"#, now);
        let ev = r.feed(r#"{"type":"result","subtype":"success","is_error":true,"result":"Failed to authenticate: OAuth session expired and could not be refreshed"}"#, now);
        assert_eq!(ev[0], EngineEvent::TurnDone { turn: 1, text: "Failed to authenticate: OAuth session expired and could not be refreshed".into(), error: Some(ErrorKind::NotSignedIn) });
    }

    #[test]
    fn classifies_errors() {
        assert_eq!(classify(Some("rate_limit"), ""), ErrorKind::LimitReached);
        assert_eq!(classify(None, "Claude AI usage limit reached|1700000000"), ErrorKind::LimitReached);
        assert_eq!(classify(None, "You've hit your limit · resets 5pm"), ErrorKind::LimitReached);
        assert_eq!(classify(None, "Invalid API key · Please run /login"), ErrorKind::NotSignedIn);
        assert_eq!(classify(None, "API Error: Connection error."), ErrorKind::Offline);
        assert_eq!(classify(None, "getaddrinfo ENOTFOUND api.anthropic.com"), ErrorKind::Offline);
        assert_eq!(classify(None, "weird"), ErrorKind::Other);
    }

    #[test]
    fn args_disable_tools_and_personal_setup() {
        let cfg = EngineConfig {
            claude_path: "claude".into(),
            model: "haiku".into(),
            system_prompt_file: "sys.txt".into(),
            workdir: ".".into(),
            priming: false,
        };
        let a = args(&cfg);
        let pos = a.iter().position(|x| x == "--tools").unwrap();
        assert_eq!(a[pos + 1], "");
        for f in ["--strict-mcp-config", "--disable-slash-commands", "--no-session-persistence", "--safe-mode", "--include-partial-messages"] {
            assert!(a.contains(&f.to_string()), "{f}");
        }
        assert!(!a.iter().any(|x| x.contains("dangerously")));
    }

    #[test]
    fn error_messages_follow_output_rules() {
        let rules = crate::checker::Rules::default();
        for k in [ErrorKind::NotFound, ErrorKind::NotSignedIn, ErrorKind::LimitReached, ErrorKind::Offline, ErrorKind::ProcessExit, ErrorKind::Timeout, ErrorKind::BadReply, ErrorKind::Other] {
            assert!(crate::checker::check(k.message(), rules).is_clean(), "{:?}", k);
        }
    }
}
