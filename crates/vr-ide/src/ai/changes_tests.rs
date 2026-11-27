//! AI changes, the store and Revert (AC-9, SEC-9..SEC-13) without the UI: trackers walk temp
//! folders and get their events by hand, plus one run with the real watcher thread.

use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use vr_core::FsEvent;
use vr_core::safe_replace::Step;
use vr_fs::FakeTrash;

use super::changes::*;

/// Files made by a test count as older than the session once this much time passed.
const MARGIN: Duration = Duration::from_millis(15);

struct Fx {
    _tmp: tempfile::TempDir,
    root: PathBuf,
    state: PathBuf,
    outside: PathBuf,
    total: Arc<AtomicU64>,
    trash: FakeTrash,
    /// Where `trash` puts what it takes, as `0`, `1`, ... in order.
    trash_dir: PathBuf,
}

impl Fx {
    fn new() -> Fx {
        let tmp = tempfile::tempdir().unwrap();
        let base = fs::canonicalize(tmp.path()).unwrap();
        let (root, state, outside) = (base.join("ws"), base.join("state"), base.join("outside"));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        Fx {
            trash: FakeTrash::new(base.join("trash")),
            trash_dir: base.join("trash"),
            _tmp: tmp,
            root,
            state,
            outside,
            total: Arc::default(),
        }
    }

    fn write(&self, rel: &str, content: impl AsRef<[u8]>) -> PathBuf {
        let p = self.root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, content).unwrap();
        p
    }

    fn limits() -> Limits {
        Limits {
            margin: Some(MARGIN),
            ..Limits::default()
        }
    }

    fn config(&self, limits: Limits) -> Config {
        Config {
            root: self.root.clone(),
            store_base: self.state.join("ai"),
            total: Arc::clone(&self.total),
            limits,
            excluded: vec![self.state.clone()],
            own: None,
            wake: Arc::new(AtomicBool::new(false)),
        }
    }

    /// A tracker whose walk has run: files written before this call are "from before".
    fn started(&self, limits: Limits) -> Changes {
        std::thread::sleep(MARGIN * 2);
        let c = Changes::manual(self.config(limits)).unwrap();
        c.scan();
        c
    }

    /// Keep of the file as a diff taken now shows it; false when it was refused.
    fn keep(&self, c: &Changes, rel: &str) -> bool {
        let d = c.diff(Path::new(rel)).unwrap();
        c.keep(Path::new(rel), &d.seen).is_ok()
    }

    fn revert(&self, c: &Changes, rel: &str) -> Result<Reverted, RevertError> {
        let d = c.diff(Path::new(rel)).unwrap();
        c.revert(Path::new(rel), &d.seen, &self.trash, &mut |_| {})
    }
}

fn rec<'a>(records: &'a [Record], rel: &str) -> &'a Record {
    records
        .iter()
        .find(|r| r.rel == Path::new(rel))
        .unwrap_or_else(|| panic!("no record of {rel}: {records:?}"))
}

fn blobs(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "blob"))
        .collect();
    v.sort();
    v
}

// ---- SEC-9 ------------------------------------------------------------------------------------

#[test]
fn sec9_a_file_read_at_start_reverts_to_its_exact_earlier_bytes() {
    let fx = Fx::new();
    let p = fx.write("src/a.rs", "one\ntwo\n");
    let c = fx.started(Fx::limits());
    assert_eq!(c.count(), 0);
    fs::write(&p, "one\n2\nthree\n").unwrap();
    c.feed(FsEvent::Modified(p.clone()));
    let records = c.records();
    let r = rec(&records, "src/a.rs");
    assert_eq!((r.kind, r.missing), (Kind::Modified, None));
    assert!(r.can_revert());
    assert_eq!((r.added, r.removed), (Some(2), Some(1)));
    let d = c.diff(Path::new("src/a.rs")).unwrap();
    let shown: Vec<(LineKind, Option<u32>, &str)> = d
        .lines
        .iter()
        .map(|l| (l.kind, l.num, l.text.as_str()))
        .collect();
    assert_eq!(
        shown,
        vec![
            (LineKind::Hunk, None, "@@ -1,2 +1,3 @@"),
            (LineKind::Context, Some(1), "one"),
            (LineKind::Removed, Some(2), "two"),
            (LineKind::Added, Some(2), "2"),
            (LineKind::Added, Some(3), "three"),
        ]
    );
    assert_eq!(fx.revert(&c, "src/a.rs").unwrap(), Reverted::Restored);
    assert_eq!(fs::read(&p).unwrap(), b"one\ntwo\n");
    assert_eq!(c.count(), 0);
    // The revert's own write is not a change of the agent's.
    c.feed(FsEvent::Modified(p.clone()));
    assert_eq!(c.count(), 0);
    // A later change gets the reverted content as its earlier version.
    fs::write(&p, "again\n").unwrap();
    c.feed(FsEvent::Modified(p.clone()));
    assert_eq!(fx.revert(&c, "src/a.rs").unwrap(), Reverted::Restored);
    assert_eq!(fs::read(&p).unwrap(), b"one\ntwo\n");
}

#[test]
fn sec9_a_file_first_seen_after_its_change_is_listed_keep_only() {
    let fx = Fx::new();
    std::thread::sleep(MARGIN * 2);
    let c = Changes::manual(fx.config(Fx::limits())).unwrap();
    // Written after the session's start and before the walk came by: what the walk reads is
    // the agent's version and must never become the earlier one.
    let early = fx.write("early.txt", "agent\n");
    c.scan();
    let records = c.records();
    let r = rec(&records, "early.txt");
    assert_eq!(
        (r.kind, r.missing),
        (Kind::Modified, Some(NoEarlier::NotSeen))
    );
    assert!(!r.can_revert());
    let d = c.diff(Path::new("early.txt")).unwrap();
    assert!(!d.can_revert);
    assert!(matches!(
        c.revert(Path::new("early.txt"), &d.seen, &fx.trash, &mut |_| {}),
        Err(RevertError::NoEarlier)
    ));
    assert_eq!(fs::read(&early).unwrap(), b"agent\n");
    assert!(blobs(&c.store_dir()).is_empty());
    // The same for a file in a folder the ignore rules leave out: listed, never copied.
    assert!(fx.keep(&c, "early.txt"));
    assert_eq!(c.count(), 0);
}

#[test]
fn sec9_ignored_folders_are_watched_but_not_copied() {
    let fx = Fx::new();
    fx.write(".gitignore", "target/\n");
    fs::create_dir_all(fx.root.join(".git")).unwrap();
    let built = fx.write("target/out.bin", "v1");
    let env = fx.write(".env", "KEY=1\n");
    let c = fx.started(Fx::limits());
    assert_eq!(blobs(&c.store_dir()).len(), 2, ".gitignore and .env");
    fs::write(&built, "v2").unwrap();
    fs::write(&env, "KEY=2\n").unwrap();
    c.feed(FsEvent::Modified(built));
    c.feed(FsEvent::Modified(env.clone()));
    let records = c.records();
    assert_eq!(
        rec(&records, "target/out.bin").missing,
        Some(NoEarlier::NotSeen)
    );
    assert!(rec(&records, ".env").can_revert());
    fx.revert(&c, ".env").unwrap();
    assert_eq!(fs::read(&env).unwrap(), b"KEY=1\n");
}

#[test]
fn created_deleted_and_undone_changes() {
    let fx = Fx::new();
    let old = fx.write("docs/old.md", "a\nb\n");
    let kept = fx.write("kept.txt", "same\n");
    let c = fx.started(Fx::limits());
    // Created in a folder the walk listed.
    let new = fx.write("new.rs", "fn main() {}\n");
    c.feed(FsEvent::Created(new.clone()));
    // Created in a folder that is new itself.
    let deep = fx.write("fresh/deep/x.txt", "1\n2\n3\n");
    c.feed(FsEvent::Created(fx.root.join("fresh")));
    fs::remove_file(&old).unwrap();
    c.feed(FsEvent::Removed(old.clone()));
    let records = c.records();
    assert_eq!(rec(&records, "new.rs").kind, Kind::Created);
    assert_eq!(rec(&records, "new.rs").added, Some(1));
    assert_eq!(rec(&records, "fresh/deep/x.txt").kind, Kind::Created);
    assert_eq!(rec(&records, "fresh/deep/x.txt").added, Some(3));
    let d = rec(&records, "docs/old.md");
    assert_eq!(
        (d.kind, d.removed, d.can_revert()),
        (Kind::Deleted, Some(2), true)
    );
    // A deleted file comes back byte for byte.
    assert_eq!(fx.revert(&c, "docs/old.md").unwrap(), Reverted::Restored);
    assert_eq!(fs::read(&old).unwrap(), b"a\nb\n");
    // A created file goes to the trash, never unlinked.
    assert_eq!(fx.revert(&c, "new.rs").unwrap(), Reverted::Trashed);
    assert!(!new.exists());
    assert_eq!(fx.trash.trashed(), vec![new.clone()]);
    // Created and removed again: nothing to show.
    fs::remove_file(&deep).unwrap();
    c.feed(FsEvent::Removed(deep));
    assert_eq!(c.count(), 0);
    // Changed and changed back to the earlier content: nothing to show.
    fs::write(&kept, "other\n").unwrap();
    c.feed(FsEvent::Modified(kept.clone()));
    assert_eq!(c.count(), 1);
    fs::write(&kept, "same\n").unwrap();
    c.feed(FsEvent::Modified(kept.clone()));
    assert_eq!(c.count(), 0);
}

