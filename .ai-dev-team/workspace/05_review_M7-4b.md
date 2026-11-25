# Review M7-4b — Git popup History / Blame tabs, commit view (code + security + design QA)

Verdict: **CHANGES_REQUESTED** (3 major, 5 minor; no security hole found)

Release binary rebuilt first (git_history.rs was newer than the dev's 23:02 build). Live pty walks:
`qa-artifacts/M7-4b/review/{r1.py,r2.py,r3.py}` + `screen_*.txt` (temp repos only; every session
quit, no git/sleep process left after exit).

## Findings

[major] crates/vr-ide/src/ui/git_history.rs:449-472 (called at :410-413; `widest` at :391-395) — a long diff line freezes the UI (AC-16e, SEC-7)
Scenario: `/git show` on a commit that adds a 470 KB minified `min.js` (one line, under HL_MAX_BYTES so it is highlighted). On every frame `styled()` turns the whole line into one `String` per char, and for each char it searches the span list linearly (`spans.iter().find`), so the cost is chars × spans. Measured (r2.py, 200x70): each of Down / Right / Right took >15 s of UI-thread CPU before the screen changed, and the view uses 0.7 CPU at idle (highlighter notifies → redraw → another multi-second frame). The editor is frozen while that commit is open. Minified bundles, lockfiles and generated files are common in commits.
Fix: build cells only for the visible columns `left..left+w` (stop once `col >= left + w`). Walk the sorted spans with a moving index instead of `find`. Compute `widest` without building the whole row (e.g. cache the row width per row once). Optionally show lines over a few KiB without syntax colour.

[major] crates/vr-ide/src/git_history.rs:225-237 (`HL_LIVE = 8`) — highlighters are rebuilt on every frame when more than 8 highlighted files are on screen
Scenario: `/git show` on a commit that changes 14 `.rs` files by one line each, with the diff area visible on a 200x70 terminal (about 4 rows per file, so more than 8 files fit; a full-screen terminal at 55+ rows reaches this). Each frame visits files 0..13 in order. The LRU (sized 8) evicts every file before it is visited again, so a new `Highlighter` and worker thread is created for each file. Each parse calls `notify`, which redraws and starts the cycle again. Measured idle with no input (r2.py): **297 new threads in 4 s, 18 % CPU**, and it never settles. The 5-file control showed 0 new threads and 0.2 % CPU.
Fix: never evict a file whose rows are on screen in the current frame (let the cache grow to the number of files visible, or keep a map and evict only files off screen). Or cap how many highlighters are created per frame and draw the rest plain.

[major] crates/vr-ide/src/git_history.rs:339-352 (worker), :499-532 — a superseded blame is never cancelled, and log/show wait behind it
Scenario (r3.py, git shim that sleeps 5 s on blame): `/git blame` app.rs, then `1`, then `3` on other.txt. The other.txt result arrived only after **8.1 s** (the rest of app.rs's blame plus its own). Then `r`, `2`, Enter: History stayed on "loading commits…" and Enter did nothing for **>15 s**, because log and show run on the same thread after the running blame. With vr-git's 30 s user timeout, a pathological blame holds History and the commit view for up to 30 s. Stale answers are dropped correctly (newest wins), but the user waits for the old work and Enter is silently ignored.
Fix: give blame its own thread and runner, separate from log/show (per-repo concurrency stays ≤ 2 with the reader). When a newer blame is asked for (or the Blame tab is left for another file), call `shutdown()` on the old blame runner and make a fresh one (`Writer::open(&repo)` costs no git run). Exit and session close already kill correctly.

[minor] crates/vr-ide/src/ui/git_popup.rs:320 — the SEC-19 banner wraps too early when an item ends exactly at the row edge
Scenario: 40x12 with `filter.x.clean, filter.x.smudge, hook pre-push`. `head` ends with "clean," but has no following space, so `rfind(", ")` misses and the row breaks after "programs:". Row 2 then ends with "ho…", so `hook pre-push` is hidden although both rows could hold every name (screen_changes_40x12.txt).
Fix: also accept a ',' at the end of `head` (check `rest[head.len()..].starts_with(' ')`), or use `rfind(',')`.

[minor] crates/vr-ide/src/git_history.rs:590-607 → vr-git repo.rs:330 — `/git show :/<regex>` is always refused
`:/fourteen` becomes `:/fourteen^{commit}`, so the suffix is searched as part of the regex. Result: "no such commit" although a commit matches (r1.py). Every such input also walks the whole history, bounded only by the 30 s timeout, on the history thread.
Fix: refuse `:/` up front with a clear notice. Or resolve `:/…` without the suffix and then check `cat-file -t` == commit.

[minor] crates/vr-ide/src/git_history.rs:475, :664-674 — the first `/git log` runs `git log` twice
`ask_log` stores `head = None` because the session's first scan has not arrived yet. When it arrives, `tick` sees a "moved" HEAD and loads the list again.
Fix: reload only when a known head changes (`history.head.is_some()`), and fill `head` from the first scan.

[minor] crates/vr-ide/src/git_history.rs:489-495, :711 — after a log error, every arrow key runs the failing log again
Example: a paging request fails with TooLarge or a timeout. `complete` stays false and `maybe_more` asks again on each Down. history() also hides the commits that were already loaded behind the error text.
Fix: stop paging after an error until `r` is pressed. Keep the list and show the error on the status row.

[minor] crates/vr-ide/src/syntax.rs:152-155 — the doc comment of `take_wake` ("True once after…") now sits on `notifier`
Fix: move it back above `take_wake`.

Not findings (checked):
- SEC-5 `/git show` input: `--output=pwned`, `-n`, `HEAD..main`, `main:renamed.rs` and a 3000-char input are all refused with a sanitized notice, and no file is created. `HEAD@{0}` opens.
- Merge commits: a `Merge:` row and the diff against the first parent (D308, a documented deviation from git's combined diff).
- Submodule entries show `Subproject commit …`, binary files show "binary file", renames show `R100 old new`.
- A hostile file name in name-status shows as `^[]52…<U+202E>`, with no raw OSC 52 / RLO in the pty output.
- Exit while a blame runs: the app exits at once, and git and its child `sleep` are gone.

## Design QA (screens: qa-artifacts/M7-4b/review/screen_{history,commit,blame,blame_right,changes}_{120x40,80x24,40x12}.txt)
- Tab bar, columns by width, `│` block rows, age colours, status rows and hints ending in `…`: as in D308 at all three sizes. At 40x12 only the active tab name shows, and keys still work.
- 40x12: the History title is cut from the left and loses its "History ·" label. The active tab name in the border carries it, so this is acceptable.
- Banner: see minor finding 4.

## Files read
- crates/vr-ide/src/git_history.rs — read
- crates/vr-ide/src/ui/git_history.rs — read
- crates/vr-ide/src/git_popup.rs (diff) — read
- crates/vr-ide/src/ui/git_popup.rs (diff) — read
- crates/vr-ide/src/app.rs, commands/slash.rs, syntax.rs, lib.rs, ui/mod.rs (diffs) — read
- crates/vr-ide/src/git_history_tests.rs — read in part (SEC-5, blame Enter, test list)
- crates/vr-ide/src/git_popup_tests.rs, slash_tests.rs (diffs) — skipped — helper visibility and registry list only
- 13 snapshot files — skipped — compared through the live screens instead
- README.md, docs/commands.md, docs/keymap.md, capabilities.md, decisions.md (D308), 02_tasks.md (diffs) — read
- Followed callees: vr-git repo.rs `resolve_commit`/`shutdown`, log.rs `log`/`commit`, blame.rs, diff.rs `diff_commit`, write.rs `Writer::open`, vr-syntax `Highlighter::new`/`query`

## Rule check M7-4b
| Rule | Status | Evidence |
|---|---|---|
| SEC-5 | pass | git_history.rs:357 → repo.rs:325 `rev-parse --verify -q --end-of-options <rev>^{commit}`; the full oid goes to `commit`/`diff_commit`; live: `--output=pwned` (no file), `-n`, `HEAD..main`, `main:path` refused; test `git_show_refuses_…` |
| SEC-7 | fail | caps hold (8 MiB blame read, 64 MiB diff, 200-commit pages, timeouts, runner killed on close/exit, no git left), but frame cost has no bound on line width (major 1) and highlighter creation has no bound (major 2) |
| SEC-15 | pass | every git string goes through `sanitize` (ids, names, emails, subjects, message, paths in FileStat/FileHead, hunk headers, per-char diff, blame lines, notices with the typed rev); dates are numeric; live: hostile file name and author show placeholders, no raw OSC 52 / RLO bytes |
| SEC-17 | pass | log/blame/show go through `git_popup::open(app, "")`, the session's repo; the history runner comes from that session's repo |
| SEC-19 | pass (minor 4) | banner wraps to 2 rows or ends with `…`; names first at 40x12; `hook pre-push` cut there by the wrap bug |
| SEC-20 | pass | no tracing/log/print in git_history.rs or ui/git_history.rs |
| AC-16c | pass | blame == porcelain, `--follow` history, commit view == `git show` (tests); merge = first-parent diff per D308; live checks of rename, binary, submodule and merge |
| AC-16e | fail | keys stay live during a slow blame (walk), but a long line freezes the UI for >15 s per key (major 1); a superseded blame holds History and show (major 3) |
| BASE-3 | pass (noted) | tests not run red before the code; 5 mutation reds recorded instead |
| BASE-4/5/10 | pass | 628/0, clippy, fmt, release build in notes; the M7-4a flaky test is reported to the leader |
| BASE-16 | pass | reuses vr-git, `sanitize`, `fit_*`, `utc_stamp`, `relative_age`, `Highlighter`; capabilities.md updated |
| BASE-32 | pass | 1/2/3 popup-only and text in the message box; Esc closes the commit view, then the popup |

## Round 1 re-check

Verdict: **APPROVED** (3 major, 5 minor all fixed; nothing new found)

Spot-runs: `cargo test -p vr-ide git_history` 21/21, `cargo test -p vr-git --test read` 21/21. Release binary (23:34) is newer than every changed source. Reviewer copies of the walks: `qa-artifacts/M7-4b/review-r1/` (r1 on a fresh temp repo, r2b on that repo, r3), every session quit, no vr-ide, git shim or sleep left.

Checked:
- M1 (visible-column styling): `window()` stops one cell past `left+w`; `span_styles` walks sorted spans with one moving index; spans are queried for the visible byte range only; row widths cached per row. The cache depends only on row text (not theme or width), so theme change and resize need no invalidation; it is rebuilt when the row count changes. r2b on the 470 KB line: Right / Right2 / Left 0.06 s CPU each, idle 0.0. Minor residual (not a finding): a wide char straddling the left edge makes `skipped` overshoot `left` by one cell, so that row is drawn one column early.
- M2 (highlighter cache): eviction only for files with `seen < frame`, so the cache grows to the files on screen and shrinks back to 8 later; memory is bounded by screen rows. r2b 14 files: 0.0 CPU, 0 new threads in 4 s (was 297).
- M3 (blame runner): own thread and runner per blame; `load_blame` shuts the old runner down before starting; `close_session` shuts it down (and joins on exit). r3: other.txt blame after 5.1 s (was 8.1), Enter on a commit while a blame runs: commit view after 1.5 s (was >15), exit while blaming 0.0 s, no git or sleep left.
- m5: `:/` is resolved bare, then `<oid>^{commit}` must return the same oid; others unchanged, all via `--end-of-options` (SEC-5). Live: `:/fourteen` opens; `--output=pwned`, `-n`, `HEAD..main`, `main:renamed.rs`, `:/nomatchzzz` refused; no file created; no raw OSC 52 / RLO.
- m6 (first log once): `head` is `None` until a scan is known; covered by test `the_first_git_log_runs_once`.
- m7: `maybe_more` asks nothing while `history.error` is set; only `ask_log` from `r` clears it.
- m4, m8: tests green; `take_wake` comment moved.

## Rule check M7-4b (round 1)
| Rule | Status | Evidence |
|---|---|---|
| SEC-5 | pass | repo.rs:325-355 `:/` bare then `^{commit}` oid equality, `--end-of-options`; live refusals in r1 |
| SEC-7 | pass | frame cost bounded by screen (r2b 0.06 s/key on 470 KB line), highlighters bounded by on-screen files, blame killed on supersede/close/exit (r3) |
| SEC-15 | pass | per-char `sanitize` kept in `window`; r1 no raw OSC 52 / RLO |
| SEC-17 | pass | unchanged: session repo only |
| SEC-19 | pass | wrap edge fixed (m4 test) |
| SEC-20 | pass | no logging added |
| AC-16c | pass | unchanged; snapshots for commit/blame/Changes unchanged |
| AC-16e | pass | long line no longer freezes; newer blame and show/log do not wait behind a slow blame |
| BASE-3/4/5/10/16/32 | pass | per developer round 1 notes; 21/21 and 21/21 re-run here |
