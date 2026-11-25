//! Git context in the editor (M7-4a; D297 "In the editor", "Multi-root"): gutter marks of the
//! active file against the index (unsaved text included), the branch and ahead/behind in the
//! status bar (with a "parent repo" label, D298) and the inline blame of the cursor line.
//!
//! Each workspace root gets its repo found off the UI thread ([`vr_git::Repo::discover`] runs
//! git); a root without git shows nothing. Every query goes to the repo's
//! [`vr_git::GitWorker`] and comes back through [`tick`], so the UI thread never waits for git
//! (AC-1, AC-16e). Results are bound to the repo they were asked of (a serial per repo) and to
//! the buffer version they describe (SEC-17): a reply from another repo or for older text is
//! never drawn as current.
//!
//! What changes git's answers: the buffer (debounced, [`DEBOUNCE`]), the file on disk (the
//! app's file watcher, [`on_fs_event`]) and `.git` itself — HEAD, the index, refs incl.
//! `refs/remotes` for ahead/behind — watched here by a watcher of its own so the explorer's
//! folder watches stay untouched. vr-git's reads never write under `.git` (SEC-8), so these
//! watches cannot loop.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use vr_core::{Encoding, FsEvent, Watcher};
use vr_git::{AheadBehind, Blame, GitError, GitWorker, Head, LineChange, Reply, Repo, Request};

use crate::app::App;
use crate::ui::safe_text::sanitize;

/// Quiet time after an edit before the gutter (and the blame) are asked for again.
pub const DEBOUNCE: Duration = Duration::from_millis(250);

/// How long the cursor rests on a line before its blame is shown (spec: ~300 ms).
pub const REST: Duration = Duration::from_millis(300);

/// Branch names longer than this are cut in the status bar (full / short form).
const BRANCH_FULL: usize = 32;
const BRANCH_SHORT: usize = 16;

/// A discovery result from its thread.
struct Discovered {
    root: PathBuf,
    generation: u64,
    repo: Option<Repo>,
    /// `~`-relative work tree when the repo was found above the root (D298).
    parent: Option<String>,
}

/// A running discovery thread. Its git runner lives on that thread, out of reach of
/// [`GitState::shutdown`], so shutdown finds the thread's children by its id and kills them.
struct Discovery {
    /// Linux thread id, set first thing on the thread (0 until then or when unknown).
    tid: Arc<AtomicI32>,
    handle: JoinHandle<()>,
}

/// How long shutdown keeps killing the gits of discoveries that are still running.
const DISCOVERY_KILL_WAIT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq)]
enum RootState {
    Discovering(u64),
    NoRepo,
    Repo { serial: u64, parent: Option<String> },
}

struct RootGit {
    path: PathBuf,
    state: RootState,
    /// Its unborn `.git` changed while a discovery ran: look again when it answers "none".
    look_again: bool,
}

enum Pending {
    Head,
    AheadBehind,
    Marks(PathBuf, u64),
    Blame(PathBuf, u64),
}

struct RepoEntry {
    /// Identity of this repo for the app's lifetime; results carry it.
    serial: u64,
    worker: GitWorker,
    /// Shares the worker's runner: `shutdown` on it is the worker's (tests check it).
    repo: Repo,
    git_dirs: Vec<PathBuf>,
    /// What this repo added to the git watcher, to take off again.
    flat_watches: Vec<PathBuf>,
    tree_watches: Vec<PathBuf>,
    head: Option<Head>,
    ahead: Option<AheadBehind>,
    pending: HashMap<u64, Pending>,
}

/// Gutter marks of one file.
struct MarkSet {
    serial: u64,
    version: u64,
    /// False once the file or the repo changed after they were computed: still drawn (they
    /// are replaced shortly), not current.
    fresh: bool,
    changes: Vec<LineChange>,
}

struct BlameSet {
    serial: u64,
    path: PathBuf,
    version: u64,
    blame: Option<Blame>,
}

/// The shown inline blame and what it belongs to.
struct Inline {
    path: PathBuf,
    version: u64,
    line: usize,
    text: String,
}

/// What [`schedule`] saw last of the active document.
#[derive(Default)]
struct Track {
    /// Path and buffer version, and since when (`None`: ask at once, a newly shown file).
    key: Option<(PathBuf, u64)>,
    since: Option<Instant>,
    /// Path, version and cursor line, and since when it rests.
    cursor: Option<(PathBuf, u64, usize)>,
    cursor_since: Option<Instant>,
}

/// The status bar's git text in two lengths; [`crate::ui::status_bar`] picks what fits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLabel {
    pub full: String,
    pub short: String,
}

