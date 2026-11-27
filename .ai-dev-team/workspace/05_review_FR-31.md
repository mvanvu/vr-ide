# Review FR-31: Explorer right-click Search / Replace in Folder (D328)

Tools available: Read, Bash, Write, Skill, SendMessage, SubagentHandback.
Test run (once): `run-heavy.sh cargo test -p vr-ide --lib explorer_menu` gave 20 passed, 0 failed.

## Findings

[major] crates/vr-ide/src/commands/explorer.rs:1773 (`folder_under_cursor`) and search.rs:614 (`scope_roots`): the folder scope is never checked against the workspace roots, so a replace can write outside the workspace.
Scenario A: root `R` holds `R/link -> /elsewhere`, a symlinked dir that the explorer can expand. The user expands it and right-clicks its child `R/link/sub`. That child is a plain `NodeKind::Dir`, so `canonicalize()` gives `/elsewhere/sub`, which then becomes `SearchPanel.folder`. The search walks it (the project walk uses `follow_links(false)` and never goes there). `plan_replace_roots(&[/elsewhere/sub])` accepts every file, because each file is inside its "root", and Replace writes files outside every workspace root. A whole-project replace refuses those files ("not a file inside the project"). The field's doc comment says "inside a workspace root", but no code enforces it.
Scenario B: a replace in folder `B/src` is running and the user removes root `B` (Remove Folder from Workspace is allowed during a replace). `roots_changed` returns early at `applying()`, before the new folder check, so the folder stays. When the replace ends, Esc runs `start` on `B/src`, which is now outside the workspace. Preview and confirm then write there, and the label shows an absolute path.
Fix: in `folder_under_cursor`, return `None` unless the canonical path `starts_with` one of `app.workspace.paths()`. That also hides the menu items on such rows. In `scope_roots`, `start` and `build_preview` (or one helper they all call), end the folder scope (`set_folder(None)` plus a notice), or refuse, when the folder is no longer under a root. Add tests for the symlink-child row and for root removal during a replace.

[major] crates/vr-ide/src/commands/find.rs:338-363 (`open_bar`): this other path into File scope keeps the folder (BASE-47 reader missed), so ctrl+f does not return to the whole project (AC-R1).
Scenario: Search in Folder `src` is shown. The user clicks into the editor, so the box stays open but loses the keys. ctrl+f now runs `open_bar` (because `box_focused` is false) and gives File scope with `search.folder` still `Some(src)`. The next ctrl+f runs `switch_scope` from File to Project and searches only `src` again. The label reads "Project: src" where AC-R1 asks for the whole project. The same happens with ctrl+r (`replace_open`), `find.next` on an empty query (line 552) and lines 622/656, which all call `open_bar`.
Fix: call `super::search::set_folder(app, None)` in `open_bar`, which is the one place File scope is entered from outside the box, and add a test: folder scope, focus to the editor, ctrl+f, ctrl+f, then expect the whole project.

[major] crates/vr-ide/src/explorer_menu_tests.rs: changed behaviours have no tests.
Untested: (1) Esc or `close_box` ending the folder scope; (2) removing the folder's root ending the scope (`roots_changed`, search.rs:570); (3) `open_folder` refusing while a replace writes, which is the data-safety refusal named in the brief; (4) a folder change dropping an existing *preview*, not just results. That last case guards against confirming a whole-project preview after switching to a folder. The code is correct by reading (`set_folder` leads to `clear_results` and then `drop_preview`), but nothing pins it.
Fix: add one test for each.

[minor] crates/vr-ide/src/commands/search.rs:1247 (`paths_moved`): renaming the scoped folder in the explorer leaves `search.folder` on the old path.
Scenario: the folder scope is `src` and the user renames `src` to `lib`. The results follow the move, but the label still says `Project: src`, and the next search or preview runs on a missing root, so every file fails with "project root: No such file". Nothing is written; the scope just stops working until the box is closed.
Fix: remap `app.search.folder` with `moved_path(f, from, to)` in `paths_moved`.

[minor] crates/vr-ide/src/ui/find_bar.rs:107-118 (`scope_label`): under 36 columns the label is ` P `, so nothing shows that the scope is limited to a folder, while AC-R1 says "its path shown as the scope". `folder_label` also cuts by `chars()` rather than cell width, so CJK folder names take up to twice the budgeted width and squeeze the query field. Fix options: ` P: <last segment…> `, or a marker such as ` P* `. Measure with `unicode_width` when truncating. Cost is small either way; the leader or user decides.

Checked and safe: `apply_replace` checks `canonicalize(path).starts_with(plan.root)` again at write time, and `plan.root` is the folder. A symlinked file inside the folder that points outside is refused in the preview. A renamed or removed folder makes `canonicalize` fail, so nothing is written. Results from a search started before a folder change are dropped (`cancel_running` and the receiver is dropped). A whole-project preview is dropped on every folder change. `switch_scope` (Project to File) and `open_folder` refuse during a replace. Esc during a replace only cancels (`escape`), and `close_box` is not reached then. Menu items appear only on `NodeKind::Dir` rows (roots and folders, not symlinked dirs and not missing roots), and they are recomputed when chosen. The palette commands are unbound (USER-5). The group root label is hidden in folder scope.

