//! Integrated terminal panel (M5-2): toggling and focus (D8, D185), keys and paste reaching the
//! PTY, resize, the exited state, the quit guard and the panel rendering. Real terminals run
//! `/bin/sh`; the snapshots use a fixed fake grid.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use vr_term::{Cell, CellFlags, Color as TColor, Cursor, CursorShape, Grid, TermStatus};

use crate::app::{App, Focus};
use crate::app_tests::{app_with_file, test_app};
use crate::clipboard::{Clipboard, MemoryClipboard};
use crate::commands::CmdArgs;
use crate::commands::terminal::TermTab;

const WAIT: Duration = Duration::from_secs(10);

/// A test app whose terminals run `/bin/sh` with a short prompt, in a temp project.
pub(crate) fn term_app() -> (tempfile::TempDir, App, MemoryClipboard) {
    let dir = tempfile::tempdir().unwrap();
    let (mut app, clip) = test_app(dir.path());
    use_sh(&mut app);
    (dir, app, clip)
}

fn use_sh(app: &mut App) {
    app.terminal.shell = Some(PathBuf::from("/bin/sh"));
    app.terminal.env = vec![
        (OsString::from("PS1"), OsString::from("$ ")),
        (OsString::from("ENV"), OsString::from("/dev/null")),
    ];
}

pub(crate) fn draw(app: &mut App, w: u16, h: u16) -> Terminal<TestBackend> {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| crate::ui::render(f, app)).unwrap();
    t
}

fn key(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    app.handle_key(KeyEvent::new(code, mods));
}

/// ctrl+` as the kitty protocol reports it.
pub(crate) fn ctrl_backtick(app: &mut App) {
    key(app, KeyCode::Char('`'), KeyModifiers::CONTROL);
}

/// The NUL byte a legacy terminal sends for ctrl+` (crossterm reports it as ctrl+space).
fn nul(app: &mut App) {
    key(app, KeyCode::Char(' '), KeyModifiers::CONTROL);
}

/// The editor gets the keys and the panel stays shown, as a click on the editor does (ctrl+`
/// hides the panel since D280).
fn leave(app: &mut App) {
    app.run_command("focus.editor", CmdArgs::None);
    assert_ne!(app.focus, Focus::Terminal);
}

/// A left click (press and release) at screen cell (`column`, `row`).
pub(crate) fn click(app: &mut App, column: u16, row: u16) {
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        app.handle_mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        });
    }
}

/// Clicks the '+' of the terminal tab bar (D280).
fn click_plus(app: &mut App) {
    let bar = crate::ui::terminal_view::tab_bar(app, app.layout.terminal.width);
    let x = app.layout.terminal.x + bar.plus.expect("the + is drawn");
    click(app, x + 1, app.layout.terminal.y);
}

/// Clicks the 'x' of terminal `i`'s tab (D280).
fn click_close(app: &mut App, i: usize) {
    let bar = crate::ui::terminal_view::tab_bar(app, app.layout.terminal.width);
    let t = bar
        .tabs
        .iter()
        .find(|t| t.index == i)
        .expect("the tab is drawn");
    let x = app.layout.terminal.x + t.x + t.close.expect("its x is drawn");
    click(app, x, app.layout.terminal.y);
}

/// Hides the panel from the editor: shift+f12 (D250, D261).
fn hide_panel(app: &mut App) {
    crate::app_tests::press(app, "shift+f12");
}

pub(crate) fn typ(app: &mut App, s: &str) {
    for c in s.chars() {
        if c == '\n' {
            key(app, KeyCode::Enter, KeyModifiers::NONE);
        } else {
            key(app, KeyCode::Char(c), KeyModifiers::NONE);
        }
    }
}

pub(crate) fn tab(app: &App) -> &TermTab {
    &app.terminal.tabs[app.terminal.active]
}

/// Waits until the active terminal's screen satisfies `ok`; returns the screen.
pub(crate) fn wait_screen(app: &App, what: &str, ok: impl Fn(&str) -> bool) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        let s = tab(app).screen_text();
        if ok(&s) {
            return s;
        }
        assert!(
            Instant::now() < deadline,
            "waiting for {what}; screen:\n{s}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(crate) fn wait_text(app: &App, needle: &str) -> String {
    wait_screen(app, needle, |s| s.contains(needle))
}

