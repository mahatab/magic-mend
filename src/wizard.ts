// First-run setup wizard. One step per screen. Connect Claude (and Allow access on Mac) must
// pass before the hotkey turns on; other steps can be skipped and then use the defaults.

import { invoke, inTauri, listen } from "./bridge";
import { TONES, LENGTHS } from "./popup-state";
import { comboFrom } from "./keys";
import logoUrl from "./assets/logo-64.png";

interface Info {
  settings: { hotkey: string; default_tone: string; default_length: string; start_at_login: boolean; theme: string; setup: { completed: string[] } };
  platform: string;
  hotkey_display: string;
  steps: string[];
  required: string[];
  first_unfinished: string;
  translucent: boolean;
}

const INSTALL_URL = "https://claude.com/product/claude-code";
const PRACTICE = "hi, i wanted to ask if you can send me the documnet by friday, i need it becuase my landlord is asking for it";

const shell = document.getElementById("shell")!;
let info: Info;
let steps: string[] = [];
let index = 0;
let connectOk = false;
let accessOk = false;
let tone = "Simple";
let length = "Same";
let startAtLogin = true;
let practiceDone = false;
let accessTimer: number | undefined;

function el<K extends keyof HTMLElementTagNameMap>(tag: K, attrs: Record<string, string> = {}, ...kids: (Node | string)[]) {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "class") e.className = v;
    else if (k === "text") e.textContent = v;
    else if (k === "html") e.innerHTML = v;
    else e.setAttribute(k, v);
  }
  e.append(...kids);
  return e;
}

function button(label: string, cls: string, onClick: () => void, disabled = false) {
  const b = el("button", { class: cls, text: label }) as HTMLButtonElement;
  b.disabled = disabled;
  b.addEventListener("click", onClick);
  return b;
}

const isMac = () => info?.platform === "macos";
const hotkeyLabel = () => (isMac() ? info.hotkey_display.replace("Alt", "Option") : info.hotkey_display);
const step = () => steps[index];
const isRequired = (s: string) => info.required.includes(s);

async function complete(s: string) {
  await invoke("wizard_complete", { step: s });
  if (!info.settings.setup.completed.includes(s)) info.settings.setup.completed.push(s);
}

function go(i: number) {
  index = Math.max(0, Math.min(steps.length - 1, i));
  window.clearInterval(accessTimer);
  void invoke("wizard_at", { step: step() });
  render();
}

async function next() {
  const s = step();
  if (s === "defaults") {
    await invoke("wizard_save_defaults", { tone, length, startAtLogin });
  }
  await complete(s);
  if (index < steps.length - 1) go(index + 1);
}

async function skip() {
  // Skipped steps keep their defaults and count as done.
  await complete(step());
  go(index + 1);
}

async function finish() {
  await complete("done");
  await invoke("wizard_finish");
  await invoke("close_window");
}

// ---------------------------------------------------------------------------------------------

function render() {
  shell.replaceChildren();
  const close = el("button", { class: "close", "aria-label": "Close setup", title: "Close" });
  close.innerHTML = `<svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true"><path d="M3 3l8 8M11 3l-8 8" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>`;
  close.addEventListener("click", () => void invoke("close_window"));
  const bar = el(
    "div",
    { class: "titlebar", "data-tauri-drag-region": "" },
    el("div", { class: "brand" }, el("img", { src: logoUrl, alt: "" }), "Magic Mend setup"),
    el("div", { class: "spacer", "data-tauri-drag-region": "" }),
    close,
  );
  const dots = el("div", { class: "dots", role: "progressbar", "aria-label": `Step ${index + 1} of ${steps.length}`, "aria-valuemin": "1", "aria-valuemax": String(steps.length), "aria-valuenow": String(index + 1) });
  steps.forEach((s, i) => dots.append(el("i", { class: i === index ? "on" : info.settings.setup.completed.includes(s) ? "done" : "" })));
  const body = el("section", { class: "step", "aria-live": "polite" });
  const footer = el("div", { class: "footer" });
  shell.append(bar, dots, body, footer);

  const back = button("Back", "btn btn-secondary", () => go(index - 1), index === 0);
  const s = step();
  const views: Record<string, () => void> = {
    welcome: () => welcome(body),
    connect: () => connect(body, footer),
    access: () => access(body),
    hotkey: () => hotkeyStep(body),
    defaults: () => defaults(body),
    try: () => tryIt(body),
    done: () => done(body),
  };
  views[s]?.();

  footer.prepend(back);
  footer.append(el("div", { class: "grow" }));
  if (!isRequired(s) && s !== "done" && s !== "welcome") footer.append(button("Skip", "link", () => void skip()));
  if (s === "done") footer.append(button("Finish", "btn btn-primary", () => void finish()));
  else {
    const blocked = (s === "connect" && !connectOk) || (s === "access" && !accessOk);
    const nextBtn = button(s === "try" && !practiceDone ? "Next" : "Next", "btn btn-primary", () => void next(), blocked);
    nextBtn.id = "next";
    footer.append(nextBtn);
  }
  (body.querySelector("[data-autofocus]") as HTMLElement | null)?.focus();
}

