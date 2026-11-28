//! The native window frontend (feature `window`): vr-window's runner drives
//! [`crate::event_loop::step`] through [`WindowApp`]; [`WindowFrontend`] turns the step's
//! title, cursor, IME and attention calls into window operations. The app core does not know
//! which frontend shows it.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crossterm::event::Event;
use ratatui::Terminal;
use ratatui::layout::{Position, Rect};
use ratatui::style::Color;
use vr_config::Settings;
use vr_window::app::{StartError, Step, WindowHandler, WindowOps, WindowOptions};
use vr_window::{CursorShape as Drawn, FontConfig, FontError, FontSet, Palette, PixelBackend, rgb};

use crate::app::App;
use crate::commands::CmdArgs;
use crate::desktop::MenuNotice;
use crate::event_loop::{self, StepState};
use crate::frontend::{CursorShape, DrawnCursor, Frontend};
use crate::ui::dialog::Outcome;

/// The window's app id / X11 class: the desktop entry's `StartupWMClass`.
pub const APP_ID: &str = "vr-ide";

/// The first window holds this many cells.
pub const START_COLS: u16 = 120;
pub const START_ROWS: u16 = 36;

/// [`Frontend`] over the window: the backend draws the cursor, the window places the IME.
pub struct WindowFrontend<'a> {
    ops: &'a mut dyn WindowOps,
}

impl<'a> WindowFrontend<'a> {
    pub fn new(ops: &'a mut dyn WindowOps) -> Self {
        Self { ops }
    }
}

/// The backend's cursor shape and whether it blinks.
pub fn drawn_cursor(shape: CursorShape) -> (Drawn, bool) {
    match shape {
        CursorShape::SteadyBlock => (Drawn::Block, false),
        CursorShape::BlinkingBlock => (Drawn::Block, true),
        CursorShape::SteadyBar => (Drawn::Bar, false),
        CursorShape::BlinkingBar => (Drawn::Bar, true),
        CursorShape::SteadyUnderline => (Drawn::Underline, false),
        CursorShape::BlinkingUnderline => (Drawn::Underline, true),
    }
}

impl Frontend for WindowFrontend<'_> {
    fn set_title(&mut self, title: &str) -> std::io::Result<()> {
        self.ops.set_title(title);
        Ok(())
    }

    fn set_cursor_shape(&mut self, shape: CursorShape) -> std::io::Result<()> {
        let (drawn, blinking) = drawn_cursor(shape);
        self.ops.set_cursor(drawn, blinking);
        Ok(())
    }

    fn set_ime_area(&mut self, area: Option<Rect>) {
        self.ops.set_ime_cell(area.map(|r| (r.x, r.y)));
    }

    /// The window decides: only while it is not focused.
    fn request_attention(&mut self) {
        self.ops.request_attention();
    }

    /// Every chord the OS delivers arrives with full modifiers.
    fn keyboard_enhanced(&self) -> bool {
        true
    }
}

/// The backend knows where ratatui put the cursor and whether it is shown.
impl DrawnCursor for PixelBackend {
    fn drawn_cursor(&self) -> Option<Position> {
        self.cursor_visible().then(|| self.cursor_position())
    }
}

/// The font the settings ask for; an empty family is the platform's monospace font.
pub fn font_config(settings: &Settings, scale: f32) -> FontConfig {
    let family = settings.font_family.trim();
    FontConfig {
        family: (!family.is_empty()).then(|| family.to_string()),
        size_pt: settings.font_size,
        line_height: settings.line_height,
        scale,
    }
}

/// The font settings this window last applied.
#[derive(Debug, Clone, PartialEq)]
struct FontSettings {
    family: String,
    size: f32,
    line_height: f32,
}

impl FontSettings {
    fn of(settings: &Settings) -> Self {
        Self {
            family: settings.font_family.clone(),
            size: settings.font_size,
            line_height: settings.line_height,
        }
    }
}

/// Applies the settings' font to the backend (settings reload). A family that is not installed
/// gives the default family at the asked size; the returned text is the warning to show.
pub fn apply_fonts(backend: &mut PixelBackend, settings: &Settings) -> Option<String> {
    let config = font_config(settings, backend.fonts().config().scale);
    match backend.set_font_config(&config) {
        Ok(()) => None,
        Err(e @ FontError::FamilyNotFound(_)) => {
            let default = FontConfig {
                family: None,
                ..config
            };
            let _ = backend.set_font_config(&default);
            Some(font_notice(&e, backend.fonts().config()))
        }
        Err(e) => Some(font_notice(&e, backend.fonts().config())),
    }
}