fn wait_status(app: &App, want: TermStatus) {
    let deadline = Instant::now() + WAIT;
    while tab(app).status() != want {
        assert!(Instant::now() < deadline, "status {:?}", tab(app).status());
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(crate) fn frame_text(t: &Terminal<TestBackend>) -> String {
    let buf = t.backend().buffer();
    let mut out = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            out.push_str(buf[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

pub(crate) fn row_text(t: &Terminal<TestBackend>, y: u16) -> String {
    frame_text(t)
        .lines()
        .nth(usize::from(y))
        .unwrap_or("")
        .to_string()
}

fn proc_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

// ---- AC-1 -----------------------------------------------------------------------------------

#[test]
fn no_terminal_is_spawned_at_startup() {
    let (_d, mut app, _) = term_app();
    let t = draw(&mut app, 80, 24);
    assert!(app.terminal.tabs.is_empty());
    assert!(!app.terminal.visible);
    assert_eq!(app.layout.terminal, Rect::default());
    // The editor keeps every row between the tab bar and the status bar.
    assert_eq!(app.layout.editor.height, 22);
    assert!(!frame_text(&t).contains("TERM"));
}

// ---- toggle state machine (AC-14a, D280) ------------------------------------------------------

/// D280: ctrl+` shows and focuses a hidden panel (a first terminal starts when there is none)
/// and hides a shown one, whatever has the keys; the terminals keep running.
#[test]
fn ctrl_backtick_shows_and_hides_the_panel_wherever_the_keys_are() {
    let (_d, mut app, _) = term_app();
    app.keyboard_enhanced = true;
    draw(&mut app, 80, 24);
    // Hidden → shown, focused, one terminal started.
    ctrl_backtick(&mut app);
    assert!(app.terminal.visible);
    assert_eq!(app.focus, Focus::Terminal);
    assert_eq!(app.terminal.tabs.len(), 1);
    let pid = tab(&app).pid().expect("running shell");
    // Shown and focused → hidden, the editor gets the keys, the shell runs on.
    ctrl_backtick(&mut app);
    assert!(!app.terminal.visible);
    assert_eq!(app.focus, Focus::Editor);
    assert_eq!(tab(&app).pid(), Some(pid));
    // Hidden again → shown and focused, same terminal (none created).
    ctrl_backtick(&mut app);
    assert!(app.terminal.visible);
    assert_eq!(app.focus, Focus::Terminal);
    assert_eq!(app.terminal.tabs.len(), 1);
    assert_eq!(tab(&app).pid(), Some(pid));
    // Shown with the editor focused → hidden (no focus step first).
    leave(&mut app);
    assert!(app.terminal.visible);
    assert!(panel_bar(&mut app).contains("ctrl+` hide"));
    ctrl_backtick(&mut app);
    assert!(!app.terminal.visible);
    assert_eq!(app.focus, Focus::Editor);
    // Shown with the explorer focused → hidden; the explorer keeps the keys.
    ctrl_backtick(&mut app);
    app.explorer.visible = true;
    app.focus = Focus::Explorer;
    ctrl_backtick(&mut app);
    assert!(!app.terminal.visible);
    assert_eq!(app.focus, Focus::Explorer);
    assert_eq!(app.terminal.tabs.len(), 1);
    assert_eq!(tab(&app).pid(), Some(pid));
    // shift+f12 from the editor still hides.
    ctrl_backtick(&mut app);
    leave(&mut app);
    hide_panel(&mut app);
    assert!(!app.terminal.visible);
}

/// The panel's tab bar row as drawn at 80x24.
fn panel_bar(app: &mut App) -> String {
    let t = draw(app, 80, 24);
    row_text(&t, app.layout.terminal.y)
}

/// Legacy terminals send ctrl+` as NUL: the same show / hide from the editor, the explorer and a
/// focused terminal (D214, D280).
#[test]
fn legacy_nul_shows_and_hides_from_everywhere() {
    let (_d, mut app, _) = term_app();
    app.keyboard_enhanced = false;
    draw(&mut app, 80, 24);
    nul(&mut app);
    assert!(app.terminal.visible);
    assert_eq!(app.focus, Focus::Terminal);
    let pid = tab(&app).pid().expect("running shell");
    // The hint promises what NUL does in here.
    let bar = panel_bar(&mut app);
    assert!(bar.contains("ctrl+` hide"), "{bar}");
    // Inside a focused terminal NUL hides the panel; the shell runs.
    nul(&mut app);
    assert!(!app.terminal.visible);
    assert_eq!(app.focus, Focus::Editor);
    assert_eq!(tab(&app).pid(), Some(pid));
    // From the editor with the panel shown: hidden.
    nul(&mut app);
    leave(&mut app);
    nul(&mut app);
    assert!(!app.terminal.visible);
    // From the explorer: shown, then hidden.
    app.explorer.visible = true;
    app.focus = Focus::Explorer;
    nul(&mut app);
    assert!(app.terminal.visible);
    assert_eq!(app.focus, Focus::Terminal);
    app.focus = Focus::Explorer;
    nul(&mut app);
    assert!(!app.terminal.visible);
    assert_eq!(tab(&app).pid(), Some(pid));
}

/// D280: with every terminal closed, ctrl+` opens a first one again.
#[test]
fn ctrl_backtick_opens_a_first_terminal_after_the_last_was_closed() {
    let (_d, mut app, _) = term_app();
    app.keyboard_enhanced = true;
    draw(&mut app, 80, 24);
    ctrl_backtick(&mut app);
    draw(&mut app, 80, 24);
    click_close(&mut app, 0);
    assert!(app.terminal.tabs.is_empty());
    assert!(!app.terminal.visible);
    ctrl_backtick(&mut app);
    assert_eq!(app.terminal.tabs.len(), 1);
    assert!(app.terminal.visible);
    assert_eq!(app.focus, Focus::Terminal);
}

/// D279: F1 opens the command window from a focused terminal (legacy ESC O P and kitty); Esc
/// gives the keys back to the terminal. Other F keys still reach the program.
#[test]
fn f1_opens_the_command_window_from_a_focused_terminal() {
    for enhanced in [false, true] {
        let (_d, mut app, _) = term_app();
        app.keyboard_enhanced = enhanced;
        draw(&mut app, 80, 24);
        ctrl_backtick(&mut app);
        assert_eq!(app.focus, Focus::Terminal);
        let ev = if enhanced {
            crate::app_tests::kitty_event("f1")
        } else {
            let (bytes, ev) = crate::app_tests::legacy_event("f1");
            assert_eq!(bytes, b"\x1bOP");
            ev.unwrap()
        };
        app.ran.clear();
        app.handle_key(ev);
        assert_eq!(app.ran, ["app.palette"], "kitty {enhanced}");
        assert!(app.palette.is_some());
        assert_eq!(app.focus, Focus::Palette);
        key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.palette.is_none());
        assert_eq!(app.focus, Focus::Terminal, "back to the terminal");
        // f2 is the program's.
        app.ran.clear();
        key(&mut app, KeyCode::F(2), KeyModifiers::NONE);
        assert!(app.ran.is_empty(), "{:?}", app.ran);
        assert_eq!(app.focus, Focus::Terminal);
    }
}

#[test]
fn terminal_pass_nul_sends_nul_to_the_program() {
    let (_d, mut app, _) = term_app();
    app.keyboard_enhanced = false;
    app.settings.terminal_pass_nul = true;
    draw(&mut app, 80, 24);
    nul(&mut app);
    assert!(app.terminal.visible);
    // Inside a focused terminal it goes to the program: cat -v shows it as ^@.
    wait_text(&app, "$");
    typ(&mut app, "cat -v\n");
    wait_text(&app, "cat -v");
    nul(&mut app);
    typ(&mut app, "x\n");
    wait_text(&app, "^@x");
    assert!(app.terminal.visible);
    assert_eq!(app.focus, Focus::Terminal);
    // The hint does not promise a hide key that would not work here.
    let bar = panel_bar(&mut app);
    assert!(bar.contains("click editor"), "{bar}");
    assert!(!bar.contains("hide"), "{bar}");
}

#[test]
fn kitty_ctrl_backtick_leaves_and_ctrl_space_reaches_the_program() {
    let (_d, mut app, _) = term_app();
    app.keyboard_enhanced = true;
    draw(&mut app, 80, 24);
    ctrl_backtick(&mut app);
    wait_text(&app, "$");
    typ(&mut app, "cat -v\n");
    wait_text(&app, "cat -v");
    // ctrl+space is its own key with the kitty protocol: the program gets it.
    nul(&mut app);
    typ(&mut app, "y\n");
    wait_text(&app, "^@y");
    assert!(app.terminal.visible);
    assert!(panel_bar(&mut app).contains("ctrl+` hide"));
    ctrl_backtick(&mut app);
    assert!(!app.terminal.visible, "ctrl+` hides the panel (D280)");
    assert_eq!(app.focus, Focus::Editor);
}

#[test]
fn legacy_nul_in_the_palette_does_not_toggle() {
    let (_d, mut app, _) = term_app();
    draw(&mut app, 80, 24);
    app.run_command("app.palette", CmdArgs::None);
    assert_eq!(app.focus, Focus::Palette);
    nul(&mut app);
    assert!(!app.terminal.visible);
    assert!(app.terminal.tabs.is_empty());
}

// ---- keys and paste reach the PTY, never the editor (AC-14a) -----------------------------------

#[test]
fn keys_typed_into_a_focused_terminal_never_reach_the_editor() {
    let (_d, path, mut app, _) = app_with_file("abc\n");
    use_sh(&mut app);
    app.keyboard_enhanced = true;
    draw(&mut app, 80, 24);
    assert!(app.edit_mode());
    ctrl_backtick(&mut app);
    typ(&mut app, "echo vr-$((6*7))\n");
    wait_text(&app, "vr-42");
    // Global and editor chords go to the program as well (ctrl+s, ctrl+w, Esc, ctrl+z).
    for c in ['s', 'w', 'z', 'a'] {
        key(&mut app, KeyCode::Char(c), KeyModifiers::CONTROL);
    }
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.focus, Focus::Terminal);
    assert_eq!(app.docs.len(), 1, "ctrl+w closed the file");
    let doc = app.active_doc().unwrap();
    assert_eq!(doc.buffer.text().to_string(), "abc\n");
    assert!(!doc.is_modified());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "abc\n");
    // Read mode (T14) does not apply inside the terminal: back in the editor the tab is read.
    leave(&mut app);
    assert!(!app.edit_mode());
}

#[test]
fn paste_goes_to_the_terminal_bracketed_when_the_program_asks() {
    let (_d, mut app, clip) = term_app();
    app.keyboard_enhanced = true;
    draw(&mut app, 80, 24);
    ctrl_backtick(&mut app);
    typ(&mut app, "printf '\\033[?2004h'; cat -v\n");
    let deadline = Instant::now() + WAIT;
    while !tab(&app).grid_now().modes.bracketed_paste {
        assert!(Instant::now() < deadline, "bracketed paste never enabled");
        std::thread::sleep(Duration::from_millis(10));
    }
    app.handle_paste("hi");
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    wait_text(&app, "^[[200~hi^[[201~");
    // ctrl+shift+v pastes the clipboard the same way.
    let mut c = clip.clone();
    c.set("yo").unwrap();
    key(
        &mut app,
        KeyCode::Char('V'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    wait_text(&app, "^[[200~yo^[[201~");
    assert!(app.docs.is_empty());
}

// ---- resize, scrollback --------------------------------------------------------------------

#[test]
fn the_terminal_follows_the_panel_size() {
    let (_d, mut app, _) = term_app();
    app.keyboard_enhanced = true;
    ctrl_backtick(&mut app);
    for (w, h) in [(80, 24), (120, 40), (40, 12)] {
        draw(&mut app, w, h);
        let area = app.layout.terminal;
        assert!(area.height >= 5, "{area:?}");
        assert_eq!(area.width, w);
        let want = format!("{} {}", area.height - 1, area.width);
        assert_eq!(
            tab(&app).size(),
            (area.width, area.height - 1),
            "pty size at {w}x{h}"
        );
        typ(&mut app, "clear; stty size\n");
        wait_text(&app, &want);
    }
    // Default height: 40 % of the screen, at least 5 rows.
    draw(&mut app, 80, 24);
    assert_eq!(app.layout.terminal.height, 10);
    draw(&mut app, 40, 12);
    assert_eq!(app.layout.terminal.height, 5);
}

#[test]
fn shift_pageup_scrolls_back_and_a_key_returns_to_the_bottom() {
    let (_d, mut app, _) = term_app();
    app.keyboard_enhanced = true;
    draw(&mut app, 80, 24);
    ctrl_backtick(&mut app);
    draw(&mut app, 80, 24);
    typ(&mut app, "seq 1 300\n");
    wait_screen(&app, "300 then a prompt", |s| s.contains("300\n$"));
    key(&mut app, KeyCode::PageUp, KeyModifiers::SHIFT);
    assert!(tab(&app).scroll > 0);
    let t = draw(&mut app, 80, 24);
    let text = frame_text(&t);
    assert!(!text.contains("300"), "{text}");
    assert!(text.contains('↑'), "no scrollback indicator:\n{text}");
    key(&mut app, KeyCode::PageDown, KeyModifiers::SHIFT);
    assert_eq!(tab(&app).scroll, 0);
    key(&mut app, KeyCode::PageUp, KeyModifiers::SHIFT);
    key(&mut app, KeyCode::PageUp, KeyModifiers::SHIFT);
    assert!(tab(&app).scroll > 0);
    // Any key returns to the bottom.
    typ(&mut app, "x");
    assert_eq!(tab(&app).scroll, 0);
}

/// Starts a shell loop in the active terminal that copies whatever the test writes into a FIFO
/// to the screen, so the test controls the program's output byte for byte. Returns the FIFO.
fn fifo_program(app: &mut App, dir: &Path, history: usize) -> PathBuf {
    let fifo = dir.join("out.fifo");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(made.success());
    typ(
        app,
        &format!(
            "seq 1 {history}; while :; do cat {}; done\n",
            fifo.display()
        ),
    );
    let last = history.to_string();
    wait_screen(app, "the history", |s| {
        s.lines().any(|l| l == last.as_str())
    });
    fifo
}

/// Writes `bytes` as the program's output and waits until the emulator has pushed at least
/// `pushed` more lines into the history.
fn emit(app: &App, fifo: &Path, bytes: &[u8], pushed: usize) {
    use std::io::Write;
    let before = tab(app).grid_now().scrollback_len;
    let mut f = std::fs::OpenOptions::new().write(true).open(fifo).unwrap();
    f.write_all(bytes).unwrap();
    drop(f);
    let deadline = Instant::now() + WAIT;
    while tab(app).grid_now().scrollback_len < before + pushed {
        assert!(Instant::now() < deadline, "output not seen");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// What an Ink-style agent (`claude`) writes: per frame a synchronized update that erases the
/// two-line live region (cursor up + erase line), prints one finished line, which scrolls into
/// the history, and redraws the live region; bracketed paste is toggled on the way.
fn ink_frames(frames: usize) -> Vec<u8> {
    let mut out = String::from("\x1b[?2004h\x1b[?25l");
    for f in 0..frames {
        out.push_str("\x1b[?2026h");
        if f > 0 {
            out.push_str("\r\x1b[2K\x1b[1A\x1b[2K\r");
        }
        out.push_str(&format!("done {f}\nspinner {f}\nstatus {f}"));
        out.push_str("\x1b[?2026l");
    }
    out.push_str("\r\n\x1b[?25h");
    out.into_bytes()
}

/// AC-F2: while a program streams output (an Ink-style redraw loop, then a flood), a view
/// scrolled back with shift+pageup stays on the same lines; new output does not pull it down
/// and the offset grows with the lines pushed into the history. A key returns to the bottom.
#[test]
fn scrolled_view_holds_while_a_program_streams_output() {
    let (d, mut app, _) = term_app();
    app.keyboard_enhanced = true;
    draw(&mut app, 80, 24);
    ctrl_backtick(&mut app);
    draw(&mut app, 80, 24);
    let fifo = fifo_program(&mut app, d.path(), 300);
    key(&mut app, KeyCode::PageUp, KeyModifiers::SHIFT);
    crate::commands::terminal::refresh(&mut app);
    draw(&mut app, 80, 24);
    let held = tab(&app).screen_text();
    let offset = tab(&app).scroll;
    assert!(offset > 0);

    let history = tab(&app).grid.scrollback_len;
    emit(&app, &fifo, &ink_frames(40), 40);
    crate::commands::terminal::refresh(&mut app);
    let t = draw(&mut app, 80, 24);
    assert_eq!(tab(&app).screen_text(), held, "view moved during redraws");
    assert!(frame_text(&t).contains('↑'), "{}", frame_text(&t));
    let grown = tab(&app).grid.scrollback_len - history;
    assert_eq!(tab(&app).scroll, offset + grown);

    let flood: String = (0..2000).map(|_| "y\n").collect();
    emit(&app, &fifo, flood.as_bytes(), 2000);
    crate::commands::terminal::refresh(&mut app);
    draw(&mut app, 80, 24);
    assert_eq!(tab(&app).screen_text(), held, "view moved during a flood");

    // Any key returns to the bottom and output is followed again.
    typ(&mut app, "x");
    assert_eq!(tab(&app).scroll, 0);
    emit(&app, &fifo, b"after 1\nafter 2\n", 2);
    crate::commands::terminal::refresh(&mut app);
    draw(&mut app, 80, 24);
    assert_eq!(tab(&app).scroll, 0);
    assert!(tab(&app).screen_text().contains("after 2"));
}

/// AC-F2 with the mouse wheel: a wheel scroll back holds while output arrives, and wheeling
/// down to the bottom follows the output again.
#[test]
fn wheel_scrolled_view_holds_while_output_arrives() {
    let (d, mut app, _) = term_app();
    app.keyboard_enhanced = true;
    draw(&mut app, 80, 24);
    ctrl_backtick(&mut app);
    draw(&mut app, 80, 24);
    let fifo = fifo_program(&mut app, d.path(), 300);
    let area = app.layout.terminal;
    let wheel = |app: &mut App, kind: MouseEventKind| {
        app.handle_mouse(MouseEvent {
            kind,
            column: area.x + 2,
            row: area.y + 2,
            modifiers: KeyModifiers::NONE,
        });
    };
    for _ in 0..3 {
        wheel(&mut app, MouseEventKind::ScrollUp);
    }
    crate::commands::terminal::refresh(&mut app);
    draw(&mut app, 80, 24);
    assert_eq!(tab(&app).scroll, 9);
    let held = tab(&app).screen_text();

    emit(&app, &fifo, &ink_frames(25), 25);
    crate::commands::terminal::refresh(&mut app);
    draw(&mut app, 80, 24);
    assert_eq!(tab(&app).screen_text(), held, "view moved");
    assert!(tab(&app).scroll >= 9 + 25);

    // Wheel down the whole way: back at the bottom, new output is followed.
    for _ in 0..30 {
        wheel(&mut app, MouseEventKind::ScrollDown);
    }
    assert_eq!(tab(&app).scroll, 0);
    emit(&app, &fifo, b"tail line\n", 1);
    crate::commands::terminal::refresh(&mut app);
    draw(&mut app, 80, 24);
    assert_eq!(tab(&app).scroll, 0);
    assert!(tab(&app).screen_text().contains("tail line"));
}

// ---- tabs, exited state ----------------------------------------------------------------------

/// D280 / D291: the tab bar's '+' opens a terminal, each tab's 'x' closes that one (its shell
/// hung up, no question); f8 / shift+f8 step; ctrl+f12 and ctrl+f4 do nothing any more.
#[test]
fn plus_and_x_open_and_close_terminals_and_f8_steps() {
    let (_d, mut app, _) = term_app();
    app.keyboard_enhanced = true;
    draw(&mut app, 80, 24);
    for gone in ["ctrl+f12", "ctrl+f4"] {
        crate::app_tests::press(&mut app, gone);
        assert!(app.terminal.tabs.is_empty(), "{gone}");
        assert!(!app.terminal.visible, "{gone}");
    }
    ctrl_backtick(&mut app);
    assert_eq!(app.terminal.tabs.len(), 1);
    let bar = panel_bar(&mut app);
    assert!(bar.contains(" 1 sh x ") && bar.contains(" + "), "{bar}");
    // '+' from the editor too: a second terminal, active, with the keys.
    leave(&mut app);
    click_plus(&mut app);
    assert_eq!(app.terminal.tabs.len(), 2);
    assert_eq!(app.terminal.active, 1);
    assert_eq!(app.focus, Focus::Terminal);
    click_plus(&mut app);
    assert_eq!(app.terminal.tabs.len(), 3);
    assert_eq!(app.terminal.active, 2);
    let bar = panel_bar(&mut app);
    assert!(
        bar.contains("1 sh") && bar.contains("2 sh") && bar.contains("3 sh"),
        "{bar}"
    );
    app.run_command("terminal.prev", CmdArgs::None);
    assert_eq!(app.terminal.active, 1);
    app.run_command("terminal.next", CmdArgs::None);
    assert_eq!(app.terminal.active, 2);
    app.run_command("terminal.next", CmdArgs::None);
    assert_eq!(app.terminal.active, 0);
    // The 'x' of an inactive tab closes that one; the active terminal stays active.
    let first = app.terminal.tabs[0].pid().unwrap();
    let second = app.terminal.tabs[1].pid().unwrap();
    app.run_command("terminal.next", CmdArgs::None);
    assert_eq!(app.terminal.active, 1);
    draw(&mut app, 80, 24);
    click_close(&mut app, 0);
    assert_eq!(app.terminal.tabs.len(), 2);
    assert_eq!(app.terminal.active, 0);
    assert_eq!(
        tab(&app).pid(),
        Some(second),
        "the same terminal stays active"
    );
    let deadline = Instant::now() + WAIT;
    while proc_alive(first) {
        assert!(
            Instant::now() < deadline,
            "closed terminal's shell still alive"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    // The 'x' of the active (first) tab: the next one becomes active.
    draw(&mut app, 80, 24);
    click_close(&mut app, 0);
    assert_eq!(app.terminal.tabs.len(), 1);
    assert_eq!(app.terminal.active, 0);
    assert_ne!(tab(&app).pid(), Some(second));
    // A click on the label (not its 'x') only activates.
    draw(&mut app, 80, 24);
    let (x, y) = (app.layout.terminal.x, app.layout.terminal.y);
    click(&mut app, x + 2, y);
    assert_eq!(app.terminal.tabs.len(), 1);
    // Closing the last one hides the panel and gives the editor the keys.
    draw(&mut app, 80, 24);
    click_close(&mut app, 0);
    assert!(app.terminal.tabs.is_empty());
    assert!(!app.terminal.visible);
    assert_eq!(app.focus, Focus::Editor);
}

/// Runs `cat` (job control on, so it takes the foreground) in the first terminal and waits for it.
fn start_cat(app: &mut App) {
    app.keyboard_enhanced = true;
    draw(app, 80, 24);
    ctrl_backtick(app);
    typ(app, "set -m; cat\n");
    let deadline = Instant::now() + WAIT;
    while tab(app).shell_in_foreground() {
        assert!(Instant::now() < deadline, "cat never took the terminal");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// D296: the 'x' asks "Close terminal?" while a program holds the terminal; Cancel (default,
/// Esc) keeps it running, Close ends it.
#[test]
fn x_asks_while_a_program_holds_the_terminal() {
    let (_d, mut app, _) = term_app();
    start_cat(&mut app);
    let pid = tab(&app).pid().unwrap();
    assert_eq!(tab(&app).foreground_name().as_deref(), Some("cat"));
    draw(&mut app, 80, 24);
    click_close(&mut app, 0);
    let d = app.dialog.as_ref().expect("the question");
    assert!(matches!(
        d.action,
        crate::commands::dialog::DialogAction::CloseTerminal(Some(p)) if p == pid
    ));
    assert_eq!(app.terminal.tabs.len(), 1);
    let t = draw(&mut app, 80, 24);
    let text = frame_text(&t);
    assert!(
        text.contains("Close terminal?") && text.contains("cat is running"),
        "{text}"
    );
    // Enter on the default button is Cancel.
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.dialog.is_none());
    assert_eq!(app.terminal.tabs.len(), 1);
    assert!(proc_alive(pid));
    // Esc is Cancel too.
    click_close(&mut app, 0);
    assert!(app.dialog.is_some());
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert_eq!(app.terminal.tabs.len(), 1);
    // Close: Left to the first button, Enter.
    click_close(&mut app, 0);
    key(&mut app, KeyCode::Left, KeyModifiers::NONE);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.terminal.tabs.is_empty());
    assert!(!app.terminal.visible);
    let deadline = Instant::now() + WAIT;
    while proc_alive(pid) {
        assert!(Instant::now() < deadline, "closed terminal's shell alive");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// D296: an idle shell and an exited terminal close at once, with no question.
#[test]
fn x_closes_an_idle_shell_and_an_exited_terminal_at_once() {
    let (_d, mut app, _) = term_app();
    app.keyboard_enhanced = true;
    draw(&mut app, 80, 24);
    ctrl_backtick(&mut app);
    leave(&mut app);
    draw(&mut app, 80, 24);
    click_plus(&mut app);
    assert_eq!(app.terminal.tabs.len(), 2);
    typ(&mut app, "exit 2\n");
    wait_status(&app, TermStatus::Exited(2));
    draw(&mut app, 80, 24);
    click_close(&mut app, 1);
    assert!(app.dialog.is_none());
    assert_eq!(app.terminal.tabs.len(), 1);
    // The idle shell (first tab), once it is up.
    let deadline = Instant::now() + WAIT;
    while !tab(&app).shell_in_foreground() {
        assert!(Instant::now() < deadline, "shell never in the foreground");
        std::thread::sleep(Duration::from_millis(10));
    }
    click_close(&mut app, 0);
    assert!(app.dialog.is_none());
    assert!(app.terminal.tabs.is_empty());
}

/// D296: the question at 80x24.
#[test]
fn render_close_terminal_dialog_80x24() {
    let (_d, _p, mut app, _) = app_with_file("fn main() {}\n");
    use_sh(&mut app);
    app.terminal.tabs.push(fake_tab("sh", TermStatus::Running));
    app.terminal.visible = true;
    draw(&mut app, 80, 24);
    click_close(&mut app, 0);
    assert!(
        app.dialog.is_some(),
        "a running tab that cannot be inspected asks"
    );
    let t = draw(&mut app, 80, 24);
    insta::assert_snapshot!("render_close_terminal_dialog_80x24", t.backend());
}

#[test]
fn grow_and_shrink_step_the_panel_height() {
    let (_d, mut app, _) = term_app();
    // Grow / shrink change the height in 5 % steps within 10..=90 %.
    app.run_command("terminal.grow", CmdArgs::None);
    assert_eq!(app.terminal.height_percent, 45);
    for _ in 0..20 {
        app.run_command("terminal.grow", CmdArgs::None);
    }
    assert_eq!(app.terminal.height_percent, 90);
    for _ in 0..20 {
        app.run_command("terminal.shrink", CmdArgs::None);
    }
    assert_eq!(app.terminal.height_percent, 10);
}

#[test]
fn inactive_tab_titles_are_fresh_when_the_panel_is_shown_again() {
    let (_d, mut app, _) = term_app();
    app.keyboard_enhanced = true;
    draw(&mut app, 80, 24);
    ctrl_backtick(&mut app);
    wait_text(&app, "$");
    // Tab 1 sets its title a moment later, while tab 2 is active and the panel is hidden.
    typ(&mut app, "sleep 0.3; printf '\\033]2;late-title\\007'\n");
    crate::commands::terminal::new_terminal(&mut app);
    leave(&mut app);
    hide_panel(&mut app);
    assert!(!app.terminal.visible);
    let deadline = Instant::now() + WAIT;
    while app.terminal.tabs[0].grid_now().title.as_deref() != Some("late-title") {
        assert!(Instant::now() < deadline, "title never set");
        std::thread::sleep(Duration::from_millis(10));
    }
    // The loop saw the wake while hidden (no snapshot then).
    crate::commands::terminal::refresh(&mut app);
    ctrl_backtick(&mut app);
    let bar = panel_bar(&mut app);
    assert!(bar.contains("1 late-title"), "{bar}");
}

#[test]
fn an_exited_terminal_shows_the_banner_and_enter_restarts_it() {
    let (dir, mut app, _) = term_app();
    app.keyboard_enhanced = true;
    draw(&mut app, 80, 24);
    ctrl_backtick(&mut app);
    // AC-14b: the shell starts in the project root.
    typ(&mut app, "pwd\n");
    let root = dir.path().canonicalize().unwrap();
    wait_text(&app, &root.display().to_string());
    let pid = tab(&app).pid().unwrap();
    typ(&mut app, "exit 3\n");
    wait_status(&app, TermStatus::Exited(3));
    let t = draw(&mut app, 80, 24);
    let text = frame_text(&t);
    assert!(
        text.contains("[process exited 3] — Enter to restart, ctrl+w to close"),
        "{text}"
    );
    let bar = row_text(&t, app.layout.terminal.y);
    assert!(bar.contains("[3]"), "exit marker in the tab bar: {bar}");
    // Other keys do nothing; Enter restarts in the same tab.
    typ(&mut app, "x");
    assert_eq!(tab(&app).status(), TermStatus::Exited(3));
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.terminal.tabs.len(), 1);
    assert_eq!(tab(&app).status(), TermStatus::Running);
    assert_ne!(tab(&app).pid(), Some(pid));
    typ(&mut app, "exit\n");
    wait_status(&app, TermStatus::Exited(0));
    // ctrl+w closes the exited terminal.
    key(&mut app, KeyCode::Char('w'), KeyModifiers::CONTROL);
    assert!(app.terminal.tabs.is_empty());
    assert!(!app.terminal.visible);
    assert_eq!(app.focus, Focus::Editor);
}

// ---- quit guard (AC-14d) ---------------------------------------------------------------------

#[test]
fn quit_asks_while_terminals_run_and_quitting_kills_them() {
    let (_d, mut app, _) = term_app();
    app.keyboard_enhanced = true;
    draw(&mut app, 80, 24);
    ctrl_backtick(&mut app);
    // A program running in the foreground (its own job's process group) ends with the shell:
    // the hangup reaches the terminal's foreground group too.
    let pid = tab(&app).pid().unwrap();
    typ(&mut app, "sleep 100\n");
    let child = {
        let deadline = Instant::now() + WAIT;
        loop {
            let kids = std::fs::read_to_string(format!("/proc/{pid}/task/{pid}/children"))
                .unwrap_or_default();
            if let Some(k) = kids.split_whitespace().next() {
                break k.parse::<u32>().unwrap();
            }
            assert!(Instant::now() < deadline, "sleep never started");
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    leave(&mut app);
    key(&mut app, KeyCode::Char('q'), KeyModifiers::CONTROL);
    let d = app.dialog.as_ref().expect("quit guard");
    let crate::ui::dialog::DialogWidget::Confirm(c) = &d.widget else {
        panic!("not a confirm dialog");
    };
    assert!(
        c.message.contains("1 terminal still running"),
        "{}",
        c.message
    );
    assert_eq!(c.buttons, ["Quit anyway", "Cancel"]);
    // Default Cancel.
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.dialog.is_none());
    assert!(!app.quit);
    // Quit anyway quits.
    key(&mut app, KeyCode::Char('q'), KeyModifiers::CONTROL);
    key(&mut app, KeyCode::Left, KeyModifiers::NONE);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.quit);
    crate::commands::terminal::shutdown(&mut app);
    assert!(app.terminal.tabs.is_empty());
    assert!(!proc_alive(pid), "shell {pid} survived the quit");
    let deadline = Instant::now() + WAIT;
    while proc_alive(child) {
        if Instant::now() > deadline {
            // Do not leave it behind for the next run, then fail.
            let _ = std::process::Command::new("kill")
                .arg(child.to_string())
                .status();
            panic!("foreground program {child} survived the quit");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn quit_guard_then_unsaved_files_still_ask() {
    let (_d, _p, mut app, _) = app_with_file("abc\n");
    use_sh(&mut app);
    app.keyboard_enhanced = true;
    typ(&mut app, "x");
    ctrl_backtick(&mut app);
    leave(&mut app);
    key(&mut app, KeyCode::Char('q'), KeyModifiers::CONTROL);
    key(&mut app, KeyCode::Left, KeyModifiers::NONE);
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    // Quit anyway on the terminals leads to the unsaved-files question, not straight out.
    assert!(!app.quit);
    let d = app.dialog.as_ref().expect("unsaved files dialog");
    assert!(matches!(
        d.action,
        crate::commands::dialog::DialogAction::Quit
    ));
    // Cancelling it and quitting again asks about the terminals again.
    key(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    key(&mut app, KeyCode::Char('q'), KeyModifiers::CONTROL);
    let d = app.dialog.as_ref().expect("terminal quit guard again");
    assert!(matches!(
        d.action,
        crate::commands::dialog::DialogAction::QuitTerminals
    ));
}

// ---- status bar, mouse -----------------------------------------------------------------------

#[test]
fn the_status_bar_shows_term_while_the_terminal_has_the_keys() {
    let (_d, _p, mut app, _) = app_with_file("abc\n");
    use_sh(&mut app);
    app.keyboard_enhanced = true;
    ctrl_backtick(&mut app);
    let t = draw(&mut app, 80, 24);
    let status = row_text(&t, 23);
    assert!(status.starts_with(" TERM "), "{status}");
    assert!(!status.contains("READ") && !status.contains("EDIT"));
    leave(&mut app);
    let t = draw(&mut app, 80, 24);
    assert!(row_text(&t, 23).starts_with(" READ "));
}

#[test]
fn a_click_dismisses_a_notice_while_the_terminal_has_the_keys() {
    let (_d, mut app, _) = term_app();
    app.terminal.tabs.push(fake_tab("sh", TermStatus::Running));
    app.terminal.visible = true;
    app.focus = Focus::Terminal;
    app.warn("something to read");
    let t = draw(&mut app, 80, 24);
    let notice = app.layout.notice;
    assert!(notice.height > 0);
    // Esc belongs to the program; the notice says how to dismiss it instead.
    let row = row_text(&t, notice.y);
    assert!(row.contains("something to read  (click)"), "{row}");
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: notice.x + 3,
        row: notice.y,
        modifiers: KeyModifiers::NONE,
    });
    assert!(app.notices.is_empty());
    // The keys stay with the terminal.
    assert_eq!(app.focus, Focus::Terminal);
}

#[test]
fn a_program_drag_released_outside_the_panel_still_gets_the_release() {
    let (_d, mut app, _) = term_app();
    app.keyboard_enhanced = true;
    draw(&mut app, 80, 24);
    ctrl_backtick(&mut app);
    draw(&mut app, 80, 24);
    wait_text(&app, "$");
    typ(&mut app, "printf '\\033[?1002h\\033[?1006h'; cat -v\n");
    let deadline = Instant::now() + WAIT;
    while !tab(&app).grid_now().modes.mouse_reporting {
        assert!(Instant::now() < deadline, "mouse reporting never enabled");
        std::thread::sleep(Duration::from_millis(10));
    }
    let area = app.layout.terminal;
    let mouse = |kind, column, row| MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    // Press on grid cell (row 1, col 2), drag up over the editor and release there.
    app.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        area.x + 2,
        area.y + 2,
    ));
    app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), 2, 3));
    app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), 2, 3));
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    // The program sees the press, the drag and the release, clamped to its top row.
    let s = wait_text(&app, "[<0;3;1m");
    assert!(s.contains("[<0;3;2M") && s.contains("[<32;3;1M"), "{s}");
    // The editor did not take the release as a click of its own.
    assert_eq!(app.focus, Focus::Terminal);
    // A release vr-ide never sees (let go outside the window) does not keep the mouse: the
    // next press elsewhere and its drag belong to the editor.
    app.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        area.x + 2,
        area.y + 2,
    ));
    let drag = mouse(MouseEventKind::Drag(MouseButton::Left), 4, 3);
    app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 2, 3));
    assert_eq!(app.focus, Focus::Editor);
    assert!(!crate::commands::terminal::handle_mouse(&mut app, &drag));
}

