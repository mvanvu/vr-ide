# Review T7 — vr-fs tree, file ops (trash), fuzzy file index

Verdict: **CHANGES T7** (CHANGES_REQUESTED)

Tools available: Read, Bash, Write, Skill, SendMessage, SubagentHandback.
Skills loaded: ai-dev-team:code-review-checklist, ai-dev-team:security-review-checklist.
Reviewer run: `cargo test -p vr-fs` → 19 passed, 1 ignored; `cargo clippy -p vr-fs --all-targets -- -D warnings` → clean; `rustfmt --check` on the vr-fs files → clean.

Data-safety focus, short answers:
- a) No permanent delete in `trash()`. trash 5.4.0 has no remove fallback; a missing trash returns an error. Exception: the EXDEV move fallback (F1).
- b) Symlinks are acted on as links: rename, `symlink_metadata`, copy-as-link, and trash 5.4.0 canonicalizes only the parent (`lib.rs:308-335`). `..` and absolute paths are not confined (F10).
- c) Refused only by check-then-act; the window is not documented (F2).
- d) The EXDEV move leaves a partial copy behind, and it can permanently lose files written during the copy (F1).
- e) A case-only rename is refused on case-insensitive filesystems (F5).
- f) The tests cover the plain refusals but not a)–e) (F4).

## Findings

[major] crates/vr-fs/src/ops.rs:196-211 — the cross-device move copies and then permanently deletes the source
Scenario: `move_to`/`rename` into a dir on another mount (for example a symlinked dir that points to a second disk) → `copy_recursive`, then `remove_dir_all(from)`. (1) A file that an agent or build writes into the source after `read_dir` has passed it is never copied and is then permanently deleted. (2) When the copy fails halfway (EACCES on one file, ENOSPC), a partial tree is left at `to` and the error names only one path. A retry then fails with `AlreadyExists`. (3) A FIFO in the tree makes `fs::copy` block forever. No test covers this path.
Fix: return a new `FsOpError::CrossDevice` on EXDEV (the UI can offer copy-then-delete). Or, after a fully successful copy, send the source to the system trash instead of `remove_*`, and on a failed copy return `PartialCopy { to }`. Skip or refuse special files. Add a test through an injectable rename seam.

[major] crates/vr-fs/src/ops.rs:72-76, 81-85, 229 — no-overwrite is check-then-act, and `fs::rename`/`fs::copy` replace an existing target
Scenario: `ensure_absent(to)` passes, then an AI agent or watcher-driven tool creates `to`, then `fs::rename` (or `fs::copy`, which opens with O_TRUNC) overwrites it, and that file is lost. Agents writing under the editor is the product's core case. The code comment says "refuses to replace", and the window is not documented.
Fix: for files and symlinks, use `fs::hard_link(from, to)` (atomic, fails with EEXIST, does not follow the link) and then `fs::remove_file(from)`. Fall back to the current path only on EPERM/unsupported (vfat), with the window documented. For dirs, `rename` can only replace an empty dir: document that. For copy of a file, use `File::create_new(to)` + `io::copy` + `set_permissions`. Alternatively ask the leader for `rustix` so you can use `renameat2(RENAME_NOREPLACE)`.

[major, uncertain — static analysis, not probed (needs a second filesystem)] crates/vr-fs/src/ops.rs:124 → trash-5.4.0 src/freedesktop.rs:591-660 — trashing a symlink to a directory across devices loops forever
Scenario: the project is on a mount whose root is not writable, so the per-volume trash fails with PermissionDenied and the crate falls back to the home trash on another fs. The crate creates a file placeholder (the source is a symlink), `rename` fails with EXDEV, and `src.is_dir()` follows the link. `copy_dir_all` → `create_dir_all(placeholder file)` returns EEXIST, which reads as AlreadyExists, so `move_to_trash` does `continue`. The loop then repeats without end, leaving a new empty `name.N` file in `Trash/files` on each pass, and the UI hangs. A symlink to a file is trashed as a copy of its target content.
Fix: in `SystemTrash::trash`, refuse a symlink path when its parent's `st_dev` differs from the trash dir's `st_dev` (an error, not a delete), or pin a fixed crate version. Add a test for a symlink to a dir that lives outside the project: the link is trashed and the target survives.

