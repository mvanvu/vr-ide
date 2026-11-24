//! Putting a prepared, fsynced temp file in place of a user file only if nobody changed the
//! file meanwhile. Shared by `Document::save` and vr-search's project replace.
//!
//! Linux has no "rename if unchanged", so the commit is a small protocol. Names: `T` the
//! target, `P` our private temp name in the same directory, `K` a visible
//! `<name>.vr-ide-conflict-PID-N`. Files: `N` ours (created exclusively at `P`), `X` the file
//! at `T` at the first exchange, `Y` the file at `T` at the second one. Another writer acts on
//! `T` only (write in place, rename over, unlink, create), at any point:
//!
//! | Step | Action | Another writer's change before the step |
//! |---|---|---|
//! | 1 | check `T` against the expected version | seen: `P` deleted (`N` was never at `T`), `Changed` |
//! | 2 | `EXCHANGE(P, T)`: `T`=`N`, `P`=`X` | whatever it left at `T` is `X`, now safe at `P`; `T` gone: `Changed` |
//! | 3 | check `X` by content, stable while read | differs: step 5. Equal: `X` is the expected version |
//! | 4 | unlink `P` (`X`), `Replaced` | lands in `N` at `T` or replaces it: it came after our save |
//! | 5 | `EXCHANGE(P, T)`: `T`=`X`, `P`=`Y` | written into `N`, or renamed over it: that is `Y`, now at `P`. `T` gone: `X` kept at `K` |
//! | 6 | check `Y`: is `N` and holds our bytes, unmodified | not proven: `Y` kept at `K`, `Kept` |
//! | 7 | unlink `P` (`N`), `Changed` | lands in `X` at `T` or replaces it: untouched by us |
//!
//! Only two things are ever unlinked: the expected version after step 3 proved its content,
//! and our own file, either never exposed at `T` or after step 6 proved identity and content.
//! Everything else is kept at `T` or at `K` with an error that ends "kept at <path>".
//!
//! What no rename-based save can see: a writer that opened a file while it was at `T`,
//! without truncating it, and writes through that handle after the file left `T` and passed
//! its check. That write goes to a file nobody can read at `T` any more. And taking the file
//! out atomically has a price: when step 3 finds another version, our bytes are readable at
//! `T` from step 2 until step 5 puts that version back.
//!
//! A crash between steps 2 and 5 leaves our bytes at `T` and the other version under the temp
//! name: stale temp files must never be deleted unseen.
//!
//! Without EXCHANGE (non-Linux, filesystems that answer EINVAL such as some FUSE and network
//! filesystems) step 1 is repeated right before a plain `rename(2)`, and a write landing
//! between that check and the rename is replaced (D63 window).

use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static CONFLICT_SEQ: AtomicU64 = AtomicU64::new(0);

/// What `replace_if_unchanged` did.
#[derive(Debug)]
pub enum Outcome {
    /// `tmp` is now at `target`; the previous file passed the check and was deleted.
    Replaced,
    /// `target` had changed; it was left as it is and `tmp` was deleted.
    Changed,
    /// The file changed again while it was being restored, or could not be checked: a version
    /// that may be someone else's was kept at `at` (never deleted).
    Kept {
        at: PathBuf,
        what: &'static str,
        cause: io::Error,
    },
}

/// The error a caller reports for `Outcome::Kept`; it ends with "kept at <path>".
pub fn kept_error(at: &Path, what: &str, cause: &io::Error) -> io::Error {
    io::Error::new(
        cause.kind(),
        format!(
            "{what} ({cause}); the other version was kept at {}",
            at.display()
        ),
    )
}

/// The caller's knowledge of the two versions involved.
pub struct Checks<'a> {
    /// `target` still holds the expected version (a stamp may short-cut the read).
    pub target_unchanged: &'a mut dyn FnMut(&Path) -> io::Result<bool>,
    /// The file at this path holds the expected version. Asked about the swapped-out file,
    /// whose ctime the swap changed: only the content can tell.
    pub holds_expected: &'a mut dyn FnMut(&Path) -> io::Result<bool>,
    /// The file at this path holds exactly the bytes the caller wrote to `tmp`.
    pub holds_ours: &'a mut dyn FnMut(&Path) -> io::Result<bool>,
}

