// The popup UI. Replace and Copy are mouse buttons only: keyboard activation is ignored.
// Enter in the follow-up box submits inside the popup and never reaches another app.

import { invoke, listen, inTauri, mock } from "./bridge";
import {
  TONES,
  LENGTHS,
  TOGGLES,
  CHANNELS,
  type OpenPayload,
  type Output,
  type PopupState,
  initialState,
  fromPayload,
  beginRequest,
  applyOutput,
  shouldAutoStart,
  replaceMode,
  canActOn,
  headerLine,
  expectedDrafts,
} from "./popup-state";
import logoUrl from "./assets/logo-64.png";

const CHIP_DEBOUNCE_MS = 300;
const app = document.getElementById("app")!;
let state: PopupState = initialState();
let debounce: number | undefined;
let resizeRaf = 0;

// ---------------------------------------------------------------------------------------------
// Helpers

function el<K extends keyof HTMLElementTagNameMap>(tag: K, attrs: Record<string, string> = {}, ...kids: (Node | string)[]) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "class") e.className = v;
    else if (k === "text") e.textContent = v;
    else e.setAttribute(k, v);
  }
  e.append(...kids);
  return e;
}

const icons = {
  close: `<svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true"><path d="M3 3l8 8M11 3l-8 8" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>`,
  send: `<svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true"><path d="M3 8h9M8.5 4l4 4-4 4" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>`,
};

/** Accepts only real mouse or touch clicks. Keyboard activation (Enter or Space on a focused
 *  button) produces a click with detail 0, which is ignored. */
function mouseOnly(btn: HTMLButtonElement, handler: () => void) {
  btn.addEventListener("click", (e) => {
    const pe = e as PointerEvent;
    if (e.detail === 0 || (pe.pointerType !== undefined && pe.pointerType === "")) {
      e.preventDefault();
      return;
    }
    handler();
  });
  btn.addEventListener("keydown", (e) => {
    if (e.key === "Enter" || e.key === " ") e.preventDefault();
  });
}

function flash(btn: HTMLButtonElement, label: string, done = 1000) {
  const old = btn.dataset.label ?? btn.textContent ?? "";
  btn.dataset.label = old;
  btn.textContent = label;
  window.setTimeout(() => {
    if (btn.isConnected) btn.textContent = btn.dataset.label ?? old;
  }, done);
}

function applyTheme(theme: string) {
  if (theme === "light" || theme === "dark") document.documentElement.dataset.theme = theme;
  else delete document.documentElement.dataset.theme;
}

// ---------------------------------------------------------------------------------------------
// Rendering

let refs: {
  target: HTMLElement;
  original: HTMLElement;
  controls: HTMLElement;
  channelHint: HTMLElement;
  drafts: HTMLElement;
  followup: HTMLTextAreaElement;
  send: HTMLButtonElement;
} | null = null;