/// T23 (D211/D218): with the explorer shown, a right press on a terminal whose program asked
/// for the mouse still reaches the program as before; no explorer menu opens.
#[test]
fn a_right_click_in_a_mouse_program_is_forwarded_not_a_menu() {
    let (_d, mut app, _) = term_app();
    app.keyboard_enhanced = true;
    app.explorer.visible = true;
    draw(&mut app, 80, 24);
    ctrl_backtick(&mut app);
    draw(&mut app, 80, 24);
    wait_text(&app, "$");
    typ(&mut app, "printf '\\033[?1000h\\033[?1006h'; cat -v\n");
    let deadline = Instant::now() + WAIT;
    while !tab(&app).grid_now().modes.mouse_reporting {
        assert!(Instant::now() < deadline, "mouse reporting never enabled");
        std::thread::sleep(Duration::from_millis(10));
    }
    let area = app.layout.terminal;
    for kind in [
        MouseEventKind::Down(MouseButton::Right),
        MouseEventKind::Up(MouseButton::Right),
    ] {
        app.handle_mouse(MouseEvent {
            kind,
            column: area.x + 2,
            row: area.y + 2,
            modifiers: KeyModifiers::NONE,
        });
    }
    key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let s = wait_text(&app, "[<2;3;2m");
    assert!(s.contains("[<2;3;2M"), "{s}");
    assert!(app.explorer.menu.is_none());
    assert_eq!(app.focus, Focus::Terminal);
}

