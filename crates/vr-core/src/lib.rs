//! Text, selection, transactions, history and document I/O (UI-free).
//!
//! Offsets: the core works in char offsets (ropey is char-indexed), cursor motions step over
//! grapheme clusters, and screen columns are display widths (`display_col`).

pub mod edit_ops;
pub mod history;
pub mod motion;
pub mod selection;
pub mod text;
pub mod transaction;
// T4 adds `document` (load/save/encoding) and `watch` (file watching) here.

pub use history::{Clock, EditKind, History, ManualClock, SystemClock};
pub use ropey;
pub use selection::{Pos, Range, Selection};
pub use text::{Buffer, BufferOptions, display_col};
pub use transaction::{Change, Transaction};
