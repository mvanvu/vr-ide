//! Write API against real git (AC-16f): every action leaves the repo exactly as the same git
//! command does, compared on twin repos built by the same command sequence.

mod common;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use common::{Fixture, repo_state};
use vr_git::{Cancel, CommitOptions, GitError, Options, Repo, Writer};

/// Two repos in the same state (same content, same commit ids).
fn twins(setup: impl Fn(&Fixture)) -> (Fixture, Fixture) {
    let a = Fixture::new();
    let b = Fixture::new();
    setup(&a);
    setup(&b);
    assert_eq!(
        repo_state(&a),
        repo_state(&b),
        "twins differ before the action"
    );
    (a, b)
}

fn base(f: &Fixture) {
    f.write("a.txt", "one\ntwo\nthree\n");
    f.write("dir/b.txt", "bee\n");
    f.write("gone.txt", "bye\n");
    f.write("old name.txt", "renamed content\nline\n");
    f.commit_all("base");
    f.write("a.txt", "one\nTWO\nthree\n");
    f.write("new.txt", "fresh\n");
    f.write("dir/c.txt", "sea\n");
    std::fs::remove_file(f.path("gone.txt")).unwrap();
}

#[test]
fn stage_file_equals_git_add() {
    for (ours, theirs) in [
        ("a.txt", "a.txt"),
        ("new.txt", "new.txt"),
        ("dir/c.txt", "dir/c.txt"),
        ("gone.txt", "gone.txt"),
    ] {
        let (a, b) = twins(base);
        a.writer().stage_file(&a.path(ours)).unwrap();
        b.git(&["add", "--", theirs]);
        assert_eq!(repo_state(&a), repo_state(&b), "{ours}");
    }
}

#[test]
fn unstage_file_equals_git_restore_staged() {
    let staged = |f: &Fixture| {
        base(f);
        f.git(&["add", "-A"]);
        f.git(&["mv", "old name.txt", "new name.txt"]);
    };
    for path in ["a.txt", "new.txt", "gone.txt", "dir/c.txt"] {
        let (a, b) = twins(staged);
        a.writer().unstage_file(&a.path(path)).unwrap();
        b.git(&["restore", "--staged", "--", path]);
        assert_eq!(repo_state(&a), repo_state(&b), "{path}");
    }
    // A staged rename is undone on both paths, as git gui does.
    let (a, b) = twins(staged);
    a.writer().unstage_file(&a.path("new name.txt")).unwrap();
    b.git(&["restore", "--staged", "--", "new name.txt", "old name.txt"]);
    assert_eq!(repo_state(&a), repo_state(&b));
}

#[test]
fn unstage_on_an_unborn_branch_removes_the_entry() {
    let (a, b) = twins(|f| {
        f.write("x.txt", "x\n");
        f.write("y.txt", "y\n");
        f.git(&["add", "-A"]);
    });
    a.writer().unstage_file(&a.path("x.txt")).unwrap();
    b.git(&["rm", "--cached", "-q", "--", "x.txt"]);
    assert_eq!(repo_state(&a), repo_state(&b));
}

#[test]
fn hunk_stage_and_unstage_equal_git_apply_of_git_hunks() {
    let content = |f: &Fixture| {
        f.write(
            "f.txt",
            (1..=30).map(|i| format!("l{i}\n")).collect::<String>(),
        );
        f.commit_all("base");
        let mut text: Vec<String> = (1..=30).map(|i| format!("l{i}")).collect();
        text[1] = "L2".into();
        text[25] = "L26".into();
        text.insert(15, "inserted".into());
        f.write("f.txt", text.join("\n") + "\n");
    };
    let (a, b) = twins(content);
    let r = a.repo();
    let d = r.diff_unstaged(Path::new("f.txt")).unwrap().unwrap();
    assert_eq!(d.hunks.len(), 3);
    a.writer().stage_hunk(&d, 1).unwrap();
    // git add -p answering n, y, n.
    let (code, _, err) = b.git_raw(&["add", "-p", "--", "f.txt"], Some(b"n\ny\nn\n"));
    assert_eq!(code, 0, "{err}");
    assert_eq!(repo_state(&a), repo_state(&b));

    // And back: unstage the staged hunk like `git reset -p` would.
    let s = r.diff_staged(Path::new("f.txt")).unwrap().unwrap();
    a.writer().unstage_hunk(&s, 0).unwrap();
    let (code, _, err) = b.git_raw(&["reset", "-q", "-p", "--", "f.txt"], Some(b"y\n"));
    assert_eq!(code, 0, "{err}");
    assert_eq!(repo_state(&a), repo_state(&b));
}

