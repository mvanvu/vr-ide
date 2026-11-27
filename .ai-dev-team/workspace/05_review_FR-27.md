# Review FR-27 — file blame annotations column (D321)
Reviewer (opus). Skills: ai-dev-team:code-review-checklist, ai-dev-team:security-review-checklist, ai-dev-team:ui-ux-design (design QA).
Diff: worktree .worktrees/FR-27, `git diff adccb95` + untracked blame_column_tests.rs and 3 snapshots.
Ran: `run-heavy.sh cargo test -p vr-git` → lib 11, read 14, security 27, worker 5 ok; `cargo test -p vr-ide --lib blame_column` → 12 passed.

## Findings

[major] crates/vr-git/src/worker.rs:245-299 (used by crates/vr-ide/src/git.rs:1357) — the column's 30 s blame blocks every other read of the repo
Scenario: `Request::FileBlame` runs on the same single-threaded per-repo worker as gutter marks, inline blame, HEAD and ahead/behind, and a running request is never cancelled. In a large repo where `git blame` of a file takes 5–30 s, every save and every tab switch with the column on queues a whole-file blame (keys differ per path, so switching through 4 tabs queues 4 blames run back to back). Gutter marks (AC-B5 "keep working beside it"), inline blame and the branch indicator then stay stale for the whole time; with `git_inline_blame` on the same file is blamed twice per save (InlineBlame and FileBlame are separate cache keys). Before D310 user reads ran on their own thread/runner (D308).
Fix: run FileBlame on its own lane (a second worker thread per repo or the old history-style thread; the runner already allows 2 concurrent git), and kill a running FileBlame when a newer one for another active path supersedes it (or drop requests for non-active tabs). Optional: serve InlineBlame from a current FileBlame of the same text. Test: shim that sleeps on `blame`, assert `LineChanges` answers while a FileBlame is running.

