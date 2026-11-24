# T7 vr-fs tree, file ops (trash), fuzzy file index
Skills called: ai-dev-team:implementation
Tools available: Bash, Read, Edit, Write, Skill, ToolSearch, SubagentHandback (Agent/Artifact present but not used: BASE-18, user forbids artifacts)
- Progress: read brief-common, plan T7 + vr-fs contract, spec D9/AC-8/AC-9/edge cases, T1 notes, trash 5.4.0 / nucleo-matcher 0.3.1 / ignore 0.4.35 sources (done)
- Progress: tree.rs + ops.rs written with tests (implementation alongside tests; first run green, see deviations); file_index tests red: 4 FAILED at todo!() (file_index.rs:92/98)
- Progress: file_index implemented; `cargo test -p vr-fs` green 19 passed; global excludes rooted at project; crate clippy clean; workspace guard blocked by vr-core/vr-config (not mine)

- Files changed: crates/vr-fs/src/lib.rs (mod + re-exports, after "T1 committed"), crates/vr-fs/src/tree.rs, crates/vr-fs/src/ops.rs, crates/vr-fs/src/file_index.rs (all new)
- Tests added (19 + 1 ignored child): tree_dirs_first_sorted, tree_hides_gitignored_by_default (AC-9: nested .gitignore, negation, .git/info/exclude, children of ignored dir flagged), tree_toggle_ignored_shows_them, tree_hides_dotfiles_unless_show_hidden (.git always hidden), tree_lazy_children (Review Focus 4: chmod 000 subdir, listing root never opens it), tree_children_outside_root_errors, tree_symlink_loop_safe (link to ancestor, to own dir, self-cycle, broken, file, safe dir link); create_file_and_dir, create_existing_errors, rename_file_and_dir, copy_dir_recursive, move_into_dir, move_into_self_refused, fake_trash_records_and_removes, trash_moves_to_trash (+ ignored trash_child); file_index_respects_gitignore, file_index_query_ranks_basename_first ("ordctl" → src/Controller/OrderController.php first), file_index_query_path_and_case, file_index_stops_when_receiver_dropped
- Real-trash test: runs the real `trash` backend in a child copy of the test binary (`--exact ops::tests::trash_child --ignored`) with XDG_DATA_HOME=<tempdir>/data and the project in the same tempdir (same mount → home trash, not a per-volume /tmp/.Trash-uid). No `set_var` in this process, so no env race with parallel tests. The child test is a no-op unless VR_FS_TRASH_TEST_PROJECT is set and asserts the trash dir sits beside the temp project.