#[test]
fn a_folder_moved_away_takes_its_files_and_keep_starts_over() {
    let fx = Fx::new();
    let a = fx.write("pkg/a.txt", "a\n");
    fx.write("pkg/b.txt", "b\n");
    let c = fx.started(Fx::limits());
    fs::rename(fx.root.join("pkg"), fx.outside.join("pkg")).unwrap();
    c.feed(FsEvent::Removed(fx.root.join("pkg")));
    let records = c.records();
    assert_eq!(records.len(), 2);
    assert!(
        records
            .iter()
            .all(|r| r.kind == Kind::Deleted && r.can_revert())
    );
    // The folder is made again for the file that comes back.
    assert_eq!(fx.revert(&c, "pkg/a.txt").unwrap(), Reverted::Restored);
    assert_eq!(fs::read(&a).unwrap(), b"a\n");
    assert!(fx.keep(&c, "pkg/b.txt"));
    assert_eq!(c.count(), 0);
    // After Keep, the file as it was kept is the earlier version of the next change.
    let k = fx.write("k.txt", "v1\n");
    c.feed(FsEvent::Created(k.clone()));
    assert!(fx.keep(&c, "k.txt"));
    fs::write(&k, "v2\n").unwrap();
    c.feed(FsEvent::Modified(k.clone()));
    assert_eq!(c.records()[0].kind, Kind::Modified);
    fx.revert(&c, "k.txt").unwrap();
    assert_eq!(fs::read(&k).unwrap(), b"v1\n");
}

#[test]
fn an_exited_program_is_not_blamed_and_a_restart_walks_again() {
    let fx = Fx::new();
    let p = fx.write("a.txt", "v1\n");
    let c = fx.started(Fx::limits());
    c.pause();
    c.scan();
    assert!(blobs(&c.store_dir()).is_empty(), "the walk's copies go");
    fs::write(&p, "by the user\n").unwrap();
    c.feed(FsEvent::Modified(p.clone()));
    assert_eq!(c.count(), 0);
    std::thread::sleep(MARGIN * 2);
    c.resume();
    c.scan();
    fs::write(&p, "by the agent\n").unwrap();
    c.feed(FsEvent::Modified(p.clone()));
    fx.revert(&c, "a.txt").unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"by the user\n");
}

// ---- SEC-10 -----------------------------------------------------------------------------------

#[test]
fn sec10_the_store_is_private_and_names_nothing() {
    let fx = Fx::new();
    let env = fx.write(".env", "SECRET=1\n");
    fs::set_permissions(&env, fs::Permissions::from_mode(0o644)).unwrap();
    // A state folder left group-writable by something else is closed.
    let base = fx.state.join("ai");
    fs::create_dir_all(&base).unwrap();
    fs::set_permissions(&base, fs::Permissions::from_mode(0o775)).unwrap();
    let c = fx.started(Fx::limits());
    let dir = c.store_dir();
    assert!(dir.starts_with(&base));
    let mode = |p: &Path| fs::symlink_metadata(p).unwrap().mode() & 0o777;
    assert_eq!(mode(&base), 0o700);
    assert_eq!(mode(&dir), 0o700);
    let stored = blobs(&dir);
    assert_eq!(stored.len(), 1);
    assert_eq!(mode(&stored[0]), 0o600);
    assert_eq!(fs::read(&stored[0]).unwrap(), b"SECRET=1\n");
    for e in fs::read_dir(&dir).unwrap() {
        let name = e.unwrap().file_name().to_string_lossy().into_owned();
        assert!(
            !name.contains("env"),
            "{name} is derived from a workspace path"
        );
    }
    fs::write(&env, "SECRET=2\n").unwrap();
    c.feed(FsEvent::Modified(env));
    assert_eq!(mode(&dir.join("index")), 0o600);
    // A base that is a link is refused: nothing is written through it.
    let fx2 = Fx::new();
    fs::create_dir_all(&fx2.state).unwrap();
    symlink(&fx2.outside, fx2.state.join("ai")).unwrap();
    assert!(Changes::manual(fx2.config(Fx::limits())).is_err());
    assert_eq!(fs::read_dir(&fx2.outside).unwrap().count(), 0);
}

#[test]
fn sec10_caps_keep_the_store_bounded_and_say_so() {
    let fx = Fx::new();
    let big = fx.write("big.sql", vec![b'x'; 40]);
    let a = fx.write("a.txt", vec![b'a'; 20]);
    let b = fx.write("b.txt", vec![b'b'; 20]);
    let limits = Limits {
        file: 32,
        agent: 30,
        ..Fx::limits()
    };
    let c = fx.started(limits);
    assert_eq!(
        blobs(&c.store_dir()).len(),
        1,
        "one 20-byte copy fits 30 bytes"
    );
    for p in [&big, &a, &b] {
        fs::write(p, "changed").unwrap();
        c.feed(FsEvent::Modified(p.to_path_buf()));
    }
    let records = c.records();
    assert_eq!(rec(&records, "big.sql").missing, Some(NoEarlier::TooLarge));
    let missing: Vec<Option<NoEarlier>> = ["a.txt", "b.txt"]
        .iter()
        .map(|r| rec(&records, r).missing)
        .collect();
    assert!(missing.contains(&None) && missing.contains(&Some(NoEarlier::Limit)));
    assert!(fx.total.load(Ordering::Relaxed) <= 30);
    // The total is shared: a second session finds no room left.
    let fx_total = Limits {
        total: 20,
        ..Fx::limits()
    };
    let c2 = Changes::manual(fx.config(fx_total)).unwrap();
    c2.scan();
    assert!(blobs(&c2.store_dir()).is_empty());
    drop(c2);
    drop(c);
    assert_eq!(fx.total.load(Ordering::Relaxed), 0);
}

#[test]
fn sec10_the_list_stops_at_its_cap_with_a_banner() {
    let fx = Fx::new();
    let limits = Limits {
        records: 3,
        ..Fx::limits()
    };
    let c = fx.started(limits);
    for i in 0..6 {
        let p = fx.write(&format!("f{i}.txt"), "x");
        c.feed(FsEvent::Created(p));
    }
    assert_eq!(c.count(), 3);
    assert_eq!(c.banner().map(|b| b.why), Some(Incomplete::RecordLimit));
}

#[test]
fn sec10_state_config_git_and_own_temp_files_are_not_recorded() {
    let fx = Fx::new();
    // The session's folder holds the state folder (a home folder as the workspace).
    let fx = Fx {
        state: fx.root.join(".local/state/vr-ide"),
        ..fx
    };
    fs::create_dir_all(fx.root.join(".git")).unwrap();
    fx.write("a.txt", "1\n");
    let c = fx.started(Fx::limits());
    let store = c.store_dir();
    assert!(store.starts_with(&fx.root));
    let a = fx.write("a.txt", "2\n");
    c.feed(FsEvent::Modified(a));
    assert_eq!(c.count(), 1);
    // The store's own writes, git's internals and a save's temp file are events too.
    for p in [
        store.join("index"),
        blobs(&store)[0].clone(),
        fx.root.join(".git/index"),
        fx.root.join("sub/.git/HEAD"),
        fx.root.join(".vr-ide-save-1-2.tmp"),
    ] {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, "x").ok();
        c.feed(FsEvent::Modified(p.clone()));
        c.feed(FsEvent::Created(p));
    }
    c.feed(FsEvent::Created(fx.root.join(".local")));
    let rels: Vec<PathBuf> = c.records().into_iter().map(|r| r.rel).collect();
    assert_eq!(rels, vec![PathBuf::from("a.txt")]);
}

#[test]
fn sec10_index_paths_are_validated_on_load() {
    let fx = Fx::new();
    let p = fx.write("ok.txt", "1\n");
    let c = fx.started(Fx::limits());
    fs::write(&p, "2\n").unwrap();
    c.feed(FsEvent::Modified(p));
    let dir = c.store_dir();
    let loaded = load_index(&dir);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].rel, PathBuf::from("ok.txt"));
    assert!(loaded[0].can_revert());
    let hex = |s: &str| s.bytes().map(|b| format!("{b:02x}")).collect::<String>();
    let mut text = fs::read_to_string(dir.join("index")).unwrap();
    let good = text.lines().nth(1).unwrap().to_string();
    let with = |path: &str| {
        let cut = good.rfind('\t').unwrap();
        format!("{}\t{}\n", &good[..cut], hex(path))
    };
    for bad in ["../../x", "/etc/passwd", "a/../b", "", "./x", "ok.txt"] {
        text.push_str(&with(bad));
    }
    text.push_str("M\tnot a line\n");
    text.push_str(&with("sub/fine.txt"));
    fs::write(dir.join("index"), text).unwrap();
    let rels: Vec<PathBuf> = load_index(&dir).into_iter().map(|r| r.rel).collect();
    assert_eq!(
        rels,
        vec![PathBuf::from("ok.txt"), PathBuf::from("sub/fine.txt")]
    );
    // A blob of another size is not a pre-image.
    fs::write(&blobs(&dir)[0], "tampered with").unwrap();
    assert!(load_index(&dir).iter().all(|r| !r.can_revert()));
}

#[test]
fn sec10_keep_close_and_the_startup_sweep_clean_up() {
    let fx = Fx::new();
    let p = fx.write("a.txt", "1\n");
    let c = fx.started(Fx::limits());
    let dir = c.store_dir();
    fs::write(&p, "2\n").unwrap();
    c.feed(FsEvent::Modified(p));
    let before = blobs(&dir);
    assert!(fx.keep(&c, "a.txt"));
    // The pre-image is gone; what is stored now is the kept content.
    let after = blobs(&dir);
    assert!(before.iter().all(|b| !after.contains(b)));
    assert_eq!(fs::read(&after[0]).unwrap(), b"2\n");
    // A folder left by a vr-ide that is gone, a link and a live session's folder.
    let base = fx.state.join("ai");
    let stale = base.join("999-0-stale");
    fs::create_dir_all(&stale).unwrap();
    fs::write(stale.join("lock"), "").unwrap();
    fs::write(stale.join("0000000000000001.blob"), "old secret").unwrap();
    fs::write(fx.outside.join("keep.txt"), "mine").unwrap();
    symlink(&fx.outside, base.join("link")).unwrap();
    sweep(&base);
    assert!(!stale.exists());
    assert!(dir.exists(), "a held lock keeps a live store");
    assert!(fx.outside.join("keep.txt").exists());
    drop(c);
    assert!(!dir.exists(), "closing the session removes its store");
}

