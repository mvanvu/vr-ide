# Review M7-1 — vr-git read API + runner (code + security)

Worktree `.worktrees/M7-read` (branch m7-read, uncommitted). Reviewer tools: Bash, Read, Write, Skill, SendMessage, SubagentHandback.

**Verdict: CHANGES_REQUESTED** (security: ISSUES_FOUND — 2 high, 2 medium, 1 low; code: 1 major)

`cargo test -p vr-git` (run-heavy.sh): 7 + 18 + 20 + 4 passed, 0 failed, 1 ignored (bench). Hostile repos tried by hand
(scripts in the reviewer scratchpad; they copy the runner's env and forced config, git 2.43.0):
1. Partial clone with `remote.origin.url=ext::sh -c touch PWNED` + `protocol.ext.allow=always` in repo config → with
   `GIT_NO_LAZY_FETCH` removed, `diff-tree -p` and `blame` **run the command** although `protocol.allow=never` is forced.
   With `GIT_ALLOW_PROTOCOL=none` → "transport 'ext' not allowed", nothing runs.
2. Superproject + submodule whose own config has `filter.y.clean=<marker>` and `.gitattributes * filter=y`, file in
   the submodule touched → `status --porcelain=v2` and `diff-files -- sub` **run the marker**; with
   `--ignore-submodules=dirty` nothing runs.
3. Odd paths, separator bytes in messages, outer GIT_* env, index.lock, hostile output config: covered by the
   existing tests, all green; I did not find a parser break.

## Findings

[high] crates/vr-git/src/repo.rs:270 (status), crates/vr-git/src/diff.rs:155 (diff-files) — CWE-78 submodule-local filter driver runs on an automatic read (SEC-3 fail)
Data path: `.git/modules/<sub>/config` `filter.y.clean` + the submodule's `.gitattributes` → git spawns `git status` in the submodule to check for dirty content → clean filter runs. `driver_config` (cmd.rs:250) only lists drivers visible from the superproject, so they are never blanked.
Exploit: a repo shared as a tarball / container volume (A2) with a submodule; the user opens any file → the automatic status runs the program. Reproduced (case 2).
Fix: pass `--ignore-submodules=dirty` to `status` and `diff-files` (and force `diff.ignoreSubmodules=dirty` in FIXED_CONFIG); the submodule's commit change is still listed. Add a marker test with the filter defined only in the submodule config.

[high] crates/vr-git/src/cmd.rs:83 — CWE-829 `protocol.allow=never` is overridden by `protocol.<name>.allow` from repo config; lazy fetch runs `ext::`/ssh commands (SEC-3 fail on git without GIT_NO_LAZY_FETCH)
Data path: repo config `protocol.ext.allow=always` + promisor remote `ext::sh -c ...` (or `core.sshCommand`, `remote.*.uploadpack`) → a read needs a missing blob → lazy fetch → command runs. git checks `protocol.<name>.allow` before `protocol.allow`.
Exploit: reproduced (case 1). Today only `GIT_NO_LAZY_FETCH` stops it; that variable does not exist before 2.39.4 / unpatched 2.40–2.45.0, which the threat model explicitly says the design must tolerate ("harmless before"). `partial_clone_never_fetches_lazily` passes only because of that variable.
Fix: set `GIT_ALLOW_PROTOCOL=none` for every read (env wins over all protocol config, supported since 2.12); M7-2 push/pull will need a per-call override, so make it part of the read profile rather than FIXED_ENV for all calls. Extend the partial-clone test with `protocol.ext.allow=always` + an `ext::` marker URL and run it once with `GIT_NO_LAZY_FETCH` stripped (shim) to prove the second layer.

[medium] crates/vr-git/src/blame.rs:93 — CWE-400 unbounded read of the worktree file (SEC-7 fail)
Scenario: a staged new file (not in HEAD) of 2 GB, cursor in it → inline blame fails fast in git, then `std::fs::read` loads the whole file on the worker to count lines → RSS +2 GB, no TooLarge. `untracked_diff` checks the size; this path does not.
Fix: `symlink_metadata` + `len() > max_file` → `TooLarge` (same as diff.rs:288-297), or count lines from a bounded read.

[medium] crates/vr-git/src/cmd.rs:367-426, worker.rs:135 — SEC-1 "killed on cancel or vr-ide exit" not implemented
Scenario: vr-ide quits while a git read hangs (slow NFS, stuck helper). The child is in its own session (setsid), the worker thread that enforced the timeout is gone → the git process stays forever. There is also no cancel path for a running request (only queued ones are superseded).
Fix: keep the live process-group ids in the `Git` inner (or a crate-level registry) and kill them in `GitWorker`'s `Drop` and in a public `shutdown()`; the app calls it on exit (M7-3 may do the call, but the library must expose it). A test: drop the worker while a sleeping shim runs → the group is gone.

[low] crates/vr-git/src/blame.rs:86 — automatic inline blame uses the 30 s user timeout (SEC-7 says automatic <= 10 s; `Options::timeout` doc lists "inline blame" as automatic)
Fix: a `user: bool` on `blame`/`blame_text` (or two entry points), auto for inline blame.

[major] crates/vr-git/src/diff.rs:275-279 — gutter compares the buffer with the raw index blob, ignoring eol/ident/working-tree-encoding (AC-16a)
Scenario: repo with `*.bat eol=crlf`, or `* text=auto` and a file with CRLF; vr-core keeps CRLF in the buffer (document.rs LineEnding). Index blob has `a\n`, buffer `a\r\n` → every line marked Modified while `git diff` shows nothing. `blame_text` is fine (git converts `--contents`).
Fix: fetch the base in worktree form — `git cat-file --filters --path=<rel> <oid>` (drivers are already blanked, so this only applies eol/ident/encoding) — or normalise CRLF on both sides when the file is `text`. Add a test with `eol=crlf`.

Notes (no change requested): `git_has_no_controlling_terminal` is vacuous without a tty (dev notes say so; the group-kill test covers setsid). The worker's stamp does not cover `refs/remotes/*`, so ahead/behind stays stale after a terminal `git push` until the app's watcher calls `invalidate()` on `refs/` (SEC-8 watcher, M7-3). `Repo::discover` is synchronous (two git runs): the app must call it off the UI thread.

## Files read
| File | |
|---|---|
| Cargo.toml, Cargo.lock | read (vr-git entry only, no new external crate; no audit needed) |
| crates/vr-git/Cargo.toml | read |
| crates/vr-git/src/{lib,cmd,parse,repo,diff,log,blame,worker}.rs | read |
| crates/vr-git/tests/security.rs | read (helpers, SEC-1/2/3/6/7 tests, partial clone, config snapshot) |
| crates/vr-git/tests/common/mod.rs | read (first 80 lines: fixture env) |
| crates/vr-git/tests/read.rs, worker.rs, bench.rs | skimmed via notes; ran |

## Rule check M7-1
| Rule | Status | Evidence |
|---|---|---|
| SEC-1 | fail (partial) | single spawn in cmd.rs, no shell, absolute git skipping relative/empty PATH (unit + `./git` test), stdin null, setsid, group SIGKILL on timeout/cap and after exit — pass; kill on cancel / vr-ide exit missing (finding medium) |
| SEC-2 | pass | `child_env` allowlist + fixed vars; repo only via current_dir; `outer_git_env_is_ignored` green. HOME/XDG/GIT_CONFIG_GLOBAL pass through (intended); no locale text is parsed, so LANG/LC_ALL need no forcing |
| SEC-3 | fail | local+global marker tests green, but submodule-local drivers run (high) and `protocol.<name>.allow` overrides `protocol.allow=never` (high) |
| SEC-4 | pass | porcelain v2 -z, plumbing diff with fixed prefixes, `%x00` formats with free text last, blame --porcelain, forced output config; hostile-config snapshot test green |
| SEC-5 | pass | `--literal-pathspecs` + env, `--` before paths, `--end-of-options`, `resolve_commit` via rev-parse --verify, `is_oid`, raw-byte paths; `odd_paths_round_trip` green |
| SEC-6 | pass | git's own discovery/ownership, no safe.directory, GIT_DISCOVERY_ACROSS_FILESYSTEM filtered, toplevel-ancestor check, `is_parent_repo_of` |
| SEC-7 | fail | timeouts, 64 MiB / 8 MiB caps, 2 slots, coalescing pass; blame fallback reads unbounded (medium); inline blame on user timeout (low) |
| SEC-8 | pass | GIT_OPTIONAL_LOCKS=0, plumbing diff instead of porcelain `git diff`; `.git` listing unchanged and index.lock respected (test green) |
| BASE-3 | pass | reds recorded (SEC-8, SEC-4, mutation reds) |
| BASE-4 | pass | test/clippy/fmt output in notes; tests re-run green |
| BASE-8 | pass | no new external crate; rustix feature only |
| BASE-16 | pass | similar/rustix reused; capabilities.md updated |
| AC-16a | fail | CRLF / eol attributes (major) |

## Round 1 re-check

**Verdict: APPROVED** (all six round-0 findings fixed; 1 new minor, non-blocking)

`cargo test -p vr-git` (run-heavy.sh): 7 + 19 + 27 + 4 passed, 0 failed, 1 ignored (bench).
Hostile runs (git 2.43.0), reviewer scratchpad `h5.sh` = the runner's new env/config (GIT_ALLOW_PROTOCOL=none,
diff.ignoreSubmodules=dirty, rest of FIXED_ENV/CONFIG); every case has a plain-git control that does run the marker:
- Lazy fetch from a blob:none partial clone, three transports: `ext::` + `protocol.ext.allow=always`; local-path
  remote + `remote.origin.uploadpack=<marker>` + `protocol.file.allow=always`; `ssh://` + `core.sshCommand=<marker>`
  + `protocol.ssh.allow=always`. diff-tree -p, blame, log -p, cat-file, ls-files: nothing runs, with and without
  GIT_NO_LAZY_FETCH. `network: true` is unreachable (only `Limits::new` builds Limits, always false; field pub(crate)).
