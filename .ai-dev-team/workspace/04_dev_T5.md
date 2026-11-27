# T5 Limit/login detection, per-account state
Task base: e150697ee34b08fc30186d66a140401b8c920dfb (tree at 1c4223a)

## Progress
- read brief, spec, plan Task 5, threat model SEC-26/27, decisions
- merged tree green: cargo test -p vr-ide ai_ → 87 passed, 0 failed
- reuse: made usage::days_from_civil, usage::write_at, ui::ai_changes::utc_offset pub(crate) (visibility only)
- red (unit): `cargo test -p vr-ide limits::` with stubs → `test result: FAILED. 3 passed; 11 failed` (the 3 passing are the "no hit" cases, which a stub returning None passes trivially)
- green: unit limits:: 14 passed; integration ai_limit 4 passed; mutation (check/on_report disabled) → 2 of 4 integration tests fail, restored
- code: limits.rs implemented; wired in ai/mod.rs (fields, refresh, restart, send_typed, read_usage → on_report) and event_loop.rs (check per pass)
- integration tests ai_limit_* added (5), login marking added
- related runs + guard green (below)

## Files changed
- `crates/vr-ide/src/ai/limits.rs` (new) — patterns, screen rules, reset parsing, account state file, session watch (`check`, `act`, `on_report`)
- `crates/vr-ide/src/ai/limits_tests.rs` (new) — 14 unit tests
- `crates/vr-ide/src/ai/mod.rs` — `pub mod limits`; `AiSession::{limit_seen, limit_watch}`; `refresh` marks output; `restart` clears the event; `send_typed` (keys, paste) marks `typed`; `read_usage` calls `limits::on_report` per changed report
- `crates/vr-ide/src/event_loop.rs` — `background_pass` calls `limits::check(app, now)` after `read_usage` (one call; deviation: file not in the row, needed because the quiet check must run on passes without output)
- `crates/vr-ide/src/ai_tests.rs` — `LIMIT_AGENT`, `limit_app`, 5 `ai_limit_*` tests
- visibility only (reuse, deviation: files not in the row): `ai/usage.rs` `days_from_civil`, `write_at` → `pub(crate)`; `ai/accounts.rs` `unlock` → `pub(crate)`; `ui/ai_changes.rs` `utc_offset` → `pub(crate)`

## Patterns and sources (also in the `limits.rs` module comment)
Taken with `strings` from the installed binaries; neither tool was run (BASE-49).
- claude 2.1.332 (`~/.local/share/claude/versions/2.1.332`, via `readlink -f ~/.local/bin/claude`):
  - `function q(e,n,o,d){let g=d?.progressSavedSuffix?" · progress saved":"";return`You've hit your ${e}${n}${g}`}`
  - names `WFe={five_hour:"5-hour limit",seven_day:"weekly limit",seven_day_opus:"Opus limit",seven_day_sonnet:"Sonnet limit",seven_day_overage_included:"Fable limit",…}`, plus `q("limit",…)`, `q("usage limit",…)`, `d.startsWith("You've hit your session limit")`
  - reset `y=_?` · resets ${_}`:""` with `_=uA(h,!0)`; `JN` prints `3pm`/`3:30pm` (≤ 24 h) or `Dec 3, 3pm` (+ year when different) + ` (${zone})`
  - `Twn={running:"Usage limit reached · wrapping up",stopped:"Usage limit reached",…}` with ` · limit resets ${gt}`
  - not counted: `You've hit your fast limit` (`· resets in …`, fast mode only), `Context limit reached`, `Usage limit reached · wrapping up`
  - login: `"Select login method:"`
