//! Threat model M7 SEC-1..SEC-8 on the read side. Every trap is first sprung with plain git
//! (the control), so a passing test proves the protection, not a dead trap.

mod common;

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use common::Fixture;
use vr_git::{GitError, GitWorker, Options, Repo, Request};

fn real_git() -> PathBuf {
    let path = std::env::var_os("PATH").unwrap();
    std::env::split_paths(&path)
        .map(|d| d.join("git"))
        .find(|p| p.is_file())
        .expect("git in PATH")
}

/// Run every read query once; results are ignored, only side effects matter.
fn read_everything(r: &Repo, file: &str) {
    let p = Path::new(file);
    let _ = r.head();
    let _ = r.ahead_behind();
    r.status().expect("status works with the trap disarmed");
    let _ = r.diff_unstaged(p);
    let _ = r.diff_staged(p);
    let _ = r.blame(p);
    let _ = r.blame_text(p, "x\ny\n");
    let _ = r.line_changes(p, "x\ny\n");
    let _ = r.log(10, None);
    let _ = r.log(10, Some(p));
    let _ = r.commit("HEAD");
    let _ = r.diff_commit("HEAD", None);
    let _ = r.diff_commit("HEAD", Some(p));
}

/// Plain git with the fixture env; its exit status does not matter.
fn plain(f: &Fixture, args: &[&str]) {
    let _ = Command::new("git")
        .current_dir(&f.dir)
        .env_clear()
        .envs(f.env())
        .args(args)
        .output()
        .unwrap();
}

#[derive(Clone, Copy)]
enum Scope {
    Local,
    Global,
}

/// A repo with a tracked, modified `a.txt` (two commits) and the hostile `key = marker script`.
fn hostile(key: &str, attrs: Option<&str>, scope: Scope) -> (Fixture, PathBuf) {
    let f = Fixture::new();
    if let Some(a) = attrs {
        f.write(".gitattributes", a);
    }
    f.write("a.txt", "one\n");
    f.commit_all("first");
    f.write("a.txt", "one\ntwo\n");
    f.commit_all("second");
    f.write("a.txt", "one\ntwo\nthree\n");
    let (script, marker) = f.marker_script(&key.replace('.', "_"));
    let s = script.to_str().unwrap();
    match scope {
        Scope::Local => {
            f.git(&["config", key, s]);
        }
        Scope::Global => {
            f.git(&["config", "--global", key, s]);
        }
    }
    (f, marker)
}

fn check_key(key: &str, attrs: Option<&str>, control: &[&[&str]]) {
    for scope in [Scope::Local, Scope::Global] {
        let (f, marker) = hostile(key, attrs, scope);
        for c in control {
            plain(&f, c);
        }
        assert!(marker.exists(), "control: plain git must run {key}");
        std::fs::remove_file(&marker).unwrap();
        // Touch the file so stat info is stale and git has to read (and convert) it again.
        f.write("a.txt", "one\ntwo\nthree\nfour\n");
        read_everything(&f.repo(), "a.txt");
        assert!(!marker.exists(), "{key} ran during a read");
    }
}

#[test]
fn core_fsmonitor_never_runs() {
    check_key("core.fsmonitor", None, &[&["status"]]);
}

#[test]
fn filter_clean_never_runs() {
    check_key("filter.x.clean", Some("* filter=x\n"), &[&["diff"]]);
}

#[test]
fn filter_process_is_disabled_by_the_empty_override() {
    // D298 (3): the empty `filter.<d>.process=` override must disable a process filter on this git.
    check_key("filter.x.process", Some("* filter=x\n"), &[&["diff"]]);
}

#[test]
fn required_filter_does_not_break_reads() {
    let (f, marker) = hostile("filter.x.clean", Some("* filter=x\n"), Scope::Local);
    f.git(&["config", "filter.x.required", "true"]);
    read_everything(&f.repo(), "a.txt");
    assert!(!marker.exists());
}

