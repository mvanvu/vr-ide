//! Test helpers (feature `test-util`): a concurrent writer that races a write path, and the
//! rules a write to a user file must satisfy against it.
//!
//! Usage: create the file, open the document / build the preview, `RacingWriter::start`, wait
//! a swept delay (`start_after`), run the operation through `timed`, `stop` the writer, read the file and
//! call `assert_no_lost_write` (one path) or `assert_versions_survive` (a move).

use std::fs::{self, OpenOptions};
use std::io::{self, Read, Seek, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// How the writer replaces the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriterMode {
    /// Truncate and write the existing file (never creates it: a vanished file ends the writer).
    InPlace,
    /// Write a sibling temp file and rename it over the path.
    TempRename,
}

/// One version the writer wrote completely.
#[derive(Clone, Debug)]
pub struct WriterVersion {
    pub bytes: Vec<u8>,
    /// How this version was written (named in the report of an unaccounted version).
    pub mode: WriterMode,
    /// Taken before the write started.
    pub begun: Instant,
    /// False when an in-place write went to a file that had already been replaced or moved
    /// away from the path (the writer held the old file open): nobody could ever read it
    /// there, so replacing it is not a lost write.
    pub visible: bool,
    /// What the path held just before this write (None if unreadable): lets the assertion see
    /// that an operation's bytes replaced the previous version even when this one wins later.
    pub saw_before: Option<Vec<u8>>,
    /// Exactly what this write replaced: the bytes of the file it overwrote (read through the
    /// handle for an in-place write, taken out by an exchange for a rename write); empty when
    /// the path held no file. None when the writer could not tell: a rename write where
    /// RENAME_EXCHANGE is unsupported or refused (non-Linux, some FUSE and network
    /// filesystems). A later version whose `replaced` equals this one's bytes proves the
    /// writer itself overwrote it.
    pub replaced: Option<Vec<u8>>,
}

/// Unique, recognisable, sequence-numbered content; no two versions are equal.
pub fn version_bytes(seq: usize) -> Vec<u8> {
    let mut s = format!("RACING-WRITER seq={seq:06}\n");
    s.push_str(&"~".repeat(2048));
    s.push('\n');
    s.into_bytes()
}

/// True for bytes produced by `version_bytes`.
pub fn is_writer_content(bytes: &[u8]) -> bool {
    bytes.starts_with(b"RACING-WRITER seq=")
}

/// A background thread that keeps rewriting one file until it has written `max_versions`
/// versions (so a run is bounded) or `stop` is called.
pub struct RacingWriter {
    stop: Arc<AtomicBool>,
    handle: JoinHandle<Vec<WriterVersion>>,
}

impl RacingWriter {
    pub fn start(
        path: &Path,
        mode: WriterMode,
        interval: Duration,
        max_versions: usize,
    ) -> RacingWriter {
        Self::start_after(Duration::ZERO, path, mode, interval, max_versions)
    }

    /// Like `start`, but the first write happens after `delay`; sweep it to place the first
    /// write at different points of an operation that starts right away.
    pub fn start_after(
        delay: Duration,
        path: &Path,
        mode: WriterMode,
        interval: Duration,
        max_versions: usize,
    ) -> RacingWriter {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let path = path.to_path_buf();
        let handle = thread::spawn(move || {
            thread::sleep(delay);
            let mut versions = Vec::new();
            for seq in 1..=max_versions {
                if flag.load(Ordering::Relaxed) {
                    break;
                }
                let bytes = version_bytes(seq);
                let begun = Instant::now();
                match write_once(&path, mode, seq, &bytes, || {}) {
                    Ok(Landed {
                        visible,
                        saw_before,
                        replaced,
                    }) => versions.push(WriterVersion {
                        bytes,
                        begun,
                        mode,
                        visible,
                        saw_before,
                        replaced,
                    }),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => break,
                    Err(e) => panic!("racing writer failed: {e}"),
                }
                thread::sleep(interval);
            }
            versions
        });
        RacingWriter { stop, handle }
    }

    /// Stops the writer, waits for it and returns every version it wrote, oldest first.
    pub fn stop(self) -> Vec<WriterVersion> {
        self.stop.store(true, Ordering::Relaxed);
        self.handle.join().expect("racing writer thread panicked")
    }
}

/// What a write learned about itself.
struct Landed {
    visible: bool,
    saw_before: Option<Vec<u8>>,
    replaced: Option<Vec<u8>>,
}

/// Returns (visible, what the path held before). `between` runs after the file is opened /
/// staged and before the bytes land: a seam for tests of the writer itself.
fn write_once(
    path: &Path,
    mode: WriterMode,
    seq: usize,
    bytes: &[u8],
    between: impl FnOnce(),
) -> io::Result<Landed> {
    write_once_with(
        path,
        mode,
        seq,
        bytes,
        between,
        crate::safe_replace::exchange_files,
    )
}

