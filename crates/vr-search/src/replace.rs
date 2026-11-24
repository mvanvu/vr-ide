//! Project replace: a per-file preview first, then writing only the confirmed files.
//!
//! Files open in a tab are not written here: vr-ide applies `replace_all_tx` to the buffer
//! (undoable) and sends only the other files to `apply_replace`.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::hash::{DefaultHasher, Hasher};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use vr_core::safe_replace::{self, Checks, Outcome, Step, exchange_files};

use crate::Query;
use crate::find::{Matcher, line_spans};
use crate::project::{MAX_FILE_BYTES, read_capped, size_text};

const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// One changed line: its 1-based number and its text (without the line break) before and
/// after the replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub line: usize,
    pub before: String,
    pub after: String,
}

/// What replacing in one file would do. `error` is set when the file cannot be replaced
/// (unreadable, too large, binary, not UTF-8, outside the root, invalid query); its hunks are
/// empty. Only a fingerprint of the file is kept, not its bytes, so many previews stay small;
/// `apply_replace` re-reads the file, refuses it if the fingerprint differs and recomputes the
/// same replacement from the bytes it read.
#[derive(Debug, Clone)]
pub struct FilePreview {
    pub path: PathBuf,
    pub hunks: Vec<Hunk>,
    pub error: Option<String>,
    replacements: usize,
    fingerprint: (u64, u64),
    root: PathBuf,
    spec: Option<Arc<Spec>>,
}

#[derive(Debug)]
struct Spec {
    matcher: Matcher,
    replacement: String,
}

impl PartialEq for FilePreview {
    fn eq(&self, other: &Self) -> bool {
        (
            &self.path,
            &self.hunks,
            &self.error,
            self.replacements,
            self.fingerprint,
        ) == (
            &other.path,
            &other.hunks,
            &other.error,
            other.replacements,
            other.fingerprint,
        )
    }
}

impl Eq for FilePreview {}

impl FilePreview {
    fn failed(path: PathBuf, error: impl Into<String>) -> Self {
        Self {
            path,
            hunks: Vec::new(),
            error: Some(error.into()),
            replacements: 0,
            fingerprint: (0, 0),
            root: PathBuf::new(),
            spec: None,
        }
    }

    /// Number of matches replaced in this file.
    pub fn replacements(&self) -> usize {
        self.replacements
    }
}

/// Length and SipHash of `bytes`: detects a file changed since the preview.
fn fingerprint(bytes: &[u8]) -> (u64, u64) {
    let mut h = DefaultHasher::new();
    h.write(bytes);
    (bytes.len() as u64, h.finish())
}

/// Previews the replacement in each of `files` (absolute, or relative to `root`). Nothing is
/// written. Line breaks, a UTF-8 BOM and every byte outside the matches stay as they are.
/// Files larger than 32 MB are refused with an error item.
pub fn plan_replace(
    root: &Path,
    q: &Query,
    replacement: &str,
    files: &[PathBuf],
) -> Vec<FilePreview> {
    plan_replace_with(root, q, replacement, files, MAX_FILE_BYTES)
}

fn plan_replace_with(
    root: &Path,
    q: &Query,
    replacement: &str,
    files: &[PathBuf],
    max_file_bytes: u64,
) -> Vec<FilePreview> {
    let spec = Matcher::new(q).map(|m| {
        m.map(|matcher| {
            Arc::new(Spec {
                matcher,
                replacement: replacement.to_string(),
            })
        })
    });
    let canon_root = fs::canonicalize(root);
    files
        .iter()
        .map(|f| {
            let path = root.join(f);
            let spec = match &spec {
                Ok(Some(s)) => s,
                Ok(None) => return FilePreview::failed(path, "empty search pattern"),
                Err(e) => return FilePreview::failed(path, e.to_string()),
            };
            let canon_root = match &canon_root {
                Ok(r) if inside(&path, r) => r,
                Ok(_) => return FilePreview::failed(path, "not a file inside the project"),
                Err(e) => return FilePreview::failed(path, format!("project root: {e}")),
            };
            let bytes = match read_capped(&path, max_file_bytes) {
                Ok(Some(b)) => b,
                Ok(None) => {
                    return FilePreview::failed(
                        path,
                        format!("larger than {}; replace refused", size_text(max_file_bytes)),
                    );
                }
                Err(e) => return FilePreview::failed(path, e.to_string()),
            };
            match replace_bytes(&bytes, spec) {
                Ok(r) => FilePreview {
                    path,
                    hunks: r.hunks,
                    error: None,
                    replacements: r.replacements,
                    fingerprint: fingerprint(&bytes),
                    root: canon_root.clone(),
                    spec: Some(spec.clone()),
                },
                Err(e) => FilePreview::failed(path, e),
            }
        })
        .collect()
}

