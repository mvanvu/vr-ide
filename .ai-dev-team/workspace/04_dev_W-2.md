# W-2 Workspace UI (explorer multi-root, status bar / welcome roots, terminal root, W-1 minors)

Main tree, master 75b9006. No commits (leader commits).

## Progress
- read capabilities.md, spec W, 04_dev_W-1.md, 05_review_W-1.md, decisions D240-D278, brief-common
- red workspace_ui_tests::explorer_shows_one_expandable_node_per_root FAILED (left: ["api", "  src", "  Cargo.toml"] — only the first root shown, D242)
- green explorer_shows_one_expandable_node_per_root (Explorer.roots / set_roots / tree_for / sync_roots; rerooted removed)
- red move_between_roots_is_refused_and_copy_works FAILED ("Explorer: Paste: /tmp/.../web is outside the project": every op confined to the first root); green after root_for/within/below_root per root, typed_folder (Move prompt `name/path`), cross_root refusal in move_paths/answer_move
- each_root_hides_what_its_own_gitignore_lists: first run failed only on my own expectation (hidden files are shown by default); the per-root rules were already in from Explorer::tree_for (first-run green for the behaviour)
- red a_missing_root_is_listed_and_the_root_menu_removes_it_without_touching_files FAILED (screen had no "gone (missing)"); green after the missing mark in ui/explorer.rs, the menu's REMOVE_ROOT item and commands/workspace.rs (workspace.* palette commands)
- workspace_palette_commands_open_their_slash_command: green on its first run — commands/workspace.rs was written in the step before (for the menu item whose test was red); recorded as a BASE-3 deviation
- red new_terminal_from_the_palette_picks_a_root FAILED ("nothing starts before the pick": the palette ran terminal.new directly); red terminal_and_relative_paths_skip_a_missing_first_root FAILED (left "/tmp/.../gone"); green after Workspace::active_root skips missing roots, terminal spawn(cwd), new_terminal_in, `/term new [root]`, slash::pick_terminal_root from palette accept
- welcome tests: red welcome_lists_each_root_and_the_recent_workspaces ("PROJECT" missing), a_click_on_a_recent_workspace_opens_it (no recent workspace row), welcome_multi_root_snapshots (new); green after welcome root_rows / workspace_rows / hits, App.root_info / root_files / welcome_hits, click routing
- red add_folder_checks_run_off_the_ui_thread (the late refusal was an error in the window, not a notice); red open_workspace_drops_results_of_the_old_roots ("stale results kept", after a stub clear_stale); FIFO test hung on the sync /ws open (stopped my own run, TaskStop) = red; green after WsJob (run_ws_job / drain_ws_job / add_root_checked / open_workspace_from), search::clear_stale, slash ws_add/ws_open/ws_recent through them
- vr-config: red read_workspace_refuses_a_fifo_a_folder_and_a_huge_file (todo!() stub; old code: a folder gave "Is a directory", a FIFO blocks); green with metadata + handle is_file checks and MAX_WORKSPACE_FILE (1 MiB) cap
- code-first (BASE-3 deviation, tests then passed on first run): saving_a_workspace_through_a_linked_folder_keeps_relative_roots, a_root_with_a_non_utf8_name_is_refused_for_files_and_lists (written in the same edit as the WsJob code); status_bar_shows_the_root_name_with_several_roots (W-1 display_path already did it)
- snapshots reviewed and accepted: welcome_multi_root_{120x40,80x24,40x12}, render_multi_root_explorer_{120x40,80x24,40x12}, render_palette_80x24 (new Workspace: rows match "sa")
- guards: fmt clean, workspace clippy -D warnings clean; full suite via run-heavy.sh green; release build ok; pty walk 9/9

