//! The user's own writes: stage / unstage a file, a hunk or lines, discard with a recovery
//! copy, commit (threat model SEC-9 .. SEC-12, SEC-14). Push and pull are in `remote.rs`.
//!
//! Every write is an explicit user action, runs one at a time per repository, re-checks its
//! path at execution, never retries, never removes git's lock files and never uses force,
//! hard resets or bulk worktree commands. git's own error text is returned as is.

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::hash::RandomState;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::time::Duration;

use crate::cmd::{Cancel, Ctl, Limits, Output, Profile};
use crate::diff::{FileDiff, LineKind};
use crate::parse::{as_bytes, fields, lossy, os};
use crate::patch::{self, Direction};
use crate::recovery::{self, Fault, KeepError, Snapshot};
use crate::repo::{FileStatus, Repo};
use crate::sanitize::sanitize_line;
use crate::state::{Operation, RepoState};
use crate::{GitError, Result};

/// Git's well-known empty tree (unstage on an unborn branch).
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// One write at a time per repository, across every [`Writer`] of it in this process.
#[derive(Default)]
pub(crate) struct RepoLock {
    busy: Mutex<bool>,
    freed: Condvar,
}

pub(crate) struct LockGuard(Arc<RepoLock>);

impl Drop for LockGuard {
    fn drop(&mut self) {
        *self.0.busy.lock().unwrap() = false;
        self.0.freed.notify_all();
    }
}

impl RepoLock {
    fn of(git_dir: &Path) -> Arc<RepoLock> {
        static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Weak<RepoLock>>>> = OnceLock::new();
        let key = git_dir
            .canonicalize()
            .unwrap_or_else(|_| git_dir.to_path_buf());
        let mut map = LOCKS.get_or_init(Default::default).lock().unwrap();
        map.retain(|_, w| w.strong_count() > 0);
        if let Some(l) = map.get(&key).and_then(Weak::upgrade) {
            return l;
        }
        let l = Arc::new(RepoLock::default());
        map.insert(key, Arc::downgrade(&l));
        l
    }

    /// Wait for the running write (a queue); a cancelled waiter gives up.
    pub(crate) fn acquire(self: &Arc<Self>, cancel: Option<&Cancel>) -> Result<LockGuard> {
        let mut busy = self.busy.lock().unwrap();
        while *busy {
            if cancel.is_some_and(Cancel::is_cancelled) {
                return Err(GitError::Cancelled);
            }
            busy = self
                .freed
                .wait_timeout(busy, Duration::from_millis(50))
                .unwrap()
                .0;
        }
        *busy = true;
        Ok(LockGuard(self.clone()))
    }
}

/// Options of [`Writer::commit`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CommitOptions {
    pub amend: bool,
    pub signoff: bool,
}

#[derive(Debug, Clone)]
enum DiscardWhat {
    /// The whole file back to its index version (or removed, when untracked).
    File { untracked: bool },
    /// Part of the worktree change reverted with `git apply -R`.
    Patch {
        shown: Box<FileDiff>,
        hunk: usize,
        selected: Vec<bool>,
    },
}

/// A discard the user is asked to confirm. [`Writer::discard`] runs it only if the target is
/// still byte-identical to what it was when this plan was made (SEC-11).
#[derive(Debug, Clone)]
pub struct DiscardPlan {
    rel: PathBuf,
    what: DiscardWhat,
    keys: RandomState,
    digest: u64,
    /// Number of hunks the discard reverts (for the confirmation text).
    pub hunks: usize,
}

impl DiscardPlan {
    /// The repo-relative path the discard changes.
    pub fn path(&self) -> &Path {
        &self.rel
    }

    pub fn is_untracked(&self) -> bool {
        matches!(self.what, DiscardWhat::File { untracked: true })
    }
}

/// The result of a discard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discarded {
    pub path: PathBuf,
    /// Copy of the bytes before the discard (`None` when the target did not exist, e.g. a
    /// deleted file brought back).
    pub recovery: Option<PathBuf>,
}

/// Write access to one repository. It has its own git runner, so a reader's shutdown (or a
/// dropped [`crate::GitWorker`]) never kills a write; call [`Writer::shutdown`] only on exit,
/// after the writes finished. Cheap to clone; clones share the runner and the write queue.
#[derive(Clone)]
pub struct Writer {
    pub(crate) repo: Repo,
    pub(crate) lock: Arc<RepoLock>,
}

impl std::fmt::Debug for Writer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Writer")
            .field("workdir", &self.repo.workdir())
            .finish()
    }
}

impl Writer {
    pub fn open(repo: &Repo) -> Result<Writer> {
        let repo = repo.with_own_runner()?;
        let lock = RepoLock::of(repo.git_dir());
        Ok(Writer { repo, lock })
    }

    /// Read access through the writer's own runner.
    pub fn repo(&self) -> &Repo {
        &self.repo
    }

    /// Kill a running write and refuse later ones (exit only: a killed write may leave
    /// `index.lock` behind, which vr-ide never removes).
    pub fn shutdown(&self) {
        self.repo.shutdown();
    }

    pub fn state(&self) -> Result<RepoState> {
        self.repo.state()
    }

