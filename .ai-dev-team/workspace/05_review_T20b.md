# Review T20b — slash commands phase 2 (code + security + design QA)

Verdict: **CHANGES_REQUESTED** (code: 4 major, 4 minor; security: no findings (SECURE); design QA: pass)

Scope: uncommitted tree vs HEAD 6fffe67, T20b files only. Fresh release build (`cargo build --release -p vr-ide` was already up to date with the tree). Dev walk re-run: 16/16 PASS (`qa-artifacts/T20b/review/walk_*.txt`). Reviewer walk: `walk_review.py` → `walk_review.txt`, screens `rv_01..12_*.txt`; `walk_big.py` → `walk_big.txt`.

## Findings

[major] crates/vr-ide/src/commands/slash.rs:1229 — /indent takes the smallest indent as the step, so a 1-column line breaks the whole file
Scenario: a 4-space C/Java/JS file with a top-level `/** … */` comment (` * doc` has a 1-space indent). The step becomes 1, so each column becomes one unit. `/indent 2` makes `    if` 8 spaces and `        y();` 16 spaces (rv_01: `'int f() {\n        if (x) {\n                y();\n'`). The user asked for 2 spaces and got 8/16. The developer's notes list this as a known limit. It still applies to most C-family files.
Fix: compute the step over the whole document and ignore 1-column leads (or leads followed by `*`). Use the most common positive difference between consecutive non-blank indent levels, falling back to the file's indent unit. Add tests for the doc-comment case.

[major] crates/vr-ide/src/commands/slash.rs:1215-1234 — /indent on a selection takes the step from the selected lines only
Scenario: a 4-space Python file. Select the two 8-space lines under `if a:` and run `/indent 2`. The step becomes 8 and the lines become `  b()` (rv_02: `'    if a:\n  b()\n  c()\n'`). They now sit less indented than their `if`, which is an IndentationError. Expected: `    b()` (two 2-space levels).
Fix: take the step from the whole document (same function as above) and apply it only to the selected lines. Add a test with a nested selection.

[major] crates/vr-ide/src/commands/slash.rs:1332-1338 — any /set reverts the explorer toggles and the terminal height
Scenario: ctrl+h hides hidden files (`toggle_hidden` changes the tree, not `settings.show_hidden`). `/set tab_width 2` then compares the tree with the unchanged setting and toggles the files back on, with the notice "explorer: hidden files shown". The same happens with show_ignored. A terminal panel resized with grow/shrink (terminal.rs:328/334) also snaps back to `terminal_height_percent`. An unrelated setting silently replaces the user's current view state.
Fix: compare the old settings with `new` (`before.show_hidden != new.show_hidden`, and the same for show_ignored and terminal_height_percent). Apply only the key that changed. Add a test: toggle hidden, `/set tab_width 2`, the explorer stays as toggled.

[major] crates/vr-ide/src/recent_files.rs:35-37 — the merge with the file is a read-modify-write with no lock, so two windows lose each other's entries
Scenario: windows A and B open files at about the same time. A's writer loads the list, B's writer loads and saves (with B's file), then A saves its copy (without B's file). B's entry is gone, and /recent in a third window never shows it. That breaks D272's "files of every window". This is a lost update of state data, not file content, so I rated it major rather than blocker; the leader may raise it.
Also on this line: a read error other than "missing" (EACCES or EIO, warning dropped by `let (mut list, _)`) loads an empty list, and the save then replaces the 20 entries with 1.
Fix: hold `std::fs::File::lock()` (std ≥1.89, toolchain 1.100) on `recent-files.toml.lock` around load+push+save. On a load warning that is not "missing", skip the write and report it. Add a test that races two writers × N records and finds every path.

