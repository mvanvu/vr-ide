# FR-16 — final review 2 fixes

Tools: Bash, Read, Edit, Write, Skill, Agent, Artifact, ToolSearch.

## Progress
- read 06_final_review_2.md, D309, capabilities; wrote failing tests for item 1 (git_popup_tests.rs)
- failing tests written for items 2,3 (git_tests.rs), 4 (config_reload_tests.rs)
- implemented items 1-5
- targeted tests green; cargo test -p vr-ide 638 passed / 7 ignored; clippy -D warnings clean; fmt --check clean; full suite + release build running (background)
- full suite + release build done; q1–q3 rerun against the worktree release binary

## Changes (worktree .worktrees/FR-16, branch fr-16, not committed)
1. D309 quit during a git write — `crates/vr-ide/src/git_popup.rs`: `RunningWrite`, `GitPopup::running_write`, `running_write(app)`, `status_segment(app)`; `shutdown` pushes `exit_messages` "git push → origin/main was cancelled" / "git commit was cancelled". `crates/vr-ide/src/commands/terminal.rs` `quit_allowed_for`: one question for terminals and the git write ("A git push is running (push → origin/main); quitting cancels it.", Quit anyway / Cancel, Cancel selected) with the same one-shot confirm, so ctrl+q / `app.quit` / `/q` / `/q!` (`QuitTerminalsDiscard`) share one flow: replace guard → terminals+git question → unsaved-files question. `crates/vr-ide/src/git.rs` `status_bar_label` (status_label + "push → origin/main…", short "push…" / "commit…" while the popup is hidden); `ui/status_bar.rs` uses it. `commands/dialog.rs` doc comment.
2. `commands/explorer.rs` `retarget`: moved tabs `git.forget_doc(old)` and `(new)`; every recent-files entry under `from` goes through `App::recent_file_renamed` (oldest first, order kept). `/rename`'s own call dropped (`commands/slash.rs`). `App::recent_file_renamed` also renames the session `recent` list (welcome).
3. `git.rs` `GitState::branch_of_root` (vr-git's discovered repo, sanitized); `app.rs` tick and `app_workspace.rs` `refresh_root_info` use it; `monitor::git_branch`, `read_small`, their 4 tests and `workspace::Root::branch` removed.
4. `app.rs` `unreachable_keys_notice()` extracted from `report_keyboard`; `config_reload.rs` `reload_keymap` shows "keymap.toml applied. Keyboard: this terminal cannot send some keys; …" (warning, detail lists the keys) instead of the plain info.
5. `theme.rs` `focus` comment.

## Tests (red first, then green)
Red run (stub `branch_of_root` → None): 5 FAILED / 1 ok (the no-job quit test is a guard):
quit_during_a_push_… , slash_q_bang_during_a_commit_… , an_explorer_rename_and_move_… (`left: Some((0, [LineChange{Modified,1,1}]))` = old path's marks), the_welcome_branch_… (`left: Some("planted\u{1b}[31m")`), a_reloaded_keymap_names_… .
Green: all 6 ok. New tests: `git_popup_tests.rs` (3; blocking pre-push / pre-commit hooks, real git), `git_tests.rs` (2), `config_reload_tests.rs` (1).
- `cargo test -p vr-ide`: 638 passed, 0 failed, 7 ignored.
- `cargo clippy --workspace --all-targets -- -D warnings`: Finished, no warnings.
- `cargo fmt --all -- --check`: clean.
- full suite (run-heavy.sh `cargo test --workspace`): 1173 passed, 0 failed, 18 ignored, exit 0.
- `cargo build --release -p vr-ide` (worktree target): Finished in 2m03s.

## Live re-walk (worktree release binary; scratchpad fr16/q1.py..q3.py, q1/q3 extended for the dialog)
- q1 push: status `READ a.txt · main ↑1 · push → origin/main… · Ln 1, Col 1`; ctrl+q → "A git push is running (push → origin/main); quitting cancels it." [Quit anyway] [Cancel]; Enter (Cancel) keeps running; ctrl+q, Left, Enter → exit in 2.5 s; stderr `vr-ide: git push → origin/main was cancelled`; remote not updated (also after the hook time).
- q2 rename: `recent-files.toml: files = [".../proj/b.txt"]`, `/recent` lists `1 b.txt`.
- q3 commit: status `… main · commit… …`; dialog "A git commit is running; quitting cancels it."; stderr `vr-ide: git commit was cancelled`; no index.lock; log `base`; staged change kept.
- Welcome branch (item 3) covered by the unit test only.

## Reuse
| Need | Reused |
|---|---|
| quit question | `terminal::quit_allowed_for` + its one-shot confirm (extended, no second guard) |
| branch read | vr-git `GitState` entry `head` + `safe_text::sanitize` (as `status_label`) |
| recent rename | `App::recent_file_renamed` |
| lost-key check | `report_keyboard` body, extracted |

## Notes
- Workspace switch (`answer_open_workspace`) with a running push orphans the session as before; D309 covers quit only.
- A push that finishes in the instant between the answer and the exit is still reported as "was cancelled".

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-10 related tests + mapped guards | pass | targeted, -p vr-ide, full suite once |
| BASE-11 no commit | pass | worktree only, nothing committed |
| BASE-14 no sleep/poll loops | pass | background task + notification |
| BASE-15 stop only own processes | pass | pty sessions killed by scripts |
| BASE-16 reuse | pass | Reuse table; duplicate HEAD reader removed |
| BASE-17 no SDK installs | pass | — |
| BASE-19 notes + progress | pass | this file |
| COST-9 nothing left running | pass | pgrep: no vr-ide of the worktree |
| SEC-6 / SEC-15 git discovery, sanitize | pass | welcome branch from vr-git, sanitized; planted parent .git test |
| TDD failing test first | pass | red run above |
| TOOL greenfield toolchain | n-a | existing project |
