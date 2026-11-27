# Review FR-2: CHANGES FR-2 (2 major, 2 minor)

Run (worktree .worktrees/FR2): `run-heavy.sh cargo test -p vr-core -p vr-search -p vr-ide` passed (vr-core 128, vr-ide 54, vr-search 35+2, 0 failed). Strict loop 20x (`cargo test -p vr-core --lib -- never_loses` + `cargo test -p vr-search -p vr-fs --test racing_writer`): fails=0. `--nocapture`: Other ok=20 refused=20 / fast ok=1 refused=79; Absent ok=11 refused=9 / fast ok=10 refused=30; kept=0.

What holds (focus a, b, d, e):
- (a) `save_as_seen` commits only through `write_to(p, Some(seen))`, which uses `replace_if_unchanged` for an existing target and `create_if_absent` for an absent one. It never calls a plain rename and never deletes. A directory, FIFO or device at commit time is refused by `write_atomic` (`!is_file`). A symlink put at the path after the version was taken changes the followed stamp, so it is refused. A dangling link resolves to its target and is created there with NOREPLACE. On the own path the precheck uses `seen`, and the commit then uses `save()`'s snapshot check. Both versions must match, so this is stricter than either check alone.
- (b) `is_own_path` compares `resolve_links` + canonical directory. A false "own" result needs the same final directory entry, and `save()` writes that same entry. A false "other" result (hard link, bind mount, a link retargeted mid-call) still commits against `seen`. Neither direction can lose data.
- (d) `of_path` is the D78 loop moved verbatim, and `disk_version` delegates to it.
- (e) The `visible` filter is strictly stronger. The NotFound arm's `Some(empty)` proves nothing, as `None` did before, because no writer version is empty. `is_none_or` loosens the rule only when an exchange fails with an error other than NotFound. tmpfs and ext4 (the CI and here) never produce that, so on them the oracle equals FR-1's. The pinned regressions all carry `Some` on later versions and still panic: round-3 loss (`swapping_rule_flags_version_written_into_our_file_and_deleted`, `..._never_saw_ours`), the reviewer's sequence (`..._did_not_overwrite`) and vr-search's racing test (T12 class, real writers, `Some` on ext4). vr-fs R2-1 does not use this oracle.

## Findings

[major] crates/vr-core/src/document.rs:768 (`of_path` → `hash_file`), also :806 (`still_fresh`) — the version of a non-regular target hangs the UI
Scenario: in Save As the user types `/dev/zero`, `/dev/urandom` or the name of a FIFO. It exists, so T6 opens the Replace prompt and calls `DiskVersion::of_path` as this API's doc tells it to. `DiskStamp::of` succeeds, then `hash_file` reads `/dev/zero` forever (`read` never returns 0), or `open` blocks on the FIFO until a writer appears. The app freezes on the UI thread with no way out. `write_atomic` would refuse these targets, but it is never reached. Before FR-2 the loop only ran on the document's own, already loaded path. Now it runs on any path the user names.
Fix: in `of_path`, stat first and return `DocError::Io(InvalidInput, "not a regular file")` when `!is_file()`. Open with `O_NONBLOCK` and `fstat` the fd before reading, so a swap to a FIFO between the stat and the open is also caught. Apply the same guard in `still_fresh` before `hash_file`. Test: `of_path("/dev/zero")` and `of_path(fifo)` return an error within the test timeout.

[major] crates/vr-core/src/document.rs:379-389 — `save_as` to another path still commits with no expectation, and an unprompted caller exists
Scenario: answer to (c). The prompt is not the only caller. The main tree's T6 `file.save_as` with `CmdArgs::Path` (commands/file.rs:179 → `save_as_doc` → `doc.save_as`) writes with no dialog and no version. An agent writes `other.txt` between `write_atomic`'s stat and its `fs::rename`, and the agent's write is replaced with no conflict copy. Under the severity anchor that window is a blocker however small. I file it as major only because D87 accepted it explicitly, so the leader has to decide.
Fix (no signature change, BASE-2 safe): in `save_as`, for another path, take `DiskVersion::of_path(p)` on entry and call `save_as_seen(p, &v)`. The call keeps its "replace what is there now" meaning and loses the commit window. Mark it `#[doc]` "prompts use `save_as_seen`". Deprecating it is not needed once it is self-checking. Update D87.

