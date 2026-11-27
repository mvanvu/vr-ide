# T4 Usage display (tab label, 90 % notice, `/ai usage`)

Task base: 0aa067f69f6ffe6b450fb1f5a598b3cab3a96f11 (HEAD 331008a). Side: fe (TUI).

## Progress
- read: plan Task 4, spec Display/AC-4, D13–D18/D26/D30, usage.rs, limits.rs (reset_text, Zone), ai/mod.rs label/short_label/read_usage, ui/ai_view.rs tab bar, slash.rs `fn ai`, ai_view_tests helpers.
- red tests written: usage_tests (usage_suffix_takes_the_higher_window, ai_usage_reset_clock_formats, ai_usage_text_lists_both_windows), ai_view_tests (label_shows_usage_suffix, label_weekly_when_higher, ai_usage_label_sizes), ai_tests (notice_once_at_90_percent, ai_usage_command_lists_windows, ai_usage_without_data)
- red: `cargo test -p vr-ide --lib usage` → `error[E0425]: cannot find function usage_suffix / reset_clock / usage_text` (15 errors; the tests existed before the code).
- code: usage.rs display helpers + `show` + `warn_high`; ai/mod.rs `AiSession::high_warned`, `read_usage` calls `warn_high`; slash.rs `/ai usage`; ai_view.rs suffix + warning paint + fit fallback; docs/commands.md, README.md.
- green: related set 225 passed; snapshots `ai_usage_{120x40,80x24,40x12}` generated once after the code was final (new files, reviewed below); guard fmt + clippy clean.

## Files changed
- `crates/vr-ide/src/ai/usage.rs` — `WARN_PERCENT`, `usage_suffix`, `reset_clock`, `account_name`, `usage_text`, `show` (`/ai usage`), `Warned`, `warn_high`, `civil_from_days`.
- `crates/vr-ide/src/ai/mod.rs` — `AiSession::high_warned`; `read_usage` warns after `limits::on_report`.
- `crates/vr-ide/src/commands/slash.rs` — `/ai usage` in `fn ai` (pattern of `rename`); registry text `; /ai usage`.
- `crates/vr-ide/src/ui/ai_view.rs` — `TabText` struct (was a 4-tuple), `Item::usage`, `BarTab::warn`; suffix after any shown label; `theme.warning` fg on the percent (or on the number when no suffix shows); fit runs with suffixes, and again without when they would fold a tab into `+N` or push the `+` out.
- `docs/commands.md` (table row + one sentence), `README.md` (new "Usage" subsection under AI panel; dropped "No usage tracking").
- Tests: `ai/usage_tests.rs`, `ai_tests.rs`, `ui/tests/ai_view_tests.rs`; new snapshots `ui/tests/snapshots/vr_ide__ui__tests__ai_view_tests__ai_usage_{120x40,80x24,40x12}.snap`.
- `.ai-dev-team/capabilities.md` — T4 line.

## Tests added
- `usage_suffix_takes_the_higher_window`, `ai_usage_reset_clock_formats`, `ai_usage_text_lists_both_windows` (unit, usage_tests).
- `label_shows_usage_suffix` (`" 2 claude@work 63% x "`, plain `label()` unchanged, suffix hides no tab / `+` at 120..16 cols), `label_weekly_when_higher` (` 91%w` cells in `theme.warning.fg` in synthwave, dracula, dark, light; name and 63 % not), `ai_usage_label_sizes` (snapshots at 120x40, 80x24, 40x12; number-only tab of a 95 % account painted warning) — ai_view_tests.
- `notice_once_at_90_percent` (real report files through `background_pass`: 91 → 95 → 89 → 92 same window = one notice; new reset + weekly 93 → two more), `ai_usage_command_lists_windows` (palette `/ai usage` → `claude@work: 5-hour 63% (resets HH:MM) · weekly 41% (resets Mon D HH:MM)`), `ai_usage_without_data` (no session / no report / extra argument) — ai_tests.

## Tab bar at 80 and 40 columns (63 % session 1, 95 % session 2 active; from the snapshots)
80x24 (panel 31 cols, fit level 3; `95%` in the warning colour):
```
" /home/user/DEV/vr-ide                          │ 1  2 claude@work 95%        + "
```
40x12 (panel full width, level 0):
```
" 1 claude 63%  2 claude@work 95% x    + "
```
120x40:
```
… │ 1 claude 63%  2 claude@work 95% x           + "
```

