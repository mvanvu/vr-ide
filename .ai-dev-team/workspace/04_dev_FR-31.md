# FR-31 Explorer right-click: Search / Replace in Folder (D328)
- Worktree `.worktrees/FR-31` (branch fr-31). No commit, nothing staged.
- Files changed: `crates/vr-ide/src/commands/{search,find,explorer,explorer_menu}.rs`, `crates/vr-ide/src/ui/find_bar.rs`, `crates/vr-ide/src/explorer_menu_tests.rs` (tests), `crates/vr-ide/src/ui/snapshots/vr_ide__ui__tests__render_palette_80x24.snap`, `README.md`, `docs/keymap.md`. `vr-search` unchanged: `search_roots` / `plan_replace_roots` take any root list, so the folder is passed as the only root (the planner and `apply_replace` refuse files outside it).
- Design: `SearchPanel.folder`; `scope_roots(app)` is read by `start` and `build_preview` (and through it the apply); group root labels off in folder scope; scope label `Project: <root/rel>` (`folder_label`, shortened from the left, narrow = ` P `). Folder scope ends: ctrl+f Project->File (`switch_scope`), `close_box` (Esc), a removed root (`roots_changed`); a change clears results/preview (`set_folder` -> `clear_results`), refused while a replace writes. Opening keeps the keys in the search field even with a kept query.
- Tests added (explorer_menu_tests.rs, all green): menu_offers_folder_search_for_a_root_and_a_folder_but_not_a_file; search_in_folder_by_mouse_and_keys_limits_results_to_the_folder; replace_in_folder_previews_and_writes_only_inside_the_folder; a_result_outside_the_folder_is_refused_in_the_preview_and_never_written; ctrl_f_leaves_the_folder_scope; folder_in_the_second_root_searches_only_that_folder; a_second_folder_choice_replaces_the_first_and_drops_its_results; replace_in_folder_gives_the_keys_to_the_search_field_even_with_a_kept_query. Red first: tests written before the code and failed to compile (no `SearchPanel::folder`); the field-focus test was added after the walk showed the kept-query case.
- Commands run:
  - `run-heavy.sh cargo test -p vr-ide --lib -- explorer search find palette ui::` -> `245 passed; 0 failed; 5 ignored`
  - `run-heavy.sh cargo test -p vr-search` -> `43 passed; 0 failed; 1 ignored`
  - `rule-checks.sh related-tests FR-31` printed the related set (covered by the runs above)
  - guard `run-heavy.sh sh -c 'cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings'` -> clean, exit 0
- Snapshots (once, `INSTA_UPDATE=always` on that one test): `crates/vr-ide/src/ui/snapshots/vr_ide__ui__tests__render_palette_80x24.snap` (+1 line "Explorer: Search in Folder", the new palette command). `render_explorer_menu` snapshot unchanged (file row).
- Pty walk on the real binary (`.ai-dev-team/workspace/qa-artifacts/FR-31/walk.py`, workspace `ws/` removed afterwards): PASS explorer shows src; menu has Search/Replace in Folder; scope shows `Project: src`; results a.txt+b.txt only; file menu lacks folder items; keyboard-chosen Replace in Folder on `other` opens replace row; preview lists only c.txt; c.txt replaced; a.txt/b.txt/top.txt untouched -> ALL PASS.
- Deviations: none. Out of scope: app.rs untouched. Note: Esc while a replace is writing keeps the folder until the replace ends and the box closes again (the label stays visible).
- Left running: nothing (walk process killed by pid, no servers/watchers).

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| Project search/replace engine and preview | capabilities.md, `vr_search::{search_roots, plan_replace_roots}` | reused as is, folder passed as the sole root; no vr-search change |
| Menu items / explorer commands | `commands/explorer_menu.rs`, registry | extended `open()` with `FOLDER_ITEMS`, two `explorer.*` commands |
| Test helpers | explorer_menu_tests.rs, search_tests.rs | reused menu helpers; small local `wait_search` copy (private in search_tests) |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | only task files; vr-search/app.rs untouched |
| BASE-2 | pass | AC-R1 behaviours covered by tests above; no contract changed |
| BASE-3 | pass | tests written before code, red (compile) first |
| BASE-4 | pass | Commands run section |
| BASE-5 | pass | run-heavy.sh for suites, build, guard; single-test runs direct |
| BASE-6 | pass | English comments, why-only |
| BASE-7 | pass | no commit, nothing staged |
| BASE-8 | n-a | no dependency added |
| BASE-9 | pass | folder canonical and inside a root; planner confines writes (test a_result_outside_the_folder...) |
| BASE-10 | pass | explorer/search/find/palette/ui:: + vr-search only |
| BASE-11 | pass | worktree only, no tree-wide history commands |
| BASE-12 | pass | nothing outside the repo except team notes and capabilities |
| BASE-13 | pass | no untracked file deleted; own walk workspace removed |
| BASE-14 | pass | all commands foreground |
| BASE-15 | pass | walk pid killed by pid |
| BASE-16 | pass | Reuse table |
| BASE-17 | pass | no toolchain installed |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | notes kept from the start |
| BASE-20 | n-a | nothing removed |
| BASE-21 | pass | no secrets in output |
| BASE-22 | pass | names follow explorer.* and FOLDER_ITEMS neighbours |
| BASE-23 | pass | root vs folder, search vs replace, mouse and keys, ctrl+f toggle each tested |
| BASE-24 | pass | no index/history commands |
| BASE-25 | pass | workspace fmt --check + clippy -D warnings clean |
| BASE-26 | n-a | no rewrite |
| BASE-27 | n-a | no shared data |
| BASE-28 | n-a | no third-party code |
| BASE-29 | n-a | no browser |
| BASE-30 | n-a | no shared app records |
| BASE-31 | n-a | no containers |
| BASE-32 | pass | D328, W-1/W-2 (multi-root test), D265 preview/confirm flow reused |
| BASE-33 | n-a | no other system |
| BASE-34 | n-a | no blocked step |
| BASE-35 | n-a | no USER rules apply |
| BASE-36 | pass | fe task in vr-ide only |
| BASE-37 | n-a | feature, not a runtime defect fix |
| BASE-38 | pass | rustfmt only on explorer_menu_tests.rs; fmt check clean elsewhere |
| BASE-39 | n-a | no schema |
| BASE-40 | n-a | no outward writes |
| BASE-41 | n-a | no credentials |
| BASE-42 | n-a | no runner settings changed |
| BASE-43 | n-a | no access rule |
| BASE-44 | n-a | no fake-data app |
| BASE-45 | pass | AC-R1 text as in the brief; spec file not edited |
| BASE-46 | pass | walk run on final binary (rebuilt after last edit) |
| BASE-47 | pass | readers: scope label, results, preview, apply, ctrl+f, close_box, roots_changed, group root label |
| BASE-48 | n-a | no machine/account state claimed |
| COST-1 | pass | run-heavy.sh |
| COST-2 | pass | developer tests only |
| COST-3 | pass | no polling |
| COST-4 | pass | no full suite |
| COST-5 | pass | nothing left |
| COST-6 | n-a | no audits |
| COST-7 | pass | pty walk only for the TUI check |
| COST-8 | pass | batched reads/edits; Edit tool used |
| COST-9 | pass | nothing running |
| COST-10 | pass | related set includes palette/ui tests |
| COST-11 | n-a | not e2e |
| COST-12 | n-a | leader rule |
| COST-13 | pass | walk.py under qa-artifacts/FR-31 |
| COST-14 | n-a | no CI |
| COST-15 | pass | one snapshot update |
| COST-16 | pass | outputs trimmed |
| COST-17 | n-a | no merge |
| COST-18 | n-a | no doomed run |
| COST-19 | n-a | no qc |
| COST-20 | n-a | no re-check round |

