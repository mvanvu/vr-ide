//! Per-line blame (`git blame --porcelain`).

use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::parse::{is_oid, lossy, os, parse_err, path_from, unquote};
use crate::repo::Repo;
use crate::{GitError, Result};

/// The commit a group of lines comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameCommit {
    pub oid: String,
    pub author: Signature,
    pub summary: String,
    /// Lines not committed yet (worktree or editor buffer); git names the author
    /// "Not Committed Yet" — the UI shows "You, uncommitted".
    pub uncommitted: bool,
    /// The commit is the history boundary (the root commit).
    pub boundary: bool,
    /// The file's path in this commit.
    pub filename: PathBuf,
    /// Parent commit and the file's path there, for "blame before this change".
    pub previous: Option<(String, PathBuf)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlameLine {
    /// Index into [`Blame::commits`].
    pub commit: usize,
    /// 1-based line in that commit's version of the file.
    pub orig_line: u32,
    /// 1-based line in the blamed text.
    pub final_line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    /// Name and email after `.mailmap`.
    pub name: String,
    pub email: String,
    /// Unix seconds.
    pub time: i64,
}

/// Blame of a whole file: `lines[i]` is line `i + 1` of the blamed text.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Blame {
    pub commits: Vec<BlameCommit>,
    pub lines: Vec<BlameLine>,
}

impl Blame {
    /// The commit of 0-based line `line`.
    pub fn commit_of(&self, line: usize) -> Option<&BlameCommit> {
        self.lines.get(line).map(|l| &self.commits[l.commit])
    }
}

impl Repo {
    /// Blame of `buffer` (unsaved editor text) as the file's current content, for the automatic
    /// inline blame of the cursor line (automatic-read timeout); lines that differ from HEAD are
    /// uncommitted. `None` when the file is not tracked.
    pub fn inline_blame(&self, path: &Path, buffer: &str) -> Result<Option<Blame>> {
        let rel = self.relative(path)?;
        let max = self.git.opts().max_file;
        if buffer.len() > max {
            return Err(GitError::TooLarge {
                cmd: "blame".into(),
                limit: max,
            });
        }
        self.blame_inner(&rel, buffer.as_bytes())
    }

    fn blame_inner(&self, rel: &Path, buffer: &[u8]) -> Result<Option<Blame>> {
        if self.index_entries(rel)?.is_empty() {
            return Ok(None);
        }
        let args: Vec<&OsStr> = vec![
            os("blame"),
            os("--porcelain"),
            os("--contents"),
            os("-"),
            os("--"),
            rel.as_os_str(),
        ];
        let out = self
            .git
            .run(self.workdir(), &args, Some(buffer), self.git.auto())?;
        if out.code == 0 {
            return parse_blame(&out.stdout).map(Some);
        }
        // In the index but blame failed: not in HEAD yet (staged new file, unborn branch).
        let text = buffer;
        if self.head_oid()?.is_some() && self.path_in_head(rel)? {
            return Err(GitError::Failed {
                cmd: "blame".into(),
                code: out.code,
                stderr: out.stderr,
            });
        }
        Ok(Some(all_uncommitted(rel, text)))
    }

    fn path_in_head(&self, rel: &Path) -> Result<bool> {
        let out = self.run(&[
            os("ls-tree"),
            os("-z"),
            os("--name-only"),
            os("HEAD"),
            os("--"),
            rel.as_os_str(),
        ])?;
        Ok(!out.is_empty())
    }
}

const ZERO_OID: &str = "0000000000000000000000000000000000000000";

fn all_uncommitted(rel: &Path, text: &[u8]) -> Blame {
    let n = if text.is_empty() {
        0
    } else {
        text.split(|&b| b == b'\n').count() - usize::from(text.ends_with(b"\n"))
    };
    let commit = BlameCommit {
        oid: ZERO_OID.into(),
        author: Signature {
            name: "Not Committed Yet".into(),
            email: String::new(),
            time: 0,
        },
        summary: String::new(),
        uncommitted: true,
        boundary: false,
        filename: rel.to_path_buf(),
        previous: None,
    };
    let lines = (0..n as u32)
        .map(|i| BlameLine {
            commit: 0,
            orig_line: i + 1,
            final_line: i + 1,
        })
        .collect();
    Blame {
        commits: if n == 0 { Vec::new() } else { vec![commit] },
        lines,
    }
}

pub(crate) fn parse_blame(out: &[u8]) -> Result<Blame> {
    let mut blame = Blame::default();
    let mut index: HashMap<String, usize> = HashMap::new();
    let mut lines = out.split(|&b| b == b'\n');
    while let Some(head) = lines.next() {
        if head.is_empty() {
            continue;
        }
        let h = lossy(head);
        let mut p = h.split(' ');
        let (Some(oid), Some(orig), Some(fin)) = (p.next(), p.next(), p.next()) else {
            return Err(parse_err("blame", h.clone()));
        };
        let (Ok(orig), Ok(fin)) = (orig.parse::<u32>(), fin.parse::<u32>()) else {
            return Err(parse_err("blame", h.clone()));
        };
        if !is_oid(oid) {
            return Err(parse_err("blame", h.clone()));
        }
        let mut fields: Vec<(&[u8], &[u8])> = Vec::new();
        // Header fields until the TAB-prefixed content line.
        for l in lines.by_ref() {
            if l.first() == Some(&b'\t') {
                break;
            }
            let (k, v) = match l.iter().position(|&b| b == b' ') {
                Some(sp) => (&l[..sp], &l[sp + 1..]),
                None => (l, &l[l.len()..]),
            };
            fields.push((k, v));
        }
        let idx = match index.get(oid) {
            Some(&i) => i,
            None => {
                let get = |k: &[u8]| fields.iter().find(|(n, _)| *n == k).map(|(_, v)| *v);
                let name = get(b"author").map(lossy).unwrap_or_default();
                let email = get(b"author-mail").map(lossy).unwrap_or_default();
                let email = email
                    .trim_start_matches('<')
                    .trim_end_matches('>')
                    .to_string();
                let time = get(b"author-time")
                    .map(lossy)
                    .and_then(|t| t.parse().ok())
                    .unwrap_or(0);
                let previous = get(b"previous").and_then(|v| {
                    let sp = v.iter().position(|&b| b == b' ')?;
                    Some((lossy(&v[..sp]), path_from(&unquote(&v[sp + 1..]))))
                });
                blame.commits.push(BlameCommit {
                    oid: oid.to_string(),
                    author: Signature { name, email, time },
                    summary: get(b"summary").map(lossy).unwrap_or_default(),
                    uncommitted: oid.bytes().all(|b| b == b'0'),
                    boundary: get(b"boundary").is_some(),
                    filename: get(b"filename")
                        .map(|f| path_from(&unquote(f)))
                        .unwrap_or_default(),
                    previous,
                });
                index.insert(oid.to_string(), blame.commits.len() - 1);
                blame.commits.len() - 1
            }
        };
        blame.lines.push(BlameLine {
            commit: idx,
            orig_line: orig,
            final_line: fin,
        });
    }
    blame.lines.sort_by_key(|l| l.final_line);
    if blame
        .lines
        .iter()
        .enumerate()
        .any(|(i, l)| l.final_line as usize != i + 1)
    {
        return Err(parse_err("blame", "line numbers are not contiguous"));
    }
    Ok(blame)
}
