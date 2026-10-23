# Magic Mend Build Spec

_2026-10-23_

Magic Mend is a small tray app for Windows first and Mac next. Select text in any app, press a hotkey, pick a tone in a polished popup, and replace the text with a human-sounding rewrite from the user's own Claude login. It never sends a message for the user.

**Name:** Magic Mend. Repo and folder name: `magic-mend`. The tray tooltip reads "Magic Mend, press Ctrl+Alt+R" (Ctrl+Option+R on Mac). The popup header shows the Magic Mend name with a small icon of a needle and thread with a sparkle. Magic Mend shortens to MM, a nod to M² Labs.

## Goal and hard rules

The goal is to cut rephrasing from six steps (open Claude, start a new chat, paste, ask, copy, paste back) to three: select, press the hotkey, click Replace.

These rules can never be broken, and every milestone must be tested against them:

1. **Never send a message.** The app never presses Enter, Send, or any submit control in another app. Its only keyboard output into another app is the paste shortcut (Ctrl+V or Cmd+V) after the user clicks Replace.
2. **Replace and Copy are mouse buttons only.** No keyboard shortcut triggers either one. Enter inside the popup's own follow-up box only submits the follow-up inside the popup.
3. **Use only the official, unmodified `claude` command, already signed in by the user.** Never read, copy, or store login tokens or credentials. Never imitate Claude Code requests or change request headers. Personal use, one user, and every call starts from a hotkey press.
4. **No telemetry, no accounts, no network calls** except what the `claude` command makes by itself.
5. **Output rules are always on:** sound human, no contractions, no em dashes, never invent facts, and keep names, numbers, dates, and links exactly as written.
6. **Protect the clipboard.** Restore the user's original clipboard after any internal copy or paste step. The only exception is the Copy button, which intentionally leaves the draft on the clipboard.

Why rule 3 matters: Anthropic blocks third-party tools that reuse subscription logins, and it paused a plan to move `claude -p` usage into a separate monthly credit. Today, `claude -p` still draws from the plan's usage limits. Re-check the [Anthropic Help Center article](https://support.claude.com/en/articles/15036540-use-the-claude-agent-sdk-with-your-plan) before each release.

Speed targets, not yet measured: popup visible within 150 ms of the hotkey, and the first draft within about 4 seconds.

## What past chats show

This comes from about 25 of the user's rephrasing chats between February and October 2026. It is a sample, not every chat.

- **"Rephrase, simple" is the most common request.** It often carries "human" or "respectful" with it.
- **Tone words the user actually types:** simple, human, respectful, warmer, casual and friendly, professional and measured, shorter and to the point, less thankful, more polite, and softer (for example, "make it a suggestion, like 'you can adjust'").
- **Drafts often contain typos and grammar slips, but the wording is sometimes already right.** In one chat the user asked what was wrong with his version and then asked to keep it. "Fix errors only" must be a first-class mode.
- **Edits happen at the word level:** swap one phrase, remove one word, restore one sentence, usually 2 to 5 follow-ups per message. Multi-turn editing is required.
- **Channels:** text, email, WhatsApp, LinkedIn. **Recipients:** landlords and hosts, recruiters, professional contacts, friends, family, and community groups where Islamic greetings are normal. Keep greetings, religious phrases, and names exactly as the user wrote them.
- **Clarifying questions slowed him down** (for example, "who is this for?" and "which channel?"). The app must never ask a question before the first draft.
- **Stated rules:** no contractions, no em dashes, and never send anything for him.

## User flow

1. On first launch, the setup wizard runs (see the setup wizard section). After that, the app starts at login and lives in the system tray. A warm Claude process is already waiting (see the engine section).
2. The user selects text in any app and presses the global hotkey. Defaults: Ctrl+Alt+R on Windows and Ctrl+Option+R on Mac, both configurable.
3. The app saves the current clipboard, waits until the user releases the hotkey's modifier keys (so the app never sends Ctrl+Alt+C by mistake), simulates the copy shortcut, waits for the clipboard change counter to move (the clipboard sequence number on Windows, the change count on Mac, with a timeout of about 400 ms), reads the selection, restores the clipboard, and records the active window (process name and window title).
4. The popup appears on the active window's monitor, shows the original text, preselects the default chips (Simple, Same length), and starts the first draft immediately. The user does not have to click anything first.
5. Two drafts stream into two cards. Each card has a **Replace** button and a **Copy** button.
6. The user can change chips (a chip change regenerates the drafts) or type a follow-up such as "remove the apology". Follow-ups continue the same conversation.
7. The user clicks **Replace** on a draft. The app puts the draft on the clipboard, hides the popup, returns focus to the original window, simulates the paste shortcut, waits about 500 ms so slow apps finish reading the clipboard, and restores the original clipboard. The pasted text never ends with a line break, because some apps treat a pasted line break like Enter. The text sits in the message box and the user presses Send himself.
8. Closing the popup (Esc or the close button) discards the conversation and starts a fresh warm process in the background.