    pub(crate) fn run_write(
        &self,
        args: &[&OsStr],
        stdin: Option<&[u8]>,
        lim: Limits,
        ctl: &Ctl,
    ) -> Result<Output> {
        self.repo
            .git
            .run_ctl(self.repo.workdir(), args, stdin, lim, ctl)
    }

    fn index_limits(&self) -> Limits {
        self.repo.git.user().with(Profile::Index)
    }

    fn run_index(&self, args: &[&OsStr], stdin: Option<&[u8]>) -> Result<()> {
        let out = self.run_write(args, stdin, self.index_limits(), &Ctl::default())?;
        ok_or_failed(args, out).map(|_| ())
    }

    /// `path` as a repo-relative path, re-checked at execution (SEC-9): inside this work tree
    /// (canonical) and not inside a nested repository (another root's repo or a submodule).
    fn target(&self, path: &Path) -> Result<PathBuf> {
        let rel = self.repo.relative(path)?;
        let wd = self.repo.workdir();
        let mut dir = rel.parent();
        while let Some(d) = dir {
            if d.as_os_str().is_empty() {
                break;
            }
            if wd.join(d).join(".git").exists() {
                return Err(GitError::Refused(format!(
                    "{} belongs to the repository at {}",
                    rel.display(),
                    wd.join(d).display()
                )));
            }
            dir = d.parent();
        }
        Ok(rel)
    }

    fn refuse_conflicted(&self, state: &RepoState, rel: &Path) -> Result<()> {
        if state.is_conflicted(rel) {
            return Err(GitError::Refused(format!(
                "{} has conflicts ({}); resolve them in the terminal",
                rel.display(),
                state
            )));
        }
        Ok(())
    }

    /// `(mode, oid, stage)` of every index entry of exactly `rel`.
    fn index_of(&self, rel: &Path) -> Result<Vec<(String, String, u8)>> {
        let out = self.repo.run(
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
            let Some(tab) = rec.iter().position(|&b| b == b'\t') else {
                continue;
            };
            if &rec[tab + 1..] != as_bytes(rel) {
                continue;
            }
            let meta = lossy(&rec[..tab]);
            let p: Vec<&str> = meta.split(' ').collect();
            if let [mode, oid, stage] = p[..]
                && let Ok(stage) = stage.parse()
            {
                v.push((mode.to_string(), oid.to_string(), stage));
            }
        }
        Ok(v)
    }

    /// Stage the whole file: a change, a new (untracked) file or a deletion (`git add --all`
    /// on exactly that path).
    pub fn stage_file(&self, path: &Path) -> Result<()> {
        let _g = self.lock.acquire(None)?;
        let rel = self.target(path)?;
        self.refuse_conflicted(&self.state()?, &rel)?;
        let full = self.repo.workdir().join(&rel);
        if let Ok(m) = std::fs::symlink_metadata(&full)
            && m.is_dir()
        {
            let gitlink = self
                .index_of(&rel)?
                .iter()
                .any(|(mode, _, _)| mode == "160000");
            if !gitlink {
                return Err(GitError::Refused(format!(
                    "{} is a folder; stage its files one by one",
                    rel.display()
                )));
            }
        }
        self.run_index(&[os("add"), os("--all"), os("--"), rel.as_os_str()], None)
    }

    /// Unstage the whole file (the index entry goes back to HEAD's; a staged rename is undone
    /// on both paths). The worktree is not touched.
    pub fn unstage_file(&self, path: &Path) -> Result<()> {
        let _g = self.lock.acquire(None)?;
        let rel = self.target(path)?;
        self.refuse_conflicted(&self.state()?, &rel)?;
        let mut paths = vec![rel.clone()];
        if let Some(src) = self.repo.staged_source(&rel)? {
            paths.push(src);
        }
        let source = match self.repo.head_oid()? {
            Some(oid) => oid,
            None => EMPTY_TREE.to_string(),
        };
        let source_arg = format!("--source={source}");
        let mut args: Vec<&OsStr> = vec![os("restore"), os("--staged"), os(&source_arg), os("--")];
        args.extend(paths.iter().map(|p| p.as_os_str()));
        self.run_index(&args, None)
    }

    /// Stage hunk `hunk` of `shown` (a worktree diff from [`Repo::diff_unstaged`]).
    pub fn stage_hunk(&self, shown: &FileDiff, hunk: usize) -> Result<()> {
        self.apply_index(shown, hunk, None, false)
    }

    /// Stage some lines of hunk `hunk`: `lines` are indexes into `hunk.lines` (context lines
    /// are ignored).
    pub fn stage_lines(&self, shown: &FileDiff, hunk: usize, lines: &[usize]) -> Result<()> {
        self.apply_index(shown, hunk, Some(lines), false)
    }

    /// Unstage hunk `hunk` of `shown` (a staged diff from [`Repo::diff_staged`]).
    pub fn unstage_hunk(&self, shown: &FileDiff, hunk: usize) -> Result<()> {
        self.apply_index(shown, hunk, None, true)
    }

    pub fn unstage_lines(&self, shown: &FileDiff, hunk: usize, lines: &[usize]) -> Result<()> {
        self.apply_index(shown, hunk, Some(lines), true)
    }

