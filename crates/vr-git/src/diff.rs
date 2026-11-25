//! Structured diffs (hunks with old/new line numbers) and gutter line changes.

use std::borrow::Cow;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::cmd::Limits;
use crate::parse::{fields, lossy, os, parse_err, path_from, unquote};
use crate::repo::{FileStatus, Repo};
use crate::{GitError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

/// One line of a hunk. `text` is the raw line without its `\n` (non-UTF-8 kept for patches).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    /// 1-based line in the old file (context and removed lines).
    pub old_line: Option<u32>,
    /// 1-based line in the new file (context and added lines).
    pub new_line: Option<u32>,
    pub text: Vec<u8>,
    /// Followed by `\ No newline at end of file`.
    pub no_newline: bool,
}

impl DiffLine {
    pub fn text_lossy(&self) -> String {
        lossy(&self.text)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: u32,
    pub old_len: u32,
    pub new_start: u32,
    pub new_len: u32,
    /// The `@@ ... @@` line as git printed it (function context included).
    pub header: String,
    pub lines: Vec<DiffLine>,
}

/// The diff of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    /// `None` for an added file.
    pub old_path: Option<PathBuf>,
    /// `None` for a deleted file.
    pub new_path: Option<PathBuf>,
    pub status: FileStatus,
    pub old_mode: Option<String>,
    pub new_mode: Option<String>,
    /// Full blob ids from the `index` line (absent for untracked or mode-only changes).
    pub old_oid: Option<String>,
    pub new_oid: Option<String>,
    pub similarity: Option<u8>,
    pub binary: bool,
    pub hunks: Vec<Hunk>,
}

impl FileDiff {
    fn empty() -> FileDiff {
        FileDiff {
            old_path: None,
            new_path: None,
            status: FileStatus::Modified,
            old_mode: None,
            new_mode: None,
            old_oid: None,
            new_oid: None,
            similarity: None,
            binary: false,
            hunks: Vec::new(),
        }
    }

    /// A header without a change: `diff-files` lists a file whose stat info is stale but whose
    /// content and mode equal the index.
    fn stat_only(&self) -> bool {
        self.status == FileStatus::Modified
            && self.hunks.is_empty()
            && !self.binary
            && self.old_mode == self.new_mode
    }

    /// The path the file has after the change (or before, when deleted).
    pub fn path(&self) -> Option<&Path> {
        self.new_path.as_deref().or(self.old_path.as_deref())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
}

/// A gutter mark. `start` is the 0-based buffer line of the first added/modified line; for
/// `Deleted` it is the number of buffer lines above the deletion point and `len` the number of
/// removed lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineChange {
    pub kind: ChangeKind,
    pub start: u32,
    pub len: u32,
}

const DIFF_ARGS: &[&str] = &[
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--src-prefix=a/",
    "--dst-prefix=b/",
    "--no-relative",
    "-U3",
    "--full-index",
    "-M",
];

/// Bytes git inspects to call a file binary.
const BINARY_PROBE: usize = 8000;

impl Repo {
    fn file_limits(&self, user: bool) -> Limits {
        let o = self.git.opts();
        Limits::new(if user { o.user_timeout } else { o.timeout }, o.max_file)
    }

    /// Worktree vs index of one file; an untracked file shows as all added, a conflicted one as
    /// [`FileStatus::Conflicted`] without hunks. `None` when unchanged.
    pub fn diff_unstaged(&self, path: &Path) -> Result<Option<FileDiff>> {
        let rel = self.relative(path)?;
        let entries = self.index_entries(&rel)?;
        if entries.iter().any(|e| e.stage != 0) {
            let mut d = FileDiff::empty();
            d.old_path = Some(rel.clone());
            d.new_path = Some(rel);
            d.status = FileStatus::Conflicted;
            return Ok(Some(d));
        }
        if entries.is_empty() {
            return self.untracked_diff(&rel);
        }
        // Plumbing, not `git diff`: the porcelain refreshes and rewrites the index after diffing.
        let mut args: Vec<&OsStr> = vec![os("diff-files"), os("--ignore-submodules=dirty")];
        args.extend(DIFF_ARGS.iter().map(|a| os(a)));
        args.extend([os("--"), rel.as_os_str()]);
        let out = self
            .git
            .run_ok(self.workdir(), &args, None, self.file_limits(false))?;
        Ok(parse_diff(&out)?.into_iter().find(|d| !d.stat_only()))
    }