- codex 0.144.1 (`~/.nvm/versions/node/v26.1.0/lib/node_modules/@openai/codex/node_modules/@openai/codex-linux-x64/vendor/x86_64-unknown-linux-musl/bin/codex`; `bin/codex.js` only dispatches to it):
  - `You've hit your usage limit.` + `Upgrade to Plus …` / `Upgrade to Pro …` / `Visit … to purchase more credits` / `To get more access now, send a request to your admin`; ` Try again at ` / ` or try again at `; `You've hit your usage limit for `; `Usage limit reached. You've reached your usage limit. Increase your limits to continue using codex.`
  - time formats `%-I:%M %p` and `%b %-d` + `st|nd|rd|th` + `, %Y %-I:%M %p`
  - login: `Sign in with ChatGPT to use Codex as part of your paid plan`, `Finish signing in via your browser`
- Other tools (gemini, aider, cursor-agent): no installed binary verified → no patterns, never a hit (see D3 (T5)).
- Throwaway: `qa-artifacts/T5/find.pl` (context search over the strings dump); the dumps (85 MB / 12 MB) were deleted after use.

## What the rule does with spoofs (SEC-26, test `spoofed_lines_in_agent_output`)
- `● You've hit …` (agent reply bullet), `▎ …` / `> …` (quote, prompt), `$ echo "You've hit …"`, `12:00:01 ERROR You've hit …`, a sentence containing it, a reset part followed by other words → no hit (whole line only; only `⎿`/`■` and a two-sided box border are stripped).
- Tool output after the line (`● Done.`), or more output within 1 s → no hit (`more_output_after_the_line_is_no_hit`, `ai_limit_line_followed_by_output_is_no_event`).
- Typed into the input box (rules / rounded box / codex `›` composer) → no hit (`the_input_box_is_ignored`).
- Residual (accepted risk in the threat model): a code block holding exactly the message as the last content above the input box, with the session quiet ≥ 1 s, is a hit — asserted in the test so the behaviour is explicit. T6's restart cap bounds it.

## Decisions (T5)
- D1 (T5): one limit event per session ends when the user typed (keys/paste through `send_typed`, not mouse) **and** a later quiet screen no longer shows a limit line, or on a restart. Why: claude echoes typing in its input box, so "output after the user's input" alone would re-fire on the old line still on screen. Cost: a user's retry that gets a second limit while the first line is still visible gives no second notice.
- D2 (T5): extra SEC-26 condition — the limit message must be the last content above the input box (rows after it only blank or indented under its own column). Why: SEC-26 "limit line followed by more output → no event", and it stops a tool result (`⎿ $ echo …`) followed by the agent's reply.
- D3 (T5): only claude and codex have patterns (verified from installed binaries); other tools never hit. The spec mentions limit detection for gemini/aider/cursor-agent; their patterns cannot be verified here → open for the leader.
- D4 (T5): `limited_until` is stored only when a reset time is known: the parsed screen time, else the latest reset of a window the session's own validated report shows ≥ 100 % (range-checked). No reset → no state entry (notice only).
- D5 (T5): a report under 100 % clears `limited_until` only when the reporting session is not at its limit now (`!limit_seen`) — claude may report 5h < 100 % while an Opus/weekly limit stops it. At most one locked update per run for `works` and one for the clear (reset by a new hit).
- D6 (T5): a login screen on a quiet session sets `works = false` for an existing entry (once per run). Login lines are matched whole (wrapped rows joined) anywhere on screen but the typed prompt row (keeps its marker); boxes are not left out because a login screen may be boxed.
- D7 (T5): state file JSON `{"tool/account":{"works":bool,"limited_until":secs}}`; entries equal to the default and past `limited_until` are dropped on every read/write; tmp `ai-account-state.tmp<pid>` via `usage::write_at` (unlink, O_EXCL|O_NOFOLLOW, fsync, rename) under the lock; the state folder is opened following links (as `accounts.rs` does).
- D8 (T5): notice `"<tool>[@<account>] hit its usage limit (resets HH:MM)"`, `Mon D HH:MM` when ≥ 24 h away, Warning level; no reset → without the parenthesis. Local time from `ui::ai_changes::utc_offset` (zone file).
- D9 (T5): a scrolled-back session is re-checked a second later (the snapshot shows scrollback rows, not the screen).