[major] crates/vr-fs/src/ops.rs:250-467, tree.rs tests — data-safety tests missing (ledger: written without a red run)
Scenario: a regression on any of these goes green.
Missing tests:
- the trash is unavailable (`XDG_DATA_HOME` read-only in the child) → `Err`, and the file is still on disk;
- trashing a symlink to an outside dir keeps the target;
- `rename`/`move_to` of a symlink moves the link and leaves the target untouched;
- the EXDEV path (F1);
- a failure partway through a copy (an unreadable file inside the dir) → the source is intact;
- `a.txt` → `A.txt`;
- copy of a dir onto an existing dir is refused and the contents are unchanged.

[minor] crates/vr-fs/src/ops.rs:72-75 — a case-only rename is refused on case-insensitive filesystems (vfat/exfat/ntfs, ext4 casefold)
Scenario: `rename(a.txt, A.txt)` → `ensure_absent` sees the same inode → "A.txt already exists". No data is lost, but the user cannot do the rename.
Fix: when `to` exists and is the same file as `from` (same dev+ino) and the names differ only in case, rename through a temporary unique name. Add a test (on Linux it must still succeed).

[minor] crates/vr-fs/src/ops.rs:209, 229 — destination errors are reported against `from`
Scenario: `rename(x, missing_dir/y)` → "x does not exist".
Fix: map errors to `to` when `from` still exists.

[minor] crates/vr-fs/src/ops.rs:106-125 — a partial batch trash is not reported
Scenario: the second of three paths fails → the first is already in the trash, and the UI only gets "trash failed".
Fix: trash the paths one by one and return the trashed and failed lists.

[minor] crates/vr-fs/src/file_index.rs:188 — cancellation is polled only every 5 000 files
Scenario: a rebuild drops the old receiver, but a slow or network tree with fewer than 5 000 files walks to the end. `file_index_stops_when_receiver_dropped` passes against an implementation that never cancels.
Fix: check an `Arc<AtomicBool>` per entry (the same shape as vr-search) and assert an early return in the test.

[minor] crates/vr-fs/src/tree.rs:350-365 — `tree_lazy_children` passes against an eager walk that swallows errors, and fails when run as root.
Fix: skip the test when the euid is 0, and assert on the exact error kind of the inner call.

[minor] crates/vr-fs/src/tree.rs:98, ops.rs (all) — no root confinement
Scenario: `children(root/../etc)` passes the lexical `strip_prefix`, and ops accept `..` or absolute paths, so a name typed in T8 such as `../../x` lands outside the project.
Fix: reject `ParentDir` components in `children`. T8 must validate typed names (no `/` or `..`); add that to the T8 brief.

## Files read
- crates/vr-fs/src/lib.rs — read
- crates/vr-fs/src/tree.rs — read
- crates/vr-fs/src/ops.rs — read
- crates/vr-fs/src/file_index.rs — read
- crates/vr-fs/Cargo.toml — read (unchanged)
- trash-5.4.0 src/lib.rs:300-335, src/freedesktop.rs:30-75, 491-730 — read (callee of `trash()`)

## Rule check T7
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | diff only in crates/vr-fs/src + team files |
| BASE-2 | pass | plan vr-fs signatures kept; additions listed in the notes' Contract section |
| BASE-4 | pass | notes carry the test, clippy and fmt output; reproduced by the reviewer (19 passed, clippy clean) |
| BASE-6 | pass | English, why-comments, no bug history |
| BASE-8 | pass | Cargo.toml unchanged; workspace deps only |
| BASE-16 | pass | capabilities.md `## vr-fs` lines added; no duplicated helpers |
| BASE-25 | pass (crate) | clippy -p vr-fs clean, rustfmt clean; the workspace guard was red only in vr-core/vr-config, which belong to other tasks |
| BASE-32 | pass | D9: `trash()` → system trash only; D15–D18 implemented (require_git off, dotfiles indexed, `Symlink(Loop)` not expandable, 200 000 cap with `truncated`) |
| AC-8 | fail (vr-fs part) | ops exist and refuse plain overwrites, but the race (F2) and EXDEV (F1) paths can lose data |
| AC-9 | pass | tree_hides_gitignored_by_default, tree_toggle_ignored_shows_them, file_index_respects_gitignore |
| D9 | pass | no permanent delete in `trash()`; an unavailable trash returns an error (crate source checked) |

