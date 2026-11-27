# Review T22 — unified find box (ctrl+f)

Verdict: **CHANGES_REQUESTED** (2 major, 6 minor). Security (replace data-safety paths): **SECURE**.

Evidence: `qa-artifacts/T22/review/` — fresh release build (`release_build.txt`); the dev walk reran 48/48
(`devwalk/walk.txt`); review walk `walk_review.py` 31/34 (`walk_review.txt`, screens `rv_*.txt`);
short-window probe `walk_short.py` (screens `short_*.txt`). All legacy VTE bytes, TERM=xterm-256color.

## Findings

[major] crates/vr-ide/src/commands/search.rs:666-669 (reached from find.rs:502 `replace_open`) — ctrl+r after opening a project result replaces the query with the selected match
Scenario: Project scope, query `/fo+/i` → 10 results → Down, Down, Enter (the result opens, the editor gets the keys, the selection is the match `foo`) → ctrl+r. `open_project` → `search::show` seeds `selection_seed` (focus is Editor) and the query becomes `foo`: literal and case-sensitive. The list still shows the 10 `/fo+/i` results; Enter in the replace field then searches and previews `foo` only. This is the flow D? "ctrl+r keeps the box's scope" was made for. Reproduced at all three sizes: `rv_08` / `rv_16` / `rv_24` (row 1 reads `Project  foo`). `find_tests::ctrl_r_from_the_editor_keeps_the_project_scope` does not open a result first, so it does not catch this.
Fix: in `show`, skip the seed when the selection is a result of the current project query (as `open_bar` already does with `current(app)`). Or skip it when `find.project()` is already true. Add a test: open a result, press ctrl+r, check the query is unchanged.

[major] crates/vr-ide/src/ui/mod.rs:41-44 + ui/layout.rs:75 — in a short window with the terminal shown, the replace field is hidden but still takes the keys and edits the file
Scenario: a 40x9 window (80x9 too), a file open, terminal panel shown, editor focused. The terminal gets placed first (`place_terminal`) and leaves the editor 2 rows. ctrl+f, type `foo`, ctrl+r: `place_find_box` clamps the box to 1 row. The `Replace` row is not drawn, but the field has the cursor (D117). Typing `ZZ` and pressing Enter replaced a match in the file without the replacement ever being shown (`short_03`, `short_04`: the tab is now `a.txt ●`). Before T22 the bar's rows were taken before the terminal's, so the bar's fields always stayed visible (regression; old `compute` took `find_rows` before `place_terminal`).
Fix (the user decides): reserve `field_rows` before `place_terminal` (the terminal shrinks, as before). Or, when the fields do not fit, show only the field that has the cursor. Or refuse the replace row (keep File find only) when `editor.height < fields + 1`.

[minor] crates/vr-ide/src/commands/find.rs:392-410 — switching File→Project sets the search fields directly, so `fields_changed` is skipped
Scenario: Project preview shown → ctrl+f (File; the preview stays, hidden) → Tab, edit the replacement → ctrl+f (Project). The query is the same, so `start` does not run and the old preview stays on screen under the new replacement. No data risk: `ask_confirm`/`apply` refuse it through `is_current` ("the fields changed"). But the screen shows a preview that the fields no longer describe. With an empty query, the old results also stay under an empty field.
Fix: call `search::fields_changed` (or `drop_preview` when it is not applying) when the switch changes the text.

[minor] crates/vr-ide/src/commands/find.rs:360 — ctrl+f from the editor during a running project replace hides the progress
Scenario: replace confirmed and running → click the editor → ctrl+f. `open_bar` sets `search.visible = false`: the progress and the "Esc cancels" path are no longer shown. The notes' design call says the progress stays in view (the switch in the box is refused for that reason), but this path bypasses the check. The end notices still arrive.
Fix: while `search::replacing(app)`, have ctrl+f from the editor bring the project box back (or refuse with the same notice).

[minor] crates/vr-ide/src/ui/find_bar.rs:53 — at 40x12 the replace preview shows one list row, so the checkbox that Space toggles is not visible
Scenario: snapshot `render_replace_preview_40x12` (explorer shown): the list cap is 3 rows, and the status note takes 2 of them ("Space toggles, Enter / replaces"). One row is left: the hunk `-2 Order total…`. The `[x] README.md` row whose state Space changes is scrolled off. `rv_31` (no explorer) shows the same note plus 2 rows.
Fix options: count the note rows outside the 2/5 cap (costs editor rows), shorten the narrow status to one row ("Space · Enter"), or keep the selected file row pinned.

