# T5 App shell: event loop, editor view, commands, tabs, status, clipboard, recovery

## Progress
- read brief, plan T5, decisions, capabilities, profile
- read vr-core/vr-config APIs, T3/T4 notes, ratatui 0.30/crossterm 0.29 sources (styled_graphemes drops control graphemes incl. tab, so the view walks graphemes with vr-core helpers)
- design: vr-ide gets src/lib.rs (modules) so the App API is a library for T6/T8/T10 and tests; main.rs stays the thin binary
- skeleton: lib.rs + app/commands/ui/event_loop/clipboard/recovery/theme with stub bodies; builds
- red dispatch_ctrl_s_saves: `left: "hello\n" right: "xhello\n"` (handle_key stub) → green after handle_key, chord_from_key, typing, file.save
- red typing_marks_modified_and_save_clears: `left: "xab\n" right: "xb\n"` (no backspace/arrows); red undo_redo_via_commands: `left: "ab\none\n" right: "abone\n"`; red unknown_command_in_keymap_shows_notice_not_crash: assertion `all.contains("nope.cmd")` failed (report_startup stub) → green after builtin keys, undo/redo, report_startup
- red copy_paste_uses_clipboard `left: None right: Some("hello")`; red cut_removes_selection `left: None right: Some("abc")`; red select_all_and_line_commands `left: "a\nb\n" right: "a\na\nb\n"` → green after edit.* commands
- red external_modify_unmodified_doc_reloads `left: "old\n" right: "new from agent\n"`; red external_modify_modified_doc_keeps_buffer_and_marks_tab `left: InSync right: ChangedOnDisk`; red external_delete_keeps_buffer_marked_deleted `left: InSync right: DeletedOnDisk`; red external_modify_reloads_within_one_second_with_real_watcher `left: "v1\n" right: "v2\n"` → green after handle_fs_event/recheck + file.save_overwrite
- red quit_and_close_with_unsaved_need_a_second_press (assert !app.quit failed), open_tabs_switch_and_reopen_activates `left: 0 right: 3`, save_as_with_path_and_reload (unwrap on missing copy.txt), bracketed_paste_is_one_undo_step `left: "\n" right: "line1\nline2\n"`, legacy_terminal_notice_lists_unreachable_default_keys (no notice) → green after quit/close confirm, file.open/save_as/reload, tab.next/prev, paste, legacy_unreachable. read_only_doc_refuses_edits, pending_and_palette_commands_show_a_notice, title_shows_project_root passed on first run (they pin guards written with the skeleton: editable_doc, PENDING stubs, App::title) — not red-first, noted
- one test expectation fixed (cursor sits after "x" after reload, so typing gives "xyone")
- red base64_matches_rfc4648_vectors `left: "" right: "Zg=="`, red osc52_fallback_writes_sequence_and_pastes_last_copy `left: "" right: "\u{1b}]52;c;\u{7}"` → green after base64/osc52_sequence
- event_loop chord tests (ctrl_s_and_ctrl_q_map_to_their_chords, legacy_control_bytes_map_back_to_their_keys, shifted_keys_use_the_unshifted_name, legacy_unreachable_chords) and registry tests passed on first run: chord_from_key was written for dispatch_ctrl_s_saves and legacy_unreachable for the legacy notice test (their red came from those tests); these pin details
- red panic_hook_writes_recovery `left: 0 right: 1` (no files written), red timestamp_is_utc_calendar_time `left: "" right: "19700101T000000Z"` → green after publish/write_recovery/utc_stamp
- red (UI, stub render): mouse_click_drag_and_wheel `left: 0 right: 17`; render_* snapshot tests failed in insta (no snapshot / blank frame) → rendering implemented (editor_view, tabs, status_bar, notice lines, mouse); reviewed the pending .snap.new frames by eye, fixed two defects found there (read-only notice wording; horizontal scroll did not re-fit at 40 cols → cursor off-screen, caught by the cursor assertion), then accepted once with INSTA_UPDATE=always (12 snapshot files)
- release timing (run-heavy): render_large_file_fast ok — 100k lines median 0.50 ms, 100 MB median 0.51 ms (worst of top/middle/end)
- pty smoke found the first frame drawn only after the 2 s kitty keyboard query in a non-answering pty → first frame now drawn before the query (report_config / report_keyboard split); bench-startup first_frame_ms=24
- pty smoke (debug binary, 80x24 pty): open smoke.txt, type "Xin chào ", ctrl+s, ctrl+q → exit 0, file 'Xin chào hello\n', title/alt-screen/status bar/tab seen
- capabilities.md (vr-ide lines) and decisions.md (9 `D? (T5)` lines) updated; scratch splice files removed; guard green

