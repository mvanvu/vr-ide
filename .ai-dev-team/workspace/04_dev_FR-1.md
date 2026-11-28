# FR-1 Phase 5 fixes: signals, font size fallback, super chords + minors
Base: 72c16b9 (master HEAD at start).

## Progress
- read findings, window_frontend.rs, app.rs, backend.rs, font.rs, key.rs, ime.rs, desktop.rs, tests
- C1: red seen (sigint/sighup tests: child killed by signal 2/1 in the window), fixed in window_frontend.rs start(); 3 signal tests green
- C2: red seen (edit mode text 'c\nxzab\n'; terminal forwarded), fixed in app.rs route_key (single key entry point); green
- U2: red seen ((12.0,1.2) vs (16.0,1.5)), fixed font.rs with_font_system + default 11 pt; green
- U4/U5/U6/C3: tests added; red shown by mutation of a copy (fill_leftover off, caret=app cursor, no hollow, focus_lost=false): 4 FAILED (left 1056816 vs right 16711680 etc.), originals copied back, 65 passed
- U3/U7/C4: tests added (compile-red: API absent before), implemented
- docs: README signals/keys/line_height, settings.toml + settings.rs line_height wording
- related tests, both clippy variants, guard, release build, X11 smoke run: all green (below)

## Per finding
| Finding | Fix | Test (red → green) |
|---|---|---|
| C1/U1 high signals | `crates/vr-ide/src/window_frontend.rs` `WindowApp::start`: SIGINT joins SIGTERM (same flag → `request_quit`, conditional default switched on by `WindowRun.sigterm` after recovery). SIGHUP is **ignored while the window is up** (handler whose flag is never read) + same conditional default after recovery. Why ignore rather than quit: the window never needed the launching terminal; on the quit path a clean session would close the editor whenever the user closes the leftover terminal of an old menu entry or the shell a `&` job came from. A handler, not `SIG_IGN`, so embedded-terminal shells do not inherit an ignored SIGHUP (the exit's hangup must still end them). Registration is now opt-in (`catch_signals()`, called by `run()`), so the unit-test process keeps ctrl+c. | `window_frontend_tests::{sigint_takes_the_quit_path_and_ends_only_after_recovery, sighup_is_ignored_by_the_window_and_ends_only_after_recovery, sigterm_ends_the_process_once_recovery_is_written_after_the_window}` (child process per signal: in-window effect, survives before recovery, dies by the signal after the switch). Red: `signal 1 in the window: ... ExitStatus(unix_wait_status(1))`, same for 2 → green. |
| U2 major font size at start | `crates/vr-window/src/font.rs` `with_font_system`: `FamilyNotFound` → same config with `family: None` (size/line height kept); full default only for invalid size/line height/scale; `FontConfig::default().size_pt` 12 → 11 (settings' default). | `backend::tests::a_missing_family_at_start_keeps_size_and_line_height`. Red: `left: (12.0, 1.2) right: (16.0, 1.5)` → green. Metric tests that measured the old default now pin `TWELVE_PT`. |
| C2 major super chords | `crates/vr-ide/src/app.rs` `route_key`: key events with SUPER or HYPER are dropped before any routing. `route_key` is the only key entry (`event_loop::handle_event` → `handle_key`), so editor, read mode, dialogs, palette, explorer menu and the embedded terminal are all covered; also covers a kitty terminal reporting super. | `app_tests::super_chords_never_type_or_run_the_plain_key` (edit mode, read mode, goto-line prompt, palette) and `terminal_tests::super_chords_are_not_forwarded_to_the_terminal` (real /bin/sh). Red: `left: "c\nxzab\n" right: "ab\n"` and terminal assert failed → green. |
| U3 title bar | `crates/vr-window/src/app.rs`: `WindowOptions.dark` → `with_theme(Some(window_theme(dark)))`; new `WindowOps::set_theme(dark)`; vr-ide `window_dark(theme)` (light → Light, else Dark), `follow_settings` calls `ops.set_theme` on a theme change. | `app::tests::the_title_bar_theme_follows_the_app_theme`, `window_frontend_tests::the_palette_follows_the_theme` (window_dark), `a_settings_reload_relays_the_grid_and_warns_for_a_missing_family` (themes `[false]` then `[false, true]`). Red: compile (API absent). |
| U4 leftover strip | `crates/vr-window/src/backend.rs` `fill_leftover`: last column's cells fill to the right edge, last row's to the bottom edge (corner included), background before the cursor swap; damage covers the strips (last row's damage height to the canvas bottom). Configured padding stays background (no caller sets padding). Spec line "padding in the background colour" (01_spec) is superseded for the leftover pixels, as the brief says; spec not edited. | `backend::tests::leftover_pixels_take_the_edge_cells_background` (301x203: corner = status-bar RED, right of last col = GREEN, untouched row = BG, damage contains the strips, block cursor on the last cell leaves the strip RED). Red by mutation of a copy (`fill_leftover` call commented): `left: 1056816 right: 16711680`. |
| U5 IME caret | `Preedit.caret: Option<u16>` (columns from the preedit start); `PreeditText::at` fills it from `cursor_columns()` end; `render_overlays` draws the cursor at `col + caret` while composing, none when `caret` is `None`. | `backend::tests::the_caret_follows_the_ime_cursor_inside_the_preedit`, `ime::tests::the_placed_preedit_carries_the_caret_column`. Red by mutation (caret = app cursor): `left: 1056816 right: 255`. |
| U6 hollow cursor | `CursorShape::HollowBlock` (outline of `line` px in the cursor colour); `app.rs` `drawn_shape(shape, focused)`; `overlays` uses it, so a focus change changes the overlay state and repaints. | `app::tests::an_unfocused_window_draws_the_block_cursor_hollow`, `backend::tests::a_hollow_block_cursor_is_an_outline`. Red by mutation (no mapping): `left: Block right: HollowBlock`. |
| U7 notice wording | `font_notice(error, applied)`: `settings: font_family "X" is not installed; using the system monospace font at 16 pt` for start and reload; `desktop::MenuNotice::{Info, Warning}` returned by `migrate_on_window_start` (texts: `updated the vr-ide menu entry to open its own window`, `could not update the vr-ide menu entry: …`); no prefix matching. | `window_frontend_tests::the_start_font_notice_names_the_setting_and_the_size_used`, `font_settings_feed_the_font_set_and_a_missing_family_warns` (exact text), `desktop_tests::migration_*` (enum). Red: compile. |
| U8 line_height docs | README table, `crates/vr-config/defaults/settings.toml`, `crates/vr-config/src/settings.rs` doc: multiple of the font size. | doc only (no test: text). |
| C3 IME focus loss | `ImeInput::focus_lost()` drops the preedit; `app.rs` on `Focused(false)` resets the IME (allow off/on + placement reset) like the Esc path when one was dropped. | `ime::tests::focus_loss_drops_the_preedit_and_asks_for_a_reset`. Red by mutation (`focus_lost` = false): failed at ime/tests.rs:193. |
| C4 install without display | `desktop::display_note(window, display)`; `run_install` adds `note: no display is reachable here; the entry opens vr-ide's window only in a desktop session (X11 or Wayland) with the window libraries installed`. | `desktop_tests::install_without_a_display_notes_that_the_entry_needs_a_desktop_session`. Red: compile. |

README "Window and terminal": SIGINT (ctrl+c in the starting shell) takes the quit path; closing that terminal (SIGHUP) leaves the window open; super chords are ignored.

## Files changed
crates/vr-ide/src/{app.rs, app_tests.rs, desktop.rs, desktop_tests.rs, terminal_tests.rs, window_frontend.rs, window_frontend_tests.rs}, crates/vr-window/src/{app.rs, app/tests.rs, backend.rs, backend/tests.rs, font.rs, input/ime.rs, input/ime/tests.rs}, crates/vr-config/{src/settings.rs, defaults/settings.toml}, README.md.

## Deviations
- C2 fixed once in `crates/vr-ide/src/app.rs` (`route_key`, the single key entry) instead of edit.rs / dialog.rs / terminal.rs / input/key.rs: one guard covers every consumer, including ones the finding did not list (explorer menu, palette) and kitty-terminal input. `input/key.rs` still reports SUPER faithfully.
- U4: configured padding (`set_padding`, unused by vr-ide) keeps the background; only the pixels beyond the last whole cell take the edge colours.
- `FontConfig::default()` is now 11 pt; vr-window metric tests pin a `TWELVE_PT` config so their measured numbers stay meaningful.

## Commands run (trimmed)
Related (log: qa-artifacts/FR-1/related.log), via run-heavy.sh:
```
cargo test -p vr-window                         test result: ok. 65 passed; 0 failed
cargo test -p vr-config                         ok. 17 passed / ok. 29 passed / ok. 8 passed
cargo test -p vr-ide --lib app_tests::          ok. 35 passed
cargo test -p vr-ide --lib terminal_tests::     ok. 57 passed
cargo test -p vr-ide --lib window_frontend_tests:: ok. 16 passed
cargo test -p vr-ide --lib desktop_tests::      ok. 23 passed
cargo test -p vr-ide --lib dialog_tests::       ok. 26 passed
cargo test -p vr-ide --lib mode_tests::         ok. 34 passed
cargo test -p vr-ide --test desktop_cli         ok. 3 passed
cargo test -p vr-ide --lib the_start_font_notice (after clippy fix)  ok. 1 passed
```
Clippy: `cargo clippy -p vr-ide --all-targets -- -D warnings` → first run: 1 error `field_reassign_with_default` (my new test), fixed → `Finished`; `--no-default-features` → `Finished` (no warnings).
rustfmt --edition 2024 on the changed .rs files only.
Guard (exactly): `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings` → no fmt output, `Finished dev profile`, exit=0.
Release: `run-heavy.sh cargo build --release -p vr-ide` → `Finished release profile [optimized] target(s) in 1m 50s`.

## Smoke run (X11 = Xwayland :0, WAYLAND_DISPLAY unset, release binary built after the last code change)
Driver `qa-artifacts/FR-1/smoke.sh` (+ drive.py / xin.py from QC T4); temp HOME/XDG/VR_IDE_CONFIG_DIR/VR_IDE_STATE_DIR under `qa-artifacts/FR-1/w/`; settings: missing family + font_size 16; keys sent only to the window whose `_NET_WM_PID` is our pid (the user's own vr-ide left alone).
```
first frame: exit=0                      (VR_IDE_EXIT_AFTER_FIRST_FRAME=1)
vr pid=3874108 window=0xc00002
after SIGHUP alive: yes                  (dirty buffer "EDITbase", no dialog: s2_after_hup.png)
after SIGINT alive: yes                  (Quit dialog: s3_after_int.png)
after Discard alive: no
vr exit=0
recovery files: 1
recovery content: EDITbase|
disk a.txt: base|
stderr: vr-ide: unsaved changes were kept in the recovery folder: .../FR-1/w/state/recovery/20261128T012729Z-vr-ide+..._a.txt
```
Screenshots show the 16 pt font kept at start (U2) and the notice `settings: font_family "No Such Font 42" is not installed; using the system monospace font at 16 pt` (U7); the status bar reaches the bottom edge (U4).
Not replayed live: super chords (the XTEST driver has no super mapping and super+letter on the user's GNOME session can fire shell shortcuts), IME caret / focus loss (no IME driver), title bar variant (Xwayland windows get mutter's server-side frame) — covered by the unit tests above.
Cleanup: the smoke script's watchdog left an orphan `sleep 60` (pid 3874111, child of my script); stopped by its pid; `pgrep -a vr-ide` afterwards shows only the user's own 1696342.

## New states (round 1)
- SIGHUP flag (never read): quit/cancel n-a; after the window and before recovery the signal stays caught (no data loss); after the switch it ends the process (tested). Repeated SIGHUP: no effect (tested once per child; handler idempotent).
- SIGINT while the Quit dialog is open: `request_quit` cancels the open dialog and asks again (existing path, same as SIGTERM; `closing_the_window_or_sigterm_takes_the_ctrl_q_path`).
- Hollow cursor / caret: drawing state only, recomputed every frame from focus and preedit; no data path.
- IME reset on focus loss: same calls as the Esc path; a dropped preedit is never committed later.

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| Signal → quit path, default after recovery | window_frontend.rs (SIGTERM, `SigtermDefault`, `exit_with`) | reused for SIGINT/SIGHUP |
| Child-process signal test | existing SIGTERM test | generalised into `signal_child` / `assert_signal_path` |
| IME reset | app.rs Esc path (`take_cancel_request`) | same calls on focus loss |
| Display detection for C4 | `cli::display_available` | reused |
| X11 smoke driver | qa-artifacts/T4/qc, signoff/xin.py | copied xin.py, new pid-scoped drive.py |
Nothing new for `capabilities.md` (helpers are local to the window frontend).

## Out of scope noticed
- `docs/keymap.md` has no line saying super chords are ignored (README now says it); not edited (not in the task row).

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | diff = task row files + app.rs (C2, see Deviations); nothing unrelated |
| BASE-2 | pass | no plan contract changed; `WindowOps` gains `set_theme` (U3 asked for the hook) |
| BASE-3 | pass | per-finding table: C1, C2, U2 behavioural red; U4, U5, U6, C3 red by mutation of copies; U3, U7, C4 compile-red; U8 doc-only |
| BASE-4 | pass | "Commands run" above |
| BASE-5 | pass | related tests, clippy, release through run-heavy.sh; nothing left running |
| BASE-6 | pass | English, why-comments (window_frontend.rs start, backend.rs fill_leftover, app.rs route_key) |
| BASE-7 | pass | no commit, nothing staged |
| BASE-9 | pass | key input dropped at the boundary; signals only set flags; no secrets |
| BASE-10 | pass | only related modules + guard; no full suite |
| BASE-11 | pass | no stash/checkout/restore; experiments via copies in scratchpad (BASE-51) |
| BASE-14 | pass | all runs foreground within limits (run-heavy timeouts); no wait loops |
| BASE-15 | pass | stopped only my own orphan `sleep 60` (pid 3874111, child of my smoke script) by pid; no pkill/killall; user's vr-ide untouched |
| BASE-16 | pass | Reuse table |
| BASE-17 | pass | nothing installed |
| BASE-18 | pass | no sub-agents |
| BASE-19 | pass | Progress section from the start |
| BASE-22 | pass | new names follow neighbours (`*_tests.rs`, `font_notice`, `MenuNotice`, `display_note`) |
| BASE-23 | pass | super chords: editor, read mode, dialog, palette, terminal; font notice: start and reload; signals: TERM/INT/HUP |
| BASE-24 | pass | no index/history command |
| BASE-25 | pass | guard exactly as written, exit 0 |
| BASE-32 | pass | D3/D5/D6 untouched (window/TUI choice, entry form, font settings kept) |
| BASE-37 | pass | SIGINT/SIGHUP and U2/U7 replayed on the release build (smoke.result, s2/s3 png); super/IME/title bar not replayable here (reason above) |
| BASE-38 | pass | rustfmt only on changed .rs files |
| BASE-46 | pass | smoke ran on the release build made after the last code change |
| BASE-48 | pass | smoke states quoted from smoke.result and pgrep output |
| BASE-49 | pass | binaries' usage from source/QC drivers, no `--help` probing |
| BASE-51 | pass | mutation experiments on copies (scratchpad backend.rs.new / ime.rs.new / app.rs.new), copied back, 65 passed after |
| COST-1 | pass | heavy runs via run-heavy.sh |
| COST-3 | pass | no polling loops (smoke uses fixed sleeps inside the driver script only) |
| COST-4 | pass | no full-suite run |
| COST-5 | pass | smoke vr-ide exited (exit=0); orphan sleep stopped |
| COST-8 | pass | batched edits; perl -pi for bulk edits, Edit tool otherwise; no python heredoc edits |
| COST-9 | pass | nothing left running |
| COST-10 | pass | related set includes tests naming changed files (desktop_tests, window_frontend_tests, desktop_cli) |
| COST-13 | pass | scripts under qa-artifacts/FR-1/; nothing in $TEAM_TOOLS |
| COST-16 | pass | related run output in qa-artifacts/FR-1/related.log, only summaries in context |
n-a: BASE-8 (no dependency change) · BASE-12, BASE-28, BASE-29, BASE-40, BASE-41 (no outside repo/system) · BASE-13 (no untracked file deleted) · BASE-20 (nothing removed) · BASE-21 (no secrets in output) · BASE-26 (no rewrite) · BASE-27, BASE-30, BASE-31 (no shared data/containers) · BASE-33 (no external system state) · BASE-34 (no blocked step) · BASE-35 (no user-rules lines for the window) · BASE-36 (single app) · BASE-39 (no schema) · BASE-42 (no test settings changed) · BASE-43 (no access rule) · BASE-44 (no fake adapter) · BASE-45 (no AC text changed) · BASE-47 (no computed value fixed) · BASE-50 (no text for it in rule-packs.md 1.0.45 §3; leader to supply if it applies) · COST-2, COST-6, COST-7, COST-11, COST-12, COST-14, COST-15, COST-17..20 (no audit/browser/e2e/CI/goldens/qc role)

## Quick pass
Finding (ui minor): with SIGHUP ignored the window outlives its terminal; at exit `eprintln!` panics on EIO from the hung-up tty, skipping `commands::search::shutdown_within` and exiting 101.
- Fix: `crates/vr-ide/src/recovery.rs` new `note(format_args!(..))` = `let _ = writeln!(stderr, "vr-ide: …")` (a failed write is dropped). Used for every stderr line of the exit paths: `event_loop::exit_with` (deferred messages, the replace-wait line, exit messages), the panic hook in `recovery::install_hook_for`, and `window_frontend::write_recovery_on_display_loss` (runs inside libX11's handler). Message text unchanged.
- Test: `event_loop::tests::exit_finishes_when_stderr_is_gone` — child process whose stderr is a pipe with the read end closed before it writes (EPIPE, same failure class as EIO); a stuck replace job and an exit message force the three exit prints; asserts the sequence reaches its end with the replace wait done and exit status success. Red: child exited `unix_wait_status(25856)` (= 101, panic) without the marker → green.
- Related (qa-artifacts/FR-1/quick.log): `event_loop::` 17 passed, `recovery` 19 passed, `window_frontend_tests::` 16 passed.
- `cargo clippy -p vr-ide --all-targets -- -D warnings` → Finished. Guard exactly (`cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`) → exit 0 (qa-artifacts/FR-1/guard.log).
- rustfmt on recovery.rs, event_loop.rs, window_frontend.rs only. `docs/keymap.md` (leader's line) untouched. Nothing left running.
