# FR-6 — disk-version change detection on a coarse timestamp tick (bug fix)

Worktree: .worktrees/FR6 (branch fr6-disk-version). Scope: crates/vr-core/src/document.rs.

## Progress
- step 0: read brief-common, board row, document.rs (DiskStamp, Snapshot racy, DiskVersion::of_path, still_fresh, save paths).
- step 1: reproduced in the worktree: `cargo test -p vr-core --lib disk_version_` → disk_version_of_a_file_that_keeps_changing_is_an_error FAILED, `Ok(DiskVersion(Expected { stamp: Some(DiskStamp { len: 9, mtime == ctime ..999007649 .. }) ...}))` at document.rs:1675. `stat` of 6 back-to-back shell writes: mtime/ctime advance in 1-3 ms jiffy steps (CONFIG_HZ=1000), size and inode unchanged.
- step 2: root cause traced to `still_fresh` (equal stamp trusted without regard to racy); test seams added (cfg(test) `hook::freeze_clock`, `hook::no_exchange`).
- step 3: red tests save_overwrite_seen/save_as_seen/save `_never_replaces_a_write_in_the_same_tick_*` (no-EXCHANGE case `false Ok(Saved { other_hard_links: 0 })`), fix in `Expected.racy` + `still_fresh`, green.
- step 4: disk_version tests: keeps_changing and is_of_one_version rewritten to change the length; new disk_version_in_one_tick_is_of_the_bytes_on_disk; mutations checked.
- step 5: full vr-core suite + racing 20x + guard (background).
- step 6: full suite 146 pass / 0 fail / 1 ignored, racing 20/20, fmt + clippy clean (qa-artifacts/FR-6/final.txt). Decisions appended (2 `D? (FR-6)` lines).

## Root cause
DiskStamp (len, mtime, ctime, dev, ino) cannot see a same-length in-place write made in the same kernel timestamp tick. On this machine ext4 timestamps come from the coarse clock (jiffies, CONFIG_HZ=1000): `stat` after back-to-back shell writes shows mtime == ctime moving in 1–3 ms steps with the same size and inode. The failing test writes "agent v0\n" and then, in each try's ReadingDiskVersion hook, "agent wN\n" — all 9 bytes, all inside one millisecond — so `of_path`'s stamp → hash → stamp sees no change on try 1 and returns `(S, hash of "agent w0\n")`. That version is truthful (it is the bytes on disk at stamp S), so the failing test by itself is not a loss — QC was right that `of_path` returns no mixed pair here.

