//! `search.*` commands and the project search / replace panel (AC-11, AC-12). Rendering:
//! `ui::search_panel`.
//!
//! The search runs on vr-search's background threads and streams batches into the panel
//! ([`drain`], called by the event loop); a new search cancels the running one.
//!
//! Project replace: Enter in the replace field shows a per-file preview (nothing written).
//! Space unchecks a file, Enter asks Replace / Cancel (Cancel is the default). A file open in
//! a tab is changed in its buffer as one transaction (undoable, not saved), never on disk
//! behind the tab; the other files are written by `vr_search::apply_replace`, which refuses a
//! file changed on disk since the preview. Each file's result is shown in the preview.
//!
//! Built-in keys while the panel has focus (not in the keymap, like the palette's): typing
//! and line editing in the fields, Tab / shift+Tab move between the fields and the list,
//! Up/Down/PageUp/PageDown/Home/End move in the list (Down leaves a field for the list),
//! Enter searches / previews / opens the result, Space checks a preview file, Esc returns to
//! the editor (closes a preview first), alt+c / alt+r / alt+w toggle case, regex and whole
//! word. A chord the keymap binds globally (f4, ctrl+shift+f) runs its command first.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread::JoinHandle;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use vr_config::Context;
use vr_core::Selection;
use vr_core::ropey::Rope;
use vr_search::{FilePreview, Hunk, ProjectMatch, Query, SearchMsg};

use super::dialog::DialogAction;
use super::registry::{CmdArgs, CmdResult, CommandSpec};
use crate::app::{App, Focus};
use crate::event_loop::chord_from_key;
use crate::ui::dialog::{ConfirmDialog, DialogWidget, LineInput};

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "search.project",
        title: "Search in Project",
        run: search_project,
    },
    CommandSpec {
        name: "search.next_result",
        title: "Search: Next Result",
        run: next_result,
    },
    CommandSpec {
        name: "search.prev_result",
        title: "Search: Previous Result",
        run: prev_result,
    },
    CommandSpec {
        name: "search.replace_project",
        title: "Replace in Project",
        run: replace_project,
    },
];

/// The confirm dialog's buttons; Cancel is the default (a writing choice never is).
pub const REPLACE: &str = "Replace";
pub const CANCEL: &str = "Cancel";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Field {
    #[default]
    Query,
    Replace,
    Results,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Idle,
    Running,
    Done {
        files: usize,
        matches: usize,
    },
}

/// The matches of one file, in arrival order.
#[derive(Debug, Clone)]
pub struct FileGroup {
    pub path: PathBuf,
    pub matches: Vec<ProjectMatch>,
}

/// One row of the results list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    File(usize),
    Match(usize, usize),
}

/// A search in progress: dropping it (or setting `cancel`) stops the walk.
#[derive(Debug)]
pub struct Running {
    pub cancel: Arc<AtomicBool>,
    rx: Receiver<SearchMsg>,
}

/// Where a previewed file's replacement goes.
#[derive(Debug, Clone)]
pub enum Target {
    /// Not previewed yet (the worker has not reached the file).
    Unknown,
    /// Open in a tab: applied to the buffer as one transaction, only if the buffer still
    /// holds the previewed `text`.
    Buffer { canon: PathBuf, text: Rope },
    /// Not open: written by `vr_search::apply_replace` if the file is still as previewed
    /// and still not open in a tab.
    Disk { canon: PathBuf },
}

#[derive(Debug, Clone)]
pub struct PreviewFile {
    pub path: PathBuf,
    pub hunks: Vec<Hunk>,
    pub count: usize,
    /// Why the file cannot be replaced (it is listed unchecked).
    pub error: Option<String>,
    pub checked: bool,
    pub target: Target,
    /// After the replace: the number of matches replaced, or why the file was skipped.
    pub result: Option<Result<usize, String>>,
    /// vr-search's preview of a file that was not open: what `apply_replace` writes.
    plan: Option<FilePreview>,
}

/// Rows of the preview list: a file, a note under it (error or result), its changed lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewRow {
    File(usize),
    Note(usize),
    Before(usize, usize),
    After(usize, usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    /// Reading the result files and computing their replacement.
    Preview,
    /// Writing the confirmed files.
    Apply,
}

/// The worker building a preview or applying it. Files go in batches of [`JOB_BATCH`];
/// the worker checks `cancel` before each batch.
#[derive(Debug)]
pub struct Job {
    pub kind: JobKind,
    /// Files finished so far, of `total`.
    pub done: usize,
    pub total: usize,
    cancel: Arc<AtomicBool>,
    rx: Receiver<JobMsg>,
    /// Apply: the files handed to the worker (each must end with a result).
    sent: Vec<usize>,
    /// The worker thread: joined on exit so a file it started is finished.
    handle: Option<JoinHandle<()>>,
}

impl Job {
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

/// A file previewed by the worker: its index, canonical path, whether it was open in a tab
/// when the preview started (then the UI thread previews its buffer) and vr-search's
/// preview otherwise.
type Planned = (usize, PathBuf, bool, Option<FilePreview>);

#[derive(Debug)]
enum JobMsg {
    Planned(Vec<Planned>),
    Applied(Vec<(usize, Result<(), String>)>),
    Done,
}

const JOB_BATCH: usize = 32;

#[derive(Debug)]
pub struct Preview {
    pub query: Query,
    pub replacement: String,
    pub files: Vec<PreviewFile>,
    /// The replace ran; the rows show each file's result.
    pub applied: bool,
    pub selected: usize,
    pub top: usize,
    /// The worker building or applying this preview.
    pub job: Option<Job>,
    /// Files the replace changed in open tabs.
    in_tabs: usize,
}

impl Preview {
    pub fn rows(&self) -> Vec<PreviewRow> {
        let mut rows = Vec::new();
        for (i, f) in self.files.iter().enumerate() {
            rows.push(PreviewRow::File(i));
            if f.error.is_some() || f.result.is_some() {
                rows.push(PreviewRow::Note(i));
            }
            for h in 0..f.hunks.len() {
                rows.push(PreviewRow::Before(i, h));
                rows.push(PreviewRow::After(i, h));
            }
        }
        rows
    }

