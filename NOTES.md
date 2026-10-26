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
- **Test hooks.** `MAGIC_MEND_DATA_DIR` points the app at a throwaway folder for automated tests. The test window, fake claude, fake codex, and harness are separate binaries that build only with `--features test-tools`, so they never end up in the installer.
- **Popup before the keys are released.** The copy shortcut can only be sent after Ctrl and Alt are let go, and real use showed people hold the hotkey for 0.5 to 7 seconds (median 2.4 s), so a popup that waited for the copy seemed to appear only when the drafts did. On Windows the popup now shows at once without taking focus (`SW_SHOWNOACTIVATE`), with placeholders and "Reading your selection...", then the selection is copied from the app still in front, and the popup fills in and takes focus. Ctrl+C is sent only if the original window is still in front, so clicking the popup early can never copy from it. Esc during the wait ends the session and the popup does not come back. On the Mac, showing a window activates the app, so the popup still opens after the copy there.
- **Clipboard wait before paste.** Clipboard history and other monitors open the clipboard to read each new value. An app that pastes at that moment finds it locked and silently pastes nothing, which happened about once in six Replace runs in testing. Replace now waits (up to 400 ms) until Magic Mend can open the clipboard twice in a row before it sends the paste shortcut. The Mac pasteboard has no lock, so the wait is a no-op there.

- **Default model: sonnet, priming off.** From the M1 report: sonnet is faster once warm and handles Proofread (then called "Fix errors only") correctly. Haiku stays available in Settings.

## ChatGPT support (v1.1.0)

- **Engine: OpenAI's official `codex app-server`.** It is the counterpart of `claude -p` with stream-json: one warm process, one ephemeral conversation (thread) per popup, words streamed as `item/agentMessage/delta`, follow-ups in the same thread, and `turn/interrupt` for replies that are no longer needed. The user signs in with `codex login` (ChatGPT); Magic Mend never touches the login and checks it with `codex login status`.
- **Why not `codex exec`.** It works (about 3 s per draft) but runs once per request, sends no words until the end, and needs saved sessions for follow-ups, which conflicts with the no-transcripts rule.
- **Isolation.** The rewrite prompt replaces Codex's own instructions (`baseInstructions`), the thread is ephemeral, the sandbox is read-only, and these are switched off: plugins, apps, skills (`skills.include_instructions=false`), memories, hooks, multi-agent, the shell and exec tools, image generation, browser and computer use, web search, AGENTS.md files (`project_doc_max_bytes=0`), and the permissions, environment, apps, and collaboration instruction blocks. The prompt the model sees went from about 15,000 tokens to about 3,100. `codex debug prompt-input` was used to verify each step.
- **The user's MCP servers.** `mcp_servers={}` does not clear servers that are already configured, so Magic Mend reads the server names from `codex mcp list --json` and switches each off with `mcp_servers.<name>.enabled=false`. A server that comes from a desktop app feature cannot be overridden by name (Codex reports "invalid transport"), so every override is checked with a harmless `codex mcp list` first and kept only when Codex accepts it. Those feature servers are already off through their feature flags.
- **Feature names change between versions.** Only flags that the installed Codex lists in `codex features list` are passed, so a renamed feature never stops the engine. Two `include_*_usage_instructions` keys that are not valid top-level settings were dropped after testing.
- **Finding `codex`.** It ships in several places: npm, the Codex desktop app (in a versioned folder), and the ChatGPT extension for VS Code and similar editors. Copies differ in age; the copy in the VS Code extension here (0.122) could not use current ChatGPT models. Magic Mend asks every copy for its version and uses the newest. The user can set a path in Settings.
- **Model: GPT-6-Luna** at low reasoning effort, from the ChatGPT timing report: as fast as Claude Sonnet once warm and it kept every fact. GPT-5.6-Luna is slightly faster but changed "7pm" to "7 pm". The model list in Settings comes live from `codex debug models`.
- **Tool and approval requests** from the server are always declined, as a second line of defense behind the switched-off tools.
- **Both installed.** The setup wizard checks Claude and ChatGPT side by side, with a live test rewrite for each, and the user picks one. The tray menu and Settings switch at any time; the warm process restarts with the new service, and the next popup uses it.
- **Hard rule 3, extended.** "Use only the official, unmodified command, already signed in by the user" now covers `codex` for ChatGPT in the same way as `claude`.

