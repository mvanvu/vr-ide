//! Integrated terminal backend: a PTY child, an emulator fed by a reader thread, and the
//! key / paste / mouse encodings. The UI only sees this crate's own types.

mod attention;
mod input;
mod marker;
mod snapshot;
mod terminal;
mod types;

pub use attention::Attention;
pub use input::{
    KeyCode, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseKind, encode_key, encode_mouse,
    encode_paste,
};
pub use terminal::{KILL_GRACE, MAX_PENDING_INPUT, Terminal};
pub use types::{
    Cell, CellFlags, Color, Cursor, CursorShape, Grid, Modes, Notify, SpawnExtras, SpawnOptions,
    TermError, TermStatus,
};