pub(crate) fn fake_tab(title: &str, status: TermStatus) -> TermTab {
    TermTab::fake(fake_grid(80, 9), title, status)
}

#[test]
fn mouse_selection_copies_and_a_click_focuses() {
    let (_d, mut app, clip) = term_app();
    app.terminal.tabs.push(fake_tab("sh", TermStatus::Running));
    app.terminal.visible = true;
    draw(&mut app, 80, 24);
    let area = app.layout.terminal;
    let mouse = |kind, column, row| MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    // Grid row 0 starts below the panel's tab bar.
    let y = area.y + 1;
    app.handle_mouse(mouse(
        MouseEventKind::Down(MouseButton::Left),
        area.x + 2,
        y,
    ));
    assert_eq!(app.focus, Focus::Terminal);
    app.handle_mouse(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        area.x + 3,
        y + 1,
    ));
    app.handle_mouse(mouse(
        MouseEventKind::Up(MouseButton::Left),
        area.x + 3,
        y + 1,
    ));
    assert_eq!(clip.contents().as_deref(), Some("ls --color\ndocs"));
    // A click in the editor area gives it the keys back.
    app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 5, 3));
    assert_eq!(app.focus, Focus::Editor);
}

/// Four tabs titled as Ubuntu's bash sets them (`user@host: dir`), the last one active.
fn long_title_app() -> App {
    let (mut app, _) = test_app(Path::new("/home/user/DEV/shop"));
    for dir in ["alpha", "beta", "gamma", "delta"] {
        let title = format!("rainy@rainy-Lenovo-IdeaPad-5: ~/DEV/{dir}");
        app.terminal
            .tabs
            .push(fake_tab(&title, TermStatus::Running));
    }
    app.terminal.active = 3;
    app.terminal.visible = true;
    app.focus = Focus::Terminal;
    app
}

