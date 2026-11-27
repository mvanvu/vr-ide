# T6 Command palette, dialogs, go-to-line, external-change prompt, quit guard (M1 UI part 2)

## Progress
- read brief-common, plan T6, T5 notes + review re-check minors, decisions D46-D72, capabilities, vr-core Document API, T4r3 worktree API shape (`disk_version` / `save_overwrite_seen(&DiskVersion)`)
- nucleo-matcher: in the workspace manifest, NOT in crates/vr-ide/Cargo.toml (manifest frozen, out of my file scope) -> reported; palette ranks through `vr_fs::FileIndex::query` (nucleo, already a vr-ide dependency) behind one adapter fn `commands::palette::rank`
- skeleton ui/dialog.rs (types, stub bodies)
- red line_input_edits_at_the_cursor `left: ("", 0) right: ("ab", 2)` → green (LineInput, grapheme steps via vr_core::text)
- red confirm_dialog_keys_and_mouse `wraps around left: Pending right: Chosen(0)` → green (ConfirmDialog keys/mouse, render, wrap, button rows; theme popup styles)
- red prompt_dialog_submits_and_cancels `left: Pending right: Submitted("12:40")` → green (PromptDialog keys)
- app.rs: confirm_twice machinery removed (dialog replaces it), Dialog/palette/exit_messages fields, modal routing; skeleton commands/{dialog,nav,palette}.rs, ui/palette.rs; T5 press-twice tests removed (superseded), F1-notice and save-overwrite-notice parts of two T5 tests removed (now dialog/palette)
- red palette_filters_by_fuzzy_name_and_shows_keys `left: Editor right: Palette` → green (commands/palette.rs open/refilter/rank/keys, registry::is_pending)
- palette_enter_runs_command / palette_escape_closes: written after accept/Esc existed → red by mutation (Esc/Enter arms emptied): both `assertion failed: app.palette.is_none()` → restored → green
- coordinator note (D64 API `disk_version()` / `save_overwrite_seen(&DiskVersion)`): adapter mirrors those names; seen version taken when the prompt opens, before anything else is read
- red goto_line_moves_cursor_and_clamps `assertion failed: matches!(.. Some(DialogAction::GotoLine))` → green (nav.goto_line prompt, nav::goto, dialog::answer + reopen_with_error)
- red quit_with_unsaved_asks `a dialog` (no dialog opened) → green (app.quit dialog Save all/Discard/Cancel, answer_quit, file::save_doc/SaveOutcome, keep_discarded -> Result, exit_messages printed by main.rs after restore_terminal)
- red close_with_unsaved_asks `a dialog` → green (file.close dialog Save/Discard/Cancel, answer_close, discard_and_close keeps the tab when recovery fails)
- red save_as_prompt `starts from the current name left: None`, open_file_prompt (unwrap on None: no prompt) → green (file.save_as / file.open prompts, answer_save_as with Replace/Cancel confirm, save_as_doc, App::rewatch); one test expectation fixed (cursor after "x": "xyone")
- red modified_doc_changed_on_disk_prompts `assertion failed: is_external(&app, false)`, keep_mine_replaces_only_the_version_shown `asks again about the newer version`, deleted_on_disk_marks_tab `a dialog` → green (recheck opens prompts for modified docs; file::{disk_version, save_overwrite_seen (interim adapter), answer_external_change, keep_mine, reload_doc, prompt_deleted, answer_deleted}); full lib 63 passed
- red render_palette / render_confirm_dialog: insta "snapshot assertion for render_*_80x24 failed" (blank palette render) → ui/palette.rs written; pending frames read by eye at 80x24 and 40x12, accepted once with INSTA_UPDATE=always (4 new .snap, T5 snapshots unchanged)
- palette_and_dialog_answer_the_mouse (App::handle_mouse → palette rows / dialog buttons): written after the routing → red by mutation (mouse routing removed in app.rs) `assertion failed: app.palette.is_none()` → restored → green
- capabilities.md (vr-ide dialog/palette/file/nav lines, app.rs line updated) and decisions.md (8 `D? (T6)` lines) updated; stale T6 comments fixed
- pty smoke (debug binary, 80x24): palette → save, external prompt → Keep mine on save, quit → Discard → stderr recovery path; all checks true, exit 0
- guard green; final run below

