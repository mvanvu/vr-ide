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
