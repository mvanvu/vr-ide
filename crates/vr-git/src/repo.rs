//! Repository discovery, HEAD, upstream and status.

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use crate::cmd::{Git, Options};
use crate::parse::{as_bytes, fields, is_oid, lossy, os, parse_err, path_from};
use crate::{GitError, Result};

/// One git work tree, found by [`Repo::discover`]. Cheap to clone.
#[derive(Clone)]
pub struct Repo {
    workdir: PathBuf,
    git_dir: PathBuf,
    common_dir: PathBuf,
    pub(crate) git: Git,
}

impl std::fmt::Debug for Repo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Repo")
            .field("workdir", &self.workdir)
            .field("git_dir", &self.git_dir)
            .finish()
    }
}

/// What HEAD points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Head {
    /// On a branch; `oid` is `None` while the branch has no commit yet (unborn).
    Branch { name: String, oid: Option<String> },
    /// Detached at a commit.
    Detached { oid: String, short: String },
}

/// Commits ahead of and behind the upstream of the current branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AheadBehind {
    /// Short upstream name, e.g. `origin/main`.
    pub upstream: String,
    pub ahead: u32,
    pub behind: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileStatus {
    Modified,
    Added,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Untracked,
    Conflicted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    /// Repo-relative path (raw bytes kept).
    pub path: PathBuf,
    /// Source of a rename or copy.
    pub orig_path: Option<PathBuf>,
    pub status: FileStatus,
}

/// `git gui`-style lists: staged = index vs HEAD; unstaged = worktree vs index, plus untracked
/// and conflicted files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub staged: Vec<StatusEntry>,
    pub unstaged: Vec<StatusEntry>,
}

/// One index entry of a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexEntry {
    pub oid: String,
    pub stage: u8,
}

impl Repo {
    /// The work tree containing `path` (a file or directory), or `None` when there is no git, the
    /// repo is bare, `path` is inside `.git`, git refuses the repo (owned by another user and not
    /// allowed by `safe.directory`), or the work tree is not an ancestor of `path`
    /// (`core.worktree` elsewhere).
    pub fn discover(path: &Path) -> Result<Option<Repo>> {
        Self::discover_with(path, Options::default())
    }

