---
has_ui: true            # a new window frontend: text rendering, input, IME
security_sensitive: false
approved: pending (builder check done 2026-11-28)
---

# Native window for vr-ide — design

Date: 2026-11-28. Status: draft, waiting for the user's approval.

## Intent and why

vr-ide is a ratatui TUI. On the desktop it runs inside GNOME Terminal (the `.desktop` entry runs
`gnome-terminal --title=vr-ide -- vr-ide --session`). The user wants:

1. vr-ide to run on its own, with no terminal emulator underneath, so it can later run the same
   way on Linux, macOS and Windows.
2. Full freedom over the keymap: today GNOME Terminal (VTE) keeps its own shortcuts and does not
   speak the kitty keyboard protocol, so chords like `ctrl+shift+<letter>`, `ctrl+/`, `ctrl+[`,
   `ctrl+h` vs Backspace are merged or lost (`event_loop::legacy_unreachable`).

## Goals

- `vr-ide` opens its own native window when a display is available; the whole IDE (editor,
  explorer, embedded terminals, AI panel, dialogs) works in it exactly as in the terminal.
- Every key chord the OS delivers to an application reaches vr-ide with full modifiers; the
  "keys this terminal cannot send" startup notice never appears in the window.
- Vietnamese input through the system input method (IBus/Fcitx with Bamboo/Unikey on Linux, the
  OS IME on macOS/Windows) works in the editor, prompts and embedded terminals, with the
  composing text shown at the cursor.
- The desktop launcher runs vr-ide directly; GNOME Terminal is no longer needed.
- The TUI stays: `vr-ide --tui`, and automatically when there is no display (SSH).

## Non-goals

- The Windows port itself (Unix-only code: `rustix`, process groups, 0600 files, PTY handling).
  It is a separate project after this one. This work only keeps the new window code free of
  Unix-only APIs so it does not add to that port.
- A general-purpose terminal application (`vr-terminal` as approach B). The name can be used for
  this window if the user wants.
- Ligatures, images in the terminal, tabs as OS windows, several windows per process.

## Anti-goals (failure even if it "works")

- Keys behaving differently between the window and `--tui` for chords both can deliver.
- Typing lag: a key press shows on screen within one frame (target ≤ 16 ms on this machine at
  its normal resolution; best effort, measured by the spike below).
- Broken Vietnamese: diacritics drawn in the wrong cell, double-typed letters with Telex, or the
  IME's composing text inserted twice.
- The window version needing GNOME or GTK to run.
- Losing unsaved work on a window close: closing the window goes through the same quit path as
  `ctrl+q` (dirty-buffer prompt, running-terminal prompt, recovery).

## Constraints

- One binary. The window is a cargo feature `window` (on by default), independent of
  `login-window`: it builds on a machine with no display and needs no GTK. "No GTK" is about the
  main window: the login window (separate process, feature `login-window`) still uses
  WebKitGTK on Linux; a build with `--no-default-features --features window` has no GTK at all.
- Linux: X11 and Wayland, no GTK/GNOME library for the main window.
- Dependencies: versions at least two weeks old, pinned as in run 5 (D74).
- The existing test suite (insta snapshots on `TestBackend`, `App::handle_key` tests) keeps
  working unchanged: the app core does not learn about the window.

## Approaches (the user picked A on 2026-11-27)

- **A. Own window, own ratatui backend (chosen).** `winit` window, a ratatui `Backend` that draws
  the cell grid into the window, keys taken straight from the OS. Most control, one app on every
  OS; we own text drawing, IME, clipboard, DPI.
- B. `vr-terminal`: a separate terminal emulator (winit + alacritty_terminal) running vr-ide via
  the kitty protocol. Rewrites a terminal, two layers, keys still pass through an encoding.
- C. Require a cross-platform terminal (WezTerm, Alacritty, kitty). Cheapest, keeps an external
  dependency and its shortcuts.