// ---- SEC-11 -----------------------------------------------------------------------------------

#[test]
fn sec11_only_regular_files_inside_the_root_are_read() {
    let fx = Fx::new();
    let secret = fx.outside.join("passwd");
    fs::write(&secret, "root:x:0:0\n").unwrap();
    symlink(&secret, fx.root.join("link")).unwrap();
    symlink(&fx.outside, fx.root.join("linkdir")).unwrap();
    let fifo = fx.root.join("pipe");
    rustix::fs::mknodat(
        rustix::fs::CWD,
        &fifo,
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::from_raw_mode(0o600),
        0,
    )
    .unwrap();
    fx.write("real.txt", "r\n");
    // Opening a FIFO for reading would wait for a writer for ever.
    let c = fx.started(Fx::limits());
    let stored: Vec<Vec<u8>> = blobs(&c.store_dir())
        .iter()
        .map(|b| fs::read(b).unwrap())
        .collect();
    assert_eq!(stored, vec![b"r\n".to_vec()]);
    // The link now points elsewhere, the pipe was replaced: listed, nothing read through.
    fs::remove_file(fx.root.join("link")).unwrap();
    symlink(fx.outside.join("other"), fx.root.join("link")).unwrap();
    c.feed(FsEvent::Created(fx.root.join("link")));
    c.feed(FsEvent::Modified(fifo));
    // Events of paths outside the root are nobody's.
    fs::write(&secret, "changed\n").unwrap();
    c.feed(FsEvent::Modified(secret));
    c.feed(FsEvent::Modified(fx.root.join("linkdir/passwd")));
    let records = c.records();
    assert_eq!(records.len(), 1, "{records:?}");
    let r = rec(&records, "link");
    assert!(!r.can_revert());
    let d = c.diff(Path::new("link")).unwrap();
    assert!(d.lines.iter().all(|l| !l.text.contains("root:x")));
    assert_eq!(blobs(&c.store_dir()).len(), 1);
}

// ---- SEC-12 -----------------------------------------------------------------------------------

#[test]
fn sec12_revert_is_refused_when_the_file_changed_since_the_diff() {
    let fx = Fx::new();
    let p = fx.write("a.txt", "old\n");
    let c = fx.started(Fx::limits());
    fs::write(&p, "agent 1\n").unwrap();
    c.feed(FsEvent::Modified(p.clone()));
    let rel = Path::new("a.txt");
    // Between the diff and the answer.
    let d = c.diff(rel).unwrap();
    fs::write(&p, "agent 2\n").unwrap();
    assert!(matches!(
        c.revert(rel, &d.seen, &fx.trash, &mut |_| {}),
        Err(RevertError::Changed)
    ));
    assert_eq!(fs::read(&p).unwrap(), b"agent 2\n");
    // Between the last check and the swap: the writer's bytes come back to the path.
    let d = c.diff(rel).unwrap();
    let mut seam = |step: Step| {
        if step == Step::BeforeSwap {
            fs::write(&p, "agent 3\n").unwrap();
        }
    };
    assert!(matches!(
        c.revert(rel, &d.seen, &fx.trash, &mut seam),
        Err(RevertError::Changed)
    ));
    assert_eq!(fs::read(&p).unwrap(), b"agent 3\n");
    assert_eq!(c.count(), 1, "the record and its pre-image stay");
    let names: Vec<String> = fs::read_dir(&fx.root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["a.txt"], "no temp file is left");
    // Shown again, it reverts.
    assert_eq!(fx.revert(&c, "a.txt").unwrap(), Reverted::Restored);
    assert_eq!(fs::read(&p).unwrap(), b"old\n");
    // A created file that changed is not trashed; a deleted one that is back is not replaced.
    let n = fx.write("n.txt", "1\n");
    c.feed(FsEvent::Created(n.clone()));
    let d = c.diff(Path::new("n.txt")).unwrap();
    fs::write(&n, "2\n").unwrap();
    assert!(matches!(
        c.revert(Path::new("n.txt"), &d.seen, &fx.trash, &mut |_| {}),
        Err(RevertError::Changed)
    ));
    // The trash holds what the Reverts above took out of the path (D18): the refused one's
    // own file and the version that was replaced. Nothing else.
    assert!(n.exists() && fx.trash.trashed().len() == 2);
    fs::remove_file(&p).unwrap();
    c.feed(FsEvent::Removed(p.clone()));
    let d = c.diff(rel).unwrap();
    fs::write(&p, "back\n").unwrap();
    assert!(matches!(
        c.revert(rel, &d.seen, &fx.trash, &mut |_| {}),
        Err(RevertError::Changed)
    ));
    assert_eq!(fs::read(&p).unwrap(), b"back\n");
}

#[test]
fn sec12_a_parent_swapped_for_a_link_is_refused() {
    let fx = Fx::new();
    let p = fx.write("sub/a.txt", "old\n");
    let c = fx.started(Fx::limits());
    fs::write(&p, "new\n").unwrap();
    c.feed(FsEvent::Modified(p.clone()));
    let rel = Path::new("sub/a.txt");
    // The same file name with the same bytes waits outside.
    fs::write(fx.outside.join("a.txt"), "new\n").unwrap();
    fs::rename(fx.root.join("sub"), fx.root.join("sub.real")).unwrap();
    symlink(&fx.outside, fx.root.join("sub")).unwrap();
    let seen = c.diff(rel);
    if let Ok(d) = seen {
        assert!(matches!(
            c.revert(rel, &d.seen, &fx.trash, &mut |_| {}),
            Err(RevertError::Outside)
        ));
    }
    assert_eq!(fs::read(fx.outside.join("a.txt")).unwrap(), b"new\n");
    // Swapped after the folder was opened and checked: the write stays in the real folder.
    fs::remove_file(fx.root.join("sub")).unwrap();
    fs::rename(fx.root.join("sub.real"), fx.root.join("sub")).unwrap();
    let d = c.diff(rel).unwrap();
    let mut seam = |step: Step| {
        if step == Step::BeforeSwap {
            fs::rename(fx.root.join("sub"), fx.root.join("sub.real")).unwrap();
            symlink(&fx.outside, fx.root.join("sub")).unwrap();
        }
    };
    assert_eq!(
        c.revert(rel, &d.seen, &fx.trash, &mut seam).unwrap(),
        Reverted::Restored
    );
    assert_eq!(fs::read(fx.outside.join("a.txt")).unwrap(), b"new\n");
    assert_eq!(fs::read(fx.root.join("sub.real/a.txt")).unwrap(), b"old\n");
    assert_eq!(fs::read_dir(&fx.outside).unwrap().count(), 1);
    // A folder moved out of the root before the swap is refused there.
    let q = fx.write("mv/b.txt", "old\n");
    c.feed(FsEvent::Created(fx.root.join("mv")));
    assert!(fx.keep(&c, "mv/b.txt"));
    fs::write(&q, "new\n").unwrap();
    c.feed(FsEvent::Modified(q));
    let d = c.diff(Path::new("mv/b.txt")).unwrap();
    let moved = fx.outside.join("mv");
    let mut first = true;
    let mut seam = |step: Step| {
        if step == Step::BeforeSwap && first {
            first = false;
        }
    };
    fs::rename(fx.root.join("mv"), &moved).unwrap();
    fs::create_dir(fx.root.join("mv")).unwrap();
    fs::write(fx.root.join("mv/b.txt"), "new\n").unwrap();
    // The path now names another folder inside the root with the same content: fine to
    // revert there, and the moved folder outside is untouched.
    c.revert(Path::new("mv/b.txt"), &d.seen, &fx.trash, &mut seam)
        .ok();
    assert_eq!(fs::read(moved.join("b.txt")).unwrap(), b"new\n");
}

#[test]
fn sec12_the_final_path_is_replaced_never_written_through() {
    let fx = Fx::new();
    let p = fx.write("run.sh", b"#!/bin/sh\n\x00\xff\r\nexit 0");
    fs::set_permissions(&p, fs::Permissions::from_mode(0o4755)).unwrap();
    let c = fx.started(Fx::limits());
    // The agent put a link to a file outside in its place.
    let victim = fx.outside.join("victim");
    fs::write(&victim, "do not touch\n").unwrap();
    fs::remove_file(&p).unwrap();
    symlink(&victim, &p).unwrap();
    c.feed(FsEvent::Created(p.clone()));
    assert_eq!(fx.revert(&c, "run.sh").unwrap(), Reverted::Restored);
    let meta = fs::symlink_metadata(&p).unwrap();
    assert!(meta.is_file(), "the link itself was replaced");
    // Byte-exact (no BOM, encoding or line-ending change); 0755 kept, setuid dropped.
    assert_eq!(fs::read(&p).unwrap(), b"#!/bin/sh\n\x00\xff\r\nexit 0");
    assert_eq!(meta.mode() & 0o7777, 0o755);
    assert_eq!(fs::read(&victim).unwrap(), b"do not touch\n");
    // A created link goes to the trash as a link.
    let l = fx.root.join("made");
    symlink(&victim, &l).unwrap();
    c.feed(FsEvent::Created(l.clone()));
    assert_eq!(fx.revert(&c, "made").unwrap(), Reverted::Trashed);
    assert!(fs::symlink_metadata(&l).is_err());
    assert_eq!(fs::read(&victim).unwrap(), b"do not touch\n");
}

