# T7 Codex limits

## Progress
- started: loaded implementation + security-review-checklist skills
- read: threat model SEC-24/SEC-31/SEC-32/T-40, decisions D3..D56, ai/usage.rs, ai/spawn.rs, ai/switch.rs, ai/mod.rs (start_program, usage_files, read_usage, restart), ai_switch_tests.rs, tests/statusline_helper.rs
- investigation (BASE-49: codex not run; `strings -n 6` of the installed binary into qa-artifacts/T7/codex.strings): see ## Investigation

## Investigation (codex 0.144.1)
Binary: `~/.nvm/versions/node/v26.1.0/lib/node_modules/@openai/codex/node_modules/@openai/codex-linux-x64/vendor/x86_64-unknown-linux-musl/bin/codex` (package.json version 0.144.1).
- `notify` is a config key (listed among the config keys in the strings; settable with `-c notify=<TOML>`); its handler is `hooks/src/legacy_notify.rs` in the `codex_hooks` crate.
- The payload strings next to it, in order: `agent-turn-complete`, `thread-id`, `turn-id`, `cwd`, `client`, `input-messages`, `last-assistant-message` — i.e. `{"type":"agent-turn-complete","thread-id":"<uuid>","turn-id":…,"cwd":…,"client":…,"input-messages":[…],"last-assistant-message":…}`, appended by codex as the LAST argv item of the notify program (codex's documented `notify = ["prog", "arg"…]` contract: argv array, program exec'd directly; the hooks engine's `SHELL -lc` path in the same file is for the new `hooks` config, not for `notify`).
- `thread-id` is the conversation id: `codex resume [SESSION_ID]` help text in the binary: "Conversation/session id (UUID) or thread name. UUIDs take precedence if it parses. If omitted, use --last to pick the most recent recorded session". Format: a canonical lowercase UUID (codex ThreadId = UUID, printed lowercase) → `usage::valid_session_id` reused.
- Usage: the notify payload carries no rate limits; codex's own status line items (`five-hour-limit`, `weekly-limit`, `thread-id` in `[tui].status_line`) are drawn by codex itself, no external command → no usage channel. Usage is not shown for codex; limit detection stays screen-based (T5 patterns).
- Not used (SEC-32): `resume --last`, rollout files / `$CODEX_HOME`, screen-scraped ids, OSC/env (none carry the id).
- Limits of the channel: notify fires only after a turn completes (no id before the first answer → "session id not known" notice, no switch); codex may also run notify for sub-agent threads (multi-agent) — then the last reported thread id wins (accepted remainder like T-31: same user, same folder).
Decision: **2a** — reliable id channel exists (`-c notify=[<exe>,"--ai-codex-notify",<report>]`, set up by vr-ide per session).
- red tests written: ai_switch_tests.rs codex_switch_resumes_by_id / codex_forged_thread_id_is_not_resumed / codex_login_screen_target_is_skipped; usage_tests.rs codex_notify_*, codex_channel_names_the_helper_in_argv; cli.rs ai_codex_notify_flag_with_exactly_two_args; spawn.rs resume_by_id (codex); tests/statusline_helper.rs codex_notify_helper_keeps_only_the_thread_id
- red 1 (compile): `cargo test -p vr-ide --lib codex_` → 14× E0425/E0609 (`parse_codex_notify`, `codex_helper`, `prepare_codex`, `MAX_NOTIFY`, `ai_codex_notify`, `SessionFiles.args` missing)
- green (helper side): usage.rs parse_codex_notify / codex_helper / run_codex_notify / prepare_codex / SessionFiles.args; cli.rs ai_codex_notify; main.rs dispatch
- red 2 (behaviour, before wiring codex into start_program / resume_words): `codex_switch_resumes_by_id` FAILED at ai_switch_tests.rs:1103 (`expect("a report path")` — no channel for codex); `codex_forged_thread_id_is_not_resumed` and `codex_login_screen_target_is_skipped` FAILED at :1090 ("no report read"); the 6 helper/cli tests ok
- green: wired codex — ai/mod.rs `usage_files` (claude → `usage::prepare`, codex → `usage::prepare_codex`) and `start_program` splices `SessionFiles.args`; spawn.rs `resume_words("codex") = ["resume"]`; switch.rs `start_screen` reads codex's sign-in screen; `cargo test -p vr-ide --lib codex_` → 9 passed (×3 runs: 0.14 s, 0.13 s, 1.04 s). One test fix: `codex_forged_thread_id_is_not_resumed` read the 5 s info notice after the loop clock had run past it — assertion moved before the extra steps (test timing, not code).
- related run (`run-heavy.sh cargo test -p vr-ide --lib -- ai cli::`): 318 passed, 1 failed `ui::tests::ai_view_tests::label_marks_auto_switch_off` — it asserted "codex does not switch: no mark"; now codex switches (D61) → test updated (codex has ⊘, gemini none) → ok.
- task command `cargo test -p vr-ide` 1st run: 1077 passed, 1 failed `ai_tests::usage_settings_only_for_claude_with_a_usable_path` (ai_tests.rs:1118, gemini ARGV) — rerun alone 3× ok; not mine: pre-existing race in ECHO_AGENT (`printf ARGV:` and the args are two writes; `wait_for("ARGV:")` can see the first only under load); that test's code path is unchanged.
- guard: clippy found `useless_format` in my usage_tests.rs:582 → `.to_string()`; rerun green.
- README.md, decisions D57–D61, capabilities.md updated.
- BASE-51 (leader, mid-task): no red-proof mutation, debug line or other experiment was made after the rule arrived (nor before: red proofs came from test-first order), so no file copies/`cmp` needed.

## Approach: 2a (reliable id channel)
Channel: `-c notify=["<vr-ide exe>","--ai-codex-notify","<state>/ai-usage/<pid>-<n>-<hex>.report"]`, two argv items before the tool's args, TOML built by `toml::Value` (serializer), no shell (codex execs the argv). Files per session: `<stem>.json` (0600 marker holding the notify argv as JSON; the helper writes only while it exists) + `<stem>.report` (0600, temp+fsync+rename), dropped with the session / on restart / on switch (`SessionFiles` Drop), swept like T3's. Helper `vr-ide --ai-codex-notify <report> <json>`: exactly 3 args, first thing in main, same report-path rules as SEC-23 (`open_report_dir`), payload ≤ 256 KiB, typed serde struct with only `type` + `thread-id`, `type == "agent-turn-complete"`, id must pass `valid_session_id` (codex thread ids are canonical lowercase UUIDs); writes `{"session_id":"<uuid>"}` only — no messages, cwd, client; prints nothing; exit 0. vr-ide re-validates on read (SEC-25, unchanged). Switch: T6 machine unchanged except `resume_words` and codex's sign-in screen; args `tool.args + ["resume", id]` via `args_by_id` (validated again at argv). No usage numbers for codex (none in the channel).

## Files changed
- crates/vr-ide/src/ai/usage.rs — codex channel: doc of the finding, `MAX_NOTIFY`, `parse_codex_notify`, `run_codex_notify`, `codex_helper`, `Channel`, `prepare_codex`, `prepare_channel`, `SessionFiles.args`
- crates/vr-ide/src/ai/mod.rs — `start_program` splices `SessionFiles.args`; `usage_files` for claude and codex; codex notice wording
- crates/vr-ide/src/ai/spawn.rs — `resume_words("codex")`; test updated
- crates/vr-ide/src/ai/switch.rs — module doc; `start_screen` codex sign-in; test
- crates/vr-ide/src/cli.rs — `AI_CODEX_NOTIFY_FLAG`, `ai_codex_notify`; test
- crates/vr-ide/src/main.rs — helper dispatch before anything else
- crates/vr-ide/src/ai/usage_tests.rs — `codex_notify_keeps_only_a_valid_thread_id`, `codex_notify_helper_writes_only_its_own_report`, `codex_channel_names_the_helper_in_argv`
- crates/vr-ide/src/ai_switch_tests.rs — `setup_tool` (setup generalised), `CODEX` fake (prints ARGV, honours CODEX_HOME, runs the notify argv like codex), `CODEX_NOTIFY_STAND_IN`, `codex_switch_resumes_by_id`, `codex_forged_thread_id_is_not_resumed`, `codex_login_screen_target_is_skipped`
- crates/vr-ide/tests/statusline_helper.rs — `codex_notify_helper_keeps_only_the_thread_id` (real binary)
- crates/vr-ide/src/ui/tests/ai_view_tests.rs — `label_marks_auto_switch_off` (codex now marked; D61)
- README.md — codex switching, the notify override, its limits
Edit methods: Edit tool for every source change, rustfmt on the changed files only, `cat >>` for appends to notes/decisions/capabilities and the new integration test.

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| id validation | capabilities.md, usage.rs | reused `usage::valid_session_id` (codex thread ids are UUIDs) |
| report path checks, atomic private write, session-bound commit | usage.rs (T3) | reused `open_report_dir`, `commit_report`, `report_json`, `read_report`, `Poll`, `sweep`, `SessionFiles` |
| per-session files in state folder | `usage::prepare` | generalised into `prepare_channel` (claude / codex), not copied |
| switch machine | switch.rs (T6) | reused whole; per-tool parts are `spawn::resume_words` and `start_screen` |
| argv by id | `spawn::args_by_id` | reused unchanged |
| codex limit + sign-in patterns | limits.rs (T5) | reused (`Tool::Codex`, `login_in`) |
| switch test fixture | ai_switch_tests `setup`, `Env` | generalised `setup_tool`; `setup` delegates |

## Final command output (trimmed)
```
$ run-heavy.sh cargo test -p vr-ide   (log: qa-artifacts/T7/cargo-test-vr-ide.log)
test result: ok. 1078 passed; 0 failed; 7 ignored  (lib)
test result: ok. 3 passed  (tests/desktop_cli.rs)
test result: ok. 4 passed  (tests/statusline_helper.rs, incl. codex_notify_helper_keeps_only_the_thread_id)
$ cargo test -p vr-ide --lib codex_
test result: ok. 9 passed; 0 failed
$ cargo fmt --all --check && run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings
FMT-OK
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 15.07s
```

## Out of scope noticed
- `ai_tests.rs` ECHO_AGENT writes `ARGV:` and the args in two printf calls; `wait_for(|s| s.contains("ARGV:"))` can race under load (seen once in `usage_settings_only_for_claude_with_a_usable_path`). Fix would be one printf or waiting for `|` — not changed.
- `/ai usage` on a codex tab says "no usage reported yet for codex@work" (codex never reports usage); a clearer wording could say so.

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| AC-8 | pass | 2a: `codex_switch_resumes_by_id` (ARGV `-c|notify=[…]|--base|resume|<id>|`, no `--last`, notice, nothing typed); finding in usage.rs module doc + ## Investigation; README "Codex sessions switch the same way…" |
| SEC-32 | pass | id only from vr-ide's own notify channel (argv, toml serializer: `codex_channel_names_the_helper_in_argv` with exe `'x$y`;z"]`); `codex_forged_thread_id_is_not_resumed` (id `--dangerously-bypass-approvals-and-sandbox` → no switch); no `resume --last` for a switch (asserted); codex's files never read (grep: no CODEX_HOME/rollout reads added); finding (0.144.1, `-c notify`, `thread-id`) recorded; README states the notify override |
| SEC-24 | pass | `valid_session_id` in helper (`codex_notify_keeps_only_a_valid_thread_id`: option, `-x`, `abc-123`, uppercase, `\n`, empty, wrong type rejected), on read (SEC-25 path), at argv (`args_by_id`, spawn test `resume_by_id_takes_only_a_canonical_uuid` codex rows); `resume`,`<id>` separate items |
| BASE-1 | pass | diff: board files + cli.rs/main.rs (helper entry, required by the channel) + ai_view_tests.rs (behaviour change D61) |
| BASE-2 | pass | SEC-24/32 contracts kept; T3 report format unchanged |
| BASE-3 | pass | red 1 (compile, 14 errors) and red 2 (behaviour: ai_switch_tests.rs:1103/1090) recorded above |
| BASE-4 | pass | ## Final command output |
| BASE-5 | pass | test/clippy runs via run-heavy.sh; nothing left running |
| BASE-6 | pass | English; comments say why |
| BASE-9 | pass | helper input validated at the boundary (argc, path rules, size, typed parse, UUID); no secrets/user text kept or logged |
| BASE-10 | pass | related: `--lib -- ai cli::`, `--test statusline_helper`, the board's `cargo test -p vr-ide`, guard |
| BASE-14 | pass | all runs foreground through run-heavy.sh (timeouts), none outlasted the limit |
| BASE-15 | pass | no process stopped; no pkill |
| BASE-16 | pass | ## Reuse; capabilities.md entry added |
| BASE-17 | pass | nothing installed |
| BASE-18 | pass | no agents started |
| BASE-19 | pass | notes created first, progress lines per step |
| BASE-22 | pass | `--ai-codex-notify` follows `--ai-statusline`; `prepare_codex` beside `prepare` |
| BASE-23 | pass | entry paths: switch (`codex_switch_resumes_by_id`), restart/restore by id (`spawn::again` codex row), helper real binary (integration test) |
| BASE-24 | pass | no git add/commit/stash |
| BASE-25 | pass | guard `cargo fmt --all --check && clippy --workspace --all-targets -D warnings` green |
| BASE-32 | pass | D3, D4, D21, D28–D56 kept (D43 applied to codex unchanged: D60; D48 extended: D61) |
| BASE-38 | pass | rustfmt run on the changed files only |
| BASE-49 | pass | codex not run; flags/fields read from the binary's strings (qa-artifacts/T7/codex.strings) |
| COST-1 | pass | heavy runs via run-heavy.sh; single-filter test runs direct |
| COST-3 | pass | no sleep/pgrep loops |
| COST-4 | pass | only the board's own `cargo test -p vr-ide` (task command), once more for clean evidence |
| COST-8 | pass | Edit tool per file, no python heredocs; no revert made (no `cat -A` needed) |
| COST-9 | pass | nothing left running |
| COST-10 | pass | `grep -rl` of changed names → ai_tests.rs, mod.rs, …; ai_view_tests found by the related run |
| COST-13 | pass | throwaway artifacts only in qa-artifacts/T7/ (codex.strings, test log) |
| COST-16 | pass | final full run written to qa-artifacts/T7/cargo-test-vr-ide.log; context got grep'd lines |

