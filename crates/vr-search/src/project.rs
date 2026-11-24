//! Project-wide search, streamed in batches from a parallel, ignore-aware walk.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use grep_matcher::LineTerminator;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::sinks::UTF8;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder};
use ignore::{WalkBuilder, WalkState};

use crate::Query;
use crate::find::{Matcher, line_spans, regex_source};

/// A batch is sent once it holds this many matches ...
const MAX_BATCH: usize = 200;
/// ... or this long after its first match arrived, whichever comes first.
const FLUSH_EVERY: Duration = Duration::from_millis(50);
/// Searching stops after this many matches; an `Error` message says so.
pub(crate) const MAX_MATCHES: usize = 100_000;
/// Lines longer than this (bytes) are cut to a window around the match in `line_text`,
/// so a minified file cannot make every match carry a megabyte-long line.
const MAX_LINE_BYTES: usize = 1024;
const WINDOW_CHARS: usize = 300;
const WINDOW_BEFORE: usize = 60;

const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

/// One match. `line` and `col` are 1-based (`file:line:col`, as `grep -n`), `col` counts
/// chars. `match_range` is a char range into `line_text`, which is the line without its
/// break, or a window of it when the line is very long.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectMatch {
    pub path: PathBuf,
    pub line: usize,
    pub col: usize,
    pub line_text: String,
    pub match_range: (usize, usize),
}

/// Messages of one search. `Done` is always the last one; `Error` reports an invalid query,
/// an unreadable root or the match limit and is followed by `Done`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchMsg {
    Batch(Vec<ProjectMatch>),
    Done {
        files: usize,
        matches: usize,
        elapsed: Duration,
    },
    Error(String),
}

/// The walk used by project search and replace: the same policy as vr-fs (D15) — `.gitignore`
/// honoured with or without a git repository, `.git/info/exclude` and global excludes, hidden
/// files included, `.git` skipped, links not followed. Configured here identically because
/// vr-fs does not export its builder.
pub(crate) fn walk_builder(root: &Path) -> WalkBuilder {
    let mut b = WalkBuilder::new(root);
    b.hidden(false)
        .parents(false)
        .ignore(false)
        .require_git(false)
        .follow_links(false)
        .filter_entry(|e| e.file_name() != ".git");
    b
}

/// Searches every non-ignored text file under `root` on background threads and streams the
/// matches to `tx`. Setting `cancel` stops the walk; dropping the receiver does too.
/// Binary files (a NUL byte) and files that are not UTF-8 are skipped.
pub fn search_project(
    root: &Path,
    q: &Query,
    cancel: Arc<AtomicBool>,
    tx: Sender<SearchMsg>,
) -> JoinHandle<()> {
    search_project_with(root, q, cancel, tx, Limits::default())
}

/// [`search_project`] over several roots (a multi-root workspace): one search, the roots
/// walked one after the other in the given order (each with its own ignore rules), so every
/// match of a root arrives before those of the next one. A root that cannot be read gives an
/// `Error` and the others are still searched; one `Done` counts them all.
pub fn search_roots(
    roots: &[PathBuf],
    q: &Query,
    cancel: Arc<AtomicBool>,
    tx: Sender<SearchMsg>,
) -> JoinHandle<()> {
    let roots = roots.to_vec();
    let q = q.clone();
    thread::spawn(move || run_search(roots, &q, cancel, &tx, Limits::default()))
}

/// Files above this size are not searched or replaced (reported, not silently dropped), so
/// memory stays bounded by walker threads × this size whatever the project holds.
pub(crate) const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Limits {
    pub max_file_bytes: u64,
    pub max_matches: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_file_bytes: MAX_FILE_BYTES,
            max_matches: MAX_MATCHES,
        }
    }
}

/// Reads `path` whole if it holds at most `max` bytes; `None` when it is larger. Never
/// allocates more than `max + 1` bytes, even if the file grows while being read.
pub(crate) fn read_capped(path: &Path, max: u64) -> io::Result<Option<Vec<u8>>> {
    let (file, hint) = open_regular(path)?;
    if hint > max {
        return Ok(None);
    }
    let mut buf = Vec::with_capacity(usize::try_from(hint).unwrap_or(0));
    file.take(max + 1).read_to_end(&mut buf)?;
    Ok((buf.len() as u64 <= max).then_some(buf))
}

