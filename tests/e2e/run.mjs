// Windows end-to-end acceptance tests. They drive the real app, the real clipboard, and real
// key presses against a test window, with a fake claude so no login is needed.
// Run: node tests/e2e/run.mjs [filter]
// The tests take over the keyboard and the foreground window for a few minutes.

import { writeFileSync, mkdirSync, readFileSync, existsSync } from "node:fs";
import { join } from "node:path";
import { execFileSync, spawn } from "node:child_process";
import {
  ROOT, FAKE, FAKE_CODEX, sleep, waitFor, keys, clipSet, clipGet, clipSeq, fg, hotkey,
  startPad, startApp, defaultSettings, popupVisible, waitDrafts, findFiles, childProcesses,
  tcpConnections, cleanTmp, Cdp, pad,
} from "./lib.mjs";

const results = [];
const filter = process.argv[2];

async function test(id, name, fn) {
  if (filter && !`${id} ${name}`.toLowerCase().includes(filter.toLowerCase())) return;
  const started = Date.now();
  process.stdout.write(`- ${id} ${name} ... `);
  try {
    const note = (await fn()) ?? "";
    results.push({ id, name, ok: true, note, ms: Date.now() - started });
    console.log(`PASS ${note}`);
  } catch (e) {
    results.push({ id, name, ok: false, note: e.message, ms: Date.now() - started });
    console.log(`FAIL ${e.message}`);
  }
}

function assert(cond, msg) {
  if (!cond) throw new Error(msg);
}

/** Opens the popup on a fresh test window and waits for drafts. */
async function openOn(app, text, name) {
  const p = await startPad(text, name);
  await sleep(150);
  hotkey();
  // A window left by an earlier test (a terminal, for example) can keep the foreground for a
  // moment. Bring the test window forward and press the hotkey once more.
  await waitFor(popupVisible, { what: "popup in front", timeout: 4000 }).catch(async () => {
    p.focus();
    await sleep(300);
    hotkey();
    await waitFor(popupVisible, { what: "popup in front" });
  });
  return p;
}

async function closePopup(app) {
  await app.popup.click(".close");
  await waitFor(() => !popupVisible(), { what: "popup closed", timeout: 3000 });
}

const enterKeys = (lines) => lines.filter((l) => /keydown .*0x0D$/.test(l) || l === "char 0x0D" || l === "char 0x0A");

// ---------------------------------------------------------------------------------------------