    pub fn discover_with(path: &Path, opts: Options) -> Result<Option<Repo>> {
        let start = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent().unwrap_or(path).to_path_buf()
        };
        let Ok(start) = start.canonicalize() else {
            return Ok(None);
        };
        let git = Git::new(opts)?;
        let args = [
            "rev-parse",
            "--is-bare-repository",
            "--is-inside-work-tree",
            "--path-format=absolute",
            "--git-dir",
            "--git-common-dir",
            "--show-toplevel",
        ]
        .map(os);
        let out = git.run(&start, &args, None, git.auto())?;
        if out.code != 0 {
            return Ok(None);
        }
        let lines: Vec<&[u8]> = out
            .stdout
            .strip_suffix(b"\n")
            .unwrap_or(&out.stdout)
            .split(|&b| b == b'\n')
            .collect();
        if lines.len() != 5 || lines[0] != b"false" || lines[1] != b"true" {
            return Ok(None);
        }
        let (git_dir, common_dir, top) = (
            path_from(lines[2]),
            path_from(lines[3]),
            path_from(lines[4]),
        );
        let Ok(top) = top.canonicalize() else {
            return Ok(None);
        };
        if !start.starts_with(&top) {
            return Ok(None);
        }
        git.watch_config(&git_dir, &common_dir);
        Ok(Some(Repo {
            workdir: top,
            git_dir,
            common_dir,
            git,
        }))
    }

    /// The same repo with a runner of its own: its shutdown and process registry are not
    /// shared with this handle's clones (a reader's worker drop must not kill a write).
    pub(crate) fn with_own_runner(&self) -> Result<Repo> {
        let git = Git::new(self.git.opts().clone())?;
        git.watch_config(&self.git_dir, &self.common_dir);
        Ok(Repo {
            workdir: self.workdir.clone(),
            git_dir: self.git_dir.clone(),
            common_dir: self.common_dir.clone(),
            git,
        })
    }

    /// The work tree root (canonical).
    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    pub fn common_dir(&self) -> &Path {
        &self.common_dir
    }

    /// Kill every git this repo (and every clone of it) is running, with its children, and
    /// refuse later reads with [`GitError::ShutDown`]. Call it on exit; dropping a
    /// [`crate::GitWorker`] calls it for the worker's repo.
    pub fn shutdown(&self) {
        self.git.shutdown();
    }

    /// True when this repo was found above `root` (e.g. a dotfiles repo at `$HOME` for a
    /// workspace root without its own `.git`); the UI labels it "parent repo".
    pub fn is_parent_repo_of(&self, root: &Path) -> bool {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        root != self.workdir && root.starts_with(&self.workdir)
    }

    /// The repo-relative form of `path` (absolute, or relative to the work tree).
    pub fn relative(&self, path: &Path) -> Result<PathBuf> {
        let rel = if path.is_absolute() {
            let abs = canonical_lenient(path);
            abs.strip_prefix(&self.workdir)
                .map(Path::to_path_buf)
                .map_err(|_| GitError::OutsideRepo(path.into()))?
        } else {
            path.to_path_buf()
        };
        let clean = rel.components().all(|c| matches!(c, Component::Normal(_)));
        if rel.as_os_str().is_empty() || !clean {
            return Err(GitError::OutsideRepo(path.into()));
        }
        Ok(rel)
    }

    pub(crate) fn run(&self, args: &[&OsStr], user: bool) -> Result<Vec<u8>> {
        let lim = if user {
            self.git.user()
        } else {
            self.git.auto()
        };
        self.git.run_ok(&self.workdir, args, None, lim)
    }

    pub(crate) fn run_code(&self, args: &[&OsStr]) -> Result<crate::cmd::Output> {
        self.git.run(&self.workdir, args, None, self.git.auto())
    }

    /// The branch HEAD is on (also when unborn) or the detached commit.
    pub fn head(&self) -> Result<Head> {
        let sym = self.run_code(&["symbolic-ref", "-q", "HEAD"].map(os))?;
        let oid = self.head_oid()?;
        if sym.code == 0 {
            let full = lossy(sym.stdout.trim_ascii_end());
            let name = full
                .strip_prefix("refs/heads/")
                .map(str::to_string)
                .unwrap_or(full);
            return Ok(Head::Branch { name, oid });
        }
        let oid = oid.ok_or_else(|| parse_err("rev-parse", "detached HEAD without a commit"))?;
        let short = self.run(
            &["rev-parse", "--short", "--verify", "-q", "HEAD"].map(os),
            false,
        )?;
        Ok(Head::Detached {
            oid,
            short: lossy(short.trim_ascii_end()),
        })
    }

    pub(crate) fn head_oid(&self) -> Result<Option<String>> {
        let out = self.run_code(&["rev-parse", "--verify", "-q", "HEAD^{commit}"].map(os))?;
        let oid = lossy(out.stdout.trim_ascii_end());
        Ok((out.code == 0 && is_oid(&oid)).then_some(oid))
    }

    /// Ahead/behind of the current branch against its upstream; `None` when detached, unborn or
    /// without an upstream (or the upstream ref is missing).
    pub fn ahead_behind(&self) -> Result<Option<AheadBehind>> {
        if !matches!(self.head()?, Head::Branch { oid: Some(_), .. }) {
            return Ok(None);
        }
        let up = self.run_code(
            &[
                "rev-parse",
                "--abbrev-ref",
                "--symbolic-full-name",
                "@{upstream}",
            ]
            .map(os),
        )?;
        if up.code != 0 {
            return Ok(None);
        }
        let upstream = lossy(up.stdout.trim_ascii_end());
        let counts = self.run_code(
            &[
                "rev-list",
                "--left-right",
                "--count",
                "HEAD...@{upstream}",
                "--",
            ]
            .map(os),
        )?;
        if counts.code != 0 {
            return Ok(None);
        }
        let text = lossy(counts.stdout.trim_ascii_end());
        let mut it = text.split('\t').map(str::parse::<u32>);
        match (it.next(), it.next()) {
            (Some(Ok(ahead)), Some(Ok(behind))) => Ok(Some(AheadBehind {
                upstream,
                ahead,
                behind,
            })),
            _ => Err(parse_err("rev-list", text)),
        }
    }

    /// Staged and unstaged changes (`git status --porcelain=v2 -z`, untracked files listed one by one).
    pub fn status(&self) -> Result<Status> {
        let args = [
            "status",
            "--porcelain=v2",
            "-z",
            "--untracked-files=all",
            "--find-renames",
            "--ignored=no",
            "--ignore-submodules=dirty",
        ]
        .map(os);
        let out = self.run(&args, false)?;
        parse_status(&out)
    }

    /// The commit `rev` names (a SHA, branch, tag, `HEAD~2`, ...), as a full object id. Anything
    /// that is not a commit — including option-like text such as `--output=x` — is refused.
    pub fn resolve_commit(&self, rev: &str) -> Result<String> {
        if rev.is_empty() || rev.contains('\0') {
            return Err(GitError::NotACommit(rev.to_string()));
        }
        let spec = format!("{rev}^{{commit}}");
        let out = self.run_code(&[
            os("rev-parse"),
            os("--verify"),
            os("-q"),
            os("--end-of-options"),
            os(&spec),
        ])?;
        let oid = lossy(out.stdout.trim_ascii_end());
        if out.code != 0 || !is_oid(&oid) {
            return Err(GitError::NotACommit(rev.to_string()));
        }
        Ok(oid)
    }

    /// Index entries of exactly `rel` (several stages when conflicted).
    pub(crate) fn index_entries(&self, rel: &Path) -> Result<Vec<IndexEntry>> {
        let out = self.run(
            &[
                os("ls-files"),
                os("-z"),
                os("--stage"),
                os("--"),
                rel.as_os_str(),
            ],
            false,
        )?;
        let mut v = Vec::new();
        for rec in fields(&out) {
            // "<mode> <oid> <stage>\t<path>"
            let Some(tab) = rec.iter().position(|&b| b == b'\t') else {
                continue;
            };
            if &rec[tab + 1..] != as_bytes(rel) {
                continue;
            }
            let meta = lossy(&rec[..tab]);
            let mut parts = meta.split(' ');
            let (_, oid, stage) = (parts.next(), parts.next(), parts.next());
            if let (Some(oid), Some(Ok(stage))) = (oid, stage.map(str::parse::<u8>)) {
                v.push(IndexEntry {
                    oid: oid.to_string(),
                    stage,
                });
            }
        }
        Ok(v)
    }

    /// Raw bytes of a blob, bounded by the per-file cap.
    pub(crate) fn blob(&self, oid: &str) -> Result<Vec<u8>> {
        if !is_oid(oid) {
            return Err(GitError::NotACommit(oid.to_string()));
        }
        let lim = crate::cmd::Limits::new(self.git.opts().timeout, self.git.opts().max_file);
        self.git.run_ok(
            &self.workdir,
            &[os("cat-file"), os("blob"), os(oid)],
            None,
            lim,
        )
    }
}

