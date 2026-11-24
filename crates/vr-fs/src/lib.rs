//! File tree, file operations and fuzzy file index.

pub mod file_index;
pub mod ops;
pub mod tree;

pub use file_index::{FileIndex, IndexMsg};
pub use ops::{
    BatchResult, FakeTrash, FsOpError, SystemTrash, Trash, ensure_within, validate_name,
};
pub use tree::{Node, NodeKind, SymlinkTarget, Tree};
