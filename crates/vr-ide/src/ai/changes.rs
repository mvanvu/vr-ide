//! AI changes (D8): what changed in an agent session's folder while its program ran, with the
//! content from before the first change (the pre-image) where vr-ide truly has it.
//!
//! inotify reports a write after it happened, so a pre-image can never be read on an event
//! (SEC-9). Instead, when a session starts a worker thread walks the folder once (bounded by
//! [`Limits`]): it puts one flat watch on every directory and copies every regular file of the
//! directories the ignore rules keep into a private store. A copy counts as the before-content
//! only when the file's ctime and mtime predate the session's start (the kernel sets ctime on
//! every write and a program cannot set it back), read through one handle whose stamp did not
//! move during the read. A file that fails the rule, lies in an ignored directory, is too large
//! or comes after a cap is still listed when it changes, but without an earlier version: Keep
//! only. Later captures (after Keep, after a save of vr-ide's own editor) hold the content as it
//! is at that moment and serve changes that come after it.
//!
//! The store is `state_dir()/ai/<generated>/` (0700, files 0600, SEC-10): blobs under generated
//! names, a lock file held while the session lives so another vr-ide's start-up sweep leaves it
//! alone, the bytes in use for the other running vr-ides' total, and an index of the records
//! (root-relative paths). Nothing reads the index back today: a store is deleted when its
//! session closes and swept after a crash, so a restored session starts with an empty list;
//! [`load_index`] is the reader a later restore must use, and it trusts no line of it.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, File, Metadata, OpenOptions};
use std::hash::Hasher;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rustix::fs::OFlags;
use vr_core::safe_replace::{self, Checks, Outcome, Step};
use vr_core::{FsEvent, OwnWrites, Watcher};

/// SEC-10 caps: one pre-image, one session's store, every session's stores together, records
/// of one session.
pub const FILE_CAP: u64 = 8 << 20;
pub const AGENT_CAP: u64 = 256 << 20;
pub const TOTAL_CAP: u64 = 512 << 20;
pub const RECORD_CAP: usize = 5_000;
/// Directories one session watches (one inotify watch each) and files its walk remembers: the
/// bound of the start-up cost in a huge folder such as `~/`.
pub const DIR_CAP: usize = 10_000;
pub const KNOWN_CAP: usize = 200_000;

/// A timestamp this close to "now" may hide a second write in the same clock tick.
const RACY_WINDOW: Duration = Duration::from_secs(2);
/// How far before the session's start a file's times must lie to count as untouched by it: the
/// kernel stamps files from a clock that lags by up to a tick; a file system that keeps whole
/// seconds gets the racy window.
const FINE_MARGIN: Duration = Duration::from_millis(100);
/// Entries the walk handles between two looks at the watcher's events.
const SCAN_STEP: usize = 64;
/// Lines of one diff tab; a longer diff is cut with a note.
const DIFF_LINES_MAX: usize = 20_000;
const DIFF_TIMEOUT: Duration = Duration::from_millis(300);
const INDEX_NAME: &str = "index";
const INDEX_HEAD: &str = "vr-ide-ai-changes 1";
const LOCK_NAME: &str = "lock";
/// A store's bytes in use, for the total of the vr-ides running side by side.
const USED_NAME: &str = "used";
/// How old the sum of the other vr-ides' stores may be, and how far a store's own figure may
/// lag: the total can be passed by that much, never without bound.
const OTHERS_TTL: Duration = Duration::from_secs(1);
const USED_LAG: u64 = 1 << 20;
/// In the name a replaced version has from the Revert until it is in the trash:
/// `<name>.vr-ide-reverted-PID-N`, the name it is found under there.
const REVERTED_MARK: &str = ".vr-ide-reverted-";

#[derive(Debug, Clone)]
pub struct Limits {
    pub file: u64,
    pub agent: u64,
    pub total: u64,
    pub records: usize,
    pub dirs: usize,
    pub known: usize,
    /// `None`: by the file's timestamp granularity ([`FINE_MARGIN`] or [`RACY_WINDOW`]).
    pub margin: Option<Duration>,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            file: FILE_CAP,
            agent: AGENT_CAP,
            total: TOTAL_CAP,
            records: RECORD_CAP,
            dirs: DIR_CAP,
            known: KNOWN_CAP,
            margin: None,
        }
    }
}

/// What a tracker needs from the app.
pub struct Config {
    /// The session's folder.
    pub root: PathBuf,
    /// `state_dir()/ai`.
    pub store_base: PathBuf,
    /// Bytes of every session's store, shared by all trackers of this vr-ide.
    pub total: Arc<AtomicU64>,
    pub limits: Limits,
    /// Folders never recorded: the state dir, the config dir, the trash.
    pub excluded: Vec<PathBuf>,
    /// Saves of vr-ide's own editor.
    pub own: Option<OwnWrites>,
    /// Set whenever the list changed (the event loop's wake flag).
    pub wake: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Modified,
    Created,
    Deleted,
}

impl Kind {
    pub fn letter(self) -> &'static str {
        match self {
            Kind::Modified => "M",
            Kind::Created => "A",
            Kind::Deleted => "D",
        }
    }
}

/// Why a changed file has no earlier version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoEarlier {
    /// vr-ide first saw the file after the change (SEC-9).
    NotSeen,
    /// Over [`Limits::file`].
    TooLarge,
    /// The session's or the total store cap was reached.
    Limit,
}

impl NoEarlier {
    /// The list's counts column.
    pub fn short(self) -> &'static str {
        match self {
            NoEarlier::NotSeen => "no earlier",
            NoEarlier::TooLarge => "too large",
            NoEarlier::Limit => "limit",
        }
    }

    /// The diff tab's header and the status bar.
    pub fn long(self) -> &'static str {
        match self {
            NoEarlier::NotSeen => "changed, no earlier version kept",
            NoEarlier::TooLarge => "too large, no earlier version kept",
            NoEarlier::Limit => "limit reached, no earlier version kept",
        }
    }
}

/// A stored pre-image: a generated file name, never derived from a workspace path.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Blob {
    id: u64,
    len: u64,
    hash: u64,
    /// Permission bits of the file it was read from.
    mode: u32,
}

/// One changed file of a session.
#[derive(Debug, Clone)]
pub struct Record {
    /// Root-relative.
    pub rel: PathBuf,
    pub kind: Kind,
    /// Set when there is no pre-image to show or write back.
    pub missing: Option<NoEarlier>,
    /// When vr-ide first saw the change.
    pub since: SystemTime,
    /// Lines added / removed against the pre-image; `None` while not counted (binary, no
    /// earlier version).
    pub added: Option<u32>,
    pub removed: Option<u32>,
    pre: Option<Blob>,
    recount: bool,
}

impl Record {
    /// Revert can act: a created file goes to the trash, the others need their pre-image.
    pub fn can_revert(&self) -> bool {
        self.kind == Kind::Created || self.pre.is_some()
    }
}

/// Why the list may miss changes (SEC-13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Incomplete {
    Watcher,
    Rescan,
    WatchLimit,
    RecordLimit,
    /// The walk's memory is full: folders past that point are not watched.
    WalkLimit,
    /// The start-up walk has not been everywhere yet.
    Scanning,
    /// The program exited (or was started again) before the walk had been everywhere: what
    /// it changed in the folders not reached was never seen.
    Unwalked,
    /// Entries of a folder changed between the session's start and the moment the walk put
    /// its watch there: a file deleted in that time was never seen.
    WalkedLate,
}

impl Incomplete {
    pub fn text(self) -> &'static str {
        match self {
            Incomplete::Watcher => "watcher error",
            Incomplete::Rescan => "watcher rescan",
            Incomplete::WatchLimit => "watch limit",
            Incomplete::RecordLimit => "file limit",
            Incomplete::WalkLimit => "too many files",
            Incomplete::Scanning => "still scanning the folder",
            Incomplete::Unwalked => "scan stopped early",
            Incomplete::WalkedLate => "deletions during the scan",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Banner {
    pub why: Incomplete,
    pub at: SystemTime,
}

// ---- stamps and safe reads --------------------------------------------------------------------

/// Cheap identity of a file's state (not followed through links).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stamp {
    len: u64,
    mtime: (i64, i64),
    ctime: (i64, i64),
    dev: u64,
    ino: u64,
    mode: u32,
}

impl Stamp {
    fn of(m: &Metadata) -> Stamp {
        Stamp {
            len: m.len(),
            mtime: (m.mtime(), m.mtime_nsec()),
            ctime: (m.ctime(), m.ctime_nsec()),
            dev: m.dev(),
            ino: m.ino(),
            mode: m.mode(),
        }
    }

    /// The later of mtime and ctime.
    fn newest(&self) -> SystemTime {
        let t = self.mtime.max(self.ctime);
        UNIX_EPOCH + Duration::new(t.0.max(0) as u64, t.1.clamp(0, 999_999_999) as u32)
    }

    /// Taken now, this stamp could survive another write (same tick, same length).
    fn racy(&self) -> bool {
        let newest = self.newest();
        !SystemTime::now()
            .duration_since(newest)
            .is_ok_and(|age| age >= RACY_WINDOW)
    }

    /// The file was last written or replaced before `t0`, clock lag allowed for.
    fn predates(&self, t0: SystemTime, margin: Option<Duration>) -> bool {
        let fine = self.mtime.1 != 0 || self.ctime.1 != 0;
        let margin = margin.unwrap_or(if fine { FINE_MARGIN } else { RACY_WINDOW });
        t0.checked_sub(margin)
            .is_some_and(|limit| self.newest() < limit)
    }
}

fn content_hash(bytes: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    h.write(bytes);
    h.finish()
}

enum ReadOut {
    Bytes(Stamp, Vec<u8>),
    TooLarge,
    /// A link, FIFO, socket, device or directory: never opened for reading.
    NotRegular,
    /// Written while it was read.
    Unstable,
}

/// A handle on a regular file that follows no link and cannot block (SEC-11): the path is
/// checked before the open and the opened file again, so a FIFO or link put there in between
/// is refused too.
fn open_regular(path: &Path) -> io::Result<Option<(File, Metadata)>> {
    if !fs::symlink_metadata(path)?.is_file() {
        return Ok(None);
    }
    let flags = OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(flags.bits() as i32)
        .open(path)
    {
        Ok(f) => f,
        // ELOOP: a link took the file's place after the check.
        Err(e) if e.raw_os_error() == Some(rustix::io::Errno::LOOP.raw_os_error()) => {
            return Ok(None);
        }
        Err(e) => return Err(e),
    };
    let meta = file.metadata()?;
    Ok(meta.is_file().then_some((file, meta)))
}

/// At most `cap` bytes of a regular file with the stamp they belong to.
fn read_regular(path: &Path, cap: u64) -> io::Result<ReadOut> {
    let Some((file, meta)) = open_regular(path)? else {
        return Ok(ReadOut::NotRegular);
    };
    let before = Stamp::of(&meta);
    if before.len > cap {
        return Ok(ReadOut::TooLarge);
    }
    let mut bytes = Vec::with_capacity(before.len as usize);
    (&file).take(cap + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > cap {
        return Ok(ReadOut::TooLarge);
    }
    if Stamp::of(&file.metadata()?) != before {
        return Ok(ReadOut::Unstable);
    }
    Ok(ReadOut::Bytes(before, bytes))
}

/// What is at a path, as the user was shown it: the expectation a Revert must still find
/// (SEC-12). Links are described, never followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileState {
    stamp: Stamp,
    /// Content of a regular file, target of a link, 0 for anything else.
    hash: u64,
    racy: bool,
}

/// The version of a file a diff showed; opaque.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seen(Option<FileState>);

impl Seen {
    fn is_folder(&self) -> bool {
        self.0.is_some_and(|s| s.stamp.mode & 0o170000 == 0o040000)
    }

