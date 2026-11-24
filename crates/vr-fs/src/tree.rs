//! Lazy project tree: one directory is read per `children` call, so huge
//! directories (node_modules, vendor) cost nothing until they are expanded.

use std::cmp::Ordering;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ignore::Match;
use ignore::gitignore::Gitignore;

/// What a tree entry points to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    File,
    Dir,
    Symlink(SymlinkTarget),
}

/// Resolved target of a symlink. Only `Dir` may be expanded; `Loop` points to
/// one of its own ancestors (or to itself) and must never be followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymlinkTarget {
    File,
    Dir,
    Loop,
    Broken,
}

impl NodeKind {
    /// True when the UI may expand this node.
    pub fn is_expandable(self) -> bool {
        matches!(self, NodeKind::Dir | NodeKind::Symlink(SymlinkTarget::Dir))
    }

    fn sorts_as_dir(self) -> bool {
        matches!(
            self,
            NodeKind::Dir | NodeKind::Symlink(SymlinkTarget::Dir | SymlinkTarget::Loop)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub path: PathBuf,
    pub kind: NodeKind,
    /// Matched by a gitignore rule (only returned when `show_ignored` is on).
    pub ignored: bool,
    /// 0 for direct children of the root.
    pub depth: usize,
}

impl Node {
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub struct Tree {
    pub root: PathBuf,
    show_hidden: bool,
    show_ignored: bool,
}

impl Tree {
    pub fn new(root: impl Into<PathBuf>, show_hidden: bool, show_ignored: bool) -> Self {
        Tree {
            root: root.into(),
            show_hidden,
            show_ignored,
        }
    }

    pub fn show_hidden(&self) -> bool {
        self.show_hidden
    }

    pub fn show_ignored(&self) -> bool {
        self.show_ignored
    }

    pub fn set_show_hidden(&mut self, on: bool) {
        self.show_hidden = on;
    }

    pub fn set_show_ignored(&mut self, on: bool) {
        self.show_ignored = on;
    }

    /// Direct children of `dir` (the root or a path below it): directories
    /// first, then case-insensitive by name. `.git` is never listed.
    pub fn children(&self, dir: &Path) -> io::Result<Vec<Node>> {
        // `..` would pass the prefix check lexically yet list a directory outside the root.
        if dir
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} contains `..`", dir.display()),
            ));
        }
        let rel = dir.strip_prefix(&self.root).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} is outside the project root", dir.display()),
            )
        })?;
        let depth = rel.components().count();
        let rules = IgnoreRules::for_dir(&self.root, dir);

        let mut nodes = Vec::new();
        for entry in read_dir_counted(dir)? {
            let entry = match entry {
                Ok(e) => e,
                // An entry that vanished or cannot be stat'ed is skipped, not fatal.
                Err(_) => continue,
            };
            let name = entry.file_name();
            if name == ".git" {
                continue;
            }
            if !self.show_hidden && name.to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let kind = if file_type.is_symlink() {
                NodeKind::Symlink(resolve_symlink(&path, dir))
            } else if file_type.is_dir() {
                NodeKind::Dir
            } else {
                NodeKind::File
            };
            let ignored = rules.is_ignored(&path, kind.sorts_as_dir());
            if ignored && !self.show_ignored {
                continue;
            }
            nodes.push(Node {
                path,
                kind,
                ignored,
                depth,
            });
        }
        nodes.sort_by(compare_nodes);
        Ok(nodes)
    }
}

fn compare_nodes(a: &Node, b: &Node) -> Ordering {
    b.kind
        .sorts_as_dir()
        .cmp(&a.kind.sorts_as_dir())
        .then_with(|| {
            let (an, bn) = (a.name(), b.name());
            an.to_lowercase()
                .cmp(&bn.to_lowercase())
                .then_with(|| an.cmp(&bn))
        })
}

