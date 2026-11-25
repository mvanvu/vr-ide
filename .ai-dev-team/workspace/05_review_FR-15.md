# Review FR-15 (light tier) - CHANGES_REQUESTED

## Findings

[major] crates/vr-ide/src/workspace_ui_tests.rs:581 - add_folder_checks_run_off_the_ui_thread weakened to "any refusal"
Scenario: run_ws_job (app_workspace.rs:77) returns the result synchronously when the check answers within recv_timeout(ZERO). That is the synchronous path the test exists to exclude. The new assertion also accepts the slash prompt's error, so on that path the test passes without ever seeing the job outlast the wait (no "waiting for the file system" notice, no ws_job). The first half only waits for len>=2 and passes on the sync path too. Under load the test now proves only "a refusal names the cause".
Fix: keep the late-notice property. Either assert the late path on every run (retry the refused add on a fresh nonexistent path until `app.ws_job.is_some()` right after Enter, bounded, then drain and require the notice "does not exist"), or make the check deterministically slower than the wait. Accepting the slash error may stay as a fallback only if the test also records that the late path was seen at least once.

## Checked, no findings
- each_root_shows_its_own_repo: wait_marks for root a before opening b; same assertions kept. OK.
- replace_never_loses_a_concurrent_write (race_enter_on): the lost-write and asks_again assertions still run in every iteration. The loop is bounded (at most 5 x 40 iterations); it breaks only at a multiple of 40 once both outcomes occurred; the final both-outcomes assert is kept. Cannot loop forever. Shared by the other race tests; only adds sweeps. OK.
- GitPopup.refusal: set on a refused show, cleared by load_commit, by every key (handle_key) and by open(); shown first on the History/Blame row only while no commit view is open. Test covers 120x40, 80x24 and 40x12. Nit (minor): a mouse tab click does not clear it, so it stays until the next key.
- window(): the blank cells for the right half of a wide char cut at the left edge are correct (skipped-left is at most cw-1, counted in total). The right-edge cut is unchanged and already handled by the `cut` flag and the "…" in draw_window. The new unit test covers the left edge only; it has no right-edge case, but the code path is unchanged.
- README line: accurate.

Spot-run: `cargo test -p vr-ide --lib git_history` 23 passed; the three named tests plus project_replace pass (4 passed).

## Files read
README.md read; dialog_tests.rs read; git_history.rs read; git_history_tests.rs read; git_popup.rs read; git_tests.rs read; ui/git_history.rs read; workspace_ui_tests.rs read; 04_dev_FR-15.md read.

## Rule check FR-15
| Rule | Status | Evidence |
|---|---|---|
| BASE-4/5/10 | pass | notes' commands; targeted tests rerun green |
| BASE-11 | pass | tree uncommitted, nothing stashed |
| BASE-15 | pass | no foreign process touched |
| SEC-15 | pass | refusal text is the already sanitized msg |
| Tests not weakened | fail | workspace_ui_tests.rs:581 accepts the synchronous slash error |
