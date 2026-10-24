//! The ChatGPT engine: OpenAI's official `codex app-server` on the user's own ChatGPT login.
//! It is the counterpart of the Claude engine: one warm process, one ephemeral conversation
//! (thread) per popup session, streamed words, and follow-ups in the same thread.
//!
//! Isolation: the rewrite prompt replaces Codex's own instructions, the thread is ephemeral (no
//! saved session), the sandbox is read-only, and tools, plugins, skills, memories, hooks,
//! AGENTS.md files, and the user's MCP servers are switched off. Any tool or approval request
//! that still arrives is declined.

use crate::engine::{classify, next_engine_id, Engine, EngineConfig, EngineError, EngineEvent, ErrorKind, Tagged};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

/// Feature flags to switch off. Only names this Codex version knows are passed.
pub const DISABLE_FEATURES: [&str; 20] = [
    "plugins", "remote_plugin", "skill_search", "apps", "hooks", "memories", "multi_agent", "shell_tool",
    "unified_exec", "tool_suggest", "image_generation", "browser_use", "computer_use", "goals", "view_image",
    "sleep_tool", "code_mode_host", "worktrees", "workspace_dependencies", "in_app_browser",
];

/// Settings that keep Codex's coding-agent context out of the conversation.
pub const CONFIG_OVERRIDES: [&str; 9] = [
    "project_doc_max_bytes=0",
    "include_permissions_instructions=false",
    "include_environment_context=false",
    "include_apps_instructions=false",
    "include_collaboration_mode_instructions=false",
    "skills.include_instructions=false",
    "check_for_update_on_startup=false",
    "analytics.enabled=false",
    "web_search=\"disabled\"",
];

/// The JSON shape Codex is held to for every turn.
pub fn output_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["variants", "subject"],
        "properties": {
            "variants": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["text"],
                    "properties": { "text": { "type": "string" } }
                }
            },
            "subject": { "type": ["string", "null"] }
        }
    })
}

/// Maps a Codex turn error to a category.
pub fn classify_codex(info: &Value, message: &str) -> ErrorKind {
    let m = message.to_lowercase();
    if let Some(s) = info.as_str() {
        match s {
            "usageLimitExceeded" | "rateLimitExceeded" | "sessionBudgetExceeded" => return ErrorKind::LimitReached,
            "unauthorized" => return ErrorKind::NotSignedIn,
            "serverOverloaded" | "internalServerError" => return ErrorKind::Other,
            _ => {}
        }
    }
    if let Some(o) = info.as_object() {
        if o.keys().any(|k| k.contains("Connection") || k.contains("Disconnected") || k.contains("Failed")) {
            return ErrorKind::Offline;
        }
    }
    if m.contains("model_not_found") || (m.contains("model") && m.contains("not supported")) {
        return ErrorKind::ModelUnavailable;
    }
    if m.contains("usage limit") || m.contains("429") {
        return ErrorKind::LimitReached;
    }
    if m.contains("401") || m.contains("unauthorized") || m.contains("codex login") {
        return ErrorKind::NotSignedIn;
    }
    classify(None, message)
}

/// The app-server conversation, as pure state. Lines in, events and lines to write out.
pub struct CodexState {
    model: String,
    instructions: String,
    cwd: String,
    thread_id: Option<String>,
    next_rpc: u64,
    queued: Vec<(u64, String)>,
    rpc_turn: HashMap<u64, u64>,
    turn_of: HashMap<String, u64>,
    acc: HashMap<String, String>,
    finals: HashMap<String, String>,
    current: Option<u64>,
    current_turn_id: Option<String>,
    start_error: Option<ErrorKind>,
}

const RPC_INITIALIZE: u64 = 1;
const RPC_THREAD_START: u64 = 2;

impl CodexState {
    pub fn new(model: &str, instructions: &str, cwd: &str) -> Self {
        CodexState {
            model: model.to_string(),
            instructions: instructions.to_string(),
            cwd: cwd.to_string(),
            thread_id: None,
            next_rpc: 10,
            queued: Vec::new(),
            rpc_turn: HashMap::new(),
            turn_of: HashMap::new(),
            acc: HashMap::new(),
            finals: HashMap::new(),
            current: None,
            current_turn_id: None,
            start_error: None,
        }
    }