function welcome(body: HTMLElement) {
  body.append(
    el("img", { class: "hero", src: logoUrl, alt: "" }),
    el("h1", { text: "Welcome to Magic Mend" }),
    el("p", { class: "lead", text: "Select text in any app, press a hotkey, and get a clear, human rewrite you can drop back in with one click." }),
    el(
      "div",
      { class: "promise" },
      el("div", {}, el("span", { class: "ic", text: "1" }), el("div", {}, el("b", { text: "It never sends a message" }), el("span", { text: "Replace only puts the text in the box. You press Send yourself." }))),
      el("div", {}, el("span", { class: "ic", text: "2" }), el("div", {}, el("b", { text: "It uses your own Claude plan" }), el("span", { text: "Rewrites run through the claude command you already signed in to, and count toward your plan limits." }))),
      el("div", {}, el("span", { class: "ic", text: "3" }), el("div", {}, el("b", { text: "Your text stays private" }), el("span", { text: "Messages live in memory only. Nothing is saved, logged, or tracked." }))),
    ),
  );
}

function connect(body: HTMLElement, _footer: HTMLElement) {
  body.append(el("h1", { text: "Connect Claude" }), el("p", { text: "Magic Mend uses the official claude command on this computer. It never handles your login or reads your credentials." }));
  const checks = el("div", { class: "checks" });
  const row = (label: string) => {
    const r = el("div", { class: "check busy" }, el("span", { class: "mark", "aria-hidden": "true" }), el("div", { class: "txt" }, el("span", { text: label }), el("small")));
    checks.append(r);
    return {
      set(state: "ok" | "bad" | "busy" | "idle", detail = "") {
        r.className = `check ${state === "idle" ? "" : state}`;
        (r.querySelector(".mark") as HTMLElement).textContent = state === "ok" ? "✓" : state === "bad" ? "!" : "";
        (r.querySelector("small") as HTMLElement).textContent = detail;
      },
    };
  };
  const found = row("Find the claude command");
  const signed = row("Check that Claude is signed in");
  const test = row("Send a tiny test rewrite");
  const help = el("div");
  const actions = el("div", { class: "row" });
  body.append(checks, help, actions);

  const run = async () => {
    connectOk = false;
    (document.getElementById("next") as HTMLButtonElement | null)?.setAttribute("disabled", "");
    help.replaceChildren();
    actions.replaceChildren();
    found.set("busy");
    signed.set("idle");
    test.set("idle");
    const c = await invoke<{ found: boolean; path: string | null; version: string | null; signed_in: boolean | null; message: string | null }>("wizard_check_claude");
    if (!c.found || !c.version) {
      found.set("bad", c.path ?? "Not found");
      help.append(el("div", { class: "help", text: c.message ?? "Magic Mend cannot find the claude command." }));
      actions.append(
        button("Get Claude Code", "btn btn-secondary", () => void openUrl(INSTALL_URL)),
        button("Browse...", "btn btn-secondary", () => void browse(run)),
        button("Retry", "btn btn-primary", () => void run()),
      );
      return;
    }
    found.set("ok", `${c.version} at ${c.path}`);
    signed.set("busy");
    if (c.signed_in === false) {
      signed.set("bad", "Not signed in");
      help.append(signInHelp());
      actions.append(button("Retry", "btn btn-primary", () => void run()));
      return;
    }
    signed.set("ok", c.signed_in ? "Signed in" : "Checked by the test below");
    test.set("busy", "This takes a few seconds");
    const t = await invoke<{ ok: boolean; message: string; ms: number; sample: string | null }>("wizard_test_engine");
    if (!t.ok) {
      test.set("bad", t.message);
      if (/signed in/i.test(t.message)) help.append(signInHelp());
      else help.append(el("div", { class: "help", text: t.message }));
      actions.append(button("Retry", "btn btn-primary", () => void run()));
      return;
    }
    test.set("ok", t.sample ? `"${t.sample}"` : `Answered in ${(t.ms / 1000).toFixed(1)} s`);
    connectOk = true;
    await complete("connect");
    (document.getElementById("next") as HTMLButtonElement | null)?.removeAttribute("disabled");
  };
  void run();
}