## Round 1 re-check

Verdict: **APPROVE T7 R1** (no blocker or major left; minors below, none can lose data)

Read: `git diff HEAD -- crates/vr-fs Cargo.toml Cargo.lock` and the untracked `ops.rs` (whole file, 1094 lines), `tree.rs` (changed `children`, `read_dir_counted`, tests) and `file_index.rs` (`walk`, tests); notes `## Fix round 1`; D24 and D25. Reviewer run: `cargo test -p vr-fs` → 36 passed, 1 ignored.

| Earlier finding | Status | Evidence |
|---|---|---|
| F1 EXDEV copy+delete | fixed | `move_with`/`move_err` map EXDEV to `CrossDevice`; no copy and no `remove_*` left in ops; `move_cross_device_refused` injects `Errno::XDEV` for a dir and a file and asserts the source is intact and nothing was copied (D24) |
| F2 check-then-act | fixed | `renameat2(RENAME_NOREPLACE)` through rustix (D25); on EINVAL/ENOSYS files use `hard_link` + `remove_file`; dirs use a checked rename whose window is documented (rename(2) can replace only an empty dir); copy uses `create_new`/`mkdir`/`symlink`; `move_fallback_without_renameat2_never_overwrites`, `rename_existing_target_refused_dirs_too`, `copy_dir_onto_existing_refused` |
| F3 symlink trash across devices | fixed (safe, over-refuses; see m3) | `check_symlink_trash` refuses before calling the crate; tested with /proc as the other device |
| F4 missing tests | fixed | each item of F4 now has a test; red runs recorded in the notes |
| case-only rename | fixed | `is_case_only_same_file` (same dev+ino, same parent, names equal ignoring case) → `rename_via_temp`; restore path tested with a hard-link pair (refused, original restored, no temp left) |
| destination errors | fixed | `destination_errors_name_the_destination` |
| batch trash | fixed | `BatchResult` per path in order, a failure does not stop the rest; `trash_batch_reports_per_item`, `trash_unavailable_keeps_file` (the file stays) |
| index cancellation | fixed | receiver checked every 256 entries of any kind; the test asserts ≤ 256 visited |
| tree_lazy_children | fixed | listing count = 1 through the single `read_dir_counted` seam; skipped when running as root |
| `..` in `children` / no confinement | fixed | `children` refuses `ParentDir`; `validate_name` and `ensure_within` added for T8 |

### New findings (all minor)

[minor] crates/vr-fs/src/ops.rs:421-423 — `hard_link` succeeds, then `remove_file(from)` fails
Scenario: both names are left on the same inode, and the error (`Io{path: from}`) does not say that `to` now exists. No data is lost, but T8 would treat the rename as failed while the tree shows two entries. This needs a filesystem without NOREPLACE where link is allowed and unlink is denied (a sticky dir with a different owner).
Fix: on failure, `remove_file(to)` only if `same_file(from, to)` still holds (that unlink loses nothing). Otherwise return an error that names both paths.

[minor] crates/vr-fs/src/ops.rs:364-372 — the case-only rename's restore can fail silently
Scenario: step 2 fails, and the move back fails too (for example something created `a.txt` in the window). The file then stays under the hidden name `.a.txt.vr-tmp-<pid>-<n>`, which the tree hides by default, and the returned error is only step 2's error.
Fix: when the restore fails, return an error that names the temp path, for example `TrashRefused`-style `{path: tmp, reason}` or a new `LeftAt(tmp)`.

[minor] crates/vr-fs/src/ops.rs:250-268 — `check_symlink_trash` also refuses symlinks the crate would handle safely
Scenario: on a secondary volume whose per-volume `.Trash-$uid` is usable, the link and the trash are on the same device, so no loop happens. The check compares against the home trash only, so the user cannot delete any symlink on that disk (error: "symlink on another filesystem than the trash"). This is safe, but the feature does not work there.
Fix: accept the link as well when `$topdir/.Trash-$uid` (or `$topdir/.Trash/$uid`) exists or can be created on the link's own device. Or accept this and keep the message.

