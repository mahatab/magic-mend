// Captures marketing screenshots of the real app with real Claude drafts.
// Needs a signed-in claude, the debug build, and the test helpers:
//   npx tauri build --debug --no-bundle
//   cargo build --features test-tools --bin mm-testpad (in src-tauri)
//   node scripts/capture-screens.mjs
// The popup's target name is set to the app each shot represents (WhatsApp, Outlook), because
// the test window stands in for those apps.

import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { homedir } from "node:os";
import {
  ROOT, startApp, defaultSettings, startPad, hotkey, waitFor, popupVisible, sleep, Cdp, pad, fg,
} from "../tests/e2e/lib.mjs";

const OUT = join(ROOT, "docs", "images", "screenshots");
mkdirSync(OUT, { recursive: true });
const CLAUDE = join(homedir(), ".local", "bin", "claude.exe");

async function shoot(cdp, selector, file) {
  await cdp.send("Emulation.setDefaultBackgroundColorOverride", { color: { r: 0, g: 0, b: 0, a: 0 } });
  await cdp.eval(`document.getAnimations().forEach(a => a.finish()), 1`);
  const box = await cdp.eval(`(() => { const r = document.querySelector(${JSON.stringify(selector)}).getBoundingClientRect(); return { x: r.x, y: r.y, width: r.width, height: r.height }; })()`);
  const r = await cdp.send("Page.captureScreenshot", { format: "png", captureBeyondViewport: true, clip: { ...box, scale: 2 } });
  const { writeFileSync } = await import("node:fs");
  writeFileSync(join(OUT, file), Buffer.from(r.data, "base64"));
  console.log("saved", file, `${Math.round(box.width)}x${Math.round(box.height)}`);
}

/** Opens the popup on a stand-in window and dresses the header as the given app. */
async function popupShot(app, { text, appName, letterColor, theme = "light", channel, tone, toggles = [], followup, file }) {
  const p = await startPad(text, "shot");
  await sleep(200);
  hotkey();
  await waitFor(popupVisible, { what: "popup", timeout: 8000 });
  const c = app.popup;
  await c.eval(`(() => {
    document.documentElement.dataset.theme = ${JSON.stringify(theme)};
    document.body.classList.remove("translucent"); document.body.classList.add("solid");
    return 1; })()`);
  const waitReady = (n) =>
    waitFor(() => c.eval(`document.querySelectorAll('.card .btn-primary:not([disabled])').length >= ${n} && !document.querySelector('.card .skeleton')`), { timeout: 45000, interval: 200, what: "drafts" });
  await waitReady(1);
  if (channel) {
    await c.eval(`(() => { const s = document.querySelector('.channel-row select'); s.value = ${JSON.stringify(channel)}; s.dispatchEvent(new Event('change')); return 1; })()`);
    await sleep(400);
  }
  if (tone) await c.click(`.tone-row .chip[data-value="${tone}"]`);
  for (const t of toggles) await c.click(`.switch[data-toggle="${t}"]`);
  if (tone || toggles.length || channel) {
    await sleep(600);
    await waitReady(1);
  }
  if (followup) {
    await c.click(".followup textarea");
    await c.eval(`(() => { const t = document.querySelector('.followup textarea'); t.value = ${JSON.stringify(followup)}; t.dispatchEvent(new Event('input')); return 1; })()`);
    await c.eval(`document.querySelector('.followup textarea').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true })), 1`);
    await sleep(800);
    await waitReady(1);
  }
  await c.eval(`(() => {
    const t = document.querySelector('.target');
    t.querySelector('img, .letter')?.remove();
    const l = document.createElement('span'); l.className = 'letter'; l.textContent = ${JSON.stringify(appName[0])};
    l.style.background = ${JSON.stringify(letterColor)}; l.style.color = '#fff';
    t.prepend(l);
    t.querySelector('strong').textContent = ${JSON.stringify(appName)};
    document.querySelector('.channel-row .hint').textContent = '(auto-detected)';
    return 1; })()`);
  await sleep(300);
  await shoot(c, "#app", file);
  await c.click(".close");
  await sleep(500);
  p.close();
}

async function main() {
  pad("release");
  const settings = defaultSettings({ claude_path: CLAUDE, model: "sonnet", theme: "light" });
  let app = await startApp({ name: "shots", settings });
  await sleep(3000);

  await popupShot(app, {
    text: "hey sam, the plumber cant come till thursday so can we push the move-in date to saturday instead, sorry for the trouble",
    appName: "WhatsApp", letterColor: "#25D366", channel: "WhatsApp", file: "popup-light.png",
  });
  await popupShot(app, {
    text: "hey sam, the plumber cant come till thursday so can we push the move-in date to saturday instead, sorry for the trouble",
    appName: "WhatsApp", letterColor: "#25D366", theme: "dark", channel: "WhatsApp", tone: "Warm", file: "popup-dark.png",
  });
  await popupShot(app, {
    text: "Hi Priya, just checking if you had a chance to look at the proposal I sent on March 12, happy to walk through it on a call this week",
    appName: "Outlook", letterColor: "#0F6CBD", channel: "Email", tone: "Professional", file: "popup-email.png",
  });
  await popupShot(app, {
    text: "me and my wife was thinking we could come for dinner on friday if your free, we will bring desert",
    appName: "Messenger", letterColor: "#0866FF", toggles: ["Fix errors only"], file: "popup-fix.png",
  });
  await popupShot(app, {
    text: "Hi Daniel, sorry for the late reply, I would be glad to join the panel on May 8 and can share my slides by May 1",
    appName: "LinkedIn", letterColor: "#0A66C2", theme: "dark", channel: "LinkedIn", followup: "remove the apology", file: "popup-followup.png",
  });

  // Settings window.
  await app.popup.eval(`window.__TAURI_INTERNALS__.invoke('open_settings'), 1`);
  const st = await Cdp.connect((u) => u.includes("settings.html"));
  await sleep(1500);
  await st.send("Emulation.setDeviceMetricsOverride", { width: 640, height: 900, deviceScaleFactor: 2, mobile: false });
  await sleep(500);
  await shoot(st, "#root", "settings.png");
  st.close();
  await app.stop();

  // Setup wizard on a fresh profile.
  app = await startApp({ name: "shots-wizard", settings: { claude_path: CLAUDE, model: "sonnet", theme: "light" } });
  const wz = await Cdp.connect((u) => u.includes("wizard.html"));
  await waitFor(() => wz.eval(`document.querySelector('h1')?.textContent === 'Welcome to Magic Mend'`), { what: "welcome" });
  await sleep(600);
  await wz.eval(`document.body.classList.remove('translucent'); document.body.classList.add('solid'); 1`);
  await shoot(wz, "#shell", "wizard-welcome.png");
  await wz.click("#next");
  await waitFor(() => wz.eval(`!document.getElementById('next').disabled`), { what: "connect passed", timeout: 60000, interval: 300 });
  await sleep(400);
  await shoot(wz, "#shell", "wizard-connect.png");
  await wz.click("#next");
  await sleep(500);
  await shoot(wz, "#shell", "wizard-hotkey.png");
  await wz.click("#next");
  await sleep(300);
  await wz.click("#next");
  await sleep(500);
  await shoot(wz, "#shell", "wizard-try.png");
  wz.close();
  await app.stop();
  console.log("done");
  process.exit(0);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