    fn apply_index(
        &self,
        shown: &FileDiff,
        hunk: usize,
        lines: Option<&[usize]>,
        staged: bool,
    ) -> Result<()> {
        let _g = self.lock.acquire(None)?;
        let (_, raw, selected) = self.checked_patch_input(shown, hunk, lines, staged)?;
        let header = patch::file_header(&raw).ok_or_else(|| {
            GitError::Refused("this change can only be staged as a whole file".into())
        })?;
        let dir = if staged {
            Direction::Reverse
        } else {
            Direction::Forward
        };
        let p =
            patch::build(header, &shown.hunks[hunk], &selected, dir).map_err(GitError::Refused)?;
        let mut args = vec![os("apply"), os("--cached"), os("--whitespace=nowarn")];
        if staged {
            args.push(os("-R"));
        }
        let mut check = args.clone();
        check.push(os("--check"));
        self.run_index(&check, Some(&p))?;
        self.run_index(&args, Some(&p))
    }

    /// Everything a hunk or line action must prove before it builds a patch (SEC-10, SEC-14,
    /// SEC-18): an ordinary text change, no conflict, no filter, and the diff git prints now is
    /// exactly the one the user saw, on the same index (and HEAD) blob.
    fn checked_patch_input(
        &self,
        shown: &FileDiff,
        hunk: usize,
        lines: Option<&[usize]>,
        staged: bool,
    ) -> Result<(PathBuf, Vec<u8>, Vec<bool>)> {
        let path = shown
            .path()
            .ok_or_else(|| GitError::Refused("diff without a path".into()))?;
        let rel = self.target(path)?;
        self.refuse_conflicted(&self.state()?, &rel)?;
        let whole = |why: &str| {
            GitError::Refused(format!(
                "{} {why}: stage, unstage or discard the whole file",
                rel.display()
            ))
        };
        match shown.status {
            FileStatus::Modified => {}
            FileStatus::Conflicted => return Err(whole("has conflicts")),
            FileStatus::Untracked | FileStatus::Added => return Err(whole("is a new file")),
            FileStatus::Deleted => return Err(whole("is deleted")),
            FileStatus::Renamed | FileStatus::Copied => return Err(whole("is renamed")),
            FileStatus::TypeChanged => return Err(whole("changed its type")),
        }
        if shown.binary {
            return Err(whole("is binary"));
        }
        if shown.old_mode != shown.new_mode {
            return Err(whole("changed its mode"));
        }
        match shown.new_mode.as_deref() {
            Some("120000") => return Err(whole("is a symlink")),
            Some("160000") => return Err(whole("is a submodule")),
            _ => {}
        }
        if hunk >= shown.hunks.len() {
            return Err(GitError::Refused("no such hunk".into()));
        }
        self.refuse_converted(&rel)?;

        let raw = if staged {
            self.repo.raw_staged(&rel)?
        } else {
            Some(self.repo.raw_unstaged(&rel)?)
        }
        .unwrap_or_default();
        let fresh = crate::diff::parse_diff(&raw)?
            .into_iter()
            .find(|d| d.path() == Some(rel.as_path()));
        if fresh.as_ref() != Some(shown) {
            return Err(GitError::Stale(rel.display().to_string()));
        }
        // Base blob ids: the index entry is the preimage of a stage and the postimage of an
        // unstage; an unstage also needs HEAD's blob to be the shown one.
        let entries = self.index_of(&rel)?;
        let index_oid = match entries.as_slice() {
            [(_, oid, 0)] => oid.clone(),
            _ => return Err(GitError::Stale(rel.display().to_string())),
        };
        let expect_index = if staged {
            &shown.new_oid
        } else {
            &shown.old_oid
        };
        if expect_index.as_deref() != Some(index_oid.as_str()) {
            return Err(GitError::Stale(rel.display().to_string()));
        }
        if staged {
            let mut spec = std::ffi::OsString::from("HEAD:");
            spec.push(rel.as_os_str());
            let head = self.repo.run_code(&[
                os("rev-parse"),
                os("--verify"),
                os("-q"),
                os("--end-of-options"),
                &spec,
            ])?;
            if head.code != 0
                || shown.old_oid.as_deref() != Some(lossy(head.stdout.trim_ascii()).as_str())
            {
                return Err(GitError::Stale(rel.display().to_string()));
            }
        }

        let h = &shown.hunks[hunk];
        let selected = match lines {
            None => vec![true; h.lines.len()],
            Some(idx) => {
                let mut s = vec![false; h.lines.len()];
                for &i in idx {
                    *s.get_mut(i)
                        .ok_or_else(|| GitError::Refused("no such line in the hunk".into()))? =
                        true;
                }
                s
            }
        };
        if !h
            .lines
            .iter()
            .zip(&selected)
            .any(|(l, &s)| s && l.kind != LineKind::Context)
        {
            return Err(GitError::Refused("no changed line selected".into()));
        }
        Ok((rel, raw, selected))
    }

