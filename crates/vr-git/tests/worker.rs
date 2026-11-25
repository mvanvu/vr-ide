//! GitWorker: non-blocking requests, versioned responses, coalescing and cache invalidation.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use common::Fixture;
use vr_git::{FileStatus, GitError, GitWorker, Options, Reply, Repo, Request, Response};

fn start(repo: Repo) -> (GitWorker, Receiver<()>) {
    let (tx, rx) = mpsc::channel();
    let w = GitWorker::start(repo, move || {
        let _ = tx.send(());
    });
    (w, rx)
}

/// Wait (notify-driven) for the response to `id`; earlier responses are collected too.
fn wait(w: &GitWorker, wake: &Receiver<()>, id: u64) -> (Response, Vec<Response>) {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut others = Vec::new();
    loop {
        while let Some(r) = w.try_recv() {
            if r.id == id {
                return (r, others);
            }
            others.push(r);
        }
        let left = deadline.saturating_duration_since(Instant::now());
        assert!(!left.is_zero(), "no response to {id}");
        let _ = wake.recv_timeout(left);
    }
}

/// The repo behind a git that takes a second for every command whose arguments contain `word`.
fn slow(f: &Fixture, word: &str) -> Repo {
    let dir = f.tmp.path().join("shim");
    std::fs::create_dir_all(&dir).unwrap();
    let real = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|d| d.join("git"))
        .find(|p| p.is_file())
        .unwrap();
    let shim = dir.join("git");
    std::fs::write(
        &shim,
        format!(
            "#!/bin/sh\ncase \"$*\" in *{word}*) sleep 1;; esac\nexec '{}' \"$@\"\n",
            real.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut env = f.env();
    for (k, v) in env.iter_mut() {
        if k == "PATH" {
            let mut p = dir.clone().into_os_string();
            p.push(":");
            p.push(&*v);
            *v = p;
        }
    }
    Repo::discover_with(
        &f.dir,
        Options {
            env: Some(env),
            ..Options::default()
        },
    )
    .unwrap()
    .unwrap()
}

fn repo_with_file() -> Fixture {
    let f = Fixture::new();
    f.write("a.txt", "one\ntwo\n");
    f.commit_all("first");
    f.write("a.txt", "one\nTWO\n");
    f.commit_all("second");
    f
}

#[test]
fn requests_never_block_and_ids_increase() {
    let f = repo_with_file();
    let repo = slow(&f, "status");
    let (w, wake) = start(repo);
    let t = Instant::now();
    let a = w.request(Request::Status);
    let b = w.request(Request::Head);
    let c = w.request(Request::Blame(f.path("a.txt")));
    assert!(
        t.elapsed() < Duration::from_millis(50),
        "request blocked for {:?}",
        t.elapsed()
    );
    assert!(a < b && b < c);
    assert!(w.try_recv().is_none(), "status is still running");
    let (r, _) = wait(&w, &wake, c);
    assert!(matches!(r.result.as_deref(), Ok(Reply::Blame(Some(_)))));
}

#[test]
fn queued_requests_for_the_same_target_are_superseded() {
    let f = repo_with_file();
    let (w, wake) = start(slow(&f, "--max-count"));
    let p = f.path("a.txt");
    // A slow log keeps the worker busy so the next three queue up behind it.
    let busy = w.request(Request::Log {
        limit: 1000,
        path: None,
    });
    let texts = ["one\n", "one\nX\n", "one\nTWO\nthree\n"];
    let ids: Vec<u64> = texts
        .iter()
        .map(|t| {
            w.request(Request::LineChanges {
                path: p.clone(),
                text: Arc::from(*t),
            })
        })
        .collect();
    let (last, earlier) = wait(&w, &wake, ids[2]);
    let Ok(reply) = last.result.as_deref() else {
        panic!("{:?}", last.result)
    };
    let Reply::LineChanges(Some(marks)) = reply else {
        panic!("{reply:?}")
    };
    assert_eq!(marks.len(), 1, "marks for the newest text only: {marks:?}");
    assert_eq!(marks[0].kind, vr_git::ChangeKind::Added);
    let superseded = earlier
        .iter()
        .filter(|r| matches!(r.result, Err(GitError::Superseded)))
        .count();
    let answered = earlier.iter().filter(|r| ids[..2].contains(&r.id)).count();
    assert_eq!(answered, 2, "every request gets exactly one response");
    assert_eq!(superseded, 2, "the two older texts were never computed");
    let _ = busy;
}

#[test]
fn cache_is_reused_and_invalidated_by_head_index_and_file_changes() {
    let f = repo_with_file();
    let (w, wake) = start(f.repo());
    let p = f.path("a.txt");
    let id = w.request(Request::Blame(p.clone()));
    let first = wait(&w, &wake, id).0.result.unwrap();
    let id = w.request(Request::Blame(p.clone()));
    let second = wait(&w, &wake, id).0.result.unwrap();
    assert!(
        Arc::ptr_eq(&first, &second),
        "unchanged repo and file → cached reply"
    );

    // File change (mtime/size) → recomputed.
    f.write("a.txt", "one\nTWO\nthree\n");
    let id = w.request(Request::Blame(p.clone()));
    let third = wait(&w, &wake, id).0.result.unwrap();
    let Reply::Blame(Some(b)) = &*third else {
        panic!()
    };
    assert_eq!(b.lines.len(), 3);
    assert!(b.commit_of(2).unwrap().uncommitted);

    // Status is cached until the index/HEAD change or an explicit invalidation.
    let id = w.request(Request::Status);
    let s1 = wait(&w, &wake, id).0.result.unwrap();
    let Reply::Status(s) = &*s1 else { panic!() };
    assert_eq!(s.unstaged.len(), 1);
    f.git(&["add", "a.txt"]);
    let id = w.request(Request::Status);
    let s2 = wait(&w, &wake, id).0.result.unwrap();
    let Reply::Status(s) = &*s2 else { panic!() };
    assert_eq!(
        (s.staged.len(), s.unstaged.len()),
        (1, 0),
        "index change seen"
    );

    f.write("b.txt", "new\n");
    w.invalidate_path(&f.path("b.txt"));
    let id = w.request(Request::Status);
    let s3 = wait(&w, &wake, id).0.result.unwrap();
    let Reply::Status(s) = &*s3 else { panic!() };
    assert!(
        s.unstaged
            .iter()
            .any(|e| e.path == Path::new("b.txt") && e.status == FileStatus::Untracked)
    );

    // A new commit moves HEAD → head reply recomputed without an explicit invalidation.
    let id = w.request(Request::Head);
    let h1 = wait(&w, &wake, id).0.result.unwrap();
    f.commit_all("third");
    let id = w.request(Request::Head);
    let h2 = wait(&w, &wake, id).0.result.unwrap();
    assert_ne!(h1, h2);

    w.invalidate();
    let id = w.request(Request::Head);
    let h3 = wait(&w, &wake, id).0.result.unwrap();
    assert_eq!(h2, h3);
    assert!(!Arc::ptr_eq(&h2, &h3), "invalidate() drops the cache");
}

#[test]
fn errors_are_delivered_not_panics() {
    let f = repo_with_file();
    let (w, wake) = start(f.repo());
    let id = w.request(Request::Commit("--output=x".into()));
    let r = wait(&w, &wake, id).0;
    assert!(matches!(r.result, Err(GitError::NotACommit(_))));
    let id = w.request(Request::DiffUnstaged("/etc/passwd".into()));
    assert!(matches!(
        wait(&w, &wake, id).0.result,
        Err(GitError::OutsideRepo(_))
    ));
}
