# T3 /ssh argument line (D5) — worktree .worktrees/T3 (branch run4-T3)

Tools available: Bash, Read, Edit, Write, Skill, ToolSearch (deferred: TaskStop, Monitor, LSP, ...), Artifact (not used).

## Progress
- read: spec D5/AC-6, threat model SEC-1..5 (+ SEC-S1..S18 headlines), ui-spec S5 + mockups palette-ssh-*, ssh/mod.rs, ssh/recent.rs, commands/ssh.rs, slash.rs (/ssh entry, submit), ui/palette.rs, ssh tests.
- code: new ssh/args.rs (+ args_tests.rs), ssh/mod.rs (argv/probe/child_path/Target ssh://), ssh/recent.rs (entries with base), commands/ssh.rs (Line, base, PATH, detail), slash.rs submit fill rule + /ssh usage, ui/palette.rs detail + middle cut, terminal.rs spawn_program_env.
- tests: ssh:: 40 pass; ssh_tests 45 pass, snapshot pending regen (status row label now `bob@web1`).
- docs: README "SSH sessions", docs/commands.md (/ssh paragraph + table row), docs/keymap.md (ssh.connect description). capabilities.md entry added.
- snapshot ssh_full_screen_60x8 regenerated once (status row `SSH  bob@web1` instead of `bob@web1:2222`: the label is now ssh's destination word, an old `h:port` means `-p port h`).
- final: guard green, related tests green (below). DONE.

## Files changed
- crates/vr-ide/src/ssh/args.rs (new): split (SEC-1), getopt walk + destination (SEC-2), Line (keeps_own_options SEC-3, local_command SEC-5, old h:port migration), classify (for T4), shown_command / wrap_command / cut_middle / quote / canonical_base.
- crates/vr-ide/src/ssh/args_tests.rs (new): 13 unit tests.
- crates/vr-ide/src/ssh/mod.rs: `pub mod args`; Target::parse accepts `ssh://`; argv(line, lc) = prefix + user words (prefix dropped whole for own options), probe_argv = `-G` + user words; child_path; probe(ssh, line, cwd, path, timeout) runs in the session folder with the cleaned PATH; login_args removed.
- crates/vr-ide/src/ssh/recent.rs: Entry{text, base}; file `<base>\t<line>` (empty base = old entry), old bare targets migrated, ignored unless owned by uid and not group/world-writable, list cut to stay <= 64 KiB.
- crates/vr-ide/src/commands/ssh.rs: Session{line, base, target}; connect parses a Line, base_for (recent line -> stored folder, else canonical active root; missing -> BASE_GONE / RECENT_BASE_GONE); start skips the probe for own options (notice); session PATH = child_path(terminal_env PATH or process PATH) for probe and session; cwd = base; rows list lines (sanitized titles); palette_detail, fit_row, shown_base (pub, for T4).
- crates/vr-ide/src/commands/slash.rs (minimal): /ssh usage `[ssh args]` + description, ArgKind::Ssh doc, submit(): a listed ssh row whose line has a local-command word (and is not the typed line) -> Submit::Fill.
- crates/vr-ide/src/ui/palette.rs (minimal, only /ssh): `palette_detail` lines in the detail area (warn = warning style bold), `ssh_detail_lines` (wrap_command with 6-col indent), /ssh rows cut with `fit_row` instead of ellipsize.
- crates/vr-ide/src/commands/terminal.rs: `spawn_program_env` (extra env after terminal_env); `spawn_program` delegates. (Outside the listed scope, needed for SEC-4 child PATH; 1 function added.)
- crates/vr-ide/src/ssh/tests.rs, crates/vr-ide/src/ssh_tests.rs: updated to the new argv/label/cwd/recent format; new tests below. Shim also writes `argv`, `cwd`, `gcwd`.
- crates/vr-ide/src/snapshots/vr_ide__ssh_tests__ssh_full_screen_60x8.snap.
- README.md, docs/commands.md, docs/keymap.md.

## Tests added
- ssh::args::tests: sec_1_split_table, sec_1_refusals_have_fixed_notices_that_do_not_echo, sec_1_the_module_names_no_shell, sec_2_destination_and_label, sec_2_refused_destinations, sec_2_old_targets_with_a_port_mean_what_they_meant, target_parse_accepts_the_ssh_uri_form, sec_3_words_that_keep_vr_ide_s_prefix_out, local_command_words_are_named, the_shown_command_resolves_relative_path_values_only, the_command_wraps_at_spaces_and_after_slashes, a_cut_row_keeps_the_first_option_and_the_destination, classify_finds_path_values_and_the_destination.
- ssh::tests: sec_3_argv_keeps_the_user_s_words_and_drops_the_prefix_for_their_own_options, sec_4_the_child_path_has_no_empty_or_relative_entries, sec_5_a_file_others_can_write_is_ignored, sec_s10_sec_5_* (rewritten: base stored, migration, 64 KiB cut).
- ssh_tests (app + fake ssh shim): ac_6_sec_4_relative_paths_resolve_in_the_workspace_folder_of_the_active_file (two roots, active file in B -> cwd B, recent has B), ac_6_sec_1_quotes_backslashes_ports_and_jump_hosts_reach_ssh_as_typed, ac_6_sec_3_full_screen_status_row_and_probe_with_the_user_s_words (-G sees -F/-o, probe cwd, status row, recent, open session reused), sec_3_the_user_s_own_local_command_or_sharing_words_keep_the_prefix_out (no probe, argv = user words), sec_4_the_session_path_has_no_empty_or_relative_entries, sec_4_a_missing_workspace_folder_refuses_the_start, sec_5_a_recent_line_runs_in_the_folder_it_ran_in (root A replay, missing folder refused), sec_5_a_recent_line_with_a_local_command_only_fills_the_input (log empty until Enter), ui_s5_the_window_shows_what_runs_and_cuts_long_rows_in_the_middle; ac_s1_sec_s9_a_refused_target_runs_nothing_and_is_not_echoed extended (each SEC-1/2 refusal + fixed notice).
- Existing SEC-S4 "hanging -G killed at the timeout -> plain tab" tests (ssh::tests + ssh_tests) now run the new probe signature.

## For T4 (Tab completion) — interface
- `crate::ssh::args::classify(line: &str, cursor: usize) -> ArgPos` — `line` is the text after `/ssh `, `cursor` a byte offset in it (clamped). Pure, reads nothing; an unclosed quote / trailing `\` is allowed (line being typed).
  - `ArgPos::Path { start }`: value of `-i`/`-F`/`-E`/`-S` (separate or attached, bundles like `-vi`), or of `-o <Key>=` / `-o "<Key> "` where Key ends in `file` or is `controlpath`/`identityagent`; `start` = byte offset in `line` where the path text begins (offsets come from the unquoted word, so a quote before the value is part of what was typed).
  - `ArgPos::Destination { start }`: the destination position (first non-option word, also after `--`); `start` = word start.
  - `ArgPos::Other`: option letters, non-path values, remote command, unknown options.
- Base folder for relative paths: `crate::commands::ssh::shown_base(app, line_text) -> PathBuf` (no disk access).
- `runs:` line: `crate::commands::ssh::palette_detail(app) -> Option<Detail{text, warn}>` (already drawn by ui/palette.rs); pure parts `args::shown_command(words, base)`, `args::wrap_command(text, width, indent)`.

## Commands run + result
- `cargo fmt --all --check && $TEAM_BIN/run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings`
  ```
      Checking vr-ide v0.1.0 (.../.worktrees/T3/crates/vr-ide)
      Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.15s
  guard exit=0   (fmt printed no diff)
  ```
- `$TEAM_BIN/run-heavy.sh cargo test -p vr-ide --lib -- ssh slash palette terminal`
  ```
  test result: ok. 274 passed; 0 failed; 1 ignored; 0 measured; 471 filtered out
  ```
- `cargo test -p vr-ide --lib ssh` → `test result: ok. 86 passed; 0 failed`
- first ssh_tests run before fixing tests that assumed cwd=home: 5 failed (cwd moved to the project per SEC-4, `a\tb` cannot be typed into the window, snapshot) → fixed, all green.

## Deviations
- Red-first: the unit tests and the new app tests were written together with the code (the old API had no Line type to fail against); BASE-11 forbids stashing to run them on the base. Red evidence is the 5 failures of existing tests whose expectations the change moves (cwd, label), not of the new tests.
- Label is the destination word only (SEC-2 example `h -p 2222` → `h`); old `bob@web1:2222` now labels `bob@web1` (snapshot changed).
- A typed line equal to a recent line runs in that recent line's folder (that is how "fill, then Enter" replays in the stored folder); the `runs:` line shows that folder, so it is visible.
- `ssh-recent` keeps the whole line incl. a remote command (spec: "keeps the argument line"); file stays 0600; README says so.
- An old-format file line without a tab is accepted only when it is a bare target (FR-25 format).

## Out-of-scope noticed
- none.

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| ssh binary lookup | ssh::find_in_path | reused |
| destination validation | Target::parse | reused + `ssh://` |
| display sanitizing of stored lines | ui::safe_text::sanitize | reused (rows, runs line) |
| private atomic file | recovery::create_private_dir/_file, ssh::recent::record | reused |
| bounded probe | ssh::probe/read_until | reused, argv/cwd/PATH extended |
| word wrap for hint | ui::dialog::wrap | reused |
| spawn with env | terminal::spawn_program | extended (`spawn_program_env`) |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| AC-6 | pass | ssh_tests::ac_6_sec_4_relative_paths_resolve_in_the_workspace_folder_of_the_active_file (argv `[-o][ControlMaster=no][-i][credentials/id_fixecom][root@app.fixecom.vn]`, cwd = root B, recent keeps line+folder), ac_6_sec_1_quotes_backslashes_ports_and_jump_hosts_reach_ssh_as_typed, ac_6_sec_3_full_screen_status_row_and_probe_with_the_user_s_words (status row `SSH  user@host`, full screen), FR-25 tests ac_s1..ac_s6 green (plain `/ssh web1`, ~/.ssh/config hosts, recent) |
| SEC-1 | pass | args_tests::sec_1_split_table / sec_1_refusals_have_fixed_notices_that_do_not_echo / sec_1_the_module_names_no_shell; ssh::tests::sec_s2_the_ssh_code_names_no_shell... now covers ssh/args.rs; sec_s9 log scan covers ssh/args.rs (no tracing of the line); argv appends nothing after user words (sec_3_argv_keeps_the_user_s_words...) |
| SEC-2 | pass | args_tests::sec_2_destination_and_label (`-p bank evil`→evil, `-vp 22 h`, `h -p 2222`, `-oPort=1 h ls -la`, `-J jump host`, `ssh://u@h:2`), sec_2_refused_destinations (`-oProxyCommand=x`, `-- -oProxyCommand=x`, `h‮x`), ssh_tests refused list: shim log empty |
| SEC-3 | pass | ssh::tests::sec_3_argv_keeps_the_user_s_words_and_drops_the_prefix_for_their_own_options; args_tests::sec_3_words_that_keep_vr_ide_s_prefix_out; ssh_tests::sec_3_the_user_s_own_local_command_or_sharing_words_keep_the_prefix_out (no -G run); ac_6_sec_3 (-G sees -F/-o); hanging -G killed at timeout: sec_s4_a_hanging_probe_is_killed_and_the_tab_opens_plain + ssh::tests::sec_s4_a_hanging_probe_is_killed_with_its_group_at_the_timeout |
| SEC-4 | pass | ac_6_sec_4 (two roots → cwd B), sec_4_the_session_path_has_no_empty_or_relative_entries (PATH `.::rel:/shim:...` → `/shim:/usr/bin:/bin`), ssh::tests::sec_4_the_child_path..., sec_s2_ssh_is_found_on_absolute_path_entries_only, sec_4_a_missing_workspace_folder_refuses_the_start; completion listing bounds are T4's |
| SEC-5 | pass | ssh::tests::sec_s10_sec_5_* (0600/0700 with umask 022, base stored, `bob@ok2:2222`→`-p 2222 bob@ok2`, invalid lines dropped, 64 KiB), sec_5_a_file_others_can_write_is_ignored (0666/0620/0602 ignored), ssh_tests::sec_5_a_recent_line_runs_in_the_folder_it_ran_in (root A replay; missing → RECENT_BASE_GONE), sec_5_a_recent_line_with_a_local_command_only_fills_the_input (log empty until Enter), ui_s5_... (middle cut keeps `-o ProxyCommand=` and `… deploy@db1`) |
| SEC-S1..S18 (kept) | pass | all prior ssh::tests and ssh_tests (46) green with the new argv |
| UI S5 (/ssh parts) | pass | ui_s5_the_window_shows_what_runs_and_cuts_long_rows_in_the_middle; local-command hint (warning style, bold) in sec_5_...fills_the_input; split error row shown via the existing slash error row (ac_s1_sec_s9 test) |
| BASE-11 | pass | no checkout/restore/stash/reset used |
| BASE-14 | pass | first build in background, no wait loops |
| BASE-15 | pass | only my own background build task (finished) |
| BASE-16 | pass | ## Reuse table; capabilities.md updated |
| BASE-17 | pass | nothing installed |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | this notes file, progress lines |
| BASE-24 | pass | nothing staged or committed |
| BASE-25 | pass | guard output above |
| BASE-38 | pass | rustfmt only on the changed .rs files |
| COST (related tests only, COST-8/10/13/15) | pass | filters `ssh slash palette terminal`; snapshot regenerated once after code was final; no throwaway scripts needed |
| COST-9 | pass | nothing left running |
| BASE-10, BASE-20..23, BASE-34, BASE-37, DESIGN-23, other BASE/COST | n-a | no removals of used code (login_args only used by argv), no network, no browser, no new secrets |

## Fix round 1
Progress:
- read: notes, commands/ssh.rs, slash.rs submit/enter/tab, ui/palette.rs, ssh/args.rs, ssh_tests (sec_5, ui_s5, flaky two).
- red tests written: args_tests (local_command -vI/-F, shown none), ssh_tests 4 new
- code: args local_command_word (walk -I, -F), shown none; palette.ssh_listed; ssh.rs line_of/picked/active_base/palette_details/base_for; slash tab/enter/prepare; ui ssh_detail_lines cut + max height; flaky: shim stty order, FAKE2 split
- green: new tests + flaky two pass; running related set + guard
- final: guard green, related set green (below). DONE.

### Findings
| # | Finding | Status | Change | Test (red -> green) |
|---|---|---|---|---|
| 1 | [major AC-6/SEC-4] typed line equal to a recent line ran in the recent folder | fixed | commands/ssh.rs `line_of`/`highlighted`/`picked`: a session/recent line brings its folder only when highlighted over a different typed text, or put in the input from the list (Tab, the local-command fill) and not edited since. `Palette.ssh_listed` (commands/palette.rs, 1 field) holds (input, folder); `slash::prepare` drops it at the first edit (edit-back stays typed). `slash::enter` hands the picked folder to `connect` through `SshState.picked` (taken once, matched to the exact line); a refused start reopens the window with it kept. Detail adds `in <folder>` (also under the local-command hint) when the folder differs from the active root. | ssh_tests::sec_4_sec_5_a_recent_line_s_folder_applies_only_to_a_line_picked_from_the_list (red: `in <A>` missing; green: picked -> cwd A, Tab fill -> A, Backspace/retype -> no `in`, typed by hand -> cwd B) |
| 2 | [major COMPACT] runs: line cut silently, destination lost | fixed | ui/palette.rs `ssh_detail_lines(d, width, rows)` + `cut_keeping`/`keep_end`: first rows, then the line holding the destination marked `…` (end mark when later lines drop); one row: `start…destination`; `in <folder>` keeps its end. `Detail.dest` = destination as shown. | ssh_tests::ui_s5_a_long_command_keeps_its_destination_and_the_box_keeps_its_height (red: box 9 vs 7 rows; green) — screen below |
| 3 | [low SEC-5] bundled `-vI` not seen | fixed | ssh/args.rs `local_command_word`: `-I` from the getopt walk opts (any bundle/attached); unparseable lines read each `-…` word's letters | args_tests::local_command_words_are_named (`-vI /x.so h`, `-qI/x.so h`, `h -vI /x.so`) red -> green; ssh_tests::sec_5_recent_lines_with_a_bundled_i_or_a_config_file_only_fill_the_input |
| 4 | [leader SEC-5] recent `-F <file>` only fills | fixed | same fn: `-F` with value not `none` (case-insens.) -> `-F`; hint `Uses its own ssh config file (-F):` / `check it; enter runs it` (`args::CONFIG_FILE` const keeps the SEC-S2 source scan for `"-F"` clean) | args_tests (`-F cfg h`, `-vFcfg h` -> -F; `-F none h`, `-Fnone h` -> None); ssh_tests sec_5_recent_lines_... (`-F none h3` runs at once, argv ends `[-F][none][h3]`) |
| 5 | [minor] shown_base vs base_for disagree | fixed | one rule `active_base`: root of the active file, else the first folder, no disk read; base_for canonicalizes it and refuses when missing (never another folder) | ssh_tests::sec_4_the_shown_folder_and_the_folder_ssh_runs_in_follow_one_rule (red: ran in root B, no BASE_GONE; green) |
| 6 | [minor D323] box height jumps between rows | fixed | `commands::ssh::palette_details(app)` (detail of every row + typed); ui reserves max lines (<= MAX_DETAIL_ROWS) | ui_s5_a_long_... (height equal before/after Down) |
| 7 | [minor] `-F none` shown as `/ws/none` | fixed | args::shown_command join skips `none` | args_tests::the_shown_command_resolves_relative_path_values_only (`-F none -i none -o UserKnownHostsFile=none h`) red -> green |
| 8 | flaky ac_s2_ac_s3 / sec_s12 | fixed (test-only) | Root causes are races in the test, not timing waits: (a) the shim printed `password: ` before `stty -echo`, so under load the typed password could echo and fail the "s3cret-pw shows nowhere" check — shim now turns echo off first, as ssh does; (b) sec_s12 waited for `FAKE2` anywhere, which the echoed command line itself contains, then asserted row 22 — the command now splits the word with quotes (`FA''KE2`) and the wait is on row 22. No product race. | both pass 5/5 repeated runs (0.5–7.6 s each) and in the related set |

Old test updated: sec_5_a_recent_line_runs_in_the_folder_it_ran_in now picks the gone-folder line from the list (Down, Enter) instead of typing it, since a typed line now runs in the active root by design.

### New states (round 1)
- `Palette.ssh_listed` (input text + folder): dropped with the window (Esc/close), at the first edit (prepare on each relist), replaced by the next fill. A refused start (folder gone) reopens with it kept, so a second Enter refuses again instead of moving to the active root. Concurrent writer / hung call: n-a (pure state, no I/O).
- `SshState.picked`: set right before the `/ssh` handler runs, taken by `connect`, cleared after the handler returns either way; used only when it names exactly the line being run.

### Deviations
- A line with `-F <file>` shows the config-file hint instead of the `runs:` line (same as local-command lines); the COMPACT test therefore uses the reviewer's line with `-o IdentityFile=…` in place of `-F`.
- `ssh_listed` is cleared on edit, not compared by text: typing the line back to the same text still counts as typed.

### Screen (80x24, long root, second row highlighted)
```
    │ web1  recent                                                         │
    │ -i credentials/id_fixecom -o IdentityFi… root@app.fixecom.vn  recent │
    │ ──────────────────────────────────────────────────────────────────── │
    │ runs: ssh -i /tmp/.tmp4f28pv/a-folder-with-a-long-name/              │
    │       and-another-long-one/fixecom/credentials/id_fixecom -o         │
    │      …root@app.fixecom.vn                                            │
    └──────────────────────────────────────────────────────────────────────┘
```

### Commands
- red: `cargo test -p vr-ide --lib -- ssh::args sec_4_sec_5_a_recent sec_4_the_shown sec_5_recent_lines ui_s5_a_long` -> `11 passed; 6 failed` (local_command_words_are_named at `-vI`, shown none, the 4 app tests: `in <A>` missing, BASE_GONE None, hint missing, height 9 vs 7)
- green: same + flaky two -> all ok
- guard: `cargo fmt --all --check && run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings` -> `Finished dev profile ... guard_exit=0`
- related: `run-heavy.sh cargo test -p vr-ide --lib -- ssh slash palette` -> `test result: ok. 190 passed; 0 failed; 1 ignored; 559 filtered out; finished in 91.14s` (one intermediate failure fixed: ssh::tests::sec_s2 source scan found a `"-F"` literal in args.rs -> `CONFIG_FILE` const)
- snapshots: none touched this round (no .snap.new).
- rustfmt only on changed files (ssh/args.rs, args_tests.rs, commands/ssh.rs, slash.rs, palette.rs, ui/palette.rs, ssh_tests.rs).

### Files changed (round 1)
crates/vr-ide/src/ssh/args.rs, ssh/args_tests.rs, commands/ssh.rs, commands/slash.rs (tab/enter/prepare + remember_listed), commands/palette.rs (field `ssh_listed`), ui/palette.rs (/ssh detail only), ssh_tests.rs, README.md (SSH paragraph).

### For T4 (interface changes)
- `shown_base(app, text)` unchanged signature; now: the picked line's folder when `text` is the highlighted picked line, else the active file's root / first folder.
- New: `commands::ssh::picked(app)`, `palette_details(app)`; `Detail` has a `dest` field; `ssh_detail_lines` takes `rows`.
- slash::tab calls `remember_listed` after `set_input`: keep it when T4 changes Tab for `/ssh`.

### Rule compliance (rows touched)
| Rule | Status | Evidence |
|---|---|---|
| AC-6 | pass | sec_4_sec_5_a_recent_line_s_folder_applies_only_to_a_line_picked_from_the_list (typed -> active file's root B) |
| SEC-4 | pass | same test + sec_4_the_shown_folder_and_the_folder_ssh_runs_in_follow_one_rule (shown = run folder, missing -> BASE_GONE, log empty) |
| SEC-5 | pass | sec_5_recent_lines_with_a_bundled_i_or_a_config_file_only_fill_the_input, args_tests::local_command_words_are_named, sec_5_a_recent_line_runs_in_the_folder_it_ran_in, sec_5_a_recent_line_with_a_local_command_only_fills_the_input |
| SEC-1..3, SEC-S1..S18 | pass | related set 190 passed incl. ssh::tests::sec_s2 source scan, all ssh_tests |
| COMPACT / UI S5 / D323 | pass | ui_s5_a_long_command_keeps_its_destination_and_the_box_keeps_its_height, ui_s5_the_window_shows_what_runs_... |
| BASE-11 | pass | no checkout/restore/stash/reset |
| BASE-14 | pass | related run moved to background by the tool, waited for its notification; no loops |
| BASE-15 / COST-9 | pass | only that task, finished; nothing left running |
| BASE-24 | pass | nothing staged or committed |
| BASE-25 | pass | guard output above |
| BASE-38 | pass | rustfmt on the changed files only |
| COST-8/10/13/15 | pass | related filter `ssh slash palette`; no snapshot regen needed; scratch files only in session scratchpad |

## Merge + quick pass 1
- merge: resolved crates/vr-ide/src/commands/terminal.rs (master's spawn_sized kept; spawn_program and spawn_program_env both call it with spawn_size) and docs/commands.md (master's /ai row + T3's /ssh row).
- quick pass (a): destination line found by position, not substring. `args::shown_command_at` returns the destination's byte offset; `Detail.dest_at` (replaces `dest`) = its position counted in non-space characters of the shown `runs:` text (wrapping only moves spaces); ui/palette.rs `cut_keeping` picks the wrapped line where that count falls.
- quick pass (b): at 1 detail row, the `in <folder>` line is kept: `runs: …dest  in <folder>` (folder takes at most half the row, cut keeping its end; the command is rewrapped to the rest and cut keeping its destination).
- red: `cargo test -p vr-ide --lib -- ui_s5_a_cut ui_s5_one_detail` -> 2 failed (rows kept `…deploy@db1` = the remote command's copy and lost `deploy@db1 printf`; 1 row showed only `runs: … web1`, no folder).
- green rows:
  - (a) `runs: ssh -o UserKnownHostsFile=/tmp/.tmpjZoXTZ/project/config/` / `known_hosts-of-the-team -o IdentityFile=/tmp/.tmpjZoXTZ/` / `project/config/ids/a-key-with-a-long-name deploy@db1 printf…`
  - (b) `runs: ssh -i /tmp/.tmp…credentials/k web1  in /tmp/.tmpMvulCH/roota`
- tests: ssh_tests::ui_s5_a_cut_command_keeps_the_real_destination_not_the_same_text_later_on, ssh_tests::ui_s5_one_detail_row_still_names_the_other_folder (helper `detail_rows` reads between the box borders).
- stray file `crates/vr-ide/src/ord| word == dest)))|X|`: created by me in fix round 1 — a failed `sed -i 's|…|X|'` whose `|w…` part sed read as its write-to-file flag. Not staged; safe to delete.
- guard: `cargo fmt --all --check && run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings` -> `Finished dev profile`, guard_exit=0; `cargo build` -> Finished.
- related (both sides): `run-heavy.sh cargo test -p vr-ide -- ssh slash palette ai session terminal` -> `352 passed; 1 failed; 1 ignored`. The failure: `not mine: crates/vr-ide/src/search_tests.rs — search_tests::apply_reports_a_file_failing_halfway: "search did not finish"` (the filter `session` matched it; `wait_search` has a 20 s deadline and the run was under load). Rerun alone -> `1 passed` (6.68 s). No search code changed.
- Merge files are free of conflict markers but not staged (BASE-24): git still lists terminal.rs and docs/commands.md as unmerged until the leader stages them.
- rustfmt only on changed files: ui/palette.rs, commands/ssh.rs, ssh/args.rs, ssh_tests.rs. Nothing left running.