## Red / green
- red (unit, stub bodies): `cargo test -p vr-ide limits::` → `test result: FAILED. 3 passed; 11 failed` (the 3 passing were "no hit" cases a stub passes)
- red (integration): with `limits::check` and `on_report` disabled → `ai_limit_counts_after_a_quiet_second_with_one_notice_per_event` and `ai_limit_state_follows_usage_reports` FAILED (2 of 4); restored
- green, final code (logs in `qa-artifacts/T5/`):
```
cargo test -p vr-ide limits::    → test result: ok. 14 passed; 0 failed; … 1015 filtered out
cargo test -p vr-ide ai_         → test result: ok. 92 passed; 0 failed; … 937 filtered out
cargo test -p vr-ide usage       → test result: ok. 18 passed; 0 failed   (usage.rs visibility change)
cargo test -p vr-ide event_loop  → test result: ok. 10 passed; 0 failed   (event_loop.rs changed)
cargo test -p vr-ide --test statusline_helper → test result: ok. 3 passed
```
Tests added: limits:: `claude_limit_lines_from_the_binary`, `codex_limit_lines_from_the_binary`, `a_wrapped_claude_line_is_joined`, `ignores_old_lines`, `more_output_after_the_line_is_no_hit`, `the_input_box_is_ignored`, `spoofed_lines_in_agent_output`, `reset_time_parsing` (incl. `resets 3pm` today/tomorrow, `2099-01-01` → none, > 8 days → none, UTC+7), `login_screen_detected`, `state_roundtrip_and_mode_0600`, `state_drops_bad_keys_and_clamps` (`claude/../x`, `claude/Work`, `gemini/default` dropped, clamp to 8 days), `state_ignores_oversize_or_garbage` (garbage, 70 KB, symlink, FIFO — no hang, not written through), `state_two_writers_keep_both` (2 threads × 40 updates), `state_file_in_a_private_folder_keeps_0600`; ai_tests `ai_limit_counts_after_a_quiet_second_with_one_notice_per_event` (900 ms → none, 1000 ms → one notice `claude hit its usage limit (resets 15:00)`, same event at +70 s → none, new event within the minute → no notice, after the minute → notice, process still running), `ai_limit_line_followed_by_output_is_no_event`, `ai_limit_typing_marks_the_session`, `ai_limit_state_follows_usage_reports` (63 % → works + limited_until cleared; 100 % → works, limit kept, no event, process running), `ai_limit_login_screen_marks_the_account_not_working`.

## Guard
```
cargo fmt --all --check                                     → FMT-OK (exit 0)
$TEAM_BIN/run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings → Finished `dev` profile … (exit 0)
```
rustfmt ran only on `ai/limits.rs` (+ its `limits_tests.rs`) and `ai_tests.rs`.