/// True when `path` resolves (links followed) to a file under the canonical `root`.
fn inside(path: &Path, root: &Path) -> bool {
    fs::canonicalize(path).is_ok_and(|p| p.starts_with(root))
}

struct Replaced {
    bytes: Vec<u8>,
    hunks: Vec<Hunk>,
    replacements: usize,
}

fn replace_bytes(original: &[u8], spec: &Spec) -> Result<Replaced, &'static str> {
    if original.contains(&0) {
        return Err("binary file");
    }
    let (bom, body) = match original.strip_prefix(UTF8_BOM) {
        Some(rest) => (UTF8_BOM, rest),
        None => (&[][..], original),
    };
    let Ok(text) = std::str::from_utf8(body) else {
        return Err("not UTF-8; replace refused to keep the encoding intact");
    };
    let m = &spec.matcher;
    let mut out = String::with_capacity(text.len());
    let mut hunks = Vec::new();
    let mut replacements = 0;
    let mut copied = 0;
    for (i, (start, end)) in line_spans(text).enumerate() {
        let line = &text[start..end];
        let mut after = String::new();
        let (mut kept, mut matched) = (0, false);
        m.each_match(line, |s, e| {
            after.push_str(&line[kept..s]);
            m.expand(line, s, &spec.replacement, &mut after);
            kept = e;
            matched = true;
            replacements += 1;
        });
        if !matched {
            continue;
        }
        after.push_str(&line[kept..]);
        if after == line {
            continue;
        }
        out.push_str(&text[copied..start]);
        out.push_str(&after);
        copied = end;
        hunks.push(Hunk {
            line: i + 1,
            before: line.to_string(),
            after,
        });
    }
    out.push_str(&text[copied..]);
    let mut bytes = Vec::with_capacity(bom.len() + out.len());
    bytes.extend_from_slice(bom);
    bytes.extend_from_slice(out.as_bytes());
    Ok(Replaced {
        bytes,
        hunks,
        replacements,
    })
}

/// Writes the previewed replacement into each confirmed file and reports one result per
/// confirmed path. Only confirmed files with a preview are touched. A file is skipped with an
/// error when its preview failed, when it changed on disk after the preview (an AI agent may
/// have rewritten it), when it now resolves outside the project, when it is read-only or has
/// other hard links (a rename would silently replace or split it), or when an earlier
/// confirmed path is the same file (a link and its target). Writes are atomic (temp file in
/// the same directory + rename) and keep the file's permissions; a symlink is written
/// through, not replaced.
pub fn apply_replace(
    previews: &[FilePreview],
    confirmed: &[PathBuf],
) -> Vec<(PathBuf, Result<(), io::Error>)> {
    let mut seen: HashMap<PathBuf, PathBuf> = HashMap::new();
    confirmed
        .iter()
        .map(|path| {
            let result = match previews.iter().find(|p| &p.path == path) {
                None => Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "no preview for this file",
                )),
                Some(p) => apply_one(p, &mut seen),
            };
            (path.clone(), result)
        })
        .collect()
}

