mod cli;

use std::io::{self, Stdout};
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::widgets::Paragraph;

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(io::stdout(), LeaveAlternateScreen);
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

    // Leave the terminal usable before the default hook prints the panic message.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        default_hook(info);
    }));

    enable_raw_mode()?;
    if let Err(e) = execute!(io::stdout(), EnterAlternateScreen) {
        restore_terminal();
        return Err(e.into());
    }
    let result = run(&launch);
    restore_terminal();
    result
}

fn run(launch: &cli::Launch) -> Result<()> {
    let mut terminal: Terminal<CrosstermBackend<Stdout>> =
        Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut text = format!("vr-ide — {}", launch.root.display());
    if let Some(f) = &launch.open_file {
        text.push_str(&format!("\nopen: {}", f.display()));
    }
    let exit_after_first = std::env::var_os("VR_IDE_EXIT_AFTER_FIRST_FRAME").is_some();
    loop {
        terminal.draw(|frame| frame.render_widget(Paragraph::new(text.as_str()), frame.area()))?;
        if exit_after_first {
            return Ok(());
        }
        if event::poll(Duration::from_millis(250))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && key.code == KeyCode::Char('q')
        {
            return Ok(());
        }
    }
}
