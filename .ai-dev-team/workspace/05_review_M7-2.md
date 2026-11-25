# Review M7-2 — vr-git write API (code + security)

Worktree `.worktrees/M7-write` (branch m7-write), uncommitted changes in `crates/vr-git`.
Verdict: **CHANGES_REQUESTED** (security: ISSUES_FOUND).

`cargo test -p vr-git`: 18 + 4 + 19 + 27 + 4 + 19 + 15 passed, 0 failed, 1 ignored (bench), same as the notes.
I ran three hostile probes against the real `Writer` API with real git 2.43 in temp repos. The probe crate is at
`/tmp/claude-1000/-home-rainy-DEV-vr-ide/5ad65441-7a3e-40b9-81c7-d8de44a0c681/scratchpad/probe`.

## Findings

[high] crates/vr-git/src/cmd.rs:757 (child_env), :713 (profile_drops) — CWE-284: `GPG_TTY` reaches gpg on a signed commit, so pinentry can open the TUI's terminal (T15, SEC-12)
Data path: the editor's environment (`GPG_TTY=$(tty)`, which GitHub's docs tell users to set) → `child_env` (it filters only `GIT_*`) → `git commit` → `gpg.program` → gpg-agent `OPTION ttyname` → pinentry-curses.
Exploit: set `commit.gpgSign=true` with a passphrase that is not cached, and run without DISPLAY (ssh/tty session), then commit from vr-ide. gpg-agent starts pinentry-curses on `/dev/pts/N` by path. `setsid` does not stop this, because pinentry opens the terminal by path and does not need a controlling tty. Pinentry then draws over the TUI and reads the user's keystrokes; the passphrase dialog competes with the editor for input.
Probe: a shim `gpg.program` recorded `GPG_TTY=/dev/pts/77` from inside `Writer::commit`. The SEC-12 test only checks `/dev/tty`, which is the wrong vector.
Fix: in every write profile, drop `GPG_TTY` and set it to a non-terminal value (`GPG_TTY=/dev/null` makes a curses/tty pinentry fail cleanly; GUI pinentries still work through DISPLAY/WAYLAND_DISPLAY). Add a test: the shim `gpg.program` must not see the outer `GPG_TTY`. Uncertain residual case: a gpg-agent that was started from this terminal and has it as its default tty. Note it for SEC-19, or refuse signing when there is no display.

[major] crates/vr-git/src/remote.rs:132,150 — push silently rewrites the user's upstream in a triangular setup
Scenario: `branch.master.remote=origin`, `merge=refs/heads/main`, with `remote.pushDefault=fork` (or `branch.master.pushRemote=fork`). Then `tracked=false`, so `set_upstream=true`, and `push --set-upstream fork` changes the config to `branch.master.remote=fork`, `merge=refs/heads/master`. This was probed with real git. Later pulls and ahead/behind then follow the fork instead of origin, and nothing tells the user.
Fix: `set_upstream` only when the branch has no upstream at all (`branch.<b>.remote` and `branch.<b>.merge` both unset). Add a test for pushRemote and pushDefault that asserts the `branch.*` config is unchanged.

[major] crates/vr-git/src/recovery.rs:55-60 — the recovery-folder refusal does not name `VR_IDE_STATE_DIR` / `recovery_dir` (D303(3) not honoured)
Scenario: `$HOME` is a dotfiles repo (`~/.git`). Every discard then fails with "recovery folder ~/.local/state/vr-ide/git-recovery is inside the git work tree ~; nothing was discarded" (probed). The message gives no way out, so discard never works for these users. A side effect: the empty folders are created inside the dotfiles tree before the refusal.
Fix: append e.g. "set VR_IDE_STATE_DIR (or recovery_dir) to a folder outside any repository". Run the work-tree check on the nearest existing ancestor before `create_dir_all`. Make the test at tests/write.rs:469 assert the hint.

[major] crates/vr-git/src/write.rs:585-634 — race after the hash check: bytes written between the snapshot and the destructive step are lost with no copy
Scenario: the digest matches at :586. The copy is written from `snap`. Then `git restore` / `apply -R` / `remove_dir_all` runs on whatever is on disk at that moment. A save from vr-ide itself (M7-3 runs the discard off the UI thread), a format-on-save tool or a build writing into an untracked folder in that window is destroyed, and the recovery copy does not contain it. The code-review anchors treat any such window as data loss unless a stress test shows nothing is lost.
Fix:
- For untracked files and folders, and for whole-file discards on the same filesystem, move the target into the recovery folder with `rename(2)` and use that as the copy. That is atomic: a concurrent writer's bytes land in the copy. Then `git restore` recreates the file. Fall back to copy-then-act only across filesystems.
- In any case, make "no save of a path while a discard of it runs" an explicit contract for M7-3 (documented on `discard`).
- Add a stress test that races a writer against discard.

