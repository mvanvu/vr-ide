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
fn term_app() -> (tempfile::TempDir, App, MemoryClipboard) {
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

fn draw(app: &mut App, w: u16, h: u16) -> Terminal<TestBackend> {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| crate::ui::render(f, app)).unwrap();
    t
}

fn key(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    app.handle_key(KeyEvent::new(code, mods));
}

/// ctrl+` as the kitty protocol reports it.
fn ctrl_backtick(app: &mut App) {
    key(app, KeyCode::Char('`'), KeyModifiers::CONTROL);
}

/// The NUL byte a legacy terminal sends for ctrl+` (crossterm reports it as ctrl+space).
fn nul(app: &mut App) {
    key(app, KeyCode::Char(' '), KeyModifiers::CONTROL);
}

fn alt_q(app: &mut App) {
    key(app, KeyCode::Char('q'), KeyModifiers::ALT);
}

fn typ(app: &mut App, s: &str) {
    for c in s.chars() {
        if c == '\n' {
            key(app, KeyCode::Enter, KeyModifiers::NONE);
        } else {
            key(app, KeyCode::Char(c), KeyModifiers::NONE);
        }
    }
}

fn tab(app: &App) -> &TermTab {
    &app.terminal.tabs[app.terminal.active]
}

/// Waits until the active terminal's screen satisfies `ok`; returns the screen.
fn wait_screen(app: &App, what: &str, ok: impl Fn(&str) -> bool) -> String {
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

fn wait_text(app: &App, needle: &str) -> String {
    wait_screen(app, needle, |s| s.contains(needle))
}

fn wait_status(app: &App, want: TermStatus) {
    let deadline = Instant::now() + WAIT;
    while tab(app).status() != want {
        assert!(Instant::now() < deadline, "status {:?}", tab(app).status());
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn frame_text(t: &Terminal<TestBackend>) -> String {
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

fn row_text(t: &Terminal<TestBackend>, y: u16) -> String {
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

// ---- toggle state machine (AC-14a) ------------------------------------------------------------

#[test]
fn ctrl_backtick_shows_hides_and_focuses_the_panel() {
    let (_d, mut app, _) = term_app();
    app.keyboard_enhanced = true;
    draw(&mut app, 80, 24);
    // Hidden → shown, focused, one terminal started.
    ctrl_backtick(&mut app);
    assert!(app.terminal.visible);
    assert_eq!(app.focus, Focus::Terminal);
    assert_eq!(app.terminal.tabs.len(), 1);
    let pid = tab(&app).pid().expect("running shell");
    // Shown and focused → hidden, editor focus; the shell keeps running.
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
    // alt+q (D8) gives the keys back to the editor; the panel stays.
    alt_q(&mut app);
    assert_eq!(app.focus, Focus::Editor);
    assert!(app.terminal.visible);
    // Shown but not focused → focused.
    ctrl_backtick(&mut app);
    assert_eq!(app.focus, Focus::Terminal);
    assert!(app.terminal.visible);
    // From the explorer too.
    app.focus = Focus::Explorer;
    ctrl_backtick(&mut app);
    assert_eq!(app.focus, Focus::Terminal);
    assert_eq!(app.terminal.tabs.len(), 1);
}

#[test]
fn legacy_nul_toggles_outside_a_terminal_and_reaches_the_shell_inside() {
    let (_d, mut app, _) = term_app();
    app.keyboard_enhanced = false;
    draw(&mut app, 80, 24);
    // In the editor, NUL is ctrl+`.
    nul(&mut app);
    assert!(app.terminal.visible);
    assert_eq!(app.focus, Focus::Terminal);
    // Inside a focused terminal it goes to the program: cat -v shows it as ^@.
    typ(&mut app, "cat -v\n");
    wait_text(&app, "cat -v");
    nul(&mut app);
    typ(&mut app, "x\n");
    wait_text(&app, "^@x");
    assert!(app.terminal.visible);
    assert_eq!(app.focus, Focus::Terminal);
    // alt+q leaves; NUL from the editor focuses the shown panel again.
    alt_q(&mut app);
    assert_eq!(app.focus, Focus::Editor);
    nul(&mut app);
    assert_eq!(app.focus, Focus::Terminal);
    // From the explorer, NUL toggles as well (hide needs the terminal focused: shown and
    // unfocused → focus).
    alt_q(&mut app);
    app.focus = Focus::Explorer;
    nul(&mut app);
    assert_eq!(app.focus, Focus::Terminal);
    // A kitty-reported ctrl+` inside the terminal hides the panel.
    app.keyboard_enhanced = true;
    ctrl_backtick(&mut app);
    assert!(!app.terminal.visible);
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
    alt_q(&mut app);
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

// ---- tabs, exited state ----------------------------------------------------------------------

#[test]
fn new_close_next_and_previous_terminal() {
    let (_d, mut app, _) = term_app();
    app.keyboard_enhanced = true;
    draw(&mut app, 80, 24);
    let new = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
    key(&mut app, KeyCode::Char('~'), new);
    assert!(app.terminal.visible);
    assert_eq!(app.focus, Focus::Terminal);
    assert_eq!(app.terminal.tabs.len(), 1);
    // ctrl+shift+` inside a focused terminal opens another one (VS Code).
    key(&mut app, KeyCode::Char('~'), new);
    assert_eq!(app.terminal.tabs.len(), 2);
    assert_eq!(app.terminal.active, 1);
    let t = draw(&mut app, 80, 24);
    let bar = row_text(&t, app.layout.terminal.y);
    assert!(bar.contains("1 sh") && bar.contains("2 sh"), "{bar}");
    app.run_command("terminal.prev", CmdArgs::None);
    assert_eq!(app.terminal.active, 0);
    app.run_command("terminal.next", CmdArgs::None);
    assert_eq!(app.terminal.active, 1);
    app.run_command("terminal.next", CmdArgs::None);
    assert_eq!(app.terminal.active, 0);
    let pid = tab(&app).pid().unwrap();
    app.run_command("terminal.close", CmdArgs::None);
    assert_eq!(app.terminal.tabs.len(), 1);
    let deadline = Instant::now() + WAIT;
    while proc_alive(pid) {
        assert!(
            Instant::now() < deadline,
            "closed terminal's shell still alive"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    // Closing the last one hides the panel and gives the editor the keys.
    app.run_command("terminal.close", CmdArgs::None);
    assert!(app.terminal.tabs.is_empty());
    assert!(!app.terminal.visible);
    assert_eq!(app.focus, Focus::Editor);
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
    alt_q(&mut app);
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
    alt_q(&mut app);
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
    alt_q(&mut app);
    let t = draw(&mut app, 80, 24);
    assert!(row_text(&t, 23).starts_with(" READ "));
}

fn fake_tab(title: &str, status: TermStatus) -> TermTab {
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

// ---- rendering --------------------------------------------------------------------------------

/// A shell screen: a coloured `ls`, a wide character and Vietnamese text, a bold line.
fn fake_grid(cols: usize, rows: usize) -> Grid {
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
fn snapshot_app() -> ((), App) {
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
    let s = cell_style(&red, p.as_ref());
    assert_eq!(s.fg, Some(p.unwrap().colors[1]));
    assert_eq!(s.bg, Some(p.unwrap().bg));
    // dark keeps the terminal's own colours.
    let s = cell_style(&red, None);
    assert_eq!(s.fg, Some(Color::Indexed(1)));
    assert_eq!(s.bg, Some(Color::Reset));
    // 256-colour and true-colour cells pass through.
    let cube = Cell {
        fg: TColor::Indexed(200),
        bg: TColor::Rgb(1, 2, 3),
        ..red.clone()
    };
    let s = cell_style(&cube, p.as_ref());
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
    let s = cell_style(&inv, p.as_ref());
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
    let s = cell_style(&inv, None);
    assert!(s.add_modifier.contains(Modifier::REVERSED));
}
