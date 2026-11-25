//! Read API against real git repos, compared with git's own porcelain output.

mod common;

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use common::Fixture;
use vr_git::{ChangeKind, FileStatus, Head, LineChange, LineKind, Repo};

#[test]
fn no_repo_bare_and_git_dir_discover_none() {
    let f = Fixture::bare_dir();
    assert!(Repo::discover_with(&f.dir, f.options()).unwrap().is_none());
    f.git(&["init", "-q", "--bare", "b.git"]);
    assert!(
        Repo::discover_with(&f.dir.join("b.git"), f.options())
            .unwrap()
            .is_none()
    );
    let g = Fixture::new();
    assert!(
        Repo::discover_with(&g.dir.join(".git"), g.options())
            .unwrap()
            .is_none()
    );
}

#[test]
fn discover_from_a_file_in_a_subdirectory() {
    let f = Fixture::new();
    f.write("src/a.rs", "x\n");
    let r = Repo::discover_with(&f.path("src/a.rs"), f.options())
        .unwrap()
        .unwrap();
    assert_eq!(r.workdir(), f.dir);
    assert_eq!(
        r.relative(&f.path("src/a.rs")).unwrap(),
        PathBuf::from("src/a.rs")
    );
    assert!(r.relative(Path::new("/etc/passwd")).is_err());
    assert!(r.relative(Path::new("../x")).is_err());
}

#[test]
fn parent_repo_is_reported() {
    let f = Fixture::new();
    f.write("sub/x.txt", "x\n");
    let r = Repo::discover_with(&f.path("sub"), f.options())
        .unwrap()
        .unwrap();
    assert!(r.is_parent_repo_of(&f.path("sub")));
    assert!(!r.is_parent_repo_of(&f.dir));
}

#[test]
fn unborn_repo_head_status_log_blame() {
    let f = Fixture::new();
    let r = f.repo();
    assert_eq!(
        r.head().unwrap(),
        Head::Branch {
            name: "main".into(),
            oid: None
        }
    );
    assert_eq!(r.ahead_behind().unwrap(), None);
    assert!(r.log(10, None).unwrap().is_empty());
    f.write("a.txt", "one\ntwo\n");
    assert!(
        r.blame(Path::new("a.txt")).unwrap().is_none(),
        "untracked → nothing"
    );
    f.git(&["add", "a.txt"]);
    let st = r.status().unwrap();
    assert_eq!(st.staged.len(), 1);
    assert_eq!(st.staged[0].status, FileStatus::Added);
    let b = r.blame(Path::new("a.txt")).unwrap().unwrap();
    assert_eq!(b.lines.len(), 2);
    assert!(b.commit_of(0).unwrap().uncommitted);
    let d = r.diff_staged(Path::new("a.txt")).unwrap().unwrap();
    assert_eq!(d.status, FileStatus::Added);
    assert_eq!(d.hunks[0].lines.len(), 2);
}

#[test]
fn head_branch_and_detached() {
    let f = Fixture::new();
    f.write("a", "1\n");
    let c1 = f.commit_all("one");
    f.write("a", "2\n");
    let c2 = f.commit_all("two");
    let r = f.repo();
    assert_eq!(
        r.head().unwrap(),
        Head::Branch {
            name: "main".into(),
            oid: Some(c2.clone())
        }
    );
    f.git(&["switch", "-q", "--detach", &c1]);
    let short = f
        .git_s(&["rev-parse", "--short", "HEAD"])
        .trim()
        .to_string();
    assert_eq!(r.head().unwrap(), Head::Detached { oid: c1, short });
    assert_eq!(r.ahead_behind().unwrap(), None);
}

#[test]
fn ahead_behind_matches_status_sb() {
    let up = Fixture::new();
    up.write("a", "1\n");
    up.commit_all("base");
    let f = Fixture::bare_dir();
    f.git_in(
        f.tmp.path(),
        &[
            OsStr::new("clone"),
            OsStr::new("-q"),
            up.dir.as_os_str(),
            f.dir.join("c").as_os_str(),
        ],
    );
    let clone = f.dir.join("c");
    let r = Repo::discover_with(&clone, f.options()).unwrap().unwrap();
    assert_eq!(r.ahead_behind().unwrap().unwrap().ahead, 0);
    std::fs::write(clone.join("b"), "x\n").unwrap();
    f.git_in(&clone, &["add", "b"]);
    f.git_in(&clone, &["commit", "-q", "-m", "local1"]);
    std::fs::write(clone.join("b"), "y\n").unwrap();
    f.git_in(&clone, &["commit", "-q", "-am", "local2"]);
    up.write("a", "2\n");
    up.commit_all("remote");
    f.git_in(&clone, &["fetch", "-q"]);
    let ab = r.ahead_behind().unwrap().unwrap();
    assert_eq!(
        (ab.upstream.as_str(), ab.ahead, ab.behind),
        ("origin/main", 2, 1)
    );
    let sb = String::from_utf8(f.git_in(&clone, &["status", "-sb"])).unwrap();
    assert!(
        sb.starts_with("## main...origin/main [ahead 2, behind 1]"),
        "{sb}"
    );
}

