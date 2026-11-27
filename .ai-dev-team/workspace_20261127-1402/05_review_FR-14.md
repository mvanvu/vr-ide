# Review FR-14

Verdict: APPROVED (no blocker/major findings; one minor note)

Tools available: Read, Bash, Write, Skill, SendMessage, SubagentHandback.

## Findings
[minor] crates/vr-ide/src/syntax.rs:frame_drawn — a follow-up frame that is itself cut stays partially coloured until the next input or worker wake.
Scenario: every query cut (very slow machine, huge viewport) -> two frames, then partial colours at idle. This is the intended no-loop trade-off; noted only.
Fix: none required.

Checked
1. vr-syntax: SpanQuery, query(), set_query_timeout() additive; spans() = query().spans, contract unchanged. Syntax::cut is an AtomicBool (spans takes &self, release store, acqrel swap in frame_drawn on &mut self). frame_drawn: cut && !follow_up asks once (wake + follow_up=true); a cut follow-up asks nothing and clears follow_up; no loop, idle CPU zero after at most one extra frame.
2. Timing tests: first-frame test now asserts order (highlighter not ready right after the frame, plain cell colour) - still fails if the frame waited for the parse. drop_nested: CPU-time ratio 5k vs 20k < 10 (n log n ~4-5, quadratic ~16), correctness kept. watch debounce: bound 1..=span/200ms+2 and, if all gaps < DEBOUNCE, < 10 events; with no debouncing 20 events fail both on an idle machine and under any realistic load (would pass only if the burst took ~4 s). Still proves debouncing.
3. route_mouse: pair taken on every event except Up/Moved, set again only by a left press on a tab; matched by tab index; old-column-different-tab = plain click (accepted). Tests cover drag, wheel, editor press, move keeps pair, scrolled overflow tab.

Spot runs: vr-ide syntax_tests 18 passed (1 ignored); vr-core watch 20 passed; vr-syntax a_query_cut 1 passed.

## Files read
crates/vr-core/src/watch.rs read; crates/vr-ide/src/app.rs read; syntax.rs read; ui/mod.rs read; syntax_tests.rs read; explorer_tests.rs read; workspace_ui_tests.rs read; crates/vr-syntax/src/highlighter.rs read; lib.rs read; tests/highlight.rs read.

## Rule check FR-14
| rule | result | note |
|---|---|---|
| BASE-10 related tests only | pass | per-crate runs then one full suite, stated in notes |
| BASE-14 no sleep/poll waits | pass | notes |
| BASE-15 stop only own processes | pass | yes pids killed by pid; user's vr-ide untouched |
| BASE-16 reuse | pass | existing wake flag, test helpers |
| BASE-17 no SDK installs | pass | no new dependency |
| BASE-18 no own agents | pass | none |
| BASE-19 notes + progress | pass | 04_dev_FR-14.md |
| COST heavy via run-heavy | pass | per notes |
| COST-9 nothing left running | pass | pgrep 0 per notes |
| contracts / additive API | pass | verified in diff |
| tests for changed behavior | pass | red-first test, mutation check |
