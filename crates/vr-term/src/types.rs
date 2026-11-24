//! Plain data types the UI consumes; nothing here depends on the emulator crate.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

/// Called from the reader / waiter threads after new output, a title change or the child's exit.
/// Calls are coalesced: after one call no other comes until the UI takes a [`Grid`] snapshot,
/// so the callback only needs to wake the UI loop (it must not block).
pub type Notify = Arc<dyn Fn() + Send + Sync>;

/// A terminal colour. The UI maps `Default` and `Indexed(0..16)` through its theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Color {
    /// The terminal's default foreground (for `fg`) or background (for `bg`).
    #[default]
    Default,
    /// The 256-colour palette: 0–15 are the ANSI colours, 16–255 the cube and greys.
    Indexed(u8),
    /// A 24-bit truecolor value.
    Rgb(u8, u8, u8),
}

/// Text attributes of one cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CellFlags {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub dim: bool,
    pub hidden: bool,
    pub strikeout: bool,
    /// The cell holds a double-width character; the next cell is its spacer.
    pub wide: bool,
    /// The cell is the right half of a wide character (or the padding before a wide character
    /// that did not fit the line); the UI draws nothing there.
    pub wide_spacer: bool,
}

/// One grid cell.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Cell {
    /// The grapheme in this cell: the base character plus any combining marks. A space when empty,
    /// an empty string for a wide-character spacer.
    pub ch: String,
    pub fg: Color,
    pub bg: Color,
    pub flags: CellFlags,
}

/// Cursor shape requested by the program (DECSCUSR).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CursorShape {
    #[default]
    Block,
    Underline,
    Beam,
    HollowBlock,
}

/// Cursor position in the snapshot's viewport (0-based).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    pub shape: CursorShape,
    /// False when the program hid the cursor or it is scrolled out of the viewport.
    pub visible: bool,
}

/// Terminal modes the UI needs for input encoding and scrolling decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modes {
    pub alt_screen: bool,
    /// Any mouse reporting mode (1000, 1002 or 1003) is on.
    pub mouse_reporting: bool,
    /// Button-event tracking (1002): drags are reported.
    pub mouse_drag: bool,
    /// Any-event tracking (1003): every motion is reported.
    pub mouse_motion: bool,
    /// SGR extended mouse encoding (1006).
    pub sgr_mouse: bool,
    pub bracketed_paste: bool,
    /// DECCKM: cursor keys send `ESC O` sequences.
    pub app_cursor: bool,
    /// DECKPAM: application keypad.
    pub app_keypad: bool,
}

/// A rendered view of the terminal.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Grid {
    pub cols: usize,
    pub rows: usize,
    /// Row-major, `rows * cols` cells.
    pub cells: Vec<Cell>,
    pub cursor: Cursor,
    /// The title set by the program (OSC 0/2), if any.
    pub title: Option<String>,
    pub modes: Modes,
    /// Lines of history above the screen (0 on the alternate screen).
    pub scrollback_len: usize,
    /// The scroll offset actually used (the request clamped to `scrollback_len`).
    pub scroll_offset: usize,
}

impl Grid {
    /// The cell at `row`, `col`; `None` outside the grid.
    pub fn cell(&self, row: usize, col: usize) -> Option<&Cell> {
        if row < self.rows && col < self.cols {
            self.cells.get(row * self.cols + col)
        } else {
            None
        }
    }

    /// The text of one row: cell graphemes joined, spacers skipped, trailing blanks trimmed.
    pub fn row_text(&self, row: usize) -> String {
        let mut text = String::new();
        if row >= self.rows {
            return text;
        }
        for cell in &self.cells[row * self.cols..(row + 1) * self.cols] {
            if !cell.flags.wide_spacer {
                text.push_str(&cell.ch);
            }
        }
        text.truncate(text.trim_end().len());
        text
    }
}

/// Lifecycle of the child process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TermStatus {
    Running,
    /// The child was reaped. A signal death reports `128 + signal`, as shells do.
    Exited(i32),
}

/// What to run and how big the terminal starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnOptions {
    /// Program to run; a bare name is looked up on `PATH`.
    pub shell: PathBuf,
    pub args: Vec<OsString>,
    /// Working directory; must be an existing directory.
    pub cwd: PathBuf,
    /// Extra environment on top of the inherited one (`TERM` is set first and may be
    /// overridden here; `COLORTERM` is only what the caller gives).
    pub env: Vec<(OsString, OsString)>,
    pub cols: u16,
    pub rows: u16,
    /// Lines of history kept above the screen.
    pub scrollback: usize,
}

/// Errors from spawning or driving a terminal.
#[derive(Debug, thiserror::Error)]
pub enum TermError {
    #[error("working directory {0} is not a directory")]
    BadCwd(PathBuf),
    #[error("could not open a pty: {0}")]
    Pty(String),
    #[error("could not start {shell}: {reason}")]
    Spawn { shell: String, reason: String },
    #[error("terminal size {cols}x{rows} is invalid")]
    BadSize { cols: u16, rows: u16 },
    #[error("the terminal has closed")]
    Closed,
    #[error("the program is not reading its input ({0} bytes pending)")]
    InputBacklog(usize),
    #[error("pty error: {0}")]
    Io(#[from] std::io::Error),
}
