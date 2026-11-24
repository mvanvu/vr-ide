# FR-10 dev notes
- start: read brief, board row
- step: copied pty harness to qa-artifacts/FR-10/h.py
- step: before-replay done (before_legacy.txt, before_kitty.txt): launch / ctrl+shift+e (legacy=ctrl+e goto) / palette-close fail; click, F6 work
- step: scope addition from leader (explorer hidden at start, ctrl+e toggles show+focus / hide+editor)
- red: explorer_is_hidden_at_start — assertion failed: !app.explorer.visible
- green: explorer_is_hidden_at_start (Explorer::new visible: false)
- red: ctrl_e_shows_and_focuses_the_explorer_so_arrows_move — assertion failed: app.explorer.visible (ctrl+e opened Go to File)
- green: same, after explorer.toggle_focus + keymap ctrl+e rebind (vr-config tests/config.rs KNOWN gains the name)
- red: ctrl_e_hides_the_explorer_and_returns_focus_to_the_editor — panicked at explorer_tests.rs:1691 (checked with the old ctrl+e = file.goto line put back for the run; restored after)
- green: same with ctrl+e = explorer.toggle_focus
- red: ctrl_e_works_wherever_ctrl_b_does — find bar (shown true) left: FindBar right: Editor (and on the old keymap ctrl+e did nothing to the side column)
- green: toggle_focus hide always gives the editor the keys
- step: decisions logged (3 x D? (FR-10)); docs README/docs/keymap.md/keymap.toml updated; snapshot render_empty_project regenerated once (welcome hint lost 'ctrl+e /')
- step: release build (run-heavy) `Finished release profile [optimized] target(s) in 31.13s`; after-replay done

