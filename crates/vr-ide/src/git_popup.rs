//! The Git popup, Changes tab (M7-3; D297 proposal "Git popup", threat model SEC-15..SEC-20):
//! unstaged and staged lists, the diff of the selected file with hunk and line selection,
//! stage / unstage / discard, the commit message box with Amend and Sign off, push, pull and
//! rescan. `/git`, `/git push`, `/git pull` and `/git <root-name>` open it.
//!
//! One popup session is bound to one repository (SEC-17): the repo of the active file's root
//! (or the named root) when it opened. It never follows the focus to another root.
//!
//! Nothing here runs git on the UI thread. A session has two threads of its own: a reader
//! (status, diffs, discard plans, the push target, the last message) and a writer that runs
//! the user's writes one after another through [`vr_git::Writer`] (its own git runner, D304;
//! the per-repo write queue, D303(2)). Push and pull are [`vr_git::Job`]s with progress and
//! cancel. Answers come back through [`tick`].
//!
//! Every action carries what was shown (SEC-18): a file action the status entry and the diff
//! the user saw (checked against a fresh read before it runs), a hunk or line action the shown
//! diff (vr-git checks it), a discard the plan made when the user pressed `d` (vr-git checks
//! its content hash). What changed in between is refused with "changed, rescan" and the view
//! is scanned again.
//!
//! Every git-derived string passes [`crate::ui::safe_text::sanitize`] before it is drawn
//! (SEC-15); nothing a repo, a remote or the user's message holds is logged (SEC-20).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use vr_git::{
    Cancel, CommitOptions, DiscardPlan, Discarded, FileDiff, FileStatus, GitError, Head, Job,
    JobEvent, LeftoverDiscard, LineKind, PushTarget, Repo, RepoState, Status, StatusEntry, Writer,
};

use crate::app::App;
use crate::commands::dialog::DialogAction;
use crate::ui::dialog::{ConfirmDialog, DialogWidget, PromptDialog};
use crate::ui::safe_text::sanitize;

/// Quiet time after a file or `.git` change before the popup scans again.
pub const RESCAN_DELAY: Duration = Duration::from_millis(200);

/// How long exit waits for a running write before its runner is shut down.
const EXIT_WAIT: Duration = Duration::from_secs(3);

/// Lines of git output kept for the output view.
const OUTPUT_LINES: usize = 200;

/// Columns one left / right moves the diff pane.
const DIFF_STEP: usize = 8;

/// Parts of the popup that take keys, in `tab` order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Unstaged,
    Staged,
    Diff,
    Message,
    Amend,
    SignOff,
    Commit,
    Push,
    Pull,
    Rescan,
}

/// `tab` order. The two file lists are one stop (`Unstaged` stands for both): the arrows move
/// from the last unstaged file on to the staged ones and back, as one column; so `tab` from a
/// list always reaches the diff of the file just selected there.
pub const RING: [Pane; 9] = [
    Pane::Unstaged,
    Pane::Diff,
    Pane::Message,
    Pane::Amend,
    Pane::SignOff,
    Pane::Commit,
    Pane::Push,
    Pane::Pull,
    Pane::Rescan,
];

/// One status listing with what the header and the banners need.
#[derive(Debug, Clone, Default)]
pub struct Scan {
    pub status: Status,
    pub state: RepoState,
    pub head: Option<Head>,
    pub ahead: Option<vr_git::AheadBehind>,
    /// Program-running config keys and hooks (SEC-19), sanitized.
    pub programs: Vec<String>,
    /// HEAD and the staged set this listing shows; `None` when they moved while it was read.
    pub base: Option<CommitBase>,
}

/// What a commit or amend was shown (SEC-18): HEAD and the staged changes. The writer checks
/// both again right before `git commit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitBase {
    head: Option<String>,
    staged: Vec<u8>,
}

fn commit_base(repo: &Repo) -> Result<CommitBase, GitError> {
    let head = match repo.head()? {
        Head::Branch { oid, .. } => oid,
        Head::Detached { oid, .. } => Some(oid),
    };
    Ok(CommitBase {
        head,
        staged: repo.staged_fingerprint()?,
    })
}

/// A commit asked for and not sent yet (it waits for the "already pushed" answer).
#[derive(Debug, Clone)]
pub struct PendingCommit {
    message: String,
    opts: CommitOptions,
    base: CommitBase,
}

/// What a file action saw: the entry in its list and, when it was shown, its diff (SEC-18).
#[derive(Debug, Clone)]
pub struct Seen {
    pub staged: bool,
    pub entry: StatusEntry,
    pub diff: Option<FileDiff>,
}

/// A write, run in order on the session's writer thread.
#[derive(Debug, Clone)]
enum WriteOp {
    StageFile(Seen),
    UnstageFile(Seen),
    Hunk {
        staged: bool,
        shown: Box<FileDiff>,
        hunk: usize,
        lines: Option<Vec<usize>>,
    },
    Discard {
        plan: Box<DiscardPlan>,
        buffer_dirty: bool,
    },
    Commit {
        message: String,
        opts: CommitOptions,
        base: CommitBase,
        cancel: Cancel,
    },
    /// Leftover of an interrupted discard: delete it (`to: None`) or move it to `to`.
    Leftover {
        path: PathBuf,
        to: Option<PathBuf>,
    },
}

impl WriteOp {
    /// Short text for the queue line (sanitized).
    fn label(&self) -> String {
        let p = |p: &Path| sanitize(&p.to_string_lossy());
        match self {
            WriteOp::StageFile(s) => format!("stage {}", p(&s.entry.path)),
            WriteOp::UnstageFile(s) => format!("unstage {}", p(&s.entry.path)),
            WriteOp::Hunk {
                staged,
                shown,
                lines,
                ..
            } => {
                let verb = if *staged { "unstage" } else { "stage" };
                let what = if lines.is_some() { "lines" } else { "hunk" };
                let path = shown.path().map(p).unwrap_or_default();
                format!("{verb} {what} of {path}")
            }
            WriteOp::Discard { plan, .. } => format!("discard {}", p(plan.path())),
            WriteOp::Commit { opts, .. } if opts.amend => "amend the last commit".into(),
            WriteOp::Commit { .. } => "commit".into(),
            WriteOp::Leftover { path, to: None } => format!("delete {}", p(path)),
            WriteOp::Leftover { path, .. } => format!("restore {}", p(path)),
        }
    }
}

/// What a finished write reports.
#[derive(Debug)]
enum WriteDone {
    Ok(String),
    Discarded(Discarded),
    Committed(String),
    Err(GitError),
}

#[derive(Debug, Clone)]
enum ReadReq {
    Scan(u64),
    Diff {
        path: PathBuf,
        staged: bool,
    },
    Leftovers,
    LastMessage,
    PushTarget,
    PrepareDiscard {
        seen: Box<Seen>,
        /// `None`: the whole file; else hunk and optional lines of the shown diff.
        hunk: Option<(usize, Option<Vec<usize>>)>,
    },
}

enum Msg {
    Scanned(u64, Result<Scan, GitError>),
    Diff {
        path: PathBuf,
        staged: bool,
        result: Result<Option<FileDiff>, GitError>,
    },
    Leftovers(Vec<LeftoverDiscard>),
    /// HEAD's id and message.
    LastMessage(Result<Option<(String, String)>, GitError>),
    PushTarget(Result<PushTarget, GitError>),
    DiscardPlan {
        label: String,
        result: Result<DiscardPlan, GitError>,
    },
    WriteDone(WriteDone),
}

/// The shown diff and what it is of.
#[derive(Debug, Clone)]
pub struct ShownDiff {
    pub path: PathBuf,
    pub staged: bool,
    pub result: Result<Option<FileDiff>, String>,
}

impl ShownDiff {
    pub fn diff(&self) -> Option<&FileDiff> {
        self.result.as_ref().ok().and_then(Option::as_ref)
    }
}

/// One row of the diff pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffRow {
    /// A line of text about the file (binary, conflicted, no line change, error).
    Info(usize),
    Header(usize),
    Line(usize, usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Push,
    Pull,
}

/// A running push or pull.
pub struct RunningJob {
    pub kind: JobKind,
    /// "push → origin/main", sanitized (SEC-16: the target is in the progress line).
    pub label: String,
    pub last: String,
    job: Job,
}

/// A multi-line text input for the commit message (no git text in it, still drawn sanitized).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextArea {
    text: String,
    /// Byte index of the cursor.
    cursor: usize,
}

impl TextArea {
    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn set(&mut self, text: &str) {
        self.text = text.to_string();
        self.cursor = self.text.len();
    }

    pub fn insert(&mut self, s: &str) {
        self.text.insert_str(self.cursor, s);
        self.cursor += s.len();
    }

    fn line_start(&self) -> usize {
        self.text[..self.cursor].rfind('\n').map_or(0, |i| i + 1)
    }

    fn line_end(&self) -> usize {
        self.text[self.cursor..]
            .find('\n')
            .map_or(self.text.len(), |i| self.cursor + i)
    }