#[test]
fn diff_textconv_never_runs() {
    check_key(
        "diff.x.textconv",
        Some("* diff=x\n"),
        &[&["diff"], &["log", "-p", "-1"]],
    );
}

#[test]
fn diff_command_never_runs() {
    check_key("diff.x.command", Some("* diff=x\n"), &[&["diff"]]);
}

#[test]
fn diff_external_never_runs() {
    check_key("diff.external", None, &[&["diff"]]);
}

#[test]
fn core_pager_never_runs() {
    // git pages only to a terminal, so the control shows git would pick the marker as pager.
    for scope in [Scope::Local, Scope::Global] {
        let (f, marker) = hostile("core.pager", None, scope);
        let pager = f.git_s(&["var", "GIT_PAGER"]);
        assert!(
            pager.contains("core_pager.sh"),
            "control: git would page through {pager}"
        );
        read_everything(&f.repo(), "a.txt");
        assert!(!marker.exists());
    }
}

#[test]
fn show_signature_gpg_program_never_runs() {
    for scope in [Scope::Local, Scope::Global] {
        let (f, marker) = hostile("gpg.program", None, scope);
        // A commit carrying a (fake) signature so --show-signature has something to verify.
        let raw = f.git_s(&["cat-file", "commit", "HEAD"]);
        let (head, msg) = raw.split_once("\n\n").unwrap();
        let signed = format!(
            "{head}\ngpgsig -----BEGIN PGP SIGNATURE-----\n \n iQEz\n -----END PGP SIGNATURE-----\n\n{msg}"
        );
        let tmp = f.tmp.path().join("signed");
        std::fs::write(&tmp, signed).unwrap();
        let oid = f
            .git_s(&["hash-object", "-t", "commit", "-w", tmp.to_str().unwrap()])
            .trim()
            .to_string();
        f.git(&["update-ref", "refs/heads/main", &oid]);
        f.git(&["config", "log.showSignature", "true"]);
        plain(&f, &["log", "-1"]);
        assert!(marker.exists(), "control: plain git log runs gpg.program");
        std::fs::remove_file(&marker).unwrap();
        read_everything(&f.repo(), "a.txt");
        assert!(!marker.exists());
    }
}

