# Review T4: vr-core documents + file watching (+ D31)

Verdict: **CHANGES T4** (CHANGES_REQUESTED), 3 major, 3 minor, 1 uncertain.

Tools available: Read, Bash, Write, Skill, SendMessage, SubagentHandback.
Commands run: `cargo test -p vr-core`: `61 passed; 0 failed; 1 ignored`. `cargo clippy -p vr-core --all-targets -- -D warnings`: Finished, clean. `rustfmt --check` on the 4 files: exit 0.
Probe: `qa-artifacts/T4-review/probe` (target dir removed).

## Findings

[major] crates/vr-core/src/document.rs:482-488: save replaces a file the user cannot write (permission bypass)
Scenario: `readonly()` is true only when all of 0o222 are clear. A file the user cannot write but that has a write bit set somewhere is not refused, and `rename` replaces it because the directory is writable. Examples: a 0o644 file owned by root in a user-owned project (docker or `sudo` output), or 0o464. Probe with 0o464: a direct `open(write)` gives `PermissionDenied`, but `doc.save()` returns `Ok` and the content becomes "keep!". With the root-owned file, `fchown` fails, so the file silently changes owner to the user. Mode bits are copied even after `fchown` failed, so setuid/setgid survive on a file that now has a different owner. This breaks D41 ("refused with PermissionDenied") and Review Focus 2.
Fix: replace the mode-bit test with a real writability probe of an existing target: `OpenOptions::new().write(true).open(target)`, with no truncate and no data written, mapped to `PermissionDenied`. This covers owner, ACLs and immutable files. When `fchown` fails, also clear 0o6000 before `set_permissions`. Test: a 0o464 file → `PermissionDenied`, bytes unchanged.

[major] crates/vr-core/src/document.rs:265-276, 519-527: check-then-rename window spans the whole write+fsync; stamp taken after the dir fsync (data loss)
Window measured from the code path: the check is at document.rs:266-269. Everything between it and the rename at :520 is inside the window: `resolve_links` (lstat per link), `fs::metadata`, `create_temp`, the full buffer write (`BufWriter`, 64 KiB chunks), `fchown`, `set_permissions` and `sync_all`. So yes, it spans the temp write and the fsync, which is O(file size) plus device flush latency: ms for small files on SSD, up to seconds for large files or slow or network disks. The platform needs only a stat right before `rename(2)` (µs). Linux has no atomic "replace only if unchanged" call, so a µs residual is the floor. Same defect class as T7 and T9: a concurrent save is lost silently, with no error and no prompt.
Scenario A: `save()` checks the stamp, then writes the whole buffer to the temp file and runs `sync_all`. On a 100 MB file or a slow disk, that takes from tens of ms to seconds. Only then does it rename. An AI agent that writes the file during that time is overwritten without `ChangedOnDisk`. A file deleted at check time and recreated in that window is also clobbered.
Scenario B: the stamp is read from our handle after `rename` and after the directory fsync (ms). An in-place write by an agent to our freshly renamed inode in that gap ends up in the snapshot stamp, while `hash` is ours. After that, `check_disk()` says `InSync` because the stamp is equal and nothing is hashed, and `ignore_own_write` suppresses the event. The next `save()` silently overwrites the agent's content.
Fix: (A) when `check` is set, re-stat the target right before `fs::rename` and compare it with the stamp seen at the check, or `None` → still `None`. On a mismatch, drop the temp file and return `ChangedOnDisk`. The leftover window is microseconds; document it in code as D25 does. (B) fstat our handle before the rename, then again immediately after it, before the dir fsync. If `len`/`mtime` differ (rename changes only ctime), store a snapshot that cannot match (for example, force a re-hash) instead of our hash. Tests: a hook or `cfg(test)` seam that writes the target between write and rename → `ChangedOnDisk`.

[major] crates/vr-core/src/watch.rs:127-131, 250-252: watcher overflow/rescan and later errors are dropped silently (AC-7 stops working)
Scenario: on an inotify queue overflow (for example, a `git checkout` touching thousands of files), notify-debouncer-full 0.8 emits a rescan event with `EventKind::Other` and the `need_rescan` flag (lib.rs:703). `translate` drops all `Other` events, so the open, unmodified document never reloads and the buffer stays stale. Watch failures after the initial `watch()` arrive only through `Err(errors)` and are only `tracing::warn`ed. Examples: a new subdirectory under a recursive root hitting `max_user_watches`, or a watched directory removed. `Watcher::watch -> Result` (D39) surfaces the limit only at the first call. Later, the UI is never told.
Fix: handle `ev.event.need_rescan()` by emitting `Modified` for every watched file and a rescan signal for dir roots. Forward errors to the caller, for example an additive `FsEvent::Error(String)` or `FsEvent::Rescan(PathBuf)`. Log the contract addition in decisions.md. Test: feed `translate` a synthetic rescan event → `Modified` for watched files.

