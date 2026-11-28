# T6 Docs for the native window
Docs only; every statement checked against the T4 worktree code (cli.rs, main.rs, desktop.rs, window_frontend.rs, settings.rs, defaults/settings.toml) and notes T3b/T4/T4a/T5.

## Files changed
- README.md: intro; Requirements (native window bullet, terminal bullet, `--no-default-features` note); Install (menu entry direct form, terminal-only build form, migration of old entries with notice); Quick start (options `--tui`/`--session`, terminal needed only in TUI, closing window = ctrl+q); new section "Window and terminal" (launch rules, fallback line, close, keys, IME, font, clipboard, menu entry); Configuration table row + new "Window font" subsection (defaults, ranges, missing family warning); Terminal notes lead-in.
- docs/keymap.md: new "Keys in the window" (all OS-delivered chords reach vr-ide; legacy limits gone; keys kept by GNOME/macOS/Windows); key spelling bullet for shifted symbols (`ctrl+{` == `ctrl+shift+[`, shown `ctrl+shift+[`); note on terminal-only limit.
- docs/commands.md: pointer line and a "Launch flags" section (`--tui`, launch rules, fallback). Generated slash-command block untouched.
- docs/languages.md: no terminal requirement mentioned; not changed.

## Checks
Anchors used: #window-and-terminal (new heading), #install, #terminal-notes, #quit-guard: all exist. No commands run besides grep/read.
Unverified by code reading (stated cautiously): the desktop key examples (super, alt+tab, cmd+q, win...) are general OS knowledge, not vr-ide behaviour.

## Rule compliance
| Rule | Status |
|---|---|
| BASE-1..49 (applicable: scope, English, no guessing, no scope creep) | pass |
| BASE-50/51 | n-a (no experiments) |
| COST | pass (nothing started) |
Counts: pass 3, n-a 1

## Fix round 1
1. README `### Clipboard`: now "In the window: system clipboard for copy and paste (inside-vr-ide-only text with a notice when it is unavailable, per `WindowClipboard`); In the terminal: <old text>". The window bullet says copy and paste and links to it.
2. Window close bullet now points to the Quit guard instead of copying the conditions.
3. Added: foreground of the starting shell (main.rs runs the window in-process, no re-spawn; `&` keeps the prompt), attention hint (`request_attention` in event_loop on a new attention mark, winit `request_user_attention` only while unfocused; marks are the existing bell/OSC 9/OSC 777 notifications, linked to Agent notifications), `StartupWMClass=vr-ide` (desktop.rs window form).
Checked: anchors #clipboard, #agent-notifications, #quit-guard exist.
