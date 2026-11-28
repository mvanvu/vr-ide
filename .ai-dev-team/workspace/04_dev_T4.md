# T4 Window app: runner, wiring, launch, quit path
Worktree: /home/rainy/DEV/vr-ide/.claude/worktrees/agent-ac23ba261343c7bd6, branch worktree-agent-ac23ba261343c7bd6 (fast-forwarded from 5562f28 to the brief's base f1279ad: the worktree was created at the old master).

## Progress
- read: brief, spec, plan, notes T1/T2/T3/T4a/T5, vr-window API, vr-ide event_loop/frontend/main
- deps: softbuffer 0.4.8 (pubtime 2025-12-13) chosen; signal-hook 0.3.18 already in Cargo.lock
- code: vr-window app.rs runner (+tests), vr-ide window_frontend.rs (+tests), cli --tui, main wiring, clipboard WindowClipboard, theme use_true_color, cell_px → fit_term/resize_with_pixels, vr-term cell_pixels, attention new_mark fix, desktop launches_in_window
- related tests (first run green; red shown by 2 mutations on copies, restored, `cmp` equal); real window runs on Wayland and X11; TUI fallbacks; verify.log; guard green; release build green
- fix: `TermTab::pty_size` gated to `all(test, feature = "window")` (no-default clippy found it unused); no-default clippy + guard rerun green

## Files changed
- `Cargo.toml` — workspace deps `softbuffer = "=0.4.8"` (default-features off; x11, x11-dlopen, wayland, wayland-dlopen; no kms/DRM), `signal-hook = "=0.3.18"` (already in the lock via crossterm)
- `Cargo.lock` — new: softbuffer 0.4.8, tiny-xlib 0.2.5, ctor 0.10.1, dtor 0.8.1 (no other entry changed version)
- `crates/vr-window/Cargo.toml` — `softbuffer`
- `crates/vr-window/src/app.rs` (new) + `app/tests.rs` (new) — the winit runner
- `crates/vr-window/src/lib.rs` — `pub mod app`
- `crates/vr-window/src/backend.rs` — one getter `cursor_position()` (deviation: outside the board's `app*.rs`; `DrawnCursor` needs the cursor through `&self`)
- `crates/vr-ide/Cargo.toml` — feature `window = ["dep:vr-window", "dep:signal-hook"]`, default `["login-window", "window"]`; signal-hook as a unix target dep
- `crates/vr-ide/src/window_frontend.rs` (new), `window_frontend_tests.rs` (new; vr-ide's `*_tests.rs` convention), `lib.rs` (module + tests behind the feature)
- `crates/vr-ide/src/main.rs` — `--tui` taken out, launch choice, window run with TUI fallback (stderr line), `IN_TERMINAL` so the panic hook / exit restore only a terminal the TUI took
- `crates/vr-ide/src/cli.rs` — `TUI_FLAG`, `take_tui_flag`, `Ui`, `choose_ui`, `display_available`, usage text `[--tui]`, module doc
- `crates/vr-ide/src/desktop.rs` — `launches_in_window()` → `cfg!(feature = "window")` (the one function)
- `crates/vr-ide/src/event_loop.rs` — `attention_marks` returns (serial, order) pairs + `new_mark` (T1 review minor), one test; lines 15-110 (`chord_from_key`, `unshift_us`) untouched
- Outside the board's list (small, each needed by an AC): `clipboard.rs` (`WindowClipboard`: arboard, no OSC 52 — AC-W6), `theme.rs` (`ColorDepth::use_true_color` — a window started without COLORTERM would draw the 256-colour theme), `app.rs` (`cell_px` field), `commands/terminal.rs` (`fit_term`, `TermTab::resize(.., cell_px)`, test accessor `pty_size`), `ai/mod.rs` (one call), `crates/vr-term/src/terminal.rs` (`cell_pixels()` getter + field, so a new terminal gets its pixel size on the next fit; the board row in 02_tasks.md lists vr-term terminal.rs)
- `.ai-dev-team/capabilities.md` — three lines under `## vr-window`

## How it works
- Launch: helpers (`--ai-statusline`, `--ai-codex-notify`, `--login-window`, launcher name, `--install-desktop`/`--uninstall-desktop`) run first, unchanged. Then `--tui` is removed from the args; `choose_ui(tui, cfg!(feature="window"), display_available(env))`. Window: true colour whatever COLORTERM says, `WindowClipboard`, no terminal setup; `window_frontend::run(&mut app)`; `Err(StartError)` → `vr-ide: <error>; running in the terminal instead` on stderr, system clipboard, terminal entered, TUI as before.
- Runner (vr-window): `EventLoop::new` → `resumed` creates the window (title `app.title()`, `with_name("vr-ide","vr-ide")` = Wayland app id / X11 WM_CLASS, inner size 120x36 cells of the font at scale 1, IME allowed, I-beam pointer), softbuffer surface, `PixelBackend` at the window's scale. Window events → crossterm events (T3 input module; synthetic X11 key presses dropped; Esc cancel → `set_ime_allowed(false/true)`); Resized / ScaleFactorChanged → `PixelBackend::resize(w,h,scale)` + `Event::Resize`. `about_to_wait`: one `WindowHandler::step` per batch, then the IME area (cell → pixels in the current layout), cursor shape/blink, preedit at the backend cursor, `render_overlays`, present: full copy when the buffer age is unknown / after a resize / on `RedrawRequested`, else only the damaged rects (age 1: this frame's, age 2: + last frame's) with `present_with_damage`; then `ControlFlow::WaitUntil(min(next_wake, next blink))`.
- vr-ide `WindowApp::step`: the first step reports the keyboard as enhanced (no "cannot send" notice), registers SIGTERM (signal-hook flag; unregistered on drop so a TUI fallback keeps the default), shows the font warning and the menu migration notice (`desktop::migrate_on_window_start(data_home, current_exe)`); close button / SIGTERM flag → `request_quit` (open dialog answered `Cancelled`, then the `app.quit` command = ctrl+q: unsaved files, running terminals/agents, running replace); the events of that batch are dropped when the quit needed no question; settings follow (font triple → `apply_fonts`, missing family → default family at the asked size + warning, `on_resize`; theme → `set_palette`); `app.cell_px` = backend cell; `event_loop::step` with `WindowFrontend`; after the first frame the index/watch/git start as in the TUI (`VR_IDE_EXIT_AFTER_FIRST_FRAME` quits there instead). Exit: `event_loop::exit` with a no-op terminal restore (the window is already closed when `run` returns).
- PTY pixels: `fit_term(t, cols, rows, app.cell_px)` resizes when the grid or the cell size changed → `resize_with_pixels` (TUI: 0x0 as before).
- `EventLoopProxy`: not used — the app has no wake hook for background threads; `next_wake` (16 ms with a running visible terminal, else 50 ms) drives the background pass, as in the TUI.
- Windows AltGr: no TODO placed — T3's key.rs is not in this task's files (tracked by the leader, run log 06:47).

## Tests added
- vr-window `app::tests::` only_damage_is_copied_when_the_buffer_holds_a_recent_frame, the_first_window_size_holds_the_grid_at_scale_one, the_ime_area_is_the_cursor_cell_in_pixels, a_blinking_cursor_flips_and_input_shows_it_again
- vr-ide `cli::tests::launch_choice_follows_the_tui_flag_the_build_and_the_display` (args x env x build)
- vr-ide `event_loop::tests::a_new_mark_is_seen_even_when_another_was_cleared`
- vr-ide `window_frontend_tests::` the_window_frontend_passes_each_call_to_the_window, the_window_gets_only_changed_values_and_no_terminal_key_notice, closing_the_window_or_sigterm_takes_the_ctrl_q_path (clean → quit; unsaved + open Go-to-Line dialog → dialog closed, Quit question, buffer kept; SIGTERM flag → same), closing_the_window_with_a_running_terminal_asks, font_settings_feed_the_font_set_and_a_missing_family_warns, a_settings_reload_relays_the_grid_and_warns_for_a_missing_family (+ theme → palette), the_palette_follows_the_theme, the_first_window_start_migrates_an_old_menu_entry (temp data home), embedded_terminals_get_the_window_cell_size (real /bin/sh PTY: 0x0, then (9,19) with the grid unchanged)

## Commands run (trimmed; full log qa-artifacts/T4/verify.log)
```
$ cargo test -p vr-ide --lib -- window_frontend cli:: event_loop:: clipboard::
test result: ok. 37 passed; 0 failed   (first run)
mutations (copies in qa-artifacts, restored, cmp equal): close without request_quit; fit_term without the cell-size check
  window_frontend_tests::closing_the_window_or_sigterm_takes_the_ctrl_q_path ... FAILED
  window_frontend_tests::embedded_terminals_get_the_window_cell_size ... FAILED
$ run-heavy.sh cargo test -p vr-ide --lib -- window_frontend cli:: event_loop:: clipboard:: terminal_tests ai_tests ai_switch desktop config_reload theme::
test result: ok. 234 passed; 0 failed; 923 filtered out
$ run-heavy.sh cargo test -p vr-window        → ok. 53 passed
$ run-heavy.sh cargo test -p vr-term          → all suites ok (23, 3, 1, 1 + 1 ignored, 1, 6, 22)
$ cargo tree -p vr-ide --no-default-features --features window -e normal | grep -ciE "gtk|glib|webkit"   → 0
$ run-heavy.sh cargo build -p vr-ide --no-default-features --features window   → exit 0
$ run-heavy.sh cargo clippy -p vr-ide --no-default-features --all-targets -- -D warnings   → Finished, exit 0 (after the pty_size cfg fix)
$ cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings   (guard, exactly)   → Finished, exit 0
$ run-heavy.sh cargo build --release -p vr-ide   → exit 0
```
Real runs (debug binary built after the last runtime edit; config/state/data dirs under qa-artifacts/T4; project qa-artifacts/T4/proj/a.txt "Xin chào thế giới"):
```
wayland VR_IDE_EXIT_AFTER_FIRST_FRAME=1           exit=0 (0.35 s)
x11 (WAYLAND_DISPLAY unset), same                  exit=0 (0.28 s)
x11 window in xwininfo: "vr-ide — /home/rainy/DEV/vr-ide": ("vr-ide" "vr-ide") 1080x648   (WM_CLASS vr-ide; 120x36 cells of 9x18)
screenshot (xwd → png): qa-artifacts/T4/x11.png — editor, tab bar, status bar, Vietnamese diacritics, theme colours
SIGTERM via `timeout --preserve-status -s TERM 4`: wayland exit=0, x11 exit=0, release binary on wayland exit=0 (quit path; the default action would give 143)
--tui (pty via script)                             exit=0, alternate screen entered, text drawn
no DISPLAY / WAYLAND_DISPLAY                       exit=0, TUI
DISPLAY=:97 (no server)                            exit=0, stderr "vr-ide: cannot connect to the display: … Failed to open connection to X server; running in the terminal instead", TUI
--install-desktop (XDG_DATA_HOME temp)             Exec="<exe>" --session, Terminal=false, StartupWMClass=vr-ide
```
No Wayland screenshot: no tool for GNOME Wayland without installing one (grim needs wlr-screencopy); X11 capture via xwd + convert (present).

## Manual check list (for the user)
1. GNOME Wayland: `vr-ide` from a terminal → window with the adwaita title bar (CSD), title "vr-ide — <folder>", close button works; with an unsaved file the close button asks Save all / Discard / Cancel.
2. IBus/Fcitx with Bamboo or Unikey (Telex): type "tieengs vieejt" in the editor, the Go-to-Line prompt and an embedded terminal → "tiếng việt"; composing text underlined at the cursor; candidate window next to the cursor; Esc while composing drops it and nothing is typed.
3. HiDPI: move the window between a 100 % and a 200 % screen (or change GNOME scaling) → text re-laid sharp, no garbled cells; `stty size` in an embedded terminal follows.
4. Chords: ctrl+shift+p, ctrl+/, ctrl+[ vs Esc, ctrl+h vs Backspace reach vr-ide; no "keys this terminal cannot send" notice.
5. Mouse: click, drag-select, wheel in editor/explorer/terminal; copy in vr-ide → paste in another app and back.
6. AI attention: an agent rings the bell while the window is unfocused → taskbar/dock hint; focused → none.
7. Menu entry: with an old GNOME Terminal entry installed by vr-ide, the first window start shows "Updated the vr-ide menu entry to open its own window." and the launcher then starts the window directly.
8. `kill <pid>` (SIGTERM) with unsaved work → the Quit question appears instead of the window vanishing.
9. settings.toml: `font_family = "JetBrains Mono"` (installed), then a family that is not installed → warning notice and the default font; `font_size` / `line_height` changes re-lay the grid live.

## Deviations from the plan (with reason)
- Files beyond the board row (listed above): each is the smallest change an AC needs (OSC 52 only in the TUI, true colour in the window, PTY pixel size reaching new terminals).
- No `EventLoopProxy`: the app has no wake hook (brief: "else rely on next_wake").
- SIGTERM handled only in the window (the TUI keeps today's behaviour); SIGINT/SIGHUP not handled (not asked).
- A TUI fallback after a failed window start keeps true colour (the depth is fixed once per run, before the app loads).
- The font warning and the migration notice are shown on the first window frame, so a TUI fallback never shows them.
- Worktree base: the worktree was created at 5562f28; moved to the brief's base f1279ad with a fast-forward on a clean tree before any change.

## Out-of-scope issues noticed
- None. Merge note: event_loop.rs is touched only at `attention_marks` / `new_mark`, the attention line in `step` and the end of the test module (T3b edits lines 15-110).

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| loop turn | event_loop::step (T1) | reused unchanged; `WindowFrontend` implements `Frontend`, `PixelBackend` implements `DrawnCursor` |
| renderer / fonts / input | vr-window backend, font, input (T2, T3) | reused; one getter added |
| quit path | `app.quit` command, `commands::dialog::answer` | reused (`request_quit` = cancel dialog + ctrl+q command) |
| menu migration | `desktop::migrate_on_window_start`, `data_home` (T5) | reused |
| PTY pixels | `Terminal::resize_with_pixels` (T4a) | reused via `fit_term` |
| clipboard | `Clipboard` trait, arboard | new `WindowClipboard` (SystemClipboard's fallback writes OSC 52 to stdout) |
| SIGTERM | signal-hook 0.3.18 (already in the lock) | reused crate version |
| test helpers | app_tests::app_with_file, terminal_tests::{term_app, ctrl_backtick, draw, tab}, desktop::install | reused |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | scope = T4 + the small listed extras with reasons (Deviations) |
| BASE-2 | pass | T1/T2/T3/T4a/T5 contracts used as documented; no contract changed (one additive getter each in vr-window and vr-term) |
| BASE-3 | pass | tests per AC listed; red shown by 2 mutations (output above) |
| BASE-4 | pass | real outputs above, qa-artifacts/T4/verify.log |
| BASE-5 | pass | heavy runs via run-heavy.sh |
| BASE-6 | pass | code, comments, notes in English |
| BASE-7 | pass | nothing staged or committed |
| BASE-8 | pass | softbuffer 0.4.8 (2025-12-13), tiny-xlib 0.2.5 (2026-04-20), ctor 0.10.1 (2026-04-22), dtor 0.8.1 (2026-04-22): all ≥ 14 days, pinned `=`; dates from the crates.io index `pubtime` |
| BASE-9 | pass | args/env boundary: `--tui` exact match, empty display vars count as unset; window title from `App::title` (sanitised) |
| BASE-10 | pass | related tests of the changed files + mapped suites only |
| BASE-11 | pass | no checkout/restore/stash; base moved with a fast-forward merge on the clean worktree |
| BASE-13 | pass | throwaway files only in qa-artifacts/T4 and the scratchpad |
| BASE-14 | pass | long builds/tests as background tasks, turn ended each time |
| BASE-15 | pass | only own processes; every window run ended by its own hook or `timeout` |
| BASE-16 | pass | Reuse table; capabilities.md updated |
| BASE-17 | pass | nothing installed; screenshot only with tools present (xwd, convert) |
| BASE-18 | pass | no agents started |
| BASE-19 | pass | notes created before the first change, progress lines |
| BASE-20 | pass | `TermTab::resize` signature change: grep for `tab.resize(` / `t.resize(` → 3 call sites, all updated |
| BASE-22 | pass | sentence-style test names; vr-ide tests in `window_frontend_tests.rs`, vr-window in `app/tests.rs` (neighbours' layout) |
| BASE-23 | pass | entry paths: window (real Wayland + X11 runs, WindowApp tests), `--tui`, no display, failed display (real runs) |
| BASE-25 | pass | guard exactly as written → exit 0; plus the no-default-features clippy → exit 0 |
| BASE-26 | pass | TUI unchanged: existing related tests green unchanged; real `--tui` run |
| BASE-32 | pass | D3: vr-window knows no App (runner generic over `WindowHandler`); D5 unaffected |
| BASE-35 | pass | repo files English, no AI attribution |
| BASE-38 | pass | rustfmt only on the changed files |
| BASE-46 | pass | real runs on binaries built after the last runtime edit (debug + release) |
| BASE-49 | pass | winit / softbuffer / signal-hook API read from ~/.cargo/registry sources |
| BASE-50 | pass | no other task's files edited beyond those listed |
| BASE-51 | pass | mutations on copies, restored, `cmp` equal, copies removed |
| COST-1 | pass | heavy runs via run-heavy.sh |
| COST-3 | pass | no polling loops; one `sleep 3` waited for the window to map before `xwd` |
| COST-8 | pass | new files written whole; edits batched |
| COST-9 | pass | nothing left running (`ps`: no vr-ide process) |
| COST-10 | pass | related set includes terminal_tests, ai_tests, ai_switch, desktop, config_reload, theme |
| COST-13 | pass | artifacts in qa-artifacts/T4 only |
| COST-16 | pass | full output in verify.log, summary here |

n-a: BASE-12 (no other repo) · BASE-21, BASE-41 (no secrets) · BASE-24 (no commit) · BASE-27, BASE-30, BASE-31, BASE-39, BASE-40 (no shared data, containers, schema, outward writes; the real ~/.local untouched: XDG_DATA_HOME pointed at qa-artifacts) · BASE-28 (no third-party code copied) · BASE-29 (no browser) · BASE-33, BASE-48 (no deployment) · BASE-34 (nothing blocked) · BASE-36 (single app) · BASE-37 (not a bug fix) · BASE-42 (no new runner) · BASE-43 (no access fix) · BASE-44 (no fake of another system) · BASE-45 (ACs as given) · BASE-47 (no computed value changed) · COST-2, COST-4..7, COST-11, COST-12, COST-14, COST-15, COST-17..21 (developer task; no UI sweep/e2e/CI/goldens)

AC: W1 pass (real Wayland/X11 window; `--tui`, no display, failed display → TUI) · W2/W3 pass (T2 backend; damage-only present, `copy_plan` test) · W4/W5 pass (T3 input wired; enhanced keyboard and no notice tested; preedit at the backend cursor; IME area per cell tested; Esc reset) — real IME typing is manual item 2 · W6 pass (mouse/wheel wired; WindowClipboard; title/cursor/attention via WindowOps tests; PTY pixels test) · W7 pass (`launches_in_window = cfg!(feature)`, real `--install-desktop` output, migration wiring test) · W8 pass (close/SIGTERM tests + real SIGTERM exit 0; font settings tests incl. missing family).

## Fix round 1
Developer replaced (context limit); continued from the notes above. Findings: review_T4.md 1-4.
### Progress
- read: review_T4.md, notes, app.rs, window_frontend.rs, main.rs, theme.rs, event_loop::exit/finish, winit EventLoopError, signal-hook flag API
- tests written first, red seen (3 in vr-window, 2 in vr-ide); fixes in app.rs, window_frontend.rs, theme.rs, main.rs; targeted runs green
- related tests, both clippy variants, guard, debug build green; real runs done (below)

### Per finding (test seen red first → green)
1. [blocker] display lost after the start → normal exit path. `crates/vr-window/src/app.rs`: `Runner.started` (set when the window and surface exist), `Runner::finish(run_app result)`: start error → `Err`; `run_app` Err before the start → `Err(StartError::EventLoop)` (TUI fallback as before); after the start → `handler.failed("the connection to the display was lost: …")`, `Ok(())` → main runs `event_loop::exit` (recovery, session save, terminal/AI/git shutdown), no TUI fallback. Presenting errors now say "drawing into the window failed: …" in the runner; `WindowApp::failed` (`crates/vr-ide/src/window_frontend.rs`) keeps the text as given.
   - red: `app::tests::a_display_lost_after_the_start_ends_through_the_app_not_as_a_start_error` — `assertion failed: runner(&mut h, true).finish(lost).is_ok()` → green.
   - chain pin (passes before and after; not a red test): `window_frontend_tests::a_lost_display_ends_through_the_exit_path_with_unsaved_work_written` (failed → take_result Err → `event_loop::finish` writes the unsaved buffer).
   - real: nested mutter (Wayland) killed under a running window, vr-ide without a tty (`setsid … </dev/null`, as from the menu), `--session`: stderr `Error: the connection to the display was lost: Exit Failure: 1`, exit 1, `state/vr-ide/session.toml` rewritten, no "running in the terminal instead".
   - real X11 (Xephyr :98 killed): stderr `X connection to :98 broken (explicit kill or server shutdown).`, exit 1 — Xlib's default IO error handler calls `exit()` inside winit before `run_app` returns, so on X11 neither the old nor the new code runs. Out of scope here (see Out-of-scope below).
2. [major] IME area re-sent after the IME starts over. `app.rs`: new `ImePlacement` (cell, last layout, pending; `set_cell`, `reset`, `take(layout)`) replaces `Control.ime_cell/ime_pending`; `resets_ime(&WindowEvent)` = `Focused(true)` or `Ime(Enabled)` → `reset()` at the top of `window_event`; the Esc-cancel `set_ime_allowed(false/true)` also calls `reset()`. The area goes out in `about_to_wait` after the step, so after focus/first map/enable the candidate window is placed at the cursor.
   - red: `app::tests::the_ime_area_is_sent_again_after_the_ime_starts_over` — `assertion left == right failed: after a reset the area goes out again`; `app::tests::focus_and_ime_enable_start_the_ime_over` — `assertion failed: resets_ime(&WindowEvent::Focused(true))` → both green.
3. [minor] SIGTERM never left ignored. `window_frontend.rs`: `start` registers `flag::register_conditional_default(SIGTERM, sigterm_default)` then `flag::register(SIGTERM, sigterm)`; `Drop` sets `sigterm_default` (no unregister: signal-hook cannot restore the default disposition, an unregistered SIGTERM stays ignored — comments say so). After the window is gone (also during the exit sequence, as in the TUI) SIGTERM ends the process.
   - red: `window_frontend_tests::sigterm_ends_the_process_once_the_window_is_gone` (re-runs the test binary as a child that starts a WindowApp, drops it, raises SIGTERM) — `the child must end by SIGTERM, got ExitStatus(unix_wait_status(0))` → green.
   - real: SIGTERM while the window runs (wayland, `timeout --preserve-status -s TERM 4`) → exit 0 (quit path, unchanged).
4. [minor, D6] fallback TUI uses the terminal's depth. `crates/vr-ide/src/theme.rs`: `DepthSource` (window flag + `COLORTERM` read once) replaces the `OnceLock`; `ColorDepth::use_true_color()` (now returns `()`; its only caller ignored the bool) and new `use_terminal_depth()`; `crates/vr-ide/src/main.rs` calls `use_terminal_depth()` in the fallback branch. Theme is rebuilt per frame from `detect()`, so the fallback TUI draws in the terminal's depth.
   - red: `theme::tests::a_window_draws_true_colour_and_its_terminal_fallback_the_terminal_depth` — `256-colour terminal; left: TrueColor` → green.
   - real: `DISPLAY=:97` (no server) in a pty 100x30: COLORTERM unset → 20 `38;5;/48;5;` sequences, 0 `38;2;`; COLORTERM=truecolor → 20 `38;2;/48;2;`, 0 indexed. Before: the T4 run `baddisplay.typescript` (COLORTERM unset) had 20 `38;2;/48;2;`.
   - limit: a terminal restored from the session before the window start got `COLORTERM=truecolor` in its child env; after a fallback its cells are still drawn in the terminal's depth by `render_cells`.

### Files changed (round 1)
`crates/vr-window/src/app.rs`, `crates/vr-window/src/app/tests.rs`, `crates/vr-ide/src/window_frontend.rs`, `crates/vr-ide/src/window_frontend_tests.rs`, `crates/vr-ide/src/theme.rs`, `crates/vr-ide/src/main.rs`.

### Commands (trimmed; full log qa-artifacts/T4/fix1.log)
```
$ rustfmt --edition 2024 <the 6 files above>
$ run-heavy.sh cargo test -p vr-window app                        → ok. 7 passed; 0 failed
$ run-heavy.sh cargo test -p vr-ide --lib -- window_frontend cli:: event_loop:: theme:: clipboard:: terminal_tests
                                                                   → ok. 125 passed; 0 failed
$ run-heavy.sh cargo clippy -p vr-ide --all-targets -- -D warnings                       → exit 0
$ run-heavy.sh cargo clippy -p vr-ide --no-default-features --all-targets -- -D warnings → exit 0
$ cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings  (guard, exactly) → Finished, exit 0
$ run-heavy.sh cargo build -p vr-ide                               → exit 0
```
Real runs (binary built after the last edit; XDG dirs under qa-artifacts/T4; scripts fix1-display-lost.sh, fix1-wayland-lost.sh):
```
wayland VR_IDE_EXIT_AFTER_FIRST_FRAME=1          exit=0
x11 (WAYLAND_DISPLAY unset), same                 exit=0
DISPLAY=:97 fallback, COLORTERM unset / truecolor exit=0 / 0, depth as above, stderr line present
nested mutter killed                              exit=1, "Error: the connection to the display was lost: Exit Failure: 1", session saved
Xephyr killed                                     exit=1, Xlib "X connection to :98 broken" (Xlib exits)
SIGTERM in window (wayland)                       exit=0
```
Nothing left running: the only vr-ide process (pid 1696342, started 2026-11-27 21:31, ~/.cargo/bin/vr-ide) is the user's, not mine; Xephyr and mutter killed by their recorded pid in the scripts.

### Out-of-scope (round 1)
- X11 display loss: Xlib's default IO error handler `exit()`s the process inside winit (exit 1), so the exit path (recovery, session save) does not run on X11. A fix needs `XSetIOErrorExitHandler` / a longjmp-free strategy in winit's X11 backend or a vr-window hook; for the leader to decide.

### Rule compliance changes (round 1)
| Rule | Status | Evidence |
|---|---|---|
| BASE-2 | pass | `run`'s documented contract ("Err only before the start") now holds; `WindowHandler::failed` doc covers display loss; `ColorDepth::use_true_color` return type `bool` → `()` (one caller, grep) |
| BASE-3 | pass | 5 red tests seen failing first (outputs above), green after |
| BASE-4 | pass | real outputs above, fix1.log |
| BASE-7 | pass | nothing staged or committed; git untouched |
| BASE-14 | pass | test/clippy/guard/build as background tasks, turn ended |
| BASE-15 | pass | only own Xephyr/mutter/vr-ide by recorded pid; the user's vr-ide left alone |
| BASE-20 | pass | `use_true_color` signature: 1 call site (main.rs) |
| BASE-23 | pass | entry paths rerun: window wayland/x11, failed display fallback, display lost after start, SIGTERM |
| BASE-25 | pass | guard exactly → exit 0 |
| BASE-38 | pass | rustfmt on the 6 changed files only |
| BASE-46 | pass | real runs on the debug binary built after the last edit |
| BASE-51 | n-a | no experiments on copies this round (red came from stubs written before the fixes) |
| COST-3 | pass | no polling loop (the hook refused one; scripts use fixed pauses) |
| COST-9 | pass | nothing of mine left running |
AC-W5: pass (IME area after focus / enable / cancel reset; real IME typing stays manual item 2). AC-W8: pass (display loss on Wayland ends through the exit path; X11 noted above).

## Fix round 2
### Progress
- read: coordinator message (QC_FAIL headless Wayland loss, X11 IO error hook, SIGTERM gap), qc lost.sh
- finding 1: qc repro forces WAYLAND_DISPLAY=wayland-0 (run.sh); corrected replay exits within 1 s. findings 2/3: red tests seen, fixes in place, targeted runs green
- related tests, both clippy variants, guard (2nd run, after a clippy `forget_non_drop` fix), build green; real runs done

### Per finding
1. [QC_FAIL] Not a vr-ide bug: qc's `qa-artifacts/T4/qc/run.sh wl` runs `export WAYLAND_DISPLAY=wayland-0`, which overrides the `env WAYLAND_DISPLAY=vrqc-wl` in lost.sh, so vr-ide ran on the user's GNOME session, and killing the headless mutter did not affect it.
   - Replayed qc's lost.sh with the same mutter command and the same run.sh, writing to `qa-artifacts/T4/r2/lost-replay.sh qc`. Its output: `vr-ide … WAYLAND_DISPLAY=wayland-0` (read from /proc/<pid>/environ), still alive after the kill, exit 124. Same result as qc's.
   - Corrected replay (`lost-replay.sh fixed`: a copy of run.sh that keeps the given WAYLAND_DISPLAY): `WAYLAND_DISPLAY=vrr2-wl`, stdin /dev/null, clean buffer and idle input. vr-ide was **gone 1 s after** the SIGKILL of mutter, exit 1, stderr `Error: the connection to the display was lost: Exit Failure: 1` (the round-1 exit path). Rerun on the final binary gave the same result.
   - No code change. QC should fix run.sh, e.g. `${WAYLAND_DISPLAY:-wayland-0}`.
2. [X11 server loss] `crates/vr-window/src/app/x11.rs` (new): `set_hook(fn())` (static OnceLock), `install()` = `x11_dl::xlib::Xlib::open()` + `XSetIOErrorHandler(io_error)`, called from `run` after `EventLoop::new` when `event_loop.is_x11()`. `io_error` is an `extern "C"` fn: it runs the hook inside `catch_unwind` and returns 0, then libX11 exits as before. Public `vr_window::app::on_x11_connection_lost(hook)` is a no-op off X11. In vr-ide (`window_frontend.rs`): `write_recovery_on_display_loss(state, dir)` = `recovery::write_recovery` (poison-tolerant `snapshot`) plus stderr lines, and it never touches App. `run(app, recovery_dir)` stores the dir in a OnceLock and registers `x11_connection_lost`. New dependency: `x11-dl = "=2.21.0"`, already in the lock through winit; only vr-window's dependency list in Cargo.lock changed.
   - red: `app::tests::a_lost_x11_connection_runs_the_registered_hook_first` failed with `the hook ran; left: 0 right: 1` → green. `window_frontend_tests::display_loss_writes_the_published_unsaved_buffers` failed with `left: 0 right: 1` → green.
   - real (`r2/x11-lost.sh`): Xephyr :98, vr-ide without a tty, `space i` + `zz` typed through XTest (qc's xin.py), then Xephyr killed. Result: exit 1, stderr `vr-ide: the connection to the display was lost` / `vr-ide: unsaved buffer written to …/r2/state/vr-ide/recovery/20261128T005706Z-…a.txt`, recovery file content `zzhello`, a.txt on disk unchanged (`hello`).
3. [SIGTERM gap] `WindowApp` no longer has a `Drop`. A new `SigtermDefault` switch (Arc<AtomicBool>, `enable()`) feeds the conditional default. `run` returns `WindowRun { result, sigterm }`. New `event_loop::exit_with(.., after_recovery, ..)` calls `after_recovery` right after `finish` (recovery). `exit` = `exit_with` with a no-op, so the TUI and the tests are unchanged. main's window branch passes `|| run.sigterm.enable()`.
   - red: `window_frontend_tests::sigterm_ends_the_process_once_recovery_is_written_after_the_window` (replaces the round-1 test; child process: WindowApp dropped → SIGTERM must be survived → switch → SIGTERM must kill). It failed with `SIGTERM before the recovery write must not end the process: ExitStatus(unix_wait_status(15))` → green.
   - pin (written with the fix, not red): `window_frontend_tests::the_exit_switches_sigterm_back_only_after_recovery_is_written`. It counts 1 recovery file at the switch.
   - real: SIGTERM while the window runs gives exit 0 on Wayland and X11 (quit path).

### Files changed (round 2)
`Cargo.toml`, `Cargo.lock` (vr-window deps +x11-dl), `crates/vr-window/Cargo.toml`, `crates/vr-window/src/app.rs`, `crates/vr-window/src/app/x11.rs` (new), `crates/vr-window/src/app/tests.rs`, `crates/vr-ide/src/window_frontend.rs`, `crates/vr-ide/src/window_frontend_tests.rs`, `crates/vr-ide/src/event_loop.rs` (only `exit` → `exit_with`, lines ~125-150; T3b's lines 15-110 untouched), `crates/vr-ide/src/main.rs`.

### Commands (trimmed; full log qa-artifacts/T4/fix2.log)
```
$ rustfmt --edition 2024 <changed .rs files>
$ run-heavy.sh cargo test -p vr-window app                         → ok. 8 passed; 0 failed
$ run-heavy.sh cargo test -p vr-ide --lib -- window_frontend cli:: event_loop:: theme:: clipboard:: terminal_tests git_tests recovery
                                                                    → ok. 166 passed; 0 failed
$ clippy -p vr-ide --all-targets -D warnings   → 1st: error `std::mem::forget` with a value that does not implement Drop (x11.rs) → removed (x11-dl caches the library process-wide) → exit 0
$ clippy -p vr-ide --no-default-features --all-targets -D warnings → exit 0
$ cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings  (guard; 1st run failed on the same lint, rerun after the fix) → exit 0
$ run-heavy.sh cargo build -p vr-ide → exit 0 (rebuilt after the last comment edit)
```
Real runs (final debug binary): wayland/x11 first-frame exit 0/0; headless mutter killed: gone within 1 s, exit 1; Xephyr killed with a dirty buffer: recovery `zzhello`; SIGTERM in the window: wayland/x11 exit 0/0. Nothing of mine left running (no mutter --headless/--nested, no Xephyr; pid 1696342 is the user's vr-ide).

### Rule compliance changes (round 2)
| Rule | Status | Evidence |
|---|---|---|
| BASE-2 | pass | `window_frontend::run` signature (+recovery_dir, returns WindowRun): 1 caller (main); `event_loop::exit` unchanged, `exit_with` added |
| BASE-3 | pass | 3 red tests seen failing, green after; 1 pin test labelled as such |
| BASE-8 | pass | x11-dl 2.21.0 already in Cargo.lock (winit dep), pinned `=` |
| BASE-20 | pass | callers of changed signatures: main.rs only (grep) |
| BASE-25 | pass | guard exactly; the 1st run failed (lint), fixed, rerun → exit 0 |
| BASE-45 | pass | QC_FAIL traced to the repro's env override; evidence (environ of the process) instead of a code change |
| BASE-46 | pass | real runs on the binary built after the last edit |
| BASE-15 / COST-9 | pass | Xephyr/mutter/vr-ide stopped by recorded pid; nothing left |
AC-W8: pass on Wayland (exit path) and X11 (recovery from the IO error hook; session save is not possible on X11 because libX11 exits).