[minor] crates/vr-fs/src/ops.rs:104-117 — `ensure_within` resolves `..` lexically and accepts the root itself
- Symlinks: `root/link/../x` normalizes to `root/x`, while the kernel would resolve it to the link target's parent, outside the root. This is safe only if callers use the returned path, never the raw input, and the doc comment does not say so. Not catching links that point outside is correct under D17: a link the user made is the user's choice, and every destructive op acts on the link itself (tested).
- `ensure_within(root, "")` returns the root itself, so T8 must not offer rename or trash on the root node.
Fix: add "use the returned path" to the doc, and for rename/move/trash require a path strictly below the root (or have T8 check it). Put both in the T8 brief.

[minor] T8 contract note — batch trash of nested selections
Scenario: selecting `dir` and `dir/a.txt` together gives `a.txt` a `NotFound` result even though it is in the trash with `dir`.
Fix: T8 drops selected paths that lie under another selected path before calling `trash`.

### Rule check T7 R1
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | crates/vr-fs/**, plus root Cargo.toml (rustix) and Cargo.lock (+1 line), both covered by D25 |
| BASE-2 | pass | `trash` → `BatchResult` is the change the leader asked for, listed in the notes; other plan signatures kept |
| BASE-4 | pass | notes round 1 list test, clippy and rustfmt output; reproduced: 36 passed |
| BASE-6 | pass | English, why-comments only |
| BASE-8 | pass | rustix 1.1.6 approved by D25, Linux-only target dependency |
| BASE-16 | pass | capabilities.md ops line updated |
| BASE-25 | pass (crate) | clippy -p vr-fs clean per notes |
| BASE-32 | pass | D9 (trash only; an unavailable trash is an error and the file is kept), D24, D25, D15–D18 |
| AC-8 | pass (vr-fs part) | no overwrite (atomic), no cross-device copy+delete, per-item trash results |
| AC-9 | pass | unchanged tests green |
| D9 | pass | no permanent-delete path left in ops |

## Quick pass 1 re-check

Verdict: **CHANGES T7 QP1** (1 major)

Read: `git diff HEAD -- crates/vr-fs` (base 167c2fa; only `ops.rs` changed) and notes `## Quick pass 1`. Reviewer run: `cargo test -p vr-fs` → 39 passed, 1 ignored.

| Round-1 minor | Status |
|---|---|
| hard_link ok, unlink of `from` fails | the error is now reported, but the undo opens a data-loss window (Q1) |
| case-only restore fails | fixed: `LeftAt { path: tmp, intended, source }`; tested with a seam that makes step 2 and the move back fail |
| `check_symlink_trash` over-refusal | accepted as a refusal; the message now says the link and its target are untouched, and the test asserts the text |
| `ensure_within` doc | fixed: "act on the returned path" and "root accepted, never offer rename/move/trash on it" are in the doc comment |
| nested batch trash | moved to the T8 brief (notes) |

[major] crates/vr-fs/src/ops.rs:497 — the undo of the extra hard link checks, then unlinks by path, so a user's new file at `to` can be deleted
Scenario: this code runs on the fallback path, which is a filesystem without RENAME_NOREPLACE but with hard links, such as NFS. `link(from, to)` succeeds and `unlink(from)` fails. `same_file(from, to)` is checked and holds. Then, before `remove(to)` runs, an agent or editor does its atomic save of `to` (write a temp file, then `rename(temp, to)`). `unlink(to)` now removes that new file. Its contents were never anywhere else, so they are lost for good. Unlink works by name, not by inode, so no check before it can close the window. The comment "unlinking `to` cannot lose anything" is true only at the moment of the check.
Fix (pick one):
- (a) Simplest and safe: drop the undo. When `remove(from)` fails, always return `BothNamesLeft { from, to, source }`. No delete happens, both names are reported, and the UI can let the user trash one of them. Update `link_move_undone_when_old_name_cannot_be_removed` to assert `BothNamesLeft` and that both names still exist.
- (b) Move `to` aside to a unique private temp name first with `rename(2)`, which is atomic: whatever is at `to` goes to the temp name. Then compare the temp name's dev+ino with `from`. If they match, unlink the temp name, which no one else uses. If not, rename it back with a checked rename, and if that fails return `LeftAt`.
(a) is preferred, because this path is already a rare double-failure case.

[minor] crates/vr-fs/src/ops.rs:500 — `BothNamesLeft` is also returned when `same_file` is false
Scenario: `to` was replaced before the check, so the two names no longer reach the same file. The message "both name the same file" is then wrong.
Fix: with (a) this cannot happen any more. With (b), return `LeftAt` or a separate message.

### Rule check T7 QP1
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | ops.rs only |
| BASE-2 | pass | additive error variants only |
| BASE-4 | pass | red/green output in notes; reproduced 39 passed |
| BASE-6 | pass | English why-comments; the comment at :495-496 states an invariant that does not hold (Q1) |
| BASE-32 | fail | the no-overwrite/no-loss guarantee (D25) is broken by Q1's unlink-by-path window |
| D9 | pass | trash path unchanged |

## Round 2 re-check

Verdict: **CHANGES T7 R2** (1 major)

Read: `git diff HEAD -- crates/vr-fs` (base 167c2fa, QP1 + round 2) and the notes `## Fix round 2`. Then I listed every call in `ops.rs` that removes, renames onto or truncates a name. Reviewer run: `cargo test -p vr-fs` → 38 passed, 1 ignored.

| QP1 item | Status |
|---|---|
| Q1 the undo removed `to` by name | fixed: `finish_link_move` no longer unlinks `to`. It returns `BothNamesLeft`, and the test denies only the removal of `from`, so any unlink of `to` would show |
| wrong `BothNamesLeft` text | fixed: "the file now has both names …; nothing was deleted" |
| comment at :495-496 | replaced with an accurate why-comment |

Every call in `ops.rs` that removes, renames onto or truncates a name:
- `renameat2(NOREPLACE)`: atomic; it cannot replace and takes whatever is at `from` → safe.
- `create_new`, `create_dir`, `symlink`, `hard_link`: all fail if the name exists → safe.
- `ops.rs:486` checked `fs::rename`: this is the vfat/exfat fallback. The window is documented and accepted in D25 → accepted, not re-filed.
- `ops.rs:465` → `finish_link_move` → `fs::remove_file(from)`: this unlinks a shared name. See R2-1.

[major] crates/vr-fs/src/ops.rs:465, 494 — the link-based move unlinks `from` by name, so a file saved to `from` in between is deleted
Scenario: this runs on a filesystem without RENAME_NOREPLACE but with hard links, such as NFS. `hard_link(from, to)` succeeds. Then an agent or editor does an atomic save of `from` (writes `from.tmp`, then `rename(from.tmp, from)`). Then `remove_file(from)` unlinks that new file. `to` still holds the old contents, so the newly saved contents are lost for good. A plain rename(2) cannot lose a save this way, because it moves whatever is at `from` in one step. Only the two-step link-then-unlink has the window. This is the same kind of problem as Q1, now on the source name. It is also more reachable than Q1, because it only needs this fallback filesystem and a save during the move; no unlink failure is required.
Fix: never unlink a name that someone else can write to. Grab the source first:
1. `fs::rename(from, tmp)`, where `tmp` is a unique private name in the same dir (the `rename_via_temp` naming). This atomically takes whatever is at `from` at that instant.
2. `fs::hard_link(tmp, to)`, which is no-replace and atomic.
3. `remove_file(tmp)`, which is safe because no one else uses that name.
4. If step 2 fails (EEXIST etc.), restore with `hard_link(tmp, from)` (no-replace) and then `remove_file(tmp)`. If `from` has been taken meanwhile, return `LeftAt { path: tmp, intended: from }`.
5. If step 3 fails, return an error naming `tmp`. Nothing is lost.

Test: with a remove seam, assert that `remove` is only ever called with the private temp name, never with `from` or `to`.

### Rule check T7 R2
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | ops.rs only |
| BASE-2 | pass | no signature change |
| BASE-4 | pass | red line in notes; reproduced 38 passed |
| BASE-6 | pass | accurate why-comment in `finish_link_move` |
| BASE-32 | fail | D25's no-loss guarantee: the link fallback still unlinks `from` by name (R2-1) |
| D9 | pass | trash path unchanged; nothing deleted permanently except the R2-1 source unlink |

### QC observation — read-only target directory reported as "already exists"
Probe (`qa-artifacts/T7-review/probe`, a scratch binary against vr-fs, ext4, uid 1000, target dir mode 0555, target name absent):
```
move_to:     Err(Io { path: ".../ro/a.txt",   source: Os { code: 13, kind: PermissionDenied } })
rename:      Err(Io { path: ".../ro/b.txt",   source: Os { code: 13, kind: PermissionDenied } })
copy:        Err(Io { path: ".../ro/c.txt",   source: Os { code: 13, kind: PermissionDenied } })
create_file: Err(Io { path: ".../ro/new.txt", source: Os { code: 13, kind: PermissionDenied } })
src intact: true
```
I could not reproduce it on the main path. `renameat2` returns EACCES, `move_err` names `to`, and `io_err` maps PermissionDenied to `Io`, not to `AlreadyExists`. On the fallback path (renameat2 EINVAL), an EACCES from `hard_link` falls through to the checked rename, which also ends in `Io(PermissionDenied)`. The code can report `AlreadyExists` only when EEXIST really came back or `exists(to)` is true. With RENAME_NOREPLACE the kernel reports EEXIST before the write-permission check, so an existing target name in a read-only dir correctly gives "already exists".
Severity if it is real: **minor**. The operation is refused, the source is intact, and only the reason shown is wrong: nothing is lost and nothing fails silently.
QC should state the filesystem, the exact op (vr-fs call or T8 UI), whether the target name already existed, and whether the test ran as root or injected the fallback seam. If it happens only in T8, it is a T8 error-mapping item.

## Round 3 re-check

Verdict: **APPROVE T7 R3** (R2-1 fixed; 2 minor items left, neither can lose data in a realistic state)

Read: `git diff HEAD -- crates/vr-fs` (base 167c2fa, QP1 + R2 + R3): `temp_sibling`, `rename_via_temp`, `move_with`, `LinkOps`, `link_move`, `restore`, `links_unsupported`, `dest_err`, and the 5 new `link_move_*` tests. Notes `## Fix round 3`; D25 and D32. Reviewer run: `cargo test -p vr-fs` → 42 passed, 1 ignored. QC's read-only message observation was withdrawn: the target existed, so "already exists" was correct.

R2-1: fixed. `link_move` takes the source first (`rename(from, tmp)`), then `link(tmp, to)`, then `remove(tmp)`.
- `link_move_only_removes_its_own_temp_name` asserts that `remove` is never called with `from` or `to`.
- `link_move_keeps_a_save_made_during_the_move` proves that a save made to `from` mid-move survives.
- Restore links back to `from` without replacing it, and returns `LeftAt` when `from` was taken meanwhile.
- `BothNamesLeft` names the temp path.

Every call in `ops.rs` that removes, renames onto or truncates a name:
| Call | Target name | Can it hit someone else's file? |
|---|---|---|
| `renameat2(NOREPLACE)` (`rename_noreplace`) | `to` | no: atomic, and it takes whatever is at `from` |
| `create_new`, `create_dir`, `symlink`, `hard_link` | new names | no: they fail if the name exists |
| `fs::rename` `ops.rs:477` (directories, fallback) | `to` | can replace only an empty directory (documented) |
| `sys_rename` step 1 in `link_move` | private temp | only on a temp-name collision (m1) |
| `sys_rename` temp → `to` / temp → `from` (no hard links) | `to` / `from` | yes, in the check-then-rename window, which is accepted under D25/D32 (vfat/exfat only) |
| `sys_remove` (the only `remove_file`) | private temp only | no |

[minor] m1 — crates/vr-fs/src/ops.rs:414-417, 519 — creating the temp name can replace an existing name
Scenario: `temp_sibling` builds `.{name}.vr-tmp-<pid>-<seq>`, and `seq` restarts at 0 in every process. Suppose an earlier vr-ide process was killed between step 1 and step 2. Its leftover temp file is then the only copy of that file. A later process that gets the same pid (pids are reused) and moves a new file with the same name as its first fallback move will run step 1, a plain `rename(from, tmp)`, which silently replaces that leftover. This needs a fallback filesystem, a crash inside a microsecond window, pid reuse, the same seq and the same file name, so it is practically unreachable.
- The case-only path (`rename_via_temp`) does refuse an existing name, because it goes through `NOREPLACE`.
- A user dotfile spelled exactly like a temp name is the same case.
Fix: in `temp_sibling`, add a time or random component (for example `SystemTime::now()` nanos), and/or loop while `exists(tmp)`. That check is against a private name, so the leftover window is irrelevant. Optional test: pre-create the first temp name and assert it survives a `link_move`.

[minor] m2 — leftover hidden `.vr-tmp` files after a crash
Scenario: the process is killed between step 1 and step 2 of `link_move`, or between the two renames of a case-only rename. The file then lives only under `.a.txt.vr-tmp-…`, which the tree hides by default. No data is lost:
- go-to-file indexes dotfiles (D16), so the file can still be found;
- every non-crash failure returns `LeftAt` or `BothNamesLeft` naming the temp path.
After a crash, though, nothing tells the user. This is acceptable for T7.
Suggestion for T8 (or T10), not T7: at project open, scan only the root level and the explorer's expanded folders for `.*.vr-tmp-*`, and show a notice with the paths.

D32 path (no RENAME_NOREPLACE and no hard links): `link_move` falls back to check-then-`rename(tmp, to)`, and `restore` to check-then-`rename(tmp, from)`. Both windows are the documented D25/D32 exception, and the code comment states it.

Note: `links_unsupported` also treats EPERM/EACCES from `link(2)` as "no hard links". So on a filesystem without NOREPLACE, a `protected_hardlinks` refusal also takes the D32 window. This is safe enough and is not filed.

### Rule check T7 R3
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | ops.rs only |
| BASE-2 | pass | no public signature change |
| BASE-4 | pass | red/green lines in notes; reproduced 42 passed |
| BASE-6 | pass | why-comments on `link_move`/`restore`, accurate |
| BASE-32 | pass | D25 no-loss holds (only the private temp name is unlinked); D24, D32, D9 honoured |
| AC-8 | pass (vr-fs part) | 42 green, including the save-during-move test |
| D9 | pass | trash path unchanged, no permanent delete |

## Quick pass 2 re-check

Verdict: **APPROVE T7 QP2**

Read: `git diff HEAD -- crates/vr-fs/src/ops.rs` (base 0e85a06): `temp_sibling`, `LinkOps.temp_candidate`, `free_temp_name`, `link_move`, the new test and the seam additions to the existing tests. Notes `## Quick pass 2`. Reviewer run: `cargo test -p vr-fs` → 43 passed, 1 ignored.

- m1 is fixed. `free_temp_name` skips any candidate that already exists (`symlink_metadata`, so a dangling link counts too) before the plain step-1 rename. The name now carries pid + nanos + seq, so a reused pid does not start on an earlier crash's names. After 64 occupied candidates it returns an `Io` error naming `from`, before anything has moved.
- What remains is a check-then-rename window on a private, freshly made name. Only another writer of that exact `.…vr-tmp-<pid>-<nanos>-<seq>` name could hit it. Not reachable; not filed.
- `link_move_never_replaces_an_existing_temp_name` pre-creates the first candidate. Its red run on the old code shows the leftover replaced, and it now asserts that the leftover keeps "only copy", the move completes, and no extra names are left. A broken implementation that reuses the first candidate fails it.
- The case-only path still goes through the no-replace `move_with`, unchanged.
- m2 (leftovers hidden after a crash) stays a T8/T10 suggestion, as noted in R3.

### Rule check T7 QP2
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | ops.rs only |
| BASE-2 | pass | private seam field only; no public signature change |
| BASE-4 | pass | red/green lines in notes; reproduced 43 passed |
| BASE-6 | pass | accurate why-comments on `free_temp_name` / `temp_sibling` |
| BASE-32 | pass | D25 no-loss: step 1 never lands on an existing name; D24/D32/D9 unchanged |
| AC-8 | pass (vr-fs part) | 43 green |