function signInHelp() {
  const d = el("div", { class: "help" });
  d.innerHTML = "Claude is not signed in. Open a terminal, run <code>claude</code>, sign in, and then click Retry.";
  return d;
}

async function browse(after: () => Promise<void>) {
  if (!inTauri) return;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({ multiple: false, directory: false, title: "Find the claude command" });
  if (typeof picked === "string") {
    await invoke("set_claude_path", { path: picked });
    await after();
  }
}

async function openUrl(url: string) {
  if (!inTauri) return void window.open(url, "_blank");
  const { openUrl } = await import("@tauri-apps/plugin-opener");
  await openUrl(url);
}

function access(body: HTMLElement) {
  body.append(
    el("h1", { text: "Allow access" }),
    el("p", { class: "lead", text: "Magic Mend needs the Accessibility permission to copy your selection and paste the draft you pick." }),
    el("p", { text: "It uses this only when you press the hotkey or click Replace. Turn on Magic Mend in the list that opens." }),
  );
  const status = el("div", { class: "checks" });
  const actions = el("div", { class: "row" });
  body.append(status, actions);
  const show = (ok: boolean) => {
    status.replaceChildren(el("div", { class: `check ${ok ? "ok" : "busy"}` }, el("span", { class: "mark", text: ok ? "✓" : "" }), el("div", { class: "txt", text: ok ? "Access is on" : "Waiting for access..." })));
  };
  actions.append(
    button("Open System Settings", "btn btn-primary", () => void invoke("open_access_settings")),
    button("Restart Magic Mend", "btn btn-secondary", () => void invoke("restart_app")),
  );
  body.append(el("p", { text: "If access is on but this step still waits, restart Magic Mend so the permission applies." }));
  const poll = async () => {
    accessOk = await invoke<boolean>("access_status");
    show(accessOk);
    if (accessOk) {
      window.clearInterval(accessTimer);
      await complete("access");
      go(index + 1);
    }
  };
  void poll();
  accessTimer = window.setInterval(() => void poll(), 1000);
}

function hotkeyStep(body: HTMLElement) {
  body.append(el("h1", { text: "Choose your hotkey" }), el("p", { text: "Press this after you select text. Click the box and press a new combination to change it." }));
  const box = el("div", { class: "keybox", tabindex: "0", role: "button", "aria-label": `${hotkeyLabel()}, hotkey. Focus and press a new combination.`, "data-autofocus": "", text: hotkeyLabel() });
  const msg = el("div");
  const actions = el("div", { class: "row" });
  body.append(box, msg, actions);
  let pending: string | null = null;
  box.addEventListener("keydown", async (e) => {
    if (e.key === "Tab" || e.key === "Escape") return;
    e.preventDefault();
    const combo = comboFrom(e);
    if (!combo) return;
    const r = await invoke<{ ok: boolean; display: string; warnings: string[]; error: string | null }>("check_hotkey", { hotkeyStr: combo });
    box.textContent = r.display;
    box.setAttribute("aria-label", `${r.display}, hotkey. Focus and press a new combination.`);
    msg.replaceChildren();
    actions.replaceChildren();
    if (r.error) msg.append(el("div", { class: "warn", text: r.error }));
    r.warnings.forEach((w) => msg.append(el("div", { class: "warn", text: w })));
    if (r.ok) {
      pending = combo;
      if (!r.warnings.length) msg.append(el("div", { class: "okline", text: `${r.display} is free to use.` }));
      actions.append(
        button(`Use ${r.display}`, "btn btn-primary", async () => {
          if (!pending) return;
          try {
            const shown = await invoke<string>("set_hotkey", { hotkeyStr: pending });
            info.hotkey_display = shown;
            msg.replaceChildren(el("div", { class: "okline", text: `Your hotkey is now ${shown}.` }));
            actions.replaceChildren();
          } catch (err) {
            msg.replaceChildren(el("div", { class: "warn", text: String(err) }));
          }
        }),
      );
    }
  });
  const check = async () => {
    const r = await invoke<{ ok: boolean; warnings: string[] }>("check_hotkey", { hotkeyStr: info.settings.hotkey });
    r.warnings.forEach((w) => msg.append(el("div", { class: "warn", text: w })));
  };
  void check();
}

