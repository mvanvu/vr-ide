# T4 vr-core documents (load/save/encoding) + file watching
Skills called: ai-dev-team:implementation
Tools available: Bash, Read, Edit, Write, Skill, ToolSearch (deferred: LSP, Monitor, TaskStop, SendMessage, WebFetch/WebSearch), Artifact/Agent (not used: BASE-18 / user rule).
- Progress: read brief-common, brief T4, plan (Contracts vr-core, Task T4, Review Focus 2/3, Decisions), spec AC-3/AC-7/AC-18/edge cases, decisions D24–D31, T2 notes, capabilities, text.rs/history.rs API, notify-debouncer-full 0.8 API.
- Progress: leader message "T7 committed (167c2fa)" received — D31 manifest change allowed (ropey entry only).
- Progress: D31 red — added `text::tests::only_lf_crlf_cr_break_lines`; red: `left: 9 right: 4` (ropey unicode_lines counted FF/VT/NEL/U+2028/U+2029).
- Progress: D31 green — root Cargo.toml ropey entry → `{ version = "=1.6.1", default-features = false, features = ["simd", "cr_lines"] }` (Cargo.lock unchanged: features are not locked); `is_line_break` = `\n`/`\r`; cargo test -p vr-core 31 passed.
- Progress: document red — 21 tests written against `todo!()` stubs (crates/vr-core/src/document.rs); `cargo test -p vr-core document::` → `test result: FAILED. 0 passed; 21 failed; 1 ignored` (each: panicked at document.rs `not yet implemented`): open_utf8_lf, open_detects_line_ending_and_highlight_limit, open_non_utf8_read_only, open_utf16_bom_read_only, open_binary_refused, open_missing_and_unreadable, save_keeps_crlf, save_keeps_missing_trailing_newline, save_keeps_bom, save_atomic_keeps_mode, save_permission_denied, save_failure_leaves_no_temp_file, save_through_symlink_keeps_link, save_refuses_when_changed_on_disk, save_ignores_touch_without_content_change, save_recreates_file_deleted_on_disk, save_untitled_and_save_as, is_modified_tracks_saved_version, reload_replaces_text_keeps_cursor_clamped, check_disk_states, save_temp_names_are_recognised (+ open_100mb_under_2s ignored).
- Progress: red — added save_refuses_read_only_file (0o444 file in writable dir) → FAILED (`not yet implemented`).
- Progress: document green — `test result: ok. 22 passed; 0 failed; 1 ignored`. Positive case added to save_temp_names_are_recognised afterwards (characterization, name format fixed by the implementation).
- Progress: watch red — 8 tests written against `todo!()` stubs (crates/vr-core/src/watch.rs); `cargo test -p vr-core watch::` → `test result: FAILED. 0 passed; 8 failed` (each `not yet implemented`): watch_modified_event, watch_removed_event, watch_debounces_burst, watch_atomic_replace_by_other_tool_is_modified, watch_ignores_own_save, watch_dir_reports_create_rename_remove, watch_filters_siblings_and_unwatch_stops, watch_symlinked_file_reports_link_path.
- Progress: watch green — `test result: ok. 8 passed; 0 failed ... finished in 2.10s`. Mutation check: disabling the own-write suppression turns watch_ignores_own_save red (`left: [Modified("/tmp/.tmpcTZZAD/a.txt")] right: []`), restored → green. Stability: full `cargo test -p vr-core` 3× → 61 passed each time.
- Progress: AC-18 timing (ignored test) once in release via run-heavy → ok, 0.25 s for the whole test (writes + opens 100 MB).
- Progress: rustfmt on own files; clippy fix (`chunks_exact(2)` → `as_chunks::<2>()`); workspace guard green; decisions.md (7 `D? (T4)` lines), capabilities.md (document.rs, watch.rs) updated. Scratch splice files in qa-artifacts/T4 removed.

