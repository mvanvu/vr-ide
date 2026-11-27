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

use crate::safe_replace;
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
        let mut stamp = Self {
            len: m.len(),
            mtime: m.modified().ok(),
            ctime: (m.ctime(), m.ctime_nsec()),
            dev: m.dev(),
            ino: m.ino(),
        };
        if let Some(t) = hook::frozen_clock() {
            let since = t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
            stamp.mtime = Some(t);
            stamp.ctime = (since.as_secs() as i64, i64::from(since.subsec_nanos()));
        }
        stamp
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

/// A stamp taken now within `RACY_WINDOW` of its mtime: a later same-length write in the same
/// timestamp tick would leave it unchanged. Ask right after taking the stamp and before reading
/// the content: a write during a long read hides behind the stamp too, and the window may
/// have ended by the time the read does.
fn is_racy(stamp: &DiskStamp) -> bool {
    stamp.mtime.is_none_or(|m| {
        SystemTime::now()
            .duration_since(m)
            .is_ok_and(|age| age < RACY_WINDOW)
            // An mtime in the future (clock skew) is treated as racy too.
            || SystemTime::now() < m
    })
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
    pub fn is_own(&self, resolved: &Path) -> bool {
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
        /// After the last freshness check, right before the file is put in place.
        BeforeSwap,
        /// The file taken out of the path was not the expected one; right before it goes back.
        BeforeSwapBack,
        AfterRename,
        /// In `disk_version`, between taking the stamp and reading the content.
        ReadingDiskVersion,
        /// In `disk_version`, after the content was read and before the stamp is taken again.
        DiskVersionRead,
        /// After the content was compared with the snapshot (`check_disk`, `save`).
        SnapshotCompared,
        /// In `load`, after the content was read.
        LoadRead,
        /// In `save_as` to another path, right after the version of the target was taken.
        SaveAsVersionTaken,
    }

    type Hook = (Stage, Box<dyn FnOnce()>);

    thread_local! {
        static HOOKS: RefCell<Vec<Hook>> = const { RefCell::new(Vec::new()) };
    }

    pub(crate) fn set(stage: Stage, f: impl FnOnce() + 'static) {
        HOOKS.with(|h| h.borrow_mut().push((stage, Box::new(f))));
    }

    pub(crate) fn fire(stage: Stage) {
        let f = HOOKS.with(|h| {
            let mut h = h.borrow_mut();
            let at = h.iter().position(|(s, _)| *s == stage)?;
            Some(h.remove(at).1)
        });
        if let Some(f) = f {
            f();
        }
    }

    thread_local! {
        static TAKEN: RefCell<Option<super::DiskVersion>> = const { RefCell::new(None) };
    }

    /// `save_as` records the version of the target it took (racing tests match it).
    pub(crate) fn version_taken(v: &super::DiskVersion) {
        TAKEN.with(|t| *t.borrow_mut() = Some(*v));
    }

    pub(crate) fn take_version_taken() -> Option<super::DiskVersion> {
        TAKEN.with(|t| t.borrow_mut().take())
    }

    thread_local! {
        static FROZEN_CLOCK: std::cell::Cell<Option<std::time::SystemTime>> =
            const { std::cell::Cell::new(None) };
        static NO_EXCHANGE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }

    /// Stamps taken on this thread from now on carry this moment as mtime and ctime: a
    /// filesystem whose timestamp tick (1 ms jiffies, NFS, FAT's 2 s) outlasts the test, so a
    /// same-length rewrite in place leaves the stamp as it was.
    pub(crate) fn freeze_clock() {
        freeze_clock_at(std::time::SystemTime::now());
    }

    /// `freeze_clock` with the tick's time chosen, e.g. just inside the racy window so a test
    /// can let the window pass while a read is running.
    pub(crate) fn freeze_clock_at(t: std::time::SystemTime) {
        FROZEN_CLOCK.with(|c| c.set(Some(t)));
    }

    pub(crate) fn frozen_clock() -> Option<std::time::SystemTime> {
        FROZEN_CLOCK.with(|c| c.get())
    }

    /// Saves on this thread find no `RENAME_EXCHANGE` (FUSE, NFS, non-Linux): the commit
    /// falls back to check + rename.
    pub(crate) fn no_exchange() {
        NO_EXCHANGE.with(|c| c.set(true));
    }

    pub(crate) fn exchange_unsupported() -> bool {
        NO_EXCHANGE.with(|c| c.get())
    }
}

#[cfg(not(test))]
mod hook {
    #[inline(always)]
    pub(crate) fn version_taken(_v: &super::DiskVersion) {}

    #[derive(Clone, Copy)]
    pub(crate) enum Stage {
        BeforeRename,
        /// After the last freshness check, right before the file is put in place.
        BeforeSwap,
        /// The file taken out of the path was not the expected one; right before it goes back.
        BeforeSwapBack,
        AfterRename,
        /// In `disk_version`, between taking the stamp and reading the content.
        ReadingDiskVersion,
        /// In `disk_version`, after the content was read and before the stamp is taken again.
        DiskVersionRead,
        /// After the content was compared with the snapshot (`check_disk`, `save`).
        SnapshotCompared,
        /// In `load`, after the content was read.
        LoadRead,
        /// In `save_as` to another path, right after the version of the target was taken.
        SaveAsVersionTaken,
    }

    #[inline(always)]
    pub(crate) fn fire(_stage: Stage) {}

    #[inline(always)]
    pub(crate) fn frozen_clock() -> Option<std::time::SystemTime> {
        None
    }

    #[inline(always)]
    pub(crate) fn exchange_unsupported() -> bool {
        false
    }
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

    /// Saves even when the file changed on disk (the user chose to keep their version),
    /// replacing whatever is there at that moment. A prompt should use `save_overwrite_seen`,
    /// which replaces only the version the user was shown (D64).
    pub fn save_overwrite(&mut self) -> Result<Saved, DocError> {
        self.save_to_own_path(false)
    }