[major] crates/vr-ide/src/git.rs:1425-1431 — annotations shift on every line below a lone CR (developer's "known limit")
Scenario: a tracked file containing one stray `\r` not followed by `\n` (old Mac line, a CR inside a string literal). ropey (`cr_lines`) counts it as a line break, git does not, so `blame.lines[i]` is drawn on rope line i: every line below the CR shows the author/date of the line above it, with no sign anything is wrong. Fix cost: small — in `build_file_blame`, walk the rope once and advance the git line index only on rope lines ending in `\n` (a lone-CR row repeats the annotation of the git line it belongs to); `remap` already works on text and needs no change. Alternative (cheaper, worse): detect a lone CR (`vr_syntax::has_lone_cr`) and show the "cannot annotate" notice instead of wrong data. User decides; a wrong author is not acceptable silently.

[minor] crates/vr-ide/src/git.rs:1534 — BASE-16: `utc_date` copies the civil-from-days algorithm of `recovery::utc_stamp` (crates/vr-ide/src/recovery.rs:143, listed in capabilities.md)
Fix: extract one `civil_from_days(days) -> (y, m, d)` and use it in both.

[minor] crates/vr-ide/src/git.rs:1406 — a reply that arrives after its tab was closed is stored again
Scenario: close a tab while its FileBlame is in flight → `forget_doc` drops the entry, the late reply re-inserts it (rope kept until shutdown). If the file is reopened at version 0 after an external change, the old annotations (version 0, other text) count as current and are drawn unremapped until the new answer arrives.
Fix: in `file_blame_arrived` ignore the reply unless a document with that path is open and `asked_file_blame[path] == (serial, stamp)` of the reply.

[minor] docs/commands.md:56 — "narrower than 76 columns … below 54 columns" are fixed numbers, but the thresholds are editor width (explorer/terminal panels subtract) and grow with the line-number gutter (≥1000 lines → 77/55).
Fix: say "when the editor leaves fewer than 72 text cells the author is shortened, below 50 the column hides", or "about 76 / 54 columns".

Checked, no finding: porcelain bounds (fin 1..=lines, entries ≤ lines, sorted+contiguous, fields cut to 1 KiB on char boundary, path fields > 16 KiB refused, missing fields default, hostile `author-time` cannot overflow `utc_date` and the label is clipped by `set_stringn`); sanitize at arrival + git error text sanitized; same `blame_inner`/runner as inline blame with `user()` limits and test for the 30 s timeout; no git per keystroke (asked once per serial+disk stamp, test asserts the counter); stale replies remapped by text, superseded ignored, serial checked before drawing; `remap` span/whole-line logic, CRLF, delete/join cases; mouse mapping uses the same width as render; no underflow at narrow widths; D310 (no slash/git command, no popup).

## Design QA (snapshots 120x40, 80x24, 40x12 + walk)
120/80: column 24 cells, `Author(≤12) date` then 3-cell gap to the numbers, `┊` continuation, "Not committed yet" on the new line, theme `git_blame` token, gutter mark `▎` and inline blame intact. 60: compact `Ann A… 2023-11-14`. 40: hidden, editor keeps code width. Matches D321/D324. No UI finding.

## Files read
| File | Status |
|---|---|
| crates/vr-git/src/blame.rs | read |
| crates/vr-git/src/cmd.rs | read |
| crates/vr-git/src/diff.rs | read |
| crates/vr-git/src/lib.rs | read |
| crates/vr-git/src/worker.rs | read (diff + run loop) |
| crates/vr-git/tests/read.rs | read |
| crates/vr-git/tests/security.rs | read |
| crates/vr-git/tests/worker.rs | read |
| crates/vr-ide/src/git.rs | read |
| crates/vr-ide/src/ui/editor_view.rs | read |
| crates/vr-ide/src/app.rs | read |
| crates/vr-ide/src/commands/edit.rs | read |
| crates/vr-ide/src/ui/explorer.rs | read |
| crates/vr-ide/src/lib.rs | read |
| crates/vr-ide/src/app_tests.rs | read |
| crates/vr-ide/src/git_tests.rs | read |
| crates/vr-ide/src/blame_column_tests.rs | read |
| crates/vr-ide/src/snapshots/vr_ide__blame_column_tests__render_blame_column_120x40.snap | read |
| crates/vr-ide/src/snapshots/vr_ide__blame_column_tests__render_blame_column_80x24.snap | read |
| crates/vr-ide/src/snapshots/vr_ide__blame_column_tests__render_blame_column_40x12.snap | read |
| crates/vr-config/defaults/keymap.toml | read |
| crates/vr-config/tests/config.rs | read |
| README.md | read |
| docs/keymap.md | read |
| docs/commands.md | read |

## SEC check
| SEC | Result | Evidence |
|---|---|---|
| SEC-1 | pass | `blame_inner` → `self.git.run` (same runner as inline blame); timeout kills group: security.rs `file_blame_runs_under_the_user_timeout` |
| SEC-2 | pass | same runner/env; `read_everything` now calls `file_blame` |
| SEC-3 | pass | same runner FIXED_CONFIG; hostile-config tests run `read_everything` incl. `file_blame` |
| SEC-4 | pass | `--porcelain` + `parse_blame_bounded` (bounds, contiguity) |
| SEC-5 | pass | path via `relative()` after `--`, literal pathspecs by runner |
| SEC-6 | pass | repo from discovery/serial only (`root_state`) |
| SEC-7 | fail (partial) | timeout 30 s, `max_file` before git and in UI, `max_output`, bounded parse: pass; but one serial worker lets a 30 s user read hold back the automatic reads (finding 1) |
| SEC-8 | pass | no writes; `reads_write_nothing_under_dot_git…` runs `file_blame` |
| SEC-15 | pass | author sanitized in `build_file_blame`, error text sanitized; test `author_names_from_git_are_sanitized` (BIDI, ESC) |
| SEC-17 | pass | `current_file_blame` checks serial and version; superseded/ShutDown dropped |

## Rule check FR-27
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | diff matches board scope |
| BASE-2 | pass | additive vr-git API; command id/title tested |
| BASE-3 | pass | qa-artifacts/FR-27/red-git.log (10× E0425) |
| BASE-4 | pass | trimmed outputs in notes; my reruns agree |
| BASE-9 | fail | SEC-7 partial (finding 1) |
| BASE-16 | fail | `utc_date` duplicates `recovery::utc_stamp` math (minor) |
| BASE-20 | pass | `parse_blame` replaced, no orphan callers |
| BASE-22 | pass | naming beside InlineBlame/timeout/edit_mode |
| BASE-23 | pass | key (kitty + legacy ESC[1;2Q), palette, read/edit mode, tabs, widths, save, untracked/no-repo/too-large tested |
| BASE-32 | pass | compared with D308 (UTC dates), D310 (no /git, no popup — command id `editor.toggle_blame`, test), D321 (layout/behaviour), D324 (shift+f2, top-row label) |
| BASE-35 | pass | USER-5: shift+f2 single key, legacy-reachable, not GNOME/VTE; USER-2/3: no mockups, real-build snapshots + pty walk |
| BASE-45 | pass | AC-B1..B5 in 01_spec_FR17-21.md |
| BASE-46 | pass | walk-result.txt 10/10 on release binary |
| COST-15 | pass | 3 new snapshots only, read above |

Round 0 verdict: CHANGES_REQUESTED

## Re-check round 1
Read: round diff of crates/vr-git/src/{cmd,blame,worker}.rs, crates/vr-git/tests/worker.rs, crates/vr-ide/src/{git,recovery,git_tests,blame_column_tests}.rs, docs/commands.md (all read). Ran via run-heavy: `cargo test -p vr-git` → lib 11, read 14, security 27, worker 8 ok; `cargo test -p vr-ide --lib -- blame_column git_tests` → 41 passed. No `sleep 30` shim process left after the runs.

| Earlier finding | Status | Evidence |
|---|---|---|
| 1 (major) blame holds back automatic reads | fixed | `run_lane` thread per worker (one queued + one running); `submit` replaces the queued one (answered Superseded) and sets the running one's cancel flag; the runner checks `cancel` after taking a slot and every 50 ms, then `kill_group` of the child's own setsid group and reaps it; the IDE cancels non-active / column-off blames each tick and asks again after Superseded (`asked_file_blame` removed). Tests: `automatic_reads_answer_while_a_file_blame_runs`, `a_newer_file_blame_cancels_the_running_one`, `a_cancelled_file_blame_stops_its_git`, `a_slow_blame_of_another_tab_never_holds_back_this_one` (30 s shim, < 5 s) |
| 2 (major) lone CR shift | fixed (D326) | `build_file_blame` advances the git line only on rope lines ending in `\n`; `fix_last` treats a final `\r` as open; test `a_lone_cr_keeps_every_annotation_on_its_line` |
| 3 (minor) BASE-16 date math | fixed | `recovery::civil_from_days` shared by `utc_stamp` and `utc_date` |
| 4 (minor) late reply for a closed tab | fixed | reply kept only when the path is open and its id equals `asked_file_blame[path].2`; test `a_reply_for_a_closed_tab_is_not_kept` |
| 5 (minor) docs thresholds | fixed | docs/commands.md now in text cells (72 / 50) with the panel/gutter caveat |

Concurrency checked:
- Lane lifetime: `Drop` calls `close` then `shutdown`. The lane thread returns when closed or when the response channel is gone. A lane blocked in `acquire` gets `ShutDown` once shutdown frees the slots.
- Cancel vs completion: a flag set after the git finished does no harm; the Ok reply is used only if its id is still the last one asked.
- The kill hits only the child's own group (`Pid::from_child` after `setsid`).
- Runner cap 2 = main worker (serial) + lane (serial), so the cap is never exceeded and neither thread starves.
- `Out.notify` sits behind a Mutex shared by the two threads.

New finding:

[minor] crates/vr-git/src/worker.rs `Lane::submit` + crates/vr-ide/src/git.rs drain id check — a slow blame can be restarted forever
Scenario: the active file's blame takes 10 s and something writes under `.git` more often than that (a prompt or tool refreshing the index, a long fetch/rebase). Each `.git` event marks the blame stale, so it is asked again; `submit` kills the running blame and older replies are dropped by the id check, so the column never appears while the events keep coming. Rare; no security or data issue.
Fix (optional): when the new request is for the same path as the running one, let the running one finish (queue the new one); cancel only for another path or column off.

### SEC check (updated)
| SEC | Result | Evidence |
|---|---|---|
| SEC-1, 2, 3, 4, 5, 6, 8, 15, 17 | pass | unchanged from round 0; the lane uses the same `Git::run` (env, FIXED_CONFIG, `--no-pager`, literal pathspecs, setsid) |
| SEC-7 | pass | user timeout 30 s, `max_file` / `max_output`, bounded parse, at most 2 git per repo (worker + lane, slot cap), the blame no longer blocks automatic reads, superseded / non-active blames killed |

### Rule check FR-27 (updated rows)
| Rule | Result | Evidence |
|---|---|---|
| BASE-9 | pass | SEC-7 now pass |
| BASE-16 | pass | `civil_from_days` shared |
| BASE-32 | pass | D308, D310, D321, D324, D326 (lone CR, own lane, cancel) — diff matches |
| BASE-1..4, 20, 22, 23, 35, 45, 46, COST-15 | pass | unchanged; the round adds tests only in the listed files, snapshots untouched |

APPROVE

Round 1 verdict: APPROVE (minor re-graded major by the leader)

## Re-check round 2
Read: round diff of crates/vr-git/src/worker.rs (`Lane::submit`, `run_lane`), crates/vr-git/tests/worker.rs, crates/vr-ide/src/git.rs (`drain_responses`, `forget_doc`, `file_blame_arrived`), crates/vr-ide/src/blame_column_tests.rs. Ran via run-heavy: `cargo test -p vr-git --test worker` → 9 passed; `cargo test -p vr-ide --lib blame_column` → 16 passed; no shim `sleep` left running.

| Item | Status | Evidence |
|---|---|---|
| Same-file blame restarted forever (r1 minor → major) | fixed | `submit` sets the cancel flag only when the running blame is for another path; a same-file request replaces only the queued one (answered Superseded). A busy `.git` re-asks, but the running blame finishes and is shown, then the newest queued one runs |
| Test catches the bug | yes | `newer_file_blames_of_the_same_file_wait_for_the_running_one` asserts the first (running) request ends `Ok(Blame)`; the round-1 code answered it `Superseded`, so the test would fail there. One response per request and at least one Superseded are asserted; the rest is left to timing, which matches the contract. IDE test `a_busy_git_folder_never_keeps_a_slow_blame_from_showing` also asserts `file_blame_requests() > 2`, so it cannot pass with the writes ignored |
| Round-1 finding 4 (closed tab never gets an old document's result) | holds | `forget_doc` (open and close) drops the path's `Pending::FileBlame` entries and cancels them in the lane, so a late reply finds no pending entry and is discarded at `e.pending.remove`; `drain_responses` still also requires the file to be open and the serial to match |
| Stale (older) result | correct | Same-path blames run one after another on one thread and answer over one channel, so an older result always arrives before the newer one and cannot overwrite it. It is stored with its own rope and version, so `sync_annotations` remaps it by text to the current buffer. It is marked `stale`, and `schedule_annotations` does not ask again while the latest request is in flight (`asked` matches). Errors of older requests are ignored; only the latest request's error is reported. A Superseded latest request clears `asked_file_blame`, so switching back asks again |

No new findings.

### SEC / Rule check (round 2)
| Item | Result | Evidence |
|---|---|---|
| SEC-7 | pass | still at most 2 git per repo (worker + lane); a superseded other-file blame is killed; a same-file one is bounded by the 30 s user timeout |
| SEC-17 | pass | reply used only with a matching serial and an open file; closed-tab replies dropped |
| SEC-1..6, 8, 15 | pass | unchanged |
| BASE-32 | pass | D326 (own lane, cancel non-active) kept; the same-file wait refines it |
| other rows | pass | unchanged from round 1 |

APPROVE