#[test]
fn hooks_never_run() {
    let f = Fixture::new();
    f.write("a.txt", "one\n");
    f.commit_all("first");
    let (script, marker) = f.marker_script("hook");
    let hooks = f.dir.join(".git/hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    std::fs::copy(&script, hooks.join("post-index-change")).unwrap();
    plain(&f, &["update-index", "--refresh"]);
    plain(&f, &["add", "a.txt"]);
    f.write("a.txt", "two\n");
    plain(&f, &["status"]);
    assert!(marker.exists(), "control: plain git runs post-index-change");
    std::fs::remove_file(&marker).unwrap();
    // Also through core.hooksPath.
    let dir = f.tmp.path().join("hookdir");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(&script, dir.join("post-index-change")).unwrap();
    f.git(&["config", "core.hooksPath", dir.to_str().unwrap()]);
    f.write("a.txt", "three\n");
    read_everything(&f.repo(), "a.txt");
    assert!(!marker.exists());
}

#[test]
fn outer_git_env_is_ignored() {
    let f = Fixture::new();
    f.write("mine.txt", "x\n");
    f.commit_all("mine");
    f.write("mine.txt", "changed\n");
    let other = Fixture::new();
    other.write("theirs.txt", "y\n");
    other.commit_all("theirs");
    other.write("theirs.txt", "changed\n");
    let (script, marker) = f.marker_script("env_fsmonitor");
    let trace = f.tmp.path().join("trace.log");
    let mut env = f.env();
    let o = |s: &str| OsString::from(s);
    env.extend([
        (o("GIT_DIR"), other.dir.join(".git").into_os_string()),
        (o("GIT_WORK_TREE"), other.dir.clone().into_os_string()),
        (
            o("GIT_INDEX_FILE"),
            other.dir.join(".git/index").into_os_string(),
        ),
        (o("GIT_CONFIG_COUNT"), o("1")),
        (o("GIT_CONFIG_KEY_0"), o("core.fsmonitor")),
        (o("GIT_CONFIG_VALUE_0"), script.into_os_string()),
        (o("GIT_CONFIG_PARAMETERS"), o("'core.pager'='false'")),
        (o("GIT_TRACE"), trace.clone().into_os_string()),
        (o("GIT_EXTERNAL_DIFF"), o("/bin/false")),
    ]);
    let r = Repo::discover_with(
        &f.dir,
        Options {
            env: Some(env),
            ..Options::default()
        },
    )
    .unwrap()
    .unwrap();
    assert_eq!(r.workdir(), f.dir);
    let st = r.status().unwrap();
    assert_eq!(st.unstaged.len(), 1);
    assert_eq!(st.unstaged[0].path, Path::new("mine.txt"));
    assert!(r.diff_unstaged(Path::new("mine.txt")).unwrap().is_some());
    assert!(!marker.exists());
    assert!(!trace.exists());
}

fn shim_dir(f: &Fixture, body: &str) -> PathBuf {
    let dir = f.tmp.path().join("shim");
    std::fs::create_dir_all(&dir).unwrap();
    let shim = dir.join("git");
    std::fs::write(
        &shim,
        format!(
            "#!/bin/sh\nREAL='{}'\n{body}\nexec \"$REAL\" \"$@\"\n",
            real_git().display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    dir
}

fn env_with_path(f: &Fixture, first: &Path) -> Vec<(OsString, OsString)> {
    let mut env = f.env();
    for (k, v) in env.iter_mut() {
        if k == "PATH" {
            let mut p = first.as_os_str().to_os_string();
            p.push(":");
            p.push(&*v);
            *v = p;
        }
    }
    env
}

#[test]
fn git_has_no_controlling_terminal() {
    let f = Fixture::new();
    f.commit_all("c");
    let marker = f.tmp.path().join("tty.ran");
    let dir = shim_dir(
        &f,
        &format!(
            "if (exec 3<>/dev/tty) 2>/dev/null; then touch '{}'; fi",
            marker.display()
        ),
    );
    let r = Repo::discover_with(
        &f.dir,
        Options {
            env: Some(env_with_path(&f, &dir)),
            ..Options::default()
        },
    )
    .unwrap()
    .unwrap();
    r.status().unwrap();
    assert!(!marker.exists(), "git opened /dev/tty");
}

fn proc_alive(pid: u32) -> bool {
    // A zombie waiting for init to reap it counts as gone.
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .map(|s| {
            s.rsplit(')')
                .next()
                .map(|t| !t.trim_start().starts_with('Z'))
                .unwrap_or(false)
        })
        .unwrap_or(false)
}

#[test]
fn hung_git_is_killed_with_its_children_at_the_timeout() {
    let f = Fixture::new();
    f.commit_all("c");
    let pidfile = f.tmp.path().join("sleep.pid");
    let body = format!(
        "case \"$*\" in *status*) sleep 30 & echo $! > '{}'; wait;; esac",
        pidfile.display()
    );
    let dir = shim_dir(&f, &body);
    let opts = Options {
        env: Some(env_with_path(&f, &dir)),
        timeout: Duration::from_millis(500),
        ..Options::default()
    };
    let r = Repo::discover_with(&f.dir, opts).unwrap().unwrap();
    let t = Instant::now();
    let res = r.status();
    assert!(matches!(res, Err(GitError::Timeout(_))), "{res:?}");
    assert!(t.elapsed() < Duration::from_secs(5), "{:?}", t.elapsed());
    let pid: u32 = std::fs::read_to_string(&pidfile)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let gone = (0..200).any(|_| {
        if proc_alive(pid) {
            std::thread::sleep(Duration::from_millis(10));
            false
        } else {
            true
        }
    });
    assert!(gone, "child sleep {pid} survived the timeout");
}

#[test]
fn git_in_the_repo_is_never_run_from_a_relative_path_entry() {
    let f = Fixture::new();
    f.commit_all("c");
    let marker = f.tmp.path().join("dotgit.ran");
    let planted = f.dir.join("git");
    std::fs::write(
        &planted,
        format!("#!/bin/sh\ntouch '{}'\n", marker.display()),
    )
    .unwrap();
    std::fs::set_permissions(&planted, std::fs::Permissions::from_mode(0o755)).unwrap();
    let env = env_with_path(&f, Path::new("."));
    let r = Repo::discover_with(
        &f.dir,
        Options {
            env: Some(env),
            ..Options::default()
        },
    )
    .unwrap()
    .unwrap();
    r.status().unwrap();
    assert!(!marker.exists());
}

#[test]
fn repo_owned_by_another_user_is_refused_unless_safe_directory_allows() {
    let f = Fixture::new();
    f.commit_all("c");
    // git's own ownership check, simulated the way git's test suite does it.
    let dir = shim_dir(&f, "export GIT_TEST_ASSUME_DIFFERENT_OWNER=1");
    let env = env_with_path(&f, &dir);
    let opts = || Options {
        env: Some(env.clone()),
        ..Options::default()
    };
    assert!(Repo::discover_with(&f.dir, opts()).unwrap().is_none());
    f.git(&[
        "config",
        "--global",
        "safe.directory",
        f.dir.to_str().unwrap(),
    ]);
    assert!(Repo::discover_with(&f.dir, opts()).unwrap().is_some());
}

#[test]
fn work_tree_elsewhere_is_unsupported() {
    let f = Fixture::new();
    f.commit_all("c");
    let other = f.tmp.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    f.git(&["config", "core.worktree", other.to_str().unwrap()]);
    assert!(Repo::discover_with(&f.dir, f.options()).unwrap().is_none());
}

fn listing(dir: &Path) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
    let mut v = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let e = e.unwrap();
            let m = e.metadata().unwrap();
            if m.is_dir() {
                stack.push(e.path());
            } else {
                v.push((e.path(), m.len(), m.modified().unwrap()));
            }
        }
    }
    v.sort();
    v
}

