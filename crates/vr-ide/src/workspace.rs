//! The workspace: the project folders (roots) one window holds (D226, spec W).
//!
//! Roots are kept in the order they were added; the first one is the primary root (the
//! explorer tree until it shows every root, the fallback of the active-root rule). Each
//! root has a display name, its folder name, disambiguated with the parent folder when two
//! roots share it. Roots are never nested: a folder inside (or around) a root is refused,
//! the same folder twice (also through a link) is a duplicate.

use std::fmt;
use std::path::{Path, PathBuf};

use vr_config::RecentWorkspace;
use vr_core::DiskVersion;

/// Stable identity of a root while it is in the workspace (names can change when roots are
/// added or removed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RootId(u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    pub id: RootId,
    /// Absolute; canonical when the folder existed when it was added.
    pub path: PathBuf,
    /// Display name (`api`, or `api (work)` when another root is also called `api`).
    pub name: String,
}

impl Root {
    /// The folder is gone (deleted or unmounted since it was added); it stays listed and
    /// removable.
    pub fn missing(&self) -> bool {
        !self.path.is_dir()
    }

    /// The checked-out git branch, read when asked (never at startup).
    pub fn branch(&self) -> Option<String> {
        crate::monitor::git_branch(&self.path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceError {
    NotFound(PathBuf),
    NotADirectory(PathBuf),
    Io(PathBuf, String),
    /// The folder is already a root (named `root`).
    Duplicate {
        path: PathBuf,
        root: String,
    },
    /// The folder is inside the root `root`.
    Nested {
        path: PathBuf,
        root: String,
    },
    /// The folder holds the root `root`.
    Contains {
        path: PathBuf,
        root: String,
    },
    /// A workspace keeps at least one root.
    LastRoot,
    UnknownRoot,
    /// A workspace file could not be read or written.
    File(String),
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(p) => write!(f, "{} does not exist", p.display()),
            Self::NotADirectory(p) => write!(f, "{} is not a folder", p.display()),
            Self::Io(p, e) => write!(f, "cannot access {}: {e}", p.display()),
            Self::Duplicate { path, root } => {
                write!(f, "{} is already in the workspace ({root})", path.display())
            }
            Self::Nested { path, root } => write!(
                f,
                "{} is inside the workspace folder {root}; nested folders are not added",
                path.display()
            ),
            Self::Contains { path, root } => write!(
                f,
                "{} contains the workspace folder {root}; nested folders are not added",
                path.display()
            ),
            Self::LastRoot => write!(f, "the last folder of the workspace cannot be removed"),
            Self::UnknownRoot => write!(f, "that folder is no longer in the workspace"),
            Self::File(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for WorkspaceError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    roots: Vec<Root>,
    next_id: u32,
    /// The `.vr-workspace` file the roots came from or were last saved to.
    file: Option<PathBuf>,
    /// The version of `file` read or written then: a save onto it replaces only this one
    /// (D244), so another window's save or a hand edit is never lost.
    file_version: Option<DiskVersion>,
}

impl Workspace {
    /// One root, taken as given (today's single project root).
    pub fn single(path: PathBuf) -> Workspace {
        let mut ws = Workspace {
            roots: Vec::new(),
            next_id: 0,
            file: None,
            file_version: None,
        };
        ws.push(path);
        ws
    }

    /// Roots from a list of folders, in order. Folders that are nested in (or hold) an
    /// earlier one, or repeat it, are left out and returned as errors; a folder that does
    /// not exist is kept (shown as missing). `Err` when no folder is left.
    pub fn from_paths(
        paths: &[PathBuf],
    ) -> Result<(Workspace, Vec<WorkspaceError>), WorkspaceError> {
        let mut ws: Option<Workspace> = None;
        let mut refused = Vec::new();
        for p in paths {
            let p = real(&normalize(p));
            match &mut ws {
                None => ws = Some(Workspace::single(p)),
                Some(w) => {
                    if let Err(e) = w.check(&p) {
                        refused.push(e);
                    } else {
                        w.push(p);
                    }
                }
            }
        }
        let ws = ws.ok_or_else(|| WorkspaceError::File("no folder to open".to_string()))?;
        Ok((ws, refused))
    }

    /// Roots of the workspace file `file` (see [`Workspace::from_paths`]).
    pub fn from_file(file: &Path) -> Result<(Workspace, Vec<WorkspaceError>), WorkspaceError> {
        // Taken before the read: a change after it is a later version, and a save refuses it.
        let version = DiskVersion::of_path(file).ok();
        let paths =
            vr_config::read_workspace(file).map_err(|e| WorkspaceError::File(e.to_string()))?;
        let (mut ws, refused) = Workspace::from_paths(&paths)?;
        ws.file = Some(real(file));
        ws.file_version = version;
        Ok((ws, refused))
    }

    pub fn roots(&self) -> &[Root] {
        &self.roots
    }

    /// The first root.
    pub fn primary(&self) -> &Root {
        &self.roots[0]
    }

    pub fn len(&self) -> usize {
        self.roots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    /// More than one root: paths are shown as `root-name/relative/path`.
    pub fn is_multi(&self) -> bool {
        self.roots.len() > 1
    }

    pub fn file(&self) -> Option<&Path> {
        self.file.as_deref()
    }

    /// The version of [`Workspace::file`] last read or written (see `file_version`).
    pub fn file_version(&self) -> Option<&DiskVersion> {
        self.file_version.as_ref()
    }

    /// Sets the file with an unknown version: a save onto it then refuses until it is read or
    /// written again.
    pub fn set_file(&mut self, file: Option<PathBuf>) {
        self.file = file;
        self.file_version = None;
    }

    /// Sets the file and the version of it just read or written.
    pub fn set_file_with_version(&mut self, file: PathBuf, version: Option<DiskVersion>) {
        self.file = Some(file);
        self.file_version = version;
    }

    pub fn get(&self, id: RootId) -> Option<&Root> {
        self.roots.iter().find(|r| r.id == id)
    }

    /// The root folders, in order.
    pub fn paths(&self) -> Vec<PathBuf> {
        self.roots.iter().map(|r| r.path.clone()).collect()
    }

    /// Adds the existing folder `path` (links resolved) as the last root.
    pub fn add(&mut self, path: &Path) -> Result<RootId, WorkspaceError> {
        let meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(WorkspaceError::NotFound(path.to_path_buf()));
            }
            Err(e) => return Err(WorkspaceError::Io(path.to_path_buf(), e.to_string())),
        };
        if !meta.is_dir() {
            return Err(WorkspaceError::NotADirectory(path.to_path_buf()));
        }
        let canon = path
            .canonicalize()
            .map_err(|e| WorkspaceError::Io(path.to_path_buf(), e.to_string()))?;
        self.check(&canon)?;
        Ok(self.push(canon))
    }

    /// Removes a root (nothing on disk is touched). The last root stays.
    pub fn remove(&mut self, id: RootId) -> Result<Root, WorkspaceError> {
        let i = self
            .roots
            .iter()
            .position(|r| r.id == id)
            .ok_or(WorkspaceError::UnknownRoot)?;
        if self.roots.len() == 1 {
            return Err(WorkspaceError::LastRoot);
        }
        let root = self.roots.remove(i);
        self.rename();
        Ok(root)
    }

    /// The root `p` is in (lexically: `p` as the app resolves paths).
    pub fn root_of(&self, p: &Path) -> Option<&Root> {
        self.roots.iter().find(|r| p.starts_with(&r.path))
    }

    /// The active-root rule: the root of the active file, else the first root.
    pub fn active_root(&self, active_file: Option<&Path>) -> &Root {
        active_file
            .and_then(|p| self.root_of(p))
            .unwrap_or_else(|| self.primary())
    }

    /// `p` for display: relative to the root (one root, as before), `root-name/relative`
    /// with several roots, the full path outside every root.
    pub fn display_path(&self, p: &Path) -> String {
        if !self.is_multi() {
            return p
                .strip_prefix(&self.primary().path)
                .unwrap_or(p)
                .display()
                .to_string();
        }
        match self.root_of(p) {
            Some(r) => {
                let rel = p.strip_prefix(&r.path).unwrap_or(p);
                if rel.as_os_str().is_empty() {
                    r.name.clone()
                } else {
                    format!("{}/{}", r.name, rel.display())
                }
            }
            None => p.display().to_string(),
        }
    }

    /// What the window is called: the root's path (one root), the workspace file's name, or
    /// the root names.
    pub fn label(&self) -> String {
        if let Some(stem) = self.file.as_deref().and_then(Path::file_stem) {
            return stem.to_string_lossy().into_owned();
        }
        if !self.is_multi() {
            return self.primary().path.display().to_string();
        }
        let names: Vec<&str> = self.roots.iter().map(|r| r.name.as_str()).collect();
        names.join(", ")
    }

    /// This workspace as an entry of the recent list.
    pub fn recent_entry(&self) -> RecentWorkspace {
        RecentWorkspace {
            file: self.file.clone(),
            roots: self.paths(),
        }
    }

    /// The text of a `.vr-workspace` file at `file` listing the roots.
    pub fn to_toml(&self, file: &Path) -> String {
        vr_config::format_workspace(file, &self.paths())
    }

    /// Refuses `path` (canonical) when it repeats, sits in or holds a root.
    fn check(&self, path: &Path) -> Result<(), WorkspaceError> {
        for r in &self.roots {
            let root = real(&r.path);
            let (path_buf, name) = (path.to_path_buf(), r.name.clone());
            if path == root {
                return Err(WorkspaceError::Duplicate {
                    path: path_buf,
                    root: name,
                });
            }
            if path.starts_with(&root) {
                return Err(WorkspaceError::Nested {
                    path: path_buf,
                    root: name,
                });
            }
            if root.starts_with(path) {
                return Err(WorkspaceError::Contains {
                    path: path_buf,
                    root: name,
                });
            }
        }
        Ok(())
    }

    fn push(&mut self, path: PathBuf) -> RootId {
        let id = RootId(self.next_id);
        self.next_id += 1;
        self.roots.push(Root {
            id,
            path,
            name: String::new(),
        });
        self.rename();
        id
    }

    /// Display names: the folder name; `name (parent)` when roots share it; a number after
    /// that when the parents are the same too. Names never contain '/', so a shown path
    /// `name/relative` splits back unambiguously.
    fn rename(&mut self) {
        let base = |p: &Path| {
            p.file_name()
                .map_or_else(
                    || p.display().to_string(),
                    |n| n.to_string_lossy().into_owned(),
                )
                .replace('/', "_")
        };
        let bases: Vec<String> = self.roots.iter().map(|r| base(&r.path)).collect();
        let mut names: Vec<String> = Vec::with_capacity(bases.len());
        for (i, r) in self.roots.iter().enumerate() {
            let shared = bases.iter().filter(|b| **b == bases[i]).count() > 1;
            let mut name = if shared {
                let parent = r.path.parent().map(base).unwrap_or_default();
                format!("{} ({parent})", bases[i])
            } else {
                bases[i].clone()
            };
            let mut n = 2;
            let first = name.clone();
            while names.contains(&name) {
                name = format!("{first} {n}");
                n += 1;
            }
            names.push(name);
        }
        for (r, n) in self.roots.iter_mut().zip(names) {
            r.name = n;
        }
    }
}

/// `p` with `.` removed and `..` applied lexically.
pub fn normalize(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push(c);
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// `p` with links resolved when it exists, else as given.
fn real(p: &Path) -> PathBuf {
    p.canonicalize().unwrap_or_else(|_| p.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let base = d.path().canonicalize().unwrap();
        (d, base)
    }

    fn mkdir(p: &Path) -> PathBuf {
        std::fs::create_dir_all(p).unwrap();
        p.to_path_buf()
    }

    fn names(ws: &Workspace) -> Vec<&str> {
        ws.roots().iter().map(|r| r.name.as_str()).collect()
    }

    #[test]
    fn roots_are_named_and_disambiguated_with_the_parent() {
        let (_d, base) = tmp();
        let mut ws = Workspace::single(mkdir(&base.join("work/api")));
        assert_eq!(names(&ws), ["api"]);
        ws.add(&mkdir(&base.join("web"))).unwrap();
        let other = ws.add(&mkdir(&base.join("old/api"))).unwrap();
        assert_eq!(names(&ws), ["api (work)", "web", "api (old)"]);
        ws.add(&mkdir(&base.join("x/old/api"))).unwrap();
        assert_eq!(
            names(&ws),
            ["api (work)", "web", "api (old)", "api (old) 2"]
        );
        // Removing a root renames the others back.
        ws.remove(other).unwrap();
        let last = ws.roots()[2].id;
        ws.remove(last).unwrap();
        assert_eq!(names(&ws), ["api", "web"]);
    }

    #[test]
    fn nested_duplicate_and_linked_roots_are_refused() {
        let (_d, base) = tmp();
        let a = mkdir(&base.join("a"));
        let mut ws = Workspace::single(a.clone());
        assert!(matches!(ws.add(&a), Err(WorkspaceError::Duplicate { .. })));
        assert!(matches!(
            ws.add(&base.join("a/./")),
            Err(WorkspaceError::Duplicate { .. })
        ));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&a, base.join("link")).unwrap();
            assert!(matches!(
                ws.add(&base.join("link")),
                Err(WorkspaceError::Duplicate { .. })
            ));
        }
        let e = ws.add(&mkdir(&base.join("a/inner"))).unwrap_err();
        assert!(matches!(e, WorkspaceError::Nested { .. }), "{e:?}");
        assert!(e.to_string().contains("nested"), "{e}");
        assert!(matches!(
            ws.add(&base),
            Err(WorkspaceError::Contains { .. })
        ));
        std::fs::write(base.join("f.txt"), "x").unwrap();
        assert!(matches!(
            ws.add(&base.join("f.txt")),
            Err(WorkspaceError::NotADirectory(_))
        ));
        assert!(matches!(
            ws.add(&base.join("none")),
            Err(WorkspaceError::NotFound(_))
        ));
        // A sibling whose name starts like the root is not nested.
        ws.add(&mkdir(&base.join("ab"))).unwrap();
        assert_eq!(ws.len(), 2);
        // The last root stays.
        let (a_id, ab_id) = (ws.roots()[0].id, ws.roots()[1].id);
        ws.remove(a_id).unwrap();
        assert_eq!(ws.remove(ab_id), Err(WorkspaceError::LastRoot));
        assert_eq!(ws.remove(a_id), Err(WorkspaceError::UnknownRoot));
    }

    #[test]
    fn display_path_and_active_root() {
        let (_d, base) = tmp();
        let (a, b) = (mkdir(&base.join("a")), mkdir(&base.join("b")));
        let mut ws = Workspace::single(a.clone());
        // One root: relative, as before.
        assert_eq!(ws.display_path(&a.join("src/x.rs")), "src/x.rs");
        assert_eq!(ws.display_path(Path::new("/etc/hosts")), "/etc/hosts");
        assert_eq!(ws.label(), a.display().to_string());
        ws.add(&b).unwrap();
        assert_eq!(ws.display_path(&a.join("src/x.rs")), "a/src/x.rs");
        assert_eq!(ws.display_path(&b.join("y.rs")), "b/y.rs");
        assert_eq!(ws.display_path(&b), "b");
        assert_eq!(ws.display_path(Path::new("/etc/hosts")), "/etc/hosts");
        assert_eq!(ws.label(), "a, b");
        assert_eq!(ws.active_root(Some(&b.join("y.rs"))).path, b);
        assert_eq!(ws.active_root(Some(Path::new("/etc/hosts"))).path, a);
        assert_eq!(ws.active_root(None).path, a);
    }

    #[test]
    fn from_paths_keeps_missing_roots_and_reports_refused_ones() {
        let (_d, base) = tmp();
        let a = mkdir(&base.join("a"));
        let gone = base.join("gone");
        let (ws, refused) =
            Workspace::from_paths(&[a.clone(), a.join("sub"), gone.clone(), a.clone()]).unwrap();
        assert_eq!(ws.paths(), [a.clone(), gone.clone()]);
        assert!(ws.roots()[1].missing() && !ws.roots()[0].missing());
        assert_eq!(refused.len(), 2, "{refused:?}");
        assert!(Workspace::from_paths(&[]).is_err());

        // A workspace file round trip, relative inside its folder.
        let file = base.join("team.vr-workspace");
        std::fs::write(&file, ws.to_toml(&file)).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(
            text.contains("\"a\"") && text.contains("\"gone\""),
            "{text}"
        );
        let (back, refused) = Workspace::from_file(&file).unwrap();
        assert!(refused.is_empty());
        assert_eq!(back.paths(), ws.paths());
        assert_eq!(back.file(), Some(file.as_path()));
        assert_eq!(back.label(), "team");
        assert_eq!(
            back.recent_entry(),
            RecentWorkspace {
                file: Some(file.clone()),
                roots: vec![a, gone]
            }
        );
        assert!(Workspace::from_file(&base.join("none.vr-workspace")).is_err());
    }
}
