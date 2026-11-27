//! The seam between the app and what shows it. The app core takes crossterm event types from
//! every frontend and draws through a ratatui backend; what differs between a terminal and a
//! native window (title, cursor shape, the IME candidate window, attention, keyboard reporting)
//! goes through [`Frontend`]. [`Crossterm`] is the terminal one, together with the terminal
//! setup and restore `main` uses.

use std::io::{self, Stdout, Write};
use std::sync::atomic::{AtomicBool, Ordering};

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
use ratatui::backend::{CrosstermBackend, TestBackend};
use ratatui::layout::{Position, Rect};

/// The text cursor's shape: block, bar or underline, steady or blinking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorShape {
    SteadyBlock,
    BlinkingBlock,
    SteadyBar,
    BlinkingBar,
    SteadyUnderline,
    BlinkingUnderline,
}

impl CursorShape {
    /// The shape a crossterm cursor style names. The user's own shape is unknown here; it
    /// counts as the blinking block terminals start with.
    pub fn from_crossterm(style: SetCursorStyle) -> Self {
        match style {
            SetCursorStyle::DefaultUserShape | SetCursorStyle::BlinkingBlock => Self::BlinkingBlock,
            SetCursorStyle::SteadyBlock => Self::SteadyBlock,
            SetCursorStyle::BlinkingUnderScore => Self::BlinkingUnderline,
            SetCursorStyle::SteadyUnderScore => Self::SteadyUnderline,
            SetCursorStyle::BlinkingBar => Self::BlinkingBar,
            SetCursorStyle::SteadyBar => Self::SteadyBar,
        }
    }

    pub fn to_crossterm(self) -> SetCursorStyle {
        match self {
            Self::SteadyBlock => SetCursorStyle::SteadyBlock,
            Self::BlinkingBlock => SetCursorStyle::BlinkingBlock,
            Self::SteadyBar => SetCursorStyle::SteadyBar,
            Self::BlinkingBar => SetCursorStyle::BlinkingBar,
            Self::SteadyUnderline => SetCursorStyle::SteadyUnderScore,
            Self::BlinkingUnderline => SetCursorStyle::BlinkingUnderScore,
        }
    }
}

/// What a frontend does for the app besides delivering events and drawing cells.
/// [`crate::event_loop::step`] calls these only when the value changed.
pub trait Frontend {
    /// The window title. An error leaves it counted as not written, so the next step tries again.
    fn set_title(&mut self, title: &str) -> io::Result<()>;
    /// The text cursor's shape. An error leaves it counted as not written.
    fn set_cursor_shape(&mut self, shape: CursorShape) -> io::Result<()>;
    /// The cell area of the text cursor after a draw (`None`: no cursor shown), where an input
    /// method puts its candidate window.
    fn set_ime_area(&mut self, area: Option<Rect>);
    /// An embedded program rang the bell or sent a notification for an AI session the user is
    /// not viewing (a new attention mark).
    fn request_attention(&mut self);
    /// Whether key events tell apart every modified key (kitty protocol in a terminal);
    /// `App::report_keyboard` takes it, and `chord_from_key` decodes with it.
    fn keyboard_enhanced(&self) -> bool;
}

/// The cell where the last draw left the text cursor shown, as the backend knows it without
/// asking the device. `None` when the cursor is hidden or the backend cannot tell cheaply.
pub trait DrawnCursor {
    fn drawn_cursor(&self) -> Option<Position>;
}

/// A terminal places the input method's window at its own cursor; asking it for the cursor
/// position is a round trip, so it is not done.
impl<W: Write> DrawnCursor for CrosstermBackend<W> {
    fn drawn_cursor(&self) -> Option<Position> {
        None
    }
}

impl DrawnCursor for TestBackend {
    fn drawn_cursor(&self) -> Option<Position> {
        self.cursor_visible().then(|| self.cursor_position())
    }
}

/// The terminal frontend: escape sequences on `out` (stdout in the app).
pub struct Crossterm<W: Write = Stdout> {
    out: W,
    enhanced: bool,
}

impl Crossterm<Stdout> {
    /// The frontend on stdout; `enhanced` is the result of [`enhance_keyboard`].
    pub fn stdout(enhanced: bool) -> Self {
        Self::new(io::stdout(), enhanced)
    }
}

impl<W: Write> Crossterm<W> {
    pub fn new(out: W, enhanced: bool) -> Self {
        Self { out, enhanced }
    }

    pub fn into_inner(self) -> W {
        self.out
    }
}