pub struct GitState {
    started: bool,
    /// Limits and environment of the git runner (tests point it at a hermetic home).
    pub options: vr_git::Options,
    /// [`DEBOUNCE`] and [`REST`]; tests set their own.
    pub debounce: Duration,
    pub rest: Duration,
    /// Wall clock for "3 days ago" (Unix seconds); `None` = now. Tests fix it.
    pub now_secs: Option<i64>,
    roots: Vec<RootGit>,
    repos: Vec<RepoEntry>,
    next_serial: u64,
    next_generation: u64,
    tx: Sender<Discovered>,
    rx: Receiver<Discovered>,
    discoveries: Vec<Discovery>,
    /// Roots whose own `.git` folder git does not take (yet): `git init` / `git clone` still
    /// filling it. Root and its `.git`, watched flat; a change there looks again.
    unborn_git: Vec<(PathBuf, PathBuf)>,
    /// Set by shutdown: a discovery thread that has not run git yet does not start it.
    closed: Arc<AtomicBool>,
    watcher: Option<Watcher>,
    fs_rx: Option<Receiver<FsEvent>>,
    marks: HashMap<PathBuf, MarkSet>,
    asked_marks: HashMap<PathBuf, (u64, u64)>,
    asked_blame: Option<(u64, PathBuf, u64)>,
    blame: Option<BlameSet>,
    inline: Option<Inline>,
    track: Track,
    sent: u64,
    /// Messages for the notice line (a `.git` folder that cannot be watched), taken by tick.
    warnings: Vec<String>,
}

impl Default for GitState {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        GitState {
            started: false,
            options: vr_git::Options::default(),
            debounce: DEBOUNCE,
            rest: REST,
            now_secs: None,
            roots: Vec::new(),
            repos: Vec::new(),
            next_serial: 0,
            next_generation: 0,
            tx,
            rx,
            discoveries: Vec::new(),
            unborn_git: Vec::new(),
            closed: Arc::new(AtomicBool::new(false)),
            watcher: None,
            fs_rx: None,
            marks: HashMap::new(),
            asked_marks: HashMap::new(),
            asked_blame: None,
            blame: None,
            inline: None,
            track: Track::default(),
            sent: 0,
            warnings: Vec::new(),
        }
    }
}

impl GitState {
    /// The repo of the root holding `path` (roots never nest, so the first match is it).
    fn serial_for(&self, path: &Path) -> Option<u64> {
        let root = self.roots.iter().find(|r| path.starts_with(&r.path))?;
        match root.state {
            RootState::Repo { serial, .. } => Some(serial),
            _ => None,
        }
    }

    fn root_state(&self, path: &Path) -> Option<&RootState> {
        self.roots
            .iter()
            .find(|r| path.starts_with(&r.path))
            .map(|r| &r.state)
    }

    fn entry(&self, serial: u64) -> Option<&RepoEntry> {
        self.repos.iter().find(|e| e.serial == serial)
    }

    fn entry_mut(&mut self, serial: u64) -> Option<&mut RepoEntry> {
        self.repos.iter_mut().find(|e| e.serial == serial)
    }

    fn request(&mut self, serial: u64, req: Request, pending: Pending) {
        if let Some(e) = self.entry_mut(serial) {
            let id = e.worker.request(req);
            e.pending.insert(id, pending);
            self.sent += 1;
        }
    }

    /// Current gutter marks of `path`: the version of the text they describe and the marks.
    /// `None` until they arrive, after a change made them stale, or without a repo.
    pub fn marks_for(&self, path: &Path) -> Option<(u64, &[LineChange])> {
        let m = self.marks.get(path)?;
        (m.fresh && Some(m.serial) == self.serial_for(path)).then_some((m.version, &m.changes))
    }

    /// Marks to draw for `path`: the latest of its repo, also while a newer answer is on its
    /// way (no flicker while typing).
    pub fn shown_marks(&self, path: &Path) -> Option<&[LineChange]> {
        let m = self.marks.get(path)?;
        (Some(m.serial) == self.serial_for(path)).then_some(m.changes.as_slice())
    }

    /// A root's discovery is still running.
    pub fn discovering(&self) -> bool {
        self.roots
            .iter()
            .any(|r| matches!(r.state, RootState::Discovering(_)))
    }

    /// Repos with a running worker.
    pub fn repo_count(&self) -> usize {
        self.repos.len()
    }

    /// Requests sent to the workers so far (tests check the debounce).
    pub fn requests_sent(&self) -> u64 {
        self.sent
    }

    /// The repo of root `root`, for reads only: it shares the worker's runner, so removing the
    /// root, replacing the workspace or exiting kills whatever it runs. Writes (M7-3's popup)
    /// use `vr_git::Writer::open` with a runner of their own, never this clone (D304).
    pub fn repo_of_root(&self, root: &Path) -> Option<Repo> {
        let r = self.roots.iter().find(|r| r.path == root)?;
        match r.state {
            RootState::Repo { serial, .. } => self.entry(serial).map(|e| e.repo.clone()),
            _ => None,
        }
    }