#[test]
fn status_lists_match_porcelain() {
    let f = Fixture::new();
    f.write("mod.txt", "a\n");
    f.write("del.txt", "a\n");
    f.write("old name.txt", "same content here\nline two\nline three\n");
    f.write("both.txt", "a\n");
    f.commit_all("base");
    f.write("mod.txt", "b\n");
    std::fs::remove_file(f.path("del.txt")).unwrap();
    f.git(&["mv", "old name.txt", "new name.txt"]);
    f.write("both.txt", "staged\n");
    f.git(&["add", "both.txt"]);
    f.write("both.txt", "worktree\n");
    f.write("new/untracked.txt", "u\n");
    f.write("added.txt", "x\n");
    f.git(&["add", "added.txt"]);
    let st = f.repo().status().unwrap();
    let names = |v: &[vr_git::StatusEntry]| -> Vec<(String, FileStatus)> {
        v.iter()
            .map(|e| (e.path.to_string_lossy().into_owned(), e.status))
            .collect()
    };
    let mut staged = names(&st.staged);
    staged.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        staged,
        vec![
            ("added.txt".into(), FileStatus::Added),
            ("both.txt".into(), FileStatus::Modified),
            ("new name.txt".into(), FileStatus::Renamed),
        ]
    );
    let ren = st
        .staged
        .iter()
        .find(|e| e.status == FileStatus::Renamed)
        .unwrap();
    assert_eq!(ren.orig_path.as_deref(), Some(Path::new("old name.txt")));
    let mut unstaged = names(&st.unstaged);
    unstaged.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        unstaged,
        vec![
            ("both.txt".into(), FileStatus::Modified),
            ("del.txt".into(), FileStatus::Deleted),
            ("mod.txt".into(), FileStatus::Modified),
            ("new/untracked.txt".into(), FileStatus::Untracked),
        ]
    );
    // Same set of paths as git's short format.
    let short = f.git_s(&["status", "--porcelain=v1", "-uall"]);
    assert_eq!(short.lines().count(), 6, "{short}"); // "MM both.txt" is one line
}

