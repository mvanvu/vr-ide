# Review FR-16 — final review 2 fixes

Verdict: **APPROVED** (no blocker / major; security: SECURE)

Scope: worktree `.worktrees/FR-16` (branch fr-16, uncommitted), `git diff` of the 16 files below. Release binary `target/release/vr-ide` (00:27) is newer than every source file.

## Verified
- Quit order: `commands/app.rs:28` `quit` = `quit_unblocked` (replace guard) → `terminal::quit_allowed` (terminals + git write, one `ConfirmDialog`, `selected = 1` = Cancel) → unsaved question. `/q!` (`quit_discarding`, app.rs:125) same order with `QuitTerminalsDiscard`. The one-shot `quit_confirmed` is `mem::take`n, so a Cancel on the later unsaved question does not leave a stale confirm.
- Exit message: `git_popup::shutdown` pushes it; `event_loop.rs:147-170` restores the terminal first and prints `exit_messages` last. Live: `\x1b[?1049l` then `vr-ide: git push → origin/main was cancelled` (q1), `... git commit was cancelled` (q3).
- Status bar: `git::status_bar_label` appends `push → origin/main…` / `commit…` only while the popup is hidden; the short tier is `push…`; labels are sanitized at creation (`answer_push` `sanitize(target)`, `upstream()` sanitized).
- Retarget: tabs `forget_doc(old)` + `(new)`; every recent entry under `from` (component-wise `strip_prefix`) renamed, session `recent` list too; `/rename`'s duplicate call dropped. Live folder rename `dir` → `dir2` (my q4_review.py): `recent-files.toml` = `dir2/a.txt, dir2/c.txt, z.txt`; `/recent` lists the new paths.
- Welcome branch: `GitState::branch_of_root` (vr-git discovered repo, `sanitize`). `grep` for `git_branch|read_small|.branch()|join(".git")|"HEAD"` in `crates/vr-ide`: no hit; only vr-git reads `.git`. Removed `monitor::git_branch`, `read_small`, `Root::branch`: no dangling users (builds, tests compile).
- Keymap reload: `unreachable_keys_notice` shared with `report_keyboard`; reload shows the warning with "keymap.toml applied." prefix.

## Open points (judged)
- Workspace switch during a push: popup is orphaned (`git_popup.rs:2481`), the push keeps running, the hidden-popup segment keeps showing it and quit still asks. Acceptable; nothing is cancelled silently.
- Push finishing between Quit anyway and `shutdown`: the "was cancelled" line can be wrong within one event-loop iteration (ms). Acceptable; the user's re-push or re-commit is harmless or visibly a no-op. Optional: in `shutdown`, drain the job's done message without blocking before writing the line.

## Findings
None.

## Live / tests
- q1 push: status `main ↑1 · push → origin/main…`; dialog "A git push is running (push → origin/main); quitting cancels it." (wrapped), Cancel default keeps running; Quit anyway exits in 2.5 s; remote not updated, also after the hook time.
- q2 file rename: `recent-files.toml` and `/recent` show `b.txt`.
- q3 commit: `main · commit…`; dialog; exit message; no index.lock; log `base`; staged change kept.
- `cargo test -p vr-ide -- git_popup git_tests config_reload`: 78 passed, 0 failed.

## Files read
crates/vr-ide/src/{app.rs, app_workspace.rs, commands/dialog.rs, commands/explorer.rs, commands/slash.rs, commands/terminal.rs, config_reload.rs, config_reload_tests.rs, git.rs, git_popup.rs, git_popup_tests.rs, git_tests.rs, monitor.rs, theme.rs, ui/status_bar.rs, workspace.rs} — read. Followed: commands/app.rs (quit), event_loop.rs (exit order), recent_files.rs, vr-config recent_files.rs, git_popup.rs (close_session, roots_changed, labels).

## Rule check FR-16
| Rule | Status | Evidence |
|---|---|---|
| BASE-10 tests | pass | 6 new tests; targeted run 78 ok; notes: -p 638 ok, workspace 1173 ok |
| BASE-11 no commit | pass | `git status`: uncommitted |
| BASE-14 no sleep/poll loops | pass | no new loops in product code |
| BASE-15 own processes only | pass | scripts kill their own pty sessions |
| BASE-16 reuse | pass | duplicate HEAD reader removed; quit question and `report_keyboard` body reused |
| BASE-19 notes | pass | 04_dev_FR-16.md with Rule compliance |
| COST-9 nothing left running | pass | `pgrep` after the scripts: none |
| SEC-6 git discovery boundaries | pass | `branch_of_root` uses vr-git roots only; planted parent `.git` test returns None |
| SEC-15 sanitize git text | pass | `sanitize` on branch / short; push / pull labels sanitized |
| TDD | pass | red run recorded in notes (5 failed) |