    /// Initialize, then open one ephemeral thread with the rewrite prompt as its only instructions.
    pub fn handshake(&self) -> Vec<Value> {
        vec![
            json!({ "jsonrpc": "2.0", "id": RPC_INITIALIZE, "method": "initialize",
                    "params": { "clientInfo": { "name": "magic-mend", "version": env!("CARGO_PKG_VERSION") } } }),
            json!({ "jsonrpc": "2.0", "method": "initialized" }),
            json!({ "jsonrpc": "2.0", "id": RPC_THREAD_START, "method": "thread/start",
                    "params": {
                        "model": self.model,
                        "ephemeral": true,
                        "sandbox": "read-only",
                        "approvalPolicy": "never",
                        "baseInstructions": self.instructions,
                        "cwd": self.cwd,
                    } }),
        ]
    }

    pub fn thread_ready(&self) -> bool {
        self.thread_id.is_some()
    }

    /// A new user turn. A turn still running is interrupted, since only the newest reply matters.
    pub fn start_turn(&mut self, turn: u64, text: &str) -> (Vec<Value>, Vec<EngineEvent>) {
        if let Some(kind) = self.start_error {
            return (Vec::new(), vec![EngineEvent::TurnDone { turn, text: String::new(), error: Some(kind) }]);
        }
        let Some(thread) = self.thread_id.clone() else {
            self.queued.push((turn, text.to_string()));
            return (Vec::new(), Vec::new());
        };
        let mut out = Vec::new();
        if let Some(running) = self.current_turn_id.take() {
            out.push(json!({ "jsonrpc": "2.0", "id": self.bump(), "method": "turn/interrupt",
                             "params": { "threadId": thread, "turnId": running } }));
        }
        let id = self.bump();
        self.rpc_turn.insert(id, turn);
        self.current = Some(turn);
        out.push(json!({ "jsonrpc": "2.0", "id": id, "method": "turn/start",
                         "params": {
                             "threadId": thread,
                             "input": [{ "type": "text", "text": text }],
                             "outputSchema": output_schema(),
                             "effort": "low",
                         } }));
        (out, Vec::new())
    }

    fn bump(&mut self) -> u64 {
        self.next_rpc += 1;
        self.next_rpc
    }

    pub fn on_line(&mut self, line: &str) -> (Vec<Value>, Vec<EngineEvent>) {
        let mut writes = Vec::new();
        let mut events = Vec::new();
        let Ok(m) = serde_json::from_str::<Value>(line) else { return (writes, events) };
        let method = m.get("method").and_then(|x| x.as_str());
        let id = m.get("id").and_then(|x| x.as_u64());

        // A request from the server (tool approval, user input, and so on): always decline.
        if let (Some(_), Some(_)) = (method, m.get("id")) {
            writes.push(json!({ "jsonrpc": "2.0", "id": m["id"].clone(),
                                "error": { "code": -32601, "message": "Magic Mend does not allow tools or approvals." } }));
            return (writes, events);
        }

        // A response to one of our requests.
        if let (None, Some(id)) = (method, id) {
            if id == RPC_THREAD_START {
                if let Some(t) = m.pointer("/result/thread/id").and_then(|x| x.as_str()) {
                    self.thread_id = Some(t.to_string());
                    for (turn, text) in std::mem::take(&mut self.queued) {
                        let (w, e) = self.start_turn(turn, &text);
                        writes.extend(w);
                        events.extend(e);
                    }
                } else {
                    let msg = m.pointer("/error/message").and_then(|x| x.as_str()).unwrap_or("");
                    let kind = classify_codex(&Value::Null, msg);
                    self.start_error = Some(kind);
                    for (turn, _) in std::mem::take(&mut self.queued) {
                        events.push(EngineEvent::TurnDone { turn, text: String::new(), error: Some(kind) });
                    }
                }
            } else if let Some(turn) = self.rpc_turn.remove(&id) {
                if let Some(tid) = m.pointer("/result/turn/id").and_then(|x| x.as_str()) {
                    self.turn_of.insert(tid.to_string(), turn);
                    if self.current == Some(turn) {
                        self.current_turn_id = Some(tid.to_string());
                    }
                    // Words that arrived before this response.
                    if let Some(text) = self.acc.get(tid) {
                        events.push(EngineEvent::Delta { turn, text: text.clone() });
                    }
                } else if let Some(msg) = m.pointer("/error/message").and_then(|x| x.as_str()) {
                    events.push(EngineEvent::TurnDone { turn, text: String::new(), error: Some(classify_codex(&Value::Null, msg)) });
                }
            }
            return (writes, events);
        }

        let p = &m["params"];
        match method {
            Some("item/agentMessage/delta") => {
                let tid = p["turnId"].as_str().unwrap_or("").to_string();
                let acc = self.acc.entry(tid.clone()).or_default();
                acc.push_str(p["delta"].as_str().unwrap_or(""));
                if let Some(turn) = self.turn_of.get(&tid) {
                    events.push(EngineEvent::Delta { turn: *turn, text: acc.clone() });
                }
            }
            Some("item/completed") => {
                if p["item"]["type"] == "agentMessage" {
                    let tid = p["turnId"].as_str().unwrap_or("").to_string();
                    if let Some(text) = p["item"]["text"].as_str() {
                        self.finals.insert(tid, text.to_string());
                    }
                }
            }
            Some("turn/completed") => {
                let t = &p["turn"];
                let tid = t["id"].as_str().unwrap_or("").to_string();
                let Some(turn) = self.turn_of.get(&tid).copied() else { return (writes, events) };
                if self.current_turn_id.as_deref() == Some(tid.as_str()) {
                    self.current_turn_id = None;
                }
                let text = self
                    .finals
                    .remove(&tid)
                    .or_else(|| {
                        t["items"].as_array().and_then(|items| {
                            items.iter().rev().find(|i| i["type"] == "agentMessage").and_then(|i| i["text"].as_str().map(str::to_string))
                        })
                    })
                    .or_else(|| self.acc.get(&tid).cloned())
                    .unwrap_or_default();
                self.acc.remove(&tid);
                let error = match t["status"].as_str() {
                    Some("completed") => None,
                    Some("interrupted") => Some(ErrorKind::Other),
                    _ => {
                        let e = &t["error"];
                        Some(classify_codex(&e["codexErrorInfo"], e["message"].as_str().unwrap_or("")))
                    }
                };
                events.push(EngineEvent::TurnDone { turn, text, error });
            }
            _ => {}
        }
        (writes, events)
    }
}

