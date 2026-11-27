# FR-4 notes
Red: ai_diff_header_keeps_a_right_margin failed on old code ("...revert│"); picker title test written against the cut title (snapshot showed 'New session in /home/user/DEV/').
Change: ai_changes.rs kx = x+(w-kw-1), fits reserve 3; ai_view.rs folder = status_bar::shorten_left(folder, w-2-lead).
Green: `INSTA_UPDATE=no cargo test -p vr-ide --lib -- ai_view ai_changes` 37 passed.
Snapshots: ai_changes_120x40, ai_changes_limits_120x40, ai_revert_confirm_120x40, ai_account_busy_80x24, ai_account_remove_80x24 (regenerated once, INSTA_UPDATE=always).
Rule compliance: USER-2 n-a, USER-5 n-a, others pass.

## Guard (BASE-25)
cargo fmt --all --check: FMT_OK
clippy --workspace --all-targets -- -D warnings: Finished dev profile, CLIPPY_EXIT=0 (no warnings)
INSTA_UPDATE=no cargo test -p vr-ide --lib -- ai_view ai_changes: test result: ok. 37 passed; 0 failed

## Review — 2026-11-27
Verdict: APPROVED. No findings. Files read: ai_changes.rs, ai_view.rs, both test files (read), 5 .snap files (read, diffs only the intended rows).
Notes (no action): the head-fit reserve (3 + keys) matches kx = w - kw - 1, so no underflow when keys show; shorten_left is reused unchanged (status_bar.rs:288); the 80x24 snapshot shows "…er/DEV/vr-ide " (folder name kept, 1 free cell).

## Rule check FR-4
| Rule | Result |
|---|---|
| Text not cut without a rule (… keeps folder name) | pass |
| 1-cell right margin on the diff header | pass |
| Mockups (.ans) vs snapshots | pass (80x24 shortens, 120x40 fits) |
| USER-2 / USER-5 | n-a |
| SWEEP-4 (other callers; shorten_left unchanged) | pass |
| BASE red-first | pass (red recorded in notes) |
| BASE-25 guard in notes | pass |