#[test]
fn long_titles_keep_the_active_tab_the_hint_and_the_status_in_view() {
    let mut app = long_title_app();
    let t = draw(&mut app, 80, 24);
    let bar = row_text(&t, app.layout.terminal.y);
    // Every tab is told apart by the end of its title (the directory).
    for dir in ["alpha", "beta", "gamma", "delta"] {
        assert!(bar.contains(dir), "{dir} missing: {bar}");
    }
    assert!(bar.contains("ctrl+` hide"), "hint dropped: {bar}");
    let status = row_text(&t, 23);
    assert!(status.contains(" 4/4 · 80×9 "), "{status}");
    // A click on a label activates that tab (hit test matches the drawing).
    let col = bar.find("beta").map(|b| bar[..b].chars().count()).unwrap() as u16;
    let area = app.layout.terminal;
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: area.x + col,
        row: area.y,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.terminal.active, 1);
    // At 40 columns the active tab stays in view with a marker for the hidden ones.
    app.terminal.active = 3;
    let t = draw(&mut app, 40, 12);
    let bar = row_text(&t, app.layout.terminal.y);
    assert!(bar.contains(" 4 ") && bar.contains("delta"), "{bar}");
    assert!(bar.contains("+"), "no marker for hidden tabs: {bar}");
    let status = row_text(&t, 11);
    assert!(status.contains("4/4"), "{status}");
}

