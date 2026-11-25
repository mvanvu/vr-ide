//! Recovery copies taken before a discard (threat model SEC-11): owner-only, fsynced, outside
//! every repository; a failed copy stops the discard.

use std::ffi::{OsStr, OsString};
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::hash::{BuildHasher, Hasher, RandomState};
use std::io::{Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::cmd::{Git, Profile};
use crate::{GitError, Result};

/// `<state dir>/git-recovery` from the runner's environment (the rules of vr-config's
/// `state_dir`: `VR_IDE_STATE_DIR`, `XDG_STATE_HOME/vr-ide`, `~/.local/state/vr-ide`). Only a
/// fallback: vr-config reads the process environment (and the passwd entry), which a library
/// with an injected environment and hermetic tests cannot use, and vr-git stays free of the
/// config crate. The app passes `Options::recovery_dir` from vr-config so one rule applies.
pub(crate) fn default_dir(var: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let set = |k: &str| var(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let state = if let Some(d) = set("VR_IDE_STATE_DIR") {
        d
    } else if let Some(x) = set("XDG_STATE_HOME").filter(|p| p.is_absolute()) {
        x.join("vr-ide")
    } else {
        set("HOME")?.join(".local/state/vr-ide")
    };
    Some(state.join("git-recovery"))
}

/// The recovery dir, created owner-only; refused when it is not a plain directory owned by
/// this user. Inside a git work tree (a dotfiles repository at `$HOME`, say) a copy could be
/// committed, so the folder is used only when that repository tracks nothing in it and, once
/// a `.gitignore` of `*` is at its root, `git check-ignore` confirms it ignores it (D305(3)).
/// No folder is created before the first of those checks passes, and what this call created
/// is removed again when the second fails.
pub(crate) fn prepare_dir(dir: &Path, git: &Git) -> Result<PathBuf> {
    if !dir.is_absolute() {
        return Err(GitError::Refused(format!(
            "recovery folder {} is not an absolute path; {WAY_OUT}",
            dir.display()
        )));
    }
    let existing = dir
        .ancestors()
        .find(|a| a.exists())
        .unwrap_or(Path::new("/"));
    let base = existing
        .canonicalize()
        .map_err(|e| io_err("resolve", existing, e))?;
    let target = match dir.strip_prefix(existing) {
        Ok(rest) => base.join(rest),
        Err(_) => dir.to_path_buf(),
    };
    let work_tree = base
        .ancestors()
        .find(|a| a.join(".git").exists())
        .map(Path::to_path_buf);
    if let Some(wt) = &work_tree {
        let refuse = |why: &str| refuse_in_repo(&target, wt, why);
        let out = git
            .run(
                &base,
                &[
                    OsStr::new("ls-files"),
                    OsStr::new("-z"),
                    OsStr::new("--"),
                    target.as_os_str(),
                ],
                None,
                git.auto(),
            )
            .map_err(|e| refuse(&e.to_string()))?;
        if out.code != 0 {
            return Err(refuse(out.stderr.lines().next().unwrap_or("git failed")));
        }
        if !out.stdout.is_empty() {
            return Err(refuse("that repository tracks files in it"));
        }
    }

    let mut created = Vec::new();
    let made = create_owned(&target, &mut created).and_then(|()| {
        if let Some(wt) = &work_tree {
            ignore_in_repo(&target, wt, git, &mut created)?;
        }
        Ok(())
    });
    if let Err(e) = made {
        // Undo only what this call made; everything it made is empty but the `.gitignore`.
        for p in created.iter().rev() {
            let _ = if p.ends_with(".gitignore") {
                fs::remove_file(p)
            } else {
                fs::remove_dir(p)
            };
        }
        return Err(e);
    }
    target
        .canonicalize()
        .map_err(|e| io_err("resolve", &target, e))
}

const WAY_OUT: &str = "set VR_IDE_STATE_DIR (or the recovery_dir option) to a folder outside \
                       any repository; nothing was discarded";

fn refuse_in_repo(dir: &Path, wt: &Path, why: &str) -> GitError {
    GitError::Refused(format!(
        "recovery folder {} is inside the git work tree {} and cannot be kept out of it ({}); \
         {WAY_OUT}",
        dir.display(),
        wt.display(),
        crate::sanitize::sanitize_line(why)
    ))
}

/// Create the missing folders down to `dir` (the last one owner-only), then check that `dir`
/// is a plain folder owned by this user and narrow its mode to the owner.
fn create_owned(dir: &Path, created: &mut Vec<PathBuf>) -> Result<()> {
    let missing: Vec<&Path> = dir.ancestors().take_while(|a| !a.exists()).collect();
    for p in missing.into_iter().rev() {
        let mode = if p == dir { 0o700 } else { 0o777 };
        match DirBuilder::new().mode(mode).create(p) {
            Ok(()) => created.push(p.to_path_buf()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(io_err("create", p, e)),
        }
    }
    let meta = fs::symlink_metadata(dir).map_err(|e| io_err("stat", dir, e))?;
    if !meta.is_dir() || meta.uid() != rustix::process::geteuid().as_raw() {
        return Err(GitError::Refused(format!(
            "recovery folder {} is not a folder owned by you; nothing was discarded",
            dir.display()
        )));
    }
    // Only narrow: group/other access goes, the owner keeps what it has.
    let mode = meta.permissions().mode() & 0o777;
    if mode & 0o077 != 0 {
        fs::set_permissions(dir, fs::Permissions::from_mode(mode & 0o700))
            .map_err(|e| io_err("chmod", dir, e))?;
    }
    Ok(())
}

/// Put `*` in `dir/.gitignore` (kept when it exists) and ask git whether the repository at
/// `wt` now ignores what goes into `dir`.
fn ignore_in_repo(dir: &Path, wt: &Path, git: &Git, created: &mut Vec<PathBuf>) -> Result<()> {
    let gi = dir.join(".gitignore");
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(&gi)
    {
        Ok(mut f) => {
            created.push(gi.clone());
            f.write_all(b"*\n")
                .and_then(|()| f.sync_all())
                .map_err(|e| io_err("write", &gi, e))?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(io_err("create", &gi, e)),
    }
    // check-ignore refuses the `literal` pathspec magic every read forces, so it runs under
    // the commit profile (no forced literal pathspecs; check-ignore starts no hook, filter or
    // other program) with a plain name relative to its cwd, which has no magic to misread.
    let out = git
        .run(
            dir,
            &[
                OsStr::new("check-ignore"),
                OsStr::new("-q"),
                OsStr::new("--"),
                OsStr::new("20260101T000000.000000000Z-repo-file"),
            ],
            None,
            git.auto().with(Profile::Commit),
        )
        .map_err(|e| refuse_in_repo(dir, wt, &e.to_string()))?;
    if out.code != 0 {
        return Err(refuse_in_repo(
            dir,
            wt,
            "that repository does not ignore it",
        ));
    }
    Ok(())
}

/// `rename(2)` that fails with `AlreadyExists` instead of replacing `to`.
pub(crate) fn rename_noreplace(from: &Path, to: &Path) -> std::io::Result<()> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        from,
        rustix::fs::CWD,
        to,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(std::io::Error::from)
}

/// Points where tests inject a fault into a discard (review M7-2 round 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fault {
    #[cfg_attr(not(test), allow(dead_code))]
    None,
    /// Panic after the target left its path and before its copy is complete.
    Window,
    /// Fail the removal of a moved target after its cross-filesystem copy was verified.
    Remove,
}

#[cfg(test)]
thread_local! {
    pub(crate) static FAULT: std::cell::Cell<Fault> = const { std::cell::Cell::new(Fault::None) };
}

#[cfg(test)]
pub(crate) fn fault(at: Fault) -> bool {
    FAULT.with(|f| f.get() == at)
}

#[cfg(not(test))]
#[inline(always)]
pub(crate) fn fault(_: Fault) -> bool {
    false
}

/// How [`copy_across`] failed.
pub(crate) enum KeepError {
    /// No copy exists and the moved target is whole and still in place.
    NotKept(GitError),
    /// The copy is complete, but removing the moved target failed part way: `cause` is the
    /// removal error; what is left of the target is still at the moved path.
    Partial { copy: PathBuf, cause: String },
}

/// True when `a` and `b` are on one filesystem (so a rename between them can work).
pub(crate) fn same_filesystem(a: &Path, b: &Path) -> bool {
    match (fs::metadata(a), fs::metadata(b)) {
        (Ok(x), Ok(y)) => x.dev() == y.dev(),
        _ => false,
    }
}

/// A mount point inside the target: its content is another filesystem, which a rename-aside
/// keeps but a removal after the copy would reach into.
fn crosses_mount(parent: &fs::Metadata, child: &fs::Metadata) -> bool {
    parent.dev() != child.dev()
}

/// In a sticky-bit folder (like `/tmp`) only the entry's owner, the folder's owner or root
/// may remove an entry.
fn sticky_blocks(dir_mode: u32, dir_uid: u32, entry_uid: u32, me: u32) -> bool {
    dir_mode & 0o1000 != 0 && me != 0 && me != dir_uid && me != entry_uid
}

/// Immutable or append-only inode flags (`chattr +i` / `+a`) make a removal fail even for the
/// owner. Not-checkable entries (symlinks, special files, filesystems without the ioctl) pass.
fn is_immutable(path: &Path, meta: &fs::Metadata) -> bool {
    if !(meta.is_file() || meta.is_dir()) {
        return false;
    }
    let Ok(f) = OpenOptions::new()
        .read(true)
        .custom_flags(
            rustix::fs::OFlags::NOFOLLOW.bits() as i32 | rustix::fs::OFlags::NONBLOCK.bits() as i32,
        )
        .open(path)
    else {
        return false;
    };
    rustix::fs::ioctl_getflags(&f)
        .is_ok_and(|fl| fl.intersects(rustix::fs::IFlags::IMMUTABLE | rustix::fs::IFlags::APPEND))
}

/// Refuse a folder discard that could only be done part way: across filesystems the folder
/// is removed entry by entry after its copy, and an entry that cannot be removed (a folder
/// this user cannot write to, a mount point inside, an entry in someone else's sticky folder,
/// an immutable or append-only file) would stop that half done. Checked before anything moves.
pub(crate) fn check_removable(full: &Path, rel: &Path) -> Result<()> {
    let me = rustix::process::geteuid().as_raw();
    let refuse = |why: &str, at: &Path| {
        GitError::Refused(format!(
            "{} cannot be removed because {why} ({}); fix it or delete it in the terminal; \
             nothing was discarded",
            rel.display(),
            at.display()
        ))
    };
    let top = fs::symlink_metadata(full).map_err(|e| io_err("stat", full, e))?;
    if !top.is_dir() {
        return Ok(());
    }
    let mut stack = vec![(PathBuf::new(), top)];
    while let Some((sub, meta)) = stack.pop() {
        // `join` of an empty path would add a trailing slash, which fails on a file.
        let d = if sub.as_os_str().is_empty() {
            full.to_path_buf()
        } else {
            full.join(&sub)
        };
        let shown = rel.join(&sub);
        let writable = rustix::fs::access(
            &d,
            rustix::fs::Access::WRITE_OK | rustix::fs::Access::EXEC_OK,
        )
        .is_ok();
        if !writable {
            return Err(refuse("its folder is read-only", &shown));
        }
        if is_immutable(&d, &meta) {
            return Err(refuse("it is immutable or append-only", &shown));
        }
        for e in fs::read_dir(&d).map_err(|e| io_err("list", &d, e))? {
            let e = e.map_err(|e| io_err("list", &d, e))?;
            let child =
                fs::symlink_metadata(e.path()).map_err(|er| io_err("stat", &e.path(), er))?;
            let at = shown.join(e.file_name());
            if sticky_blocks(meta.mode(), meta.uid(), child.uid(), me) {
                return Err(refuse(
                    "it is in a sticky folder owned by someone else",
                    &at,
                ));
            }
            if child.is_dir() {
                if crosses_mount(&meta, &child) {
                    return Err(refuse("a mount point is inside", &at));
                }
                stack.push((sub.join(e.file_name()), child));
            } else if is_immutable(&e.path(), &child) {
                return Err(refuse("it is immutable or append-only", &at));
            }
        }
    }
    Ok(())
}

fn copy_names(repo: &Path, rel: &Path, dir: &Path) -> impl Iterator<Item = PathBuf> {
    let base = copy_name(repo, rel);
    let dir = dir.to_path_buf();
    (0..100).map(move |n| {
        dir.join(if n == 0 {
            base.clone()
        } else {
            format!("{base}.{n}")
        })
    })
}

/// Rename `from` (the target itself, or its aside) straight into the recovery folder as
/// `<utc>-<repo>-<sanitized rel>`: one atomic step, so a crash leaves it where copies are
/// documented to be, and a program still writing through an open descriptor writes into the
/// copy. `None` when the folder is on another filesystem (EXDEV; nothing moved). Once the
/// rename succeeded the result is `Some`: a failing fsync of the folder is only logged.
pub(crate) fn move_into(
    dir: &Path,
    repo: &Path,
    rel: &Path,
    from: &Path,
) -> Result<Option<PathBuf>> {
    for target in copy_names(repo, rel, dir) {
        match rename_noreplace(from, &target) {
            Ok(()) => {
                if let Err(e) = sync_dir(dir) {
                    tracing::warn!("recovery copy {} kept, but {e}", target.display());
                }
                return Ok(Some(target));
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) if e.raw_os_error() == Some(rustix::io::Errno::XDEV.raw_os_error()) => {
                return Ok(None);
            }
            Err(e) => return Err(io_err("move to the recovery folder", from, e)),
        }
    }
    Err(GitError::Refused(
        "could not name a recovery copy; nothing was discarded".into(),
    ))
}

/// Make a kept copy private like a written one (the folder is owner-only anyway). Called only
/// once the copy is checked, since the check compares the mode too.
pub(crate) fn seal(copy: &Path) {
    if fs::symlink_metadata(copy).is_ok_and(|m| m.is_file()) {
        let _ = fs::set_permissions(copy, fs::Permissions::from_mode(0o600));
    }
}

/// Keep `moved` (a target renamed aside in the work tree) in the recovery folder: a rename
/// when possible, else [`copy_across`]. The kept path, or the error of the copy.
pub(crate) fn keep(
    dir: &Path,
    repo: &Path,
    rel: &Path,
    moved: &Path,
    cap: u64,
) -> std::result::Result<PathBuf, KeepError> {
    match move_into(dir, repo, rel, moved) {
        Ok(Some(c)) => {
            seal(&c);
            Ok(c)
        }
        Ok(None) => copy_across(dir, repo, rel, moved, cap),
        Err(e) => Err(KeepError::NotKept(e)),
    }
}

/// Keep `moved` in a recovery folder on another filesystem: the bytes are copied and fsynced,
/// read again to verify nothing changed meanwhile, and only then is `moved` removed. Before
/// the copy every folder in `moved` must be removable ([`check_removable`]).
pub(crate) fn copy_across(
    dir: &Path,
    repo: &Path,
    rel: &Path,
    moved: &Path,
    cap: u64,
) -> std::result::Result<PathBuf, KeepError> {
    use KeepError::NotKept;
    check_removable(moved, rel).map_err(NotKept)?;
    let mut verify_failures = 0;
    for target in copy_names(repo, rel, dir) {
        let snap = snapshot(moved, cap).map_err(NotKept)?;
        match write_new(&target, &snap) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                remove_any(&target);
                return Err(NotKept(io_err("write recovery copy", &target, e)));
            }
        }
        if let Err(e) = sync_dir(dir) {
            remove_any(&target);
            return Err(NotKept(e));
        }
        if snapshot(moved, cap).ok().as_ref() != Some(&snap) {
            // Written to while copying: copy again.
            remove_any(&target);
            verify_failures += 1;
            if verify_failures == 3 {
                return Err(NotKept(GitError::Refused(format!(
                    "{} keeps changing; nothing was discarded",
                    rel.display()
                ))));
            }
            continue;
        }
        let removed = if fault(Fault::Remove) {
            Err(std::io::Error::other("injected fault"))
        } else {
            match &snap {
                Snapshot::Dir(_) => fs::remove_dir_all(moved),
                _ => fs::remove_file(moved),
            }
        };
        return match removed {
            Ok(()) => Ok(target),
            Err(e) => Err(KeepError::Partial {
                copy: target,
                cause: format!("remove {}: {e}", moved.display()),
            }),
        };
    }
    Err(NotKept(GitError::Refused(
        "could not name a recovery copy; nothing was discarded".into(),
    )))
}