#[test]
fn hunk_actions_are_refused_for_whole_file_changes() {
    let f = Fixture::new();
    f.write("bin", b"\0\x01binary\n");
    f.write("text", "a\nb\n");
    f.write("mode", "m\n");
    f.write("ren", "r1\nr2\nr3\nr4\n");
    std::os::unix::fs::symlink("text", f.path("link")).unwrap();
    f.commit_all("base");
    f.write("bin", b"\0\x02binary\n");
    std::fs::remove_file(f.path("link")).unwrap();
    std::os::unix::fs::symlink("mode", f.path("link")).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(f.path("mode"), std::fs::Permissions::from_mode(0o755)).unwrap();
    f.write("mode", "m\nM\n");
    f.write("untracked", "u\n");
    let r = f.repo();
    let w = f.writer();
    for p in ["bin", "link", "mode", "untracked"] {
        let d = r.diff_unstaged(Path::new(p)).unwrap().unwrap();
        let before = std::fs::read(f.dir.join(".git/index")).unwrap();
        let e = w.stage_hunk(&d, 0).unwrap_err();
        assert!(matches!(e, GitError::Refused(_)), "{p}: {e:?}");
        assert!(w.prepare_discard_hunk(&d, 0, None).is_err(), "{p}");
        assert_eq!(std::fs::read(f.dir.join(".git/index")).unwrap(), before);
    }
    f.git(&["mv", "ren", "ren2"]);
    f.write("ren2", "r1\nr2\nr3\nR4\n");
    f.git(&["add", "ren2"]);
    let s = r.diff_staged(Path::new("ren2")).unwrap().unwrap();
    assert!(matches!(w.unstage_hunk(&s, 0), Err(GitError::Refused(_))));
    // An attribute-driven filter makes the shown diff differ from git's: refused.
    f.write(".gitattributes", "text filter=x\n");
    f.write("text", "a\nB\n");
    let d = r.diff_unstaged(Path::new("text")).unwrap().unwrap();
    assert!(matches!(w.stage_hunk(&d, 0), Err(GitError::Refused(_))));
}

#[test]
fn stale_view_is_refused_and_the_index_is_unchanged() {
    let f = Fixture::new();
    f.write("f.txt", "a\nb\nc\n");
    f.commit_all("base");
    f.write("f.txt", "a\nB\nc\n");
    let r = f.repo();
    let d = r.diff_unstaged(Path::new("f.txt")).unwrap().unwrap();
    // Changed from outside after it was shown.
    f.write("f.txt", "a\nBB\nc\n");
    let before = std::fs::read(f.dir.join(".git/index")).unwrap();
    let e = f.writer().stage_hunk(&d, 0).unwrap_err();
    assert!(matches!(e, GitError::Stale(_)), "{e:?}");
    assert!(e.to_string().ends_with("changed, rescan"));
    assert_eq!(std::fs::read(f.dir.join(".git/index")).unwrap(), before);
    // Index changed behind the view (same worktree diff text is impossible then, but the
    // base blob check catches a staged change that leaves the hunk equal).
    f.write("f.txt", "a\nB\nc\n");
    let d = r.diff_unstaged(Path::new("f.txt")).unwrap().unwrap();
    f.write("other", "x\n");
    f.git(&["add", "other"]);
    f.writer().stage_hunk(&d, 0).unwrap();
}

#[test]
fn failing_check_leaves_the_index_byte_identical() {
    let f = Fixture::new();
    f.write("f.txt", "a\nb\nc\n");
    f.commit_all("base");
    f.write("f.txt", "a\nB\nc\n");
    // Selection of the added line only: valid. Then make git's check fail by a hook-free
    // trick: an index.lock held by another process makes both check and apply fail before
    // touching the index.
    let r = f.repo();
    let d = r.diff_unstaged(Path::new("f.txt")).unwrap().unwrap();
    let lock = f.dir.join(".git/index.lock");
    std::fs::write(&lock, "").unwrap();
    let before = std::fs::read(f.dir.join(".git/index")).unwrap();
    let e = f.writer().stage_lines(&d, 0, &[2]).unwrap_err();
    assert!(matches!(e, GitError::Failed { .. }), "{e:?}");
    assert!(e.to_string().contains("index.lock"), "{e}");
    assert!(lock.exists(), "vr-ide must never remove index.lock");
    assert_eq!(std::fs::read(f.dir.join(".git/index")).unwrap(), before);
    std::fs::remove_file(&lock).unwrap();
}

