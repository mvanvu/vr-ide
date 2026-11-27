# Review FR-30 — Editor tabs: x close button

Tools available: Read, Bash, Write, Skill, SendMessage, SubagentHandback.

## Findings
None blocking. Notes (no action required):
- minor (info) ui/tabs.rs `close_offset`: a tab wider than the bar (label cut) has no clickable x; the user falls back to ctrl+w. This matches the terminal tabs and the notes.
- Not run: the `workspace_ui` test run was denied by the permission classifier, so I rely on the developer's evidence (686 passed, walk 6/6 at 3 widths).

- minor (tests) workspace_ui_tests.rs `x_on_a_dirty_tab_opens_the_unsaved_dialog_and_cancel_keeps_it`: covers a dirty NON-active tab (tab 0 dirty, tab 1 active), the dialog opening and Esc keeping both tabs. It does not press Save or Discard, and does not assert that `active` stays 1 or that the dialog targets tab 0. Those paths are correct by code reading (below), but a Discard test on the non-active tab would pin the contract. Not blocking.
- minor (tests) `the_double_click_still_reveals_with_the_x_in_place` asserts `explorer.visible`, which may already be true before the clicks, so it proves little. Not blocking.

Data safety (checked in code):
- `request_close(app, i)` takes the dirty check, the name and `DocRef::of(app, i)` from tab `i`, never from `app.active`. The dialog therefore belongs to the clicked tab.
- `answer_close` resolves the tab again via `DocRef::find` (index and path match, otherwise a path search). It saves or discards that tab, not the active one, and does nothing if the tab is gone.
- Tabs cannot change while the dialog is up: `route_mouse` sends all mouse events to the dialog first (app.rs 1390), and the keyboard is gated the same way. The active tab stays unchanged when a non-active tab is closed (`close_doc` adjusts `active`).
- Discard writes the recovery copy before closing, and a failed write keeps the tab. Save on a non-active tab uses `save_doc(i)`. A disk change leads to the existing external-change prompt, which is also tied to a DocRef.
- Untitled tabs (path None): the dialog message is empty-named, as before. `DocRef.find` matches by index only, which is safe here because the dialog is modal.

Design QA:
- The x sits after the disk-state marker as ` x `, so `close_offset` = start + width - 2 uses the same label spans as the render. The wide-chars snapshot is consistent.
- x is dim on inactive tabs, as the terminal tabs are. It uses the base style on the active tab.
- The x is hit only when `w == full`, so a cut label never closes on a click.
- A click on the x does not touch `last_tab_click`, except that `take()` clears it. The label path and the T25 double click are unchanged.
- Snapshots: 42 files, 42 removed lines and 42 added lines. Sample read, and each diff only adds " x" on the tab row (see Files read).

## Files read
| File | Status |
|---|---|
| crates/vr-ide/src/ui/tabs.rs | read |
| crates/vr-ide/src/commands/file.rs | read (diff, plus answer_close, save_doc, discard_and_close) |
| crates/vr-ide/src/app.rs | read (diff, plus close_doc and route_mouse head) |
| crates/vr-ide/src/commands/dialog.rs | read (DocRef, CloseTab arm; callee) |
| crates/vr-ide/src/workspace_ui_tests.rs | read (all 6 new tests) |
| README.md | read |
| snapshots (42) — sample read: snapshots/vr_ide__blame_column_tests__render_blame_column_120x40.snap, ui/snapshots/vr_ide__ui__tests__render_confirm_dialog_40x12.snap, ui/snapshots/vr_ide__ui__tests__render_find_box_empty_40x12.snap, ui/snapshots/vr_ide__ui__tests__render_replace_preview_120x40.snap, ui/snapshots/vr_ide__ui__tests__render_search_panel_80x24.snap, ui/snapshots/vr_ide__ui__tests__render_wide_chars_80x24.snap | sampled; the other files show the same pattern in the aggregate diff (42 pairs, all adding " x") |

## Rule check FR-30
| Rule | Result |
|---|---|
| BASE-1 | pass — AC-X1 behaviors present: x, close with dialog, label click, double click |
| BASE-3 | pass — notes record tests written first (compile-red) |
| BASE-6 | pass — no stray scope; the changes are in the board files |
| BASE-16 | pass — close logic reused (`request_close` extracted); the x offset follows the terminal tab idea |
| BASE-23 | pass — no new comments about history |
| BASE-25 | pass — fmt and clippy evidence in the notes |
| BASE-32 | pass — evidence is the developer's walk at 120/80/40 columns (6/6) and the unit tests; I ran no tests |

APPROVE

## Re-check QP1
Read the round's diff of `crates/vr-ide/src/workspace_ui_tests.rs` (read). Tests only; I ran nothing.
- `x_on_a_dirty_inactive_tab_with_discard_closes_only_it_and_keeps_a_copy`: the helper dirties tab 0, makes tab 1 active, clicks tab 0's x and asserts the dialog opens with `(docs, active) == (2, 1)`. Right+Enter then picks Discard. The test asserts the dialog is gone, one doc left (b.ts), `active == 0`, b's buffer text unchanged and not modified, a.rs on disk still `"a\n"` (not saved), and exactly one file under the recovery dir. This pins the data-safety contract.
- `x_on_a_dirty_inactive_tab_with_save_writes_that_file_and_leaves_the_other_tab`: Enter (Save, the first button) leaves a.rs on disk as `"za\n"` and b.ts as `"b\n"`. Only b.ts remains, `active == 0`, and it is not modified.
- `the_double_click_still_reveals_with_the_x_in_place`: now asserts the explorer is hidden before the second press, then visible, focus on the explorer, 2 docs (no close), and the explorer cursor on a.rs. This is no longer vacuous.
Both earlier test gaps are closed. No new findings.

APPROVE
