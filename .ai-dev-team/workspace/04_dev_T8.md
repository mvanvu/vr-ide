# T8 Explorer panel + go-to-file (M2 UI)
Task base: 9197473778ee245b24e67b46cd0fced819288773. Side: fe (ratatui). Data-safety code (explorer chooses which user paths are renamed/moved/copied/trashed).

## Progress
- read brief-common, plan T8 + contracts, spec D5/D9/AC-1/8/9, decisions D9–D98, T6 notes (dialogs/palette API), T7 review (ensure_within/root/nested batch), vr-fs tree/ops/file_index, vr-core watch/document/testutil, vr-ide app/ui/commands
- skeleton: Focus::Explorer, App.{explorer,trash,file_index,index_rx,index_stale}, layout explorer column (explorer_width), commands/explorer.rs registered (explorer.*, file.goto, focus.cycle out of PENDING), ui/explorer.rs; test_app hides the explorer (T5/T6 tests address absolute columns); palette test's pending probe now 'search.replace_project'; render_palette snapshot now differs (explorer commands listed) — re-accept at the end
- red explorer_shows_root_tree `left: [] right: [".tmpJtOT2i", "  src", ...]` → green (Explorer::{expand, collapse, reload, reload_all, rebuild}, ensure_loaded)
- red explorer_expand_collapse_keyboard_and_mouse `left: Editor right: Explorer` (explorer.focus did nothing) → green (explorer.toggle/focus, built-in keys, mouse, ui/explorer.rs render)
- red explorer_new_file_opens_it `ctrl+n opens the name prompt` → green (explorer.new_file/new_folder prompts, answer_new: validate_name → ensure_within → ops::create_*; DialogAction::{NewEntry,Rename,MoveTo,Trash}; describe(FsOpError))
- red explorer_rename_open_file_updates_tab `f2 opens the rename prompt` → green (explorer.rename, answer_rename: validate_name → below_root/ensure_within → ops::rename; retarget tabs (files and folders, modified buffers kept) + App::rewatch; root refused)
- red explorer_delete_confirms_then_trashes `delete asks first (D9)` → green (explorer.delete confirm Move to Trash / Cancel, default Cancel, unsaved names in the message; answer_trash: below_root per path, drop_nested, keep_discarded first (abort on failure), App.trash batch, per-path errors via describe, tabs of trashed paths closed)
- red explorer_copy_paste_and_move `NotFound` at tests/main.rs read (paste did nothing) → green (explorer.copy/paste with free 'copy' names on AlreadyExists, explorer.move prompt → answer_move: ensure_within dest (folder), below_root per path, drop_nested, ops::move_to, retarget tabs, per-path errors)
- red explorer_reveal_active_file `assertion failed: app.explorer.visible` → green (explorer.reveal, reveal_path expands ancestors)
- red explorer_toggle_ignored `left: [] right: ["target", "debug.log"]` → green (explorer.toggle_ignored / toggle_hidden re-read every expanded folder)
- red explorer_refreshes_on_fs_events (rows without added.rs) and explorer_follows_the_real_watcher (5 s deadline, no row) → green (on_fs_event notes expanded parents per Created/Removed/Renamed/unknown Modified/Rescan, flush re-reads once per batch; App::{handle_fs_event, drain_fs_events} call them; App::watch_root watches the root recursively); test fixture corrected once (Controller had not been expanded)
- red focus_cycle_editor_explorer `left: Editor right: Explorer` → green (focus.cycle)
- red goto_file_opens_selected: first compile errors (no drain_index / PaletteItem.path), then runtime `left: Editor right: Palette` → green (PaletteMode::{Commands,Files}, PaletteItem{title: String, path, detail}, open_files, file_items, accept opens files, App::{start_index, drain_index}, index_stale set by fs events/ops). NOTE: one perl substitution with '|' delimiter mangled the head of commands/palette.rs; repaired by hand with Edit (header + refilter + accept), no git restore used
- red startup_lists_leftover_temp_files_and_keeps_them (no notice) → green (is_leftover + report_leftovers on first load of the root and on each expand, each path named once, read-only scan)
- operation_errors_name_their_paths, trash_of_unsaved_file_stops_when_recovery_fails: written after describe/answer_trash → red by mutation (CrossDevice text generic: `"move failed a.txt mnt/a.txt" lacks "different file systems"`; keep_discarded error ignored: `assertion failed: trash.trashed().is_empty()`) → restored → green
- explorer_{rename,move}_never_loses_a_concurrent_write (RacingWriter InPlace + TempRename × 20 offsets, through f2 / explorer.move prompts with a tab open; assert_versions_survive + tab follows): written after rename/move → red by mutation (rename as read + write + remove_file by name), 3/3 `panicked at crates/vr-core/src/testutil.rs:485:23: lost write: the newest writer version is in no file` → restored → green
- render_explorer / render_explorer_narrow: red as missing snapshots (`snapshot assertion for 'render_explorer_80x24' failed`); frames read by eye, name ellipsis added, then accepted once together with render_palette_{80x24,40x12} (one line changed each: "Focus Next Part  focus.cycle  f6" is now listed — no longer pending)
- wiring: event_loop drains the index each frame; main.rs calls `watch_root` + `start_index` after the first frame. NOTE: a second '|'-delimited perl edit mangled line 1 of main.rs; repaired by hand (line 1 restored, insertion redone with Edit) — no git restore used
- decisions.md (8 `D? (T8)` lines), capabilities.md (explorer, ui/explorer, palette/App additions)
- pty smoke (qa-artifacts/T8/pty_smoke.py, debug binary, 80x24, temp XDG trash): all checks true, exit 0
- guard green; racing + real-watcher tests 5 runs in a row green

