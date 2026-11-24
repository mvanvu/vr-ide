//! A text file on disk: loading, encoding detection, atomic saving and disk-change tracking.
//!
//! Saving never writes into the existing file: the new content goes to a temp file in the same
//! directory, is fsynced, gets the original mode and owner, and is renamed over the target.
//! A save also refuses to replace content someone else wrote since we loaded or saved
//! (`DocError::ChangedOnDisk`); `save_overwrite` is the explicit "keep mine" path.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::hash::{DefaultHasher, Hasher};
use std::io::{self, BufWriter, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

use ropey::Rope;

use crate::text::Buffer;
use crate::transaction::{Change, Transaction};

/// Bytes scanned for NUL when deciding whether a file is binary.
pub const BINARY_SNIFF_LEN: usize = 8 * 1024;

const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";
const TEMP_PREFIX: &str = ".vr-ide-save-";
const TEMP_SUFFIX: &str = ".tmp";
// Same limit as the kernel's ELOOP.
const MAX_LINK_DEPTH: usize = 40;
/// Two seconds covers FAT's 2 s mtime resolution and coarse kernel/NFS clocks.
const RACY_WINDOW: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    Lf,
    CrLf,
    Cr,
}

impl LineEnding {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
            Self::Cr => "\r",
        }
    }

    /// The first line break in `s` decides; text without one is `Lf`.
    pub fn detect(s: &str) -> Self {
        let b = s.as_bytes();
        match b.iter().position(|&c| c == b'\n' || c == b'\r') {
            Some(i) if b[i] == b'\r' && b.get(i + 1) == Some(&b'\n') => Self::CrLf,
            Some(i) if b[i] == b'\r' => Self::Cr,
            _ => Self::Lf,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf8Bom,
    /// Not editable; the buffer holds a display-only decoding.
    Other(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiskState {
    InSync,
    ChangedOnDisk,
    DeletedOnDisk,
}

#[derive(Debug, thiserror::Error)]
pub enum DocError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("binary file")]
    Binary,
    #[error("permission denied: {0}")]
    PermissionDenied(PathBuf),
    #[error("file is not UTF-8 and is open read-only")]
    NotUtf8ReadOnly,
    #[error("file changed on disk since it was loaded")]
    ChangedOnDisk,
    #[error("document has no path")]
    NoPath,
}

/// Cheap identity of a file's state on disk. `ctime` is included because tools that restore
/// mtime (`cp -p`, `rsync -t`) cannot restore it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskStamp {
    len: u64,
    mtime: Option<SystemTime>,
    ctime: (i64, i64),
    dev: u64,
    ino: u64,
}

impl DiskStamp {
    /// Stamp of the file `path` points to (links followed); `None` when it does not exist.
    pub fn of(path: &Path) -> io::Result<Option<Self>> {
        match fs::metadata(path) {
            Ok(m) => Ok(Some(Self::from_meta(&m))),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    fn from_meta(m: &fs::Metadata) -> Self {
        Self {
            len: m.len(),
            mtime: m.modified().ok(),
            ctime: (m.ctime(), m.ctime_nsec()),
            dev: m.dev(),
            ino: m.ino(),
        }
    }
}

/// What we last read from or wrote to disk.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Snapshot {
    stamp: DiskStamp,
    hash: u64,
    /// Taken within `RACY_WINDOW` of the file's mtime: a same-tick rewrite on a coarse clock
    /// (FAT, NFS, jiffies) would keep the stamp, so the content is re-hashed instead.
    racy: bool,
}

impl Snapshot {
    fn new(stamp: DiskStamp, hash: u64) -> Self {
        let racy = stamp.mtime.is_none_or(|m| {
            SystemTime::now()
                .duration_since(m)
                .is_ok_and(|age| age < RACY_WINDOW)
                // An mtime in the future (clock skew) is treated as racy too.
                || SystemTime::now() < m
        });
        Self { stamp, hash, racy }
    }
}

/// Result of a successful save.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Saved {
    /// Other names (hard links) the old file had; they keep the old content (D46).
    pub other_hard_links: u64,
}

/// Saves a `Document` records here are not reported back by the `Watcher` that handed it out.
#[derive(Debug, Clone, Default)]
pub struct OwnWrites(Arc<Mutex<HashMap<PathBuf, Snapshot>>>);

impl OwnWrites {
    fn map(&self) -> MutexGuard<'_, HashMap<PathBuf, Snapshot>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn record(&self, resolved: PathBuf, snap: Snapshot) {
        self.map().insert(resolved, snap);
    }

    /// The file at `resolved` is still our most recent save and nothing has touched it since.
    /// Any write, chmod or replacement changes ctime or the inode, so the stamp must be the
    /// one we wrote (content is re-checked for a racy stamp). The first mismatch drops the
    /// entry: a later revert to the same bytes (git checkout, an agent undoing itself) is a
    /// new external change and must be reported.
    pub(crate) fn is_own(&self, resolved: &Path) -> bool {
        let Some(snap) = self.map().get(resolved).copied() else {
            return false;
        };
        // Hashing runs without the lock so a save on the UI thread never waits for it.
        let own = match DiskStamp::of(resolved) {
            Ok(Some(cur)) if cur == snap.stamp => {
                !snap.racy || hash_file(resolved).is_ok_and(|h| h == snap.hash)
            }
            _ => false,
        };
        if !own {
            let mut map = self.map();
            if map.get(resolved).is_some_and(|s| s.stamp == snap.stamp) {
                map.remove(resolved);
            }
        }
        own
    }

    /// Our save at `resolved` no longer describes the document (it was reloaded).
    pub(crate) fn forget(&self, resolved: &Path) {
        self.map().remove(resolved);
    }

    pub(crate) fn retain(&self, keep: impl Fn(&Path) -> bool) {
        self.map().retain(|p, _| keep(p));
    }
}

