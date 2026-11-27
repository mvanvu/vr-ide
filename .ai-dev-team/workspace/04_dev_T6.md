# T6 AI changes: pre-images, list, diff, keep, revert (D8)

## Progress
- started: reading spec, threat model, ui-spec, T5 notes
- read: spec D8, SEC-9..13, ui-spec S3 + key plumbing, 7 mockups, T5 notes, ai/mod.rs, ui/ai_view.rs, app.rs, editor_view, watch.rs, safe_replace.rs, document.rs. Warm build ok.
- design: own flat inotify watches per directory (bounded) + baseline copy under the "ctime predates session start" rule; no git blobs (filters make blob bytes differ from the worktree bytes).
- red->green: vr-core `watch_dirs_reports_entries_of_each_and_unwatch_dirs_ends_it` (E0599 no method `watch_dirs`) -> 1 passed. vr-core: `Watcher::{watch_dirs, unwatch_dirs}`, `OwnWrites::is_own` pub, `create_temp` pub.
- code: ai/changes.rs (store, walk, events, diff, keep, revert) and ai/changes_tests.rs: 19 tests. The backend tests were written after the backend code (red not observed for them); the first run was red on two real defects the tests found (events through a linked folder were recorded; a link at the final path could not be replaced through safe_replace) -> fixed -> 19 passed.
- resumed after an interruption: five files hit by a faulty bulk substitution (`\|` inside a `|`-delimited perl pattern) were repaired by hand (ai/mod.rs, ai/changes.rs, ui/ai_view.rs, ui/ai_changes.rs, ui/tabs.rs); build + tests confirm.
- red: ai_changes_tests.rs written before review.rs / ui (27 compile errors: no `ai::review`, `AiSession::new`, `list`, `diff`, `ConfirmDialog.note` ...) -> code -> 9/12 -> fixes -> 12 passed. Screen tests + 7 snapshots (generated once). Keymap, docs, capabilities.

## Status: DONE (worktree .worktrees/T6, branch run4-T6; nothing staged, nothing committed, nothing running)