    /// The diff showed a file (or a link whose target is) exactly `bytes`.
    #[cfg(test)]
    pub fn shows(&self, bytes: &[u8]) -> bool {
        self.0
            .is_some_and(|s| s.stamp.len == bytes.len() as u64 && s.hash == content_hash(bytes))
    }
}

fn hash_stream(mut file: &File) -> io::Result<u64> {
    let mut h = DefaultHasher::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match file.read(&mut buf) {
            Ok(0) => return Ok(h.finish()),
            Ok(n) => h.write(&buf[..n]),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
}

/// The state at `path` now, read until it held still.
fn state_of(path: &Path) -> io::Result<Option<FileState>> {
    for _ in 0..5 {
        let meta = match fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let stamp = Stamp::of(&meta);
        let racy = stamp.racy();
        // Replaced or removed between the look and the read: looked at again.
        let gone = |e: &io::Error| e.kind() == io::ErrorKind::NotFound;
        let hash = if meta.is_file() {
            match open_regular(path) {
                Ok(Some((file, _))) => hash_stream(&file)?,
                Ok(None) => continue,
                Err(e) if gone(&e) => continue,
                Err(e) => return Err(e),
            }
        } else if meta.file_type().is_symlink() {
            match fs::read_link(path) {
                Ok(to) => content_hash(to.as_os_str().as_bytes()),
                // EINVAL: no longer a link.
                Err(e) if gone(&e) || e.kind() == io::ErrorKind::InvalidInput => continue,
                Err(e) => return Err(e),
            }
        } else {
            0
        };
        let again = match fs::symlink_metadata(path) {
            Ok(m) => Stamp::of(&m),
            Err(_) => continue,
        };
        if again == stamp {
            return Ok(Some(FileState { stamp, hash, racy }));
        }
    }
    Err(io::Error::other(
        "the file keeps changing on disk; try again",
    ))
}

/// `path` still holds what `seen` describes. A different stamp with the same content (chmod
/// aside) is the same version; an equal racy stamp proves nothing, so the content decides.
fn still(path: &Path, seen: &Seen) -> io::Result<bool> {
    let cur = match fs::symlink_metadata(path) {
        Ok(m) => Some(m),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    match (&seen.0, cur) {
        (None, None) => Ok(true),
        (Some(s), Some(m)) => {
            let now = Stamp::of(&m);
            if now == s.stamp && !s.racy {
                return Ok(true);
            }
            if now.len != s.stamp.len || now.mode != s.stamp.mode {
                return Ok(false);
            }
            Ok(state_of(path)?.is_some_and(|c| c.hash == s.hash))
        }
        _ => Ok(false),
    }
}

/// The file swapped out of the path holds what `seen` describes; the swap changed its ctime,
/// so only kind, length and content can tell.
fn holds(path: &Path, seen: &Seen) -> io::Result<bool> {
    let Some(s) = &seen.0 else {
        return Ok(false);
    };
    let m = fs::symlink_metadata(path)?;
    let now = Stamp::of(&m);
    if now.len != s.stamp.len || now.mode != s.stamp.mode {
        return Ok(false);
    }
    Ok(state_of(path)?.is_some_and(|c| c.hash == s.hash))
}

// ---- state ------------------------------------------------------------------------------------

/// What the walk knows of a file that has no record.
#[derive(Debug, Clone)]
struct Base {
    stamp: Stamp,
    racy: bool,
    blob: Option<Blob>,
    why: Option<NoEarlier>,
}

struct State {
    /// Canonical.
    root: PathBuf,
    /// The store folder of this session.
    dir: PathBuf,
    _lock: File,
    limits: Limits,
    /// `state_dir()/ai`: the stores of every vr-ide of this user.
    base: PathBuf,
    total: Arc<AtomicU64>,
    used: u64,
    /// `used` as last written to the store for the other vr-ides.
    flushed: Option<u64>,
    /// Bytes the other vr-ides' stores hold, and when that was read.
    others: Option<(Instant, u64)>,
    next_blob: u64,
    excluded: Vec<PathBuf>,
    own: Option<OwnWrites>,
    /// Start of the program's current run.
    t0: SystemTime,
    running: bool,
    closed: bool,
    /// Bumped by every start, pause and resume: a walk of an older run stops.
    epoch: u64,
    scanning: bool,
    known: HashMap<PathBuf, Base>,
    /// Directories whose entries were all listed (root-relative).
    walked: HashSet<PathBuf>,
    /// Directories watched (root-relative).
    dirs: HashSet<PathBuf>,
    records: Vec<Record>,
    banner: Option<Banner>,
    index_dirty: bool,
    /// Files (root-relative) the editor is saving or has saved, until the save's event shows
    /// the file to be that save, or something else.
    saving: HashSet<PathBuf>,
}

struct Shared {
    state: Mutex<State>,
    version: AtomicU64,
    wake: Arc<AtomicBool>,
    stop: AtomicBool,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn bump(&self) {
        self.version.fetch_add(1, Ordering::Relaxed);
        self.wake.store(true, Ordering::Relaxed);
    }
}

impl State {
    fn blob_path(&self, id: u64) -> PathBuf {
        self.dir.join(format!("{id:016x}.blob"))
    }

    /// Room for a pre-image of `len` bytes, or why not (existing ones are never evicted).
    fn reserve(&mut self, len: u64) -> Result<u64, NoEarlier> {
        if len > self.limits.file {
            return Err(NoEarlier::TooLarge);
        }
        if self.used + len > self.limits.agent {
            return Err(NoEarlier::Limit);
        }
        // This vr-ide's sessions are counted exactly, the other running ones by what their
        // stores say.
        let (cap, others) = (self.limits.total, self.others_used());
        self.total
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |t| {
                (t.saturating_add(len).saturating_add(others) <= cap).then_some(t + len)
            })
            .map_err(|_| NoEarlier::Limit)?;
        self.used += len;
        self.next_blob += 1;
        if self
            .flushed
            .is_none_or(|f| self.used.abs_diff(f) >= USED_LAG)
        {
            self.flush_used();
        }
        Ok(self.next_blob)
    }

    /// Bytes in the stores of the other vr-ide processes of this user (SEC-10: the total is
    /// one for all of them). A store names its figure itself; one that cannot be read as a
    /// plain small file counts as nothing.
    fn others_used(&mut self) -> u64 {
        if let Some((at, sum)) = self.others
            && at.elapsed() < OTHERS_TTL
        {
            return sum;
        }
        let ours = format!("{}-", std::process::id());
        let mut sum = 0u64;
        for e in fs::read_dir(&self.base).into_iter().flatten().flatten() {
            if e.file_name().to_string_lossy().starts_with(&ours) {
                continue;
            }
            if let Ok(ReadOut::Bytes(_, b)) = read_regular(&e.path().join(USED_NAME), 32)
                && let Some(n) = std::str::from_utf8(&b)
                    .ok()
                    .and_then(|s| s.trim().parse::<u64>().ok())
            {
                sum = sum.saturating_add(n);
            }
        }
        self.others = Some((Instant::now(), sum));
        sum
    }

    /// Writes this store's bytes in use where the other vr-ides read them.
    fn flush_used(&mut self) {
        if self.closed || self.flushed == Some(self.used) {
            return;
        }
        let tmp = self.dir.join(format!("{USED_NAME}.tmp"));
        let _ = fs::remove_file(&tmp);
        let done = crate::recovery::create_private_file(&tmp)
            .and_then(|mut f| f.write_all(format!("{}\n", self.used).as_bytes()))
            .and_then(|()| fs::rename(&tmp, self.dir.join(USED_NAME)));
        if done.is_ok() {
            self.flushed = Some(self.used);
        }
    }

    fn release(&mut self, len: u64) {
        self.used = self.used.saturating_sub(len);
        if self
            .flushed
            .is_some_and(|f| self.used.abs_diff(f) >= USED_LAG)
        {
            self.flush_used();
        }
        let _ = self
            .total
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |t| {
                Some(t.saturating_sub(len))
            });
    }

    /// Stores `bytes` (read from a file with `mode`) as a 0600 blob.
    fn put(&mut self, bytes: &[u8], mode: u32) -> Result<Blob, NoEarlier> {
        if self.closed {
            return Err(NoEarlier::NotSeen);
        }
        let len = bytes.len() as u64;
        let id = self.reserve(len)?;
        let path = self.blob_path(id);
        // No fsync: a store that did not survive a crash is swept at the next start anyway, and
        // the walk copies thousands of files.
        let written =
            crate::recovery::create_private_file(&path).and_then(|mut f| f.write_all(bytes));
        if written.is_err() {
            let _ = fs::remove_file(&path);
            self.release(len);
            return Err(NoEarlier::NotSeen);
        }
        Ok(Blob {
            id,
            len,
            hash: content_hash(bytes),
            mode: mode & 0o7777,
        })
    }

    fn drop_blob(&mut self, blob: &Blob) {
        let _ = fs::remove_file(self.blob_path(blob.id));
        self.release(blob.len);
    }

    /// The exact bytes of a pre-image; an altered blob is refused.
    fn read_blob(&self, blob: &Blob) -> io::Result<Vec<u8>> {
        let bad = || io::Error::new(io::ErrorKind::InvalidData, "stored earlier version damaged");
        match read_regular(&self.blob_path(blob.id), blob.len)? {
            ReadOut::Bytes(_, bytes)
                if bytes.len() as u64 == blob.len && content_hash(&bytes) == blob.hash =>
            {
                Ok(bytes)
            }
            _ => Err(bad()),
        }
    }

    fn record_at(&self, rel: &Path) -> Option<usize> {
        self.records.iter().position(|r| r.rel == rel)
    }

    /// `.git` internals, the state / config / trash folders, vr-ide's own temp files and the
    /// versions a Revert replaced, on their way to the trash.
    fn skipped(&self, rel: &Path) -> bool {
        // `.Trash-<uid>` / `.Trash`: the trash of another mount.
        let internal = |c: Component| {
            let name = c.as_os_str().to_string_lossy();
            name == ".git" || name == ".Trash" || name.starts_with(".Trash-")
        };
        if rel.as_os_str().is_empty() || rel.components().any(internal) {
            return true;
        }
        let abs = self.root.join(rel);
        let reverted = rel
            .file_name()
            .is_some_and(|n| n.to_string_lossy().contains(REVERTED_MARK));
        reverted || vr_core::is_save_temp(&abs) || self.excluded.iter().any(|x| abs.starts_with(x))
    }

    fn note(&mut self, why: Incomplete) {
        if self.banner.is_none_or(|b| b.why != why) {
            self.banner = Some(Banner {
                why,
                at: SystemTime::now(),
            });
        }
    }

    /// Adds a record unless the cap is reached; then the list says it may be incomplete and
    /// the pre-image goes back to where it was.
    fn add(&mut self, rel: &Path, kind: Kind, base: Option<Base>) -> bool {
        if self.records.len() >= self.limits.records {
            self.note(Incomplete::RecordLimit);
            if let Some(b) = base {
                self.known.insert(rel.to_path_buf(), b);
            }
            return false;
        }
        let (pre, why) = match base {
            Some(b) => (b.blob, b.why),
            None => (None, None),
        };
        let missing = match kind {
            Kind::Created => None,
            _ if pre.is_some() => None,
            _ => Some(why.unwrap_or(NoEarlier::NotSeen)),
        };
        self.records.push(Record {
            rel: rel.to_path_buf(),
            kind,
            missing,
            since: SystemTime::now(),
            added: None,
            removed: None,
            pre,
            recount: true,
        });
        self.index_dirty = true;
        true
    }

    fn remove_record(&mut self, i: usize) -> Record {
        self.index_dirty = true;
        self.records.remove(i)
    }

    /// Forgets the walk's copies (the program stopped, or starts again).
    fn clear_base(&mut self) {
        let blobs: Vec<Blob> = self.known.drain().filter_map(|(_, b)| b.blob).collect();
        for b in blobs {
            self.drop_blob(&b);
        }
        self.walked.clear();
        self.dirs.clear();
        self.saving.clear();
    }

    /// The remembered state of `rel`, taken out to become a record's earlier version. When
    /// the editor saved the file since (D8: not the agent's change) and the file is no
    /// longer that save, the copy is from before the user's save, not from before the
    /// agent's change: it is dropped, the change is listed without an earlier version.
    fn take_base(&mut self, rel: &Path) -> Option<Base> {
        let mut base = self.known.remove(rel)?;
        if self.saving.remove(rel) {
            if let Some(blob) = base.blob.take() {
                self.drop_blob(&blob);
            }
            base.why = Some(NoEarlier::NotSeen);
        }
        Some(base)
    }

    fn save_index(&mut self) {
        if !self.index_dirty || self.closed {
            return;
        }
        self.index_dirty = false;
        let mut out = String::from(INDEX_HEAD);
        out.push('\n');
        for r in &self.records {
            let (id, len, hash, mode) = r
                .pre
                .as_ref()
                .map_or((0, 0, 0, 0), |b| (b.id, b.len, b.hash, b.mode));
            let missing = match r.missing {
                None => "-",
                Some(NoEarlier::NotSeen) => "n",
                Some(NoEarlier::TooLarge) => "t",
                Some(NoEarlier::Limit) => "l",
            };
            let since = r
                .since
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let path: String = r
                .rel
                .as_os_str()
                .as_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            out.push_str(&format!(
                "{}\t{id:x}\t{len}\t{hash:x}\t{mode:o}\t{missing}\t{since}\t{path}\n",
                r.kind.letter()
            ));
        }
        let tmp = self.dir.join(format!("{INDEX_NAME}.tmp"));
        let _ = fs::remove_file(&tmp);
        let done = crate::recovery::create_private_file(&tmp)
            .and_then(|mut f| f.write_all(out.as_bytes()))
            .and_then(|()| fs::rename(&tmp, self.dir.join(INDEX_NAME)));
        if let Err(e) = done {
            // Paths and contents stay out of the log (SEC-10).
            tracing::warn!(error = %e, "AI changes: the index could not be written");
        }
    }
}

/// The desktop trash folders of this user (files moved there are not the agent's changes).
pub fn trash_dirs() -> Vec<PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    data.map(|d| d.join("Trash")).into_iter().collect()
}

