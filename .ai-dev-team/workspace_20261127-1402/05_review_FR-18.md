# Review FR-18 (AC-F2)
Tools: Skill, Bash, Read, Write, SubagentHandback.

## Findings
[minor] crates/vr-ide/src/commands/terminal.rs:795 - output still clears the mouse selection
Scenario: scrolled view while a program streams, user drags a selection -> each output wake sets `selection = None`, so the selection is lost every frame (the dev notes flag it as out of scope). Not part of AC-F2 (view holds; selection coordinates map correctly because the snapshot offset is the emulator's).
Fix: clear the selection only when the grid scrolls under it (offset unchanged and scrollback_len unchanged), or leave as a follow-up FR.

No blocker or major findings.

Checked: locking (scroll/scroll_to_bottom/snapshot take the same lock via `term()`, no I/O under it, clamp to +-2^30 avoids i32 overflow); alt screen (shift+pgup/pgdn skipped, wheel sent as arrows, unchanged); resize while scrolled (emulator clamps, mirror updated in `TermTab::refresh`); selection copy uses a snapshot at the same offset; mouse reporting uses `tab.scroll == 0` after `tab.refresh()`; `restart` makes a new Terminal (offset 0). All `snapshot(` callers updated (grep). `cargo test -p vr-term --test pty scrolled` passed once; walk-before FAIL / walk-after PASS read.

## Files read
| File | Status |
|---|---|
| crates/vr-term/src/terminal.rs | read |
| crates/vr-term/src/snapshot.rs | read |
| crates/vr-term/tests/pty.rs | read |
| crates/vr-term/tests/flood.rs | read |
| crates/vr-term/tests/common/mod.rs | read |
| crates/vr-ide/src/commands/terminal.rs | read |
| crates/vr-ide/src/terminal_tests.rs | read |

## Rule check FR-18
| Rule | Result | Note |
|---|---|---|
| BASE-1 | pass | 7 files, all scroll related |
| BASE-2 | pass | internal API change, every caller updated (grep) |
| BASE-3 | pass | red.log: 2 tests failed before fix |
| BASE-6 | pass | comments in English, why-only |
| BASE-25 | pass | guard.log exit 0 (dev evidence) |
| BASE-32 | pass | D315: only key/paste/typed line returns to bottom, output keeps view; D227/D207 wheel/mouse routing unchanged |
| BASE-37 | pass | walk-before False / walk-after True on real binaries |
| BASE-47 | pass | renderer, indicator, cursor, mouse reporting, selection copy, resize, restart, alt screen all read from the emulator offset or mirror |

APPROVE

## Re-check QP1
Earlier minor (selection cleared on every output wake): fixed. `cargo test -p vr-ide --lib terminal_tests::selection` -> 2 passed.

Correctness of a kept selection (`refresh_holds_view`, terminal.rs:166-180 and refresh:812):
- Offset clamp at the cap: below the top the emulator adds exactly the pushed lines to the offset (anchored), so rows keep their lines; at offset == scrollback_len the clamp may have dropped lines, and then only an unchanged screen keeps the selection. Correct and conservative.
- Resize: size change fails the `(rows, cols)` check, and `fit` already clears the selection. Tab switch and panel hidden: cleared (`p.visible` check, existing clears). Alternate screen entered: alt grid offset is 0 -> `g.scroll_offset < old` -> dropped. History clear / scroll to 0: offset decreases -> dropped.
- Wheel and shift+pageup move the view and already clear the selection; `in_step` (`grid.scroll_offset == tab.scroll`) also covers a stale mirror.
- Cost: one grid clone plus one snapshot per wake, only while a selection exists in a visible panel; the next frame's snapshot is then the same work already needed. Acceptable.
- BASE-47 list in the notes covers every selection reader; I found no other.

[minor, not blocking] terminal.rs type_line (~1062): `scroll_to_bottom` without clearing the selection, unlike `paste`. The selection is dropped on the first output wake (in_step false), and typing a line echoes output, so the stale highlight lasts one frame at most. Optional: clear it there too.

APPROVE