/// The notice for a font setting the window could not use, worded as the other settings
/// notices; `applied` is the font the window uses instead (its size is named).
pub fn font_notice(error: &FontError, applied: &FontConfig) -> String {
    match error {
        FontError::FamilyNotFound(family) => format!(
            "settings: font_family {family:?} is not installed; using the system monospace font \
             at {} pt",
            applied.size_pt
        ),
        other => format!("settings: {other}"),
    }
}

/// Whether the window's title bar (client-side decorations, the GTK variant on X11) is dark:
/// every theme but `light` paints a dark grid.
pub fn window_dark(theme: &str) -> bool {
    theme != "light"
}

/// The window's colours for what the theme leaves to the terminal (`Color::Reset`, named
/// colours): the theme's own text colours when it has them, light ones for `light`.
pub fn palette_for(theme: &str) -> Palette {
    let mut palette = Palette::default();
    if theme == "light" {
        palette.foreground = rgb(0x1f, 0x23, 0x28);
        palette.background = rgb(0xff, 0xff, 0xff);
        return palette;
    }
    if let Some(t) = crate::theme::Theme::named(theme) {
        if let Some(Color::Rgb(r, g, b)) = t.text.fg {
            palette.foreground = rgb(r, g, b);
        }
        if let Some(Color::Rgb(r, g, b)) = t.text.bg {
            palette.background = rgb(r, g, b);
        }
    }
    palette
}

/// The close button or SIGTERM: what ctrl+q does (unsaved files and running terminals ask
/// first, a running replace refuses), after an open dialog is closed as Esc would close it.
pub fn request_quit(app: &mut App) {
    if let Some(d) = app.dialog.take() {
        crate::commands::dialog::answer(app, d, Outcome::Cancelled);
    }
    app.run_command("app.quit", CmdArgs::None);
    app.dirty = true;
}

/// The first window start rewrites a vr-ide menu entry of the GNOME Terminal form (T5); the
/// result is shown as a notice (a failed write as a warning).
pub fn migrate_menu_entry(app: &mut App, data_home: Option<&Path>, exe: Option<&Path>) {
    let (Some(data_home), Some(exe)) = (data_home, exe) else {
        return;
    };
    match crate::desktop::migrate_on_window_start(data_home, exe) {
        Some(MenuNotice::Warning(text)) => app.warn(text),
        Some(MenuNotice::Info(text)) => app.info(text),
        None => {}
    }
}

/// A cell's size for the PTYs (`TIOCSWINSZ` pixels), saturating.
fn cell_px(backend: &PixelBackend) -> (u16, u16) {
    let cell = backend.layout().cell;
    let px = |v: u32| u16::try_from(v).unwrap_or(u16::MAX);
    (px(cell.width), px(cell.height))
}

/// Gives SIGTERM, SIGINT and SIGHUP their default action (ending the process) back. signal-hook
/// cannot take its handler off a signal, and an unregistered signal would stay ignored; a
/// conditional default registered with the window does it once this is switched on.
#[derive(Debug, Clone, Default)]
pub struct SigtermDefault(Arc<AtomicBool>);