## Commands run (final, trimmed; full logs in `.ai-dev-team/workspace/qa-artifacts/T4/`)
```
$ cargo test -p vr-ide --lib -- ai_ ai_view slash usage label_ notice_once limits   (related-tests.log)
test ai::usage::tests::ai_usage_reset_clock_formats ... ok
test ai::usage::tests::ai_usage_text_lists_both_windows ... ok
test ai::usage::tests::usage_suffix_takes_the_higher_window ... ok
test ai_tests::ai_usage_command_lists_windows ... ok
test ai_tests::ai_usage_without_data ... ok
test ai_tests::notice_once_at_90_percent ... ok
test slash_tests::docs_table_matches_the_registry ... ok
test ui::tests::ai_view_tests::label_shows_usage_suffix ... ok
test ui::tests::ai_view_tests::ai_usage_label_sizes ... ok
test ui::tests::ai_view_tests::label_weekly_when_higher ... ok
test result: ok. 225 passed; 0 failed; 1 ignored; 0 measured; 824 filtered out; finished in 2.89s
$ cargo fmt --all --check                                  → rc=0 (fmt.log empty)
$ run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings   → rc=0 (clippy.log)
    Finished `dev` profile [unoptimized + debuginfo] target(s)
```
(Earlier clippy round flagged `cloned_ref_to_slice_refs` twice in my test; fixed with `std::slice::from_ref`.)

## Decisions (D? (T4))
- D? (T4): the usage suffix lives in the tab bar only (`ui::ai_view` via `usage::usage_suffix`); `AiPanel::label` / `short_label` stay plain because notices, diff tab names, review and the status bar read them. Plan placed it in `label`; the test `label_shows_usage_suffix` checks the drawn tab (`" 2 claude@work 63% x "`) and that `label()` is unchanged.
- D? (T4): percent shown as its whole-number part (floor), so `90%` always comes with the warning (89.6 → `89%`, no warning); over 100 shows as is (`130%w`).
- D? (T4): the suffix follows any shown label, the short one included; the suffixes are dropped (all tabs) before a tab would fold into `+N` or the `+` would be pushed out; a tab without a shown suffix whose account is ≥ 90 % gets its number in the warning colour.
- D? (T4): warning colour = `theme.warning.fg` painted on the percent only (tab background kept); `tab_warning` (red, exited sessions) is not reused.
- D? (T4): 90 % notice (warning level): `claude@work used 91% of its 5-hour limit (resets 15:00)` / `… of its weekly limit (resets Dec 3 15:00)`; once per session per window, a window being its reset time (a report without a reset counts as the window already warned); kept across a restart on the same account. T6 should reset `AiSession::high_warned` when it moves a session to another account.
- D? (T4): `/ai usage` → info notice `claude@work: 5-hour 63% (resets 15:00) · weekly 41% (resets Dec 3 15:00)`, a window not sent → `weekly not reported`; errors in the palette: `/ai usage: no AI session is active`, `/ai usage: no usage reported yet for claude`, `/ai usage takes no argument`. The default account shows as the bare tool (`claude`), as in the tab and in T5's limit notice.

