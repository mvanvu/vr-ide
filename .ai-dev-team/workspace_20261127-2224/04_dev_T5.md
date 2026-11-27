# T5 AI panel (D7, D14) — worktree .worktrees/T5, branch run4-T5 (base 0470b1b)

Tools available: Agent, Artifact, Bash, Edit, Read, Write, Skill, ToolSearch, SubagentHandback (+ deferred: TaskStop, Monitor, ...).
Status: DONE-pending-T1-wiring (restore code is behind `ai::restore::{capture, restore}` with its own serde struct; session.rs / cli.rs / main.rs untouched).

## Progress
- read: task row, spec D7/D11/D14, ui-spec S1/S2/key plumbing, 6 mockups, SEC-8, terminal machinery.
- warm build in background (`cargo test -p vr-ide -p vr-config --no-run`), ok.
- red: vr-config tests (`ai_context_keeps_only_its_table`, `ai_tools_are_checked_and_ai_presets_still_read`, KEYSET_V2 rows) failed to compile (no `Context::Ai`, no `ai_tools`) → green after keymap/settings changes (26 passed).
- code: ai/{mod,spawn,picker,restore}.rs, ui/ai_view.rs, layout `place_ai`, app/terminal/status/theme/event-loop wiring.
- red→green: ai_tests (13 incl. unit), ai_view_tests (7) — first runs failed on test-script printf and expectations, fixed.
- related run: whole vr-ide + vr-config (app.rs key/mouse routing and render are shared) — failures were the welcome screen's new `f12  AI panel` line in 17 existing snapshots + 2 tests whose removed-key / no-key lists the new keys changed; updated; snapshots regenerated once (`INSTA_UPDATE=always`), diff checked: only the welcome hint shift.
- guard + final run: see below.

