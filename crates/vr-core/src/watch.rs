//! Debounced file watching for open documents and the project tree.
//!
//! A watched file is observed through its (canonical) parent directory, because editors and
//! AI agents usually replace files by renaming a temp file over them, which ends an inotify
//! watch on the file itself. Events arrive on the debouncer's thread, are coalesced to one
//! event per path per batch and are sent with the paths the caller passed to `watch`.
//!
//! Own saves: give each `Document` the watcher's `own_writes()` once (`set_own_writes`);
//! `save` then records what it wrote and the watcher drops events while the file still holds
//! exactly that content. A kernel queue overflow becomes `Modified`/`Removed` for every watched
//! file plus `Rescan` per directory; later watcher failures arrive as `FsEvent::Error`.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use notify::event::{ModifyKind, RenameMode};
use notify::{EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{
    DebounceEventResult, DebouncedEvent, Debouncer, RecommendedCache, new_debouncer,
};

use crate::document::{OwnWrites, is_save_temp};

/// Quiet time before a burst of changes to one path is reported.
pub const DEBOUNCE: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsEvent {
    Modified(PathBuf),
    Removed(PathBuf),
    Created(PathBuf),
    Renamed(PathBuf, PathBuf),
    /// Events under this watched directory were lost (kernel queue overflow): re-read it.
    Rescan(PathBuf),
    /// The watcher failed after `watch` returned (e.g. the inotify watch limit); changes may
    /// go unreported until the user acts.
    Error(String),
}

#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    #[error(transparent)]
    Notify(#[from] notify::Error),
    #[error("cannot watch {0}: {1}")]
    Path(PathBuf, io::Error),
}

/// What the caller asked for, keyed by resolved (canonical) path.
#[derive(Default)]
struct Scope {
    /// Resolved file → path as given to `watch`.
    files: HashMap<PathBuf, PathBuf>,
    /// Resolved directory root (recursive) → path as given.
    dirs: HashMap<PathBuf, PathBuf>,
    /// Saves of our own documents (filled by `Document::save`).
    own: OwnWrites,
}

impl Scope {
    fn dir_root(&self, p: &Path) -> Option<(&PathBuf, &PathBuf)> {
        self.dirs.iter().find(|(root, _)| p.starts_with(root))
    }

    fn contains(&self, p: &Path) -> bool {
        self.files.contains_key(p) || self.dir_root(p).is_some()
    }

    /// The caller's spelling of `p`.
    fn caller_path(&self, p: &Path) -> PathBuf {
        if let Some(given) = self.files.get(p) {
            return given.clone();
        }
        match self.dir_root(p) {
            Some((root, given)) => match p.strip_prefix(root) {
                Ok(rest) if rest.as_os_str().is_empty() => given.clone(),
                Ok(rest) => given.join(rest),
                Err(_) => p.to_path_buf(),
            },
            None => p.to_path_buf(),
        }
    }

    /// Our own save is still what is on disk.
    fn is_own_write(&self, p: &Path) -> bool {
        self.own.is_own(p)
    }

    /// Notify watches needed: recursive roots, plus the parent of each file outside them.
    fn notify_roots(&self) -> HashMap<PathBuf, RecursiveMode> {
        let mut roots = HashMap::new();
        for r in self.dirs.keys() {
            let nested = self.dirs.keys().any(|o| o != r && r.starts_with(o));
            if !nested {
                roots.insert(r.clone(), RecursiveMode::Recursive);
            }
        }
        for f in self.files.keys() {
            if let Some(parent) = f.parent()
                && self.dir_root(parent).is_none()
            {
                roots
                    .entry(parent.to_path_buf())
                    .or_insert(RecursiveMode::NonRecursive);
            }
        }
        roots
    }
}

pub struct Watcher {
    debouncer: Debouncer<RecommendedWatcher, RecommendedCache>,
    scope: Arc<Mutex<Scope>>,
    active: HashMap<PathBuf, RecursiveMode>,
}

impl Watcher {
    /// Starts the debouncer thread; it stops when the `Watcher` is dropped.
    pub fn new(tx: Sender<FsEvent>) -> Result<Self, WatchError> {
        let scope = Arc::new(Mutex::new(Scope::default()));
        let shared = Arc::clone(&scope);
        let debouncer = new_debouncer(DEBOUNCE, None, move |res: DebounceEventResult| {
            deliver(res, &shared, &tx);
        })?;
        Ok(Self {
            debouncer,
            scope,
            active: HashMap::new(),
        })
    }

    /// A directory is watched recursively (every path under it is reported); a file
    /// (existing or not yet created) is reported only for itself. Symlinks are followed and
    /// events use the path given here.
    pub fn watch(&mut self, p: &Path) -> Result<(), WatchError> {
        let is_dir = fs::metadata(p).is_ok_and(|m| m.is_dir());
        let resolved = resolve(p).map_err(|e| WatchError::Path(p.to_path_buf(), e))?;
        {
            let mut scope = lock(&self.scope);
            if is_dir {
                scope.dirs.insert(resolved.clone(), p.to_path_buf());
            } else {
                scope.files.insert(resolved.clone(), p.to_path_buf());
            }
        }
        if let Err(e) = self.sync() {
            let mut scope = lock(&self.scope);
            scope.dirs.remove(&resolved);
            scope.files.remove(&resolved);
            drop(scope);
            let _ = self.sync();
            return Err(e);
        }
        Ok(())
    }

    pub fn unwatch(&mut self, p: &Path) {
        {
            let mut scope = lock(&self.scope);
            scope.files.retain(|_, given| given != p);
            scope.dirs.retain(|_, given| given != p);
            let scope = &*scope;
            scope
                .own
                .retain(|r| scope.files.contains_key(r) || scope.dir_root(r).is_some());
        }
        if let Err(e) = self.sync() {
            tracing::warn!(path = %p.display(), error = %e, "unwatch could not update watches");
        }
    }

    /// Hand this to `Document::set_own_writes` so the document's saves are not reported back.
    pub fn own_writes(&self) -> OwnWrites {
        lock(&self.scope).own.clone()
    }

    fn sync(&mut self) -> Result<(), WatchError> {
        let wanted = lock(&self.scope).notify_roots();
        let stale: Vec<PathBuf> = self
            .active
            .iter()
            .filter(|(path, mode)| wanted.get(*path) != Some(*mode))
            .map(|(path, _)| path.clone())
            .collect();
        for path in stale {
            // Fails when the directory is already gone, which ends the watch anyway.
            let _ = self.debouncer.unwatch(&path);
            self.active.remove(&path);
        }
        for (path, mode) in wanted {
            if !self.active.contains_key(&path) {
                self.debouncer.watch(&path, mode)?;
                self.active.insert(path, mode);
            }
        }
        Ok(())
    }
}

/// Sends one batch to the caller; false once the receiver is gone.
fn deliver(res: DebounceEventResult, scope: &Mutex<Scope>, tx: &Sender<FsEvent>) -> bool {
    let out = match res {
        Ok(events) => translate(&events, &lock(scope)),
        Err(errors) => errors
            .into_iter()
            .map(|e| {
                tracing::warn!(error = %e, "file watcher error");
                FsEvent::Error(e.to_string())
            })
            .collect(),
    };
    out.into_iter().all(|e| tx.send(e).is_ok())
}

fn lock(scope: &Mutex<Scope>) -> MutexGuard<'_, Scope> {
    scope
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Canonical path; for a path that does not exist yet, its canonical parent joined with
/// the file name.
fn resolve(p: &Path) -> io::Result<PathBuf> {
    match fs::canonicalize(p) {
        Ok(r) => Ok(r),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let name = p
                .file_name()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
            let parent = match p.parent() {
                Some(d) if !d.as_os_str().is_empty() => d,
                _ => Path::new("."),
            };
            Ok(fs::canonicalize(parent)?.join(name))
        }
        Err(e) => Err(e),
    }
}

