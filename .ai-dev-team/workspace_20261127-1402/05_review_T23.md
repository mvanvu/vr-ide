# Review T23 — explorer right-click menu (code + design QA)

Tools available: Read, Bash, Write, Skill, SendMessage, SubagentHandback.

## Verdict: APPROVED

No blocker or major finding. New key bindings: none (key column reads `App::display_key`; no command or chord added).

## Findings
[minor] README.md (explorer section, new paragraph) — wording: "`up`/`down` and `enter`, or a left click, run an item" reads as if the arrows run items; they move, `enter` runs (docs/keymap.md says it right) and `home`/`end` are not mentioned.
Scenario: a reader expects `down` to run Delete. Fix: "`up`/`down`/`home`/`end` move, `enter` or a left click runs an item".

## What was checked
- Dispatch: `choose` closes the menu then `run_command("explorer.*")`; no file op code added. Delete still opens its confirm (live: "Move to Trash" / Cancel keeps the file); confinement (`ensure_within`, root refusal) is in the commands, unchanged.
- Target: right press selects the row (`move_cursor`), below the rows = cursor 0 (root); row of a multi-selection keeps it. Dimming matches `targets` / rename's depth check / `copied`.
- Routing: menu sits after the dialog and before palette/terminal/editor in `route_mouse`; in `route_key` before every other handler. Live (walk2, 80x24, 120x40, 40x12): click in editor closes and does not move focus or cursor; wheel is swallowed; ctrl+e / other key / dialog close it; right release never runs an item (only `Down(Left)` chooses).
- Terminal: a terminal program that grabbed the mouse still gets `ESC[<2;x;yM/m` (unit test + live at all three sizes), no menu opens; with the menu open a press over the terminal only closes it.
- Placement: under the pointer row, above when no room, clamped left/bottom; live right-click on the last explorer row at all three sizes keeps the clicked row readable; box always inside the screen. No right-edge case is reachable (explorer is on the left), `place` clamps anyway (`right - w` cannot underflow, `w` is capped to the screen).
- Snapshots 80x24, 120x40, 40x12 read: aligned, keys right-aligned, 40x12 box covers the root row and part of the editor (popup behaviour, clicked row visible). Dimmed Paste asserted by colour in the test.
- Design QA (popup vs palette look): same `popup`/`popup_border`/`list_selected`/`dim` tokens, keys in dim, disabled items dim and skipped; pass.
- Dev notes: tests, clippy, fmt, full suite, release build, walk 28/28 present; release binary newer than the sources. Rule compliance section present and consistent with the code.

## Files read
crates/vr-ide/src/commands/explorer_menu.rs read; ui/explorer_menu.rs read; explorer_menu_tests.rs read (test list, not every body); app.rs diff read; commands/explorer.rs diff read; commands/mod.rs, lib.rs, ui/mod.rs diffs read; terminal_tests.rs diff read; 3 snapshots read; README.md, docs/keymap.md diffs read; .ai-dev-team/* notes read (capabilities line, 04_dev_T23.md).

## Rule check T23
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 scope | pass | menu, routing, docs only |
| BASE-2 tests first / coverage | pass | 11 menu tests + snapshot test + terminal forward test cover target, dimming, keys, mouse, edges, multi-selection |
| BASE-4/5 real output | pass | notes list commands, full_suite.txt, walk.txt |
| BASE-16 reuse | pass | dispatches existing commands, `targets`, `display_key`, popup theme |
| USER-1 keys are the user's | pass | no binding added; key column from the keymap |
| Modal routing | pass | live + tests |
| D109 confinement / delete confirm | pass | unchanged commands, live confirm |
| UI sizes 80x24 / 120x40 / 40x12 | pass | snapshots + live screens |
| Docs | pass (1 minor wording) | README, keymap.md |

Screens: /home/rainy/DEV/vr-ide/.ai-dev-team/workspace/qa-artifacts/T23/review/r_*.txt (script: /tmp/claude-1000/-home-rainy-DEV-vr-ide/5ad65441-7a3e-40b9-81c7-d8de44a0c681/scratchpad/walk2.py).
