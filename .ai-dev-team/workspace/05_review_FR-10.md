# Review FR-10 — APPROVED (2 minor)

## Findings
[minor] crates/vr-ide/src/ui/editor_view.rs:339 — welcome screen no longer tells how to reveal the explorer
Scenario: start with no file: tree hidden (new default) and the hint list is palette / go to file / quit; nothing names ctrl+e or ctrl+b, so a new user cannot discover the explorer without the README.
Fix: add ("explorer.toggle_focus", "Toggle explorer") to `hints` (key read from the keymap, USER-1); regenerate the two empty-project snapshots once.

[minor] README.md:187, docs/keymap.md:25 — legacy-terminal side effect of ctrl+shift+e undocumented
Scenario: terminal without the kitty protocol sends ctrl+shift+e as ctrl+e. Tree shown (ctrl+b) with the editor focused, user presses ctrl+shift+e to focus it -> the tree hides. Hidden tree: it shows and focuses (correct). Grade: minor, only a terminal-limited alias, and matches the user's "shown -> hide" rule (D143, logged).
Fix (docs only): one line near the troubleshooting "A key does nothing" item: "Without the kitty protocol `ctrl+shift+e` arrives as `ctrl+e` and toggles the explorer; use `ctrl+e` or F6". If the user later wants to remove the surprise: shown-but-not-focused -> focus instead of hide (changes the stated ctrl+e rule, needs the user's OK).

## Verification
- Root cause 1 (start-up tree shown, focus Editor): real; moot now because the tree is hidden at start, and ctrl+e gives the keys (Explorer::new visible=false; toggle_focus -> show_and_focus sets Focus::Explorer, ensure_loaded).
- Root cause 2 (legacy ctrl+shift+e == ctrl+e == file.goto): real, seen in the keymap diff; fixed by rebinding ctrl+e.
- User requests met: hidden at start; hidden -> show + focus; shown -> hide + Focus::Editor (also from find bar); search panel holding the column -> show explorer + focus (D143). ctrl+e no longer file.goto, ctrl+p remains.
- Docs: README lines 85/112/185-186, docs/keymap.md rows, keymap.toml comment all match the code. No other mention of ctrl+e as Go to File left (the .worktrees/T13 copy is another task's tree).
- KNOWN list in vr-config tests/config.rs: required, the default keymap is validated against it; correct and minimal.
- Tests honest: 4 tests with red lines in the notes (3rd red obtained by restoring the old binding, stated). Test 4 is a loose "ctrl+e acts where ctrl+b does" parity check, acceptable. Snapshot change is the one hint line only. test_app hiding line removed, consistent with D142.
- Design QA (key behaviour): one key, symmetric toggle, Esc/F6/click unchanged (D100); only gap is discoverability (finding 1).

## Files read
README.md read; docs/keymap.md read; crates/vr-config/defaults/keymap.toml read; crates/vr-config/tests/config.rs read; crates/vr-ide/src/commands/explorer.rs read (plus toggle/show_and_focus context); crates/vr-ide/src/app_tests.rs read; crates/vr-ide/src/explorer_tests.rs read; both render_empty_project snapshots read; crates/vr-ide/src/ui/editor_view.rs (welcome, context) read; 04_dev_FR-10.md, decisions.md D142-D144 read.

## Rule check FR-10
| rule | result |
|---|---|
| BASE-1 | pass |
| BASE-2 | pass (additive command) |
| BASE-3 | pass (red lines present) |
| BASE-6 | pass |
| BASE-25 | pass (guard output in notes) |
| BASE-32 | pass (D100 kept; D101/AC-1 amended, D142-D144 logged; notes say "D?" labels, cosmetic) |
| AC-1 (amended) | pass (hidden at start; welcome still shows root) |
| AC-8 | pass |
| USER-1 | pass (override for ctrl+e logged; finding 1 keeps hint keymap-driven) |

## Quick pass 1 re-check
APPROVE. Diff ef16b74 read in full (README.md, docs/keymap.md, ui/editor_view.rs, ui/mod.rs, both snapshots).
- Minor 1 closed: welcome gains ("explorer.toggle_focus", "Toggle explorer"), key read from the keymap; snapshots add only that line (the trailing blank row removed is the fixed-height shift); test welcome_names_the_explorer_key has a red line and checks both the keymap hint and "ctrl+e".
- Minor 2 closed: README Troubleshooting and docs/keymap.md state exactly what the code does (legacy ctrl+shift+e = ctrl+e toggles; shown hides; use ctrl+e or F6).
- Nothing else changed; no code outside the welcome hint. Test counts (197 passed) taken from the notes.
