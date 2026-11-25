//! Discard under injected faults (review M7-2 round 1): a failure after the recovery copy is
//! made must name the copy, and a panic between moving the target and finishing the copy must
//! leave the earlier content where the user can find it.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::recovery::{FAULT, Fault};
use crate::{GitError, Options, Repo, Writer};

struct Fx {
    _tmp: tempfile::TempDir,
    dir: PathBuf,
    env: Vec<(OsString, OsString)>,
}

impl Fx {
    fn new() -> Fx {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let home = root.join("home");
        let dir = root.join("repo");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(home.join(".gitconfig"), "").unwrap();
        let env = vec![
            ("PATH".into(), std::env::var_os("PATH").unwrap_or_default()),
            ("HOME".into(), home.clone().into()),
            ("GIT_CONFIG_GLOBAL".into(), home.join(".gitconfig").into()),
            ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
            ("LC_ALL".into(), "C".into()),
        ];
        let f = Fx {
            _tmp: tmp,
            dir,
            env,
        };
        f.git(&["init", "-q", "-b", "main"]);
        f.write("f", "a\n");
        f.git(&["add", "f"]);
        f.git(&[
            "-c",
            "user.name=A",
            "-c",
            "user.email=a@example.com",
            "commit",
            "-qm",
            "c",
        ]);
        f
    }

    fn git(&self, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(&self.dir)
            .env_clear()
            .envs(self.env.clone())
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "{args:?}: {out:?}");
    }

    fn write(&self, rel: &str, text: &str) {
        let p = self.dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    fn writer(&self, recovery: &Path) -> (Repo, Writer) {
        let repo = Repo::discover_with(
            &self.dir,
            Options {
                env: Some(self.env.clone()),
                recovery_dir: Some(recovery.to_path_buf()),
                ..Options::default()
            },
        )
        .unwrap()
        .unwrap();
        let w = Writer::open(&repo).unwrap();
        (repo, w)
    }
}

fn other_filesystem() -> Option<tempfile::TempDir> {
    use std::os::unix::fs::MetadataExt;
    let other = tempfile::tempdir_in("/dev/shm").ok()?;
    let here = tempfile::tempdir().ok()?;
    let dev = |p: &Path| std::fs::metadata(p).unwrap().dev();
    (dev(other.path()) != dev(here.path())).then_some(other)
}

fn with_fault<T>(at: Fault, f: impl FnOnce() -> T) -> T {
    FAULT.with(|c| c.set(at));
    let r = f();
    FAULT.with(|c| c.set(Fault::None));
    r
}

/// Every file below `dir` (relative) with its content.
fn files(dir: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let rel = p.strip_prefix(dir).unwrap().to_string_lossy().into_owned();
                out.push((rel, std::fs::read_to_string(&p).unwrap_or_default()));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn failure_after_the_cross_filesystem_copy_names_the_copy() {
    let Some(other) = other_filesystem() else {
        eprintln!("skipped: no /dev/shm on another filesystem");
        return;
    };
    let f = Fx::new();
    f.write("u/a.txt", "A\n");
    f.write("u/sub/b.txt", "B\n");
    let rec = other.path().join("git-recovery");
    let (repo, w) = f.writer(&rec);
    let plan = w.prepare_discard_file(&f.dir.join("u")).unwrap();
    let e = with_fault(Fault::Remove, || w.discard(&plan, false)).unwrap_err();
    let text = e.to_string();
    assert!(matches!(e, GitError::Io(_)), "{e:?}");
    let copy = std::fs::read_dir(&rec)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| !p.ends_with(".gitignore"))
        .expect("a recovery copy");
    assert!(text.contains(&copy.display().to_string()), "{text}");
    assert!(text.contains("partly removed"), "{text}");
    assert!(!text.contains("nothing was discarded"), "{text}");
    assert_eq!(
        files(&copy),
        vec![
            ("a.txt".to_string(), "A\n".to_string()),
            ("sub/b.txt".to_string(), "B\n".to_string())
        ]
    );
    // What was not removed is back under its own name, not hidden.
    assert!(f.dir.join("u").exists());
    assert!(crate::leftover_discards(&repo).unwrap().is_empty());
}

fn panic_in_the_window(recovery: &Path) -> (Fx, Repo) {
    let f = Fx::new();
    f.write("f", "edited\n");
    let (repo, w) = f.writer(recovery);
    let plan = w.prepare_discard_file(&f.dir.join("f")).unwrap();
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_fault(Fault::Window, || w.discard(&plan, false))
    }));
    FAULT.with(|c| c.set(Fault::None));
    assert!(r.is_err(), "the injected panic must fire");
    (f, repo)
}

#[test]
fn panic_in_the_window_on_one_filesystem_leaves_the_copy_in_the_recovery_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let rec = tmp.path().join("git-recovery");
    let (f, repo) = panic_in_the_window(&rec);
    let copies: Vec<_> = files(&rec)
        .into_iter()
        .filter(|(n, _)| n != ".gitignore")
        .collect();
    assert_eq!(copies.len(), 1, "{copies:?}");
    assert_eq!(copies[0].1, "edited\n");
    assert!(copies[0].0.ends_with("-repo-f"), "{copies:?}");
    assert!(crate::leftover_discards(&repo).unwrap().is_empty());
    drop(f);
}

#[test]
fn panic_in_the_window_across_filesystems_leaves_a_listed_leftover() {
    let Some(other) = other_filesystem() else {
        eprintln!("skipped: no /dev/shm on another filesystem");
        return;
    };
    let (f, repo) = panic_in_the_window(&other.path().join("git-recovery"));
    let left = crate::leftover_discards(&repo).unwrap();
    assert_eq!(left.len(), 1, "{left:?}");
    assert!(left[0].earlier_content);
    assert_eq!(
        std::fs::read_to_string(f.dir.join(&left[0].path)).unwrap(),
        "edited\n"
    );
}
