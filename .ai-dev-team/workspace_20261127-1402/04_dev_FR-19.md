# FR-19 Drag to reorder terminal tabs (AC-F3)
Worktree `.worktrees/FR-19` (branch fr-19 from master 6c7dd87). Side: fe. Skills loaded: `ai-dev-team:implementation`.
Tools available: Bash, Read, Edit, Write, Skill, ToolSearch (deferred: TaskStop, LSP, ...), Agent (not used, BASE-18).

## Progress
- Read spec AC-F3, rules_run3.md, ui/terminal_view.rs (tab_bar/BarTab/TabBar), commands/terminal.rs (handle_mouse, bar_hit, resize), app.rs route_mouse, README/docs keymap lines. Editor tabs (ui/tabs.rs) have no drag-reorder: nothing to reuse beyond `tab_bar` + `bar_hit`.
- Red: new crates/vr-ide/src/terminal_tab_drag_tests.rs (9 tests) — 7 FAILED with order unchanged (left: ["one","two","three"]), 2 ok (press-without-move, no-flip trivially).
- Green: drag_tab_to + tab_drag state in commands/terminal.rs; 9/9 terminal_tab_drag tests pass. README + docs/keymap.md updated.
- Related tests, guard, build, pty walk on the final code (below). Done.
- Round 1: merge conflict in commands/terminal.rs resolved; related tests + guard green. QC e1 red in a new unit test and on the binary; fixed (leftward mirror rule); docs updated; replaying qc_walk.py on the rebuilt binary.

