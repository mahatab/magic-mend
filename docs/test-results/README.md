# Test results by milestone

Updated 2026-10-23. Details for each suite are in the files next to this one.

## M1 Engine harness

| Check | Result |
| --- | --- |
| Engine flags verified against `claude --help` (2.1.296) | Done. Differences from the spec are listed in NOTES.md (`--system-prompt-file` is hidden, `--safe-mode` added, `--bare` refused because it drops the subscription login). |
| Stream-json protocol (init, partial deltas, result, error lines) | Probed against the real CLI; recorded output is used in the engine unit tests. |
| Warm-up behavior | The CLI emits `init` only after the first message, so a spare cannot report ready by itself. |
| Timing report for 20 messages, Haiku and Sonnet, with and without priming | **Waiting for sign-in.** The CLI's login on this computer has expired (`claude auth status` reports `loggedIn: false`). Run `claude auth login`, then `mm-harness timing`. |
| Setup isolation (acceptance test 18) | **Waiting for sign-in.** `mm-harness isolation` is ready. |

## M2 Popup UI

| Check | Result |
| --- | --- |
| Layout, chips, toggles, cards, buttons, streaming, skeleton | Pass (mock backend in the browser) |
| Light and dark themes | Pass |
| Reduced motion | All animations are turned off by `prefers-reduced-motion`; the popup is visible with no animation at all |
| Five rapid chip clicks send one request | Pass |
| Enter and Space on a focused Replace or Copy do nothing | Pass |
| Accessibility: axe-core on popup (light and dark), wizard, settings | 0 violations after fixes (accent darkened to pass AA, hidden heading, landmarks, labels) |

## M3 and M4 Windows end to end

See [e2e-windows.md](e2e-windows.md). 16 of 17 passed in the first full run; the wizard test failed because the computer locked itself during the run (the lock screen blocks hotkeys and key simulation). It needs a rerun on an unlocked desktop.

| Acceptance test | Status |
| --- | --- |
| 1 Notepad style text box, undo, clipboard restored | Pass |
| 2, 3, 4 WhatsApp, Gmail, Messenger, LinkedIn, Outlook | Manual: needs the real apps and accounts. Channel detection is covered by unit tests. |
| 5 Enter safety | Pass |
| 6 No selection | Pass |
| 7 Fix errors only (one draft) | Pass. Wording quality needs the live engine. |
| 8 Output rules across 20 outputs | Checker logic: pass (unit tests). Live outputs: waiting for sign-in. |
| 9 Facts kept | Waiting for sign-in (`mm-harness timing` checks every sample) |
| 10 Follow-ups | Flow: pass. Live wording: waiting for sign-in (`mm-harness quality`). |
| 11 Warm spare | Pass |
| 12 Failures: missing, signed out, offline, plan limit, timeout | Pass |
| 13 Privacy: no text on disk, no app sockets | Pass |
| 14 Resources | Measured: app 43 MB plus WebView2; real claude spare to be measured after sign-in |
| 15 Elevated apps | Detection and fallback are built; not automated, because it needs an administrator window and a UAC prompt |
| 16 Terminals | Pass |
| 17 Held modifiers | Pass |
| 18 Setup isolation | Waiting for sign-in |
| 19 Rapid chip changes | Pass |
| 20 Line breaks | Pass |
| 21, 22, 23 Wizard | Needs a rerun on an unlocked desktop |

## M5 Mac port

Mac code type-checks for `aarch64-apple-darwin` on Windows. CI builds and tests it on a real Mac (`workflow_dispatch` with `mac=true`). Running the acceptance tests on a Mac needs a Mac.

## Unit tests

- Rust: 45 pass (`cargo test --lib`)
- Frontend: 19 pass (`npm test`), including a scan of all sources, comments, and docs for contractions and em dashes