#[test]
fn reads_write_nothing_under_dot_git_and_respect_index_lock() {
    let f = Fixture::new();
    f.write("a.txt", "one\n");
    f.commit_all("first");
    f.write("a.txt", "one\ntwo\n");
    f.commit_all("second");
    // Make the index stat info stale (same content, other mtime) so a read that refreshes the
    // index, like porcelain `git diff`, would rewrite it.
    let old = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    std::fs::File::options()
        .write(true)
        .open(f.path("a.txt"))
        .unwrap()
        .set_modified(old)
        .unwrap();
    let before = listing(&f.dir.join(".git"));
    let r = f.repo();
    read_everything(&r, "a.txt");
    assert_eq!(listing(&f.dir.join(".git")), before);
    let lock = f.dir.join(".git/index.lock");
    std::fs::write(&lock, "").unwrap();
    read_everything(&r, "a.txt");
    assert!(r.status().is_ok());
    assert!(lock.exists());
}

#[test]
fn file_over_the_cap_is_too_large() {
    let f = Fixture::new();
    f.write("big.txt", "a\n");
    f.commit_all("c");
    f.write("big.txt", "x\n".repeat(10_000));
    let opts = Options {
        max_file: 4096,
        ..f.options()
    };
    let r = Repo::discover_with(&f.dir, opts).unwrap().unwrap();
    assert!(matches!(
        r.diff_unstaged(Path::new("big.txt")),
        Err(GitError::TooLarge { .. })
    ));
    assert!(matches!(
        r.line_changes(Path::new("big.txt"), &"x\n".repeat(10_000)),
        Err(GitError::TooLarge { .. })
    ));
    assert!(matches!(
        r.blame_text(Path::new("big.txt"), &"x\n".repeat(10_000)),
        Err(GitError::TooLarge { .. })
    ));
}

