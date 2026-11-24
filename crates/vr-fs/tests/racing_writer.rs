//! Moves and renames of a file that another writer is rewriting must never lose a version.

use std::fs;
use std::path::Path;
use std::time::Duration;

const STEP: Duration = Duration::from_micros(25);

use vr_core::testutil::{RacingWriter, WriterMode, assert_versions_survive, sweep_offsets};

const INITIAL: &[u8] = b"initial\n";

fn read_if_exists(p: &Path) -> Option<Vec<u8>> {
    fs::read(p).ok()
}

fn race(op: impl Fn(&Path, &Path, &Path) -> Vec<std::path::PathBuf>) {
    for mode in [WriterMode::InPlace, WriterMode::TempRename] {
        for offset in sweep_offsets(20, STEP) {
            let dir = tempfile::tempdir().unwrap();
            let src = dir.path().join("src.txt");
            let dest_dir = dir.path().join("dest");
            fs::create_dir(&dest_dir).unwrap();
            fs::write(&src, INITIAL).unwrap();
            let writer =
                RacingWriter::start_after(offset, &src, mode, Duration::from_micros(200), 12);
            let places = op(&src, dir.path(), &dest_dir);
            let versions = writer.stop();
            let found: Vec<Vec<u8>> = places.iter().filter_map(|p| read_if_exists(p)).collect();
            assert_versions_survive(INITIAL, &found, &versions);
        }
    }
}

#[test]
fn rename_of_a_file_being_rewritten_loses_no_version() {
    race(|src, dir, _| {
        let to = dir.join("renamed.txt");
        vr_fs::ops::rename(src, &to).unwrap();
        vec![src.to_path_buf(), to]
    });
}

#[test]
fn move_to_of_a_file_being_rewritten_loses_no_version() {
    race(|src, _, dest_dir| {
        let to = vr_fs::ops::move_to(src, dest_dir).unwrap();
        vec![src.to_path_buf(), to]
    });
}