[minor] crates/vr-core/src/document.rs:393-397 — the `save_as_seen` doc tells the UI to "ask again with a fresh version", which cannot succeed on the own path
Scenario: the document is `[changed on disk]`. The user runs Save As, keeps the current name, and T6 takes `of_path` at submit. `save()` refuses against the old snapshot. A caller that follows the doc re-asks with a fresh version and gets the same refusal every time. The right flow for this case is the D81 Keep mine / Reload prompt → `save_overwrite_seen`. No caller exists yet.
Fix: document that `ChangedOnDisk` with `disk_state == ChangedOnDisk` (own path) means the D81 external-change prompt, not a new Replace prompt. Carry this into the T6 brief.

[minor] crates/vr-core/src/testutil.rs:331 — `is_none_or` lets the strict oracle weaken silently
Scenario: CI's TMPDIR moves to a filesystem without RENAME_EXCHANGE (some overlay, FUSE or NFS setups). Every later `replaced: None` then excuses all earlier versions, so a round-3-class deletion passes without a word. The run still reports "strict".
Fix: in `race_with` and vr-search's racing test, count versions with `replaced == None` and fail (or `eprintln!` plus skip the strict test explicitly) when exchange is supported, so a weakened run is never green unnoticed.

## Files read
- crates/vr-core/src/document.rs: read (whole diff; `save_to_own_path`, `write_to`, `write_atomic`, `still_fresh`, `resolve_links` as callees)
- crates/vr-core/src/testutil.rs: read (whole diff, `write_once`, `no_lost_write`, pinned regression tests)
- crates/vr-core/src/safe_replace.rs: callee only (`replace_if_unchanged`, `create_if_absent`)
- crates/vr-search/tests/racing_writer.rs: oracle use only (grep)
- crates/vr-ide/src/commands/file.rs (main tree, T6): `save_as` caller only

## Rule check FR-2
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | `git diff master --stat`: only vr-core document.rs, testutil.rs |
| BASE-2 | pass | additive `DiskVersion::of_path`, `Document::save_as_seen`; no signature changed; testutil `Target`/`race_with` private |
| BASE-3 | pass | red lines per test in notes; (b), own path and racing shown red by mutation to the pre-fix code (deviation stated) |
| BASE-4 | pass | notes carry test, loop, clippy and fmt output; reproduced here (tests ok, loop 20x fails=0) |
| BASE-6 | pass | English why-comments; no bug history |
| BASE-32 | fail (major) | D63/D73/D74/D78 honoured; D87's unchecked `save_as` to another path conflicts with the severity anchor (finding 2); D88 honoured but silently weakens on non-EXCHANGE filesystems (minor 4); D75–D77 strict `_swapping` oracle kept |

## Round 1 re-check

Verdict: APPROVE FR-2 R1. All 4 findings are fixed. Two new minors, neither blocking.

Run (worktree): `run-heavy.sh cargo test -p vr-core -p vr-search -p vr-ide` passed (vr-core 134, vr-ide 54, vr-search 35+2, 0 failed). Strict loop 20x (vr-core `never_loses --nocapture` plus vr-search/vr-fs `racing_writer`): fails=0. No `WARNING` line in any run, so no version had an unknown `replaced`. `ok=` per variant over 20 runs:
- Own: 12–18, fast 0–2
- Other: 10–18, fast 0–1
- Absent: 5–11, fast 0–1
- Unprompted: 8–18 (both speeds)