## Repro (BASE-37)
Harness: `.ai-dev-team/workspace/qa-artifacts/FR-10/fr10.py <bin> [kitty]` — pty, TERM=xterm-256color, 80x24; without `kitty` the harness never answers the kitty query (like the user's GNOME-style terminal), with `kitty` it answers `CSI ? 0 u`. Project a/(x.txt), b.txt, c.txt. "WORKS" = Down then Right expand a/ (x.txt appears in the tree column).

Before — installed `~/.cargo/bin/vr-ide` (master dbf08cc), `before_legacy.txt` / `before_kitty.txt`:
```
FAILS  launch, no click (focus at start-up)              [both]
WORKS  click the root row                                 [both]
FAILS  ctrl+shift+e (legacy: arrives as ctrl+e)           [legacy: Go to File opens; kitty: WORKS]
FAILS  ctrl+shift+e then Esc                              [both; Esc = back to editor, D100]
WORKS  F6 focus.cycle                                     [both]
FAILS  ctrl+p then Esc (palette closed)                   [both; focus returns to the editor]
WORKS  app-cursor arrows ESC O B / ESC O C, click b.txt + Up, Enter + F6 + Home, shift+Down, ctrl+w last tab  [both]
```
After — `target/release/vr-ide` with the fix, `after_legacy.txt` / `after_kitty.txt` (old scenarios run with SHOW=1: ctrl+b first, since the tree now starts hidden):
```
HIDDEN  explorer at start-up                                                     [both]
WORKS   ctrl+e (show + focus), Down Right                                        [both]
WORKS   ctrl+e, app-cursor arrows ESC O B / ESC O C                              [both]
WORKS   ctrl+e shows, ctrl+e hides                                               [both]
WORKS   open b.txt, ctrl+e hides (typing edits), ctrl+e shows, arrows move       [both]
old section: click / F6 / app-cursor / open / shift / ctrl+w WORKS;
  "ctrl+b then arrows" FAILS by design (ctrl+b shows without focus, unchanged per the leader);
  legacy ctrl+shift+e on a shown tree = ctrl+e = hides it (D? FR-10); kitty ctrl+shift+e WORKS;
  Esc / palette close return to the editor (D100, unchanged)
```

## Root cause
1. Start-up: the tree was shown (D101) with its cursor row highlighted, but `App::new` gave the editor the keys (`focus: Focus::Editor`); with no file open the arrows went to an empty editor. Closing the palette returns to that same editor focus.
2. The user's terminal has no kitty keyboard protocol: `ctrl+shift+e` (explorer.focus) arrives as `ctrl+e`, bound to `file.goto` — so no default key could focus the tree; only a mouse click or F6 did. Arrow parsing (ESC [ A and ESC O A, modifiers) and the built-in explorer keys were fine.

Fix (with the user's 21:02 scope addition): explorer hidden at start (`Explorer::new` visible: false); `ctrl+e` = new `explorer.toggle_focus` (hidden, or the search panel in the column → show + focus; shown → hide + editor focus). The key that reveals the tree now gives it the keys, in legacy and kitty terminals alike. No start-up focus rule added (the tree is not shown at start any more).

## Files changed
- crates/vr-ide/src/commands/explorer.rs — `explorer.toggle_focus` command + `toggle_focus`; `Explorer::new` visible false
- crates/vr-config/defaults/keymap.toml — ctrl+e → explorer.toggle_focus (next to ctrl+b, with a why-comment); ctrl+e no longer file.goto
- crates/vr-config/tests/config.rs — KNOWN command list gains `explorer.toggle_focus` (the default keymap is validated against it)
- crates/vr-ide/src/app_tests.rs — `test_app` no longer hides the explorer (now the default)
- crates/vr-ide/src/explorer_tests.rs — 4 tests (below)
- crates/vr-ide/src/ui/snapshots/vr_ide__ui__tests__render_empty_project_{80x24,40x12}.snap — welcome hint "ctrl+e / ctrl+p Go to file" → "ctrl+p Go to file" (the only change; regenerated once)
- README.md (Quick start line, Files table Go to File row, Explorer table ctrl+e / ctrl+b rows), docs/keymap.md (file.goto row, new explorer.toggle_focus row)
- .ai-dev-team/workspace/decisions.md — 3 x `D? (FR-10)`
Reuse: show_and_focus / ensure_loaded reused; harness copied from qa-artifacts/signoff/h.py. capabilities.md unchanged (nothing reusable added).

## Tests (red → green)
| test | red | green |
|---|---|---|
| explorer_is_hidden_at_start | `assertion failed: !app.explorer.visible` | ok |
| ctrl_e_shows_and_focuses_the_explorer_so_arrows_move | `assertion failed: app.explorer.visible` | ok |
| ctrl_e_hides_the_explorer_and_returns_focus_to_the_editor | panicked at explorer_tests.rs:1691 (run with the old `ctrl+e = file.goto` line put back, then restored) | ok |
| ctrl_e_works_wherever_ctrl_b_does | `find bar (shown true) left: FindBar right: Editor` | ok |

Final runs:
```
cargo test -p vr-ide --lib explorer_tests::   test result: ok. 41 passed; 0 failed
cargo test -p vr-ide --lib app_tests::        test result: ok. 24 passed; 0 failed
cargo test -p vr-ide                          test result: ok. 178 passed; 0 failed; 3 ignored
cargo test -p vr-config                       test result: ok. 3 passed / ok. 16 passed
```

## Guard
```
rustfmt --edition 2024 <changed .rs files>; cargo fmt --all --check   -> no output
cargo clippy -p vr-ide --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.49s
```

## New states (round 1)
None: no thread, job or kept value; `explorer.toggle_focus` only flips `explorer.visible` / `search.visible` / `focus`, like explorer.toggle and explorer.focus. A second ctrl+e toggles back; Esc in the tree goes to the editor (D100); no I/O beyond `ensure_loaded` (the existing explorer.focus path, which reports an unreadable root).

## Rule compliance
| rule | status | evidence |
|---|---|---|
| BASE-1 | pass | only explorer keys/focus + key docs; theme/styles untouched (T13) |
| BASE-2 | pass | no contract changed; additive command `explorer.toggle_focus` |
| BASE-3 | pass | red line per test above, one test at a time |
| BASE-4 | pass | trimmed outputs above |
| BASE-5 | pass | release build via run-heavy.sh |
| BASE-6 | pass | English, why-comments only |
| BASE-10 | pass | cargo test -p vr-ide (+ vr-config, whose files changed) |
| BASE-11 | pass | no tree-rewriting git commands used |
| BASE-14 | pass | release build and replays as background tasks |
| BASE-15 | pass | stopped my own replay b1yzmpbhi with TaskStop; nothing left running |
| BASE-16 | pass | Reuse line above |
| BASE-17 | pass | no installs |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | progress lines above |
| BASE-24 | pass | nothing staged or committed |
| BASE-25 | pass | guard above |
| BASE-32 | pass | D100 keys kept; D101/AC-1 amended per user 21:02 and logged |
| BASE-38 | pass | rustfmt only on changed files |
| COST | pass | related tests, one full run, one release build, snapshots regenerated once |
| new-states | pass | section above (none added) |
| AC-8 | pass | tree usable by keyboard: ctrl+e then arrows (pty replay, both terminal modes) |
| USER-1 | pass | VS Code keys kept except ctrl+e, overridden by the user's explicit key (logged) |
n-a: none
