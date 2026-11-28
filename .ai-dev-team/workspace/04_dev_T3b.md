# T3b notes: chord parity TUI vs window

## Files changed
- crates/vr-ide/src/event_loop.rs: `chord_from_key` unshifts a US shifted symbol and sets shift whenever ctrl or alt is held, regardless of the incoming SHIFT flag (crossterm drops it for kitty alternate keys). New test `shifted_symbols_with_ctrl_or_alt_name_the_shift_even_when_the_terminal_drops_it`.
- crates/vr-window/src/input/key.rs: plain shift+space now yields `Char(' ')` with SHIFT (what crossterm gives for kitty CSI-u `ESC[32;2u`), so both name it "shift+space"; plain space unchanged. Test row "shift+space" in key/tests.rs updated to expect SHIFT.

## Red evidence (before fix)
test event_loop::tests::shifted_symbols_with_ctrl_or_alt_name_the_shift_even_when_the_terminal_drops_it ... FAILED
thread 'event_loop::tests::shifted_symbols_with_ctrl_or_alt_name_the_shift_even_when_the_terminal_drops_it' (3465660) panicked at crates/vr-ide/src/event_loop.rs:552:9:
assertion `left == right` failed
  left: "ctrl+!"
 right: "ctrl+shift+1"
test result: FAILED. 15 passed; 1 failed; 0 ignored; 0 measured; 1131 filtered out; finished in 0.51s
test input::key::tests::key_table ... FAILED
thread 'input::key::tests::key_table' (3465936) panicked at crates/vr-window/src/input/key/tests.rs:456:5:
test result: FAILED. 17 passed; 1 failed; 0 ignored; 0 measured; 31 filtered out; finished in 0.00s

Parity harness before fix (scratch copy, rows added: shift+space, alt+shift+1, ctrl+alt+shift+`): 5 NO rows: shift+space (shift+space vs space), alt+shift+1 (alt+! vs shift+alt+1), ctrl+alt+shift+` (ctrl+alt+~ vs ctrl+shift+alt+`), ctrl+shift+/ (ctrl+? vs ctrl+shift+/), ctrl+shift+1 (ctrl+! vs ctrl+shift+1). Original harness win.rs backed up at qa-artifacts/T3b/win.rs.orig.

## Green
- cargo test -p vr-ide event_loop: 16 passed, 0 failed
- cargo test -p vr-window input: 18 passed, 0 failed
- clippy both crates --all-targets -D warnings: clean

## Parity table after fix: 54 rows equal, 0 NO (full table: qa-artifacts/T3b/table.md)
| shift+/ (?) | ? | ? | yes |
| space | space | space | yes |
| shift+space | shift+space | shift+space | yes |
| alt+shift+1 | shift+alt+1 | shift+alt+1 | yes |
| ctrl+alt+shift+` | ctrl+shift+alt+` | ctrl+shift+alt+` | yes |
| ctrl+shift+/ | ctrl+shift+/ | ctrl+shift+/ | yes |
| ctrl+shift+1 | ctrl+shift+1 | ctrl+shift+1 | yes |

## Guard
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings: exit 0 (one pre-existing manifest warning: unused workspace dependency vr-window)

## Rule compliance
| Rule | Status |
|---|---|
| BASE-1..49 (applicable: reproduce first, related tests, no scope creep, English comments) | pass |
| BASE-50/51 (experiments from a copy) | pass (harness win.rs backed up before edit, rows added in scratch only) |
| COST | pass (nothing left running; no servers) |
| AC-W4 | pass |
Counts: pass 4, n-a 0

## Fix round 1
- Cause: the unshift in `chord_from_key` made `ctrl+{` (as written in user keymaps, parsed as `Char('{')`+ctrl) a different chord from `ctrl+shift+[`, so such bindings never fired.
- Fix: moved the normalisation to the keymap boundary. `KeyChord::new` (crates/vr-config/src/keys.rs) turns ctrl/alt + a US shifted symbol into shift + its base key, so parsed config chords and terminal/window events meet in one canonical form. `chord_from_key` no longer unshifts itself. Display reads "ctrl+shift+[" for a binding written `ctrl+{` (sensible, VS Code form). Plain typing (no ctrl/alt) is unchanged.
- Side effect: `ctrl+plus` now canonicalises to `ctrl+shift+=`; updated `parse_chords` in crates/vr-config/tests/config.rs and the FORBIDDEN entry in `default_keymap_obeys_user_5` (crates/vr-ide/src/app_tests.rs). Not bound by default.
- Tests (red first: config test and the vr-ide e2e test failed before the change, e.g. left `Char('{')` no shift vs right `Char('[')` shift; e2e left "\tx\n" right "x\n"; copies of the files taken under qa-artifacts/T3b/copy/):
  - vr-config `ctrl_or_alt_with_a_shifted_symbol_is_the_shift_chord_of_its_base_key`; existing `unquoted_keys_with_quote_or_brace_parse` still passes.
  - vr-ide `user_binding_with_a_shifted_symbol_fires_from_either_spelling_of_the_key`: bindings `ctrl+{` and `ctrl+shift+[`, events `Char('{')`+CTRL and +CTRL|SHIFT, all run `edit.outdent`.
  - event_loop test extended with SHIFT already set (ctrl+alt+shift+`, alt+shift+1, ctrl+shift+/, shift+space).
  - vr-window key table rows "alt+shift+1" and "ctrl+alt+shift+`" through `translate_key`.
  - Cross-frontend unit test not added: vr-window and vr-ide do not depend on each other; the harness table plus the shapes above cover it.
- Results: vr-config 17+29+8 passed; vr-ide `--lib` 1141 passed, 0 failed (7 ignored); vr-window `input` 18 -> all passed (now with 2 more rows); parity harness re-run: 54 rows equal, 0 NO (qa-artifacts/T3b/table2.md).
- Guard `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`: exit 0.