    /// A clean/smudge filter or a working-tree encoding makes the shown diff differ from what
    /// git compares, so a patch built from it could stage the wrong bytes.
    fn refuse_converted(&self, rel: &Path) -> Result<()> {
        let out = self.repo.run(
            &[
                os("check-attr"),
                os("-z"),
                os("filter"),
                os("working-tree-encoding"),
                os("--"),
                rel.as_os_str(),
            ],
            false,
        )?;
        let f = fields(&out);
        for c in f.chunks(3) {
            if c.len() == 3 && c[2] != b"unspecified" && c[2] != b"unset" {
                return Err(GitError::Refused(format!(
                    "{} has a {} attribute: stage, unstage or discard the whole file",
                    rel.display(),
                    lossy(c[1])
                )));
            }
        }
        Ok(())
    }

    fn snapshot(&self, rel: &Path) -> Result<Snapshot> {
        recovery::snapshot(
            &self.repo.workdir().join(rel),
            self.repo.git.opts().recovery_cap,
        )
    }

    fn plan(&self, rel: PathBuf, what: DiscardWhat, hunks: usize) -> Result<DiscardPlan> {
        let keys = RandomState::new();
        let digest = recovery::digest(&keys, &self.snapshot(&rel)?);
        Ok(DiscardPlan {
            rel,
            what,
            keys,
            digest,
            hunks,
        })
    }

    /// Prepare discarding every worktree change of a file (back to its index version), or an
    /// untracked file, symlink or folder (removed). Ask the user, then call [`Writer::discard`].
    pub fn prepare_discard_file(&self, path: &Path) -> Result<DiscardPlan> {
        let rel = self.target(path)?;
        self.refuse_conflicted(&self.state()?, &rel)?;
        let entries = self.index_of(&rel)?;
        if entries.iter().any(|(mode, _, _)| mode == "160000") {
            return Err(GitError::Refused(format!(
                "{} is a submodule; discard inside it in the terminal",
                rel.display()
            )));
        }
        let untracked = entries.is_empty();
        if untracked {
            let tracked_below = self.repo.run(
                &[os("ls-files"), os("-z"), os("--"), rel.as_os_str()],
                false,
            )?;
            if !tracked_below.is_empty() {
                return Err(GitError::Refused(format!(
                    "{} contains tracked files; discard them one by one",
                    rel.display()
                )));
            }
            if matches!(self.snapshot(&rel)?, Snapshot::Absent) {
                return Err(GitError::Refused(format!(
                    "{} does not exist",
                    rel.display()
                )));
            }
            return self.plan(rel, DiscardWhat::File { untracked: true }, 1);
        }
        let hunks = match self.repo.diff_unstaged(&rel)? {
            None => {
                return Err(GitError::Refused(format!(
                    "{} has no worktree changes",
                    rel.display()
                )));
            }
            Some(d) => d.hunks.len().max(1),
        };
        self.plan(rel, DiscardWhat::File { untracked: false }, hunks)
    }

    /// Prepare discarding hunk `hunk` (or the given lines of it) of a worktree diff.
    pub fn prepare_discard_hunk(
        &self,
        shown: &FileDiff,
        hunk: usize,
        lines: Option<&[usize]>,
    ) -> Result<DiscardPlan> {
        let (rel, raw, selected) = self.checked_patch_input(shown, hunk, lines, false)?;
        let header = patch::file_header(&raw).ok_or_else(|| {
            GitError::Refused("this change can only be discarded as a whole file".into())
        })?;
        patch::build(header, &shown.hunks[hunk], &selected, Direction::Reverse)
            .map_err(GitError::Refused)?;
        self.plan(
            rel,
            DiscardWhat::Patch {
                shown: Box::new(shown.clone()),
                hunk,
                selected,
            },
            1,
        )
    }

