//! AC-16e perf bench (ignored; run with `cargo test -p vr-git --release --test bench -- --ignored --nocapture`).
//! Fixture: 50 000 files; `src/hot.txt` (5 000 lines) changed by each of 5 000 commits and renamed
//! from `old/hot.txt` at commit 2 500; `src/typical.txt` (5 000 lines) changed by every 10th
//! commit. Set `VR_GIT_BENCH_DIR` to keep and reuse the fixture between runs.

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use vr_git::{GitWorker, Options, Reply, Repo, Request};

const FILES: usize = 50_000;
const COMMITS: usize = 5_000;
const LINES: usize = 5_000;

fn build(dir: &Path, env: &[(std::ffi::OsString, std::ffi::OsString)]) {
    let git = |args: &[&str]| {
        let st = Command::new("git")
            .current_dir(dir)
            .env_clear()
            .envs(env.iter().cloned())
            .args(args)
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?}");
    };
    git(&["init", "-q", "-b", "main"]);
    let mut child = Command::new("git")
        .current_dir(dir)
        .env_clear()
        .envs(env.iter().cloned())
        .args(["fast-import", "--quiet"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let mut w = std::io::BufWriter::new(child.stdin.take().unwrap());
    let data = |w: &mut dyn Write, s: &[u8]| {
        writeln!(w, "data {}", s.len()).unwrap();
        w.write_all(s).unwrap();
        w.write_all(b"\n").unwrap();
    };
    let mut hot: Vec<String> = (0..LINES).map(|i| format!("L{i:05} r0\n")).collect();
    let mut typical: Vec<String> = (0..LINES)
        .map(|i| format!("fn line_{i}() {{}} // r0\n"))
        .collect();
    let mut t = 1_600_000_000u64;
    write!(
        w,
        "commit refs/heads/main\nmark :1\ncommitter A U Thor <a@x> {t} +0000\n"
    )
    .unwrap();
    data(&mut w, b"initial\n");
    for i in 0..FILES {
        writeln!(w, "M 100644 inline d{}/f{i}.txt", i / 1000).unwrap();
        data(&mut w, format!("file {i}\n").as_bytes());
    }
    writeln!(w, "M 100644 inline old/hot.txt").unwrap();
    data(&mut w, hot.concat().as_bytes());
    writeln!(w, "M 100644 inline src/typical.txt").unwrap();
    data(&mut w, typical.concat().as_bytes());
    for c in 1..COMMITS {
        t += 60;
        let k = (c * 7919) % LINES;
        hot[k] = format!("L{k:05} r{c}\n");
        let path = if c >= COMMITS / 2 {
            "src/hot.txt"
        } else {
            "old/hot.txt"
        };
        let who = ["alice", "bob", "carol"][c % 3];
        write!(w, "commit refs/heads/main\nmark :{}\nauthor {who} <{who}@x> {t} +0000\ncommitter {who} <{who}@x> {t} +0000\n", c + 1).unwrap();
        data(&mut w, format!("change {c}\n").as_bytes());
        writeln!(w, "from :{c}").unwrap();
        if c == COMMITS / 2 {
            writeln!(w, "D old/hot.txt").unwrap();
        }
        writeln!(w, "M 100644 inline {path}").unwrap();
        data(&mut w, hot.concat().as_bytes());
        if c % 10 == 0 {
            for j in 0..5 {
                let k = (c * 31 + j * 997) % LINES;
                typical[k] = format!("fn line_{k}() {{}} // r{c}\n");
            }
            writeln!(w, "M 100644 inline src/typical.txt").unwrap();
            data(&mut w, typical.concat().as_bytes());
        }
    }
    drop(w);
    assert!(child.wait().unwrap().success());
    // fast-import writes long delta chains; a repack gives the pack a normal clone would have.
    git(&["repack", "-adf", "-q", "--depth=50", "--window=10"]);
    // What a user repo has after any automatic gc: a commit-graph for fast history walks.
    git(&["gc", "-q"]);
    git(&["read-tree", "-m", "-u", "HEAD"]);
    git(&["update-index", "--refresh", "-q"]);
}

fn fixture() -> (Option<common::Fixture>, PathBuf, Options) {
    let f = common::Fixture::bare_dir();
    let env = f.env();
    let (keep, dir) = match std::env::var_os("VR_GIT_BENCH_DIR") {
        Some(d) => (None, PathBuf::from(d)),
        None => (Some(()), f.dir.join("bench")),
    };
    if !dir.join(".git").exists() {
        std::fs::create_dir_all(&dir).unwrap();
        let t = Instant::now();
        build(&dir, &env);
        println!("fixture built in {:?}", t.elapsed());
    }
    let opts = Options {
        env: Some(env),
        ..Options::default()
    };
    (keep.map(|_| f), dir, opts)
}

fn timed<T>(label: &str, f: impl FnOnce() -> T) -> (T, Duration) {
    let t = Instant::now();
    let v = f();
    let d = t.elapsed();
    println!("{label}: {d:?}");
    (v, d)
}

#[test]
#[ignore]
fn bench_ac16e() {
    let (_keep, dir, opts) = fixture();
    std::fs::write(dir.join("d7/f7001.txt"), "changed\n").unwrap();
    let repo = Repo::discover_with(&dir, opts).unwrap().unwrap();

    let (st, _) = timed("status, 50k files", || repo.status().unwrap());
    assert_eq!(st.unstaged.len(), 1);
    let (log, d_log) = timed("log --follow, 5k-commit file", || {
        repo.log(10_000, Some(Path::new("src/hot.txt"))).unwrap()
    });
    assert_eq!(log.len(), COMMITS);
    assert_eq!(
        log.last().unwrap().path.as_deref(),
        Some(Path::new("old/hot.txt"))
    );
    let (_, d_log_page) = timed("log --follow, first 200", || {
        repo.log(200, Some(Path::new("src/hot.txt"))).unwrap()
    });
    let (b, d_typical) = timed("blame 5k lines, 500 commits", || {
        repo.blame(Path::new("src/typical.txt")).unwrap().unwrap()
    });
    assert_eq!(b.lines.len(), LINES);
    let (b, d_hot) = timed("blame 5k lines, 5k commits + rename", || {
        repo.blame(Path::new("src/hot.txt")).unwrap().unwrap()
    });
    assert_eq!(b.lines.len(), LINES);
    let text = std::fs::read_to_string(dir.join("src/typical.txt"))
        .unwrap()
        .replace("line_42()", "line_42_edited()");
    let (marks, d_gutter) = timed("line_changes 5k lines", || {
        repo.line_changes(Path::new("src/typical.txt"), &text)
            .unwrap()
            .unwrap()
    });
    assert_eq!(marks.len(), 1);

    // Off the caller's thread: the request returns at once, the answer arrives later.
    let (tx, rx) = mpsc::channel();
    let w = GitWorker::start(repo.clone(), move || {
        let _ = tx.send(());
    });
    let mut worst = Duration::ZERO;
    let mut ids = Vec::new();
    for req in [
        Request::Status,
        Request::Blame(dir.join("src/typical.txt")),
        Request::Log {
            limit: 10_000,
            path: Some(dir.join("src/hot.txt")),
        },
        Request::LineChanges {
            path: dir.join("src/typical.txt"),
            text: Arc::from(text.as_str()),
        },
    ] {
        let t = Instant::now();
        ids.push(w.request(req));
        worst = worst.max(t.elapsed());
    }
    println!("worst request() call on the caller thread: {worst:?}");
    assert!(worst < Duration::from_millis(5));
    let last = *ids.last().unwrap();
    let t = Instant::now();
    loop {
        let mut done = false;
        while let Some(r) = w.try_recv() {
            assert!(r.result.is_ok(), "{:?}", r.result);
            if r.id == last {
                done = true;
                assert!(matches!(
                    r.result.as_deref(),
                    Ok(Reply::LineChanges(Some(_)))
                ));
            }
        }
        if done {
            break;
        }
        rx.recv_timeout(Duration::from_secs(60))
            .expect("worker answered");
    }
    println!("worker answered all four in {:?}", t.elapsed());
    println!(
        "RESULT log_all={d_log:?} log_200={d_log_page:?} blame_typical={d_typical:?} blame_hot={d_hot:?} gutter={d_gutter:?}"
    );
    assert!(
        d_typical < Duration::from_secs(1),
        "AC-16e blame of a 5000-line file"
    );
}
