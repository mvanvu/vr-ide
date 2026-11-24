# T2 vr-core text, selection, transactions, history, edit ops
Skills called: ai-dev-team:implementation
- Progress: read brief-common, plan (Contracts vr-core, Decisions, Task T2, Review Focus 1/5), spec AC-3/AC-10/edge cases, T1 notes, capabilities (empty for vr-core). ropey 1.6.1 default features = unicode_lines (CR, LF, CRLF + Unicode breaks).
- Progress: waited for "T1 committed" (19dbd52) before touching lib.rs; manifests untouched.
- Progress: red — 25 tests written (selection, transaction, history, text, motion, edit_ops) against todo!() stubs; all 25 FAILED (not yet implemented).
```
red run: cargo test -p vr-core (stubs = todo!())
test edit_ops::tests::delete_line_last_line_no_newline ... FAILED
test edit_ops::tests::duplicate_line ... FAILED
test edit_ops::tests::indent_outdent_selection ... FAILED
test edit_ops::tests::move_line_up_down_bounds ... FAILED
test edit_ops::tests::toggle_comment_php ... FAILED
test history::tests::ac3_type_delete_undo_redo_steps ... FAILED
test history::tests::apply_multi_change_one_undo ... FAILED
test history::tests::revision_returns_to_saved_state_after_undo ... FAILED
test history::tests::undo_redo_groups ... FAILED
test motion::tests::extend_and_collapse ... FAILED
test motion::tests::grapheme_motion_combining ... FAILED
test motion::tests::grapheme_motion_emoji_zwj ... FAILED
test motion::tests::line_start_smart_home_and_end ... FAILED
test motion::tests::vertical_motion_keeps_display_column ... FAILED
test motion::tests::word_motion_unicode ... FAILED
test result: FAILED. 0 passed; 25 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test selection::tests::selection_maps_through_changes ... FAILED
test selection::tests::selection_merges_overlapping_ranges ... FAILED
test text::tests::crlf_text_treated_as_one_line_break ... FAILED
test text::tests::display_col_wide ... FAILED
test text::tests::insert_and_delete_ascii ... FAILED
test text::tests::multi_cursor_insert_and_delete ... FAILED
test text::tests::pos_roundtrip_uses_grapheme_columns ... FAILED
test transaction::tests::apply_then_invert_restores_text ... FAILED
test transaction::tests::map_pos_counts_chars ... FAILED
test transaction::tests::normalize_sorts_clamps_and_drops_overlaps ... FAILED
```
- Progress: green — implementations spliced in; cargo test -p vr-core 25 passed.
- Progress: added characterization test `long_line_grapheme_boundaries_match_across_chunks` (3000-unit single line, >10 rope chunks; passed on first run because it pins chunk-edge handling already implemented — not a red/green test).
- Progress: rustfmt on own files; clippy -p vr-core and workspace guard green; capabilities.md and decisions.md updated.

- Files changed: crates/vr-core/src/lib.rs (module list + re-exports, edited after "T1 committed"), new crates/vr-core/src/{selection,transaction,history,text,motion,edit_ops}.rs. No manifest touched.
- Tests added (26): selection::{selection_maps_through_changes, selection_merges_overlapping_ranges}; transaction::{apply_then_invert_restores_text, normalize_sorts_clamps_and_drops_overlaps, map_pos_counts_chars}; history::{undo_redo_groups, ac3_type_delete_undo_redo_steps, revision_returns_to_saved_state_after_undo, apply_multi_change_one_undo}; text::{insert_and_delete_ascii, multi_cursor_insert_and_delete, display_col_wide, pos_roundtrip_uses_grapheme_columns, crlf_text_treated_as_one_line_break, long_line_grapheme_boundaries_match_across_chunks}; motion::{grapheme_motion_combining, grapheme_motion_emoji_zwj, extend_and_collapse, vertical_motion_keeps_display_column, line_start_smart_home_and_end, word_motion_unicode}; edit_ops::{delete_line_last_line_no_newline, duplicate_line, move_line_up_down_bounds, indent_outdent_selection, toggle_comment_php}.
- Plan test names all present (`toggle_comment_php` covers indentation kept; Review Focus 1 = grapheme_motion_*, display_col_wide, long_line_*; Review Focus 5 / AC-10 base = apply_multi_change_one_undo).
- Contract: types and signatures as in plan Contracts. Additive helpers (BASE-2): Buffer::{revision, history, options, set_options, set_clock, break_undo_group}, Default/Debug for Buffer; BufferOptions; Range::{new, point, from, to, is_empty, map, clamp}; Selection::{new, single, point, ranges, primary, primary_index, len, is_empty, map, clamp, transform}; Change::new; Transaction::{new, with_selection}; transaction::{normalize, apply_changes, invert_changes, map_pos}; history::{Clock, SystemClock, ManualClock, EditKind, History, GROUP_IDLE}; text::{char_at_display_col, char_to_pos, pos_to_char, line_content_end, line_break_str, default_line_break, prev/next_grapheme_boundary, is_line_break}. `ropey` is re-exported from vr-core.
- Deviations: none in contract. Buffer lives in text.rs (plan file list has no buffer.rs; text.rs is "Rope wrapper"). Malformed transactions (unsorted/overlapping/out of range) are normalized (sorted, clamped, overlaps dropped with tracing::warn) instead of panicking.
- For T4: use `Buffer::revision()` (not `version()`) for `saved_version` so edit + undo back reads as not modified; call `break_undo_group()` on save. Selection-only changes bump neither version nor revision.
- For T5: `BufferOptions` must be set from settings (tab_width, indent_unit); `display_col` / `char_at_display_col` give render columns; `ManualClock` available for UI tests.
- Out-of-scope: vr-core manifest still warns `unused dependency` for notify-debouncer-full and thiserror (used by T4).