#[test]
fn conflicted_file_is_listed_and_has_no_gutter() {
    let f = Fixture::new();
    f.write("c.txt", "base\n");
    f.commit_all("base");
    f.git(&["switch", "-q", "-c", "other"]);
    f.write("c.txt", "theirs\n");
    f.commit_all("theirs");
    f.git(&["switch", "-q", "main"]);
    f.write("c.txt", "ours\n");
    f.commit_all("ours");
    let out = std::process::Command::new("git")
        .current_dir(&f.dir)
        .env_clear()
        .envs(f.env())
        .args([
            "-c",
            "user.name=a",
            "-c",
            "user.email=a@x",
            "merge",
            "-q",
            "other",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let r = f.repo();
    let st = r.status().unwrap();
    assert_eq!(
        st.unstaged
            .iter()
            .find(|e| e.path == Path::new("c.txt"))
            .unwrap()
            .status,
        FileStatus::Conflicted
    );
    assert_eq!(r.line_changes(Path::new("c.txt"), "x\n").unwrap(), None);
    assert_eq!(
        r.diff_unstaged(Path::new("c.txt")).unwrap().unwrap().status,
        FileStatus::Conflicted
    );
}

/// Parse `git diff -U0` hunk headers into gutter marks, the reference for AC-16a.
fn marks_from_git(f: &Fixture, rel: &str) -> Vec<LineChange> {
    let out = f.git_s(&["diff", "-U0", "--no-color", "--", rel]);
    out.lines()
        .filter_map(|l| l.strip_prefix("@@ -"))
        .map(|l| {
            let mut it = l.split(' ');
            let old = it.next().unwrap();
            let new = it.next().unwrap().trim_start_matches('+');
            let len = |r: &str| {
                r.split_once(',')
                    .map_or(1, |(_, n)| n.parse::<u32>().unwrap())
            };
            let start = |r: &str| r.split(',').next().unwrap().parse::<u32>().unwrap();
            let (ol, nl, ns) = (len(old), len(new), start(new));
            match (ol, nl) {
                (0, _) => LineChange {
                    kind: ChangeKind::Added,
                    start: ns - 1,
                    len: nl,
                },
                (_, 0) => LineChange {
                    kind: ChangeKind::Deleted,
                    start: ns,
                    len: ol,
                },
                _ => LineChange {
                    kind: ChangeKind::Modified,
                    start: ns - 1,
                    len: nl,
                },
            }
        })
        .collect()
}

#[test]
fn gutter_marks_match_git_diff_for_unsaved_buffers() {
    let f = Fixture::new();
    let base: String = (1..=40).map(|i| format!("line {i}\n")).collect();
    f.write("g.txt", &base);
    f.commit_all("base");
    let r = f.repo();
    let mut lines: Vec<String> = base.lines().map(str::to_string).collect();
    lines[2] = "changed 3".into();
    lines.insert(10, "inserted a".into());
    lines.insert(11, "inserted b".into());
    lines.drain(20..23);
    lines[30] = "changed late".into();
    lines.push("tail".into());
    let buffer = lines.join("\n") + "\n";
    let ours = r
        .line_changes(Path::new("g.txt"), &buffer)
        .unwrap()
        .unwrap();
    // The buffer is unsaved: the worktree file is still the committed one.
    assert_eq!(f.git_s(&["status", "--porcelain"]), "");
    f.write("g.txt", &buffer);
    assert_eq!(ours, marks_from_git(&f, "g.txt"));
    assert_eq!(
        ours.iter().map(|c| c.kind).collect::<Vec<_>>(),
        vec![
            ChangeKind::Modified,
            ChangeKind::Added,
            ChangeKind::Deleted,
            ChangeKind::Modified,
            ChangeKind::Added
        ]
    );
    // Untracked → nothing to show.
    f.write("u.txt", "x\n");
    assert_eq!(r.line_changes(Path::new("u.txt"), "x\n").unwrap(), None);
}

#[test]
fn gutter_is_against_the_index_not_head() {
    let f = Fixture::new();
    f.write("a.txt", "1\n2\n3\n");
    f.commit_all("base");
    f.write("a.txt", "1\nTWO\n3\n");
    f.git(&["add", "a.txt"]);
    let r = f.repo();
    assert_eq!(
        r.line_changes(Path::new("a.txt"), "1\nTWO\n3\n")
            .unwrap()
            .unwrap(),
        vec![]
    );
}

#[test]
fn diffs_carry_line_numbers_and_match_git() {
    let f = Fixture::new();
    let base: String = (1..=20).map(|i| format!("l{i}\n")).collect();
    f.write("d.txt", &base);
    f.commit_all("base");
    f.write(
        "d.txt",
        base.replace("l5\n", "L5\n").replace("l15\n", "l15\nnew\n"),
    );
    let r = f.repo();
    let d = r.diff_unstaged(Path::new("d.txt")).unwrap().unwrap();
    assert_eq!(d.hunks.len(), 2);
    let h = &d.hunks[0];
    assert_eq!(
        (h.old_start, h.old_len, h.new_start, h.new_len),
        (2, 7, 2, 7)
    );
    let removed = h
        .lines
        .iter()
        .find(|l| l.kind == LineKind::Removed)
        .unwrap();
    assert_eq!(
        (removed.old_line, removed.text_lossy().as_str()),
        (Some(5), "l5")
    );
    let added = d.hunks[1]
        .lines
        .iter()
        .find(|l| l.kind == LineKind::Added)
        .unwrap();
    assert_eq!(
        (added.new_line, added.text_lossy().as_str()),
        (Some(16), "new")
    );
    let git_headers: Vec<String> = f
        .git_s(&["diff", "--", "d.txt"])
        .lines()
        .filter(|l| l.starts_with("@@"))
        .map(str::to_string)
        .collect();
    assert_eq!(
        d.hunks.iter().map(|h| h.header.clone()).collect::<Vec<_>>(),
        git_headers
    );
    assert_eq!(d.old_oid.as_deref().map(str::len), Some(40));
    assert!(r.diff_staged(Path::new("d.txt")).unwrap().is_none());
    f.git(&["add", "d.txt"]);
    assert!(r.diff_unstaged(Path::new("d.txt")).unwrap().is_none());
    assert_eq!(
        r.diff_staged(Path::new("d.txt"))
            .unwrap()
            .unwrap()
            .hunks
            .len(),
        2
    );
}

#[test]
fn untracked_diff_is_all_added_and_no_newline_kept() {
    let f = Fixture::new();
    f.commit_all("empty");
    f.write("n.txt", "a\nb");
    let d = f.repo().diff_unstaged(Path::new("n.txt")).unwrap().unwrap();
    assert_eq!(d.status, FileStatus::Untracked);
    let l = &d.hunks[0].lines;
    assert_eq!(l.len(), 2);
    assert!(l[1].no_newline);
    assert!(l.iter().all(|l| l.kind == LineKind::Added));
}

#[test]
fn staged_rename_and_commit_diff_detect_renames() {
    let f = Fixture::new();
    let body: String = (1..=10).map(|i| format!("row {i}\n")).collect();
    f.write("a.txt", &body);
    f.commit_all("base");
    f.git(&["mv", "a.txt", "b.txt"]);
    f.write("b.txt", body.replace("row 3", "ROW 3"));
    f.git(&["add", "b.txt"]);
    let r = f.repo();
    let d = r.diff_staged(Path::new("b.txt")).unwrap().unwrap();
    assert_eq!(d.status, FileStatus::Renamed);
    assert_eq!(d.old_path.as_deref(), Some(Path::new("a.txt")));
    let sha = f.commit_all("rename");
    let all = r.diff_commit(&sha, None).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].status, FileStatus::Renamed);
    let one = r.diff_commit(&sha, Some(Path::new("b.txt"))).unwrap();
    assert_eq!(one, all);
    // Root commit diffs against the empty tree.
    let root = f
        .git_s(&["rev-list", "--max-parents=0", "HEAD"])
        .trim()
        .to_string();
    let rd = r.diff_commit(&root, None).unwrap();
    assert_eq!(rd[0].status, FileStatus::Added);
}

