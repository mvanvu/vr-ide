//! Gutter line changes: the editor's buffer against the index version of a file.

use std::borrow::Cow;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::parse::{fields, lossy, os};
use crate::repo::Repo;
use crate::{GitError, Result};

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

/// Bytes git inspects to call a file binary.
const BINARY_PROBE: usize = 8000;

impl Repo {
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
        let out = self.run(&[
            os("check-attr"),
            os("-z"),
            os("text"),
            os("crlf"),
            os("eol"),
            os("--"),
            rel.as_os_str(),
        ])?;
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
        // `input` is the one non-boolean value; anything else follows git's own bool parsing
        // (a valueless key, `2` and `-1` are true).
        let raw = self.run_code(&[os("config"), os("--get"), os("core.autocrlf")])?;
        if raw.code != 0 {
            return Ok(EolAction::Keep);
        }
        if raw.stdout.trim_ascii().eq_ignore_ascii_case(b"input") {
            return Ok(EolAction::Auto);
        }
        let out = self.run_code(&[
            os("config"),
            os("--type=bool"),
            os("--get"),
            os("core.autocrlf"),
        ])?;
        Ok(if out.code == 0 && out.stdout.trim_ascii() == b"true" {
            EolAction::Auto
        } else {
            EolAction::Keep
        })
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;

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
