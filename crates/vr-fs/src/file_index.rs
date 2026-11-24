//! Fuzzy go-to-file index. The walk runs on a background thread and hands the
//! finished index to the UI through a channel; dropping the receiver stops it.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::thread::JoinHandle;

use nucleo_matcher::{Config, Matcher, Utf32Str};

/// Walks stop collecting after this many files so a project with a huge
/// non-ignored tree cannot exhaust memory; `FileIndex::truncated` reports it.
pub const MAX_FILES: usize = 200_000;

/// Walk entries between two `Progress` messages. Each send doubles as the
/// cancellation check: it fails once the receiver is dropped.
const CANCEL_CHECK_EVERY: usize = 256;

/// Basename matches always rank above path-only matches.
const BASENAME_BONUS: u32 = 1 << 16;

#[derive(Debug)]
pub enum IndexMsg {
    /// Files collected so far.
    Progress(usize),
    Ready(FileIndex),
    Error(String),
}

#[derive(Debug, Clone, Default)]
pub struct FileIndex {
    root: PathBuf,
    /// Root-relative, sorted.
    files: Vec<PathBuf>,
    truncated: bool,
    /// Several roots: `(folder, name)` in order; files are then `name/relative`.
    roots: Vec<(PathBuf, String)>,
}

impl FileIndex {
    /// Walks `root` in the background (gitignore rules honoured, `.git`
    /// skipped, symlinks not followed) and sends `Progress` then `Ready`.
    pub fn build(root: impl Into<PathBuf>, tx: Sender<IndexMsg>) -> JoinHandle<()> {
        let root = root.into();
        std::thread::spawn(move || {
            walk_roots(vec![(root, String::new())], tx);
        })
    }

    /// Walks several roots `(folder, name)` in order, on one background thread. With more
    /// than one root each file is listed as `name/relative/path` (root order, then path
    /// order) and [`FileIndex::absolute`] maps it back; with one root the index is the one
    /// [`FileIndex::build`] makes. Each root keeps its own ignore rules. A root that cannot
    /// be read is skipped; `Error` only when none can.
    pub fn build_roots(roots: Vec<(PathBuf, String)>, tx: Sender<IndexMsg>) -> JoinHandle<()> {
        std::thread::spawn(move || {
            walk_roots(roots, tx);
        })
    }

    /// The absolute path of `shown`, a path this index lists.
    pub fn absolute(&self, shown: &Path) -> PathBuf {
        if self.roots.len() > 1 {
            for (root, name) in &self.roots {
                if let Ok(rest) = shown.strip_prefix(name) {
                    return root.join(rest);
                }
            }
        }
        self.root.join(shown)
    }

