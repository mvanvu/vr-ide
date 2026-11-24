## Review FR-1 — APPROVE FR-1 (two minors)

Run: `cargo test -p vr-core -p vr-search -p vr-ide keep_mine` passed (2 tests matched, no failures). I also ran `cargo test -p vr-core testutil`. vr-fs and vr-search `--test racing_writer` ran 10x with no failures.

Scope: testutil.rs only. Public signatures are unchanged: `assert_no_lost_write*` and `RacingWriter` keep their signatures. `WriterVersion` gains the `replaced` field (additive, test-util only). `Landed` is private.

Oracle:
- False negative on the reviewer's sequence (v1 deleted, a refusal, v2 saw ours, v3 replaced v2): flagged. v1 is accounted for only if a later write's `replaced` equals v1's bytes. A write that replaced ours or v2 does not prove that, and `swapping_rule_flags_a_deleted_version_the_writer_did_not_overwrite` pins it.
- False positive for a correct implementation: none seen. The legitimate case (swap-back restores v1, then the writer truncates it) gives `replaced == v1` through the handle read, and is accepted.

[minor] testutil.rs, TempRename arm — any `exchange_files` failure falls back to a plain rename with `replaced: None`
Scenario: on a filesystem without RENAME_EXCHANGE (or any exchange error other than a gone path), every version has `replaced = None`. A refused run that legitimately had `ours` visible then flags every unfinal version as "lost". This is a false positive in the strict loop on such a CI filesystem. tmpfs and ext4 support it, and the runs here were clean.
Fix: when `exchange_files` returns `Unsupported` or `EINVAL`, either skip the "not accounted" assertion for versions whose `replaced` is unknown, or note the filesystem requirement in the helper docs.

[minor] testutil.rs, in-place arm — `replaced` is trusted even when the write went to an already-replaced file
Scenario: a later in-place writer opens the old inode before the op unlinks it and reads v1 through the handle. It writes into a dead inode (`visible == false`), but `replaced == v1` still accounts for v1, although the op replaced it at the path. The window is tiny and under-reports only.
Fix: count a `replaced` proof only from versions with `visible == true`.

Read-before-truncate (weakening check): the writer no longer opens with O_TRUNC. It does open, read via the handle, `set_len(0)`, rewind, write. The empty-file window still exists, now between `set_len` and `write_all`, so an op that reads the file mid-write still sees an empty or partial file. The sweeps cover those offsets. What is lost is the exact timing of open(O_TRUNC), so an op's stat or read racing in that microsecond is placed about one read-length later. Editors that truncate on open are modelled slightly less literally, but the lost-write oracle is stronger. Acceptable.

## Files read
crates/vr-core/src/testutil.rs (diff and surrounding code), crates/vr-core/src/safe_replace.rs (`exchange_files`).
Not reviewed: the other uncommitted vr-ide changes in the main tree (outside the brief).