#[test]
fn log_follows_renames_and_matches_git() {
    let f = Fixture::new();
    f.write("old.txt", "a\nb\nc\nd\n");
    f.commit_all("one");
    f.write("other.txt", "x\n");
    f.commit_all("unrelated");
    f.git(&["mv", "old.txt", "new.txt"]);
    f.commit_all("rename");
    f.write("new.txt", "a\nb\nc\nd\ne\n");
    f.commit_all("edit\n\nbody \x1e with separator");
    let r = f.repo();
    let log = r.log(100, Some(Path::new("new.txt"))).unwrap();
    let git: Vec<String> = f
        .git_s(&["log", "--follow", "--format=%H", "--", "new.txt"])
        .lines()
        .map(str::to_string)
        .collect();
    assert_eq!(log.iter().map(|c| c.oid.clone()).collect::<Vec<_>>(), git);
    assert_eq!(
        log.iter().map(|c| c.summary.as_str()).collect::<Vec<_>>(),
        ["edit", "rename", "one"]
    );
    assert_eq!(log[2].path.as_deref(), Some(Path::new("old.txt")));
    assert_eq!(log[0].path.as_deref(), Some(Path::new("new.txt")));
    assert_eq!(log[0].author.name, "Ann Author");
    let all = r.log(2, None).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[1].parents.len(), 1);
    let info = r.commit(&log[0].oid).unwrap();
    assert_eq!(info.message, "edit\n\nbody \x1e with separator");
    assert_eq!(info.summary(), "edit");
    let git_show = f.git_s(&["show", "-s", "--format=%an|%ae|%at|%cn", &log[0].oid]);
    assert_eq!(
        git_show.trim(),
        format!(
            "{}|{}|{}|{}",
            info.author.name, info.author.email, info.author.time, info.committer.name
        )
    );
    assert_eq!(r.commit("HEAD").unwrap().oid, log[0].oid);
}