    /// (line, column in chars) of the cursor.
    pub fn position(&self) -> (usize, usize) {
        let before = &self.text[..self.cursor];
        let line = before.matches('\n').count();
        let col = self.text[self.line_start()..self.cursor].chars().count();
        (line, col)
    }

    fn move_vertical(&mut self, down: bool) {
        let col = self.text[self.line_start()..self.cursor].chars().count();
        let target_start = if down {
            let end = self.line_end();
            if end >= self.text.len() {
                return;
            }
            end + 1
        } else {
            let start = self.line_start();
            if start == 0 {
                return;
            }
            self.text[..start - 1].rfind('\n').map_or(0, |i| i + 1)
        };
        let line_end = self.text[target_start..]
            .find('\n')
            .map_or(self.text.len(), |i| target_start + i);
        let line = &self.text[target_start..line_end];
        let off = line.char_indices().nth(col).map_or(line.len(), |(i, _)| i);
        self.cursor = target_start + off;
    }

    /// Editing keys; true when used.
    pub fn handle_key(&mut self, key: &KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char(c) if !ctrl => {
                let mut b = [0u8; 4];
                self.insert(c.encode_utf8(&mut b));
            }
            KeyCode::Enter => self.insert("\n"),
            KeyCode::Backspace => {
                if let Some(c) = self.text[..self.cursor].chars().next_back() {
                    self.cursor -= c.len_utf8();
                    self.text.remove(self.cursor);
                }
            }
            KeyCode::Delete => {
                if self.cursor < self.text.len() {
                    self.text.remove(self.cursor);
                }
            }
            KeyCode::Left => {
                if let Some(c) = self.text[..self.cursor].chars().next_back() {
                    self.cursor -= c.len_utf8();
                }
            }
            KeyCode::Right => {
                if let Some(c) = self.text[self.cursor..].chars().next() {
                    self.cursor += c.len_utf8();
                }
            }
            KeyCode::Home => self.cursor = self.line_start(),
            KeyCode::End => self.cursor = self.line_end(),
            KeyCode::Up => self.move_vertical(false),
            KeyCode::Down => self.move_vertical(true),
            _ => return false,
        }
        true
    }
}

/// Screen areas of the last frame, for the mouse.
#[derive(Debug, Clone, Default)]
pub struct Areas {
    pub popup: Rect,
    pub unstaged: Rect,
    pub staged: Rect,
    pub diff: Rect,
    pub message: Rect,
    pub tabs: Rect,
    pub buttons: Vec<(Rect, Pane)>,
}

pub struct GitPopup {
    pub visible: bool,
    /// Session id (dialog answers name the session they belong to).
    pub id: u64,
    pub workdir: PathBuf,
    /// The workspace root it was opened for.
    pub root: PathBuf,
    /// `~`-relative work tree, sanitized; tests may set a fixed one.
    pub repo_label: String,
    /// The root's name when the repo was found above the root (D298, SEC-17).
    pub parent_of: Option<String>,
    writer: Writer,
    read_tx: Option<Sender<ReadReq>>,
    write_tx: Option<Sender<WriteOp>>,
    rx: Receiver<Msg>,
    threads: Vec<JoinHandle<()>>,
    /// Labels of the writes sent and not finished, oldest (running) first.
    pub queue: VecDeque<String>,
    /// Per queued write: the absolute path of a discard (popped with `queue`).
    queued_discards: VecDeque<Option<PathBuf>>,
    /// Cancel of a running commit (hooks may wait).
    commit_cancel: Option<Cancel>,
    pub job: Option<RunningJob>,
    scan_gen: u64,
    /// The latest scan asked for has not answered yet.
    pub scanning: bool,
    pub rescan_at: Option<Instant>,
    pub scan: Option<Scan>,
    pub scan_error: Option<String>,
    pub focus: Pane,
    pub selected: [usize; 2],
    pub list_top: [usize; 2],
    /// The list whose file the diff pane shows (false: unstaged).
    pub showing_staged: bool,
    pub diff: Option<ShownDiff>,
    /// Diff requests not answered yet.
    pub diff_pending: usize,
    pub diff_cursor: usize,
    pub diff_anchor: Option<usize>,
    pub diff_top: usize,
    /// Columns the diff pane is scrolled to the right (left/right while it has focus).
    pub diff_left: usize,
    pub message: TextArea,
    pub amend: bool,
    pub signoff: bool,
    /// The last commit's message as loaded for Amend, and the draft it replaced.
    amend_loaded: Option<String>,
    pre_amend: Option<String>,
    /// The commit whose message Amend loaded: an amend of another one is refused (SEC-18).
    amend_oid: Option<String>,
    /// git's output of the last push/pull or failed write, shown in the diff pane.
    pub output: Vec<String>,
    pub show_output: bool,
    /// The latest result line (sanitized).
    pub note: String,
    pub leftovers: VecDeque<LeftoverDiscard>,
    /// Absolute paths whose discard runs: saves of them wait (D305(4)).
    pub discarding: Vec<PathBuf>,
    /// The root went away: the session ends when its writes are done.
    pub orphan: bool,
    /// The writer thread stopped: reported once, then the session ends.
    writer_stopped: bool,
    pub areas: Areas,
    /// Push asked for its target to show the confirmation.
    push_asked: bool,
}

impl GitPopup {
    /// Test hook: the writer thread is gone, as after a failed spawn.
    #[cfg(test)]
    pub(crate) fn kill_writer(&mut self) {
        self.write_tx = Some(mpsc::channel().0);
    }

    pub fn writer(&self) -> &Writer {
        &self.writer
    }

    /// Writes sent or running (index writes, commit, push/pull).
    pub fn busy(&self) -> bool {
        !self.queue.is_empty() || self.job.is_some()
    }

    fn read(&self, req: ReadReq) {
        if let Some(tx) = &self.read_tx {
            let _ = tx.send(req);
        }
    }

    fn write(&mut self, op: WriteOp) {
        self.queue.push_back(op.label());
        // Saves of a discarded path wait from now until its discard is done (D305(4)).
        let abs = match &op {
            WriteOp::Discard { plan, .. } => Some(self.workdir.join(plan.path())),
            _ => None,
        };
        if let Some(a) = &abs {
            self.discarding.push(a.clone());
        }
        self.queued_discards.push_back(abs);
        if !self.write_tx.as_ref().is_some_and(|tx| tx.send(op).is_ok()) {
            // The writer thread is gone (it never started or died): nothing will answer.
            self.queue.pop_back();
            if let Some(Some(a)) = self.queued_discards.pop_back()
                && let Some(i) = self.discarding.iter().rposition(|x| *x == a)
            {
                self.discarding.remove(i);
            }
            self.orphan = true;
            self.visible = false;
            self.writer_stopped = true;
        }
    }

    pub fn rescan(&mut self) {
        self.scan_gen += 1;
        self.scanning = true;
        self.rescan_at = None;
        self.read(ReadReq::Scan(self.scan_gen));
    }

    /// The entries of the unstaged (0) or staged (1) list.
    pub fn list(&self, which: usize) -> &[StatusEntry] {
        match &self.scan {
            Some(s) if which == 0 => &s.status.unstaged,
            Some(s) => &s.status.staged,
            None => &[],
        }
    }

    /// The list a list pane shows (0 unstaged, 1 staged); for other panes, the one the diff is of.
    pub fn list_index(&self) -> usize {
        match self.focus {
            Pane::Unstaged => 0,
            Pane::Staged => 1,
            _ => usize::from(self.showing_staged),
        }
    }

    pub fn selected_entry(&self) -> Option<&StatusEntry> {
        let w = self.list_index();
        self.list(w).get(self.selected[w])
    }

    /// Asks for the diff of the selected entry of list `which` (when it is not shown yet).
    fn show_selected(&mut self, which: usize, force: bool) {
        self.showing_staged = which == 1;
        let Some(e) = self.list(which).get(self.selected[which]).cloned() else {
            if self.diff.as_ref().is_some_and(|d| d.staged == (which == 1)) {
                self.diff = None;
            }
            return;
        };
        let same = self
            .diff
            .as_ref()
            .is_some_and(|d| d.path == e.path && d.staged == (which == 1));
        if same && !force {
            return;
        }
        if !same {
            self.diff = None;
            self.diff_cursor = 0;
            self.diff_anchor = None;
            self.diff_top = 0;
            self.diff_left = 0;
        }
        self.diff_pending += 1;
        self.read(ReadReq::Diff {
            path: e.path,
            staged: which == 1,
        });
    }

    /// The rows of the diff pane.
    pub fn diff_rows(&self) -> Vec<DiffRow> {
        let Some(d) = &self.diff else {
            return Vec::new();
        };
        let Some(fd) = d.diff() else {
            return vec![DiffRow::Info(0)];
        };
        if fd.hunks.is_empty() {
            return vec![DiffRow::Info(0)];
        }
        let mut rows = Vec::new();
        for (h, hunk) in fd.hunks.iter().enumerate() {
            rows.push(DiffRow::Header(h));
            for l in 0..hunk.lines.len() {
                rows.push(DiffRow::Line(h, l));
            }
        }
        rows
    }