/// Whether the file at `path` (whose stamp is `cur`) still has the snapshot's content.
/// An equal stamp is trusted only for a settled snapshot; a racy one re-reads the file.
pub(crate) fn snapshot_matches(snap: &Snapshot, path: &Path, cur: DiskStamp) -> io::Result<bool> {
    if cur.len != snap.stamp.len {
        return Ok(false);
    }
    if snap.stamp == cur && !snap.racy {
        return Ok(true);
    }
    Ok(hash_file(path)? == snap.hash)
}

/// setuid/setgid must not survive on a file that now belongs to someone else (the saver).
fn restored_mode(mode: u32, owner_restored: bool) -> u32 {
    if owner_restored { mode } else { mode & !0o6000 }
}

/// Points in `write_atomic` where tests inject a concurrent writer.
#[cfg(test)]
pub(crate) mod hook {
    use std::cell::RefCell;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Stage {
        BeforeRename,
        AfterRename,
    }

    type Hook = (Stage, Box<dyn FnOnce()>);

    thread_local! {
        static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
    }

    pub(crate) fn set(stage: Stage, f: impl FnOnce() + 'static) {
        HOOK.with(|h| *h.borrow_mut() = Some((stage, Box::new(f))));
    }

    pub(crate) fn fire(stage: Stage) {
        let f = HOOK.with(|h| {
            let mut h = h.borrow_mut();
            match h.take() {
                Some((s, f)) if s == stage => Some(f),
                other => {
                    *h = other;
                    None
                }
            }
        });
        if let Some(f) = f {
            f();
        }
    }
}

#[cfg(not(test))]
mod hook {
    #[derive(Clone, Copy)]
    pub(crate) enum Stage {
        BeforeRename,
        AfterRename,
    }

    #[inline(always)]
    pub(crate) fn fire(_stage: Stage) {}
}

#[derive(Debug)]
pub struct Document {
    pub path: Option<PathBuf>,
    pub buffer: Buffer,
    pub line_ending: LineEnding,
    pub encoding: Encoding,
    pub read_only: bool,
    pub disk_state: DiskState,
    /// False when the file was larger than the highlight limit given to `open`.
    pub highlight: bool,
    highlight_limit: u64,
    saved_revision: u64,
    disk: Option<Snapshot>,
    own_writes: Option<OwnWrites>,
}

impl Document {
    pub fn untitled() -> Self {
        let buffer = Buffer::default();
        Self {
            path: None,
            saved_revision: buffer.revision(),
            buffer,
            line_ending: LineEnding::Lf,
            encoding: Encoding::Utf8,
            read_only: false,
            disk_state: DiskState::InSync,
            highlight: true,
            highlight_limit: u64::MAX,
            disk: None,
            own_writes: None,
        }
    }

    pub fn open(path: &Path, max_size_highlight: u64) -> Result<Self, DocError> {
        let loaded = load(path)?;
        let buffer = Buffer::from_str(&loaded.text);
        Ok(Self {
            path: Some(path.to_path_buf()),
            saved_revision: buffer.revision(),
            buffer,
            line_ending: LineEnding::detect(&loaded.text),
            encoding: loaded.encoding,
            read_only: loaded.read_only,
            disk_state: DiskState::InSync,
            highlight: loaded.snapshot.stamp.len <= max_size_highlight,
            highlight_limit: max_size_highlight,
            disk: Some(loaded.snapshot),
            own_writes: None,
        })
    }

    /// Compares undo revisions (D26), so editing and undoing back reads as unmodified.
    pub fn is_modified(&self) -> bool {
        self.buffer.revision() != self.saved_revision
    }

    /// Saves to `path`; `Err(ChangedOnDisk)` when someone else changed the file since we
    /// loaded or saved it (the UI then asks and may call `save_overwrite`).
    pub fn save(&mut self) -> Result<Saved, DocError> {
        self.save_to_own_path(true)
    }

    /// Saves even when the file changed on disk (the user chose to keep their version).
    pub fn save_overwrite(&mut self) -> Result<Saved, DocError> {
        self.save_to_own_path(false)
    }

    /// Writes to `p` (the UI confirms replacing an existing file) and makes it the document path.
    pub fn save_as(&mut self, p: &Path) -> Result<Saved, DocError> {
        if self.read_only {
            return Err(DocError::NotUtf8ReadOnly);
        }
        // The user confirmed replacing whatever is at `p`.
        let saved = self.write_to(p, None)?;
        self.path = Some(p.to_path_buf());
        Ok(saved)
    }

    /// Replaces the text with the file's content as one undo step, keeping the cursor
    /// clamped; the document is unmodified afterwards. A failed load leaves the buffer as is.
    pub fn reload(&mut self) -> Result<(), DocError> {
        let path = self.path.clone().ok_or(DocError::NoPath)?;
        let loaded = match load(&path) {
            Err(DocError::Io(e)) if e.kind() == io::ErrorKind::NotFound => {
                self.disk_state = DiskState::DeletedOnDisk;
                return Err(DocError::Io(e));
            }
            r => r?,
        };
        if let Some(own) = &self.own_writes
            && let Ok(resolved) = fs::canonicalize(&path)
        {
            own.forget(&resolved);
        }
        if *self.buffer.text() != loaded.text.as_str() {
            let sel = self.buffer.selection().clone();
            let len = self.buffer.text().len_chars();
            self.buffer.break_undo_group();
            self.buffer.apply(
                Transaction::new(vec![Change::new(0, len, loaded.text.as_str())])
                    .with_selection(sel),
            );
            self.buffer.break_undo_group();
        }
        self.line_ending = LineEnding::detect(&loaded.text);
        self.encoding = loaded.encoding;
        self.read_only = loaded.read_only;
        self.highlight = loaded.snapshot.stamp.len <= self.highlight_limit;
        self.saved_revision = self.buffer.revision();
        self.disk = Some(loaded.snapshot);
        self.disk_state = DiskState::InSync;
        Ok(())
    }

