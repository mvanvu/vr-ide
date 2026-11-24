//! Text, selection, transactions, history and document I/O (UI-free).
//!
//! Offsets: the core works in char offsets (ropey is char-indexed), cursor motions step over
//! grapheme clusters, and screen columns are display widths (`display_col`).

pub mod document;
pub mod edit_ops;
pub mod history;
pub mod motion;
pub mod selection;
pub mod text;
pub mod transaction;
pub mod watch;

pub use document::{
    BINARY_SNIFF_LEN, DiskStamp, DiskState, DocError, Document, Encoding, LineEnding, OwnWrites,
    Saved, is_save_temp,
};
pub use history::{Clock, EditKind, History, ManualClock, SystemClock};
pub use ropey;
pub use selection::{Pos, Range, Selection};
pub use text::{Buffer, BufferOptions, display_col};
pub use transaction::{Change, Transaction};
pub use watch::{DEBOUNCE, FsEvent, WatchError, Watcher};