    /// Run a confirmed discard. Refused when the editor has unsaved changes to the file
    /// (`buffer_dirty`) or the file changed since the plan was made (SEC-11).
    ///
    /// The new content (the index version, or the file with the selection reverted) is
    /// computed by git and written beside the target first. Then the target is renamed
    /// straight into the recovery folder, which is atomic, and checked once more against the
    /// plan there (a mismatch puts it back); only then does the new content take its place,
    /// with a rename that never replaces a file another program created meanwhile. So a
    /// concurrent writer loses no byte: what it writes through a descriptor opened before the
    /// move lands in the recovery copy, and a file it creates at the path after the move is
    /// kept (the discard then reports that). When the move fails, nothing is discarded. An
    /// untracked target is only moved; a symlink is moved itself, never followed.
    ///
    /// With the recovery folder on another filesystem the target is renamed aside to a hidden
    /// `.vr-ide-discard-*.old` name beside it, copied, fsynced and verified, and only then
    /// removed; a program still holding the file open after that writes into the removed
    /// file. A folder with a subfolder this user cannot write to is refused before anything
    /// moves. If the removal still fails part way, the error says the target was partly
    /// removed and names the full copy. A crash in between leaves the hidden name, which
    /// [`leftover_discards`] lists. Contract for the caller (M7-3): no
    /// save of a path, by vr-ide itself or its format-on-save, may start while a discard of
    /// that path runs; block it until this call returns.
    pub fn discard(&self, plan: &DiscardPlan, buffer_dirty: bool) -> Result<Discarded> {
        if buffer_dirty {
            return Err(GitError::Refused(format!(
                "{} has unsaved changes in the editor; save or revert them first",
                plan.rel.display()
            )));
        }
        let _g = self.lock.acquire(None)?;
        let wd = self.repo.workdir().to_path_buf();
        let rel = self.target(&plan.rel)?;
        // Through the real filesystem too: a parent folder swapped for a symlink since the
        // plan resolves elsewhere, and std::fs would follow it (git refuses such paths).
        if self.target(&wd.join(&plan.rel))? != rel {
            return Err(GitError::Stale(rel.display().to_string()));
        }
        self.refuse_conflicted(&self.state()?, &rel)?;
        let snap = self.snapshot(&rel)?;
        if recovery::digest(&plan.keys, &snap) != plan.digest {
            return Err(GitError::Stale(rel.display().to_string()));
        }
        let dir = match &self.repo.git.opts().recovery_dir {
            Some(d) => d.clone(),
            None => recovery::default_dir(|k| self.repo.git.env_var(k).map(|v| v.to_os_string()))
                .ok_or_else(|| {
                GitError::Refused("no folder for recovery copies (HOME is not set)".into())
            })?,
        };
        let dir = recovery::prepare_dir(&dir, &self.repo.git)?;
        let replacement = match &plan.what {
            DiscardWhat::File { untracked: true } => None,
            DiscardWhat::File { untracked: false } => Some(self.index_version(&rel)?),
            DiscardWhat::Patch {
                shown,
                hunk,
                selected,
            } => {
                let lines: Vec<usize> = (0..selected.len()).filter(|&i| selected[i]).collect();
                let (_, raw, sel) = self.checked_patch_input(shown, *hunk, Some(&lines), false)?;
                let header = patch::file_header(&raw).ok_or_else(|| {
                    GitError::Refused("this change can only be discarded as a whole file".into())
                })?;
                let p = patch::build(header, &shown.hunks[*hunk], &sel, Direction::Reverse)
                    .map_err(GitError::Refused)?;
                let Snapshot::File { bytes, mode } = &snap else {
                    return Err(GitError::Stale(rel.display().to_string()));
                };
                Some(Replacement::File {
                    bytes: self.reverted(&rel, bytes, *mode, &p, &dir)?,
                    mode: *mode,
                    exact: true,
                })
            }
        };

        let full = wd.join(&rel);
        let parent = full.parent().unwrap_or(&wd).to_path_buf();
        let new = match &replacement {
            Some(r) => Some(TempPath(r.write_beside(&parent)?)),
            None => None,
        };
        let copy = if matches!(snap, Snapshot::Absent) {
            None
        } else {
            if matches!(snap, Snapshot::Dir(_)) && !recovery::same_filesystem(&dir, &parent) {
                recovery::check_removable(&full, &rel)?;
            }
            Some(move_out(
                plan,
                &dir,
                &wd,
                &rel,
                &full,
                &parent,
                self.repo.git.opts().recovery_cap,
            )?)
        };
        if let Some(new) = new {
            if let Err(e) = recovery::rename_noreplace(&new.0, &full) {
                let earlier = copy
                    .as_ref()
                    .map(|c| format!("; the earlier content is in {}", c.display()))
                    .unwrap_or_default();
                return Err(if e.kind() == std::io::ErrorKind::AlreadyExists {
                    GitError::Refused(format!(
                        "{} was created again while it was being discarded; that new file was \
                         kept{earlier}",
                        rel.display()
                    ))
                } else {
                    GitError::Io(format!("write {}: {e}{earlier}", rel.display()))
                });
            }
            new.release();
        }
        let _ = std::fs::File::open(&parent).and_then(|d| d.sync_all());
        // Refresh the stat info git keeps for the rewritten file, as git's own worktree
        // writes do. Best effort: a held index lock only leaves the entry for git to re-hash.
        let _ = self.run_write(
            &[os("update-index"), os("-q"), os("--refresh")],
            None,
            self.index_limits(),
            &Ctl::default(),
        );
        Ok(Discarded {
            path: rel,
            recovery: copy,
        })
    }

    /// A git call whose output is file content: bounded by the recovery cap, not by the
    /// listing limit; run like terminal git (filters apply, D298(1)).
    fn run_content(
        &self,
        args: &[&OsStr],
        stdin: Option<&[u8]>,
        env: &[(OsString, OsString)],
    ) -> Result<Vec<u8>> {
        let o = self.repo.git.opts();
        let cap = usize::try_from(o.recovery_cap).unwrap_or(usize::MAX);
        let lim = Limits::new(o.user_timeout, cap).with(Profile::Index);
        let ctl = Ctl {
            env: env.to_vec(),
            ..Ctl::default()
        };
        let out = self.run_write(args, stdin, lim, &ctl)?;
        ok_or_failed(args, out)
    }

    /// Blob `oid` as git writes it to the worktree at `rel` (smudge filter, line endings).
    fn worktree_form(&self, rel: &Path, oid: &str) -> Result<Vec<u8>> {
        let mut path_arg = OsString::from("--path=");
        path_arg.push(rel.as_os_str());
        self.run_content(
            &[os("cat-file"), os("--filters"), &path_arg, os(oid)],
            None,
            &[],
        )
    }

