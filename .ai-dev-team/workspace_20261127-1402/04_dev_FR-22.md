# FR-22 notes

- crates/vr-git/src/lib.rs: GitError doc paragraph rewrapped at 100 columns, same words (no line over 100 left).
- crates/vr-ide/src/theme.rs: `Theme.focus` doc now names its only users (explorer separator in ui/explorer.rs, terminal panel edge in ui/terminal_view.rs); the Git popup mention is gone.
- Comments only, no code change.

Guard (run-heavy.sh, worktree): `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings` -> Finished, no warnings.

## Rule compliance
| Rule | Status |
|---|---|
| BASE-1 | pass (comments only) |
| BASE-2 | pass |
| BASE-3 | pass |
| BASE-4 | pass |
| BASE-9 | pass |
