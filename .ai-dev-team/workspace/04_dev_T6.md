# T6 Auto-switch

- Task: board row T6 (be), base 61a694b, HEAD 24feba9. Mode implement.
- Progress:
  - read spec/plan/TM/decisions
  - vr-term: Terminal::hang_up/force_kill (non-blocking) + test added (green)
  - tests written: crates/vr-ide/src/ai_switch_tests.rs (16), unit tests in ai/switch.rs (2), ai/spawn.rs (1)
  - RED (behaviour stubbed out: tick/next_account/start_screen/off_mark/command/again/args_by_id return early, `resumed` never set):
    `cargo test -p vr-ide --lib -- ai_switch_tests switch:: resume_by_id` -> `test result: FAILED. 1 passed; 18 failed` (the one passing is the guard `no_switch_at_100_percent_without_limit_screen`, which must hold with and without the feature)
  - stubs removed, implementation in place (switch.rs, mod.rs, spawn.rs, limits.rs, restore.rs, session.rs, slash.rs, ui/ai_view.rs, event_loop.rs, commands/terminal.rs, vr-term)

## Handover (developer 1 -> fresh developer; context limit)

State: implementation complete, tree compiles, every test green, clippy clean, fmt clean at the last run. Nothing staged or committed. Left: the items under "Left to do" (mostly notes/compliance, one verification, optional gaps).

