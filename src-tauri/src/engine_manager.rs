//! Warm spare lifecycle (spec, "Lifecycle (warm spare)"):
//! one spare waits; the popup takes it; follow-ups reuse it; after Replace, Copy, or close it is
//! killed and a fresh spare starts. Spares are recycled after 6 idle hours and after sleep.

use crate::checker::Rules;
use crate::drafts::{DraftFlow, Output, TurnKind};
use crate::engine::{ClaudeEngine, Engine, EngineConfig, EngineEvent, ErrorKind, Tagged};
use crate::logging;
use crate::platform::imp;
use crate::protocol::Request;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};

pub const IDLE_RECYCLE: Duration = Duration::from_secs(6 * 60 * 60);
/// Sleep longer than this since the last check means the computer slept.
const SLEEP_JUMP_MS: u64 = 30_000;

pub struct EngineManager {
    cfg: Option<EngineConfig>,
    warm: bool,
    rules: Rules,
    tx: Sender<Tagged>,
    spare: Option<ClaudeEngine>,
    spare_primed: bool,
    spare_since: Instant,
    spare_failures: u32,
    next_spare_at: Instant,
    active: Option<ClaudeEngine>,
    flow: DraftFlow,
    in_session: bool,
    replayed: bool,
    sleep_mark: u64,
    session_started: Option<Instant>,
    first_words_logged: bool,
}

impl EngineManager {
    pub fn new(cfg: Option<EngineConfig>, warm: bool, rules: Rules) -> (Self, Receiver<Tagged>) {
        let (tx, rx) = channel();
        (
            EngineManager {
                cfg,
                warm,
                rules,
                tx,
                spare: None,
                spare_primed: false,
                spare_since: Instant::now(),
                spare_failures: 0,
                next_spare_at: Instant::now(),
                active: None,
                flow: DraftFlow::new(rules),
                in_session: false,
                replayed: false,
                sleep_mark: imp::sleep_ms_total(),
                session_started: None,
                first_words_logged: false,
            },
            rx,
        )
    }

    pub fn configure(&mut self, cfg: Option<EngineConfig>, warm: bool, rules: Rules) {
        let changed = match (&self.cfg, &cfg) {
            (Some(a), Some(b)) => a.claude_path != b.claude_path || a.model != b.model || a.priming != b.priming,
            (None, None) => false,
            _ => true,
        };
        self.cfg = cfg;
        self.warm = warm;
        self.rules = rules;
        if changed || !warm {
            self.drop_spare();
        }
        self.spare_failures = 0;
        self.next_spare_at = Instant::now();
        self.ensure_spare();
    }

    pub fn has_config(&self) -> bool {
        self.cfg.is_some()
    }

