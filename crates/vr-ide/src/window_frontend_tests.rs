//! The window frontend (feature `window`): what `step` gives the window, the close button and
//! SIGTERM through the ctrl+q path, font settings, the menu entry migration and the PTY cell
//! size. The window itself is replaced by a recording [`WindowOps`]; drawing uses the real
//! pixel backend over the system fonts.

use std::sync::atomic::Ordering;
use std::time::Instant;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::layout::Rect;
use vr_window::app::{WindowHandler, WindowOps};
use vr_window::{CursorShape as Drawn, FontConfig, FontSet, Palette, PixelBackend, rgb};

use crate::app::App;
use crate::commands::CmdArgs;
use crate::commands::dialog::DialogAction;
use crate::frontend::{CursorShape, Frontend};
use crate::window_frontend::{
    WindowApp, WindowFrontend, apply_fonts, drawn_cursor, font_config, font_notice,
    migrate_menu_entry, palette_for, request_quit, window_dark,
};

/// Records what the app asks of the window.
#[derive(Default)]
struct FakeOps {
    titles: Vec<String>,
    cursors: Vec<(Drawn, bool)>,
    ime: Vec<Option<(u16, u16)>>,
    attention: usize,
    /// `true`: dark title bar.
    themes: Vec<bool>,
}

impl WindowOps for FakeOps {
    fn set_title(&mut self, title: &str) {
        self.titles.push(title.to_string());
    }

    fn set_cursor(&mut self, shape: Drawn, blinking: bool) {
        self.cursors.push((shape, blinking));
    }

    fn set_ime_cell(&mut self, cell: Option<(u16, u16)>) {
        self.ime.push(cell);
    }

    fn request_attention(&mut self) {
        self.attention += 1;
    }

    fn set_theme(&mut self, dark: bool) {
        self.themes.push(dark);
    }
}

fn window_terminal() -> Terminal<PixelBackend> {
    let (fonts, warning) = FontSet::new(&FontConfig::default());
    assert_eq!(warning, None);
    Terminal::new(PixelBackend::new(fonts, Palette::default(), 800, 400)).unwrap()
}

fn step(w: &mut WindowApp<'_>, t: &mut Terminal<PixelBackend>, ops: &mut FakeOps) -> bool {
    w.step(t, ops, Vec::new(), Instant::now()).quit
}

fn typed(app: &mut App, c: char) {
    app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
}

fn notices(app: &App) -> String {
    app.notices
        .iter()
        .map(|n| n.text.as_str())
        .collect::<Vec<_>>()
        .join(" | ")
}

#[test]
fn the_window_frontend_passes_each_call_to_the_window() {
    let mut ops = FakeOps::default();
    let mut f = WindowFrontend::new(&mut ops);
    assert!(f.keyboard_enhanced(), "every chord arrives in the window");
    f.set_title("vr-ide — x").unwrap();
    f.set_cursor_shape(CursorShape::BlinkingBar).unwrap();
    f.set_cursor_shape(CursorShape::SteadyBlock).unwrap();
    f.set_ime_area(Some(Rect::new(3, 4, 1, 1)));
    f.set_ime_area(None);
    f.request_attention();
    assert_eq!(ops.titles, ["vr-ide — x"]);
    assert_eq!(ops.cursors, [(Drawn::Bar, true), (Drawn::Block, false)]);
    assert_eq!(ops.ime, [Some((3, 4)), None]);
    assert_eq!(ops.attention, 1);

    let all = [
        (CursorShape::SteadyBlock, (Drawn::Block, false)),
        (CursorShape::BlinkingBlock, (Drawn::Block, true)),
        (CursorShape::SteadyBar, (Drawn::Bar, false)),
        (CursorShape::BlinkingBar, (Drawn::Bar, true)),
        (CursorShape::SteadyUnderline, (Drawn::Underline, false)),
        (CursorShape::BlinkingUnderline, (Drawn::Underline, true)),
    ];
    for (shape, want) in all {
        assert_eq!(drawn_cursor(shape), want, "{shape:?}");
    }
}

