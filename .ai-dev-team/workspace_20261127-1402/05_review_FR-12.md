# Review FR-12: APPROVED (1 minor)

Tools available: Read, Bash, Write, Skill, SendMessage (unused), SubagentHandback.

## Findings
[minor] crates/vr-ide/src/ui/welcome.rs (compact hints) with ui/status_bar.rs notice rows - the Search hint is not visible on a legacy keyboard at 40x12 / 30x12.
Scenario: legacy keyboard, 40x12: the 3-row notice leaves room for two hints (ctrl+p, f1); `ctrl+alt+f Search` (the only reachable search key) is not shown; it appears from about 60x24 (see qa-artifacts/FR-12/review/40x12-a-welcome.txt). User decides: accept (f1 is shown) or raise Search above Explorer in the priority list for a legacy keyboard.
Fix: optional; give the reachable alternative a higher hint priority when !keyboard_enhanced, or shorten the notice to 2 rows at 40 columns.

The 2 majors and 6 minors from r4 are closed (checked against code and the real build): legacy welcome never shows an unreachable key (welcome.rs filter, test + snapshot, seen at 120x40/80x24); notice text/wrap (remedy + "(Esc)" visible at 120, 80, 60, 40, 30; only the key list is cut with an ellipsis); COLORTERM only at TrueColor (child_env; pty run: truecolor inherited -> CT=[truecolor], unset -> CT=[]; SpawnOptions unchanged, vr-term API compatible, pty test passes it through opts.env); status bar names `alt+q editor` before the size; compact welcome drops the root row; EDITOR_MIN_ROWS=3 with the 0-row guard; covered terminal gets no keys and reveal uncovers it; fast poll only for a Running tab. Nothing else changed in the diff beyond D220/D221, docs and snapshots.

## ctrl+alt bindings (D8 / collisions)
- 7 chords (f,h,s,e,k,1,`) are new; the only previous ctrl+alt default was ctrl+alt+enter; no collisions in the global, editor, explorer or terminal tables.
- A focused terminal checks only the `[terminal]` table (alt+q, ctrl+`, ctrl+shift+`, ctrl+alt+`) and sends everything else to the program: ctrl+alt+k / e / s / h in a focused terminal stayed in the terminal in the pty run (ESC ^K, ^E, ^S, ^H reach the shell; readline M-^H intact). Reserved in `[terminal]`: only ctrl+alt+` (kitty-only; ESC NUL on legacy is ctrl+alt+space, not affected). Acceptable.
- Legacy pty: ESC ^F opens search (3 sizes), ESC ^H opens Replace in Project, ESC ^E focuses the explorer, ESC ^S opens Save As, ESC ^K runs delete line (read-mode notice).

## Design QA (fresh release build, pty, TERM=xterm-256color, legacy keyboard, /bin/sh ENV=/dev/null)
Evidence: $TEAM_WS/qa-artifacts/FR-12/review/*.txt, qa.out, ct.out (text screens; the harness is text-only, no PNG).
- Welcome 120x40, 80x24: hints ctrl+p, f1, ctrl+e, ctrl+alt+f Search, ctrl+`, ctrl+q; no unreachable key; notice readable with remedy and (Esc). pass
- Welcome 40x12: hints ctrl+p, f1 only (see minor); notice: remedy, (Esc), ellipsis at the end. pass with the minor.
- ctrl+alt+f (ESC ^F) opens search at 120x40, 80x24, 40x12. pass
- Terminal panel 40x12 focused: status bar `TERM  sh  1/1 · 40×4 · alt+q editor`. pass. At 100x30: `1/1 · 100×11 · alt+q editor`.
- Notice shows (click) instead of (Esc) while the terminal is focused: existing behaviour, unchanged.

## Files read
README.md read; docs/keymap.md read; crates/vr-config/defaults/keymap.toml read; app.rs read; app_tests.rs read; commands/terminal.rs read; event_loop.rs read; search_tests.rs read; terminal_tests.rs read; ui/layout.rs read; ui/mod.rs read; ui/status_bar.rs read; ui/terminal_view.rs read; ui/welcome.rs read; ui/tests/welcome_tests.rs read; vr-term/src/terminal.rs, types.rs, tests/pty.rs read; all *.snap skipped - generated (behaviour checked on the real build screens).

## Rule check FR-12
| Rule | Result |
|---|---|
| BASE-1 scope (only listed findings + D220/D221) | pass |
| BASE-3 red/green evidence (fast-poll test green-first admitted) | pass |
| BASE-4 notes / guard output (fmt, clippy, tests in notes) | pass |
| BASE-32 decisions D220/D221, USER-1..4 (vr-ide adapts, no other app changed) | pass |
| BASE-14 / BASE-15 / COST-13 (background build, own scripts only, no process killing) | pass |
