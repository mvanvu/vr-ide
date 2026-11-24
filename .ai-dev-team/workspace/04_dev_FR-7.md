# FR-7 notes — flaky find debounce test

## Root cause
`draw()` (outside `handle_key_at`) calls `find::refresh`, which reads `app.now()`; with no event time it falls back to the real `Instant::now()` (correct in production). In `find_debounces_rescans_in_a_large_file` the editor-phase pending rescan is created only by that draw, so its `due` = real now + 150 ms. The test then ticked the fake clock at `t0 + 1300 ms`; when the test had already run > ~1.15 s of real time (debug build under load), `due` was after that tick and no rescan ran: `left (0, 2), right (2, 4)` at the last assert. Product code is consistent (real clock in production, injected only for key events); the test mixed clocks.

## Reproduce (red)
6 busy-loops (`timeout 100 sh -c 'while :; do :; done'`, load 10+): 3/3 runs failed with `left: (0, 2) right: (2, 4)`, ~1.6 s.

## Fix
crates/vr-ide/src/find_tests.rs only: the editor phase anchors its ticks to real instants taken before/after the draw (`before_draw + 100 ms` -> still old matches; `after_draw + 300 ms` -> rescanned). Still asserts: no rescan per editor key, no rescan before the pause, rescan after it (D124/D125). Nothing weakened; the earlier fake-clock phase is unchanged.

## Green
Same load (6 busy-loops, load 11-14): 10/10 `ok`. `cargo test -p vr-ide --lib find_tests::` -> 8 passed, 1 ignored. `cargo fmt --all --check` clean; `cargo clippy -p vr-ide --all-targets -- -D warnings` clean.

## Rule compliance
| Rule | Status |
|---|---|
| BASE-1,3,4,5,6,10,11,14,15,17,18,19,24,25,32,38, COST | pass |
| AC-10 | pass |
n-a: 0. Busy-loops were bounded by `timeout` and have exited; nothing left running.