Edge cases the app must handle:

- **Nothing selected:** the popup opens with an empty editable box labelled "Paste or type text". Replace is disabled and Copy still works.
- **Selection cannot be read** (password field, locked app): show one plain line and keep the box editable.
- **Original window closed or lost:** Replace turns into Copy only and says so.
- **Claude not installed, not signed in, or plan limit reached:** show a plain message that names the fix. Never show a stack trace.
- **Hotkey pressed while the popup is open:** focus the existing popup. Never open a second one.
- **Target app clicked while the popup is open:** the original selection may be gone. The app watches for the target window taking focus. If it does, the Replace button changes to "Paste at cursor" and the header says the selection may have changed.
- **Long selection (over 4,000 characters):** say that Magic Mend is built for short messages and ask the user to confirm before the text goes to Claude.

## Popup design

The popup must feel like a native, calm, premium tool. Layout (480 px wide, height grows to a 640 px maximum, then the drafts area scrolls):

```
+--------------------------------------------------+
|  [app icon] Replace will paste into: WhatsApp [x]|
|  ORIGINAL                                        |
|  hi chi, we had a great stay, can we check out   |
|--------------------------------------------------|
|  TONE    (Simple) Respectful Warm Casual         |
|          Professional Firm                       |
|  LENGTH  Shorter  (Same)  Fuller                 |
|  [ ] Fix errors only  [ ] Softer  [ ] Less thankful |
|  CHANNEL  Text v        (auto-detected)          |
|--------------------------------------------------|
|  +--------------------------------------------+  |
|  | Draft A text streams in here               |  |
|  |                       [ Copy ]  [ Replace ]|  |
|  +--------------------------------------------+  |
|  +--------------------------------------------+  |
|  | Draft B text streams in here               |  |
|  |                       [ Copy ]  [ Replace ]|  |
|  +--------------------------------------------+  |
|  Ask for a change...                          (>)|
+--------------------------------------------------+
```

**Visual style**