/// Opens `path` for reading only if it is a regular file, and returns it with its length.
/// A FIFO or device (put in place of a result file after the search) would block the open
/// or the read forever, so it is refused before the open, the open does not wait
/// (O_NONBLOCK), and the opened handle is checked again in case the path was swapped in
/// between. Mirrors vr-core's private `hash_file` guard (D89).
pub(crate) fn open_regular(path: &Path) -> io::Result<(fs::File, u64)> {
    if !fs::metadata(path)?.is_file() {
        return Err(not_regular());
    }
    let mut opts = fs::OpenOptions::new();
    opts.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32);
    }
    let file = opts.open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(not_regular());
    }
    Ok((file, meta.len()))
}

fn not_regular() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "not a regular file")
}

/// "32 MB" / "100 bytes" for messages.
pub(crate) fn size_text(bytes: u64) -> String {
    const MB: u64 = 1024 * 1024;
    if bytes >= MB {
        format!("{} MB", bytes / MB)
    } else {
        format!("{bytes} bytes")
    }
}

pub(crate) fn search_project_with(
    root: &Path,
    q: &Query,
    cancel: Arc<AtomicBool>,
    tx: Sender<SearchMsg>,
    limits: Limits,
) -> JoinHandle<()> {
    let roots = vec![root.to_path_buf()];
    let q = q.clone();
    thread::spawn(move || run_search(roots, &q, cancel, &tx, limits))
}

/// State shared by the walker threads of one search.
struct WalkCtx {
    search: FileSearch,
    /// Canonical roots: a link to a file inside one is skipped, the file itself is listed.
    canon_roots: Vec<PathBuf>,
    cancel: Arc<AtomicBool>,
    limits: Limits,
    files: AtomicUsize,
    oversized: AtomicUsize,
    total: AtomicUsize,
    limit_hit: AtomicBool,
}

fn run_search(
    roots: Vec<PathBuf>,
    q: &Query,
    cancel: Arc<AtomicBool>,
    tx: &Sender<SearchMsg>,
    limits: Limits,
) {
    let started = Instant::now();
    let done = |files, matches| {
        let _ = tx.send(SearchMsg::Done {
            files,
            matches,
            elapsed: started.elapsed(),
        });
    };
    let matcher = match Matcher::new(q) {
        Ok(Some(m)) => m,
        Ok(None) => return done(0, 0),
        Err(e) => {
            let _ = tx.send(SearchMsg::Error(e.to_string()));
            return done(0, 0);
        }
    };
    let mut readable = Vec::with_capacity(roots.len());
    for root in roots {
        match fs::read_dir(&root) {
            Ok(_) => readable.push(root),
            Err(e) => {
                let _ = tx.send(SearchMsg::Error(format!("{}: {e}", root.display())));
            }
        }
    }
    if readable.is_empty() {
        return done(0, 0);
    }
    let ctx = Arc::new(WalkCtx {
        search: FileSearch {
            matcher,
            prefilter: prefilter(q),
        },
        canon_roots: readable
            .iter()
            .filter_map(|r| fs::canonicalize(r).ok())
            .collect(),
        cancel: cancel.clone(),
        limits,
        files: AtomicUsize::new(0),
        oversized: AtomicUsize::new(0),
        total: AtomicUsize::new(0),
        limit_hit: AtomicBool::new(false),
    });
    let (mtx, mrx) = mpsc::channel();
    let walker = {
        let ctx = ctx.clone();
        thread::spawn(move || {
            for root in &readable {
                if ctx.cancel.load(Ordering::Relaxed) || ctx.limit_hit.load(Ordering::Relaxed) {
                    break;
                }
                walk(root, &ctx, &mtx);
            }
        })
    };
    let sent = forward_batches(&mrx, tx, &cancel, MAX_BATCH, FLUSH_EVERY);
    let _ = walker.join();
    let oversized = ctx.oversized.load(Ordering::Relaxed);
    if oversized > 0 {
        let _ = tx.send(SearchMsg::Error(format!(
            "{oversized} file{} larger than {} not searched",
            if oversized == 1 { "" } else { "s" },
            size_text(limits.max_file_bytes)
        )));
    }
    if ctx.limit_hit.load(Ordering::Relaxed) {
        let _ = tx.send(SearchMsg::Error(format!(
            "more than {} matches; search stopped",
            limits.max_matches
        )));
    }
    done(ctx.files.load(Ordering::Relaxed), sent);
}

