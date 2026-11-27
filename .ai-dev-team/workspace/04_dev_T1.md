# T1 Session store + --session launch + restore (D1–D3)
Worktree: .worktrees/T1 (branch run4-T1)

## Progress
- read: spec D1–D3/AC-1..4, threat model SEC-6, UI spec S6, cli.rs, main.rs, app.rs, recovery.rs, explorer, terminal panel
- red: session_tests.rs (13 tests) + cli_session_flag test against a stub session.rs: `cargo test -p vr-ide session` → `test result: FAILED. 16 passed; 11 failed` (the 2 passing session tests are the no-I/O / home cases the stub already satisfies)
- wiring: Cargo.toml toml dep, lib.rs, app.rs field+tick hook, event_loop exit hook, terminal add_tab_in pub(crate), cli --session, main.rs
- code: crates/vr-ide/src/session.rs (store, load checks, restore, debounced/quit save, AI API); flaky lock re-take under parallel forks fixed with 10×10 ms try_lock retries; quit wait 5 s (unbounded in tests)
- green: related tests + guard (below); smoke run of the binary (below)
- docs: README.md launch section (`--session` lines only)

## Files changed
- crates/vr-ide/src/session.rs (new) — Session/TabState/TerminalLayout/AiSection/AiSession structs (serde, TOML), `start`, `restore`, `tick`, `save_on_quit`, `snapshot`, `sanitize`, `take_restored_ai`, `set_ai`, `flush`, `home_dir`; private `load`/`refused`/`keep_bad`/`take_lock`/`write_file`/`remove_stale_temps`/`Writer` thread
- crates/vr-ide/src/session_tests.rs (new) — 13 tests
- crates/vr-ide/src/cli.rs — `SESSION_FLAG`, `Launch.session`, `--session` alone → home workspace; usage text; test `cli_session_flag_opens_home_alone`
- crates/vr-ide/src/main.rs — `--session`: `session::start` before the app, `session::restore` after `start_watcher`
- crates/vr-ide/src/app.rs — field `session: Option<session::Keeper>` (None init) and one call `crate::session::tick(self, now)` in `App::tick`
- crates/vr-ide/src/event_loop.rs — one call `crate::session::save_on_quit(app)` in `exit`, before terminals are shut down (their /proc cwd names the folders)
- crates/vr-ide/src/commands/terminal.rs — `add_tab_in` made `pub(crate)` (restore starts a shell per saved folder)
- crates/vr-ide/src/lib.rs — `pub mod session`, `mod session_tests`
- crates/vr-ide/Cargo.toml (+ one Cargo.lock line) — `toml.workspace = true` (already a workspace dependency, used by vr-config)
- README.md — `--session` lines in the launch section

## AI panel API (for T5)
`Session.ai: AiSection { shown: bool, width: Option<u16> /* percent, 10..=90 */, sessions: Vec<AiSession { tool, account, folder: PathBuf, label }> }`.
- On start: `session::take_restored_ai(&mut app) -> Option<AiSection>` returns the section read from the file once (entries with control chars, > 256 chars or a non-absolute folder dropped; ≤ 32 entries). Tool, account and root still have to pass T5's own SEC-8 / SEC-18 checks.
- On every panel change: `session::set_ai(&mut app, AiSection)`; the debounced save and the quit save write it. Until then the restored section is written back unchanged.
- Both are no-ops without `--session` (`app.session == None`).

## Tests added
session_tests: session_without_a_saved_one_opens_home_only (AC-1), session_restores_roots_tabs_explorer_and_terminal_layout (AC-2, fresh read, shell cwd via /proc), session_file_is_private_and_replaces_a_symlink_without_following_it (0700/0600, no temp left, stale temp removed, symlink target untouched), a_change_is_saved_within_two_seconds, a_corrupt_file_gives_one_notice_opens_home_and_is_kept_as_bad (line 4 text, .bad 0600; `line = 1e30` → notice, no panic), unsafe_session_files_are_ignored_with_one_notice (2 MiB, 0664, symlink, FIFO), numbers_are_clamped_and_counts_capped, missing_file_tabs_are_skipped_with_one_notice (exact S6 text, `… +4`), tabs_outside_the_roots_or_not_regular_files_are_skipped (outside, escaping link, FIFO, `..`, dir; relative root dropped), a_second_session_window_runs_without_saving, the_file_holds_no_buffer_text_and_no_commands, the_ai_section_is_kept_for_the_ai_panel, console_and_path_launches_never_touch_the_session (AC-3); cli::tests::cli_session_flag_opens_home_alone.