/// `write_once` with the exchange as a seam (tests of the writer's error handling).
fn write_once_with(
    path: &Path,
    mode: WriterMode,
    seq: usize,
    bytes: &[u8],
    between: impl FnOnce(),
    exchange: fn(&Path, &Path) -> io::Result<()>,
) -> io::Result<Landed> {
    let saw_before = fs::read(path).ok();
    match mode {
        WriterMode::InPlace => {
            // Opened without truncating so the old bytes can be read through the handle: that is
            // exactly the file this write replaces, wherever the path points by then.
            let mut f = OpenOptions::new().read(true).write(true).open(path)?;
            let mut replaced = Vec::new();
            f.read_to_end(&mut replaced)?;
            // Visible means this handle was the file at the path when the write began; a later
            // replace of the path must not hide the write from the assertion.
            let here = fs::metadata(path).map(|m| m.ino()).ok();
            let opened = f.metadata()?.ino();
            between();
            f.set_len(0)?;
            f.rewind()?;
            f.write_all(bytes)?;
            // A write that went into a file replaced meanwhile can never be read at the path.
            let after = fs::metadata(path).map(|m| m.ino()).ok();
            let visible = here == Some(opened) && after == Some(opened);
            Ok(Landed {
                visible,
                replaced: Some(replaced),
                saw_before,
            })
        }
        WriterMode::TempRename => {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let tmp: PathBuf = path.with_file_name(format!(".{name}.racing-{seq}"));
            fs::write(&tmp, bytes)?;
            between();
            // Exchange, then read what came out: exactly the file this write replaced. When the
            // path is gone a plain rename creates it and nothing was replaced. Without exchange
            // support a plain rename replaces a file it cannot read any more: unknown. Any other
            // failure ends the writer (the test fails) rather than leave `replaced` unknown.
            let replaced = match exchange(&tmp, path) {
                Ok(()) => {
                    let old = fs::read(&tmp)?;
                    fs::remove_file(&tmp)?;
                    Some(old)
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    fs::rename(&tmp, path)?;
                    Some(Vec::new())
                }
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::Unsupported | io::ErrorKind::InvalidInput
                    ) =>
                {
                    fs::rename(&tmp, path)?;
                    None
                }
                Err(e) => {
                    let _ = fs::remove_file(&tmp);
                    return Err(e);
                }
            };
            Ok(Landed {
                visible: true,
                replaced,
                saw_before,
            })
        }
    }
}

/// When an operation ran and whether it reported success.
#[derive(Clone, Copy, Debug)]
pub struct OpRun {
    pub ok: bool,
    pub finished: Instant,
}

/// Runs `op` and records its outcome; `is_ok` says whether it reported success.
pub fn timed<T>(op: impl FnOnce() -> T, is_ok: impl FnOnce(&T) -> bool) -> (T, OpRun) {
    let result = op();
    let run = OpRun {
        ok: is_ok(&result),
        finished: Instant::now(),
    };
    (result, run)
}

/// The no-lost-write rule for an operation that replaces one file, run against a writer
/// that started after the operation's baseline was taken (the document was opened, the
/// preview made). `final_bytes` is the file after the writer stopped.
///
/// * The file is never torn: it holds `ours`, one writer version or `initial`.
/// * An operation that reported an error never leaves `ours` in the file.
/// * An operation that reported success and left `ours` replaced every version the writer
///   had begun before it returned, so there must be none: with a writer that begins
///   rewriting before the operation ends, success may only be followed by the writer's own
///   later write winning. A version that landed after the freshness check and was replaced
///   by `ours` is exactly the lost write.
/// * The same holds when a later version wins: a version that saw `ours` in the file before it
///   wrote shows `ours` replaced the version before it.
pub fn assert_no_lost_write(
    initial: &[u8],
    final_bytes: &[u8],
    ours: &[u8],
    versions: &[WriterVersion],
    run: OpRun,
) {
    no_lost_write(initial, final_bytes, ours, versions, run, None);
}

/// `assert_no_lost_write` for an operation that takes the file out of the path with an atomic
/// exchange and puts it back when it is not the version it expected (`safe_replace`). While
/// such a run checks the file it took out, `ours` is readable at the path, so on a run that
/// reported an error the writer may have seen `ours` there. In exchange, such a run must
/// account for every version: one that was at the path, or whose writer saw `ours`, is still
/// in the file, or in a conflict copy the error names (`kept`: the bytes of those copies), or
/// was overwritten by the writer's own later version. The final content and every rule for a
/// run that reported success are unchanged.
pub fn assert_no_lost_write_swapping(
    initial: &[u8],
    final_bytes: &[u8],
    ours: &[u8],
    versions: &[WriterVersion],
    run: OpRun,
    kept: &[Vec<u8>],
) {
    no_lost_write(initial, final_bytes, ours, versions, run, Some(kept));
}

fn no_lost_write(
    initial: &[u8],
    final_bytes: &[u8],
    ours: &[u8],
    versions: &[WriterVersion],
    run: OpRun,
    put_back_on_error: Option<&[Vec<u8>]>,
) {
    whole_content(
        initial,
        final_bytes,
        ours,
        versions,
        run,
        put_back_on_error.is_some(),
    );
    if let Some(kept) = put_back_on_error
        && !run.ok
    {
        assert_versions_accounted_for(final_bytes, ours, versions, kept);
        return;
    }
    let is_version = versions.iter().any(|v| v.bytes == final_bytes);
    // A later version that found `ours` in the file shows `ours` replaced the version before it,
    // even though that later version is the final content.
    let overtaken = versions
        .windows(2)
        .filter(|w| w[0].visible && w[1].saw_before.as_deref() == Some(ours))
        .count();
    assert!(
        overtaken == 0,
        "lost write: {overtaken} writer version(s) were replaced by our bytes and a later \
         writer version overwrote ours"
    );
    if final_bytes == ours && !is_version {
        let replaced = versions
            .iter()
            .filter(|v| v.visible && v.begun < run.finished)
            .count();
        assert!(
            replaced == 0,
            "lost write: {replaced} writer version(s) begun before the operation finished \
             were replaced by our bytes and the operation reported success"
        );
    }
}