### Known limits

- OpenAI labels `app-server` experimental. Magic Mend was built and tested against `codex-cli 0.163.0-alpha`; a future protocol change may need an update. The fake codex used by the tests documents the subset Magic Mend relies on.
- GPT-6-Luna follows the repeated-word rule less reliably than Claude: on the "move-in date" example, about one draft in four still said "move the move-in date".

## Mac signing and notarization

- **Workflow.** `.github/workflows/release-mac.yml`, modeled on the Synode release workflow: import the Developer ID certificate into a temporary keychain, build the universal app, let Tauri sign it (hardened runtime) and notarize it, verify with `codesign --verify --strict`, `spctl --assess` (must say "Notarized Developer ID"), and `stapler validate`, then attach the `.dmg` to the release. It runs on `v*` tags and by hand with a tag.
- **Secrets.** `APPLE_CERTIFICATE` (base64 of the `.p12`), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD` (an app-specific password, not the Apple ID password), and `APPLE_TEAM_ID`.
- **No Mac needed.** The certificate request and the `.p12` were made on Windows with the OpenSSL that ships with Git. The `.p12` uses the older SHA1 and 3DES encryption (`-keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES -macalg sha1`) so the macOS `security import` on the build machine accepts it. In Git Bash, `-subj "/CN=..."` needs `MSYS_NO_PATHCONV=1`, or Git Bash turns it into a Windows path.
- **Stapling.** The ticket is stapled to the app inside the `.dmg`, so it opens offline. The `.dmg` file itself carries no ticket, which Gatekeeper does not need.
- **Certificate expiry.** The current Developer ID certificate is valid until 24 November 2027. Replace the two certificate secrets before then.

## Deviations

- **Tone "Clear" and toggle "Proofread".** The spec calls them "Simple" and "Fix errors only". "Simple" was unclear as a tone name, and "Proofread" is the name people know from Apple's writing tools for the same job. The behavior is the one the spec describes. Saved settings and requests that still use the old names load as the new ones.
- **WM_PASTE for classic edit boxes.** The spec says paste is simulated with Ctrl+V. A classic Windows edit box (and a WinForms text box built on one) gets a WM_PASTE message instead. That box tries to open the clipboard only once, and a background app that reads the clipboard while Ctrl is held made about 1 paste in 8 land on nothing. WM_PASTE presses no key and returns after the paste. Rich edit boxes, browsers, and every other app still get Ctrl+V. No Enter is ever sent either way.
- **Window corners on Windows 11.** With acrylic turned on, Windows draws the frame, and its rounded corner radius is 8 px, not 16 px. When acrylic is not available (Windows 10) the popup draws its own 16 px corners on a solid background.
- **Look: paper, ink, and one thread.** The spec suggests a soft indigo accent and a needle with a sparkle. That read as a generic AI product, so the icon is now a magic pencil: a red editing pencil in a top hat turns a tangled scribble into a clean line, with madder stars where the change happens, on a paper tile with an ink outline. No gradient tile and no indigo. The UI uses warm paper neutrals with madder red (#A33B2E, #F08A77 in dark mode) as the only accent. Errors use plum (#8E1F4F) so they never look like a selected chip, and the focus ring is ink. The loading skeleton is plain gray lines that fade slowly, in place of the moving shimmer. Every text color passes WCAG AA in both themes, and axe-core reports 0 violations.
- **Channel title rule for "Mail".** Title words match whole words only, so "Mailchimp" does not count as email.

## Ideas (not built)

- Expand common contractions locally as a last resort when the engine fix round still leaves one. Today the draft stays blocked with a plain message, as the spec requires.
