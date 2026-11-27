# FR-17 ctrl+f7 (terminal.toggle_position) not working in VTE
- Skills called: ai-dev-team:implementation
- Root cause (reproduced live, BASE-37): ctrl+f7 DOES reach vr-ide from VTE (legacy `ESC [ 18 ; 5 ~` decodes to ctrl+f7; hidden panel / editor focus toggled the position). But after `ctrl+\`` the focus is in the terminal, where D8 hands every key not listed in `[terminal]` to the program: the shell received the sequence and printed ";5~" and the panel did not move. The old test even asserted "went to the program" (stale: panel position was already Right). So no rebind needed (USER-5 / D313 not triggered: key reaches vr-ide, is not a VTE/GNOME key).
- Fix: `"ctrl+f7" = "terminal.toggle_position"` added to the `[terminal]` table of crates/vr-config/defaults/keymap.toml (global binding kept). D227 unchanged (hidden panel stays hidden; session only).
- Files changed: crates/vr-config/defaults/keymap.toml, crates/vr-config/tests/config.rs (terminal repeat allowlist), crates/vr-ide/src/terminal_right_tests.rs (ctrl_f7_toggles_the_position: focused terminal, kitty + legacy), docs/keymap.md (2 places), README.md.
- Red: `cargo test -p vr-ide ctrl_f7` failed "focused terminal, enhanced=true left: Bottom right: Right"; green after fix.
- Live pty (qa-artifacts/FR-17/walk.py, reuses T10/vt.py), 120x40, VTE bytes: before = shell shows ";5~" (focused terminal); after = panel 120x15 bottom -> 48x36 right, no stray text.
- Commands: `cargo test -p vr-ide -p vr-config --no-fail-fast`: vr-ide lib 638 passed, 0 failed (7 ignored); vr-config lib 17, tests/config.rs 23 (after allowlist fix), others ok. Final: `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test -p vr-ide terminal_right` 14 passed.
- Out-of-scope noticed: f8 / shift+f8 / ctrl+f8 / ctrl+f9 (terminal next/prev/grow/shrink) also go to the program in a focused terminal (docs say "from the editor"); left unchanged.
- Nothing left running; no commit/stage.

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| legacy bytes helper | capabilities.md | reused app_tests::legacy_event / kitty_event |
| pty screen model | capabilities.md T10 | reused qa-artifacts/T10/vt.py; new walk.py in qa-artifacts/FR-17 |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | diff limited to keymap, tests, docs |
| BASE-3 | pass | red then green: ctrl_f7_toggles_the_position |
| BASE-4 | pass | outputs above |
| BASE-10 | pass | related tests + vr-ide/vr-config only |
| BASE-11 | pass | no git index changes |
| BASE-24 | pass | stayed on branch fr-17 |
| BASE-25 | pass | fmt + clippy guard clean |
| BASE-32 | pass | D279-5 (ctrl+f7/f8/f9 editor-only) -> D314 (leader, confirm-on-return): ctrl+f7 also in [terminal]; f8/shift+f8/ctrl+f8/ctrl+f9 stay editor-only. D313 not triggered (key reaches vr-ide) |
| BASE-35 | pass | USER-4/5 honoured |
| BASE-37 | pass | live pty before/after |
| COST-1 | pass | no full suite beyond the two crates |
| COST-8 | pass | batched edits, Edit/perl only |
| COST-10 | pass | grep for tests naming the keymap/terminal files |
| COST-16 | pass | one build dir, runs through run-heavy |
| AC-F1 | pass | VTE bytes toggle from editor and focused terminal (test + pty) |
| USER-5 | pass | key unchanged, ctrl-only, reaches vr-ide legacy |
n-a (not exercised by this keymap/docs/test fix): BASE-2 BASE-5 BASE-6 BASE-7 BASE-8 BASE-9 BASE-12 BASE-13 BASE-14 BASE-15 BASE-16 BASE-17 BASE-18 BASE-19 BASE-20 BASE-21 BASE-22 BASE-23 BASE-26 BASE-27 BASE-28 BASE-29 BASE-30 BASE-31 BASE-33 BASE-34 BASE-36 BASE-38 BASE-39 BASE-40 BASE-41 BASE-42 BASE-43 BASE-44 BASE-45 BASE-46 BASE-47 BASE-48 COST-2 COST-3 COST-4 COST-5 COST-6 COST-7 COST-9 COST-11 COST-12 COST-13 COST-14 COST-15 COST-17 COST-18 COST-19 COST-20 
- Fix round 1: README.md focused-terminal bullet names ctrl+f7 and says other panel keys are editor-only; README/keymap.md lines rewrapped. Docs only.