[minor] crates/vr-ide/src/commands/slash.rs:1076 — /rename does not update the recent-files list
Scenario: /rename a.txt → b.txt. `recent-files.toml` still lists only a.txt (dev walk_16 and this review's re-run: `files = [".../a.txt"]`), so `/recent 1` opens a file that no longer exists and fails.
Fix: after the rename, replace `from` with `to` in `app.recent_files` and record `to`.

[minor] crates/vr-ide/src/recent_files.rs:25 — the writer thread is not joined at exit
Scenario: run `vr-ide x.rs` then `/q` within a moment. `main` returns while the record is still queued, and the entry is lost. The doc comment says "after the paths already recorded are written", which does not hold.
Fix: call `flush_recent_files()` in `event_loop::exit`, or keep the JoinHandle and join it on Drop.

[minor] crates/vr-core/src/edit_ops.rs:380 — whole-file text commands run on the UI thread and rebuild the rope twice
Scenario: a 1,000,000-line (60 MB) file. `/sort` freezes the screen for 2.2 s with no notice, and `/upper` for 0.2 s (walk_big.txt). `apply_blocks` clones the rope and applies the change to it only to compute the cursor, then `buffer.apply` does the same work again.
Fix: compute `same_place` from the buffer after `apply` (save the line/column first). Optionally show a "working…" notice above a size threshold.

[minor] crates/vr-ide/src/commands/slash.rs:1269 — /eol on a file without line breaks says nothing and does not stick
Scenario: a one-line file, `/eol crlf`. `line_ending` changes, but there is no notice, the file is not modified, and closing and reopening detects LF again.
Fix: add a notice such as "/eol: no line breaks yet; new lines use CRLF".

Security: no findings. `/config` and `/keymap` create the file with `create_new` (O_EXCL; a symlink at the path is never followed), and an existing file is opened unchanged (rv_08). The mode comes from the umask (0664 here). Paths are XDG or `VR_IDE_*_DIR`. `/set` parses `v = {raw}` into a separate table and keeps only `v`. `/rename` reuses the explorer checks; rv_03 shows the overwrite refused and rv_04/05 show the unsaved edits following the tab. Recovery entries follow by design, because they are published from `doc.path` on each frame.

Design QA: pass. The 40x12, 80x24 and 120x40 list snapshots cut rows with "…" (D270). The keys tab is aligned, has its own label and refuses typing (walk_11/12). The error lines sit under the input (rv_03).

## Files read
| File | Status |
|---|---|
| crates/vr-config/src/{keymap,lib,settings,recent_files}.rs | read |
| crates/vr-core/src/edit_ops.rs | read |
| crates/vr-ide/src/{app,app_workspace,lib,syntax,recent_files}.rs | read |
| crates/vr-ide/src/commands/{slash,explorer}.rs | read (diff) |
| crates/vr-ide/src/ui/tabs.rs | read |
| crates/vr-ide/src/slash_tests.rs, slash2_tests.rs | read (diff; slash2 test names + indent/eol tests) |
| 3 render_slash_list_*.snap | read |
| docs/commands.md | skipped — generated table, checked by `docs_table_matches_the_registry` |
| .ai-dev-team/capabilities.md, decisions.md | read |

## Rule check T20b
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | only the 16 phase-2 commands + the T20 QC message |
| BASE-2 | pass | `answer_rename` still does select_path on Ok(Some); additive APIs |
| BASE-3 | pass | red runs in notes (recent_files tests not seen red, stated) |
| BASE-4 | pass | outputs in notes, full-suite.txt, release.txt |
| BASE-6 | pass | English, why-comments |
| BASE-8 | pass | no manifest change |
| BASE-16 | pass | Reuse table; rename_in shares the explorer checks; RecentFiles mirrors RecentWorkspaces (same pattern; acceptable) |
| BASE-32 | pass | defaults/keymap.toml not in the diff |
| AC-20b (phase 2) | fail | /indent findings 1–2; /set finding 3 |
| AC-20d | pass | registry test + docs table test; 3 snapshots reviewed |
| BASE-25/38 | n-o | not rerun (notes: fmt/clippy clean) |

## Round 1 re-check

Verdict: **APPROVED**. The 4 majors and the 4 minors are fixed with tests that fail on the old code. Nothing new was opened.

Spot-runs: `cargo test -p vr-ide --lib slash2` 23 ok; `cargo test -p vr-config recent` 6 ok (racing writers, unreadable list); `cargo test -p vr-core edit_ops` 10 ok (1 ignored timing test); `dev-r1/walk_r1.py` against the existing `target/release/vr-ide` 3/3 PASS (rename in recent-files.toml, open then /q recorded, /eol crlf on a one-line file).

Checked:
- /indent: `indent_step` reads the whole rope, skips blank and `*`-led lines, takes the most common increase (tie to the smaller), uses 1-column increases only as a last resort. A selection uses the same step. Tests cover the C/Java doc comment and the nested selection.
- /set: `apply_settings` compares old and new per key; the explorer toggles and the terminal height change only when their key changed. The test toggles hidden, runs `/set tab_width`, and the toggle stays.
- RecentFiles: `File::lock` (std) on the sidecar `recent-files.toml.lock`, which is never replaced, around load, change and atomic save. The lock is released on drop. An Io warning returns an error before `save`, so the file is kept; the test checks the bytes are unchanged. The 2-thread race test passes.
- Rename: `recent_file_renamed` updates memory and queues `Renamed` on disk. Exit: `close(500 ms)` flushes with a bounded `recv_timeout`; the test and walk #2 pass.
- rewrite_lines: one pass with `lines_at` and `content_len` (CRLF is one break, a bare CR is handled by `is_line_break`). Break n ends line n, extra lines reuse the block's last break, and the last line stays without one. Tested for CRLF and for fewer lines.
- `insert_newline`: the file's `line_ending` is used only when `rope.len_lines() == 1`. A file with any break still uses `default_line_break`, so there is no regression. A file that ends with a break has 2 lines and takes the old path.
- Redo cursor: the cursor is set after `apply`, so a redo restores the mapped cursor, not the same line and column. Undo is unchanged. Acceptable, cosmetic. Whole-file `/sort` on 1M lines went from 2.2 s to 0.46 s (dev measurement; I did not rerun the 1M-line walk).

Non-blocking notes (no action required):
- `indent_step` also skips a line that starts with `*` as code (`*p = 1;` in C), so that line does not count towards the step. At worst it loses one sample.
- No test for a mixed CR/LF block in `rewrite_lines`. The code path is covered by `content_len`.

## Rule check T20b
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | round 1 touches only the 8 findings |
| BASE-3 | pass | red runs in the notes, tests above fail on the old behavior |
| BASE-4 | pass | spot-runs above, walk_r1 3/3 |
| BASE-8 | pass | `File::lock` is std, no manifest change |
| BASE-16 | pass | reuse of `RecentFiles`, `write_atomic`, `lines_at` |
| AC-20b (phase 2) | pass | /indent, /set fixed with tests and walk evidence |
| AC-20d | pass | registry and docs tests unchanged and green in the notes |
| SEC (recent-files lock) | pass | sidecar lock, error path keeps the file |
