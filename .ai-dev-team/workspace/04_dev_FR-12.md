# FR-12 notes (Phase 5 fixes after M5; mode fix; side fe)

Tools available: Bash, Read, Write, Edit, Skill, Agent (unused), ToolSearch, SubagentHandback (plus MCP tools, unused).

## Progress
- started; reading welcome.rs, status_bar.rs, app.rs
- MAJOR 1: red (legacy welcome showed ctrl+shift+f) -> green; hints() filters legacy_unreachable keys when !keyboard_enhanced, hint omitted when none (D? FR-12: omit rather than 'f1 Search (palette)'); welcome_app() now sets keyboard_enhanced=true so existing snapshots keep the full key set
- D220: ctrl+alt+f added to keymap.toml (search.project), docs/keymap.md, README; test search_tests::ctrl_alt_f_opens_the_project_search_panel red (binding off) -> green (event ctrl+alt+f == crossterm parse of ESC+^F, tested enhanced and legacy); welcome now shows ctrl+alt+f (shortest reachable key) incl. legacy snapshot welcome_legacy_keyboard_120x40
- MAJOR 2: red (notice cut/lowercase) -> green; Notice gets a detail field (key list, last), status_bar wraps/cuts with an ellipsis; text 'Keyboard: this terminal cannot send some keys; use the command palette (f1).  (Esc)  Unavailable: <keys>'
- minor a: red(compile)->green truecolor_cells_follow_the_colour_depth (cell_style takes ColorDepth; commands::terminal::child_env sets COLORTERM only at TrueColor; vr-term no longer forces COLORTERM, pty test passes it via opts.env). minor b: red->green the_status_bar_names_the_way_out_before_the_size (status right part tiers: i/n · size · alt+q editor, then i/n · alt+q editor, then size, then i/n)
- minor c: compact welcome drops root_line (test compact_welcome_does_not_repeat_the_root_and_shows_one_more_hint). minor d: EDITOR_MIN_ROWS=3 in place_terminal, focus.editor keeps keys when editor has 0 rows, monitor samples only when docs empty and layout.editor.height>0 (tests a_tall_panel_leaves_the_editor_a_few_rows, a_zero_row_editor_never_gets_the_keys_or_a_monitor; both red first)
- minor e: red->green a_covered_terminal_gets_no_keys_and_showing_it_uncovers_it (covered() guard in terminal::handle_key; reveal hides a covering search/explorer column). minor f: the_fast_poll_runs_only_while_a_terminal_is_running (green on first run: the poll_interval change and test were compiled together; red not observed separately)
- D221: ctrl+alt+{s,e,f,h,k,1,`} bound (keymap.toml global; terminal.new also in [terminal]), no collisions (only ctrl+alt+enter existed); docs/keymap.md + README updated. Terminal paste: an emulator's bracketed paste (Event::Paste -> terminal::paste) already worked; existing test paste_goes_to_the_terminal_bracketed_when_the_program_asks covers handle_paste
- D221 tests: app_tests::ctrl_alt_alternatives_run_the_same_commands (all 7 chords, enhanced and legacy; ESC+ctrl-letter is crossterm's ctrl+alt+letter event) and legacy_notice_lists_only_chords_without_a_reachable_alternative (red -> green after the notice grouped lost keys per command and legacy_unreachable learned ctrl+alt+` = ESC NUL). Caveat: ctrl+alt+1 and ctrl+alt+` are not deliverable on a legacy keyboard (ESC 1, ESC NUL), so they stay in the notice list.
- Decisions: D? (FR-12) welcome Search hint on a legacy keyboard shows the reachable ctrl+alt+f (shortest reachable key); a hint with no reachable key is omitted. D? (FR-12) hints pick the shortest reachable key, so enhanced welcome now shows ctrl+alt+f rather than ctrl+shift+f. No ctrl+alt+<key> collisions found.

## Snapshots regenerated once (INSTA_UPDATE=always)
Changed: welcome_120x40/80x24/40x12 (Search key ctrl+alt+f, compact welcome without root row, +1 hint), render_empty_project 40x12/80x24, render_terminal_panel 120x40/80x24/40x12, render_exited_terminal 80x24/40x12, render_terminal_long_titles_80x24 (status bar "alt+q editor", legacy welcome hints), render_palette 40x12/80x24 (search.project shows both keys). New: welcome_legacy_keyboard_120x40.

## New states
Notice: "Keyboard: ...; use the command palette (f1).  (Esc)  Unavailable: <keys>" cut with an ellipsis at the end only. Focused terminal status: "i/n · cols×rows · alt+q editor" -> "i/n · alt+q editor" -> size -> "i/n". Terminal panel never takes the last 3 editor rows when the screen has 8+ body rows. Covered terminal (no cells): keys go to the editor, reveal hides a covering search/explorer column. Fast poll only with a running terminal.

## Rule compliance
pass: BASE-1 (only listed findings + D220/D221), BASE-3 (red lines above; exception: the fast-poll test was green on first run), BASE-4 (below), BASE-6, BASE-11, BASE-24, BASE-25, BASE-38, BASE-32 (D220/D221 honoured, USER-1 keys from keymap), COST-8, COST-15.
n-a: BASE-2, BASE-5, BASE-8, BASE-10 (vr-ide/vr-term/vr-config as the brief says), BASE-14/15/16/17/18/19 (no long jobs, no agents, no new reusable), all other IDs.

## Guard
cargo fmt --all --check: clean. cargo clippy --workspace --all-targets -- -D warnings: Finished, no warnings.
cargo test -p vr-ide -p vr-term -p vr-config: vr-ide 356 passed / 4 ignored; vr-term 7 + pty 1 + 1 + 1(ignored 1); vr-config 3 + 18 + 8 + 19 etc., 0 failed.