/// One `FsEvent` per path for a debounced batch; the final kind of a touched path is read
/// from the file system, which also folds create+modify and delete+recreate bursts.
fn translate(events: &[DebouncedEvent], scope: &Scope) -> Vec<FsEvent> {
    let mut renames = Vec::new();
    let mut touched: Vec<PathBuf> = Vec::new();
    let mut created: HashSet<PathBuf> = HashSet::new();
    let mut touch = |p: &Path, is_create: bool, touched: &mut Vec<PathBuf>| {
        if !touched.iter().any(|t| t == p) {
            touched.push(p.to_path_buf());
        }
        if is_create {
            created.insert(p.to_path_buf());
        }
    };
    let mut rescan = false;
    for ev in events {
        if ev.event.need_rescan() {
            rescan = true;
            continue;
        }
        match ev.event.kind {
            EventKind::Access(_)
            | EventKind::Modify(ModifyKind::Metadata(_))
            | EventKind::Other => {}
            EventKind::Modify(ModifyKind::Name(RenameMode::Both)) if ev.event.paths.len() == 2 => {
                let (from, to) = (&ev.event.paths[0], &ev.event.paths[1]);
                if is_save_temp(from) || !scope.contains(from) {
                    // Content moved into place: for `to` this is new content, not a rename
                    // (a watched file reports it as Modified, a tree path as Created).
                    touch(to, true, &mut touched);
                } else {
                    renames.push((from.clone(), to.clone()));
                }
            }
            // Moved in from outside the watched scope.
            EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
                for p in &ev.event.paths {
                    touch(p, true, &mut touched);
                }
            }
            _ => {
                for p in &ev.event.paths {
                    touch(p, false, &mut touched);
                }
            }
        }
    }

    let mut out = Vec::new();
    let mut done: HashSet<PathBuf> = HashSet::new();
    if rescan {
        // Events were lost: every open file must be re-checked and every tree re-read.
        for (resolved, given) in &scope.files {
            out.push(if fs::symlink_metadata(resolved).is_ok() {
                FsEvent::Modified(given.clone())
            } else {
                FsEvent::Removed(given.clone())
            });
            done.insert(resolved.clone());
        }
        out.extend(
            scope
                .dirs
                .values()
                .map(|given| FsEvent::Rescan(given.clone())),
        );
    }
    for (from, to) in renames {
        out.push(FsEvent::Renamed(
            scope.caller_path(&from),
            scope.caller_path(&to),
        ));
        // An open document at `to` was replaced by the rename.
        if scope.files.contains_key(&to) {
            out.push(FsEvent::Modified(scope.caller_path(&to)));
        }
        done.insert(from);
        done.insert(to);
    }
    for p in touched {
        if is_save_temp(&p) || done.contains(&p) || !scope.contains(&p) {
            continue;
        }
        let ev = if fs::symlink_metadata(&p).is_err() {
            FsEvent::Removed(scope.caller_path(&p))
        } else if scope.is_own_write(&p) {
            continue;
        } else if created.contains(&p) && !scope.files.contains_key(&p) {
            FsEvent::Created(scope.caller_path(&p))
        } else {
            FsEvent::Modified(scope.caller_path(&p))
        };
        out.push(ev);
    }
    out
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::sync::mpsc::{self, Receiver};
    use std::time::Instant;

    use super::*;
    use crate::document::Document;
    use crate::motion;

    /// Everything that arrives within `d`.
    fn collect(rx: &Receiver<FsEvent>, d: Duration) -> Vec<FsEvent> {
        let end = Instant::now() + d;
        let mut out = Vec::new();
        while let Some(left) = end.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(e) => out.push(e),
                Err(_) => break,
            }
        }
        out
    }

    /// The first event within `d`, and how long it took.
    fn first(rx: &Receiver<FsEvent>, d: Duration) -> Option<(FsEvent, Duration)> {
        let t = Instant::now();
        rx.recv_timeout(d).ok().map(|e| (e, t.elapsed()))
    }

    fn setup() -> (tempfile::TempDir, Watcher, Receiver<FsEvent>) {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        (dir, Watcher::new(tx).unwrap(), rx)
    }

    #[test]
    fn watch_modified_event() {
        let (dir, mut w, rx) = setup();
        let p = dir.path().join("a.txt");
        fs::write(&p, "1").unwrap();
        w.watch(&p).unwrap();
        fs::write(&p, "2").unwrap();
        let (ev, took) = first(&rx, Duration::from_secs(1)).expect("event within 1 s");
        assert_eq!(ev, FsEvent::Modified(p.clone()));
        assert!(took < Duration::from_secs(1));
        assert_eq!(collect(&rx, Duration::from_millis(400)), vec![]);
    }

    #[test]
    fn watch_removed_event() {
        let (dir, mut w, rx) = setup();
        let p = dir.path().join("a.txt");
        fs::write(&p, "1").unwrap();
        w.watch(&p).unwrap();
        fs::remove_file(&p).unwrap();
        let (ev, _) = first(&rx, Duration::from_secs(1)).expect("event within 1 s");
        assert_eq!(ev, FsEvent::Removed(p));
    }

    #[test]
    fn watch_debounces_burst() {
        let (dir, mut w, rx) = setup();
        let p = dir.path().join("a.txt");
        fs::write(&p, "0").unwrap();
        w.watch(&p).unwrap();
        for i in 0..20 {
            fs::write(&p, i.to_string()).unwrap();
            std::thread::sleep(Duration::from_millis(5));
        }
        let events = collect(&rx, Duration::from_millis(1200));
        assert!(
            (1..=2).contains(&events.len()),
            "20 writes in ~100 ms → 1–2 events, got {events:?}"
        );
        assert!(events.iter().all(|e| *e == FsEvent::Modified(p.clone())));
    }

    #[test]
    fn watch_atomic_replace_by_other_tool_is_modified() {
        let (dir, mut w, rx) = setup();
        let p = dir.path().join("a.txt");
        fs::write(&p, "old").unwrap();
        w.watch(&p).unwrap();
        // What AI agents and most editors do: write a sibling temp file, rename it over.
        let tmp = dir.path().join("a.txt.agent-tmp");
        fs::write(&tmp, "new").unwrap();
        fs::rename(&tmp, &p).unwrap();
        let events = collect(&rx, Duration::from_millis(900));
        assert_eq!(events, vec![FsEvent::Modified(p)]);
    }

    #[test]
    fn watch_ignores_own_save() {
        let (dir, mut w, rx) = setup();
        let p = dir.path().join("a.txt");
        fs::write(&p, "a").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        // Registered once per document; every later save is suppressed by `save` itself.
        doc.set_own_writes(w.own_writes());
        w.watch(&p).unwrap();
        motion::doc_end(&mut doc.buffer, false);
        doc.buffer.insert_text("b");
        doc.save().unwrap();
        assert_eq!(collect(&rx, Duration::from_millis(700)), vec![]);
        // Someone else writing afterwards is reported again.
        fs::write(&p, "agent").unwrap();
        let (ev, _) = first(&rx, Duration::from_secs(1)).expect("event within 1 s");
        assert_eq!(ev, FsEvent::Modified(p));
    }

    #[test]
    fn revert_to_our_saved_content_is_reported() {
        let (dir, mut w, rx) = setup();
        let p = dir.path().join("a.txt");
        fs::write(&p, "base\n").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        doc.set_own_writes(w.own_writes());
        w.watch(&p).unwrap();
        motion::doc_end(&mut doc.buffer, false);
        doc.buffer.insert_text("C1");
        doc.save().unwrap();
        let saved = fs::read(&p).unwrap();
        assert_eq!(collect(&rx, Duration::from_millis(600)), vec![]);
        fs::write(&p, "agent edit\n").unwrap();
        assert_eq!(
            collect(&rx, Duration::from_millis(600)),
            vec![FsEvent::Modified(p.clone())]
        );
        doc.reload().unwrap();
        // `git checkout` (or the agent undoing itself) puts our earlier save back.
        fs::write(&p, &saved).unwrap();
        assert_eq!(
            collect(&rx, Duration::from_millis(600)),
            vec![FsEvent::Modified(p.clone())]
        );
        assert_eq!(doc.check_disk(), crate::document::DiskState::ChangedOnDisk);
    }

    #[test]
    fn revert_without_reload_is_reported() {
        let (dir, mut w, rx) = setup();
        let p = dir.path().join("a.txt");
        fs::write(&p, "base\n").unwrap();
        let mut doc = Document::open(&p, 1 << 20).unwrap();
        doc.set_own_writes(w.own_writes());
        w.watch(&p).unwrap();
        motion::doc_end(&mut doc.buffer, false);
        doc.buffer.insert_text("C1");
        doc.save().unwrap();
        let saved = fs::read(&p).unwrap();
        assert_eq!(collect(&rx, Duration::from_millis(600)), vec![]);
        // Rewritten with identical bytes by someone else: a new write, not our save.
        fs::write(&p, &saved).unwrap();
        assert_eq!(
            collect(&rx, Duration::from_millis(600)),
            vec![FsEvent::Modified(p.clone())]
        );
    }

    #[test]
    fn watch_dir_reports_create_rename_remove() {
        let (dir, mut w, rx) = setup();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("sub").join("b.txt");
        fs::create_dir(dir.path().join("sub")).unwrap();
        w.watch(dir.path()).unwrap();
        fs::write(&a, "x").unwrap();
        assert_eq!(
            collect(&rx, Duration::from_millis(700)),
            vec![FsEvent::Created(a.clone())]
        );
        fs::rename(&a, &b).unwrap();
        assert_eq!(
            collect(&rx, Duration::from_millis(700)),
            vec![FsEvent::Renamed(a, b.clone())]
        );
        fs::remove_file(&b).unwrap();
        assert_eq!(
            collect(&rx, Duration::from_millis(700)),
            vec![FsEvent::Removed(b)]
        );
    }

    #[test]
    fn watch_filters_siblings_and_unwatch_stops() {
        let (dir, mut w, rx) = setup();
        let p = dir.path().join("a.txt");
        let sibling = dir.path().join("b.txt");
        fs::write(&p, "1").unwrap();
        w.watch(&p).unwrap();
        fs::write(&sibling, "x").unwrap();
        assert_eq!(collect(&rx, Duration::from_millis(600)), vec![]);
        w.unwatch(&p);
        fs::write(&p, "2").unwrap();
        assert_eq!(collect(&rx, Duration::from_millis(600)), vec![]);
    }

    #[test]
    fn watch_symlinked_file_reports_link_path() {
        let (dir, mut w, rx) = setup();
        let real = dir.path().join("real");
        fs::create_dir(&real).unwrap();
        let target = real.join("t.txt");
        fs::write(&target, "1").unwrap();
        let link = dir.path().join("link.txt");
        symlink(&target, &link).unwrap();
        w.watch(&link).unwrap();
        fs::write(&target, "2").unwrap();
        let (ev, _) = first(&rx, Duration::from_secs(1)).expect("event within 1 s");
        assert_eq!(ev, FsEvent::Modified(link));
    }

    #[test]
    fn rescan_reports_every_watched_file() {
        let (dir, mut w, _rx) = setup();
        let present = dir.path().join("a.txt");
        fs::write(&present, "1").unwrap();
        let gone = dir.path().join("gone.txt");
        let sub = tempfile::tempdir().unwrap();
        w.watch(&present).unwrap();
        w.watch(&gone).unwrap();
        w.watch(sub.path()).unwrap();
        // What notify-debouncer-full sends after an inotify queue overflow.
        let overflow = DebouncedEvent {
            event: notify::Event::new(EventKind::Other).set_flag(notify::event::Flag::Rescan),
            time: Instant::now(),
        };
        let mut out = translate(&[overflow], &lock(&w.scope));
        out.sort_by_key(|e| format!("{e:?}"));
        let mut want = vec![
            FsEvent::Modified(present),
            FsEvent::Removed(gone),
            FsEvent::Rescan(sub.path().to_path_buf()),
        ];
        want.sort_by_key(|e| format!("{e:?}"));
        assert_eq!(out, want);
    }

    #[test]
    fn watcher_errors_reach_the_caller() {
        let scope = Mutex::new(Scope::default());
        let (tx, rx) = mpsc::channel();
        let err = notify::Error::generic("inotify watch limit reached");
        assert!(deliver(Err(vec![err]), &scope, &tx));
        match rx.try_recv() {
            Ok(FsEvent::Error(m)) => assert!(m.contains("watch limit"), "{m}"),
            other => panic!("expected FsEvent::Error, got {other:?}"),
        }
    }

    #[test]
    fn watch_move_in_from_outside_is_created() {
        let (dir, mut w, rx) = setup();
        let outside = tempfile::tempdir().unwrap();
        let src = outside.path().join("x.php");
        fs::write(&src, "<?php").unwrap();
        w.watch(dir.path()).unwrap();
        let dest = dir.path().join("x.php");
        fs::rename(&src, &dest).unwrap();
        assert_eq!(
            collect(&rx, Duration::from_millis(700)),
            vec![FsEvent::Created(dest)]
        );
    }
}