    /// The repo of root `root` and, when it was found above the root, its `~` path (D298), for
    /// the Git popup. The popup writes through its own `vr_git::Writer` (D304).
    pub fn repo_info_of_root(&self, root: &Path) -> Option<(Repo, Option<String>)> {
        let r = self.roots.iter().find(|r| r.path == root)?;
        match &r.state {
            RootState::Repo { serial, parent } => self
                .entry(*serial)
                .map(|e| (e.repo.clone(), parent.clone())),
            _ => None,
        }
    }

    /// The blame of the active document's current text has arrived (as of the last tick).
    pub fn blame_ready(&self) -> bool {
        let (Some((path, version)), Some(b)) = (&self.track.key, &self.blame) else {
            return false;
        };
        b.path == *path && b.version == *version && Some(b.serial) == self.serial_for(path)
    }

    /// Kills every running git and stops every worker and watch (exit, workspace closed).
    pub fn shutdown(&mut self) {
        for e in &self.repos {
            e.worker.shutdown();
        }
        self.closed.store(true, Ordering::SeqCst);
        // Threads already running keep the old flag; a later start gets a fresh one.
        self.closed = Arc::new(AtomicBool::new(false));
        kill_discoveries(std::mem::take(&mut self.discoveries));
        self.repos.clear();
        self.roots.clear();
        self.watcher = None;
        self.unborn_git.clear();
        self.fs_rx = None;
        self.marks.clear();
        self.asked_marks.clear();
        self.asked_blame = None;
        self.blame = None;
        self.inline = None;
        self.started = false;
    }

    /// Forgets what was asked of repo `serial` (it changed): the next tick asks again.
    fn forget_repo(&mut self, serial: u64) {
        self.asked_marks.retain(|_, (s, _)| *s != serial);
        if self
            .asked_blame
            .as_ref()
            .is_some_and(|(s, ..)| *s == serial)
        {
            self.asked_blame = None;
        }
        for m in self.marks.values_mut().filter(|m| m.serial == serial) {
            m.fresh = false;
        }
    }

    /// A document on `path` was opened or closed: a new document starts at version 0 again,
    /// so nothing asked or answered for an earlier one may stand for it (the file may have
    /// changed while no tab and no watch had it).
    pub fn forget_doc(&mut self, path: &Path) {
        self.asked_marks.remove(path);
        self.marks.remove(path);
        if self.asked_blame.as_ref().is_some_and(|(_, p, _)| p == path) {
            self.asked_blame = None;
        }
        if self.blame.as_ref().is_some_and(|b| b.path == path) {
            self.blame = None;
        }
        if self.inline.as_ref().is_some_and(|i| i.path == path) {
            self.inline = None;
        }
    }

    /// Forgets what was asked about `path` (the file changed on disk).
    fn forget_path(&mut self, path: &Path) {
        self.asked_marks.remove(path);
        if self.asked_blame.as_ref().is_some_and(|(_, p, _)| p == path) {
            self.asked_blame = None;
        }
        if let Some(m) = self.marks.get_mut(path) {
            m.fresh = false;
        }
    }
}