function render() {
  app.replaceChildren();
  const s = state;

  // Header: brand, where Replace will paste, close.
  const target = el("div", { class: "target" });
  const header = el(
    "header",
    {},
    el("div", { class: "brand" }, el("img", { src: logoUrl, alt: "" }), "Magic Mend"),
    target,
  );
  // Esc closes too, so the close button stays out of the Tab order (chips come first).
  const close = el("button", { class: "close", "aria-label": "Close Magic Mend", title: "Close (Esc)", tabindex: "-1" });
  close.innerHTML = icons.close;
  close.addEventListener("click", closePopup);
  header.append(close);

  // Original text.
  const original = el("section", { class: "section original", "aria-label": "Original text" });

  // Controls.
  const controls = el("section", { class: "section controls", "aria-label": "Rewrite options" });
  controls.append(el("span", { class: "label", id: "tone-label", text: "Tone" }));
  controls.append(chipGroup("tone-label", TONES, () => state.tone, (v) => setChip("tone", v), "tone-row"));
  controls.append(el("span", { class: "label", id: "length-label", text: "Length" }));
  controls.append(chipGroup("length-label", LENGTHS, () => state.length, (v) => setChip("length", v), "length-row"));
  const toggles = el("div", { class: "toggles", role: "group", "aria-label": "Toggles" });
  for (const t of TOGGLES) {
    const sw = el("button", { class: "switch", role: "switch", "aria-checked": String(s.toggles.includes(t)), "data-toggle": t });
    sw.append(el("span", { class: "track", "aria-hidden": "true" }), t === "Less thankful" ? "Less thankful" : t);
    sw.addEventListener("click", () => toggle(t));
    toggles.append(sw);
  }
  controls.append(toggles);
  controls.append(el("span", { class: "label", id: "channel-label", text: "Channel" }));
  const select = el("select", { "aria-labelledby": "channel-label" }) as HTMLSelectElement;
  for (const c of CHANNELS) select.append(el("option", { value: c, text: c }));
  select.value = s.channel;
  select.addEventListener("change", () => {
    state.channel = select.value;
    state.channelAuto = false;
    updateControls();
    scheduleRequest(0);
  });
  const channelHint = el("span", { class: "hint" });
  controls.append(el("div", { class: "channel-row" }, select, channelHint));

  // Drafts.
  const drafts = el("section", { class: "drafts", "aria-label": "Drafts" });
  const live = el("div", { class: "sr-only", role: "status", "aria-live": "polite", id: "live" });

  // Follow-up.
  const followup = el("textarea", { rows: "1", placeholder: "Ask for a change...", "aria-label": "Ask for a change" }) as HTMLTextAreaElement;
  const send = el("button", { class: "send", "aria-label": "Send change request", title: "Send" }) as HTMLButtonElement;
  send.innerHTML = icons.send;
  followup.addEventListener("keydown", (e) => {
    // Enter submits inside the popup only. Shift+Enter adds a new line.
    if (!document.hasFocus()) {
      e.preventDefault();
      return;
    }
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      e.stopPropagation();
      submitFollowup();
    }
  });
  followup.addEventListener("input", () => {
    followup.style.height = "auto";
    followup.style.height = Math.min(followup.scrollHeight, 96) + "px";
    updateFollowup();
  });
  send.addEventListener("click", submitFollowup);
  const footer = el("section", { class: "followup" }, followup, send);

  app.append(el("h1", { class: "sr-only", text: "Magic Mend" }), header, original, controls, drafts, footer, live);
  refs = { target, original, controls, channelHint, drafts, followup, send };
  renderTarget();
  renderOriginal();
  updateControls();
  renderDrafts();
  updateFollowup();
}

function chipGroup(labelId: string, options: readonly string[], get: () => string, set: (v: string) => void, cls: string) {
  const group = el("div", { class: `chips ${cls}`, role: "radiogroup", "aria-labelledby": labelId });
  for (const o of options) {
    const chip = el("button", { class: "chip", role: "radio", "aria-checked": String(get() === o), "data-value": o, text: o });
    chip.addEventListener("click", () => set(o));
    chip.addEventListener("keydown", (e) => {
      // Arrow keys move between chips, like a radio group.
      if (e.key !== "ArrowRight" && e.key !== "ArrowLeft") return;
      e.preventDefault();
      const i = options.indexOf(get());
      const next = options[(i + (e.key === "ArrowRight" ? 1 : options.length - 1)) % options.length];
      set(next);
      (group.querySelector(`[data-value="${next}"]`) as HTMLElement | null)?.focus();
    });
    group.append(chip);
  }
  return group;
}

function renderTarget() {
  if (!refs) return;
  const t = state.payload?.target;
  refs.target.replaceChildren();
  refs.target.classList.toggle("warn", state.targetFocused || !state.targetAlive);
  if (t?.icon) refs.target.append(el("img", { src: t.icon, alt: "" }));
  else if (t?.present) refs.target.append(el("span", { class: "letter", "aria-hidden": "true", text: (t.app_name || "?").slice(0, 1).toUpperCase() }));
  const line = headerLine(state);
  const span = el("span", { title: line });
  const m = /^(Replace will paste into: )(.*)$/.exec(line);
  if (m) span.append(m[1], el("strong", { text: m[2] }));
  else span.textContent = line;
  refs.target.append(span);
}

