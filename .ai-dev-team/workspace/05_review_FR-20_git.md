# Review FR-20, group A (vr-git, vr-config, Cargo.lock)

Reviewer, opus. Scope: `git diff HEAD` in `.worktrees/FR-20` (base b2d1b61) for crates/vr-git, crates/vr-config, Cargo.lock.
Tools used: Read, Bash (git diff/show, grep, one `run-heavy.sh cargo test -p vr-git`), Write (this file).

Scope note: `git diff master` also shows `crates/vr-config/defaults/keymap.toml` and `crates/vr-config/tests/config.rs` (the ctrl+f7 terminal line going away). Those are master moving ahead (FR-17 merged after b2d1b61). The worktree does not touch them (`git diff HEAD` lists only settings.toml and settings.rs), so a merge keeps FR-17's line. Not an FR-20 change.

Test run (`cargo test -p vr-git`, run-heavy): unit 7 ok, read 13 ok, security 26 ok, worker 4 ok, bench 1 ignored. Log: `qa-artifacts/FR-20/review_git_test.log`.

## Findings

[minor] crates/vr-git/tests/security.rs:585 (`submodule_local_filter_never_runs`), also :89 `core_fsmonitor_never_runs`, :147 `show_signature_gpg_program_never_runs`. Guards no kept read reaches.
Scenario: the kept reads are `symbolic-ref`/`rev-parse`, `rev-list` for ahead/behind, `ls-files --stage`, `cat-file`, `check-attr`, `config --get` and `blame --contents -`. None of them refreshes the index, recurses into a submodule or runs `log`. If someone deleted `core.fsmonitor=false`, `diff.ignoreSubmodules=dirty` or `log.showSignature=false` from FIXED_CONFIG, these three tests would still pass. They are still useful as regression guards: `read_everything` runs every public read, so a future read that does reach these paths (for example a new `status`) gets tested. They are not oracles for today's code. The developer's flake was in the submodule test's control step (`plain git status` sometimes does not run the filter, a stat-timing issue). That makes the test fail loudly. It never makes it pass when it should fail, so the test is not weakened. Removing the diff-files step was correct because `diff_unstaged` is gone.
Fix: add one line above `read_everything` (or above these three tests) saying they guard future reads only. Optionally make the control deterministic, for example by retrying the control `status` once, so `cargo test -p vr-git` stops flaking.

[minor] crates/vr-git/tests/worker.rs:173. The comment and the dev notes say the test checks that `invalidate_path` drops the repo-wide replies. It does not.
Scenario: the test only proves that another file's reply stays cached and this file's reply is dropped. It never requests `Head`/`AheadBehind` between `invalidate_path` calls. If `REPO_WIDE_KINDS` became 0, Head would survive a file invalidation and every test would stay green. The behaviour itself is right: `retain(kind >= 2)` drops Head (0) and AheadBehind (1), the same as the old `kind > 2` minus Status.
Fix: request `Head` before and after `w.invalidate_path(&f.path("b.txt"))` and assert `!Arc::ptr_eq`. Or reword the comment.

[minor] crates/vr-git/src/cmd.rs:93-95, :110. Comments justify config by commands vr-git no longer runs: "status and diff-files also pass the flag" and "so `log --follow` drops it". The `status.*` and `log.*` keys are harmless defence in depth, but the comments now point at parsers that no longer exist.
Fix: change the comments to "kept as defence in depth for any later read", or drop the `log.*` and `status.*` keys.

[minor] crates/vr-git/src/lib.rs:35, :50. Two doc-comment lines were rejoined to 158 and 107 columns. rustfmt does not wrap comments, so the guard passes, but the rest of the file wraps at 100.
Fix: rewrap.

Cross-group note (for the vr-ide reviewer, uncertain): `leftover_discards` is gone. Recovery copies under `~/.local/state/vr-ide/git-recovery` that earlier builds left behind are no longer listed anywhere. Check that the README or docs mention the folder if any user may still have copies there.

## Files read