    /// Builds an index from root-relative paths (tests and callers that
    /// already hold a file list).
    pub fn from_paths(root: impl Into<PathBuf>, mut files: Vec<PathBuf>) -> Self {
        files.sort();
        files.dedup();
        FileIndex {
            root: root.into(),
            files,
            truncated: false,
            roots: Vec::new(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn files(&self) -> &[PathBuf] {
        &self.files
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// Best `limit` matches as (root-relative path, score, char indices into
    /// the path's display string). A match inside the file name ranks above
    /// any match that needs the directories. Smart case: an uppercase letter
    /// in the query makes it case-sensitive. An empty query lists files in
    /// path order with score 0.
    pub fn query(&self, q: &str, limit: usize) -> Vec<(PathBuf, u32, Vec<u32>)> {
        let needle: String = q.chars().filter(|c| !c.is_whitespace()).collect();
        if limit == 0 {
            return Vec::new();
        }
        if needle.is_empty() {
            return self
                .files
                .iter()
                .take(limit)
                .map(|p| (p.clone(), 0, Vec::new()))
                .collect();
        }

        let mut config = Config::DEFAULT;
        config.set_match_paths();
        config.ignore_case = !needle.chars().any(char::is_uppercase);
        let mut matcher = Matcher::new(config);
        let mut needle_buf = Vec::new();
        let needle_utf32 = Utf32Str::new(&needle, &mut needle_buf);
        let whole_path_only = needle.contains('/');

        let mut hay_buf = Vec::new();
        let mut indices = Vec::new();
        let mut hits: Vec<(usize, u32, Vec<u32>)> = Vec::new();
        for (i, path) in self.files.iter().enumerate() {
            let text = path.to_string_lossy();
            let base_start = text.rfind('/').map_or(0, |b| b + 1);
            if !whole_path_only {
                let base = &text[base_start..];
                indices.clear();
                if let Some(score) = matcher.fuzzy_indices(
                    Utf32Str::new(base, &mut hay_buf),
                    needle_utf32,
                    &mut indices,
                ) {
                    let offset = text[..base_start].chars().count() as u32;
                    let idx = indices.iter().map(|&x| x + offset).collect();
                    hits.push((i, BASENAME_BONUS + u32::from(score), idx));
                    continue;
                }
            }
            indices.clear();
            if let Some(score) = matcher.fuzzy_indices(
                Utf32Str::new(&text, &mut hay_buf),
                needle_utf32,
                &mut indices,
            ) {
                hits.push((i, u32::from(score), indices.clone()));
            }
        }

        // Higher score first; among equals the shorter path, then path order.
        hits.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| {
                    self.files[a.0]
                        .as_os_str()
                        .len()
                        .cmp(&self.files[b.0].as_os_str().len())
                })
                .then_with(|| a.0.cmp(&b.0))
        });
        hits.truncate(limit);
        hits.into_iter()
            .map(|(i, score, idx)| (self.files[i].clone(), score, idx))
            .collect()
    }
}

#[cfg(test)]
fn walk(root: PathBuf, tx: Sender<IndexMsg>) -> usize {
    walk_roots(vec![(root, String::new())], tx)
}

/// How one root's walk ended.
enum Walked {
    Done,
    /// [`MAX_FILES`] reached: the files so far are kept, no later root is walked.
    Full,
    /// The receiver is gone.
    Cancelled,
}

fn walk_roots(roots: Vec<(PathBuf, String)>, tx: Sender<IndexMsg>) -> usize {
    let mut visited = 0;
    let several = roots.len() > 1;
    let mut files = Vec::new();
    let mut errors = Vec::new();
    let mut readable = 0;
    let mut truncated = false;
    for (root, name) in &roots {
        if let Err(e) = std::fs::read_dir(root) {
            errors.push(format!("{}: {e}", root.display()));
            continue;
        }
        readable += 1;
        let prefix = several.then(|| Path::new(name));
        let start = files.len();
        let walked = walk_one(root, prefix, &tx, &mut files, &mut visited);
        files[start..].sort();
        match walked {
            Walked::Done => {}
            Walked::Full => {
                truncated = true;
                break;
            }
            Walked::Cancelled => return visited,
        }
    }
    if readable == 0 {
        let _ = tx.send(IndexMsg::Error(errors.join("; ")));
        return visited;
    }
    let first = roots.first().map(|r| r.0.clone()).unwrap_or_default();
    let mut index = if several {
        FileIndex {
            root: first,
            files,
            truncated: false,
            roots,
        }
    } else {
        FileIndex::from_paths(first, files)
    };
    index.truncated = truncated;
    let _ = tx.send(IndexMsg::Ready(index));
    visited
}

/// Adds the files under `root` to `files` (relative, behind `prefix` when given).
fn walk_one(
    root: &Path,
    prefix: Option<&Path>,
    tx: &Sender<IndexMsg>,
    files: &mut Vec<PathBuf>,
    visited: &mut usize,
) -> Walked {
    // Same ignore policy as the tree: gitignore rules (with or without a
    // repo), `.git/info/exclude`, global excludes; dotfiles stay findable.
    // `parents(false)`: a root's rules never come from the folders above it, nor from
    // another root.
    let walker = ignore::WalkBuilder::new(root)
        .hidden(false)
        .parents(false)
        .ignore(false)
        .require_git(false)
        .follow_links(false)
        .filter_entry(|e| e.file_name() != ".git")
        .build();

    for entry in walker {
        *visited += 1;
        // Checked on every kind of entry, so a tree of mostly folders cancels as fast.
        // A failed send means the receiver is gone: stop the walk.
        if visited.is_multiple_of(CANCEL_CHECK_EVERY)
            && tx.send(IndexMsg::Progress(files.len())).is_err()
        {
            return Walked::Cancelled;
        }
        // Unreadable entries (permissions, races) are skipped.
        let Ok(entry) = entry else { continue };
        let Some(ft) = entry.file_type() else {
            continue;
        };
        let is_file = ft.is_file() || (ft.is_symlink() && entry.path().is_file());
        if !is_file {
            continue;
        }
        let Ok(rel) = entry.path().strip_prefix(root) else {
            continue;
        };
        if files.len() == MAX_FILES {
            return Walked::Full;
        }
        files.push(match prefix {
            Some(p) => p.join(rel),
            None => rel.to_path_buf(),
        });
    }
    Walked::Done
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::mpsc;
    use std::time::Duration;

    fn touch(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"x").unwrap();
    }