function renderOriginal() {
  if (!refs) return;
  const o = refs.original;
  o.replaceChildren();
  if (!state.editable) {
    o.append(el("span", { class: "label", text: "Original" }), el("div", { class: "text", text: state.text }));
  } else {
    const id = "orig-input";
    o.append(el("label", { class: "label", for: id, text: "Paste or type text" }));
    const ta = el("textarea", { id, placeholder: "Paste or type text" }) as HTMLTextAreaElement;
    ta.value = state.text;
    ta.addEventListener("input", () => {
      state.text = ta.value;
      updateFollowup();
      scheduleRequest(800);
    });
    o.append(ta);
  }
  if (state.notice) o.append(el("div", { class: "notice", text: state.notice }));
}

function updateControls() {
  if (!refs) return;
  const c = refs.controls;
  c.querySelectorAll<HTMLElement>(".tone-row .chip").forEach((b) => b.setAttribute("aria-checked", String(b.dataset.value === state.tone)));
  c.querySelectorAll<HTMLElement>(".length-row .chip").forEach((b) => b.setAttribute("aria-checked", String(b.dataset.value === state.length)));
  c.querySelectorAll<HTMLElement>(".switch").forEach((b) => b.setAttribute("aria-checked", String(state.toggles.includes(b.dataset.toggle!))));
  c.classList.toggle("fixing", state.toggles.includes("Fix errors only"));
  const sel = c.querySelector("select") as HTMLSelectElement;
  if (sel) sel.value = state.channel;
  refs.channelHint.textContent = state.channelAuto && state.payload?.target.present ? "(auto-detected)" : "";
}

function updateFollowup() {
  if (!refs) return;
  const hasDrafts = state.drafts.some((d) => d.text);
  refs.send.disabled = !refs.followup.value.trim() || !state.text.trim() || !hasDrafts;
}