| File | Status |
|---|---|
| Cargo.lock | read (diff hunk only: `tracing` dropped from vr-git deps; package kept for other crates) |
| crates/vr-config/defaults/settings.toml | read |
| crates/vr-config/src/settings.rs | read |
| crates/vr-config/defaults/keymap.toml | skipped: not changed by the worktree (master-side FR-17 diff) |
| crates/vr-config/tests/config.rs | skipped: not changed by the worktree (master-side FR-17 diff) |
| crates/vr-git/Cargo.toml | read |
| crates/vr-git/src/lib.rs | read |
| crates/vr-git/src/cmd.rs | read (diff + kept FIXED_ENV/FIXED_CONFIG/child_env) |
| crates/vr-git/src/diff.rs | read (kept line_changes compared with base: identical) |
| crates/vr-git/src/blame.rs | read |
| crates/vr-git/src/repo.rs | read |
| crates/vr-git/src/sanitize.rs | read |
| crates/vr-git/src/worker.rs | read |
| crates/vr-git/src/discard_faults.rs (deleted) | read (master test list; write side only) |
| crates/vr-git/src/log.rs (deleted) | read (Signature moved to blame.rs; rest log-only) |
| crates/vr-git/src/patch.rs (deleted) | read (test list; staging patches only) |
| crates/vr-git/src/programs.rs (deleted) | read (test list; write programs) |
| crates/vr-git/src/recovery.rs (deleted) | read (test list; discard recovery) |
| crates/vr-git/src/remote.rs (deleted) | read (push/pull jobs) |
| crates/vr-git/src/state.rs (deleted) | read (repo operation state for writes) |
| crates/vr-git/src/write.rs (deleted) | read (test list; Writer) |
| crates/vr-git/tests/common/mod.rs | read |
| crates/vr-git/tests/security.rs | read |
| crates/vr-git/tests/read.rs | read (test lists; replaced tests compared with base) |
| crates/vr-git/tests/worker.rs | read |
| crates/vr-git/tests/bench.rs | read |
| crates/vr-git/tests/write.rs (deleted) | read (test list) |
| crates/vr-git/tests/write_security.rs (deleted) | read (test list + every read-API use) |
| crates/vr-git/tests/patch_props.rs (deleted) | read (test list) |

## Deleted tests

| Deleted | Verdict |
|---|---|
| write.rs (26 tests), patch_props.rs (4), unit tests in write/patch/recovery/programs/discard_faults | behaviour removed (stage, unstage, hunk/line patches, discard + recovery, commit, amend) |
| write_security.rs (19) | behaviour removed. Read-path overlap checked: `a_readers_shutdown_never_kills_a_writer` used `status()` only to set up the writer case; reader shutdown is still covered by `shutdown_kills_a_running_read_and_refuses_new_ones` and `dropping_the_worker_kills_its_running_git`. `path_of_another_repo_is_refused` is write-side; the read OutsideRepo case is covered by worker.rs `errors_are_delivered_not_panics` (LineChanges on /etc/passwd). GPG_TTY and editor env stay unit-tested in cmd.rs. |
| security.rs `blame_of_a_staged_file_over_the_cap_is_too_large` | covered: `file_over_the_cap_is_too_large` (inline_blame and line_changes branches) |
| security.rs reads in read_everything / snapshot / partial clone / lazy fetch | re-expressed on head/ahead_behind/inline_blame/line_changes. Both partial-clone tests now `read-tree HEAD` so the index points at missing blobs. `partial_clone_never_fetches_lazily` asserts `is_err` and an unchanged pack listing, which is a strong oracle. The ext:: remote serves nothing, so read-tree cannot download blobs and the lazy-fetch test still meets missing blobs. |
| security.rs submodule diff-files step | behaviour removed (diff_unstaged); see finding 1 |
| diff.rs `hunk_lines_carry_old_and_new_numbers`, `header_split_for_mode_only_change_with_spaces`; sanitize.rs `userinfo_is_redacted` | behaviour removed (diff view parser, push/pull URL redaction) |
| read.rs status/diff/log/commit/revision/fingerprint tests | behaviour removed |
| read.rs unborn/blame/conflicted/symlink tests | replaced (kept API). The symlink test lost its diff-of-retargeted-link half, which only exercised diff_unstaged. |
| worker.rs Status/Log/Commit/DiffUnstaged uses | replaced with Head/AheadBehind/InlineBlame/LineChanges (see finding 2) |
| bench.rs status/log/blame timings | behaviour removed; inline blame (AC-16e < 1 s) and gutter kept |