struct NoTrash;

impl vr_fs::Trash for NoTrash {
    fn trash(&self, paths: &[PathBuf]) -> vr_fs::BatchResult {
        paths
            .iter()
            .map(|p| {
                (
                    p.clone(),
                    Err(vr_fs::FsOpError::Trash("no trash here".into())),
                )
            })
            .collect()
    }
}

#[test]
fn sec12_without_a_trash_a_created_file_stays() {
    let fx = Fx::new();
    let c = fx.started(Fx::limits());
    let n = fx.write("n.txt", "1\n");
    c.feed(FsEvent::Created(n.clone()));
    let d = c.diff(Path::new("n.txt")).unwrap();
    let got = c.revert(Path::new("n.txt"), &d.seen, &NoTrash, &mut |_| {});
    assert!(matches!(got, Err(RevertError::Failed(e)) if e.contains("no trash here")));
    assert_eq!(fs::read(&n).unwrap(), b"1\n");
    assert_eq!(c.count(), 1);
}

// ---- SEC-13 -----------------------------------------------------------------------------------

#[test]
fn sec13_a_watcher_error_or_rescan_marks_the_list() {
    let fx = Fx::new();
    let c = fx.started(Fx::limits());
    assert_eq!(c.banner(), None);
    c.feed(FsEvent::Error("inotify watch limit reached".into()));
    assert_eq!(c.banner().map(|b| b.why), Some(Incomplete::Watcher));
    c.dismiss_banner();
    assert_eq!(c.banner(), None);
    c.feed(FsEvent::Rescan(fx.root.clone()));
    assert_eq!(c.banner().map(|b| b.why), Some(Incomplete::Rescan));
    // Too many folders to watch is the same kind of news.
    fx.write("a/b/c/d.txt", "x");
    let few = Limits {
        dirs: 2,
        ..Fx::limits()
    };
    let c = fx.started(few);
    assert_eq!(c.banner().map(|b| b.why), Some(Incomplete::WatchLimit));
}

// ---- the real watcher -------------------------------------------------------------------------

#[test]
fn the_worker_thread_records_what_a_program_changes() {
    let fx = Fx::new();
    let m = fx.write("src/m.txt", "one\n");
    let d = fx.write("src/d.txt", "gone\n");
    std::thread::sleep(MARGIN * 2);
    let cfg = fx.config(Fx::limits());
    let wake = Arc::clone(&cfg.wake);
    let c = Changes::start(cfg).unwrap();
    let long = Duration::from_secs(10);
    assert!(wait_until(long, || !c.scanning()));
    fs::write(&m, "one\ntwo\n").unwrap();
    fs::remove_file(&d).unwrap();
    fx.write("new/dir/a.txt", "a\n");
    assert!(
        wait_until(long, || c.count() == 3
            && c.records()
                .iter()
                .all(|r| r.added.is_some() || r.removed.is_some())),
        "{:?}",
        c.records()
    );
    assert!(wake.load(Ordering::Relaxed));
    let records = c.records();
    assert_eq!(rec(&records, "src/m.txt").kind, Kind::Modified);
    assert_eq!(rec(&records, "src/d.txt").kind, Kind::Deleted);
    assert_eq!(rec(&records, "new/dir/a.txt").kind, Kind::Created);
    assert_eq!(fx.revert(&c, "src/m.txt").unwrap(), Reverted::Restored);
    assert_eq!(fs::read(&m).unwrap(), b"one\n");
    // The revert's own write does not come back as a change.
    std::thread::sleep(vr_core::DEBOUNCE * 3);
    assert_eq!(c.count(), 2);
    let dir = c.store_dir();
    drop(c);
    assert!(!dir.exists());
}

// ---- fix round 1: honest list (SEC-13) --------------------------------------------------------

fn why(c: &Changes) -> Option<Incomplete> {
    c.banner().map(|b| b.why)
}

#[test]
fn sec13_a_walk_that_ran_out_of_memory_marks_the_list() {
    let fx = Fx::new();
    for i in 0..4 {
        fx.write(&format!("d{i}/f.txt"), "x");
    }
    let few = Limits {
        known: 2,
        ..Fx::limits()
    };
    let c = fx.started(few);
    assert!(!c.scanning(), "the walk gave up");
    assert_eq!(why(&c), Some(Incomplete::WalkLimit));
    // It is news like any other: Enter hides it.
    c.dismiss_banner();
    assert_eq!(c.banner(), None);
}

#[test]
fn sec13_the_list_is_marked_until_the_walk_has_been_everywhere() {
    let fx = Fx::new();
    fx.write("a/b.txt", "x");
    std::thread::sleep(MARGIN * 2);
    let c = Changes::manual(fx.config(Fx::limits())).unwrap();
    assert_eq!(why(&c), Some(Incomplete::Scanning));
    c.dismiss_banner();
    assert_eq!(why(&c), Some(Incomplete::Scanning), "it ends with the walk");
    c.scan();
    assert_eq!(c.banner(), None);
    // The program exited and starts again: a new walk, and the list is drawn again.
    c.pause();
    assert_eq!(
        c.banner(),
        None,
        "nothing is attributed while it is not running"
    );
    let v = c.version();
    c.resume();
    assert_ne!(c.version(), v);
    assert_eq!(why(&c), Some(Incomplete::Scanning));
    // A cause with a time of its own shows first and outlives the walk.
    c.feed(FsEvent::Rescan(fx.root.clone()));
    assert_eq!(why(&c), Some(Incomplete::Rescan));
    c.scan();
    assert_eq!(why(&c), Some(Incomplete::Rescan));
}

// ---- fix round 1: baselines taken later (SEC-9) -----------------------------------------------

#[test]
fn sec9_a_write_right_after_an_own_save_is_not_taken_for_the_save() {
    let fx = Fx::new();
    let p = fx.write("a.txt", "old\n");
    let own = vr_core::OwnWrites::default();
    let mut cfg = fx.config(Fx::limits());
    cfg.own = Some(own.clone());
    std::thread::sleep(MARGIN * 2);
    let c = Changes::manual(cfg).unwrap();
    c.scan();
    // The editor saves the file ...
    let mut doc = vr_core::Document::open(&p, u64::MAX).unwrap();
    doc.set_own_writes(own.clone());
    doc.buffer = vr_core::Buffer::from_str("mine\n");
    doc.save().unwrap();
    assert_eq!(fs::read(&p).unwrap(), b"mine\n");
    // ... and the agent writes between the save's event and the read of the new baseline.
    let agent = p.clone();
    seam::before_baseline_read(move || fs::write(&agent, "agent\n").unwrap());
    c.feed(FsEvent::Modified(p.clone()));
    c.feed(FsEvent::Modified(p.clone()));
    let records = c.records();
    let r = rec(&records, "a.txt");
    assert_eq!(
        (r.kind, r.missing),
        (Kind::Modified, Some(NoEarlier::NotSeen)),
        "the agent's bytes are a change without an earlier version, never the baseline"
    );
    assert!(!r.can_revert());
    assert!(
        blobs(&c.store_dir()).is_empty(),
        "no copy of the agent's bytes"
    );
    // An own save nobody raced is the baseline of what comes next.
    assert!(fx.keep(&c, "a.txt"));
    doc.buffer = vr_core::Buffer::from_str("mine 2\n");
    doc.save_overwrite().unwrap();
    c.feed(FsEvent::Modified(p.clone()));
    assert_eq!(c.count(), 0);
    fs::write(&p, "agent 2\n").unwrap();
    c.feed(FsEvent::Modified(p.clone()));
    assert_eq!(fx.revert(&c, "a.txt").unwrap(), Reverted::Restored);
    assert_eq!(fs::read(&p).unwrap(), b"mine 2\n");
}

#[test]
fn keep_keeps_only_the_version_that_was_shown() {
    let fx = Fx::new();
    let p = fx.write("a.txt", "old\n");
    let c = fx.started(Fx::limits());
    let rel = Path::new("a.txt");
    fs::write(&p, "agent 1\n").unwrap();
    c.feed(FsEvent::Modified(p.clone()));
    // The file changed between the diff and Keep: nothing is kept, the earlier version stays.
    let d = c.diff(rel).unwrap();
    fs::write(&p, "agent 2\n").unwrap();
    assert!(matches!(c.keep(rel, &d.seen), Err(KeepError::Changed)));
    assert_eq!(c.count(), 1);
    assert!(rec(&c.records(), "a.txt").can_revert());
    assert_eq!(blobs(&c.store_dir()).len(), 1);
    // It changed between Keep's check and the read of the new baseline: the kept version is
    // gone unread, so what is there now is a change without an earlier version.
    let d = c.diff(rel).unwrap();
    let agent = p.clone();
    seam::before_baseline_read(move || fs::write(&agent, "agent 3\n").unwrap());
    assert!(c.keep(rel, &d.seen).is_ok());
    let records = c.records();
    let r = rec(&records, "a.txt");
    assert_eq!(
        (r.kind, r.missing),
        (Kind::Modified, Some(NoEarlier::NotSeen))
    );
    assert!(blobs(&c.store_dir()).is_empty());
    // Undisturbed, Keep makes the shown version the baseline; a kept deletion stays deleted.
    assert!(fx.keep(&c, "a.txt"));
    assert_eq!(c.count(), 0);
    fs::write(&p, "agent 4\n").unwrap();
    c.feed(FsEvent::Modified(p.clone()));
    assert_eq!(fx.revert(&c, "a.txt").unwrap(), Reverted::Restored);
    assert_eq!(fs::read(&p).unwrap(), b"agent 3\n");
    fs::remove_file(&p).unwrap();
    c.feed(FsEvent::Removed(p.clone()));
    let d = c.diff(rel).unwrap();
    fs::write(&p, "back\n").unwrap();
    assert!(matches!(c.keep(rel, &d.seen), Err(KeepError::Changed)));
    assert!(matches!(
        c.keep(Path::new("none.txt"), &d.seen),
        Err(KeepError::Gone)
    ));
}