### Design as built
- `crates/vr-ide/src/ai/switch.rs` (new; module doc has the state machine). Per session `AiSession::switch: switch::State { handled, typed, pending: Option<Pending>, times, unconfirmed }` and `AiSession::auto_switch` (from `settings.ai_auto_switch` at spawn; `/ai autoswitch on|off`).
- `switch::tick(app, now)` is called on every loop pass from `event_loop::background_pass`, right after `limits::check`. Nothing blocks the loop.
  1. New limit event (`limit_seen && !handled`) -> `on_limit(app, i, now)`: returns silently when auto-switch off / not running / session not shared / tool has no `spawn::resume_words` (claude only; codex = T7). Then: limit within `SETTLE` 60 s of a switch and the new run has no validated report (`limit_watch.works_marked`) -> warn "claude@a hit its limit right after the switch; not switching again"; no valid id -> info "claude@work: session id not known; not switching accounts"; `SWITCH_MAX` 3 starts in `SWITCH_WINDOW` 30 min used up -> `auto_switch = false` + warn "claude@c: 3 account switches in 30 minutes; auto-switch is off for this session"; else `choose()`.
  2. `choose()`: `next_account` (order: default, then `accounts::for_tool` order — alphabetical — starting after the current, wrapping; skips current, `tried`, not `works`, `limited_until > now`), then per candidate `share_history` (history::ensure must be Linked), `accounts::hold` (named only), `accounts::ready`; a failing candidate goes into `tried` and the next is taken. None -> `none_left`: "all claude accounts are limited until 15:00" (earliest future `limited_until` of all the tool's accounts, the current one included) or "no other claude account to switch to"; silent when the tool has no named account at all.
  3. Phase `Ending { to, lock, since, killed }`: `TermTab::hang_up()` (SIGHUP to the group, no wait). Each pass: old run exited -> `start()`; after `END_WAIT` 5 s -> `force_kill()` (SIGKILL group); `KILL_WAIT` 2 s later still running -> switch given up (target hold dropped, warn "… did not end; not switched").
  4. `start()`: `start_program(.., spawn::Start::Id(id), ..)` -> argv `tool.args + ["--resume", id]` built by `spawn::args_by_id`, which re-validates the id (`usage::valid_session_id`, SEC-24). Ok: `tab.replace` (old run has exited), `account` + `account_lock` set together (old hold dropped here, i.e. after exit), `shared = true`, `mode = Continue`, `usage = None` then id kept via `started(Start::Id)`, `high_warned` reset (D36), usage files swapped, `limit_seen = false`, `limit_watch.restarted()`, `resumed = true`, `times.push(now)`, warn "switched claude from work to personal (work limited until 15:00)" (without the bracket when no reset is known). Err: both holds dropped, tab stays exited, error "AI: switch to claude@personal failed: …", no further attempt.
  5. Phase `Starting { pid, since, read }`: run exited or pid changed -> state dropped, nothing typed. After each quiet second with new output the screen is read once (`start_screen`): `Login` -> account `works = false`, warn "claude@personal is not logged in; trying the next account", `choose()` again with the same `tried`, hang up, back to `Ending` (counts toward the 3); `Dialog` -> warn "claude@personal is waiting: type to continue"; `Ready` -> unless `switch.typed`, writes `encode_paste(ai_auto_switch_prompt, modes) + "\r"` to that session's own tab (setting re-checked: 1..=200 chars, no control char); `PROMPT_WAIT` 30 s without a ready box -> the "waiting" notice (silent when the user typed).
- Lock order: target's history link -> target hold (shared flock, non-blocking) -> `ready` -> hang up old run -> wait for exit -> start new -> swap `account`/`account_lock` (old hold released). The state-file lock is only taken inside `limits::update_state` / `load_state` calls, never while waiting. No blocking lock anywhere.
- `switch.typed`: set by `send_typed` (keys, paste), `forward_mouse` (non-empty mouse report), and by `handle_key` / `paste` while a switch is busy (also when the run has already exited); reset when a limit event starts.
- `start_screen` (SEC-30 / D8): Ready = a prompt row (`> ❯ ›`) directly under a rule with a rule within 4 rows below, among the last 12 non-empty rows, and no dialog line there (`CLAUDE_DIALOGS` prefixes, or a selected numbered choice `❯ 1. …`). Other tools: never ready.
- `limits::Watch::resumed` (new): a run started on an earlier conversation (mode != New, or by id) shows its tail again; a limit line there is not an event until a line (CR/LF) was sent to the run, or after any input the limit line is gone. Set in `AiSession::started`. Without it every switch target would at once count the replayed limit line of the old account as its own limit (wrong `limited_until`, switch storm), and every restored session that ended on a limit would be switched at restore.
- Restart / restore by id (D5, SEC-31): `spawn::again(tool, id)` -> `Start::Id` when the tool continues, resumes by id and the id is valid; `restart()` and `restore::restore` use it; `session::AiSession::session_id` (written only when non-empty, cleared on load unless a canonical UUID); `restore::capture` stores it. `switch.unconfirmed`: a run started by id that has not reported yet; if it exits like that (claude: "No conversation found"), the next Enter gives the id up and uses `continue_args`, and capture does not store it. `restart()` is refused while a switch is busy and re-takes the account hold when a failed switch dropped it.
- Tab mark: `ui/ai_view.rs` `Item.off` -> `OFF_MARK` (" ⊘") after the label, before the usage suffix, dropped together with the suffix when the bar is short; only for a tool that can switch. `AiPanel::label` stays plain (D33).

### Files touched
- new: `crates/vr-ide/src/ai/switch.rs`, `crates/vr-ide/src/ai_switch_tests.rs`
- `crates/vr-ide/src/ai/mod.rs` (fields, `spawn_started`, `session_id()`, `started()`, `start_program(.., Start, ..)`, `restart`, typed marks, read_usage confirms), `ai/spawn.rs` (`Start`, `resume_words`, `args_by_id`, `again` + test), `ai/restore.rs`, `session.rs`, `commands/slash.rs` (`/ai autoswitch`, description reworded so the palette detail keeps 2 rows — no snapshot changed), `docs/commands.md`, `README.md`
- outside the board row, each needed:
  - `crates/vr-term/src/terminal.rs` + `tests/pty.rs`: `Terminal::hang_up()` / `force_kill()` — non-blocking SIGHUP / SIGKILL of the child's group. Why: SEC-28 needs "hang up, wait ≤ 5 s for the exit, then kill" without blocking the UI loop; the only existing API, `kill()`, blocks and gives SIGHUP just `KILL_GRACE` 300 ms, and signalling by pid from vr-ide would lose vr-term's reaped-pid guard (`signal_group` holds the status lock).
  - `commands/terminal.rs`: `TermTab::{hang_up, force_kill}` (wrappers).
  - `event_loop.rs`: one call `switch::tick` (as T3/T5 added theirs).
  - `ai/limits.rs`: visibility (`pub(super)` on `Tool`, `Screen`, `strip`, `is_prompt`, `is_rule`, `login_in`), `Watch::{resumed, quiet, last_output}`, two lines in `act` and one in `typed_input` for `resumed`.
  - `ui/ai_view.rs` + `ui/tests/ai_view_tests.rs`: the off mark.
  - `ai_tests.rs` (helpers `pub(crate)`, one struct literal), `session_tests.rs` (literal + new test), `lib.rs` (test module), `.ai-dev-team/capabilities.md`.

### Tests (all green at the last run)
- `ai_switch_tests`: switches_to_next_working_account_and_resumes, switch_resumes_own_session_id_with_two_tabs, no_switch_at_100_percent_without_limit_screen, no_continue_prompt_after_user_input, all_limited_notifies_and_keeps_session, no_session_id_notifies_only (also the SEC-24 bad ids), autoswitch_off_per_session (command and setting), login_screen_target_is_skipped, the_old_run_ends_before_the_new_one_starts, a_run_ignoring_the_hang_up_is_killed_after_five_seconds, a_target_that_cannot_be_held_is_skipped_and_a_failed_start_leaves_no_lock, three_switches_in_thirty_minutes_then_notice_only, the_prompt_goes_only_to_a_ready_input_box_of_that_session, a_replayed_limit_line_is_no_event_before_input, restart_resumes_by_the_known_id_and_falls_back_after_a_failed_resume, restore_resumes_each_tab_by_its_stored_id
- `ai::switch::tests`: next_account_order_and_skips, only_the_idle_input_box_is_ready · `ai::spawn::tests::resume_by_id_takes_only_a_canonical_uuid` · `session_tests::the_ai_session_id_is_kept_only_as_a_canonical_uuid` · `ui::tests::ai_view_tests::label_marks_auto_switch_off` · vr-term `pty::hang_up_and_force_kill_signal_the_group_without_waiting`
- Red: see the progress lines above (`FAILED. 1 passed; 18 failed` with the behaviour stubbed). Added after the code and checked by mutation (both went red, mutations reverted): the session-id test (sanitize loop disabled) and the re-hold assertion of the failed-start test. Not seen red: the vr-term test (new API) and `label_marks_auto_switch_off` (it failed first on a real overflow at width 16, which is why the mark now gives way with the suffix).
- Last green lines:
  - `$TEAM_BIN/run-heavy.sh cargo test -p vr-ide -p vr-term` -> vr-ide lib `test result: ok. 1064 passed; 0 failed; 7 ignored`, desktop_cli 3, statusline_helper 3, vr-term lib 21, attention 3, env 1, flood 1 (1 ignored), leak 1, marker 6, pty 22 — all ok. (Run before the slash description rewording and the last two test additions; after them: `cargo test -p vr-ide --lib -- slash` -> `84 passed`, `ai_switch_tests ai_limit` -> `23 passed` six times in a row, and `--test-threads=1 ai_switch_tests` -> `16 passed`.)
  - `$TEAM_BIN/run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings` -> `Finished` with no warning (run before the last test-only additions and the slash wording edit).
  - `cargo fmt --all --check` -> no output (same point in time).

### Left to do (exact next steps)
1. Rerun the guard on the final tree: `cargo fmt --all --check && $TEAM_BIN/run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings`, then `cargo test -p vr-ide --lib` (or the related filters `switch:: ai_ session restore slash label_`) and paste the output here.
2. A background command was started just before this handover and may still be running or have finished: task id `botku5sae` (`strings` of the newest `~/.local/share/claude/versions/*` binary into the scratchpad file `claude.strings`, then counts of the dialog phrases). It only reads the binary, never runs it (BASE-49). Its output: `/tmp/claude-1000/-home-rainy-DEV-vr-ide/acc263d3-c75e-46fa-b257-e9ca3b386c8d/tasks/botku5sae.output`. I did not see its result. Use it to confirm or correct `CLAUDE_DIALOGS` in switch.rs (phrases with count 0 are guesses from memory of claude's UI); stop the task if it still runs.
3. Unverified, say so in the report: the shape of claude's idle input box (prompt row between two rules / in a rounded box) and the dialog phrases were written from knowledge of claude's UI, not captured from a run; claude replaying the old limit line on `--resume` and exiting with an error for an unknown id are assumptions as well. A wrong guess fails safe: nothing is typed and the "type to continue" notice appears after 30 s.
4. Write the remaining notes sections: `## Reuse` table, `D? (T6)` list (below), `## Rule compliance` with every BASE (1–49) and COST id once, the AC-6/AC-7 and SEC-24/28/29/30/31/39 rows (tests above map to them), guard output. Points to disclose there:
   - BASE-38: `cargo fmt -p vr-ide -p vr-term` was run once (crate-wide, not per file); `git status` showed only files of this task changed.
   - COST-8: two attempts were stopped by the hook before anything was written (a python heredoc edit of vr-term, a `sed -i` over several files); mechanical one-file `sed -i` edits were used after that, the rest through Edit.
   - BASE-10: the whole `cargo test -p vr-ide -p vr-term` ran once (board Verify column; the change touches the event loop, TermTab and vr-term).
   - BASE-3: see "Not seen red" above.
5. Optional gaps a reviewer may ask for: a test for the 7 s give-up path (`… did not end; not switched`; hard to fake — SIGKILL cannot be ignored), a test that a mouse report blocks the prompt, `/ai autoswitch` tab completion (not built), the SEC-39 "20 idle sessions" seam is T3's and untouched.

### Decisions so far — D? (T6), for the leader to number
- D?a: the switch never blocks the loop: non-blocking hang-up, exit watched on later passes; 5 s then SIGKILL, 2 s more then given up. Needs `Terminal::hang_up/force_kill` in vr-term.
- D?b: order is history link -> hold -> ready (the link first, as `spawn_session` does: making it needs the account exclusively), not SEC-28's literal hold -> ready -> ensure.
- D?c: a resumed run's limit line is no event until a line was sent to it (`Watch::resumed`); applies to every resumed start (switch, restart, restore, picker Continue/Resume), a change to T5 behaviour.
- D?d: a start by id that exits without a report is not retried by id (falls back to `continue_args`), and that id is not written to the session file.
- D?e: switch and "type to continue" notices are warnings (stay until dismissed: the user is probably away); "session id not known" is info.
- D?f: a tool with only its default account gets no second notice at a limit; with accounts but none usable and no reset known: "no other claude account to switch to".
- D?g: every start by a switch counts toward 3 per 30 min, login-screen retries included; `/ai autoswitch on` resets the count.
- D?h: a dialog on the new run gives the notice at once (not after 30 s).
- D?i: the off mark is drawn in the tab bar only, gives way with the usage suffix, and only for a tool that can switch; glyph `⊘`.
- D?j: `/ai` description reworded ("(e.g. claude)" dropped) so the palette's detail area keeps two rows and no snapshot changes.
- D?k: restore/restart use the id only when the tool has `continue_args` (it replaces them); a tool configured without them starts new as before.
- D?l: on a switch the old account's usage windows are dropped from the session (the tab shows no percent until the new account reports); the id is kept.

### Addendum to step 2 (the background task finished, exit 0; nothing is running)
Counts of each phrase in `strings -n 8` of claude 2.1.332 (binary read, never run):
- found: "Is this a project you created or one you trust" 2 · "Yes, I trust this folder" 2 · "Select login method" 2 · "Choose the text style" 2 · "Let's get started" 2 · "Enter to confirm" 3 · "Detected a custom API key" 2 · "Do you want to use this API key" 2 · "Resume from summary" 2 · "Resume full session" 2 · "Browser didn't open" 13 · "Paste code here" 10 · "Do you want to proceed" 7 · "No conversation found with session ID" 4 (supports D?d) · "for shortcuts" 5
- not found (0): "Do you trust the files in this folder" (older wording; harmless extra entry), "Yes, proceed", "Press Enter to continue", "Resume Session", "Resuming conversation"
- found and NOT yet in `CLAUDE_DIALOGS` — next developer: add these prefixes (and a row to `only_the_idle_input_box_is_ready`): "Quick safety check" 4, "Accessing workspace" 2, "Security notes" 2, "WARNING: Claude Code running in Bypass Permissions mode" 2, "Yes, I accept" 2. The trust dialog of this version is therefore covered today only by "Is this a project you created or one you trust", "Yes, I trust" and the selected-choice rule; the test agent's trust screen uses the old wording "Do you trust the files in this folder?" — switch it to the current one.
- Still unverified: the layout of the idle input box and whether a dialog is drawn with a `❯ 1.` choice row (strings give wording, not layout).
- Continuation (developer 2): read notes + diff stat
  - item 1 RED: `only_the_idle_input_box_is_ready` (new 2.1.332 dialog rows) -> FAILED `left: Ready right: Dialog` on "Quick safety check"; new `no_continue_prompt_while_a_dialog_is_on_screen` -> FAILED, log `"got personal Continue where you left off."`
  - item 1 GREEN: CLAUDE_DIALOGS updated + `dialog_text` (border title, choice mark, leading count) -> 4 passed
  - item 2: new `a_run_that_outlives_the_kill_is_not_switched` (fake tab swapped in after the hang-up) green; mutation `KILL_WAIT * 100` -> FAILED at ai_switch_tests.rs:614, reverted
  - BASE-3 mutations (reverted): ai_view.rs:254 `if false && it.off` -> label_marks_auto_switch_off FAILED (ai_view_tests.rs:707); terminal.rs hang_up `Signal::CONT` -> pty hang_up_and_force_kill FAILED (pty.rs:336)
  - item 2: /ai autoswitch on/off completion: RED completion_tests::ai_autoswitch_completes_on_and_off (left "/ai autoswitch of"), GREEN completion_tests 23 passed
  - final run started in background (fmt check, clippy, related vr-ide tests, vr-term) -> .ai-dev-team/workspace/qa-artifacts/T6/final-run.log

## Continuation (developer 2)

### Done
1. `CLAUDE_DIALOGS` (crates/vr-ide/src/ai/switch.rs) updated from `strings` of claude 2.1.332 (binary read only, never run — BASE-49). Added: "Accessing workspace:", "Quick safety check", "No, exit", "Security notes:", "WARNING: Claude Code running in Bypass Permissions mode", "In Bypass Permissions mode", "Yes, I accept", "New MCP server found in this project", "new MCP servers found in this project", "Allow external CLAUDE.md file imports?", "Use Claude Code's terminal setup?". Older entries kept (fail safe). New `dialog_text(row)`: matching ignores a border/title drawn into a box edge (`╭─ Accessing workspace: ─╮`, the TrustDialog renders "Accessing workspace:" as a box title), a choice mark (`❯ Yes, I accept` — the settings-trust list uses `hideIndexes`, so `is_choice` alone would miss it) and a leading count (`3 new MCP servers …`). `limits::PROMPTS` became `pub(super)` (reused, no copy).
   Not added on purpose: "Press Enter to …" (44 hits, several are hints shown while claude works next to its input box — would block the prompt forever), "Welcome to Claude Code" (banner of a normal start).
   Test agent: the `trust` accounts now print `dialog.txt` above the idle input box (worst case: a box is on screen too); `TRUST` const = the 2.1.332 trust dialog. New helper `no_prompt_under(dialog, seen)`.
2. Give-up path: `a_run_that_outlives_the_kill_is_not_switched` (after the hang-up the tab is swapped for a fake Running tab whose kill does nothing; 5 s + 1.9 s still busy, +0.1 s given up, notice once, account unchanged, target's hold free — `hold_exclusive` succeeds — no second attempt after 60 s). `/ai autoswitch ` tab completion of `on`/`off` (commands/completion.rs) + docs/commands.md line 12. Skipped: a test that a mouse report blocks the prompt (same `switch.typed` flag as keys, covered by `no_continue_prompt_after_user_input`); SEC-39 "20 idle sessions" seam is T3's and untouched.
3. Rerun on the final tree — below.

### Files changed in this round
crates/vr-ide/src/ai/switch.rs, crates/vr-ide/src/ai/limits.rs (PROMPTS visibility), crates/vr-ide/src/ai_switch_tests.rs, crates/vr-ide/src/commands/completion.rs, crates/vr-ide/src/completion_tests.rs, docs/commands.md, .ai-dev-team/capabilities.md (line 136), $TEAM_WS/decisions.md (D? (T6) lines).

### Red / green (this round)
- `cargo test -p vr-ide --lib -- switch::tests::only_the_idle` -> `assertion left == right failed:  Quick safety check: Is this a project you created or one you trust?  left: Ready right: Dialog` -> after code `ok`.
- `ai_switch_tests::no_continue_prompt_while_a_dialog_is_on_screen` -> FAILED, log `["start work", "got work limit", "start personal", "got personal Continue where you left off."]` -> after code `ok`.
- `completion_tests::ai_autoswitch_completes_on_and_off` -> FAILED `left: "/ai autoswitch of" right: "/ai autoswitch off"` -> after code `completion_tests 23 passed`.
- Mutations (each reverted, `git diff --stat` of the files unchanged afterwards): `>= KILL_WAIT * 100` -> `a_run_that_outlives_the_kill_is_not_switched` panicked at ai_switch_tests.rs:614; ui/ai_view.rs:254 `if false && it.off` -> `label_marks_auto_switch_off` panicked at ai_view_tests.rs:707; vr-term `hang_up` sending `Signal::CONT` -> `pty::hang_up_and_force_kill_signal_the_group_without_waiting` panicked at pty.rs:336. These close the "not seen red" items of developer 1.

### Final run (full log: .ai-dev-team/workspace/qa-artifacts/T6/final-run.log)
```
== fmt
fmt: no diff
== clippy   ($TEAM_BIN/run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.04s
clippy exit 0
== vr-ide related   ($TEAM_BIN/run-heavy.sh cargo test -p vr-ide --lib -- switch:: ai_switch_tests ai_limit ai_ session restore slash ai_view completion_tests)
test result: ok. 261 passed; 0 failed; 1 ignored; 0 measured; 812 filtered out; finished in 2.22s
== vr-term   ($TEAM_BIN/run-heavy.sh cargo test -p vr-term)
lib 21 ok · attention 3 ok · env 1 ok · flood 1 ok (1 ignored) · leak 1 ok · marker 6 ok · pty 22 ok · doc 0
vr-term exit 0
```
All 18 `ai_switch_tests`, both `ai::switch::tests`, `completion_tests::ai_autoswitch_completes_on_and_off` listed `ok` in the log.

### Still unverified (say so to the leader)
The layout of claude's idle input box and whether a dialog sits over it or replaces it come from knowledge of claude's UI and the binary's strings, not from a captured screen (claude is never run, BASE-49). Replaying the old limit line on `--resume` is an assumption; "No conversation found with session ID" exists in the binary (supports D?d). Every wrong guess fails safe: nothing is typed, the "type to continue" notice appears.

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| choice-mark characters for dialog matching | limits.rs `PROMPTS` | reused (made `pub(super)`), no copy |
| a run that survives SIGKILL for the give-up test | commands/terminal.rs `TermTab::fake` (used by ai_changes_tests) | reused: fake Running tab swapped in |
| dialog screens in the switch tests | ai_switch_tests `AGENT`, `Env::flag` | extended: `dialog.txt` + `Env::dialog`, `no_prompt_under` |
| completion of a fixed word list after `/ai <word> ` | completion.rs `/ai <tool> @` branch, `Cand`, `found` | same pattern, new branch |
| switch state check in tests | `switch::State::busy()` | reused instead of opening `pending` |
| hold/lock check | `accounts::hold_exclusive` | reused |
| (developer 1) history link, hold, ready, encode_paste, usage id validation, account_name / reset_clock | ai/accounts.rs, ai/history.rs, ai/usage.rs, limits.rs, capabilities.md | reused as listed in the Handover |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| AC-6 | pass | ai_switch_tests::switches_to_next_working_account_and_resumes, switch_resumes_own_session_id_with_two_tabs, no_continue_prompt_after_user_input (final-run.log ok) |
| AC-7 | pass | no_session_id_notifies_only, all_limited_notifies_and_keeps_session, autoswitch_off_per_session, login_screen_target_is_skipped |
| SEC-24 (use) | pass | spawn::tests::resume_by_id_takes_only_a_canonical_uuid; `spawn::args_by_id` re-validates; no_session_id_notifies_only (bad ids); session_tests::the_ai_session_id_is_kept_only_as_a_canonical_uuid |
| SEC-28 | pass | the_old_run_ends_before_the_new_one_starts, a_run_ignoring_the_hang_up_is_killed_after_five_seconds, a_run_that_outlives_the_kill_is_not_switched (new), a_target_that_cannot_be_held_is_skipped_and_a_failed_start_leaves_no_lock; vr-term pty::hang_up_and_force_kill_signal_the_group_without_waiting |
| SEC-29 | pass | three_switches_in_thirty_minutes_then_notice_only, a_replayed_limit_line_is_no_event_before_input, no_switch_at_100_percent_without_limit_screen; give-up makes no second attempt (new test, +60 s) |
| SEC-30 | pass | switch::tests::only_the_idle_input_box_is_ready (13 rows of 2.1.332 dialogs), no_continue_prompt_while_a_dialog_is_on_screen (8 dialogs, new), the_prompt_goes_only_to_a_ready_input_box_of_that_session (2.1.332 trust screen), no_continue_prompt_after_user_input |
| SEC-31 | pass | restart_resumes_by_the_known_id_and_falls_back_after_a_failed_resume, restore_resumes_each_tab_by_its_stored_id |
| SEC-39 (T6 part) | pass | restarts bounded by SEC-29 (three_switches_… test); `tick` does no blocking work and reads a screen once per quiet second (switch.rs `watch`); the 20-idle-sessions seam is T3's |
| BASE-1 | pass | this round: switch.rs, limits.rs (1 line), ai_switch_tests.rs, completion.rs (+test), docs/commands.md; outside-row files of developer 1 justified in the Handover |
| BASE-2 | pass | `/ai autoswitch on|off`, argv `--resume <id>`, notice texts as in the plan/decisions; no contract changed |
| BASE-3 | pass | red runs above (this round) and in developer 1's progress lines; tests added after code were made red by mutation (vr-term pty, label_marks_auto_switch_off, give-up, session id, re-hold) |
| BASE-4 | pass | "Final run" block above |
| BASE-5 | pass | clippy and test runs through run-heavy.sh; nothing left running |
| BASE-6 | pass | code, comments, docs and notes in English; comments state why (e.g. switch.rs `dialog_text` doc), no bug history |
| BASE-9 | pass | session id re-validated at argv (SEC-24); prompt setting re-checked (1..=200 chars, no control char) before typing; no secrets logged |
| BASE-10 | pass (disclosed) | this round only related filters + vr-term; developer 1 ran the whole `cargo test -p vr-ide -p vr-term` once (change touches event loop, TermTab, vr-term) |
| BASE-14 | pass | the final run went as background task br44ro4ym, turn ended, no polling |
| BASE-15 | pass | no process stopped; the test's swapped-out real run is killed by the test itself (`real.force_kill()`) |
| BASE-16 | pass | `## Reuse` table |
| BASE-17 | pass | nothing installed |
| BASE-18 | pass | no agents started |
| BASE-19 | pass | progress lines in this file for each step |
| BASE-22 | pass | test names follow neighbours (`ai_completes_…`, sentence-style switch tests) |
| BASE-23 | pass | entry paths: limit event, `/ai autoswitch` command + setting, restart, restore — each tested |
| BASE-24 | pass | no git index/history command; `git status --short` only |
| BASE-25 | pass | `cargo fmt --all --check && clippy --workspace --all-targets -D warnings` on the final tree (final-run.log) |
| BASE-32 | pass | D3/D4/D5/D8/D18/D19–D39 kept: D36 (`high_warned` reset on switch), D33 (label plain), D8 (prompt only into ready box), D5 (resume by id) |
| BASE-38 | pass (disclosed) | this round: `rustfmt --edition 2024 crates/vr-ide/src/ai_switch_tests.rs` only; developer 1 ran `cargo fmt -p vr-ide -p vr-term` once crate-wide, `git status` then showed only this task's files |
| BASE-45 | pass | AC-6/AC-7 text in 01_spec.md lines 18–19 |
| BASE-49 | pass | claude read with `strings`/`grep -a` only, never run |
| COST-1 | pass | heavy runs via run-heavy.sh |
| COST-2 | pass | only own related tests |
| COST-3 | pass | no sleep/pgrep loops |
| COST-8 | pass (disclosed) | developer 1: two attempts stopped by the hook before writing (python heredoc edit, multi-file `sed -i`); this round: one empty no-op `python3 -I -` heredoc was issued and did nothing (no file written), then all code edits via Edit, mutations/one-line visibility via single-file `sed -i` |
| COST-9 | pass | nothing left running (br44ro4ym completed; botku5sae completed earlier) |
| COST-10 | pass | `grep -rl` of changed files: completion_tests (completion.rs), ai_view_tests, session_tests, slash tests, ai_tests in the filter set |
| COST-13 | pass | log in $TEAM_WS/qa-artifacts/T6/; nothing in $TEAM_TOOLS |
| COST-16 | pass | full output in qa-artifacts/T6/final-run.log, only summaries read |

n-a: BASE-7 (no commit) · BASE-8 (no dependency change) · BASE-11, BASE-13 (no stash/checkout/deletion of untracked files) · BASE-12 (nothing written outside the repo except scratchpad/qa-artifacts) · BASE-20 (nothing removed) · BASE-21 (no secret in output) · BASE-26 (no rewrite) · BASE-27, BASE-30, BASE-31 (no shared database, demo or container) · BASE-28 (no third-party code) · BASE-29, BASE-40, BASE-41, BASE-43 (no browser, outward write, credential or access rule) · BASE-33 (no remote environment) · BASE-34 (no blocked step) · BASE-35 (no USER-n line for this area) · BASE-36 (desktop app scope unchanged) · BASE-37 (not a runtime-defect fix) · BASE-39 (no schema) · BASE-42 (no test-runner settings changed) · BASE-44 (no client fake adapter) · BASE-46 (no runtime walk; unit/integration evidence on the final tree) · BASE-47 (no computed value changed) · BASE-48 (no machine state claimed) · COST-4 (no full suite) · COST-5, COST-7 (no browser/server) · COST-6 (no audit, no UI sweep: tab-bar mark covered by ai_view tests) · COST-11 (no e2e) · COST-12 (leader rule) · COST-14 (no CI) · COST-15, COST-17 (no goldens/snapshots changed) · COST-18 (no full run) · COST-19, COST-20, COST-21 (qc rules)

Nothing left running.

## Fix round 1
- read findings 1-3
- F1 RED: new ai_switch_tests::a_replayed_limit_line_after_the_resume_list_is_no_event -> panicked 'the earlier run's line' (limit_seen true after the pick). (A first draft whose tail held a '> earlier question' row passed before the fix — that row hides the limit line from the detector; dropped so the test shows the bug.) GREEN: Watch::picking (first line of a Resume start picks, does not end resumed) + dropped the 'hit.is_none() && typed' clearing -> ai_switch_tests ai_limit limits 52 passed

## Fix round 1
Files: crates/vr-ide/src/ai/limits.rs (`Watch::picking`, `typed_input`, dropped clearing in `act`), crates/vr-ide/src/ai/mod.rs (`started` sets `picking` for Resume), crates/vr-ide/src/ai/switch.rs (`State::ending`, `tick`, `watch` read order), crates/vr-ide/src/ai_switch_tests.rs (3 tests, `long` flag in AGENT), README.md (line 491).

| # | Finding | Reply | Test |
|---|---|---|---|
| 1 | Resume picker clears `resumed` | fixed: `Watch::picking` — the first submitted line of a Resume start only ends `picking`; the `found.hit.is_none() && typed` clearing removed (the arrow-key path) | `a_replayed_limit_line_after_the_resume_list_is_no_event`: list, Down/Up + quiet read, Enter, tail ends on the limit line -> no limit_seen, state file empty, no notice; then the user's line + fresh limit -> limit_seen |
| 2 | limit during `Starting` used up silently | fixed: `tick` no longer marks an event handled during `Ending`; during `Starting` it drops the start watch and calls `on_limit` | `a_limit_while_the_new_run_starts_is_acted_on`: user types `limit` into the new run before the prompt -> "claude@personal hit its limit right after the switch; not switching again" once, nothing pending |
| 3 | `read` set before the scroll check | fixed: `*read` is set only after the scroll check | `a_screen_skipped_while_scrolled_back_is_read_again` (new `long` agent flag, 60 lines) |

Red / green:
- F1 RED: panicked `the earlier run's line` (limit_seen true after the pick). A first draft whose tail held a `> earlier question` row passed before the fix (that row keeps the limit line from being read as claude's); the row was dropped so the test shows the bug. GREEN after the fix. Arrow-key path checked by mutation: putting the old `hit.is_none() && typed` clearing back -> the test fails at ai_switch_tests.rs:960 `the earlier run's line`; reverted.
- F2 RED: `assertion left == right failed: ["claude@work hit its usage limit (resets 15:00)", "switched claude from work to personal (work limited until 15:00)"] left: 0 right: 1` with `limit_seen` true (event used up). GREEN after the fix.
- F3: NOT seen red, and I could not make it red. Mutation (setting `*read` inside the scrolled branch again) still passes, because `limits::check` runs first in the same pass and, while the screen is scrolled back, re-arms `output_at = now` (limits.rs "Scrolled back … look again a while later"); `read != Some(out)` then holds once back at the bottom. The skip in `watch` can only see a scrolled screen in a pass where `limits::check` has also seen it, so the stuck case is not reachable today; the fix keeps `watch` correct on its own (it no longer depends on that coupling). The test stays as behaviour evidence. Its first version timed out for a test reason (the `ACCT:` line had scrolled off, so `switched()` waited forever); it now waits for the account and `line 60`.

Guard and related (full log .ai-dev-team/workspace/qa-artifacts/T6/r1-final.log):
```
fmt: no diff   (cargo fmt --all --check; rustfmt run only on ai_switch_tests.rs, switch.rs, limits.rs, mod.rs)
run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings -> Finished, clippy exit 0
run-heavy.sh cargo test -p vr-ide --lib -- switch:: ai_switch_tests ai_limit ai_ limits README
test result: ok. 154 passed; 0 failed; 0 ignored; 0 measured; 923 filtered out
```
Flake check: `a_screen_skipped_…` 4 runs in a row ok; F1/F2 tests ran together repeatedly ok.

Updated rule rows:
- SEC-29 / AC-6: pass — plus a_limit_while_the_new_run_starts_is_acted_on, a_replayed_limit_line_after_the_resume_list_is_no_event.
- SEC-30: pass — plus a_screen_skipped_while_scrolled_back_is_read_again.
- BASE-3: pass for F1, F2 (seen red); F3 disclosed above (not reachable red, mutation-proof impossible).
- BASE-25 / BASE-4: r1-final.log above.
- BASE-38: rustfmt only on the four files changed this round.
- COST-8 (disclosed): one Bash call (a `sed -i` on switch.rs inside a test loop) was stopped by the hook before it ran; nothing written. Temporary `eprintln!` debug lines and two mutations were added with Edit / one-file `sed` and removed (`grep -c DBG` = 0, `MUTATION` gone).
- COST-9: nothing left running (b5dg78rbs finished).
D? lines: two `D? (T6 R1)` lines appended to decisions.md.

## Fix round 2
- read findings 1-2
Files: crates/vr-ide/src/ai/switch.rs (line 500: `return false;` back on its own line), crates/vr-ide/src/ai/limits.rs (`typed_input`: empty line), crates/vr-ide/src/ai_switch_tests.rs (new `the_switch_alone_never_reads_a_scrolled_back_screen`; picker test: second Enter, agent ignores empty lines), README.md line 491.

| # | Finding | Reply | Test |
|---|---|---|---|
| 1 | scroll guard swallowed by a comment (switch.rs:500) | fixed. Cause: my round-1 revert of the `*read` mutation used an Edit whose old text began with the newline, which joined `return false;` onto the comment line; the round-1 test could not see it because `limits::check` re-arms the quiet spell while scrolled | `the_switch_alone_never_reads_a_scrolled_back_screen` calls `switch::tick` only (no `limits::check`): scroll back, two ticks -> still busy, nothing typed; back at the bottom one tick -> prompt typed |
| 2 | second Enter in the list ends `resumed` | fixed: `typed_input` ends `resumed` only on a non-empty line; while `picking` the first line (empty or not) only ends `picking` | `a_replayed_limit_line_after_the_resume_list_is_no_event` now presses Enter twice; the agent ignores empty lines like claude |

Red / green:
- F1 RED (with the swallowed guard, i.e. the tree as reviewed): `the_switch_alone_…` panicked `timed out; screen:` (stuck, never re-read). GREEN after the fix, 3 runs in a row with `a_screen_skipped_…`. Mutation `*read = Some(out);` inside the guard (the round-0 order) -> same timeout, reverted with an Edit that keeps the line break (checked with `cat -A`: `        return false;$` on its own line).
- F2 RED: panicked at ai_switch_tests.rs:997 `the earlier run's line`. GREEN after the fix (`ai_switch_tests::a_replayed limits` 28 passed, 3 runs).
- Swallowed-statement search: `git diff HEAD --name-only | grep '\.rs$' | xargs grep -nE '//.*;\s*$' | grep -v '//[/!]'` -> only switch.rs:500 (fixed) plus four prose comments whose sentence goes on in the next comment line (completion.rs:418, commands/terminal.rs:1104, session.rs:716, vr-term terminal.rs:151 — each next line checked, comment text). Also `grep -nE '\S\s{4,}(return|let|if|\*|s\.|self\.)'` on lines with `//` in switch.rs and limits.rs -> none. Every `MUTATION` / `DBG` marker gone (`grep -c` 0).

Guard and related (log .ai-dev-team/workspace/qa-artifacts/T6/r2-final.log):
```
fmt: no diff   (rustfmt only on ai_switch_tests.rs, switch.rs, limits.rs)
run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings -> Finished, clippy exit 0
run-heavy.sh cargo test -p vr-ide --lib -- switch:: ai_switch_tests ai_limit ai_ limits
test result: ok. 155 passed; 0 failed; 0 ignored; 0 measured; 923 filtered out
```
Updated rows: SEC-30 pass (+ the_switch_alone_never_reads_a_scrolled_back_screen); AC-6 / SEC-29 pass (picker test with a double Enter); BASE-3 pass — the round-1 F3 disclosure is closed, the fix is now proven red/green; BASE-38 rustfmt only on the 3 changed Rust files; COST-9 nothing running (b1ezkuzmx finished). D? line: one `D? (T6 R2)` appended to decisions.md.