/// A refused run that had `ours` at the path for a moment: the writer's sight of `ours` no
/// longer shows a loss by itself, so each version is traced instead. A version is accounted
/// for only when it is the final content, in a conflict copy, or exactly what a later write
/// replaced (`replaced`): the writer overwriting it itself is proven, never inferred. Only a
/// later write that landed at the path (`visible`) proves anything. A version followed by a
/// visible write that could not tell what it replaced (`replaced: None`, a filesystem without
/// RENAME_EXCHANGE) cannot be traced and is not checked: on such a filesystem this part of
/// the rule is weaker (reported on stderr); the final-content rules still apply. Strict
/// racing tests call `assert_exchange_proofs_known`, which fails a run with unknown
/// `replaced` where RENAME_EXCHANGE works.
fn assert_versions_accounted_for(
    final_bytes: &[u8],
    ours: &[u8],
    versions: &[WriterVersion],
    kept: &[Vec<u8>],
) {
    let saw_ours = |v: &WriterVersion| v.saw_before.as_deref() == Some(ours);
    let mut unaccounted = Vec::new();
    for (i, v) in versions.iter().enumerate() {
        if !v.visible && !saw_ours(v) {
            continue;
        }
        // Only a write into a file that is readable proves what it replaced: one that went
        // into a file deleted from the path read that file, not the path's version. A write
        // the writer saw off the path still counts when its file came back: its bytes are the
        // final content, in a conflict copy, or what a later write read at the path (version
        // bytes are unique, so they name that file).
        let readable = |w: &&WriterVersion| {
            w.visible
                || w.bytes == final_bytes
                || kept.contains(&w.bytes)
                || versions
                    .iter()
                    .any(|x| x.replaced.as_deref() == Some(&w.bytes[..]))
        };
        let mut later = versions[i + 1..].iter().filter(readable);
        let proven = |w: &&WriterVersion| w.replaced.as_deref() == Some(&v.bytes[..]);
        let untraceable = |w: &&WriterVersion| w.replaced.is_none();
        let survives =
            v.bytes == final_bytes || kept.contains(&v.bytes) || later.clone().any(|w| proven(&w));
        if !survives && later.any(|w| untraceable(&w)) {
            eprintln!(
                "WARNING: writer version {} of {} not checked: a later write could not tell \
                 what it replaced (no RENAME_EXCHANGE)",
                i + 1,
                versions.len()
            );
            continue;
        }
        if !survives {
            unaccounted.push(format!(
                "version {} of {} [mode={:?} visible={} saw_ours={} replaced_is_ours={}]",
                i + 1,
                versions.len(),
                v.mode,
                v.visible,
                saw_ours(v),
                v.replaced.as_deref() == Some(ours)
            ));
        }
    }
    // Known rare false positive (D75(2)/D76(3)): mode=InPlace, visible=false, saw_ours=true,
    // replaced_is_ours=true. Anything else is a real loss.
    assert!(
        unaccounted.is_empty(),
        "lost write: writer versions in no file after a run that reported an error, and the \
         writer did not overwrite them: {}",
        unaccounted.join("; ")
    );
}

/// Whether RENAME_EXCHANGE works in `dir` (probed with two scratch files that are removed).
pub fn exchange_supported(dir: &Path) -> bool {
    let a = dir.join(format!(".racing-probe-a-{}", std::process::id()));
    let b = dir.join(format!(".racing-probe-b-{}", std::process::id()));
    let ok = fs::write(&a, b"a").is_ok()
        && fs::write(&b, b"b").is_ok()
        && crate::safe_replace::exchange_files(&a, &b).is_ok();
    let _ = fs::remove_file(&a);
    let _ = fs::remove_file(&b);
    ok
}

/// Call after `RacingWriter::stop` in a strict racing test, with the directory of the raced
/// file. A version that could not tell what it replaced (`replaced: None`) leaves the
/// versions before it untraceable, which weakens `assert_no_lost_write_swapping`. Where
/// RENAME_EXCHANGE works that must never happen, so it fails the test; where it does not
/// work the weaker run is reported on stderr, never passed silently.
pub fn assert_exchange_proofs_known(dir: &Path, versions: &[WriterVersion]) {
    let unknown = versions.iter().filter(|v| v.replaced.is_none()).count();
    if unknown == 0 {
        return;
    }
    assert!(
        !exchange_supported(dir),
        "oracle weakened: {unknown} writer version(s) could not tell what they replaced \
         although RENAME_EXCHANGE works in {}",
        dir.display()
    );
    eprintln!(
        "WARNING: strict racing oracle weakened: RENAME_EXCHANGE is unsupported in {}; \
         {unknown} writer version(s) cannot be traced",
        dir.display()
    );
}

