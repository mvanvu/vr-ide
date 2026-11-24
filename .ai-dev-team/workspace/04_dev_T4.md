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

## Fix round 3 (worktree .worktrees/T4r3, branch t4-round3)
- Progress: read brief (T12 finding: strict racing-writer test loses ~13%), testutil.rs, 04_dev_T12.md, vr-search replace.rs commit_replace (D49). Plan: shared `vr_core::safe_replace` (exchange + verify swapped-out file + swap-back that must bring back our own inode, else keep a visible conflict copy; NOREPLACE when no file is expected; documented stat+rename fallback without EXCHANGE); Document::save and vr-search replace both use it; vr-core gains the workspace `rustix` dependency (D25/D45 crate).
- Progress: red — new cfg(test) stage `BeforeSwap` (after the last check, right before the file is put in place) + 3 document tests: save_never_replaces_in_place_write_at_swap → `Ok(Saved { other_hard_links: 0 })` (agent version replaced), save_never_replaces_renamed_in_version_at_swap → `Ok(Saved { other_hard_links: 0 })`, save_never_replaces_file_created_at_swap → `assertion failed: matches!(doc.save(), Err(DocError::ChangedOnDisk))`. Strict racing test before the fix: `2/20 failed` ("lost write: 1 writer version(s) begun before the operation finished were replaced by our bytes and the operation reported success").
- Progress: red — `vr_core::safe_replace` API on `todo!()` + 4 tests (replaces_an_unchanged_file, write_between_check_and_swap_is_swapped_back [exchange and fallback], version_put_in_place_during_swap_back_is_kept [new: swap-back must return our inode], create_if_absent_never_replaces) → 4 FAILED (not yet implemented).
- Progress: green — helper implemented (logic moved from vr-search replace.rs, plus the inode check on swap-back and create_if_absent); `write_atomic` commits through it; vr-core 94 passed. Strict test un-ignored. vr-search `commit_replace` now a wrapper over the helper (its seam tests unchanged and green).

### Design (closes the D25 window)
- `save()` (check on): 1) the existing pre-check (stamp, else content) refuses early; 2) after the temp file is written and fsynced, `safe_replace::replace_if_unchanged`: check the target again, `renameat2(RENAME_EXCHANGE)` temp ⇄ target, then check the swapped-out file by content (length + SipHash = the snapshot hash; its ctime changed with the swap, so no stamp shortcut). Match → delete it, `Ok`. Mismatch → swap back; the file that comes back must be our temp inode → delete it, `ChangedOnDisk` (other version untouched at the path). If a third version was put at the path between the two swaps, or the swap-back / verify fails, that version is kept as `<name>.vr-ide-conflict-PID-N` (never deleted) and save returns `DocError::Io` whose message ends "kept at <path>".
- File expected absent (deleted on disk): `create_if_absent` = `RENAME_NOREPLACE` → else hard link (atomic no-replace) → else check + rename.
- `save_overwrite` / `save_as` (user chose): plain `rename` as before.
- Not a loss by definition (testutil `visible = false`): a writer that holds the old file open and writes after the swap writes into a file no longer at the path.
- Public API unchanged (`save/save_overwrite/save_as -> Result<Saved, DocError>`, `ChangedOnDisk`); additive: `pub mod vr_core::safe_replace`. vr-core gains `rustix.workspace = true` (fs feature, already a workspace dependency per D25/D45; Cargo.lock: rustix added under vr-core only).

### Fallback without RENAME_EXCHANGE (for decisions.md)
Proposed line: `D? (T4 r3) Save commit — Document::save and vr-search replace share vr_core::safe_replace: RENAME_EXCHANGE + content check of the swapped-out file + swap-back that must return our own inode (else the other version is kept as <name>.vr-ide-conflict-PID-N, never deleted, error "kept at"); absent target → RENAME_NOREPLACE → hard link → check+rename. Fallback where EXCHANGE answers EINVAL/ENOTSUP (non-Linux, some FUSE/network filesystems): check right before a plain rename(2); a write landing between that check and the rename is replaced (the D25/D32 window, measured ≈10–15 % under a racing writer on this machine); on vfat/exFAT without hard links the absent-target case has the same window. Amends D25 for vr-core and D49.`
Code documents it in `crates/vr-core/src/safe_replace.rs` (module doc) and `create_if_absent` doc. Mutation evidence that the fallback has the window: forcing it in `save` → strict test `3/20 failed`.

