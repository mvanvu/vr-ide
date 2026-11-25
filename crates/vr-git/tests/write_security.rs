//! Threat model SEC-9 .. SEC-14 for the write side (02_threat-model_M7.md).

mod common;

use std::ffi::OsString;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::Fixture;
use vr_git::{Cancel, CommitOptions, GitError, GitWorker, JobEvent, Options, Repo, Writer};

fn real_git() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|d| d.join("git"))
        .find(|p| p.is_file())
        .unwrap()
}

fn script(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn writer_env(
    f: &Fixture,
    env: Vec<(OsString, OsString)>,
    tweak: impl FnOnce(&mut Options),
) -> Writer {
    let mut opts = Options {
        env: Some(env),
        ..Options::default()
    };
    tweak(&mut opts);
    let r = Repo::discover_with(&f.dir, opts).unwrap().unwrap();
    Writer::open(&r).unwrap()
}

fn with_identity(f: &Fixture) -> Vec<(OsString, OsString)> {
    let mut env = f.env();
    for (k, v) in [
        ("GIT_AUTHOR_NAME", "Ann Author"),
        ("GIT_AUTHOR_EMAIL", "ann@example.com"),
        ("GIT_COMMITTER_NAME", "Cid Committer"),
        ("GIT_COMMITTER_EMAIL", "cid@example.com"),
    ] {
        env.push((k.into(), v.into()));
    }
    env
}

fn prepend_path(env: &mut [(OsString, OsString)], dir: &Path) {
    for (k, v) in env.iter_mut() {
        if k == "PATH" {
            let mut p = dir.as_os_str().to_os_string();
            p.push(":");
            p.push(&*v);
            *v = p;
        }
    }
}

fn proc_alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .map(|s| {
            s.rsplit(')')
                .next()
                .is_some_and(|t| !t.trim_start().starts_with('Z'))
        })
        .unwrap_or(false)
}

fn wait_for_file(p: &Path) -> String {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(10) {
        if let Ok(s) = std::fs::read_to_string(p)
            && s.ends_with('\n')
        {
            return s;
        }
        std::thread::yield_now();
    }
    panic!("{} never written", p.display());
}

fn gone_soon(pid: u32) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(3) {
        if !proc_alive(pid) {
            return true;
        }
        std::thread::yield_now();
    }
    false
}

// ---- SEC-9 ----

#[test]
fn held_index_lock_returns_gits_error_and_stays() {
    let f = Fixture::new();
    f.write("a", "a\n");
    f.commit_all("c");
    f.write("a", "b\n");
    let lock = f.dir.join(".git/index.lock");
    std::fs::write(&lock, "held by another process").unwrap();
    let e = f.writer().stage_file(&f.path("a")).unwrap_err();
    match &e {
        GitError::Failed { cmd, stderr, .. } => {
            assert_eq!(cmd, "add");
            assert!(stderr.contains("index.lock"), "{stderr}");
        }
        e => panic!("{e:?}"),
    }
    assert_eq!(std::fs::read(&lock).unwrap(), b"held by another process");
}

#[test]
fn path_of_another_repo_is_refused_before_git_runs() {
    let a = Fixture::new();
    a.commit_all("a");
    let b = Fixture::new();
    b.write("theirs.txt", "b\n");
    // A nested repo inside A (another root's repo, or a submodule work tree).
    a.git(&["init", "-q", "nested"]);
    a.write("nested/inner.txt", "n\n");

    let log = a.tmp.path().join("calls.log");
    let shim = a.tmp.path().join("shim/git");
    script(
        &shim,
        &format!(
            "echo \"$*\" >> '{}'\nexec '{}' \"$@\"",
            log.display(),
            real_git().display()
        ),
    );
    let mut env = a.env();
    prepend_path(&mut env, shim.parent().unwrap());
    let w = writer_env(&a, env, |_| {});
    let calls = || {
        std::fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .count()
    };
    let before = calls();
    let e = w.stage_file(&b.path("theirs.txt")).unwrap_err();
    assert!(matches!(e, GitError::OutsideRepo(_)), "{e:?}");
    assert!(w.prepare_discard_file(&b.path("theirs.txt")).is_err());
    let e = w.stage_file(&a.path("nested/inner.txt")).unwrap_err();
    assert!(matches!(e, GitError::Refused(_)), "{e:?}");
    assert!(w.prepare_discard_file(&a.path("nested/inner.txt")).is_err());
    assert!(w.stage_file(&a.path("../repo/../x")).is_err());
    assert_eq!(calls(), before, "git was started for a refused path");
}

