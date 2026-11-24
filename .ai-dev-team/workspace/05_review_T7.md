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
