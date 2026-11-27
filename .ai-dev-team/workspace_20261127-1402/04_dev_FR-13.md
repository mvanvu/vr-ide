# FR-13 — flaky highlight oracle under load + recent_files close test

## Progress
- read failure output qa-artifacts/T23/qc/syntax_rerun.txt, test oracle (tests/highlight.rs), Highlighter::spans / Snapshot::spans, D162–D170
- hypothesis: `spans()` is bounded by `query_timeout` = 50 ms on the caller's thread (D162, returns what it has); the oracle queries both highlighters with that budget, so under load (debug build) a query is cut short and the oracle compares a truncated list. Fresh side strict subset ("only fresh []") fits.
- repro run 1 (16 `yes` + ambient load, seed 12648430) started; temporary DIAG print added to fuzz_against_fresh (removed before done)
- repro run 1 (16 `yes`, 8 cores, seed 12648430, 200 steps): passed in 405 s — the cut is rare (1 of 8000 checks in QC's run)
- repro run 2 (6 `yes` + test pinned to 2 cores, 40 steps): passed in 48 s
- deterministic proof (temporary test, removed): the failing text from syntax_rerun.txt, fresh highlighter with query_timeout 60 s → 45 spans and contains all 23 "only incremental" spans; with query_timeout 0 → 11 spans ending at byte 70, missing exactly those 23. So the incremental result was right and the fresh side's list was cut by the 50 ms query budget → (b) oracle artefact.
- fix bug 1: tests/highlight.rs `options()` = product defaults with query_timeout = PARSE_WAIT; used by highlighter(), no_idle(), incremental_equals_full and the two stale-span tests that compare spans for equality
- fix bug 2: recent_files.rs test close bound 500 ms → 30 s (close returns as soon as the writer answers)
- 5× loop under 16 `yes` (load 27–48): recent_files test 5/5 ok; fuzz oracle seed 12648430 5/5 ok (435, 441, 362, 272, 312 s)
- cargo test -p vr-syntax (run-heavy, ambient load 15–28): edit_during_the_initial_parse_never_returns_stale_spans failed twice — line 988 (no version-0 spans seen) and line 104 (fresh highlighter of the 2 MB text not Ready). Root cause: the 10 s parse_timeout; a 2 MB debug parse takes ~10 s alone, under load it passes 10 s, the root parse times out and the document is plain (D162) — so v0 is never shown / the oracle's fresh highlighter is disabled. Same class as bug 1 (a product limit leaking into the oracle). Fix: options() also sets parse_timeout = PARSE_WAIT (parse_timeout_stays_plain_and_reports keeps 1 µs for the limit itself; worker_never_blocks already did this with 3600 s).
- after the parse_timeout fix, under 16 `yes` (load 14–24): cargo test -p vr-syntax all green (highlight 26 passed, 5 ignored, 335 s); edit_during_the_initial_parse 3/3 ok, each 27–30 s — the 2 MB parse alone took > 10 s, confirming the old limit would have timed out

## Root cause
Bug 1 is (b), an oracle artefact, not a highlighter bug. `Highlighter::spans()` runs on the caller's thread with `query_timeout` = 50 ms (D162: returns what it has by then). The oracle compared `spans(0..len)` of two highlighters built with the product options, so on a loaded machine one query was cut short. In QC's failure the fresh side was cut: replaying the exact failing text, a fresh highlighter with a 60 s query budget has 45 spans and contains all 23 "only incremental" spans; with a 0 budget it has 11 spans ending at byte 70 and lacks exactly those 23. The incremental result shown to the user was right. Same class, found while verifying: the 10 s `parse_timeout` leaves a 2 MB debug parse plain under load, which broke `edit_during_the_initial_parse_never_returns_stale_spans` (line 988 / line 104).
Bug 2: `close(500 ms)` returned before the writer thread finished the locked, atomic write under load; the load then saw an empty list. `close` returns as soon as the writer answers, so the bound only matters for a stuck writer.

## Fix
- crates/vr-syntax/tests/highlight.rs: `options()` = product defaults with `query_timeout` and `parse_timeout` = `PARSE_WAIT` (120 s); used by `highlighter()` (every fresh-parse oracle), `no_idle()`, `incremental_equals_full`, `edit_during_the_initial_parse_never_returns_stale_spans`, `edits_after_a_parse_shift_old_spans_until_the_reparse_lands`. Not weaker: the limits keep their own tests (`spans_call_is_bounded_by_the_query_timeout` with 0, `parse_timeout_stays_plain_and_reports` / `edits_after_a_timeout_stay_plain_and_reset_tries_again` with 1 µs), perf tests keep the product defaults, the worker logic (incremental, verify, idle) is unchanged.
- crates/vr-ide/src/recent_files.rs (test only): `close(Duration::from_secs(30))`. Product code unchanged.
- No product code changed.

## Evidence
- 5× each under 16 `yes` (load 27–48): `close_writes_what_was_recorded_just_before` 5/5 ok (0.01–0.04 s); `VR_SYNTAX_FUZZ_SEED=12648430 every_sample_equals_a_fresh_parse_after_each_random_char_edit` 5/5 ok (435/441/362/272/312 s). (Run with the query fix; the later parse_timeout addition only relaxes another limit on the same options.)
- `cargo test -p vr-syntax` under 16 `yes`: unit 24 ok, grammars 25 ok, highlight 26 ok / 5 ignored, registry 6 ok.
- `cargo clippy --workspace --all-targets -- -D warnings`: Finished, no warnings. `cargo fmt -p vr-syntax -- --check` + rustfmt check on recent_files.rs: clean. (`cargo fmt --all --check` reports diffs only in T19's files: vr-ide/src/lib.rs, slash_tests.rs.)
- full suite once via run-heavy (`cargo test --workspace`, ambient load ~28): every crate ok except vr-ide lib 493 passed / 2 failed: `explorer_tests::drop_nested_keeps_order_and_scales` (< 1 s bound) and `syntax_tests::the_first_frame_does_not_wait_for_the_parse` (< 2 s bound) — wall-clock perf bounds, not in FR-13's files; both pass rerun (0.66 s, 0.61 s). Same run's vr-syntax had the pre-fix parse_timeout failure, since fixed.
- Load processes: all started `yes` killed (pgrep -x yes → 0) after every run.

## Out of scope (for the leader)
- vr-ide wall-clock tests `drop_nested_keeps_order_and_scales` and `the_first_frame_does_not_wait_for_the_parse` fail under load the same way (fixed time bounds on a debug build).
- Product question, not changed: when a viewport `spans()` call hits the 50 ms budget the frame shows partial colours (D162 by design); whether the editor redraws once more after a cut frame was not checked here.

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-10 related tests only + mapped guards | pass | vr-syntax tests, the two named tests, clippy, fmt; full suite once via run-heavy |
| BASE-14 no sleep/poll waits | pass | long runs as background tasks, resumed on completion |
| BASE-15 stop only what I started | pass | `yes` pids recorded in scratchpad load.pids, killed by pid |
| BASE-16 reuse | pass | reused PARSE_WAIT, the worker_never_blocks pattern (large parse_timeout) |
| BASE-17 no tool installs | pass | none |
| BASE-18 no own agents | pass | none |
| BASE-19 notes from start | pass | this file |
| COST-8 / COST-9 | pass | edits via Edit/perl one-line; nothing left running |
| scope (only vr-syntax + recent_files test module) | pass | git diff: crates/vr-syntax/tests/highlight.rs, crates/vr-ide/src/recent_files.rs (test module) |
| no product behaviour change | pass | no src change |
| no commit/stash/reset | pass | none |