// ---- fix round 1: Revert (SEC-12) -------------------------------------------------------------

#[test]
fn sec12_a_folder_at_the_path_is_never_moved_or_replaced() {
    let fx = Fx::new();
    let p = fx.write("a.txt", "old\n");
    let c = fx.started(Fx::limits());
    let rel = Path::new("a.txt");
    // The agent put a folder with its work where the file was.
    fs::remove_file(&p).unwrap();
    fs::create_dir(&p).unwrap();
    fs::write(p.join("work.txt"), "agent work\n").unwrap();
    c.feed(FsEvent::Created(p.clone()));
    let records = c.records();
    assert_eq!(rec(&records, "a.txt").kind, Kind::Deleted, "{records:?}");
    assert_eq!(rec(&records, "a.txt/work.txt").kind, Kind::Created);
    let d = c.diff(rel).unwrap();
    assert!(
        d.lines.iter().any(|l| l.text.contains("a folder")),
        "{:?}",
        d.lines.first()
    );
    assert!(matches!(
        c.revert(rel, &d.seen, &fx.trash, &mut |_| {}),
        Err(RevertError::Folder)
    ));
    assert_eq!(fs::read(p.join("work.txt")).unwrap(), b"agent work\n");
    let names = |dir: &Path| -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    };
    assert_eq!(names(&fx.root), ["a.txt"], "nothing was moved aside");
    assert_eq!(c.count(), 2, "the records stay");
    // The folder gone again, the file comes back.
    fs::remove_dir_all(&p).unwrap();
    assert_eq!(fx.revert(&c, "a.txt").unwrap(), Reverted::Restored);
    assert_eq!(fs::read(&p).unwrap(), b"old\n");
    // A folder swapped in after the last check: it comes back to the path untouched.
    fs::write(&p, "agent\n").unwrap();
    c.feed(FsEvent::Modified(p.clone()));
    let d = c.diff(rel).unwrap();
    let mut seam = |step: Step| {
        if step == Step::BeforeSwap {
            fs::remove_file(&p).unwrap();
            fs::create_dir(&p).unwrap();
            fs::write(p.join("late.txt"), "late\n").unwrap();
        }
    };
    assert!(matches!(
        c.revert(rel, &d.seen, &fx.trash, &mut seam),
        Err(RevertError::Changed)
    ));
    assert_eq!(fs::read(p.join("late.txt")).unwrap(), b"late\n");
    assert_eq!(names(&fx.root), ["a.txt"]);
    // A created file that is a folder now is not sent to the trash.
    let n = fx.write("n.txt", "1\n");
    c.feed(FsEvent::Created(n.clone()));
    fs::remove_file(&n).unwrap();
    fs::create_dir(&n).unwrap();
    let d = c.diff(Path::new("n.txt")).unwrap();
    assert!(matches!(
        c.revert(Path::new("n.txt"), &d.seen, &fx.trash, &mut |_| {}),
        Err(RevertError::Folder)
    ));
    // The trash holds vr-ide's own file from the refused Revert above (D18), no folder.
    let own = |t: &PathBuf| t.to_string_lossy().contains(".vr-ide-reverted-");
    assert!(n.is_dir() && fx.trash.trashed().iter().all(own));
}

#[test]
fn sec12_a_version_kept_aside_is_named_by_its_real_path() {
    let fx = Fx::new();
    let p = fx.write("a.txt", "old\n");
    let c = fx.started(Fx::limits());
    let rel = Path::new("a.txt");
    fs::write(&p, "agent 1\n").unwrap();
    c.feed(FsEvent::Modified(p.clone()));
    let d = c.diff(rel).unwrap();
    // One writer in place before the swap, another by rename while the first one's version
    // goes back: neither version may be deleted.
    let other = fx.root.join("w.tmp");
    let mut seam = |step: Step| match step {
        Step::BeforeSwap => fs::write(&p, "agent 2\n").unwrap(),
        Step::BeforeSwapBack => {
            fs::write(&other, "agent 3\n").unwrap();
            fs::rename(&other, &p).unwrap();
        }
        _ => {}
    };
    let Err(RevertError::Failed(msg)) = c.revert(rel, &d.seen, &fx.trash, &mut seam) else {
        panic!("a version had to be kept aside");
    };
    let at = kept_at(&msg).unwrap_or_else(|| panic!("{msg}"));
    assert!(at.starts_with(&fx.root), "a path the user can open: {msg}");
    assert_eq!(fs::read(&at).unwrap(), b"agent 3\n");
    assert_eq!(fs::read(&p).unwrap(), b"agent 2\n");
    assert_eq!(c.count(), 1);
}

/// The path at the end of a "kept at <path>" error.
fn kept_at(msg: &str) -> Option<PathBuf> {
    msg.rsplit_once("kept at ").map(|(_, p)| PathBuf::from(p))
}

/// A trash that lets another writer act right before the file is moved.
struct RacedTrash<'a> {
    inner: &'a FakeTrash,
    before: std::cell::RefCell<Option<Box<dyn FnOnce()>>>,
    /// Answers "done" without moving anything.
    idle: bool,
}

impl vr_fs::Trash for RacedTrash<'_> {
    fn trash(&self, paths: &[PathBuf]) -> vr_fs::BatchResult {
        if let Some(f) = self.before.borrow_mut().take() {
            f();
        }
        if self.idle {
            return paths.iter().map(|p| (p.clone(), Ok(()))).collect();
        }
        self.inner.trash(paths)
    }
}

#[test]
fn sec12_a_created_file_written_to_while_it_is_trashed_is_reported() {
    let fx = Fx::new();
    let c = fx.started(Fx::limits());
    let raced = |f: Box<dyn FnOnce()>, idle: bool| RacedTrash {
        inner: &fx.trash,
        before: std::cell::RefCell::new(Some(f)),
        idle,
    };
    // Written in place after the last check: it is in the trash (never unlinked), and the
    // answer says the trash holds a version that was not shown.
    let n = fx.write("n.txt", "1\n");
    c.feed(FsEvent::Created(n.clone()));
    let d = c.diff(Path::new("n.txt")).unwrap();
    let w = n.clone();
    let t = raced(Box::new(move || fs::write(&w, "2\n").unwrap()), false);
    assert_eq!(
        c.revert(Path::new("n.txt"), &d.seen, &t, &mut |_| {})
            .unwrap(),
        Reverted::TrashedChanged
    );
    assert_eq!(fx.trash.trashed(), std::slice::from_ref(&n));
    assert_eq!(c.count(), 0);
    // Replaced by a rename after the last check: the same answer.
    let m = fx.write("m.txt", "1\n");
    c.feed(FsEvent::Created(m.clone()));
    let d = c.diff(Path::new("m.txt")).unwrap();
    let (w, tmp) = (m.clone(), fx.root.join("w.tmp"));
    let t = raced(
        Box::new(move || {
            fs::write(&tmp, "2\n").unwrap();
            fs::rename(&tmp, &w).unwrap();
        }),
        false,
    );
    assert_eq!(
        c.revert(Path::new("m.txt"), &d.seen, &t, &mut |_| {})
            .unwrap(),
        Reverted::TrashedUnchecked
    );
    assert_eq!(fx.trash.trashed().len(), 2);
    // A trash that moved nothing: the file is still there and so is its record.
    let k = fx.write("k.txt", "1\n");
    c.feed(FsEvent::Created(k.clone()));
    let d = c.diff(Path::new("k.txt")).unwrap();
    let t = raced(Box::new(|| {}), true);
    // Whatever the trash did move was not this file (its folder may have been swapped for a
    // link in that instant): the message says so.
    assert!(matches!(
        c.revert(Path::new("k.txt"), &d.seen, &t, &mut |_| {}),
        Err(RevertError::Failed(e)) if e.contains("the trash may hold a different file")
    ));
    assert_eq!(fs::read(&k).unwrap(), b"1\n");
    assert_eq!(c.count(), 1);
    // Nobody in between: plainly trashed, a link as the link it is.
    assert_eq!(fx.revert(&c, "k.txt").unwrap(), Reverted::Trashed);
}

// ---- fix round 1: caps (SEC-10) ---------------------------------------------------------------

#[test]
fn sec10_the_total_counts_the_stores_of_other_running_vr_ides() {
    let fx = Fx::new();
    let a = fx.write("a.txt", vec![b'a'; 20]);
    // The store of another vr-ide process holds 500 bytes.
    let other = fx.state.join("ai/1-0-00000000000000aa");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("used"), "500\n").unwrap();
    let limits = Limits {
        total: 510,
        ..Fx::limits()
    };
    let c = fx.started(limits.clone());
    assert!(blobs(&c.store_dir()).is_empty(), "500 + 20 is over 510");
    fs::write(&a, "changed").unwrap();
    c.feed(FsEvent::Modified(a.clone()));
    assert_eq!(rec(&c.records(), "a.txt").missing, Some(NoEarlier::Limit));
    drop(c);
    // With room, this session's own use is written where the others read it.
    fs::write(other.join("used"), "400\n").unwrap();
    fs::write(&a, vec![b'a'; 20]).unwrap();
    let c = fx.started(limits);
    assert_eq!(blobs(&c.store_dir()).len(), 1);
    assert_eq!(
        fs::read_to_string(c.store_dir().join("used"))
            .unwrap()
            .trim(),
        "20"
    );
    assert_eq!(
        fs::metadata(c.store_dir().join("used")).unwrap().mode() & 0o777,
        0o600
    );
    // A damaged or hostile file of another store counts as nothing, and is not followed.
    fs::remove_file(other.join("used")).unwrap();
    symlink("/etc/passwd", other.join("used")).unwrap();
    let b = fx.write("b.txt", vec![b'b'; 20]);
    c.feed(FsEvent::Created(b.clone()));
    assert!(fx.keep(&c, "b.txt"));
    assert_eq!(blobs(&c.store_dir()).len(), 2);
}

