//! File operations behind the explorer.
//!
//! Overwrite guarantee: no operation here replaces or truncates an existing
//! path. Wherever the OS allows it the refusal is atomic (`O_EXCL` create,
//! `mkdir`, `link`, `renameat2(RENAME_NOREPLACE)`), so a file an AI agent
//! creates between the user's action and the syscall is never lost. Delete
//! goes to the system trash (D9), never `remove_*`.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, thiserror::Error)]
pub enum FsOpError {
    #[error("{0} already exists")]
    AlreadyExists(PathBuf),
    #[error("{0} does not exist")]
    NotFound(PathBuf),
    #[error("cannot move or copy {from} into itself ({to})")]
    IntoItself { from: PathBuf, to: PathBuf },
    #[error("{0} has no file name")]
    NoFileName(PathBuf),
    /// Moving between filesystems would need copy + delete; refused (D24) so
    /// nothing is half-moved. The caller may offer a copy instead.
    #[error("{from} and {to} are on different filesystems; copy instead")]
    CrossDevice { from: PathBuf, to: PathBuf },
    /// `to` was created by this copy but is incomplete; the source is untouched.
    #[error("copy to {to} stopped partway; {to} is incomplete: {source}")]
    PartialCopy { to: PathBuf, source: Box<FsOpError> },
    #[error("invalid name {0:?}")]
    InvalidName(String),
    #[error("{0} is outside the project")]
    OutsideRoot(PathBuf),
    #[error("{path} was not trashed: {reason}")]
    TrashRefused { path: PathBuf, reason: String },
    #[error("{path}: {source}")]
    Io { path: PathBuf, source: io::Error },
    #[error("trash failed: {0}")]
    Trash(String),
}

/// One result per input path of a batch operation, in input order.
pub type BatchResult = Vec<(PathBuf, Result<(), FsOpError>)>;

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> FsOpError + '_ {
    move |source| match source.kind() {
        io::ErrorKind::AlreadyExists => FsOpError::AlreadyExists(path.to_path_buf()),
        io::ErrorKind::NotFound => FsOpError::NotFound(path.to_path_buf()),
        io::ErrorKind::CrossesDevices => FsOpError::CrossDevice {
            from: path.to_path_buf(),
            to: path.to_path_buf(),
        },
        _ => FsOpError::Io {
            path: path.to_path_buf(),
            source,
        },
    }
}

/// Errors of a two-path syscall: blamed on the source only when the source is
/// gone, otherwise on the destination (missing parent, read-only target dir).
fn move_err(from: &Path, to: &Path, e: io::Error) -> FsOpError {
    if e.kind() == io::ErrorKind::CrossesDevices {
        return FsOpError::CrossDevice {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
        };
    }
    if exists(from) {
        io_err(to)(e)
    } else {
        io_err(from)(e)
    }
}

/// `symlink_metadata` so a broken symlink still counts as existing.
fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

fn ensure_present(path: &Path) -> Result<(), FsOpError> {
    if !exists(path) {
        return Err(FsOpError::NotFound(path.to_path_buf()));
    }
    Ok(())
}

/// Checks one path component typed by the user (new file, rename).
pub fn validate_name(name: &str) -> Result<(), FsOpError> {
    let bad = name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\0')
        || (cfg!(windows) && name.contains('\\'));
    if bad {
        return Err(FsOpError::InvalidName(name.to_string()));
    }
    Ok(())
}

/// Resolves `p` (relative paths are taken from `root`) with `.` and `..`
/// handled lexically, and refuses a result outside `root`. Symlinks are not
/// resolved: a link inside the project is the user's own choice of target.
pub fn ensure_within(root: &Path, p: &Path) -> Result<PathBuf, FsOpError> {
    let root_norm = normalize(&absolute(root)?);
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        root_norm.join(p)
    };
    let full = normalize(&joined);
    if !full.starts_with(&root_norm) {
        return Err(FsOpError::OutsideRoot(p.to_path_buf()));
    }
    Ok(full)
}

fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// Creates an empty file; missing parent directories are created (typing
/// `a/b/new.rs` in the explorer is one step).
/// Overwrite: never — `O_CREAT|O_EXCL` fails atomically if `path` exists,
/// including a dangling symlink.
pub fn create_file(path: &Path) -> Result<(), FsOpError> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(io_err(parent))?;
    }
    fs::File::create_new(path).map_err(io_err(path))?;
    Ok(())
}

/// Creates a directory (and missing parents).
/// Overwrite: never — the last `mkdir` fails atomically if `path` exists.
pub fn create_dir(path: &Path) -> Result<(), FsOpError> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(io_err(parent))?;
    }
    fs::create_dir(path).map_err(io_err(path))
}

