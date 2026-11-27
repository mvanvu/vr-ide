# Review T10 — CHANGES T10

Tests: `cargo test -p vr-ide` 132 passed, 0 failed, 2 ignored; `cargo clippy -p vr-ide --all-targets -D warnings` clean.

## Findings

[major] crates/vr-ide/src/commands/search.rs:178-200, 841-888 — replace preview stays valid after the query, replacement or options change
Scenario: preview "foo"→"bar" is shown. The user edits the replace field to "baz" (or edits the query) without pressing Enter there, presses Down, then Enter → Replace. `apply` writes the old `p.replacement` "bar" to every closed file while the field shows "baz". Those disk writes cannot be undone. Only `start()` clears the preview, and typing in a field never calls it.
Fix: drop the preview (or mark it stale and refuse `ask_confirm`) whenever `input`/`replace` text changes after it was built. Option toggles already restart the search. Optionally name the query and the replacement in the confirm text. Test: build a preview, type into the replace field, press Enter on the list → no dialog (or a fresh preview), files untouched.

[major] crates/vr-ide/src/commands/search.rs:607-614, 730-734 (+ vr-search replace.rs:250) — preview and apply run on the UI thread with no bound on file count, plus O(n²) path matching
Scenario: (b) The 32 MB cap is enforced per file by `read_capped` in `plan_replace` and `apply_one`. Nothing bounds the number of files: up to 100k result groups × ≤32 MB, read once at preview and again at apply, one after another on the event-loop thread. During that time there is no redraw, no input and no cancel (for example 200 log or fixture files of 20 MB each = 4 GB read, twice). Independently, `files.iter_mut().find(|f| f.path == fp.path)` runs once per previewed file (and the same lookup happens in `apply`, and `previews.iter().find` inside T9's `apply_replace`). With 50k one-match files that is about 1.25e9 path comparisons, which freezes the UI for seconds before any I/O. D120 accepted the UI thread as "bounded by the files with results", but that bound is not small.
Fix: index `files` by path in a HashMap, or zip by position, since `plan_replace` keeps the input order. Run `plan_replace`/`apply_replace` on a worker thread and stream the per-file results back the way `drain` does, or at least cap the size of a preview and say so. Leader's call whether D120 stands, but the O(n²) part should be fixed in any case.

[major] crates/vr-ide/src/commands/find.rs:111-138, 362-379 — find bar match list is unbounded and recomputed on every edit
Scenario: open a 100 MB log file (the editor has no size cap), ctrl+f, type `e` (or turn on regex and type `.`). `find_all` copies the whole rope (`to_string`) and collects every match: tens of millions of `(usize, usize)`, i.e. hundreds of MB. This repeats on each keystroke in the field (`query_changed`). While the bar is open it also repeats on every keystroke in the editor, because `refresh` in `find_bar::render` invalidates on `version`. Typing freezes, and memory can exhaust.
Fix: cap the collected matches (VS Code stops at 19 999 and shows "19999+"), add the cap to vr-search or stop collecting in vr-ide, and avoid a full rescan per editor keystroke (debounce, or scan only the visible range plus a count limit). Test: a file with > cap matches shows "?/cap+" and `matches.len() <= cap`.

[major] crates/vr-ide/src/commands/search.rs:912-913 — clicking a result on a narrow screen leaves the keys on a hidden panel
Scenario: below 40 columns the panel takes the whole width. A left click on a result row calls `open_selected`, which hides the panel and focuses the editor, and then `handle_mouse` sets `app.focus = Focus::Search`. The editor is now shown, but every typed key goes to the invisible panel's list (`_ if in_list => {}` swallows it) until the user happens to press Esc.
Fix: set `Focus::Search` only when `app.search.visible` is still true. Test: 36x12, click a result → focus Editor, typing changes the buffer.

[minor] crates/vr-ide/src/commands/search.rs:555-557, 363 — `preview_pending` outlives the request
Scenario: Enter in the replace field while the search runs, then Esc, or Enter in the query field for another search. When that search ends, a preview appears unasked and takes the list (`field = Results`). Nothing is written without the dialog.
Fix: clear `preview_pending` in `escape` and in `start` when the start does not come from `request_preview`.

[minor] crates/vr-ide/src/commands/search.rs:657-668 — confirm text hides a truncated result set
Scenario: a project with more than 100 000 matches. "Replace 100000 matches in N files?" does not say that the search stopped, so the remaining matches stay without a word in the dialog. The panel does show the error.
Fix: append "(search stopped at the match limit; other files are not included)" when `errors` holds the limit error.

[minor] crates/vr-ide/src/ui/snapshots/vr_ide__ui__tests__render_palette_40x12.snap:15 — palette box covers the status row at 40x12 (pre-existing T5/T6 sizing, now visible)
Scenario: at 12 rows with 8 or more items, the box's bottom border is drawn on the status row (" a└───┘"), so the status bar is hidden while the palette is open. Fix options and their cost: (1) cap the palette height at `area.height - 2` in ui/palette.rs, a one-line change that shows one item fewer at 12 rows; (2) leave it as a modal overlay. User decision.

[minor] crates/vr-ide/src/app_tests.rs (+ "T10 built the last ones"), commands/registry.rs ("Empty since T10 built"), dialog_tests.rs ("became real commands in T10") — comments narrate task history (BASE-6)
Fix: state the current fact, for example "Every plan command is registered and listed".

Not findings (checked):
- (a) The apply path is single-threaded: `open_tabs` and `apply_replace` run in one event-loop call, so no tab can open or close in between. Opened, edited and closed tabs after the preview are each skipped with a reason, which is shown in a Note row and in one warning. Symlinked tabs are matched by canonical path. Hard links are refused by vr-search. The confirm dialog defaults to Cancel, and D91 settle and repeat gap apply (`open_dialog` sets `modal_since`).
- (c) `attach` cancels the old search and drops its receiver, so late messages are never read; the test covers this.
- (d) `edit.*` is swallowed (ctrl+v pastes into the field) and shift+tab is caught by the Tab arm before `edit.outdent`. alt+c/r/w as documented built-ins match the D79/D100 precedent and D116, and D111 keymap-first ordering holds. USER-1 is acceptable. A `[find]` context in vr-config is an optional follow-up; keymap.toml's header could name these built-ins (vr-config's file).
- (e) Replace-one and replace-all each make one undo step (break_undo_group on both sides). Captures are tested. Zero-length matches and Unicode use char offsets throughout and are covered by vr-search tests.
- (f) The racing test uses the strict `assert_no_lost_write_swapping` and `assert_exchange_proofs_known` checks, both writer modes, both outcomes required, and a mutation proof is recorded. It is adequate as an integration check; the commit window itself is covered in vr-search.