/// Points between the protocol's steps (module doc); tests place another writer there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    BeforeSwap,
    AfterSwap,
    BeforeDeleteOld,
    BeforeSwapBack,
    AfterSwapBack,
    BeforeDeleteOurs,
}

/// Puts `tmp` (same directory as `target`, fully written and fsynced, not written again by
/// the caller) at `target` only if the file there is the expected version. `swap` is
/// `exchange_files` (a seam for tests), `seam` is called between the steps (pass `|_| {}`).
///
/// On `Err`, `tmp` was never at `target` and still holds the caller's bytes (safe to delete).
pub fn replace_if_unchanged(
    tmp: &Path,
    target: &Path,
    checks: Checks<'_>,
    swap: &mut dyn FnMut(&Path, &Path) -> io::Result<()>,
    seam: &mut dyn FnMut(Step),
) -> io::Result<Outcome> {
    let ours = Mark::of(tmp)?;
    if !(checks.target_unchanged)(target)? {
        remove_unexposed(tmp);
        return Ok(Outcome::Changed);
    }
    seam(Step::BeforeSwap);
    match swap(tmp, target) {
        Ok(()) => {}
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::InvalidInput | io::ErrorKind::Unsupported
            ) =>
        {
            // No EXCHANGE here: re-check and rename (the documented window).
            return if (checks.target_unchanged)(target)? {
                fs::rename(tmp, target)?;
                Ok(Outcome::Replaced)
            } else {
                remove_unexposed(tmp);
                Ok(Outcome::Changed)
            };
        }
        // The file vanished after the check: someone deleted or moved it.
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            remove_unexposed(tmp);
            return Ok(Outcome::Changed);
        }
        Err(e) => return Err(e),
    }
    // Our file has been at `target` from here on: nothing below may fail with `Err`, which
    // would let the caller delete whatever `tmp` names now.
    seam(Step::AfterSwap);
    match proven(tmp, None, checks.holds_expected) {
        Ok(true) => {
            seam(Step::BeforeDeleteOld);
            let _ = fs::remove_file(tmp);
            Ok(Outcome::Replaced)
        }
        Ok(false) => Ok(restore(tmp, target, &ours, checks.holds_ours, swap, seam)),
        Err(cause) => Ok(keep(
            tmp,
            target,
            "the file was replaced but its previous version could not be verified",
            cause,
        )),
    }
}

/// Steps 5 to 7: the swapped-out file is not the expected version, so it goes back to
/// `target`. What comes out of `target` is deleted only when it is proven to be our own file
/// with our own bytes; our file was reachable at `target` meanwhile, so anything else is
/// someone's newer version.
fn restore(
    tmp: &Path,
    target: &Path,
    ours: &Mark,
    holds_ours: &mut dyn FnMut(&Path) -> io::Result<bool>,
    swap: &mut dyn FnMut(&Path, &Path) -> io::Result<()>,
    seam: &mut dyn FnMut(Step),
) -> Outcome {
    seam(Step::BeforeSwapBack);
    if let Err(cause) = swap(tmp, target) {
        return keep(
            tmp,
            target,
            "the file changed on disk and could not be restored",
            cause,
        );
    }
    seam(Step::AfterSwapBack);
    match proven(tmp, Some(ours), holds_ours) {
        Ok(true) => {
            seam(Step::BeforeDeleteOurs);
            let _ = fs::remove_file(tmp);
            Outcome::Changed
        }
        Ok(false) => keep(
            tmp,
            target,
            "the file changed again while it was being restored",
            io::Error::other("a newer version was written during the save"),
        ),
        Err(cause) => keep(
            tmp,
            target,
            "the file changed on disk and what the save took out could not be checked",
            cause,
        ),
    }
}

