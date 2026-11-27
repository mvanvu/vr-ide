//! Shared conversation history between the accounts of one AI tool (AC-2, SEC-19..SEC-21).
//!
//! Every named account's history entry (`projects` for claude, `sessions` for codex) becomes a
//! symlink to the default account's history folder, so `/resume` and a switch between accounts
//! see one history; logins stay per account. This is the only code that looks inside an account
//! folder, and only at that one entry: absent → linked; already linked there → kept; a real
//! folder → its content is moved into the shared folder (once, under the account's exclusive
//! lock, fd-based, never replacing a file) and the emptied folder becomes the link; anything else
//! (a link elsewhere, a file, a clash, a link inside) leaves everything as found and the account
//! "not shared" with a fixed reason. File contents are never opened.

use std::collections::BTreeMap;
use std::ffi::{CString, OsStr, OsString};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};

use rustix::fs::{AtFlags, FileType, Mode, OFlags};
use rustix::io::Errno;

use super::{AiPanel, accounts};

/// At most this many names are looked at in one migration (SEC-20, T-27).
pub const MAX_ENTRIES: usize = 200_000;
/// At most this many folder levels below the history entry.
pub const MAX_DEPTH: usize = 32;

/// The history entry inside an account folder of `tool`; a fixed map, never from input.
pub fn entry(tool: &str) -> Option<&'static str> {
    match tool {
        "claude" => Some("projects"),
        "codex" => Some("sessions"),
        _ => None,
    }
}

/// Whether an account's history is the tool's shared one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shared {
    Linked,
    /// Left as found; the text says why (fixed texts, no names from inside the account).
    NotShared(String),
}

/// The default account's history folder of `tool` (D1, SEC-21): claude
/// `${CLAUDE_CONFIG_DIR:-$HOME/.claude}/projects`, codex `${CODEX_HOME:-$HOME/.codex}/sessions`,
/// from vr-ide's own environment; an empty or relative value counts as unset.
pub fn shared_dir(
    tool: &str,
    home: &Path,
    claude_config_dir: Option<&OsStr>,
    codex_home: Option<&OsStr>,
) -> Option<PathBuf> {
    let name = entry(tool)?;
    let (var, fallback) = match tool {
        "claude" => (claude_config_dir, ".claude"),
        "codex" => (codex_home, ".codex"),
        _ => return None,
    };
    let base = var
        .map(Path::new)
        .filter(|p| p.is_absolute())
        .map_or_else(|| home.join(fallback), Path::to_path_buf);
    Some(base.join(name))
}

/// [`shared_dir`] for every tool with history, ignoring an inherited `CLAUDE_CONFIG_DIR` /
/// `CODEX_HOME` that points into `<state>/accounts/` (SEC-38). No home: nothing is shared.
pub fn dirs(
    state: &Path,
    home: Option<&Path>,
    claude_config_dir: Option<&OsStr>,
    codex_home: Option<&OsStr>,
) -> BTreeMap<String, PathBuf> {
    let Some(home) = home else {
        return BTreeMap::new();
    };
    let ccd = claude_config_dir.filter(|v| !points_into_accounts(state, Path::new(v)));
    let cxh = codex_home.filter(|v| !points_into_accounts(state, Path::new(v)));
    ["claude", "codex"]
        .into_iter()
        .filter_map(|t| Some((t.to_string(), shared_dir(t, home, ccd, cxh)?)))
        .collect()
}

/// The inherited `CLAUDE_CONFIG_DIR` / `CODEX_HOME` that point into vr-ide's own account
/// folders (SEC-38, T-48): a child started without an account must not run on one of them.
pub fn stray_vars(state: &Path, vars: impl Iterator<Item = (OsString, OsString)>) -> Vec<OsString> {
    let ours = [accounts::var_for("claude"), accounts::var_for("codex")];
    vars.filter(|(k, v)| {
        ours.iter().flatten().any(|o| k == o)
            && Path::new(v).is_absolute()
            && points_into_accounts(state, Path::new(v))
    })
    .map(|(k, _)| k)
    .collect()
}

/// One startup warning per [`stray_vars`] entry: the value is ignored for vr-ide's children.
pub fn stray_warnings(
    state: &Path,
    vars: impl Iterator<Item = (OsString, OsString)>,
) -> Vec<vr_config::ConfigWarning> {
    stray_vars(state, vars)
        .into_iter()
        .map(|k| vr_config::ConfigWarning::InvalidValue {
            file: state.to_path_buf(),
            key: k.to_string_lossy().into_owned(),
            message: "inherited value points into vr-ide's account folders; it is ignored for the terminals, ssh sessions and agents vr-ide starts".into(),
        })
        .collect()
}

/// Startup: the shared folders of the panel, which change recording also skips (SEC-10).
pub fn configure(
    panel: &mut AiPanel,
    state: &Path,
    home: Option<&Path>,
    claude_config_dir: Option<&OsStr>,
    codex_home: Option<&OsStr>,
) {
    panel.history_dirs = dirs(state, home, claude_config_dir, codex_home);
    panel
        .changes_excluded
        .extend(panel.history_dirs.values().cloned());
}

