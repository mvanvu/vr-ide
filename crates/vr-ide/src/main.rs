use std::io::{self, Stdout};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use vr_ide::app::App;
use vr_ide::frontend::{self, Crossterm, Frontend, restore_terminal};
use vr_ide::{cli, clipboard, event_loop, recovery, session, ui};

fn main() -> Result<()> {
    let mut argv = std::env::args_os();
    let argv0 = argv.next().unwrap_or_default();
    let mut args: Vec<_> = argv.collect();
    // An account's login launcher (claude's `$BROWSER`, SEC-34): known by its own name, before
    // everything else; under that name the editor never starts.
    match cli::login_launcher(&argv0, &args) {
        cli::LauncherCall::No => {}
        cli::LauncherCall::Refused => {
            eprintln!("vr-ide: a login launcher takes exactly one argument");
            std::process::exit(2);
        }
        cli::LauncherCall::Run(l) => {
            let code = vr_ide::ai::login::run_launcher(&l.state, &l.tool, &l.account, &l.target);
            std::process::exit(code);
        }
    }
    // The login window process the launcher starts: the same place, before any terminal setup.
    if let Some((tool, account, url)) = cli::login_window(&args) {
        std::process::exit(vr_ide::ai::login::run_helper(&tool, &account, &url));
    }
    if args.first().is_some_and(|a| a == cli::LOGIN_WINDOW_FLAG) {
        eprintln!("usage: vr-ide --login-window <tool> <account> <url>");
        std::process::exit(2);
    }
    // claude's statusLine helper (SEC-23): before the panic hook, session, config and terminal.
    if let Some(report) = cli::ai_statusline(&args) {
        std::process::exit(vr_ide::ai::usage::run_helper(&report));
    }
    // codex's notify program (SEC-32): the same place, the same rules.
    if let Some((report, payload)) = cli::ai_codex_notify(&args) {
        std::process::exit(vr_ide::ai::usage::run_codex_notify(&report, &payload));
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
    // Every editor form takes `--tui`; the helper forms above never open a window.
    let tui_flag = cli::take_tui_flag(&mut args);
    let ui = cli::choose_ui(
        tui_flag,
        cfg!(feature = "window"),
        cli::display_available(|k| std::env::var_os(k)),
    );
    let window = ui == cli::Ui::Window;
    let launch = match cli::resolve(&args, &std::env::current_dir()?) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("vr-ide: {e}");
            std::process::exit(2);
        }
    };

    if window {
        // The window draws any colour, whatever COLORTERM the launcher had.
        vr_ide::theme::ColorDepth::use_true_color();
    }
    recovery::install_panic_hook(vr_config::state_dir().join("recovery"), restore_if_entered);

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
    // The window has no OSC 52 fallback: there is no terminal to read it.
    let clipboard: Box<dyn clipboard::Clipboard> = if window {
        Box::new(clipboard::WindowClipboard::new())
    } else {
        clipboard::system()
    };
    let (mut app, warnings) = App::load_workspace(workspace, clipboard);
    if !window {
        enter_terminal(&app)?;
    }
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

    #[cfg(feature = "window")]
    if window {
        let recovery_dir = vr_config::state_dir().join("recovery");
        match vr_ide::window_frontend::run(&mut app, &recovery_dir) {
            Ok(run) => {
                return event_loop::exit_with(
                    run.result,
                    &mut app,
                    &recovery_dir,
                    &mut restore_if_entered,
                    &mut || run.sigterm.enable(),
                    vr_ide::commands::search::EXIT_WAIT,
                );
            }
            Err(e) => {
                eprintln!("vr-ide: {e}; running in the terminal instead");
                app.clipboard = clipboard::system();
                vr_ide::theme::ColorDepth::use_terminal_depth();
                enter_terminal(&app)?;
            }
        }
    }

    let result = run(&mut app);
    // Terminal and recovery first, then the bounded wait for a project replace, then the
    // exit messages (readable now that the alternate screen is gone).
    event_loop::exit(
        result,
        &mut app,
        &vr_config::state_dir().join("recovery"),
        &mut restore_if_entered,
        vr_ide::commands::search::EXIT_WAIT,
    )
}

/// Set once the terminal UI took the terminal: a window run never touches the terminal it was
/// started from, on exit or in the panic hook.
static IN_TERMINAL: AtomicBool = AtomicBool::new(false);

fn enter_terminal(app: &App) -> io::Result<()> {
    IN_TERMINAL.store(true, Ordering::SeqCst);
    frontend::enter_terminal(&app.title())?;
    recovery::GLOBAL.set_tui_active(true);
    Ok(())
}

fn restore_if_entered() {
    if IN_TERMINAL.load(Ordering::SeqCst) {
        restore_terminal();
    }
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
    let mut frontend = Crossterm::stdout(frontend::enhance_keyboard());
    // Without the kitty protocol the startup notice lists the keys that cannot be told apart.
    app.report_keyboard(frontend.keyboard_enhanced());
    event_loop::run(&mut terminal, &mut frontend, app)
}
