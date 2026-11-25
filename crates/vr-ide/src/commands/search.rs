//! `search.*` commands and the Project scope of the find box (AC-11, AC-12, T22; the box
//! itself is `commands::find`). Rendering: `ui::find_bar` and `ui::search_panel` (the list).
//! `visible` means the box is open on the project.
//!
//! The search runs on vr-search's background threads and streams batches into the box
//! ([`drain`], called by the event loop); a new search cancels the running one. The query is
//! the box's text in the find box syntax (`vr_search::Query::parse`); an invalid regex is
//! shown and nothing is searched.
//!
//! Project replace: Enter in the replace field shows a per-file preview (nothing written).
//! Space unchecks a file, Enter asks Replace / Cancel (Cancel is the default). A file open in
//! a tab is changed in its buffer as one transaction (undoable, not saved), never on disk
//! behind the tab; the other files are written by `vr_search::apply_replace`, which refuses a
//! file changed on disk since the preview. Each file's result is shown in the preview.
//!
//! Built-in keys while the box has the keys in Project scope (not in the keymap, like the
//! palette's): typing and line editing in the fields, Tab / shift+Tab move between the fields
//! and the list, Up/Down/PageUp/PageDown/Home/End move in the list (Down leaves a field for
//! the list), Enter searches / previews / opens the result, Space checks a preview file, Esc
//! closes a preview first, then the box (read mode). A key the keymap binds globally (ctrl+r:
//! project replace, ctrl+h, f1) runs its command first; so do the box's editor keys (ctrl+f
//! switches the scope, f3 / shift+f3 step through the results).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
    /// The workspace root's name, when the workspace has several roots.
    pub root: Option<String>,
}

/// One row of the results list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// The root of group `.0` (its name heads that root's files).
    Root(usize),
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
    /// Apply: the file the worker is on (set before it touches the file, cleared after its
    /// result is sent), so a worker blocked in I/O can be named.
    current: Arc<Mutex<Option<PathBuf>>>,
    /// Apply: files opened in a tab since the replace started are skipped.
    gate: ReplaceGate,
    /// When Esc cancelled the replace ("Cancelling…" since then).
    cancelled_at: Option<Instant>,
    /// The user confirmed quitting although the cancelled replace has not stopped.
    quit_anyway: bool,
}

impl Job {
    fn new(kind: JobKind, total: usize, cancel: Arc<AtomicBool>, rx: Receiver<JobMsg>) -> Job {
        Job {
            kind,
            done: 0,
            total,
            cancel,
            rx,
            sent: Vec::new(),
            current: Arc::new(Mutex::new(None)),
            gate: ReplaceGate::default(),
            cancelled_at: None,
            quit_anyway: false,
        }
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// The file the worker is on, if any.
    fn current_file(&self) -> Option<PathBuf> {
        self.current.lock().map_or(None, |c| c.clone())
    }
}

/// Hand-off between the UI thread and an apply worker for files opened in a tab while the
/// replace runs: such a file is never written. Both sides decide under one lock, and no I/O
/// happens under it, so the UI thread never waits on a write (a hung mount included).
#[derive(Debug, Default)]
struct Claims {
    /// Canonical paths of files a tab opened on since the replace started.
    opened: HashSet<PathBuf>,
    /// The file the worker is writing now.
    writing: Option<PathBuf>,
}

#[derive(Debug, Default, Clone)]
pub struct ReplaceGate(Arc<Mutex<Claims>>);

impl ReplaceGate {
    fn claims(&self) -> std::sync::MutexGuard<'_, Claims> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// UI thread, before a tab opens on `canon`: false while the worker writes that file
    /// (the tab would read a file being replaced); otherwise the worker skips it from now
    /// on. `None` when refused, `Some(true)` when this call added it.
    pub fn open(&self, canon: &Path) -> Option<bool> {
        let mut c = self.claims();
        if c.writing.as_deref() == Some(canon) {
            return None;
        }
        Some(c.opened.insert(canon.to_path_buf()))
    }

    /// UI thread: the tab announced with [`ReplaceGate::open`] did not open after all.
    pub fn open_failed(&self, canon: &Path) {
        self.claims().opened.remove(canon);
    }

    /// Worker, before writing `canon`: false when a tab opened on it.
    fn begin(&self, canon: &Path) -> bool {
        let mut c = self.claims();
        if c.opened.contains(canon) {
            return false;
        }
        c.writing = Some(canon.to_path_buf());
        true
    }