    /// Index vs HEAD of one file, renames detected. `None` when nothing is staged for it.
    pub fn diff_staged(&self, path: &Path) -> Result<Option<FileDiff>> {
        let rel = self.relative(path)?;
        let base = if self.head_oid()?.is_some() {
            "HEAD"
        } else {
            EMPTY_TREE
        };
        let names = [
            os("diff-index"),
            os("--cached"),
            os("-z"),
            os("--name-status"),
            os("-M"),
            os("--end-of-options"),
            os(base),
            os("--"),
        ];
        let listing = self.run(&names, false)?;
        let Some(paths) = pair_for(&listing, &rel) else {
            return Ok(None);
        };
        let mut args: Vec<&OsStr> = vec![os("diff-index"), os("--cached")];
        args.extend(DIFF_ARGS.iter().map(|a| os(a)));
        args.extend([os("--end-of-options"), os(base), os("--")]);
        args.extend(paths.iter().map(|p| p.as_os_str()));
        let out = self
            .git
            .run_ok(self.workdir(), &args, None, self.file_limits(false))?;
        Ok(parse_diff(&out)?
            .into_iter()
            .find(|d| d.path() == Some(rel.as_path())))
    }

    /// A commit against its first parent (the empty tree for a root commit), all files or the one
    /// at `path` (renames detected). `rev` is resolved with [`Repo::resolve_commit`] first.
    pub fn diff_commit(&self, rev: &str, path: Option<&Path>) -> Result<Vec<FileDiff>> {
        let oid = self.resolve_commit(rev)?;
        let rel = path.map(|p| self.relative(p)).transpose()?;
        let parent = self.run(
            &[
                os("rev-parse"),
                os("--verify"),
                os("-q"),
                os("--end-of-options"),
                os(&format!("{oid}^1")),
            ],
            true,
        );
        let base = match parent {
            Ok(p) => lossy(p.trim_ascii_end()),
            Err(GitError::Failed { .. }) => EMPTY_TREE.to_string(),
            Err(e) => return Err(e),
        };
        let mut paths: Vec<PathBuf> = Vec::new();
        if let Some(rel) = &rel {
            let names = [
                os("diff-tree"),
                os("-r"),
                os("-z"),
                os("--name-status"),
                os("-M"),
                os("--end-of-options"),
                os(&base),
                os(&oid),
                os("--"),
            ];
            let listing = self.run(&names, true)?;
            match pair_for(&listing, rel) {
                Some(p) => paths = p,
                None => return Ok(Vec::new()),
            }
        }
        let mut args: Vec<&OsStr> = vec![os("diff-tree"), os("-r")];
        args.extend(DIFF_ARGS.iter().map(|a| os(a)));
        args.extend([os("--end-of-options"), os(&base), os(&oid), os("--")]);
        args.extend(paths.iter().map(|p| p.as_os_str()));
        let lim = if rel.is_some() {
            self.file_limits(true)
        } else {
            self.git.user()
        };
        let out = self.git.run_ok(self.workdir(), &args, None, lim)?;
        let diffs = parse_diff(&out)?;
        Ok(match &rel {
            Some(rel) => diffs
                .into_iter()
                .filter(|d| d.path() == Some(rel.as_path()))
                .collect(),
            None => diffs,
        })
    }

    /// Gutter marks of `buffer` (the editor's text, saved or not) against the index version of
    /// `path`. `None` when the file is not in the index (untracked), conflicted or binary.
    pub fn line_changes(&self, path: &Path, buffer: &str) -> Result<Option<Vec<LineChange>>> {
        let rel = self.relative(path)?;
        let max = self.git.opts().max_file;
        if buffer.len() > max {
            return Err(GitError::TooLarge {
                cmd: "line_changes".into(),
                limit: max,
            });
        }
        let entries = self.index_entries(&rel)?;
        let [entry] = entries.as_slice() else {
            return Ok(None);
        };
        if entry.stage != 0 {
            return Ok(None);
        }
        let base = self.blob(&entry.oid)?;
        if is_binary(&base) {
            return Ok(None);
        }
        let buffer = self.clean_eol(&rel, &base, buffer)?;
        Ok(Some(line_changes_between(&lossy(&base), &buffer)))
    }