## Out-of-scope noticed
- `02_tasks.md` shows as modified in the tree (leader's board edits, not mine).
- Spec lists screen limit detection for gemini/aider/cursor-agent; no verified patterns (D3).

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| session id / usage of the session | T3 `usage.rs`, capabilities.md | reused `AiSession::usage` (validated by `usage::valid_session_id` at parse) |
| screen text | `TermTab::screen_text` / `grid_now` | reused `grid_now()` + `Grid::row_text` (needs `scroll_offset` and `cols`) |
| notice | attention.rs, `App::warn` | reused `app.warn` (fixed text); attention marks are for bells, not used |
| atomic 0600 write | `usage::write_at`, `accounts::write_index` | reused `usage::write_at` (made pub(crate)) |
| lock | `accounts::lock_index` / `unlock` | same pattern, reused `accounts::unlock` (pub(crate)) |
| local time | `ui::ai_changes::{utc_offset, clock}` | reused `utc_offset` (pub(crate)) |
| civil date | `usage::days_from_civil` | reused (pub(crate)); `civil()` (inverse) added in limits.rs |
| name / tool validation | `accounts::{validate, var_for, DEFAULT}` | reused in `state_key` |
| per-pass hook | `event_loop::background_pass`, `read_usage` | one call added after `read_usage` |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| AC-5 | pass | `ai_limit_counts_after_a_quiet_second_with_one_notice_per_event` (one notice per event with `resets 15:00`, process running); `ai_limit_state_follows_usage_reports` (100 % → no event, process running); nothing in T5 ends a process |
| SEC-26 | pass | whole line + markers only (`spoofed_lines_in_agent_output`), last 12 rows (`ignores_old_lines`), input box (`the_input_box_is_ignored`), quiet ≥ 1 s and later output (`ai_limit_*` tests, `more_output_after_the_line_is_no_hit`), one event / ≤ 1 notice per 60 s (`ai_limit_counts…`), notice from fixed text + validated label + parsed time (`act`, `account_label`, `reset_text`), reset only in [now, now+8 d] (`reset_time_parsing`), `2099-01-01` → event without reset (`codex_limit_lines_from_the_binary`) |
| SEC-27 | pass | `state_roundtrip_and_mode_0600`, `state_ignores_oversize_or_garbage` (O_NOFOLLOW, regular, ≤ 64 KiB, FIFO no hang), `state_drops_bad_keys_and_clamps` (`claude/../x` dropped, clamp), `state_two_writers_keep_both` (flock on `ai-account-state.lock`), `works` only from a report with a UUID id (`on_report`, `ai_limit_state_follows_usage_reports`), cleared by < 100 % report |
| BASE-1 | pass | diff: limits.rs/limits_tests.rs (new), ai/mod.rs, ai_tests.rs; 1 call in event_loop.rs + 4 visibility changes listed as deviations; switch.rs/login.rs/slash.rs/label untouched |
| BASE-2 | pass | plan interfaces kept: `LimitHit`, `limit_hit`, `login_screen`, `AccountState`, `load_state`, `save_state`, `AiSession::limit_seen`, notice text; additions `limit_hit_at`, `update_state`, `state_key`, `parse_when` |
| BASE-3 | pass | red lines above (unit stub run 11 failed; integration mutation 2 failed) |
| BASE-4 | pass | Red / green and Guard sections |
| BASE-5 | pass | clippy through `run-heavy.sh`; related tests direct as the brief says; nothing left running |
| BASE-6 | pass | English code/comments; comments say why |
| BASE-9 | pass | screen text never reaches notices or files; keys validated (`state_key`), times range-checked, file read bounded/no-follow |
| BASE-10 | pass | only `limits::`, `ai_`, `usage`, `event_loop`, `statusline_helper` + guard |
| BASE-11 | pass | no stash/checkout/reset; mutation check via copy to scratchpad and back |
| BASE-14 | pass | first `ai_` run as a background task, turn ended until notified; no waits |
| BASE-15 | pass | no process stopped; no pkill |
| BASE-16 | pass | `## Reuse` table; capabilities.md entry added |
| BASE-17 | pass | nothing installed; nothing written to `$TEAM_TOOLS` |
| BASE-18 | pass | no agents started |
| BASE-19 | pass | notes created first, progress lines per step (top of this file) |
| BASE-21 | pass | no secrets in output or notes |
| BASE-22 | pass | `STATE_FILE`/`STATE_LOCK` follow `ai-accounts`/`ai-accounts.lock`; tests `ai_limit_*` beside `usage_*` |
| BASE-23 | pass | entry paths: event loop pass (`ai_limit_*` via `background_pass`), report path (`ai_limit_state_follows_usage_reports`), restart clears (`restart` code path; covered by `restarted()`), keys/paste (`ai_limit_typing_marks_the_session` for keys; paste uses the same `send_typed`) |
| BASE-24 | pass | no git index/history commands |
| BASE-25 | pass | guard exactly as written, both parts, Guard section |
| BASE-32 | pass | D3 (UUID id only via `Usage.session_id`), D11 untouched, D16 (`read_usage` cadence unchanged; `on_report` only on changed reports) |
| BASE-34 | pass | no blocked step |
| BASE-38 | pass | rustfmt only on limits.rs(+tests) and ai_tests.rs |
| BASE-45 | pass | AC-5 text matches the spec |
| BASE-49 | pass | claude/codex never run; `strings` on the binaries and reading `bin/codex.js` only |

n-a: BASE-7 (no commit) · BASE-8 (no dependency change) · BASE-12, BASE-40 (nothing outside the repo) · BASE-13 (no untracked file deleted; only my own strings dumps in qa-artifacts) · BASE-20 (nothing removed) · BASE-26 (no rewrite) · BASE-27, BASE-30 (no shared data) · BASE-28 (no third-party code) · BASE-29, BASE-41 (no browser/credentials) · BASE-31 (no container) · BASE-33 (no other system) · BASE-35 (no USER rule on this area) · BASE-36 (in app scope) · BASE-37 (not a runtime defect fix) · BASE-39 (no schema) · BASE-42 (no test settings changed) · BASE-43 (no access rule) · BASE-44 (not a client task) · BASE-46 (no runtime walk; tests on final code) · BASE-47 (no fixed value) · BASE-48 (no machine state claimed)

| Rule | Status | Evidence |
|---|---|---|
| COST-1 | pass | clippy via run-heavy.sh; small related runs direct |
| COST-3 | pass | no sleep/pgrep; background task notification used once |
| COST-4 | pass | no full suite |
| COST-8 | pass | batched reads; new files written whole; edits by Edit/perl -pi |
| COST-9 | pass | nothing left running |
| COST-10 | pass | related set includes `usage`, `event_loop`, `statusline_helper` (files mentioning changed files) |
| COST-13 | pass | `qa-artifacts/T5/find.pl` only |
| COST-16 | pass | final runs logged to `qa-artifacts/T5/test-*.log`, `guard.log`; summary lines read |

n-a: COST-2 (developer tests only) · COST-5, COST-6, COST-7 (no browser, no UI sweep, no API) · COST-11 (no e2e) · COST-12 (leader) · COST-14 (no CI) · COST-15, COST-17 (no goldens) · COST-18 (no full run) · COST-19, COST-20, COST-21 (qc)

## Fix round 1
Base: task-base T5 1cbbbaef59c7d93855affbf950d01b9d5aaf035b. Replacement developer, continuing from the notes above and the uncommitted tree.
### Progress (R1)
- read notes, limits.rs, limits_tests.rs, ai_tests limit tests, SEC-26/27, D19–D27, probe `qa-artifacts/T5/rev/p.rs`, QC F1
- claude strings for finding 6: searching the 2.1.332 binary (`grep -a -o`)
- strings (claude 2.1.332, byte offsets via `grep -a -b -o -F` then `dd` context; no dump kept): `q(name, suffix)` = `You've hit your ${name}${suffix}` with names `monthly spend limit`, `org's monthly spend limit`, `channel's monthly spend limit` (`_i`), `channel's monthly usage limit` (`yi`), `team's pooled group budget` (`eun`); suffixes ` · resets ${_}`, ` · your ${limitName} resets ${resetTime}`, ` · run … to raise it, or visit …`, ` · visit … to raise it`; UI line `You've hit your monthly spend limit · /usage-credits to raise it`; sentences `You've hit your monthly spend limit.` / `You've hit your channel's monthly spend limit.` / `You're out of usage credits.` + ` See Usage in Settings` / ` Switch to another model… to continue.` / ` Run /usage-credits to keep using … or /model to switch models.` / ` /model to switch models.`; `uHi` prefix list holds `You're out of usage credits`; ` · out of usage credits` appended to a window line
- tests written first (limits_tests: 8 new/changed; ai_tests: 2 new, 2 changed)
- code: all 7 findings implemented in limits.rs (+ `send_typed` → `Watch::typed_input` in ai/mod.rs, `parse_tzif` → pub(crate) in ui/ai_changes.rs); green: `limits::` 22 passed, `ai_limit` 7 passed
- red: mutation script `qa-artifacts/T5/mutate.sh` (each mutation undoes one fix, file restored on exit) running in background → `qa-artifacts/T5/r1-red.log`
- green + guard done (below); nothing left running

### Per finding
| # | Change | Red (mutation undoing the fix, `qa-artifacts/T5/r1-red.log`) | Green |
|---|---|---|---|
| 1 blocker | `local_time`: month word head via `w.get(..3)?` (no byte-offset slice). Scanned limits.rs for other slices: `&text[at + AT.len()..]` (after an ASCII `find`), `name[..1]` (ASCII const `MONTHS`), `&words[2..]`/`&rest[1..]` (Vec slices, length checked) — all safe | `non_ascii_reset_text_is_not_a_time` FAILED, `arbitrary_unicode_screens_never_panic` FAILED (`limits::` 27 passed; 2 failed) | both ok |
| 2 medium | `login_in` takes the last [`WINDOW`] rows, the line must be the last content (`Screen::last_content`, same as a limit) and not in what the user typed; `act` never marks `works = false` for a run whose report with an id already set `works_marked` | `login_line_is_as_strict_as_a_limit_line` FAILED (window/last-content removed); `ai_limit_login_line_after_a_report_keeps_the_account_working` FAILED (`works_marked` guard removed) | ok |
| 3 minor | new `Zone` (name, TZif offsets, `known`): `Zone::local()` = `TZ` (`:name` or `name`, validated, read under `/usr/share/zoneinfo`) else `/etc/localtime` (name from the link, else `/etc/timezone`), none → UTC; POSIX rule strings / unreadable → unknown. Wall time → instant with the offset at that instant (`Zone::to_utc`). `parse_when` drops a screen time when the shown ` (zone)` is not ours (`UTC`/`Etc/UTC`/`UCT`/`Universal`/`Zulu` equal) or our zone is unknown → report reset used. Notice shows ` UTC` when the zone is unknown | `reset_time_across_dst_and_zones` FAILED (offset at `now`); zone-name check removed → `claude_limit_lines_from_the_binary`, `reset_time_across_dst_and_zones`, `non_ascii_reset_text_is_not_a_time` FAILED | ok, plus `local_zone_follows_tz_and_localtime` (TZ with/without colon, link name, no localtime → UTC, `../etc/passwd`, absolute, `Test/../…`, POSIX string → unknown) |
| 4 minor | `PAST_GRACE` = 3600 s: a clock time that passed < 1 h ago stays today's, so `parse_when` rejects it (no `limited_until` from the screen; report reset used) | `a_time_just_passed_is_not_tomorrow` FAILED (grace 1 s = old rule) | ok |
| 5 minor | `LimitHit::sticky` / `AccountState::sticky` (JSON `"sticky": true`, dropped with `limited_until`): model limits (Opus/Sonnet/Fable, codex `usage limit for …`), spend caps, pooled budget, credits are sticky; a report < 100 % never clears a sticky limit (it lasts to its reset) | `ai_limit_state_follows_usage_reports` (case 63 % + sticky) FAILED | ok (63 % → cleared, 63 % sticky → kept, 100 % → kept) |
| 6 minor | claude names `monthly spend limit`, `org's/channel's monthly spend limit`, `channel's monthly usage limit`, `team's pooled group budget`; head `You're out of usage credits`; a `.` sentence tail accepted only for these and only followed by claude's own sentences; reset part also `your <window> resets <t>` | `claude_spend_and_credit_lines_from_the_binary` FAILED | ok |
| 7 low (QC F1) | `Watch::typed_text`: the text of keys/pastes as sent (escape sequences dropped, backspace applied, ≤ 2048 chars, cleared on restart); `check` drops a limit or login line whose text is in it | `ai_limit_typed_text_echoed_is_no_event` FAILED | ok (+ `typed_input_is_kept_as_text`) |

### Decisions (T5 R1)
- D? (T5 R1) a: finding 5 — option "only 5-hour/weekly limits are cleared by a report": the hit's kind is stored as `sticky` in the state file (SEC-27's "both windows < 100 %" only speaks for the windows the report shows). Cost: a sticky limit that ends early stays until its shown reset (≤ 8 days clamp).
- D? (T5 R1) b: finding 7 — instead of "skip while typed since last output" (claude echoes typing, so output cannot be told from echo), a bounded buffer of what the user typed this run; a screen line contained in it (whitespace aside) is never a limit or login. Cost: if the user typed the exact full message earlier in the run, the real one is missed (conservative side of SEC-26).
- D? (T5 R1) c: finding 3 — screen times are believed only in our zone; a claude time with another ` (zone)`, or any time when our zone cannot be read, is dropped for the report's reset. Notice clock shows ` UTC` when the zone is unknown (D26 text otherwise unchanged).
- D? (T5 R1) d: finding 2 — login lines follow the limit rules (window, last content, not typed); box rows stay allowed (a login screen may be boxed).
- D? (T5 R1) e: finding 4 — grace of 1 h for a clock time just passed.

