//! The draft flow for one popup session: which reply is current, the one JSON repair, the one
//! rule fix, and the local em dash repair. Pure logic, so it is tested without a process.

use crate::checker::{self, Rules};
use crate::engine::{EngineEvent, ErrorKind, Provider};
use crate::protocol::{parse_reply, partial_variants, Request};
use serde::Serialize;
use std::time::{Duration, Instant};

pub const TURN_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnKind {
    Request,
    JsonRepair,
    RuleFix,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Draft {
    pub text: String,
    /// Replace and Copy only act on drafts that passed the output check.
    pub ok: bool,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Output {
    Partial { req_id: u64, variants: Vec<String> },
    Checking { req_id: u64 },
    Final { req_id: u64, drafts: Vec<Draft>, subject: Option<String> },
    Error { req_id: u64, kind: ErrorKind, message: String },
    /// Not for the UI: the caller sends this text to the engine and reports the turn back.
    #[serde(skip)]
    Send { req_id: u64, text: String, kind: TurnKind },
}

#[derive(Debug, Clone)]
struct Pending {
    req_id: u64,
    turn: u64,
    sent_at: Instant,
}

#[derive(Debug, Default)]
pub struct DraftFlow {
    rules: Option<Rules>,
    provider: Provider,
    latest_req: u64,
    request: Option<Request>,
    pending: Option<Pending>,
    fix_attempted: bool,
    json_retry: bool,
    followups: Vec<String>,
}

pub fn fix_message(issues: &str) -> String {
    format!(
        "Your last reply broke the rules: it used {issues}. Rewrite the same drafts without them. Return the full JSON again."
    )
}

pub const JSON_REPAIR_MESSAGE: &str = "Your last reply was not valid JSON. Resend the same drafts as valid JSON only, in the format {\"variants\": [{\"text\": \"...\"}]}.";

impl DraftFlow {
    pub fn new(rules: Rules) -> Self {
        DraftFlow { rules: Some(rules), ..Default::default() }
    }

    pub fn for_provider(rules: Rules, provider: Provider) -> Self {
        DraftFlow { rules: Some(rules), provider, ..Default::default() }
    }

    fn rules(&self) -> Rules {
        self.rules.unwrap_or_default()
    }

    pub fn latest_req(&self) -> u64 {
        self.latest_req
    }

    pub fn request(&self) -> Option<&Request> {
        self.request.as_ref()
    }

    /// A new request from the UI. Returns the text to send.
    pub fn begin(&mut self, req_id: u64, request: Request) -> String {
        let request = request.sanitized();
        match &request.followup {
            Some(f) => self.followups.push(f.clone()),
            None => self.followups.clear(),
        }
        self.latest_req = req_id;
        self.fix_attempted = false;
        self.json_retry = false;
        let text = request.to_json();
        self.request = Some(request);
        text
    }

    /// The caller sent a message and got this turn number back.
    pub fn sent(&mut self, req_id: u64, turn: u64, kind: TurnKind) {
        if req_id == self.latest_req {
            let _ = kind;
            self.pending = Some(Pending { req_id, turn, sent_at: Instant::now() });
        }
    }

    pub fn is_waiting(&self) -> bool {
        self.pending.is_some()
    }

    /// Call often. Reports a timeout once when the current turn takes too long.
    pub fn check_timeout(&mut self, now: Instant) -> Option<Output> {
        let p = self.pending.as_ref()?;
        if now.duration_since(p.sent_at) >= TURN_TIMEOUT {
            let req_id = p.req_id;
            self.pending = None;
            return Some(error(req_id, ErrorKind::Timeout, self.provider));
        }
        None
    }

    /// The text to replay on a fresh process after a crash. Earlier follow-ups are folded in
    /// because the new process has no memory of them.
    pub fn replay_message(&self) -> Option<String> {
        let mut r = self.request.clone()?;
        if self.followups.len() > 1 {
            r.followup = Some(self.followups.join(". Then: "));
        }
        Some(r.to_json())
    }

    pub fn on_event(&mut self, ev: &EngineEvent) -> Vec<Output> {
        let Some(p) = self.pending.clone() else { return Vec::new() };
        match ev {
            EngineEvent::Delta { turn, text } if *turn == p.turn => {
                let v = partial_variants(text);
                if v.is_empty() {
                    Vec::new()
                } else {
                    vec![Output::Partial { req_id: p.req_id, variants: self.limit(v) }]
                }
            }
            EngineEvent::TurnDone { turn, text, error: err } if *turn == p.turn => {
                self.pending = None;
                if let Some(kind) = err {
                    return vec![error(p.req_id, *kind, self.provider)];
                }
                self.finish(p.req_id, text)
            }
            EngineEvent::Exited { error: kind } => {
                self.pending = None;
                vec![error(p.req_id, *kind, self.provider)]
            }
            _ => Vec::new(),
        }
    }

    fn limit(&self, mut v: Vec<String>) -> Vec<String> {
        if let Some(r) = &self.request {
            v.truncate(r.expected_drafts().max(1));
        }
        v
    }

    fn finish(&mut self, req_id: u64, raw: &str) -> Vec<Output> {
        let Some(mut reply) = parse_reply(raw) else {
            if !self.json_retry {
                self.json_retry = true;
                return vec![Output::Send { req_id, text: JSON_REPAIR_MESSAGE.into(), kind: TurnKind::JsonRepair }];
            }
            return vec![error(req_id, ErrorKind::BadReply, self.provider)];
        };
        if let Some(r) = &self.request {
            reply.variants.truncate(r.expected_drafts().max(1));
            if r.channel != "Email" {
                reply.subject = None;
            }
        }
        let rules = self.rules();
        let findings: Vec<_> = reply.variants.iter().map(|v| checker::check(&v.text, rules)).collect();
        let subject_findings = reply.subject.as_deref().map(|s| checker::check(s, rules)).unwrap_or_default();
        let all_clean = findings.iter().all(|f| f.is_clean()) && subject_findings.is_clean();
        if !all_clean && !self.fix_attempted {
            self.fix_attempted = true;
            let mut all = findings.clone();
            all.push(subject_findings);
            let texts: Vec<String> = reply.variants.iter().map(|v| v.text.clone()).collect();
            return vec![
                Output::Partial { req_id, variants: texts },
                Output::Checking { req_id },
                Output::Send { req_id, text: fix_message(&checker::describe(&all)), kind: TurnKind::RuleFix },
            ];
        }
        // Local repair for any em dash that remains, then check again.
        let drafts = reply
            .variants
            .iter()
            .map(|v| {
                let text = if rules.no_em_dashes { checker::fix_em_dashes(&v.text) } else { v.text.clone() };
                let f = checker::check(&text, rules);
                Draft { ok: f.is_clean(), issues: f.contractions.clone(), text }
            })
            .collect();
        let subject = reply.subject.map(|s| if rules.no_em_dashes { checker::fix_em_dashes(&s) } else { s });
        vec![Output::Final { req_id, drafts, subject }]
    }
}

fn error(req_id: u64, kind: ErrorKind, provider: Provider) -> Output {
    Output::Error { req_id, kind, message: kind.message_for(provider) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(text: &str) -> Request {
        Request {
            text: text.into(),
            tone: "Simple".into(),
            length: "Same".into(),
            toggles: vec![],
            channel: "Text".into(),
            followup: None,
            drafts: None,
        }
    }

    fn done(turn: u64, text: &str) -> EngineEvent {
        EngineEvent::TurnDone { turn, text: text.into(), error: None }
    }

    fn start(flow: &mut DraftFlow, req_id: u64, turn: u64) {
        flow.begin(req_id, req("hi"));
        flow.sent(req_id, turn, TurnKind::Request);
    }

    #[test]
    fn clean_reply_is_final_and_ok() {
        let mut f = DraftFlow::new(Rules::default());
        start(&mut f, 1, 1);
        let out = f.on_event(&EngineEvent::Delta { turn: 1, text: r#"{"variants":[{"text":"Hi the"#.into() });
        assert_eq!(out, vec![Output::Partial { req_id: 1, variants: vec!["Hi the".into()] }]);
        let out = f.on_event(&done(1, r#"{"variants":[{"text":"Hi there"},{"text":"Hello"}]}"#));
        match &out[0] {
            Output::Final { drafts, subject, .. } => {
                assert_eq!(drafts.len(), 2);
                assert!(drafts.iter().all(|d| d.ok));
                assert!(subject.is_none());
            }
            o => panic!("{o:?}"),
        }
        assert!(!f.is_waiting());
    }

    #[test]
    fn stale_replies_are_dropped() {
        // Five quick chip changes: only the reply to the last one is shown.
        let mut f = DraftFlow::new(Rules::default());
        for i in 1..=5 {
            start(&mut f, i, i);
        }
        for t in 1..=4 {
            assert!(f.on_event(&EngineEvent::Delta { turn: t, text: r#"{"variants":[{"text":"old"#.into() }).is_empty());
            assert!(f.on_event(&done(t, r#"{"variants":[{"text":"old"}]}"#)).is_empty());
        }
        let out = f.on_event(&done(5, r#"{"variants":[{"text":"new"},{"text":"new2"}]}"#));
        assert!(matches!(&out[0], Output::Final { req_id: 5, .. }));
    }

    #[test]
    fn contractions_trigger_one_fix_then_block() {
        let mut f = DraftFlow::new(Rules::default());
        start(&mut f, 1, 1);
        let out = f.on_event(&done(1, r#"{"variants":[{"text":"I don't know"},{"text":"I do not know"}]}"#));
        let send = out.iter().find_map(|o| match o { Output::Send { text, kind, .. } => Some((text.clone(), *kind)), _ => None }).unwrap();
        assert_eq!(send.1, TurnKind::RuleFix);
        assert!(send.0.contains("don't"));
        f.sent(1, 2, TurnKind::RuleFix);
        // Still broken after the fix: no more retries, the draft is marked not ok.
        let out = f.on_event(&done(2, r#"{"variants":[{"text":"I don't know"},{"text":"I do not know"}]}"#));
        match &out[0] {
            Output::Final { drafts, .. } => {
                assert!(!drafts[0].ok);
                assert_eq!(drafts[0].issues, vec!["don't".to_string()]);
                assert!(drafts[1].ok);
            }
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn em_dash_remaining_after_fix_is_repaired_locally() {
        let mut f = DraftFlow::new(Rules::default());
        start(&mut f, 1, 1);
        let reply = "{\"variants\":[{\"text\":\"Thanks \u{2014} see you\"}]}";
        let out = f.on_event(&done(1, reply));
        assert!(out.iter().any(|o| matches!(o, Output::Send { kind: TurnKind::RuleFix, .. })));
        f.sent(1, 2, TurnKind::RuleFix);
        let out = f.on_event(&done(2, reply));
        match &out[0] {
            Output::Final { drafts, .. } => {
                assert_eq!(drafts[0].text, "Thanks, see you");
                assert!(drafts[0].ok);
            }
            o => panic!("{o:?}"),
        }
    }

    #[test]
    fn invalid_json_gets_one_repair() {
        let mut f = DraftFlow::new(Rules::default());
        start(&mut f, 1, 1);
        let out = f.on_event(&done(1, "Sure, here are drafts"));
        assert!(matches!(&out[0], Output::Send { kind: TurnKind::JsonRepair, .. }));
        f.sent(1, 2, TurnKind::JsonRepair);
        let out = f.on_event(&done(2, "still not json"));
        assert!(matches!(&out[0], Output::Error { kind: ErrorKind::BadReply, .. }));
    }

    #[test]
    fn errors_and_exit_and_timeout() {
        let mut f = DraftFlow::new(Rules::default());
        start(&mut f, 1, 1);
        let out = f.on_event(&EngineEvent::TurnDone { turn: 1, text: "x".into(), error: Some(ErrorKind::LimitReached) });
        assert!(matches!(&out[0], Output::Error { kind: ErrorKind::LimitReached, .. }));
        start(&mut f, 2, 2);
        let out = f.on_event(&EngineEvent::Exited { error: ErrorKind::ProcessExit });
        assert!(matches!(&out[0], Output::Error { kind: ErrorKind::ProcessExit, .. }));
        start(&mut f, 3, 3);
        assert!(f.check_timeout(Instant::now()).is_none());
        let out = f.check_timeout(Instant::now() + TURN_TIMEOUT + Duration::from_millis(1)).unwrap();
        assert!(matches!(out, Output::Error { kind: ErrorKind::Timeout, .. }));
        assert!(f.check_timeout(Instant::now() + TURN_TIMEOUT * 2).is_none());
    }

    #[test]
    fn fix_errors_only_returns_one_draft_and_subject_only_for_email() {
        let mut f = DraftFlow::new(Rules::default());
        let mut r = req("i has a typo");
        r.toggles = vec!["Fix errors only".into()];
        f.begin(1, r);
        f.sent(1, 1, TurnKind::Request);
        let out = f.on_event(&done(1, r#"{"variants":[{"text":"I have a typo"},{"text":"extra"}],"subject":"S"}"#));
        match &out[0] {
            Output::Final { drafts, subject, .. } => {
                assert_eq!(drafts.len(), 1);
                assert!(subject.is_none());
            }
            o => panic!("{o:?}"),
        }
        let mut r = req("hello");
        r.channel = "Email".into();
        f.begin(2, r);
        f.sent(2, 2, TurnKind::Request);
        let out = f.on_event(&done(2, r#"{"variants":[{"text":"Hi,\n\nHello.\n\nBest,"}],"subject":"Quick hello"}"#));
        assert!(matches!(&out[0], Output::Final { subject: Some(s), .. } if s == "Quick hello"));
    }

    #[test]
    fn replay_folds_followups() {
        let mut f = DraftFlow::new(Rules::default());
        f.begin(1, req("hi"));
        let mut r = req("hi");
        r.followup = Some("remove the apology".into());
        f.begin(2, r.clone());
        r.followup = Some("make it shorter".into());
        f.begin(3, r);
        let replay = f.replay_message().unwrap();
        assert!(replay.contains("remove the apology. Then: make it shorter"));
        // A chip change starts over.
        f.begin(4, req("hi"));
        assert!(f.replay_message().unwrap().contains("\"followup\":null"));
    }

    #[test]
    fn rules_off_allows_contractions() {
        let mut f = DraftFlow::new(Rules { no_contractions: false, no_em_dashes: true });
        start(&mut f, 1, 1);
        let out = f.on_event(&done(1, r#"{"variants":[{"text":"I don't know"}]}"#));
        assert!(matches!(&out[0], Output::Final { drafts, .. } if drafts[0].ok));
    }
}