    /// What discarding every worktree change of `rel` writes: its index entry.
    fn index_version(&self, rel: &Path) -> Result<Replacement> {
        let (mode, oid) = match self.index_of(rel)?.as_slice() {
            [(mode, oid, 0)] => (mode.clone(), oid.clone()),
            _ => return Err(GitError::Stale(rel.display().to_string())),
        };
        if mode == "120000" {
            let target = self.run_content(&[os("cat-file"), os("blob"), os(&oid)], None, &[])?;
            return Ok(Replacement::Link(target));
        }
        Ok(Replacement::File {
            bytes: self.worktree_form(rel, &oid)?,
            // A new file's mode, narrowed by the umask, as git creates it.
            mode: if mode == "100755" { 0o777 } else { 0o666 },
            exact: false,
        })
    }

    /// `bytes` (the worktree file) with the reverse patch `p` applied, computed in a private
    /// temporary index so the worktree and the real index are not touched.
    fn reverted(
        &self,
        rel: &Path,
        bytes: &[u8],
        mode: u32,
        p: &[u8],
        dir: &Path,
    ) -> Result<Vec<u8>> {
        let mut path_arg = OsString::from("--path=");
        path_arg.push(rel.as_os_str());
        let blob = self.run_content(
            &[os("hash-object"), os("-w"), os("--stdin"), &path_arg],
            Some(bytes),
            &[],
        )?;
        let oid = lossy(blob.trim_ascii());
        let index = TempPath(temp_name(dir, "index"));
        let env = [(
            OsString::from("GIT_INDEX_FILE"),
            index.0.clone().into_os_string(),
        )];
        let kind = if mode & 0o111 != 0 {
            "100755"
        } else {
            "100644"
        };
        let mut rec = format!("{kind} {oid}\t").into_bytes();
        rec.extend_from_slice(as_bytes(rel));
        rec.push(0);
        self.run_content(
            &[os("update-index"), os("-z"), os("--index-info")],
            Some(&rec),
            &env,
        )?;
        let args = [
            os("apply"),
            os("--cached"),
            os("-R"),
            os("--whitespace=nowarn"),
        ];
        let mut check = args.to_vec();
        check.push(os("--check"));
        self.run_content(&check, Some(p), &env)?;
        self.run_content(&args, Some(p), &env)?;
        let ls = self.run_content(
            &[
                os("ls-files"),
                os("-z"),
                os("--stage"),
                os("--"),
                rel.as_os_str(),
            ],
            None,
            &env,
        )?;
        drop(index);
        let new_oid = fields(&ls)
            .first()
            .and_then(|r| lossy(r).split(' ').nth(1).map(str::to_string))
            .ok_or_else(|| GitError::Stale(rel.display().to_string()))?;
        self.worktree_form(rel, &new_oid)
    }

    /// HEAD's full message (for an Amend box), `None` on an unborn branch.
    pub fn last_message(&self) -> Result<Option<String>> {
        match self.repo.head_oid()? {
            Some(oid) => Ok(Some(self.repo.commit(&oid)?.message)),
            None => Ok(None),
        }
    }

    /// Record the staged changes with `message`, sent on stdin (`-F -`), never through an
    /// editor (SEC-12). Hooks and signing run as git decides (accepted T18), without a
    /// terminal, within [`crate::Options::hook_timeout`]; `cancel` kills them. Returns the new
    /// HEAD.
    pub fn commit(&self, message: &str, opts: CommitOptions, cancel: &Cancel) -> Result<String> {
        if message.trim().is_empty() {
            return Err(GitError::Refused("the message is empty".into()));
        }
        if message.contains('\0') {
            return Err(GitError::Refused("the message contains a NUL byte".into()));
        }
        let _g = self.lock.acquire(Some(cancel))?;
        let state = self.state()?;
        if let Some(op @ (Operation::Rebase | Operation::Am)) = state.operation {
            return Err(GitError::Refused(format!(
                "a {} is in progress; finish it in the terminal",
                op.name()
            )));
        }
        if !state.conflicts.is_empty() {
            return Err(GitError::Refused(format!(
                "{state}; resolve the conflicts in the terminal"
            )));
        }
        if opts.amend {
            if let Some(op) = state.operation {
                return Err(GitError::Refused(format!(
                    "a {} is in progress; amend is not possible",
                    op.name()
                )));
            }
            if self.repo.head_oid()?.is_none() {
                return Err(GitError::Refused("there is no commit to amend".into()));
            }
        }
        let mut args = vec![os("commit"), os("--cleanup=whitespace"), os("-F"), os("-")];
        if opts.amend {
            args.push(os("--amend"));
        }
        if opts.signoff {
            args.push(os("--signoff"));
        }
        let o = self.repo.git.opts();
        let lim = Limits::new(o.hook_timeout, o.max_output).with(Profile::Commit);
        let ctl = Ctl {
            cancel: Some(cancel.clone()),
            ..Ctl::default()
        };
        let out = self.run_write(&args, Some(message.as_bytes()), lim, &ctl)?;
        ok_or_failed(&args, out)?;
        self.repo
            .head_oid()?
            .ok_or_else(|| GitError::Io("no HEAD after the commit".into()))
    }
}

/// What a discard puts at the target's path.
enum Replacement {
    /// `mode` is set exactly when `exact`, else it is a new file's mode the umask narrows.
    File {
        bytes: Vec<u8>,
        mode: u32,
        exact: bool,
    },
    /// A symlink to these bytes.
    Link(Vec<u8>),
}