/// A root-relative path an index may name: no `..`, nothing absolute, nothing empty.
fn valid_rel(rel: &Path) -> bool {
    !rel.as_os_str().is_empty() && rel.components().all(|c| matches!(c, Component::Normal(_)))
}

/// Records of a store folder's index (no caller yet, see the module doc). Every line is
/// checked: a path that is not plainly below the root, a damaged line or a repeated path is
/// dropped; a pre-image whose blob is missing or has another size is dropped with it.
pub fn load_index(dir: &Path) -> Vec<Record> {
    let Ok(ReadOut::Bytes(_, bytes)) = read_regular(&dir.join(INDEX_NAME), FILE_CAP) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    let mut lines = text.lines();
    if lines.next() != Some(INDEX_HEAD) {
        return Vec::new();
    }
    let mut out: Vec<Record> = Vec::new();
    for line in lines {
        let f: Vec<&str> = line.split('\t').collect();
        let [kind, id, len, hash, mode, missing, since, path] = f[..] else {
            continue;
        };
        let kind = match kind {
            "M" => Kind::Modified,
            "A" => Kind::Created,
            "D" => Kind::Deleted,
            _ => continue,
        };
        if path.len() % 2 != 0 || !path.is_ascii() {
            continue;
        }
        let Ok(raw) = (0..path.len() / 2)
            .map(|i| u8::from_str_radix(&path[2 * i..2 * i + 2], 16))
            .collect::<Result<Vec<u8>, _>>()
        else {
            continue;
        };
        let rel = PathBuf::from(std::ffi::OsString::from_vec(raw));
        if !valid_rel(&rel) || out.iter().any(|r| r.rel == rel) {
            continue;
        }
        let (Ok(id), Ok(len), Ok(hash), Ok(mode), Ok(since)) = (
            u64::from_str_radix(id, 16),
            len.parse::<u64>(),
            u64::from_str_radix(hash, 16),
            u32::from_str_radix(mode, 8),
            since.parse::<u64>(),
        ) else {
            continue;
        };
        let missing = match missing {
            "-" => None,
            "t" => Some(NoEarlier::TooLarge),
            "l" => Some(NoEarlier::Limit),
            _ => Some(NoEarlier::NotSeen),
        };
        let blob = (id != 0 && missing.is_none() && kind != Kind::Created)
            .then(|| dir.join(format!("{id:016x}.blob")))
            .filter(|p| fs::symlink_metadata(p).is_ok_and(|m| m.is_file() && m.len() == len))
            .map(|_| Blob {
                id,
                len,
                hash,
                mode: mode & 0o7777,
            });
        let missing = match (&blob, kind) {
            (None, Kind::Modified | Kind::Deleted) => Some(missing.unwrap_or(NoEarlier::NotSeen)),
            _ => None,
        };
        out.push(Record {
            rel,
            kind,
            missing,
            since: UNIX_EPOCH + Duration::from_secs(since),
            added: None,
            removed: None,
            pre: blob,
            recount: true,
        });
    }
    out
}