/** Renders the draft cards. Streaming text is updated in place so new words fade in. */
function renderDrafts() {
  if (!refs) return;
  const box = refs.drafts;
  const mode = replaceMode(state);
  const existing = new Map<string, HTMLElement>();
  box.querySelectorAll<HTMLElement>(".card").forEach((c) => existing.set(c.dataset.index!, c));
  box.replaceChildren();

  if (state.needsLongConfirm) {
    const yes = el("button", { class: "btn btn-secondary", text: "Send anyway" }) as HTMLButtonElement;
    yes.addEventListener("click", () => {
      state.needsLongConfirm = false;
      startRequest();
    });
    box.append(
      el(
        "div",
        { class: "banner info", role: "alert" },
        el("p", { text: `Magic Mend is built for short messages. This text has ${state.text.length.toLocaleString()} characters. Send it to Claude anyway?` }),
        yes,
      ),
    );
  }

  if (state.subject) {
    const copy = el("button", { class: "btn btn-secondary", text: "Copy", "aria-label": "Copy subject line" }) as HTMLButtonElement;
    mouseOnly(copy, async () => {
      if (await invoke<boolean>("copy_text", { text: state.subject, check: true })) flash(copy, "Copied");
    });
    box.append(el("div", { class: "subject" }, el("span", { class: "label", text: "Subject" }), el("span", { class: "value", text: state.subject, title: state.subject }), copy));
  }

  state.drafts.forEach((d, i) => {
    const card = el("article", { class: "card", "data-index": String(i), "aria-label": `Draft ${String.fromCharCode(65 + i)}` });
    const body = el("div", { class: "body" });
    const old = existing.get(String(i))?.querySelector(".body") as HTMLElement | undefined;
    if (!d.text && (d.status === "waiting" || d.status === "streaming")) {
      body.append(el("div", { class: "skeleton", "aria-label": "Writing draft" }, el("i"), el("i"), el("i")));
    } else if (old && old.dataset.text && d.text.startsWith(old.dataset.text) && d.text !== old.dataset.text) {
      // Keep what was shown and fade in only the new words.
      body.append(...Array.from(old.childNodes));
      body.append(el("span", { class: "w", text: d.text.slice(old.dataset.text.length) }));
    } else if (old && old.dataset.text === d.text) {
      body.append(...Array.from(old.childNodes));
    } else {
      body.textContent = d.text;
    }
    body.dataset.text = d.text;
    card.append(body);

    const foot = el("div", { class: "foot" });
    const status = el("span", { class: "status" });
    if (d.status === "checking") status.textContent = "Checking the wording...";
    else if (d.status === "streaming") status.textContent = "Writing...";
    else if (d.status === "blocked") {
      status.textContent = `Still has contractions (${d.issues.join(", ")}). Ask for a change or try again.`;
      status.classList.add("bad");
    }
    foot.append(status);
    const actOk = canActOn(d);
    const copy = el("button", { class: "btn btn-secondary", text: "Copy", "aria-label": `Copy draft ${String.fromCharCode(65 + i)}`, "aria-description": "Mouse only" }) as HTMLButtonElement;
    copy.disabled = !actOk;
    mouseOnly(copy, () => doCopy(d.text, copy));
    const replace = el("button", {
      class: "btn btn-primary",
      text: mode.label,
      "aria-label": `${mode.label} with draft ${String.fromCharCode(65 + i)}`,
      "aria-description": "Mouse only. Pastes into the original app. You press Send yourself.",
    }) as HTMLButtonElement;
    replace.disabled = !actOk || !mode.enabled;
    mouseOnly(replace, () => doReplace(d.text, replace));
    foot.append(copy, replace);
    card.append(foot);
    box.append(card);
  });

  if (mode.reason && state.drafts.length) box.append(el("div", { class: "reason", text: mode.reason }));

  if (state.error) {
    const retry = el("button", { class: "btn btn-secondary", text: "Retry" }) as HTMLButtonElement;
    retry.addEventListener("click", () => startRequest(true));
    box.append(el("div", { class: "banner error", role: "alert" }, el("p", { text: state.error.message }), retry));
  }
  updateFollowup();
  scheduleResize();
}

// ---------------------------------------------------------------------------------------------
// Actions

function setChip(kind: "tone" | "length", value: string) {
  if (state[kind] === value) return;
  state[kind] = value;
  updateControls();
  scheduleRequest(CHIP_DEBOUNCE_MS);
}

function toggle(t: string) {
  state.toggles = state.toggles.includes(t) ? state.toggles.filter((x) => x !== t) : [...state.toggles, t];
  updateControls();
  scheduleRequest(CHIP_DEBOUNCE_MS);
}

/** Waits for chip changes to settle before sending, so only the last choice is drafted. */
function scheduleRequest(delay: number) {
  window.clearTimeout(debounce);
  if (!state.text.trim() || state.needsLongConfirm) return;
  refs?.drafts.classList.add("fading");
  debounce = window.setTimeout(() => startRequest(), delay);
}

function startRequest(retry = false) {
  window.clearTimeout(debounce);
  if (!state.text.trim()) return;
  const req = beginRequest(state);
  refs?.drafts.classList.remove("fading");
  renderDrafts();
  void invoke(retry ? "retry_drafts" : "request_drafts", { reqId: state.reqId, request: req });
}

function submitFollowup() {
  if (!refs) return;
  const text = refs.followup.value.trim();
  if (!text || !state.text.trim() || !state.drafts.some((d) => d.text)) return;
  refs.followup.value = "";
  refs.followup.style.height = "auto";
  const req = beginRequest(state, text);
  renderDrafts();
  void invoke("request_drafts", { reqId: state.reqId, request: req });
}

