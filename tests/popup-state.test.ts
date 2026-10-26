import { describe, it, expect } from "vitest";
import {
  fromPayload,
  beginRequest,
  applyOutput,
  replaceMode,
  shouldAutoStart,
  expectedDrafts,
  headerLine,
  canActOn,
  locked,
  pickChip,
  flipToggle,
  buildRequest,
  type OpenPayload,
} from "../src/popup-state";
import { comboFrom } from "../src/keys";

function payload(over: Partial<OpenPayload> = {}): OpenPayload {
  return {
    session_id: 1,
    capture: { kind: "text", text: "hi there" },
    target: { app_name: "Notepad", process: "notepad.exe", icon: null, elevated: false, own_window: false, present: true },
    channel: "Text",
    tone: "Clear",
    length: "Same",
    toggles: [],
    drafts: 2,
    long_text_limit: 4000,
    theme: "system",
    translucent: false,
    engine_error: null,
    no_contractions: true,
    no_em_dashes: true,
    ...over,
  };
}

describe("opening", () => {
  it("preselects defaults and starts at once", () => {
    const s = fromPayload(payload());
    expect(s.tone).toBe("Clear");
    expect(s.length).toBe("Same");
    expect(s.editable).toBe(false);
    expect(shouldAutoStart(s)).toBe(true);
  });

  it("nothing selected gives an editable box and no auto start", () => {
    const s = fromPayload(payload({ capture: { kind: "nothing" } }));
    expect(s.editable).toBe(true);
    expect(shouldAutoStart(s)).toBe(false);
    expect(replaceMode(s).enabled).toBe(false);
  });

  it("pending selection shows placeholders and sends nothing until the text arrives", () => {
    const s = fromPayload(payload({ capture: { kind: "pending" } }));
    expect(s.pending).toBe(true);
    expect(s.editable).toBe(false);
    expect(s.drafts).toHaveLength(2);
    expect(s.drafts.every((d) => d.status === "waiting" && !d.text)).toBe(true);
    expect(shouldAutoStart(s)).toBe(false);
    expect(s.drafts.some((d) => canActOn(d))).toBe(false);
    const filled = fromPayload(payload({ capture: { kind: "text", text: "hello there" } }));
    expect(filled.pending).toBe(false);
    expect(shouldAutoStart(filled)).toBe(true);
  });

  it("unreadable selection shows one plain line", () => {
    const s = fromPayload(payload({ capture: { kind: "unreadable", reason: "The selection is not text." } }));
    expect(s.notice).toBe("The selection is not text.");
    expect(s.editable).toBe(true);
  });

  it("long text needs a yes first", () => {
    const s = fromPayload(payload({ capture: { kind: "text", text: "x".repeat(4001) } }));
    expect(s.needsLongConfirm).toBe(true);
    expect(shouldAutoStart(s)).toBe(false);
  });

  it("warns when the clipboard cannot be fully restored", () => {
    const s = fromPayload(payload({ clipboard_warning: true }));
    expect(s.notice).toContain("cannot fully restore");
  });

  it("engine error at open is shown and blocks auto start", () => {
    const s = fromPayload(payload({ engine_error: "Claude is not signed in." }));
    expect(s.error?.message).toBe("Claude is not signed in.");
    expect(shouldAutoStart(s)).toBe(false);
  });
});

describe("requests", () => {
  it("matches the spec request shape", () => {
    const s = fromPayload(payload({ toggles: ["Softer"] }));
    const r = beginRequest(s);
    expect(r).toEqual({ text: "hi there", tone: "Clear", length: "Same", toggles: ["Softer"], channel: "Text", followup: null });
    expect(s.reqId).toBe(1);
    expect(s.drafts).toHaveLength(2);
    expect(s.drafts.every((d) => d.status === "waiting")).toBe(true);
  });

  it("proofread expects one draft", () => {
    const s = fromPayload(payload({ toggles: ["Proofread"] }));
    expect(expectedDrafts(s)).toBe(1);
    beginRequest(s);
    expect(s.drafts).toHaveLength(1);
  });

  it("proofread locks tone, length, and the other toggles", () => {
    const s = fromPayload(payload({ toggles: ["Proofread"] }));
    expect(locked(s)).toBe(true);
    expect(locked(s, "Softer")).toBe(true);
    expect(locked(s, "Proofread")).toBe(false);
    expect(pickChip(s, "tone", "Warm")).toBe(false);
    expect(pickChip(s, "length", "Shorter")).toBe(false);
    expect(flipToggle(s, "Softer")).toBe(false);
    expect([s.tone, s.length, s.toggles]).toEqual(["Clear", "Same", ["Proofread"]]);
  });

  it("proofread sends no other toggles and turning it off restores the picks", () => {
    const s = fromPayload(payload({ toggles: ["Softer"] }));
    expect(pickChip(s, "tone", "Warm")).toBe(true);
    expect(pickChip(s, "length", "Shorter")).toBe(true);
    expect(flipToggle(s, "Proofread")).toBe(true);
    expect(buildRequest(s).toggles).toEqual(["Proofread"]);
    expect(flipToggle(s, "Proofread")).toBe(true);
    expect(locked(s)).toBe(false);
    const r = buildRequest(s);
    expect([r.tone, r.length, r.toggles]).toEqual(["Warm", "Shorter", ["Softer"]]);
  });

  it("follow-ups keep the old text while new drafts stream", () => {
    const s = fromPayload(payload());
    beginRequest(s);
    applyOutput(s, { type: "final", req_id: 1, drafts: [{ text: "A", ok: true, issues: [] }, { text: "B", ok: true, issues: [] }], subject: null });
    const r = beginRequest(s, "remove the apology");
    expect(r.followup).toBe("remove the apology");
    expect(s.drafts.map((d) => d.text)).toEqual(["A", "B"]);
  });

  it("sends a draft count only when it is not two", () => {
    const s = fromPayload(payload({ drafts: 3 }));
    expect(beginRequest(s).drafts).toBe(3);
  });
});