    /// The preview is still being built.
    pub fn building(&self) -> bool {
        self.job
            .as_ref()
            .is_some_and(|j| j.kind == JobKind::Preview)
    }

    /// A replace of this preview is running.
    pub fn applying(&self) -> bool {
        self.job.as_ref().is_some_and(|j| j.kind == JobKind::Apply)
    }

    /// "Preparing preview 12/80" / "Replacing 3/80": the worker's progress.
    pub fn progress_text(&self) -> Option<String> {
        let j = self.job.as_ref()?;
        Some(match (j.kind, j.cancelled()) {
            (JobKind::Apply, true) => format!("Cancelling… {}/{}", j.done, j.total),
            (JobKind::Apply, false) => {
                format!("Replacing {}/{} (Esc cancels)", j.done, j.total)
            }
            (JobKind::Preview, _) => {
                format!("Preparing preview {}/{} (Esc cancels)", j.done, j.total)
            }
        })
    }

    /// The file a row belongs to.
    pub fn file_of(row: PreviewRow) -> usize {
        match row {
            PreviewRow::File(i)
            | PreviewRow::Note(i)
            | PreviewRow::Before(i, _)
            | PreviewRow::After(i, _) => i,
        }
    }
}

#[derive(Debug, Default)]
pub struct SearchPanel {
    pub visible: bool,
    pub replace_mode: bool,
    pub field: Field,
    pub input: LineInput,
    pub replace: LineInput,
    pub case_sensitive: bool,
    pub regex: bool,
    pub whole_word: bool,
    /// The query the results are for.
    pub query: Option<Query>,
    pub groups: Vec<FileGroup>,
    index: HashMap<PathBuf, usize>,
    pub total: usize,
    pub status: Status,
    /// Errors of the search (invalid regex, files too large, the match limit).
    pub errors: Vec<String>,
    pub run: Option<Running>,
    /// Build the replace preview as soon as the running search is done.
    pub preview_pending: bool,
    pub preview: Option<Preview>,
    /// Test hook: when set, the preview / replace worker passes a batch of files only while
    /// this holds permits (one per batch).
    pub hold: Option<Arc<AtomicUsize>>,
    /// Selected row of the results list, and the first visible one.
    pub selected: usize,
    pub top: usize,
    /// Panel and list areas in the last frame.
    pub area: Rect,
    pub list_area: Rect,
}

impl SearchPanel {
    pub fn query(&self) -> Query {
        Query {
            pattern: self.input.text().to_string(),
            regex: self.regex,
            case_sensitive: self.case_sensitive,
            whole_word: self.whole_word,
        }
    }

    /// A preview is being built or a replace runs.
    pub fn busy(&self) -> bool {
        self.preview.as_ref().is_some_and(|p| p.job.is_some())
    }

    pub fn running(&self) -> bool {
        self.run.is_some()
    }

    /// Results rows: each file followed by its matches.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::with_capacity(self.groups.len() + self.total);
        for (gi, g) in self.groups.iter().enumerate() {
            rows.push(Row::File(gi));
            rows.extend((0..g.matches.len()).map(|mi| Row::Match(gi, mi)));
        }
        rows
    }

    /// "3 results in 2 files" / "Searching… 12 results".
    pub fn status_text(&self) -> String {
        let plural =
            |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        match self.status {
            Status::Idle => String::new(),
            Status::Running => format!("Searching… {}", plural(self.total, "result", "results")),
            Status::Done { .. } if self.total == 0 => "No results".to_string(),
            Status::Done { .. } => format!(
                "{} in {}",
                plural(self.total, "result", "results"),
                plural(self.groups.len(), "file", "files")
            ),
        }
    }
}

/// Stops the running search (its thread sees the flag; dropping the receiver ends it too).
fn cancel_running(app: &mut App) {
    if let Some(r) = app.search.run.take() {
        r.cancel.store(true, Ordering::SeqCst);
    }
}

/// Shows the results of `rx` from now on; the previous search is cancelled and whatever it
/// still sends is never read.
pub fn attach(app: &mut App, rx: Receiver<SearchMsg>, cancel: Arc<AtomicBool>) {
    cancel_running(app);
    drop_preview(app);
    let s = &mut app.search;
    s.groups.clear();
    s.index.clear();
    s.total = 0;
    s.errors.clear();
    s.selected = 0;
    s.top = 0;
    s.status = Status::Running;
    s.run = Some(Running { cancel, rx });
    app.dirty = true;
}

/// Starts a search for the panel's query off the UI thread (cancelling the running one).
/// Not while a replace runs: its results are still coming in.
pub fn start(app: &mut App) {
    // A preview asked for earlier is for another search; `request_preview` asks again.
    app.search.preview_pending = false;
    if app.search.preview.as_ref().is_some_and(Preview::applying) {
        app.info("Replace in Project: the replace is running; Esc cancels it");
        return;
    }
    let q = app.search.query();
    if q.pattern.is_empty() {
        cancel_running(app);
        drop_preview(app);
        let s = &mut app.search;
        s.groups.clear();
        s.index.clear();
        s.total = 0;
        s.errors.clear();
        s.status = Status::Idle;
        s.query = None;
        return;
    }
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    // Detached: the walk ends when it is done, cancelled or its receiver is dropped.
    drop(vr_search::search_project(&app.root, &q, cancel.clone(), tx));
    attach(app, rx, cancel);
    app.search.query = Some(q);
}