### New states (round 1)
- `Watch::typed_text` (per session, per run): quit/restart → cleared (`restarted`, test `typed_input_is_kept_as_text`); huge paste → trimmed to the last 2048 chars at a char boundary (same test); invalid UTF-8 → lossy, never a panic (`arbitrary_unicode_screens_never_panic` feeds it). No thread, no I/O.
- `AccountState::sticky` (state file): old files without it read as false; dropped with `limited_until` by `clean`; written only when true (`state_roundtrip_and_mode_0600` has a sticky entry).
- `Zone::local()` (process-wide `OnceLock`, read once): an unreadable / FIFO-free path — only regular files are read (`metadata().is_file()` before reading, ≤ 256 KiB); TZ value validated before it names a path.

### Runs (final code)
```
cargo test -p vr-ide limits::    → test result: ok. 22 passed; 0 failed; … 1017 filtered out
cargo test -p vr-ide ai_limit    → test result: ok. 7 passed; 0 failed; … 1032 filtered out
cargo test -p vr-ide event_loop  → test result: ok. 10 passed; 0 failed
cargo test -p vr-ide ai_changes  → test result: ok. 26 passed; 0 failed   (parse_tzif visibility)
cargo test -p vr-ide ai_         → test result: ok. 94 passed; 0 failed
cargo fmt --all --check          → FMT-OK
$TEAM_BIN/run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings → Finished `dev` profile … (exit 0)
```
Logs: `qa-artifacts/T5/r1-tests.log`, `r1-fmt.log`, `r1-clippy.log`, `r1-red.log`. rustfmt only on limits.rs, limits_tests.rs, ai_tests.rs.
Files changed this round: `crates/vr-ide/src/ai/limits.rs`, `crates/vr-ide/src/ai/limits_tests.rs`, `crates/vr-ide/src/ai_tests.rs`, `crates/vr-ide/src/ai/mod.rs` (`send_typed` passes the bytes to `Watch::typed_input`), `crates/vr-ide/src/ui/ai_changes.rs` (`parse_tzif` → pub(crate), visibility only, reuse).
Not touched: switch.rs, login.rs, slash.rs, tab label, history.rs, terminal.rs, ssh. claude/codex never run (strings only).
Out of scope noticed: `utc_offset`/`parse_tzif` ignore the TZif footer rule, so with a slim zone file times after the last stored transition use the last offset (affects ui clock too).