## Files read
| File | Status |
|---|---|
| crates/vr-ide/src/commands/search.rs | read |
| crates/vr-ide/src/commands/find.rs | read |
| crates/vr-ide/src/commands/dialog.rs | read (whole file) |
| crates/vr-ide/src/commands/{explorer,mod,registry}.rs | read (diff) |
| crates/vr-ide/src/ui/find_bar.rs, ui/search_panel.rs | read |
| crates/vr-ide/src/ui/{mod,layout,editor_view}.rs | read (diff) |
| crates/vr-ide/src/{app,event_loop,lib,theme}.rs | read (diff + handle_key/mouse/open_dialog/editable_doc) |
| crates/vr-ide/src/search_tests.rs | read |
| crates/vr-ide/src/find_tests.rs | read (assertions) |
| crates/vr-ide/src/{app_tests,dialog_tests}.rs | read (diff) |
| ui/snapshots render_find_bar_*, render_search_panel_*, render_replace_preview_*, render_palette_* | read |
| vr-search replace.rs / find.rs / project.rs (callees) | read (relevant fns) |

## Rule check T10
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | only crates/vr-ide/src/** changed (git status) |
| BASE-2 | pass | vr-search API used as contracted; layout signature + additive App items listed as deviations |
| BASE-3 | pass | red line per test in notes; the two non-red-first tests are named |
| BASE-4 | pass | test/clippy/fmt/pty output in notes; reproduced 132 passed, clippy clean |
| BASE-6 | fail (minor) | task-history comments (finding 8) |
| BASE-16 | pass | LineInput, ConfirmDialog, wrap, vr-search, testutil reused; capabilities appended |
| BASE-25 | pass | fmt/clippy clean in notes; clippy re-run clean |
| BASE-32 | pass | D33–D37/D49–D53 via vr-search; D63/D74/D96/D99 strict racing oracle; D87–D99 confirm Cancel default + D91 settle; D100–D115 explorer kept; D116–D121 implemented (D120 perf bound challenged in finding 2) |
| USER-1 | pass | keymap-first in both panels; alt+c/r/w documented built-ins (D116) |
| AC-10 | pass | find_tests (6), regex capture replace-all one undo |
| AC-11 | pass | streaming, grouping, Enter → file:line:col, cancel; < 300 ms not measured here (pty smoke) |
| AC-12 | fail (major) | per-file preview shown, but a stale preview can write a replacement other than the one shown in the field (finding 1) |

## Round 1 re-check — CHANGES T10 R1

Scope: `git -C .worktrees/T10r1 diff HEAD -- crates/vr-ide` (12 files), notes `## Fix round 1`, D122–D127. `cargo test -p vr-ide` in the worktree: 144 passed, 0 failed, 3 ignored.

### Earlier findings
| Finding | Status | Check |
|---|---|---|
| M1 stale preview | fixed | `fields_changed` drops a not-applying preview on typing, paste and toggles. `is_current` is checked again in `ask_confirm` and `apply`. Two tests. |
| M2 UI-thread I/O, O(n²) | fixed | The preview worker canonicalizes paths and runs `plan_replace`. The apply worker runs `apply_replace` per batch of 32. The UI thread only canonicalizes tab paths and diffs tab buffers. Results are zipped back by position and progress is counted per batch, so it is O(n). A buffer is changed only when `doc.buffer.text() == text` (search.rs:979), on the UI thread. Cancel is checked before each batch; cancelled and unreached files get "cancelled; not replaced", and on a worker disconnect "stopped before this file" (search.rs:827-831). New searches and previews are refused while a replace runs, and the preview is never dropped during apply. |
| M3 unbounded find | fixed | `find_capped` scans 256 KB chunks of whole lines, stops at 20 000 and shows "+". The chunk/find_all equality test covers zero-length matches, CRLF, lone \r, U+2028 and Unicode at chunk sizes 1/3/7. Documents over 1 MiB are rescanned 150 ms after the last change, via `App::tick` and the 50 ms poll. Commands use `refresh_now`. |
| M4 hidden-panel focus | fixed | The click path checks `visible`, and `route_key` moves focus from a hidden panel to the editor. |
| minors (pending, truncated confirm, palette D122/D127, BASE-6) | fixed | Tests present; snapshot shows the status row. |
| `hold` hook | ok | Only tests set it; `Default` is None; `proceed` with None never waits. |

### New findings
[major] crates/vr-ide/src/commands/app.rs:23-37 with commands/search.rs:1032 — quitting while a replace runs kills the worker mid-write, with no report
Scenario: a replace of 2 000 files shows "Replacing 300/2000". The user presses ctrl+q with no unsaved tabs. `app.quit = true`, the event loop returns, and the process exits, killing the detached worker. Some files are replaced and some are not, and nothing says which. A file that was mid-commit leaves a `.vr-ide-save-*.tmp` (not deleted; D106 names it on the next start). D126's "a file already started finishes" does not hold on this path. This behaviour is new in this round, because apply used to be synchronous.
Fix: in `quit`, when `app.search.preview` is applying, either refuse with a notice ("a replace is running; Esc cancels it") or set cancel and wait for the worker to finish the file it is on. Keep the JoinHandle in `Job` so the wait is possible. Test: start a held apply, run `app.quit`, then check that `app.quit` is false and a notice was shown (or that the worker was joined and every file has a result).

[minor] search.rs:1117-1123 — the "N files not replaced: …" warning lists every path
Scenario: cancelling a 50 000-file replace builds a warning of about 3 MB. It is wrapped again on every frame, and nobody can read it.
Fix: name the first 5 or so, then "and N more (see the panel)"; group the "cancelled" ones as a count.

[minor] search.rs:1156-1166 — an option toggle during a replace flips the flag but the search is refused
Scenario: alt+c while "Replacing" shows `Aa` on, but the results are still case-insensitive. No write risk (`is_current` blocks a confirm).
Fix: refuse the toggle itself while applying, as D126 says.

[minor] search_tests.rs:885 — the apply-cancel test cancels before the first batch only
A partial run (some batches written, the rest "cancelled") is not covered. Suggest releasing `hold` for one batch: for example, a counting hold that lets N batches pass.

### Files read (round 1)
app.rs, app_tests.rs, dialog_tests.rs, commands/registry.rs, ui/mod.rs, ui/palette.rs, ui/search_panel.rs: read (diff). commands/search.rs: read (lines 100-400 and 630-1329, the whole changed logic). commands/find.rs: read (diff). search_tests.rs: read (new worker tests). find_tests.rs: skimmed (new cap and debounce tests). render_palette_40x12.snap: read via the notes and the palette test. commands/app.rs `quit`: read (caller).

### Rule check T10 R1
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | crates/vr-ide/src/** only |
| BASE-2 | pass | vr-search API unchanged; additive vr-ide items listed |
| BASE-3 | pass | red line per fix; guard tests shown red by mutation |
| BASE-4 | pass | tests/racing/release timing/clippy in notes; 144 passed reproduced |
| BASE-6 | pass | history comments rewritten |
| BASE-16 | pass | `Running`/`drain` pattern reused for `Job` |
| BASE-25 | pass | fmt and clippy clean in notes |
| BASE-32 | pass | D122–D127 implemented; D126's "a started file finishes" fails only on the quit path (finding above) |
| USER-1 | pass | no new keys |
| AC-10 | pass | cap + debounce + equality tests |
| AC-11 | pass | narrow-click focus test |
| AC-12 | fail (major) | quit during a replace leaves a partial, unreported replace |

## Round 2 re-check — CHANGES T10 R2

Scope: round-2 files `commands/search.rs`, `commands/app.rs`, `app.rs`, `main.rs` and `search_tests.rs` in `.worktrees/T10r1` (diff against the round-1 snapshot), plus notes `## Fix round 2` / `## New states (round 2)` and D128. `cargo test -p vr-ide`: 151 passed, 0 failed, 3 ignored.

### Round-1 findings
| Finding | Status | Check |
|---|---|---|
| [major] quit during a replace | fixed | `quit` and `answer_quit` refuse while `replacing()`. `main.rs` calls `shutdown_jobs` after the loop: cancel, join, `drain_job`, and the summary goes to `exit_messages`. Tests: quit refused, shutdown joined with 32 files replaced, 48 cancelled and no `.tmp` left. |
| [minor] long warning | fixed | 5 files named, then "and N more"; test asserts under 400 chars |
| [minor] toggles during a replace | fixed | `toggle_option` refused; test |
| [minor] partial cancel untested | fixed | the permit-counter hook lets one batch pass; each of the 80 files gets one result, checked against the disk |

### New-states table audit
The cells match the code: Quit, Esc, a second request, toggles and typing, external writes and a worker panic, in each of the four states. Panic handling: a disconnect ends the job, unreported files get "stopped before this file", and the panel is not left stuck. Test: `panicking_worker_is_reported_not_stuck`, on both the UI path and the exit path.

One gap that can leave the app stuck has no handling and no test. The table assumes the worker always reaches its next cancel check.

[major] commands/search.rs:1185-1200, commands/app.rs:26-29 — a worker blocked in I/O makes quit impossible and exit hang
Scenario: a result file sits on a stalled sshfs/NFS mount, or is replaced by a FIFO between search and apply. vr-search's walk skips non-regular files, but `plan_replace`/`apply_one` call `read_capped`, which runs `File::open` with no file-type check, so the open blocks. The worker never returns to `proceed`.
- During "Replacing", or "Cancelling…" after Esc, `replacing()` stays true. ctrl+q is refused forever, and Esc only sets the flag again. The only way out is to kill the process from another terminal, which skips recovery for unsaved tabs.
- During "Preparing preview", quit is allowed, but `shutdown` also joins the preview worker. Exit then hangs before `restore_terminal`, leaving the alternate screen frozen, and before `event_loop::finish` writes recovery.
- Even without a hang, joining a preview worker adds nothing: it never writes. It can still delay exit by a whole batch (up to 32 files × 32 MB).
Fix (vr-ide):
- `shutdown` joins only Apply jobs. A preview job is cancelled and left alone.
- Bound the apply join (for example, wait at most a few seconds, then exit with "the replace was still writing <file>; check it").
- While "Cancelling…" has lasted more than N seconds, let quit through with a confirm that names this risk.
- Run the recovery write (`finish`) and `restore_terminal` before any wait.
Follow-up for vr-search (T9's code; leader to route): refuse non-regular files in `read_capped` callers before opening them, as D89 does for Save As.
Tests: a worker held without a permit, cancelled, with a bounded wait → `shutdown` returns within the bound. A Preview job never delays `shutdown`.

### Open item: main-thread panic (D128)
Judgement: **minor, accept as documented.**
- It needs a panic on the UI thread while a replace is writing.
- The panic hook restores the terminal and keeps the buffers in recovery.
- The worker dies at most mid-commit. vr-core's commit protocol (temp file, then exchange) never deletes the other version: it stays in `.vr-ide-save-*.tmp`, and the next start names that file (D106).
- What is lost is the summary of which files were replaced. The search panel can show it again after a fresh search.
- Joining from the panic hook risks a deadlock, and so does the blocked-I/O case above, so not joining is the right call. Optional: write one stderr line from the hook ("a project replace was interrupted; check <root> for partially replaced files"), using a global flag set while `replacing()`.

### Files read (round 2)
commands/search.rs (proceed, build_preview spawn, apply worker, apply_summary, replacing, shutdown, toggle_option, the panic test): read. commands/app.rs, app.rs, main.rs: read (diff). search_tests.rs (new tests from held_apply to apply_reports_a_file_failing_halfway): read. vr-search project.rs walk filter and read_capped: read (callee).

### Rule check T10 R2
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | crates/vr-ide/src/** only |
| BASE-2 | pass | additive `replacing`, `shutdown`, `App::shutdown_jobs`, `Job::handle` |
| BASE-3 | pass | 4 red lines; the panic test shown red by mutation; characterization tests named |
| BASE-4 | pass | 151 passed reproduced; clippy and fmt in notes |
| BASE-6 | pass | comments state why |
| BASE-16 | pass | `Job` pattern reused |
| BASE-25 | pass | per notes |
| BASE-32 | pass | D128 implemented as written; its stated gap is judged minor above |
| USER-1 | pass | no new keys |
| AC-10 / AC-11 | pass | unchanged since R1 |
| AC-12 | fail (major) | a blocked worker makes quit impossible and exit hang (finding above) |

## Round 3 re-check — APPROVE T10 R3

Scope: `git -C .worktrees/T10r3 diff HEAD -- crates` (10 files: vr-ide search.rs, commands/app.rs, dialog.rs, event_loop.rs, main.rs, recovery.rs, app.rs, search_tests.rs; vr-search project.rs, replace.rs), notes `## Fix round 3` / `## New states (round 3)`, D129.

Tests: `cargo test -p vr-ide -p vr-search`.
- First run: vr-ide 155 passed, 1 failed. The failure was `app_tests::external_modify_reloads_within_one_second_with_real_watcher`: "took ≥ 1 s" with the real inotify watcher, under the parallel load of both crates.
- That test alone: passed in 0.82 s.
- vr-ide run again: 156 passed, 0 failed, 3 ignored.
- vr-search: 37 + 2 passed.
- The flaky test is a wall-clock watcher test outside T10's code. The leader may want to track it (1 s budget under parallel load).

### Round-2 major (worker blocked in I/O)
| Item | Status | Check |
|---|---|---|
| Exit order | fixed | `event_loop::exit` runs restore_terminal, then `finish` (recovery), then a stderr line, then `shutdown_within(EXIT_WAIT)`, then the exit messages. The test checks that restore and recovery happen within 200 ms with a 500 ms stuck worker. |
| Preview never waited | fixed | `shutdown_within` cancels a Preview job and drops it, with no recv. The JoinHandle is gone. Test `a_preview_job_never_delays_shutdown`. |
| Bounded apply wait + "state unknown" | fixed | `drain_job_until` uses `recv_timeout` up to the deadline. Afterwards: summary, "still writing <file>", then "File state unknown: …" (5 named + "and N more"). `REPLACE_RUNNING` is cleared. Test. |
| Cancel bound + confirm | fixed | Esc stamps `cancelled_at` once. `quit_block` returns `Replacing` before CANCEL_WAIT and `Stuck(file)` after. Confirm "Quit anyway / Cancel" names the file. Quit anyway → `quit_anyway`, then the usual unsaved-files flow. `answer_quit` checks again. The test drives the clock through `handle_key_at`. |
| vr-search `open_regular` | fixed | stat (refuse non-regular), then an O_NONBLOCK open, then fstat on the handle (refuse). Used by `read_capped` (search walk, plan, apply freshness read) and `holds` (all three commit checks: target_unchanged, holds_expected, holds_ours). `apply_one` refuses a non-regular target first. Other opens are `write_temp` (`create_new`, a fresh file) and `sync_dir` (a directory), and neither can block on a FIFO. Error paths in safe_replace: a check failing before the swap gives `Err`, and the temp is removed (still ours). After the swap, `keep()` runs and nothing is deleted. FIFO tests for plan and apply, guarded by `within_2s`. |
| One file per call | fixed | `current` is set before `canonicalize` and the apply. One `apply_replace` call and one message per file. The vr-ide-side `seen` set keeps link/target dedup. Cancel is still checked per batch (D126). |

### New-states table (round 3) audit
- All cells match the code: quit, exit, Esc, a mid-run error, an input change, a second request, a concurrent writer, a hung open/read/write and a main-thread panic, in each of the five states.
- The hung-mount column (Esc → Cancelling…, then the confirm, then bounded exit) has no gap that leaves the app without a way out.
- Every data-safety cell has a test, except the panic-hook line (documented; optional item).
- Hung write (`sync_all` or rename on a hung mount): covered by the same bound. Exit names the file, and vr-core's commit never deletes the other version.

### Stated limits
1. **Leaked kernel-blocked thread: accept (n-a / informational).** A thread blocked in an uninterruptible syscall cannot be cancelled in Rust or POSIX. It holds only its batch's previews. The app stays usable: the job is dropped from the UI state after exit gives up, and during the session the user gets the confirm. On a hard NFS mount the process may linger in D-state after exit, but the terminal is restored and recovery is already written. That was the order the finding asked for.
2. **UI thread canonicalizes open-tab paths at preview start, at apply and in `drain_job`: minor, follow-up.** Only tab files are touched. The editor already stats a tab's file on the UI thread for save, reload and the disk-version checks, so a hung mount behind an open tab already freezes ctrl+s. T10 adds one more trigger (Enter in the replace field) but no new class of hang. Cheap fix if wanted: pass the raw tab paths to the preview worker and canonicalize there (at apply, compare against what the worker resolved), or cache the canonical path on the Document when it is opened.

### Minor findings (do not block)
[minor] crates/vr-ide/src/commands/search.rs (`ask_quit_stuck`) — the "Quit anyway / Cancel" confirm defaults to "Quit anyway" (`ConfirmDialog::new` gives selected 0)
Scenario: after the cancel bound, ctrl+q, then Enter more than 500 ms later quits and leaves the named file in an unknown state. The unsaved-files dialog still follows, and exit still waits 5 s. Other prompts that end something irreversible default to Cancel (D92/D104/ReplaceInFiles).
Fix: `dialog.selected = 1`, and assert it in `quit_allowed_after_the_cancel_bound_with_a_confirm`.

[minor] limit 2 above (UI-thread canonicalize of tab paths): follow-up as described.

### Files read (round 3)
All 10 files in the round diff: read. Callee vr-core `safe_replace::replace_if_unchanged`/`restore`: read (error handling after a check fails). `ui/dialog.rs` ConfirmDialog default: read.

### Rule check T10 R3
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | vr-search change was routed by the leader (round-2 follow-up); public API unchanged |
| BASE-2 | pass | `plan_replace`/`apply_replace` signatures unchanged; additive vr-ide items listed |
| BASE-3 | pass | red lines: vr-search ×2, vr-ide ×3, exit order ×1; characterization test named |
| BASE-4 | pass | evidence in notes; reproduced (see the flaky-test note above) |
| BASE-6 | pass | comments state why |
| BASE-16 | pass | mirrors vr-core's private `hash_file` (private, cannot be reused); ConfirmDialog/DialogAction reused |
| BASE-25 | pass | fmt/clippy per notes |
| BASE-32 | pass | D89 applied in vr-search; D123–D129 hold |
| USER-1 | pass | no new keys |
| AC-10 / AC-11 | pass | unchanged |
| AC-12 | pass | preview/apply safety, quit/exit bounded, FIFO refused, racing writer green (InPlace 29/11, TempRename 31/9 per notes) |

- QP1 re-check — APPROVE T10 QP1: `ask_quit_stuck` sets `confirm.selected = 1` (Cancel default, comment says why); test asserts `c.selected == 1`; `cargo test -p vr-ide quit_allowed` 1 passed.
