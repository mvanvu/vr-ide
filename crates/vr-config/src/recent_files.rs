//! Recently opened files (`state_dir()/recent-files.toml`): the last [`RECENT_FILES_MAX`]
//! files opened in any vr-ide, newest first (`/recent`).
//!
//! State, not configuration: a file that cannot be parsed starts a fresh list (reported once)
//! and is replaced on the next write, which goes through [`crate::write_atomic`].

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ConfigWarning;

/// File name of the list inside the state directory.
pub const RECENT_FILES_FILE: &str = "recent-files.toml";

/// Files kept in the list.
pub const RECENT_FILES_MAX: usize = 20;

#[derive(Debug, Default, Serialize, Deserialize)]
struct FileFormat {
    #[serde(default)]
    files: Vec<PathBuf>,
}

/// The recent-files list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecentFiles {
    pub entries: Vec<PathBuf>,
}

impl RecentFiles {
    /// Reads `path`. A missing file is an empty list; an unreadable or corrupt one is an
    /// empty list plus a warning.
    pub fn load(path: &Path) -> (RecentFiles, Option<ConfigWarning>) {
        let mut warnings = Vec::new();
        let Some(text) = crate::read_user_file(path, &mut warnings) else {
            return (RecentFiles::default(), warnings.pop());
        };
        match toml::from_str::<FileFormat>(&text) {
            Ok(f) => {
                let mut list = RecentFiles::default();
                // Oldest first, so the newest ends up in front and duplicates collapse.
                for p in f.files.into_iter().rev() {
                    list.push(p);
                }
                (list, None)
            }
            Err(e) => (
                RecentFiles::default(),
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

    /// Puts `file` in front (removing an older copy) and keeps the newest
    /// [`RECENT_FILES_MAX`].
    pub fn push(&mut self, file: PathBuf) {
        self.entries.retain(|e| *e != file);
        self.entries.insert(0, file);
        self.entries.truncate(RECENT_FILES_MAX);
    }

    /// `from` was renamed to `to`: the old path leaves the list, the new one is the newest.
    pub fn renamed(&mut self, from: &Path, to: PathBuf) {
        self.entries.retain(|e| e != from);
        self.push(to);
    }

    /// Merges `file` as the newest entry into the list kept in `path` (see [`Self::update`]).
    pub fn record(path: &Path, file: PathBuf) -> Result<(), String> {
        RecentFiles::update(path, |list| list.push(file))
    }

    /// Applies `f` to the list kept in `path` and writes it back: load, change and save run
    /// under an advisory lock on a sidecar file, so two vr-ide windows recording at the same
    /// moment both keep their entries. A list that cannot be read (other than missing) is
    /// left as it is and the error is returned; a corrupt one starts fresh (state, not
    /// configuration).
    pub fn update(path: &Path, f: impl FnOnce(&mut RecentFiles)) -> Result<(), String> {
        let shown = |e: std::io::Error| format!("cannot write {}: {e}", path.display());
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(shown)?;
        }
        let mut lock_path = path.as_os_str().to_owned();
        lock_path.push(".lock");
        // The list itself is replaced by a rename on each save, so the lock lives on a file
        // that is never replaced. Released when `lock` is dropped.
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .map_err(shown)?;
        lock.lock().map_err(shown)?;
        let (mut list, warning) = RecentFiles::load(path);
        if let Some(w @ ConfigWarning::Io { .. }) = warning {
            return Err(format!("{w}; the list is left as it is"));
        }
        f(&mut list);
        list.save(path).map_err(shown)
    }

    /// Writes the list to `path` atomically (folder created when missing).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let body = toml::to_string(&FileFormat {
            files: self.entries.clone(),
        })
        .map_err(std::io::Error::other)?;
        crate::write_atomic(path, body.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_files_are_bounded_newest_first_and_deduplicated() {
        let mut list = RecentFiles::default();
        for i in 0..25 {
            list.push(PathBuf::from(format!("/p/{i}.rs")));
        }
        assert_eq!(list.entries.len(), RECENT_FILES_MAX);
        assert_eq!(list.entries[0], PathBuf::from("/p/24.rs"));
        assert_eq!(list.entries[19], PathBuf::from("/p/5.rs"));
        list.push(PathBuf::from("/p/10.rs"));
        assert_eq!(list.entries[0], PathBuf::from("/p/10.rs"));
        assert_eq!(list.entries.len(), RECENT_FILES_MAX);
        assert_eq!(
            list.entries
                .iter()
                .filter(|p| **p == Path::new("/p/10.rs"))
                .count(),
            1
        );
    }

    #[test]
    fn two_racing_writers_keep_every_entry() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("state").join(RECENT_FILES_FILE);
        let per = RECENT_FILES_MAX / 2;
        let writers: Vec<_> = ["a", "b"]
            .into_iter()
            .map(|w| {
                let path = path.clone();
                std::thread::spawn(move || {
                    for i in 0..per {
                        RecentFiles::record(&path, PathBuf::from(format!("/{w}/{i}.rs"))).unwrap();
                    }
                })
            })
            .collect();
        for w in writers {
            w.join().unwrap();
        }
        let (list, w) = RecentFiles::load(&path);
        assert!(w.is_none());
        for w in ["a", "b"] {
            for i in 0..per {
                let p = PathBuf::from(format!("/{w}/{i}.rs"));
                assert!(
                    list.entries.contains(&p),
                    "{} lost: {:?}",
                    p.display(),
                    list.entries
                );
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_list_is_not_replaced() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join(RECENT_FILES_FILE);
        let mut list = RecentFiles::default();
        list.push(PathBuf::from("/kept.rs"));
        list.save(&path).unwrap();
        let before = std::fs::read(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
        if std::fs::read(&path).is_ok() {
            return; // running as root: permissions do not stop the read
        }
        let err = RecentFiles::record(&path, PathBuf::from("/new.rs")).unwrap_err();
        assert!(err.contains("cannot read"), "{err}");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn recent_files_round_trip_and_survive_a_corrupt_file() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("state").join(RECENT_FILES_FILE);
        let (empty, w) = RecentFiles::load(&path);
        assert!(empty.entries.is_empty() && w.is_none());
        let mut list = RecentFiles::default();
        list.push(PathBuf::from("/a b/x.rs"));
        list.push(PathBuf::from("/c/y.md"));
        list.save(&path).unwrap();
        let (back, w) = RecentFiles::load(&path);
        assert!(w.is_none());
        assert_eq!(back, list);
        std::fs::write(&path, "files = 3").unwrap();
        let (fresh, w) = RecentFiles::load(&path);
        assert!(fresh.entries.is_empty());
        assert!(w.unwrap().to_string().contains("starts fresh"));
    }
}