### Rule compliance (rows touched this round)
| Rule | Status | Evidence |
|---|---|---|
| AC-5 | pass | `ai_limit_counts_after_a_quiet_second_with_one_notice_per_event` still green; reset time correct across DST/TZ (`reset_time_across_dst_and_zones`, `local_zone_follows_tz_and_localtime`); `a_time_just_passed_is_not_tomorrow`; 100 % alone: `ai_limit_state_follows_usage_reports` |
| SEC-26 | pass | no panic on any text (`arbitrary_unicode_screens_never_panic`, 4000 generated screens × 3 tools + login + parse + typed); typed echo never a hit (`ai_limit_typed_text_echoed_is_no_event`); login under the same rules (`login_line_is_as_strict_as_a_limit_line`); new spend/credit lines whole-line only (`claude_spend_and_credit_lines_from_the_binary`); reset only in [now, now+8 d] and in our zone |
| SEC-27 | pass | `works` not unset for a validated run (`ai_limit_login_line_after_a_report_keeps_the_account_working`); `sticky` round-trips 0600 (`state_roundtrip_and_mode_0600`); cleared only for window limits (`ai_limit_state_follows_usage_reports`) |
| BASE-1 | pass | only the files above; `ui/ai_changes.rs` visibility change listed as deviation |
| BASE-3 | pass | each fix has a mutation that turns its test red (`r1-red.log`) |
| BASE-4 | pass | Runs section |
| BASE-9 | pass | screen text and typed text only compared, never stored or shown; TZ validated (`valid_zone_name`) before naming a file, zone file regular + bounded |
| BASE-10 | pass | related runs only: `limits::`, `ai_limit`, `ai_`, `event_loop`, `ai_changes` + guard |
| BASE-11 | pass | mutations via copy-and-restore in `mutate.sh` (trap restores; `cmp` confirmed restored); no git stash/checkout |
| BASE-14 | pass | mutation run as background task, turn ended until notified; the slow `grep -a` context search stopped by TaskStop and replaced with an offset search |
| BASE-15 | pass | only my own background task stopped (TaskStop) |
| BASE-25 | pass | guard exactly as written, both parts |
| BASE-38 | pass | rustfmt on limits.rs, limits_tests.rs, ai_tests.rs only |
| BASE-49 | pass | claude never run; `grep -a -b -o -F` + `dd` over the binary only |
| COST-1 | pass | clippy via run-heavy.sh |
| COST-8 | pass | Edit tool for code; perl -pi only for one-line renames in test calls and the mutation script (throwaway) |
| COST-9 | pass | nothing left running |
| COST-13 | pass | throwaway `qa-artifacts/T5/mutate.sh`; nothing in `$TEAM_TOOLS` |
| COST-16 | pass | logs in `qa-artifacts/T5/r1-*.log` |
Other rows unchanged from the table above.