/// `p` is `<state>/accounts` or below it, once links and `..` are resolved as far as the path
/// exists. A path that cannot be resolved (relative, `..` past a missing part) counts as inside:
/// callers then ignore it.
fn points_into_accounts(state: &Path, p: &Path) -> bool {
    let accounts = accounts::accounts_dir(state);
    let real = resolve(&accounts).unwrap_or_else(|| accounts.clone());
    resolve(p).is_none_or(|r| r.starts_with(&accounts) || r.starts_with(&real))
}

/// The real path of `p`: its longest existing ancestor canonicalised, the missing rest appended.
fn resolve(p: &Path) -> Option<PathBuf> {
    let mut rest: Vec<&OsStr> = Vec::new();
    let mut cur = p;
    loop {
        if let Ok(real) = std::fs::canonicalize(cur) {
            return Some(rest.iter().rev().fold(real, |acc, r| acc.join(r)));
        }
        let mut parts = cur.components();
        match parts.next_back() {
            Some(Component::Normal(n)) => {
                rest.push(n);
                cur = parts.as_path();
            }
            _ => return None,
        }
    }
}

/// Makes `account_dir`'s history entry of `tool` a link to `shared` (AC-2), creating `shared`
/// (0700) when missing. Runs before the account's session takes its hold.
pub fn ensure(account_dir: &Path, tool: &str, shared: &Path) -> Shared {
    ensure_bounded(account_dir, tool, shared, MAX_ENTRIES, MAX_DEPTH)
}

fn ensure_bounded(
    account_dir: &Path,
    tool: &str,
    shared: &Path,
    max_entries: usize,
    max_depth: usize,
) -> Shared {
    let bounds = Bounds {
        entries: max_entries,
        depth: max_depth,
    };
    match link(account_dir, tool, shared, bounds) {
        Ok(()) => Shared::Linked,
        Err(why) => Shared::NotShared(why),
    }
}

// Fixed reasons (SEC-19): nothing found inside an account is ever named.
const BAD_ACCOUNT: &str = "not an account folder";
const CHANGED: &str = "history changed meanwhile; shared on a later start";
const UNUSABLE: &str = "history entry unusable";
const ELSEWHERE: &str = "history entry links elsewhere; left as it is";
const NOT_A_FOLDER: &str = "history entry is not a folder; left as it is";
const IN_USE: &str = "in use; shared on a later start";
const OTHER_FS: &str = "history is on another file system than the shared folder; nothing moved";
const TOO_LARGE: &str = "history too large to share";
const TOO_DEEP: &str = "history too deep to share";
const CLASH: &str = "a file exists on both sides; left in place";
const SHARED_CHANGED: &str = "shared history folder changed meanwhile; shared on a later start";
const LINK_INSIDE: &str = "history holds a link or special file; left as it is";
const UNREADABLE: &str = "history unreadable";
const UNWRITABLE: &str = "cannot write to the shared history folder";
const NO_SAFE_MOVE: &str = "files cannot be moved here without a risk of replacing; not moved";

#[derive(Clone, Copy)]
struct Bounds {
    entries: usize,
    depth: usize,
}

/// The checked shared folder: its canonical path (the link target) and a descriptor on it.
struct Target {
    path: PathBuf,
    fd: OwnedFd,
}

fn dir_flags() -> OFlags {
    OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC
}

/// SEC-19: everything happens relative to the account folder's descriptor, on the one name
/// `entry`; only a real folder is entered (SEC-20).
fn link(account_dir: &Path, tool: &str, shared: &Path, bounds: Bounds) -> Result<(), String> {
    let entry = entry(tool).ok_or_else(|| format!("{tool} keeps no history to share"))?;
    let accounts_dir = account_dir.parent().ok_or(BAD_ACCOUNT)?;
    let name = account_dir
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or(BAD_ACCOUNT)?;
    let name = accounts::validate(name).map_err(|_| BAD_ACCOUNT)?;
    if accounts_dir.file_name() != Some(OsStr::new(accounts::ACCOUNTS_DIR)) {
        return Err(BAD_ACCOUNT.into());
    }
    let state = accounts_dir.parent().ok_or(BAD_ACCOUNT)?;
    accounts::secure(accounts_dir)?;
    accounts::secure(account_dir)?;
    let target = shared_folder(state, shared)?;
    let acc = open_account(account_dir)?;
    match rustix::fs::statat(&acc, entry, AtFlags::SYMLINK_NOFOLLOW) {
        Err(Errno::NOENT) => {
            linkable(&target)?;
            rustix::fs::symlinkat(&target.path, &acc, entry).map_err(|_| CHANGED.to_string())
        }
        Err(_) => Err(UNUSABLE.into()),
        Ok(st) => match FileType::from_raw_mode(st.st_mode) {
            FileType::Symlink => {
                let to = rustix::fs::readlinkat(&acc, entry, Vec::new())
                    .map_err(|_| UNUSABLE.to_string())?;
                if to.as_bytes() == target.path.as_os_str().as_bytes() {
                    Ok(())
                } else {
                    Err(ELSEWHERE.into())
                }
            }
            FileType::Directory => {
                drop(acc);
                migrate(state, name, account_dir, entry, &target, bounds)
            }
            _ => Err(NOT_A_FOLDER.into()),
        },
    }
}