## Files changed
New: crates/vr-ide/src/commands/workspace.rs, crates/vr-ide/src/workspace_ui_tests.rs, 6 new snapshots (src/snapshots/vr_ide__workspace_ui_tests__render_multi_root_explorer_{120x40,80x24,40x12}.snap, src/ui/tests/snapshots/..welcome_multi_root_{120x40,80x24,40x12}.snap).
Changed (vr-ide): commands/explorer.rs (ExplorerRoot, roots / set_roots / tree_for / root_of / sync_roots / load_root, Row.label/missing, per-root confinement root_for / within / below_root, typed_folder, cross_root refusal; `rerooted` removed), commands/explorer_menu.rs (REMOVE_ROOT item), ui/explorer.rs (missing mark), app_workspace.rs (WsJob, add_root_checked, open_workspace_from, refresh_root_info, count_root_files, save_workspace: non-UTF-8 refusal + canonical base, remember_workspace skips non-UTF-8, roots_changed via sync_roots, clear_stale on switch), app.rs (fields root_info / root_files / welcome_hits / ws_job / ws_job_wait, RootInfo, explorer roots at construction, tick refreshes root info, index counts, welcome click), workspace.rs (check_folder / add_checked / named_paths / non_utf8_root / NotUtf8, active_root skips missing roots, check_against), commands/terminal.rs (spawn(cwd), add_tab_in, new_terminal_in), commands/slash.rs (`/term new [root]`, pick_terminal_root, open_recent_workspace, ws_add / ws_open / ws_recent off-thread, set_input pub(crate)), commands/palette.rs (New Terminal pick), commands/search.rs (clear_stale), commands/{mod,registry}.rs, event_loop.rs (drain_ws_job), ui/welcome.rs (PROJECT / WORKSPACES blocks, hits), ui/editor_view.rs, lib.rs, slash_tests.rs (term new title), ui/tests/welcome_tests.rs (+3 tests), snapshots render_palette_{80x24,40x12} (new Workspace rows).
vr-config: workspace_file.rs (read_workspace regular-file + 1 MiB cap, MAX_WORKSPACE_FILE, test), recent.rs (`same` pub), lib.rs (export). vr-fs: untouched.
Docs: docs/commands.md (/term new [root]), docs/keymap.md (workspace.* palette commands, terminal root pick).

## Design calls (D? (W-2), appended to decisions.md)
1. All roots read and expanded at load; roots collapse only with several; missing mark learned on read (no frame stat).
2. Hidden/ignored toggles global, ignore rules per root.
3. Ops confined per root; moves/cut-paste across roots refused per path, copy allowed; Move prompt takes `root-name/path`.
4. Root-row menu: extra item "Remove Folder from Workspace" (not with one root); right click below the rows targets the last root.
5. workspace.* palette commands open the /ws slash command; New Terminal from the palette with several roots opens the `/term new ` root list (active root highlighted); key keeps the active root.
6. Welcome PROJECT / WORKSPACES blocks; branch/missing cached with monitor samples; counts once per index.
7. Add Folder / Open workspace reading on a worker with a 300 ms in-place wait; later results as notices; a newer request replaces.
8. Non-UTF-8: Add refuses; save refuses; recent skips with notice; read_workspace regular file ≤ 1 MiB.
9. Open Workspace drops results not searched again.

## New states (round 0)
| State | quit | cancel/Esc | error halfway | input changes | second request | concurrent writer | hung call | test |
|---|---|---|---|---|---|---|---|---|
| WsJob (Add Folder check / workspace read on a worker) | detached thread, nothing to wait for | the F1 window closes; the job still applies when it answers (notice) | error → warning notice, nothing changes | roots changed meanwhile: add_checked re-checks against the current roots | replaces the pending job (old result never read) | no file writes | worker blocked (dead mount / FIFO swapped after the stat); UI never waits more than 300 ms | add_folder_checks_run_off_the_ui_thread, opening_a_fifo_as_a_workspace_file_is_refused, read_workspace_refuses_a_fifo_a_folder_and_a_huge_file |
| root_info / root_files caches | n-a | n-a | git_branch errors → None | roots change → entries of removed roots dropped, new ones on the next sample | n-a | n-a | git_branch reads .git/HEAD with its FIFO guard (monitor tests) | welcome_lists_each_root_and_the_recent_workspaces |
| ExplorerRoot.missing | n-a | n-a | unreadable root → empty, warning | watcher event / toggles re-read the root and clear or set it | n-a | n-a | listing a root on a dead mount blocks the UI like any folder (pre-existing explorer design) | a_missing_root_is_listed_and_the_root_menu_removes_it_without_touching_files |

