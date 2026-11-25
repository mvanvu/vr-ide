# Review M7-3: Git popup, Changes tab (code + security + design QA)

Verdict: **CHANGES_REQUESTED** (security: ISSUES_FOUND)

Scope: the uncommitted M7-3 diff on master a7249a8 (files listed below). I ran a live pty walk on target/release/vr-ide (built 21:50, newer than every source file) against temp repos and bare remotes: `qa-artifacts/M7-3/review/walk.txt` and `screen_*.txt`.

## Findings

1. [blocker | SEC high] git_popup.rs:1055-1068 (reached from 1776 `p` and 1724 Push button). SEC-17 fail: pressing Push inside the popup switches it to another repo.
   Scenario: workspace with roots alpha and beta, active file beta/beta.txt. Run `/git alpha`, type a draft, then press `p`. `push()` calls `ensure_session`, which compares the active file's repo, not the popup's. It closes the alpha session, which loses the draft, and opens beta. The dialog then reads "Push main of …/beta to origin/main?" with Push as the default (`screen_sec17_p_from_root_b.txt`). If alpha is busy, `open` returns an error that `let _ = push(app)` throws away, so `p` silently does nothing.
   Fix: inside the popup, push the session's own repo (no `ensure_session`). Keep `ensure_session` for the `/git push` command only. When that command would replace an idle session that holds a draft, refuse or keep the draft. Add a test for `p` and the Push button with the active file in another root.