#[test]
fn apply_whitespace_config_does_not_change_staged_bytes() {
    let f = Fixture::new();
    f.git(&["config", "apply.whitespace", "fix"]);
    f.git(&["config", "apply.ignoreWhitespace", "change"]);
    f.write("f.txt", "a\nb\nc\n");
    f.commit_all("base");
    f.write("f.txt", "a\nb  \t\nc\n\n\n");
    let r = f.repo();
    let d = r.diff_unstaged(Path::new("f.txt")).unwrap().unwrap();
    f.writer().stage_hunk(&d, 0).unwrap();
    assert_eq!(f.index_bytes("f.txt"), b"a\nb  \t\nc\n\n\n");
}

#[test]
fn crlf_with_autocrlf_stages_lines_and_discards_hunks() {
    let f = Fixture::new();
    f.git(&["config", "core.autocrlf", "true"]);
    f.write("f.txt", "a\r\nb\r\nc\r\nd\r\n");
    f.commit_all("base");
    assert_eq!(f.index_bytes("f.txt"), b"a\nb\nc\nd\n");
    f.write("f.txt", "a\r\nB\r\nc\r\nD\r\n");
    let r = f.repo();
    let d = r.diff_unstaged(Path::new("f.txt")).unwrap().unwrap();
    // Hunk 0 holds both changes (one hunk with context); stage only "-b +B".
    let lines: Vec<usize> = d.hunks[0]
        .lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.text.starts_with(b"b") || l.text.starts_with(b"B"))
        .map(|(i, _)| i)
        .collect();
    let w = f.writer();
    w.stage_lines(&d, 0, &lines).unwrap();
    assert_eq!(f.index_bytes("f.txt"), b"a\nB\nc\nd\n");
    let d = r.diff_unstaged(Path::new("f.txt")).unwrap().unwrap();
    let plan = w.prepare_discard_hunk(&d, 0, None).unwrap();
    let done = w.discard(&plan, false).unwrap();
    // The worktree keeps its CRLF form, now equal to the index.
    assert_eq!(f.read("f.txt"), b"a\r\nB\r\nc\r\nd\r\n");
    assert_eq!(
        std::fs::read(done.recovery.unwrap()).unwrap(),
        b"a\r\nB\r\nc\r\nD\r\n"
    );
}

fn commit_env(f: &Fixture) -> Vec<(OsString, OsString)> {
    let mut env = f.env();
    for (k, v) in [
        ("GIT_AUTHOR_NAME", "Ann Author"),
        ("GIT_AUTHOR_EMAIL", "ann@example.com"),
        ("GIT_COMMITTER_NAME", "Cid Committer"),
        ("GIT_COMMITTER_EMAIL", "cid@example.com"),
        ("GIT_AUTHOR_DATE", "1800000000 +0000"),
        ("GIT_COMMITTER_DATE", "1800000000 +0000"),
    ] {
        env.push((k.into(), v.into()));
    }
    env
}

fn writer_with_env(f: &Fixture) -> Writer {
    let r = Repo::discover_with(
        &f.dir,
        Options {
            env: Some(commit_env(f)),
            ..Options::default()
        },
    )
    .unwrap()
    .unwrap();
    Writer::open(&r).unwrap()
}