    pub fn spare_state(&self) -> &'static str {
        match (&self.spare, self.spare_primed) {
            (None, _) => "none",
            (Some(_), true) => "primed",
            (Some(_), false) => "started",
        }
    }

    pub fn spare_pid(&self) -> Option<u32> {
        self.spare.as_ref().and_then(|s| s.pid())
    }

    pub fn active_pid(&self) -> Option<u32> {
        self.active.as_ref().and_then(|s| s.pid())
    }

    fn spawn(&self) -> Result<ClaudeEngine, ErrorKind> {
        let cfg = self.cfg.clone().ok_or(ErrorKind::NotFound)?;
        let mut e = ClaudeEngine::new(cfg, self.tx.clone());
        e.start().map_err(|err| err.0)?;
        Ok(e)
    }

    fn drop_spare(&mut self) {
        if let Some(mut s) = self.spare.take() {
            s.stop();
        }
        self.spare_primed = false;
    }

    /// Starts a spare when warm mode is on and none is waiting.
    pub fn ensure_spare(&mut self) {
        if !self.warm || self.spare.is_some() || self.in_session || self.cfg.is_none() {
            return;
        }
        if Instant::now() < self.next_spare_at {
            return;
        }
        match self.spawn() {
            Ok(e) => {
                self.spare = Some(e);
                self.spare_primed = false;
                self.spare_since = Instant::now();
                logging::event("spare_started", &[]);
            }
            Err(kind) => self.spare_failed(kind),
        }
    }

    fn spare_failed(&mut self, kind: ErrorKind) {
        self.spare_failures += 1;
        let backoff = Duration::from_secs((2u64.pow(self.spare_failures.min(8))).min(300));
        self.next_spare_at = Instant::now() + backoff;
        logging::event("spare_failed", &[("category", kind.category())]);
    }

    /// The hotkey was pressed: take the spare, or start a process on demand.
    pub fn begin_session(&mut self) -> Result<(), ErrorKind> {
        self.end_active();
        self.in_session = true;
        self.replayed = false;
        self.first_words_logged = false;
        self.session_started = Some(Instant::now());
        self.flow = DraftFlow::new(self.rules);
        let warm_hit = self.spare.is_some();
        let engine = match self.spare.take() {
            Some(mut s) => {
                if s.is_running() {
                    s
                } else {
                    self.spawn()?
                }
            }
            None => self.spawn()?,
        };
        logging::event("session_start", &[("warm", if warm_hit { "1" } else { "0" })]);
        self.spare_primed = false;
        self.active = Some(engine);
        Ok(())
    }

    pub fn in_session(&self) -> bool {
        self.in_session
    }

    /// Sends a request from the popup.
    pub fn request(&mut self, req_id: u64, request: Request) -> Vec<Output> {
        if !self.in_session {
            if let Err(k) = self.begin_session() {
                return vec![error_out(req_id, k)];
            }
        }
        if self.active.is_none() {
            match self.spawn() {
                Ok(e) => self.active = Some(e),
                Err(k) => return vec![error_out(req_id, k)],
            }
        }
        let text = self.flow.begin(req_id, request);
        self.send_turn(req_id, &text, TurnKind::Request)
    }

    fn send_turn(&mut self, req_id: u64, text: &str, kind: TurnKind) -> Vec<Output> {
        let Some(active) = self.active.as_mut() else { return vec![error_out(req_id, ErrorKind::ProcessExit)] };
        match active.send(text) {
            Ok(turn) => {
                self.flow.sent(req_id, turn, kind);
                Vec::new()
            }
            Err(e) => {
                // The process is gone. Start a fresh one and send once more.
                if !self.replayed {
                    self.replayed = true;
                    if let Ok(mut fresh) = self.spawn() {
                        let replay = self.flow.replay_message().unwrap_or_else(|| text.to_string());
                        if let Ok(turn) = fresh.send(&replay) {
                            self.active = Some(fresh);
                            self.flow.sent(req_id, turn, kind);
                            return Vec::new();
                        }
                    }
                }
                vec![error_out(req_id, e.0)]
            }
        }
    }

    /// Retry after an error or timeout: a fresh process and the same request.
    pub fn retry(&mut self, req_id: u64, request: Request) -> Vec<Output> {
        self.end_active();
        self.replayed = false;
        match self.spawn() {
            Ok(e) => {
                self.active = Some(e);
                self.in_session = true;
                self.request(req_id, request)
            }
            Err(k) => vec![error_out(req_id, k)],
        }
    }

    fn end_active(&mut self) {
        if let Some(mut a) = self.active.take() {
            a.stop();
        }
    }

    /// Replace, Copy, or close: kill the session process and start a fresh spare.
    pub fn end_session(&mut self) {
        self.end_active();
        self.in_session = false;
        self.flow = DraftFlow::new(self.rules);
        self.session_started = None;
        self.next_spare_at = Instant::now();
        self.ensure_spare();
    }

    pub fn handle(&mut self, t: Tagged) -> Vec<Output> {
        let is_active = self.active.as_ref().map(|a| a.id) == Some(t.engine);
        let is_spare = self.spare.as_ref().map(|s| s.id) == Some(t.engine);
        if is_spare {
            match t.event {
                EngineEvent::Primed { ms } => {
                    self.spare_primed = true;
                    self.spare_failures = 0;
                    logging::timing("spare_primed", ms);
                }
                EngineEvent::Exited { error } => {
                    self.spare = None;
                    self.spare_primed = false;
                    self.spare_failed(error);
                }
                EngineEvent::TurnDone { turn: 0, error: Some(kind), .. } => {
                    logging::event("priming_failed", &[("category", kind.category())]);
                }
                _ => {}
            }
            return Vec::new();
        }
        if !is_active {
            return Vec::new(); // an old process
        }
        if let EngineEvent::Exited { .. } = &t.event {
            self.active = None;
            if self.flow.is_waiting() && !self.replayed {
                // Died mid-session: respawn and replay the current request once.
                self.replayed = true;
                logging::event("engine_replay", &[]);
                if let (Ok(mut fresh), Some(replay)) = (self.spawn(), self.flow.replay_message()) {
                    let req_id = self.flow.latest_req();
                    if let Ok(turn) = fresh.send(&replay) {
                        self.active = Some(fresh);
                        self.flow.sent(req_id, turn, TurnKind::Request);
                        return Vec::new();
                    }
                }
            }
        }
        if let (EngineEvent::Delta { .. }, false, Some(start)) = (&t.event, self.first_words_logged, self.session_started) {
            self.first_words_logged = true;
            logging::timing("first_words", start.elapsed().as_millis());
        }
        let mut ui = Vec::new();
        for out in self.flow.on_event(&t.event) {
            match out {
                Output::Send { req_id, text, kind } => ui.extend(self.send_turn(req_id, &text, kind)),
                Output::Error { kind, .. } => {
                    logging::event("engine_error", &[("category", kind.category())]);
                    ui.push(out);
                }
                Output::Final { .. } => {
                    if let Some(start) = self.session_started {
                        logging::timing("drafts_final", start.elapsed().as_millis());
                    }
                    ui.push(out);
                }
                other => ui.push(other),
            }
        }
        ui
    }

    /// Call about once a second.
    pub fn tick(&mut self) -> Vec<Output> {
        let mut out = Vec::new();
        if let Some(o) = self.flow.check_timeout(Instant::now()) {
            logging::event("engine_error", &[("category", "timeout")]);
            out.push(o);
        }
        let slept = imp::sleep_ms_total();
        let woke = slept.saturating_sub(self.sleep_mark) > SLEEP_JUMP_MS;
        self.sleep_mark = slept;
        if self.spare.is_some() && (woke || self.spare_since.elapsed() > IDLE_RECYCLE) {
            logging::event("spare_recycled", &[("reason", if woke { "wake" } else { "idle" })]);
            self.drop_spare();
            self.next_spare_at = Instant::now();
        }
        if let Some(s) = self.spare.as_mut() {
            if !s.is_running() {
                self.spare = None;
                self.spare_primed = false;
            }
        }
        self.ensure_spare();
        out
    }

    pub fn shutdown(&mut self) {
        self.end_active();
        self.drop_spare();
    }
}

fn error_out(req_id: u64, kind: ErrorKind) -> Output {
    Output::Error { req_id, kind, message: kind.message().to_string() }
}