async function doCopy(text: string, btn: HTMLButtonElement) {
  const ok = await invoke<boolean>("copy_text", { text, check: true });
  if (!ok) return;
  flash(btn, "Copied");
  // The draft stays on the clipboard. The session ends so the next message starts clean.
  window.setTimeout(closePopup, 900);
}

async function doReplace(text: string, btn: HTMLButtonElement) {
  btn.disabled = true;
  const r = await invoke<{ result: string; reason?: string }>("replace_draft", { text });
  if (r.result === "pasted") {
    flash(btn, "Replaced");
    return;
  }
  btn.disabled = false;
  if (r.result === "window_lost") {
    state.targetAlive = false;
    state.notice = "The original window was lost, so the draft was copied instead. Paste it where you need it.";
  } else if (r.result === "elevated") {
    state.notice = "That app runs as administrator, so the draft was copied instead. Paste it with Ctrl+V.";
  } else {
    state.notice = r.reason ?? "Replace did not work. Use Copy.";
  }
  renderTarget();
  renderOriginal();
  renderDrafts();
}

function closePopup() {
  window.clearTimeout(debounce);
  state = initialState();
  app.classList.remove("open");
  void invoke("close_popup");
}

// ---------------------------------------------------------------------------------------------
// Sizing: the window follows the content, up to 640 px.

function scheduleResize() {
  cancelAnimationFrame(resizeRaf);
  resizeRaf = requestAnimationFrame(() => {
    const h = Math.ceil(app.getBoundingClientRect().height);
    if (h > 0) void invoke("resize_popup", { height: h });
  });
}
new ResizeObserver(scheduleResize).observe(app);

// ---------------------------------------------------------------------------------------------
// Events

function open(p: OpenPayload) {
  window.clearTimeout(debounce);
  state = fromPayload(p);
  applyTheme(p.theme);
  document.body.classList.toggle("translucent", p.translucent);
  document.body.classList.toggle("solid", !p.translucent);
  render();
  app.classList.remove("open");
  void app.offsetWidth; // restart the open animation
  app.classList.add("open");
  // Keyboard focus lands on the first chip, so Esc and Tab work at once.
  const first = (state.editable ? app.querySelector("#orig-input") : app.querySelector(".tone-row .chip[aria-checked='true']")) as HTMLElement | null;
  (first ?? app).focus();
  if (shouldAutoStart(state)) startRequest();
}

document.addEventListener("keydown", (e) => {
  if (!document.hasFocus()) {
    e.preventDefault();
    return;
  }
  if (e.key === "Escape") {
    e.preventDefault();
    closePopup();
  }
});

void listen<OpenPayload>("popup:open", open);
void listen<Output>("popup:output", (o) => {
  if (!applyOutput(state, o)) return;
  renderDrafts();
  const live = document.getElementById("live");
  if (live && o.type === "final") live.textContent = o.drafts.length === 1 ? "One draft is ready." : `${o.drafts.length} drafts are ready.`;
  if (live && o.type === "error") live.textContent = o.message;
});
void listen("popup:target-focused", () => {
  state.targetFocused = true;
  renderTarget();
  renderDrafts();
});
void listen("popup:closed", () => {
  state = initialState();
  app.classList.remove("open");
});

// Browser preview: open with fake data (append ?mock, plus &empty, &email, &long, &error).
if (!inTauri) {
  const q = new URLSearchParams(location.search);
  const over: Partial<OpenPayload> = {};
  if (q.has("empty")) over.capture = { kind: "nothing" };
  if (q.has("email")) over.channel = "Email";
  if (q.has("long")) over.capture = { kind: "text", text: "word ".repeat(900) };
  if (q.has("error")) over.engine_error = "Claude is not signed in. Open a terminal, run claude, sign in, and then click Retry.";
  if (q.has("dark")) over.theme = "dark";
  if (q.has("fix")) over.toggles = ["Fix errors only"];
  (window as unknown as { __mm: unknown }).__mm = { mock, state: () => state, expectedDrafts: () => expectedDrafts(state) };
  open(mock.payload(over));
}