#[test]
fn partial_clone_never_fetches_lazily() {
    let up = Fixture::new();
    up.write("a.txt", "one\n");
    up.commit_all("c1");
    up.write("a.txt", "two\n");
    up.commit_all("c2");
    up.git(&["config", "uploadpack.allowFilter", "true"]);
    let f = Fixture::bare_dir();
    let url = format!("file://{}", up.dir.display());
    let clone = f.dir.join("c");
    f.git_in(
        f.tmp.path(),
        &[
            "clone",
            "-q",
            "--no-checkout",
            "--filter=blob:none",
            &url,
            clone.to_str().unwrap(),
        ],
    );
    let packs = || listing(&clone.join(".git/objects"));
    let before = packs();
    let r = Repo::discover_with(&clone, f.options()).unwrap().unwrap();
    let t = Instant::now();
    let res = r.diff_commit("HEAD", None);
    assert!(res.is_err(), "missing blobs must not be fetched: {res:?}");
    assert!(t.elapsed() < Duration::from_secs(5));
    assert_eq!(packs(), before, "no objects were fetched");
}

/// Everything the parsers produce for a repo with renames, odd names and history.
fn snapshot(r: &Repo) -> String {
    let p = Path::new("dir/b c.txt");
    // git stamps uncommitted lines with the current time.
    let mut blame = r.blame(p).unwrap();
    for c in blame
        .iter_mut()
        .flat_map(|b| b.commits.iter_mut())
        .filter(|c| c.uncommitted)
    {
        c.author.time = 0;
    }
    format!(
        "{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}\n{:?}",
        r.head().unwrap(),
        r.status().unwrap(),
        r.diff_unstaged(p).unwrap(),
        r.diff_staged(Path::new("renamed.txt")).unwrap(),
        blame,
        r.log(10, Some(p)).unwrap(),
        r.log(10, None).unwrap(),
        r.commit("HEAD").unwrap(),
        r.diff_commit("HEAD", None).unwrap(),
    )
}

#[test]
fn user_and_repo_config_cannot_reshape_parsed_output() {
    let f = Fixture::new();
    let body: String = (1..=30).map(|i| format!("line {i}\n")).collect();
    f.write("dir/b c.txt", &body);
    f.write("caf\u{e9}.txt", "x\n");
    f.write("orig.txt", &body);
    f.commit_all("first");
    f.write(
        "dir/b c.txt",
        body.replace("line 3\n", "LINE 3\n")
            .replace("line 9\n", "LINE 9\n"),
    );
    f.commit_all("second\n\nbody");
    f.write("dir/b c.txt", body.replace("line 20\n", "x\n"));
    f.git(&["mv", "orig.txt", "renamed.txt"]);
    let clean = snapshot(&f.repo());
    let keys = [
        ("color.ui", "always"),
        ("color.diff", "always"),
        ("color.status", "always"),
        ("diff.noprefix", "true"),
        ("diff.mnemonicPrefix", "true"),
        ("diff.relative", "true"),
        ("diff.renames", "false"),
        ("diff.context", "10"),
        ("diff.interHunkContext", "10"),
        ("diff.suppressBlankEmpty", "true"),
        ("status.relativePaths", "true"),
        ("status.renames", "false"),
        ("status.showUntrackedFiles", "no"),
        ("core.quotePath", "true"),
        ("format.pretty", "oneline"),
        ("log.decorate", "full"),
        ("log.follow", "true"),
        ("log.showRoot", "false"),
        ("i18n.logOutputEncoding", "ISO-8859-1"),
        ("blame.showEmail", "true"),
        ("blame.blankBoundary", "true"),
    ];
    for (k, v) in keys {
        f.git(&["config", "--global", k, v]);
        f.git(&["config", k, v]);
    }
    let sub = f.path("dir");
    let r = Repo::discover_with(&sub, f.options()).unwrap().unwrap();
    assert_eq!(snapshot(&r), clean);
}

#[test]
fn filter_smudge_never_runs() {
    // The gutter's base is read from the index; a smudge driver must not run either way.
    check_key(
        "filter.x.smudge",
        Some("* filter=x\n"),
        &[&["checkout", "HEAD~1", "--", "a.txt"]],
    );
}

