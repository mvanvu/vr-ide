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