## Summary
- Files changed: crates/vr-ide/src/{app.rs, app_tests.rs, lib.rs, main.rs, theme.rs}, commands/{app.rs, file.rs, mod.rs, registry.rs}, ui/{mod.rs, editor_view.rs}. New: commands/{dialog.rs, nav.rs, palette.rs}, ui/{dialog.rs, palette.rs}, dialog_tests.rs, ui/snapshots/vr_ide__ui__tests__render_{palette,confirm_dialog}_{80x24,40x12}.snap. Team files: this notes file, decisions.md, capabilities.md, qa-artifacts/T6/pty_smoke.py.
- Not touched: vr-core (T4 escalation worktree), vr-config, vr-fs, vr-search, any Cargo.toml.
- Tests added (13): ui::dialog::tests {line_input_edits_at_the_cursor, confirm_dialog_keys_and_mouse, prompt_dialog_submits_and_cancels}; dialog_tests {palette_filters_by_fuzzy_name_and_shows_keys, palette_enter_runs_command, palette_escape_closes, goto_line_moves_cursor_and_clamps, quit_with_unsaved_asks, close_with_unsaved_asks, save_as_prompt, open_file_prompt, modified_doc_changed_on_disk_prompts, keep_mine_replaces_only_the_version_shown, deleted_on_disk_marks_tab}; ui::tests {render_palette, render_confirm_dialog, palette_and_dialog_answer_the_mouse}. (16 counting the ui ones.)
- Tests removed (superseded by the dialog, D57/D65 "press twice" gone): quit_and_close_with_unsaved_need_a_second_press, held_quit_or_close_key_never_confirms, escape_or_time_cancels_a_pending_confirmation, discarded_buffers_are_kept_in_recovery (covered by quit/close dialog tests), changed_on_disk_notice_names_the_overwrite_key (the notice is now the prompt). Trimmed: the save/overwrite tail of external_modify_modified_doc_keeps_buffer_and_marks_tab, the no-path tail of save_as_with_path_and_reload, the F1 part of pending_and_palette_commands_show_a_notice (renamed pending_commands_show_a_notice).

### APIs for T8 / T10
- Dialogs: `App::open_dialog(DialogWidget, DialogAction)`; widgets `ui::dialog::{ConfirmDialog::new(title, msg, &[buttons]) (+ .selected default), PromptDialog::new(title, label, initial) (+ .error), LineInput, DialogWidget, Outcome, wrap}`; add a `DialogAction` variant and an arm in `commands::dialog::answer`; `reopen_with_error` keeps a prompt open with a message; `DocRef::{of, find}` names a tab across closes. Input routing is modal (dialog takes every key/mouse/paste).
- Palette: `commands::palette::{Palette, PaletteItem, open, close, refilter, accept, handle_key, handle_mouse, paste, rank}`, `ui::palette::render`; `Focus::Palette` ↔ `Context::Palette`. T8 go-to-file can reuse `LineInput` + the list drawing (generalise `PaletteItem` source) and `vr_fs::FileIndex::query` directly.
- File flows: `commands::file::{save_doc -> SaveOutcome, save_as_doc, discard_and_close, prompt_external_change, prompt_deleted, reload_doc}`; T8 rename of an open file can call `App::rewatch`.