### Acceptance evidence
(1) strict test enabled (testutil.rs, `#[ignore]` removed), 50 consecutive runs:
```
strict loop: pass=50 fail=0
     13 save: ok=23 refused=17      (per-run split of the 40 racing saves; log: qa-artifacts/T4r3/strict_loop.log)
     13 save: ok=22 refused=18
     11 save: ok=24 refused=16
      4 save: ok=26 refused=14
      4 save: ok=25 refused=15
mutant (exchange forced off): 3/20 failed  → restored
```
(2) `cargo test -p vr-core -p vr-fs -p vr-search` (worktree):
```
vr-core   test result: ok. 95 passed; 0 failed; 1 ignored (open_100mb, AC-18 timing)
vr-fs     test result: ok. 43 passed; 0 failed; 1 ignored (pre-existing trash helper) + racing_writer 2 passed
vr-search test result: ok. 34 passed; 0 failed; 1 ignored (pre-existing project.rs:817) + racing_writer 2 passed
vr-search racing_writer + replace tests looped 30×: failures=0
```
`cargo clippy -p vr-core -p vr-search --all-targets --features vr-core/test-util -- -D warnings` → Finished, no warnings. `rustfmt --edition 2024 --check` on document.rs, safe_replace.rs, testutil.rs, lib.rs, vr-search replace.rs → exit 0. `ps`: no test process left.
(3) fallback documented above and in code.

Files changed (worktree): crates/vr-core/Cargo.toml (+rustix), Cargo.lock (rustix under vr-core), crates/vr-core/src/{lib.rs (+`pub mod safe_replace`), safe_replace.rs (new), document.rs (commit via helper, `holds_expected`, stage BeforeSwap, 3 tests), testutil.rs (strict test enabled)}, crates/vr-search/src/replace.rs (commit_replace → wrapper; exchange_files/keep_conflict/TmpHolds/commit_inner moved to vr-core). Main tree: .ai-dev-team/capabilities.md (safe_replace line), these notes. Edit methods: Write for the new module (tests first, then the implementation spliced over the saved test module), Edit for document.rs, perl -0pi for small multi-edits and the mutation, sed/cat splice for replace.rs.

### Reuse
| Needed | Looked in | Result |
|---|---|---|
| compare-and-swap replace | vr-search replace.rs `commit_replace` (D49), capabilities.md | moved into `vr_core::safe_replace` (strengthened swap-back inode check), vr-search now calls it — one implementation |
| racing writer | `vr_core::testutil` (T12) | reused unchanged; strict test enabled |

### Rule rows touched
| Rule | Status | Evidence |
|---|---|---|
| BASE-2 | pass | Document API unchanged; additive `vr_core::safe_replace` |
| BASE-3 | pass | reds above (3 document, 4 helper, strict 2/20) before code; mutation 3/20 |
| BASE-4 | pass | Acceptance evidence above |
| BASE-8 | pass | rustix is a workspace dependency (D25/D45); added to vr-core per leader brief (shared helper) |
| BASE-16 | pass | Reuse table; capabilities.md line |
| BASE-32 | pass | D24/D25/D49 no-overwrite, now without the window where EXCHANGE exists |
| BASE-38 | pass | rustfmt on the 5 changed files only |
| COST-9 | pass | no background tasks; ps check clean |
| AC-3 | pass | save still atomic (exchange/rename), all save_* tests green |