[minor] crates/vr-core/src/watch.rs:257, 267-270: a file moved into a watched dir is reported as `Modified`, not `Created`
Scenario: `mv /elsewhere/x.php project/src/` makes the debouncer emit `Modify(Name(To))` ("move in", debouncer lib.rs:856). That falls into the `_` arm with `is_create = false` → `FsEvent::Modified(new path)`. T8's explorer gets a Modified event for a path it does not know. Likewise, a temp+rename by another vr-ide instance to a new path gives `touch(to, false)`, so it reads as Modified.
Fix: treat `RenameMode::To` (and `Both` from a save temp when `to` did not exist before) as create. Then `created && !files.contains_key` gives `Created`. Test: rename from outside the root → `Created`.

[minor] crates/vr-core/src/document.rs:520: hard-linked files are silently split by save
Scenario: a file with `nlink > 1` (pnpm store links, `cp -l` backups) is replaced by a new inode. The other names keep the old content and nothing tells the user. Notes list this as a known limit. It is filed here so the leader/user can decide. Fix options and cost: (a) refuse with a dedicated error when `nlink > 1` (~10 lines, the user picks Save As); (b) a warning field on the result (~5 lines); (c) in-place write for that case (breaks Review Focus 2 atomicity; not recommended).

[minor] crates/vr-core/src/watch.rs:178-184: own-save suppression is a separate call T6 must remember
Scenario: if T6 calls `ignore_own_write` late (after the 200 ms debounce flush, for example after a slow UI frame) or forgets it, our own save comes back as `Modified`. Reload then makes it a no-op change, so nothing is lost but it adds noise. Fix (optional): document the ordering in the API ("call before returning to the event loop"), or have `Document` return the stamp from `save*` so the call site is obvious.

[uncertain] crates/vr-core/src/document.rs:284, watch.rs:79-83: stamp equality short-circuits the hash on coarse-timestamp file systems
Scenario: on NFS/FAT, or a kernel without multigrain timestamps, an in-place rewrite with the same length in the same timestamp tick as our load or save keeps `(len, mtime, ctime, dev, ino)` identical. It reads as `InSync` and is suppressed, and a later save overwrites it. This depends on the file system and on the timing; it was not reproduced. Fix if wanted: re-hash when `mtime` is within one second of "now" at snapshot time (the racy-git rule).

Checked and fine: temp `create_new` + pid/counter name, cleaned by `TempGuard` on every error path. Order is write → fchown → chmod → fsync → rename → dir fsync, so a crash never truncates the original. Symlinks write through to the target, the link is kept, and loops are capped at 40. Read-only `Other` encodings are refused on all three save paths. Byte-exact CRLF, lone CR, mixed endings, BOM and no final newline: the buffer is verbatim and the BOM is stripped and re-added. `is_modified` uses revision (D26). Reload is one undo step, and a failed load leaves the buffer untouched. The debouncer thread stops on drop (`set_stop` + `notify_all`). D31: ropey resolves only `simd`+`cr_lines` across the workspace (`cargo tree -e features -i ropey`), and `is_line_break`, `line_content_end` and `grapheme_width` agree on `\r` alone.

## Files read
| File | Status |
|---|---|
| Cargo.toml (root, ropey line) | read |
| crates/vr-core/src/lib.rs | read |
| crates/vr-core/src/text.rs (diff + 255-330) | read |
| crates/vr-core/src/document.rs (new) | read |
| crates/vr-core/src/watch.rs (new) | read |
| .ai-dev-team/workspace/04_dev_T4.md | read |
| notify-debouncer-full 0.8.0 src/lib.rs (rescan, rename, Drop) | read (callee contract) |