## How the code splits today (findings)

- The app core takes crossterm events: `App::handle_key(KeyEvent)`, `handle_mouse(MouseEvent)`,
  `handle_paste(&str)`, `on_resize()`; drawing is `ui::render(frame, app)`;
  `event_loop::run<B: Backend>` is already generic over the ratatui backend.
- Terminal-only calls are few and in two files: `main.rs` (raw mode, alternate screen, mouse
  capture, bracketed paste, title, kitty flags, `restore_terminal` in the panic hook) and
  `event_loop.rs` (cursor shape, title, `event::poll/read`). The clipboard falls back to OSC 52
  on stdout (`clipboard.rs`).
- Embedded terminals (vr-term, alacritty_terminal) are drawn as ratatui cells and encode keys to
  PTY bytes from `KeyEvent` — both unchanged by a new frontend.
- `tao` is already a dependency, but only for the login window (separate process, GTK on Linux).

## Decided parts of the how (designer's calls, for the user to check)

### Frontend split

- Threading (designer's call after review): winit owns the main thread (`ApplicationHandler`).
  `event_loop::run` is split into a `step(app, events, now)` function (handle the events,
  background pass, draw if needed, report the next wake time); the crossterm loop becomes
  `poll → read → step`, and the winit handler calls `step` from its callbacks. `App` stays on
  the main thread; background threads wake the loop through an `EventLoopProxy`. No `Send`
  requirement is added to `App`.

- A small `Frontend` seam in `vr-ide`: `set_title`, `set_cursor_shape`, `set_ime_position`,
  `wake` (from background threads), and the event source. Two implementations: the existing
  crossterm one (moved, not rewritten) and the window one in a new module `window/`.
- The window translates OS events into the same crossterm types the app already takes
  (`KeyEvent` with `KeyEventKind` and full modifiers, `MouseEvent` in cell coordinates, paste,
  resize). The app core, keymap parser, terminal key encoding and all tests stay as they are.
- In the window, `chord_from_key` runs with `enhanced = true`; `legacy_unreachable` warnings are
  not shown. The keymap defaults do not change: bindings on chords the TUI cannot deliver simply
  start working in the window; a chord both can deliver behaves the same in both.
- Bell and OSC 9/777 from embedded programs keep today's handling (AI attention mark and
  notice); in addition the window asks for attention (`request_user_attention`, the taskbar
  hint) when it is not focused.
- Embedded PTYs get the cell size in pixels with the cell grid on resize (`TIOCSWINSZ`
  `ws_xpixel`/`ws_ypixel`).

### Window and drawing

- Window: `winit` (X11 + Wayland on Linux, AppKit, Win32). Not `tao`: it pulls GTK on Linux,
  which is what the user wants to drop. The login window stays on tao/wry in its own process.
- Wayland on GNOME has no server-side title bars: winit's client-side decorations
  (`sctk-adwaita` feature) draw the title bar. X11 and Wayland are both supported; the app id /
  WM_CLASS is `vr-ide` so the desktop groups the window with its launcher.
- Drawing: CPU, `softbuffer` for the pixel buffer + `cosmic-text`/`swash` for shaping and glyph
  rasterising (font fallback, combining marks for Vietnamese, emoji, wide CJK cells). Glyphs are
  cached per (char, style, size). Only cells ratatui reports as changed are redrawn. A GPU path
  (wgpu) is not needed for a cell grid and adds driver risk; it can be added later behind the
  same backend if profiling asks for it.
- The grid: cell size from the font's advance and line height; the window size in pixels gives
  the grid size in cells; leftover pixels are padding in the background colour. HiDPI: scale
  factor from winit, font size in points × scale.
- Colours: ratatui `Color::Rgb` as is; the 16 named and 256 indexed colours from a palette in the
  theme (the current themes already use Rgb in most places).
- Cursor: block / bar / underline drawn by the backend, blinking like today's
  `BlinkingBar`/`SteadyBlock` choice.

