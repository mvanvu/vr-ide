# FR-5 Phase 5 final-review fixes (crates/vr-ide)
task-base FR-5 0e5754fd5d57ebd3038233f71bb02799f54a2688

## Progress
- read brief-common, 06_final_review.json, task row.
- MAJOR red: explorer_tests::save_as_new_name_appears_in_the_explorer — panicked: rows [.., "    main.rs", ..] lack "    new.rs".
- MAJOR red: explorer_tests::save_recreating_a_deleted_file_brings_its_row_back (real watcher) — panicked: rows lack "    main.rs" after save.
- MAJOR green: explorer::note_written (re-reads the shown parent when its listing lacks the saved file), called from file::save_doc and file::save_as_doc on success. Both tests ok.
- minor stale-search red: search_tests::search_results_follow_an_explorer_rename (panicked at paths assert: old paths kept) and replace_preview_dropped_after_an_explorer_rename (panicked: stale preview kept).
- minor stale-search green: search::paths_moved called from explorer::retarget (rewrites group/match paths + index; drops a non-applying, not-applied preview touching the moved paths with an info notice). Both ok.
- minor reveal red: explorer_tests::reveal_of_a_hidden_or_ignored_file_says_how_to_show_it — panicked ".github/workflows/ci.yml: hidden folder expanded".
- minor reveal green: reveal_path walks root-first and expands a folder only when it is listed in its parent (one re-read for a stale listing); otherwise info notice naming the toggle command (not_shown_reason). explorer_tests:: 32 passed.
- minor two-tabs red: explorer_tests::rename_or_move_onto_a_path_open_in_a_tab_is_refused — panicked "refused in the prompt" (rename went through); closing_a_tab_keeps_the_watch_of_another_tab_on_the_path — panicked "the remaining tab lost its watch".
- minor two-tabs green: explorer::check_tab_free in answer_rename and move_paths (cut/paste too); App::close_doc unwatches only when no other tab holds the path. Both ok.
- minor narrow red: search_tests::narrow_esc_and_focus_cycle_never_type_into_a_hidden_editor — panicked "esc: typed into an editor of width 0".
- minor narrow green: search::focus_editor (hides the full-width panel before giving the editor the keys) used by escape(), open_selected() and focus.cycle. search_tests:: 28 passed.
- minor replace-vs-tab red: search_tests::tab_opened_during_a_replace_is_never_written (replaces the old tab_edited_during_a_replace_loses_nothing, which asserted the old behaviour) — panicked "written behind the tab".
- minor replace-vs-tab green: search::ReplaceGate (Claims {opened, writing} under one Mutex, no I/O under it) on the apply Job; App::open_path calls search::before_tab_opens; worker calls gate.begin/end around each write; skipped file result "opened in a tab during the replace; not replaced". Racing test gate_tests::a_tab_never_opens_on_a_file_the_worker_writes + opening_the_file_being_written_is_refused_then_allowed (written right after the code; mutation check: with begin() ignoring `opened` both fail — "file 947: opened at 3419, written until 3426", "the worker writes a file open in a tab").
- guard: fmt ok, clippy --workspace clean; full `cargo test -p vr-ide` once (165 passed, 1 failed not mine, see below).

## Summary
- Files changed: crates/vr-ide/src/app.rs, crates/vr-ide/src/commands/explorer.rs, crates/vr-ide/src/commands/file.rs, crates/vr-ide/src/commands/search.rs, crates/vr-ide/src/explorer_tests.rs, crates/vr-ide/src/search_tests.rs. No vr-core change. Team files: decisions.md (6 `D? (FR-5)` lines), capabilities.md (4 lines).
- Tests added: explorer_tests::{save_as_new_name_appears_in_the_explorer, save_recreating_a_deleted_file_brings_its_row_back, reveal_of_a_hidden_or_ignored_file_says_how_to_show_it, rename_or_move_onto_a_path_open_in_a_tab_is_refused, closing_a_tab_keeps_the_watch_of_another_tab_on_the_path}; search_tests::{search_results_follow_an_explorer_rename, replace_preview_dropped_after_an_explorer_rename, narrow_esc_and_focus_cycle_never_type_into_a_hidden_editor, tab_opened_during_a_replace_is_never_written (replaces tab_edited_during_a_replace_loses_nothing, whose assertion was the behaviour the finding removes)}; commands::search::gate_tests::{a_tab_never_opens_on_a_file_the_worker_writes (racing), opening_the_file_being_written_is_refused_then_allowed}.
- Per finding: MAJOR fixed on the vr-ide side (explorer::note_written after save / Save As); stale search paths fixed; reveal of hidden/ignored fixed; replace worker vs new tab fixed (ReplaceGate); two tabs on one path fixed (check_tab_free + close_doc shared-watch guard); Esc/F6 below 40 cols fixed (search::focus_editor).
- Deviations: no planned contract changed; additive pub helpers: explorer::note_written, search::{paths_moved, focus_editor, before_tab_opens, ReplaceGate, OPENED_DURING_REPLACE}.
- Snapshots: no render change; none regenerated (snapshot tests pass in the full run).
- Out of scope: a search still running during a rename can deliver the moved file under its old path after paths_moved ran; Enter then reports the file missing; searching again fixes it. Not changed (rare).