## Files changed
- vr-config: `defaults/keymap.toml` (global f12 / ctrl+f12 / f7 / shift+f7 / f3, `[terminal]` + f12 / ctrl+f12, new `[ai]`), `defaults/settings.toml` (`ai_width_percent`, `[[ai_tools]]` claude / codex / gemini / aider / cursor-agent with `continue_args` / `resume_args`), `src/keymap.rs` (`Context::Ai`, no global fallback), `src/settings.rs` (`AiTool`, `valid_tool_name`, checks, `ai_presets` read as `ai_tools`), `src/lib.rs`, `tests/config.rs`.
- vr-ide new: `src/ai/mod.rs` (panel state, commands, keys, mouse, sessions), `src/ai/spawn.rs` (SEC-8), `src/ai/picker.rs` (S2 + D14 second step), `src/ai/restore.rs` (serialisable state, capture/restore), `src/ui/ai_view.rs` (tab bar fit rule, grid, picker, exited line, separator, status), `src/ai_tests.rs`, `src/ui/tests/ai_view_tests.rs` + 9 snapshots.
- vr-ide changed: `app.rs` (`Focus::Ai`, `App.ai`, key/paste/mouse routing), `commands/terminal.rs` (TermTab helpers, `spawn_sized`, `right_in` false while the AI panel shows (L1-A), ctrl+f7 notice, `focus.editor` keeps a covering panel, quit counts AI sessions), `ui/terminal_view.rs` (`render_cells` shared grid drawing), `ui/layout.rs` (`place_ai`, `AppLayout.{ai, ai_sep}`), `ui/mod.rs`, `ui/status_bar.rs`, `ui/welcome.rs` (`f12  AI panel`), `theme.rs` + `theme/synthwave.rs` (tokens `mode_ai`, `attention`, `note`, `diff_added_bg`, `diff_removed_bg` in synthwave / dracula / dark / light + contrast checks), `event_loop.rs` (AI refresh on the shared wake, poll, shutdown), `commands/{registry,dialog,explorer,slash}.rs` (commands, `CloseAiSession`, tab cycle → AI panel, `/ai <tool> [@account]`, `/ai rename <name>`), `config_reload.rs`, `slash_tests.rs`, `app_tests.rs` (REMOVED_V2), `ui/tests/welcome_tests.rs`, 17 existing snapshots (welcome line).
- docs: `docs/keymap.md` (Global rows, `focus.editor` = f3, `[ai]` section, `[terminal]` nine keys, read-mode tab row, grow/shrink rows, built-in rows for picker and session, context list), `docs/commands.md` (`/ai`), `README.md` (### AI panel).

## Tests added
- vr-config: `ai_context_keeps_only_its_table`, `ai_tools_are_checked_and_ai_presets_still_read`, KEYSET_V2 rows, settings defaults (`ai_tools`, `ai_width_percent`).
- `ai::spawn::tests` (args per mode, display, `resolve` absolute-only incl. `./claude` and `claude; touch …`, PATH/env cleaning).
- `ai_tests.rs`: f12 empty picker / hide; second step New / Continue / Choose with argv, canonical cwd, no `VR_IDE_*`; esc back; hide-show-step-focus-width (agents keep running hidden; ctrl+f8 only in the panel); focused session gives ctrl+w / ctrl+n / f5 / f6 to the agent; close asks while running → empty state; Enter restarts exited with continue args; `/ai` + rename; L1-A right terminal at the bottom + ctrl+f7 notice; quit counts sessions; restore (continue args, keys stay, three refusals, nothing run, capture); tab cycle.
- `ui/layout.rs` `ai_panel_widths_follow_the_spec` (mockup widths 120x40 / 80x24 ± explorer / 40x12).
- `ui/tests/ai_view_tests.rs`: snapshots ai_panel 120x40 / 80x24 / 40x12, ai_panel_six 80x24 (fit rule + `+N`), ai_picker 120x40, ai_picker_second 120x40 / 40x12, ai_empty 80x24, ai_exited 80x24; asserts the exact mockup bar ` 1 claude  2 @work ◆  3 codex    + ` at 120x40 with explorer, focus-coloured separator, attention-coloured mark, `mode_ai` badge; short labels; tokens in every theme.

## Commands run + result (trimmed)
- `cargo test -p vr-config` (red, before the change): `error[E0599]: no variant ... named Ai found for enum vr_config::Context`; `error[E0609]: no field ai_tools on type Settings` → after: `test result: ok. 26 passed; 0 failed`.
- `cargo test -p vr-ide ai_tests` first run: 4 FAILED (test script printf repeated per argument, esc test on a non-resuming tool) → fixed tests → `ok. 13 passed`.
- `INSTA_UPDATE=always run-heavy.sh cargo test -p vr-ide` (snapshots regenerated once): 736 passed, 1 failed (ssh_tests::sec_s12_labels…, timing; alone: `ok. 1 passed`).
- Guard `cargo fmt --all --check && run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings`: `Finished dev profile … guard exit 0` (qa-artifacts/T5/guard.log).
- Final `INSTA_UPDATE=no run-heavy.sh cargo test -p vr-ide -p vr-config` (qa-artifacts/T5/test2.log): vr-config `ok. 17 passed`, `ok. 26 passed`, `ok. 8 passed`; vr-ide `ok. 737 passed; 0 failed; 7 ignored`; `test exit 0`.
- Formatting: `rustfmt --edition 2024` on the changed .rs files only (BASE-38).

## Deviations from the plan (with reason)
- `f6` (`ai.changes`), `f5` / `shift+f5` (`ai.keep` / `ai.revert`) are not bound: they belong to T6's changes list; with T5 f5/f6 reach the agent (tested). T6 adds the bindings and the `±N` / f6 parts of the status and tab bar.
- Accounts (T8): data model only — `AiSession.account`, label `@account`, `spawn::account_env` hook (empty), picker shows `default` rows; no `+ Add account…` / `del remove` in the picker footer; `/ai claude @x` is refused ("only default is available"); a restored session with an account is refused with the SEC-18 notice text.
- Attention (T7): `AiSession.attention` + `ai::attention_target` used by f12; nothing sets it yet.
- Picker second step (no mockup): one compact list under the tool name — `New`, `Continue last  --continue`, `Choose earlier  --resume` (args right-aligned, dim); snapshots `ai_picker_second_*`.
- Exited line is drawn on the grid's last rows (wrapped, popup colours) rather than flowing after the output as in ai-exited-empty-80x24.
- Tab bar: the picker tab loses its `x` at fit level 2 like any active tab (the mockup kept `4 new x` with numbers-only neighbours); `+` is hidden while the picker tab is open (as in the mockup).
- `ai_presets` renamed to `ai_tools` (spec name); an old user file's `ai_presets` still loads as `ai_tools` (test).
- Session restore wiring waits for "T1 merged" (brief).

## Out-of-scope issues noticed
- `ssh_tests::sec_s12_labels_come_from_the_target_and_the_status_row_cannot_be_drawn_over` failed once in the whole-crate run under load and passed when rerun alone (timing; not touched by T5).

## New states
- AI panel visible / hidden; picker open (with `back`) / empty state; second step; session running / exited; separator drag; selection / mouse capture. Quit asks while sessions run (dialog); hidden sessions keep their PTY; close hangs up the process group off the UI thread (`TermTab::close_later`); shutdown joins them in parallel.

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| PTY tabs, spawn, keys/paste/mouse encoding, selection copy | capabilities.md, commands/terminal.rs | reused `TermTab`, `spawn_program` → `spawn_sized`, `term_key`, `selected_text`, `clamp_cell`, `mouse_kind`, `drop_later` (no second PTY stack) |
| Grid drawing on the panel colours | ui/terminal_view.rs | factored `render_cells` out of `render_grid`; both panels draw through it |
| PATH lookup over absolute entries | ssh/mod.rs `find_in_path` | reused in `ai::spawn::resolve` |
| Display sanitizing, `~` paths | ui/safe_text.rs, git.rs `home_relative` | reused |
| Confirm dialog, palette open + typed input | ui/dialog.rs, slash::set_input | reused |
| Layout split | ui/layout.rs | new `place_ai` (the right-terminal helpers are editor-column based; the AI panel spans the tab row too) |
| Session file store | T1 (in parallel) | own `AiPanelState` (serde) until T1 merges |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| AC-8 / D7 column, full height | pass | `ui::layout::ai_tests::ai_panel_widths_follow_the_spec` (120x40: editor 54, sep 84, panel 35, rows 0..39); snapshot ai_panel_120x40 |
| AC-8 sessions as tabs, labels + overflow rule | pass | `ai_view_tests::{ai_panel_sessions (exact mockup bar), ai_panel_six_sessions_fit, ai_panel_short_labels}`; `ai_tests::picker_second_step…` (`claude`, `claude·2`) |
| AC-8 ai_tools default, only installed offered, cursor-agent | pass | vr-config `settings_defaults_and_override`; `ai_tests::f12_opens_the_empty_picker…` (missing = aider); ai_picker snapshot `Not on PATH: aider, cursor-agent` |
| AC-8 new session in active file's workspace folder | pass | `ai_tests::picker_second_step…` asserts `CWD:<canonical root>`; `ai::start_folder` uses `terminal::new_tab_dir` |
| AC-8 f12 show/hide, agents keep running, attention hook | pass | `ai_tests::hide_show_step_focus_and_width`; `ai::attention_target` (T7 sets `AiSession.attention`) |
| AC-8 ctrl+f12 picker, f7/shift+f7, f3, ctrl+f8/f9 | pass | `ai_tests::{picker_second_step…, hide_show_step_focus_and_width}`; vr-config `ai_context_keeps_only_its_table` |
| AC-8 /ai <tool> in this panel; ai.rename / ai.close in palette | pass | `ai_tests::slash_ai_opens_in_the_panel_and_renames`, `close_asks_while_running…`; `slash_tests::ai_starts_only_tools_of_ai_tools_never_through_a_shell` |
| AC-8 exited state `[exited n] enter: start again` | pass | `ai_view_tests::ai_exited_session`, `ai_tests::enter_restarts_an_exited_session_with_continue_args` |
| L1-A (D12) right terminal at the bottom while shown, notice | pass | `ai_tests::right_terminal_goes_below_the_editor_while_the_panel_shows` |
| AC-13 / D14 second step, per-tool args, no history read | pass | `ai_tests::picker_second_step_starts_each_mode_with_its_arguments` (argv `--base|--continue|`, `--base|--resume|`, gemini without the step); `ai::spawn::tests::args_per_mode_and_display`; ai/ opens no tool files |
| D11 accounts: data model only | pass | `AiSession.account`, label `@account` (ai_panel snapshots), `spawn::account_env` hook; picker shows `default` rows only |
| Session restore behind own struct; no session.rs/cli.rs/main.rs edits | pass (wiring pending T1) | `ai::restore::{AiPanelState, capture, restore}`; `ai_tests::restore_reopens_with_continue_args_and_refuses_the_rest`; git status lists none of the three files |
| SEC-8 settings-only argv, no shell, absolute PATH, canonical cwd, env | pass | `spawn::tests::{resolve_uses_absolute_entries_only, path_and_env_are_cleaned}`; restore test (`claude; touch …` refused, marker file absent); `VRIDE:0` in the shim output; restore sends no input (shim screen shows only argv/cwd) |
| Key plumbing (global, [terminal] f12/ctrl+f12, [ai], lost keys, f5 reaches the program) | pass (f5/f6 bindings left to T6) | vr-config `defaults_load_without_user_file`, `ai_context_keeps_only_its_table`; `ai_tests::a_focused_session_gives_other_keys_to_the_agent`; docs/keymap.md `[ai]` |
| USER-5 keys reachable without kitty | pass | only F / ctrl+F keys added; `app_tests::removed_keys_run_nothing` list updated |
| Docs (keymap.md, README, commands.md) | pass | docs/keymap.md, README.md `### AI panel`, docs/commands.md; the docs-table test passes |
| Look: S1/S2 mockups, tokens in every theme | pass | 9 ai_view snapshots; `ai_view_tests::ai_tokens_in_every_theme`; theme contrast tests include `mode_ai`, `note`, `attention` |
| BASE-11 no checkout/restore/stash/reset | pass | none run |
| BASE-14 no waiting loops | pass | long runs as background tasks, turn ended |
| BASE-15 stop only own tasks | pass | nothing stopped by name; nothing left running |
| BASE-16 reuse | pass | `## Reuse` table; TermTab / vr-term reused, no second PTY stack |
| BASE-17 no SDK installs | pass | none |
| BASE-18 no sub-agents | pass | none started |
| BASE-19 notes from the start | pass | this file, progress lines |
| BASE-24 never stage or commit | pass | git status: unstaged changes only |
| BASE-25 guard | pass | guard.log `guard exit 0` |
| BASE-34 / DESIGN-23 no faked UA / unofficial endpoints | n-a | no network use |
| BASE-38 formatters on changed files only | pass | `rustfmt --edition 2024 <changed files>` |
| COST related tests, snapshots regenerated once, red first | pass | red vr-config compile errors first; whole-crate run because app.rs routing and render are shared; one `INSTA_UPDATE=always` run |
| COST-8 edits via Edit / perl, no python heredocs | pass | Edit / Write / perl only |
| COST-9 nothing left running | pass | background tasks finished; test agents end with their app |
| COST-13 throwaway in qa-artifacts/T5, nothing in $TEAM_TOOLS | pass | qa-artifacts/T5/{test1,test2,guard}.log |
| Comments English, why-only, no history | pass | ai/*.rs, ui/ai_view.rs |

## Handover
- State: T5 is DONE-pending-T1-wiring. No edit is half done; the worktree builds, guard passes, `cargo test -p vr-ide -p vr-config` is green (vr-config 17+26+8, vr-ide 737 passed / 0 failed / 7 ignored; logs qa-artifacts/T5/{guard.log,test2.log}). Nothing staged or committed; nothing running.
- Done: every file listed under "Files changed" above; tests under "Tests added"; 9 new ai_view snapshots + 17 existing ones regenerated once (welcome `f12  AI panel` line only); docs and capabilities.md updated.
- Left (exact):
  1. After the leader says "T1 merged": merge master into run4-T5 (the leader merges; resolve conflicts in shared files: README, docs/keymap.md, docs/commands.md, keymap.toml, app.rs, ui/mod.rs, slash.rs, terminal.rs), then wire `crate::ai::restore::{capture, restore}` into T1's session struct (`ai` section ← `AiPanelState`): save = `capture(app)`; restore = `restore(app, &state)` after the workspace roots are restored (it checks roots against `app.workspace`). Add one test in T1's session tests that a saved panel round-trips and refusals give notices. Rerun the guard and `cargo test -p vr-ide -p vr-config`.
  2. Nothing else for T5. f5/f6/shift+f5 bindings and `±N` belong to T6; attention marking (`AiSession.attention`, `ai::attention_target`) to T7; accounts (`AiSession.account`, `spawn::account_env`, picker rows `+ Add account…`, `del remove`) to T8.
- Known failing tests: none. Flaky under load once: `ssh_tests::sec_s12_labels_come_from_the_target_and_the_status_row_cannot_be_drawn_over` (passes alone; not T5 code).
- Design decisions taken: see "Deviations from the plan" (f5/f6 left to T6; `ai_presets` → `ai_tools` with old key still read; picker second step = one compact list New / Continue last / Choose earlier with args right-aligned; exited line drawn on the grid's last rows; picker tab drops its `x` at fit level 2; `+` hidden while the picker tab is open; under 60 columns right of the explorer the panel takes the whole area and keys go to it). Session labels: `·k` counts earlier sessions with the same tool+account; inactive short label drops the tool when it equals the active session's tool.
- Half-edited: nothing.

## Quick pass 1 + wiring
Worktree .worktrees/T5 (branch run4-T5 = c8675ed + master with T1). Second developer (first one's context full).
Tools available: Agent, Artifact, Bash, Edit, Read, Write, Skill, ToolSearch, SubagentHandback (+ deferred: TaskStop, Monitor, ...).
### Progress
- read: T5 notes + handover, T1 notes (AI panel API), SEC-8 / SEC-18, D15, QC T5, session.rs, ai/{mod,restore}.rs, ui/ai_view.rs, quit path in commands/terminal.rs.
- red: 5 new / changed tests in `crates/vr-ide/src/ai_tests.rs`; first run (after fixing the merge's compile error, see below): `test result: FAILED. 14 passed; 4 failed` — `the_panel_round_trips_through_the_session_file` (ai_tests.rs:491, no `label = "review"` in the file: nothing reported), `a_refused_session_is_not_written_back` (:529, 0 sessions reopened), `an_exited_session_scrolls_back` (:597, scroll 0), `the_exited_line_is_drawn_below_the_agent_output` (:621, `LAST` covered by the exited line). `the_ai_quit_question_keeps_the_edit_mode` passed on the first run: the AI question is already the shared `DialogAction::QuitTerminals` question, answered by `terminal::answer_quit` → `app.quit` command (contract holds; the test pins it).
- code: wiring, scroll keys, D15, vr-config / README minors (below) → green.
- guard + related run (below). Nothing running.

### Changes
- Merge fix: `commands/terminal.rs` `TermTab::new` (T5) lacked T1's new `cwd` field → `cwd: None` (the merged branch did not compile).
- `ai/restore.rs`: own serde struct removed; works on T1's `session::AiSection` (account `""` = default, label = the AI: Rename Session name, width only when it differs from `ai_width_percent`; T1's section has no active index, so a restore activates tab 1). New `from_session(app)` (takes `take_restored_ai` once, restores, then reports the panel so refused tabs are not written back) and `report(app)` (calls `set_ai` when the capture differs from the last one; a no-op before `from_session` and without `--session`). Restore rules unchanged: tool by name in `ai_tools` and resolved on the absolute PATH, folder a live canonical workspace root, any account refused (T8), one error notice per refused tab, continue args, no input, keys stay.
- Hooks: `main.rs` calls `ai::restore::from_session` right after `session::restore` (roots, tabs, terminals back first); `session.rs` calls `ai::restore::report` before the snapshot in `tick` (each check) and in `save_on_quit` — 1 line each. Deviation: "set_ai on every panel change" is done as a report before every save check instead of a call at each of the ~15 mutation sites; the file sees the same state, and no mutation site can be missed.
- `ai/mod.rs`: `AiPanel.reported`; `handle_key` handles shift+pageup / pagedown before the exited early return (exited sessions scroll back); `fit` gives an exited session its grid less the exited line's rows.
- `ui/ai_view.rs` (D15): `exited_lines` / `reserved_rows` (wrapped line count, the program keeps at least one row); the grid is drawn in the rows above, the exited line in the reserved rows below (overlay on the last row only when the grid has a single row). The real emulator shrinks by pushing its top lines into the history, so they remain reachable with shift+pageup. Snapshot `ai_exited` unchanged on rerun (its fake grid has blank last rows, and the line still sits on the last rows), so nothing was regenerated.
- vr-config: `AiTool::resumes()` deleted (no callers: `grep -rn "resumes()"`); settings.rs comment → `// Legacy key: ai_presets is read as ai_tools.`; `defaults/settings.toml` documents the legacy key, "set only one of the two", and continue_args / resume_args for Continue last / Choose earlier.
- README ### AI panel: legacy key, exited scroll back + exit line below, `--session` restore of the panel and its refusals.

### Tests (ai_tests.rs)
- `restore_reopens_with_continue_args_and_refuses_the_rest` (on `AiSection`; + `aider not on PATH`; no input sent = the agent screen holds only its 3 start lines; capture equals the expected section).
- `the_panel_round_trips_through_the_session_file` (two `--session` windows on one state dir: shown, width, 2 sessions, name `review`, claude `--continue` / gemini plain, canonical cwd, keys stay; no `--base` or script path in the file; hide + close → `shown = false`, gemini gone).
- `a_refused_session_is_not_written_back` (tool removed from ai_tools → one exact notice, next save keeps only claude).
- `the_ai_quit_question_keeps_the_edit_mode` (running AI session, edit mode, app.quit → question "1 AI session still running" → `dialog::answer` Chosen(0) → `app.quit`, session file `edit = true`).
- `an_exited_session_scrolls_back`, `the_exited_line_is_drawn_below_the_agent_output` (D15).

### Commands (trimmed)
- red: `cargo test -p vr-ide -- ai_tests` → `test result: FAILED. 14 passed; 4 failed` (above).
- green: `cargo test -p vr-ide -- ai_tests ai_view` → `test result: ok. 25 passed; 0 failed`.
- format: `rustfmt --edition 2024` on the changed .rs files only.
- guard: `run-heavy.sh sh -c 'cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings'` → `Finished dev profile … guard exit 0` (qa-artifacts/T5/guard-qp1.log).
- related: `INSTA_UPDATE=no run-heavy.sh sh -c 'cargo test -p vr-ide -- ai session && cargo test -p vr-config'` (qa-artifacts/T5/test-qp1.log): 106 passed, 1 failed — `not mine: ssh_tests::ac_s2_ac_s3_a_session_starts_as_a_tab_and_goes_full_screen_after_login — s3cret-pw shows in:` (matched by the `session` filter; timing under load from parallel worktrees); alone: `ok. 1 passed`. Rerun of the set: `ok. 107 passed; 0 failed` (qa-artifacts/T5/test-qp1-rerun.log, incl. 7 ai_view_tests snapshot tests). vr-config: `ok. 17 passed`, `ok. 26 passed`, `ok. 8 passed`.

### Rule compliance rows touched (quick pass 1)
| Rule | Status | Evidence |
|---|---|---|
| AC-8 | pass | ai_tests 25 green; exited session scroll + D15 tests |
| AC-13 | pass | restored sessions start with continue args (round-trip test argv) |
| SEC-8 | pass | restore test: `claude; touch …` not in ai_tools, `aider` not on PATH, folder outside → refused, marker absent, no input; file holds names only |
| SEC-18 | pass | account tab refused with notice, never default (`restore_reopens_…`); T8 adds accounts |
| BASE-11 | pass | no checkout / restore / stash / reset used |
| BASE-14 | pass | heavy runs through run-heavy.sh; no waiting loops |
| BASE-15 | pass | nothing started outside tests; test agents ended by `quit_window` / drop |
| BASE-19 | pass | this section |
| BASE-20 | pass | `resumes()` grep: no callers before deletion |
| BASE-24 | pass | nothing staged or committed |
| BASE-25 | pass | guard exit 0 |
| BASE-38 | pass | rustfmt only on the changed files |
| COST-15 | pass | no snapshot regenerated (unchanged) |
| Comments English, why-only, no history | pass | settings.rs legacy-key comment replaced |