fn git_record(f: &Fixture, extra: &[&str], msg: &str) {
    use std::io::Write;
    let mut args = vec!["commit", "--cleanup=whitespace", "-F", "-"];
    args.extend_from_slice(extra);
    let mut child = Command::new("git")
        .current_dir(&f.dir)
        .env_clear()
        .envs(commit_env(f))
        .args(&args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(msg.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
}

#[test]
fn record_equals_git_with_message_on_stdin() {
    let long = "x".repeat(64 << 10);
    let messages = [
        "--amend\n\nnot an option",
        "Subject # not a comment\n\n# kept line\nbody ✓ ünïcode",
        "  padded  \n\n\n\nbody\n\n",
        long.as_str(),
    ];
    for msg in messages {
        let (a, b) = twins(|f| {
            f.write("a.txt", "1\n");
            f.commit_all("base");
            f.write("a.txt", "2\n");
            f.git(&["add", "a.txt"]);
        });
        let w = writer_with_env(&a);
        let oid = w
            .commit(msg, CommitOptions::default(), &Cancel::new())
            .unwrap();
        git_record(&b, &[], msg);
        assert_eq!(repo_state(&a), repo_state(&b));
        assert_eq!(oid, b.head());
    }
}

#[test]
fn amend_and_signoff_equal_git() {
    let (a, b) = twins(|f| {
        f.write("a.txt", "1\n");
        f.commit_all("first message");
        f.write("a.txt", "2\n");
        f.git(&["add", "a.txt"]);
    });
    let w = writer_with_env(&a);
    let last = w.last_message().unwrap().unwrap();
    assert_eq!(last, "first message");
    let opts = CommitOptions {
        amend: true,
        signoff: true,
    };
    w.commit(&last, opts, &Cancel::new()).unwrap();
    git_record(&b, &["--amend", "--signoff"], &last);
    assert_eq!(repo_state(&a), repo_state(&b));
    assert!(
        a.git_s(&["log", "-1", "--format=%B"])
            .contains("Signed-off-by: Cid Committer <cid@example.com>")
    );
}

#[test]
fn record_refusals_and_git_messages() {
    let f = Fixture::new();
    let w = writer_with_env(&f);
    let c = Cancel::new();
    assert!(matches!(
        w.commit("  \n\t", CommitOptions::default(), &c),
        Err(GitError::Refused(_))
    ));
    assert_eq!(w.last_message().unwrap(), None);
    let amend = CommitOptions {
        amend: true,
        signoff: false,
    };
    assert!(matches!(
        w.commit("m", amend, &c),
        Err(GitError::Refused(_))
    ));
    f.write("a", "a\n");
    f.commit_all("c");
    // Nothing staged: git's own explanation comes back.
    let e = w.commit("m", CommitOptions::default(), &c).unwrap_err();
    match e {
        GitError::Failed { stderr, .. } => assert!(stderr.contains("nothing"), "{stderr}"),
        e => panic!("{e:?}"),
    }
}

#[test]
fn discard_file_keeps_a_private_recovery_copy() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    f.write("a.txt", "committed\n");
    f.commit_all("c");
    f.write("a.txt", "precious edit\n");
    let w = f.writer();
    let plan = w.prepare_discard_file(&f.path("a.txt")).unwrap();
    assert_eq!(plan.hunks, 1);
    // Unsaved buffer: refused, nothing touched.
    assert!(matches!(w.discard(&plan, true), Err(GitError::Refused(_))));
    assert_eq!(f.read("a.txt"), b"precious edit\n");
    let done = w.discard(&plan, false).unwrap();
    assert_eq!(f.read("a.txt"), b"committed\n");
    let copy = done.recovery.unwrap();
    assert!(copy.starts_with(f.recovery_dir()), "{copy:?}");
    assert_eq!(std::fs::read(&copy).unwrap(), b"precious edit\n");
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&copy), 0o600);
    assert_eq!(mode(&f.recovery_dir()), 0o700);
    let name = copy.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.ends_with("-repo-a.txt"), "{name}");
    // The repo state equals `git restore --worktree`.
    assert!(f.git(&["status", "--porcelain"]).is_empty());
}

#[test]
fn discard_is_refused_when_the_file_changed_after_confirmation() {
    let f = Fixture::new();
    f.write("a.txt", "a\nb\n");
    f.commit_all("c");
    f.write("a.txt", "a\nB\n");
    let w = f.writer();
    let plan = w.prepare_discard_file(&f.path("a.txt")).unwrap();
    f.write("a.txt", "a\nB\nmore typed after the dialog\n");
    let e = w.discard(&plan, false).unwrap_err();
    assert!(matches!(e, GitError::Stale(_)), "{e:?}");
    assert_eq!(f.read("a.txt"), b"a\nB\nmore typed after the dialog\n");
    let d = f.repo().diff_unstaged(Path::new("a.txt")).unwrap().unwrap();
    let plan = w.prepare_discard_hunk(&d, 0, None).unwrap();
    f.write("a.txt", "a\nB\nmore\n");
    assert!(w.discard(&plan, false).is_err());
    assert_eq!(f.read("a.txt"), b"a\nB\nmore\n");
}

#[test]
fn unwritable_recovery_folder_stops_the_discard() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    f.write("a.txt", "a\n");
    f.commit_all("c");
    f.write("a.txt", "edit\n");
    let dir = f.recovery_dir();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    let w = f.writer();
    let plan = w.prepare_discard_file(&f.path("a.txt")).unwrap();
    let r = w.discard(&plan, false);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    // Running as root would make the folder writable anyway.
    if rustix_is_root() {
        return;
    }
    assert!(r.is_err(), "{r:?}");
    assert_eq!(f.read("a.txt"), b"edit\n");
}