impl Replacement {
    /// Write to a fresh hidden name in `dir` (fsynced); the caller renames it into place.
    fn write_beside(&self, dir: &Path) -> Result<PathBuf> {
        let path = temp_name(dir, "new");
        let r = match self {
            Replacement::File { bytes, mode, exact } => (|| {
                let mut f = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(*mode)
                    .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
                    .open(&path)?;
                if *exact {
                    f.set_permissions(std::fs::Permissions::from_mode(*mode))?;
                }
                f.write_all(bytes)?;
                f.sync_all()
            })(),
            Replacement::Link(target) => {
                std::os::unix::fs::symlink(OsStr::from_bytes(target), &path)
            }
        };
        match r {
            Ok(()) => Ok(path),
            Err(e) => {
                let _ = std::fs::remove_file(&path);
                Err(GitError::Io(format!("write {}: {e}", path.display())))
            }
        }
    }
}

/// A hidden name in `dir` that no other discard (of this process or another) uses.
fn temp_name(dir: &Path, kind: &str) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    dir.join(format!("{LEFTOVER_PREFIX}{pid}-{n}-{nanos}.{kind}"))
}

/// A path this call made; removed on drop unless released.
struct TempPath(PathBuf);

impl TempPath {
    fn release(self) {
        std::mem::forget(self);
    }
}

impl Drop for TempPath {
    fn drop(&mut self) {
        let _ = match std::fs::symlink_metadata(&self.0) {
            Ok(m) if m.is_dir() => std::fs::remove_dir_all(&self.0),
            Ok(_) => std::fs::remove_file(&self.0),
            Err(_) => Ok(()),
        };
    }
}

/// Take the target at `full` out of the work tree into the recovery folder and check it once
/// more against the plan; the kept copy. On the same filesystem it is renamed straight into
/// the folder (one atomic step, so a crash leaves it there). Only across filesystems is it
/// first renamed aside to a hidden `.vr-ide-discard-*.old` name beside it, which nothing ever
/// deletes but a completed copy: a crash or panic before that leaves it for
/// [`leftover_discards`] to show. A target that changed since the plan goes back (`Stale`).
fn move_out(
    plan: &DiscardPlan,
    dir: &Path,
    wd: &Path,
    rel: &Path,
    full: &Path,
    parent: &Path,
    cap: u64,
) -> Result<PathBuf> {
    let stale = || GitError::Stale(rel.display().to_string());
    let unchanged = |p: &Path| {
        recovery::snapshot(p, cap).is_ok_and(|s| recovery::digest(&plan.keys, &s) == plan.digest)
    };
    if let Some(copy) = recovery::move_into(dir, wd, rel, full)? {
        if recovery::fault(Fault::Window) {
            panic!("injected fault after the move into the recovery folder");
        }
        if !unchanged(&copy) {
            if recovery::rename_noreplace(&copy, full).is_ok() {
                return Err(stale());
            }
            recovery::seal(&copy);
            return Err(created_again(rel, &copy));
        }
        recovery::seal(&copy);
        return Ok(copy);
    }
    let aside = temp_name(parent, LEFTOVER_OLD);
    recovery::rename_noreplace(full, &aside)
        .map_err(|e| GitError::Io(format!("move {} aside: {e}", rel.display())))?;
    if recovery::fault(Fault::Window) {
        panic!("injected fault after the move aside");
    }
    if !unchanged(&aside) {
        return Err(put_back(&aside, full, dir, wd, rel, cap).unwrap_or_else(stale));
    }
    match recovery::copy_across(dir, wd, rel, &aside, cap) {
        Ok(c) => Ok(c),
        Err(KeepError::NotKept(e)) => Err(put_back(&aside, full, dir, wd, rel, cap).unwrap_or(e)),
        Err(KeepError::Partial { copy, cause }) => {
            // Never "nothing was discarded" once a copy exists: say what happened and where
            // the full content is. What is left goes back under its own name when it can.
            let left = if recovery::rename_noreplace(&aside, full).is_ok() {
                String::new()
            } else {
                format!("; what is left of it is in {}", aside.display())
            };
            Err(GitError::Io(format!(
                "the discard of {r} stopped after its recovery copy was made ({cause}); {r} \
                 was partly removed{left}; the full earlier content is in {c}",
                r = rel.display(),
                c = copy.display()
            )))
        }
    }
}

fn created_again(rel: &Path, kept: &Path) -> GitError {
    GitError::Refused(format!(
        "{} was created again while it was being discarded; that new file was kept and the \
         earlier content is in {}",
        rel.display(),
        kept.display()
    ))
}

/// Undo a move aside. The target goes back to its path (`None`) unless a program created a
/// file there meanwhile; then that file stays and the moved one is kept in the recovery folder
/// (or, failing that, left under its hidden name), and the error says where.
fn put_back(
    aside: &Path,
    full: &Path,
    dir: &Path,
    wd: &Path,
    rel: &Path,
    cap: u64,
) -> Option<GitError> {
    if recovery::rename_noreplace(aside, full).is_ok() {
        return None;
    }
    let kept = match recovery::keep(dir, wd, rel, aside, cap) {
        Ok(c) | Err(KeepError::Partial { copy: c, .. }) => c,
        Err(KeepError::NotKept(_)) => aside.to_path_buf(),
    };
    Some(created_again(rel, &kept))
}

