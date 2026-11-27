//! Accounts per agent session (D11): one config folder per account,
//! `<state_dir>/accounts/<name>/` (0700), handed to the tool through its own variable
//! (`CLAUDE_CONFIG_DIR`, `CODEX_HOME`) in that session's environment only. The tool writes its
//! login there; vr-ide never opens, lists or copies anything inside an account folder, except
//! the one history entry [`super::history`] links to the tool's shared history (SEC-19, the
//! amended SEC-16).
//!
//! Every name goes through [`validate`] (SEC-15); every folder is checked by [`secure`] before
//! it is used (SEC-16); removal is [`remove`] (SEC-17). Which tool an account belongs to (ui-spec
//! Q1) is kept beside the folders in `<state_dir>/ai-accounts`, one `name tool` line each.

use std::collections::BTreeMap;
use std::ffi::{CStr, OsString};
use std::io::Read;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::path::{Path, PathBuf};

/// The folder of the account folders, under the state folder.
pub const ACCOUNTS_DIR: &str = "accounts";
/// The file recording which tool each account was added under.
pub const INDEX_FILE: &str = "ai-accounts";
/// Serialises changes to the index across windows.
pub const INDEX_LOCK: &str = "ai-accounts.lock";
/// The reserved name: the tool's own config, no variable.
pub const DEFAULT: &str = "default";
/// Longest account name, in characters.
pub const NAME_MAX: usize = 32;
/// The index is a few names; anything larger is not ours and is ignored.
const INDEX_MAX: u64 = 64 * 1024;

/// The variable that moves `tool`'s config folder. The map is fixed here: a tool without an
/// entry cannot take an account, and no setting can name another variable (SEC-18).
pub fn var_for(tool: &str) -> Option<&'static str> {
    match tool {
        "claude" => Some("CLAUDE_CONFIG_DIR"),
        "codex" => Some("CODEX_HOME"),
        _ => None,
    }
}

/// Why a name is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameError {
    Empty,
    TooLong,
    Chars,
    Reserved,
}

impl NameError {
    /// Fixed text: the refused name itself is never echoed.
    pub fn message(self) -> &'static str {
        match self {
            NameError::Empty => "type a name",
            NameError::TooLong => "at most 32 characters",
            NameError::Chars => "use a-z 0-9 - _ only",
            NameError::Reserved => "default is reserved",
        }
    }
}

/// The one validator for account names (SEC-15): `[a-z0-9][a-z0-9_-]{0,31}`, and not `default`
/// in any case. A valid name has no `.`, `/`, `\`, space, control or non-ASCII character, so it
/// is always one plain path component.
pub fn validate(name: &str) -> Result<&str, NameError> {
    if name.eq_ignore_ascii_case(DEFAULT) {
        return Err(NameError::Reserved);
    }
    let b = name.as_bytes();
    let Some(&first) = b.first() else {
        return Err(NameError::Empty);
    };
    let plain = |c: u8| c.is_ascii_lowercase() || c.is_ascii_digit();
    if !plain(first) || !b.iter().all(|&c| plain(c) || c == b'-' || c == b'_') {
        return Err(NameError::Chars);
    }
    if b.len() > NAME_MAX {
        return Err(NameError::TooLong);
    }
    Ok(name)
}

/// `<state>/accounts`.
pub fn accounts_dir(state: &Path) -> PathBuf {
    state.join(ACCOUNTS_DIR)
}

/// `<state>/accounts/<name>` for a valid name; the only way an account path is made.
pub fn path_of(state: &Path, name: &str) -> Result<PathBuf, NameError> {
    validate(name).map(|n| accounts_dir(state).join(n))
}

/// The account folders: entries of `accounts/` whose name is valid and that are real folders
/// (a symlink or a file is skipped), sorted. Nothing inside them is looked at.
pub fn folders(state: &Path) -> Vec<String> {
    let dir = accounts_dir(state);
    if !is_real_dir(&dir) {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            validate(&name).ok()?;
            is_real_dir(&dir.join(&name)).then_some(name)
        })
        .collect();
    out.sort();
    out
}

/// The accounts added under `tool`, sorted.
pub fn for_tool(state: &Path, tool: &str) -> Vec<String> {
    let index = read_index(state);
    folders(state)
        .into_iter()
        .filter(|n| index.get(n).is_some_and(|t| t == tool))
        .collect()
}

