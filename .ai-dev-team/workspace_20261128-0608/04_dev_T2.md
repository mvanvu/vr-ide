# T2 Shared history between accounts
Task base: 88d838e0bb1b7411423963cd0ed143d7f7e8fe2c (brief). Side: be.

## Progress
- read: brief, spec "Shared history", plan Task 2, threat model SEC-19/20/21/38, T-24..T-27, T-48, amended T-20/SEC-16, decisions D1; accounts.rs, ai/mod.rs (spawn_session/start_program/restart/add_account), app.rs load_workspace, spawn.rs, changes.rs exclusion.
- red: `cargo test -p vr-ide history::` with stub bodies -> `test result: FAILED. 4 passed; 12 failed` (the 4 passing are refusal tests the stub's constant NotShared satisfies; cross_filesystem_is_not_shared ran for real: /dev/shm is another device here).
- code: history.rs implemented; accounts.rs `hold_exclusive` (shared body `take_hold` with `hold`). green: `cargo test -p vr-ide history::` -> `test result: ok. 16 passed; 0 failed`.
- wiring red: `cargo test -p vr-ide named_sessions_share` -> `error[E0609]: no field `shared` on type `ai::AiSession`` (2 errors); green after mod.rs/app.rs wiring -> `test ai_account_tests::named_sessions_share_the_tools_history ... ok`.
- related + guard runs (below). Nothing started in the background.

## Files changed
- `crates/vr-ide/src/ai/history.rs` (new): `entry`, `shared_dir(tool, home, claude_config_dir, codex_home)` (D1 signature), `dirs`, `stray_vars`, `configure`, `Shared`, `ensure` (+ private `ensure_bounded` for bound tests), 16 tests.
- `crates/vr-ide/src/ai/accounts.rs`: `hold_exclusive` (non-blocking exclusive account lock, error `"in use"`), `hold` and it share `take_hold` (same identity re-check after the lock); module doc names the one history entry exception.
- `crates/vr-ide/src/ai/mod.rs`: `pub mod history`; `AiPanel::history_dirs`; `AiSession::shared`; `share_history()` called by `spawn_session` before the account hold (picker New/Resume, add account, `/ai <tool> @acc`, restore all go through it); `start_program` removes `history::stray_vars` from the agent env (SEC-38).
- `crates/vr-ide/src/app.rs` (not in the board row, see deviations): `load_workspace` calls `history::configure` with `$HOME`, `CLAUDE_CONFIG_DIR`, `CODEX_HOME` of vr-ide's own environment.
- `crates/vr-ide/src/ai_account_tests.rs`: `named_sessions_share_the_tools_history`.
- `README.md`: Accounts section — history shared, logins not; when an account stays "not shared"; `chmod 700` hint.
- `.ai-dev-team/capabilities.md`: one line.

## Tests added
history.rs: new_account_gets_link, correct_link_is_kept, merge_moves_nested_files, name_clash_is_not_shared, link_elsewhere_is_refused_and_kept (symlink, regular file, FIFO), codex_shared_dir_honours_codex_home, busy_account_is_skipped, cross_filesystem_is_not_shared (real: /dev/shm dev 27 vs /tmp dev 66306 here; skips with a stderr line where both are one device), link_inside_entry_is_not_shared, shared_folder_open_to_others_is_not_shared (0755/0750/0703, mode unchanged), shared_folder_inside_accounts_is_refused (other account, own entry, `..`, link to state), swapped_subdir_is_not_followed (seam), destination_created_meanwhile_is_kept (seam), bounds_stop_before_moving, inherited_vars_into_accounts_are_ignored, shared_dirs_are_excluded_from_change_recording.
ai_account_tests.rs: named_sessions_share_the_tools_history (merge + link on start, file entry → warning + unshared + untouched, default shared, tool without shared folder unshared, second session on a linked account).

## Commands run (final code)
```
cargo test -p vr-ide history::      -> test result: ok. 16 passed; 0 failed; ... 969 filtered out
cargo test -p vr-ide accounts::     -> test result: ok. 12 passed; 0 failed
cargo test -p vr-ide ai_            -> test result: ok. 82 passed; 0 failed
cargo test -p vr-ide ai::           -> test result: ok. 87 passed; 0 failed
cargo test -p vr-ide completion_tests -> test result: ok. 22 passed; 0 failed   (sets state_dir; SEC-16 listing tests)
cargo fmt --all --check             -> (no output) FMT-OK
$TEAM_BIN/run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings
                                    -> Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.12s
```
(first clippy run: `clippy::type_complexity` on the test seam's thread_local → `type Seam` alias; rerun clean.)

## Deviations from the plan (with reason)
- `accounts::create` does not call `ensure`: it would need the shared folder from the process environment, so every existing `accounts::create` test would link to (and might create) the real `~/.claude/projects`. Instead `add_account` → `spawn_session` runs `share_history` right after `create`, so a new account is linked before its first session (same effect, covered by the integration test).
- `start_program` does not call `ensure`; `spawn_session` does, before `accounts::hold`, because the migration needs the account lock exclusively (SEC-20) and the session's own shared hold would make it "in use". `restart` (Enter on an exited tab) keeps the session's `shared` flag and does not retry: its own hold is still held.
- `shared_dir` takes `claude_config_dir` (D1). `ensure` keeps the plan's `(account_dir, tool, shared)` and derives state/name from `account_dir` (validated: parent must be `accounts`, name passes `accounts::validate`).
- `app.rs` touched (2 statements) — the only place the state folder, `changes_excluded` and the env are set at startup; tests never call `load_workspace`, so tests never compute the real `~/.claude`.
- Migration is two passes: a check pass (links/specials, clashes, bounds, depth) that moves nothing, then the move pass re-checking each step on descriptors. Why: with one pass, files moved before a clash would vanish from the unshared account's own `/resume`.

## Decisions (append to decisions.md)
- D9 (T2): a clash, link or bound found anywhere stops the migration before anything moves (check pass first); a race found in the move pass stops there and leaves each file in exactly one place. Why: an unshared account keeps seeing all of its conversations.
- D10 (T2): the link target is the canonical shared path; an existing link is accepted only when byte-equal to it (SEC-19). If the user later moves `~/.claude` behind a symlink, accounts report "links elsewhere" until relinked by hand. confirm-on-return.
- D11 (T2): the shared folder is computed once at startup (`history::configure`) from vr-ide's own `$HOME`/`CLAUDE_CONFIG_DIR`/`CODEX_HOME`; no `$HOME` → nothing shared, no warning.
- D12 (T2): a named account is linked on every session start (spawn_session), not in `accounts::create`; `restart` does not retry.

## Out-of-scope / open items for the leader
- SEC-38 remainder: inherited `CLAUDE_CONFIG_DIR`/`CODEX_HOME` into `<state>/accounts/` are removed only from AI agent spawns (`start_program`) and ignored for the shared folder. Shells (`commands/terminal.rs::spawn_program`), ssh (`commands/ssh.rs::withheld_env`) and `/run` still inherit them, and there is no startup notice; `history::stray_vars(state, std::env::vars_os())` is ready for those call sites (files outside this row).
- SEC-38 wiring has no integration test: it needs the process env changed (`set_var` is unsafe with parallel tests); `stray_vars` is unit-tested and the call is `crates/vr-ide/src/ai/mod.rs` start_program.
- The remove-account dialog (`ai/mod.rs` ask_remove_account) still says "its login, settings and history"; with sharing, a linked account's history stays (only the link goes). Text change belongs with SEC-17/SEC-37 (T8) or a follow-up.
- Real `~/.claude/projects` here is 0700 (stat, read only); a user whose folder is 0755 gets "not shared … run chmod 700" for every named account (SEC-21 as decided).
- Small residual race: a regular file swapped for a symlink between `statat` and `renameat2` moves the link itself into the shared folder (rename never follows, no data moves).

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| account folder checks | accounts.rs | reused `accounts::secure`, `validate`, `accounts_dir`, `ACCOUNTS_DIR`, `var_for` |
| exclusive account lock | accounts.rs `hold`/`remove` | extended: `hold_exclusive` sharing `take_hold` with `hold` |
| private dir creation | `grep create_private_dir` | reused `crate::recovery::create_private_dir` |
| fd-based no-follow walk | accounts.rs `remove_tree_at` | same rustix pattern (`openat` O_NOFOLLOW, `statat` SYMLINK_NOFOLLOW, `Dir::read_from`) |
| no-replace rename | capabilities.md (vr-fs ops, vr-core safe_replace) | rustix `renameat_with(NOREPLACE)` direct: those helpers are path-based, this needs dirfd-relative names |
| display of paths | git.rs | reused `git::home_relative` + `ui::safe_text::sanitize` |
| change-recording exclusion | app.rs / changes.rs | reused `AiPanel::changes_excluded` (tracker canonicalises it) |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| AC-2 | pass | history.rs tests new_account_gets_link, merge_moves_nested_files, name_clash_is_not_shared, link_elsewhere_is_refused_and_kept; ai_account_tests named_sessions_share_the_tools_history |
| SEC-19 | pass | history.rs `link()` opens the account with O_DIRECTORY|O_NOFOLLOW after `accounts::secure`, statat/readlinkat/symlinkat on the fixed `entry`; tests link_elsewhere_is_refused_and_kept (link, file, FIFO no hang); reasons are fixed consts; `grep -nE "std::fs::(rename|remove_|read_dir)" history.rs` → test lines only |
| SEC-20 | pass | `migrate()`: `accounts::hold_exclusive` (busy_account_is_skipped), fd walk + RENAME_NOREPLACE (destination_created_meanwhile_is_kept), O_NOFOLLOW levels (swapped_subdir_is_not_followed, link_inside_entry_is_not_shared), EXDEV/dev check (cross_filesystem_is_not_shared, ran for real), REMOVEDIR only, bounds 200 000/32 (bounds_stop_before_moving), runs before the spawn |
| SEC-21 | pass | `shared_folder()`; tests shared_folder_open_to_others_is_not_shared (mode unchanged), shared_folder_inside_accounts_is_refused, codex_shared_dir_honours_codex_home, new_account_gets_link (created 0700, canonical target) |
| SEC-38 (accounts part) | pass, remainder open | inherited vars into accounts ignored for the shared folder and removed from agent spawns: inherited_vars_into_accounts_are_ignored, mod.rs start_program; shells/ssh//run + startup notice listed above as open |
| SEC-10 ext. | pass | shared_dirs_are_excluded_from_change_recording; app.rs `history::configure` |
| BASE-1 | pass | diff: history.rs, accounts.rs, mod.rs, README, ai_account_tests.rs, app.rs (2 statements, deviation stated) |
| BASE-2 | pass | plan interfaces `entry`, `shared_dir` (D1 signature per brief), `Shared`, `ensure`; `AiSession::shared: bool` |
| BASE-3 | pass | red lines in Progress (12 failed; E0609) before code |
| BASE-4 | pass | Commands run section |
| BASE-5 | pass | clippy via run-heavy.sh; tests light direct per profile; nothing left running |
| BASE-6 | pass | English, why-comments only |
| BASE-7 | pass | no commit, nothing staged |
| BASE-9 | pass | fixed-map names, validated account name, no-follow fd ops, no secrets/names logged |
| BASE-10 | pass | only history::, accounts::, ai_, ai::, completion_tests + guard |
| BASE-11 | pass | no stash/checkout/reset |
| BASE-14 | pass | all commands foreground, no waits |
| BASE-15 | pass | no process stopped/started in background |
| BASE-16 | pass | Reuse table |
| BASE-17 | pass | nothing installed; $TEAM_TOOLS untouched |
| BASE-18 | pass | no agents started |
| BASE-19 | pass | this file, created before the first change, progress lines |
| BASE-22 | pass | names follow neighbours (`hold_exclusive` beside `hold`, `history_dirs` beside `state_dir`) |
| BASE-23 | pass | entry paths: new account (add_account→spawn_session), `/ai @acc`, restore, picker all via spawn_session; test covers `/ai @acc` and a second session; restart path documented (no retry) |
| BASE-24 | pass | no git index commands |
| BASE-25 | pass | guard command exactly as profile: fmt OK, clippy Finished |
| BASE-32 | pass | D1 honoured (`shared_dir` claude follows CLAUDE_CONFIG_DIR), SEC decisions kept |
| BASE-38 | pass | rustfmt only on the 5 changed .rs files |
| BASE-45 | pass | AC-2 text is in 01_spec.md (spec section "Shared history") |
| COST-1 | pass | clippy through run-heavy.sh |
| COST-3 | pass | no sleep/pgrep loops |
| COST-8 | pass | edits with Edit/perl -pi, one Write for the new file |
| COST-9 | pass | nothing left running |
| COST-10 | pass | related set includes ai_account_tests (mentions accounts), completion_tests (state_dir) |
| COST-13 | pass | scratch file only under qa-artifacts/T2 (removed) |
| COST-16 | pass | only summary lines read (grep on test result/errors) |

n-a: BASE-8 (no dependency change) · BASE-12, BASE-29, BASE-40, BASE-41 (no outside system) · BASE-13, BASE-27, BASE-30, BASE-31 (no shared env/containers/data; temp dirs only) · BASE-20 (nothing removed) · BASE-21 (no secrets in output) · BASE-26 (no rewrite) · BASE-28 (own code) · BASE-33, BASE-46, BASE-48 (no runtime env/state on a real machine) · BASE-34 (no blocked step) · BASE-35, BASE-36 (no USER-n/app scope for this area) · BASE-37 (no runtime defect fix) · BASE-39 (no schema) · BASE-42 (no test settings changed) · BASE-43 (no access rule) · BASE-44 (no client/fake adapter) · BASE-47 (no fixed computed value) · BASE-49 (no program usage learned) · COST-2, COST-4, COST-5, COST-6, COST-7, COST-11, COST-12, COST-14, COST-15, COST-17, COST-18, COST-19, COST-20, COST-21 (no UI, browser, e2e, CI, goldens, full suite or qc role in this task)

## Quick pass 1
| # | Finding | Reply |
|---|---|---|
| 1 | shared folder swapped between canonicalize and open (T-26) | fixed: `still_at(path, fd)` in history.rs — `fstat(fd)` dev/ino == `stat(path)` and `canonicalize(path) == path` (no link in any part), checked right after the open in `shared_folder` and again by `linkable()` right before the link is made (absent-entry case before `symlinkat`, migration before the entry's `unlinkat`). Reason "shared history folder … changed meanwhile". Test `swapped_shared_folder_is_refused` (seams `shared` and `link`: parent of the shared folder swapped for a link to an outside folder → NotShared, outside folder empty, account entry still a real folder, each file in exactly one place). |
| 2 | clash text says "conversation" | fixed: `CLASH` = "a file exists on both sides; left in place"; README says "a file exists on both sides"; `name_clash_is_not_shared` asserts the exact text. |
| 3 | no test for a migration finishing on the next start | added `partial_migration_finishes_next_start` (2nd move's destination created by a seam → NotShared; blocker removed; `ensure` again → Linked; `-p` holds exactly a.jsonl "A" and b.jsonl "B"; link correct). Behaviour already existed: passed on its first run (no red possible). |
| 4 | shared hold during a set-up says "being removed" | fixed: `hold`'s busy reason is "account is busy (being removed or set up)" (accounts.rs `hold`); test `a_hold_during_set_up_says_busy` (exclusive hold held → `hold` gives that text, `hold_exclusive` gives "in use"; after drop `hold` works). |

Red (tests written first):
```
test ai::history::tests::name_clash_is_not_shared ... FAILED        (assertion left == right)
test ai::history::tests::swapped_shared_folder_is_refused ... FAILED (panicked: linked)
test result: FAILED. 16 passed; 2 failed            (history::)
test ai::accounts::fix_tests::a_hold_during_set_up_says_busy ... FAILED
test result: FAILED. 12 passed; 1 failed            (accounts::)
```
Green:
```
cargo test -p vr-ide history::          -> test result: ok. 18 passed; 0 failed
cargo test -p vr-ide accounts::         -> test result: ok. 13 passed; 0 failed
cargo test -p vr-ide ai_account_tests   -> test result: ok. 8 passed; 0 failed
cargo fmt --all --check                 -> FMT-OK
$TEAM_BIN/run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings -> Finished `dev` profile in 2.51s
```
Files changed this round: crates/vr-ide/src/ai/history.rs, crates/vr-ide/src/ai/accounts.rs, README.md. Rule rows touched: SEC-20 (+ partial_migration_finishes_next_start), SEC-21 / T-26 (+ swapped_shared_folder_is_refused) — still pass. Nothing left running.