## Rule check T4
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | diff limited to Cargo.toml ropey line, vr-core lib/text/document/watch |
| BASE-2 | pass | Contract kept. Additions logged in D39; `watch -> Result` flagged. `saved_version` → private `saved_revision` (D26) |
| BASE-3 | pass | Red lines recorded: D31 `left: 9 right: 4`, 21+1 document and 8 watch tests failing on `todo!()` before the code. Tests were written in batches against stubs, not one at a time |
| BASE-4 | pass | trimmed test/clippy/fmt output in notes; reproduced: 61 passed, clippy clean |
| BASE-6 | pass | English; comments explain why; no bug history |
| BASE-8 | pass | no new dependency; ropey feature change per D31; Cargo.lock unchanged |
| BASE-25 | pass | `rustfmt --check` exit 0 on the 4 files; `cargo clippy -p vr-core --all-targets -D warnings` clean |
| BASE-32 | fail | D26 and D31 are honoured. D41 is broken by the permission finding. D24/D25 no-overwrite has an avoidable wide window (findings 1 and 2) |
| AC-3 | pass | is_modified_tracks_saved_version, save_atomic_keeps_mode |
| AC-7 | fail | normal path passes (watch_* tests, reload); after an overflow or rescan, no reload happens and the user is not told (finding 3) |
| AC-18 | pass | open_100mb_under_2s (ignored), 0.25 s in release per notes |
| Review Focus 2 | fail | CRLF, missing final newline, non-UTF-8 read-only and atomic save are OK; the permission bypass and the save window are not |
| Review Focus 3 | pass (with finding 3) | unmodified → reload, deleted → kept (D42), modified → `ChangedOnDisk` |

## Round 1 re-check

Verdict: **CHANGES T4 R1**. 1 new major (introduced by the m3 fix) and 1 minor. Every round-0 finding is fixed.
`cargo test -p vr-core`: `73 passed; 0 failed; 1 ignored`. Diff read: the full current document.rs and watch.rs, plus the lib.rs re-exports.

### Round-0 findings
| # | Status | Check |
|---|---|---|
| 1 permission bypass | fixed | document.rs:647-659: a non-regular target is refused, then an `open(write)` probe. :691 `restored_mode` strips 0o6000 when fchown fails. Tests: save_refuses_file_user_cannot_write (0o464, save and save_overwrite), restored_mode_drops_setid_bits_when_owner_not_restored, save_refuses_non_regular_target |
| 2a check-then-rename window | fixed | :696-703 re-stats right before `rename`. The window went from O(write + fsync) to stat→rename (µs), documented in code (D25). A deleted file being recreated is covered (`Some(None)`). Tests drive a writer at `Stage::BeforeRename` |
| 2b stamp after dir fsync | fixed | :710 fstats right after the rename, and the save snapshot is always `racy`, so the next comparison re-hashes. Test write_right_after_rename_is_not_counted_as_ours |
| 3 rescan/errors dropped | fixed | watch.rs:260-263, 294-310: a rescan gives `Modified`/`Removed` per file and `Rescan` per dir. :205-217: `Err` becomes `FsEvent::Error` |
| m1 move-in | fixed | watch.rs:279 `RenameMode::To` counts as create |
| m2 hard links | fixed | `Saved { other_hard_links }` (D46) |
| m3 ignore_own_write timing | fixed, but see R1-1 | `OwnWrites` is filled by `write_to` |
| uncertain coarse clock | fixed | `RACY_WINDOW` 2 s; a differing length short-circuits |

### Coordinator questions
- **Write probe side effects.** `open(O_WRONLY)` without truncate and with no write changes neither mtime nor ctime nor atime, so our stamp does not move. inotify sees `IN_OPEN` + `IN_CLOSE_WRITE`, which notify maps to `EventKind::Access(..)`. `translate` drops it (watch.rs:265), so our own watcher is not confused and the re-check sees an unchanged stamp. Other tools that watch `close_write` see one extra event per save. That is harmless and not filed.
- **False ChangedOnDisk loop.** None found. `expect` is the stamp of the file reached by following links, and the re-check stats the same inode through `resolve_links`. Our own code changes nothing on the target between them: the probe leaves ctime alone, and chown/chmod act on the temp file. A loop would need ctime to change during every save. A racy snapshot only adds a re-hash, which matches and settles after 2 s. `save_overwrite` skips the re-check, so the user is never stuck. Remaining cost: see R1-2.
- **OwnWrites suppressing a real external write: yes.** See R1-1. Probe result below.
- **API changes.** `save*/save_as -> Result<Saved, DocError>`, `OwnWrites` with `Document::set_own_writes` and `Watcher::own_writes`, `ignore_own_write` removed, and `FsEvent::{Rescan, Error}`. These are additive or breaking changes to the plan contract (BASE-2). They are listed in the notes, and the proposed decision line has not yet been added to decisions.md. D39 still names `ignore_own_write`, and D41 still describes only the 0o444 case. The leader should log the round-1 line and amend D39/D41 so T5/T6 build against the current API.