fn apply_one(p: &FilePreview, seen: &mut HashMap<PathBuf, PathBuf>) -> io::Result<()> {
    if let Some(e) = &p.error {
        return Err(io::Error::new(io::ErrorKind::InvalidData, e.clone()));
    }
    let Some(spec) = &p.spec else {
        return Ok(());
    };
    if p.hunks.is_empty() {
        return Ok(());
    }
    let target = fs::canonicalize(&p.path)?;
    if !target.starts_with(&p.root) {
        return Err(io::Error::other("not a file inside the project any more"));
    }
    if let Some(first) = seen.get(&target) {
        return Err(io::Error::other(format!(
            "same file as {}; replaced once",
            first.display()
        )));
    }
    seen.insert(target.clone(), p.path.clone());
    let meta = fs::metadata(&target)?;
    // rename() ignores the target's mode, so a read-only file needs an explicit refusal.
    if meta.permissions().readonly() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "file is read-only; not written",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.nlink() > 1 {
            return Err(io::Error::other(format!(
                "file has {} hard links; a replace would split them; not written \
                 (open the file and save it to replace it there)",
                meta.nlink()
            )));
        }
    }
    let expected_len = p.fingerprint.0;
    let current = match read_capped(&target, expected_len)? {
        Some(bytes) if fingerprint(&bytes) == p.fingerprint => bytes,
        _ => return Err(changed_on_disk()),
    };
    let replaced = replace_bytes(&current, spec).map_err(io::Error::other)?;
    let tmp = write_temp(&target, &replaced.bytes)?;
    commit_replace(
        &tmp,
        &target,
        &current,
        &replaced.bytes,
        &mut exchange_files,
        &mut || {},
    )
}