/// Makes `state_dir()/ai` private: created 0700; an existing one must be a real folder of
/// this user and loses any group / other access.
fn private_base(base: &Path) -> io::Result<()> {
    crate::recovery::create_private_dir(base)?;
    let meta = fs::symlink_metadata(base)?;
    if !meta.is_dir() || meta.uid() != rustix::process::getuid().as_raw() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "the AI changes folder is not a folder of this user",
        ));
    }
    if meta.mode() & 0o077 != 0 {
        fs::set_permissions(base, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// A new store folder under a generated name, 0700, with its lock held.
fn create_store(base: &Path) -> io::Result<(PathBuf, File)> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    private_base(base)?;
    for _ in 0..16 {
        let mut rand = [0u8; 8];
        rustix::rand::getrandom(&mut rand[..], rustix::rand::GetRandomFlags::empty())?;
        let name = format!(
            "{}-{}-{:016x}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed),
            u64::from_ne_bytes(rand)
        );
        let dir = base.join(name);
        let mut b = fs::DirBuilder::new();
        std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
        match b.create(&dir) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
        let lock = crate::recovery::create_private_file(&dir.join(LOCK_NAME))?;
        rustix::fs::flock(&lock, rustix::fs::FlockOperation::NonBlockingLockExclusive)?;
        return Ok((dir, lock));
    }
    Err(io::Error::other("no free AI changes folder name"))
}

/// Start-up: removes the store folders no running vr-ide holds (SEC-10). Entries that are not
/// real folders are left alone and no link is followed; a folder without a lock file is one
/// being made (left for a minute) or a broken one.
pub fn sweep(base: &Path) {
    let Ok(entries) = fs::read_dir(base) else {
        return;
    };
    for e in entries.flatten() {
        let dir = e.path();
        let Ok(meta) = fs::symlink_metadata(&dir) else {
            continue;
        };
        if !meta.is_dir() {
            continue;
        }
        let flags = OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
        let stale = match OpenOptions::new()
            .read(true)
            .custom_flags(flags.bits() as i32)
            .open(dir.join(LOCK_NAME))
        {
            Ok(lock) => {
                rustix::fs::flock(&lock, rustix::fs::FlockOperation::NonBlockingLockExclusive)
                    .is_ok()
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => meta
                .modified()
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > Duration::from_secs(60)),
            Err(_) => false,
        };
        if stale {
            let _ = fs::remove_dir_all(&dir);
        }
    }
}

// ---- the engine: walk and events --------------------------------------------------------------

/// The version a file must still be when it is read again as a baseline.
enum Expect<'a> {
    /// The most recent save of vr-ide's own editor.
    Own(&'a OwnWrites),
    /// What a diff showed (Keep).
    Shown(&'a Seen),
}

/// The start-up walk: breadth first from the root, a watch on each directory before it is
/// listed, so nothing changes unseen between the two.
struct Scan {
    epoch: u64,
    queue: VecDeque<PathBuf>,
    /// Directories the ignore rules keep; files of the others are remembered without a copy.
    clean: HashSet<PathBuf>,
    /// Entries of the directory being listed.
    entries: Option<(PathBuf, fs::ReadDir)>,
}

struct Engine {
    shared: Arc<Shared>,
    /// `None` in tests that feed events by hand.
    watcher: Option<Watcher>,
    scan: Option<Scan>,
    epoch: u64,
}

/// Directories below `root` that the ignore rules (.gitignore, .ignore, git's excludes) keep,
/// root-relative, at most `cap` entries looked at.
fn clean_dirs(root: &Path, cap: usize, stop: &dyn Fn() -> bool) -> HashSet<PathBuf> {
    let mut out = HashSet::new();
    out.insert(PathBuf::new());
    let walk = ignore::WalkBuilder::new(root)
        .hidden(false)
        .follow_links(false)
        .filter_entry(|e| e.file_name() != ".git")
        .build();
    for (n, entry) in walk.flatten().enumerate() {
        if n >= cap || (n % 256 == 0 && stop()) {
            break;
        }
        if entry.file_type().is_some_and(|t| t.is_dir())
            && let Ok(rel) = entry.path().strip_prefix(root)
        {
            out.insert(rel.to_path_buf());
        }
    }
    out
}

impl Engine {
    fn new(shared: Arc<Shared>, watcher: Option<Watcher>) -> Engine {
        Engine {
            shared,
            watcher,
            scan: None,
            epoch: 0,
        }
    }

    /// Follows a start, pause or resume: old watches end, a running session is walked again.
    fn sync_epoch(&mut self) {
        let (epoch, running, root, old) = {
            let mut st = self.shared.lock();
            if st.epoch == self.epoch {
                return;
            }
            let old: Vec<PathBuf> = st.dirs.iter().map(|d| st.root.join(d)).collect();
            st.clear_base();
            st.scanning = st.running;
            (st.epoch, st.running, st.root.clone(), old)
        };
        self.epoch = epoch;
        if let Some(w) = &mut self.watcher {
            w.unwatch_dirs(&old);
        }
        self.scan = None;
        if running {
            let (shared, known) = (Arc::clone(&self.shared), self.shared.lock().limits.known);
            let stop = move || shared.stop.load(Ordering::Relaxed);
            self.scan = Some(Scan {
                epoch,
                queue: VecDeque::from([PathBuf::new()]),
                clean: clean_dirs(&root, known, &stop),
                entries: None,
            });
        }
        self.shared.bump();
    }

    /// Watches `rel` (a directory); false when the cap is reached or the watch failed.
    fn watch(&mut self, rel: &Path) -> bool {
        let abs = {
            let mut st = self.shared.lock();
            if st.dirs.contains(rel) {
                return true;
            }
            if st.dirs.len() >= st.limits.dirs {
                st.note(Incomplete::WatchLimit);
                return false;
            }
            st.dirs.insert(rel.to_path_buf());
            st.root.join(rel)
        };
        let Some(w) = &mut self.watcher else {
            return true;
        };
        let failed = w.watch_dirs(std::slice::from_ref(&abs));
        if failed.is_empty() {
            return true;
        }
        let mut st = self.shared.lock();
        st.dirs.remove(rel);
        // A directory that went away meanwhile is not a lost watch.
        if abs.is_dir() {
            st.note(Incomplete::WatchLimit);
        }
        false
    }

    /// Up to `budget` entries of the walk; true once it is finished.
    fn scan_step(&mut self, budget: usize) -> bool {
        let Some(mut scan) = self.scan.take() else {
            return true;
        };
        let root = self.shared.lock().root.clone();
        let mut left = budget;
        while left > 0 {
            if self.shared.lock().epoch != scan.epoch {
                return true;
            }
            if scan.entries.is_none() {
                let Some(rel) = scan.queue.pop_front() else {
                    self.finish_scan();
                    return true;
                };
                left = left.saturating_sub(1);
                let watched = self.shared.lock().dirs.contains(&rel);
                if !self.watch(&rel) {
                    continue;
                }
                if !watched && self.changed_unwatched(&root.join(&rel)) {
                    self.shared.lock().note(Incomplete::WalkedLate);
                    self.shared.bump();
                }
                match fs::read_dir(root.join(&rel)) {
                    Ok(rd) => scan.entries = Some((rel, rd)),
                    Err(_) => continue,
                }
            }
            let Some((dir, rd)) = &mut scan.entries else {
                continue;
            };
            let Some(entry) = rd.next() else {
                let dir = dir.clone();
                self.shared.lock().walked.insert(dir);
                scan.entries = None;
                continue;
            };
            left = left.saturating_sub(1);
            let Ok(entry) = entry else {
                continue;
            };
            let rel = dir.join(entry.file_name());
            if self.shared.lock().skipped(&rel) {
                continue;
            }
            // `DirEntry::file_type` does not follow links: a linked folder is an entry, never
            // walked.
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                scan.queue.push_back(rel);
                continue;
            }
            let clean = scan.clean.contains(dir.as_path());
            if !self.baseline_file(&rel, clean) {
                // The walk's memory is full: directories from here on are not walked, so a
                // file in them is never taken for a new one. They are not watched either:
                // the list says that changes may be missing (SEC-13).
                self.shared.lock().note(Incomplete::WalkLimit);
                self.finish_scan();
                return true;
            }
        }
        self.scan = Some(scan);
        false
    }

    /// Entries of `dir` came or went between the session's start and now, when its watch
    /// begins: every such change sets the folder's own times. A file created in that time is
    /// still found by the walk, one deleted then is in no list (SEC-13).
    fn changed_unwatched(&self, dir: &Path) -> bool {
        let (t0, margin) = {
            let st = self.shared.lock();
            (st.t0, st.limits.margin)
        };
        fs::symlink_metadata(dir).is_ok_and(|m| !Stamp::of(&m).predates(t0, margin))
    }

    fn finish_scan(&mut self) {
        self.scan = None;
        let mut st = self.shared.lock();
        st.scanning = false;
        st.flush_used();
        drop(st);
        self.shared.bump();
    }

    /// The walk met `rel`: remembered, with a copy when it is truly the content from before
    /// the session. False when the walk's memory is full.
    fn baseline_file(&mut self, rel: &Path, clean: bool) -> bool {
        let (abs, t0, margin, cap, epoch, own) = {
            let st = self.shared.lock();
            if st.record_at(rel).is_some() || st.known.contains_key(rel) {
                return true;
            }
            if st.known.len() >= st.limits.known {
                return false;
            }
            (
                st.root.join(rel),
                st.t0,
                st.limits.margin,
                st.limits.file,
                st.epoch,
                st.own.clone(),
            )
        };
        let Ok(meta) = fs::symlink_metadata(&abs) else {
            return true;
        };
        let stamp = Stamp::of(&meta);
        let plain = |why| Base {
            stamp,
            racy: stamp.racy(),
            blob: None,
            why,
        };
        if !stamp.predates(t0, margin) && !own.as_ref().is_some_and(|o| o.is_own(&abs)) {
            // Written since the session started and before the walk came by: a change whose
            // earlier content was never read (SEC-9).
            let mut st = self.shared.lock();
            if st.epoch == epoch && st.record_at(rel).is_none() {
                st.add(rel, Kind::Modified, None);
                drop(st);
                self.shared.bump();
            }
            return true;
        }
        let mut base = plain(Some(NoEarlier::NotSeen));
        let mut bytes = None;
        if clean && meta.is_file() {
            match read_regular(&abs, cap) {
                Ok(ReadOut::Bytes(st, b)) if st == stamp => bytes = Some(b),
                Ok(ReadOut::TooLarge) => base.why = Some(NoEarlier::TooLarge),
                _ => {}
            }
        }
        let mut st = self.shared.lock();
        if st.epoch != epoch || st.record_at(rel).is_some() {
            return true;
        }
        if let Some(b) = bytes {
            match st.put(&b, stamp.mode) {
                Ok(blob) => {
                    base.blob = Some(blob);
                    base.why = None;
                }
                Err(why) => base.why = Some(why),
            }
        }
        st.known.insert(rel.to_path_buf(), base);
        true
    }

    /// The content at `rel` becomes what a later change is compared with (after Keep, after a
    /// save of vr-ide's own editor) - but only content proven to be the version `expect`
    /// names. The file is read some time after the Keep or the save was noticed, and the
    /// agent may have written in between: bytes that are not proven are the agent's, and
    /// they are listed as a change without an earlier version, never stored as one (SEC-9).
    fn rebase(shared: &Shared, rel: &Path, expect: Expect<'_>) {
        let (abs, cap) = {
            let mut st = shared.lock();
            if let Some(old) = st.known.remove(rel).and_then(|b| b.blob) {
                st.drop_blob(&old);
            }
            (st.root.join(rel), st.limits.file)
        };
        #[cfg(test)]
        seam::run();
        let Ok(meta) = fs::symlink_metadata(&abs) else {
            return;
        };
        let stamp = Stamp::of(&meta);
        let mut base = Base {
            stamp,
            racy: true,
            blob: None,
            why: Some(NoEarlier::NotSeen),
        };
        let read = read_regular(&abs, cap);
        // Asked after the read, about the bytes that were read.
        let proven = match (&expect, &read) {
            (_, Ok(ReadOut::Bytes(s, _))) if *s != stamp => false,
            // Still our save, untouched since: the stamp of the read is the save's.
            (Expect::Own(own), _) => own.is_own(&abs),
            (Expect::Shown(Seen(Some(shown))), Ok(ReadOut::Bytes(_, bytes))) => {
                shown.stamp.len == bytes.len() as u64 && shown.hash == content_hash(bytes)
            }
            // Nothing is stored for what was not read; the look alone decides.
            (Expect::Shown(seen), _) => still(&abs, seen).unwrap_or(false),
        };
        let mut st = shared.lock();
        if st.record_at(rel).is_some() || !st.running {
            return;
        }
        if !proven {
            // A file where the kept deletion was is a new file.
            let kind = match expect {
                Expect::Shown(Seen(None)) => Kind::Created,
                _ => Kind::Modified,
            };
            st.add(rel, kind, None);
            st.save_index();
            return;
        }
        match read {
            Ok(ReadOut::Bytes(s, bytes)) if s == stamp => match st.put(&bytes, stamp.mode) {
                Ok(blob) => {
                    base.blob = Some(blob);
                    base.why = None;
                }
                Err(why) => base.why = Some(why),
            },
            Ok(ReadOut::TooLarge) => base.why = Some(NoEarlier::TooLarge),
            _ => {}
        }
        st.known.insert(rel.to_path_buf(), base);
    }

    fn handle(&mut self, events: Vec<FsEvent>) {
        if events.is_empty() {
            return;
        }
        if !self.shared.lock().running {
            return;
        }
        let mut gone_dirs = Vec::new();
        for ev in events {
            match ev {
                FsEvent::Modified(p) | FsEvent::Created(p) => self.touch(&p, &mut gone_dirs),
                FsEvent::Removed(p) => self.gone(&p, &mut gone_dirs),
                FsEvent::Renamed(from, to) => {
                    self.gone(&from, &mut gone_dirs);
                    self.touch(&to, &mut gone_dirs);
                }
                FsEvent::Rescan(_) => self.shared.lock().note(Incomplete::Rescan),
                FsEvent::Error(_) => self.shared.lock().note(Incomplete::Watcher),
            }
        }
        if !gone_dirs.is_empty() {
            self.dirs_gone(&gone_dirs);
        }
        self.settle();
    }

    /// Counts and the index follow what the walk and the events recorded.
    fn settle(&mut self) {
        self.recount();
        let mut st = self.shared.lock();
        st.save_index();
        st.flush_used();
        drop(st);
        self.shared.bump();
    }

    /// The root-relative path of an event's path, when it is one to record: below the root
    /// by its real location too (a path through a linked folder names a file elsewhere,
    /// SEC-11).
    fn rel_of(&self, p: &Path) -> Option<PathBuf> {
        let (root, rel) = {
            let st = self.shared.lock();
            let rel = p.strip_prefix(&st.root).ok()?.to_path_buf();
            if st.skipped(&rel) {
                return None;
            }
            (st.root.clone(), rel)
        };
        let parent = rel.parent().unwrap_or(Path::new(""));
        match fs::canonicalize(root.join(parent)) {
            Ok(real) if real != root.join(parent) => None,
            // A folder that is gone took the file with it.
            _ => Some(rel),
        }
    }

    fn touch(&mut self, p: &Path, gone_dirs: &mut Vec<PathBuf>) {
        let Some(rel) = self.rel_of(p) else {
            return;
        };
        match fs::symlink_metadata(p) {
            Err(_) => self.gone(p, gone_dirs),
            Ok(m) if m.is_dir() => {
                // A folder where a file was: that file is gone.
                let mut st = self.shared.lock();
                if !st.dirs.contains(&rel) {
                    Engine::file_gone(&mut st, &rel);
                }
                drop(st);
                self.new_dir(&rel)
            }
            Ok(m) => self.changed(&rel, &m, false),
        }
    }

    /// A directory that appeared during the session: watched and listed at once; its files are
    /// new ones.
    fn new_dir(&mut self, rel: &Path) {
        if self.shared.lock().dirs.contains(rel) {
            return;
        }
        let root = self.shared.lock().root.clone();
        let mut queue = VecDeque::from([rel.to_path_buf()]);
        while let Some(dir) = queue.pop_front() {
            if !self.watch(&dir) {
                continue;
            }
            let Ok(rd) = fs::read_dir(root.join(&dir)) else {
                continue;
            };
            for entry in rd.flatten() {
                let rel = dir.join(entry.file_name());
                if self.shared.lock().skipped(&rel) {
                    continue;
                }
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    queue.push_back(rel);
                } else if let Ok(m) = fs::symlink_metadata(root.join(&rel)) {
                    self.changed(&rel, &m, true);
                }
            }
            self.shared.lock().walked.insert(dir);
        }
    }

    /// Something is at `rel` after an event (never read as a pre-image here: the event came
    /// after the write).
    fn changed(&mut self, rel: &Path, meta: &Metadata, new_dir: bool) {
        let stamp = Stamp::of(meta);
        let (abs, own, t0, margin) = {
            let st = self.shared.lock();
            (st.root.join(rel), st.own.clone(), st.t0, st.limits.margin)
        };
        let existing = {
            let st = self.shared.lock();
            st.record_at(rel)
                .map(|i| (st.records[i].kind, st.records[i].pre.clone()))
        };
        if let Some((kind, pre)) = existing {
            // Back to exactly the earlier version (the agent undid itself): nothing changed.
            let same = pre.as_ref().is_some_and(|b| {
                meta.is_file()
                    && stamp.len == b.len
                    && stamp.mode & 0o7777 == b.mode
                    && state_of(&abs).is_ok_and(|s| s.is_some_and(|s| s.hash == b.hash))
            });
            let mut st = self.shared.lock();
            let Some(i) = st.record_at(rel) else {
                return;
            };
            if same {
                let r = st.remove_record(i);
                st.known.insert(
                    rel.to_path_buf(),
                    Base {
                        stamp,
                        racy: true,
                        blob: r.pre,
                        why: None,
                    },
                );
                return;
            }
            if kind == Kind::Deleted {
                st.records[i].kind = Kind::Modified;
                st.index_dirty = true;
            }
            st.records[i].recount = true;
            return;
        }
        if let Some(own) = own.as_ref().filter(|o| o.is_own(&abs)) {
            self.shared.lock().saving.remove(rel);
            Engine::rebase(&self.shared, rel, Expect::Own(own));
            return;
        }
        let base = self.shared.lock().known.get(rel).cloned();
        match base {
            Some(b) => {
                if b.stamp == stamp && !(b.racy && b.blob.is_some()) {
                    return;
                }
                let same = b.blob.as_ref().is_some_and(|blob| {
                    meta.is_file()
                        && stamp.len == blob.len
                        && stamp.mode & 0o7777 == blob.mode
                        && state_of(&abs).is_ok_and(|s| s.is_some_and(|s| s.hash == blob.hash))
                });
                let mut st = self.shared.lock();
                if same {
                    if let Some(k) = st.known.get_mut(rel) {
                        k.stamp = stamp;
                        k.racy = stamp.racy();
                    }
                    return;
                }
                if let Some(b) = st.take_base(rel) {
                    st.add(rel, Kind::Modified, Some(b));
                }
            }
            None => {
                if !new_dir && stamp.predates(t0, margin) {
                    return;
                }
                let mut st = self.shared.lock();
                let parent = rel.parent().unwrap_or(Path::new(""));
                // A file the walk did not list in a directory it did list is new; elsewhere
                // nobody knows whether it existed.
                let kind = if new_dir || st.walked.contains(parent) {
                    Kind::Created
                } else {
                    Kind::Modified
                };
                st.add(rel, kind, None);
            }
        }
    }

    fn gone(&mut self, p: &Path, gone_dirs: &mut Vec<PathBuf>) {
        let Some(rel) = self.rel_of(p) else {
            return;
        };
        if fs::symlink_metadata(p).is_ok() {
            return;
        }
        let mut st = self.shared.lock();
        if st.dirs.contains(&rel) {
            gone_dirs.push(rel.clone());
        }
        Engine::file_gone(&mut st, &rel);
    }

    fn file_gone(st: &mut State, rel: &Path) {
        if let Some(i) = st.record_at(rel) {
            match st.records[i].kind {
                // Made and removed within the session: nothing to show.
                Kind::Created => {
                    st.remove_record(i);
                }
                Kind::Modified => {
                    st.records[i].kind = Kind::Deleted;
                    st.records[i].recount = true;
                    st.index_dirty = true;
                }
                Kind::Deleted => {}
            }
        } else if let Some(b) = st.take_base(rel) {
            st.add(rel, Kind::Deleted, Some(b));
        }
    }

    /// Watched directories went away (removed or moved out): their files went with them, with
    /// no event of their own when the directory was moved.
    fn dirs_gone(&mut self, gone: &[PathBuf]) {
        let mut st = self.shared.lock();
        let under = |p: &Path| gone.iter().any(|d| p.starts_with(d));
        let files: Vec<PathBuf> = st
            .known
            .keys()
            .chain(st.records.iter().map(|r| &r.rel))
            .filter(|p| under(p))
            .cloned()
            .collect();
        let root = st.root.clone();
        for rel in files {
            if fs::symlink_metadata(root.join(&rel)).is_err() {
                Engine::file_gone(&mut st, &rel);
            }
        }
        let dirs: Vec<PathBuf> = st.dirs.iter().filter(|d| under(d)).cloned().collect();
        let mut off = Vec::new();
        for d in dirs {
            if fs::symlink_metadata(root.join(&d)).is_err() {
                st.dirs.remove(&d);
                st.walked.remove(&d);
                off.push(root.join(&d));
            }
        }
        drop(st);
        if let Some(w) = &mut self.watcher {
            w.unwatch_dirs(&off);
        }
    }

    /// Line counts of the records that changed (read outside the lock).
    fn recount(&mut self) {
        let todo: Vec<(PathBuf, Kind, Option<Blob>)> = {
            let mut st = self.shared.lock();
            st.records
                .iter_mut()
                .filter(|r| r.recount)
                .map(|r| {
                    r.recount = false;
                    (r.rel.clone(), r.kind, r.pre.clone())
                })
                .collect()
        };
        for (rel, kind, pre) in todo {
            let (abs, cap, old) = {
                let st = self.shared.lock();
                let old = pre.as_ref().and_then(|b| st.read_blob(b).ok());
                (st.root.join(&rel), st.limits.file, old)
            };
            let new = match read_regular(&abs, cap) {
                Ok(ReadOut::Bytes(_, b)) => Some(b),
                _ => None,
            };
            let counts = match kind {
                Kind::Created => new.as_deref().map(|n| counts(b"", n)),
                Kind::Deleted => old.as_deref().map(|o| counts(o, b"")),
                Kind::Modified => match (&old, &new) {
                    (Some(o), Some(n)) => Some(counts(o, n)),
                    _ => None,
                },
            }
            .flatten();
            let mut st = self.shared.lock();
            if let Some(i) = st.record_at(&rel) {
                st.records[i].added = counts.map(|c| c.0);
                st.records[i].removed = counts.map(|c| c.1);
            }
        }
    }
}