n-a: BASE-7 (no commits), BASE-8 (no dependency change; `toml` already a dependency), BASE-11, BASE-13 (no tree/untracked changes), BASE-12, BASE-28, BASE-29, BASE-30, BASE-31, BASE-33, BASE-40, BASE-41, BASE-43 (no outside system, container, browser or credentials), BASE-20 (nothing removed), BASE-21 (no secrets in output), BASE-26 (no rewrite), BASE-27 (no shared data; tests use temp dirs only), BASE-34 (nothing blocked), BASE-35, BASE-36 (no UI screen/user rule; tab mark covered by existing test), BASE-37 (not a runtime defect fix), BASE-39 (no schema), BASE-42 (no test settings), BASE-44 (not a client task), BASE-45 (AC-8 text unchanged), BASE-46 (no runtime evidence beyond tests), BASE-47 (no computed value changed), BASE-48 (no machine state claimed)
n-a: COST-2, COST-6, COST-7, COST-11, COST-15, COST-17, COST-19, COST-20 (no UI sweep, e2e, goldens or browser), COST-5 (no browser/server), COST-12 (leader), COST-14 (no CI wait), COST-18 (no doomed full run), COST-21 (qc)

## Fix round 1
- read findings 1–3; codex not-found string confirmed in the binary strings: `No saved session found with ID ` (codex 0.144.1)
- red: fake codex gains `quit` and `missing` (prints `Error: No saved session found with ID <id>. …` and exits 1 when resumed with a listed id); new tests `codex_keeps_its_id_when_a_resumed_run_ends_without_a_turn` FAILED at ai_switch_tests.rs:1231 (session.toml id `""`, want ID) and `codex_drops_an_id_it_did_not_find_and_starts_new` FAILED at :1264 (ARGV `…|--base|resume|--last|`)
- green (finding 1): spawn.rs `CODEX_NOT_FOUND`; mod.rs `AiSession::id_refused()` (claude: unconfirmed + exited, as D43; codex: unconfirmed + exited + not-found line on screen, also when wrapped); `restart` uses it and starts codex `New` after a refused id; restore.rs capture filters with `id_refused()`. D62 replaces D60.
- finding 2: README line (128 KiB per-argument limit → no id for that turn) + `MAX_NOTIFY` doc (E2BIG); README line on codex keeping its id across a quit before an answer.
- finding 3: ai_tests.rs `ECHO_AGENT` and `STATUS_AGENT` print the `ARGV:` line in one printf (`printf 'ARGV:%s\n' "$(printf '%s|' "$@")"`), so a wait for `ARGV:` sees the whole line; test code only.
- BASE-51: no red-proof mutation, debug line or experiment in this round (red came from the new tests before the code) → no copies, no `cmp` needed; formatting by rustfmt on the changed files only.
- Edit methods: Edit tool; rustfmt on changed files; `cat >>` for notes/decisions.

