//! `apply_replace` against a writer rewriting the same file must never lose a write.

use std::fs;
use std::time::Duration;

const STEP: Duration = Duration::from_micros(400);

use vr_core::testutil::{
    RacingWriter, WriterMode, assert_no_lost_write, assert_no_lost_write_swapping, sweep_offsets,
    timed,
};
use vr_search::{Query, apply_replace, plan_replace};

const INITIAL: &[u8] = b"foo one\nbar\nfoo two\n";
const OURS: &[u8] = b"BAR one\nbar\nBAR two\n";

fn query() -> Query {
    Query {
        pattern: "foo".into(),
        regex: false,
        case_sensitive: true,
        whole_word: false,
    }
}

#[test]
fn apply_replace_never_loses_a_concurrent_write() {
    let (mut ok_count, mut err_count) = (0, 0);
    for mode in [WriterMode::InPlace, WriterMode::TempRename] {
        for offset in sweep_offsets(20, STEP) {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path();
            let file = root.join("victim.txt");
            fs::write(&file, INITIAL).unwrap();
            let previews = plan_replace(root, &query(), "BAR", std::slice::from_ref(&file));
            assert!(previews[0].error.is_none());
            let writer =
                RacingWriter::start_after(offset, &file, mode, Duration::from_millis(3), 4);
            let (result, run) = timed(
                || apply_replace(&previews, std::slice::from_ref(&file)),
                |r| r[0].1.is_ok(),
            );
            let versions = writer.stop();
            let final_bytes = fs::read(&file).unwrap();
            // The conflict copy a refused replace names, if any.
            let kept: Vec<Vec<u8>> = result[0]
                .1
                .as_ref()
                .err()
                .and_then(|e| {
                    let msg = e.to_string();
                    msg.rsplit_once(" kept at ")
                        .map(|(_, at)| fs::read(at).unwrap())
                })
                .into_iter()
                .collect();
            assert_no_lost_write_swapping(INITIAL, &final_bytes, OURS, &versions, run, &kept);
            if run.ok {
                ok_count += 1
            } else {
                err_count += 1
            }
        }
    }
    eprintln!("apply_replace: ok={ok_count} refused={err_count}");
}

#[test]
fn apply_replace_without_a_writer_writes_ours() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let file = root.join("victim.txt");
    fs::write(&file, INITIAL).unwrap();
    let previews = plan_replace(root, &query(), "BAR", std::slice::from_ref(&file));
    let (_, run) = timed(
        || apply_replace(&previews, std::slice::from_ref(&file)),
        |r| r[0].1.is_ok(),
    );
    assert!(run.ok);
    assert_no_lost_write(INITIAL, &fs::read(&file).unwrap(), OURS, &[], run);
}