#[test]
fn write_modules_use_no_forbidden_commands_or_flags() {
    let sources = [
        include_str!("../src/write.rs"),
        include_str!("../src/remote.rs"),
        include_str!("../src/patch.rs"),
        include_str!("../src/recovery.rs"),
    ];
    let forbidden = [
        "\"--force\"",
        "\"-f\"",
        "\"--force-with-lease\"",
        "\"--hard\"",
        "\"reset\"",
        "\"clean\"",
        "\"checkout\"",
        "\"stash\"",
        "safe.directory",
        "\"--unidiff-zero\"",
        "\"--reject\"",
        "\"--unsafe-paths\"",
        "\"-m\"",
        "\"index.lock\"",
        "index.lock\")",
    ];
    for src in sources {
        for bad in forbidden {
            assert!(!src.contains(bad), "{bad} found in a write module");
        }
    }
}

#[test]
fn a_readers_shutdown_never_kills_a_writer() {
    let f = Fixture::new();
    f.write("a", "a\n");
    f.commit_all("c");
    f.write("a", "b\n");
    let r = f.repo();
    let w = Writer::open(&r).unwrap();
    let worker = GitWorker::start(r.clone(), || {});
    drop(worker);
    assert!(matches!(r.status(), Err(GitError::ShutDown)));
    w.stage_file(&f.path("a")).unwrap();
    assert!(!w.repo().status().unwrap().staged.is_empty());
}

#[test]
fn writes_queue_one_at_a_time_per_repo() {
    let f = Fixture::new();
    f.commit_all("c");
    let log = f.tmp.path().join("hook.log");
    // A slow pre-commit hook: a second write must wait for the first.
    script(
        &f.dir.join(".git/hooks/pre-commit"),
        &format!(
            "echo start >> '{0}'; sleep 0.4; echo end >> '{0}'",
            log.display()
        ),
    );
    f.write("a", "1\n");
    f.git(&["add", "a"]);
    let w1 = writer_env(&f, with_identity(&f), |_| {});
    let w2 = writer_env(&f, with_identity(&f), |_| {});
    let t =
        std::thread::spawn(move || w1.commit("first", CommitOptions::default(), &Cancel::new()));
    let start = Instant::now();
    while !log.exists() && start.elapsed() < Duration::from_secs(5) {
        std::thread::yield_now();
    }
    f.write("b", "2\n");
    // Staging waits until the record has finished.
    w2.stage_file(&f.path("b")).unwrap();
    assert_eq!(std::fs::read_to_string(&log).unwrap(), "start\nend\n");
    t.join().unwrap().unwrap();
}

// ---- SEC-12 ----

#[test]
fn hooks_run_on_record_but_no_editor_and_no_terminal() {
    let f = Fixture::new();
    f.write("a", "1\n");
    f.commit_all("c");
    let (editor, editor_ran) = f.marker_script("editor");
    f.git(&["config", "core.editor", editor.to_str().unwrap()]);
    let pre = f.tmp.path().join("pre-commit.ran");
    let msg = f.tmp.path().join("commit-msg.ran");
    let tty = f.tmp.path().join("tty.ran");
    script(
        &f.dir.join(".git/hooks/pre-commit"),
        &format!(
            "touch '{}'; if (exec 3<>/dev/tty) 2>/dev/null; then touch '{}'; fi",
            pre.display(),
            tty.display()
        ),
    );
    script(
        &f.dir.join(".git/hooks/commit-msg"),
        &format!("cp \"$1\" '{}'", msg.display()),
    );
    f.write("a", "2\n");
    f.git(&["add", "a"]);
    let w = writer_env(&f, with_identity(&f), |_| {});
    w.commit("hooked\n", CommitOptions::default(), &Cancel::new())
        .unwrap();
    assert!(pre.exists(), "pre-commit must run (D298(1))");
    assert_eq!(std::fs::read_to_string(&msg).unwrap(), "hooked\n");
    assert!(!editor_ran.exists(), "an editor was started");
    assert!(!tty.exists(), "a hook reached the terminal");
    // Reads still never run hooks.
    let reader = f.repo();
    reader.status().unwrap();
}