fn run(shared: Arc<Shared>) {
    let (tx, rx) = mpsc::channel();
    let watcher = match Watcher::new(tx) {
        Ok(w) => Some(w),
        Err(_) => {
            shared.lock().note(Incomplete::Watcher);
            None
        }
    };
    let alive = watcher.is_some();
    let mut engine = Engine::new(Arc::clone(&shared), watcher);
    while !shared.stop.load(Ordering::Relaxed) {
        engine.sync_epoch();
        let events = drain(&rx, engine.scan.is_none());
        engine.handle(events);
        if engine.scan.is_some() {
            if !alive {
                // Without a watcher the walk still gives every file its earlier version.
                shared.lock().note(Incomplete::Watcher);
            }
            engine.scan_step(SCAN_STEP);
            if engine.shared.lock().records.iter().any(|r| r.recount) {
                engine.settle();
            }
        }
    }
}

/// Pending events; with `wait`, up to a short time for the first one.
fn drain(rx: &Receiver<FsEvent>, wait: bool) -> Vec<FsEvent> {
    let mut out = Vec::new();
    if wait && let Ok(e) = rx.recv_timeout(Duration::from_millis(100)) {
        out.push(e);
    }
    out.extend(rx.try_iter());
    out
}

// ---- diff -------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Hunk,
    Note,
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    /// Line number in the file now (context, added) or in the earlier version (removed).
    pub num: Option<u32>,
    pub text: String,
    /// Where the line starts in the text it is from (bytes): [`Diff::old_text`] for a removed
    /// line, [`Diff::new_text`] for the others. The syntax colours are looked up there.
    pub at: Option<usize>,
}

/// A record's diff as shown in the editor, with the version of the file it shows.
#[derive(Debug, Clone)]
pub struct Diff {
    pub rel: PathBuf,
    pub kind: Kind,
    pub missing: Option<NoEarlier>,
    pub since: SystemTime,
    pub lines: Vec<DiffLine>,
    pub added: u32,
    pub removed: u32,
    pub can_revert: bool,
    pub seen: Seen,
    /// The two texts as the lines were cut from them; `None` for a side that is absent,
    /// binary or was not read. Kept in memory only, like the lines.
    pub old_text: Option<Arc<str>>,
    pub new_text: Option<Arc<str>>,
}

/// A side of a diff as text, when it is text.
fn side_text(bytes: Option<&[u8]>) -> Option<Arc<str>> {
    let bytes = bytes.filter(|b| !is_binary(b))?;
    Some(Arc::from(String::from_utf8_lossy(bytes).as_ref()))
}

/// Byte offset at which each of `slices` starts in the text they were cut from.
fn starts<'a>(slices: impl Iterator<Item = &'a str>) -> Vec<usize> {
    let mut at = 0;
    slices
        .map(|s| {
            let here = at;
            at += s.len();
            here
        })
        .collect()
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(8000)].contains(&0)
}

fn line_text(s: &str) -> String {
    s.trim_end_matches('\n').trim_end_matches('\r').to_string()
}

/// (added, removed) lines; `None` for binary content.
fn counts(old: &[u8], new: &[u8]) -> Option<(u32, u32)> {
    if is_binary(old) || is_binary(new) {
        return None;
    }
    let (old, new) = (String::from_utf8_lossy(old), String::from_utf8_lossy(new));
    let diff = similar::TextDiff::configure()
        .timeout(DIFF_TIMEOUT)
        .diff_lines(old.as_ref(), new.as_ref());
    let (mut a, mut r) = (0u32, 0u32);
    for op in diff.ops() {
        match op.tag() {
            similar::DiffTag::Equal => {}
            similar::DiffTag::Delete => r += op.old_range().len() as u32,
            similar::DiffTag::Insert => a += op.new_range().len() as u32,
            similar::DiffTag::Replace => {
                r += op.old_range().len() as u32;
                a += op.new_range().len() as u32;
            }
        }
    }
    Some((a, r))
}

/// The nearest line above `at` that starts a block (no indent), as git shows after `@@`.
fn hunk_context(old: &[&str], at: usize) -> String {
    old[..at.min(old.len())]
        .iter()
        .rev()
        .find(|l| {
            l.chars()
                .next()
                .is_some_and(|c| c.is_alphabetic() || c == '_')
        })
        .map(|l| {
            let t = line_text(l);
            t.trim_end_matches('{')
                .trim_end()
                .chars()
                .take(60)
                .collect()
        })
        .unwrap_or_default()
}