impl Drop for GitState {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// ---- roots and discovery ------------------------------------------------------------------

/// Starts git for the workspace roots (main calls it after the first frame, AC-1).
pub fn start(app: &mut App) {
    app.git.started = true;
    roots_changed(app);
}

/// Brings the per-root repos in step with the workspace: new roots are discovered off the UI
/// thread, removed ones end their worker (when no other root uses the repo).
pub fn roots_changed(app: &mut App) {
    let git = &mut app.git;
    if !git.started {
        return;
    }
    let paths = app.workspace.paths();
    let mut old = std::mem::take(&mut git.roots);
    for p in paths {
        if let Some(i) = old.iter().position(|r| r.path == p) {
            git.roots.push(old.remove(i));
            continue;
        }
        let state = discover(git, &p);
        git.roots.push(RootGit {
            path: p,
            state,
            look_again: false,
        });
    }
    for r in &old {
        unwatch_unborn(git, &r.path);
    }
    drop_unused_repos(git);
    app.dirty = true;
}

/// Starts finding the repo of `root` (a new generation: an older answer is ignored).
fn discover(git: &mut GitState, root: &Path) -> RootState {
    git.next_generation += 1;
    let generation = git.next_generation;
    if spawn_discovery(git, root, generation) {
        RootState::Discovering(generation)
    } else {
        RootState::NoRepo
    }
}

fn spawn_discovery(git: &mut GitState, root: &Path, generation: u64) -> bool {
    git.discoveries.retain(|d| !d.handle.is_finished());
    let (tx, opts, root) = (git.tx.clone(), git.options.clone(), root.to_path_buf());
    let tid = Arc::new(AtomicI32::new(0));
    let (my_tid, closed) = (tid.clone(), git.closed.clone());
    let spawned = std::thread::Builder::new()
        .name("vr-git-discover".into())
        .spawn(move || {
            my_tid.store(current_tid(), Ordering::SeqCst);
            if closed.load(Ordering::SeqCst) {
                return;
            }
            // Errors (no git binary, a refused repo) mean "no repo": nothing is shown.
            let repo = Repo::discover_with(&root, opts).ok().flatten();
            let parent = repo
                .as_ref()
                .filter(|r| r.is_parent_repo_of(&root))
                .map(|r| home_relative(r.workdir()));
            let _ = tx.send(Discovered {
                root,
                generation,
                repo,
                parent,
            });
        });
    match spawned {
        Ok(handle) => {
            git.discoveries.push(Discovery { tid, handle });
            true
        }
        Err(_) => false,
    }
}

/// The calling thread's kernel id (Linux); 0 elsewhere.
fn current_tid() -> i32 {
    std::fs::read_link("/proc/thread-self")
        .ok()
        .and_then(|l| l.file_name()?.to_str()?.parse().ok())
        .unwrap_or(0)
}

/// Kills the gits that the still running discovery threads started, with their process
/// groups (the runner makes each git a session leader), until the threads end or
/// [`DISCOVERY_KILL_WAIT`] passes. A thread's children are listed by the kernel per thread
/// (`/proc/self/task/<tid>/children`), so only gits of these threads are hit.
fn kill_discoveries(mut running: Vec<Discovery>) {
    let end = Instant::now() + DISCOVERY_KILL_WAIT;
    loop {
        running.retain(|d| !d.handle.is_finished());
        if running.is_empty() || Instant::now() >= end {
            return;
        }
        for d in &running {
            let tid = d.tid.load(Ordering::SeqCst);
            if tid > 0 {
                kill_children_of_thread(tid);
            }
        }
        // A killed git ends its runner call at once; the thread then returns.
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn kill_children_of_thread(tid: i32) {
    use rustix::process::{Pid, Signal, getpgid, kill_process, kill_process_group};
    let Ok(list) = std::fs::read_to_string(format!("/proc/self/task/{tid}/children")) else {
        return;
    };
    for pid in list
        .split_whitespace()
        .filter_map(|s| s.parse::<i32>().ok())
        .filter_map(Pid::from_raw)
    {
        if getpgid(Some(pid)).is_ok_and(|g| g == pid) {
            let _ = kill_process_group(pid, Signal::KILL);
        } else {
            let _ = kill_process(pid, Signal::KILL);
        }
    }
}

/// `path` with the home folder written `~`, sanitized for display.
pub(crate) fn home_relative(path: &Path) -> String {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let shown = match home.as_deref().and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    };
    sanitize(&shown)
}

fn drop_unused_repos(git: &mut GitState) {
    let used: Vec<u64> = git
        .roots
        .iter()
        .filter_map(|r| match r.state {
            RootState::Repo { serial, .. } => Some(serial),
            _ => None,
        })
        .collect();
    let (keep, gone): (Vec<RepoEntry>, Vec<RepoEntry>) = std::mem::take(&mut git.repos)
        .into_iter()
        .partition(|e| used.contains(&e.serial));
    git.repos = keep;
    for e in gone {
        e.worker.shutdown();
        if let Some(w) = &mut git.watcher {
            for d in &e.flat_watches {
                w.unwatch_dir(d);
            }
            for d in &e.tree_watches {
                w.unwatch(d);
            }
        }
        git.marks.retain(|_, m| m.serial != e.serial);
        git.forget_repo(e.serial);
    }
}

fn drain_discovered(app: &mut App) {
    let found: Vec<Discovered> = app.git.rx.try_iter().collect();
    for d in found {
        let git = &mut app.git;
        let Some(i) = git
            .roots
            .iter()
            .position(|r| r.path == d.root && r.state == RootState::Discovering(d.generation))
        else {
            // The root was removed (or re-added) meanwhile: nothing runs on this repo yet.
            continue;
        };
        let look_again = std::mem::take(&mut git.roots[i].look_again);
        let Some(repo) = d.repo else {
            git.roots[i].state = if look_again {
                discover(git, &d.root)
            } else {
                RootState::NoRepo
            };
            watch_unborn(git, &d.root);
            continue;
        };
        unwatch_unborn(git, &d.root);
        // Two roots inside one repo share its worker.
        let serial = match git
            .repos
            .iter()
            .find(|e| e.repo.workdir() == repo.workdir())
        {
            Some(e) => e.serial,
            None => add_repo(git, repo),
        };
        git.roots[i].state = RootState::Repo {
            serial,
            parent: d.parent,
        };
        app.dirty = true;
    }
}

fn add_repo(git: &mut GitState, repo: Repo) -> u64 {
    git.next_serial += 1;
    let serial = git.next_serial;
    let mut git_dirs = vec![repo.git_dir().to_path_buf()];
    if repo.common_dir() != repo.git_dir() {
        git_dirs.push(repo.common_dir().to_path_buf());
    }
    let worker = GitWorker::start(repo.clone(), || {});
    let mut entry = RepoEntry {
        serial,
        worker,
        repo,
        git_dirs,
        flat_watches: Vec::new(),
        tree_watches: Vec::new(),
        head: None,
        ahead: None,
        pending: HashMap::new(),
    };
    watch_repo(git, &mut entry);
    git.repos.push(entry);
    git.request(serial, Request::Head, Pending::Head);
    git.request(serial, Request::AheadBehind, Pending::AheadBehind);
    serial
}

/// HEAD, the index, packed-refs and FETCH_HEAD are entries of the git dir (one flat watch);
/// branch and remote refs live under `refs/` (one recursive watch, a small tree).
/// The `.git` watcher, made on first use (a failure is a notice, once per try).
fn git_watcher(git: &mut GitState) -> Option<&mut Watcher> {
    if git.watcher.is_none() {
        let (tx, rx) = mpsc::channel();
        match Watcher::new(tx) {
            Ok(w) => {
                git.watcher = Some(w);
                git.fs_rx = Some(rx);
            }
            Err(err) => {
                git.warnings.push(format!(
                    "git: cannot watch .git ({err}); branch and marks refresh after edits only"
                ));
                return None;
            }
        }
    }
    git.watcher.as_mut()
}

/// `<root>/.git` is a folder but no repo was found: git is still making it (the root's
/// own watch saw it appear before HEAD was written). Watching it finds the repo once done.
fn watch_unborn(git: &mut GitState, root: &Path) {
    let dot_git = root.join(".git");
    let is_dir = std::fs::symlink_metadata(&dot_git).is_ok_and(|m| m.is_dir());
    if !is_dir || git.unborn_git.iter().any(|(r, _)| r == root) {
        return;
    }
    let Some(w) = git_watcher(git) else {
        return;
    };
    // Unwatchable: the root's own watch still sees `.git` go and come back.
    if w.watch_dir(&dot_git).is_ok() {
        git.unborn_git.push((root.to_path_buf(), dot_git));
    }
}

fn unwatch_unborn(git: &mut GitState, root: &Path) {
    if let Some(i) = git.unborn_git.iter().position(|(r, _)| r == root) {
        let (_, dot_git) = git.unborn_git.remove(i);
        if let Some(w) = &mut git.watcher {
            w.unwatch_dir(&dot_git);
        }
    }
}

fn watch_repo(git: &mut GitState, e: &mut RepoEntry) {
    let Some(w) = git_watcher(git) else {
        return;
    };
    let mut failed = Vec::new();
    for d in &e.git_dirs {
        match w.watch_dir(d) {
            Ok(()) => e.flat_watches.push(d.clone()),
            Err(err) => failed.push(err.to_string()),
        }
    }
    let refs = e.repo.common_dir().join("refs");
    match w.watch(&refs) {
        Ok(()) => e.tree_watches.push(refs),
        Err(err) => failed.push(err.to_string()),
    }
    for f in failed {
        git.warnings.push(format!(
            "git: {}; branch and marks refresh after edits only",
            sanitize(&f)
        ));
    }
}

// ---- events -------------------------------------------------------------------------------

/// A file-system event of the app's watcher (open files, shown folders): the file's cached
/// answers are dropped and asked again.
pub fn on_fs_event(app: &mut App, ev: &FsEvent) {
    if root_git_changed(app, ev) {
        app.dirty = true;
    }
    let git = &mut app.git;
    if git.repos.is_empty() {
        return;
    }
    let paths: Vec<PathBuf> = match ev {
        FsEvent::Modified(p) | FsEvent::Created(p) | FsEvent::Removed(p) => vec![p.clone()],
        FsEvent::Renamed(a, b) => vec![a.clone(), b.clone()],
        // Events were lost: every file may have changed.
        FsEvent::Rescan(_) => {
            for serial in git.repos.iter().map(|e| e.serial).collect::<Vec<_>>() {
                if let Some(e) = git.entry(serial) {
                    e.worker.invalidate();
                }
                git.forget_repo(serial);
            }
            return;
        }
        FsEvent::Error(_) => return,
    };
    for p in &paths {
        if let Some(serial) = git.serial_for(p) {
            if let Some(e) = git.entry(serial) {
                e.worker.invalidate_path(p);
            }
            git.forget_path(p);
        }
    }
    for p in &paths {
        crate::git_popup::on_change(app, p);
    }
}

/// `<root>/.git` appeared, went away or was renamed (`git init`, `git clone .`, `rm -rf
/// .git`), seen by the explorer's watch of each root: that root's repo is found again.
/// Returns whether a discovery started.
fn root_git_changed(app: &mut App, ev: &FsEvent) -> bool {
    let git = &mut app.git;
    if !git.started {
        return false;
    }
    let paths: Vec<&Path> = match ev {
        FsEvent::Created(p) | FsEvent::Removed(p) => vec![p],
        FsEvent::Renamed(a, b) => vec![a, b],
        FsEvent::Modified(_) | FsEvent::Rescan(_) | FsEvent::Error(_) => return false,
    };
    let hit: Vec<PathBuf> = git
        .roots
        .iter()
        .filter(|r| paths.iter().any(|p| *p == r.path.join(".git")))
        .map(|r| r.path.clone())
        .collect();
    for root in &hit {
        unwatch_unborn(git, root);
        let state = discover(git, root);
        if let Some(r) = git.roots.iter_mut().find(|r| r.path == *root) {
            r.state = state;
        }
    }
    if !hit.is_empty() {
        drop_unused_repos(git);
    }
    !hit.is_empty()
}

/// Events of the `.git` watcher: the repo's answers are all dropped, HEAD and upstream asked
/// again, and the open file's marks and blame on the next tick.
fn drain_git_events(app: &mut App) {
    let git = &mut app.git;
    let Some(rx) = &git.fs_rx else {
        return;
    };
    let events: Vec<FsEvent> = rx.try_iter().collect();
    if events.is_empty() {
        return;
    }
    // A `.git` being made changed: look for the repo again (one discovery per root per tick).
    let unborn: Vec<PathBuf> = git
        .unborn_git
        .iter()
        .filter(|(_, d)| events.iter().any(|ev| event_under(ev, d)))
        .map(|(r, _)| r.clone())
        .collect();
    for root in unborn {
        let Some(i) = git.roots.iter().position(|r| r.path == root) else {
            continue;
        };
        // `git clone` writes for a long time: one discovery at a time per root.
        if matches!(git.roots[i].state, RootState::Discovering(_)) {
            git.roots[i].look_again = true;
        } else {
            git.roots[i].state = discover(git, &root);
        }
    }
    let mut changed: Vec<u64> = Vec::new();
    for ev in &events {
        let paths: Vec<&Path> = match ev {
            FsEvent::Modified(p)
            | FsEvent::Created(p)
            | FsEvent::Removed(p)
            | FsEvent::Rescan(p) => {
                vec![p]
            }
            FsEvent::Renamed(a, b) => vec![a, b],
            FsEvent::Error(_) => {
                changed = git.repos.iter().map(|e| e.serial).collect();
                break;
            }
        };
        for e in &git.repos {
            if paths
                .iter()
                .any(|p| e.git_dirs.iter().any(|d| p.starts_with(d)))
                && !changed.contains(&e.serial)
            {
                changed.push(e.serial);
            }
        }
    }
    let mut workdirs = Vec::new();
    for serial in changed {
        if let Some(e) = git.entry(serial) {
            e.worker.invalidate();
            workdirs.push(e.repo.workdir().to_path_buf());
        }
        git.forget_repo(serial);
        git.request(serial, Request::Head, Pending::Head);
        git.request(serial, Request::AheadBehind, Pending::AheadBehind);
    }
    // The Git popup on that repo scans again too.
    for wd in workdirs {
        crate::git_popup::on_change(app, &wd);
    }
}

/// The Git popup wrote to the repo at `workdir`: gutter marks, blame and the status bar's
/// branch are asked again (the `.git` watcher may see it later or, on some filesystems, not).
pub fn repo_written(app: &mut App, workdir: &Path) {
    let git = &mut app.git;
    let serials: Vec<u64> = git
        .repos
        .iter()
        .filter(|e| e.repo.workdir() == workdir)
        .map(|e| e.serial)
        .collect();
    for serial in serials {
        if let Some(e) = git.entry(serial) {
            e.worker.invalidate();
        }
        git.forget_repo(serial);
        git.request(serial, Request::Head, Pending::Head);
        git.request(serial, Request::AheadBehind, Pending::AheadBehind);
    }
    app.dirty = true;
}

fn event_under(ev: &FsEvent, dir: &Path) -> bool {
    match ev {
        FsEvent::Modified(p) | FsEvent::Created(p) | FsEvent::Removed(p) | FsEvent::Rescan(p) => {
            p.starts_with(dir)
        }
        FsEvent::Renamed(a, b) => a.starts_with(dir) || b.starts_with(dir),
        FsEvent::Error(_) => false,
    }
}

fn drain_responses(app: &mut App) {
    let git = &mut app.git;
    let mut got = Vec::new();
    for e in &mut git.repos {
        while let Some(r) = e.worker.try_recv() {
            if let Some(p) = e.pending.remove(&r.id) {
                got.push((e.serial, p, r.result));
            }
        }
    }
    if got.is_empty() {
        return;
    }
    app.dirty = true;
    for (serial, pending, result) in got {
        // A newer request of the same kind is on its way; shutdown is the end.
        if matches!(result, Err(GitError::Superseded | GitError::ShutDown)) {
            continue;
        }
        if let Err(e) = &result {
            tracing::debug!(error = %e, "git read failed");
        }
        let reply = result.ok();
        match pending {
            Pending::Head => {
                let head = match reply.as_deref() {
                    Some(Reply::Head(h)) => Some(h.clone()),
                    _ => None,
                };
                if let Some(e) = git.entry_mut(serial) {
                    e.head = head;
                }
            }
            Pending::AheadBehind => {
                let ab = match reply.as_deref() {
                    Some(Reply::AheadBehind(a)) => a.clone(),
                    _ => None,
                };
                if let Some(e) = git.entry_mut(serial) {
                    e.ahead = ab;
                }
            }
            Pending::Marks(path, version) => match reply.as_deref() {
                Some(Reply::LineChanges(c)) => {
                    git.marks.insert(
                        path,
                        MarkSet {
                            serial,
                            version,
                            fresh: true,
                            changes: c.clone().unwrap_or_default(),
                        },
                    );
                }
                // A failed read (too large, timed out) shows no marks rather than stale ones.
                _ => {
                    git.marks.remove(&path);
                }
            },
            Pending::Blame(path, version) => {
                let blame = match reply.as_deref() {
                    Some(Reply::Blame(b)) => b.clone(),
                    _ => None,
                };
                git.blame = Some(BlameSet {
                    serial,
                    path,
                    version,
                    blame,
                });
            }
        }
    }
}

// ---- per frame ----------------------------------------------------------------------------

/// Called from [`App::tick`]: takes discoveries, `.git` events and answers, then asks for what
/// the active document needs and updates the inline blame.
pub fn tick(app: &mut App, now: Instant) {
    if !app.git.started {
        return;
    }
    drain_discovered(app);
    drain_git_events(app);
    drain_responses(app);
    for w in std::mem::take(&mut app.git.warnings) {
        app.warn(w);
    }
    schedule(app, now);
}

/// The text git compares: the buffer as it would be saved (a UTF-8 BOM included).
fn buffer_text(doc: &vr_core::Document) -> Arc<str> {
    let rope = doc.buffer.text();
    let mut s = String::with_capacity(rope.len_bytes() + 3);
    if doc.encoding == Encoding::Utf8Bom {
        s.push('\u{feff}');
    }
    for chunk in rope.chunks() {
        s.push_str(chunk);
    }
    Arc::from(s)
}

fn set_inline(app: &mut App, inline: Option<Inline>) {
    let same = match (&app.git.inline, &inline) {
        (None, None) => true,
        (Some(a), Some(b)) => {
            a.text == b.text && a.line == b.line && a.version == b.version && a.path == b.path
        }
        _ => false,
    };
    if !same {
        app.git.inline = inline;
        app.dirty = true;
    }
}

fn schedule(app: &mut App, now: Instant) {
    let doc = app.docs.get(app.active);
    // Only files git can compare: UTF-8 text with a path inside a root that has a repo.
    let target =
        doc.filter(|d| !d.read_only && matches!(d.encoding, Encoding::Utf8 | Encoding::Utf8Bom));
    let Some((doc, path)) = target.and_then(|d| d.path.clone().map(|p| (d, p))) else {
        app.git.track = Track::default();
        set_inline(app, None);
        return;
    };
    let Some(serial) = app.git.serial_for(&path) else {
        app.git.track = Track::default();
        set_inline(app, None);
        return;
    };
    let version = doc.buffer.version();
    let rope = doc.buffer.text();
    let head = doc.buffer.selection().primary().head;
    let line = rope.char_to_line(head.min(rope.len_chars()));
    let too_large = rope.len_bytes() > app.git.options.max_file;
    let blame_on = app.settings.git_inline_blame;

    let git = &mut app.git;
    let key = (path.clone(), version);
    if git.track.key.as_ref() != Some(&key) {
        let same_file = git.track.key.as_ref().is_some_and(|(p, _)| *p == path);
        git.track.since = same_file.then_some(now);
        git.track.key = Some(key);
    }
    let stable = git
        .track
        .since
        .is_none_or(|s| now.saturating_duration_since(s) >= git.debounce);
    if stable {
        let want_marks = git.asked_marks.get(&path) != Some(&(serial, version));
        let want_blame = blame_on && git.asked_blame != Some((serial, path.clone(), version));
        if (want_marks || want_blame) && too_large {
            // vr-git would refuse it (TooLarge); copying the text is not worth it.
            git.marks.remove(&path);
            git.asked_marks.insert(path.clone(), (serial, version));
            git.asked_blame = Some((serial, path.clone(), version));
        } else if want_marks || want_blame {
            let text = buffer_text(&app.docs[app.active]);
            let git = &mut app.git;
            if want_marks {
                git.request(
                    serial,
                    Request::LineChanges {
                        path: path.clone(),
                        text: text.clone(),
                    },
                    Pending::Marks(path.clone(), version),
                );
                git.asked_marks.insert(path.clone(), (serial, version));
            }
            if want_blame {
                git.request(
                    serial,
                    Request::InlineBlame {
                        path: path.clone(),
                        text,
                    },
                    Pending::Blame(path.clone(), version),
                );
                git.asked_blame = Some((serial, path.clone(), version));
            }
        }
    }

    let git = &mut app.git;
    let ckey = (path.clone(), version, line);
    if git.track.cursor.as_ref() != Some(&ckey) {
        git.track.cursor = Some(ckey);
        git.track.cursor_since = Some(now);
    }
    let rested = git
        .track
        .cursor_since
        .is_some_and(|s| now.saturating_duration_since(s) >= git.rest);
    let blame = git
        .blame
        .as_ref()
        .filter(|b| b.serial == serial && b.path == path && b.version == version);
    let text = match blame {
        Some(b) if blame_on && rested => b
            .blame
            .as_ref()
            .and_then(|bl| annotation(bl, line, git.now_secs)),
        _ => None,
    };
    let inline = text.map(|text| Inline {
        path,
        version,
        line,
        text,
    });
    set_inline(app, inline);
}

/// "Author, 3 days ago • subject" for 0-based `line`, or "You, uncommitted".
fn annotation(blame: &Blame, line: usize, now_secs: Option<i64>) -> Option<String> {
    let c = blame.commit_of(line)?;
    if c.uncommitted {
        return Some("You, uncommitted".to_string());
    }
    let now = now_secs.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64)
    });
    Some(format!(
        "{}, {} • {}",
        sanitize(&c.author.name),
        relative_age(now - c.author.time),
        sanitize(&c.summary)
    ))
}