- Frameless window with 16 px rounded corners, always on top, no taskbar entry, soft shadow.
- Translucent blurred background: acrylic on Windows 11 and native vibrancy on Mac. Use a solid color when the effect is unavailable.
- Follows the OS light or dark theme. All colors are CSS variables defined once as design tokens: neutral surfaces, one accent color (suggested: soft indigo near #5B6CFF), and a muted text color.
- System UI font (Segoe UI Variable on Windows, SF Pro on Mac). Body 14 px. Section labels 11 px, uppercase, slightly spaced.
- Chips are 28 px tall pills. A selected chip has the accent fill. An unselected chip has a thin border. Toggles are small switches.
- Draft cards have a 12 px radius, a subtle shadow, and selectable text.
- **Replace** is the primary button (accent fill). **Copy** is the secondary button (outline). Keep an 8 px gap between them and a 36 px minimum height so neither is easy to hit by mistake. After a click, the button shows "Replaced" or "Copied" for about one second.
- The header always shows the target app name and icon so the user can see where Replace will paste.

**Motion** (all of it disabled when the OS asks for reduced motion)

- Open: 120 ms fade and slight scale up.
- Waiting for the first words: a soft shimmer skeleton in each card.
- Drafts stream in word by word. A chip change cross-fades the cards.

**Focus and keyboard**

- The popup takes keyboard focus when it opens. Tab order is chips, toggles, draft buttons, then the follow-up box.
- Esc closes the popup.
- In the follow-up box, Enter submits the follow-up inside the popup only, and Shift+Enter adds a new line. A visible send arrow does the same. The popup must confirm it has focus before accepting any key, so no keypress can reach the app underneath.
- Replace and Copy have no keyboard shortcut at all.

**Accessibility:** contrast of at least AA, visible focus rings, and screen reader labels on every control.

## Chips and prompt

Every draft sounds human. "Human" is the baseline for all tones, not a chip.

| Group | Option | Meaning |
| --- | --- | --- |
| Tone (pick one, Simple is the default) | Simple | Plain, short, natural words. Direct, no filler. |
| Tone | Respectful | Polite and courteous, for example "could you please". Not stiff. |
| Tone | Warm | Friendly and appreciative with one warm touch, never heavy gratitude. |
| Tone | Casual | Relaxed everyday talk, like a text to a friend. |
| Tone | Professional | Clear, measured, businesslike. Suits recruiters and work contacts. |
| Tone | Firm | A clear position with no hedging. Polite but direct. |
| Length (pick one, Same is the default) | Shorter | Cut to the core, roughly 30 to 50 percent shorter. |
| Length | Same | About the same length as the original. |
| Length | Fuller | Add natural connecting words for clarity. Never add new facts. |
| Toggle | Fix errors only | Correct spelling, grammar, punctuation, and word slips. Keep the user's own words. Ignore the tone and length chips. Return one draft. |
| Toggle | Softer | Turn demands into suggestions, for example "you can adjust" instead of "adjust". |
| Toggle | Less thankful or apologetic | Keep at most one thank-you and one apology, and cut extra courtesy. |

**Channel** (auto-detected from the active app, with a manual override):

- **Text and WhatsApp:** no subject and no sign-off. Add a greeting only if the original has one. Keep emojis only if the original has them.
- **Email:** a short greeting and a sign-off word such as "Best,". Use a name only if it appears in the original. Also return a subject line, shown above the drafts. Replace pastes the body only, and the subject line has its own Copy button.
- **LinkedIn:** slightly more professional, no emojis, short paragraphs.

**Request sent to Claude on each turn**

```
{"text": "<selected text>", "tone": "Simple", "length": "Same", "toggles": ["Softer"], "channel": "Text", "followup": null}
```

**System prompt for the engine**

```
You rewrite short messages for one person. You do not chat and you never ask questions.

Always:
- Sound like a real person wrote it. Plain words. No corporate filler.
- Never use contractions. Write "do not", "I am", "it is".
- Never use em dashes. Use a comma, a period, or "and".
- Keep every fact exactly as written: names, numbers, dates, times, prices, links, addresses.
- Never invent facts, promises, or details. Never add a name that is not in the original.
- Keep the input language. Keep greetings and religious phrases (for example "Assalamu Alaikum") as written.
- Keep the sender's first person voice.
- Do not add emojis unless the original has them.

Each request gives tone, length, toggles, and channel. Apply them.
When a follow-up is given, apply it to the latest drafts and return the full updated set.

Return only JSON: {"variants": [{"text": "..."}, {"text": "..."}], "subject": "..."}
Return two variants that differ in wording, not in meaning. Return one when Fix errors only is on.
Include "subject" only for the email channel.
```

**Output check in the app:** after each response, scan every draft for contractions (n't, 're, 've, 'll, 'm, and it's, that's, there's, let's, he's, she's, what's) and for em dashes. Match both straight and curly apostrophes, and also flag 'd. If any are found, ask the engine once to fix them. If an em dash remains, replace it locally with a comma followed by one space, then remove any double spaces. Drafts stream in before the check can run, so Replace and Copy stay disabled on a draft until it passes. They must never act on a draft that fails the check.

## Architecture

**Recommended stack: Tauri 2** (Rust core, web UI in HTML, CSS, and TypeScript). Reasons: small memory footprint for an always-running tray app, one codebase for Windows and Mac, and full control over the popup's look with web styling. Electron is the fallback if native key simulation proves hard in Rust. Claude Code should confirm the choice in the first milestone and say so if it changes it.

| Component | Job | Notes |
| --- | --- | --- |
| Tray and autostart | Keep the app running at login, show a tray menu (Settings, Run setup again, Pause, Quit) | Tauri tray and autostart plugins |
| Hotkey manager | Register the global hotkey | Tauri global-shortcut plugin. Handle conflicts with a clear message. |
| Selection capture | Save clipboard, simulate copy, read text, restore clipboard | Needs key simulation. Wait for the clipboard to change, with a timeout of about 400 ms. |
| Active window tracker | Record the window that was active at hotkey time (process name, title, handle) | Used for channel detection and for returning focus on Replace |
| Channel detector | Map the active window to Text, Email, WhatsApp, or LinkedIn | Rules table below |
| Engine manager | Own the warm Claude process, send requests, parse streamed replies, respawn after each session | See the engine section |
| Popup window | The UI described in the popup design section | Separate hidden window, shown on hotkey, reused each time |
| Replace and Copy actions | Write clipboard, refocus the original window, simulate paste, restore clipboard | Replace must never press Enter |
| Output checker | Scan drafts for contractions and em dashes | See the chips and prompt section |
| Settings store | Save settings in a local JSON file | No cloud sync |

**Channel detection rules** (check process name first, then window title, case-insensitive; the user can override per message and change the rules in settings):

- Process or title contains Outlook, Thunderbird, Mail, or "Gmail" in a browser title: **Email**
- Process or title contains WhatsApp: **WhatsApp**
- Browser title contains "LinkedIn": **LinkedIn**
- Browser title contains "Messenger" or "Facebook", or the app is Messages, Telegram, or Signal: **Text**
- Anything else: **Text**

**Suggested code layout** (Claude Code may adjust it):

```
magic-mend/
  src-tauri/src/
    main.rs            app setup, tray, autostart
    hotkey.rs          global hotkey
    selection.rs       capture, clipboard save and restore
    window_tracker.rs  active window per platform
    actions.rs         replace and copy
    engine.rs          warm claude process, protocol, respawn
    checker.rs         contraction and em dash check
    settings.rs        load and save settings
  src/                 popup UI (TypeScript, CSS tokens)
  prompts/system.txt   the system prompt from this spec
  docs/SPEC.md         this spec
```

## Claude engine

The app keeps one warm `claude` process waiting in the background so the hotkey does not pay the startup delay. This is the official command on the user's own login, in line with hard rule 3.

**Lifecycle (warm spare)**

1. When the app starts, it launches one spare process and waits until it is ready.
2. When the user presses the hotkey, the popup takes the spare and sends the first request.
3. Follow-ups go to the same process, so the conversation keeps its context.
4. When the user clicks Replace or Copy, or closes the popup, the app kills that process and launches a new spare in the background. Every message therefore starts with a clean context, and details never leak between messages.
5. If the hotkey is pressed before a spare is ready, start one on demand and show the skeleton loader.
6. If a spare dies, respawn it. A setting turns warm mode off, which falls back to starting a process per hotkey press.
7. Recycle the spare after it sits idle for more than 6 hours and after the computer wakes from sleep, so a stale process never answers the hotkey.

**Process settings.** The flag names below come from the Claude Code headless documentation and recent releases. Claude Code must verify each one against `claude --help` and the current docs, and adjust if a name has changed.

| Purpose | Flag or approach |
| --- | --- |
| Stay alive and accept several messages | Print mode with `--input-format stream-json` and `--output-format stream-json` (plus `--verbose` if the output format requires it) |
| Stream words as they arrive | `--include-partial-messages` |
| Fast model | `--model` set to the fastest model that passes the quality tests. Start with the Haiku-class alias and compare with Sonnet. Expose it in settings. |
| Use the rephrase prompt only | Replace the default system prompt with `prompts/system.txt` (`--system-prompt` or `--system-prompt-file`) |
| No tools | Disable all built-in tools (`--tools` with an empty value) |
| No extra connectors | `--strict-mcp-config` with no servers |
| No skills or slash commands | `--disable-slash-commands` |
| No outside project settings | Run from a dedicated empty folder in the app data directory |
| No saved transcripts | `--no-session-persistence`. If the installed version lacks it, delete only the transcript files of the app's own working directory after each session. |
| No personal Claude Code setup | The user-level CLAUDE.md, hooks, plugins, output styles, and MCP servers must not load. Check `--setting-sources` and related flags. Prove it with a test: put a marker line in the user-level CLAUDE.md and a logging hook in user settings, then confirm neither affects the engine. |
| No auto-update or extra traffic | Set `DISABLE_AUTOUPDATER=1` and `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1` in the child process environment (verify the names). |
| No console window on Windows | Spawn with the `CREATE_NO_WINDOW` flag. If `claude` is installed as a .cmd shim, find a way to start it without a visible window. |

**Message format.** Send one JSON line per user message on standard input, using the stream-json shape from the docs:

```
{"type":"user","message":{"role":"user","content":[{"type":"text","text":"<the request JSON from the chips and prompt section>"}]}}
```

Read JSON lines from standard output. Use an incremental JSON parser to pull each variant's text out of the partial reply and show it as it streams. When the reply completes, parse it fully as the JSON object defined in the prompt, then run the output check. If the final reply is not valid JSON, ask the engine once to resend it as valid JSON.

**Overlapping requests.** The user may change chips while drafts are still streaming. Wait 300 ms after the last chip change before sending. The process answers messages in order, so the app counts completed replies and shows only the reply to the newest request. Older replies are discarded when they finish. If the CLI supports an interrupt message in stream-json mode, Claude Code may use it to stop a stale reply sooner.

**Warm-up test (milestone 1).** It is not yet known whether the process does its heavy startup before the first message or when the first message arrives. Measure both: time from spawn to ready, and time from first message to first words. If startup happens lazily, send a tiny priming message such as "Reply with ok" when the spare launches. It costs a few tokens, and the priming exchange sits harmlessly in a context that is thrown away after the session.

**Failures.** Detect and explain each in plain words: `claude` not found, not signed in, plan limit reached, process exit, and a 30 second timeout (with a Retry button). If a process dies mid-session, respawn it and replay the current request once. Never log message text. Log only the error category.

**Engine interface.** Put the engine behind a small interface (`start`, `send`, `stop`) so a second backend can be added later. Do not build any other backend in version 1.

## Platform notes

Build Windows first, but keep platform code behind one interface per feature (hotkey, selection capture, window tracker, paste) so the Mac port only adds implementations.

**Windows**

- Copy and paste are simulated with Ctrl+C and Ctrl+V.
- At hotkey time, record the foreground window handle. On Replace, hide the popup, bring that window back to the front, wait about 80 ms, then paste. Windows restricts which process may take focus, so Claude Code must test this carefully.
- An app running as administrator ignores keys from a normal app. Detect this case and show a plain message with Copy still working.
- Use acrylic or Mica on Windows 11 and a solid background on Windows 10.
- Install with an installer that registers autostart. A signed build is optional for personal use.

**Mac**

- Copy and paste use Cmd+C and Cmd+V. Default hotkey: Ctrl+Option+R.
- Key simulation and window tracking need the Accessibility permission. The setup wizard's permission step explains it, opens the right System Settings pane, and detects when access is granted.
- Show a menu bar item and no Dock icon.
- Prefer a non-activating panel for the popup so the original app stays in front while the popup still accepts typing. If that is not possible, activate the popup and re-activate the original app on Replace.
- Use native window vibrancy for the blurred background.
- A local build is enough for personal use. Sharing the app later needs an Apple Developer ID and notarization.

**Both platforms: finding `claude`.** Apps started from the desktop do not inherit the terminal's PATH. Search the common install locations (for example the user's `.local/bin`, npm global folder, Homebrew folders on Mac) and ask a login shell as a last step. Let the user set the path by hand in settings. On first run, run `claude --version` and show a clear message if it fails.

