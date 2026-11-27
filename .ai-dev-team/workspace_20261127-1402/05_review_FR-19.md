# Review FR-19 Drag to reorder terminal tabs

Tools available: Read, Bash, Write, Skill, SendMessage, SubagentHandback.
Test run (once): `cargo test -p vr-ide --lib terminal_tab_drag` -> 9 passed, 0 failed.

## Findings
No blocker or major finding.

[minor] crates/vr-ide/src/commands/terminal.rs:1310 (drag_tab_to) — leftward overflow walk is undocumented
Scenario: with tabs hidden behind `+N` on the left, dragging to the left edge moves the tab one place per drag event (the window follows the active tab), same as rightward. README only describes the rightward case ("past the last tab shown").
Fix: add half a sentence to README / keymap, or leave as is.

Checked and sound:
- Drag state cannot stick: cleared in close_tab, hide, set_position (ctrl+f7); Up(Left) ends it from anywhere (handle_mouse runs before the panel hit test); a new Down clears it if the Up was lost (dialog or popup took it); `cur >= n` and the `me` lookup are guarded.
- A press on `x` or `+` goes to BarHit::Close / New, so no drag starts. Pressing the empty bar or `+N` still resizes at the bottom; the right-side separator is untouched.
- Indices in other structures: the close-confirm dialog stores the pid, and answer_close finds the tab by pid, so a move mid-dialog stays correct. Tab state (pty, title, scroll, grid) moves as a whole TermTab. `selection` is panel-level and is reset on press. restart / close_active / status / f8 all read `active`, which follows the tab.
- `+` appends at the end (unchanged). T25 double-click belongs to editor tabs; route_mouse takes last_tab_click on Drag, so nothing changes for it.
- Anti-flip arithmetic and the past-last branch are consistent with the tests; the paths walked in the notes match the tests.

## Files read
| File | Status |
|---|---|
| crates/vr-ide/src/commands/terminal.rs | read (diff + handle_mouse, bar_hit, close_tab, answer_close, restart) |
| crates/vr-ide/src/lib.rs | read |
| crates/vr-ide/src/terminal_tab_drag_tests.rs | read (test list; run once) |
| README.md | read |
| docs/keymap.md | read |

## Rule check FR-19
| Rule | Result | Note |
|---|---|---|
| BASE-1 | pass | AC-F3 met per tests and pty walk |
| BASE-2 | pass | no contract change |
| BASE-3 | pass | no input parsing added; indices bounds-guarded |
| BASE-6 | pass | tests + walk in notes |
| BASE-16 | pass | reuses tab_bar / BarTab hit data; editor tabs have no drag to reuse |
| BASE-23 | pass | bottom, right, overflow all covered by tests and walk |
| BASE-25 | pass | README and keymap updated |
| BASE-32 | pass | no new dependency, no unscoped files; only listed files changed |
| BASE-47 | pass | numbers, status i/n, f8/shift+f8, x, confirm dialog (pid), restart, title, `+` all read order/active and stay correct |

APPROVE

## Re-check round 1
Test run (once): `cargo test -p vr-ide --lib terminal_tab_drag` -> 10 passed, 0 failed (incl. `a_lone_drawn_tab_walks_left_past_hidden_ones`).

- New left rule (`drag_tab_to`, terminal.rs:1367) fires only when the dragged tab is the first one drawn. `tab_bar` keeps the active tab last in the window whenever two or more tabs are drawn with a hidden start (`start = active+1-k`), and the pressed/dragged tab is always the active one. With several tabs drawn, the first drawn tab is therefore the dragged one only at index 0, where `cur.checked_sub(1)` is `None`. Wiggling inside the own label can't move a tab in the normal case. It can move only a lone drawn tab (bar too narrow for more), which is the intended case.
- Fix is consistent with the earlier doc/README/keymap text (leftward walk now documented). The README/keymap diff in the round also carries master's unrelated FR-20/21 text; no FR-19 conflict there.
- Merge resolution: `TerminalPanel` keeps both `tab_drag` (l.283, init l.308) and `quit_confirmed` (l.286, l.309); `tab_drag` still cleared in close_tab, set_position, hide (l.420, 534, 627); handle_mouse tab-drag block and press arm intact (l.1142-1153, 1210); FR-18 merge introduces no index-keyed state conflicting with the tab move (per-tab state moves with the TermTab).
- No new findings.

APPROVE
