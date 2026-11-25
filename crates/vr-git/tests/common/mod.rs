//! Temp repos built with the real git CLI, in a hermetic environment: HOME, XDG_CONFIG_HOME and
//! GIT_CONFIG_GLOBAL point into the temp dir and system config is off, so the developer's own
//! config can neither hide nor cause a failure.
#![allow(dead_code)]

use std::ffi::{OsStr, OsString};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use vr_git::{Options, Repo};

pub struct Fixture {
    pub tmp: tempfile::TempDir,
    pub dir: PathBuf,
    pub home: PathBuf,
    time: std::cell::Cell<i64>,
}

impl Fixture {
    /// An empty, initialised repo on branch `main`.
    pub fn new() -> Fixture {
        let f = Fixture::bare_dir();
        f.git(&["init", "-q", "-b", "main"]);
        f
    }

    /// A temp dir with a hermetic home but no repo.
    pub fn bare_dir() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let home = root.join("home");
        let dir = root.join("repo");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(home.join(".gitconfig"), "").unwrap();
        Fixture {
            tmp,
            dir,
            home,
            time: std::cell::Cell::new(1_700_000_000),
        }
    }

    /// The environment both the fixture's git and the library under test start from.
    pub fn env(&self) -> Vec<(OsString, OsString)> {
        let path = std::env::var_os("PATH").unwrap_or_default();
        vec![
            ("PATH".into(), path),
            ("HOME".into(), self.home.clone().into()),
            ("XDG_CONFIG_HOME".into(), self.home.join(".config").into()),
            (
                "GIT_CONFIG_GLOBAL".into(),
                self.home.join(".gitconfig").into(),
            ),
            ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
            ("LC_ALL".into(), "C".into()),
        ]
    }

    pub fn options(&self) -> Options {
        Options {
            env: Some(self.env()),
            ..Options::default()
        }
    }

    pub fn repo(&self) -> Repo {
        Repo::discover_with(&self.dir, self.options())
            .unwrap()
            .expect("repo")
    }

    pub fn git_in<S: AsRef<OsStr>>(&self, dir: &Path, args: &[S]) -> Vec<u8> {
        let t = self.time.get() + 60;
        self.time.set(t);
        let date = format!("{t} +0000");
        let out = Command::new("git")
            .current_dir(dir)
            .env_clear()
            .envs(self.env())
            .env("GIT_AUTHOR_NAME", "Ann Author")
            .env("GIT_AUTHOR_EMAIL", "ann@example.com")
            .env("GIT_COMMITTER_NAME", "Cid Committer")
            .env("GIT_COMMITTER_EMAIL", "cid@example.com")
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?}: {}",
            args.iter()
                .map(|a| a.as_ref().to_os_string())
                .collect::<Vec<_>>(),
            String::from_utf8_lossy(&out.stderr)
        );
        out.stdout
    }

    pub fn git<S: AsRef<OsStr>>(&self, args: &[S]) -> Vec<u8> {
        self.git_in(&self.dir, args)
    }

    pub fn git_s<S: AsRef<OsStr>>(&self, args: &[S]) -> String {
        String::from_utf8_lossy(&self.git(args)).into_owned()
    }

    pub fn write<P: AsRef<Path>>(&self, rel: P, text: impl AsRef<[u8]>) {
        let p = self.dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    /// Stage everything and record a commit; returns its full id.
    pub fn commit_all(&self, msg: &str) -> String {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "--allow-empty", "-m", msg]);
        self.head()
    }

    pub fn head(&self) -> String {
        self.git_s(&["rev-parse", "HEAD"]).trim().to_string()
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.dir.join(rel)
    }

    /// A shell script that creates `marker` when run.
    pub fn marker_script(&self, name: &str) -> (PathBuf, PathBuf) {
        let marker = self.tmp.path().join(format!("{name}.ran"));
        let script = self.tmp.path().join(format!("{name}.sh"));
        std::fs::write(
            &script,
            format!("#!/bin/sh\ntouch '{}'\ncat\n", marker.display()),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        (script, marker)
    }
}
