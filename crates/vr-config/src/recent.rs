//! Recently opened workspaces (`state_dir()/recent.toml`): the last [`RECENT_WORKSPACES_MAX`]
//! root sets or workspace files, newest first.
//!
//! The file is state, not configuration: a file that cannot be parsed is reported once and
//! replaced by a fresh list on the next write. Writes go to a temporary file in the same
//! folder that is renamed over the old one, so a crash or a second vr-ide writing at the same
//! time never leaves a half-written file (the later writer's list wins).

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ConfigWarning;

/// File name of the list inside the state directory.
pub const RECENT_FILE: &str = "recent.toml";

/// Workspaces kept in the list.
pub const RECENT_WORKSPACES_MAX: usize = 10;

/// One opened workspace: its workspace file (when it was opened from one) and its roots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentWorkspace {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<PathBuf>,
    pub roots: Vec<PathBuf>,
}

impl RecentWorkspace {
    /// The same workspace: the same file, or (both without a file) the same roots in any
    /// order.
    pub fn same(&self, other: &RecentWorkspace) -> bool {
        match (&self.file, &other.file) {
            (Some(a), Some(b)) => a == b,
            (None, None) => {
                let (mut a, mut b) = (self.roots.clone(), other.roots.clone());
                a.sort();
                b.sort();
                a == b
            }
            _ => false,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct FileFormat {
    #[serde(default)]
    workspace: Vec<RecentWorkspace>,
}

/// The recent-workspaces list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecentWorkspaces {
    pub entries: Vec<RecentWorkspace>,
}

impl RecentWorkspaces {
    /// Reads `path`. A missing file is an empty list; an unreadable or corrupt one is an
    /// empty list plus a warning (it is replaced on the next [`RecentWorkspaces::save`]).
    pub fn load(path: &Path) -> (RecentWorkspaces, Option<ConfigWarning>) {
        let mut warnings = Vec::new();
        let Some(text) = crate::read_user_file(path, &mut warnings) else {
            return (RecentWorkspaces::default(), warnings.pop());
        };
        match toml::from_str::<FileFormat>(&text) {
            Ok(f) => {
                let mut list = RecentWorkspaces::default();
                // Oldest first, so the newest ends up in front and duplicates collapse.
                for e in f.workspace.into_iter().rev() {
                    if !e.roots.is_empty() {
                        list.push(e);
                    }
                }
                (list, None)
            }
            Err(e) => (
                RecentWorkspaces::default(),
                Some(ConfigWarning::Malformed {
                    file: path.to_owned(),
                    message: format!(
                        "{}; the list starts fresh",
                        crate::toml_error_message(&text, &e)
                    ),
                }),
            ),
        }
    }

    /// Puts `entry` in front (removing an older copy of the same workspace) and keeps the
    /// newest [`RECENT_WORKSPACES_MAX`].
    pub fn push(&mut self, entry: RecentWorkspace) {
        self.entries.retain(|e| !e.same(&entry));
        self.entries.insert(0, entry);
        self.entries.truncate(RECENT_WORKSPACES_MAX);
    }

    /// Writes the list to `path` atomically (folder created when missing).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let body = toml::to_string(&FileFormat {
            workspace: self.entries.clone(),
        })
        .map_err(std::io::Error::other)?;
        write_atomic(path, body.as_bytes())
    }
}

/// Writes `bytes` to `path` through a fsynced temporary file in the same folder renamed over
/// it: readers see the old or the new content, never a mix. For state files owned by
/// vr-ide; user files go through vr-core's freshness-checked saves.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut n = 0u32;
    let (tmp, mut file) = loop {
        let tmp = dir.join(format!(".{name}.{}-{n}.tmp", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
            Ok(f) => break (tmp, f),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && n < 100 => n += 1,
            Err(e) => return Err(e),
        }
    };
    let written = file
        .write_all(bytes)
        .and_then(|()| file.sync_all())
        .and_then(|()| std::fs::rename(&tmp, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(roots: &[&str]) -> RecentWorkspace {
        RecentWorkspace {
            file: None,
            roots: roots.iter().map(PathBuf::from).collect(),
        }
    }

    #[test]
    fn recent_list_is_bounded_newest_first_and_deduplicated() {
        let mut list = RecentWorkspaces::default();
        for i in 0..15 {
            list.push(ws(&[&format!("/p/{i}")]));
        }
        assert_eq!(list.entries.len(), RECENT_WORKSPACES_MAX);
        assert_eq!(list.entries[0], ws(&["/p/14"]));
        assert_eq!(list.entries[9], ws(&["/p/5"]));
        // The same root set (any order) moves to the front instead of a second copy.
        list.push(ws(&["/a", "/b"]));
        list.push(ws(&["/p/13"]));
        list.push(ws(&["/b", "/a"]));
        assert_eq!(list.entries[0], ws(&["/b", "/a"]));
        assert_eq!(list.entries[1], ws(&["/p/13"]));
        assert_eq!(
            list.entries
                .iter()
                .filter(|e| e.roots.contains(&PathBuf::from("/a")))
                .count(),
            1
        );
        // A workspace file is one entry whatever its roots are now.
        let file = |roots: &[&str]| RecentWorkspace {
            file: Some(PathBuf::from("/w/t.vr-workspace")),
            ..ws(roots)
        };
        list.push(file(&["/x"]));
        list.push(file(&["/x", "/y"]));
        assert_eq!(list.entries[0], file(&["/x", "/y"]));
        assert_ne!(list.entries[1].file, list.entries[0].file);
    }

    #[test]
    fn recent_list_round_trips_and_survives_a_corrupt_file() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("state/recent.toml");
        let (empty, w) = RecentWorkspaces::load(&path);
        assert!(empty.entries.is_empty() && w.is_none());

        let mut list = RecentWorkspaces::default();
        list.push(ws(&["/a b", "/c"]));
        list.push(RecentWorkspace {
            file: Some(PathBuf::from("/w/t.vr-workspace")),
            roots: vec![PathBuf::from("/w/x")],
        });
        list.save(&path).unwrap();
        let (back, w) = RecentWorkspaces::load(&path);
        assert!(w.is_none());
        assert_eq!(back, list);
        // No temporary file is left behind.
        assert_eq!(
            std::fs::read_dir(path.parent().unwrap()).unwrap().count(),
            1
        );

        std::fs::write(&path, "[[workspace]\nroots = 3").unwrap();
        let (fresh, w) = RecentWorkspaces::load(&path);
        assert!(fresh.entries.is_empty());
        let w = w.expect("a corrupt file is reported");
        assert!(w.to_string().contains("starts fresh"), "{w}");
        // The next save replaces the corrupt file with a valid list.
        let mut fresh = fresh;
        fresh.push(ws(&["/n"]));
        fresh.save(&path).unwrap();
        assert_eq!(RecentWorkspaces::load(&path).0, fresh);
    }
}