Earlier findings:
- Major 1, fixed. `hash_file` (document.rs:670) stats, opens with O_NONBLOCK, fstats the fd and refuses anything that is not a regular file with InvalidInput. That covers `of_path`, `still_fresh`, `holds_expected`/`holds_ours` and OwnWrites. The 2 s-bounded tests cover `/dev/zero`, a FIFO, and a FIFO swapped in after the version was taken. O_NONBLOCK has no effect on regular files. A swap after the stat is caught by the fstat. `rustix` is an existing vr-core dependency (BASE-8 ok). `proven` in safe_replace still checks `regular` via lstat before it calls the content check, so the swapped-out symlink case from D73 is unchanged.
- Major 2, fixed (D89). For another path, `save_as` does `of_path(p)?` → hook → `save_as_seen(p, &now)`, so it has no unchecked commit left. The own path is still `save()`. Production code has no hook overhead: in `cfg(not(test))`, `fire` is an empty `#[inline(always)]`.
- Minor 3, fixed. The `save_as_seen` doc separates the two cases: another path re-prompts Replace, the own path goes to the D81 prompt.
- Minor 4, fixed (D90). The writer returns `None` only on Unsupported/InvalidInput. Other errors, or an unreadable swapped-out file, are `Err` and panic the writer. `assert_exchange_proofs_known` runs in `race_with` after every `stop()` and fails where exchange works. Both are pinned by red-then-green tests that use the exchange seam. The skip also warns on stderr. vr-search's racing test does not call the assert yet; D90 records it as outside FR-2's files, to be routed.

D77 statement: correct for the oracles that existed before. The rules for Own, vr-search and success runs are unchanged or stricter (the `visible` filter). The round-3 should_panic tests carry `Some` on every later version, and the zero warnings above show the skip never fired.

New findings:

[minor] crates/vr-core/src/testutil.rs:871 — the Unprompted variant excuses the version in flight when `save_as` takes its version, and the D77 statement does not say so
Scenario: the writer records `begun` before its read, temp write and exchange. A version that begins before `taken` and lands while `of_path` runs or just after it gets partitioned out. A regression that replaced it would not be reported. This is the one version a broken D78 re-read loop would lose (stamp of v_k, hash of v_k+1, same length → `still_fresh` passes). It is not an earlier finding escaping: the D78 loop stays pinned by `disk_version_of_a_file_that_keeps_changing_is_an_error`, and Own/Other/Absent take their version before the writer starts. But this is a new oracle with an excused window, and D77 requires the window to be stated.
Fix: add one sentence to the notes and D90 stating the excused window and naming the test that pins D78. Optionally narrow it: record `landed` after the write and partition on `landed < taken`. That excuses only versions that were on disk by the time the version was taken, with no false positive for a correct implementation.

[minor] crates/vr-core/src/testutil.rs:919 (`race_with`) — no assert that the success path was exercised
Scenario: the fast Other and fast Absent variants had 0–1 successful runs in each of 20 loops here. Most loops never reach the success path, so on those variants the strict success rules (overtaken / replaced-by-ours, the T12 class) are tested only now and then. That is expected with a writer at `Duration::ZERO`. The normal variants reliably cover success (minimum ok=5 for Absent and 10 for Other over 20 runs), and so do the hook unit tests. If a later change broke `save_as_seen` so that it always refused, the racing tests would stay green.
Fix: in `race_with`, return `(ok, refused)` and assert `ok > 0 && refused > 0` for the normal (3 ms) variants of each target. Do not assert it for the fast variants, where `ok == 0` is legitimate and the assert would be flaky. Optionally add a `fast` sweep with later offsets if success under a fast writer should be guaranteed.

## Files read (round 1)
- crates/vr-core/src/document.rs: read (round diff: hook stage, `save_as`, `save_as_seen` doc, `hash_file`, tests)
- crates/vr-core/src/testutil.rs: read (round diff: `write_once_with`, accounted-for skip, `exchange_supported`, `assert_exchange_proofs_known`, Unprompted target, tests)
- crates/vr-core/src/safe_replace.rs: `proven` (callee check only)

## Rule check FR-2 R1
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | only vr-core document.rs, testutil.rs |
| BASE-2 | pass | no signature changed; `exchange_supported`/`assert_exchange_proofs_known` additive in test-util |
| BASE-3 | pass | red line per item in Fix round 1; item 3 is doc only |
| BASE-4 | pass | notes carry tests, loop, clippy, fmt; reproduced here |
| BASE-6 | pass | English, why-comments |
| BASE-32 | pass | D89, D90 implemented; D77 statement given (minor 1 asks for one more sentence) |