#[test]
fn failing_hook_reports_its_text_and_cancel_kills_a_hung_hook() {
    let f = Fixture::new();
    f.write("a", "1\n");
    f.commit_all("c");
    f.write("a", "2\n");
    f.git(&["add", "a"]);
    let hook = f.dir.join(".git/hooks/pre-commit");
    script(&hook, "echo 'lint failed: a' >&2; exit 1");
    let w = writer_env(&f, with_identity(&f), |_| {});
    match w.commit("m", CommitOptions::default(), &Cancel::new()) {
        Err(GitError::Failed { stderr, .. }) => assert!(stderr.contains("lint failed"), "{stderr}"),
        r => panic!("{r:?}"),
    }
    let pidfile = f.tmp.path().join("hook.pid");
    script(
        &hook,
        &format!("sleep 30 & echo $! > '{}'; wait", pidfile.display()),
    );
    let cancel = Cancel::new();
    let c2 = cancel.clone();
    let t = std::thread::spawn(move || w.commit("m", CommitOptions::default(), &c2));
    let pid: u32 = wait_for_file(&pidfile).trim().parse().unwrap();
    cancel.cancel();
    let r = t.join().unwrap();
    assert!(matches!(r, Err(GitError::Cancelled)), "{r:?}");
    assert!(gone_soon(pid), "hook child still alive");
    assert_eq!(f.git_s(&["log", "--format=%s"]).lines().count(), 1);
}

// ---- SEC-13 / SEC-14: push and pull ----

/// A bare "remote", and a clone with one pushed commit on main.
fn remote_and_clone() -> (Fixture, Fixture) {
    let remote = Fixture::bare_dir();
    remote.git(&["init", "-q", "--bare", "-b", "main"]);
    let f = Fixture::new();
    f.git(&["remote", "add", "origin", remote.dir.to_str().unwrap()]);
    f.write("a.txt", "base\n");
    f.commit_all("base");
    f.git(&["push", "-q", "-u", "origin", "main"]);
    (remote, f)
}

fn second_clone(remote: &Fixture) -> Fixture {
    let g = Fixture::bare_dir();
    g.git(&["clone", "-q", remote.dir.to_str().unwrap(), "."]);
    g
}

#[test]
fn push_and_pull_through_a_local_remote() {
    let (remote, f) = remote_and_clone();
    f.write("a.txt", "base\nmore\n");
    f.commit_all("second");
    let w = writer_env(&f, f.env(), |_| {});
    let target = w.push_target().unwrap();
    assert_eq!(target.to_string(), "origin/main");
    assert!(!target.set_upstream);
    let out = w.push(target, || {}).wait().unwrap();
    assert!(out.state.is_clean());
    assert_eq!(
        remote.git_s(&["rev-parse", "main"]),
        f.git_s(&["rev-parse", "HEAD"])
    );

    // A new branch gets an upstream.
    f.git(&["switch", "-q", "-c", "topic"]);
    let target = w.push_target().unwrap();
    assert_eq!(target.to_string(), "origin/topic");
    assert!(target.set_upstream);
    w.push(target, || {}).wait().unwrap();
    assert_eq!(
        f.git_s(&["rev-parse", "--abbrev-ref", "topic@{upstream}"])
            .trim(),
        "origin/topic"
    );

    // Pull a fast-forward made elsewhere.
    f.git(&["switch", "-q", "main"]);
    let g = second_clone(&remote);
    g.write("b.txt", "from g\n");
    g.commit_all("from g");
    g.git(&["push", "-q"]);
    let job = w.pull(|| {});
    let mut events = 0;
    let out = loop {
        match job.try_recv() {
            Some(JobEvent::Done(r)) => break r.unwrap(),
            Some(JobEvent::Progress(_)) => events += 1,
            None => std::thread::yield_now(),
        }
    };
    assert!(out.state.is_clean());
    assert_eq!(f.read("b.txt"), b"from g\n");
    let _ = events;
}

#[test]
fn push_is_never_forced() {
    let (remote, f) = remote_and_clone();
    let g = second_clone(&remote);
    g.write("g.txt", "g\n");
    g.commit_all("g");
    g.git(&["push", "-q"]);
    let remote_head = remote.git_s(&["rev-parse", "main"]);
    f.write("f.txt", "f\n");
    f.commit_all("f");
    let w = writer_env(&f, f.env(), |_| {});
    let e = w.push(w.push_target().unwrap(), || {}).wait().unwrap_err();
    match e {
        GitError::Failed { stderr, .. } => assert!(stderr.contains("rejected"), "{stderr}"),
        e => panic!("{e:?}"),
    }
    assert_eq!(remote.git_s(&["rev-parse", "main"]), remote_head);
}

