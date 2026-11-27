# Review FR-20 (group B: crates/vr-ide, docs, README)

No findings. save_blocked guards (3 sites) were only the popup's discard-in-progress check (D305(4)); nothing else is protected by them. terminal.rs quit guard: only git lines/comments removed, terminal confirm (Quit anyway / Cancel, Cancel selected, quit_confirmed one-shot) intact.

## Files read
| File | Status |
|---|---|
| README.md | read |
| crates/vr-ide/src/app.rs, app_workspace.rs, event_loop.rs, lib.rs | read |
| crates/vr-ide/src/commands/dialog.rs, file.rs, slash.rs, terminal.rs | read |
| crates/vr-ide/src/git.rs, git_tests.rs, slash_tests.rs | read |
| crates/vr-ide/src/ui/mod.rs, ui/status_bar.rs | read |
| docs/commands.md, docs/keymap.md | read |
| git_popup.rs, git_history.rs, git_popup_tests.rs, git_history_tests.rs, ui/git_popup.rs, ui/git_history.rs | read — deleted whole module, confirmed absent from lib.rs / ui/mod.rs |
| 16 render_git_popup_* .snap | skipped — deleted snapshot of removed popup (each belongs to removed popup/history tests) |

## Rule check FR-20
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | only popup/commands/docs touched; theme.rs left (known) |
| BASE-6 | pass | no bug-history comments added; no dangling popup refs (grep) |
| BASE-20 | pass | grep git_popup/git_history/GitPopup//git/popup wording/D297-D309 in crates, docs, README, config: only theme.rs:50 (known) and unrelated theme.popup |
| BASE-23 | pass | slash entries, key/paste/mouse routing, dialogs, tick/exit/roots hooks, render all removed; no keymap/menu entry existed |
| BASE-25 | pass | docs/README/keymap say only what exists; git_inline_blame via settings or /set |
| BASE-32 | pass | git_tests asserts no slash command starts with "git"; /set git_inline_blame path tested; gutter, branch, inline blame kept |
| Gate 14 | pass | deleted tests map to removed behaviour or replaced tests (dev notes table checked against diff) |
| AC-F4 | pass | all 8 commands, popup, D309 quit guard gone; gutter/blame/branch kept |

Note (info): git.rs:1 header still cites D297, a decision reference not a popup reference; fine.

APPROVE
