# Run 4 — task board

| ID | Title | Owner | Depends | Tags | Rules | Files | Verify | Status | Rounds |
|---|---|---|---|---|---|---|---|---|---|
| DS | UI design: AI panel, AI changes view, notify mark, palette Tab completion, restore notices, key table (.ans mockups) | fe | - | ui | DESIGN, D7–D10, USER-2, USER-5 | $TEAM_WS/02_ui-spec.md, $TEAM_WS/mockups/*.ans | design QA (sonnet reviewer) + user approval | done (DESIGN_PASS r1; approved D12) | 1 |
| TM | Threat model: ssh argument line, AI pre-image store and revert, session file | be | - | security | SEC-n | $TEAM_WS/02_threat-model.md | - | done (SEC-1..18) | 0 |
| T1 | Session store + `--session` launch + restore (D1–D3) | be | DS (notices only) | - | AC-1..4, AC-11 | crates/vr-ide/src/session.rs (new), cli.rs, main.rs, app.rs hooks | `cargo test -p vr-ide` | done (review APPROVE r2, QC_PASS; merged 0d05d5a) | 2 |
| T2 | Desktop launcher + icon (D4) | fe | T1 (relaxed: no edits to cli.rs, main.rs until T1 merged) | ui | AC-5 | crates/vr-ide/src/desktop.rs (new), assets/vr-ide.svg, cli.rs, README | `cargo test -p vr-ide` | done (review APPROVE r1, QC_PASS r1; merged 849fa1c) | 1 |
| T3 | `/ssh` argument line (D5) | be | TM | security | AC-6, SEC-1..5 | crates/vr-ide/src/commands/ssh.rs, ssh/mod.rs, docs | `cargo test -p vr-ide` | fix round 1 | 1 |
| T4 | Palette Tab completion (D6) | fe | DS | ui | AC-7 | crates/vr-ide/src/commands/palette.rs, ui/palette.rs, completion.rs (new) | `cargo test -p vr-ide` | fix round 1 | 0 |
| T5 | AI panel (D7) | fe | DS, T1 (relaxed: restore wiring after T1 merged; no edits to session.rs, cli.rs, main.rs until then) | ui | AC-8, AC-13 | crates/vr-ide/src/ai/ (new), ui/ai_view.rs (new), ui/mod.rs layout, keymap, settings | `cargo test -p vr-ide -p vr-config` | done (review APPROVE + QP1, QC_PASS r1; merged f4b7347) | 0 |
| T6 | AI changes: pre-images, list, diff, keep, revert (D8) | be+fe | T5 (relaxed: built on T5 wip c8675ed; T5 review fixes merged in before T6 review) | ui, security | AC-9, SEC-9..13 | crates/vr-ide/src/ai/changes.rs (new), ui/ai_changes.rs (new) | `cargo test -p vr-ide` | open: review CHANGES_REQUESTED, round 1 not started; QC stopped (branch run4-T6 pushed) | 0 |
| T7 | Agent notify mark (D9) | fe | T5 | ui | AC-10 | crates/vr-term (bell / OSC 9 / 777), ai tab badge, notices | `cargo test -p vr-term -p vr-ide` | done (review APPROVE r1, QC_PASS; merged 5994263) | 1 |
| T8 | Accounts per agent session via CLAUDE_CONFIG_DIR / CODEX_HOME (D11) | fe | T5 (relaxed: from T5 wip c8675ed; T5 quick pass merged in before review) | ui, security | AC-12, SEC-15..18 | crates/vr-ide/src/ai/accounts.rs (new), picker in ui/ai_view.rs, docs | `cargo test -p vr-ide` | done (review APPROVE r1 + merge 2, QC_PASS r1; merged b4f6c77) | 1 |
| T9 | Red search tests on master: search_tests::apply_reports_a_file_failing_halfway, project_replace_never_loses_a_concurrent_write ("search did not finish" at 20 s) | be | - | - | AC-11 | crates/vr-ide/src/search_tests.rs (+ product code only if a real defect) | `cargo test -p vr-ide -- search_tests` | done (review APPROVE r1; merged b277e88) | 1 |

## Notes
- phase0: paths ✓ (TEAM_BIN 1.0.41) · profile fields ✓ (2026-11-24) · local fields ✓ · test logins n-a · temporary values ✓ (none) · capabilities ✓ · gitignore ✓ · upstream ✓ (pushed b0d6d0f at 13:33; 4 local commits since) · commit identity ✓ (as run 3) · guard coverage ✓ (run 3)
- archive: workspace → workspace_20261127-1402, committed 0470b1b.
- presence: present (user typed, 15:55)
- approvals: commit per task + reinstall + push at the end (user, 14:02).
- fable-policy: auto (user, 14:35)
- user words override USER-3 for this run: mockups first, user approves (D10).
- build waits for the user's design approval (user words), incl. tasks that need no design.
- 14:08 user: AI tabs = sessions (several of one tool) → D7 amended, sent to DS. Question answered (not decided): a cswap-like account tool — options A/B/C to the user.
- 14:10 user: accounts option 2 → D11, AC-12, T8. Sent to DS and TM.
- 14:11 TM DONE SEC-1..14 (T3: 1–5, T1: 6, T2: 7, T5: 8, T6: 9–13, T7: 14). D11 addendum queued to the same reviewer. Accepted risks listed in the TM → shown to the user with the design approval (secret pre-images 0600 until Keep/Revert/close).
- 14:12 TM addendum SEC-15..18 (T8). Accepted risk for the user: removing an account deletes local tokens only (no provider revoke).
- 14:20 DS DONE (25 .ans, ui-spec; L1 A/B, L2 A/B; keys f12 / ctrl+f12 / f3 / f6 / f7 / shift+f7 / f5 / shift+f5; COST-8 one python heredoc in a throwaway script — noted, not product). Design QA sonnet.
- 14:22 verdict gate DS QA ✓ DESIGN_FAIL: 5 blockers (SEC-5/9/13/17/18 UI states missing), 2 major (SEC-12 dialog line, 2 clipped lines), minors. L1-A, L2-A best. Round 1 → same designer.
- 14:26 DS r1 DONE (12/12; 31 .ans, check 31/31). Re-check: same design QA reviewer.
- 14:27 verdict gate DS re-check ✓ DESIGN_PASS (12/12, check 31/31). Shown to the user for approval with the hand-off questions.
- 14:30 user approved design (D12), fable auto, risks D13; presence away. Commit + reinstall + push approved.
- parallel: T1 ∥ T3 ∥ T4 ∥ T5 — each in its own worktree (.worktrees/T1,T3,T4,T5; branches run4-T*; same crate → worktrees). Files mostly disjoint; shared: README, docs/keymap.md, docs/commands.md, keymap.toml, app.rs, ui/mod.rs, palette.rs (T3 /ssh entry vs T4).
- merge route: shared docs/keymap/README → each task edits only its own sections/lines; before merge-back the leader merges master into the worktree and the task's developer resolves conflicts (DONE merge). Merge order: T1, T3, T4, T5.
- T5 on opus (not high-risk → not a Fable case; T6 is case 3 → fable).
- hand-off: ready (15:54) — account 3 active (user), no open questions; confirm-on-return D15, D16
- eta: push ~18:00 (set 16:40, basis: T6 QC → round 1 (~40 min) → re-check (~15 min), T8/T9 close meanwhile)
- 14:36 user: new session asks whether to resume → D14/AC-13 into T5 (picker step; no new mockup round — designer's picker + a second step, design QA on the build).
- 14:52 T4 DONE (gate ✓: 17 new tests, related 258 pass, guard ✓, 18 pass/1 n-a). Merge notes: switch ssh_slot_at_end to T3's classifier; ai_tool_names to T5's field. flaky? ssh_tests::ac_s2_ac_s3 failed once in a related run, passed alone (watch). Review sonnet ∥ QC sonnet.
- 14:54 T1 DONE (gate ✓: 27 session tests, related 191 pass, guard ✓). Outside row: Cargo.toml/Cargo.lock toml dep (workspace dep, accepted), event_loop.rs, terminal.rs add_tab_in pub(crate). AI API: take_restored_ai / set_ai. Review opus (lock = concurrency → high-risk) ∥ QC sonnet.
- 14:56 verdict gate T4 review ✓ APPROVE (3 minor: 40x12 tag cuts the leading '/' — visible UI defect → fix; ssh path snapshot without ~ form; '<dir>' vs '<folder>'). Quick pass after QC. T2 starts now (sonnet, worktree, relaxed on T1: desktop.rs/icon first, cli wiring after T1 merge).
- 14:58 T2 DONE-pending-cli (desktop.rs 7 tests, guard ✓; red-first not followed → ledger; no StartupWMClass: GNOME Terminal server owns windows, USER-4). Waits for T1 merge to wire flags; review after wiring.
- 14:59 verdict gate T1 review ✓ CHANGES_REQUESTED (medium SEC-6: existing state dir 0775 / symlink not enforced; 3 minor: exited-shell pid cwd, saved set before write success, 2 s test not measuring; low: restore_explorer symlink mid-path). Fix round 1 after T1 QC (worktree build in use).
- 15:00 T3 DONE (gate ✓: ssh 86 / related 274 pass, guard ✓; red-first partial → ledger). Overlaps T4 in slash.rs + ui/palette.rs (merge after T4; T4's ssh_slot_at_end → args::classify at that merge). Outside row: commands/terminal.rs spawn_program_env (T5 also touches terminal.rs). Review opus (security) ∥ QC sonnet.
- 15:03 T5 DONE-pending-T1-wiring (gate ✓: vr-ide 737 / vr-config 51 pass, guard ✓, 32 pass/1 n-a). Deviations: f5/f6 unbound (T6), ai_presets→ai_tools rename (old key read), exited line at grid bottom, no ±N (T6). Flaky under load again: ssh_tests::sec_s12 (passes alone) → known-flaky, owner T3 fix round. T5 dev at ~376k → wiring goes to a fresh developer. Review opus? no — T5 not high-risk → sonnet review + sonnet QC, queued until a verifier slot frees.
- 15:06 T5 wip committed c8675ed on run4-T5. T6 (fable, case 3) and T7 (sonnet) start in worktrees from c8675ed (relaxed: T5 review fixes merged into them before their review; files mostly new). parallel: T6 ∥ T7 — T6 ai/changes.rs, ui/ai_changes.rs, tab ±N; T7 vr-term bell/OSC + attention mark; shared: ui/ai_view.rs tab label (both add a marker) → merge route: T7 merges after T6, resolves ai_view.rs.
- 15:08 verdict gate T3 review ✓ CHANGES_REQUESTED (2 major: typed line equal to a recent one runs in the recent folder; runs: line cut without … losing destination; low: bundled -I bypasses fill rule; minors: shown_base vs base_for, box height jumps, -F none). Leader adds (spec gap, narrowing): a recent line with -F <file> also only fills. Round 1 after T3 QC. T5 review (sonnet) starts now; T5 QC when a verifier slot frees.
- 15:10 verdict gate T1 QC ✓ QC_PASS (AC-1..4, AC-11; low: edit mode lost when quit dialog shows). Fix round 1 → same T1 developer (SEC-6 dir mode/symlink, pid cwd, saved-before-write, 2 s test, explorer symlink, edit mode on quit). T5 QC sonnet started.
- 15:12 verdict gate T5 review ✓ APPROVE (4 minor: ai_presets rename undocumented, history comment settings.rs:101, dead AiTool::resumes, exited session no keyboard scroll). UI trade-off (exited line covers the last 2–3 agent rows) → D15 taken while away. Quick pass + T1 wiring after T5 QC (fresh developer; first dev at ~386k).
- 15:13 T7 DONE (gate ✓: vr-term green, vr-ide 742/1 flaky real-watcher timing (passes alone; known flaky under load), guard ✓). Deviation: status badge overlays the status text on narrow widths. Review sonnet now; QC when a slot frees. T8 starts (opus: account folder deletion = data deletion) in worktree from c8675ed.
- 15:16 T1 r1 DONE (6/6, session 33 pass ×3, related 197 pass, guard ✓). Re-check: opus reviewer + QC replay (edit mode, 0775 dir). T7 DONE earlier; T8 started.
- 15:18 verdict gate T1 re-check ✓ CHANGES_REQUESTED (major: remember_edit_mode overwritten in the tick between dialog answer and exit — edit mode still lost on the real path; 5 earlier findings pass). Round 2 waits for the QC replay running on the worktree.
- 15:19 verdict gate T7 review ✓ CHANGES_REQUESTED (major: badge cuts status text; major: test asserts the wrong thing; minors: DCS BEL skip, regular terminals queue bells, OSC 9;n, comment). QC had not started → round 1 now, QC runs on the fixed tree (no edits under a running verdict).
- 15:20 verdict gate T1 QC re-check ✓ QC_FAIL (edit mode on real quit path; 0775 → 0700 pass; restore pass). Round 2 → same developer with the real-path test.
- 15:21 verdict gate T3 QC ✓ QC_PASS 39/39 (load ~40 caused 2 spurious walk timeouts, rerun green). Round 1 → fresh opus developer (first at ~278k): review majors + -I + -F fill rule + minors + 2 flaky ssh tests (known-red owner T3).
- 15:23 verdict gate T4 QC ✓ QC_PASS (all flows both sizes; ws_big_dir FAIL line = harness wall-time only; obs: /ssh host completes without trailing space / no runs: line). Quick pass 1 queued (5 developers running): 40x12 tag cuts '/', ssh snapshot ~ form, <dir>→<folder>, host completion space + runs line; combined with switching to T3's args::classify once T3 is approved (merge order T3 then T4).
- 15:24 verdict gate T5 QC ✓ QC_PASS (AC-8, AC-13, SEC-8, keys at 3 sizes; QC once started a real gemini via /usr/bin PATH and killed its own pid — noted). T5 quick pass (4 review minors + D15 exited rows) + T1 restore wiring → one fresh developer once T1 is merged and a slot frees.
- 15:26 T7 r1 DONE (6/6; vr-term green, vr-ide 743 + 1 timing flake search_tests::project_replace_never_loses_a_concurrent_write passes alone → watch). Re-check: same reviewer ∥ QC sonnet. T4 QP1 → same T4 developer (slot freed by T7).
- 15:28 verdict gate T7 re-check ✓ APPROVE (6/6 fixed). Waiting QC T7. Note: T7 touches commands/ssh.rs (SpawnExtras plumbing) — merge after T3.
- 15:30 T1 r2 DONE (remember_edit_mode stops at app.quit; 2 real-path tests red→green; session 35, related 199 pass; replay e6/recheck pass on release). Re-check: opus reviewer + QC replay.
- 15:32 verdict gate T1 re-check 2 ✓ APPROVE. Contract for T5 wiring: the AI-sessions quit question must answer by setting app.quit via dialog::answer. Waiting QC replay → commit + merge T1.
- 15:34 T1 done, committed b1d227b, merged 0d05d5a. master merged into run4-T5 clean. T5 QP1 + session wiring → fresh opus developer. T2 wiring next free slot.
- 15:36 T4 QP1 DONE (4/4; related 259 pass, guard ✓). Re-check: reviewer diff + QC the /ssh host case. T2 wip committed, master merged in clean; cli wiring → same T2 developer.
- 15:38 verdict gate T4 QP1 re-check ✓ APPROVE. Waiting QC replay of the /ssh host case. Merge order: T3 first (classifier), then T4 with the classifier switch.
- 15:40 verdict gate T4 QC re-check ✓ QC_PASS. T4 committed on run4-T4; merges after T3 (classifier switch at that merge).
- 15:42 T2 DONE (cli wiring; lib 40 + desktop_cli 2 pass; guard ✓). Review opus (uninstall removes files on the user's disk → high-risk) ∥ QC sonnet.
- 15:45 verdict gate T2 review ✓ CHANGES_REQUESTED (major: install overwrites an unmarked user vr-ide.desktop/icon/symlink silently; major: notes lack ## Rule compliance (DONE gate miss by the leader — recorded); minors: half install leaves icon, target/ install note, loose marker match). Icon design QA pass (16–48 px). Round 1 after T2 QC.
- 15:47 verdict gate T2 QC ✓ QC_FAIL (overwrites unmarked user files; rest pass). Round 1 → same T2 developer.
- 15:49 verdict gate T7 QC ✓ QC_PASS (AC-10, SEC-14 at 3 sizes; rate limit unit-test only). T7 committed on run4-T7; merges after T5 (merge order T5 → T6/T7/T8 with conflicts in ai_view.rs resolved per route).
- 15:51 T2 r1 DONE (5/5; lib 43 + cli 3 pass, guard ✓, notes table added). Re-check: opus reviewer + QC replay.
- 15:53 T3 r1 DONE (8/8 red→green; related 190 pass; flaky ssh tests fixed: test races, 5/5). Re-check: opus reviewer + QC replay of changed cases.
- 15:55 verdict gate T2 re-check ✓ APPROVE (5/5). Waiting QC replay.
- 15:57 verdict gate T2 QC re-check ✓ QC_PASS. T2 done, merged 849fa1c.
- 15:59 verdict gate T3 re-check ✓ APPROVE (2 minor: dest line found by substring; 'in <folder>' dropped at 1 detail row — hides the folder warning → quick pass after QC re-check).
- 16:01 T5 QP1 + wiring DONE (merge fix cwd: None; restore via AiSection, report-before-save; minors; D15; ai 25 pass, related 107 pass, guard ✓; gap: AI active tab not restored). Re-check: reviewer + QC replay of restore/exited.
- 16:03 verdict gate T5 re-check ✓ APPROVE (no findings). Notes: AI active tab not restored (AiSection has no index) and refused tabs are dropped on next save → D16. Waiting QC replay.
- 16:05 T8 DONE (accounts; related 27 pass; full crate 738 + 11 load-timing reds that pass at --test-threads=2; guard ✓; deviations: account→tool record file <state>/ai-accounts, '_' allowed, @Default refused, removal refused while any tab uses it, @account completion pending T4). Review waits: T5 (restore.rs rewritten) merges first, then master into run4-T8 (conflicts → T8 dev), then review opus ∥ QC. known flaky under load: search_tests (9), event_loop exit_restores…, app_tests external_modify… → sign-off watches; owner if red alone.
- 16:07 verdict gate T5 QC re-check ✓ QC_PASS (restore, refused tab notice, D15 exited, console no restore). T5 committed on run4-T5, master (T2) merged in clean; related tests running before merge to master.
- 16:09 merged T5 branch: related 148 + 2 pass. T5 done, merged f4b7347. Next: master into run4-T7 (leader) and run4-T8 / run4-T6 (developers resolve conflicts).
- 16:12 T3 committed on run4-T3; master merged into T3 (conflicts terminal.rs, docs/commands.md → T3 dev + quick pass 2 minors), T7 (clean → dev verifies), T8 (wip committed; conflict ai/restore.rs → T8 dev). Stray untracked file in .worktrees/T3 (shell accident name) left untouched, not staged.
- 15:51 account: 2 → 3 vufixecom@fixecom.vn (user asked; agents hit the weekly limit at 15:36 with account 1's reset time)
- 15:53 T7 after master merge: guard ✓, vr-term green, vr-ide related 95 + 1 known flake (ssh sec_s12, passes alone; T3's fix not merged yet). T7 done, merged 5994263.
- 15:56 T8 merge DONE (restore.rs resolved on T5's AiSection + account check; ai/session 121 pass, guard ✓); merge committed. Review opus ∥ QC sonnet.
- 16:00 verdict gate T8 review ✓ CHANGES_REQUESTED (major: picker footer cut at 80x24; minors: ready accepts unrecorded folder for any tool; index RMW no lock + read error = empty; low: busy check only this instance → D17 implement locks; TOCTOU in remove; index perms). Round 1 after T8 QC.
- 15:55 user: finish the running tasks (T3, T4, T6, T8 incl. their fix rounds), then record progress, commit and push so the next session continues. Sign-off, Phase 5, reinstall and --install-desktop move to the next session.
- 16:05 T3 merge + QP1 DONE (conflicts resolved; dest by position; 1-row folder; guard ✓; related 352 + 1 load flake search apply_reports… passes alone). Stray file in .worktrees/T3 created by T3 dev (sed w flag) — leave, removed with the worktree. Re-check: opus reviewer.
- 16:08 verdict gate T3 merge+QP1 ✓ APPROVE. T3 done, merged e6a5d7e. Next: master into run4-T4 (T4 dev resolves + switches to args::classify).
- 16:09 master into run4-T4: conflicts palette.rs, slash.rs, ui/palette.rs, docs/commands.md → T4 dev (+ classifier switch, ai_tools field).
- 16:12 T6 DONE (fable; 773 pass + new tests; caveats: red-first fail for ai/changes.rs → ledger; flaky ai_tests focused-keys; no syntax colour in diff; index not reloaded (store cleared on close); cap per process; dialog wrap differs at 80x24; outside-row files incl. vr-core; a bad perl edit broke 5 files then fixed — reviewer to look closely). wip committed, master merged in: conflicts Cargo.toml, ai/mod.rs, ai_view_tests.rs → fresh opus developer (first at ~380k). Then review opus ∥ QC.
- 16:15 verdict gate T8 QC ✓ QC_PASS (all flows). Round 1 → same T8 dev (footer cut, ready requires record, index lock, D17 cross-window lock, TOCTOU fd-relative remove, index 0600, tmp cleanup).
- 16:18 T4 merge DONE (4 conflicts resolved, classifier switched, ai_tools field; related 270 pass; guard ✓). Re-check: T4 reviewer on the merge. Unowned red check: search_tests run once on master (apply_reports_a_file_failing_halfway failed alone once at load 27).
- 16:20 known-red: crates/vr-ide/src/search_tests.rs (2 tests) — red on master 5994263+ at load ~15 (30 pass / 2 fail, 20 s waits) — until T9.
- 16:22 verdict gate T4 merge re-check ✓ APPROVE. T4 done, merged 18f9270.
- 16:25 T8 r1 DONE (7/7 red-first; ai/account/session 126 pass + search_tests red (known-red T9); guard ✓). Re-check: opus reviewer + QC replay.
- 16:28 verdict gate T8 re-check ✓ APPROVE (2 low notes: unbounded recursion depth, 0500 subdirs → report). Waiting QC replay.
- 16:31 T6 merge DONE (3 conflicts resolved; ai/attention/session/status 168 pass + search red (T9), vr-core 156, guard ✓; focused-keys flake 0/18). New flaky: ai_tests::the_panel_round_trips_through_the_session_file 1/18 — session::take_lock 10×10 ms retry vs a forked PTY child holding the lock fd (T1 area) → owner: next session (recorded). Review opus ∥ QC sonnet.
- 16:36 T9 DONE (cause: slow fsync under IO load; fix: progress-based wait, race sweep scaled to unraced replace time; 6/6 under heavy load; guard ✓). Data-safety oracle change → opus reviewer must agree it still catches lost writes. Leader runs search_tests file once in the worktree.
- 16:38 T9 worktree: search_tests 32/32 pass.
- 16:40 verdict gate T6 review ✓ CHANGES_REQUESTED (major: KNOWN_CAP silent; changes in not-yet-walked dirs lost without notice; rebase after own-save can take agent content as baseline (SEC-9); Keep on diff tab without expectation; no racing-writer stress test for Revert; diff without syntax colour (spec) — included; minors: index comment, cap per process, trash TOCTOU, directory at path swapped out, fixture counts, 80-col folder column, dialog orphan word, NFS ctime note). Round 1 after T6 QC → the T6 merge developer (opus, has the code in context; reason: original fable dev context full).
- 16:44 verdict gate T8 QC re-check ✓ QC_PASS (refusal at confirm, not at Delete — acceptable). T8 committed; master merged in: conflicts README, ai/mod.rs, ai_view_tests.rs, docs/commands.md → T8 dev (+ @account completion if small). T9 review ✓ CHANGES_REQUESTED (major: progress wait hides restart livelock; minor: single unraced sample) → round 1 same T9 dev.
- 16:47 T8 merge 2 DONE (4 conflicts resolved + @account completion; related 188 pass + search known-red; guard ✓). Re-check: T8 opus reviewer.
- 16:49 verdict gate T8 merge 2 ✓ APPROVE. T8 done, merged b4f6c77.
- 16:51 T9 r1 DONE (StallWatch high-water + 10 min ceiling, self-tests; slowest of 3 samples; search_tests 34 pass; guard ✓). Re-check: opus reviewer.
- 16:31 verdict gate T9 re-check ✓ APPROVE (reviewer's written oracle agreement in the verdict). T9 done, merged b277e88. QC T6 stopped on the user's wrap-up request.

## Handover for the next session (16:31, user: "record progress, commit & push so the next session continues")
- master b277e88 holds T1, T2, T3, T4, T5, T7, T8, T9 (each reviewed + QC'd, merged --no-ff). Pushed to origin.
- T6 (AI changes list / diff / keep / revert) is NOT on master: branch `run4-T6` (e5fafa5 = T6 wip + merge of master at f4b7347+), pushed to origin. Open:
  1. Merge master (b277e88) into run4-T6 first (T3/T4/T8/T9 landed since; expect conflicts in ai/mod.rs, ai_view.rs, keymap, docs).
  2. Fix round 1 from the opus review (16:40 board line above): KNOWN_CAP silent stop (SEC-13); changes in not-yet-walked dirs lost without notice (show "scanning… may be incomplete"); rebase after own-save can adopt agent content as baseline (SEC-9: re-check own stamp after the read); Keep on a diff tab needs the diff's expectation; racing-writer stress test for Revert (RacingWriter InPlace/TempRename, regular file + link at path); diff syntax colour (ui-spec); minors (index comment, per-process cap, trash TOCTOU, directory at path, fixture counts, 80-col folder column, dialog wrap, NFS ctime note in README).
  3. Then review re-check (opus) + QC (sonnet) — QC brief in this file's 16:31 dispatch.
- Not started (move to next session): sign-off (full suite once), Phase 5 final review (≥4 tasks + security), reinstall vr-ide from a clean checkout + `vr-ide --install-desktop`, push.
- Known flaky (watch at sign-off): ai_tests::the_panel_round_trips_through_the_session_file (session::take_lock 10×10 ms retry vs a forked PTY child holding the lock fd — owner: next session, T1 area); event_loop exit_restores… under IO load.
- Confirm with the user: D15, D16 (decisions.md). Fable used: T6 developer (case 3).
- Worktrees under .worktrees/ (T1–T9) are local only; T3 holds a stray untracked file from a sed accident (delete with the worktree). Next session: `git worktree remove` merged ones.
- Account: Claude account 3 active (user, 15:51).