## Fix round 1
1. MAJOR data safety: fixed. `search::in_workspace(app, folder)` (canonical path inside a canonical current root) is the one check. `explorer::folder_under_cursor` returns None otherwise (menu items hidden, command refuses); `search::scope_ok` re-checks at `start`, `build_preview`, `apply` and in `roots_changed`: outside -> scope ends (folder None, results/preview dropped) with a warn notice "the folder is outside the workspace now", nothing searched or written. While a replace writes, confirmed files are left alone. Tests (red first for A, B; A re-checked red by removing the guard): a_subfolder_under_a_symlinked_dir_is_not_a_folder_scope (A), a_folder_outside_every_root_ends_the_scope_before_search_and_preview (B).
2. MAJOR open_bar: fixed (`set_folder(None)` in `open_bar`, covers ctrl+f, ctrl+r, find.next paths). Test ctrl_f_through_the_editor_path_also_ends_the_folder_scope (red before).
3. MAJOR missing tests: added esc_ends_the_folder_scope_so_the_next_project_search_is_whole, removing_the_root_of_the_folder_scope_ends_it, open_folder_is_refused_while_a_replace_writes (held apply worker, scope unchanged, outside file unwritten), changing_the_folder_drops_the_old_preview.
4. minor paths_moved: folder follows a rename (`moved_path`). Test renaming_the_scoped_folder_moves_the_scope_with_it (red before).
5. minor: narrow label ` P: <folder tail> ` (width/3 cells, min 4); `folder_label` cuts by display width (Span width). Test narrow_boxes_keep_a_folder_marker_and_labels_cut_by_cell_width (red before).
- New states: `scope_ok` ends the scope from `start` / `build_preview` / `apply` / `roots_changed`. Quit: unchanged (quit checks replacing first). Esc/cancel: unchanged. Input changing: a changed scope drops results and preview. Second request during a replace: refused as before. Concurrent writer: planner/apply root checks unchanged. Tests cover each ending path above.
- Commands: `run-heavy.sh cargo test -p vr-ide --lib -- explorer search find palette ui::` -> 254 passed, 0 failed, 5 ignored; `cargo test -p vr-search` -> 43 passed; guard `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings` clean; rustfmt only on explorer_menu_tests.rs. No snapshot changed this round.
- Walks on the rebuilt binary: `qa-artifacts/FR-31/walk.py` ALL PASS (new case A: expanded symlinked dir child offers no folder search); `qc_walk.py` ALL PASS at 120x40 and 80x24. At 110x30 five checks "fail" only because the script slices the explorer at a fixed 30 columns while the explorer is 26 wide there (screen dump shows correct results: a.txt, link_out.txt, b.txt under rootA/src); 110x30 is not a QC size.
- Rule rows touched: BASE-2 pass (AC-R1 via open_bar fixed), BASE-9 pass (in_workspace re-checked at every use), BASE-23 pass (Esc, root removal, refusal, preview drop, open_bar tested), BASE-47 pass (readers added: open_bar, paths_moved, roots_changed via scope_ok, scope label narrow). Nothing left running; walk workspaces removed.
