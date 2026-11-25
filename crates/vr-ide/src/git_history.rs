//! The Git popup's History and Blame tabs and the commit view (M7-4b; D297 proposal, AC-16c,
//! AC-16e): `/git log`, `/git log file`, `/git blame`, `/git show <rev>`, keys `2` / `3`.
//!
//! - History: the repo's commits, or one file's following renames (`git log --follow`), loaded
//!   a page at a time ([`PAGE`]) as the selection nears the end; only the visible rows are drawn.
//! - Blame: the active file (its unsaved buffer) or the file selected in Changes, one block per
//!   run of lines from the same commit with author and relative age, coloured by age; lines not
//!   committed yet are marked. A blame can take seconds (M7-1 measured 3.1 s for a file touched
//!   by 5 000 commits): the tab shows "blaming…" and every key keeps working.
//! - Commit view (Enter on a commit or a blame line, or `/git show`): what `git show` shows —
//!   id, author, date, message, the changed files and the diff (first parent), syntax-coloured.
//!
//! Log and show run on one thread of the popup session with a git runner of its own (queued
//! requests of one kind are coalesced: only the newest runs); each blame runs on a thread and
//! runner of its own, killed when a newer blame starts or the session ends. So a slow blame
//! never delays History, the commit view, the Changes tab's reads or the user's writes, and
//! never makes the next blame wait. A revision the user typed is resolved with
//! `rev-parse --verify` before anything else reads it (SEC-5, in vr-git). Every git-derived
//! string passes [`sanitize`] before it is drawn (SEC-15).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ropey::Rope;
use vr_git::{
    Blame, CommitInfo, CommitSummary, FileDiff, FileStatus, GitError, Head, LineKind, Repo,
};
use vr_syntax::{HighlightOptions, Highlighter, LanguageRegistry, Notify};

use crate::app::App;
use crate::git_popup::{GitPopup, Tab};
use crate::ui::safe_text::sanitize;

/// Commits asked for per page of the history list.
pub const PAGE: usize = 200;
/// The next page is asked for when the selection is this close to the last loaded commit.
const LOOKAHEAD: usize = 20;
/// Files of a commit larger than this (diff text) are shown without syntax colours.
const HL_MAX_BYTES: usize = 512 * 1024;
/// Syntax highlighters a commit view keeps for files off screen (one per file, made when the
/// file is on screen; files on screen always keep theirs).
const HL_LIVE: usize = 8;
/// Largest worktree file read for a blame (vr-git's per-file cap).
const MAX_FILE: u64 = 8 * 1024 * 1024;
/// Columns one left / right scrolls.
const STEP: usize = 8;

// ---- state ----------------------------------------------------------------------------------

/// The History tab.
#[derive(Debug, Default)]
pub struct History {
    /// The file whose history is shown (absolute), `None` for the repo's.
    pub path: Option<PathBuf>,
    pub commits: Vec<CommitSummary>,
    /// Commits asked for by the last request.
    pub limit: usize,
    pub loading: bool,
    /// Fewer commits came back than were asked for: the list is whole.
    pub complete: bool,
    pub error: Option<String>,
    pub selected: usize,
    pub top: usize,
    generation: u64,
    answered: bool,
    /// HEAD when the list was asked for (`Some(None)`: unborn); `None` until a scan told it.
    /// A newer HEAD loads the list again.
    head: Option<Option<String>>,
    /// `git log` requests sent (the tests count them).
    pub(crate) asked: u64,
}

impl History {
    /// An answer to the latest request arrived (the list may still be empty).
    pub fn loaded(&self) -> bool {
        self.answered
    }
}

/// A finished blame and the text it is of (one sanitized string per line).
#[derive(Debug, Clone)]
pub struct BlameShown {
    pub blame: Blame,
    pub lines: Vec<String>,
    /// Width of the widest line (measured once, on the blame thread).
    pub widest: usize,
}

/// The Blame tab.
#[derive(Debug, Default)]
pub struct BlameView {
    /// The blamed file (absolute).
    pub path: Option<PathBuf>,
    /// `Ok(None)`: the file is not tracked.
    pub result: Option<Result<Option<BlameShown>, String>>,
    pub running: bool,
    pub started: Option<Instant>,
    /// Selected line (0-based).
    pub cursor: usize,
    pub top: usize,
    pub left: usize,
    generation: u64,
    /// Version of the editor buffer blamed; `None`: the file on disk.
    version: Option<u64>,
}

/// One row of the commit view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CvRow {
    /// `commit <id>`, `Author: …`, `Date: …` (sanitized).
    Meta(String),
    /// One line of the commit message (sanitized), drawn indented.
    Message(String),
    /// A note (sanitized): "3 files changed", "binary file", an error.
    Info(String),
    Blank,
    /// File `i` of the commit in the file list.
    FileStat(usize),
    /// The heading of file `i`'s diff.
    FileHead(usize),
    /// Hunk header (file, hunk).
    Hunk(usize, usize),
    /// Diff line (file, hunk, line).
    Line(usize, usize, usize),
}