// ---- rendering --------------------------------------------------------------------------------

/// A shell screen: a coloured `ls`, a wide character and Vietnamese text, a bold line.
pub(crate) fn fake_grid(cols: usize, rows: usize) -> Grid {
    let mut g = Grid {
        cols,
        rows,
        cells: vec![
            Cell {
                ch: " ".into(),
                ..Cell::default()
            };
            cols * rows
        ],
        cursor: Cursor {
            row: 4,
            col: 2,
            shape: CursorShape::Beam,
            visible: true,
        },
        title: None,
        ..Grid::default()
    };
    let mut put = |row: usize, col: usize, text: &str, fg: TColor, flags: CellFlags| {
        let mut c = col;
        for ch in text.chars() {
            if c >= cols || row >= rows {
                return;
            }
            let wide = matches!(ch, '世' | '界');
            g.cells[row * cols + c] = Cell {
                ch: ch.to_string(),
                fg,
                bg: TColor::Default,
                flags: CellFlags { wide, ..flags },
            };
            if wide && c + 1 < cols {
                g.cells[row * cols + c + 1] = Cell {
                    ch: String::new(),
                    flags: CellFlags {
                        wide_spacer: true,
                        ..CellFlags::default()
                    },
                    ..Cell::default()
                };
                c += 1;
            }
            c += 1;
        }
    };
    let plain = CellFlags::default();
    let bold = CellFlags {
        bold: true,
        ..CellFlags::default()
    };
    put(0, 0, "$ ls --color", TColor::Default, plain);
    put(1, 0, "docs", TColor::Indexed(4), bold);
    put(1, 6, "src", TColor::Indexed(4), bold);
    put(1, 11, "README.md", TColor::Default, plain);
    put(1, 22, "run.sh", TColor::Indexed(2), bold);
    put(2, 0, "$ echo 'xin chào 世界'", TColor::Default, plain);
    put(3, 0, "xin chào 世界", TColor::Rgb(0xff, 0x9d, 0x3c), plain);
    put(4, 0, "$ ", TColor::Default, plain);
    g
}

/// Fake tabs only (nothing is spawned), under a fixed root so the frame is the same on every run.
pub(crate) fn snapshot_app() -> ((), App) {
    let (mut app, _) = test_app(Path::new("/home/user/DEV/shop"));
    app.terminal.tabs.push(fake_tab("sh", TermStatus::Running));
    app.terminal
        .tabs
        .push(fake_tab("htop", TermStatus::Exited(0)));
    app.terminal.visible = true;
    app.focus = Focus::Terminal;
    ((), app)
}