## Summary
- Files changed (worktree, diff vs HEAD 6c7dd87; `git diff master` also lists FR-21's explorer/theme files because master moved on after the branch point — not part of this task):
  - `crates/vr-ide/src/commands/terminal.rs` — `TerminalPanel.tab_drag: Option<usize>` (l.247); a press on a tab label arms it (l.1174); while armed, `Drag(Left)` calls `drag_tab_to` and `Up(Left)` ends it wherever the mouse is, a new `Down` clears it (l.1106-1119); `drag_tab_to` (l.1310) moves the tab inside `tabs` (Vec remove/insert) and `active` follows it; cleared in `close_tab`, `set_position`, `hide`. Doc comment of `handle_mouse` updated.
  - `crates/vr-ide/src/terminal_tab_drag_tests.rs` (new, 9 tests), `crates/vr-ide/src/lib.rs` (+ `mod terminal_tab_drag_tests`).
  - `README.md` (terminal tab bar paragraph), `docs/keymap.md` (Terminal mouse row).
- Behaviour decided:
  - A press on a tab label (not its `x`) activates it (unchanged) and arms a tab drag. A press on the empty part of the bar or the `+N` marker still resizes a bottom panel (unchanged: a label press never resized). On the right the separator stays the resize handle.
  - Live reorder: the dragged tab (the active one, already drawn with `tab_active`) moves while the mouse moves, so the bar itself is the visual hint; no new style (editor tabs have no drag to be consistent with).
  - Anti-flip: the tab moves onto tab j only once the mouse would lie on the dragged tab at its new place (`x + me.width >= end_j` to the right, `x < x_j + me.width` to the left).
  - Past the last drawn tab (`+N`, `+`, empty bar): it moves after that tab, or one place further per drag cell when it already is the last drawn one, which carries it past tabs hidden behind `+N`. Left of the panel counts as its first column; with overflow the window keeps the active tab last, so dragging left walks it to the front.
  - Labels number by position, so the numbers, status `i/n` and `f8`/`shift+f8` follow the new order. A tab moves as a whole `TermTab` (pty, title, scroll): nothing respawns.
- Deviations: none. vr-term untouched (the order lives in vr-ide).
- Out-of-scope noticed: `rule-checks.sh related-tests FR-19` also lists vr-git / vr-term / vr-config / vr-syntax tests by name stem (`lib`, `terminal`); those crates cannot depend on vr-ide, so they are not related to this diff and were not run.
- Capabilities to add (leader, main tree): `commands/terminal.rs` `drag_tab_to` (terminal tab drag reorder); `qa-artifacts/FR-19/walk.py` (SGR-mouse press/drag/release pty walk on the real binary).

## Paths (BASE-23)
1. Bottom panel, drag right and left — `dragging_a_tab_right_moves_it_there_and_keeps_it_active`, `dragging_a_tab_left_moves_it_there`; pty walk.
2. Right panel — `tabs_reorder_on_the_right_too`; pty walk.
3. Overflow (`+N`) both ways — `an_overflowing_bar_carries_the_tab_past_hidden_ones`; pty walk (right, 5 tabs, `+3`).
4. Press-release without movement / drag staying on the tab — `a_press_and_release_without_movement_only_switches`; pty walk.
5. `f8`/`shift+f8` and status — `f8_and_shift_f8_follow_the_new_order`; pty walk (f8 from the editor).
6. Release outside the panel, stray drag afterwards — `a_drag_released_outside_the_panel_ends_the_move`.
7. Empty bar resize, `x` after a move — `the_empty_bar_still_resizes_and_plus_and_x_still_work`; `+` in the pty walk (every tab after the first opened with `+`).
8. Wide label over a narrow one (no flip-back) — `a_wide_tab_over_a_narrow_one_does_not_flip_back`.

## Commands + output (trimmed)
Red (before the code), `run-heavy.sh cargo test -p vr-ide terminal_tab_drag`:
```
test terminal_tab_drag_tests::dragging_a_tab_right_moves_it_there_and_keeps_it_active ... FAILED
  left: ["one", "two", "three"]
 right: ["two", "three", "one"]
(7 FAILED with the order unchanged; press-without-move and no-flip passed trivially — no-flip now also asserts it reaches place 2)
```
Green: `test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 645 filtered out`

Related set on the final code, `run-heavy.sh cargo test -p vr-ide -- terminal workspace_ui_tests ui::tests` (terminal_tests, terminal_right_tests, terminal_tab_drag_tests, workspace_ui_tests (reads `tab_bar`), ui::tests incl. welcome/preview listed by related-tests):
```
test result: ok. 160 passed; 0 failed; 2 ignored; 0 measured; 492 filtered out; finished in 0.39s
```
Guard, `run-heavy.sh bash -c 'cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings'`:
```
    Checking vr-ide v0.1.0 (.../.worktrees/FR-19/crates/vr-ide)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 8.54s
```
(fmt check silent = clean; rustfmt was run only on commands/terminal.rs and terminal_tab_drag_tests.rs.)
Build `run-heavy.sh cargo build -p vr-ide` -> `Finished dev`; binary sha1 5d20cf6d43be, built after the last code change.
Snapshots: none regenerated — no rendering code changed; no `.snap.new` in the worktree status.

## Pty walk — real binary, 120x40, /bin/sh, SGR mouse reports (`qa-artifacts/FR-19/walk.py`, reuses T10/vt.py)
```
=== bottom: three tabs, CCC active
bar   :  1 AAA x  2 BBB x  3 CCC x  +                         ctrl+` hide
=== bottom: AAA dragged onto CCC
bar   :  1 BBB x  2 CCC x  3 AAA x  +                         ctrl+` hide
status:  TERM  AAA                            3/3 · 120×15 · ctrl+` hide
=== bottom: press-release on BBB (switch only)
bar   :  1 BBB x  2 CCC x  3 AAA x  +
status:  TERM  BBB                            1/3 · 120×15
=== bottom: f8 from the editor after BBB
status:  TERM  CCC                            2/3 · 120×15
=== right: five tabs, overflow   (ctrl+f7 from the focused terminal)
bar   : ... │ 4 …tle-DDD x  5 …tle-EEE x  +3  +  ctrl+` hide
status:  TERM  a-much-longer-title-EEE         5/5 · 48×37
=== right: EEE dragged to the first tab shown
bar   : ... │ 1 …er-title-EEE x  2 BBB x  +3  +  ctrl+` hide
status:  TERM  a-much-longer-title-EEE         1/5 · 48×37
=== right: EEE dragged onto +N and beyond
bar   : ... │ 4 …tle-DDD x  5 …tle-EEE x  +3  +  ctrl+` hide
status:  TERM  a-much-longer-title-EEE         5/5 · 48×37
```
Titles are set by each shell (OSC 0) and stay with their tab after every move: nothing restarted. After the walk: `ps -eo pid,args | grep -c "[w]orktrees/FR-19/target/debug/vr-ide"` -> `0`.

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| tab layout + hit test | `ui/terminal_view.rs` `tab_bar`/`BarTab`, `commands/terminal.rs` `bar_hit` | reused `tab_bar` (same layout as the drawing) in `drag_tab_to`; `bar_hit` for the press |
| drag state pattern | `TerminalPanel.resizing` (T19 separator drag) | same shape: armed on press, Drag/Up handled first wherever the mouse is |
| editor tab drag-reorder | `ui/tabs.rs`, `app.rs` route_mouse, 04_dev_T25.md | none exists (double-click only); nothing to reuse |
| test helpers | `terminal_tests::{draw, fake_tab, row_text}`, `app_tests::test_app` | reused |
| pty screen model | capabilities, qa-artifacts/T10/vt.py, FR-17/walk.py | reused vt.py; new walk.py with SGR mouse |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | diff vs HEAD: terminal.rs (tab bar mouse only), new test file, lib.rs mod line, README/keymap lines; no keymap/git/explorer code touched |
| BASE-2 | pass | AC-F3 followed; no contract change |
| BASE-3 | pass | red run above (7 FAILED, order unchanged) before `drag_tab_to` existed |
| BASE-4 | pass | related tests, guard, build output above |
| BASE-5 | pass | tests/clippy/build via run-heavy.sh; walk process ended, count 0 |
| BASE-6 | pass | English; why-comments (anti-flip, past-the-end rule) |
| BASE-9 | pass | only mouse columns as input: `saturating_sub`, index bound checked (`cur >= n` clears the drag) |
| BASE-10 | pass | related set only (terminal*, workspace_ui_tests, ui::tests); no full suite |
| BASE-11 | pass | worked only in .worktrees/FR-19; main tree only $TEAM_WS notes and qa-artifacts |
| BASE-14 | pass | all runs foreground, under the limit; no waiting loops |
| BASE-15 | pass | only the walk's own child ended, by pid inside walk.py |
| BASE-16 | pass | ## Reuse |
| BASE-18 | pass | no agents started |
| BASE-19 | pass | notes created before the first change, progress lines |
| BASE-22 | pass | test module named like its neighbours (`terminal_right_tests` -> `terminal_tab_drag_tests`) |
| BASE-23 | pass | ## Paths: bottom, right, overflow, no-move, f8, outside release, resize / x / + |
| BASE-24 | pass | no index or history command run |
| BASE-25 | pass | guard output above, on the final code |
| BASE-32 | pass | D312 (panel tabs, not editor tabs): only terminal panel tabs; D227: right separator still resizes (terminal_right_tests pass); D314 ctrl+f7 unchanged (walk uses it from a focused terminal) |
| BASE-35 | pass | USER-3: built directly, no mockup round; USER-5: no key added or changed |
| BASE-37 | pass | pty walk on the real binary (bottom, right, overflow) |
| BASE-38 | pass | rustfmt only on the two changed .rs files |
| BASE-46 | pass | walk and tests ran on binary 5d20cf6d43be built after the last code change (uncommitted on 6c7dd87) |
| BASE-48 | pass | walk prints bar/status read back from the screen after each step; ps count read after |

n-a: BASE-7 (no commit asked), BASE-8 (no dependency added), BASE-12 (no other repository), BASE-13 (no shared env or others' untracked files touched), BASE-17 (no toolchain change), BASE-20 (nothing removed), BASE-21 (no secrets in output), BASE-26 (no rewrite), BASE-27 (tests use temp dirs and fakes, no shared data), BASE-28 (no third-party code), BASE-29 (no browser), BASE-30 (checks wrote only a scratch temp dir), BASE-31 (no containers), BASE-33 (target env is a local terminal, covered by the pty walk), BASE-34 (nothing blocked), BASE-36 (inside vr-ide's terminal panel scope), BASE-39 (no schema), BASE-40 (no outward writes), BASE-41 (no credentials), BASE-42 (no new test runner or settings), BASE-43 (no access fix), BASE-44 (no data adapter; the fake TermTab already runs the flow), BASE-45 (criteria from spec AC-F3), BASE-47 (no computed value fixed; the order is the Vec itself, every reader indexes it)

| Rule | Status | Evidence |
|---|---|---|
| COST-1 | pass | heavy runs via run-heavy.sh |
| COST-3 | pass | no polling |
| COST-4 | pass | no full suite |
| COST-5 | pass | walk temp dir in the scratchpad; vr-ide ended |
| COST-7 | pass | fake tabs in unit tests; pty walk only for the runtime check |
| COST-8 | pass | edits via Edit and one perl pass; no python heredoc edits |
| COST-9 | pass | nothing left running (ps count 0) |
| COST-10 | pass | related set includes the test files naming tab_bar/handle_mouse (workspace_ui_tests, terminal_right_tests) and related-tests' vr-ide files |
| COST-13 | pass | walk.py in $TEAM_WS/qa-artifacts/FR-19/ |
| COST-15 | pass | no snapshot changed; none regenerated |
| COST-16 | pass | output trimmed with grep/tail |
| COST-19 | pass | one guard run on the final code |
| COST-20 | pass | filtered test runs for re-checks |

n-a: COST-2 (developer role only), COST-6 (no audit), COST-11 (no e2e target in this project), COST-12 (no network requests), COST-14 (no CI or deploy), COST-17 (no generated images), COST-18 (no full run started)

## Merge
- Leader merged master (FR-20, FR-21) into fr-19; one conflict: `crates/vr-ide/src/commands/terminal.rs` (TerminalPanel fields). Resolved by keeping `tab_drag` and FR-20's shorter `quit_confirmed` doc comment (git-job wording removed); no markers left (`grep -c` -> 0). Not staged: the leader stages it.
- After the merge: `run-heavy.sh cargo test -p vr-ide --lib -- terminal workspace_ui_tests ui::tests` -> `161 passed; 0 failed; 2 ignored`; guard fmt + clippy -> `Finished dev`, clean.

## Fix round 1
1. QC defect e1 (narrow right panel, one tab drawn with 5 hidden before it, dragging left never moves it) — fixed.
   - Root cause: `drag_tab_to` had a "past the end" rule on the right only; left of the first drawn tab, or inside the lone tab, no move applied.
   - Fix: `commands/terminal.rs:1335` mirror rule: when the dragged tab is the first drawn one and the mouse is on its leading cells (its space and number, `x < me.x + 2`) or left of the panel, it moves one place toward the front (`cur.checked_sub(1)`). Only a bar that draws the active tab alone has tabs hidden before its first drawn one, because the window otherwise keeps the active tab last, so the rule changes nothing elsewhere. Doc comment updated.
   - Test `a_lone_drawn_tab_walks_left_past_hidden_ones` (terminal_tab_drag_tests.rs:316): 80x24 right panel, 6 tabs, active 6, one tab drawn. A drag to the panel's second column moves it to place 5; a drag over the editor walks it to the front, and the other tabs keep their order. Red first: `left: 5 right: 0` with no rule, and `left: 5 right: 4` with an `x <= me.x` rule, which misses QC's stop at the panel's second column. Green with the final rule.
   - Live replay on the real binary, `qa-artifacts/FR-19/qc_walk.py <bin> <tmp> 24 80`:
     - before the fix (binary c9041b93e1eb): `[80x24] FAIL e1 dragged to the front (FFF is 1/6) ('a-much-longer-title-FFF', 6, 6)`
     - after the fix (binary 2bc44091eb8a, built after the last code change): `24 80: 19 PASS, 0 FAIL` with `[80x24] PASS e1 dragged to the front (FFF is 1/6) ('a-much-longer-title-FFF', 1, 6)`; `40 120: 19 PASS, 0 FAIL`, including e1-e3.
     - vr-ide processes left after the replay: 0.
2. Review minor: README (terminal tab bar paragraph) and docs/keymap.md (Terminal mouse row) now also say that at the bar's left edge, past the first tab shown, the tab moves toward the front the same way — fixed.

Final runs of round 1 (on the final code):
```
run-heavy.sh cargo test -p vr-ide --lib -- terminal workspace_ui_tests ui::tests
test result: ok. 162 passed; 0 failed; 2 ignored; 0 measured; 432 filtered out; finished in 0.36s
run-heavy.sh bash -c 'cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings'
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.87s
run-heavy.sh cargo build -p vr-ide -> Finished; sha1 2bc44091eb8a
```
rustfmt was run only on commands/terminal.rs and terminal_tab_drag_tests.rs. No snapshots changed.

### Rule rows touched (round 1)
| Rule | Status | Evidence |
|---|---|---|
| BASE-3 | pass | `a_lone_drawn_tab_walks_left_past_hidden_ones` seen red twice (no rule; `x <= me.x` rule) before the final rule |
| BASE-4 | pass | round 1 final runs above |
| BASE-23 | pass | overflow path to the left at a narrow width now covered: unit test + qc_walk e1 at 80x24 and 120x40 |
| BASE-25 | pass | guard clean after the merge and after the fix |
| BASE-37 | pass | QC's e1 replayed on the real binary before (FAIL) and after (PASS) |
| BASE-46 | pass | replay ran on binary 2bc44091eb8a, built after the last code change |
| BASE-11 / BASE-24 | pass | merge conflict resolved with Edit only; nothing staged or committed |
| COST-9 | pass | 0 vr-ide processes after the replay |
| COST-10 | pass | related set as in round 0 |