/// Unified diff lines (three lines of context) and the counts.
fn diff_lines(old: &[u8], new: &[u8], kind: Kind) -> (Vec<DiffLine>, u32, u32) {
    let note = |text: String| DiffLine {
        kind: LineKind::Note,
        num: None,
        text,
        at: None,
    };
    if is_binary(old) || is_binary(new) {
        let text = format!(
            "Binary file: {} bytes before, {} bytes now; no text diff.",
            old.len(),
            new.len()
        );
        return (vec![note(text)], 0, 0);
    }
    let (old_s, new_s) = (String::from_utf8_lossy(old), String::from_utf8_lossy(new));
    let diff = similar::TextDiff::configure()
        .timeout(DIFF_TIMEOUT)
        .diff_lines(old_s.as_ref(), new_s.as_ref());
    let old_lines: Vec<&str> = diff.iter_old_slices().collect();
    let (old_at, new_at) = (
        starts(diff.iter_old_slices()),
        starts(diff.iter_new_slices()),
    );
    let mut out = Vec::new();
    let (mut added, mut removed) = (0u32, 0u32);
    'groups: for group in diff.grouped_ops(3) {
        let (Some(first), Some(last)) = (group.first(), group.last()) else {
            continue;
        };
        let (o, n) = (
            first.old_range().start..last.old_range().end,
            first.new_range().start..last.new_range().end,
        );
        let start = |r: &std::ops::Range<usize>| if r.is_empty() { r.start } else { r.start + 1 };
        let context = match kind {
            Kind::Created => "new file".to_string(),
            Kind::Deleted => "deleted".to_string(),
            Kind::Modified => hunk_context(&old_lines, o.start),
        };
        out.push(DiffLine {
            kind: LineKind::Hunk,
            num: None,
            text: format!(
                "@@ -{},{} +{},{} @@ {context}",
                start(&o),
                o.len(),
                start(&n),
                n.len()
            )
            .trim_end()
            .to_string(),
            at: None,
        });
        for op in &group {
            for change in diff.iter_changes(op) {
                if out.len() >= DIFF_LINES_MAX {
                    out.push(note(format!("Diff cut at {DIFF_LINES_MAX} lines.")));
                    break 'groups;
                }
                let (kind, num, side) = match change.tag() {
                    similar::ChangeTag::Equal => (LineKind::Context, change.new_index(), &new_at),
                    similar::ChangeTag::Insert => {
                        added += 1;
                        (LineKind::Added, change.new_index(), &new_at)
                    }
                    similar::ChangeTag::Delete => {
                        removed += 1;
                        (LineKind::Removed, change.old_index(), &old_at)
                    }
                };
                out.push(DiffLine {
                    kind,
                    num: num.map(|i| i as u32 + 1),
                    text: line_text(change.value()),
                    at: num.and_then(|i| side.get(i).copied()),
                });
            }
        }
    }
    (out, added, removed)
}

/// The file as it is now, numbered, for a record without an earlier version.
fn plain_lines(new: Option<&[u8]>) -> Vec<DiffLine> {
    let Some(new) = new.filter(|b| !is_binary(b)) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(new);
    let lines: Vec<&str> = text.split_inclusive('\n').take(DIFF_LINES_MAX).collect();
    let at = starts(lines.iter().copied());
    lines
        .iter()
        .zip(at)
        .enumerate()
        .map(|(i, (l, at))| DiffLine {
            kind: LineKind::Context,
            num: Some(i as u32 + 1),
            text: line_text(l),
            at: Some(at),
        })
        .collect()
}

// ---- revert -----------------------------------------------------------------------------------

#[derive(Debug)]
pub enum RevertError {
    /// The file is not what the diff showed any more; nothing was written.
    Changed,
    /// No pre-image: Keep only.
    NoEarlier,
    /// The record is gone (kept or reverted meanwhile).
    Gone,
    /// The file's folder is no longer inside the session's folder; nothing was written.
    Outside,
    /// A folder is at the path now; it is never moved, replaced or trashed.
    Folder,
    Failed(String),
}

/// The folder a Revert writes in, held open: every path of the swap goes through this handle
/// (`/proc/self/fd/N/name`), so a folder swapped for a link after the check cannot move the
/// write elsewhere (SEC-12).
struct Anchor {
    dir: File,
    real: PathBuf,
    via_fd: bool,
}

impl Anchor {
    fn open(parent: &Path, root: &Path) -> Result<Anchor, RevertError> {
        let fail = |e: io::Error| RevertError::Failed(e.to_string());
        let dir = OpenOptions::new()
            .read(true)
            .custom_flags((OFlags::DIRECTORY | OFlags::CLOEXEC).bits() as i32)
            .open(parent)
            .map_err(fail)?;
        let link = PathBuf::from(format!("/proc/self/fd/{}", dir.as_raw_fd()));
        let (real, via_fd) = match fs::read_link(&link) {
            Ok(p) => (p, true),
            Err(_) => (fs::canonicalize(parent).map_err(fail)?, false),
        };
        let a = Anchor { dir, real, via_fd };
        if !a.real.starts_with(root) {
            return Err(RevertError::Outside);
        }
        Ok(a)
    }

    /// The open folder is (still) inside `root`.
    fn inside(&self, root: &Path) -> bool {
        if !self.via_fd {
            return fs::canonicalize(&self.real).is_ok_and(|p| p.starts_with(root));
        }
        fs::read_link(self.base()).is_ok_and(|p| p.starts_with(root))
    }

    fn base(&self) -> PathBuf {
        if self.via_fd {
            PathBuf::from(format!("/proc/self/fd/{}", self.dir.as_raw_fd()))
        } else {
            self.real.clone()
        }
    }

    fn path(&self, name: &std::ffi::OsStr) -> PathBuf {
        self.base().join(name)
    }

    /// Where the open folder is now (it may have been moved since it was opened).
    fn real_now(&self) -> PathBuf {
        if self.via_fd {
            fs::read_link(self.base()).unwrap_or_else(|_| self.real.clone())
        } else {
            self.real.clone()
        }
    }
}

/// The file a Revert sends to the trash, held open from its last check until after the move.
struct Pin {
    /// Readable when it is a regular file; a path-only handle otherwise.
    file: File,
    regular: bool,
    id: (u64, u64),
}

/// What became of the pinned file.
enum Moved {
    /// It is still at the path.
    No,
    /// It left the path with the content that was shown.
    Same,
    /// It left the path, written to since the check.
    Written,
    /// It is linked nowhere any more: something else was at the path when the trash came
    /// (a replacement by rename), or the trash copied it to another file system.
    Unknown,
}

impl Pin {
    /// `None` when the path does not hold the version `seen` (the same file, by identity).
    fn open(path: &Path, seen: &Seen) -> io::Result<Option<Pin>> {
        let Some(shown) = &seen.0 else {
            return Ok(None);
        };
        let regular = shown.stamp.mode & 0o170000 == 0o100000;
        let opened = if regular {
            open_regular(path).map(|o| o.map(|(f, _)| f))
        } else {
            // A link (or FIFO, socket, device) is held by a handle that cannot read it.
            let flags = OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC;
            OpenOptions::new()
                .read(true)
                .custom_flags(flags.bits() as i32)
                .open(path)
                .map(Some)
        };
        let file = match opened {
            Ok(Some(f)) => f,
            Ok(None) => return Ok(None),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let meta = file.metadata()?;
        let id = (meta.dev(), meta.ino());
        // The handle is on the file the check below is about, and that check passes.
        if id != (shown.stamp.dev, shown.stamp.ino) || !still(path, seen)? {
            return Ok(None);
        }
        Ok(Some(Pin { file, regular, id }))
    }

    fn after(&self, path: &Path, seen: &Seen) -> Moved {
        if fs::symlink_metadata(path).is_ok_and(|m| (m.dev(), m.ino()) == self.id) {
            return Moved::No;
        }
        let Ok(meta) = self.file.metadata() else {
            return Moved::Unknown;
        };
        if meta.nlink() == 0 {
            return Moved::Unknown;
        }
        let same = !self.regular
            || seen.0.is_some_and(|s| {
                meta.len() == s.stamp.len && hash_stream(&self.file).is_ok_and(|h| h == s.hash)
            });
        if same { Moved::Same } else { Moved::Written }
    }
}

/// What a Revert did, for the notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reverted {
    Restored,
    /// Restored, but the version that was replaced could not be moved to the trash: it is
    /// kept in this file next to the restored one (never deleted).
    RestoredOldKept(PathBuf),
    Trashed,
    /// In the trash, but written to while it was moved there: the trash holds a version
    /// the diff did not show.
    TrashedChanged,
    /// Something went to the trash, but the file that was checked was replaced or copied on
    /// the way: what the trash holds could not be compared with what the diff showed.
    TrashedUnchecked,
}

#[derive(Debug)]
pub enum KeepError {
    /// The file is not what the diff showed any more; the record stays.
    Changed,
    /// The record is gone (kept or reverted meanwhile).
    Gone,
    Failed(String),
}

/// The name a replaced version of `name` has on its way to the trash.
fn reverted_name(name: &std::ffi::OsStr) -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let name = name.to_string_lossy();
    // Under NAME_MAX (255 bytes) with room for the suffix.
    let mut cut = name.len().min(200);
    while !name.is_char_boundary(cut) {
        cut -= 1;
    }
    format!(
        "{}{REVERTED_MARK}{}-{}",
        &name[..cut],
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    )
}

/// `a` becomes `b` unless something is at `b`.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn rename_noreplace(a: &Path, b: &Path) -> io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    renameat_with(CWD, a, CWD, b, RenameFlags::NOREPLACE).map_err(io::Error::from)
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn rename_noreplace(_a: &Path, _b: &Path) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// Puts `tmp` at `path` in exchange for the link (or other non-file) `seen` describes; when
/// something else came out, it is put back and nothing is replaced.
fn swap_out(
    tmp: &Path,
    path: &Path,
    seen: &Seen,
    ours: (u64, u64),
    seam: &mut dyn FnMut(Step),
) -> io::Result<Outcome> {
    if !still(path, seen)? {
        fs::remove_file(tmp)?;
        return Ok(Outcome::Changed);
    }
    seam(Step::BeforeSwap);
    match safe_replace::exchange_files(tmp, path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            fs::remove_file(tmp)?;
            return Ok(Outcome::Changed);
        }
        Err(e) => return Err(e),
    }
    // From here `tmp` names what was at the path.
    seam(Step::AfterSwap);
    if holds(tmp, seen).unwrap_or(false) {
        seam(Step::BeforeDeleteOld);
        return match fs::remove_file(tmp) {
            Ok(()) => Ok(Outcome::Replaced),
            Err(cause) => Ok(Outcome::Kept {
                at: tmp.to_path_buf(),
                what: "the replaced link could not be removed",
                cause,
            }),
        };
    }
    seam(Step::BeforeSwapBack);
    match safe_replace::exchange_files(tmp, path) {
        // Ours is back under the temp name, unless someone replaced it meanwhile: only a
        // file proven to be ours is deleted.
        Ok(()) => {
            seam(Step::AfterSwapBack);
            let is_ours = fs::symlink_metadata(tmp).is_ok_and(|m| m.is_file() && m.len() == ours.0)
                && state_of(tmp).is_ok_and(|s| s.is_some_and(|s| s.hash == ours.1));
            if is_ours {
                fs::remove_file(tmp)?;
                return Ok(Outcome::Changed);
            }
            Ok(Outcome::Kept {
                at: tmp.to_path_buf(),
                what: "the file changed while it was being put back",
                cause: io::Error::other("another version appeared"),
            })
        }
        Err(cause) => Ok(Outcome::Kept {
            at: tmp.to_path_buf(),
            what: "the file changed and could not be put back",
            cause,
        }),
    }
}