#[test]
fn render_terminal_panel() {
    for (w, h) in [(80, 24), (120, 40), (40, 12)] {
        let (_d, mut app) = snapshot_app();
        let t = draw(&mut app, w, h);
        insta::assert_snapshot!(format!("render_terminal_panel_{w}x{h}"), t.backend());
    }
}

#[test]
fn render_exited_terminal() {
    let (_d, mut app) = snapshot_app();
    app.terminal.active = 1;
    let t = draw(&mut app, 80, 24);
    insta::assert_snapshot!("render_exited_terminal_80x24", t.backend());
}

#[test]
fn the_exited_banner_keeps_both_actions_at_40_columns() {
    let (_d, mut app) = snapshot_app();
    app.terminal.tabs[1] = fake_tab("htop", TermStatus::Exited(3));
    app.terminal.active = 1;
    let t = draw(&mut app, 40, 12);
    let area = app.layout.terminal;
    let banner = row_text(&t, area.y + area.height - 1);
    assert!(
        banner.contains("exited 3") && banner.contains("Enter") && banner.contains("ctrl+w"),
        "{banner}"
    );
    insta::assert_snapshot!("render_exited_terminal_40x12", t.backend());
}

#[test]
fn concealed_text_is_not_drawn_in_any_theme() {
    for theme in ["synthwave", "dracula", "dark", "light"] {
        let (_d, mut app) = snapshot_app();
        app.settings.theme = theme.to_string();
        let mut g = fake_grid(80, 9);
        for (i, ch) in "secret".chars().enumerate() {
            g.cells[5 * 80 + i] = Cell {
                ch: ch.to_string(),
                flags: CellFlags {
                    hidden: true,
                    ..CellFlags::default()
                },
                ..Cell::default()
            };
        }
        app.terminal.tabs[0] = TermTab::fake(g, "sh", TermStatus::Running);
        let t = draw(&mut app, 80, 24);
        let text = frame_text(&t);
        assert!(!text.contains("secret"), "{theme}:\n{text}");
    }
}

#[test]
fn render_long_titles_80x24() {
    let mut app = long_title_app();
    let t = draw(&mut app, 80, 24);
    insta::assert_snapshot!("render_terminal_long_titles_80x24", t.backend());
}

/// D291: the palette's terminal commands, without New / Close Terminal.
#[test]
fn render_palette_terminal_commands_80x24() {
    let (mut app, _) = test_app(Path::new("/p"));
    app.run_command("app.palette", CmdArgs::None);
    typ(&mut app, "terminal:");
    let t = draw(&mut app, 80, 24);
    let screen = frame_text(&t);
    assert!(!screen.contains("New Terminal") && !screen.contains("Close Terminal"));
    insta::assert_snapshot!("render_palette_terminal_commands_80x24", t.backend());
}

#[test]
fn panel_cursor_shows_only_while_focused() {
    let (_d, mut app) = snapshot_app();
    let mut t = draw(&mut app, 80, 24);
    let area = app.layout.terminal;
    t.backend_mut()
        .assert_cursor_position((area.x + 2, area.y + 1 + 4));
    assert_eq!(
        crate::commands::terminal::cursor_style(&app),
        Some(crossterm::cursor::SetCursorStyle::SteadyBar)
    );
    app.focus = Focus::Editor;
    draw(&mut app, 80, 24);
    assert_eq!(crate::commands::terminal::cursor_style(&app), None);
}

#[test]
fn panel_colours_follow_the_theme() {
    use crate::theme::Theme;
    use crate::ui::terminal_view::cell_style;
    use ratatui::style::{Color, Modifier};
    let synth = Theme::synthwave_with(crate::theme::ColorDepth::TrueColor);
    let p = synth.ansi_palette().copied();
    let red = Cell {
        ch: "x".into(),
        fg: TColor::Indexed(1),
        bg: TColor::Default,
        flags: CellFlags::default(),
    };
    let s = cell_style(&red, p.as_ref(), crate::theme::ColorDepth::TrueColor);
    assert_eq!(s.fg, Some(p.unwrap().colors[1]));
    assert_eq!(s.bg, Some(p.unwrap().bg));
    // dark keeps the terminal's own colours.
    let s = cell_style(&red, None, crate::theme::ColorDepth::TrueColor);
    assert_eq!(s.fg, Some(Color::Indexed(1)));
    assert_eq!(s.bg, Some(Color::Reset));
    // 256-colour and true-colour cells pass through.
    let cube = Cell {
        fg: TColor::Indexed(200),
        bg: TColor::Rgb(1, 2, 3),
        ..red.clone()
    };
    let s = cell_style(&cube, p.as_ref(), crate::theme::ColorDepth::TrueColor);
    assert_eq!(
        (s.fg, s.bg),
        (Some(Color::Indexed(200)), Some(Color::Rgb(1, 2, 3)))
    );
    // Inverse swaps the resolved colours; attributes map to modifiers.
    let inv = Cell {
        flags: CellFlags {
            inverse: true,
            bold: true,
            italic: true,
            underline: true,
            dim: true,
            ..CellFlags::default()
        },
        ..red.clone()
    };
    let s = cell_style(&inv, p.as_ref(), crate::theme::ColorDepth::TrueColor);
    assert_eq!(s.fg, Some(p.unwrap().bg));
    assert_eq!(s.bg, Some(p.unwrap().colors[1]));
    for m in [
        Modifier::BOLD,
        Modifier::ITALIC,
        Modifier::UNDERLINED,
        Modifier::DIM,
    ] {
        assert!(s.add_modifier.contains(m), "{m:?}");
    }
    // Without a palette inverse is left to the terminal.
    let s = cell_style(&inv, None, crate::theme::ColorDepth::TrueColor);
    assert!(s.add_modifier.contains(Modifier::REVERSED));
}

/// At 256 colours a program's true-colour cells are mapped to the nearest xterm index, as the
/// theme's own colours are; the child is told it may use true colour only when vr-ide can.
#[test]
fn truecolor_cells_follow_the_colour_depth() {
    use crate::theme::ColorDepth;
    use crate::ui::terminal_view::cell_style;
    use ratatui::style::Color;
    let cell = Cell {
        ch: "x".into(),
        fg: TColor::Rgb(255, 0, 0),
        bg: TColor::Rgb(1, 2, 3),
        flags: CellFlags::default(),
    };
    let s = cell_style(&cell, None, ColorDepth::TrueColor);
    assert_eq!(
        (s.fg, s.bg),
        (Some(Color::Rgb(255, 0, 0)), Some(Color::Rgb(1, 2, 3)))
    );
    let s = cell_style(&cell, None, ColorDepth::Indexed256);
    assert_eq!(s.fg, Some(ColorDepth::Indexed256.color(0xff0000)));
    assert!(matches!(s.fg, Some(Color::Indexed(_))), "{s:?}");
    assert!(matches!(s.bg, Some(Color::Indexed(_))), "{s:?}");
    assert_eq!(s.fg, Some(Color::Indexed(196)));

    let get = |env: &[(std::ffi::OsString, std::ffi::OsString)]| {
        env.iter()
            .rev()
            .find(|(k, _)| k == "COLORTERM")
            .map(|(_, v)| v.to_string_lossy().into_owned())
    };
    let user = vec![("VR_X".into(), "1".into())];
    let env = crate::commands::terminal::child_env(&user, ColorDepth::TrueColor);
    assert_eq!(get(&env).as_deref(), Some("truecolor"));
    assert!(env.contains(&user[0]));
    assert_eq!(
        get(&crate::commands::terminal::child_env(
            &user,
            ColorDepth::Indexed256
        )),
        None
    );
    // The user's own setting still wins.
    let own = vec![("COLORTERM".into(), "yes".into())];
    let env = crate::commands::terminal::child_env(&own, ColorDepth::TrueColor);
    assert_eq!(get(&env).as_deref(), Some("yes"));
}

/// A focused terminal always shows the way out in the status bar; the size gives way first.
#[test]
fn the_status_bar_names_the_way_out_before_the_size() {
    let (_d, mut app, _) = term_app();
    app.terminal.tabs.push(fake_tab("sh", TermStatus::Running));
    app.terminal.visible = true;
    app.focus = Focus::Terminal;
    for w in [30, 36, 40, 80] {
        let t = draw(&mut app, w, 12);
        let status = row_text(&t, 11);
        assert!(status.contains("ctrl+` hide"), "{w}: {status}");
        if w <= 36 {
            assert!(!status.contains('×'), "{w}: {status}");
        }
    }
}

