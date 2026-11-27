# T8 Accounts per agent session (D11) — worktree .worktrees/T8, branch run4-T8 (base c8675ed)

Tools available: Agent, Artifact, Bash, Edit, Read, Write, Skill, ToolSearch, SubagentHandback (+ deferred: TaskStop, Monitor, ...).

## Progress
- read: brief, spec D11/AC-12, threat model SEC-15..18, T5 notes.
- warm build (background, run-heavy) ok.
- code: ai/accounts.rs (validator, folders, secure, ready, env, create, remove + unit tests); `AiPanel.state_dir` (set from `vr_config::state_dir()` in `App::load_workspace`).
- red: ai_account_tests.rs (6 tests) → compile errors: no `Row::Add`, no `Picker.adding`, no `answer_remove_account` (T5 picker has no accounts).
- code: picker rows (default, named accounts, `+ Add account…`), name field, Delete → remove dialog / busy notice, `answer_remove_account`, `/ai <tool> @account`, `spawn::account_env` → `accounts::env`, restore real check, ai_view field/footer/status, DialogAction::RemoveAiAccount, docs (README #### Accounts, commands.md + slash registry text).
- green: `cargo test -p vr-ide -- ai_account ai::accounts ai_tests ai::spawn` → 27 passed (2 T5 tests adjusted for the new `+ Add account…` row).
- rustfmt on changed files; guard running in background.
- guard 1: compile error in ai_view_tests (`picker::` path) → fixed; clippy clean.
- snapshots regenerated (`INSTA_UPDATE=always`): whole crate once, then ai_view/slash once more after the footer rule (`del remove` only with a named account) and keeping the `/ai` description unchanged (the longer one grew the palette list by a row).
- final: guard exit 0; `INSTA_UPDATE=no cargo test -p vr-ide` 738 passed / 11 failed (search/event_loop/watcher timing under load avg ~28, not T8 code) → rerun of those files `--test-threads=2`: 43 passed, 0 failed.

Status: DONE. Nothing staged or committed; nothing running.

## Files changed
- new `crates/vr-ide/src/ai/accounts.rs` (+ 5 unit tests), new `crates/vr-ide/src/ai_account_tests.rs` (6 tests).
- `crates/vr-ide/src/ai/mod.rs`: `AiPanel.state_dir`; `/ai <tool> @account` parsing/validation; `start_program` takes `spawn::account_env(...)?` (covers new, picker, restart, restore); picker name-field keys/paste, Delete → `ask_remove_account` / `account_busy` / `answer_remove_account` / `rebuild_picker`; `add_account`; clicks ignored while the field is open; `REMOVE` button.
- `ai/picker.rs`: `Row::Add`, `AddField`, `FIELD_MAX`, `Picker.adding`, named account rows from `accounts::for_tool`, `add_tool()`, Add rows selectable; `supports_accounts` = `accounts::var_for`.
- `ai/spawn.rs`: `account_env(state, tool, account) -> Result` delegating to `accounts::env`.
- `ai/restore.rs`: real account check (`accounts::ready`), no fallback; session reopened with its account.
- `ui/ai_view.rs`: Add row / name field with rule or error, named accounts in the `mode_ai` purple, footer `del remove` (only with a named account) / `enter create + start  esc cancel`, status `new account`.
- `commands/dialog.rs` `DialogAction::RemoveAiAccount`; `commands/slash.rs` `/ai` arg hint `<tool> [@account]`; `app.rs` sets `app.ai.state_dir` from `vr_config::state_dir()`; `lib.rs` test module.
- tests adjusted: `ai_tests.rs` (row count + one extra Down past `+ Add account…`), `slash_tests.rs` (`/ai needs <tool> [@account]`), `ui/tests/ai_view_tests.rs` (ai_picker with a `work` account; new `ai_account_screens`).
- snapshots: updated `ai_picker_120x40`, `ai_empty_80x24`; new `ai_account_add_120x40`, `ai_account_remove_80x24`, `ai_account_busy_80x24`.
- docs: README `#### Accounts`, docs/commands.md `/ai <tool> [@account]`.
- `.ai-dev-team/capabilities.md` (main tree): accounts line.

## Tests added
- `ai::accounts::tests::{names_are_validated_by_one_rule (SEC-15 table: ../x, /etc, a/b, ., .., A B, a\x1b, é, Default, 33 chars … refused, nothing created; work, alt-2 accepted), variables_are_a_fixed_map, folders_are_private_real_and_checked_before_use (0700, wide 0755/0777 → 0700, symlink account / symlink accounts/ refused, target untouched), listing_and_tools (real valid dirs only, unique across tools, tampered index lines dropped, orphan taken over), remove_deletes_only_the_account (link inside not followed, sibling + accounts/ stay, symlinked account → link only, crafted names refused)}`.
- `ai_account_tests::{picker_groups_accounts_per_tool, add_account_creates_the_folder_and_starts_on_it, sessions_on_accounts_get_their_own_variable_only (two claude accounts side by side, Default/codex unaffected, process env unchanged, /ai refusals, unsafe folder refused at spawn), remove_account_asks_refuses_while_in_use_and_deletes_only_it (plural/singular busy text, default hint, Cancel default, Remove, re-check after the question), restore_reopens_on_the_same_account_or_not_at_all (5 refusal notices, no Default spawn, 0755 → 0700), restart_checks_the_account_again}`.
- `ai_view_tests::ai_account_screens` (+3 snapshots).

## Commands run + result (trimmed)
- red: `cargo test -p vr-ide ai_account` → `error[E0599]: no variant named Add found for enum picker::Row`, `error[E0609]: no field adding on type &Picker`, `error[E0425]: cannot find function answer_remove_account`.
- green: `cargo test -p vr-ide -- ai_account ai::accounts ai_tests ai::spawn` → `ok. 27 passed; 0 failed`.
- guard (qa-artifacts/T8/guard.log): `cargo fmt --all --check && run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings` → `Finished dev profile … guard exit 0`.
- final (qa-artifacts/T8/test2.log): `INSTA_UPDATE=no run-heavy.sh cargo test -p vr-ide` → `738 passed; 11 failed; 7 ignored`. Failures: 9 search_tests ("search did not finish", "worker did not reach 32"), event_loop exit_restores…, app_tests external_modify…real_watcher ("took 1.29s"); load average 28 on 16 cores. Rerun of those files (qa-artifacts/T8/rerun2.log, `--test-threads=2`): `ok. 43 passed; 0 failed`. Every ai_* test passed in the full run.
- SEC-16 grep: `grep -n "read_dir\|File::open\|remove_dir_all\|set_var" crates/vr-ide/src/ai/*.rs` → only `read_dir` of `accounts/` (accounts.rs:95), `File::open` of `<state>/ai-accounts` (outside the account folders), `remove_dir_all` in `remove`; no `set_var`.

## Deviations from the plan (with reason)
- Tool per account (DS Q1) is recorded in `<state>/ai-accounts` (`name tool` lines, outside the account folders, written with `vr_config::write_atomic`, read capped at 64 KiB, invalid lines dropped). A folder without a record is not listed and is taken over if added again (recovers from a lost record without reading inside the folder).
- Rule line under the name field reads `a-z 0-9 - _, for claude` (mockup: `a-z 0-9 -`): SEC-15's regex also allows `_`.
- `del remove` in the footer only when a named account is listed (nothing to remove otherwise; keeps ai-empty-80x24's `enter start  esc hide`).
- Busy check counts every open session tab on the account (running or exited: an exited tab can restart on it).
- `/ai claude @default` = default; `@Default` (other case) is refused as reserved (SEC-15).
- `@account` completion in the palette: not done. T4's completion hook is not in this base (c8675ed); `accounts::for_tool(state, tool)` is the source to plug in after T4 merges.
- Removing an account does not revoke tokens (D13, accepted); the dialog says so.
- Clicks in the picker are ignored while the name field is open (rows under it shift by one line).

## Out-of-scope issues noticed
- not mine: search_tests (9), event_loop::tests::exit_restores_and_writes_recovery_before_waiting_for_a_replace, app_tests::external_modify_reloads_within_one_second_with_real_watcher: timing under load avg ~28; pass on rerun.

## New states
- Picker name field open (typing / error shown / Esc cancels / Enter creates + starts / paste takes one line, max 64 chars); remove dialog open (a session started on the account meanwhile → re-checked, refused; tested); account folder deleted or made a symlink while a session exists → restart refused with a notice (tested); restore with missing / invalid / reserved / other-tool account → no tab, notice (tested).

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| 0700 folder creation | capabilities.md, recovery.rs | reused `recovery::create_private_dir` |
| state folder | vr-config paths.rs | reused `vr_config::state_dir()` (only in `App::load_workspace`; tests set temp dirs) |
| atomic small state file | vr-config recent.rs | reused `vr_config::write_atomic` for the index |
| fd-based checks | rustix (workspace dep, fs feature) | `rustix::fs::{open, fstat, fchmod}`, `process::geteuid` |
| confirm dialog, notices | ui/dialog.rs, app.rs | reused `ConfirmDialog` (paragraph wrap), `app.info/warn/error` |
| display sanitizing, ~ paths | ui/safe_text.rs, git.rs | reused `sanitize`, `home_relative` |
| child env per spawn | ai/mod.rs `start_program` → `term::spawn_sized(env)` | reused T5 hook `spawn::account_env` |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| AC-12 two sessions on two accounts side by side, tab shows account | pass | `ai_account_tests::sessions_on_accounts_get_their_own_variable_only` (own CLAUDE_CONFIG_DIR per tab, labels `claude@work`, `claude@alt`) |
| AC-12 restore on the same account | pass | `ai_account_tests::restore_reopens_on_the_same_account_or_not_at_all` |
| AC-12 remove asks and deletes only its folder | pass | `ai_account_tests::remove_account_asks_refuses_while_in_use_and_deletes_only_it`, `accounts::tests::remove_deletes_only_the_account` |
| D11 picker grouping / add + /login notice / `@account` label / default = no variable | pass | `picker_groups_accounts_per_tool`, `add_account_creates_the_folder_and_starts_on_it`; snapshots ai_picker_120x40, ai_account_add_120x40 |
| D11 never reads/copies tokens, no usage tracking, no rotation | pass | SEC-16 grep; no code reads inside `accounts/<name>` |
| DS Q1 account per tool, unique names | pass | `accounts::tests::listing_and_tools` (`work is taken (claude)`, `account belongs to claude`) |
| DS Q3 remove refused while in use | pass | busy notices singular/plural + re-check after the dialog; snapshot ai_account_busy_80x24 |
| SEC-15 one validator, path only from the validated name, listing of real valid dirs | pass | `accounts::validate` used by create/remove/ready/path_of, `/ai`, restore; `names_are_validated_by_one_rule`, `listing_and_tools` |
| SEC-16 0700, real dir, owner, fchmod via O_NOFOLLOW fd, never read inside | pass | `accounts::secure`; `folders_are_private_real_and_checked_before_use`; restore 0755 → 0700; spawn refuses a symlinked account |
| SEC-17 confirm (Cancel default), refused in use, re-validate, symlink_metadata, remove_dir_all, no trash, nothing outside | pass | `accounts::remove`; `remove_deletes_only_the_account`; `remove_account_asks…`; snapshot ai_account_remove_80x24 |
| SEC-18 env only on that child, fixed map, unmapped tools cannot pick, restore without fallback, continue args with own variable | pass | `accounts::env` / `var_for`; `variables_are_a_fixed_map`; `sessions_on_accounts…` (process env unchanged, gemini refused); restore test (`ARGV:--continue|` + own folder, 5 refusals); `restart_checks_the_account_again` |
| D13 accepted risk (no revoke) | pass | dialog text "The sign-in stays valid at the provider…" (tested) |
| UI S2 mockups | pass (deviations above) | 3 new + 2 updated snapshots; `ai_account_screens` |
| `/ai claude @work`; `@account` completion | pass / n-a | `/ai` tests; completion: T4 hook not in this base (deviation) |
| BASE-11 no history-rewriting git commands | pass | none run (one help lookup was blocked by the hook before running) |
| BASE-14 no waiting loops | pass | long runs in background, turn ended |
| BASE-15 stop only own tasks | pass | nothing stopped; nothing left running |
| BASE-16 reuse | pass | `## Reuse` table; capabilities.md line added |
| BASE-17 no SDK installs | pass | none |
| BASE-18 no sub-agents | pass | none |
| BASE-19 notes from the start | pass | this file, progress lines |
| BASE-24 never stage or commit | pass | git status: unstaged / untracked only |
| BASE-25 guard | pass | guard.log `guard exit 0` |
| BASE-34 / DESIGN-23 | n-a | no network |
| BASE-38 formatters on changed files only | pass | `rustfmt --edition 2024 <changed files>` |
| COST red first, related tests, snapshots once at the end | pass | red compile errors; related ai tests; whole-crate run because app/dialog/slash/render are shared; snapshots regenerated at the end (plus one small ai_view/slash regen after the footer-rule fix) |
| COST-8 Edit/perl only | pass | Edit / Write / perl |
| COST-9 nothing left running | pass | background tasks finished; test agents end with their app |
| COST-13 throwaway in qa-artifacts/T8 | pass | qa-artifacts/T8/{warm,guard,test1,test2,rerun,rerun2}.log |
| Tests use temp state dirs only | pass | `AiPanel.state_dir` defaults to `None`; every test sets a tempdir (the dialog snapshot uses a fixed non-existent path; nothing is written there) |
| Comments English, why-only, no history | pass | accounts.rs, mod.rs, picker.rs, ai_view.rs |

## Merge
- master merged into run4-T8 by the leader. The one conflict, `crates/vr-ide/src/ai/restore.rs`, is resolved in the working tree. It is not staged and still shows `UU` in the index; the leader marks it resolved.
- Resolution: kept T5's final structure (`session::AiSection` / `AiSession`, `capture` / `from_session` / `report`, refused tabs not written back, `MAX_AI_SESSIONS`, label → `clean_name`). Applied T8's check:
  - `check()` returns `(root, account)`; an empty `account` = default (no variable).
  - Any other account must pass `accounts::ready` (SEC-15 name, SEC-16 folder checks incl. 0700 via fchmod, tool mapping, tool record). Otherwise `Not reopened: <tool>@<name>, <why> (no fallback)`.
  - The reopened session spawns with `account` via `spawn_session` → `start_program` → `spawn::account_env` (SEC-18).
  - Leftover conflict markers: none (grep over crates/, docs, README).
- `crates/vr-ide/src/ai_account_tests.rs`: the restore test now uses `session::{AiSection, AiSession}`. It also has a default (`account: ""`) session between the two account sessions. That session reopens with the inherited CLAUDE_CONFIG_DIR, and `capture` keeps `["work", "", "alt"]`. The 5 refusal notices are unchanged.
- `@account` palette completion: left out. T4 is not merged; source for it: `accounts::for_tool(state, tool)`.
- `cargo build -p vr-ide` → `Finished dev profile`.
- `cargo test -p vr-ide -- ai session` (qa-artifacts/T8/merge-test.log) → `ok. 121 passed; 0 failed` (incl. the 6 ai_account_tests and session_tests).
- Guard (qa-artifacts/T8/merge-guard.log): `cargo fmt --all --check && run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings` → `guard exit 0`.
- rustfmt only on restore.rs and ai_account_tests.rs. Nothing staged or committed; nothing running.

## Fix round 1
Red first: new tests in `ai::accounts::fix_tests` (5), `ai_account_tests::an_open_session_holds_its_account_for_other_windows`, and the footer check in `ai_view_tests::ai_account_screens`. The first run failed to compile: no `INDEX_LOCK`, no `hold`.

1. [major, COMPACT] fixed. `ui/ai_view.rs`, picker footer:
   - Pairs that do not fit are dropped whole from the end; the first pair always stays.
   - The footer row is cleared first. Without that, a list longer than the grid left its text under the footer; the new 40x12 check showed `enterastart … new`.
   - Test: `footer_has_whole_pairs` at 80x24 and 40x12.
   - Snapshots regenerated once: `ai_account_busy_80x24` and `ai_account_remove_80x24` now end in `enter start  del remove`, as in the mockups. No other snapshot changed.
2. [minor, DS Q1/SEC-18] fixed. `accounts::ready` requires the index record to name this tool. If there is no record, it refuses with `account not recorded; add it again`. The folder checks run first, so the "missing" and "not a real folder" messages stay specific. Test: `ready_needs_the_record_of_this_tool`.
3. [minor] fixed. Index reads and the lock:
   - `read_index_strict` treats a missing index as empty. An index that cannot be read (not UTF-8, over 64 KiB, not a file, opened with O_NOFOLLOW) is an error.
   - `create` and `remove` refuse with `cannot read the account record (…)` and leave the file untouched.
   - Every read-modify-write holds `flock(LOCK_EX)` on `<state>/ai-accounts.lock` (0600, O_NOFOLLOW).
   - Tests: `record_is_private_locked_and_never_rewritten_from_a_failed_read`, `concurrent_adds_keep_every_record` (8 threads, 8 records).
4. [D17] fixed. Account locks across windows:
   - Each session holds `accounts::AccountLock`, a shared non-blocking `flock` on `accounts/.<name>.lock`. The lock file sits beside the account folder, not inside it, and the listing ignores it.
   - The lock is taken in `spawn_session` before the folder checks, kept in `AiSession.account_lock`, and released when the tab closes.
   - `remove` takes the lock exclusive and non-blocking. If that fails, it returns `in use in another window` and the notice reads `Not removed: <name>, in use in another window`.
   - After removing, it unlinks the lock file. A session started during a removal is refused with `account is being removed`, or sees the folder gone.
   - Tests: `remove_is_refused_while_another_window_holds_the_account`, `an_open_session_holds_its_account_for_other_windows`.
5. [low] fixed. `remove` opens `accounts/` with O_DIRECTORY|O_NOFOLLOW and works relative to that descriptor:
   - `statat(SYMLINK_NOFOLLOW)` on the account; a symlinked account is removed with `unlinkat` (the link only).
   - A folder goes through `remove_tree_at`: each level is opened with `openat` + O_NOFOLLOW, entries are checked with `statat` without following links, files and links are unlinked, and empty folders are removed with `unlinkat(REMOVEDIR)`.
   - Test: `remove_never_follows_a_swapped_accounts_folder` (`accounts/` swapped for a symlink → refused, the outside folder intact; a deep tree with dot names, a read-only file and a link → gone, link target intact). The earlier `remove_deletes_only_the_account` still passes.
6. [low] fixed. The index is written 0600 through its own temp file (create_new, mode 0600, fsync, rename). An existing 0644 index ends up 0600 (tested).
7. [QC obs] fixed. Under the index lock, before each write, files named `.ai-accounts.*.tmp` in the state folder are removed. Only that pattern goes; `.recent.*.tmp` stays (tested).

Test adjusted: `restart_checks_the_account_again` deletes the folder with `std::fs::remove_dir_all`, standing for a deletion outside vr-ide. `accounts::remove` is now refused while the exited tab still holds the account.

New states (round 1):
- **Account lock held per open session:**
  - Quit and closing a tab release it (the fd closes).
  - A crash releases it too (the kernel drops the flock).
  - A removal that starts in between makes the new session fail with a notice; nothing is left stuck.
- **Index lock:** a blocking exclusive lock, held only for the short read-modify-write in create/remove. A file system that never returns would block that one action.

Commands:
- `INSTA_UPDATE=no run-heavy.sh cargo test -p vr-ide -- ai_account ai::accounts ai session` (qa-artifacts/T8/fix1-test.log) → `126 passed; 1 failed`. Every ai, account and session test passed.
  - The failure: `search_tests::apply_reports_a_file_failing_halfway` ("search did not finish", its 20 s limit). Rerun alone it failed again (qa-artifacts/T8/fix1-rerun.log) with load average 28.8 on 16 cores.
  - not mine: crates/vr-ide/src/search_tests.rs — `search did not finish`. Untouched by T8; it passed in the merge run.
- Guard (qa-artifacts/T8/fix1-guard.log): `cargo fmt --all --check && run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings` → `guard exit 0`.
- rustfmt only on the changed files.

Rule rows updated:
- SEC-16: pass. Index and lock files 0600; lock files are outside the account folders.
- SEC-17: pass. Removal relative to the `accounts/` fd with no link following; refused while held in any window.
- SEC-18 / DS Q1: pass. A record for this tool is required.
- COMPACT: pass. Footer keeps whole pairs, tested at 80x24 and 40x12.
- BASE-24: pass. Nothing staged or committed; nothing running.

## Merge 2
- master (T3 ssh, T4 completion, T7 attention) was merged into run4-T8 by the leader. The four conflicts are resolved in the working tree. They are not staged and still show `UU` in the index; the leader marks them resolved.
  - `README.md`: kept both `#### Accounts` (T8) and `#### Agent notifications` (T7), in that order. The accounts "Use" bullet now mentions `tab` after `@`.
  - `crates/vr-ide/src/ai/mod.rs`: `pub mod accounts;` + `pub mod attention;`. The other chunks auto-merged: `AiSession` has both `account_lock` and `notify`; T7's `attention::collect` and `SpawnExtras.attention` are in `start_program` beside the account env.
  - `crates/vr-ide/src/ui/tests/ai_view_tests.rs`: kept both, T8's `footer_has_whole_pairs` and T7's `ai_hidden_notify` / `badge_reflows_the_status_text`. The fake `session()` sets `account_lock: None` and `notify`.
  - `docs/commands.md`: master's rows (T3 `/ssh [ssh args]`, `/ws add <folder>`), with the `/ai <tool> [@account]` usage that the registry generates (the docs-table test passes).
- `@account` completion added (small, on T4's hook):
  - In `commands/completion.rs`, `/ai <tool> <word>` with `<word>` empty or starting with `@` completes `@default` plus `accounts::for_tool(state, tool)` (validated listing only).
  - Completes nothing for a tool without accounts, a tool not in `ai_tools`, a word without `@`, or a third word.
  - Test `completion_tests::ai_completes_the_accounts_of_the_tool`: red first (`left: "/ai claude @w"`, `right: "/ai claude @work"`), then green.
- `cargo build -p vr-ide` → Finished.
- `INSTA_UPDATE=no run-heavy.sh cargo test -p vr-ide -- ai ai_account attention completion slash` (qa-artifacts/T8/merge2-test.log) → `188 passed; 1 failed; 1 ignored`. All ai, account, attention, completion and slash tests passed.
  - not mine: crates/vr-ide/src/search_tests.rs `apply_reports_a_file_failing_halfway` — `search did not finish`, as in fix round 1, under load average ~28.
- Guard (qa-artifacts/T8/merge2-guard.log): `cargo fmt --all --check && run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings` → `guard exit 0`.
- rustfmt only on the changed files. Nothing staged or committed; nothing running.
