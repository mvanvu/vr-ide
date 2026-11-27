//! GitWorker: non-blocking requests, versioned responses, coalescing and cache invalidation.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use common::Fixture;
use vr_git::{GitError, GitWorker, Options, Reply, Repo, Request, Response};

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
    slow_for(f, word, 1)
}

/// The repo behind a git that sleeps `secs` before every command whose arguments match the
/// shell pattern `*word*`.
fn slow_for(f: &Fixture, word: &str, secs: u32) -> Repo {
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
            "#!/bin/sh\ncase \"$*\" in *{word}*) sleep {secs};; esac\nexec '{}' \"$@\"\n",
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
    let repo = slow(&f, "symbolic-ref");
    let (w, wake) = start(repo);
    let t = Instant::now();
    let a = w.request(Request::Head);
    let b = w.request(Request::AheadBehind);
    let c = w.request(Request::InlineBlame {
        path: f.path("a.txt"),
        text: Arc::from("one\nTWO\n"),
    });
    assert!(
        t.elapsed() < Duration::from_millis(50),
        "request blocked for {:?}",
        t.elapsed()
    );
    assert!(a < b && b < c);
    assert!(w.try_recv().is_none(), "head is still running");
    let (r, _) = wait(&w, &wake, c);
    assert!(matches!(r.result.as_deref(), Ok(Reply::Blame(Some(_)))));
}

