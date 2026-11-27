# Spec — M5 Integrated terminal

flags: has_ui=true (terminal UI), security_sensitive=false, platforms=linux
source: 01_spec.md scope item 5, C1, D8, D10, AC-14; user 2026-11-25 ("ctrl+` toggles the console terminal")

## Goal
A bottom panel runs real shells in PTYs so the user can work (git, build, AI agents, htop, vim) without
leaving vr-ide. ctrl+` toggles it, as in VS Code.

## Scope
- Bottom panel with one or more terminal tabs; each runs `$SHELL` (fallback `/bin/sh`) as a login-less
  interactive shell in the project root, with `TERM=xterm-256color`, `COLORTERM=truecolor`.
- Emulation with `alacritty_terminal` (C1): truecolor, 256 colours, alternate screen, cursor shapes,
  bracketed paste, mouse reporting, scrollback (default 10 000 lines, setting `terminal_scrollback`).
- Keys:
  - `ctrl+\`` toggles the panel: hidden → show and focus the active terminal (create one if none);
    shown and focused → hide, focus returns to the editor; shown but not focused → focus it.
    Terminals without the kitty protocol send ctrl+` as NUL (same as ctrl+space / ctrl+@): that byte also
    toggles when the editor or explorer has focus; inside a focused terminal NUL is passed to the shell
    unless the kitty protocol reports a real ctrl+`.
  - `ctrl+shift+\`` new terminal tab (VS Code); palette commands for new / close / next / previous terminal.
  - D8: a focused terminal receives every key except the `[terminal]` keymap entries — `alt+q` (back to the
    editor) and `ctrl+\`` (hide); the user may add more there.
- Panel height: default 40 % of the screen, min 5 rows, resizable with commands (grow/shrink) and the
  mouse on the panel border; PTY resized to the panel on every layout change.
- Tab bar shows each terminal's title (OSC title or the shell name) and an exit marker; a terminal whose
  process exits shows "[process exited N] — Enter to restart, ctrl+w to close".
- Scrollback with shift+pageup/pagedown and the mouse wheel when the program has not enabled mouse
  reporting; any key or output returns to the bottom.
- Copy: mouse selection in the panel copies to the clipboard (existing clipboard path); paste with
  ctrl+shift+v (bracketed when the program enabled it).
- Theme: the 16 ANSI colours and default fg/bg come from the active theme (synthwave/dracula palettes;
  dark/light keep terminal defaults).
- Quit guard: quitting with running terminal processes asks "N terminals still running — Quit anyway /
  Cancel" (default Cancel); processes get SIGHUP on quit.
- No PTY I/O on the UI thread: a reader thread per terminal feeds the emulator; the UI renders the grid.

## Out of scope
AI presets (M6), split terminals, shell integration (cwd tracking, command marks), terminal search,
links/hyperlink clicking, Windows/ConPTY.

## Acceptance criteria
- AC-14: Given the terminal panel, When the user runs `htop`, `lazygit` or `vim` inside, Then it renders
  correctly (colours, cursor, alternate screen) and resizes with the panel.
- AC-14a: ctrl+` shows/focuses/hides the panel as above, in kitty and legacy terminals; alt+q returns to
  the editor; the editor never receives keys typed into a focused terminal.
- AC-14b: A new terminal starts in the project root with the user's shell; `exit` shows the exited state;
  Enter restarts.
- AC-14c: Output of 100 MB (`yes | head -c 100M`) does not freeze the UI (keys still handled within
  100 ms) and memory stays bounded by the scrollback setting.
- AC-14d: Quitting with a running terminal asks first; quitting kills the child process group.
- AC-1 still holds: no terminal is spawned at startup; startup < 100 ms, RSS < 60 MB with the panel closed.

## Edge cases
Resize to tiny sizes; wide/CJK/emoji and Vietnamese diacritics; programs that never read input; a shell
that fails to start ($SHELL missing); SIGCHLD races; very long lines; ctrl+c / ctrl+z / ctrl+d passed
through; read mode (T14) does not apply to terminals.

## Open questions
None — defaults taken per the spec and VS Code (D10); decisions logged in decisions.md.
