# Task board — vr-ide M1–M3

| ID | Task | Owner | Depends on | Tags | Rules | Files | Verify with | Status | Rounds |
|----|------|-------|-----------|------|-------|-------|-------------|--------|--------|
| T1 | Workspace scaffold, CLI root resolution, bench script | be | - | - | TOOL, AC-1, AC-2 | Cargo.toml, Cargo.lock, rust-toolchain.toml, .gitignore, rustfmt.toml, crates/*/Cargo.toml, crates/*/src/lib.rs, crates/vr-ide/src/{main,cli}.rs, scripts/bench-startup.sh, README.md | guard + `cargo test -p vr-ide cli` | done | 0 |
| T2 | vr-core text, selection, transactions, history, edit ops | be | T1 (relaxed: no edits to Cargo.toml, Cargo.lock, crates/vr-core/Cargo.toml, crates/vr-core/src/lib.rs until T1 committed) | - | AC-3, AC-10 | crates/vr-core/src/{lib,text,selection,transaction,history,motion,edit_ops}.rs | `cargo test -p vr-core` | done | 0 |
| T3 | vr-config keymap + settings (D10 defaults) | be | T1 (relaxed: no edits to Cargo.toml, Cargo.lock, crates/vr-config/Cargo.toml, crates/vr-config/src/lib.rs until T1 committed) | - | AC-5 | crates/vr-config/** | `cargo test -p vr-config` | done | 0 |
| T4 | vr-core documents (load/save/encoding) + file watching | be | T2 | - | AC-3, AC-7, AC-18 | crates/vr-core/src/{document,watch,lib}.rs | `cargo test -p vr-core` | fix | 3 |
| T5 | App shell: event loop, editor view, commands, tabs, status, clipboard, recovery | fe | T1, T2, T3, T4 | ui | AC-1, AC-3, AC-4, AC-5, AC-7, AC-18 | crates/vr-ide/src/{main,app,event_loop,clipboard,recovery,theme}.rs, commands/{registry,app,file,edit,tab}.rs, ui/{layout,editor_view,tabs,status_bar}.rs, ui/snapshots/ | `cargo test -p vr-ide` | in-progress | 0 |
| T6 | Palette, dialogs, go-to-line, external-change prompt, quit guard | fe | T5 | ui | AC-6, AC-7 | crates/vr-ide/src/ui/{palette,dialog}.rs, commands/{nav,palette}.rs, app.rs, ui/snapshots/ | `cargo test -p vr-ide` | todo | 0 |
| T7 | vr-fs tree, file ops (trash), fuzzy file index | be | T1 (relaxed: no edits to Cargo.toml, Cargo.lock, crates/vr-fs/Cargo.toml, crates/vr-fs/src/lib.rs until T1 committed) | - | AC-8, AC-9 | crates/vr-fs/** | `cargo test -p vr-fs` | done | 3 |
| T8 | Explorer panel + go-to-file | fe | T6, T7 | ui | AC-1, AC-8, AC-9 | crates/vr-ide/src/ui/{explorer,layout,palette}.rs, commands/explorer.rs, app.rs, ui/snapshots/ | `cargo test -p vr-ide` | todo | 0 |
| T9 | vr-search engine: find/replace, project search, project replace | be | T2 | - | AC-9, AC-10, AC-11, AC-12 | crates/vr-search/** | `cargo test -p vr-search` | done | 2 |
| T10 | Find/replace bar + project search panel | fe | T8, T9 | ui | AC-10, AC-11, AC-12 | crates/vr-ide/src/ui/{find_bar,search_panel,layout}.rs, commands/{find,search}.rs, app.rs, ui/snapshots/ | `cargo test -p vr-ide` | todo | 0 |
| T11 | Bench run, README, docs/keymap.md | be | T10 | - | AC-1 | README.md, scripts/bench-startup.sh, docs/keymap.md | bench script | todo | 0 |
| T12 | Shared racing-writer test helper (vr-core test utils) + use it in T4/T7/T9 write-path tests | be | T4, T9 | - | AC-3, AC-8, AC-12 | crates/vr-core/src/testutil.rs (cfg test feature), tests in vr-core, vr-fs, vr-search | `cargo test -p vr-core -p vr-fs -p vr-search` | done | 0 |

Rule packs: only `TOOL` (T1) applies besides `BASE`/`COST`. Web/mobile packs (SWEEP, COMPACT, REAL, EASE, ARCH, …) are `n-a: terminal UI, no web/mobile screen`; TUI look is covered by ratatui snapshot tests at 80x24 and 40x12.

## Notes
- presence: away (user, 11:31)
- hand-off: ready (11:31) — push verified working (19dbd52)
- eta: ~16:35 (set 12:15, moved +15m: T4 (critical path) needs fix round 2; remaining T4 round 2 + re-check ~20m → T5 40m → T6 25m → T8 35m → T10 35m → T11 15m + sign-off 25m + Phase 5 40m + buffer)
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
- waiting: T4 waits for the T2 quick pass (same crate vr-core, shared compile unit); D31 (ropey unicode_lines off, root Cargo.toml) goes into T4 after T7 is committed (T7 edits root Cargo.toml for rustix)
- parallel: T9 (vr-search) ∥ T7 fix (vr-fs) ∥ T2 quick pass (vr-core) — files disjoint, 11:44
- T7 verdicts (11:48): round 1 review APPROVE (opus, 5 minor → quick pass), QC_PASS R1; gate ✓
- T8 brief must include (review T7): use the path ensure_within returns; never offer rename/trash/move on the root node; drop selected paths nested under another selected path before batch trash; handle per-path BatchResult
- T7 fix round 2 (11:51): quick pass item (1) — the leader's instruction to undo the extra hard link — opened a delete-by-name race (major); fix (a): never unlink, return BothNamesLeft
- T7 round 2 gets qc re-check (file-move behaviour changed)
- T7 fix round 3 (11:53): R2-1 major — hard-link fallback unlinks `from` by name (a concurrent save to `from` lost); + QC note: EACCES into read-only dir reported as AlreadyExists. Last round before escalation (fable-policy auto).
- T7 verdicts (11:57): round 3 review APPROVE (opus; m1 temp-name collision → quick pass 2, m2 → T8 brief), QC_PASS R3; gate ✓
- T8 brief must include (review T7 m2): startup notice listing leftover '.*.vr-tmp-*' files in the root and expanded folders
- model: T9 review opus — apply_replace overwrites user files on disk (data-safety high-risk)
- model: T4 review opus — save overwrites user files (data-safety high-risk)
- T9 fix round 1 (12:01): 3 major (apply TOCTOU across write+fsync, unbounded whole-file reads, prefilter misses inline-flag patterns) + minors; D45
- T12 added by leader while away (12:02): recurring race class T7/T9 → shared racing-writer fixture; sonnet; after T4 and T9 are done
- T4 fix round 1 (12:03): 3 major (save replaces files the user cannot write; changed-on-disk check far from rename + stamp read after dir fsync; watcher drops rescan/errors) + minors; D46
- T4 fix round 2 (12:14): R1-1 major (stale own-write entry hides a revert → AC-7 reload missed), R1-2 minor (metadata-only change → false ChangedOnDisk); QC R1 pass
- T9 fix round 2 (12:15): swap-back path can delete the agent's version held in tmp (major) + (?x) whole-word wrap minor; QC R1 pass (120 racing runs clean)
- T4 verdicts (12:18): round 2 review APPROVE (opus), QC_PASS R2; gate ✓. Note for T6: a continuous chmod storm makes every save ChangedOnDisk — save_overwrite is the way out
- T9 verdicts (12:20): round 2 review APPROVE (opus), QC_PASS R2; gate ✓
- T12 runs in worktree .worktrees/T12 (branch t12-racing-writer, own target/) — shared compile unit with running vr-ide tasks; merge --no-ff when done (12:21)
- T12 found (12:29): Document::save strict no-lost-write fails ~13% (writer lands between the last stat and rename; D25 window is hit easily, not rarely). T4 reopens as fix round 3 (last) in a worktree after T12 merges; QC of T4 earlier missed it (its rule allowed it). D25 'accepted window' no longer holds for save.
- T12 verdicts (12:39): review APPROVE + QP1/QP2 re-checks APPROVE; QC skipped (test-only task; 50× clean runs in notes); merged --no-ff
