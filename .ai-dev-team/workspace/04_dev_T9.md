# T9 - red search tests on master

Tools available: Bash, Read, Write, Edit, Agent, Skill, ToolSearch, Artifact, SubagentHandback.

## Cause (test-only, no product defect)
- The replace worker (`crates/vr-ide/src/commands/search.rs`, one plain thread per job, no shared pool) writes each file as temp + `sync_all` + exchange + dir fsync (`crates/vr-search/src/replace.rs`). Under load its wall time is bounded by ext4 journal commits, not by CPU or cores. Another developer saw the worker in `jbd2_log_wait_commit`, 13 of 80 files in 15 s, io pressure full avg60 45%.
- `search_tests::apply_reports_a_file_failing_halfway` (80 files): `wait_search` had a fixed 20 s wall deadline -> "search did not finish" at search_tests.rs:56.
- `project_replace_never_loses_a_concurrent_write`: sweeps a racing writer over offsets 0..12 ms (40 x 300 us). With a slow fsync the replace outlasts the whole sweep, so the writer always wins: `ok=0 refused=40` -> the "both outcomes must occur" assert (line 605) fails (seen for InPlace).
- Not a run-4 change: no new watcher or roots in the search path; the failure reproduces on current master code alone and depends only on disk/IO load.
- Evidence before fix (2 tests, load 20-30, `--test-threads` 1/2): 2 of 5 runs failed at threads=1 (one with both tests failing), 2 of 4 failed at threads=2; passes in 1.7 s on an idle disk.

## Fix (crates/vr-ide/src/search_tests.rs only)
- `wait_search`: the deadline is now a stall limit (20 s without progress), where progress = result groups/matches found + preview files + files with a replace result. A slow disk keeps advancing; a real hang still fails after 20 s. A raised fixed timeout would only move the failure point, so it was not used.
- Race sweep: one unraced replace is timed first, and the offset step is `max(300 us, unraced/20)`, so the sweep spans ~2x the replace duration on any disk and both outcomes (refused, ok) occur. On a fast disk the step stays 300 us (unchanged behaviour).
- `event_loop::tests::exit_restores_and_writes_recovery_before_waiting_for_a_replace` uses a hold-gated replace (no fsync-bound wait), so it was left unchanged.

## Runs after the fix (IO + CPU load from `qa-artifacts/T9/load.sh`: 4 dd conv=fsync writers + 16 busy loops, load 14-40)
6 of 6 passed (`qa-artifacts/T9/probe.sh 6`); run 1 took 115 s of wall time under heavy fsync stall and passed, the others 3-8 s.

## Guard
`cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`: clean (Finished, no warnings).

## Reuse
| Need | Found | Action |
| --- | --- | --- |
| wait / sweep helpers | `wait_search`, `sweep_offsets` | extended in place |

## Rule compliance
pass: BASE-10 (related tests only), BASE-11, BASE-14, BASE-15, BASE-16, BASE-19, BASE-24, BASE-25, COST-8, COST-9, COST-13. n-a: security/UI rules.
Nothing left running; load scripts finished; no temp worktree created (the run-3 base comparison was not needed: the cause is load-dependent fsync latency, visible on the current code).

## Fix round 1
- `wait_search` now uses `StallWatch` (search_tests.rs): progress counts only when it exceeds the previous high-water mark, so a clear-and-refill loop (0 -> N -> 0) stalls and fails after 20 s; a 10 min absolute ceiling covers unbounded growth.
- Self-tests: `stall_watch_fails_a_refill_loop` (should_panic "search did not finish", fake oscillating counter, 100 ms stall) and `stall_watch_accepts_steady_growth`.
- Race sweep: the step comes from the slowest of 3 unraced samples (`max`), not one sample.
- Runs: 5 runs of the two tests, the first one overlapping the heavy IO/CPU load (passed, 232 s of wall time because of fsync stalls), the other 4 in 2-4 s; `cargo test -p vr-ide -- search_tests`: 34 passed, 0 failed. Guard `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`: clean. Nothing left running.