/// Start of the hidden names a discard uses in the work tree.
const LEFTOVER_PREFIX: &str = ".vr-ide-discard-";
/// Suffix of a moved-aside target (earlier content); `new` is a discard's unused replacement.
const LEFTOVER_OLD: &str = "old";

/// A hidden `.vr-ide-discard-*` file or folder a discard left in the work tree because it was
/// killed (power loss, SIGKILL) between moving a target aside and finishing its recovery copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeftoverDiscard {
    /// Repo-relative path of the leftover.
    pub path: PathBuf,
    /// `true` for a `*.old` leftover: the target's content before the discard, which may be
    /// the only copy; move it back or into the recovery folder. `false` for a `*.new` one,
    /// the unused replacement, safe to delete.
    pub earlier_content: bool,
}

/// Leftovers of interrupted discards anywhere in the work tree, ignored folders included
/// (`git ls-files --others --ignored` with only the leftover name as exclude pattern, so the
/// walk is as long as one `git status` with ignored files). For M7-3 to show at repo open.
pub fn leftover_discards(repo: &Repo) -> Result<Vec<LeftoverDiscard>> {
    let exclude = format!("--exclude={LEFTOVER_PREFIX}*");
    let out = repo.run(
        &[
            os("ls-files"),
            os("-z"),
            os("--others"),
            os("--ignored"),
            os(&exclude),
        ],
        false,
    )?;
    let mut found = std::collections::BTreeSet::new();
    for f in fields(&out) {
        // A leftover folder lists every file in it: report the folder once.
        let mut top = PathBuf::new();
        for c in Path::new(OsStr::from_bytes(f)).components() {
            top.push(c);
            if leftover_kind(c.as_os_str().as_bytes()).is_some() {
                found.insert(top);
                break;
            }
        }
    }
    Ok(found
        .into_iter()
        .map(|path| LeftoverDiscard {
            earlier_content: path.file_name().and_then(|n| leftover_kind(n.as_bytes()))
                == Some(true),
            path,
        })
        .collect())
}

/// `Some(true)` for exactly `.vr-ide-discard-<pid>-<n>-<nanos>.old`, `Some(false)` for the
/// `.new` twin, `None` for any other name, so a user's `.vr-ide-discard-notes.txt` is never listed.
fn leftover_kind(name: &[u8]) -> Option<bool> {
    let rest = name.strip_prefix(LEFTOVER_PREFIX.as_bytes())?;
    let (stem, kind) = if let Some(s) = rest.strip_suffix(b".old") {
        (s, true)
    } else {
        (rest.strip_suffix(b".new")?, false)
    };
    let mut parts = stem.split(|&b| b == b'-');
    for _ in 0..3 {
        let p = parts.next()?;
        if p.is_empty() || !p.iter().all(u8::is_ascii_digit) {
            return None;
        }
    }
    parts.next().is_none().then_some(kind)
}

#[cfg(test)]
mod leftover_name_tests {
    use super::leftover_kind;

    #[test]
    fn only_the_generated_shape_matches() {
        assert_eq!(leftover_kind(b".vr-ide-discard-12-3-456.old"), Some(true));
        assert_eq!(leftover_kind(b".vr-ide-discard-12-3-456.new"), Some(false));
        for bad in [
            &b".vr-ide-discard-notes.txt"[..],
            b".vr-ide-discard-old",
            b".vr-ide-discard-1-2.old",
            b".vr-ide-discard-1-2-3-4.old",
            b".vr-ide-discard-1-2-x.old",
            b".vr-ide-discard-1--3.old",
            b".vr-ide-discard-1-2-3.bak",
            b".vr-ide-discard-1-2-3.old.txt",
        ] {
            assert_eq!(leftover_kind(bad), None, "{}", String::from_utf8_lossy(bad));
        }
    }

    #[test]
    fn name_generator_matches_the_matcher() {
        let n = super::temp_name(std::path::Path::new("d"), "old");
        let name = std::os::unix::ffi::OsStrExt::as_bytes(n.file_name().unwrap());
        assert_eq!(leftover_kind(name), Some(true));
    }
}

#[cfg(test)]
#[path = "discard_faults.rs"]
mod discard_faults;

/// A non-zero exit as [`GitError::Failed`] with git's text (stderr, or stdout when git
/// explains itself there, as `commit` does for "nothing to commit"), each line passed through
/// [`sanitize_line`]: hooks and filters print what they like.
pub(crate) fn ok_or_failed(args: &[&OsStr], out: Output) -> Result<Vec<u8>> {
    if out.code == 0 {
        return Ok(out.stdout);
    }
    let text = if out.stderr.trim().is_empty() {
        lossy(&out.stdout).trim_end().to_string()
    } else {
        out.stderr
    };
    let stderr = text
        .split('\n')
        .map(sanitize_line)
        .collect::<Vec<_>>()
        .join("\n");
    Err(GitError::Failed {
        cmd: args
            .first()
            .map(|a| a.to_string_lossy().into_owned())
            .unwrap_or_default(),
        code: out.code,
        stderr,
    })
}
