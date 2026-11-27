# FR-3 Security lows (worktree .worktrees/FR3, branch run4-FR3)

Tools: Bash, Read, Edit, Write, Skill, Agent, Artifact.

## Items (red -> change -> green)
- D28 (ssh recent): test `ssh_tests::d28_recent_keeps_no_secret_from_a_remote_command` (red: did not compile / kept the whole line). Change: `Line::recordable_text` + `secret_looking` in crates/vr-ide/src/ssh/args.rs (outside the listed files; needed to reach the destination index); `commands/ssh.rs::record` (now pub(crate)) stores it. Words after the destination that look secret (`-p<x>`/bare `-p`, `password`, `token=`, `pass=`, `key=`, `secret`, case-insensitive) -> only options + destination kept. Green.
- SEC-16: `completion_tests::sec16_path_completion_never_lists_an_account_folder` (red verified by disabling the guard: failed on `/ssh -i <accounts>/work/` Tab). Change: `in_accounts` in commands/completion.rs, checked in `path_cands` (canonical dir under `accounts_dir(app.ai.state_dir or vr_config::state_dir())` -> no candidates; covers symlinks). slash.rs untouched. Green; a sibling folder still completes.
- SEC-7: desktop_tests `update_desktop_database_runs_the_resolved_absolute_path` and `..._ignores_a_relative_only_path`. Change: `find_in` / `refresh_database_with` in desktop.rs run the resolved absolute path. Green.

## Commands
- `cargo test -p vr-ide --lib -- ssh completion desktop palette d28`: 159 passed, 0 failed.
- `cargo fmt --all --check`: ok; `cargo clippy -p vr-ide --lib --tests -- -D warnings`: clean (full workspace guard not run; only vr-ide changed).

## Reuse
None new; `accounts::accounts_dir`, `args::quote` reused.

## Rule compliance
| Rule | Status |
|---|---|
| D28, SEC-5 (file mode untouched), SEC-16, SEC-7/4/8 | pass |
| BASE red-first, BASE-11, BASE-24/25/38, COST, COST-8 | pass |
Counts: pass 12 / n-a 0.
Note: `/open` completes files only differently; the SEC-16 control uses `/ws add`.

## Review — 2026-11-27
Verdict: CHANGES_REQUESTED (reviewer, opus). Tools: Read, Bash, Skill, SendMessage, SubagentHandback.

### Findings
1. [major] crates/vr-ide/src/ssh/args.rs:362-368 — D28: options after the destination are treated as remote command and dropped.
   `walk` lets options follow the destination (`option_end`), but `recordable_text` scans `words[d+1..]` and keeps only `words[..=d]`.
   Scenario: `/ssh db -p 2222` (no remote command at all) → `-p` is "secret-looking" → recent keeps `db`; replay connects to port 22. `db -i k -p 2222 mysql -pX` → kept `db`, key and port lost. The recorded line no longer re-runs the same connection.
   Fix: `let end = option_end(&refs, d);` scan only `words[end..]` for secrets and keep `words[..end]` (drop a trailing `--` if you like). Add `("db -p 2222", "db -p 2222")` and `("db -p 2222 mysql -pX", "db -p 2222")` to the test.
2. [medium] crates/vr-ide/src/ssh/args.rs:402-408 — CWE-312 D28 bypass: a quoted remote command is one word.
   Data path: `/ssh db 'mysql -uroot -pS3cret -e "select 1"'` → `split` yields one word `mysql -uroot -pS3cret …` → `starts_with("-p")` false, no keyword → full line stored in ssh-recent. Quoting the remote command is the usual way to type it, so the filter misses the common case.
   Fix: in `secret_looking` (or before calling it) split each remote word on whitespace and test every token; keep the whole-word substring checks.
