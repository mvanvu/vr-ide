# FR-25 SSH sessions from the command window, full screen after login (D317)

Skills: ai-dev-team:implementation, ai-dev-team:security-review-checklist
Worktree: .worktrees/FR-25 (branch fr-25 from master 5797347). Nothing staged or committed.
Status: DONE (all steps below finished; nothing open).

## Progress
- read spec AC-S1..S6 and threat model SEC-S1..S18
- vr-term: marker scanner (src/marker.rs, red 6 failed -> green 7), Terminal::spawn_with + SpawnExtras + marker_seen, tests/marker.rs 5 pass
- vr-ide ssh module (ssh/mod.rs target+argv+nonce+probe, ssh/config.rs, ssh/recent.rs): tests red 18 failed -> green (ssh::tests)
- vr-ide wiring skeleton (commands/ssh.rs stubs, terminal.rs hooks, slash /ssh, ui/ssh_view.rs, setting ssh_fullscreen); ssh_tests.rs red: 24 failed / 1 passed
- commands/ssh.rs real implementation: ssh_tests green
- docs (README "SSH sessions", docs/commands.md, docs/keymap.md), setting test
- pty walk qa-artifacts/FR-25/walk.py on the real binary: 38/38 PASS
- robustness: probe exit wait bounded (ssh/mod.rs EXIT_POLL), stuck probe given up after a grace period (commands/ssh.rs PROBE_GRACE), 2 tests
- snapshot ssh_full_screen_60x8 written once, after the code was final
- guard (fmt --check, clippy -D warnings) clean; related tests green; walk rerun on the final build 38/38

## Files changed
- crates/vr-term/src/marker.rs (new): `MarkerScan` byte matcher (holds at most len-1 bytes), 7 unit tests
- crates/vr-term/src/terminal.rs: `Terminal::spawn_with(opts, SpawnExtras, notify)`, `marker_seen()`; the read loop passes the bytes through the scanner before `parser.advance`; the event is raised only by the first marker while the child runs
- crates/vr-term/src/types.rs, lib.rs: `SpawnExtras{marker, env_remove}`
- crates/vr-term/tests/marker.rs (new): 5 PTY tests
- crates/vr-ide/src/ssh/mod.rs (new): `Target::parse` (SEC-S1), `Nonce` (SEC-S3/S7), `argv`, `probe_argv`, `find_in_path`, `probe` (SEC-S4)
- crates/vr-ide/src/ssh/config.rs (new): bounded `hosts(home)` (SEC-S11)
- crates/vr-ide/src/ssh/recent.rs (new): `load`, `record` (SEC-S10)
- crates/vr-ide/src/ssh/tests.rs (new): 23 unit tests
- crates/vr-ide/src/commands/ssh.rs (new): command `ssh.connect`, `Session`, `SshState`, `rows`, `connect`, `reconnect`, `poll`
- crates/vr-ide/src/commands/terminal.rs: `TermTab.ssh`; `title()` (ssh label from the target; OSC titles of every tab through `safe_text::sanitize`); `TerminalPanel.full`; `full_screen / sync_full / enter_full / select_tab / reveal_panel`; `push_tab / replace_term / spawn_program` (extracted from the shell spawn); `refresh` calls `ssh::poll`; Enter on an exited ssh tab reconnects; mouse handling without a tab-bar row in full screen; `shell_in_foreground` is false for ssh tabs
- crates/vr-ide/src/commands/{slash.rs, palette.rs, registry.rs, mod.rs}: `/ssh [target]` (`ArgKind::Ssh`), `ssh ` typed in the window becomes `/ssh `, `Palette.ssh_hosts` (config read once per window)
- crates/vr-ide/src/ui/ssh_view.rs (new), ui/mod.rs: full-screen layout and status row; the grid is drawn by `terminal_view::render_grid`
- crates/vr-ide/src/ui/terminal_view.rs: one hunk, `fn render_grid` -> `pub(crate) fn render_grid`
- crates/vr-ide/src/app.rs (`App.ssh`), lib.rs, recovery.rs (`create_private_dir/file` now pub(crate)), Cargo.toml (rustix feature `rand`, no new crate)
- crates/vr-ide/src/ssh_tests.rs (new, 26 tests), slash_tests.rs (registry list), snapshots/vr_ide__ssh_tests__ssh_full_screen_60x8.snap
- crates/vr-config: `ssh_fullscreen` (src/settings.rs, defaults/settings.toml with its doc comment, tests/config.rs)
- README.md, docs/commands.md, docs/keymap.md

