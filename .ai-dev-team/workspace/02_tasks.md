# Task board — run 6 (native window)

| ID | Title | Owner | Depends | Files | Tags | Rules | Verify | Status | Rounds |
|---|---|---|---|---|---|---|---|---|---|
| T1 | Frontend seam: event_loop::step + Frontend trait | fe | — | crates/vr-ide/src/{event_loop.rs,main.rs,frontend.rs,lib.rs} + tests | ui | BASE, COST, AC-W1 | cargo test -p vr-ide; guard | done | 0 |
| T2 | vr-window renderer (PixelBackend, fonts, glyph cache) + bench gate | fe | — | crates/vr-window/** (new), Cargo.toml, Cargo.lock | ui | BASE, COST, AC-W2, AC-W3 | cargo test -p vr-window; bench | done | 1 |
| T3 | vr-window input translation + IME state | fe | T2 | crates/vr-window/src/input*.rs, crates/vr-window/src/lib.rs | ui | BASE, COST, AC-W4, AC-W5 | cargo test -p vr-window | done | 0 |
| T5 | Desktop entry new form + migration | fe | T1 | crates/vr-ide/src/desktop.rs, desktop_tests.rs | security | BASE, COST, AC-W7 | cargo test -p vr-ide desktop | done | 1 |
| T4a | Font settings + PTY pixel size | fe | — | crates/vr-config/**, crates/vr-term/src/terminal.rs + tests | ui | BASE, COST, AC-W6, AC-W8 | cargo test -p vr-config -p vr-term | done | 0 |
| T3b | Chord parity TUI vs window (shifted symbols, shift+space) | fe | T3 | crates/vr-ide/src/event_loop.rs, crates/vr-window/src/input/key.rs + tests | ui | BASE, COST, AC-W4 | cargo test -p vr-ide event_loop; cargo test -p vr-window input; parity harness rerun | done | 1 |
| T4 | Window app: runner, wiring, launch, quit path, font settings, PTY pixels | fe | T1, T2, T3, T4a, T5 (T3b relaxed: worktree, no edits to event_loop.rs 15-110 or vr-config keys.rs) | crates/vr-window/src/app*.rs, crates/vr-ide/src/{main.rs,cli.rs,window_frontend.rs,…}, crates/vr-ide/Cargo.toml, crates/vr-config/**, crates/vr-term/src/terminal.rs, defaults/settings.toml | ui | BASE, COST, AC-W1..AC-W8 | cargo test --workspace (related); guard both feature sets; release build | done | 2 |
| T6 | Docs | fe | T4 | README.md, docs/*.md | docs | BASE | leader spot-check + sonnet reviewer | done | 1 |
| FR-1 | Phase 5 fixes: signals, font size fallback, super chords + minors | fe | Phase 5 | crates/vr-window/src/{app.rs,backend.rs,font.rs,input/key.rs,input/ime.rs}+tests, crates/vr-ide/src/{window_frontend.rs,commands/edit.rs,ui/dialog.rs,commands/terminal.rs,desktop.rs}+tests, README.md, crates/vr-config defaults+settings.rs (doc line) | ui | BASE, COST, AC-W4, AC-W5, AC-W8 | related tests; guard; window smoke | in progress | 0 |

## Acceptance criteria (from the spec)
- AC-W1: With a display, `vr-ide` opens a native window running the full IDE; `--tui` or no display runs the terminal UI unchanged.
- AC-W2: Text is drawn on a cell grid from the configured font (family/size/line height, HiDPI scale), with wide and combining graphemes in their cells and the theme's colours.
- AC-W3: A full redraw at the machine's resolution stays ≤ 16 ms (best effort; measured), only changed cells are redrawn.
- AC-W4: Every chord the OS delivers reaches the app as a crossterm KeyEvent with full modifiers and press/repeat/release; ctrl chords use the logical key when Latin/ASCII, else the US physical key.
- AC-W5: IME preedit is drawn underlined at the cursor and never sent; committed text goes in once as per-character key events; the candidate window follows the cursor.
- AC-W6: Mouse, wheel, clipboard (arboard), title, cursor shape, attention request and PTY pixel size work in the window.
- AC-W7: `--install-desktop` writes the direct-launch entry; a vr-ide-written gnome-terminal entry is rewritten on the first window start with a notice; other entries untouched.
- AC-W8: Window close and SIGTERM go through the ctrl+q quit path; font settings with a missing family warn and fall back.

## Notes
- TEAM_BIN → /home/rainy/.claude/plugins/cache/mvanvu/ai-dev-team/1.0.45/skills/project-profiling/references
- archive: workspace → workspace_20261128-0608 (uncommitted; first commit of the run is the archive alone)
- phase0: paths ✓ · profile fields ✓ (tui) · local fields ✓ · upstream ✓ (0/0) · commit identity ✓ (as run 5) · display ✓ (wayland-0 + :0, 1920x1080) · no IME daemon running (IME checks manual) · fc-match monospace = DejaVu Sans Mono · monitor on (local.md)
- presence: away (user, 06:09: "Làm đi, tôi treo máy")
- fable-policy: auto (carried from run 5 while away — confirm-on-return)
- commits: one local commit per task when done (reversible, confirm-on-return); push held for user
- parallel: T1 ∥ T2 — files disjoint (vr-ide vs new crate vr-window; T2 owns Cargo.toml/lock), test_isolation per-crate
- 06:10 T1 started (developer opus), T2 started (developer opus); task bases: committed per task, HEAD 6c53fdc
- eta: ~11:00 (set 06:10, updated 07:01: T1–T3, T4a, T5, T3b done; remaining T4 (~2 h incl. review/QC) → T6 → sign-off + Phase 5)
- 06:20 T1 DONE gate ✓ (34 rule rows, guard-line ok; workspace guard red only in in-progress vr-window). Review (sonnet) + QC (sonnet) started. T5 waits for T1 commit (same crate vr-ide).
- 06:21 verdict gate T1 review ✓ APPROVED (1 minor: attention count, carried to T4 brief). Waiting for QC T1.
- 06:22 verdict gate T1 QC ✓ QC_PASS (pty driver: title, cursor shapes, save, paste, terminal, resize, dirty quit, restore). T1 done, committed. T5 starts.
- 06:22 T5 started (developer opus, high-risk). parallel: T2 ∥ T5 — files disjoint (vr-window+Cargo vs desktop.rs).
- 06:24 T2 DONE gate ✓ (bench gate passed: full redraw 4.8 ms @1080p, 21 ms @4K best effort; leader viewed crop.png). Review (sonnet) + QC (sonnet) started. T3 waits for T2 commit (same crate, no edits under a running verdict).
- 06:26 verdict gate T2 review ✓ CHANGES_REQUESTED (2 major, 2 minor). Fix round waits for QC T2.
- 06:27 verdict gate T2 QC ✓ QC_PASS (PNGs a–f, bench 4.77 ms reproduces; observation: default font resolved FreeMono, not fc monospace). Fix round 1 sent to T2 developer: 2 major + 2 minor + default font (spec Settings).
- 06:27 T5 DONE gate ✓ (31 rows, guard green). Review (opus, high-risk) + QC (sonnet, temp XDG_DATA_HOME) started.
- 06:28 verdict gate T5 QC ✓ QC_PASS (install form, validate, idempotent, foreign refused, uninstall, real ~/.local untouched; migration unverified → T4 qc). Waiting for opus review.
- 06:29 verdict gate T5 review ✓ CHANGES_REQUESTED (1 major from my brief, 1 minor taken, 1 low left). Fix round 1 sent to T5 developer.
- 06:32 T5 round 1 DONE (23 tests, guard 0). Re-check sent to the opus reviewer and the QC (install form changed).
- 06:32 T2 round 1 DONE (31 tests, guard 0; adds fontconfig-parser =0.5.8, already in lock). Re-check sent to the reviewer and QC.
- 06:32 verdict gate T5 re-check review ✓ APPROVED (T4 brief must switch launches_in_window to cfg!(feature="window")). Waiting for QC re-check.
- 06:32 verdict gate T5 QC re-check ✓ QC_PASS. T5 done, committed.
- 06:32 verdict gate T2 QC re-check ✓ QC_PASS (default font DejaVu Sans Mono = fc-match). Waiting for review re-check.
- 06:33 verdict gate T2 re-check review ✓ APPROVED (1 minor → report). T2 done, committed. T3 starts.
- 06:34 T3 started (developer opus). T4 split: T4a (vr-config font settings + vr-term PTY pixels, sonnet, small) started now. parallel: T3 ∥ T4a — files disjoint (vr-window+Cargo vs vr-config/vr-term), per-crate tests.
- 06:38 T4a DONE gate ✓ (guard green). Review (sonnet) + QC (sonnet) started.
- 06:39 verdict gate T4a review ✓ APPROVED (2 minor: test cases → quick pass after QC; caller → T4). Waiting for QC.
- 06:40 verdict gate T4a QC ✓ QC_PASS (warnings name keys; /set unproven at runtime, statically same check). Quick pass 1 (test cases) sent to T4a developer.
- 06:41 T4a quick pass (tests only) spot-checked by the leader on the diff (4 cases added, no code). T4a done, committed.
- 06:45 T3 DONE gate ✓ (49 tests, guard 0, winit =0.30.13 no GTK). Review (sonnet, focus TUI/window chord parity) + QC (sonnet, parity harness TUI kitty decode vs window translate) started.
- 06:47 verdict gate T3 review ✓ APPROVED; finding 1 (chord parity, vr-ide event_loop.rs) moved into T4's scope; shift+space → quick pass after QC; AltGr → tracked for Windows port.
- 06:49 verdict gate T3 QC: QC_FAIL on 2 chords — cause is vr-ide chord_from_key (TUI drops SHIFT), not T3's code; T3's own criteria pass (47/49 equal, IME pass). T3 done and committed; the parity defect becomes fix task T3b (sonnet, small) before T4 (shared compile units).
- 06:50 T3b started (developer sonnet). monitor scan started (sonnet). monitor-last: 06:50 (0 findings, 4 notes)
- 06:53 T3b DONE (54/54 parity rows equal per dev). Re-check sent to T3's reviewer and QC.
- 06:53 verdict gate T3b QC ✓ QC_PASS (53/53 equal, typing unchanged). Waiting for review re-check.
- 06:53 verdict gate T3b review re-check ✓ CHANGES_REQUESTED (user keymap ctrl+{ regression). Fix round 1 sent to the T3b developer (adds crates/vr-config/src/keys.rs to its files).
- 06:54 T4 started (developer opus) in its own git worktree from f1279ad — relaxed over T3b (shared compile unit vr-ide; worktree isolates; brief forbids chord_from_key/keys.rs). Merge route: T3b commits on master first; T4 branch merges master before its merge back.
- 06:58 T3b round 1 DONE (normalise in KeyChord::new; 1141 vr-ide lib tests, guard 0). Re-check sent to reviewer (incl. USER-5 oracle edit) and QC (parity + runtime ctrl+{ binding).
- 06:59 verdict gate T3b review re-check ✓ APPROVED (USER-5 oracle same key, canonical). Waiting for QC re-check.
- 07:01 verdict gate T3b QC round 1 ✓ QC_PASS (parity all equal; runtime ctrl+{ binding fires for 3 byte forms, control unchanged). T3b done, committed. T4 branch must merge master before merging back.
- 07:30 T4 DONE gate ✓ (46 rows, guard + no-default-features clippy + release build green; leader viewed x11.png). Committed wip 518833c on the worktree branch, merged master (9fb8efa, clean). Review (opus: quit path guards unsaved work, signals) + QC (sonnet, real window X11/Wayland) started. T6 docs started (sonnet), relaxed over T4: docs files only.
- 07:31 T6 DONE (README, keymap.md, commands.md). Its sonnet review waits for T4's verdicts (fix rounds may change documented behaviour).
- 07:34 verdict gate T4 review ✓ CHANGES_REQUESTED (1 blocker: lost display → unsaved work lost; 1 major IME area; 2 minor). Fix round waits for QC T4. T4 developer at ~283k context → fix round goes to a fresh opus developer briefed from 04_dev_T4.md (COST-16).
- 07:35 verdict gate T4 QC ✓ QC_PASS (X11+Wayland real runs; close/SIGTERM prompts, recovery written; PTY pixels; clipboard; migration notice; font warning. Unproven: IME, HiDPI, Wayland CSD, attention, cursor blink → manual list). Fix round 1 → fresh opus developer in the worktree.
- 07:36 verdict gate T6 review ✓ CHANGES_REQUESTED (1 major, 3 minor). Fix round 1 sent to the T6 developer. T6 commits after T4 merges.
- 07:37 verdict gate T6 re-check ✓ APPROVED (docs-only task; no qc per docs route). Commit right after T4 merges.
- 07:44 T4 round 1 DONE (4 findings, red→green; guard + both clippy green; Wayland display-loss run: normal exit, session saved). Open: on X11 Xlib's IO error handler exit(1)s before run_app returns → recovery skipped on X server loss; asked the opus reviewer for a fix-or-limit opinion. Re-check sent to reviewer + QC.
- 07:45 verdict gate T4 review re-check ✓ APPROVED. Fix task T4b added (from the reviewer: X11 IO-error emergency recovery write; SIGTERM default only after finish) — starts after T4 merges. Waiting for QC re-check of T4.
- 07:49 verdict gate T4 QC re-check ✗ QC_FAIL (Wayland headless compositor killed → vr-ide keeps running, idle). Fix round 2 to the round-1 developer, with the planned T4b items folded in (same files: X11 IO-error recovery hook, SIGTERM default after finish); T4b row removed.
- 07:58 T4 round 2 DONE: Wayland QC_FAIL attributed to qc's run.sh overriding WAYLAND_DISPLAY (dev replay shows exit within 1 s with the right display); X11 IO-error recovery hook added (Xephyr kill → recovery written); SIGTERM default after finish. Sent to QC to confirm/withdraw + X11 case, and to the opus reviewer.
- 07:59 verdict gate T4 QC re-check 2 ✓ QC_PASS (qc withdrew the Wayland fail: own script bug; X11 Xephyr kill → recovery written). Waiting for the opus review re-check.
- 07:59 verdict gate T4 review re-check 2 ✓ APPROVED. T4 done: squash-merged the worktree branch into master as one commit (deviation from --no-ff: the branch carries wip commits; squash keeps one reviewed commit per task). T6 committed after it.
- 08:00 all 8 tasks done (T1 T2 T3 T3b T4a T5 T4 T6). Sign-off qc (sonnet, full suite once) + Phase 5 unattended via Agent: correctness (opus), ui (opus) on 6c53fdc..c945a44. T4 worktree left in place (locked by the harness).
- 08:05 Phase 5 ui returned 1 high + 4 major + 3 minor (06_ui_findings.md). Verifiers start when the correctness dimension reports (cap).
- 08:07 Phase 5 correctness returned 1 high (= U1) + 2 major + 1 minor. Verification: U1/C1 confirmed by two independent dimensions; remaining 6 majors verified by 3 opus verifiers, 2 findings each (cap: max_verifiers 4 with sign-off running; deviation from one-per-finding logged).
- 08:08 Phase 5 verified: confirmed C1/U1 (high), U2, C2 (major); minors U3–U8, C3, C4. FR-1 (fix task) starts after sign-off reports. 06_final_review.json written.
- 08:12 sign-off ✓ SIGNOFF_PASS (1728 passed, 0 failed, 18 ignored; guard both feature sets; release build; window + --tui walks). FR-1 started (developer opus: signal/data-safety).
