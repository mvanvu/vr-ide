# Review FR-28 (palette detail area) - CHANGES_REQUESTED

Tools available: Read, Bash, Write, Skill, SendMessage, SubagentHandback. Ran `cargo test -p vr-ide --lib -- palette` once via run-heavy.sh: 24 passed.

## Findings

[major] crates/vr-ide/src/ui/palette.rs:206-216 (row_fits) and :53-57 — a cut title is never readable
Scenario: Go to File, a root-level file with a long name in a narrow box (`detail` = "" so row_fits returns true), or a file whose name is cut but whose folder `detail` is non-empty. In the first case no area shows. In the second the area shows only the folder, not the name. Same for a slash row: the title ends in "…" (D270) and the area shows only the description, never the full title. At 40x12 the box is 36 wide, so this is reachable. The user complaint was text cut with nowhere to read it. Here the cut text is the title.
Fix: make `row_fits` also require `title_w <= width` (no detail then means fits only when the title fits). When not fitting, the area text is the whole row text, `title  detail  keys`, wrapped by `wrap_cut`. For a file that is `name  folder`.

[major] crates/vr-ide/src/ui/palette.rs:206-216 — for ordinary command rows the area shows only the command id (noise, and the layout jumps)
Scenario: 40x12, query "sa". "Save (overwrite)" is fully visible; only its id `file.save_overwrite` is hidden for lack of room. The area opens with the id (see the changed snapshot). The title is the thing a user reads, and the id was never promised as a description. Because the area depends on the selected row, the list shrinks by 2 rows on this row and grows back on "Save As" (id fits). Arrowing down therefore resizes the box and re-scrolls the list (the snapshot lost "Replace in Project" and "Search in Project"). That is jitter, against "minimal, nvim-like density".
Fix: for non-slash command rows treat the row as fitting when the title (plus keys) fits; the id is secondary. Show the area only when the title, or a real description (slash text, file folder), is cut. If the layout should not jump, reserve the area's rows for the whole palette session at constrained sizes, or let it overlay the last list rows without changing `list_rows`. Decide with the user which one; I recommend no resize.

[minor] crates/vr-ide/src/commands/palette.rs:229-231 — PageUp/PageDown step uses `list_area.height`, which now depends on the previous selection. The step varies from page to page. It follows from the finding above; resolved by that fix or the reserved-height option.

[minor] crates/vr-ide/src/ui/tests/palette_detail.rs — the tests use short titles ("/long", "/a"), so they never cover a cut title, a file row, a keys-bearing row, an error line plus detail, or the 40x12 list-height bounds (selected row visible, `top` valid). The "hidden when it fits" test asserts `─` absent, but a theme border could also match. Add cases for a long file name at 40x12 and a selected row near the end of a long list.

## Checked and fine
- Height maths: `spare = room - (3 + err + 1)`, `detail_lines <= spare - 1`, so `detail_rows <= room - 4 - err` and `list_rows >= 1`. The input line and the selected row stay. The `top` fix-up runs after `list_rows` is known, so the selection is visible and `top` stays valid at 120x40, 80x24 and 40x12.
- Click: `list_area` excludes the separator and detail rows, and the area stays inside `p.area`. A click on a row accepts it, as before. The wheel and keys go through `move_selection`, and the area is recomputed every frame. `wrap_cut` truncates with "…" within the width. The `dialog::wrap` hard-wrap handles long words.
- README line added; `mod palette_detail` is registered in ui/mod.rs.

## Design QA
- 40x12 snapshot: separator and area are dim, aligned to the text column, and the box fits above the status row. Contrast is the same as the dim detail used in rows. The density is acceptable except for the resize jitter and the id noise above. The area content is a dev id, not what the user asked to read.
- 80x24 and 120x40 are covered only by the "contains TAILWORD" test, not by a snapshot. I judged them from the maths and the test, not from images.

