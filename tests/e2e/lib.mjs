// Helpers for the Windows end-to-end tests: the test window, the clipboard, the app under
// test (with its own data folder and the fake claude), and a small DevTools client that
// clicks inside the popup with real mouse events.

import { spawn, execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync, readFileSync, existsSync, rmSync, readdirSync, statSync } from "node:fs";
import { join, resolve, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { tmpdir } from "node:os";

const here = dirname(fileURLToPath(import.meta.url));
export const ROOT = resolve(here, "..", "..");
export const BIN = join(ROOT, "src-tauri", "target", "debug");
export const APP = join(BIN, "magic-mend.exe");
export const PAD = join(BIN, "mm-testpad.exe");
export const FAKE = join(BIN, "mm-fakeclaude.exe");
export const FAKE_CODEX = join(BIN, "mm-fakecodex.exe");
export const CDP_PORT = 9333;

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export async function waitFor(fn, { timeout = 8000, interval = 50, what = "condition" } = {}) {
  const end = Date.now() + timeout;
  let last;
  while (Date.now() < end) {
    try {
      last = await fn();
      if (last) return last;
    } catch (e) {
      last = e;
    }
    await sleep(interval);
  }
  throw new Error(`timed out waiting for ${what} (last: ${last instanceof Error ? last.message : JSON.stringify(last)})`);
}

export function pad(...args) {
  return execFileSync(PAD, args, { encoding: "utf8" }).trim();
}

/** Windows the tests own. Keys are only ever sent while one of them is in front, so a test can
 *  never type into an app the person at the computer is using. */
const OWN_TITLES = [/^MM Testpad$/, /^Magic Mend$/, /^Magic Mend setup$/, /^MM-TERM-/, /^Administrator: MM-TERM-/];
export function assertOwnForeground(what) {
  const f = JSON.parse(execFileSync(PAD, ["fg"], { encoding: "utf8" }).trim() || "null");
  if (!f || !OWN_TITLES.some((re) => re.test(f.title ?? ""))) {
    throw new Error(`refusing to send ${what}: "${f?.title ?? "nothing"}" (${f?.process ?? "?"}) is in front, not a test window`);
  }
}
export const keys = (combo, hold = 20) => {
  assertOwnForeground(combo);
  return pad("keys", combo, String(hold));
};
export const clipSet = (text) => pad("clip-set", text);
export const clipGet = () => JSON.parse(pad("clip-get"));
export const clipSeq = () => Number(pad("clip-seq"));
export const fg = () => JSON.parse(pad("fg") || "null");
export const hotkey = (hold = 30) => keys("ctrl+alt+r", hold);

/** Starts the test window with the given text, fully selected. */
export async function startPad(text, name = "pad") {
  const dir = join(tmpdir(), "mm-e2e", name + "-" + Date.now());
  mkdirSync(dir, { recursive: true });
  const proc = spawn(PAD, ["window", dir, text], { stdio: ["ignore", "pipe", "inherit"] });
  const hwnd = await new Promise((res, rej) => {
    proc.stdout.on("data", (d) => {
      const m = /ready (\d+)/.exec(String(d));
      if (m) res(Number(m[1]));
    });
    proc.on("exit", () => rej(new Error("testpad exited")));
  });
  const p = {
    proc,
    dir,
    hwnd,
    text: () => (existsSync(join(dir, "text.txt")) ? readFileSync(join(dir, "text.txt"), "utf8") : null),
    keys: () => (existsSync(join(dir, "keys.log")) ? readFileSync(join(dir, "keys.log"), "utf8").split("\n").filter(Boolean) : []),
    focus: () => pad("focus", "MM Testpad"),
    close: () => {
      try {
        proc.kill();
      } catch {}
    },
  };
  await waitFor(() => p.text() === text.replace(/\r\n/g, "\n"), { what: "testpad text" });
  await waitFor(() => fg()?.title === "MM Testpad", { what: "testpad in front", timeout: 4000 }).catch(() => p.focus());
  return p;
}

// ---------------------------------------------------------------------------------------------
// DevTools protocol client.

export class Cdp {
  constructor(ws) {
    this.ws = ws;
    this.id = 0;
    this.pending = new Map();
    ws.addEventListener("message", (m) => {
      const msg = JSON.parse(m.data);
      if (msg.id && this.pending.has(msg.id)) {
        const { res, rej } = this.pending.get(msg.id);
        this.pending.delete(msg.id);
        msg.error ? rej(new Error(msg.error.message)) : res(msg.result);
      }
    });
  }
  static async connect(match, port = CDP_PORT, timeout = 15000) {
    const target = await waitFor(
      async () => {
        const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
        return list.find((t) => t.type === "page" && match(t.url));
      },
      { timeout, interval: 200, what: "devtools target" },
    );
    const ws = new WebSocket(target.webSocketDebuggerUrl);
    await new Promise((r, j) => {
      ws.addEventListener("open", r, { once: true });
      ws.addEventListener("error", j, { once: true });
    });
    return new Cdp(ws);
  }
  send(method, params = {}) {
    const id = ++this.id;
    this.ws.send(JSON.stringify({ id, method, params }));
    return new Promise((res, rej) => this.pending.set(id, { res, rej }));
  }
  async eval(expr) {
    const r = await this.send("Runtime.evaluate", { expression: expr, returnByValue: true, awaitPromise: true });
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text);
    return r.result.value;
  }
  /** A real mouse click at the center of the first element matching the selector. */
  async click(selector) {
    const box = await this.eval(`(() => { const e = document.querySelector(${JSON.stringify(selector)}); if (!e) return null; e.scrollIntoView({block: "nearest"}); const r = e.getBoundingClientRect(); return {x: r.x + r.width / 2, y: r.y + r.height / 2}; })()`);
    if (!box) throw new Error(`no element ${selector}`);
    for (const type of ["mouseMoved", "mousePressed", "mouseReleased"]) {
      await this.send("Input.dispatchMouseEvent", { type, x: box.x, y: box.y, button: "left", clickCount: 1 });
    }
  }
  async screenshot(file) {
    const r = await this.send("Page.captureScreenshot", { format: "png" });
    writeFileSync(file, Buffer.from(r.data, "base64"));
  }
  close() {
    try {
      this.ws.close();
    } catch {}
  }
}

