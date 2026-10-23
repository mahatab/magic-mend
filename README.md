# Magic Mend

<img src="assets/icon-1024.png" width="72" align="right" alt="">

Select text in any app, press **Ctrl+Alt+R** (Ctrl+Option+R on Mac), pick a tone, and click **Replace**. Magic Mend rewrites the text with your own Claude login through the official `claude` command and puts it back where it came from.

**It never sends a message for you.** Replace only pastes the draft into the text box. You press Send yourself.

Rephrasing goes from six steps (open Claude, new chat, paste, ask, copy, paste back) to three: select, press the hotkey, click Replace.

## Install

### Windows

1. Install [Claude Code](https://claude.com/product/claude-code) and sign in once: open a terminal, run `claude`, and follow the prompts.
2. Run `Magic Mend_1.0.0_x64-setup.exe` (from `src-tauri/target/release/bundle/nsis/` after a build). It installs for your user only and starts at login.
3. The setup wizard opens. It finds `claude`, sends one tiny test rewrite, lets you pick a hotkey and your default tone, and has a practice box to try it end to end. It takes under two minutes.

### Mac

Build it on the Mac (see Build). The setup wizard adds one step: it asks for the Accessibility permission, which Magic Mend needs to copy your selection and paste the draft. A local build is enough for personal use. Sharing it needs an Apple Developer ID and notarization.

## Using it

1. Select text in any app: WhatsApp, Outlook, a browser, Notepad, and so on.
2. Press the hotkey. The popup opens on the same screen and starts two drafts at once.
3. Change the tone or length, flip a toggle, or type a change such as "remove the apology" and press Enter. Follow-ups keep the conversation, so you can edit word by word.
4. Click **Replace** on the draft you like, or **Copy** to keep it on the clipboard.

| Control | What it does |
| --- | --- |
| Tone | Simple (default), Respectful, Warm, Casual, Professional, Firm |
| Length | Shorter, Same (default), Fuller |
| Fix errors only | Corrects spelling and grammar, keeps your own words, returns one draft |
| Softer | Turns demands into suggestions ("you can adjust") |
| Less thankful | At most one thank-you and one apology |
| Channel | Text, Email, WhatsApp, LinkedIn. Detected from the app, and you can change it. Email adds a greeting, a sign-off, and a subject line with its own Copy button. |

Every draft sounds like a person wrote it, never uses contractions or em dashes, and keeps names, numbers, dates, links, greetings, and religious phrases exactly as you wrote them.

**Keys.** Esc closes the popup. Enter in the change box submits the change inside the popup, and Shift+Enter adds a line. Replace and Copy have no keyboard shortcut at all, by design: only a mouse click triggers them.

**Good to know.**

- Nothing selected? The popup opens with an empty box. Paste or type, and Copy works. Replace stays off.
- If you click back into the app while the popup is open, Replace turns into **Paste at cursor**, because your selection may have changed.
- Selections over 4,000 characters ask before anything goes to Claude.
- The hotkey does nothing in terminals, password managers, and remote desktop apps. You can edit that list in Settings.
- Apps running as administrator ignore keys from normal apps. Magic Mend says so, and Copy still works.

## Privacy

- Your text goes only to the `claude` command on this computer, which sends it to Anthropic under your own plan. Magic Mend opens no network connections of its own. Its web views also block every network request through their content security policy.
- Messages and drafts live in memory only. They are never written to disk or to the log. The log holds error types, timings, and the version.
- Magic Mend never reads, copies, or stores your Claude login. It checks sign-in with `claude auth status`, which reports only yes or no.
- Your clipboard is saved before each internal copy or paste and restored afterward, every format included. Temporary values are marked private so clipboard history skips them. The Copy button is the one exception: it leaves the draft on the clipboard on purpose.
- No accounts, no analytics, no automatic updates.

Rewrites count toward your Claude plan limits. Anthropic's rules for using a subscription with tools like this can change, so check the [Help Center article](https://support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-plan) before each release.

## Settings

Open Settings from the tray icon (right-click it). Everything is saved to a local JSON file: `%APPDATA%\com.m2labs.magicmend\settings.json` on Windows, `~/Library/Application Support/com.m2labs.magicmend/settings.json` on Mac.

You can change the hotkey, default tone, length and toggles, number of drafts, model, warm mode, theme, start at login, popup position, excluded apps, channel rules, the path to `claude`, and turn the no-contractions and no-em-dash rules on or off. The tray menu also has Pause, Run setup again, and Quit.

## How it works

Magic Mend is a [Tauri 2](https://tauri.app) app: a Rust core and a web popup in TypeScript.

- **Warm engine.** One `claude` process waits in the background so the hotkey does not pay the start-up delay. The popup takes it, follow-ups reuse it, and after Replace, Copy, or close it is ended and a fresh one starts. Every message begins with a clean context. Spares are recycled after 6 idle hours and after the computer wakes.
- **Isolated.** The process runs from an empty folder with `--safe-mode`, `--setting-sources ""`, `--strict-mcp-config`, `--tools ""`, `--disable-slash-commands`, and `--no-session-persistence`, plus the rewrite prompt in `prompts/system.txt`. Your CLAUDE.md, hooks, plugins, and MCP servers do not load, and no transcript is saved.
- **Output check.** Every draft is scanned for contractions (straight and curly apostrophes) and em dashes. If one is found, Claude is asked once to fix it, then any leftover em dash is replaced locally. Replace and Copy stay off until a draft passes.
- **Capture and paste.** Magic Mend saves the clipboard, waits for you to let go of the hotkey, sends the copy shortcut, waits for the clipboard to change, reads the text, and restores the clipboard. Replace puts the draft on the clipboard, returns focus to the original window, sends the paste shortcut, waits half a second, and restores the clipboard. The copy and paste shortcuts are the only keys Magic Mend ever sends to another app.

## Build

You need Node 22 or later, Rust (stable), and on Windows the Visual Studio C++ build tools and WebView2 (included in Windows 11).

```bash
npm install
npx tauri build
```

The installer lands in `src-tauri/target/release/bundle/`. For a debug build with the web assets inside, run `npx tauri build --debug --no-bundle`.

## Tests

```bash
npm test
```

```bash
cd src-tauri && cargo test --lib
```

| Suite | Command | What it covers |
| --- | --- | --- |
| Rust unit tests | `cargo test --lib` | Output checker, request and reply parsing, streaming extraction, draft flow (stale replies, the one fix round, JSON repair, timeouts), channel detection, excluded apps, settings, hotkeys, error messages |
| Frontend unit tests | `npm test` | Popup logic, Replace button states, hotkey capture, and a scan of the whole source tree for contractions and em dashes |
| Windows end to end | `node tests/e2e/run.mjs` | The real app, real clipboard, and real keys against a test window, with a fake `claude`. Covers acceptance tests 1, 5, 6, 7, 11 to 14, 16, 17, 19, 20, and 21 to 23. It takes over the keyboard for a few minutes, so leave the computer alone while it runs. |
| Engine harness | `mm-harness timing`, `quality`, `isolation` | Live Claude timings for the M1 report, follow-up and fix-errors quality, and proof that your personal Claude Code setup does not reach the engine |

Results are in [docs/test-results](docs/test-results).

## Project layout

```
src-tauri/src/
  app.rs             tray, hotkey, popup, wizard, settings, engine thread
  engine.rs          the claude process, flags, stream-json reader, error types
  engine_manager.rs  warm spare lifecycle
  drafts.rs          which reply is current, fix round, JSON repair, timeout
  checker.rs         contraction and em dash check
  protocol.rs        request and reply JSON, streaming extraction
  selection.rs       capture with clipboard save and restore
  actions.rs         Replace and Copy
  channel.rs         channel rules and excluded apps
  settings.rs        settings file and setup progress
  hotkey.rs          hotkey parsing and conflict warnings
  platform/          Windows and Mac implementations
  bin/               harness, test window, fake claude (tests only)
src/                 popup, wizard, and settings UI
prompts/system.txt   the rewrite prompt
docs/SPEC.md         the build spec
NOTES.md             decisions, deviations, and ideas
```
