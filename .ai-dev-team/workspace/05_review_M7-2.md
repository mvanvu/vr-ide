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
