//! Background git queries for one repo: versioned requests, coalescing and a result cache.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};

use crate::blame::Blame;
use crate::diff::{FileDiff, LineChange};
use crate::log::{CommitInfo, CommitSummary};
use crate::parse::is_oid;
use crate::repo::{AheadBehind, Head, Repo, Status};
use crate::{GitError, Result};

/// A query for the worker. Buffer texts are shared, not copied, between the editor and the worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    Head,
    AheadBehind,
    Status,
    DiffUnstaged(PathBuf),
    DiffStaged(PathBuf),
    DiffCommit {
        rev: String,
        path: Option<PathBuf>,
    },
    Log {
        limit: usize,
        path: Option<PathBuf>,
    },
    Commit(String),
    Blame(PathBuf),
    BlameText {
        path: PathBuf,
        text: Arc<str>,
    },
    /// Inline blame of the cursor line (automatic read: short timeout).
    InlineBlame {
        path: PathBuf,
        text: Arc<str>,
    },
    LineChanges {
        path: PathBuf,
        text: Arc<str>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    Head(Head),
    AheadBehind(Option<AheadBehind>),
    Status(Status),
    Diff(Option<FileDiff>),
    Diffs(Vec<FileDiff>),
    Log(Vec<CommitSummary>),
    Commit(CommitInfo),
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
    arg: String,
}

impl Request {
    fn key(&self) -> Key {
        let k = |kind, path: Option<&PathBuf>, arg: String| Key {
            kind,
            path: path.cloned(),
            arg,
        };
        match self {
            Request::Head => k(0, None, String::new()),
            Request::AheadBehind => k(1, None, String::new()),
            Request::Status => k(2, None, String::new()),
            Request::DiffUnstaged(p) => k(3, Some(p), String::new()),
            Request::DiffStaged(p) => k(4, Some(p), String::new()),
            Request::DiffCommit { rev, path } => k(5, path.as_ref(), rev.clone()),
            Request::Log { limit, path } => k(6, path.as_ref(), limit.to_string()),
            Request::Commit(rev) => k(7, None, rev.clone()),
            Request::Blame(p) => k(8, Some(p), String::new()),
            Request::BlameText { path, .. } => k(9, Some(path), String::new()),
            Request::LineChanges { path, .. } => k(10, Some(path), String::new()),
            Request::InlineBlame { path, .. } => k(11, Some(path), String::new()),
        }
    }

    /// Results that can never change (a full commit id).
    fn immutable(&self) -> bool {
        match self {
            Request::Commit(rev) | Request::DiffCommit { rev, .. } => {
                rev.len() >= 40 && is_oid(rev)
            }
            _ => false,
        }
    }

    fn text(&self) -> Option<&str> {
        match self {
            Request::BlameText { text, .. }
            | Request::InlineBlame { text, .. }
            | Request::LineChanges { text, .. } => Some(text),
            _ => None,
        }
    }

    fn path(&self) -> Option<&Path> {
        match self {
            Request::DiffUnstaged(p) | Request::DiffStaged(p) | Request::Blame(p) => {
                Some(p.as_path())
            }
            Request::BlameText { path, .. }
            | Request::InlineBlame { path, .. }
            | Request::LineChanges { path, .. } => Some(path.as_path()),
            Request::DiffCommit { path, .. } | Request::Log { path, .. } => path.as_deref(),
            _ => None,
        }
    }
}

enum Msg {
    Req(u64, Request),
    Invalidate(Option<PathBuf>),
}

/// Runs the queries of one repo on its own thread; no method blocks.
pub struct GitWorker {
    tx: Sender<Msg>,
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
        std::thread::Builder::new()
            .name("vr-git".into())
            .spawn(move || run(repo, req_rx, resp_tx, Box::new(notify)))
            .expect("spawn git worker");
        GitWorker {
            tx,
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
        let _ = self.tx.send(Msg::Req(id, req));
        id
    }

    /// Drop every cached result (HEAD, index or many files changed).
    pub fn invalidate(&self) {
        let _ = self.tx.send(Msg::Invalidate(None));
    }

    /// Drop cached results of one file (and the status lists).
    pub fn invalidate_path(&self, path: &Path) {
        let _ = self.tx.send(Msg::Invalidate(Some(path.to_path_buf())));
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
    if req.immutable() {
        return None;
    }
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
        Request::Status => Reply::Status(repo.status()?),
        Request::DiffUnstaged(p) => Reply::Diff(repo.diff_unstaged(p)?),
        Request::DiffStaged(p) => Reply::Diff(repo.diff_staged(p)?),
        Request::DiffCommit { rev, path } => Reply::Diffs(repo.diff_commit(rev, path.as_deref())?),
        Request::Log { limit, path } => Reply::Log(repo.log(*limit, path.as_deref())?),
        Request::Commit(rev) => Reply::Commit(repo.commit(rev)?),
        Request::Blame(p) => Reply::Blame(repo.blame(p)?),
        Request::BlameText { path, text } => Reply::Blame(repo.blame_text(path, text)?),
        Request::InlineBlame { path, text } => Reply::Blame(repo.inline_blame(path, text)?),
        Request::LineChanges { path, text } => Reply::LineChanges(repo.line_changes(path, text)?),
    })
}

fn run(repo: Repo, rx: Receiver<Msg>, tx: Sender<Response>, notify: Box<dyn Fn() + Send>) {
    let mut cache = Cache {
        map: HashMap::new(),
    };
    let send = |r: Response| -> bool {
        let ok = tx.send(r).is_ok();
        notify();
        ok
    };
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
                    .retain(|k, _| k.path.as_deref() != Some(&p) && k.kind > 2),
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