/// The syntax colours of one file of a commit: the file's diff lines as one text.
struct FileHl {
    file: usize,
    hl: Highlighter,
    /// Byte offset of each line in the text, `[hunk][line]`.
    starts: Vec<Vec<usize>>,
    /// The last frame that drew one of the file's lines.
    seen: u64,
}

/// One commit's details and diff.
pub struct CommitView {
    /// The revision as asked for (sanitized).
    pub rev: String,
    pub oid: Option<String>,
    pub info: Option<CommitInfo>,
    pub diffs: Vec<FileDiff>,
    pub error: Option<String>,
    pub loading: bool,
    pub rows: Vec<CvRow>,
    pub top: usize,
    pub left: usize,
    /// The tab Esc goes back to.
    pub back: Tab,
    generation: u64,
    hl: Vec<FileHl>,
    /// Frames drawn; a highlighter used in the current one is on screen.
    frame: u64,
    /// Drawn width of each row (`u32::MAX`: not measured yet).
    widths: Vec<u32>,
    /// Files that get no colours (no grammar, too large).
    plain: Vec<usize>,
    /// What drawing this view has cost so far (the tests bound it per frame).
    pub(crate) work: Work,
}

/// Counters of the commit view's drawing work.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Work {
    /// Diff cells given a style.
    pub cells: u64,
    /// Bytes of diff text whose syntax spans were asked for.
    pub queried: u64,
    /// Rows whose width was measured.
    pub measured: u64,
    /// Syntax highlighters made.
    pub hl_made: u64,
}

impl CommitView {
    fn loading(rev: &str, back: Tab, generation: u64) -> CommitView {
        CommitView {
            rev: sanitize(rev),
            oid: None,
            info: None,
            diffs: Vec::new(),
            error: None,
            loading: true,
            rows: Vec::new(),
            top: 0,
            left: 0,
            back,
            generation,
            hl: Vec::new(),
            frame: 0,
            widths: Vec::new(),
            plain: Vec::new(),
            work: Work::default(),
        }
    }

    /// The text of `row` as drawn (without the message indent), sanitized.
    pub fn row_text(&self, row: &CvRow) -> String {
        match row {
            CvRow::Meta(s) | CvRow::Message(s) | CvRow::Info(s) => s.clone(),
            CvRow::Blank => String::new(),
            CvRow::FileStat(i) => {
                let d = &self.diffs[*i];
                let path = |p: &Option<PathBuf>| {
                    p.as_deref()
                        .map(|p| sanitize(&p.to_string_lossy()))
                        .unwrap_or_default()
                };
                match d.status {
                    FileStatus::Renamed | FileStatus::Copied => format!(
                        "{}{:03} {} {}",
                        crate::ui::git_popup::status_letter(d.status),
                        d.similarity.unwrap_or(0),
                        path(&d.old_path),
                        path(&d.new_path)
                    ),
                    _ => format!(
                        "{} {}",
                        crate::ui::git_popup::status_letter(d.status),
                        file_name(d)
                    ),
                }
            }
            CvRow::FileHead(i) => file_name(&self.diffs[*i]),
            CvRow::Hunk(f, h) => sanitize(&self.diffs[*f].hunks[*h].header),
            CvRow::Line(f, h, l) => {
                let line = &self.diffs[*f].hunks[*h].lines[*l];
                let mark = match line.kind {
                    LineKind::Added => '+',
                    LineKind::Removed => '-',
                    LineKind::Context => ' ',
                };
                format!("{mark}{}", sanitize(&line.text_lossy()))
            }
        }
    }

    /// Starts drawing a frame: highlighters used from now on are on screen.
    pub(crate) fn begin_frame(&mut self) {
        self.frame += 1;
    }

    /// Drawn width of row `i` (its text plus room for the mark and the scroll markers),
    /// measured once per row.
    pub(crate) fn row_width(&mut self, i: usize) -> usize {
        if self.widths.len() != self.rows.len() {
            self.widths = vec![u32::MAX; self.rows.len()];
        }
        if self.widths[i] == u32::MAX {
            self.work.measured += 1;
            let w = crate::ui::git_popup::width(&self.row_text(&self.rows[i])) + 4;
            self.widths[i] = u32::try_from(w).unwrap_or(u32::MAX - 1);
        }
        self.widths[i] as usize
    }