fn walk(root: &Path, ctx: &Arc<WalkCtx>, mtx: &Sender<Vec<ProjectMatch>>) {
    walk_builder(root).build_parallel().run(|| {
        let (ctx, mtx) = (ctx.clone(), mtx.clone());
        let mut searcher = SearcherBuilder::new()
            .line_number(true)
            // Must agree with the prefilter, which is built with `crlf(true)`.
            .line_terminator(LineTerminator::crlf())
            .binary_detection(BinaryDetection::none())
            .build();
        Box::new(move |entry| visit(&ctx, &mtx, &mut searcher, entry))
    });
}

fn visit(
    ctx: &WalkCtx,
    mtx: &Sender<Vec<ProjectMatch>>,
    searcher: &mut Searcher,
    entry: Result<ignore::DirEntry, ignore::Error>,
) -> WalkState {
    if ctx.cancel.load(Ordering::Relaxed) || ctx.limit_hit.load(Ordering::Relaxed) {
        return WalkState::Quit;
    }
    // Unreadable entries (permissions, races) are skipped.
    let Ok(entry) = entry else {
        return WalkState::Continue;
    };
    let path = entry.path();
    let Some(ft) = entry.file_type() else {
        return WalkState::Continue;
    };
    if ft.is_symlink() {
        if !path.is_file() {
            return WalkState::Continue;
        }
        // The target inside the project (any of its roots) is listed under its own name
        // already.
        let target_inside = fs::canonicalize(path)
            .is_ok_and(|t| ctx.canon_roots.iter().any(|root| t.starts_with(root)));
        if target_inside {
            return WalkState::Continue;
        }
    } else if !ft.is_file() {
        return WalkState::Continue;
    }
    let bytes = match read_capped(path, ctx.limits.max_file_bytes) {
        Ok(Some(b)) => b,
        Ok(None) => {
            ctx.oversized.fetch_add(1, Ordering::Relaxed);
            return WalkState::Continue;
        }
        Err(_) => return WalkState::Continue,
    };
    let max = ctx.limits.max_matches;
    let budget = max.saturating_sub(ctx.total.load(Ordering::Relaxed));
    let Some(mut found) = ctx
        .search
        .search_file(searcher, path, &bytes, &ctx.cancel, budget)
    else {
        return WalkState::Continue;
    };
    ctx.files.fetch_add(1, Ordering::Relaxed);
    if found.is_empty() {
        return WalkState::Continue;
    }
    let before = ctx.total.fetch_add(found.len(), Ordering::Relaxed);
    if before + found.len() > max {
        ctx.limit_hit.store(true, Ordering::Relaxed);
        found.truncate(max.saturating_sub(before));
    }
    if !found.is_empty() && mtx.send(found).is_err() {
        return WalkState::Quit;
    }
    WalkState::Continue
}

