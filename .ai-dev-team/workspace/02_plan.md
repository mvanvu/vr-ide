# Native window — implementation plan (run 6)

Spec: `.ai-dev-team/workspace/01_spec.md` (copy of `.ai-dev-team/specs/2026-11-28-native-window-design.md`, approved by the user 2026-11-28).

**Goal:** vr-ide opens its own winit window (CPU-drawn ratatui grid, OS keys, IME) with the TUI kept as `--tui` / no-display fallback.

**Architecture:** a new crate `crates/vr-window` holds everything that does not know `App`: the pixel renderer (a ratatui `Backend` drawing into an RGBA buffer with cosmic-text glyphs), the input translation (winit events → crossterm `KeyEvent` / `MouseEvent` / paste, IME preedit state) and the winit runner shell. `vr-ide` gets a frontend seam (`event_loop::step`, `Frontend` trait) and wires the window frontend behind the `window` feature.

**Tech:** winit 0.30.x, softbuffer, cosmic-text (swash), ratatui 0.30.2, crossterm 0.29 types. Versions ≥ 2 weeks old, pinned in `Cargo.lock` (D74 of run 5).

## Global constraints
- App core, keymap, terminal key encoding and all existing tests unchanged in behaviour (spec: Constraints).
- `vr-window` uses no Unix-only API (spec: Non-goals — keep the Windows port small).
- No GTK in the `window` feature; `login-window` stays as is.
- English code/docs; no AI attribution in commits.

## Review focus (inputs the spec implies, most likely to bite)
1. Wide (CJK) and combining (Vietnamese, emoji) graphemes in one ratatui cell symbol: drawn once, never shifting the grid — T2 test.
2. HiDPI scale change while running (moving the window between screens) — re-lays grid, no blurry/garbled cells — T2/T4 tests.
3. IME preedit then cancel (Esc) / commit with multiple characters / commit while a dialog is open — T3 tests.
4. Ctrl chords on a non-Latin layout and with Shift (ctrl+shift+a, ctrl+/, ctrl+[ vs Esc, ctrl+h vs Backspace, ctrl+i vs Tab) — T3 table test.
5. Closing the window with unsaved buffers / running terminals, and SIGTERM — goes through the ctrl+q path — T4 test.

## Tasks
- **T1 Frontend seam (vr-ide).** Split `event_loop::run` into `step(app, events, now) -> StepOutcome { quit, next_wake }`; `Frontend` trait (`set_title`, `set_cursor_shape`, `set_ime_area`, `request_attention`, `is_enhanced`); crossterm frontend = today's code moved. No behaviour change; existing tests green; new unit tests for `step`.
- **T2 Renderer (vr-window, new crate) + spike gate.** `PixelBackend` implementing ratatui `Backend` over an RGBA `Canvas`; `FontSet` (family, size pt, line height, scale) via cosmic-text with fallback; glyph cache; palette (16/256 + Rgb); cursor (block/bar/underline); preedit drawing hook; damage = changed cells only. Bench example `cargo run -p vr-window --example bench --release`: full redraw and one-line change at 1920×1080 and 3840×2160. **Gate:** full redraw > 16 ms at 1920×1080 → stop, report, plan switches to GPU (leader decision logged).
- **T3 Input (vr-window).** Pure translation functions: winit `KeyEvent` + modifiers → crossterm `KeyEvent` (kind press/repeat/release; logical vs physical rule from spec), mouse pixel → cell, wheel accumulation, IME state machine (preedit shown, raw keys suppressed while composing, commit → per-char key events). Table tests.
- **T4 Window app (vr-window runner + vr-ide wiring + vr-term pixel size + vr-config font settings).** `ApplicationHandler` with softbuffer surface, `EventLoopProxy` wake, `ControlFlow::WaitUntil(next_wake)`, title/cursor/IME area/attention via the `Frontend` trait, CSD on Wayland, app id `vr-ide`; launch choice (`window` feature + display → window; `--tui`; else TUI); close/SIGTERM → quit path; PTY `ws_xpixel/ws_ypixel`; settings `font_family`, `font_size`, `line_height` (+ reload); `chord_from_key(enhanced=true)`; no legacy-unreachable notice in the window. Integration test: headless where possible; manual check list.
- **T5 Desktop entry (vr-ide `desktop.rs`).** New form (`Exec=<exe> --session`, `Terminal=false`, `StartupWMClass=vr-ide`); `migrate_on_window_start() -> Option<String>` (rewrite only a marker-carrying gnome-terminal entry, returns the notice text). T4 calls it. High-risk (overwrites a user file).
- **T6 Docs.** README (requirements, window vs `--tui`, fonts, IME), `docs/keymap.md` (OS-reserved keys per OS, chords now available), `docs/commands.md` (`--tui`), settings docs.

Order: T1 ∥ T2 (different crates; T2 owns Cargo.toml/Cargo.lock). T3 after T2 (same crate). T5 after T1 committed (same crate vr-ide, disjoint file). T4 after T1, T2, T3, T5. T6 after T4.