## Quick pass 1 re-check

Verdict: CHANGES FR-2 QP1. Two flaky asserts were added to the tests. Product code is unchanged except for the test-only record call, which is an empty inline fn outside cfg(test).

Run (worktree): `cargo test -p vr-core -p vr-search -p vr-ide` passed (135 / 54 / 35+2, 0 failed). Strict loop 20x (vr-core `never_loses race_leaves` + vr-search/vr-fs `racing_writer`): fails=0, WARNING=0, partial_takes=0 in 180 race lines.

To check the flake risk of the new asserts, I also ran the vr-core test binary 200x with the filter `fast_writer unprompted`. Load average was 16–20 on 16 cores. Result: 9 failures, none a "lost write":
- 7x at testutil.rs:826: `Unprompted: the race did not exercise both paths (ok=0 refused=40)`.
- 2x at testutil.rs:915: `save_as took no version`.
- Unprompted alone, 15 runs under the same load: 3 ms ok ranged 3–22; fast ok was 0 in 4 of 15 runs.

### Agreement on the loosened oracle (D77): AGREED
The rule is: a later write `w` counts as proof only when it is visible, or when its file came back, meaning `w.bytes` is the final content, is in the conflict copy, or is what a later write read at the path.

Why this is sound:
- Version bytes are unique, and the op only renames files, never copies them.
- So `w.bytes` can be read at the end or in a copy, or by a later writer that opened or exchanged the path, only if `w`'s inode was at the path again, or kept, after `w` wrote.
- The writer is sequential, so a later reader cannot have opened the inode before `w` wrote.
- `w.replaced == v` means that inode held `v` when `w` read it. So the writer itself overwrote `v`, and the op did not remove `v` from the path.
- An unlinked inode never comes back, so a write into a deleted file still proves nothing.

The earlier findings are all still caught:
- **Round-3 loss**: the pinned should_panic tests use visible later versions, which were counted before this change too. They still panic.
- **FR-1 dead-inode case**: `swapping_rule_ignores_proof_from_a_write_into_a_dead_file` still panics. Nothing makes a dead inode readable.
- **T12 13% loss**: it is judged by the success-run rules, which this change does not touch.
- **R2-1 source unlink**: it is judged by vr-fs's own test, which does not use this function.

The change is also still stricter than the FR-1 oracle, which counted every later write.

### (A) Unprompted excuse by hash: correct
- `save_as` records exactly what it took.
- Matching `holds` excuses only `..=i`: everything before `v_i` was already overwritten by the writer, and `v_i` is what `save_as` was asked to replace.
- It trusts `of_path`'s own record, so a broken D78 loop would be excused by construction. The developer states this, and D78 is pinned by `disk_version_of_a_file_that_keeps_changing_is_an_error` and `disk_version_is_of_one_version_of_the_file`.
- The fallback was never taken in my runs either (partial_takes=0).

### (C) vr-search: correct
It is a one-line call after `stop()`.

### The one-off "writer version 11 of 11 is in no file"
It is not a mutation artefact. Mutation A only changes which version is taken; a refused run still goes through the same safe_replace refusal. So the same report can come up on correct code, rarely.

It is very likely the accepted D75(2)/D76(3) case:
- A rename writer cannot end in no file. If it exchanges ours out, the swap-back takes its file out instead, and `proven(…, ours)` fails, so the file is kept as a conflict copy.
- Only an in-place write can end in no file. It saw ours, opened our temp inode before the swap-back, and its bytes landed after `proven`'s last Mark check, into the inode the op then deleted.
- It is also the last version: the one in flight when the op returns.

The oracle must keep flagging it. Excusing `!visible && replaced == ours` would also excuse a round-3-class write that landed at the path, because `visible` is sampled only after the write.

