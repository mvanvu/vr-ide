# FR-27 File blame annotations toggle in the editor (D321)
Worktree: .worktrees/FR-27 (branch fr-27 from adccb95). Skills: ai-dev-team:implementation.
Tools available: Bash, Read, Edit, Write, Skill, ToolSearch (deferred: TaskStop, LSP, ...), Artifact (unused).

## Progress
- 2026-11-27 11:15:12 notes created; read spec AC-B1..B5, D321/D310/D308/D297/D311, USER-5, rules_run3.
- 2026-11-27 11:19:50 red: vr-git tests added (read.rs file_blame_matches_porcelain..., security.rs file_blame_runs_under_the_user_timeout + file_blame in read_everything/too-large, worker.rs file_blame_request..., blame.rs parse bounds) — fail to compile: no file_blame / FileBlame / user_timeout / parse_blame_bounded / MAX_FIELD (log qa-artifacts/FR-27/red-git.log)
- 2026-11-27 11:20:41 green: vr-git file_blame/user timeout/bounded parse/FileBlame request (cargo test -p vr-git all ok)
- 2026-11-27 11:23:22 red: crates/vr-ide/src/blame_column_tests.rs (11 tests) fails to compile — no editor.toggle_blame / annotations_* / blame_width / remap / Ann / utc_date / file_blame_requests
- 2026-11-27 11:25:31 code: git.rs column state/scheduling/remap/labels, editor_view column, app.rs mouse offset, edit.rs command, keymap shift+f2; 9/11 blame_column tests green (mouse test expectation fixed; snapshots pending)
- 2026-11-27 11:27:06 docs (README Git, docs/keymap.md, docs/commands.md); fmt on changed files; clippy clean; snapshots render_blame_column_{120x40,80x24,40x12} generated (second pass only to drop the real-clock notice from the picture)
- 2026-11-27 11:2x walk 1 on the release binary: untracked notice hidden behind "Blame annotations on (+1 more)" → fixed (reason notice replaces the "on" notice, `git::blame_infos`), unit assertion added in `outside_a_repo_and_untracked_files_say_so_and_show_nothing`; walk 2 all PASS
- 2026-11-27 11:31 final related tests, guard, walk green (below)