describe("outputs", () => {
  it("ignores replies to older requests", () => {
    const s = fromPayload(payload());
    beginRequest(s);
    beginRequest(s);
    expect(applyOutput(s, { type: "partial", req_id: 1, variants: ["old"] })).toBe(false);
    expect(applyOutput(s, { type: "partial", req_id: 2, variants: ["new"] })).toBe(true);
    expect(s.drafts[0].text).toBe("new");
    expect(s.drafts[0].status).toBe("streaming");
  });

  it("streaming drafts cannot be replaced or copied until checked", () => {
    const s = fromPayload(payload());
    beginRequest(s);
    applyOutput(s, { type: "partial", req_id: 1, variants: ["Hello there"] });
    expect(canActOn(s.drafts[0])).toBe(false);
    applyOutput(s, { type: "checking", req_id: 1 });
    expect(canActOn(s.drafts[0])).toBe(false);
    applyOutput(s, { type: "final", req_id: 1, drafts: [{ text: "Hello there", ok: true, issues: [] }, { text: "I don't", ok: false, issues: ["don't"] }], subject: null });
    expect(canActOn(s.drafts[0])).toBe(true);
    expect(canActOn(s.drafts[1])).toBe(false);
    expect(s.drafts[1].status).toBe("blocked");
  });

  it("errors keep only ready drafts", () => {
    const s = fromPayload(payload());
    beginRequest(s);
    applyOutput(s, { type: "error", req_id: 1, kind: "timeout", message: "Claude took longer than 30 seconds. Click Retry." });
    expect(s.error?.kind).toBe("timeout");
    expect(s.drafts).toHaveLength(0);
    expect(s.busy).toBe(false);
  });

  it("email subject is kept", () => {
    const s = fromPayload(payload({ channel: "Email" }));
    beginRequest(s);
    applyOutput(s, { type: "final", req_id: 1, drafts: [{ text: "Hi,\n\nThanks.\n\nBest,", ok: true, issues: [] }], subject: "Thanks" });
    expect(s.subject).toBe("Thanks");
  });
});

describe("replace button", () => {
  it("normal, paste at cursor, window lost, elevated, no window", () => {
    const s = fromPayload(payload());
    expect(replaceMode(s)).toEqual({ label: "Replace", enabled: true, reason: null });
    s.targetFocused = true;
    expect(replaceMode(s).label).toBe("Paste at cursor");
    expect(headerLine(s)).toContain("may have changed");
    s.targetFocused = false;
    s.targetAlive = false;
    expect(replaceMode(s)).toMatchObject({ label: "Copy only", enabled: false });
    const e = fromPayload(payload({ target: { ...payload().target, elevated: true }, capture: { kind: "unreadable", reason: "admin" } }));
    expect(replaceMode(e).enabled).toBe(false);
    expect(replaceMode(e).reason).toContain("administrator");
    const n = fromPayload(payload({ target: { ...payload().target, present: false } }));
    expect(replaceMode(n).enabled).toBe(false);
    expect(headerLine(n)).toBe("No window to paste into");
  });

  it("header names the target app", () => {
    expect(headerLine(fromPayload(payload()))).toBe("Replace will paste into: Notepad");
  });
});

describe("hotkey capture", () => {
  const k = (code: string, mods: Partial<Record<"ctrlKey" | "altKey" | "shiftKey" | "metaKey", boolean>> = {}) =>
    comboFrom({ ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, code, ...mods });
  it("reads physical keys", () => {
    expect(k("KeyR", { ctrlKey: true, altKey: true })).toBe("Ctrl+Alt+R");
    expect(k("Space", { ctrlKey: true, shiftKey: true })).toBe("Ctrl+Shift+Space");
    expect(k("F9", { metaKey: true })).toBe("Cmd+F9");
    expect(k("Digit7", { ctrlKey: true })).toBe("Ctrl+7");
    expect(k("ControlLeft", { ctrlKey: true })).toBeNull();
    expect(k("Enter", { ctrlKey: true })).toBeNull();
  });
});
