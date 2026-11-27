## QC T1
Engine: cargo (tui project, no browser). Throwaway harness: `$TEAM_WS/qa-artifacts/T1/` (standalone crate, path dependency on vr-config, run with CARGO_TARGET_DIR inside the artifact dir, target removed afterwards; no repo files added).

| Check | Result |
| `cargo test -p vr-config` | 17+27+8 passed, 0 failed; `ai_auto_switch_defaults_and_override ... ok` |
| user file absent / keys absent | switch=true, prompt "Continue where you left off.", 0 warnings |
| override `ai_auto_switch=false`, prompt "Go on." | applied, 0 warnings |
| prompt of 200 chars (ASCII and 200 x "é") | accepted, 0 warnings |
| prompt of 201 chars | default kept, 1 warning (InvalidValue) |
| tab character | default kept, 1 warning |
| multi-line (basic string `\n` and multi-line TOML string) | default kept, 1 warning each |
| empty prompt | default kept, 1 warning |
| `ai_auto_switch = "yes"` | switch=true (default), warning "invalid type: string "yes", expected a boolean" |
| prompt = 5 (wrong type) | default kept, 1 warning |
| bad prompt + valid `ai_auto_switch=false` together | switch=false applied, prompt default, 1 warning (keys isolated) |

## Rule check T1
| Rule | Status | Evidence |
| AC-1 defaults true / "Continue where you left off." | pass | absent-file and absent-keys runs |
| AC-1 user overrides apply | pass | override_ok run |
| AC-1 empty / >200 / control char -> warning + default kept | pass | 201, tab, newline, empty runs |
| Boundary 200 chars accepted (counted in chars) | pass | len200, 200_multibyte |
| Wrong type -> warning + default | pass | switch_yes, prompt_wrongtype |