### New findings
[major] crates/vr-core/src/document.rs:165-178, 450-454 and watch.rs:89-91, 329: a stale own-write entry hides a real external change (AC-7 reload missed)
Scenario: entries are never removed except on `unwatch`, and `is_own` matches by content hash. Steps: our save writes C1, the entry is X→C1. An agent writes C2, which is reported, and T6 reloads to C2. Then `git checkout -- X` (or the agent reverting itself) puts C1 back on disk. The event is suppressed because the disk equals our old save. The buffer keeps showing C2 while the disk holds C1, and no prompt appears until the user's next save. Probe (`qa-artifacts/T4-review/probe`): `after agent write: [Modified(a.txt)]`, reload → buffer "agent edit\n", revert to the saved content → `[]`, `check_disk` → `ChangedOnDisk`. Data is not lost, because `save()` still refuses with `ChangedOnDisk`. But AC-7 / Review Focus 3 fails silently in a common "undo the agent's change" flow. This is a regression: round 0 matched inode+ctime, which a revert never reproduces.
Fix: make an entry one-shot for the state it describes. In `is_own`, remove the entry when the disk no longer matches. Also have `Document::reload` (and any load that replaces `self.disk`) remove its path from `own_writes`. Test: save → external write (event) → reload → write our saved bytes back → `Modified` expected.

[minor] crates/vr-core/src/document.rs:700: the re-check compares stamps only, so a metadata-only change during the write yields a false `ChangedOnDisk`
Scenario: a sync or backup tool sets an xattr or chmod (ctime changes) while a large file is being written. `save()` refuses and T6 shows "changed on disk" although the content is identical. Retrying works, so this is not a loop. Fix (cheap): on a stamp mismatch with equal `len`, re-hash and proceed when it equals the snapshot hash. Only the mismatch path pays for the read.

### Rule check T4 (R1)
| Rule | Result | Evidence |
|---|---|---|
| BASE-2 | pass | contract changes listed in the notes; decisions.md update pending with the leader (see above) |
| BASE-3 | pass | 11 red lines + FIFO hang recorded before the fixes; mutation for failed_rename_removes_temp_file |
| BASE-4 | pass | 73 passed reproduced; clippy/rustfmt output in notes |
| BASE-32 | pass | D25 window narrowed and documented; D41 refusal now covers ownership/ACL; D46 |
| AC-7 | fail | R1-1 (revert to our last saved content not reported) |
| Review Focus 2 | pass | permission, window and racy-stamp fixes verified |
| Review Focus 3 | fail | R1-1 |

## Round 2 re-check

Verdict: **APPROVE T4 R2**. R1-1 and R1-2 are fixed, and I found nothing new.
`cargo test -p vr-core`: `76 passed; 0 failed; 1 ignored`. Read: document.rs `OwnWrites` (:154-203), `reload` (:352-375), `save_to_own_path`/`write_to` (:415-480), `Expected`/`still_fresh` (:640-665), `write_atomic` re-check (:735-770); watch.rs tests :453-510.

| # | Status | Check |
|---|---|---|
| R1-1 | fixed | `is_own` requires `cur == snap.stamp` exactly. Any external write, chmod or replace changes ctime or the inode, so a revert never matches. Our save snapshot is always racy, so content is hashed too. The first mismatch removes the entry, guarded so a newer `record` is not removed. `reload` calls `forget` with the same canonical key as `record`. Tests: revert_to_our_saved_content_is_reported, revert_without_reload_is_reported (mutation red per notes) |
| R1-2 | fixed | `still_fresh`: equal stamp → proceed. Same length → re-hash against the snapshot hash and require the stamp unchanged after the read. Anything else → `ChangedOnDisk`. Test metadata_change_during_save_is_not_a_conflict |

Searched for remaining suppression of a real change:
- A same-tick, same-length in-place rewrite with different bytes keeps the stamp. The racy hash catches it.
- A write between the stat and the hash in `is_own` changes the bytes, so it is reported. A write after `is_own` returns raises a new inotify event, which is a new batch and fails the stamp check.
- A delete is reported as `Removed` before `is_own` is consulted.
- An external write with identical bytes and an identical stamp is suppressed. This is harmless: it is the same content.

Searched for a false ChangedOnDisk loop:
- The pre-rename check fails only on a content change, a length change, or a stamp that moves during the hash read.
- The post-save stamp comes from our handle. If a later stat differs (NFS attribute caching), the hash matches and the snapshot is refreshed, so there is no repeat.
- `save_overwrite` skips the check. The user can never be stuck.