    /// The version of the file that is on disk now, for the "changed on disk" prompt: take
    /// it when the prompt (or the diff it offers) is shown and pass it to
    /// `save_overwrite_seen` when the user chooses to keep their text (D64). Take it before
    /// reading the text the prompt shows: the user then sees this version or a later one,
    /// and a later one makes `save_overwrite_seen` refuse.
    pub fn disk_version(&self) -> Result<DiskVersion, DocError> {
        DiskVersion::of_path(self.path.as_deref().ok_or(DocError::NoPath)?)
    }

    /// Saves over the file only if it still is the version `seen` (from `disk_version`) the
    /// user was shown when they chose to overwrite; `Err(ChangedOnDisk)` when a newer version
    /// (or a file where none was shown) is there, so the UI asks again with a fresh
    /// `disk_version`. Same commit as `save`: that newer version is never replaced.
    pub fn save_overwrite_seen(&mut self, seen: &DiskVersion) -> Result<Saved, DocError> {
        if self.read_only {
            return Err(DocError::NotUtf8ReadOnly);
        }
        let path = self.path.clone().ok_or(DocError::NoPath)?;
        // Refuse before the temp file is written; the commit checks again.
        if !still_fresh(&path, &seen.0).map_err(|e| io_error(e, &path))? {
            self.disk_state = DiskState::ChangedOnDisk;
            return Err(DocError::ChangedOnDisk);
        }
        self.write_to(&path, Some(seen.0))
    }

    /// Writes to `p` and makes it the document path. The document's own path (also through
    /// `.` or a symlink) is a `save`, with its changed-on-disk check. Another path replaces
    /// the version that is there when the call begins, and only that one: a version written
    /// during the save is never replaced (`Err(ChangedOnDisk)`), D89. A prompt uses
    /// `save_as_seen` with the version the user was shown.
    pub fn save_as(&mut self, p: &Path) -> Result<Saved, DocError> {
        if self.read_only {
            return Err(DocError::NotUtf8ReadOnly);
        }
        if self.is_own_path(p) {
            return self.save();
        }
        let now = DiskVersion::of_path(p)?;
        hook::version_taken(&now);
        hook::fire(hook::Stage::SaveAsVersionTaken);
        self.save_as_seen(p, &now)
    }

    /// Save As that writes to `p` only if it still is the version `seen`
    /// (`DiskVersion::of_path`, an absent file included) and makes `p` the document path.
    /// Same commit as `save`: another version is never replaced, and a file created meanwhile
    /// is never replaced either (no-replace create).
    ///
    /// Another path: `Err(ChangedOnDisk)` when anything but `seen` is there; the document
    /// keeps its path and `disk_state`, and the UI asks again (Replace) with a fresh version.
    ///
    /// The document's own path is a `save` that also expects `seen`. Its `Err(ChangedOnDisk)`
    /// sets `disk_state = ChangedOnDisk` and means the file differs from what the document
    /// last loaded or saved: a fresh version does not help. The UI shows the external-change
    /// prompt instead (Keep mine → `save_overwrite_seen`, or Reload; D81).
    pub fn save_as_seen(&mut self, p: &Path, seen: &DiskVersion) -> Result<Saved, DocError> {
        if self.read_only {
            return Err(DocError::NotUtf8ReadOnly);
        }
        let own = self.is_own_path(p);
        // Refuse before the temp file is written; the commit checks again.
        if !still_fresh(p, &seen.0).map_err(|e| io_error(e, p))? {
            if own {
                self.disk_state = DiskState::ChangedOnDisk;
            }
            return Err(DocError::ChangedOnDisk);
        }
        if own {
            return self.save();
        }
        let saved = self.write_to(p, Some(seen.0))?;
        self.path = Some(p.to_path_buf());
        Ok(saved)
    }

    /// `p` names the file the document is saved to: the same path, or the same final location
    /// once symlinks are followed and the directory is made canonical.
    fn is_own_path(&self, p: &Path) -> bool {
        let Some(own) = self.path.as_deref() else {
            return false;
        };
        own == p || location(own).is_some_and(|a| location(p).is_some_and(|b| a == b))
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
            let racy = cur.as_ref().is_some_and(is_racy);
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
                racy,
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
        let racy = is_racy(&cur);
        let same = snapshot_matches(&snap, path, cur).map_err(|e| io_error(e, path))?;
        hook::fire(hook::Stage::SnapshotCompared);
        // Refreshing also lets a racy snapshot settle once its mtime is old enough.
        if same && (snap.stamp != cur || snap.racy) {
            self.disk = Some(Snapshot {
                stamp: cur,
                hash: snap.hash,
                racy,
            });
        }
        Ok(!same)
    }

