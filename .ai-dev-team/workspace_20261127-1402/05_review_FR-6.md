# Review FR-6 — disk-version change detection on a coarse timestamp tick

Reviewer (opus), round 0. Worktree /home/rainy/DEV/vr-ide/.worktrees/FR6 (branch fr6-disk-version, uncommitted), diff = `git diff` (1 file).

Verdict: **CHANGES_REQUESTED** (1 blocker)

The direction is right. (a) is a real product defect, and `Expected.racy` plus the `still_fresh` rule fix it for any same-tick write that lands **after** the version was hashed. One gap is left in the same defect class: the version is marked racy or settled at the wrong moment.

## Findings

[blocker] crates/vr-core/src/document.rs:876 (also :592) — "racy" is judged after the content was read, so a write that happens during a long read can still be trusted by its stamp
Scenario: no RENAME_EXCHANGE (D63 fallback: FUSE, NFS, exFAT, non-Linux). File `f` was last written at mtime `m`. `of_path` takes stamp S at t0 (t0 − m < tick), then `hash_file` reads a large file, or a slow NFS file, until t1 ≥ m + 2 s. While it reads, an agent rewrites `f` in place with the same length inside the tick of `m`. The reader sees the old bytes (or a truncated prefix), and the re-stamp still equals S. `is_racy(&stamp)` then runs at t1: age ≥ 2 s, so `racy = false`. Result: `DiskVersion(S, hash of old)` marked settled. When the user clicks Keep mine, `still_fresh` finds `cur == S` and `!racy`, returns `true`, and the fallback `rename` replaces the agent's version, which the user was never shown. On a 2 s tick (FAT-style) this needs no large file: stamp at m + 1.9 s, read finishes at m + 2.1 s. `save_to_own_path` has the same problem: `is_racy` at :592 runs after `changed_since_snapshot` (:583), which may hash the file. With EXCHANGE, `holds_expected` catches it (the swapped-out bytes ≠ the expected hash), so only the fallback is open. That is the path FR-6 set out to close. The severity anchor applies: a concurrent write is overwritten.
Why: racy has to describe the moment the read started. Only a write during the read can hide behind an equal stamp. If the check runs after the read, a long read moves the age past `RACY_WINDOW`. Evaluating earlier is always the safer direction: more stamps count as racy, which costs one more hash.
Fix: in `of_path`, compute `let racy = is_racy(&stamp);` right after the first `DiskStamp::of` and before `hook::fire`/`hash_file`, and store that value. In `save_to_own_path`, compute it from `cur` before `changed_since_snapshot`. The same late check exists in `load` (:674–686: stamp → `read_to_end` → `Snapshot::new` judges racy after the read). That predates FR-6, but `Snapshot` now shares `is_racy`; give `Snapshot::new` a racy value computed before the read, or document that load is out of scope. Test: add a seam that freezes the clock at a given time (e.g. `freeze_clock_at(now − RACY_WINDOW + 50 ms)`) and a stage after the hash and before the re-stamp that rewrites the file with the same length and then sleeps past the window. With `no_exchange`, `save_overwrite_seen(&seen)` must refuse and the agent's bytes must stay. This test should be red on the current code.

No other findings. Checked and found clean:
- `still_fresh`: an equal racy stamp goes to hash + held-still re-stamp; the absent↔absent case is still trusted.
- `Expected` with `stamp: Some` always has `hash: Some`: `save_to_own_path` returns ChangedOnDisk first when `disk` is None.
- `Expected` equality ignores `racy`; no product code in crates/vr-ide compares `DiskVersion` values (file.rs only passes them to `save_as_seen` / `save_overwrite_seen`).
- Watcher own-write suppression (`OwnWrites::is_own`, `Snapshot`) did not change.
- `no_exchange` returns `ErrorKind::Unsupported`, which `safe_replace::replace_if_unchanged` (safe_replace.rs:114–120) routes to the real fallback.
- Hang check: the new hash goes through `hash_file` (regular files only, O_NONBLOCK, rechecked after open), so a FIFO or device fails at once. A huge file is hashed by the same calls as before (`of_path` and step 3 already read it), so this adds no new kind of blocking point. It only adds one or two more full reads per save of a racy version.