#[test]
fn conflicting_pull_reports_the_merge_and_blocks_hunk_staging() {
    let (remote, f) = remote_and_clone();
    let g = second_clone(&remote);
    g.write("a.txt", "theirs\n");
    g.commit_all("theirs");
    g.git(&["push", "-q"]);
    f.write("a.txt", "ours\n");
    f.commit_all("ours");
    f.git(&["config", "pull.rebase", "false"]);
    let w = writer_env(&f, with_identity(&f), |_| {});
    let out = w.pull(|| {}).wait().unwrap();
    assert_eq!(out.state.to_string(), "merge, 1 conflict");
    assert!(
        out.output.iter().any(|l| l.contains("CONFLICT")),
        "{:?}",
        out.output
    );
    let index = std::fs::read(f.dir.join(".git/index")).unwrap();
    let d = f.repo().diff_unstaged(Path::new("a.txt")).unwrap().unwrap();
    assert!(matches!(w.stage_hunk(&d, 0), Err(GitError::Refused(_))));
    assert!(matches!(
        w.stage_file(&f.path("a.txt")),
        Err(GitError::Refused(_))
    ));
    assert!(matches!(w.pull(|| {}).wait(), Err(GitError::Refused(_))));
    assert_eq!(std::fs::read(f.dir.join(".git/index")).unwrap(), index);
}

#[test]
fn remote_text_is_sanitized_in_progress_lines() {
    let (remote, f) = remote_and_clone();
    script(
        &remote.dir.join("hooks/pre-receive"),
        "printf 'evil \\033]52;c;ZXZpbA==\\007 \\033[2J done\\n' >&2",
    );
    f.write("a.txt", "x\n");
    f.commit_all("x");
    let w = writer_env(&f, f.env(), |_| {});
    let job = w.push(w.push_target().unwrap(), || {});
    let mut lines = Vec::new();
    loop {
        match job.try_recv() {
            Some(JobEvent::Progress(l)) => lines.push(l),
            Some(JobEvent::Done(r)) => {
                r.unwrap();
                break;
            }
            None => std::thread::yield_now(),
        }
    }
    let evil: Vec<&String> = lines.iter().filter(|l| l.contains("evil")).collect();
    assert_eq!(evil.len(), 1, "{lines:?}");
    assert!(evil[0].starts_with("remote: "), "{}", evil[0]);
    assert!(
        !evil[0].contains('\x1b') && !evil[0].contains('\x07'),
        "{}",
        evil[0]
    );
    assert!(evil[0].contains("^[]52;c;ZXZpbA==^G"), "{}", evil[0]);
}

#[test]
fn ssh_cannot_reach_the_terminal_and_cancel_kills_it() {
    let (_remote, f) = remote_and_clone();
    f.git(&[
        "remote",
        "set-url",
        "origin",
        "ssh://git@example.invalid/r.git",
    ]);
    let tty = f.tmp.path().join("tty.ran");
    let pidfile = f.tmp.path().join("ssh.pid");
    let askpass_ran = f.tmp.path().join("askpass.ran");
    let shim = f.tmp.path().join("bin/fake-ssh");
    script(
        &shim,
        &format!(
            "if (exec 3<>/dev/tty) 2>/dev/null; then touch '{}'; fi\nsleep 30 & echo $! > '{}'; wait",
            tty.display(),
            pidfile.display()
        ),
    );
    let askpass = f.tmp.path().join("bin/askpass");
    script(&askpass, &format!("touch '{}'", askpass_ran.display()));
    let mut env = f.env();
    env.push(("GIT_SSH_COMMAND".into(), shim.clone().into()));
    env.push(("SSH_ASKPASS".into(), askpass.clone().into()));
    env.push(("GIT_ASKPASS".into(), askpass.clone().into()));
    env.push(("DISPLAY".into(), ":0".into()));
    let w = writer_env(&f, env, |_| {});
    let job = w.pull(|| {});
    let pid: u32 = wait_for_file(&pidfile).trim().parse().unwrap();
    job.cancel();
    let r = job.wait();
    assert!(matches!(r, Err(GitError::Cancelled)), "{r:?}");
    assert!(gone_soon(pid), "ssh child still alive");
    assert!(!tty.exists(), "ssh opened the terminal");
    assert!(!askpass_ran.exists());
}