/// A symlink is a loop when its target is the directory holding it or one of
/// that directory's ancestors; expanding it would recurse forever.
fn resolve_symlink(link: &Path, parent: &Path) -> SymlinkTarget {
    match fs::canonicalize(link) {
        Ok(target) => {
            if !target.is_dir() {
                return SymlinkTarget::File;
            }
            match fs::canonicalize(parent) {
                Ok(parent) if parent.starts_with(&target) => SymlinkTarget::Loop,
                _ => SymlinkTarget::Dir,
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => SymlinkTarget::Broken,
        // ELOOP (a link chain pointing back at itself) and anything else unresolvable.
        Err(_) => SymlinkTarget::Loop,
    }
}

/// Gitignore rules that apply inside one directory: every `.gitignore` from the
/// project root down to that directory, `.git/info/exclude` and the user's
/// global excludes file. Rules are honoured with or without a git repository,
/// matching the file index and project search.
pub(crate) struct IgnoreRules {
    /// Deepest directory first, so the closest `.gitignore` decides.
    levels: Vec<Gitignore>,
    exclude: Gitignore,
    global: Gitignore,
}

impl IgnoreRules {
    pub(crate) fn for_dir(root: &Path, dir: &Path) -> Self {
        let mut levels = Vec::new();
        let mut current = Some(dir);
        while let Some(d) = current {
            let (gi, _) = Gitignore::new(d.join(".gitignore"));
            levels.push(gi);
            if d == root {
                break;
            }
            current = d.parent().filter(|p| p.starts_with(root));
        }
        let exclude = {
            let mut b = ignore::gitignore::GitignoreBuilder::new(root);
            b.add(root.join(".git").join("info").join("exclude"));
            b.build().unwrap_or_else(|_| Gitignore::empty())
        };
        // Rooted at the project, not the cwd, so global rules apply to project paths.
        let (global, _) = ignore::gitignore::GitignoreBuilder::new(root).build_global();
        IgnoreRules {
            levels,
            exclude,
            global,
        }
    }

    pub(crate) fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
        let sources = self
            .levels
            .iter()
            .chain(std::iter::once(&self.exclude))
            .chain(std::iter::once(&self.global));
        for gi in sources {
            if gi.is_empty() || !path.starts_with(gi.path()) {
                continue;
            }
            match gi.matched_path_or_any_parents(path, is_dir) {
                Match::Ignore(_) => return true,
                Match::Whitelist(_) => return false,
                Match::None => {}
            }
        }
        false
    }
}

#[cfg(test)]
thread_local! {
    /// Directory listings done by `children` on this thread (laziness check).
    static READ_DIRS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The single place `children` lists a directory.
fn read_dir_counted(dir: &Path) -> io::Result<fs::ReadDir> {
    #[cfg(test)]
    READ_DIRS.with(|c| c.set(c.get() + 1));
    fs::read_dir(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git_init(dir: &Path) {
        // The rules apply without git too; the repo mirrors real projects.
        let _ = Command::new("git").arg("init").arg("-q").arg(dir).status();
    }

    fn touch(p: &Path) {
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(p, b"x").unwrap();
    }

    fn names(nodes: &[Node]) -> Vec<String> {
        nodes.iter().map(Node::name).collect()
    }

    #[test]
    fn tree_dirs_first_sorted() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        touch(&root.join("b.txt"));
        touch(&root.join("A.txt"));
        touch(&root.join("c.rs"));
        fs::create_dir(root.join("zeta")).unwrap();
        fs::create_dir(root.join("Alpha")).unwrap();
        fs::create_dir(root.join("beta")).unwrap();
        let tree = Tree::new(root, false, false);
        let nodes = tree.children(root).unwrap();
        assert_eq!(
            names(&nodes),
            ["Alpha", "beta", "zeta", "A.txt", "b.txt", "c.rs"]
        );
        assert!(nodes.iter().all(|n| n.depth == 0));
        assert_eq!(nodes[0].kind, NodeKind::Dir);
        assert_eq!(nodes[3].kind, NodeKind::File);
    }

    #[test]
    fn tree_hides_gitignored_by_default() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        git_init(root);
        fs::write(root.join(".gitignore"), "target/\n*.log\n!keep.log\n").unwrap();
        touch(&root.join("target/debug/app"));
        touch(&root.join("src/main.rs"));
        touch(&root.join("src/trace.log"));
        touch(&root.join("keep.log"));
        touch(&root.join("err.log"));
        fs::write(root.join("src/.gitignore"), "gen.rs\n").unwrap();
        touch(&root.join("src/gen.rs"));
        fs::write(root.join(".git/info/exclude"), "local.txt\n").unwrap();
        touch(&root.join("local.txt"));

        let hidden = Tree::new(root, false, false);
        assert_eq!(names(&hidden.children(root).unwrap()), ["src", "keep.log"]);
        assert_eq!(
            names(&hidden.children(&root.join("src")).unwrap()),
            ["main.rs"]
        );

        let shown = Tree::new(root, false, true);
        let top = shown.children(root).unwrap();
        assert_eq!(
            names(&top),
            ["src", "target", "err.log", "keep.log", "local.txt"]
        );
        let flag = |n: &str| top.iter().find(|x| x.name() == n).unwrap().ignored;
        assert!(flag("target") && flag("err.log") && flag("local.txt"));
        assert!(!flag("src") && !flag("keep.log"));
        // Everything below an ignored directory is ignored too.
        let inside = shown.children(&root.join("target")).unwrap();
        assert_eq!(names(&inside), ["debug"]);
        assert!(inside[0].ignored);
        assert_eq!(inside[0].depth, 1);
        let src = shown.children(&root.join("src")).unwrap();
        assert!(src.iter().find(|n| n.name() == "gen.rs").unwrap().ignored);
    }

    #[test]
    fn tree_toggle_ignored_shows_them() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::write(root.join(".gitignore"), "out/\n").unwrap();
        fs::create_dir(root.join("out")).unwrap();
        let mut tree = Tree::new(root, false, false);
        assert!(tree.children(root).unwrap().is_empty());
        tree.set_show_ignored(true);
        assert_eq!(names(&tree.children(root).unwrap()), ["out"]);
    }

    #[test]
    fn tree_hides_dotfiles_unless_show_hidden() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        git_init(root);
        touch(&root.join(".env"));
        touch(&root.join(".github/ci.yml"));
        touch(&root.join("main.rs"));
        let tree = Tree::new(root, false, false);
        assert_eq!(names(&tree.children(root).unwrap()), ["main.rs"]);
        let tree = Tree::new(root, true, true);
        assert_eq!(
            names(&tree.children(root).unwrap()),
            [".github", ".env", "main.rs"]
        );
    }

    #[test]
    fn tree_lazy_children() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        touch(&root.join("node_modules/pkg/index.js"));
        touch(&root.join("src/deep/x.rs"));
        touch(&root.join("a.txt"));
        let tree = Tree::new(root, false, false);
        READ_DIRS.with(|c| c.set(0));
        let top = tree.children(root).unwrap();
        assert_eq!(
            READ_DIRS.with(|c| c.get()),
            1,
            "only the requested dir is listed"
        );
        assert_eq!(names(&top), ["node_modules", "src", "a.txt"]);
        assert!(top.iter().all(|n| n.depth == 0));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // An unreadable subdirectory: listing the root still works, and the
            // subdirectory is opened only when it is expanded.
            let locked = root.join("node_modules");
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
            if fs::read_dir(&locked).is_ok() {
                // Root or CAP_DAC_READ_SEARCH: permission bits prove nothing here.
                fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
                return;
            }
            let top = tree.children(root);
            let inner = tree.children(&locked);
            fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
            assert_eq!(names(&top.unwrap()), ["node_modules", "src", "a.txt"]);
            assert_eq!(inner.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        }
    }

    #[test]
    fn tree_children_refuses_parent_components() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("proj");
        fs::create_dir_all(root.join("sub")).unwrap();
        let tree = Tree::new(&root, false, false);
        let err = tree.children(&root.join("sub/../..")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn tree_children_outside_root_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let tree = Tree::new(tmp.path(), false, false);
        let err = tree.children(other.path()).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    }

    #[cfg(unix)]
    #[test]
    fn tree_symlink_loop_safe() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        touch(&root.join("lib/a.rs"));
        touch(&root.join("other/b.rs"));
        symlink(root, root.join("lib/up")).unwrap();
        symlink(root.join("lib"), root.join("lib/self")).unwrap();
        symlink(root.join("other"), root.join("lib/side")).unwrap();
        symlink(root.join("lib/a.rs"), root.join("lib/file_link")).unwrap();
        symlink(root.join("missing"), root.join("lib/broken")).unwrap();
        symlink(root.join("lib/cycle"), root.join("lib/cycle")).unwrap();

        let tree = Tree::new(root, false, false);
        let nodes = tree.children(&root.join("lib")).unwrap();
        let kind = |n: &str| nodes.iter().find(|x| x.name() == n).unwrap().kind;
        assert_eq!(kind("up"), NodeKind::Symlink(SymlinkTarget::Loop));
        assert_eq!(kind("self"), NodeKind::Symlink(SymlinkTarget::Loop));
        assert_eq!(kind("cycle"), NodeKind::Symlink(SymlinkTarget::Loop));
        assert_eq!(kind("side"), NodeKind::Symlink(SymlinkTarget::Dir));
        assert_eq!(kind("file_link"), NodeKind::Symlink(SymlinkTarget::File));
        assert_eq!(kind("broken"), NodeKind::Symlink(SymlinkTarget::Broken));
        assert!(!kind("up").is_expandable());
        assert!(kind("side").is_expandable());
        // Expanding the safe link lists the target's entries.
        let side = tree.children(&root.join("lib/side")).unwrap();
        assert_eq!(names(&side), ["b.rs"]);
    }
}
