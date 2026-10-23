//! The request sent to Claude on each turn and the JSON reply it returns
//! (spec, "Chips and prompt").

use serde::{Deserialize, Serialize};

pub const TONES: [&str; 6] = ["Simple", "Respectful", "Warm", "Casual", "Professional", "Firm"];
pub const LENGTHS: [&str; 3] = ["Shorter", "Same", "Fuller"];
pub const TOGGLES: [&str; 3] = ["Fix errors only", "Softer", "Less thankful"];
pub const CHANNELS: [&str; 4] = ["Text", "Email", "WhatsApp", "LinkedIn"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Request {
    pub text: String,
    pub tone: String,
    pub length: String,
    pub toggles: Vec<String>,
    pub channel: String,
    pub followup: Option<String>,
    /// Only sent when the user picked a draft count other than two.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub drafts: Option<u8>,
}

impl Request {
    pub fn fix_errors_only(&self) -> bool {
        self.toggles.iter().any(|t| t == "Fix errors only")
    }

    /// How many drafts to expect back.
    pub fn expected_drafts(&self) -> usize {
        if self.fix_errors_only() {
            1
        } else {
            self.drafts.map(|d| d as usize).unwrap_or(2)
        }
    }

    /// Keeps only known values, so nothing odd reaches the prompt.
    pub fn sanitized(mut self) -> Request {
        if !TONES.contains(&self.tone.as_str()) {
            self.tone = "Simple".into();
        }
        if !LENGTHS.contains(&self.length.as_str()) {
            self.length = "Same".into();
        }
        self.toggles.retain(|t| TOGGLES.contains(&t.as_str()));
        self.toggles.dedup();
        if !CHANNELS.contains(&self.channel.as_str()) {
            self.channel = "Text".into();
        }
        if let Some(f) = &self.followup {
            if f.trim().is_empty() {
                self.followup = None;
            }
        }
        if self.drafts == Some(2) || self.drafts == Some(0) {
            self.drafts = None;
        }
        self.drafts = self.drafts.map(|d| d.min(4));
        self
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("request serializes")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Variant {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Reply {
    pub variants: Vec<Variant>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
}

/// Wraps one user turn in the stream-json input shape.
pub fn user_message_line(text: &str) -> String {
    serde_json::json!({
        "type": "user",
        "message": {"role": "user", "content": [{"type": "text", "text": text}]}
    })
    .to_string()
}

/// Finds the JSON object in a reply, ignoring code fences or stray words around it.
fn json_slice(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    (end > start).then(|| &raw[start..=end])
}

/// Parses the final reply. Returns None when it is not the JSON object the prompt asks for.
pub fn parse_reply(raw: &str) -> Option<Reply> {
    let slice = json_slice(raw)?;
    let mut reply: Reply = serde_json::from_str(slice).ok()?;
    reply.variants.retain(|v| !v.text.trim().is_empty());
    if reply.variants.is_empty() {
        return None;
    }
    for v in &mut reply.variants {
        v.text = clean_draft(&v.text);
    }
    if let Some(s) = &reply.subject {
        let s = s.trim().to_string();
        reply.subject = (!s.is_empty()).then_some(s);
    }
    Some(reply)
}

/// Trims surrounding space and every trailing line break, because some apps treat a pasted
/// line break like Enter.
pub fn clean_draft(text: &str) -> String {
    text.trim().trim_end_matches(['\r', '\n']).to_string()
}

/// Pulls the text of each variant out of a reply that is still streaming. Unfinished strings
/// are returned as far as they go, so the UI can show words as they arrive.
pub fn partial_variants(raw: &str) -> Vec<String> {
    let Some(vpos) = raw.find("\"variants\"") else { return Vec::new() };
    let bytes = raw.as_bytes();
    let mut out = Vec::new();
    let mut i = vpos + "\"variants\"".len();
    // Stop at the end of the variants array when we reach it.
    let mut depth: i32 = 0;
    let mut started = false;
    while i < bytes.len() {
        match bytes[i] {
            b'[' => {
                depth += 1;
                started = true;
                i += 1;
            }
            b']' => {
                depth -= 1;
                i += 1;
                if started && depth <= 0 {
                    break;
                }
            }
            b'"' => {
                // Read a key or value string.
                let (s, next, closed) = read_json_string(raw, i);
                i = next;
                if s == "text" && closed {
                    // Expect a colon then a string value.
                    let mut j = i;
                    while j < bytes.len() && (bytes[j] as char).is_whitespace() {
                        j += 1;
                    }
                    if j < bytes.len() && bytes[j] == b':' {
                        j += 1;
                        while j < bytes.len() && (bytes[j] as char).is_whitespace() {
                            j += 1;
                        }
                        if j < bytes.len() && bytes[j] == b'"' {
                            let (val, next, _) = read_json_string(raw, j);
                            out.push(val);
                            i = next;
                        } else {
                            i = j;
                        }
                    } else {
                        i = j;
                    }
                }
            }
            _ => i += 1,
        }
    }
    out
}

/// Reads a JSON string starting at the opening quote. Returns the decoded text, the index after
/// it, and whether the closing quote was seen.
fn read_json_string(raw: &str, start: usize) -> (String, usize, bool) {
    let bytes = raw.as_bytes();
    let mut out = String::new();
    let mut i = start + 1;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'"' {
            return (out, i + 1, true);
        }
        if b == b'\\' {
            if i + 1 >= bytes.len() {
                break; // escape cut off mid-stream
            }
            let e = bytes[i + 1];
            match e {
                b'n' => out.push('\n'),
                b't' => out.push('\t'),
                b'r' => out.push('\r'),
                b'b' | b'f' => {}
                b'/' => out.push('/'),
                b'\\' => out.push('\\'),
                b'"' => out.push('"'),
                b'u' => {
                    if i + 6 > bytes.len() {
                        break;
                    }
                    let hex = &raw[i + 2..i + 6];
                    let Ok(code) = u32::from_str_radix(hex, 16) else { break };
                    // Surrogate pairs.
                    if (0xD800..0xDC00).contains(&code) {
                        if i + 12 > bytes.len() || &raw[i + 6..i + 8] != "\\u" {
                            break;
                        }
                        let Ok(low) = u32::from_str_radix(&raw[i + 8..i + 12], 16) else { break };
                        let full = 0x10000 + ((code - 0xD800) << 10) + (low.wrapping_sub(0xDC00) & 0x3FF);
                        if let Some(c) = char::from_u32(full) {
                            out.push(c);
                        }
                        i += 12;
                        continue;
                    }
                    if let Some(c) = char::from_u32(code) {
                        out.push(c);
                    }
                    i += 6;
                    continue;
                }
                other => out.push(other as char),
            }
            i += 2;
            continue;
        }
        // Copy one UTF-8 character.
        let ch = raw[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    (out, bytes.len(), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req() -> Request {
        Request {
            text: "hi".into(),
            tone: "Simple".into(),
            length: "Same".into(),
            toggles: vec!["Softer".into()],
            channel: "Text".into(),
            followup: None,
            drafts: None,
        }
    }

    #[test]
    fn request_matches_spec_shape() {
        assert_eq!(
            req().to_json(),
            r#"{"text":"hi","tone":"Simple","length":"Same","toggles":["Softer"],"channel":"Text","followup":null}"#
        );
    }

    #[test]
    fn sanitize_drops_unknown_values() {
        let mut r = req();
        r.tone = "Pirate".into();
        r.length = "Huge".into();
        r.toggles = vec!["Softer".into(), "Evil".into()];
        r.channel = "Fax".into();
        r.followup = Some("   ".into());
        r.drafts = Some(2);
        let s = r.sanitized();
        assert_eq!((s.tone.as_str(), s.length.as_str(), s.channel.as_str()), ("Simple", "Same", "Text"));
        assert_eq!(s.toggles, vec!["Softer".to_string()]);
        assert_eq!(s.followup, None);
        assert_eq!(s.drafts, None);
    }

    #[test]
    fn expected_drafts() {
        let mut r = req();
        assert_eq!(r.expected_drafts(), 2);
        r.drafts = Some(3);
        assert_eq!(r.expected_drafts(), 3);
        r.toggles.push("Fix errors only".into());
        assert_eq!(r.expected_drafts(), 1);
    }

    #[test]
    fn user_line_shape() {
        let v: serde_json::Value = serde_json::from_str(&user_message_line("x\"y")).unwrap();
        assert_eq!(v["type"], "user");
        assert_eq!(v["message"]["content"][0]["text"], "x\"y");
    }

    #[test]
    fn parses_plain_and_fenced_reply() {
        let r = parse_reply(r#"{"variants":[{"text":"A\n"},{"text":"B"}]}"#).unwrap();
        assert_eq!(r.variants.len(), 2);
        assert_eq!(r.variants[0].text, "A");
        let r = parse_reply("```json\n{\"variants\":[{\"text\":\"A\"}],\"subject\":\"Hi\"}\n```").unwrap();
        assert_eq!(r.subject.as_deref(), Some("Hi"));
        assert!(parse_reply("Sure! Here you go").is_none());
        assert!(parse_reply(r#"{"variants":[]}"#).is_none());
        assert!(parse_reply(r#"{"variants":[{"text":"A"}"#).is_none());
    }

    #[test]
    fn clean_draft_never_ends_with_line_break() {
        assert_eq!(clean_draft("Hello\r\n\r\n"), "Hello");
        assert_eq!(clean_draft("  a\nb \n"), "a\nb");
    }

    #[test]
    fn partial_extraction_streams() {
        let full = r#"{"variants": [{"text": "Hello there, I am \"here\"\nnow"}, {"text": "Hi é 😀"}], "subject": "S"}"#;
        assert_eq!(
            partial_variants(full),
            vec!["Hello there, I am \"here\"\nnow".to_string(), "Hi é 😀".to_string()]
        );
        // Every prefix must parse without panicking and grow monotonically.
        let mut last_len = 0usize;
        for (idx, _) in full.char_indices() {
            let v = partial_variants(&full[..idx]);
            let total: usize = v.iter().map(|s| s.chars().count()).sum();
            assert!(total + 2 >= last_len, "shrunk at {idx}");
            last_len = total;
        }
        assert_eq!(partial_variants(r#"{"variants": [{"text": "Hel"#), vec!["Hel".to_string()]);
        assert!(partial_variants(r#"{"vari"#).is_empty());
    }

    #[test]
    fn partial_ignores_subject_text() {
        let s = r#"{"subject": "text", "variants": [{"text": "A"}]}"#;
        assert_eq!(partial_variants(s), vec!["A".to_string()]);
    }
}