## Commands run (final)
```
cargo test -p vr-core
test result: ok. 26 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.13s
cargo clippy -p vr-core --all-targets -- -D warnings
    Checking vr-core v0.1.0 ... Finished `dev` profile  (only cargo manifest `unused dependency` warnings for T4 deps)
guard: cargo fmt --all --check            -> exit 0
       cargo clippy --workspace --all-targets -- -D warnings -> Finished `dev` profile, exit 0
```
Build: vr-core is a library; the test + clippy runs compile it (no separate binary output changed).

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| rope, graphemes, widths | capabilities.md (vr-core empty), workspace deps | built on ropey 1.6.1 + unicode-segmentation GraphemeCursor (chunk-aware) + unicode-width; capabilities.md updated with 6 lines |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | only crates/vr-core/src/* (git status); team files in workspace |
| BASE-2 | pass | plan Contracts types/signatures kept; additive helpers listed above; revision() logged in decisions.md |
| BASE-3 | pass | red block above: 25 tests FAILED on todo!() stubs, then green; long_line_* is a characterization test (noted) |
| BASE-4 | pass | Commands run (final) |
| BASE-6 | pass | English, why-comments only |
| BASE-8 | pass | no manifest edits; only workspace deps used |
| BASE-10 | pass | only `cargo test -p vr-core` |
| BASE-11 | pass | no stash/checkout/reset |
| BASE-16 | pass | Reuse table; capabilities.md vr-core section filled |
| BASE-17 | pass | no installs |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | progress lines above |
| BASE-24 | pass | no git add/commit |
| BASE-25 | pass | guard exit 0 |
| BASE-32 | pass | D1 non-modal (insert_text types), plan "Offsets" (char offsets, grapheme motions, display-width cols), D10 VS Code-like home/up/down/comment |
| BASE-38 | pass | `rustfmt --edition 2024` on the 7 vr-core files only |
| AC-3 | pass | ac3_type_delete_undo_redo_steps (text at each step, version grows), revision_returns_to_saved_state_after_undo |
| AC-10 (base) | pass | apply_multi_change_one_undo (3 changes, one undo, history empty after) |
| COST | pass | related tests only, no servers/watchers/background tasks; nothing left running |
n-a: BASE-5 (no workspace/release build needed for a library task), BASE-14, BASE-15 (no long or background commands), BASE-34 (no web sources)

## Quick pass 1
Items from 05_review_T2.md (1) normalize order, (2) debug_assert on overlaps, (3) indent keeps start at col 0. unicode_lines item left for T4 (D31).
- Red run (tests first):
```
test edit_ops::tests::indent_keeps_selection_start_at_column_0 ... FAILED   left: (4, 11) right: (0, 11)
test transaction::tests::normalize_overlap_asserts_in_debug_and_drops_in_release - should panic ... FAILED
test transaction::tests::normalize_result_does_not_depend_on_list_order ... FAILED   left: [Change{5,8,""}] right: [Change{5,5,"x"}, Change{5,8,""}]
test result: FAILED. 27 passed; 3 failed
```
- Old test normalize_sorts_clamps_and_drops_overlaps split: no-op/clamp/sort part kept as normalize_sorts_clamps_and_drops_noops; overlap part moved to the debug-assert test.
- After the normalize fix, the new guard test `text::tests::delete_with_cursor_touching_a_selection_does_not_overlap` went red (panicked at transaction.rs:69, the new debug_assert). Cause: `Buffer::delete` built a cursor's grapheme span that reached into a neighbouring selection. Fixed by trimming each span to the previous change's end.
- Fixes:
  - (1) crates/vr-core/src/transaction.rs `normalize` sorts by `(from, to)`.
  - (2) Same function: `debug_assert!(false, "overlapping change …")` next to the existing warn; release builds still drop the overlap.
  - (3) crates/vr-core/src/edit_ops.rs `indent` builds selection_after with `apply_by_pos`. The start endpoint of a non-empty range at column 0 stays at column 0; other endpoints on indented lines shift by the unit width; a single cursor still moves with the text.
  - Extra: crates/vr-core/src/text.rs `Buffer::delete` no longer emits overlapping changes (needed for (2)).
- Green:
```
cargo test -p vr-core -> test result: ok. 30 passed; 0 failed
cargo clippy -p vr-core --all-targets -- -D warnings -> Finished, exit 0
rustfmt --edition 2024 on transaction.rs, text.rs, edit_ops.rs only
```
- not-run: the release-mode branch of `normalize_overlap_asserts_in_debug_and_drops_in_release`. A release test build was skipped (heavy, not requested). `cfg_attr(debug_assertions, should_panic)` makes the same test assert the drop in release.
- Rule rows touched: BASE-3 pass (red lines above), BASE-4 pass (green output above), BASE-38 pass (rustfmt on 3 own files). No Cargo.toml/Cargo.lock edits, no git commands.
