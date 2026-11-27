# T4 Palette Tab completion (D6, AC-7)
Worktree: .worktrees/T4 (branch run4-T4, base 0470b1b). Side fe.

## Progress
- read: spec D6/AC-7, ui-spec S5, mockups palette-tab-*/palette-ssh-*/palette-ai-commands, SEC-4; code commands/palette.rs, ui/palette.rs, commands/slash.rs (existing Tab: slash::tab/complete/path_candidates/DirListing).
- red: completion_tests.rs written first; `cargo test -p vr-ide completion` → 18 compile errors (no `commands::completion`, no `Palette.completion`, no `DirListing::len`), i.e. red because the feature is missing.
- code: commands/completion.rs (new), palette.rs (state + keys), slash.rs (bounded listing, cached_listing, unsafe names skipped, old slash::tab/complete removed), ui/palette.rs (tag, detail, folder colour, runs wrap).
- related tests green; snapshots regenerated once (INSTA_UPDATE=always, completion_tests only) and opened: tab_list_80x24, tab_list_40x12, tab_prefix_40x12, tab_prefix_80x24, ssh_host_80x24, ssh_path_120x40.
- docs: docs/commands.md Tab bullets, README one paragraph under slash commands.
- guard green. capabilities.md line added.

## Files changed
- crates/vr-ide/src/commands/completion.rs (new): sources, phases, ssh classifier, runs line, ai tool names
- crates/vr-ide/src/commands/palette.rs: `Palette.completion`, `Palette.moved`; Tab/Shift+Tab, Esc/Enter/Up/Down in the list; paste and edits end a completion
- crates/vr-ide/src/commands/slash.rs: `READ_LIMIT` 1000 on folder reads, `cached_listing`, `DirListing::len/is_empty`, `in_argument` pub(crate), control-char names skipped in path rows; removed `tab` / `complete` / `common_prefix` (replaced by completion.rs)
- crates/vr-ide/src/commands/mod.rs, crates/vr-ide/src/lib.rs: module lines
- crates/vr-ide/src/ui/palette.rs: `tab: list (N)` / `i/N` right of the input; completion detail (common part, key hint, `files in …`, `runs: ssh …` wrapped at spaces and after `/`); folder candidates in the folder colour (popup_border fg)
- crates/vr-ide/src/completion_tests.rs (new): 17 tests, 10 snapshots (crates/vr-ide/src/snapshots/vr_ide__completion_tests__palette_*)
- crates/vr-ide/src/slash_tests.rs: `ws_add_tab_completes_folders` second step is now Tab, Tab (D6 list) instead of Down, Tab
- docs/commands.md, README.md

## Behaviour (S5)
- One match completes (folder ends in `/`, no space; `/ssh` `/run` `/ai` words get a space; one-argument commands `/save-as` `/ws …` keep the text as typed, no space).
- Several: common part inserted, `tab: list (N)`, the row is the command's own, detail `Common part completed: …`. No common part: the list opens at once.
- List: rows = candidates (hosts with source right-aligned), input = selected candidate, `i/N`; tab / shift+tab / up / down step (wrapping); enter takes it (nothing runs); esc restores the text from before the list and keeps the palette (a second esc closes it); typing or cursor keys close the list and edit; a click takes the clicked candidate.
- A row highlighted with the arrows before Tab is taken as is (keeps the existing `/term` + Down + Tab behaviour).
- Tab outside slash mode: unchanged.

