//! A stand-in for the `claude` command, used only by automated tests so they run without a
//! Claude login. It speaks the same stream-json protocol. Not shipped.
//!
//! MM_FAKE_MODE: normal (default), signed_out, limit, offline, hang, crash, contraction, slow
//! MM_FAKE_MODE_FILE: a file holding the mode, read at each start (wins over MM_FAKE_MODE)
//! MM_FAKE_LOG:  a file that receives one JSON line per argument list and per message

use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::time::Duration;

fn log(v: Value) {
    if let Ok(path) = std::env::var("MM_FAKE_LOG") {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{v}");
        }
    }
}

fn out(v: Value) {
    let mut o = std::io::stdout().lock();
    let _ = writeln!(o, "{v}");
    let _ = o.flush();
}

fn reply_text(user: &str, mode: &str, fixed: &mut bool) -> String {
    if user == "Reply with ok" {
        return "ok".into();
    }
    if user.starts_with("Your last reply broke the rules") {
        *fixed = true;
        return json!({"variants": [{"text": "I do not know yet, I will tell you soon."}, {"text": "I am not sure yet, I will let you know."}]}).to_string();
    }
    if user.starts_with("Your last reply was not valid JSON") {
        return json!({"variants": [{"text": "Fixed draft."}]}).to_string();
    }
    let Ok(req) = serde_json::from_str::<Value>(user) else {
        return "Hello there.".into();
    };
    let text = req["text"].as_str().unwrap_or("").trim().to_string();
    let tone = req["tone"].as_str().unwrap_or("Clear");
    let channel = req["channel"].as_str().unwrap_or("Text");
    let toggles: Vec<String> = req["toggles"].as_array().map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect()).unwrap_or_default();
    let followup = req["followup"].as_str();
    if mode == "contraction" && !*fixed {
        return json!({"variants": [{"text": "I don't know yet \u{2014} I'll tell you soon."}, {"text": "I am not sure yet, I will let you know."}]}).to_string();
    }
    if mode == "badjson" {
        return "Sure! Here are your drafts: A and B".into();
    }
    let mut base = text.clone();
    if let Some(f) = followup {
        base = format!("{base} (after: {f})");
    }
    if text.contains("MULTILINE") {
        base = "First line of the draft.\nSecond line of the draft.\n\nLast line.\n".into();
    }
    let n = if toggles.iter().any(|t| t == "Proofread") { 1 } else { req["drafts"].as_u64().unwrap_or(2) as usize };
    let variants: Vec<Value> = (0..n)
        .map(|i| json!({"text": format!("[{tone}] {} {}", ["Draft A:", "Draft B:", "Draft C:", "Draft D:"][i], base)}))
        .collect();
    let mut r = json!({ "variants": variants });
    if channel == "Email" {
        r["subject"] = json!(format!("[{tone}] Subject"));
    }
    r.to_string()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // A mode file lets one test switch modes while the app keeps running.
    let mode = std::env::var("MM_FAKE_MODE_FILE")
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("MM_FAKE_MODE").ok())
        .unwrap_or_else(|| "normal".into());
    if args.first().map(String::as_str) == Some("--version") {
        println!("9.9.9 (Fake Claude)");
        return;
    }
    if args.first().map(String::as_str) == Some("auth") {
        println!("{}", json!({"loggedIn": mode != "signed_out", "authMethod": if mode == "signed_out" { "none" } else { "claude.ai" }}));
        return;
    }
    log(json!({"argv": args, "cwd": std::env::current_dir().ok().map(|p| p.display().to_string())}));
    if mode == "crash" {
        eprintln!("fake crash");
        std::process::exit(3);
    }
    let session = "00000000-0000-0000-0000-000000000000";
    let mut fixed = false;
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
        let user = v["message"]["content"][0]["text"].as_str().unwrap_or("").to_string();
        log(json!({"message": user}));
        out(json!({"type": "system", "subtype": "init", "session_id": session, "tools": [], "mcp_servers": []}));
        match mode.as_str() {
            "hang" => continue,
            "signed_out" | "limit" | "offline" => {
                let (code, text) = match mode.as_str() {
                    "signed_out" => ("authentication_failed", "Failed to authenticate: OAuth session expired and could not be refreshed"),
                    "limit" => ("rate_limit", "Claude AI usage limit reached|1900000000"),
                    _ => ("unknown", "API Error: Connection error."),
                };
                out(json!({"type": "assistant", "error": code, "message": {"content": [{"type": "text", "text": text}]}}));
                out(json!({"type": "result", "subtype": "success", "is_error": true, "result": text}));
                continue;
            }
            _ => {}
        }
        let text = reply_text(&user, &mode, &mut fixed);
        let delay = if mode == "slow" { 120 } else { 12 };
        let chars: Vec<char> = text.chars().collect();
        for chunk in chars.chunks(6) {
            let piece: String = chunk.iter().collect();
            out(json!({"type": "stream_event", "event": {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": piece}}}));
            std::thread::sleep(Duration::from_millis(delay));
        }
        out(json!({"type": "assistant", "message": {"content": [{"type": "text", "text": text}]}}));
        out(json!({"type": "result", "subtype": "success", "is_error": false, "result": text}));
    }
}