- Submodule-local `filter.y.clean/process` + `diff.z.textconv`, superproject `submodule.sub.ignore=none`: status and
  diff-files with `--ignore-submodules=dirty` -> nothing runs. Without the flag they run even with
  `diff.ignoreSubmodules=dirty` forced (per-submodule `ignore` beats it), so the FIXED_CONFIG entry is not a second
  layer; the flags are the defence. Other descending reads: vr-git's only diff-index calls are `--cached` (no
  worktree, nothing runs); log, blame, diff-tree, ls-files, check-attr do not descend (verified).
- Blame: size refused before git for a regular worktree file; fallback read via symlink_metadata (FIFO/socket -> None,
  no blocking open; symlink -> target text, as git) and `take(max+1)`. git itself also refuses non-regular files.
- Process registry: closed-check, spawn, then register under the `live` lock with a re-check, so a spawn that loses
  the race is killed; acquire() waiters get ShutDown once slots free. Tests kill a hung shim within 2 s.
- inline_blame: worker `Request::InlineBlame` -> `Repo::inline_blame` -> `git.auto()`; test proves the 300 ms limit.
- CRLF: eol_action follows convert.c (text, then legacy crlf, eol attr -> text unless binary/auto, then
  core.autocrlf); auto_binary matches gather_stats/convert_is_binary; core.eol and core.safecrlf do not change the
  clean output (diff only warns); mixed endings convert pair by pair as git does. One mismatch below.