/// Renames `from` to the full path `to`. A symlink is renamed as a link.
/// A rename that only changes letter case works on case-insensitive
/// filesystems too (via a temporary name). Renaming to the same path is a no-op.
/// Overwrite: never — see `move_with`; refused atomically except on
/// filesystems without `RENAME_NOREPLACE`, where only an empty directory
/// created in a microsecond window could be replaced.
pub fn rename(from: &Path, to: &Path) -> Result<(), FsOpError> {
    ensure_present(from)?;
    if from == to {
        return Ok(());
    }
    refuse_into_itself(from, to)?;
    if is_case_only_same_file(from, to) {
        return rename_via_temp(from, to, &rename_noreplace);
    }
    move_with(from, to, &rename_noreplace)
}

/// Copies a file, symlink or directory tree to the full path `to`.
/// Symlinks are copied as links, never followed, so loops cannot recurse.
/// Sockets, FIFOs and devices are refused (opening a FIFO would block).
/// Overwrite: never — every entry is created with `O_EXCL`, `mkdir` or
/// `symlink`, which fail atomically if the name exists; copying onto an
/// existing directory is refused before anything is written.
/// A failure after `to` was created returns `PartialCopy { to }`.
pub fn copy(from: &Path, to: &Path) -> Result<(), FsOpError> {
    ensure_present(from)?;
    refuse_into_itself(from, to)?;
    match copy_entry(from, to) {
        Ok(()) => Ok(()),
        Err((true, e)) => Err(FsOpError::PartialCopy {
            to: to.to_path_buf(),
            source: Box::new(e),
        }),
        Err((false, e)) => Err(e),
    }
}

/// Moves `from` into the directory `dest_dir`, keeping its name, and returns
/// the new path. A symlink is moved as a link.
/// Overwrite: never — same guarantee as `rename`. Across filesystems it
/// returns `CrossDevice` and leaves everything as it was (D24).
pub fn move_to(from: &Path, dest_dir: &Path) -> Result<PathBuf, FsOpError> {
    ensure_present(from)?;
    let name = from
        .file_name()
        .ok_or_else(|| FsOpError::NoFileName(from.to_path_buf()))?;
    if !dest_dir.is_dir() {
        return Err(FsOpError::NotFound(dest_dir.to_path_buf()));
    }
    let to = dest_dir.join(name);
    refuse_into_itself(from, &to)?;
    move_with(from, &to, &rename_noreplace)?;
    Ok(to)
}

/// Moves every path to the system trash (D9), one by one, and reports each.
/// Overwrite/delete: never deletes permanently; an unavailable trash is an
/// error and the path stays where it was.
pub fn trash(paths: &[PathBuf]) -> BatchResult {
    SystemTrash.trash(paths)
}

/// Trash backend, so callers (and their tests) can swap the system trash for a fake.
pub trait Trash {
    /// One result per path, in input order; a failed path does not stop the rest.
    fn trash(&self, paths: &[PathBuf]) -> BatchResult;
}

/// The desktop trash (freedesktop on Linux, Finder on macOS, Recycle Bin on Windows).
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemTrash;

impl Trash for SystemTrash {
    fn trash(&self, paths: &[PathBuf]) -> BatchResult {
        paths.iter().map(|p| (p.clone(), trash_one(p))).collect()
    }
}

fn trash_one(path: &Path) -> Result<(), FsOpError> {
    ensure_present(path)?;
    #[cfg(all(unix, not(target_os = "macos")))]
    if let Some(dir) = home_trash_dir() {
        check_symlink_trash(path, &dir)?;
    }
    trash::delete(path).map_err(|e| FsOpError::Trash(e.to_string()))
}

/// Where the freedesktop home trash lives (same rule as the `trash` crate).
#[cfg(all(unix, not(target_os = "macos")))]
fn home_trash_dir() -> Option<PathBuf> {
    let non_empty = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
    non_empty("XDG_DATA_HOME")
        .map(|d| PathBuf::from(d).join("Trash"))
        .or_else(|| non_empty("HOME").map(|h| PathBuf::from(h).join(".local/share/Trash")))
}

/// trash 5.4.0 handles a symlink that must cross devices to reach the trash
/// by copying through the link, which for a link to a directory can repeat
/// without end. Such a link is refused instead; nothing is changed.
#[cfg(unix)]
fn check_symlink_trash(path: &Path, trash_dir: &Path) -> Result<(), FsOpError> {
    use std::os::unix::fs::MetadataExt;
    let meta = fs::symlink_metadata(path).map_err(io_err(path))?;
    if !meta.file_type().is_symlink() {
        return Ok(());
    }
    let parent = match path.parent().filter(|p| !p.as_os_str().is_empty()) {
        Some(p) => p.to_path_buf(),
        None => PathBuf::from("."),
    };
    let link_dev = fs::metadata(&parent).map_err(io_err(&parent))?.dev();
    // The trash may not exist yet; it will be created on its nearest existing ancestor.
    let trash_dev = trash_dir
        .ancestors()
        .find_map(|a| fs::metadata(a).ok())
        .map(|m| m.dev());
    match trash_dev {
        Some(dev) if dev == link_dev => Ok(()),
        _ => Err(FsOpError::TrashRefused {
            path: path.to_path_buf(),
            reason: "symlink on another filesystem than the trash".into(),
        }),
    }
}