/// What identifies a file and shows a write to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Mark {
    regular: bool,
    dev: u64,
    ino: u64,
    len: u64,
    mtime: (i64, i64),
    ctime: (i64, i64),
}

impl Mark {
    fn of(p: &Path) -> io::Result<Self> {
        let m = fs::symlink_metadata(p)?;
        Ok(Self {
            regular: m.file_type().is_file(),
            dev: m.dev(),
            ino: m.ino(),
            len: m.len(),
            mtime: (m.mtime(), m.mtime_nsec()),
            ctime: (m.ctime(), m.ctime_nsec()),
        })
    }

    /// Same file with no write since `earlier`. A rename changes ctime, so it is left out;
    /// mtime also shows a truncating open that left the length as it was.
    fn unwritten_since(&self, earlier: &Mark) -> bool {
        (self.dev, self.ino, self.len, self.mtime)
            == (earlier.dev, earlier.ino, earlier.len, earlier.mtime)
    }
}

/// True when the file at `path` is a regular file (a symlink put there is not followed), is
/// `same_as` unwritten when given, passes `content`, and did not change while it was read.
fn proven(
    path: &Path,
    same_as: Option<&Mark>,
    content: &mut dyn FnMut(&Path) -> io::Result<bool>,
) -> io::Result<bool> {
    let before = Mark::of(path)?;
    if !before.regular || same_as.is_some_and(|m| !before.unwritten_since(m)) {
        return Ok(false);
    }
    Ok(content(path)? && Mark::of(path)? == before)
}

