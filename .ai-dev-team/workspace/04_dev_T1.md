# T1 Frontend seam: event_loop::step + Frontend trait
Progress:
- read: event_loop.rs, main.rs, lib.rs, ai/attention.rs, commands/terminal.rs cursor_style, ratatui-core Frame/TestBackend API
- code: new crates/vr-ide/src/frontend.rs; event_loop.rs split into step + run; main.rs terminal code moved to frontend.rs
- related tests: `cargo test -p vr-ide --lib -- event_loop frontend a_new_mark_asks` → 19 passed
- full crate tests + clippy (run-heavy): green
- real TUI smoke run on the built binary: quits on ctrl+q, exit 0
- guard: workspace form fails only in crates/vr-window (in progress, T2); `-p vr-ide` form green

## Files changed
- `crates/vr-ide/src/frontend.rs` (new): `Frontend` trait, `CursorShape`, `DrawnCursor`, `Crossterm<W>` frontend, `enter_terminal` / `restore_terminal` / `enhance_keyboard` (moved from main.rs unchanged in behaviour, `ENHANCED` static moved with them), `tests::FakeFrontend` (pub(crate), test only).
- `crates/vr-ide/src/event_loop.rs`: `StepOutcome`, `StepState`, `step`, `handle_event`, `attention_marks`; `run` now `run(terminal, frontend, app)` = `poll → read (lazy drain) → step`; `cursor_style` returns `CursorShape`; `set_cursor_shape` / `set_title` go through the frontend. `chord_from_key`, `legacy_unreachable`, `draw_frame`, `finish`, `exit`, `poll_interval`, `background_pass`, `changed_title` unchanged.
- `crates/vr-ide/src/main.rs`: terminal setup/restore/keyboard push now call `vr_ide::frontend::*`; builds `Crossterm::stdout(enhance_keyboard())`, `app.report_keyboard(frontend.keyboard_enhanced())`.
- `crates/vr-ide/src/lib.rs`: `pub mod frontend;`
- `crates/vr-ide/src/ai_tests.rs`: one new test appended (no existing test changed).
- `.ai-dev-team/capabilities.md`: two lines under "vr-ide (app)".
Not touched: Cargo.toml / Cargo.lock (their diff in `git status` belongs to the vr-window task), desktop.rs, other crates.

## Tests added
- `event_loop::tests::step_handles_events_in_order_then_draws` — x, y, resize, z in order → "xyza", drawn, not quit.
- `event_loop::tests::step_stops_reading_events_at_a_quit` — ctrl+q then x: quit outcome, the event after the quit is never pulled from the iterator.
- `event_loop::tests::step_reports_the_next_wake_from_the_poll_interval` — `next_wake == POLL == poll_interval(app)`.
- `event_loop::tests::step_gives_the_frontend_only_changed_values` — title, cursor shape and IME area on first draw; nothing on a redraw with no change; only the IME area after a cursor move; title after a workspace switch, a failed title write retried once, then not again.
- `event_loop::tests::a_title_already_shown_is_not_written_again` — `StepState::with_title`.
- `frontend::tests::cursor_shapes_round_trip_through_crossterm`, `crossterm_frontend_writes_title_and_cursor_sequences` (same bytes as the old `execute!` calls), `test_backend_reports_the_shown_cursor_only`.
- `ai_tests::a_new_mark_asks_the_frontend_for_attention` — real PTY agent rings the bell with the panel hidden → `request_attention` once; the same mark again asks nothing more.