    /// Spans of the bytes `range` of diff line (file, hunk, line), relative to the line's
    /// start, sorted and disjoint: `None` when the file gets no colours. Makes the file's
    /// highlighter the first time it is asked for; a file on screen keeps its highlighter (the
    /// cache only drops files not drawn in this frame, so many files on screen never rebuild).
    pub(crate) fn spans(
        &mut self,
        (f, h, l): (usize, usize, usize),
        range: std::ops::Range<usize>,
        registry: &Arc<LanguageRegistry>,
        notify: &Notify,
    ) -> Option<Vec<(std::ops::Range<usize>, vr_syntax::Capture)>> {
        if self.plain.contains(&f) {
            return None;
        }
        let at = match self.hl.iter().position(|x| x.file == f) {
            Some(i) => i,
            None => {
                let Some(x) = file_highlighter(&self.diffs[f], f, registry, notify) else {
                    self.plain.push(f);
                    return None;
                };
                self.work.hl_made += 1;
                while self.hl.len() >= HL_LIVE {
                    let off_screen = self
                        .hl
                        .iter()
                        .enumerate()
                        .filter(|(_, x)| x.seen < self.frame)
                        .min_by_key(|(_, x)| x.seen)
                        .map(|(i, _)| i);
                    match off_screen {
                        Some(i) => drop(self.hl.swap_remove(i)),
                        None => break,
                    }
                }
                self.hl.push(x);
                self.hl.len() - 1
            }
        };
        let x = &mut self.hl[at];
        x.seen = self.frame;
        let start = x.starts[h][l];
        self.work.queried += range.len() as u64;
        let q = x.hl.query(start + range.start..start + range.end);
        Some(
            q.spans
                .into_iter()
                .map(|(r, c)| (r.start.saturating_sub(start)..r.end - start, c))
                .collect(),
        )
    }
}

fn file_name(d: &FileDiff) -> String {
    d.path()
        .map(|p| sanitize(&p.to_string_lossy()))
        .unwrap_or_default()
}

fn file_highlighter(
    d: &FileDiff,
    file: usize,
    registry: &Arc<LanguageRegistry>,
    notify: &Notify,
) -> Option<FileHl> {
    let path = d.path()?;
    let lang = registry
        .language_for_path(path)
        .filter(|id| registry.has_grammar(id))?;
    let mut text = String::new();
    let mut starts = Vec::new();
    for hunk in &d.hunks {
        let mut s = Vec::new();
        for line in &hunk.lines {
            s.push(text.len());
            text.push_str(&line.text_lossy());
            text.push('\n');
        }
        starts.push(s);
        if text.len() > HL_MAX_BYTES {
            return None;
        }
    }
    let hl = Highlighter::new(
        registry.clone(),
        lang,
        Rope::from_str(&text),
        HighlightOptions::default(),
        Some(notify.clone()),
    );
    Some(FileHl {
        file,
        hl,
        starts,
        seen: 0,
    })
}

// ---- the session's history thread and blame runs --------------------------------------------

pub(crate) enum HistReq {
    Show {
        generation: u64,
        rev: String,
    },
    Log {
        generation: u64,
        limit: usize,
        path: Option<PathBuf>,
    },
}

impl HistReq {
    fn kind(&self) -> usize {
        match self {
            HistReq::Show { .. } => 0,
            HistReq::Log { .. } => 1,
        }
    }
}

pub(crate) enum HistMsg {
    Show {
        generation: u64,
        result: Result<(CommitInfo, Result<Vec<FileDiff>, GitError>), GitError>,
    },
    Log {
        generation: u64,
        result: Result<Vec<CommitSummary>, GitError>,
    },
    Blame {
        generation: u64,
        result: Result<Option<BlameShown>, GitError>,
    },
}

/// The history thread (log and show; a blame runs on a thread and runner of its own, see
/// [`start_blame`]): the newest request of each kind first-come, older ones of the same kind
/// dropped unanswered (their generation is stale anyway).
pub(crate) fn worker(repo: Repo, rx: Receiver<HistReq>, tx: Sender<HistMsg>) {
    while let Ok(first) = rx.recv() {
        let mut latest: [Option<HistReq>; 2] = [None, None];
        for r in std::iter::once(first).chain(rx.try_iter()) {
            let k = r.kind();
            latest[k] = Some(r);
        }
        for r in latest.into_iter().flatten() {
            if tx.send(run(&repo, r)).is_err() {
                return;
            }
        }
    }
}

fn run(repo: &Repo, req: HistReq) -> HistMsg {
    match req {
        HistReq::Show { generation, rev } => {
            let result = repo.resolve_commit(&rev).and_then(|oid| {
                let info = repo.commit(&oid)?;
                Ok((info, repo.diff_commit(&oid, None)))
            });
            HistMsg::Show { generation, result }
        }
        HistReq::Log {
            generation,
            limit,
            path,
        } => HistMsg::Log {
            generation,
            result: repo.log(limit, path.as_deref()),
        },
    }
}

/// Runs one blame on a thread with a git runner of its own (making one runs no git), so a
/// blame of seconds never holds History or the commit view, and a newer blame (or the end of
/// the session) kills it at once through [`Repo::shutdown`]. Returns that runner.
pub(crate) fn start_blame(
    base: &Repo,
    tx: Sender<HistMsg>,
    generation: u64,
    path: PathBuf,
    text: Option<Arc<str>>,
) -> Result<(Repo, std::thread::JoinHandle<()>), GitError> {
    let runner = vr_git::Writer::open(base)?.repo().clone();
    let r = runner.clone();
    let handle = std::thread::Builder::new()
        .name("vr-git-popup-blame".into())
        .spawn(move || {
            let result = blame_of(&r, &path, text.as_deref());
            let _ = tx.send(HistMsg::Blame { generation, result });
        })?;
    Ok((runner, handle))
}