fn is_real_dir(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_dir())
}

/// `name → tool` for display and checks; an unreadable index lists nothing.
fn read_index(state: &Path) -> BTreeMap<String, String> {
    read_index_strict(state).unwrap_or_default()
}

/// `name → tool` from the index: a missing file is empty, an unreadable one (not UTF-8, too
/// large, not a file) is an error so that nobody rewrites it from a partial view. Lines with an
/// invalid name or tool are dropped.
fn read_index_strict(state: &Path) -> std::io::Result<BTreeMap<String, String>> {
    use rustix::fs::{Mode, OFlags};
    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK;
    let fd = match rustix::fs::open(state.join(INDEX_FILE), flags, Mode::empty()) {
        Err(rustix::io::Errno::NOENT) => return Ok(BTreeMap::new()),
        r => r?,
    };
    let file = std::fs::File::from(fd);
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::other("not a file"));
    }
    let mut text = String::new();
    file.take(INDEX_MAX + 1).read_to_string(&mut text)?;
    if text.len() as u64 > INDEX_MAX {
        return Err(std::io::Error::other("too large"));
    }
    Ok(text
        .lines()
        .filter_map(|line| {
            let (name, tool) = line.split_once(' ')?;
            validate(name).ok()?;
            var_for(tool)?;
            Some((name.to_string(), tool.to_string()))
        })
        .collect())
}

/// Held while the index is read, changed and written: windows adding or removing accounts at
/// the same time do not lose each other's records.
struct IndexLock(OwnedFd);

impl Drop for IndexLock {
    fn drop(&mut self) {
        unlock(&self.0);
    }
}

/// Lets go of a `flock` before its descriptor closes. The lock belongs to the open file, which a
/// child forked meanwhile by any thread shares until it execs; closing only our descriptor
/// would leave the lock held through that copy (a closed session's account then reads "in use").
pub(crate) fn unlock(fd: &OwnedFd) {
    let _ = rustix::fs::flock(fd, rustix::fs::FlockOperation::Unlock);
}

fn lock_index(state: &Path) -> Result<IndexLock, String> {
    use rustix::fs::{FlockOperation, Mode, OFlags};
    std::fs::create_dir_all(state).map_err(|e| format!("cannot create folder ({e})"))?;
    let flags = OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let fd = rustix::fs::open(state.join(INDEX_LOCK), flags, Mode::RUSR | Mode::WUSR)
        .map_err(|e| format!("cannot lock the account record ({e})"))?;
    rustix::fs::flock(&fd, FlockOperation::LockExclusive)
        .map_err(|e| format!("cannot lock the account record ({e})"))?;
    Ok(IndexLock(fd))
}

