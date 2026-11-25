# Review FR-13

Verdict: APPROVED

## Files read
- crates/vr-syntax/tests/highlight.rs (diff): read
- crates/vr-ide/src/recent_files.rs (diff, test module only): read

## Findings
None blocking.

[minor] product question (not a defect of this task) — a `spans()` call cut at the 50 ms query timeout is not signalled to the caller. `Snapshot::spans` (vr-syntax highlighter.rs ~1340) just returns what `paint` painted before the deadline. `SyntaxState::spans` (vr-ide syntax.rs:179) and the draw path (ui/editor_view.rs:417-421) have no "cut" flag, and redraw is triggered only by a worker publishing spans or by input. So a cut frame keeps partial colours until the next input or edit. Rare, since it needs more than 50 ms per viewport query. Suggest a low-priority follow-up task (return a `truncated` flag and schedule one redraw), not part of FR-13.

## Judgement
- Root cause proof: sufficient. The deterministic replay of the failing text shows the same text with a 60 s query budget gives 45 spans, containing all 23 "only incremental" spans. With a 0 budget it gives 11 spans, missing exactly those 23. That fits "fresh side cut by the 50 ms budget". The repro under load did not fail again (1 in 8000 checks), but the replay is a direct proof. The parse_timeout leak was found separately with a reproduced failure.
- Oracle stays meaningful: `incremental_equals_full` still compares the incremental highlighter and a fresh one on full spans. Both now use the same unbounded options, so the comparison is on complete results. The worker logic (incremental, verify, idle) is unchanged.
- Limits keep their tests: `spans_call_is_bounded_by_the_query_timeout` (0) at line 1250, `parse_timeout_stays_plain_and_reports` (1 µs) at line 813, and the timeout-reset test at line 1223. Perf tests keep the product defaults. `options()` is applied to the oracle helpers only.
- recent_files: the change is test-only. `close()` (line 78) uses `recv_timeout`, which returns as soon as the writer answers, so the 30 s bound costs nothing on the normal path and only caps a stuck writer. Sound.
- Spot-run `cargo test -p vr-ide recent_files`: 4 passed.

## Rule check FR-13
| Rule | Status | Evidence |
|---|---|---|
| BASE-10 related tests only | pass | notes list vr-syntax tests and the named tests; recent_files rerun 4/4 ok |
| BASE-16 reuse | pass | reuses PARSE_WAIT and the large-parse_timeout pattern |
| BASE-19 notes from start | pass | 04_dev_FR-13.md has progress, root cause and evidence |
| Rule compliance section present | pass | present, evidence matches the diff |
| Scope (test-only) | pass | diff touches only tests/highlight.rs and the recent_files test module |
| No product behaviour change | pass | no src change outside `#[cfg(test)]` |