## Quick pass 1
- read QP1 findings; extracted `record_limit` (old behaviour) to test the merge
- red: `cargo test -p vr-ide limits::` → `a_shorter_limit_keeps_the_longer_one ... FAILED` (limits_tests.rs:702, 3 h overwrote 5 d), `watch_debug_never_shows_typed_text ... FAILED` (:691, typed text in `{:?}`); `test result: FAILED. 22 passed; 2 failed`
- code + green + guard (below); nothing left running

| # | Change | Test |
|---|---|---|
| 1 minor | `act` records through new `record_limit`: the later reset wins together with its own `sticky`; an earlier one is ignored; an equal one keeps `sticky` when either is | `a_shorter_limit_keeps_the_longer_one` (Opus 5 d sticky, then 5-hour 3 h → 5 d sticky kept; 6 d → replaces; same 6 d sticky → sticky) |
| 2 minor | `Watch` Debug hand-written: every field as before, `typed_text` shown only as `<N chars>` | `watch_debug_never_shows_typed_text` (`{:?}` and `{:#?}`) |

```
cargo test -p vr-ide limits::  → test result: ok. 24 passed; 0 failed
cargo test -p vr-ide ai_limit  → test result: ok. 7 passed; 0 failed
cargo fmt --all --check        → FMT-OK
$TEAM_BIN/run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings → Finished `dev` profile … (exit 0)
```
Logs: `qa-artifacts/T5/qp1-tests.log`, `qp1-fmt.log`, `qp1-clippy.log`. rustfmt only on limits.rs and limits_tests.rs. Files: `crates/vr-ide/src/ai/limits.rs`, `crates/vr-ide/src/ai/limits_tests.rs`.
Rule rows touched: SEC-27 pass (`a_shorter_limit_keeps_the_longer_one`: a limit is never shortened by a later hit); BASE-21 pass (typed text never in Debug output, `watch_debug_never_shows_typed_text`); BASE-3 pass (red above); BASE-4/BASE-25 pass (block above); BASE-38 pass; COST-9 pass (nothing running). Others unchanged.
