# FR-20 Remove /git commands, git popup, vr-git write side
- Skills loaded: ai-dev-team:implementation
- Mode: implement (removal), side fe, worktree .worktrees/FR-20 (branch fr-20), nothing staged or committed.

## Removed
- vr-ide: modules git_popup, git_history, ui/git_popup, ui/git_history and their tests (git_popup_tests, git_history_tests) plus 16 insta snapshots (popup/history).
- Slash entries `git blame line`, `git`, `git log`, `git log file`, `git blame`, `git show`, `git push`, `git pull` and their handlers (commands/slash.rs).
- Wiring: App.git_popup / git_drafts / git_popup_shown, key/paste/mouse routing, the Enter-gap exception for the message box (enter_is_not_deliberate lost its `gap` argument), tick/shutdown/roots_changed hooks, the save_blocked guards in commands/file.rs (3 sites), DialogAction::Git* variants and answer arms (commands/dialog.rs), the git-recovery recovery_dir setup (app.rs), git.rs: repo_info_of_root, repo_written, status_bar_label (status bar now calls status_label), on_change calls.
- Quit guard D309: commands/terminal.rs quit_allowed_for no longer asks about a running git job (only the git lines of that function and two comments were edited; FR-17/18/19 area otherwise untouched).
- vr-git: write.rs, patch.rs, recovery.rs, remote.rs, discard_faults.rs, programs.rs, log.rs, state.rs; diff.rs reduced to line_changes (+ eol handling); repo.rs lost status / staged_fingerprint / resolve_commit / with_own_runner; blame.rs lost blame / blame_text (kept inline_blame; Signature moved here); worker Request/Reply reduced to Head, AheadBehind, InlineBlame, LineChanges; cmd.rs lost Profile, Cancel, Ctl, run_ctl, write config, network/idle logic; Options lost hook_timeout, network_idle_timeout, recovery_dir, recovery_cap, user_timeout; GitError lost Cancelled / Refused / Stale; redact_userinfo removed; unused `tracing` dependency dropped.
- Docs: README Git section, settings row, M7 line; docs/commands.md (Git paragraphs, 8 table rows); docs/keymap.md Git popup row; settings.toml and settings.rs comments (no more `/git blame line`).
- capabilities.md: vr-git entry rewritten (read-only API); git_popup, git_history, write side and programs entries removed.

## Kept (and why)
Gutter marks (Repo::line_changes, ui/editor_view marks), inline blame (Repo::inline_blame, setting git_inline_blame, now switched only by settings or `/set git_inline_blame`), branch + ahead/behind in status bar and welcome screen (Head, AheadBehind), GitWorker, hardened runner cmd::Git, sanitize_line, ui/safe_text.rs (still used by git.rs). No setting other than git_inline_blame existed for git.

## Deleted/changed tests -> coverage
- git_popup_tests, git_history_tests, vr-git write.rs / write_security.rs / patch_props.rs, read.rs status / diff / log / commit / staged-fingerprint / resolve tests: behaviour removed.
- read.rs: unborn, blame, odd-paths and symlink tests re-expressed on head / ahead_behind / inline_blame / line_changes / relative; conflicted test keeps the no-gutter assertion.
- security.rs: read_everything, snapshot, hang/timeout/shutdown tests moved to head / inline_blame / line_changes (shim hangs `symbolic-ref`); partial-clone tests run `git read-tree HEAD` first so line_changes really meets a missing blob; blame-staged-over-cap test dropped (file_over_the_cap_is_too_large covers it via inline_blame); the fsmonitor and submodule-filter controls remain but no vr-git read reaches those code paths any more.
- worker.rs: cache test uses InlineBlame; asserts invalidate_path drops repo-wide kinds and that file's reply only.
- bench.rs: status/log/blame timings dropped; inline blame, gutter and worker kept (ignored test).
- git_tests.rs: `/git blame line` uses replaced by `/set git_inline_blame true|false`; the registration test now asserts no slash command starts with `git` (AC-F4).
- slash_tests.rs: the 8 git names removed from the registered list.

## Searches (BASE-20 / COST-10)
grep over crates, docs, README, defaults for: git_popup, git_history, GitPopup, Writer, Writer::open, "/git ", "git blame", "git log", "git show", "git push", "git pull", D297/D304/D305/D307/D309, SEC-16/18/19, M7-3/M7-4b, popup, git-recovery, git_drafts, running_write, save_blocked, status_segment, redact_userinfo, Cancel/Refused/Stale/Cancelled, recovery_dir/recovery_cap/hook_timeout/network_idle_timeout/user_timeout, tracing. Also keymap.toml defaults, explorer/editor menus (no git entry existed), welcome hints (branch only), palette registry (slash table only), test/guard lists (slash_tests list, git_tests). Remaining hits are unrelated (App.recovery_dir, RacingWriter, `git pull` in install docs, `.gitignore`).

