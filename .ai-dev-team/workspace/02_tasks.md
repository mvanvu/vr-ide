# Run 5 — task board (AI usage + account switch + login window)

Spec: `.ai-dev-team/workspace/01_spec.md` · Plan: `.ai-dev-team/workspace/02_plan.md` (task N = plan Task N)

| ID | Title | Owner | Depends | Tags | Rules | Files | Verify | Status | Rounds |
|---|---|---|---|---|---|---|---|---|---|
| TM | Threat model delta: shared history, statusLine helper, auto-restart, login web view | be | - | security | SEC-n | .ai-dev-team/workspace/02_threat-model.md | - | in progress | 0 |
| T1 | Settings `ai_auto_switch`, `ai_auto_switch_prompt` | be | - | - | AC-1 | crates/vr-config/src/settings.rs, crates/vr-config/defaults/settings.toml, crates/vr-config/tests/config.rs | `cargo test -p vr-config` | done (review APPROVE, QC_PASS) | 0 |
| T2 | Shared history between accounts | be | TM | security | AC-2, SEC-n | crates/vr-ide/src/ai/history.rs (new), ai/accounts.rs, ai/mod.rs, README.md | `cargo test -p vr-ide` | todo | 0 |
| T3 | Claude usage report (statusLine helper) | be | TM, T2 | security | AC-3, SEC-n | crates/vr-ide/src/ai/usage.rs (new), cli.rs, main.rs, ai/mod.rs, Cargo.toml, crates/vr-ide/Cargo.toml, Cargo.lock | `cargo test -p vr-ide` | todo | 0 |
| T5 | Limit/login detection, per-account state (sets `works` on usage reports) | be | T3 | security | AC-5, SEC-n | crates/vr-ide/src/ai/limits.rs (new), ai/mod.rs | `cargo test -p vr-ide` | todo | 0 |
| T4 | Usage display (label, 90 % notice, `/ai usage`) | fe | T3, T5 | ui | AC-4 | ai/mod.rs (label), ui/ai_view.rs, ai/usage.rs, commands/slash.rs, docs/commands.md, README.md | `cargo test -p vr-ide` | todo | 0 |
| T6 | Auto-switch | be | T1, T2, T4, T5 | security | AC-6, AC-7, SEC-n | crates/vr-ide/src/ai/switch.rs (new), ai/mod.rs, commands/slash.rs, docs/commands.md, README.md | `cargo test -p vr-ide` | todo | 0 |
| T7 | Codex limits | be | T6 | security | AC-8 | ai/usage.rs, ai/mod.rs, ai/switch.rs, README.md | `cargo test -p vr-ide` | todo | 0 |
| T8 | Login window inside vr-ide | be | T7, apt deps (user) | security | AC-9, SEC-n | crates/vr-ide/src/ai/login.rs (new), cli.rs, main.rs, ai/mod.rs, ai/accounts.rs, crates/vr-ide/Cargo.toml, Cargo.lock, README.md | `cargo test -p vr-ide`, `--no-default-features` | todo | 0 |

## Notes
- phase0: paths ✓ · profile fields ✓ (tui, no web packs) · local fields ✓ · e2e one spec n-a (tui) · test logins n-a · temporary values ✓ (none) · stray-team-files ✓ · capabilities ✓ · env-drift n-a · tracked-media ✓ · upstream ✓ (pushed 8b627c8; ahead 0) · commit identity ✓ (mvanvu <mvanvu@gmail.com> = push account mvanvu; credential username pinned, push dry-run ok) · plugin: loaded 1.0.43, marketplace 1.0.44, local source 1.0.45 unpushed · monitor on (local.md)
- archive: workspace → workspace_20261127-2224 (run 4, all done; T3/T4 status cells stale, merged per notes), uncommitted
- Order change vs plan: T5 before T4; the "works" flag is set in T5 (consumes T3's report), not T3.
- All tasks are in the `vr-ide` crate except T1 (vr-config): only T1 runs beside others (parallel: T1 ∥ TM/T2 — files disjoint, different crates).
- presence: away (user, 22:24) — hand-off pending
- fable-policy: auto (user, 22:24)
- commits: one local commit per task when done; push origin master at the end of the run (user, 22:24)
- account_switch: none (local.md, user)
- hand-off: pending — apt build deps for T8 (user sudo), plugin update before leaving
- parallel: TM ∥ T1 — TM read-only, T1 in vr-config only (22:27)
- eta: ~06:30 (set 22:27, basis: critical path TM→T2→T3→T5→T4→T6→T7→T8 + final review, ~45–90 min per task incl. review/qc, sequential in one crate)
- 22:27 verdict gate T1 review ✓ APPROVE (sonnet, files read 3/3, no findings, rule check table present). Waiting: QC T1.
- 22:28 verdict gate T1 QC ✓ QC_PASS (sonnet, 10 harness cases + crate tests). T1 done.