## Privacy and safety

- Message text and drafts live only in memory during a session. The app never writes them to disk or to logs.
- Logs hold error categories, timings, and the app version only.
- Selected text goes only to the local `claude` process, which sends it to Anthropic under the user's own plan. The app opens no network connections of its own. Add a test that proves it.
- **Excluded apps:** the hotkey does nothing in password managers, remote desktop clients, terminals (Windows Terminal, Command Prompt, PowerShell, Terminal, iTerm2), and any app the user adds. Terminals are excluded because a simulated Ctrl+C can stop a running program and a pasted line break can run a command. Ship a sensible default list.
- **Clipboard:** save and restore every clipboard format the platform allows, not only text. If the original clipboard holds something that cannot be restored, tell the user before Replace runs.
- **Clipboard history:** mark the temporary clipboard values as private so history managers skip them. Claude Code should verify the platform flags: on Windows the `ExcludeClipboardContentFromMonitorProcessing` and `CanIncludeInClipboardHistory` formats, and on Mac the `org.nspasteboard.ConcealedType` type.
- No analytics and no automatic updates without the user's consent.
- No screenshots, no reading of window contents beyond the selected text and the active window's process name and title.

## First-run setup wizard

On first launch, a short wizard runs before the hotkey turns on. It uses the popup's look (same design tokens, blurred background, light and dark themes), shows one step per screen with Back and Next buttons and a step indicator, and aims for under two minutes from install to first rewrite.