## Mechanism (SEC-9)
- A tracker per session (`ai/changes.rs`), created before the agent's program is spawned; `t0` = that moment.
- A worker thread walks the session folder once, breadth first: one flat inotify watch per directory (set before the directory is listed), `.git`, `.Trash*`, state / config / trash folders and vr-ide temp files skipped, links never followed.
- A file is copied into the store only when (a) its directory is kept by the ignore rules (`ignore` crate: .gitignore etc.), (b) it is a regular file opened `O_NOFOLLOW|O_NONBLOCK`, at most cap bytes, stamp equal before and after the read, and (c) max(mtime, ctime) lies before `t0` minus a margin (100 ms; 2 s on whole-second file systems). ctime cannot be set back by a program, so a copy that passes is the before-content. A file that fails (c) during the walk is listed at once as changed without an earlier version.
- Events never produce a pre-image: an event for a file with a baseline copy makes a record carrying that copy; for a file without one, a record "no earlier" (Keep only). Later baselines (after Keep, after an own save of the editor, after Revert) hold the content of that moment.
- Git blobs are not used: clean/smudge filters and autocrlf make blob bytes differ from the worktree bytes, and Revert must be byte-exact.
- Bounds: 8 MiB/file, 256 MiB/session, 512 MiB over the sessions of this vr-ide, 5 000 records, 10 000 watched directories, 200 000 remembered files; over a bound → row `too large` / `limit`, or the "may be incomplete" banner (watch limit, file limit). Program exit pauses the tracker (walk copies dropped, records kept); restart walks again.
- Revert: expectation (`Seen`: lstat stamp + content hash, racy rule) taken when the diff is built; parent folder opened as an fd and every path of the swap goes through `/proc/self/fd/N/name` (closes the canonical-parent window of the threat model's accepted risk); `safe_replace::replace_if_unchanged` / `create_if_absent`; a link at the final path is exchanged out, never written through; mode = recorded & 0o777; temp via `vr_core::create_temp` (watcher ignores it); created file → `App.trash`.

## Files changed
- new: crates/vr-ide/src/ai/{changes.rs, changes_tests.rs, review.rs}, src/ai_changes_tests.rs, src/ui/ai_changes.rs, src/ui/tests/ai_changes_view_tests.rs + 7 snapshots.
- changed: vr-core {watch.rs (`watch_dirs`, `unwatch_dirs` + test), document.rs (`create_temp`, `OwnWrites::is_own` public), lib.rs}; vr-config defaults/keymap.toml (global f6 / f5 / shift+f5, `[ai]` f6), tests/config.rs; vr-ide Cargo.toml (`similar`, `ignore` as dependencies; Cargo.lock follows), ai/mod.rs, app.rs (store dir, exclusions, start-up sweep thread), commands/{dialog.rs (`AiRevert`), registry.rs}, lib.rs, ui/{ai_view.rs (list hook, `±N` in `tab_bar`/`changes_badge`, status `±N f6`), dialog.rs (`ConfirmDialog.note`), editor_view.rs (diff gutter / row backgrounds), layout.rs (`diff_head`), mod.rs, status_bar.rs, tabs.rs}, ui/tests/ai_view_tests.rs (`AiSession::new`), ai_tests.rs (f6 is now `[ai]`'s; shift+f5 checked instead), app_tests.rs (REMOVED_V2), snapshot render_palette_80x24 (three new commands).
- docs: README.md (AI panel: rows + "AI changes" paragraphs), docs/keymap.md (global rows, `[ai]` f6, built-in row for the list). .ai-dev-team/capabilities.md (one line).

## Tests
- vr-core `watch_dirs_reports_entries_of_each_and_unwatch_dirs_ends_it`.
- `ai::changes_tests` (19): sec9 ×3, created/deleted/undone, folder moved away + keep, exit/restart, sec10 ×6 (perms 0700/0600 + 0775 base closed + symlinked base refused, caps, record cap, exclusions with the state dir inside the root, index validation, keep/close/sweep), sec11 (FIFO, symlink, linked folder, outside paths), sec12 ×4 (changed since shown incl. the BeforeSwap seam, parent swapped for a link, link at the final path + binary + 0755/setuid, trash unavailable), sec13 banner, real watcher thread.
- `ai_changes_tests` (12): f6 list + diff, keep, revert dialogs (texts, Cancel default) for M / A / D, refusal + diff reload, unsaved edits refused + clean tab reloads, no earlier version, f5/shift+f5 inert elsewhere, diff tab read-only / not in recovery or recent / find works, sanitized names + banner, mouse, close removes the store, one real agent script end to end.
- `ui::tests::ai_changes_view_tests` (5) + `ui::ai_changes::tests` (1).

## Commands run (trimmed)
- `cargo test -p vr-core watch_dirs` red: `error[E0599]: no method named watch_dirs` → `ok. 1 passed`.
- `cargo test -p vr-ide --lib ai_changes_tests` red: `could not compile ... 27 previous errors` → `ok. 12 passed`.
- `cargo test -p vr-ide --lib changes_tests`: first `17 passed; 2 failed` → `ok. 19 passed`.
- `INSTA_UPDATE=no cargo test -p vr-ide` (qa-artifacts/T6/test-ide2.log): `ok. 773 passed; 0 failed; 7 ignored`, `test exit 0` (+1 zone-file test added after: 774).
- `run-heavy.sh cargo test -p vr-ide -p vr-config -p vr-core` (test.log): vr-config `17 / 26 / 8 passed`; vr-core `155 passed; 1 failed` → rerun `ok. 156 passed`.
- Guard `cargo fmt --all --check && run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings` (guard.log): `Finished dev profile`, `guard exit 0`.

## Deviations from the plan / spec
- Red first not followed for `ai/changes.rs` (tests written right after the code; they then found two defects). Followed for vr-core and the app/UI layer.
- The diff body has no syntax colours (the diff tab is a generated buffer without a language); `+`/`-` signs, row backgrounds, hunk and note styles are there.
- The index file is written and `load_index` validates it (tested), but no flow reads it back: a store is deleted when its session closes or vr-ide quits and swept after a crash, so restored sessions start with an empty list.
- The 512 MiB total is counted over the sessions of one vr-ide process, not across several running vr-ide instances.
- Row shape differs slightly from the mockup at equal width (` M session.rs …es/vr-ide/src +9 -3`: the folder uses the room the shorter counts leave). Dialog frame is the app's existing one (`┌ Revert session.rs? ──`), up to 60 columns wide, so the 80x24 texts wrap differently from the mockup.
- f5 / shift+f5 are global keymap entries whose commands do nothing without a target; `[terminal]` and `[ai]` do not list them, so they reach the program there.
- Outside the row's file list: vr-core (watch.rs, document.rs, lib.rs), vr-ide Cargo.toml / Cargo.lock, ai/review.rs (new), ui/{dialog,editor_view,layout,tabs,status_bar}.rs, commands/{dialog,registry}.rs, app.rs.

## Out-of-scope issues noticed
- Flaky under load, pass alone / on rerun: `vr-core document::tests::disk_version_read_racing_the_end_of_the_window_stays_racy`, `vr-ide search_tests::project_replace_never_loses_a_concurrent_write`.
- docs/commands.md and the welcome screen do not mention f6 (not in scope).

## New states
- Tracker: scanning / running / paused (program exited) / closed. Quit and tab close drop it (store removed, worker stops at its next loop turn, ≤ 100 ms); a crash leaves the store for the next start's sweep (lock file held by live sessions). A FIFO or device in the folder is never opened; a hung network file system would stall only the worker thread.
- List open (focused / session focused), diff tab open, Revert dialog open: Esc cancels; the file changing meanwhile → refusal + reloaded diff; session closed meanwhile → the answer does nothing.

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| swap only if unchanged | capabilities.md, vr-core/safe_replace.rs | reused `replace_if_unchanged`, `create_if_absent`, `exchange_files`, `kept_error` |
| trash | vr-fs ops.rs | reused `App.trash` (`vr_fs::Trash`, `FakeTrash` in tests) |
| private dir / file | recovery.rs | reused `create_private_dir`, `create_private_file` |
| file watcher | vr-core/watch.rs | reused `Watcher` (own instance per session, like git.rs); added `watch_dirs` / `unwatch_dirs` because `watch_dir` re-syncs all watches per call |
| own-write filter | document.rs `OwnWrites` | reused (`is_own` made public) |
| temp file the watcher ignores | document.rs `create_temp` | reused (made public) |
| line diff | vr-git/diff.rs uses `similar` | reused the crate; vr-git's diff is line marks against HEAD, not a unified diff |
| ignore rules | vr-fs file index uses `ignore` | reused the crate |
| read-only generated tab | commands/slash.rs `/keys` | same pattern (`View.title`) |
| confirm dialog, sanitize, `shorten_left`, `wrap` | ui/dialog.rs, ui/safe_text.rs, ui/status_bar.rs | reused; `ConfirmDialog.note` added |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| AC-9 / D8 record, list per session, ±N | pass | `ai_changes_tests::{f6_opens_the_list…, a_real_agents_changes…}`; `ai_changes_view_tests::ai_changes_count_in_the_tab_bar_and_status` |
| AC-9 diff in the editor (DIFF badge) | pass | `ai_changes_view_tests::ai_changes_list_and_diff`, snapshots ai_changes_120x40 / 80x24 |
| AC-9 Keep / Revert (asks, refuses on change, atomic, created → trash) | pass | `ai_changes_tests::{keep_drops…, revert_asks_first…, revert_is_refused…}` |
| AC-9 UI states the attribution | pass | list statement + dialog note asserted in `ai_changes_list_and_diff`, `ai_revert_dialogs_and_refusal` |
| SEC-9 true pre-image or none | pass | `changes_tests::sec9_*` (3), `without_an_earlier_version_only_keep_works` |
| SEC-10 store | pass (see deviations: index not read back; total per process) | `changes_tests::sec10_*` (6) |
| SEC-11 capture reads | pass | `changes_tests::sec11_only_regular_files_inside_the_root_are_read`; `open_regular` in ai/changes.rs |
| SEC-12 revert | pass | `changes_tests::sec12_*` (4), `ai_changes_tests::{revert_is_refused…, revert_refuses_unsaved_edits…}` |
| SEC-13 honest list, no spill | pass | `changes_tests::sec13_*`, `ai_changes_tests::sec13_*` (2), snapshot ai_changes_limits_120x40 |
| D13 accepted risk | pass | README "AI changes" states 0600 copies until Keep / Revert / close |
| UI S3 + key plumbing, 7 mockups | pass (deviations above: no syntax colours in the diff, dialog frame) | 7 snapshots; vr-config `ai_context_keeps_only_its_table`; `f5_and_shift_f5_do_nothing_outside…`; `ai_tests::a_focused_session_gives_other_keys_to_the_agent` |
| T7 isolation of the tab-label change | pass | `changes_badge` + `TabBar.changes` in ui/ai_view.rs; `tab_text` untouched |
| BASE-11 | pass | no checkout / restore / stash / reset run |
| BASE-14 | pass | no waiting loops; one background build |
| BASE-15 | pass | nothing stopped by name |
| BASE-16 | pass | Reuse table |
| BASE-17 | pass | nothing installed |
| BASE-18 | pass | no agents started |
| BASE-19 | pass | this file |
| BASE-24 | pass | `git diff --cached` empty |
| BASE-25 | pass | guard.log `guard exit 0` |
| BASE-34 / DESIGN-23 | n-a | no network use |
| BASE-38 | pass | `rustfmt --edition 2024` on changed files only |
| COST related tests, snapshots once | pass | whole vr-ide crate (app routing / render shared) once + reruns of failures; one `INSTA_UPDATE=always` run |
| COST red first | fail for ai/changes.rs only | see deviations |
| COST-8 | pass with one mishap | Edit / Write / perl scripts; the faulty perl pattern is noted above |
| COST-9 | pass | nothing left running |
| COST-13 | pass | logs in qa-artifacts/T6; scripts in the session scratchpad |
| Comments English, why-only | pass | new files |

## Late check
- `cargo test -p vr-ide --lib ai` repeated 8 times: 6 × `ok. 104 passed`, 2 × one failure, both `ai_tests::a_focused_session_gives_other_keys_to_the_agent` (`timed out; screen:` empty, in `wait_for`). `cargo test -p vr-ide --lib ai_tests` repeated 12 times: 12 × ok. This run changed that test's key list (f6 → shift+f5, since f6 now belongs to `[ai]`); whether the timeout is load on the PTY tests or that change is not established.

## Merge
Merge of master (T1 session, T2 desktop, T3 ssh, T5 final AI panel + restore + D15, T7 attention) into run4-T6, resolved by a second developer in .worktrees/T6. Nothing staged or committed (BASE-24); no checkout / restore / stash / reset / abort (BASE-11).

### Resolved files (conflict markers gone; still `UU` until the leader stages them)
- `crates/vr-ide/Cargo.toml`: both dependency sets (`similar`, `ignore` + `toml`).
- `crates/vr-ide/src/ai/mod.rs`: modules `attention` + `changes` (+ `changes_tests`); `AiSession` keeps T6's `serial / changes / records / banner / changes_version` and T7's `notify`; `AiSession::new` initialises `notify: Default::default()` (with `attention: None`); `spawn_session` keeps T6's form (tracker via `review::start` before `start_program`, then `AiSession::new`, `name`, `changes`).
- `crates/vr-ide/src/ui/tests/ai_view_tests.rs`: fake sessions through `AiSession::new` (T7's `notify` comes from `new`).

### Semantic check of the merged files
- Every `AiSession` (the panel's type) is built by `AiSession::new`: `spawn_session` (picker, `/ai`, `open_tool`) and T5's restore (`ai::restore::restore` → `spawn_session`), tests (`ai_view_tests`, `ai_changes_tests`, `ai_changes_view_tests`). `session::AiSession` struct literals in restore.rs / ai_tests.rs / session_tests.rs are the session-file type, not the panel's.
- The tracker comes before the spawn on every path: `spawn_session` (restored sessions included); `restart` (Enter on an exited session) calls `changes.resume()` before `start_program`.
- `start_program` sets `SpawnExtras.attention = true` (T7) for new, restored and restarted sessions.
- `ai::refresh`: T7's `attention::collect` first, T6's `review::refresh` last; both results count.
- `ui/ai_view.rs` tab bar: T7 mark (`mark: attention.is_some()`, marked tabs stay out of `+N`) and T6 `changes_badge` (`±N`, levels 0–1) are both in place.
- `ui/status_bar.rs`: T7's `render` reflows the bar into the width the badge leaves and calls `render_bar`, which runs T6's `ai_changes::render_status` (list / diff status) first, then the AI / terminal / editor status.
- `handle_key` order: `[ai]` table → NUL toggle → picker → T6 `review::handle_key` (list keys) → T5 scroll back (also on exited sessions) → exited / running session keys.
- keymap.toml merged cleanly: global f6 / f5 / shift+f5, `[ai]` f6; T5's built-in keys comment extended by T6.
- Snapshots: none changed, none regenerated (no `.snap.new`).

### Commands (trimmed; logs in qa-artifacts/T6/)
- `run-heavy.sh cargo build -p vr-ide --tests` → `Finished dev profile` (no warnings).
- `INSTA_UPDATE=no run-heavy.sh … cargo test -p vr-ide -- ai attention session status ai_view ai_changes` (merge-test.log, merge-rerun.log): lib `168 passed; 1 failed` (`search_tests::apply_reports_a_file_failing_halfway`, see below); main `ok. 0`, desktop_cli `ok. 0 passed; 3 filtered out`.
- `cargo test -p vr-core` → `ok. 156 passed; 0 failed; 2 ignored`.
- `cargo test -p vr-config` → `ok. 17 passed`, `ok. 26 passed`, `ok. 8 passed`.
- Guard `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings` → `guard exit 0`.
- `cargo test -p vr-ide --lib ai` x 18 (merge-rerun.log, roundtrip.log): 14 x `ok. 121 passed`; the failures: 4 x `search_tests::apply_reports_a_file_failing_halfway`, 1 x `event_loop::tests::exit_restores_and_writes_recovery_before_waiting_for_a_replace` (same replace worker), 1 x `ai_tests::the_panel_round_trips_through_the_session_file` (:528).
- `cargo test -p vr-ide --lib search_tests` → `ok. 32 passed` (includes the apply test).

### Failures outside the merged files (not mine)
- `search_tests::apply_reports_a_file_failing_halfway — search did not finish` (search_tests.rs:56). Not a hang: `/proc/<pid>/task/*/wchan` of the replace worker = `jbd2_log_wait_commit` (fsync of `.vr-ide-save-*.tmp` in /tmp on ext4), 13 of 80 files written after ~15 s; `/proc/pressure/io` `full avg60=45%`, load 31 (other worktrees building). The 20 s deadline is shorter than 80 fsyncs under that disk pressure; passes with `search_tests` (32 ok) and in 14 of 18 `ai` runs.
- `ai_tests::the_panel_round_trips_through_the_session_file` (:528, `!body.contains("gemini")`, 1 of 18; 6 single reruns ok). Likely cause (from the code, the failing body not captured): `session::take_lock` retries `try_lock` 10 x 10 ms; a PTY child forked by another test thread holds a copy of the just-released lock fd until it execs, so under load the second window gets no lock, `saves()` is false, `save_on_quit` writes nothing and the file keeps window 1's AI section (the `shown = false` it matched is the terminal section's). Specific to several windows in one test process; owner session.rs (T1).

### Flaky `ai_tests::a_focused_session_gives_other_keys_to_the_agent`
- 0 failures in 18 post-merge `ai` runs. The test already waits on conditions (`wait_for`, 10 s deadline, 10 ms steps, live `snapshot`), so a condition-based wait changes nothing. The earlier failures showed an empty screen, which means the first wait (`CWD:` never printed), before any key is sent: neither the f6 → shift+f5 key change nor the keys could cause it. No product race found in the path (tracker start does not block; the reader parses every read, the snapshot spin only yields). Cause: the agent script not printing within 10 s while the machine is under the I/O pressure measured above. No change made.

### Rule compliance rows (merge)
| Rule | Status | Evidence |
|---|---|---|
| D7 / D12 / D16 panel behaviour kept | pass | ai_tests in `cargo test -p vr-ide --lib ai` 121 ok |
| D8 changes (tracker before spawn, restored too) | pass | `spawn_session` (ai/mod.rs), restore → `spawn_session`; ai_changes_tests green |
| D9 attention (SpawnExtras.attention, notify, badge) | pass | `start_program`; ai_tests notify tests + ai_hidden_notify snapshots unchanged |
| D13 / D15 | pass | exited-row tests (`an_exited_session_scrolls_back`, `the_exited_line_is_drawn_below_the_agent_output`) green |
| BASE-11 | pass | no checkout / restore / stash / reset / merge --abort |
| BASE-14 | pass | long runs as background tasks via run-heavy.sh, no waiting loops |
| BASE-15 | pass | only my own background tasks; nothing killed |
| BASE-24 | pass | nothing staged or committed |
| BASE-25 | pass | guard exit 0 |
| BASE-38 | pass | no formatter run beyond `cargo fmt --check` |
| COST-8 | pass | conflict hunks resolved with tested `perl -0pi` patterns, result reviewed in `git diff` |
| COST-9 | pass | nothing left running |
| COST-13 | pass | scripts / logs in qa-artifacts/T6 |
| COST-15 | pass | no snapshot regenerated |

## Merge master (b277e88) — 2026-11-27

Resolved (markers removed, both sides kept): crates/vr-ide/src/ai/mod.rs (AiPanel fields + `new`: T6 changes_* fields and master `state_dir`; `spawn_session`: account hold first, then `review::start`), crates/vr-ide/src/commands/dialog.rs (`AiRevert` and `RemoveAiAccount` variants and answer arms), crates/vr-ide/src/lib.rs (both test modules `ai_account_tests`, `ai_changes_tests`), crates/vr-ide/src/ui/tests/ai_view_tests.rs (T6's `AiSession::new` form).

API adaptations:
- `AiSession::new` (T6) gained `account_lock: None` for master's new field; `spawn_session` sets `session.account_lock = account_lock;` after construction (keeps the 5-arg signature used by T6 tests). `session.name = name` kept.

Tests: `cargo test -p vr-ide -- ai changes ai_view dialog`: 185 passed, 0 failed. Whole crate (run-heavy): 889 passed, 0 failed, 7 ignored; 3 passed in the integration target.
Guard: `cargo fmt --all --check` clean (no output); `cargo clippy --workspace --all-targets -- -D warnings` Finished, no warnings.
No markers left (grep over crates/vr-ide/src empty). Nothing left running.

## Fix round 1 — 2026-11-27

### Progress
- started (new developer, from the notes): read notes, spec D8, ui-spec S3, threat model SEC-9..13, ai/changes.rs, ai/review.rs, ui/ai_changes.rs, the three test files, safe_replace.rs, OwnWrites, syntax.rs, editor_view.rs, dialog.rs, mockups ai-changes-A-80x24 / ai-revert-created-80x24.
- checkpoint (leader COST-16). Work is in the worktree only, nothing staged. Logs: qa-artifacts/T6/fix1-red-backend.log (first red run), fix1-red-mutation.log.

#### Done so far (backend, `cargo test -p vr-ide --lib ai::changes_tests`: 32 passed after the fixes)
Method: API stubs first (new enum variants, `keep(rel, seen)` ignoring `seen`, test seam), then the tests, red run, then the code.
| Item | Test (ai/changes_tests.rs) | Red | Change (ai/changes.rs) |
|---|---|---|---|
| a KNOWN_CAP silent | `sec13_a_walk_that_ran_out_of_memory_marks_the_list` | `left: None right: Some(WalkLimit)` | `scan_step`: `note(Incomplete::WalkLimit)` ("too many files") before `finish_scan` |
| b unwalked folders | `sec13_the_list_is_marked_until_the_walk_has_been_everywhere` | `left: None right: Some(Scanning)` | `Changes::banner()` returns `Incomplete::Scanning` while `scanning && running` and no other cause; not dismissable; `pause`/`resume` bump the version. UI text still to do |
| c own save rebase | `sec9_a_write_right_after_an_own_save_is_not_taken_for_the_save` | with the old rule (mutation `Expect::Own => true`): `no record of a.txt: []` (agent bytes adopted silently) | `Engine::rebase(shared, rel, Expect)`: `Expect::Own` asks `own.is_own` again after the read, `Expect::Shown(seen)` compares the read bytes with the diff's hash; not proven → record without earlier version, nothing stored |
| d Keep without expectation | `keep_keeps_only_the_version_that_was_shown` | `assertion failed: matches!(c.keep(rel, &d.seen), Err(KeepError::Changed))` | `Changes::keep(rel, &Seen) -> Result<(), KeepError>` (`still` check, then rebase with `Expect::Shown`); review.rs `keep_cmd` passes the diff tab's `seen` (a diff taken now from the list), refusal → diff reloads + `Not kept: X changed since shown; diff updated` |
| e stress | `sec12_revert_races_a_writer_{in_place,by_rename}_on_a_{file,link}` (4) | the two link tests were red on real defects: `No such file or directory (os error 2)` (state_of gave an error for a link replaced mid-read) and `kept at /proc/self/fd/19/.vr-ide-save-…tmp`; the file tests passed on the old code. Mutation proof (checks removed) still to run after the last tuning | `state_of` re-reads when the file vanishes mid-read; kept paths named by the folder's real path (`Anchor::real_now`); `swap_out` calls the `Step` seam |
| minor: folder swapped in at the path | `sec12_a_folder_at_the_path_is_never_moved_or_replaced` | `no record of a.txt: [a.txt/work.txt Created]` (a file replaced by a folder was not listed; Revert moved the folder to a temp name) | `touch`: folder where a file was → `file_gone`; `RevertError::Folder` (never exchanged, moved or trashed); diff note |
| minor: kept path | `sec12_a_version_kept_aside_is_named_by_its_real_path` | `kept at /proc/self/fd/10/a.txt.vr-ide-conflict-…` | see e |
| minor: trash TOCTOU | `sec12_a_created_file_written_to_while_it_is_trashed_is_reported` | `left: Trashed right: TrashedChanged` | `Pin`: the checked file is held open across the trash call; after it: still in place → error, written → `Reverted::TrashedChanged`, unlinked (replaced by rename, or a cross-device copy) → `Reverted::TrashedUnchecked`; both are warnings in review.rs. The system trash works by path, so the window is detected, not closed |
| minor: cap per process | `sec10_the_total_counts_the_stores_of_other_running_vr_ides` | `500 + 20 is over 510` | interpretation: the 512 MiB total was per vr-ide process; now each store writes its bytes to `<store>/used` (0600, lag ≤ 1 MiB) and `reserve` adds the other processes' figures (read at most once a second, links not followed) |
| minor: fixture counts | `sec10_the_shipped_caps_hold_for_a_9_mib_file_and_6000_new_files` | passed at once (a missing test, no defect): the SEC-10 verify numbers with the shipped caps instead of tiny fixture caps | test only (`Changes::feed_all`) |
| minor: index comment | — | — | module doc and `load_index` doc say that nothing reads the index back today |

#### In progress
- e: tuning the stress test (jitter on both sides, 60 rounds) — a run of 3 repetitions is in the background; then the mutation proof (`holds_expected` → `Ok(true)`, `swap_out`'s `holds` → true must fail the four tests) and restore.

#### Left
- UI for b: banner text for `Scanning` in ui/ai_changes.rs (`! List may be incomplete (still scanning the folder)`, no "enter hides"), app-level test + snapshot.
- d: app-level test in ai_changes_tests.rs (Keep from a diff tab after the file changed → refused, diff reloads).
- f: syntax colours in the diff (plan: `DiffLine.at` = byte offset in its source text, `Diff.old_text/new_text`, two highlighters in `DiffTab` created through new helpers in syntax.rs, spans mapped to buffer bytes in ui/editor_view.rs; `base.patch(syntax)` so the row background stays).
- minors UI: folder column decided for the whole list (all drawn rows have ≥ 10 cells for it, else none → 80x24 matches the mockup); dialog orphan word (confirm dialog pulls a word down when the last line would be one word); README (NFS ctime note, limits wording, Keep refusal, trash notices).
- then: related tests, snapshots once, guard, whole crate once (run-heavy), rule table, `## New states (round 1)`.

#### Files touched so far
crates/vr-ide/src/ai/changes.rs, ai/changes_tests.rs, ai/review.rs.
- e finished: rename writers stage their file before the wait, both sides jitter around the swap steps, 60 rounds. Mutation proof run and restored. UI items, f, README done; related tests, snapshots (once), guard done; whole crate started.

### Result per finding (reply: all `fixed`; two interpretations flagged)
Backend rows a–d and the minors are in the checkpoint table above (red output → change); additions since then:
- **b (UI)** `ui::tests::ai_changes_view_tests::ai_changes_scanning_notice` — red `left: "   (still scanning the folder a" right: "   (still scanning the folder) "` → ui/ai_changes.rs: `! List may be incomplete (still scanning the folder)`, no `enter hides`, warning colour; 80x24 and 120x40 rows asserted → green.
- **d (app)** `ai_changes_tests::keep_is_refused_when_the_file_changed_since_it_was_shown` (f5 on the diff tab after the file changed → nothing kept, `Not kept: session.rs changed since shown; diff updated`, diff reloaded; kept after it is shown again). Green with the backend fix (its red is the backend test's).
- **e** `sec12_revert_races_a_writer_{in_place,by_rename}_on_a_{file,link}`: writer and Revert run on two threads; the writer wakes on each `Step` of the swap, both sides pause 0–12 µs at random, 60 rounds each (a run: `InPlace on a file: 199 swaps, 46 refused, 95 kept aside, 1 writes no save can see`; `Rename on a file: 73 swaps, 11 refused, 6 kept aside`; `Rename on a link: 6 refused, 11 kept aside`; `InPlace on a link: 67 refused, 10 kept aside`). Checked per round: every answer is Restored / Changed / Gone / "kept at <existing path>" / "keeps changing"; the path holds the earlier version or one whole version; a version whose file a Revert unlinked is the version that Revert showed; the newest version is at the path, was shown, or is in a file an error named; no stray file in the folder; link targets outside never written, nothing created outside.
  Red (fix1-red-stress-mutation.log, checks of the swapped-out file removed): `round 4: a Revert unlinked r004-k000001, which it had not shown` (rename on a file), `round 12: a Revert unlinked …/outside/v5, which it had not shown` (rename on a link). Limit, stated: under that mutation the two in-place tests stayed green — a write through a handle opened before the swap cannot be told from safe_replace's documented blind spot by timing alone, so the in-place tests bound that case (it is counted and printed) and check everything else.
- **f** `ui::tests::ai_changes_view_tests::ai_diff_lines_have_syntax_colours` (themes dracula and homebrew) — red `left: Rgb(248, 248, 242) right: Rgb(255, 121, 198)` (keyword in the text colour) → `DiffLine.at` (byte offset of the line in its source text), `Diff.old_text / new_text`, `review::DiffSyntax` (one highlighter per side through new `Syntax::standalone` / `Syntax::spans_of`), `ui::ai_changes::diff_spans` maps the spans of the drawn rows into the diff buffer, editor_view paints them on the row's own background (`base.patch(syntax)`; unchanged for other tabs where `base` is `theme.text`) → green: keyword / type / comment colours on added, removed and context lines, hunk header unchanged, a "no earlier" file coloured, a deleted file has one highlighter, a closed tab none.
- **minor folder column at 80 columns** — red `left: " M session.rs …r-ide/src +9 -3 " right: " M session.rs            +9 -3 "` → the folder is a column of the list: shown when every drawn row that has a folder leaves it ≥ 10 cells, else in no row (80x24 now equals mockup ai-changes-A-80x24; 120x40 unchanged) → green.
- **minor dialog orphan word** — red: `ai_revert_dialogs_and_refusal` (`│ lines).` alone on a line) → ui/dialog.rs `wrap_text` for a confirm dialog's message and note: a last line of one word takes the word before it along → `…moves it to the trash` / `(88 lines).` → green. Same height. It also changed one snapshot outside T6: `render_confirm_dialog_40x12` (`…in the recovery` / `folder.` → `…in the` / `recovery folder.`).
- **minor README NFS ctime** — paragraph added (server clock, NTP); README also updated for Keep's refusal, the folder refusal, kept files, the trash notices, the scanning / too-many-files notices, the total over all running windows, diff colours.
- Before / after of the visible defects on the real render (BASE-37): qa-artifacts/T6/fix1-snapshot-diff.txt (TestBackend rows at 80x24; 120x40 rows asserted in the tests, unchanged).

Interpretations to confirm (the brief gave one line each):
1. "cap counted per process instead of per session": read as the earlier deviation "the 512 MiB total is counted per vr-ide process" → now over all running vr-ide processes (`used` files). If the reviewer meant another cap, say which.
2. "fixture counts in tests": read as the SEC-10 verify numbers (9 MiB, 6 000 files) being tested only with tiny fixture caps → a test with the shipped caps.

### New states (round 1)
| State | Quit / tab close | Esc / cancel | Error halfway | Input changes | Second request | Concurrent writer | Call that never returns | Test |
|---|---|---|---|---|---|---|---|---|
| `Incomplete::Scanning` notice (derived from `scanning && running`, no stored state) | gone with the tracker | Enter on it does nothing | walk stopped by a cap → `WalkLimit` replaces it | program exits → hidden (not running); restart → shown again | — | — | a hung file system stalls the walk: the notice stays, which is true | `sec13_the_list_is_marked_until…`, `ai_changes_scanning_notice` |
| Keep with an expectation (`KeepError::Changed`) | — | — | `still` fails to read → `Not kept: …` error, record stays | file changed → refused, diff reloads | record gone → nothing | write between check and baseline read → listed "no earlier", nothing stored | reads use `O_NONBLOCK` on regular files only | `keep_keeps_only_the_version_that_was_shown`, `keep_is_refused_when…` |
| Baseline re-read proof (`Expect::Own / Shown`) | worker stops | — | unread → no copy | agent write → record without earlier version | — | covered (seam before the look) | as above | `sec9_a_write_right_after_an_own_save…` |
| `<store>/used` file + 1 s cache of the other stores | removed with the store; a crashed process's store counts until the start-up sweep | — | unwritable → others see an older figure (≤ 1 MiB lag by design) | — | — | another same-uid program can write any number there (accepted in the threat model: bounded damage = no more copies) | plain small file, `O_NOFOLLOW|O_NONBLOCK`, 32 bytes | `sec10_the_total_counts_the_stores_of_other_running_vr_ides` |
| `Pin` across the trash call | handle closed on return | — | trash error → file stays, record stays | written → `TrashedChanged`; replaced / copied → `TrashedUnchecked` (both in the trash, never unlinked) | — | covered | the trash call itself is the existing one | `sec12_a_created_file_written_to_while_it_is_trashed_is_reported` |
| `RevertError::Folder` | — | — | — | folder removed → Revert works | — | folder swapped in after the check → swapped back, `Changed` | — | `sec12_a_folder_at_the_path_is_never_moved_or_replaced` |
| Diff highlighters (2 worker threads per open diff tab) | dropped with `DiffTab` (close, reload, session close, quit) → workers stop | — | over `max_highlight_bytes` → the highlighter disables itself, lines stay plain | diff reload builds new ones | one diff tab at a time | texts are private copies | parse is off the UI thread; queries have the editor's time budget (`cut` → one follow-up frame) | `ai_diff_lines_have_syntax_colours` |

### Files changed (round 1)
crates/vr-ide/src/ai/changes.rs, ai/changes_tests.rs, ai/review.rs, ai_changes_tests.rs, ui/ai_changes.rs, ui/tests/ai_changes_view_tests.rs, README.md; outside the row's list, each for one item: syntax.rs (`standalone`, `spans_of` — f), ui/editor_view.rs (diff spans, row background under syntax — f), ui/dialog.rs (`wrap_text` — dialog orphan). Snapshots regenerated once (4): ai_changes_80x24, ai_revert_created_80x24, ai_revert_refused_80x24, render_confirm_dialog_40x12.

### Out of scope, noticed
- `swap_out` (a link at the path) keeps a version it cannot put back under the temp name (`.vr-ide-save-…tmp`), not a `…vr-ide-conflict…` name; the message names it. Unchanged.
- A cross-device trash (copy + delete) always gives the `TrashedUnchecked` notice for a created file: the trash API does not say what it moved.

### Tests and commands (round 1, trimmed; logs in qa-artifacts/T6/fix1-*.log)
- Red runs: fix1-red-backend.log (`test result: FAILED. 23 passed; 9 failed`), fix1-red-mutation.log (own-save rule), fix1-red-stress-mutation.log, fix1-red-ui.log (`FAILED. 17 passed; 3 failed`), fix1-red-syntax.log.
- Related, after the last code change: `INSTA_UPDATE=always cargo test -p vr-ide --lib -- ai changes ai_view dialog syntax editor` → `test result: ok. 245 passed; 0 failed; 1 ignored` (the one snapshot update of the round; no `.snap.new` left).
- Stress alone, 4 runs in a row: 4 × `test result: ok. 4 passed` (≈ 2.4 s each).
- Whole crate once: `INSTA_UPDATE=no run-heavy.sh cargo test -p vr-ide` (fix1-crate.log) → lib `test result: ok. 905 passed; 0 failed; 7 ignored`, desktop_cli `ok. 3 passed`, `exit 0`. None of the known flaky tests failed.
- New tests: ai/changes_tests.rs +13 (19 → 32), ai_changes_tests.rs +1, ui/tests/ai_changes_view_tests.rs +2, plus new asserts in two existing screen tests.
- Guard (fix1-guard.log): `cargo fmt --all --check && run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings` → `Finished dev profile`, `guard exit 0`.
- Formatter: `rustfmt --edition 2024` on the 9 changed .rs files only.

### Rule compliance (round 1)
| Rule | Status | Evidence |
|---|---|---|
| AC-9 / D8 list, diff, keep, revert | pass | whole crate 905 ok; `keep_is_refused_when_the_file_changed_since_it_was_shown`, `ai_diff_lines_have_syntax_colours` |
| SEC-9 | pass | `sec9_a_write_right_after_an_own_save_is_not_taken_for_the_save`, `keep_keeps_only_the_version_that_was_shown` (no blob of unproven bytes) |
| SEC-10 | pass | `sec10_the_total_counts_the_stores_of_other_running_vr_ides` (`used` 0600, link not followed), `sec10_the_shipped_caps_hold_for_a_9_mib_file_and_6000_new_files`; index comment corrected (changes.rs module doc) |
| SEC-11 | pass | unchanged paths; `Pin::open` / `others_used` read through `open_regular` / `O_NOFOLLOW` only |
| SEC-12 | pass | 4 stress tests, `sec12_a_folder_at_the_path_is_never_moved_or_replaced`, `sec12_a_version_kept_aside_is_named_by_its_real_path`, `sec12_a_created_file_written_to_while_it_is_trashed_is_reported` (trash window detected, not closed: path-based trash API) |
| SEC-13 | pass | `sec13_a_walk_that_ran_out_of_memory_marks_the_list`, `sec13_the_list_is_marked_until_the_walk_has_been_everywhere`, `ai_changes_scanning_notice`; diff texts and highlighters in memory only |
| D13 | pass | README paragraph unchanged in meaning; `used` holds a number only |
| UI S3 (diff colours, list rows, dialog) at 80x24 and 120x40, both themes | pass | `ai_diff_lines_have_syntax_colours` (dracula, homebrew), row asserts at both sizes, fix1-snapshot-diff.txt |
| USER-2 | pass | no mockup or preview made; evidence is the terminal render of the real widgets (TestBackend rows) |
| USER-5 | pass | no key added or changed |
| BASE red first | pass | red output per item above; e and the own-save rule by mutation (stated), "fixture counts" test passed at once (stated) |
| BASE-11 | pass | no checkout / restore / stash / clean / reset; mutations undone by copying back the saved file |
| BASE-14 | pass | whole crate as one background task, no waiting loop |
| BASE-15 / COST-9 | pass | nothing stopped by name; nothing left running |
| BASE-16 | pass | reused `open_regular`, `still`, `safe_replace` steps, `create_private_file`, `Highlighter`, `dialog::wrap`; nothing new for others beyond `Syntax::standalone` / `spans_of` |
| BASE-18 | pass | no agents started |
| BASE-19 | pass | progress lines and checkpoint above |
| BASE-24 | pass | `git diff --cached --stat` empty |
| BASE-25 | pass | `guard exit 0` |
| BASE-37 | pass | fix1-snapshot-diff.txt (before / after rows of the real render) |
| BASE-38 | pass | rustfmt on changed files only |
| COST related tests / whole crate once | pass | commands above |
| COST-8 | pass with a note | the first edits by the Edit tool; after the context warning, batched edits through perl scripts that replace an exact string and stop unless it occurs exactly once (no regex over code) — no broken file, build checked after each batch |
| COST-13 | pass | logs in qa-artifacts/T6, scripts in the session scratchpad |
| COST-15 | pass | snapshots regenerated once, after the code was final |
| BASE-17, BASE-34 / DESIGN-23, query-count rules | n-a | nothing installed, no network, no data access layer |

DONE

## Review re-check r1 — 2026-11-27
Verdict: **CHANGES_REQUESTED** (1 major; trade-offs below go to the user).

### Original findings
| # | Status | Evidence |
|---|---|---|
| a KNOWN_CAP silent | fixed | changes.rs scan_step `note(Incomplete::WalkLimit)`; test `sec13_a_walk_that_ran_out_of_memory_marks_the_list` |
| b unwalked dirs | partly fixed | notice only while `scanning && running` (changes.rs `banner()`); see M1 |
| c own-save rebase | fixed | `rebase(.., Expect::Own)`: read stamp must equal the look, `is_own` asked after the read; unproven → record without earlier version |
| d Keep without expectation | fixed | `keep(rel, &Seen)` + `Expect::Shown` content proof; review.rs `keep_cmd` uses the diff tab's `seen`; app test |
| e racing-writer stress | fixed for rename writers and links (mutation red in fix1-red-stress-mutation.log); in-place writers: see T1 |
| f diff syntax colour | fixed | `diff_spans` + `DiffSyntax`; `base.patch` keeps `theme.text` for other tabs (editor_view.rs:355,440) |
| minors | fixed | index doc, `<store>/used`, Pin, folder refusal, shipped-caps test, folder column (80x24 now equals mockup ai-changes-A-80x24), dialog orphan, README NFS |

### New findings
- [major] crates/vr-ide/src/ai/changes.rs:2227 (`banner`) and :2256 (`pause`) — SEC-13: the "still scanning" notice disappears while what it warned about stays missing. Scenario: root `~/` (default), agent runs and exits before the walk reaches `~/proj/sub` → `pause()` sets `running=false`, `sync_epoch` drops the scan, `banner()` returns None; edits and deletions there are never listed and the list looks complete. Same after a finished walk for files deleted in folders the walk reached late. Fix: in `pause()` when `scanning`, `note(Incomplete::ScanUnfinished)` (persistent, "enter hides"); in `finish_scan`, if any event batch was handled while scanning, note a dismissable "files deleted during the scan may be missing". Test: pause mid-walk → banner set.
- [minor] changes.rs:2459 — `Moved::No` says "the trash reported success but the file is still in place"; if the parent was swapped for a link after `real_now()` the trash moved another file (the T-15 window). Message should say the trash may hold another file at that path.

### Trade-offs for the user (not accepted by the reviewer)
- T1 (data safety) in-place writer through a handle opened before the swap, writing after the swapped-out file passed its check: the write goes into the unlinked file and is lost. Measured: 1 in the developer's run, 0 in mine (226 swaps). Same window as every vr-ide save (safe_replace module doc). Cost-of-fix option: move the swapped-out version to the trash instead of `remove_file` (late writes then land in the trash copy; cost: one trash entry per Revert).
- T2 trash TOCTOU: a write/replace between Pin check and trash sends an unshown version to the trash; detected (`TrashedChanged` / `TrashedUnchecked` warnings), recoverable from the trash, not prevented (path-based trash API).
- T3 `<store>/used` is writable by same-uid programs (can starve pre-images); within the threat model's same-uid statement.
- Interpretations "cap per process" and "fixture counts": both readings are reasonable; accepted.
- `render_confirm_dialog_40x12` change: same height, orphan avoided; crate run with `INSTA_UPDATE=no` shows no other snapshot changed — accepted (SWEEP-4 pass).

### Rule check T6
| ID | Result | Evidence |
|---|---|---|
| AC-9 | pass | keep/revert/diff paths + app tests; crate 905 ok (fix1-crate.log) |
| SEC-9 | pass | `rebase` proof (changes.rs ~1236); `sec9_a_write_right_after_an_own_save…` |
| SEC-10 | pass | `used` via `create_private_file`, read via `read_regular` (NOFOLLOW); shipped-caps test |
| SEC-11 | pass | Pin uses `open_regular` / `O_PATH|O_NOFOLLOW`; no new follow |
| SEC-12 | pass with T1/T2 flagged | 4 stress tests (re-ran in-place file: ok, 0 lost); folder refusal; mutation red log |
| SEC-13 | fail | M1 |
| BASE red-first | pass | fix1-red-*.log; fixture-count test green at once (stated) |
| SWEEP-4 | pass | only `render_confirm_dialog_40x12` changed; other tabs keep `theme.text` base |

## Fix round 2 — 2026-11-27

### Progress
- started (fresh developer, worktree .worktrees/T6r2, branch run4-T6-r2): read the round-1 notes, the re-check, D18 / D19, ai/changes.rs (banner, pause, walk, revert, swap_out), vr-core safe_replace.rs, review.rs revert notices, ui/ai_changes.rs banner, the backend and view tests.
- design: (M1) `pause` / `resume` during a walk leave a lasting `Incomplete::Unwalked`; the walk notes `Incomplete::WalkedLate` when a folder's own mtime shows that entries changed in it between the session's start and its watch (evidence, not a guess from unrelated events). (D18) the swapped-out file is renamed to `<name>.vr-ide-reverted-PID-N` at safe_replace's `BeforeDeleteOld` step and then trashed; a trash that fails leaves it there and the notice names it. (minor) `Moved::No` text.
- red: 11 new / changed tests fail for the stated reasons (qa-artifacts/T6/fix2-red.log); the two link stress tests stay green (links are still unlinked: nothing can be written into a link).
- green: 12 new tests + adapted ones; related set 350 passed; stress 8 runs ok; guard run. Next: whole crate once.
- whole crate once: 914 passed, exit 0 (qa-artifacts/T6/fix2-crate.log). Correction of the line above: 9 new tests (6 backend, 2 app, 1 screen), not 12.

### Result per finding
| Item | Reply | Test | Red (fix2-red.log) | Change |
|---|---|---|---|---|
| [major] SEC-13 exit before the walk was everywhere | fixed | `ai::changes_tests::sec13_a_walk_the_program_did_not_wait_for_leaves_the_list_marked` | `left: None right: Some(Unwalked)` | ai/changes.rs `pause()`: while `scanning`, `note(Incomplete::Unwalked)` (lasting, Enter hides; survives the next run's walk). `resume()` on a run whose walk still runs notes the same |
| [major] SEC-13 deleted during the scan in a folder walked late | fixed, by evidence instead of the suggested "events were handled during the scan" | `…::sec13_a_folder_changed_before_its_watch_marks_the_list` | `left: None right: Some(WalkedLate)` | `scan_step`: right after a folder's watch is placed, `changed_unwatched` looks at the folder's own mtime / ctime; not older than the session's start (same `predates` rule and margin as for files) → `note(Incomplete::WalkedLate)`. Reason for the other design: events in already-watched folders say nothing about unwatched ones, and no events does not mean no deletions there; a deletion always changes its folder's mtime. Folders already watched through an event (made by the agent) are not checked |
| SEC-13 texts | fixed | `ui::tests::ai_changes_view_tests::ai_changes_scan_gap_notices` | `left: [" ! List may be incomplete      ", "   (scan stopped early at 11:48"]` (cut at 80 columns) | ui/ai_changes.rs: `! Scan stopped early: changes may be missing · enter hides`, `! Deletions during the scan may be missing · enter hides` — both whole in the banner's two lines at 80x24 (29 + 28 cells; the second line ends at the panel's last cell) and 120x40, rows asserted at both sizes; Enter hides |
| [minor] `Moved::No` text | fixed | `sec12_a_created_file_written_to_while_it_is_trashed_is_reported` (assert added) | assertion failed at the `Failed(e) if e.contains("the trash may hold a different file")` | `…still in place; the trash may hold a different file from that path` |
| D18 replaced version to the trash | done | `sec12_the_replaced_version_goes_to_the_trash_with_a_late_write` (handle opened before the swap, written at `BeforeDeleteOld` → the bytes are in the trash copy, nlink 1, one entry named `a.txt.vr-ide-reverted-PID-N`, no stray file); stress `sec12_revert_races_a_writer_{in_place,by_rename}_on_a_file` now assert that no version is ever unlinked and every trash entry is the shown version, a late write (in place only) or the refused Revert's own file | `left: 0 right: 1` (trash empty); stress: `round 0: a Revert unlinked a version; it belongs in the trash`, `round 0: one trash entry per Revert left: 0 right: 1` | `revert`: a wrapper around the seam renames the file safe_replace is about to delete (no-replace rename, at `BeforeDeleteOld`) to `<name>.vr-ide-reverted-PID-N`; safe_replace's delete then finds nothing; after `Replaced` the file goes to the trash by its real path. vr-core is not changed. Such names are skipped by the tracker (`State::skipped`) |
| D18, trash that fails | new state | `sec12_without_a_trash_the_replaced_version_is_kept_by_name` (a trash that errs, and one that answers "done" and moves nothing) | panicked at the `let Reverted::RestoredOldKept(at) = got else` | `Reverted::RestoredOldKept(path)` (the enum is no longer `Copy`): the file is restored, the record goes, the replaced version stays under its name; review.rs warns `Reverted X; the replaced version could not be moved to the trash, kept at <path>` |
| D18, the Revert's own file (found by the stress test, 1 run in ~10) | extended, flagged below | `sec12_a_refused_reverts_own_file_goes_to_the_trash_with_a_late_write` | `never unlinked left: 0 right: 1` | The other file the protocol deletes: our own, swapped back out because the path had changed (`BeforeDeleteOurs`). A program that opened the path while it was there loses a late write the same way. It takes the same road to the trash; when the trash does not take it, it is deleted as before (nothing of vr-ide's stays in the folder) |
| QC obs 1: own save, then the agent writes at once → Revert wrote back the pre-save file | defect, fixed | `ai::changes_tests::sec9_the_version_before_an_own_save_is_not_what_revert_writes_back`, app: `ai_changes_tests::an_own_save_the_agent_wrote_over_is_not_undone_by_revert` (ctrl+s through the real command) | `left: (Modified, None) right: (Modified, Some(NotSeen))`; app: `the copy from before the user's save is not offered` | D8 line relied on: "files … changed by anything other than vr-ide's own editor are recorded with their content before the first change" — the content before the agent's first change is the user's save, which vr-ide has no copy of once the agent wrote over it; SEC-9: true pre-image or none. `Changes::own_save(path, begins)` is called by commands/file.rs right before each editor save (`save_doc`, save as, overwrite) and again when the save replaced nothing (`ChangedOnDisk`); `State::saving` marks the file; `take_base` drops the older copy when the file turns out not to be that save → listed `no earlier`, Keep only. An own save nobody raced still becomes the new baseline (round-1 path, unchanged). `OwnWrites` could not answer this: the editor's watcher drops its entry at the first mismatch, on another thread |
| QC obs 2: newest refusal behind older ones | fixed for refusals of Keep / Revert | `ai_changes_tests::a_newer_refusal_replaces_the_older_ones` | `left: ["something else", "Not kept: …", "Not kept: …", "Not reverted: …"] right: ["something else", "Not reverted: …"]` | review.rs `refused()`: a new `Not kept:` / `Not reverted:` / `No earlier version of` notice removes the earlier ones of these kinds (10 call sites). The app's queue rule (oldest first, `+N more`) is untouched: an unrelated older warning still shows first — that is the notice row's rule for every feature, not changed here |

### New states (round 2)
| State | Quit / tab close | Esc / Enter | Error halfway | Input changes | Second request | Concurrent writer | Call that never returns | Test |
|---|---|---|---|---|---|---|---|---|
| `Incomplete::Unwalked` (stored banner) | gone with the tracker | Enter hides | — | restart: stays (the new walk cannot find the old run's changes) | a second exit mid-walk after Enter: noted again | — | — | `sec13_a_walk_the_program_did_not_wait_for…`, `ai_changes_scan_gap_notices` |
| `Incomplete::WalkedLate` (stored banner) | gone with the tracker | Enter hides; a later late folder of the same walk shows it again | folder vanished before the look: no note | a file created (not deleted) there also changes the folder's time: the note says "may" | — | — | one `lstat` per folder on the worker thread, as the walk's `read_dir` | `sec13_a_folder_changed_before_its_watch…` |
| `<name>.vr-ide-reverted-PID-N` between the rename and the trash | a crash in between leaves the file under that visible name with the replaced version (never deleted) | — | rename refused → safe_replace deletes as before; trash fails → `RestoredOldKept` names the path (own file: deleted) | — | names carry a per-process counter; no-replace rename | a late write lands in it wherever it is (the point of D18) | the trash call is the existing one | the three D18 tests, stress |
| `State::saving` (set of paths) | cleared with the walk's memory on pause / resume | — | save failed otherwise than `ChangedOnDisk`: the mark stays → a later agent change is listed `no earlier` (safe side) | the save's event shows the file to be the save → mark gone, new baseline | — | agent writes before the event → `no earlier`; agent deletes → `D`, `no earlier` | — | `sec9_the_version_before_an_own_save…` |

### Files changed (round 2)
crates/vr-ide/src/ai/changes.rs, ai/changes_tests.rs, ai/review.rs, ai_changes_tests.rs, ui/ai_changes.rs, ui/tests/ai_changes_view_tests.rs, README.md; outside the row's list for QC obs 1: commands/file.rs (three `own_save` calls). No snapshot changed (crate run with `INSTA_UPDATE=no`, no `.snap.new`). Older tests adapted to D18: trash counts in `sec12_revert_is_refused_when_the_file_changed_since_the_diff`, `sec12_a_folder_at_the_path_is_never_moved_or_replaced`, `revert_asks_first_and_writes_the_earlier_version_back`, `a_real_agents_changes_are_listed_kept_and_reverted`; `ai_changes_scanning_notice` waits the test margin before `resume` (the fixture's folders were changed in the same instant).

### For the leader / user
1. D18 was extended to the refused Revert's own file (row above): a Revert refused after its swap also leaves one trash entry (the earlier version's bytes). Say so if only the replaced version should go to the trash; the stress test then needs its "write no save can see" allowance back.
2. Without a trash for the folder (some mounts) every Revert leaves `<name>.vr-ide-reverted-…` next to the file, with a warning. The alternative is deleting it there (the round-1 behaviour).
3. The Revert dialog still says "Today's text, +9 -3, is lost." (ui-spec S3 text, not changed): with D18 that text is in the trash. Wording is the spec's to change.
4. Not covered by D18: a link at the path (`swap_out` still unlinks the link — nothing can be written into a link), and file systems without EXCHANGE, where safe_replace falls back to a plain rename (its documented window).
5. `WalkedLate` will show often in a home-folder session (the agent rewrites files in its own config folder before the walk gets there). It is true there; Enter hides it.
6. The banner's second line may end at the panel's last cell at 80 columns (28 of 28 cells). Older banner texts with a time (`watcher rescan at 14:32 · enter hides`) were already cut at 80x24 before this round; not touched.
7. `ai_account_tests::remove_account_asks_refuses_while_in_use_and_deletes_only_it` failed once in a related run and passed alone and in the crate run: not mine (no file of it touched).

### Tests and commands (round 2, trimmed)
- Red: qa-artifacts/T6/fix2-red.log — `test result: FAILED. 3 passed; 11 failed` (first batch), then `sec12_a_refused_reverts_own_file…` `FAILED. 0 passed; 1 failed`.
- Related, after the last code change: `INSTA_UPDATE=no cargo test -p vr-ide --lib -- ai changes ai_view dialog file save notice` → `test result: ok. 350 passed; 0 failed; 4 ignored`.
- Stress, 8 runs in a row after the last change: 8 × `test result: ok. 4 passed` (e.g. `InPlace on a file: 233 swaps, 55 refused, 118 kept aside, 1 late writes kept in the trash`).
- Whole crate once: `INSTA_UPDATE=no run-heavy.sh cargo test -p vr-ide` → lib `test result: ok. 914 passed; 0 failed; 7 ignored`, desktop_cli `ok. 3 passed`, `exit 0` (fix2-crate.log).
- Guard (fix2-guard.log): `cargo fmt --all --check && run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings` → `Finished dev profile`, `guard exit 0`.
- Formatter: `rustfmt --edition 2024` on the changed .rs files only.

### Rule compliance (round 2, rows that changed)
| Rule | Status | Evidence |
|---|---|---|
| AC-9 / D8 | pass | crate 914 ok; `an_own_save_the_agent_wrote_over_is_not_undone_by_revert` |
| SEC-9 | pass | `sec9_the_version_before_an_own_save_is_not_what_revert_writes_back` (copy older than the user's save dropped) |
| SEC-12 / D18 | pass | three D18 tests; file stress: no version unlinked, every trash entry accounted for; `Moved::No` text asserted. Limits in "For the leader" 4 |
| D19 | pass | trash race unchanged: `TrashedChanged` / `TrashedUnchecked` tests green |
| SEC-13 | pass | `sec13_a_walk_the_program_did_not_wait_for_leaves_the_list_marked`, `sec13_a_folder_changed_before_its_watch_marks_the_list`, `ai_changes_scan_gap_notices` (80x24, 120x40) |
| USER-2 / USER-5 | pass | no mockup; no key added or changed (Enter on the banner as before) |
| BASE red first | pass | fix2-red.log, one red per item |
| BASE-11 / BASE-24 | pass | no checkout / restore / stash / reset; `git diff --cached --stat` empty; nothing committed |
| BASE-14 / BASE-15 / COST-9 | pass | crate run as one background task, no waiting loop; nothing left running |
| BASE-25 | pass | `guard exit 0` |
| BASE-38 / COST-15 | pass | rustfmt on changed files; no snapshot regenerated (none changed) |
| COST related / crate once | pass | commands above |
| COST-8 | pass with a note | Edit tool for enum, README; other edits through a script replacing an exact string that must occur exactly once, and three counted perl substitutions; no python |
| COST-13 | pass | logs in qa-artifacts/T6; helper script in the session scratchpad |
- addendum (leader, D20: decisions 1 and 2 stand): the Revert dialog for a modified file now ends "Today's text, +N -M, goes to the trash." (ai/review.rs; red `left: "…is lost." right: "…goes to the trash."` in `revert_asks_first_and_writes_the_earlier_version_back`). One golden regenerated once: `ai_revert_confirm_120x40` (that line only, same dialog size); 80x24 asserted in `ai_revert_dialogs_and_refusal` (text, note and buttons whole). Related `-- ai changes ai_view dialog` green with `INSTA_UPDATE=no`; guard `guard exit 0` (qa-artifacts/T6/fix2-guard-text.log).

DONE

## Review re-check r2 — 2026-11-27
Verdict: **CHANGES_REQUESTED** (3 major, 3 minor; trade-offs flagged for the user).

### Earlier items
| Item | Status | Evidence |
|---|---|---|
| [major] SEC-13 exit mid-walk | fixed | `pause()` / `resume()` note `Unwalked` while `scanning` (changes.rs ~2374, ~2390); red→green `sec13_a_walk_the_program_did_not_wait_for…` |
| [major] SEC-13 deletions in folders walked late | fixed for folders that still exist; gap left, see N1 | per-folder mtime/ctime check `changed_unwatched` (changes.rs ~1127). Judgement: the developer's rule is better than the one I suggested in r1 — it is evidence per folder (a deletion always sets its folder's mtime/ctime, ctime cannot be backdated), while "events were handled during the scan" says nothing about folders that are not watched yet. It only fails when the folder itself is gone before the walk reaches it (N1) |
| [minor] `Moved::No` text | fixed | changes.rs ~2584; asserted in `sec12_a_created_file_written_to_while_it_is_trashed_is_reported` |
| D18 / D20 aside + trash | works on EXCHANGE filesystems; not on others (N3); no retry on a name collision (n2) | seam wrapper at `BeforeDeleteOld` / `BeforeDeleteOurs` (changes.rs ~2654); `Replaced` → trash by real path, `RestoredOldKept` when it stays; refused → trash, otherwise delete (D20). safe_replace's `remove_file(tmp)` then finds nothing (vr-core safe_replace.rs:142,180, result ignored) — fine. A link at the path: the link is unlinked, the link's target is untouched, nothing lost — accepted |
| D21 own_save hook | race fixed for a single save; re-opened by a second save, see N2 | `take_base` drops the old copy when `saving` is marked; app test `an_own_save_the_agent_wrote_over_is_not_undone_by_revert` |
| Newest refusal replaces older ones | fixed | review.rs `refused()`; `a_newer_refusal_replaces_the_older_ones` |
| SWEEP-4 own_save on every save | pass | 3 save paths covered (`save_doc`, `save_as_doc`, `keep_mine`; `app.rs:74` and `app_workspace.rs:302` go through `save_doc`). No AI session → empty loop; path outside a root → `strip_prefix` returns early; save errors are reported as before. Cost: one `canonicalize` of the parent folder + two short locks per session per save |

### New findings
- **[major] N1 — crates/vr-ide/src/ai/changes.rs:1124 (`scan_step`) with :1078 (`watch`) and :1522 (`gone`) — SEC-13: deletions in a folder removed before the walk reached it are not listed and nothing says so.** Scenario: root `~/`, the walk has watched `~/proj` and queued `~/proj/sub` (already-existing files `sub/a.rs`, …). The agent runs `rm -rf ~/proj/sub` (or `mv sub /tmp/x`) before the walk gets there. The `Removed(sub)` event → `gone()` → `file_gone(sub)`: no record, nothing in `known` → nothing. The walk pops `sub`: `watch` fails, `abs.is_dir()` is false → no note → `continue`; `changed_unwatched` is never reached. When the walk finishes, "Scanning" goes away: the deleted files are not listed and the list looks complete. Fix: in `scan_step`, when `watch(&rel)` fails and `symlink_metadata(root.join(&rel))` is not a folder, `note(Incomplete::WalkedLate)` (the folder was queued, so it existed at the start of the walk); test: queue a folder, delete it before the step → banner `WalkedLate`.
- **[major] N2 — crates/vr-ide/src/ai/changes.rs:2298 (`own_save(.., false)`) with commands/file.rs:77/468/641 — SEC-9 / D21: a second save brings back the copy from before the user's save.** Scenario: the user saves A (mark set, S1 written); the agent / format-on-save rewrites A right after (W); within the ~200–300 ms before the tracker handles the events (`DEBOUNCE` 200 ms + `recv_timeout` 100 ms), the user presses ctrl+s again → `doc.save()` → `ChangedOnDisk` → `own_save(false)` removes the mark that the *first* save set. The tracker then handles W: not own, `take_base` finds no mark → record `Modified` with the copy from before S1 → Revert writes back the pre-S1 text, i.e. it undoes the user's save. Fix: `own_save(true)` returns whether it inserted the mark (or counts per path); the `ChangedOnDisk` path removes it only when this save inserted it. Test: own_save(true) → write S1 → agent write → own_save(true) + own_save(false) → event → `(Modified, Some(NotSeen))`.
- **[major] N3 — crates/vr-ide/src/ai/changes.rs:2654 (seam wrapper) with vr-core/src/safe_replace.rs:117–125 and ai/review.rs:613 (dialog text) — D18/D20: without RENAME_EXCHANGE (NFS, sshfs/FUSE — the README names NFS) the replaced version is unlinked, yet the dialog says "Today's text, +N -M, goes to the trash."** In the fallback, safe_replace does a plain `rename(tmp, target)`. Neither `BeforeDeleteOld` nor the wrapper runs, `aside` stays false, `Reverted::Restored` → "Reverted X". Today's text is gone, while the user confirmed a dialog that promised a trash copy. In round 1 the text said "is lost", which was true. Fix (cost: small): wrap `swap` too. When `exchange_files` returns `InvalidInput`/`Unsupported`, first `hard_link(target, aside)` (same folder handle), set `aside`, then return the error so the fallback renames. After `Replaced` the aside goes to the trash as now; after `Changed` remove the link. If the link fails, return `Reverted::Restored` with a different notice ("the replaced version could not be kept"), or refuse. Alternative (cost: copy only): the dialog says "goes to the trash" only when the folder's filesystem supports EXCHANGE (probe at dialog time).
- [minor] n1 — changes.rs:683 (`skipped`) — any name that *contains* `.vr-ide-reverted-` is never recorded, including folders: the walk skips them and nothing marks it. Scenario: the agent writes `notes.vr-ide-reverted-x` or works under such a folder → not listed, no notice. A leftover `RestoredOldKept` file is also never tracked again. Fix: match only the exact form `<name>.vr-ide-reverted-<pid>-<n>`, and only files; better, a `State` set of the names this process put aside, removed after the trash.
- [minor] n2 — changes.rs:2657 — `rename_noreplace` to `aside_name` is tried once. A stale `<name>.vr-ide-reverted-PID-N` (same PID after a reboot, a crash leftover) → `AlreadyExists` → the version is deleted, with no notice (D18 silently not applied). Fix: retry with the next counter, like `keep_conflict` (100 tries). Also reuse: `rename_noreplace` and the name-cutting code duplicate vr-core `safe_replace.rs:290/322` (BASE-16, minor). Make them `pub` there.
- [minor] n3 — changes.rs:689 (`note`) — one banner slot: a `WalkedLate` noted later replaces a not-yet-dismissed `Unwalked` ("changes may be missing" → only "Deletions … may be missing"), which understates the gap. Fix: do not let `WalkedLate` replace `Unwalked`.

### Design QA (banner, dialog line)
- **[major] D1 — crates/vr-ide/src/ui/ai_changes.rs:262–275 — the timed banners lose "· enter hides" at 80x24 (COMPACT clip; the developer flagged it as old, so it is filed here).** Panel is 31 cells; the text wraps at 29 into 2 lines: `! List may be incomplete` / `(watcher rescan at 14:32) ·` and the third line `enter hides` is cut. The same happens for every `Watcher`, `Rescan`, `WatchLimit`, `RecordLimit` and `WalkLimit` text: the user is not told how to hide it. Also, continuation lines start at x+3 but wrap at width-2, so a 29-cell continuation line runs 1 cell past the panel's room. Fix options (cost): (a) shorter text, e.g. `! Incomplete: watcher rescan at 14:32 · enter hides` → `! Incomplete: watcher rescan` / `at 14:32 · enter hides` (copy only, no layout change; needs a ui-spec text change); (b) `BANNER_LINES = 3` (one more row taken from the list at every size); (c) drop the time at widths < 40 (loses "when"). In every case, wrap continuation lines at `width-3`.
- [minor] D2 — the new `Unwalked` / `WalkedLate` second line ends on the panel's last cell (`may be missing · enter hides`, 28/28), while the file rows keep a 1-cell right margin (`+9 -3 `). Fix: shorten by ≥1 word ("Scan stopped early: changes may be missing · enter hides" → "Scan stopped early; changes may be missing · enter hides" does not help). For example `! Scan stopped early: some changes not listed · enter hides`, or fix the width with option (a) above.
- Dialog "goes to the trash." — whole at 80x24 and 120x40 (asserted; golden diff is that line only). It passes on EXCHANGE filesystems; see N3 for the others.

### Trade-offs for the user (flagged, not accepted)
- T4 (function vs data safety): a save that fails with an error other than `ChangedOnDisk` (disk full, EACCES) leaves the mark: the next agent change to that file is listed `no earlier` (Keep only), and the true earlier copy is deleted (`take_base` drops the blob). This is safe, but Revert is lost for no reason. Alternative: clear the mark on errors that `vr-core` guarantees happened before the swap. Cost: an error classification in `DocError`.
- T5: `WalkedLate` shows in almost every home-root session (the agent's own config folders change at start). It is true there, and Enter hides it. Noise vs a quieter rule (e.g. skip folders created after t0) — the user decides.
- Q1 (spec question, not a finding): a user save over a file that *already* has a record keeps the pre-agent copy (`changed()` returns at the record branch before the own-save check, changes.rs ~1440). Revert then writes back the text from before the agent and the user's save together. The diff shows it and the dialog asks; the brief's wording "Revert never undoes the user's own save" is wider than D21. Leader/user: confirm D8 "content before the first change" is meant here.

### Files read
crates/vr-ide/src/ai/changes.rs (read, diff + walk/event/rebase/revert context), ai/review.rs (read), commands/file.rs (read), ui/ai_changes.rs (read), ui/tests/ai_changes_view_tests.rs (read), README.md (read), ai/changes_tests.rs and ai_changes_tests.rs (names, red log, the own-save test — not line by line), snapshot `ai_revert_confirm_120x40.snap` (read, one line). Context: vr-core safe_replace.rs, document.rs (`save`, `OwnWrites`), watch.rs (`DEBOUNCE`), ui/status_bar.rs.

## Rule check T6
| ID | Result | Evidence |
|---|---|---|
| AC-9 | pass | list/diff/keep/revert paths; crate 914 ok (fix2-crate.log); related after text change green |
| SEC-9 | fail | N2 (second save clears the first save's mark → pre-save copy offered); single-save case pass (`sec9_the_version_before_an_own_save…`) |
| SEC-12 / D18 / D20 | fail | N3 (fallback filesystems unlink today's text against the dialog); EXCHANGE path pass (three D18 tests, stress logs) |
| SEC-13 | fail | N1 (folder gone before the walk reached it); Unwalked / WalkedLate pass |
| BASE red-first | pass | fix2-red.log: 11 + 1 red with the stated reasons |
| SWEEP-4 | pass | every editor save path calls `own_save`; no AI session = no work; error paths unchanged |

## Fix round 3 — 2026-11-27

### Progress
- started (same developer as round 2; worktree .worktrees/T6r3, branch run4-T6-r3 from b750f01): read re-check r2, D20–D24.
- design: N1 a queued folder that is gone when the walk gets there (watch fails and it is no folder, or `read_dir` says not found) → `note(WalkedLate)`. N2 `State::last_save` remembers whether the latest `own_save(begin)` set the mark; `own_save(false)` clears only then. N3 the swap wrapper, on "no EXCHANGE", hard-links the target to the aside name (else copies it, else refuses the Revert) before safe_replace's fallback rename; aside → trash as on the EXCHANGE path. D22 banners `! Incomplete: <why>[ at HH:MM] · enter hides`, continuation lines one cell short of the panel's edge. n1 a set of the names this process put aside replaces the name filter. n2 retry with the next counter; vr-core's `rename_noreplace` / name cut made `pub` and reused. n3 `WalkedLate` does not replace `Unwalked`.
- red: 6 backend tests fail for the stated reasons (qa-artifacts/T6/fix3-red.log).
- green: backend 44 passed; banner copy done (view tests green), ui-spec and README updated; golden ai_changes_limits_120x40 regenerated once. Next: related set, vr-core tests, stress repeats, clippy, crate.
- whole crates once: vr-ide 921 passed, vr-core 156 passed, exit 0 (qa-artifacts/T6/fix3-crate.log); guard exit 0.

### Result per finding (red in qa-artifacts/T6/fix3-red.log)
| Item | Reply | Test | Red | Change |
|---|---|---|---|---|
| N1 [major SEC-13] queued folder gone before the walk | fixed | `ai::changes_tests::sec13_a_folder_gone_before_the_walk_reached_it_marks_the_list` (removed, and moved out of the root; the walk is stopped after the root with the new test helper `scan_some`) | `moved: false left: None right: Some(WalkedLate)` | ai/changes.rs `scan_step`: when the watch fails and the path is no folder any more, or `read_dir` answers not found / not a folder → `note(WalkedLate)`. Both places, since without a watcher (and in a race) the loss shows only at `read_dir` |
| N2 [major SEC-9 / D21] second save clears the first save's mark | fixed | `…::sec9_a_second_save_that_wrote_nothing_keeps_the_first_saves_mark` (real `Document::save` twice, the second answers `ChangedOnDisk`) | `left: (Modified, None) right: (Modified, Some(NotSeen))` | `State::last_save = (path, whether this begin set the mark)`; `own_save(false)` removes the mark only when the latest begin for that path set it. commands/file.rs unchanged (saves are synchronous: begin and its end are consecutive calls) |
| N3 [major D18 / D20] no RENAME_EXCHANGE | fixed by keeping the promise | `…::sec12_without_exchange_the_replaced_version_still_goes_to_the_trash`: (1) hard link — late write through an old handle is in the trash copy, nlink 1; (2) no hard links — a copy of the shown version in the trash; (3) file changed before the rename — `Changed`, nothing trashed, no stray; (4) neither link nor copy — refused `…cannot be kept on this file system`, file and record stay | `left: 0 right: 1` (trash empty after "Reverted") | `revert`: the swap passed to safe_replace is wrapped; on `InvalidInput` / `Unsupported` from the exchange it first gives the file at the path a second name `<name>.vr-ide-reverted-PID-N` (`twin`: hard link, else an exclusive-create copy with the file's mode; the result must hold the shown version, else it is removed and the Revert answers `Changed`), then lets safe_replace do its fallback rename. `Replaced` → trash as on the EXCHANGE path (`RestoredOldKept` when the trash leaves it); `Changed` or an error → the second name is removed. No twin possible → the swap fails before anything is at the path: `Not reverted: X: the replaced version cannot be kept on this file system (…)`. The dialog text is true on every file system. Test seam: `seam::file_system_lacks(Lacks { exchange, links, copies })` (thread-local, tests only) |
| D1 [major, D22] timed banners at 80x24 | fixed | `ui::tests::ai_changes_view_tests::ai_changes_banners_keep_the_hint_and_the_margin` (7 causes × 80x24 and 120x40: two lines, whole text with `enter hides`, last cell free on both lines, file row right under) + explicit rows for the D22 example; `ai_changes_scan_gap_notices`, `ai_changes_without_an_earlier_version` updated | `Watcher at 80x24: [" ! List may be incomplete      ", "   (watcher error at 14:32) ·  ", …]` (hint cut) | ui/ai_changes.rs: `! Incomplete: <cause> at HH:MM · enter hides`; `banner_lines`: first line `width-2`, later lines `width-4` (they start two cells further in and keep the last cell free); `at HH:MM` and `enter hides` are joined by a no-break space while wrapping. 80x24: `! Incomplete: watcher rescan` / `at 14:32 · enter hides` |
| D2 [minor] new notices flush at the edge | fixed | same test | — | `! Incomplete: scan stopped early · enter hides`, `! Incomplete: files deleted during scan · enter hides` (the round-2 wording did not fit with a margin; "deletions during the scan" was one cell too long for line 1 at 80 columns) |
| n1 name filter too wide | fixed | `…::a_name_like_a_put_aside_version_made_by_the_agent_is_listed` | `left: ["a.txt.vr-ide-reverted-1-2/in.txt"] right: […, "notes.vr-ide-reverted-x"]` | `State::aside`: the root-relative names this tracker's Reverts put aside, entered before the file exists, removed once the trash took it (or it was deleted); `skipped` asks the set, not the name. A version the trash did not take stays in the set for the session (it is vr-ide's file, named in the notice) |
| n2 aside-name collision, duplicated helpers | fixed | `…::sec12_a_taken_aside_name_is_passed_over` (40 stale files on the next counters stay untouched, the version is in the trash) | `the replaced version left: 0 right: 1` | `put_aside` retries the next counter on `AlreadyExists` (100 tries, like `keep_conflict`); vr-core safe_replace.rs: `rename_noreplace` is `pub`, the name cut is `pub fn name_stem` (used by `keep_conflict` too); the copies in changes.rs are gone |
| n3 one notice slot | fixed | `…::sec13_a_later_narrower_notice_does_not_replace_the_wider_one` | `left: Some(WalkedLate) right: Some(Unwalked)` | `State::note`: `WalkedLate` does not replace an undismissed `Unwalked` |

D23, D24: no change, as decided.

### New states (round 3)
| State | Quit / tab close | Esc / Enter | Error halfway | Input changes | Second request | Concurrent writer | Call that never returns | Test |
|---|---|---|---|---|---|---|---|---|
| Twin (second name or copy) before the fallback rename | a crash between the link and the rename leaves `<name>.vr-ide-reverted-PID-N` next to the untouched file (a second name of it, or a copy): nothing lost, one leftover | — | link refused → copy; copy fails → removed, Revert refused; rename fails → twin removed, error as before | file changed after the diff: the twin does not hold the shown version → removed, `Changed`; changed between twin and rename → safe_replace's re-check says `Changed`, twin removed | counter per process, never over an existing file | a write between the re-check and the rename is replaced (safe_replace's documented window on such file systems); with a hard link that write is still in the trash copy when it went into the same file | the copy reads a regular file opened `O_NONBLOCK` (`open_regular`); a hung network mount hangs the Revert like any save there | `sec12_without_exchange_…` |
| `State::aside` (names) | gone with the tracker | — | entered before the rename, removed when it fails | — | — | an agent file of the same shape under another name is listed (n1 test) | — | n1 test, `sec12_without_a_trash_the_replaced_version_is_kept_by_name` |
| `State::last_save` | gone with the tracker | — | a save that fails otherwise than `ChangedOnDisk` never calls the end: the mark stays (D23) | — | a second begin overwrites it: only the latest save can take back its own mark | — | — | N2 test |

### Files changed (round 3)
crates/vr-ide/src/ai/changes.rs, ai/changes_tests.rs, ui/ai_changes.rs, ui/tests/ai_changes_view_tests.rs, README.md; crates/vr-core/src/safe_replace.rs (two helpers made `pub`, as the reviewer asked: BASE-16); team file `.ai-dev-team/workspace/02_ui-spec.md` (banner copy, S3 "Incomplete list"). One golden regenerated once: `ai_changes_limits_120x40` (the two banner lines only).

### For the leader / user
1. On a file system with neither EXCHANGE nor hard links nor room for a copy, Revert of a modified file is refused (it worked in round 1, deleting the text). That follows "no silent loss"; the alternative is a dialog that says "is lost" there.
2. The copy fallback (no hard links, e.g. vfat) keeps the shown version, not a late write through an old handle: there is no file to share with the writer there.
3. A link at the path on a file system without EXCHANGE: `swap_out` answers the exchange error (`Not reverted: …`), as before; nothing is lost.
4. `WalkedLate` now also shows when a folder the agent itself made and removed during the scan was on the walk's list (D23 accepts the noise).
5. `Scanning` keeps its old wording (`! List may be incomplete (still scanning the folder)`): it has no time and no hint and already fits with the margin.

### Tests and commands (round 3, trimmed)
- Red: fix3-red.log — backend `test result: FAILED. 0 passed; 6 failed`; screens `FAILED. 6 passed; 3 failed`.
- New tests: 6 backend, 1 screen; 2 screen tests updated.
- Related after the last code change: `INSTA_UPDATE=no cargo test -p vr-ide --lib -- ai changes ai_view dialog file save notice` → `ok. 357 passed; 0 failed; 4 ignored`; `cargo test -p vr-core --lib -- safe_replace` → `ok. 15 passed`.
- Stress, 6 runs in a row: 6 × `ok. 4 passed` (e.g. `InPlace on a file: 170 swaps, 36 refused, 75 kept aside, 3 late writes kept in the trash`).
- Whole crates once (fix3-crate.log): `INSTA_UPDATE=no run-heavy.sh cargo test -p vr-ide` → lib `ok. 921 passed; 0 failed; 7 ignored`, desktop_cli `ok. 3 passed`, `exit vr-ide 0`; `cargo test -p vr-core` (its file changed) → `ok. 156 passed; 0 failed; 2 ignored`, `exit vr-core 0`.
- Guard (fix3-guard.log): `cargo fmt --all --check && run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings` → `guard exit 0` (the first run was red on `items_after_test_module`; the helper was moved).
- Formatter: `rustfmt --edition 2024` on the changed .rs files only.

### Rule compliance (round 3, rows that changed)
| Rule | Status | Evidence |
|---|---|---|
| SEC-9 / D21 | pass | `sec9_a_second_save_that_wrote_nothing_keeps_the_first_saves_mark`, `sec9_the_version_before_an_own_save…` |
| SEC-12 / D18 / D20 | pass | `sec12_without_exchange_the_replaced_version_still_goes_to_the_trash`, `sec12_a_taken_aside_name_is_passed_over`, round-2 D18 tests, stress 6 runs |
| SEC-13 | pass | `sec13_a_folder_gone_before_the_walk_reached_it_marks_the_list`, `sec13_a_later_narrower_notice…`, banner test at 80x24 and 120x40 |
| D22 / UI S3 | pass | `ai_changes_banners_keep_the_hint_and_the_margin`; ui-spec text updated |
| AC-9 | pass | crate 921 ok |
| USER-2 / USER-5 | pass | no mockup; no key added or changed |
| BASE red first | pass | fix3-red.log, one red per item |
| BASE-11 / BASE-24 | pass | no checkout / restore / stash / reset; nothing staged or committed; T6r2 not touched |
| BASE-14 / BASE-15 / COST-9 | pass | crate run as one background task; nothing left running |
| BASE-16 | pass | vr-core `rename_noreplace`, `name_stem` reused; `open_regular`, `holds`, `dialog::wrap` reused |
| BASE-25 | pass | `guard exit 0` |
| BASE-38 / COST-15 | pass | rustfmt on changed files; one golden, regenerated once after the code was final |
| COST related / crate once | pass | commands above |
| COST-8 | pass with a note | Edit tool for README; code edits through the exact-string, once-only replace script; no python |
| COST-13 | pass | logs in qa-artifacts/T6 |

DONE

## Review re-check r3 — 2026-11-27
Verdict: **APPROVED** (no blocker or major left; 2 minors, both optional).

### r2 findings
| Item | Status | Evidence |
|---|---|---|
| N1 [major SEC-13] queued folder gone before the walk | fixed | changes.rs `scan_step` ~1132 (watch failed and the path is not a folder) and ~1150 (`read_dir` NotFound / NotADirectory) → `note(WalkedLate)`; red→green `sec13_a_folder_gone_before_the_walk_reached_it_marks_the_list` (removed and moved out) |
| N2 [major SEC-9/D21] second save cleared the first save's mark | fixed | `own_save`: `marked = … && saving.insert(rel)` (false when an earlier save's mark is already there), `last_save = (rel, marked)`; the end call clears only when the latest begin set it. Traced: save1 begin (marked) → save2 begin (insert false → not marked) → ChangedOnDisk end → mark stays. The tracker handled save1 in between → save2 marks and takes back its own mark. Test `sec9_a_second_save_that_wrote_nothing_keeps_the_first_saves_mark` uses the real `Document::save` |
| N3 [major D18/D20] no RENAME_EXCHANGE | fixed | swap wrapper ~2814: on InvalidInput/Unsupported, `twin` (hard link, else a create_new copy with the mode) is checked with `holds(at, seen)` before safe_replace's fallback rename. The fallback re-check (`still`) falls back to the hash after the link changes ctime, so a Revert with a hard link is not refused. Changed or error → only the twin's name is removed. No twin → refused (D25). Test covers link, copy, changed, neither |
| D1 [major] timed banners cut at 80x24 | fixed (D22) | `banner_lines`: first line at width-2, later lines at width-4, NBSP glue on `at HH:MM` and `· enter hides`. Golden `ai_changes_limits_120x40` changed only the 2 banner rows (diff read). 7 causes asserted at 80x24 and 120x40, last cell free |
| D2 [minor] flush at the edge | fixed | `ai_changes_scan_gap_notices` rows end with a space |
| n1 name filter | fixed | `State::aside` set (inserted before the file exists, removed after trash/delete/failure); test `a_name_like_a_put_aside_version_made_by_the_agent_is_listed` |
| n2 collision / reuse | fixed | `put_aside` retries 100 counters on AlreadyExists; `sec12_a_taken_aside_name_is_passed_over` |
| n3 one notice slot | fixed | `note`: WalkedLate does not replace Unwalked; test |
| SWEEP-4 (vr-core) | pass | `rename_noreplace` only made `pub`; `name_stem` is the same code moved out of `keep_conflict` (byte-for-byte same cut); no other vr-core caller; vr-core 156 passed (fix3-crate.log) |

### New (minor, do not block)
- [minor] m1 — ui/ai_changes.rs ~273: at 120x40 the Unwalked banner breaks as `! Incomplete: scan stopped early` / `· enter hides`, so line 2 starts with the separator. Fix: also glue the word before the hint (`early·` with NBSP), which gives `! Incomplete: scan stopped` / `early · enter hides`. Copy-only cost.
- [minor] m2 — changes.rs `twin` copy fallback (no hard links, e.g. vfat): the whole file is copied on the UI thread before the Revert. A large file on a slow mount stalls the UI for the copy. Fix if wanted: cap the copy by `limits.file`, and above the cap refuse like D25.
- Note, outside T6: vr-fs/src/ops.rs:452 has its own `rename_noreplace` copy (it was there before T6). It could now use vr-core's.

### Files read
crates/vr-core/src/safe_replace.rs (diff), crates/vr-ide/src/ai/changes.rs (diff plus `still`/`holds`/checks context), ui/ai_changes.rs (diff), ui/tests/ai_changes_view_tests.rs (diff), snapshot `ai_changes_limits_120x40` (diff), README.md (skipped — copy only, checked against code in r2), ai/changes_tests.rs (test names + red log). Logs: fix3-red.log (6 + 3 red), fix3-crate.log (vr-ide 921, vr-core 156 ok).

## Rule check T6
| ID | Result | Evidence |
|---|---|---|
| AC-9 | pass | crate 921 ok; keep/revert/diff app tests |
| SEC-9 | pass | N2 trace + test; single-save test from r2 |
| SEC-12 / D18 / D20 / D25 | pass | EXCHANGE and no-EXCHANGE paths both put the replaced version in the trash or refuse; stress 6 × ok |
| SEC-13 | pass | Unwalked, WalkedLate (late folder, folder gone), WalkLimit, Scanning all set a banner; the narrower notice does not replace the wider one |
| BASE red-first | pass | fix3-red.log: 6 backend + 3 screen red with the stated reasons |
| SWEEP-4 | pass | save paths unchanged since r2 (pass); vr-core change is visibility + same-code refactor, 156 ok; one golden, banner rows only |

## Quick pass 1 (r3) — 2026-11-27
- m1: the banner text now joins the last word of the reason (or the time) to the hint with a no-break space (`{why}{GLUE}{hint}`), so a continuation line never starts with `·`. Copy unchanged.
- Red: `ai_changes_scan_gap_notices` expected `early · enter hides` on line 2 at 120x40; failed with `· enter hides`. Green after the change.
- Files: `crates/vr-ide/src/ui/ai_changes.rs` (banner text, 2 format strings + comment), `crates/vr-ide/src/ui/tests/ai_changes_view_tests.rs` (120x40 expectation). No snapshot changed (the limits_120x40 snapshot shows the timed notice, unaffected).
- Tests: `INSTA_UPDATE=no cargo test -p vr-ide --lib -- ai_changes ai_view` → 35 passed, 0 failed. `cargo clippy -p vr-ide --lib --tests` clean.
- Sizes: 80x24 and 120x40 covered by `ai_changes_scan_gap_notices` and `ai_changes_banners_keep_the_hint_and_the_margin` (all notices); 40x12 not asserted (banner not drawn there by existing layout rule, unchanged).
- Not staged/committed. rustfmt --check reports a pre-existing import order in ai_changes.rs (not touched).
DONE
- Guard (BASE-25), in .worktrees/T6r3: `cargo fmt --all --check` → rc=0, no output (the earlier import-order note came from a bare `rustfmt --edition 2021`, not the project config; no `use` line is in my diff). `run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings` → `Finished dev profile`, rc=0.

## Review re-check quick pass 1 (r3, m1) — 2026-11-27
**APPROVED.** ui/ai_changes.rs now glues the last word to the hint (`{}{GLUE}{hint}`, and `at{GLUE}HH:MM{GLUE}{hint}` for the timed texts). The U+00A0 is only for wrapping: `banner_lines` replaces it with a plain space before `set_stringn`, so the terminal never gets an NBSP. The width math stays right because `dialog::wrap` measures with unicode-width, where U+00A0 is 1 cell, the same as the space it stands for. Slicing `text[first.len()..]` is safe: the first line is a byte prefix of `text`, and the replacement happens after that slice. The longest glued unit, `early · enter hides` (19 cells), fits the continuation width at 80x24 (27) and 120x40 (31). The tests assert the exact rows at both sizes (`early · enter hides`, `during scan · enter hides`, `at 14:32 · enter hides`) and that the last cell is free. Panels narrower than about 23 cells would split the glued unit character by character (the old long-word behaviour); this layout never gets that narrow. m1 closed.
- escalation (fable): started; notes, QC r3 report, harness, changes.rs notice code and review.rs key handling read. Hypothesis: Enter lands on a file row (opens the diff), not on the banner row.

## Escalation — 2026-11-27
Fresh developer (fable). Worktree `.worktrees/T6r3`, release build current (`run-heavy.sh cargo build --release -p vr-ide` → `Finished`, nothing rebuilt). **No code changed in this round.**

### Progress
- read: notes, QC r3 report, QC harness (`flows_r3.py`, `dbg1..6.py`, `dbg1.txt`), `ai/changes.rs` (`note`, `scan_step`, `banner`, `dismiss_banner`, `pause`), `ai/review.rs` (`refresh`, `open_list`, `handle_key`), ui-spec S3, D22 / D23.
- replay of QC's exact steps on the build: `qa-artifacts/T6/esc/esc1.py` → `esc1-before.txt`. Reproduced, and the cause is visible on the screen.
- probe of the same cause showing again during the walk: `esc/esc2.py` → `esc2-before.txt`.

### Root cause of D-r3-1: Enter never reached the banner row
Nothing is stuck and the tracker does not re-note. In QC's scenario the keys do what ui-spec S3 says; the steps press Enter on a file row.
- ui-spec S3 (line 157): the banner "is the list's first row, `enter` on it dismisses it". `review::handle_key`: Enter on `Row::Banner` → `dismiss_banner`; Enter on `Row::File` → `open_diff`.
- The list opens with the first *file* selected (`open_list`: `selected: first_file`), and a banner that arrives while the list is open keeps the selected file selected (`refresh`: `l.selected += 1`, "the selected file stays the selected file"; asserted since round 1 by `ai_changes_tests::sec13_names_are_sanitized_and_a_watcher_error_shows_a_banner`: "the file stays selected", then Up, Enter hides).
- Why only `rm` + `mv` together: the slow `rm -rf` of 100 folders gives the walk time to reach and copy one of the `d28xx` folders before `mv` takes it out of the workspace. That folder's 50 files become 50 `D` rows (correct: they left the workspace). With rows in the list the selection is on a file, so every Enter opens (re-opens) its diff. In QC's contrast cases (`rm` only, `mv` only of folders the walk had not reached) the list has 0 files, the banner is the only row, it is selected, and Enter hides it. The one `mv`-only failure QC saw (dbg3) fits: timing let the walk reach a folder first.
- Evidence already in QC's own artifact: `qc/dbg1.txt` shows the diff tab `± f41` / `DIFF big/d2813/f41 · 1/50` opened by those Enters.

### Replay (BASE-37), `esc/esc1-before.txt`, 80x24 dracula, QC's steps verbatim
```
banner first seen after 1 s
[before Enter, 25 s later]  Changes · claude · 50 fil…   ! Incomplete: files deleted / during scan · enter hides   D f8 big/d2813 …   status: DIFF big/d2813/f8 · 1/50   (a file row is selected)
QC step: Enter 1 -> banner shown: True | diff tab open: True
QC step: Enter 2 -> banner shown: True | diff tab open: True
QC step: Enter 3 -> banner shown: True | diff tab open: True
[f6, Up to the first row]   status: AI claude ±50 (the banner row is selected)
Enter on the banner row -> banner shown: False
10 s later -> banner shown: False
```
There is no "after" build: nothing was changed, because the only change that makes QC's steps pass as written is a change of the specified key behaviour (below).

### Same cause showing again (brief's rule on re-arming), `esc/esc2-before.txt`
Banner hidden on its row as soon as it appears, while the walk still runs: `hidden 2 times; came back at (s): [2.8]`, `at the end shown: False`. While the walk is still finding folders that went away, each newly found one notes `WalkedLate` again, so a notice hidden in that window (here the first ~3 s) comes back; it stops with the walk and never returns afterwards (10 s and 45 s checks). Left as it is: each return is a newly found gap, and suppressing it would hide a later loss (the brief forbids weakening the marking). Bounded by the walk, not endless. If the user wants "hidden once = hidden for this walk", that is a second decision.

### QC flow 2 note: a WalkedLate after an exit mid-walk
It is never produced, by construction: `pause()` raises `epoch`, and `scan_step` returns at its first check `epoch != scan.epoch`, so the walk visits no further folder after the exit; `Unwalked` is the notice for everything not reached. The `covered` rule in `State::note` (n3) matters only for a restart (`resume`) whose new walk finds changed folders while an `Unwalked` notice is still shown. QC's "later folder removals do not replace it" passes because no walk runs, which is the intended state.

### Decision needed (product, not mine to pick)
The hint `enter hides` is true only with the banner row selected; a careful tester read it as "Enter hides from anywhere". Options:
- (a) Keep (ui-spec S3 as written, tested). QC replays with Up to the banner row, then Enter. No change. **Proposed default.**
- (b) Opening the list (f6) while a banner is shown selects the banner row, so the first Enter hides it and the selection then lands on the first file. A banner arriving while the list is open still does not take the selection. Small change in `open_list` + 1 test; changes "f6, Enter opens the first diff" whenever a banner is shown.
- (c) A banner takes the selection whenever it appears. Makes QC's steps pass as written, but an Enter meant to open a diff can hide a notice the user has not read: weakens SEC-13 in practice. Not recommended.
- (d) Copy that names the row (e.g. `↑ enter hides`): reopens the D22 width work at 80x24.

### Tests / commands
- `INSTA_UPDATE=no cargo test -p vr-ide --lib -- sec13_names_are_sanitized_and_a_watcher_error_shows_a_banner sec13_a_folder_gone` → `test result: ok. 2 passed; 0 failed` (the existing tests that pin both halves: the notice is set; a file stays selected and Up + Enter hides).
- Related set, whole crate and guard: not rerun; the tree is byte-identical to the quick-pass state they last ran on (35 related passed, guard rc 0; crate 921 passed in fix3-crate.log before the copy-only quick pass).
- No red/green: no defect in the code to turn red.

### Rule compliance (escalation)
| Rule | Status | Evidence |
|---|---|---|
| SEC-13 marking / "enter hides" | pass per ui-spec S3 (Enter on the banner row); open question on the hint | esc1-before.txt; test above; marking untouched |
| SEC-13 same cause re-arming | pass with a note | esc2-before.txt: returns only while the walk still finds losses, never after |
| BASE red-first | n-a | no code defect found; nothing changed |
| BASE-37 | pass | QC's exact steps replayed on the build (esc1.py reuses QC's harness); no after-build because no change |
| BASE-11 / BASE-24 | pass | no checkout / restore / stash; nothing staged or committed; `git status` unchanged |
| BASE-25 guard | n-a | no file changed |
| BASE-38 / COST-15 | n-a | no formatter, no golden |
| BASE-14 / BASE-15 / COST-9 | pass | foreground runs only; both harness sessions killed by pid; `ps` shows nothing left; `ws/big` and `qc/moved_out` removed |
| COST related / crate once | n-a | no change to verify; 2 pinned tests run |
| COST-8 / COST-13 | pass | scripts in `qa-artifacts/T6/esc/`; nothing in `$TEAM_TOOLS` |

BLOCKED: needs-user-decision — banner dismissal when files are listed (D-r3-1 is the specified behaviour, not a stuck state; options a–d above, default a)
