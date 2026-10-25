// Pure popup logic: requests, outputs, and what the Replace button may do.
// No DOM here, so it is covered by unit tests.

export const TONES = ["Simple", "Respectful", "Warm", "Casual", "Professional", "Firm"] as const;
export const LENGTHS = ["Shorter", "Same", "Fuller"] as const;
export const TOGGLES = ["Fix errors only", "Softer", "Less thankful"] as const;
export const CHANNELS = ["Text", "Email", "WhatsApp", "LinkedIn"] as const;

export type Capture =
  | { kind: "text"; text: string }
  | { kind: "nothing" }
  | { kind: "unreadable"; reason: string }
  /** The popup is up while the hotkey keys are still held; the selection is read next. */
  | { kind: "pending" };

export interface Target {
  app_name: string;
  process: string;
  icon: string | null;
  elevated: boolean;
  own_window: boolean;
  present: boolean;
}

export interface OpenPayload {
  session_id: number;
  capture: Capture;
  target: Target;
  channel: string;
  tone: string;
  length: string;
  toggles: string[];
  drafts: number;
  long_text_limit: number;
  theme: string;
  translucent: boolean;
  engine_error: string | null;
  no_contractions: boolean;
  no_em_dashes: boolean;
  clipboard_warning?: boolean;
  /** The service writing the drafts, such as "Claude" or "ChatGPT". */
  provider?: string;
}

export interface Request {
  text: string;
  tone: string;
  length: string;
  toggles: string[];
  channel: string;
  followup: string | null;
  drafts?: number;
}

export type DraftStatus = "waiting" | "streaming" | "checking" | "ready" | "blocked";

export interface DraftView {
  text: string;
  status: DraftStatus;
  issues: string[];
}

export type Output =
  | { type: "partial"; req_id: number; variants: string[] }
  | { type: "checking"; req_id: number }
  | { type: "final"; req_id: number; drafts: { text: string; ok: boolean; issues: string[] }[]; subject: string | null }
  | { type: "error"; req_id: number; kind: string; message: string };

export interface PopupState {
  payload: OpenPayload | null;
  text: string;
  editable: boolean;
  notice: string | null;
  tone: string;
  length: string;
  toggles: string[];
  channel: string;
  channelAuto: boolean;
  reqId: number;
  lastRequest: Request | null;
  drafts: DraftView[];
  subject: string | null;
  error: { kind: string; message: string } | null;
  needsLongConfirm: boolean;
  targetFocused: boolean;
  targetAlive: boolean;
  busy: boolean;
  /** Waiting for the selection: placeholders show and nothing is sent. */
  pending: boolean;
}

export function initialState(): PopupState {
  return {
    payload: null,
    text: "",
    editable: true,
    notice: null,
    tone: "Simple",
    length: "Same",
    toggles: [],
    channel: "Text",
    channelAuto: true,
    reqId: 0,
    lastRequest: null,
    drafts: [],
    subject: null,
    error: null,
    needsLongConfirm: false,
    targetFocused: false,
    targetAlive: true,
    busy: false,
    pending: false,
  };
}

export function fromPayload(p: OpenPayload): PopupState {
  const s = initialState();
  s.payload = p;
  s.tone = p.tone;
  s.length = p.length;
  s.toggles = [...p.toggles];
  s.channel = p.channel;
  s.targetAlive = p.target.present;
  if (p.capture.kind === "text") {
    s.text = p.capture.text;
    s.editable = false;
    s.needsLongConfirm = p.capture.text.length > p.long_text_limit;
  } else if (p.capture.kind === "pending") {
    s.pending = true;
    s.editable = false;
    s.drafts = Array.from({ length: expectedDrafts(s) }, () => ({ text: "", status: "waiting" as DraftStatus, issues: [] }));
  } else if (p.capture.kind === "unreadable") {
    s.notice = p.capture.reason;
  }
  if (p.clipboard_warning && p.capture.kind === "text") {
    s.notice = "Your clipboard holds something Magic Mend cannot fully restore, such as a drawing. Replace may leave only part of it.";
  }
  if (p.engine_error) {
    s.error = { kind: "engine", message: p.engine_error };
  }
  return s;
}

