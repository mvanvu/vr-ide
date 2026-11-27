# FR-2 vr-core: version of any path + save_as_seen + FR-1 oracle minors
Worktree: .worktrees/FR2 (branch fr2-save-as-seen). Side: be. Files: crates/vr-core/src/{document,testutil}.rs.

## Progress
- Read brief-common, 05_review_T6.md (blocker 1, 2), 05_review_FR-1.md (2 minors), 04_dev_FR-1.md, decisions D63/D64/D73-D78, safe_replace.rs (module doc state machine), document.rs, testutil.rs.
- Red of_path: disk_version_of_any_path_counts_absence_as_a_version panicked at todo!() (stub); green after moving the disk_version loop into DiskVersion::of_path.
- Red (a): save_as_seen_never_replaces_a_version_written_after_the_replace_prompt panicked at document.rs:1672 (assert matches ChangedOnDisk; stub save_as_seen = current save_as, replaced v2).
- Code: DiskVersion::of_path, Document::save_as_seen, save_as(own path) = save(), is_own_path/location helpers, write_to sets disk_state only for the own path. (a) green.
- Red (b) + own path: written after the code, so shown red by mutation back to the pre-fix behaviour (save_as_seen = write_to(p, None); is_own_path = false): save_as_seen_never_replaces_a_file_created_after_it_was_seen_absent panicked at :1761 (ChangedOnDisk expected), save_as_to_the_own_path_checks_freshness_like_save at :1791, save_as_to_the_own_path_saves_an_unchanged_file at :1819, (a) at :1729 again. Restored from backup: 5/5 green.
- testutil: race harness generalised (`race_with(Target::{Own,Other,Absent}, ...)`; `race_document_save_with` delegates). New strict tests `save_as_seen_never_loses_a_concurrent_write` and `save_as_seen_never_loses_a_write_from_a_fast_writer` (Other: InPlace + TempRename; Absent: TempRename, the only writer that creates). Red by mutation (save_as_seen = write_to(p, None)): both panicked at testutil.rs:282 "lost write: 1 writer version(s) were replaced by our bytes and a later writer version overwrote ours". Green after restore: Other ok=19 refused=21, fast ok=3 refused=77; Absent ok=10 refused=10, fast ok=11 refused=29.
- Red minor (2): swapping_rule_ignores_proof_from_a_write_into_a_dead_file "test did not panic as expected" (testutil.rs:594). Fix: only `visible` later writes count as proof in assert_versions_accounted_for. Green (21 passed).
- Red minor (1): rename_writer_onto_a_missing_path_knows_it_replaced_nothing (left: None, right: Some([])) and swapping_rule_does_not_count_unknown_replaced_as_a_loss (panicked testutil.rs:324 "lost write: writer version 1 of 2 is in no file..."). Fix: write_once TempRename records Some(empty) when the path is gone (NotFound) and None only when exchange is unsupported/refused; assert_versions_accounted_for treats a later visible write with unknown `replaced` as possibly having replaced the version (not checked); filesystem requirement documented on `WriterVersion::replaced` and the assert. Green (23 passed).
- Related tests, strict loop 30x, clippy, fmt: all green (below). capabilities.md and decisions.md updated.

## Summary
- Files changed: crates/vr-core/src/document.rs, crates/vr-core/src/testutil.rs (worktree .worktrees/FR2).
- New API (additive, BASE-2; no existing signature changed):
  - `impl DiskVersion { pub fn of_path(path: &Path) -> Result<DiskVersion, DocError> }` — links followed, absent file is a version, re-reads until stamp and content agree (D78 loop moved here; `Document::disk_version` now delegates).
  - `pub fn Document::save_as_seen(&mut self, p: &Path, seen: &DiskVersion) -> Result<Saved, DocError>` — pre-check `still_fresh(p, seen)` then `write_to(p, Some(seen))`: existing target → `safe_replace::replace_if_unchanged` (EXCHANGE protocol), absent → `create_if_absent` (NOREPLACE). `ChangedOnDisk` (or a "kept at" `Io` error) leaves path, disk_state and modified flag of the document as they were. Own path → `still_fresh(seen)` and then `save()`.
  - `save_as(p)`: own path (also `dir/./name` or via symlink: `is_own_path` compares the canonical location) is now `save()` with the snapshot check; another path unchanged (unchecked, documented: prompts use `save_as_seen`).
  - `write_to` updates `disk_state` on a refusal only when writing the document's own path.