fn rustix_is_root() -> bool {
    std::fs::metadata("/proc/self")
        .map(|m| std::os::unix::fs::MetadataExt::uid(&m) == 0)
        .unwrap_or(false)
}

fn writer_with_recovery(
    f: &Fixture,
    env: Vec<(OsString, OsString)>,
    dir: Option<PathBuf>,
) -> Writer {
    let r = Repo::discover_with(
        &f.dir,
        Options {
            env: Some(env),
            recovery_dir: dir,
            ..Options::default()
        },
    )
    .unwrap()
    .unwrap();
    Writer::open(&r).unwrap()
}

/// `git status` of the repo at `dir`, untracked files included.
fn status_in(f: &Fixture, dir: &Path) -> String {
    String::from_utf8_lossy(&f.git_in(dir, &["status", "--porcelain", "--untracked-files=all"]))
        .into_owned()
}

/// D305(3): a recovery folder inside a work tree is used only once that repository ignores
/// it, through a `.gitignore` of `*` at its root.
#[test]
fn recovery_folder_inside_a_repo_is_ignored_there() {
    let f = Fixture::new();
    f.write("a.txt", "a\n");
    f.commit_all("c");
    f.write("a.txt", "edit\n");
    let dir = f.dir.join("recovery");
    let w = writer_with_recovery(&f, f.env(), Some(dir.clone()));
    let done = w
        .discard(&w.prepare_discard_file(&f.path("a.txt")).unwrap(), false)
        .unwrap();
    assert_eq!(f.read("a.txt"), b"a\n");
    assert_eq!(std::fs::read(done.recovery.unwrap()).unwrap(), b"edit\n");
    assert_eq!(std::fs::read(dir.join(".gitignore")).unwrap(), b"*\n");
    assert_eq!(status_in(&f, &f.dir), "");
}

/// `$HOME` is a dotfiles repository: the default recovery folder works and stays out of it.
#[test]
fn recovery_folder_in_a_dotfiles_home_works_and_stays_ignored() {
    let f = Fixture::new();
    f.write("a.txt", "a\n");
    f.commit_all("c");
    f.write("a.txt", "edit\n");
    f.git_in(&f.home, &["init", "-q", "-b", "main"]);
    std::fs::write(f.home.join(".bashrc"), "alias x=y\n").unwrap();
    f.git_in(&f.home, &["add", ".bashrc", ".gitconfig"]);
    let w = writer_with_recovery(&f, f.env(), None);
    let done = w
        .discard(&w.prepare_discard_file(&f.path("a.txt")).unwrap(), false)
        .unwrap();
    let copy = done.recovery.unwrap();
    assert!(copy.starts_with(f.recovery_dir()), "{copy:?}");
    assert_eq!(std::fs::read(&copy).unwrap(), b"edit\n");
    assert_eq!(f.read("a.txt"), b"a\n");
    let st = status_in(&f, &f.home);
    assert!(!st.contains(".local"), "{st}");
}

/// A recovery folder the enclosing repository tracks files in (or a repository git cannot
/// read) is refused with the way out, and nothing is created.
#[test]
fn recovery_folder_that_cannot_be_ignored_is_refused_with_a_hint() {
    let f = Fixture::new();
    f.write("a.txt", "a\n");
    f.commit_all("c");
    f.write("a.txt", "edit\n");

    // Tracked content inside the recovery folder.
    f.git_in(&f.home, &["init", "-q", "-b", "main"]);
    let tracked = f.recovery_dir().join("notes.txt");
    std::fs::create_dir_all(tracked.parent().unwrap()).unwrap();
    std::fs::write(&tracked, "mine\n").unwrap();
    f.git_in(&f.home, &["add", "-f", tracked.to_str().unwrap()]);
    let w = writer_with_recovery(&f, f.env(), None);
    let plan = w.prepare_discard_file(&f.path("a.txt")).unwrap();
    let e = w.discard(&plan, false).unwrap_err().to_string();
    assert!(
        e.contains("VR_IDE_STATE_DIR") && e.contains("recovery_dir"),
        "{e}"
    );
    assert!(!f.recovery_dir().join(".gitignore").exists());
    assert_eq!(f.read("a.txt"), b"edit\n");

    // A `.git` git cannot read: refused before any folder is made.
    std::fs::remove_dir_all(f.home.join(".git")).unwrap();
    std::fs::remove_dir_all(f.home.join(".local")).unwrap();
    std::fs::write(f.home.join(".git"), "gitdir: /nonexistent/vr-ide-test\n").unwrap();
    let e = w.discard(&plan, false).unwrap_err().to_string();
    assert!(
        e.contains("VR_IDE_STATE_DIR") && e.contains("recovery_dir"),
        "{e}"
    );
    assert!(
        !f.home.join(".local").exists(),
        "a folder was created before the check"
    );
    assert_eq!(f.read("a.txt"), b"edit\n");
}