fn remove_any(p: &Path) {
    let _ = match fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => fs::remove_dir_all(p),
        _ => fs::remove_file(p),
    };
}

fn copy_name(repo: &Path, rel: &Path) -> String {
    let repo_name = repo
        .file_name()
        .map(|n| sanitize(n.as_bytes()))
        .unwrap_or_else(|| "repo".into());
    format!(
        "{}-{}-{}",
        utc_stamp(SystemTime::now()),
        repo_name,
        sanitize(rel.as_os_str().as_bytes())
    )
}

/// What a target looks like on disk, read without following a symlink.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Snapshot {
    Absent,
    File {
        bytes: Vec<u8>,
        mode: u32,
    },
    Link(PathBuf),
    /// Files under an untracked directory, relative to it, in walk order.
    Dir(Vec<(PathBuf, Snapshot)>),
}

/// Read `path` (bounded by `cap` bytes in total).
pub(crate) fn snapshot(path: &Path, cap: u64) -> Result<Snapshot> {
    let mut budget = cap;
    snap(path, &mut budget)
}

fn snap(path: &Path, budget: &mut u64) -> Result<Snapshot> {
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Snapshot::Absent),
        Err(e) => return Err(io_err("stat", path, e)),
    };
    let too_big = || {
        GitError::Refused(format!(
            "{} is too large for a recovery copy; nothing was discarded",
            path.display()
        ))
    };
    if meta.file_type().is_symlink() {
        return Ok(Snapshot::Link(
            fs::read_link(path).map_err(|e| io_err("read", path, e))?,
        ));
    }
    if meta.is_file() {
        if meta.len() > *budget {
            return Err(too_big());
        }
        let mut bytes = Vec::with_capacity(meta.len() as usize);
        File::open(path)
            .and_then(|f| f.take(*budget + 1).read_to_end(&mut bytes))
            .map_err(|e| io_err("read", path, e))?;
        if bytes.len() as u64 > *budget {
            return Err(too_big());
        }
        *budget -= bytes.len() as u64;
        return Ok(Snapshot::File {
            bytes,
            mode: meta.permissions().mode() & 0o777,
        });
    }
    if meta.is_dir() {
        let mut names: Vec<PathBuf> = fs::read_dir(path)
            .map_err(|e| io_err("list", path, e))?
            .map(|e| e.map(|e| PathBuf::from(e.file_name())))
            .collect::<std::io::Result<_>>()
            .map_err(|e| io_err("list", path, e))?;
        names.sort();
        let mut items = Vec::new();
        for n in names {
            if n.as_os_str() == ".git" {
                return Err(GitError::Refused(format!(
                    "{} contains a git repository; discard it in the terminal",
                    path.display()
                )));
            }
            items.push((n.clone(), snap(&path.join(&n), budget)?));
        }
        return Ok(Snapshot::Dir(items));
    }
    Err(GitError::Refused(format!(
        "{} is not a file, folder or symlink",
        path.display()
    )))
}

