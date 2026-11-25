# Review T19 — terminal panel on the right (D227)

Verdict: APPROVED (2 minor, 1 note). Tools: Bash, Read, Skill, SendMessage, SubagentHandback.

## Files read
crates/vr-config/src/settings.rs, lib.rs, defaults/settings.toml, defaults/keymap.toml: read. crates/vr-config/tests/config.rs: skipped (tests; passes). crates/vr-ide/src/ui/layout.rs, ui/mod.rs, ui/terminal_view.rs, commands/terminal.rs, commands/slash.rs, app.rs, lib.rs: read. terminal_right_tests.rs, slash_tests.rs, app_tests.rs, terminal_tests.rs: skimmed (the find-box test read). Snapshots right 80x24 / editor 80x24 and the palette diff: read. docs/commands.md, docs/keymap.md, README.md: read. FR-13 files: ignored.

## Checks run
cargo test -p vr-ide terminal_right: 13/13 ok. cargo test -p vr-config: 16+22+8 ok. Live pty on target/release/vr-ide (built 12:39, after the last source edit 11:57): dev walk.py 13/13; edge.py, find.py, focus*.py in qa-artifacts/T19/review/ (screens edge_*.txt, find_80x24_right.txt).

## Layout math
- right_fits(column) with column = screen minus explorer, 60 min. right_cols = round(col*pct), >= 20, <= col-21. Live: 120x40 = 48 cols (sep 71); 80x24 = 32 (PTY 19x32); 78x24 = 31; 60x12 = 24 (PTY 7x24); 59x12 and 40x12 fall back to the bottom (PTY 4x59, 4x40) with a notice saying why. Explorer open: code path (compute subtracts the explorer before right_in) is correct; not walked live.
- PTY: terminal::fit every frame; new terminal uses spawn_size (hidden panel only, so the unreduced editor is used: correct).
- Drag: separator handled before the grid and the captured-mouse path; live with a program in mouse mode (1000+1006): in-grid click reaches it at the right cell, a press on the separator and the drag never reach it, separator moved to col 60. Clamps both ends via right_cols. Tab bar is not a handle on the right. There is no explorer edge drag in the code base, so no conflict.
- grow/shrink, /term right|bottom, /set apply, validation 10..=90: correct, tests cover them. Find box (D275) sits left of the separator over the editor; live at 80x24 it fits in 47 columns.
- ctrl+f7 is the only new binding (keymap.toml, docs/keymap.md, keyset test); nothing else added.

## Findings
[minor] crates/vr-ide/src/commands/terminal.rs:1162-1180 — drag quantised to whole percents
Scenario: editor column 200 wide, drag the separator by one column → percent rounds (1 % = 2 columns), so the separator trails the mouse by up to a column and snaps; at 300+ columns by more.
Fix: optional; keep the percent but accept it, or store columns for the session. User decision.

[minor] crates/vr-ide/src/commands/terminal.rs:337-363 — grow / shrink do not clamp to what is drawn
Scenario: 80-column editor (drawable max 74 %): ctrl+f8 pressed 8 times stores 80 %, drawn 74 %; ctrl+f9 then needs two presses before the panel visibly shrinks. The drag path does clamp; the keys do not (same at the bottom, pre-existing).
Fix: clamp the stored percent to right_cols(column, 90) / right_cols(column, 10) as resize_to does.

[note, user decision] ctrl+f7 (like the existing ctrl+f8/f9) is not seen while a terminal has the keys
Scenario: terminal focused, ctrl+f7 → the shell receives ESC[18;5~ (live, focus2.py); the user must press ctrl+` first. Consistent with docs ("every key goes to the program except ctrl+` and [terminal] keys") and with grow/shrink, so not filed as a defect. Adding the three to the `[terminal]` table would let the panel be moved from inside it.

## Docs
docs/commands.md, docs/keymap.md, README.md, settings.toml comments agree with the code. docs/keymap.md find-box sentence is one very long line (style only).

## Rule rows
Developer's Rule compliance section is present and each pass has evidence; BASE-10/14/15/16/17/18/19, COST: no contrary evidence (no leftover vr-ide process after my runs; my own sessions killed).

## Rule check T19
| Rule | Result | Evidence |
|---|---|---|
| BASE-10 related tests + guards | pass | notes list tests, clippy, fmt; my 13 + config tests green |
| BASE-14 no sleep/poll loops | pass | no loops added in product code |
| BASE-15 stop only own processes | pass | walk kills its session |
| BASE-16 reuse | pass | explorer edge style, Settings checks, apply_settings reused |
| BASE-17 no SDK install | pass | none |
| BASE-18 no own agents | pass | none |
| BASE-19 notes + progress | pass | 04_dev_T19.md |
| COST heavy via run-heavy | pass | notes: full suite, release build |
| TDD tests first | pass | tests cover each new path |
| Snapshots reviewed | pass | 3 new snaps and palette diff read |
| USER-1 hints from keymap | pass | notice and hints use key_hint |
| Scope (no FR-13 files, no commits) | pass | diff shows only T19 files besides FR-13's |
| Secure by default | n-o | settings validation only |
| UI design gate | pass | TUI layout per D227; separator drawn like the explorer edge |

## Design QA
Layout, separator colour (focus / dim), tab bar, notice wording and find-box placement match D227/D275/D277 at 120x40, 80x24, 60x12, 59x12, 40x12; no clipped or overlapping cells seen in the screens.