/// D305(4): a program appending to the file while discards run loses no byte: every record
/// is in the worktree or in a recovery copy.
#[test]
fn discard_races_concurrent_writers_without_losing_a_byte() {
    discard_race(None, true);
}

/// The same race with the recovery folder on another filesystem (copy, fsync, verify).
#[test]
fn discard_races_concurrent_writers_across_filesystems() {
    let Ok(other) = tempfile::tempdir_in("/dev/shm") else {
        eprintln!("skipped: no /dev/shm");
        return;
    };
    let probe = Fixture::bare_dir();
    use std::os::unix::fs::MetadataExt;
    if std::fs::metadata(other.path()).unwrap().dev()
        == std::fs::metadata(&probe.dir).unwrap().dev()
    {
        eprintln!("skipped: /dev/shm is on the same filesystem");
        return;
    }
    // Across filesystems the moved file is removed once its copy is verified, so a program
    // that keeps the file open could still write into the removed one: only open-by-name
    // writers are raced here (documented on `Writer::discard`).
    discard_race(Some(other.path().join("git-recovery")), false);
}

fn discard_race(dir: Option<PathBuf>, held_writer: bool) {
    use std::io::Write;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    let f = Fixture::new();
    f.write("log.txt", "base\n");
    f.write("keep", "k\n");
    f.commit_all("c");
    let recovery = dir.clone().unwrap_or_else(|| f.recovery_dir());
    let w = writer_with_recovery(&f, f.env(), dir);
    let stop = Arc::new(AtomicBool::new(false));
    let path = f.path("log.txt");
    let mut writers = Vec::new();
    let ids: &[u32] = if held_writer { &[0, 1] } else { &[1] };
    for &id in ids {
        let (stop, path) = (stop.clone(), path.clone());
        writers.push(std::thread::spawn(move || {
            let open = || {
                std::fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(&path)
                    .unwrap()
            };
            // Writer 0 keeps its descriptor (a logger); writer 1 opens by name each time.
            let mut held = open();
            let mut n = 0u32;
            while !stop.load(Ordering::Relaxed) {
                let rec = format!("w{id}-{n:07}\n");
                if id == 0 {
                    held.write_all(rec.as_bytes()).unwrap();
                } else {
                    open().write_all(rec.as_bytes()).unwrap();
                }
                n += 1;
                // Bursts with quiet gaps of 0-40 ms: some discards pass their check, and some
                // writes land between that check and the move.
                if n.is_multiple_of(4) {
                    let gap = (u64::from(n).wrapping_mul(0x9e37_79b9) >> 7) % 40;
                    std::thread::sleep(std::time::Duration::from_millis(gap));
                }
            }
            (id, n)
        }));
    }
    let (mut done, mut stale, mut recreated) = (0, 0, 0);
    let t = std::time::Instant::now();
    while done < 25 && t.elapsed() < std::time::Duration::from_secs(60) {
        let Ok(plan) = w.prepare_discard_file(&path) else {
            continue;
        };
        match w.discard(&plan, false) {
            Ok(_) => done += 1,
            Err(GitError::Stale(_)) => stale += 1,
            Err(e) => {
                assert!(e.to_string().contains("created again"), "{e}");
                recreated += 1;
            }
        }
    }
    eprintln!("{done} discards, {stale} stale, {recreated} with the file created again");
    stop.store(true, Ordering::Relaxed);
    let counts: Vec<(u32, u32)> = writers.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(done >= 5, "only {done} discards ran");

    // Every file left anywhere: the worktree (but .git) and the recovery folder.
    fn collect(p: &Path, out: &mut Vec<u8>) {
        for e in std::fs::read_dir(p).unwrap() {
            let e = e.unwrap();
            let t = e.file_type().unwrap();
            if e.file_name() == ".git" {
                continue;
            }
            if t.is_dir() {
                collect(&e.path(), out);
            } else if t.is_file() {
                out.extend(std::fs::read(e.path()).unwrap());
            }
        }
    }
    let mut all = Vec::new();
    collect(&f.dir, &mut all);
    collect(&recovery, &mut all);
    let all = String::from_utf8_lossy(&all);
    let found: std::collections::HashSet<&str> = all.lines().collect();
    for &(id, n) in &counts {
        for i in 0..n {
            let rec = format!("w{id}-{i:07}");
            assert!(found.contains(rec.as_str()), "{rec} lost ({done} discards)");
        }
    }
}