## Deviations / notes for the leader
- /ssh classifier: `completion::ssh_slot_at_end(line)` is a LOCAL stand-in (whitespace split, `\ ` escapes, quotes not parsed). At the T3 merge switch its body (and `ssh_roles`, used by `ssh_runs_line`) to T3's classifier. Host candidates use `app.ssh.recent` (`Target::as_str`); T3 changes the recent list to argument lines + base folder, so adapt `ssh_hosts()` then.
- /ssh path base (SEC-4): `active_root().path.canonicalize()`; a missing root completes nothing (no fallback). `-E`, `-S` and any `-o …File=` key are path arguments too.
- /ai tools: `ai_tool_names` reads `settings.value_text("ai_tools")` (works when T5 adds a top-level `ai_tools = [...]`; a `[[ai_tools]]` table array is not in `Settings::names()` and falls back to claude/codex/gemini/aider). The parser accepts a list of names or tables with `name`; only plain program names are kept. Switch to the typed field after T5 merges. `@account` not done (out of scope).
- /run file arguments complete relative to the active root (the terminal's cwd is not tracked).
- /ws bases unchanged (active root, `~` expanded).
- Bounded: 1000 entries read per folder (`slash::READ_LIMIT`, also for the existing typing rows), 100 shown; read_dir never opens a FIFO as a file; names with control/BIDI characters are not offered; labels sanitized; shell-special characters escaped with `\` in `/ssh` and `/run` words.
- 40x12: the list keeps at least one row; the detail takes the rows left (2 lines in tab_list_40x12); the counter and tag stay.
- `ssh_tests::ac_s2_ac_s3_a_session_starts_as_a_tab_and_goes_full_screen_after_login` failed once in a parallel related run (recent-file assertion, shim timing), passed on the single rerun and in the final related run.

## Commands + output (trimmed)
```
$ cargo test -p vr-ide completion          (before code)
error[E0432]: unresolved import `crate::commands::completion`
error: could not compile `vr-ide` (lib test) due to 18 previous errors
$ INSTA_UPDATE=always cargo test -p vr-ide completion_tests
test result: ok. 17 passed; 0 failed
$ cargo test -p vr-ide -- slash palette ssh completion workspace mode_tests terminal_tests::render_palette
test result: ok. 258 passed; 0 failed; 1 ignored; 0 measured; 479 filtered out; finished in 16.89s
$ cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.03s
guard exit: 0
```

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| folder listing cached per window | capabilities.md (`slash::DirListing`, `Palette.dir_cache`) | reused through new `slash::cached_listing`, cap added |
| command / value candidates | `slash::rows` | reused as candidate source |
| ssh config hosts | `slash::prepare` → `Palette.ssh_hosts` | reused |
| display sanitizing | `ui::safe_text::{sanitize, is_unsafe_char}` | reused |
| ellipsis / wrap | `ui/palette.rs` `ellipsize`, `wrap_cut` | reused; new `wrap_runs` (breaks after `/`) |
| path normalize | `workspace::normalize` | reused |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| AC-7 | pass | completion_tests.rs (17 tests); snapshots at 80x24, 120x40, 40x12 |
| D6 | pass | command names `command_names_common_part_then_list_then_step`; paths `ws_add_folders_common_list_take`, `ws_open_lists_folders_and_workspace_files_only`, `ssh_identity_completes_files_of_the_active_files_root`, `run_completes_file_arguments_not_the_program`; hosts `ssh_destination_completes_config_hosts_and_recent_targets`; tools `ai_completes_tool_names`, `ai_tool_names_from_setting_text` |
| D12 | pass | look per S5 / mockups: palette_tab_prefix_80x24, palette_tab_list_120x40/80x24/40x12, palette_ssh_path_120x40, palette_ssh_host_80x24 snapshots |
| SEC-4 (path base, bounded) | pass | `ssh_base()` canonical active root, no fallback; `listing_is_bounded_and_unsafe_names_are_skipped`; `one_folder_ends_with_a_slash_hidden_only_after_a_dot` |
| palette keys / find box | pass | `existing_palette_keys_still_work`; related run 258 passed; find bar code untouched |
| BASE-11 | pass | no branch or file resets used |
| BASE-14 | pass | first build in background, turn ended; no waiting loops |
| BASE-15 | pass | nothing left running |
| BASE-16 | pass | Reuse table; capabilities.md line added |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | notes from the start, progress lines |
| BASE-24 | pass | nothing staged or committed |
| BASE-25 | pass | guard output above, exit 0 |
| BASE-38 | pass | rustfmt only on the changed .rs files |
| COST-8 | pass | Edit tool / perl -pi; no python heredocs |
| COST-13 | pass | no throwaway scripts; nothing written to TEAM_TOOLS |
| COST-15 | pass | snapshots regenerated once after the code was final; 6 opened |
| BASE-34 / DESIGN-23 | n-a | no network use |

## Quick pass 1
1. fixed: 40x12 tag. ui/palette.rs keeps the typed text plus the cursor readable: `tab: list (N)` when it fits, else `tab (N)` (`Completion::short_tag`), else no tag (the list counter `i/N` always stays). Snapshot palette_tab_prefix_40x12: `> /ws add ../fixecom      tab (2)`. `renders_common_part_and_lists` asserts `> /ws add ../fixecom` plus the tag at every size.
2. fixed: the `~` form. The test home is the temp folder (`app.ssh.home`). `tilde()` now takes the home vr-ide uses for ssh (`app.ssh.home`), not `$HOME`, so tests set no process env. palette_ssh_path_120x40 shows `files in ~/DEV/fixecom (workspace folder of the active file)` on one line, as in the mockup; the test asserts that text.
3. fixed: `/ws add <folder>` (slash.rs usage hint, docs/commands.md table, slash_tests parse error `/ws add needs <folder>`, completion test). No other snapshot carried `<dir>`.
4. fixed: one completed `/ssh` destination keeps a trailing space and gets a new `Phase::Done`: there is no tag and no list, the `runs: ssh …` line shows under the rows, Enter acts as usual and the next edit drops the line. The arrow-highlight shortcut no longer applies to `/ssh`: the slash rows' fills have no space and would skip the line, which is the cause of the QC case. Test `a_completed_destination_gets_a_space_and_shows_what_runs` (`/ssh git` → `/ssh git.example.com ` + `runs: ssh git.example.com`) was red before the fix, green now.
- `ssh_slot_at_end` not switched (still the local stand-in, for the merge).
- Commands:
```
$ cargo test -p vr-ide completion_tests        (before fix)
test completion_tests::a_completed_destination_gets_a_space_and_shows_what_runs ... FAILED
test completion_tests::renders_ssh_path_and_host_lists ... FAILED   (files in ~/DEV/fixecom missing)
$ INSTA_UPDATE=always cargo test -p vr-ide completion_tests      (once, code final)
test result: ok. 18 passed; 0 failed
$ cargo test -p vr-ide -- slash palette ssh completion workspace mode_tests terminal_tests::render_palette
test result: ok. 259 passed; 0 failed; 1 ignored; 0 measured; 479 filtered out
$ cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.90s   (exit 0)
```
- Snapshots opened: palette_tab_prefix_40x12, palette_ssh_path_120x40, palette_ssh_host_80x24 (now 5 hosts: git.example.com was added to the fixture config).
- Rule rows touched: D12 pass (snapshots above), BASE-25 pass (guard above), COST-15 pass (one regeneration), BASE-38 pass (rustfmt on changed files only).

## Merge
Master (T1, T2, T3, T5, T7) merged into run4-T4 by the leader; conflicts resolved keeping both sides.
- commands/palette.rs: `Palette` keeps `completion`, `moved` (T4) and `ssh_listed` (T3); `open` initialises all three.
- commands/slash.rs: T3's `remember_listed` kept (now `pub(crate)`); T3's `slash::tab` dropped (it called the removed `complete`); completion.rs calls `remember_listed` after every input it sets (`completion::set_input`: `ssh::picked` before, `remember_listed` after).
- ui/palette.rs: T3's `/ssh` detail (`palette_detail` / `palette_details`, `ssh_detail_lines`, `fit_row`, warn style, `in <folder>`) kept. While a completion is open: Common / path list / other lists use T4's detail; a `/ssh` destination list uses `ssh::typed_detail(app)` (the input's line; the list holds candidates, not T3's rows). T4's own `ssh_runs_line` / `wrap_runs` removed.
- docs/commands.md: master's `/ai` and `/ssh [ssh args]` rows, `/ws add <folder>`; the Tab bullet says `/ai` names come from `ai_tools`, and the arrow-highlight rule does not apply to /ssh.
- commands/ssh.rs (T3 file, one addition): `pub fn typed_detail(app) -> Option<Detail>` = `line_of(app, None)` + `detail_for`.

Interface switch:
- `completion::ssh_slot_at_end` now maps `crate::ssh::args::classify(line, line.len())` (`Path{start}` → `Path`, `Destination{start}` → `Host`, `Other`). Local `ssh_roles`, option tables and `ssh_runs_line` deleted. One rule kept on top: `Path{start}` at the very end right after the option letter (`-i` + Tab, nothing typed yet) is `Other`: otherwise `-i` + Tab completed `-imain.rs` and broke T3's "Tab puts the listed line in the input" (ssh_tests::sec_4_sec_5_…).
- Base for `/ssh` paths: `ssh::shown_base(app, arg)` canonicalized; missing → nothing completed.
- `ssh_hosts()`: recent `Entry{text, base}` → destination via `ssh::args::Line::parse(text, home).target()`; then config hosts.
- `/ssh` where nothing completes (option being typed, remote command) or a row highlighted with the arrows: Tab puts the highlighted listed line in the input (T3's behaviour, with `remember_listed`).
- Done phase removed: a completed destination now shows T3's own `runs:` detail for the typed line (the window's normal detail); test `a_completed_destination_gets_a_space_and_shows_what_runs` still passes.
- `/ai`: `ai_tool_names` reads `settings.ai_tools[].name` (typed field); `tool_names_from_toml` + its test removed; tests: `ai_completes_tool_names` (names of the default ai_tools, in order), `ai_completes_the_names_of_the_ai_tools_setting` (`cursor-agent` added). `@account` left for T8.
- completion_tests fixture: recent = `Entry::new("-i credentials/id_fixecom root@app.fixecom.vn", None)`; classifier test adds `-i` → Other.

Commands:
```
$ cargo test -p vr-ide completion_tests
test result: ok. 19 passed; 0 failed
$ run-heavy.sh cargo test -p vr-ide -- slash palette ssh completion ai
(first run) 268 passed; 2 failed: ssh_tests::sec_4_sec_5_a_recent_line_s_folder_applies_only_to_a_line_picked_from_the_list (`/ssh -i` + Tab gave `-imain.rs `; fixed as above), search_tests::apply_reports_a_file_failing_halfway ("search did not finish", 20 s deadline; load average 27 at the time; search code untouched)
(final)     test result: ok. 270 passed; 0 failed; 1 ignored; 0 measured; 567 filtered out
$ cargo fmt --all --check && run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 22.40s   (exit 0)
```
- Snapshots: no regeneration needed: all 10 completion snapshots and T3's ssh/slash snapshots passed unchanged (no .snap.new).
- Conflict markers: none left; files not staged (BASE-24), so git still lists palette.rs, slash.rs, ui/palette.rs, docs/commands.md as unmerged until the leader stages them.
- rustfmt only on changed files. Nothing left running.