## Commands run + result (trimmed)
- `cargo test -p vr-ide --lib workspace_ui_tests`: 16 passed (after each step; reds in Progress).
- `run-heavy.sh cargo test -p vr-ide -p vr-config -p vr-fs`: vr-config 17 + 22 + 8, vr-fs 44 (1 ignored) + 2, vr-ide 513 passed (7 ignored) — all ok.
- `cargo fmt --all --check`: clean. `run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings`: Finished, no warnings.
- `run-heavy.sh cargo test --workspace` (qa-artifacts/W-2/dev/full-suite.log): every `test result: ok`, exit=0 (vr-ide 513, vr-search 43/44, terminal 155, ...).
- `run-heavy.sh cargo build --release -p vr-ide`: Finished release, exit=0.
- pty walk `python3 -I qa-artifacts/W-2/dev/walk.py` (release `vr-ide dirA dirB`, legacy bytes F1 = ESC O P, ctrl+e, NUL, ctrl+q; SGR 1006 clicks): 9/9 PASS (welcome PROJECT, explorer roots, status `dirB/b.txt`, root menu Remove item, palette New Terminal root list, pwd = …/dirB, Remove Folder, files kept, recent.toml in a private state dir). Screens walk_01..09.txt. Nothing left running.

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| root pick / path completion | slash.rs ArgKind::Root, Path | reused (`/term new [root]`, palette commands open /ws commands) |
| per-root listing with ignore rules | vr_fs::Tree | one Tree per root via tree_for |
| context menu | explorer_menu.rs (T23) | one extra item |
| FIFO-safe read | vr-search open_regular (crate-private, rustix) | same checks with std in vr-config + the worker thread (no new dependency) |
| branch | monitor::git_branch via Root::branch | reused in refresh_root_info |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | W-2 items only; vr-config change limited to read_workspace guard + `same` pub (review minor) |
| BASE-2 | pass | W-1 API kept (add_root sync, remove_root, open_workspace, save_workspace); additive helpers listed in capabilities.md |
| BASE-3 | pass (with deviations noted) | red line per test in Progress; code-first: palette commands test, linked-folder save, non-UTF-8 test; first-run green: status bar, gitignore behaviour |
| BASE-4 | pass | Commands run section |
| BASE-5 | pass | suite, clippy, release through run-heavy.sh |
| BASE-6 | pass | English, why-comments |
| BASE-8 | pass | no manifest change |
| BASE-10 | pass | vr-ide, vr-config, vr-fs tests + the one requested full run |
| BASE-11 | pass | no stash/checkout/reset |
| BASE-14 | pass | full suite + release as a background task |
| BASE-15 | pass | stopped only my own hung test run (TaskStop) |
| BASE-16 | pass | Reuse table; capabilities.md appended |
| BASE-17 | pass | no installs |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | this file |
| BASE-24 | pass | nothing staged or committed |
| BASE-25 | pass | fmt + clippy clean |
| BASE-32 | pass | D240-D248, D109, D110, D241, D262/D276, USER-5 (no new keys) |
| BASE-38 | pass | `cargo fmt --all` changed only my files (git status checked) |
| COST | pass | related tests, one full run, one release build, scripts in qa-artifacts/W-2/dev, private config/state dirs, nothing left running |
| new-states rule | pass | table above |
| AC-W1 | pass | explorer_shows_one_expandable_node_per_root, status_bar_shows_the_root_name_with_several_roots, walk |
| AC-W2 (UI) | pass | search results grouped by root (W-1), open_workspace_drops_results_of_the_old_roots |
| AC-W3 | pass | a_missing_root_..._removes_it_without_touching_files, add_folder_checks_run_off_the_ui_thread, walk |
| AC-W4 (UI) | pass | workspace_palette_commands_open_their_slash_command, a_click_on_a_recent_workspace_opens_it, saving_a_workspace_through_a_linked_folder_keeps_relative_roots |
| AC-W5 | pass | move_between_roots_is_refused_and_copy_works, each_root_hides_what_its_own_gitignore_lists |
| AC-W6 | pass | new_terminal_from_the_palette_picks_a_root, terminal_and_relative_paths_skip_a_missing_first_root, walk pwd |
n-a: BASE-34 (no external sources), design-gate (USER-3: no mockup round)

## Round 1 (review fixes, D292)

### Progress
- Findings read: 05_review_W-2.md (1 major, 3 minor), D292.
- F1 (major) red: `a_replaced_workspace_job_is_named_in_a_notice` (no notice), `a_late_workspace_answer_waits_for_the_open_dialog` ("applied under the dialog" left 2 right 1) → green.
- F2 (minor) red: `moving_into_a_folder_link_to_another_root_is_refused` (x.txt moved into web/sub) → green.
- F3 (minor) red: `a_missing_root_is_not_watched` (2 notices: missing + cannot watch) → green. The release walk then still showed the watch error (explorer hidden on the welcome screen, so the root was not yet marked missing when the watches started); red test `a_missing_root_is_named_once_when_watches_start_before_the_explorer` (2 notices) → green.
- F4: README updated.