## Test oracle agreement (BASE-32 / D77 / D99)

I agree with (b).
- `disk_version_of_a_file_that_keeps_changing_is_an_error` still targets the T4 review item 2 / D78 classes: (1) the re-stamp after the read is missing, (2) retries have no bound or no error. Each hook write now changes the length (9 → 10..14 bytes), so on any tick the stamp moves on every try. Removing the re-stamp returns Ok, and doubling the tries makes the 6th call succeed. The developer's mutation runs show both failing, and the assertions (`Err(Io)`, then Ok) are byte-identical.
- `disk_version_is_of_one_version_of_the_file` is stronger than before. On a 1 ms tick the old form passed even without the re-stamp, so it was vacuous. The new length makes "stamp of v1 + hash of v2" differ from the second version on every machine.
- One thing is no longer exercised: neither test now shows that a **same-length** change is seen through mtime/ctime alone. I accept that. This is not the class these tests were added for. `DiskStamp` equality is derived over all fields, and the same-length case is now carried by the content check, which the new test pins.
- `disk_version_in_one_tick_is_of_the_bytes_on_disk` and the three `*_in_the_same_tick_*` tests cover the same-length same-tick case honestly.
  - `freeze_clock` only replaces mtime/ctime in `DiskStamp::from_meta`. That simulates a tick longer than the test; len, ino and dev stay real.
  - `rewrite_in_same_tick` asserts that the stamp really did not move, so each test proves its own setup.
  - The refusals can only come from the content comparison, so a product that trusts the equal stamp fails them. The developer's red lines show this with the old shortcut restored.
  - If the frozen time ages past 2 s on a slow runner, `racy` becomes false and the tests fail. They cannot pass by accident.
  - In release both seams are `#[inline(always)]` stubs (`None` / `false`) under `cfg(not(test))`; vr-ide builds vr-core without `cfg(test)`.
  - Limit: these tests cover a write before or after the hash, not during it. That is the blocker above, and it needs its own test.

## Files read
- crates/vr-core/src/document.rs — read (the whole diff, plus DiskStamp, Snapshot, OwnWrites::is_own, snapshot_matches, save_overwrite_seen, save_as, save_as_seen, check_disk, save_to_own_path, changed_since_snapshot, load ordering, hash_file, of_path, still_fresh, holds_expected, write_atomic)
- crates/vr-core/src/safe_replace.rs:99–135 — read (fallback trigger and where `target_unchanged` is called)
- crates/vr-ide/src/commands/file.rs — grep of DiskVersion uses only (no equality comparisons)
- Team files: 04_dev_FR-6.md, 05_qc_report.md sign-off, brief-common.md severity anchor, decisions D25/D46/D63/D64/D73–D78/D99/D130/D131, 04_dev_T4.md / 05_review_T4.md / 04_dev_FR-2.md on the purpose of the two tests

## Rule check FR-6
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | `git diff --stat`: only crates/vr-core/src/document.rs changed |
| BASE-2 | pass | public API unchanged; `Expected` is private; `DiskVersion` still derives `PartialEq, Eq, Copy` (now through a manual `Expected` eq); vr-ide callers unaffected |
| BASE-3 | pass | the notes give a red line for each new test (document.rs:1808/1831/1869/1776) and the mutation runs for the two changed tests |
| BASE-6 | pass | English comments say why (`is_racy`, `Expected.racy`, `still_fresh`, the test comments on length and tick) |
| BASE-25 | pass | from the notes: fmt OK, `clippy -p vr-core --all-targets -D warnings` clean (not rerun; vr-core is the only changed crate) |
| BASE-32 | fail | D64/D25 (never replace a version the user was not shown) still fails on the D63 fallback in the blocker scenario; D77/D99 oracle untouched (pass) |
| AC-3 | pass | the notes give the suite at 146/0; the save path logic is correct apart from the blocker |
| AC-7 | fail | the Keep mine save (`save_overwrite_seen`) can still replace a same-tick write made during the version's read when there is no EXCHANGE (blocker) |
| Test oracle (b) | pass | agreement above |