Residual risks (accepted, documented): the µs window between stat and `rename` (D25). Own-write entries for watched files that are never unwatched stay in memory, bounded by the number of files saved.

### Rule check T4 (R2)
| Rule | Result | Evidence |
|---|---|---|
| BASE-2 | pass | no public API change this round; round-1 decisions.md update still with the leader |
| BASE-3 | pass | 2 reds recorded before the fix; mutation for revert_without_reload_is_reported |
| BASE-4 | pass | 76 passed reproduced; clippy/rustfmt output in notes |
| BASE-32 | pass | D24/D25 no-overwrite and no false own-suppression |
| AC-7 | pass | revert tests + earlier watch_* tests |
| Review Focus 2 | pass | — |
| Review Focus 3 | pass | — |

## Round 3 re-check (worktree .worktrees/T4r3, branch t4-round3)

Verdict: **CHANGES T4 R3**, with 1 blocker. It is in the shared helper, so it affects `Document::save` and vr-search replace alike.

Commands:
- `for i in $(seq 20); do cargo test -q -p vr-core document_save_never_loses_a_concurrent_write || break; done` → 20/20 pass.
- `cargo test -p vr-core -p vr-search` → vr-core `95 passed; 1 ignored`, vr-search `34 passed; 1 ignored`, racing_writer `2 passed`.

Probe: `qa-artifacts/T4-review/probe` against the worktree (target dir removed).
Read: the full diff (document.rs, lib.rs, testutil.rs, vr-core Cargo.toml, Cargo.lock, vr-search replace.rs) and all of safe_replace.rs.

### Findings
[blocker] crates/vr-core/src/safe_replace.rs:96-106: an in-place write into our inode during the swap→swap-back gap is deleted
Scenario: writer A writes the target in place after our last check, so the swapped-out file no longer matches. Between our `EXCHANGE` and the swap-back, our inode N sits at `target`. A second in-place write by path in that gap lands in N. Examples: the agent's next edit, a formatter run, or `fs::write`. The gap also includes `unchanged(tmp)` hashing the whole swapped-out file, so it is O(file size), not µs. The swap-back brings N to `tmp`. `file_id(tmp) == ours` passes, because the inode check proves only identity, not content. `remove_ours(tmp)` then deletes the agent's latest version. The path shows the older version v1, and the result is `Changed`, which the UI reports as an ordinary "changed on disk".
Probe (the public `replace_if_unchanged` with the real `exchange_files`; `before_swap` writes "agent v1", the second `swap` call first writes "agent v2 (latest)" by path): `outcome Changed; target now "agent v1\n"; dir ["a.txt"]`. v2 is gone, with no conflict file and no error.
The strict racing test does not exercise this sequence, which needs two writes, one before the swap and one in the gap. That is why 20/20 pass.
Fix: after the swap-back, delete `tmp` only if it is our inode and still holds exactly our bytes. Pass our length + hash, which `write_atomic` already computes as `hasher.finish()`, or an `ours_intact` closure. Otherwise keep it via `keep_conflict` and return `Kept`. The same check applies to vr-search, which gets it through the helper. Test: a seam test like the probe (in-place write on the second `swap` call) → `Kept`, conflict file holds v2, target holds v1. Also add a vr-search variant.

### Audit notes (no finding)
- **`Replaced` path:** the swapped-out file X is content-verified, then deleted. Writes after the verify reach X only through an fd opened before the swap. That version was never visible at the path again, which is inherent to any rename-based save (testutil `visible = false`). Accepted.
- **Swap-back inode check:** this correctly catches a third version renamed in. That version is kept under `<name>.vr-ide-conflict-PID-N`, created by hard link + unlink, so it never replaces anything. The name is cut to 200 bytes at a char boundary, so it stays under NAME_MAX. The message ends "kept at <path>". In `Document`, `guard.0 = None` stops `TempGuard` deleting it. If `keep_conflict` fails, the version stays at the hidden temp name and is still not deleted.
- **Error paths:** every error after the swap becomes `Kept`. Errors before the swap leave only our bytes at `tmp`, so `TempGuard` deletes them safely.
- **Absent target:** `RENAME_NOREPLACE` → `AlreadyExists` refused; hard link fallback (atomic no-replace, `tmp` unlinked after); check+rename only where neither exists (vfat/exFAT). That last window is D63's documented decision. The test save_never_replaces_file_created_at_swap is red→green per notes.
- **EXCHANGE unavailable (EINVAL/ENOTSUP):** check+rename window, documented in the module and in D63, with mutation evidence (3/20). Linux ext4/btrfs/xfs/tmpfs support EXCHANGE.
- **vr-search:** behaviour is unchanged except for the strengthened swap-back. `commit_replace` maps Replaced → sync_dir + Ok, Changed → `changed_on_disk()`, Kept → sync_dir + "kept at" error, and Err → remove tmp + Err. These are the same mappings as the old `commit_inner`/`TmpHolds`. The test module is untouched (diff shows non-test lines only), the seams (`swap`, `before_swap`) are kept and pass, and `TEMP_SEQ` is still used for temp names.
- **`save_overwrite` / `save_as`:** plain `rename` (document.rs `None =>` arm), by design per D63. The user chose to replace. A version written after the user's choice is still replaced. If the leader wants that closed too, `save_overwrite` could carry the stamp the user saw at the prompt. That is a leader decision, not filed.
- **New dependency:** rustix is a workspace dependency (D25/D45). It is added to vr-core only, and Cargo.lock gains one line.