/// A keyed hash of a snapshot: equal at confirmation and execution, or the discard stops.
pub(crate) fn digest(keys: &RandomState, s: &Snapshot) -> u64 {
    let mut h = keys.build_hasher();
    feed(&mut h, s);
    h.finish()
}

fn feed(h: &mut impl Hasher, s: &Snapshot) {
    match s {
        Snapshot::Absent => h.write_u8(0),
        Snapshot::File { bytes, mode } => {
            h.write_u8(1);
            h.write_u32(*mode);
            h.write_usize(bytes.len());
            h.write(bytes);
        }
        Snapshot::Link(t) => {
            h.write_u8(2);
            h.write(t.as_os_str().as_bytes());
            h.write_u8(0);
        }
        Snapshot::Dir(items) => {
            h.write_u8(3);
            h.write_usize(items.len());
            for (n, s) in items {
                h.write(n.as_os_str().as_bytes());
                h.write_u8(0);
                feed(h, s);
            }
        }
    }
}

fn write_new(target: &Path, snap: &Snapshot) -> std::io::Result<()> {
    match snap {
        Snapshot::Absent => Ok(()),
        Snapshot::File { bytes, .. } => {
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
                .open(target)?;
            // The umask can only narrow the mode; make it exactly owner read/write.
            f.set_permissions(fs::Permissions::from_mode(0o600))?;
            f.write_all(bytes)?;
            f.sync_all()
        }
        // Copied as a link, never followed (SEC-11).
        Snapshot::Link(t) => std::os::unix::fs::symlink(t, target),
        Snapshot::Dir(items) => {
            DirBuilder::new().mode(0o700).create(target)?;
            for (n, s) in items {
                write_new(&target.join(n), s)?;
            }
            sync_dir(target).map_err(|e| std::io::Error::other(e.to_string()))
        }
    }
}