## Round 1 re-check

Verdict: **APPROVED**. The round-0 blocker is fixed, and the fix opened nothing new.

- **Blocker closed on every path.** Racy is now decided right after the stamp is taken and before any read, in all four places a stamp is kept:
  - `of_path`: `let racy = is_racy(&stamp)` comes before the `ReadingDiskVersion` hook and `hash_file`.
  - `save_to_own_path`: racy is decided right after `DiskStamp::of`, before `changed_since_snapshot`.
  - `changed_since_snapshot` refresh: `is_racy(&cur)` comes before `snapshot_matches`. This also closes the check_disk/save snapshot-refresh case, which round 0 did not list.
  - `load`: racy is decided before `read_to_end`.
  - `Snapshot::new` is removed. The remaining `Snapshot {..}` constructions are the four above, plus the post-save one in `write_atomic` (`racy: true`) and two test fixtures, so no caller can judge racy after a read.
  - `still_fresh` and `OwnWrites::is_own` are unchanged.
- **Seams are honest.**
  - `freeze_clock_at(now − 2 s + 500 ms)` only sets the stamp's mtime and ctime. `is_racy` still reads the real clock, so the hook's sleep really moves the age past `RACY_WINDOW`, as a long read would.
  - `assert_racy_now` proves the stamp starts racy, and `rewrite_in_same_tick` proves the write was invisible to the stamp. A slow runner therefore fails the test rather than passing it vacuously.
  - The new stages `DiskVersionRead`, `SnapshotCompared` and `LoadRead` sit exactly at the read/re-stamp boundary they name.
  - The `cfg(not(test))` stubs are `fire` (a no-op), `frozen_clock` (`None`) and `exchange_unsupported` (`false`); the new stages exist only as enum variants. `freeze_clock_at` is test-only.
  - I traced each of the 5 new tests against the round-0 code. Each takes the "settled → trust stamp → fallback rename" or "refresh as settled → InSync" path, which matches the reported red lines.
- **No regression.**
  - Round-0 tests and their assertions are untouched.
  - Judging racy earlier only adds hashes, never trusts more.
  - No new blocking read: the extra hashing goes through `hash_file`.
  - The cost is about 0.55 s of sleep per new test (5 tests).
- **Module doc header:** `git diff -U0` shows no `//!` line added or removed; lines 1–6 are the original header. `git status` lists only document.rs as changed in the worktree, and there are no stray lines in the hunks.

Remaining limitation, unchanged: the D63 check→rename window when EXCHANGE is not available. It is accepted.

## Rule check FR-6 r1
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | `git status`: only crates/vr-core/src/document.rs |
| BASE-2 | pass | no public signature changed; `Snapshot::new` was private |
| BASE-3 | pass | 5 red lines in `## Round 1` (document.rs:1820/1835/1853/1871/1888), consistent with my trace of the round-0 code |
| BASE-6 | pass | `is_racy` doc says to ask right after the stamp, before the read; each new stage is documented |
| BASE-25 | pass | from the notes: fmt OK, clippy -p vr-core clean (not rerun) |
| BASE-32 | pass | D25/D64 hold in the round-0 scenario on all four paths; D77/D99 oracle untouched; D138 recorded |
| AC-3 | pass | from the notes: vr-core 151/0, strict racing 20/20 |
| AC-7 | pass | Keep mine refuses a write made during the version's read (`disk_version_read_racing_the_end_of_the_window_stays_racy`) |
| Test oracle (b) | pass | unchanged from round 0 |