## Out-of-scope / left
- crates/vr-ide/src/theme.rs ~l.50: doc of `Theme.focus` still says "title of the Git popup" (theme.rs is FR-21's file, not edited). No Theme field was found used only by the popup.
- vr-git cmd.rs keeps editor / GPG env hardening (harmless, still unit-tested).
- Flake seen once: vr-git `security::submodule_local_filter_never_runs` failed in one run (its "control" step depends on stat timing), then passed 7 times in a row and in the whole run.

## Commands run (trimmed)
- cargo test -p vr-git: unit 7 ok; bench 1 ignored; read 13 ok; security 26 ok (once 25 + the flake above); worker 4 ok.
- cargo test -p vr-ide (whole, run-heavy, background): first run 570 passed, 3 failed (git_tests using `/git blame line`); after the fix `cargo test -p vr-ide --lib`: 573 passed, 0 failed, 7 ignored. Log: workspace/qa-artifacts/FR-20/test.log.
- Snapshots: none changed; only the 16 popup/history .snap files deleted. INSTA_UPDATE not needed.
- Guard via run-heavy: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo build --workspace` -> Finished, no warnings.
- rule-checks.sh related-tests FR-20 listed related files; the whole vr-ide run covers them.
- Nothing left running.

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| status label for status bar | capabilities.md, git.rs | reused git::status_label |
| Signature type | log.rs (deleted) | moved to vr-git/src/blame.rs |
| switch inline blame in tests | slash.rs | reused `/set git_inline_blame` |

## Deleted tests
| Deleted | Verdict |
|---|---|
| crates/vr-ide/src/git_popup_tests.rs (all) | behaviour removed (popup, Changes tab, writes, quit guard for git jobs) |
| crates/vr-ide/src/git_history_tests.rs (all) | behaviour removed (History, Blame tab, commit view, /git log, /git blame, /git show) |
| 16 insta snapshots render_git_popup_* | behaviour removed with their tests |
| crates/vr-git/tests/write.rs, write_security.rs | behaviour removed (stage, unstage, discard, commit, amend, push, pull) |
| crates/vr-git/tests/patch_props.rs | behaviour removed (hunk and line patches for staging) |
| read.rs: status_lists_match_porcelain, diffs_carry_line_numbers_and_match_git, untracked_diff_*, staged_rename_and_commit_diff_*, log_follows_renames_*, user_typed_revisions_*, message_search_revisions_*, the_staged_fingerprint_* | behaviour removed (status, diff views, log, commit, rev resolution) |
| read.rs: unborn_repo_head_status_log_blame, blame_matches_porcelain_*, odd_paths_round_trip, symlink_is_addressed_by_its_own_path | replaced by unborn_repo_head_and_inline_blame, inline_blame_matches_porcelain_and_marks_unsaved_lines, odd_paths_round_trip, symlink_is_addressed_by_its_own_path (kept API only) |
| security.rs: blame_of_a_staged_file_over_the_cap_is_too_large | covered by file_over_the_cap_is_too_large (inline_blame branch) |
| vr-git unit tests of redact_userinfo, parse_diff and log/status parsers (modules deleted or trimmed) | behaviour removed |
| git_tests.rs: blame_line_is_a_registered_slash_command_and_setting | replaced by inline_blame_is_switched_by_the_setting_and_there_is_no_git_slash_command |
| slash_tests.rs: the 8 git names in the registered list | behaviour removed |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | only the git popup, git commands, vr-git write side and their docs touched; theme.rs doc and cmd.rs env hardening left and noted |
| BASE-4 | pass | trimmed test and guard output in Commands run; log in qa-artifacts/FR-20/test.log |
| BASE-5 | pass | test runs and guard through run-heavy.sh; nothing left running |
| BASE-6 | pass | English notes and comments; no bug-history comments |
| BASE-7 | pass | no commit, no stage |
| BASE-8 | pass | no new dependency; Cargo.lock changed only because vr-git's unused `tracing` dependency was removed (vr-git/Cargo.toml, lock updated by cargo) |
| BASE-10 | pass | related vr-git tests per step; one whole `-p vr-ide` run at the end as the brief allows (the removal touches crate wiring); no all-guards run |
| BASE-11 | pass | no stash, checkout, restore, reset or branch switch; deletions only inside the worktree with plain rm |
| BASE-12 | pass | nothing written outside the worktree except the notes and capabilities.md under $TEAM_* |
| BASE-14 | pass | whole test run as a background task, no sleep or polling |
| BASE-15 | pass | no pkill; the one background task ended by itself |
| BASE-16 | pass | Reuse table; capabilities.md updated (entries of removed code deleted) |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | notes created at the start, progress lines added |
| BASE-20 | pass | Searches section: every reference searched, kept code listed |
| BASE-23 | pass | every entry path gone: slash entries, keys, mouse, paste, dialogs, quit guard, save guards, status bar job segment |
| BASE-24 | pass | no index or history command run |
| BASE-25 | pass | `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo build --workspace` clean |
| BASE-32 | pass | D310 honoured (popup parts of D297, D305-D309 removed); M7-4a editor marks decisions kept |
| BASE-38 | pass | cargo fmt on vr-git, vr-ide, vr-config only; no other file reformatted |
| BASE-45 | pass | AC-F4 is in 01_spec_FR17-21.md; see its row |
| AC-F4 | pass | no `/git*` entry (git_tests::inline_blame_is_switched_by_the_setting_and_there_is_no_git_slash_command), popup, keys, write side and the D309 guard removed (grep clean); gutter, inline blame and branch tests pass (573 vr-ide tests); README, docs/commands.md, docs/keymap.md and settings docs updated |
| COST-1 | pass | heavy runs via run-heavy.sh |
| COST-3 | pass | no sleep loops or pgrep |
| COST-4 | pass | one whole vr-ide run (allowed by the brief), reruns only of failing files |
| COST-5 | pass | no browser, server or watcher started |
| COST-8 | pass | batched reads and edits; perl and Edit only |
| COST-9 | pass | nothing left running |
| COST-10 | pass | slash_tests list, git_tests and test/guard lists grepped for removed names |
| COST-13 | pass | scratch only in /tmp and workspace/qa-artifacts/FR-20 |
| COST-15 | pass | no snapshot regenerated; only popup snapshots deleted |
| COST-16 | pass | full test output in qa-artifacts/FR-20/test.log; trimmed summary in context |
| n-a: BASE-2, BASE-3, BASE-9 | n-a | removal: no contract, new behaviour or input surface added (the 3 failing git_tests were old `/git blame line` uses, fixed) |
| n-a: BASE-13, BASE-21, BASE-22, BASE-27, BASE-30, BASE-31 | n-a | no untracked file deleted, no secrets, no new names, no shared data or containers touched |
| n-a: BASE-17, BASE-26, BASE-28, BASE-29, BASE-33, BASE-34, BASE-40, BASE-41 | n-a | no toolchain change, no rewrite, no third-party code, no browser session, no other system, no outward write, no credentials |
| n-a: BASE-35, BASE-36, BASE-37, BASE-39, BASE-42, BASE-43, BASE-44, BASE-46, BASE-47, BASE-48 | n-a | no USER rules apply, no app scope or schema change, no runtime-defect fix, no test-runner settings, no access rule, no fake adapter, no runtime evidence claim, no changed computed value, no machine-state claim |
| n-a: COST-2, COST-6, COST-7, COST-11, COST-12, COST-14, COST-17, COST-18, COST-19, COST-20 | n-a | no UI check or sweep, no e2e, no CI wait, no merged snapshot conflict, no doomed full run, leader-only or qc-only items |

## Quick pass 1
- 1: security.rs: "guards future reads" comments on `core_fsmonitor_never_runs`, `show_signature_gpg_program_never_runs`, `submodule_local_filter_never_runs`; the submodule control now runs `git status` inside the submodule (always rehashes the stale-stat file). `cargo test -p vr-git --test security submodule_local_filter_never_runs` x10: 10/10 ok.
- 2: worker.rs: Head requested before/after `invalidate_path(b.txt)`, `!Arc::ptr_eq`. With the repo-wide drop disabled in worker.rs:260 the test failed at worker.rs:181 ("a path invalidation drops repo-wide replies"); restored, passes.
- 3: cmd.rs comments reworded as defence in depth. 4: lib.rs docs rewrapped at 100. 5: README Git section sentence on `git-recovery`.
- `cargo test -p vr-git` via run-heavy: all ok (log qa-artifacts/FR-20/qp1_test.log). `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`: clean.
- Rules touched: BASE-1 pass (only findings), BASE-3 pass (mutation shown), BASE-6 pass, BASE-11/24 pass (no index/branch commands), BASE-25 pass, BASE-38 pass (fmt -p vr-git), COST-8 pass, COST-16 pass.