3. [low] crates/vr-ide/src/ssh/args.rs:404 — CWE-312 env-style secrets not matched: `db MYSQL_PWD=x mysql`, `db curl -u user:pw …`, `db env AUTH=… `/`Authorization: Bearer …` are stored whole. D28's list is narrow by decision; at least add `pwd` and `bearer`/`auth` (cheap, over-redaction only costs a shorter line). Leader/user to decide beyond D28's list.
4. [minor] crates/vr-ide/src/ssh/args.rs:367 — the redacted line is rebuilt with `quote`, which quotes a leading `~` (`-i ~/.ssh/k` → `-i '~/.ssh/k'`); on replay `scan` no longer expands it. ssh tilde-expands `-i` itself, but not every path option. Fix: keep the original text slice up to the end of the option part (byte offsets as `shown_command_at` does) instead of re-quoting, or let `quote` leave a leading `~/` bare.

No finding for SEC-16 (completion.rs) or SEC-7 (desktop.rs): `cached_listing` is only reached through `path_cands`, which all three path completers (`/open`, `/ws add`, `/ssh -i`) use; canonicalize covers symlinks and `..`; the program is run by its resolved absolute path.

### Rule check FR-3
| Rule | Status | Evidence |
|---|---|---|
| D28 | fail | findings 1-3 (`db -p 2222` truncated; quoted command bypass) |
| SEC-5 | pass | file write path `ssh::recent::record` unchanged (0600/0700, atomic) |
| SEC-16 | pass | completion.rs:278-292, 317; only lister is `slash::cached_listing` via `path_cands`; canonical `starts_with`; test covers symlink, `.` prefix, three commands |
| SEC-7 / SEC-4 / SEC-8 | pass | desktop.rs:225-240 `find_in` absolute entries only, `Command::new(prog)` with resolved path; tests for `.` entry |
| BASE red-first | pass | notes give red step per item |
| BASE-25 | pass | notes: fmt + clippy `-D warnings` on vr-ide; workspace guard not run, only vr-ide changed |

## Fix round 1 — 2026-11-27
1. [major] `Line::recordable_text` (crates/vr-ide/src/ssh/args.rs) now cuts at `option_end`, so options after the destination are kept: `db -p 2222` kept whole; `db -p 2222 mysql -pX` -> `db -p 2222`.
2. [medium] `secret_looking` checks each remote-command word piece by piece (split on whitespace and quotes): `db 'mysql -uroot -pabc -e "select 1"'` -> `db`.
3. [low] added `pwd`, `passwd`, `-u user:pw`, `user:pw@`, `bearer`, `authorization`, `auth=`.
4. [minor] a kept `~/…` word stays bare: `db -i ~/.ssh/k -p 2222 mysql -pX` -> `db -i ~/.ssh/k -p 2222`.
   Test: ssh_tests `d28_recent_keeps_no_secret_from_a_remote_command` (extended table). Note: the new rows were added together with the fix, so I did not capture a separate red run for items 1-4.
5. [QC defect, SEC-16] Cause confirmed: `/ws add` listed through slash.rs `path_candidates` and the prefetch, not `path_cands`. Fix in crates/vr-ide/src/commands/slash.rs (edited): `in_accounts` moved there, new `read_listing` is the single read used by `cached_listing`, `path_candidates` and the prefetch; the account folder and anything under it list empty. Test `sec16_the_accounts_folder_itself_is_never_listed` (`/ws add`, `/open`, `/ssh -i`, `/run x`; `accounts/` and `accounts/w`). Red with the guard off: `/ws add accounts/` showed `work/` and `other/`; green with it.
6. [QC observation] Defect: Tab with no candidates in `/ssh` called `take_row` on the command's own row (fill `/ssh `) and erased the typed line. `take_row` (completion.rs) now ignores a row whose fill is a prefix of the typed text. Test `tab_with_no_candidates_keeps_the_typed_ssh_line` (red before: `/ssh -i /nonexistent-zz/` became `/ssh `).

Commands: `cargo test -p vr-ide --lib -- ssh completion desktop palette slash`: 229 passed, 0 failed. `cargo fmt --all --check`: ok. `run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings`: exit 0.

