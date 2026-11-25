# Review T21: keyset v2 (code + design QA)

Verdict: **CHANGES_REQUESTED** (2 major, 2 minor). Two open user decisions (D264, D265) are reported, not resolved.

Tools used: Read, Bash (git diff, grep, run-heavy.sh release build, pty walks), Write (this file only).

## Findings

[major] crates/vr-ide/src/commands/explorer.rs:1478 + crates/vr-ide/src/commands/find.rs:614 — Tab focus cycle gets stuck in the find bar
Scenario: file open, explorer shown, find bar open (ctrl+f, type, ctrl+b, esc). The editor has the keys in read mode. Tab moves focus to the find bar (explorer.rs:1478). After that, Tab is the find bar's own field switch, which does nothing when the bar has no replace row. Pressing Tab more times never reaches the explorer or the terminal. Review walk `qa-artifacts/T21/review/r1.txt`, lines B to E: after 3 Tabs, typing `q` lands in the find field ("Find alphaq", `r1_05_typed_q.txt`). docs/keymap.md (Read-mode `tab` row), the keymap.toml header and README all promise "editor → find bar → side panel → terminal → editor". AC-21b ("tab cycles focus") fails while the bar is open.
Fix: make what Tab does match the documented cycle. Either the read-mode Tab skips the find bar, or Tab in the find bar runs `focus.cycle` once it leaves its last field (find field when there is no replace row, replace field otherwise). Add a key-level test with the bar open and the explorer shown. The user decides which option. D265 covers the search panel keeping Tab but not the find bar.

[major] crates/vr-ide/src/commands/find.rs:379-383, crates/vr-ide/src/app.rs:1152 — changed behaviour without tests
Scenario: three new paths have no test. (1) ctrl+r on a read-only file: `enter_edit_mode` warns, but the replace row opens anyway. (2) ctrl+r with no file open (welcome) runs the project replace. (3) A mouse event clears a pending space (`route_mouse`). Any of these can regress without a test failing. The mode test comment "A click between the two keys forgets the first space" (mode_tests.rs) actually sets `app.focus = Explorer` and clicks nothing, so it tests the focus path, not the click path.
Fix: add a test for each case. For (1): assert the warning, that read mode is kept, and that Enter / ctrl+a in the replace field leave the text unchanged. For (2): assert `search.replace_mode` and focus Search. For (3): space, a mouse click on the editor, then `i` → still read mode. Also correct that test comment.

[minor] crates/vr-ide/src/app.rs:191 — a pending space eats the next Esc and survives a paste
Scenario: read mode with a notice shown, the user presses space then Esc. The notice stays and SPACE_HINT is added (walk F, `r1_06_space_esc.txt`). Also, `handle_paste` does not clear `space_pending`: space, then a bracketed paste (refused in read mode), then `i` enters edit mode.
Fix: let Esc cancel a pending space without showing the hint, and clear `space_pending` in `handle_paste`.

[minor] crates/vr-ide/src/ui/find_bar.rs:20 — "Enter one · ctrl+a all" spells a key with a capital
Scenario: every other key label in the UI is lower case (welcome "ctrl+o Open file", the palette key column, docs: `enter`). The find bar replace row shows "Enter".
Fix: "enter one · ctrl+a all" (update the snapshot and docs/README quotes too).

## Open user decisions (not findings; leave to the user)
- D264: shift+f3 never arrives without the kitty protocol. Reproduced on a fresh release build: walk 79/80, FAIL "shift+f3: previous match". AC-21a stays partial until the user decides.
- D265: the replace-all key in the replace field is `ctrl+a`, which is not in the spec (the spec says "asks one / all"). Also not in the spec: **Tab in the explorer runs focus.cycle** (explorer.rs:487). Both are built-in key meanings the code adds. Tab-in-explorer is missing from D265 and should be added there for the user.

