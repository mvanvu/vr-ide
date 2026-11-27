# Run 5 — task board (AI usage + account switch + login window)

Spec: `.ai-dev-team/workspace/01_spec.md` · Plan: `.ai-dev-team/workspace/02_plan.md` (task N = plan Task N)

| ID | Title | Owner | Depends | Tags | Rules | Files | Verify | Status | Rounds |
|---|---|---|---|---|---|---|---|---|---|
| TM | Threat model delta: shared history, statusLine helper, auto-restart, login web view | be | - | security | SEC-n | .ai-dev-team/workspace/02_threat-model.md | - | done (SEC-19..39; C1–C8 → D1–D8) | 0 |
| T1 | Settings `ai_auto_switch`, `ai_auto_switch_prompt` | be | - | - | AC-1 | crates/vr-config/src/settings.rs, crates/vr-config/defaults/settings.toml, crates/vr-config/tests/config.rs | `cargo test -p vr-config` | done (review APPROVE, QC_PASS) | 0 |
| T2 | Shared history between accounts | be | TM | security | AC-2, SEC-n | crates/vr-ide/src/ai/history.rs (new), ai/accounts.rs, ai/mod.rs, README.md | `cargo test -p vr-ide` | done (review APPROVE + QP1 re-check, QC_PASS) | 0 |
| T2b | SEC-38 for shells, ssh and /run (drop inherited CLAUDE_CONFIG_DIR/CODEX_HOME into accounts/),  | be | T2 | security | SEC-38 | commands/terminal.rs, commands/ssh.rs, commands/run (where /run spawns) | `cargo test -p vr-ide` | done (review APPROVE + QP1 re-check, QC_PASS; committed on run5-T2b, merge after T3) | 0 |
| T3 | Claude usage report (statusLine helper) | be | TM, T2 | security | AC-3, SEC-n | crates/vr-ide/src/ai/usage.rs (new), cli.rs, main.rs, ai/mod.rs, Cargo.toml, crates/vr-ide/Cargo.toml, Cargo.lock | `cargo test -p vr-ide` | done (review APPROVE r1, QC_PASS + re-check) | 1 |
| T5 | Limit/login detection, per-account state (sets `works` on usage reports) | be | T3 | security | AC-5, SEC-n | crates/vr-ide/src/ai/limits.rs (new), ai/mod.rs | `cargo test -p vr-ide` | done (review APPROVE r1 + QP1, QC_PASS + re-check) | 1 |
| T4 | Usage display (label, 90 % notice, `/ai usage`) | fe | T3, T5 | ui | AC-4 | ai/mod.rs (label), ui/ai_view.rs, ai/usage.rs, commands/slash.rs, docs/commands.md, README.md | `cargo test -p vr-ide` | todo | 0 |
| T6 | Auto-switch | be | T1, T2, T4, T5 | security | AC-6, AC-7, SEC-n | crates/vr-ide/src/ai/switch.rs (new), ai/mod.rs, commands/slash.rs, docs/commands.md, README.md | `cargo test -p vr-ide` | todo | 0 |
| T7 | Codex limits | be | T6 | security | AC-8 | ai/usage.rs, ai/mod.rs, ai/switch.rs, README.md | `cargo test -p vr-ide` | todo | 0 |
| T8 | Login window inside vr-ide | be | T7, apt deps (user) | security | AC-9, SEC-n | crates/vr-ide/src/ai/login.rs (new), cli.rs, main.rs, ai/mod.rs, ai/accounts.rs, crates/vr-ide/Cargo.toml, Cargo.lock, README.md | `cargo test -p vr-ide`, `--no-default-features` | todo | 0 |

