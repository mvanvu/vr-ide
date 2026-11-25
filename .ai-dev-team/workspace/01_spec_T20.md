# Spec — T20 Command window: slash commands in the F1 palette

flags: has_ui=true (terminal UI), security_sensitive=false, platforms=linux
source: user 2026-11-25 step by step (D258, D259, D260) + "follow the proposal, also drop f12 and f5" (D263).

## Window
- F1 (everywhere, edit mode included) and `space space` in normal mode open the palette (built in T21).
- Typing `/` starts a slash command: the list shows matching commands with their argument hint and
  description; Tab completes; Enter runs; Esc closes and returns to normal (read) mode.
- Unknown command / bad argument: an error line under the input, the text stays for correction.
- Without `/` the palette behaves as today (fuzzy command list).

## Phase 1 (this task) — the recommended groups
- File: `/save-as [path]`, `/reload` (f12 and f5 are removed as keys, D263).
- Terminal: `/term new`, `/term close`, `/term right`, `/term bottom` (the last two only once T19 lands;
  until then listed as unavailable), `/run <command>` (sends the command line + Enter to the active
  terminal, opening one if needed; typed by the user, nothing auto-run), `/ai <tool>` (opens a terminal running
  that tool, e.g. `claude`; a missing tool shows the shell's own error).
- Workspace: `/ws add <dir>` (path completion), `/ws remove`, `/ws save [file]`, `/ws open <file>`, `/ws recent`.
- View: `/theme synthwave|dracula|dark|light`, `/wrap`, `/hidden`, `/ignored`.
- Help: `/help` (every slash command with its arguments and description; the docs table is generated from
  the same registry and a test checks they match).
- Quit: `/q` (unsaved guard), `/q!` (discard).

## Phase 2 (task T20b, queued) — the rest of the proposal
`/rename <name>`, `/copy-path`, `/copy-relpath`, `/recent`, `/sort`, `/unique`, `/upper`, `/lower`, `/trim`,
`/indent 2|4|tab`, `/eol lf|crlf`, `/lang <id>`, `/set <name> <value>`, `/config`, `/keymap`, `/keys`.

## Later
`/git …` with M7 (waiting for the user).

## Acceptance criteria
- AC-20a: F1 / space space open the window; `/` lists slash commands; Enter runs, Esc returns to normal mode.
- AC-20b: every phase-1 command works with its arguments; bad arguments show an error and keep the text.
- AC-20c: `/run` and `/ai` never run anything without the user's Enter; `/q` keeps the unsaved guard.
- AC-20d: `/help` and docs match the registry (test); snapshots 80x24, 120x40, 40x12.