## Commands run + result
Red (stub session.rs): `cargo test -p vr-ide session` → `test result: FAILED. 16 passed; 11 failed`

Green, 5 repeated runs: `cargo test -p vr-ide session` → `test result: ok. 27 passed; 0 failed; ... 708 filtered out`

Related set: `run-heavy.sh cargo test -p vr-ide -- session cli event_loop terminal app_tests`
```
test result: ok. 191 passed; 0 failed; 0 ignored; 0 measured; 544 filtered out; finished in 0.86s
```
Guard: `run-heavy.sh sh -c 'cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings'`
```
    Checking vr-ide v0.1.0 (/home/vu-dev/DEV/vr-ide/.worktrees/T1/crates/vr-ide)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.32s
```
(fmt check silent = clean; rustfmt run only on the changed files.)

Smoke (qa-artifacts/T1, temp HOME and state dir): `VR_IDE_EXIT_AFTER_FIRST_FRAME=1 script -qec "vr-ide --session"` → state dir 0700 with `session.lock` 0600 and `session.toml` 0600 (`roots = [<temp home>]`); `vr-ide .` with another state dir → that dir is never created; `vr-ide --session x` → `vr-ide: usage: vr-ide [<dir>|<file>]... | vr-ide <file>.vr-workspace | vr-ide --session`, exit 2.