Recommendation (tests only, not blocking): print mode, `visible`, `saw_before == ours` and `replaced == ours` in the "is in no file" panic. A recurrence can then be classified: in-place, not visible, `replaced == ours` is D76(3); anything else is a real loss. The leader should record that the strict rule has this known rare false positive.

### Findings

[major] crates/vr-core/src/testutil.rs:826 (`assert_both_outcomes`) — the both-outcomes assert depends on timing and fails under load
Scenario: on a loaded machine (a CI runner, or parallel test binaries) the 3 ms Unprompted sweep gave `ok=0 refused=40` and the test failed. That happened 7 times in 200 runs here, with no loss involved. The Own, Other and Absent 3 ms sweeps can do the same, since the same timing drives them. A data-safety suite that flakes red teaches people to rerun until green.
Fix: get coverage without depending on timing.
- Option 1: add one deterministic success run per target (writer started after the op returns) and one deterministic refusal per target (hook write at `BeforeSwap`). Both already exist as unit tests for Other and Absent, so `assert_both_outcomes` can be dropped or kept as an `eprintln!`.
- Option 2: repeat the sweep, up to N times, until both outcomes are seen. The strict rule still runs on every run.
Red: force ok=0 by mutation and show that only the deterministic check fails.

[major] crates/vr-core/src/testutil.rs:914-915 — Unprompted panics with "save_as took no version" when `save_as` legitimately fails before taking a version
Scenario: under the fast writer, `DiskVersion::of_path` gives up after 5 re-reads ("the file keeps changing on disk; try again"). `save_as` then returns `Err(Io)` before `hook::version_taken`, and `take_version_taken().expect(…)` panics. That is a false failure, seen 2 times in 200 runs. The unfired `SaveAsVersionTaken` hook also stays queued in the thread-local list. In the next run of that thread it fires into the previous run's `Rc`, which would leave the new `taken` cell `None` if the fallback is ever needed.
Fix: when no version was taken, require the result to be that `Io` error. Excuse nothing; every version must survive and ours must not be on disk. Clear the queued hook after each run, for example by draining `hook` stages in the harness. Red: a stub `of_path` failure, or a hook writing at `ReadingDiskVersion` 5 times, as in `disk_version_of_a_file_that_keeps_changing_is_an_error`.

## Files read (QP1)
- crates/vr-core/src/document.rs: read (hook `version_taken`/`take_version_taken`, `save_as` record call, `DiskVersion::holds`)
- crates/vr-core/src/testutil.rs: read (`assert_versions_accounted_for` readable rule, new self-test, `assert_both_outcomes`, `race_with` Unprompted excuse)
- crates/vr-search/tests/racing_writer.rs: read (import plus `assert_exchange_proofs_known` call)

## Rule check FR-2 QP1
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | vr-core document.rs/testutil.rs; vr-search racing_writer.rs as routed by D90/D96 |
| BASE-2 | pass | no public signature changed; test-only items |
| BASE-3 | pass | red by product mutations A/B and red self-tests, in notes |
| BASE-4 | pass | notes carry outputs; loop reproduced; the flakes above were not visible in a 20x loop |
| BASE-6 | pass | English why-comments |
| BASE-32 | pass | D96 implemented; D77 agreement given above |

## FR-3 review

Verdict: APPROVE FR-3. Both QP1 majors are fixed. One leader decision is open; it does not come from FR-3.

Run (worktree .worktrees/FR3): `run-heavy.sh cargo test -p vr-core -p vr-search -p vr-ide` passed (vr-core 135, vr-ide 79, vr-search 35+2, 0 failed). Diff scope: only `crates/vr-core/src/testutil.rs`.