    /// The text of an info row.
    pub fn diff_info(&self) -> String {
        let Some(d) = &self.diff else {
            return String::new();
        };
        match &d.result {
            Err(e) => sanitize(e),
            Ok(None) => "no changes".into(),
            Ok(Some(fd)) if fd.status == FileStatus::Conflicted => {
                "conflicted: resolve it in the terminal".into()
            }
            Ok(Some(fd)) if fd.binary => "binary file: stage or discard it as a whole".into(),
            Ok(Some(_)) => "no line changes (mode, rename or empty file)".into(),
        }
    }

    /// The hunk of the diff cursor and the changed lines selected in it (`None`: no range).
    fn selection(&self) -> Result<(usize, Option<Vec<usize>>), String> {
        let rows = self.diff_rows();
        let hunk_of = |r: &DiffRow| match r {
            DiffRow::Header(h) | DiffRow::Line(h, _) => Some(*h),
            DiffRow::Info(_) => None,
        };
        let cur = self.diff_cursor.min(rows.len().saturating_sub(1));
        let Some(h) = rows.get(cur).and_then(hunk_of) else {
            return Err("no hunk here".into());
        };
        let Some(anchor) = self.diff_anchor.filter(|&a| a != cur) else {
            return Ok((h, None));
        };
        let (a, b) = (anchor.min(cur), anchor.max(cur));
        let fd = self.diff.as_ref().and_then(ShownDiff::diff);
        let mut lines = Vec::new();
        for r in &rows[a..=b.min(rows.len() - 1)] {
            if hunk_of(r) != Some(h) {
                return Err("select lines of one hunk".into());
            }
            if let DiffRow::Line(_, l) = r
                && fd.is_some_and(|fd| fd.hunks[h].lines[*l].kind != LineKind::Context)
            {
                lines.push(*l);
            }
        }
        if lines.is_empty() {
            return Err("no changed line is selected".into());
        }
        Ok((h, Some(lines)))
    }

    /// The selected file as an action sees it (SEC-18).
    fn seen(&self) -> Option<Seen> {
        let which = self.list_index();
        let entry = self.list(which).get(self.selected[which])?.clone();
        let diff = self
            .diff
            .as_ref()
            .filter(|d| d.path == entry.path && d.staged == (which == 1))
            .and_then(ShownDiff::diff)
            .cloned();
        Some(Seen {
            staged: which == 1,
            entry,
            diff,
        })
    }

    /// The header's branch text: `main ↑1 ↓0`, `detached abc1234`, sanitized.
    pub fn branch_text(&self) -> String {
        let Some(scan) = &self.scan else {
            return String::new();
        };
        let mut s = match &scan.head {
            Some(Head::Branch { name, .. }) => sanitize(name),
            Some(Head::Detached { short, .. }) => format!("detached {}", sanitize(short)),
            None => String::new(),
        };
        if let Some(a) = &scan.ahead {
            s.push_str(&format!(" ↑{} ↓{}", a.ahead, a.behind));
        }
        s
    }

    /// The upstream name for the pull label.
    fn upstream(&self) -> String {
        self.scan
            .as_ref()
            .and_then(|s| s.ahead.as_ref())
            .map_or_else(|| "upstream".to_string(), |a| sanitize(&a.upstream))
    }

    /// Text of the button `pane`.
    pub fn button_label(&self, pane: Pane) -> String {
        let check = |on: bool| if on { "[x]" } else { "[ ]" };
        match pane {
            Pane::Amend => format!("{} Amend", check(self.amend)),
            Pane::SignOff => format!("{} Sign off", check(self.signoff)),
            Pane::Commit if self.commit_cancel.is_some() => "Cancel commit".into(),
            Pane::Commit => "Commit".into(),
            Pane::Push if self.job.as_ref().is_some_and(|j| j.kind == JobKind::Push) => {
                "Cancel push".into()
            }
            Pane::Push => "Push".into(),
            Pane::Pull if self.job.as_ref().is_some_and(|j| j.kind == JobKind::Pull) => {
                "Cancel pull".into()
            }
            Pane::Pull => "Pull".into(),
            Pane::Rescan => "Rescan".into(),
            _ => String::new(),
        }
    }

    /// The line under the buttons: a running job, the write queue, else the last result.
    pub fn status_line(&self) -> String {
        if let Some(j) = &self.job {
            let last = if j.last.is_empty() {
                "running".to_string()
            } else {
                j.last.clone()
            };
            // Writes queued behind the push or pull wait for its lock (D303(2)).
            return match self.queue.front() {
                Some(first) => format!(
                    "{}: {last} · {} waiting: {first}",
                    j.label,
                    self.queue.len()
                ),
                None => format!("{}: {last}", j.label),
            };
        }
        if let Some(first) = self.queue.front() {
            let waiting = self.queue.len() - 1;
            return if waiting == 0 {
                format!("running: {first}")
            } else {
                format!("running: {first} · {waiting} waiting")
            };
        }
        if self.scanning && self.scan.is_none() {
            return "scanning…".into();
        }
        self.note.clone()
    }

    /// The banner rows: program-running config (SEC-19) and an operation in progress (SEC-14).
    pub fn banners(&self) -> Vec<(bool, String)> {
        let mut v = Vec::new();
        if let Some(e) = &self.scan_error {
            v.push((true, format!("git: {e}")));
        }
        let Some(scan) = &self.scan else {
            return v;
        };
        if !scan.programs.is_empty() {
            v.push((
                false,
                format!(
                    "this repo's config runs programs on writes: {}",
                    scan.programs.join(", ")
                ),
            ));
        }
        if !scan.state.is_clean() {
            let what = match scan.state.operation {
                Some(op) => format!("{} in progress", op.name()),
                None => "unresolved".to_string(),
            };
            v.push((
                true,
                format!(
                    "{what} ({}): finish it in the terminal; conflicted files and commit are blocked here",
                    scan.state
                ),
            ));
        }
        v
    }
}

// ---- threads ------------------------------------------------------------------------------

fn reader(writer: Writer, rx: Receiver<ReadReq>, tx: Sender<Msg>) {
    let repo = writer.repo().clone();
    for req in rx {
        // Only the newest scan matters; diffs of a list run in the order asked.
        let msg = match req {
            ReadReq::Scan(generation) => Msg::Scanned(generation, scan(&repo)),
            ReadReq::Diff { path, staged } => {
                let result = if staged {
                    repo.diff_staged(&path)
                } else {
                    repo.diff_unstaged(&path)
                };
                Msg::Diff {
                    path,
                    staged,
                    result,
                }
            }
            ReadReq::Leftovers => {
                Msg::Leftovers(vr_git::leftover_discards(&repo).unwrap_or_default())
            }
            ReadReq::LastMessage => Msg::LastMessage(last_message(&repo)),
            ReadReq::PushTarget => Msg::PushTarget(writer.push_target()),
            ReadReq::PrepareDiscard { seen, hunk } => {
                let label = match &hunk {
                    None => String::new(),
                    Some((h, None)) => format!("hunk {}", h + 1),
                    Some((_, Some(l))) => format!("{} selected line(s)", l.len()),
                };
                let result = check_seen(&repo, &seen).and_then(|()| match (&hunk, &seen.diff) {
                    (None, _) => writer.prepare_discard_file(&seen.entry.path),
                    (Some((h, lines)), Some(d)) => {
                        writer.prepare_discard_hunk(d, *h, lines.as_deref())
                    }
                    (Some(_), None) => Err(GitError::Stale(
                        seen.entry.path.to_string_lossy().into_owned(),
                    )),
                });
                Msg::DiscardPlan { label, result }
            }
        };
        if tx.send(msg).is_err() {
            return;
        }
    }
}

/// HEAD's id and full message (for the Amend box), `None` on an unborn branch.
fn last_message(repo: &Repo) -> Result<Option<(String, String)>, GitError> {
    match commit_base(repo)?.head {
        Some(oid) => {
            let message = repo.commit(&oid)?.message;
            Ok(Some((oid, message)))
        }
        None => Ok(None),
    }
}

fn scan(repo: &Repo) -> Result<Scan, GitError> {
    // HEAD and the staged set read before and after the listing: a commit made from what is
    // shown is checked against them, so they must describe the same moment.
    let before = commit_base(repo).ok();
    let status = repo.status()?;
    let state = repo.state()?;
    let head = repo.head().ok();
    let ahead = repo.ahead_behind().ok().flatten();
    let after = commit_base(repo).ok();
    Ok(Scan {
        status,
        state,
        head,
        ahead,
        programs: repo
            .write_programs()
            .unwrap_or_default()
            .iter()
            .map(|k| sanitize(k))
            .collect(),
        base: before.filter(|b| after.as_ref() == Some(b)),
    })
}

