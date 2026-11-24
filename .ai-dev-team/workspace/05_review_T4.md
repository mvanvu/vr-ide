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