fn blame_of(repo: &Repo, path: &Path, text: Option<&str>) -> Result<Option<BlameShown>, GitError> {
    let shown = |b: Option<Blame>, content: &str| {
        b.map(|blame| {
            let lines = display_lines(content);
            let widest = lines
                .iter()
                .map(|l| crate::ui::git_popup::width(l))
                .max()
                .unwrap_or(0);
            BlameShown {
                blame,
                lines,
                widest,
            }
        })
    };
    if let Some(t) = text {
        return Ok(shown(repo.blame_text(path, t)?, t));
    }
    // The text blamed is the text shown: read once, blamed as read (bounded, regular files).
    match read_bounded(path)? {
        Some(bytes) => match String::from_utf8(bytes) {
            Ok(s) => Ok(shown(repo.blame_text(path, &s)?, &s)),
            // Not UTF-8: git blames the file itself; the lines are shown lossily.
            Err(e) => {
                let lossy = String::from_utf8_lossy(e.as_bytes()).into_owned();
                Ok(shown(repo.blame(path)?, &lossy))
            }
        },
        None => Ok(shown(repo.blame(path)?, "")),
    }
}

/// The file's bytes when it is a regular file no larger than [`MAX_FILE`]; `None` for anything
/// else (missing, a symlink, a FIFO that would block the read).
fn read_bounded(path: &Path) -> Result<Option<Vec<u8>>, GitError> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_file() => {
            if m.len() > MAX_FILE {
                return Err(GitError::TooLarge {
                    cmd: "blame".into(),
                    limit: MAX_FILE as usize,
                });
            }
        }
        _ => return Ok(None),
    }
    let Ok(file) = std::fs::File::open(path) else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE {
        return Err(GitError::TooLarge {
            cmd: "blame".into(),
            limit: MAX_FILE as usize,
        });
    }
    Ok(Some(bytes))
}

/// One sanitized string per line, as git counts lines (a CRLF line shows without its CR).
fn display_lines(text: &str) -> Vec<String> {
    text.split_inclusive('\n')
        .map(|l| {
            let l = l.strip_suffix('\n').unwrap_or(l);
            sanitize(l.strip_suffix('\r').unwrap_or(l))
        })
        .collect()
}

// ---- requests ---------------------------------------------------------------------------------

fn send(p: &mut GitPopup, req: HistReq) -> bool {
    p.hist_tx.as_ref().is_some_and(|tx| tx.send(req).is_ok())
}

fn head_oid(p: &GitPopup) -> Option<String> {
    match p.scan.as_ref()?.head.as_ref()? {
        Head::Branch { oid, .. } => oid.clone(),
        Head::Detached { oid, .. } => Some(oid.clone()),
    }
}

/// Loads the history of `path` (`None`: the repo) from the first page.
fn load_log(p: &mut GitPopup, path: Option<PathBuf>) {
    let h = &mut p.history;
    if h.path != path {
        h.selected = 0;
        h.top = 0;
        h.commits.clear();
        h.answered = false;
    }
    h.path = path;
    h.limit = PAGE.max(h.commits.len().div_ceil(PAGE) * PAGE);
    ask_log(p);
}

fn ask_log(p: &mut GitPopup) {
    p.history.asked += 1;
    p.history.generation += 1;
    p.history.loading = true;
    p.history.error = None;
    p.history.head = p.scan.is_some().then(|| head_oid(p));
    let req = HistReq::Log {
        generation: p.history.generation,
        limit: p.history.limit,
        path: p.history.path.clone(),
    };
    if !send(p, req) {
        p.history.loading = false;
        p.history.answered = true;
        p.history.error = Some("git: the history reader is not running".into());
    }
}

/// The next page, when the selection is near the end of an incomplete list.
fn maybe_more(p: &mut GitPopup) {
    let h = &p.history;
    // After a failed request only `r` asks again (a key must not rerun a failing log).
    let near_end = h.selected + LOOKAHEAD >= h.commits.len();
    if !h.loading && !h.complete && h.answered && h.error.is_none() && near_end {
        p.history.limit = h.commits.len() + PAGE;
        ask_log(p);
    }
}

/// Blames `path` (absolute): its editor buffer when a tab holds it, else the file on disk.
/// `force` blames again even when the same text was blamed.
fn load_blame(app: &mut App, path: PathBuf, force: bool) {
    let now = app.now();
    let doc = app.docs.iter().find(|d| d.path.as_deref() == Some(&*path));
    let version = doc.map(|d| d.buffer.version());
    let text: Option<Arc<str>> = doc.map(|d| Arc::from(d.buffer.text().to_string()));
    let Some(p) = &mut app.git_popup else {
        return;
    };
    let b = &mut p.blame;
    let same = b.path.as_deref() == Some(&*path) && b.version == version;
    if same && !force && (b.running || matches!(b.result, Some(Ok(_)))) {
        return;
    }
    if !same {
        b.cursor = 0;
        b.top = 0;
        b.left = 0;
        b.result = None;
    }
    b.path = Some(path.clone());
    b.version = version;
    b.generation += 1;
    b.running = true;
    b.started = Some(now);
    let generation = b.generation;
    // The blame running now is of an older request: kill it rather than wait for it.
    if let Some(old) = p.blame_run.take() {
        old.shutdown();
    }
    p.blame_threads.retain(|h| !h.is_finished());
    let started = match (&p.hist_repo, &p.hist_back) {
        (Some(base), Some(tx)) => start_blame(base, tx.clone(), generation, path, text)
            .map_err(|e| sanitize(&format!("git: {e}"))),
        _ => Err("git: the history reader is not running".into()),
    };
    match started {
        Ok((runner, handle)) => {
            p.blame_run = Some(runner);
            p.blame_threads.push(handle);
        }
        Err(e) => {
            p.blame.running = false;
            p.blame.result = Some(Err(e));
        }
    }
}

