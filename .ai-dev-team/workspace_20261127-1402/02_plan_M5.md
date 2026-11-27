# Plan — M5 Integrated terminal

Spec: `.ai-dev-team/workspace/01_spec_M5.md`. Global constraints of 02_plan.md hold (no blocking I/O on the
UI thread, thiserror in libraries, insta snapshots, VS Code keys).

## Architecture
- New crate `crates/vr-term` (no UI types):
  - `Terminal::spawn(SpawnOptions{shell, cwd, env, size, scrollback})` — portable-pty child + PTY master;
    a reader thread feeds bytes into `alacritty_terminal::Term` behind a mutex; a writer handle for input;
    `resize(cols, rows)`; `status()` (running / exited code); `kill()` (SIGHUP to the process group) on
    drop; a `Notify` callback after new output (coalesced) so the UI redraws without polling.
  - `snapshot(&self, viewport) -> Grid` — cells (char, fg, bg, flags), cursor (pos, shape, visible), title,
    mode flags (alt screen, mouse reporting, bracketed paste, app cursor), scrollback offset.
  - `input` helpers: key → bytes encoding (xterm, honouring app-cursor / app-keypad modes), paste
    (bracketed when enabled), mouse events → SGR mouse reports when reporting is on.
- vr-ide:
  - `layout`: a bottom panel region; `Focus::Terminal`; panel visibility/height state.
  - `terminal_panel` UI: tab bar + grid render (theme ANSI palette), cursor, exited banner, scrollback
    indicator; mouse selection → clipboard.
  - commands: `terminal.toggle` (ctrl+`, legacy NUL), `terminal.new` (ctrl+shift+`), `terminal.close`,
    `terminal.next/prev`, `terminal.grow/shrink`, `focus.editor` (alt+q, already reserved), keymap
    `[terminal]` table semantics per D8; quit guard extension.
  - Theme: `Theme::ansi_palette()` for synthwave/dracula; dark/light → terminal defaults (Color::Indexed 0–15).

## Contracts
- `vr_term::Terminal`, `SpawnOptions`, `Grid/Cell/CursorShape`, `TermStatus`, `encode_key(KeyEvent, Modes)
  -> Vec<u8>`, `encode_paste(&str, Modes)`, `encode_mouse(...)`.
- Dependencies (pinned exact): `portable-pty`, `alacritty_terminal` — licence check in T-M5-1.

## Tasks
1. **M5-1 (be, opus)** vr-term crate: spawn, reader thread, emulator, resize, kill, status, notify,
   snapshot, key/paste/mouse encoding; tests with `/bin/sh -c` scripts (echo colours, alt screen via
   `tput smcup`, resize reported by `stty size`, exit codes, 100 MB output bounded memory and throughput,
   kill on drop leaves no zombie).
2. **M5-2 (fe, opus)** Panel + focus + keys in vr-ide: layout, rendering, tabs, D8 routing, ctrl+` incl.
   legacy NUL, scrollback, selection copy/paste, exited state + restart, quit guard, theme ANSI palettes,
   keymap/docs. Snapshots at 80x24/120x40/40x12.
3. **M5-3 (fe, sonnet)** Docs (README section, docs/keymap.md), previews, bench (AC-1 with panel closed;
   RSS with one terminal open), QC walk of htop / lazygit / vim (AC-14) by qc.
4. Sign-off qc + Phase 5 (correctness + ui).

Order: M5-1 → M5-2 → M5-3. M5-1 may start in a worktree now (own crate; root manifest only).

## Risks
- alacritty_terminal API churn → pin the version; wrap it behind vr-term's own types.
- Legacy ctrl+` = NUL collides with ctrl+space (future completion, M8): documented; kitty protocol avoids it.
- Child process cleanup on panic/quit → process group + SIGHUP, tested.