## Notes
- phase0: paths ✓ · profile fields ✓ (tui, no web packs) · local fields ✓ · e2e one spec n-a (tui) · test logins n-a · temporary values ✓ (none) · stray-team-files ✓ · capabilities ✓ · env-drift n-a · tracked-media ✓ · upstream ✓ (pushed 8b627c8; ahead 0) · commit identity ✓ (mvanvu <mvanvu@gmail.com> = push account mvanvu; credential username pinned, push dry-run ok) · plugin: loaded 1.0.43, marketplace 1.0.44, local source 1.0.45 unpushed · monitor on (local.md)
- archive: workspace → workspace_20261127-2224 (run 4, all done; T3/T4 status cells stale, merged per notes), uncommitted
- Order change vs plan: T5 before T4; the "works" flag is set in T5 (consumes T3's report), not T3.
- All tasks are in the `vr-ide` crate except T1 (vr-config): only T1 runs beside others (parallel: T1 ∥ TM/T2 — files disjoint, different crates).
- presence: away (user, 22:24)
- fable-policy: auto (user, 22:24)
- commits: one local commit per task when done; push origin master at the end of the run (user, 22:24)
- account_switch: none (local.md, user)
- hand-off: ready (22:31) — apt deps installed (webkit2gtk-4.1 2.52.6), plugin 1.0.45 loaded, account_switch: none (cswap present), Workflow not used while away (Phase 5 via Agent)
- parallel: TM ∥ T1 — TM read-only, T1 in vr-config only (22:27)
- eta: ~05:20 (set 22:27 as ~06:30, updated 23:19: 5 tasks done in ~65 min; remaining T5 45, T4 40, T6 90, T7 40, T8 90, final review 45)
- 22:27 verdict gate T1 review ✓ APPROVE (sonnet, files read 3/3, no findings, rule check table present). Waiting: QC T1.
- 22:28 verdict gate T1 QC ✓ QC_PASS (sonnet, 10 harness cases + crate tests). T1 done.
- TEAM_BIN → /home/rainy/.claude/plugins/cache/mvanvu/ai-dev-team/1.0.45/skills/project-profiling/references (plugin 1.0.45 reloaded by the user, 22:31)
- 22:34 TM done: SEC-19..39, contradictions C1–C8 decided as D1–D8 (decisions.md). T2 starts.
- 22:46 T2 DONE gate ✓ (notes, compliance 40 pass + n-a lines, guard clean). Extra files app.rs, ai_account_tests.rs, capabilities.md accepted (startup wiring, tests). Follow-up row T2b from T2's open items. Review (opus, high-risk) + QC start.
- 22:48 verdict gate T2 QC ✓ QC_PASS (harness on history::ensure, 6 cases with hashes; spawn wiring covered by dev test only). Waiting: review T2 (opus).
- 22:50 verdict gate T2 review ✓ APPROVE (opus, files read 6 + callees, 4 minor incl. 1 low security). Quick pass 1 to the T2 developer; re-check by the same opus reviewer (security finding).
- 22:52 T2 QP1 DONE (4/4; red→green except the continuation test, which already held; guard clean). Re-check by the T2 opus reviewer.
- 22:52 verdict gate T2 QP1 re-check ✓ APPROVE (4/4 closed). T2 done. Remove-account dialog wording moved from T2b to T8 (SEC-37 area).
- 22:53 parallel: T3 (main tree) ∥ T2b (worktree .worktrees/run5-T2b) — files disjoint (T2b: commands/terminal.rs, ssh.rs; T3: usage.rs, cli.rs, main.rs, ai/mod.rs, manifests); same crate → worktree. Merge order: T3 first? no — whichever is done first, merge main into the other before its merge.
- 22:56 T2b DONE gate ✓ (after table resend). Review opus + QC sonnet on the worktree.
- 22:57 verdict gate T2b QC ✓ QC_PASS (real spawns: shell, /run, ssh; control without state dir). Note: qc added and removed a temporary test in the worktree while the review ran (restored; git status = dev's 2 files). Non-defect noted: ssh -G probe inherits env (no shell). Waiting: review T2b.
- 22:58 verdict gate T2b review ✓ APPROVE (opus, 3 minor incl. 1 low security). Quick pass 1 → T2b developer; re-check by the same opus reviewer.
- 23:00 T2b QP1 DONE (3/3, red→green, guard clean; 7 files incl. ssh/mod.rs, history.rs, app.rs). Re-check by the T2b opus reviewer.
- 23:01 verdict gate T2b QP1 re-check ✓ APPROVE (3/3 closed). T2b done, committed on run5-T2b; merged into master after T3 is committed (T3 is compiling in the main tree; files disjoint).
- 23:09 T3 DONE gate ✓ (compliance pass 41 / n-a 32 / fail 1 BASE-49 → ledger; guard clean). Extra files: usage_tests.rs, tests/statusline_helper.rs, ai_tests.rs, ai_account_tests.rs, capabilities.md. Unverified until the user returns: SEC-22 precedence of --settings over a project statusLine (needs signed-in claude). Review opus + QC sonnet.
- 23:12 verdict gate T3 QC ✓ QC_PASS (real helper via sh -c, hostile inputs, quoting; SEC-22 project-precedence unverified — needs signed-in claude). Waiting: review T3.
- 23:12 verdict gate T3 review ✓ REQUEST_CHANGES (opus; 2 major: reports read only on PTY wake [AC-3 fail], restart untested [BASE-23]; 3 minor). Severity re-grade: agree. Fix round 1 → T3 developer (all 5). Re-check: reviewer; qc re-checks the hidden-panel/idle read case (functional defect qc had passed).
- 23:17 T3 R1 DONE (5/5; red→green; related 303 + e2e 3 pass; guard clean; event_loop.rs added). Re-check: T3 reviewer (all findings); QC T3 re-checks the hidden-panel/idle read case.
- 23:18 verdict gate T3 R1 re-check ✓ APPROVE (opus, 5/5 closed, no new). Waiting: QC T3 re-check (hidden panel).
- 23:18 verdict gate T3 QC re-check ✓ QC_PASS (hidden panel, idle, 1 read/s). T3 done. Next: merge run5-T2b into master, then T5.
- 23:38 T5 DONE gate ✓ (merged tree green first; compliance per ID; guard clean). Dev's D1–D9 renumbered D19–D27; open point (other tools) decided D21. Extra files: event_loop.rs (one call), visibility in usage.rs/accounts.rs/ui/ai_changes.rs, limits_tests.rs, ai_tests.rs. Review opus + QC sonnet.
- 23:40 verdict gate T5 QC ✓ QC_PASS (7 cases; F1 low: user-typed limit text at column 0 in a program without input box → notice; F2 info = D21). Process note: qc edited ai_tests.rs temporarily (restored from .orig) while the T5 review ran — future qc briefs: harness under qa-artifacts, not repo files. Waiting: review T5.
- 23:42 verdict gate T5 review ✓ REQUEST_CHANGES (opus; blocker: non-ASCII reset text panics the main loop limits.rs:475 — confirmed by probe; medium: printed login line marks account not working; 4 minor: DST/TZ, rollover, cross-session clear, spend/credit stop messages; uncertain: real limit-screen layout unverified → user check on return). + QC F1. Fix round 1 → a FRESH opus developer from 04_dev_T5.md (T5's developer is at 261k tokens, COST-16).
- 23:42 note: leader re-ran task-base T5 by mistake (now 1cbbbaef = tree at the start of fix round 1, used as the round-1 base). Full T5 diff = git diff HEAD + untracked limits*.rs (HEAD 1c4223a has no T5 change). Fix round 1 → fresh opus developer.
- 23:59 T5 R1 DONE (7/7, mutation red→green per finding, related tests + guard clean; ui/ai_changes.rs visibility only). Decisions D28–D32. Out of scope noted: TZif footer rule ignored by utc_offset (also the UI clock) → Phase 6 report. Re-check: T5 reviewer (all); QC re-checks the crash (non-ASCII) and typed-text cases.
- 00:00 verdict gate T5 R1 re-check ✓ APPROVE (opus; 7/7 closed; 2 new minor: shorter limit overwrites a longer sticky one; derived Debug on Watch would print typed text). Quick pass after QC's re-check reports (no edits under a running verdict).
- 00:01 verdict gate T5 QC re-check ✓ QC_PASS (crash replay: no panic; F1 typed/pasted at column 0: no notice). codex keeps the notice without a time when the reset is unparsable (genuine head) — accepted. Quick pass 1 (2 minors) → T5 R1 developer; re-check by the T5 reviewer.
- 00:03 T5 QP1 DONE (2/2 red→green, guard clean). Re-check by the T5 reviewer.
- 00:03 verdict gate T5 QP1 re-check ✓ APPROVE. T5 done. Next: T4.
