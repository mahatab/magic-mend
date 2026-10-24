# Test results

Updated 2026-10-24 for v1.1.0 (Claude and ChatGPT). Details for each suite are in the files next to this one and in the timing reports.

## Summary

| Suite | Result |
| --- | --- |
| Rust unit tests (`cargo test --lib`) | 53 pass, on Windows and on macOS (CI) |
| UI unit tests (`npm test`), including a scan of every source, comment, and doc for contractions and em dashes | 19 pass |
| Windows end to end ([e2e-windows.md](e2e-windows.md)): real app, real clipboard, real keys, fake Claude and fake Codex | 23 pass |
| Live Claude: 20 messages, Haiku and Sonnet, cold, warm, primed ([report](../M1-timing-report.md)) | pass |
| Live ChatGPT: 20 messages, GPT-5.6-Luna, GPT-6-Luna, GPT-6.1-Sol, cold and warm ([report](../ChatGPT-timing-report.md)) | pass |
| Setup isolation, live: marker in user CLAUDE.md plus a hook (Claude), marker in `~/.codex/AGENTS.md` (ChatGPT) | pass for both; control runs picked up the marker |
| Accessibility (axe-core): popup light and dark, wizard, settings | 0 violations |
| macOS: universal build and unit tests in CI | pass |

## Acceptance tests from the spec

| Test | Claude | ChatGPT | How |
| --- | --- | --- | --- |
| 1 Text box: replace, Ctrl+Z restores, clipboard restored | Pass | Pass | End to end |
| 2, 3, 4 WhatsApp, Gmail, Messenger, LinkedIn, Outlook | Manual | Manual | Needs the real apps and accounts; channel rules are unit tested |
| 5 Enter never reaches the app underneath | Pass | Pass (same popup) | End to end |
| 6 No selection | Pass | Pass (same popup) | End to end |
| 7 Fix errors only | Pass | Pass | End to end, plus live: both correct "i has send" to "I sent" (Claude Haiku does not, so Sonnet is the Claude default) |
| 8 No contractions or em dashes | Pass: 90 of 90 live runs | Pass: 69 of 69 live runs | Live harness |
| 9 Facts kept | Pass (Sonnet) | Pass (GPT-6-Luna) | Live harness, flags read by hand |
| 10 Follow-ups | Pass | Pass | Live harness and end to end |
| 11 Warm spare, speed | Pass: drafts 1.6 s | Pass: drafts 1.6 s | Popup visible in 70 to 125 ms |
| 12 Failures: missing, signed out, offline, plan limit, timeout | Pass | Pass, plus "model not on your plan" | End to end |
| 13 Privacy: no text on disk, no app sockets | Pass | Pass (same app process) | End to end |
| 14 Resources | App 43 MB, WebView2 about 360 MB, claude spare 182 MB | codex spare similar | Measured |
| 15 Elevated apps | Built, not automated | Same | Needs an administrator window and a UAC prompt |
| 16 Terminals | Pass | Pass (same capture path) | End to end |
| 17 Held modifiers | Pass | Pass (same capture path) | End to end |
| 18 Setup isolation | Pass | Pass | Live harness |
| 19 Rapid chip changes | Pass | Pass | End to end (ChatGPT interrupts stale turns) |
| 20 Line breaks | Pass | Pass (same paste path) | End to end |
| 21, 22, 23 Wizard | Pass | Pass | End to end: both detected, fixes for missing and signed out, choice saved, practice box filled by ChatGPT |
| New: switch between Claude and ChatGPT | Pass | Pass | End to end: the next popup uses the new choice |
| New: only one installed | Pass | Pass | End to end: the wizard picks the installed one and offers the other's install |

## Bugs found by testing and fixed

- **Paste could silently do nothing** (about 1 Replace in 6 on this computer): clipboard monitors briefly lock the clipboard after the draft is written, and an app that pastes at that moment gets nothing. Magic Mend now waits until the clipboard is free before sending the paste shortcut. 10 of 10 runs passed afterward, and the full suite passed.
- **Practice box ignored the hotkey**: Tauri reported the wizard as not focused while Windows had it in front. Magic Mend now checks the real foreground window.
- **Capture waited 10 seconds** when Windows reported a letter key as stuck after a lock screen. Capture now waits only for modifier keys.
- **Test safety**: the end-to-end tests now refuse to send any key unless one of their own windows is in front, so a test can never type into an app someone is using.