fn sync_dir(dir: &Path) -> Result<()> {
    File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(|e| io_err("sync", dir, e))
}

/// Bytes outside `[A-Za-z0-9._-]` become `%XX`; long names are cut (the timestamp and a
/// numeric suffix keep them unique).
fn sanitize(b: &[u8]) -> String {
    let mut s = String::new();
    for &c in b {
        if c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-') {
            s.push(c as char);
        } else {
            s.push_str(&format!("%{c:02X}"));
        }
        if s.len() >= 160 {
            break;
        }
    }
    // A name made only of dots would be "." or "..".
    if s.bytes().all(|c| c == b'.') {
        s = s.replace('.', "%2E");
    }
    s
}

/// `20261125T120102.123456789Z`
fn utc_stamp(t: SystemTime) -> String {
    let d = t.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}.{:09}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        d.subsec_nanos()
    )
}

fn io_err(what: &str, path: &Path, e: std::io::Error) -> GitError {
    GitError::Io(format!("{what} {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn stamp_is_utc_calendar_time() {
        let t = UNIX_EPOCH + Duration::new(1_764_072_062, 5);
        assert_eq!(utc_stamp(t), "20251125T120102.000000005Z");
        assert_eq!(utc_stamp(UNIX_EPOCH), "19700101T000000.000000000Z");
    }

    #[test]
    fn names_are_sanitized() {
        assert_eq!(sanitize(b"src/a b.rs"), "src%2Fa%20b.rs");
        assert_eq!(sanitize(b".."), "%2E%2E");
        assert_eq!(sanitize(b"\xff\n"), "%FF%0A");
    }

    #[test]
    fn default_dir_follows_the_state_dir_rules() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| OsString::from(v))
            }
        };
        assert_eq!(
            default_dir(env(&[("VR_IDE_STATE_DIR", "/s"), ("HOME", "/h")])),
            Some(PathBuf::from("/s/git-recovery"))
        );
        assert_eq!(
            default_dir(env(&[("XDG_STATE_HOME", "/x"), ("HOME", "/h")])),
            Some(PathBuf::from("/x/vr-ide/git-recovery"))
        );
        assert_eq!(
            default_dir(env(&[("XDG_STATE_HOME", "rel"), ("HOME", "/h")])),
            Some(PathBuf::from("/h/.local/state/vr-ide/git-recovery"))
        );
        assert_eq!(default_dir(env(&[])), None);
    }
}

