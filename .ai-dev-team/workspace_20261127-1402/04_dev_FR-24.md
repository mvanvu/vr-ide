# FR-24 notes

Files: crates/vr-config/defaults/keymap.toml, crates/vr-config/tests/config.rs, crates/vr-ide/src/terminal_right_tests.rs, README.md, docs/keymap.md.

- Red first: new test `ctrl_f8_and_ctrl_f9_resize_from_a_focused_terminal` failed ("width enhanced=true Right").
- Added `ctrl+f8`/`ctrl+f9` to `[terminal]`; config allowlist extended.
- Green: `cargo test -p vr-config --test config` 23 passed; `cargo test -p vr-ide --lib terminal` 90 passed (incl. terminal_right); `cargo fmt --check && cargo clippy --workspace --all-targets -D warnings` clean.
- Live walk not run (optional).

## Rule compliance
Rules in $TEAM_WS/rules_run3.md: BASE-1..48, COST-1..20.
- pass: BASE-1, BASE-3, BASE-4, BASE-25, BASE-32, BASE-35, COST-8, COST-10
- n-a: all other BASE and COST IDs (small keymap/docs change; no server, deps, UI layout, or background processes)
Nothing left running.