/// The first frame gives title, cursor and IME cell once; a redraw with nothing changed gives
/// nothing; the keyboard counts as enhanced (no "cannot send" notice) and the PTY cell size is
/// the backend's.
#[test]
fn the_window_gets_only_changed_values_and_no_terminal_key_notice() {
    let (_d, _p, mut app, _) = crate::app_tests::app_with_file("a\n");
    let mut t = window_terminal();
    let mut ops = FakeOps::default();
    let mut w = WindowApp::new(&mut app);
    assert!(!step(&mut w, &mut t, &mut ops));
    assert_eq!(ops.titles.len(), 1);
    assert_eq!(
        ops.cursors,
        [(Drawn::Bar, true)],
        "edit mode types into text"
    );
    let cursor = t.backend().cursor_position();
    assert_eq!(ops.ime, [Some((cursor.x, cursor.y))]);

    // Moving the cursor: only the IME cell follows.
    let right = Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
    w.step(&mut t, &mut ops, vec![right], Instant::now());
    assert_eq!((ops.titles.len(), ops.cursors.len()), (1, 1));
    assert_eq!(ops.ime.last(), Some(&Some((cursor.x + 1, cursor.y))));
    assert!(!step(&mut w, &mut t, &mut ops));
    assert_eq!(ops.ime.len(), 2, "nothing new without a change");
    drop(w);

    assert!(app.keyboard_enhanced);
    let text = notices(&app);
    assert!(!text.contains("cannot send"), "{text}");
    let cell = t.backend().layout().cell;
    assert_eq!(app.cell_px, (cell.width as u16, cell.height as u16));
}

/// Review focus 5: the close button with nothing unsaved quits at once; with an unsaved file it
/// asks as ctrl+q does, after closing the dialog that was open; SIGTERM does the same.
#[test]
fn closing_the_window_or_sigterm_takes_the_ctrl_q_path() {
    let (_d, _p, mut app, _) = crate::app_tests::app_with_file("a\n");
    let mut t = window_terminal();
    let mut ops = FakeOps::default();
    {
        let mut w = WindowApp::new(&mut app);
        assert!(!step(&mut w, &mut t, &mut ops));
        w.close_requested();
        assert!(step(&mut w, &mut t, &mut ops), "nothing unsaved: quits");
    }

    let (_d, _p, mut app, _) = crate::app_tests::app_with_file("a\n");
    typed(&mut app, 'x');
    app.run_command("nav.goto_line", CmdArgs::None);
    assert!(matches!(
        app.dialog.as_ref().map(|d| &d.action),
        Some(DialogAction::GotoLine)
    ));
    {
        let mut w = WindowApp::new(&mut app);
        w.close_requested();
        assert!(!step(&mut w, &mut t, &mut ops), "unsaved: asks first");
    }
    assert!(!app.quit);
    assert!(
        matches!(
            app.dialog.as_ref().map(|d| &d.action),
            Some(DialogAction::Quit)
        ),
        "the open dialog was closed and the quit question shown"
    );
    assert_eq!(
        app.docs[0].buffer.text().to_string(),
        "xa\n",
        "nothing lost"
    );

    // SIGTERM: the same question (the flag is what the signal handler sets).
    app.dialog = None;
    {
        let mut w = WindowApp::new(&mut app);
        w.sigterm_flag().store(true, Ordering::SeqCst);
        assert!(!step(&mut w, &mut t, &mut ops));
        assert!(!w.sigterm_flag().load(Ordering::SeqCst), "handled once");
    }
    assert!(matches!(
        app.dialog.as_ref().map(|d| &d.action),
        Some(DialogAction::Quit)
    ));
}

/// A running terminal asks before the window closes, as for ctrl+q.
#[test]
fn closing_the_window_with_a_running_terminal_asks() {
    let (_d, mut app, _) = crate::terminal_tests::term_app();
    crate::terminal_tests::ctrl_backtick(&mut app);
    assert_eq!(app.terminal.running(), 1);
    request_quit(&mut app);
    assert!(!app.quit);
    assert!(matches!(
        app.dialog.as_ref().map(|d| &d.action),
        Some(DialogAction::QuitTerminals)
    ));
    crate::commands::terminal::shutdown(&mut app);
}