/// Opens the commit view of `rev`; Esc goes back to `back`.
fn load_commit(p: &mut GitPopup, rev: &str, back: Tab) {
    p.refusal = None;
    p.commit_gen += 1;
    p.commit = Some(CommitView::loading(rev, back, p.commit_gen));
    let req = HistReq::Show {
        generation: p.commit_gen,
        rev: rev.to_string(),
    };
    if !send(p, req)
        && let Some(c) = &mut p.commit
    {
        c.loading = false;
        c.error = Some("git: the history reader is not running".into());
    }
}

// ---- commands ---------------------------------------------------------------------------------

/// `/git log` (the repo) and `/git log file` (the active file, following renames).
pub fn log(app: &mut App, file: bool) -> Result<(), String> {
    let path = if file {
        Some(
            app.active_doc()
                .and_then(|d| d.path.clone())
                .ok_or("git log file: no file is open")?,
        )
    } else {
        None
    };
    crate::git_popup::open(app, "")?;
    let Some(p) = &mut app.git_popup else {
        return Ok(());
    };
    p.tab = Tab::History;
    p.commit = None;
    load_log(p, path);
    Ok(())
}

/// `/git blame`: the active file.
pub fn blame(app: &mut App) -> Result<(), String> {
    let path = app
        .active_doc()
        .and_then(|d| d.path.clone())
        .ok_or("git blame: no file is open")?;
    crate::git_popup::open(app, "")?;
    if let Some(p) = &mut app.git_popup {
        p.tab = Tab::Blame;
        p.commit = None;
    }
    load_blame(app, path, true);
    Ok(())
}

/// `/git show <rev>`: the commit view; the revision is resolved by `rev-parse --verify` on the
/// history thread (SEC-5), a refusal is a notice.
pub fn show(app: &mut App, rev: &str) -> Result<(), String> {
    let rev = rev.trim();
    if rev.is_empty() {
        return Err("git show: name a commit".into());
    }
    crate::git_popup::open(app, "")?;
    let Some(p) = &mut app.git_popup else {
        return Ok(());
    };
    if p.tab == Tab::Changes {
        p.tab = Tab::History;
    }
    if p.tab == Tab::History && !p.history.answered && !p.history.loading {
        load_log(p, None);
    }
    let back = p.tab;
    load_commit(p, rev, back);
    Ok(())
}

/// Keys `1` / `2` / `3` and the tab bar: Changes, History (the last one shown, else the repo's),
/// Blame (the file selected in Changes when coming from there, else the active file).
pub fn switch(app: &mut App, tab: Tab) {
    let Some(p) = &mut app.git_popup else {
        return;
    };
    let from = p.tab;
    p.commit = None;
    p.tab = tab;
    match tab {
        Tab::Changes => {}
        Tab::History => {
            if !p.history.answered && !p.history.loading {
                load_log(p, None);
            }
        }
        Tab::Blame => {
            let selected = (from == Tab::Changes)
                .then(|| p.selected_entry().map(|e| p.workdir.join(&e.path)))
                .flatten();
            let (workdir, last) = (p.workdir.clone(), p.blame.path.clone());
            let active = app
                .active_doc()
                .and_then(|d| d.path.clone())
                .filter(|d| d.starts_with(&workdir));
            match selected.or(active).or(last) {
                Some(path) => load_blame(app, path, false),
                None => {
                    if let Some(p) = &mut app.git_popup {
                        p.blame.result = Some(Err(
                            "no file to blame: open one, or select one in the Changes tab".into(),
                        ));
                    }
                }
            }
        }
    }
}

// ---- answers ---------------------------------------------------------------------------------

/// Takes the history thread's answers (from `git_popup::tick`); true when something changed.
pub fn tick(app: &mut App) -> bool {
    let Some(p) = &mut app.git_popup else {
        return false;
    };
    let msgs: Vec<HistMsg> = match &p.hist_rx {
        Some(rx) => rx.try_iter().collect(),
        None => Vec::new(),
    };
    let mut changed = !msgs.is_empty();
    for m in msgs {
        answer(app, m);
    }
    // HEAD moved (a commit, a pull, the terminal): the shown history loads again. A list asked
    // for before the session's first scan takes that scan's HEAD as its own (no second load).
    if let Some(p) = &mut app.git_popup
        && p.scan.is_some()
    {
        let now = head_oid(p);
        match &p.history.head {
            None if p.history.asked > 0 => p.history.head = Some(now),
            Some(was)
                if *was != now
                    && p.tab == Tab::History
                    && p.history.answered
                    && !p.history.loading =>
            {
                ask_log(p);
                changed = true;
            }
            _ => {}
        }
    }
    if let Some(p) = &app.git_popup
        && p.blame.running
        && p.tab == Tab::Blame
        && p.visible
    {
        // The elapsed time in the status line.
        changed = true;
    }
    changed
}