pub struct CodexEngine {
    id: u64,
    cfg: EngineConfig,
    child: Option<Child>,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    state: Arc<Mutex<CodexState>>,
    tx: Sender<Tagged>,
    sent: u64,
    #[cfg(windows)]
    job: Option<crate::platform::windows::KillJob>,
}

fn write_all(stdin: &Arc<Mutex<Option<ChildStdin>>>, lines: &[Value]) -> bool {
    let Ok(mut guard) = stdin.lock() else { return false };
    let Some(w) = guard.as_mut() else { return false };
    for l in lines {
        let mut s = l.to_string();
        s.push('\n');
        if w.write_all(s.as_bytes()).is_err() {
            return false;
        }
    }
    w.flush().is_ok()
}

/// Codex flags that are known to this version, so a renamed feature never stops the engine.
fn known_features(program: &std::path::Path) -> Vec<String> {
    static CACHE: Mutex<Option<(std::path::PathBuf, Vec<String>)>> = Mutex::new(None);
    if let Ok(c) = CACHE.lock() {
        if let Some((p, v)) = c.as_ref() {
            if p == program {
                return v.clone();
            }
        }
    }
    let names: Vec<String> = crate::claude_path::command(program)
        .args(["features", "list"])
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.contains("removed"))
                .filter_map(|l| l.split_whitespace().next().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    if let Ok(mut c) = CACHE.lock() {
        *c = Some((program.to_path_buf(), names.clone()));
    }
    names
}

/// The full argument list for `codex app-server`.
pub fn args(program: &std::path::Path) -> Vec<String> {
    let known = known_features(program);
    let mut a = vec!["app-server".to_string()];
    for f in DISABLE_FEATURES {
        if known.is_empty() || known.iter().any(|k| k == f) {
            a.push("--disable".into());
            a.push(f.into());
        }
    }
    for c in CONFIG_OVERRIDES.iter().map(|s| s.to_string()).chain(crate::codex_path::mcp_overrides(program)) {
        a.push("-c".into());
        a.push(c);
    }
    a
}