## Escalation (worktree .worktrees/T4r3, branch t4-round3)
Skills called: ai-dev-team:implementation. Tools available: Bash, Read, Edit, Write, Skill, ToolSearch, SubagentHandback (Agent/Artifact not used: BASE-18 / user rule).
- Progress: read brief-common, notes (all rounds), 05_review_T4.md (R0–R3), QC T4 r3, decisions D25/D32/D45–D49/D63/D64, safe_replace.rs, testutil.rs, document.rs save path, vr-search replace.rs commit path and tests.
- Progress: red 1 — `safe_replace::tests::in_place_write_between_swap_and_swap_back_is_kept` (reviewer's sequence) on the round-3 code: `expected Kept, got Changed; target Ok("agent1\n"), dir ["a.txt"]` (v2 deleted).
- Progress: red 2 — vr-search `replace::tests::commit_keeps_in_place_write_made_between_swap_and_swap_back`: `file changed on disk since the preview; not written; dir ["a.txt"]` (v2 deleted, no "kept at").
- Progress: red 3 — D64 `document::tests::save_overwrite_seen_replaces_only_the_version_shown` against a stub that calls `save_overwrite`: `Ok(Saved { other_hard_links: 0 })` (the newer agent version replaced).
- Progress: state machine written (below), `safe_replace.rs` rewritten to it, Document and vr-search callers moved to `Checks`/`Step`, D64 API implemented. vr-core 109 / vr-search 35 green.
- Progress: seam tests per step added; guards that went green together with the implementation were each seen red by mutation (table below); one extra test added because mutant M1 survived (`same_length_write_with_restored_mtime_is_kept`).
- Progress: racing test now also fails on any file left behind that the error does not name; fast-writer racing test added; loops and guards run (below).

### Root cause
Rounds 1–3 each proved something about one name at one instant (stamp before rename; content of the file taken out; identity of the file brought back) and then unlinked a name. The protocol never stated which files may be unlinked at all. Once our file `N` has been at the target it is an ordinary user-visible file: any writer may have written into it or replaced it, so "it is our inode" says nothing about whose bytes it holds. The rule that was missing: **a file is unlinked only if it was never at the target, or its content was proven at the moment of unlinking** (expected version by content; ours by identity + unwritten + content). Everything else is kept.

### Commit protocol — state machine
Names: `T` target (links resolved by the caller), `P` private temp name in the same directory (`.vr-ide-save-PID-N.tmp`, created exclusively), `K` visible `<name>.vr-ide-conflict-PID-N` (created without replacing). Files: `N` ours, `X` the file at `T` at exchange 1, `Y` the file at `T` at exchange 2. An outside writer acts on `T` only: W = write in place by path, R = rename another file over it, U = unlink, C = create. "fd" = a handle it opened earlier.

| # | Our step | State after | Outside action in the gap before this step | What happens to its data |
|---|---|---|---|---|
| 0 | create `P` exclusively, write, fsync | `T`=old, `P`=`N` | any | handled by step 1 |
| 1 | check `T` = expected (stamp, else content) | unchanged | W/R/U/C since load | seen → `P` unlinked (`N` never at `T`), `Changed`; data untouched at `T` |
| 2 | `EXCHANGE(P,T)` (atomic) | `T`=`N`, `P`=`X` | W/R after step 1 | it is in `X`, now at `P`: step 3 decides. U → ENOENT, `P`=`N` never exposed → unlinked, `Changed`. R of a symlink/dir → `X` is not a regular file → step 5 |
| 3 | prove `X`: regular file, content = expected, lstat equal before/after the read | — | fd-write into `X` before/during the read | content or lstat differs → step 5 (nothing unlinked) |
| 4 | unlink `P` (`X` = the verified original) → `Replaced` | `T`=`N` | W/R/U on `T` after step 2 | acts on `N`/after our commit: theirs is at `T` (later write wins), nothing of theirs unlinked |
| 5 | `EXCHANGE(P,T)` (atomic) | `T`=`X`, `P`=`Y` | W into `N` at `T` | `Y`=`N` with their bytes → step 6 keeps it at `K` |
|   |   |   | R over `T` | `Y`= their file → step 6 keeps it at `K` (identity) |
|   |   |   | U of `T` | exchange fails ENOENT: `X` kept at `K`, `Kept` (`T` stays absent) |
|   |   |   | exchange fails otherwise | `X` kept at `K`, `Kept` (`T` holds `N`) |
| 6 | prove `Y`: regular, same dev+ino as `N`, same length and mtime as before step 2, content = ours, lstat equal before/after the read | — | open(O_TRUNC) of `N` while at `T`, bytes not yet written | length or mtime changed (mtime also when ours is empty) → `K`; the later bytes land in `K` |
|   |   |   | any failure of the proof (also an I/O error) | `Y` kept at `K`, `Kept`, error "… kept at <K>" |
| 7 | unlink `P` (`N`, proven) → `Changed` | `T`=`X` | W/R/U on `T` after step 5 | acts on `X` at `T`; we do not touch `T` again |
| K | `RENAME_NOREPLACE(P,K)`, else `link(P,K)`+unlink `P`; if both fail `P` stays and is reported | — | — | never replaces an existing file |

Expected-absent target (`create_if_absent`): `RENAME_NOREPLACE(P,T)` (C in any gap → AlreadyExists → `P` unlinked, never exposed) → else `link(P,T)` (atomic no-replace; then only the name `P` is removed) → else check + rename (vfat/exFAT, D32 window).

Unlinks in the whole protocol: `P`=`N` never exposed (steps 1, 2-U, errors before step 2 → caller's guard); `P`=`X` after step 3; `P`=`N` after step 6; the name `P` after a successful hard link. No `Err` can leave `replace_if_unchanged` after step 2, so the callers' "delete the temp on error" can only ever hit the never-exposed `N`.

Not covered, stated in the module doc and proposed for decisions.md:
1. Fallback without EXCHANGE (EINVAL/ENOTSUP): check right before a plain `rename(2)` — D63's documented window, unchanged (no lossless variant exists without an atomic exchange: a hard-link backup would catch in-place writes but not a rename-over).
2. A writer that opened `X` or `N` **without truncating** while it was at `T` and writes through that handle after the file left `T` and passed its proof (gap between steps 3→4 or 6→7): the bytes go to an unlinked file. Same for every rename-based save; the reviewer accepted it for `X` in round 3.
3. On a refused save our bytes are readable at `T` between steps 2 and 5 (µs when the length differs; one read of the file when it has the expected length). The fast-writer racing test observed this 6–8 times in 50 runs × 80 saves; `testutil::assert_no_lost_write` treats such a sighting on a refused run as a failure, so a new `assert_no_lost_write_swapping` is used for that test only (the existing strict tests keep the old rule and passed 50/50, but the old rule can in principle fire there too — leader's call whether vr-search/vr-core strict tests should move to the swapping rule).
4. Crash between steps 2 and 5: `T` holds ours, the other version sits under the temp name → stale temps must never be auto-deleted (nothing in the workspace does: `grep is_save_temp` → watcher filter only).

### Required behaviour
| Req | Where |
|---|---|
| (a) only proven files are unlinked | `safe_replace::proven` + table above |
| (b) in doubt → `K` + "kept at" | `safe_replace::keep`, `kept_error`; Document returns `DocError::Io` whose message ends "kept at <path>", vr-search the same per file |
| (c) public Document API unchanged | `save`, `save_overwrite`, `save_as`, `DocError::ChangedOnDisk` untouched. Changed: `vr_core::safe_replace::replace_if_unchanged` signature (`Checks`, `Step`; D63's helper module, only callers are Document and vr-search) |
| (d) D64 | additive `Document::disk_version() -> Result<DiskVersion, DocError>` and `Document::save_overwrite_seen(&DiskVersion) -> Result<Saved, DocError>`; `DiskVersion` re-exported from the crate root. T6 flow: `save()` → `ChangedOnDisk` → `let seen = doc.disk_version()?` when the prompt is shown → "keep mine" → `doc.save_overwrite_seen(&seen)` → `ChangedOnDisk` again means a newer version arrived: prompt again with a fresh `disk_version()`. A file absent at the prompt is a version too (a file created since is not replaced). |

### Tests (red evidence)
| Test | Red |
|---|---|
| safe_replace `in_place_write_between_swap_and_swap_back_is_kept` | red 1 above (before the code) |
| vr-search `commit_keeps_in_place_write_made_between_swap_and_swap_back` | red 2 above (before the code) |
| document `save_overwrite_seen_replaces_only_the_version_shown` | red 3 above (stub) |
| document `save_keeps_second_write_made_while_restoring` (BeforeSwap + BeforeSwapBack hooks), `save_overwrite_seen_never_replaces_a_write_racing_it`, `save_overwrite_seen_with_no_file_shown` | need the new hook stage / API; written with the implementation, guarded by the mutants below (M1/M3 change `Kept` to `Changed`) |
| step 1→2 gap: `write_between_check_and_swap_is_swapped_back` (existing), `version_renamed_in_between_check_and_swap_is_swapped_back`, `file_deleted_between_check_and_swap_stays_deleted` | characterization (round-3 behaviour kept) |
| step 2 type change: `symlink_put_in_place_before_swap_is_restored` | mutant M4 (regular-file check off): FAILED |
| step 3 read: `write_during_the_check_of_the_swapped_out_file_restores_it` | mutant M5 (no lstat-stable check): FAILED |
| steps 2→4: `write_after_our_file_is_in_place_wins` (AfterSwap, BeforeDeleteOld × in-place, rename-over) | characterization |
| steps 2→5: reviewer's test; `version_put_in_place_during_swap_back_is_kept` (existing); `version_with_our_bytes_renamed_in_during_swap_back_is_kept` | mutant M3 (identity off): FAILED |
| steps 2→5: `same_length_write_with_restored_mtime_is_kept` | mutant M1 (content proof of ours off): FAILED (the only test that needs the content check: length and mtime are restored) |
| steps 2→5: `file_opened_for_writing_while_ours_was_in_place_is_kept` (ours empty and non-empty) | mutant M2 (mtime not compared): FAILED; M3: FAILED |
| steps 2→5: `file_deleted_between_swap_and_swap_back_keeps_the_other_version` | characterization |
| steps 5→7: `write_after_swap_back_stays_at_the_target` (AfterSwapBack, BeforeDeleteOurs × in-place, rename-over) | characterization |
| testutil `swapping_rule_allows_sight_of_ours_only_on_a_refused_run` | new assertion's own test |
Mutants were applied to `safe_replace.rs` one at a time and the file restored from a byte-identical copy (`cmp` → restored).

### Results (worktree, final code)
```
$ cargo test -p vr-core -p vr-fs -p vr-search
vr-core   test result: ok. 112 passed; 0 failed; 1 ignored   (ignored: open_100mb, AC-18 timing)
vr-fs     test result: ok. 43 passed; 0 failed; 1 ignored  + racing_writer 2 passed
vr-search test result: ok. 35 passed; 0 failed; 1 ignored  + racing_writer 2 passed
$ 50 × cargo test -p vr-core --features test-util --lib document_save_never_loses   (2 tests per run:
  document_save_never_loses_a_concurrent_write + ..._a_write_from_a_fast_writer)
strict loop (2 tests per run): pass=50 fail=0        log: qa-artifacts/T4esc/strict_loop.log
$ 30 × (cargo test -p vr-search --test racing_writer && cargo test -p vr-search --lib replace::)
vr-search loop: pass=30 fail=0                       log: qa-artifacts/T4esc/search_loop.log
$ cargo clippy -p vr-core -p vr-search --all-targets --features vr-core/test-util -- -D warnings
    Finished `dev` profile
$ rustfmt --edition 2024 --check crates/vr-core/src/{testutil,safe_replace,document,lib}.rs crates/vr-search/src/replace.rs → exit 0
$ ps: no test process left
```
Honest limit of the racing evidence: in all loops the racing tests reported `kept=0`, i.e. the random writer never placed a second write inside the exchange→swap-back gap (it is sub-µs for versions of another length). That gap is proven by the deterministic seam tests and mutants above, not by the race.

### Files changed (worktree)
crates/vr-core/src/safe_replace.rs (protocol, `Checks`, `Step`, `proven`, 15 tests), crates/vr-core/src/document.rs (`DiskVersion`, `disk_version`, `save_overwrite_seen`, commit via `Checks`, hook stage `BeforeSwapBack` and several hooks per save, 4 tests), crates/vr-core/src/lib.rs (re-export `DiskVersion`), crates/vr-core/src/testutil.rs (`assert_no_lost_write_swapping`, leftover-file check in the save race, fast-writer test), crates/vr-search/src/replace.rs (`commit_replace` passes our bytes; 1 test). Main tree: .ai-dev-team/capabilities.md (3 entries), .ai-dev-team/workspace/decisions.md (3 `D?` lines), these notes.
Incident: one multi-substitution perl run with `|` delimiters put two replacement blocks at wrong offsets in document.rs and vr-search replace.rs; both were removed by exact offset (only inserted text and one restored character), the intended edits re-applied by literal match, and the files compile, pass and are rustfmt-clean. Later edits used a literal-match script (scratchpad) or the Edit tool.

### Out-of-scope issues noticed
- vr-search `apply_one` re-reads the file (`read_capped`) and then `commit_replace` reads it twice more; the exposure in point 3 for a same-length change is one read of the file. A stamp fast-path would shorten it; not done (behaviour change outside the finding).
- `Document` does not set `disk_state` when a save ends with a conflict copy; the next `check_disk()` reports `ChangedOnDisk` (asserted in `save_keeps_second_write_made_while_restoring`).

### Reuse
| Needed | Looked in | Result |
|---|---|---|
| expectation of a disk version for D64 | document.rs `Expected`, `still_fresh`, `holds_expected` | reused: `DiskVersion` wraps `Expected`, `save_overwrite_seen` goes through `write_to` like `save` |
| racing writer | `vr_core::testutil` | reused; one assertion variant and one test added |

## Rule compliance (escalation)
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | files of the board row only (+ lib.rs re-export, capabilities/decisions/notes) |
| BASE-2 | pass | Document contract unchanged; additive `disk_version`/`save_overwrite_seen`/`DiskVersion`; `safe_replace::replace_if_unchanged` signature change listed for decisions.md |
| BASE-3 | pass | reds 1–3 before the code; guards added with the code each seen red by mutant (table) |
| BASE-4 | pass | Results block |
| BASE-5 | pass | only crate-scoped test runs and loops in the foreground; nothing left running (ps) |
| BASE-6 | pass | English, why-comments; module doc carries the state machine |
| BASE-8 | pass | no manifest change |
| BASE-10 | pass | `-p vr-core -p vr-fs -p vr-search` only |
| BASE-11 | pass | no git commands at all |
| BASE-14 | pass | longest foreground loop ≈ 2 min, under the limit; no polling |
| BASE-15 | pass | no background tasks started |
| BASE-16 | pass | Reuse table; capabilities.md updated |
| BASE-17 | pass | no installs |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | progress lines above |
| BASE-24 | pass | no git add/commit |
| BASE-25 | pass | rustfmt --check on the 5 files exit 0; clippy -p vr-core -p vr-search --all-targets -D warnings clean (workspace-wide guard not run from the worktree: other crates are other tasks') |
| BASE-32 | pass | D25/D49/D63: table above; D64: `save_overwrite_seen`; D32/D63 fallback kept as decided |
| BASE-38 | pass | rustfmt only on the 5 changed files |
| COST | pass | related tests only; loops as the brief requires; scratch files in the session scratchpad and qa-artifacts/T4esc |
| AC-3 | pass | save_* tests green (112) |
n-a: BASE-7, BASE-9, BASE-12, BASE-13, BASE-20–23, BASE-26–31, BASE-33–37 (not exercised by this fix: no UI, no removals, no external sources, no new names outside neighbours' convention)

### Follow-up (D76 point 2, tests only)
- Progress: strict racing tests moved to `assert_no_lost_write_swapping`: `race_document_save(true)` in crates/vr-core/src/testutil.rs (`document_save_never_loses_a_concurrent_write`; the fast-writer test already used it) and `apply_replace_never_loses_a_concurrent_write` in crates/vr-search/tests/racing_writer.rs. The no-writer tests keep `assert_no_lost_write`. The swapping rule still fails a refused run whose final content is ours (`swapping_rule_allows_sight_of_ours_only_on_a_refused_run`, third case). No non-test code changed.
```
$ 50 × (cargo test -p vr-core --features test-util --lib document_save_never_loses && cargo test -p vr-search --test racing_writer)
strict loop: pass=50 fail=0          log: qa-artifacts/T4esc/strict_loop_swapping.log
$ cargo test -p vr-core -p vr-search → vr-core 112 passed / 1 ignored; vr-search 35 passed / 1 ignored + racing_writer 2 passed
$ clippy -p vr-core -p vr-search --all-targets --features vr-core/test-util -D warnings → Finished; rustfmt --check on the 2 files → exit 0; ps: nothing running
```

### Escalation follow-up 2 (review `## Escalation re-check`, D77)
- Progress: read the re-check (1 major test rule, 2 minor) and D77. Tests first, all five seen red in one run before any fix:
```
testutil::tests::swapping_rule_flags_version_written_into_our_file_and_deleted - should panic ... FAILED   (note: test did not panic as expected)
testutil::tests::swapping_rule_flags_deleted_version_that_never_saw_ours - should panic ... FAILED          (note: test did not panic as expected)
document::tests::disk_version_is_of_one_version_of_the_file          left/right: same hash, stamp mtime …24387477 vs …25233829 (stamp of v1 paired with the hash of v2)
document::tests::disk_version_of_a_file_that_keeps_changing_is_an_error   Ok(DiskVersion(..)) where Err(Io) is expected
document::tests::save_keeps_second_write_made_while_restoring        left: InSync  right: ChangedOnDisk   (new assertion on `doc.disk_state` before `check_disk`)
test result: FAILED. 112 passed; 5 failed; 1 ignored
```
- Progress: fixes applied, green; loops and guards below.

| # | Finding | Reply | Fix | Tests |
|---|---|---|---|---|
| 1 | major: `_swapping` rule skips `overtaken` on refused runs | fixed | `assert_no_lost_write_swapping(.., kept)`: on a run that reported an error the blanket exemption is gone; `assert_versions_accounted_for` requires every version that was visible at the path **or** whose writer saw our bytes to be the final content, or in a conflict copy the error names, or seen by a later writer version (overwritten by the writer itself). One excuse only, for a version that did not itself see ours: the next write began while our bytes stood in for it (then that write, not the save, replaced it after the swap-back). Sight of ours on a refused run stays allowed. Runs that reported success keep the old rules unchanged. The racing tests (vr-core save ×2, vr-search apply) pass the bytes of the conflict copy named by the error | `swapping_rule_flags_version_written_into_our_file_and_deleted` (R3: v2 saw ours, v3 found v1 again), `swapping_rule_flags_deleted_version_that_never_saw_ours` (same loss when the writer had read the path earlier — wider than the reviewer's rule), `swapping_rule_accepts_sight_of_ours_when_the_version_survives` (overwritten by the writer / kept copy / final), `swapping_rule_allows_sight_of_ours_only_on_a_refused_run` (still green) |
| 2 | minor: `disk_version()` stamp taken before the read, not re-checked | fixed | stamp → read → stamp again by path; a moved stamp (or a file deleted in between) re-reads, up to `DISK_VERSION_TRIES` = 5, then `DocError::Io("the file keeps changing on disk; try again")`. Rustdoc tells T6 to take it before reading the text the prompt shows | `disk_version_is_of_one_version_of_the_file`, `disk_version_of_a_file_that_keeps_changing_is_an_error` (new test seam `Stage::ReadingDiskVersion`) |
| 3 | minor: conflict copy leaves `disk_state` InSync | fixed | `Written::Kept(err)` → `write_to` sets `disk_state = ChangedOnDisk`, returns the same `DocError::Io` ("… kept at <path>") | `save_keeps_second_write_made_while_restoring` |

```
$ cargo test -p vr-core -p vr-fs -p vr-search
vr-core 117 passed / 1 ignored; vr-fs 43 passed / 1 ignored + racing_writer 2; vr-search 35 passed / 1 ignored + racing_writer 2
$ 50 × cargo test -p vr-core --features test-util --lib document_save_never_loses   (2 tests per run)
strict save loop (2 tests per run): pass=50 fail=0     log: qa-artifacts/T4esc/strict_loop_fu2.log
$ 30 × (cargo test -p vr-search --test racing_writer && cargo test -p vr-search --lib replace::)
vr-search loop: pass=30 fail=0                         log: qa-artifacts/T4esc/search_loop_fu2.log
$ cargo clippy -p vr-core -p vr-search --all-targets --features vr-core/test-util -- -D warnings → Finished
$ rustfmt --edition 2024 --check (testutil, safe_replace, document, lib, vr-search replace.rs, tests/racing_writer.rs) → exit 0;  ps: nothing running
```
Limit unchanged: the racing loops again reported `kept=0` in all 100 save runs, so the strict oracle's refused-run branch with a conflict copy is exercised by its self-tests and the seam tests, not by the race.
Files changed this round (worktree): crates/vr-core/src/testutil.rs, crates/vr-core/src/document.rs, crates/vr-search/tests/racing_writer.rs. No change to safe_replace.rs or vr-search src. Additive API: `DiskVersion: PartialEq + Eq`; `assert_no_lost_write_swapping` gained the `kept` argument (test-util). Main tree: capabilities.md (one entry), decisions.md (one `D?` line), these notes.

| Rule | Status | Evidence |
|---|---|---|
| BASE-3 | pass | five reds above before the fixes |
| BASE-4 | pass | block above |
| BASE-2 | pass | Document contract unchanged; additions listed |
| BASE-32 | pass | D77 oracle; D64 token consistent |
| BASE-38 | pass | rustfmt on the changed files only |
| COST-9 | pass | nothing running |
