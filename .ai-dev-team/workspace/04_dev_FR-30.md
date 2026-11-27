# FR-30 Editor tabs: x close button (D327)

Worktree .worktrees/FR-30 (fr-30). Progress: tests written first (compile-red: `tabs::close_offset` missing), implemented, snapshots regenerated once, walk run, clippy clean.

## Files
- crates/vr-ide/src/ui/tabs.rs: label ends " x " (x dim on inactive tabs, as the terminal tabs); `close_offset`, `Part`, `hit_part`.
- crates/vr-ide/src/commands/file.rs: `close` split into `request_close(app, i)` (ctrl+w uses it with the active tab; dialog is `CloseTab(DocRef::of(i))`).
- crates/vr-ide/src/app.rs: tab press routes `Part::Close(i)` to `request_close`, `Part::Label(i)` keeps the old path (switch + T25 double click).
- crates/vr-ide/src/workspace_ui_tests.rs: 6 new tests (x on every tab; inactive clean closes only it, active stays; x on active; dirty opens dialog, Esc keeps; label/padding click switches; overflow at 40/80/120 keeps the active x reachable; double click).
- README.md line 148.
- Close of a non-active tab does NOT switch to it first (close_doc already keeps `active` pointing right).
- A label cut by the bar edge has no clickable x (`close_offset` -> None); the active tab is always whole unless wider than the bar. Editor bar has no `+N` marker: it scrolls to keep the active tab.

## Snapshots regenerated (42, once, INSTA_UPDATE=always): only the tab row gained " x"
ui tests: render_confirm_dialog (40x12, 80x24), render_explorer_80x24, render_explorer_menu (120x40, 40x12, 80x24), render_explorer_narrow, render_file_with_selection (40x12, 80x24), render_find_bar (3), render_find_box_empty (3), render_long_line_horizontal_scroll (2), render_notice_and_read_only_status (2), render_palette (2), render_replace_preview (3), render_search_panel (3), render_tabs_modified (2), render_wide_chars (2); blame_column render (3); git_tests render_git_gutter_status_and_blame (3); find_tests render_replace_dialog_80x24; terminal_tests render_close_terminal_dialog_80x24; workspace_ui_tests render_multi_root_explorer (3).

## Commands
- `cargo test -p vr-ide --lib -- tab ui:: app_tests workspace_ui`: ok after snapshots (164+ passed).
- `cargo test -p vr-ide --lib`: 686 passed once; later runs under parallel load hit one timing flake in ssh_tests::ac_s6_sec_s16_sixteen_sessions_at_most, passes alone (1 passed) and with ssh_tests+terminal_tests (80 passed).
- `cargo clippy -p vr-ide --all-targets -- -D warnings`: clean. rustfmt run on my 3 files only.
- Walk: `python3 -I qa-artifacts/FR-30/walk.py <bin> <dir> <rows> <cols>` at 120x30, 80x30, 40x24: 6/6 each (x on all tabs, x on inactive a.txt closes only it, label click, dirty marker, x on dirty tab opens dialog, Esc keeps tab). Not walked: ssh/pty-heavy overflow (covered by unit test at 40/80/120).

## Reuse
| Need | Reused |
| x hit testing idea | terminal_view tab_bar `close` offset (cut label = no x) |
| close with dialog | file.rs close/CloseTab dialog (extracted to request_close) |

## Rule compliance
pass: BASE-3 (compile-red first), BASE-4, BASE-16, BASE-23, BASE-25, BASE-32, BASE-37, COST-8, COST-15, BASE-14, BASE-15, BASE-38, COST-13.
n-a: all other BASE-1..48 / COST-1..20 IDs (no server, no new deps, no commits; nothing outside scope).
Nothing left running.

## Profile guard (BASE-25), final code
`run-heavy.sh sh -c 'cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings'`
fmt --check: no diff (clippy ran after it in the && chain); clippy: `Finished dev profile ... target(s) in 0.91s`, no warnings or errors.

## Quick pass 1
Tests only (crates/vr-ide/src/workspace_ui_tests.rs), no behaviour change, no defect found:
- `x_on_a_dirty_inactive_tab_with_discard_closes_only_it_and_keeps_a_copy`: dialog opens with nothing changed (active stays tab 1); Discard -> tab gone, other tab intact and unmodified, a.rs on disk untouched, exactly one recovery file.
- `x_on_a_dirty_inactive_tab_with_save_writes_that_file_and_leaves_the_other_tab`: Save -> a.rs "za\n" on disk, b.ts untouched, only a closed, active tab unchanged.
- `the_double_click_still_reveals_with_the_x_in_place` now asserts hidden before, visible + focused after, nothing closed, cursor on a.rs.
`cargo test -p vr-ide --lib workspace_ui`: 35 passed. Guard (fmt --all --check + clippy --workspace --all-targets -D warnings): clean, `Finished dev profile`.
