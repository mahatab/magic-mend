// Settings window. Everything is saved to a local JSON file when you click Save.

import { invoke, inTauri, listen } from "./bridge";
import { TONES, LENGTHS, TOGGLES, CHANNELS } from "./popup-state";
import { comboFrom } from "./keys";
import logoUrl from "./assets/logo-64.png";

interface Rule {
  process: string[];
  title: string[];
  browser_only: boolean;
  channel: string;
}
interface Settings {
  hotkey: string;
  default_tone: string;
  default_length: string;
  default_toggles: string[];
  drafts: number;
  model: string;
  warm_mode: boolean;
  priming: boolean;
  theme: string;
  start_at_login: boolean;
  popup_position: string;
  excluded_apps: string[];
  channel_rules: Rule[];
  claude_path: string | null;
  no_contractions: boolean;
  no_em_dashes: boolean;
  paused: boolean;
  setup: unknown;
}
interface Info {
  settings: Settings;
  platform: string;
  version: string;
  hotkey_display: string;
  detected_claude: string | null;
  log_path: string | null;
}

const root = document.getElementById("root")!;
let info: Info;
let s: Settings;
let dirty = false;

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

function changed() {
  dirty = true;
  const st = document.getElementById("status");
  if (st) st.textContent = "You have unsaved changes.";
  const save = document.getElementById("save") as HTMLButtonElement | null;
  if (save) save.disabled = false;
}

function item(name: string, hint: string, control: Node, id?: string) {
  const label = id ? el("label", { for: id }, name) : el("span", { class: "name" }, name);
  if (hint) label.append(el("small", { text: hint }));
  return el("div", { class: "item" }, label, control);
}

function sw(get: () => boolean, set: (v: boolean) => void, label: string) {
  const b = el("button", { class: "switch", role: "switch", "aria-checked": String(get()), "aria-label": label });
  b.append(el("span", { class: "track", "aria-hidden": "true" }));
  b.addEventListener("click", () => {
    set(!get());
    b.setAttribute("aria-checked", String(get()));
    changed();
  });
  return b;
}

function chips(options: readonly string[], get: () => string, set: (v: string) => void, label: string) {
  const row = el("div", { class: "chips", role: "radiogroup", "aria-label": label });
  const paint = () => row.querySelectorAll<HTMLElement>(".chip").forEach((c) => c.setAttribute("aria-checked", String(c.dataset.value === get())));
  options.forEach((o) => {
    const c = el("button", { class: "chip", role: "radio", "data-value": o, text: o });
    c.addEventListener("click", () => {
      set(o);
      paint();
      changed();
    });
    row.append(c);
  });
  paint();
  return row;
}

function select(id: string, options: [string, string][], get: () => string, set: (v: string) => void) {
  const sel = el("select", { id }) as HTMLSelectElement;
  options.forEach(([v, t]) => sel.append(el("option", { value: v, text: t })));
  sel.value = get();
  sel.addEventListener("change", () => {
    set(sel.value);
    changed();
  });
  return sel;
}

function hotkeyControl() {
  const wrap = el("div");
  const box = el("div", { class: "keybox", tabindex: "0", role: "button", "aria-label": `${info.hotkey_display}, hotkey. Focus and press a new combination.`, text: info.hotkey_display });
  const msg = el("div", { class: "msg" });
  box.addEventListener("keydown", async (e) => {
    if (e.key === "Tab" || e.key === "Escape") return;
    e.preventDefault();
    const combo = comboFrom(e);
    if (!combo) return;
    const r = await invoke<{ ok: boolean; display: string; warnings: string[]; error: string | null }>("check_hotkey", { hotkeyStr: combo });
    box.textContent = r.display;
    box.setAttribute("aria-label", `${r.display}, hotkey. Focus and press a new combination.`);
    if (r.error) {
      msg.className = "msg bad";
      msg.textContent = r.error;
      return;
    }
    s.hotkey = combo;
    changed();
    msg.className = r.warnings.length ? "msg bad" : "msg good";
    msg.textContent = r.warnings.length ? r.warnings.join(" ") : `${r.display} is free to use.`;
  });
  wrap.append(box, msg);
  return wrap;
}

function toggleChips() {
  const row = el("div", { class: "chips", role: "group", "aria-label": "Default toggles" });
  TOGGLES.forEach((t) => {
    const c = el("button", { class: "chip", role: "checkbox", "aria-checked": String(s.default_toggles.includes(t)), text: t });
    c.addEventListener("click", () => {
      s.default_toggles = s.default_toggles.includes(t) ? s.default_toggles.filter((x) => x !== t) : [...s.default_toggles, t];
      c.setAttribute("aria-checked", String(s.default_toggles.includes(t)));
      changed();
    });
    row.append(c);
  });
  return row;
}