1. **Welcome.** One sentence on what Magic Mend does, the promise that it never sends a message, and a note that rewrites use the user's own Claude plan limits.
2. **Connect Claude.** Find the `claude` command, run `claude --version`, then send one tiny test request through the engine. Show a green check when it works. If `claude` is missing, show the official install link, a Browse button to set the path by hand, and a Retry button. If it is not signed in, tell the user to open a terminal, run `claude`, sign in, and click Retry. The wizard never handles the login itself and never reads credentials.
3. **Allow access (Mac only).** Explain why the Accessibility permission is needed (to copy the selection and paste the draft), open the right System Settings pane, and move on by itself once access is granted. If the platform needs an app restart for the permission to apply, say so and offer a Restart button.
4. **Choose your hotkey.** Show the default. The user can press a new combination to change it. Check for conflicts, including the AltGr issue on Windows, and warn in plain words.
5. **Pick your defaults.** Default tone and length, with Simple and Same preselected, and the "Start at login" switch, on by default.
6. **Try it.** A practice box holds a sample message with a few typos. The user selects it, presses the hotkey, picks a draft, and clicks Replace. The wizard confirms the box changed. This tests selection capture, the engine, and paste end to end. A Skip link is available.
7. **Done.** Show where the tray icon (menu bar item on Mac) lives and how to open settings.

