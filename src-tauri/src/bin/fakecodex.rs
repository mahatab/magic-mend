//! A stand-in for OpenAI's `codex` command, used only by automated tests so they run without a
//! ChatGPT login. It speaks the subset of the app-server protocol Magic Mend uses. Not shipped.
//!
//! MM_FAKE_MODE (or a file named by MM_FAKE_MODE_FILE): normal, signed_out, limit, offline,
//! hang, crash, contraction, badmodel
//! MM_FAKE_LOG: a file that receives one JSON line per argument list and per user message

use serde_json::{json, Value};
use std::io::{BufRead, Write};
use std::time::Duration;

fn mode() -> String {
    std::env::var("MM_FAKE_MODE_FILE")
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("MM_FAKE_MODE").ok())
        .unwrap_or_else(|| "normal".into())
}

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

/// Draft text that names the provider, so tests can tell which engine answered.
fn reply_text(user: &str, mode: &str, fixed: &mut bool) -> String {
    if user.starts_with("Your last reply broke the rules") {
        *fixed = true;
        return json!({"variants": [{"text": "I do not know yet, I will tell you soon."}], "subject": null}).to_string();
    }
    let Ok(req) = serde_json::from_str::<Value>(user) else {
        return json!({"variants": [{"text": "Hello there."}], "subject": null}).to_string();
    };
    if mode == "contraction" && !*fixed {
        return json!({"variants": [{"text": "I don't know yet \u{2014} I'll tell you soon."}], "subject": null}).to_string();
    }
    let text = req["text"].as_str().unwrap_or("").trim();
    let tone = req["tone"].as_str().unwrap_or("Clear");
    let channel = req["channel"].as_str().unwrap_or("Text");
    let fix = req["toggles"].as_array().map(|a| a.iter().any(|t| t == "Proofread")).unwrap_or(false);
    let mut base = text.to_string();
    if let Some(f) = req["followup"].as_str() {
        base = format!("{base} (after: {f})");
    }
    let n = if fix { 1 } else { req["drafts"].as_u64().unwrap_or(2) as usize };
    let variants: Vec<Value> = (0..n).map(|i| json!({"text": format!("[ChatGPT {tone}] {} {base}", ["Draft A:", "Draft B:", "Draft C:", "Draft D:"][i])})).collect();
    let subject = if channel == "Email" { json!(format!("[ChatGPT {tone}] Subject")) } else { Value::Null };
    json!({ "variants": variants, "subject": subject }).to_string()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = mode();
    match args.first().map(String::as_str) {
        Some("--version") => {
            println!("codex-cli 9.9.9");
            return;
        }
        Some("login") => {
            if mode == "signed_out" {
                println!("Not logged in");
                std::process::exit(1);
            }
            println!("Logged in using ChatGPT");
            return;
        }
        Some("mcp") => {
            println!("[]");
            return;
        }
        Some("features") => {
            for f in ["plugins", "apps", "hooks", "memories", "shell_tool", "unified_exec", "skill_search"] {
                println!("{f:<40} stable             true");
            }
            return;
        }
        Some("debug") => {
            println!("{}", json!({"models": [{"slug": "gpt-5.6-luna", "display_name": "GPT-5.6-Luna", "visibility": "list"}, {"slug": "gpt-6.1-sol", "display_name": "GPT-6.1-Sol", "visibility": "list"}]}));
            return;
        }
        Some("app-server") => {}
        _ => {
            eprintln!("fake codex: unsupported command");
            std::process::exit(2);
        }
    }
    log(json!({"argv": args, "cwd": std::env::current_dir().ok().map(|p| p.display().to_string())}));
    if mode == "crash" {
        std::process::exit(3);
    }
    let mut fixed = false;
    let mut turn_no = 0u64;
    let mut model = String::new();
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let Ok(m) = serde_json::from_str::<Value>(&line) else { continue };
        let id = m.get("id").cloned();
        match m["method"].as_str() {
            Some("initialize") => out(json!({"id": id, "result": {"userAgent": "fake"}})),
            Some("thread/start") => {
                model = m["params"]["model"].as_str().unwrap_or("").to_string();
                log(json!({"thread_start": {"ephemeral": m["params"]["ephemeral"], "sandbox": m["params"]["sandbox"], "model": model,
                           "instructions_len": m["params"]["baseInstructions"].as_str().map(|s| s.len()).unwrap_or(0)}}));
                out(json!({"id": id, "result": {"thread": {"id": "fake-thread"}}}));
                out(json!({"method": "thread/started", "params": {"thread": {"id": "fake-thread"}}}));
            }
            Some("turn/interrupt") => out(json!({"id": id, "result": {}})),
            Some("turn/start") => {
                turn_no += 1;
                let tid = format!("fake-turn-{turn_no}");
                let user = m["params"]["input"][0]["text"].as_str().unwrap_or("").to_string();
                log(json!({"message": user, "effort": m["params"]["effort"], "schema": m["params"]["outputSchema"].is_object()}));
                out(json!({"id": id, "result": {"turn": {"id": tid, "status": "inProgress"}}}));
                if mode == "hang" {
                    continue;
                }
                let fail = |info: Value, msg: &str| {
                    out(json!({"method": "turn/completed", "params": {"threadId": "fake-thread",
                        "turn": {"id": tid, "status": "failed", "error": {"message": msg, "codexErrorInfo": info}}}}));
                };
                match mode.as_str() {
                    "signed_out" => {
                        fail(json!("unauthorized"), "401 Unauthorized");
                        continue;
                    }
                    "limit" => {
                        fail(json!("usageLimitExceeded"), "You've hit your usage limit.");
                        continue;
                    }
                    "offline" => {
                        fail(json!({"httpConnectionFailed": {"httpStatusCode": null}}), "stream error");
                        continue;
                    }
                    "badmodel" => {
                        fail(Value::Null, &format!("The '{model}' model is not supported when using Codex with a ChatGPT account."));
                        continue;
                    }
                    _ => {}
                }
                let text = reply_text(&user, &mode, &mut fixed);
                let chars: Vec<char> = text.chars().collect();
                for chunk in chars.chunks(6) {
                    let piece: String = chunk.iter().collect();
                    out(json!({"method": "item/agentMessage/delta", "params": {"threadId": "fake-thread", "turnId": tid, "itemId": "m", "delta": piece}}));
                    std::thread::sleep(Duration::from_millis(10));
                }
                out(json!({"method": "item/completed", "params": {"threadId": "fake-thread", "turnId": tid, "item": {"type": "agentMessage", "id": "m", "text": text}}}));
                out(json!({"method": "turn/completed", "params": {"threadId": "fake-thread", "turn": {"id": tid, "status": "completed", "items": []}}}));
            }
            _ => {
                if id.is_some() {
                    out(json!({"id": id, "result": {}}));
                }
            }
        }
    }
}