impl<W: Write> Frontend for Crossterm<W> {
    fn set_title(&mut self, title: &str) -> io::Result<()> {
        execute!(self.out, SetTitle(title))
    }

    /// Terminals without cursor shapes ignore the sequence; [`restore_terminal`] restores the
    /// user's shape on exit.
    fn set_cursor_shape(&mut self, shape: CursorShape) -> io::Result<()> {
        execute!(self.out, shape.to_crossterm())
    }

    fn set_ime_area(&mut self, _area: Option<Rect>) {}

    /// The bell already reached the terminal through the AI session's own notice.
    fn request_attention(&mut self) {}

    fn keyboard_enhanced(&self) -> bool {
        self.enhanced
    }
}

/// Set once the kitty keyboard flags were pushed, so restoring pops them (also from the
/// panic hook).
static ENHANCED: AtomicBool = AtomicBool::new(false);

/// Raw mode, alternate screen, mouse capture, bracketed paste and the first window title.
/// When the setup after raw mode fails, the terminal is restored before the error returns.
pub fn enter_terminal(title: &str) -> io::Result<()> {
    enable_raw_mode()?;
    let setup = execute!(
        io::stdout(),
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste,
        SetTitle(title)
    );
    if let Err(e) = setup {
        restore_terminal();
        return Err(e);
    }
    Ok(())
}

/// Undoes [`enter_terminal`] and [`enhance_keyboard`]; safe to call more than once and from
/// the panic hook.
pub fn restore_terminal() {
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

/// Turns on the kitty keyboard protocol when the terminal has it. Without it,
/// ctrl+shift+<letter>, ctrl+/ and similar chords cannot be told apart. Asks the terminal and
/// waits for the answer (up to 2 s in crossterm), so it runs after the first frame.
pub fn enhance_keyboard() -> bool {
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
    enhanced
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Records what `step` gives the frontend; `fail_title` makes title writes fail.
    #[derive(Default)]
    pub(crate) struct FakeFrontend {
        pub(crate) titles: Vec<String>,
        pub(crate) shapes: Vec<CursorShape>,
        pub(crate) ime: Vec<Option<Rect>>,
        pub(crate) attention: usize,
        pub(crate) fail_title: bool,
    }

    impl Frontend for FakeFrontend {
        fn set_title(&mut self, title: &str) -> std::io::Result<()> {
            if self.fail_title {
                return Err(std::io::Error::other("gone"));
            }
            self.titles.push(title.to_string());
            Ok(())
        }
        fn set_cursor_shape(&mut self, shape: CursorShape) -> std::io::Result<()> {
            self.shapes.push(shape);
            Ok(())
        }
        fn set_ime_area(&mut self, area: Option<Rect>) {
            self.ime.push(area);
        }
        fn request_attention(&mut self) {
            self.attention += 1;
        }
        fn keyboard_enhanced(&self) -> bool {
            true
        }
    }

    const ALL: [CursorShape; 6] = [
        CursorShape::SteadyBlock,
        CursorShape::BlinkingBlock,
        CursorShape::SteadyBar,
        CursorShape::BlinkingBar,
        CursorShape::SteadyUnderline,
        CursorShape::BlinkingUnderline,
    ];

    #[test]
    fn cursor_shapes_round_trip_through_crossterm() {
        for s in ALL {
            assert_eq!(CursorShape::from_crossterm(s.to_crossterm()), s);
        }
    }

    /// The terminal frontend writes the same sequences the event loop wrote before the seam.
    #[test]
    fn crossterm_frontend_writes_title_and_cursor_sequences() {
        let mut f = Crossterm::new(Vec::new(), true);
        f.set_title("vr-ide — a").unwrap();
        f.set_cursor_shape(CursorShape::BlinkingBar).unwrap();
        f.set_ime_area(Some(Rect::new(1, 1, 1, 1)));
        f.request_attention();
        assert!(f.keyboard_enhanced());
        let mut want = Vec::new();
        execute!(want, SetTitle("vr-ide — a"), SetCursorStyle::BlinkingBar).unwrap();
        assert_eq!(f.into_inner(), want);
    }

    #[test]
    fn test_backend_reports_the_shown_cursor_only() {
        use ratatui::backend::Backend;
        let mut b = TestBackend::new(10, 3);
        b.set_cursor_position((4, 2)).unwrap();
        assert_eq!(b.drawn_cursor(), None, "hidden");
        b.show_cursor().unwrap();
        assert_eq!(b.drawn_cursor(), Some(Position::new(4, 2)));
    }
}