[minor] crates/vr-ide/src/commands/find.rs (handle_key, removed `KeyCode::Enter if shift_only`) — built-in keys changed beyond the spec
shift+Enter (previous match, D116 built-in) is removed, and PageUp/PageDown plus the mouse wheel/click on the File-scope list are added. Neither change is in 01_spec_T21.md or 01_spec_T22.md (the spec names Up/Down, Enter, Esc, f3/shift+f3). Flagged for the user's decision. No key is proposed here.

[minor] crates/vr-ide/src/ui/find_bar.rs:244 — the empty Project box says "ctrl+f: search the file" with no file open
With no file open (the box opens on the project), ctrl+f in the box is refused ("Find in file: no file is open"). Fix: leave the scope part of the hint out when `active_doc()` is None.

[minor] crates/vr-ide/src/commands/app.rs:111, crates/vr-ide/src/app_workspace.rs:209, crates/vr-ide/src/commands/search.rs:1424 — user-facing text still names the removed panel
"Esc in the search panel cancels it" and "(see the panel)". Fix: "Esc in the find box cancels it" and "(see the find box)". Code comments at app.rs:747 and 1123 and in status_bar.rs:16 are stale too.

## Checked, no finding
- Parser (`syntax.rs`): `/i`, `//`, `//i`, `/`, unclosed `/fo+`, `\/` in both modes, `and\/i`, backslashes kept, selection seeding round-trip (`rv_06`: `a/b` → `a\/b`, 1/1). Known corner, not filed: a regex ending in an escaped backslash (`/a\\/`) reads as literal. `[\\]` works.
- File list against the editor's own find: same `find_capped` engine (D124/D125 unchanged). CRLF rows are clean, and `/foo$/` matches before CR (`rv_03`). Multi-byte and CJK regex work (`rv_04`, `rv_05`). A 4000-col line shows `…` with context and is read through a window (`line_window`) (`rv_01`). The list is virtual.
- Project: root header rows (`Row::Root`, unique root names via workspace disambiguation); `row_count` matches `rows()`. Open-at-match puts the editor in read mode with the box kept open. f3 in Project scope runs `step_result`.
- Replace safety: `apply`, `is_current`, `ReplaceGate`, the Buffer-text compare, the confirm dialog defaulting to Cancel, the Space toggle, Esc closing the preview, and the D247 `roots_changed` rerun are unchanged in the diff. A bad regex is refused before both search and preview. Review walk: Space unchecks, confirm asks, Esc leaves files untouched (`rv_25`–`rv_33`). Dev walk: confirmed write.
- Esc → close box + read mode (File replace in edit mode too). Layout keeps 1 editor row at 80x24/120x40/40x12 and with the terminal at 40x10/40x12 (`short_06`–`short_12`).
- `find.toggle_*` is gone from the registry, keymap.toml comments, docs/keymap.md, README and the config tests. The slash commands never referenced it. A test asserts the toggles are absent.

## Files read
crates/vr-search/src/syntax.rs, lib.rs; crates/vr-ide/src/commands/find.rs, search.rs (diff + start/attach/show/request_preview/ask_confirm/apply/handle_key context), explorer.rs, terminal.rs; app.rs; ui/find_bar.rs, ui/layout.rs, ui/mod.rs (diffs); README.md, docs/keymap.md, crates/vr-config/defaults/keymap.toml, crates/vr-config/tests/config.rs — read.
Tests (find_tests, search_tests, mode_tests, app_tests, explorer_tests, terminal_tests, ui/tests/preview.rs) — read where they touch findings (ctrl+r project test, toggle-gone test); others skimmed via notes. ui/search_panel.rs — diff skimmed (list reused). Snapshots — find_bar 80x24, find_box_empty 40x12, replace_preview 40x12 read; others: skipped (dev reviewed, walk screens cover).