impl CodexEngine {
    pub fn new(cfg: EngineConfig, tx: Sender<Tagged>) -> Self {
        let instructions = std::fs::read_to_string(&cfg.system_prompt_file).unwrap_or_else(|_| crate::SYSTEM_PROMPT.to_string());
        let state = CodexState::new(&cfg.model, &instructions, &cfg.workdir.to_string_lossy());
        CodexEngine {
            id: next_engine_id(),
            cfg,
            child: None,
            stdin: Arc::new(Mutex::new(None)),
            state: Arc::new(Mutex::new(state)),
            tx,
            sent: 0,
            #[cfg(windows)]
            job: None,
        }
    }
}

impl Engine for CodexEngine {
    fn start(&mut self) -> Result<(), EngineError> {
        if !self.cfg.program.is_file() {
            return Err(EngineError(ErrorKind::NotFound));
        }
        std::fs::create_dir_all(&self.cfg.workdir).map_err(|_| EngineError(ErrorKind::Other))?;
        let mut cmd = crate::claude_path::command(&self.cfg.program);
        cmd.args(args(&self.cfg.program))
            .current_dir(&self.cfg.workdir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = cmd.spawn().map_err(|e| {
            EngineError(if e.kind() == std::io::ErrorKind::NotFound { ErrorKind::NotFound } else { ErrorKind::ProcessExit })
        })?;
        #[cfg(windows)]
        {
            self.job = crate::platform::windows::KillJob::attach(&child);
        }
        let stdout = child.stdout.take().expect("stdout piped");
        *self.stdin.lock().unwrap() = child.stdin.take();
        self.child = Some(child);

        let handshake = self.state.lock().unwrap().handshake();
        if !write_all(&self.stdin, &handshake) {
            return Err(EngineError(ErrorKind::ProcessExit));
        }

        let (tx, id, state, stdin) = (self.tx.clone(), self.id, self.state.clone(), self.stdin.clone());
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                let (writes, events) = state.lock().unwrap().on_line(&line);
                if !writes.is_empty() {
                    write_all(&stdin, &writes);
                }
                for event in events {
                    let _ = tx.send(Tagged { engine: id, event });
                }
            }
            let _ = tx.send(Tagged { engine: id, event: EngineEvent::Exited { error: ErrorKind::ProcessExit } });
        });
        Ok(())
    }

    fn send(&mut self, text: &str) -> Result<u64, EngineError> {
        self.sent += 1;
        let turn = self.sent;
        let (writes, events) = self.state.lock().unwrap().start_turn(turn, text);
        if !writes.is_empty() && !write_all(&self.stdin, &writes) {
            return Err(EngineError(ErrorKind::ProcessExit));
        }
        for event in events {
            let _ = self.tx.send(Tagged { engine: self.id, event });
        }
        Ok(turn)
    }

    fn stop(&mut self) {
        *self.stdin.lock().unwrap() = None;
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        #[cfg(windows)]
        {
            self.job = None;
        }
    }

    fn id(&self) -> u64 {
        self.id
    }

    fn pid(&self) -> Option<u32> {
        self.child.as_ref().map(|c| c.id())
    }

    fn is_running(&mut self) -> bool {
        match self.child.as_mut() {
            Some(c) => matches!(c.try_wait(), Ok(None)),
            None => false,
        }
    }
}

impl Drop for CodexEngine {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(v: Value) -> String {
        v.to_string()
    }

    fn ready() -> CodexState {
        let mut s = CodexState::new("gpt-5.6-luna", "PROMPT", "C:/engine");
        let hs = s.handshake();
        assert_eq!(hs[2]["params"]["ephemeral"], true);
        assert_eq!(hs[2]["params"]["baseInstructions"], "PROMPT");
        assert_eq!(hs[2]["params"]["sandbox"], "read-only");
        s.on_line(&line(json!({"id": 1, "result": {}})));
        s
    }