### D64 adapter (follow-up)
- `commands::file::{disk_version, save_overwrite_seen}` — `wire to vr-core save_overwrite_seen when T4 escalation merges`. Seen version (stamp + content hash) is taken when the prompt opens, before anything else is read (coordinator note). Keep mine re-reads it right before `Document::save_overwrite` and refuses (re-prompt, "changed again") when it differs: `keep_mine_replaces_only_the_version_shown` covers a newer write and a delete while the prompt is open.
- Known limit until the swap: a write landing between that re-check and the rename inside `save_overwrite` is still replaced (the window vr-core's `save_overwrite_seen` closes inside the atomic replace). No racing-writer stress test for this path yet: it would fail by design on the interim adapter; it belongs with the swap (T12 `RacingWriter`). Per the severity anchor this window is a blocker class — flagged here for the leader.

### Commands run (final, trimmed)
```
$ cargo test -p vr-ide
test result: ok. 66 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.82s   (lib)
test result: ok. 0 passed ... (main.rs, doc-tests)
$ cargo clippy -p vr-ide --all-targets -- -D warnings   → Finished (cargo manifest note only: vr-search unused dependency until T10)
$ guard: cargo fmt --all --check → exit 0; cargo clippy --workspace --all-targets -- -D warnings → Finished, no warnings
$ python3 -I qa-artifacts/T6/pty_smoke.py <abs>/target/debug/vr-ide <tmp>
palette shown: True
1. saved via palette: 'Ahello\n'
2. external prompt shown: True
3. save prompt shown: True | file still: 'agent\n'
4. after Keep mine: 'ABhello\n'
5. quit dialog shown: True
exit status: 0
stderr after restore: vr-ide: unsaved changes were kept in the recovery folder: <tmp>/state/recovery/20261124T061749Z-...
file at exit: 'ABhello\n'
recovery files: ['ABChello\n']
```
(tmp work dir removed; `ps` shows no vr-ide process.)

### Deviations from the plan (with reason)
- nucleo-matcher is not in crates/vr-ide/Cargo.toml (manifest frozen, outside my file scope): ranking goes through `vr_fs::FileIndex` (nucleo) in `commands::palette::rank`. Adding `nucleo-matcher.workspace = true` to vr-ide would let `rank` call nucleo directly (one function to change) — leader's call.
- Compare button skipped (needs a diff view; M7 has one) — D? (T6).
- New files beyond the plan list: commands/dialog.rs (DialogAction + answer), dialog_tests.rs.
- `App::handle_key_at` and the confirm_twice machinery removed (only the press-twice flow used them; BASE-20 grep: app_tests only).

### Out of scope noticed
- `Document::save_as` writes with no freshness expectation; the Replace confirmation is the only guard for an existing target (same window class as above; vr-core).

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| fuzzy ranking | capabilities.md (vr-fs file_index), workspace manifest | reused `vr_fs::FileIndex::from_paths` + `query` (nucleo, smart case, char hits) |
| grapheme steps in the input | vr-core text.rs | reused `prev/next_grapheme_boundary` |
| clamp line/col | vr-core text.rs | reused `pos_to_char`, `char_to_pos` |
| recovery write on discard | app.rs `keep_discarded`, recovery.rs | extended `keep_discarded` to return Result instead of a second writer |
| keys shown in palette | vr-config keymap | reused `Keymap::keys_for` |
| disk stamp | vr-core document.rs | reused `DiskStamp::of`; content hash added in the adapter only |
| popup widgets | ratatui | `Block`, `Clear`; no existing dialog code in the repo |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | `git status`: only crates/vr-ide/src/** + team files |
| BASE-2 | pass | registry contract unchanged (`CommandSpec`, `CmdArgs`); additive APIs listed above; D64 adapter mirrors vr-core's `disk_version` / `save_overwrite_seen` names |
| BASE-3 | pass | red line per test in Progress; palette_enter/escape and the mouse test red by mutation (recorded); one test expectation corrected (save_as_prompt) |
| BASE-4 | pass | "Commands run (final)" |
| BASE-5 | pass | build and workspace clippy via run-heavy.sh; smoke child exited 0 |
| BASE-6 | pass | English; comments explain why (held Enter, seen version taken first, adapter window) |
| BASE-8 | pass | no manifest change; nucleo reached through vr-fs; missing direct dep reported |
| BASE-9 | pass | user files written only via `Document::save/save_overwrite/save_as`; Keep mine only after the seen-version re-check; recovery before any discard |
| BASE-10 | pass | `cargo test -p vr-ide` only |
| BASE-11 | pass | no stash/checkout/restore/reset; mutations reverted by copying the saved file back |
| BASE-14 | pass | no waits/polls; all runs foreground < 1 min |
| BASE-15 | pass | nothing started in background; nothing killed |
| BASE-16 | pass | Reuse table; capabilities.md updated |
| BASE-17 | pass | no installs |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | notes created first, progress line per step |
| BASE-20 | pass | confirm_twice / handle_key_at / CONFIRM_* referenced only by app.rs, commands/{app,file}.rs and app_tests.rs (grep) — all updated |
| BASE-23 | pass | palette by keys (F1, ctrl+shift+p), by command and by mouse; dialogs by keyboard and mouse; external prompt by event and by save |
| BASE-24 | pass | no git index commands |
| BASE-25 | pass | fmt check exit 0, workspace clippy clean |
| BASE-32 | pass | D10/USER-1 palette keys from keymap; D46 hard-link notice kept in save paths; D57/D65 replaced by dialog (decision lines); D64 via adapter; D70 resolve used for Save As/Open |
| BASE-35 | pass | USER-1: no chord hard-coded; dialog/palette keys are documented built-ins (keymap.toml `[palette]`, D? (T6)) |
| BASE-38 | pass | `rustfmt --edition 2024` on my files only |
| BASE-46 | pass | smoke ran on a binary built after the last code change |
| COST-1 / COST-3 / COST-9 | pass | heavy via run-heavy; no polling; nothing left running (`ps` count 0) |
| COST-8 | pass | new files written whole, edits batched |
| COST-10 | pass | related set = vr-ide tests (one crate) |
| COST-13 | pass | script in qa-artifacts/T6/ |
| COST-15 | pass | snapshots accepted once after code was final, frames read first |
| COST-16 | pass | output filtered to summary lines |
| AC-6 | pass | palette_filters_by_fuzzy_name_and_shows_keys ("sav" → file.save with "ctrl+s"), palette_enter_runs_command, palette_escape_closes, render_palette_*, smoke step 1 |
| AC-7 | pass | modified_doc_changed_on_disk_prompts, keep_mine_replaces_only_the_version_shown, deleted_on_disk_marks_tab, smoke steps 2-4; residual adapter window noted above |
| D64 | pass (interim) | seen version taken at prompt open; newer write/delete re-prompts (test); swap pending T4 merge |
| USER-1 | pass | see BASE-35 |

n-a: BASE-7 (no commit) · BASE-12, BASE-13 (nothing outside the repo, no untracked user files deleted) · BASE-21 (no secrets) · BASE-22 (no new env var/config key) · BASE-26 (not a rewrite) · BASE-27, BASE-30, BASE-31, BASE-39, BASE-41, BASE-43, BASE-44 (no DB, containers, schema, credentials, access rules, fakes of other systems) · BASE-28, BASE-29, BASE-33, BASE-34, BASE-36, BASE-40 (no third-party code, browser, external system, outward writes) · BASE-37 (not a defect fix) · BASE-42 (no test runner settings) · BASE-45 (AC text unchanged) · BASE-47 (no fixed value) · COST-2 · COST-4 (no full suite) · COST-6, COST-7 (no web UI) · COST-11 (no e2e) · COST-12 · COST-14 · COST-17 · COST-18 · COST-19, COST-20

## D64 wiring
Brief: T4 merged (0bb48ab); swap the interim adapter for `Document::disk_version()` / `Document::save_overwrite_seen(&DiskVersion)` and add the racing-writer test skipped in T6.

### Progress
- read vr-core rustdoc of `disk_version` / `save_overwrite_seen` / `DiskVersion`, `vr_core::testutil` (`RacingWriter`, `timed`, `sweep_offsets`, `assert_no_lost_write_swapping`) and the vr-search racing test pattern
- crates/vr-ide/Cargo.toml: `vr-core = { workspace = true, features = ["test-util"] }` under `[dev-dependencies]` (the one approved manifest line)
- new test `dialog_tests::keep_mine_never_loses_a_concurrent_write`: modified buffer, file rewritten to the shown version, ctrl+s → save prompt, Keep mine selected, then a `RacingWriter` (InPlace and TempRename, 25 start offsets × 200 µs, 6 versions 300 µs apart) races the timed Enter; asserts `assert_no_lost_write_swapping` (strict D77 rule; conflict copies `*.vr-ide-conflict-*` passed as `kept`) and that a refused Keep mine re-prompts
- red against the interim adapter, 3 runs out of 3: `panicked at crates/vr-core/src/testutil.rs:264:5: lost write: 1 writer version(s) were replaced by our bytes and a later writer version overwrote ours`
- swap: `prompt_external_change` takes `Document::disk_version()` when the prompt opens (before any text is shown); `keep_mine` calls `doc.save_overwrite_seen(&seen)`; `ChangedOnDisk` still re-prompts ("changed again"). Adapter deleted: `SeenVersion`, `commands::file::{disk_version, save_overwrite_seen}` and its own re-check; `DialogAction::ExternalChange.seen` is now `vr_core::DiskVersion`
- green: racing test 13 runs in a row (3 + 10), e.g. `keep_mine: ok=23 refused=27`, range ok=15..23 / refused=27..35 per run of 50 — both outcomes exercised, no lost write

### Files changed in this round
crates/vr-ide/Cargo.toml (dev-dependency line), crates/vr-ide/src/commands/file.rs, crates/vr-ide/src/commands/dialog.rs, crates/vr-ide/src/dialog_tests.rs (one test added). Nothing else touched.

### Commands run (trimmed)
```
$ cargo test -p vr-ide
test result: ok. 67 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.84s
$ cargo test -p vr-ide --lib keep_mine_never_loses -- --nocapture   (x10)
keep_mine: ok=21 refused=29 / ok=20 refused=30 / ok=23 refused=27 / ok=18 refused=32 / ok=21 refused=29 /
ok=20 refused=30 / ok=15 refused=35 / ok=16 refused=34 / ok=18 refused=32 / ok=18 refused=32
$ cargo clippy -p vr-ide --all-targets -- -D warnings   → Finished (manifest note: vr-search unused until T10)
$ rustfmt --edition 2024 commands/file.rs commands/dialog.rs dialog_tests.rs
$ cargo fmt --all --check → exit 1, only in crates/vr-core/src/testutil.rs (being edited by another agent at the time; mtime = run time) — not mine; my files are formatted
```

### Rule rows touched
| Rule | Status | Evidence |
|---|---|---|
| BASE-3 | pass | red line above recorded against the interim adapter before the swap |
| BASE-8 | pass | only the approved dev-dependency line |
| BASE-9 | pass | Keep mine goes through vr-core `save_overwrite_seen` (atomic, refuses an unseen version); racing test with the strict rule |
| BASE-16 | pass | adapter removed; vr-core API and `vr_core::testutil` reused |
| BASE-20 | pass | `grep SeenVersion\|fn disk_version\|fn save_overwrite_seen crates/vr-ide/src` → nothing left |
| BASE-25 | pass (mine) | clippy clean; fmt diff only in vr-core/src/testutil.rs (not mine) |
| D64 | pass | interim limit removed; `keep_mine_never_loses_a_concurrent_write`, `keep_mine_replaces_only_the_version_shown` |
| COST-9 | pass | no background tasks or processes started |

## Fix round 1
Brief: B1 own-path Save As bypasses D64; B2 Save As to another target without freshness (needs FR-2 `save_as_seen`, done last); M1 Reload without recovery copy; M2 legacy auto-repeat Enter answers dialogs; minors (racing test asserts both outcomes, App-level modal routing test, Replace onto a file open in another tab).

### Progress
- read 05_review_T6.md, QC T6, 04_dev_FR-2.md, decisions D57/D64/D65/D74/D80-D89; master has no `save_as_seen` yet (grep on crates/vr-core/src/document.rs: none)
- B1 red `save_as_to_own_path_checks_freshness_like_save`: `assertion left == right failed: Save As replaced an unseen version left: "mine old\n" right: "agent\n"` → green: `commands::file::is_own_path` (same path or same canonical location) routes Save As (prompt and `CmdArgs::Path`) to `save_doc` (snapshot check → D81 prompt); one expectation in the test corrected (cursor position after Keep mine → compare with the buffer text)
- racing test refactored into `race_enter(name, shown, prepare)` (strict `assert_no_lost_write_swapping`, refused ⇒ prompt again, asserts ok>0 && refused>0 per writer mode — minor 1); sweep widened to 30 × 300 µs so both outcomes occur (5 runs: ok 10..20 / refused 10..20 per mode)
- B1 racing variant `save_as_to_own_path_never_loses_a_concurrent_write`: written after the fix → red by mutation back to the pre-fix code (`is_own_path` false + old `exists && path != target`): `panicked at crates/vr-core/src/testutil.rs:282:5: lost write: 1 writer version(s) were replaced by our bytes and a later writer version overwrote ours` → restored → green
- M1 red `reload_keeps_unsaved_edits_in_recovery`: `assertion left == right failed left: [] right: ["mine old\n"]` → green: `reload_doc` calls `App::keep_discarded(&[i])` first (info notice names the copy; Err → error notice, no reload); `file.reload` goes through `reload_doc`; prompt texts now say "yours goes to the recovery folder first"
- M2 skeleton (`App::handle_key_at`, `enter_settle`/`enter_repeat_gap` fields, consts; `test_app` sets both to zero) then red `held_or_in_flight_enter_never_answers_a_dialog`: `panicked at dialog_tests.rs:733:5: a held Enter answered the Close dialog` → guard in `App::route_key` (`enter_is_not_deliberate`: Repeat kind, < ENTER_SETTLE since `modal_since`, < ENTER_REPEAT_GAP since `last_enter`; applies while a dialog is open or the palette has focus; `modal_since` set by `open_dialog` and `palette::open` with the event's time) → next red `assertion failed: !path2.exists()` (default of the deleted prompt recreated the file) → deleted prompt gets `Keep` (default, writes nothing) → green. Palette part shown red by mutation (guard for dialogs only): `assertion failed: app.palette.is_some()` → restored
- minor 2 `input_while_a_dialog_is_open_never_edits_the_buffer`: passes on the current code → red by mutation (dialog branch of `handle_paste` removed): `left: "xpasteda\n" right: "xa\n"` → restored → green
- minor 3 red `save_as_onto_a_file_open_in_another_tab`: `one tab per path left: [".../b.txt", ".../b.txt"]` → green: `check_other_tabs` (prompt refuses a target open with unsaved changes; also in `save_as_doc` for `CmdArgs::Path`), `save_as_doc` closes the other unmodified tabs on the target before `rewatch` (closing unwatches the path)
- B2 red `replace_never_overwrites_a_version_written_after_the_prompt`: `Replace overwrote a version the user was not asked about left: "xa\n" right: "agent v2\n"`. Master still has no `save_as_seen` / `DiskVersion::of_path` (grep crates/vr-core/src/document.rs: none, re-checked after the guard run) → STOPPED here per brief; test marked `#[ignore = "B2: waits for vr-core Document::save_as_seen (FR-2)"]` so the suite stays green
- decisions.md: 5 `D? (T6 r1)` lines; capabilities.md: app.rs and file.rs lines updated
- fmt/clippy/guard green; snapshots unchanged (no pending .snap.new)

### Remaining for the resume (B2, after FR-2 merges)
1. `DialogAction::ReplaceFile(DocRef, PathBuf)` → add `seen: vr_core::DiskVersion`, taken with `DiskVersion::of_path(&target)` in `answer_save_as` when the Replace prompt opens (before the message is built); for a name that does not exist take it at submit and commit at once.
2. `save_as_doc` takes `&DiskVersion` and calls `Document::save_as_seen(target, seen)`; `Err(ChangedOnDisk)` → warn "<name> changed on disk; nothing was replaced" and open the Replace prompt again with a fresh `of_path` (absent target that appeared → Replace prompt); a "kept at" Io error → error notice. `CmdArgs::Path` route (no prompt) takes `of_path` itself just before the call (D89 semantics). Non-regular target → error from vr-core, shown in the prompt.
3. Un-ignore `replace_never_overwrites_a_version_written_after_the_prompt`; add racing variants through `race_enter` (generalise it to race a given path: Other target with InPlace + TempRename on the Replace Enter; Absent target with TempRename on the submit Enter), strict `assert_no_lost_write_swapping` + `assert_exchange_proofs_known` (D90), both outcomes asserted.
4. Rerun `cargo test -p vr-ide`, clippy, guard; snapshots once.

### Files changed in this round
crates/vr-ide/src/{app.rs, app_tests.rs (test_app zero guard), dialog_tests.rs, commands/file.rs, commands/palette.rs}. Team files: this notes file, decisions.md, capabilities.md.

### Commands run (trimmed)
```
$ cargo test -p vr-ide
test result: ok. 73 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out; finished in 0.82s   (lib; 3rd ignored = B2 test waiting for FR-2)
test result: ok. 0 passed ... (main.rs, doc-tests)
$ cargo test -p vr-ide --lib never_loses -- --nocapture   (x5)
save_as_own InPlace: ok=18 refused=12 | keep_mine InPlace: ok=20 refused=10 | save_as_own TempRename: ok=18 refused=12 | keep_mine TempRename: ok=17 refused=13
... range over 5 runs: ok 10..20 / refused 10..20 per mode, no lost write
$ cargo clippy -p vr-ide --all-targets -- -D warnings → Finished (manifest note: vr-search unused until T10)
$ cargo fmt --all --check → exit 0
$ run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings → Finished, only the vr-search manifest note
```

### Findings
| Finding | Reply |
|---|---|
| B1 own-path Save As bypasses D64 | fixed (`is_own_path` → `save_doc`; deterministic + RacingWriter tests) |
| B2 Save As to another target unchecked | WAITING FR-2: red test recorded and ignored; flow plan above |
| M1 Reload without recovery copy | fixed |
| M2 legacy/in-flight Enter answers dialogs | fixed (settle + repeat gap, injectable clock; watcher-opened deleted prompt defaults to Keep; event external prompt already defaults to Keep mine) |
| minor racing test both outcomes | fixed (`race_enter` asserts ok>0 && refused>0 per mode) |
| minor App-level modal routing test | fixed |
| minor Replace onto a file open in another tab | fixed (refuse when that tab is modified, otherwise close it) |

### Rule rows touched
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | only crates/vr-ide/src/** + team files |
| BASE-2 | pass | additive: `App::handle_key_at`, `ENTER_SETTLE`, `ENTER_REPEAT_GAP`, `enter_settle`/`enter_repeat_gap`, `file::KEEP`; no contract changed |
| BASE-3 | pass | red line per test above (B1, B1 racing by mutation, M1, M2 + palette part by mutation, minors, B2) |
| BASE-4 | pass | Commands run |
| BASE-9 | pass | own-path Save As through `Document::save` snapshot check; Reload only after recovery write; no write without freshness except B2 (pending FR-2, flagged) |
| BASE-25 | pass | fmt check 0, workspace clippy clean |
| BASE-32 | pass | D64/D74/D81 (own-path), D80 refined (Enter guard), D82 amended (Keep default), D86 (one tab per path); D87–D89 pending (B2) |
| BASE-38 | pass | rustfmt on my 5 files only |
| AC-6 | pass | palette tests green; palette Enter guarded |
| AC-7 | pass (B2 pending) | prompts + Reload recovery + own-path Save As |
| D57 | pass | superseded by dialogs; held-key protection restored via the Enter guard (T5-style timed test) |
| D64 | pass | Keep mine + own-path Save As racing tests |
| USER-1 | pass | no new chords; Enter guard is timing only |
| COST-9 | pass | no background tasks, no processes started |

### B2
Resumed after FR-2 merged (e4c7cf2: `DiskVersion::of_path`, `Document::save_as_seen`, checked `save_as`).
- `replace_never_overwrites_a_version_written_after_the_prompt` (red line above: `left: "xa\n" right: "agent v2\n"`), still red on master with FR-2 (`--include-ignored`: same assertion at dialog_tests.rs:863) → code → `#[ignore]` removed → green
- code (crates/vr-ide/src/commands/{file,dialog}.rs):
  - `DialogAction::ReplaceFile { doc, target, seen: DiskVersion }`; `answer_save_as` takes `DiskVersion::of_path(&target)` before anything is shown (error → shown in the prompt, e.g. a device), existing target → `open_replace(.., seen)`, new name → `save_as_doc(.., Some(&seen))` at submit
  - `save_as_doc(app, i, target, Option<&DiskVersion>) -> Result<(), SaveAsError::{Changed, Failed(String)}>`: `Some` → `Document::save_as_seen`, `None` (`file.save_as` with a path argument, no prompt) → checked `Document::save_as` (D89)
  - `answer_replace`: Changed → warning "<name> changed on disk; nothing was replaced" + `reopen_replace` (fresh `of_path`); new name created meanwhile → "was created on disk meanwhile" + Replace prompt. Own path never reaches this (routed to `save_doc`/D81 earlier)
- racing helper generalised: `race_enter_on(name, modes, shown, lead, prepare -> raced path, asks_again)`; success = saved to the raced path; calls `assert_exchange_proofs_known` (D90); sweep 40 × 300 µs; `lead` sleeps after the writer starts so some writes land while the prompt is still open
- `replace_never_loses_a_concurrent_write` (InPlace + TempRename, lead 3 ms, refused ⇒ Replace prompt again): written after the code → red by mutation (`save_as_seen` → `save_as`, i.e. version taken at commit, not at prompt), 3/3: `panicked at crates/vr-core/src/testutil.rs:328:9: lost write: 6 writer version(s) begun before the operation finished were replaced by our bytes and the operation reported success` → restored → green
- `save_as_new_name_never_loses_a_concurrent_create` (TempRename creates the name, lead 1 ms, refused ⇒ Replace prompt): green; no red possible from vr-ide — after FR-2 every vr-core Save As path checks the target, so no mutation inside vr-ide reproduces the old unchecked create (the create window itself is proven red/green in FR-2's harness, `race_with(Target::Absent)`)
- `save_as_refuses_a_non_regular_target` (`/dev/null` → prompt error, nothing written): red by mutation (version taken of the own file instead of the target): `panicked at dialog_tests.rs:985:5` → restored → green
- 6 runs of all 4 racing tests: replace ok 10..15 / refused 25..30 per mode; save_as_new ok 19..24 / refused 16..21; keep_mine / save_as_own ok 21..26; 0 failures; earlier 10-run loop (lead 3 ms, 30 offsets) fails=0

#### Commands (trimmed)
```
$ cargo test -p vr-ide
test result: ok. 77 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.89s   (2 ignored = existing perf tests)
$ cargo test -p vr-ide --lib -- never_loses --nocapture   (x6)
replace InPlace: ok=15 refused=25 | replace TempRename: ok=14 refused=26 | save_as_new TempRename: ok=19 refused=21 | ... 4 passed
$ cargo clippy -p vr-ide --all-targets -- -D warnings → Finished (manifest note: vr-search unused until T10)
$ cargo fmt --all --check → exit 0
$ run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings → Finished, only the vr-search manifest note
snapshots: unchanged, no .snap.new
```
`--include-ignored` once showed `ui::tests::render_large_file_fast` failing (an `#[ignore]`d timing test, debug build): not run by `cargo test -p vr-ide`, untouched.

#### Rule rows touched
| Rule | Status | Evidence |
|---|---|---|
| BASE-2 | pass | uses vr-core `DiskVersion::of_path`, `Document::save_as_seen` as published; vr-ide additions `SaveAsError`, `answer_replace`, `ReplaceFile{doc,target,seen}` |
| BASE-3 | pass | B2 deterministic red recorded before the code; racing Replace and non-regular red by mutation; new-name racing red not expressible (above) |
| BASE-9 | pass | every Save As write goes through a freshness-checked vr-core call; strict racing tests for Replace and new name |
| BASE-32 | pass | D87–D90 honoured (version at prompt / submit, re-prompt, non-regular refused, exchange proofs asserted) |
| AC-7 | pass | B2 closed |
| COST-9 | pass | no background tasks or processes |

Findings: B2 — fixed.

## Fix round 2
Brief: R1-1 — a refused Replace / Keep mine / new-name Save As sometimes leaves only a notice (no prompt): (a) no fresh version could be taken after Changed (D78 "keeps changing"), (b) the commit ended with a conflict copy ("kept at", `Failed`). Required: after any refusal the user is back in a prompt showing the error; without a version the choice stays refused (writes nothing) until one is taken.

### Progress
- read `## Round 1 re-check` in 05_review_T6.md; vr-core's `hook` seams are `#[cfg(test)] pub(crate)` (not reachable from vr-ide) → test seam in vr-ide `commands::file::seam` (cfg(test), thread-local: fail the next N version reads, fail the next commit with a given error)
- red (seam present, not wired) `refused_choice_reprompts_when_no_version_can_be_taken`: `panicked at dialog_tests.rs:1013:5: other.txt already exists. Replace it?` (prompt without the "keeps changing" error); `refused_choice_with_an_error_reprompts_with_it`: `panicked at dialog_tests.rs:1071:5: no prompt; notices:` (branch (b): conflict-copy error → notice only, no Replace prompt)
- code (crates/vr-ide/src/commands/{file,dialog}.rs):
  - `DialogAction::ReplaceFile.seen` and `ExternalChange.seen` are `Option<DiskVersion>`; `None` = no version could be read: Replace / Keep mine then write nothing, read again and reopen the prompt (D? (T6 r2))
  - `open_replace(.., seen, error)` / `reopen_replace(.., error)`: the error goes first in the prompt message (`with_errors`); a failed read (`take_version` Err) reopens with `seen: None` and the D78 text "the file keeps changing on disk; try again"
  - `answer_replace`: `Changed` → warn + reopen; `Failed(e)` (conflict copy, other errors) → reopen with `e` (was: error notice only, branch (b)); `None` → read again + reopen
  - `prompt_external_change_with(.., error)`: opens even when `disk_version` fails (was: error notice only, branch (a)), with the reason in the message; `keep_mine` refusal of any kind → prompt again with the reason (was: `Err(e)` → notice only)
  - new-name Save As: `Changed` → Replace prompt with "was created on disk meanwhile"; `Failed` → name prompt with the error (already)
  - test seam `commands::file::seam` (cfg(test), thread-local): `fail_next_versions(n)` (`take_version`/`doc_version` return the D78 error), `fail_next_commit(msg)` (`injected_commit_error` replaces the commit result of Replace/new-name/Keep mine)
- green: both deterministic tests; racing tests' assert now prints the dialog and the notices; `save_as_new` accepts the name prompt with an error as "asks again" (any refusal → a prompt)
- full `cargo test -p vr-ide` × 10 in a row: 0 failures (below)

### Commands (trimmed)
```
$ for i in 1..10: cargo test -p vr-ide
run 1: ok test result: ok. 79 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished in 0.95s
run 2: ok test result: ok. 79 passed; 0 failed; 2 ignored; ... finished in 0.95s
run 3: ok ... 79 passed; 0 failed ... 0.94s
run 4: ok ... 79 passed; 0 failed ... 0.94s
run 5: ok ... 79 passed; 0 failed ... 0.90s
run 6: ok ... 79 passed; 0 failed ... 0.90s
run 7: ok ... 79 passed; 0 failed ... 1.01s
run 8: ok ... 79 passed; 0 failed ... 0.93s
run 9: ok ... 79 passed; 0 failed ... 1.40s
run 10: ok ... 79 passed; 0 failed ... 1.35s
fails=0
$ cargo clippy -p vr-ide --all-targets -- -D warnings → Finished (manifest note: vr-search unused until T10)
$ cargo fmt --all --check → exit 0
$ run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings → Finished, only the vr-search manifest note
snapshots unchanged (no .snap.new)
```

### Files changed in this round
crates/vr-ide/src/commands/file.rs, crates/vr-ide/src/commands/dialog.rs, crates/vr-ide/src/dialog_tests.rs (2 tests added, racing assert messages, save_as_new predicate). Team files: this notes file, decisions.md.

### Findings
| Finding | Reply |
|---|---|
| R1-1 flaky refused Replace without prompt | fixed — option (2): every refusal reopens the prompt with the reason in it; deterministic seam tests for (a) and (b); 10 full-suite runs clean |

### Rule rows touched
| Rule | Status | Evidence |
|---|---|---|
| BASE-3 | pass | red lines of both seam tests recorded before the wiring |
| BASE-4 | pass | 10-run full-suite loop output above |
| BASE-9 | pass | without a version nothing is written; the no-lost-write oracle unchanged in all 4 racing tests |
| BASE-25 | pass | fmt 0, workspace clippy clean |
| BASE-32 | pass | D64/D78/D81/D87 honoured; D? (T6 r2) logged |
| AC-7 | pass | prompts always reopen after a refusal |
| COST-9 | pass | no background tasks or processes |
