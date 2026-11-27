# T2b SEC-38 for shells, ssh and /run
- Files changed: crates/vr-ide/src/commands/terminal.rs (new `without_stray_vars`, called in `spawn_sized`, the one spawn path of shells, ssh, /run and AI agents), crates/vr-ide/src/terminal_tests.rs (test).
- Test added: terminal_tests::spawned_terminals_drop_inherited_account_variables.
- Progress: read code; red test; code; related tests; guard; notes.
- Red: `error[E0432]: unresolved import crate::commands::terminal::without_stray_vars` (test written before the function).
- Green: `test terminal_tests::spawned_terminals_drop_inherited_account_variables ... ok`.
- Commands run + result:
  - `cargo test -p vr-ide terminal` -> `test result: ok. 110 passed; 0 failed`
  - `cargo test -p vr-ide ssh` -> `test result: ok. 99 passed; 0 failed`
  - `cargo fmt --all --check` -> no output, exit 0
  - `$TEAM_BIN/run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings` -> `Finished dev profile ... target(s) in 30.68s` (no warnings)
- Deviations from the plan: none. Startup notice skipped (not simple).
- Out-of-scope issues noticed: ai/mod.rs `start_program` still adds `stray_vars` itself; harmless, deduplicated by the new function.

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| detect inherited vars pointing into accounts/ | capabilities.md, `grep -rn stray_vars crates` | reused `crates/vr-ide/src/ai/history.rs` `stray_vars` |
| ssh withheld env | `grep -rn withheld_env` | left as is; ssh gets SEC-38 through `spawn_sized` |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| SEC-38 | pass | terminal_tests::spawned_terminals_drop_inherited_account_variables (CLAUDE_CONFIG_DIR into accounts/ dropped, CODEX_HOME outside kept, no state dir drops nothing); terminal.rs `spawn_sized` |
| BASE-1 | pass | diff touches only commands/terminal.rs and terminal_tests.rs |
| BASE-2 | pass | no contract changed; `SpawnExtras.env_remove` used as is |
| BASE-3 | pass | red line above: E0432 before the function existed; then ok |
| BASE-4 | pass | related-test, fmt and clippy output quoted above |
| BASE-5 | pass | clippy via `run-heavy.sh`; the two small related test runs ran directly as the brief allows; nothing left running |
| BASE-6 | pass | English code and notes; the comment explains why (every terminal child passes here) |
| BASE-9 | pass | the change removes inherited account variables from child environments (T-48) |
| BASE-10 | pass | only `terminal` and `ssh` related tests plus the guard ran |
| BASE-11 | pass | work in worktree .worktrees/run5-T2b; no stash/checkout/reset |
| BASE-14 | pass | no waiting loops; all commands finished in the foreground |
| BASE-15 | pass | no process killed |
| BASE-16 | pass | `## Reuse` table above |
| BASE-17 | pass | nothing installed; nothing written to $TEAM_TOOLS |
| BASE-18 | pass | no agents started |
| BASE-19 | pass | notes file created, progress line per step |
| BASE-23 | pass | one spawn path covers shell, ssh, /run and agents; one test of the shared function |
| BASE-24 | pass | only `git status --short` run; nothing staged or committed |
| BASE-25 | pass | `cargo fmt --all --check` (no output, exit 0) && `cargo clippy --workspace --all-targets -- -D warnings` (via run-heavy: `Finished dev profile ... in 30.68s`) |
| BASE-32 | pass | D11 kept: the account's own variable is set per spawn after the removal |
| BASE-38 | pass | `rustfmt` ran on only commands/terminal.rs and terminal_tests.rs |
| COST-1 | pass | clippy through run-heavy.sh |
| COST-2 | pass | only the developer's tests ran |
| COST-3 | pass | no sleep/pgrep polling |
| COST-4 | pass | no full-suite run; related tests only |
| COST-5 | pass | no browser, server or watcher started |
| COST-8 | pass | Edit tool for edits, one pass per file; one append for the test |
| COST-9 | pass | nothing left running |
| COST-10 | pass | related set: `grep -rln child_env` found terminal_tests.rs; ssh tests run too |
| COST-13 | pass | no scratch scripts; nothing in $TEAM_TOOLS |

n-a: BASE-7 (no commit in this task) · BASE-8 (no dependency change) · BASE-12, BASE-13, BASE-20, BASE-21, BASE-22 (no outside write, no deletions, no secrets, no new names beyond one function) · BASE-26, BASE-27, BASE-28, BASE-29, BASE-30, BASE-31, BASE-33, BASE-34, BASE-35, BASE-36, BASE-37, BASE-39, BASE-40, BASE-41, BASE-42, BASE-43, BASE-44, BASE-45, BASE-46, BASE-47, BASE-48, BASE-49 (no rewrite, DB, browser, container, UI, schema, outward write, bug report or CI change in this task) · COST-6, COST-7, COST-11, COST-12, COST-14, COST-15, COST-16, COST-17, COST-18, COST-19, COST-20, COST-21 (no UI sweep, e2e, CI wait, goldens, qc work or leader-only step)

## Quick pass 1
Files: crates/vr-ide/src/ssh/mod.rs (`probe` takes `env_remove`), commands/ssh.rs (probe caller passes `without_stray_vars(..).env_remove`), ai/history.rs (`stray_warnings`), app.rs (startup warnings), ssh/tests.rs, terminal_tests.rs.
1. Probe: test `ssh::tests::sec_38_the_probe_drops_the_variables_it_is_told_to` (shim ssh exits 3 while a variable is set). Red: E0061 (probe took 5 args). Green: ok.
2. Startup notice: one `ConfigWarning::InvalidValue` per stray variable (no new variant in vr-config), text says the value "is ignored for the terminals, ssh sessions and agents vr-ide starts". Test `ai::history::tests::stray_variables_give_one_startup_warning_each`. Red: E0425 `stray_warnings` missing. Green: ok. Wired in app.rs after `history::configure`.
3. Spawn path: `terminal_tests::a_shell_tab_does_not_inherit_an_account_variable` re-runs itself as a child process with `CLAUDE_CONFIG_DIR=/state/vr-ide/accounts/claude/w` really inherited (no `set_var`), opens a real /bin/sh tab with `state_dir=/state/vr-ide` and asserts `echo "<$CLAUDE_CONFIG_DIR>"` prints `<>`. This replaces an injectable source with the real inherited environment. Red: with the `without_stray_vars` call removed from `spawn_sized` the test fails (`FAILED`, panic at the `/accounts/` assert); restored, green: ok.
- Related: `cargo test -p vr-ide terminal` 111 passed; `ssh` 100 passed; `history` 22 passed.
- Guard: `cargo fmt --all --check` exit 0 && `run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings` -> `Finished dev profile ... in 3.16s`, no warnings.
- Formatting: rustfmt only on the 7 changed files. Nothing left running; nothing staged.
