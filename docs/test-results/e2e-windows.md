# Windows end-to-end results

Run on 2026-10-24 with `node tests/e2e/run.mjs` against the debug build and the fake claude.

| Test | What | Result | Notes |
| --- | --- | --- | --- |
| 1 | Replace in a text box, undo, clipboard restored | Pass | text replaced, Ctrl+Z restored it, clipboard (text and custom format) restored |
| hard-rule-3 | Engine uses the isolation flags and its own empty folder | Pass | cwd data/engine |
| 5 | Enter in the popup never reaches the app underneath | Pass | Enter submitted the follow-up in the popup only; focused Replace ignored Enter; Esc closed |
| 6 | No selection: editable box, Replace disabled, Copy works | Pass | Copy left the draft on the clipboard as intended |
| 7 | Fix errors only gives one draft | Pass |  |
| 11 | Warm spare: a new process is ready after Replace | Pass | one spare after Replace; popup visible 109 ms after the hotkey (app timing; 199 ms seen by the test), drafts 464 ms |
| edge | Original window closed: Replace turns into Copy only | Pass | "Copy only" with "The original window was closed, so Replace is off. Use Copy." |
| 17 | Held hotkey still captures and sends no stray shortcut | Pass | selection captured after a 1.5 s hold |
| 19 | Rapid chip changes end with drafts for the last chip only | Pass | five clicks, one request, Firm drafts shown |
| 20 | Multi-line draft pastes with no trailing line break and no Enter | Pass |  |
| 12a | Failure: signed out shows a plain message | Pass | "Claude is not signed in. Open a terminal, run claude, sign in, and then click Retry." then Retry worked after signing in |
| 12b | Failure: plan limit and offline show plain messages | Pass | limit: "Your Claude plan limit has been reached. Try again when your limit resets."; offline: "Claude cannot reach the internet. Check your connection and click Retry." |
| 12c | Failure: claude missing shows the fix | Pass | "Magic Mend cannot find the claude command. Install Claude Code, or set the path in Settings." |
| 12d | Failure: timeout after 30 seconds with Retry | Pass | "Claude took longer than 30 seconds. Click Retry." after about 31 s |
| 16 | Terminals: the hotkey does nothing | Pass | no popup and no copy in WindowsTerminal.exe, WindowsTerminal.exe |
| 13 | Privacy: no message text on disk, no network connections from the app | Pass | 3 app files scanned, 0 hits; app process sockets: 0 TCP, 0 UDP |
| 14 | Resources: idle memory with the warm spare | Pass | app 43 MB, WebView2 355 MB, spare (fake) 4 MB |
| 21-23 | Wizard: fresh install, Claude problems, closed early, practice box | **Fail** | timed out waiting for popup over the wizard (last: false) |