/// A 90 % panel still leaves the editor a few rows on a screen that has them.
#[test]
fn a_tall_panel_leaves_the_editor_a_few_rows() {
    let (_d, mut app, _) = term_app();
    app.terminal.tabs.push(fake_tab("sh", TermStatus::Running));
    app.terminal.visible = true;
    app.terminal.height_percent = 90;
    for h in [12, 24, 40] {
        draw(&mut app, 80, h);
        assert!(app.layout.editor.height >= 3, "{h}: {:?}", app.layout);
    }
}

/// With no editor row (a tiny screen) the keys stay in the terminal, and the resource monitor
/// is not sampled for a welcome screen nobody can see.
#[test]
fn a_zero_row_editor_never_gets_the_keys_or_a_monitor() {
    let (_d, mut app, _) = term_app();
    app.terminal.tabs.push(fake_tab("sh", TermStatus::Running));
    app.terminal.visible = true;
    app.focus = Focus::Terminal;
    let t = draw(&mut app, 80, 7);
    assert_eq!(app.layout.editor.height, 0, "{:?}", app.layout);
    // No editor row to give the keys to: ctrl+` hides the panel (as everywhere, D280).
    let screen = frame_text(&t);
    assert!(screen.contains("ctrl+` hide"), "{screen}");
    let now = std::time::Instant::now();
    app.tick(now);
    assert!(!app.monitor.running());
    for enhanced in [false, true] {
        app.keyboard_enhanced = enhanced;
        app.terminal.visible = true;
        app.focus = Focus::Terminal;
        draw(&mut app, 80, 7);
        if enhanced {
            ctrl_backtick(&mut app);
        } else {
            nul(&mut app);
        }
        assert!(!app.terminal.visible, "kitty {enhanced}");
        assert_eq!(app.focus, Focus::Editor, "kitty {enhanced}");
    }
    app.keyboard_enhanced = false;
    app.terminal.visible = true;
    app.focus = Focus::Terminal;
    draw(&mut app, 80, 7);
    app.tick(now);
    assert!(!app.monitor.running());
    // With rows for the editor the welcome is drawn and sampled.
    draw(&mut app, 80, 24);
    app.tick(now + std::time::Duration::from_secs(2));
    assert_eq!(app.monitor.running(), app.monitor.supported());
}

/// terminal_pass_nul = true gives NUL to the program, and a click on the editor leaves the
/// terminal; with no editor row to click, NUL hides the panel so the keyboard can still leave.
#[test]
fn a_zero_row_editor_hides_the_panel_on_nul_even_with_pass_nul() {
    let (_d, mut app, _) = term_app();
    app.settings.terminal_pass_nul = true;
    app.terminal.tabs.push(fake_tab("sh", TermStatus::Running));
    app.terminal.visible = true;
    app.focus = Focus::Terminal;
    let t = draw(&mut app, 80, 7);
    assert_eq!(app.layout.editor.height, 0, "{:?}", app.layout);
    let screen = frame_text(&t);
    assert!(screen.contains("ctrl+` hide"), "{screen}");
    nul(&mut app);
    assert!(!app.terminal.visible);
    assert_eq!(app.focus, Focus::Editor);
    // With editor rows NUL goes to the program again.
    app.terminal.visible = true;
    app.focus = Focus::Terminal;
    draw(&mut app, 80, 24);
    nul(&mut app);
    assert!(app.terminal.visible);
    assert_eq!(app.focus, Focus::Terminal);
}

/// T22: the find box on the project sits above the editor, never in a full-width side
/// column, so it cannot cover the terminal panel: the terminal keeps its cells and its keys
/// on a narrow screen, and showing the panel leaves the box open.
#[test]
fn the_find_box_never_covers_the_terminal() {
    let (_d, mut app, _) = term_app();
    app.terminal.tabs.push(fake_tab("sh", TermStatus::Running));
    crate::commands::find::open_project(&mut app, false);
    app.terminal.visible = true;
    app.focus = Focus::Terminal;
    draw(&mut app, 30, 12);
    assert_eq!(app.layout.terminal.width, 30, "{:?}", app.layout);
    key(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
    assert_eq!(app.focus, Focus::Terminal);
    assert!(app.search.visible, "the box stays open");

    // Hidden panel, open box: showing the panel gives it the keys and the box stays.
    let (_d, mut app, _) = term_app();
    app.terminal.tabs.push(fake_tab("sh", TermStatus::Running));
    crate::commands::find::open_project(&mut app, false);
    draw(&mut app, 30, 12);
    app.keyboard_enhanced = true;
    ctrl_backtick(&mut app);
    assert!(app.terminal.visible && app.search.visible);
    assert_eq!(app.focus, Focus::Terminal);
    draw(&mut app, 30, 12);
    assert!(app.layout.terminal.width > 0, "{:?}", app.layout);
}

/// The fast poll is for programs that may still print; exited terminals do not need it.
#[test]
fn the_fast_poll_runs_only_while_a_terminal_is_running() {
    use crate::event_loop::poll_interval;
    let (_d, mut app, _) = term_app();
    let idle = poll_interval(&app);
    app.terminal.visible = true;
    assert_eq!(poll_interval(&app), idle);
    app.terminal
        .tabs
        .push(fake_tab("sh", TermStatus::Exited(0)));
    assert_eq!(poll_interval(&app), idle, "only exited terminals");
    app.terminal.tabs.push(fake_tab("sh", TermStatus::Running));
    assert!(poll_interval(&app) < idle);
}

#[test]
fn a_new_terminal_starts_in_the_root_of_the_active_file() {
    let d = tempfile::tempdir().unwrap();
    let base = d.path().canonicalize().unwrap();
    let (a, b) = (base.join("rootA"), base.join("rootB"));
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    std::fs::write(b.join("f.txt"), "x\n").unwrap();
    let (mut app, _) = test_app(&a);
    use_sh(&mut app);
    app.keyboard_enhanced = true;
    app.add_root(&b).unwrap();
    app.open_path(&b.join("f.txt")).unwrap();
    draw(&mut app, 120, 30);
    ctrl_backtick(&mut app);
    wait_text(&app, "$");
    typ(&mut app, "pwd\n");
    let screen = wait_screen(&app, "pwd output", |s| s.contains("/root"));
    assert!(screen.contains("rootB"), "{screen}");
    assert!(!screen.contains("rootA"), "{screen}");
}

/// T24 review: the close 'x' is drawn in the muted colour, apart from the title, so
/// " 1 tmux x " does not read as one word; the title keeps the tab's own colour.
#[test]
fn the_close_x_is_styled_apart_from_the_title() {
    let (_d, mut app, _) = term_app();
    app.keyboard_enhanced = true;
    draw(&mut app, 80, 24);
    ctrl_backtick(&mut app);
    let t = draw(&mut app, 80, 24);
    let bar = crate::ui::terminal_view::tab_bar(&app, app.layout.terminal.width);
    let tab = &bar.tabs[0];
    let y = app.layout.terminal.y;
    let x0 = app.layout.terminal.x + tab.x;
    let close = x0 + tab.close.expect("its x is drawn");
    let buf = t.backend().buffer();
    assert_eq!(buf[(close, y)].symbol(), "x");
    assert_eq!(buf[(close - 1, y)].symbol(), " ", "a space before the x");
    let dim_fg = crate::theme::Theme::from_name(&app.settings.theme).dim.fg;
    assert_eq!(buf[(close, y)].fg, dim_fg.unwrap(), "x in the muted colour");
    // The tab number keeps the active tab's colour, which is not the muted one.
    assert_ne!(buf[(x0 + 1, y)].fg, buf[(close, y)].fg, "title vs x");
    // Same background as the rest of the label.
    assert_eq!(buf[(close, y)].bg, buf[(x0 + 1, y)].bg);
}