/// The entry and diff the user saw still describe the repo (SEC-18).
fn check_seen(repo: &Repo, seen: &Seen) -> Result<(), GitError> {
    let stale = || GitError::Stale(seen.entry.path.to_string_lossy().into_owned());
    let status = repo.status()?;
    let list = if seen.staged {
        &status.staged
    } else {
        &status.unstaged
    };
    if list.iter().find(|e| e.path == seen.entry.path) != Some(&seen.entry) {
        return Err(stale());
    }
    if let Some(shown) = &seen.diff {
        let now = if seen.staged {
            repo.diff_staged(&seen.entry.path)?
        } else {
            repo.diff_unstaged(&seen.entry.path)?
        };
        if now.as_ref() != Some(shown) {
            return Err(stale());
        }
    }
    Ok(())
}

fn writer_loop(writer: Writer, rx: Receiver<WriteOp>, tx: Sender<Msg>) {
    for op in rx {
        let done = match run_write(&writer, op) {
            Ok(d) => d,
            Err(e) => WriteDone::Err(e),
        };
        if tx.send(Msg::WriteDone(done)).is_err() {
            return;
        }
    }
}

fn run_write(w: &Writer, op: WriteOp) -> Result<WriteDone, GitError> {
    let label = op.label();
    match op {
        WriteOp::StageFile(seen) => {
            check_seen(w.repo(), &seen)?;
            w.stage_file(&seen.entry.path)?;
        }
        WriteOp::UnstageFile(seen) => {
            check_seen(w.repo(), &seen)?;
            w.unstage_file(&seen.entry.path)?;
        }
        WriteOp::Hunk {
            staged,
            shown,
            hunk,
            lines,
        } => match (staged, lines) {
            (false, None) => w.stage_hunk(&shown, hunk)?,
            (false, Some(l)) => w.stage_lines(&shown, hunk, &l)?,
            (true, None) => w.unstage_hunk(&shown, hunk)?,
            (true, Some(l)) => w.unstage_lines(&shown, hunk, &l)?,
        },
        WriteOp::Discard { plan, buffer_dirty } => {
            return w.discard(&plan, buffer_dirty).map(WriteDone::Discarded);
        }
        WriteOp::Commit {
            message,
            opts,
            base,
            cancel,
        } => {
            // SEC-18: a commit made or an index staged from outside since the view was drawn
            // would be amended away or recorded unseen. (git has no "expected parent" option;
            // what is left is the moment between this read and git's own.)
            let now = commit_base(w.repo())?;
            if now.head != base.head {
                return Err(GitError::Stale("HEAD".into()));
            }
            if now.staged != base.staged {
                return Err(GitError::Stale("what is staged".into()));
            }
            return w.commit(&message, opts, &cancel).map(WriteDone::Committed);
        }
        WriteOp::Leftover { path, to } => leftover(w.repo().workdir(), &path, to.as_deref())?,
    }
    Ok(WriteDone::Ok(label))
}

/// Deletes an unused `.new` leftover or moves an `.old` one to `to` (never over an existing
/// file); both repo-relative and inside the work tree.
pub(crate) fn leftover(workdir: &Path, path: &Path, to: Option<&Path>) -> Result<(), GitError> {
    let inside = |p: &Path| {
        !p.as_os_str().is_empty()
            && p.components()
                .all(|c| matches!(c, std::path::Component::Normal(_)))
    };
    if !inside(path) {
        return Err(GitError::OutsideRepo(path.to_path_buf()));
    }
    let full = workdir.join(path);
    match to {
        None => {
            let meta = std::fs::symlink_metadata(&full)?;
            if meta.is_dir() {
                return Err(GitError::Refused(format!(
                    "{} is a folder; remove it in the terminal",
                    path.display()
                )));
            }
            std::fs::remove_file(&full)?;
        }
        Some(to) => {
            if !inside(to) {
                return Err(GitError::OutsideRepo(to.to_path_buf()));
            }
            // Never into git's own folder (a hook there would run on the next write).
            if to
                .components()
                .any(|c| c.as_os_str().eq_ignore_ascii_case(".git"))
            {
                return Err(GitError::Refused(format!(
                    "{} is inside .git; choose a name in the work tree",
                    to.display()
                )));
            }
            let (dir, name) = target_dir(workdir, to)?;
            rustix::fs::renameat_with(
                rustix::fs::CWD,
                &full,
                &dir,
                name,
                rustix::fs::RenameFlags::NOREPLACE,
            )
            .map_err(|e| match e {
                rustix::io::Errno::EXIST => {
                    GitError::Refused(format!("{} already exists", to.display()))
                }
                e => GitError::Io(e.to_string()),
            })?;
        }
    }
    Ok(())
}

/// The existing folder of `to` (repo-relative) opened beneath `workdir` without following a
/// symlink on the way (CWE-59), and the file name in it.
fn target_dir<'a>(
    workdir: &Path,
    to: &'a Path,
) -> Result<(rustix::fd::OwnedFd, &'a std::ffi::OsStr), GitError> {
    use rustix::fs::{Mode, OFlags, ResolveFlags};
    let name = to
        .file_name()
        .ok_or_else(|| GitError::OutsideRepo(to.to_path_buf()))?;
    let parent = to
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let flags = OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC;
    let io = |e: rustix::io::Errno| GitError::Io(e.to_string());
    let root = rustix::fs::open(workdir, flags, Mode::empty()).map_err(io)?;
    let refused = || {
        GitError::Refused(format!(
            "{}: a folder on the way is a link or not in the work tree",
            to.display()
        ))
    };
    match rustix::fs::openat2(
        &root,
        parent,
        flags,
        Mode::empty(),
        ResolveFlags::BENEATH | ResolveFlags::NO_SYMLINKS,
    ) {
        Ok(fd) => Ok((fd, name)),
        Err(rustix::io::Errno::NOENT | rustix::io::Errno::NOTDIR) => Err(GitError::Refused(
            format!("{}: no such folder", parent.display()),
        )),
        // Kernels before 5.6: the resolved folder must be the one named, link-free.
        Err(rustix::io::Errno::NOSYS) => {
            let root = std::fs::canonicalize(workdir)?;
            let want = if parent == Path::new(".") {
                root
            } else {
                root.join(parent)
            };
            match std::fs::canonicalize(workdir.join(parent)) {
                Ok(got) if got == want => {
                    let fd = rustix::fs::open(&got, flags, Mode::empty()).map_err(io)?;
                    Ok((fd, name))
                }
                Ok(_) => Err(refused()),
                Err(e) => Err(e.into()),
            }
        }
        Err(_) => Err(refused()),
    }
}

// ---- opening ------------------------------------------------------------------------------

/// Opens the popup on the repo of the root named `root` (empty: the active file's root).
/// Reuses the session of that repo when it is still there.
pub fn open(app: &mut App, root: &str) -> Result<(), String> {
    let (root_path, root_name) = if root.is_empty() {
        let r = app.active_root();
        (r.path.clone(), r.name.clone())
    } else {
        // Root names only, never a path (SEC-17).
        let r = app
            .workspace
            .roots()
            .iter()
            .find(|r| r.name == root)
            .ok_or_else(|| format!("/git: no such root {}", sanitize(root)))?;
        (r.path.clone(), r.name.clone())
    };
    let Some((repo, parent)) = app.git.repo_info_of_root(&root_path) else {
        return Err(if app.git.discovering() {
            "git: still looking for the repository; try again in a moment".into()
        } else {
            format!("git: {} is not in a git repository", sanitize(&root_name))
        });
    };
    if let Some(p) = &mut app.git_popup
        && p.workdir == repo.workdir()
        && !p.orphan
    {
        p.visible = true;
        p.root = root_path;
        p.parent_of = parent.is_some().then(|| sanitize(&root_name));
        p.rescan();
        // Leftovers are offered on every open (one kept last time is offered again).
        p.leftovers.clear();
        p.read(ReadReq::Leftovers);
        shown(app);
        return Ok(());
    }
    if let Some(p) = &app.git_popup
        && p.busy()
    {
        return Err(format!(
            "git: a write is still running in {}; wait for it or cancel it there",
            p.repo_label
        ));
    }
    let writer = Writer::open(&repo).map_err(|e| format!("git: {}", sanitize(&e.to_string())))?;
    let id = app.git_popup.as_ref().map_or(1, |p| p.id + 1);
    if let Some(old) = app.git_popup.take() {
        keep_draft(app, &old);
        close_session(old, false);
    }
    let mut message = TextArea::default();
    if let Some(i) = app
        .git_drafts
        .iter()
        .position(|(w, _)| w.as_path() == repo.workdir())
    {
        message.set(&app.git_drafts.remove(i).1);
    }
    let (tx, rx) = mpsc::channel();
    let (read_tx, read_rx) = mpsc::channel();
    let (write_tx, write_rx) = mpsc::channel();
    let mut threads = Vec::new();
    let (w, t) = (writer.clone(), tx.clone());
    if let Ok(h) = std::thread::Builder::new()
        .name("vr-git-popup-read".into())
        .spawn(move || reader(w, read_rx, t))
    {
        threads.push(h);
    }
    let w = writer.clone();
    if let Ok(h) = std::thread::Builder::new()
        .name("vr-git-popup-write".into())
        .spawn(move || writer_loop(w, write_rx, tx))
    {
        threads.push(h);
    }
    let mut p = GitPopup {
        visible: true,
        id,
        workdir: repo.workdir().to_path_buf(),
        root: root_path,
        repo_label: crate::git::home_relative(repo.workdir()),
        parent_of: parent.is_some().then(|| sanitize(&root_name)),
        writer,
        read_tx: Some(read_tx),
        write_tx: Some(write_tx),
        rx,
        threads,
        queue: VecDeque::new(),
        queued_discards: VecDeque::new(),
        commit_cancel: None,
        job: None,
        scan_gen: 0,
        scanning: false,
        rescan_at: None,
        scan: None,
        scan_error: None,
        focus: Pane::Unstaged,
        selected: [0, 0],
        list_top: [0, 0],
        showing_staged: false,
        diff: None,
        diff_pending: 0,
        diff_cursor: 0,
        diff_anchor: None,
        diff_top: 0,
        diff_left: 0,
        message,
        amend: false,
        signoff: false,
        amend_loaded: None,
        pre_amend: None,
        amend_oid: None,
        output: Vec::new(),
        show_output: false,
        note: String::new(),
        leftovers: VecDeque::new(),
        discarding: Vec::new(),
        orphan: false,
        writer_stopped: false,
        areas: Areas::default(),
        push_asked: false,
    };
    p.rescan();
    p.read(ReadReq::Leftovers);
    app.git_popup = Some(p);
    shown(app);
    Ok(())
}

