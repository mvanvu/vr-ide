//! Git access for the editor (UI-free): reads, and the user's own writes through [`Writer`].
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
//!
//! Writes (stage, unstage, hunk and line patches, discard with a recovery copy, commit,
//! push, pull) go through [`Writer`], which owns a runner of its own (a reader's shutdown never
//! kills a write) and runs one write at a time per repository.

mod blame;
mod cmd;
mod diff;
mod log;
mod parse;
mod patch;
mod programs;
mod recovery;
mod remote;
mod repo;
mod sanitize;
mod state;
mod worker;
mod write;

pub use blame::{Blame, BlameCommit, BlameLine};
pub use cmd::{Cancel, Options};
pub use diff::{ChangeKind, DiffLine, FileDiff, Hunk, LineChange, LineKind};
pub use log::{CommitInfo, CommitSummary, Signature};
pub use remote::{Job, JobEvent, JobOutcome, PushTarget};
pub use repo::{AheadBehind, FileStatus, Head, Repo, Status, StatusEntry};
pub use sanitize::{redact_userinfo, sanitize_line};
pub use state::{Operation, RepoState};
pub use worker::{GitWorker, Reply, Request, Response};
pub use write::{
    CommitOptions, DiscardPlan, Discarded, LeftoverDiscard, Writer, leftover_discards,
};

use std::path::PathBuf;

/// Errors of git queries. `Clone` so the worker can cache and resend results.
///
/// The texts carry repository paths and git's own words, which a hostile repository controls.
/// `Display` passes the path-bearing variants (`OutsideRepo`, `NotACommit`, `Io`, `Refused`,
/// `Stale`) through [`sanitize_line`]. Even so, the UI runs every error text through
/// [`sanitize_line`] at display time, including `Failed` from a read and any field it reads
/// directly from a variant: no caller relies on the producer having done it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GitError {
    #[error("git executable not found in PATH")]
    GitNotFound,
    #[error("path is outside the repository: {}", sanitize_line(&.0.to_string_lossy()))]
    OutsideRepo(PathBuf),
    #[error("not a commit: {}", sanitize_line(.0))]
    NotACommit(String),
    #[error("git {0} timed out")]
    Timeout(String),
    #[error("git {cmd} output is larger than {limit} bytes")]
    TooLarge { cmd: String, limit: usize },
    /// git exited non-zero. From a [`Writer`] call `stderr` is already passed through
    /// [`sanitize_line`] line by line (push and pull also redact URL credentials); from a read
    /// it is git's raw text, so the UI runs it through [`sanitize_line`] before showing it.
    #[error("git {cmd} failed ({code}): {stderr}")]
    Failed {
        cmd: String,
        code: i32,
        stderr: String,
    },
    #[error("could not parse git {cmd} output: {what}")]
    Parse { cmd: String, what: String },
    #[error("i/o error: {}", sanitize_line(.0))]
    Io(String),
    #[error("request superseded by a newer one")]
    Superseded,
    #[error("git access was shut down")]
    ShutDown,
    #[error("cancelled")]
    Cancelled,
    /// A write vr-ide does not perform in this state (conflict, binary file, unsaved buffer,
    /// ...); the text says why and what to do instead.
    #[error("{}", sanitize_line(.0))]
    Refused(String),
    /// What the user saw is no longer what is on disk or in the index; rescan and retry.
    #[error("{} changed, rescan", sanitize_line(.0))]
    Stale(String),
}

impl From<std::io::Error> for GitError {
    fn from(e: std::io::Error) -> Self {
        GitError::Io(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, GitError>;