/// Takes every message the running search and the preview / replace worker sent so far,
/// without blocking.
pub fn drain(app: &mut App) {
    drain_job(app);
    let Some(run) = &app.search.run else {
        return;
    };
    let mut msgs = Vec::new();
    let mut ended = false;
    loop {
        match run.rx.try_recv() {
            Ok(m) => msgs.push(m),
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                ended = true;
                break;
            }
        }
    }
    if msgs.is_empty() && !ended {
        return;
    }
    let mut done = false;
    let mut warnings = Vec::new();
    let s = &mut app.search;
    for m in msgs {
        match m {
            SearchMsg::Batch(batch) => {
                for pm in batch {
                    let gi = match s.index.get(&pm.path) {
                        Some(&gi) => gi,
                        None => {
                            s.groups.push(FileGroup {
                                path: pm.path.clone(),
                                matches: Vec::new(),
                            });
                            s.index.insert(pm.path.clone(), s.groups.len() - 1);
                            s.groups.len() - 1
                        }
                    };
                    s.groups[gi].matches.push(pm);
                    s.total += 1;
                }
            }
            SearchMsg::Error(e) => {
                warnings.push(format!("Search: {e}"));
                s.errors.push(e);
            }
            SearchMsg::Done { files, matches, .. } => {
                s.status = Status::Done { files, matches };
                done = true;
            }
        }
    }
    if ended && !done {
        s.errors.push("the search stopped unexpectedly".to_string());
        s.status = Status::Done {
            files: 0,
            matches: s.total,
        };
        done = true;
    }
    if done {
        s.run = None;
    }
    for w in warnings {
        app.warn(w);
    }
    if done && std::mem::take(&mut app.search.preview_pending) {
        build_preview(app);
    }
    app.dirty = true;
}

/// The selection of the editor as a one-line query, if it is one.
fn selection_seed(app: &App) -> Option<String> {
    if app.focus != Focus::Editor {
        return None;
    }
    let doc = app.active_doc()?;
    let r = doc.buffer.selection().primary();
    if r.is_empty() || r.to() - r.from() > 1000 {
        return None;
    }
    let s = doc.buffer.text().slice(r.from()..r.to()).to_string();
    (!s.contains(['\n', '\r'])).then_some(s)
}

fn show(app: &mut App, replace: bool) {
    if let Some(seed) = selection_seed(app) {
        app.search.input.set(&seed);
    }
    let s = &mut app.search;
    s.visible = true;
    if replace {
        s.replace_mode = true;
    }
    s.field = if replace && !s.input.text().is_empty() {
        Field::Replace
    } else {
        Field::Query
    };
    app.focus = Focus::Search;
    app.dirty = true;
}

fn search_project(app: &mut App, _: CmdArgs) -> CmdResult {
    show(app, false);
    Ok(())
}

fn replace_project(app: &mut App, _: CmdArgs) -> CmdResult {
    show(app, true);
    Ok(())
}

/// Opens the file of match `mi` of group `gi` and selects the match (file:line:col).
fn open_match(app: &mut App, gi: usize, mi: usize) {
    let Some(m) = app
        .search
        .groups
        .get(gi)
        .and_then(|g| g.matches.get(mi))
        .cloned()
    else {
        return;
    };
    if let Err(e) = app.open_path(&m.path) {
        app.error(e.to_string());
        return;
    }
    if let Some(doc) = app.active_doc_mut() {
        let rope = doc.buffer.text();
        let line = m
            .line
            .saturating_sub(1)
            .min(rope.len_lines().saturating_sub(1));
        let end = vr_core::text::line_content_end(rope, line);
        let from = (rope.line_to_char(line) + m.col.saturating_sub(1)).min(end);
        let to = (from + m.match_range.1.saturating_sub(m.match_range.0)).min(end);
        doc.buffer.set_selection(Selection::single(from, to));
    }
    app.cursor_moved();
}

/// Enter on a result row: opens it (a file row opens its first match) and gives the editor
/// the keys. On a screen too narrow for both, the panel steps aside.
fn open_selected(app: &mut App) {
    let rows = app.search.rows();
    let Some(&row) = rows.get(app.search.selected) else {
        return;
    };
    let (gi, mi) = match row {
        Row::Match(g, m) => (g, m),
        Row::File(g) => (g, 0),
    };
    open_match(app, gi, mi);
    app.focus = Focus::Editor;
    if app.layout.search.width > 0 && app.layout.editor.width == 0 {
        app.search.visible = false;
    }
}

/// f4 / shift+f4: the next (previous) result after the selected row, wrapping; opens it.
fn step_result(app: &mut App, forward: bool) {
    let rows = app.search.rows();
    let matches: Vec<usize> = (0..rows.len())
        .filter(|&i| matches!(rows[i], Row::Match(..)))
        .collect();
    if matches.is_empty() {
        app.info("Search: no results");
        return;
    }
    let cur = app.search.selected;
    let on_match = matches!(rows.get(cur), Some(Row::Match(..)));
    let i = if forward {
        matches
            .iter()
            .copied()
            .find(|&i| if on_match { i > cur } else { i >= cur })
            .unwrap_or(matches[0])
    } else {
        matches
            .iter()
            .rev()
            .copied()
            .find(|&i| i < cur)
            .unwrap_or(matches[matches.len() - 1])
    };
    app.search.selected = i;
    if let Row::Match(g, m) = rows[i] {
        open_match(app, g, m);
    }
}

fn next_result(app: &mut App, _: CmdArgs) -> CmdResult {
    step_result(app, true);
    Ok(())
}