/// Regroups per-file results into batches of at most `max_batch` matches, sent when full or
/// `flush_every` after their first match, so results stream while the walk goes on.
/// Returns the number of matches sent.
fn forward_batches(
    rx: &Receiver<Vec<ProjectMatch>>,
    tx: &Sender<SearchMsg>,
    cancel: &AtomicBool,
    max_batch: usize,
    flush_every: Duration,
) -> usize {
    let mut batch: Vec<ProjectMatch> = Vec::new();
    let mut deadline: Option<Instant> = None;
    let mut sent = 0;
    let mut flush = |batch: &mut Vec<ProjectMatch>| {
        if batch.is_empty() || cancel.load(Ordering::Relaxed) {
            batch.clear();
            return;
        }
        let n = batch.len();
        if tx.send(SearchMsg::Batch(std::mem::take(batch))).is_ok() {
            sent += n;
        } else {
            // Nobody listens any more: stop the walk.
            cancel.store(true, Ordering::Relaxed);
        }
    };
    loop {
        let next = match deadline {
            Some(d) => rx.recv_timeout(d.saturating_duration_since(Instant::now())),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match next {
            Ok(found) => {
                if batch.is_empty() {
                    deadline = Some(Instant::now() + flush_every);
                }
                batch.extend(found);
                while batch.len() >= max_batch {
                    let rest = batch.split_off(max_batch);
                    flush(&mut batch);
                    batch = rest;
                }
                if batch.is_empty() {
                    deadline = None;
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                flush(&mut batch);
                deadline = None;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    flush(&mut batch);
    sent
}

/// A grep-regex matcher that finds candidate lines fast. It is only a filter: every
/// candidate line is re-checked by the exact line matcher. `None` when its line model could
/// differ from ours: `\A`/`\z` (buffer vs line anchors) or a pattern it rejects.
fn prefilter(q: &Query) -> Option<RegexMatcher> {
    if q.regex && !prefilter_safe(&q.pattern) {
        return None;
    }
    RegexMatcherBuilder::new()
        .case_insensitive(!q.case_sensitive)
        .multi_line(true)
        .line_terminator(Some(b'\n'))
        .crlf(true)
        .build(&regex_source(q))
        .ok()
}

/// False when grep-regex's buffer-oriented matching could reject a line our per-line matcher
/// accepts: `\A`/`\z` (buffer vs line anchors) and inline flag groups such as `(?-m)` (which
/// turn `^`/`$` back into buffer anchors). Plain `(?:`, `(?<name>` and `(?P<name>` groups
/// are fine. Escaped text may trigger a false alarm, which only costs speed.
fn prefilter_safe(pattern: &str) -> bool {
    if pattern.contains(r"\A") || pattern.contains(r"\z") {
        return false;
    }
    pattern
        .match_indices("(?")
        .all(|(i, _)| matches!(pattern.as_bytes().get(i + 2), Some(b':' | b'<' | b'P')))
}

struct FileSearch {
    matcher: Matcher,
    prefilter: Option<RegexMatcher>,
}

impl FileSearch {
    /// Matches in one file, at most `budget + 1` (one more tells the caller the limit was
    /// crossed). `None` for a binary or non-UTF-8 file.
    fn search_file(
        &self,
        searcher: &mut Searcher,
        path: &Path,
        bytes: &[u8],
        cancel: &AtomicBool,
        budget: usize,
    ) -> Option<Vec<ProjectMatch>> {
        if bytes.contains(&0) {
            return None;
        }
        let text = std::str::from_utf8(bytes.strip_prefix(UTF8_BOM).unwrap_or(bytes)).ok()?;
        let mut out = Vec::new();
        let on_line = |out: &mut Vec<ProjectMatch>, line_no: usize, line: &str| {
            if out.len() <= budget {
                self.line_matches(path, line_no, line, out);
            }
            out.len() <= budget && !cancel.load(Ordering::Relaxed)
        };
        // grep-searcher numbers lines by `\n` only; a lone `\r` (D31) needs our own split.
        let fast = match &self.prefilter {
            Some(pre) if !has_lone_cr(text.as_bytes()) => {
                let sink =
                    UTF8(|n, line: &str| Ok(on_line(&mut out, n as usize, strip_break(line))));
                searcher.search_slice(pre, text.as_bytes(), sink).is_ok()
            }
            _ => false,
        };
        if !fast {
            out.clear();
            for (i, (start, end)) in line_spans(text).enumerate() {
                if !on_line(&mut out, i + 1, &text[start..end]) {
                    break;
                }
            }
        }

        out.truncate(budget + 1);
        Some(out)
    }

    fn line_matches(&self, path: &Path, line_no: usize, line: &str, out: &mut Vec<ProjectMatch>) {
        // Matches come left to right, so char columns are counted incrementally.
        let (mut byte, mut chars) = (0, 0);
        self.matcher.each_match(line, |s, e| {
            chars += line[byte..s].chars().count();
            byte = s;
            let (line_text, match_range) = snippet(line, s, e, chars);
            out.push(ProjectMatch {
                path: path.to_path_buf(),
                line: line_no,
                col: chars + 1,
                line_text,
                match_range,
            });
        });
    }
}

/// `line_text` and the char range of `s..e` in it; `s_chars` is the char column of `s`.
fn snippet(line: &str, s: usize, e: usize, s_chars: usize) -> (String, (usize, usize)) {
    let match_chars = line[s..e].chars().count();
    if line.len() <= MAX_LINE_BYTES {
        return (line.to_string(), (s_chars, s_chars + match_chars));
    }
    let from = line[..s]
        .char_indices()
        .rev()
        .take(WINDOW_BEFORE)
        .last()
        .map_or(s, |(i, _)| i);
    let before = line[from..s].chars().count();
    let to = line[from..]
        .char_indices()
        .nth(WINDOW_CHARS)
        .map_or(line.len(), |(i, _)| from + i);
    let end_in_window = before + line[s..e.min(to).max(s)].chars().count();
    (line[from..to].to_string(), (before, end_in_window))
}

fn strip_break(line: &str) -> &str {
    let line = line.strip_suffix('\n').unwrap_or(line);
    line.strip_suffix('\r').unwrap_or(line)
}

fn has_lone_cr(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .enumerate()
        .any(|(i, &b)| b == b'\r' && bytes.get(i + 1) != Some(&b'\n'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::mpsc;

    fn q(pattern: &str) -> Query {
        Query {
            pattern: pattern.into(),
            ..Query::default()
        }
    }

    fn write(root: &Path, rel: &str, body: impl AsRef<[u8]>) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }

    /// Runs a search to the end and returns every message in arrival order.
    fn run(root: &Path, query: &Query) -> Vec<SearchMsg> {
        let (tx, rx) = mpsc::channel();
        let handle = search_project(root, query, Arc::new(AtomicBool::new(false)), tx);
        let msgs: Vec<SearchMsg> = rx.iter().collect();
        handle.join().unwrap();
        msgs
    }

    fn matches_of(msgs: &[SearchMsg]) -> Vec<ProjectMatch> {
        let mut out: Vec<ProjectMatch> = msgs
            .iter()
            .filter_map(|m| match m {
                SearchMsg::Batch(b) => Some(b.clone()),
                _ => None,
            })
            .flatten()
            .collect();
        out.sort_by(|a, b| (&a.path, a.line, a.col).cmp(&(&b.path, b.line, b.col)));
        out
    }

    fn rel_paths(root: &Path, ms: &[ProjectMatch]) -> Vec<String> {
        let mut v: Vec<String> = ms
            .iter()
            .map(|m| {
                m.path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        v.dedup();
        v
    }

    #[test]
    fn search_roots_walks_each_root_in_order_with_its_own_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        write(&a, ".gitignore", "*.log\n");
        write(&a, "x.rs", "needle\n");
        write(&a, "skip.log", "needle\n");
        write(&b, "y.log", "needle\n");
        write(&b, "z.rs", "needle needle\n");
        for i in 0..50 {
            write(&a, &format!("many/f{i}.rs"), "needle\n");
        }
        let (tx, rx) = mpsc::channel();
        let roots = [a.clone(), b.clone()];
        let handle = search_roots(&roots, &q("needle"), Arc::new(AtomicBool::new(false)), tx);
        let msgs: Vec<SearchMsg> = rx.iter().collect();
        handle.join().unwrap();
        // Arrival order: every match of root a before any of root b.
        let arrived: Vec<PathBuf> = msgs
            .iter()
            .filter_map(|m| match m {
                SearchMsg::Batch(b) => Some(b.iter().map(|m| m.path.clone())),
                _ => None,
            })
            .flatten()
            .collect();
        let first_b = arrived.iter().position(|p| p.starts_with(&b)).unwrap();
        assert!(arrived[..first_b].iter().all(|p| p.starts_with(&a)));
        assert!(arrived[first_b..].iter().all(|p| p.starts_with(&b)));
        let found = matches_of(&msgs);
        assert!(!found.iter().any(|m| m.path == a.join("skip.log")));
        assert!(found.iter().any(|m| m.path == b.join("y.log")), "{found:?}");
        // One Done for the whole search, counting both roots.
        assert_eq!(
            msgs.iter()
                .filter(|m| matches!(m, SearchMsg::Done { .. }))
                .count(),
            1
        );
        assert!(matches!(
            msgs.last(),
            Some(SearchMsg::Done {
                // a: .gitignore, x.rs, 50 in many/; b: y.log, z.rs.
                files: 54,
                matches: 54,
                ..
            })
        ));

        // A missing root is reported; the others are still searched.
        let (tx, rx) = mpsc::channel();
        let roots = [dir.path().join("gone"), b.clone()];
        search_roots(&roots, &q("needle"), Arc::new(AtomicBool::new(false)), tx)
            .join()
            .unwrap();
        let msgs: Vec<SearchMsg> = rx.iter().collect();
        assert!(
            msgs.iter()
                .any(|m| matches!(m, SearchMsg::Error(e) if e.contains("gone")))
        );
        assert_eq!(matches_of(&msgs).len(), 3);
    }

    #[test]
    fn project_search_respects_gitignore() {
        // Not a git repository: .gitignore still applies (D15); dotfiles are searched, .git is not.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, ".gitignore", "target/\n*.log\n");
        write(root, "src/a.rs", "let needle = 1;\n");
        write(root, "src/sub/.gitignore", "gen.rs\n");
        write(root, "src/sub/gen.rs", "needle\n");
        write(root, "src/sub/keep.rs", "needle\n");
        write(root, "target/debug/x.rs", "needle\n");
        write(root, "run.log", "needle\n");
        write(root, ".env", "needle=1\n");
        let msgs = run(root, &q("needle"));
        let found = matches_of(&msgs);
        assert_eq!(
            rel_paths(root, &found),
            [".env", "src/a.rs", "src/sub/keep.rs"]
        );
        assert!(matches!(
            msgs.last(),
            Some(SearchMsg::Done { matches: 3, .. })
        ));
        // In a repository the .git directory itself is never searched.
        write(root, ".git/config", "needle\n");
        let found = matches_of(&run(root, &q("needle")));
        assert_eq!(
            rel_paths(root, &found),
            [".env", "src/a.rs", "src/sub/keep.rs"]
        );
    }

    #[test]
    fn project_search_streams() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for i in 0..2000 {
            write(
                root,
                &format!("d{}/f{i}.txt", i % 20),
                format!("one\nfind me {i}\n"),
            );
        }
        let (tx, rx) = mpsc::channel();
        let handle = search_project(root, &q("find me"), Arc::new(AtomicBool::new(false)), tx);
        let first = rx.recv().unwrap();
        let first_at = Instant::now();
        assert!(
            matches!(first, SearchMsg::Batch(_)),
            "first message: {first:?}"
        );
        let mut msgs = vec![first];
        msgs.extend(rx.iter());
        handle.join().unwrap();
        let batches: Vec<usize> = msgs
            .iter()
            .filter_map(|m| match m {
                SearchMsg::Batch(b) => Some(b.len()),
                _ => None,
            })
            .collect();
        assert!(batches.len() >= 10, "batches: {batches:?}");
        assert!(batches.iter().all(|&n| (1..=MAX_BATCH).contains(&n)));
        assert_eq!(batches.iter().sum::<usize>(), 2000);
        let Some(SearchMsg::Done {
            files,
            matches,
            elapsed,
        }) = msgs.last()
        else {
            panic!("last message is not Done: {:?}", msgs.last());
        };
        assert_eq!((*files, *matches), (2000, 2000));
        assert!(first_at.elapsed() <= *elapsed);
        let m = matches_of(&msgs)
            .into_iter()
            .find(|m| m.path.ends_with("d7/f7.txt"))
            .unwrap();
        assert_eq!(
            (m.line, m.col, m.line_text.as_str(), m.match_range),
            (2, 1, "find me 7", (0, 7))
        );
    }

    #[test]
    fn forward_batches_flushes_on_timer_while_walk_runs() {
        // A slow walk: the first match must reach the UI after ~FLUSH_EVERY, not at the end.
        let (mtx, mrx) = mpsc::channel();
        let (tx, rx) = mpsc::channel();
        let cancel = AtomicBool::new(false);
        let producer = thread::spawn(move || {
            mtx.send(vec![pm("a", 1)]).unwrap();
            let got = rx.recv_timeout(Duration::from_secs(2));
            mtx.send(vec![pm("b", 1)]).unwrap();
            (got, rx)
        });
        let sent = forward_batches(&mrx, &tx, &cancel, MAX_BATCH, FLUSH_EVERY);
        drop(tx);
        let (got, rx) = producer.join().unwrap();
        assert!(
            matches!(got, Ok(SearchMsg::Batch(ref b)) if b.len() == 1),
            "{got:?}"
        );
        assert_eq!(sent, 2);
        assert_eq!(rx.iter().count(), 1);
    }

    fn pm(path: &str, line: usize) -> ProjectMatch {
        ProjectMatch {
            path: path.into(),
            line,
            col: 1,
            line_text: String::new(),
            match_range: (0, 0),
        }
    }

    #[test]
    fn project_search_cancel_stops() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let body = format!("{}hit\n", "filler line\n".repeat(64));
        for i in 0..3000 {
            write(root, &format!("d{}/f{i}.txt", i % 30), &body);
        }
        // Cancelled before it starts: nothing is searched or streamed.
        let (tx, rx) = mpsc::channel();
        let handle = search_project(root, &q("hit"), Arc::new(AtomicBool::new(true)), tx);
        let msgs: Vec<SearchMsg> = rx.iter().collect();
        handle.join().unwrap();
        assert!(
            matches!(
                msgs[..],
                [SearchMsg::Done {
                    files: 0,
                    matches: 0,
                    ..
                }]
            ),
            "{msgs:?}"
        );
        // Cancelled after the first batch: the walk stops early and still ends with Done.
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let handle = search_project(root, &q("hit"), cancel.clone(), tx);
        let first = rx.recv().unwrap();
        cancel.store(true, Ordering::Relaxed);
        let mut msgs = vec![first];
        msgs.extend(rx.iter());
        handle.join().unwrap();
        let Some(SearchMsg::Done { files, matches, .. }) = msgs.last() else {
            panic!("no Done: {msgs:?}");
        };
        assert!(
            *files < 3000 && *matches < 3000,
            "walk did not stop: {files} files"
        );
        assert_eq!(matches_of(&msgs).len(), *matches);
    }

    #[test]
    fn project_search_skips_binary() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "text.txt", "needle\n");
        write(root, "blob.bin", b"needle\0\x01\x02");
        write(root, "latin1.txt", b"needle caf\xe9\n");
        let msgs = run(root, &q("needle"));
        assert_eq!(rel_paths(root, &matches_of(&msgs)), ["text.txt"]);
        assert!(matches!(
            msgs.last(),
            Some(SearchMsg::Done {
                files: 1,
                matches: 1,
                ..
            })
        ));
    }

    #[test]
    fn project_search_line_numbers_follow_d31() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // CRLF + BOM goes through grep-searcher; a lone \r forces the exact line split.
        write(root, "crlf.txt", "\u{feff}x needle\r\nno\r\n  needle$\r\n");
        write(root, "cr.txt", "a\rneedle\rb\n needle");
        write(root, "uni.txt", "Tiếng Việt needle needle\n");
        let re = |p: &str| Query {
            regex: true,
            ..q(p)
        };
        let found = matches_of(&run(root, &re("needle")));
        type Row = (String, usize, usize, (usize, usize), String);
        let got: Vec<Row> = found
            .iter()
            .map(|m| {
                let name = m.path.file_name().unwrap().to_string_lossy().into_owned();
                (name, m.line, m.col, m.match_range, m.line_text.clone())
            })
            .collect();
        assert_eq!(
            got,
            [
                ("cr.txt".into(), 2, 1, (0, 6), "needle".into()),
                ("cr.txt".into(), 4, 2, (1, 7), " needle".into()),
                ("crlf.txt".into(), 1, 3, (2, 8), "x needle".into()),
                ("crlf.txt".into(), 3, 3, (2, 8), "  needle$".into()),
                (
                    "uni.txt".into(),
                    1,
                    12,
                    (11, 17),
                    "Tiếng Việt needle needle".into()
                ),
                (
                    "uni.txt".into(),
                    1,
                    19,
                    (18, 24),
                    "Tiếng Việt needle needle".into()
                ),
            ]
        );
        // Anchors are per line in both paths, and `$` sits before the CRLF.
        let found = matches_of(&run(root, &re(r"^\s*needle\$?$")));
        let lines: Vec<(String, usize)> = found
            .iter()
            .map(|m| {
                (
                    m.path.file_name().unwrap().to_string_lossy().into_owned(),
                    m.line,
                )
            })
            .collect();
        assert_eq!(
            lines,
            [
                ("cr.txt".into(), 2),
                ("cr.txt".into(), 4),
                ("crlf.txt".into(), 3)
            ]
        );
    }

    #[test]
    fn project_search_long_line_is_windowed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let line = format!("{}needle{}", "é".repeat(5000), "x".repeat(5000));
        write(root, "min.js", &line);
        let found = matches_of(&run(root, &q("needle")));
        assert_eq!(found.len(), 1);
        let m = &found[0];
        assert_eq!((m.line, m.col), (1, 5001));
        assert_eq!(m.line_text.chars().count(), WINDOW_CHARS);
        let (a, b) = m.match_range;
        assert_eq!(
            m.line_text.chars().skip(a).take(b - a).collect::<String>(),
            "needle"
        );
    }

    #[test]
    fn project_search_reports_invalid_regex_and_bad_root() {
        let dir = tempfile::tempdir().unwrap();
        let bad = Query {
            regex: true,
            ..q("(")
        };
        let msgs = run(dir.path(), &bad);
        assert!(matches!(&msgs[0], SearchMsg::Error(e) if e.contains("invalid regex")));
        assert!(matches!(msgs[1], SearchMsg::Done { matches: 0, .. }));
        let msgs = run(&dir.path().join("missing"), &q("x"));
        assert!(matches!(&msgs[0], SearchMsg::Error(_)));
        assert!(matches!(msgs[1], SearchMsg::Done { .. }));
    }

    /// Manual measurement for AC-11 on a real tree (not run by default):
    /// `VR_SEARCH_BENCH_ROOT=<dir> VR_SEARCH_BENCH_TERM=<term> cargo test -p vr-search --release
    /// -- --ignored --nocapture project_search_first_batch_latency`
    #[test]
    #[ignore]
    fn project_search_first_batch_latency() {
        let Ok(root) = std::env::var("VR_SEARCH_BENCH_ROOT") else {
            return;
        };
        let term = std::env::var("VR_SEARCH_BENCH_TERM").unwrap_or_else(|_| "fn ".into());
        let (tx, rx) = mpsc::channel();
        let started = Instant::now();
        let handle = search_project(
            Path::new(&root),
            &q(&term),
            Arc::new(AtomicBool::new(false)),
            tx,
        );
        let mut first = None;
        let mut batches = 0;
        for msg in rx.iter() {
            match msg {
                SearchMsg::Batch(_) => {
                    batches += 1;
                    first.get_or_insert(started.elapsed());
                }
                SearchMsg::Done {
                    files,
                    matches,
                    elapsed,
                } => {
                    println!(
                        "first batch {first:?}, {batches} batches, {files} files, {matches} matches, done {elapsed:?}"
                    );
                }
                SearchMsg::Error(e) => println!("error: {e}"),
            }
        }
        handle.join().unwrap();
    }

    #[test]
    fn project_search_skips_oversized_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "small.txt", "needle\n");
        write(root, "big.sql", format!("needle\n{}", "x".repeat(200)));
        let (tx, rx) = mpsc::channel();
        let limits = Limits {
            max_file_bytes: 100,
            ..Limits::default()
        };
        let handle = search_project_with(
            root,
            &q("needle"),
            Arc::new(AtomicBool::new(false)),
            tx,
            limits,
        );
        let msgs: Vec<SearchMsg> = rx.iter().collect();
        handle.join().unwrap();
        assert_eq!(rel_paths(root, &matches_of(&msgs)), ["small.txt"]);
        let errors: Vec<&String> = msgs
            .iter()
            .filter_map(|m| match m {
                SearchMsg::Error(e) => Some(e),
                _ => None,
            })
            .collect();
        assert_eq!(errors.len(), 1, "{msgs:?}");
        assert!(
            errors[0].contains("1 file") && errors[0].contains("larger than"),
            "{}",
            errors[0]
        );
        assert!(matches!(
            msgs.last(),
            Some(SearchMsg::Done {
                files: 1,
                matches: 1,
                ..
            })
        ));
    }

    #[test]
    fn project_search_prefilter_never_drops_matches() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let text = "x\nfoo\nfoo bar\n";
        write(root, "a.txt", text);
        let rope = ropey::Rope::from_str(text);
        for pattern in [
            r"foo(?-m)$",
            r"(?-m)^foo",
            r"(?m)foo$",
            r"(?s)foo.",
            r"(?i)FOO",
        ] {
            let query = Query {
                regex: true,
                case_sensitive: true,
                ..q(pattern)
            };
            let in_buffer = crate::find_all(&rope, &query).unwrap().len();
            let in_project = matches_of(&run(root, &query)).len();
            assert_eq!(in_project, in_buffer, "{pattern}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn project_search_lists_a_linked_file_once() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "a.txt", "needle\n");
        std::os::unix::fs::symlink("a.txt", root.join("link.txt")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "o.txt", "needle\n");
        std::os::unix::fs::symlink(outside.path().join("o.txt"), root.join("ext.txt")).unwrap();
        let found = matches_of(&run(root, &q("needle")));
        // The in-project target is reported under its own name; a link out of the project is kept.
        assert_eq!(rel_paths(root, &found), ["a.txt", "ext.txt"]);
    }
}
