# Review T18 — own keyset, leader chord engine, which-key popup, display keys

Verdict: **CHANGES_REQUESTED**

Tools available: Read, Bash, Write, Skill, SendMessage, SubagentHandback.
Scope: `git diff -- crates docs README.md` + untracked `chord.rs`, `which_key.rs`, `chord_tests.rs` (main tree, T18 uncommitted).
Release build: `run-heavy.sh cargo build --release -p vr-ide` OK; pty walk `qa-artifacts/T18/review/wk.py` (legacy keyboard, TERM=xterm-256color, SHELL=/bin/sh, ENV=/dev/null).

## Findings

[major] crates/vr-ide/src/commands/terminal.rs:326-330 (with toggle at :276-282) — a focused terminal in a short window can no longer be left or hidden
Scenario: window 80x7 (editor gets 0 rows), focus the terminal (ctrl+`), press ctrl+` again → `focus_editor` returns early, nothing happens; status still promises `ctrl+` editor`. Before T18 the same key hid the panel. `<leader> j` cannot be typed (ctrl+d goes to the shell — in the walk it sent EOF and killed the shell). Keyboard-only users are stuck until they resize. Live: `short_terminal_7rows_*.txt`. Test `a_zero_row_editor_never_gets_the_keys_or_a_monitor` asserts the stuck state instead of a way out. Same trap with `terminal_pass_nul = true` (D237 "click the editor") when there is no editor row to click.
Fix: in `toggle`, when the focused terminal cannot give the keys back (editor has no rows), hide the panel (old behaviour) — and let the hint say `ctrl+` hide` in that state; add a test.

[major] crates/vr-ide/src/ui/which_key.rs:39-59 — which-key popup cuts the list at 80x24 and 40x12 (COMPACT)
Scenario: leader at 80x24 → 21 of 29 rows shown, `q r s t u up v y` never visible (`leader_popup_80x24.txt`, also in the editor: `leader_popup_editor_80x24.txt`, ends at `o +more …`); at 40x12 only 9 rows, and descriptions are cut mid-word without ellipsis (`Terminal: Focus Previous Termin`, `leader_popup_40x12.txt`). The popup is the only discovery aid for the leader keys, and 80x24 is the default terminal size. There is horizontal room for two columns at 80 wide (popup is 44 wide).
Fix (cost order): lay rows out in as many columns as the width allows (2 at 80, 3 at 120); shorten titles in the popup (drop the `Terminal: ` / `Explorer: ` group prefix, or group by prefix); ellipsize cut text; if still too many, `?`/`space` pages. Add a snapshot at 80x24 and 40x12 asserting every key is visible or a page marker shows.

[major] crates/vr-config/defaults/keymap.toml:84, crates/vr-ide/src/app_tests.rs:617 — USER-5 broken on the agent's word: `ctrl+i` default key (BASE-35)
Scenario: USER-5 names `ctrl+i=Tab` among keys a legacy terminal cannot tell apart; the default keymap binds it and the USER-5 test whitelists it in a comment ("the one allowed exception"). No decision records the conflict between D145 (the user's ctrl+i) and USER-5. Functionally it works (Tab in read mode), but the display key `ctrl+i` is shown in the palette and the read-mode hint, and the test is not honest about USER-5.
Fix: leader records a decision naming D145 vs USER-5 and the options (keep ctrl+i as the documented D145 exception, or a reachable key such as `<leader> i`-group / `ctrl+r` style replacement), takes it per BASE-35; the test's allowance then names that decision.

[major] crates/vr-ide/src/commands/chord.rs:63-74, crates/vr-config/src/keymap.rs:110-128, crates/vr-config/src/settings.rs:112-118 — a direct key that is also a sequence prefix silently never runs
Scenario A: user keymap `[editor] "ctrl+d" = "edit.duplicate_line"` → `seq_lookup(Editor,[ctrl+d])` is `Prefix` (global sequences), the chord engine eats the key, the binding never fires, no warning.
Scenario B: user adds `"ctrl+o x" = "file.goto"` → ctrl+o (file.open) is dead everywhere, silently.
Scenario C: `leader_key = "ctrl+s"` passes validation → Save is dead; `leader_key = "ctrl+i"` / `"ctrl+m"` / `"ctrl+shift+x"` / `"ctrl+`"` also pass although a legacy terminal never sends them as such (ctrl+` is taken by the NUL rule first).
Scenario D: user `"leader o" = "file.open"` → the default `leader o c/w/r` group becomes unreachable silently (exact match wins).
No test covers any of these (the brief asked for "a user binding of a key that is a sequence prefix" and continuation conflicts).
Fix: at load/`set_leader`, warn (startup notice) when a direct binding in the same or global context equals a sequence prefix, and when a sequence is also a prefix of a longer one; reject in `leader_key` keys that are `legacy_unreachable`, indistinguishable (ctrl+i/m/[/h/j/space/2/@/`) or already a direct global key. Tests for A–D.

[major] docs/keymap.md:21-23 — documented sequence syntax does not parse
Scenario: user copies `"<leader> f" = "search.project"` → `"<leader>"` is not a key → InvalidKey warning, binding dropped. Only the bare word `leader` works (same paragraph says so).
Fix: write `"leader f"` in the docs (or accept `<leader>` in `Keymap::bind`).

[major, uncertain] crates/vr-config/defaults/keymap.toml:33 — `f1` may be a key GNOME Terminal takes (USER-5)
Scenario: GNOME Terminal's default shortcut Help › Contents is F1; if active, the palette's display key (`f1`, shown on welcome, notices, docs) opens Help instead. Depends on the user's GNOME Terminal shortcut settings, not visible here. `<leader> p` still works.
Fix: leader checks GNOME Terminal › Preferences › Shortcuts; if F1 is bound, make `<leader> p` the display key or pick another reachable direct key.

[major] crates/vr-ide/src/commands/find.rs:534-541 — find option toggles silently do nothing outside the focused find bar / search panel
Scenario: find bar open, user clicks into the editor, presses `ctrl+d o c` (offered by the popup in the editor) → `_ => {}`, no toggle, no hint.
Fix: act on the visible find bar when the editor has the focus, else on a visible search panel; otherwise `app.info("open Find (ctrl+f) first")` via `key_hint`.

[minor] stale comments/docs that describe removed keys: crates/vr-ide/src/commands/find.rs:5-7 and commands/search.rs:17-18 (alt+c/r/w, ctrl+h, ctrl+shift+f), app.rs:40-42 (ctrl+h, ctrl+alt+enter, ctrl+shift+f), commands/palette.rs:150 ("fallback first (D225); the VS Code key stays bound"), ui/terminal_view.rs:38 (D225), crates/vr-config/defaults/settings.toml:21-24 (user-visible: "hides the panel", "alt+q then leaves the terminal"), settings.rs:33-35 ("instead of hiding the panel"). Fix: rewrite to the D230 behaviour.

[minor] crates/vr-ide/src/app.rs:761-765 — `on_resize` was inserted under `cursor_moved`'s doc comment; `on_resize` gets both docs, `cursor_moved` none. Fix: move the line.

[minor] crates/vr-ide/src/chord_tests.rs:250 — test name `a_focused_terminal_keeps_ctrl_k_for_its_program` says ctrl+k; the leader is ctrl+d. Rename to `..._keeps_the_leader_...`.

[minor] docs/keymap.md:16-18 — "a short notice says so" for every cancel; focus change, dialog and palette cancel silently (`chord.rs:111-112`, `cancel(app, None)`). Fix the sentence.

[minor] which-key content: on the welcome screen (no file) the editor context lists editor-only text commands (`/ a c d l r up down`) that do nothing there; at 80x24 the popup also covers the title row; symbols sort before letters, pushing mnemonic letters below the cut. Fix: hide editor sequences without an active doc; sort letters first.

## Checks that passed
- USER-5 table otherwise: no alt, no ctrl+shift+letter, no ctrl+PageUp/Down, no F10/F11, every first key legacy-deliverable; one key per command except `app.palette` (D234, proposed).
- Chord engine: Esc / unbound / second leader / 10 s timeout (also without a tick) / dialog / palette / focus change end it; resize keeps it; popup shows after 400 ms (50 ms poll) and on `?`; a focused terminal never starts a chord. `seq_lookup` prefix scan is correct for `Vec` ordering.
- Terminal: `[terminal]` holds only ctrl+`; NUL handling and `terminal_pass_nul` follow D214/D237; hints come from `key_hint`/`leave_text`.
- display_key used by welcome, palette, notices, terminal hints, explorer not-shown hint, read-mode hint; grep finds no other hard-coded key text in non-test code (Esc / Enter built-ins only).
- Removed alt handlers in find.rs/search.rs left no dead path (`toggle`/`toggle_option` now reached through the commands).
- app_tests.rs vs HEAD: every HEAD test fn still present except the three replaced on purpose (ctrl+alt alternatives, two legacy-notice tests); new helpers `leader`, `leader_key_event`, `legacy_notice_app` and two new tests; nothing else lost.
- README and docs/keymap.md tables match keymap.toml (except the `<leader>` syntax finding).

## Design QA
Screens: `/home/rainy/DEV/vr-ide/.ai-dev-team/workspace/qa-artifacts/T18/review/*.txt` (legacy keyboard, real release binary).
- Leader pending: status row right shows `ctrl+d …` at 120x40 / 80x24 / 40x12 — pass.
- Which-key popup after 0.5 s: 120x40 shows all 29 rows, readable — pass; 80x24 clipped (21/29) and 40x12 clipped + mid-word cut — **major** (finding 2). Group popup `ctrl+d o` — pass.
- Esc cancel shows `ctrl+d o: cancelled` — pass.
- Palette key column: one key per row, chords as `ctrl+d s`, right-aligned, fits at 80x24 and 40x12 — pass.
- Welcome hints: `ctrl+p / f1 / ctrl+e / ctrl+d f / ctrl+` / ctrl+q`; at 40x12 the terminal hint gives way (priority) — pass.
- Terminal: hint `ctrl+` editor` at 80x24/40x12, ctrl+` gives keys back and panel stays — pass; at 80x7 ctrl+` does nothing — **major** (finding 1).

## Files read
- README.md — read
- crates/vr-config/defaults/keymap.toml — read
- crates/vr-config/defaults/settings.toml — read
- crates/vr-config/src/keymap.rs — read
- crates/vr-config/src/lib.rs — read
- crates/vr-config/src/settings.rs — read
- crates/vr-config/tests/config.rs — read (assertions)
- crates/vr-ide/src/app.rs — read
- crates/vr-ide/src/app_tests.rs — read (diff + fn list vs HEAD)
- crates/vr-ide/src/chord_tests.rs — read
- crates/vr-ide/src/commands/chord.rs — read
- crates/vr-ide/src/commands/explorer.rs — read
- crates/vr-ide/src/commands/find.rs — read
- crates/vr-ide/src/commands/mod.rs — read
- crates/vr-ide/src/commands/palette.rs — read
- crates/vr-ide/src/commands/search.rs — read
- crates/vr-ide/src/commands/terminal.rs — read
- crates/vr-ide/src/dialog_tests.rs, explorer_tests.rs, find_tests.rs, mode_tests.rs, search_tests.rs, syntax_tests.rs, terminal_tests.rs — read
- crates/vr-ide/src/event_loop.rs — read
- crates/vr-ide/src/lib.rs — read
- crates/vr-ide/src/ui/mod.rs, status_bar.rs, terminal_view.rs, welcome.rs, which_key.rs, tests/preview.rs, tests/welcome_tests.rs — read
- snapshots (*.snap, 14 files) — welcome 40x12 and palette 80x24/40x12 read; the rest skipped — generated, covered by the live walk
- docs/keymap.md — read

## Rule check T18
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | diff limited to keymap/settings/key display/chord engine/docs; no W-1 files |
| BASE-2 | fail | docs contract `"<leader> f"` does not parse (docs/keymap.md:22) — finding 5 |
| BASE-3 | pass | red seen for display_key/USER-5/vr-config sequence tests; chord_tests red was a compile failure of the missing module (acceptable), but tests for prefix shadowing are missing — finding 4 |
| BASE-6 | pass | English; no bug history; stale comments filed as minor |
| BASE-25 | pass | notes: fmt --check clean, clippy -D warnings clean (profile guard) |
| BASE-32 | fail | D230 "ctrl+` returns the keys" — no way out at 0 editor rows (finding 1); D229/D239 cancel paths honoured; D234-D236 proposed, followed |
| BASE-35 | fail | USER-5 ctrl+i allowance on the agent's word (finding 3); F1 uncertain (finding 6) |
| new states | fail | pending chord: all exits tested; new "terminal refuses to give keys back" state has no exit (finding 1) |

## Round 1 re-check

Verdict: **APPROVED**

Scope: round-1 diff on the main tree (D249-D253, `## Round 1` of 04_dev_T18.md). Fresh release build (`run-heavy.sh cargo build --release -p vr-ide`, exit 0); pty walk `qa-artifacts/T18/review/r1.py`, legacy bytes, TERM=xterm-256color, empty VR_IDE_CONFIG_DIR: 7/7 PASS, screens `qa-artifacts/T18/review/r1_*.txt`.

### D250 table
- `crates/vr-config/defaults/keymap.toml` matches D250 key for key and context for context (all 31 entries plus the kept direct keys; ctrl+f7 only a reserved comment). A script compared it with the docs/keymap.md table: no mismatch.
- One key per command: `default_keymap_obeys_user_5` walks `bindings()` across every context (global, editor, explorer, palette, terminal) and asserts exactly one key per command, no alt, no ctrl+shift, no F10/F11, no ctrl+PageUp/Down, legacy-deliverable, no ambiguous byte. The ctrl+i allowance now names D249. The find bar, search panel and dialogs have no keymap tables; their built-in keys use no alt (grep of non-test source: ALT only in key decoding and in the explorer/dialog "ignore modified keys" guards).
- `every_d250_key_runs_its_command` dispatches every D250 key in both keyboard modes; `display_key_is_the_one_key_of_a_command` covers the whole table.
- ctrl+f2 is bound twice (global toggle_word, explorer new_folder). The context key wins (D251), and docs/keymap.md:49-52 says so.

### Leftovers
grep for `leader`, `chord::`, `which_key`, `seq_text`, `SeqMatch`, `sequences`, `DEFAULT_LEADER`, `Continuation` in crates/docs/README: only the D250 "no sequences" comments and the two config tests asserting that `leader f` / `ctrl+d f` / `leader_key` are now rejected. chord.rs, which_key.rs and chord_tests.rs are gone, and status_bar.rs is back to HEAD.

### Earlier findings
| # | Status | Evidence |
|---|---|---|
| 1 0-row editor stuck terminal | closed | terminal.rs `toggle` hides when `editor_covered`; NUL also hides with pass_nul on there; hint `ctrl+` hide`; live: r1_short_before / r1_short_after_nul (80x7) |
| 2 which-key clipped | gone | popup removed (D250) |
| 3 ctrl+i vs USER-5 | closed | D249 recorded; test allowance names D249 |
| 4 prefix shadowing / leader_key | gone | no sequences or leader_key; keys with spaces are InvalidKey (config.rs:578-610) |
| 5 docs syntax | closed | docs rewritten; `documented_keymap_examples_are_valid` loads every keymap toml block |
| 6 F1 / GNOME | closed | D249 (help shortcut disabled on this machine), D250 user decision |
| 7 find toggles silent | closed | find.rs `toggle_option_key`: focused bar/panel, else open bar, else shown panel, else hint; live: count 1/3 -> 1/2 with the bar focused, toggles again after a click into the editor |
| minors (stale comments, on_resize doc, test name, cancel notice, popup content) | closed / gone | app.rs:37-42, 759-765, find.rs header, palette.rs, terminal_view.rs, settings.toml |

### Design QA (r1)
- Welcome hints: `ctrl+p / f1 / ctrl+e / ctrl+t / ctrl+` / ctrl+q` at 120x40 and 80x24, with a narrower key column. At 40x12 the terminal hint gives way by priority. Pass.
- F1 palette key column: one key per row, right-aligned, fits at 80x24 and 40x12. Pass.
- ctrl+t opens the project search at all three sizes. Pass.
- ctrl+F1 toggles match case with the find bar focused and from the editor while the bar is open. Pass.
- 80x7 focused terminal: the hint reads `ctrl+` hide`, and NUL hides the panel. Pass.

### New minor (non-blocking)
[minor] README.md:168: "wherever the keys are" for ctrl+f1/f2/f3 leaves out the explorer exception (ctrl+f2 = New Folder there), which docs/keymap.md:51 states. Fix: add "(in the explorer ctrl+f2 is New Folder)".

### Files read (r1)
keymap.toml, settings.toml, keymap.rs/settings.rs (via grep and tests), config.rs (new tests), app.rs, app_tests.rs (USER-5, D250 tests), find.rs, terminal.rs, terminal_view.rs, event_loop.rs, docs/keymap.md, README.md (key tables), snapshots: skipped (generated; live walk covers welcome and palette).

### Rule check T18 (r1)
| Rule | Result | Evidence |
|---|---|---|
| BASE-1 | pass | round-1 diff limited to keys, terminal toggle, find toggles, docs, tests |
| BASE-2 | pass | D250 table exact; docs syntax valid (guard test) |
| BASE-3 | pass | reds recorded for 0-row, find toggles and vr-config D250; non-red-first tests are stated |
| BASE-6 | pass | stale comments rewritten; no bug history |
| BASE-25 | pass | notes: fmt --check clean, clippy -D warnings clean |
| BASE-32 | pass | D249-D253 and D250 honoured; D230 way out restored (D252) |
| BASE-35 | pass | USER-5 test complete across contexts; ctrl+i exception recorded (D249) |
| new states | pass | "editor covered + focused terminal" exits via ctrl+`/NUL, tested in both modes and with pass_nul on and off |