## Summary
- Files changed: crates/vr-ide/src/{app.rs, app_tests.rs (test_app hides the explorer), dialog_tests.rs (pending probe now `search.replace_project`), event_loop.rs, lib.rs, main.rs}, commands/{dialog.rs, mod.rs, palette.rs, registry.rs}, ui/{layout.rs, mod.rs, palette.rs}, ui/snapshots/vr_ide__ui__tests__render_palette_{80x24,40x12}.snap (re-accepted).
- New: commands/explorer.rs, ui/explorer.rs, explorer_tests.rs, ui/snapshots/vr_ide__ui__tests__render_explorer_{80x24,narrow}.snap. Team files: this notes file, decisions.md, capabilities.md, qa-artifacts/T8/pty_smoke.py.
- Not touched: vr-core, vr-fs, vr-search, vr-config, any Cargo.toml.
- Tests added (19): explorer_tests {explorer_shows_root_tree, explorer_expand_collapse_keyboard_and_mouse, explorer_new_file_opens_it, explorer_rename_open_file_updates_tab, explorer_delete_confirms_then_trashes, explorer_copy_paste_and_move, explorer_reveal_active_file, explorer_toggle_ignored, explorer_refreshes_on_fs_events, explorer_follows_the_real_watcher, focus_cycle_editor_explorer, goto_file_opens_selected, startup_lists_leftover_temp_files_and_keeps_them, operation_errors_name_their_paths, trash_of_unsaved_file_stops_when_recovery_fails, explorer_rename_never_loses_a_concurrent_write, explorer_move_never_loses_a_concurrent_write}; ui::tests {render_explorer (render_explorer_80x24), render_explorer_narrow}.