#[test]
fn silent_transport_is_killed_after_the_idle_limit() {
    let (_remote, f) = remote_and_clone();
    f.git(&["remote", "set-url", "origin", "ssh://example.invalid/r.git"]);
    let shim = f.tmp.path().join("bin/silent-ssh");
    script(&shim, "exec sleep 30");
    let mut env = f.env();
    env.push(("GIT_SSH_COMMAND".into(), shim.into()));
    let w = writer_env(&f, env, |o| {
        o.network_idle_timeout = Duration::from_millis(400)
    });
    let t = Instant::now();
    let r = w.pull(|| {}).wait();
    assert!(matches!(r, Err(GitError::Timeout(_))), "{r:?}");
    assert!(t.elapsed() < Duration::from_secs(5));
}

#[test]
fn ssh_runs_in_batch_mode_by_default() {
    let (_remote, f) = remote_and_clone();
    f.git(&["remote", "set-url", "origin", "ssh://example.invalid/r.git"]);
    let args = f.tmp.path().join("ssh.args");
    let bin = f.tmp.path().join("bin");
    script(
        &bin.join("ssh"),
        &format!("echo \"$*\" > '{}'; exit 255", args.display()),
    );
    let mut env = f.env();
    prepend_path(&mut env, &bin);
    let w = writer_env(&f, env, |_| {});
    assert!(w.pull(|| {}).wait().is_err());
    let used = std::fs::read_to_string(&args).unwrap();
    assert!(used.contains("-o BatchMode=yes"), "{used}");
}