/// Test double: moves trashed paths into `holding_dir` and records them, so
/// tests never touch the user's real trash.
#[derive(Debug)]
pub struct FakeTrash {
    holding_dir: PathBuf,
    trashed: Mutex<Vec<PathBuf>>,
}

impl FakeTrash {
    pub fn new(holding_dir: impl Into<PathBuf>) -> Self {
        FakeTrash {
            holding_dir: holding_dir.into(),
            trashed: Mutex::new(Vec::new()),
        }
    }

    /// Original paths trashed so far, in order.
    pub fn trashed(&self) -> Vec<PathBuf> {
        self.trashed.lock().map(|v| v.clone()).unwrap_or_default()
    }

    fn trash_one(&self, path: &Path) -> Result<(), FsOpError> {
        ensure_present(path)?;
        fs::create_dir_all(&self.holding_dir).map_err(io_err(&self.holding_dir))?;
        let mut trashed = self
            .trashed
            .lock()
            .map_err(|_| FsOpError::Trash("fake trash lock poisoned".into()))?;
        let slot = self.holding_dir.join(trashed.len().to_string());
        move_with(path, &slot, &rename_noreplace)?;
        trashed.push(path.to_path_buf());
        Ok(())
    }
}

impl Trash for FakeTrash {
    fn trash(&self, paths: &[PathBuf]) -> BatchResult {
        paths
            .iter()
            .map(|p| (p.clone(), self.trash_one(p)))
            .collect()
    }
}

/// Compares absolute, lexically normalised forms; canonicalising `to` is not
/// possible because it does not exist yet.
fn refuse_into_itself(from: &Path, to: &Path) -> Result<(), FsOpError> {
    let from_abs = normalize(&absolute(from)?);
    let to_abs = normalize(&absolute(to)?);
    let from_real = fs::canonicalize(from).unwrap_or_else(|_| from_abs.clone());
    let to_real = to
        .parent()
        .and_then(|p| fs::canonicalize(p).ok())
        .and_then(|p| to.file_name().map(|n| p.join(n)))
        .unwrap_or_else(|| to_abs.clone());
    let is_dir = fs::symlink_metadata(from)
        .map(|m| m.is_dir())
        .unwrap_or(false);
    if is_dir && (to_abs.starts_with(&from_abs) || to_real.starts_with(&from_real)) {
        return Err(FsOpError::IntoItself {
            from: from.to_path_buf(),
            to: to.to_path_buf(),
        });
    }
    Ok(())
}

fn absolute(p: &Path) -> Result<PathBuf, FsOpError> {
    std::path::absolute(p).map_err(io_err(p))
}

/// Same directory, names equal ignoring case, and both names reach one file:
/// a case-insensitive filesystem showing the source under the new spelling.
fn is_case_only_same_file(from: &Path, to: &Path) -> bool {
    let (Some(fname), Some(tname)) = (from.file_name(), to.file_name()) else {
        return false;
    };
    if from.parent() != to.parent() || fname == tname {
        return false;
    }
    if fname.to_string_lossy().to_lowercase() != tname.to_string_lossy().to_lowercase() {
        return false;
    }
    same_file(from, to)
}

#[cfg(unix)]
fn same_file(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (fs::symlink_metadata(a), fs::symlink_metadata(b)) {
        (Ok(x), Ok(y)) => x.dev() == y.dev() && x.ino() == y.ino(),
        _ => false,
    }
}

#[cfg(not(unix))]
fn same_file(a: &Path, b: &Path) -> bool {
    // Without inode numbers, compare the canonical paths the OS reports.
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// `a.txt` → `.a.txt.vr-tmp-…` → `A.txt`; on failure of the second step the
/// temp name is moved back, so the file always keeps one of its names.
fn rename_via_temp(from: &Path, to: &Path, sys: &RenameFn) -> Result<(), FsOpError> {
    let name = from.file_name().unwrap_or_default().to_string_lossy();
    let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = from.with_file_name(format!(".{name}.vr-tmp-{}-{seq}", std::process::id()));
    move_with(from, &tmp, sys)?;
    match move_with(&tmp, to, sys) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = move_with(&tmp, from, sys);
            Err(e)
        }
    }
}

type RenameFn = dyn Fn(&Path, &Path) -> io::Result<()>;

/// `renameat2(RENAME_NOREPLACE)`: fails with EEXIST instead of replacing.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn rename_noreplace(from: &Path, to: &Path) -> io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    renameat_with(CWD, from, CWD, to, RenameFlags::NOREPLACE).map_err(io::Error::from)
}