### Files changed (round 1)
- crates/vr-ide/src/app_workspace.rs — `WsJob.held`: the answer is kept while `app.dialog` or `app.palette` is open and applied on the first `drain_ws_job` pass after both close; `run_ws_job` posts "<old> cancelled by <new>" when it replaces a pending job; `has_answer` (test-only accessor).
- crates/vr-ide/src/commands/explorer.rs — `really_in_another_root`: `move_paths` (cut+paste and Move) canonicalizes `dest` once per batch and refuses every path with the `cross_root` message when the real path is inside another (not missing) root; copy is unchanged. `missing_root_notice` shared; `load_root` skips it when the root was already named.
- crates/vr-ide/src/app.rs — `sync_explorer_watches`: roots marked missing are never watched; with several roots, a root about to be watched that is not found (one stat per new root watch, never per frame) is marked missing and gets the single missing notice instead of a watch error.
- crates/vr-ide/src/workspace_ui_tests.rs — 5 new tests (above) + `held_job` helper (blocks the job on a channel; deterministic with `ws_job_wait = ZERO`).
- README.md — "Workspace (several folders)" section (multi-root CLI `vr-ide dir1 dir2 …`, `.vr-workspace` alone, missing mark, moves refused between folders incl. through links, copy allowed, `/ws` commands); "at most one argument" sentence replaced; project-root sentence points to it; Move prompt wording with several roots (`folder-name/path`, same folder); right-click "Remove Folder from Workspace" (not on the last folder; files and tabs stay).
- No key binding changed (T24 owns D280/D290/D291).

### Commands run (round 1)
- `cargo test -p vr-ide --lib -- <new tests>` red/green as above.
- `cargo fmt --all -- --check` → rc 0.
- run-heavy `cargo clippy --workspace --all-targets -- -D warnings` → rc 0 (first run flagged `has_answer` unused outside tests → `#[cfg(test)]`). Log: qa-artifacts/W-2/dev-r1/clippy.log.
- run-heavy `cargo test -p vr-ide -p vr-config -p vr-fs` → 610 passed, 1 failed: `syntax_tests::the_first_frame_does_not_wait_for_the_parse` ("first frame took 2.95s", load average 25–28 from other agents); rerun alone → ok (0.38 s), and ok in the full run. Log: dev-r1/related.log.
- run-heavy `cargo test --workspace` (once, final code) → 922 passed, 0 failed, 17 ignored. Log: dev-r1/full-suite.log.
- run-heavy `cargo build --release` → Finished (binary newer than every changed source). Log: dev-r1/release.log.
- `python3 -I qa-artifacts/W-2/dev-r1/walk.py` (copy of review/walk.py; the link probe is now a check, plus a startup check "no watch error") → 10/10 PASS. Screens: dev-r1/walk_01..10.txt, log dev-r1/walk.log.
- Earlier interim run (under load) also showed `explorer_tests::drop_nested_keeps_order_and_scales` at 1.23 s; rerun alone ok, ok in the full run; code untouched.

### Rule compliance (round 1 updates)
| Rule | Status | Evidence |
|---|---|---|
| BASE-3 | pass | red line per finding above, each test written before the fix |
| BASE-4 / BASE-5 | pass | commands above; heavy runs via run-heavy.sh |
| BASE-10 | pass | related crates + one full run on final code |
| BASE-11 / BASE-24 | pass | no commit, stash, checkout, restore or reset |
| BASE-14 | pass | heavy chain as background tasks, no polling loops |
| BASE-25 | pass | fmt --check rc 0, clippy -D warnings rc 0 |
| BASE-32 | pass | D292 applied; D109 kept (copy into links allowed; links outside every root left to `below_root`); no key changes (USER-5, T24) |
| new-states | pass | WsJob "second request replaces" → notice + test; late answer under dialog/palette held → test |
| AC-W3 | pass | missing root: one notice, not watched (2 tests, walk_02) |
| AC-W5 | pass | move through a folder link into another root refused (test, walk_07); copy works |
| COST | pass | walk in private config/state dirs; nothing left running |
