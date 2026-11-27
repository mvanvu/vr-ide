//! Background git queries for one repo: versioned requests, coalescing and a result cache.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Condvar, Mutex};

use crate::blame::Blame;
use crate::diff::LineChange;
use crate::repo::{AheadBehind, Head, Repo};
use crate::{GitError, Result};

/// A query for the worker. Buffer texts are shared, not copied, between the editor and the worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Head,
    AheadBehind,
    /// Inline blame of the cursor line (automatic read: short timeout).
    InlineBlame {
        path: PathBuf,
        text: Arc<str>,
    },
    LineChanges {
        path: PathBuf,
        text: Arc<str>,
    },
    /// Blame of the whole file for the blame column (a read the user asked for: user timeout).
    FileBlame {
        path: PathBuf,
        text: Arc<str>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    Head(Head),
    AheadBehind(Option<AheadBehind>),
    Blame(Option<Blame>),
    LineChanges(Option<Vec<LineChange>>),
}

/// The answer to request `id`. A request replaced by a newer one of the same kind and path
/// before it ran is answered with [`GitError::Superseded`].
#[derive(Debug, Clone)]
pub struct Response {
    pub id: u64,
    pub request: Request,
    pub result: Result<Arc<Reply>>,
}

/// Coalescing key: kind and target, never the buffer text (the newest text wins).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    kind: u8,
    path: Option<PathBuf>,
}

/// Kinds below this are repo-wide: a change of any file drops them, while per-file replies of
/// other files stay cached.
const REPO_WIDE_KINDS: u8 = 2;

impl Request {
    fn key(&self) -> Key {
        let k = |kind, path: Option<&PathBuf>| Key {
            kind,
            path: path.cloned(),
        };
        match self {
            Request::Head => k(0, None),
            Request::AheadBehind => k(1, None),
            Request::InlineBlame { path, .. } => k(2, Some(path)),
            Request::LineChanges { path, .. } => k(3, Some(path)),
            Request::FileBlame { path, .. } => k(4, Some(path)),
        }
    }

    fn text(&self) -> Option<&str> {
        match self {
            Request::InlineBlame { text, .. }
            | Request::LineChanges { text, .. }
            | Request::FileBlame { text, .. } => Some(text),
            _ => None,
        }
    }

    fn path(&self) -> Option<&Path> {
        match self {
            Request::InlineBlame { path, .. }
            | Request::LineChanges { path, .. }
            | Request::FileBlame { path, .. } => Some(path.as_path()),
            _ => None,
        }
    }
}

enum Msg {
    Req(u64, Request),
    Invalidate(Option<PathBuf>),
}

/// Runs the queries of one repo on its own thread, and whole-file blames on a second one (the
/// lane), so a blame that takes many seconds never holds back the automatic reads; no method
/// blocks.
pub struct GitWorker {
    tx: Sender<Msg>,
    lane: Arc<Lane>,
    rx: Receiver<Response>,
    next_id: std::cell::Cell<u64>,
    workdir: PathBuf,
    /// Shares the runner with the worker thread's repo, for [`GitWorker::shutdown`].
    repo: Repo,
}

impl GitWorker {
    /// Start the worker thread; `notify` runs (on the worker thread) after each response is queued.
    pub fn start(repo: Repo, notify: impl Fn() + Send + 'static) -> GitWorker {
        let (tx, req_rx) = mpsc::channel();
        let (resp_tx, rx) = mpsc::channel();
        let workdir = repo.workdir().to_path_buf();
        let handle = repo.clone();
        let out = Arc::new(Out {
            tx: resp_tx,
            notify: Mutex::new(Box::new(notify)),
        });
        let lane = Arc::new(Lane::default());
        let (lane_repo, lane_out, lane_in) = (repo.clone(), out.clone(), lane.clone());
        std::thread::Builder::new()
            .name("vr-git".into())
            .spawn(move || run(repo, req_rx, out))
            .expect("spawn git worker");
        std::thread::Builder::new()
            .name("vr-git-blame".into())
            .spawn(move || run_lane(lane_repo, lane_in, lane_out))
            .expect("spawn git blame lane");
        GitWorker {
            tx,
            lane,
            rx,
            next_id: std::cell::Cell::new(0),
            workdir,
            repo: handle,
        }
    }

    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    /// Queue a query; returns its id (increasing), which the matching [`Response`] carries.
    pub fn request(&self, req: Request) -> u64 {
        let id = self.next_id.get() + 1;
        self.next_id.set(id);
        if matches!(req, Request::FileBlame { .. }) {
            // One file blame waiting and one running per repo, never more.
            self.lane.submit(id, req);
        } else {
            let _ = self.tx.send(Msg::Req(id, req));
        }
        id
    }