[minor] crates/vr-git/src/write.rs:583 — the re-check at execution does not canonicalise
`plan.rel` is relative, so `Repo::relative` skips `canonical_lenient`. If a parent component is swapped for a symlink between plan and run (by a hostile process with write access to the repo), `remove_file`/`remove_dir_all` acts through it. git's own paths refuse "beyond a symbolic link"; std::fs does not.
Fix: call `target(&workdir.join(&plan.rel))`, or check that no parent component is a symlink before an std::fs removal.

[minor] crates/vr-git/src/write.rs:708 — git's stderr from index and commit writes is returned unsanitised (remote.rs sanitises push/pull)
That is fine only if M7-3 runs every `GitError` text through `sanitize_line`. State this on `GitError::Failed`, or sanitise here too for consistency.

[minor] crates/vr-git/src/recovery.rs:17 — re-implements `vr_config::paths::state_dir` (BASE-16, declared)
The two copies can drift: `directories` also falls back to getpwuid when HOME is unset. M7-3 should pass `Options::recovery_dir` from vr-config so that one rule applies.

[minor] tests/patch_props.rs:82-125 — files of 0-11 lines with at most 4 edits almost never produce a second hunk
The start/offset arithmetic of a non-first hunk (patch.rs:91-101) is therefore barely exercised by the line properties. Add a variant with ~40-line files and edits spread far apart, so that `h > 0` is chosen often.