**Wizard rules**

- Steps 2 and 3 must pass before the hotkey turns on. Other steps can be skipped, and skipped steps use the defaults.
- If the user closes the wizard early, the app stays in the tray with a "Finish setup" item, and pressing the hotkey reopens the wizard at the first unfinished step.
- Save progress in the settings file (setup version and completed steps). The tray menu has "Run setup again".
- If a later version needs a new step (for example a new permission), show only that step.
- The practice box lives in Magic Mend's own window, so the hotkey must work there during the wizard only.
- All wizard text follows the no-contractions and no-em-dash rule.

## Settings

Store settings in a local JSON file with a simple settings window opened from the tray.

| Setting | Default |
| --- | --- |
| Hotkey | Ctrl+Alt+R on Windows, Ctrl+Option+R on Mac |
| Default tone | Simple |
| Default length | Same |
| Default toggles | All off |
| Number of drafts | 2 |
| Model | Fastest model that passes the quality tests |
| Warm mode | On |
| Theme | Follow the system |
| Start at login | On |
| Popup position | Centered on the active window's monitor, with an option to open near the cursor |
| Excluded apps | Password managers, remote desktop clients, and terminals, editable |
| Channel rules | The rules from the architecture section, editable |
| Path to `claude` | Auto-detected, editable |
| No contractions and no em dashes | On, and each can be turned off |

## Milestones and acceptance tests

Build in this order. After each milestone, run its tests and report the results in a short table before starting the next one.

| Milestone | Scope | Done when |
| --- | --- | --- |
| M1 Engine harness | Command-line program only, no UI. Start the warm process with the settings above, send requests, parse streamed replies, run the output check. | A timing report covers 20 sample messages: spawn to ready, first message to first words, and total time, for the Haiku-class and Sonnet-class models, with and without a priming message. It ends with a go or no-go on warm mode and a model recommendation. |
| M2 Popup UI | The full popup with fake drafts: chips, toggles, cards, buttons, streaming animation, light and dark themes. | It matches the design section and works with reduced motion on. |
| M3 Windows end to end | Hotkey, selection capture, popup, engine, Replace, Copy, clipboard restore, focus return. | Acceptance tests 1 to 8 pass on Windows. |
| M4 Product shell | First-run setup wizard, channel detection, settings window, tray, autostart, excluded apps, failure messages, installer. | Tests 9 to 15 and the wizard tests pass. |
| M5 Mac port | Accessibility step in the setup wizard, panel behavior, menu bar item. | All tests pass on Mac. |
| M6 Polish | Accessibility pass, performance pass, README. | No open defects. |

**Acceptance tests**

