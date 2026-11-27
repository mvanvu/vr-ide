use std::io::{self, Stdout};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use crossterm::cursor::SetCursorStyle;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, SetTitle, disable_raw_mode, enable_raw_mode,
    supports_keyboard_enhancement,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use vr_ide::app::App;
use vr_ide::{cli, clipboard, event_loop, recovery, session, ui};

/// Set once the kitty keyboard flags were pushed, so restoring pops them (also from the
/// panic hook).
static ENHANCED: AtomicBool = AtomicBool::new(false);

fn restore_terminal() {
    let mut out = io::stdout();
    if ENHANCED.swap(false, Ordering::SeqCst) {
        let _ = execute!(out, PopKeyboardEnhancementFlags);
    }
    let _ = execute!(
        out,
        SetCursorStyle::DefaultUserShape,
        DisableBracketedPaste,
        DisableMouseCapture,
        LeaveAlternateScreen
    );
    let _ = disable_raw_mode();
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    // claude's statusLine helper (SEC-23): before the panic hook, session, config and terminal.
    if let Some(report) = cli::ai_statusline(&args) {
        std::process::exit(vr_ide::ai::usage::run_helper(&report));
    }
    match cli::desktop_action(&args) {
        Ok(Some(action)) => {
            let result = match action {
                cli::DesktopAction::Install => vr_ide::desktop::run_install(),
                cli::DesktopAction::Uninstall => vr_ide::desktop::run_uninstall(),
            };
            match result {
                Ok(msg) => println!("{msg}"),
                Err(e) => {
                    eprintln!("vr-ide: {e:#}");
                    std::process::exit(1);
                }
            }
            return Ok(());
        }
        Ok(None) => {}
        Err(e) => {
            eprintln!("vr-ide: {e}");
            std::process::exit(2);
        }
    }
    let launch = match cli::resolve(&args, &std::env::current_dir()?) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("vr-ide: {e}");
            std::process::exit(2);
        }
    };

    recovery::install_panic_hook(vr_config::state_dir().join("recovery"), restore_terminal);

    let cli::Launch {
        workspace,
        open_files,
        notices,
        session: keep_session,
    } = launch;
    // `--session` only (D2): lock and read the saved session, which names the roots.
    let (workspace, restore) = if keep_session {
        let started = session::start(&vr_config::state_dir(), &workspace.primary().path);
        (started.workspace, Some(started.restore))
    } else {
        (workspace, None)
    };
    let (mut app, warnings) = App::load_workspace(workspace, clipboard::system());
    enable_raw_mode()?;
    let setup = execute!(
        io::stdout(),
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste,
        SetTitle(app.title())
    );
    if let Err(e) = setup {
        restore_terminal();
        return Err(e.into());
    }
    recovery::GLOBAL.set_tui_active(true);
    app.report_config(warnings);
    for n in notices {
        app.warn(n);
    }
    app.start_watcher();
    if let Some(restore) = restore {
        session::restore(&mut app, restore);
        // After the roots: a saved agent reopens only in one of them.
        vr_ide::ai::restore::from_session(&mut app);
    }
    for file in &open_files {
        if let Err(e) = app.open_path(file) {
            app.error(e.to_string());
        }
    }

    let result = run(&mut app);
    // Terminal and recovery first, then the bounded wait for a project replace, then the
    // exit messages (readable now that the alternate screen is gone).
    event_loop::exit(
        result,
        &mut app,
        &vr_config::state_dir().join("recovery"),
        &mut restore_terminal,
        vr_ide::commands::search::EXIT_WAIT,
    )
}

fn run(app: &mut App) -> Result<()> {
    let mut terminal: Terminal<CrosstermBackend<Stdout>> =
        Terminal::new(CrosstermBackend::new(io::stdout()))?;
    // The first frame goes out before the keyboard query: a terminal that never answers it
    // makes crossterm wait up to 2 s.
    terminal.draw(|f| ui::render(f, app))?;
    if std::env::var_os("VR_IDE_EXIT_AFTER_FIRST_FRAME").is_some() {
        return Ok(());
    }
    // After the first frame: the index walks the project tree in the background; the folder
    // watches are one per shown folder.
    app.watch_root();
    app.start_index();
    app.start_git();
    app.remember_workspace();
    // Without the kitty protocol, ctrl+shift+<letter>, ctrl+/ and similar chords cannot be
    // told apart; the startup notice lists the affected keys.
    let enhanced = matches!(supports_keyboard_enhancement(), Ok(true))
        && execute!(
            io::stdout(),
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
                    // Labels auto-repeat, so a held quit / close key cannot confirm itself.
                    | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
            )
        )
        .is_ok();
    ENHANCED.store(enhanced, Ordering::SeqCst);
    app.report_keyboard(enhanced);
    event_loop::run(&mut terminal, app)
}
