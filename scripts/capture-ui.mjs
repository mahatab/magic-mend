// Renders the app's own UI (the same HTML, CSS, and TypeScript the app ships) in headless Edge
// with real Claude drafts from scripts/scenes-out.json, and saves 2x screenshots.
// Run `mm-harness scenes scripts/scenes.json scripts/scenes-out.json` first for fresh drafts.
//   node scripts/capture-ui.mjs

import { spawn } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { ROOT, Cdp, sleep, waitFor } from "../tests/e2e/lib.mjs";

const OUT = join(ROOT, "docs", "images", "screenshots");
const PORT = 1421;
const CDP = 9444;
const EDGE = ["C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe", "C:/Program Files/Microsoft/Edge/Application/msedge.exe"].find(existsSync);
const scenes = JSON.parse(readFileSync(join(ROOT, "scripts", "scenes-out.json"), "utf8"));
mkdirSync(OUT, { recursive: true });

// Serve the production build: the exact files the app ships.
const vite = spawn(process.execPath, [join(ROOT, "node_modules", "vite", "bin", "vite.js"), "preview", "--port", String(PORT), "--strictPort"], { cwd: ROOT, stdio: "ignore" });
const edge = spawn(EDGE, ["--headless=new", `--remote-debugging-port=${CDP}`, `--user-data-dir=${join(tmpdir(), "mm-capture-edge")}`, "--hide-scrollbars", "about:blank"], { stdio: "ignore" });

async function page() {
  await waitFor(async () => (await fetch(`http://localhost:${PORT}/`)).ok, { what: "vite", timeout: 20000, interval: 300 });
  return Cdp.connect((u) => u.startsWith("about:blank"), CDP, 20000);
}

async function setup(c, { width, height, dark }) {
  await c.send("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: 2, mobile: false });
  await c.send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: dark ? "dark" : "light" }] });
  await c.send("Emulation.setDefaultBackgroundColorOverride", { color: { r: 0, g: 0, b: 0, a: 0 } });
}

async function go(c, url) {
  await c.send("Page.navigate", { url });
  await waitFor(() => c.eval(`document.readyState === 'complete' && !!window.__mm || !!document.querySelector('#shell h1, #root h1')`), { what: url, timeout: 15000 });
}

async function shoot(c, selector, file) {
  await c.eval(`document.getAnimations().forEach(a => a.finish()), 1`);
  const box = await c.eval(`(() => { const r = document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect(); return { x: r.x, y: r.y, width: r.width, height: r.height }; })()`);
  const r = await c.send("Page.captureScreenshot", { format: "png", captureBeyondViewport: true, clip: { ...box, scale: 1 } });
  writeFileSync(join(OUT, file), Buffer.from(r.data, "base64"));
  console.log("saved", file);
}

/** Opens the popup with a target app and a scene's real drafts. */
async function popup(c, { scene, text, app, color, channel, tone = "Simple", toggles = [], dark = false, file, midStream = false, followupNote }) {
  await setup(c, { width: 480, height: 900, dark });
  await go(c, `http://localhost:${PORT}/?mock`);
  const s = scenes[scene];
  await c.eval(`(() => {
    window.__mm.mock.scripted = ${JSON.stringify(s)};
    window.__mm.open(window.__mm.mock.payload({
      capture: { kind: "text", text: ${JSON.stringify(text)} },
      target: { app_name: ${JSON.stringify(app)}, process: "", icon: null, elevated: false, own_window: false, present: true },
      channel: ${JSON.stringify(channel)}, tone: ${JSON.stringify(tone)}, toggles: ${JSON.stringify(toggles)}, theme: ${JSON.stringify(dark ? "dark" : "light")},
    }));
    const l = document.querySelector('.target .letter'); l.style.background = ${JSON.stringify(color)}; l.style.color = '#fff';
    return 1; })()`);
  if (midStream) {
    await waitFor(() => c.eval(`(document.querySelector('.card .body')?.textContent.length ?? 0) > 40`), { what: "partial", timeout: 8000, interval: 20 });
  } else {
    await waitFor(() => c.eval(`document.querySelectorAll('.card .btn-primary:not([disabled])').length >= 1`), { what: "drafts", timeout: 15000 });
    if (followupNote) await c.eval(`(() => { const t = document.querySelector('.followup textarea'); t.value = ${JSON.stringify(followupNote)}; return 1; })()`);
  }
  if (!midStream) await c.eval(`(() => { const st = document.createElement("style"); st.textContent = "*{animation:none!important;transition:none!important}"; document.head.appendChild(st); return 1; })()`);
  await sleep(250);
  await shoot(c, "#app", file);
}

async function main() {
  const c = await page();
  await c.send("Page.enable");
  await c.send("Runtime.enable");

  const main = "hey sam, the plumber cant come till thursday so can we push the move-in date to saturday instead, sorry for the trouble";
  await popup(c, { scene: "whatsapp", text: main, app: "WhatsApp", color: "#25D366", channel: "WhatsApp", file: "popup-light.png" });
  await popup(c, { scene: "warm", text: main, app: "WhatsApp", color: "#25D366", channel: "WhatsApp", tone: "Warm", dark: true, file: "popup-dark.png" });
  await popup(c, { scene: "whatsapp", text: main, app: "WhatsApp", color: "#25D366", channel: "WhatsApp", file: "popup-streaming.png", midStream: true });
  await popup(c, {
    scene: "email", text: "Hi Priya, just checking if you had a chance to look at the proposal I sent on March 12, happy to walk through it on a call this week",
    app: "Outlook", color: "#0F6CBD", channel: "Email", tone: "Professional", file: "popup-email.png",
  });
  await popup(c, {
    scene: "fix", text: "me and my wife was thinking we could come for dinner on friday if your free, we will bring desert",
    app: "Messenger", color: "#0866FF", channel: "Text", toggles: ["Fix errors only"], file: "popup-fix.png",
  });
  await popup(c, {
    scene: "followup", text: "Hi Daniel, sorry for the late reply, I would be glad to join the panel on May 8 and can share my slides by May 1",
    app: "LinkedIn", color: "#0A66C2", channel: "LinkedIn", dark: true, file: "popup-followup.png", followupNote: "remove the apology",
  });
  await popup(c, {
    scene: "greeting", text: "Good morning everyone! quick reminder the community cleanup is this sunday at 9am at Riverside Park, bring gloves if you have them",
    app: "WhatsApp", color: "#25D366", channel: "WhatsApp", file: "popup-greeting.png",
  });

  for (const [step, file, wait] of [["welcome", "wizard-welcome.png", 600], ["connect", "wizard-connect.png", 2600], ["hotkey", "wizard-hotkey.png", 600], ["try", "wizard-try.png", 600]]) {
    await setup(c, { width: 560, height: 640, dark: false });
    await go(c, `http://localhost:${PORT}/wizard.html?step=${step}`);
    await sleep(wait);
    await shoot(c, "#shell", file);
  }
  await setup(c, { width: 640, height: 980, dark: false });
  await go(c, `http://localhost:${PORT}/settings.html`);
  await sleep(700);
  await shoot(c, ".scroll", "settings.png");

  c.close();
}

main()
  .catch((e) => {
    console.error(e);
    process.exitCode = 1;
  })
  .finally(() => {
    edge.kill();
    vite.kill();
    setTimeout(() => process.exit(), 300);
  });
