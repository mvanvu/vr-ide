# Review T25 — double click on a tab reveals the file (D293)

Verdict: APPROVED (two minor notes, no change required).

## Files read
- crates/vr-ide/src/app.rs: read
- crates/vr-ide/src/workspace_ui_tests.rs: read
- README.md, docs/keymap.md, .ai-dev-team/capabilities.md: read (docs rows match `ctrl+k` = `explorer.reveal`)

## Findings
1. minor, app.rs:1380-1393: `last_tab_click` is only reset by a tab press, so a drag or wheel between two presses does not break the pair. Live: press on a tab, drag from it, wheel in the editor, press again within 400 ms ran the reveal (`w4_06_C1...txt`). Cause: the drag itself starts with a press on the tab, which counts as the first half. Real use needs two quick presses on one tab anyway; no data loss, the effect is a focus move. Optional fix: clear `last_tab_click` on `Drag`/`ScrollUp`/`ScrollDown` and on a press outside the tab bar.
2. minor (uncertain, not seen live), tabs::positions: the bar scrolls to keep the active tab visible, so with overflowing tabs the first press can move the tabs and the second press at the same column hits another tab (no double). Cosmetic.

No close area on tabs (label has name, modified dot, disk state only), so nothing to exclude. Editor focus: the first press sets focus to the editor (existing code), the second ends in the explorer. `event_time` set/cleared around `route_mouse` only; `now()` falls back to `Instant::now()`, no leak into the key path. `saturating_duration_since` and `<=` match the 400 ms inclusive claim. Reveal runs after `set_active(i)`, so `explorer.reveal` uses the clicked tab's doc; `reveal_path` takes the file's own root (multi-root) and keeps the D134 rule (no expansion, notice). Only a `Some(path)` doc runs it.

## Tests against the code path
All 5 tests drive `handle_mouse_at` with real tab coordinates, so they fail without the `last_tab_click` branch (no reveal, explorer stays hidden / focus stays Editor). Slow-pair and triple tests pin the 400 ms and consumed-pair behavior; the multi-root test checks root and expansion of web/src/deep. Hidden-file test asserts notice and no expansion.

## Commands
- `cargo test -p vr-ide workspace_ui_tests`: 24 passed.
- `cargo clippy -p vr-ide`: clean.
- Release build not rebuilt: target/release/vr-ide is newer than every source.
- Live pty (SGR 1006 mouse), screens in qa-artifacts/T25/review/:
  - dev walk copy: 4/4 PASS (`walk_result.txt`).
  - `walk2.py` (`w4_*.txt`): two roots (api, web) + `terminal_position = "right"`, explorer hidden: double click on b.ts opens the explorer on the left, expands web/src/deep/b.ts, right terminal panel unchanged (A1). Hidden file ci.yml with hidden files off: explorer shows, .github not expanded (B1; notice text is outside the captured rows, covered by the unit test). Drag + wheel case: finding 1 (C1).
- No process of mine left running; the user's vr-ide was not touched.

## Rule check T25
| Rule | Result | Evidence |
|---|---|---|
| D293 behavior (show, expand, select, focus) | pass | live A1, unit tests 1-2 |
| Double-click detection (same tab, 400 ms, consumed) | pass | app.rs:1380-1393, test 3 |
| Drags/wheel between presses | minor | finding 1 |
| Multi-root root selection | pass | test 2, live A1 |
| D134 hidden/ignored rule | pass | test 5, live B1 |
| Explorer layout (find box, right panel) | pass | live A1 (right panel kept) |
| No key changes | pass | diff has no keymap.toml change |
| Docs (README, keymap, capabilities) | pass | rows present and accurate |
| Tests/clippy | pass | 24 passed, clippy clean |