## Rule check T22
| Rule | Status | Evidence |
|---|---|---|
| AC-22a | pass | dev walk 02-05/24-27; `rv_01`-`rv_05` |
| AC-22b | pass | grouping + open-at-match: walk 12-14, `rv_07`; `project_results_are_grouped_by_root` |
| AC-22c | pass | syntax.rs tests; walk 08-11; `rv_03`, `rv_05`, `rv_06` |
| AC-22d | fail | ctrl+r in Project scope after opening a result swaps the query (major 1); shift+f3 legacy is D264 (open, not counted) |
| AC-22e | pass | snapshots at 3 sizes; dev timing.txt (1M lines 15.8 ms, 50k files 4.7 ms frame) |
| BASE-2 | fail (minor) | built-in shift+Enter removed, PageUp/PageDown/mouse added outside the spec (user decides) |
| BASE-3 | pass | parser red-first noted; behaviour tests listed; gap: no test for ctrl+r after opening a result |
| BASE-4/5 | pass | real outputs in qa-artifacts/T22/dev; release build reproduced |
| BASE-16 | pass | worker, preview/apply, find engine and list renderer reused |
| COMPACT (clip) | fail | hidden replace field at height 9 with the terminal shown (major 2); 1-row preview list at 40x12 (minor) |
| D37/D49/D123/D126/D135 | pass | apply/is_current/ReplaceGate untouched; walk confirms |
| D264/D265 | pass | left open, unchanged |
| USER-5 | pass | no new keymap binding; legacy ctrl+f switch tested |

## Round 1 re-check
Verdict: **APPROVED**. No new findings. Both majors and all six minors are fixed, each with a test that fails without the fix.

- major 1: `SearchPanel.opened` (file and range set by `open_match`, search.rs:737) is checked in `selection_seed` (search.rs:664). The test `ctrl_r_after_opening_a_result_keeps_the_project_query` covers Enter, f3, and a user selection that still seeds (`f`). It checks the query and the result total.
- major 2: ui/mod.rs reserves `field_rows` (`place_find_box`) before the terminal, then `grow_find_box` from what is left. `render` gives the cursor to the query when the box has no row for the replace field, in both scopes. `the_box_fields_keep_their_rows_with_the_terminal_shown` checks 40x9 (both rows drawn, box above the terminal) and 40x4 (hidden field gets no keys, text unchanged, Project scope too).
- fields_changed on scope switch: `switch_scope` calls it when the text changed, and an emptied query runs `start`. Covered by `scope_switch_with_changed_fields_drops_the_stale_preview` (preview dropped, results cleared).
- replace progress on ctrl+f: `find.open` goes to the project box while `replacing`, and `show` keeps the fields. Covered by `ctrl_f_from_the_editor_during_a_replace_keeps_the_progress`.
- 40x12 preview: the notes are counted on top of the 2/5 share, so the `[x]` row stays. The assertion in `render_replace_preview` checks for `[x]` and "Space toggles".
- shift+Enter: restored in the find field (find.rs:745), with `f3_and_shift_f3_step_and_wrap`. D275 keeps PageUp/PageDown/wheel/click.
- Hint: it drops the scope key when no file is open (`hint`, find_bar.rs:263).
- Wording: no user-facing "search panel" is left. The remaining mentions are code comments (search.rs:509, 1396, 1827, 1923, 2170). They are cosmetic and not filed.

40x9 with the terminal shown: the editor gets 0 rows after the box. This is acceptable. Before T22, `compute` also took `find_rows` before `place_terminal` (HEAD layout.rs:81), so the behaviour matches. The query and replace rows and the match count stay visible, and the replace field takes keys only while it is drawn.

Spot-runs:
- `cargo test -p vr-ide --lib find_tests`: 20 passed, 3 ignored.
- `cargo test -p vr-ide --lib search_tests`: 32 passed.
- `dev-r1/walk_short.py` against `target/release/vr-ide`: 9/9. No vr-ide process was left running.

## Files read
Round 1 diff of commands/search.rs (selection_seed, show, open_match), commands/find.rs (switch_scope, handle_key), ui/find_bar.rs, ui/mod.rs, find_tests.rs and search_tests.rs (the new tests) — read. Snapshots — skipped (the dev reviewed them, and the asserted rows cover them).

## Rule check T22
| Rule | Status | Evidence |
|---|---|---|
| AC-22d | pass | ctrl+r after a result keeps the query (test); shift+f3 legacy D264 stays open, not counted |
| COMPACT (clip) | pass | 40x9 fields drawn before the terminal; hidden field gets no keys; 40x12 preview shows `[x]` |
| BASE-2 / USER-5 | pass | shift+Enter restored as a D116 built-in; list keys per D275; no new binding |
| BASE-3 | pass | each fix has a named test, red first per the notes; re-run green |
| D275 | pass | fields before the terminal; shift+Enter kept |
| D264/D265 | pass | left open, unchanged |