### Rule check T4 (R3)
| Rule | Result | Evidence |
|---|---|---|
| BASE-2 | pass | Document API unchanged; `pub mod safe_replace` additive (D63) |
| BASE-3 | pass | reds recorded (3 document, 4 helper, strict 2/20) before the code |
| BASE-4 | pass | reproduced: 20/20 strict, vr-core 95 / vr-search 34+2 passed |
| BASE-8 | pass | rustix from the workspace manifest, per D25/D45/D63 |
| BASE-16 | pass | T9 commit logic moved, not duplicated |
| BASE-32 | fail | D24/D25/D63 no-overwrite: blocker above |
| AC-3 | pass | save atomic; save_* green |
| Review Focus 2 | fail | blocker above (a concurrent writer's latest version deleted) |

## Escalation re-check (worktree .worktrees/T4r3, uncommitted on 1188018)

Verdict: **CHANGES T4 ESC**, with 1 major (test rule) and 2 minor. The production protocol is correct: no blocker remains.

Commands:
- Strict loop 20× (`cargo test -q -p vr-core --lib document_save_never_loses`, 2 tests per run) → 20/20 pass. Every run reported `kept=0` for both the normal and the fast writer.
- `cargo test -p vr-core -p vr-fs -p vr-search` → vr-core 112 passed / 1 ignored; vr-fs 43 / 1 ignored + racing_writer 2; vr-search 35 / 1 ignored + racing_writer 2.

Scope note: `git diff master -- crates` also shows vr-ide deletions (≈4 000 lines). These come only from the branch base (1188018) predating master's T5 commit 07061ec. Master has no change under vr-core, vr-fs or vr-search since that base, so the T4 change is the 7 files in `git diff --stat` and merges cleanly. Merge only those, never "the worktree state".

Read: all of safe_replace.rs (non-test code line by line, the test names, the seam tests for the R3 sequence). The full diffs of document.rs, lib.rs, testutil.rs, vr-search replace.rs and racing_writer.rs. No stray or mis-inserted code was found after the reported perl incident: every hunk is intentional, imports are used, and clippy/rustfmt are clean per notes.

### State machine vs code
| Step | Code | Outside W / R / U / C | Verdict |
|---|---|---|---|
| 1 check | `target_unchanged` (stamp short-cut allowed) | seen → `remove_unexposed` (N never at T) | ok |
| 2 EXCHANGE | `swap`; EINVAL/ENOTSUP → D63 fallback; ENOENT → Changed (atomic failure, N unexposed); other Err → caller deletes the unexposed N | U → ENOENT; C after U → swapped like X; R of symlink/dir → X non-regular | ok |
| 3 prove X | `proven(tmp, None, holds_expected)`: lstat regular, content (Document: len + hash; vr-search: bytes), lstat equal after the read | a coarse-clock same-tick same-length write passes step 1 but fails the content proof here | ok |
| 4 unlink X | after `BeforeDeleteOld` | only fd-holders can still write X (D75/D76 accepted class) | ok |
| 5 swap back | `restore`; Err (U of T → ENOENT, others) → `keep` X at K | W into N / R over N → Y | ok |
| 6 prove Y | `proven(tmp, Some(ours), holds_ours)`: same dev/ino/len/mtime as `ours` (taken after fsync, before step 1), content = our bytes, lstat stable | in-place write, O_TRUNC, restored mtime (content catches it), rename-in (identity) | ok |
| 7 unlink N | after `BeforeDeleteOurs` | fd-holders only (accepted) | ok |
| K | `RENAME_NOREPLACE` → `link`+unlink of the name → else stays at P and P is reported | never replaces | ok |

The unlink rule holds. `remove_unexposed` is called only before exposure (steps 1, 2-ENOENT, 2-fallback refused). `fs::remove_file(tmp)` runs only right after a `proven(..) == Ok(true)`. `Err` cannot leave after step 2, so the callers' delete-on-Err (Document `TempGuard`, vr-search `Err(e)` arm) only ever removes the unexposed N. My R3 sequence is covered by `in_place_write_between_swap_and_swap_back_is_kept`, the vr-search twin and `save_keeps_second_write_made_while_restoring` (v2 kept at K, T = v1). M1/M2/M3 mutants were recorded red.

Symlinks: Document resolves links before the commit. A symlink or directory put at T during the save is never followed or deleted: `proven` is lstat-based, the symlink goes back to T via swap-back (`symlink_put_in_place_before_swap_is_restored`), and `keep_conflict` moves a symlink itself, not its target. Step 1's `still_fresh` follows links, which can only cause a refusal, never a loss.

Conflict naming: `<name≤200 bytes at a char boundary>.vr-ide-conflict-PID-SEQ`. `RENAME_NOREPLACE` comes first. EEXIST → next SEQ, other errors → hard link, hard-link failure → the file stays at P and P is named in the error. Nothing is replaced and nothing is deleted. OK.

D64 API: `disk_version()` (absent = a version) and `save_overwrite_seen()` refuse early via `still_fresh`, then commit through the same `replace_if_unchanged` with `Expected` = the seen version, or `create_if_absent` for absent. The tests cover a newer version arriving before the commit, a newer version arriving at `BeforeSwap`, and a file created where none was shown. `save_overwrite`/`save_as` keep a plain rename, by design (D74). OK.

### Findings
[major] crates/vr-core/src/testutil.rs:222-226 (`no_lost_write`, `put_back_on_error`): the `_swapping` rule drops the only racing check for the R3 loss class on refused runs
Scenario: on a refused run the rule skips `overtaken` completely. Under round-3 code, v1 lands before the swap, v2 is written in place into our file while it sits at T, and v2 is deleted with our file. That run is refused and v2 "saw ours". The writer's later versions make the final content a writer version. The leftover check passes because no file is left, so the strict test passes while v2 is in no file. In other words, `document_save_never_loses_a_concurrent_write` and `apply_replace_never_loses_a_concurrent_write` would not detect the regression they are named for. The gap is currently guarded only by the deterministic seam tests, and the race never reached it in any loop (`kept=0` in all 20 runs here and in the developer's 50). D76.2 allows our bytes to be seen on a refused run. It does not require giving up the "nothing lost" check for those runs.
Fix (keeps D76.2's flake-avoidance): on a refused run, replace the blanket exemption with a survival check. Every version `v` whose `saw_before == ours` must be found in the final content, or in the kept copy the error names (the test already reads it), or be overwritten by the writer itself (a later version with `saw_before == v.bytes`). Red evidence: the round-3 `restore` without `holds_ours` and a seam/racing run that places a write at `BeforeSwapBack`, or a unit test of the rule with that version list.

[minor] crates/vr-core/src/document.rs (`disk_version`): the stamp is taken before the hash read, and stability is not checked
Scenario: a writer finishes between `metadata()` and the read. `DiskVersion` then pairs the old stamp with the new content's hash. `save_overwrite_seen` → `still_fresh` → the stamp differs, the length is the same, and the hash equals the new content → it replaces a version the prompt may not have shown, since T6 reads the displayed text separately. Fix: re-stat after the read and retry or refuse if the stamp moved. Document for T6 that `disk_version()` must be taken before reading the text it shows, or have `DiskVersion` carry the bytes it hashed.

[minor] crates/vr-core/src/document.rs (`write_atomic`, `Outcome::Kept` arm): `disk_state` is not set to `ChangedOnDisk` when a save ends with a conflict copy. The developer listed this. `check_disk()` corrects it on the next call, but T6 gets `DocError::Io` while the state still reads `InSync`. Fix: set `disk_state = ChangedOnDisk` before returning the kept error.

### Rule check T4 (ESC)
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | 7 T4 files only (vr-ide lines in `diff master` are branch-base lag, not edits) |
| BASE-2 | pass | Document contract unchanged; additive D64 API; `replace_if_unchanged` signature change logged in D73 |
| BASE-3 | pass | reds 1–3 before the code; guards seen red by mutants M1–M5 |
| BASE-4 | pass | reproduced: strict 20/20, 112 / 43+2 / 35+2 passed |
| BASE-6 | pass | module doc states the protocol; comments explain why |
| BASE-8 | pass | no manifest change this round |
| BASE-32 | pass | D63/D73–D76 implemented as decided |
| AC-3 / Review Focus 2 | pass | production protocol loses nothing in every modeled gap |
| Review Focus 3 | pass | D64 flow |
| test rule (D76.2) | fail | major above: the swapping rule is not strict enough |

## Escalation follow-up 2 re-check (worktree 269ce36 + merge 51d3a8c)

Verdict: **APPROVE T4 ESC2**. All three items are fixed. One minor (test-oracle limit) is filed for the record and does not block.

Commands:
- `cargo test -p vr-core -p vr-fs -p vr-search -p vr-ide` → vr-core 117 passed / 1 ignored; vr-fs 43 / 1 ignored + racing_writer 2; vr-search 35 / 1 ignored + racing_writer 2; vr-ide 54 passed / 2 ignored. T5's vr-ide compiles and passes against the new vr-core.
- Strict loop 20× (`--lib document_save_never_loses`, 2 tests per run) → 20/20 pass, with `kept=0` in all 40 runs.

Read: testutil.rs `no_lost_write` and `assert_versions_accounted_for` (:160-300), document.rs `disk_version` (:354-384), `write_to` (:520-545), `write_atomic` Kept arm (:865-880), and the vr-search racing_writer kept wiring (:46-57).

| # | Status | Check |
|---|---|---|
| 1 oracle | fixed | On a refused run, every version that was visible or saw ours must be the final content, in a named conflict copy, or seen by a later writer version. Traced on the R3 loss (v1 written before the swap, v2 written into our file and deleted, v3 later): v1 survives (v3 saw it). v2 saw ours, so it has no excuse and is in no file → **flagged**. This is pinned by `swapping_rule_flags_version_written_into_our_file_and_deleted` (red before the fix per notes). The rule is also wider than my proposal: `swapping_rule_flags_deleted_version_that_never_saw_ours`. Runs that reported success keep the old rules. |
| 2 disk_version | fixed | stamp → hash → stamp, by path, retried up to 5×, then `Io("keeps changing")`. ABA is impossible because the stamp includes ino + ctime. Tests `disk_version_is_of_one_version_of_the_file` and `…keeps_changing_is_an_error`. |
| 3 disk_state | fixed | `Written::Kept` → `write_to` sets `ChangedOnDisk`, returns the "kept at" error; asserted in `save_keeps_second_write_made_while_restoring`. |

### The one exception (`replaced_by_next_write`)
[minor] crates/vr-core/src/testutil.rs (`assert_versions_accounted_for`, `replaced_by_next_write`): the exception excuses a real loss whenever that loss looks the same as the legitimate case
Scenario: a protocol regression that deletes the swapped-out X (v1) and does not swap back, but reports a refusal. The path then keeps ours until the writer's next in-place write v2, which saw ours. The oracle sees v1 (did not see ours) followed by v2 (saw ours) → excused, and v2 is the final content. Probe (synthetic versions through the public `assert_no_lost_write_swapping`, `qa-artifacts/T4-review/probe`): `oracle on 'v1 deleted, never restored': PASSES (loss not flagged)`. The same observation also occurs legitimately: the writer reads ours, our swap-back restores v1, and the writer truncates v1. So the oracle cannot tell the two apart with `saw_before` read by path before the write.
Why it does not block: that regression class (a missing or wrong swap-back) is caught deterministically by the seam tests: `write_between_check_and_swap_is_swapped_back` (asserts the agent's bytes are at T, exchange and fallback), `save_never_replaces_in_place_write_at_swap`, and the vr-search twin. The race never reaches the gap anyway (`kept=0` in all runs).
Fix (optional, test-only): record what each write actually replaced instead of what the path showed beforehand. For InPlace, open without truncate, read through the handle, then `set_len(0)` and write. For TempRename, `RENAME_EXCHANGE` the writer's temp with the path and read what came out before unlinking it. Then "overwritten by the writer itself" becomes exact, and the exception can be removed.

### Rule check T4 (ESC2)
| Rule | Result | Evidence |
|---|---|---|
| BASE-2 | pass | Document contract unchanged; `DiskVersion: PartialEq + Eq`, test-util `kept` argument (D78) |
| BASE-3 | pass | five reds recorded before the fixes |
| BASE-4 | pass | reproduced: 117 / 43+2 / 35+2 / 54 passed, strict 20/20 |
| BASE-32 | pass | D64/D77/D78 |
| AC-3 / AC-7 / Review Focus 2–3 | pass | — |
