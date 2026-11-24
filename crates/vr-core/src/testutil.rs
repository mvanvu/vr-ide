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
    /// handle for an in-place write, taken out by an exchange for a rename write). None when
    /// the writer could not tell. A later version whose `replaced` equals this one's bytes
    /// proves the writer itself overwrote it.
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
            // Exchange, then read what came out: exactly the file this write replaced. Without
            // exchange support, or when the path is gone, a plain rename creates the path.
            let replaced = match crate::safe_replace::exchange_files(&tmp, path) {
                Ok(()) => {
                    let old = fs::read(&tmp).ok();
                    fs::remove_file(&tmp)?;
                    old
                }
                Err(_) => {
                    fs::rename(&tmp, path)?;
                    None
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
/// replaced (`replaced`): the writer overwriting it itself is proven, never inferred.
/// Writers that could not read what they replaced (`replaced: None`) prove nothing.
fn assert_versions_accounted_for(
    final_bytes: &[u8],
    ours: &[u8],
    versions: &[WriterVersion],
    kept: &[Vec<u8>],
) {
    let saw_ours = |v: &WriterVersion| v.saw_before.as_deref() == Some(ours);
    for (i, v) in versions.iter().enumerate() {
        if !v.visible && !saw_ours(v) {
            continue;
        }
        let later = &versions[i + 1..];
        let survives = v.bytes == final_bytes
            || kept.contains(&v.bytes)
            || later
                .iter()
                .any(|w| w.replaced.as_deref() == Some(&v.bytes[..]));
        assert!(
            survives,
            "lost write: writer version {} of {} is in no file after a run that reported an \
             error, and the writer did not overwrite it",
            i + 1,
            versions.len()
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
    }

    fn race_document_save_with(
        rule: Rule,
        interval: Duration,
        versions: usize,
        step: Duration,
        sweep: usize,
    ) {
        use crate::Document;
        let (mut ok_count, mut err_count, mut kept_count) = (0, 0, 0);
        for mode in [WriterMode::InPlace, WriterMode::TempRename] {
            for offset in sweep_offsets(sweep, step) {
                let dir = tempfile::tempdir().unwrap();
                let p = dir.path().join("doc.txt");
                let initial = b"initial\n".to_vec();
                fs::write(&p, &initial).unwrap();
                let mut doc = Document::open(&p, u64::MAX).unwrap();
                doc.buffer.insert_text("X");
                let ours = b"Xinitial\n".to_vec();
                let writer = RacingWriter::start_after(offset, &p, mode, interval, versions);
                let (result, run) = timed(|| doc.save(), |r| r.is_ok());
                let versions = writer.stop();
                let final_bytes = fs::read(&p).unwrap();
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
                    if entry == p {
                        continue;
                    }
                    assert_eq!(Some(&entry), kept.as_ref(), "unreported file left behind");
                    let bytes = fs::read(&entry).unwrap();
                    assert!(
                        bytes == ours || versions.iter().any(|v| v.bytes == bytes),
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
        eprintln!("save: ok={ok_count} refused={err_count} kept={kept_count}");
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