async function main() {
  cleanTmp();
  pad("release"); // clear any key Windows still reports as held
  console.log("Windows end-to-end tests (fake claude and fake codex)\n");
  let app = await startApp({ name: "main" });
  await sleep(1500);

  await test("1", "Replace in a text box, undo, clipboard restored", async () => {
    clipSet("ORIGINAL-CLIP-123");
    const before = "hello wrold, i has a typo";
    const p = await openOn(app, before, "t1");
    const shown = await app.popup.eval(`document.querySelector('.original .text')?.textContent`);
    assert(shown === before, `popup shows "${shown}"`);
    let c = clipGet();
    assert(c.text === "ORIGINAL-CLIP-123" && c.custom, `clipboard after capture: ${JSON.stringify(c)}`);
    const drafts = await waitDrafts(app, 2);
    await app.popup.click(".card .btn-primary");
    await waitFor(() => p.text() === drafts[0], { what: "text replaced" }).catch((e) => {
      const r = app.log().match(/replace result=\w+/g);
      throw new Error(`${e.message}; app said: ${r ? r.join(", ") : "no replace logged"}; foreground: ${fg()?.title}; window got: ${p.keys().slice(-8).join(" | ")}; text now: "${p.text()}"`);
    });
    assert(!p.text().endsWith("\n"), "trailing line break");
    await sleep(700);
    c = clipGet();
    assert(c.text === "ORIGINAL-CLIP-123" && c.custom, `clipboard after Replace: ${JSON.stringify(c)}`);
    assert(!popupVisible(), "popup still open after Replace");
    const k = p.keys();
    assert(enterKeys(k).length === 0, `Enter reached the app: ${enterKeys(k)}`);
    // A classic edit box gets WM_PASTE instead of the Ctrl+V shortcut.
    assert(k.includes("keydown ctrl+0x43") && k.includes("paste") && !k.includes("keydown ctrl+0x56"), `expected Ctrl+C, then WM_PASTE and no Ctrl+V: ${k.join(", ")}`);
    p.focus();
    await sleep(150);
    keys("ctrl+z");
    await waitFor(() => p.text() === before, { what: "undo restores the original" });
    p.close();
    return "text replaced, Ctrl+Z restored it, clipboard (text and custom format) restored";
  });

  await test("hard-rule-3", "Engine uses the isolation flags and its own empty folder", async () => {
    const argv = app.fakeLog().find((l) => l.argv)?.argv ?? [];
    for (const f of ["-p", "--input-format", "--output-format", "--include-partial-messages", "--strict-mcp-config", "--disable-slash-commands", "--no-session-persistence", "--safe-mode", "--system-prompt-file"]) {
      assert(argv.includes(f), `missing ${f}`);
    }
    assert(argv[argv.indexOf("--tools") + 1] === "", "--tools is not empty");
    assert(!argv.some((a) => /dangerously|bypass/i.test(a)), "a permission bypass flag is set");
    const cwd = app.fakeLog().find((l) => l.argv)?.cwd ?? "";
    assert(/engine[\\/](claude|chatgpt)$/i.test(cwd), `cwd is ${cwd}`);
    return `cwd ${cwd.split(/[\\/]/).slice(-2).join("/")}`;
  });

  await test("5", "Enter in the popup never reaches the app underneath", async () => {
    const p = await openOn(app, "enter safety check", "t5");
    await waitDrafts(app, 2);
    await app.popup.click(".followup textarea");
    keys("m");
    keys("enter"); // submits the follow-up inside the popup
    await sleep(300);
    await app.popup.click(".tone-row .chip:nth-child(2)");
    keys("enter");
    keys("space");
    await app.popup.eval(`document.querySelector('.card .btn-primary')?.focus(), 1`);
    keys("enter");
    await sleep(600);
    assert(popupVisible(), "popup closed by a key press");
    const k = p.keys();
    assert(enterKeys(k).length === 0, `Enter reached the app: ${enterKeys(k)}`);
    const sent = app.fakeLog().filter((l) => l.message?.includes('"followup":"m"'));
    assert(sent.length === 1, "follow-up was not submitted inside the popup");
    keys("esc");
    await waitFor(() => !popupVisible(), { what: "Esc closes the popup", timeout: 3000 });
    assert(p.text() === "enter safety check", "app text changed");
    p.close();
    return "Enter submitted the follow-up in the popup only; focused Replace ignored Enter; Esc closed";
  });

  await test("6", "No selection: editable box, Replace disabled, Copy works", async () => {
    const p = await startPad("caret only, nothing selected", "t6");
    keys("end");
    await sleep(150);
    clipSet("KEEP-ME");
    hotkey();
    await waitFor(popupVisible, { what: "popup" });
    const editable = await app.popup.eval(`!!document.querySelector('#orig-input')`);
    assert(editable, "no editable box");
    await app.popup.click("#orig-input");
    for (const ch of "hi") keys(ch);
    await waitDrafts(app, 2);
    const states = await app.popup.eval(`[...document.querySelectorAll('.card')].map(c => [c.querySelector('.btn-primary').disabled, c.querySelector('.btn-secondary').disabled])`);
    assert(states.every(([r, c]) => r && !c), `buttons: ${JSON.stringify(states)}`);
    assert(clipGet().text === "KEEP-ME", "clipboard changed");
    await app.popup.click(".card .btn-secondary");
    await waitFor(() => clipGet().text.startsWith("[Clear] Draft A: hi"), { what: "Copy put the draft on the clipboard" });
    await waitFor(() => !popupVisible(), { what: "popup closes after Copy", timeout: 3000 });
    p.close();
    return "Copy left the draft on the clipboard as intended";
  });

  await test("7", "Proofread gives one draft and locks the other chips", async () => {
    const p = await openOn(app, "i has a eror", "t7");
    await waitDrafts(app, 2);
    await app.popup.click('.switch[data-toggle="Proofread"]');
    await waitFor(() => app.popup.eval(`document.querySelectorAll('.card').length === 1 && document.querySelector('.card .body').textContent.includes('Draft A')`), { what: "one draft" });
    const req = app.fakeLog().filter((l) => l.message?.includes("Proofread")).pop();
    assert(req, "request without the toggle");
    // Tone, length, and the other toggles are locked: clicks and keys send nothing.
    const locked = await app.popup.eval(`[...document.querySelectorAll('.tone-row .chip, .length-row .chip, .switch:not([data-toggle="Proofread"])')].every(b => b.disabled) && getComputedStyle(document.querySelector('.proof-note')).display !== 'none'`);
    assert(locked, "controls not disabled or note hidden under Proofread");
    const before = app.fakeLog().filter((l) => l.message?.startsWith("{")).length;
    await app.popup.click(".tone-row .chip:nth-child(3)");
    await app.popup.click(".length-row .chip:nth-child(1)");
    await app.popup.click('.switch[data-toggle="Softer"]');
    await sleep(800);
    const extra = app.fakeLog().filter((l) => l.message?.startsWith("{")).length - before;
    assert(extra === 0, `${extra} requests sent from locked controls`);
    // Turning Proofread off brings back two drafts in the tone picked before.
    await app.popup.click('.switch[data-toggle="Proofread"]');
    await waitFor(() => app.popup.eval(`document.querySelectorAll('.card .body').length === 2 && [...document.querySelectorAll('.card .body')].every(b => b.textContent.startsWith('[Clear]')) && !document.querySelector('.tone-row .chip').disabled`), { what: "two Clear drafts after Proofread off" });
    await closePopup(app);
    p.close();
    return "one draft; locked chips sent no request; turning Proofread off restored the picks";
  });

  await test("11", "Warm spare: a new process is ready after Replace", async () => {
    const p = await openOn(app, "warm spare check", "t11");
    await waitDrafts(app, 2);
    const spawnsBefore = app.fakeLog().filter((l) => l.argv).length;
    await app.popup.click(".card .btn-primary");
    await waitFor(() => !popupVisible(), { what: "popup hidden" });
    await waitFor(() => app.fakeLog().filter((l) => l.argv).length === spawnsBefore + 1, { what: "new spare started" });
    await sleep(500);
    const kids = childProcesses(app.proc.pid).filter((k) => /fakeclaude/i.test(k.Name));
    assert(kids.length === 1, `expected one claude process, found ${kids.length}`);
    p.close();
    const p2 = await startPad("second message", "t11b");
    const t0 = Date.now();
    hotkey();
    await waitFor(popupVisible, { what: "popup" });
    const visibleMs = Date.now() - t0;
    await waitDrafts(app, 2);
    const firstDraftMs = Date.now() - t0;
    await closePopup(app);
    p2.close();
    // The app logs its own time from hotkey to visible popup, without the test helper's overhead.
    const times = [...app.log().matchAll(/popup_visible ms=(\d+)/g)].map((m) => Number(m[1]));
    const appMs = times[times.length - 1];
    assert(appMs <= 150, `popup took ${appMs} ms in the app (target 150 ms)`);
    return `one spare after Replace; popup visible ${appMs} ms after the hotkey (app timing; ${visibleMs} ms seen by the test), drafts ${firstDraftMs} ms`;
  });

  await test("edge", "Original window closed: Replace turns into Copy only", async () => {
    const p = await openOn(app, "window closed check", "tclosed");
    await waitDrafts(app, 2);
    p.close();
    const label = await waitFor(
      () => app.popup.eval(`(() => { const b = document.querySelector('.card .btn-primary'); return b && b.disabled && b.textContent === 'Copy only' ? b.textContent : null })()`),
      { what: "Copy only", timeout: 4000 },
    );
    const reason = await app.popup.eval(`document.querySelector('.reason')?.textContent`);
    await closePopup(app);
    return `"${label}" with "${reason}"`;
  });

  await test("17", "Held hotkey still captures and sends no stray shortcut", async () => {
    const p = await startPad("held modifiers check", "t17");
    await sleep(150);
    hotkey(1500);
    await waitFor(popupVisible, { what: "popup" });
    const shown = await app.popup.eval(`document.querySelector('.original .text')?.textContent`);
    assert(shown === "held modifiers check", `captured "${shown}"`);
    const k = p.keys();
    assert(!k.some((l) => /ctrl\+alt\+0x43|alt\+0x43|shift\+0x43/.test(l)), `stray shortcut: ${k.join(", ")}`);
    await closePopup(app);
    p.close();
    return "selection captured after a 1.5 s hold";
  });

  await test("19", "Rapid chip changes end with drafts for the last chip only", async () => {
    const p = await openOn(app, "rapid chips", "t19");
    await waitDrafts(app, 2);
    const before = app.fakeLog().filter((l) => l.message?.startsWith("{")).length;
    for (const n of [2, 3, 4, 5, 6]) await app.popup.click(`.tone-row .chip:nth-child(${n})`);
    await waitFor(() => app.popup.eval(`[...document.querySelectorAll('.card .body')].every(b => b.textContent.startsWith('[Firm]')) && document.querySelectorAll('.card .btn-secondary:not([disabled])').length === 2`), { what: "Firm drafts" });
    const sent = app.fakeLog().filter((l) => l.message?.startsWith("{")).slice(before);
    assert(sent.length === 1, `${sent.length} requests sent`);
    await closePopup(app);
    p.close();
    return "five clicks, one request, Firm drafts shown";
  });

  await test("20", "Multi-line draft pastes with no trailing line break and no Enter", async () => {
    const p = await openOn(app, "MULTILINE please", "t20");
    await waitDrafts(app, 2);
    await app.popup.click(".card .btn-primary");
    const want = "[Clear] Draft A: First line of the draft.\nSecond line of the draft.\n\nLast line.";
    await waitFor(() => p.text() === want, { what: "multi-line text pasted" }).catch((e) => {
      const r = app.log().match(/replace result=\w+/g);
      throw new Error(`${e.message}; app said: ${r ? r.join(", ") : "no replace logged"}; foreground: ${fg()?.title}; window got: ${p.keys().slice(-8).join(" | ")}; text now: ${JSON.stringify(p.text())}`);
    });
    const k = p.keys();
    assert(enterKeys(k).length === 0, "Enter reached the app");
    assert(k.includes("paste") && !k.includes("keydown ctrl+0x56"), `expected WM_PASTE and no Ctrl+V: ${k.join(", ")}`);
    p.close();
    return "pasted with WM_PASTE, no Ctrl+V and no Enter";
  });

  await test("12a", "Failure: signed out shows a plain message", async () => {
    app.setMode("signed_out");
    await app.stop();
    app = await startApp({ name: "signedout", mode: "signed_out" });
    const p = await openOn(app, "signed out check", "t12a");
    const msg = await waitFor(() => app.popup.eval(`document.querySelector('.banner.error p')?.textContent`), { what: "error banner" });
    assert(/not signed in/i.test(msg) && /run claude/i.test(msg), msg);
    app.setMode("normal");
    await app.popup.click(".banner.error .btn");
    await waitDrafts(app, 2);
    await closePopup(app);
    p.close();
    return `"${msg}" then Retry worked after signing in`;
  });

  await test("12b", "Failure: plan limit and offline show plain messages", async () => {
    const out = [];
    for (const [mode, re] of [["limit", /plan limit/i], ["offline", /internet/i]]) {
      app.setMode(mode);
      const p = await openOn(app, `${mode} check`, "t12b");
      // The spare was started before the mode changed, so press Retry once to use the new mode.
      let msg = await waitFor(() => app.popup.eval(`document.querySelector('.banner.error p')?.textContent || (document.querySelectorAll('.card .btn-secondary:not([disabled])').length ? 'drafts' : null)`), { what: "result" });
      if (msg === "drafts") {
        await app.popup.click(".tone-row .chip:nth-child(2)");
        await sleep(100);
      }
      if (msg === "drafts" || !re.test(msg)) {
        await app.popup.eval(`void 0`);
        await closePopup(app);
        p.close();
        const p2 = await openOn(app, `${mode} check again`, "t12b2");
        msg = await waitFor(() => app.popup.eval(`document.querySelector('.banner.error p')?.textContent`), { what: "error banner" });
        await closePopup(app);
        p2.close();
      } else {
        await closePopup(app);
        p.close();
      }
      assert(re.test(msg), `${mode}: ${msg}`);
      out.push(`${mode}: "${msg}"`);
    }
    app.setMode("normal");
    return out.join("; ");
  });

  await test("12c", "Failure: claude missing shows the fix", async () => {
    await app.stop();
    app = await startApp({ name: "missing", settings: defaultSettings({ claude_path: "C:\\nope\\claude.exe" }) });
    const p = await openOn(app, "missing check", "t12c");
    const msg = await waitFor(() => app.popup.eval(`document.querySelector('.banner.error p')?.textContent`), { what: "error banner" });
    assert(/cannot find the claude command/i.test(msg) && /Settings/.test(msg), msg);
    await closePopup(app);
    p.close();
    return `"${msg}"`;
  });

  await test("12d", "Failure: timeout after 30 seconds with Retry", async () => {
    await app.stop();
    app = await startApp({ name: "hang", mode: "hang" });
    await sleep(800);
    const p = await openOn(app, "timeout check", "t12d");
    const t0 = Date.now();
    const msg = await waitFor(() => app.popup.eval(`document.querySelector('.banner.error p')?.textContent`), { what: "timeout banner", timeout: 40000, interval: 500 });
    const secs = Math.round((Date.now() - t0) / 1000);
    assert(/30 seconds/.test(msg), msg);
    const retry = await app.popup.eval(`!!document.querySelector('.banner.error .btn')`);
    assert(retry, "no Retry button");
    await closePopup(app);
    p.close();
    return `"${msg}" after about ${secs} s`;
  });

  await test("16", "Terminals: the hotkey does nothing", async () => {
    await app.stop();
    app = await startApp({ name: "term" });
    await sleep(1000);
    const out = [];
    for (const [exe, args, title] of [["powershell.exe", ["-NoExit", "-Command", "$host.UI.RawUI.WindowTitle='MM-TERM-PS'"], "MM-TERM-PS"], ["cmd.exe", ["/k", "title MM-TERM-CMD"], "MM-TERM-CMD"]]) {
      const proc = spawn("cmd.exe", ["/c", "start", "", exe, ...args], { stdio: "ignore", detached: true });
      proc.unref();
      const info = await waitFor(() => { const f = fg(); return f && f.title.includes(title) ? f : null; }, { what: `${title} in front`, timeout: 8000 });
      const seq = clipSeq();
      hotkey();
      await sleep(1200);
      assert(!popupVisible(), `popup opened in ${info.process}`);
      assert(clipSeq() === seq, `clipboard touched in ${info.process}`);
      out.push(info.process);
      try {
        execFileSync("taskkill", ["/FI", `WINDOWTITLE eq ${title}*`, "/T", "/F"], { stdio: "ignore" });
      } catch {}
      await sleep(400);
    }
    return `no popup and no copy in ${out.join(", ")}`;
  });

  await test("13", "Privacy: no message text on disk, no network connections from the app", async () => {
    const secret = "zebra-quartz-7731 secret message";
    const p = await openOn(app, secret, "t13");
    await waitDrafts(app, 2);
    await app.popup.click(".card .btn-primary");
    await waitFor(() => !popupVisible(), { what: "done" });
    await sleep(800);
    const files = findFiles(app.dir).filter((f) => !/EBWebView/i.test(f) && !/fake\.log$|fake-mode\.txt$/.test(f));
    const hits = files.filter((f) => {
      try {
        return readFileSync(f).includes("zebra-quartz");
      } catch {
        return false;
      }
    });
    assert(hits.length === 0, `message text found in ${hits.join(", ")}`);
    const projects = join(process.env.USERPROFILE, ".claude", "projects");
    const slug = app.dir.replace(/[:\\/.]/g, "-");
    const transcripts = existsSync(projects) ? findFiles(projects).filter((f) => f.includes(slug)) : [];
    assert(transcripts.length === 0, "a transcript folder was created");
    const net = tcpConnections(app.proc.pid);
    assert(net.tcp === 0 && net.udp === 0, `app has ${net.tcp} TCP and ${net.udp} UDP sockets`);
    p.close();
    return `${files.length} app files scanned, 0 hits; app process sockets: ${net.tcp} TCP, ${net.udp} UDP`;
  });

  await test("14", "Resources: idle memory with the warm spare", async () => {
    await sleep(1500);
    const own = Number(execFileSync("powershell", ["-NoProfile", "-Command", `(Get-Process -Id ${app.proc.pid}).WorkingSet64`], { encoding: "utf8" }).trim());
    const kids = childProcesses(app.proc.pid);
    // WebView2 processes are children (and grandchildren) of the app.
    const ps = `$all = Get-CimInstance Win32_Process; $ids = @(${app.proc.pid}); $i = 0; while ($i -lt $ids.Count) { $ids += @($all | Where-Object { $_.ParentProcessId -eq $ids[$i] -and $_.Name -eq 'msedgewebview2.exe' } | Select-Object -ExpandProperty ProcessId); $i++ }; $w = $ids | Select-Object -Skip 1; if ($w) { (Get-Process -Id $w | Measure-Object WorkingSet64 -Sum).Sum } else { 0 }`;
    const webview = Number(execFileSync("powershell", ["-NoProfile", "-Command", ps], { encoding: "utf8" }).trim());
    const spare = kids.filter((k) => /claude/i.test(k.Name)).reduce((a, k) => a + Number(k.WorkingSetSize), 0);
    const mb = (b) => (b / 1048576).toFixed(0) + " MB";
    return `app ${mb(own)}, WebView2 ${mb(webview)}, spare (fake) ${mb(spare)}`;
  });

  // ---- ChatGPT (fake codex app-server) ----
  await app.stop();
  app = await startApp({ name: "chatgpt", settings: defaultSettings({ provider: "chatgpt" }) });
  await sleep(1500);

  await test("gpt-1", "ChatGPT: Replace end to end, isolated and ephemeral", async () => {
    clipSet("CLIP-BEFORE-GPT");
    const p = await openOn(app, "chatgpt replace check", "g1");
    const drafts = await waitDrafts(app, 2);
    assert(drafts[0].startsWith("[ChatGPT Clear] Draft A:"), `draft from ${drafts[0]}`);
    const badge = await app.popup.eval(`document.querySelector('.brand .via')?.textContent`);
    assert(badge === "ChatGPT", `badge ${badge}`);
    await app.popup.click(".card .btn-primary");
    await waitFor(() => p.text() === drafts[0], { what: "text replaced" });
    await sleep(700);
    const c = clipGet();
    assert(c.text === "CLIP-BEFORE-GPT" && c.custom, `clipboard after Replace: ${JSON.stringify(c)}`);
    assert(enterKeys(p.keys()).length === 0, "Enter reached the app");
    const log = app.fakeLog();
    const argv = log.find((l) => l.argv && l.argv[0] === "app-server")?.argv ?? [];
    for (const [flag, value] of [["--disable", "shell_tool"], ["--disable", "plugins"], ["--disable", "hooks"], ["--disable", "memories"], ["-c", "project_doc_max_bytes=0"], ["-c", "skills.include_instructions=false"]]) {
      assert(argv.some((a, i) => a === flag && argv[i + 1] === value), `missing ${flag} ${value}`);
    }
    const ts = log.find((l) => l.thread_start)?.thread_start;
    assert(ts && ts.ephemeral === true && ts.sandbox === "read-only" && ts.instructions_len > 500, `thread start ${JSON.stringify(ts)}`);
    assert(ts.model === "gpt-6-luna", `model ${ts.model}`);
    const msg = log.find((l) => l.message);
    assert(msg.effort === "low" && msg.schema === true, `turn ${JSON.stringify(msg)}`);
    p.close();
    return "replaced via ChatGPT; ephemeral read-only thread with our prompt; tools, plugins, hooks, memories, project docs, skills off";
  });

  await test("gpt-2", "ChatGPT: follow-up, Proofread, rapid chip changes", async () => {
    const p = await openOn(app, "chatgpt flow check", "g2");
    await waitDrafts(app, 2);
    await app.popup.click(".followup textarea");
    keys("x");
    keys("enter");
    await waitFor(() => app.popup.eval(`[...document.querySelectorAll('.card .body')].every(b => b.textContent.includes('(after: x)')) && document.querySelectorAll('.card .btn-secondary:not([disabled])').length === 2`), { what: "follow-up drafts" });
    await app.popup.click('.switch[data-toggle="Proofread"]');
    await waitFor(() => app.popup.eval(`document.querySelectorAll('.card').length === 1 && !!document.querySelector('.card .btn-secondary:not([disabled])')`), { what: "one draft" });
    await app.popup.click('.switch[data-toggle="Proofread"]');
    for (const n of [2, 3, 4, 5, 6]) await app.popup.click(`.tone-row .chip:nth-child(${n})`);
    await waitFor(() => app.popup.eval(`[...document.querySelectorAll('.card .body')].every(b => b.textContent.startsWith('[ChatGPT Firm]')) && document.querySelectorAll('.card .btn-secondary:not([disabled])').length === 2`), { what: "Firm drafts" });
    await closePopup(app);
    p.close();
    return "follow-up kept context, Proofread gave one draft, five quick clicks ended on Firm";
  });

  await test("gpt-3", "Switching between Claude and ChatGPT applies to the next popup", async () => {
    const order = [];
    for (const [provider, prefix, badge] of [["claude", "[Clear] Draft A:", "Claude"], ["chatgpt", "[ChatGPT Clear] Draft A:", "ChatGPT"], ["claude", "[Clear] Draft A:", "Claude"]]) {
      await app.popup.eval(`window.__TAURI_INTERNALS__.invoke('set_provider', { provider: '${provider}' })`);
      await sleep(800);
      const p = await openOn(app, `switch to ${provider}`, "g3");
      const drafts = await waitDrafts(app, 2);
      const b = await app.popup.eval(`document.querySelector('.brand .via')?.textContent`);
      assert(drafts[0].startsWith(prefix) && b === badge, `${provider}: "${drafts[0].slice(0, 30)}" badge ${b}`);
      order.push(badge);
      await closePopup(app);
      p.close();
    }
    const saved = JSON.parse(readFileSync(join(app.dir, "config", "settings.json"), "utf8"));
    assert(saved.provider === "claude", `saved ${saved.provider}`);
    await app.popup.eval(`window.__TAURI_INTERNALS__.invoke('set_provider', { provider: 'chatgpt' })`);
    await sleep(800);
    return `drafts came from ${order.join(", then ")}; the choice is saved`;
  });

  await test("gpt-4", "ChatGPT failures show plain messages that name ChatGPT", async () => {
    const out = [];
    for (const [mode, re] of [["signed_out", /ChatGPT is not signed in.*codex login/], ["limit", /ChatGPT plan limit/], ["offline", /ChatGPT cannot reach the internet/], ["badmodel", /ChatGPT model is not available/]]) {
      app.setMode(mode);
      // The spare that is already running started in the old mode, so use it once and close it.
      const warm = await openOn(app, `${mode} warm-up`, "g4");
      await sleep(400);
      await closePopup(app);
      warm.close();
      await sleep(600);
      const p = await openOn(app, `${mode} check`, "g4b");
      const msg = await waitFor(() => app.popup.eval(`document.querySelector('.banner.error p')?.textContent`), { what: `${mode} banner` });
      assert(re.test(msg), `${mode}: ${msg}`);
      out.push(`${mode}: "${msg}"`);
      await closePopup(app);
      p.close();
    }
    app.setMode("normal");
    return out.join("; ");
  });

  await app.stop();
  app = await startApp({ name: "oneprovider", settings: { claude_path: FAKE, codex_path: "C:\\nope\\codex.exe" } });
  await test("gpt-5", "Wizard with only Claude installed picks it and offers Codex", async () => {
    const wz = await Cdp.connect((u) => u.includes("wizard.html"));
    await waitFor(() => wz.eval(`document.querySelector('h1')?.textContent === 'Welcome to Magic Mend'`), { what: "welcome" });
    await wz.click("#next");
    await waitFor(() => wz.eval(`document.querySelector('.provider[data-key="claude"]').classList.contains('chosen')`), { what: "Claude chosen", timeout: 15000 });
    const gpt = await wz.eval(`(() => { const c = document.querySelector('.provider[data-key="chatgpt"]'); return { state: c.querySelector('.pstate').textContent, buttons: [...c.querySelectorAll('button')].map(b => b.textContent), help: c.querySelector('.help')?.textContent }; })()`);
    assert(gpt.state === "Not installed" && gpt.buttons.includes("Get Codex") && gpt.buttons.includes("Browse..."), JSON.stringify(gpt));
    assert(!(await wz.eval(`document.getElementById('next').disabled`)), "Next disabled");
    wz.close();
    return `ChatGPT card: "${gpt.help}"`;
  });

  await app.stop();

  // Wizard tests run on a fresh data folder.
  await test("21-23", "Wizard: fresh install, Claude problems, closed early, practice box", async () => {
    // A first-run settings file with only the fake claude path, so the user's real claude
    // (which may be signed in) is never used here.
    // Both fakes start signed out, so both cards show the fix. The user's real commands are never used.
    let w = await startApp({ name: "wizard", mode: "signed_out", settings: { claude_path: FAKE, codex_path: FAKE_CODEX } });
    const wiz = await Cdp.connect((u) => u.includes("wizard.html"));
    await waitFor(() => wiz.eval(`document.querySelector('h1')?.textContent === 'Welcome to Magic Mend'`), { what: "welcome step" });
    // Closed early: the hotkey must reopen the wizard and capture nothing.
    await wiz.click(".close");
    await sleep(500);
    const p = await startPad("closed early check", "t23");
    const seq = clipSeq();
    hotkey();
    await sleep(1500);
    assert(clipSeq() === seq, "clipboard touched before setup passed");
    assert(w.fakeLog().filter((l) => l.argv).length === 0, "claude started before setup passed");
    const wiz2 = await Cdp.connect((u) => u.includes("wizard.html"));
    const step = await waitFor(() => wiz2.eval(`document.querySelector('h1')?.textContent`), { what: "wizard reopened" });
    assert(step === "Welcome to Magic Mend", `reopened at ${step}`);
    p.close();
    await wiz2.click("#next");
    await waitFor(() => wiz2.eval(`document.querySelector('h1')?.textContent === 'Connect your AI'`), { what: "connect step" });
    // Signed out: the fix is shown.
    const card = (k) => `document.querySelector('.provider[data-key="${k}"]')`;
    const retry = (k) => wiz2.eval(`[...${card(k)}.querySelectorAll('button')].find(b => b.textContent === 'Retry').click()`);
    for (const k of ["claude", "chatgpt"]) {
      await waitFor(() => wiz2.eval(`${card(k)}.querySelector('.help')?.textContent.includes('not signed in')`), { what: `${k} sign-in help`, timeout: 15000 });
    }
    const chatgptHelp = await wiz2.eval(`${card("chatgpt")}.querySelector('.help').textContent`);
    assert(/codex login/.test(chatgptHelp), `ChatGPT help: ${chatgptHelp}`);
    assert(await wiz2.eval(`document.getElementById('next').disabled`), "Next enabled with nothing ready");
    // Claude missing: point at a bad path, retry, see the fix; then a good path passes.
    await wiz2.eval(`window.__TAURI_INTERNALS__.invoke('set_claude_path', { path: 'C:\\\\nope\\\\claude.exe' })`);
    await retry("claude");
    await waitFor(() => wiz2.eval(`${card("claude")}.querySelector('.help')?.textContent.includes('cannot find')`), { what: "missing help" });
    const hasBrowse = await wiz2.eval(`[...${card("claude")}.querySelectorAll('button')].some(b => b.textContent === 'Browse...')`);
    assert(hasBrowse, "no Browse button");
    w.setMode("normal");
    await wiz2.eval(`window.__TAURI_INTERNALS__.invoke('set_claude_path', { path: ${JSON.stringify(FAKE)} })`);
    await retry("claude");
    await retry("chatgpt");
    // Both detected and ready: Claude is picked first, the user switches to ChatGPT.
    for (const k of ["claude", "chatgpt"]) {
      await waitFor(() => wiz2.eval(`${card(k)}.classList.contains('ready')`), { what: `${k} ready`, timeout: 15000 });
    }
    assert(await wiz2.eval(`${card("claude")}.classList.contains('chosen')`), "Claude not preselected");
    await wiz2.click('.provider[data-key="chatgpt"]');
    assert(await wiz2.eval(`${card("chatgpt")}.getAttribute('aria-checked') === 'true'`), "ChatGPT not chosen");
    await waitFor(() => wiz2.eval(`!document.getElementById('next').disabled`), { what: "connect passed", timeout: 15000 });
    const t0 = Date.now();
    await wiz2.click("#next"); // hotkey
    await wiz2.click("#next"); // defaults
    await sleep(200);
    await wiz2.click("#next"); // try it
    await waitFor(() => wiz2.eval(`document.querySelector('h1')?.textContent === 'Try it'`), { what: "try step" });
    await wiz2.click(".practice");
    // DevTools clicks do not bring a window to the front, so do it the way a user would.
    pad("focus", "Magic Mend setup");
    await waitFor(() => fg()?.title === "Magic Mend setup", { what: "wizard in front", timeout: 4000 });
    await wiz2.eval(`document.querySelector('.practice').focus(), document.querySelector('.practice').select(), 1`);
    await sleep(200);
    hotkey();
    await waitFor(popupVisible, { what: "popup over the wizard" });
    await waitDrafts(w, 2);
    await w.popup.click(".card .btn-primary");
    await waitFor(() => wiz2.eval(`document.querySelector('.practice').value.startsWith('[ChatGPT Clear] Draft A:')`), { what: "practice box replaced by ChatGPT" });
    const ok = await wiz2.eval(`!!document.querySelector('.check.ok')`);
    assert(ok, "wizard did not confirm the change");
    const practiceMs = Date.now() - t0;
    await wiz2.click("#next");
    await waitFor(() => wiz2.eval(`document.querySelector('h1')?.textContent === 'You are all set'`), { what: "done step" });
    await wiz2.eval(`[...document.querySelectorAll('button')].find(b => b.textContent === 'Finish').click()`);
    await sleep(800);
    const saved = JSON.parse(readFileSync(join(w.dir, "config", "settings.json"), "utf8"));
    assert(saved.setup.version === 1 && saved.setup.completed.includes("connect"), "setup progress not saved");
    assert(saved.provider === "chatgpt", `provider saved as ${saved.provider}`);
    wiz.close();
    wiz2.close();
    await w.stop();
    return `closed early: nothing captured or started; both providers detected, signed-out and missing fixes shown, ChatGPT chosen and saved; practice Replace by ChatGPT worked ${practiceMs} ms after Connect passed`;
  });

  // Report.
  const passed = results.filter((r) => r.ok).length;
  console.log(`\n${passed}/${results.length} passed`);
  const lines = [
    "# Windows end-to-end results",
    "",
    `Run on ${new Date().toISOString().slice(0, 10)} with \`node tests/e2e/run.mjs\` against the debug build with the fake claude and the fake codex.`,
    "",
    "| Test | What | Result | Notes |",
    "| --- | --- | --- | --- |",
    ...results.map((r) => `| ${r.id} | ${r.name} | ${r.ok ? "Pass" : "**Fail**"} | ${r.note.replace(/\|/g, "/")} |`),
    "",
  ];
  if (!filter) {
    mkdirSync(join(ROOT, "docs", "test-results"), { recursive: true });
    writeFileSync(join(ROOT, "docs", "test-results", "e2e-windows.md"), lines.join("\n"));
  }
  process.exit(passed === results.length ? 0 : 1);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
