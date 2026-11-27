use std::io::{self, Stdout};

use anyhow::Result;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use vr_ide::app::App;
use vr_ide::frontend::{self, Crossterm, Frontend, restore_terminal};
use vr_ide::{cli, clipboard, event_loop, recovery, session, ui};

fn main() -> Result<()> {
    let mut argv = std::env::args_os();
    let argv0 = argv.next().unwrap_or_default();
    let args: Vec<_> = argv.collect();
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
    frontend::enter_terminal(&app.title())?;
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
    let mut frontend = Crossterm::stdout(frontend::enhance_keyboard());
    // Without the kitty protocol the startup notice lists the keys that cannot be told apart.
    app.report_keyboard(frontend.keyboard_enhanced());
    event_loop::run(&mut terminal, &mut frontend, app)
}