## Summary
- Files changed: `Cargo.toml` (root, ropey entry only — D31), `crates/vr-core/src/text.rs` (`is_line_break` = `\n`/`\r`, test `only_lf_crlf_cr_break_lines`), `crates/vr-core/src/lib.rs` (modules `document`, `watch` + re-exports), new `crates/vr-core/src/document.rs`, new `crates/vr-core/src/watch.rs`. Cargo.lock unchanged (features are not locked).
- Tests added (31 + 1 ignored): text::only_lf_crlf_cr_break_lines; document::{open_utf8_lf, open_detects_line_ending_and_highlight_limit, open_non_utf8_read_only, open_utf16_bom_read_only, open_binary_refused, open_missing_and_unreadable, save_keeps_crlf, save_keeps_missing_trailing_newline, save_keeps_bom, save_atomic_keeps_mode, save_permission_denied, save_refuses_read_only_file, save_failure_leaves_no_temp_file, save_through_symlink_keeps_link, save_refuses_when_changed_on_disk, save_ignores_touch_without_content_change, save_recreates_file_deleted_on_disk, save_untitled_and_save_as, is_modified_tracks_saved_version, reload_replaces_text_keeps_cursor_clamped, check_disk_states, save_temp_names_are_recognised, open_100mb_under_2s (#[ignore])}; watch::{the 8 above}.
- Edit methods: Write for new files and the notes, Edit for text.rs/document.rs, `perl -pi`/`perl -0pi` for Cargo.toml, lib.rs, capabilities.md and the mutation check (one perl multi-line replacement on text.rs left a stray tail, fixed with one Edit).

### Public API added (for T5/T6/T8)
vr_core::document (re-exported at crate root):
- `pub const BINARY_SNIFF_LEN: usize` (8 KiB)
- `pub enum LineEnding { Lf, CrLf, Cr }` + `fn as_str(self) -> &'static str`, `fn detect(&str) -> Self`
- `pub enum Encoding { Utf8, Utf8Bom, Other(String) }` (`Other("UTF-16LE"|"UTF-16BE"|"non-UTF-8")`)
- `pub enum DiskState { InSync, ChangedOnDisk, DeletedOnDisk }`
- `pub enum DocError { Io(io::Error), Binary, PermissionDenied(PathBuf), NotUtf8ReadOnly, ChangedOnDisk, NoPath }` (thiserror)
- `pub struct DiskStamp` (Copy, Eq; private fields) + `fn of(&Path) -> io::Result<Option<Self>>`
- `pub struct Document { pub path: Option<PathBuf>, pub buffer: Buffer, pub line_ending, pub encoding, pub read_only: bool, pub disk_state: DiskState, pub highlight: bool, .. }`
- `Document::untitled()`, `open(&Path, max_size_highlight: u64) -> Result<Self, DocError>`, `is_modified()`, `save()`, `save_overwrite()`, `save_as(&Path)`, `reload()`, `check_disk() -> DiskState`, `disk_stamp() -> Option<DiskStamp>`
- `pub fn is_save_temp(&Path) -> bool`
vr_core::watch (re-exported): `pub const DEBOUNCE: Duration` (200 ms), `pub enum FsEvent { Modified, Removed, Created, Renamed(from, to) }`, `pub enum WatchError { Notify(notify::Error), Path(PathBuf, io::Error) }`, `pub struct Watcher` with `new(Sender<FsEvent>) -> Result<Self, WatchError>`, `watch(&mut self, &Path) -> Result<(), WatchError>`, `unwatch(&mut self, &Path)`, `ignore_own_write(&self, &Path, DiskStamp)`.

How T6 uses it (AC-7): on `FsEvent::Modified(p)` for an open doc → `doc.check_disk()`; `ChangedOnDisk` + `!is_modified()` → `reload()`; `ChangedOnDisk` + modified → prompt (Reload → `reload()`, Keep mine → `save_overwrite()` on save); `Removed(p)` → `check_disk()` gives `DeletedOnDisk`, buffer kept. After every successful save → `watcher.ignore_own_write(path, doc.disk_stamp().unwrap())`. `save()` → `Err(ChangedOnDisk)` → same prompt.

### Deviations from the plan (all additive, logged in decisions.md as `D? (T4)`)
- `Watcher::watch` returns `Result<(), WatchError>` (plan: no return value) — an inotify limit must reach the UI.
- `DocError` gains `ChangedOnDisk` (brief) and `NoPath` (save of an untitled doc); `LineEnding` gains `Cr` (D31 counts lone CR); `Document` field is `saved_revision` (D26), plus private `highlight_limit`, `disk`; public `highlight`.
- Line endings are not normalised: the buffer holds the file's bytes as text, so mixed endings stay byte-exact; `line_ending` = first break.

### Out-of-scope / known limits (notes, not fixed)
- Rename-based save breaks hard links and does not copy xattrs/ACLs (std has no API; would need a new dependency, BASE-8).
- A non-writable directory makes saving impossible even when the file itself is writable (no non-atomic fallback, by design); Save As elsewhere works.
- Check-then-rename window in `save()` (a writer between the disk check and the rename is overwritten); no lock API exists on Linux for cooperative tools.
- A symlinked file whose target lives in another directory: deleting the link itself is not reported (only the target's directory is watched).
- vr-search (T9) has its own temp+rename writer for project replace; it could reuse vr-core's save path later (leader's call; different crate, in progress).
- Workspace clippy prints manifest warnings `unused dependency arboard/vr-config/vr-fs/vr-search` in crates/vr-ide/Cargo.toml — not mine, not errors.

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| atomic save / temp+rename | capabilities.md (vr-fs ops: rename/copy no-overwrite), `grep -rn "rename\|create_new" crates/*/src` | vr-fs ops are move/copy without content writing and vr-core must not depend on vr-fs (layering); built `write_atomic` in document.rs |
| line-break / CRLF handling | capabilities.md (text.rs `is_line_break`, `default_line_break`) | reused: buffer keeps breaks, T2's CRLF-aware motions apply |
| modified tracking | text.rs `Buffer::revision()`, `break_undo_group()` (D26) | reused |
| debounced watching | workspace deps notify 9 + notify-debouncer-full 0.8 | reused (no new dependency) |

## Commands run (final, trimmed)
```
$ cargo test -p vr-core
test result: ok. 61 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 2.10s
$ run-heavy.sh cargo test -p vr-core --release open_100mb -- --ignored
test document::tests::open_100mb_under_2s ... ok
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 61 filtered out; finished in 0.25s
$ cargo fmt --all --check        → exit 0 (no output)
$ cargo clippy --workspace --all-targets -- -D warnings
warning: unused dependency `arboard` (crates/vr-ide/Cargo.toml, manifest warning, not mine) ... 
    Finished `dev` profile [unoptimized + debuginfo] target(s)
$ cargo clippy -p vr-core --all-targets -- -D warnings   (after touching lib.rs to force a recheck)
    Checking vr-core v0.1.0 ... Finished `dev` profile
$ rule-checks.sh related-tests T4 → crates/vr-config/tests/config.rs
$ cargo test -p vr-config --test config → test result: ok. 16 passed; 0 failed
```

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | `git status`: only crates/vr-core/src/{document,watch,lib,text}.rs, root Cargo.toml (ropey line), team files |
| BASE-2 | pass | Contract types/signatures kept (`Document::open(path, max_size_highlight)`, `save/save_as/reload/is_modified`, `Watcher::new(tx)`, `FsEvent` variants); additive items listed under Deviations and in decisions.md; one return-type addition (`watch -> Result`) flagged for the leader |
| BASE-3 | pass | red lines above: D31 `left: 9 right: 4`; document 21 FAILED + save_refuses_read_only_file FAILED; watch 8 FAILED — all before the code; mutation check for own-save suppression |
| BASE-4 | pass | "Commands run" section above |
| BASE-5 | pass | release timing build via run-heavy.sh; nothing left running |
| BASE-6 | pass | English; comments say why (e.g. document.rs `write_atomic` doc, `rename() ignores the target's mode`) |
| BASE-8 | pass | no new dependency; ropey feature change is the leader's D31 instruction; Cargo.lock unchanged |
| BASE-10 | pass | `cargo test -p vr-core` only (+ the one related vr-config test file from COST-10) |
| BASE-11 | pass | no stash/checkout/restore/reset/branch switch |
| BASE-14 | pass | all runs finished in the foreground (longest ~20 s) |
| BASE-15 | pass | no background tasks started; Watcher threads end when tests drop them |
| BASE-16 | pass | Reuse table; capabilities.md lines for document.rs and watch.rs |
| BASE-17 | pass | no installs |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | this file, progress line per step |
| BASE-24 | pass | no git add/commit |
| BASE-25 | pass | `cargo fmt --all --check` exit 0; `cargo clippy --workspace --all-targets -- -D warnings` Finished |
| BASE-32 | pass | D26 (`saved_revision` vs `revision()`, break_undo_group on save — is_modified_tracks_saved_version), D31 (only_lf_crlf_cr_break_lines), D24/D25 (save_refuses_when_changed_on_disk, save_untitled_and_save_as), C6 (notify + debouncer) |
| BASE-38 | pass | `rustfmt --edition 2024` on document.rs, watch.rs, lib.rs, text.rs only |
| COST-1 | pass | release build via run-heavy.sh; tests per crate |
| COST-3 | pass | no sleep/poll loops (tests use `recv_timeout`; the burst test's 5 ms sleeps generate the writes under test) |
| COST-5 | pass | no servers/browsers; watchers dropped at test end |
| COST-8 | pass | new files written whole; edits batched; methods listed in Summary |
| COST-9 | pass | nothing left running |
| COST-10 | pass | related-tests → crates/vr-config/tests/config.rs, run: 16 passed |
| COST-13 | pass | scratch splice files only under qa-artifacts/T4 (removed) |
| COST-16 | pass | outputs filtered with grep/tail |
| AC-3 | pass | is_modified_tracks_saved_version; save_atomic_keeps_mode (new inode, no temp left) |
| AC-7 | pass | watch_modified_event (< 1 s), watch_debounces_burst, watch_atomic_replace_by_other_tool_is_modified, watch_ignores_own_save, check_disk_states, reload_replaces_text_keeps_cursor_clamped, save_refuses_when_changed_on_disk (prompt itself is T6) |
| AC-18 | pass | open_100mb_under_2s (#[ignore]) run once in release: ok, 0.25 s |
n-a: COST-2, COST-6, COST-7, COST-11, COST-15, COST-19, COST-20 (no UI, browser, e2e, goldens or qc script in a library task) · COST-4 (no full suite) · COST-12, COST-14, COST-17, COST-18 (leader / CI / goldens / sign-off rules)

## Fix round 1
- Progress: read 05_review_T4.md (3 major, 3 minor, 1 uncertain) and D46; plan: writability probe + setid strip, re-check right before rename + post-rename stamp guard with cfg(test) stage hooks, racy-snapshot rule, rescan/error events, move-in = Created, `Saved { other_hard_links }`, own-write registry filled by `save()`.
- Progress: skeleton (stubs keep old behaviour): `Snapshot.racy`, `Saved`, `OwnWrites` (no-op), `snapshot_matches` (old logic), `restored_mode` (identity), cfg(test) `hook::{set, fire}` with `Stage::{BeforeRename, AfterRename}` fired around `rename`; `FsEvent::{Rescan, Error}`, `deliver()` (errors only logged), `Watcher::own_writes()`, `Document::set_own_writes()`; `ignore_own_write` removed.
- Progress: round-1 red (7 document + 4 watch, each for the right reason):
```
save_refuses_file_user_cannot_write            assertion failed: matches!(doc.save(), Err(DocError::PermissionDenied(_)))   (0o464 saved)
restored_mode_drops_setid_bits_when_owner_not_restored   left: 3565 right: 493
save_refuses_write_racing_the_temp_write       assertion failed: matches!(doc.save(), Err(DocError::ChangedOnDisk))
save_refuses_file_created_while_saving_deleted_file  assertion failed: matches!(doc.save(), Err(DocError::ChangedOnDisk))
write_right_after_rename_is_not_counted_as_ours left: InSync right: ChangedOnDisk
equal_stamp_on_coarse_clock_still_compares_content assertion failed: !snapshot_matches(&racy, &p, stamp).unwrap()
save_reports_other_hard_links                  left: 0 right: 1
watcher_errors_reach_the_caller                expected FsEvent::Error, got Err(Empty)
rescan_reports_every_watched_file              left: [] right: [Modified(a.txt), Removed(gone.txt), Rescan(dir)]
watch_move_in_from_outside_is_created          left: [Modified(x.php)] right: [Created(x.php)]
watch_ignores_own_save (registry, no manual call) left: [Modified(a.txt)] right: []
```
- Progress: round-1 green — implementation below; `cargo test -p vr-core` → 73 passed (71 after the first pass, then 2 more tests below).
- Progress: extra (found while fixing): the write probe now rejects a directory target before any temp exists, so `save_failure_leaves_no_temp_file` no longer reached the cleanup path → added `failed_rename_removes_temp_file` (hook turns the target into a non-empty dir before rename; characterization of the existing TempGuard; mutation "no remove_file in Drop" → red `left: [".vr-ide-save-622833-0.tmp", "a.txt"] right: ["a.txt"]`, restored → green). Opening a FIFO for writing blocks → red test `save_refuses_non_regular_target` hung until `timeout 15` killed it (no orphan left, checked with ps); fixed by refusing non-regular targets (`InvalidInput`, before the probe).

### Fixes per finding
| # | Finding | Reply | Fix | Test (red → green) |
|---|---|---|---|---|
| 1 | major: save replaces a file the user cannot write; owner change; setid survives | fixed | `write_atomic`: non-regular target refused; `OpenOptions::new().write(true).open(target)` probe (no truncate, no data) → `PermissionDenied`; `restored_mode(mode, owner_restored)` strips 0o6000 when fchown failed; mode applied via `Permissions::from_mode` | save_refuses_file_user_cannot_write (0o464), restored_mode_drops_setid_bits_when_owner_not_restored |
| 2a | major: check-then-rename window spans write+fsync | fixed | `save()` passes the stamp seen at its check (`Some(None)` when absent) to `write_atomic`, which re-stats the target after fsync, immediately before `rename`; mismatch → temp dropped, `ChangedOnDisk`, `disk_state = ChangedOnDisk`. Residual µs window documented in code (no CAS rename on Linux, D25). `save_overwrite`/`save_as` skip the re-check (user chose) | save_refuses_write_racing_the_temp_write, save_refuses_file_created_while_saving_deleted_file (cfg(test) hook `Stage::BeforeRename` = concurrent writer) |
| 2b | major: stamp read after dir fsync counts an agent write as ours | fixed | stamp now fstat-ed right after rename (before dir fsync), and a save snapshot is always `racy`: the next comparison (check_disk, save, watcher suppression) re-hashes instead of trusting an equal stamp | write_right_after_rename_is_not_counted_as_ours (hook `Stage::AfterRename` does a same-length in-place rewrite) |
| 3 | major: overflow/rescan and later errors dropped | fixed | `translate`: `need_rescan()` → `Modified`/`Removed` for every watched file + `FsEvent::Rescan(dir)` per watched dir; `deliver`: `Err(errors)` → `FsEvent::Error(msg)` each | rescan_reports_every_watched_file (synthetic Rescan-flag event), watcher_errors_reach_the_caller |
| m1 | minor: move-in reported as Modified | fixed | `RenameMode::To` and rename-from-outside/temp mark the path created → `Created` for tree paths (watched files still `Modified`) | watch_move_in_from_outside_is_created |
| m2 | minor: hard links split silently (D46) | fixed | `save/save_overwrite/save_as -> Result<Saved, DocError>`, `Saved { other_hard_links: u64 }` (nlink − 1 of the replaced file) | save_reports_other_hard_links |
| m3 | minor: `ignore_own_write` timing | fixed | removed `Watcher::ignore_own_write`; `Watcher::own_writes() -> OwnWrites` handed once to `Document::set_own_writes`; `write_to` records (canonical target, snapshot) right after the rename, so no per-save call exists to forget | watch_ignores_own_save (now without a manual call) |
| u | uncertain: coarse timestamps | fixed (cheap) | `Snapshot.racy` = mtime within `RACY_WINDOW` (2 s) of snapshot time (or in the future/unknown); racy or equal-length-equal-stamp → re-hash; a matched racy snapshot is refreshed so it settles; different length short-circuits to "changed" without hashing | equal_stamp_on_coarse_clock_still_compares_content |

### API changes this round (for decisions.md; T5/T6 consume them)
- `Document::save`, `save_overwrite`, `save_as` return `Result<Saved, DocError>` (was `()`); new `pub struct Saved { pub other_hard_links: u64 }` (D46 notice).
- New `pub struct OwnWrites` (opaque, Clone), `Document::set_own_writes(OwnWrites)`, `Watcher::own_writes() -> OwnWrites`; removed `Watcher::ignore_own_write`.
- `FsEvent` gains `Rescan(PathBuf)` (events lost under that watched dir: re-read it) and `Error(String)` (watcher failure after `watch` returned).
- Behaviour: save refuses any existing target we cannot open for writing (`PermissionDenied`) and any non-regular target (`Io(InvalidInput)`); setuid/setgid stripped when the owner cannot be restored; `save()` re-checks freshness right before rename.
Proposed decisions.md line: `D? (T4 r1) Save/watch hardening — writability probe + non-regular refusal; setid stripped when owner not restored; freshness re-check immediately before rename (µs residual, D25); save snapshots are racy (re-hash on next compare) and snapshots within 2 s of mtime are racy (coarse clocks); FsEvent::{Rescan, Error}; own saves registered by Document via Watcher::own_writes; Saved{other_hard_links} (D46) — review T4 findings 1–3`.

### Commands (round 1, trimmed)
```
$ cargo test -p vr-core            (×3)
test result: ok. 73 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 2.11s
$ run-heavy.sh cargo test -p vr-core --release open_100mb -- --ignored
test document::tests::open_100mb_under_2s ... ok   (0.29s)
$ cargo clippy -p vr-core --all-targets -- -D warnings   (after touch lib.rs)
    Checking vr-core v0.1.0 ... Finished `dev` profile
$ rustfmt --edition 2024 --check crates/vr-core/src/{document,watch,lib,text}.rs → exit 0
```
Files changed this round: crates/vr-core/src/document.rs, crates/vr-core/src/watch.rs, crates/vr-core/src/lib.rs (re-exports `OwnWrites`, `Saved`); `.ai-dev-team/capabilities.md` (2 lines). Edit methods: Edit tool, `perl -0pi` multi-substitutions (one perl run mangled the file head through an unescaped `|`; repaired with Edit, then switched to `#` delimiters). One empty `python3 -` heredoc was run by mistake; it wrote nothing.

### Rule rows touched
| Rule | Status | Evidence |
|---|---|---|
| BASE-2 | pass | contract changes listed above (return type of save*, FsEvent variants, own-writes registry replacing ignore_own_write) for the leader/decisions.md |
| BASE-3 | pass | red table above (11 reds + FIFO hang) before each fix; mutation for the cleanup characterization test |
| BASE-4 | pass | Commands (round 1) |
| BASE-32 | pass | D25/D41 (writability probe, re-check before rename), D46 (`Saved.other_hard_links`) |
| BASE-38 | pass | rustfmt on document.rs, watch.rs, lib.rs only |
| COST-9 | pass | the timed-out FIFO red run left no process (ps check); no background tasks |
| AC-7 | pass | rescan_reports_every_watched_file, watcher_errors_reach_the_caller, watch_ignores_own_save, write_right_after_rename_is_not_counted_as_ours |

## Fix round 2
- Progress: read `## Round 1 re-check` (R1-1 major: stale own-write entry hides a revert; R1-2 minor: metadata-only change during the write → false ChangedOnDisk). Plan: own-write entry valid only while the disk stamp is exactly the one we wrote (ctime/ino change on any write or replace), dropped at the first mismatch and on reload; re-check before rename falls back to a content hash on equal length.
- Progress: round-2 red (tests written first):
```
watch::tests::revert_to_our_saved_content_is_reported   (save C1 → agent write → reload → our saved bytes written back)
  watch.rs:473  left: []  right: [Modified("/tmp/.tmpRV59iE/a.txt")]
document::tests::metadata_change_during_save_is_not_a_conflict   (hook BeforeRename: chmod 0o640 on the target)
  document.rs:1231  called `Result::unwrap()` on an `Err` value: ChangedOnDisk
```
- Progress: round-2 green — 75 passed; then added `revert_without_reload_is_reported` (identical bytes rewritten by someone else, no reload) to pin the stamp rule on its own; mutation "is_own matches by content again" → red `left: [] right: [Modified(a.txt)]`, restored → green. Final 76 passed (×2).

### Fixes per finding
| # | Finding | Reply | Fix | Test |
|---|---|---|---|---|
| R1-1 | major: stale own-write entry hides a revert to our saved bytes | fixed | `OwnWrites::is_own` (document.rs): suppress only while the disk stamp equals the stamp we wrote (any write, chmod or replace changes ctime/inode), content re-hashed for a racy stamp; the first mismatch removes the entry (only if it is still the same entry; hashing runs without the lock). New `OwnWrites::forget`, called by `Document::reload` for its path | revert_to_our_saved_content_is_reported, revert_without_reload_is_reported, watch_ignores_own_save (still green) |
| R1-2 | minor: metadata-only change during the write → false ChangedOnDisk | fixed | `save()` passes `Expected { stamp, hash }`; `still_fresh` (right before rename): equal stamp → go; same length → re-hash and require the snapshot hash and an unchanged stamp after the read; otherwise ChangedOnDisk. The extra read is paid only on a mismatch | metadata_change_during_save_is_not_a_conflict; save_refuses_write_racing_the_temp_write / save_refuses_file_created_while_saving_deleted_file still green |

No public API change this round (`Expected`, `still_fresh`, `OwnWrites::forget` are private / crate-private).

### Commands (round 2, trimmed)
```
$ cargo test -p vr-core   (×2)
test result: ok. 76 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 2.12s
$ cargo clippy -p vr-core --all-targets -- -D warnings   (after touch lib.rs)
    Checking vr-core v0.1.0 ... Finished `dev` profile
$ rustfmt --edition 2024 --check crates/vr-core/src/{document,watch,lib,text}.rs → exit 0
$ ps check: no vr_core test process left
```
Files changed this round: crates/vr-core/src/document.rs, crates/vr-core/src/watch.rs (test only). Edit methods: Edit tool for tests, one `perl -0pi` multi-substitution for the implementation, `perl -0pi` for the mutation and its revert.

### Rule rows touched
| Rule | Status | Evidence |
|---|---|---|
| BASE-3 | pass | 2 reds above before the fixes; mutation for the characterization test |
| BASE-4 | pass | Commands (round 2) |
| BASE-32 | pass | D24/D25: a revert by someone else is never taken as ours; D47 API unchanged |
| BASE-38 | pass | rustfmt on document.rs, watch.rs only |
| COST-9 | pass | nothing left running (ps) |
| AC-7 | pass | revert_to_our_saved_content_is_reported, revert_without_reload_is_reported |