#[test]
fn font_settings_feed_the_font_set_and_a_missing_family_warns() {
    let mut settings = vr_config::Settings::default();
    let c = font_config(&settings, 2.0);
    assert_eq!(c.family, None, "empty: the platform's monospace");
    assert_eq!((c.size_pt, c.line_height, c.scale), (11.0, 1.2, 2.0));
    settings.font_family = "  DejaVu Sans Mono ".into();
    assert_eq!(
        font_config(&settings, 1.0).family.as_deref(),
        Some("DejaVu Sans Mono")
    );

    let mut t = window_terminal();
    let default_family = t.backend().fonts().family().to_string();
    let small = t.backend().layout().cell;
    settings.font_family = String::new();
    settings.font_size = 20.0;
    assert_eq!(apply_fonts(t.backend_mut(), &settings), None);
    let big = t.backend().layout().cell;
    assert!(
        big.width > small.width && big.height > small.height,
        "{small:?} → {big:?}"
    );

    settings.font_family = "No Such Font Family 42".into();
    settings.font_size = 14.0;
    let warning = apply_fonts(t.backend_mut(), &settings).expect("a warning");
    assert_eq!(
        warning,
        "settings: font_family \"No Such Font Family 42\" is not installed; using the system \
         monospace font at 14 pt"
    );
    assert_eq!(t.backend().fonts().family(), default_family);
    assert_eq!(
        t.backend().fonts().config().size_pt,
        14.0,
        "the asked size stays"
    );
}

#[test]
fn a_settings_reload_relays_the_grid_and_warns_for_a_missing_family() {
    let (_d, _p, mut app, _) = crate::app_tests::app_with_file("a\n");
    let mut t = window_terminal();
    let mut ops = FakeOps::default();
    let mut w = WindowApp::new(&mut app);
    step(&mut w, &mut t, &mut ops);
    let before = t.backend().layout().cell;
    // What a reload of settings.toml does to `app.settings` between two steps.
    w.app_mut().settings.font_size = 20.0;
    w.app_mut().settings.font_family = "Missing Family 42".into();
    step(&mut w, &mut t, &mut ops);
    let after = t.backend().layout().cell;
    assert!(after.height > before.height, "{before:?} → {after:?}");
    drop(w);
    assert_eq!(app.cell_px, (after.width as u16, after.height as u16));
    let text = notices(&app);
    assert!(text.contains("Missing Family 42"), "{text}");

    // A theme change gives the window the theme's colours.
    let mut w = WindowApp::new(&mut app);
    step(&mut w, &mut t, &mut ops);
    w.app_mut().settings.theme = "light".into();
    step(&mut w, &mut t, &mut ops);
    assert_eq!(t.backend().palette().background, rgb(0xff, 0xff, 0xff));
    // FR-1 U3: and the title bar the matching light / dark variant.
    assert_eq!(ops.themes, [false]);
    w.app_mut().settings.theme = "dracula".into();
    step(&mut w, &mut t, &mut ops);
    assert_eq!(ops.themes, [false, true]);
}

#[test]
fn the_palette_follows_the_theme() {
    for (theme, dark) in [
        ("light", false),
        ("dark", true),
        ("dracula", true),
        ("synthwave", true),
    ] {
        assert_eq!(window_dark(theme), dark, "{theme}");
    }
    assert_eq!(palette_for("dark"), Palette::default());
    assert_eq!(palette_for("light").background, rgb(0xff, 0xff, 0xff));
    let dracula = crate::theme::Theme::named("dracula").unwrap();
    if let Some(ratatui::style::Color::Rgb(r, g, b)) = dracula.text.bg {
        assert_eq!(palette_for("dracula").background, rgb(r, g, b));
    }
    assert_eq!(palette_for("no such theme"), Palette::default());
}

/// AC-W7: the first window start rewrites a vr-ide menu entry of the GNOME Terminal form and
/// says so; the next start says nothing.
#[test]
fn the_first_window_start_migrates_an_old_menu_entry() {
    use crate::desktop::{EntryForm, install};
    let tmp = tempfile::tempdir().unwrap();
    let exe = tmp.path().join("bin/vr-ide");
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::write(&exe, "#!/bin/sh\n").unwrap();
    let data = tmp.path().join("data");
    install(&data, &exe, EntryForm::GnomeTerminal).unwrap();

    let (_d, _p, mut app, _) = crate::app_tests::app_with_file("a\n");
    migrate_menu_entry(&mut app, Some(&data), Some(&exe));
    let text = notices(&app);
    assert!(text.contains("own window"), "{text}");
    let desktop = std::fs::read_to_string(data.join("applications/vr-ide.desktop")).unwrap();
    assert!(desktop.contains("Terminal=false"), "{desktop}");

    let count = app.notices.len();
    migrate_menu_entry(&mut app, Some(&data), Some(&exe));
    assert_eq!(app.notices.len(), count, "nothing to say the second time");
    migrate_menu_entry(&mut app, None, Some(&exe));
    assert_eq!(app.notices.len(), count);
}