/// No portable no-replace rename here; `move_with` takes the fallback path.
#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn rename_noreplace(_from: &Path, _to: &Path) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// Moves `from` to `to` without ever replacing `to`. `sys` is the
/// no-replace rename (a seam so tests can inject EXDEV or EINVAL).
fn move_with(from: &Path, to: &Path, sys: &RenameFn) -> Result<(), FsOpError> {
    let e = match sys(from, to) {
        Ok(()) => return Ok(()),
        Err(e) => e,
    };
    match e.kind() {
        // EINVAL / ENOSYS: kernel or filesystem without RENAME_NOREPLACE.
        io::ErrorKind::InvalidInput | io::ErrorKind::Unsupported => {}
        _ => return Err(move_err(from, to, e)),
    }
    let meta = fs::symlink_metadata(from).map_err(io_err(from))?;
    if !meta.is_dir() {
        // link(2) does not follow symlinks and fails with EEXIST atomically.
        match fs::hard_link(from, to) {
            Ok(()) => return fs::remove_file(from).map_err(io_err(from)),
            Err(e)
                if !matches!(
                    e.kind(),
                    io::ErrorKind::PermissionDenied | io::ErrorKind::Unsupported
                ) =>
            {
                return Err(move_err(from, to, e));
            }
            // Filesystem without hard links (vfat, exfat): checked rename below.
            Err(_) => {}
        }
    }
    // Checked rename. Race window: something created at `to` between the check
    // and rename(2) could be replaced. For a directory source rename(2) only
    // replaces an empty directory (non-empty → ENOTEMPTY, file → ENOTDIR), so
    // no file content can be lost; files reach this only on filesystems with
    // neither RENAME_NOREPLACE nor hard links.
    if exists(to) {
        return Err(FsOpError::AlreadyExists(to.to_path_buf()));
    }
    fs::rename(from, to).map_err(|e| move_err(from, to, e))
}

/// Copies one entry. The bool says whether `to` had been created when the
/// error happened (the caller turns that into `PartialCopy`).
fn copy_entry(from: &Path, to: &Path) -> Result<(), (bool, FsOpError)> {
    let fail = |created: bool| move |e: FsOpError| (created, e);
    let meta = fs::symlink_metadata(from)
        .map_err(io_err(from))
        .map_err(fail(false))?;
    let ft = meta.file_type();
    if ft.is_symlink() {
        let target = fs::read_link(from)
            .map_err(io_err(from))
            .map_err(fail(false))?;
        return make_symlink(&target, to).map_err(fail(false));
    }
    if ft.is_dir() {
        let entries = fs::read_dir(from)
            .map_err(io_err(from))
            .map_err(fail(false))?;
        fs::create_dir(to)
            .map_err(io_err(to))
            .map_err(fail(false))?;
        for entry in entries {
            let entry = entry.map_err(io_err(from)).map_err(fail(true))?;
            copy_entry(&entry.path(), &to.join(entry.file_name())).map_err(|(_, e)| (true, e))?;
        }
        fs::set_permissions(to, meta.permissions())
            .map_err(io_err(to))
            .map_err(fail(true))?;
        return Ok(());
    }
    if !ft.is_file() {
        return Err((
            false,
            FsOpError::Io {
                path: from.to_path_buf(),
                source: io::Error::new(
                    io::ErrorKind::Unsupported,
                    "not a regular file, directory or symlink",
                ),
            },
        ));
    }
    // Open the source first so an unreadable file leaves no empty target.
    let mut src = fs::File::open(from)
        .map_err(io_err(from))
        .map_err(fail(false))?;
    let mut dst = fs::File::create_new(to)
        .map_err(|e| move_err(from, to, e))
        .map_err(fail(false))?;
    io::copy(&mut src, &mut dst)
        .map_err(io_err(to))
        .map_err(fail(true))?;
    dst.set_permissions(meta.permissions())
        .map_err(io_err(to))
        .map_err(fail(true))?;
    Ok(())
}

#[cfg(unix)]
fn make_symlink(target: &Path, link: &Path) -> Result<(), FsOpError> {
    std::os::unix::fs::symlink(target, link).map_err(io_err(link))
}