- testutil: `WriterVersion::replaced` = `Some(empty)` when a rename write found the path gone; `None` only without EXCHANGE. `assert_versions_accounted_for`: only `visible` later writes count as proof; a later visible write with `replaced: None` makes the version untraceable (skipped, documented). Race harness `race_with(Target::{Own, Other, Absent}, ...)`.
- Tests added: document.rs `disk_version_of_any_path_counts_absence_as_a_version`, `save_as_seen_never_replaces_a_version_written_after_the_replace_prompt` (a), `save_as_seen_never_replaces_a_file_created_after_it_was_seen_absent` (b), `save_as_to_the_own_path_checks_freshness_like_save`, `save_as_to_the_own_path_saves_an_unchanged_file`; testutil.rs `save_as_seen_never_loses_a_concurrent_write`, `save_as_seen_never_loses_a_write_from_a_fast_writer` (strict `_swapping` rule), `rename_writer_onto_a_missing_path_knows_it_replaced_nothing`, `swapping_rule_does_not_count_unknown_replaced_as_a_loss`, `swapping_rule_ignores_proof_from_a_write_into_a_dead_file`.
- Deviations: the red runs for (b), own path and the racing variant were shown by mutation back to the pre-fix code, because the implementation for (a) already covered them (recorded above with lines).
- Out of scope noticed: vr-ide (this worktree is before T6) still calls `save_as`; T6 must take `DiskVersion::of_path` when the Replace prompt opens (or at submit for a missing name), call `save_as_seen`, and re-prompt on ChangedOnDisk. A `git diff --stat` (read-only) was run once in the worktree despite "no git commands"; nothing changed by it.

## Commands (worktree, trimmed)
```
$ run-heavy.sh cargo test -p vr-core -p vr-fs -p vr-search -p vr-ide   (exit=0)
test result: ok. 128 passed; 0 failed; 1 ignored   (vr-core lib)
test result: ok. 43 passed; 0 failed; 1 ignored
test result: ok. 2 passed; 0 failed
test result: ok. 54 passed; 0 failed; 2 ignored
test result: ok. 35 passed; 0 failed; 1 ignored
test result: ok. 2 passed; 0 failed
$ for i in 1..30: cargo test -p vr-core --lib -- never_loses; cargo test -p vr-search -p vr-fs --test racing_writer
strict loop 30x fails=0
test testutil::tests::document_save_never_loses_a_concurrent_write ... ok
test testutil::tests::document_save_never_loses_a_write_from_a_fast_writer ... ok
test testutil::tests::save_as_seen_never_loses_a_concurrent_write ... ok
test testutil::tests::save_as_seen_never_loses_a_write_from_a_fast_writer ... ok
(--nocapture: Other ok=19 refused=21; fast Other ok=3 refused=77; Absent ok=10 refused=10; fast Absent ok=11 refused=29)
$ cargo clippy -p vr-core --all-targets --features vr-core/test-util -- -D warnings
Finished `dev` profile
$ rustfmt --edition 2024 --check crates/vr-core/src/{document,testutil}.rs   -> clean
$ cargo fmt --all --check -> clean
$ cargo clippy --workspace --all-targets -- -D warnings
warning: `vr-ide` (manifest) generated 2 warnings  (unused vr-search dependency note, manifest only)
Finished `dev` profile
```

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| version of a path | document.rs `disk_version` | moved its D78 loop into `DiskVersion::of_path`, `disk_version` delegates |
| fresh commit for Save As | document.rs `write_to`/`write_atomic`, safe_replace | reused unchanged (`Some(expected)`: EXCHANGE or NOREPLACE) |
| pre-check | `still_fresh` | reused |
| racing harness | testutil `race_document_save_with` | generalised into `race_with(Target, ...)` |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | only crates/vr-core/src/{document,testutil}.rs changed (board row) |
| BASE-2 | pass | additive: `DiskVersion::of_path`, `Document::save_as_seen`; no signature changed; `save_as` own-path behaviour per brief |
| BASE-3 | pass | red lines per test in Progress (of_path todo!, (a) :1672, (b)/own/racing by mutation, minors (1)/(2) red) |
| BASE-4 | pass | Commands section |
| BASE-5 | pass | test run via run-heavy.sh; nothing left running |
| BASE-6 | pass | English, why-comments only |
| BASE-8 | pass | no manifest change |
| BASE-10 | pass | related crates only per brief command |
| BASE-11 | pass | no stash/checkout/restore/reset |
| BASE-14 | pass | all commands finished in foreground |
| BASE-15 | pass | no background tasks started |
| BASE-16 | pass | Reuse table; capabilities.md line 13 extended |
| BASE-17 | pass | no installs |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | this file, progress per step |
| BASE-24 | pass | no git add/commit |
| BASE-25 | pass | fmt --all --check clean; clippy --workspace -D warnings finished |
| BASE-32 | pass | D63/D73 same safe_replace commit; D64/D74 absent is a version, only seen version replaced; D75-D77 strict `_swapping` oracle on save_as_seen; D78 re-read loop kept in of_path |
| BASE-38 | pass | rustfmt only on the two files |
| COST | pass | related tests only, one heavy run |
n-a: none further (no UI, no web, no deps)