    /// Compares the file on disk with what we last loaded or saved and updates `disk_state`.
    /// Reads the file only when its stamp changed (to ignore touches without content changes).
    /// An I/O error keeps the previous state.
    pub fn check_disk(&mut self) -> DiskState {
        let Some(path) = self.path.clone() else {
            return self.disk_state;
        };
        let state = match DiskStamp::of(&path) {
            Ok(None) if self.disk.is_some() => DiskState::DeletedOnDisk,
            Ok(None) => DiskState::InSync,
            Ok(Some(cur)) => match self.changed_since_snapshot(&path, cur) {
                Ok(true) => DiskState::ChangedOnDisk,
                Ok(false) => DiskState::InSync,
                Err(_) => return self.disk_state,
            },
            Err(_) => return self.disk_state,
        };
        self.disk_state = state;
        state
    }

    /// Stamp of the file as we last loaded or saved it.
    pub fn disk_stamp(&self) -> Option<DiskStamp> {
        self.disk.map(|s| s.stamp)
    }

    /// Registers this document's saves with a `Watcher` (`Watcher::own_writes`).
    pub fn set_own_writes(&mut self, w: OwnWrites) {
        self.own_writes = Some(w);
    }

    fn save_to_own_path(&mut self, check: bool) -> Result<Saved, DocError> {
        if self.read_only {
            return Err(DocError::NotUtf8ReadOnly);
        }
        let path = self.path.clone().ok_or(DocError::NoPath)?;
        let expect = if check {
            let cur = DiskStamp::of(&path).map_err(|e| io_error(e, &path))?;
            let changed = match cur {
                // Recreating a deleted file overwrites nobody's content.
                None => false,
                Some(cur) => self.changed_since_snapshot(&path, cur)?,
            };
            if changed {
                self.disk_state = DiskState::ChangedOnDisk;
                return Err(DocError::ChangedOnDisk);
            }
            Some(Expected {
                stamp: cur,
                hash: self.disk.map(|s| s.hash),
            })
        } else {
            None
        };
        self.write_to(&path, expect)
    }

    fn changed_since_snapshot(&mut self, path: &Path, cur: DiskStamp) -> Result<bool, DocError> {
        let Some(snap) = self.disk else {
            // A file appeared where we never read or wrote one.
            return Ok(true);
        };
        let same = snapshot_matches(&snap, path, cur).map_err(|e| io_error(e, path))?;
        // Refreshing also lets a racy snapshot settle once its mtime is old enough.
        if same && (snap.stamp != cur || snap.racy) {
            self.disk = Some(Snapshot::new(cur, snap.hash));
        }
        Ok(!same)
    }

    fn write_to(&mut self, path: &Path, expect: Option<Expected>) -> Result<Saved, DocError> {
        let target = resolve_links(path).map_err(|e| io_error(e, path))?;
        let bom = self.encoding == Encoding::Utf8Bom;
        let written = write_atomic(&target, bom, self.buffer.text(), expect)
            .map_err(|e| io_error(e, path))?;
        let (snapshot, saved) = match written {
            Written::Done { snapshot, saved } => (snapshot, saved),
            Written::ChangedOnDisk => {
                self.disk_state = DiskState::ChangedOnDisk;
                return Err(DocError::ChangedOnDisk);
            }
        };
        // Typing after a save starts a new undo group, so undo can return to the saved text.
        self.buffer.break_undo_group();
        self.saved_revision = self.buffer.revision();
        self.disk = Some(snapshot);
        self.disk_state = DiskState::InSync;
        if let Some(own) = &self.own_writes
            && let Ok(resolved) = fs::canonicalize(&target)
        {
            own.record(resolved, snapshot);
        }
        Ok(saved)
    }
}

/// True for the temp files `save` writes next to the target before renaming them over it.
pub fn is_save_temp(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with(TEMP_PREFIX) && n.ends_with(TEMP_SUFFIX))
}

fn io_error(e: io::Error, path: &Path) -> DocError {
    if e.kind() == io::ErrorKind::PermissionDenied {
        DocError::PermissionDenied(path.to_path_buf())
    } else {
        DocError::Io(e)
    }
}

struct Loaded {
    text: String,
    encoding: Encoding,
    read_only: bool,
    snapshot: Snapshot,
}

fn load(path: &Path) -> Result<Loaded, DocError> {
    let mut file = File::open(path).map_err(|e| io_error(e, path))?;
    // Stamp before reading: a write racing the read then shows up as a changed stamp and hash.
    let meta = file.metadata().map_err(|e| io_error(e, path))?;
    let stamp = DiskStamp::from_meta(&meta);
    let mut bytes = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(0));
    file.read_to_end(&mut bytes)
        .map_err(|e| io_error(e, path))?;
    let hash = content_hash(&bytes);
    let (text, encoding) = decode(bytes)?;
    Ok(Loaded {
        read_only: matches!(encoding, Encoding::Other(_)),
        text,
        encoding,
        snapshot: Snapshot::new(stamp, hash),
    })
}