## Checks that held (verified)
- Patches:
  - The header is git's own (`diff-files` / `diff-index --full-index`).
  - The fresh diff must equal the shown one, the index blob (and HEAD's blob for unstage) must match the shown one, otherwise Stale.
  - `apply --check` runs first, with `apply.whitespace=nowarn` / `ignoreWhitespace=no` forced.
  - Lines after a "No newline at end of file" are refused identically in the model.
  - Binary, symlink, submodule, mode, rename, new, deleted, filter and encoding cases are refused.
  - CRLF with autocrlf is covered.
- Index lock: a held lock gives git's error and the lock stays. I also probed a cancel of a hook-running commit: no `index.lock` was left and the next stage worked.
- Commit: `-F -` on stdin, `GIT_EDITOR=true` plus `core.editor=true`, `GIT_SEQUENCE_EDITOR=true`.
- Push/pull:
  - The askpass variables are dropped, `core.askPass=` is set, `SSH_ASKPASS_REQUIRE=never`.
  - The default ssh is BatchMode.
  - The refspec is fixed `refs/heads/a:refs/heads/b` after `--end-of-options`. A `remote.*.mirror` config makes git die, so it cannot force.
  - Cancel and the idle limit kill the group, and the lines are redacted and sanitised.
- D303(1): `run_ctl` blanks drivers and keeps `core.hooksPath=/dev/null` for `Profile::Read` only. Every read path, including `Writer::repo()`, `ls-files`, `check-attr`, `state` and `config`, uses `Repo::run` / `run_code`, which means Read. The 27 M7-1 security tests are still green.

## Files read
| File | Status |
|---|---|
| crates/vr-git/src/cmd.rs | read (diff + child_env, kill_group) |
| crates/vr-git/src/diff.rs | read (diff) |
| crates/vr-git/src/lib.rs | read |
| crates/vr-git/src/repo.rs | read (diff + relative, canonical_lenient, head_oid) |
| crates/vr-git/src/write.rs | read |
| crates/vr-git/src/patch.rs | read |
| crates/vr-git/src/recovery.rs | read |
| crates/vr-git/src/remote.rs | read |
| crates/vr-git/src/state.rs | read |
| crates/vr-git/src/sanitize.rs | read |
| crates/vr-git/tests/patch_props.rs | read |
| crates/vr-git/tests/write.rs | read (test list, recovery test) |
| crates/vr-git/tests/write_security.rs | read (test list) |
| crates/vr-git/tests/common/mod.rs, tests/read.rs | skimmed (fixture env, autocrlf cases) |

## Rule check M7-2
| Rule | Status | Evidence |
|---|---|---|
| SEC-1 | pass | every write goes through `spawn` (absolute git, setsid, group kill); ssh/hook children share the group |
| SEC-2 | fail (partial) | the `GIT_*` allowlist and fixed env hold for writes, but `GPG_TTY` is passed to signing (finding 1) |
| SEC-3 | pass | Read profile is unchanged; write profiles run drivers and hooks only for user writes (D303(1)) |
| SEC-4 | pass | parsers unchanged; plumbing `-z` for ls-files, check-attr, diff-index |
| SEC-5 | pass | paths after `--` with literal pathspecs on Index; `--end-of-options` for revisions and push |
| SEC-6 | pass | `target()` = canonical `relative` plus refusal of a nested `.git` (minor finding on the execution-time re-check) |
| SEC-7 | pass | caps and timeouts kept; `hook_timeout` 600 s, network idle 120 s, recovery cap 256 MiB |
| SEC-8 | pass | reads unchanged; writes are user-only |
| SEC-9 | pass | RepoLock queue; path re-checked; lock never deleted (test plus probe); no force, reset, clean or retry |
| SEC-10 | pass | see "Checks that held"; property tests are strong except for multi-hunk coverage (minor) |
| SEC-11 | fail | copy, 0600/0700, fsync, cap, hash check and buffer_dirty all hold; the post-check race (finding 4) and the dotfiles-`$HOME` error without a way out (finding 3, D303(3)) do not |
| SEC-12 | fail | stdin message, no editor, amend and signoff hold; the gpg pinentry can reach the TUI terminal through `GPG_TTY` (finding 1) |
| SEC-13 | pass | prompt and askpass neutralised, BatchMode, no force, cancel and idle kill, redaction and sanitising (push upstream bug is filed as a correctness finding) |
| SEC-14 | pass | `state()` markers plus unmerged entries; conflicted paths, commit during rebase/am, and pull while not clean are refused; conflicting-pull test |
| BASE-16 | pass (minor) | state_dir duplication declared |
| BASE-32 | fail | D303(3) error text does not name VR_IDE_STATE_DIR / recovery_dir |

## Round 1 re-check

Verdict: **CHANGES_REQUESTED** (one major data-safety finding; security: SEC-11 partial).

`cargo test -p vr-git`: 19 + 4 + 19 + 27 + 4 + 23 + 19 passed, 0 failed, 1 ignored (bench). Both race stress tests run 5 more times: all green, 0 bytes lost (1-30 stale put-backs and 0-4 "created again" per run).
I re-ran the probe (`.../scratchpad/probe`, cases 1-3 plus a new case 5). Findings 1-3 of round 0 no longer reproduce:
- the `gpg.program` shim sees only `GPG_TTY=/dev/null`;
- with a triangular setup the probe pushes to fork/master and `branch.master.*` is unchanged;
- with a dotfiles `$HOME` the discard succeeds and `.local/` shows as ignored (`!!`).

### Findings

[major] crates/vr-git/src/recovery.rs:256-260 with write.rs:673 / :964-984 — a folder discard across filesystems can fail halfway: the user's folder is left partly deleted, and the error does not say that a full copy exists
Probe 5a: the recovery folder is on /dev/shm. The untracked folder `u/` holds `a.txt`, `z.txt` and `ro/b.txt`, and `ro` is 0555. The steps:
1. The copy is written and verified.
2. `remove_dir_all(moved)` deletes `a.txt` and `z.txt`, then fails on `ro/b.txt` (EACCES).
3. `keep` returns Err, and `put_back` renames the partial tree back to `u/`.
The result is `Err("i/o error: remove …/.vr-ide-discard-…old: Permission denied")`. `u/` now holds only `ro/b.txt`. The full copy is in the recovery folder, but the message does not name it. So the user sees a failed discard that still removed files (read-only subfolders are common in Go module caches and some build outputs).
The same misleading path exists on the same filesystem: after `rename_noreplace` into the recovery folder succeeds, a failing `symlink_metadata` or `sync_dir(dir)?` (recovery.rs:224/229) returns Err. `put_back` then cannot find the aside file and reports "created again … earlier content is in <hidden aside path>". That path does not exist, and the target path is now empty.
Fix:
- In `keep`, once the copy is verified (or the rename into the folder has succeeded), never report "nothing was discarded". If the removal fails, return an error that names the copy and says the folder was partly removed. Do not rename a partial tree back as if nothing happened. Alternatively, check before copying that every directory in the tree is writable by the owner, and refuse otherwise.
- Treat a failing `sync_dir` after a successful rename as success with a warning, not as an error.
- Add a test with a 0555 subfolder and the recovery folder on another filesystem.

[minor] crates/vr-git/src/write.rs:659-675 — a crash or kill between "move aside" and "keep" leaves the original only under a hidden name in the work tree. The aside file is also guarded by a `TempPath` that deletes it when dropped.
The window covers the re-hash, which reads up to 256 MiB, and across filesystems also the full copy, fsync and re-read. After a power loss or SIGKILL in that window, the user sees `deleted: <file>` in status, plus untracked `?? .vr-ide-discard-<pid>-<n>-<ns>.old` and `.new` files. Nothing in the docs or in vr-ide points to them. `git add -A` would commit them. A panic in that window (unwinding) runs `TempPath::drop` and deletes the user's only copy.
Fix:
- On the same filesystem, rename the target straight into the recovery folder (one atomic step, so a crash leaves it where copies are documented to be). Re-hash it there and put it back with NOREPLACE on a mismatch. Use the aside step only on EXDEV.
- Hold the aside file in a guard that does not delete it.
- Add a note for M7-3: at repo open, list leftover `.vr-ide-discard-*` files, or document how to recover them by hand.

[minor] crates/vr-git/src/write.rs:608,686,979 and recovery.rs:112,250 — the `Refused`, `Stale` and `Io` texts of writes carry raw paths (`display()` keeps ESC and BEL). The docs on `GitError` say that Writer errors are already sanitised, but they say so only for `Failed`.
A hostile repo's untracked file named `\e]52;c;…\a` gives a discard error containing the raw escape sequence. If M7-3 trusts the doc, the escape reaches the terminal.
Fix: document that every `GitError` text goes through `sanitize_line` in the UI, or sanitise in `Display`.

### Checks that held (verified)
- **GPG_TTY**: `FIXED_ENV` sets `GPG_TTY=/dev/null` on every child, and the outer value is filtered out. `GPG_AGENT_INFO` and `PINENTRY_USER_DATA` are dropped, which the unit test and the probe show. ssh-format signing has no controlling tty (setsid). The residual case of an agent started from this terminal is noted for SEC-19.
- **Upstream**: `set_upstream` is set only when `branch.<b>.remote` and `branch.<b>.merge` are both unset (remote.rs:150).
- **prepare_dir**:
  - A relative path is refused with the hint (probe 5c), so the `./.vr-ide` fallback of vr-config is refused.
  - `ls-files` runs under `Profile::Read` against the nearest existing ancestor before anything is created.
  - `check-ignore` under `Profile::Commit` keeps the forced `core.fsmonitor=false` and `GIT_ALLOW_PROTOCOL=none`. check-ignore itself starts no hook and reads no content, so no filter runs. Probe 5b: the enclosing repo has `core.fsmonitor`, a hooks path, clean/smudge/process filters on `*`, untrackedCache and splitIndex. After the discard the marker file is empty, and the recovery folder is ignored there.
  - On failure, only what this call created is removed.
- **New discard**:
  - Whole-file discard: the content comes from `cat-file --filters --path` under `Profile::Index`. It runs smudge and eol conversion exactly as the old worktree write did, which is consistent with D303(1)/T18.
  - Hunk and lines discard: `refuse_converted` excludes filter attributes before `hash-object --path`. The private `GIT_INDEX_FILE` lives in the recovery folder and is removed on drop, so the real index and the worktree stay untouched until the rename.
  - Install: NOREPLACE in both directions, a re-hash after the move aside, put-back on a mismatch, a copy and fsync across filesystems with a verify loop, and an fsync of the parent.
- **Symlinked parent**: canonical re-check at execution (`discard_does_not_follow_a_parent_swapped_for_a_symlink`). The residual TOCTOU after the check needs a hostile local writer; accepted.
- **Error text**: git's stderr from writes is sanitised (`ok_or_failed`, test with OSC 52).
- **Property tests**: one seed in three is a 30-200-line file with 2-8 edits, and each line property asserts at least 15 non-first hunks.

### Files read (round 1)
| File | Status |
|---|---|
| crates/vr-git/src/recovery.rs | read |
| crates/vr-git/src/write.rs | read (discard, reverted, worktree_form, put_back, ok_or_failed, temp paths) |
| crates/vr-git/src/cmd.rs | read (FIXED_ENV, TTY_HINT_ENV, profiles, spawn, run_ctl) |
| crates/vr-git/src/remote.rs | read (push_target, push) |
| crates/vr-git/src/lib.rs | read (GitError docs) |
| crates/vr-git/tests/patch_props.rs, write.rs, write_security.rs | read (new tests, race helper) |

## Rule check M7-2 (round 1)
| Rule | Status | Evidence |
|---|---|---|
| SEC-1 | pass | unchanged; every child through `spawn` (setsid, group kill) |
| SEC-2 | pass | `GPG_TTY=/dev/null` fixed, pinentry hints dropped; shim probe sees `/dev/null` |
| SEC-3 | pass | Read profile unchanged; check-ignore and ls-files for the recovery folder start no repo program (probe 5b) |
| SEC-4 | pass | plumbing `-z`, including ls-files `--stage` in the temp index |
| SEC-5 | pass | paths after `--`; `--path=` for content commands; `--end-of-options` on push |
| SEC-6 | pass | canonical execution-time re-check (symlinked-parent test) |
| SEC-7 | pass | content commands capped by `recovery_cap`; temp index removed |
| SEC-8 | pass | unchanged |
| SEC-9 | pass | no destructive git verbs or force; lock queue; write stderr sanitised |
| SEC-10 | pass | multi-hunk property coverage now asserted (>= 15 later hunks) |
| SEC-11 | fail (partial) | D305(3) and D305(4) hold on the normal path, with 0 bytes lost in 10 stress runs. A cross-FS folder discard can partly remove the folder and report a plain I/O error without naming the copy (major). The crash window leaves the original under a hidden worktree name (minor) |
| SEC-12 | pass | stdin message, no editor; signing cannot reach the TUI terminal via GPG_TTY |
| SEC-13 | pass | upstream set only for a branch without one (probe + test) |
| SEC-14 | pass | unchanged |
| BASE-16 | pass | duplication documented; M7-3 passes `recovery_dir` |
| BASE-32 | pass (partial) | D305(1)-(3) honoured; D305(4) except the partial-removal path above |

## Round 2 re-check

Verdict: **APPROVED**. The round-1 major and both minors are fixed. Two new minors remain; neither blocks. Fix them before M7-3 builds on `leftover_discards`.

Commands run:
- `cargo test -p vr-git`: 22 + 4 + 19 + 27 + 4 + 26 + 19 passed, 0 failed, 1 ignored (bench).
- `discard_races` run twice with `--nocapture`: same filesystem 5/4 stale and 0/2 created again; across filesystems 2/2 stale and 0/0 created again; all green.
- Probe (`.../scratchpad/probe`), case 5a: the result is `Refused("u cannot be removed because its folder u/ro is read-only; ...; nothing was discarded")`. `a.txt`, `z.txt` and `ro/b.txt` are untouched, and the recovery folder is empty. 5b and 5c are unchanged: the marker file is empty, and the relative recovery folder is refused.
- New probe case 6. 6a is the leftover listing below. In 6b a folder has `-wx` permissions: the discard fails up front with an `Io` "list ... Permission denied" error, and nothing moves.

### Findings

[minor] crates/vr-git/src/write.rs:1086-1108: `leftover_discards` matches every name that starts with `.vr-ide-discard-`, not only the names `temp_name` makes. Any name that does not end in `.old` is reported as `earlier_content: false`. The doc calls such an entry "safe to delete".
Probe 6a: the user files `.vr-ide-discard-notes.txt` and `d/.vr-ide-discard-plan.md` come back as `earlier_content: false`. If M7-3 follows the doc ("delete `.new` ones"), it offers to delete user files. A user file `.vr-ide-discard-backup.old` is also reported as a discard's earlier content.
Fix:
- Accept only the exact shape `.vr-ide-discard-<digits>-<digits>-<digits>.(old|new)`, and skip everything else.
- Set `earlier_content` only for `.old`.
- Add a test with near-miss names.

[minor] crates/vr-git/src/recovery.rs:253-287: `check_removable` catches folders without write or search permission, and immutable folders (`access(W_OK)` reports EPERM for those). It does not catch the following cases:
- (a) A mount point inside the folder. A FUSE mount such as sshfs needs no root. On the cross-filesystem path, `snapshot` copies the files under the mount, then `remove_dir_all` deletes them on the mounted filesystem and stops with EBUSY at the mount point.
- (b) Entries a sticky-bit folder protects (owned by another user).
- (c) An immutable or append-only file in a writable folder.

All three end in `KeepError::Partial`. The error text is honest: it says "partly removed" and names the complete copy. So no data is lost without notice. Only (a) reaches beyond the work tree, and a forced git clean would do the same. On one filesystem, the rename moves the mount along with the folder and deletes nothing. I could not reproduce (a) here because user namespaces are blocked; this is static analysis.
Fix (cheap, since every folder is already stat'ed):
- Refuse when a subfolder's `st_dev` differs from the top folder's.
- Optionally, refuse when a folder has the sticky bit set and is owned by someone else.

### Checks that held (verified)
- **check_removable**:
  - It runs before anything moves, whenever the target is a folder and the recovery folder's device differs from the parent's (write.rs:662).
  - It runs again in `copy_across` for bind mounts (same device, but the rename fails with EXDEV); the aside is put back.
  - It does not follow symlinks.
- **Partial always names the copy**:
  - In `move_out` (write.rs:1005-1018), the error says "partly removed" and names the copy.
  - If the remains cannot go back under their own name, the error also names the aside.
  - `put_back` maps `Partial` to the copy path. "Nothing was discarded" never appears once a copy exists.
- **Same filesystem**:
  - `move_into` renames the target with NOREPLACE straight into the recovery folder, then re-hashes it there, before `seal`, because the digest includes the mode.
  - On a mismatch it puts the target back with NOREPLACE. If that fails, the error names the copy (`created_again`).
  - EXDEV returns `None`. Every other rename error returns Err with nothing moved.
- **The EXDEV aside is never deleted**: it is a plain `PathBuf` with no `Drop`. Only the `.new` replacement keeps a `TempPath`, and deleting it is correct. The fault tests for a panic in the window (same filesystem and across filesystems) pass.
- **A failing `sync_dir` after the rename** is logged with `tracing::warn!` and the discard succeeds (recovery.rs:315-317). Before removal, a sync failure in `copy_across` still means the copy was not kept, which is correct.
- **Display**: `OutsideRepo`, `NotACommit`, `Io`, `Refused` and `Stale` pass through `sanitize_line`, which also escapes `\n`, ESC, BEL, C1 and bidi controls. The doc no longer claims more than that.
- **The fault hook is gone from release builds**:
  - `FAULT` and the test version of `fault()` are `#[cfg(test)]`.
  - Outside tests, `fault()` is `#[inline(always)] fn -> false`, so the `if fault(..) { panic!/Err }` branches fold away.
  - `discard_faults` is `#[cfg(test)]`. Integration tests and release builds compile the non-test version, so there is no runtime cost and no behaviour change.

### Files read (round 2)
| File | Status |
|---|---|
| crates/vr-git/src/recovery.rs | read (Fault, KeepError, same_filesystem, check_removable, move_into, seal, keep, copy_across) |
| crates/vr-git/src/write.rs | read (discard 600-706, temp_name, TempPath, move_out, created_again, put_back, leftover_discards) |
| crates/vr-git/src/lib.rs | read (GitError Display, docs, re-exports) |
| crates/vr-git/src/discard_faults.rs | read (fixture; tests run) |
| crates/vr-git/src/sanitize.rs | read (sanitize_line) |
| crates/vr-git/Cargo.toml, Cargo.lock | skipped (tracing workspace dep only) |
| crates/vr-git/tests/write.rs | skimmed (new tests located, run) |

## Rule check M7-2 (round 2)
| Rule | Status | Evidence |
|---|---|---|
| SEC-1 | pass | unchanged |
| SEC-2 | pass | unchanged |
| SEC-3 | pass | probe 5b: marker file empty |
| SEC-4 | pass | `leftover_discards` uses `ls-files -z` |
| SEC-5 | pass | unchanged |
| SEC-6 | pass | unchanged; canonical re-check at execution |
| SEC-7 | pass | `copy_across` and the re-hash are capped by `recovery_cap` |
| SEC-8 | pass | unchanged |
| SEC-9 | pass | unchanged |
| SEC-10 | pass | unchanged |
| SEC-11 | pass | Probe 5a is refused up front. A partial removal names the copy. The same-filesystem move is atomic into the recovery folder. The aside is never deleted. The races are green twice. Residual: mount, sticky and immutable cases end in an honest `Partial` (minor) |
| SEC-12 | pass | unchanged |
| SEC-13 | pass | unchanged |
| SEC-14 | pass | unchanged |
| SEC-15 | pass | `Display` sanitises the path-bearing variants (test with OSC 52, ESC and C1) |
| BASE-16 | pass | unchanged |
| BASE-32 | pass | D305(1)-(4) honoured, including the partial-removal path |
