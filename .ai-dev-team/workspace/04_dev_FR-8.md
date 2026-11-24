# FR-8 notes

- Red (major): dialog_tests::keep_mine_recreating_a_deleted_file_brings_its_row_back — "no row after Keep mine recreated the file". Green after keep_mine's Ok branch calls explorer::note_written (keep_mine write unchanged).
- Red (minor 1): explorer_tests::saving_a_filtered_file_does_not_re_read_the_folder — left 3, right 1. Green after Explorer.filtered set (cleared per folder on re-read, so toggles and watcher reloads re-evaluate); test-only RELOADS counter in explorer.rs.
- Red (minor 2): commands::search::gate_tests::being_replaced_refusal_is_a_warning_on_every_path — Error vs Warning. Green: search::report_open_error used by explorer (open, new file), palette, search panel; wording now "try again in a moment" (fits Save As too).
- Final: `cargo test -p vr-ide --lib` 173 passed / 0 failed / 3 ignored; fmt check and clippy -D warnings clean.

## New states (round 1)
- explorer.filtered: only affects whether a save re-reads a folder; quit/cancel n-a; a re-read of the folder (watcher, toggle, reload_all) forgets entries; a file un-filtered meanwhile gets its row from that re-read; no I/O, no data written.

## Rule compliance
| Rule | Evidence |
| BASE-1,2 | only listed files; keep_mine write untouched |
| BASE-3 | red lines above |
| BASE-4,10,25 | test/guard output above |
| BASE-6 | English comments |
| BASE-11,24,38 | no git mutations; rustfmt on changed files |
| BASE-14,15,18,19 | nothing background; notes kept |
| AC-3,7,8,12 | save/keep-mine/open paths covered by tests above |
n-a: BASE-5,16,17,32 (no heavy cmds, no new reusable, no installs, no new decisions), COST (related tests only)