    /// Drop every cached result (HEAD, index or many files changed).
    pub fn invalidate(&self) {
        let _ = self.tx.send(Msg::Invalidate(None));
    }

    /// Drop cached results of one file.
    pub fn invalidate_path(&self, path: &Path) {
        let _ = self.tx.send(Msg::Invalidate(Some(path.to_path_buf())));
    }

    /// Drop file blame `id` if it waits, kill its git if it runs (answered
    /// [`GitError::Superseded`]); nothing when it already ended.
    pub fn cancel_file_blame(&self, id: u64) {
        self.lane.cancel(id);
    }

    pub fn try_recv(&self) -> Option<Response> {
        self.rx.try_recv().ok()
    }
}

impl GitWorker {
    /// Kill the running git (with its children) and refuse further queries of this repo
    /// ([`Repo::shutdown`]); queued requests are answered with [`GitError::ShutDown`].
    pub fn shutdown(&self) {
        self.repo.shutdown();
    }
}

impl Drop for GitWorker {
    fn drop(&mut self) {
        self.lane.close();
        self.shutdown();
    }
}

/// What a cached result depended on.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Stamp {
    repo: Vec<Option<(i64, i64, u64, u64)>>,
    file: Option<(i64, i64, u64, u64)>,
    text: u64,
}

const CACHE_MAX: usize = 256;

struct Cache {
    map: HashMap<Key, (Option<Stamp>, Arc<Reply>)>,
}

fn stat(p: &Path) -> Option<(i64, i64, u64, u64)> {
    let m = std::fs::metadata(p).ok()?;
    Some((m.mtime(), m.mtime_nsec(), m.size(), m.ino()))
}

/// Files whose change means HEAD, the index or refs moved.
fn repo_stamp(repo: &Repo) -> Vec<Option<(i64, i64, u64, u64)>> {
    let (g, c) = (repo.git_dir(), repo.common_dir());
    let mut files = vec![
        g.join("HEAD"),
        g.join("index"),
        g.join("logs/HEAD"),
        c.join("packed-refs"),
        c.join("FETCH_HEAD"),
    ];
    if let Ok(head) = std::fs::read_to_string(g.join("HEAD"))
        && let Some(r) = head.trim().strip_prefix("ref: ")
    {
        files.push(c.join(r));
        files.push(c.join("logs").join(r));
    }
    files.iter().map(|f| stat(f)).collect()
}

fn stamp_of(repo: &Repo, req: &Request) -> Option<Stamp> {
    let file = req
        .path()
        .map(|p| {
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                repo.workdir().join(p)
            }
        })
        .and_then(|p| stat(&p));
    let text = req.text().map_or(0, |t| {
        let mut h = DefaultHasher::new();
        t.hash(&mut h);
        h.finish()
    });
    Some(Stamp {
        repo: repo_stamp(repo),
        file,
        text,
    })
}

fn execute(repo: &Repo, req: &Request) -> Result<Reply> {
    Ok(match req {
        Request::Head => Reply::Head(repo.head()?),
        Request::AheadBehind => Reply::AheadBehind(repo.ahead_behind()?),
        Request::InlineBlame { path, text } => Reply::Blame(repo.inline_blame(path, text)?),
        Request::LineChanges { path, text } => Reply::LineChanges(repo.line_changes(path, text)?),
        Request::FileBlame { path, text } => Reply::Blame(repo.file_blame(path, text)?),
    })
}

/// Where both threads of a worker put their answers.
struct Out {
    tx: Sender<Response>,
    notify: Mutex<Box<dyn Fn() + Send>>,
}

impl Out {
    /// False once the worker's owner is gone.
    fn send(&self, r: Response) -> bool {
        let ok = self.tx.send(r).is_ok();
        (self.notify.lock().unwrap())();
        ok
    }
}

/// The file blame lane: at most one blame waiting and one running. A newer request replaces
/// the waiting one; for another file it also kills the running one, so switching tabs never
/// stacks blames up. A newer request for the running file waits for it: killing it would let a
/// busy `.git` (asking again faster than the blame takes) keep the answer from ever arriving.
#[derive(Default)]
struct LaneState {
    queued: Option<(u64, Request)>,
    /// Id, file and cancel flag of the blame that runs.
    running: Option<(u64, PathBuf, Arc<AtomicBool>)>,
    /// Requests replaced or cancelled before they ran, still to be answered.
    dropped: Vec<(u64, Request)>,
    closed: bool,
}