fn prev_result(app: &mut App, _: CmdArgs) -> CmdResult {
    step_result(app, false);
    Ok(())
}

/// Canonical form of `p` (links resolved); `p` itself when it cannot be resolved.
fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// Open tabs by the canonical path of their file: a tab opened through a link is the
/// same file as the search result of its target.
fn open_tabs(app: &App) -> HashMap<PathBuf, usize> {
    app.docs
        .iter()
        .enumerate()
        .filter_map(|(i, d)| d.path.as_deref().map(|p| (canon(p), i)))
        .collect()
}

/// The changed lines of replacing `q` in `text`: matches never span lines and the
/// replacement holds no line break, so line `n` before is line `n` after.
fn buffer_hunks(text: &Rope, q: &Query, replacement: &str) -> Result<(Vec<Hunk>, usize), String> {
    let tx = vr_search::replace_all_tx(text, q, replacement).map_err(|e| e.to_string())?;
    let count = tx.changes.len();
    let mut lines: Vec<usize> = tx
        .changes
        .iter()
        .map(|c| text.char_to_line(c.from))
        .collect();
    lines.dedup();
    let mut after = text.clone();
    vr_core::transaction::apply_changes(&mut after, &tx.changes);
    let content = |r: &Rope, n: usize| {
        let s = r.line_to_char(n);
        r.slice(s..vr_core::text::line_content_end(r, n))
            .to_string()
    };
    let hunks = lines
        .into_iter()
        .map(|n| Hunk {
            line: n + 1,
            before: content(text, n),
            after: content(&after, n),
        })
        .collect();
    Ok((hunks, count))
}

/// Enter in the replace field: the preview of the results of the current query (searched
/// first when the results are for another query or still coming).
fn request_preview(app: &mut App) {
    let q = app.search.query();
    if q.pattern.is_empty() {
        app.info("Replace in Project: type what to search for first");
        return;
    }
    if app.search.preview.as_ref().is_some_and(Preview::applying) {
        app.info("Replace in Project: the replace is running; Esc cancels it");
        return;
    }
    if app.search.query.as_ref() != Some(&q) {
        start(app);
    }
    if app.search.running() {
        app.search.preview_pending = true;
        return;
    }
    build_preview(app);
}