impl SigtermDefault {
    pub fn enable(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Where the X11 connection-lost hook writes recovery (set by [`run`]).
static RECOVERY_DIR: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// What happens when libX11 is about to end the process on a lost server, as in the panic
/// hook: the published unsaved buffers go to `dir`, a line goes to stderr. Never touches the
/// app (it runs inside libX11's error handler).
pub fn write_recovery_on_display_loss(
    state: &crate::recovery::RecoveryState,
    dir: &Path,
) -> Vec<std::path::PathBuf> {
    let written = crate::recovery::write_recovery(state, dir);
    crate::recovery::note(format_args!("the connection to the display was lost"));
    for p in &written {
        crate::recovery::note(format_args!("unsaved buffer written to {}", p.display()));
    }
    written
}

fn x11_connection_lost() {
    if let Some(dir) = RECOVERY_DIR.get() {
        write_recovery_on_display_loss(&crate::recovery::GLOBAL, dir);
    }
}

/// vr-ide's side of the window runner.
pub struct WindowApp<'a> {
    app: &'a mut App,
    state: StepState,
    /// The loop's result once it failed (drawing, a lost surface).
    result: anyhow::Result<()>,
    close: bool,
    /// Set by SIGTERM and SIGINT (registered once the window is up).
    sigterm: Arc<AtomicBool>,
    /// Switched on once recovery is written after the window: SIGTERM, SIGINT and SIGHUP then
    /// take their default action (end the process).
    sigterm_default: SigtermDefault,
    started: bool,
    /// `VR_IDE_EXIT_AFTER_FIRST_FRAME`: quit after the first frame (smoke runs).
    exit_after_first_frame: bool,
    /// Whether the start registers the signal handlers (the real window; tests opt in).
    signals: bool,
    fonts: FontSettings,
    theme: String,
    /// Where the first start looks for the menu entry to migrate (`None`: skip).
    menu: Option<(std::path::PathBuf, std::path::PathBuf)>,
    /// Warnings found before the window existed (the font), shown once it does.
    start_warnings: Vec<String>,
}

impl<'a> WindowApp<'a> {
    pub fn new(app: &'a mut App) -> Self {
        let fonts = FontSettings::of(&app.settings);
        let theme = app.settings.theme.clone();
        Self {
            app,
            state: StepState::default(),
            result: Ok(()),
            close: false,
            sigterm: Arc::new(AtomicBool::new(false)),
            sigterm_default: SigtermDefault::default(),
            started: false,
            exit_after_first_frame: false,
            signals: false,
            fonts,
            theme,
            menu: None,
            start_warnings: Vec::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn app_mut(&mut self) -> &mut App {
        self.app
    }

    /// The flag SIGTERM and SIGINT set (tests set it by hand).
    pub fn sigterm_flag(&self) -> Arc<AtomicBool> {
        self.sigterm.clone()
    }

    /// The start registers the process signal handlers (see [`WindowApp::start`]).
    pub fn catch_signals(&mut self) {
        self.signals = true;
    }

    /// The switch that gives SIGTERM its default action back (see [`SigtermDefault`]).
    pub fn sigterm_default(&self) -> SigtermDefault {
        self.sigterm_default.clone()
    }

    /// The loop's result, for `event_loop::exit`.
    pub fn take_result(&mut self) -> anyhow::Result<()> {
        std::mem::replace(&mut self.result, Ok(()))
    }

    /// Runs once the window shows: the window's keyboard, the signals, the menu entry.
    fn start(&mut self) {
        // No "keys this terminal cannot send" notice: every chord arrives.
        self.app.report_keyboard(true);
        #[cfg(unix)]
        if self.signals {
            use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
            use signal_hook::flag;
            // signal-hook cannot take its handler back off a signal: an unregistered signal
            // would stay ignored. The conditional default ends the process once the exit wrote
            // recovery (`SigtermDefault`); until then a signal only sets its flag.
            // SIGTERM and ctrl+c in the launching shell (SIGINT) ask the window to quit, with
            // the quit guard. SIGHUP (the launching terminal closed, or the empty terminal an
            // old menu entry left) is not about the window, which never needed that terminal:
            // its flag is never read. A handler rather than SIG_IGN, so the shells of embedded
            // terminals do not inherit an ignored SIGHUP and still end on the shutdown's hangup.
            let hangup = Arc::new(AtomicBool::new(false));
            for (sig, flag_of) in [
                (SIGTERM, &self.sigterm),
                (SIGINT, &self.sigterm),
                (SIGHUP, &hangup),
            ] {
                let _ = flag::register_conditional_default(sig, self.sigterm_default.0.clone());
                let _ = flag::register(sig, flag_of.clone());
            }
        }
        for w in std::mem::take(&mut self.start_warnings) {
            self.app.warn(w);
        }
        if let Some((data_home, exe)) = self.menu.take() {
            migrate_menu_entry(self.app, Some(&data_home), Some(&exe));
        }
    }

    /// A settings reload changed the font or the theme: the grid is laid out again.
    fn follow_settings(&mut self, backend: &mut PixelBackend, ops: &mut dyn WindowOps) {
        let fonts = FontSettings::of(&self.app.settings);
        if fonts != self.fonts {
            self.fonts = fonts;
            if let Some(warning) = apply_fonts(backend, &self.app.settings) {
                self.app.warn(warning);
            }
            self.app.on_resize();
            self.app.dirty = true;
        }
        if self.app.settings.theme != self.theme {
            self.theme = self.app.settings.theme.clone();
            backend.set_palette(palette_for(&self.theme));
            ops.set_theme(window_dark(&self.theme));
            self.app.dirty = true;
        }
    }
}

impl WindowHandler for WindowApp<'_> {
    fn step(
        &mut self,
        terminal: &mut Terminal<PixelBackend>,
        ops: &mut dyn WindowOps,
        events: Vec<Event>,
        now: Instant,
    ) -> Step {
        if self.result.is_err() {
            return Step {
                quit: true,
                next_wake: Duration::ZERO,
            };
        }
        if !self.started {
            self.start();
        }
        if std::mem::take(&mut self.close) || self.sigterm.swap(false, Ordering::SeqCst) {
            request_quit(self.app);
        }
        // Nothing typed after a quit that needs no question reaches the app.
        let events = if self.app.quit { Vec::new() } else { events };
        self.follow_settings(terminal.backend_mut(), ops);
        self.app.cell_px = cell_px(terminal.backend());
        let mut frontend = WindowFrontend::new(ops);
        let out = event_loop::step(
            terminal,
            &mut frontend,
            &mut self.state,
            self.app,
            events,
            now,
        );
        let out = match out {
            Ok(out) => out,
            Err(e) => {
                self.result = Err(e);
                return Step {
                    quit: true,
                    next_wake: Duration::ZERO,
                };
            }
        };
        if !self.started {
            self.started = true;
            if self.exit_after_first_frame {
                return Step {
                    quit: true,
                    next_wake: out.next_wake,
                };
            }
            // After the first frame, as in the terminal: the index walks the project tree in
            // the background; the folder watches are one per shown folder.
            self.app.watch_root();
            self.app.start_index();
            self.app.start_git();
            self.app.remember_workspace();
        }
        Step {
            quit: out.quit,
            next_wake: out.next_wake,
        }
    }

    fn close_requested(&mut self) {
        self.close = true;
    }

    fn failed(&mut self, error: String) {
        if self.result.is_ok() {
            self.result = Err(anyhow::anyhow!(error));
        }
    }
}

/// How a window run ended.
pub struct WindowRun {
    /// The loop's result, for `event_loop::exit`.
    pub result: anyhow::Result<()>,
    /// Switch on once recovery is written (`event_loop::exit_with`): until then SIGTERM still
    /// only sets the window's quit flag.
    pub sigterm: SigtermDefault,
}

/// Runs the app in its own window until it quits. `Err` when the window could not be shown
/// (the caller falls back to the terminal; nothing of the window touched the app).
/// `recovery_dir` is where a lost X11 server still gets the unsaved buffers written.
pub fn run(app: &mut App, recovery_dir: &Path) -> Result<WindowRun, StartError> {
    let _ = RECOVERY_DIR.set(recovery_dir.to_path_buf());
    vr_window::app::on_x11_connection_lost(x11_connection_lost);
    let (fonts, font_warning) = FontSet::new(&font_config(&app.settings, 1.0));
    let options = WindowOptions {
        title: app.title(),
        app_id: APP_ID.to_string(),
        fonts,
        palette: palette_for(&app.settings.theme),
        dark: window_dark(&app.settings.theme),
        cols: START_COLS,
        rows: START_ROWS,
    };
    let mut handler = WindowApp::new(app);
    handler.catch_signals();
    handler.exit_after_first_frame = std::env::var_os("VR_IDE_EXIT_AFTER_FIRST_FRAME").is_some();
    // The title is set with the window.
    handler.state = StepState::with_title(options.title.clone());
    handler.menu = crate::desktop::data_home()
        .ok()
        .zip(std::env::current_exe().ok());
    handler
        .start_warnings
        .extend(font_warning.map(|w| font_notice(&w, options.fonts.config())));
    vr_window::app::run(options, &mut handler)?;
    Ok(WindowRun {
        result: handler.take_result(),
        sigterm: handler.sigterm_default(),
    })
}