## How T4 uses the seam
```rust
// main thread, inside the winit ApplicationHandler
let mut state = StepState::default();          // or with_title(..) if the window was created with it
// on each batch of translated events (KeyEvent/MouseEvent/Paste/Resize as crossterm::event::Event):
let out = event_loop::step(&mut terminal, &mut window_frontend, &mut state, &mut app, events, Instant::now())?;
if out.quit { /* event_loop.exit(); then event_loop::exit(...) as main does */ }
// else: ControlFlow::WaitUntil(Instant::now() + out.next_wake); background threads wake via EventLoopProxy
```
- The window backend implements `ratatui::backend::Backend + frontend::DrawnCursor` (it sees ratatui's `show_cursor`/`hide_cursor`/`set_cursor_position` calls, so it reports the cursor cell cheaply); `step` then passes `Some(Rect{x,y,1,1})` (cells) to `Frontend::set_ime_area`, only when it changes — the window converts cells to pixels.
- `Frontend::request_attention` is called by `step` when the number of AI sessions with an attention mark grows during the background pass (the hook point is `attention_marks` in event_loop.rs). The window decides whether it is focused (spec: only when not focused). The terminal frontend ignores it.
- `keyboard_enhanced()` → `true` for the window; pass it to `app.report_keyboard(..)` once at start; `chord_from_key(key, app.keyboard_enhanced)` is unchanged.
- `App` needs no `Send`; `step` is called on the thread that owns `App`.

## Commands run (trimmed output)
- `cargo test -p vr-ide --lib -- event_loop frontend a_new_mark_asks` → `test result: ok. 19 passed; 0 failed`
- `run-heavy.sh cargo test -p vr-ide` (log qa-artifacts/T1/run1.log):
  `test result: ok. 1129 passed; 0 failed; 7 ignored` (lib) + bins/integration `3 passed`, `6 passed`, `4 passed`, doc 0.
- `run-heavy.sh cargo clippy -p vr-ide --all-targets -- -D warnings` → `Finished` (only warning: manifest `unused workspace dependency vr-window`, from the other task's Cargo.toml).
- `rustfmt --edition 2024` on event_loop.rs, frontend.rs, main.rs, lib.rs, ai_tests.rs.
- Real run (BASE-46), binary built 06:16 after the last runtime edit: `(sleep 4; printf '\021') | timeout 20 script -qfec "stty rows 24 cols 80; target/debug/vr-ide qa-artifacts/T1/proj/a.txt"` with state/config dirs under qa-artifacts/T1 → `exit 0` (ctrl+q quit through `poll → read → step`; not the timeout), file text "hello" drawn, title sequence `ESC]0;vr-ide — …` once, cursor-shape sequence once. Typescript: qa-artifacts/T1/smoke.typescript.
- Guard exactly as written (`cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings`, log qa-artifacts/T1/guard.log) → exit 1:
  `Error writing files: failed to resolve mod 'tests': crates/vr-window/src/backend/tests.rs does not exist` — vr-window only (in progress in parallel).
- Guard with `-p vr-ide` (`cargo fmt -p vr-ide --check && cargo clippy -p vr-ide --all-targets -- -D warnings`, log qa-artifacts/T1/guard-vr-ide.log) → exit 0, `Finished`.

## Deviations from the plan (with reason)
- `Frontend::set_title` / `set_cursor_shape` return `io::Result<()>`: today's loop counts a failed write as not written and retries on the next draw; kept.
- IME area: ratatui's `Frame` keeps the cursor position private, so the cell comes from the backend through a small `DrawnCursor` trait (`step` requires `B: Backend + DrawnCursor`). Crossterm returns `None` (asking a terminal is a round trip; terminals place the IME themselves), so in the TUI `set_ime_area(None)` is called once and is a no-op.
- `step` takes `now: Instant` from the caller (taken just before the queued events are read instead of just after; the reads are non-blocking, timers have 50 ms granularity).
- Error path only: if `event::read`/`poll` fails mid-drain, `step` still runs the background pass and a draw before `run` returns the read error (before, the read error returned at once). The returned error is the read error, as before.
- `StepState::with_title`: the terminal sets the first title in `enter_terminal`, so `run` starts with it as written (same as before).

## Out-of-scope issues noticed
- None in vr-ide. The workspace guard is red until crates/vr-window gets `src/backend/tests.rs` (T2).

## Reuse
| Needed | Looked in | Result |
|---|---|---|
| terminal title/cursor/keyboard code | event_loop.rs, main.rs | moved into frontend.rs, not rewritten |
| cursor shape from the terminal panel | `commands::terminal::cursor_style` | reused, mapped with `CursorShape::from_crossterm` |
| attention event | ai/attention.rs (`mark`, `SessionState.attention`) | counted in `step`; no change to attention.rs |
| test app / AI notify agent | `app_tests::app_with_file`, `ai_tests::notify_app`, `say` | reused |

## Rule compliance
| Rule | Status | Evidence |
|---|---|---|
| BASE-1 | pass | diff limited to crates/vr-ide/src/{frontend.rs,event_loop.rs,main.rs,lib.rs,ai_tests.rs} + capabilities.md (`git status --short`) |
| BASE-2 | pass | brief contract: `step` + `Frontend` (5 methods); deviations above with reasons |
| BASE-3 | pass | new behaviour (`step`, `Frontend`) covered by the 9 tests listed; written together with the code (a refactor with behaviour pinned by the existing tests), first run of `step_gives_the_frontend_only_changed_values` failed (test setup: switch refused on a modified buffer), fixed in the test |
| BASE-4 | pass | outputs pasted above, logs in qa-artifacts/T1/ |
| BASE-5 | pass | crate tests, clippy and guard through run-heavy.sh |
| BASE-6 | pass | code, comments and notes in English |
| BASE-9 | pass | no new input boundary; titles still sanitized by `App::title` (`the_window_title_follows_a_workspace_switch` green) |
| BASE-10 | pass | related tests first, then `cargo test -p vr-ide` as the brief asks |
| BASE-11 | pass | no checkout/restore/stash; originals copied to qa-artifacts/T1/*.orig before splicing |
| BASE-13 | pass | throwaway files only under qa-artifacts/T1 |
| BASE-14 | pass | the long run ran as a background task; turn ended, resumed on notification |
| BASE-15 | pass | only own processes; smoke run under `timeout`, exited 0 |
| BASE-16 | pass | `## Reuse` table; capabilities.md updated |
| BASE-18 | pass | no agents started |
| BASE-19 | pass | notes created before the first change, progress lines above |
| BASE-20 | pass | removed `ENHANCED`/`restore_terminal` from main.rs only after moving them; `grep -rn "event_loop::run"` → main.rs only caller |
| BASE-22 | pass | test names follow neighbours (sentence-style), module `frontend` next to `event_loop` |
| BASE-23 | pass | two entry paths of `step`: terminal `run` (real smoke run + crate tests) and direct calls (new step tests) |
| BASE-25 | pass | guard run exactly as written: fails only in crates/vr-window (output above); `-p vr-ide` form green |
| BASE-26 | pass | parity of moved terminal code: `crossterm_frontend_writes_title_and_cursor_sequences` (same bytes), cursor-shape round trip, existing tests unchanged and green, real run |
| BASE-32 | pass | D3 honoured: no window code in vr-ide; seam only |
| BASE-35 | pass | repo files in English; no AI attribution |
| BASE-38 | pass | rustfmt only on the 5 changed files |
| BASE-46 | pass | real run on binary built after the last runtime edit (smoke.typescript) |
| BASE-49 | pass | ratatui/crossterm usage read from their sources in ~/.cargo/registry, no probing runs |
| BASE-50 | pass | n/a for a developer beyond: no files of other tasks edited |
| BASE-51 | pass | splice done from copies `qa-artifacts/T1/event_loop.rs.orig`, `main.rs.orig` |
| COST-1 | pass | heavy runs via run-heavy.sh |
| COST-3 | pass | no polling loops (the smoke run's single input delay is a pipe, not a wait on a process) |
| COST-8 | pass | new file written whole; edits batched |
| COST-9 | pass | nothing left running (background task btyfmenw0 completed; smoke run exited) |
| COST-10 | pass | `grep -rln "event_loop\|frontend::" src/*_tests.rs` → ai, syntax, git, terminal, app tests: all in the `cargo test -p vr-ide` run (1129 passed) |
| COST-13 | pass | scripts/artifacts only in qa-artifacts/T1 |
| COST-16 | pass | full outputs in logs, summaries pasted |

n-a: BASE-7, BASE-24 (no commit/index change; leader commits) · BASE-8, BASE-17 (no dependency or toolchain change) · BASE-12 (no other repository) · BASE-21, BASE-41 (no secrets) · BASE-27, BASE-30, BASE-31, BASE-39, BASE-40 (no shared data, containers, schema or outward writes) · BASE-28 (no third-party code added) · BASE-29 (no browser) · BASE-33, BASE-48 (no deployment/machine state claimed) · BASE-34 (nothing blocked) · BASE-36 (single app) · BASE-37 (not a bug fix) · BASE-42 (no new test runner) · BASE-43 (no access fix) · BASE-44 (no fake of another system changed) · BASE-45 (criteria in the spec, AC-W1 used as given) · BASE-47 (no computed value changed) · COST-2, COST-6, COST-7, COST-11, COST-12, COST-14, COST-17..21 (developer task, no UI sweep/e2e/CI/goldens/qc) · COST-4 (crate suite run because the brief's command names it) · COST-5 (no browser/server) · COST-15 (no goldens touched)
AC-W1 (dev part): pass — all pre-existing tests green unchanged; real TUI run quits normally; `step` drivable by any `Backend + DrawnCursor` and `Frontend` (FakeFrontend + TestBackend tests).
