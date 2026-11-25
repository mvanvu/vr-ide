# FR-15 (bug fixes after M7-4b QC)

## 1. Flaky under load
Repro: 16 `yes` loops, 5 runs of the three tests: each_root_shows_its_own_repo failed 2/5 (`shown_marks(&pa)` None vs Some(0)), add_folder_checks_run_off_the_ui_thread 1/5 (empty notices at the final assertion). replace_never_loses_a_concurrent_write did not fail in 17 loaded runs (alone and with its module).
- each_root: root a's first marks answer was still in flight when b opened; the test now waits for a's marks (`wait_marks`) after open_path(&pa). Same assertions kept.
- add_folder: with a zero wait the check can answer before recv_timeout, so the refusal is the slash prompt's error, not a late notice. The test accepts the cause in notices or the slash error.
- replace_never_loses: suspected cause (not reproduced): the writer's timing drifts under load and one 40-offset sweep sees only one outcome, failing the "both outcomes occurred" assertion. `race_enter_on` now sweeps again (up to 5 sweeps) until both occurred; every iteration keeps the lost-write assertions.
Evidence: 5x in a row under 16 `yes` loops (started and killed by pid): all 3 pass every time.

## 2. /git show refusal at 40x12
Cause: the refusal was only a notice, drawn on the row the popup covers. Fix: `GitPopup.refusal` (git_popup.rs) is set on refusal, shown first on the History/Blame status row (git_history::status_line), cleared on the next key or the next show. Test git_history_tests::git_show_refusal_is_visible_in_a_small_popup (120x40, 80x24, 40x12).

## 3. Wide char at the left scroll edge
Cause: `window` skipped the whole wide char, so the rest drew one column early. Fix: blank cells for the visible right half (ui/git_history.rs). Test ui::git_history::tests::a_wide_char_cut_by_the_left_edge_keeps_the_rest_at_its_column (CJK, odd scroll).

## 4. README
Narrow-popup tab bar note added (README.md, Git section).

## Commands
fmt --check ok; clippy --workspace --all-targets -D warnings ok; cargo test -p vr-ide --lib targeted tests ok; full `cargo test --workspace` via run-heavy exit 0 (vr-ide 635 passed, 0 failed, 7 ignored); release build ok. Nothing running (load loops killed).

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-4/5/10 | pass | commands above |
| BASE-11 | pass | nothing committed, stashed or reset |
| BASE-15 | pass | only my pids killed |
| SEC-15 | pass | refusal text goes through `sanitize` as before |
| Tests not weakened | pass | assertions kept; only waits/sweeps/accepted channels added |

## Quick pass
Review findings fixed (05_review_FR-15.md):
- [major] `add_folder_checks_run_off_the_ui_thread` (crates/vr-ide/src/workspace_ui_tests.rs): a test-only gate (`App.ws_check_gate`, `#[cfg(test)]`, taken by `add_root_checked` in crates/vr-ide/src/app_workspace.rs; worker waits on it, 20 s cap) holds the check. The test now requires, on every run: Enter returns with `ws_job` pending and the "waiting for the file system" notice, no "does not exist" yet; then it releases the gate and requires the late notice "does not exist". The "either path" assertion is gone.
- [nit] a mouse click on a tab clears `GitPopup.refusal` (crates/vr-ide/src/git_popup.rs); test `a_tab_click_clears_the_refusal` in crates/vr-ide/src/git_history_tests.rs.
Proof:
- Mutation: `rx.recv_timeout(self.ws_job_wait)` -> `Duration::from_secs(30)` (UI thread waits for the check): test failed at "Enter must return while the check runs" (20.01s). Restored; `cmp` against the backup identical.
- 5x under 16 `yes` (killed by pid afterwards): 2 passed each time, 0.09-0.17s.
- `cargo test -p vr-ide`: 636 passed, 0 failed, 7 ignored. clippy `-p vr-ide --all-targets -D warnings`: clean. `cargo fmt --check`: ok.
- Nothing left running.
