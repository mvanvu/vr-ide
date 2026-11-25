# FR-14 developer notes

## Progress
- read FR-13 review/dev notes, Highlighter::spans / Snapshot::spans, Syntax::spans, editor_view::visible_spans, event loop wake path
- repro: syntax_tests::a_frame_cut_by_the_query_budget_asks_for_one_follow_up red before wiring (panicked: no redraw asked after a cut frame); vr-syntax SpanQuery/query/set_query_timeout added
- item 1 fixed (Syntax::frame_drawn + cut flag; ui::render_with calls it); item 2 tests rewritten; item 3 route_mouse takes the pair on every event but Up/Moved; mutation check: keeping the pair fails 'drag in between'
- load run 1 (16 yes, ambient load ~16 on 8 cores): first frame 2.37-3.14 s in all 5 rounds (old <2 s limit fails = repro); burst span 145-229 ms, 1 event; drop_nested best-of-7 wall ratio failed round 5 (68 ms -> 1.10 s): wall-clock min still biased (short runs fit one time slice) -> switch to thread CPU time
- load run 2 (CPU-time clock): drop_nested best CPU 56-59 ms -> 246-252 ms (ratio ~4.3) all rounds; first frame 1.7-3.2 s wall; burst span up to 1.08 s (2 events) in round 4; all 5 rounds green; probes removed
- final load run 3 (probe-free code, 16 yes, load avg 33-48): 5/5 rounds green for the three tests; yes pids killed by the script trap, pgrep count 0
- running: cargo test -p vr-ide -p vr-syntax -p vr-core via run-heavy (background b7cf1dmdk)
- `cargo test -p vr-ide -p vr-syntax -p vr-core` (run-heavy): all ok — vr-core 155 passed / 2 ignored, vr-ide 543 passed / 7 ignored, vr-syntax tests 24+25+27(5 ignored)+6 passed, 0 failed
- fmt --check OK; clippy --workspace --all-targets -D warnings: Finished, no warnings

## 1. Cut viewport query → one follow-up redraw (D162)

Repro: `syntax_tests::a_frame_cut_by_the_query_budget_asks_for_one_follow_up` (query budget 0) was red before the fix
(`panicked at syntax_tests.rs:482: no redraw asked after a cut frame`): the cut frame had fewer coloured cells and nothing asked for another frame.

Root cause: `Snapshot::spans` stops at the deadline and returns what it painted; neither `Highlighter::spans`, `Syntax::spans`
nor `editor_view::visible_spans` knew it was cut, and the event loop redraws only on input or a worker wake.

Fix:
- vr-syntax (additive): `SpanQuery { spans, truncated }`, `Highlighter::query(range) -> SpanQuery`, `Highlighter::set_query_timeout`;
  `spans()` = `query().spans` (unchanged contract). Exported from lib.rs.
- vr-ide `Syntax`: `spans()` uses `query()` and records a cut in an `AtomicBool`; `Syntax::frame_drawn()` (called at the end of
  `ui::render_with`) sets the existing wake flag once when the frame was cut, unless that frame was itself the follow-up
  (`follow_up` flag). So: at most one extra frame per frame caused by something else; a document whose every query is cut
  does not redraw in a loop. Test-only `Syntax::set_query_timeout`.
- Tests: vr-syntax `a_query_cut_by_its_budget_says_so` (0 budget → truncated; restored → not truncated, more spans);
  vr-ide test above: partial frame → redraw asked → cut follow-up asks nothing → key frame gets its own follow-up →
  budget restored → follow-up draws the full colours (same coloured-cell count as the settled frame) and asks nothing.

## 2. Wall-clock tests under load

Root cause (measured with temporary probes under 16 `yes` + ambient load ~16 on 8 cores): the asserted quantity was the
machine's speed, not the property. First frame took 1.7–3.2 s wall (old limit 2 s: fails most rounds); the watch burst of
20 writes "~100 ms" took up to 1.08 s (old "1–2 events" holds only for ~100 ms bursts); drop_nested wall time varies with
time slices. Changes:
- `syntax_tests::the_first_frame_does_not_wait_for_the_parse`: checks order instead of time — after the first frame the
  highlighter is still not ready (`wait(0, ZERO)` false) and the frame is plain. A frame that waited for the parse would
  return with it landed (4.8 MB debug parse takes seconds; open + frame milliseconds; load stretches both).