fn answer(app: &mut App, m: HistMsg) {
    let now = now_secs(app.git.now_secs);
    let Some(p) = &mut app.git_popup else {
        return;
    };
    match m {
        HistMsg::Log { generation, result } => {
            let h = &mut p.history;
            if generation != h.generation {
                return;
            }
            h.loading = false;
            h.answered = true;
            match result {
                Ok(list) => {
                    let keep = h.commits.get(h.selected).map(|c| c.oid.clone());
                    h.complete = list.len() < h.limit;
                    h.commits = list;
                    if let Some(oid) = keep
                        && let Some(i) = h.commits.iter().position(|c| c.oid == oid)
                    {
                        h.selected = i;
                    }
                    h.selected = h.selected.min(h.commits.len().saturating_sub(1));
                }
                Err(e) => h.error = Some(sanitize(&e.to_string())),
            }
        }
        HistMsg::Blame { generation, result } => {
            let b = &mut p.blame;
            if generation != b.generation {
                return;
            }
            b.running = false;
            p.blame_run = None;
            b.result = Some(result.map_err(|e| sanitize(&e.to_string())));
            let n = match &b.result {
                Some(Ok(Some(s))) => s.blame.lines.len(),
                _ => 0,
            };
            b.cursor = b.cursor.min(n.saturating_sub(1));
        }
        HistMsg::Show { generation, result } => {
            let Some(c) = &mut p.commit else {
                return;
            };
            if c.generation != generation {
                return;
            }
            match result {
                Ok((info, diffs)) => {
                    c.loading = false;
                    c.oid = Some(info.oid.clone());
                    match diffs {
                        Ok(d) => c.diffs = d,
                        Err(e) => c.error = Some(sanitize(&e.to_string())),
                    }
                    c.rows = commit_rows(&info, &c.diffs, c.error.as_deref(), now);
                    c.info = Some(info);
                }
                Err(e) => {
                    let msg = refusal(&c.rev, &e);
                    let back = c.back;
                    p.commit = None;
                    p.tab = back;
                    p.refusal = Some(msg.clone());
                    app.error(msg);
                }
            }
        }
    }
}

/// The notice for a revision that did not resolve to one commit.
fn refusal(rev: &str, e: &GitError) -> String {
    match e {
        GitError::NotACommit(_) => {
            let hex = (4..40).contains(&rev.len()) && rev.bytes().all(|b| b.is_ascii_hexdigit());
            if hex {
                format!(
                    "git show: {rev}: no such commit, or the short id matches more than one commit (type more of it)"
                )
            } else {
                format!("git show: {rev}: no such commit")
            }
        }
        e => format!("git show: {rev}: {}", sanitize(&e.to_string())),
    }
}

pub(crate) fn now_secs(fixed: Option<i64>) -> i64 {
    fixed.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64)
    })
}

/// `YYYY-MM-DD HH:MM` (UTC) of Unix seconds.
pub fn utc_date_time(secs: i64) -> String {
    let s = crate::recovery::utc_stamp(secs.max(0) as u64);
    format!(
        "{}-{}-{} {}:{}",
        &s[0..4],
        &s[4..6],
        &s[6..8],
        &s[9..11],
        &s[11..13]
    )
}

/// Short age for narrow columns: "now", "5m", "3h", "4d", "2mo", "3y".
pub fn short_age(secs: i64) -> String {
    const MIN: i64 = 60;
    const HOUR: i64 = 60 * MIN;
    const DAY: i64 = 24 * HOUR;
    match secs {
        s if s < MIN => "now".into(),
        s if s < HOUR => format!("{}m", s / MIN),
        s if s < DAY => format!("{}h", s / HOUR),
        s if s < 30 * DAY => format!("{}d", s / DAY),
        s if s < 365 * DAY => format!("{}mo", s / (30 * DAY)),
        s => format!("{}y", s / (365 * DAY)),
    }
}