## Files read
| File | Status |
|---|---|
| README.md | read |
| docs/keymap.md | read |
| crates/vr-ide/src/commands/explorer.rs | read (diff and `NodeKind` context) |
| crates/vr-ide/src/commands/explorer_menu.rs | read (diff and `open`) |
| crates/vr-ide/src/commands/find.rs | read (diff, `open_bar`, `open`, `switch_scope`, `close_box`, `handle_key`) |
| crates/vr-ide/src/commands/search.rs | read (diff, `start`, `roots_changed`, `build_preview`, apply worker, `paths_moved`, `escape`, `show`) |
| crates/vr-ide/src/ui/find_bar.rs | read |
| crates/vr-ide/src/explorer_menu_tests.rs | read |
| crates/vr-ide/src/ui/snapshots/vr_ide__ui__tests__render_palette_80x24.snap | read (one palette row added; the last row scrolls out, as expected) |
| crates/vr-search/src/replace.rs | read (callee: `plan_in_roots`, `apply_replace` root check) |
| crates/vr-fs/src/tree.rs | read (callee: `NodeKind`, symlink expansion) |
| .ai-dev-team/workspace/04_dev_FR-31.md, qa-artifacts/FR-31/walk.py | read |

## Rule check FR-31
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | diff limited to the board files plus README, keymap, snapshot |
| BASE-2 | fail | AC-R1 "ctrl+f returns to whole project" breaks through `open_bar` (finding 2); the folder is not confined to the workspace (finding 1) |
| BASE-3 | pass | notes: tests first, red on the missing field; 8 tests, 20/20 green on my run |
| BASE-6 | pass | comments in English, explain why, no bug history |
| BASE-16 | pass | engine reused (`search_roots`, `plan_replace_roots`); no duplicated logic |
| BASE-23 | fail | Esc, root removal, refusal during a replace and preview drop on a scope change are untested (finding 3) |
| BASE-25 | pass | notes: fmt --check and clippy -D warnings clean |
| BASE-32 | pass | compared with D328 (root uses the whole root, files keep their menu: yes), D247 (replace untouched by root changes: yes, but see finding 1B), D281/D283/D284 (multi-root menu order, Remove item kept last), D279/D265 (preview/confirm flow reused unchanged), USER-5 (no default key) |
| BASE-47 | fail | `open_bar` (File scope entry) and `paths_moved` read or need to update the scope but were not changed; `roots_changed` misses the applying path (findings 1B, 2, 4) |

CHANGES_REQUESTED

## Re-check round 1
Scope: the uncommitted diff of search.rs, find.rs, explorer.rs, ui/find_bar.rs and explorer_menu_tests.rs, plus `## Fix round 1` in the notes. `run-heavy.sh cargo test -p vr-ide --lib explorer_menu` gave 29 passed, 0 failed.

Earlier findings:
1. Folder outside the workspace: fixed.
   - Case A: `folder_under_cursor` now requires `in_workspace`, so a child of an expanded link gets no menu items and the command refuses it. Test: `a_subfolder_under_a_symlinked_dir_is_not_a_folder_scope`.
   - Case B: `roots_changed` still returns early while a replace is applying. Afterwards, the next `start`, `build_preview` or `apply` runs `scope_ok`, which ends the scope with a warning and drops the results and preview, so nothing is searched or written. Test: `a_folder_outside_every_root_ends_the_scope_before_search_and_preview`.
2. `open_bar`: fixed. It now calls `set_folder(None)`, which also covers the ctrl+r and find.next paths. Test: `ctrl_f_through_the_editor_path_also_ends_the_folder_scope`.
3. Missing tests: added for Esc, root removal, refusal during a replace (held worker; scope and outside file unchanged) and preview drop on a folder change.
4. `paths_moved`: the folder now follows a rename. Test added.
5. Narrow label: now ` P: <tail> `, and `folder_label` cuts by cell width; the ellipsis cell is counted in the budget (`w` starts at 1). Test added.

Leader's focus points:
- **TOCTOU between `scope_ok` and the write.** This is still covered. `plan.root` is the canonical folder from when the preview was built. At write time, `apply_replace` checks `canonicalize(file).starts_with(plan.root)` again. Two swaps after `scope_ok` are both refused because the file resolves outside `plan.root`: a folder (or one of its parents) replaced by a link that points outside, or a subdirectory swapped for such a link. A folder that is gone fails `canonicalize`, so nothing is written. The window left between that `canonicalize` and the atomic rename is the same as for a whole-project replace and was not introduced by this task.
- **Root removed during an apply.** The worker finishes the files that were confirmed, and each of them was already inside the folder when the preview and the confirm happened. This matches D247 for a whole-project replace, which also keeps writing the files of a removed root. No new file can enter the run, and the scope ends at the next use. Accepted.
- **Canonicalization cost.** `start` runs on Enter and on explicit triggers, never per keystroke (search.rs 578/676/1006/1807/1847). `scope_ok` costs 1 + roots `realpath` calls per search, preview or confirm, and the menu adds the same per right-click. Rendering the label makes no syscalls. This is negligible. Note (not a finding): workspace roots are already stored canonical (workspace.rs:463), so canonicalizing the roots again in `in_workspace` is redundant but harmless.

No new defects in this round's diff.

### Rule check FR-31 (round 1)
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | same file set |
| BASE-2 | pass | AC-R1: ctrl+f returns to the whole project from both paths; scope confined to the workspace |
| BASE-3 | pass | notes: new tests red first (A also re-checked red with the guard removed) |
| BASE-6 | pass | comments in English, explain why |
| BASE-16 | pass | engine and `moved_path` reused |
| BASE-23 | pass | each scope-ending path and each refusal tested; 29/29 |
| BASE-25 | pass | notes: fmt and clippy clean |
| BASE-32 | pass | D328, D247 (writing replace untouched), D283/D284, USER-5 |
| BASE-47 | pass | readers: label (wide and narrow), start, preview, apply, ctrl+f / `open_bar` / `close_box`, `roots_changed`, `paths_moved`, group root label |

APPROVE