    fn write_to(&mut self, path: &Path, expect: Option<Expected>) -> Result<Saved, DocError> {
        let target = resolve_links(path).map_err(|e| io_error(e, path))?;
        let bom = self.encoding == Encoding::Utf8Bom;
        let written = write_atomic(&target, bom, self.buffer.text(), expect)
            .map_err(|e| io_error(e, path))?;
        // A refused Save As to another file says nothing about the document's own file.
        let own = self.path.as_deref() == Some(path);
        let (snapshot, saved) = match written {
            Written::Done { snapshot, saved } => (snapshot, saved),
            Written::ChangedOnDisk => {
                if own {
                    self.disk_state = DiskState::ChangedOnDisk;
                }
                return Err(DocError::ChangedOnDisk);
            }
            Written::Kept(e) => {
                // The path holds someone else's version, not what we loaded.
                if own {
                    self.disk_state = DiskState::ChangedOnDisk;
                }
                return Err(DocError::Io(e));
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
    let racy = is_racy(&stamp);
    let mut bytes = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(0));
    file.read_to_end(&mut bytes)
        .map_err(|e| io_error(e, path))?;
    hook::fire(hook::Stage::LoadRead);
    let hash = content_hash(&bytes);
    let (text, encoding) = decode(bytes)?;
    Ok(Loaded {
        read_only: matches!(encoding, Encoding::Other(_)),
        text,
        encoding,
        snapshot: Snapshot { stamp, hash, racy },
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

/// Hashes a regular file. Anything else is refused before it is read: `/dev/zero` never ends,
/// and opening a FIFO waits for a writer. The open does not block (O_NONBLOCK) and the opened
/// file is checked again, so a FIFO put at the path after the stat is refused too.
fn hash_file(path: &Path) -> io::Result<u64> {
    if !fs::metadata(path)?.is_file() {
        return Err(not_regular());
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(not_regular());
    }
    hash_reader(&file)
}

fn not_regular() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "not a regular file")
}

fn hash_reader(mut file: &File) -> io::Result<u64> {
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

/// Where a save to `path` lands: links followed, the directory canonical. `None` when the
/// directory cannot be resolved.
fn location(path: &Path) -> Option<PathBuf> {
    let target = resolve_links(path).ok()?;
    let name = target.file_name()?;
    let dir = match target.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };
    Some(fs::canonicalize(dir).ok()?.join(name))
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

/// A new file in `dir`, created exclusively with `mode`, under a name `is_save_temp` knows (the
/// watcher never reports it): the temp file of an atomic replace next to its target.
pub fn create_temp(dir: &Path, mode: u32) -> io::Result<(PathBuf, File)> {
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

/// How often `disk_version` reads a file that changes while it is read before giving up.
const DISK_VERSION_TRIES: usize = 5;

/// A version of a file on disk as the user was shown it (`Document::disk_version`); opaque.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiskVersion(Expected);

impl DiskVersion {
    /// This version is a file holding exactly `bytes` (racing tests).
    #[cfg(test)]
    pub(crate) fn holds(&self, bytes: &[u8]) -> bool {
        self.0.hash == Some(content_hash(bytes))
    }

    /// The version of whatever is at `path` now (links followed); an absent file is a version
    /// too (D74). Take it when the user is shown the target, e.g. the Replace prompt of Save
    /// As, or at submit for a name that does not exist, and pass it to
    /// `Document::save_as_seen`.
    pub fn of_path(path: &Path) -> Result<DiskVersion, DocError> {
        // A stamp and a hash taken around a write would describe two different versions, and
        // the save would then accept the later one as "seen": read until the file held still.
        for _ in 0..DISK_VERSION_TRIES {
            let read = || {
                let Some(stamp) = DiskStamp::of(path)? else {
                    return Ok(Some(Expected {
                        stamp: None,
                        hash: None,
                        racy: false,
                    }));
                };
                let racy = is_racy(&stamp);
                hook::fire(hook::Stage::ReadingDiskVersion);
                let hash = hash_file(path)?;
                hook::fire(hook::Stage::DiskVersionRead);
                io::Result::Ok((DiskStamp::of(path)? == Some(stamp)).then_some(Expected {
                    stamp: Some(stamp),
                    hash: Some(hash),
                    racy,
                }))
            };
            match read() {
                Ok(Some(version)) => return Ok(DiskVersion(version)),
                Ok(None) => {}
                // Deleted between the stamp and the read: look again.
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(io_error(e, path)),
            }
        }
        Err(DocError::Io(io::Error::other(
            "the file keeps changing on disk; try again",
        )))
    }
}

/// What the caller's changed-on-disk check saw: the target's stamp (`None`: absent) and the
/// hash of the content it holds.
#[derive(Debug, Clone, Copy)]
struct Expected {
    stamp: Option<DiskStamp>,
    hash: Option<u64>,
    /// The stamp was racy (`is_racy`) when the check saw it: a same-length write in the same
    /// timestamp tick keeps it, so an equal stamp proves nothing and the content decides.
    racy: bool,
}

/// One version of the file: when the stamp was seen does not make it another version.
impl PartialEq for Expected {
    fn eq(&self, other: &Self) -> bool {
        self.stamp == other.stamp && self.hash == other.hash
    }
}

impl Eq for Expected {}

/// The target still holds what the caller's check saw. A different stamp with the same
/// length may be a metadata-only change (chmod, xattr by a sync or backup tool), and an equal
/// racy stamp may hide a write in the same tick: the content decides, and the stamp must not
/// move again while it is read.
fn still_fresh(target: &Path, expected: &Expected) -> io::Result<bool> {
    let cur = DiskStamp::of(target)?;
    if cur == expected.stamp && !(expected.racy && cur.is_some()) {
        return Ok(true);
    }
    match (cur, expected.stamp, expected.hash) {
        (Some(c), Some(s), Some(h)) if c.len == s.len => {
            Ok(hash_file(target)? == h && DiskStamp::of(target)? == Some(c))
        }
        _ => Ok(false),
    }
}

/// The file swapped out of the path holds the content the caller's check saw. The swap changed
/// its ctime, so only the content can tell.
fn holds_expected(swapped: &Path, expected: &Expected) -> io::Result<bool> {
    let (Some(s), Some(h)) = (expected.stamp, expected.hash) else {
        return Ok(false);
    };
    Ok(fs::symlink_metadata(swapped)?.len() == s.len && hash_file(swapped)? == h)
}

/// What `write_atomic` did.
enum Written {
    Done {
        snapshot: Snapshot,
        saved: Saved,
    },
    /// The target no longer held what the check saw; nothing replaced.
    ChangedOnDisk,
    /// The target changed and a version was kept under a conflict name; the error names it.
    Kept(io::Error),
}

/// Temp file in the target's directory → write → owner and mode → fsync → put in place only
/// if unchanged (`safe_replace`) → fsync dir. The target's data is never written, so a crash leaves either the old
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
    let mut written_len = 0u64;
    let mut w = BufWriter::with_capacity(64 * 1024, &file);
    if bom {
        w.write_all(UTF8_BOM)?;
        hasher.write(UTF8_BOM);
        written_len += UTF8_BOM.len() as u64;
    }
    for chunk in text.chunks() {
        w.write_all(chunk.as_bytes())?;
        hasher.write(chunk.as_bytes());
        written_len += chunk.len() as u64;
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
    let written_hash = hasher.finish();
    hook::fire(hook::Stage::BeforeRename);
    // The file is put in place only if it still holds what the caller's check saw, without a
    // window in which another writer's version can be replaced (`safe_replace`).
    let committed = match &expect {
        // Overwrite chosen by the user.
        None => {
            hook::fire(hook::Stage::BeforeSwap);
            fs::rename(&tmp, target)?;
            true
        }
        Some(Expected { stamp: None, .. }) => {
            hook::fire(hook::Stage::BeforeSwap);
            safe_replace::create_if_absent(&tmp, target)?
        }
        Some(expected) => {
            let outcome = safe_replace::replace_if_unchanged(
                &tmp,
                target,
                safe_replace::Checks {
                    target_unchanged: &mut |p| still_fresh(p, expected),
                    holds_expected: &mut |p| holds_expected(p, expected),
                    holds_ours: &mut |p| {
                        Ok(fs::symlink_metadata(p)?.len() == written_len
                            && hash_file(p)? == written_hash)
                    },
                },
                &mut |a: &Path, b: &Path| {
                    if hook::exchange_unsupported() {
                        return Err(io::ErrorKind::Unsupported.into());
                    }
                    safe_replace::exchange_files(a, b)
                },
                &mut |step| match step {
                    safe_replace::Step::BeforeSwap => hook::fire(hook::Stage::BeforeSwap),
                    safe_replace::Step::BeforeSwapBack => hook::fire(hook::Stage::BeforeSwapBack),
                    _ => {}
                },
            )?;
            match outcome {
                safe_replace::Outcome::Replaced => true,
                safe_replace::Outcome::Changed => false,
                safe_replace::Outcome::Kept { at, what, cause } => {
                    guard.0 = None;
                    return Ok(Written::Kept(safe_replace::kept_error(&at, what, &cause)));
                }
            }
        }
    };
    // `tmp` was renamed into place or deleted by `safe_replace`; after an error above it was
    // never at the target and the guard deletes it.
    guard.0 = None;
    if !committed {
        return Ok(Written::ChangedOnDisk);
    }
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
            hash: written_hash,
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

    /// A writer that lands after save's last check, right before the file is put in place.
    fn save_with_writer_at_swap(
        writer: impl FnOnce(&Path) + 'static,
    ) -> (Result<Saved, DocError>, PathBuf, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, "mine\n").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        type_at_end(&mut doc, "edit");
        let at = p.clone();
        hook::set(hook::Stage::BeforeSwap, move || writer(&at));
        let r = doc.save();
        if r.is_err() {
            assert!(doc.is_modified());
        }
        (r, p, dir)
    }

    #[test]
    fn save_never_replaces_in_place_write_at_swap() {
        let (r, p, dir) = save_with_writer_at_swap(|p| fs::write(p, "agent\n").unwrap());
        assert!(matches!(r, Err(DocError::ChangedOnDisk)), "{r:?}");
        assert_eq!(fs::read_to_string(&p).unwrap(), "agent\n");
        assert_eq!(dir_names(dir.path()), vec!["a.txt"]);
    }

    #[test]
    fn save_never_replaces_renamed_in_version_at_swap() {
        let (r, p, dir) = save_with_writer_at_swap(|p| {
            let other = p.with_file_name("agent.tmp");
            fs::write(&other, "agent\n").unwrap();
            fs::rename(&other, p).unwrap();
        });
        assert!(matches!(r, Err(DocError::ChangedOnDisk)), "{r:?}");
        assert_eq!(fs::read_to_string(&p).unwrap(), "agent\n");
        assert_eq!(dir_names(dir.path()), vec!["a.txt"]);
    }

    #[test]
    fn save_never_replaces_file_created_at_swap() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, "mine").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        fs::remove_file(&p).unwrap();
        let at = p.clone();
        hook::set(hook::Stage::BeforeSwap, move || {
            fs::write(&at, "agent").unwrap()
        });
        assert!(matches!(doc.save(), Err(DocError::ChangedOnDisk)));
        assert_eq!(fs::read_to_string(&p).unwrap(), "agent");
        assert_eq!(dir_names(dir.path()), vec!["a.txt"]);
    }

    #[test]
    fn save_overwrite_seen_replaces_only_the_version_shown() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, "mine\n").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        type_at_end(&mut doc, "edit");
        fs::write(&p, "agent v1\n").unwrap();
        assert!(matches!(doc.save(), Err(DocError::ChangedOnDisk)));
        // The prompt shows v1; the agent writes v2 before the user answers.
        let seen = doc.disk_version().unwrap();
        fs::write(&p, "agent v2\n").unwrap();
        let r = doc.save_overwrite_seen(&seen);
        assert!(matches!(r, Err(DocError::ChangedOnDisk)), "{r:?}");
        assert_eq!(fs::read_to_string(&p).unwrap(), "agent v2\n");
        assert_eq!(doc.disk_state, DiskState::ChangedOnDisk);
        assert!(doc.is_modified());
        // Shown v2 and confirmed again: now it is replaced.
        let seen = doc.disk_version().unwrap();
        doc.save_overwrite_seen(&seen).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "mine\nedit");
        assert!(!doc.is_modified());
        assert_eq!(doc.check_disk(), DiskState::InSync);
        assert_eq!(dir_names(dir.path()), vec!["a.txt"]);
    }

    #[test]
    fn save_keeps_second_write_made_while_restoring() {
        // One write lands after the last check, the writer's next one while the first is
        // being put back: both versions must still exist afterwards.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, "mine\n").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        type_at_end(&mut doc, "edit");
        let (first, second) = (p.clone(), p.clone());
        hook::set(hook::Stage::BeforeSwap, move || {
            fs::write(&first, "agent v1\n").unwrap()
        });
        hook::set(hook::Stage::BeforeSwapBack, move || {
            fs::write(&second, "agent v2 (latest)\n").unwrap()
        });
        let err = match doc.save() {
            Err(DocError::Io(e)) => e.to_string(),
            other => panic!("{other:?}; dir {:?}", dir_names(dir.path())),
        };
        let (_, kept) = err.rsplit_once(" kept at ").expect(&err);
        assert_eq!(fs::read_to_string(kept).unwrap(), "agent v2 (latest)\n");
        assert_eq!(fs::read_to_string(&p).unwrap(), "agent v1\n");
        let names = dir_names(dir.path());
        assert_eq!(names.len(), 2, "{names:?}");
        assert!(
            names.iter().all(|n| !is_save_temp(Path::new(n))),
            "{names:?}"
        );
        assert!(doc.is_modified());
        assert_eq!(doc.disk_state, DiskState::ChangedOnDisk);
        assert_eq!(doc.check_disk(), DiskState::ChangedOnDisk);
    }

    #[test]
    fn disk_version_is_of_one_version_of_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, "agent v1\n").unwrap();
        let doc = Document::open(&p, 1 << 20).unwrap();
        // Written after the stamp was taken and before the content is read, with a new length
        // so the stamp moves however coarse the timestamp tick is (a same-length write inside
        // one tick is `disk_version_in_one_tick_is_of_the_bytes_on_disk`).
        let at = p.clone();
        hook::set(hook::Stage::ReadingDiskVersion, move || {
            fs::write(&at, "agent v22\n").unwrap()
        });
        let seen = doc.disk_version().unwrap();
        assert_eq!(seen, doc.disk_version().unwrap());
    }

    #[test]
    fn disk_version_of_a_file_that_keeps_changing_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, "agent v0\n").unwrap();
        let doc = Document::open(&p, 1 << 20).unwrap();
        // A new length on every write: the stamp moves even when the timestamp tick (1 ms
        // jiffies, NFS, FAT) outlasts all five writes. Same-length writes inside one tick are
        // `disk_version_in_one_tick_is_of_the_bytes_on_disk`.
        for i in 0..DISK_VERSION_TRIES {
            let at = p.clone();
            hook::set(hook::Stage::ReadingDiskVersion, move || {
                fs::write(&at, format!("agent w{}\n", "+".repeat(i + 2))).unwrap()
            });
        }
        let r = doc.disk_version();
        assert!(matches!(r, Err(DocError::Io(_))), "{r:?}");
        doc.disk_version().unwrap();
    }

    /// Freezes the tick 500 ms before the end of its racy window, so a read can outlast it.
    fn freeze_clock_near_window_end() -> SystemTime {
        let tick = SystemTime::now() - RACY_WINDOW + Duration::from_millis(500);
        hook::freeze_clock_at(tick);
        tick
    }

    /// What a long read (large file, slow NFS) does: the racy window of `tick` ends meanwhile.
    fn let_racy_window_pass(tick: SystemTime) {
        let end = tick + RACY_WINDOW + Duration::from_millis(50);
        std::thread::sleep(end.duration_since(SystemTime::now()).unwrap_or_default());
    }

    /// During the read: a same-length rewrite in the tick, then the window ends.
    fn rewrite_during_read(stage: hook::Stage, p: &Path, text: &'static str, tick: SystemTime) {
        let at = p.to_path_buf();
        hook::set(stage, move || {
            rewrite_in_same_tick(&at, text);
            let_racy_window_pass(tick);
        });
    }

    fn assert_racy_now(p: &Path) {
        let stamp = DiskStamp::of(p).unwrap().unwrap();
        assert!(is_racy(&stamp), "setup: the stamp must start racy");
    }

    #[test]
    fn disk_version_read_racing_the_end_of_the_window_stays_racy() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        let tick = freeze_clock_near_window_end();
        hook::no_exchange();
        fs::write(&p, "mine\n").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        type_at_end(&mut doc, "edit");
        fs::write(&p, "agent v1\n").unwrap();
        assert_racy_now(&p);
        rewrite_during_read(hook::Stage::DiskVersionRead, &p, "agent v2\n", tick);
        let seen = doc.disk_version().unwrap();
        assert!(seen.holds(b"agent v1\n"), "{seen:?}");
        let r = doc.save_overwrite_seen(&seen);
        assert!(matches!(r, Err(DocError::ChangedOnDisk)), "{r:?}");
        assert_eq!(fs::read_to_string(&p).unwrap(), "agent v2\n");
        assert_eq!(dir_names(dir.path()), vec!["a.txt"]);
    }

    #[test]
    fn save_as_seen_version_read_racing_the_end_of_the_window_stays_racy() {
        let (dir, mut doc, p, other) = doc_and_other();
        let tick = freeze_clock_near_window_end();
        hook::no_exchange();
        fs::write(&other, "theirs v1\n").unwrap();
        assert_racy_now(&other);
        rewrite_during_read(hook::Stage::DiskVersionRead, &other, "theirs v2\n", tick);
        let seen = DiskVersion::of_path(&other).unwrap();
        let r = doc.save_as_seen(&other, &seen);
        assert!(matches!(r, Err(DocError::ChangedOnDisk)), "{r:?}");
        assert_eq!(fs::read_to_string(&other).unwrap(), "theirs v2\n");
        assert_untouched_doc(&doc, &p);
        assert_eq!(dir_names(dir.path()), vec!["a.txt", "other.txt"]);
    }

    #[test]
    fn save_check_racing_the_end_of_the_window_stays_racy() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        let tick = freeze_clock_near_window_end();
        hook::no_exchange();
        fs::write(&p, "agent v1\n").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        type_at_end(&mut doc, "edit");
        assert_racy_now(&p);
        rewrite_during_read(hook::Stage::SnapshotCompared, &p, "agent v2\n", tick);
        let r = doc.save();
        assert!(matches!(r, Err(DocError::ChangedOnDisk)), "{r:?}");
        assert_eq!(fs::read_to_string(&p).unwrap(), "agent v2\n");
        assert_eq!(dir_names(dir.path()), vec!["a.txt"]);
    }

    #[test]
    fn check_disk_racing_the_end_of_the_window_stays_racy() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        let tick = freeze_clock_near_window_end();
        hook::no_exchange();
        fs::write(&p, "agent v1\n").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        type_at_end(&mut doc, "edit");
        assert_racy_now(&p);
        rewrite_during_read(hook::Stage::SnapshotCompared, &p, "agent v2\n", tick);
        // The read saw v1; v2 landed while it ran and must not be hidden from later checks.
        doc.check_disk();
        assert_eq!(doc.check_disk(), DiskState::ChangedOnDisk);
        let r = doc.save();
        assert!(matches!(r, Err(DocError::ChangedOnDisk)), "{r:?}");
        assert_eq!(fs::read_to_string(&p).unwrap(), "agent v2\n");
    }

    #[test]
    fn load_racing_the_end_of_the_window_stays_racy() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        let tick = freeze_clock_near_window_end();
        hook::no_exchange();
        fs::write(&p, "agent v1\n").unwrap();
        assert_racy_now(&p);
        rewrite_during_read(hook::Stage::LoadRead, &p, "agent v2\n", tick);
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        assert_eq!(doc.buffer.text().to_string(), "agent v1\n");
        assert_eq!(doc.check_disk(), DiskState::ChangedOnDisk);
        type_at_end(&mut doc, "edit");
        let r = doc.save();
        assert!(matches!(r, Err(DocError::ChangedOnDisk)), "{r:?}");
        assert_eq!(fs::read_to_string(&p).unwrap(), "agent v2\n");
    }

    #[test]
    fn disk_version_in_one_tick_is_of_the_bytes_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        hook::freeze_clock();
        hook::no_exchange();
        fs::write(&p, "mine\n").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        type_at_end(&mut doc, "edit");
        fs::write(&p, "agent v1\n").unwrap();
        // Same length, same tick, after the stamp was taken and before the content is read:
        // no stamp shows it, so the version must be of the bytes read, never of v1.
        let at = p.clone();
        hook::set(hook::Stage::ReadingDiskVersion, move || {
            rewrite_in_same_tick(&at, "agent v2\n")
        });
        let seen = doc.disk_version().unwrap();
        assert!(seen.holds(b"agent v2\n"), "{seen:?}");
        // v1 was never what the version describes, so it is not replaced either.
        rewrite_in_same_tick(&p, "agent v1\n");
        let r = doc.save_overwrite_seen(&seen);
        assert!(matches!(r, Err(DocError::ChangedOnDisk)), "{r:?}");
        assert_eq!(fs::read_to_string(&p).unwrap(), "agent v1\n");
        rewrite_in_same_tick(&p, "agent v2\n");
        doc.save_overwrite_seen(&seen).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "mine\nedit");
    }

    #[test]
    fn save_overwrite_seen_never_replaces_a_write_racing_it() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, "mine\n").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        type_at_end(&mut doc, "edit");
        fs::write(&p, "agent v1\n").unwrap();
        let seen = doc.disk_version().unwrap();
        for stage in [hook::Stage::BeforeRename, hook::Stage::BeforeSwap] {
            let at = p.clone();
            hook::set(stage, move || fs::write(&at, "agent v2\n").unwrap());
            let r = doc.save_overwrite_seen(&seen);
            assert!(matches!(r, Err(DocError::ChangedOnDisk)), "{r:?}");
            assert_eq!(fs::read_to_string(&p).unwrap(), "agent v2\n");
            assert_eq!(dir_names(dir.path()), vec!["a.txt"]);
            fs::write(&p, "agent v1\n").unwrap();
        }
        // Rewritten with the bytes the user saw: that is the version they agreed to replace.
        doc.save_overwrite_seen(&seen).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "mine\nedit");
    }

    #[test]
    fn save_overwrite_seen_with_no_file_shown() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, "mine\n").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        type_at_end(&mut doc, "edit");
        fs::remove_file(&p).unwrap();
        // The prompt said "deleted on disk"; a file created since then is not replaced.
        let seen = doc.disk_version().unwrap();
        fs::write(&p, "agent\n").unwrap();
        let r = doc.save_overwrite_seen(&seen);
        assert!(matches!(r, Err(DocError::ChangedOnDisk)), "{r:?}");
        assert_eq!(fs::read_to_string(&p).unwrap(), "agent\n");
        fs::remove_file(&p).unwrap();
        let at = p.clone();
        hook::set(hook::Stage::BeforeSwap, move || {
            fs::write(&at, "agent\n").unwrap()
        });
        let r = doc.save_overwrite_seen(&seen);
        assert!(matches!(r, Err(DocError::ChangedOnDisk)), "{r:?}");
        assert_eq!(fs::read_to_string(&p).unwrap(), "agent\n");
        fs::remove_file(&p).unwrap();
        doc.save_overwrite_seen(&seen).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "mine\nedit");
        assert_eq!(dir_names(dir.path()), vec!["a.txt"]);
        // Untitled documents have no disk version.
        assert!(matches!(
            Document::untitled().disk_version(),
            Err(DocError::NoPath)
        ));
    }

    /// Writes `text` in place and checks that the stamp did not move: the coarse tick the
    /// test runs under hides the write from every stamp comparison.
    fn rewrite_in_same_tick(p: &Path, text: &str) {
        let before = DiskStamp::of(p).unwrap();
        fs::write(p, text).unwrap();
        assert_eq!(
            DiskStamp::of(p).unwrap(),
            before,
            "the write moved the stamp"
        );
    }

    #[test]
    fn save_overwrite_seen_never_replaces_a_write_in_the_same_tick_as_the_version() {
        for exchange in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let p = dir.path().join("a.txt");
            fs::write(&p, "mine\n").unwrap();
            let mut doc = Document::open(&p, 1 << 20).unwrap();
            type_at_end(&mut doc, "edit");
            hook::freeze_clock();
            if !exchange {
                hook::no_exchange();
            }
            fs::write(&p, "agent v1\n").unwrap();
            // The prompt is shown with v1; the agent writes v2 before the clock ticks.
            let seen = doc.disk_version().unwrap();
            rewrite_in_same_tick(&p, "agent v2\n");
            let r = doc.save_overwrite_seen(&seen);
            assert!(
                matches!(r, Err(DocError::ChangedOnDisk)),
                "{exchange} {r:?}"
            );
            assert_eq!(fs::read_to_string(&p).unwrap(), "agent v2\n", "{exchange}");
            assert_eq!(dir_names(dir.path()), vec!["a.txt"], "{exchange}");
            // Rewritten with the bytes the user saw: that version may be replaced.
            rewrite_in_same_tick(&p, "agent v1\n");
            doc.save_overwrite_seen(&seen).unwrap();
            assert_eq!(fs::read_to_string(&p).unwrap(), "mine\nedit", "{exchange}");
        }
    }

    #[test]
    fn save_never_replaces_a_write_in_the_same_tick_as_its_check() {
        for exchange in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let p = dir.path().join("a.txt");
            hook::freeze_clock();
            if !exchange {
                hook::no_exchange();
            }
            fs::write(&p, "agent v1\n").unwrap();
            let mut doc = Document::open(&p, 1 << 20).unwrap();
            type_at_end(&mut doc, "edit");
            // Before the check: the snapshot is racy, so the content is compared.
            rewrite_in_same_tick(&p, "agent v2\n");
            assert_eq!(doc.check_disk(), DiskState::ChangedOnDisk, "{exchange}");
            let r = doc.save();
            assert!(
                matches!(r, Err(DocError::ChangedOnDisk)),
                "{exchange} {r:?}"
            );
            assert_eq!(fs::read_to_string(&p).unwrap(), "agent v2\n", "{exchange}");
            doc.reload().unwrap();
            type_at_end(&mut doc, "edit");
            // After the check, before the commit.
            let at = p.clone();
            hook::set(hook::Stage::BeforeRename, move || {
                rewrite_in_same_tick(&at, "agent v3\n")
            });
            let r = doc.save();
            assert!(
                matches!(r, Err(DocError::ChangedOnDisk)),
                "{exchange} {r:?}"
            );
            assert_eq!(fs::read_to_string(&p).unwrap(), "agent v3\n", "{exchange}");
            assert_eq!(dir_names(dir.path()), vec!["a.txt"], "{exchange}");
        }
    }

    #[test]
    fn save_as_seen_never_replaces_a_write_in_the_same_tick_as_the_version() {
        for exchange in [true, false] {
            let (dir, mut doc, p, other) = doc_and_other();
            hook::freeze_clock();
            if !exchange {
                hook::no_exchange();
            }
            fs::write(&other, "theirs v1\n").unwrap();
            // The Replace prompt shows v1; the agent writes v2 before the clock ticks.
            let seen = DiskVersion::of_path(&other).unwrap();
            rewrite_in_same_tick(&other, "theirs v2\n");
            let r = doc.save_as_seen(&other, &seen);
            assert!(
                matches!(r, Err(DocError::ChangedOnDisk)),
                "{exchange} {r:?}"
            );
            assert_eq!(
                fs::read_to_string(&other).unwrap(),
                "theirs v2\n",
                "{exchange}"
            );
            assert_untouched_doc(&doc, &p);
            assert_eq!(
                dir_names(dir.path()),
                vec!["a.txt", "other.txt"],
                "{exchange}"
            );
            rewrite_in_same_tick(&other, "theirs v1\n");
            doc.save_as_seen(&other, &seen).unwrap();
            assert_eq!(
                fs::read_to_string(&other).unwrap(),
                "mine\nedit",
                "{exchange}"
            );
        }
    }

    #[test]
    fn disk_version_of_any_path_counts_absence_as_a_version() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("other.txt");
        let absent = DiskVersion::of_path(&p).unwrap();
        assert_eq!(absent, DiskVersion::of_path(&p).unwrap());
        fs::write(&p, "agent v1\n").unwrap();
        let v1 = DiskVersion::of_path(&p).unwrap();
        assert_ne!(v1, absent);
        fs::write(&p, "agent v2\n").unwrap();
        assert_ne!(DiskVersion::of_path(&p).unwrap(), v1);
        // The document's own version is the version of its path.
        let doc = Document::open(&p, 1 << 20).unwrap();
        assert_eq!(
            doc.disk_version().unwrap(),
            DiskVersion::of_path(&p).unwrap()
        );
    }

    /// A modified document at `a.txt` and an existing `other.txt` holding "theirs v1\n".
    fn doc_and_other() -> (tempfile::TempDir, Document, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.txt");
        fs::write(&p, "mine\n").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        type_at_end(&mut doc, "edit");
        let other = dir.path().join("other.txt");
        fs::write(&other, "theirs v1\n").unwrap();
        (dir, doc, p, other)
    }

    /// The refused Save As left the document as it was: same path, still modified.
    fn assert_untouched_doc(doc: &Document, p: &Path) {
        assert_eq!(doc.path.as_deref(), Some(p));
        assert!(doc.is_modified());
        assert_eq!(doc.disk_state, DiskState::InSync);
    }

    #[test]
    fn save_as_seen_never_replaces_a_version_written_after_the_replace_prompt() {
        let (dir, mut doc, p, other) = doc_and_other();
        // The Replace prompt shows v1; the agent writes v2 before the user answers.
        let seen = DiskVersion::of_path(&other).unwrap();
        fs::write(&other, "theirs v2\n").unwrap();
        let r = doc.save_as_seen(&other, &seen);
        assert!(matches!(r, Err(DocError::ChangedOnDisk)), "{r:?}");
        assert_eq!(fs::read_to_string(&other).unwrap(), "theirs v2\n");
        assert_untouched_doc(&doc, &p);
        // Writes after the last check, right before the file is put in place.
        let seen = DiskVersion::of_path(&other).unwrap();
        for stage in [hook::Stage::BeforeRename, hook::Stage::BeforeSwap] {
            let at = other.clone();
            hook::set(stage, move || fs::write(&at, "theirs v3\n").unwrap());
            let r = doc.save_as_seen(&other, &seen);
            assert!(matches!(r, Err(DocError::ChangedOnDisk)), "{stage:?} {r:?}");
            assert_eq!(fs::read_to_string(&other).unwrap(), "theirs v3\n");
            assert_untouched_doc(&doc, &p);
            fs::write(&other, "theirs v2\n").unwrap();
        }
        assert_eq!(dir_names(dir.path()), vec!["a.txt", "other.txt"]);
        // The version the user confirmed is replaced and becomes the document's file.
        let seen = DiskVersion::of_path(&other).unwrap();
        doc.save_as_seen(&other, &seen).unwrap();
        assert_eq!(fs::read_to_string(&other).unwrap(), "mine\nedit");
        assert_eq!(fs::read_to_string(&p).unwrap(), "mine\n");
        assert_eq!(doc.path.as_deref(), Some(other.as_path()));
        assert!(!doc.is_modified());
        assert_eq!(doc.check_disk(), DiskState::InSync);
    }

    #[test]
    fn save_as_seen_never_replaces_a_file_created_after_it_was_seen_absent() {
        let (dir, mut doc, p, _) = doc_and_other();
        let new = dir.path().join("new.txt");
        let seen = DiskVersion::of_path(&new).unwrap();
        fs::write(&new, "agent\n").unwrap();
        let r = doc.save_as_seen(&new, &seen);
        assert!(matches!(r, Err(DocError::ChangedOnDisk)), "{r:?}");
        assert_eq!(fs::read_to_string(&new).unwrap(), "agent\n");
        assert_untouched_doc(&doc, &p);
        // Created after the last check, right before the file is put in place.
        fs::remove_file(&new).unwrap();
        for stage in [hook::Stage::BeforeRename, hook::Stage::BeforeSwap] {
            let at = new.clone();
            hook::set(stage, move || fs::write(&at, "agent\n").unwrap());
            let r = doc.save_as_seen(&new, &seen);
            assert!(matches!(r, Err(DocError::ChangedOnDisk)), "{stage:?} {r:?}");
            assert_eq!(fs::read_to_string(&new).unwrap(), "agent\n");
            assert_untouched_doc(&doc, &p);
            fs::remove_file(&new).unwrap();
        }
        assert_eq!(dir_names(dir.path()), vec!["a.txt", "other.txt"]);
        doc.save_as_seen(&new, &seen).unwrap();
        assert_eq!(fs::read_to_string(&new).unwrap(), "mine\nedit");
        assert_eq!(doc.path.as_deref(), Some(new.as_path()));
    }

    #[test]
    fn save_as_to_the_own_path_checks_freshness_like_save() {
        let (dir, mut doc, p, _) = doc_and_other();
        fs::write(&p, "agent\n").unwrap();
        // The prompt is prefilled with the current name and the user presses Enter; the
        // version taken at submit is the agent's, which the user never chose to replace.
        let same = dir.path().join(".").join("a.txt");
        for path in [p.clone(), same] {
            let seen = DiskVersion::of_path(&path).unwrap();
            let r = doc.save_as_seen(&path, &seen);
            assert!(matches!(r, Err(DocError::ChangedOnDisk)), "{r:?}");
            assert_eq!(fs::read_to_string(&p).unwrap(), "agent\n");
            let r = doc.save_as(&path);
            assert!(matches!(r, Err(DocError::ChangedOnDisk)), "{r:?}");
            assert_eq!(fs::read_to_string(&p).unwrap(), "agent\n");
            assert_eq!(doc.disk_state, DiskState::ChangedOnDisk);
            assert_eq!(doc.path.as_deref(), Some(p.as_path()));
            assert!(doc.is_modified());
        }
        assert_eq!(dir_names(dir.path()), vec!["a.txt", "other.txt"]);
    }

    #[test]
    fn save_as_to_the_own_path_saves_an_unchanged_file() {
        let (_dir, mut doc, p, _) = doc_and_other();
        let seen = DiskVersion::of_path(&p).unwrap();
        doc.save_as_seen(&p, &seen).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "mine\nedit");
        type_at_end(&mut doc, "2");
        doc.save_as(&p).unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "mine\nedit2");
        assert!(!doc.is_modified());
        // A version seen before another writer's change is not the one on disk.
        let seen = DiskVersion::of_path(&p).unwrap();
        fs::write(&p, "agent\n").unwrap();
        doc.reload().unwrap();
        type_at_end(&mut doc, "3");
        let r = doc.save_as_seen(&p, &seen);
        assert!(matches!(r, Err(DocError::ChangedOnDisk)), "{r:?}");
        assert_eq!(fs::read_to_string(&p).unwrap(), "agent\n");
    }

    /// Runs `f` on another thread and fails when it has not returned within two seconds (a
    /// blocked read or open would otherwise hang the test run).
    fn within_2s<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        rx.recv_timeout(Duration::from_secs(2))
            .expect("blocked for more than 2 s")
    }

    fn mkfifo(p: &Path) {
        rustix::fs::mknodat(
            rustix::fs::CWD,
            p,
            rustix::fs::FileType::Fifo,
            rustix::fs::Mode::from_raw_mode(0o644),
            0,
        )
        .unwrap();
    }

    #[test]
    fn disk_version_of_a_device_is_an_error_without_reading_it() {
        let r = within_2s(|| DiskVersion::of_path(Path::new("/dev/zero")));
        assert!(
            matches!(&r, Err(DocError::Io(e)) if e.kind() == io::ErrorKind::InvalidInput),
            "{r:?}"
        );
    }

    #[test]
    fn disk_version_of_a_fifo_is_an_error_without_blocking() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("pipe");
        mkfifo(&fifo);
        let at = fifo.clone();
        let r = within_2s(move || DiskVersion::of_path(&at));
        assert!(
            matches!(&r, Err(DocError::Io(e)) if e.kind() == io::ErrorKind::InvalidInput),
            "{r:?}"
        );
    }

    #[test]
    fn save_as_seen_refuses_a_fifo_put_at_the_target_without_blocking() {
        // Seen as an empty file; a FIFO (also length 0) takes its place before the commit.
        let (dir, mut doc, p, _) = doc_and_other();
        let target = dir.path().join("empty.txt");
        fs::write(&target, "").unwrap();
        let seen = DiskVersion::of_path(&target).unwrap();
        fs::remove_file(&target).unwrap();
        mkfifo(&target);
        let (r, doc) = within_2s(move || {
            let r = doc.save_as_seen(&target, &seen);
            (r, doc)
        });
        assert!(r.is_err(), "{r:?}");
        assert_untouched_doc(&doc, &p);
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