## Checked, no finding
- keymap.toml matches the spec table exactly. vr-config `defaults_load_without_user_file` now also fails on any binding missing from KEYSET_V2, and every removed key is free (ctrl+f7 free for T19).
- Read-mode `space` handling: the pending space is cleared on focus change, edit mode, dialog and mouse. It never reaches typing in edit mode, dialogs, the palette, the find bar or a focused terminal (`read_mode_key` runs only when the focus is Editor and not in edit mode, after the modal, terminal and find-bar routing). No stuck state: a pending space is consumed by exactly one key.
- In edit mode Tab indents and shift+tab outdents. ctrl+i (kitty) is unbound. Legacy 0x09 cycles the focus.
- ctrl+r: editor → in-file replace in edit mode; search panel and explorer → project replace.
- Explorer ctrl+n: File / Folder dialog, File is the default, Esc cancels, then the name prompt (walk 46-49).
- Removed keys do nothing (walk 64-75, `removed_keys_run_nothing`).
- docs/keymap.md, README, welcome, palette column and the read-mode hint agree with the code (apart from the Tab-cycle claim above).
- Snapshot diffs are all intended: the welcome row (Search → ctrl+o Open file), the palette key column (f12 / ctrl+t / f4 / shift+f4 removed, f6 → tab), and the find bar help row. The terminal snapshots changed only in the welcome row behind them.

## Design QA
Sheets: the 13 changed snapshots plus walk screens (`qa-artifacts/T21/review/walk_*.txt`, `r1_*.txt`). Welcome 40x12 / 80x24 / 120x40: no overflow. The palette key column is aligned and empty for palette-only commands. Find bar 80x24: the help text is right-aligned and the replace field keeps ≥ 10 cells; the help is hidden below 36 columns. The only UI finding is the "Enter" capital above (minor).