/// Writes the index (0600) through a temp file renamed over it. Called under [`IndexLock`], so
/// any `.ai-accounts.*.tmp` left in the state folder is from a write that was killed: those, and
/// only those, are removed first.
fn write_index(
    state: &Path,
    _lock: &IndexLock,
    index: &BTreeMap<String, String>,
) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let prefix = format!(".{INDEX_FILE}.");
    if let Ok(entries) = std::fs::read_dir(state) {
        for e in entries.filter_map(Result::ok) {
            let name = e.file_name();
            let name = name.to_string_lossy();
            let ours = name.starts_with(&prefix) && name.ends_with(".tmp");
            if ours && e.file_type().is_ok_and(|t| !t.is_dir()) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let text: String = index.iter().map(|(n, t)| format!("{n} {t}\n")).collect();
    let tmp = state.join(format!("{prefix}{}.tmp", std::process::id()));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    let written = file
        .write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .and_then(|()| std::fs::rename(&tmp, state.join(INDEX_FILE)));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written
}

/// Opens `accounts/` itself (never through a symlink) for work relative to it.
fn open_accounts(state: &Path) -> Result<OwnedFd, String> {
    use rustix::fs::{Mode, OFlags};
    use rustix::io::Errno;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    rustix::fs::open(accounts_dir(state), flags, Mode::empty()).map_err(|e| match e {
        Errno::NOENT => "account folder missing".to_string(),
        Errno::LOOP | Errno::NOTDIR => "account folder is not a real folder".to_string(),
        e => format!("account folder unusable ({e})"),
    })
}

/// `accounts/.<name>.lock`, beside the account folder (never inside it).
fn open_account_lock(dir: &OwnedFd, name: &str) -> Result<OwnedFd, String> {
    use rustix::fs::{Mode, OFlags};
    let flags = OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    rustix::fs::openat(dir, format!(".{name}.lock"), flags, Mode::RUSR | Mode::WUSR)
        .map_err(|e| format!("cannot lock the account ({e})"))
}

/// A session's hold on its account (D17): a shared lock on `accounts/.<name>.lock` for as long as
/// the session lives, in any vr-ide window; [`remove`] needs it exclusively.
#[derive(Debug)]
pub struct AccountLock(OwnedFd);

impl Drop for AccountLock {
    fn drop(&mut self) {
        unlock(&self.0);
    }
}

#[cfg(test)]
thread_local! {
    /// Tests: runs in [`hold`] between opening the lock file and locking it.
    static BEFORE_HOLD_LOCK: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        const { std::cell::RefCell::new(None) };
}

/// Takes the hold on account `name` before its session starts. Refused while a removal has it.
pub fn hold(state: &Path, name: &str) -> Result<AccountLock, String> {
    take_hold(
        state,
        name,
        rustix::fs::FlockOperation::NonBlockingLockShared,
        "account is busy (being removed or set up)",
    )
}

/// The exclusive hold for changing the account's history entry (SEC-20): refused at once while
/// a session in any window (or a removal) holds the account.
pub fn hold_exclusive(state: &Path, name: &str) -> Result<AccountLock, String> {
    take_hold(
        state,
        name,
        rustix::fs::FlockOperation::NonBlockingLockExclusive,
        "in use",
    )
}

fn take_hold(
    state: &Path,
    name: &str,
    op: rustix::fs::FlockOperation,
    busy: &str,
) -> Result<AccountLock, String> {
    use rustix::fs::AtFlags;
    let name = validate(name).map_err(|_| "account name invalid".to_string())?;
    let dir = open_accounts(state)?;
    let missing = rustix::fs::statat(&dir, name, AtFlags::SYMLINK_NOFOLLOW).is_err();
    if missing {
        return Err("account folder missing".into());
    }
    let fd = open_account_lock(&dir, name)?;
    #[cfg(test)]
    if let Some(f) = BEFORE_HOLD_LOCK.with(|h| h.borrow_mut().take()) {
        f();
    }
    rustix::fs::flock(&fd, op).map_err(|_| busy.to_string())?;
    let lock = AccountLock(fd);
    // A removal between the open and the lock deleted the folder and unlinked this lock file:
    // the hold is on nothing, so the session must not start.
    let held = rustix::fs::fstat(&lock.0).map_err(|e| format!("cannot lock the account ({e})"))?;
    let named = rustix::fs::statat(&dir, format!(".{name}.lock"), AtFlags::SYMLINK_NOFOLLOW);
    let same = named.is_ok_and(|n| n.st_dev == held.st_dev && n.st_ino == held.st_ino);
    if !same || rustix::fs::statat(&dir, name, AtFlags::SYMLINK_NOFOLLOW).is_err() {
        return Err("account folder missing".into());
    }
    Ok(lock)
}

/// Deletes the folder `name` under `parent` without following any link: each level is opened
/// with `O_NOFOLLOW` relative to the one above, links and files are unlinked, never entered.
fn remove_tree_at(parent: BorrowedFd<'_>, name: &CStr) -> std::io::Result<()> {
    use rustix::fs::{AtFlags, FileType, Mode, OFlags};
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let fd = rustix::fs::openat(parent, name, flags, Mode::empty())?;
    let mut names = Vec::new();
    for entry in rustix::fs::Dir::read_from(&fd)? {
        let n = entry?.file_name().to_owned();
        if n.as_bytes() != b"." && n.as_bytes() != b".." {
            names.push(n);
        }
    }
    for n in names {
        let st = rustix::fs::statat(&fd, n.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)?;
        if FileType::from_raw_mode(st.st_mode) == FileType::Directory {
            remove_tree_at(fd.as_fd(), &n)?;
        } else {
            rustix::fs::unlinkat(&fd, n.as_c_str(), AtFlags::empty())?;
        }
    }
    rustix::fs::unlinkat(parent, name, AtFlags::REMOVEDIR)?;
    Ok(())
}

/// SEC-16 before every use: `dir` must be a real folder (opened with `O_NOFOLLOW`, so a symlink
/// is refused) owned by this user; group / other bits are dropped with `fchmod` on that same
/// descriptor. Nothing inside is read.
pub fn secure(dir: &Path) -> Result<(), String> {
    use rustix::fs::{Mode, OFlags};
    use rustix::io::Errno;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let fd = rustix::fs::open(dir, flags, Mode::empty()).map_err(|e| match e {
        Errno::NOENT => "account folder missing".to_string(),
        Errno::LOOP | Errno::NOTDIR => "account folder is not a real folder".to_string(),
        e => format!("account folder unusable ({e})"),
    })?;
    let st = rustix::fs::fstat(&fd).map_err(|e| format!("account folder unusable ({e})"))?;
    if st.st_uid != rustix::process::geteuid().as_raw() {
        return Err("account folder belongs to another user".into());
    }
    if st.st_mode & 0o077 != 0 {
        rustix::fs::fchmod(&fd, Mode::RWXU)
            .map_err(|e| format!("account folder could not be made private ({e})"))?;
    }
    Ok(())
}

/// The folder `tool` may run on as `name`, checked now (SEC-15, SEC-16, SEC-18): the tool has a
/// variable, the name is valid, it was not added under another tool, `accounts/` and the folder
/// pass [`secure`]. Never created here: a missing folder is an error, not a new account.
pub fn ready(state: Option<&Path>, tool: &str, name: &str) -> Result<PathBuf, String> {
    if var_for(tool).is_none() {
        return Err(format!("{tool} cannot use accounts"));
    }
    let path = validate(name).map_err(|_| "account name invalid".to_string())?;
    let state = state.ok_or("account folder missing")?;
    let path = accounts_dir(state).join(path);
    secure(&accounts_dir(state))?;
    secure(&path)?;
    // Only the tool it was added under may use it (ui-spec Q1): one folder never serves as both
    // `CLAUDE_CONFIG_DIR` and `CODEX_HOME`.
    match read_index(state).get(name) {
        Some(t) if t == tool => Ok(path),
        Some(other) => Err(format!("account belongs to {other}")),
        None => Err("account not recorded; add it again".into()),
    }
}

/// The variable for a session of `tool` on `account`, for that child's environment only (never
/// `std::env::set_var`). `None` (default) adds nothing: the inherited environment stays.
pub fn env(
    state: Option<&Path>,
    tool: &str,
    account: Option<&str>,
) -> Result<Vec<(OsString, OsString)>, String> {
    let Some(name) = account else {
        return Ok(Vec::new());
    };
    let path = ready(state, tool, name).map_err(|e| format!("account {name}: {e}"))?;
    let var = var_for(tool).expect("checked by ready");
    Ok(vec![(OsString::from(var), path.into_os_string())])
}

/// Adds account `name` under `tool`: `accounts/` and `accounts/<name>/` made 0700 and checked,
/// the tool recorded. A name taken by any tool is refused (names are unique across tools); a
/// folder left without a record (by hand, or a failed write) is taken over after the checks.
pub fn create(state: &Path, tool: &str, name: &str) -> Result<PathBuf, String> {
    let name = validate(name).map_err(|e| e.message().to_string())?;
    if var_for(tool).is_none() {
        return Err(format!("{tool} cannot use accounts"));
    }
    let lock = lock_index(state)?;
    let mut index =
        read_index_strict(state).map_err(|e| format!("cannot read the account record ({e})"))?;
    match index.get(name) {
        Some(other) if other != tool => return Err(format!("{name} is taken ({other})")),
        Some(_) if is_real_dir(&accounts_dir(state).join(name)) => {
            return Err(format!("{name} exists"));
        }
        _ => {}
    }
    let dir = accounts_dir(state);
    let path = dir.join(name);
    // A symlink in either place is refused by `secure` before anything is created through it.
    if std::fs::symlink_metadata(&dir).is_ok() {
        secure(&dir)?;
    }
    crate::recovery::create_private_dir(&dir).map_err(|e| format!("cannot create folder ({e})"))?;
    secure(&dir)?;
    if std::fs::symlink_metadata(&path).is_ok() {
        secure(&path)?;
    }
    crate::recovery::create_private_dir(&path)
        .map_err(|e| format!("cannot create folder ({e})"))?;
    secure(&path)?;
    index.insert(name.to_string(), tool.to_string());
    write_index(state, &lock, &index).map_err(|e| format!("cannot record the account ({e})"))?;
    Ok(path)
}

/// Deletes account `name` for good (SEC-17): the name is validated again; `accounts/` is opened
/// without following a link and everything happens relative to that descriptor, so a link
/// swapped in cannot redirect the delete; a symlinked account is unlinked (its target
/// untouched), a folder is removed level by level without entering links. No trash: the token
/// files must not survive elsewhere. Nothing outside `accounts/<name>` (and its lock file) is
/// touched. Refused while a session holds the account in any window (D17); the caller has asked
/// and checked its own sessions.
pub fn remove(state: &Path, name: &str) -> Result<(), String> {
    use rustix::fs::{AtFlags, FileType, FlockOperation};
    let name = validate(name).map_err(|e| e.message().to_string())?;
    let index_lock = lock_index(state)?;
    let mut index =
        read_index_strict(state).map_err(|e| format!("cannot read the account record ({e})"))?;
    let dir = open_accounts(state)?;
    let lock = open_account_lock(&dir, name)?;
    rustix::fs::flock(&lock, FlockOperation::NonBlockingLockExclusive)
        .map_err(|_| "in use in another window".to_string())?;
    let lock = AccountLock(lock);
    let removed: std::io::Result<()> =
        match rustix::fs::statat(&dir, name, AtFlags::SYMLINK_NOFOLLOW) {
            Err(rustix::io::Errno::NOENT) => Ok(()),
            Err(e) => Err(e.into()),
            Ok(st) => match FileType::from_raw_mode(st.st_mode) {
                FileType::Directory => {
                    let c = std::ffi::CString::new(name).expect("validated: no NUL");
                    remove_tree_at(dir.as_fd(), &c)
                }
                FileType::Symlink => {
                    rustix::fs::unlinkat(&dir, name, AtFlags::empty()).map_err(Into::into)
                }
                _ => return Err("not an account folder".into()),
            },
        };
    removed.map_err(|e| format!("cannot remove the folder ({e})"))?;
    let _ = rustix::fs::unlinkat(&dir, format!(".{name}.lock"), AtFlags::empty());
    drop(lock);
    if index.remove(name).is_some() {
        write_index(state, &index_lock, &index)
            .map_err(|e| format!("cannot update the record ({e})"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    fn mode(p: &Path) -> u32 {
        std::fs::symlink_metadata(p).unwrap().mode() & 0o777
    }

    /// SEC-15 table: every refused name creates nothing.
    #[test]
    fn names_are_validated_by_one_rule() {
        let d = tempfile::tempdir().unwrap();
        let long = "a".repeat(33);
        for bad in [
            "../x",
            "/etc",
            "a/b",
            ".",
            "..",
            "A B",
            "a\x1b",
            "é",
            "Default",
            "DEFAULT",
            "default",
            "",
            "-a",
            "_a",
            "a.b",
            "a\\b",
            "a\0b",
            "Work",
            " work",
            "a\u{202e}b",
            &long,
        ] {
            assert!(validate(bad).is_err(), "{bad:?}");
            assert!(path_of(d.path(), bad).is_err(), "{bad:?}");
            assert!(create(d.path(), "claude", bad).is_err(), "{bad:?}");
            assert!(remove(d.path(), bad).is_err(), "{bad:?}");
            assert!(ready(Some(d.path()), "claude", bad).is_err(), "{bad:?}");
        }
        assert_eq!(
            std::fs::read_dir(d.path()).unwrap().count(),
            0,
            "nothing created"
        );
        for good in ["work", "alt-2", "a", "0_x", &"a".repeat(32)] {
            assert_eq!(validate(good), Ok(good));
        }
        assert_eq!(validate("Default"), Err(NameError::Reserved));
        assert_eq!(validate(&long), Err(NameError::TooLong));
        assert_eq!(
            path_of(d.path(), "work").unwrap(),
            d.path().join("accounts/work")
        );
    }

    #[test]
    fn variables_are_a_fixed_map() {
        assert_eq!(var_for("claude"), Some("CLAUDE_CONFIG_DIR"));
        assert_eq!(var_for("codex"), Some("CODEX_HOME"));
        for t in ["gemini", "aider", "cursor-agent", "LD_PRELOAD", "PATH", ""] {
            assert_eq!(var_for(t), None);
        }
        assert_eq!(env(None, "claude", None).unwrap(), vec![]);
        assert_eq!(env(None, "gemini", None).unwrap(), vec![]);
        assert!(
            env(None, "gemini", Some("work"))
                .unwrap_err()
                .contains("gemini cannot use accounts")
        );
    }

    /// SEC-16: created 0700; a wider pre-made folder is made 0700 before use; a symlink in
    /// place of the folder (or of `accounts/`) is refused and its target gets nothing.
    #[test]
    fn folders_are_private_real_and_checked_before_use() {
        let d = tempfile::tempdir().unwrap();
        let state = d.path();
        let p = create(state, "claude", "work").unwrap();
        assert_eq!(p, state.join("accounts/work"));
        assert_eq!(mode(&state.join("accounts")), 0o700);
        assert_eq!(mode(&p), 0o700);
        let env = env(Some(state), "claude", Some("work")).unwrap();
        assert_eq!(
            env,
            vec![(
                OsString::from("CLAUDE_CONFIG_DIR"),
                p.clone().into_os_string()
            )]
        );
        // Pre-made wide folders.
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(
            state.join("accounts"),
            std::fs::Permissions::from_mode(0o777),
        )
        .unwrap();
        ready(Some(state), "claude", "work").unwrap();
        assert_eq!(mode(&p), 0o700);
        assert_eq!(mode(&state.join("accounts")), 0o700);
        // A symlink as the account folder.
        let outside = tempfile::tempdir().unwrap();
        std::fs::set_permissions(outside.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink(outside.path(), state.join("accounts/alt")).unwrap();
        assert_eq!(
            ready(Some(state), "claude", "alt").unwrap_err(),
            "account folder is not a real folder"
        );
        assert!(create(state, "claude", "alt").is_err());
        assert_eq!(mode(outside.path()), 0o755, "target untouched");
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
        assert_eq!(folders(state), ["work"], "the link is not listed");
        // A symlink as `accounts/`.
        let state2 = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), state2.path().join("accounts")).unwrap();
        assert!(create(state2.path(), "claude", "work").is_err());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
        assert!(folders(state2.path()).is_empty());
        // Missing.
        assert_eq!(
            ready(Some(state), "claude", "gone").unwrap_err(),
            "account folder missing"
        );
        assert_eq!(
            ready(None, "claude", "work").unwrap_err(),
            "account folder missing"
        );
    }

    /// The listing keeps valid real folders only; accounts belong to the tool they were added
    /// under and names are unique across tools (ui-spec Q1).
    #[test]
    fn listing_and_tools() {
        let d = tempfile::tempdir().unwrap();
        let state = d.path();
        create(state, "claude", "work").unwrap();
        create(state, "codex", "cx").unwrap();
        assert_eq!(
            create(state, "codex", "work").unwrap_err(),
            "work is taken (claude)"
        );
        assert_eq!(create(state, "claude", "work").unwrap_err(), "work exists");
        assert!(
            create(state, "gemini", "g")
                .unwrap_err()
                .contains("cannot use")
        );
        let acc = state.join("accounts");
        std::fs::write(acc.join("file"), "x").unwrap();
        std::fs::create_dir(acc.join("Bad")).unwrap();
        std::fs::create_dir(acc.join(".hidden")).unwrap();
        std::os::unix::fs::symlink(acc.join("work"), acc.join("link")).unwrap();
        assert_eq!(folders(state), ["cx", "work"]);
        assert_eq!(for_tool(state, "claude"), ["work"]);
        assert_eq!(for_tool(state, "codex"), ["cx"]);
        assert_eq!(
            ready(Some(state), "codex", "work").unwrap_err(),
            "account belongs to claude"
        );
        // A tampered index line cannot name another variable or a bad name.
        std::fs::write(
            state.join(INDEX_FILE),
            "work claude\n../x claude\nevil LD_PRELOAD\ncx codex\n",
        )
        .unwrap();
        assert_eq!(read_index(state).len(), 2);
        // A folder without a record is taken over when added again.
        std::fs::create_dir(acc.join("orphan")).unwrap();
        assert!(for_tool(state, "claude").iter().all(|n| n != "orphan"));
        create(state, "claude", "orphan").unwrap();
        assert_eq!(for_tool(state, "claude"), ["orphan", "work"]);
    }

    /// SEC-17: only `accounts/<name>` goes, links inside are not followed, a linked account
    /// loses only the link, `accounts/` and siblings stay.
    #[test]
    fn remove_deletes_only_the_account() {
        let d = tempfile::tempdir().unwrap();
        let state = d.path();
        let work = create(state, "claude", "work").unwrap();
        create(state, "claude", "alt").unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("keep"), "x").unwrap();
        std::os::unix::fs::symlink(outside.path(), work.join("link")).unwrap();
        std::fs::create_dir(work.join("sub")).unwrap();
        std::fs::write(work.join("sub/.credentials.json"), "t").unwrap();
        remove(state, "work").unwrap();
        assert!(!work.exists());
        assert!(outside.path().join("keep").exists(), "outside intact");
        assert!(state.join("accounts/alt").is_dir(), "sibling intact");
        assert!(state.join("accounts").is_dir(), "accounts/ stays");
        assert_eq!(for_tool(state, "claude"), ["alt"]);
        // A symlinked account: the link goes, its target stays.
        std::os::unix::fs::symlink(outside.path(), state.join("accounts/lnk")).unwrap();
        remove(state, "lnk").unwrap();
        assert!(std::fs::symlink_metadata(state.join("accounts/lnk")).is_err());
        assert!(outside.path().join("keep").exists());
        // Crafted names.
        for bad in ["..", ".", "../alt", "default", ""] {
            assert!(remove(state, bad).is_err(), "{bad:?}");
        }
        assert!(state.join("accounts/alt").is_dir());
    }
}

#[cfg(test)]
mod fix_tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    /// DS Q1 / SEC-18: a folder with no record for this tool is not used.
    #[test]
    fn ready_needs_the_record_of_this_tool() {
        let d = tempfile::tempdir().unwrap();
        let state = d.path();
        create(state, "claude", "work").unwrap();
        std::fs::create_dir(state.join("accounts/orphan")).unwrap();
        assert_eq!(
            ready(Some(state), "codex", "orphan").unwrap_err(),
            "account not recorded; add it again"
        );
        assert_eq!(
            ready(Some(state), "codex", "work").unwrap_err(),
            "account belongs to claude"
        );
        assert!(ready(Some(state), "claude", "work").is_ok());
    }

    /// An unreadable record is never rewritten (the other accounts would be lost); the record is
    /// 0600; leftover temp files of a killed write are cleaned up; the lock file exists.
    #[test]
    fn record_is_private_locked_and_never_rewritten_from_a_failed_read() {
        let d = tempfile::tempdir().unwrap();
        let state = d.path();
        create(state, "claude", "work").unwrap();
        let index = state.join(INDEX_FILE);
        assert_eq!(std::fs::metadata(&index).unwrap().mode() & 0o777, 0o600);
        assert!(state.join(INDEX_LOCK).exists());
        // Unreadable: a folder in its place / bytes that are not UTF-8.
        std::fs::write(&index, b"work claude\n\xff\xfe\n").unwrap();
        assert!(
            create(state, "claude", "alt")
                .unwrap_err()
                .contains("cannot read the account record")
        );
        assert_eq!(std::fs::read(&index).unwrap(), b"work claude\n\xff\xfe\n");
        assert!(
            remove(state, "work")
                .unwrap_err()
                .contains("cannot read the account record")
        );
        std::fs::write(&index, "work claude\n").unwrap();
        // Leftovers of a killed write (only that pattern goes).
        std::fs::write(state.join(".ai-accounts.123-0.tmp"), "x").unwrap();
        std::fs::write(state.join(".recent.123-0.tmp"), "x").unwrap();
        std::fs::set_permissions(&index, std::fs::Permissions::from_mode(0o644)).unwrap();
        create(state, "claude", "alt").unwrap();
        assert!(!state.join(".ai-accounts.123-0.tmp").exists());
        assert!(state.join(".recent.123-0.tmp").exists());
        assert_eq!(std::fs::metadata(&index).unwrap().mode() & 0o777, 0o600);
        assert_eq!(for_tool(state, "claude"), ["alt", "work"]);
    }

    /// Concurrent adds from several windows keep every record.
    #[test]
    fn concurrent_adds_keep_every_record() {
        let d = tempfile::tempdir().unwrap();
        let state = d.path().to_path_buf();
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let state = state.clone();
                std::thread::spawn(move || create(&state, "claude", &format!("a{i}")).unwrap())
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(for_tool(&state, "claude").len(), 8);
    }

    /// D17: a session in any window holds the account; remove is refused until it lets go.
    #[test]
    fn remove_is_refused_while_another_window_holds_the_account() {
        let d = tempfile::tempdir().unwrap();
        let state = d.path();
        let work = create(state, "claude", "work").unwrap();
        let held = hold(state, "work").unwrap();
        let second = hold(state, "work").unwrap();
        assert_eq!(
            remove(state, "work").unwrap_err(),
            "in use in another window"
        );
        assert!(work.is_dir());
        drop(held);
        assert!(remove(state, "work").is_err(), "still one holder");
        drop(second);
        remove(state, "work").unwrap();
        assert!(!work.exists());
        assert!(!state.join("accounts/.work.lock").exists());
        assert!(hold(state, "work").is_err(), "nothing to hold");
    }

    /// D17: a closed session lets go of its account at once, even while a child forked before
    /// that (another session's agent, say) has not started its program yet.
    #[test]
    fn a_dropped_hold_frees_the_account_despite_a_forked_child() {
        let d = tempfile::tempdir().unwrap();
        let state = d.path();
        let work = create(state, "claude", "work").unwrap();
        let held = hold(state, "work").unwrap();
        let child =
            crate::session_tests::child_paused_before_exec(std::time::Duration::from_millis(400));
        drop(held);
        remove(state, "work").unwrap();
        assert!(!work.exists());
        child.join().unwrap();
    }

    /// SEC-20: while another window sets up the account's history (exclusive hold), a session
    /// start is refused with that reason, not as a removal.
    #[test]
    fn a_hold_during_set_up_says_busy() {
        let d = tempfile::tempdir().unwrap();
        let state = d.path();
        create(state, "claude", "work").unwrap();
        let setting_up = hold_exclusive(state, "work").unwrap();
        assert_eq!(
            hold(state, "work").unwrap_err(),
            "account is busy (being removed or set up)"
        );
        assert_eq!(hold_exclusive(state, "work").unwrap_err(), "in use");
        drop(setting_up);
        hold(state, "work").unwrap();
    }

    /// D17: a removal that runs between `hold` opening the lock file and locking it wins: the
    /// hold is refused, so no session starts on the removed account.
    #[test]
    fn a_hold_racing_a_removal_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let state = d.path().to_path_buf();
        let work = create(&state, "claude", "work").unwrap();
        let s = state.clone();
        BEFORE_HOLD_LOCK.with(|h| {
            *h.borrow_mut() = Some(Box::new(move || remove(&s, "work").unwrap()));
        });
        assert_eq!(hold(&state, "work").unwrap_err(), "account folder missing");
        assert!(!work.exists());
        assert!(!state.join("accounts/.work.lock").exists());
    }

    /// SEC-17: the delete walks from the `accounts/` descriptor; a symlink swapped in for
    /// `accounts/` is never followed.
    #[test]
    fn remove_never_follows_a_swapped_accounts_folder() {
        let d = tempfile::tempdir().unwrap();
        let state = d.path();
        create(state, "claude", "work").unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir(outside.path().join("work")).unwrap();
        std::fs::write(outside.path().join("work/keep"), "x").unwrap();
        std::fs::rename(state.join("accounts"), state.join("moved")).unwrap();
        std::os::unix::fs::symlink(outside.path(), state.join("accounts")).unwrap();
        assert!(remove(state, "work").is_err());
        assert!(outside.path().join("work/keep").exists());
        // Deep trees with links, read-only files and dot names go entirely.
        std::fs::remove_file(state.join("accounts")).unwrap();
        std::fs::rename(state.join("moved"), state.join("accounts")).unwrap();
        let work = state.join("accounts/work");
        std::fs::create_dir_all(work.join("a/b/.c")).unwrap();
        std::fs::write(work.join("a/b/.c/t"), "t").unwrap();
        std::fs::write(work.join(".hidden"), "t").unwrap();
        std::fs::set_permissions(work.join(".hidden"), std::fs::Permissions::from_mode(0o400))
            .unwrap();
        std::os::unix::fs::symlink(outside.path(), work.join("a/out")).unwrap();
        remove(state, "work").unwrap();
        assert!(!work.exists());
        assert!(outside.path().join("work/keep").exists());
    }
}