### Input

- Keys: winit's logical key + physical key + modifiers → `KeyEvent`. For `ctrl`/`alt` chords
  the logical key is used when it is a Latin letter or ASCII symbol, else the physical key's US
  name (so chords work on non-Latin layouts, as kitty's alternate keys do); key repeat as
  `KeyEventKind::Repeat`, release as `Release` (as with the kitty flags today, so held quit/close
  keys still cannot confirm themselves).
- IME: enabled on the window; the composing (preedit) text is drawn underlined at the cursor cell
  and is not sent to the app until committed. While the IME is composing, raw key events are
  not sent; committed text goes in as one key event per character (so an embedded terminal
  gets its UTF-8 bytes and the editor's normal typing path runs). Without an active
  composition, keys go in as key events as usual. The IME candidate window is placed at the cursor
  via `set_ime_cursor_area` every frame the cursor moves.
- Mouse: pixel position → cell; buttons, drag, wheel (line steps, pixel deltas accumulated into
  lines).
- Clipboard: `arboard` as today (it does not need a terminal); the OSC 52 fallback is only for
  the TUI.
- Window close button = the `ctrl+q` quit path.

### Launch

- `vr-ide` (any arguments) opens the window when the `window` feature is built and a display is
  present (Linux: `WAYLAND_DISPLAY` or `DISPLAY` set; macOS/Windows: always). Otherwise it runs
  the TUI as today.
- `vr-ide --tui` forces the terminal UI.
- Started from a terminal, the window process stays in the foreground of that shell; the user
  adds `&` to keep the shell. The desktop launcher runs `vr-ide --session` directly.
- `--install-desktop` writes the new form: `Exec=<exe> --session`, `Terminal=false`,
  `StartupWMClass=vr-ide`, the same icon and marker. An existing entry that carries vr-ide's
  marker (`desktop::is_ours`) and still has the gnome-terminal form is rewritten on the first
  window start, with a notice saying so; an entry without the marker is never touched.
- A window close or SIGTERM while a dialog is open goes through the same quit path (the dialog
  is closed first).

### Settings

- `font_family` (default: the fontconfig `monospace` match on Linux, Menlo on macOS, Consolas on
  Windows; no bundled font), `font_size` (points, default 11), `line_height` (default 1.2). A
  family that is not found gives a warning and the default.
  Read on start and on settings reload; a change re-lays the grid.
- The theme gets an optional 16-colour palette; without it a built-in palette is used.

### Keys the OS keeps

- Keys the OS or desktop shell grabs before any app (`super`, `alt+tab`, GNOME's
  `ctrl+alt+arrows`, macOS `cmd+q`/`cmd+tab`) still cannot reach vr-ide. `docs/keymap.md` lists
  them per OS.

### Order of work

- First a spike task: open a winit window, draw a full screen of text with softbuffer +
  cosmic-text at the machine's resolution, measure full redraw and one-line change times, and
  type Vietnamese through IBus. If a full redraw cannot stay under ~16 ms, the plan switches the
  drawing to a GPU path before building on it.

## Left to the builder

- Exact crate versions (winit 0.30.x, softbuffer, cosmic-text) and their two-week age check.
- Glyph cache size, damage tracking details, frame pacing (draw on event or wake, no busy loop;
  background threads wake the event loop through an `EventLoopProxy`).
- How `poll_interval`'s fast polling for running programs maps to winit (`ControlFlow::WaitUntil`).
- Wide characters and combining marks in cells: follow ratatui's cell symbols (a cell may hold
  a grapheme cluster).
- Tests: unit tests for key translation (table of winit key → `KeyEvent`), mouse pixel → cell,
  grid size from window size, preedit handling; one headless render test that draws a small
  buffer into a pixel buffer and checks a few pixels. Manual check list for X11, Wayland and
  IBus-Bamboo Telex.
