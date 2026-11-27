# FR-33 notes (worktree .worktrees/FR-33, branch fr-33)

## Changes
- crates/vr-ide/src/commands/terminal.rs: `terminal.new` (runs `new_terminal`) and `terminal.close` (runs `close_tab_asking` on the active tab, D296 question included) registered.
- crates/vr-ide/src/commands/palette.rs: `PALETTE_HIDDEN` keeps both out of the palette (D291 kept; registry entry is only needed so `[terminal]` can bind them).
- crates/vr-config/defaults/keymap.toml: `[terminal]` ctrl+n / ctrl+w; header comments updated.
- crates/vr-config/tests/config.rs: KNOWN and terminal repeat list. crates/vr-ide/src/terminal_tests.rs: 4 new tests; the AC-14a test no longer sends ctrl+w to the program.
- README.md, docs/keymap.md: key tables and [terminal] text.

## Tests
red first: 3 of 4 new failed. After: vr-ide lib 716 pass / 7 ignored, vr-config 17+24+8 pass.
Guard: `cargo fmt --all --check` no output; `cargo clippy --workspace --all-targets -- -D warnings` Finished, no warnings.
No palette or help text change, so no snapshots regenerated.

## Rule compliance
| Rule | Status |
|---|---|
| AC-K1 | pass |
| AC-K2 | pass |
| USER-5 | pass |
| BASE-11 / BASE-24 / BASE-25 / BASE-38 | pass |
| COST | pass |
| others | n-a |