- `explorer_tests::drop_nested_keeps_order_and_scales`: correctness on 40 001 paths kept; scaling measured as the ratio of
  5k vs 20k paths in this thread's CPU time (`/proc/thread-self/schedstat`, wall clock fallback), best of 5 interleaved
  runs, must be < 10 (n log n ≈ 4–5, quadratic 16). First try with wall-clock best-of-7 still failed once under load
  (68 ms → 1.10 s), hence CPU time; measured ratio then 4.3 every round.
- `vr-core watch::tests::watch_debounces_burst`: the bound follows the measured burst span: 1..=span/DEBOUNCE+2 events
  (= 1–2 on an idle machine as before, since the debouncer emits one event per DEBOUNCE window from the first unreported
  change); plus "coalesced" (< 10 events for 20 writes) whenever every pause was shorter than DEBOUNCE.

Evidence: final run on probe-free code, 16 `yes` started and killed by pid in the script trap
(scratchpad load5.sh), load average 33–48: 5/5 rounds all three tests ok; `pgrep -c -x yes` = 0 afterwards.

## 3. T25 minors (app.rs route_mouse)

- The pending first press (`last_tab_click`) is taken at the start of every mouse event except `Up` and `Moved`; only a left
  press on a tab sets it again. So a drag, the wheel or a press anywhere else (editor, explorer, dialog, terminal, notice,
  empty tab-bar space) breaks the pair; a move or the release does not.
- Overflow: the pair is matched by tab index (identity), not by column. Test `a_tab_scrolled_by_the_first_press_still_takes_the_second`:
  first press on the clipped last tab scrolls the bar (its x changes), the second press on that tab at its new place
  reveals it. Interpretation note: a second press at the OLD column that now lands on a different tab is a single click
  on that tab (no reveal) — kept, since the pair is by identity.
- Test `a_drag_the_wheel_or_a_press_elsewhere_between_two_tab_presses_breaks_the_pair` (drag, wheel, editor press,
  and a move that keeps the pair). Mutation check: keeping the pair on every event fails it with "drag in between".

## Reuse
| need | reused |
|---|---|
| redraw request | existing `Syntax::wake` flag polled by `event_loop::run` (no new loop hook) |
| mouse tests | `tab_click`, `two_tabs`, `folders`, `ws_app`, `draw` helpers of workspace_ui_tests |

## Files
- crates/vr-syntax/src/highlighter.rs, crates/vr-syntax/src/lib.rs, crates/vr-syntax/tests/highlight.rs
- crates/vr-ide/src/syntax.rs, crates/vr-ide/src/ui/mod.rs, crates/vr-ide/src/app.rs
- crates/vr-ide/src/syntax_tests.rs, crates/vr-ide/src/explorer_tests.rs, crates/vr-ide/src/workspace_ui_tests.rs
- crates/vr-core/src/watch.rs
- full suite `cargo test --workspace` (run-heavy): exit 0, 948 passed, 0 failed, 17 ignored
- `cargo build --release` (run-heavy): exit 0, Finished release in 51.09s
- nothing left running: load generators killed by pid (trap), no servers/watchers started; background runs finished

## Rule compliance
| rule | status | evidence |
|---|---|---|
| BASE-10 related tests + mapped guards only | pass | per-crate tests, then full suite once as asked |
| BASE-14 no sleep/poll waits | pass | long runs in background, resumed on notification |
| BASE-15 stop only own processes | pass | 16 `yes` killed by recorded pid; user's vr-ide untouched |
| BASE-16 reuse | pass | `## Reuse` table |
| BASE-17 no SDK installs | pass | no new dependency (schedstat read via std::fs) |
| BASE-18 no own agents | pass | none started |
| BASE-19 notes + progress lines | pass | this file |
| COST heavy via run-heavy | pass | test runs, full suite, release build |
| COST-9 nothing left running | pass | pgrep yes = 0 |
| contracts / additive API | pass | `spans()` unchanged; `SpanQuery`, `query`, `set_query_timeout` added |
| no commit / stash / checkout | pass | working tree only |
| secrets / input validation | n-a | no boundary input or secrets touched |
| UI SWEEP | n-a | no screen layout change (one extra frame only) |