/// A local HTTP server answering every request with 401 (a remote that needs credentials).
fn auth_server() -> (u16, std::thread::JoinHandle<()>) {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    l.set_nonblocking(true).unwrap();
    let h = std::thread::spawn(move || {
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(10) {
            match l.accept() {
                Ok((mut s, _)) => {
                    s.set_nonblocking(false).unwrap();
                    s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
                    let mut buf = [0u8; 4096];
                    let _ = s.read(&mut buf);
                    let _ = s.write_all(
                        b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"r\"\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                }
                Err(_) => std::thread::sleep(Duration::from_millis(20)),
            }
        }
    });
    (port, h)
}

#[test]
fn http_auth_without_a_helper_fails_at_once_and_hides_the_token() {
    let (_remote, f) = remote_and_clone();
    let (port, _server) = auth_server();
    let askpass_ran = f.tmp.path().join("askpass.ran");
    let askpass = f.tmp.path().join("bin/askpass");
    script(
        &askpass,
        &format!("touch '{}'; echo secret", askpass_ran.display()),
    );
    let mut env = f.env();
    env.push(("GIT_ASKPASS".into(), askpass.clone().into()));
    env.push(("SSH_ASKPASS".into(), askpass.into()));
    for url in [
        format!("http://127.0.0.1:{port}/r.git"),
        format!("http://u:tok3n@127.0.0.1:{port}/r.git"),
    ] {
        f.git(&["remote", "set-url", "origin", &url]);
        let w = writer_env(&f, env.clone(), |_| {});
        let t = Instant::now();
        let job = w.pull(|| {});
        let mut lines = Vec::new();
        let r = loop {
            match job.try_recv() {
                Some(JobEvent::Progress(l)) => lines.push(l),
                Some(JobEvent::Done(r)) => break r,
                None => std::thread::yield_now(),
            }
        };
        assert!(t.elapsed() < Duration::from_secs(8), "{:?}", t.elapsed());
        let e = r.unwrap_err();
        let text = format!("{e} {lines:?}");
        assert!(!text.contains("tok3n"), "{text}");
        assert!(!askpass_ran.exists(), "askpass ran");
        assert!(matches!(e, GitError::Failed { .. }), "{e:?}");
    }
}

// ---- Round 1 (05_review_M7-2.md, D305) ----

/// A signed record: the `gpg.program` shim must never learn the editor's terminal, so a
/// curses pinentry cannot draw over the TUI or read its keys (SEC-12, D305(1)).
#[test]
fn signing_never_sees_the_terminal_through_gpg_tty() {
    let f = Fixture::new();
    f.write("a", "1\n");
    f.commit_all("c");
    f.write("a", "2\n");
    f.git(&["add", "a"]);
    let seen = f.tmp.path().join("gpg.env");
    let shim = f.tmp.path().join("gpg-shim");
    script(
        &shim,
        &format!(
            "printf 'GPG_TTY=%s\\nGPG_AGENT_INFO=%s\\nPINENTRY_USER_DATA=%s\\n' \
             \"${{GPG_TTY-unset}}\" \"${{GPG_AGENT_INFO-unset}}\" \"${{PINENTRY_USER_DATA-unset}}\" \
             > '{}'; exit 1",
            seen.display()
        ),
    );
    f.git(&["config", "gpg.program", shim.to_str().unwrap()]);
    f.git(&["config", "commit.gpgSign", "true"]);
    let mut env = with_identity(&f);
    env.push(("GPG_TTY".into(), "/dev/pts/77".into()));
    env.push((
        "GPG_AGENT_INFO".into(),
        "/run/user/1/gnupg/S.gpg-agent:0:1".into(),
    ));
    env.push(("PINENTRY_USER_DATA".into(), "USE_CURSES=1".into()));
    let w = writer_env(&f, env, |_| {});
    let r = w.commit("signed\n", CommitOptions::default(), &Cancel::new());
    assert!(matches!(r, Err(GitError::Failed { .. })), "{r:?}");
    let seen = std::fs::read_to_string(&seen).expect("the shim gpg ran");
    assert!(seen.contains("GPG_TTY=/dev/null\n"), "{seen}");
    assert!(!seen.contains("pts/77"), "{seen}");
    assert!(seen.contains("GPG_AGENT_INFO=unset\n"), "{seen}");
    assert!(seen.contains("PINENTRY_USER_DATA=unset\n"), "{seen}");
}

/// git's own error text from an index or record write is made safe to show (SEC-15 rules).
#[test]
fn git_error_text_from_writes_is_sanitized() {
    let f = Fixture::new();
    f.write("a", "1\n");
    f.commit_all("c");
    f.write("a", "2\n");
    f.git(&["add", "a"]);
    script(
        &f.dir.join(".git/hooks/pre-commit"),
        "printf 'lint \\033]52;c;aGk=\\007 failed\\n' >&2; exit 1",
    );
    let w = writer_env(&f, with_identity(&f), |_| {});
    match w.commit("m", CommitOptions::default(), &Cancel::new()) {
        Err(GitError::Failed { stderr, .. }) => {
            assert!(
                !stderr.contains('\u{1b}') && !stderr.contains('\u{7}'),
                "{stderr:?}"
            );
            assert!(stderr.contains("lint ^[]52;c;aGk=^G failed"), "{stderr:?}");
        }
        r => panic!("{r:?}"),
    }
}

/// A triangular setup (pull from origin, push to a fork) keeps its upstream: push sets one
/// only for a branch that has none (D305(2)).
#[test]
fn push_keeps_the_upstream_of_a_triangular_setup() {
    for how in ["remote.pushDefault", "branch.main.pushRemote"] {
        let (_origin, f) = remote_and_clone();
        let fork = Fixture::bare_dir();
        fork.git(&["init", "-q", "--bare", "-b", "main"]);
        f.git(&["remote", "add", "fork", fork.dir.to_str().unwrap()]);
        f.git(&["config", how, "fork"]);
        f.write("a.txt", "base\nfork work\n");
        f.commit_all("fork work");
        let w = writer_env(&f, f.env(), |_| {});
        let target = w.push_target().unwrap();
        assert_eq!(target.to_string(), "fork/main", "{how}");
        assert!(!target.set_upstream, "{how}");
        w.push(target, || {}).wait().unwrap();
        assert_eq!(
            fork.git_s(&["rev-parse", "main"]),
            f.git_s(&["rev-parse", "HEAD"])
        );
        assert_eq!(f.git_s(&["config", "branch.main.remote"]).trim(), "origin");
        assert_eq!(
            f.git_s(&["config", "branch.main.merge"]).trim(),
            "refs/heads/main"
        );
    }
}

/// A parent folder swapped for a symlink between confirmation and execution: the discard
/// must not act through it (SEC-6, SEC-9).
#[test]
fn discard_does_not_follow_a_parent_swapped_for_a_symlink() {
    let f = Fixture::new();
    f.write("keep", "k\n");
    f.commit_all("c");
    f.write("sub/a.txt", "same\n");
    let outside = f.tmp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("a.txt"), "same\n").unwrap();
    let w = f.writer();
    let plan = w.prepare_discard_file(&f.path("sub/a.txt")).unwrap();
    std::fs::rename(f.path("sub"), f.tmp.path().join("sub.moved")).unwrap();
    std::os::unix::fs::symlink(&outside, f.path("sub")).unwrap();
    let r = w.discard(&plan, false);
    assert!(r.is_err(), "{r:?}");
    assert_eq!(std::fs::read(outside.join("a.txt")).unwrap(), b"same\n");
}
