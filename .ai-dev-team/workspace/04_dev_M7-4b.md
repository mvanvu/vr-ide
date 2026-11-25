# Dev notes — M7-4b Git popup History and Blame tabs

Main tree /home/rainy/DEV/vr-ide (master b7f104c). Owner fe. Scope: D297 History + Blame tabs, /git log, /git log file, /git blame, /git show <sha>; AC-16c, AC-16e; SEC-5, SEC-15; M7-3 QC minor (SEC-19 banner wrap / '…', hint row '…'). Nothing committed.
Tools available: Bash, Read, Write, Edit, Skill, Agent, Artifact, ToolSearch, SubagentHandback (+ deferred: Monitor, TaskStop, WebFetch, WebSearch, ...).

## Progress
- read proposal, spec M7 (AC-16c/e), threat model SEC-5/15/19, decisions D297-D307, 04_dev_M7-1, 04_dev_M7-3, QC finding (05_qc_report.md M7-3 finding 1)
- 2026-11-25T22:50:06+07:00 tests first: crates/vr-ide/src/git_history_tests.rs (13 tests: log/--follow, paging, blame vs porcelain, unsaved + Changes selection, slow blame shim, commit view vs git show, syntax colour, blame Enter, SEC-5 show refusals, SEC-15 hostile, keys/mouse, banner/hint QC minor, 9 snapshots); popup test helpers made pub(crate)
- 2026-11-25T22:55:36+07:00 git_history.rs, ui/git_history.rs, popup wiring (Tab, history thread with own runner, keys/mouse, header tabs, banner wrap, hint …), slash /git log|log file|blame|show, Syntax::notifier; builds
- 2026-11-25T22:59:14+07:00 mutations (one build, then restored by copy, cmp checked): Log path dropped -> history test red; buffer ignored -> unsaved-blame test red; summary unsanitized -> hostile test red (+ snapshot); back() disabled -> commit-view test red (blame_enter test gained the same assert); hex refusal off -> SEC-5 test red; banner max_rows=1 stayed green at 80x24 (the short lead-in fits; wrap covered by unit test)
- 2026-11-25T23:00:07+07:00 cargo test -p vr-ide 628/0; snapshots reviewed (4 Changes snapshots: header tabs + hint …; 9 new); fmt own files; workspace clippy -D warnings clean; release build started
- release build (run-heavy) ok; pty walk qa-artifacts/M7-4b/dev/m74b.py 16/26 first: the app's ENTER_SETTLE (Enter within 500 ms of the popup opening is ignored, existing guard) ate the walk's Enter, and one expected string was longer than the 80-column row; walk script fixed (wait 0.8 s, shorter string) -> 26/26
- full suite via run-heavy.sh: see Commands run

