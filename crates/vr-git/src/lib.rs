//! Read-only git access for the editor (UI-free).
//!
//! Every query runs the system `git` CLI through one hardened runner ([`cmd`]): no shell, an
//! absolute git path, a new session without a controlling terminal, a filtered environment,
//! config overrides that stop repo or user config from running programs, machine-readable
//! output formats only, a timeout and an output cap. `gix` was measured and not used (see the
//! M7-1 notes): the CLI is complete for blame/log --follow/status and its trust model
//! (`safe.directory`) is the one users already configure.
//!
//! Paths in results are repo-relative and keep their raw bytes (`PathBuf` built from `-z`
//! output), so non-UTF-8 names round-trip; use `to_string_lossy` for display only.
//! [`GitWorker`] runs queries on a background thread with versioned, coalesced requests and
//! a cache invalidated on HEAD/index/file change.

mod blame;
mod cmd;
mod diff;
mod log;
mod parse;
mod repo;
mod worker;

pub use blame::{Blame, BlameCommit, BlameLine};
pub use cmd::Options;
pub use diff::{ChangeKind, DiffLine, FileDiff, Hunk, LineChange, LineKind};
pub use log::{CommitInfo, CommitSummary, Signature};
pub use repo::{AheadBehind, FileStatus, Head, Repo, Status, StatusEntry};
pub use worker::{GitWorker, Reply, Request, Response};

use std::path::PathBuf;

/// Errors of git queries. `Clone` so the worker can cache and resend results.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GitError {
    #[error("git executable not found in PATH")]
    GitNotFound,
    #[error("path is outside the repository: {}", .0.display())]
    OutsideRepo(PathBuf),
    #[error("not a commit: {0}")]
    NotACommit(String),
    #[error("git {0} timed out")]
    Timeout(String),
    #[error("git {cmd} output is larger than {limit} bytes")]
    TooLarge { cmd: String, limit: usize },
    #[error("git {cmd} failed ({code}): {stderr}")]
    Failed {
        cmd: String,
        code: i32,
        stderr: String,
    },
    #[error("could not parse git {cmd} output: {what}")]
    Parse { cmd: String, what: String },
    #[error("i/o error: {0}")]
    Io(String),
    #[error("request superseded by a newer one")]
    Superseded,
    #[error("git access was shut down")]
    ShutDown,
}

impl From<std::io::Error> for GitError {
    fn from(e: std::io::Error) -> Self {
        GitError::Io(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, GitError>;