/// The caps as shipped (SEC-10 verify): 9 MiB file, 6 000 created files.
#[test]
fn sec10_the_shipped_caps_hold_for_a_9_mib_file_and_6000_new_files() {
    let fx = Fx::new();
    let big = fx.write("dump.sql", vec![b'x'; 9 << 20]);
    let c = fx.started(Fx::limits());
    assert!(blobs(&c.store_dir()).is_empty(), "over 8 MiB: no copy");
    fs::write(&big, "changed").unwrap();
    c.feed(FsEvent::Modified(big.clone()));
    let records = c.records();
    let r = rec(&records, "dump.sql");
    assert_eq!(r.missing, Some(NoEarlier::TooLarge));
    assert!(!r.can_revert());
    assert!(
        c.keep(
            Path::new("dump.sql"),
            &c.diff(Path::new("dump.sql")).unwrap().seen
        )
        .is_ok()
    );
    let made: Vec<FsEvent> = (0..6_000)
        .map(|i| FsEvent::Created(fx.write(&format!("gen/f{i}.txt"), "x\n")))
        .collect();
    c.feed(FsEvent::Created(fx.root.join("gen")));
    c.feed_all(made);
    assert_eq!(c.count(), RECORD_CAP);
    assert_eq!(why(&c), Some(Incomplete::RecordLimit));
    let stored: u64 = blobs(&c.store_dir())
        .iter()
        .map(|p| fs::metadata(p).unwrap().len())
        .sum();
    assert!(stored <= AGENT_CAP && fx.total.load(Ordering::Relaxed) <= TOTAL_CAP);
}

// ---- fix round 1: Revert against a racing writer (SEC-12) -------------------------------------

/// How the racing program writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Writer {
    /// Into the file that is at the path (a link: unlink, then link again).
    InPlace,
    /// A new file (or link) under another name, renamed over the path.
    Rename,
}

const PRE: &[u8] = b"old\n";
const RACE_ROUNDS: u32 = 60;

/// Spins for a few microseconds, a different number each time: both sides do it so that the
/// writer's writes land before, between and after the steps of the swap.
fn jitter(rng: &mut u64, max_micros: u64) {
    *rng ^= *rng << 13;
    *rng ^= *rng >> 7;
    *rng ^= *rng << 17;
    let (from, wait) = (
        std::time::Instant::now(),
        Duration::from_micros(*rng % max_micros),
    );
    while from.elapsed() < wait {
        std::hint::spin_loop();
    }
}

/// What both sides of a race share: a clock that orders their steps, a counter the Revert
/// bumps at each step of its swap (the writer wakes on it, so its writes land around those
/// steps), and the Reverts tried in this round.
#[derive(Default)]
struct Race {
    clock: AtomicU64,
    go: AtomicU64,
    tries: AtomicU64,
}

impl Race {
    fn tick(&self) -> u64 {
        self.clock.fetch_add(1, Ordering::SeqCst) + 1
    }
}

/// One Revert of a round, on the shared clock.
struct Attempt {
    /// When our file was at the path (0: it never was).
    swapped: u64,
    ended: u64,
}

/// What the reverting side saw in one round.
#[derive(Default)]
struct Round {
    /// The version the one successful Revert replaced.
    replaced: Option<Seen>,
    /// Versions an error said it kept aside.
    kept: Vec<PathBuf>,
    attempts: Vec<Attempt>,
    refused: u32,
}

/// Reverts `rel` over and over while `busy` holds, then once more in peace. Every answer must
/// be one a user can act on; anything else fails the test.
fn revert_while(fx: &Fx, c: &Changes, rel: &Path, race: &Race, busy: &dyn Fn() -> bool) -> Round {
    let mut round = Round::default();
    let mut rng = race.tick() | 1;
    loop {
        let last = !busy();
        race.tries.fetch_add(1, Ordering::SeqCst);
        let d = match c.diff(rel) {
            Ok(d) => d,
            Err(e) if e == "no such change" && round.replaced.is_some() => {
                if last {
                    return round;
                }
                continue;
            }
            Err(e) => {
                assert!(e.contains("keeps changing"), "diff: {e}");
                assert!(!last, "a file at rest can be shown: {e}");
                continue;
            }
        };
        let mut swapped = 0;
        let got = c.revert(rel, &d.seen, &fx.trash, &mut |step| {
            if step == Step::AfterSwap {
                swapped = race.tick();
            }
            // The writer wakes here; the pause leaves it room before the next step.
            race.go.fetch_add(1, Ordering::SeqCst);
            jitter(&mut rng, 12);
        });
        round.attempts.push(Attempt {
            swapped,
            ended: race.tick(),
        });
        match got {
            Ok(Reverted::Restored) => {
                assert!(round.replaced.is_none(), "one record, one Revert");
                round.replaced = Some(d.seen);
            }
            Err(RevertError::Changed) => {
                assert!(!last, "a file at rest is what the diff showed");
                round.refused += 1;
            }
            Err(RevertError::Gone) => assert!(round.replaced.is_some()),
            Err(RevertError::Failed(e)) => match kept_at(&e) {
                Some(at) => {
                    assert!(fs::symlink_metadata(&at).is_ok(), "{e}");
                    round.kept.push(at);
                }
                None => assert!(e.contains("keeps changing") && !last, "{e}"),
            },
            other => panic!("{other:?}"),
        }
        if last {
            return round;
        }
    }
}

/// A version the writer put at the path, with a handle on the file (or link) that holds it.
struct Put {
    what: Vec<u8>,
    opened: u64,
    ended: u64,
    pin: Option<fs::File>,
}

impl Put {
    /// The file that held this version is linked nowhere any more. The writer never unlinks
    /// a version before this is asked, so a Revert did.
    fn unlinked(&self) -> bool {
        self.pin
            .as_ref()
            .is_some_and(|f| f.metadata().unwrap().nlink() == 0)
    }
}

/// The writing side of a round: version after version through `put`, each right after a step
/// of a Revert (or a pause), until enough Reverts were tried. Returns the versions whose file
/// a Revert unlinked while the writer ran, and the last one put.
fn write_while(
    race: &Race,
    first: Put,
    seed: u64,
    version: &(dyn Fn(u32) -> Vec<u8> + Sync),
    stage: &(dyn Fn(&[u8]) + Sync),
    put: &(dyn Fn(&[u8]) -> Option<fs::File> + Sync),
) -> (Vec<Put>, Put) {
    let (mut unlinked, mut last, mut k) = (Vec::new(), first, 0);
    let mut rng = seed | 1;
    let mut seen_go = race.go.load(Ordering::SeqCst);
    while (race.tries.load(Ordering::SeqCst) < 3 || k < 6) && k < 100_000 {
        // Whatever takes long is done before the wait: what follows a step is one call.
        k += 1;
        let what = version(k);
        stage(&what);
        let waiting = std::time::Instant::now();
        while race.go.load(Ordering::SeqCst) == seen_go
            && waiting.elapsed() < Duration::from_millis(8)
        {
            std::hint::spin_loop();
        }
        seen_go = race.go.load(Ordering::SeqCst);
        jitter(&mut rng, 12);
        if last.unlinked() {
            unlinked.push(std::mem::replace(
                &mut last,
                Put {
                    what: Vec::new(),
                    opened: 0,
                    ended: 0,
                    pin: None,
                },
            ));
        }
        let opened = race.tick();
        let pin = put(&what);
        last = Put {
            what,
            opened,
            ended: race.tick(),
            pin,
        };
    }
    (unlinked, last)
}

/// Entries of `dir` other than `allowed`.
fn strays(dir: &Path, allowed: &[PathBuf]) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| !allowed.contains(p))
        .collect()
}

/// What a round must have left, whatever the interleaving was:
/// - a version whose file a Revert unlinked is the version that Revert showed (`shown`), or,
///   for a writer in place only, a write through a handle opened before the swap that landed
///   after the swapped-out file passed its check: what no rename-based save can see
///   (safe_replace's module doc). Such a write ended after the swap of an attempt that was
///   still running when its file was opened;
/// - the newest version is at the path, or was replaced by the Revert that showed it, or is
///   in a file an error named.
fn check_round(
    round: u32,
    style: Writer,
    got: &Round,
    unlinked: &[Put],
    last: &Put,
    at_rest_is_last: bool,
    kept_holds_last: bool,
) -> u32 {
    let mut blind = 0;
    let name = |p: &Put| String::from_utf8_lossy(&p.what).trim().to_string();
    let last_gone = last.unlinked();
    for p in unlinked.iter().chain(last_gone.then_some(last)) {
        let shown = got.replaced.is_some_and(|s| s.shows(&p.what));
        let unseen = style == Writer::InPlace
            && got
                .attempts
                .iter()
                .any(|a| a.swapped != 0 && a.swapped < p.ended && p.opened < a.ended);
        assert!(
            shown || unseen,
            "round {round}: a Revert unlinked {}, which it had not shown",
            name(p)
        );
        blind += u32::from(!shown);
    }
    assert!(
        at_rest_is_last || last_gone || kept_holds_last,
        "round {round}: the newest version {} is nowhere",
        name(last)
    );
    blind
}

