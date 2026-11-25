//! Commit history and commit details.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::Result;
use crate::parse::{fields, is_oid, lossy, os, parse_err, path_from};
use crate::repo::Repo;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    /// Name and email after `.mailmap`.
    pub name: String,
    pub email: String,
    /// Unix seconds.
    pub time: i64,
}

/// One line of a history list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitSummary {
    pub oid: String,
    pub short: String,
    pub parents: Vec<String>,
    pub author: Signature,
    pub committer: Signature,
    pub summary: String,
    /// The file's path in this commit when the log follows a file (changes across renames).
    pub path: Option<PathBuf>,
}

/// Full details of one commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitInfo {
    pub oid: String,
    pub parents: Vec<String>,
    pub author: Signature,
    pub committer: Signature,
    /// The raw message (subject, blank line, body).
    pub message: String,
}

impl CommitInfo {
    pub fn summary(&self) -> &str {
        self.message.lines().next().unwrap_or("")
    }
}

// The free-text field is always last, so no byte in it can shift the other fields.
const LOG_FORMAT: &str = "--format=%H%x00%h%x00%P%x00%aN%x00%aE%x00%at%x00%cN%x00%cE%x00%ct%x00%s";
const LOG_FIELDS: usize = 10;
const SHOW_FORMAT: &str = "--format=%H%x00%P%x00%aN%x00%aE%x00%at%x00%cN%x00%cE%x00%ct%x00%B";

impl Repo {
    /// Newest-first commits reachable from HEAD, at most `limit`; with `path`, the commits that
    /// touched that file, following renames (`git log --follow`). Empty for an unborn HEAD.
    pub fn log(&self, limit: usize, path: Option<&Path>) -> Result<Vec<CommitSummary>> {
        if self.head_oid()?.is_none() || limit == 0 {
            return Ok(Vec::new());
        }
        let rel = path.map(|p| self.relative(p)).transpose()?;
        let max = format!("--max-count={limit}");
        let mut args: Vec<&OsStr> = vec![os("log"), os("-z"), os(LOG_FORMAT), os(&max)];
        if rel.is_some() {
            args.extend([os("--follow"), os("--name-status"), os("-M")]);
        }
        args.extend([os("--end-of-options"), os("HEAD"), os("--")]);
        if let Some(rel) = &rel {
            args.push(rel.as_os_str());
        }
        let out = self.run(&args, true)?;
        parse_log(&out)
    }

    /// Details of the commit `rev` names (resolved with [`Repo::resolve_commit`]).
    pub fn commit(&self, rev: &str) -> Result<CommitInfo> {
        let oid = self.resolve_commit(rev)?;
        let out = self.run(
            &[
                os("show"),
                os("-s"),
                os(SHOW_FORMAT),
                os("--end-of-options"),
                os(&oid),
            ],
            true,
        )?;
        let mut parts = out.splitn(9, |&b| b == 0);
        let mut next = || {
            parts
                .next()
                .map(lossy)
                .ok_or_else(|| parse_err("show", "missing field"))
        };
        let oid = next()?;
        let parents = split_parents(&next()?);
        let author = signature(next()?, next()?, &next()?)?;
        let committer = signature(next()?, next()?, &next()?)?;
        let mut message = next()?;
        while message.ends_with('\n') || message.ends_with('\0') {
            message.pop();
        }
        if !is_oid(&oid) {
            return Err(parse_err("show", "bad commit id"));
        }
        Ok(CommitInfo {
            oid,
            parents,
            author,
            committer,
            message,
        })
    }
}

fn split_parents(s: &str) -> Vec<String> {
    s.split(' ')
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

fn signature(name: String, email: String, time: &str) -> Result<Signature> {
    let time = time
        .parse()
        .map_err(|_| parse_err("log", format!("bad time {time}")))?;
    Ok(Signature { name, email, time })
}

pub(crate) fn parse_log(out: &[u8]) -> Result<Vec<CommitSummary>> {
    let f = fields(out);
    let mut v = Vec::new();
    let mut i = 0;
    while i < f.len() {
        if f.len() - i < LOG_FIELDS {
            return Err(parse_err("log", "truncated record"));
        }
        let r: Vec<String> = f[i..i + LOG_FIELDS].iter().map(|b| lossy(b)).collect();
        i += LOG_FIELDS;
        if !is_oid(&r[0]) {
            return Err(parse_err("log", format!("bad commit id {}", r[0])));
        }
        let mut path = None;
        // With --name-status the record is followed by "\n<status>" and one or two paths.
        if let Some(st) = f.get(i).and_then(|s| s.strip_prefix(b"\n")) {
            let two = matches!(st.first(), Some(b'R') | Some(b'C'));
            let n = if two { 2 } else { 1 };
            if f.len() < i + 1 + n {
                return Err(parse_err("log", "truncated name-status"));
            }
            path = Some(path_from(f[i + n]));
            i += 1 + n;
        }
        let [oid, short, parents, an, ae, at, cn, ce, ct, summary] =
            <[String; LOG_FIELDS]>::try_from(r).expect("10 fields");
        v.push(CommitSummary {
            oid,
            short,
            parents: split_parents(&parents),
            author: signature(an, ae, &at)?,
            committer: signature(cn, ce, &ct)?,
            summary,
            path,
        });
    }
    Ok(v)
}