    /// `buffer` with line endings as git's clean conversion stores it, so the marks equal
    /// `git diff` (which compares the cleaned worktree file with the index). The index blob
    /// is compared raw: a smudged base (`cat-file --filters`) would still differ for
    /// `text=auto` files kept with CRLF in the worktree. Only eol is converted here; `ident`
    /// and `working-tree-encoding` are not (the editor does not edit non-UTF-8 text).
    fn clean_eol<'a>(&self, rel: &Path, base: &[u8], buffer: &'a str) -> Result<Cow<'a, str>> {
        if !buffer.contains("\r\n") {
            return Ok(Cow::Borrowed(buffer));
        }
        let convert = match self.eol_action(rel)? {
            EolAction::Text => true,
            // git's safe-autocrlf rule: CRLF already in the index stays.
            EolAction::Auto => !has_crlf(base) && !auto_binary(buffer.as_bytes()),
            EolAction::Keep => false,
        };
        Ok(if convert {
            Cow::Owned(buffer.replace("\r\n", "\n"))
        } else {
            Cow::Borrowed(buffer)
        })
    }

    /// The clean-side end-of-line handling git applies to `rel` (convert.c: the `text`, legacy
    /// `crlf` and `eol` attributes, then `core.autocrlf`).
    fn eol_action(&self, rel: &Path) -> Result<EolAction> {
        let out = self.run(
            &[
                os("check-attr"),
                os("-z"),
                os("text"),
                os("crlf"),
                os("eol"),
                os("--"),
                rel.as_os_str(),
            ],
            false,
        )?;
        let f = fields(&out);
        let value = |name: &[u8]| -> &[u8] {
            f.chunks(3)
                .find(|c| c.len() == 3 && c[1] == name)
                .map_or(b"unspecified", |c| c[2])
        };
        let from_text = |v: &[u8]| match v {
            b"set" | b"input" => Some(EolAction::Text),
            b"unset" => Some(EolAction::Keep),
            b"auto" => Some(EolAction::Auto),
            _ => None,
        };
        let action = from_text(value(b"text")).or_else(|| from_text(value(b"crlf")));
        let eol_set = matches!(value(b"eol"), b"lf" | b"crlf");
        match action {
            Some(a) => return Ok(a),
            None if eol_set => return Ok(EolAction::Text),
            None => {}
        }
        let out = self.run_code(&[os("config"), os("--get"), os("core.autocrlf")])?;
        let v = lossy(out.stdout.trim_ascii()).to_ascii_lowercase();
        Ok(match v.as_str() {
            "true" | "yes" | "on" | "1" | "input" if out.code == 0 => EolAction::Auto,
            _ => EolAction::Keep,
        })
    }

    fn untracked_diff(&self, rel: &Path) -> Result<Option<FileDiff>> {
        let full = self.workdir().join(rel);
        let meta = match std::fs::symlink_metadata(&full) {
            Ok(m) => m,
            Err(_) => return Ok(None),
        };
        let max = self.git.opts().max_file;
        if !meta.is_file() {
            return Ok(None);
        }
        if meta.len() > max as u64 {
            return Err(GitError::TooLarge {
                cmd: "read".into(),
                limit: max,
            });
        }
        let data = std::fs::read(&full)?;
        let mut d = FileDiff::empty();
        d.new_path = Some(rel.to_path_buf());
        d.status = FileStatus::Untracked;
        if is_binary(&data) {
            d.binary = true;
            return Ok(Some(d));
        }
        if data.is_empty() {
            return Ok(Some(d));
        }
        let mut lines = Vec::new();
        let ends_nl = data.ends_with(b"\n");
        let body = if ends_nl {
            &data[..data.len() - 1]
        } else {
            &data[..]
        };
        for (i, l) in body.split(|&b| b == b'\n').enumerate() {
            lines.push(DiffLine {
                kind: LineKind::Added,
                old_line: None,
                new_line: Some(i as u32 + 1),
                text: l.to_vec(),
                no_newline: false,
            });
        }
        if !ends_nl && let Some(last) = lines.last_mut() {
            last.no_newline = true;
        }
        let n = lines.len() as u32;
        d.hunks.push(Hunk {
            old_start: 0,
            old_len: 0,
            new_start: 1,
            new_len: n,
            header: format!("@@ -0,0 +1,{n} @@"),
            lines,
        });
        Ok(Some(d))
    }
}