### Brief must-haves → where
| Must-have | Code | Test |
|---|---|---|
| validate_name on every typed name | answer_new, answer_rename | explorer_new_file_opens_it ("a/b.rs", ".."), rename "lib.rs.tmp/x" |
| ensure_within, use the returned path | `within`/`below_root` → every op uses the result (new, rename from+to, paste src+dest, move dest+each src, trash each) | copy_paste_and_move ("../outside" → "outside the project") |
| never rename/trash/move the root | `below_root` + `ROOT_REFUSED`; row depth 0 filtered in `targets` | rename/delete/move tests (root) |
| drop nested before batch trash | `drop_nested` in `targets` and again in answer_trash/answer_move | delete test (src/Controller, src/main.rs dropped) |
| per-path BatchResult | answer_trash, paste, move collect each error | delete test (vanished a.txt named, b.txt trashed); move (name taken) |
| delete confirms, default Cancel, Enter settle | ConfirmDialog selected=1 via `App::open_dialog` (T6 guard) | delete test |
| tabs follow rename/move incl. modified buffer; folder rename | `retarget` + `App::rewatch` | rename test, copy_paste_and_move (modified README moved) |
| leftover notice (root + expanded), never delete | `report_leftovers`, `is_leftover` | startup_lists_leftover_temp_files_and_keeps_them, smoke step 1/6 |
| CrossDevice/PartialCopy/BothNamesLeft/LeftAt/TrashRefused/AlreadyExists messages | `describe` | operation_errors_name_their_paths |
| unsaved: rename/move keep buffer; trash asks, buffer to recovery | message in confirm; `keep_discarded` before trash, abort on failure | delete test, trash_of_unsaved_file_stops_when_recovery_fails |
| refresh on FsEvents incl. Rescan, keep expansion/selection | `on_fs_event` + `flush` | explorer_refreshes_on_fs_events, explorer_follows_the_real_watcher |
| go to file: background FileIndex, palette, Enter opens | `open_files`, `start_index`/`drain_index` | goto_file_opens_selected, smoke step 2 |
| PENDING cleaned | registry.rs (only T10 names remain) | registry_has_every_plan_command_once, default_keymap_validates_cleanly |
| writes only via vr-fs ops / Document | grep: no std::fs write/rename/remove in commands/explorer.rs (only `read_dir` for leftovers) | racing tests |
| racing-writer test | race_explorer (rename + move) | both green; red by mutation recorded |
| snapshots | ui::tests | render_explorer_80x24, render_explorer_narrow |

### Commands run (final, trimmed)
```
$ cargo test -p vr-ide
test result: ok. 98 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.81s   (lib; 2 ignored = existing perf tests)
test result: ok. 0 passed ... (main.rs, doc-tests)
$ for i in 1..5: cargo test -p vr-ide --lib -- never_loses_a_concurrent_write explorer_follows
test result: ok. 6 passed; 0 failed ... (x5, 0.78–0.87s)
$ cargo clippy -p vr-ide --all-targets -- -D warnings → Finished (manifest note only: vr-search unused until T10)
$ cargo fmt --all --check → rc=0
$ run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings → Finished, only the vr-search manifest note
$ python3 -I qa-artifacts/T8/pty_smoke.py <abs>/target/debug/vr-ide <scratch tmp>
1. tree shown: True | leftover notice: True
2. go to file shown: True | opened main.rs: True
3. new file on disk: True (folder under the explorer cursor = root)
4. rename prompt: True | renamed: True True
5. delete confirm: True | in trash: True | gone: True   (XDG_DATA_HOME = temp dir)
6. outside create shown: True | leftover kept: True
exit status: 0
```
(tmp work dir removed; `ps` count of vr-ide = 0.)

### Deviations (with reason)
- `test_app` hides the explorer (D? T8): the T5/T6 tests address absolute screen columns; explorer tests show it.
- `commands/palette.rs`, `commands/dialog.rs`, `commands/registry.rs`, `event_loop.rs`, `main.rs`, `lib.rs`, `app_tests.rs`, `dialog_tests.rs` edited beyond the board's file list (go-to-file reuses the palette; dialog arms; PENDING; drain/startup wiring; test module) — all inside crates/vr-ide/src/**, as the brief allows.
- Additive APIs (BASE-2): `PaletteItem.title` is now `String` with new `path`/`detail` fields; `layout::compute` takes `explorer_cols`; `App::{watch_root, start_index, drain_index}`, `App.{explorer, trash, file_index}`.