#[test]
fn queued_requests_for_the_same_target_are_superseded() {
    let f = repo_with_file();
    let (w, wake) = start(slow(&f, "symbolic-ref"));
    let p = f.path("a.txt");
    // A slow head keeps the worker busy so the next three queue up behind it.
    let busy = w.request(Request::Head);
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
    let text: Arc<str> = Arc::from("one\nTWO\n");
    let blame = |text: &Arc<str>| Request::InlineBlame {
        path: p.clone(),
        text: text.clone(),
    };
    let id = w.request(blame(&text));
    let first = wait(&w, &wake, id).0.result.unwrap();
    let id = w.request(blame(&text));
    let second = wait(&w, &wake, id).0.result.unwrap();
    assert!(
        Arc::ptr_eq(&first, &second),
        "unchanged repo and file → cached reply"
    );

    // File change (mtime/size) → recomputed.
    f.write("a.txt", "one\nTWO\nthree\n");
    let id = w.request(blame(&Arc::from("one\nTWO\nthree\n")));
    let third = wait(&w, &wake, id).0.result.unwrap();
    let Reply::Blame(Some(b)) = &*third else {
        panic!()
    };
    assert_eq!(b.lines.len(), 3);
    assert!(b.commit_of(2).unwrap().uncommitted);

    // A change of one file drops the repo-wide replies and the replies of that file only.
    let id = w.request(blame(&Arc::from("one\nTWO\nthree\n")));
    let kept = wait(&w, &wake, id).0.result.unwrap();
    let id = w.request(Request::Head);
    let head_before = wait(&w, &wake, id).0.result.unwrap();
    w.invalidate_path(&f.path("b.txt"));
    let id = w.request(Request::Head);
    let head_after = wait(&w, &wake, id).0.result.unwrap();
    assert!(
        !Arc::ptr_eq(&head_before, &head_after),
        "a path invalidation drops repo-wide replies"
    );
    let id = w.request(blame(&Arc::from("one\nTWO\nthree\n")));
    let again = wait(&w, &wake, id).0.result.unwrap();
    assert!(
        Arc::ptr_eq(&kept, &again),
        "another file's reply stays cached"
    );
    w.invalidate_path(&p);
    let id = w.request(blame(&Arc::from("one\nTWO\nthree\n")));
    let redone = wait(&w, &wake, id).0.result.unwrap();
    assert!(
        !Arc::ptr_eq(&kept, &redone),
        "this file's reply was dropped"
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
    let id = w.request(Request::LineChanges {
        path: "/etc/passwd".into(),
        text: Arc::from("x\n"),
    });
    assert!(matches!(
        wait(&w, &wake, id).0.result,
        Err(GitError::OutsideRepo(_))
    ));
}

#[test]
fn file_blame_request_is_answered_with_a_blame() {
    let f = repo_with_file();
    let (w, wake) = start(f.repo());
    let id = w.request(Request::FileBlame {
        path: f.path("a.txt"),
        text: Arc::from("one\nTWO\nthree\n"),
    });
    let r = wait(&w, &wake, id).0.result.unwrap();
    let Reply::Blame(Some(b)) = &*r else {
        panic!("{r:?}");
    };
    assert_eq!(b.lines.len(), 3);
    assert!(!b.commit_of(1).unwrap().uncommitted);
    assert!(b.commit_of(2).unwrap().uncommitted);
}

/// A repo with `a.txt` and `slow.txt` whose blame takes 30 s (any other git answers at once).
fn slow_blame_repo() -> (Fixture, Repo) {
    let f = repo_with_file();
    f.write("slow.txt", "s\n");
    f.commit_all("slow");
    let repo = slow_for(&f, "blame*slow.txt", 30);
    (f, repo)
}

fn file_blame(f: &Fixture, name: &str) -> Request {
    Request::FileBlame {
        path: f.path(name),
        text: Arc::from(std::fs::read_to_string(f.path(name)).unwrap().as_str()),
    }
}

#[test]
fn automatic_reads_answer_while_a_file_blame_runs() {
    let (f, repo) = slow_blame_repo();
    let (w, wake) = start(repo);
    let blame = w.request(file_blame(&f, "slow.txt"));
    // Let the blame reach git before the automatic reads are asked.
    std::thread::sleep(Duration::from_millis(300));
    let t = Instant::now();
    let marks = w.request(Request::LineChanges {
        path: f.path("a.txt"),
        text: Arc::from("one\nX\n"),
    });
    let head = w.request(Request::Head);
    let (r, earlier) = wait(&w, &wake, head);
    assert!(matches!(r.result.as_deref(), Ok(Reply::Head(_))), "{r:?}");
    let m = earlier
        .iter()
        .find(|r| r.id == marks)
        .expect("marks answered");
    assert!(matches!(
        m.result.as_deref(),
        Ok(Reply::LineChanges(Some(_)))
    ));
    assert!(
        earlier.iter().all(|r| r.id != blame),
        "the blame still runs"
    );
    assert!(
        t.elapsed() < Duration::from_secs(5),
        "automatic reads waited for the file blame: {:?}",
        t.elapsed()
    );
    w.cancel_file_blame(blame);
}

#[test]
fn a_newer_file_blame_cancels_the_running_one() {
    let (f, repo) = slow_blame_repo();
    let (w, wake) = start(repo);
    let slow = w.request(file_blame(&f, "slow.txt"));
    std::thread::sleep(Duration::from_millis(300));
    let t = Instant::now();
    let fast = w.request(file_blame(&f, "a.txt"));
    let (r, earlier) = wait(&w, &wake, fast);
    assert!(
        matches!(r.result.as_deref(), Ok(Reply::Blame(Some(_)))),
        "{r:?}"
    );
    let old = earlier
        .iter()
        .find(|r| r.id == slow)
        .expect("old blame answered");
    assert!(matches!(old.result, Err(GitError::Superseded)), "{old:?}");
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
}

#[test]
fn a_cancelled_file_blame_stops_its_git() {
    let (f, repo) = slow_blame_repo();
    let (w, wake) = start(repo);
    let id = w.request(file_blame(&f, "slow.txt"));
    std::thread::sleep(Duration::from_millis(300));
    let t = Instant::now();
    w.cancel_file_blame(id);
    let (r, _) = wait(&w, &wake, id);
    assert!(matches!(r.result, Err(GitError::Superseded)), "{r:?}");
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    // The lane takes the next blame at once.
    let id = w.request(file_blame(&f, "a.txt"));
    assert!(matches!(
        wait(&w, &wake, id).0.result.as_deref(),
        Ok(Reply::Blame(Some(_)))
    ));
}

#[test]
fn newer_file_blames_of_the_same_file_wait_for_the_running_one() {
    let f = repo_with_file();
    f.write("slow.txt", "s\n");
    f.commit_all("slow");
    let (w, wake) = start(slow_for(&f, "blame*slow.txt", 2));
    // Asked again and again more often than the blame takes (a busy `.git`).
    let first = w.request(file_blame(&f, "slow.txt"));
    let mut ids = vec![first];
    for _ in 0..8 {
        std::thread::sleep(Duration::from_millis(300));
        ids.push(w.request(file_blame(&f, "slow.txt")));
    }
    let last = *ids.last().unwrap();
    let (r, earlier) = wait(&w, &wake, last);
    assert!(
        matches!(r.result.as_deref(), Ok(Reply::Blame(Some(_)))),
        "{r:?}"
    );
    let first = earlier
        .iter()
        .find(|r| r.id == first)
        .expect("first answered");
    assert!(
        matches!(first.result.as_deref(), Ok(Reply::Blame(Some(_)))),
        "the running blame was killed: {:?}",
        first.result
    );
    let skipped = earlier
        .iter()
        .filter(|r| matches!(r.result, Err(GitError::Superseded)))
        .count();
    // Requests replaced while waiting are dropped; how many ran in between depends on timing.
    let answered = earlier.iter().filter(|r| ids.contains(&r.id)).count();
    assert_eq!(
        answered,
        ids.len() - 1,
        "every request gets exactly one response"
    );
    assert!(skipped >= 1, "waiting requests were replaced, not stacked");
}