/// Canonicalize what exists of `path`: a deleted file keeps its name under the canonical parent.
fn canonical_lenient(path: &Path) -> PathBuf {
    // The last component is kept as is: a tracked symlink names itself, not its target.
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => parent
            .canonicalize()
            .unwrap_or_else(|_| canonical_lenient(parent))
            .join(name),
        _ => path.canonicalize().unwrap_or_else(|_| path.to_path_buf()),
    }
}

fn kind(c: u8) -> Option<FileStatus> {
    Some(match c {
        b'M' => FileStatus::Modified,
        b'A' => FileStatus::Added,
        b'D' => FileStatus::Deleted,
        b'R' => FileStatus::Renamed,
        b'C' => FileStatus::Copied,
        b'T' => FileStatus::TypeChanged,
        _ => return None,
    })
}

pub(crate) fn parse_status(out: &[u8]) -> Result<Status> {
    let mut st = Status::default();
    let recs = fields(out);
    let mut i = 0;
    while i < recs.len() {
        let rec = recs[i];
        i += 1;
        let bad = || parse_err("status", lossy(rec));
        match rec.first() {
            Some(b'1') | Some(b'2') => {
                // 1 XY sub mH mI mW hH hI path | 2 XY sub mH mI mW hH hI Xscore path\0orig
                let n = if rec[0] == b'1' { 8 } else { 9 };
                let parts: Vec<&[u8]> = rec.splitn(n + 1, |&b| b == b' ').collect();
                if parts.len() != n + 1 || parts[1].len() != 2 {
                    return Err(bad());
                }
                let path = path_from(parts[n]);
                let orig = if rec[0] == b'2' {
                    let o = recs.get(i).ok_or_else(bad)?;
                    i += 1;
                    Some(path_from(o))
                } else {
                    None
                };
                let (x, y) = (parts[1][0], parts[1][1]);
                if let Some(k) = kind(x) {
                    let orig_path = if matches!(k, FileStatus::Renamed | FileStatus::Copied) {
                        orig.clone()
                    } else {
                        None
                    };
                    st.staged.push(StatusEntry {
                        path: path.clone(),
                        orig_path,
                        status: k,
                    });
                }
                if let Some(k) = kind(y) {
                    let orig_path = if matches!(k, FileStatus::Renamed | FileStatus::Copied) {
                        orig
                    } else {
                        None
                    };
                    st.unstaged.push(StatusEntry {
                        path,
                        orig_path,
                        status: k,
                    });
                }
            }
            Some(b'u') => {
                let parts: Vec<&[u8]> = rec.splitn(11, |&b| b == b' ').collect();
                if parts.len() != 11 {
                    return Err(bad());
                }
                st.unstaged.push(StatusEntry {
                    path: path_from(parts[10]),
                    orig_path: None,
                    status: FileStatus::Conflicted,
                });
            }
            Some(b'?') => {
                let path = rec.get(2..).ok_or_else(bad)?;
                st.unstaged.push(StatusEntry {
                    path: path_from(path),
                    orig_path: None,
                    status: FileStatus::Untracked,
                });
            }
            Some(b'#') | Some(b'!') => {}
            _ => return Err(bad()),
        }
    }
    Ok(st)
}