#[derive(Default)]
struct Lane {
    state: Mutex<LaneState>,
    wake: Condvar,
}

impl Lane {
    fn submit(&self, id: u64, req: Request) {
        let mut s = self.state.lock().unwrap();
        if let Some((_, running, cancel)) = &s.running
            && req.path() != Some(running.as_path())
        {
            cancel.store(true, Ordering::SeqCst);
        }
        if let Some(old) = s.queued.replace((id, req)) {
            s.dropped.push(old);
        }
        self.wake.notify_one();
    }

    fn cancel(&self, id: u64) {
        let mut s = self.state.lock().unwrap();
        if let Some(old) = s.queued.take_if(|(q, _)| *q == id) {
            s.dropped.push(old);
        }
        if let Some((r, _, cancel)) = &s.running
            && *r == id
        {
            cancel.store(true, Ordering::SeqCst);
        }
        self.wake.notify_one();
    }

    fn close(&self) {
        let mut s = self.state.lock().unwrap();
        s.closed = true;
        if let Some((_, _, cancel)) = &s.running {
            cancel.store(true, Ordering::SeqCst);
        }
        self.wake.notify_one();
    }
}

/// Whole-file blames, one at a time, uncached: the column asks once per saved state of a file.
fn run_lane(repo: Repo, lane: Arc<Lane>, out: Arc<Out>) {
    loop {
        let (dropped, next, closed) = {
            let mut s = lane.state.lock().unwrap();
            while s.queued.is_none() && s.dropped.is_empty() && !s.closed {
                s = lane.wake.wait(s).unwrap();
            }
            let next = s.queued.take().map(|(id, req)| {
                let cancel = Arc::new(AtomicBool::new(false));
                let path = req.path().map(Path::to_path_buf).unwrap_or_default();
                s.running = Some((id, path, cancel.clone()));
                (id, req, cancel)
            });
            (std::mem::take(&mut s.dropped), next, s.closed)
        };
        for (id, request) in dropped {
            let result = Err(GitError::Superseded);
            if !out.send(Response {
                id,
                request,
                result,
            }) {
                return;
            }
        }
        let Some((id, request, cancel)) = next else {
            if closed {
                return;
            }
            continue;
        };
        let result = if closed {
            Err(GitError::ShutDown)
        } else {
            match &request {
                Request::FileBlame { path, text } => repo
                    .file_blame_cancellable(path, text, cancel)
                    .map(|b| Arc::new(Reply::Blame(b))),
                other => execute(&repo, other).map(Arc::new),
            }
        };
        lane.state.lock().unwrap().running = None;
        if !out.send(Response {
            id,
            request,
            result,
        }) || closed
        {
            return;
        }
    }
}

fn run(repo: Repo, rx: Receiver<Msg>, out: Arc<Out>) {
    let mut cache = Cache {
        map: HashMap::new(),
    };
    let send = |r: Response| -> bool { out.send(r) };
    while let Ok(first) = rx.recv() {
        let mut batch = vec![first];
        loop {
            match rx.try_recv() {
                Ok(m) => batch.push(m),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
        let mut reqs: Vec<(u64, Request)> = Vec::new();
        for m in batch {
            match m {
                Msg::Invalidate(None) => cache.map.clear(),
                Msg::Invalidate(Some(p)) => cache
                    .map
                    .retain(|k, _| k.path.as_deref() != Some(&p) && k.kind >= REPO_WIDE_KINDS),
                Msg::Req(id, r) => reqs.push((id, r)),
            }
        }
        // Keep only the newest request per key; older ones are answered as superseded.
        let mut newest: HashMap<Key, u64> = HashMap::new();
        for (id, r) in &reqs {
            newest.insert(r.key(), *id);
        }
        for (id, req) in reqs {
            let key = req.key();
            if newest.get(&key) != Some(&id) {
                if !send(Response {
                    id,
                    request: req,
                    result: Err(GitError::Superseded),
                }) {
                    return;
                }
                continue;
            }
            let stamp = stamp_of(&repo, &req);
            let cached = cache
                .map
                .get(&key)
                .filter(|(s, _)| *s == stamp)
                .map(|(_, r)| r.clone());
            let result = match cached {
                Some(r) => Ok(r),
                None => {
                    let r = execute(&repo, &req).map(Arc::new);
                    if let Ok(reply) = &r {
                        if cache.map.len() >= CACHE_MAX {
                            cache.map.clear();
                        }
                        cache.map.insert(key, (stamp, reply.clone()));
                    }
                    r
                }
            };
            if !send(Response {
                id,
                request: req,
                result,
            }) {
                return;
            }
        }
    }
}
