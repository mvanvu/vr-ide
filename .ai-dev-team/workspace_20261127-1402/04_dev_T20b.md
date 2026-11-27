# T20b — Slash commands phase 2 (developer notes)

## Progress
- Start: read 01_spec_T20.md (Phase 2), 04_dev_T20.md incl. Round 1, capabilities.md, D258–D271.
- vr-config Red: settings names/value_text/with_value and user_file_template tests FAILED (todo! stubs: 4 failed). recent_files.rs written with its tests in one step (a copy of the proven recent.rs pattern; its 2 tests passed on first run — not seen red, noted).
- vr-config Green: 14 lib tests ok (one fix: range messages already name the key, no "key: " prefix twice).
- vr-core Red: text_command_tests (rewrite_lines, rewrite_selection, convert_line_breaks) 4 FAILED on todo! stubs. Green: edit_ops 10/10.
- vr-ide: slash2_tests.rs written before the handlers (red = did not compile: missing `recent_files_file`, `config_dir`, `View.title`, `flush_recent_files`, `language_ids`); the run after the code: 15/18, 3 test-expectation fixes (D268: Enter on `/indent` / `/lang` with nothing typed completes the name instead of an error; `/recent` alone leaves the window open with the list, so `run` not `ran`). Then 18/18.
- Registry test extended with the 16 names; docs/commands.md table regenerated from `print_docs_table`, prose for the new commands; `/lang` description without `<id>` (raw angle brackets in a markdown cell).
- Snapshots: only the 3 `render_slash_list_*` changed (rows now start with the new file commands after /reload); error snapshots unchanged. Reviewed and accepted.
- cargo test -p vr-ide -p vr-config: 448 + 14 + 21 + 8 ok. clippy -D warnings clean, fmt --check clean.
- Walk script written (qa-artifacts/T20b/dev/walk.py); capabilities.md (6 lines) and decisions.md (one D? (T20b) entry) updated. Full suite + release build running in the background via run-heavy.sh.
- Full suite + release done (below). Walk run 1: 15/16; the one FAIL was the walk's own check (it wanted " keys " with a space after it; the keys tab is the last thing on the tab row). Fixed in walk.py, not in the app. Run 2: 16/16. Nothing left running.