### Commands (trimmed)
```
$ cargo test -p vr-ide --lib codex_            (red, before the fix)
test ai_switch_tests::codex_keeps_its_id_when_a_resumed_run_ends_without_a_turn ... FAILED   (:1231 left: "" right: ID)
test ai_switch_tests::codex_drops_an_id_it_did_not_find_and_starts_new ... FAILED            (:1264 ARGV …|--base|resume|--last|)
test result: FAILED. 9 passed; 2 failed
$ run-heavy.sh cargo test -p vr-ide --lib -- ai codex_ usage restore session cli::   (log qa-artifacts/T7/r1/related.log)
test result: ok. 357 passed; 0 failed
$ run-heavy.sh cargo test -p vr-ide --test statusline_helper
test result: ok. 4 passed; 0 failed
$ cargo test -p vr-ide --lib -- codex_ usage_settings_only claude_session_reads_usage   (×3)
test result: ok. 13 passed; 0 failed  (3/3)
$ cargo fmt --all --check && run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings
FMT-OK
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.64s
```

### Files changed (round 1)
crates/vr-ide/src/ai/spawn.rs, crates/vr-ide/src/ai/mod.rs, crates/vr-ide/src/ai/restore.rs, crates/vr-ide/src/ai/usage.rs (doc), crates/vr-ide/src/ai_switch_tests.rs, crates/vr-ide/src/ai_tests.rs, README.md, decisions.md (D62)