## Files read
| File | Status |
|---|---|
| crates/vr-ide/src/ui/palette.rs | read |
| crates/vr-ide/src/ui/mod.rs | read |
| crates/vr-ide/src/ui/tests/palette_detail.rs | read |
| crates/vr-ide/src/ui/snapshots/vr_ide__ui__tests__render_palette_40x12.snap | read |
| README.md | read |
| crates/vr-ide/src/commands/palette.rs (callee: mouse, keys, PaletteItem) | read |
| crates/vr-ide/src/ui/dialog.rs (wrap) | read |

## Rule check FR-28
| Rule | Result | Note |
|---|---|---|
| BASE-1 | pass | Behaviour matches AC-P1 for rows with a description; see the findings for the gaps. |
| BASE-3 | pass | The tests pass. |
| BASE-6 | n-o | No contract change. |
| BASE-16 | pass | Reuses `dialog::wrap`; `row_fits` repeats the row's own width maths (palette.rs:152-159 and :206-216). Extract one helper when fixing. |
| BASE-23 | pass | No new error path. |
| BASE-25 | pass | README updated. |
| BASE-32 (D322) | fail | A cut title is not readable (finding 1), and the area shows a command id instead of the full row text. |

CHANGES_REQUESTED

## Re-check round 1
Ran `cargo test -p vr-ide --lib -- palette` once: 30 passed.

Earlier findings:
- Cut title not readable (major): fixed. `RowFit::is_cut` includes `title_w > width`, and `row_text` shows title, description and keys. Tests cover a cut root file name, a cut folder-file, a cut slash title and cut keys.
- Command id as the description and box resize (major): fixed. `row_text` and `is_cut` skip the id (`item.name` non-empty), and the detail rows are reserved from `any_cut` over the whole list, so arrowing keeps the box and list size. `arrowing_never_resizes_the_box` covers the three sizes (area and list_area equal, selection visible, input and status row kept). That also fixes PageUp/PageDown (`list_area.height` stable) and the BASE-16 duplicate (`RowFit` is shared).
- Tests (minor): fixed. The hidden assertion now checks `below_list == 0`.
- No new defect found. `reserved <= spare-1`, so `list_rows >= 1`. `top` is fixed up after the height is known.

[minor] crates/vr-ide/src/ui/palette.rs:57-62 — the reservation is always 3 detail lines plus the separator, however much the cut rows need. Scenario: 40x12, query "sa" (the changed snapshot). The selected row fits, yet 4 of the 10 box rows hold a separator and 3 blank lines. The list shrinks from 7 rows to 3, and the box shows an empty gap. Reserved space is allowed by D323, but its size is not.
Fix: reserve `min(MAX_DETAIL_ROWS, max wrapped-line count over the cut rows)`, using `wrap_cut` per cut row. The usual case, a one-line overflow, then costs 2 rows, not 4. This stays within D323 because the value changes only with the filter. It is taste, so the leader or user decides; it is not a blocker.

Density verdict on the 40x12 snapshot: acceptable as a stable rule, but the blank 3-line gap is the weak point. It reads heavy for "minimal, nvim-like density"; the max-needed reservation above removes most of it.

Files read this round: ui/palette.rs, the 40x12 snapshot, ui/tests/palette_detail.rs (all read in full).

APPROVE

## Re-check QP1
Read the `need` computation in ui/palette.rs, the 40x12 snapshot and the new test (I did not rerun the tests).
- `need` is the largest wrapped line count (capped at 3) over the cut rows, independent of the selection, so arrowing still never resizes the box. `reserved = need.min(spare-1)` keeps the input line and `list_rows >= 1`.
- A cut row's `row_text` is wider than the row, so it wraps to at least 2 lines. The minimum reservation is separator + 2, as the developer says.
- The "sa" snapshot at 40x12 now shows 4 list rows and 3 reserved rows (separator + 2 blank), against 3 and 4 before. A gap remains while the selected row fits. That is the cost of D323's no-resize rule and acceptable.
- The new test checks 3 reserved rows for a two-line need and 4 for a three-line need.
- No new finding.

APPROVE