## Deviations / out of scope
- `usage::reset_clock` and `civil_from_days` repeat `limits::reset_text` / `limits::civil` (limits.rs was read-only for this task). Follow-up for whoever next owns limits.rs: make `reset_text(at, now)` call `usage::reset_clock(at, now, Zone::local())` and `civil` call `usage::civil_from_days`.
- An `ai_tools` entry named `usage` (or `rename`) cannot be started with `/ai usage`; same rule as the existing `rename` word.
- Status bar keeps the plain label (no percent); not in the plan.

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| session usage | T3 `AiSession::usage`, `usage::Usage/Window` | reused |
| read loop hook | `ai::read_usage` (D16) | extended: `warn_high` after `limits::on_report` |
| local reset time, D26 format | `limits::reset_text` (private, limits.rs read-only) | rebuilt as `usage::reset_clock(at, now, &Zone)` with `limits::Zone`; duplication noted |
| account label `claude@work` | `limits::account_label` (private) | `usage::account_name` (same rule) |
| warning colour | `Theme.warning` (all four themes) | reused |
| tab fit levels | `ui::ai_view::tab_bar` | extended (suffix, fallback) |
| subcommand parsing | `fn ai` `rename` branch | same pattern |
| notices | `App::warn` / `App::info` | reused |
| test fakes | `usage_app`, `STATUS_AGENT`, `panel_app`, `slash_tests::{slash,key,error,notices}` | reused |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| AC-4 | pass | `label_shows_usage_suffix`, `label_weekly_when_higher`, `notice_once_at_90_percent`, `ai_usage_command_lists_windows`, `ai_usage_without_data`, `usage_suffix_takes_the_higher_window` |
| REAL | pass | snapshots `ai_usage_{120x40,80x24,40x12}`; `bar_fits` at 120/80/40/30/25/20/16 cols: same tabs and `+` as plain labels |
| EASE | pass | `/ai usage` is one palette command; tab percent visible without action; warning colour from `theme.warning` in all four themes (`label_weekly_when_higher`) |
| USER-2 | pass | review on terminal snapshots (text renders above), no HTML |
| USER-3 | pass | no mockup round; built directly |
| USER-5 | pass | no keybinding added (`git diff -- crates/vr-config` empty); slash command only |
| BASE-1 | pass | diff limited to the row's files + tests/snapshots + capabilities.md; limits.rs untouched (`git diff --stat -- …/limits.rs` empty) |
| BASE-2 | pass | `usage_suffix(u: &Usage) -> Option<(String, bool)>` as planned; label placement deviation recorded as D? (T4) |
| BASE-3 | pass | red run: E0425 for `usage_suffix`/`reset_clock`/`usage_text` before code |
| BASE-4 | pass | outputs above, logs in qa-artifacts/T4 |
| BASE-5 | pass | clippy via run-heavy.sh; related tests direct as the brief says; nothing left running |
| BASE-6 | pass | English, why-comments only |
| BASE-7 | pass | no commit, nothing staged |
| BASE-8 | n-a | no dependency added |
| BASE-9 | pass | `/ai usage` rejects extra args; texts built only from validated report fields and validated account names (no screen text) |
| BASE-10 | pass | related filters only (ai_, ai_view, slash, usage, label_, notice_once, limits) |
| BASE-11 | pass | no stash/checkout/reset |
| BASE-12 | pass | nothing written outside the repo (scratch only under .ai-dev-team) |
| BASE-13 | pass | deleted only my own stale `ai_usage_120x40.snap.new` from the pre-generation failing run |
| BASE-14 | pass | all commands foreground and finished; no waits |
| BASE-15 | pass | no process stopped |
| BASE-16 | pass | Reuse table |
| BASE-17 | pass | nothing installed |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | notes created before the first change, progress lines above |
| BASE-20 | pass | removed only README sentence "No usage tracking"; `TabText` tuple replaced, its only users in ai_view.rs updated |
| BASE-21 | n-a | no secrets in output |
| BASE-22 | pass | test names follow neighbours (`ai_usage_*`, `label_*`), snapshot names `ai_usage_<w>x<h>` |
| BASE-23 | pass | notice tested through the real report path (background_pass); `/ai usage` tested through the palette |
| BASE-24 | pass | no git index changes |
| BASE-25 | pass | guard `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings` rc=0 |
| BASE-26 | n-a | not a rewrite |
| BASE-27 | n-a | no shared data; tests use temp dirs |
| BASE-28 | n-a | no third-party code |
| BASE-29 | n-a | no browser session |
| BASE-30 | n-a | no shared app records |
| BASE-31 | n-a | no containers |
| BASE-32 | pass | D13–D18 untouched; D26 format in `reset_clock`; D30 zone via `limits::Zone::local()` |
| BASE-33 | n-a | no external system state |
| BASE-34 | n-a | no blocked step (one hook refusal of a loop command; rerun as single commands) |
| BASE-35 | pass | USER-2/3/5 rows above |
| BASE-36 | pass | within the AI panel scope |
| BASE-37 | n-a | not a defect fix |
| BASE-38 | pass | `cargo fmt -p vr-ide` changed only my files (git status shows only my files) |
| BASE-39 | n-a | no schema |
| BASE-40 | n-a | no outward writes |
| BASE-41 | n-a | no credentials |
| BASE-42 | n-a | no test settings changed |
| BASE-43 | n-a | no access rule |
| BASE-44 | n-a | no fake data adapter flow |
| BASE-45 | pass | AC-4 text identical in 01_spec.md |
| BASE-46 | n-a | no runtime walk; evidence is tests on the final code |
| BASE-47 | pass | `## Readers` not needed for a new value; `label()` readers listed in D? (T4) (notices, diff tab names, review, status bar) kept plain |
| BASE-48 | n-a | no machine state claimed |
| BASE-49 | pass | claude/codex never run; fake agents only |
| COST-1 | pass | clippy via run-heavy.sh |
| COST-2 | pass | only own tests |
| COST-3 | pass | no polling |
| COST-4 | pass | no full suite |
| COST-5 | pass | nothing started |
| COST-6 | pass | only the AI tab bar and `/ai usage` checked |
| COST-7 | n-a | no web/API |
| COST-8 | pass | edits with Edit, batched reads |
| COST-9 | pass | nothing left running |
| COST-10 | pass | related set includes ai_view_tests, ai_changes_view_tests (tab bar), ai_tests limits tests (read_usage), slash docs test |
| COST-11 | n-a | no e2e |
| COST-12 | n-a | leader rule |
| COST-13 | pass | logs under qa-artifacts/T4; nothing in $TEAM_TOOLS |
| COST-14 | n-a | no CI waits |
| COST-15 | pass | snapshots generated once with `INSTA_UPDATE=always` on `ai_usage_label_sizes` after the code was final; only new files, no existing snapshot changed |
| COST-16 | pass | final runs logged to qa-artifacts/T4; only summary lines read |
| COST-17 | n-a | no merge |
| COST-18 | n-a | no full run |
| COST-19 | n-a | qc rule |
| COST-20 | n-a | no re-check |
| COST-21 | n-a | qc rule |