/// Waits while the test hook lets no batch pass (each batch takes one permit); false once
/// the job is cancelled.
fn proceed(cancel: &AtomicBool, hold: Option<&AtomicUsize>) -> bool {
    loop {
        if cancel.load(Ordering::SeqCst) {
            return false;
        }
        let Some(h) = hold else {
            return true;
        };
        if h.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// Starts the per-file preview on a worker thread; nothing is written. The UI thread reads
/// no result file: the worker resolves each path, leaves the files open in a tab (when the
/// preview starts) to the UI thread, which previews their buffers, and runs
/// `vr_search::plan_replace` on the others, batch by batch.
fn build_preview(app: &mut App) {
    let Some(q) = app.search.query.clone() else {
        return;
    };
    if app.search.groups.is_empty() {
        app.info("Replace in Project: no results to replace");
        return;
    }
    drop_preview(app);
    let replacement = app.search.replace.text().to_string();
    let tabs: HashSet<PathBuf> = open_tabs(app).into_keys().collect();
    let paths: Vec<PathBuf> = app.search.groups.iter().map(|g| g.path.clone()).collect();
    let files = paths
        .iter()
        .map(|p| PreviewFile {
            path: p.clone(),
            hunks: Vec::new(),
            count: 0,
            error: None,
            checked: false,
            target: Target::Unknown,
            result: None,
            plan: None,
        })
        .collect();
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let mut job = Job {
        kind: JobKind::Preview,
        done: 0,
        total: paths.len(),
        cancel: cancel.clone(),
        rx,
        sent: Vec::new(),
        handle: None,
    };
    let (root, hold) = (app.root.clone(), app.search.hold.clone());
    let (wq, wr) = (q.clone(), replacement.clone());
    // Not joined while the app runs: it stops at the next batch once cancelled or once the
    // receiver is gone; `shutdown` joins it on exit.
    job.handle = Some(std::thread::spawn(move || {
        for (b, batch) in paths.chunks(JOB_BATCH).enumerate() {
            if !proceed(&cancel, hold.as_deref()) {
                return;
            }
            let canons: Vec<PathBuf> = batch.iter().map(|p| canon(p)).collect();
            let in_tab: Vec<bool> = canons.iter().map(|c| tabs.contains(c)).collect();
            let disk: Vec<PathBuf> = batch
                .iter()
                .zip(&in_tab)
                .filter(|&(_, &t)| !t)
                .map(|(p, _)| p.clone())
                .collect();
            // plan_replace answers each path in order: zipped back by position.
            let mut plans = vr_search::plan_replace(&root, &wq, &wr, &disk).into_iter();
            let out: Vec<Planned> = canons
                .into_iter()
                .zip(in_tab)
                .enumerate()
                .map(|(i, (c, t))| (b * JOB_BATCH + i, c, t, if t { None } else { plans.next() }))
                .collect();
            if tx.send(JobMsg::Planned(out)).is_err() {
                return;
            }
        }
        let _ = tx.send(JobMsg::Done);
    }));
    app.search.preview = Some(Preview {
        query: q,
        replacement,
        files,
        applied: false,
        selected: 0,
        top: 0,
        job: Some(job),
        in_tabs: 0,
    });
    app.search.field = Field::Results;
    app.dirty = true;
}

/// Takes what the preview / replace worker sent so far, without blocking.
fn drain_job(app: &mut App) {
    let Some(job) = app.search.preview.as_ref().and_then(|p| p.job.as_ref()) else {
        return;
    };
    let mut msgs = Vec::new();
    let mut ended = false;
    loop {
        match job.rx.try_recv() {
            Ok(JobMsg::Done) => {
                ended = true;
                break;
            }
            Ok(m) => msgs.push(m),
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                ended = true;
                break;
            }
        }
    }
    if msgs.is_empty() && !ended {
        return;
    }
    let Some(mut p) = app.search.preview.take() else {
        return;
    };
    let (q, r) = (p.query.clone(), p.replacement.clone());
    // Files open in a tab are previewed here, from their buffers (few, and in memory).
    let mut tabs: Option<HashMap<PathBuf, usize>> = None;
    for m in msgs {
        match m {
            JobMsg::Planned(items) => {
                if let Some(j) = &mut p.job {
                    j.done += items.len();
                }
                for (i, c, in_tab, plan) in items {
                    let f = &mut p.files[i];
                    if in_tab {
                        let tabs = tabs.get_or_insert_with(|| open_tabs(app));
                        preview_tab(app, f, c, tabs, &q, &r);
                    } else {
                        match plan {
                            Some(fp) => {
                                f.hunks = fp.hunks.clone();
                                f.count = fp.replacements();
                                f.error = fp.error.clone();
                                f.plan = Some(fp);
                            }
                            None => f.error = Some("not previewed".to_string()),
                        }
                        f.target = Target::Disk { canon: c };
                    }
                }
            }
            JobMsg::Applied(items) => {
                if let Some(j) = &mut p.job {
                    j.done += items.len();
                }
                for (i, r) in items {
                    let f = &mut p.files[i];
                    f.result = Some(r.map(|()| f.count));
                }
            }
            JobMsg::Done => {}
        }
    }
    if ended && let Some(j) = p.job.take() {
        match j.kind {
            JobKind::Preview => {
                for f in &mut p.files {
                    if matches!(f.target, Target::Unknown) {
                        f.error = Some("the preview stopped before this file".to_string());
                    } else if f.error.is_none() && f.count == 0 {
                        f.error = Some("no match any more (changed since the search)".to_string());
                    }
                    f.checked = f.error.is_none();
                }
            }
            JobKind::Apply => {
                for &i in &j.sent {
                    let f = &mut p.files[i];
                    if f.result.is_none() {
                        f.result = Some(Err("the replace stopped before this file".into()));
                    }
                }
                p.applied = true;
            }
        }
        let applied = p.applied;
        app.search.preview = Some(p);
        if applied {
            report_apply(app);
        }
    } else {
        app.search.preview = Some(p);
    }
    app.dirty = true;
}

/// Previews a file that was open in a tab when the preview started, from its buffer.
fn preview_tab(
    app: &App,
    f: &mut PreviewFile,
    c: PathBuf,
    tabs: &HashMap<PathBuf, usize>,
    q: &Query,
    replacement: &str,
) {
    let Some(&i) = tabs.get(&c) else {
        f.error = Some("closed during the preview; not replaced (search again)".to_string());
        f.target = Target::Disk { canon: c };
        return;
    };
    let doc = &app.docs[i];
    let text = doc.buffer.text().clone();
    if doc.read_only {
        f.error = Some("open read-only (not UTF-8); not replaced".to_string());
    } else {
        match buffer_hunks(&text, q, replacement) {
            Ok((hunks, count)) => {
                f.hunks = hunks;
                f.count = count;
            }
            Err(e) => f.error = Some(e),
        }
    }
    f.target = Target::Buffer { canon: c, text };
}

/// True when `p` was built for what the fields show now.
fn is_current(s: &SearchPanel, p: &Preview) -> bool {
    p.query == s.query() && p.replacement == s.replace.text()
}

/// Drops the preview and stops its worker (a replace that runs is never dropped: callers
/// check [`Preview::applying`]).
fn drop_preview(app: &mut App) {
    if let Some(p) = app.search.preview.take()
        && let Some(j) = p.job
    {
        j.cancel.store(true, Ordering::SeqCst);
    }
    app.dirty = true;
}

/// Enter on the preview: Replace / Cancel for the checked files (Cancel is the default).
fn ask_confirm(app: &mut App) {
    let Some(p) = &app.search.preview else {
        return;
    };
    if p.applied {
        app.info("Replace in Project: done; Esc searches again");
        return;
    }
    if p.job.is_some() {
        app.info("Replace in Project: the preview is not ready yet (Esc cancels it)");
        return;
    }
    if !is_current(&app.search, p) {
        drop_preview(app);
        app.info("Replace in Project: the fields changed; Enter in the replace field previews");
        return;
    }
    let checked: Vec<&PreviewFile> = p
        .files
        .iter()
        .filter(|f| f.checked && f.error.is_none())
        .collect();
    if checked.is_empty() {
        app.info("Replace in Project: no file is checked");
        return;
    }
    let matches: usize = checked.iter().map(|f| f.count).sum();
    let in_tabs = checked
        .iter()
        .filter(|f| matches!(f.target, Target::Buffer { .. }))
        .count();
    let mut msg = format!(
        "Replace {matches} match{} in {} file{}?",
        if matches == 1 { "" } else { "es" },
        checked.len(),
        if checked.len() == 1 { "" } else { "s" }
    );
    if in_tabs > 0 {
        msg.push_str(&format!(
            " {in_tabs} open in tabs change in their buffers (undo there; not saved)."
        ));
    }
    msg.push_str(" A file changed since the preview is skipped.");
    // vr-search reports the match limit only as text ("... search stopped").
    if app
        .search
        .errors
        .iter()
        .any(|e| e.contains("search stopped"))
    {
        msg.push_str(
            " The search stopped at the match limit: this covers only part of the project.",
        );
    }
    let mut dialog = ConfirmDialog::new("Replace in Project", msg, &[REPLACE, CANCEL]);
    dialog.selected = 1;
    app.open_dialog(DialogWidget::Confirm(dialog), DialogAction::ReplaceInFiles);
}

/// The confirm dialog's Replace: applies the checked files of the preview. Files open in a
/// tab change in their buffers here, on the UI thread, and only when the buffer still holds
/// the previewed text; the other files are written by `vr_search::apply_replace` on a worker
/// thread, never one that is open in a tab now. Each file's result shows in the preview.
pub fn apply(app: &mut App) {
    let Some(mut p) = app.search.preview.take() else {
        return;
    };
    if p.applied || p.job.is_some() || !is_current(&app.search, &p) {
        app.search.preview = Some(p);
        return;
    }
    let tabs = open_tabs(app);
    let mut items: Vec<(usize, FilePreview)> = Vec::new();
    for (i, f) in p
        .files
        .iter_mut()
        .enumerate()
        .filter(|(_, f)| f.checked && f.error.is_none())
    {
        match &f.target {
            Target::Buffer { canon, text } => {
                f.result = Some(match tabs.get(canon) {
                    None => Err("closed since the preview; not replaced (search again)".into()),
                    Some(&ti) => {
                        let doc = &mut app.docs[ti];
                        if doc.buffer.text() != text {
                            Err("edited since the preview; not replaced (search again)".into())
                        } else if doc.read_only {
                            Err("open read-only; not replaced".into())
                        } else {
                            match vr_search::replace_all_tx(text, &p.query, &p.replacement) {
                                Ok(tx) => {
                                    let n = tx.changes.len();
                                    doc.buffer.break_undo_group();
                                    doc.buffer.apply(tx);
                                    doc.buffer.break_undo_group();
                                    p.in_tabs += 1;
                                    Ok(n)
                                }
                                Err(e) => Err(e.to_string()),
                            }
                        }
                    }
                });
            }
            Target::Disk { canon } => {
                if tabs.contains_key(canon) {
                    f.result = Some(Err(
                        "opened in a tab since the preview; not replaced (search again)".into(),
                    ));
                } else if let Some(plan) = f.plan.take() {
                    items.push((i, plan));
                } else {
                    f.result = Some(Err("not previewed; not replaced".into()));
                }
            }
            Target::Unknown => f.result = Some(Err("not previewed; not replaced".into())),
        }
    }
    if items.is_empty() {
        p.applied = true;
        app.search.preview = Some(p);
        report_apply(app);
        return;
    }
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    p.job = Some(Job {
        kind: JobKind::Apply,
        done: 0,
        total: items.len(),
        cancel: cancel.clone(),
        rx,
        sent: items.iter().map(|(i, _)| *i).collect(),
        handle: None,
    });
    let hold = app.search.hold.clone();
    // Joined only on exit (`shutdown`), like the preview's worker. A file it has not
    // reached when cancelled is reported as not replaced; one it started is finished (each
    // write is atomic).
    let handle = std::thread::spawn(move || {
        // apply_replace refuses a second path to the same file within one call; the
        // batches share this set so a link and its target in two batches are refused too.
        let mut seen: HashSet<PathBuf> = HashSet::new();
        let mut batches = items.chunks(JOB_BATCH);
        for batch in batches.by_ref() {
            if !proceed(&cancel, hold.as_deref()) {
                let rest = batch
                    .iter()
                    .map(|(i, _)| (*i, Err("cancelled; not replaced".into())));
                let _ = tx.send(JobMsg::Applied(rest.collect()));
                break;
            }
            let mut out = Vec::with_capacity(batch.len());
            let mut previews = Vec::with_capacity(batch.len());
            let mut idx = Vec::with_capacity(batch.len());
            for (i, fp) in batch {
                match std::fs::canonicalize(&fp.path) {
                    Ok(c) if seen.contains(&c) => {
                        out.push((*i, Err("same file as another result; replaced once".into())));
                    }
                    c => {
                        if let Ok(c) = c {
                            seen.insert(c);
                        }
                        previews.push(fp.clone());
                        idx.push(*i);
                    }
                }
            }
            let paths: Vec<PathBuf> = previews.iter().map(|fp| fp.path.clone()).collect();
            // One result per confirmed path, in order: zipped back by position.
            let results = vr_search::apply_replace(&previews, &paths);
            out.extend(
                idx.into_iter()
                    .zip(results)
                    .map(|(i, (_, r))| (i, r.map_err(|e| e.to_string()))),
            );
            if tx.send(JobMsg::Applied(out)).is_err() {
                return;
            }
        }
        let rest: Vec<(usize, Result<(), String>)> = batches
            .flatten()
            .map(|(i, _)| (*i, Err("cancelled; not replaced".into())))
            .collect();
        if !rest.is_empty() {
            let _ = tx.send(JobMsg::Applied(rest));
        }
        let _ = tx.send(JobMsg::Done);
    });
    if let Some(j) = &mut p.job {
        j.handle = Some(handle);
    }
    app.search.preview = Some(p);
    app.dirty = true;
}

/// Files named in the "not replaced" warning; the panel lists every one.
const NAMED_FAILURES: usize = 5;

/// The replace's summary: "Replaced …" and the files that were not replaced (a few named
/// with their reasons, the rest counted).
fn apply_summary(app: &App) -> (Option<String>, Option<String>) {
    let Some(p) = &app.search.preview else {
        return (None, None);
    };
    let (mut files, mut matches) = (0, 0);
    let mut failed = 0;
    let mut named = Vec::new();
    for f in &p.files {
        match &f.result {
            Some(Ok(n)) => {
                files += 1;
                matches += n;
            }
            Some(Err(e)) => {
                failed += 1;
                if named.len() < NAMED_FAILURES {
                    named.push(format!("{} ({e})", app.display_path(&f.path)));
                }
            }
            None => {}
        }
    }
    let done = (files > 0).then(|| {
        let mut msg = format!(
            "Replaced {matches} match{} in {files} file{}",
            if matches == 1 { "" } else { "es" },
            if files == 1 { "" } else { "s" }
        );
        if p.in_tabs > 0 {
            msg.push_str(&format!(" ({} in open tabs, not saved yet)", p.in_tabs));
        }
        msg
    });
    let not_done = (failed > 0).then(|| {
        let mut msg = format!(
            "Replace in Project: {failed} file{} not replaced: {}",
            if failed == 1 { "" } else { "s" },
            named.join("; ")
        );
        if failed > named.len() {
            msg.push_str(&format!(
                " and {} more (see the panel)",
                failed - named.len()
            ));
        }
        msg
    });
    (done, not_done)
}

/// The notices after a replace.
fn report_apply(app: &mut App) {
    let (done, not_done) = apply_summary(app);
    if let Some(m) = done {
        app.info(m);
    }
    if let Some(m) = not_done {
        app.warn(m);
    }
    app.dirty = true;
}

/// True while a project replace writes files.
pub fn replacing(app: &App) -> bool {
    app.search.preview.as_ref().is_some_and(Preview::applying)
}

/// Before the process exits (after a quit or a terminal error): a running worker is
/// cancelled and joined, so the batch in hand finishes and no file is cut off mid-write;
/// then every file's result is recorded, and a replace's summary is printed after the
/// terminal is restored.
pub fn shutdown(app: &mut App) {
    let Some(job) = app.search.preview.as_mut().and_then(|p| p.job.as_mut()) else {
        return;
    };
    let apply = job.kind == JobKind::Apply;
    job.cancel.store(true, Ordering::SeqCst);
    if let Some(h) = job.handle.take() {
        // A panicked worker is reported like one that stopped: its files say so.
        let _ = h.join();
    }
    drain_job(app);
    if apply {
        let (done, not_done) = apply_summary(app);
        app.exit_messages.extend(done.into_iter().chain(not_done));
    }
}

/// Moves the selected row of the list in view (results or preview) by `delta`.
fn move_selection(app: &mut App, delta: isize) {
    let page_rows = |n: usize| n.max(1) as isize - 1;
    if let Some(p) = &mut app.search.preview {
        let last = page_rows(p.rows().len());
        p.selected = (p.selected as isize + delta).clamp(0, last) as usize;
    } else {
        let last = page_rows(app.search.rows().len());
        app.search.selected = (app.search.selected as isize + delta).clamp(0, last) as usize;
    }
}

fn toggle_checked(app: &mut App) {
    let Some(p) = &mut app.search.preview else {
        return;
    };
    if p.applied || p.job.is_some() {
        return;
    }
    let rows = p.rows();
    if let Some(&row) = rows.get(p.selected) {
        let f = &mut p.files[Preview::file_of(row)];
        if f.error.is_none() {
            f.checked = !f.checked;
        }
    }
}

/// alt+c / alt+r / alt+w. Refused while a replace runs, so the toggles always show what
/// the results were searched with (a new search would be refused too).
fn toggle_option(app: &mut App, c: char) {
    if replacing(app) {
        app.info("Replace in Project: the replace is running; Esc cancels it");
        return;
    }
    let s = &mut app.search;
    match c {
        'c' => s.case_sensitive = !s.case_sensitive,
        'r' => s.regex = !s.regex,
        _ => s.whole_word = !s.whole_word,
    }
    fields_changed(app);
    if !app.search.input.text().is_empty() {
        start(app);
    }
}

fn next_field(app: &mut App, forward: bool) {
    let s = &mut app.search;
    let mut order = vec![Field::Query];
    if s.replace_mode {
        order.push(Field::Replace);
    }
    order.push(Field::Results);
    let i = order.iter().position(|f| *f == s.field).unwrap_or(0);
    let n = order.len();
    s.field = order[if forward {
        (i + 1) % n
    } else {
        (i + n - 1) % n
    }];
}

/// Esc: cancels a running replace (the files it has not reached are reported), closes the
/// preview (stopping it while it is built; after a replace the search runs again so the
/// results are current), otherwise gives the editor the keys; the panel stays.
fn escape(app: &mut App) {
    app.search.preview_pending = false;
    match &app.search.preview {
        Some(p) if p.applying() => {
            if let Some(j) = &p.job {
                j.cancel.store(true, Ordering::SeqCst);
            }
        }
        Some(p) => {
            let applied = p.applied;
            drop_preview(app);
            if applied {
                start(app);
            }
            app.search.field = Field::Results;
        }
        None => app.focus = Focus::Editor,
    }
}

/// Keys while the panel has focus; true when used.
pub fn handle_key(app: &mut App, key: &KeyEvent) -> bool {
    let bound = chord_from_key(key, app.keyboard_enhanced)
        .is_some_and(|c| app.keymap.command_for_in(Context::Global, &c).is_some());
    if bound {
        return false;
    }
    let m = key.modifiers;
    let plain = m.is_empty();
    let page = app.search.list_area.height.max(1) as isize;
    let in_list = app.search.field == Field::Results;
    match key.code {
        KeyCode::Esc if plain => escape(app),
        KeyCode::Char(c @ ('c' | 'r' | 'w')) if m == KeyModifiers::ALT => toggle_option(app, c),
        KeyCode::Tab | KeyCode::BackTab if m.difference(KeyModifiers::SHIFT).is_empty() => {
            let back = key.code == KeyCode::BackTab || m.contains(KeyModifiers::SHIFT);
            next_field(app, !back);
        }
        KeyCode::Enter if !plain => {}
        KeyCode::Enter => match app.search.field {
            Field::Query => start(app),
            Field::Replace => request_preview(app),
            Field::Results if app.search.preview.is_some() => ask_confirm(app),
            Field::Results => open_selected(app),
        },
        KeyCode::Down if !in_list => app.search.field = Field::Results,
        KeyCode::Up if in_list => move_selection(app, -1),
        KeyCode::Down => move_selection(app, 1),
        KeyCode::PageUp if in_list => move_selection(app, -page),
        KeyCode::PageDown if in_list => move_selection(app, page),
        KeyCode::Home if in_list => move_selection(app, isize::MIN / 2),
        KeyCode::End if in_list => move_selection(app, isize::MAX / 2),
        KeyCode::Char(' ') if in_list && app.search.preview.is_some() => toggle_checked(app),
        // Typing in the list does nothing (it never reaches the editor).
        _ if in_list => {}
        _ => {
            let s = &mut app.search;
            let input = match s.field {
                Field::Replace => &mut s.replace,
                _ => &mut s.input,
            };
            let before = input.text().to_string();
            if !input.handle_key(key) {
                return false;
            }
            if input.text() != before {
                fields_changed(app);
            }
        }
    }
    app.dirty = true;
    true
}

/// The query, the replacement or an option changed: a preview built for the old ones is
/// dropped, so it can never be confirmed (it would write a replacement the fields no longer
/// show). A replace that already runs keeps its preview: it shows that replace's results.
fn fields_changed(app: &mut App) {
    let s = &mut app.search;
    s.preview_pending = false;
    if s.preview.as_ref().is_some_and(|p| !p.applying()) {
        drop_preview(app);
    }
}

/// Clicks: a field takes the focus, a list row is selected (a result row also opens, the
/// panel keeps the keys), the wheel moves the selection.
pub fn handle_mouse(app: &mut App, ev: &MouseEvent) {
    let list = app.search.list_area;
    let inside = |r: Rect| {
        ev.column >= r.x && ev.column < r.x + r.width && ev.row >= r.y && ev.row < r.y + r.height
    };
    match ev.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            app.focus = Focus::Search;
            if inside(list) {
                app.search.field = Field::Results;
                let i = usize::from(ev.row - list.y);
                if let Some(p) = &mut app.search.preview {
                    let row = p.top + i;
                    if row < p.rows().len() {
                        p.selected = row;
                    }
                } else {
                    let row = app.search.top + i;
                    if row < app.search.rows().len() {
                        app.search.selected = row;
                        open_selected(app);
                        // The panel keeps the keys only while it is shown (a narrow screen hides it).
                        if app.search.visible {
                            app.focus = Focus::Search;
                        }
                    }
                }
            } else {
                let a = app.search.area;
                app.search.field = match ev.row.saturating_sub(a.y) {
                    2 if app.search.replace_mode => Field::Replace,
                    _ => Field::Query,
                };
            }
        }
        MouseEventKind::ScrollDown => move_selection(app, 1),
        MouseEventKind::ScrollUp => move_selection(app, -1),
        _ => {}
    }
    app.dirty = true;
}