## Files read
| File | Status |
|---|---|
| crates/vr-config/defaults/keymap.toml, src/keymap.rs, tests/config.rs | read |
| crates/vr-ide/src/app.rs, commands/{dialog,edit,explorer,find,search}.rs | read |
| crates/vr-ide/src/ui/{find_bar,mod,terminal_view,welcome}.rs, ui/tests/{preview,welcome_tests}.rs | read |
| crates/vr-ide/src/{app,mode,explorer,find,search,dialog,syntax,terminal,workspace}_tests.rs | read (diff) |
| 17 .snap files | read (changed rows) |
| docs/keymap.md, README.md | read |
| .ai-dev-team/* | spec, D261-D265, dev notes read; others are team notes |

## Commands
- `run-heavy.sh cargo build --release -p vr-ide`: rebuilt, because welcome.rs / terminal_view.rs were newer than the dev's binary. Finished.
- `qa-artifacts/T21/review/walk.py` (dev walk, legacy bytes): 79 / 80 (shift+f3, D264). Output in `walk.txt`.
- `qa-artifacts/T21/review/r1.py` (review edge cases): output in `r1.txt` and `r1_*.txt`.

## Rule check T21
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | diff limited to keys, read-mode built-ins, replace entry, File/Folder, docs, tests |
| BASE-2 | pass (open D265) | keymap = spec table (config test asserts no extra binding); ctrl+a replace-all and explorer Tab are user-pending |
| BASE-3 | pass | red-first list in notes; consistent with the tests |
| BASE-4 | pass | walk reproduced (79/80) on a fresh release build |
| BASE-5 | pass | full suite and build via run-heavy.sh in the notes |
| BASE-6 | pass | comments explain why |
| BASE-16 | pass | ConfirmDialog, prompt_new_in, key_event reused |
| USER-5 | pass | no alt / ctrl+shift / F10 / F11; exceptions space prefix and ctrl+h approved; `default_keymap_obeys_user_5` |
| COST-15 | pass | snapshot rows changed only where intended |
| AC-21a | partial | everything except shift+f3 legacy (D264) |
| AC-21b | fail | Tab cycle stuck in the find bar (major 1) |
| AC-21c | pass | in-file and project replace verified; read-only and no-file branches untested (major 2) |
| AC-21d | pass | REMOVED_V2 tests + walk |

## Round 1 re-check
Verdict: **APPROVED**. All 4 findings fixed with tests that fail without the fix. D264 / D265 left open, no key proposed.

Tools used: Read, Bash (git diff, grep, cargo test, r1.py).

### Findings re-verified
1. Tab stuck in the find bar: fixed. `explorer.rs:1478-1491` `focus_cycle` no longer moves Editor to FindBar (the FindBar arms only matter if focus.cycle runs from the bar, e.g. the palette). `mode_tests::tab_cycle_skips_the_open_find_bar` covers legacy and kitty, bar open and explorer shown, 6 Tabs, `q` typed at each editor stop, the find field stays "alpha", ctrl+f still reaches the bar. It is a real key-level test. `r1.py` on the release binary: "E typed q goes to find field: False". Docs (`docs/keymap.md:27`, README:138, keymap.toml header lines 25-26) all say the find bar is outside the cycle, which matches D266.
2. Untested paths: fixed. Read-only ctrl+r (`find.rs:383-387`) is covered by `read_only_file_stays_in_read_mode_on_space_i_ctrl_r_and_palette` (warning, read mode, no bar or replace row, Enter and ctrl+a leave the text unchanged). The no-file ctrl+r test and the real mouse-click test (`handle_mouse` Down(Left) on the editor, then `i` stays read) are present. The dev reports the first two characterisation tests green before the fix, as asked. The misleading comment was corrected.
3. Esc / paste: fixed. `app.rs:785-790` Esc cancels the pending space and returns false, so the notice is dismissed with no hint. `handle_paste` (app.rs:1133) clears it. Tests `esc_after_space_cancels_it_and_dismisses_the_notice` and `a_paste_between_space_and_the_next_key_drops_the_space` assert this. `r1.py` F: no notice left.
4. Label: fixed. `find_bar.rs:20` "enter one · ctrl+a all", the README and docs quotes, the mode test assertion and the find bar snapshot are all lower case. A grep finds no remaining "Enter one".

### No key binding change
The keymap.toml diff versus base abdbe42 holds only the keyset-v2 changes already in the spec table (removals, `ctrl+h`, `ctrl+r`, `ctrl+n`). Round 1 added nothing: the keymap.toml header change is comments only, and the vr-config test still rejects any extra binding.

### Commands
- `cargo test -p vr-ide mode_tests`: 34 passed. `find_tests`: 9 passed, 1 ignored.
- `r1.py` on `target/release/vr-ide`: E False, F no hint, G palette ok.
- The full suite, clippy, fmt and the release build are taken from the dev notes (not rerun).

### Files read
| File | Status |
|---|---|
| crates/vr-ide/src/commands/explorer.rs (focus_cycle), commands/find.rs (replace_open) | read |
| crates/vr-ide/src/app.rs (read_mode_key, handle_paste) | read |
| crates/vr-ide/src/ui/find_bar.rs, mode_tests.rs (new tests) | read |
| docs/keymap.md, README.md, crates/vr-config/defaults/keymap.toml | read |
| the find bar snapshot | skipped, label row only per the dev notes; the mode test asserts the label |

## Rule check T21
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | round 1 touches only the 4 findings |
| BASE-2 | pass (open D264, D265) | no binding added or changed; D266 applied; D264 / D265 untouched |
| BASE-3 | pass | red_first.txt lists 5 red tests; the 2 characterisation tests are named |
| BASE-4 | pass | mode_tests 34 and find_tests 9 pass; r1.py rerun |
| BASE-5 | pass | full suite and build via run-heavy.sh in the notes |
| BASE-6 | pass | comments explain why |
| BASE-16 | pass | existing helpers reused |
| USER-5 | pass | no key change |
| COST-15 | pass | one snapshot row changed |
| AC-21a | partial | shift+f3 without kitty (D264, user decision) |
| AC-21b | pass | Tab cycle test and walk |
| AC-21c | pass | read-only and no-file branches now tested |
| AC-21d | pass | unchanged |
