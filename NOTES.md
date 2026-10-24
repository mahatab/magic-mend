# Build notes

Decisions, deviations from the spec, and ideas that are not in the spec.

## Decisions

- **Stack: Tauri 2** (Rust core, TypeScript popup), as the spec recommends. Key simulation uses the Win32 `SendInput` API directly, which is reliable, so there is no reason to switch to Electron.
- **System prompt.** `prompts/system.txt` is the spec prompt word for word, followed by the meanings of each tone, length, toggle, and channel copied from the spec tables. Without them the model only sees the option names.
- **Flags checked against `claude --help` (version 2.1.296).**
  - `--system-prompt-file` works but is not listed in `--help`. The app uses it so the prompt never goes through command line quoting.
  - `--safe-mode` is new and turns off CLAUDE.md, skills, plugins, hooks, MCP servers, output styles, and custom agents while keeping the normal login. The app uses it together with `--setting-sources ""`, `--strict-mcp-config`, `--tools ""`, `--disable-slash-commands`, and `--no-session-persistence`.
  - `--bare` was not used: it refuses the subscription login (OAuth), which would break hard rule 3.
  - The stream-json `init` line arrives only after the first message, so a spare cannot report "ready" by itself. See the M1 report.
- **Environment.** The engine removes `CLAUDECODE`, `CLAUDE_CODE_*`, and `CLAUDE_AGENT_SDK_*` variables a parent Claude Code session may have set, then sets `DISABLE_AUTOUPDATER=1` and `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`.
- **Sign-in check.** The wizard runs `claude auth status`, which prints only `loggedIn` and the method. The app never opens credential files.

- **Hotkey default (Ctrl+Alt+R).** It does not clash with a default shortcut in Chrome, Edge, or WhatsApp. In Word and Outlook it normally types the registered sign, so while Magic Mend runs that symbol shortcut is taken; AutoCorrect (R) still works. On keyboard layouts where Ctrl+Alt acts as AltGr and Ctrl+Alt+R types a character (for example Polish), the wizard and Settings warn and suggest Ctrl+Shift+Space. The check asks the active keyboard layout directly.
- **Enter in the follow-up box.** Kept as the spec default. The end-to-end test sends a real Enter key while the popup is in front and confirms nothing reaches the app underneath.
- **Modifier wait.** Capture waits for Ctrl, Alt, Shift, and Win to be released, not the hotkey letter. Windows can report a letter as held long after it was released (after a lock screen, for example), which blocked capture for 10 seconds in testing. A held letter cannot turn Ctrl+C into another shortcut.
- **Mac popup.** Tauri has no non-activating panel, so the spec fallback is used: the popup activates, and Replace re-activates the original app before Cmd+V.
- **Test hooks.** `MAGIC_MEND_DATA_DIR` points the app at a throwaway folder for automated tests. The test window, fake claude, and harness are separate binaries that are not part of the installer.

- **Default model: sonnet, priming off.** From the M1 report: sonnet is faster once warm and handles Fix errors only correctly. Haiku stays available in Settings.

## Deviations

- **Window corners on Windows 11.** With acrylic turned on, Windows draws the frame, and its rounded corner radius is 8 px, not 16 px. When acrylic is not available (Windows 10) the popup draws its own 16 px corners on a solid background.
- **Channel title rule for "Mail".** Title words match whole words only, so "Mailchimp" does not count as email.

## Ideas (not built)

- Expand common contractions locally as a last resort when the engine fix round still leaves one. Today the draft stays blocked with a plain message, as the spec requires.