    /// Worker: the write of the file from [`ReplaceGate::begin`] is over.
    fn end(&self) {
        self.claims().writing = None;
    }
}

pub const OPENED_DURING_REPLACE: &str = "opened in a tab during the replace; not replaced";

/// An open refused for a moment because the replace is writing that file (shown as a
/// warning: it passes by itself).
#[derive(Debug)]
pub struct BeingReplaced(pub String);

impl std::fmt::Display for BeingReplaced {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BeingReplaced {}

/// Shows a failed open: the temporary replace refusal is a warning (it passes by itself),
/// anything else an error.
pub fn report_open_error(app: &mut App, e: &anyhow::Error) {
    if e.is::<BeingReplaced>() {
        app.warn(e.to_string());
    } else {
        app.error(e.to_string());
    }
}

/// Before a tab opens on `path`: while a replace runs, the worker is told to leave the file
/// alone; refused (with the message) while the worker is writing that very file. Returns
/// the gate and canonical path to pass to [`ReplaceGate::open_failed`] if the open fails.
pub fn before_tab_opens(app: &App, path: &Path) -> Result<Option<(ReplaceGate, PathBuf)>, String> {
    let Some(job) = app
        .search
        .preview
        .as_ref()
        .and_then(|p| p.job.as_ref())
        .filter(|j| j.kind == JobKind::Apply)
    else {
        return Ok(None);
    };
    let c = canon(path);
    match job.gate.open(&c) {
        Some(true) => Ok(Some((job.gate.clone(), c))),
        Some(false) => Ok(None),
        None => Err(format!(
            "{} is being written by Replace in Project right now; try again in a moment",
            app.display_path(path)
        )),
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
    /// The file and char range of the result last opened from the list: that selection
    /// does not seed the query (the user did not choose it).
    pub opened: Option<(PathBuf, usize, usize)>,
    /// Panel and list areas in the last frame.
    pub area: Rect,
    pub list_area: Rect,
}

impl SearchPanel {
    /// The query of the box's text (the find box syntax, `vr_search::Query::parse`).
    pub fn query(&self) -> Query {
        Query::parse(self.input.text())
    }

    /// A preview is being built or a replace runs.
    pub fn busy(&self) -> bool {
        self.preview.as_ref().is_some_and(|p| p.job.is_some())
    }

    pub fn running(&self) -> bool {
        self.run.is_some()
    }

    /// `rows().len()` without building the rows.
    pub fn row_count(&self) -> usize {
        let roots = self
            .groups
            .iter()
            .enumerate()
            .filter(|(i, g)| g.root.is_some() && (*i == 0 || self.groups[i - 1].root != g.root))
            .count();
        roots + self.groups.len() + self.total
    }

    /// Results rows: each file followed by its matches; with several workspace roots, a root
    /// row before the first file of each root (results come root by root).
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::with_capacity(self.groups.len() + self.total);
        let mut root = None;
        for (gi, g) in self.groups.iter().enumerate() {
            if g.root.is_some() && g.root != root {
                root.clone_from(&g.root);
                rows.push(Row::Root(gi));
            }
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
    // An invalid regex is shown in the box; nothing is searched.
    if let Err(e) = q.check() {
        cancel_running(app);
        drop_preview(app);
        let s = &mut app.search;
        s.groups.clear();
        s.index.clear();
        s.total = 0;
        s.errors = vec![e.to_string()];
        s.status = Status::Idle;
        s.query = Some(q);
        app.dirty = true;
        return;
    }
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    // Detached: the walk ends when it is done, cancelled or its receiver is dropped.
    // Every root, one after the other: results come grouped by root, then by file.
    drop(vr_search::search_roots(
        &app.workspace.paths(),
        &q,
        cancel.clone(),
        tx,
    ));
    attach(app, rx, cancel);
    app.search.query = Some(q);
}

/// The workspace roots changed: shown results (and a running search) are for the old roots,
/// so the same query runs again over the new ones; a replace preview built for the old
/// roots is dropped with them. A replace that is writing files keeps going (its files were
/// confirmed), and its results stay until it is done.
pub fn roots_changed(app: &mut App) {
    if app.search.preview.as_ref().is_some_and(Preview::applying) {
        return;
    }
    let shown = app.search.query.clone();
    if shown.is_some() && shown == Some(app.search.query()) {
        start(app);
    }
}

/// Another workspace opened (W-2): results that [`roots_changed`] did not search again
/// (the query field was edited since) belong to the old roots; they are dropped, with a
/// search still running over them. A replace never runs here (Open Workspace waits for it).
pub fn clear_stale(app: &mut App) {
    if app.search.busy() {
        return;
    }
    let shown = app.search.query.clone();
    if shown.is_some() && shown == Some(app.search.query()) {
        return;
    }
    cancel_running(app);
    drop_preview(app);
    let s = &mut app.search;
    s.groups.clear();
    s.index.clear();
    s.total = 0;
    s.errors.clear();
    s.selected = 0;
    s.top = 0;
    s.status = Status::Idle;
    s.query = None;
    app.dirty = true;
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
    let ws = &app.workspace;
    let s = &mut app.search;
    for m in msgs {
        match m {
            SearchMsg::Batch(batch) => {
                for pm in batch {
                    let gi = match s.index.get(&pm.path) {
                        Some(&gi) => gi,
                        None => {
                            let root = ws
                                .is_multi()
                                .then(|| ws.root_of(&pm.path).map(|r| r.name.clone()))
                                .flatten();
                            s.groups.push(FileGroup {
                                path: pm.path.clone(),
                                matches: Vec::new(),
                                root,
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
    // The result the box opened is selected: the box's query found it, so it stays.
    if let (Some((path, from, to)), Some(p)) = (&app.search.opened, &doc.path)
        && path == p
        && (*from, *to) == (r.from(), r.to())
    {
        return None;
    }
    let s = doc.buffer.text().slice(r.from()..r.to()).to_string();
    (!s.contains(['\n', '\r'])).then_some(s)
}

/// Shows the project scope of the find box (`find::open_project`) and gives it the keys. A
/// selection in the editor becomes the query, typed so that it searches literally.
pub(crate) fn show(app: &mut App, replace: bool) {
    // A running replace is shown as it is: its fields stay what it was confirmed with.
    if replacing(app) {
        app.search.visible = true;
        app.focus = Focus::Search;
        app.dirty = true;
        return;
    }
    if let Some(seed) = selection_seed(app).and_then(|s| vr_search::literal_syntax(&s)) {
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
    super::find::open_project(app, false);
    Ok(())
}

fn replace_project(app: &mut App, _: CmdArgs) -> CmdResult {
    super::find::open_project(app, true);
    Ok(())
}

/// Opens the file of match `mi` of group `gi` and selects the match (file:line:col); false
/// when nothing was opened.
fn open_match(app: &mut App, gi: usize, mi: usize) -> bool {
    let Some(m) = app
        .search
        .groups
        .get(gi)
        .and_then(|g| g.matches.get(mi))
        .cloned()
    else {
        return false;
    };
    if let Err(e) = app.open_path(&m.path) {
        report_open_error(app, &e);
        return false;
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
        app.search.opened = doc.path.clone().map(|p| (p, from, to));
    }
    app.cursor_moved();
    true
}

/// Enter on a result row: opens it (a file or root row opens its first match) and gives the
/// editor the keys in read mode; the find box stays open on the project.
fn open_selected(app: &mut App) {
    let rows = app.search.rows();
    let Some(&row) = rows.get(app.search.selected) else {
        return;
    };
    let (gi, mi) = match row {
        Row::Match(g, m) => (g, m),
        Row::File(g) | Row::Root(g) => (g, 0),
    };
    if open_match(app, gi, mi) {
        focus_editor(app);
    }
}

/// Gives the editor the keys (the find box stays open above it).
pub fn focus_editor(app: &mut App) {
    app.focus = Focus::Editor;
    app.dirty = true;
}

/// search.next_result / search.prev_result (palette), and f3 / shift+f3 while the find box
/// is on the project: the next (previous) result after the selected row, wrapping; opens it.
pub(crate) fn step_result(app: &mut App, forward: bool) {
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
    if let Err(e) = q.check() {
        app.search.errors = vec![e.to_string()];
        app.dirty = true;
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
    let job = Job::new(JobKind::Preview, paths.len(), cancel.clone(), rx);
    let (roots, hold) = (app.workspace.paths(), app.search.hold.clone());
    let (wq, wr) = (q.clone(), replacement.clone());
    // Never joined: it writes nothing, and stops at the next batch once cancelled or once
    // the receiver is gone; exit does not wait for it.
    std::thread::spawn(move || {
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
            // Each file is confined to its own root.
            let mut plans = vr_search::plan_replace_roots(&roots, &wq, &wr, &disk).into_iter();
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
    });
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
    drain_job_until(app, None);
}

/// [`drain_job`]; with a `deadline`, waits until the worker ends or the deadline passes.
fn drain_job_until(app: &mut App, deadline: Option<Instant>) {
    let Some(job) = app.search.preview.as_ref().and_then(|p| p.job.as_ref()) else {
        return;
    };
    let mut msgs = Vec::new();
    let mut ended = false;
    loop {
        // Err(true): the worker is gone (also after a panic); Err(false): nothing yet.
        let next = match deadline {
            None => job
                .rx
                .try_recv()
                .map_err(|e| e == TryRecvError::Disconnected),
            Some(d) => job
                .rx
                .recv_timeout(d.saturating_duration_since(Instant::now()))
                .map_err(|e| e == RecvTimeoutError::Disconnected),
        };
        match next {
            Ok(JobMsg::Done) | Err(true) => {
                ended = true;
                break;
            }
            Ok(m) => msgs.push(m),
            Err(false) => break,
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
                REPLACE_RUNNING.store(false, Ordering::SeqCst);
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

const MOVED_WHILE_SEARCHING: &str =
    "files were moved in the explorer while this search ran; search again for their new paths";

/// `p` with its `from` prefix replaced by `to` (`from` itself becomes `to`), if under it.
fn moved_path(p: &Path, from: &Path, to: &Path) -> Option<PathBuf> {
    let rest = p.strip_prefix(from).ok()?;
    Some(if rest.as_os_str().is_empty() {
        to.to_path_buf()
    } else {
        to.join(rest)
    })
}

/// The explorer renamed or moved `from` (a file or a folder) to `to`: the results follow
/// it. A preview that is not applying holds the old paths and the versions read there; it
/// is dropped (with a notice) rather than confirmed for files that moved. A running replace
/// keeps its preview: its worker reports a moved file as not found.
pub fn paths_moved(app: &mut App, from: &Path, to: &Path) {
    let s = &mut app.search;
    let mut changed = false;
    for g in &mut s.groups {
        if let Some(n) = moved_path(&g.path, from, to) {
            g.path = n;
            changed = true;
        }
        for m in &mut g.matches {
            if let Some(n) = moved_path(&m.path, from, to) {
                m.path = n;
            }
        }
    }
    if changed {
        s.index = s
            .groups
            .iter()
            .enumerate()
            .map(|(i, g)| (g.path.clone(), i))
            .collect();
    }
    // The running walk may still send the old paths (it read them before the move).
    if s.running() && !s.errors.iter().any(|e| e == MOVED_WHILE_SEARCHING) {
        s.errors.push(MOVED_WHILE_SEARCHING.to_string());
    }
    let stale = s.preview.as_ref().is_some_and(|p| {
        !p.applying() && !p.applied && p.files.iter().any(|f| f.path.starts_with(from))
    });
    if stale {
        drop_preview(app);
        app.search.field = Field::Results;
        app.info(
            "Replace in Project: files moved since the preview; Enter in the replace field \
             previews again",
        );
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
/// thread, never one that is open in a tab: at the confirm, or opened since ([`ReplaceGate`]).
/// Each file's result shows in the preview.
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
                                    app.syntax.reset(ti);
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
    let mut job = Job::new(JobKind::Apply, items.len(), cancel.clone(), rx);
    job.sent = items.iter().map(|(i, _)| *i).collect();
    let current = job.current.clone();
    let gate = job.gate.clone();
    p.job = Some(job);
    let hold = app.search.hold.clone();
    // Never joined while the app runs. A file it has not reached when cancelled is reported
    // as not replaced; the batch in hand is finished (each write is atomic). On exit
    // `shutdown` waits for it, but only up to a bound: a file on a hung mount can block it
    // in the kernel, where no cancel check is reached.
    std::thread::spawn(move || {
        // apply_replace refuses a second path to the same file within one call; this set
        // also covers a link and its target in two calls.
        let mut seen: HashSet<PathBuf> = HashSet::new();
        let set_current = |f: Option<&Path>| {
            if let Ok(mut c) = current.lock() {
                *c = f.map(Path::to_path_buf);
            }
        };
        let mut batches = items.chunks(JOB_BATCH);
        for batch in batches.by_ref() {
            if !proceed(&cancel, hold.as_deref()) {
                let rest = batch
                    .iter()
                    .map(|(i, _)| (*i, Err("cancelled; not replaced".into())));
                let _ = tx.send(JobMsg::Applied(rest.collect()));
                break;
            }
            // One file per call and per message, so the UI knows each result as soon as it
            // is on disk and, if the worker hangs, exactly which file it is on.
            for (i, fp) in batch {
                set_current(Some(&fp.path));
                let c = std::fs::canonicalize(&fp.path);
                let key = c.as_ref().map_or_else(|_| fp.path.clone(), Clone::clone);
                let r = match c {
                    Ok(c) if seen.contains(&c) => {
                        Err("same file as another result; replaced once".into())
                    }
                    _ if !gate.begin(&key) => Err(OPENED_DURING_REPLACE.into()),
                    c => {
                        if let Ok(c) = c {
                            seen.insert(c);
                        }
                        let path = [fp.path.clone()];
                        let r = vr_search::apply_replace(std::slice::from_ref(fp), &path)
                            .pop()
                            .map_or(Err("no result".into()), |(_, r)| {
                                r.map_err(|e| e.to_string())
                            });
                        gate.end();
                        r
                    }
                };
                let sent = tx.send(JobMsg::Applied(vec![(*i, r)]));
                set_current(None);
                if sent.is_err() {
                    return;
                }
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
    REPLACE_RUNNING.store(true, Ordering::SeqCst);
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
                " and {} more (see the find box)",
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

/// How long "Cancelling…" may last before quit is offered anyway (with a confirm).
pub const CANCEL_WAIT: Duration = Duration::from_secs(5);

/// Longest wait on exit for an apply worker to finish the batch in hand.
pub const EXIT_WAIT: Duration = Duration::from_secs(5);

/// Set while an apply worker runs, so the panic hook can say a replace was interrupted.
pub static REPLACE_RUNNING: AtomicBool = AtomicBool::new(false);

/// Why quit cannot go ahead at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuitBlock {
    /// A replace is running, or was cancelled less than [`CANCEL_WAIT`] ago: refused.
    Replacing,
    /// The cancelled replace has not stopped for [`CANCEL_WAIT`]: its worker is probably
    /// blocked in I/O on this file (if known). Quit needs a confirm.
    Stuck(Option<PathBuf>),
}

/// What a running project replace means for quit (`None`: nothing in the way).
pub fn quit_block(app: &App) -> Option<QuitBlock> {
    let job = app.search.preview.as_ref()?.job.as_ref()?;
    if job.kind != JobKind::Apply || job.quit_anyway {
        return None;
    }
    match job.cancelled_at {
        Some(t) if app.now().saturating_duration_since(t) >= CANCEL_WAIT => {
            Some(QuitBlock::Stuck(job.current_file()))
        }
        _ => Some(QuitBlock::Replacing),
    }
}

/// The confirm for [`QuitBlock::Stuck`].
pub fn ask_quit_stuck(app: &mut App, file: Option<&Path>) {
    ask_quit_stuck_for(app, file, DialogAction::QuitStuckReplace);
}

/// [`ask_quit_stuck`] answered by `action` (`/q!` keeps its discard through the question).
pub fn ask_quit_stuck_for(app: &mut App, file: Option<&Path>, action: DialogAction) {
    let on = file.map_or_else(
        || "in a file operation".to_string(),
        |f| format!("on {}", app.display_path(f)),
    );
    let message = format!(
        "The cancelled replace has not stopped after {} s; it is probably blocked {on} \
         (a hung disk or network mount). Quit anyway leaves that file's state unknown; \
         files not reached are not replaced.",
        CANCEL_WAIT.as_secs()
    );
    let mut confirm = ConfirmDialog::new("Quit", message, &[QUIT_ANYWAY, super::file::CANCEL]);
    // Quitting is irreversible here: Enter alone must not do it.
    confirm.selected = 1;
    app.open_dialog(DialogWidget::Confirm(confirm), action);
}

pub const QUIT_ANYWAY: &str = "Quit anyway";

/// "Quit anyway" chosen: quit no longer waits for this replace (exit still waits up to
/// [`EXIT_WAIT`] and names the file).
pub fn allow_quit(app: &mut App) {
    if let Some(j) = app.search.preview.as_mut().and_then(|p| p.job.as_mut()) {
        j.quit_anyway = true;
    }
}

/// Before the process exits (after a quit or a terminal error), with the terminal already
/// restored and recovery written: see [`shutdown_within`] with [`EXIT_WAIT`].
pub fn shutdown(app: &mut App) {
    shutdown_within(app, EXIT_WAIT);
}

/// A preview worker writes nothing: it is cancelled and left alone. An apply worker is
/// cancelled and given up to `bound` to finish the batch in hand (each write is atomic), so
/// every file gets its result. A worker blocked in I/O (a hung mount) never reaches its
/// cancel check: after `bound` the exit goes on, and the message names the file it was on
/// and the files whose state is unknown. The replace's summary goes to `exit_messages`.
pub fn shutdown_within(app: &mut App, bound: Duration) {
    let Some(p) = app.search.preview.as_mut() else {
        return;
    };
    let Some(job) = p.job.as_mut() else {
        return;
    };
    job.cancel.store(true, Ordering::SeqCst);
    if job.kind == JobKind::Preview {
        p.job = None;
        return;
    }
    drain_job_until(app, Some(Instant::now() + bound));
    let mut stuck = None;
    if let Some(p) = app.search.preview.as_mut()
        && let Some(job) = p.job.take()
    {
        REPLACE_RUNNING.store(false, Ordering::SeqCst);
        p.applied = true;
        let unknown: Vec<PathBuf> = job
            .sent
            .iter()
            .filter(|&&i| p.files[i].result.is_none())
            .map(|&i| p.files[i].path.clone())
            .collect();
        stuck = Some((job.current_file(), unknown));
    }
    let (done, not_done) = apply_summary(app);
    app.exit_messages.extend(done.into_iter().chain(not_done));
    if let Some((current, unknown)) = stuck {
        let mut msg = match current {
            Some(f) => format!(
                "Replace in Project: the replace was still writing {} when vr-ide quit \
                 (blocked for more than {} s); check that file",
                app.display_path(&f),
                bound.as_secs_f32()
            ),
            None => "Replace in Project: the replace had not stopped when vr-ide quit".into(),
        };
        let named: Vec<String> = unknown
            .iter()
            .take(NAMED_FAILURES)
            .map(|f| app.display_path(f))
            .collect();
        if !named.is_empty() {
            msg.push_str(&format!(". File state unknown: {}", named.join(", ")));
        }
        if unknown.len() > named.len() {
            msg.push_str(&format!(" and {} more", unknown.len() - named.len()));
        }
        app.exit_messages.push(msg);
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
/// results are current), otherwise closes the find box: the editor gets the keys in read
/// mode.
fn escape(app: &mut App) {
    app.search.preview_pending = false;
    let now = app.now();
    match &mut app.search.preview {
        Some(p) if p.applying() => {
            if let Some(j) = &mut p.job {
                j.cancel.store(true, Ordering::SeqCst);
                // A second Esc keeps the first time: the cancel bound counts from it.
                j.cancelled_at.get_or_insert(now);
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
        None => super::find::close_box(app),
    }
}

/// The find box commands whose keys the editor table holds (ctrl+f switches the scope, f3 /
/// shift+f3 step through the results): they work in the box on the project too.
const BOX_COMMANDS: &[&str] = &["find.open", "find.next", "find.prev"];

/// Keys while the box has the keys in Project scope; true when used.
pub fn handle_key(app: &mut App, key: &KeyEvent) -> bool {
    let chord = chord_from_key(key, app.keyboard_enhanced);
    let box_cmd = chord
        .as_ref()
        .and_then(|c| app.keymap.command_for_in(Context::Editor, c))
        .filter(|c| BOX_COMMANDS.contains(c))
        .map(str::to_owned);
    if let Some(cmd) = box_cmd {
        app.run_command(&cmd, CmdArgs::None);
        return true;
    }
    let bound = chord.is_some_and(|c| app.keymap.command_for_in(Context::Global, &c).is_some());
    if bound {
        return false;
    }
    let m = key.modifiers;
    let plain = m.is_empty();
    let page = app.search.list_area.height.max(1) as isize;
    let in_list = app.search.field == Field::Results;
    match key.code {
        KeyCode::Esc if plain => escape(app),
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
/// The open file's highlights follow the query.
pub(crate) fn fields_changed(app: &mut App) {
    let s = &mut app.search;
    s.preview_pending = false;
    if s.preview.as_ref().is_some_and(|p| !p.applying()) {
        drop_preview(app);
    }
    super::find::mirror_project_fields(app);
}

/// Clicks: a field takes the focus, a list row is selected (a result row also opens, the
/// box keeps the keys), the wheel moves the selection.
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
                    1 if app.search.replace_mode => Field::Replace,
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
pub(crate) mod tests {
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
    /// in "Replacing"). On exit, `shutdown` takes the disconnect without panicking itself.
    #[test]
    fn panicking_worker_is_reported_not_stuck() {
        for on_exit in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let (mut app, _) = crate::app_tests::test_app(dir.path());
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                tx.send(JobMsg::Applied(vec![(0, Ok(()))])).unwrap();
                panic!("worker bug (expected by the test)");
            });
            install_job(&mut app, JobKind::Apply, rx);
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

    /// A preview of files a, b, c whose worker talks through `rx`.
    fn install_job(app: &mut App, kind: JobKind, rx: Receiver<JobMsg>) {
        let mut job = Job::new(kind, 3, Arc::new(AtomicBool::new(false)), rx);
        if kind == JobKind::Apply {
            job.sent = vec![0, 1, 2];
        }
        app.search.preview = Some(Preview {
            query: app.search.query(),
            replacement: String::new(),
            files: vec![file("a"), file("b"), file("c")],
            applied: false,
            selected: 0,
            top: 0,
            job: Some(job),
            in_tabs: 0,
        });
    }

    /// A worker blocked inside a file (an open, read or write that never returns, as on a
    /// hung network mount): an apply worker reports a, then hangs on b, ignoring `cancel`.
    /// It is released 3 s later, so a caller that waits for it shows up as slow, not hung.
    pub(crate) fn stuck_job(app: &mut App, kind: JobKind) {
        let (tx, rx) = mpsc::channel();
        install_job(app, kind, rx);
        if kind == JobKind::Apply {
            let job = app.search.preview.as_ref().unwrap().job.as_ref().unwrap();
            *job.current.lock().unwrap() = Some(PathBuf::from("b"));
        }
        std::thread::spawn(move || {
            if kind == JobKind::Apply {
                tx.send(JobMsg::Applied(vec![(0, Ok(()))])).unwrap();
            }
            std::thread::sleep(Duration::from_secs(3));
            drop(tx);
        });
    }

    /// Exit with an apply worker that never returns: `shutdown` gives up after the bound,
    /// names the file that may be mid-write and the files whose state is unknown, and still
    /// reports what was replaced.
    #[test]
    fn shutdown_gives_up_on_a_held_apply_worker_and_names_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, _) = crate::app_tests::test_app(dir.path());
        stuck_job(&mut app, JobKind::Apply);
        let t = Instant::now();
        shutdown_within(&mut app, Duration::from_millis(300));
        let took = t.elapsed();
        assert!(took < Duration::from_secs(2), "shutdown waited {took:?}");
        assert!(
            took >= Duration::from_millis(250),
            "gave up at once: {took:?}"
        );
        let msgs = app.exit_messages.join(" | ");
        assert!(msgs.contains("Replaced 1 match in 1 file"), "{msgs}");
        assert!(msgs.contains("still writing b"), "{msgs}");
        assert!(msgs.contains("state unknown: b, c"), "{msgs}");
        assert!(!app.search.busy(), "job still attached");
    }

    /// "Preparing preview" with a worker blocked in a read: Esc drops the preview at once
    /// and the panel is free for a new search (the blocked thread is left behind).
    #[test]
    fn esc_drops_a_stuck_preview_at_once() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, _) = crate::app_tests::test_app(dir.path());
        stuck_job(&mut app, JobKind::Preview);
        app.search.visible = true;
        app.focus = Focus::Search;
        let t = Instant::now();
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(
            t.elapsed() < Duration::from_millis(500),
            "{:?}",
            t.elapsed()
        );
        assert!(app.search.preview.is_none() && !app.search.busy());
        assert_eq!(quit_block(&app), None);
    }

    /// A preview writes nothing: exit cancels it and never waits for its worker.
    #[test]
    fn a_preview_job_never_delays_shutdown() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, _) = crate::app_tests::test_app(dir.path());
        stuck_job(&mut app, JobKind::Preview);
        let t = Instant::now();
        shutdown_within(&mut app, Duration::from_secs(5));
        let took = t.elapsed();
        assert!(
            took < Duration::from_millis(500),
            "shutdown waited {took:?}"
        );
        assert!(app.exit_messages.is_empty(), "{:?}", app.exit_messages);
        assert!(!app.search.busy(), "preview job still attached");
    }

    /// "Cancelling…" that never ends (the worker is blocked in I/O): quit stays refused
    /// until the cancel bound has passed, then asks, naming the stuck file; Cancel keeps the
    /// app, "Quit anyway" quits.
    #[test]
    fn quit_allowed_after_the_cancel_bound_with_a_confirm() {
        use crate::commands::dialog::{self, DialogAction};
        use crate::ui::dialog::Outcome;
        let dir = tempfile::tempdir().unwrap();
        let (mut app, _) = crate::app_tests::test_app(dir.path());
        stuck_job(&mut app, JobKind::Apply);
        app.search.visible = true;
        app.focus = Focus::Search;
        let t0 = Instant::now();
        drain(&mut app);
        app.handle_key_at(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), t0);
        assert!(replacing(&app), "Esc stops a stuck worker at once?");
        let ctrl_q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL);
        app.handle_key_at(ctrl_q, t0 + Duration::from_secs(1));
        assert!(!app.quit && app.dialog.is_none(), "quit before the bound");
        let later = t0 + CANCEL_WAIT + Duration::from_secs(1);
        for (button, quits) in [(1, false), (0, true)] {
            app.handle_key_at(ctrl_q, later);
            let d = app.dialog.take().expect("a confirm after the cancel bound");
            assert!(matches!(d.action, DialogAction::QuitStuckReplace));
            let DialogWidget::Confirm(c) = &d.widget else {
                panic!("confirm expected")
            };
            assert!(c.message.contains("blocked on b"), "{}", c.message);
            assert_eq!(c.buttons, ["Quit anyway", "Cancel"]);
            assert_eq!(c.selected, 1, "Cancel is the default button");
            dialog::answer(&mut app, d, Outcome::Chosen(button));
            assert_eq!(app.quit, quits, "button {button}");
        }
    }
}

#[cfg(test)]
mod gate_tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// Racing test: a worker writing files and a UI thread opening tabs on the same
    /// files at the same time. A file is never written after (or while) a tab opened on it:
    /// every file both written and opened was opened only after its write was over.
    #[test]
    fn a_tab_never_opens_on_a_file_the_worker_writes() {
        const N: usize = 4000;
        for _ in 0..5 {
            let gate = ReplaceGate::default();
            let clock = Arc::new(AtomicUsize::new(1));
            let files: Vec<PathBuf> = (0..N).map(|i| PathBuf::from(format!("/p/f{i}"))).collect();
            let (wg, wc, wf) = (gate.clone(), clock.clone(), files.clone());
            let worker = std::thread::spawn(move || {
                let mut writes = vec![None; N];
                for (i, f) in wf.iter().enumerate() {
                    if wg.begin(f) {
                        let s = wc.fetch_add(1, Ordering::SeqCst);
                        std::hint::spin_loop();
                        let e = wc.fetch_add(1, Ordering::SeqCst);
                        wg.end();
                        writes[i] = Some((s, e));
                    }
                }
                writes
            });
            let mut opened = vec![None; N];
            let mut refused = 0;
            // Opens run from both ends towards the worker, so they cross it.
            for i in (0..N).rev().step_by(2).chain((0..N).step_by(3)) {
                match gate.open(&files[i]) {
                    Some(_) => {
                        opened[i].get_or_insert(clock.fetch_add(1, Ordering::SeqCst));
                    }
                    None => refused += 1,
                }
            }
            let writes = worker.join().unwrap();
            for i in 0..N {
                if let (Some((_, e)), Some(t)) = (writes[i], opened[i]) {
                    assert!(t > e, "file {i}: opened at {t}, written until {e}");
                }
            }
            // Refusals happen only while that file is being written; they are rare.
            assert!(refused < N, "{refused}");
        }
    }

    /// While the worker writes a file, a tab cannot open on it (the user is told to try
    /// again); once the write is over it opens, and from then on the worker skips it.
    #[test]
    fn opening_the_file_being_written_is_refused_then_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.txt");
        std::fs::write(&f, "foo\n").unwrap();
        let (mut app, _) = crate::app_tests::test_app(dir.path());
        let (_tx, rx) = mpsc::channel();
        let job = Job::new(JobKind::Apply, 1, Arc::new(AtomicBool::new(false)), rx);
        let gate = job.gate.clone();
        app.search.preview = Some(Preview {
            query: app.search.query(),
            replacement: String::new(),
            files: Vec::new(),
            applied: false,
            selected: 0,
            top: 0,
            job: Some(job),
            in_tabs: 0,
        });
        let c = std::fs::canonicalize(&f).unwrap();
        assert!(gate.begin(&c));
        let e = app.open_path(&f).unwrap_err().to_string();
        assert!(e.contains("being written by Replace in Project"), "{e}");
        assert!(app.docs.is_empty());
        gate.end();
        app.open_path(&f).unwrap();
        assert_eq!(app.docs.len(), 1);
        assert!(!gate.begin(&c), "the worker writes a file open in a tab");
        // A tab that fails to open does not keep its file from being replaced.
        let missing = dir.path().join("gone.txt");
        assert!(app.open_path(&missing).is_err());
        assert!(gate.begin(&missing));
    }

    /// FR-8: the temporary refusal is one warning on every open path (explorer, search
    /// panel) and its wording fits saving too.
    #[test]
    fn being_replaced_refusal_is_a_warning_on_every_path() {
        use crate::app::NoticeLevel;
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.txt");
        std::fs::write(&f, "foo\n").unwrap();
        let (mut app, _) = crate::app_tests::test_app(dir.path());
        app.explorer.visible = true;
        crate::commands::explorer::ensure_loaded(&mut app);
        let (_tx, rx) = mpsc::channel();
        let job = Job::new(JobKind::Apply, 1, Arc::new(AtomicBool::new(false)), rx);
        let gate = job.gate.clone();
        app.search.preview = Some(Preview {
            query: app.search.query(),
            replacement: String::new(),
            files: Vec::new(),
            applied: false,
            selected: 0,
            top: 0,
            job: Some(job),
            in_tabs: 0,
        });
        assert!(gate.begin(&std::fs::canonicalize(&f).unwrap()));
        let i = app.explorer.rows.iter().position(|r| r.path == f).unwrap();
        app.explorer.cursor = i;
        app.focus = crate::app::Focus::Explorer;
        app.handle_key(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ));
        let n = app.notices.back().expect("a notice");
        assert!(n.text.contains("being written"), "{}", n.text);
        assert_eq!(n.level, NoticeLevel::Warning, "{}", n.text);
        assert!(!n.text.contains("open it again"), "{}", n.text);
        assert!(app.docs.is_empty());
        app.notices.clear();
        app.run_command("file.open", crate::commands::CmdArgs::Path(f.clone()));
        let n = app.notices.back().expect("a notice");
        assert_eq!(n.level, NoticeLevel::Warning, "{}", n.text);
        assert!(!n.text.starts_with("Open File"), "{}", n.text);
    }
}

#[cfg(test)]
mod refused_open_tests {
    use super::*;

    /// Opening a result whose file the replace is writing right now is refused as a
    /// warning; the box keeps the keys and stays shown.
    #[test]
    fn refused_open_keeps_the_panel_and_its_keys() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("a.txt");
        std::fs::write(&f, "foo\n").unwrap();
        let (mut app, _) = crate::app_tests::test_app(dir.path());
        let (_tx, rx) = mpsc::channel();
        let job = Job::new(JobKind::Apply, 1, Arc::new(AtomicBool::new(false)), rx);
        let gate = job.gate.clone();
        app.search.preview = Some(Preview {
            query: app.search.query(),
            replacement: String::new(),
            files: Vec::new(),
            applied: false,
            selected: 0,
            top: 0,
            job: Some(job),
            in_tabs: 0,
        });
        app.search.groups.push(FileGroup {
            path: f.clone(),
            matches: vec![ProjectMatch {
                path: f.clone(),
                line: 1,
                col: 1,
                line_text: "foo".into(),
                match_range: (0, 3),
            }],
            root: None,
        });
        app.search.visible = true;
        app.search.selected = 1;
        app.focus = Focus::Search;
        assert!(gate.begin(&std::fs::canonicalize(&f).unwrap()));
        open_selected(&mut app);
        assert!(app.docs.is_empty());
        assert_eq!(app.focus, Focus::Search);
        assert!(app.search.visible, "the panel and its progress stay shown");
        let n = app.notices.back().unwrap();
        assert_eq!(n.level, crate::app::NoticeLevel::Warning, "{}", n.text);
    }
}