#[test]
fn submodule_local_filter_never_runs() {
    let sub = Fixture::new();
    sub.write(".gitattributes", "* filter=y\n");
    sub.write("f.txt", "one\n");
    sub.commit_all("s1");
    let f = Fixture::new();
    f.write("x.txt", "x\n");
    f.commit_all("t1");
    let url = sub.dir.to_str().unwrap();
    f.git(&[
        "-c",
        "protocol.file.allow=always",
        "submodule",
        "-q",
        "add",
        url,
        "sub",
    ]);
    f.commit_all("add sub");
    // The driver lives only in .git/modules/sub/config: invisible from the superproject.
    let (script, marker) = f.marker_script("sub_filter");
    f.git_in(
        &f.path("sub"),
        &["config", "filter.y.clean", script.to_str().unwrap()],
    );
    let dirty = || {
        f.write("sub/f.txt", "one\nchanged\n");
        // Stale stat info forces git to look at the content.
        let old = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        std::fs::File::options()
            .write(true)
            .open(f.path("sub/f.txt"))
            .unwrap()
            .set_modified(old)
            .unwrap();
    };
    dirty();
    plain(&f, &["status"]);
    assert!(
        marker.exists(),
        "control: plain git status runs the submodule's filter"
    );
    std::fs::remove_file(&marker).unwrap();
    let r = f.repo();
    for p in ["sub", "x.txt"] {
        dirty();
        read_everything(&r, p);
        assert!(!marker.exists(), "submodule filter ran during reads of {p}");
    }
    dirty();
    let _ = r.diff_unstaged(Path::new("sub"));
    assert!(!marker.exists(), "submodule filter ran during diff-files");
}

/// A partial clone whose promisor remote is an `ext::` command allowed by repo config.
fn hostile_partial_clone() -> (Fixture, Fixture, PathBuf, PathBuf) {
    let up = Fixture::new();
    up.write("a.txt", "one\n");
    up.commit_all("c1");
    up.write("a.txt", "two\n");
    up.commit_all("c2");
    up.git(&["config", "uploadpack.allowFilter", "true"]);
    let f = Fixture::bare_dir();
    let url = format!("file://{}", up.dir.display());
    let clone = f.dir.join("c");
    f.git_in(
        f.tmp.path(),
        &[
            "clone",
            "-q",
            "--no-checkout",
            "--filter=blob:none",
            &url,
            clone.to_str().unwrap(),
        ],
    );
    let marker = f.tmp.path().join("ext.ran");
    let ext = format!("ext::sh -c touch% {}% 1>&2", marker.display());
    f.git_in(&clone, &["config", "remote.origin.url", &ext]);
    f.git_in(&clone, &["config", "protocol.ext.allow", "always"]);
    (up, f, clone, marker)
}

#[test]
fn lazy_fetch_cannot_run_a_transport_command_even_without_no_lazy_fetch() {
    let (_up, f, clone, marker) = hostile_partial_clone();
    let _ = Command::new("git")
        .current_dir(&clone)
        .env_clear()
        .envs(f.env())
        .args(["diff-tree", "-p", "HEAD~1", "HEAD"])
        .output()
        .unwrap();
    assert!(marker.exists(), "control: plain git runs the ext:: remote");
    std::fs::remove_file(&marker).unwrap();
    // Second layer: a git without GIT_NO_LAZY_FETCH (older than 2.39.4 / unpatched 2.40-2.45).
    let dir = shim_dir(&f, "unset GIT_NO_LAZY_FETCH");
    let opts = [
        f.options(),
        Options {
            env: Some(env_with_path(&f, &dir)),
            ..Options::default()
        },
    ];
    for (i, opts) in opts.into_iter().enumerate() {
        let r = Repo::discover_with(&clone, opts).unwrap().unwrap();
        let _ = r.diff_commit("HEAD", None);
        let _ = r.diff_commit("HEAD", Some(Path::new("a.txt")));
        let _ = r.blame(Path::new("a.txt"));
        let _ = r.log(10, Some(Path::new("a.txt")));
        assert!(
            !marker.exists(),
            "read {i} ran the promisor remote's command"
        );
    }
}