/// A regular file at the path while another program keeps writing it.
fn race_on_a_file(style: Writer) {
    let fx = Fx::new();
    let p = fx.write("a.txt", PRE);
    let side = fx.root.join("w.tmp");
    let c = fx.started(Fx::limits());
    let rel = Path::new("a.txt");
    let race = Race::default();
    // One whole version in one write; the handle names the file it went into.
    let put = |v: &[u8]| -> Option<fs::File> {
        use std::io::Write;
        match style {
            Writer::InPlace => {
                let mut f = fs::OpenOptions::new().write(true).open(&p).unwrap();
                f.write_all(v).unwrap();
                Some(f)
            }
            Writer::Rename => {
                let f = fs::File::open(&side).unwrap();
                fs::rename(&side, &p).unwrap();
                Some(f)
            }
        }
    };
    let stage = |v: &[u8]| {
        if style == Writer::Rename {
            fs::write(&side, v).unwrap();
        }
    };
    let (mut refused, mut swaps, mut aside, mut blind) = (0, 0, 0, 0);
    let mut trashed = 0;
    let mut kept_all: Vec<PathBuf> = Vec::new();
    for round in 0..RACE_ROUNDS {
        let version = |k: u32| format!("r{round:03}-k{k:06}\n").into_bytes();
        let what = version(0);
        stage(&what);
        let first = Put {
            pin: put(&what),
            what,
            opened: 0,
            ended: 0,
        };
        c.feed(FsEvent::Modified(p.clone()));
        assert_eq!(c.count(), 1, "round {round}");
        race.tries.store(0, Ordering::SeqCst);
        let (got, unlinked, last) = std::thread::scope(|s| {
            let w =
                s.spawn(|| write_while(&race, first, u64::from(round) + 1, &version, &stage, &put));
            let got = revert_while(&fx, &c, rel, &race, &|| !w.is_finished());
            let (unlinked, last) = w.join().unwrap();
            (got, unlinked, last)
        });
        let end = fs::read(&p).unwrap();
        let whole =
            end.len() == last.what.len() && end.starts_with(format!("r{round:03}-k").as_bytes());
        assert!(
            end == PRE || whole,
            "round {round}: torn or foreign: {end:?}"
        );
        let kept = got
            .kept
            .iter()
            .any(|k| fs::read(k).is_ok_and(|b| b == last.what));
        // D18: a version a Revert takes out of the path is never unlinked. It is in the
        // trash: the version that Revert showed, or (a writer in place only) a write through
        // a handle opened before the swap, which landed in the trash copy instead of nowhere.
        assert!(
            unlinked.is_empty() && !last.unlinked(),
            "round {round}: a Revert unlinked a version; it belongs in the trash"
        );
        let all = fx.trash.trashed().len();
        let mut trash_holds_last = false;
        for slot in trashed..all {
            let bytes = fs::read(fx.trash_dir.join(slot.to_string())).unwrap();
            let shown = got.replaced.is_some_and(|s| s.shows(&bytes));
            let late = style == Writer::InPlace
                && bytes.len() == last.what.len()
                && bytes.starts_with(format!("r{round:03}-k").as_bytes());
            // A refused Revert's own file, taken back out of the path.
            let ours = bytes == PRE;
            assert!(
                shown || late || ours,
                "round {round}: the trash holds {bytes:?}, which no Revert showed"
            );
            blind += u32::from(!shown && !ours);
            trash_holds_last |= bytes == last.what;
        }
        let swapped = got.attempts.iter().filter(|a| a.swapped != 0).count();
        assert!(
            all - trashed >= usize::from(got.replaced.is_some()) && all - trashed <= swapped,
            "round {round}: at most one trash entry per Revert that swapped"
        );
        trashed = all;
        assert!(
            end == last.what || kept || trash_holds_last,
            "round {round}: the newest version is nowhere"
        );
        assert_eq!(got.replaced.is_some(), c.count() == 0, "round {round}");
        refused += got.refused;
        swaps += got.attempts.iter().filter(|a| a.swapped != 0).count();
        aside += got.kept.len();
        kept_all.extend(got.kept);
        let mut allowed = vec![p.clone(), side.clone()];
        allowed.extend(kept_all.iter().cloned());
        assert_eq!(
            strays(&fx.root, &allowed),
            Vec::<PathBuf>::new(),
            "round {round}"
        );
    }
    eprintln!(
        "{style:?} on a file: {swaps} swaps, {refused} refused, {aside} kept aside, \
         {blind} late writes kept in the trash"
    );
    // At rest the earlier version comes back byte for byte.
    stage(b"at rest\n");
    drop(put(b"at rest\n"));
    c.feed(FsEvent::Modified(p.clone()));
    assert_eq!(fx.revert(&c, "a.txt").unwrap(), Reverted::Restored);
    assert_eq!(fs::read(&p).unwrap(), PRE);
}

/// A link at the path (the agent put it there) while another program keeps pointing it
/// somewhere else. Its targets lie outside the folder and are never written.
fn race_on_a_link(style: Writer) {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::OpenOptionsExt;
    let fx = Fx::new();
    let p = fx.write("run.sh", PRE);
    let side = fx.root.join("w.tmp");
    let targets: Vec<PathBuf> = (0..8).map(|i| fx.outside.join(format!("v{i}"))).collect();
    for (i, t) in targets.iter().enumerate() {
        fs::write(t, format!("victim {i}\n")).unwrap();
    }
    let c = fx.started(Fx::limits());
    let rel = Path::new("run.sh");
    let race = Race::default();
    // A handle on the link itself, when `at` is one.
    let pin = |at: &Path| -> Option<fs::File> {
        let flags = rustix::fs::OFlags::PATH | rustix::fs::OFlags::NOFOLLOW;
        fs::OpenOptions::new()
            .read(true)
            .custom_flags(flags.bits() as i32)
            .open(at)
            .ok()
            .filter(|f| f.metadata().is_ok_and(|m| m.file_type().is_symlink()))
    };
    let put = |to: &[u8]| -> Option<fs::File> {
        let to = Path::new(std::ffi::OsStr::from_bytes(to));
        match style {
            Writer::InPlace => loop {
                match fs::remove_file(&p) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => panic!("{e}"),
                }
                match symlink(to, &p) {
                    Ok(()) => break pin(&p),
                    // A Revert put the file back in the gap; like `ln -sf`, once more.
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(e) => panic!("{e}"),
                }
            },
            Writer::Rename => {
                let held = pin(&side);
                fs::rename(&side, &p).unwrap();
                held
            }
        }
    };
    let stage = |to: &[u8]| {
        if style == Writer::Rename {
            let _ = fs::remove_file(&side);
            symlink(Path::new(std::ffi::OsStr::from_bytes(to)), &side).unwrap();
        }
    };
    let (mut refused, mut aside) = (0, 0);
    let mut kept_all: Vec<PathBuf> = Vec::new();
    for round in 0..RACE_ROUNDS {
        let version = |k: u32| {
            let t = &targets[(round as usize * 3 + k as usize) % targets.len()];
            t.as_os_str().as_bytes().to_vec()
        };
        let what = version(0);
        stage(&what);
        let first = Put {
            pin: put(&what),
            what,
            opened: 0,
            ended: 0,
        };
        c.feed(FsEvent::Created(p.clone()));
        assert_eq!(c.count(), 1, "round {round}");
        race.tries.store(0, Ordering::SeqCst);
        let (got, unlinked, last) = std::thread::scope(|s| {
            let w =
                s.spawn(|| write_while(&race, first, u64::from(round) + 1, &version, &stage, &put));
            let got = revert_while(&fx, &c, rel, &race, &|| !w.is_finished());
            let (unlinked, last) = w.join().unwrap();
            (got, unlinked, last)
        });
        let meta = fs::symlink_metadata(&p).unwrap();
        let at_rest = if meta.is_file() {
            assert_eq!(fs::read(&p).unwrap(), PRE, "round {round}");
            None
        } else {
            let to = fs::read_link(&p).unwrap();
            assert!(targets.contains(&to), "round {round}: {to:?}");
            Some(to.as_os_str().as_bytes().to_vec())
        };
        let kept = got
            .kept
            .iter()
            .any(|k| fs::read_link(k).is_ok_and(|t| t.as_os_str().as_bytes() == last.what));
        // A link is never written into: no write can go unseen, whatever the style.
        let blind = check_round(
            round,
            Writer::Rename,
            &got,
            &unlinked,
            &last,
            at_rest.as_ref() == Some(&last.what),
            kept,
        );
        assert_eq!(blind, 0);
        refused += got.refused;
        aside += got.kept.len();
        kept_all.extend(got.kept);
        let mut allowed = vec![p.clone(), side.clone()];
        allowed.extend(kept_all.iter().cloned());
        assert_eq!(
            strays(&fx.root, &allowed),
            Vec::<PathBuf>::new(),
            "round {round}"
        );
        // Nothing was written through a link, nothing was put outside.
        for (i, t) in targets.iter().enumerate() {
            assert_eq!(fs::read(t).unwrap(), format!("victim {i}\n").as_bytes());
        }
        assert_eq!(fs::read_dir(&fx.outside).unwrap().count(), targets.len());
    }
    eprintln!("{style:?} on a link: {refused} refused, {aside} kept aside");
    stage(targets[0].as_os_str().as_bytes());
    drop(put(targets[0].as_os_str().as_bytes()));
    c.feed(FsEvent::Created(p.clone()));
    assert_eq!(fx.revert(&c, "run.sh").unwrap(), Reverted::Restored);
    assert!(fs::symlink_metadata(&p).unwrap().is_file());
    assert_eq!(fs::read(&p).unwrap(), PRE);
}

#[test]
fn sec12_revert_races_a_writer_in_place_on_a_file() {
    race_on_a_file(Writer::InPlace);
}