    #[test]
    fn queues_until_thread_is_ready_then_streams_and_completes() {
        let mut s = ready();
        let (w, e) = s.start_turn(1, "{\"text\":\"hi\"}");
        assert!(w.is_empty() && e.is_empty(), "queued before the thread exists");
        let (w, _) = s.on_line(&line(json!({"id": 2, "result": {"thread": {"id": "th1"}}})));
        assert_eq!(w.len(), 1);
        assert_eq!(w[0]["method"], "turn/start");
        assert_eq!(w[0]["params"]["effort"], "low");
        assert_eq!(w[0]["params"]["outputSchema"]["required"][0], "variants");
        let rpc = w[0]["id"].as_u64().unwrap();
        // A word may arrive before the turn/start response.
        let (_, e) = s.on_line(&line(json!({"method": "item/agentMessage/delta", "params": {"threadId": "th1", "turnId": "tu1", "itemId": "m", "delta": "{\"vari"}})));
        assert!(e.is_empty());
        let (_, e) = s.on_line(&line(json!({"id": rpc, "result": {"turn": {"id": "tu1"}}})));
        assert_eq!(e, vec![EngineEvent::Delta { turn: 1, text: "{\"vari".into() }]);
        let (_, e) = s.on_line(&line(json!({"method": "item/agentMessage/delta", "params": {"threadId": "th1", "turnId": "tu1", "itemId": "m", "delta": "ants\""}})));
        assert_eq!(e, vec![EngineEvent::Delta { turn: 1, text: "{\"variants\"".into() }]);
        s.on_line(&line(json!({"method": "item/completed", "params": {"turnId": "tu1", "item": {"type": "agentMessage", "text": "FINAL"}}})));
        let (_, e) = s.on_line(&line(json!({"method": "turn/completed", "params": {"threadId": "th1", "turn": {"id": "tu1", "status": "completed", "items": []}}})));
        assert_eq!(e, vec![EngineEvent::TurnDone { turn: 1, text: "FINAL".into(), error: None }]);
    }

    #[test]
    fn a_new_turn_interrupts_the_running_one() {
        let mut s = ready();
        s.on_line(&line(json!({"id": 2, "result": {"thread": {"id": "th1"}}})));
        let (w, _) = s.start_turn(1, "a");
        s.on_line(&line(json!({"id": w[0]["id"], "result": {"turn": {"id": "tu1"}}})));
        let (w, _) = s.start_turn(2, "b");
        assert_eq!(w[0]["method"], "turn/interrupt");
        assert_eq!(w[0]["params"]["turnId"], "tu1");
        assert_eq!(w[1]["method"], "turn/start");
        let (_, e) = s.on_line(&line(json!({"method": "turn/completed", "params": {"turn": {"id": "tu1", "status": "interrupted"}}})));
        assert!(matches!(e[0], EngineEvent::TurnDone { turn: 1, error: Some(_), .. }));
    }

    #[test]
    fn server_requests_are_declined() {
        let mut s = ready();
        let (w, e) = s.on_line(&line(json!({"id": 99, "method": "item/commandExecution/requestApproval", "params": {}})));
        assert!(e.is_empty());
        assert_eq!(w[0]["id"], 99);
        assert!(w[0]["error"].is_object());
    }

    #[test]
    fn failures_are_classified() {
        let mut s = ready();
        s.on_line(&line(json!({"id": 2, "result": {"thread": {"id": "th1"}}})));
        let (w, _) = s.start_turn(1, "a");
        s.on_line(&line(json!({"id": w[0]["id"], "result": {"turn": {"id": "tu1"}}})));
        let (_, e) = s.on_line(&line(json!({"method": "turn/completed", "params": {"turn": {"id": "tu1", "status": "failed",
            "error": {"message": "limit", "codexErrorInfo": "usageLimitExceeded"}}}})));
        assert_eq!(e, vec![EngineEvent::TurnDone { turn: 1, text: String::new(), error: Some(ErrorKind::LimitReached) }]);
        assert_eq!(classify_codex(&json!("unauthorized"), ""), ErrorKind::NotSignedIn);
        assert_eq!(classify_codex(&json!({"httpConnectionFailed": {"httpStatusCode": null}}), ""), ErrorKind::Offline);
        assert_eq!(
            classify_codex(&Value::Null, "The 'gpt-5.1-codex-max' model is not supported when using Codex with a ChatGPT account."),
            ErrorKind::ModelUnavailable
        );
    }

    #[test]
    fn thread_start_failure_fails_queued_turns() {
        let mut s = ready();
        s.start_turn(1, "a");
        let (_, e) = s.on_line(&line(json!({"id": 2, "error": {"code": -32600, "message": "failed to load configuration"}})));
        assert!(matches!(e[0], EngineEvent::TurnDone { turn: 1, error: Some(_), .. }));
        let (_, e) = s.start_turn(2, "b");
        assert!(matches!(e[0], EngineEvent::TurnDone { turn: 2, error: Some(_), .. }));
    }
}
