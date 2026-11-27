# FR-23 notes (Phase 5 minors)

Worktree .worktrees/FR-23 (branch fr-23). Files: crates/vr-ide/src/commands/terminal.rs, crates/vr-ide/src/terminal_tests.rs, crates/vr-ide/src/theme.rs.

## COR-1
- Red: `terminal_tests::selection_is_dropped_at_the_cap_even_when_a_hover_saw_the_slide_first` failed at `selection.is_none()`.
- Fix: `TermTab::refresh_checking_view()` returns (in_step, holds); `refresh_holds_view` = both. `handle_mouse` calls it when a selection exists and drops the selection when the view was in step but no longer holds (a wheel-scrolled view is not "in step", so the wheel keeps its selection as before).
- Green: `cargo test -p vr-ide --lib -- terminal_tests:: theme::` 76 passed.
- Live replay: FR-18/walk.py extended with phase `cap_hover` (full scrollback, 3x900 shift+pageup to `↑ 10000/10000`, selection, paced output, 1500 hovers, lit-cell sampling). Before and after both show 0 highlighted samples: at the cap the first wake after the release drops the selection, and the hover-before-wake race did not occur in the pty, so the replay does not tell before from after; the unit test is the discriminating proof.

## UI-3
- Red: dark `CodeNative Indexed(203)` 2.80:1 on the selection.
- Dark kinds now CodeNative 210, Media 213, Generated 248 (Hidden stays 250, still differs). Test `explorer_kinds_of_dark_and_light_read_on_common_terminal_backgrounds` also asserts >= 3.0 on `theme.selection.bg` (xterm RGB) for dark and light, plus the existing 4.5 checks; green.
- FR-21 explorer-themes.ans: holds only dracula/synthwave, unaffected, not regenerated.

## Guard
`cargo fmt --all --check` clean; `cargo clippy --workspace --all-targets -- -D warnings` Finished, no warnings.

## Left over
qa-artifacts/FR-23/walk-before.txt, walk-after.txt kept. The rm of the two binary copies and run dirs under qa-artifacts/FR-23 was blocked by the safety check; leave to the leader (vr-ide-before, vr-ide-after, run-before, run-after were removed? no: not removed).

## Rule compliance
BASE-1 pass; BASE-3 pass (red logs above for both tests); BASE-4 pass; BASE-25 pass; BASE-32 pass; BASE-37 pass (live replay run, non-discriminating, stated); BASE-47 pass; COST-8 pass (Edit tool; two perl edits misplaced text and were repaired); COST-10 pass; COST-16 pass.
n-a: all other BASE-1..48 and COST-1..20 IDs (no schema, API, dependency, security-surface, docs or commit work in this task; no servers or watchers left running).