#[cfg(test)]
mod removable_tests {
    use super::*;

    #[test]
    fn sticky_folder_blocks_only_strangers() {
        // (mode, dir owner, entry owner, me)
        assert!(sticky_blocks(0o41777, 0, 5, 7));
        assert!(!sticky_blocks(0o41777, 7, 5, 7), "folder owner");
        assert!(!sticky_blocks(0o41777, 0, 7, 7), "entry owner");
        assert!(!sticky_blocks(0o41777, 0, 5, 0), "root");
        assert!(!sticky_blocks(0o40755, 0, 5, 7), "not sticky");
    }

    #[test]
    fn a_different_device_is_a_mount_point() {
        let dir = std::env::temp_dir();
        let a = fs::metadata(&dir).unwrap();
        assert!(!crosses_mount(&a, &a));
        // /proc is its own filesystem on Linux.
        let root = fs::metadata("/").unwrap();
        if let Ok(p) = fs::metadata("/proc")
            && p.dev() != root.dev()
        {
            assert!(crosses_mount(&root, &p));
        }
    }

    #[test]
    fn a_plain_tree_passes_and_a_read_only_folder_is_refused() {
        let root = std::env::temp_dir().join(format!("vr-removable-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("sub/f"), "x").unwrap();
        assert!(check_removable(&root, Path::new("t")).is_ok());
        fs::set_permissions(root.join("sub"), fs::Permissions::from_mode(0o555)).unwrap();
        let r = check_removable(&root, Path::new("t"));
        fs::set_permissions(root.join("sub"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::remove_dir_all(&root).unwrap();
        if rustix::process::geteuid().as_raw() != 0 {
            assert!(matches!(r, Err(GitError::Refused(_))));
        }
    }
}