// ---- the handle -------------------------------------------------------------------------------

/// A session's change tracker. Dropping it ends the worker and deletes the store (the tab was
/// closed: SEC-10 cleanup).
pub struct Changes {
    shared: Arc<Shared>,
    #[cfg(test)]
    engine: Option<std::cell::RefCell<Engine>>,
}

impl Changes {
    fn build(cfg: Config) -> io::Result<Arc<Shared>> {
        let root = fs::canonicalize(&cfg.root)?;
        let (dir, lock) = create_store(&cfg.store_base)?;
        let mut excluded: Vec<PathBuf> = cfg
            .excluded
            .iter()
            .chain(std::iter::once(&cfg.store_base))
            .map(|p| fs::canonicalize(p).unwrap_or_else(|_| p.clone()))
            .collect();
        excluded.dedup();
        Ok(Arc::new(Shared {
            state: Mutex::new(State {
                root,
                dir,
                _lock: lock,
                limits: cfg.limits,
                base: cfg.store_base,
                total: cfg.total,
                used: 0,
                flushed: None,
                others: None,
                next_blob: 0,
                excluded,
                own: cfg.own,
                t0: SystemTime::now(),
                running: true,
                closed: false,
                epoch: 1,
                scanning: true,
                known: HashMap::new(),
                walked: HashSet::new(),
                dirs: HashSet::new(),
                records: Vec::new(),
                banner: None,
                index_dirty: false,
                saving: HashSet::new(),
            }),
            version: AtomicU64::new(0),
            wake: cfg.wake,
            stop: AtomicBool::new(false),
        }))
    }

    /// Starts tracking `cfg.root`; call it before the agent's program is started, so every
    /// write of the program is later than the start time taken here. The walk and the watching
    /// run on a thread of their own.
    pub fn start(cfg: Config) -> io::Result<Changes> {
        let shared = Changes::build(cfg)?;
        let worker = Arc::clone(&shared);
        std::thread::Builder::new()
            .name("vr-ai-changes".into())
            .spawn(move || run(worker))?;
        Ok(Changes {
            shared,
            #[cfg(test)]
            engine: None,
        })
    }

    /// A tracker without a thread or watcher: tests walk and feed events themselves.
    #[cfg(test)]
    pub fn manual(cfg: Config) -> io::Result<Changes> {
        let shared = Changes::build(cfg)?;
        let engine = Engine::new(Arc::clone(&shared), None);
        Ok(Changes {
            shared,
            engine: Some(std::cell::RefCell::new(engine)),
        })
    }

    /// Runs the start-up walk to its end.
    #[cfg(test)]
    pub fn scan(&self) {
        let mut e = self.engine.as_ref().expect("manual").borrow_mut();
        e.sync_epoch();
        while !e.scan_step(SCAN_STEP) {}
        e.settle();
    }

    #[cfg(test)]
    pub fn feed(&self, ev: FsEvent) {
        let mut e = self.engine.as_ref().expect("manual").borrow_mut();
        e.sync_epoch();
        e.handle(vec![ev]);
    }

    /// Several events as one batch of the watcher.
    #[cfg(test)]
    pub fn feed_all(&self, events: Vec<FsEvent>) {
        let mut e = self.engine.as_ref().expect("manual").borrow_mut();
        e.sync_epoch();
        e.handle(events);
    }

    #[cfg(test)]
    pub fn store_dir(&self) -> PathBuf {
        self.shared.lock().dir.clone()
    }

    /// The folder tracked (canonical).
    pub fn root(&self) -> PathBuf {
        self.shared.lock().root.clone()
    }

    /// Gives every record the same time (screens with a fixed clock).
    #[cfg(test)]
    pub fn set_times(&self, at: SystemTime) {
        let mut st = self.shared.lock();
        for r in &mut st.records {
            r.since = at;
        }
        if let Some(b) = &mut st.banner {
            b.at = at;
        }
        drop(st);
        self.shared.bump();
    }

    /// Marks the list as possibly incomplete, as the walk or the watcher would.
    #[cfg(test)]
    pub fn set_incomplete(&self, why: Incomplete) {
        self.shared.lock().note(why);
        self.shared.bump();
    }

    /// The editor is about to save `abs` (`begins`), or that save wrote nothing.
    /// Call it right before the save, so no write of the agent can come between the save and
    /// this knowledge: a copy taken before the save must not pass for the version before a
    /// change the agent makes after it.
    pub fn own_save(&self, abs: &Path, begins: bool) {
        let root = self.shared.lock().root.clone();
        let real = abs
            .parent()
            .and_then(|d| fs::canonicalize(d).ok())
            .zip(abs.file_name())
            .map(|(d, n)| d.join(n));
        let Some(rel) = real.as_deref().and_then(|p| p.strip_prefix(&root).ok()) else {
            return;
        };
        let mut st = self.shared.lock();
        if !begins {
            st.saving.remove(rel);
        } else if st.running && st.record_at(rel).is_none() && st.known.contains_key(rel) {
            st.saving.insert(rel.to_path_buf());
        }
    }

    /// Marks the record of `rel` as one without an earlier version, for `why`.
    #[cfg(test)]
    pub fn set_missing(&self, rel: &Path, why: NoEarlier) {
        let mut st = self.shared.lock();
        if let Some(i) = st.record_at(rel) {
            if let Some(blob) = st.records[i].pre.take() {
                st.drop_blob(&blob);
            }
            st.records[i].missing = Some(why);
        }
        drop(st);
        self.shared.bump();
    }

    /// Drops the walk's copy of `rel`, as for a file it could not copy.
    #[cfg(test)]
    pub fn forget_earlier(&self, rel: &Path) {
        let mut st = self.shared.lock();
        if let Some(blob) = st.known.get_mut(rel).and_then(|b| b.blob.take()) {
            st.drop_blob(&blob);
        }
    }

    /// Changes whenever the list, its counts or its banner did.
    pub fn version(&self) -> u64 {
        self.shared.version.load(Ordering::Relaxed)
    }

    pub fn records(&self) -> Vec<Record> {
        self.shared.lock().records.clone()
    }

    pub fn count(&self) -> usize {
        self.shared.lock().records.len()
    }

    /// Why the list may miss changes: a cause noted by the watcher, a cap or a walk that was
    /// not everywhere in time, else the start-up walk while it runs (a folder it has not
    /// reached is not watched yet). The walk's own notice ends with the walk, not by hand;
    /// what the walk could not see by then is a noted cause and outlives it.
    pub fn banner(&self) -> Option<Banner> {
        let st = self.shared.lock();
        st.banner.or_else(|| {
            (st.scanning && st.running).then_some(Banner {
                why: Incomplete::Scanning,
                at: st.t0,
            })
        })
    }

    /// Hides the "may be incomplete" banner until the next cause.
    pub fn dismiss_banner(&self) {
        self.shared.lock().banner = None;
        self.shared.bump();
    }

    /// The start-up walk still runs.
    pub fn scanning(&self) -> bool {
        self.shared.lock().scanning
    }

    pub fn running(&self) -> bool {
        self.shared.lock().running
    }

    /// The program exited: nothing is attributed to it any more. The list stays; the walk's
    /// copies go (they describe the folder as it was while the program ran).
    pub fn pause(&self) {
        let mut st = self.shared.lock();
        if st.running {
            if st.scanning {
                // The walk stops here. What the program did in the folders it had not
                // reached was never seen, and no later walk can tell: the list says so
                // until the notice is dismissed (SEC-13).
                st.note(Incomplete::Unwalked);
            }
            st.running = false;
            st.epoch += 1;
            drop(st);
            self.shared.bump();
        }
    }

    /// The program starts again (call before it is started): a fresh walk from now.
    pub fn resume(&self) {
        let mut st = self.shared.lock();
        if st.running && st.scanning {
            // The walk of the run before is cut short like by an exit.
            st.note(Incomplete::Unwalked);
        }
        st.running = true;
        st.t0 = SystemTime::now();
        st.epoch += 1;
        st.scanning = true;
        drop(st);
        self.shared.bump();
    }

    /// The diff of `rel` (earlier version against the file now) and the version of the file it
    /// shows, which a Revert must still find.
    pub fn diff(&self, rel: &Path) -> Result<Diff, String> {
        let (abs, cap, record, old) = {
            let st = self.shared.lock();
            let i = st.record_at(rel).ok_or("no such change")?;
            let r = st.records[i].clone();
            let old = match &r.pre {
                Some(b) => Some(st.read_blob(b).map_err(|e| e.to_string())?),
                None => None,
            };
            (st.root.join(rel), st.limits.file, r, old)
        };
        // The expectation first, the content after it: a write in between shows as a changed
        // stamp or hash at Revert, never as an accepted version.
        let seen = Seen(state_of(&abs).map_err(|e| e.to_string())?);
        let new = match read_regular(&abs, cap) {
            Ok(ReadOut::Bytes(_, b)) => Some(b),
            _ => None,
        };
        let note = |text: &str| DiffLine {
            kind: LineKind::Note,
            num: None,
            text: text.to_string(),
            at: None,
        };
        let (mut lines, mut added, mut removed) = (Vec::new(), 0, 0);
        match (record.kind, &old, &new) {
            (Kind::Created, _, Some(n)) => {
                (lines, added, removed) = diff_lines(b"", n, Kind::Created)
            }
            (Kind::Deleted, Some(o), _) => {
                (lines, added, removed) = diff_lines(o, b"", Kind::Deleted)
            }
            (Kind::Modified, Some(o), Some(n)) => {
                (lines, added, removed) = diff_lines(o, n, Kind::Modified)
            }
            (Kind::Modified | Kind::Deleted, None, _) => lines = plain_lines(new.as_deref()),
            _ => {}
        }
        if seen.is_folder() {
            let text = "Now a folder is at this path; it is not read, and revert is off while \
                        it is there.";
            lines.insert(0, note(text));
        } else if new.is_none() && record.kind != Kind::Deleted {
            let what = match seen.0 {
                None => "The file is gone.",
                Some(s) if s.stamp.mode & 0o170000 == 0o120000 => {
                    "Now a link; its target is not read."
                }
                Some(s) if s.stamp.mode & 0o170000 != 0o100000 => "Not a regular file; not read.",
                Some(_) => "Too large to show.",
            };
            lines.insert(0, note(what));
        }
        Ok(Diff {
            rel: record.rel.clone(),
            kind: record.kind,
            missing: record.missing,
            since: record.since,
            lines,
            added,
            removed,
            can_revert: record.can_revert(),
            seen,
            // The sides the lines above were cut from (an absent side has no lines).
            old_text: match record.kind {
                Kind::Created => None,
                _ => side_text(old.as_deref()),
            },
            new_text: match record.kind {
                Kind::Deleted => None,
                _ => side_text(new.as_deref()),
            },
        })
    }