#[test]
fn sec12_revert_races_a_writer_by_rename_on_a_file() {
    race_on_a_file(Writer::Rename);
}

#[test]
fn sec12_revert_races_a_writer_in_place_on_a_link() {
    race_on_a_link(Writer::InPlace);
}

#[test]
fn sec12_revert_races_a_writer_by_rename_on_a_link() {
    race_on_a_link(Writer::Rename);
}

// ---- fix round 2: a walk that was not everywhere in time (SEC-13) -----------------------------

#[test]
fn sec13_a_walk_the_program_did_not_wait_for_leaves_the_list_marked() {
    let fx = Fx::new();
    fx.write("proj/sub/a.txt", "x");
    std::thread::sleep(MARGIN * 2);
    let c = Changes::manual(fx.config(Fx::limits())).unwrap();
    // The program exits before the walk reached its folders: what it did there is unseen.
    c.pause();
    assert_eq!(why(&c), Some(Incomplete::Unwalked));
    // The next run's walk does not find those changes either: the notice stays ...
    std::thread::sleep(MARGIN * 2);
    c.resume();
    c.scan();
    assert_eq!(why(&c), Some(Incomplete::Unwalked));
    // ... until Enter.
    c.dismiss_banner();
    assert_eq!(c.banner(), None);
    // After a walk that was everywhere an exit leaves nothing to say.
    c.pause();
    assert_eq!(c.banner(), None);
    // Started again while the walk of the run before still ran.
    c.resume();
    c.resume();
    assert_eq!(why(&c), Some(Incomplete::Unwalked));
}

#[test]
fn sec13_a_folder_changed_before_its_watch_marks_the_list() {
    let fx = Fx::new();
    let gone = fx.write("late/gone.txt", "x");
    fx.write("calm/stay.txt", "x");
    std::thread::sleep(MARGIN * 2);
    let c = Changes::manual(fx.config(Fx::limits())).unwrap();
    // The program deletes a file in a folder the walk has not put its watch on yet.
    fs::remove_file(&gone).unwrap();
    c.scan();
    assert_eq!(c.count(), 0, "nothing tells that the file was ever there");
    assert_eq!(why(&c), Some(Incomplete::WalkedLate));
    c.dismiss_banner();
    assert_eq!(c.banner(), None);
    // Folders nobody touched since the start say nothing.
    c.pause();
    std::thread::sleep(MARGIN * 2);
    c.resume();
    c.scan();
    assert_eq!(c.banner(), None);
}

// ---- fix round 2: the replaced version goes to the trash (D18) --------------------------------

#[test]
fn sec12_the_replaced_version_goes_to_the_trash_with_a_late_write() {
    use std::io::Write;
    let fx = Fx::new();
    let p = fx.write("a.txt", "old\n");
    let c = fx.started(Fx::limits());
    fs::write(&p, "agent 1\n").unwrap();
    c.feed(FsEvent::Modified(p.clone()));
    let d = c.diff(Path::new("a.txt")).unwrap();
    // A program opened the file before the swap and writes after the swapped-out file passed
    // its check: no check can see that write, so the file it lands in must not be deleted.
    let mut late = fs::OpenOptions::new().write(true).open(&p).unwrap();
    let got = c.revert(Path::new("a.txt"), &d.seen, &fx.trash, &mut |step| {
        if step == Step::BeforeDeleteOld {
            late.write_all(b"late 22\n").unwrap();
        }
    });
    assert_eq!(got.unwrap(), Reverted::Restored);
    assert_eq!(fs::read(&p).unwrap(), b"old\n");
    assert_eq!(late.metadata().unwrap().nlink(), 1, "never unlinked");
    let trashed = fx.trash.trashed();
    assert_eq!(trashed.len(), 1, "one trash entry per Revert");
    let name = trashed[0].file_name().unwrap().to_string_lossy();
    assert!(name.starts_with("a.txt.vr-ide-reverted-"), "{name}");
    assert_eq!(trashed[0].parent(), Some(fx.root.as_path()));
    assert_eq!(fs::read(fx.trash_dir.join("0")).unwrap(), b"late 22\n");
    assert_eq!(
        strays(&fx.root, std::slice::from_ref(&p)),
        Vec::<PathBuf>::new()
    );
    assert_eq!(c.count(), 0);
}

/// The other file a Revert would delete: its own, put back out of the path because the file
/// had changed. A program that opened the path in that instant holds it too.
#[test]
fn sec12_a_refused_reverts_own_file_goes_to_the_trash_with_a_late_write() {
    use std::io::Write;
    let fx = Fx::new();
    let p = fx.write("a.txt", "old\n");
    let c = fx.started(Fx::limits());
    fs::write(&p, "agent 1\n").unwrap();
    c.feed(FsEvent::Modified(p.clone()));
    let d = c.diff(Path::new("a.txt")).unwrap();
    let mut late: Option<fs::File> = None;
    let got = c.revert(
        Path::new("a.txt"),
        &d.seen,
        &fx.trash,
        &mut |step| match step {
            Step::BeforeSwap => fs::write(&p, "agent 2\n").unwrap(),
            Step::AfterSwap => late = Some(fs::OpenOptions::new().write(true).open(&p).unwrap()),
            Step::BeforeDeleteOurs => late.as_mut().unwrap().write_all(b"late 22\n").unwrap(),
            _ => {}
        },
    );
    assert!(matches!(got, Err(RevertError::Changed)), "{got:?}");
    assert_eq!(fs::read(&p).unwrap(), b"agent 2\n");
    assert_eq!(
        late.unwrap().metadata().unwrap().nlink(),
        1,
        "never unlinked"
    );
    assert_eq!(fx.trash.trashed().len(), 1);
    assert_eq!(fs::read(fx.trash_dir.join("0")).unwrap(), b"late 22\n");
    assert_eq!(
        strays(&fx.root, std::slice::from_ref(&p)),
        Vec::<PathBuf>::new()
    );
    assert_eq!(c.count(), 1);
    // Without a trash its own file is deleted as before: nothing of vr-ide's stays behind.
    let d = c.diff(Path::new("a.txt")).unwrap();
    let got = c.revert(Path::new("a.txt"), &d.seen, &NoTrash, &mut |step| {
        if step == Step::BeforeSwap {
            fs::write(&p, "agent 3\n").unwrap();
        }
    });
    assert!(matches!(got, Err(RevertError::Changed)), "{got:?}");
    assert_eq!(
        strays(&fx.root, std::slice::from_ref(&p)),
        Vec::<PathBuf>::new()
    );
}

#[test]
fn sec12_without_a_trash_the_replaced_version_is_kept_by_name() {
    let fx = Fx::new();
    let p = fx.write("a.txt", "old\n");
    let q = fx.write("b.txt", "old\n");
    let c = fx.started(Fx::limits());
    for f in [&p, &q] {
        fs::write(f, "agent 1\n").unwrap();
        c.feed(FsEvent::Modified(f.to_path_buf()));
    }
    let idle = RacedTrash {
        inner: &fx.trash,
        before: std::cell::RefCell::new(None),
        idle: true,
    };
    let trashes: [(&PathBuf, &dyn vr_fs::Trash); 2] = [(&p, &NoTrash), (&q, &idle)];
    for (f, trash) in trashes {
        let rel = Path::new(f.file_name().unwrap());
        let d = c.diff(rel).unwrap();
        let got = c.revert(rel, &d.seen, trash, &mut |_| {}).unwrap();
        let Reverted::RestoredOldKept(at) = got else {
            panic!("{got:?}");
        };
        assert_eq!(fs::read(f).unwrap(), b"old\n");
        assert_eq!(fs::read(&at).unwrap(), b"agent 1\n");
        assert_eq!(at.parent(), Some(fx.root.as_path()));
        // The file vr-ide left there is not a change of the agent.
        c.feed(FsEvent::Created(at));
    }
    assert_eq!(c.count(), 0);
    assert!(fx.trash.trashed().is_empty());
}

// ---- fix round 2: an own save the agent wrote over (D8, SEC-9) --------------------------------

#[test]
fn sec9_the_version_before_an_own_save_is_not_what_revert_writes_back() {
    let fx = Fx::new();
    let p = fx.write("a.txt", "old\n");
    let q = fx.write("b.txt", "old b\n");
    let own = vr_core::OwnWrites::default();
    let mut cfg = fx.config(Fx::limits());
    cfg.own = Some(own.clone());
    std::thread::sleep(MARGIN * 2);
    let c = Changes::manual(cfg).unwrap();
    c.scan();
    // The editor saves the file, and the agent writes before the save's event is looked at:
    // nothing shows any more that there was a save.
    let mut doc = vr_core::Document::open(&p, u64::MAX).unwrap();
    doc.set_own_writes(own.clone());
    doc.buffer = vr_core::Buffer::from_str("mine\n");
    c.own_save(&p, true);
    doc.save().unwrap();
    fs::write(&p, "agent\n").unwrap();
    c.feed(FsEvent::Modified(p.clone()));
    let records = c.records();
    let r = rec(&records, "a.txt");
    assert_eq!(
        (r.kind, r.missing),
        (Kind::Modified, Some(NoEarlier::NotSeen)),
        "the walk's copy is older than the user's save: writing it back would undo the save"
    );
    assert!(!r.can_revert());
    // A save that wrote nothing leaves the walk's copy in use.
    c.own_save(&q, true);
    c.own_save(&q, false);
    fs::write(&q, "agent\n").unwrap();
    c.feed(FsEvent::Modified(q.clone()));
    assert_eq!(fx.revert(&c, "b.txt").unwrap(), Reverted::Restored);
    assert_eq!(fs::read(&q).unwrap(), b"old b\n");
}