## Summary
- Files changed: crates/vr-ide/src/main.rs (rewritten: terminal setup only); new: crates/vr-ide/src/{lib,app,app_tests,event_loop,clipboard,recovery,theme}.rs, commands/{mod,registry,app,file,edit,tab}.rs, ui/{mod,layout,editor_view,tabs,status_bar}.rs, ui/snapshots/*.snap (12). Team files: capabilities.md, decisions.md, this notes file, qa-artifacts/T5/{pty_smoke.py, release_checks.sh}.
- Not touched: vr-core, vr-config, vr-fs, vr-search, any Cargo.toml, cli.rs.
- Tests added (39 incl. 1 ignored): app_tests (19), ui::tests (8 incl. render_large_file_fast #[ignore]), event_loop (4), clipboard (2), registry (2), recovery (2).

### App API and command registry for T6/T8/T10
- `App` fields: `root, docs: Vec<Document>, views: Vec<View{top,left,follow_cursor}>, active, focus: Focus{Editor}, notices: VecDeque<Notice{level,text,expires}>, keymap, settings, clipboard: Box<dyn Clipboard>, fs_rx, keyboard_enhanced, quit, dirty, layout: AppLayout{tabs,editor,notice,status}`.
- `App` methods: `new(root, keymap, settings, clipboard)`, `load(root, clipboard) -> (App, warnings)`, `report_config/report_keyboard/report_startup`, `start_watcher`, `open_path(&Path)` (re-open activates), `close_doc(i)`, `set_active(i)`, `active_doc[_mut]`, `active_view_mut`, `editable_doc` (read-only guard), `cursor_moved`, `active_name`, `display_path`, `notify/info/warn/error`, `tick(now)`, `key_hint(cmd)`, `confirm_twice(cmd)`, `run_command(name, CmdArgs) -> bool`, `handle_key(KeyEvent)`, `handle_paste(&str)`, `handle_mouse(MouseEvent)`, `handle_fs_event(FsEvent)`, `drain_fs_events`, `title`.
- Key dispatch: release events ignored → Esc dismisses a notice → built-in editor keys (`commands::edit::handle_builtin_key`) → `chord_from_key` → `keymap.command_for_in(focus.context(), chord)` → `run_command`. T6/T8/T10 add `Focus` variants (and their `Context`) and route keys for their panels before the built-in editor keys.
- Registry: `CommandSpec { name, title, run: fn(&mut App, CmdArgs) -> CmdResult }`, `CmdArgs::{None, Path(PathBuf)}` (add variants as needed), `CmdResult = anyhow::Result<()>` (Err → error notice "<title>: <msg>"), `all_commands()`, `find(name)`, `command_names()`. Each group file exports `COMMANDS`; add the group to `registry::GROUPS` and delete its names from `registry::PENDING`. Additive name: `file.save_overwrite`.
- T6 replaces: `app.palette` stub, `file.save_as` without path, `file.open` without path, the "press again" confirmations in `app.quit`/`file.close` (`confirm_twice`), and the ChangedOnDisk prompt (currently tab `!` + notice + save refusal).

### Commands run (final, trimmed)
```
$ cargo test -p vr-ide
test result: ok. 38 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.81s   (lib)
test result: ok. 0 passed ...  (main.rs, doc-tests)
$ run-heavy.sh cargo test -p vr-ide --release render_large_file_fast -- --ignored --nocapture
render_large_file_fast 100k lines: median frame (worst position) 503.191µs
render_large_file_fast 100 MB: median frame (worst position) 506.028µs
test ui::tests::render_large_file_fast ... ok
$ cargo clippy -p vr-ide --all-targets -- -D warnings      → Finished (only cargo's manifest note: vr-fs/vr-search unused deps, pre-existing, not a lint)
$ guard: cargo fmt --all --check → exit 0; cargo clippy --workspace --all-targets -- -D warnings → Finished, no warnings
$ run-heavy.sh qa-artifacts/T5/release_checks.sh
first_frame_ms=24  (target < 100, includes pty setup)
max_rss_mb=3  (target < 60)
big.txt bytes: 104857600
first_frame_with_100MB_file_ms=152
max_rss_mb_100MB=218
$ python3 -I qa-artifacts/T5/pty_smoke.py target/debug/vr-ide <tmp>
sent type / sent ctrl+s / sent ctrl+q
exit status: 0
file now: 'Xin chào hello\n'
title set: True ; alt screen entered/left: True True ; status bar seen: True | tab seen: True
```

### Deviations from the plan (with reason)
- `src/lib.rs` and `src/app_tests.rs`, `commands/mod.rs`, `ui/mod.rs` added beyond the listed files (module roots; App API as a library).
- Notices render on their own rows above the status bar instead of inside it (readability at 40 cols); the status bar keeps `file · Ln x, Col y · EOL · encoding` (file left, rest right-aligned, file shortened from the left with `…`).
- `report_startup` split into `report_config` + `report_keyboard` (first frame before the keyboard query); `report_startup` kept as the combined call.
- Pending plan commands are registered as stubs (see decisions); `file.save_overwrite` added.

### Measured / known limits (for review)
- Opening a 100 MB file blocks the UI thread ~150 ms (read + rope build, release) and takes ~218 MB RSS; frames afterwards ~0.5 ms. Moving the load to a thread is left for a later task.
- A very long single line is walked from its start each frame (horizontal scroll needs the column); minified multi-MB lines will be slower than 5 ms.
- Recovery publishes the modified buffers after each drawn frame (cheap rope clones); edits after the last frame before a panic are not in the recovery file. The hook uses `try_lock` so it cannot deadlock.
- OSC 52 has no size cap here; some terminals ignore very large sequences. Inside tmux OSC 52 needs `set-clipboard on`.
- In a bare pty that never answers the kitty keyboard query, crossterm waits 2 s after the first frame (keys typed meanwhile are queued, not lost).
- Saving goes only through `Document::save` / `save_overwrite` / `save_as`; recovery writes only under `state_dir()/recovery`. Save and recovery are driven through the App API with real temp files (`dispatch_ctrl_s_saves`, `external_modify_modified_doc_keeps_buffer_and_marks_tab`, `panic_hook_writes_recovery`), ready for racing-writer tests once T12's `RacingWriter` lands.
- Out of scope noticed: cargo reports `vr-fs`/`vr-search` as unused dependencies of vr-ide until T8/T10 use them.

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| grapheme stepping, display width | capabilities.md (vr-core text.rs), ratatui `Span::styled_graphemes` | reused `vr_core::text::next_grapheme_boundary` + `Span::width` (ratatui's styled_graphemes drops tabs/control graphemes, unusable) |
| key chord type, keymap lookup, validation | capabilities.md (vr-config) | reused `KeyChord::new`, `Keymap::command_for_in`, `validate`, `keys_for`, `bindings` |
| save / reload / disk check / own-write suppression | capabilities.md (vr-core document.rs, watch.rs) | reused `Document::{save, save_overwrite, save_as, reload, check_disk, set_own_writes}`, `Watcher::{new, watch, unwatch, own_writes}` |
| line edit ops, motions | capabilities.md (vr-core edit_ops.rs, motion.rs) | reused all |
| char→Pos for status | vr-core text.rs | reused `char_to_pos` |
| base64 for OSC 52 | workspace deps (no base64 crate) | wrote 20-line `clipboard::base64` (BASE-8: no new dependency) |
| launch root | cli.rs | reused `cli::resolve` unchanged |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | `git status`: only crates/vr-ide/src/** (+ team files); vr-core/vr-config/vr-fs/vr-search/Cargo.toml untouched |
| BASE-2 | pass | registry.rs `CommandSpec{name,title,run}`, `all_commands() -> &'static [CommandSpec]`; plan command names pinned by `registry_has_every_plan_command_once`; additive helpers listed under "App API" |
| BASE-3 | pass | red lines per test in Progress above; tests that passed first time listed with reason (pin guards whose red came from an earlier test) |
| BASE-4 | pass | "Commands run (final, trimmed)" above |
| BASE-5 | pass | release test + release build/bench via run-heavy.sh; smoke process exited (exit status 0), `ps` shows no vr-ide left |
| BASE-6 | pass | English code/comments; comments explain why (e.g. main.rs first-frame-before-query, editor_view horizontal follow) |
| BASE-8 | pass | no manifest change; base64 hand-written instead of a new crate |
| BASE-9 | pass | user files written only via `Document::save*`; recovery files `create_new` 0600 in a 0700 dir under state_dir (`panic_hook_writes_recovery` checks mode and that the project file is unchanged); paths from CmdArgs joined to root, errors become notices |
| BASE-10 | pass | only `cargo test -p vr-ide` (+ the ignored release test) |
| BASE-11 | pass | no stash/checkout/restore/reset run |
| BASE-14 | pass | release runs as background tasks (b0odwza2j, bkz10481q) then foreground via run-heavy; no wait loops |
| BASE-15 | pass | both background tasks finished on their own; nothing stopped, nothing killed |
| BASE-16 | pass | `## Reuse` table; capabilities.md vr-ide lines added |
| BASE-17 | pass | no installs |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | this file created at start; progress line per step |
| BASE-23 | pass | save reachable by key and by command: `dispatch_ctrl_s_saves` (ctrl+s) + `typing_marks_modified_and_save_clears` (run_command); paste by key and bracketed paste: `copy_paste_uses_clipboard`, `bracketed_paste_is_one_undo_step`; external change by fed event and by real watcher |
| BASE-24 | pass | no git add/commit/reset |
| BASE-25 | pass | `cargo fmt --all --check` exit 0; `cargo clippy --workspace --all-targets -- -D warnings` Finished |
| BASE-32 | pass | D5 root via cli::resolve; D10/USER-1 keys only from keymap (`key_hint`, welcome screen reads keymap); D27 BufferOptions from settings (`App::buffer_options`); D31/D38 line breaks kept (CRLF recovery test); D42 reload one undo step (`save_as_with_path_and_reload`); D46/D47 `Saved.other_hard_links` notice, own_writes wired (`..._with_real_watcher` own-save echo check) |
| BASE-35 | pass | USER-1: no binding hard-coded; built-in keys are only the plan's cursor/typing list (edit.rs `handle_builtin_key`); notices name keys via `keymap.keys_for` |
| BASE-38 | pass | `rustfmt --edition 2024` on my files only |
| BASE-46 | pass | smoke, bench and release timing ran on the final main.rs (binary mtimes 12:37 > main.rs 12:36:50) |
| COST-1 | pass | heavy runs through run-heavy.sh |
| COST-3 | pass | no sleep/pgrep loops (the smoke script's select() pump is the probe itself, bounded) |
| COST-5 | pass | smoke pty child exited; no servers/watchers left |
| COST-8 | pass | new files written whole; splices batched |
| COST-9 | pass | nothing left running |
| COST-10 | pass | related set = all vr-ide tests (one crate); snapshots live beside ui/mod.rs |
| COST-13 | pass | scripts in qa-artifacts/T5/ |
| COST-15 | pass | snapshots accepted with INSTA_UPDATE=always after rendering was final; one re-accept after the horizontal-scroll fix (found by reading the pending frames, not by regenerating blindly) |
| COST-16 | pass | runs filtered to summary lines |
| AC-1 | pass | `title_shows_project_root`; main.rs `SetTitle(app.title())`; smoke "title set: True"; welcome + tab bar show root (`render_empty_project_*`) |
| AC-3 | pass | `typing_marks_modified_and_save_clears`, `undo_redo_via_commands`, `render_tabs_modified_*` (`●`) |
| AC-4 | pass | `copy_paste_uses_clipboard` (fake clipboard incl. text set by another app), `cut_removes_selection`, `osc52_fallback_writes_sequence_and_pastes_last_copy`; `clipboard::system()` picks arboard with a display, OSC 52 without |
| AC-5 | pass | `dispatch_ctrl_s_saves`, `unknown_command_in_keymap_shows_notice_not_crash`, `default_keymap_validates_cleanly`, `legacy_terminal_notice_lists_unreachable_default_keys` |
| AC-7 | pass | `external_modify_unmodified_doc_reloads`, `external_modify_reloads_within_one_second_with_real_watcher` (< 1 s asserted), `external_modify_modified_doc_keeps_buffer_and_marks_tab` (buffer kept, tab `!`, save refused), `external_delete_keeps_buffer_marked_deleted` |
| AC-18 | pass | `render_large_file_fast` release: 0.50 ms (100k lines), 0.51 ms (100 MB) < 5 ms; 100 MB first frame 152 ms in a pty |
| USER-1 | pass | see BASE-35 |

n-a: BASE-7 (no commit) · BASE-12, BASE-13 (nothing outside repo, no untracked user files deleted) · BASE-20 (nothing removed besides main.rs's old placeholder loop, replaced) · BASE-21 (no secrets in output) · BASE-22 (no new env var, config key or other named setting) · BASE-26 (not a rewrite) · BASE-27, BASE-30, BASE-31, BASE-39, BASE-41, BASE-43, BASE-44 (no shared DB, containers, schema, credentials, access rules, fakes of other systems) · BASE-28, BASE-29, BASE-33, BASE-34, BASE-36, BASE-40 (no third-party code, browser, external system, outward writes, app scope line) · BASE-37 (not a defect fix) · BASE-42 (no test runner settings changed) · BASE-45 (AC text read from spec, unchanged) · BASE-47 (no fixed value) · COST-2 (developer role only) · COST-4 (no full suite) · COST-6, COST-7 (no web UI) · COST-11 (no e2e) · COST-12 (leader rule) · COST-14 (no CI) · COST-17 (no merge) · COST-18 (no full run) · COST-19, COST-20 (qc check sets)

## Fix round 1
### Progress
- fresh developer: read brief-common, notes, review T5, QC T5
- red (7 new app tests, run together before any fix; stubs only: `handle_key_at` = `handle_key`, `recovery_dir` field):
  - held_quit_or_close_key_never_confirms: `a Repeat event confirmed quit` (app_tests.rs:427)
  - escape_or_time_cancels_a_pending_confirmation: `Esc did not cancel the pending quit`
  - discarded_buffers_are_kept_in_recovery: unwrap on read_dir NotFound (no recovery written)
  - paste_uses_the_document_line_ending: `left: "a\rb\r\nc\nx\n" right: "a\nb\nc\nx\n"`
  - changed_on_disk_notice_names_the_overwrite_key: notice still says "palette"
  - dotdot_path_opens_the_same_tab: `left: 2 right: 1`
  - legacy_keyboard_notice_does_not_hide_later_notices: front notice is the keyboard warning, not "a.txt changed on disk; not saved"
  - rescan_renamed_and_error_events passed first run (M7 handlers existed untested) — red by mutation, see BASE-3 below
- red (recovery/event_loop, before their code):
  - long_accented_path_name_fits_the_file_name_limit: with the old char-count naming `left: 1 right: 2` (ENAMETOOLONG, buffer skipped)
  - refused_name_falls_back_to_a_short_one: stub `left: 0 right: 1`
  - panic_hook_acts_only_for_the_main_thread: hook acted for a worker thread `left: (2, 2) right: (0, 0)`
  - edits_are_published_before_a_frame_is_drawn: draw-then-publish `left: 0 right: 1` ("the last edits are not in recovery")
  - loop_error_writes_unsaved_buffers_to_recovery: stub finish `left: 0 right: 1`
  - quit_and_close_with_unsaved_need_a_second_press failed after the M1 change (two presses 0 ms apart now count as auto-repeat) → rewritten with timed presses (handle_key_at)
- green: M1 (confirm_twice with repeat gap / TTL / Esc cancel, keep_discarded, REPORT_EVENT_TYPES in main.rs), M2 (with_line_ending), M4 (byte budget + untitled fallback + snapshot retry), M5 (draw_frame publishes first), main-thread hook, finish() on run Err, notice key for file.save_overwrite, App::resolve, until_next keyboard notice → `cargo test -p vr-ide` 51 passed, 1 ignored
- red M3: long_line_keystroke_to_frame_fast (release) `end #0: 3.386553396s` (col_of/walk_line from the line start) → green after long-line mode: worst median 100.187µs; status_bar_keeps_the_file_name_at_40_columns red `40 cols: "…ed on disk] · Ln 1, Col 2 · LF · UTF-8 "` → green; columns_match_vr_core passed first run (the duplicates already agreed with vr-core, control chars included) → duplicates col_of/char_at_col deleted, editor uses vr_core::display_col / char_at_display_col, test now pins renderer cells == vr-core columns
- BASE-3 mutation check (`qa-artifacts/T5/mutations.sh`, tree md5 identical before/after: `TREE-RESTORED`): every listed test goes red under a mutation of the code it covers — see table below
- snapshots: render_tabs_modified_40x12 and render_notice_and_read_only_status_40x12 changed (status bar only), read by eye, accepted once with INSTA_UPDATE=always after the code was final; the other 10 unchanged
- pty smoke on the debug binary (legacy pty, REPORT_EVENT_TYPES path untouched there): type, ctrl+s, ctrl+q → exit 0, file 'Xin chào hello\n'
- decisions.md: 8 `D? (T5 r1)` lines; capabilities.md vr-ide lines updated

### Per finding
| Finding | Reply | Fix | Test (red → green) |
|---|---|---|---|
| M1 D57 held key / stale confirm | fixed | `App::confirm_twice(cmd, warning)`: Repeat events never confirm; a press within `CONFIRM_REPEAT_GAP` (700 ms) of the previous press of that key only refreshes it; `CONFIRM_TTL` 30 s; Esc / any other input → `cancel_confirm` (also removes the warning); `keep_discarded` writes discarded buffers to `state_dir()/recovery` (quit and close); main.rs pushes `REPORT_EVENT_TYPES` | `held_quit_or_close_key_never_confirms` (red: "a Repeat event confirmed quit"), `escape_or_time_cancels_a_pending_confirmation` (red: "Esc did not cancel the pending quit"), `discarded_buffers_are_kept_in_recovery` (red: read_dir NotFound); `quit_and_close_with_unsaved_need_a_second_press` updated to timed presses |
| M2 paste line ending | fixed | `edit::with_line_ending` in `insert_pasted` (bracketed paste and edit.paste) | `paste_uses_the_document_line_ending` (red `left: "a\rb\r\nc\nx\n" right: "a\nb\nc\nx\n"`; LF doc + CRLF doc via ctrl+v) |
| M3 long line (AC-18) | fixed | `editor_view`: lines > `LONG_LINE` (16384 chars) scroll by char offset, cells counted from `left` (`walk_from`, `follow_long` walks ≤ one screen, `char_at_screen_col`); status Col = char offset there | release `long_line_keystroke_to_frame_fast` (10 MB line; end/type/left/right/backspace/home): red `end #0: 3.386553396s` → worst median 100.187µs; debug `long_line_follow_and_hit_test` |
| M4 recovery name / every buffer | fixed | `sanitize` keeps ≤ 200 bytes at a char boundary; non-AlreadyExists error → `<stamp>-untitled` fallback; `snapshot` retries try_lock (≤ 100 ms) | `long_accented_path_name_fits_the_file_name_limit` (red with the old naming `left: 1 right: 2`), `refused_name_falls_back_to_a_short_one` (red `left: 0 right: 1`) |
| M5 publish before draw | fixed | `event_loop::draw_frame` publishes, then draws | `edits_are_published_before_a_frame_is_drawn` (red `left: 0 right: 1`) |
| M6 file name at 40 cols | fixed | status bar picks the richest (right part, flags) that leaves room for the file name; path shortened from the left first; snapshots updated (`" third.md [changed] · Ln 1, Col 2"`, `" l1.txt [read-only] · Ln 1, Col 1"`) | `status_bar_keeps_the_file_name_at_40_columns` (red `40 cols: "…ed on disk] · Ln 1, Col 2 · LF · UTF-8 "`; 30/40/80 cols, deep path) |
| M7 Rescan / Renamed / Error | fixed (tests) | handlers were already right; now pinned | `rescan_renamed_and_error_events` passed first run; red by 3 mutations: Rescan filter `left: "a1\n" right: "a2\n"`, Renamed without `to` `left: "a2\n" right: "a3\n"`, Error ignored → assertion at app_tests.rs:539 |
| minor: overwrite key in notice | fixed | notice names `keymap.keys_for("file.save_overwrite")`, or "bind file.save_overwrite to a key in keymap.toml" when unbound; no palette mention | `changed_on_disk_notice_names_the_overwrite_key` (red: notice said "palette") |
| minor: BASE-3 ×9 | fixed (evidence) | mutation check | see BASE-3 table |
| minor: USER-1 Esc | fixed (documented) | `app::DISMISS_KEY` constant, built in; decision line added to the built-in list | — (no behaviour change) |
| minor: BASE-16 col_of / char_at_col | fixed | duplicates deleted; editor uses `vr_core::display_col` / `vr_core::text::char_at_display_col`; renderer widths = vr-core's | `columns_match_vr_core` (passed first run: the duplicates agreed, control chars included — now pins renderer cells == vr-core columns) |
| minor: `sub/../a.txt` | fixed | `App::resolve` lexical normalisation in open_path and save_as | `dotdot_path_opens_the_same_tab` (red `left: 2 right: 1`) |
| uncertain: hook threads / run Err | fixed | hook acts only on the thread that installed it (main); `event_loop::finish` writes modified buffers to recovery on Err and names the files in the error (main.rs) | `panic_hook_acts_only_for_the_main_thread` (red `left: (2, 2) right: (0, 0)`), `loop_error_writes_unsaved_buffers_to_recovery` (red `left: 0 right: 1`) |
| QC minor: keyboard notice hides later ones | fixed | that notice is `until_next` (dropped when another notice arrives) and expires after 15 s | `legacy_keyboard_notice_does_not_hide_later_notices` (red: front notice was the keyboard warning) |
| QC concurrent writer (D25) | not mine | vr-core `Document::save`, T4 escalation; vr-ide still calls `Document::save` only | — |

### BASE-3 mutation table (`qa-artifacts/T5/mutations.sh`)
| Test | Mutation | Red line |
|---|---|---|
| ctrl_s_and_ctrl_q_map_to_their_chords | ctrl read from ALT | `left: "s" right: "ctrl+s"` |
| legacy_control_bytes_map_back_to_their_keys | `'5' => '5'` | `left: "ctrl+5" right: "ctrl+]"` |
| shifted_keys_use_the_unshifted_name | no unshift | `left: "ctrl+shift+!" right: "ctrl+shift+1"` |
| legacy_unreachable_chords | `Enter => m.shift` | assertion `u("ctrl+alt+enter")…` failed |
| registry_has_every_plan_command_once | focus.cycle renamed | `focus.cycle left: 0 right: 1` |
| default_keymap_validates_cleanly | same | `left: [UnknownCommand { key: "f6", command: "focus.cycle" }] right: []` |
| read_only_doc_refuses_edits | guard disabled | `left: "xpastecafé\n" right: "café\n"` |
| pending_and_palette_commands_show_a_notice | palette text empty | assertion `contains("palette")` failed |
| title_shows_project_root | title without root | assertion `title().contains(root)` failed |

### Commands run (round 1, final, trimmed)
```
$ cargo test -p vr-ide
test result: ok. 54 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.82s   (lib)
test result: ok. 0 passed ... (main.rs, doc-tests)
$ run-heavy.sh cargo test -p vr-ide --release --lib -- --ignored --nocapture
long_line_keystroke_to_frame_fast 10 MB line: worst median 100.187µs (end)
render_large_file_fast 100k lines: median frame (worst position) 517.106µs
render_large_file_fast 100 MB: median frame (worst position) 520.968µs
test result: ok. 2 passed; 0 failed
$ cargo clippy -p vr-ide --all-targets -- -D warnings      → Finished (only the manifest unused-dep note for vr-fs/vr-search)
$ guard: cargo fmt --all --check → exit 0; cargo clippy --workspace --all-targets -- -D warnings → Finished, no warnings
$ python3 -I qa-artifacts/T5/pty_smoke.py <abs>/target/debug/vr-ide <tmp>  → exit status: 0, file 'Xin chào hello\n', title/alt screen/status bar/tab seen
```

### Files changed in round 1
crates/vr-ide/src/{app.rs, app_tests.rs, event_loop.rs, recovery.rs, main.rs}, commands/{app.rs, file.rs, edit.rs}, ui/{mod.rs, editor_view.rs, status_bar.rs}, ui/snapshots/{render_tabs_modified_40x12, render_notice_and_read_only_status_40x12}.snap. Team files: decisions.md, capabilities.md, qa-artifacts/T5/mutations.sh, this file.

### Reuse (round 1)
| Needed | Looked in | Result |
|---|---|---|
| display columns / column → char | vr-core text.rs | reused `vr_core::display_col`, `text::char_at_display_col`; deleted editor_view duplicates |
| grapheme stepping back for long-line scroll | vr-core text.rs | reused `prev_grapheme_boundary` / `next_grapheme_boundary` |
| recovery writer for discards and loop errors | recovery.rs | extended (`write_docs`, `write_entries`) instead of a second writer |
| line ending of the document | vr-core `LineEnding::as_str` | reused |

### Rule compliance (round 1 — rows this round touched; the rest as in the table above)
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | `git status`: only crates/vr-ide/src/** + team files |
| BASE-2 | pass | plan contracts unchanged; additive helpers listed in capabilities.md (`confirm_twice` now takes the warning text — internal API, T6 replaces it) |
| BASE-3 | pass | red line per new test above; first-run passes (rescan_renamed_and_error_events, columns_match_vr_core) and the 9 listed tests proven by mutation |
| BASE-4 | pass | "Commands run (round 1)" |
| BASE-5 | pass | release tests via run-heavy.sh; smoke child exited 0 |
| BASE-6 | pass | comments explain why (repeat gap, publish-before-draw, long-line cost) |
| BASE-8 | pass | no manifest change |
| BASE-9 | pass | user files written only through `Document::save*`; recovery writes only under state_dir()/recovery (0700/0600, create_new); D25 window left to T4 as instructed |
| BASE-10 | pass | `cargo test -p vr-ide` only |
| BASE-11 | pass | no stash/checkout/restore/reset; mutations reverted by script, md5 equal |
| BASE-14 | pass | no waits/polls; release run finished in foreground (< 1 min) |
| BASE-15 | pass | nothing started in background; nothing killed |
| BASE-16 | pass | Reuse table; capabilities.md updated |
| BASE-19 | pass | progress lines in this section |
| BASE-24 | pass | no git index commands |
| BASE-25 | pass | fmt check exit 0, workspace clippy clean |
| BASE-32 | pass | D57 refined, D59 amended for long lines (decisions.md `D? (T5 r1)`); D64 left to T6 |
| BASE-38 | pass | `rustfmt --edition 2024` on the 11 changed files only |
| COST-1 / COST-3 / COST-9 | pass | heavy via run-heavy; no polling; nothing left running (`ps` shows no vr-ide/cargo) |
| COST-15 | pass | snapshots accepted once after the code was final |
| AC-3 | pass | typing/undo/save tests green; paste normalised |
| AC-7 | pass | `rescan_renamed_and_error_events`, notice key test |
| AC-18 | pass | 10 MB line keystroke→frame 0.1 ms; 100 MB file 0.52 ms frames |
| D57 | pass | three M1 tests + recovery on discard |
| USER-1 | pass | keys in notices from keymap (`keys_for`); Esc built in, documented (D? T5 r1) |
n-a: as in the first table (unchanged by this round).