fn commit_rows(info: &CommitInfo, diffs: &[FileDiff], error: Option<&str>, now: i64) -> Vec<CvRow> {
    let mut rows = vec![CvRow::Meta(format!("commit {}", sanitize(&info.oid)))];
    if info.parents.len() > 1 {
        let short: Vec<String> = info
            .parents
            .iter()
            .map(|p| sanitize(&p[..p.len().min(7)]))
            .collect();
        rows.push(CvRow::Meta(format!(
            "Merge: {} (diff against the first parent)",
            short.join(" ")
        )));
    }
    let sig = |s: &vr_git::Signature| format!("{} <{}>", sanitize(&s.name), sanitize(&s.email));
    rows.push(CvRow::Meta(format!("Author: {}", sig(&info.author))));
    if info.committer.name != info.author.name || info.committer.email != info.author.email {
        rows.push(CvRow::Meta(format!("Commit: {}", sig(&info.committer))));
    }
    rows.push(CvRow::Meta(format!(
        "Date:   {} UTC ({})",
        utc_date_time(info.author.time),
        crate::git::relative_age(now - info.author.time)
    )));
    rows.push(CvRow::Blank);
    for l in info.message.split('\n') {
        rows.push(CvRow::Message(sanitize(l)));
    }
    rows.push(CvRow::Blank);
    if let Some(e) = error {
        rows.push(CvRow::Info(format!("diff: {e}")));
        return rows;
    }
    let n = diffs.len();
    rows.push(CvRow::Info(format!(
        "{n} file{} changed",
        if n == 1 { "" } else { "s" }
    )));
    rows.extend((0..n).map(CvRow::FileStat));
    for (f, d) in diffs.iter().enumerate() {
        rows.push(CvRow::Blank);
        rows.push(CvRow::FileHead(f));
        if d.binary {
            rows.push(CvRow::Info("binary file".into()));
        } else if d.hunks.is_empty() {
            rows.push(CvRow::Info(
                "no line changes (mode, rename or empty file)".into(),
            ));
        }
        for (h, hunk) in d.hunks.iter().enumerate() {
            rows.push(CvRow::Hunk(f, h));
            rows.extend((0..hunk.lines.len()).map(|l| CvRow::Line(f, h, l)));
        }
    }
    rows
}

// ---- keys and mouse ---------------------------------------------------------------------------

/// Esc in the commit view: back to the list it came from. False when no commit view shows.
pub fn back(p: &mut GitPopup) -> bool {
    match p.commit.take() {
        Some(c) => {
            p.tab = c.back;
            true
        }
        None => false,
    }
}

fn step(cur: usize, n: usize, code: KeyCode, page: usize) -> usize {
    let last = n.saturating_sub(1);
    match code {
        KeyCode::Up => cur.saturating_sub(1),
        KeyCode::Down => (cur + 1).min(last),
        KeyCode::PageUp => cur.saturating_sub(page),
        KeyCode::PageDown => (cur + page).min(last),
        KeyCode::Home => 0,
        KeyCode::End => last,
        _ => cur,
    }
    .min(last)
}

fn page(p: &GitPopup) -> usize {
    usize::from(p.areas.history.height.saturating_sub(1)).max(1)
}

/// Keys of the History and Blame tabs and of the commit view (`1`/`2`/`3`, `esc` and `tab`
/// are taken by `git_popup::handle_key` first).
pub fn handle_key(app: &mut App, key: &KeyEvent) {
    let Some(p) = &mut app.git_popup else {
        return;
    };
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return;
    }
    let pg = page(p);
    if key.code == KeyCode::Backspace && back(p) {
        return;
    }
    if let Some(c) = &mut p.commit {
        match key.code {
            KeyCode::Left => c.left = c.left.saturating_sub(STEP),
            KeyCode::Right => c.left += STEP,
            KeyCode::Up
            | KeyCode::Down
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Home
            | KeyCode::End => {
                let n = c.rows.len().saturating_sub(pg).max(1);
                c.top = step(c.top, n, key.code, pg);
            }
            _ => {}
        }
        return;
    }
    match (p.tab, key.code) {
        (Tab::History, KeyCode::Enter) => {
            if let Some(c) = p.history.commits.get(p.history.selected) {
                let oid = c.oid.clone();
                load_commit(p, &oid, Tab::History);
            }
        }
        (Tab::History, KeyCode::Char('r')) => ask_log(p),
        (
            Tab::History,
            KeyCode::Up
            | KeyCode::Down
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Home
            | KeyCode::End,
        ) => {
            let h = &mut p.history;
            h.selected = step(h.selected, h.commits.len(), key.code, pg);
            maybe_more(p);
        }
        (Tab::Blame, KeyCode::Enter) => open_blamed(app),
        (Tab::Blame, KeyCode::Char('r')) => {
            if let Some(path) = p.blame.path.clone() {
                load_blame(app, path, true);
            }
        }
        (Tab::Blame, KeyCode::Left) => p.blame.left = p.blame.left.saturating_sub(STEP),
        (Tab::Blame, KeyCode::Right) => p.blame.left += STEP,
        (
            Tab::Blame,
            KeyCode::Up
            | KeyCode::Down
            | KeyCode::PageUp
            | KeyCode::PageDown
            | KeyCode::Home
            | KeyCode::End,
        ) => {
            let n = blame_len(p);
            p.blame.cursor = step(p.blame.cursor, n, key.code, pg);
        }
        _ => {}
    }
}

fn blame_len(p: &GitPopup) -> usize {
    match &p.blame.result {
        Some(Ok(Some(s))) => s.blame.lines.len(),
        _ => 0,
    }
}