## Fix round 1
- Read 05_review_FR-2.md (2 major, 2 minor) and D89. Plan: (1) hash_file refuses non-regular files (stat, O_NONBLOCK open, fstat); (2) save_as other path = of_path + save_as_seen; (3) doc; (4) writer fails on an unexpected exchange error + public assert that no version has unknown `replaced` where exchange works.
- Red item 1: disk_version_of_a_device_is_an_error_without_reading_it, disk_version_of_a_fifo_is_an_error_without_blocking, save_as_seen_refuses_a_fifo_put_at_the_target_without_blocking: all 'blocked for more than 2 s: Timeout' (document.rs:1832). Fix: hash_file stats first, opens O_NONBLOCK, fstats the fd, refuses non-regular with InvalidInput 'not a regular file' (covers of_path, still_fresh, holds_expected, OwnWrites). Green 3/3.
- Red item 2: new testutil `unprompted_save_as_never_loses_a_concurrent_write` (Target::Unprompted: plain `save_as` to another existing file with the racing writer; normal and fast writer; strict `_swapping` rule). Test-only hook `Stage::SaveAsVersionTaken` marks when save_as took the target's version. Only versions begun before that instant are excused, so the oracle never reports a false loss. Red on the unchecked code (hook fired at entry): panicked testutil.rs:289 "lost write: 1 writer version(s) were replaced by our bytes and a later writer version overwrote ours". Fix: save_as(other) = `DiskVersion::of_path(p)?` then `save_as_seen(p, &now)` (D89). Green: Unprompted ok=13 refused=27; fast ok=11 refused=69; existing save_as tests green.
- Item 3: `save_as_seen` doc rewritten. For another path, ChangedOnDisk → Replace prompt with a fresh version. For the own path, ChangedOnDisk sets disk_state and leads to the external-change prompt (Keep mine → save_overwrite_seen / Reload, D81), never a retry. Doc only, so no test.
- Red item 4: `rename_writer_fails_on_an_unexpected_exchange_error` panicked testutil.rs:643 "unknown `replaced` accepted silently"; `unknown_replaced_where_exchange_works_fails_the_run` "test did not panic as expected" (testutil.rs:659), both against stubs. Fix: `write_once_with` exchange seam. `replaced = None` only for Unsupported/InvalidInput; other errors return Err (the writer panics "racing writer failed"); a failed read of the swapped-out file is an Err too. New public `exchange_supported(dir)` and `assert_exchange_proofs_known(dir, &versions)`, called in `race_with` after `stop()`. The skip in `assert_versions_accounted_for` now prints a WARNING. Green: 26 testutil tests.
- D77 statement: the oracle still catches both losses.
  - The round-3 loss is pinned by `swapping_rule_flags_version_written_into_our_file_and_deleted` and `..._deleted_version_that_never_saw_ours` (plus the reviewer's `..._did_not_overwrite`). Every later version there carries `Some` replaced, so the new skip never applies. All still panic (in the 26 green, should_panic).
  - The T12 ~13% loss (a write between the last stat and the rename on a successful run) is judged by the success rules ("replaced by our bytes and the operation reported success" / "overtaken"). Those rules are unchanged and do not use `replaced`. The item-2 red run reproduced exactly that class on the unchecked `save_as` ("lost write: 1 writer version(s) were replaced by our bytes and a later writer version overwrote ours").
  - On ext4/tmpfs `replaced` is never None (assert_exchange_proofs_known passed in all 30 loop runs), so the strict rule here equals FR-1's plus the `visible` filter.

### Fix round 1 commands (worktree, trimmed)
```
$ rustfmt --edition 2024 --check crates/vr-core/src/{document,testutil}.rs -> FMT-OK
$ run-heavy.sh cargo test -p vr-core -p vr-search -p vr-ide   exit=0
test result: ok. 134 passed; 0 failed; 1 ignored   (vr-core lib)
test result: ok. 54 passed; 0 failed; 2 ignored    (vr-ide)
test result: ok. 35 passed; 0 failed; 1 ignored    (vr-search)
test result: ok. 2 passed; 0 failed                (racing_writer)
$ strict loop 30x (vr-core `never_loses` + vr-search/vr-fs racing_writer): fails=0
--nocapture: Own ok=11 refused=29 | fast Own ok=1 refused=79 | Other ok=16 refused=24 | fast Other ok=0 refused=80
             Absent ok=8 refused=12 | fast Absent ok=0 refused=40 | Unprompted ok=12 refused=28 | fast Unprompted ok=10 refused=70
$ cargo clippy -p vr-core --all-targets --features vr-core/test-util -- -D warnings -> Finished, no warnings
$ cargo fmt --all --check -> clean; cargo clippy --workspace --all-targets -- -D warnings -> Finished (vr-ide manifest note only)
```
- Per finding: 1 fixed, 2 fixed, 3 fixed (doc), 4 fixed (fails where exchange works, warns where it does not).
- Rule compliance changes: BASE-3 pass (red line per item above; item 3 is doc only). BASE-32 pass (D89 implemented; D88 amended in the ledger; D77 statement above). Every other row unchanged. Nothing left running.

## Quick pass 1
- Read Round 1 re-check (2 minors). Plan: (A) narrow the Unprompted excuse: match the version save_as took against writer versions by content (exact), and fall back to the begun split only when it took a partial in-place state; state the remaining window. (B) assert ok>0 and refused>0 for the 3 ms variants. (C) vr-search racing test calls assert_exchange_proofs_known.
- (C) crates/vr-search/tests/racing_writer.rs calls `assert_exchange_proofs_known(root, &versions)` right after `writer.stop()` (import added). That is the only change to the file.
- (A) Unprompted excuse narrowed. `save_as` records the `DiskVersion` it took (`hook::version_taken`, test-only; the non-test build has an empty inline fn). `DiskVersion::holds(bytes)` is cfg(test). `race_with` excuses versions `..=i`, where `i` is the version whose content the taken version holds. It excuses none when the taken version is the initial file. Only when the taken version is a state no complete version explains (an in-place write's truncated file) does it fall back to excusing versions begun before the take, counted as `partial_takes`. I did not use the reviewer's `landed < taken` split: `landed` is recorded after the write returns, so a preempted writer would report as protected a version the save legitimately replaced (a false loss), and it would still excuse versions landing between the read and the record.
  - Window stated (D77): only in the `partial_takes` fallback, a version in flight when the version was taken (begun before it) that a regression replaced is not reported. `partial_takes` was 0 in every one of the 20 loop runs. D78's re-read is pinned by `disk_version_of_a_file_that_keeps_changing_is_an_error` and `disk_version_is_of_one_version_of_the_file`.
  - Red for (A) by product mutation A: `save_as` takes a version, records it, sleeps 2 ms, takes a new one and commits against that. Over 10 runs `unprompted_save_as_never_loses_a_concurrent_write` failed 4 times. Three were "lost write: 1 writer version(s) were replaced by our bytes and a later writer version overwrote ours" (the intended catch). One is described below. Restored: green.
- Oracle false positive found while checking mutation A (testutil, tests only). The round-0 `visible` filter (FR-1 minor 2) could report a lost write on a refused run when a correct save swapped its file in and back:
  - An in-place write opened the version's file. Its bytes landed while that file was off the path, so the writer marked it not visible. The swap-back then returned that file to the path, so the writer itself had overwritten the earlier version.
  - Seen in a debug trace: v57 vis=false repl=v56, v58 repl=v57, final=v58 → "writer version 56 is in no file".
  - Fix: a later write counts as proof when it is visible, or when its file came back (its bytes are the final content, in a conflict copy, or what a later write read at the path).
  - Self-test `swapping_rule_accepts_proof_from_a_write_whose_file_came_back`, red twice. The final-content and kept-copy case failed at testutil.rs:373, and the "read by a later write" case at :378, both with "lost write: writer version 1 of N is in no file...". Green after the fix. `swapping_rule_ignores_proof_from_a_write_into_a_dead_file` still panics, so a write into a file the run deleted still proves nothing.
- Unexplained once: in 1 of 10 mutation-A runs there was a refused-run report, "writer version 11 of 11 is in no file". 25 more mutated runs did not reproduce it, and it was never seen on correct code (0 failures in the 20× loop and in all earlier loops). Hypothesis: the D75(2)/D76(3) accepted case. A writer that saw our bytes opens our file at the path and writes after the swap-back proved and deleted it. The FR-1 oracle checks invisible versions that saw ours, so it reports this case by design. Not changed in a tests-only pass; flagged for the leader.
- (B) `race_with` returns `(ok, refused)`. `assert_both_outcomes` is asserted for the 3 ms variants only: Own (`document_save_race_leaves_whole_content`, `document_save_never_loses_a_concurrent_write`), Other and Absent (`save_as_seen_never_loses_a_concurrent_write`) and Unprompted. The fast variants are not asserted. Red by product mutation B (save_as_seen always returns ChangedOnDisk): "Unprompted: the race did not exercise both paths (ok=0 refused=40)" and "Other: ... (ok=0 refused=40)". Restored: green.

### Quick pass 1 commands (worktree, trimmed)
```
$ rustfmt --edition 2024 --check crates/vr-core/src/{document,testutil}.rs crates/vr-search/tests/racing_writer.rs -> FMT-OK
$ run-heavy.sh cargo test -p vr-core -p vr-search -p vr-ide   exit=0
test result: ok. 135 passed; 0 failed; 1 ignored   (vr-core lib)
test result: ok. 54 passed; 0 failed; 2 ignored    (vr-ide)
test result: ok. 35 passed; 0 failed; 1 ignored    (vr-search)
test result: ok. 2 passed; 0 failed                (racing_writer)
$ strict loop 20x (vr-core never_loses + race_leaves, vr-search/vr-fs racing_writer): fails=0, WARNING lines=0, partial_takes=0
  3 ms ok/refused seen: Own 11-17 / 23-29, Other 14-16, Absent 7-8, Unprompted 8-17 (fast variants 0-1 ok)
$ cargo clippy -p vr-core --all-targets --features vr-core/test-util -- -D warnings -> Finished
$ cargo clippy -p vr-search --all-targets -- -D warnings -> Finished
$ cargo fmt --all --check -> clean
```
- Product code: `save_as` behaviour is unchanged. Only a test-only record call was added, an empty inline fn outside tests. Nothing left running.
