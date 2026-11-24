# Review FR-7: APPROVED

No findings.

Anchoring: the editor-phase pending rescan is created only by `draw` (find_bar.rs:35 -> `find::refresh`, real clock, find.rs:143-146). Its due time D lies in [before_draw+150ms, after_draw+150ms]. `tick(before_draw+100ms)` is always before D, so the old matches stay. `tick(after_draw+300ms)` is always after D, so the rescan runs. Both hold at any load, because the instants are taken around the draw and not before the test started.

Oracle kept: "no rescan per editor key", "pause has not elapsed" (new) and "rescan after the pause" are all asserted. The fake-clock phase 1 is unchanged. Nothing weakened.

Mutations that still fail the test:
- No debounce, so `draw` rescans at once: `matches[0] == (2,4)` fails the "no rescan per editor key" assert.
- Debounce below 100 ms: the "pause has not elapsed" assert fails.
- Rescan never runs: the last assert fails.

Not run: the single test run was not needed. The dev's red/green evidence (3/3 red, 10/10 green under load) is in 04_dev_FR-7.md.

## Files read
- crates/vr-ide/src/find_tests.rs: read (the whole diff)
- crates/vr-ide/src/commands/find.rs: read, context only
- crates/vr-ide/src/app.rs (`now`, `handle_key_at`): read, context only

## Rule check FR-7
| Rule | Result |
|---|---|
| BASE-1 (only find_tests.rs changed) | pass |
| BASE-3 (red under load shown, green 10x) | pass |
| BASE-6 | pass |
| BASE-25 | pass |
| AC-10 | pass |
| Oracle (D124/D125 assertions kept; mutations above fail it) | pass |