// ---------------------------------------------------------------------------------------------
// The app under test.

export function defaultSettings(over = {}) {
  return {
    hotkey: "Ctrl+Alt+R",
    default_tone: "Clear",
    default_length: "Same",
    default_toggles: [],
    drafts: 2,
    model: "haiku",
    warm_mode: true,
    priming: false,
    theme: "light",
    start_at_login: false,
    popup_position: "center",
    claude_path: FAKE,
    codex_path: FAKE_CODEX,
    provider: "claude",
    chatgpt_model: "gpt-6-luna",
    no_contractions: true,
    no_em_dashes: true,
    paused: false,
    setup: { version: 1, completed: ["welcome", "connect", "hotkey", "defaults", "try", "done"], pending_new: [] },
    ...over,
  };
}

export async function startApp({ settings = defaultSettings(), mode = "normal", name = "app", fresh = false } = {}) {
  const dir = join(tmpdir(), "mm-e2e", name + "-" + Date.now());
  mkdirSync(join(dir, "config"), { recursive: true });
  if (!fresh) writeFileSync(join(dir, "config", "settings.json"), JSON.stringify(settings, null, 2));
  const fakeLog = join(dir, "fake.log");
  const modeFile = join(dir, "fake-mode.txt");
  writeFileSync(modeFile, mode);
  const env = {
    ...process.env,
    MAGIC_MEND_DATA_DIR: dir,
    MM_FAKE_LOG: fakeLog,
    MM_FAKE_MODE_FILE: modeFile,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${CDP_PORT}`,
    // Keep the test browser data apart from an installed copy of the app.
    WEBVIEW2_USER_DATA_FOLDER: join(dir, "webview"),
  };
  for (const k of Object.keys(env)) if (/^(CLAUDECODE|CLAUDE_CODE_|CLAUDE_AGENT_SDK)/.test(k)) delete env[k];
  const proc = spawn(APP, [], { env, stdio: "ignore" });
  const app = {
    proc,
    dir,
    fakeLog: () => (existsSync(fakeLog) ? readFileSync(fakeLog, "utf8").split("\n").filter(Boolean).map((l) => JSON.parse(l)) : []),
    setMode: (m) => writeFileSync(modeFile, m),
    log: () => (existsSync(join(dir, "logs", "magic-mend.log")) ? readFileSync(join(dir, "logs", "magic-mend.log"), "utf8") : ""),
    popup: null,
    async stop() {
      this.popup?.close();
      try {
        execFileSync("taskkill", ["/PID", String(proc.pid), "/T", "/F"], { stdio: "ignore" });
      } catch {}
      await sleep(500);
    },
  };
  app.popup = await Cdp.connect((u) => /tauri\.localhost\/(index\.html)?$/.test(u) || u.endsWith("/index.html"));
  await app.popup.send("Runtime.enable");
  return app;
}

export const popupVisible = () => {
  const f = fg();
  return f && f.process.toLowerCase() === "magic-mend.exe" && f.title === "Magic Mend";
};

/** Waits until the popup is in front and has at least `n` drafts that can be used. */
export async function waitDrafts(app, n = 1, timeout = 10000) {
  return waitFor(
    () => app.popup.eval(`document.querySelectorAll('.card .btn-secondary:not([disabled])').length >= ${n} ? [...document.querySelectorAll('.card .body')].map(b => b.textContent) : null`),
    { timeout, what: `${n} ready drafts` },
  );
}

export function findFiles(dir) {
  const out = [];
  if (!existsSync(dir)) return out;
  for (const e of readdirSync(dir)) {
    const p = join(dir, e);
    const s = statSync(p);
    if (s.isDirectory()) out.push(...findFiles(p));
    else out.push(p);
  }
  return out;
}

export function childProcesses(pid) {
  try {
    const out = execFileSync("powershell", ["-NoProfile", "-Command", `Get-CimInstance Win32_Process -Filter "ParentProcessId=${pid}" | Select-Object ProcessId,Name,WorkingSetSize | ConvertTo-Json`], { encoding: "utf8" });
    const v = out.trim() ? JSON.parse(out) : [];
    return Array.isArray(v) ? v : [v];
  } catch {
    return [];
  }
}

export function tcpConnections(pid) {
  const out = execFileSync("powershell", ["-NoProfile", "-Command", `@(Get-NetTCPConnection -OwningProcess ${pid} -ErrorAction SilentlyContinue).Count; @(Get-NetUDPEndpoint -OwningProcess ${pid} -ErrorAction SilentlyContinue).Count`], { encoding: "utf8" });
  const [tcp, udp] = out.trim().split(/\s+/).map(Number);
  return { tcp, udp };
}

export function cleanTmp() {
  try {
    rmSync(join(tmpdir(), "mm-e2e"), { recursive: true, force: true });
  } catch {}
}
