//! Crash recovery (C6): on a panic, every modified buffer is written to
//! `state_dir()/recovery/<timestamp>-<sanitized path>`. Never into the project.
//!
//! The panic hook cannot borrow the `App`, so the event loop publishes cheap rope clones
//! (ropey shares nodes) of the modified buffers before each frame. The same writer keeps
//! buffers the user discards (quit / close without saving) and the buffers left when the
//! event loop fails.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::thread::ThreadId;
use std::time::Duration;

use vr_core::Document;
use vr_core::ropey::Rope;

/// Bytes of the sanitized path kept in a recovery file name. With the stamp and a counter the
/// name stays well under the 255-byte limit of common file systems; a smaller limit is caught
/// by the `untitled` fallback name.
pub const NAME_BUDGET: usize = 200;

#[derive(Debug, Clone)]
pub struct Entry {
    pub path: Option<PathBuf>,
    pub text: Rope,
}

impl Entry {
    fn of(d: &Document) -> Entry {
        Entry {
            path: d.path.clone(),
            text: d.buffer.text().clone(),
        }
    }
}

/// The modified buffers as of the last publish.
#[derive(Debug, Default)]
pub struct RecoveryState(Mutex<Vec<Entry>>);

impl RecoveryState {
    pub const fn new() -> Self {
        Self(Mutex::new(Vec::new()))
    }

    /// Records the modified documents (rope clones share nodes, so this is cheap).
    pub fn publish(&self, docs: &[Document]) {
        let entries = docs
            .iter()
            .filter(|d| d.is_modified())
            .map(Entry::of)
            .collect();
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = entries;
    }

    /// The published entries. Never blocks for long: a panic hook may run while another
    /// thread holds the lock for its short store, or on the thread that holds it.
    pub fn snapshot(&self) -> Vec<Entry> {
        for _ in 0..50 {
            match self.0.try_lock() {
                Ok(g) => return g.clone(),
                Err(std::sync::TryLockError::Poisoned(p)) => return p.into_inner().clone(),
                Err(std::sync::TryLockError::WouldBlock) => {
                    std::thread::sleep(Duration::from_millis(2))
                }
            }
        }
        Vec::new()
    }
}

/// The process-wide state the panic hook reads.
pub static GLOBAL: RecoveryState = RecoveryState::new();

/// `YYYYMMDDTHHMMSSZ` for seconds since the Unix epoch (civil-from-days, proleptic Gregorian).
pub fn utc_stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

/// The document path flattened into one file-name component of at most `budget` bytes,
/// keeping the end (the file name) and cutting at a char boundary.
fn sanitize(path: Option<&Path>, budget: usize) -> String {
    let Some(p) = path else {
        return "untitled".to_string();
    };
    let s: String = p
        .to_string_lossy()
        .trim_start_matches('/')
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let mut start = s.len().saturating_sub(budget);
    while !s.is_char_boundary(start) {
        start += 1;
    }
    s[start..].to_string()
}

/// Writes the published buffers to `dir` (see [`write_entries`]).
pub fn write_recovery(state: &RecoveryState, dir: &Path) -> Vec<PathBuf> {
    write_entries(&state.snapshot(), dir, NAME_BUDGET)
}

/// Writes `docs` to `dir` whether or not they are modified (callers pick them).
pub fn write_docs(docs: &[&Document], dir: &Path) -> Vec<PathBuf> {
    let entries: Vec<Entry> = docs.iter().map(|d| Entry::of(d)).collect();
    write_entries(&entries, dir, NAME_BUDGET)
}

/// Writes `entries` to `dir` (created 0700; files 0600, never replacing an existing file)
/// and returns the files written. A name the file system refuses falls back to
/// `<stamp>-untitled`. Errors are skipped: a panic hook must not panic again.
pub fn write_entries(entries: &[Entry], dir: &Path, name_budget: usize) -> Vec<PathBuf> {
    if entries.is_empty() || create_private_dir(dir).is_err() {
        return Vec::new();
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let stamp = utc_stamp(secs);
    let mut written = Vec::new();
    for e in entries {
        let bases = [
            sanitize(e.path.as_deref(), name_budget),
            "untitled".to_string(),
        ];
        'bases: for base in &bases {
            for n in 0..100 {
                let name = if n == 0 {
                    format!("{stamp}-{base}")
                } else {
                    format!("{stamp}-{n}-{base}")
                };
                let target = dir.join(name);
                match create_private_file(&target) {
                    Ok(mut f) => {
                        if write_rope(&mut f, &e.text).is_ok() {
                            written.push(target);
                        }
                        break 'bases;
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(_) => continue 'bases,
                }
            }
        }
    }
    written
}

fn write_rope(f: &mut std::fs::File, text: &Rope) -> std::io::Result<()> {
    use std::io::Write;
    for chunk in text.chunks() {
        f.write_all(chunk.as_bytes())?;
    }
    f.sync_all()
}

fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    let mut b = std::fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut b, 0o700);
    b.create(dir)
}

fn create_private_file(p: &Path) -> std::io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    o.open(p)
}

/// Panic hook for the calling (main) thread; see [`install_hook_for`].
pub fn install_panic_hook(dir: PathBuf, restore_terminal: fn()) {
    install_hook_for(&GLOBAL, dir, restore_terminal, std::thread::current().id());
}