/// Enter on a blame line: that line's commit.
fn open_blamed(app: &mut App) {
    let Some(p) = &mut app.git_popup else {
        return;
    };
    let Some(Ok(Some(s))) = &p.blame.result else {
        return;
    };
    let Some(c) = s.blame.commit_of(p.blame.cursor) else {
        return;
    };
    if c.uncommitted {
        let n = p.blame.cursor + 1;
        app.info(format!("git: line {n} is not committed yet"));
        return;
    }
    let oid = c.oid.clone();
    load_commit(p, &oid, Tab::Blame);
}

/// Mouse on the History / Blame body and the commit view: the wheel moves, a click selects.
pub fn handle_mouse(app: &mut App, ev: &MouseEvent) {
    let Some(p) = &mut app.git_popup else {
        return;
    };
    let r = p.areas.history;
    let inside =
        ev.column >= r.x && ev.column < r.x + r.width && ev.row >= r.y && ev.row < r.y + r.height;
    if !inside {
        return;
    }
    let code = match ev.kind {
        MouseEventKind::ScrollUp => Some(KeyCode::Up),
        MouseEventKind::ScrollDown => Some(KeyCode::Down),
        _ => None,
    };
    if let Some(code) = code {
        let k = KeyEvent::new(code, KeyModifiers::NONE);
        let times = if p.commit.is_some() { 3 } else { 1 };
        for _ in 0..times {
            handle_key(app, &k);
        }
        return;
    }
    if !matches!(ev.kind, MouseEventKind::Down(MouseButton::Left)) || p.commit.is_some() {
        return;
    }
    let row = usize::from(ev.row - r.y);
    match p.tab {
        Tab::History => {
            let i = p.history.top + row;
            if i < p.history.commits.len() {
                p.history.selected = i;
                maybe_more(p);
            }
        }
        Tab::Blame => {
            let i = p.blame.top + row;
            if i < blame_len(p) {
                p.blame.cursor = i;
            }
        }
        Tab::Changes => {}
    }
}

/// The status line of the History / Blame tabs and the commit view.
pub fn status_line(p: &GitPopup, now: i64, at: Instant) -> String {
    if let Some(r) = p.refusal.as_ref().filter(|_| p.commit.is_none()) {
        return r.clone();
    }
    if let Some(c) = &p.commit {
        if c.loading {
            return format!("loading {}…", c.rev);
        }
        return match &c.info {
            Some(i) => format!(
                "{} · {} · {} · ←→ scroll",
                sanitize(&i.oid[..i.oid.len().min(7)]),
                sanitize(&i.author.name),
                crate::git::relative_age(now - i.author.time)
            ),
            None => String::new(),
        };
    }
    match p.tab {
        Tab::History => {
            let h = &p.history;
            if h.loading {
                return if h.commits.is_empty() {
                    "loading commits…".into()
                } else {
                    format!("{} commits · loading more…", h.commits.len())
                };
            }
            if let Some(e) = &h.error {
                return format!("git: {e} · r try again");
            }
            match h.commits.get(h.selected) {
                Some(c) => format!(
                    "{} · {} · {} · {}",
                    sanitize(&c.short),
                    sanitize(&c.author.name),
                    utc_date_time(c.author.time),
                    crate::git::relative_age(now - c.author.time)
                ),
                None => String::new(),
            }
        }
        Tab::Blame => {
            let b = &p.blame;
            if b.running {
                let secs = b
                    .started
                    .map_or(0.0, |s| at.saturating_duration_since(s).as_secs_f32());
                return format!("blaming… {secs:.1} s");
            }
            let Some(Ok(Some(s))) = &b.result else {
                return String::new();
            };
            match s.blame.commit_of(b.cursor) {
                Some(c) if c.uncommitted => {
                    format!("line {}: You, uncommitted", b.cursor + 1)
                }
                Some(c) => format!(
                    "line {}: {} · {} · {} · {}",
                    b.cursor + 1,
                    sanitize(&c.oid[..c.oid.len().min(7)]),
                    sanitize(&c.author.name),
                    crate::git::relative_age(now - c.author.time),
                    sanitize(&c.summary)
                ),
                None => String::new(),
            }
        }
        Tab::Changes => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_short_ages() {
        assert_eq!(utc_date_time(1_700_000_000), "2023-11-14 22:13");
        assert_eq!(short_age(5), "now");
        assert_eq!(short_age(3 * 86_400 + 5), "3d");
        assert_eq!(short_age(65 * 86_400), "2mo");
        assert_eq!(short_age(800 * 86_400), "2y");
    }

    #[test]
    fn display_lines_count_like_git() {
        assert_eq!(display_lines("a\r\nb\n"), vec!["a", "b"]);
        assert_eq!(display_lines("a\nb"), vec!["a", "b"]);
        assert!(display_lines("").is_empty());
        assert_eq!(display_lines("\x1b[2J\n"), vec!["^[[2J"]);
    }

    #[test]
    fn refusals_say_why() {
        let e = GitError::NotACommit("x".into());
        assert!(refusal("abcd", &e).contains("more than one"));
        assert!(refusal("nope", &e).ends_with("no such commit"));
    }
}
