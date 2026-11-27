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
