# Spec — T20 Command line (':')

flags: has_ui=true (terminal UI), security_sensitive=false, platforms=linux
source: user 2026-11-25 (D254); readings D255, D256.

## Goal
Run any command by name instead of remembering key combos, vim-style: ':' opens a command line.

## Scope
- ':' (shift+; — a plain character, reachable from every terminal) opens a one-line prompt ':' on the bottom
  row (over the status bar) when focus is the editor in read mode, the explorer or the welcome screen.
  In edit mode ':' inserts text; terminal, find bar, palette and dialogs keep their own keys.
- Enter runs the command; Esc closes the line and returns to read (normal) mode; Backspace on an empty
  line closes it.
- Commands: every palette command has a command-line name (lower-case words, `terminal new`, `explorer`,
  `theme dracula`, `search <text>`, `workspace add <path>`…); vim shorthands `w`, `q`, `wq`, `q!`, `<n>` (go to
  line), `e <path>` (open file, path completion); later features register their own names (M7: `git gui`,
  `git blame`, `git show`, `git log`, `git blame line`).
- Tab completes the command (and paths for path arguments); a suggestion list above the line shows matching
  commands with their key binding (if any) and a one-line description; Up/Down recall history (last 100,
  `$XDG_STATE_HOME/vr-ide/history`, written off the UI thread).
- Unknown command / bad argument: an error notice, the text stays for correction.
- docs/keymap.md and README: a "Command line" section with the command table (generated test checks it
  matches the registry); welcome hint shows `: command`.

## Acceptance criteria
- AC-20a: In read mode ':' shows the prompt; `git gui`-style names resolve (until M7 lands, an unknown-feature
  notice); `terminal` + Enter toggles the terminal; Esc returns to read mode with nothing run.
- AC-20b: Every palette command is reachable by a name; the docs table matches the registry (test).
- AC-20c: `:w`, `:q`, `:wq`, `:q!`, `:42`, `:e src/main.rs` behave as save / quit (with unsaved guard) /
  save+quit / quit discarding / go to line 42 / open file.
- AC-20d: In edit mode ':' types a colon; in a focused terminal ':' goes to the shell.
- AC-20e: Tab completion, suggestion list and history work at 80x24 and 40x12 (snapshots).
- AC-1 still holds.
