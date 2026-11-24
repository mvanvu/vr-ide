//! In-buffer find/replace and project search/replace (UI-free).
//!
//! Matches never span lines. A line ends at `\n`, `\r\n` or a lone `\r` (D31), and the empty
//! "line" after a final line break does not exist (as in `grep -n` and `sed`). In-buffer
//! results are char offsets into the rope; project results carry 1-based line and column.

mod find;
mod project;
mod replace;

pub use find::{find_all, replace_all_tx};
pub use project::{ProjectMatch, SearchMsg, search_project, search_roots};
pub use replace::{FilePreview, Hunk, apply_replace, plan_replace, plan_replace_roots};

/// What to look for. In literal mode the pattern and the replacement are taken verbatim;
/// in regex mode the replacement expands `$1`, `${name}` and `$0`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    pub pattern: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum QueryError {
    #[error("invalid regex: {0}")]
    InvalidRegex(String),
}