## Deviations
- Save timing is "first change + 750 ms, checked every 500 ms" (worst case about 1.5 s plus one loop poll) instead of a resetting debounce, so continuous cursor moves cannot postpone the save past 2 s.
- Lock take retries 10 × 10 ms: a child forked by another thread briefly holds a copy of a just-released flock until it execs (seen as a flaky test); without it a restart right after a quit could wrongly report another window.
- Ignored (unsafe) file notice is `Session file ignored (<reason>); opened ~/`, and the second-window notice `Another vr-ide --session window keeps the session; this one is not saved` — S6 has no text for these.
- Terminal folders come from `/proc/<shell pid>/cwd` at save time; a folder outside every root (or gone) restarts in the first existing root. SSH tabs are not saved.
- Cargo.toml gains the existing workspace `toml` dependency (not in the row's file list).

## Out-of-scope issues noticed
- none

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| 0700 dir / 0600 file | brief, recovery.rs | reused `recovery::create_private_dir` / `create_private_file` |
| atomic write | vr-config `write_atomic`, vr-core safe_replace | own `write_file` on top of the private-file helper: `write_atomic` makes the temp with the umask mode and no 0700 dir; safe_replace is the user-file freshness protocol |
| state dir | vr_config | `vr_config::state_dir()` in main |
| root checks (W-1) | workspace.rs | `Workspace::from_paths` (missing kept `(missing)`, nested refused) |
| advisory lock | vr-config recent_files (`File::lock`) | same std API, `try_lock` |
| restart a shell in a folder | commands/terminal.rs | `add_tab_in` (made pub(crate)) |
| explorer expand | commands/explorer.rs | `ensure_loaded`, `Explorer::expand` |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| AC-1 | pass | session_tests::session_without_a_saved_one_opens_home_only; cli::tests::cli_session_flag_opens_home_alone |
| AC-2 | pass | session_tests::session_restores_roots_tabs_explorer_and_terminal_layout (roots, tabs, cursor, scroll, edit mode, active tab, expanded, terminal shown/position/size, fresh shell in its folder, file read fresh); AI struct + API: session_tests::the_ai_section_is_kept_for_the_ai_panel |
| AC-3 | pass | session_tests::console_and_path_launches_never_touch_the_session; main.rs calls `session::start` only when `Launch.session`; smoke: `vr-ide .` creates no state dir |
| AC-4 | pass | session_file_is_private_and_replaces_a_symlink_without_following_it, a_change_is_saved_within_two_seconds, a_corrupt_file_gives_one_notice_opens_home_and_is_kept_as_bad; `save_on_quit` in event_loop::exit |
| SEC-6 | pass | perms/symlink test; unsafe_session_files_are_ignored_with_one_notice (owner/mode via `refused`, 1 MiB, FIFO, link); numbers_are_clamped_and_counts_capped; tabs_outside_the_roots_or_not_regular_files_are_skipped; missing_file_tabs_are_skipped_with_one_notice; a_second_session_window_runs_without_saving; the_file_holds_no_buffer_text_and_no_commands; temp + fsync + rename and stale-temp cleanup (a killed save leaves the old or the new file) |
| S6 notices | pass | exact texts asserted in missing_file_tabs_are_skipped_with_one_notice and a_corrupt_file_gives_one_notice_opens_home_and_is_kept_as_bad |
| AC-11 | pass | related set 191 passed (terminal, event_loop, app, cli, session); full suite at sign-off |
| BASE-11 | pass | no branch-discarding git command used |
| BASE-24 | pass | nothing staged or committed (git status: 8 modified + 2 untracked) |
| BASE-25 | pass | guard output above |
| BASE-38 | pass | rustfmt only on the changed .rs files |
| BASE-16 | pass | Reuse table; capabilities.md entry added |
| COST | pass | related filters only; heavy runs via run-heavy.sh; the one background build finished; nothing left running |
| DESIGN-23, BASE-34 | n-a | no network or external endpoints |

## Fix round 1
Red first: 6 new tests against the round-0 code → `test result: FAILED. 13 passed; 6 failed` (tightened dir 0775 left as 509; symlinked dir gave 0 notices; failed write never retried → NotFound; explorer expanded `inlink/x`; exited terminal saved as primary root; quit save wrote `edit = false`). The rewritten timing test (4) passed on the old code too, since the behaviour was already right; only the test was wrong.

| # | Finding | Reply | Change | Test |
|---|---|---|---|---|
| 1 | state dir mode / symlinked dir / lock owner | fixed | `session.rs` `private_dir`: create 0700; an existing dir must be a real dir (symlink_metadata), owned by the user; group/other bits → chmod 0700, re-checked, else error. Used by `take_lock` and `write_file`. `take_lock` refuses a `session.lock` that is not the user's regular file. Refusal → `Session not saved: cannot lock …` warning, no lock, no writes | `an_open_existing_state_dir_is_tightened_to_0700`, `a_symlinked_state_dir_is_never_written` (other-owner lock file needs root: not testable here) |
| 2 | exited shell pid | fixed | `TermTab.cwd` (spawn folder, set by `add_tab_in`); snapshot reads `/proc/<pid>/cwd` only while `running()`, else `cwd`, else primary root | `an_exited_terminal_keeps_the_folder_it_started_in` |
| 3 | `saved` before a successful write | fixed | `Keeper::new_error` clears `saved` whenever a write error arrives; `tick` takes the error before comparing, so the next check writes again | `a_failed_write_is_retried_at_the_next_check` |
| 4 | timing test | fixed | `a_change_is_saved_within_two_seconds` now waits for the first save (flush), changes at t, ticks every 100 ms and asserts the file changed before t+2 s | same test |
| 5 | explorer link in a middle component | fixed | `restore_explorer` also requires `canonicalize(d)` under a root | `the_explorer_never_expands_through_a_link_out_of_the_root` |
| 6 | edit mode lost behind the quit question | fixed | `tick` notes the active file tab's (path, edit) on every pass while no dialog is open (`Keeper::edit_seen`); `snapshot` keeps that edit mode when `app.quit` or a dialog is open | `edit_mode_left_for_the_quit_question_is_saved_as_it_was` (also: read mode chosen before the quit stays read) |

Files changed this round: crates/vr-ide/src/session.rs, crates/vr-ide/src/session_tests.rs, crates/vr-ide/src/commands/terminal.rs (`TermTab.cwd` field, set in `add_tab_in`).

### New states (round 1)
- `Keeper::edit_seen`: updated on every loop pass without a dialog; quit → used only with `app.quit` or an open dialog; Cancel on the quit question → the next pass records the real read mode; tab switch / close → path no longer matches the active tab, ignored; never written by itself.
- `saved = None` after a write error: the next check (≤ 0.5 s) marks the change pending and writes it again after `SAVE_DELAY`; a folder that keeps failing gives one notice per distinct error text (no notice flood); at quit the error goes to the exit messages.
- `TermTab.cwd`: set once at spawn; ssh tabs keep `None` and are not saved anyway.

### Commands (round 1)
`cargo test -p vr-ide session` (3 runs) → `test result: ok. 33 passed; 0 failed; ... 708 filtered out`

Related set `run-heavy.sh cargo test -p vr-ide -- session cli event_loop terminal app_tests`:
```
test result: ok. 197 passed; 0 failed; 0 ignored; 0 measured; 544 filtered out; finished in 0.84s
```
(One earlier run of the set at load average 36 failed two timing tests that do not touch the session: `event_loop::tests::exit_restores_and_writes_recovery_before_waiting_for_a_replace` (took ≥ 2 s) and `app_tests::external_modify_reloads_within_one_second_with_real_watcher`. Both passed twice run alone (0.8 s), and the whole set then passed as above.)

Guard `run-heavy.sh sh -c 'cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings'`:
```
    Checking vr-ide v0.1.0 (/home/vu-dev/DEV/vr-ide/.worktrees/T1/crates/vr-ide)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.90s
```

### Rule compliance rows touched (round 1)
| Rule | Status | Evidence |
|---|---|---|
| SEC-6 | pass | round-0 tests + an_open_existing_state_dir_is_tightened_to_0700, a_symlinked_state_dir_is_never_written, the_explorer_never_expands_through_a_link_out_of_the_root, an_exited_terminal_keeps_the_folder_it_started_in |
| AC-4 | pass | a_change_is_saved_within_two_seconds (measured after a completed save), a_failed_write_is_retried_at_the_next_check |
| AC-2 | pass | edit_mode_left_for_the_quit_question_is_saved_as_it_was + round-0 restore test |
| AC-11 | pass | related set 197 passed |
| BASE-25 | pass | guard output above |
| BASE-38 | pass | rustfmt only on session.rs, session_tests.rs, commands/terminal.rs |
| BASE-24 | pass | nothing staged or committed |

## Fix round 2
Finding (reviewer + QC, same defect): edit mode lost on the real quit path. `dialog::answer` takes the dialog out, the answer sets `app.quit`, the mode rule puts the tab in read mode, then the loop ticks once more before it returns and `remember_edit_mode` recorded edit=false.
- fixed: `session.rs` `remember_edit_mode` records nothing once `app.quit` is set (it already records nothing while a dialog is open). So the mode from before any quit question is kept: unsaved files (Save all / Discard), running terminals (Quit anyway), and any later question (AI sessions) that answers by setting `app.quit`. Cancel leaves `app.quit` false, so the next pass records the real read mode. A user's own switch to read mode before quitting is recorded by the passes before the quit (`edit_mode_left_for_the_quit_question_is_saved_as_it_was`, second half).
- Tests (real path: open file, edit mode, tick, `app.quit` command → dialog, `dialog::answer(Chosen)` + `sync_mode` as at the end of an input, `app.tick`, `save_on_quit`):
  `edit_mode_is_kept_through_the_unsaved_files_question` (Save all), `edit_mode_is_kept_through_the_running_terminals_question` (Quit anyway).
  Red before the fix: `test result: FAILED. 1 passed; 2 failed` (both at `body.contains("edit = true")`); green after.
- Test-only helper `session::file_for_tests`.
- Files: crates/vr-ide/src/session.rs, crates/vr-ide/src/session_tests.rs.

### Commands (round 2)
- `cargo test -p vr-ide session` → `test result: ok. 35 passed; 0 failed; ... 708 filtered out`
- Related `run-heavy.sh cargo test -p vr-ide -- session cli event_loop terminal app_tests` → `test result: ok. 199 passed; 0 failed; 0 ignored; 0 measured; 544 filtered out; finished in 1.95s`
- Guard `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings` →
```
    Checking vr-ide v0.1.0 (/home/vu-dev/DEV/vr-ide/.worktrees/T1/crates/vr-ide)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.64s
```
- Release build (`run-heavy.sh cargo build --release -p vr-ide`, target/release/vr-ide 15:16), QC replays:
  - `python3 -I qa-artifacts/T1/e6.py`:
```
mid  EDIT a/f.txt · Ln 7, Col 7 · LF · UTF-8 ['edit = true']
dialog: [... '│ 1 terminal still running; quitting ends it. │', '│ [Quit anyway] [Cancel] │']  READ a/f.txt ...
after ['edit = true']
```
  - `python3 -I qa-artifacts/T1/recheck.py`: PASS saved edit = true; PASS relaunch shows EDIT; PASS state dir now 0700; PASS session saved; PASS session.toml 0600; PASS no notice of not saved; PASS quit; PASS 3 roots, 3 tabs, terminal right shown, edit; PASS skipped notice; PASS EDIT + cursor restored; PASS terminal panel restored right.
  - No vr-ide process left (`ps` shows none).

### Rule compliance rows touched (round 2)
| Rule | Status | Evidence |
|---|---|---|
| AC-2 | pass | edit_mode_is_kept_through_the_unsaved_files_question, edit_mode_is_kept_through_the_running_terminals_question; e6.py `after ['edit = true']`; recheck.py "relaunch shows EDIT" |
| AC-11 | pass | related set 199 passed |
| BASE-25 | pass | guard output above |
| BASE-38 | pass | rustfmt only on session.rs, session_tests.rs |
| BASE-24 | pass | nothing staged or committed |
| COST | pass | release build via run-heavy.sh in the background, finished; QC scripts reused, no new scripts |