QP1 findings:
- Major 1 (timing-dependent both-outcomes), fixed. `assert_both_outcomes(target)` is now deterministic. With no writer the save must succeed and leave ours on disk. With a hook write at `BeforeSwap` (after the last freshness check) it must refuse and leave theirs on disk. The check runs for Own, Other, Absent and Unprompted. For Absent, `create_if_absent` fires `BeforeSwap` before NOREPLACE, so the refusal is real. Leftover hooks are drained. The developer proved it red by moving the hook to `BeforeSwapBack`. The sweeps no longer assert counts, and the strict rule still judges every racing run.
- Major 2 (Unprompted without a version), fixed. If `take_version_taken()` returns `None`, the result must be the "keeps changing" Io error and ours must not be on disk. Nothing is excused, so every version is checked strictly. Draining `SaveAsVersionTaken` stops the hook from leaking into the thread's next run.
- Diagnostic: every unaccounted version is reported with mode, visible, saw_ours and replaced_is_ours. The rule is unchanged, so the oracle is not loosened (D77).

### Classification of `version 31 of 60 [mode=InPlace visible=false saw_ours=true replaced_is_ours=true]`: the accepted D75(2)/D76(3) case, not a safe_replace loss

Walk-through:
- The in-place writer read the path while ours was there (saw_ours). It opened our temp inode and read `Xinitial\n` through the handle (replaced_is_ours).
- In `write_once`, the order is: read → `here` → `between` → `set_len(0)` → write → `after`. The writer was preempted before `set_len`, which is likely under the 14 busy loops of the load run.
- Meanwhile the refused save swapped back and took our inode off the path. It then ran `proven(tmp, Some(ours), holds_ours)` and deleted the inode.
- `proven` would have caught any write before its last `Mark::of`. A version is 2073 bytes and ours is 9, so `set_len` or the write changes `len`. `unwritten_since` compares `len` and `mtime`, and the content hash is checked in between. Either check failing means `keep` → conflict copy → accounted.
- So the only way to lose v31 is a `set_len`/write after `proven`'s final Mark, into a file already off the path and then unlinked. That is exactly "a writer holding an fd and writing after the file left the path" (D75(2), accepted in D76(3)).
- It does not have to be the last version. After its orphaned write the writer continues with v32…v60 at the path until `stop()`, and those land normally. The "last version" in my QP1 note was a heuristic, not a requirement.

What in safe_replace could allow a real loss instead: nothing in this trace. Every write that lands while our inode is at the path, or before the final Mark, is seen and kept.

### Can the oracle tell the case apart without loosening it (D77)?
Not with the four recorded flags. A round-3-class loss (a write into our file while it was at the path, then deleted by a broken protocol) also ends `visible=false` when the swap-out happens before the writer's `after` sample. So excusing that flag combination would loosen the oracle. Keep it strict.

Narrowing that is possible, but it is still a loosening and needs a D77 statement:
- The writer samples the path again immediately before `set_len`, and records `detached_before_write = (ino at path != opened)`.
- Excuse only InPlace ∧ detached_before_write ∧ saw_ours ∧ replaced_is_ours.
- Such a write by definition began after the file left the path (D76(3)).
- Round-3 stays caught: its write landed at the path, so `detached_before_write = false`.
- Remaining gap: a regression that puts our inode back on the path after the write and then deletes it. No later writer version would read it there only if it is removed again before the writer's next version.

Leader decision needed (not an FR-3 change): either (a) keep the strict rule and accept a rare, classifiable false positive (about 1 in 200 runs under heavy synthetic load, 0 in normal loops), or (b) add the pre-write sample and the narrow excuse above with a D77 statement. Until then, a CI failure whose diagnostic shows exactly that flag combination is D76(3). Any other combination is a real loss.

## Files read (FR-3)
- crates/vr-core/src/testutil.rs: read (whole FR-3 diff; `write_once` InPlace order for the classification)
- crates/vr-core/src/safe_replace.rs: `restore`, `proven`, `Mark::unwritten_since`, `keep` (callee check only)

## Rule check FR-3
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | only testutil.rs |
| BASE-2 | pass | additive `WriterVersion.mode`; no signature changed |
| BASE-3 | pass | mutation red (hook moved to BeforeSwapBack) for item 1; items 2/3 are harness branches |
| BASE-4 | pass | outputs in notes; reproduced (tests pass) |
| BASE-6 | pass | English why-comments |
| BASE-32 | pass | D77 rule unchanged; D96 kept |
