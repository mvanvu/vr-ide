# Review FR-23

No findings. The one `cargo test` run (`terminal_tests::selection theme::`) passed: 30 passed, 0 failed.

## COR-1 notes (no defects)
- `handle_mouse` drops a selection only when `in_step && !holds`. The wake path drops it when `!(in_step && holds)`, so the two agree.
- Wheel: `scroll_by` leaves the stored grid's `scroll_offset` different from `tab.scroll`, so `in_step` is false. The wheel branch clears the selection anyway.
- Hover below the cap: `scroll_offset < scrollback_len`, so `holds` is true and the selection stays.
- Drag or release after the drop: `selecting` stays true and the drag branch only writes `head` when `selection` is `Some`. This is the same behaviour as a drop from the wake path.
- Live view (offset 0): `in_step` is false, so a hover keeps the selection. The output flag is not cleared by the hover, so the next wake still drops it.
- Test oracle: without the fix the hover's `tab.refresh()` stores the slid grid, so the wake's `refresh_holds_view` compares slid against slid and keeps the selection. The test waits until `grid_now()` has slid, then hovers, so it covers that case. The test is sound as the discriminating proof. The developer's live replay could not reproduce the race, which is stated plainly in the notes.
- Gap (not a defect): there is no test that a hover below the cap or a wheel scroll keeps or drops the selection. Existing tests `selection_in_a_scrolled_view_survives_streaming_output` and `selection_is_dropped_when_its_lines_leave_the_scrollback` cover the wake path.

## UI-3 notes
- Dark picks are CodeNative 210, Media 213 and Generated 248. Hidden stays 250, so Generated and Hidden still differ.
- The new 3:1 check on `selection.bg` is added, and the existing 4.5:1 checks and `explorer_generated_and_hidden_differ_by_colour_in_every_theme` still pass.

## Files read
| File | Status |
|---|---|
| crates/vr-ide/src/commands/terminal.rs | read |
| crates/vr-ide/src/terminal_tests.rs | read |
| crates/vr-ide/src/theme.rs | read |

## Rule check FR-23
| Rule | Result |
|---|---|
| BASE-1 | pass |
| BASE-3 | pass (red log for both tests in the notes; the COR-1 test is discriminating by reading) |
| BASE-6 | pass |
| BASE-25 | pass |
| BASE-32 | pass (qa-artifacts/FR-23 leftovers are the developer's stated leave-over for the leader, not a running process) |
| BASE-37 | pass (live replay run and stated as non-discriminating) |
| BASE-47 | pass |

APPROVE