/// GitLens-style age: "just now", "5 minutes ago", "3 days ago", "2 years ago".
pub fn relative_age(secs: i64) -> String {
    const MIN: i64 = 60;
    const HOUR: i64 = 60 * MIN;
    const DAY: i64 = 24 * HOUR;
    let (n, unit) = match secs {
        s if s < MIN => return "just now".to_string(),
        s if s < HOUR => (s / MIN, "minute"),
        s if s < DAY => (s / HOUR, "hour"),
        s if s < 30 * DAY => (s / DAY, "day"),
        s if s < 365 * DAY => (s / (30 * DAY), "month"),
        s => (s / (365 * DAY), "year"),
    };
    let plural = if n == 1 { "" } else { "s" };
    format!("{n} {unit}{plural} ago")
}

// ---- what the UI draws ----------------------------------------------------------------------

/// The inline blame of the active document's cursor line, when it is shown now.
pub fn inline_text(app: &App) -> Option<String> {
    let i = app.git.inline.as_ref()?;
    let doc = app.active_doc()?;
    let rope = doc.buffer.text();
    let head = doc.buffer.selection().primary().head;
    let line = rope.char_to_line(head.min(rope.len_chars()));
    (doc.path.as_deref() == Some(&i.path) && doc.buffer.version() == i.version && line == i.line)
        .then(|| i.text.clone())
}