    fn build_sync(root: &Path) -> FileIndex {
        let (tx, rx) = mpsc::channel();
        let handle = FileIndex::build(root, tx);
        let index = loop {
            match rx.recv_timeout(Duration::from_secs(20)).unwrap() {
                IndexMsg::Progress(_) => continue,
                IndexMsg::Ready(index) => break index,
                IndexMsg::Error(e) => panic!("index error: {e}"),
            }
        };
        handle.join().unwrap();
        index
    }

    fn top(index: &FileIndex, q: &str, n: usize) -> Vec<String> {
        index
            .query(q, n)
            .into_iter()
            .map(|(p, _, _)| p.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn file_index_respects_gitignore() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let _ = std::process::Command::new("git")
            .args(["init", "-q"])
            .arg(root)
            .status();
        fs::write(root.join(".gitignore"), "node_modules/\n*.log\n").unwrap();
        touch(&root.join("src/main.rs"));
        touch(&root.join("node_modules/pkg/index.js"));
        touch(&root.join("debug.log"));
        touch(&root.join(".github/ci.yml"));
        #[cfg(unix)]
        std::os::unix::fs::symlink(root, root.join("src/loop")).unwrap();

        let index = build_sync(root);
        let files: Vec<String> = index
            .files()
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        assert_eq!(files, [".github/ci.yml", ".gitignore", "src/main.rs"]);
        assert!(!index.truncated());
        assert_eq!(index.root(), root);
    }

    #[test]
    fn file_index_query_ranks_basename_first() {
        let index = FileIndex::from_paths(
            "/p",
            [
                "docs/order/control/list.md",
                "src/ordinal/catalog.rs",
                "src/Controller/OrderController.php",
                "src/Controller/OrderService.php",
                "tests/Controller/OrderControllerTest.php",
            ]
            .iter()
            .map(PathBuf::from)
            .collect(),
        );
        let hits = index.query("ordctl", 10);
        assert_eq!(
            hits[0].0,
            PathBuf::from("src/Controller/OrderController.php")
        );
        assert_eq!(
            hits[1].0,
            PathBuf::from("tests/Controller/OrderControllerTest.php")
        );
        // Indices point into the full path: "OrderController.php" starts at char 15.
        let (path, _, idx) = &hits[0];
        let chars: Vec<char> = path.to_string_lossy().chars().collect();
        let picked: String = idx.iter().map(|&i| chars[i as usize]).collect();
        assert_eq!(picked.to_lowercase(), "ordctl");
        assert!(idx[0] >= 15);
        // Path-only matches still appear, below basename matches.
        assert!(
            hits.iter()
                .any(|h| h.0 == Path::new("docs/order/control/list.md"))
        );
        assert!(
            hits.iter()
                .all(|h| h.0 != Path::new("src/Controller/OrderService.php"))
        );
        assert_eq!(index.query("ordctl", 1).len(), 1);
    }

    #[test]
    fn file_index_query_path_and_case() {
        let index = FileIndex::from_paths(
            "/p",
            ["src/lib.rs", "crates/vr-fs/src/lib.rs", "Lib.md"]
                .iter()
                .map(PathBuf::from)
                .collect(),
        );
        // A slash means the query targets the whole path.
        assert_eq!(top(&index, "vrfs/lib", 5), ["crates/vr-fs/src/lib.rs"]);
        // Uppercase makes the query case-sensitive.
        assert_eq!(top(&index, "Lib", 5), ["Lib.md"]);
        assert_eq!(top(&index, "", 2), ["Lib.md", "crates/vr-fs/src/lib.rs"]);
        assert!(index.query("zzz", 5).is_empty());
    }

    fn build_roots_sync(roots: &[(&Path, &str)]) -> Result<FileIndex, String> {
        let (tx, rx) = mpsc::channel();
        let roots = roots
            .iter()
            .map(|(p, n)| (p.to_path_buf(), n.to_string()))
            .collect();
        let handle = FileIndex::build_roots(roots, tx);
        let out = loop {
            match rx.recv_timeout(Duration::from_secs(20)).unwrap() {
                IndexMsg::Progress(_) => continue,
                IndexMsg::Ready(index) => break Ok(index),
                IndexMsg::Error(e) => break Err(e),
            }
        };
        handle.join().unwrap();
        out
    }

    fn names(index: &FileIndex) -> Vec<String> {
        index
            .files()
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn file_index_over_two_roots_names_each_root_and_keeps_its_gitignore() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = (tmp.path().join("api"), tmp.path().join("web"));
        fs::create_dir_all(&a).unwrap();
        fs::write(a.join(".gitignore"), "*.log\n").unwrap();
        touch(&a.join("src/main.rs"));
        touch(&a.join("debug.log"));
        touch(&b.join("keep.log"));
        touch(&b.join("src/main.rs"));

        let index = build_roots_sync(&[(&a, "api"), (&b, "web")]).unwrap();
        // Root order first, then path order; a's .gitignore does not hide b's log.
        assert_eq!(
            names(&index),
            [
                "api/.gitignore",
                "api/src/main.rs",
                "web/keep.log",
                "web/src/main.rs"
            ]
        );
        assert_eq!(
            index.absolute(Path::new("web/src/main.rs")),
            b.join("src/main.rs")
        );
        assert_eq!(
            index.absolute(Path::new("api/src/main.rs")),
            a.join("src/main.rs")
        );
        assert_eq!(
            top(&index, "main", 5),
            ["api/src/main.rs", "web/src/main.rs"]
        );
        assert_eq!(top(&index, "web/main", 5), ["web/src/main.rs"]);

        // One root: the paths and the mapping are those of `build`.
        let one = build_roots_sync(&[(&a, "api")]).unwrap();
        assert_eq!(names(&one), [".gitignore", "src/main.rs"]);
        assert_eq!(one.root(), a);
        assert_eq!(
            one.absolute(Path::new("src/main.rs")),
            a.join("src/main.rs")
        );

        // A missing root is skipped while the others are indexed; all missing is an error.
        let gone = tmp.path().join("gone");
        let some = build_roots_sync(&[(&gone, "gone"), (&b, "web")]).unwrap();
        assert_eq!(names(&some), ["web/keep.log", "web/src/main.rs"]);
        let err = build_roots_sync(&[(&gone, "gone")]).unwrap_err();
        assert!(err.contains("gone"), "{err}");
    }

    #[test]
    fn file_index_stops_when_receiver_dropped() {
        let tmp = tempfile::tempdir().unwrap();
        for i in 0..2_000 {
            fs::write(tmp.path().join(format!("f{i}.txt")), b"x").unwrap();
        }
        let (tx, rx) = mpsc::channel();
        drop(rx);
        let visited = walk(tmp.path().to_path_buf(), tx);
        assert!(
            visited <= CANCEL_CHECK_EVERY,
            "walk went on for {visited} entries after the receiver was dropped"
        );
        // The public entry point also ends cleanly with nobody listening.
        let (tx, rx) = mpsc::channel();
        drop(rx);
        FileIndex::build(tmp.path(), tx).join().unwrap();
    }
}