/// Puts `tmp` at `target` only if nothing is there; `false` (and `tmp` deleted) when
/// something is. Uses `renameat2(RENAME_NOREPLACE)`, then a hard link (also atomic
/// no-replace) where that is unsupported; on filesystems with neither (vfat, exFAT) it checks
/// and renames, with the D32 window.
pub fn create_if_absent(tmp: &Path, target: &Path) -> io::Result<bool> {
    let refused = |tmp: &Path| {
        remove_unexposed(tmp);
        Ok(false)
    };
    match rename_noreplace(tmp, target) {
        Ok(()) => return Ok(true),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return refused(tmp),
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::InvalidInput | io::ErrorKind::Unsupported
            ) => {}
        Err(e) => return Err(e),
    }
    match fs::hard_link(tmp, target) {
        Ok(()) => {
            // Only the temp name goes; the file stays at `target`.
            let _ = fs::remove_file(tmp);
            return Ok(true);
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return refused(tmp),
        Err(_) => {}
    }
    if fs::symlink_metadata(target).is_ok() {
        return refused(tmp);
    }
    fs::rename(tmp, target)?;
    Ok(true)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn exchange_files(a: &Path, b: &Path) -> io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    renameat_with(CWD, a, CWD, b, RenameFlags::EXCHANGE).map_err(io::Error::from)
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub fn exchange_files(_a: &Path, _b: &Path) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn rename_noreplace(a: &Path, b: &Path) -> io::Result<()> {
    use rustix::fs::{CWD, RenameFlags, renameat_with};
    renameat_with(CWD, a, CWD, b, RenameFlags::NOREPLACE).map_err(io::Error::from)
}

#[cfg(not(any(target_os = "linux", target_os = "android")))]
fn rename_noreplace(_a: &Path, _b: &Path) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// Deletes `tmp` while it has never been at the target: only the caller's own private file
/// was ever reachable under that name.
fn remove_unexposed(tmp: &Path) {
    let _ = fs::remove_file(tmp);
}

fn keep(tmp: &Path, target: &Path, what: &'static str, cause: io::Error) -> Outcome {
    Outcome::Kept {
        at: keep_conflict(tmp, target),
        what,
        cause,
    }
}

/// Moves `tmp` to a visible `<name>.vr-ide-conflict-PID-N` next to `target` without ever
/// replacing an existing file (no-replace rename, else hard link + unlink of the temp name),
/// and returns where it now is. If that fails, `tmp` stays where it is and its own path is
/// returned.
fn keep_conflict(tmp: &Path, target: &Path) -> PathBuf {
    let Some(dir) = target.parent() else {
        return tmp.to_path_buf();
    };
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Keep the name under NAME_MAX (255 bytes) with room for the suffix.
    let mut cut = name.len().min(200);
    while !name.is_char_boundary(cut) {
        cut -= 1;
    }
    for _ in 0..100 {
        let kept = dir.join(format!(
            "{}.vr-ide-conflict-{}-{}",
            &name[..cut],
            std::process::id(),
            CONFLICT_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        match rename_noreplace(tmp, &kept) {
            Ok(()) => return kept,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => {}
        }
        match fs::hard_link(tmp, &kept) {
            Ok(()) => {
                let _ = fs::remove_file(tmp);
                return kept;
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(_) => break,
        }
    }
    tmp.to_path_buf()
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fs::{self, OpenOptions};
    use std::io::Write;

    use super::*;

    const OURS: &str = "new\n";

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    fn setup_with(old: &str, ours: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.txt");
        fs::write(&target, old).unwrap();
        let tmp = dir.path().join(".vr-ide-save-test.tmp");
        fs::write(&tmp, ours).unwrap();
        (dir, target, tmp)
    }

    fn setup(old: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
        setup_with(old, OURS)
    }

    /// Runs the protocol expecting `old` at the target, with `ours` in the temp file.
    fn run_with(
        tmp: &Path,
        target: &Path,
        old: &'static str,
        ours: &'static str,
        swap: &mut dyn FnMut(&Path, &Path) -> io::Result<()>,
        seam: &mut dyn FnMut(Step),
    ) -> Outcome {
        let mut expected = |p: &Path| Ok(fs::read(p)? == old.as_bytes());
        let mut expected2 = |p: &Path| Ok(fs::read(p)? == old.as_bytes());
        let mut is_ours = |p: &Path| Ok(fs::read(p)? == ours.as_bytes());
        replace_if_unchanged(
            tmp,
            target,
            Checks {
                target_unchanged: &mut expected,
                holds_expected: &mut expected2,
                holds_ours: &mut is_ours,
            },
            swap,
            seam,
        )
        .unwrap()
    }

    /// Another writer and the step before which it acts.
    type Writer<'a> = (Step, &'a mut dyn FnMut(&Path));

    /// The real exchange, with `writer` acting on the target at each listed step.
    fn run(tmp: &Path, target: &Path, old: &'static str, writers: &mut [Writer<'_>]) -> Outcome {
        run_with(tmp, target, old, OURS, &mut exchange_files, &mut |step| {
            for (at, writer) in writers.iter_mut() {
                if *at == step {
                    writer(target);
                }
            }
        })
    }

    fn write_in_place(text: &'static str) -> impl FnMut(&Path) {
        move |p| fs::write(p, text).unwrap()
    }

    fn rename_over(text: &'static str) -> impl FnMut(&Path) {
        move |p| {
            let other = p.with_file_name("agent.tmp");
            fs::write(&other, text).unwrap();
            fs::rename(&other, p).unwrap();
        }
    }

    fn unlink(p: &Path) {
        fs::remove_file(p).unwrap()
    }

    fn kept_at(out: Outcome, dir: &Path) -> PathBuf {
        let Outcome::Kept { at, .. } = out else {
            panic!("expected Kept, got {out:?}; dir {:?}", names(dir));
        };
        let name = at.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("a.txt.vr-ide-conflict-"), "{name}");
        at
    }

    #[test]
    fn replaces_an_unchanged_file() {
        let (dir, target, tmp) = setup("old\n");
        let out = run(&tmp, &target, "old\n", &mut []);
        assert!(matches!(out, Outcome::Replaced), "{out:?}");
        assert_eq!(fs::read_to_string(&target).unwrap(), "new\n");
        assert_eq!(names(dir.path()), ["a.txt"]);
    }

    #[test]
    fn write_between_check_and_swap_is_swapped_back() {
        for exchange in [true, false] {
            let (dir, target, tmp) = setup("old\n");
            let mut swap = |a: &Path, b: &Path| {
                if exchange {
                    exchange_files(a, b)
                } else {
                    Err(io::Error::from(io::ErrorKind::InvalidInput))
                }
            };
            let at = target.clone();
            // With exchange the write lands after the last check; without it, before the
            // fallback's re-check (the documented window is between that re-check and rename).
            let out = run_with(&tmp, &target, "old\n", OURS, &mut swap, &mut |step| {
                if step == Step::BeforeSwap {
                    fs::write(&at, "agent\n").unwrap()
                }
            });
            assert!(matches!(out, Outcome::Changed), "{exchange}: {out:?}");
            assert_eq!(
                fs::read_to_string(&target).unwrap(),
                "agent\n",
                "{exchange}"
            );
            assert_eq!(names(dir.path()), ["a.txt"], "{exchange}");
        }
    }

    #[test]
    fn version_renamed_in_between_check_and_swap_is_swapped_back() {
        let (dir, target, tmp) = setup("old\n");
        let out = run(
            &tmp,
            &target,
            "old\n",
            &mut [(Step::BeforeSwap, &mut rename_over("agent\n"))],
        );
        assert!(matches!(out, Outcome::Changed), "{out:?}");
        assert_eq!(fs::read_to_string(&target).unwrap(), "agent\n");
        assert_eq!(names(dir.path()), ["a.txt"]);
    }

    #[test]
    fn file_deleted_between_check_and_swap_stays_deleted() {
        let (dir, target, tmp) = setup("old\n");
        let out = run(
            &tmp,
            &target,
            "old\n",
            &mut [(Step::BeforeSwap, &mut unlink)],
        );
        assert!(matches!(out, Outcome::Changed), "{out:?}");
        assert!(names(dir.path()).is_empty(), "{:?}", names(dir.path()));
    }

    #[test]
    fn symlink_put_in_place_before_swap_is_restored() {
        let (dir, target, tmp) = setup("old\n");
        // The link points to a file with the expected content: reading through it would
        // pass the check and the link itself would be deleted.
        let out = run(
            &tmp,
            &target,
            "old\n",
            &mut [(Step::BeforeSwap, &mut |p: &Path| {
                let elsewhere = p.with_file_name("elsewhere.txt");
                fs::rename(p, &elsewhere).unwrap();
                std::os::unix::fs::symlink(&elsewhere, p).unwrap();
            })],
        );
        assert!(matches!(out, Outcome::Changed), "{out:?}");
        assert!(fs::symlink_metadata(&target).unwrap().is_symlink());
        assert_eq!(fs::read_to_string(&target).unwrap(), "old\n");
        assert_eq!(names(dir.path()), ["a.txt", "elsewhere.txt"]);
    }

    #[test]
    fn write_after_our_file_is_in_place_wins() {
        for step in [Step::AfterSwap, Step::BeforeDeleteOld] {
            for rename in [false, true] {
                let (dir, target, tmp) = setup("old\n");
                let mut in_place = write_in_place("agent\n");
                let mut renamed = rename_over("agent\n");
                let writer: &mut dyn FnMut(&Path) =
                    if rename { &mut renamed } else { &mut in_place };
                let out = run(&tmp, &target, "old\n", &mut [(step, writer)]);
                assert!(matches!(out, Outcome::Replaced), "{step:?} {out:?}");
                assert_eq!(fs::read_to_string(&target).unwrap(), "agent\n", "{step:?}");
                assert_eq!(names(dir.path()), ["a.txt"], "{step:?}");
            }
        }
    }

    #[test]
    fn in_place_write_between_swap_and_swap_back_is_kept() {
        let (dir, target, tmp) = setup("old\n");
        let out = run(
            &tmp,
            &target,
            "old\n",
            &mut [
                (Step::BeforeSwap, &mut write_in_place("agent1\n")),
                // The writer's next version is written by path while our file sits there.
                (Step::BeforeSwapBack, &mut write_in_place("agent2\n")),
            ],
        );
        let at = kept_at(out, dir.path());
        assert_eq!(fs::read_to_string(&target).unwrap(), "agent1\n");
        assert_eq!(fs::read_to_string(&at).unwrap(), "agent2\n");
        assert_eq!(names(dir.path()).len(), 2);
    }

    #[test]
    fn version_put_in_place_during_swap_back_is_kept() {
        let (dir, target, tmp) = setup("old\n");
        let out = run(
            &tmp,
            &target,
            "old\n",
            &mut [
                (Step::BeforeSwap, &mut write_in_place("agent1\n")),
                (Step::BeforeSwapBack, &mut rename_over("agent2\n")),
            ],
        );
        let at = kept_at(out, dir.path());
        assert_eq!(fs::read_to_string(&target).unwrap(), "agent1\n");
        assert_eq!(fs::read_to_string(&at).unwrap(), "agent2\n");
        assert_eq!(names(dir.path()).len(), 2);
        let msg = kept_error(&at, "changed twice", &io::Error::other("x")).to_string();
        assert!(
            msg.ends_with(&format!(" kept at {}", at.display())),
            "{msg}"
        );
    }

    #[test]
    fn version_with_our_bytes_renamed_in_during_swap_back_is_kept() {
        let (dir, target, tmp) = setup("old\n");
        // Equal content is no proof that the file is ours.
        let out = run(
            &tmp,
            &target,
            "old\n",
            &mut [
                (Step::BeforeSwap, &mut write_in_place("agent1\n")),
                (Step::BeforeSwapBack, &mut rename_over(OURS)),
            ],
        );
        let at = kept_at(out, dir.path());
        assert_eq!(fs::read_to_string(&target).unwrap(), "agent1\n");
        assert_eq!(fs::read_to_string(&at).unwrap(), OURS);
    }

    #[test]
    fn file_deleted_between_swap_and_swap_back_keeps_the_other_version() {
        let (dir, target, tmp) = setup("old\n");
        let out = run(
            &tmp,
            &target,
            "old\n",
            &mut [
                (Step::BeforeSwap, &mut write_in_place("agent1\n")),
                (Step::BeforeSwapBack, &mut unlink),
            ],
        );
        let at = kept_at(out, dir.path());
        assert_eq!(fs::read_to_string(&at).unwrap(), "agent1\n");
        assert_eq!(names(dir.path()).len(), 1);
    }

    #[test]
    fn write_after_swap_back_stays_at_the_target() {
        for step in [Step::AfterSwapBack, Step::BeforeDeleteOurs] {
            for rename in [false, true] {
                let (dir, target, tmp) = setup("old\n");
                let mut in_place = write_in_place("agent2\n");
                let mut renamed = rename_over("agent2\n");
                let writer: &mut dyn FnMut(&Path) =
                    if rename { &mut renamed } else { &mut in_place };
                let out = run(
                    &tmp,
                    &target,
                    "old\n",
                    &mut [
                        (Step::BeforeSwap, &mut write_in_place("agent1\n")),
                        (step, writer),
                    ],
                );
                assert!(matches!(out, Outcome::Changed), "{step:?} {out:?}");
                assert_eq!(fs::read_to_string(&target).unwrap(), "agent2\n", "{step:?}");
                assert_eq!(names(dir.path()), ["a.txt"], "{step:?}");
            }
        }
    }

    #[test]
    fn file_opened_for_writing_while_ours_was_in_place_is_kept() {
        // The writer opens (and truncates) our file by path, and its bytes land only after
        // the file went back to the temp name. With empty content of ours the truncation
        // leaves the length as it was.
        for ours in ["", OURS] {
            let (dir, target, tmp) = setup_with("old\n", ours);
            let handle = RefCell::new(None);
            let out = run_with(
                &tmp,
                &target,
                "old\n",
                ours,
                &mut exchange_files,
                &mut |step| match step {
                    Step::BeforeSwap => fs::write(&target, "agent1\n").unwrap(),
                    Step::BeforeSwapBack => {
                        // Past the timestamp tick of a filesystem with a coarse clock.
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        let f = OpenOptions::new()
                            .write(true)
                            .truncate(true)
                            .open(&target)
                            .unwrap();
                        *handle.borrow_mut() = Some(f);
                    }
                    _ => {}
                },
            );
            let at = kept_at(out, dir.path());
            handle
                .borrow_mut()
                .take()
                .unwrap()
                .write_all(b"agent2\n")
                .unwrap();
            assert_eq!(fs::read_to_string(&target).unwrap(), "agent1\n", "{ours:?}");
            assert_eq!(fs::read_to_string(&at).unwrap(), "agent2\n", "{ours:?}");
            assert_eq!(names(dir.path()).len(), 2, "{ours:?}");
        }
    }

    #[test]
    fn same_length_write_with_restored_mtime_is_kept() {
        // Neither the length nor the mtime shows this write; only the content does.
        let (dir, target, tmp) = setup("old\n");
        let out = run(
            &tmp,
            &target,
            "old\n",
            &mut [
                (Step::BeforeSwap, &mut write_in_place("agent1\n")),
                (Step::BeforeSwapBack, &mut |p: &Path| {
                    let mtime = fs::metadata(p).unwrap().modified().unwrap();
                    let mut f = OpenOptions::new().write(true).open(p).unwrap();
                    f.write_all(b"NEW\n").unwrap();
                    f.set_modified(mtime).unwrap();
                }),
            ],
        );
        let at = kept_at(out, dir.path());
        assert_eq!(fs::read_to_string(&target).unwrap(), "agent1\n");
        assert_eq!(fs::read_to_string(&at).unwrap(), "NEW\n");
    }

    #[test]
    fn write_during_the_check_of_the_swapped_out_file_restores_it() {
        let (dir, target, tmp) = setup("old\n");
        let writer = RefCell::new(
            OpenOptions::new()
                .append(true)
                .open(&target)
                .map(Some)
                .unwrap(),
        );
        let mut expected = |p: &Path| Ok(fs::read(p)? == b"old\n");
        // A handle opened before the swap appends right after the content was read.
        let mut expected_then_append = |p: &Path| {
            let same = fs::read(p)? == b"old\n";
            if let Some(mut f) = writer.borrow_mut().take() {
                f.write_all(b"appended\n")?;
            }
            Ok(same)
        };
        let mut is_ours = |p: &Path| Ok(fs::read(p)? == OURS.as_bytes());
        let out = replace_if_unchanged(
            &tmp,
            &target,
            Checks {
                target_unchanged: &mut expected,
                holds_expected: &mut expected_then_append,
                holds_ours: &mut is_ours,
            },
            &mut exchange_files,
            &mut |_| {},
        )
        .unwrap();
        assert!(matches!(out, Outcome::Changed), "{out:?}");
        assert_eq!(fs::read_to_string(&target).unwrap(), "old\nappended\n");
        assert_eq!(names(dir.path()), ["a.txt"]);
    }

    #[test]
    fn create_if_absent_never_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.txt");
        let tmp = dir.path().join(".vr-ide-save-test.tmp");
        fs::write(&tmp, "new\n").unwrap();
        assert!(create_if_absent(&tmp, &target).unwrap());
        assert_eq!(fs::read_to_string(&target).unwrap(), "new\n");
        fs::write(&tmp, "second\n").unwrap();
        assert!(!create_if_absent(&tmp, &target).unwrap());
        assert_eq!(fs::read_to_string(&target).unwrap(), "new\n");
        assert_eq!(names(dir.path()), ["a.txt"]);
    }
}