## Contract (additive helpers, BASE-2)
Plan signatures kept: `Tree{pub root}`, `Node{path,kind,ignored,depth}`, `Tree::new(root, show_hidden, show_ignored)`, `children(&self,&Path)->io::Result<Vec<Node>>`, `ops::{create_file,create_dir,rename,copy,move_to,trash}` → `Result<_, FsOpError>`, `FileIndex::build(root, Sender<IndexMsg>) -> JoinHandle<()>`, `query(&self,&str,usize)->Vec<(PathBuf,u32,Vec<u32>)>`.
Defined by me (plan left open): `NodeKind{File,Dir,Symlink(SymlinkTarget)}`, `SymlinkTarget{File,Dir,Loop,Broken}`, `IndexMsg{Progress(usize),Ready(FileIndex),Error(String)}`, `move_to` returns the new `PathBuf`, `FsOpError{AlreadyExists,NotFound,IntoItself,NoFileName,Io,Trash}`.
Additive: `Tree::{set_show_hidden,set_show_ignored,show_hidden,show_ignored}`, `Node::name`, `NodeKind::is_expandable`, `Trash` trait + `SystemTrash` + `FakeTrash` (for T8's `explorer_delete_confirms_then_trashes`), `FileIndex::{from_paths,root,files,len,is_empty,truncated}`, `MAX_FILES`.
Query paths are root-relative; indices are char positions in that relative path string (T8 joins with root to open).

## Deviations
- BASE-3 partial: tree.rs and ops.rs tests were written in the same pass as their implementation, so their first run was already green (not seen red); file_index tests were seen red (4 FAILED at `todo!()`) before the implementation.
- Extra tests beyond the plan list: tree_toggle_ignored_shows_them, tree_children_outside_root_errors, fake_trash_records_and_removes, file_index_query_path_and_case, file_index_stops_when_receiver_dropped.

## Out-of-scope noticed
- Workspace guard fails in crates being built in parallel: `cargo fmt --all --check` diffs in crates/vr-config/src/{keymap,keys,paths}.rs, crates/vr-config/tests/config.rs; `cargo clippy --workspace` fails compiling vr-core (E0583 module files not found: text, motion, history, edit_ops) — not mine (T2/T3 in progress).

## Commands run (final)
```
cargo test -p vr-fs (red, file_index stubs)
test result: FAILED. 15 passed; 4 failed; 1 ignored  (file_index.rs:92/98 not yet implemented)
cargo test -p vr-fs (green)
test ops::tests::trash_moves_to_trash ... ok
test file_index::tests::file_index_query_ranks_basename_first ... ok
test tree::tests::tree_lazy_children ... ok
test tree::tests::tree_symlink_loop_safe ... ok
test result: ok. 19 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.02s
cargo clippy -p vr-fs --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.38s   (exit 0)
rustfmt --edition 2024 --check crates/vr-fs/src/*.rs   -> exit 0
cargo fmt --all --check            -> exit 1, diffs only in crates/vr-config/** (not mine)
cargo clippy --workspace --all-targets -- -D warnings -> exit 101, vr-core does not compile (not mine)
```

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| gitignore matching | capabilities.md (vr-fs empty), ignore 0.4.35 | `ignore::gitignore::Gitignore` stack for the tree, `ignore::WalkBuilder` for the index |
| fuzzy matching | plan Decisions | nucleo-matcher `Matcher::fuzzy_indices` with `set_match_paths` |
| trash | plan Tech Stack | `trash::delete_all`; capabilities.md updated with tree/ops/file_index lines |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | only crates/vr-fs/src/** plus team files (capabilities, decisions, notes) |
| BASE-2 | pass | Contract section above; plan signatures unchanged, additions listed |
| BASE-3 | fail→noted | file_index seen red; tree/ops tests written with code (Deviations) |
| BASE-4 | pass | Commands run (final) |
| BASE-5 | pass | no heavy build needed (crate tests/clippy only); child test process exits with the test; nothing running |
| BASE-6 | pass | English, why-comments only |
| BASE-8 | pass | crates/vr-fs/Cargo.toml untouched; deps ignore/trash/nucleo-matcher/thiserror/tempfile from T1 |
| BASE-10 | pass | `cargo test -p vr-fs` only |
| BASE-11 | pass | no stash/checkout/reset |
| BASE-14 | pass | all commands finished in foreground in seconds |
| BASE-15 | pass | no background tasks started |
| BASE-16 | pass | Reuse table; capabilities.md ## vr-fs lines added |
| BASE-17 | pass | no installs |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | progress lines above |
| BASE-24 | pass | no git add/commit |
| BASE-25 | pass (crate) | crate clippy exit 0 + vr-fs rustfmt check exit 0; workspace guard red only in vr-core/vr-config (not mine) |
| BASE-32 | pass | D9 trash via `trash` crate, tests never touch the real user trash; decisions.md entries added |
| BASE-38 | pass | `rustfmt --edition 2024 crates/vr-fs/src/*.rs` only |
| COST | pass | related tests only, no servers/watchers/browsers, no polling |
| AC-8 | pass | create_file_and_dir, rename_file_and_dir, copy_dir_recursive, move_into_dir, trash_moves_to_trash (on disk + Trash/files + .trashinfo) |
| AC-9 | pass | tree_hides_gitignored_by_default, tree_toggle_ignored_shows_them, file_index_respects_gitignore |
| Review Focus 4 | pass | tree_lazy_children; index on background thread with cap + cancel (file_index_stops_when_receiver_dropped) |
n-a: BASE-7, BASE-9, BASE-12, BASE-13, BASE-20…BASE-23, BASE-26…BASE-31, BASE-33…BASE-37 (not in this brief's developer list / no UI, no removals, no DB)

## Fix round 1
- Progress: read 05_review_T7.md, D24, D25 (done)
- Progress: rustix 1.1.6 added (D25); pass 1 = new tests against old behaviour + todo!() stubs for new fns. Red run `cargo test -p vr-fs ops::` → 13 passed; 11 failed:
  - ensure_within_refuses_outside / validate_name_rejects_bad_names: `not yet implemented`
  - destination_errors_name_the_destination: `assertion failed: matches!(err, FsOpError::NotFound(p) if p == to)`
  - copy_refuses_special_files: `Io { .. s.sock, Os { code: 6, "No such device or address" } }` (old copy opened the socket)
  - copy_partial_failure_reports_target: `expected PartialCopy, got Err(Io { .. secret.txt, PermissionDenied })`
  - move_cross_device_refused: `unwrap_err() on an Ok value` (old fallback copied + deleted)
  - move_fallback_without_renameat2_never_overwrites: `assertion failed: matches!(err, FsOpError::AlreadyExists(p) if p == b.txt)`
  - rename_case_only: `unwrap() on Err AlreadyExists(A.txt)` (same-name rename refused)
  - trash_refuses_cross_device_symlink: `unwrap_err() on an Ok value`
  - trash_batch_reports_per_item: `left: [("", false)]` (whole batch failed as one)
  - trash_unavailable_keeps_file: child `assertion left == right failed left: ""` (no per-path result)
  - green at once (pin behaviour the old code already had): copy_dir_onto_existing_refused, rename_existing_target_refused_dirs_too (pre-existing target, old check-then-act), rename_case_only_same_inode_goes_through_temp, rename_and_move_symlink_move_the_link, trash_symlink_keeps_outside_target
- Progress: pass 2 ops implementation → `cargo test -p vr-fs ops::` 24 passed
- Progress: tree/file_index red run → `tree_lazy_children`: `assertion left == right failed: only the requested dir is listed left: 0` (no counted listing yet); `tree_children_refuses_parent_components`: `unwrap_err() on an Ok value: [Node { path: ".../proj/sub/../../proj", .. }]`; `file_index_stops_when_receiver_dropped`: `walk went on for 2001 entries after the receiver was dropped`
- Progress: fixes applied; full crate green, clippy clean

### rustix (D25)
`rustix = { version = "1.1.6", features = ["fs"] }` in root `[workspace.dependencies]`; vr-fs: `[target.'cfg(any(target_os = "linux", target_os = "android"))'.dependencies] rustix.workspace = true`. `cargo info rustix` → version 1.1.6 (newest); crates.io versions API `created_at` 2026-10-27 → 28 days old (≥ 14). Already in Cargo.lock as a transitive dep; Cargo.lock diff is one line (`+ "rustix",` in vr-fs deps).

### Findings
| Finding | Reply | Where |
|---|---|---|
| F1 EXDEV copy+delete | fixed: `FsOpError::CrossDevice{from,to}`, no copy, no remove (D24) | ops.rs `move_with`, `move_err`; test move_cross_device_refused (seam `sys: &RenameFn` injects `Errno::XDEV`) |
| F2 check-then-act | fixed: `renameat2(RENAME_NOREPLACE)` for rename/move; EINVAL/ENOSYS → `hard_link`+`remove_file` for files/symlinks, checked rename for dirs with the window documented (rename(2) can only replace an empty dir); EPERM/unsupported hard link (vfat) → checked rename; copy via `File::create_new`+`io::copy`, `create_dir`, `symlink` (all O_EXCL-like); create_dir/create_file no longer pre-check; doc comment on each op states its overwrite guarantee | ops.rs; tests rename_existing_target_refused_dirs_too, move_fallback_without_renameat2_never_overwrites, copy_dir_onto_existing_refused |
| F3 symlink trash across devices | fixed: `check_symlink_trash` refuses a symlink whose parent `st_dev` differs from the home trash dir's (nearest existing ancestor) → `TrashRefused`, nothing done | ops.rs `trash_one`, `home_trash_dir`; test trash_refuses_cross_device_symlink (/proc as the other device) |
| F4 data-safety tests | fixed: trash_unavailable_keeps_file (XDG_DATA_HOME is a regular file, works as root too), trash_symlink_keeps_outside_target, rename_and_move_symlink_move_the_link, move_cross_device_refused, copy_partial_failure_reports_target (+ copy_refuses_special_files: socket), rename_case_only, copy_dir_onto_existing_refused, rename_existing_target_refused_dirs_too (pre-existing target with renameat2) | ops.rs tests |
| case-only rename | fixed: same dev+ino and names equal ignoring case → rename via `.name.vr-tmp-<pid>-<n>` in the same dir, moved back if step 2 fails; same path → no-op | `is_case_only_same_file`, `rename_via_temp`; tests rename_case_only, rename_case_only_same_inode_goes_through_temp (hard link stands in for a case-insensitive fs: refused, original restored, no temp left) |
| destination errors | fixed: `move_err` blames `to` while `from` still exists; copy's create errors name `to` | test destination_errors_name_the_destination |
| batch trash | fixed — **contract change**: `ops::trash(paths) -> BatchResult` = `Vec<(PathBuf, Result<(), FsOpError>)>`; `Trash::trash` same; each path trashed separately, failures don't stop the rest | test trash_batch_reports_per_item |
| index cancellation | fixed: `Progress` send (= receiver check) every 256 walk entries of any kind; `walk` returns the visited count | test asserts ≤ 256 visited with 2 000 files (was 2001 → red) |
| tree_lazy_children | fixed: thread-local count of listings in `children` (single seam `read_dir_counted`) must be 1; chmod part skipped when permission bits do not apply (root), asserts `PermissionDenied` | tree.rs |
| validate_name / ensure_within | added (re-exported from lib.rs) for T8 | tests validate_name_rejects_bad_names, ensure_within_refuses_outside |
| reviewer minor: `..` in `children` | fixed: `children` refuses any `..` component (InvalidInput) | test tree_children_refuses_parent_components |

New error variants: `CrossDevice{from,to}`, `PartialCopy{to, source: Box<FsOpError>}`, `InvalidName`, `OutsideRoot`, `TrashRefused{path,reason}`. Copy now refuses sockets/FIFOs/devices (Unsupported) instead of opening them.

### Commands (round 1, final)
```
cargo test -p vr-fs
test result: ok. 36 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.05s
cargo clippy -p vr-fs --all-targets -- -D warnings
    Checking vr-fs v0.1.0 ... Finished `dev` profile   (exit 0)
rustfmt --edition 2024 --check crates/vr-fs/src/*.rs  -> exit 0
```

### Rule compliance (round 1)
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | crates/vr-fs/**, root Cargo.toml (one rustix line), Cargo.lock (+1 line), team files |
| BASE-2 | pass | plan signatures kept except the leader-requested `trash` → `BatchResult` (listed above); additions listed |
| BASE-3 | pass | every new test run red first (red lines above); 5 tests green at once are pins of behaviour the old code already had, listed |
| BASE-4 | pass | commands above |
| BASE-5 | pass | crate-only commands; trash child processes exit with their tests; nothing running |
| BASE-6 | pass | English, why-comments |
| BASE-8 | pass | only rustix, approved by D25 |
| BASE-10 | pass | `cargo test -p vr-fs` only |
| BASE-11, BASE-24 | pass | no stash/checkout/reset, no git index changes |
| BASE-14, BASE-15 | pass | foreground commands of seconds; no background tasks |
| BASE-16 | pass | capabilities.md vr-fs ops line updated |
| BASE-17, BASE-18 | pass | no installs, no sub-agents |
| BASE-19 | pass | progress lines |
| BASE-25 | pass (crate) | clippy -p vr-fs exit 0, rustfmt check exit 0 (workspace guard not run: T2 building in vr-core) |
| BASE-32 | pass | D9 (trash only, unavailable trash → error, file kept), D24 (CrossDevice), D25 (rustix NOREPLACE) |
| BASE-38 | pass | rustfmt on crates/vr-fs/src/*.rs only |
| COST | pass | related tests only, no servers/browsers |
| AC-8 | pass | ops tests incl. race/EXDEV/partial-copy paths |
| AC-9 | pass | unchanged tests green |
| Review Focus 4 | pass | tree_lazy_children (listing count = 1), index cancels within 256 entries |
n-a: BASE-7, BASE-9, BASE-12, BASE-13, BASE-20…BASE-23, BASE-26…BASE-31, BASE-33…BASE-37

## Quick pass 1
Base: T7 committed as 167c2fa; only crates/vr-fs/src/ops.rs and the notes changed (no Cargo.toml/Cargo.lock edits, no git commands).
- Progress: read `## Round 1 re-check`; added the `finish_link_move` seam (`remove: &RemoveFn`) with the old behaviour, `LeftAt`/`BothNamesLeft` variants and 3 tests + message assertion → red run `cargo test -p vr-fs ops::` 23 passed; 4 failed:
  - link_move_undone_when_old_name_cannot_be_removed: `the extra link is undone`
  - link_move_names_both_paths_when_undo_fails: `expected BothNamesLeft, got Io { path: ".../a.txt", source: Kind(PermissionDenied) }`
  - case_rename_reports_temp_name_when_restore_fails: `expected LeftAt, got Io { path: ".../A.txt", source: Kind(PermissionDenied) }`
  - trash_refuses_cross_device_symlink: `.../link was not trashed: symlink on another filesystem than the trash` (message lacked the reason)
- Progress: fixes applied → green.

| Item | Reply | Where |
|---|---|---|
| (1) link ok, unlink of `from` fails | fixed: unlink `to` again only while `same_file(from, to)` holds, then return the original error on `from`; if that undo fails → `FsOpError::BothNamesLeft { from, to, source }` | `finish_link_move` (called from `move_with` fallback); tests link_move_undone_when_old_name_cannot_be_removed, link_move_names_both_paths_when_undo_fails |
| (2) case-only restore fails | fixed: `FsOpError::LeftAt { path: <temp>, intended: to, source }` | `rename_via_temp`; test case_rename_reports_temp_name_when_restore_fails (seam: step 1 real, steps 2–3 EACCES) |
| (3) refusal message | fixed, refusal kept: "the symlink is on another device than the trash, which the trash backend cannot move safely; the link and its target are untouched" | `check_symlink_trash`; trash_refuses_cross_device_symlink asserts the text |
| (4) ensure_within doc | done: callers act on the returned path only (lexical check vs. kernel resolution through links); the root itself is accepted, so rename/move/trash must never be offered on the root | doc comment on `ensure_within` |

For the T8 brief (from the re-check, not changed here): nested selections in a batch trash (`dir` + `dir/a.txt`) give the child a `NotFound` result. T8 should drop descendants of selected dirs before calling `trash`.

```
cargo test -p vr-fs
test result: ok. 39 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.04s
cargo clippy -p vr-fs --all-targets -- -D warnings -> Finished (exit 0)
rustfmt --edition 2024 --check crates/vr-fs/src/*.rs -> exit 0
```

### Rule compliance (quick pass 1)
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | only crates/vr-fs/src/ops.rs + notes |
| BASE-2 | pass | public signatures unchanged; two new error variants (additive) |
| BASE-3 | pass | 4 red lines above, before the fixes |
| BASE-4 | pass | output above |
| BASE-5, BASE-14, BASE-15 | pass | foreground crate commands; nothing running |
| BASE-6 | pass | English why-comments |
| BASE-8 | pass | no manifest change |
| BASE-10 | pass | `cargo test -p vr-fs` only |
| BASE-11, BASE-24 | pass | no git commands |
| BASE-16, BASE-17, BASE-18, BASE-19 | pass | nothing new to index; no installs; no sub-agents; progress lines |
| BASE-25 | pass (crate) | clippy -p vr-fs + rustfmt check exit 0 |
| BASE-32 | pass | D9/D24/D25 behaviour unchanged |
| BASE-38 | pass | rustfmt on ops.rs only |
| COST | pass | related tests only |
| AC-8 | pass | 39 tests green |
n-a: BASE-7, BASE-9, BASE-12, BASE-13, BASE-20…BASE-23, BASE-26…BASE-31, BASE-33…BASE-37, AC-9 (untouched)

## Fix round 2
- Progress: read the leader's round-2 finding (atomic save between `same_file` and the unlink of `to` could be deleted for good) and fix (a).
- Red: replaced `link_move_undone_when_old_name_cannot_be_removed` and `link_move_names_both_paths_when_undo_fails` with `link_move_keeps_both_names_when_old_name_cannot_be_removed`. Its seam denies only the removal of `from`, so any unlink of `to` would show. `cargo test -p vr-fs link_move` → `expected BothNamesLeft, got Io { path: ".../a.txt", source: Kind(PermissionDenied) }` (1 failed).
- Fix: `finish_link_move` never unlinks anything after a failed removal of `from`; it returns `BothNamesLeft { from, to, source }`. The comment now says why `to` is never unlinked. Message: "the file now has both names, {from} and {to}; nothing was deleted (removing {from} failed: {source})". The variant doc was updated to match.

| Item | Reply | Where |
|---|---|---|
| undo removed | fixed | ops.rs `finish_link_move` |
| accurate message | fixed | `FsOpError::BothNamesLeft` `#[error]` |
| test | rewritten: asserts `BothNamesLeft` with both paths, the message text, and both names still present with content "a"; the "undo fails" test was deleted (merged) | `link_move_keeps_both_names_when_old_name_cannot_be_removed` |
| comment at 495-496 | replaced | ops.rs `finish_link_move` |

```
cargo test -p vr-fs
test result: ok. 38 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.04s
cargo clippy -p vr-fs --all-targets -- -D warnings -> Finished (exit 0)
rustfmt --edition 2024 --check crates/vr-fs/src/ops.rs -> exit 0
```
(38 = 39 − 2 merged tests + 1 new.)

### Rule compliance (round 2)
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | only crates/vr-fs/src/ops.rs + notes |
| BASE-2 | pass | no signature change; error variant text only |
| BASE-3 | pass | red line above before the fix |
| BASE-4 | pass | output above |
| BASE-5, BASE-14, BASE-15 | pass | foreground crate commands; nothing running |
| BASE-6 | pass | why-comment, no history |
| BASE-8 | pass | no manifest change |
| BASE-10 | pass | `cargo test -p vr-fs` |
| BASE-11, BASE-24 | pass | no git commands |
| BASE-16, BASE-17, BASE-18, BASE-19 | pass | nothing new to index; no installs; no sub-agents; progress lines |
| BASE-25 | pass (crate) | clippy + rustfmt check exit 0 |
| BASE-32 | pass | D9/D24/D25 unchanged; no permanent delete anywhere in ops |
| BASE-38 | pass | rustfmt on ops.rs only |
| COST | pass | related tests only |
| AC-8 | pass | 38 green |
n-a: BASE-7, BASE-9, BASE-12, BASE-13, BASE-20…BASE-23, BASE-26…BASE-31, BASE-33…BASE-37, AC-9 (untouched)
- Round 2 clarification (behaviour over steps): checked again with no code change. Option (a) already gives the required behaviour. After a failed move no path removes or overwrites a name: `finish_link_move` returns `BothNamesLeft` (both names, nothing deleted); `rename_via_temp` returns `LeftAt { path: <temp> }` when the restore fails; restores use the no-replace move. The test failed before the fix (red line above) and passes after it. One window is left, documented since D25 and outside this round: on filesystems with neither RENAME_NOREPLACE nor hard links (vfat/exfat), a file move falls back to the checked rename in `move_with`.

## Fix round 3
- Progress: read `## Round 2 re-check` (R2-1). The permission-label item was dropped by the leader's update: the reviewer could not reproduce it, and I found no path that mislabels EACCES (it still maps to `Io(PermissionDenied)` on the target), so no test was written for it.
- Pass A: added the seam `LinkOps { rename, link, remove }` (`LinkOps::REAL` = fs::rename / fs::hard_link / fs::remove_file) and `link_move`, still with the old behaviour (link `from`→`to`, then unlink `from`), plus 5 tests. Red run `cargo test -p vr-fs link_move` → 2 passed; 4 failed:
  - link_move_only_removes_its_own_temp_name: `removed a shared name: "/tmp/.tmpKNUKuw/a.txt"`
  - link_move_reports_both_names_when_temp_cannot_be_removed: panicked at `is_own_temp` with `"/tmp/.tmpipx1aU/a.txt"` (the unlink targeted `from`)
  - link_move_reports_temp_when_source_name_was_taken: `expected LeftAt, got AlreadyExists(".../b.txt")`
  - link_move_keeps_a_save_made_during_the_move: the test's saving seam panicked with `NotFound` (old code linked and unlinked `from` itself, so the save step never matched)
  - green at once: link_move_existing_target_restores_source (the old code also refused a target that already existed)
- Pass B (fix): `link_move` takes the source first:
  1. `rename(from, temp)`. The temp is a private, unique `.name.vr-tmp-<pid>-<n>` name. This takes whatever is at `from` at that instant, so a save landing on `from` later stays there.
  2. `link(temp, to)`, which never replaces.
  3. `remove(temp)`, the only name that is ever unlinked.

  On failure, `restore` links the temp back to `from` (never replacing) and removes the temp. If `from` was taken meanwhile → `LeftAt { path: temp }`. If the temp cannot be removed → `BothNamesLeft` naming the temp and the other name. On filesystems without hard links the temp → `to` step is the checked rename (the D25 window; documented in the code). `finish_link_move` and the `RemoveFn` alias were removed. Destination-side errors use `dest_err(from, to, e)`, because `from` no longer exists after the source is taken.
- Unlink audit of ops.rs: the only `remove_file` is `sys_remove`, and it is reached only through `ops.remove` on the private temp name (in `link_move` and `restore`). `rename` overwrites nothing: temp names are unique, and the temp → `to` and temp → `from` renames are checked renames that run only on filesystems without hard links (D25 window).

| Item | Reply | Where |
|---|---|---|
| R2-1 source unlinked by name | fixed (the reviewer's grab-first design) | ops.rs `link_move`, `restore`, `links_unsupported`, `dest_err`, `temp_sibling` |
| required seam test (remove never gets `from`/`to`) | added, red then green | `link_move_only_removes_its_own_temp_name` |
| extra cases | added | `link_move_keeps_a_save_made_during_the_move` (a save on `from` mid-move survives; `to` has the old content), `link_move_existing_target_restores_source`, `link_move_reports_both_names_when_temp_cannot_be_removed`, `link_move_reports_temp_when_source_name_was_taken` |
| QC permission label | dropped per leader update (not reproducible; no mislabelling path found) | — |

```
cargo test -p vr-fs
test result: ok. 42 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.05s
cargo clippy -p vr-fs --all-targets -- -D warnings -> Finished (exit 0)
rustfmt --edition 2024 --check crates/vr-fs/src/ops.rs -> exit 0
```
(42 = 38 − 1 replaced test + 5 new.)

### Rule compliance (round 3)
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | only crates/vr-fs/src/ops.rs + notes |
| BASE-2 | pass | no public signature change |
| BASE-3 | pass | red lines above, before the fix |
| BASE-4 | pass | output above |
| BASE-5, BASE-14, BASE-15 | pass | foreground crate commands; nothing running |
| BASE-6 | pass | why-comments on link_move/restore |
| BASE-8 | pass | no manifest change |
| BASE-10 | pass | `cargo test -p vr-fs` |
| BASE-11, BASE-24 | pass | no git commands |
| BASE-16, BASE-17, BASE-18, BASE-19 | pass | private helpers only; no installs; no sub-agents; progress lines |
| BASE-25 | pass (crate) | clippy + rustfmt check exit 0 |
| BASE-32 | pass | D25 no-loss: no shared name is unlinked; D9/D24 unchanged |
| BASE-38 | pass | rustfmt on ops.rs only |
| COST | pass | related tests only |
| AC-8 | pass | 42 green |
n-a: BASE-7, BASE-9, BASE-12, BASE-13, BASE-20…BASE-23, BASE-26…BASE-31, BASE-33…BASE-37, AC-9 (untouched)