#[cfg(windows)]
fn make_symlink(target: &Path, link: &Path) -> Result<(), FsOpError> {
    let resolved = link.parent().map(|p| p.join(target));
    let is_dir = resolved.map(|p| p.is_dir()).unwrap_or(false);
    let r = if is_dir {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    };
    r.map_err(io_err(link))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(p: &Path, s: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, s).unwrap();
    }

    fn read(p: &Path) -> String {
        fs::read_to_string(p).unwrap()
    }

    fn all_ok(results: &BatchResult) -> bool {
        results.iter().all(|(_, r)| r.is_ok())
    }

    /// True when permission bits do not stop this process (root, CAP_DAC_*).
    #[cfg(unix)]
    fn privileged(locked: &Path) -> bool {
        fs::read_dir(locked).is_ok() || fs::File::open(locked).is_ok()
    }

    #[test]
    fn create_file_and_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("a/b/new.rs");
        create_file(&f).unwrap();
        assert!(f.is_file());
        assert_eq!(fs::read(&f).unwrap(), b"");
        let d = tmp.path().join("x/y");
        create_dir(&d).unwrap();
        assert!(d.is_dir());
    }

    #[test]
    fn create_existing_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let f = tmp.path().join("f.txt");
        write(&f, "keep");
        assert!(matches!(create_file(&f), Err(FsOpError::AlreadyExists(_))));
        assert_eq!(read(&f), "keep");
        assert!(matches!(
            create_dir(tmp.path()),
            Err(FsOpError::AlreadyExists(_))
        ));
        assert!(matches!(create_dir(&f), Err(FsOpError::AlreadyExists(_))));
    }

    #[test]
    fn rename_file_and_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join("old.txt"), "1");
        rename(&root.join("old.txt"), &root.join("new.txt")).unwrap();
        assert!(!root.join("old.txt").exists());
        assert_eq!(read(&root.join("new.txt")), "1");

        write(&root.join("dir/inner.txt"), "2");
        rename(&root.join("dir"), &root.join("renamed")).unwrap();
        assert_eq!(read(&root.join("renamed/inner.txt")), "2");

        write(&root.join("other.txt"), "3");
        assert!(matches!(
            rename(&root.join("other.txt"), &root.join("new.txt")),
            Err(FsOpError::AlreadyExists(_))
        ));
        assert_eq!(read(&root.join("new.txt")), "1");
        assert!(matches!(
            rename(&root.join("nope"), &root.join("x")),
            Err(FsOpError::NotFound(_))
        ));
    }

    #[test]
    fn rename_existing_target_refused_dirs_too() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join("a/x.txt"), "a");
        // An empty directory is the one target plain rename(2) would replace.
        fs::create_dir(root.join("empty")).unwrap();
        let err = rename(&root.join("a"), &root.join("empty")).unwrap_err();
        assert!(matches!(err, FsOpError::AlreadyExists(p) if p == root.join("empty")));
        assert_eq!(read(&root.join("a/x.txt")), "a");
        assert!(root.join("empty").is_dir());
    }

    #[test]
    fn rename_case_only() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join("a.txt"), "content");
        rename(&root.join("a.txt"), &root.join("A.txt")).unwrap();
        let names: Vec<String> = fs::read_dir(root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["A.txt"]);
        assert_eq!(read(&root.join("A.txt")), "content");
        // Renaming to the same name is a no-op, not an error.
        rename(&root.join("A.txt"), &root.join("A.txt")).unwrap();
        assert_eq!(read(&root.join("A.txt")), "content");
    }

    #[cfg(unix)]
    #[test]
    fn rename_case_only_same_inode_goes_through_temp() {
        // Two hard links whose names differ in case are what a case-insensitive
        // filesystem looks like to `rename`: same inode under both spellings.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join("b.txt"), "one");
        fs::hard_link(root.join("b.txt"), root.join("B.txt")).unwrap();
        // On a case-sensitive fs `B.txt` really exists, so the temp route must
        // refuse and put `b.txt` back.
        let err = rename(&root.join("b.txt"), &root.join("B.txt")).unwrap_err();
        assert!(matches!(err, FsOpError::AlreadyExists(p) if p == root.join("B.txt")));
        assert_eq!(read(&root.join("b.txt")), "one");
        assert_eq!(read(&root.join("B.txt")), "one");
        assert_eq!(
            fs::read_dir(root).unwrap().count(),
            2,
            "no temp left behind"
        );
    }

    #[cfg(unix)]
    #[test]
    fn rename_and_move_symlink_move_the_link() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join("outside/target.txt"), "t");
        fs::create_dir(root.join("proj")).unwrap();
        fs::create_dir(root.join("proj/dest")).unwrap();
        symlink(root.join("outside"), root.join("proj/link")).unwrap();
        rename(&root.join("proj/link"), &root.join("proj/renamed")).unwrap();
        let to = move_to(&root.join("proj/renamed"), &root.join("proj/dest")).unwrap();
        assert!(fs::symlink_metadata(&to).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_link(&to).unwrap(), root.join("outside"));
        assert_eq!(read(&root.join("outside/target.txt")), "t");
        assert_eq!(fs::read_dir(root.join("outside")).unwrap().count(), 1);
    }

    #[test]
    fn destination_errors_name_the_destination() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join("x.txt"), "x");
        let to = root.join("missing_dir/y.txt");
        let err = rename(&root.join("x.txt"), &to).unwrap_err();
        assert!(matches!(err, FsOpError::NotFound(p) if p == to));
        let err = copy(&root.join("x.txt"), &to).unwrap_err();
        assert!(matches!(err, FsOpError::NotFound(p) if p == to));
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn move_cross_device_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join("src/a.txt"), "a");
        let exdev = |_: &Path, _: &Path| Err(io::Error::from(rustix::io::Errno::XDEV));
        let to = root.join("other");
        let err = move_with(&root.join("src"), &to, &exdev).unwrap_err();
        assert!(matches!(err, FsOpError::CrossDevice { .. }), "{err:?}");
        assert_eq!(read(&root.join("src/a.txt")), "a");
        assert!(!to.exists(), "nothing copied");
        let err = move_with(&root.join("src/a.txt"), &root.join("b.txt"), &exdev).unwrap_err();
        assert!(matches!(err, FsOpError::CrossDevice { .. }), "{err:?}");
        assert_eq!(read(&root.join("src/a.txt")), "a");
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn move_fallback_without_renameat2_never_overwrites() {
        // Filesystems without RENAME_NOREPLACE answer EINVAL.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let einval = |_: &Path, _: &Path| Err(io::Error::from(rustix::io::Errno::INVAL));
        write(&root.join("a.txt"), "a");
        write(&root.join("b.txt"), "b");
        let err = move_with(&root.join("a.txt"), &root.join("b.txt"), &einval).unwrap_err();
        assert!(matches!(err, FsOpError::AlreadyExists(p) if p == root.join("b.txt")));
        assert_eq!(read(&root.join("a.txt")), "a");
        assert_eq!(read(&root.join("b.txt")), "b");
        move_with(&root.join("a.txt"), &root.join("c.txt"), &einval).unwrap();
        assert!(!root.join("a.txt").exists());
        assert_eq!(read(&root.join("c.txt")), "a");
        write(&root.join("d/x.txt"), "x");
        fs::create_dir(root.join("e")).unwrap();
        let err = move_with(&root.join("d"), &root.join("e"), &einval).unwrap_err();
        assert!(matches!(err, FsOpError::AlreadyExists(_)));
        move_with(&root.join("d"), &root.join("f"), &einval).unwrap();
        assert_eq!(read(&root.join("f/x.txt")), "x");
    }

    #[test]
    fn copy_dir_recursive() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join("src/a.rs"), "a");
        write(&root.join("src/deep/b.rs"), "b");
        #[cfg(unix)]
        std::os::unix::fs::symlink("..", root.join("src/deep/up")).unwrap();
        copy(&root.join("src"), &root.join("copy")).unwrap();
        assert_eq!(read(&root.join("copy/a.rs")), "a");
        assert_eq!(read(&root.join("copy/deep/b.rs")), "b");
        assert_eq!(read(&root.join("src/a.rs")), "a");
        #[cfg(unix)]
        {
            let link = root.join("copy/deep/up");
            assert!(
                fs::symlink_metadata(&link)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            assert_eq!(fs::read_link(&link).unwrap(), PathBuf::from(".."));
        }
        copy(&root.join("src/a.rs"), &root.join("a_copy.rs")).unwrap();
        assert_eq!(read(&root.join("a_copy.rs")), "a");
        assert!(matches!(
            copy(&root.join("src/a.rs"), &root.join("a_copy.rs")),
            Err(FsOpError::AlreadyExists(_))
        ));
        assert!(matches!(
            copy(&root.join("src"), &root.join("src/deep/again")),
            Err(FsOpError::IntoItself { .. })
        ));
    }

    #[test]
    fn copy_dir_onto_existing_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join("src/a.rs"), "new");
        write(&root.join("dst/a.rs"), "old");
        write(&root.join("dst/keep.rs"), "keep");
        let err = copy(&root.join("src"), &root.join("dst")).unwrap_err();
        assert!(matches!(err, FsOpError::AlreadyExists(p) if p == root.join("dst")));
        assert_eq!(read(&root.join("dst/a.rs")), "old");
        assert_eq!(read(&root.join("dst/keep.rs")), "keep");
        assert_eq!(fs::read_dir(root.join("dst")).unwrap().count(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn copy_partial_failure_reports_target() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join("src/a.txt"), "a");
        write(&root.join("src/secret.txt"), "s");
        let secret = root.join("src/secret.txt");
        fs::set_permissions(&secret, fs::Permissions::from_mode(0o000)).unwrap();
        if privileged(&secret) {
            fs::set_permissions(&secret, fs::Permissions::from_mode(0o644)).unwrap();
            eprintln!("skipped: permission bits do not apply to this user");
            return;
        }
        let to = root.join("dst");
        let err = copy(&root.join("src"), &to);
        fs::set_permissions(&secret, fs::Permissions::from_mode(0o644)).unwrap();
        match err {
            Err(FsOpError::PartialCopy {
                to: partial,
                source,
            }) => {
                assert_eq!(partial, to);
                assert!(matches!(*source, FsOpError::Io { ref path, .. } if *path == secret));
            }
            other => panic!("expected PartialCopy, got {other:?}"),
        }
        assert_eq!(read(&root.join("src/a.txt")), "a");
        assert_eq!(read(&secret), "s");
        assert!(
            !to.join("secret.txt").exists(),
            "no empty file for the unreadable one"
        );
    }

    #[cfg(unix)]
    #[test]
    fn copy_refuses_special_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("src")).unwrap();
        let _sock = std::os::unix::net::UnixListener::bind(root.join("src/s.sock")).unwrap();
        let err = copy(&root.join("src/s.sock"), &root.join("copy.sock")).unwrap_err();
        assert!(
            matches!(err, FsOpError::Io { ref source, .. } if source.kind() == io::ErrorKind::Unsupported),
            "{err:?}"
        );
        assert!(!root.join("copy.sock").exists());
    }

    #[test]
    fn move_into_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join("a.txt"), "a");
        write(&root.join("pkg/inner.txt"), "i");
        fs::create_dir(root.join("dest")).unwrap();
        let to = move_to(&root.join("a.txt"), &root.join("dest")).unwrap();
        assert_eq!(to, root.join("dest/a.txt"));
        assert!(!root.join("a.txt").exists());
        let to = move_to(&root.join("pkg"), &root.join("dest")).unwrap();
        assert_eq!(read(&to.join("inner.txt")), "i");

        write(&root.join("a.txt"), "second");
        assert!(matches!(
            move_to(&root.join("a.txt"), &root.join("dest")),
            Err(FsOpError::AlreadyExists(_))
        ));
        assert_eq!(read(&root.join("dest/a.txt")), "a");
        assert_eq!(read(&root.join("a.txt")), "second");
    }

    #[test]
    fn move_into_self_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join("pkg/sub/x.txt"), "x");
        assert!(matches!(
            move_to(&root.join("pkg"), &root.join("pkg")),
            Err(FsOpError::IntoItself { .. })
        ));
        assert!(matches!(
            move_to(&root.join("pkg"), &root.join("pkg/sub")),
            Err(FsOpError::IntoItself { .. })
        ));
        assert!(matches!(
            rename(&root.join("pkg"), &root.join("pkg/sub/pkg2")),
            Err(FsOpError::IntoItself { .. })
        ));
        assert!(root.join("pkg/sub/x.txt").is_file());
        fs::create_dir(root.join("pkg2")).unwrap();
        move_to(&root.join("pkg2"), &root.join("pkg/sub")).unwrap();
    }

    #[test]
    fn validate_name_rejects_bad_names() {
        for bad in ["", ".", "..", "a/b", "/", "nul\0x"] {
            assert!(
                matches!(validate_name(bad), Err(FsOpError::InvalidName(_))),
                "{bad:?} accepted"
            );
        }
        for good in ["a.txt", ".env", "..hidden", "with space", "Tiếng Việt.md"] {
            validate_name(good).unwrap();
        }
    }

    #[test]
    fn ensure_within_refuses_outside() {
        let root = Path::new("/proj");
        assert_eq!(
            ensure_within(root, Path::new("src/../lib/./a.rs")).unwrap(),
            PathBuf::from("/proj/lib/a.rs")
        );
        assert_eq!(
            ensure_within(root, Path::new("/proj/x")).unwrap(),
            PathBuf::from("/proj/x")
        );
        assert_eq!(
            ensure_within(root, Path::new("")).unwrap(),
            PathBuf::from("/proj")
        );
        for bad in [
            "../x",
            "src/../../x",
            "/etc/passwd",
            "/proj/../proj2/a",
            "/projx",
        ] {
            assert!(
                matches!(
                    ensure_within(root, Path::new(bad)),
                    Err(FsOpError::OutsideRoot(_))
                ),
                "{bad} accepted"
            );
        }
    }

    #[test]
    fn fake_trash_records_and_removes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join("a.txt"), "a");
        write(&root.join("d/b.txt"), "b");
        let fake = FakeTrash::new(root.join(".holding"));
        let paths = vec![root.join("a.txt"), root.join("d")];
        assert!(all_ok(&fake.trash(&paths)));
        assert!(!root.join("a.txt").exists() && !root.join("d").exists());
        assert_eq!(fake.trashed(), paths);
    }

    #[test]
    fn trash_batch_reports_per_item() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write(&root.join("a.txt"), "a");
        write(&root.join("c.txt"), "c");
        let fake = FakeTrash::new(root.join(".holding"));
        let paths = vec![root.join("a.txt"), root.join("missing"), root.join("c.txt")];
        let results = fake.trash(&paths);
        let got: Vec<(PathBuf, bool)> = results
            .iter()
            .map(|(p, r)| (p.clone(), r.is_ok()))
            .collect();
        assert_eq!(
            got,
            [
                (paths[0].clone(), true),
                (paths[1].clone(), false),
                (paths[2].clone(), true)
            ]
        );
        assert!(matches!(results[1].1, Err(FsOpError::NotFound(_))));
        assert_eq!(fake.trashed(), [paths[0].clone(), paths[2].clone()]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn trash_refuses_cross_device_symlink() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("outside")).unwrap();
        symlink(root.join("outside"), root.join("link")).unwrap();
        write(&root.join("plain.txt"), "p");
        // /proc is always its own filesystem, so it stands in for a trash on another device.
        let other_dev = Path::new("/proc/self");
        if !other_dev.exists() {
            return;
        }
        let err = check_symlink_trash(&root.join("link"), other_dev).unwrap_err();
        assert!(matches!(err, FsOpError::TrashRefused { .. }), "{err:?}");
        // A trash dir that does not exist yet is judged by its nearest existing parent.
        check_symlink_trash(&root.join("link"), &root.join("data/Trash")).unwrap();
        check_symlink_trash(&root.join("plain.txt"), other_dev).unwrap();
        assert!(root.join("link").exists() && root.join("outside").is_dir());
    }

    const CHILD_PROJECT_VAR: &str = "VR_FS_TRASH_TEST_PROJECT";
    const CHILD_MODE_VAR: &str = "VR_FS_TRASH_TEST_MODE";

    /// Runs `trash_child` in a child copy of this test binary with
    /// XDG_DATA_HOME pointing into the temp dir, so the user's trash is never
    /// touched and no test mutates this process's environment.
    #[cfg(all(unix, not(target_os = "macos")))]
    fn run_trash_child(data_home: &Path, project: &Path, mode: &str) {
        let exe = std::env::current_exe().unwrap();
        let out = std::process::Command::new(exe)
            .args([
                "--exact",
                "ops::tests::trash_child",
                "--ignored",
                "--nocapture",
            ])
            .env("XDG_DATA_HOME", data_home)
            .env(CHILD_PROJECT_VAR, project)
            .env(CHILD_MODE_VAR, mode)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "child failed: {stdout}{stderr}");
        assert!(stdout.contains("1 passed"), "child did not run: {stdout}");
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn trash_moves_to_trash() {
        let tmp = tempfile::tempdir().unwrap();
        let data_home = tmp.path().join("data");
        fs::create_dir(&data_home).unwrap();
        let project = tmp.path().join("project");
        write(&project.join("gone.txt"), "bye");
        write(&project.join("dir/x.txt"), "x");
        run_trash_child(&data_home, &project, "ok");
        assert!(!project.join("gone.txt").exists());
        assert!(!project.join("dir").exists());
        let files = data_home.join("Trash/files");
        assert_eq!(read(&files.join("gone.txt")), "bye");
        assert_eq!(read(&files.join("dir/x.txt")), "x");
        assert!(data_home.join("Trash/info/gone.txt.trashinfo").is_file());
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn trash_unavailable_keeps_file() {
        let tmp = tempfile::tempdir().unwrap();
        // XDG_DATA_HOME is a regular file, so no trash directory can be made
        // under it, whatever the user's privileges.
        let data_home = tmp.path().join("data");
        fs::write(&data_home, "not a dir").unwrap();
        let project = tmp.path().join("project");
        write(&project.join("keep.txt"), "still here");
        run_trash_child(&data_home, &project, "unavailable");
        assert_eq!(read(&project.join("keep.txt")), "still here");
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn trash_symlink_keeps_outside_target() {
        let tmp = tempfile::tempdir().unwrap();
        let data_home = tmp.path().join("data");
        fs::create_dir(&data_home).unwrap();
        let outside = tmp.path().join("outside");
        write(&outside.join("precious.txt"), "p");
        let project = tmp.path().join("project");
        fs::create_dir(&project).unwrap();
        std::os::unix::fs::symlink(&outside, project.join("link")).unwrap();
        run_trash_child(&data_home, &project, "symlink");
        assert!(fs::symlink_metadata(project.join("link")).is_err());
        assert_eq!(read(&outside.join("precious.txt")), "p");
        let trashed = data_home.join("Trash/files/link");
        assert!(
            fs::symlink_metadata(&trashed)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    /// Child half of the real-trash tests; does nothing unless started by them.
    #[test]
    #[ignore = "started by the trash_* tests with a temp XDG_DATA_HOME"]
    fn trash_child() {
        let Some(project) = std::env::var_os(CHILD_PROJECT_VAR) else {
            return;
        };
        let data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
        let project = PathBuf::from(project);
        // Refuse unless the trash really lives beside the temp project.
        assert!(data_home.is_some_and(|d| d.parent() == project.parent()));
        let mode = std::env::var(CHILD_MODE_VAR).unwrap_or_default();
        match mode.as_str() {
            "ok" => {
                let results = trash(&[project.join("gone.txt"), project.join("dir")]);
                assert!(all_ok(&results), "{results:?}");
                let again = trash(&[project.join("gone.txt")]);
                assert!(matches!(again[0].1, Err(FsOpError::NotFound(_))));
            }
            "unavailable" => {
                let results = trash(&[project.join("keep.txt")]);
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].0, project.join("keep.txt"));
                assert!(results[0].1.is_err(), "{results:?}");
            }
            "symlink" => {
                let results = trash(&[project.join("link")]);
                assert!(all_ok(&results), "{results:?}");
            }
            other => panic!("unknown mode {other:?}"),
        }
    }
}