#[test]
fn discard_untracked_file_folder_and_symlink() {
    let f = Fixture::new();
    f.write("keep", "k\n");
    f.commit_all("c");
    f.write("u.txt", "untracked\n");
    f.write("udir/x", "x\n");
    f.write("udir/sub/y", "y\n");
    std::os::unix::fs::symlink("/etc/passwd", f.path("evil")).unwrap();
    let passwd = std::fs::read("/etc/passwd").unwrap();
    let w = f.writer();

    let done = w
        .discard(&w.prepare_discard_file(&f.path("u.txt")).unwrap(), false)
        .unwrap();
    assert!(!f.path("u.txt").exists());
    assert_eq!(
        std::fs::read(done.recovery.unwrap()).unwrap(),
        b"untracked\n"
    );

    let done = w
        .discard(&w.prepare_discard_file(&f.path("udir")).unwrap(), false)
        .unwrap();
    assert!(!f.path("udir").exists());
    let copy = done.recovery.unwrap();
    assert_eq!(std::fs::read(copy.join("x")).unwrap(), b"x\n");
    assert_eq!(std::fs::read(copy.join("sub/y")).unwrap(), b"y\n");

    let done = w
        .discard(&w.prepare_discard_file(&f.path("evil")).unwrap(), false)
        .unwrap();
    assert!(
        std::fs::symlink_metadata(f.path("evil")).is_err(),
        "link removed"
    );
    assert_eq!(
        std::fs::read("/etc/passwd").unwrap(),
        passwd,
        "target untouched"
    );
    let copy = done.recovery.unwrap();
    assert!(
        std::fs::symlink_metadata(&copy)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read_link(&copy).unwrap(), Path::new("/etc/passwd"));

    // A folder with tracked files is not an untracked folder.
    f.write("tracked/t", "t\n");
    f.commit_all("t");
    assert!(w.prepare_discard_file(&f.path("tracked")).is_err());
}

#[test]
fn discard_of_a_deleted_file_brings_it_back() {
    let f = Fixture::new();
    f.write("a.txt", "a\n");
    f.commit_all("c");
    std::fs::remove_file(f.path("a.txt")).unwrap();
    let w = f.writer();
    let done = w
        .discard(&w.prepare_discard_file(&f.path("a.txt")).unwrap(), false)
        .unwrap();
    assert_eq!(done.recovery, None);
    assert_eq!(f.read("a.txt"), b"a\n");
}

#[test]
fn state_reports_in_progress_operations() {
    let f = Fixture::new();
    f.write("a.txt", "base\n");
    f.commit_all("base");
    f.git(&["switch", "-q", "-c", "side"]);
    f.write("a.txt", "side\n");
    f.commit_all("side");
    f.git(&["switch", "-q", "main"]);
    f.write("a.txt", "main\n");
    f.commit_all("main");
    let (code, _, _) = f.git_raw(&["merge", "side"], None);
    assert_ne!(code, 0);
    let w = f.writer();
    let st = w.state().unwrap();
    assert_eq!(st.to_string(), "merge, 1 conflict");
    assert!(matches!(
        w.stage_file(&f.path("a.txt")),
        Err(GitError::Refused(_))
    ));
    assert!(matches!(
        w.prepare_discard_file(&f.path("a.txt")),
        Err(GitError::Refused(_))
    ));
    assert!(matches!(
        w.commit("m", CommitOptions::default(), &Cancel::new()),
        Err(GitError::Refused(_))
    ));
    f.git(&["merge", "--abort"]);
    assert!(w.state().unwrap().is_clean());

    // A stopped rebase blocks records.
    f.git(&["switch", "-q", "side"]);
    let (code, _, _) = f.git_raw(&["rebase", "main"], None);
    assert_ne!(code, 0);
    let st = w.state().unwrap();
    assert_eq!(st.operation, Some(vr_git::Operation::Rebase));
    f.write("a.txt", "resolved\n");
    f.git(&["add", "a.txt"]);
    let e = w
        .commit("m", CommitOptions::default(), &Cancel::new())
        .unwrap_err();
    assert!(e.to_string().contains("rebase"), "{e}");
}

/// A tmpfs folder on another filesystem than the fixtures, or `None` (the test is skipped).
fn other_filesystem() -> Option<tempfile::TempDir> {
    use std::os::unix::fs::MetadataExt;
    let other = tempfile::tempdir_in("/dev/shm").ok()?;
    let probe = Fixture::bare_dir();
    if std::fs::metadata(other.path()).unwrap().dev()
        == std::fs::metadata(&probe.dir).unwrap().dev()
    {
        return None;
    }
    Some(other)
}

