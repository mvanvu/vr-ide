//! In-progress operations and unmerged entries (threat model SEC-14).

use std::path::PathBuf;

use crate::Result;
use crate::parse::{fields, os, path_from};
use crate::repo::Repo;

/// A multi-step git operation left in progress (usually by a conflict).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Merge,
    Rebase,
    /// `git am` (shares `rebase-apply/` with the old rebase backend).
    Am,
    CherryPick,
    Revert,
}

impl Operation {
    pub fn name(self) -> &'static str {
        match self {
            Operation::Merge => "merge",
            Operation::Rebase => "rebase",
            Operation::Am => "am",
            Operation::CherryPick => "cherry-pick",
            Operation::Revert => "revert",
        }
    }
}

/// What the repo is in the middle of; the write side refuses unsafe actions while it lasts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoState {
    pub operation: Option<Operation>,
    /// Paths with unmerged index entries (stages 1-3), each once.
    pub conflicts: Vec<PathBuf>,
}

impl RepoState {
    pub fn is_clean(&self) -> bool {
        self.operation.is_none() && self.conflicts.is_empty()
    }

    pub fn is_conflicted(&self, rel: &std::path::Path) -> bool {
        self.conflicts.iter().any(|c| c == rel)
    }
}

impl std::fmt::Display for RepoState {
    /// e.g. `merge, 1 conflict`, `rebase`, `2 conflicts`, `clean`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let n = self.conflicts.len();
        let conflicts = match n {
            0 => None,
            1 => Some("1 conflict".to_string()),
            n => Some(format!("{n} conflicts")),
        };
        match (self.operation, conflicts) {
            (None, None) => f.write_str("clean"),
            (Some(op), None) => f.write_str(op.name()),
            (None, Some(c)) => f.write_str(&c),
            (Some(op), Some(c)) => write!(f, "{}, {c}", op.name()),
        }
    }
}

impl Repo {
    /// The operation in progress (from the markers git leaves in the per-worktree git dir) and
    /// the unmerged paths.
    pub fn state(&self) -> Result<RepoState> {
        let g = self.git_dir();
        let operation = if g.join("rebase-merge").is_dir() {
            Some(Operation::Rebase)
        } else if g.join("rebase-apply").is_dir() {
            Some(if g.join("rebase-apply/applying").exists() {
                Operation::Am
            } else {
                Operation::Rebase
            })
        } else if g.join("MERGE_HEAD").is_file() {
            Some(Operation::Merge)
        } else if g.join("CHERRY_PICK_HEAD").is_file() {
            Some(Operation::CherryPick)
        } else if g.join("REVERT_HEAD").is_file() {
            Some(Operation::Revert)
        } else {
            None
        };
        let out = self.run(&[os("ls-files"), os("-z"), os("--unmerged")], false)?;
        let mut conflicts: Vec<PathBuf> = Vec::new();
        for rec in fields(&out) {
            // "<mode> <oid> <stage>\t<path>"
            if let Some(tab) = rec.iter().position(|&b| b == b'\t') {
                let p = path_from(&rec[tab + 1..]);
                if !conflicts.contains(&p) {
                    conflicts.push(p);
                }
            }
        }
        Ok(RepoState {
            operation,
            conflicts,
        })
    }
}