/// Panic hook: a panic on `main` (the thread that owns the terminal and the buffers)
/// restores the terminal first so messages are readable, saves the buffers, then hands over
/// to the previous hook. Panics on other threads (file watcher, clipboard) only reach the
/// previous hook: the editor keeps running and its terminal must stay as it is.
pub fn install_hook_for(
    state: &'static RecoveryState,
    dir: PathBuf,
    restore_terminal: fn(),
    main: ThreadId,
) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().id() == main {
            restore_terminal();
            for p in write_recovery(state, &dir) {
                eprintln!("vr-ide: unsaved buffer written to {}", p.display());
            }
        }
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panic_hook_writes_recovery() {
        let project = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        let a = project.path().join("src/main.rs");
        std::fs::create_dir_all(a.parent().unwrap()).unwrap();
        std::fs::write(&a, "fn main() {}\r\n").unwrap();
        let b = project.path().join("clean.txt");
        std::fs::write(&b, "untouched").unwrap();
        let mut da = Document::open(&a, u64::MAX).unwrap();
        da.buffer.insert_text("// Xin chào 👋\r\n");
        let db = Document::open(&b, u64::MAX).unwrap();

        let rs = RecoveryState::new();
        rs.publish(&[da, db]);
        let dir = state.path().join("recovery");
        let written = write_recovery(&rs, &dir);

        assert_eq!(written.len(), 1, "only modified buffers: {written:?}");
        let f = &written[0];
        assert!(f.starts_with(&dir), "never into the project: {f:?}");
        let name = f.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.ends_with("src_main.rs"), "{name}");
        assert!(!name.contains('/'));
        assert_eq!(
            std::fs::read_to_string(f).unwrap(),
            "// Xin chào 👋\r\nfn main() {}\r\n"
        );
        // The project itself is unchanged.
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "fn main() {}\r\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(f).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        // A second crash in the same second does not overwrite the first file.
        let again = write_recovery(&rs, &dir);
        assert_eq!(again.len(), 1);
        assert_ne!(again[0], written[0]);
    }

    fn modified(path: Option<PathBuf>, text: &str) -> Document {
        let mut d = Document::untitled();
        d.buffer.insert_text(text);
        d.path = path;
        assert!(d.is_modified());
        d
    }

    #[test]
    fn long_accented_path_name_fits_the_file_name_limit() {
        let state = tempfile::tempdir().unwrap();
        let long = format!("/home/{}tệp-cuối.rs", "thư-mục-tiếng-việt/".repeat(12));
        assert!(long.chars().count() > 200 && long.len() > 255);
        let docs = [
            modified(Some(PathBuf::from(&long)), "một\n"),
            modified(None, "hai\n"),
        ];
        let written = write_docs(&docs.iter().collect::<Vec<_>>(), &state.path().join("r"));
        assert_eq!(written.len(), 2, "{written:?}");
        let name = written[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(name.len() <= 255, "{} bytes", name.len());
        assert!(name.ends_with("tệp-cuối.rs"), "{name}");
        assert_eq!(std::fs::read_to_string(&written[0]).unwrap(), "một\n");
    }

    #[test]
    fn refused_name_falls_back_to_a_short_one() {
        let state = tempfile::tempdir().unwrap();
        let long = format!("/{}.rs", "x".repeat(400));
        let doc = modified(Some(PathBuf::from(long)), "kept\n");
        let rs = RecoveryState::new();
        rs.publish(std::slice::from_ref(&doc));
        // A name budget past the file-system limit makes the first name fail (ENAMETOOLONG).
        let written = write_entries(&rs.snapshot(), &state.path().join("r"), 400);
        assert_eq!(written.len(), 1, "{written:?}");
        let name = written[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(name.ends_with("-untitled"), "{name}");
        assert_eq!(std::fs::read_to_string(&written[0]).unwrap(), "kept\n");
    }

    static RESTORED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    fn count_restore() {
        RESTORED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    #[test]
    fn panic_hook_acts_only_for_the_main_thread() {
        let state = tempfile::tempdir().unwrap();
        let dir = state.path().join("r");
        let rs: &'static RecoveryState = Box::leak(Box::new(RecoveryState::new()));
        rs.publish(&[modified(None, "x\n")]);
        install_hook_for(rs, dir.clone(), count_restore, std::thread::current().id());
        let worker = std::thread::spawn(|| panic!("worker thread panic (expected by the test)"));
        assert!(worker.join().is_err());
        let after_worker = RESTORED.load(std::sync::atomic::Ordering::SeqCst);
        let files_after_worker = std::fs::read_dir(&dir).map_or(0, |d| d.count());
        let main = std::panic::catch_unwind(|| panic!("main thread panic (expected by the test)"));
        let _ = std::panic::take_hook();
        assert!(main.is_err());
        assert_eq!(
            (after_worker, files_after_worker),
            (0, 0),
            "worker panic acted"
        );
        assert_eq!(RESTORED.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    }

    #[test]
    fn timestamp_is_utc_calendar_time() {
        assert_eq!(utc_stamp(0), "19700101T000000Z");
        assert_eq!(utc_stamp(1_795_478_400), "20261124T000000Z");
        assert_eq!(utc_stamp(951_782_400 + 3661), "20000229T010101Z");
    }
}
