// Calls into the Rust side. Outside Tauri (a plain browser with ?mock), a fake backend streams
// sample drafts so the popup can be designed and tested without Claude.

import type { OpenPayload, Output, Request } from "./popup-state";

type Handler<T> = (payload: T) => void;

export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export async function invoke<T = unknown>(cmd: string, args: Record<string, unknown> = {}): Promise<T> {
  if (inTauri) {
    const core = await import("@tauri-apps/api/core");
    return core.invoke<T>(cmd, args);
  }
  return mock.invoke(cmd, args) as Promise<T>;
}

export async function listen<T>(event: string, handler: Handler<T>): Promise<void> {
  if (inTauri) {
    const ev = await import("@tauri-apps/api/event");
    await ev.listen<T>(event, (e) => handler(e.payload));
    return;
  }
  mock.on(event, handler as Handler<unknown>);
}

// ---------------------------------------------------------------------------------------------
// Fake backend for design work and browser tests.

const SAMPLE = "hi chi, we had a great stay, can we check out at 1pm instead of 11, sorry for asking so late";

const FAKE: Record<string, string[]> = {
  Simple: [
    "Hi Chi, we had a great stay. Could we check out at 1pm instead of 11? Sorry for asking so late.",
    "Hi Chi, our stay was great. Is it possible to check out at 1pm instead of 11? Sorry for the late ask.",
  ],
  Respectful: [
    "Hi Chi, thank you for a wonderful stay. Could you please let us know if we may check out at 1pm instead of 11? I apologize for asking so late.",
    "Hi Chi, we truly enjoyed our stay. Would it be possible to check out at 1pm instead of 11? I am sorry for the late request.",
  ],
};

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

export const fakeSettings = {
  hotkey: "Ctrl+Alt+R",
  default_tone: "Simple",
  default_length: "Same",
  default_toggles: [] as string[],
  drafts: 2,
  model: "sonnet",
  warm_mode: true,
  priming: false,
  theme: "system",
  start_at_login: true,
  popup_position: "center",
  excluded_apps: ["WindowsTerminal", "powershell", "cmd", "1Password", "mstsc"],
  channel_rules: [
    { process: ["outlook", "thunderbird"], title: ["Outlook", "Mail"], browser_only: false, channel: "Email" },
    { process: [], title: ["Gmail"], browser_only: true, channel: "Email" },
    { process: ["whatsapp"], title: ["WhatsApp"], browser_only: false, channel: "WhatsApp" },
    { process: [], title: ["LinkedIn"], browser_only: true, channel: "LinkedIn" },
  ],
  claude_path: null as string | null,
  no_contractions: true,
  no_em_dashes: true,
  paused: false,
  setup: { version: 0, completed: [] as string[], pending_new: [] as string[] },
};

const listeners = new Map<string, Handler<unknown>[]>();

export const mock = {
  log: [] as { cmd: string; args: Record<string, unknown> }[],
  on(event: string, h: Handler<unknown>) {
    listeners.set(event, [...(listeners.get(event) ?? []), h]);
  },
  emit(event: string, payload: unknown) {
    (listeners.get(event) ?? []).forEach((h) => h(payload));
  },
  payload(overrides: Partial<OpenPayload> = {}): OpenPayload {
    return {
      session_id: 1,
      capture: { kind: "text", text: SAMPLE },
      target: { app_name: "WhatsApp", process: "WhatsApp.exe", icon: null, elevated: false, own_window: false, present: true },
      channel: "WhatsApp",
      tone: "Simple",
      length: "Same",
      toggles: [],
      drafts: 2,
      long_text_limit: 4000,
      theme: "system",
      translucent: false,
      engine_error: null,
      no_contractions: true,
      no_em_dashes: true,
      ...overrides,
    };
  },
  async invoke(cmd: string, args: Record<string, unknown>): Promise<unknown> {
    this.log.push({ cmd, args });
    switch (cmd) {
      case "request_drafts":
      case "retry_drafts":
        this.stream(args.reqId as number, args.request as Request);
        return null;
      case "replace_draft":
        return { result: "pasted" };
      case "copy_text":
        return true;
      case "target_status":
        return { alive: true, focused: false };
      case "app_info":
        return {
          settings: { ...fakeSettings },
          platform: "windows",
          version: "1.0.0",
          hotkey_display: "Ctrl+Alt+R",
          detected_claude: "C:/Users/me/.local/bin/claude.exe",
          log_path: null,
          steps: ["welcome", "connect", "hotkey", "defaults", "try", "done"],
          required: ["connect"],
          first_unfinished: "welcome",
          translucent: false,
        };
      case "wizard_check_claude":
        await sleep(400);
        return { found: true, path: "C:/Users/me/.local/bin/claude.exe", version: "2.1.296 (Claude Code)", signed_in: true, message: null };
      case "wizard_test_engine":
        await sleep(1200);
        return { ok: true, message: "Claude is connected.", ms: 2400, sample: "Thanks for helping me move last weekend." };
      case "check_hotkey":
        return { ok: true, display: String(args.hotkeyStr), warnings: [], error: null };
      case "set_hotkey":
        return String(args.hotkeyStr);
      case "access_status":
        return true;
      default:
        return null;
    }
  },
  /** Streams fake drafts word by word, like the real engine. */
  stream(reqId: number, req: Request) {
    const fix = req.toggles.includes("Fix errors only");
    let drafts = (FAKE[req.tone] ?? FAKE.Simple).slice(0, fix ? 1 : req.drafts ?? 2);
    if (fix) drafts = ["Hi Chi, we had a great stay. Can we check out at 1pm instead of 11? Sorry for asking so late."];
    if (req.followup) drafts = drafts.map((d) => d.replace(/ ?(Sorry|I apologize|I am sorry)[^.]*\./, ""));
    if (req.channel === "Email") drafts = drafts.map((d) => d.replace(/^Hi Chi, /, "Hi Chi,\n\n") + "\n\nBest,");
    const words = drafts.map((d) => d.split(/(?<= )/));
    const max = Math.max(...words.map((w) => w.length));
    let i = 0;
    const tick = () => {
      i += 1;
      const variants = words.map((w) => w.slice(0, i).join(""));
      this.emit("popup:output", { type: "partial", req_id: reqId, variants } satisfies Output);
      if (i < max) setTimeout(tick, 35);
      else
        setTimeout(() => {
          this.emit("popup:output", {
            type: "final",
            req_id: reqId,
            drafts: drafts.map((text) => ({ text, ok: true, issues: [] })),
            subject: req.channel === "Email" ? "Late checkout request" : null,
          } satisfies Output);
        }, 120);
    };
    setTimeout(tick, 600);
  },
};