The real gap is downstream: `still_fresh` returned `true` on `cur == expected.stamp` without regard to whether that stamp was racy when taken. `Snapshot` (check_disk, own-path save check) and `OwnWrites::is_own` already re-hash racy stamps; `Expected` (DiskVersion, and the own-path save's commit expectation) did not. Per path:

| Path | With RENAME_EXCHANGE (local Linux fs) | Without EXCHANGE (FUSE, NFS, non-Linux: D63 fallback) |
|---|---|---|
| save_overwrite_seen (D64/D74, Keep mine prompt) | step-1 check fooled, step-3 content check of the swapped-out file refuses: safe | **overwrote** a same-length write made in the version's tick, however long the user then took at the prompt |
| save_as_seen / save_as to another path (FR-2, D89) | safe (step 3) | **overwrote** the same way |
| save (own path, D25/D40) | check: racy snapshot hashed → refused; commit: step 3 | check safe; a same-tick write between the check and the commit passed the fallback re-check (which never looked at content) — wider than the documented D63 check→rename window |
| check_disk | racy snapshot hashed: safe | same |
| watcher own-write suppression (OwnWrites::is_own) | racy → hash: safe (identical bytes rewritten in the same tick count as ours: no data at stake) | same |
| DiskVersion::of_path | returns the bytes on disk at its stamp; cannot see a write between stamp and hash inside one tick, but the version is of the bytes it read, so it is a real version | same |

## Decision: (a) and (b)
- (a) Product defect, fixed: `Expected` gains `racy: bool` (same rule as Snapshot, extracted to `is_racy`, judged when the stamp is confirmed: after the hash in `of_path`, after the content check in `save_to_own_path`). `still_fresh` trusts an equal stamp only when it was settled; a racy one goes to the content comparison (hash + stamp held still). `Expected` equality ignores `racy` (when a version was seen does not make it another version; keeps `disk_version()` comparisons stable across the 2 s boundary).
- (b) for the two disk_version tests: their change (same-length in-place write) is invisible to any stamp inside one tick, so on a coarse clock they cannot pass (keeps_changing) or pass vacuously (is_of_one_version: the "no re-stamp" mutation did not fail it on this machine). Changed only how the change is produced — each write now has a new length — assertions untouched. Reviewer to confirm.

### Why the new test forms still catch their defect classes
| Test | Defect class (T4 review item 2, D78) | New form | Proof |
|---|---|---|---|
| disk_version_of_a_file_that_keeps_changing_is_an_error | of_path returns a version while the file keeps changing (no re-stamp after the read, or no bound on retries) | 5 in-place writes of lengths 10..14 (`"agent w" + "+"×(i+2) + "\n"`); still `Err(DocError::Io)` then Ok | mutation "no re-stamp" (`true \|\| DiskStamp::of(..) == Some(stamp)`) → FAILED; mutation "10 tries" (`0..DISK_VERSION_TRIES * 2`) → FAILED; restored → ok |
| disk_version_is_of_one_version_of_the_file | stamp of v1 paired with the hash of v2 | hook writes "agent v22\n" (10 bytes) between stamp and read; still `assert_eq!(seen, doc.disk_version())` | mutation "no re-stamp" → FAILED (before the change it passed vacuously here) |
| disk_version_in_one_tick_is_of_the_bytes_on_disk (new) | the same class where no stamp can help: same length, same tick | frozen clock + no EXCHANGE; hook writes v2 between stamp and read | version holds exactly v2 (`holds`); a save over v1 (same stamp) refuses, over v2 succeeds; red before the fix |

### Other tests sharing the "a rewrite moves the stamp" assumption
Searched same-length `fs::write` followed by a stamp-based check in document.rs, watch.rs, testutil.rs:
- disk_version_is_of_one_version_of_the_file — shared it, fixed above.
- watch.rs `revert_without_reload_is_reported` — rewrites identical bytes 600 ms after our save; assumes a tick < 600 ms (on FAT's 2 s tick the identical rewrite would count as our own write). No data at stake; outside scope, left as is.
- disk_version_of_any_path_counts_absence_as_a_version — same-length v1→v2 but asserts the versions differ, which the hash ensures: not affected.
- testutil racing writers (`version_bytes`, all the same length) — oracle and commit step 3 are content-based: not affected.
- equal_stamp_on_coarse_clock_still_compares_content, check_disk/save tests — go through Snapshot's racy re-hash: not affected.

## Files changed
- crates/vr-core/src/document.rs — `is_racy` (extracted from `Snapshot::new`), `Expected.racy` + manual `PartialEq`/`Eq`, `still_fresh` racy rule, the three `Expected` constructors; cfg(test) seams `hook::freeze_clock`/`frozen_clock` (applied in `DiskStamp::from_meta`) and `hook::no_exchange`/`exchange_unsupported` (exchange wrapped in `write_atomic`); release stubs are inline no-ops. Tests: 4 new, 2 changed (how the change is produced only), helper `rewrite_in_same_tick` (asserts the stamp did not move, so each same-tick test proves its own setup).
- Team files: decisions.md (2 lines), this file, qa-artifacts/FR-6/final.sh + final.txt.

## Tests added / changed, with red lines
- save_overwrite_seen_never_replaces_a_write_in_the_same_tick_as_the_version — red before the fix: `panicked at document.rs:1808: false Ok(Saved { other_hard_links: 0 })` (exchange=false) → green.
- save_as_seen_never_replaces_a_write_in_the_same_tick_as_the_version — red: `document.rs:1831: false Ok(Saved { other_hard_links: 0 })` → green.
- save_never_replaces_a_write_in_the_same_tick_as_its_check — written after the fix; red shown by temporarily restoring the old shortcut (`!(false && expected.racy ..)`): `document.rs:1869: false Ok(Saved { other_hard_links: 0 })` (BeforeRename write, no EXCHANGE) → restored → green.
- disk_version_in_one_tick_is_of_the_bytes_on_disk — red with the old shortcut: `document.rs:1776: Ok(Saved { other_hard_links: 0 })` → green.
- disk_version_of_a_file_that_keeps_changing_is_an_error, disk_version_is_of_one_version_of_the_file — changed (b), mutation proof above.

## Commands + output (trimmed; qa-artifacts/FR-6/final.txt)
```
$ cargo test -p vr-core --lib disk_version_          (before)
test document::tests::disk_version_of_a_file_that_keeps_changing_is_an_error ... FAILED
panicked at crates/vr-core/src/document.rs:1675:9: Ok(DiskVersion(Expected { stamp: Some(DiskStamp { len: 9, ..., ctime: (1795521294, 999007649), .. }), hash: Some(..) }))
test result: FAILED. 4 passed; 1 failed

$ cargo test -p vr-core --lib disk_version_   (after, 3 runs)
test result: ok. 6 passed; 0 failed   (x3)

$ cargo test -p vr-core
test result: ok. 146 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 4.02s
test result: ok. 0 passed; 0 failed (doc-tests)

$ 20x cargo test -p vr-core --lib -- testutil::tests::document_save testutil::tests::save_as_seen_never_loses testutil::tests::unprompted_save_as
test result: ok. 7 passed; 0 failed; ... 140 filtered out
racing: 20 pass / 0 fail

$ cargo fmt --all --check  → fmt OK
$ cargo clippy -p vr-core --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 6.17s
```

## New states (round 0)
- `Expected.racy` (a kept value inside DiskVersion and the save expectation). Quit / cancel / Esc: no effect (a value, no thread). Error halfway: the extra hash in still_fresh returns `Err` → the save fails at step 1 or, in the fallback, before the rename; nothing exposed. Input changing / second request: each prompt takes a fresh DiskVersion; a version kept for long stays racy → one hash per save attempt, never a trusted stale stamp. Concurrent writer: the three same-tick tests + racing 20/20. Call that never returns: hash_file already refuses FIFOs/devices without blocking (FR-2 tests `disk_version_of_a_fifo_is_an_error_without_blocking`, `save_as_seen_refuses_a_fifo_put_at_the_target_without_blocking` pass); a hung NFS read blocks this hash the same way the existing step-3 hash already does — no new kind of blocking point.
- Test seams (`freeze_clock`, `no_exchange`) are cfg(test) thread-locals; release paths are no-op stubs.

## Remaining window (unchanged, documented)
D63: without EXCHANGE a write landing between the last check and the plain rename is replaced. FR-6 removes the extra case where that check itself was fooled by an equal racy stamp.

## Out of scope noticed
- watch.rs `revert_without_reload_is_reported` assumes a tick < 600 ms (see above).

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| racy-stamp rule | document.rs `Snapshot::new` | extracted to `is_racy`, used by Snapshot and Expected |
| content check with stamp held still | `still_fresh` unequal-stamp branch | reused for the equal racy case |
| injecting writers | document.rs `hook` (Stage, set/fire) | reused; clock/exchange seams added beside it |
| racing oracle | vr_core::testutil (T12) | reused unchanged for the 20x run |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | only crates/vr-core/src/document.rs changed (diff --stat: 1 file) |
| BASE-2 | pass | no public signature changed; `Expected` is private; `DiskVersion` stays `PartialEq + Eq + Copy` |
| BASE-3 | pass | red line per test in "Tests added / changed" |
| BASE-4 | pass | "Commands + output", qa-artifacts/FR-6/final.txt |
| BASE-5 | pass | first build, suite, racing and guard through run-heavy.sh; nothing left running |
| BASE-6 | pass | English comments explaining why (`Expected.racy`, `is_racy`, test comments) |
| BASE-8 | pass | no dependency added |
| BASE-10 | pass | `cargo test -p vr-core` only |
| BASE-11 | pass | worktree only; mutations made and reverted with perl, `grep -c` = 0 after each |
| BASE-14 | pass | first build and final run as background tasks, turn ended |
| BASE-15 | pass | no process stopped |
| BASE-16 | pass | Reuse table |
| BASE-17 | pass | no installs |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | Progress lines |
| BASE-24 | pass | nothing staged or committed |
| BASE-25 | pass | fmt OK; clippy -p vr-core clean (workspace clippy not run: this worktree changes vr-core only; FR-5 edits vr-ide on the main tree) |
| BASE-32 | pass | D25/D63/D64/D74/D78/D89 honoured; D77/D99 strict oracle untouched; no data-safety assertion weakened |
| BASE-38 | pass | `rustfmt --edition 2024 crates/vr-core/src/document.rs` only |
| COST | pass | related tests during the loop, suite once at the end, no polling |
| AC-3 | pass | save tests in suite 146/0 |
| AC-7 | pass | the external-change prompt's save (save_overwrite_seen) refuses same-tick writes; check_disk tests pass |

## Round 1
Finding (reviewer, 05_review_FR-6.md, blocker): racy was decided after the content was read. A same-length write during a read that outlasts the racy window then hid behind an equal stamp that had been judged settled, and on the no-EXCHANGE fallback the agent's version was replaced. — **fixed**

### Progress
- r1 step 1: seams `hook::freeze_clock_at(t)` and stages `DiskVersionRead` (of_path: after the hash, before the second stamp), `SnapshotCompared` (changed_since_snapshot: after the content compare), `LoadRead` (load: after read_to_end).
  - A perl one-liner whose `|` delimiter clashed with `|e|` damaged the first line of the file (an `e` in the module doc). Repaired with Edit, and the hooks were inserted with Edit. `git diff` shows no change to the header.
- r1 step 2: 5 red tests (below), each run alone before the fix.
- r1 step 3: fix; document:: 58 pass; full suite, racing 20x and guard (qa-artifacts/FR-6/final-r1.txt).

### Fix
Racy is decided right after the stamp is taken, before anything is read, in every place a stamp is kept:
| Place | Before | After |
|---|---|---|
| `DiskVersion::of_path` | `is_racy(&stamp)` after hash + re-stamp | `let racy = is_racy(&stamp)` right after the first stamp, before the `ReadingDiskVersion` hook and `hash_file` |
| `save_to_own_path` (Expected) | after `changed_since_snapshot` (which may hash) | right after `DiskStamp::of(&path)` |
| `changed_since_snapshot` refresh (check_disk, save) | `Snapshot::new(cur, ..)` after `snapshot_matches` hashed | `is_racy(&cur)` before `snapshot_matches`; the snapshot is built with that value |
| `load` (pre-FR-6, now shares `is_racy`) | `Snapshot::new` after `read_to_end` | `is_racy(&stamp)` right after the stamp from the open handle |
`Snapshot::new` is removed, so no caller can judge racy after a read again. The `is_racy` doc says to ask right after taking the stamp. Judging earlier only ever marks more stamps racy, which costs at most one more hash per check.

### Tests (red before the fix, green after)
Common setup:
- The clock is frozen 500 ms before the end of the racy window (`freeze_clock_near_window_end`), and `no_exchange` is set.
- `assert_racy_now` proves the stamp starts racy.
- `rewrite_during_read` hooks the stage named for each test below. The hook does a same-length rewrite in the tick (it asserts the stamp did not move), then sleeps until the window has ended, as a long read on a large file or slow NFS would.

| Test | Stage | Red (before the fix) |
|---|---|---|
| disk_version_read_racing_the_end_of_the_window_stays_racy (Keep mine, save_overwrite_seen) | DiskVersionRead | `document.rs:1820: Ok(Saved { other_hard_links: 0 })` |
| save_as_seen_version_read_racing_the_end_of_the_window_stays_racy | DiskVersionRead | `document.rs:1835: Ok(Saved { other_hard_links: 0 })` |
| save_check_racing_the_end_of_the_window_stays_racy (save) | SnapshotCompared | `document.rs:1853: Ok(Saved { other_hard_links: 0 })` |
| check_disk_racing_the_end_of_the_window_stays_racy (snapshot refresh) | SnapshotCompared | `document.rs:1871: left: InSync right: ChangedOnDisk` |
| load_racing_the_end_of_the_window_stays_racy (load) | LoadRead | `document.rs:1888: left: InSync right: ChangedOnDisk` |
Each asserts `Err(ChangedOnDisk)` (or ChangedOnDisk on check) and that the file still holds the agent's v2. A slow runner that lets 500 ms pass before the first stamp fails `assert_racy_now`; the test cannot pass by accident. Round-0 tests and assertions are unchanged.

### Output (qa-artifacts/FR-6/final-r1.txt)
```
$ cargo test -p vr-core --lib document::
test result: ok. 58 passed; 0 failed; 1 ignored; ... 93 filtered out; finished in 0.59s
$ cargo test -p vr-core
test result: ok. 151 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 4.09s
$ 20x strict racing (testutil document_save, save_as_seen_never_loses, unprompted_save_as)
test result: ok. 7 passed; 0 failed; ... 145 filtered out
racing: 20 pass / 0 fail
$ cargo fmt --all --check → fmt OK
$ cargo clippy -p vr-core --all-targets -- -D warnings → Finished (no warnings)
```

### New states (round 1)
None added to product code. The racy value is the same `racy` bool as in round 0, now taken earlier. Test-only: the `freeze_clock_at` thread-local and three hook stages, which are no-op stubs in release.
- Quit, cancel/Esc, a second request and an input change behave as in round 0 (a value, no thread).
- An error halfway: the same `?` paths as before.
- A concurrent writer: the 5 tests above plus racing 20/20.
- A call that never returns: no new read is added. Earlier racy only makes `still_fresh` hash through `hash_file` (non-blocking for FIFOs and devices), as round 0 already did.

### Rule compliance (round 1 changes)
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | still only crates/vr-core/src/document.rs |
| BASE-3 | pass | 5 red lines above, each before the fix |
| BASE-4 | pass | final-r1.txt |
| BASE-25 | pass | fmt OK, clippy -p vr-core clean |
| BASE-32 | pass | D64/D25 hold in the reviewer's scenario; round-0 oracle and assertions unchanged |
| BASE-38 | pass | rustfmt on document.rs only |
| AC-7 | pass | Keep mine refuses a write made during the version's read (disk_version_read_racing_…) |
Other rows unchanged from round 0.

## Round 2
Finding (QC r1): strict racing 20x under 4 busy-loop hogs → 2 fail, both `document_save_never_loses_a_write_from_a_fast_writer` at testutil.rs:402 (assert_versions_accounted_for), signature not captured.

### Progress
- r2 step 1: round-0 reference built from a copy (qa-artifacts/FR-6/r0src, own target/): the worktree sources with the four racy decisions moved back after the read (document.rs lines 603/624/705/897; `diff` shows only those 4 lines). No stash/checkout in the worktree.
- r2 step 2: loop script qa-artifacts/FR-6/loop.sh (test binary direct, QC's 3 filters = 7 tests, every failing run's full output kept in qa-artifacts/FR-6/r2/fail-<label>-<n>.txt, fixed run budget).