/// The first two bullets of `assert_no_lost_write` only: for a write path with a documented
/// window between its last freshness check and the replace, where the third rule cannot hold.
pub fn assert_whole_content(
    initial: &[u8],
    final_bytes: &[u8],
    ours: &[u8],
    versions: &[WriterVersion],
    run: OpRun,
) {
    whole_content(initial, final_bytes, ours, versions, run, false);
}

fn whole_content(
    initial: &[u8],
    final_bytes: &[u8],
    ours: &[u8],
    versions: &[WriterVersion],
    run: OpRun,
    put_back_on_error: bool,
) {
    let is_ours = final_bytes == ours;
    let is_version = versions.iter().any(|v| v.bytes == final_bytes);
    assert!(
        is_ours || is_version || final_bytes == initial,
        "torn or foreign content ({} bytes, starts {:?})",
        final_bytes.len(),
        String::from_utf8_lossy(&final_bytes[..final_bytes.len().min(40)])
    );
    let saw_ours = versions
        .iter()
        .any(|v| v.saw_before.as_deref() == Some(ours));
    assert!(
        run.ok || !saw_ours || put_back_on_error,
        "operation reported an error but our bytes were in the file"
    );
    if is_ours && !is_version {
        assert!(
            run.ok,
            "operation reported an error but our bytes are in the file"
        );
    }
}

/// The rule for a rename or move of a file being rewritten: nothing the writer wrote
/// completely is lost. Every file found afterwards (at the old and the new path) holds the
/// initial bytes or a writer version, and the newest version written is among them.
pub fn assert_versions_survive(initial: &[u8], found: &[Vec<u8>], versions: &[WriterVersion]) {
    assert!(!found.is_empty(), "the file vanished");
    for f in found {
        assert!(
            f == initial || versions.iter().any(|v| &v.bytes == f),
            "torn or foreign content ({} bytes)",
            f.len()
        );
    }
    match versions.last() {
        Some(last) => assert!(
            found.contains(&last.bytes),
            "lost write: the newest writer version is in no file"
        ),
        None => assert!(found.iter().any(|f| f == initial), "initial bytes lost"),
    }
}