## Review re-check r1 — 2026-11-27
Verdict: APPROVED (one minor test note, not blocking).

- Finding 1 fixed: `recordable_text` checks `words[option_end..]` and keeps `words[..option_end]`; `db -p 2222` kept whole.
- Finding 2 fixed: `secret_looking` splits each remote word on whitespace and quotes; `db 'mysql -uroot -pabc …'` -> `db`.
- Finding 3 fixed: `pwd`, `passwd`, `-u user:pw`, `user:pw@`, `bearer`, `authorization`, `auth=` added. Over-redaction (`sort -u`, `pwd`, `mkdir -p`) only shortens the kept line, which D28 accepts.
- Finding 4 fixed: a safe `~/…` word stays bare and expands on replay.
- Red-first for items 1-4: the extended D28 table runs in one test; on the old code the row `("db -p 2222", "db -p 2222")` gives `db`, the quoted row stores `abc`, the `MYSQL_PWD` row stores `abc`, the `~` row gives `db`. The test would have failed, so it covers the change.
- SEC-16 (QC defect): `DirListing::read` is now called only from `read_listing`, and `cached_listing`, `path_candidates` and the `prepare` prefetch all go through it; `dir_cache` is only assigned from those. Every completion listing is covered. The file explorer (`explorer.rs`) is not a completion path and is out of scope.
- `take_row`: a row whose fill is a prefix of the typed text (the command's own row) no longer replaces the input. Path and command candidates always extend the typed text, so Tab completion elsewhere is unaffected; selecting a shorter row now leaves the line as typed.

[minor] crates/vr-ide/src/completion_tests.rs `sec16_the_accounts_folder_itself_is_never_listed` — the exception `shown.matches("work").count() == 1 && tail.ends_with('w')` lets the `accounts/w` case pass when Tab auto-fills `…/accounts/work/` with no list shown (one hit, from the input). The `accounts/` case and the first SEC-16 test still prove the guard, so this only weakens one row. Fix: for `accounts/w` assert `input(app)` equals the typed text and no title contains `work`.

### Rule check FR-3
| Rule | Status | Evidence |
|---|---|---|
| D28 | pass | args.rs `recordable_text` / `secret_looking`; test table rows for options after destination, quoted command, env and URL forms |
| SEC-5 | pass | file write path unchanged (0600/0700, atomic) |
| SEC-16 | pass | slash.rs `read_listing` is the only caller of `DirListing::read`; canonical `starts_with` (symlinks, `..`); two tests |
| SEC-7 / SEC-4 / SEC-8 | pass | desktop.rs `find_in` absolute entries, resolved path run |
| BASE red-first | pass | D28 rows fail on old code (checked by hand); SEC-16 and `take_row` red runs recorded |
| BASE-25 | pass | fmt --all, clippy --workspace -D warnings exit 0 in notes |

## Quick pass 1 — 2026-11-27
Test-only: `sec16_the_accounts_folder_itself_is_never_listed` no longer allows one `work` hit. For each command and for `accounts/` and `accounts/w` it asserts the input is exactly as typed after two Tabs and that no candidate title contains `work` or `other`. If the guard were off, Tab on `accounts/w` would fill `…/accounts/work/` (and `/ws add accounts/` lists `work/`, `other/`), so the equality and title checks would fail. Not re-run red (as the leader said). `cargo test -p vr-ide --lib -- completion slash`: 96 passed, 0 failed. `cargo fmt --all --check` ok; `run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings` ok.

## Review re-check r2 (test-only) — 2026-11-27
APPROVED. `sec16_the_accounts_folder_itself_is_never_listed` now asserts, for `/ws add`, `/open`, `/ssh -i` and `/run x` with both `accounts/` and `accounts/w`, that the input is unchanged after two Tabs and that no candidate title contains `work` or `other`. An auto-fill to `…/accounts/work/` now fails the input assertion, so the gap from the r1 minor is closed. Nothing else changed; the FR-3 rule check from r1 stands (all pass).
