# FR-3 Racing tests: timing-free both-outcomes coverage (tests only)
- Worktree: .worktrees/FR3 (branch fr3-racing-tests). Only file changed: crates/vr-core/src/testutil.rs.
- Progress: read review QP1 -> (1) deterministic outcomes -> (2) no-version Unprompted case -> (3) diagnostic panic -> related tests -> mutation red -> load loop.

## Changes
1. `assert_both_outcomes(target)` is now timing-free: one run with no writer must succeed (ours on disk), one run with a hook write at `BeforeSwap` must refuse (their bytes survive). Called for Own, Other, Absent, Unprompted. The racing sweeps no longer assert on ok/refused counts; the strict rule still judges every racing run.
2. Unprompted, no version taken: the result must be `Err(DocError::Io)` "keeps changing", ours must not be on disk, nothing is excused (every version is checked). `SaveAsVersionTaken` hook is drained after each run (counter `no_version` in the stderr line).
3. The "is in no file" panic now lists every unaccounted version with mode, visible, saw_ours, replaced_is_ours; comment names the known false positive (InPlace, !visible, saw_ours, replaced_is_ours, last version). Added field `WriterVersion.mode` (additive; not constructed outside testutil.rs). Rule itself unchanged (D77).

## Evidence
- Red: moving the refusal hook to `BeforeSwapBack` => `Unprompted: refusal expected` (testutil.rs:879), only the deterministic check fails; restored.
- `cargo test -p vr-core -p vr-fs -p vr-search -p vr-ide`: vr-core 135, vr-fs 43, vr-ide 79, vr-search 35+2, all passed, 0 failed.
- clippy `-p vr-core --features vr-core/test-util --all-targets -D warnings`: clean. rustfmt --check testutil.rs: ok.
- Load loop (200x `fast_writer unprompted`, 14 busy loops alongside): see final result in report; script qa-artifacts/FR-3/loop.sh.

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | only crates/vr-core/src/testutil.rs |
| BASE-2 | pass | public signatures unchanged; additive field `WriterVersion.mode` |
| BASE-3 | pass | mutation red above (item 1); items 2/3 are harness branches checked by the load loop |
| BASE-4 | pass | outputs above |
| BASE-6 | pass | English why-comments |
| BASE-32 / D77 | pass | oracle not loosened; the panic only reports more |
| D96 | pass | tests only |
n-a: BASE-5, BASE-8, BASE-10..25 (no builds beyond cargo test, no deps, no stash/git), COST.