### Out of scope / risks noticed
- `watch_root` adds recursive inotify watches over the whole project (vr-core Watcher has no non-recursive folder mode). It runs after the first frame but blocks input for the walk on very large trees, and a tree over `fs.inotify.max_user_watches` (65536 here) only gets a warning notice (refresh after own actions still works). A non-recursive "watch these folders" mode in vr-core would fit the lazy explorer better — leader's call.
- Collapsing a folder forgets the expansion of folders below it (re-read when shown again).

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| tree listing, ignore rules | capabilities (vr-fs tree) | reused `Tree::children` lazily per expanded folder |
| file ops, no-overwrite | vr-fs ops | reused `create_file/create_dir/rename/copy/move_to`, `validate_name`, `ensure_within`, `Trash`/`SystemTrash`/`FakeTrash` |
| prompts / confirm | T6 dialogs | reused `PromptDialog`, `ConfirmDialog`, `reopen_with_error`, `open_dialog` (Enter settle), new `DialogAction` variants |
| go to file list | T6 palette | extended `Palette` with `PaletteMode::Files` (same input, list, keys, mouse, render) |
| fuzzy files | vr-fs file_index | reused `FileIndex::build/query` |
| recovery before discarding | app.rs | reused `App::keep_discarded` |
| watcher re-targeting | app.rs | reused `App::rewatch` |
| watching the tree | vr-core watch | reused `Watcher::watch(dir)` (recursive) |
| save temp names | vr-core document | reused `vr_core::document::is_save_temp` |
| racing writer | vr_core::testutil | reused `RacingWriter`, `assert_versions_survive`, `sweep_offsets` |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | `git status`: crates/vr-ide/src/** + team files only |
| BASE-2 | pass | registry contract unchanged (`CommandSpec`, `CmdArgs`, names); additive APIs listed under Deviations |
| BASE-3 | pass | red line per test in Progress; 4 tests red by mutation (recorded); snapshots red as missing |
| BASE-4 | pass | "Commands run (final)" |
| BASE-5 | pass | workspace clippy via run-heavy.sh; smoke child exited 0; nothing left running |
| BASE-6 | pass | English; comments say why (no-overwrite, ensure_within result, leftovers never deleted) |
| BASE-8 | pass | no manifest change |
| BASE-9 | pass | user files written only via vr_fs::ops (create/rename/copy/move/trash) and Document (save); recovery before trash; racing tests |
| BASE-10 | pass | `cargo test -p vr-ide` only |
| BASE-11 | pass | no stash/checkout/restore/reset; mutations reverted by copying the saved file back; mangled lines repaired by hand |
| BASE-14 | pass | all runs foreground < 1 min |
| BASE-15 | pass | nothing started in background; nothing killed |
| BASE-16 | pass | Reuse table; capabilities.md updated |
| BASE-17 | pass | no installs |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | notes created first, progress line per step |
| BASE-20 | pass | PENDING entries removed: names now registered in explorer::COMMANDS (registry tests green); palette test probe moved to a still-pending name |
| BASE-23 | pass | explorer by keys, by mouse and by palette/commands; go to file by ctrl+p (key) and command; focus by f6, ctrl+shift+e, mouse |
| BASE-24 | pass | no git index commands |
| BASE-25 | pass | fmt check rc=0, workspace clippy clean |
| BASE-32 | pass | D9 (confirm + trash), D10/USER-1 (keys only from keymap), D15–D18 (vr-fs tree/index policies used as is), D24 (CrossDevice message, no copy+delete), D25/D32 (vr-fs no-overwrite ops only), D76.4 (leftover notice, never deleted), D79–D98 (T6 dialogs, Enter guard, recovery before discard) |
| BASE-35 | pass | USER-1: f2/delete/ctrl+c/ctrl+v/ctrl+n/ctrl+b/ctrl+shift+e/ctrl+p from vr-config defaults; built-ins documented (D? T8) |
| BASE-38 | pass | `rustfmt --edition 2024` on my files only |
| BASE-46 | pass | smoke ran on a binary built after the last code change |
| COST-1 / COST-3 / COST-9 | pass | heavy via run-heavy; no polling loops in shell; nothing left running |
| COST-8 | pass | new files written whole; edits batched |
| COST-10 | pass | related set = vr-ide tests (one crate) |
| COST-13 | pass | script in qa-artifacts/T8/ |
| COST-15 | pass | snapshots accepted once after code was final, frames read first |
| COST-16 | pass | output filtered to summary lines |
| AC-1 | pass | explorer_shows_root_tree, render_explorer_*, smoke 1 |
| AC-8 | pass | new_file, rename (tabs follow), copy_paste_and_move, delete_confirms_then_trashes, racing tests, smoke 3–5 |
| AC-9 | pass | explorer_toggle_ignored; goto_file_opens_selected (ignored file not offered) |
| USER-1 | pass | see BASE-35 |

n-a: BASE-7 (no commit) · BASE-12, BASE-13 (nothing outside the repo touched; leftovers never deleted) · BASE-21 (no secrets) · BASE-22 (no new env var/config key) · BASE-26 (not a rewrite) · BASE-27, BASE-30, BASE-31, BASE-39, BASE-41, BASE-43, BASE-44 (no DB, containers, schema, credentials, access rules, fakes of other systems) · BASE-28, BASE-29, BASE-33, BASE-34, BASE-36, BASE-40 (no third-party code, browser, external system, outward writes) · BASE-37 (not a defect fix) · BASE-42 (no test runner settings) · BASE-45 (AC text unchanged) · BASE-47 (no fixed value) · COST-2 · COST-4 (no full suite) · COST-6, COST-7 (no web UI) · COST-11 (no e2e) · COST-12 · COST-14 · COST-17 · COST-18 · COST-19, COST-20

## Fix round 1
Scope: QC folder paste, review M2 (wheel/hit-test), minors (USER-1 keys, D108 cut, index selection, dead code, default layout test, drop_nested, D109 symlink, ignored text marker). M1 (D110) last, needs FR-4 watch_dir.
- read brief-common, 04_dev_T8, 05_review_T8, QC T8; master watch.rs has no `watch_dir` (FR-4 still in worktree) → M1 deferred
- red paste_into_own_parent_makes_a_copy_name `not pasted: cannot move or copy src into itself (src)` → green (copy_into skips the candidate equal to the source); test fixed once (2nd paste must re-select the root target)
- red explorer_wheel_scrolls_freely_and_clicks_hit_drawn_rows `the redraw pulled the view back to the cursor left: 1 right: 12` → green (Explorer.follow_cursor cleared by the wheel, set by keys/clicks/select_path; Explorer.drawn_top stored by render, used by click hit tests; wheel clamps top to rows-height)
- red user_explorer_bindings_of_plain_keys_win_over_built_ins `space binding never ran left: false right: true` → green (route_key: built-in explorer keys only when the chord has no binding in the explorer context)
- red explorer_cut_then_paste_moves `assertion failed: crate::commands::find("explorer.cut").is_some()` → green (explorer.cut, Explorer.cut flag, paste of a cut = move_paths once (shared with answer_move), same-folder no-op, AlreadyExists/IntoItself reported); default key line belongs to vr-config (out of file scope, see notes)
- red goto_file_keeps_selection_when_index_arrives `left: Some("README.md") right: Some("src/main.rs")` → green (palette::refresh_keeping_selection, used by drain_index; typing still resets to the top)
- removed dead statements in explorer_shows_root_tree
- FR-4 merged (0048d5a): M1 in this round. Wrote explorer_watches_only_shown_folders, goto_file_rebuilds_its_index_each_time_it_opens (red runs below)
- red drop_nested_keeps_order_and_scales `panicked ... 122.773923797s` (O(n^2) on 40k paths) → green (sort by components + one pass against the last kept path; HashSet filter keeps order, repeats once); called once, in `targets` (removed from answer_trash and the move loop: their paths come from `targets`)
- red explorer_watches_only_shown_folders `the root only, not recursive left: {} right: {"/tmp/.tmpmYpYfI"}`; red goto_file_rebuilds_its_index_each_time_it_opens `opening go to file rebuilds the index` → green (App::watch_root = watching flag + App::sync_explorer_watches: watch_dir root + each expanded, unwatch_dir on collapse/vanish, called after keys, mouse, commands, fs-event flushes; recursive root watch removed; open_files always start_index; index_stale removed)
- red linked_folder_outside_the_project_refuses_destructive_ops (rename went through: empty error text at explorer_tests.rs:1216) → green (below_root: canonical parent must be under the canonical root; covers rename, move, cut-paste, trash; new/paste/trash of the link itself allowed)
- red ignored_rows_carry_a_text_marker `the notice says what the marker means` → green (ui/explorer IGNORED_MARK " ·" after ignored names, kept when the name is cut; toggle notice "(marked ·)")
- default_layout_with_explorer_clicks_editor_and_tabs: green at first run (coverage gap, code was right); red by mutation (char_at_screen without area.x) `left: (13, 13) right: (9, 9)` → restored → green
- explorer_cut_paste_never_loses_a_concurrent_write (race_explorer("cut")): written after the cut code, reuses the move harness; green, 3 runs in a row with the other racing + real-watcher tests
- rustfmt on changed files; cargo test -p vr-ide 110 passed; clippy clean; release build; pty flows (fix1_flows.py = qc_flows.py with the contradictory folder-copy condition corrected) running
- pty flows on the release binary (built after the last code change): every T8 flow PASS (AC-1+AC-9, create, rename file/folder, copy-paste incl. "src copy" ×2, move, delete, delete-unsaved, safety, nested trash, leftovers, External <1 s, GoTo, Tiny 40x12, Big: inotify_watches=1 (563 before), Concurrent-rename 20/20); the T5-era flows the harness also runs (AC-7c, R1-ctrl+q-esc-ctrl+q, R1-legacy-refusal-visible, RECOVERY n-o, AC-7-race script TypeError) check pre-T6 behaviour that their T6 successors cover (all T6 flows PASS); none involves the explorer. Temp dirs removed; `ps` count of vr-ide = 0

### Findings → reply
| Finding | Reply | Code | Test (red line) |
|---|---|---|---|
| QC folder paste in own parent | fixed | `copy_into` skips the candidate equal to the source; every try still O_EXCL (no overwrite) | paste_into_own_parent_makes_a_copy_name (`not pasted: cannot move or copy src into itself (src)`) |
| M2 wheel pinned / click after scroll | fixed | `Explorer.follow_cursor` (wheel clears; keys, clicks, select_path set), `Explorer.drawn_top` from render used by click hit tests; wheel clamps to rows-height | explorer_wheel_scrolls_freely_and_clicks_hit_drawn_rows (`the redraw pulled the view back to the cursor left: 1 right: 12`) |
| M1 / D110 recursive watch on UI thread | fixed | `App::watch_root` → `sync_explorer_watches`: `watch_dir` root + expanded, `unwatch_dir` on collapse or vanish; called after keys, mouse, commands, fs-event flushes; recursive root watch gone; open-file watches untouched; `open_files` rebuilds the index each open; `index_stale` removed | explorer_watches_only_shown_folders (`the root only, not recursive left: {} right: {root}`), goto_file_rebuilds_its_index_each_time_it_opens (`opening go to file rebuilds the index`); pty Big: 1 watch |
| USER-1 plain keys swallowed | fixed | route_key: built-in explorer keys only when no keymap binding for the chord in the explorer context | user_explorer_bindings_of_plain_keys_win_over_built_ins (`space binding never ran left: false right: true`) |
| D108 explorer.cut | fixed (command); default key needs vr-config | `explorer.cut`, `Explorer.cut`, paste of a cut = `move_paths` once (shared with explorer.move), same-folder no-op | explorer_cut_then_paste_moves (`find("explorer.cut").is_some()` failed); explorer_cut_paste_never_loses_a_concurrent_write (written after; green) |
| drain_index resets selection | fixed | `palette::refresh_keeping_selection` (by name + path), used by drain_index | goto_file_keeps_selection_when_index_arrives (`left: Some("README.md") right: Some("src/main.rs")`) |
| dead code explorer_tests.rs:60-62 | fixed | removed | — |
| default layout test | fixed | — | default_layout_with_explorer_clicks_editor_and_tabs (green first: coverage gap; red by mutation `left: (13, 13) right: (9, 9)`) |
| drop_nested O(n²), twice | fixed | sorted pass, HashSet filter, order kept, repeats once; called only in `targets` | drop_nested_keeps_order_and_scales (`122.773923797s`) |
| D109 linked folder outside root | fixed | `below_root`: canonical parent must be under the canonical root (rename, move, cut-paste, trash); new/paste/trash of the link itself allowed | linked_folder_outside_the_project_refuses_destructive_ops (rename went through) |
| QC ignored flag colour only | fixed | trailing " ·" (kept when cut), toggle notice "(marked ·)" | ignored_rows_carry_a_text_marker (`the notice says what the marker means`) |
| BASE-3 (4 tests after code, round 0) | noted | — | this round: each test run red before its code, except the cut racing test and the layout test (stated above) |

### Needs the leader (outside crates/vr-ide/src/**)
- `crates/vr-config/defaults/keymap.toml`, `[explorer]` table: add `"ctrl+x" = "explorer.cut"` (D108). If vr-config/tests/config.rs keeps a list of command names (line ~43), add `explorer.cut` there as well. Until then ctrl+x in the explorer does nothing; explorer.cut works from the palette and from a user binding (tested).

### Risks noticed
- D109 check is a canonicalize before the op: a link swapped between the check and the op is not covered (same window class as `ensure_within`, D70).
- Snapshots: no snapshot changed this round (render_explorer frames show no ignored rows; palette frames do not reach "Explorer: Cut"), so nothing was re-accepted.

### Commands run (final, trimmed)
```
$ cargo test -p vr-ide
test result: ok. 110 passed; 0 failed; 2 ignored; ... finished in 0.82s   (lib)
test result: ok. 0 passed ... (main.rs, doc-tests)
$ 3x cargo test -p vr-ide --lib -- never_loses_a_concurrent_write explorer_follows explorer_watches
test result: ok. 8 passed; 0 failed ... (0.81 s, 0.93 s, 0.92 s)
$ cargo clippy -p vr-ide --all-targets -- -D warnings → Finished (manifest note: vr-search unused until T10)
$ run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings → Finished (same note only)
$ cargo fmt --all --check → rc=0
$ run-heavy.sh cargo build --release -p vr-ide → Finished release in 9.29s
$ python3 -I qa-artifacts/T8/fix1_flows.py target/release/vr-ide <scratch> → all T8 flows PASS (see progress line)
```
Files changed this round: crates/vr-ide/src/{app.rs, main.rs, explorer_tests.rs}, commands/{explorer.rs, palette.rs}, ui/explorer.rs. Team files: this notes file, decisions.md (5 `D? (T8 r1)` lines), capabilities.md, qa-artifacts/T8/fix1_flows.py.

### Rule compliance (round 1)
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | only crates/vr-ide/src/** + team files; the vr-config key line handed to the leader |
| BASE-2 | pass | registry contract unchanged; additive: `explorer.cut`, `Explorer.{follow_cursor, drawn_top, cut, watched, watching}`, `App::sync_explorer_watches`, `palette::refresh_keeping_selection`; `App.index_stale` removed (crate-private) |
| BASE-3 | pass | red line per test in Findings; two exceptions stated (layout test red by mutation, cut racing test written after) |
| BASE-4 | pass | Commands run (final) |
| BASE-5 | pass | workspace clippy and release build via run-heavy.sh; nothing left running |
| BASE-6 | pass | English; comments say why (into-itself order, wheel vs cursor, D109 lexical check, D110 cost) |
| BASE-8 | pass | no manifest change |
| BASE-9 | pass | writes only via vr_fs::ops; cut-paste racing test; paste never overwrites (O_EXCL tries) |
| BASE-10 | pass | `cargo test -p vr-ide` only |
| BASE-11 | pass | no stash/checkout/restore/reset; mutation restored with sed on the one line |
| BASE-14 | pass | release build and pty flows ran as background tasks, turn ended while they ran |
| BASE-15 | pass | nothing killed; `ps` count of vr-ide = 0 |
| BASE-16 | pass | reused move/copy harness, RacingWriter, watch_dir from FR-4; capabilities.md updated |
| BASE-17 / BASE-18 | pass | no installs; no sub-agents |
| BASE-19 | pass | progress lines under Fix round 1 |
| BASE-20 | pass | index_stale: all 13 references removed (grep clean) |
| BASE-23 | pass | cut by command and by key binding; watches by keys and mouse paths (sync after both) |
| BASE-24 | pass | no git index commands |
| BASE-25 | pass | fmt --check rc=0; workspace clippy clean |
| BASE-32 | pass | D9, D103/D108, D109, D110 applied; D107 amended (decisions.md) |
| BASE-38 | pass | rustfmt --edition 2024 on the six changed files only |
| BASE-46 | pass | pty flows on a release build made after the last code change |
| COST-1 / COST-3 / COST-9 | pass | heavy via run-heavy; no polling loops; nothing left running |
| COST-8 | pass | Edit tool; sed only for single-line edits; no perl on code |
| COST-10 | pass | related set = vr-ide tests |
| COST-13 | pass | fix1_flows.py in qa-artifacts/T8/ |
| COST-15 | pass | no snapshot changed, none re-accepted |
| AC-1 / AC-8 / AC-9 | pass | tests above; pty flows |
| USER-1 | pass (vr-ide side) | keymap wins for bound chords; ctrl+x default pending the vr-config line |
| D9, D100–D110 | pass | see BASE-32 |

n-a: BASE-7 (no commit) · BASE-12, BASE-13 · BASE-21 (no secrets) · BASE-22 (no new env var/config key) · BASE-26 · BASE-27, BASE-30, BASE-31, BASE-39, BASE-41, BASE-43, BASE-44 · BASE-28, BASE-29, BASE-33, BASE-34, BASE-36, BASE-40 · BASE-37 (QC defect replayed through the same key path in a test and in the pty flow) · BASE-42 · BASE-45 · BASE-47 · COST-2, COST-4, COST-6, COST-7, COST-11, COST-12, COST-14, COST-16–COST-20

### D108 default key (leader-approved, outside crates/vr-ide/src/**)
- red: crates/vr-config/tests/config.rs `defaults_load_without_user_file` (new asserts: explorer ctrl+x → explorer.cut, editor ctrl+x stays edit.cut; `explorer.cut` added to the command-name list) `panicked at crates/vr-config/tests/config.rs:237:5: left: None right: Some("explorer.cut")` → green after `"ctrl+x" = "explorer.cut"` in crates/vr-config/defaults/keymap.toml `[explorer]`
- explorer_tests: the cut and D109 tests no longer load a user keymap; they press ctrl+x on the default keymap
```
$ cargo test -p vr-config -p vr-ide
test result: ok. 3 passed ... (vr-config unit) | ok. 16 passed ... (vr-config tests/config.rs) | ok. 110 passed; 0 failed; 2 ignored (vr-ide lib)
$ cargo fmt --all --check → rc=0
$ cargo clippy -p vr-ide -p vr-config --all-targets -- -D warnings → Finished (vr-search manifest note only)
$ run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings → Finished
```
- USER-1 / D108 now pass fully: ctrl+x in the explorer = explorer.cut by default.
