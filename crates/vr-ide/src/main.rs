use std::io::{self, Stdout};
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
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
use vr_ide::{cli, clipboard, event_loop, recovery, ui};

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
        DisableBracketedPaste,
        DisableMouseCapture,
        LeaveAlternateScreen
    );
    let _ = disable_raw_mode();
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let launch = match cli::resolve(&args, &std::env::current_dir()?) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("vr-ide: {e}");
            std::process::exit(2);
        }
    };

    recovery::install_panic_hook(vr_config::state_dir().join("recovery"), restore_terminal);

    let (mut app, warnings) = App::load(launch.root.clone(), clipboard::system());
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
    app.report_config(warnings);
    app.start_watcher();
    if let Some(file) = &launch.open_file
        && let Err(e) = app.open_path(file)
    {
        app.error(e.to_string());
    }

    let result = run(&mut app);
    restore_terminal();
    // Only now can the user read them (the alternate screen is gone).
    for m in &app.exit_messages {
        eprintln!("vr-ide: {m}");
    }
    event_loop::finish(result, &app, &vr_config::state_dir().join("recovery"))
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
