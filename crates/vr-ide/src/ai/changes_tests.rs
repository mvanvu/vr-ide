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
    assert!(c.keep(Path::new("early.txt")));
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
    assert!(c.keep(Path::new("pkg/b.txt")));
    assert_eq!(c.count(), 0);
    // After Keep, the file as it was kept is the earlier version of the next change.
    let k = fx.write("k.txt", "v1\n");
    c.feed(FsEvent::Created(k.clone()));
    assert!(c.keep(Path::new("k.txt")));
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
    assert!(c.keep(Path::new("a.txt")));
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
    assert!(n.exists() && fx.trash.trashed().is_empty());
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
    assert!(c.keep(Path::new("mv/b.txt")));
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