function rulesTable() {
  const wrap = el("div");
  const table = el("table", { class: "rules" });
  const paint = () => {
    table.replaceChildren(
      el("thead", {}, el("tr", {}, el("th", { text: "Process names" }), el("th", { text: "Window title words" }), el("th", { text: "Browser only" }), el("th", { text: "Channel" }), el("th", {}, el("span", { class: "sr-only", text: "Delete" })))),
    );
    const body = el("tbody");
    s.channel_rules.forEach((r, i) => {
      const proc = el("input", { type: "text", "aria-label": `Rule ${i + 1} process names`, value: r.process.join(", ") }) as HTMLInputElement;
      proc.addEventListener("input", () => {
        r.process = proc.value.split(",").map((x) => x.trim()).filter(Boolean);
        changed();
      });
      const title = el("input", { type: "text", "aria-label": `Rule ${i + 1} title words`, value: r.title.join(", ") }) as HTMLInputElement;
      title.addEventListener("input", () => {
        r.title = title.value.split(",").map((x) => x.trim()).filter(Boolean);
        changed();
      });
      const browser = el("input", { type: "checkbox", "aria-label": `Rule ${i + 1} browser only` }) as HTMLInputElement;
      browser.checked = r.browser_only;
      browser.addEventListener("change", () => {
        r.browser_only = browser.checked;
        changed();
      });
      const ch = select(`rule-${i}`, CHANNELS.map((c) => [c, c]), () => r.channel, (v) => (r.channel = v));
      ch.setAttribute("aria-label", `Rule ${i + 1} channel`);
      const del = el("button", { class: "icon-btn", "aria-label": `Delete rule ${i + 1}`, title: "Delete", text: "✕" });
      del.addEventListener("click", () => {
        s.channel_rules.splice(i, 1);
        changed();
        paint();
      });
      body.append(el("tr", {}, el("td", {}, proc), el("td", {}, title), el("td", {}, browser), el("td", {}, ch), el("td", {}, del)));
    });
    table.append(body);
  };
  paint();
  const add = el("button", { class: "btn btn-secondary", text: "Add rule" });
  add.addEventListener("click", () => {
    s.channel_rules.push({ process: [], title: [], browser_only: false, channel: "Text" });
    changed();
    paint();
  });
  wrap.append(table, el("div", { class: "row", style: "margin-top:8px" }, add, el("small", { text: "Process names are checked first, then window titles. Anything else is Text." })));
  return wrap;
}

function claudeControl() {
  const wrap = el("div");
  const input = el("input", { type: "text", id: "claude-path", placeholder: info.detected_claude ?? "Not found", value: s.claude_path ?? "" }) as HTMLInputElement;
  input.classList.add("grow");
  input.addEventListener("input", () => {
    s.claude_path = input.value.trim() || null;
    changed();
  });
  const browse = el("button", { class: "btn btn-secondary", text: "Browse..." });
  browse.addEventListener("click", async () => {
    if (!inTauri) return;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const p = await open({ multiple: false, directory: false, title: "Find the claude command" });
    if (typeof p === "string") {
      input.value = p;
      s.claude_path = p;
      changed();
    }
  });
  const msg = el("div", { class: "msg" });
  msg.textContent = info.detected_claude ? `Found: ${info.detected_claude}` : "Magic Mend cannot find the claude command.";
  msg.className = info.detected_claude ? "msg good" : "msg bad";
  wrap.append(el("div", { class: "row" }, input, browse), msg, el("small", { text: "Leave empty to find it on its own." }));
  return wrap;
}