## Summary
- Files changed: crates/vr-git/src/{blame.rs,cmd.rs,diff.rs,lib.rs,worker.rs}, crates/vr-git/tests/{read.rs,security.rs,worker.rs}, crates/vr-ide/src/{git.rs,ui/editor_view.rs,app.rs,commands/edit.rs,ui/explorer.rs (fit → pub(crate)),lib.rs,app_tests.rs,git_tests.rs (helpers pub(crate))}, crates/vr-config/defaults/keymap.toml, crates/vr-config/tests/config.rs, README.md (Git), docs/keymap.md, docs/commands.md
- New: crates/vr-ide/src/blame_column_tests.rs (12 tests), snapshots crates/vr-ide/src/snapshots/vr_ide__blame_column_tests__render_blame_column_{120x40,80x24,40x12}.snap
- Key chosen: **`shift+f2`** → `editor.toggle_blame` (global table), palette title "Toggle Blame Annotations". Record as `D? (FR-27)`: USER-5 — an F-key with shift (no ctrl+letter is free: all bound, removed (i/t/u) or legacy-ambiguous (j/m)); legacy VTE sends ESC[1;2Q which crossterm reads as shift+F2 (unlike shift+f3 = cursor report, D264/D279) — proven in the pty walk on the real binary and `legacy_shift_f2_reaches_the_toggle`; not a GNOME Terminal/desktop key; not bound before and not in REMOVED_V2; one key per command (app_tests `every_keyset_v2_key_runs_its_command`, the ctrl+shift / F10/F11 bindings check, config.rs `defaults_load_without_user_file`). Command id is not a slash command (FR-20 test `inline_blame_is_switched_by_the_setting_and_there_is_no_git_slash_command` still green).

## Design
- vr-git: `Repo::file_blame(path, buffer)` = the old `blame_text` recovered from 0ed1ab7^ (blame `--porcelain --contents -` of the editor text, same `blame_inner` as inline blame) with the recovered `Options::user_timeout` (30 s) / `Git::user()`; `Request::FileBlame` (cache key kind 4, `Reply::Blame`); `parse_blame_bounded(out, max_lines)` replaces `parse_blame` for both blames: line numbers must be 1..=lines of the sent text, never more entries than lines, author/e-mail/summary cut to 1024 bytes on a char boundary, filename/previous over 16 KiB refused. `vr_git::equal_lines(old, new)` (similar line diff, 100 ms deadline) for the remap.
- vr-ide git.rs: `annotations` flag (session, every tab); per-path `FileBlame {serial, rope, version, lines: Vec<Ann>, labels (sanitized author + UTC date), stamp (DiskStamp at ask time), stale}`; asked once per (serial, disk stamp) — a save changes the stamp → asked again; fs events of the file / `.git` events mark it stale → asked again; edits never ask (test asserts `file_blame_requests()` unchanged). `sync_annotations` (each tick and before drawing) moves annotations through edits with `remap`: common prefix/suffix of the two ropes (vr_syntax `Edit::diff_in`, the same one the highlighter uses) bounds the changed lines, which are matched by `equal_lines` (≤ 20 000 lines, else all "not committed"); lines outside keep theirs.
- editor_view: column of 24 cells (author ≤12 + date) when ≥48 text cells stay, 18 (author ≤6 + date) when ≥32 stay, else hidden; drawn at the left in `theme.git_blame` (existing token, D311 style), line numbers and gutter marks shift right; `App::char_at_screen` adds the same width (mouse test). Label on the first line of a run and on the top visible row; `┊` on the rest; "Not committed yet" / "Uncommitted" (compact). No soft wrap exists in the editor (horizontal scroll only), so AC-B4's wrap clause has nothing to apply to; horizontal scroll keeps the column fixed.
- Dates: UTC YYYY-MM-DD per D308.
- Notices (info, once per path and reason while on): not in a git repository, not tracked by git, too large to blame, not UTF-8 text; a failed read is a warning with git's text sanitized. A reason notice replaces the plain "Blame annotations on" notice.

## Deviations
- Label also on the top visible row when a run starts above the screen (so a scrolled view never shows only marks) — beyond "first line of each run".
- Read-only (non-UTF-8) documents show no column with a notice (git compares UTF-8 text only, same as the gutter).
- Files with a lone CR: the rope counts it as a line break, git does not, so annotations can shift on such files (same limit as the gutter's line model).
- One release build ran without run-heavy (first walk build), the second through it.

## Out of scope noticed
- `ui::palette::ellipsize` and `ui::explorer::fit` are duplicates; reused `fit` (made pub(crate)), not merged (palette is FR-25's file).
- capabilities.md not edited (brief: only notes in the main tree). Proposed entries: `vr_git::Repo::file_blame` (user-timeout whole-file blame), `vr_git::equal_lines` (line mapping old→new), `crate::git::utc_date`, `crate::git::remap` (carry per-line data through edits).

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| whole-file blame | history `git log -- crates/vr-git/src/blame.rs`, 0ed1ab7^ | extended crates/vr-git/src/blame.rs (`blame_text` recovered as `file_blame`, shares `blame_inner`) |
| user timeout | 0ed1ab7^ cmd.rs | recovered `user_timeout` / `Git::user()` |
| hardened runner | crates/vr-git/src/cmd.rs | reused unchanged |
| worker / cache / coalescing | crates/vr-git/src/worker.rs | extended (`Request::FileBlame`) |
| porcelain parse | blame.rs `parse_blame` | extended into `parse_blame_bounded` |
| sanitizing | ui/safe_text.rs `sanitize` | reused |
| cut to width | ui/explorer.rs `fit`, ui/palette.rs `ellipsize` | reused `fit` (pub(crate)) |
| changed span between two ropes | vr_syntax `Edit::diff_in` (syntax.rs) | reused |
| line diff | vr-git `similar` (diff.rs) | new `equal_lines` beside `line_changes_between` |
| repo per root, serials, pending map, debounce, notices | crates/vr-ide/src/git.rs | extended |
| theme token | theme.rs `git_blame` | reused (no new token) |
| test fixtures | git_tests.rs `Fixture`, `single`, `wait`, `draw`, `row`, `wait_marks`; app_tests `legacy_event`, `space_i` | reused (helpers made pub(crate)) |
| pty walk | qa-artifacts/FR-20/qc_walk.py, T10/vt.py | reused pattern + vt.Screen |

## Security (read path)
Covered M7 threat-model lines for the new read: SEC-1 (absolute git, own runner, new session/no tty — same `Git::run`), SEC-2 (env allowlist — same runner), SEC-3 (FIXED_CONFIG: no hooks, filters, fsmonitor, textconv, pager — same runner; `security.rs::read_everything` now also calls `file_blame`, so every hostile-config test covers it), SEC-4 (`--porcelain` only; `--no-pager`, `--literal-pathspecs` by the runner; config cannot reshape output — `user_and_repo_config_cannot_reshape_parsed_output` unchanged), SEC-5 (path after `--`, repo-relative via `relative()`, literal pathspecs), SEC-6 (repo from discovery only), SEC-7 (user timeout kills the git group: `file_blame_runs_under_the_user_timeout`; `max_file` cap before git: `file_over_the_cap_is_too_large` + UI-side "too large" without asking; `max_output` cap; bounded parse: `parse_refuses_line_numbers_past_the_text`, `parse_refuses_more_lines_than_the_text_has`, `parse_cuts_overlong_fields`), SEC-8 (reads write nothing under .git — `reads_write_nothing_under_dot_git_and_respect_index_lock` runs `file_blame` too), SEC-15 (author sanitized at arrival, git error text sanitized: `author_names_from_git_are_sanitized`), SEC-17 (results bound to repo serial and to the rope/version they describe; drawn only when serial matches the path's repo and the version is current).

## New states
- `annotations` on/off: quit/shutdown clears per-file data (`GitState::shutdown`); off → nothing drawn at once (test); a second toggle re-asks nothing unless stale.
- `FileBlame` per path: tab close/open (`forget_doc`) drops it; file changed on disk / `.git` change → stale, asked again; save → new stamp → asked again; superseded request → newest wins (worker); hung git → user timeout → warning once, no column; repo removed → serial mismatch → not drawn.
- Pending `FileBlame` reply for an older text → remapped from that text to the current one before drawing.

## Commands run (trimmed)
- red: `cargo test -p vr-git --no-run` → E0425 parse_blame_bounded / MAX_FIELD …; `cargo test -p vr-ide --lib blame_column --no-run` → E0425 annotations_on / blame_width / remap / utc_date …
- `run-heavy.sh cargo test -p vr-git`: lib 11 passed; read 14 passed; security 27 passed; worker 5 passed; bench 1 ignored
- `run-heavy.sh cargo test -p vr-config --test config`: 23 passed
- `run-heavy.sh cargo test -p vr-ide --lib -- blame_column git_tests app_tests mode_tests ui:: syntax_tests find_tests explorer_tests language_snapshot slash_tests slash2_tests commands::`: `test result: ok. 351 passed; 0 failed; 7 ignored; 260 filtered out`; after the last comment-only edit `blame_column`: 12 passed
- `rule-checks.sh related-tests FR-27`: config.rs, vr-git tests bench/common/read/security/worker, ui/tests/preview.rs + welcome snapshots/welcome_tests.rs, vr-syntax/tests/highlight.rs — all run above except vr-syntax/tests/highlight.rs (vr-syntax not changed; listed because `Edit` is now also used from git.rs)
- guard `run-heavy.sh bash -c 'cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings'` → exit 0, `Finished dev profile`
- snapshots regenerated with INSTA_UPDATE=always on `render_blame_column` only (second pass right after the first to clear a real-clock notice from the picture); no other snapshot changed (INSTA_UPDATE=no run green)

## Walk (real binary target/release/vr-ide of this worktree, final code; script qa-artifacts/FR-27/walk.py, output walk-result.txt)
PASS read-mode-toggle-on 'Quinn Check… 2026-11-27   1 alpha'
PASS read-mode-continuation '┊                         2 beta'
PASS status-read
PASS edit-mode-new-line 'Not committed yet         1▎new'
PASS edit-mode-kept-line 'Quinn Check… 2026-11-27   2 alpha'
PASS save-rereads (file saved; row1 Not committed yet / row2 Quinn Check…)
PASS toggle-off '  1▎new'
PASS untracked-notice (notice line: 'Blame annotations: u.txt is not tracked by git')
PASS narrow-40-hidden '  1▎new'
PASS width-60-compact 'Quinn… 2026-11-27   2 alpha'
throwaway repo removed: True (qa-artifacts/FR-27/repo and home deleted by the script)

## Entry paths (BASE-23)
key shift+f2 (legacy bytes + crossterm event, read and edit mode), palette title, `run_command`; read mode, edit mode; several tabs (`the_state_holds_for_every_tab_of_the_session`); widths 120/80/60/40 (tests + snapshots + walk); untracked / outside repo / too large; save.

## Edit methods
Edit tool, `perl -pi/-0pi -e` substitutions, heredoc appends for new tests, one Write per new file; no python3 heredoc edits.

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | diff limited to the brief's scope list; out-of-scope notes above |
| BASE-2 | pass | command id `editor.toggle_blame`, title "Toggle Blame Annotations" (`the_palette_title_and_default_key_run_the_toggle`); vr-git API additive |
| BASE-3 | pass | red logs above (compile-red for new API), walk-1 red → fix → test assertion for the notice |
| BASE-4 | pass | trimmed outputs above |
| BASE-5 | pass | tests/guard via run-heavy; first release build not (noted under Deviations); nothing left running |
| BASE-6 | pass | English, why-comments |
| BASE-7 | pass | no commit |
| BASE-8 | pass | no new dependency (similar already in vr-git, vr-syntax already in vr-ide) |
| BASE-9 | pass | Security section |
| BASE-10 | pass | related modules + mapped guard only |
| BASE-11 | pass | worktree only; no checkout/stash/reset |
| BASE-14 | pass | foreground run-heavy; no waits |
| BASE-15 | pass | no background tasks; walk kills only its own pty children |
| BASE-16 | pass | Reuse table |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | progress lines above |
| BASE-20 | pass | nothing removed (`parse_blame` renamed: `grep -n parse_blame crates/vr-git` → only the bounded one) |
| BASE-22 | pass | `editor.toggle_blame` follows `editor.edit_mode`/`editor.read_mode`; `user_timeout` beside `timeout`; `FileBlame` beside `InlineBlame` |
| BASE-23 | pass | Entry paths section |
| BASE-24 | pass | no index commands |
| BASE-25 | pass | guard exit 0 on final code |
| BASE-32 | pass | D310 (no /git command, no popup), D321 (layout/behaviour), D308 (UTC dates), D311 (existing theme token), D297 inline blame untouched |
| BASE-35 | pass | USER-5 key analysis above; USER-1 superseded; USER-3 no mockups; USER-4 nothing outside vr-ide |
| BASE-45 | pass | AC-B1..B5 exist in 01_spec_FR17-21.md |
| BASE-46 | pass | walk ran on the final runtime code (after the notice fix; later edits were test comments only) |
| BASE-47 | n-a | no existing value's computation changed (new column) |
| BASE-48 | n-a | no machine state claimed |
n-a: BASE-12, BASE-13, BASE-17, BASE-21, BASE-26, BASE-27, BASE-28, BASE-29, BASE-30, BASE-31, BASE-33, BASE-34, BASE-36, BASE-37, BASE-38 (pass: fmt only on changed files), BASE-39, BASE-40, BASE-41, BASE-42, BASE-43, BASE-44 — no other repos, untracked deletions (only the walk's own throwaway repo), shared tools, secrets, rewrites, shared DBs, third-party code, browsers, containers, target envs, permission blocks, app scope changes, schema, outward writes, credentials, test-runner settings, access rules or fake adapters in this task. BASE-38 is a pass: `cargo fmt -- <changed files>` only.
| COST-1 | pass | run-heavy for suites/guard (one build exception noted) |
| COST-2 | pass | developer tests + walk only |
| COST-3 | pass | no polling loops (test `wait` helper is an existing in-test tick loop) |
| COST-4 | pass | no full suite |
| COST-5 | pass | pty children killed by the script |
| COST-8 | pass | batched edits; edit methods listed |
| COST-9 | pass | nothing running |
| COST-10 | pass | related-tests output above |
| COST-13 | pass | script in qa-artifacts/FR-27; vt.py from T10 by path |
| COST-15 | pass | snapshots regenerated after final rendering code (see note) |
| COST-16 | pass | logs in qa-artifacts/FR-27/*.log, read by grep/tail |
| COST-19 | pass | walk.py is the check set |
n-a: COST-6, COST-7, COST-11, COST-12, COST-14, COST-17, COST-18, COST-20 — no audits, browsers, e2e, leader batching, CI waits, merges of goldens, full runs or qc re-checks here.

## Fix round 1
Fresh developer (opus), continuing from the notes above. Tools: Bash, Read, Edit, Write, Skill, ToolSearch (deferred TaskStop etc.).
- 2026-11-27 11:42:30 read review 05_review_FR-27.md, D326, notes; starting
- 2026-11-27 11:47:19 fix 1 vr-git: red (3 worker tests time out at 20 s, log qa-artifacts/FR-27/r1-red-worker.log) → green: cancellable Limits + FileBlame lane (cargo test -p vr-git all ok, log r1-git.log)
- 2026-11-27 11:5x fix 2/4 + IDE side of fix 1: red (blame_column: lone CR row 4 shows Bob on "two"; closed-tab reply kept; "timed out waiting for big asked again", log r1-red-ide.log) → green (r1-ide.log)
- 2026-11-27 11:5x fix 3 (civil_from_days), fix 5 (docs), fmt on changed files, guard exit 0, release build, walk 10/10, qc_walk 23/23

### Replies per finding
1. major (FileBlame holds back automatic reads) — fixed. vr-git: `GitWorker` now runs whole-file blames on a second thread, the **lane** (`crates/vr-git/src/worker.rs` `Lane`/`run_lane`): at most one waiting and one running; a newer `FileBlame` (any path) replaces the waiting one (answered `Superseded`) and kills the running one. New `GitWorker::cancel_file_blame(id)` drops / kills one request. Kill path: `Limits.cancel` (`Arc<AtomicBool>`, `crates/vr-git/src/cmd.rs`) checked before spawn and on every 50 ms wait tick → git group killed, `GitError::Superseded`; `Repo::file_blame_cancellable` (`blame.rs`). The runner's 2 slots cover main + lane. Lane is uncached (the column asks once per saved state). vr-ide (`git.rs`): `cancel_unneeded_file_blames` each tick cancels every pending FileBlame when the column is off, else those of non-active files; a dropped request clears `asked_file_blame` so the file is asked again when shown. Tests: vr-git `automatic_reads_answer_while_a_file_blame_runs` (shim sleeps 30 s on `blame*slow.txt`; LineChanges + Head answer < 5 s while it runs), `a_newer_file_blame_cancels_the_running_one`, `a_cancelled_file_blame_stops_its_git`; vr-ide `a_slow_blame_of_another_tab_never_holds_back_this_one` (column + gutter marks of the new tab < 5 s; old tab's blame dropped and asked again on return). Not done (optional in review): serving InlineBlame from FileBlame.
2. major (lone CR shift) — fixed per D326: `build_file_blame` walks the rope lines and advances the git line only after an editor line ending in `\n`; `fix_last` also blanks the empty line after a final lone `\r`. Test `a_lone_cr_keeps_every_annotation_on_its_line`.
3. minor (BASE-16 date math) — fixed: `crate::recovery::civil_from_days` shared by `recovery::utc_stamp` and `git::utc_date` (tests `utc_dates`, recovery's `utc_stamp` asserts green).
4. minor (late reply after close) — fixed: `drain_responses` keeps a FileBlame answer only when a document with that path is open and `asked_file_blame[path]` holds this request id (`asked_file_blame` now `(serial, stamp, id)`, `GitState::request` returns the id). Test `a_reply_for_a_closed_tab_is_not_kept`.
5. minor (docs thresholds) — fixed: docs/commands.md states the thresholds as text cells beside the line numbers (72 / 50), "about 76 columns with no side panel; panels and a wider gutter take more".

### New states (round 1)
- Lane `queued` / `running` / `dropped` / `closed`:
  - quit / worker dropped: `Lane::close` sets the running cancel flag (git killed) and the thread exits; a request still queued is answered `ShutDown` then the thread ends; `Repo::shutdown` still kills everything (`a_hung_discovery_git_is_killed_at_exit`, IDE test ends with `git.shutdown()`).
  - cancel / tab switch / column off: running git killed within one 50 ms tick (`a_cancelled_file_blame_stops_its_git`, < 5 s); queued one answered `Superseded` without running.
  - second request: replaces the queued one, kills the running one (`a_newer_file_blame_cancels_the_running_one`).
  - error halfway: a failed / timed-out blame is answered as before (user timeout test `file_blame_runs_under_the_user_timeout` unchanged, green).
  - input changes: newest text wins (same as superseding).
  - call that never returns: the user timeout (30 s) or a cancel kills the group; automatic reads never wait for it (`automatic_reads_answer_while_a_file_blame_runs`).
  - a cancel racing a finished blame: the finished answer is sent; the UI keeps it only if it is the path's last request and the tab is open.
- `asked_file_blame` request id: a dropped request clears the entry (asked again when shown, test above); answers of older requests or closed tabs ignored (test above).

### Commands (round 1, trimmed)
- red vr-git `run-heavy.sh cargo test -p vr-git --test worker` (stub `cancel_file_blame`): `automatic_reads_answer_while_a_file_blame_runs ... FAILED`, `a_newer_file_blame_cancels_the_running_one ... FAILED`, `a_cancelled_file_blame_stops_its_git ... FAILED` (`no response to N` after 20 s) — qa-artifacts/FR-27/r1-red-worker.log
- red vr-ide `run-heavy.sh cargo test -p vr-ide --lib -- blame_column`: lone CR `Bob Builder… 2023-11-14   3 two` on row 3; `a late answer came back for a closed tab`; `timed out waiting for big asked again` — 12 passed; 3 failed — r1-red-ide.log
- `run-heavy.sh cargo test -p vr-git`: lib 11, read 14, security 27, worker 8 passed (bench 1 ignored) — r1-git.log
- `run-heavy.sh cargo test -p vr-ide --lib -- blame_column git_tests recovery`: `test result: ok. 55 passed; 0 failed` (blame_column 15, git_tests 26, recovery 14) — r1-ide.log; snapshots unchanged (no new .snap.new)
- guard `run-heavy.sh bash -c 'cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings'` → exit 0, `Finished dev profile` — r1-guard.log
- `run-heavy.sh cargo build --release -p vr-ide` → Finished — r1-build.log
- walk (BASE-37/46) `python3 walk.py target/release/vr-ide` on the rebuilt binary: 10/10 PASS, throwaway repo removed — r1-walk-result.txt
- QC script `python3 qc_walk.py target/release/vr-ide`: 23/23 PASS, throwaway removed — r1-qc_walk-result.txt
- nothing left running (`ps` shows no vr-ide of this worktree; no background tasks started)

### Files changed in round 1
crates/vr-git/src/{cmd.rs,blame.rs,worker.rs}, crates/vr-git/tests/worker.rs, crates/vr-ide/src/{git.rs,recovery.rs,git_tests.rs (git_app_with, single_with, shim_git pub(crate)),blame_column_tests.rs (3 tests)}, docs/commands.md

### Reuse (round 1)
| Needed | Looked in | Result |
|---|---|---|
| kill a running git | cmd.rs `kill_group`, timeout path in `spawn` | reused: cancel ends the wait loop like the timeout |
| date math | recovery.rs `utc_stamp` | extracted `civil_from_days`, used by both |
| slow-git shim | vr-git tests `slow`, vr-ide git_tests `shim_git` | extended (`slow_for`) / reused (`shim_git` pub(crate)) |
Proposed capabilities.md entries: `vr_git::GitWorker::cancel_file_blame`, `vr_git::Repo::file_blame_cancellable`, `crate::recovery::civil_from_days`.

### Edit methods (round 1)
Edit tool for code; `perl -0pi` substitutions in two places, both of which mangled a line through the `|` delimiter and were repaired at once with `sed` line edits (diff checked); heredoc appends for new tests; no python3 heredoc edits.

### Rule compliance rows touched in round 1
| Rule | Status | Evidence |
|---|---|---|
| BASE-3 | pass | behaviour-red logs r1-red-worker.log, r1-red-ide.log before the fixes |
| BASE-4 | pass | trimmed outputs above |
| BASE-9 | pass | SEC-7: user read on its own lane, cancellable, automatic reads never wait (tests above); SEC-17: answers bound to the path's last request id and an open tab |
| BASE-16 | pass | `civil_from_days` shared; Reuse (round 1) |
| BASE-25 | pass | guard exit 0 on final code (r1-guard.log) |
| BASE-32 | pass | D326 (lone CR annotated correctly, own lane, superseded/non-active cancelled), D308 UTC dates unchanged, D310 no git command added |
| BASE-37 | pass | walk + qc_walk replayed on the rebuilt release binary after the fix |
| BASE-46 | pass | walk 10/10, qc_walk 23/23 on final code |
| BASE-47 | pass | `utc_date` / `utc_stamp` computation moved, not changed: `utc_dates` + recovery stamp asserts green |
| COST-8 | pass | batched edits; edit methods listed (two perl mishaps repaired) |
| COST-15 | pass | no snapshot regenerated (rendering unchanged; insta run green) |
| COST-16 | pass | logs in qa-artifacts/FR-27/r1-*.log read by grep |

## Fix round 2
- 2026-11-27 11:56:08 read Re-check round 1 (same-file re-asks kill the running blame forever); starting
- red: vr-git `newer_file_blames_of_the_same_file_wait_for_the_running_one` → `the running blame was killed: Err(Superseded)` (r2-red-worker.log); vr-ide `a_busy_git_folder_never_keeps_a_slow_blame_from_showing` → `the column never appeared` after 12 s of `.git` writes every 300 ms against a 2 s blame (r2-red-ide.log)
- green, guard, release build, walk (below)

### Reply to the round-1 re-check finding (busy `.git` restarts a slow blame forever) — fixed
- `crates/vr-git/src/worker.rs` `Lane::submit`: a newer FileBlame for the **same file** as the running one no longer kills it; it is queued (replacing any queued one, answered `Superseded`) and runs afterwards. A request for another file still kills the running one; `cancel_file_blame(id)` and `close` unchanged. `LaneState.running` now holds the path.
- `crates/vr-ide/src/git.rs` `drain_responses`: a FileBlame answer is kept when its tab is open and its repo serial is still the file's, also when it is older than the path's last request; it is then marked `stale` (drawn and remapped from its own text by `sync_annotations`; the newer queued answer replaces it). Only the last request's failure is reported; a dropped (`Superseded`) last request still clears `asked_file_blame` so it is asked again. The answer order is safe because one lane per repo runs blames one after another.
- `GitState::forget_doc` now also drops the pending FileBlame requests of that path (and cancels them), so a closed / reopened tab never gets an older document's answer (keeps round-1 finding 4: `a_reply_for_a_closed_tab_is_not_kept` green).
- Test note: the vr-git test checks first + last answered with a blame, every request answered once, ≥1 waiting request replaced; how many ran in between depends on timing (an exact count failed once, so it was loosened to what the contract promises).

### New states (round 2)
- Queued same-file request behind a running one: quit → `close` kills the running one, queued answered `ShutDown`; tab switch → the IDE cancels both (another path / `cancel_file_blame`); column off → both cancelled; second / third request → replaces the queued one; never-returning git → user timeout 30 s, then the queued one runs; failure of an older request → ignored (the last one reports).
- Older answer marked stale: drawn and remapped; replaced by the newer answer; if the newer was dropped, `asked_file_blame` is cleared and the file asked again.

### Commands (round 2, trimmed)
- `run-heavy.sh cargo test -p vr-git`: lib 11, read 14, security 27 passed; worker 9 passed (r2-git.log, r2-git-worker.log after the assertion change)
- `run-heavy.sh cargo test -p vr-ide --lib -- blame_column git_tests`: `test result: ok. 42 passed; 0 failed` (r2-ide.log)
- guard fmt + clippy `-D warnings` → exit 0, `Finished dev profile` (r2-guard.log)
- `run-heavy.sh cargo build --release -p vr-ide` → Finished; `walk.py` on it 10/10 PASS, throwaway removed (r2-walk-result.txt); nothing left running
- Files changed in round 2: crates/vr-git/src/worker.rs, crates/vr-git/tests/worker.rs, crates/vr-ide/src/git.rs, crates/vr-ide/src/blame_column_tests.rs. Edits with the Edit tool and heredoc appends of the new tests; rustfmt only on these files.

### Rule compliance rows touched in round 2
| Rule | Status | Evidence |
|---|---|---|
| BASE-3 | pass | r2-red-worker.log, r2-red-ide.log before the fix |
| BASE-4 | pass | trimmed outputs above |
| BASE-9 | pass | SEC-7: still at most worker + lane per repo; another file's blame still killed; same-file request waits (bounded by the user timeout) |
| BASE-25 | pass | r2-guard.log exit 0 |
| BASE-32 | pass | D326 (own lane, superseded / non-active cancelled), the leader's round-2 expectation (same file queued, other file cancels) |
| BASE-37 / BASE-46 | pass | walk 10/10 on the rebuilt release binary |
| COST-8 | pass | Edit tool only for code this round |
| COST-15 | pass | no snapshot touched |