## Changes
- `crates/vr-config/src/settings.rs`: `Settings`/`AiPreset` derive `Serialize`; `DEFAULT_SETTINGS` pub; `Settings::{names, value_text, with_value}` + 3 tests.
- `crates/vr-config/src/keymap.rs`: `DEFAULT_KEYMAP` pub. `crates/vr-config/src/lib.rs`: exports, `user_file_template` + 1 test.
- `crates/vr-config/src/recent_files.rs` (new): `RecentFiles`, `RECENT_FILES_FILE`, `RECENT_FILES_MAX` = 20 + 2 tests.
- `crates/vr-core/src/edit_ops.rs`: `rewrite_lines`, `rewrite_selection`, `convert_line_breaks` (additive) + 4 tests.
- `crates/vr-ide/src/recent_files.rs` (new): `RecentWriter` (writer thread, merges with the file, error reported on the next open).
- `crates/vr-ide/src/app.rs`: `View.title`; `App::{recent_files, recent_files_file, recent_writer, config_dir, flush_recent_files}`; `load_workspace` loads the recent files and sets `config_dir`; `remember_recent` records the file; `read_only_warning` names a generated tab; `sync_mode`: a read-only untitled tab is not in edit mode.
- `crates/vr-ide/src/syntax.rs`: `DocSyntax.forced`, `Syntax::{set_language, language_ids}`.
- `crates/vr-ide/src/commands/explorer.rs`: `rename_in(app, root, path, name)` (the explorer's checks for any root); `answer_rename` now calls it (same behaviour); `below_root_of`.
- `crates/vr-ide/src/commands/slash.rs`: `ArgKind::{RecentFile, Language, Setting}`, `picks_from_list`, `INDENTS`, `EOLS`, 16 commands, `setting_candidates`, `key_table`.
- `crates/vr-ide/src/ui/tabs.rs`: the tab label uses `View.title` when set.
- `crates/vr-ide/src/app_workspace.rs:123`: T20 QC LOW: "exists; /ws save does not replace another file" (test: `ws_save_onto_another_existing_file_says_it_is_not_replaced`; no doc text had the old message).
- `crates/vr-ide/src/slash2_tests.rs` (new, 18 tests); `slash_tests.rs`: helpers `pub(crate)`, registry test lists the 16 new names; 3 `render_slash_list_*` snapshots updated.
- `docs/commands.md`: Tab completion list, prose for file/text/settings commands, table regenerated (test-checked).
- `.ai-dev-team/capabilities.md` (6 lines), `.ai-dev-team/workspace/decisions.md` (one `D? (T20b)` entry).
- No key binding added or changed (keymap.toml defaults untouched).

## Reuse
| Need | Reused | Note |
|---|---|---|
| list, Tab, Enter, error under the input | T20 slash registry (`rows/complete/submit`, `Palette.error`) | new ArgKinds only |
| rename | explorer rename (`validate_name`, `below_root`, `check_tab_free`, `vr_fs::ops::rename`, `retarget`) | factored into `rename_in` with the root as a parameter |
| edit-mode gate | `App::editable_doc` | read mode shows the read-mode hint |
| line blocks, one undo step | `edit_ops::{line_blocks, apply_by_pos}`, `Buffer::apply` | |
| state file | `vr_config::write_atomic`, the RecentWorkspaces pattern | |
| setting checks | `settings::parse` (load's checks) | `with_value` goes through it |
| clipboard | `App.clipboard` | |
| language ids | `LanguageRegistry::{ids, has_grammar}` | |
| docs table | `docs_table` + `docs_table_matches_the_registry` | |

## Command table (phase 2)
| Command | Behaviour |
|---|---|
| `/rename <name>` | explorer checks in the file's workspace root; tab follows (unsaved edits kept); no file / untitled / outside the roots → error |
| `/copy-path`, `/copy-relpath` | clipboard; relpath relative to the root holding the file; outside every root → error |
| `/recent [n\|name]` | no argument: the window lists the files; n opens entry n; text: newest entry whose shown path contains it |
| `/sort` `/unique` `/upper` `/lower` `/trim` | selection (lines, or text for upper/lower) or the whole file; edit mode required; one undo step; "nothing to change" notice |
| `/indent 2\|4\|tab` | re-indent (see D? T20b) and set the file's indent unit |
| `/eol lf\|crlf` | converts every CR/LF/CRLF, sets `line_ending`, file modified; "already" notice |
| `/lang <id>` | highlighter override until the tab closes; unknown id → error; read mode allowed (no text change) |
| `/set <name> <value>` | session-only, type/range checked, theme names checked; open files, explorer toggles and terminal height follow |
| `/config`, `/keymap` | open the user file; a missing one is made from the defaults, every key line commented out |
| `/keys` | read-only "keys" tab from the keymap; a second /keys shows the same tab |

## Tests (final runs)
```
cargo test -p vr-core --lib edit_ops         → ok. 10 passed
cargo test -p vr-ide -p vr-config            → vr-ide lib ok. 448 passed; 0 failed; 5 ignored; vr-config 14 + 21 + 8 ok
cargo test -p vr-ide --lib slash             → ok. 62 passed (slash2_tests 18, slash_tests 44); 1 ignored
cargo fmt --all --check                      → (no output, exit 0)
cargo clippy --workspace --all-targets -- -D warnings → Finished `dev` profile
run-heavy.sh cargo test --workspace -j 4     → 26 result lines, all ok, 844 passed, exit=0 (qa-artifacts/T20b/dev/full-suite.txt)
run-heavy.sh cargo build --release -p vr-ide → Finished `release` profile in 35.66s, exit=0 (release.txt)
```

## PTY walk (legacy bytes, release binary, empty HOME, temp config/state dirs)
`qa-artifacts/T20b/dev/walk.py` → `walk.txt`, screens `walk_01..16_*.txt`: **16 / 16 PASS**. Opens in READ; /sort in READ → read-mode hint, text unchanged; space i → EDIT; shift+down ×2 + /sort → only lines 1–2 sorted; /lang rust → notice, "fn" redrawn with its own colour; /upper with no selection → whole file; /trim; /eol crlf → status CRLF + ●; ctrl+s → disk `B\r\nC\r\nFN MAIN() {}\r\nZZ\r\n`; `/set tab_size 2` → "unknown setting tab_size" (the setting is `tab_width`), text kept; /set tab_width 2 → session notice; /keys → read-only "keys" tab with the table, typing refused; /recent lists a.txt; /recent 1 + /rename b.txt → renamed on disk, tab shows b.txt; /q quits; state/recent-files.toml lists a.txt. Nothing left running.

## Open points for the leader
- The brief's sample `/set tab_size 2`: no such setting exists (the key is `tab_width`); it is refused with "unknown setting tab_size", the walk shows both.
- `/config` / `/keymap`: no live reload of settings/keymap exists, so the notice says changes apply at the next start; making the file with commented lines (rather than an active copy of the defaults) is a developer choice (D? T20b).
- `/recent` keeps its own persistent list; the welcome screen's session list (8) is unchanged — merging them is a product choice.
- `/indent` step detection (smallest space-only indent) can be thrown off by 1-space continuation lines (e.g. ` * ` in C block comments); documented.

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | only the 16 phase-2 commands + the QC LOW message; no soft wrap, no T19 |
| BASE-2 | pass | additive: new fns/fields/variants; `answer_rename` keeps its signature and behaviour (explorer tests green) |
| BASE-3 | pass | red runs: vr-config 4 FAILED on stubs, vr-core 4 FAILED on stubs, slash2 compile-red before the handlers (recent_files.rs tests not seen red, noted) |
| BASE-4 | pass | outputs above + qa-artifacts/T20b/dev/{full-suite,release,walk}.txt |
| BASE-5 | pass | full suite + release via run-heavy.sh in the background |
| BASE-6 | pass | English; comments say why |
| BASE-8 | pass | no dependency added (serde Serialize from the existing serde) |
| BASE-10 | pass | vr-ide + vr-config (+ vr-core edit_ops); full suite once |
| BASE-11 / BASE-24 | pass | no stash/checkout/restore/reset/commit |
| BASE-14 | pass | heavy run in the background, turn ended until notified |
| BASE-15 | pass | only my own processes; none left |
| BASE-16 | pass | Reuse table; capabilities.md appended |
| BASE-17 | pass | no installs |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | this file, a progress line per step |
| BASE-25 / BASE-38 | pass | fmt --check clean, clippy -D warnings clean; fmt changed only this task's files (git status) |
| BASE-32 | pass | no key binding added or changed |
| AC-20b | pass | every phase-2 command tested incl. bad arguments and read mode (slash2_tests), walk |
| AC-20d | pass | docs_table_matches_the_registry, registry test, 3 list snapshots reviewed |

n-a: BASE-7, BASE-9, BASE-12, BASE-13, BASE-20..23, BASE-26..31, BASE-33..37 (no network, schema or new crate; user files are written only by the existing freshness-checked save path; the config templates are made with O_EXCL and never replace a file).

## Round 1 (review fixes, 05_review_T20b.md)

### Progress
- Read the review (4 major, 4 minor), D272/D273.
- Finding 4 red: `vr-config` `two_racing_writers_keep_every_entry` and `an_unreadable_list_is_not_replaced` FAILED (3 runs, unlocked `RecentFiles::record` stub).
- Finding 4 green: `RecentFiles::record` (sidecar `recent-files.toml.lock`, `File::lock`, Io warning → error, file kept): 4/4 ok, 3 runs.
- Red for findings 1, 2, 3 and minors 1, 2, 4: `cargo test -p vr-ide --lib -- slash2 recent_files` → 6 FAILED (indent doc comment: `        if`/`                y();`; selection: `  b()`; /set: hidden toggled back; rename: list still a.txt; close stub: `left: []`; eol: no notice).
- Green: `indent_step` (whole document, skips blank and ` *` lines, mode of increases, 1-column increases only as a last resort); `/set` applies only changed keys; `RecentWriter::{renamed, close}` + `App::{recent_file_renamed, close_recent_files}`, `event_loop::exit` closes with 500 ms; `/eol` no-break notice + `insert_newline` uses `doc.line_ending` when the text has no break. `cargo test -p vr-ide --lib -- slash2 recent_files` → 25 passed.
- Minor 3 (/sort on 1M lines): walk_big before (release, round-1 tree without the perf change): `/upper 0.23s`, `/sort 2.22s`. Removing the second rope build in `apply_blocks` alone: `/sort 2.19s` (not the bottleneck). Ignored timing test `sort_timing_on_a_million_lines` (release): `rewrite_lines sort: 2.274506638s`. Cause: per-line `content()` + `line_break_str()` lookups and allocations. Now one pass over `rope.lines_at(s)` (`content_len`): `rewrite_lines sort: 405.217615ms`. vr-core 155 passed.
- Release build + PTY: walk_big after `/upper 0.21s`, `/sort 0.46s` (before 2.22s); walk_review A and B now correct; walk_r1.py 3/3 PASS. fmt --check clean, clippy -D warnings clean, `cargo test -p vr-ide -p vr-config -p vr-core` all ok (454 + 16 + 21 + 8 + 155). docs/commands.md (/indent, /eol, /set prose), capabilities.md line 91, D272 amended. Full suite running in the background via run-heavy.sh.
- Full suite done: 26 result lines all ok, 852 passed, exit=0 (qa-artifacts/T20b/dev-r1/full-suite.txt). docs_table test rerun after the prose edit: ok. Nothing left running.

### Fixes
| Finding | Fix | Test (red → green) |
|---|---|---|
| major 1 /indent doc comment | `slash.rs` `indent_step`: whole document, skips blank and `*`-led lines, most common indent increase (tie → smaller), 1-column increases only when there are no others, fallback the file's unit | `indent_ignores_block_comment_leads_when_finding_the_step` (C + Java-in-class) |
| major 2 /indent selection | the step comes from the whole document, applied to the selected lines | `indent_on_a_selection_takes_the_step_from_the_whole_file` (selection gives `    b()`, whole file `  if a:` / `    b()`) |
| major 3 /set | `apply_settings` compares old vs new: buffer options only when they changed; explorer toggles / terminal height only when that key changed | `set_of_an_unrelated_key_keeps_the_explorer_toggles_and_terminal_height` |
| major 4 recent-files race | `vr_config::RecentFiles::{record, update}`: `File::lock` on `recent-files.toml.lock` around load-change-save; an Io load warning returns an error and leaves the file; the writer reports it as a notice on the next open | `two_racing_writers_keep_every_entry` (2 threads × 10), `an_unreadable_list_is_not_replaced` (chmod 000) |
| minor /rename | `App::recent_file_renamed` → `RecentFiles::renamed` in memory + `RecentWriter::renamed` on disk | `rename_updates_the_recent_files_list`; walk_r1 #1 |
| minor writer at exit | `RecentWriter::close(wait)` (flush with `recv_timeout`), `event_loop::exit` calls `close_recent_files(500 ms)` after recovery | `close_writes_what_was_recorded_just_before`; walk_r1 #2 |
| minor /sort 1M lines | `apply_blocks`: no second rope build (cursor line/col read before, placed after `apply`); `rewrite_lines`: one pass with `rope.lines_at` (`content_len`) instead of per-line lookups | ignored timing test 2.27 s → 0.41 s; walk_big /sort 2.22 s → 0.46 s |
| minor /eol no breaks | notice "/eol: no line breaks yet; new lines use CRLF (Enter, paste)"; `edit.rs insert_newline` uses `doc.line_ending` when the text has no break (paste already did) | `eol_on_a_file_without_line_breaks_sets_the_style_for_new_lines`; walk_r1 #3 |

Not changed: the explorer's own rename does not update /recent (only `/rename`, per the finding; a folder rename would need prefix rewriting). After a whole-file text command the cursor is set after `apply`, so a redo restores the mapped cursor rather than the same line/column (undo unchanged).

### Files changed in round 1
`crates/vr-config/src/recent_files.rs`, `crates/vr-ide/src/recent_files.rs`, `crates/vr-ide/src/app.rs`, `crates/vr-ide/src/event_loop.rs`, `crates/vr-ide/src/commands/slash.rs`, `crates/vr-ide/src/commands/edit.rs`, `crates/vr-core/src/edit_ops.rs`, `crates/vr-ide/src/slash2_tests.rs`, `docs/commands.md`, `.ai-dev-team/capabilities.md`, `.ai-dev-team/workspace/decisions.md` (D272 amended).

### Evidence (round 1)
```
cargo test -p vr-config --lib recent_files (red, stub)  → 2 FAILED (racing, unreadable), 3 runs
cargo test -p vr-ide --lib -- slash2 recent_files (red) → 6 FAILED
cargo test -p vr-ide -p vr-config -p vr-core            → 454 / 16 / 21 / 8 / 155 passed, 0 failed
cargo fmt --all --check                                 → exit 0
cargo clippy --workspace --all-targets -- -D warnings   → Finished
run-heavy.sh cargo test --workspace -j 4                → 26 ok lines, 852 passed, exit=0 (dev-r1/full-suite.txt)
run-heavy.sh cargo build --release -p vr-ide            → Finished `release` (dev-r1/release.txt)
dev-r1/walk_review.txt  A: '/**\n * doc\n */\nint f() {\n  if (x) {\n    y();\n  }\n}\n'  B: 'def f():\n    if a:\n    b()\n    c()\n'  C–G unchanged from the review
dev-r1/walk_big.txt     /upper 0.21s, /sort 0.46s (walk_big_before.txt: 0.23s / 2.22s)
dev-r1/walk_r1.txt      3/3 PASS (rename → recent-files.toml has b.txt only; open + /q → entry written; /eol crlf one-line → notice, disk 'one line\r\n')
```

### Rule compliance (round 1 rows; others as above)
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | only the 8 review findings; no key binding change |
| BASE-3 | pass | each fix had a failing test first (8 red, listed above) |
| BASE-4 | pass | outputs above + dev-r1/*.txt |
| BASE-5 / BASE-14 | pass | full suite + release via run-heavy.sh; suite in the background, turn ended until notified |
| BASE-6 | pass | English, why-comments |
| BASE-8 | pass | no dependency added (`File::lock` is std, toolchain 1.100) |
| BASE-10 | pass | vr-ide + vr-config + vr-core; full suite once |
| BASE-11 / BASE-24 | pass | no commit/stash/checkout/restore/reset |
| BASE-15 | pass | only my own processes; PTY sessions killed by the walks |
| BASE-16 | pass | reused `RecentFiles`, `write_atomic`, `lines_at`; capabilities.md updated |
| BASE-19 | pass | progress lines above |
| BASE-25 / BASE-38 | pass | fmt --check, clippy -D warnings clean |
| BASE-32 | pass | keymap defaults untouched |
| AC-20b | pass | /indent (findings 1–2) and /set (finding 3) fixed with tests and PTY evidence |
| COST-8 | pass | after one blocked heredoc edit, edits via Edit / perl only |
