// Windows end-to-end acceptance tests. They drive the real app, the real clipboard, and real
// key presses against a test window, with a fake claude so no login is needed.
// Run: node tests/e2e/run.mjs [filter]
// The tests take over the keyboard and the foreground window for a few minutes.

import { writeFileSync, mkdirSync, readFileSync, existsSync } from "node:fs";
import { join } from "node:path";
import { execFileSync, spawn } from "node:child_process";
import {
  ROOT, FAKE, sleep, waitFor, keys, clipSet, clipGet, clipSeq, fg, hotkey,
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
  await waitFor(popupVisible, { what: "popup in front" });
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
  console.log("Windows end-to-end tests (fake claude)\n");
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
    await waitFor(() => p.text() === drafts[0], { what: "text replaced" });
    assert(!p.text().endsWith("\n"), "trailing line break");
    await sleep(700);
    c = clipGet();
    assert(c.text === "ORIGINAL-CLIP-123" && c.custom, `clipboard after Replace: ${JSON.stringify(c)}`);
    assert(!popupVisible(), "popup still open after Replace");
    const k = p.keys();
    assert(enterKeys(k).length === 0, `Enter reached the app: ${enterKeys(k)}`);
    assert(k.includes("keydown ctrl+0x43") && k.includes("keydown ctrl+0x56"), `copy and paste keys missing: ${k.join(", ")}`);
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
    assert(/engine$/i.test(cwd), `cwd is ${cwd}`);
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
    await waitFor(() => clipGet().text.startsWith("[Simple] Draft A: hi"), { what: "Copy put the draft on the clipboard" });
    await waitFor(() => !popupVisible(), { what: "popup closes after Copy", timeout: 3000 });
    p.close();
    return "Copy left the draft on the clipboard as intended";
  });

  await test("7", "Fix errors only gives one draft", async () => {
    const p = await openOn(app, "i has a eror", "t7");
    await waitDrafts(app, 2);
    await app.popup.click('.switch[data-toggle="Fix errors only"]');
    await waitFor(() => app.popup.eval(`document.querySelectorAll('.card').length === 1 && document.querySelector('.card .body').textContent.includes('Draft A')`), { what: "one draft" });
    const req = app.fakeLog().filter((l) => l.message?.includes("Fix errors only")).pop();
    assert(req, "request without the toggle");
    await closePopup(app);
    p.close();
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
    return `one spare after Replace; next popup visible in ${visibleMs} ms (includes key and helper overhead), drafts in ${firstDraftMs} ms`;
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
    const want = "[Simple] Draft A: First line of the draft.\nSecond line of the draft.\n\nLast line.";
    await waitFor(() => p.text() === want, { what: "multi-line text pasted" });
    assert(enterKeys(p.keys()).length === 0, "Enter reached the app");
    p.close();
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

  await app.stop();

  // Wizard tests run on a fresh data folder.
  await test("21-23", "Wizard: fresh install, Claude problems, closed early, practice box", async () => {
    let w = await startApp({ name: "wizard", fresh: true, mode: "signed_out" });
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
    await waitFor(() => wiz2.eval(`document.querySelector('h1')?.textContent === 'Connect Claude'`), { what: "connect step" });
    // Signed out: the fix is shown.
    await waitFor(() => wiz2.eval(`document.querySelector('.help')?.textContent.includes('not signed in')`), { what: "sign-in help", timeout: 15000 });
    // Claude missing: point at a bad path, retry, see the fix; then a good path passes.
    await wiz2.eval(`window.__TAURI_INTERNALS__.invoke('set_claude_path', { path: 'C:\\\\nope\\\\claude.exe' })`);
    await wiz2.eval(`[...document.querySelectorAll('button')].find(b => b.textContent === 'Retry').click()`);
    await waitFor(() => wiz2.eval(`document.querySelector('.help')?.textContent.includes('cannot find')`), { what: "missing help" });
    const hasBrowse = await wiz2.eval(`[...document.querySelectorAll('button')].some(b => b.textContent === 'Browse...')`);
    assert(hasBrowse, "no Browse button");
    w.setMode("normal");
    await wiz2.eval(`window.__TAURI_INTERNALS__.invoke('set_claude_path', { path: ${JSON.stringify(FAKE)} })`);
    await wiz2.eval(`[...document.querySelectorAll('button')].find(b => b.textContent === 'Retry').click()`);
    await waitFor(() => wiz2.eval(`!document.getElementById('next').disabled`), { what: "connect passed", timeout: 15000 });
    const t0 = Date.now();
    await wiz2.click("#next"); // hotkey
    await wiz2.click("#next"); // defaults
    await sleep(200);
    await wiz2.click("#next"); // try it
    await waitFor(() => wiz2.eval(`document.querySelector('h1')?.textContent === 'Try it'`), { what: "try step" });
    await wiz2.click(".practice");
    await wiz2.eval(`document.querySelector('.practice').select(), 1`);
    await sleep(200);
    hotkey();
    await waitFor(popupVisible, { what: "popup over the wizard" });
    await waitDrafts(w, 2);
    await w.popup.click(".card .btn-primary");
    await waitFor(() => wiz2.eval(`document.querySelector('.practice').value.startsWith('[Simple] Draft A:')`), { what: "practice box replaced" });
    const ok = await wiz2.eval(`!!document.querySelector('.check.ok')`);
    assert(ok, "wizard did not confirm the change");
    const practiceMs = Date.now() - t0;
    await wiz2.click("#next");
    await waitFor(() => wiz2.eval(`document.querySelector('h1')?.textContent === 'You are all set'`), { what: "done step" });
    await wiz2.eval(`[...document.querySelectorAll('button')].find(b => b.textContent === 'Finish').click()`);
    await sleep(800);
    const saved = JSON.parse(readFileSync(join(w.dir, "config", "settings.json"), "utf8"));
    assert(saved.setup.version === 1 && saved.setup.completed.includes("connect"), "setup progress not saved");
    wiz.close();
    wiz2.close();
    await w.stop();
    return `closed early: nothing captured or started; missing and signed-out fixes shown; practice Replace worked ${practiceMs} ms after Connect passed`;
  });

  // Report.
  const passed = results.filter((r) => r.ok).length;
  console.log(`\n${passed}/${results.length} passed`);
  const lines = [
    "# Windows end-to-end results",
    "",
    `Run on ${new Date().toISOString().slice(0, 10)} with \`node tests/e2e/run.mjs\` against the debug build and the fake claude.`,
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