function render() {
  root.replaceChildren();
  const scroll = el("div", { class: "scroll" });
  scroll.append(el("h1", {}, el("img", { src: logoUrl, alt: "" }), "Magic Mend"), el("p", { class: "sub", text: `Version ${info.version}. Settings are saved on this computer only.` }));

  const general = el("fieldset", {}, el("legend", { text: "General" }));
  general.append(
    item("Hotkey", "Click the box and press a new combination.", hotkeyControl()),
    item("Start at login", "", sw(() => s.start_at_login, (v) => (s.start_at_login = v), "Start at login")),
    item("Theme", "", select("theme", [["system", "Follow the system"], ["light", "Light"], ["dark", "Dark"]], () => s.theme, (v) => (s.theme = v)), "theme"),
    item("Popup position", "", select("pos", [["center", "Centered on the active window's screen"], ["cursor", "Near the mouse pointer"]], () => s.popup_position, (v) => (s.popup_position = v)), "pos"),
    item("Pause Magic Mend", "The hotkey does nothing while paused.", sw(() => s.paused, (v) => (s.paused = v), "Pause Magic Mend")),
  );

  const rewrites = el("fieldset", {}, el("legend", { text: "Rewrites" }));
  const drafts = el("input", { type: "number", id: "drafts", min: "1", max: "4", value: String(s.drafts) }) as HTMLInputElement;
  drafts.addEventListener("input", () => {
    s.drafts = Math.min(4, Math.max(1, Number(drafts.value) || 2));
    changed();
  });
  const model = el("input", { type: "text", id: "model", value: s.model, list: "models" }) as HTMLInputElement;
  model.addEventListener("input", () => {
    s.model = model.value.trim() || "haiku";
    changed();
  });
  const models = el("datalist", { id: "models" }, el("option", { value: "haiku" }), el("option", { value: "sonnet" }), el("option", { value: "opus" }));
  rewrites.append(
    item("Default tone", "", chips(TONES, () => s.default_tone, (v) => (s.default_tone = v), "Default tone")),
    item("Default length", "", chips(LENGTHS, () => s.default_length, (v) => (s.default_length = v), "Default length")),
    item("Default toggles", "", toggleChips()),
    item("Number of drafts", "Fix errors only always returns one.", drafts, "drafts"),
    item("Model", "A Claude model name or alias, such as haiku or sonnet.", el("div", {}, model, models), "model"),
    item("Warm mode", "Keeps one claude process ready so drafts start faster.", sw(() => s.warm_mode, (v) => (s.warm_mode = v), "Warm mode")),
  );

  const rules = el("fieldset", {}, el("legend", { text: "Output rules" }));
  rules.append(
    item("No contractions", "Writes \"do not\" instead of the short form.", sw(() => s.no_contractions, (v) => (s.no_contractions = v), "No contractions")),
    item("No em dashes", "Uses a comma or a period instead.", sw(() => s.no_em_dashes, (v) => (s.no_em_dashes = v), "No em dashes")),
  );

  const excluded = el("fieldset", {}, el("legend", { text: "Excluded apps" }));
  const ex = el("textarea", { id: "excluded", "aria-label": "Excluded apps, one per line" }) as HTMLTextAreaElement;
  ex.value = s.excluded_apps.join("\n");
  ex.addEventListener("input", () => {
    s.excluded_apps = ex.value.split("\n").map((x) => x.trim()).filter(Boolean);
    changed();
  });
  excluded.append(el("p", { class: "sub", style: "margin:8px 0", text: "The hotkey does nothing in these apps. Use the process name without .exe, one per line. Terminals are listed because a copy shortcut can stop a running program there." }), ex);

  const channels = el("fieldset", {}, el("legend", { text: "Channel rules" }));
  channels.append(rulesTable());

  const claude = el("fieldset", {}, el("legend", { text: "Claude" }));
  claude.append(item("Path to claude", "", claudeControl(), "claude-path"));

  const about = el("fieldset", { class: "about" }, el("legend", { text: "Privacy" }));
  about.append(
    el("p", { text: "Magic Mend sends your selected text only to the claude command on this computer, which sends it to Anthropic under your own plan. The app opens no network connections of its own." }),
    el("p", { text: "Messages and drafts stay in memory and are never written to disk. The log holds error types and timings only." }),
  );
  if (info.log_path) about.append(el("p", {}, "Log file: ", el("code", { text: info.log_path })));

  scroll.append(general, rewrites, rules, excluded, channels, claude, about);

  const bar = el("div", { class: "bar" });
  const status = el("span", { class: "status", id: "status", text: "" });
  const save = el("button", { class: "btn btn-primary", id: "save", text: "Save" }) as HTMLButtonElement;
  save.disabled = true;
  save.addEventListener("click", async () => {
    try {
      await invoke("save_settings", { settings: s });
      dirty = false;
      status.textContent = "Saved.";
      save.disabled = true;
      await load();
    } catch (e) {
      status.textContent = String(e);
    }
  });
  const close = el("button", { class: "btn btn-secondary", text: "Close" });
  close.addEventListener("click", () => void invoke("close_window"));
  bar.append(status, close, save);
  root.append(scroll, bar);
}

async function load() {
  info = await invoke<Info>("app_info");
  s = structuredClone(info.settings);
  if (s.theme === "light" || s.theme === "dark") document.documentElement.dataset.theme = s.theme;
  else delete document.documentElement.dataset.theme;
  const top = document.querySelector(".scroll")?.scrollTop ?? 0;
  render();
  const sc = document.querySelector(".scroll");
  if (sc) sc.scrollTop = top;
}

void listen("settings:changed", () => {
  if (!dirty) void load();
});
void load();