/// The folder at `path` is the one `fd` holds, reached through no link at all (T-26): a path
/// part swapped for a link after `canonicalize` would send the files and the link apart.
fn still_at(path: &Path, fd: &OwnedFd) -> bool {
    let (Ok(held), Ok(named)) = (rustix::fs::fstat(fd), rustix::fs::stat(path)) else {
        return false;
    };
    held.st_dev == named.st_dev
        && held.st_ino == named.st_ino
        && std::fs::canonicalize(path).is_ok_and(|c| c == path)
}

/// Right before the link is made: its target is still the folder the files went to.
fn linkable(target: &Target) -> Result<(), String> {
    seam("link", &CString::default());
    if still_at(&target.path, &target.fd) {
        Ok(())
    } else {
        Err(SHARED_CHANGED.into())
    }
}

fn open_account(account_dir: &Path) -> Result<OwnedFd, String> {
    rustix::fs::open(account_dir, dir_flags(), Mode::empty())
        .map_err(|_| "account folder is not a real folder".to_string())
}

/// SEC-21: the shared folder is created (0700) when missing, then refused when it lies inside
/// vr-ide's account folders, is not this user's, or is open to others (never chmod-ed: it is
/// the user's own config).
fn shared_folder(state: &Path, shared: &Path) -> Result<Target, String> {
    let shown = crate::ui::safe_text::sanitize(&crate::git::home_relative(shared));
    let inside = || format!("shared history folder {shown} is inside vr-ide's account folders");
    if !shared.is_absolute() || points_into_accounts(state, shared) {
        return Err(inside());
    }
    if std::fs::symlink_metadata(shared).is_err() {
        crate::recovery::create_private_dir(shared)
            .map_err(|e| format!("cannot create shared history folder {shown} ({e})"))?;
    }
    let path = std::fs::canonicalize(shared)
        .map_err(|e| format!("shared history folder {shown} unusable ({e})"))?;
    if points_into_accounts(state, &path) {
        return Err(inside());
    }
    seam("shared", &CString::default());
    let fd = rustix::fs::open(&path, dir_flags(), Mode::empty())
        .map_err(|_| format!("shared history folder {shown} is not a folder"))?;
    if !still_at(&path, &fd) {
        return Err(format!("shared history folder {shown} changed meanwhile"));
    }
    let st = rustix::fs::fstat(&fd)
        .map_err(|e| format!("shared history folder {shown} unusable ({e})"))?;
    if st.st_uid != rustix::process::geteuid().as_raw() {
        return Err(format!(
            "shared history folder {shown} belongs to another user"
        ));
    }
    if st.st_mode & 0o077 != 0 {
        return Err(format!(
            "shared history folder {shown} is open to others; run chmod 700 {shown}"
        ));
    }
    Ok(Target { path, fd })
}

/// SEC-20: under the account's exclusive hold, a check pass (links, specials, clashes, bounds)
/// moves nothing; the move pass then re-checks each step on descriptors and moves files with a
/// no-replace rename. Only an emptied entry is replaced by the link. A crash leaves every file in
/// exactly one place and the next start continues.
fn migrate(
    state: &Path,
    name: &str,
    account_dir: &Path,
    entry: &str,
    target: &Target,
    bounds: Bounds,
) -> Result<(), String> {
    let _hold = accounts::hold_exclusive(state, name)
        .map_err(|e| if e == "in use" { IN_USE.to_string() } else { e })?;
    let acc = open_account(account_dir)?;
    let src = rustix::fs::openat(&acc, entry, dir_flags(), Mode::empty())
        .map_err(|_| CHANGED.to_string())?;
    let dev = |fd: &OwnedFd| rustix::fs::fstat(fd).map(|s| s.st_dev);
    let (from, to) = (dev(&src), dev(&target.fd));
    if from.is_err() || from != to {
        return Err(OTHER_FS.into());
    }
    let mut left = bounds.entries;
    check(
        src.as_fd(),
        Some(target.fd.as_fd()),
        1,
        bounds.depth,
        &mut left,
    )?;
    let mut left = bounds.entries;
    move_all(src.as_fd(), target.fd.as_fd(), 1, bounds.depth, &mut left)?;
    drop(src);
    linkable(target)?;
    rustix::fs::unlinkat(&acc, entry, AtFlags::REMOVEDIR).map_err(|_| CHANGED.to_string())?;
    rustix::fs::symlinkat(&target.path, &acc, entry).map_err(|_| CHANGED.to_string())
}

/// The names in `dir`, counted against the bound.
fn names(dir: BorrowedFd<'_>, left: &mut usize) -> Result<Vec<CString>, String> {
    let mut out = Vec::new();
    for e in rustix::fs::Dir::read_from(dir).map_err(|_| UNREADABLE.to_string())? {
        let e = e.map_err(|_| UNREADABLE.to_string())?;
        let n = e.file_name();
        if n.to_bytes() == b"." || n.to_bytes() == b".." {
            continue;
        }
        if *left == 0 {
            return Err(TOO_LARGE.into());
        }
        *left -= 1;
        out.push(n.to_owned());
    }
    Ok(out)
}

fn kind_at(dir: BorrowedFd<'_>, name: &CString) -> Result<FileType, String> {
    match rustix::fs::statat(dir, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW) {
        Ok(st) => Ok(FileType::from_raw_mode(st.st_mode)),
        Err(Errno::NOENT) => Err(CHANGED.into()),
        Err(_) => Err(UNREADABLE.into()),
    }
}