/// Prefix and suffix of vr-core's save temps, so the watcher and the explorer treat replace
/// temps like save temps. The name is short so a long target name cannot hit NAME_MAX.
const TEMP_PREFIX: &str = ".vr-ide-save-";
const TEMP_SUFFIX: &str = ".tmp";
static TEMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Writes `bytes` to a new temp file next to `target` with `target`'s permissions (set before
/// any content lands in it) and fsyncs it. Returns the temp path; on error nothing is left.
fn write_temp(target: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
    let perms = fs::metadata(target)?.permissions();
    let dir = target
        .parent()
        .ok_or_else(|| io::Error::other("file has no parent directory"))?;
    let (tmp, mut file) = loop {
        let tmp = dir.join(format!(
            "{TEMP_PREFIX}{}-s{}{TEMP_SUFFIX}",
            std::process::id(),
            TEMP_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new().write(true).create_new(true).open(&tmp) {
            Ok(f) => break (tmp, f),
            // Someone else's file: never touch it, take the next name.
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    let written = (|| {
        file.set_permissions(perms)?;
        file.write_all(bytes)?;
        file.sync_all()
    })();
    match written {
        Ok(()) => Ok(tmp),
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            Err(e)
        }
    }
}

fn changed_on_disk() -> io::Error {
    io::Error::other("file changed on disk since the preview; not written")
}

/// Puts the fsynced temp file `tmp` in place of `target` only if `target` still holds
/// `expected`, through vr-core's `safe_replace` (shared with `Document::save`): with
/// `renameat2(RENAME_EXCHANGE)` the swapped-out file is compared and swapped back on a
/// mismatch; what then comes out of the path is deleted only if it still is our temp file
/// holding `ours`, and a version that may be someone else's is kept under a visible
/// `<name>.vr-ide-conflict-…` name and the error ends with "kept at <path>". `swap` is the
/// atomic exchange (a seam for tests), `before_swap` a seam to simulate a concurrent writer.
fn commit_replace(
    tmp: &Path,
    target: &Path,
    expected: &[u8],
    ours: &[u8],
    swap: &mut dyn FnMut(&Path, &Path) -> io::Result<()>,
    before_swap: &mut dyn FnMut(),
) -> io::Result<()> {
    let outcome = safe_replace::replace_if_unchanged(
        tmp,
        target,
        Checks {
            target_unchanged: &mut |p| holds(p, expected),
            holds_expected: &mut |p| holds(p, expected),
            holds_ours: &mut |p| holds(p, ours),
        },
        swap,
        &mut |step| {
            if step == Step::BeforeSwap {
                before_swap()
            }
        },
    );
    match outcome {
        Ok(Outcome::Replaced) => {
            sync_dir(target);
            Ok(())
        }
        Ok(Outcome::Changed) => Err(changed_on_disk()),
        Ok(Outcome::Kept { at, what, cause }) => {
            sync_dir(target);
            Err(safe_replace::kept_error(&at, what, &cause))
        }
        Err(e) => {
            // `tmp` was never at the target: it still holds only our replacement.
            let _ = fs::remove_file(tmp);
            Err(e)
        }
    }
}

/// True when the file at `path` holds exactly `expected` (read at most one byte more).
fn holds(path: &Path, expected: &[u8]) -> io::Result<bool> {
    let mut buf = Vec::with_capacity(expected.len() + 1);
    fs::File::open(path)?
        .take(expected.len() as u64 + 1)
        .read_to_end(&mut buf)?;
    Ok(buf == expected)
}

/// Makes the rename durable; best effort, as some filesystems refuse fsync on directories.
fn sync_dir(target: &Path) {
    if let Some(dir) = target.parent()
        && let Ok(d) = fs::File::open(dir)
    {
        let _ = d.sync_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn re(pattern: &str) -> Query {
        Query {
            pattern: pattern.into(),
            regex: true,
            case_sensitive: true,
            whole_word: false,
        }
    }

    fn hunk(line: usize, before: &str, after: &str) -> Hunk {
        Hunk {
            line,
            before: before.into(),
            after: after.into(),
        }
    }

    #[test]
    fn plan_replace_previews_hunks() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(
            root.join("a.rs"),
            "fn one() {}\nlet x = 1;\nfn two() {} fn three() {}\n",
        )
        .unwrap();
        fs::write(root.join("b.rs"), "nothing here\n").unwrap();
        let src_before = fs::read(root.join("a.rs")).unwrap();
        let files = [PathBuf::from("a.rs"), root.join("b.rs")];
        let previews = plan_replace(root, &re(r"fn (\w+)"), "fn new_$1", &files);
        assert_eq!(previews.len(), 2);
        assert_eq!(previews[0].path, root.join("a.rs"));
        assert_eq!(previews[0].error, None);
        assert_eq!(
            previews[0].hunks,
            [
                hunk(1, "fn one() {}", "fn new_one() {}"),
                hunk(
                    3,
                    "fn two() {} fn three() {}",
                    "fn new_two() {} fn new_three() {}"
                ),
            ]
        );
        assert_eq!(previews[0].replacements(), 3);
        assert_eq!(
            (previews[1].hunks.len(), previews[1].error.as_deref()),
            (0, None)
        );
        // Planning never writes.
        assert_eq!(fs::read(root.join("a.rs")).unwrap(), src_before);
    }

    #[test]
    fn apply_replace_only_confirmed_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for name in ["a.txt", "b.txt", "c.txt"] {
            fs::write(root.join(name), "old value\n").unwrap();
        }
        let files: Vec<PathBuf> = ["a.txt", "b.txt", "c.txt"].map(PathBuf::from).to_vec();
        let previews = plan_replace(root, &re("old"), "new", &files);
        let confirmed = [root.join("a.txt"), root.join("c.txt")];
        let results = apply_replace(&previews, &confirmed);
        let paths: Vec<&PathBuf> = results.iter().map(|(p, _)| p).collect();
        assert_eq!(paths, [&confirmed[0], &confirmed[1]]);
        assert!(results.iter().all(|(_, r)| r.is_ok()), "{results:?}");
        assert_eq!(
            fs::read_to_string(root.join("a.txt")).unwrap(),
            "new value\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("b.txt")).unwrap(),
            "old value\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("c.txt")).unwrap(),
            "new value\n"
        );
        // A confirmed path without a preview is an error, not a silent write.
        let results = apply_replace(&previews, &[root.join("zzz.txt")]);
        assert_eq!(
            results[0].1.as_ref().unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        // No temp files are left behind.
        let mut names: Vec<String> = fs::read_dir(root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, ["a.txt", "b.txt", "c.txt"]);
    }

    #[test]
    fn apply_replace_keeps_crlf() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let cases: [(&str, &[u8], &[u8]); 3] = [
            (
                "crlf.txt",
                b"\xEF\xBB\xBFold\r\nkeep\r\nold old\r\n",
                b"\xEF\xBB\xBFnew\r\nkeep\r\nnew new\r\n",
            ),
            (
                "mixed.txt",
                b"old\rold\nold\r\nold",
                b"new\rnew\nnew\r\nnew",
            ),
            ("vi.txt", "Tiếng old\n".as_bytes(), "Tiếng new\n".as_bytes()),
        ];
        let files: Vec<PathBuf> = cases.iter().map(|c| PathBuf::from(c.0)).collect();
        for (name, before, _) in &cases {
            fs::write(root.join(name), before).unwrap();
        }
        let previews = plan_replace(root, &re("^old|old$|old"), "new", &files);
        let confirmed: Vec<PathBuf> = previews.iter().map(|p| p.path.clone()).collect();
        let results = apply_replace(&previews, &confirmed);
        assert!(results.iter().all(|(_, r)| r.is_ok()), "{results:?}");
        for (name, _, after) in &cases {
            assert_eq!(fs::read(root.join(name)).unwrap(), *after, "{name}");
        }
        // The BOM is not part of line 1 in the preview.
        assert_eq!(previews[0].hunks[0], hunk(1, "old", "new"));
    }

    #[test]
    fn apply_replace_skips_file_changed_since_preview() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("a.txt"), "old\n").unwrap();
        fs::write(root.join("b.txt"), "old\n").unwrap();
        let files = [PathBuf::from("a.txt"), PathBuf::from("b.txt")];
        let previews = plan_replace(root, &re("old"), "new", &files);
        // An agent rewrites a.txt while the preview is on screen.
        fs::write(root.join("a.txt"), "old\nagent line\n").unwrap();
        let results = apply_replace(&previews, &[root.join("a.txt"), root.join("b.txt")]);
        let err = results[0].1.as_ref().unwrap_err();
        assert!(err.to_string().contains("changed on disk"), "{err}");
        assert!(results[1].1.is_ok());
        assert_eq!(
            fs::read_to_string(root.join("a.txt")).unwrap(),
            "old\nagent line\n"
        );
        assert_eq!(fs::read_to_string(root.join("b.txt")).unwrap(), "new\n");
    }

    #[test]
    fn plan_replace_refuses_unsafe_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("proj");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("latin1.txt"), b"old caf\xe9\n").unwrap();
        fs::write(root.join("blob.bin"), b"old\0").unwrap();
        fs::write(dir.path().join("outside.txt"), "old\n").unwrap();
        let files = ["latin1.txt", "blob.bin", "../outside.txt", "missing.txt"].map(PathBuf::from);
        let previews = plan_replace(&root, &re("old"), "new", &files);
        for p in &previews {
            assert!(p.error.is_some() && p.hunks.is_empty(), "{p:?}");
        }
        assert!(previews[0].error.as_deref().unwrap().contains("UTF-8"));
        let confirmed: Vec<PathBuf> = previews.iter().map(|p| p.path.clone()).collect();
        let results = apply_replace(&previews, &confirmed);
        assert!(results.iter().all(|(_, r)| r.is_err()));
        assert_eq!(fs::read(root.join("latin1.txt")).unwrap(), b"old caf\xe9\n");
        assert_eq!(
            fs::read_to_string(dir.path().join("outside.txt")).unwrap(),
            "old\n"
        );
        // An invalid query fails every preview instead of panicking.
        let bad = plan_replace(&root, &re("("), "x", &[PathBuf::from("latin1.txt")]);
        assert!(bad[0].error.as_deref().unwrap().contains("invalid regex"));
    }

    #[cfg(unix)]
    #[test]
    fn apply_replace_keeps_mode_and_symlink() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("run.sh"), "echo old\n").unwrap();
        fs::set_permissions(root.join("run.sh"), fs::Permissions::from_mode(0o750)).unwrap();
        symlink("run.sh", root.join("link.sh")).unwrap();
        let previews = plan_replace(root, &re("old"), "new", &[PathBuf::from("link.sh")]);
        let results = apply_replace(&previews, &[root.join("link.sh")]);
        assert!(results[0].1.is_ok(), "{results:?}");
        assert!(
            fs::symlink_metadata(root.join("link.sh"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::read_to_string(root.join("run.sh")).unwrap(),
            "echo new\n"
        );
        let mode = fs::metadata(root.join("run.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o750);
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn commit_keeps_agent_write_made_before_the_swap() {
        // Both the exchange path and the plain-rename fallback must notice a write that lands
        // after the temp file is written and fsynced.
        for exchange in [true, false] {
            let mut swap = |a: &Path, b: &Path| {
                if exchange {
                    exchange_files(a, b)
                } else {
                    Err(io::Error::from(io::ErrorKind::Unsupported))
                }
            };
            let dir = tempfile::tempdir().unwrap();
            let target = dir.path().join("a.txt");
            fs::write(&target, "old\n").unwrap();
            let tmp = write_temp(&target, b"new\n").unwrap();
            let err = commit_replace(&tmp, &target, b"old\n", b"new\n", &mut swap, &mut || {
                fs::write(&target, "agent\n").unwrap()
            })
            .unwrap_err();
            assert!(
                err.to_string().contains("changed on disk"),
                "{exchange}: {err}"
            );
            assert_eq!(
                fs::read_to_string(&target).unwrap(),
                "agent\n",
                "{exchange}"
            );
            assert_eq!(names(dir.path()), ["a.txt"], "{exchange}");
            // Without interference the same steps replace the file.
            let tmp = write_temp(&target, b"new\n").unwrap();
            commit_replace(&tmp, &target, b"agent\n", b"new\n", &mut swap, &mut || {}).unwrap();
            assert_eq!(fs::read_to_string(&target).unwrap(), "new\n", "{exchange}");
            assert_eq!(names(dir.path()), ["a.txt"], "{exchange}");
        }
    }

    #[test]
    fn plan_replace_refuses_oversized_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("big.txt"), format!("old\n{}", "x".repeat(200))).unwrap();
        fs::write(root.join("small.txt"), "old\n").unwrap();
        let files = [PathBuf::from("big.txt"), PathBuf::from("small.txt")];
        let previews = plan_replace_with(root, &re("old"), "new", &files, 100);
        let err = previews[0].error.as_deref().unwrap();
        assert!(err.contains("larger than"), "{err}");
        assert_eq!(previews[1].hunks.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn apply_replace_refuses_read_only_and_hard_linked_files() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("ro.txt"), "old\n").unwrap();
        fs::set_permissions(root.join("ro.txt"), fs::Permissions::from_mode(0o444)).unwrap();
        fs::write(root.join("linked.txt"), "old\n").unwrap();
        fs::hard_link(root.join("linked.txt"), root.join("other-name.txt")).unwrap();
        let files = [PathBuf::from("ro.txt"), PathBuf::from("linked.txt")];
        let previews = plan_replace(root, &re("old"), "new", &files);
        let results = apply_replace(&previews, &[root.join("ro.txt"), root.join("linked.txt")]);
        let ro = results[0].1.as_ref().unwrap_err();
        assert_eq!(ro.kind(), io::ErrorKind::PermissionDenied, "{ro}");
        let hl = results[1].1.as_ref().unwrap_err();
        assert!(hl.to_string().contains("hard link"), "{hl}");
        assert!(hl.to_string().contains("open the file and save it"), "{hl}");
        assert_eq!(fs::read_to_string(root.join("ro.txt")).unwrap(), "old\n");
        assert_eq!(
            fs::read_to_string(root.join("other-name.txt")).unwrap(),
            "old\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn apply_replace_rechecks_root_and_dedupes_links() {
        use std::os::unix::fs::symlink;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("proj");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("a.txt"), "old\n").unwrap();
        fs::write(root.join("b.txt"), "old\n").unwrap();
        fs::write(dir.path().join("outside.txt"), "old\n").unwrap();
        symlink("a.txt", root.join("link-a.txt")).unwrap();
        symlink("b.txt", root.join("link-b.txt")).unwrap();
        let files = ["a.txt", "link-a.txt", "link-b.txt"].map(PathBuf::from);
        let previews = plan_replace(&root, &re("old"), "new", &files);
        // Between preview and apply, link-b is retargeted outside the project (same bytes).
        fs::remove_file(root.join("link-b.txt")).unwrap();
        symlink("../outside.txt", root.join("link-b.txt")).unwrap();
        let confirmed = files.map(|f| root.join(f));
        let results = apply_replace(&previews, &confirmed);
        assert!(results[0].1.is_ok(), "{results:?}");
        let dup = results[1].1.as_ref().unwrap_err().to_string();
        assert!(
            dup.contains("same file as") && dup.contains("a.txt"),
            "{dup}"
        );
        let out = results[2].1.as_ref().unwrap_err().to_string();
        assert!(out.contains("inside the project"), "{out}");
        assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "new\n");
        assert_eq!(
            fs::read_to_string(dir.path().join("outside.txt")).unwrap(),
            "old\n"
        );
    }

    #[test]
    fn write_temp_never_touches_existing_names_and_fits_long_names() {
        let dir = tempfile::tempdir().unwrap();
        let long = format!("{}.txt", "n".repeat(250));
        let target = dir.path().join(&long);
        fs::write(&target, "old\n").unwrap();
        let next = TEMP_SEQ.load(Ordering::Relaxed);
        let squatters: Vec<PathBuf> = (next..next + 3)
            .map(|n| {
                let p = dir.path().join(format!(
                    "{TEMP_PREFIX}{}-s{n}{TEMP_SUFFIX}",
                    std::process::id()
                ));
                fs::write(&p, "someone else's").unwrap();
                p
            })
            .collect();
        let previews = plan_replace(dir.path(), &re("old"), "new", &[PathBuf::from(&long)]);
        let results = apply_replace(&previews, std::slice::from_ref(&target));
        assert!(results[0].1.is_ok(), "{results:?}");
        assert_eq!(fs::read_to_string(&target).unwrap(), "new\n");
        for p in &squatters {
            assert_eq!(fs::read_to_string(p).unwrap(), "someone else's");
        }
        assert_eq!(names(dir.path()).len(), 4);
    }

    /// The path an error message ends with (`… kept at <path>`).
    fn kept_path(err: &io::Error) -> PathBuf {
        let msg = err.to_string();
        let (_, path) = msg.rsplit_once(" kept at ").expect(&msg);
        PathBuf::from(path)
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn commit_keeps_other_version_when_swap_back_fails() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.txt");
        fs::write(&target, "old\n").unwrap();
        let tmp = write_temp(&target, b"new\n").unwrap();
        let mut calls = 0;
        let mut swap = |a: &Path, b: &Path| {
            calls += 1;
            if calls == 1 {
                exchange_files(a, b)
            } else {
                Err(io::Error::other("injected swap-back failure"))
            }
        };
        let err = commit_replace(&tmp, &target, b"old\n", b"new\n", &mut swap, &mut || {
            fs::write(&target, "agent\n").unwrap()
        })
        .unwrap_err();
        let kept = kept_path(&err);
        assert_eq!(fs::read_to_string(&kept).unwrap(), "agent\n", "{err}");
        assert_eq!(kept.parent(), Some(dir.path()));
        let kept_name = kept.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            kept_name.starts_with("a.txt.vr-ide-conflict-"),
            "{kept_name}"
        );
        assert!(!kept_name.starts_with(TEMP_PREFIX));
        assert_eq!(fs::read_to_string(&target).unwrap(), "new\n");
        assert_eq!(names(dir.path()), ["a.txt", kept_name.as_str()]);
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn commit_keeps_in_place_write_made_between_swap_and_swap_back() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.txt");
        fs::write(&target, "old\n").unwrap();
        let tmp = write_temp(&target, b"new\n").unwrap();
        let mut calls = 0;
        let mut swap = |a: &Path, b: &Path| {
            calls += 1;
            if calls == 2 {
                // The writer's next version is written by path while our file sits there.
                fs::write(&target, "agent2\n").unwrap();
            }
            exchange_files(a, b)
        };
        let err = commit_replace(&tmp, &target, b"old\n", b"new\n", &mut swap, &mut || {
            fs::write(&target, "agent1\n").unwrap()
        })
        .unwrap_err();
        assert_eq!(fs::read_to_string(&target).unwrap(), "agent1\n", "{err}");
        assert!(
            err.to_string().contains(" kept at "),
            "{err}; dir {:?}",
            names(dir.path())
        );
        let kept = kept_path(&err);
        assert_eq!(fs::read_to_string(&kept).unwrap(), "agent2\n", "{err}");
        assert_eq!(names(dir.path()).len(), 2);
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn commit_keeps_other_version_when_verify_fails() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.txt");
        fs::write(&target, "agent\n").unwrap();
        let tmp = write_temp(&target, b"new\n").unwrap();
        let mut swap = |a: &Path, b: &Path| {
            exchange_files(a, b)?;
            // The swapped-out file cannot be read back (stands in for EMFILE or EIO).
            fs::set_permissions(a, fs::Permissions::from_mode(0o000))
        };
        let result = commit_replace(&tmp, &target, b"agent\n", b"new\n", &mut swap, &mut || {});
        if fs::File::open(&target).is_ok() && result.is_ok() {
            // Running as root: permissions do not stop the read, nothing to assert.
            return;
        }
        let err = result.unwrap_err();
        let kept = kept_path(&err);
        fs::set_permissions(&kept, fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(fs::read_to_string(&kept).unwrap(), "agent\n", "{err}");
        assert_eq!(fs::read_to_string(&target).unwrap(), "new\n");
        assert_eq!(names(dir.path()).len(), 2);
    }
}