#[test]
fn blame_matches_porcelain_and_marks_uncommitted_lines() {
    let f = Fixture::new();
    f.write("b.txt", "one\ntwo\nthree\n");
    let c1 = f.commit_all("first");
    f.write("b.txt", "one\nTWO\nthree\n");
    let c2 = f.commit_all("second");
    let r = f.repo();
    let b = r.blame(Path::new("b.txt")).unwrap().unwrap();
    let oids: Vec<&str> = (0..3)
        .map(|i| b.commit_of(i).unwrap().oid.as_str())
        .collect();
    assert_eq!(oids, [c1.as_str(), c2.as_str(), c1.as_str()]);
    let git: Vec<String> = f
        .git_s(&["blame", "--line-porcelain", "--", "b.txt"])
        .lines()
        .filter(|l| l.len() > 40 && l.as_bytes()[..40].iter().all(u8::is_ascii_hexdigit))
        .map(|l| l[..40].to_string())
        .collect();
    assert_eq!(oids, git.iter().map(String::as_str).collect::<Vec<_>>());
    assert_eq!(b.commit_of(1).unwrap().summary, "second");
    assert!(b.commit_of(0).unwrap().boundary || !b.commit_of(0).unwrap().uncommitted);

    let bt = r
        .blame_text(Path::new("b.txt"), "one\nTWO\nedited\nnew line\n")
        .unwrap()
        .unwrap();
    assert_eq!(bt.lines.len(), 4);
    let unc: Vec<bool> = (0..4)
        .map(|i| bt.commit_of(i).unwrap().uncommitted)
        .collect();
    assert_eq!(unc, [false, false, true, true]);
    // The saved file is untouched by blame_text.
    assert_eq!(
        std::fs::read_to_string(f.path("b.txt")).unwrap(),
        "one\nTWO\nthree\n"
    );
}

#[test]
fn odd_paths_round_trip() {
    let f = Fixture::new();
    let names: Vec<&[u8]> = vec![
        b"with space.txt",
        b"-rf",
        b"--output=x",
        b":(glob)*",
        b"*",
        b":!a",
        b"a\nb",
        b"a\tb",
        b"\"q\"",
        b"caf\xc3\xa9.txt",
        b"bad\xff\xfe.txt",
    ];
    for n in &names {
        f.write(OsStr::from_bytes(n), "v1\n");
    }
    f.commit_all("odd");
    for n in &names {
        f.write(OsStr::from_bytes(n), "v1\nv2\n");
    }
    let r = f.repo();
    let st = r.status().unwrap();
    assert_eq!(st.unstaged.len(), names.len(), "{:?}", st.unstaged);
    for n in &names {
        let p = Path::new(OsStr::from_bytes(n));
        assert_eq!(
            st.unstaged.iter().filter(|e| e.path == p).count(),
            1,
            "{p:?}"
        );
        let d = r
            .diff_unstaged(p)
            .unwrap()
            .unwrap_or_else(|| panic!("diff of {p:?}"));
        assert_eq!(d.path(), Some(p), "diff path of {p:?}");
        assert_eq!(
            d.hunks[0]
                .lines
                .iter()
                .filter(|l| l.kind == LineKind::Added)
                .count(),
            1
        );
        let b = r.blame(p).unwrap().unwrap();
        assert_eq!(b.lines.len(), 2);
        assert!(b.commit_of(1).unwrap().uncommitted);
        assert_eq!(r.log(5, Some(p)).unwrap().len(), 1, "log of {p:?}");
        assert_eq!(r.line_changes(p, "v1\nv2\n").unwrap().unwrap().len(), 1);
    }
    let sha = f.head();
    let all = r.diff_commit(&sha, None).unwrap();
    let mut got: Vec<PathBuf> = all
        .iter()
        .map(|d| d.path().unwrap().to_path_buf())
        .collect();
    got.sort();
    let mut want: Vec<PathBuf> = names
        .iter()
        .map(|n| PathBuf::from(OsStr::from_bytes(n)))
        .collect();
    want.sort();
    assert_eq!(got, want);
    assert!(!f.path("x").exists(), "--output=x must never write a file");
}

#[test]
fn user_typed_revisions_are_verified() {
    let f = Fixture::new();
    f.write("a", "1\n");
    f.commit_all("one");
    let r = f.repo();
    for bad in ["--output=x", "-p", "nope", "HEAD:a", ""] {
        assert!(
            matches!(r.commit(bad), Err(vr_git::GitError::NotACommit(_))),
            "{bad}"
        );
        assert!(
            matches!(
                r.diff_commit(bad, None),
                Err(vr_git::GitError::NotACommit(_))
            ),
            "{bad}"
        );
    }
    assert!(!f.path("x").exists());
    assert!(r.commit("HEAD~0").is_ok());
}