export function expectedDrafts(s: PopupState): number {
  if (s.toggles.includes("Fix errors only")) return 1;
  return s.payload?.drafts ?? 2;
}

/** Drafts start immediately unless there is no text or a long text still needs a yes. */
export function shouldAutoStart(s: PopupState): boolean {
  return !s.pending && s.text.trim().length > 0 && !s.needsLongConfirm && !s.error;
}

export function buildRequest(s: PopupState, followup: string | null = null): Request {
  const r: Request = {
    text: s.text,
    tone: s.tone,
    length: s.length,
    toggles: TOGGLES.filter((t) => s.toggles.includes(t)),
    channel: s.channel,
    followup: followup && followup.trim() ? followup.trim() : null,
  };
  const n = s.payload?.drafts ?? 2;
  if (n !== 2) r.drafts = n;
  return r;
}

/** Starts a new request: bumps the id and shows skeleton cards. */
export function beginRequest(s: PopupState, followup: string | null = null): Request {
  s.reqId += 1;
  s.error = null;
  s.subject = followup ? s.subject : null;
  const n = expectedDrafts(s);
  const keep = followup !== null;
  s.drafts = Array.from({ length: n }, (_, i) => ({
    text: keep ? s.drafts[i]?.text ?? "" : "",
    status: "waiting" as DraftStatus,
    issues: [],
  }));
  s.busy = true;
  const req = buildRequest(s, followup);
  s.lastRequest = req;
  return req;
}

/** Applies an engine output. Outputs for older requests are ignored. */
export function applyOutput(s: PopupState, o: Output): boolean {
  if (o.req_id !== s.reqId) return false;
  switch (o.type) {
    case "partial": {
      o.variants.forEach((text, i) => {
        if (!s.drafts[i]) s.drafts[i] = { text: "", status: "waiting", issues: [] };
        s.drafts[i].text = text;
        if (s.drafts[i].status === "waiting" || s.drafts[i].status === "streaming") s.drafts[i].status = "streaming";
      });
      return true;
    }
    case "checking":
      s.drafts.forEach((d) => (d.status = "checking"));
      return true;
    case "final":
      s.drafts = o.drafts.map((d) => ({ text: d.text, status: d.ok ? "ready" : "blocked", issues: d.issues }));
      s.subject = o.subject;
      s.busy = false;
      return true;
    case "error":
      s.error = { kind: o.kind, message: o.message };
      s.busy = false;
      s.drafts = s.drafts.filter((d) => d.status === "ready");
      return true;
  }
}

export interface ReplaceMode {
  label: string;
  enabled: boolean;
  /** One plain line shown near the drafts when Replace cannot paste. */
  reason: string | null;
}

export function replaceMode(s: PopupState): ReplaceMode {
  const t = s.payload?.target;
  if (!t || !t.present || s.payload?.capture.kind !== "text") {
    if (t?.elevated) return { label: "Replace", enabled: false, reason: "This app runs as administrator. Use Copy instead." };
    return { label: "Replace", enabled: false, reason: s.payload?.capture.kind === "text" ? "Replace is off because there is no window to paste into. Use Copy." : null };
  }
  if (t.elevated) return { label: "Replace", enabled: false, reason: "This app runs as administrator. Use Copy instead." };
  if (!s.targetAlive) return { label: "Copy only", enabled: false, reason: "The original window was closed, so Replace is off. Use Copy." };
  if (s.targetFocused) return { label: "Paste at cursor", enabled: true, reason: null };
  return { label: "Replace", enabled: true, reason: null };
}

export function canActOn(d: DraftView): boolean {
  return d.status === "ready" && d.text.trim().length > 0;
}

export function headerLine(s: PopupState): string {
  const t = s.payload?.target;
  if (!t || !t.present) return "No window to paste into";
  if (s.targetFocused) return `The selection in ${t.app_name || "the app"} may have changed`;
  return `Replace will paste into: ${t.app_name || t.process || "the app"}`;
}