function chipRow(options: readonly string[], get: () => string, set: (v: string) => void, label: string) {
  const row = el("div", { class: "chips", role: "radiogroup", "aria-label": label });
  const paint = () => row.querySelectorAll<HTMLElement>(".chip").forEach((c) => c.setAttribute("aria-checked", String(c.dataset.value === get())));
  for (const o of options) {
    const c = el("button", { class: "chip", role: "radio", "data-value": o, text: o });
    c.addEventListener("click", () => {
      set(o);
      paint();
    });
    row.append(c);
  }
  paint();
  return row;
}

function defaults(body: HTMLElement) {
  body.append(el("h1", { text: "Pick your defaults" }), el("p", { text: "The popup starts with these. You can change them for any message." }));
  body.append(
    el("div", { class: "field" }, el("span", { class: "label", text: "Tone" }), chipRow(TONES, () => tone, (v) => (tone = v), "Default tone")),
    el("div", { class: "field" }, el("span", { class: "label", text: "Length" }), chipRow(LENGTHS, () => length, (v) => (length = v), "Default length")),
  );
  const sw = el("button", { class: "switch", role: "switch", "aria-checked": String(startAtLogin) });
  sw.append(el("span", { class: "track", "aria-hidden": "true" }), "Start Magic Mend at login");
  sw.addEventListener("click", () => {
    startAtLogin = !startAtLogin;
    sw.setAttribute("aria-checked", String(startAtLogin));
  });
  body.append(el("div", { class: "field" }, el("span", { class: "label", text: "Startup" }), sw));
}

function tryIt(body: HTMLElement) {
  body.append(el("h1", { text: "Try it" }));
  const p = el("p");
  p.innerHTML = `Select all the text in the box below, press <kbd>${hotkeyLabel()}</kbd>, pick a draft, and click <b>Replace</b>.`;
  const ta = el("textarea", { class: "practice", "aria-label": "Practice box", spellcheck: "false", "data-autofocus": "" }) as HTMLTextAreaElement;
  ta.value = PRACTICE;
  const status = el("div");
  body.append(p, ta, status);
  ta.addEventListener("focus", () => ta.select(), { once: true });
  ta.addEventListener("input", () => {
    if (ta.value.trim() && ta.value !== PRACTICE && !practiceDone) {
      practiceDone = true;
      status.replaceChildren(el("div", { class: "check ok" }, el("span", { class: "mark", text: "✓" }), el("div", { class: "txt", text: "It worked. The box now holds your new draft." })));
    }
  });
}

function done(body: HTMLElement) {
  const where = isMac() ? "the menu bar, at the top right of your screen" : "the system tray, near the clock (open the ^ arrow if you do not see it)";
  body.append(
    el("h1", { text: "You are all set" }),
    el("p", { class: "lead", text: `Select text anywhere and press ${hotkeyLabel()}.` }),
    el("div", { class: "trayart" }, el("img", { src: logoUrl, alt: "" }), el("div", {}, el("b", { text: "Magic Mend lives in " + (isMac() ? "the menu bar" : "the tray") }), el("p", { text: `Look for the needle icon in ${where}. Right-click it for Settings, Pause, and Quit.` }))),
  );
}

// ---------------------------------------------------------------------------------------------

async function start() {
  info = await invoke<Info>("app_info");
  if (info.settings.theme === "light" || info.settings.theme === "dark") document.documentElement.dataset.theme = info.settings.theme;
  tone = info.settings.default_tone;
  length = info.settings.default_length;
  startAtLogin = info.settings.start_at_login;
  steps = info.steps;
  connectOk = info.settings.setup.completed.includes("connect");
  accessOk = info.settings.setup.completed.includes("access");
  const translucent = inTauri && info.translucent;
  document.body.classList.toggle("translucent", translucent);
  document.body.classList.toggle("solid", !translucent);
  const want = new URLSearchParams(location.search).get("step") ?? info.first_unfinished;
  go(Math.max(0, steps.indexOf(want)));
}

void listen<string>("wizard:goto", (s) => go(Math.max(0, steps.indexOf(s))));
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") void invoke("close_window");
});
void start();
