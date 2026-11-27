# FR-28 notes
- ui/palette.rs: detail area (separator + <=3 wrapped lines, dim) under the list when the selected row does not show its detail in full; shrinks list rows, never the input/selected row; truncated by room.
- ui/tests/palette_detail.rs: 3 tests (3 sizes, cut + selection follows, hidden when fits). Red first: 2 failed before the change (tail not on screen).
- Snapshot changed: crates/vr-ide/src/ui/snapshots/vr_ide__ui__tests__render_palette_40x12.snap (only one).
- README: one line.
- `cargo test -p vr-ide --lib -- palette ui::` via run-heavy: 84 passed; clippy vr-ide lib clean.

## Guard (BASE-25), final code
`run-heavy.sh sh -c 'cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings'`
```
    Checking vr-search v0.1.0 (.../FR-28/crates/vr-search)
    Checking vr-ide v0.1.0 (.../FR-28/crates/vr-ide)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.29s
```
fmt clean, clippy zero warnings.

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | diff: ui/palette.rs, ui/mod.rs (mod line), ui/tests/palette_detail.rs, one 40x12 snapshot, README line |
| BASE-3 | pass | palette_detail tests failed before the code (tail word not on screen), pass after |
| BASE-4 | pass | related run `cargo test -p vr-ide --lib -- palette ui::`: 84 passed, 0 failed; guard output above |
| BASE-16 | pass | reused ui::dialog::wrap and ellipsize in ui/palette.rs; nothing new for capabilities.md |
| BASE-23 | pass | palette_detail.rs: `/` rows at 3 sizes; selection by key (Down) and by selected index (click path sets p.selected, area recomputed each render); command and file rows share the same render path |
| BASE-25 | pass | guard output above |
| BASE-32 | pass | D322 honoured: detail area <=3 lines, dim, thin separator, hidden when it fits |
| COST-8 | pass | edits via Edit tool |
| COST-15 | pass | snapshots regenerated once after code final; only render_palette_40x12.snap changed |
| AC-P1 | pass | palette_detail.rs full_description_is_readable_at_three_sizes, detail_is_cut_to_three_lines_and_follows_selection, detail_area_is_hidden_when_the_text_fits |
| USER-2 | pass | compact dim area, input and selected row kept at 40x12 (test) |
| USER-3 | pass | built directly, light tier |

n-a: BASE-2, 8, 9, 20, 21, 22, 26, 27, 28, 29, 30, 31, 33, 34, 36, 39, 40, 41, 43, 44, 47 (no contract, dependency, security surface, data, schema, external system or value-fix in this task) · BASE-5, 14, 15, COST-1, 3, 5, 9 (heavy runs went through run-heavy.sh; nothing started that stays running; no polling) · BASE-6, 7, 11, 12, 13, 24 (English only, no commit/index/shared-tree/outside-repo/deletion actions) · BASE-10, COST-2, 4, 6, 7, 10, 11, 12, 13, 14 (only related tests, no suite/audit/e2e/browser; no throwaway scripts needed) · BASE-17, 18, 19, 35, 37, 38, 42, 45, 46, 48 (no toolchain change, no sub-agents; notes written, formatters ran only on my two files, no runtime defect repro, no machine state claims, AC-P1 is in the brief)
n-a: COST-16, 17, 18, 19, 20 (tail output only, no snapshot merge, no full run, no qc measurement set, no re-check round)

## Fix round 1 (D323)
- palette.rs: one `RowFit` helper shared by row render and the cut check; `is_cut` = title wider than the row, keys hidden, or a real description (not a command id) hidden. Detail text = whole row (`title  description  keys`), wrapped <=3 lines with `…`.
- Detail rows (separator + 3) are reserved while any listed row is cut, so arrowing never resizes the box; PageUp/PageDown step (list_area.height) is therefore stable. Capped at the room so input and selected row stay.
- Tests (palette_detail.rs, 9): cut file name root-level and with folder, cut slash title, command id never shown, cut keys shown, arrowing keeps area/list_area at 3 sizes with selected row visible and status row clear, error line + area, no area rows when all fit (below_list == 0). Red first: 4 of the new ones failed before the code.
- Snapshot regenerated once at the end; changed: crates/vr-ide/src/ui/snapshots/vr_ide__ui__tests__render_palette_40x12.snap (80x24 unchanged).
- Related `cargo test -p vr-ide --lib -- palette ui::` (run-heavy): 90 passed. Guard (fmt --all --check + clippy --workspace --all-targets -D warnings, run-heavy): clean.

## Quick pass 1
- palette.rs: reserved detail lines = max wrapped lines (<=3) over the cut rows of the current filtered list, still independent of the selection (D323); capped by the room.
- Test the_area_reserves_only_the_lines_a_cut_row_needs: red first (left 4, right 3), then green; a list with a 3-line row still reserves 3 (+ separator).
- Snapshot regenerated once: crates/vr-ide/src/ui/snapshots/vr_ide__ui__tests__render_palette_40x12.snap (only one changed).
- Related `-- palette ui::` via run-heavy: 91 passed. Guard (fmt --all --check + clippy --workspace --all-targets -D warnings): clean.