/// A sub-folder, never through a link (a link swapped in fails with `ELOOP`).
fn open_at(dir: BorrowedFd<'_>, name: &CString) -> Result<OwnedFd, String> {
    rustix::fs::openat(dir, name.as_c_str(), dir_flags(), Mode::empty()).map_err(|e| match e {
        Errno::LOOP | Errno::NOTDIR => LINK_INSIDE.to_string(),
        _ => UNREADABLE.to_string(),
    })
}

fn check(
    src: BorrowedFd<'_>,
    dst: Option<BorrowedFd<'_>>,
    depth: usize,
    max_depth: usize,
    left: &mut usize,
) -> Result<(), String> {
    for n in names(src, left)? {
        let there = match dst {
            None => None,
            Some(d) => match rustix::fs::statat(d, n.as_c_str(), AtFlags::SYMLINK_NOFOLLOW) {
                Ok(st) => Some(FileType::from_raw_mode(st.st_mode)),
                Err(Errno::NOENT) => None,
                Err(_) => return Err(UNWRITABLE.into()),
            },
        };
        match kind_at(src, &n)? {
            FileType::Directory => {
                if depth > max_depth {
                    return Err(TOO_DEEP.into());
                }
                let sub = open_at(src, &n)?;
                let dsub = match (there, dst) {
                    (None, _) | (_, None) => None,
                    (Some(FileType::Directory), Some(d)) => {
                        Some(open_at(d, &n).map_err(|_| CLASH.to_string())?)
                    }
                    (Some(_), _) => return Err(CLASH.into()),
                };
                check(
                    sub.as_fd(),
                    dsub.as_ref().map(AsFd::as_fd),
                    depth + 1,
                    max_depth,
                    left,
                )?;
            }
            FileType::RegularFile if there.is_some() => return Err(CLASH.into()),
            FileType::RegularFile => {}
            _ => return Err(LINK_INSIDE.into()),
        }
    }
    Ok(())
}

fn move_all(
    src: BorrowedFd<'_>,
    dst: BorrowedFd<'_>,
    depth: usize,
    max_depth: usize,
    left: &mut usize,
) -> Result<(), String> {
    for n in names(src, left)? {
        match kind_at(src, &n)? {
            FileType::Directory => {
                if depth > max_depth {
                    return Err(TOO_DEEP.into());
                }
                seam("enter", &n);
                let sub = open_at(src, &n)?;
                match rustix::fs::mkdirat(dst, n.as_c_str(), Mode::RWXU) {
                    Ok(()) | Err(Errno::EXIST) => {}
                    Err(_) => return Err(UNWRITABLE.into()),
                }
                let dsub = open_at(dst, &n).map_err(|_| CLASH.to_string())?;
                move_all(sub.as_fd(), dsub.as_fd(), depth + 1, max_depth, left)?;
                drop(sub);
                // Fails on a folder that is not empty: never a recursive delete.
                rustix::fs::unlinkat(src, n.as_c_str(), AtFlags::REMOVEDIR)
                    .map_err(|_| CHANGED.to_string())?;
            }
            FileType::RegularFile => {
                seam("move", &n);
                move_file(src, &n, dst).map_err(|e| match e {
                    Errno::EXIST => CLASH.to_string(),
                    Errno::XDEV => OTHER_FS.to_string(),
                    Errno::INVAL | Errno::NOSYS => NO_SAFE_MOVE.to_string(),
                    _ => UNWRITABLE.to_string(),
                })?;
            }
            _ => return Err(LINK_INSIDE.into()),
        }
    }
    Ok(())
}