/// AC-W6: embedded terminals get the window's cell size in pixels with the grid; the terminal
/// UI gives 0 x 0.
#[test]
fn embedded_terminals_get_the_window_cell_size() {
    let (_d, mut app, _) = crate::terminal_tests::term_app();
    crate::terminal_tests::ctrl_backtick(&mut app);
    crate::terminal_tests::draw(&mut app, 80, 24);
    let (grid, cell) = crate::terminal_tests::tab(&app).pty_size().unwrap();
    assert_eq!(cell, (0, 0), "terminal UI");
    app.cell_px = (9, 19);
    crate::terminal_tests::draw(&mut app, 80, 24);
    assert_eq!(
        crate::terminal_tests::tab(&app).pty_size(),
        Some((grid, (9, 19))),
        "same grid, the cell size given"
    );
    crate::commands::terminal::shutdown(&mut app);
}

/// Review blocker 1, the app side: a display lost after the start ends as a failed loop, which
/// `event_loop::exit` turns into the recovery write (session save and child shutdown follow).
#[test]
fn a_lost_display_ends_through_the_exit_path_with_unsaved_work_written() {
    let (_d, _p, mut app, _) = crate::app_tests::app_with_file("a\n");
    typed(&mut app, 'x');
    let mut t = window_terminal();
    let mut ops = FakeOps::default();
    let result = {
        let mut w = WindowApp::new(&mut app);
        assert!(!step(&mut w, &mut t, &mut ops));
        w.failed("the connection to the display was lost: Exit Failure: 1".into());
        w.take_result()
    };
    let err = result.expect_err("the loop failed");
    assert!(err.to_string().contains("display was lost"), "{err:#}");
    let recovery = tempfile::tempdir().unwrap();
    let out = crate::event_loop::finish(Err(err), &app, recovery.path()).unwrap_err();
    assert!(
        format!("{out:#}").contains("unsaved buffers written"),
        "{out:#}"
    );
}

/// What a signal does to a window with an unsaved buffer, in a child process (the signal may end
/// it): while the window is up, then after the window is gone but before recovery is written,
/// then after the switch (`SigtermDefault`). Returns the child's stdout and how it ended.
#[cfg(unix)]
fn signal_child(test: &str, sig: i32) -> (String, std::process::ExitStatus) {
    use std::io::Write;
    const CHILD: &str = "VR_IDE_TEST_SIGNAL_CHILD";
    if std::env::var(CHILD).ok() == Some(sig.to_string()) {
        let (_d, _p, mut app, _) = crate::app_tests::app_with_file("a\n");
        typed(&mut app, 'x');
        let mut t = window_terminal();
        let mut ops = FakeOps::default();
        let mut out = std::io::stdout();
        let switch = {
            let mut w = WindowApp::new(&mut app);
            w.catch_signals();
            step(&mut w, &mut t, &mut ops);
            signal_hook::low_level::raise(sig).unwrap();
            let quit = step(&mut w, &mut t, &mut ops);
            let asked = matches!(
                w.app_mut().dialog.as_ref().map(|d| &d.action),
                Some(DialogAction::Quit)
            );
            let _ = writeln!(out, "window: quit={quit} asked={asked}");
            w.sigterm_default()
        };
        // The window is gone, recovery not written yet.
        signal_hook::low_level::raise(sig).unwrap();
        let _ = writeln!(out, "survived before recovery");
        let _ = out.flush();
        switch.enable();
        signal_hook::low_level::raise(sig).unwrap();
        let _ = writeln!(out, "survived after the switch");
        let _ = out.flush();
        return (String::new(), std::process::ExitStatus::default());
    }
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            &format!("window_frontend_tests::{test}"),
            "--test-threads=1",
            "--nocapture",
        ])
        .env(CHILD, sig.to_string())
        .stderr(std::process::Stdio::null())
        .output()
        .unwrap();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status,
    )
}

#[cfg(unix)]
fn in_child() -> bool {
    std::env::var_os("VR_IDE_TEST_SIGNAL_CHILD").is_some()
}

/// Asserts the parent's view: the window asked before quitting (`asked`), the signal before the
/// recovery write did not end the process, and after the switch the signal did.
#[cfg(unix)]
fn assert_signal_path(test: &str, sig: i32, asked: bool) {
    use std::os::unix::process::ExitStatusExt;
    let (stdout, status) = signal_child(test, sig);
    if in_child() {
        return;
    }
    assert!(
        stdout.contains(&format!("window: quit=false asked={asked}")),
        "signal {sig} in the window: {stdout:?} {status:?}"
    );
    assert!(
        stdout.contains("survived before recovery"),
        "signal {sig} before the recovery write must not end the process: {stdout:?} {status:?}"
    );
    assert!(!stdout.contains("survived after the switch"), "{stdout:?}");
    assert_eq!(
        status.signal(),
        Some(sig),
        "after the switch the child must end by signal {sig}, got {status:?}"
    );
}