Nothing left running.

## Fix round 1
- read: findings 1 (BASE-16 duplicates) and 2 (D39 `!` mark); limits.rs callers of civil / account_label / reset_text: limits.rs:739, 1160, 1164 only (limits_tests use none of them).
- finding 1 (BASE-16, D38): fixed. limits.rs drops `civil`, `account_label` and `reset_text` (-41 lines). Its date parse now calls `usage::civil_from_days`, and the limit notice calls `usage::account_name` and `usage::reset_clock(t, secs, Zone::local())`. The limit notice text is unchanged: `ai_limit_counts_after_a_quiet_second_with_one_notice_per_event` still expects `claude hit its usage limit (resets 15:00)` and passes. All 24 `ai::limits::tests` pass. `limits::MONTHS` (lowercase, used to parse screen dates) stays.
- finding 2 (D39): fixed. In `ui/ai_view.rs` `tab_text`, a tab at 90 % or more whose percent is not shown gets `WARN_MARK` `!` after its number (` 2! `). Only the `!` takes `theme.warning.fg`; the number no longer does. The mark is part of the tab text, so the fit levels count its width.
  - red: `ai_usage_label_sizes` updated first (expects ` 2! ` and the `!` cell in the warning colour). It failed with `left: " 2 "`.
  - green after the code.
  - `bar_fits` now allows one column per `!` mark: the tab count with percents + marks >= the plain tab count, and the `+` check applies only when there is no mark.
  - No snapshot changed: in the 80x24 snapshot the 95 % tab is the active one, so the mark does not show there. No regeneration was needed.
  - README: one line (`2!`).
- related: `cargo test -p vr-ide --lib -- limits:: usage ai_ ai_view slash label_ notice_once` gave `test result: ok. 224 passed; 0 failed; 1 ignored` (log `qa-artifacts/T4/r1-related-tests.log`).
- guard: `cargo fmt --all --check` rc=0. `run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings` rc=0 (`r1-clippy.log`). The first clippy run flagged `bool_assert_comparison` in my test; I removed that redundant assert, because the 63 % case is checked with that tab inactive.

### Updated rows (round 1)
| Rule | Status | Evidence |
|---|---|---|
| BASE-16 | pass | one copy each of `civil_from_days`, `account_name`, `reset_clock` in usage.rs, used by limits.rs (`grep -c "fn civil\|fn reset_text\|fn account_label" limits.rs` = 0) |
| BASE-1 | pass | limits.rs edited under D38 (lifted lock) |
| BASE-32 | pass | D38, D39 applied |
| BASE-20 | pass | removed functions had only the three call sites in limits.rs, all replaced; limits_tests use none of them |
| REAL / EASE | pass | ` 2! `: the warning is shown by a glyph, not colour alone (`ai_usage_label_sizes`) |
| COST-15 | pass | no snapshot changed in this round |
| BASE-3 | pass | red `left: " 2 "` before the code |
| BASE-25 | pass | guard rc=0 |
Nothing left running.