## Tests added
vr-term `marker::tests` (7) and `tests/marker.rs` (5); vr-ide `ssh::tests` (23) and `ssh_tests` (26); vr-config `ssh_fullscreen_defaults_to_on_and_is_settable`.
Seen red first: marker 6 failed against the stub; ssh::tests 18 failed against stubs; ssh_tests 24 failed against the behaviour-less skeleton.

## Commands run + result (final code)
```
$ run-heavy.sh cargo fmt --all --check            -> no output, rc 0
$ run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings   -> no warning, no error
$ run-heavy.sh cargo test -p vr-ide --lib -- terminal slash ssh ui:: palette config_reload dialog app_tests recovery
test result: ok. 356 passed; 0 failed; 3 ignored; 0 measured; 292 filtered out; finished in 0.99s
$ cargo test -p vr-ide --lib ssh
test result: ok. 49 passed; 0 failed; 0 ignored; 0 measured; 602 filtered out
$ run-heavy.sh cargo test -p vr-term
lib 14 passed · env 1 · flood 1 (1 ignored) · leak 1 · marker 5 · pty 21 — 0 failed
$ run-heavy.sh cargo test -p vr-config
lib 24 passed · tests/config.rs 17 passed · others 8/14/1/1 — 0 failed
$ python3 qa-artifacts/FR-25/walk.py target/debug/vr-ide <scratch>     (full output: qa-artifacts/FR-25/walk_result.txt)
INFO a18 state folder mode: 775 (made by vr-ide's other state files before the first session)
RESULT 38/38 passed
```
`rule-checks.sh related-tests FR-25` listed crates/vr-config/tests/config.rs and crates/vr-term/tests/*.rs (run above) plus vr-git, vr-syntax and welcome tests that only share file names with changed files (`common/mod.rs`, `mod.rs`); the welcome tests ran inside the `ui::` filter, vr-git / vr-syntax code is untouched and was not run.

## AC coverage
| AC | Evidence |
|---|---|
| AC-S1 | `ssh_tests::ac_s1_the_list_shows_open_sessions_recent_targets_and_config_hosts`, `ac_s1_sec_s9_a_refused_target_runs_nothing_and_is_not_echoed`, `ssh::tests::sec_s1_*`, `sec_s10_*`, `sec_s11_*`; walk a01-a04 |
| AC-S2 | `ssh_tests::ac_s2_ac_s3_a_session_starts_as_a_tab_and_goes_full_screen_after_login` (tab label, prompt answered in the tab, exact argv); walk a05-a08 |
| AC-S3 | same test (full screen, status row, pty 80x23), `ac_s3_no_full_screen_when_the_user_has_a_local_command`, `sec_s8_*`, snapshot; walk a09, c01, c06, d01 |
| AC-S4 | `ac_s4_ctrl_backtick_leaves_and_choosing_the_session_returns`, `ac_s4_resize_follows_the_window_and_f1_opens_over_the_session`, `sec_s14_*`; walk a10-a16 |
| AC-S5 | `ac_s5_exit_ends_full_screen_and_enter_reconnects_with_a_new_nonce`, `sec_s18_sec_s10_*`; walk a17, a19, a20, b01 |
| AC-S6 | `ac_s6_sec_s16_sixteen_sessions_at_most`, `ac_s6_sec_s17_quit_asks_about_a_live_session_and_ends_it`; walk a22, a23 |
| AC-T1 note | `ui/ssh_view.rs` has no colour of its own for the session: it calls `terminal_view::render_grid`. After FR-26 is merged in, check where FR-26 put its whole-area fill: if it is in `terminal_view::render` rather than in `render_grid`, `ssh_view::render` must call the same helper |

## SEC coverage
| SEC | Status | Evidence |
|---|---|---|
| S1 | pass | `ssh::tests::sec_s1_refused_targets` (the threat model's table and more), `sec_s1_accepted_targets_and_their_text`; `ssh_tests::ac_s1_sec_s9_*` (shim log empty); walk a04 |
| S2 | pass | `ssh::tests::sec_s2_argv_for_each_target_shape`, `sec_s2_ssh_is_found_on_absolute_path_entries_only`, `sec_s2_the_ssh_code_names_no_shell_and_no_weakening_option` (the grep as a test); walk a07, a08 (PATH starts with `.`, a trap `ssh` in the cwd is not run) |
| S3 | pass | `sec_s3_ten_thousand_local_commands_are_fixed_apart_from_a_unique_hex_nonce`, `sec_s3_shells_print_exactly_the_marker` (sh, bash, dash; zsh and fish are not installed), `sec_s3_ssh_echoes_the_local_command_unchanged` (real `ssh -G`, config /dev/null) |
| S4 | pass | `sec_s4_real_ssh_g_output_is_parsed` (real ssh, temp config, no network), `sec_s4_the_probe_gets_no_stdin_no_terminal_and_only_the_probe_argv`, `sec_s4_a_hanging_probe_is_killed_with_its_group_at_the_timeout`, `sec_s4_probe_output_over_the_cap_is_failed`, `sec_s4_a_probe_that_closes_its_output_but_stays_is_killed_at_the_timeout`; `ssh_tests::ac_s3_no_full_screen_when_the_user_has_a_local_command`, `sec_s4_a_hanging_probe_is_killed_and_the_tab_opens_plain`, `sec_s4_fifty_listed_hosts_run_no_probe`; walk a03, a06, d02. The 5 s and 256 KiB values are asserted as constants; the kill itself is exercised at 300-400 ms |
| S5 | pass (argv); sshd part not-run | `-o ControlMaster=no` in every argv test; real `ssh -G` prints `controlmaster false`. The localhost-sshd check is qc's and no sshd is installed on this machine |
| S6 | pass | vr-term `marker::tests::*` (split at every offset, several markers, wrong and 31-char nonce, held bytes released in order), `tests/marker.rs::the_marker_raises_the_event_and_never_reaches_the_screen_or_the_history`; walk c03, c06 |
| S7 | pass | `ssh::tests::sec_s7_the_nonce_does_not_show_in_debug_output`, `ssh_tests::ac_s2_ac_s3_*` (frame, notices, recent file), `sec_s15_*` (env); walk a24 (every byte vr-ide drew and every file it wrote). vr-ide installs no log sink, so "tracing at trace level" has no output to grep; `sec_s9_terminal_and_ssh_code_log_no_keys_bytes_or_nonce` is the static check in its place |
| S8 | pass | `ssh_tests::sec_s8_a_marker_changes_only_the_tab_whose_own_stream_carried_it`, `sec_s8_a_forged_marker_from_the_remote_side_is_no_login`; vr-term `tests/marker.rs::a_marker_printed_after_the_child_exited_raises_nothing` |
| S9 | pass | `ac_s1_sec_s9_*` (fixed notice, no echo, no history), `ssh::tests::sec_s9_*` (no log call in vr-term, the panel or the ssh code except one I/O error of the recent file); walk a24 (password absent) |
| S10 | pass, one open point | `ssh::tests::sec_s10_*` (0600 / 0700 also under umask 022, atomic replace, symlink replaced and its target untouched, 64 KiB, re-validation), `ssh_tests::sec_s10_a_tampered_recent_file_lists_only_valid_targets`, `sec_s18_sec_s10_*`; walk a18, b02. Open point: a state folder that already exists keeps its mode (walk: 775, created earlier by the recent-workspaces code); only a folder this code creates is 0700. The file is always 0600 |
| S11 | pass | `ssh::tests::sec_s11_*` (50 MiB file, /dev/zero, FIFO, self-Include, loop, `/*`, depth / file / glob / alias caps, under 200 ms) |
| S12 | pass | `ssh_tests::sec_s12_labels_come_from_the_target_and_the_status_row_cannot_be_drawn_over`, `sec_s12_osc_titles_of_shell_tabs_are_sanitized`; walk c02, c04, c05 |
| S13 | pass | `ssh_tests::sec_s13_a_session_cannot_write_the_clipboard_and_query_floods_stay_bounded` (OSC 52, OSC 8 click, 8000 `ESC[6n`); walk c03. ssh tabs use the same emulator setup as shells (`Osc52::Disabled`); the flood is 8000 queries, not 10 MiB (the existing vr-term flood test covers volume) |
| S14 | pass | `ssh_tests::sec_s14_reserved_keys_never_reach_a_session_that_set_keyboard_modes` (only the bracketed paste reached the shim); walk c07 |
| S15 | pass | `ssh_tests::sec_s15_*`, vr-term `tests/marker.rs::withheld_variables_do_not_reach_the_child`; walk a21 (vr-ide itself ran with VR_IDE_* and KITTY_WINDOW_ID set) |
| S16 | pass | `ac_s6_sec_s16_sixteen_sessions_at_most`, `sec_s4_a_hanging_probe_*` (one probe at a time), `sec_s16_a_stuck_probe_is_given_up_after_its_grace_period` |
| S17 | pass | `ac_s6_sec_s17_*`, `sec_s17_closing_a_live_session_asks_and_a_typed_line_never_goes_to_it`; walk a22, a23, c08 |
| S18 | pass | `sec_s18_sec_s10_*`, `ac_s5_*` (new nonce; the old nonce in the new stream is no login); walk a19, b01, b02 |

## Deviations from the plan / decisions for the leader
- No new key (USER-5): ctrl+` returns to the editor. No `D? (FR-25)` entry needed.
- `ssh ` typed in the command window turns into `/ssh ` (one list implementation, the slash command's); the palette command `ssh.connect` "SSH: Connect to Host" opens the same list.
- ctrl+` showing the panel never goes full screen by itself. Full screen comes from the login event (when that tab has the keys), a click on the session's tab, or the ssh list. A login that completes while the user is elsewhere gives a notice instead of taking the keys.
- SEC-S10 open point: an existing state folder is not chmod-ed (other features own it). Leader decides whether it should be tightened to 0700.
- Notices are not drawn while a session is full screen (only the grid and the status row are); they show after leaving if still alive.
- With `ssh_fullscreen = false` no `ssh -G` runs at all.
- An existing ControlPath socket gives a notice; the marker options are still passed, so a stale socket does not cost the full screen.
- `.ai-dev-team/capabilities.md` is outside $TEAM_WS in the main tree, so it was not edited. Lines to add:
  - vr-term: `Terminal::spawn_with(opts, SpawnExtras{marker, env_remove}, notify)`, `marker_seen()`; `marker::MarkerScan`
  - `crates/vr-ide/src/ssh/` — `Target::parse` (validated `[user@]host[:port]`), `Nonce`, `argv`, `probe`, `find_in_path(path, name)`, `config::hosts(home)`, `recent::{load, record}`
  - `commands::terminal::{spawn_program, push_tab, replace_term, select_tab, enter_full, full_screen, sync_full}`; `commands::ssh::{connect, rows, poll, SshState}`; `ui::ssh_view`; fake-ssh shim and `Fx` fixture in `ssh_tests.rs`; pty walk `qa-artifacts/FR-25/walk.py`

## Out-of-scope issues noticed
- `recent.toml` in the state folder is written 664 in a 775 folder.

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| find a program on PATH safely | capabilities.md, `crates/vr-git/src/cmd.rs` `find_git` | same rule in `ssh::find_in_path`; `find_git` is private to vr-git and fixed to `git`, exporting it would change that crate's API |
| private folder / file | `crates/vr-ide/src/recovery.rs` | reused `create_private_dir`, `create_private_file` (made pub(crate)) |
| display filter | `crates/vr-ide/src/ui/safe_text.rs` | reused `sanitize` for OSC titles |
| PTY spawn and env | `commands/terminal.rs` spawn / `child_env`, vr-term `OUTER_TERMINAL_ENV` | extracted `spawn_program`, used by shells and ssh |
| quit / close confirm | T26 `quit_allowed`, `close_tab_asking` | reused unchanged |
| list in the command window | `commands/slash.rs` `ArgKind`, `workspace::open_slash` | new `ArgKind::Ssh` on the existing rows / submit / Tab code |
| child with timeout and group kill | `crates/vr-git/src/cmd.rs` spawn | same pattern in `ssh::probe`; vr-git's is bound to its `Git` type |
| grid rendering | `ui/terminal_view.rs` | reused `render_grid` |
| screen model for pty walks | `qa-artifacts/T10/vt.py` | reused |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | files listed above are the task's; the 664 recent.toml is only noted |
| BASE-2 | pass | argv / LocalCommand / file format as the threat model writes them: `sec_s2_argv_for_each_target_shape`, `sec_s3_*`, `sec_s10_*` |
| BASE-3 | pass | red runs in Progress (6, 18, 24 failures) before the code |
| BASE-4 | pass | "Commands run + result" |
| BASE-5 | pass | suites, clippy and builds through run-heavy.sh; single filters directly; nothing left running |
| BASE-6 | pass | English comments, "why" only |
| BASE-7 | pass | no commit, nothing staged |
| BASE-8 | pass | no new crate; rustix feature `rand` (OS CSPRNG for the nonce) |
| BASE-9 | pass | `Target::parse` is the only way to a target; `ssh::tests::sec_s1_*` |
| BASE-10 | pass | related modules and the guard only; no full suite |
| BASE-11 | pass | no stash / checkout / restore / reset; work only in the worktree |
| BASE-13 | pass | only my own `*.snap.new` files and my scratch folder were removed |
| BASE-14 | pass | no wait loops; one command the harness moved to the background was left to finish |
| BASE-15 | pass | no pkill / killall; the walk ends only the vr-ide it started |
| BASE-16 | pass | Reuse table |
| BASE-17 | pass | nothing installed (zsh, fish, sshd absent and reported) |
| BASE-18 | pass | no agents started |
| BASE-19 | pass | Progress section from the first step |
| BASE-20 | pass | nothing removed; the shell tab code was moved into `push_tab / replace_term / spawn_program` with its callers |
| BASE-21 | pass | walk_result.txt holds no nonce or password (grep count 0); failure details replace the nonce |
| BASE-22 | pass | `ssh_fullscreen`, `ssh.connect`, `/ssh`, `ssh-recent` follow their neighbours |
| BASE-23 | pass | every way in is tested: `/ssh`, `ssh `, palette command, click on the tab, Enter on an exited tab, list row of an open session |
| BASE-24 | pass | no index or history command |
| BASE-25 | pass | fmt --check and clippy clean on the final code |
| BASE-32 | pass | D317, D320 (setting, notice why, sanitized titles), D314 / D316 (kept keys: `sec_s14_*`), D315 (scroll code untouched), D227 (ctrl+f7 still moves the panel), T26 (`ac_s6_sec_s17_*`) |
| BASE-35 | pass | USER-3 no mockup round; USER-5 no new key; hints come from the keymap (`key_hint`) |
| BASE-37 | pass | not a defect fix; the walk runs the real binary |
| BASE-38 | pass | `cargo fmt -p` on the three touched crates; `--all --check` shows no other diff |
| BASE-45 | pass | AC-S1..S6 are in 01_spec_FR17-21.md lines 20-25 |
| BASE-46 | pass | walk and test runs repeated after the last code change |
| BASE-47 | pass | every reader of a tab title goes through `TermTab::title()` (tab bar, status bar); `sec_s12_*` checks both rows |
| BASE-48 | pass | state folder mode read back in the walk (775), not assumed |
| n-a: BASE-12, 26, 27, 28, 29, 30, 31, 33, 34, 36, 39, 40, 41, 42, 43, 44 | n-a | no other repository, rewrite, shared database, third-party code, browser, container, remote system, schema, credential, test-runner setting, access rule or fake data adapter in this task |
| COST-1 | pass | as BASE-5 |
| COST-2 | pass | developer tests and one pty walk only |
| COST-3 | pass | no sleep loops in commands (tests wait on conditions with deadlines) |
| COST-4 | pass | no full suite |
| COST-5 | pass | no server, watcher or browser started |
| COST-8 | pass | batched reads, whole-file writes, perl for multi-place edits |
| COST-9 | pass | nothing left running (process check after the walk: 0) |
| COST-10 | pass | related set from `rule-checks.sh related-tests FR-25`, see Commands |
| COST-13 | pass | walk.py and its result under qa-artifacts/FR-25; work folders in the session scratchpad |
| COST-15 | pass | the one snapshot written once |
| COST-16 | pass | outputs filtered to result lines; walk output in walk_result.txt |
| n-a: COST-6, 7, 11, 12, 14, 17, 18, 19, 20 | n-a | no web screens, API reads, e2e, leader batching, CI, image goldens, full run or qc check set |

## Fix round 1
Findings: review 05_review_FR-25.md + leader re-grade (#1 MAJOR late probe takes the keys; #2 `Host` comment tokens; #3 `SpawnExtras` Debug; #4 probe reader thread; #5 BASE-16 shared helper).

### Progress (round 1)
- read notes, review, commands/ssh.rs, ssh/{mod,config}.rs, vr-term types.rs, terminal.rs push_tab/reveal, app.rs route_key/paste/mouse
- red: vr-term marker `debug_output_of_the_spawn_extras_hides_the_marker` FAILED; vr-ide ssh: 5 failed / 49 passed (`sec_s11_only_concrete_*` #2, `sec_s4_a_helper_outside_the_group_*` #4 leaked reader thread, `ac_s2_a_late_probe_answer_{never_takes_keys_typed_elsewhere (focus moved to the tab), leaves_an_open_command_window_alone (Esc went to Terminal), does_not_switch_the_tab_the_user_types_in (active 1)}` #1); `ac_s2_a_late_probe_answer_takes_the_keys_when_nothing_happened_meanwhile` green (guard of the kept behaviour)
- code: #1 `KeysAt` snapshot in commands/ssh.rs + `App.input_count` + `push_tab(.., activate)` + `terminal::show_panel`; #2 `tokens` stops at `#`; #3 manual `Debug` for `SpawnExtras`; #4 probe reads its pipe with `poll` up to the deadline (no reader thread; rustix feature `event`)
- green: vr-term marker 6/6; vr-ide `--lib ssh` 54/54 (twice; the new helper test first failed once under load reading the helper's pid before it was written — the test's cleanup now waits for it); no `sleep 30x` helper left (ps)
- replay before the fix: qa-artifacts/FR-25/late_probe_walk.py on the pre-fix binary (built from fb6a5de) -> RESULT 5/8; L-02 no notice, L-04 FAIL `ssh read: 'jjkk'` (editor keys taken as the password), L-05 FAIL
- replay after the fix: late_probe_walk.py on the rebuilt binary -> 8/8 (qa-artifacts/FR-25/late_probe_walk_result.txt)
- related run: 6 slash snapshots failed (`slash_tests::render_slash_mode`), all the FR-28 detail area under a cut selected row (`/save-as …`, `/theme …`) that the merge brought in; reviewed each diff and accepted them
- guard clean; related tests green; qc_walk.py on the rebuilt binary 60/60

### Findings
| # | Reply | Change |
|---|---|---|
| 1 MAJOR late probe takes the keys | fixed | `commands/ssh.rs`: `KeysAt { input, focus, palette, dialog, active }` taken in `start`, kept in `Pending`. In `open` a new tab takes the keys (`push_tab(.., true)` + `reveal_panel`) only when `KeysAt` is unchanged and neither palette nor dialog was open. Otherwise `push_tab(.., false)` (active tab and full screen of another session unchanged), `terminal::show_panel` (shown, focus kept) and the notice `SSH <target> ready — click its tab`. `App.input_count` (app.rs) counts keys (not releases), pastes and mouse presses. That also covers a case the review's fix would have missed: the user starts from the editor, the focus stays "unchanged", but the user types there meanwhile. The old "palette open -> prev_focus = Terminal" branch is gone |
| 2 minor `Host x # comment` | fixed | `ssh/config.rs` `tokens`: an unquoted token starting with `#` ends the line; a quoted `"#x"` is still one (quoted, never listed) token |
| 3 minor `SpawnExtras` Debug | fixed | `vr-term/src/types.rs`: manual `Debug`, `marker: Some("..")` / `None`, `env_remove` printed |
| 4 low reader thread | fixed | `ssh/mod.rs`: no reader thread any more. `read_until` polls the pipe (`rustix::event::poll`, with what is left of the deadline) and reads only when it is readable or hung up. At the deadline the pipe is dropped and the group killed, so a writer outside the group (`setsid`) holds nothing. The probe thread ends within timeout + kill. rustix feature `event` added (no new crate) |
| 5 minor BASE-16 shared helper | left, reason | The common part is now only the 10-line PATH rule (`find_in_path` vs vr-git's private `find_git`). The probe's read and kill no longer match vr-git's spawn. Sharing would add a public non-git API to `vr-git`, a crate outside this task's files (board row: vr-ide, vr-term, vr-config). Follow-up for the leader if wanted: `vr_git::find_program(path, name)` used by both |

### Files changed (round 1)
- crates/vr-ide/src/commands/ssh.rs (`KeysAt`, `Pending.keys`, `open(.., keys)`)
- crates/vr-ide/src/app.rs (`input_count` + 3 increments)
- crates/vr-ide/src/commands/terminal.rs (`push_tab(.., activate)`, `show_panel`)
- crates/vr-ide/src/ssh/config.rs, crates/vr-ide/src/ssh/mod.rs (`read_until`; `mpsc` import removed), crates/vr-ide/Cargo.toml (rustix `event`)
- crates/vr-term/src/types.rs
- tests: crates/vr-ide/src/ssh_tests.rs (shim mode `gslow`; 4 tests `ac_s2_a_late_probe_answer_*`), crates/vr-ide/src/ssh/tests.rs (`sec_s11_only_concrete_*` +2 lines, `sec_s4_a_helper_outside_the_group_that_keeps_the_output_open_leaves_no_thread`), crates/vr-term/tests/marker.rs (`debug_output_of_the_spawn_extras_hides_the_marker`)
- snapshots: crates/vr-ide/src/snapshots/vr_ide__slash_tests__render_slash_{list,error}_{40x12,80x24,120x40}.snap (FR-28 detail area, see Progress)
- qa: qa-artifacts/FR-25/late_probe_walk.py (reuses qc_walk.py's `Run` by executing the part of the file above its flows; a slow-`-G` shim that records what its password prompt read), late_probe_walk_result.txt, qc_walk_result_r1.txt

### New states (round 1)
- `Pending.keys` (snapshot): quit while pending: unchanged (pending dropped with the app). Esc or palette meanwhile: counts as input, so no keys. Error halfway (spawn fails): `open` returns before any tab, and the snapshot is unused. A second request: refused while pending (SEC-S16), as before. A probe that never answers: given up after `PROBE_GRACE` as before. Tests: `ac_s2_a_late_probe_answer_{never_takes_keys_typed_elsewhere, leaves_an_open_command_window_alone, does_not_switch_the_tab_the_user_types_in, takes_the_keys_when_nothing_happened_meanwhile}`
- An inactive session tab (no keys): its login marker gives the existing notice "logged in; choose its tab for full screen" (poll only goes full screen for the active, focused tab). L-05 in the replay chooses it and logs in
- `read_until` (poll loop): a hung pipe ends at the deadline, EINTR is retried, and output over the cap fails. Tests: `sec_s4_a_helper_outside_*`, plus the existing `hanging_probe`, `over_the_cap` and `closes_its_output_but_stays` tests

### Commands run + result (round 1, final code)
```
$ run-heavy.sh cargo fmt --all --check          -> rc 0 (FMT-OK)
$ run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings  -> Finished, no warning
$ run-heavy.sh cargo test -p vr-term
lib 14 · env 1 · flood 1 (1 ignored) · leak 1 · marker 6 · pty 21 — 0 failed
$ run-heavy.sh cargo test -p vr-config
17 · 24 · 8 · 0 passed — 0 failed
$ run-heavy.sh cargo test -p vr-ide --lib ssh
test result: ok. 54 passed; 0 failed; 0 ignored; 0 measured; 616 filtered out
$ run-heavy.sh cargo test -p vr-ide --lib -- terminal slash ssh ui:: palette app_tests dialog
test result: ok. 354 passed; 0 failed; 3 ignored; 0 measured; 313 filtered out
$ python3 qa-artifacts/FR-25/late_probe_walk.py <pre-fix binary fb6a5de> <scratch>
FAIL L-02 (no notice) · FAIL L-04 ssh read: 'jjkk' · FAIL L-05 · RESULT 5/8
$ python3 qa-artifacts/FR-25/late_probe_walk.py target/debug/vr-ide <scratch>
RESULT 8/8 passed
$ python3 qa-artifacts/FR-25/qc_walk.py target/debug/vr-ide <scratch>
RESULT 60/60 passed            (qa-artifacts/FR-25/qc_walk_result_r1.txt)
```
Both result files: 0 hits for the walk password or a 32-hex nonce. Afterwards no vr-ide, shim or `sleep 30x` helper process was left (ps), and the scratch folders were removed.

### Capabilities lines to add (capabilities.md is outside $TEAM_WS)
- `commands::terminal::show_panel(app)` (show the panel, keep the focus); `push_tab(.., activate: bool)`
- `App.input_count`: user input counter (keys, pastes, mouse presses), to tell "did the user do anything since X"
- `qa-artifacts/FR-25/late_probe_walk.py`: pattern for reusing qc_walk.py's `Run` with a modified shim

### Rule compliance (rows touched in round 1)
| Rule | Status | Evidence |
|---|---|---|
| BASE-3 | pass | red first: vr-term 1 failed, vr-ide 5 failed (Progress, round 1) before any code change |
| BASE-4 | pass | "Commands run + result (round 1)" |
| BASE-8 | pass | no new crate; rustix feature `event` |
| BASE-9 | pass | no new input boundary; `tokens` is stricter (`sec_s11_only_concrete_*`) |
| BASE-10 | pass | only vr-term, vr-config, `vr-ide --lib ssh` and the related filters for app.rs/terminal.rs; no full suite |
| BASE-16 | pass | finding 5 reasoned above; the walk reuses qc_walk.py's harness instead of copying it |
| BASE-19 | pass | Progress (round 1) line per step |
| BASE-21 | pass | result files grep 0 for password / nonce; `SpawnExtras` Debug redacted |
| BASE-25 | pass | fmt --check, clippy -D warnings clean |
| BASE-32 | pass | D317 / D325 ("full screen only on login with keys; notices otherwise") hold: `takes_the_keys_when_nothing_happened_meanwhile`, L-07 |
| BASE-37 | pass | the defect was replayed on the pre-fix binary (L-04 `ssh read: 'jjkk'`) and after the fix (8/8); tests drive the real `App` key routing |
| BASE-38 | pass | `cargo fmt -p vr-ide -p vr-term` only; only this task's files are modified |
| BASE-46 | pass | the walks ran on the binary built after the last code change (the build after it was a no-op) |
| COST-8 | pass | Edit tool for code; one perl pass per file for the mechanical multi-site edits (app.rs, terminal.rs); no python heredoc edits |
| COST-9 | pass | nothing left running (ps after the walks) |
| COST-13 | pass | scripts and results in qa-artifacts/FR-25; scratch in the session scratchpad; nothing in $TEAM_TOOLS |
| COST-15 | pass | slash snapshots accepted once after reviewing each diff |
| COST-16 | pass | outputs filtered to result lines |

Other rows are unchanged from round 0.

Status: DONE FR-25 r1