/// The git part of the status bar for the active file's root (no file: the active root's),
/// once HEAD is known; `None` without a repo.
pub fn status_label(app: &App) -> Option<StatusLabel> {
    let path = match app.active_doc() {
        Some(d) => d.path.clone()?,
        None => app.active_root().path.clone(),
    };
    let RootState::Repo { serial, parent } = app.git.root_state(&path)? else {
        return None;
    };
    let e = app.git.entry(*serial)?;
    let (full, short) = match e.head.as_ref()? {
        Head::Branch { name, .. } => {
            let name = sanitize(name);
            (cut(&name, BRANCH_FULL), cut(&name, BRANCH_SHORT))
        }
        Head::Detached { short, .. } => {
            let short = sanitize(short);
            (format!("detached {short}"), short)
        }
    };
    let sync = e.ahead.as_ref().map_or_else(String::new, |a| {
        let mut parts = Vec::new();
        if a.ahead > 0 {
            parts.push(format!("↑{}", a.ahead));
        }
        if a.behind > 0 {
            parts.push(format!("↓{}", a.behind));
        }
        parts.join(" ")
    });
    let with_sync = |s: String| {
        if sync.is_empty() {
            s
        } else {
            format!("{s} {sync}")
        }
    };
    let (mut full, mut short) = (with_sync(full), with_sync(short));
    if let Some(p) = parent {
        full.push_str(&format!(" · parent repo: {p}"));
        short.push_str(" · parent repo");
    }
    Some(StatusLabel { full, short })
}

/// `s` cut to `max` chars with an ellipsis.
fn cut(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Exit and workspace close: every git process is killed and no new one starts.
pub fn shutdown(app: &mut App) {
    app.git.shutdown();
}