/// Bracketed paste into the focused field.
pub fn paste(app: &mut App, text: &str) {
    let s = &mut app.search;
    match s.field {
        Field::Replace => s.replace.insert_str(text),
        Field::Query => s.input.insert_str(text),
        Field::Results => return,
    }
    if !text.is_empty() {
        fields_changed(app);
    }
    app.dirty = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str) -> PreviewFile {
        PreviewFile {
            path: PathBuf::from(name),
            hunks: Vec::new(),
            count: 1,
            error: None,
            checked: true,
            target: Target::Disk {
                canon: PathBuf::from(name),
            },
            result: None,
            plan: None,
        }
    }

    /// A replace worker that panics halfway: the files it reported keep their results, the
    /// others say the replace stopped before them, and the panel is free again (never stuck
    /// in "Replacing"). On exit, `shutdown` joins it without panicking itself.
    #[test]
    fn panicking_worker_is_reported_not_stuck() {
        for on_exit in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let (mut app, _) = crate::app_tests::test_app(dir.path());
            let (tx, rx) = mpsc::channel();
            let handle = std::thread::spawn(move || {
                tx.send(JobMsg::Applied(vec![(0, Ok(()))])).unwrap();
                panic!("worker bug (expected by the test)");
            });
            app.search.preview = Some(Preview {
                query: app.search.query(),
                replacement: String::new(),
                files: vec![file("a"), file("b"), file("c")],
                applied: false,
                selected: 0,
                top: 0,
                job: Some(Job {
                    kind: JobKind::Apply,
                    done: 0,
                    total: 3,
                    cancel: Arc::new(AtomicBool::new(false)),
                    rx,
                    sent: vec![0, 1, 2],
                    handle: Some(handle),
                }),
                in_tabs: 0,
            });
            if on_exit {
                shutdown(&mut app);
                let msgs = app.exit_messages.join(" | ");
                assert!(msgs.contains("2 files not replaced"), "{msgs}");
            } else {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
                while app.search.busy() {
                    assert!(std::time::Instant::now() < deadline, "stuck in Replacing");
                    drain(&mut app);
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            }
            let p = app.search.preview.as_ref().unwrap();
            assert!(p.applied && p.job.is_none());
            assert_eq!(p.files[0].result, Some(Ok(1)));
            for f in &p.files[1..] {
                let e = f.result.clone().unwrap().unwrap_err();
                assert!(e.contains("stopped before this file"), "{e}");
            }
        }
    }
}
