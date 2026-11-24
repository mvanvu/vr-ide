# Review T2 — CHANGES T2 (no blocker, no major; 3 minor, 1 uncertain)

Tools available: Read, Bash, Write, Skill, SendMessage, SubagentHandback.
Ran once: `cargo test -p vr-core` = 26 passed; `cargo clippy -p vr-core --all-targets -- -D warnings` = clean (only the T4 `unused dependency` manifest warnings).

## Findings

[minor] crates/vr-core/src/transaction.rs:53 — normalize sorts by `from` only, so overlap dropping depends on input order
Scenario: `[Change(5,8,""), Change(5,5,"x")]` (delete and insert at the same start): stable sort keeps the delete first, then the insert has `5 < 8` and is dropped. The order `[insert, delete]` keeps both. A caller (T9 replace_all, multi-range ops) gets a different result by list order alone.
Fix: sort by `(from, to)` so a pure insertion at a span start always sorts first.

[minor] crates/vr-core/src/transaction.rs:64-67 — overlapping changes are dropped with only `tracing::warn`
Scenario: a buggy caller sends overlapping changes. The batch applies partially, as one undoable step, with nothing visible but a log line if a subscriber exists. Text is never corrupted, so it is safe in the data-integrity sense, but caller bugs stay hidden in tests.
Fix: add `debug_assert!(false, ...)` or `#[cfg(debug_assertions)] panic!` next to the warn, so tests and debug builds fail loudly while release stays tolerant. Keep the drop for release.

[minor] crates/vr-core/src/edit_ops.rs:224-241 — indent moves the anchor and head of a range that starts at column 0
Scenario: select lines 1..3 starting at col 0 and press Tab. The default `Selection::map` treats a cursor at a pure-insertion point as "after the inserted text", so the selection start shifts past the new indent on the first line. VS Code keeps the start at column 0. Every covered line is still indented, and repeated indent works because blocks are line-based.
Fix: build `selection_after` with `apply_by_pos` the way duplicate/move do, or map range starts with a "before" bias for indent.

[uncertain] crates/vr-core/src/text.rs:250-255 — `is_line_break` and ropey `unicode_lines` count VT, FF, NEL, U+2028 and U+2029 as line breaks
Scenario: a PHP or C file with a form feed (C sources often have one), or U+2028 inside a string literal. The editor shows a line split there and its `Ln` numbers drift from `php`, `grep -n` and VS Code, which only treat LF, CRLF and CR as breaks. Text is preserved, so nothing is corrupted. It depends on whether the user wants strict LF/CRLF/CR semantics; D26-D30 and the spec do not record this choice.
Fix: either record it in decisions.md as intended, or disable ropey's `unicode_lines` (a manifest change, T1/leader) and shrink `is_line_break` to `\n` and `\r`.

Non-findings, probed and fine:
- Chunk-edge grapheme handling follows ropey's GraphemeCursor pattern, and the 3000-unit test pins it.
- CRLF is one break in motion, backspace, delete, line_content_end and line ops. The last line without a newline is handled in delete_line, duplicate and move.
- The empty buffer is a safe no-op in every motion and op.
- map_pos covers insert at cursor, delete spanning the cursor, and replace.
- Inverse changes are sorted and non-overlapping. Undo then a new edit drops the redo branch.
- Revision ids are unique per state, so equal revision means equal text. T4 must call `break_undo_group()` on save (noted by dev).
- Performance: per-keystroke work is O(log n). `char_at_display_col`, `char_to_pos` and `pos_to_char` copy the whole line to a `String`, which is O(line length) on vertical move and line ops. That is acceptable at 100k chars (about 1 ms) and not a finding.
- History is unbounded. Tolerable for the M1 scope.

## Files read
- crates/vr-core/src/lib.rs — read
- crates/vr-core/src/text.rs — read
- crates/vr-core/src/selection.rs — read
- crates/vr-core/src/transaction.rs — read
- crates/vr-core/src/history.rs — read
- crates/vr-core/src/motion.rs — read
- crates/vr-core/src/edit_ops.rs — read

## Rule check T2
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | only crates/vr-core/src/*; the `git status` check showed no other file |
| BASE-2 | pass | plan Contracts signatures kept; additive helpers listed in notes; `revision()` has D26; `Buffer` lives in text.rs (the plan file list has no buffer.rs) |
| BASE-3 | pass | red lines are recorded per test, but red = `todo!()` panics with the 25 tests written together and the code spliced in as a batch, which is weak evidence; the long_line test is declared a characterization test |
| BASE-4 | pass | notes carry the test and clippy output; both reproduced here |
| BASE-6 | pass | English, why-comments, no bug history |
| BASE-8 | pass | no manifest touched |
| BASE-16 | pass | Reuse table in the notes; capabilities.md updated |
| BASE-25 | pass | guard output in the notes; clippy -p vr-core reproduced clean |
| BASE-32 | pass | D1 non-modal insert, plan Offsets, D26-D30 honoured; the line-break set is the uncertain item above |
| AC-3 | pass | `ac3_type_delete_undo_redo_steps` and `revision_returns_to_saved_state_after_undo` |
| AC-10 (base) | pass | `apply_multi_change_one_undo`: 3 changes = 1 undo, history empty afterwards |
| Review Focus 1 | pass | `grapheme_motion_*`, `display_col_wide`, `long_line_*` cover combining marks, ZWJ, flags, CJK and tabs |
| Review Focus 5 (base) | pass | multi-change `Transaction` = one undo |