/// Git's well-known empty tree, the base of a root commit or an unborn HEAD.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

fn is_binary(data: &[u8]) -> bool {
    data[..data.len().min(BINARY_PROBE)].contains(&0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EolAction {
    /// CRLF becomes LF.
    Text,
    /// Like `Text` unless the content looks binary or the index already has CRLF.
    Auto,
    /// Stored as is.
    Keep,
}

fn has_crlf(data: &[u8]) -> bool {
    data.windows(2).any(|w| w == b"\r\n")
}

/// git's `convert_is_binary` for `text=auto`: a lone CR, a NUL, or more than 1 in 128
/// control characters.
fn auto_binary(data: &[u8]) -> bool {
    let (mut printable, mut nonprintable) = (0usize, 0usize);
    for (i, &b) in data.iter().enumerate() {
        match b {
            b'\r' if data.get(i + 1) != Some(&b'\n') => return true,
            0 => return true,
            b'\r' | b'\n' => {}
            127 => nonprintable += 1,
            // BS, TAB, ESC and FF count as printable.
            8 | 9 | 27 | 12 => printable += 1,
            1..=31 => nonprintable += 1,
            _ => printable += 1,
        }
    }
    (printable >> 7) < nonprintable
}

/// From `--name-status -z` output, the paths of the entry whose new (or only) path is `rel`:
/// `[old, new]` for a rename/copy, `[rel]` otherwise.
fn pair_for(listing: &[u8], rel: &Path) -> Option<Vec<PathBuf>> {
    let f = fields(listing);
    let mut i = 0;
    while i < f.len() {
        let st = f[i];
        let two = matches!(st.first(), Some(b'R') | Some(b'C'));
        if two && i + 2 < f.len() {
            let (old, new) = (path_from(f[i + 1]), path_from(f[i + 2]));
            if new == rel {
                return Some(vec![old, new]);
            }
            i += 3;
        } else {
            if f.get(i + 1).is_some_and(|p| path_from(p) == rel) {
                return Some(vec![rel.to_path_buf()]);
            }
            i += 2;
        }
    }
    None
}

/// Marks for `new` against `old`, using the same line model as `git diff -U0`.
pub(crate) fn line_changes_between(old: &str, new: &str) -> Vec<LineChange> {
    use similar::{Algorithm, DiffOp, TextDiff};
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Myers)
        .deadline(Instant::now() + Duration::from_millis(500))
        .diff_lines(old, new);
    let mut out = Vec::new();
    for op in diff.ops() {
        match *op {
            DiffOp::Equal { .. } => {}
            DiffOp::Insert {
                new_index, new_len, ..
            } => out.push(LineChange {
                kind: ChangeKind::Added,
                start: new_index as u32,
                len: new_len as u32,
            }),
            DiffOp::Delete {
                new_index, old_len, ..
            } => out.push(LineChange {
                kind: ChangeKind::Deleted,
                start: new_index as u32,
                len: old_len as u32,
            }),
            DiffOp::Replace {
                new_index, new_len, ..
            } => out.push(LineChange {
                kind: ChangeKind::Modified,
                start: new_index as u32,
                len: new_len as u32,
            }),
        }
    }
    // similar may emit a delete next to an insert; git -U0 reports them as one modified hunk.
    let mut merged: Vec<LineChange> = Vec::with_capacity(out.len());
    for c in out {
        if let Some(prev) = merged.last_mut() {
            let touches = prev.start
                + if prev.kind == ChangeKind::Deleted {
                    0
                } else {
                    prev.len
                }
                == c.start;
            match (prev.kind, c.kind) {
                (ChangeKind::Deleted, ChangeKind::Added) if prev.start == c.start => {
                    *prev = LineChange {
                        kind: ChangeKind::Modified,
                        start: c.start,
                        len: c.len,
                    };
                    continue;
                }
                (ChangeKind::Added | ChangeKind::Modified, ChangeKind::Deleted) if touches => {
                    prev.kind = ChangeKind::Modified;
                    continue;
                }
                _ => {}
            }
        }
        merged.push(c);
    }
    merged
}

/// Parse unified diff output of `git diff` (with the fixed prefixes of [`DIFF_ARGS`]).
pub(crate) fn parse_diff(out: &[u8]) -> Result<Vec<FileDiff>> {
    let mut files: Vec<FileDiff> = Vec::new();
    let mut header_names: Option<(PathBuf, PathBuf)> = None;
    let mut combined = false;
    let mut remaining = (0u32, 0u32);
    let (mut old_no, mut new_no) = (0u32, 0u32);
    let mut lines = out.split(|&b| b == b'\n').peekable();
    while let Some(line) = lines.next() {
        if lines.peek().is_none() && line.is_empty() {
            break;
        }
        let in_hunk = remaining.0 > 0 || remaining.1 > 0;
        if in_hunk {
            let file = files
                .last_mut()
                .ok_or_else(|| parse_err("diff", "hunk before file"))?;
            let hunk = file
                .hunks
                .last_mut()
                .ok_or_else(|| parse_err("diff", "line before hunk"))?;
            let (kind, text) = match line.first() {
                Some(b' ') => (LineKind::Context, &line[1..]),
                Some(b'+') => (LineKind::Added, &line[1..]),
                Some(b'-') => (LineKind::Removed, &line[1..]),
                Some(b'\\') => {
                    if let Some(l) = hunk.lines.last_mut() {
                        l.no_newline = true;
                    }
                    continue;
                }
                None => (LineKind::Context, line),
                _ => {
                    return Err(parse_err(
                        "diff",
                        format!("unexpected hunk line {}", lossy(line)),
                    ));
                }
            };
            let (o, n) = match kind {
                LineKind::Context => {
                    old_no += 1;
                    new_no += 1;
                    remaining = (remaining.0.saturating_sub(1), remaining.1.saturating_sub(1));
                    (Some(old_no - 1), Some(new_no - 1))
                }
                LineKind::Removed => {
                    old_no += 1;
                    remaining.0 = remaining.0.saturating_sub(1);
                    (Some(old_no - 1), None)
                }
                LineKind::Added => {
                    new_no += 1;
                    remaining.1 = remaining.1.saturating_sub(1);
                    (None, Some(new_no - 1))
                }
            };
            hunk.lines.push(DiffLine {
                kind,
                old_line: o,
                new_line: n,
                text: text.to_vec(),
                no_newline: false,
            });
            continue;
        }
        if line.starts_with(b"\\") {
            // "\ No newline at end of file" right after the last line of a hunk.
            if let Some(l) = files
                .last_mut()
                .and_then(|f| f.hunks.last_mut())
                .and_then(|h| h.lines.last_mut())
            {
                l.no_newline = true;
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix(b"diff --git ") {
            if let Some(f) = files.last_mut() {
                finish(f, header_names.take());
            }
            files.push(FileDiff::empty());
            header_names = split_header(rest);
            combined = false;
            continue;
        }
        if line.starts_with(b"diff --cc ") || line.starts_with(b"diff --combined ") {
            if let Some(f) = files.last_mut() {
                finish(f, header_names.take());
            }
            let mut d = FileDiff::empty();
            d.status = FileStatus::Conflicted;
            let name = path_from(&unquote(
                &line[line.iter().position(|&b| b == b' ').unwrap_or(0) + 1..][3..],
            ));
            d.old_path = Some(name.clone());
            d.new_path = Some(name);
            files.push(d);
            combined = true;
            continue;
        }
        if combined {
            continue;
        }
        let Some(file) = files.last_mut() else {
            continue;
        };
        if let Some(h) = line.strip_prefix(b"@@ ") {
            let hunk = parse_hunk_header(h).ok_or_else(|| parse_err("diff", lossy(line)))?;
            old_no = hunk.old_start.max(1);
            new_no = hunk.new_start.max(1);
            if hunk.old_len == 0 {
                old_no = hunk.old_start + 1;
            }
            if hunk.new_len == 0 {
                new_no = hunk.new_start + 1;
            }
            remaining = (hunk.old_len, hunk.new_len);
            file.hunks.push(Hunk {
                header: lossy(line),
                ..hunk
            });
        } else if let Some(p) = line.strip_prefix(b"--- ") {
            file.old_path = side_path(p, b"a/");
        } else if let Some(p) = line.strip_prefix(b"+++ ") {
            file.new_path = side_path(p, b"b/");
        } else if let Some(p) = line
            .strip_prefix(b"rename from ")
            .or_else(|| line.strip_prefix(b"copy from "))
        {
            file.old_path = Some(path_from(&unquote(p)));
            file.status = if line.starts_with(b"rename") {
                FileStatus::Renamed
            } else {
                FileStatus::Copied
            };
        } else if let Some(p) = line
            .strip_prefix(b"rename to ")
            .or_else(|| line.strip_prefix(b"copy to "))
        {
            file.new_path = Some(path_from(&unquote(p)));
        } else if let Some(m) = line.strip_prefix(b"new file mode ") {
            file.status = FileStatus::Added;
            file.new_mode = Some(lossy(m));
        } else if let Some(m) = line.strip_prefix(b"deleted file mode ") {
            file.status = FileStatus::Deleted;
            file.old_mode = Some(lossy(m));
        } else if let Some(m) = line.strip_prefix(b"old mode ") {
            file.old_mode = Some(lossy(m));
        } else if let Some(m) = line.strip_prefix(b"new mode ") {
            file.new_mode = Some(lossy(m));
        } else if let Some(s) = line.strip_prefix(b"similarity index ") {
            file.similarity = lossy(s).trim_end_matches('%').parse().ok();
        } else if let Some(ix) = line.strip_prefix(b"index ") {
            let ix = lossy(ix);
            let ids = ix.split(' ').next().unwrap_or_default();
            if let Some((a, b)) = ids.split_once("..") {
                file.old_oid = Some(a.to_string());
                file.new_oid = Some(b.to_string());
            }
            if let Some(mode) = ix.split(' ').nth(1) {
                file.old_mode.get_or_insert_with(|| mode.to_string());
                file.new_mode.get_or_insert_with(|| mode.to_string());
            }
        } else if line.starts_with(b"Binary files ") || line.starts_with(b"GIT binary patch") {
            file.binary = true;
        }
    }
    if let Some(f) = files.last_mut() {
        finish(f, header_names.take());
    }
    Ok(files)
}

/// Fill paths the `---`/`+++`/rename lines did not give (mode-only and binary changes).
fn finish(f: &mut FileDiff, names: Option<(PathBuf, PathBuf)>) {
    if f.status == FileStatus::Conflicted {
        return;
    }
    if let Some((a, b)) = names {
        if f.old_path.is_none() && f.status != FileStatus::Added {
            f.old_path = Some(a);
        }
        if f.new_path.is_none() && f.status != FileStatus::Deleted {
            f.new_path = Some(b);
        }
    }
    if f.status == FileStatus::Modified && f.old_mode.is_some() && f.new_mode.is_some() {
        let (o, n) = (
            f.old_mode.as_deref().unwrap_or(""),
            f.new_mode.as_deref().unwrap_or(""),
        );
        if o != n
            && (o.starts_with("120")
                || n.starts_with("120")
                || o.starts_with("160")
                || n.starts_with("160"))
        {
            f.status = FileStatus::TypeChanged;
        }
    }
}

fn side_path(p: &[u8], prefix: &[u8]) -> Option<PathBuf> {
    // git appends a TAB after an unquoted name that contains a space.
    let p = p.strip_suffix(b"\t").unwrap_or(p);
    if p == b"/dev/null" {
        return None;
    }
    let raw = unquote(p);
    Some(path_from(raw.strip_prefix(prefix).unwrap_or(&raw)))
}

/// Names from `diff --git a/X b/Y`: quoted tokens, or two equal halves for unquoted names.
fn split_header(rest: &[u8]) -> Option<(PathBuf, PathBuf)> {
    let strip = |v: Vec<u8>, pre: &[u8]| -> Option<PathBuf> { v.strip_prefix(pre).map(path_from) };
    if rest.first() == Some(&b'"') {
        let end = quoted_end(rest)?;
        let a = unquote(&rest[..=end]);
        let b_raw = rest.get(end + 2..)?;
        return Some((strip(a, b"a/")?, strip(unquote(b_raw), b"b/")?));
    }
    if rest.last() == Some(&b'"') {
        let start = rest.iter().position(|&b| b == b'"')?;
        let a = rest.get(..start.checked_sub(1)?)?.to_vec();
        return Some((strip(a, b"a/")?, strip(unquote(&rest[start..]), b"b/")?));
    }
    if rest.len() % 2 == 1 {
        let mid = rest.len() / 2;
        if rest[mid] == b' ' {
            let (a, b) = (&rest[..mid], &rest[mid + 1..]);
            if a.get(2..) == b.get(2..) {
                return Some((strip(a.to_vec(), b"a/")?, strip(b.to_vec(), b"b/")?));
            }
        }
    }
    None
}

fn quoted_end(s: &[u8]) -> Option<usize> {
    let mut i = 1;
    while i < s.len() {
        match s[i] {
            b'\\' => i += 2,
            b'"' => return Some(i),
            _ => i += 1,
        }
    }
    None
}

fn parse_hunk_header(h: &[u8]) -> Option<Hunk> {
    let text = lossy(h);
    let mut parts = text.split(' ');
    let old = parts.next()?.strip_prefix('-')?;
    let new = parts.next()?.strip_prefix('+')?;
    let range = |r: &str| -> Option<(u32, u32)> {
        match r.split_once(',') {
            Some((s, l)) => Some((s.parse().ok()?, l.parse().ok()?)),
            None => Some((r.parse().ok()?, 1)),
        }
    };
    let (old_start, old_len) = range(old)?;
    let (new_start, new_len) = range(new)?;
    Some(Hunk {
        old_start,
        old_len,
        new_start,
        new_len,
        header: String::new(),
        lines: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hunk_lines_carry_old_and_new_numbers() {
        let out = b"diff --git a/f b/f\nindex 1111111111111111111111111111111111111111..2222222222222222222222222222222222222222 100644\n--- a/f\n+++ b/f\n@@ -2,3 +2,3 @@ fn x\n a\n-b\n+B\n c\n\\ No newline at end of file\n";
        let d = parse_diff(out).unwrap();
        assert_eq!(d.len(), 1);
        let h = &d[0].hunks[0];
        assert_eq!(
            (h.old_start, h.old_len, h.new_start, h.new_len),
            (2, 3, 2, 3)
        );
        let nums: Vec<_> = h
            .lines
            .iter()
            .map(|l| (l.kind, l.old_line, l.new_line))
            .collect();
        assert_eq!(
            nums,
            vec![
                (LineKind::Context, Some(2), Some(2)),
                (LineKind::Removed, Some(3), None),
                (LineKind::Added, None, Some(3)),
                (LineKind::Context, Some(4), Some(4)),
            ]
        );
        assert!(h.lines[3].no_newline);
        assert_eq!(
            d[0].old_oid.as_deref(),
            Some("1111111111111111111111111111111111111111")
        );
        assert_eq!(d[0].new_mode.as_deref(), Some("100644"));
    }

    #[test]
    fn header_split_for_mode_only_change_with_spaces() {
        let out = b"diff --git a/a b c b/a b c\nold mode 100644\nnew mode 100755\n";
        let d = parse_diff(out).unwrap();
        assert_eq!(d[0].old_path.as_deref(), Some(Path::new("a b c")));
        assert_eq!(d[0].new_path.as_deref(), Some(Path::new("a b c")));
        assert!(d[0].hunks.is_empty());
    }

    #[test]
    fn line_changes_kinds() {
        let old = "a\nb\nc\nd\n";
        assert_eq!(
            line_changes_between(old, "a\nb\nX\nc\nd\n"),
            vec![LineChange {
                kind: ChangeKind::Added,
                start: 2,
                len: 1
            }]
        );
        assert_eq!(
            line_changes_between(old, "a\nB\nc\nd\n"),
            vec![LineChange {
                kind: ChangeKind::Modified,
                start: 1,
                len: 1
            }]
        );
        assert_eq!(
            line_changes_between(old, "a\nd\n"),
            vec![LineChange {
                kind: ChangeKind::Deleted,
                start: 1,
                len: 2
            }]
        );
        assert_eq!(line_changes_between(old, old), vec![]);
    }
}