### New states (round 1)
- "id refused" is derived, not stored: computed from `switch.unconfirmed`, `running()` and the exited screen. Quit / Esc / no answer → id kept (`codex_keeps_its_id_when_a_resumed_run_ends_without_a_turn`); not-found line → id dropped, next start New (`codex_drops_an_id_it_did_not_find_and_starts_new`); a later run reports → `unconfirmed` cleared (unchanged T6 path). Scrolled back at exit: `screen_text` reads at the scroll position, so the line could be missed and the id kept — next Enter then gets the same not-found line again and drops it (no data loss, no hang).

### Updated rule rows (round 1)
| Rule | Status | Evidence |
|---|---|---|
| SEC-32 | pass | id kept/dropped only on codex's own exit line; never `resume --last` after a dropped id: `codex_drops_an_id_it_did_not_find_and_starts_new` |
| BASE-3 | pass | red above (2 FAILED for the right reasons) before code |
| BASE-4 | pass | commands block above |
| BASE-10 | pass | related set: codex_, ai, usage, restore, session, cli::, statusline_helper |
| BASE-25 | pass | guard green |
| BASE-32 | pass | D62 replaces D60 (leader's finding); D43 kept for claude |
| COST-8 | pass | Edit tool only; no revert, so no `cat -A` check needed |
| COST-16 | pass | related run output in qa-artifacts/T7/r1/related.log |

## Fix round 2
- started (replacement developer): read findings 1–2, Investigation, Fix round 1, `git diff HEAD` of ai/mod.rs `id_refused`, fake codex in ai_switch_tests.rs
- red tests written first: `codex_drops_an_id_it_did_not_find_and_starts_new` now runs over panel widths 10, 16, 24, 30, 35, 80 (layout.ai set before open, so every run's PTY gets that width); new `codex_keeps_its_id_when_only_the_conversation_mentions_not_found`; fake codex flag `tail` (a resumed conversation whose tail prints the not-found line at column 0, then a blank row and its input box)
- red: `cargo test -p vr-ide --lib codex_` → 2 FAILED: `codex_drops_an_id_it_did_not_find_and_starts_new` at ai_switch_tests.rs:1271 `width 10` (session.toml id `0f8e2c1a-…` kept, want `""`); `codex_keeps_its_id_when_only_the_conversation_mentions_not_found` at :1308 (id `""`, want ID). One test fix before red: the narrow width is set after the limit line is read (codex's limit/TURN lines wrap at 10 cols, so the test waits could not see them; limit reading at narrow widths is T5, not touched).
- green: spawn.rs `codex_not_found(screen, id)` + `NOT_FOUND_REST_MAX`; mod.rs `id_refused` calls it with the run's own id (`session_id()`). Finding 1: both sides compared with every blank removed. Finding 2: only the rows after the last blank row (the exit message block), the last `Error:` there, must read `Error:` + CODEX_NOT_FOUND + this run's id, and at most 120 non-blank chars follow (the rest of codex's sentence is 57).
- unit test `ai::spawn::tests::codex_not_found_line_at_every_width_and_only_as_the_exit_message` (widths 6..=200, other id, tail + input box, long trailer, empty).
- BASE-51: copied crates/vr-ide/src/ai/spawn.rs → qa-artifacts/T7/r2/spawn.rs.orig; mutation (block = whole screen instead of rows after the last blank) → unit test FAILED at spawn.rs:384 and the tail test FAILED at ai_switch_tests.rs:1308; restored with `cp` back; `cmp` identical (RESTORED-IDENTICAL).
- decisions.md D62 gains the R2 sentence.

### Commands (round 2, trimmed)
```
$ cargo test -p vr-ide --lib codex_        (red)
test result: FAILED. 10 passed; 2 failed
$ cargo test -p vr-ide --lib codex_        (green, ×3)
test result: ok. 13 passed; 0 failed   (0.62 s, 0.52 s, 0.52 s)
$ cargo fmt --all --check                  → FMT-OK
$ run-heavy.sh cargo test -p vr-ide --lib -- codex_ ai_switch_tests restore session   (log qa-artifacts/T7/r2/related.log)
test result: ok. 104 passed; 0 failed
$ run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.48s
```

### Files changed (round 2)
crates/vr-ide/src/ai/spawn.rs, crates/vr-ide/src/ai/mod.rs, crates/vr-ide/src/ai_switch_tests.rs, .ai-dev-team/workspace/decisions.md (D62)

### Updated rule rows (round 2)
| Rule | Status | Evidence |
|---|---|---|
| AC-8 | pass | codex switch tests 13/13 green ×3 |
| SEC-31 | pass | id dropped only on the exit message naming that run's id; the resume argv still built by `args_by_id` (unchanged) |
| SEC-32 | pass | after a dropped id the next start is New, never `resume --last`, at widths 10/16/24/30/35/80 |
| BASE-3 | pass | 2 FAILED for the right reasons before the fix |
| BASE-51 | pass | one copy, `cp` back, `cmp` identical (above) |
| BASE-25 | pass | fmt + clippy green |
| COST-8 | pass | Edit tool only |

## Fix round 3
- finding: `codex_not_found` anchored on `Error:`, which only the fake printed.
- source check (reading only, BASE-49): the npm package (`@openai/codex` 0.144.1) ships no tui source text, only bin/codex.js and the binary. In the binary's strings (qa-artifacts/T7/codex.strings 584401–584406, tui/src/session_resume.rs area) the pieces are `No saved session found with ID ` / `. Run \`codex ` / `` ` without an ID to choose from existing sessions.`` and no `Error: ` next to them. Whether something prefixes it at print time cannot be seen from the strings, so both forms are handled.
- red (tests first): the fake codex's `missing` now prints the binary's sentence with no prefix; the unit test runs prefixes "", "Error: ", "! " over widths. `cargo test -p vr-ide --lib codex_` → 2 FAILED: `codex_drops_an_id_it_did_not_find_and_starts_new` at ai_switch_tests.rs:1271 `width 10` (id kept); `codex_not_found_line_at_every_width_and_only_as_the_exit_message` at spawn.rs:382. The unit test starts at width 2: at width 1 every space of the sentence becomes a blank row (not a usable panel).
- green: spawn.rs `codex_not_found` drops the `Error:` anchor. It finds the last squeezed `CODEX_NOT_FOUND + id` in the exit block (rows after the last blank one) and keeps the ≤ 120 trailing-character rule. The doc comment says so.
- BASE-51: copied crates/vr-ide/src/ai/spawn.rs → qa-artifacts/T7/r3/spawn.rs.orig. Mutation: block = whole screen. The unit test FAILED at spawn.rs:389 and the tail test FAILED at ai_switch_tests.rs:1308, so finding 2 stays guarded without the anchor. Restored with `cp`; `cmp` identical (RESTORED-IDENTICAL).
- decisions.md D62 says R2/R3 (no `Error:` anchor).

### Commands (round 3, trimmed)
```
$ cargo test -p vr-ide --lib codex_        (red)   → FAILED. 11 passed; 2 failed
$ cargo test -p vr-ide --lib codex_        (green, ×3) → ok. 13 passed; 0 failed (0.74 s, 0.76 s, 0.78 s)
$ cargo fmt --all --check                  → FMT-OK
$ run-heavy.sh cargo test -p vr-ide --lib -- codex_ ai_switch_tests restore session  (log qa-artifacts/T7/r3/related.log) → ok. 104 passed; 0 failed
$ run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings → Finished (clean)
```

### Files changed (round 3)
crates/vr-ide/src/ai/spawn.rs, crates/vr-ide/src/ai_switch_tests.rs (fake codex sentence + doc), .ai-dev-team/workspace/decisions.md (D62)

### Updated rule rows (round 3)
| Rule | Status | Evidence |
|---|---|---|
| SEC-31 | pass | id dropped only on the exit block naming that run's id, with or without a prefix |
| SEC-32 | pass | after a dropped id the next start is New, never `resume --last`, at all six widths with the prefix-less sentence |
| BASE-3 | pass | 2 FAILED before the fix |
| BASE-51 | pass | copy, `cp` back, `cmp` identical |
| BASE-25 | pass | fmt + clippy green |
| COST-8 | pass | Edit tool only |