/// Ends a session: its threads finish what they run and stop. `wait` (exit): wait a moment
/// for a running write, then shut the writer's runner down.
/// A draft typed for that repo waits for its next session (never moves to another repo).
fn keep_draft(app: &mut App, old: &GitPopup) {
    let draft = old.message.text();
    if !draft.trim().is_empty() && old.amend_loaded.as_deref() != Some(draft) {
        app.git_drafts.retain(|(w, _)| *w != old.workdir);
        app.git_drafts
            .push((old.workdir.clone(), draft.to_string()));
    }
}

fn close_session(mut p: GitPopup, wait: bool) {
    p.read_tx = None;
    p.write_tx = None;
    if !wait {
        return;
    }
    if let Some(c) = &p.commit_cancel {
        c.cancel();
    }
    if let Some(j) = &p.job {
        j.job.cancel();
    }
    let end = Instant::now() + EXIT_WAIT;
    for h in std::mem::take(&mut p.threads) {
        while !h.is_finished() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    p.writer.shutdown();
}

/// Exit: running writes get a moment to finish, push/pull and commit hooks are cancelled.
pub fn shutdown(app: &mut App) {
    if let Some(p) = app.git_popup.take() {
        close_session(p, true);
    }
}

/// `/git push` (the command, bound to the active file's repo): the push target is shown
/// first; while a push or pull runs, it asks to cancel.
pub fn push(app: &mut App) -> Result<(), String> {
    ensure_session(app)?;
    push_here(app);
    Ok(())
}

/// `p` and the Push button: the session's own repo (SEC-17), never the active file's.
fn push_here(app: &mut App) {
    let Some(p) = &mut app.git_popup else {
        return;
    };
    if p.job.is_some() {
        ask_cancel_job(app);
        return;
    }
    p.push_asked = true;
    p.note = "push: reading the target…".into();
    p.read(ReadReq::PushTarget);
}

/// `/git pull`: runs in the background (conflicts stay in the terminal, D297).
pub fn pull(app: &mut App) -> Result<(), String> {
    ensure_session(app)?;
    start_pull(app);
    Ok(())
}

/// A session on the active file's repo, opened when there is none (or it is of another repo
/// and idle; a busy one is kept and the error says why); the popup is shown. Only the
/// `/git push` and `/git pull` commands use it: inside the popup an action stays on the
/// popup's repo (SEC-17).
fn ensure_session(app: &mut App) -> Result<(), String> {
    let root = app.active_root().path.clone();
    let want = app.git.repo_info_of_root(&root).map(|(r, _)| r);
    let have = app.git_popup.as_ref().map(|p| p.workdir.clone());
    match (want, have) {
        (Some(r), Some(w)) if r.workdir() == w => {
            if let Some(p) = &mut app.git_popup {
                p.visible = true;
            }
            shown(app);
            Ok(())
        }
        _ => open(app, ""),
    }
}

// ---- per frame ----------------------------------------------------------------------------

/// Takes answers of the session's threads and job, and scans again after a change.
pub fn tick(app: &mut App, now: Instant) {
    let Some(p) = &mut app.git_popup else {
        return;
    };
    let msgs: Vec<Msg> = p.rx.try_iter().collect();
    let mut events = Vec::new();
    if let Some(j) = &p.job {
        while let Some(e) = j.job.try_recv() {
            events.push(e);
        }
    }
    if p.rescan_at.is_some_and(|t| now >= t) {
        p.rescan();
    }
    let changed = !msgs.is_empty() || !events.is_empty();
    for m in msgs {
        handle_msg(app, m);
    }
    for e in events {
        handle_job_event(app, e);
    }
    if changed {
        app.dirty = true;
    }
    offer_leftover(app);
    if app
        .git_popup
        .as_mut()
        .is_some_and(|p| std::mem::take(&mut p.writer_stopped))
    {
        app.warn("git: the session's writer stopped; the session was closed");
    }
    if let Some(p) = &app.git_popup
        && p.orphan
        && !p.busy()
        && let Some(p) = app.git_popup.take()
    {
        keep_draft(app, &p);
        close_session(p, false);
    }
}

fn handle_msg(app: &mut App, m: Msg) {
    let Some(p) = &mut app.git_popup else {
        return;
    };
    match m {
        Msg::Scanned(generation, result) => {
            if generation != p.scan_gen {
                return;
            }
            p.scanning = false;
            match result {
                Ok(s) => {
                    p.scan_error = None;
                    apply_scan(p, s);
                }
                Err(e) => p.scan_error = Some(sanitize(&e.to_string())),
            }
        }
        Msg::Diff {
            path,
            staged,
            result,
        } => {
            p.diff_pending = p.diff_pending.saturating_sub(1);
            // Only the selected file's diff is shown.
            let which = usize::from(staged);
            let current = p.showing_staged == staged
                && p.list(which).get(p.selected[which]).map(|e| &e.path) == Some(&path);
            if !current {
                return;
            }
            let result = result.map_err(|e| e.to_string());
            if let Some(d) = &mut p.diff
                && d.path == path
                && d.staged == staged
            {
                d.result = result;
            } else {
                p.diff = Some(ShownDiff {
                    path,
                    staged,
                    result,
                });
            }
            let n = p.diff_rows().len();
            p.diff_cursor = p.diff_cursor.min(n.saturating_sub(1));
            if p.diff_anchor.is_some_and(|a| a >= n) {
                p.diff_anchor = None;
            }
        }
        Msg::Leftovers(l) => p.leftovers.extend(l),
        Msg::LastMessage(r) => match r {
            Ok(Some((oid, m))) if p.amend => {
                p.amend_oid = Some(oid);
                if p.message.text().trim().is_empty() || p.amend_loaded.is_some() {
                    p.pre_amend = Some(p.message.text().to_string());
                    p.message.set(&m);
                    p.amend_loaded = Some(m);
                }
            }
            Ok(_) => {}
            Err(e) => {
                let msg = format!("git: {}", sanitize(&e.to_string()));
                app.error(msg);
            }
        },
        Msg::PushTarget(r) => {
            if !std::mem::take(&mut p.push_asked) {
                return;
            }
            match r {
                Ok(t) => {
                    let upstream = if t.set_upstream {
                        " and make it the upstream of the branch"
                    } else {
                        ""
                    };
                    let message = format!(
                        "Push {} of {} to {}{upstream}?",
                        sanitize(&t.branch),
                        p.repo_label,
                        sanitize(&t.to_string())
                    );
                    let id = p.id;
                    app.open_dialog(
                        DialogWidget::Confirm(ConfirmDialog::new(
                            "Push",
                            message,
                            &["Push", "Cancel"],
                        )),
                        DialogAction::GitPush {
                            session: id,
                            target: Box::new(t),
                        },
                    );
                }
                Err(e) => failed(app, "push", &e),
            }
        }
        Msg::DiscardPlan { label, result } => match result {
            Ok(plan) => ask_discard(app, plan, &label),
            Err(e) => failed(app, "discard", &e),
        },
        Msg::WriteDone(done) => write_done(app, done),
    }
}

fn apply_scan(p: &mut GitPopup, s: Scan) {
    // Keep each list's selection on its file when it is still listed.
    let old: [Option<PathBuf>; 2] =
        [0, 1].map(|w| p.list(w).get(p.selected[w]).map(|e| e.path.clone()));
    p.scan = Some(s);
    for w in 0..2 {
        let n = p.list(w).len();
        if let Some(path) = &old[w]
            && let Some(i) = p.list(w).iter().position(|e| &e.path == path)
        {
            p.selected[w] = i;
        }
        p.selected[w] = p.selected[w].min(n.saturating_sub(1));
    }
    // The diff pane follows the list it showed; an emptied list hands over to the other.
    let mut which = usize::from(p.showing_staged);
    if p.list(which).is_empty() && !p.list(1 - which).is_empty() {
        which = 1 - which;
    }
    if matches!(p.focus, Pane::Unstaged | Pane::Staged) {
        p.focus = if which == 1 {
            Pane::Staged
        } else {
            Pane::Unstaged
        };
    }
    p.show_selected(which, true);
}

fn handle_job_event(app: &mut App, e: JobEvent) {
    let Some(p) = &mut app.git_popup else {
        return;
    };
    match e {
        JobEvent::Progress(line) => {
            if let Some(j) = &mut p.job {
                // vr-git sanitizes and redacts; the UI filter runs on everything drawn anyway.
                j.last = sanitize(&line);
            }
        }
        JobEvent::Done(r) => {
            let Some(j) = p.job.take() else {
                return;
            };
            let verb = match j.kind {
                JobKind::Push => "push",
                JobKind::Pull => "pull",
            };
            p.rescan();
            let workdir = p.workdir.clone();
            match r {
                Ok(out) => {
                    p.output = keep_output(&out.output);
                    if out.state.is_clean() {
                        p.note = format!("{} done", j.label);
                        app.info(format!("git: {} done", j.label));
                    } else {
                        p.show_output = true;
                        p.note = format!("{verb} stopped: {}", out.state);
                        app.warn(format!(
                            "git: {verb} stopped ({}); resolve it in the terminal",
                            out.state
                        ));
                    }
                }
                Err(e) => failed(app, verb, &e),
            }
            crate::git::repo_written(app, &workdir);
        }
    }
}

fn keep_output(lines: &[String]) -> Vec<String> {
    let start = lines.len().saturating_sub(OUTPUT_LINES);
    lines[start..].iter().map(|l| sanitize(l)).collect()
}

/// A git error: its text (sanitized) in a notice and in the output view; a stale view is
/// scanned again (SEC-18).
fn failed(app: &mut App, what: &str, e: &GitError) {
    // git's own words, line by line, each through the display filter.
    let lines: Vec<String> = e.to_string().lines().map(sanitize).collect();
    let first = lines.first().cloned().unwrap_or_default();
    if let Some(p) = &mut app.git_popup {
        if let GitError::Stale(_) = e {
            p.rescan();
        }
        if lines.len() > 1 {
            p.output = keep_output(&lines);
            p.show_output = true;
        }
        p.note = format!("{what} failed: {first}");
    }
    app.error(format!("git {what}: {first}"));
}

fn write_done(app: &mut App, done: WriteDone) {
    let Some(p) = &mut app.git_popup else {
        return;
    };
    let label = p.queue.pop_front().unwrap_or_default();
    if let Some(Some(abs)) = p.queued_discards.pop_front()
        && let Some(i) = p.discarding.iter().position(|x| *x == abs)
    {
        p.discarding.remove(i);
    }
    let workdir = p.workdir.clone();
    p.rescan();
    match done {
        WriteDone::Ok(l) => {
            p.note = format!("done: {l}");
        }
        WriteDone::Discarded(d) => {
            let path = sanitize(&d.path.to_string_lossy());
            let text = match &d.recovery {
                Some(r) => format!(
                    "discarded {path}; the earlier content is in {}",
                    sanitize(&r.to_string_lossy())
                ),
                None => format!("discarded {path}"),
            };
            p.note = text.clone();
            app.info(format!("git: {text}"));
        }
        WriteDone::Committed(oid) => {
            p.commit_cancel = None;
            p.message.set("");
            p.amend = false;
            p.amend_loaded = None;
            p.pre_amend = None;
            p.amend_oid = None;
            let short: String = oid.chars().take(7).collect();
            p.note = format!("committed {}", sanitize(&short));
            app.info(format!("git: committed {}", sanitize(&short)));
        }
        WriteDone::Err(e) => {
            if label == "commit" || label.starts_with("amend") {
                p.commit_cancel = None;
            }
            let what = label.split(' ').next().unwrap_or("write").to_string();
            failed(app, &what, &e);
        }
    }
    // Editor gutter marks and the status bar branch follow the write.
    crate::git::repo_written(app, &workdir);
}

/// Leftovers of interrupted discards are offered one at a time while the popup shows.
fn offer_leftover(app: &mut App) {
    if app.dialog.is_some() {
        return;
    }
    let Some(p) = &mut app.git_popup else {
        return;
    };
    if !p.visible {
        return;
    }
    let Some(l) = p.leftovers.pop_front() else {
        return;
    };
    let shown = sanitize(&l.path.to_string_lossy());
    let id = p.id;
    let (message, buttons): (String, &[&str]) = if l.earlier_content {
        (
            format!(
                "An interrupted discard left {shown} in {}: the content of a file before the \
                 discard (its name was not recorded). Restore it under a name, or keep it there?",
                p.repo_label
            ),
            &["Keep", "Restore as…"],
        )
    } else {
        (
            format!(
                "An interrupted discard left {shown} in {}: an unused replacement, safe to \
                 delete. Delete it?",
                p.repo_label
            ),
            &["Keep", "Delete"],
        )
    };
    app.open_dialog(
        DialogWidget::Confirm(ConfirmDialog::new("Interrupted discard", message, buttons)),
        DialogAction::GitLeftover {
            session: id,
            leftover: l,
        },
    );
}

// ---- dialog answers -----------------------------------------------------------------------

fn session(app: &mut App, id: u64) -> Option<&mut GitPopup> {
    app.git_popup.as_mut().filter(|p| p.id == id)
}

/// Discard confirmed (button 1; 0 is Cancel, the default).
pub fn answer_discard(app: &mut App, id: u64, plan: DiscardPlan, button: usize) {
    if button != 1 {
        return;
    }
    let Some(p) = session(app, id) else {
        return;
    };
    let abs = p.workdir.join(plan.path());
    let dirty = app.docs.iter().any(|d| {
        d.is_modified()
            && d.path
                .as_deref()
                .is_some_and(|dp| dp == abs || dp.starts_with(&abs))
    });
    if dirty {
        let shown = sanitize(&plan.path().to_string_lossy());
        app.error(format!(
            "git discard: {shown} has unsaved changes in the editor; save or revert them first"
        ));
        return;
    }
    let Some(p) = session(app, id) else {
        return;
    };
    p.write(WriteOp::Discard {
        plan: Box::new(plan),
        buffer_dirty: dirty,
    });
}

pub fn answer_push(app: &mut App, id: u64, target: PushTarget, button: usize) {
    if button != 0 {
        return;
    }
    let Some(p) = session(app, id) else {
        return;
    };
    if p.job.is_some() {
        return;
    }
    let label = format!("push → {}", sanitize(&target.to_string()));
    let job = p.writer.push(target, || {});
    p.job = Some(RunningJob {
        kind: JobKind::Push,
        label,
        last: String::new(),
        job,
    });
    p.show_output = false;
}

/// Cancel dialog of a running job: button 1 cancels.
pub fn answer_cancel_job(app: &mut App, id: u64, button: usize) {
    if button != 1 {
        return;
    }
    if let Some(p) = session(app, id)
        && let Some(j) = &p.job
    {
        j.job.cancel();
        p.note = format!("{}: cancelling", j.label);
    }
}

pub fn answer_leftover(app: &mut App, id: u64, leftover: LeftoverDiscard, button: usize) {
    if button != 1 {
        return;
    }
    let Some(p) = session(app, id) else {
        return;
    };
    if leftover.earlier_content {
        let dir = leftover
            .path
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .map(|d| format!("{}/", d.to_string_lossy()))
            .unwrap_or_default();
        let id = p.id;
        app.open_dialog(
            DialogWidget::Prompt(PromptDialog::new(
                "Restore as",
                "Name for it (relative to the repository; an existing file is never replaced)",
                &dir,
            )),
            DialogAction::GitLeftoverRestore {
                session: id,
                path: leftover.path,
            },
        );
    } else {
        p.write(WriteOp::Leftover {
            path: leftover.path,
            to: None,
        });
    }
}

pub fn answer_leftover_restore(app: &mut App, id: u64, path: PathBuf, to: &str) {
    let to = to.trim();
    if to.is_empty() {
        return;
    }
    if let Some(p) = session(app, id) {
        p.write(WriteOp::Leftover {
            path,
            to: Some(PathBuf::from(to)),
        });
    }
}

// ---- actions ------------------------------------------------------------------------------

fn notice_refused(app: &mut App, text: impl Into<String>) {
    let text = text.into();
    if let Some(p) = &mut app.git_popup {
        p.note = text.clone();
    }
    app.warn(format!("git: {text}"));
}

/// `space`: stage the selected unstaged file or unstage the selected staged one.
fn toggle_file(app: &mut App) {
    let Some(p) = &mut app.git_popup else {
        return;
    };
    let Some(seen) = p.seen() else {
        return;
    };
    if seen.entry.status == FileStatus::Conflicted {
        return notice_refused(app, "a conflicted file is resolved in the terminal");
    }
    let op = if seen.staged {
        WriteOp::UnstageFile(seen)
    } else {
        WriteOp::StageFile(seen)
    };
    p.write(op);
}

/// `enter`: stage / unstage the hunk of the diff cursor, or the selected lines of it.
fn apply_hunk(app: &mut App) {
    let Some(p) = &mut app.git_popup else {
        return;
    };
    let Some(shown) = p.diff.as_ref().and_then(ShownDiff::diff).cloned() else {
        return;
    };
    let staged = p.diff.as_ref().is_some_and(|d| d.staged);
    match p.selection() {
        Ok((hunk, lines)) => {
            p.diff_anchor = None;
            p.write(WriteOp::Hunk {
                staged,
                shown: Box::new(shown),
                hunk,
                lines,
            });
        }
        Err(e) => notice_refused(app, e),
    }
}

/// `d`: discard the selected unstaged file (list) or the hunk / lines of the diff cursor
/// (diff pane); a confirmation with Cancel first follows (SEC-16).
fn discard(app: &mut App) {
    let Some(p) = &mut app.git_popup else {
        return;
    };
    let from_diff = p.focus == Pane::Diff;
    if p.list_index() == 1 {
        return notice_refused(app, "discard works on unstaged changes; unstage first");
    }
    let Some(seen) = p.seen() else {
        return;
    };
    let hunk = if from_diff {
        if seen.diff.is_none() {
            return;
        }
        match p.selection() {
            Ok(sel) => Some(sel),
            Err(e) => return notice_refused(app, e),
        }
    } else {
        None
    };
    p.read(ReadReq::PrepareDiscard {
        seen: Box::new(seen),
        hunk,
    });
}

fn ask_discard(app: &mut App, plan: DiscardPlan, label: &str) {
    let Some(p) = &app.git_popup else {
        return;
    };
    let path = sanitize(&plan.path().to_string_lossy());
    let what = if plan.is_untracked() {
        format!("Delete the untracked {path} in {}?", p.repo_label)
    } else if label.is_empty() {
        let n = plan.hunks;
        let hunks = if n == 1 {
            "1 hunk".into()
        } else {
            format!("{n} hunks")
        };
        format!("Discard changes to {}: {path} ({hunks})?", p.repo_label)
    } else {
        format!("Discard {label} of {}: {path}?", p.repo_label)
    };
    let message = format!(
        "{what}\nThe current content is copied to the git-recovery folder first; the notice \
         names the copy."
    );
    let id = p.id;
    let mut dialog = ConfirmDialog::new("Discard", message, &["Cancel", "Discard"]);
    dialog.selected = 0;
    app.open_dialog(
        DialogWidget::Confirm(dialog),
        DialogAction::GitDiscard {
            session: id,
            plan: Box::new(plan),
        },
    );
}

/// ctrl+s (`button` false) or the Commit button. While a commit runs, the button cancels it
/// and ctrl+s asks first. The commit carries HEAD and the staged set shown (SEC-18); amending
/// a commit already on the upstream asks first.
fn commit(app: &mut App, button: bool) {
    let Some(p) = &mut app.git_popup else {
        return;
    };
    if let Some(c) = &p.commit_cancel {
        if button {
            c.cancel();
            p.note = "commit: cancelling".into();
        } else {
            let id = p.id;
            let message = format!(
                "A commit is running in {} (its hooks may wait). Cancel it?",
                p.repo_label
            );
            app.open_dialog(
                DialogWidget::Confirm(ConfirmDialog::new(
                    "Git",
                    message,
                    &["Keep running", "Cancel it"],
                )),
                DialogAction::GitCancelCommit { session: id },
            );
        }
        return;
    }
    if p.message.text().trim().is_empty() {
        return notice_refused(app, "write a commit message first");
    }
    let base = p.scan.as_ref().and_then(|s| s.base.clone());
    let stale = match &base {
        None => true,
        Some(b) => p.amend && p.amend_oid.is_some() && p.amend_oid != b.head,
    };
    let Some(base) = base.filter(|_| !stale) else {
        // The view (or the message Amend loaded) is not of the HEAD there now.
        p.rescan();
        let text = if p.amend {
            "HEAD changed, rescan (turn Amend off and on to load its message)"
        } else {
            "the repository changed, rescan"
        };
        return notice_refused(app, text);
    };
    let pending = PendingCommit {
        message: p.message.text().to_string(),
        opts: CommitOptions {
            amend: p.amend,
            signoff: p.signoff,
        },
        base,
    };
    let pushed = p.amend
        && p.scan
            .as_ref()
            .and_then(|s| s.ahead.as_ref())
            .is_some_and(|a| a.ahead == 0);
    if pushed {
        let upstream = p.upstream();
        let message = format!(
            "The last commit of {} is already on {upstream}. Amend it anyway? A push after it \
             is rejected (vr-ide never forces); that is sorted out in the terminal.",
            p.repo_label
        );
        let id = p.id;
        let mut dialog = ConfirmDialog::new("Amend", message, &["Cancel", "Amend anyway"]);
        dialog.selected = 0;
        app.open_dialog(
            DialogWidget::Confirm(dialog),
            DialogAction::GitAmendPushed {
                session: id,
                commit: Box::new(pending),
            },
        );
        return;
    }
    send_commit(p, pending);
}

fn send_commit(p: &mut GitPopup, c: PendingCommit) {
    let cancel = Cancel::new();
    p.commit_cancel = Some(cancel.clone());
    p.write(WriteOp::Commit {
        message: c.message,
        opts: c.opts,
        base: c.base,
        cancel,
    });
}

/// "Already on the upstream" answered: button 1 amends.
pub fn answer_amend_pushed(app: &mut App, id: u64, commit: PendingCommit, button: usize) {
    if button != 1 {
        return;
    }
    if let Some(p) = session(app, id)
        && p.commit_cancel.is_none()
    {
        send_commit(p, commit);
    }
}

/// ctrl+s during a commit answered: button 1 cancels it.
pub fn answer_cancel_commit(app: &mut App, id: u64, button: usize) {
    if button != 1 {
        return;
    }
    if let Some(p) = session(app, id)
        && let Some(c) = &p.commit_cancel
    {
        c.cancel();
        p.note = "commit: cancelling".into();
    }
}

fn toggle_amend(p: &mut GitPopup) {
    p.amend = !p.amend;
    p.amend_oid = None;
    if p.amend {
        p.read(ReadReq::LastMessage);
    } else if p.amend_loaded.as_deref() == Some(p.message.text()) {
        // The box still holds the loaded message: give the draft back.
        let draft = p.pre_amend.take().unwrap_or_default();
        p.message.set(&draft);
        p.amend_loaded = None;
    } else {
        p.amend_loaded = None;
        p.pre_amend = None;
    }
}

fn start_pull(app: &mut App) {
    let Some(p) = &mut app.git_popup else {
        return;
    };
    if p.job.is_some() {
        ask_cancel_job(app);
        return;
    }
    let label = format!("pull ← {}", p.upstream());
    let job = p.writer.pull(|| {});
    p.job = Some(RunningJob {
        kind: JobKind::Pull,
        label,
        last: String::new(),
        job,
    });
    p.show_output = false;
}

fn ask_cancel_job(app: &mut App) {
    let Some(p) = &app.git_popup else {
        return;
    };
    let Some(j) = &p.job else {
        return;
    };
    let id = p.id;
    let message = format!("{} is running. Cancel it?", j.label);
    app.open_dialog(
        DialogWidget::Confirm(ConfirmDialog::new(
            "Git",
            message,
            &["Keep running", "Cancel it"],
        )),
        DialogAction::GitCancelJob { session: id },
    );
}

fn activate(app: &mut App, pane: Pane) {
    let Some(p) = &mut app.git_popup else {
        return;
    };
    match pane {
        Pane::Amend => toggle_amend(p),
        Pane::SignOff => p.signoff = !p.signoff,
        Pane::Commit => commit(app, true),
        Pane::Push => push_here(app),
        Pane::Pull => start_pull(app),
        Pane::Rescan => p.rescan(),
        _ => {}
    }
}

// ---- keys ---------------------------------------------------------------------------------

/// Keys while the popup shows (it is modal: every key stops here).
pub fn handle_key(app: &mut App, key: &KeyEvent) {
    let Some(p) = &mut app.git_popup else {
        return;
    };
    app.dirty = true;
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    match key.code {
        KeyCode::Esc => {
            p.visible = false;
            return;
        }
        KeyCode::Tab | KeyCode::BackTab => {
            let i = ring_index(p.focus);
            let back = key.code == KeyCode::BackTab || shift;
            let n = RING.len();
            p.focus = match RING[if back { (i + n - 1) % n } else { (i + 1) % n }] {
                // Back to the list the diff shows.
                Pane::Unstaged if p.showing_staged => Pane::Staged,
                pane => pane,
            };
            return;
        }
        _ => {}
    }
    if p.focus == Pane::Message {
        if ctrl && matches!(key.code, KeyCode::Char('s')) {
            commit(app, false);
        } else {
            p.message.handle_key(key);
        }
        return;
    }
    if ctrl {
        return;
    }
    match key.code {
        KeyCode::Char('1') => {}
        KeyCode::Char('c') => p.focus = Pane::Message,
        KeyCode::Char('r') => p.rescan(),
        KeyCode::Char('p') => push_here(app),
        KeyCode::Char('d') => discard(app),
        KeyCode::Char(' ') | KeyCode::Enter
            if matches!(
                p.focus,
                Pane::Amend | Pane::SignOff | Pane::Commit | Pane::Push | Pane::Pull | Pane::Rescan
            ) =>
        {
            let pane = p.focus;
            activate(app, pane);
        }
        KeyCode::Char(' ') => toggle_file(app),
        KeyCode::Enter => apply_hunk(app),
        KeyCode::Up
        | KeyCode::Down
        | KeyCode::PageUp
        | KeyCode::PageDown
        | KeyCode::Home
        | KeyCode::End => navigate(p, key.code, shift),
        // The diff pane scrolls sideways (lines wider than the pane end in an ellipsis).
        KeyCode::Left if p.focus == Pane::Diff => {
            p.diff_left = p.diff_left.saturating_sub(DIFF_STEP);
        }
        KeyCode::Right if p.focus == Pane::Diff => p.diff_left += DIFF_STEP,
        KeyCode::Left | KeyCode::Right
            if !matches!(p.focus, Pane::Unstaged | Pane::Staged | Pane::Diff) =>
        {
            let i = ring_index(p.focus);
            let j = if key.code == KeyCode::Left {
                i - 1
            } else {
                i + 1
            };
            if let Some(&pane) = RING.get(j)
                && !matches!(pane, Pane::Message)
            {
                p.focus = pane;
            }
        }
        _ => {}
    }
}

fn ring_index(pane: Pane) -> usize {
    let pane = if pane == Pane::Staged {
        Pane::Unstaged
    } else {
        pane
    };
    RING.iter().position(|&x| x == pane).unwrap_or(0)
}

fn page(rect: Rect) -> usize {
    usize::from(rect.height.saturating_sub(1)).max(1)
}

fn navigate(p: &mut GitPopup, code: KeyCode, shift: bool) {
    p.show_output = false;
    match p.focus {
        Pane::Unstaged | Pane::Staged => {
            let w = usize::from(p.focus == Pane::Staged);
            let n = p.list(w).len();
            // One column: Down past the last unstaged file goes on to the staged ones, Up
            // above the first staged file back (also out of an empty section).
            let other = 1 - w;
            let cross = !p.list(other).is_empty()
                && match (w, code) {
                    (0, KeyCode::Down) => n == 0 || p.selected[0] + 1 >= n,
                    (1, KeyCode::Up) => n == 0 || p.selected[1] == 0,
                    _ => false,
                };
            if cross {
                p.focus = if other == 0 {
                    Pane::Unstaged
                } else {
                    Pane::Staged
                };
                p.selected[other] = if other == 1 { 0 } else { p.list(0).len() - 1 };
                p.show_selected(other, false);
                return;
            }
            if n == 0 {
                return;
            }
            let area = if w == 0 {
                p.areas.unstaged
            } else {
                p.areas.staged
            };
            let s = &mut p.selected[w];
            *s = step(*s, n, code, page(area));
            p.show_selected(w, false);
        }
        Pane::Diff => {
            let n = p.diff_rows().len();
            if n == 0 {
                return;
            }
            if shift {
                p.diff_anchor.get_or_insert(p.diff_cursor);
            } else {
                p.diff_anchor = None;
            }
            p.diff_cursor = step(p.diff_cursor, n, code, page(p.areas.diff));
        }
        _ => {}
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

pub fn paste(app: &mut App, text: &str) {
    if let Some(p) = &mut app.git_popup
        && p.focus == Pane::Message
    {
        p.message
            .insert(&text.replace("\r\n", "\n").replace('\r', "\n"));
        app.dirty = true;
    }
}

// ---- mouse --------------------------------------------------------------------------------

fn inside(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

/// Mouse on the popup: lists, diff rows, the message box, buttons and the tab.
pub fn handle_mouse(app: &mut App, ev: &MouseEvent) {
    let Some(p) = &mut app.git_popup else {
        return;
    };
    app.dirty = true;
    let (x, y) = (ev.column, ev.row);
    let a = p.areas.clone();
    let wheel = match ev.kind {
        MouseEventKind::ScrollUp => Some(KeyCode::Up),
        MouseEventKind::ScrollDown => Some(KeyCode::Down),
        _ => None,
    };
    if let Some(code) = wheel {
        for (r, pane) in [
            (a.unstaged, Pane::Unstaged),
            (a.staged, Pane::Staged),
            (a.diff, Pane::Diff),
        ] {
            if inside(r, x, y) {
                if pane == Pane::Diff && p.focus == Pane::Diff {
                    // The focused diff keeps its cursor in view: the wheel moves the cursor.
                    for _ in 0..3 {
                        navigate(p, code, false);
                    }
                } else if pane == Pane::Diff {
                    let n = p.diff_rows().len();
                    p.diff_top = if code == KeyCode::Up {
                        p.diff_top.saturating_sub(3)
                    } else {
                        (p.diff_top + 3).min(n.saturating_sub(1))
                    };
                } else {
                    p.focus = pane;
                    navigate(p, code, false);
                }
            }
        }
        return;
    }
    let shift = ev.modifiers.contains(KeyModifiers::SHIFT);
    match ev.kind {
        MouseEventKind::Down(MouseButton::Left) => {}
        MouseEventKind::Drag(MouseButton::Left)
            if inside(a.diff, x, y) && p.focus == Pane::Diff =>
        {
            let row = p.diff_top + usize::from(y - a.diff.y);
            if row < p.diff_rows().len() {
                p.diff_anchor.get_or_insert(p.diff_cursor);
                p.diff_cursor = row;
            }
            return;
        }
        _ => return,
    }
    if let Some((_, pane)) = a.buttons.iter().find(|(r, _)| inside(*r, x, y)) {
        let pane = *pane;
        p.focus = pane;
        activate(app, pane);
        return;
    }
    if inside(a.tabs, x, y) {
        return;
    }
    for (r, w) in [(a.unstaged, 0usize), (a.staged, 1)] {
        if inside(r, x, y) {
            p.focus = if w == 0 { Pane::Unstaged } else { Pane::Staged };
            p.show_output = false;
            // The first row of a list area is its header.
            let row = usize::from(y - r.y);
            if row >= 1 {
                let i = p.list_top[w] + row - 1;
                if i < p.list(w).len() {
                    p.selected[w] = i;
                }
            }
            p.show_selected(w, false);
            return;
        }
    }
    if inside(a.diff, x, y) {
        p.focus = Pane::Diff;
        p.show_output = false;
        let row = p.diff_top + usize::from(y - a.diff.y);
        if row < p.diff_rows().len() {
            if shift {
                p.diff_anchor.get_or_insert(p.diff_cursor);
            } else {
                p.diff_anchor = None;
            }
            p.diff_cursor = row;
        }
        return;
    }
    if inside(a.message, x, y) {
        p.focus = Pane::Message;
    }
}

// ---- hooks --------------------------------------------------------------------------------

/// A save of `path` must wait while its discard runs (D305(4)): the message, or `None`.
pub fn save_blocked(app: &App, path: &Path) -> Option<String> {
    let p = app.git_popup.as_ref()?;
    p.discarding
        .iter()
        .any(|d| path == d || path.starts_with(d))
        .then(|| {
            format!(
                "{}: a git discard of this file is running; save again when it is done",
                app.display_path(path)
            )
        })
}

/// A file under the session's work tree or its `.git` changed: scan again shortly.
pub fn on_change(app: &mut App, path: &Path) {
    if let Some(p) = &mut app.git_popup
        && path.starts_with(&p.workdir)
        && p.rescan_at.is_none()
    {
        p.rescan_at = Some(Instant::now() + RESCAN_DELAY);
    }
}

/// The workspace roots changed: a session whose root went away ends once its writes are done
/// (never killed, D304).
pub fn roots_changed(app: &mut App) {
    let paths = app.workspace.paths();
    if let Some(p) = &mut app.git_popup
        && !paths.contains(&p.root)
    {
        p.orphan = true;
        p.visible = false;
    }
}

/// The popup came on screen: the palette closes, and an Enter still held from the command
/// window is not taken as a stage (the app's settle time for modal windows).
fn shown(app: &mut App) {
    crate::commands::palette::close(app);
    app.modal_since = Some(app.now());
    app.dirty = true;
}