fn decode(mut bytes: Vec<u8>) -> Result<(String, Encoding), DocError> {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return Ok((
            decode_utf16(rest, u16::from_le_bytes),
            Encoding::Other("UTF-16LE".into()),
        ));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return Ok((
            decode_utf16(rest, u16::from_be_bytes),
            Encoding::Other("UTF-16BE".into()),
        ));
    }
    if bytes[..bytes.len().min(BINARY_SNIFF_LEN)].contains(&0) {
        return Err(DocError::Binary);
    }
    let encoding = if bytes.starts_with(UTF8_BOM) {
        bytes.drain(..UTF8_BOM.len());
        Encoding::Utf8Bom
    } else {
        Encoding::Utf8
    };
    match String::from_utf8(bytes) {
        Ok(s) => Ok((s, encoding)),
        // Shown as Latin-1 (every byte maps to a char) and never written back.
        Err(e) => Ok((
            e.into_bytes().iter().map(|&b| char::from(b)).collect(),
            Encoding::Other("non-UTF-8".into()),
        )),
    }
}

fn decode_utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> String {
    // A dangling odd byte cannot form a code unit and is dropped.
    let (pairs, _) = bytes.as_chunks::<2>();
    let units = pairs.iter().map(|&c| unit(c));
    char::decode_utf16(units)
        .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

fn content_hash(bytes: &[u8]) -> u64 {
    let mut h = DefaultHasher::new();
    h.write(bytes);
    h.finish()
}

fn hash_file(path: &Path) -> io::Result<u64> {
    let mut file = File::open(path)?;
    let mut h = DefaultHasher::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = match file.read(&mut buf) {
            Ok(0) => return Ok(h.finish()),
            Ok(n) => n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        h.write(&buf[..n]);
    }
}

/// Follows symlinks at `path` so a save writes through to the link target and keeps the
/// link. A missing final target (dangling link) resolves to that target, which is created.
fn resolve_links(path: &Path) -> io::Result<PathBuf> {
    let mut p = path.to_path_buf();
    for _ in 0..MAX_LINK_DEPTH {
        match fs::symlink_metadata(&p) {
            Ok(m) if m.file_type().is_symlink() => {
                let link = fs::read_link(&p)?;
                p = match p.parent() {
                    Some(dir) if link.is_relative() => dir.join(link),
                    _ => link,
                };
            }
            Ok(_) => return Ok(p),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(p),
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("too many levels of symbolic links"))
}

/// Removes the temp file unless the rename consumed it.
struct TempGuard(Option<PathBuf>);

impl Drop for TempGuard {
    fn drop(&mut self) {
        if let Some(p) = self.0.take() {
            let _ = fs::remove_file(p);
        }
    }
}

fn create_temp(dir: &Path, mode: u32) -> io::Result<(PathBuf, File)> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    for _ in 0..100 {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = dir.join(format!(
            "{TEMP_PREFIX}{}-{n}{TEMP_SUFFIX}",
            std::process::id()
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&tmp)
        {
            Ok(f) => return Ok((tmp, f)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free temp file name for saving",
    ))
}

/// What the caller's changed-on-disk check saw: the target's stamp (`None`: absent) and the
/// hash of the content it holds.
struct Expected {
    stamp: Option<DiskStamp>,
    hash: Option<u64>,
}

/// The target still holds what the caller's check saw. A different stamp with the same
/// length may be a metadata-only change (chmod, xattr by a sync or backup tool): the content
/// decides, and the stamp must not move again while it is read.
fn still_fresh(target: &Path, expected: &Expected) -> io::Result<bool> {
    let cur = DiskStamp::of(target)?;
    if cur == expected.stamp {
        return Ok(true);
    }
    match (cur, expected.stamp, expected.hash) {
        (Some(c), Some(s), Some(h)) if c.len == s.len => {
            Ok(hash_file(target)? == h && DiskStamp::of(target)? == Some(c))
        }
        _ => Ok(false),
    }
}

/// What `write_atomic` did.
enum Written {
    Done {
        snapshot: Snapshot,
        saved: Saved,
    },
    /// The target no longer had the expected stamp right before the rename; nothing written.
    ChangedOnDisk,
}

/// Temp file in the target's directory → write → owner and mode → fsync → freshness re-check
/// → rename → fsync dir. The target's data is never written, so a crash leaves either the old
/// or the new file. `expect`: the target's stamp (or absence) seen by the caller's
/// changed-on-disk check; `None` skips the re-check (overwrite chosen by the user).
fn write_atomic(
    target: &Path,
    bom: bool,
    text: &Rope,
    expect: Option<Expected>,
) -> io::Result<Written> {
    let dir = match target.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let existing = match fs::metadata(target) {
        Ok(m) => Some(m),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    if let Some(m) = &existing {
        // A directory, FIFO or device is never replaced (and opening a FIFO would block).
        if !m.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a regular file",
            ));
        }
        // rename() checks only the directory's permissions, so ask the kernel whether we may
        // write the file itself (mode bits, owner, ACLs, chattr +i). Opened without truncate
        // and closed without writing: the file is untouched.
        OpenOptions::new().write(true).open(target)?;
    }
    let other_hard_links = existing.as_ref().map_or(0, |m| m.nlink().saturating_sub(1));
    let mode = existing.as_ref().map_or(0o666, |m| m.mode() & 0o7777);
    let (tmp, file) = create_temp(&dir, mode)?;
    let mut guard = TempGuard(Some(tmp.clone()));

    let mut hasher = DefaultHasher::new();
    let mut w = BufWriter::with_capacity(64 * 1024, &file);
    if bom {
        w.write_all(UTF8_BOM)?;
        hasher.write(UTF8_BOM);
    }
    for chunk in text.chunks() {
        w.write_all(chunk.as_bytes())?;
        hasher.write(chunk.as_bytes());
    }
    w.flush()?;
    drop(w);

    if let Some(m) = &existing {
        let own = file.metadata()?;
        let mut owner_restored = own.uid() == m.uid() && own.gid() == m.gid();
        if !owner_restored {
            // Only root may give a file away; a plain user keeps the file as their own.
            match std::os::unix::fs::fchown(&file, Some(m.uid()), Some(m.gid())) {
                Ok(()) => owner_restored = true,
                Err(e) => {
                    tracing::warn!(path = %target.display(), error = %e, "save could not keep the owner");
                }
            }
        }
        // After chown (which clears setuid/setgid) and regardless of the umask.
        let mode = restored_mode(m.mode() & 0o7777, owner_restored);
        file.set_permissions(fs::Permissions::from_mode(mode))?;
    }
    file.sync_all()?;
    hook::fire(hook::Stage::BeforeRename);
    if let Some(expected) = &expect {
        // Last freshness check, as close to rename(2) as Linux allows: the write and fsync
        // above can take seconds, a stat takes microseconds. A writer landing between this stat
        // and the rename is still replaced; Linux has no compare-and-swap rename (D25).
        if !still_fresh(target, expected)? {
            return Ok(Written::ChangedOnDisk);
        }
    }
    fs::rename(&tmp, target)?;
    guard.0 = None;
    hook::fire(hook::Stage::AfterRename);
    // From our own handle: rename changed the inode's ctime. Someone may already have written
    // to the new file since the rename, so the snapshot is racy: the next comparison re-reads
    // the content instead of trusting this stamp.
    let stamp = DiskStamp::from_meta(&file.metadata()?);
    if let Err(e) = File::open(&dir).and_then(|d| d.sync_all()) {
        tracing::warn!(dir = %dir.display(), error = %e, "fsync of the directory failed after save");
    }
    Ok(Written::Done {
        snapshot: Snapshot {
            stamp,
            hash: hasher.finish(),
            racy: true,
        },
        saved: Saved { other_hard_links },
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::time::{Duration, Instant};

    use super::*;
    use crate::motion;
    use crate::selection::Selection;

    fn dir_names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    fn type_at_end(doc: &mut Document, s: &str) {
        motion::doc_end(&mut doc.buffer, false);
        doc.buffer.insert_text(s);
    }

    fn running_as_root() -> bool {
        // Root bypasses permission bits, so permission tests cannot fail as intended.
        fs::metadata("/proc/self")
            .map(|m| std::os::unix::fs::MetadataExt::uid(&m) == 0)
            .unwrap_or(false)
    }

    #[test]
    fn open_utf8_lf() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, "hello\nxin chào\n").unwrap();
        let doc = Document::open(&p, 1 << 20).unwrap();
        assert_eq!(doc.buffer.text().to_string(), "hello\nxin chào\n");
        assert_eq!(doc.line_ending, LineEnding::Lf);
        assert_eq!(doc.encoding, Encoding::Utf8);
        assert!(!doc.read_only && !doc.is_modified() && doc.highlight);
        assert_eq!(doc.disk_state, DiskState::InSync);
        assert_eq!(doc.path.as_deref(), Some(p.as_path()));
    }

    #[test]
    fn open_detects_line_ending_and_highlight_limit() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("w.txt");
        fs::write(&p, "a\r\nb\n").unwrap();
        let doc = Document::open(&p, 3).unwrap();
        assert_eq!(doc.line_ending, LineEnding::CrLf, "first break wins");
        assert!(!doc.highlight, "5 bytes > 3-byte highlight limit");
        fs::write(&p, "a\rb").unwrap();
        assert_eq!(Document::open(&p, 10).unwrap().line_ending, LineEnding::Cr);
    }

    #[test]
    fn open_non_utf8_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("latin1.txt");
        let bytes = b"caf\xe9 cr\xe8me\n".to_vec();
        fs::write(&p, &bytes).unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        assert!(doc.read_only);
        assert!(matches!(doc.encoding, Encoding::Other(_)));
        assert_eq!(doc.buffer.text().to_string(), "café crème\n");
        type_at_end(&mut doc, "x");
        assert!(matches!(doc.save(), Err(DocError::NotUtf8ReadOnly)));
        assert!(matches!(
            doc.save_overwrite(),
            Err(DocError::NotUtf8ReadOnly)
        ));
        let other = dir.path().join("copy.txt");
        assert!(matches!(
            doc.save_as(&other),
            Err(DocError::NotUtf8ReadOnly)
        ));
        assert_eq!(fs::read(&p).unwrap(), bytes, "original bytes untouched");
        assert!(!other.exists());
    }

    #[test]
    fn open_utf16_bom_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("u16.txt");
        let mut bytes = vec![0xFF, 0xFE];
        for u in "hi\n".encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        fs::write(&p, &bytes).unwrap();
        let doc = Document::open(&p, 1 << 20).unwrap();
        assert!(doc.read_only);
        assert_eq!(doc.encoding, Encoding::Other("UTF-16LE".into()));
        assert_eq!(doc.buffer.text().to_string(), "hi\n");
    }

    #[test]
    fn open_binary_refused() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.bin");
        fs::write(&p, b"\x7fELF\x02\x01\x00\x00abc").unwrap();
        assert!(matches!(Document::open(&p, 1 << 20), Err(DocError::Binary)));
        // A NUL past the sniff window is not looked at.
        let mut late = vec![b'a'; BINARY_SNIFF_LEN];
        late.push(0);
        fs::write(&p, &late).unwrap();
        assert!(Document::open(&p, 1 << 20).is_ok());
    }

    #[test]
    fn open_missing_and_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("nope.txt");
        match Document::open(&p, 0) {
            Err(DocError::Io(e)) => assert_eq!(e.kind(), io::ErrorKind::NotFound),
            other => panic!("expected NotFound, got {other:?}"),
        }
        if running_as_root() {
            return;
        }
        fs::write(&p, "secret").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o000)).unwrap();
        match Document::open(&p, 0) {
            Err(DocError::PermissionDenied(q)) => assert_eq!(q, p),
            other => panic!("expected PermissionDenied, got {other:?}"),
        }
    }

    #[test]
    fn save_keeps_crlf() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("crlf.txt");
        // Mixed endings are written back exactly; new lines typed via Enter use the file's break.
        fs::write(&p, "one\r\ntwo\nthree\r\n").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        assert_eq!(doc.line_ending, LineEnding::CrLf);
        type_at_end(&mut doc, "four");
        doc.save().unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"one\r\ntwo\nthree\r\nfour");
    }

    #[test]
    fn save_keeps_missing_trailing_newline() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("nonl.txt");
        fs::write(&p, "a\nb").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        type_at_end(&mut doc, "c");
        doc.save().unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"a\nbc");
    }

    #[test]
    fn save_keeps_bom() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("bom.txt");
        fs::write(&p, b"\xEF\xBB\xBFhi\n").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        assert_eq!(doc.encoding, Encoding::Utf8Bom);
        assert_eq!(
            doc.buffer.text().to_string(),
            "hi\n",
            "BOM is not buffer text"
        );
        type_at_end(&mut doc, "x");
        doc.save().unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"\xEF\xBB\xBFhi\nx");
    }

    #[test]
    fn save_atomic_keeps_mode() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("run.sh");
        fs::write(&p, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        let ino_before = std::os::unix::fs::MetadataExt::ino(&fs::metadata(&p).unwrap());
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        type_at_end(&mut doc, "echo hi\n");
        doc.save().unwrap();
        let meta = fs::metadata(&p).unwrap();
        assert_eq!(meta.permissions().mode() & 0o7777, 0o755);
        assert_ne!(
            std::os::unix::fs::MetadataExt::ino(&meta),
            ino_before,
            "written to a new inode and renamed over the old one"
        );
        assert_eq!(fs::read_to_string(&p).unwrap(), "#!/bin/sh\necho hi\n");
        assert_eq!(dir_names(dir.path()), vec!["run.sh"], "no temp file left");
        assert!(!doc.is_modified());
        assert_eq!(doc.disk_state, DiskState::InSync);
    }

    #[test]
    fn save_permission_denied() {
        if running_as_root() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("ro");
        fs::create_dir(&sub).unwrap();
        let p = sub.join("f.txt");
        fs::write(&p, "keep").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        type_at_end(&mut doc, "!");
        fs::set_permissions(&sub, fs::Permissions::from_mode(0o555)).unwrap();
        let r = doc.save();
        fs::set_permissions(&sub, fs::Permissions::from_mode(0o755)).unwrap();
        match r {
            Err(DocError::PermissionDenied(q)) => assert_eq!(q, p),
            other => panic!("expected PermissionDenied, got {other:?}"),
        }
        assert_eq!(fs::read_to_string(&p).unwrap(), "keep");
        assert_eq!(dir_names(&sub), vec!["f.txt"]);
        assert!(doc.is_modified(), "failed save keeps the modified marker");
    }

    #[test]
    fn save_refuses_read_only_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("ro.txt");
        fs::write(&p, "keep").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o444)).unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        type_at_end(&mut doc, "!");
        // The directory is writable, so only an explicit check stops rename from replacing it.
        assert!(matches!(doc.save(), Err(DocError::PermissionDenied(_))));
        assert!(matches!(
            doc.save_overwrite(),
            Err(DocError::PermissionDenied(_))
        ));
        assert_eq!(fs::read_to_string(&p).unwrap(), "keep");
        assert_eq!(dir_names(dir.path()), vec!["ro.txt"]);
    }

    #[test]
    fn save_refuses_non_regular_target() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("pipe");
        let ok = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            return;
        }
        let mut doc = Document::untitled();
        doc.buffer.insert_text("x");
        // Opening a FIFO for writing would block without a reader; it must fail fast instead.
        assert!(matches!(doc.save_as(&fifo), Err(DocError::Io(_))));
        assert_eq!(dir_names(dir.path()), vec!["pipe"]);
    }

    #[test]
    fn failed_rename_removes_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, "x").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        type_at_end(&mut doc, "y");
        let swap = p.clone();
        // After the temp file is written, the target turns into a non-empty directory, so
        // rename(2) fails.
        hook::set(hook::Stage::BeforeRename, move || {
            fs::remove_file(&swap).unwrap();
            fs::create_dir(&swap).unwrap();
            fs::write(swap.join("child"), "c").unwrap();
        });
        assert!(matches!(doc.save_overwrite(), Err(DocError::Io(_))));
        assert_eq!(
            dir_names(dir.path()),
            vec!["a.txt"],
            "only the directory remains"
        );
        assert!(p.is_dir() && doc.is_modified());
    }

    #[test]
    fn save_failure_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, "x").unwrap();
        let target = dir.path().join("taken");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("child"), "c").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        // A directory is never replaced; refused before any temp file exists.
        assert!(doc.save_as(&target).is_err());
        assert_eq!(dir_names(dir.path()), vec!["a.txt", "taken"]);
        assert_eq!(
            doc.path.as_deref(),
            Some(p.as_path()),
            "path unchanged on failure"
        );
    }

    #[test]
    fn save_through_symlink_keeps_link() {
        let dir = tempfile::tempdir().unwrap();
        let real_dir = dir.path().join("real");
        fs::create_dir(&real_dir).unwrap();
        let target = real_dir.join("t.txt");
        fs::write(&target, "a\n").unwrap();
        let link = dir.path().join("link.txt");
        symlink("real/t.txt", &link).unwrap();
        let mut doc = Document::open(&link, 1 << 20).unwrap();
        type_at_end(&mut doc, "b");
        doc.save().unwrap();
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "a\nb");
        assert_eq!(dir_names(&real_dir), vec!["t.txt"]);
        assert_eq!(dir_names(dir.path()), vec!["link.txt", "real"]);
    }

    #[test]
    fn save_refuses_when_changed_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, "mine\n").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        type_at_end(&mut doc, "edit");
        fs::write(&p, "written by an agent\n").unwrap();
        assert!(matches!(doc.save(), Err(DocError::ChangedOnDisk)));
        assert_eq!(doc.disk_state, DiskState::ChangedOnDisk);
        assert_eq!(fs::read_to_string(&p).unwrap(), "written by an agent\n");
        assert!(doc.is_modified());
        doc.save_overwrite().unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "mine\nedit");
        assert_eq!(doc.disk_state, DiskState::InSync);
        // After our own save the next plain save goes through.
        type_at_end(&mut doc, "!");
        doc.save().unwrap();
    }

    #[test]
    fn save_ignores_touch_without_content_change() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, "same\n").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        fs::write(&p, "same\n").unwrap();
        type_at_end(&mut doc, "x");
        doc.save().unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "same\nx");
    }

    #[test]
    fn save_recreates_file_deleted_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, "a").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        fs::remove_file(&p).unwrap();
        assert_eq!(doc.check_disk(), DiskState::DeletedOnDisk);
        assert_eq!(doc.buffer.text().to_string(), "a", "buffer kept");
        doc.save().unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "a");
        assert_eq!(doc.disk_state, DiskState::InSync);
    }

    #[test]
    fn save_untitled_and_save_as() {
        let dir = tempfile::tempdir().unwrap();
        let mut doc = Document::untitled();
        assert!(doc.path.is_none() && !doc.is_modified());
        doc.buffer.insert_text("new");
        assert!(doc.is_modified());
        assert!(matches!(doc.save(), Err(DocError::NoPath)));
        let p = dir.path().join("new.txt");
        doc.save_as(&p).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "new");
        assert_eq!(doc.path.as_deref(), Some(p.as_path()));
        assert!(!doc.is_modified());
        // An untitled buffer pointed at a file someone else created must not clobber it.
        let q = dir.path().join("exists.txt");
        fs::write(&q, "theirs").unwrap();
        let mut other = Document::untitled();
        other.path = Some(q.clone());
        other.buffer.insert_text("mine");
        assert!(matches!(other.save(), Err(DocError::ChangedOnDisk)));
        assert_eq!(fs::read_to_string(&q).unwrap(), "theirs");
    }

    #[test]
    fn is_modified_tracks_saved_version() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, "ab").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        type_at_end(&mut doc, "c");
        assert!(doc.is_modified());
        doc.buffer.undo();
        assert!(!doc.is_modified(), "edit + undo back = not modified");
        doc.buffer.redo();
        assert!(doc.is_modified());
        doc.save().unwrap();
        assert!(!doc.is_modified());
        // Typing right after a save starts a new undo group, so undo returns to "abc" exactly.
        doc.buffer.insert_text("d");
        assert!(doc.is_modified());
        doc.buffer.undo();
        assert_eq!(doc.buffer.text().to_string(), "abc");
        assert!(!doc.is_modified());
        doc.buffer.undo();
        assert!(doc.is_modified(), "undo past the save point is modified");
    }

    #[test]
    fn reload_replaces_text_keeps_cursor_clamped() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, "0123456789").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        doc.buffer.set_selection(Selection::point(8));
        fs::write(&p, "abc\r\n").unwrap();
        assert_eq!(doc.check_disk(), DiskState::ChangedOnDisk);
        doc.reload().unwrap();
        assert_eq!(doc.buffer.text().to_string(), "abc\r\n");
        assert_eq!(doc.buffer.selection(), &Selection::point(5));
        assert_eq!(doc.line_ending, LineEnding::CrLf);
        assert!(!doc.is_modified());
        assert_eq!(doc.disk_state, DiskState::InSync);
        // The external rewrite is one undo step.
        doc.buffer.undo();
        assert_eq!(doc.buffer.text().to_string(), "0123456789");
        assert!(doc.is_modified());
        // A reload that turns the file binary leaves the buffer alone.
        fs::write(&p, b"\x00\x01").unwrap();
        assert!(matches!(doc.reload(), Err(DocError::Binary)));
        assert_eq!(doc.buffer.text().to_string(), "0123456789");
    }

    #[test]
    fn check_disk_states() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, "a").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        assert_eq!(doc.check_disk(), DiskState::InSync);
        type_at_end(&mut doc, "b");
        doc.save().unwrap();
        assert_eq!(doc.check_disk(), DiskState::InSync, "our own save");
        fs::write(&p, "other").unwrap();
        assert_eq!(doc.check_disk(), DiskState::ChangedOnDisk);
        fs::remove_file(&p).unwrap();
        assert_eq!(doc.check_disk(), DiskState::DeletedOnDisk);
        assert_eq!(doc.disk_state, DiskState::DeletedOnDisk);
        assert_eq!(Document::untitled().check_disk(), DiskState::InSync);
    }

    #[test]
    fn save_temp_names_are_recognised() {
        assert!(!is_save_temp(Path::new("/x/a.txt")));
        assert!(!is_save_temp(Path::new("/x/.hidden")));
        assert!(is_save_temp(Path::new("/x/.vr-ide-save-12-3.tmp")));
    }

    #[test]
    fn save_refuses_file_user_cannot_write() {
        if running_as_root() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.txt");
        fs::write(&p, "keep").unwrap();
        // Group may write, the owner (us) may not: not `readonly()`, yet not writable by us.
        fs::set_permissions(&p, fs::Permissions::from_mode(0o464)).unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        type_at_end(&mut doc, "!");
        assert!(matches!(doc.save(), Err(DocError::PermissionDenied(_))));
        assert!(matches!(
            doc.save_overwrite(),
            Err(DocError::PermissionDenied(_))
        ));
        assert_eq!(fs::read_to_string(&p).unwrap(), "keep");
        assert_eq!(dir_names(dir.path()), vec!["f.txt"]);
    }

    #[test]
    fn restored_mode_drops_setid_bits_when_owner_not_restored() {
        assert_eq!(restored_mode(0o6755, true), 0o6755);
        assert_eq!(restored_mode(0o6755, false), 0o0755);
        assert_eq!(
            restored_mode(0o1777, false),
            0o1777,
            "sticky bit is harmless"
        );
    }

    #[test]
    fn metadata_change_during_save_is_not_a_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, "mine\n").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        type_at_end(&mut doc, "edit");
        let tool = p.clone();
        // A backup/sync tool touches only metadata (ctime changes, content does not).
        hook::set(hook::Stage::BeforeRename, move || {
            fs::set_permissions(&tool, fs::Permissions::from_mode(0o640)).unwrap();
        });
        doc.save().unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "mine\nedit");
        assert_eq!(dir_names(dir.path()), vec!["a.txt"]);
    }

    #[test]
    fn save_refuses_write_racing_the_temp_write() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, "mine\n").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        type_at_end(&mut doc, "edit");
        let agent = p.clone();
        // The agent writes while our temp file is being written and fsynced.
        hook::set(hook::Stage::BeforeRename, move || {
            fs::write(&agent, "agent\n").unwrap();
        });
        assert!(matches!(doc.save(), Err(DocError::ChangedOnDisk)));
        assert_eq!(fs::read_to_string(&p).unwrap(), "agent\n");
        assert_eq!(dir_names(dir.path()), vec!["a.txt"], "temp file removed");
        assert!(doc.is_modified());
        assert_eq!(doc.disk_state, DiskState::ChangedOnDisk);
    }

    #[test]
    fn save_refuses_file_created_while_saving_deleted_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, "mine").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        fs::remove_file(&p).unwrap();
        let agent = p.clone();
        hook::set(hook::Stage::BeforeRename, move || {
            fs::write(&agent, "agent").unwrap();
        });
        assert!(matches!(doc.save(), Err(DocError::ChangedOnDisk)));
        assert_eq!(fs::read_to_string(&p).unwrap(), "agent");
    }

    #[test]
    fn write_right_after_rename_is_not_counted_as_ours() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, "a").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        type_at_end(&mut doc, "b");
        let agent = p.clone();
        // Same-length in-place rewrite of our freshly renamed inode.
        hook::set(hook::Stage::AfterRename, move || {
            let mut f = OpenOptions::new().write(true).open(&agent).unwrap();
            f.write_all(b"XY").unwrap();
        });
        doc.save().unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "XY");
        assert_eq!(doc.check_disk(), DiskState::ChangedOnDisk);
        type_at_end(&mut doc, "c");
        assert!(matches!(doc.save(), Err(DocError::ChangedOnDisk)));
        assert_eq!(fs::read_to_string(&p).unwrap(), "XY");
    }

    #[test]
    fn equal_stamp_on_coarse_clock_still_compares_content() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, "aaaa").unwrap();
        let stamp = DiskStamp::of(&p).unwrap().unwrap();
        // Same stamp but other content: what a same-tick rewrite looks like on FAT/NFS.
        let racy = Snapshot {
            stamp,
            hash: content_hash(b"bbbb"),
            racy: true,
        };
        assert!(!snapshot_matches(&racy, &p, stamp).unwrap());
        let settled = Snapshot {
            racy: false,
            ..racy
        };
        assert!(snapshot_matches(&settled, &p, stamp).unwrap());
        let doc = Document::open(&p, 0).unwrap();
        assert!(doc.disk.unwrap().racy, "a file written just now is racy");
    }

    #[test]
    fn save_reports_other_hard_links() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        let other = dir.path().join("backup.txt");
        fs::write(&p, "a").unwrap();
        fs::hard_link(&p, &other).unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        type_at_end(&mut doc, "b");
        assert_eq!(doc.save().unwrap().other_hard_links, 1);
        assert_eq!(fs::read_to_string(&p).unwrap(), "ab");
        assert_eq!(
            fs::read_to_string(&other).unwrap(),
            "a",
            "D46: other name keeps old"
        );
        type_at_end(&mut doc, "c");
        assert_eq!(doc.save().unwrap().other_hard_links, 0);
    }

    #[test]
    #[ignore = "timing (AC-18); run at sign-off with --release -- --ignored"]
    fn open_100mb_under_2s() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("big.txt");
        let line = "fn main() { println!(\"xin chào thế giới\"); } // 0123456789\n";
        let n = 100 * 1024 * 1024 / line.len() + 1;
        fs::write(&p, line.repeat(n)).unwrap();
        let t = Instant::now();
        let doc = Document::open(&p, 1 << 20).unwrap();
        let took = t.elapsed();
        assert!(!doc.highlight);
        assert_eq!(doc.buffer.text().len_lines(), n + 1);
        assert!(took < Duration::from_secs(2), "open took {took:?}");
    }
}