/// A rename that fails instead of replacing (T-25); without one nothing is moved.
#[cfg(any(target_os = "linux", target_os = "android", target_vendor = "apple"))]
fn move_file(src: BorrowedFd<'_>, name: &CString, dst: BorrowedFd<'_>) -> rustix::io::Result<()> {
    rustix::fs::renameat_with(
        src,
        name.as_c_str(),
        dst,
        name.as_c_str(),
        rustix::fs::RenameFlags::NOREPLACE,
    )
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
fn move_file(_: BorrowedFd<'_>, _: &CString, _: BorrowedFd<'_>) -> rustix::io::Result<()> {
    Err(Errno::NOSYS)
}

fn seam(_step: &str, _name: &CString) {
    #[cfg(test)]
    SEAM.with(|s| {
        if let Some(f) = s.borrow_mut().as_mut() {
            f(&format!("{_step}:{}", _name.to_string_lossy()));
        }
    });
}

#[cfg(test)]
type Seam = Option<Box<dyn FnMut(&str)>>;

#[cfg(test)]
thread_local! {
    /// Tests: runs during the move pass with `enter:<name>` before a source folder is opened
    /// and `move:<name>` before a file is moved.
    static SEAM: std::cell::RefCell<Seam> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    struct Fx {
        d: tempfile::TempDir,
        state: PathBuf,
        account: PathBuf,
        shared: PathBuf,
    }

    fn fx(tool: &str) -> Fx {
        let d = tempfile::tempdir().unwrap();
        let state = d.path().join("state");
        let account = accounts::create(&state, tool, "work").unwrap();
        let shared = d.path().join("home/.claude/projects");
        Fx {
            d,
            state,
            account,
            shared,
        }
    }

    fn mode(p: &Path) -> u32 {
        std::fs::symlink_metadata(p).unwrap().mode() & 0o777
    }

    fn chmod(p: &Path, m: u32) {
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
    }

    fn write(p: &Path, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn read(p: &Path) -> String {
        std::fs::read_to_string(p).unwrap()
    }

    fn canon(p: &Path) -> PathBuf {
        std::fs::canonicalize(p).unwrap()
    }

    /// A shared folder that exists already, private as it must be.
    fn private_shared(f: &Fx) {
        std::fs::create_dir_all(&f.shared).unwrap();
        chmod(&f.shared, 0o700);
    }

    fn not_shared(s: Shared) -> String {
        match s {
            Shared::NotShared(why) => why,
            Shared::Linked => panic!("linked"),
        }
    }

    fn is_real_dir(p: &Path) -> bool {
        std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_dir())
    }

    fn set_seam(f: impl FnMut(&str) + 'static) {
        SEAM.with(|s| *s.borrow_mut() = Some(Box::new(f)));
    }

    #[test]
    fn new_account_gets_link() {
        let f = fx("claude");
        assert_eq!(ensure(&f.account, "claude", &f.shared), Shared::Linked);
        let link = f.account.join("projects");
        assert_eq!(std::fs::read_link(&link).unwrap(), canon(&f.shared));
        assert_eq!(mode(&f.shared), 0o700, "created private");
        let g = fx("codex");
        let shared = g.d.path().join("cx/sessions");
        assert_eq!(ensure(&g.account, "codex", &shared), Shared::Linked);
        assert_eq!(
            std::fs::read_link(g.account.join("sessions")).unwrap(),
            canon(&shared)
        );
        assert!(
            std::fs::symlink_metadata(g.account.join("projects")).is_err(),
            "only the tool's own entry"
        );
        assert!(not_shared(ensure(&g.account, "gemini", &shared)).contains("history"));
    }

    #[test]
    fn correct_link_is_kept() {
        let f = fx("claude");
        assert_eq!(ensure(&f.account, "claude", &f.shared), Shared::Linked);
        let link = f.account.join("projects");
        let ino = std::fs::symlink_metadata(&link).unwrap().ino();
        assert_eq!(ensure(&f.account, "claude", &f.shared), Shared::Linked);
        assert_eq!(std::fs::symlink_metadata(&link).unwrap().ino(), ino);
        assert_eq!(std::fs::read_link(&link).unwrap(), canon(&f.shared));
    }

    #[test]
    fn merge_moves_nested_files() {
        let f = fx("claude");
        let entry = f.account.join("projects");
        write(&entry.join("-p/a.jsonl"), "A");
        write(&entry.join("-p/sub/c.jsonl"), "C");
        write(&entry.join("-q/d.jsonl"), "D");
        write(&f.shared.join("-p/b.jsonl"), "B");
        private_shared(&f);
        assert_eq!(ensure(&f.account, "claude", &f.shared), Shared::Linked);
        assert_eq!(read(&f.shared.join("-p/a.jsonl")), "A");
        assert_eq!(read(&f.shared.join("-p/b.jsonl")), "B");
        assert_eq!(read(&f.shared.join("-p/sub/c.jsonl")), "C");
        assert_eq!(read(&f.shared.join("-q/d.jsonl")), "D");
        assert_eq!(std::fs::read_link(&entry).unwrap(), canon(&f.shared));
        assert_eq!(mode(&f.shared.join("-q")), 0o700, "new folders private");
        // Through the link the account sees everything.
        assert_eq!(read(&entry.join("-p/b.jsonl")), "B");
    }

    #[test]
    fn name_clash_is_not_shared() {
        let f = fx("claude");
        let entry = f.account.join("projects");
        write(&entry.join("-p/a.jsonl"), "mine");
        write(&entry.join("-p/z.jsonl"), "other");
        write(&f.shared.join("-p/a.jsonl"), "theirs");
        private_shared(&f);
        let why = not_shared(ensure(&f.account, "claude", &f.shared));
        // Any file clashes, not only conversations (claude keeps memory files there too).
        assert_eq!(why, "a file exists on both sides; left in place");
        assert_eq!(read(&entry.join("-p/a.jsonl")), "mine");
        assert_eq!(read(&f.shared.join("-p/a.jsonl")), "theirs");
        assert_eq!(read(&entry.join("-p/z.jsonl")), "other", "nothing moved");
        assert!(!f.shared.join("-p/z.jsonl").exists());
        assert!(is_real_dir(&entry));
        // A folder on one side and a file on the other is a clash too.
        let g = fx("claude");
        write(&g.account.join("projects/-p"), "file");
        write(&g.shared.join("-p/x.jsonl"), "x");
        private_shared(&g);
        not_shared(ensure(&g.account, "claude", &g.shared));
        assert_eq!(read(&g.account.join("projects/-p")), "file");
    }

    #[test]
    fn link_elsewhere_is_refused_and_kept() {
        let outside = tempfile::tempdir().unwrap();
        write(&outside.path().join("keep"), "k");
        let f = fx("claude");
        let entry = f.account.join("projects");
        std::os::unix::fs::symlink(outside.path(), &entry).unwrap();
        not_shared(ensure(&f.account, "claude", &f.shared));
        assert_eq!(std::fs::read_link(&entry).unwrap(), outside.path());
        assert_eq!(read(&outside.path().join("keep")), "k");
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 1);
        // A regular file.
        let g = fx("claude");
        write(&g.account.join("projects"), "f");
        not_shared(ensure(&g.account, "claude", &g.shared));
        assert_eq!(read(&g.account.join("projects")), "f");
        // A FIFO: refused without opening it (no hang).
        let h = fx("claude");
        let fifo = h.account.join("projects");
        rustix::fs::mknodat(rustix::fs::CWD, &fifo, FileType::Fifo, Mode::RUSR, 0).unwrap();
        not_shared(ensure(&h.account, "claude", &h.shared));
        let t = std::fs::symlink_metadata(&fifo).unwrap().file_type();
        assert!(std::os::unix::fs::FileTypeExt::is_fifo(&t));
    }

    #[test]
    fn codex_shared_dir_honours_codex_home() {
        let home = Path::new("/h");
        let x = Some(OsStr::new("/x"));
        assert_eq!(
            shared_dir("codex", home, None, x),
            Some(PathBuf::from("/x/sessions"))
        );
        assert_eq!(
            shared_dir("codex", home, None, None),
            Some(PathBuf::from("/h/.codex/sessions"))
        );
        for unusable in ["", "rel/dir"] {
            assert_eq!(
                shared_dir("codex", home, None, Some(OsStr::new(unusable))),
                Some(PathBuf::from("/h/.codex/sessions"))
            );
        }
        assert_eq!(
            shared_dir("claude", home, None, x),
            Some(PathBuf::from("/h/.claude/projects"))
        );
        // D1: claude follows CLAUDE_CONFIG_DIR as codex follows CODEX_HOME.
        assert_eq!(
            shared_dir("claude", home, Some(OsStr::new("/c")), None),
            Some(PathBuf::from("/c/projects"))
        );
        assert_eq!(shared_dir("gemini", home, None, x), None);
        assert_eq!(entry("claude"), Some("projects"));
        assert_eq!(entry("codex"), Some("sessions"));
        assert_eq!(entry("aider"), None);
    }

    /// SEC-20: a session on the account (any window) holds it; the migration waits for a later
    /// start and moves nothing meanwhile.
    #[test]
    fn busy_account_is_skipped() {
        let f = fx("claude");
        let entry = f.account.join("projects");
        write(&entry.join("-p/a.jsonl"), "A");
        let held = accounts::hold(&f.state, "work").unwrap();
        assert!(not_shared(ensure(&f.account, "claude", &f.shared)).contains("in use"));
        assert_eq!(read(&entry.join("-p/a.jsonl")), "A");
        assert!(!f.shared.join("-p").exists());
        drop(held);
        assert_eq!(ensure(&f.account, "claude", &f.shared), Shared::Linked);
        assert_eq!(read(&f.shared.join("-p/a.jsonl")), "A");
    }

    /// SEC-20: no copy-then-delete across file systems. Needs a tmpfs `/dev/shm` on another
    /// device than the temp folder; skipped (with a line on stderr) where there is none.
    #[test]
    fn cross_filesystem_is_not_shared() {
        let Ok(other) = tempfile::tempdir_in("/dev/shm") else {
            eprintln!("skipped: no /dev/shm");
            return;
        };
        let f = fx("claude");
        let dev = |p: &Path| std::fs::metadata(p).unwrap().dev();
        if dev(other.path()) == dev(&f.account) {
            eprintln!("skipped: /dev/shm is on the same device");
            return;
        }
        let shared = other.path().join("projects");
        let entry = f.account.join("projects");
        write(&entry.join("-p/a.jsonl"), "A");
        assert!(not_shared(ensure(&f.account, "claude", &shared)).contains("file system"));
        assert_eq!(read(&entry.join("-p/a.jsonl")), "A");
        assert!(!shared.join("-p").exists());
        assert!(is_real_dir(&entry));
    }

    /// SEC-20: a link (or FIFO) anywhere inside stops the migration before anything moves.
    #[test]
    fn link_inside_entry_is_not_shared() {
        let outside = tempfile::tempdir().unwrap();
        write(&outside.path().join("keep"), "k");
        let f = fx("claude");
        let entry = f.account.join("projects");
        write(&entry.join("-a/a.jsonl"), "A");
        std::fs::create_dir_all(entry.join("-p")).unwrap();
        std::os::unix::fs::symlink(outside.path(), entry.join("-p/out")).unwrap();
        not_shared(ensure(&f.account, "claude", &f.shared));
        assert_eq!(read(&entry.join("-a/a.jsonl")), "A", "nothing moved");
        assert!(!f.shared.join("-a").exists());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 1);
        assert!(entry.join("-p/out").is_symlink());
    }

    /// SEC-21: a shared folder others can read or write is refused, never chmod-ed.
    #[test]
    fn shared_folder_open_to_others_is_not_shared() {
        for m in [0o755, 0o750, 0o703] {
            let f = fx("claude");
            std::fs::create_dir_all(&f.shared).unwrap();
            chmod(&f.shared, m);
            let why = not_shared(ensure(&f.account, "claude", &f.shared));
            assert!(why.contains("chmod 700"), "{why}");
            assert_eq!(mode(&f.shared), m, "mode unchanged");
            assert!(std::fs::symlink_metadata(f.account.join("projects")).is_err());
        }
    }

    /// SEC-21: the shared folder may not be inside vr-ide's account folders (another account's
    /// entry, or the account's own).
    #[test]
    fn shared_folder_inside_accounts_is_refused() {
        let f = fx("claude");
        let other = accounts::create(&f.state, "claude", "other").unwrap();
        not_shared(ensure(&f.account, "claude", &other.join("projects")));
        assert!(std::fs::symlink_metadata(other.join("projects")).is_err());
        let own = f.account.join("projects");
        not_shared(ensure(&f.account, "claude", &own));
        assert!(std::fs::symlink_metadata(&own).is_err(), "nothing created");
        // Reached through `..` or a link to the state folder.
        let sneaky = f.state.join("accounts/../accounts/other/projects");
        not_shared(ensure(&f.account, "claude", &sneaky));
        let via = f.d.path().join("via");
        std::os::unix::fs::symlink(&f.state, &via).unwrap();
        not_shared(ensure(&f.account, "claude", &via.join("accounts/other/x")));
        assert!(std::fs::symlink_metadata(other.join("x")).is_err());
    }

    /// SEC-20 / T-24: a source folder swapped for a link between the check and the move is not
    /// entered; the outside folder is untouched.
    #[test]
    fn swapped_subdir_is_not_followed() {
        let outside = tempfile::tempdir().unwrap();
        write(&outside.path().join("keep"), "k");
        let f = fx("claude");
        let entry = f.account.join("projects");
        write(&entry.join("-p/sub/a.jsonl"), "A");
        let parked = f.d.path().join("parked");
        let (sub, out, park) = (
            entry.join("-p/sub"),
            outside.path().to_path_buf(),
            parked.clone(),
        );
        set_seam(move |step| {
            if step == "enter:sub" {
                std::fs::rename(&sub, &park).unwrap();
                std::os::unix::fs::symlink(&out, &sub).unwrap();
            }
        });
        not_shared(ensure(&f.account, "claude", &f.shared));
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 1);
        assert_eq!(read(&outside.path().join("keep")), "k");
        assert_eq!(read(&parked.join("a.jsonl")), "A");
        assert!(is_real_dir(&entry));
    }

    /// SEC-20: a migration stopped after some files moved finishes on the next start; every
    /// file ends up in exactly one place.
    #[test]
    fn partial_migration_finishes_next_start() {
        let f = fx("claude");
        let entry = f.account.join("projects");
        write(&entry.join("-p/a.jsonl"), "A");
        write(&entry.join("-p/b.jsonl"), "B");
        private_shared(&f);
        let dir = f.shared.join("-p");
        let made = std::rc::Rc::new(std::cell::RefCell::new(None::<PathBuf>));
        let (d, m) = (dir.clone(), made.clone());
        let mut moves = 0;
        set_seam(move |step| {
            if let Some(name) = step.strip_prefix("move:") {
                moves += 1;
                if moves == 2 {
                    std::fs::write(d.join(name), "other").unwrap();
                    *m.borrow_mut() = Some(d.join(name));
                }
            }
        });
        not_shared(ensure(&f.account, "claude", &f.shared));
        let blocker = made.borrow_mut().take().unwrap();
        std::fs::remove_file(&blocker).unwrap();
        assert_eq!(ensure(&f.account, "claude", &f.shared), Shared::Linked);
        assert_eq!(read(&dir.join("a.jsonl")), "A");
        assert_eq!(read(&dir.join("b.jsonl")), "B");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        assert_eq!(std::fs::read_link(&entry).unwrap(), canon(&f.shared));
    }

    /// T-26: a path part of the shared folder swapped for a link between resolving it and
    /// opening it (or before the link is made) is refused; nothing moves to the other place.
    #[test]
    fn swapped_shared_folder_is_refused() {
        for when in ["shared", "link"] {
            let f = fx("claude");
            let entry = f.account.join("projects");
            write(&entry.join("-p/a.jsonl"), "A");
            private_shared(&f);
            let outside = tempfile::tempdir().unwrap();
            let elsewhere = outside.path().join("projects");
            std::fs::create_dir(&elsewhere).unwrap();
            chmod(&elsewhere, 0o700);
            let parent = f.shared.parent().unwrap().to_path_buf();
            let parked = f.d.path().join("parked");
            let (out, park) = (outside.path().to_path_buf(), parked.clone());
            set_seam(move |step| {
                if step.strip_suffix(":") == Some(when) {
                    std::fs::rename(&parent, &park).unwrap();
                    std::os::unix::fs::symlink(&out, &parent).unwrap();
                }
            });
            let why = not_shared(ensure(&f.account, "claude", &f.shared));
            assert!(why.contains("changed meanwhile"), "{when}: {why}");
            assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0, "{when}");
            assert!(is_real_dir(&entry), "{when}");
            let in_account = entry.join("-p/a.jsonl").exists();
            let in_shared = parked.join("projects/-p/a.jsonl").exists();
            assert!(in_account ^ in_shared, "{when}: exactly one place");
        }
    }

    /// SEC-20 / T-25: a file written to the shared side between the check and the move is never
    /// replaced; the account keeps its own.
    #[test]
    fn destination_created_meanwhile_is_kept() {
        let f = fx("claude");
        let entry = f.account.join("projects");
        write(&entry.join("-p/a.jsonl"), "mine");
        private_shared(&f);
        let dest = f.shared.join("-p/a.jsonl");
        let d = dest.clone();
        set_seam(move |step| {
            if step == "move:a.jsonl" {
                std::fs::write(&d, "theirs").unwrap();
            }
        });
        not_shared(ensure(&f.account, "claude", &f.shared));
        assert_eq!(read(&dest), "theirs");
        assert_eq!(read(&entry.join("-p/a.jsonl")), "mine");
        assert!(is_real_dir(&entry));
        // The next start finds the clash in the check pass and still moves nothing.
        not_shared(ensure(&f.account, "claude", &f.shared));
        assert_eq!(read(&entry.join("-p/a.jsonl")), "mine");
    }

    /// SEC-20 / T-27: too many names or levels → not shared, nothing moved.
    #[test]
    fn bounds_stop_before_moving() {
        let f = fx("claude");
        let entry = f.account.join("projects");
        for i in 0..5 {
            write(&entry.join(format!("-p/{i}.jsonl")), "x");
        }
        let why = not_shared(ensure_bounded(&f.account, "claude", &f.shared, 3, 32));
        assert!(why.contains("too large"), "{why}");
        assert_eq!(std::fs::read_dir(entry.join("-p")).unwrap().count(), 5);
        let g = fx("claude");
        write(&g.account.join("projects/a/b/c/d/e.jsonl"), "x");
        let why = not_shared(ensure_bounded(&g.account, "claude", &g.shared, 100, 3));
        assert!(why.contains("too deep"), "{why}");
        assert_eq!(read(&g.account.join("projects/a/b/c/d/e.jsonl")), "x");
        assert_eq!(
            ensure_bounded(&g.account, "claude", &g.shared, 100, 4),
            Shared::Linked
        );
    }

    /// SEC-38: inherited variables that point into `<state>/accounts/` are neither used for the
    /// shared folder nor passed on; others are.
    #[test]
    fn inherited_vars_into_accounts_are_ignored() {
        let f = fx("claude");
        let home = f.d.path().join("home");
        let inside = f.account.clone();
        let sneaky = f.state.join("accounts/../accounts/work");
        for v in [inside.as_os_str(), sneaky.as_os_str()] {
            let d = dirs(&f.state, Some(&home), Some(v), Some(v));
            assert_eq!(d["claude"], home.join(".claude/projects"));
            assert_eq!(d["codex"], home.join(".codex/sessions"));
        }
        let d = dirs(&f.state, Some(&home), Some(OsStr::new("/c")), None);
        assert_eq!(d["claude"], PathBuf::from("/c/projects"));
        assert_eq!(d.len(), 2);
        assert!(dirs(&f.state, None, None, None).is_empty());
        let vars = vec![
            (OsString::from("CLAUDE_CONFIG_DIR"), inside.clone().into()),
            (OsString::from("CODEX_HOME"), OsString::from("/x")),
            (OsString::from("PATH"), inside.clone().into()),
        ];
        assert_eq!(
            stray_vars(&f.state, vars.into_iter()),
            [OsString::from("CLAUDE_CONFIG_DIR")]
        );
        let vars = vec![(OsString::from("CODEX_HOME"), sneaky.into())];
        assert_eq!(stray_vars(&f.state, vars.into_iter()).len(), 1);
    }

    /// SEC-38: one startup warning per stray variable, naming it and saying it is ignored.
    #[test]
    fn stray_variables_give_one_startup_warning_each() {
        let f = fx("claude");
        let inside = f.state.join("accounts/claude/work");
        let vars = vec![
            (OsString::from("CLAUDE_CONFIG_DIR"), inside.into()),
            (
                OsString::from("CODEX_HOME"),
                OsString::from("/home/u/.codex"),
            ),
        ];
        let w = stray_warnings(&f.state, vars.into_iter());
        assert_eq!(w.len(), 1);
        let text = w[0].to_string();
        assert!(text.contains("CLAUDE_CONFIG_DIR"), "{text}");
        assert!(text.contains("ignored"), "{text}");
        assert!(stray_warnings(&f.state, std::iter::empty()).is_empty());
    }

    /// SEC-10: the shared folders are excluded from AI change recording.
    #[test]
    fn shared_dirs_are_excluded_from_change_recording() {
        let f = fx("claude");
        let home = f.d.path().join("home");
        let mut panel = AiPanel::new(30);
        panel.changes_excluded.push(f.state.clone());
        configure(
            &mut panel,
            &f.state,
            Some(&home),
            None,
            Some(OsStr::new("/cx")),
        );
        assert_eq!(panel.history_dirs["claude"], home.join(".claude/projects"));
        assert_eq!(panel.history_dirs["codex"], PathBuf::from("/cx/sessions"));
        assert_eq!(
            panel.changes_excluded,
            [
                f.state.clone(),
                home.join(".claude/projects"),
                PathBuf::from("/cx/sessions")
            ]
        );
    }
}