/// Offsets (a multiple of `step`) to wait between starting the writer and the operation, swept so
/// the operation meets the writer at different points of its cycle.
pub fn sweep_offsets(iterations: usize, step: Duration) -> impl Iterator<Item = Duration> {
    (0..iterations).map(move |i| step * i as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(ok: bool) -> OpRun {
        OpRun {
            ok,
            finished: Instant::now() + Duration::from_secs(1),
        }
    }

    fn v(seq: usize) -> WriterVersion {
        WriterVersion {
            bytes: version_bytes(seq),
            begun: Instant::now(),
            mode: WriterMode::TempRename,
            visible: true,
            saw_before: None,
            replaced: None,
        }
    }

    #[test]
    #[should_panic(expected = "lost write")]
    fn flags_lost_write() {
        assert_no_lost_write(b"init", b"ours", b"ours", &[v(1)], run(true));
    }

    #[test]
    #[should_panic(expected = "reported an error")]
    fn flags_error_that_wrote() {
        assert_no_lost_write(b"init", b"ours", b"ours", &[], run(false));
    }

    #[test]
    #[should_panic(expected = "torn")]
    fn flags_torn_content() {
        assert_no_lost_write(b"init", b"RACING-WRI", b"ours", &[v(1)], run(true));
    }

    #[test]
    fn accepts_writer_winning_and_clean_save() {
        let versions = [v(1), v(2)];
        let last = versions[1].bytes.clone();
        assert_no_lost_write(b"init", &last, b"ours", &versions, run(true));
        assert_no_lost_write(b"init", b"ours", b"ours", &[], run(true));
        assert_no_lost_write(b"init", b"init", b"ours", &[], run(false));
    }

    #[test]
    #[should_panic(expected = "lost write")]
    fn flags_replace_of_a_write_that_began_visible() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.txt");
        fs::write(&p, "init").unwrap();
        let other = dir.path().join("other.txt");
        fs::write(&other, "ours").unwrap();
        let begun = Instant::now();
        // The write lands at the path; the operation replaces the path afterwards.
        let Landed {
            visible,
            saw_before,
            replaced,
        } = write_once(&p, WriterMode::InPlace, 1, &version_bytes(1), || {}).unwrap();
        assert!(visible);
        fs::rename(&other, &p).unwrap();
        let version = WriterVersion {
            bytes: version_bytes(1),
            begun,
            mode: WriterMode::InPlace,
            visible,
            saw_before,
            replaced,
        };
        assert_no_lost_write(b"init", b"ours", b"ours", &[version], run(true));
    }

    #[test]
    fn write_into_an_already_replaced_file_is_not_a_lost_write() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.txt");
        fs::write(&p, "init").unwrap();
        let other = dir.path().join("other.txt");
        fs::write(&other, "ours").unwrap();
        let begun = Instant::now();
        // A correct operation replaces the path after the writer opened it; the writer's bytes
        // then land in the unlinked file and nobody can read them.
        let Landed {
            visible,
            saw_before,
            replaced,
        } = write_once(&p, WriterMode::InPlace, 1, &version_bytes(1), || {
            fs::rename(&other, &p).unwrap()
        })
        .unwrap();
        let version = WriterVersion {
            bytes: version_bytes(1),
            begun,
            mode: WriterMode::InPlace,
            visible,
            saw_before,
            replaced,
        };
        assert_no_lost_write(b"init", b"ours", b"ours", &[version], run(true));
    }

    #[test]
    #[should_panic(expected = "lost write")]
    fn flags_replace_of_an_earlier_version_when_a_later_one_wins() {
        let (v1, mut v2) = (v(1), v(2));
        v2.saw_before = Some(b"ours".to_vec());
        let last = v2.bytes.clone();
        assert_no_lost_write(b"init", &last, b"ours", &[v1, v2], run(true));
    }

    #[test]
    fn swapping_rule_allows_sight_of_ours_only_on_a_refused_run() {
        let (v1, mut v2) = (v(1), v(2));
        v2.saw_before = Some(b"ours".to_vec());
        v2.replaced = Some(v1.bytes.clone());
        let last = v2.bytes.clone();
        let versions = [v1, v2];
        assert_no_lost_write_swapping(b"init", &last, b"ours", &versions, run(false), &[]);
        let lost = std::panic::catch_unwind(|| {
            assert_no_lost_write_swapping(b"init", &last, b"ours", &versions, run(true), &[])
        });
        assert!(lost.is_err(), "a replaced version on a successful run");
        let left = std::panic::catch_unwind(|| {
            assert_no_lost_write_swapping(b"init", b"ours", b"ours", &[], run(false), &[])
        });
        assert!(left.is_err(), "our bytes left in the file by a refused run");
    }

    /// v1 lands before the exchange; `second` is the writer's next version, `third` the one
    /// after it, which found `third_saw` at the path.
    fn refused_run(
        second_saw: &[u8],
        second_replaced: &[u8],
        third_replaced: usize,
    ) -> Vec<WriterVersion> {
        let (v1, mut v2, mut v3) = (v(1), v(2), v(3));
        v2.saw_before = Some(second_saw.to_vec());
        v2.replaced = Some(second_replaced.to_vec());
        v3.saw_before = Some(version_bytes(third_replaced));
        v3.replaced = Some(version_bytes(third_replaced));
        vec![v1, v2, v3]
    }

    #[test]
    #[should_panic(expected = "lost write")]
    fn swapping_rule_flags_version_written_into_our_file_and_deleted() {
        // The writer saw our bytes at the path, wrote its second version into our file, and
        // the refused run deleted that file: the third version found the first one again.
        let versions = refused_run(b"ours", b"ours", 1);
        let last = versions[2].bytes.clone();
        assert_no_lost_write_swapping(b"init", &last, b"ours", &versions, run(false), &[]);
    }

    #[test]
    #[should_panic(expected = "lost write")]
    fn swapping_rule_flags_deleted_version_that_never_saw_ours() {
        // Same loss, but the writer had read the path before our bytes were there.
        let versions = refused_run(&version_bytes(1), &version_bytes(1), 1);
        let last = versions[2].bytes.clone();
        assert_no_lost_write_swapping(b"init", &last, b"ours", &versions, run(false), &[]);
    }

    #[test]
    #[should_panic(expected = "lost write")]
    fn swapping_rule_flags_a_deleted_version_the_writer_did_not_overwrite() {
        // v1 landed, a regression deleted it and reported a refusal; ours stood at the path
        // until v2, which replaced ours (not v1), and v3 replaced v2.
        let versions = refused_run(b"ours", b"ours", 2);
        let last = versions[2].bytes.clone();
        assert_no_lost_write_swapping(b"init", &last, b"ours", &versions, run(false), &[]);
    }

    #[test]
    fn swapping_rule_accepts_sight_of_ours_when_the_version_survives() {
        // Overwritten by the writer itself: the third version found the second.
        let versions = refused_run(b"ours", &version_bytes(1), 2);
        let last = versions[2].bytes.clone();
        assert_no_lost_write_swapping(b"init", &last, b"ours", &versions, run(false), &[]);
        // Kept in the conflict copy the error names.
        let versions = refused_run(b"ours", b"ours", 1);
        let (second, last) = (versions[1].bytes.clone(), versions[2].bytes.clone());
        assert_no_lost_write_swapping(b"init", &last, b"ours", &versions, run(false), &[second]);
        // Still in the file at the end.
        let versions = &refused_run(b"ours", &version_bytes(1), 1)[..2];
        let last = versions[1].bytes.clone();
        assert_no_lost_write_swapping(b"init", &last, b"ours", versions, run(false), &[]);
    }

    #[test]
    fn rename_writer_fails_on_an_unexpected_exchange_error() {
        // Only a filesystem without RENAME_EXCHANGE may leave `replaced` unknown; any other
        // failure would weaken the oracle without a word, so the writer stops instead.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f.txt");
        fs::write(&p, "init").unwrap();
        let denied = |_: &Path, _: &Path| Err(io::Error::from(io::ErrorKind::PermissionDenied));
        let r = write_once_with(
            &p,
            WriterMode::TempRename,
            1,
            &version_bytes(1),
            || {},
            denied,
        );
        assert!(r.is_err(), "unknown `replaced` accepted silently");
        let unsupported = |_: &Path, _: &Path| Err(io::Error::from(io::ErrorKind::Unsupported));
        let landed = write_once_with(
            &p,
            WriterMode::TempRename,
            2,
            &version_bytes(2),
            || {},
            unsupported,
        )
        .unwrap();
        assert_eq!(landed.replaced, None);
    }

    #[test]
    #[should_panic(expected = "oracle weakened")]
    fn unknown_replaced_where_exchange_works_fails_the_run() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            exchange_supported(dir.path()),
            "tmpfs/ext4 support exchange"
        );
        assert_exchange_proofs_known(dir.path(), &[v(1)]);
    }

    #[test]
    fn rename_writer_onto_a_missing_path_knows_it_replaced_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("gone.txt");
        let landed = write_once(&p, WriterMode::TempRename, 1, &version_bytes(1), || {}).unwrap();
        assert_eq!(landed.replaced, Some(Vec::new()));
        assert_eq!(fs::read(&p).unwrap(), version_bytes(1));
    }

    #[test]
    fn swapping_rule_does_not_count_unknown_replaced_as_a_loss() {
        // Without RENAME_EXCHANGE the rename writer cannot read what it replaced; v2 may well
        // have replaced v1, so the refused run is not reported as losing it.
        let (v1, v2) = (v(1), v(2));
        let last = v2.bytes.clone();
        assert_no_lost_write_swapping(b"init", &last, b"ours", &[v1, v2], run(false), &[]);
    }

    #[test]
    #[should_panic(expected = "lost write")]
    fn swapping_rule_ignores_proof_from_a_write_into_a_dead_file() {
        // v1 landed and the refused run deleted it. v2 had opened v1's file before that and
        // wrote into it after it left the path: it read v1 through its handle, but nobody can
        // read v2 at the path, so it does not show that the writer itself overwrote v1.
        let (v1, mut v2) = (v(1), v(2));
        v2.visible = false;
        v2.replaced = Some(v1.bytes.clone());
        assert_no_lost_write_swapping(b"init", b"init", b"ours", &[v1, v2], run(false), &[]);
    }

    #[test]
    fn swapping_rule_accepts_proof_from_a_write_whose_file_came_back() {
        // v2 opened v1's file, the refused run swapped ours in, v2 wrote into v1's file while
        // it was out of the path (so the writer saw it as not visible), and the swap-back put
        // that file at the path again: v2 is the final content, so the writer itself
        // overwrote v1. The same holds when that file ended in the conflict copy.
        let (v1, mut v2) = (v(1), v(2));
        v2.visible = false;
        v2.replaced = Some(v1.bytes.clone());
        let last = v2.bytes.clone();
        let versions = [v1, v2];
        assert_no_lost_write_swapping(b"init", &last, b"ours", &versions, run(false), &[]);
        let kept = [last];
        assert_no_lost_write_swapping(b"init", b"init", b"ours", &versions, run(false), &kept);
        // The file came back and v3 then replaced v2 at the path (trace of a real run).
        let (v1, mut v2, mut v3) = (v(1), v(2), v(3));
        v2.visible = false;
        v2.replaced = Some(v1.bytes.clone());
        v3.replaced = Some(v2.bytes.clone());
        let last = v3.bytes.clone();
        assert_no_lost_write_swapping(b"init", &last, b"ours", &[v1, v2, v3], run(false), &[]);
    }

    #[test]
    #[should_panic(expected = "newest writer version")]
    fn flags_vanished_version_on_move() {
        assert_versions_survive(b"init", &[b"init".to_vec()], &[v(1)]);
    }

    #[test]
    fn writer_records_unique_versions_in_both_modes() {
        for mode in [WriterMode::InPlace, WriterMode::TempRename] {
            let dir = tempfile::tempdir().unwrap();
            let p = dir.path().join("f.txt");
            fs::write(&p, "init").unwrap();
            let w = RacingWriter::start(&p, mode, Duration::from_micros(100), 5);
            let versions = w.stop();
            assert!(versions.len() <= 5);
            let on_disk = fs::read(&p).unwrap();
            match versions.last() {
                Some(l) => assert_eq!(on_disk, l.bytes),
                None => assert_eq!(on_disk, b"init"),
            }
            assert!(is_writer_content(&on_disk) || on_disk == b"init");
        }
    }

    const STEP: Duration = Duration::from_micros(400);

    type Rule = fn(&[u8], &[u8], &[u8], &[WriterVersion], OpRun, &[Vec<u8>]);

    fn race_document_save(strict: bool) {
        let rule: Rule = if strict {
            assert_no_lost_write_swapping
        } else {
            |initial, last, ours, versions, run, _| {
                assert_whole_content(initial, last, ours, versions, run)
            }
        };
        race_document_save_with(rule, Duration::from_millis(3), 4, STEP, 20);
        assert_both_outcomes(Target::Own);
    }

    /// Success and refusal of `target` without any timing: the op alone succeeds, and a write
    /// at `BeforeSwap` (after the last freshness check) makes it refuse. The racing sweeps
    /// below judge every run with the strict rule but need not hit both outcomes.
    fn assert_both_outcomes(target: Target) {
        use crate::{DiskVersion, Document};
        for refuse in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let own = dir.path().join("doc.txt");
            fs::write(&own, b"initial\n").unwrap();
            let mut doc = Document::open(&own, u64::MAX).unwrap();
            doc.buffer.insert_text("X");
            let p = match target {
                Target::Own => own.clone(),
                Target::Other | Target::Unprompted => {
                    let p = dir.path().join("other.txt");
                    fs::write(&p, b"other\n").unwrap();
                    p
                }
                Target::Absent => dir.path().join("new.txt"),
            };
            let seen = DiskVersion::of_path(&p).unwrap();
            if refuse {
                let at = p.clone();
                crate::document::hook::set(crate::document::hook::Stage::BeforeSwap, move || {
                    fs::write(&at, b"theirs\n").unwrap()
                });
            }
            let result = match target {
                Target::Own => doc.save(),
                Target::Unprompted => doc.save_as(&p),
                _ => doc.save_as_seen(&p, &seen),
            };
            // Whatever did not fire must not reach the next run of this thread.
            crate::document::hook::fire(crate::document::hook::Stage::BeforeSwap);
            let _ = crate::document::hook::take_version_taken();
            let on_disk = fs::read(&p).unwrap();
            if refuse {
                assert!(result.is_err(), "{target:?}: refusal expected");
                assert_eq!(on_disk, b"theirs\n", "{target:?}: their write survives");
            } else {
                result.unwrap_or_else(|e| panic!("{target:?}: success expected: {e}"));
                assert_eq!(on_disk, b"Xinitial\n", "{target:?}: ours written");
            }
        }
    }

    fn race_document_save_with(
        rule: Rule,
        interval: Duration,
        versions: usize,
        step: Duration,
        sweep: usize,
    ) -> (usize, usize) {
        race_with(Target::Own, rule, interval, versions, step, sweep)
    }

    /// The file the operation writes and the writer races.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Target {
        /// `save` of the document's own file.
        Own,
        /// `save_as_seen` to another existing file, seen before the writer starts.
        Other,
        /// `save_as_seen` to a name seen absent; only a rename writer can create it.
        Absent,
        /// Plain `save_as` to another existing file, no prompt: it may replace the version
        /// on disk when it takes the target's version, never one written after that.
        Unprompted,
    }

    fn race_with(
        target: Target,
        rule: Rule,
        interval: Duration,
        versions: usize,
        step: Duration,
        sweep: usize,
    ) -> (usize, usize) {
        use crate::{DiskVersion, Document};
        let (mut ok_count, mut err_count, mut kept_count, mut partial_takes, mut no_version) =
            (0, 0, 0, 0, 0);
        let modes: &[WriterMode] = match target {
            Target::Absent => &[WriterMode::TempRename],
            _ => &[WriterMode::InPlace, WriterMode::TempRename],
        };
        for &mode in modes {
            for offset in sweep_offsets(sweep, step) {
                let dir = tempfile::tempdir().unwrap();
                let own = dir.path().join("doc.txt");
                fs::write(&own, b"initial\n").unwrap();
                let mut doc = Document::open(&own, u64::MAX).unwrap();
                doc.buffer.insert_text("X");
                let ours = b"Xinitial\n".to_vec();
                let (p, initial) = match target {
                    Target::Own => (own.clone(), b"initial\n".to_vec()),
                    Target::Other | Target::Unprompted => {
                        let p = dir.path().join("other.txt");
                        fs::write(&p, b"other\n").unwrap();
                        (p, b"other\n".to_vec())
                    }
                    // Never matches: the file only exists once someone wrote it.
                    Target::Absent => (dir.path().join("new.txt"), Vec::new()),
                };
                let seen = DiskVersion::of_path(&p).unwrap();
                let taken = std::rc::Rc::new(std::cell::Cell::new(None));
                if target == Target::Unprompted {
                    let at = taken.clone();
                    crate::document::hook::set(
                        crate::document::hook::Stage::SaveAsVersionTaken,
                        move || at.set(Some(Instant::now())),
                    );
                }
                let writer = RacingWriter::start_after(offset, &p, mode, interval, versions);
                let (result, run) = timed(
                    || match target {
                        Target::Own => doc.save(),
                        Target::Unprompted => doc.save_as(&p),
                        _ => doc.save_as_seen(&p, &seen),
                    },
                    |r| r.is_ok(),
                );
                let mut versions = writer.stop();
                assert_exchange_proofs_known(dir.path(), &versions);
                let all_versions = versions.clone();
                let final_bytes = fs::read(&p).unwrap();
                let mut initial = initial;
                if target == Target::Unprompted {
                    // `save_as` may replace the version it took and, with it, the writer's
                    // earlier ones; every version after it must survive (strict rule below).
                    // Drop a hook that did not fire so it cannot leak into this thread's next run.
                    crate::document::hook::fire(crate::document::hook::Stage::SaveAsVersionTaken);
                    let excused = match crate::document::hook::take_version_taken() {
                        // `of_path` gave up under the writer before a version was taken: the
                        // refusal must be that I/O error and ours must not be on disk; nothing
                        // is excused, so the rule below checks every version.
                        None => {
                            assert!(
                                matches!(&result, Err(crate::DocError::Io(e))
                                    if e.to_string().contains("keeps changing")),
                                "save_as took no version but did not fail with the I/O error: {result:?}"
                            );
                            assert_ne!(final_bytes, ours, "ours on disk without a version taken");
                            no_version += 1;
                            0
                        }
                        Some(took) => match versions.iter().position(|v| took.holds(&v.bytes)) {
                            Some(i) => i + 1,
                            None if took.holds(&initial) => 0,
                            // Taken while an in-place write had truncated the file: the version
                            // in flight and the earlier ones are excused (the stated window).
                            None => {
                                let taken = taken.get().expect("save_as took no version");
                                partial_takes += 1;
                                versions.iter().take_while(|v| v.begun < taken).count()
                            }
                        },
                    };
                    let after = versions.split_off(excused);
                    if let Some(v) = versions.iter().find(|v| v.bytes == final_bytes) {
                        initial = v.bytes.clone();
                    }
                    versions = after;
                }
                if target != Target::Own {
                    let expected = if run.ok { &p } else { &own };
                    assert_eq!(
                        doc.path.as_ref(),
                        Some(expected),
                        "document path after Save As"
                    );
                    assert_eq!(fs::read(&own).unwrap(), b"initial\n", "own file untouched");
                }
                // Nothing is left behind except a conflict copy the error names, and that
                // copy holds a whole writer version (or our bytes when in doubt), never less.
                let kept = match &result {
                    Err(e) => e
                        .to_string()
                        .rsplit_once(" kept at ")
                        .map(|(_, at)| PathBuf::from(at)),
                    Ok(_) => None,
                };
                let mut kept_bytes = Vec::new();
                for entry in fs::read_dir(dir.path()).unwrap() {
                    let entry = entry.unwrap().path();
                    if entry == p || entry == own {
                        continue;
                    }
                    assert_eq!(Some(&entry), kept.as_ref(), "unreported file left behind");
                    let bytes = fs::read(&entry).unwrap();
                    assert!(
                        bytes == ours || all_versions.iter().any(|v| v.bytes == bytes),
                        "conflict copy holds torn content ({} bytes)",
                        bytes.len()
                    );
                    kept_count += 1;
                    kept_bytes.push(bytes);
                }
                rule(&initial, &final_bytes, &ours, &versions, run, &kept_bytes);
                if run.ok {
                    ok_count += 1
                } else {
                    err_count += 1
                }
            }
        }
        eprintln!(
            "{target:?}: ok={ok_count} refused={err_count} kept={kept_count} partial_takes={partial_takes} no_version={no_version}"
        );
        (ok_count, err_count)
    }

    #[test]
    fn document_save_race_leaves_whole_content() {
        race_document_save(false);
    }

    // `save` puts its file in place with RENAME_EXCHANGE, checks the file it took out and
    // deletes only what it proved (`safe_replace`), so no writer version is ever replaced.
    #[test]
    fn document_save_never_loses_a_concurrent_write() {
        race_document_save(true);
    }

    // A writer with no pause between its versions puts several of them inside one save, so
    // a second version can land while the save is restoring the first. It also reads the
    // path while a refused save has our bytes there, hence the swapping rule.
    #[test]
    fn document_save_never_loses_a_write_from_a_fast_writer() {
        race_document_save_with(
            assert_no_lost_write_swapping,
            Duration::ZERO,
            300,
            STEP / 4,
            40,
        );
    }

    // Save As onto a file the user was shown (the Replace prompt) while a writer keeps
    // rewriting it: only that version is ever replaced, under the strict rule.
    #[test]
    fn save_as_seen_never_loses_a_concurrent_write() {
        for target in [Target::Other, Target::Absent] {
            race_with(
                target,
                assert_no_lost_write_swapping,
                Duration::from_millis(3),
                4,
                STEP,
                20,
            );
            assert_both_outcomes(target);
        }
    }

    #[test]
    fn save_as_seen_never_loses_a_write_from_a_fast_writer() {
        for target in [Target::Other, Target::Absent] {
            race_with(
                target,
                assert_no_lost_write_swapping,
                Duration::ZERO,
                300,
                STEP / 4,
                40,
            );
        }
    }

    // `file.save_as` with a path argument calls plain `save_as` without any prompt (D89).
    #[test]
    fn unprompted_save_as_never_loses_a_concurrent_write() {
        race_with(
            Target::Unprompted,
            assert_no_lost_write_swapping,
            Duration::from_millis(3),
            4,
            STEP,
            20,
        );
        assert_both_outcomes(Target::Unprompted);
        race_with(
            Target::Unprompted,
            assert_no_lost_write_swapping,
            Duration::ZERO,
            300,
            STEP / 4,
            40,
        );
    }

    #[test]
    fn document_save_without_a_writer_writes_ours() {
        use crate::Document;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("doc.txt");
        fs::write(&p, "initial\n").unwrap();
        let mut doc = Document::open(&p, u64::MAX).unwrap();
        doc.buffer.insert_text("X");
        let (_, run) = timed(|| doc.save(), |r| r.is_ok());
        assert!(run.ok);
        assert_no_lost_write(
            b"initial\n",
            &fs::read(&p).unwrap(),
            b"Xinitial\n",
            &[],
            run,
        );
    }
}
