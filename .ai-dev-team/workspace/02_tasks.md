# Task board — vr-ide M1–M3

| ID | Task | Owner | Depends on | Tags | Rules | Files | Verify with | Status | Rounds |
|----|------|-------|-----------|------|-------|-------|-------------|--------|--------|
| T1 | Workspace scaffold, CLI root resolution, bench script | be | - | - | TOOL, AC-1, AC-2 | Cargo.toml, Cargo.lock, rust-toolchain.toml, .gitignore, rustfmt.toml, crates/*/Cargo.toml, crates/*/src/lib.rs, crates/vr-ide/src/{main,cli}.rs, scripts/bench-startup.sh, README.md | guard + `cargo test -p vr-ide cli` | done | 0 |
| T2 | vr-core text, selection, transactions, history, edit ops | be | T1 (relaxed: no edits to Cargo.toml, Cargo.lock, crates/vr-core/Cargo.toml, crates/vr-core/src/lib.rs until T1 committed) | - | AC-3, AC-10 | crates/vr-core/src/{lib,text,selection,transaction,history,motion,edit_ops}.rs | `cargo test -p vr-core` | done | 0 |
| T3 | vr-config keymap + settings (D10 defaults) | be | T1 (relaxed: no edits to Cargo.toml, Cargo.lock, crates/vr-config/Cargo.toml, crates/vr-config/src/lib.rs until T1 committed) | - | AC-5 | crates/vr-config/** | `cargo test -p vr-config` | done | 0 |
| T4 | vr-core documents (load/save/encoding) + file watching | be | T2 | - | AC-3, AC-7, AC-18 | crates/vr-core/src/{document,watch,lib}.rs | `cargo test -p vr-core` | todo | 0 |
| T5 | App shell: event loop, editor view, commands, tabs, status, clipboard, recovery | fe | T1, T2, T3, T4 | ui | AC-1, AC-3, AC-4, AC-5, AC-7, AC-18 | crates/vr-ide/src/{main,app,event_loop,clipboard,recovery,theme}.rs, commands/{registry,app,file,edit,tab}.rs, ui/{layout,editor_view,tabs,status_bar}.rs, ui/snapshots/ | `cargo test -p vr-ide` | todo | 0 |
| T6 | Palette, dialogs, go-to-line, external-change prompt, quit guard | fe | T5 | ui | AC-6, AC-7 | crates/vr-ide/src/ui/{palette,dialog}.rs, commands/{nav,palette}.rs, app.rs, ui/snapshots/ | `cargo test -p vr-ide` | todo | 0 |
| T7 | vr-fs tree, file ops (trash), fuzzy file index | be | T1 (relaxed: no edits to Cargo.toml, Cargo.lock, crates/vr-fs/Cargo.toml, crates/vr-fs/src/lib.rs until T1 committed) | - | AC-8, AC-9 | crates/vr-fs/** | `cargo test -p vr-fs` | fix | 1 |
| T8 | Explorer panel + go-to-file | fe | T6, T7 | ui | AC-1, AC-8, AC-9 | crates/vr-ide/src/ui/{explorer,layout,palette}.rs, commands/explorer.rs, app.rs, ui/snapshots/ | `cargo test -p vr-ide` | todo | 0 |
| T9 | vr-search engine: find/replace, project search, project replace | be | T2 | - | AC-9, AC-10, AC-11, AC-12 | crates/vr-search/** | `cargo test -p vr-search` | todo | 0 |
| T10 | Find/replace bar + project search panel | fe | T8, T9 | ui | AC-10, AC-11, AC-12 | crates/vr-ide/src/ui/{find_bar,search_panel,layout}.rs, commands/{find,search}.rs, app.rs, ui/snapshots/ | `cargo test -p vr-ide` | todo | 0 |
| T11 | Bench run, README, docs/keymap.md | be | T10 | - | AC-1 | README.md, scripts/bench-startup.sh, docs/keymap.md | bench script | todo | 0 |

Rule packs: only `TOOL` (T1) applies besides `BASE`/`COST`. Web/mobile packs (SWEEP, COMPACT, REAL, EASE, ARCH, …) are `n-a: terminal UI, no web/mobile screen`; TUI look is covered by ratatui snapshot tests at 80x24 and 40x12.

## Notes
- presence: away (user, 11:31)
- hand-off: ready (11:31) — push verified working (19dbd52)
- eta: ~20:55 (set 11:27, basis: critical path T1 20m → T2 90m → T4 45m → T5 90m → T6 45m → T8 90m → T10 90m → T11 20m + sign-off 30m + Phase 5 45m, no history, 5 agents)
- fable-policy: auto (user, 11:21)
- commits: one per task after review + QC pass, push to origin/master (user, 11:21)
- research: skipped — direction settled in conversation (leader, 11:21)
- plan gate: greenfield; user approved stack (approach A, Rust), scope M1–M3 and the run in the up-front questions (11:14–11:21)
- phase0: paths ✓ · profile fields ✓ (greenfield, provisional until T1) · local fields ✓ · e2e one spec n-a (no e2e) · test logins n-a · temporary values ✓ (none) · stray-team-files ✓ · capabilities ✓ (empty) · env-drift n-a · gitignore ✓ · stack n-a (no services) · upstream ✓ (origin empty) · guard coverage n-a (no tests yet)
- TEAM_BIN: /home/vu-dev/.claude/plugins/cache/mvanvu/ai-dev-team/1.0.37/skills/project-profiling/references (1.0.37)
- toolchain: rustup stable 1.100.0 installed by the user, `~/.cargo/bin` not on agents' default PATH → every brief exports it
- model: T1 on sonnet — boilerplate scaffold with one small function, no logic risk
- parallel plan: T2 ∥ T3 ∥ T7 after T1 — different crates (separate compile units), manifests frozen by T1; shared tree, shared target dir (cargo locks it)
- critical path: T1 → T2 → T4 → T5 → T6 → T8 → T10 → T11 → sign-off
- parallel: T2 ∥ T3 ∥ T7 — files disjoint (separate crates), started 11:29 while T1 is in review (relaxed: they consume only the frozen manifests)
- user-rules: USER-1 VS Code default keys, never ask (11:27)
- T1 verdicts (11:30): review APPROVE (3 minor → quick pass), QC_PASS; gate ✓ (rule check tables present, files read listed)
- model: T7 review opus — file deletion/move of user files is high-risk (sonnet reviewer stopped 11:35); T8 review on opus too (explorer decides which path is deleted/overwritten)
- T5 brief must include: call Keymap::validate(&registry_names) at startup (QC T3 note); enable keyboard enhancement flags; F1 fallback for ctrl+shift+p
- T3 verdicts (11:37): review APPROVE (2 code minors → quick pass), QC_PASS; gate ✓
- guard: off — session started in /home/vu-dev/DEV/ai-ide (renamed); restart claude in /home/vu-dev/DEV/vr-ide at the next stop point; leader applies guard rules by hand (named-file staging only)
- T7 fix round 1 (11:40): 4 major (F1 EXDEV copy+remove_dir_all, F2 overwrite race, F3 trash symlink loop, F4 data-safety tests) + minors; D24, D25
- T8 brief must include: reject typed names containing '/', '..' or NUL; confine ops to project root (review T7 minor)
- T2 verdicts (11:43): review CHANGES (3 minor + 1 uncertain, no major → quick pass), QC_PASS 80/80; gate ✓