/// Review round 1, probe 5a: across filesystems an untracked folder is removed file by file
/// after its copy, so a read-only subfolder must stop the discard before anything moves.
#[test]
fn cross_filesystem_discard_of_a_folder_it_cannot_remove_is_refused_up_front() {
    use std::os::unix::fs::PermissionsExt;
    let Some(other) = other_filesystem() else {
        eprintln!("skipped: no /dev/shm on another filesystem");
        return;
    };
    if rustix_is_root() {
        return;
    }
    let f = Fixture::new();
    f.write("f", "a\n");
    f.commit_all("c");
    f.write("u/a.txt", "A\n");
    f.write("u/ro/b.txt", "B\n");
    f.write("u/z.txt", "Z\n");
    let ro = f.path("u/ro");
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
    let rec = other.path().join("git-recovery");
    let w = writer_with_recovery(&f, f.env(), Some(rec.clone()));
    let plan = w.prepare_discard_file(&f.path("u")).unwrap();
    let r = w.discard(&plan, false);
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
    let e = r.expect_err("a folder that cannot be removed must be refused");
    assert!(matches!(e, GitError::Refused(_)), "{e:?}");
    let text = e.to_string();
    assert!(text.contains("u/ro"), "{text}");
    assert!(text.contains("nothing was discarded"), "{text}");
    assert_eq!(f.read("u/a.txt"), b"A\n");
    assert_eq!(f.read("u/ro/b.txt"), b"B\n");
    assert_eq!(f.read("u/z.txt"), b"Z\n");
    let copies: Vec<_> = std::fs::read_dir(&rec)
        .map(|d| d.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(
        copies.iter().all(|n| n == ".gitignore"),
        "no copy may be left: {copies:?}"
    );
    assert!(vr_git::leftover_discards(&f.repo()).unwrap().is_empty());
}

/// Hidden `.vr-ide-discard-*` names a killed discard leaves in the work tree are found, also
/// in subfolders and ignored folders; a folder is reported once, not file by file.
#[test]
fn leftover_discards_are_listed() {
    let f = Fixture::new();
    f.write(".gitignore", "build/\n");
    f.write("a.txt", "a\n");
    f.commit_all("c");
    f.write(".vr-ide-discard-1-0-5.old", "earlier\n");
    f.write("src/.vr-ide-discard-1-1-5.new", "new\n");
    f.write("build/.vr-ide-discard-1-2-5.old/x/y.txt", "dir\n");
    f.write("src/other.txt", "o\n");
    let mut found = vr_git::leftover_discards(&f.repo()).unwrap();
    found.sort_by(|a, b| a.path.cmp(&b.path));
    let got: Vec<(String, bool)> = found
        .iter()
        .map(|l| (l.path.to_string_lossy().into_owned(), l.earlier_content))
        .collect();
    assert_eq!(
        got,
        vec![
            (".vr-ide-discard-1-0-5.old".to_string(), true),
            ("build/.vr-ide-discard-1-2-5.old".to_string(), true),
            ("src/.vr-ide-discard-1-1-5.new".to_string(), false),
        ]
    );
}

/// Paths in Refused / Stale / Io texts come from the repository; their display must not
/// carry terminal control sequences (review round 1, minor 3).
#[test]
fn discard_error_text_hides_control_characters_of_paths() {
    let f = Fixture::new();
    f.write("a.txt", "a\n");
    f.commit_all("c");
    let name = "x\u{1b}]52;c;aGk=\u{7}.txt";
    f.write(name, "one\n");
    let w = f.writer();
    let plan = w.prepare_discard_file(&f.path(name)).unwrap();
    f.write(name, "two\n");
    let e = w.discard(&plan, false).unwrap_err();
    assert!(matches!(e, GitError::Stale(_)), "{e:?}");
    let text = e.to_string();
    assert!(
        !text.contains('\u{1b}') && !text.contains('\u{7}'),
        "{text:?}"
    );
    assert!(text.contains("x^[]52;c;aGk=^G.txt"), "{text:?}");
    let refused = GitError::Refused("bad \u{1b}[2J name".into()).to_string();
    assert_eq!(refused, "bad ^[[2J name");
    let io = GitError::Io("open \u{9b}x".into()).to_string();
    assert!(!io.contains('\u{9b}'), "{io:?}");
}