2. [major | SEC high] git_popup.rs:1638-1676, 824-828. SEC-18 fail: commit and amend carry no record of what was shown.
   Scenario: turn Amend on (it loads HEAD's message), then run `git commit` in the terminal, then press ctrl+s. The terminal's commit is amended and its message "terminal commit" is replaced by the old one (live: reflog `3145746 commit: terminal commit` → `29e3f84 commit (amend)`). The same gap applies to a plain commit: anything `git add`ed from outside before the rescan is committed without being shown.
   Fix: capture the HEAD oid and the shown staged entries at ctrl+s, and for Amend the oid whose message was loaded. In the writer, re-read HEAD and the staged entries before `commit`; on a mismatch return Stale, rescan and show "changed, rescan". Add a test that moves HEAD between loading Amend and committing.

3. [major] app.rs:1215-1224. Typing Enter twice quickly in the message box loses the blank line.
   The popup counts as modal, so `ENTER_REPEAT_GAP` (700 ms) drops any Enter that follows another within 700 ms. Typing "Subject", Enter, Enter, "body" commits `Subject\nbody` (live: `walk.txt`). The tests set both Enter gaps to zero (app_on), so they cannot catch this.
   Fix: in the message box, drop only `KeyEventKind::Repeat` and the `modal_since` settle, not the gap between two presses. Add a test that uses the real gaps.

4. [major] git_popup.rs:640-648. Writes waiting behind a running push or pull are not shown, which breaks D303(2) ("shows the queue").
   Scenario: a push that a slow pre-push hook keeps busy, then `space`. The status line shows only "push → origin/main: running", and the stage lands silently later (`screen_80x24_push_queue.txt`). A second `space` meanwhile queues a write that then fails with Stale.
   Fix: add " · N waiting" (and the first write's label) to the job line.

5. [major | design] ui/git_popup.rs:544-559. Diff lines are cut at the pane width with no marker and no horizontal scroll.
   At 80x24 the diff pane is 41 columns wide, so code past that column is staged or discarded unseen. This is the T7 hiding case, using spaces instead of escapes.
   Fix options: (a) a `…` cell at the cut (small change); (b) `left`/`right` scroll the diff pane horizontally (moderate); (c) wrap lines (larger: breaks the one-row-per-line mapping used by selection and mouse).

6. [minor] git_popup.rs:1642-1646. A second ctrl+s while a commit (with hooks) runs cancels it at once, with no question. Push asks "Keep running / Cancel it".
   Fix: ignore ctrl+s while a commit runs (the "Cancel commit" button remains), or ask the same question as for push.

7. [minor] git_popup.rs:1663-1666. Amend gives no hint when HEAD is already on the upstream (↑0). The later push is rejected (never forced), and the user has to sort it out in the terminal.
   Fix: when Amend is turned on, add the note "the last commit is already on <upstream>".

8. [minor | SEC low, CWE-59] git_popup.rs:836-876. "Restore as…" checks the name only by its text. It accepts a `.git/…` target (e.g. `.git/hooks/post-commit`, which keeps the leftover's mode) and a path through a symlinked folder (the leftover is moved outside the repo; NOREPLACE prevents overwriting but still creates a new file).
   Fix: refuse any `.git` component, and resolve the parent beneath the workdir (openat2 `RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS`, or canonicalize and check the prefix).

9. [minor | SEC low] vr-git/src/programs.rs:70-93. Three program-running keys are missing from the SEC-19 list: `gpg.ssh.defaultkeycommand` (runs on signed commits), `core.gitproxy` and `remote.<r>.vcs` (a remote helper).
   Fix: add them to `runs_program` and to the unit test.

What I checked and found correct:
- Writes run on the writer thread through `Writer::open`, never through the reader's clone.
- The discard dialog defaults to Cancel, names the repo, path and hunk count, and the notice names the recovery copy. A buffer with unsaved edits refuses the discard. `save_doc`, `save_as` and Keep mine are blocked while a discard runs.
- The `.new` and `.old` leftover dialogs work, and NOREPLACE holds.
- Hostile file names (OSC 52, RLO) never reach the terminal raw.
- The push dialog shows the target; the conflict banner shows.
- Layout at 120x40, 80x24 and 40x12 and the mouse hit areas match the snapshots.
- No global key binding changed: the popup is modal, and `esc` closes it first.

Accepted residuals: an edit made after a discard was queued, project replace, and exit cancelling commit/push/pull (D306).

## Files read
- crates/vr-ide/src/git_popup.rs — read
- crates/vr-ide/src/ui/git_popup.rs — read
- crates/vr-git/src/programs.rs, lib.rs — read
- crates/vr-ide/src/{app.rs, app_workspace.rs, event_loop.rs, git.rs, lib.rs, ui/mod.rs, commands/dialog.rs, commands/file.rs, commands/slash.rs} — read (diff hunks)
- crates/vr-ide/src/git_popup_tests.rs — read (test list; the discard and commit tests in full)
- crates/vr-ide/src/{git_tests.rs, slash_tests.rs} — read (diff)
- snapshots render_git_popup_{changes_120x40,80x24,40x12, diff_40x12} — read
- README.md, docs/commands.md, docs/keymap.md, .ai-dev-team/capabilities.md — read (diff)
- 02_tasks.md, decisions.md — skipped (leader bookkeeping)

## Rule check M7-3
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | M7-3 scope; one additive read-only API in vr-git (D306) |
| BASE-2 | pass | Writer, DiscardPlan and Job contracts used as written (04_dev_M7-2) |
| BASE-3 | fail (minor, self-reported) | tests written after the code; mutations recorded; the Enter gaps are zero in tests (finding 3) |
| BASE-4 / BASE-10 | pass | the notes record the test, clippy, fmt and release build outputs |
| BASE-16 | pass | sanitize, GitState, dialog and slash registry reused; capabilities.md updated |
| BASE-32 / keys | pass | popup keys = proposal + D306 (shift selection, tab ring); no global key changed; left/right on buttons is the dialog convention |
| USER-5 | pass | legacy key bytes drive the popup in the pty walk |
| SEC-15 | pass | every drawn git string is sanitized (lists, diff, header, banners, dialogs, errors, progress, output); raw pty output has no OSC 52 or RLO |
| SEC-16 | pass | discard defaults to Cancel and names the repo, path and recovery copy; push dialog and progress show `origin/main` |
| SEC-17 | **fail** | finding 1 (Push inside the popup switches repo and loses the draft) |
| SEC-18 | **fail** | file, hunk, line and discard: pass (check_seen, vr-git shown diff, plan hash); commit and amend: finding 2 |
| SEC-19 | pass (minor gap) | banner from the first scan; programs.rs only runs `git config --list` and reads the hooks folder; finding 9 |
| SEC-20 | pass | no tracing in git_popup.rs; vr-ide installs no log subscriber; push/pull output redacted by vr-git |
| AC-16f | pass | file, hunk, line, commit and amend+signoff twins against git; push remote == HEAD; discard compared by content (no status twin; minor gap) |
| D303 (2) | **fail** | queue hidden while a job runs (finding 4) |
| D304 | pass | `Writer::open` per session; root removal leaves the session running until idle; exit waits 3 s, then shuts down |
| D305 (4) | pass | saves blocked by queued path; dirty buffer refused before queueing |
| Design QA | fail (major) | diff clipping (finding 5); layout and mouse otherwise as in the snapshots |

## Round 1 re-check

Verdict: **APPROVED** (security: PASS). Findings 1-9 are fixed as D307 describes. Three new minors below are not blocking; they can go to the leader's follow-up list.

Evidence: `cargo test -p vr-ide git_popup` 35 passed; `cargo test -p vr-git` all suites ok (lib 28, read 20, security 27, write 26, write_security 19, worker 4, patch_props 4). `r1walk.py target/release/vr-ide` (22:15 build, newer than every source file) 27/27, output in `qa-artifacts/M7-3/review-r1/`, no process left behind. No debris from the perl incident: the heads of app.rs and ui/git_popup.rs and the app.rs diff are clean.

Earlier findings:
1. Fixed. Every in-popup action (space, enter, d, ctrl+s/Commit, p/Push, Pull, r/Rescan, Amend, leftovers) works on `app.git_popup`. Every dialog answer checks `session(app, id)`. Only `/git push` and `/git pull` call `ensure_session` (git_popup.rs:1196,1217), as D307 allows. When an idle session is replaced, its draft goes to `git_drafts` by work tree and comes back only for that repo. The earlier `let _ = push(app)` is gone.
2. Fixed. `Scan.base` (HEAD plus `staged_fingerprint`) is read before and after the listing. ctrl+s refuses when base is None or `amend_oid` differs from the shown HEAD. The writer reads both again right before `commit` (git_popup.rs:900-906). A mode-only change is covered, because `--raw` prints both modes and the status is M. An intent-to-add entry is hidden from `diff --cached` and also skipped by commit/write-tree (git 2.43), so the commit and the fingerprint agree: no gap. I could not reproduce this live, because the team guard blocks git index commands even in scratch; I relied on git's documented behaviour plus the vr-git test. The amend-of-pushed dialog has Cancel at index 0, selected.
3. Fixed. The 700 ms gap is off only in the message box. A labelled repeat and the settle time still apply (app.rs:1219-1227). The test uses the real gaps.
4. Fixed. The status line reads `push → …: <progress> · N waiting: <first>`.
5. Fixed. `…` marks a cut line; left/right scroll 8 columns, clamped so the line's end is reachable. Line selection is by row, so staging selected lines while scrolled stages the right lines (see minor A).
6. Fixed. A second ctrl+s asks "Keep running / Cancel it" with Keep as the default. The button still cancels at once.
7. Fixed (see 2).
8. Fixed. `.git` is refused in any component, case-insensitively. The parent is opened through `openat2(BENEATH|NO_SYMLINKS)`. ENOSYS falls back to a canonical-path check; any other error refuses. `renameat2(NOREPLACE)` targets that fd. On ext4, `.GIT`, `.git `, `.git.` and Unicode-ignorable variants are distinct names that git never uses as its own folder, so they don't matter. They would matter only on NTFS or HFS+, and this Linux-only openat2 path does not target those. The case-insensitive check also covers casefold or vfat mounts.
9. Fixed. The three keys were added. programs.rs only runs `git config --local --list` and reads the hooks folder, and reads run with hooks set to /dev/null and GIT_OPTIONAL_LOCKS=0.

New (minor):
A. [minor | design] ui/git_popup.rs:556. When the diff is scrolled right (`diff_left > 0`), nothing marks that text is hidden on the left. A changed line shorter than `diff_left` draws as a bare `+` or `-`, and it can be selected, staged or discarded with its text off-screen. Fix: draw `…` in the first text cell when `diff_left > 0` and the line has cells to the left, or show `col N` in the diff title.
B. [minor] git_popup.rs:466, 482, 1097-1109. `let _ = tx.send(..)` and `if let Ok(h) = spawn(..)` hide a dead session thread. `write()` queues the label but drops the op, so the queue never drains. The session then stays busy for good: `/git` on another repo is refused, and a queued Discard keeps that path save-blocked. Fix: on a send error or a failed spawn, pop the queue entry, report "git: the session's writer stopped", and mark the session orphan.
C. [minor] git_popup.rs:1270-1276. An orphaned session (its root removed) is closed without saving its draft to `git_drafts`, unlike `open()`. Fix: the same draft hand-off as open():1076-1081.

## Rule check M7-3 (round 1)
| Rule | Status | Evidence |
|---|---|---|
| SEC-15 | pass | new strings (queue line, amend/cancel dialogs, restore refusals, `…` rows) go through sanitize or are fixed text; walk: no raw OSC 52 or RLO |
| SEC-16 | pass | discard and amend-of-pushed dialogs default to Cancel; push dialog names the repo and target |
| SEC-17 | pass | popup actions bound to the session (finding 1); `/git push`/`pull` follow the active file per D307; drafts kept per repo; test + walk B |
| SEC-18 | pass | HEAD + staged fingerprint checked at ctrl+s and in the writer (finding 2); mode-only covered, i-t-a not committed; one residual window between the writer's check and git's own commit (accepted) |
| SEC-19 | pass | 3 keys added; banner from the first scan; programs.rs read-only |
| SEC-20 | pass | no logging added in round 1 |
| AC-16f | pass | commit/amend/signoff twins still green; `commit_amend_and_signoff_match_git_commit` rescans before Amend (a commit takes only what is shown); push remote == HEAD in the test and walk |
| D303 (2) | pass | queue visible behind push/pull |
| Design QA | pass (minor A) | `…` and horizontal scroll; snapshots wide_diff_{80x24,40x12,scrolled_80x24} |

## Files read (round 1)
- crates/vr-ide/src/git_popup.rs — read (round-1 areas: Scan/CommitBase, run_write, leftover/target_dir, open/ensure_session, handlers, commit/amend, keys, tail)
- crates/vr-ide/src/ui/git_popup.rs — read (head, diff pane, fit_head/skip_cells)
- crates/vr-ide/src/app.rs — read (diff)
- crates/vr-ide/src/commands/{dialog.rs, slash.rs, file.rs} — read (diff)
- crates/vr-git/src/{repo.rs, lib.rs, programs.rs}, tests/read.rs — read
- crates/vr-ide/src/git_popup_tests.rs — read (round-1 test list; SEC-17 and SEC-18 tests)
- snapshot wide_diff_scrolled_80x24 — read