1. **Notepad:** select text, press the hotkey, click Replace. The text is replaced, Ctrl+Z restores the original, and the original clipboard is back.
2. **WhatsApp desktop:** Replace puts the text in the message box and nothing is sent.
3. **Browser:** the same check in Gmail compose, Facebook Messenger, and a LinkedIn message. The channel is detected correctly and nothing is sent.
4. **Outlook:** the email channel adds a greeting and sign-off according to the rules.
5. **Enter safety:** with the popup open, pressing Enter in the follow-up box or on any control never reaches the app underneath.
6. **No selection:** the empty-box mode works and Replace is disabled.
7. **Fix errors only:** one draft, with the user's own words kept and only errors corrected.
8. **Output rules:** across 20 outputs there are zero contractions and zero em dashes.
9. **Facts:** names, numbers, dates, and links are unchanged in all test outputs.
10. **Follow-ups:** "remove the apology" changes only that, and the rest stays.
11. **Warm spare:** after Replace, a new spare is ready, and the next hotkey press meets the measured speed target.
12. **Failures:** `claude` missing, signed out, offline, and timeout each show a plain message.
13. **Privacy:** after a session, no message text exists in the app's files or logs, and the app process opens no network connections.
14. **Resources:** report idle memory use with the warm spare running.
15. **Windows elevated apps:** Replace falls back to Copy with a clear message.
16. **Terminals:** the hotkey does nothing in Windows Terminal, PowerShell, and Terminal on Mac.
17. **Held modifiers:** holding the hotkey longer than usual still captures the selection and sends no stray shortcut to the app.
18. **Setup isolation:** a marker in the user-level CLAUDE.md and a test hook in user settings have no effect on the engine.
19. **Rapid chip changes:** clicking five chips quickly ends with drafts for the last selection only.
20. **Line breaks:** Replace never adds a trailing line break, and a multi-line draft pastes without sending in WhatsApp, Messenger, and LinkedIn.
21. **Wizard, fresh install:** the wizard runs, and a first rewrite in the practice box works in under two minutes.
22. **Wizard, Claude problems:** with `claude` missing or signed out, the wizard shows the fix, and Retry passes once it is fixed.
23. **Wizard, closed early:** the hotkey reopens the wizard at the first unfinished step, and nothing is captured or sent to Claude before steps 2 and 3 pass.

**Sample test messages** (write more in the same style for the 20-message set):

- hi, i want to ask when you can send me the document, i need it by friday
- Thank you for the update, I was worried because I could not track the slip
- sorry for late reply, I was out of town for a while
- Assalamu Alaikum, how much is one share this year and when is the last date to pay
- I interviewed at the senior level, not principal, and I would love the list of open roles

## Open decisions and kickoff prompt

**Decisions for Claude Code to confirm or flag in milestone 1**

1. **Tauri or Electron.** Tauri is recommended. Confirm that key simulation works reliably on both Windows and Mac, or say why Electron is better.
2. **Enter in the follow-up box.** Default: Enter submits the follow-up inside the popup. If testing shows any leak to the app underneath, switch to a send button only.
3. **Default model.** Decide between the Haiku-class and Sonnet-class models using the quality tests and timing numbers.
4. **Priming message.** Keep it only if the timing test shows it helps.
5. **Hotkey default.** Check it against common shortcuts in Outlook, Chrome, and WhatsApp, and propose another if it conflicts. Ctrl+Alt acts as AltGr on many non-US Windows keyboard layouts, so Ctrl+Alt+R can type a character there.

**Kickoff prompt to paste into the Claude Code session** (save this spec as `docs/SPEC.md` first):

```
Read docs/SPEC.md fully before writing any code. It is the build spec for Magic Mend, a cross-platform tray app that rewrites selected text in a popup using my own Claude login through the official claude command.

Rules for this session:
- Build in milestone order and start with M1 only. Do not start M2 until I approve the M1 timing report.
- Obey the hard rules in the spec. If any requirement conflicts with them, stop and ask me.
- Verify every claude command flag against claude --help and the current official docs before using it. Tell me about any flag that differs from the spec.
- Do not read, copy, or store my Claude login credentials, and do not imitate Claude Code requests.
- Never add anything that presses Enter or Send in another app.
- All user-facing text, comments, and the README must use no contractions and no em dashes.
- After each milestone, run its acceptance tests and give me a short table of results.
- Do not add features that are not in the spec. Put ideas in a notes file instead.
```