## Not mine
- find_tests::find_debounces_rescans_in_a_large_file failed in the full run and once more alone: `left: (0, 2) right: (2, 4)` at find_tests.rs:312. find.rs / find_tests.rs are untouched by FR-5. Likely cause: `draw()` outside `handle_key_at` makes `find::refresh` use the real clock (`app.now()` falls back to Instant::now()), so the pending rescan is due at real_now+150 ms while the test ticks at the synthetic t0+1300 ms; the test takes ~1.25 s in debug on this loaded machine, so the due time lands after the tick. Timing-dependent T10 test.

## New states (round 0)
1. ReplaceGate (Claims{opened, writing}) on the apply Job, shared with the worker.
   - quit: shutdown cancels the worker as before; the gate dies with the Job; nothing new waits on it.
   - cancel/Esc: the worker stops at the next batch; files not reached are "cancelled"; a file in `opened` is never written either way.
   - error halfway: an apply_replace error is a value, gate.end() still runs; a panic leaves `writing` set, but the job ends on disconnect (drain sets p.job = None), so open_path no longer consults the gate and no file stays unopenable (panicking_worker_is_reported_not_stuck passes).
   - input changing: Save As of a tab onto a pending file writes a new version, so apply_replace's freshness check refuses it (existing guard); an explorer rename of a pending file makes the worker report not found.
   - second request: a new search / preview / toggles stay refused while replacing (requests_are_refused_while_replacing).
   - concurrent writer: worker and UI decide under one mutex; gate_tests::a_tab_never_opens_on_a_file_the_worker_writes (4000 files x 5 runs, opens crossing the worker) asserts no file is opened before or while it is written; a mutation (begin ignoring `opened`) turns it red.
   - call that never returns: no I/O under the lock; a worker hung in a write holds only `writing` = that file; the UI refuses only that file ("open it again in a moment") and never blocks. The D129 stuck-worker quit path is unchanged.
   - failed open: open_failed() withdraws the claim (last part of opening_the_file_being_written_is_refused_then_allowed).
2. paths_moved: rewrites results in place; a stale preview is dropped with drop_preview (never one that is applying). No thread, nothing kept.
3. reveal: no new state; at most one extra re-read of a folder already shown.
4. note_written: no new state; one re-read of the shown parent only when its listing lacks the file.
5. check_tab_free / close_doc guard / focus_editor: branches only, no kept values.

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| reload a shown folder | explorer.rs `reload`, `refresh_after` | reused `Explorer::reload` + `rebuild` |
| path prefix rewrite | explorer `retarget` | same strip_prefix rule in search::moved_path |
| drop a preview | search `drop_preview` | reused |
| refuse a target held by a tab (D95) | file::check_other_tabs | not reusable as is (index-based, equal location only, not "under a folder"); small check_tab_free in explorer.rs |
| narrow-screen panel hide | search::open_selected (D118) | factored into search::focus_editor, used by open_selected, escape, focus.cycle |
| canonical path | search::canon | reused |

## Commands run (trimmed)
```
cargo test -p vr-ide --lib -- explorer_tests:: search_tests:: gate_tests app_tests:: dialog_tests::
test result: ok. 111 passed; 0 failed; 0 ignored; 0 measured; 58 filtered out; finished in 2.35s
run-heavy.sh cargo test -p vr-ide
test find_tests::find_debounces_rescans_in_a_large_file ... FAILED   (not mine, see above)
test result: FAILED. 165 passed; 1 failed; 3 ignored; 0 measured; 0 filtered out; finished in 6.50s
cargo fmt --all --check && echo FMT_OK   -> FMT_OK
cargo clippy --workspace --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.89s
```

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | diff limited to crates/vr-ide (6 files); out-of-scope note above |
| BASE-2 | pass | no planned contract changed; additive helpers listed under Deviations |
| BASE-3 | pass | red line per test in Progress; the two gate unit tests were written right after the code, red shown by a mutation run |
| BASE-4 | pass | Commands run section |
| BASE-5 | pass | full crate test via run-heavy.sh; nothing left running |
| BASE-6 | pass | comments explain why, English |
| BASE-8 | pass | no manifest change |
| BASE-10 | pass | only `-p vr-ide`; find_tests failure rerun once, reported not mine |
| BASE-11 | pass | no tree-rewriting git commands used |
| BASE-14 | pass | every command finished within the foreground limit |
| BASE-15 | pass | no background tasks started |
| BASE-16 | pass | Reuse table; capabilities.md +4 lines |
| BASE-17 | pass | no installs |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | this file, one progress line per step |
| BASE-24 | pass | nothing staged or committed |
| BASE-25 | pass | fmt ok, clippy --workspace -D warnings clean |
| BASE-32 | pass | D82/D95/D118/D120/D126 honoured; 6 new `D? (FR-5)` entries (D120/D126 amended for the gate) |
| BASE-38 | pass | rustfmt --edition 2024 on the 6 changed files only |
| COST | pass | related tests per step, one full crate run at the end, no polling loops, no snapshot regeneration needed |
n-a: none of the listed IDs.
