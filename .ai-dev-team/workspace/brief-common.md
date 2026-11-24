# Common brief part (every developer / reviewer / qc of this run reads this first)

## Paths — put in front of every command (each tool call is a new shell)
The export line with this machine's absolute values is the first line of your brief (PATH must include $HOME/.cargo/bin).
Project root: the repo root named in your brief — run commands as `cd <root> && …` in the same call.

## Inputs every task reads
- Spec: `.ai-dev-team/workspace/01_spec.md` (decisions D1–D13, leader calls C1–C6, AC-1…AC-18). D10 = VS Code default keys.
- Plan: `.ai-dev-team/workspace/02_plan.md` — Global Constraints, Review Focus, Contracts, Decisions, your Task section.
- Board: `.ai-dev-team/workspace/02_tasks.md`. Profile: `.ai-dev-team/project-profile.md`. Capabilities: `.ai-dev-team/capabilities.md`.
- Decisions ledger: `.ai-dev-team/workspace/decisions.md` (append `D? (T{id}) question — choice — why`; the leader numbers).

## Rule list (developer) — full text: `$TEAM_BIN/../../dev-team/references/rule-packs.md` §3, COST: `$TEAM_BIN/../../dev-team/references/resource-budget.md` §7
- BASE-1 Scope: only the task's files/behavior change; unrelated problems go in notes.
- BASE-2 Contracts: signatures and types match the plan's Contracts exactly; a wrong contract → `BLOCKED`, not changed silently (small additive helpers are fine; list them in notes).
- BASE-3 Tests first, each seen failing before the code.
- BASE-4 Evidence: trimmed real output of final related tests, clippy and build in the notes.
- BASE-5 Heavy commands (workspace builds, release builds) via `$TEAM_BIN/run-heavy.sh`; nothing left running.
- BASE-6 English; comments explain why, never bug history.
- BASE-8 No new dependency beyond the workspace manifest (manifests are frozen by T1; need one → report to the leader).
- BASE-10 Test scope: `cargo test -p <your crate>` only; a failure outside your crate → rerun once, report `not mine`.
- BASE-11 No stash/checkout/restore/reset/branch switch on the shared tree.
- BASE-14 A command that may outlast the foreground limit runs as a background task; then end your turn.
- BASE-15 Stop only your own background tasks with `TaskStop <id>`; never pkill/killall.
- BASE-16 Reuse first: check capabilities.md; append what you add that others can reuse.
- BASE-17 No toolchain installs (rustup stable 1.100.0 is installed).
- BASE-18 No sub-agents.
- BASE-19 Notes file `.ai-dev-team/workspace/04_dev_T{id}.md` created at start, one progress line per step.
- BASE-24 No `git add/commit/reset/rm --cached` — the leader commits.
- BASE-25 Guard before DONE: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings` (if another crate being built in parallel breaks the workspace clippy, run `cargo clippy -p <your crate> --all-targets -- -D warnings` and report the other crate `not mine`).
- BASE-32 Decisions honoured (spec D/C entries, decisions.md).
- BASE-38 Formatters only on your files: `rustfmt --edition 2024 <files>`.
- COST: related tests only; build once at the end; no polling loops.
Parallel tasks run on the same tree in other crates (T2 vr-core, T3 vr-config, T7 vr-fs, later T9 vr-search): never edit another crate's files; cargo's target-dir lock may make you wait briefly — that is normal.

## Report (developer)
Notes end with `## Rule compliance` (rule-packs.md §6: one row per exercised rule ID with evidence, unexercised IDs grouped on `n-a:` lines, every ID exactly once) and the guard output. Final message: `DONE T{id}` or `BLOCKED T{id} — <reason>`, files changed, test/guard result lines.

## Fixed lines
- A command that may outlast the foreground limit runs as a background task; then end your turn — no Monitor, `sleep` loop or progress hand-back (`BASE-14`).
- Throwaway scripts go in `$TEAM_WS/qa-artifacts/<task>/`; never write to `$TEAM_TOOLS` or search the disk for tools (`COST-13`).
- No faked `User-Agent`, no unofficial endpoints; a blocked source → switch to the next allowed one and say so (`DESIGN-23`, `BASE-34`).
- Stop only your own background tasks with `TaskStop <id>`; never `pkill` / `killall` (`BASE-15`).
- Edit files with the Edit tool; bulk regex with `perl -pi -e`; never edit through `python3 -` heredocs (`COST-8`).
- Run formatters and auto-fixers only on the files you changed (`BASE-38`); regenerate goldens/snapshots once, after the code is final (`COST-15`).
- No questions to the user: take the plan's choice or the safest option and log it in decisions.md.

## Ledger warning (failed in two tasks: T3, T7)
- BASE-3: write each test, run it and SEE IT FAIL before writing the code it covers — one test at a time, not a batch written together with the implementation. Record the red line per test in the notes.

## Guard hook off (session folder renamed)
- The guard hook is off for this run (session folder renamed): never run `git checkout --`, `git restore`, `git stash`, `git clean`, `git reset --hard`, `pkill`/`killall`, `nohup` or a trailing `&`.

## Fix briefs (leader rule from 11:51)
- A fix or quick-pass brief carries the verifier finding, the expected behaviour and the test that must go red then green. Suggestions are the verifier's, never a leader recipe; the developer may pick another fix. On data-safety code (save/overwrite, delete, move, project replace) a quick pass changes behaviour only where the finding says; anything more is a counted round with qc.

## Ledger warning (recurring defect class: T7, T9)
- Writes to user files: no gap between the freshness check and the replace; a concurrent writer must never be overwritten or deleted — test it with a writer racing the operation (T7/T9 class).
- QC of any task that writes user files: one concurrent-writer check per write path (a background loop saving the file while the operation runs).

## Reporting
- Your verdict / DONE report goes in your final message only; never also SendMessage it to the leader — it would arrive twice.

## Severity anchor (reviewers of write paths)
- A race window in which a concurrent writer's data is overwritten or deleted is a `blocker` however small; "narrow enough" is never accepted without a racing-writer stress test (vr_core::testutil, T12) that loses nothing.

## Report files
- Writing your notes / verdict files under `.ai-dev-team/workspace/` is required. The user instruction "never create artifacts" means the claude.ai Artifact tool only, not these files.

## New states rule (from 16:51)
- A fix that adds a state, thread, background job, branch or kept value lists it under `## New states (round n)` in the notes, with how quit, cancel/Esc, an error halfway, the input changing, a second request and a concurrent writer behave there, plus a call that never returns (a hung network file system, a FIFO or device in place of a file), and one test per case that can lose data or leave the app stuck.