## Files
- new: crates/vr-ide/src/git_history.rs (state, history thread, commands, keys, mouse, status line), crates/vr-ide/src/ui/git_history.rs (render), crates/vr-ide/src/git_history_tests.rs (13 tests), 9 snapshots `vr_ide__git_history_tests__render_git_popup_{history,commit,blame}_{120x40,80x24,40x12}.snap`
- changed: crates/vr-ide/src/git_popup.rs (`Tab`, session fields, history thread start/shutdown, keys 1/2/3, Esc back, mouse on tabs, tick), crates/vr-ide/src/ui/git_popup.rs (tab bar, dispatch to the History/Blame body, banner wrap + `…`, hint `…`, `wrap` + unit test, helpers pub(crate)), crates/vr-ide/src/app.rs (message-box Enter rule only on the Changes tab), crates/vr-ide/src/commands/slash.rs (4 commands), crates/vr-ide/src/syntax.rs (`Syntax::notifier`), crates/vr-ide/src/lib.rs, crates/vr-ide/src/ui/mod.rs, crates/vr-ide/src/git_popup_tests.rs (helpers pub(crate); keys test: 2/3 now switch tabs), crates/vr-ide/src/slash_tests.rs (registry list); 4 Changes snapshots (header tab bar, hint `…`) reviewed
- docs: README.md (### Git: tabs, History, Blame, commit view; roadmap line), docs/commands.md (paragraph + 4 table rows), docs/keymap.md (Git popup row); .ai-dev-team/capabilities.md (git_history entry)
- pty walk: .ai-dev-team/workspace/qa-artifacts/M7-4b/dev/{m74b.py, walk.txt, screen_*.txt}

## Design calls
- D? (M7-4b) One history thread per popup session with a git runner of its own (`Writer::open(&repo).repo()`, no git runs to make it): a 3 s blame holds neither the reader's slots nor the writer's; queued requests of one kind are coalesced (only the newest log / blame / show runs; older ones are dropped, their generation is stale). Closing the session (or exit) shuts that runner down, killing a running log/blame at once (reads only). No vr-git change.
- D? (M7-4b) History paging: the log is asked for 200 commits and asked again with 200 more when the selection comes within 20 of the end (git log has no stable offset across new commits; re-running from HEAD keeps the list consistent and keeps the selection by commit id). Only visible rows are drawn. The list reloads by itself when HEAD moves while History shows; `r` reloads.
- D? (M7-4b) `2` shows the last history shown, else the repo's; `/git log` and `/git log file` choose. `3` blames the file selected in Changes when pressed there, else the active file (its unsaved buffer, through `blame --contents -`), else the last blamed file. A file not open is read once (regular file, <= 8 MiB) and that text is blamed, so text and blame always match; non-UTF-8 files are blamed from disk and shown lossily.
- D? (M7-4b) Blame layout: one block per run of lines from the same commit, the first row shows a short age (`now`, `5m`, `3h`, `4d`, `2mo`, `3y`) and the author (column 18/16/10 cells by width, none under 50), the others a `│`; then the line number and the text (left/right scroll, `…` at either cut). Age colours from theme fields only: newest third bold `git_modified`, middle normal text, oldest third `git_blame`, relative to this file's oldest and newest commit. Uncommitted lines: `+ You, uncommitted` in `git_added`. The status row names the selected line's commit (id, author, relative age, subject).
- D? (M7-4b) Commit view: rows like `git show` — `commit <id>`, `Merge: …` (diff against the first parent, as vr-git's `diff_commit`), `Author:`, `Commit:` when the committer differs, `Date: YYYY-MM-DD HH:MM UTC (relative)` (UTC: vr-git's log/commit carry Unix seconds without the author's zone), the message indented by 4, `N files changed` + name-status rows, then each file's hunks. Syntax colours per file: one highlighter over the file's diff lines (both sides, in diff order), made when a row of the file is on screen, at most 8 alive, files over 512 KiB plain; `+`/`-` keep their colours, code without a capture takes the line colour. Scroll only (no cursor): up/down/pageup/pagedown/home/end, left/right sideways; Esc or Backspace back to the list (History, or Blame when opened from a line).
- D? (M7-4b) `/git show <rev>`: the popup opens on History (or stays on Blame), the commit view loads; vr-git resolves the text with `rev-parse --verify -q --end-of-options <rev>^{commit}` on the history thread (SEC-5). A refusal closes the view and is an error notice: `git show: <rev>: no such commit`, and for a 4-39 hex text `… no such commit, or the short id matches more than one commit (type more of it)` (git's -q exit code does not tell the two apart; no vr-git change).
- D? (M7-4b) Keys: `1`/`2`/`3` everywhere in the popup except the Changes message box (there they are text); `esc` closes the commit view first, then the popup; `tab` does nothing in History/Blame; `enter` opens; `r` reloads / blames again. Mouse: a click on a tab name switches, a click selects a commit or a blame line, the wheel moves (3 rows in the commit view). The Changes tab's message-box Enter rule (D307) applies only on that tab.
- D? (M7-4b) Tab bar in the top border: ` Changes  History  Blame ` with the shown one in `tab_active`; when the title would leave fewer than 16 cells, only the shown tab's name. SEC-19 / scan banners show on the Changes tab only (they are about writes; History/Blame get the rows).
- D? (M7-4b) M7-3 QC minor: a banner wider than the popup wraps to a second row (breaking between `, ` items first, so `hook pre-push` stays whole) while at least 5 rows are left above the message box, else one row; if it is still cut, the program banner switches to the short lead-in `! writes run programs: <names>` and whatever is left ends with `…`. The hint row ends with `…` when cut.

## Tests (cargo test -p vr-ide: 628 passed, 0 failed, 7 ignored)
git_history_tests.rs (13) + git_history unit (3) + ui::git_popup `wrap` unit (1):
- `history_of_the_repo_and_of_a_renamed_file_match_git_log`: /git log file on b.txt (was a.txt, other files' commits between) == `git log --follow --format=%H -- b.txt` (4), complete; /git log == `git log --format=%H` (6); screen title, subject, author
- `history_loads_more_commits_while_scrolling`: 450 commits (fast-import): 200, End -> 400 with the selection kept, End -> 450 complete == `git log`
- `blame_matches_git_blame_porcelain_and_marks_uncommitted_lines`: 3 authors + an uncommitted worktree line: commit and author per line == `git blame --porcelain` (Not Committed Yet -> uncommitted); screen shows `You, uncommitted`, `3d`
- `blame_of_unsaved_edits_and_of_the_file_selected_in_changes`: a typed (unsaved) top line is uncommitted, the rest committed, disk unchanged; `1`, then `3` from Changes blames the selected other.txt
- `a_slow_blame_shows_blaming_and_never_blocks_the_keys`: PATH shim git sleeping 2 s before blame: /git blame returns < 500 ms, `blaming…` at 120x40 and 40x12, `2`/`3` switch in < 200 ms while blaming, result arrives
- `the_commit_view_matches_git_show`: Enter on the newest commit: `commit <id>`, `Author:` line, `Date:` == `git show --date=format-local` with TZ=UTC, message rows == `%B`, file rows == `--name-status -M` (M/A/D), +/-/context lines == `git show` hunks; Esc back, Esc closes
- `the_commit_view_colours_the_diff_by_syntax`: `/git show HEAD` on a .rs change: the `pub` cell gets the theme's keyword colour
- `blame_enter_opens_the_lines_commit`: line 2 -> Bea's commit; Esc back to Blame, cursor kept
- `git_show_refuses_unknown_ambiguous_and_option_like_revisions` (SEC-5): `nope` -> "no such commit"; `--output=x` refused, no file x; a 4-hex prefix shared by two of 400 commits -> "more than one"; `HEAD~3` opens == `rev-parse HEAD~3`
- `hostile_authors_messages_and_names_are_drawn_safely_in_every_view` (SEC-15): ESC CSI, OSC 52, C1 CSI, CR, U+202E in author, subject, body, file name and content: no unsafe cell in History, commit view and Blame at 120x40/80x24/40x12; placeholders visible; notices clean
- `number_keys_switch_tabs_but_type_in_the_message_box`: 2/3/1, "fix 123" typed in the message box, click on the History tab name, wheel moves the selection
- `the_program_banner_and_the_hint_end_visibly_on_small_screens`: 80x24 shows `filter.x.clean` and `hook pre-push` (2 rows), the hint ends with `…`; 40x12 names first, `…` when cut
- `render_git_popup_history_blame_and_commit`: 9 snapshots (reviewed: columns by width, `+ You, uncommitted`, `│` block rows, status rows, tab bar; the 40x12 commit view shows the header rows and scrolls for the diff)
- M7-3 file: `keys_stay_in_the_popup_and_esc_keeps_the_draft` now asserts 2/3/1 switch tabs; 4 Changes snapshots updated (tab bar, hint `…`), diffs read
Red evidence: tests written before the code but run first against the finished code (honest: no stub run); mutations instead (one build, restored by copy, cmp checked): Log path dropped -> history test red; buffer ignored -> unsaved-blame test red; summary unsanitized -> hostile test + snapshot red; `back()` disabled -> commit-view test red (the blame Enter test then gained the same assert); hex refusal off -> SEC-5 test red; banner one row only -> stayed green at 80x24 because the short lead-in fits there (wrap covered by its unit test).

## Commands run
- `cargo test -p vr-ide` -> 628 passed, 0 failed, 7 ignored (also after the last change)
- `run-heavy.sh cargo clippy --workspace --all-targets -- -D warnings` -> Finished, no warnings (one collapsible-if in the tests fixed first)
- `cargo fmt --all --check` -> exit 0 (rustfmt on own files only)
- `run-heavy.sh cargo build --release -p vr-ide` -> Finished (binary 23:02, after the last source change)
- pty walk `qa-artifacts/M7-4b/dev/m74b.py target/release/vr-ide` -> 26/26 (walk.txt, screen_*.txt)
- `run-heavy.sh cargo test --workspace -j 4` -> every crate ok except vr-ide 627 passed / 1 failed: `git_tests::each_root_shows_its_own_repo` (M7-4a test, file not touched; the same race M7-3 reported). It passes alone 3/3 and in the two `cargo test -p vr-ide` runs after (628/0). Likely cause for the leader: the test waits for root a's branch (`status_text == "main"`), not for a's marks, before opening b's file, so under load `shown_marks(&pa)` can still be `None` at line 585; a `wait_marks` after `open_path(&pa)` would close it. Not changed here (out of scope).
- Nothing left running: the background build and suite finished; both pty sessions exited (`no process left in the repo` check); temp repos live in tempdirs.

## SEC compliance
| ID | Status | Evidence |
|---|---|---|
| SEC-5 | pass | `/git show` text goes only to vr-git `resolve_commit` (`rev-parse --verify -q --end-of-options <rev>^{commit}`) before `commit`/`diff_commit` use the full id; History/Blame pass only ids from git's output and absolute paths (vr-git: `--end-of-options`, `--`, literal pathspecs). Test `git_show_refuses_unknown_ambiguous_and_option_like_revisions` (unknown, `--output=x` with no file made, ambiguous 4-hex prefix, `HEAD~3`); walk `/git show nope` |
| SEC-7 / AC-16e | pass | all log/blame/show on the session's history thread with its own runner (vr-git timeouts and caps apply); paging 200 at a time; blame file read bounded (regular file, 8 MiB); highlighters only for on-screen files, <= 8, <= 512 KiB; `a_slow_blame_shows_blaming_and_never_blocks_the_keys` + walk (3 s shim: `blaming…`, `2` switches at once) |
| SEC-15 | pass | every git string through `sanitize` (short/full ids, authors, emails, subjects, message lines, paths, hunk headers, diff and blame lines per char, error texts, the typed revision in notices); test at 3 sizes with visible placeholders (mutation red); walk: no raw OSC 52 / RLO bytes in the app's output |
| SEC-17 | pass | History/Blame/show use the popup session's repo (opened by `open(app, "")` like `/git`); a file of another repo gives git's "outside the repository" error, never another repo's data |
| SEC-19 | pass (QC minor fixed) | banner wraps to two rows or uses the short lead-in and ends with `…`; names shown at 80x24, names first at 40x12; `wrap` unit test |
| SEC-20 | pass | no tracing in git_history.rs / ui/git_history.rs; messages and diffs are only drawn |
| SEC-1/2/3/4/8 | pass (vr-git, unchanged) | reads through vr-git's hardened runner (no shell, filtered env, config overrides, no index writes) |

## Reuse
| Need | Found | Decision |
|---|---|---|
| log, commit, diff_commit, blame/blame_text, resolve_commit | vr-git (M7-1) | reused as is, no vr-git change |
| own runner for slow reads | `Writer::open` (makes a runner of its own, runs no git) | reused for the history thread |
| display filter | `ui::safe_text::sanitize` | reused |
| relative age, wall clock in tests | `git::relative_age`, `GitState.now_secs` | reused; added `short_age` for narrow columns |
| UTC date | `recovery::utc_stamp` | reused (`utc_date_time` formats it) |
| syntax colours | vr-syntax `Highlighter`, `Theme::syntax`, the app's registry | reused; `Syntax::notifier()` added so the commit view's spans wake the loop |
| row fitting with `…` | `ui::git_popup::{fit_head, fit_tail, skip_cells, put, width}` | made pub(crate), reused |
| popup tests, hermetic repos | `git_popup_tests` helpers, `git_tests::Fixture` | helpers made pub(crate), reused |
| pty harness | qa-artifacts/FR-10/h.py | reused |
Added to capabilities.md: crates/vr-ide/src/git_history.rs entry.

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | M7-4b scope + the M7-3 QC minor; no vr-git change; one small vr-ide addition (`Syntax::notifier`) |
| BASE-2 | pass | vr-git API used as written |
| BASE-3 | partial | tests written before the code, but first run against the finished code (no stub run); 5 mutation reds recorded |
| BASE-4 | pass | outputs above |
| BASE-5 / COST | pass | clippy, release build, full suite via run-heavy.sh; related tests otherwise |
| BASE-6 | pass | English; comments say why |
| BASE-8 | pass | no new crate (ropey, vr-syntax already deps of vr-ide) |
| BASE-10 | pass | `cargo test -p vr-ide`, full suite once |
| BASE-11 / BASE-24 | pass | nothing committed, stashed or reset; git only in temp repos (Rust tests, walk) |
| BASE-14 | pass | release build and full suite as background tasks |
| BASE-15 / COST-9 | pass | only own processes; none left |
| BASE-16 | pass | Reuse table, capabilities.md |
| BASE-17 / BASE-18 | pass | no installs, no sub-agents |
| BASE-19 | pass | progress lines |
| BASE-25 / BASE-38 | pass | clippy -D warnings, fmt --check; rustfmt on own files |
| BASE-32 | pass | D297 keys 1/2/3 popup-only, Enter opens, Esc closes; D306/D307 kept (message-box Enter rule scoped to Changes) |
| USER-5 | pass | pty walk with legacy key bytes (digits, Enter, Esc, arrows, F1) |
| TOOL | n-a | existing toolchain |
| web UI packs (SWEEP, COMPACT, REAL, EASE) | n-a | terminal UI; ratatui snapshots at 120x40 / 80x24 / 40x12 |

## Round 1

### Progress
- 2026-11-25T23:22:19+07:00 Round 1 started: read review (3 major, 5 minor)
- 2026-11-25T23:25:15+07:00 minors: syntax.rs doc moved; vr-git resolve_commit :/ test red -> fixed (21/21 read)
- 2026-11-25T23:28:13+07:00 5 new tests red (20 KB line variant for the long-line test): cells 20480 > bound, hl_made 294 vs 14, fast blame timed out behind slow, log asked 2 vs 1, failed page rerun 3 vs 2; wrap unit test red
- 2026-11-25T23:32:48+07:00 fixes in: windowed diff drawing (ui/git_history.rs window/span_styles/draw_window), cached row widths, highlighter cache keeps on-screen files, spans queried for the visible bytes only, bounded fit_head, blame widest on the blame thread, blame on its own thread+runner (start_blame, old runner shut down), log head filled from the first scan, no paging after an error (status row shows it); 5 new tests + wrap test green; 3 history snapshots changed only by the removed second log (status row shows the selected commit instead of 'loading more…')
- 2026-11-25T23:50+07:00 cargo test -p vr-ide, clippy, fmt --check, full suite, release build, walks r1/r2/r3 (+ r2b probe) done

### Findings fixed
| # | Finding | Fix | Test (red first) |
|---|---|---|---|
| M1 | long diff line styles every char x every span per frame | crates/vr-ide/src/ui/git_history.rs `window` builds only the cells in `left..left+w` (stops one cell past the edge); `span_styles` walks the sorted spans with one moving index; the highlighter is asked only for the visible bytes (`CommitView::spans` takes a byte range); row widths measured once per row (`CommitView::row_width`, cached); `fit_head` (ui/git_popup.rs) stops walking once the text is known not to fit; blame's widest line measured once on the blame thread (`BlameShown::widest`) | `a_long_diff_line_costs_only_the_visible_cells` (470 KB line, 200x70: cells per frame <= 200*70, queried bytes <= 4*200*70, a repeated frame measures 0 rows; scrolled row shows `+…` + line[8..] and ends with `…`). Red with a 20 KB line: 20480 cells in one frame |
| M2 | highlighter LRU of 8 rebuilt every frame with >8 files on screen | `CommitView::begin_frame` + `FileHl::seen`: the cache evicts only files not drawn in the current frame (LRU among them), so it grows to the files on screen and shrinks back to 8 as new files come in | `many_files_on_screen_keep_their_highlighters` (14 files, 200x70, >8 visible): made == visible after frame 1 and unchanged after 20 frames with spans landing. Red: 294 vs 14 |
| M3 | blame on the log/show thread, superseded blame not cancelled | `git_history::start_blame`: each blame on its own thread and runner (`Writer::open`, no git run); `load_blame` shuts the previous runner down first; `close_session` shuts the running one down and (exit) joins its thread; the history thread now only runs log and show | `a_newer_blame_kills_the_old_one_and_history_never_waits_for_blame` (shim sleeps 30 s on slow.rs): fast.rs answers (wait limit 20 s < 30 s), the slow blame's sh is gone (/proc), History and a commit view load while another slow blame runs (still alive), shutdown kills it. Red: timed out waiting for the fast blame |
| m4 | SEC-19 banner wraps early when an item ends at the edge | ui/git_popup.rs `wrap`: a head ending in ',' followed by a space is cut whole | `banners_wrap_between_names_and_end_with_an_ellipsis` + edge case. Red: ["abc: one,", "  two, three …"] |
| m5 | `/git show :/regex` always refused | crates/vr-git/src/repo.rs `resolve_commit`: `:/` forms resolved bare, then `<oid>^{commit}` must give the same oid; other forms unchanged (`verify_rev` helper, errors still propagate) | vr-git tests/read.rs `message_search_revisions_resolve_to_the_matching_commit`. Red: panicked at the `:/fourteen` unwrap |
| m6 | first /git log runs twice | `History::head` is `Option<Option<String>>`: `None` until a scan is known; a list asked before the first scan takes that scan's HEAD without reloading; a known HEAD that changes still reloads | `the_first_git_log_runs_once`. Red: asked 2 |
| m7 | after a log error every arrow reruns it; list hidden | `maybe_more` asks nothing while `history.error` is set (only `r` retries); the list stays drawn, the status row shows `git: <error> · r try again` in the error colour; the body message only when no commit is loaded | `a_failed_page_keeps_the_list_and_retries_only_on_r` (shim fails `git log` while a marker exists). Red: asked 3 vs 2 |
| m8 | doc comment of `take_wake` on `notifier` | moved back (crates/vr-ide/src/syntax.rs) | n-a |

Snapshots: only the 3 `render_git_popup_history_*` changed: the status row shows the selected commit instead of "3 commits · loading more…" (that was the duplicate log from m6). Commit view, blame and Changes snapshots are unchanged, so the windowed drawing gives the same output as before.

### Commands run (trimmed)
- `cargo test -p vr-ide`: `test result: ok. 633 passed; 0 failed; 7 ignored`
- `cargo test -p vr-git --test read`: `ok. 21 passed; 0 failed`
- `cargo clippy --workspace --all-targets -- -D warnings`: clean; `cargo fmt --all -- --check`: clean
- run-heavy `cargo test --workspace`: 1010 passed, 1 failed: `git_tests::each_root_shows_its_own_repo` (git_tests.rs:585 `left: None right: Some(0)`), the M7-4a flaky test already reported above (`shown_marks(&pa)` read before b's marks land under load). Not touched by this round; 5/5 passes when run alone, passed in the `-p vr-ide` run
- run-heavy `cargo build --release`: `Finished release profile in 54.66s`
- pty walks on the fresh binary (qa-artifacts/M7-4b/dev-r1/, reviewer scripts copied, outputs `r1_out.txt`, `r2_out.txt`, `r3_out.txt`, `r2b_out.txt`, `screen_*.txt`):
  - r2 (14 files, 200x70): idle cpu 0.0, 0 new threads in 4 s (review: 297 threads, 18 % CPU); 5 files: 0.002, 0
  - r2 minified: each key 0.02-0.04 s CPU (review: >15 s CPU per key); idle 0.003. r2 still prints "changed after 15 s" because it takes the "before" screen 50 ms *after* the key, when the fast redraw has already happened (and Down cannot scroll: all rows fit). r2b.py takes it before the key: Right 0.06 s, Right2 0.06 s, Left 0.05 s
  - r1: `:/fourteen` opens the commit; `--output=pwned` no file; no raw OSC 52 / RLO; quit ok
  - r3 (shim sleeps 5 s on blame): other.txt blame after 5.1 s (its own 5 s; review 8.1 s); Enter on a commit while a blame runs: commit view after 1.6 s (review >15 s); exit while blaming 0.0 s, no process left
- Nothing left running: background runs finished, every pty session exited, no vr-ide or shim `sleep` left (ps checked). A `sleep 30` left by the red run of the blame test (its app was never shut down) was killed by pid after I checked its parent was the test's shim.

### Rule compliance (round 1)
| Rule | Status | Evidence |
|---|---|---|
| BASE-3 | pass | 7 tests red before the fixes (counts above) |
| BASE-4/5/10 | pass | outputs above; full suite once via run-heavy |
| BASE-11/24 | pass | no commit/stash/reset; git only in temp repos from Rust tests and the walks |
| BASE-14/15 | pass | full suite, release build and walks as background tasks; only own processes |
| BASE-16 | pass | reused `Writer::open`, `sanitize`, `width`, reviewer scripts |
| BASE-25/38 | pass | clippy -D warnings, fmt --check |
| SEC-5 | pass | `:/` forms still pass `--end-of-options` and must end at a commit; other refusals unchanged (r1) |
| SEC-7 / AC-16e | pass | frame cost bounded by the screen (M1), highlighters bounded by the files on screen (M2), a newer blame or the end of the session kills the old one (M3) |
| SEC-15 | pass | per-char sanitize kept in `window`; hostile test and r1 unchanged |
| SEC-19 | pass | wrap edge fixed (m4) |
| SEC-20 | pass | no logging added |