    /// Keep: the record and its pre-image go, while the file is still the version `seen` (what
    /// the diff showed); that version is what later changes are compared with.
    pub fn keep(&self, rel: &Path, seen: &Seen) -> Result<(), KeepError> {
        let abs = {
            let st = self.shared.lock();
            st.record_at(rel).ok_or(KeepError::Gone)?;
            st.root.join(rel)
        };
        // Keep says "this version is fine": only the version that was shown can be meant.
        if !still(&abs, seen).map_err(|e| KeepError::Failed(e.to_string()))? {
            return Err(KeepError::Changed);
        }
        let running = {
            let mut st = self.shared.lock();
            let Some(i) = st.record_at(rel) else {
                return Err(KeepError::Gone);
            };
            let r = st.remove_record(i);
            if let Some(b) = &r.pre {
                st.drop_blob(b);
            }
            st.save_index();
            st.running
        };
        if running {
            Engine::rebase(&self.shared, rel, Expect::Shown(seen));
        }
        self.shared.bump();
        Ok(())
    }

    /// Revert (SEC-12): writes the pre-image back, or moves a created file to the trash, only
    /// while the file is still what `seen` describes. `seam` is called between the steps of
    /// the swap (tests place another writer there).
    pub fn revert(
        &self,
        rel: &Path,
        seen: &Seen,
        trash: &dyn vr_fs::Trash,
        seam: &mut dyn FnMut(Step),
    ) -> Result<Reverted, RevertError> {
        let fail = |e: io::Error| RevertError::Failed(e.to_string());
        let (root, record, bytes) = {
            let st = self.shared.lock();
            let i = st.record_at(rel).ok_or(RevertError::Gone)?;
            let r = st.records[i].clone();
            let bytes = match (&r.pre, r.kind) {
                (_, Kind::Created) => None,
                (Some(b), _) => Some(st.read_blob(b).map_err(fail)?),
                (None, _) => return Err(RevertError::NoEarlier),
            };
            (st.root.clone(), r, bytes)
        };
        if !valid_rel(rel) {
            return Err(RevertError::Outside);
        }
        if seen.is_folder() {
            // A folder cannot be compared with what was shown, and whatever is in it is
            // work nobody was shown: it is not exchanged, moved aside or trashed.
            return Err(RevertError::Folder);
        }
        let target = root.join(rel);
        let (Some(parent), Some(name)) = (target.parent(), target.file_name()) else {
            return Err(RevertError::Outside);
        };
        if record.kind == Kind::Deleted && !parent.exists() {
            // The folder went with the file: made again, below the nearest folder that is
            // still inside the root.
            let mut up = parent;
            while !up.exists() {
                up = up.parent().ok_or(RevertError::Outside)?;
            }
            if !fs::canonicalize(up).map_err(fail)?.starts_with(&root) {
                return Err(RevertError::Outside);
            }
            fs::create_dir_all(parent).map_err(fail)?;
        }
        let anchor = Anchor::open(parent, &root)?;
        let path = anchor.path(name);
        if !still(&path, seen).map_err(fail)? {
            return Err(RevertError::Changed);
        }
        let Some(bytes) = bytes else {
            // A created file: to the trash by its real path (the link itself when it is one),
            // never unlinked.
            if !anchor.inside(&root) {
                return Err(RevertError::Outside);
            }
            // The trash works by path and cannot be told which version to take. The file
            // that was checked is held open across the move, so a write or a replacement in
            // between does not pass for the version that was shown.
            if seen.0.is_none() {
                return Err(RevertError::Failed("the file is no longer there".into()));
            }
            let pin = Pin::open(&path, seen)
                .map_err(fail)?
                .ok_or(RevertError::Changed)?;
            let real = anchor.real_now().join(name);
            match trash.trash(std::slice::from_ref(&real)).pop() {
                Some((_, Ok(()))) => {}
                Some((_, Err(e))) => return Err(RevertError::Failed(e.to_string())),
                None => return Err(RevertError::Failed("the trash did not answer".into())),
            }
            let done = match pin.after(&path, seen) {
                Moved::No => {
                    // The trash moved something else or nothing: the folder may have been
                    // swapped for a link after its real path was read.
                    let text = "the trash reported success but the file is still in place; \
                                the trash may hold a different file from that path";
                    return Err(RevertError::Failed(text.into()));
                }
                Moved::Same => Reverted::Trashed,
                Moved::Written => Reverted::TrashedChanged,
                Moved::Unknown => Reverted::TrashedUnchecked,
            };
            let mut st = self.shared.lock();
            if let Some(i) = st.record_at(rel) {
                st.remove_record(i);
            }
            st.known.remove(rel);
            st.save_index();
            drop(st);
            self.shared.bump();
            return Ok(done);
        };
        let mode = record.pre.as_ref().map_or(0o600, |b| b.mode) & 0o777;
        // 0600 until the bytes are in; the final mode is set on the handle, whatever the umask.
        let (tmp, file) = vr_core::create_temp(&anchor.base(), 0o600).map_err(fail)?;
        let prepared = (|| {
            (&file).write_all(&bytes)?;
            file.set_permissions(fs::Permissions::from_mode(mode))?;
            file.sync_all()
        })();
        if let Err(e) = prepared {
            let _ = fs::remove_file(&tmp);
            return Err(fail(e));
        }
        let (len, hash) = (bytes.len() as u64, content_hash(&bytes));
        let mut outside = false;
        let (aside_name, mut aside) = (reverted_name(name), false);
        let link_like = seen.0.is_some_and(|s| s.stamp.mode & 0o170000 != 0o100000);
        let done = if link_like {
            // A link (or another non-file) is at the path: nothing can be written into it, so
            // it is taken out by the same atomic exchange and checked by what came out.
            if anchor.inside(&root) {
                swap_out(&tmp, &path, seen, (len, hash), seam)
            } else {
                let _ = fs::remove_file(&tmp);
                outside = true;
                Ok(Outcome::Changed)
            }
        } else if seen.0.is_none() {
            if anchor.inside(&root) {
                seam(Step::BeforeSwap);
                safe_replace::create_if_absent(&tmp, &path).map(|made| {
                    if made {
                        Outcome::Replaced
                    } else {
                        Outcome::Changed
                    }
                })
            } else {
                // Never offered to the swap: still our own unexposed file.
                let _ = fs::remove_file(&tmp);
                outside = true;
                Ok(Outcome::Changed)
            }
        } else {
            // A file that was at the path is not deleted (D18): the replaced version, or our
            // own file when it is taken back out because the path had changed. A program
            // that opened it there can still write through that handle after the file passed
            // its check, and no check sees that write: so the file gets a name of its own
            // right where the protocol would delete it (the delete then finds nothing), and
            // goes to the trash below, late writes included. Where the rename is refused the
            // protocol deletes it as every save does.
            let mut seam = |step: Step| {
                seam(step);
                if matches!(step, Step::BeforeDeleteOld | Step::BeforeDeleteOurs)
                    && rename_noreplace(&tmp, &anchor.path(aside_name.as_ref())).is_ok()
                {
                    aside = true;
                }
            };
            safe_replace::replace_if_unchanged(
                &tmp,
                &path,
                Checks {
                    // Asked right before the swap: the folder must still be inside the root.
                    target_unchanged: &mut |p| {
                        if !anchor.inside(&root) {
                            outside = true;
                            return Ok(false);
                        }
                        still(p, seen)
                    },
                    holds_expected: &mut |p| holds(p, seen),
                    holds_ours: &mut |p| {
                        Ok(fs::symlink_metadata(p)?.len() == len
                            && state_of(p)?.is_some_and(|s| s.hash == hash))
                    },
                },
                &mut safe_replace::exchange_files,
                &mut seam,
            )
        };
        match done {
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                Err(fail(e))
            }
            Ok(Outcome::Changed) => {
                if aside {
                    // Our own file, with the earlier version's bytes and whatever was
                    // written into it while it was at the path. Not taken by the trash, it
                    // is deleted as before: nothing of ours stays in the user's folder.
                    let at = anchor.path(aside_name.as_ref());
                    let _ = trash.trash(&[anchor.real_now().join(&aside_name)]);
                    if fs::symlink_metadata(&at).is_ok() {
                        let _ = fs::remove_file(&at);
                    }
                }
                Err(if outside {
                    RevertError::Outside
                } else {
                    RevertError::Changed
                })
            }
            Ok(Outcome::Kept { at, what, cause }) => {
                // Named by the folder's real path: the handle's path is no use to the user.
                let at = at
                    .file_name()
                    .map_or_else(|| at.clone(), |n| anchor.real_now().join(n));
                Err(RevertError::Failed(
                    safe_replace::kept_error(&at, what, &cause).to_string(),
                ))
            }
            Ok(Outcome::Replaced) => {
                // By its real path, as the trash works; a trash that did not take it leaves
                // it under its name, and the answer says where.
                let kept = aside
                    .then(|| {
                        let real = anchor.real_now().join(&aside_name);
                        let _ = trash.trash(std::slice::from_ref(&real));
                        fs::symlink_metadata(anchor.path(aside_name.as_ref()))
                            .is_ok()
                            .then_some(real)
                    })
                    .flatten();
                let _ = anchor.dir.sync_all();
                let stamp = file.metadata().ok().map(|m| Stamp::of(&m));
                let mut st = self.shared.lock();
                if let Some(i) = st.record_at(rel) {
                    let r = st.remove_record(i);
                    // The pre-image is the file's content again: it stays as what a later
                    // change is compared with, so this write is not taken for one.
                    match (r.pre, stamp, st.running) {
                        (Some(blob), Some(stamp), true) => {
                            st.known.insert(
                                rel.to_path_buf(),
                                Base {
                                    stamp,
                                    racy: true,
                                    blob: Some(blob),
                                    why: None,
                                },
                            );
                        }
                        (Some(blob), ..) => st.drop_blob(&blob),
                        _ => {}
                    }
                }
                st.save_index();
                drop(st);
                self.shared.bump();
                Ok(kept.map_or(Reverted::Restored, Reverted::RestoredOldKept))
            }
        }
    }
}

impl Drop for Changes {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        let mut st = self.shared.lock();
        st.closed = true;
        st.running = false;
        st.epoch += 1;
        let used = st.used;
        st.release(used);
        st.known.clear();
        st.records.clear();
        // The lock file is inside; the folder was made by this tracker under a generated name.
        let _ = fs::remove_dir_all(&st.dir);
    }
}

/// Waits until `cond` holds or `limit` passed.
#[cfg(test)]
pub fn wait_until(limit: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let end = std::time::Instant::now() + limit;
    while std::time::Instant::now() < end {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    cond()
}

/// Another writer placed between a decision and the read that follows it (this thread's next
/// re-read of a file as a new baseline).
#[cfg(test)]
pub mod seam {
    use std::cell::RefCell;

    thread_local! {
        static BEFORE_READ: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
    }

    pub fn before_baseline_read(f: impl FnOnce() + 'static) {
        BEFORE_READ.with(|s| *s.borrow_mut() = Some(Box::new(f)));
    }

    pub(super) fn run() {
        if let Some(f) = BEFORE_READ.with(|s| s.borrow_mut().take()) {
            f();
        }
    }
}
