# T3 vr-window input translation + IME state

## Progress
- read brief, spec "Input", plan review focus 3-4, T2 notes, vr-ide chord_from_key / route_key, vr-term encode_key, crossterm 0.29 kitty CSI-u decoding (registry source)
- deps: winit =0.30.13 (2026-03-02) + crossterm; 71 new lock crates all published <= 2026-11-14 (pin-project held 1.1.13, ahash 0.8.12); no gtk/glib under vr-window
- code: input.rs, input/{key,mouse,ime}.rs + tests; 18 input tests green; red shown by 4 mutations on copies (restored, copies removed)
- crate tests 49 passed; clippy -p vr-window clean; guard exit 0 (qa-artifacts/T3/guard.log); capabilities.md updated

## Files changed
- `Cargo.toml` — workspace dep `winit = "=0.30.13"`, `default-features = false`, features `rwh_06, x11, wayland, wayland-dlopen, wayland-csd-adwaita` (= winit's defaults, spelled out)
- `Cargo.lock` — 71 new crate versions (winit and its platform deps); no existing entry changed version
- `crates/vr-window/Cargo.toml` — deps `crossterm`, `winit` (workspace)
- `crates/vr-window/src/lib.rs` — `pub mod input;`, crate doc
- `crates/vr-window/src/input.rs` (new) — module root + re-exports
- `crates/vr-window/src/input/key.rs`, `input/key/tests.rs` (new)
- `crates/vr-window/src/input/mouse.rs`, `input/mouse/tests.rs` (new)
- `crates/vr-window/src/input/ime.rs`, `input/ime/tests.rs` (new)
- `.ai-dev-team/capabilities.md` — three `vr-window` input lines

Scope note: the board lists `crates/vr-window/src/input*.rs`; the module is `src/input.rs` with submodules in `src/input/` (same layout as T2's `backend.rs` + `backend/tests.rs`).

## API for T4
```rust
use vr_window::input::{ImeInput, KeyInput, MouseInput, PreeditText, cell_at_clamped, key_events, translate_key, modifiers};
// state kept by the runner
let mut mods = ModifiersState::empty();      // WindowEvent::ModifiersChanged(m) => mods = m.state()
let mut ime = ImeInput::new();
let mut mouse = MouseInput::new();
// WindowEvent::KeyboardInput { event, .. }
for k in ime.key(&KeyInput::from(&event), mods) { app_events.push(Event::Key(k)); }
if ime.take_cancel_request() { window.set_ime_allowed(false); window.set_ime_allowed(true); } // Esc dropped a composition
// WindowEvent::Ime(ev)
for k in ime.ime(&ev) { app_events.push(Event::Key(k)); }   // commit -> one Press per char
// drawing: preedit at the app's cursor cell
backend.set_preedit(ime.preedit().map(|p| p.at(cursor_col, cursor_row)));
// p.cursor_columns() -> Option<(start_col, end_col)> inside the preedit, if a caret is wanted
// WindowEvent::CursorMoved { position, .. } (physical px)
if let Some(m) = mouse.moved(&backend.layout(), position.x, position.y, mods) { push(Event::Mouse(m)) }
// WindowEvent::MouseInput { state, button, .. }
if let Some(m) = mouse.button(button, state, mods) { push(Event::Mouse(m)) }
// WindowEvent::MouseWheel { delta, .. }
for m in mouse.wheel(&backend.layout(), delta, mods) { push(Event::Mouse(m)) }
// WindowEvent::Focused(false)
mouse.focus_lost();
```
- `translate_key(&KeyInput, ModifiersState) -> Option<KeyEvent>`: one event, `None` for dropped keys and for multi-character text; `key_events` returns all (used by `ImeInput::key`).
- Key events carry kind Press / Repeat (winit `repeat`) / Release; keypad keys carry `KeyEventState::KEYPAD`.
- The app reads them with `chord_from_key(key, enhanced = true)`.

## Translation rules (AC-W4)
Shaped like crossterm 0.29's decoding of the kitty flags the TUI pushes (`DISAMBIGUATE_ESCAPE_CODES | REPORT_ALTERNATE_KEYS | REPORT_EVENT_TYPES`, read in `crossterm-0.29.0/src/event/sys/unix/parse.rs` `parse_csi_u_encoded_key_code` and `char_code_to_event`):
- Text keys without ctrl/alt/super: the typed text (`text`, else the logical key), `SHIFT` only on uppercase letters — `shift+/` = `Char('?')` no modifiers, `shift+a` = `Char('A')` + shift (crossterm's legacy text decoding).
- ctrl/alt/super chords: the logical key when it is an ASCII letter/symbol, else the US name of the physical key (ctrl+ф → ctrl+a, ctrl+ς → ctrl+w, ctrl+é on AZERTY → ctrl+2, macOS option+a = å → alt+a). Letters lowercase unless shift (caps lock never adds shift). ctrl+[ = `Char('[')`+ctrl (Esc is `Esc`), ctrl+h = `Char('h')` (Backspace is `Backspace`), ctrl+i = `Char('i')` (Tab is `Tab`), ctrl+/ = `Char('/')`, ctrl+shift+/ = `Char('?')`+ctrl+shift, ctrl+shift+a = `Char('A')`+ctrl+shift.
- Named keys keep all modifiers; shift+Tab = `BackTab` + shift; Space is `Char(' ')`; F1–F35, Menu, PrintScreen, Pause, keypad 5 without num lock = `KeypadBegin`.
- Dropped (`None`): modifier-only keys, lock keys, dead keys without a chord, unknown keys. The kitty flags the TUI uses do not report modifier keys either, and the app counts every key event as input (`route_key` clears a pending read-mode space), so `KeyCode::Modifier` would change behaviour.

## IME (AC-W5)
- `Preedit(text, cursor)` → held as `PreeditText` (never sent); empty text clears it. `Commit(text)` → clears the preedit, one `Press` per character (`\n`/`\r` → Enter, `\t` → Tab, other control characters dropped); no Release, since `App::route_key` returns on Release anyway (`crates/vr-ide/src/app.rs:1308`).
- While a preedit exists every key event is held back (winit on X11 already filters most; Wayland / other IMEs may still deliver some). Esc pressed then drops the preedit and sets a one-shot cancel request so the runner resets the IME; the Esc release that follows goes out as a Release, which the app ignores.
- `Disabled` clears the preedit; with no preedit (IME idle or disabled) keys flow as usual.

## Tests added (crates/vr-window/src/input)
- `key::tests::key_table` (62 rows: plain/shifted text, ambiguous ctrl chords, ctrl+shift letters and symbols, alt/super, Cyrillic/Greek/AZERTY/macOS-option chords, named keys with modifiers, F-keys, dropped keys), `keypad_keys_carry_the_keypad_state` (keypad Enter, digit, begin, arrow, ctrl+keypad +), `repeat_and_release_are_reported_with_the_same_key`, `text_of_several_characters_is_one_event_each`, `every_function_key_has_its_number`, `modifiers_map_one_to_one`
- `mouse::tests::pixels_map_to_cells_clamped_to_the_grid` (padding edge, outside window, inf/NaN), `a_degenerate_layout_never_panics`, `motion_reports_only_cell_changes_and_drag_while_a_button_is_held`, `wheel_lines_follow_crossterm_directions`, `small_pixel_deltas_accumulate_into_lines`, `bogus_wheel_deltas_are_bounded`
- `ime::tests::telex_viet_composes_then_commits_once`, `esc_cancels_the_preedit_without_sending_anything`, `an_ime_that_clears_the_preedit_itself_cancels_too`, `commit_without_preedit_and_of_several_characters`, `keys_flow_when_the_ime_is_disabled_or_idle`, `preedit_cursor_is_in_display_columns`

## Commands run
```
$ cargo test -p vr-window input
test result: ok. 18 passed; 0 failed; 0 ignored; 0 measured; 31 filtered out; finished in 0.00s
$ cargo test -p vr-window
test result: ok. 49 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.74s
$ cargo clippy -p vr-window --all-targets -- -D warnings
    Checking vr-window v0.1.0 (/home/rainy/DEV/vr-ide/crates/vr-window)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 24.43s
$ rustfmt --edition 2024 crates/vr-window/src/input.rs crates/vr-window/src/input/{key,mouse,ime}.rs crates/vr-window/src/input/{key,mouse,ime}/tests.rs crates/vr-window/src/lib.rs
$ $TEAM_BIN/run-heavy.sh bash -c 'cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings'   (qa-artifacts/T3/guard.log)
warning: unused workspace dependency `vr-window`   (manifest warning from T2, until T4)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.07s
exit=0
```
Red shown by mutation (copies of key.rs / ime.rs / mouse.rs in qa-artifacts/T3, restored with `cp`, `cmp` equal, copies removed; final run 18 passed):
- chord always uses the logical key → `key_table` FAILED
- no letter case on chords → `key_table` FAILED
- keys not held back while composing → `telex_viet_…`, `esc_cancels_…`, `keys_flow_…` FAILED
- wheel remainder kept when the direction turns → `small_pixel_deltas_accumulate_into_lines` FAILED

## Crate versions (D74: published ≥ 14 days before 2026-11-28, i.e. ≤ 2026-11-14; dates = crates.io index `pubtime`)
- winit 0.30.13 — 2026-03-02 (0.30.x latest)
- crossterm 0.29.0 — already in the workspace
- All 71 new lock entries listed with dates in `qa-artifacts/T3/newcrates-dates.txt` (made by `qa-artifacts/T3/pubtime.sh` from the official sparse index); newest is web-sys 0.3.107 / wasm-bindgen-futures 0.4.80 (2026-10-30). Held back: `pin-project` 1.1.13 (2026-05-13; 1.2.0 is 2026-11-19), `ahash` 0.8.12 (2025-05-08; 0.8.14 is 2026-11-25) via `cargo update --precise`.
- Features: winit's default set spelled out; `cargo tree -p vr-window -e normal | grep -ci "gtk|glib"` = 0. `wayland-csd-adwaita` (title bar drawn by sctk-adwaita with ab_glyph) kept for T4's CSD on Wayland.

## Deviations from the plan
- Shift on ctrl/alt chords with symbols: crossterm's kitty decoding replaces the key by the shifted symbol *and drops SHIFT* (ctrl+shift+1 → `Char('!')` + ctrl, which `chord_from_key` names "ctrl+!"). The window keeps SHIFT (`Char('!')` + ctrl + shift → "ctrl+shift+1"), matching the app's own test `shifted_keys_use_the_unshifted_name` and the keymap names. For letters both give "ctrl+shift+a".
- AC-W4 "Latin letter" read as ASCII letter: non-ASCII Latin letters on a chord (macOS option+a = å, AZERTY é) go to the physical key, because keymaps only name ASCII keys.
- `translate_key` returns `Option<KeyEvent>` as asked; `key_events` returns a `Vec` for keys that type several characters (Windows dead key that did not combine).
- Esc-cancel adds `take_cancel_request()` for T4 to reset the IME (`set_ime_allowed(false/true)`).
- Tests were written together with the code and passed on the first run; red was shown by the four mutations above.

## Out-of-scope issues noticed
- Windows AltGr arrives as ctrl+alt; a character typed with AltGr (German `@`) would come through as a ctrl+alt chord. Linux/macOS are unaffected (AltGr is not in winit's `ModifiersState` there). For T4/T6 to check on Windows.

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| key → app chord rules | `crates/vr-ide/src/event_loop.rs` `chord_from_key` | consumer only (vr-window must not depend on vr-ide); output shaped for it |
| pixel → cell | T2 `GridLayout::cell_at` (`backend.rs:78`) | not reused: returns `None` in padding/outside; clamping needed for drag, new `cell_at_clamped` over the same `GridLayout` fields |
| preedit drawing type | T2 `Preedit` | reused: `PreeditText::at(col,row) -> Preedit` |
| key/mouse types | capabilities.md, crossterm 0.29 | reused crossterm `KeyEvent` / `MouseEvent` (what the app reads) |
| width of text | workspace `unicode-width` | reused for preedit cursor columns |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | `git status`: Cargo.toml, Cargo.lock, crates/vr-window/{Cargo.toml,src/lib.rs,src/input*}, capabilities.md, notes; nothing in vr-ide/vr-config/vr-term; no event loop/window created |
| BASE-2 | pass | crossterm 0.29 types as the app reads them; winit 0.30.13 types; API above |
| BASE-3 | pass | tests written with the code; red by 4 mutations, each failing named tests (Commands run) |
| BASE-4 | pass | trimmed outputs above; qa-artifacts/T3/guard.log |
| BASE-5 | pass | guard via run-heavy.sh; nothing left running |
| BASE-6 | pass | English; comments say why (e.g. key.rs module doc, ime.rs `Commit` presses-only) |
| BASE-7 | pass | no stage/commit |
| BASE-8 | pass | winit 0.30.13 (2026-03-02); 71 new crates ≤ 2026-11-14 (newcrates-dates.txt); pin-project/ahash held back |
| BASE-9 | pass | boundary input bounded: wheel steps capped (`MAX_STEPS`, `bogus_wheel_deltas_are_bounded`), NaN/inf pixel positions clamped, preedit cursor offsets clamped to char boundaries (`preedit_cursor_is_in_display_columns`) |
| BASE-10 | pass | only `cargo test -p vr-window`, clippy -p vr-window, guard once |
| BASE-11 | pass | no checkout/restore/stash/switch |
| BASE-13 | pass | removed only my own scratch files in qa-artifacts/T3 (lock copy, mutation copies, list temp files) |
| BASE-14 | pass | no polling; guard finished in the foreground (1 s, incremental) |
| BASE-15 | pass | no process stopped |
| BASE-16 | pass | Reuse table; capabilities.md lines added |
| BASE-17 | pass | nothing installed; crates via cargo only |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | notes created before the first change; Progress section |
| BASE-21 | pass | no secrets in output |
| BASE-22 | pass | module/test layout as T2 (`x.rs` + `x/tests.rs`), `=` pin in workspace deps, `*.workspace = true` |
| BASE-23 | pass | each entry path tested: `translate_key` and `key_events` (key tests), `ImeInput::key` / `ImeInput::ime` (ime tests), `MouseInput::{moved,button,wheel,focus_lost}`, `cell_at_clamped` |
| BASE-24 | pass | no git index/history command |
| BASE-25 | pass | guard exactly as written, exit 0 (guard.log) |
| BASE-32 | pass | D3: no App knowledge, no vr-ide dependency in vr-window |
| BASE-34 | pass | publish dates from the official crates.io sparse index, default curl user agent |
| BASE-38 | pass | rustfmt only on the changed files |
| BASE-49 | pass | winit/crossterm behaviour read from ~/.cargo/registry sources (crossterm parse.rs, winit event.rs, keyboard.rs, x11 event_processor.rs); no program run for usage |
| BASE-50 | n-a | verifier rule; this is an implementation task |
| BASE-51 | pass | Cargo.lock and the three source files copied to qa-artifacts/T3 before experiments; mutations restored from the copies (`cmp` equal), copies removed; final 18 passed |
| COST-1 | pass | guard via run-heavy.sh |
| COST-3 | pass | no sleep/pgrep |
| COST-8 | pass | new files written whole; edits batched; no python heredoc edits |
| COST-9 | pass | nothing left running |
| COST-10 | pass | related set = vr-window unit tests; `grep -rl "vr_window::input|input::key|input::ime|input::mouse" crates` finds nothing outside crates/vr-window/src/input |
| COST-13 | pass | scratch only under qa-artifacts/T3; nothing in $TEAM_TOOLS |
| COST-16 | pass | guard output to a log, tail read |
| AC-W4 | pass | `key_table` rows for ctrl+[ / Esc, ctrl+h / Backspace, ctrl+i / Tab, ctrl+/ vs ctrl+shift+/, ctrl+shift+a, alt+letter, `?` with shift, ctrl+ф/ctrl+ς → US physical, F-keys; keypad Enter in `keypad_keys_carry_the_keypad_state`; Press/Repeat/Release in `repeat_and_release_are_reported_with_the_same_key` |
| AC-W5 | pass | `telex_viet_composes_then_commits_once` (preedit updates, raw keys held back, commit "việt" once as 4 presses), `esc_cancels_the_preedit_without_sending_anything`, `commit_without_preedit_and_of_several_characters`, `keys_flow_when_the_ime_is_disabled_or_idle`; preedit → T2 `Preedit` via `PreeditText::at` |

n-a: BASE-12, BASE-29, BASE-30, BASE-31, BASE-33, BASE-40, BASE-41, BASE-48 (no other repo, system, container, browser or outward write) · BASE-20 (nothing removed) · BASE-26 (not a rewrite) · BASE-27, BASE-44 (no shared data, no fakes) · BASE-28 (no third-party code changed) · BASE-35 (no USER-n lines for this area) · BASE-36 (no app scope change) · BASE-37, BASE-47 (not a defect fix) · BASE-39 (no schema) · BASE-42 (no test settings changed) · BASE-43 (no access rules) · BASE-45 (AC-W4/W5 used as written; the two readings are listed under Deviations) · BASE-46 (no runtime qc walk; pure functions) · COST-2, COST-4..7, COST-11, COST-12, COST-14, COST-15, COST-17..21 (no browser, e2e, audit, CI, goldens or qc role)
