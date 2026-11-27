# Task board — run 6 (native window)

| ID | Title | Owner | Depends | Files | Tags | Rules | Verify | Status | Rounds |
|---|---|---|---|---|---|---|---|---|---|
| T1 | Frontend seam: event_loop::step + Frontend trait | fe | — | crates/vr-ide/src/{event_loop.rs,main.rs,frontend.rs,lib.rs} + tests | ui | BASE, COST, AC-W1 | cargo test -p vr-ide; guard | done | 0 |
| T2 | vr-window renderer (PixelBackend, fonts, glyph cache) + bench gate | fe | — | crates/vr-window/** (new), Cargo.toml, Cargo.lock | ui | BASE, COST, AC-W2, AC-W3 | cargo test -p vr-window; bench | in progress | 0 |
| T3 | vr-window input translation + IME state | fe | T2 | crates/vr-window/src/input*.rs, crates/vr-window/src/lib.rs | ui | BASE, COST, AC-W4, AC-W5 | cargo test -p vr-window | todo | 0 |
| T5 | Desktop entry new form + migration | fe | T1 | crates/vr-ide/src/desktop.rs, desktop_tests.rs | security | BASE, COST, AC-W7 | cargo test -p vr-ide desktop | todo | 0 |
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
