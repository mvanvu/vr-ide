# Task board — run 6 (native window)

| ID | Title | Owner | Depends | Files | Tags | Rules | Verify | Status | Rounds |
|---|---|---|---|---|---|---|---|---|---|
| T1 | Frontend seam: event_loop::step + Frontend trait | fe | — | crates/vr-ide/src/{event_loop.rs,main.rs,frontend.rs,lib.rs} + tests | ui | BASE, COST, AC-W1 | cargo test -p vr-ide; guard | done | 0 |
| T2 | vr-window renderer (PixelBackend, fonts, glyph cache) + bench gate | fe | — | crates/vr-window/** (new), Cargo.toml, Cargo.lock | ui | BASE, COST, AC-W2, AC-W3 | cargo test -p vr-window; bench | done | 1 |
| T3 | vr-window input translation + IME state | fe | T2 | crates/vr-window/src/input*.rs, crates/vr-window/src/lib.rs | ui | BASE, COST, AC-W4, AC-W5 | cargo test -p vr-window | in progress | 0 |
| T5 | Desktop entry new form + migration | fe | T1 | crates/vr-ide/src/desktop.rs, desktop_tests.rs | security | BASE, COST, AC-W7 | cargo test -p vr-ide desktop | done | 1 |
| T4a | Font settings + PTY pixel size | fe | — | crates/vr-config/**, crates/vr-term/src/terminal.rs + tests | ui | BASE, COST, AC-W6, AC-W8 | cargo test -p vr-config -p vr-term | done | 0 |
| T4 | Window app: runner, wiring, launch, quit path, font settings, PTY pixels | fe | T1, T2, T3, T5 | crates/vr-window/src/app*.rs, crates/vr-ide/src/{main.rs,cli.rs,window_frontend.rs,…}, crates/vr-ide/Cargo.toml, crates/vr-config/**, crates/vr-term/src/terminal.rs, defaults/settings.toml | ui | BASE, COST, AC-W1..AC-W8 | cargo test --workspace (related); guard both feature sets; release build | todo | 0 |
| T6 | Docs | fe | T4 | README.md, docs/*.md | docs | BASE | leader spot-check + sonnet reviewer | todo | 0 |

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
- eta: ~12:15 (set 06:10, basis: critical path T2→T3→T4→T6→sign-off+Phase 5, ~6 h, no history for window work)
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