#[test]
fn symlink_is_addressed_by_its_own_path() {
    let f = Fixture::new();
    std::os::unix::fs::symlink("/etc/hostname", f.path("link")).unwrap();
    f.commit_all("link");
    let r = f.repo();
    assert_eq!(r.relative(&f.path("link")).unwrap(), PathBuf::from("link"));
    std::fs::remove_file(f.path("link")).unwrap();
    std::os::unix::fs::symlink("/etc/passwd", f.path("link")).unwrap();
    let d = r.diff_unstaged(&f.path("link")).unwrap().unwrap();
    assert_eq!(d.path(), Some(Path::new("link")));
    assert_eq!(d.new_mode.as_deref(), Some("120000"));
}

#[test]
fn gutter_follows_git_line_ending_conversion() {
    // (attributes, core.autocrlf, text committed, editor buffer): the marks must equal
    // `git diff -U0` of the buffer saved to disk, i.e. after git's own clean conversion.
    let cases: &[(&str, Option<&str>, &str, &str)] = &[
        ("*.txt eol=crlf\n", None, "one\r\ntwo\r\n", "one\r\ntwo\r\n"),
        ("*.txt eol=crlf\n", None, "one\r\ntwo\r\n", "one\r\nTWO\r\n"),
        (
            "*.txt text=auto\n",
            None,
            "one\r\ntwo\r\n",
            "one\r\ntwo\r\n",
        ),
        (
            "*.txt text=auto\n",
            None,
            "one\r\ntwo\r\n",
            "one\r\ntwo\r\nthree\r\n",
        ),
        ("*.txt text\n", None, "one\ntwo\n", "one\r\ntwo\r\n"),
        ("", Some("true"), "one\ntwo\n", "one\r\ntwo\r\n"),
        ("", Some("input"), "one\ntwo\n", "ONE\r\ntwo\r\n"),
        // No conversion: a switch to CRLF is a change of every line.
        ("", None, "one\ntwo\n", "one\r\ntwo\r\n"),
        (
            "*.txt -text\n",
            Some("true"),
            "one\ntwo\n",
            "one\r\ntwo\r\n",
        ),
        // text=auto keeps CRLF already in the index (git's safe-autocrlf rule).
        ("", None, "one\r\ntwo\r\n", "one\r\ntwo\r\n"),
        ("", None, "one\r\ntwo\r\n", "one\ntwo\r\n"),
    ];
    for (i, &(attrs, autocrlf, committed, buffer)) in cases.iter().enumerate() {
        let f = Fixture::new();
        if let Some(v) = autocrlf {
            f.git(&["config", "core.autocrlf", v]);
        }
        if !attrs.is_empty() {
            f.write(".gitattributes", attrs);
        }
        f.write("a.txt", committed);
        f.commit_all("base");
        let r = f.repo();
        let ours = r.line_changes(Path::new("a.txt"), buffer).unwrap().unwrap();
        f.write("a.txt", buffer);
        assert_eq!(
            ours,
            marks_from_git(&f, "a.txt"),
            "case {i}: {attrs:?} {autocrlf:?}"
        );
    }
    // Every spelling git reads as true (a valueless key, 2, -1) or as input converts.
    for (i, spelling) in [
        "\tautocrlf\n",
        "\tautocrlf = 2\n",
        "\tautocrlf = -1\n",
        "\tautocrlf = INPUT\n",
        "\tautocrlf = off\n",
    ]
    .iter()
    .enumerate()
    {
        let f = Fixture::new();
        let cfg = f.dir.join(".git/config");
        let mut text = std::fs::read_to_string(&cfg).unwrap();
        text.push_str("[core]\n");
        text.push_str(spelling);
        std::fs::write(&cfg, text).unwrap();
        f.write("a.txt", "one\ntwo\n");
        f.commit_all("base");
        let r = f.repo();
        let buffer = "one\r\nTWO\r\n";
        let ours = r.line_changes(Path::new("a.txt"), buffer).unwrap().unwrap();
        f.write("a.txt", buffer);
        assert_eq!(
            ours,
            marks_from_git(&f, "a.txt"),
            "spelling {i}: {spelling:?}"
        );
    }
    // The case the review found: a CRLF checkout must not mark every line.
    let f = Fixture::new();
    f.write(".gitattributes", "*.bat eol=crlf\n");
    f.write("a.bat", "one\r\ntwo\r\n");
    f.commit_all("c");
    let r = f.repo();
    assert_eq!(
        r.line_changes(Path::new("a.bat"), "one\r\ntwo\r\n")
            .unwrap(),
        Some(vec![])
    );
}