## Rule check FR-20

| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | worktree changes only vr-git removal, two vr-config comments, lock line; keymap/config.rs differences are master-side |
| BASE-2 | pass | `line_changes`, `inline_blame`, `head`, `ahead_behind`, `relative` signatures and bodies unchanged (line_changes identical to base; inline_blame = old `blame_buffer(.., user=false)`); worker invalidation keeps Head/AheadBehind repo-wide |
| BASE-6 | pass | English; no bug-history comments (stale comments: finding 3) |
| BASE-8 | pass | Cargo.lock: only `"tracing"` removed from vr-git's dependency list; no new dependency |
| BASE-9 | pass | read profile unchanged: driver keys always blanked, FIXED_CONFIG intact (fsmonitor, hooksPath=/dev/null, pager, diff.external, protocol.allow=never, showSignature), `GIT_ALLOW_PROTOCOL=none`, `--no-pager`, `--literal-pathspecs`, child_env allowlist + FIXED_ENV + TTY hints, timeout/max_output/max_file/max_concurrent, setsid/shutdown kill; removed parts were write/network profiles only |
| BASE-20 | pass | grep of vr-git for tracing/Writer/recovery/Cancel/user_timeout/status(): no leftover code; only stale comments (finding 3) |
| BASE-25 | pass | notes hold `cargo fmt --all --check && cargo clippy ... -D warnings && cargo build --workspace` clean; my vr-git run green |
| BASE-32 | pass | D310 honoured (write side and popup-only reads gone, gutter/inline blame/branch kept); D305 moot with the write side; M7-1 notes: core.autocrlf bool parsing kept in `eol_action`, shutdown shared per Repo and tested (with_own_runner removed together with the writer) |
| Verdict-gate 14 | pass | every deleted test maps to removed behaviour or a covering test (table above) |
| AC-F4 (vr-git part) | pass | stage/discard/commit/amend/push/pull code and API removed; kept read API tested (read 13, security 26, worker 4) |

No background task of mine is still running.

APPROVE

## Re-check QP1

Scope: the QP1 changes to security.rs, worker.rs, cmd.rs, lib.rs and README.md (Git section). The developer's log `qa-artifacts/FR-20/qp1_test.log` shows unit 7, read 13, security 26 and worker 4 passed.

| Earlier finding | Status | Evidence |
|---|---|---|
| 1 Guards no kept read reaches | fixed | Comments at security.rs:88, :147 and :586 say these tests guard future reads. The submodule control now runs `git status` inside `sub`. That proves the hostile `filter.y.clean` is armed, and it is deterministic because the stale-stat file is always rehashed. The read assertion is unchanged. As before, removing `diff.ignoreSubmodules=dirty` today fails no test, because no kept read reaches it. The new control does not change that, and it did not weaken the read assertion. The test still fails if a future read added to `read_everything` runs the submodule's filter. One limit: the control now shows the filter is armed, but no longer that a status run from the superproject reaches it. That status run only reached it some of the time, which is exactly what made the old control flaky, so this is accepted and not a finding. |
| 2 Worker repo-wide drop not asserted | fixed | worker.rs:176-184 requests Head before and after `invalidate_path(b.txt)` and asserts `!Arc::ptr_eq`. The developer reports this assertion failed with the drop at worker.rs:260 disabled. |
| 3 Stale cmd.rs comments | fixed | cmd.rs:93-95 and :110 are reworded as defence in depth. |
| 4 lib.rs over-long doc lines | partly fixed | lib.rs:50 is rewrapped. lib.rs:36 is still 157 columns ("[`sanitize_line`]. Even so, the UI runs every error text ... including `Failed` from a read and any field it reads"). This is minor and cosmetic; rewrap that line at 100. |
| README git-recovery note | fixed | The README Git section says recovery copies in `~/.local/state/vr-ide/git-recovery` are no longer listed and can be deleted by hand. |

No new defect. lib.rs:6-8 still says the CLI "is complete for blame/log --follow/status". That text is unchanged and describes the original reason for the choice, so it is not a finding.

No background task of mine is running.

APPROVE