#[test]
fn blame_of_a_staged_file_over_the_cap_is_too_large() {
    let f = Fixture::new();
    f.write("a.txt", "a\n");
    f.commit_all("c");
    f.write("new.txt", "x\n");
    f.git(&["add", "new.txt"]);
    f.write("new.txt", "x\n".repeat(10_000));
    let opts = Options {
        max_file: 4096,
        ..f.options()
    };
    let r = Repo::discover_with(&f.dir, opts).unwrap().unwrap();
    let res = r.blame(Path::new("new.txt"));
    assert!(matches!(res, Err(GitError::TooLarge { .. })), "{res:?}");
}

/// A git whose `status` starts a 30 s sleep and records its pid.
fn hanging_status(f: &Fixture) -> (Options, PathBuf) {
    let pidfile = f.tmp.path().join("sleep.pid");
    let body = format!(
        "case \"$*\" in *status*) sleep 30 & echo $! > '{}.tmp'; mv '{0}.tmp' '{0}'; wait;; esac",
        pidfile.display()
    );
    let dir = shim_dir(f, &body);
    let opts = Options {
        env: Some(env_with_path(f, &dir)),
        ..Options::default()
    };
    (opts, pidfile)
}

fn started_pid(pidfile: &Path) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Ok(s) = std::fs::read_to_string(pidfile) {
            return s.trim().parse().unwrap();
        }
        assert!(Instant::now() < deadline, "the hanging git never started");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn gone_soon(pid: u32) -> bool {
    (0..200).any(|_| {
        if proc_alive(pid) {
            std::thread::sleep(Duration::from_millis(10));
            false
        } else {
            true
        }
    })
}

#[test]
fn shutdown_kills_a_running_read_and_refuses_new_ones() {
    let f = Fixture::new();
    f.commit_all("c");
    let (opts, pidfile) = hanging_status(&f);
    let r = Repo::discover_with(&f.dir, opts).unwrap().unwrap();
    let r2 = r.clone();
    let reader = std::thread::spawn(move || r2.status());
    let pid = started_pid(&pidfile);
    let t = Instant::now();
    r.shutdown();
    let res = reader.join().unwrap();
    assert!(matches!(res, Err(GitError::ShutDown)), "{res:?}");
    assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    assert!(gone_soon(pid), "child sleep {pid} survived shutdown");
    assert!(matches!(r.head(), Err(GitError::ShutDown)));
}

#[test]
fn dropping_the_worker_kills_its_running_git() {
    let f = Fixture::new();
    f.commit_all("c");
    let (opts, pidfile) = hanging_status(&f);
    let r = Repo::discover_with(&f.dir, opts).unwrap().unwrap();
    let w = GitWorker::start(r, || {});
    w.request(Request::Status);
    let pid = started_pid(&pidfile);
    drop(w);
    assert!(gone_soon(pid), "child sleep {pid} survived the worker");
}

#[test]
fn inline_blame_uses_the_automatic_timeout() {
    let f = Fixture::new();
    f.write("a.txt", "one\n");
    f.commit_all("c");
    let dir = shim_dir(&f, "case \"$*\" in *blame*) sleep 5;; esac");
    let opts = Options {
        env: Some(env_with_path(&f, &dir)),
        timeout: Duration::from_millis(300),
        user_timeout: Duration::from_secs(30),
        ..Options::default()
    };
    let r = Repo::discover_with(&f.dir, opts).unwrap().unwrap();
    let t = Instant::now();
    let res = r.inline_blame(Path::new("a.txt"), "one\n");
    assert!(matches!(res, Err(GitError::Timeout(_))), "{res:?}");
    assert!(t.elapsed() < Duration::from_secs(3), "{:?}", t.elapsed());
}
