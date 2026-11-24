## Review T12 — APPROVE T12

Tools available: Read, Bash, Write, Skill, SendMessage, SubagentHandback.

Run in the worktree:
- `cargo test -p vr-core -p vr-fs -p vr-search`: 84, 43+2 and 34+2 passed, 0 failed, 3 ignored.
- Racing loop 10x (vr-fs, vr-search): clean.
- `cargo tree -e features -p vr-ide`: no vr-core `test-util` feature. The feature is enabled only through the dev-dependencies of vr-fs and vr-search.

### Findings
[minor] crates/vr-core/src/testutil.rs:117 — `visible` is computed after the write finishes (false negative)
Scenario: the in-place writer writes into the file at the path. The op then renames over the path before the writer's `fs::metadata(path)` runs. The inode differs, so the version gets `visible = false`. The op's replace has really destroyed a visible write, but `assert_no_lost_write` skips it. The window is small (between `write_all` and `metadata`), so the miss is rare. The helper can under-report but never fails a correct implementation.
Fix: stat the path before `write_all` (right after `open`) and compare it with `f.metadata()`. If the inode is the same at open time, the write was visible when it started.

[minor] crates/vr-core/src/testutil.rs:166-177 — the third rule is skipped when a later writer version ends up as the final bytes
Scenario: version v1 lands, `ours` replaces it, the op reports success, then v2 overwrites `ours`. `is_version` is true, so the check is skipped. v1 was lost to the op, but the test passes. This is not a false positive. It reduces strictness when 4 versions are written at a 3 ms interval.
Fix (optional): check every version that is not the last one. A visible version `begun < finished` with a later version also visible is still only lost if `ours` overlapped it, which cannot be told apart, so an acceptable alternative is to document the limit in the doc comment.

[minor] crates/vr-fs/tests/racing_writer.rs — the rename and move tests cannot fail against the current `ops` implementation
Scenario: `rename(2)` within one filesystem keeps the inode, so these tests are guards against a future copy+delete implementation. They are not evidence of AC-8 today. The notes make no such claim, so this is informational.

### Checked and found fine
- Determinism: no timing asserts. The writer is bounded (at most 12 versions). Start offsets are swept in 20 steps. The `not found` error ends the writer cleanly.
- No false positive for in-place writes to an old inode: `visible = false` is handled correctly.
- `#[ignore]` on the strict save test has a stated reason (D25 window, T4 round 3). The non-strict CI test still asserts whole content and no ours-after-error.
- Deviation from the Contracts (`Vec<WriterVersion>`, `OpRun`) is additive, test-only and listed in the notes.

## Files read
- Cargo.lock — read (1-line diff)
- crates/vr-core/Cargo.toml — read
- crates/vr-core/src/lib.rs — read
- crates/vr-core/src/testutil.rs — read
- crates/vr-fs/Cargo.toml — read
- crates/vr-fs/tests/racing_writer.rs — read
- crates/vr-search/Cargo.toml — read
- crates/vr-search/tests/racing_writer.rs — read

## Rule check T12
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | only the test infrastructure and manifests changed |
| BASE-2 | pass | module under `cfg(any(test, feature = "test-util"))`; feature only in dev-dependencies; `cargo tree` shows it is off for vr-ide; deviations are additive and listed |
| BASE-3 | pass | the notes record a red line (stub assertion failed, `save_overwrite` mutation went red) |
| BASE-4 | pass | results in the notes, confirmed by my run |
| BASE-8 | pass | Cargo.lock gains only the vr-core edge under vr-fs |
| BASE-32 | pass | D25 and D49 windows are documented in comments and the ignore reason |

## Quick pass 1 re-check
Minor 2 (earlier version replaced while a later one wins): fixed. `saw_before` is read just before each write, and `overtaken` fails when a visible version is followed by a version that saw `ours`. The self-test `flags_replace_of_an_earlier_version_when_a_later_one_wins` covers it. `cargo test -p vr-core`: 86 passed, 2 ignored.

Minor 1 (`visible` taken after the write): the false negative is gone, but the fix moves the error to the other side.
[minor] crates/vr-core/src/testutil.rs:130-134 — `visible` is now taken at open time and is not re-checked after `write_all`
Scenario: the in-place writer opens the file, so the inode matches and `visible = true`. The op then renames over the path, and `write_all` lands in the now-unlinked old inode. This is the D49 writer-held-old-file window, which no implementation can detect. The final file is `ours`, a visible version has `begun < finished`, and `assert_no_lost_write` panics "lost write". The same case triggers `overtaken` when the next version sees `ours`. A correct implementation can fail the strict `apply_replace` test, which runs in CI. The window is about the open-to-write time of 2 KB, a few microseconds, so it will be rare (the developer's 50x loop was clean).
Fix: take both stats and set `visible = same inode at open && same inode after write_all`. The old rare false negative only returns when the rename lands between the write and the second stat, and there are no false positives. Alternatively, accept and document it. The brief allows either, so this is not blocking.

Verdict: APPROVE T12 QP1 (one minor note, not blocking).

## Quick pass 2 re-check
`write_once` (testutil.rs:126-137) now sets `visible` only when the inode at the path matches the open handle both at open and after `write_all`. A write into an already-replaced file is invisible, so a correct implementation no longer fails on the D49 window. The remaining miss (a rename between the write and the final stat) is under-report only and is documented. The new self-test and the adjusted QP1 self-test are present. `cargo test -p vr-core`: 87 passed, 2 ignored. No new findings.

Verdict: APPROVE T12 QP2.