## QC T2
Engine: Rust harness (no TUI/browser; tui project, driving the TUI judged impractical). Scratch crate `qa-artifacts/T2/qa_t2` calls the public `vr_ide::ai::history::{ensure, shared_dir}` on temp dirs (fake HOME, nothing under the real ~/.claude, ~/.codex or ~/.local/state/vr-ide touched). Built with `run-heavy.sh cargo build --offline` (separate target dir, removed after). Output: `qa-artifacts/T2/run.log`.
Real output lines:
- C1: `Linked`; link == canonical shared path; sha256 a.jsonl d27a9131...8158 before = after (in shared), b.jsonl f2c82dec...51ad before = after; via link lists [a.jsonl, b.jsonl].
- C5 (second run): `Linked unchanged=true`.
- C2 clash: `NotShared("a conversation exists on both sides; left in place")`, all hashes unchanged, entry still a dir, other.jsonl not moved to shared.
- C3 link elsewhere: `NotShared("history entry links elsewhere; left as it is")`, link and target file unchanged, shared empty.
- C4 shared 0755: `NotShared("shared history folder ... is open to others; run chmod 700 ...")`, mode still 755, account entry still a dir with file.
- C6 codex: shared_dir = $CODEX_HOME/sessions; unset -> $HOME/.codex/sessions; claude honours CLAUDE_CONFIG_DIR; ensure -> `Linked`, link = $CODEX_HOME/sessions.
Not covered: real-binary end-to-end (spawn_session wiring is covered by the developer's `named_sessions_share_the_tools_history`, not rerun).

## Rule check T2
| Rule | Result | Evidence |
|---|---|---|
| AC-2 (link, recursive merge, refusals touch nothing) | PASS | C1, C2, C3, C5, C6 |
| SEC-19 (one entry, foreign link kept, no names in reasons) | PASS | C3, fixed reason texts |
| SEC-20 (move not loss, idempotent, clash leaves all) | PASS | C1, C2, C5 hashes |
| SEC-21 (shared folder 0755 refused, mode unchanged, created 0700, env-derived path) | PASS | C4, C6 |

## QC T2b
Engine: cargo test (no browser needed). Temporary test `terminal_tests::qc_t2b_tmp_real_spawn_env` was added to the worktree, run, then removed (file restored from backup; `grep -c qc_t2b` = 0).
Setup: state dir in a temp folder, process env `CLAUDE_CONFIG_DIR=<tmp>/state/accounts/x`, `CODEX_HOME=/tmp/elsewhere`, real spawn via `spawn_program` (shell and /run path) and `spawn_program_env` (the ssh path, `commands/ssh.rs:683`) with a fake `ssh` script printing its env.
Command: `cargo test -p vr-ide --lib -- --exact terminal_tests::qc_t2b_tmp_real_spawn_env --nocapture`
Output:
- SHELL_OUT: `CC=[] CX=[/tmp/elsewhere]`
- SSH_OUT: `SSH CC=[] CX=[/tmp/elsewhere]`
- CONTROL_OUT (state_dir = None): `CC=[/tmp/.tmpDx5QLE/state/accounts/x] CX=[/tmp/elsewhere]`
- `test result: ok. 1 passed`
Code reading: shell (`terminal.rs:940`), ssh (`ssh.rs:683` -> `spawn_program_env`) and agents (`ai/mod.rs:770`) all reach `spawn_sized`, which applies `without_stray_vars`. `ssh/mod.rs::probe` (`ssh -G`, no terminal, env only PATH set; inherits env) is not a terminal child and is out of the stated scope; it runs no user shell, only `Match exec` could see the variables (note, not a defect).

## Rule check T2b
| Rule | Status | Evidence |
|---|---|---|
| SEC-38 | pass | account CLAUDE_CONFIG_DIR gone in shell and ssh child; CODEX_HOME=/tmp/elsewhere kept; control shows removal is the cause |
| BASE-14 | pass | foreground short runs |
| BASE-15 | pass | nothing killed |
| COST-13 | pass | no scripts in $TEAM_TOOLS |

## QC T3
Engine: real binary `target/debug/vr-ide` (built once with run-heavy.sh) + scratch Rust harness `qa-artifacts/T3/qa_t3` calling `vr_ide::ai::usage::{prepare, read_report}`; script `qa-artifacts/T3/run.sh`, full output `qa-artifacts/T3/run.log`. The `statusLine.command` from the generated settings file was run through `sh -c "<command>"` like claude does. All state in `mktemp -d` (removed); no real claude/codex, nothing under ~/.claude or ~/.local/state/vr-ide touched. Harness `target/` removed afterwards.

(1) Normal: settings file mode 600, `ai-usage/` mode 700, body `{"statusLine":{"command":"'<exe>' --ai-statusline '<state>/ai-usage/<pid>-1-<32hex>.report'","type":"command"}}`. Helper: exit 0, stdout `Opus 5.5 · 5h 63%`, report mode 600, `read_report` = session_id 0f8e2c1a-..., five_hour 63.0, seven_day 20.5, model "Opus 5.5". Extra fields dropped.

(2) Hostile (exit code / report):
- 1 MB JSON: exit 0, stdout empty, no report.
- invalid JSON: exit 0, empty, no report.
- session_id `--dangerously-skip-permissions`, uppercase UUID, 37 chars: exit 0, report written with usage but WITHOUT session_id (read_report session_id None).
- resets_at +1e11 s and -5: exit 0, report with resets_at null for both, percentages kept.
- ANSI/OSC/BEL/newline in display_name: exit 0, stdout and report hold caret text (`^[[31mR^Gx^Jy`), `od -c` shows no raw 0x1b/0x07/0x0a in either.
- stdin never closed (fifo writer held open 8 s): helper exited 0 after 2.02 s, stdout empty, no report.
Each ran in <10 ms except the stdin case.

(3) State path `sp ace'quo te`: command `'<exe>' --ai-statusline '<...>/sp ace'\''quo te/ai-usage/....report'`, ran through sh -c, exit 0, report written 0600 with correct values. Path with backslash and path with newline: `prepare` returns `Err("state folder path unusable")` (no settings file, so no `--settings`).

(4) Wrong forms: path outside ai-usage (`<w>/outside.report`), parent `ai-usage-x`, bad file name `evil.report`, relative path, `..` path: exit 0, no stdout/stderr, nothing created (find for report-like files none; `ai-usage/` listing unchanged). NOTE: `--ai-statusline` with no argument or with two arguments is not treated as helper mode (by design, `cli::ai_statusline` test asserts None) and falls to the normal launcher: exit 2, stderr `vr-ide: path does not exist: .../--ai-statusline`, nothing written, no UI started. Differs from the brief's "exit 0" but is harmless (claude only runs the exact two-arg command); minor, no action required.

Not verified: SEC-22 "a project's settings cannot override ours" (needs a signed-in claude; dev read claude's settings precedence from the binary: flagSettings above project/local).

## Rule check T3
| Rule | Result | Evidence |
|---|---|---|
| AC-3 (report of session id + 5h/weekly usage via helper) | pass | (1): report 0600, read_report values, compact status line |
| SEC-22 quoting / 0600 / path guard | pass | (1) modes, (3) quote with space+quote works, backslash/newline refused |
| SEC-22 project settings cannot override | unverified | needs a signed-in claude, not run |
| SEC-23 helper writes only own report, bounded, no stdout leak | pass | (2) big/bad/never-closed stdin 2 s bound; (4) nothing written for wrong forms |
| SEC-24 session id canonical UUID | pass | (2) option-like, uppercase, 37-char ids dropped from report |
| SEC-25 report fields validated, control chars stripped | pass | (2) resets_at bounds, ANSI neutralised |
| BASE-14 / BASE-15 / COST-13 | pass | foreground short runs; nothing killed; scripts under qa-artifacts/T3 |

## QC T3 re-check
Case: usage report must reach the session with the panel hidden and no PTY output (fix round 1: `event_loop::background_pass` calls `ai::read_usage`).
Engine: temporary unit test `ai_tests::qc_t3_recheck_hidden_idle_reports` (added, run, removed; `grep -c qc_t3` = 0 afterwards). Fake claude from `usage_app()` (prints once, `exec cat`, idles), started via `ai::spawn_session` with a temp state folder; no frame drawn (`layout.ai` = 0x0, panel hidden); report file rewritten directly, `background_pass` driven with synthetic `Instant`s. Command: `cargo test -p vr-ide --lib -- qc_t3_recheck claude_session_reads_usage_report usage_settings_only --nocapture`.
Output:
- initial pass: usage 63.0 read.
- write1 (11%), pass at +1100 ms: usage=11, dirty=true, reads=1
- write2 (22%) 100 ms-equivalent after the read, pass at +300 ms: usage still 11, dirty=false (read limited to 1/s)
- write3 (33%), pass at +1100 ms after the first read: usage=33 (the last value), dirty=true, reads=2
- `test result: ok. 3 passed` (the two existing usage tests still pass).
Rule check: AC-3 pass (hidden panel, idle agent); SEC-39 read-rate bound pass (no read at +300 ms).

## QC T5 — limit/login detection, per-account state
Method: temporary tests appended to `crates/vr-ide/src/ai_tests.rs` (fake agent `sh` script started through `spawn_session`, temp state folder, `background_pass` with injected clock); file restored from `qa-artifacts/T5/ai_tests.rs.orig` afterwards (no `qc_t5` left). Logs: `qa-artifacts/T5/qc_run1.log`, `qc_run2.log`. Real claude/codex never run. Engine: cargo test (no browser; be task).

| # | Scenario | Result (real output) |
|---|---|---|
| 1 | claude screen `⎿  You've hit your 5-hour limit · resets 3pm` + full rounded input box | @500 ms and @999 ms: notices=[]; @1000 ms: `["claude hit its usage limit (resets 15:00)"]`; repeat passes to +30 s: still 1; running=true; state `{"claude/default":{"limited_until":1795878000,"works":false}}` mode 600. With account `work`: `claude@work hit its usage limit (resets 15:00)` |
| 2 | codex `■ You've hit your usage limit. Try again at 3:30 PM.` + composer | `["codex hit its usage limit (resets 15:30)"]`, state `codex/default` limited_until 1795879800, mode 600 |
| 3 | spoofs: inside longer line; `> ` quote; `●` bullet; limit line followed by `● Done` then quiet; echoed in input box; `> ` prompt echo; typed via keys into a box-less screen | first six: notices=[], state absent, running. Typed via real key events (tty echo on a bare screen with no input box): notice shown — see F1 |
| 4 | usage report "5h 100% used" on screen, no limit line; plus existing `ai_limit_state_follows_usage_reports` at 100 % | no notice, running=true, limit not ended/created |
| 5 | login screen `Select login method:` with seeded `works:true` | before `works:true`, after `{"claude/default":{"limited_until":1795801208,"works":false}}` mode 600 (a default entry with no limit is dropped from the file by design, absence = works false) |
| 6 | two writers: 2 threads x 60 updates; 2 separate processes x 150 updates (run concurrently) | both: valid JSON, both `claude/a` and `codex/b` keys present, mode 600, lock file present |
| 7 | garbage bytes, 1 MB junk, 1 MB valid-looking JSON, wrong types, array | `load_state` = {} each, no crash; following update rewrites a valid 61-byte file |
Own-process check: `cargo test -p vr-ide --lib ai_limit` 5 passed; `limits::` 14 passed.

Notes: my first claude run showed no notice because my fake box had no bottom border (a single rule is not an input box and the rule row counted as later content). With a complete box it passes; controls had to pass before the spoof results counted, and they did (scenario 1).

Findings (non-blocking):
- F1 (low): text typed by the user with real keys into a program that only tty-echoes (no prompt marker, no input box) at column 0 and then goes quiet is treated as a limit line (notice + state `limited_until`). Real claude/codex draw typed text inside the input box or after `>`/`›`, which are ignored (scenarios 3, tests `the_input_box_is_ignored`). Cheap hardening: ignore a screen check when `limit_watch.typed` was set since the last tool output. Matches the accepted residual in the threat model (T6 restart cap bounds it).
- F2 (info): D3 — gemini/aider/cursor-agent have no patterns; leader's call.

## Rule check T5
| Rule | Status | Evidence |
|---|---|---|
| AC-5 | pass | scenarios 1, 2 (one notice with reset time, only after >= 1 s quiet, no repeat), 4 (100 % alone: no notice, process running) |
| SEC-26 | pass (F1 residual) | scenario 3: whole-line only, quote/bullet/longer line/followed output/input-box echo give no notice; notice text fixed |
| SEC-27 | pass | scenarios 5, 6, 7: mode 0600, locked updates keep both keys across threads and processes, garbage/1 MB ignored without crash |
| BASE-14 | pass | cargo runs in background, no polling |
| BASE-15 | pass | no process stopped |
| BASE-17 | pass | temp dirs removed, test file restored |
| BASE-18 | pass | no agents |
| BASE-19 | pass | results written to report |

## QC T5 re-check
Harness: temporary module `crates/vr-ide/src/qc_t5_tests.rs` plus one `mod` line in the untouched `lib.rs`; both removed afterwards (lib.rs diff empty again; no task-diff file touched). Copy kept as `qa-artifacts/T5/qc_t5_tests.rs.txt`, log `qa-artifacts/T5/qc_recheck.log`. Fake agents through `spawn_session` + `background_pass` at 0/0.5/1/1.1/5 s.

| Case | Real output |
|---|---|
| claude `resets naïve`, `resets 3pmé`, `resets éé` | no panic, notices=[], state absent, process running |
| codex `Try again at aaé.`, `…3:30 PMé`, `…at é` | no panic, process running, state absent; notice `codex hit its usage limit` (fixed text, no reset part, no screen text). By design: the codex head `You've hit your usage limit.` is genuine alone and an unparsable reset only drops the time. Flagged in case a non-time should also suppress the codex notice (claude does) |
| control claude `resets 3pm` | `claude hit its usage limit (resets 15:00)`, state written |
| F1: user types exact claude and codex limit text, keys / keys+Enter / paste, bare tty echo at column 0, quiet 5 s | 6 of 6: notices=[], no `ai-account-state` file created |

## Rule check T5 re-check
| Rule | Status | Evidence |
|---|---|---|
| SEC-26 | pass | no panic on non-ASCII reset text; typed/pasted limit text gives no notice and no state |
| SEC-27 | pass | no state written for any spoof case |
| BASE-17 | pass | temp module and lib.rs line removed |

## QC T4
Engine: cargo test (TUI, TestBackend buffer + cell styles; no browser). Harness: temporary `crates/vr-ide/src/qc_t4.rs` + `mod qc_t4;` in lib.rs, both removed afterwards (copy: qa-artifacts/T4/qc_t4.rs.txt, log: qa-artifacts/T4/qc_run.log). Real `spawn_session` (fake claude script, temp state dir), report files written by hand, `background_pass`. `git diff --stat` before 683 insertions / after 686 (lib.rs and src not in the delta; the +3 are leader workspace .md edits made meanwhile); qc_t4 absent from `git status`.
Command: `run-heavy.sh cargo test -p vr-ide --lib qc_t4 -- --nocapture --test-threads=1` -> 5 passed, 0 failed.

- Case 1 (63 %/41 %): row ` 1 claude 63% x ... +`; no `w`, no notice, 63% cell fg (255,46,151) != warning (255,157,60). PASS
- Case 2 (40 %/91 %): ` 1 claude 91%w x`, painted warning; one notice `claude used 91% of its weekly limit (resets Nov 30 17:17)`. PASS
- Case 3: 89 -> none; 90 -> one (`used 90% of its 5-hour limit (resets 18:17)`); 95, 95 -> still one; new resets_at 91 % -> second notice; 97 % same new window -> still 2. PASS
- Case 4: no data -> `/ai usage: no usage reported yet for claude`; with data -> `claude: 5-hour 63% (resets 18:17) · weekly 41% (resets Nov 30 17:17)`; `/ai usage now` -> `/ai usage takes no argument`. PASS
- Case 5 (six sessions): rows
  - 40x12 active=1: ` 1 claude 63%  2  3  4  5  6          + `
  - 40x12 active=6: ` 1  2  3  4  5  6 claude·6 70%        + `
  - 80x24 (panel 31 cols): ` 1 claude  2  3  4  5  6     + ` (suffixes dropped, no +N); 120x40: ` 1 claude 63%  2  3  4  5  6                 + `
  - tabs end inside the bar and `+` stays past them at 40/30/80/120. PASS
- Case 6 (themes, 91%w vs 63% / label): synthwave fg (255,157,60) vs (255,46,151)/(185,166,230); dracula (255,184,108) vs (189,147,249)/(154,168,212); dark Yellow vs White/Gray; light Indexed(130) vs Black/DarkGray. Warning differs from normal in all four themes, both tab states. PASS

Not re-checked by me (covered by dev test `ai_usage_label_sizes`, 80x24): warning colour on a number-only tab at 40x12 with six sessions; at 40x12 here the 95 % tab 2 shows `2` (suffix dropped) and I did not read its cell style.

## Rule check T4
| Rule | Result | Evidence |
|---|---|---|
| AC-4 tab label percent `63%` / `91%w` when weekly higher | PASS | cases 1, 2 |
| warning style at >= 90 % | PASS | case 6, four themes |
| 90 % notice once per window | PASS | case 3 |
| `/ai usage` both windows, reset times, account | PASS | case 4 (`claude@work` form covered by dev `ai_usage_command_lists_windows`) |
| Fit 80x24 / 120x40 / 40x12, suffixes drop first (D35) | PASS | case 5 |
| Four themes | PASS | case 6 |

## QC T4 re-check
Temporary `qc_t4.rs` + `mod qc_t4;` (removed; copy `qa-artifacts/T4/qc_t4_recheck.rs.txt`, log `qc_recheck.log`; lib.rs has no diff, no qc_t4 in git status). Command: `run-heavy.sh cargo test -p vr-ide --lib -- qc_t4_recheck ai_limit_counts_after --nocapture --test-threads=1` -> 2 passed.
- Six sessions, tab 2 inactive at 95 %, 40x12 and 80x24, active tab 1 and 6, four themes: tab 2 label ` 2! `, `more` = None (nothing folded), all six tabs kept, row ends with `+`. Rows: 40x12 ` 1 claude 63%  2!  3  4  5  6         + `; 80x24 ` ... │ 1 claude  2!  3  4  5  6    + `. The `2` cell is not warning colour, the `!` cell fg == theme.warning.fg in synthwave, dracula, dark, light. PASS
- T5 limit notice: `ai_limit_counts_after_a_quiet_second_with_one_notice_per_event` passes, asserting `claude hit its usage limit (resets 15:00)`. PASS

Rule check T4 re-check: D39 `N!` marker PASS; fit/+N PASS; T5 notice text unchanged PASS.

## QC T6
Engine: cargo test (real spawn path, fake `claude` shell agents, temp state dirs; no browser; claude/codex never run). Harness: temporary `crates/vr-ide/src/qc_t6.rs` + `mod qc_t6;` in lib.rs, both removed (copy: qa-artifacts/T6/qc_t6.rs.txt; `git diff --stat` before/after identical, 22 files, untracked list unchanged). Accounts via `accounts::create`, marked working by state file; "new account has reported" simulated with `limit_watch.works_marked = true` in case 8 (as dev tests do).
Command: `run-heavy.sh cargo test -p vr-ide --lib qc_t6 -- --nocapture --test-threads=1` -> run 1: 6 pass, 3 fail (all three harness bugs: a missing state key index, a wait loop without clock advance, a restart that exited unreported so the id was rightly dropped per D43); run 2 after fixing the harness: `9 passed; 0 failed` (qc_run2.log, first run qc_run.log).

- C1 work -> personal: log `start work, got work limit, start personal, got personal Continue where you left off.`; pids file `work 2398343 cfg=.state/accounts/work | personal 2398351 cfg=.state/accounts/personal`; old pid alive=false; `--resume|<ID>|` asserted in argv (no `--continue`); continue prompt echoed once; notices `claude@work hit its usage limit (resets 15:00)`, `switched claude from work to personal (work limited until 15:00)`; label `claude@personal`; state `claude/personal works=true; claude/work works=true limited_until=1795878000`. PASS
- C2 two tabs same folder (ID / ID2), tab b limited: b -> personal with `--resume|ID2|` only; tab a pid unchanged and alive, still `work`, screen has no `--resume`/`GOT:`; only b got a prompt. PASS
- C3 user typed after the limit: switch to personal happened, log has no `Continue`, notices only the limit and `switched ...`; no "type to continue" notice (even after +31 s). See note N1.
- C4 personal limited (+600 s), default not working: notice `all claude accounts are limited until 18:08`; pid unchanged, running, account work, log `start work, got work limit`. PASS
- C5 `/ai autoswitch off`: notice `auto-switch off for claude@work`, limit notice only, pid same; tab bar row ` 1 claude@work ⊘ x`. Setting ai_auto_switch=false: same row and limit notice only, no switch. PASS
- C6 personal shows `Select login method:`: personal marked not working (entry not worked in state), notice `claude@personal is not logged in; trying the next account`, then `switched claude from personal to third`, argv `ACCT:third`; log has no `got personal`; one prompt typed into third. PASS
- C7 target with trust screen (`Quick safety check ...` + `Accessing workspace:`): log `start work, got work limit, start personal` (nothing typed), one notice `claude@personal is waiting: type to continue`. PASS
- C8 four limit events: starts 4 (work, a, b, c); event 4 on c: no switch, `auto_switch=false`, mark ` ⊘`, one notice `claude@c: 3 account switches in 30 minutes; auto-switch is off for this session`. PASS
- C9 restart (Enter on exited) with id: `RAN:--base|--resume|0f8e2c1a-...|`, no `--continue`. session.toml section from a live session: `session_id = "0f8e2c1a-7b3d-4e5f-9a1b-2c3d4e5f6a7b"`; restore of that section: `ARGV:--base|--resume|0f8e2c1a-...|`. PASS

Notes
- N1 (brief vs implementation): the brief expects a "type to continue" notice in case 3 (user typed). The implementation is silent when the user typed; spec AC-6 and threat model SEC-30 (c) only require no prompt, and the notice is required for (e) 30 s timeout and dialogs (D44, D47), which C7 confirms. Not counted as a defect; leader to confirm.
- Unverified (dev disclosed, unchanged): real claude idle-box layout and replayed limit line on `--resume` are assumptions; fails safe.
- Dev suite not rerun by me; dev's final-run.log shows 261 related tests ok.

## Rule check T6
| Rule | Result | Evidence |
|---|---|---|
| AC-6 | PASS | C1, C2, C3 |
| AC-7 | PASS | C4, C5, C6 (no id covered by dev `no_session_id_notifies_only`, not rerun) |
| SEC-28 (old run gone, hold/lock) | PASS | C1 old pid dead before new start (log order, /proc); dev kill/hold tests not rerun |
| SEC-29 (3 per 30 min, login retry) | PASS | C8, C6 |
| SEC-30 (prompt only to ready box, not after typing) | PASS | C3, C7 (two dialogs), C1 once |
| SEC-31 (resume by id) | PASS | C9, C2 |
| BASE-14/15/17 | PASS | background run, nothing killed by me, temp module + mod line removed, diff stat same |

## QC T6 re-check
Result: BLOCKED (no change to /home/rainy/DEV/vr-ide; nothing written there, nothing run).
Case to check: `StartMode::Resume` start on personal (picker, Down/Up + Enter, replayed tail ends on a limit line -> no state, no notice, no switch; after a real user line and a fresh limit line -> normal event).
Why a standalone harness cannot do it: the limit/switch machinery is driven only from `event_loop::background_pass(app, now)` (`pub(crate)`, called from `run`), which calls `ai::limits::check` and `ai::switch::tick` (both `pub(crate)`). `ai::spawn_session` (needed for `StartMode::Resume`) is also `pub(crate)`; the public `ai::open_tool` only starts `StartMode::New`. The only public entry that runs the passes is `event_loop::run`, which polls real terminal input and the real clock, so a quiet second cannot be given by the test and keys cannot be injected without a pty driver around it. The test helpers (`ai_tests::{ai_app, pass, wait_for, ...}`) are `pub(crate)` inside the `#[cfg(test)]` modules.
Needed to proceed: either a temporary `#[cfg(test)] mod` line + file in crates/vr-ide/src (allowed once the reviewer's re-check of the tree is finished), or a pty-driven run of `vr-ide` itself with a fake claude (heavier, real-time waits).

## QC T6 re-check (replaced the BLOCKED note above)
Engine: cargo test with a temporary in-crate harness (`crates/vr-ide/src/qc_t6.rs` + one `mod qc_t6;` line in lib.rs), current tree after fix round 2. Both removed; `git diff --stat` (24 files, 570 insertions, 50 deletions) and `git status --short` identical before/after. Files I wrote in the project: crates/vr-ide/src/qc_t6.rs (removed), the lib.rs mod line (removed), qa-artifacts/T6/{head.rs, tail.rs, qc_t6.rs.txt, qc_t6_choose_earlier.rs.txt, qc_run*.log, qc_recheck2.log, ds_/st_ before/after txt, diffstat_*.txt, untracked_before.txt}, this report.
Command: `run-heavy.sh cargo test -p vr-ide --lib qc_t6 -- --nocapture --test-threads=1` -> `1 passed; 0 failed`.
Case: Resume start (StartMode::Resume) on personal, fake claude shows a list; keys Down, Up, Enter (agent got `[ESC[B ESC[A]`), then replayed tail ending on the 5-hour limit line.
- After pick, 3 quiet seconds: limit_seen=false, notices=[], state personal works=true limited_until=None. PASS
- After a second empty Enter (+3 quiet seconds): same: no notice, nothing recorded, account personal, log `start personal, got personal [ESC[B ESC[A], got personal []`. PASS
- Then a real line `real` and fresh limit line: notices `claude@personal hit its usage limit (resets 15:00)`, `switched claude from personal to other (personal limited until 15:00)`; state personal limited_until=1795878000; old pid 2469639 -> new pid 2469644, argv ACCT:other. PASS
Rule check T6 re-check: AC-6 PASS, AC-7 PASS (no event on replay), SEC-29 PASS (no event from a replayed tail), BASE-14/15/17 PASS.

## QC T7 (Codex limits) - QC_PASS
Engine: in-crate temporary harness (BASE-50; removed, `git status --short` and `git diff --stat` identical before/after), fake codex shell script through the real spawn path (`start_program`, PTY), REAL helper = `target/debug/vr-ide` (built once via run-heavy.sh). The fake parses the notify TOML with python `tomllib` and runs program + one JSON arg like codex. Harness copy: qa-artifacts/T7/qc_t7_tests.rs.txt; logs qc_run.log, qc_q5.log, q*.argv, q*.log. Tools: Bash only (no browser needed); no real codex/claude run.

| Case | Result | Evidence |
|---|---|---|
| 1 ARGV + helper | pass | ARGV `-c|notify=["/home/rainy/DEV/vr-ide/target/debug/vr-ide", "--ai-codex-notify", "<state>/ai-usage/<pid>-1-<hex>.report"]|--base|`; TOML parsed back, helper run with payload carrying input-messages "secret msg"/last-assistant-message; report body exactly `{"session_id":"0f8e2c1a-..."}`, mode 600; session_id() = ID |
| 2 limit switch | pass | restart ARGV `...report(new)"]|--base|resume|0f8e2c1a-...|`, no `--last`; old pid gone (/proc absent); CODEX_HOME=`.state/accounts/personal`; log: start work, turn, limit, start personal - nothing typed; notices: "codex@work hit its usage limit (resets 15:00)", "switched codex from work to personal (work limited until 15:00)", "codex@personal is waiting: type to continue" |
| 3 forged id | pass | `--dangerously-bypass-approvals-and-sandbox` and uppercase UUID: real helper writes nothing (report absent), session_id None, limit gives "codex@work: session id not known; not switching accounts", same pid/account |
| 4a resumed, no turn | pass | session.toml id kept (ID); Enter ARGV `...|--base|resume|<ID>|`, no `--last` |
| 4b not-found, w=30 and 35 | pass | prefix-less sentence, exit; id dropped (""), Enter ARGV `...|--base|` only (no resume, no --last), mode New |
| 5 helper hardening | pass with note | outside ai-usage, state root, live-marker + 300 KiB payload (direct `run_codex_notify`): rc 0, nothing written; positive control writes report. Real binary: relative path, `..` path, valid payload with no marker: exit 0, nothing written. A 300 KiB argument cannot be exec'd at all (E2BIG, 128 KiB per-arg limit; shell reports "Argument list too long"), so the size cap was proven in-process |

Observation (not a defect against the rules): real binary with wrong arg counts (`--ai-codex-notify` with 0, 1 or 3+ args) is not taken as the helper form; it falls through to normal CLI parsing and exits 2 with "path does not exist: --ai-codex-notify", writes nothing. Same design as `--ai-statusline` (cli.rs tests `ai_codex_notify_flag_with_exactly_two_args`). Brief expected exit 0; SEC-23 "accepts only its exact form, writes only its own report" holds. Codex always sends the exact form.

## Rule check T7
| Rule | Status | Evidence |
|---|---|---|
| AC-8 | pass | cases 1, 2: id via notify channel, `resume <id>` |
| SEC-32 | pass | cases 2, 3, 4b: no `--last` on switch; refused id starts New |
| SEC-31 | pass | case 4: id kept/dropped only per codex exit message, ids validated |
| SEC-23 | pass | case 5: only exact form, only own report under ai-usage with live marker, mode 0600, payload cap |
| BASE-50 | pass | harness + mod line removed; status/stat identical |
Project files written: crates/vr-ide/src/qc_t7_tests.rs (deleted), one `mod` line in crates/vr-ide/src/ai_switch_tests.rs (removed). Artifacts only under qa-artifacts/T7/.

## QC T8
Engines: real binary `target/debug/vr-ide` (built once via run-heavy), fake `xdg-open`/`open` (records argv + env BROWSER) first on PATH, state/HOME under `qa-artifacts/T8/qc/` (removed afterwards). No Lightpanda/browser/window used; claude/codex never run. Launcher = symlink `<state>/ai-login/bin/vr-ide-login.claude.work` -> the exe, called with `BROWSER=/bin/false` in the environment, DISPLAY and WAYLAND_DISPLAY unset (and, for the second round, `DISPLAY=:99`, a display that does not exist).

| # | Case | Result |
|---|---|---|
| 1a | launcher, one claude start URL (`https://claude.ai/oauth/authorize?redirect_uri=http://localhost:1234/callback&x=1`), no display | exit 1, no stderr text, opener log not created, state tree afterwards = only the launcher link (no `claude-work/`, no lock) -> pass |
| 1b | launcher with 0 args / 2 args | stderr `vr-ide: a login launcher takes exactly one argument`, exit 2, opener not called, nothing written -> pass |
| 2a | `https://github.com/login/device` | exit 0; fake got `ARGC=1 ARG=[https://github.com/login/device] BROWSER=<unset>` (was `/bin/false` in) -> pass |
| 2b | `-https://x.example/--version`, `--help`, `-oProxyCommand=x` | exit 0; fake got ARGC=1 with that exact string, **no `--` separator** -> a real xdg-open would read it as an option. FAIL vs the brief's "not passed as an option" (see defect Q1) |
| 3 | refusals, both with no display and with `DISPLAY=:99`: `https://claude.ai.evil.com/oauth/authorize`, `http://claude.ai/...`, `https://user@claude.ai/...`, `javascript:alert(1)`, `file:///etc/passwd`, `https://clаude.ai/...` (Cyrillic а), `https://xn--clude-9nf.ai/...`, `https://evil.com/oauth/authorize`, start URL with two `redirect_uri` | all: exit 0, handed unchanged to the fake opener (ARGC=1, BROWSER unset), no window attempt, nothing written. Matches D6/D64/D66 (a non-allowed URL is not a login start, so it goes to the system opener, never to the window). Pass |
| 4 | `vr-ide --login-window` with 0, 1, 2 and 4 args; bad tool `../x`, bad account `../../etc` | all: `vr-ide: path does not exist: <cwd>/--login-window`, exit 2, nothing written. Refused, but through the generic "unknown path" error, not a login-specific message (cosmetic, note Q2). Exactly 3 args + valid URL, no display: `vr-ide: login window not opened (no display)`, exit 1, nothing written -> pass |
| 5 | account removal | not reachable from the CLI (no remove flag in `cli.rs`) and `remove` is not public, so no black-box run. Ran the 3 developer tests by name once as the closest evidence: `remove_account_removes_login_data`, `remove_is_refused_while_the_login_window_is_open`, `remove_never_follows_links_in_the_login_data` -> 3 passed. Read their bodies: they assert the web data dir, lock and launcher link are gone, other accounts untouched, symlinked `claude-work`, in-data links and a swapped `ai-login/` are never followed. Pass (code path) |

Fake opener log (all lines, same shape): `ARGC=1`, `ARG=[<url>]`, `BROWSER=<unset>`.

Defects
- Q1 (low): `login::opener_command` does `c.arg(target).env_remove("BROWSER")` with no `--` before the target (login.rs line ~398). An argument that starts with `-` (`--help`, `-o...`) reaches the system opener as an option. The caller is the agent that already controls the command line, so the impact is small, but the brief asked for it not to be an option and D6's "unchanged, one argument" does not conflict with adding `--` (xdg-open, open and `cmd /c start` differ: check each; `xdg-open -- url` is accepted by the script). Repro: run the launcher with `--help`, read the fake's argv. Expected: either `--` first or refusal; actual: `ARG=[--help]`.
- Q2 (cosmetic): `--login-window` with the wrong number of arguments says `path does not exist: .../--login-window`.

Unverified (needs the user's manual check): everything with a real window: the window opening and titles, per-account clean/remembered sign-in, window closes on success, allowlist redirect/`window.open` blocking (SEC-36), page-level `blocked <host>`, the "window is up" byte hand-off and its ~5 s wait, the data under `~/.local/state/vr-ide/ai-login/` having 0700/0600 modes and nothing under `~/.local/share` or `~/.cache`, the remove dialog text; handover steps 1-8.

Project files written: none (only `$TEAM_WS/05_qc_report.md` and the transient `qa-artifacts/T8/qc/`, removed).

## Rule check T8
| Rule | Status | Evidence |
|---|---|---|
| AC-9 | partial: pass at the launcher/CLI, window parts unverified | cases 1-5 above; no-display exit 1 and the URL/paste fallback is claude's own (BROWSER handed over only with a display, D69, not testable from outside) |
| SEC-33 | pass (observable part) | fake opener got no `BROWSER` (case 2a, 3) |
| SEC-34 | pass | exactly one argument (1b), refused counts (4), nothing started or written |
| SEC-35 | pass (launcher side) | lookalike/userinfo/http/scheme URLs never reach a window (3) |
| SEC-36 | unverified | needs the window |
| SEC-37 | pass (code path) | case 5 tests; modes unverified live |
| SEC-38 | not exercised from outside (history seam is unit-tested by the developer) |
| BASE-50 | pass | no repo file edited |
| BASE-14/15/17 | pass | nothing left running; no pkill; no DB |

## QC T8 re-check
Rebuilt once via run-heavy; same fake opener, no display, `BROWSER=/bin/false` in.
- Q1: launcher with `--help`, `-oProxyCommand=x`, `-https://x/--version`, `-`: exit 1 each, fake opener log empty (never called). `https://github.com/login/device`: exit 0, `ARGC=1 ARG=[https://github.com/login/device] BROWSER=<unset>`. Pass.
- Q2: `vr-ide --login-window` with 0/1/2/4 args: `usage: vr-ide --login-window <tool> <account> <url>`, exit 2, state tree afterwards = only the launcher link. Pass.
Project files written: none (qa-artifacts/T8/qc removed).
