# Windows end-to-end results

Run on 2026-10-25 with `node tests/e2e/run.mjs` against the debug build with the fake claude and the fake codex.

| Test | What | Result | Notes |
| --- | --- | --- | --- |
| 1 | Replace in a text box, undo, clipboard restored | Pass | text replaced, Ctrl+Z restored it, clipboard (text and custom format) restored |
| hard-rule-3 | Engine uses the isolation flags and its own empty folder | Pass | cwd engine/claude |
| 5 | Enter in the popup never reaches the app underneath | Pass | Enter submitted the follow-up in the popup only; focused Replace ignored Enter; Esc closed |
| 6 | No selection: editable box, Replace disabled, Copy works | Pass | Copy left the draft on the clipboard as intended |
| 7 | Fix errors only gives one draft | Pass |  |
| 11 | Warm spare: a new process is ready after Replace | Pass | one spare after Replace; popup visible 8 ms after the hotkey (app timing; 185 ms seen by the test), drafts 427 ms |
| edge | Original window closed: Replace turns into Copy only | Pass | "Copy only" with "The original window was closed, so Replace is off. Use Copy." |
| 17 | Held hotkey still captures and sends no stray shortcut | Pass | selection captured after a 1.5 s hold |
| 19 | Rapid chip changes end with drafts for the last chip only | Pass | five clicks, one request, Firm drafts shown |
| 20 | Multi-line draft pastes with no trailing line break and no Enter | **Intermittent** | the paste was missed in this run. Rerun eight times on the code before and after the instant popup change: 7 of 8 and 6 of 8 passed, so the problem predates the change and is under investigation. No Enter ever reached the app |
| 12a | Failure: signed out shows a plain message | Pass | "Claude is not signed in. Open a terminal, run claude, sign in, and then click Retry." then Retry worked after signing in |
| 12b | Failure: plan limit and offline show plain messages | Pass | limit: "Your Claude plan limit has been reached. Try again when your limit resets."; offline: "Claude cannot reach the internet. Check your connection and click Retry." |
| 12c | Failure: claude missing shows the fix | Pass | "Magic Mend cannot find the claude command. Install Claude Code, or set the path in Settings." |
| 12d | Failure: timeout after 30 seconds with Retry | Pass | "Claude took longer than 30 seconds. Click Retry." after about 31 s |
| 16 | Terminals: the hotkey does nothing | Pass | no popup and no copy in WindowsTerminal.exe, WindowsTerminal.exe |
| 13 | Privacy: no message text on disk, no network connections from the app | Pass | 3 app files scanned, 0 hits; app process sockets: 0 TCP, 0 UDP |
| 14 | Resources: idle memory with the warm spare | Pass | app 43 MB, WebView2 365 MB, spare (fake) 4 MB |
| gpt-1 | ChatGPT: Replace end to end, isolated and ephemeral | Pass | replaced via ChatGPT; ephemeral read-only thread with our prompt; tools, plugins, hooks, memories, project docs, skills off |
| gpt-2 | ChatGPT: follow-up, Fix errors only, rapid chip changes | Pass | follow-up kept context, Fix errors only gave one draft, five quick clicks ended on Firm |
| gpt-3 | Switching between Claude and ChatGPT applies to the next popup | Pass | drafts came from Claude, then ChatGPT, then Claude; the choice is saved |
| gpt-4 | ChatGPT failures show plain messages that name ChatGPT | Pass | signed_out: "ChatGPT is not signed in. Open a terminal, run codex login, sign in, and then click Retry."; limit: "Your ChatGPT plan limit has been reached. Try again when your limit resets."; offline: "ChatGPT cannot reach the internet. Check your connection and click Retry."; badmodel: "This ChatGPT model is not available on your plan. Pick another model in Settings." |
| gpt-5 | Wizard with only Claude installed picks it and offers Codex | Pass | ChatGPT card: "Magic Mend cannot find the codex command. Install the Codex app or the codex command from OpenAI, or set the path in Settings." |
| 21-23 | Wizard: fresh install, Claude problems, closed early, practice box | Pass | closed early: nothing captured or started; both providers detected, signed-out and missing fixes shown, ChatGPT chosen and saved; practice Replace by ChatGPT worked 1698 ms after Connect passed |
