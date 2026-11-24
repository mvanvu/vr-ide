//! Test helpers (feature `test-util`): a concurrent writer that races a write path, and the
//! rules a write to a user file must satisfy against it.
//!
//! Usage: create the file, open the document / build the preview, `RacingWriter::start`, wait
//! a swept delay (`start_after`), run the operation through `timed`, `stop` the writer, read the file and
//! call `assert_no_lost_write` (one path) or `assert_versions_survive` (a move).

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
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
    /// Taken before the write started.
    pub begun: Instant,
    /// False when an in-place write went to a file that had already been replaced or moved
    /// away from the path (the writer held the old file open): nobody could ever read it
    /// there, so replacing it is not a lost write.
    pub visible: bool,
    /// What the path held just before this write (None if unreadable): lets the assertion see
    /// that an operation's bytes replaced the previous version even when this one wins later.
    pub saw_before: Option<Vec<u8>>,
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
                    Ok((visible, saw_before)) => versions.push(WriterVersion {
                        bytes,
                        begun,
                        visible,
                        saw_before,
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

/// Returns (visible, what the path held before). `between` runs after the file is opened /
/// staged and before the bytes land: a seam for tests of the writer itself.
fn write_once(
    path: &Path,
    mode: WriterMode,
    seq: usize,
    bytes: &[u8],
    between: impl FnOnce(),
) -> io::Result<(bool, Option<Vec<u8>>)> {
    let saw_before = fs::read(path).ok();
    match mode {
        WriterMode::InPlace => {
            let mut f = OpenOptions::new().write(true).truncate(true).open(path)?;
            // Visible means this handle was the file at the path when the write began; a later
            // replace of the path must not hide the write from the assertion.
            let here = fs::metadata(path).map(|m| m.ino()).ok();
            let opened = f.metadata()?.ino();
            between();
            f.write_all(bytes)?;
            // A write that went into a file replaced meanwhile can never be read at the path.
            let after = fs::metadata(path).map(|m| m.ino()).ok();
            let visible = here == Some(opened) && after == Some(opened);
            Ok((visible, saw_before))
        }
        WriterMode::TempRename => {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let tmp: PathBuf = path.with_file_name(format!(".{name}.racing-{seq}"));
            fs::write(&tmp, bytes)?;
            between();
            fs::rename(&tmp, path)?;
            Ok((true, saw_before))
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
    assert_whole_content(initial, final_bytes, ours, versions, run);
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

/// The first two bullets of `assert_no_lost_write` only: for a write path with a documented
/// window between its last freshness check and the replace, where the third rule cannot hold.
pub fn assert_whole_content(
    initial: &[u8],
    final_bytes: &[u8],
    ours: &[u8],
    versions: &[WriterVersion],
    run: OpRun,
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
        run.ok || !saw_ours,
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
            visible: true,
            saw_before: None,
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
        let (visible, saw_before) =
            write_once(&p, WriterMode::InPlace, 1, &version_bytes(1), || {}).unwrap();
        assert!(visible);
        fs::rename(&other, &p).unwrap();
        let version = WriterVersion {
            bytes: version_bytes(1),
            begun,
            visible,
            saw_before,
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
        let (visible, saw_before) =
            write_once(&p, WriterMode::InPlace, 1, &version_bytes(1), || {
                fs::rename(&other, &p).unwrap()
            })
            .unwrap();
        let version = WriterVersion {
            bytes: version_bytes(1),
            begun,
            visible,
            saw_before,
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

    fn race_document_save(strict: bool) {
        use crate::Document;
        let (mut ok_count, mut err_count) = (0, 0);
        for mode in [WriterMode::InPlace, WriterMode::TempRename] {
            for offset in sweep_offsets(20, STEP) {
                let dir = tempfile::tempdir().unwrap();
                let p = dir.path().join("doc.txt");
                let initial = b"initial\n".to_vec();
                fs::write(&p, &initial).unwrap();
                let mut doc = Document::open(&p, u64::MAX).unwrap();
                doc.buffer.insert_text("X");
                let ours = b"Xinitial\n".to_vec();
                let writer =
                    RacingWriter::start_after(offset, &p, mode, Duration::from_millis(3), 4);
                let (_, run) = timed(|| doc.save(), |r| r.is_ok());
                let versions = writer.stop();
                let final_bytes = fs::read(&p).unwrap();
                if strict {
                    assert_no_lost_write(&initial, &final_bytes, &ours, &versions, run);
                } else {
                    assert_whole_content(&initial, &final_bytes, &ours, &versions, run);
                }
                if run.ok {
                    ok_count += 1
                } else {
                    err_count += 1
                }
            }
        }
        eprintln!("save: ok={ok_count} refused={err_count}");
    }

    #[test]
    fn document_save_race_leaves_whole_content() {
        race_document_save(false);
    }

    // `save` keeps the window between its last stat and rename(2) (D25: Linux has no
    // compare-and-swap rename), and a writer blocked behind the save's fsync lands in it often.
    // Run with `--ignored` to measure; enable by default when that window is closed.
    #[test]
    #[ignore = "known gap D25: a writer landing between the last stat and rename is replaced"]
    fn document_save_never_loses_a_concurrent_write() {
        race_document_save(true);
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