### Findings

[minor] crates/vr-git/src/diff.rs:337-342 — `core.autocrlf` read as raw text, so git-bool spellings other than
true/yes/on/1 are taken as "off"
Scenario (verified with git 2.43, scratchpad `h6.sh`): `[core] autocrlf` (bare key; `--get` prints an empty line),
`autocrlf = 2` or `-1` -> git converts CRLF (`git diff` empty); vr-git returns Keep -> a CRLF buffer marks every line
Modified.
Fix: treat `input` (case-insensitive) first, else ask `git config --type=bool --get core.autocrlf` and convert on
`true`.

Notes (no change requested): (1) read.rs:656 says "text=auto keeps CRLF already in the index" but the case has no
attributes and no autocrlf (Keep path); the Auto + CRLF-in-index branch (diff.rs:293) has no test (commit CRLF first,
then add `* text=auto`). (2) Two git nuances not mirrored, both edge-only: has_crlf_in_index ignores an index blob git
deems binary (lone CR), and gather_stats discounts a trailing `\032`. (3) For M7-2/M7-3: shutdown is permanent and
shared by every clone of the Repo, so dropping a GitWorker also kills an in-flight M7-2 write run through a clone; a
killed commit can leave `index.lock`. Give writes their own Repo/runner, or call shutdown only on exit/workspace close
after writes finish. (4) `ident`, working-tree-encoding and LFS-style clean filters still make gutter marks differ
from `git diff` (documented in the dev notes).

### Files read (round 1)
| File | |
|---|---|
| crates/vr-git/src/cmd.rs | read (whole) |
| crates/vr-git/src/blame.rs | read (whole) |
| crates/vr-git/src/diff.rs | read 1-520 (all changed code: diff-files flag, line_changes, clean_eol, eol_action, auto_binary) |
| crates/vr-git/src/repo.rs | read shutdown, status, run/run_code, index_entries |
| crates/vr-git/src/worker.rs | read 100-240 (InlineBlame wiring, shutdown, Drop) |
| crates/vr-git/src/lib.rs | grep (ShutDown variant) |
| crates/vr-git/tests/security.rs | read 650-819 (new tests) |
| crates/vr-git/tests/read.rs | read 627-690 (CRLF cases) |

## Rule check M7-1 (round 1)
| Rule | Status | Evidence |
|---|---|---|
| SEC-1 | pass | + live process-group registry, race-safe registration (cmd.rs:368, 410-420), Repo/GitWorker::shutdown, Drop; tests `shutdown_kills_a_running_read_and_refuses_new_ones`, `dropping_the_worker_kills_its_running_git` green. Per-request cancel of a running read not offered (shutdown covers exit/close) |
| SEC-2 | pass | unchanged |
| SEC-3 | pass | GIT_ALLOW_PROTOCOL=none on every call (cmd.rs:387-390), network opt-in unreachable; ext / file+uploadpack / ssh lazy fetch blocked with and without GIT_NO_LAZY_FETCH (h5.sh, controls run); `--ignore-submodules=dirty` on status + diff-files, submodule-local drivers do not run (h5.sh); tests green |
| SEC-4 | pass | unchanged |
| SEC-5 | pass | unchanged; new check-attr call has `--` before the path |
| SEC-6 | pass | unchanged |
| SEC-7 | pass | blame bounded before git and in the fallback, FIFO/symlink safe; inline blame on the automatic timeout (test) |
| SEC-8 | pass | unchanged; no new index-writing command |
| AC-16a | pass (minor) | 11 cases + `*.bat eol=crlf` equal `git diff -U0`; autocrlf spelling gap (minor) |
| BASE-3 / BASE-4 | pass | reds recorded in the dev notes; tests re-run green |