/// Review minor 3 (rounds 1 and 2): SIGTERM takes the quit path (the unsaved buffer asks), and
/// after the window is gone still only sets the flag until recovery is written (the switch),
/// then ends the process (default action), never left ignored.
#[cfg(unix)]
#[test]
fn sigterm_ends_the_process_once_recovery_is_written_after_the_window() {
    assert_signal_path(
        "sigterm_ends_the_process_once_recovery_is_written_after_the_window",
        signal_hook::consts::SIGTERM,
        true,
    );
}

/// FR-1 C1: ctrl+c in the launching shell (SIGINT) takes the same quit path as SIGTERM.
#[cfg(unix)]
#[test]
fn sigint_takes_the_quit_path_and_ends_only_after_recovery() {
    assert_signal_path(
        "sigint_takes_the_quit_path_and_ends_only_after_recovery",
        signal_hook::consts::SIGINT,
        true,
    );
}

/// FR-1 C1: closing the launching terminal (SIGHUP) leaves the window as it is (no question, no
/// quit); after the window it ends the process only once recovery is written.
#[cfg(unix)]
#[test]
fn sighup_is_ignored_by_the_window_and_ends_only_after_recovery() {
    assert_signal_path(
        "sighup_is_ignored_by_the_window_and_ends_only_after_recovery",
        signal_hook::consts::SIGHUP,
        false,
    );
}

/// Review (round 2): a lost X11 server ends the process inside libX11; its hook writes what the
/// loop published for a crash, as the panic hook does, without the app.
#[test]
fn display_loss_writes_the_published_unsaved_buffers() {
    let (_d, _p, mut app, _) = crate::app_tests::app_with_file("a\n");
    typed(&mut app, 'x');
    let state = crate::recovery::RecoveryState::new();
    state.publish_in(&app.docs, &app.workspace);
    let recovery = tempfile::tempdir().unwrap();
    let dir = recovery.path().join("recovery");
    let written = crate::window_frontend::write_recovery_on_display_loss(&state, &dir);
    assert_eq!(written.len(), 1, "{written:?}");
    assert_eq!(std::fs::read_to_string(&written[0]).unwrap(), "xa\n");
}

/// Review minor 3 (round 2): the window's exit gives SIGTERM its default back only after the
/// unsaved buffers are in recovery.
#[test]
fn the_exit_switches_sigterm_back_only_after_recovery_is_written() {
    let (_d, _p, mut app, _) = crate::app_tests::app_with_file("a\n");
    typed(&mut app, 'x');
    let state = tempfile::tempdir().unwrap();
    let dir = state.path().join("recovery");
    let mut files_at_switch = None;
    let r = crate::event_loop::exit_with(
        Err(anyhow::anyhow!("the connection to the display was lost")),
        &mut app,
        &dir,
        &mut || {},
        &mut || files_at_switch = Some(std::fs::read_dir(&dir).map_or(0, |d| d.count())),
        std::time::Duration::from_millis(100),
    );
    assert_eq!(
        files_at_switch,
        Some(1),
        "recovery written before the switch"
    );
    assert!(format!("{:#}", r.unwrap_err()).contains("unsaved buffers written"));
}

/// FR-1 U7: the start's font notice reads like the reload's and the other settings notices,
/// with the size the window really uses (the start keeps the asked size, U2).
#[test]
fn the_start_font_notice_names_the_setting_and_the_size_used() {
    let settings = vr_config::Settings {
        font_family: "No Such Font Family 42".into(),
        font_size: 16.0,
        ..Default::default()
    };
    let (set, err) = FontSet::new(&font_config(&settings, 1.0));
    let err = err.expect("a missing family");
    assert_eq!(
        font_notice(&err, set.config()),
        "settings: font_family \"No Such Font Family 42\" is not installed; using the system \
         monospace font at 16 pt"
    );
    let invalid = vr_window::FontError::InvalidSize(0.0);
    assert!(
        font_notice(&invalid, &FontConfig::default()).starts_with("settings: font size 0 "),
        "{}",
        font_notice(&invalid, &FontConfig::default())
    );
}
