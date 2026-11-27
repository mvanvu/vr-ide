//! The session store (D1–D3, SEC-6): what a `vr-ide --session` window has open, kept in
//! `state_dir()/session.toml` and restored by the next `--session` launch (the desktop
//! launcher). Every other launch never reads or writes it.
//!
//! - Kept: workspace roots, file tabs (cursor, scroll, read/edit mode), the active tab, the
//!   explorer's expanded folders, the terminal panel's layout and each terminal's folder, and
//!   the AI panel's own section ([`AiSection`]). Never kept: buffer text, terminal input,
//!   commands, ssh lines or anything secret; a restore starts each terminal as a fresh shell
//!   and runs nothing else.
//! - Only the window holding the exclusive lock on `session.lock` writes the file; a second
//!   `--session` window says so and runs without saving.
//! - Writes are atomic (a private temp file in the same folder, fsync, rename), 0600 inside a
//!   0700 folder; a symlink at the path is replaced, never followed. The file is written at
//!   quit and at most [`SAVE_DELAY`] + 2 × [`CHECK_EVERY`] after a change.
//! - A file that is not a regular file of the user's, writable by others or over
//!   [`MAX_BYTES`] is ignored; one that does not parse is kept as [`BAD_FILE`]. Both give one
//!   notice and open the home folder. Counts are capped and numbers clamped on load; roots go
//!   through the workspace's folder rules and tabs open only regular files inside a root.
//!
//! # The AI panel's section
//! [`Session::ai`] belongs to the AI panel; nothing here acts on it. The section read at start
//! is handed out once by [`take_restored_ai`] (strings with control characters and entries
//! over the caps already dropped, but tool, account and folder still unchecked: the panel
//! applies its own rules before it starts anything). Until the panel reports its state with
//! [`set_ai`] (on every change of its tabs, accounts, visibility or width), the section read
//! at start is written back unchanged.

use std::fs::{File, Metadata, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use vr_config::TerminalPosition;
use vr_core::Selection;

use crate::app::{App, NoticeLevel};
use crate::recovery::{create_private_dir, create_private_file};
use crate::workspace::{Workspace, WorkspaceError, normalize};

pub const SESSION_FILE: &str = "session.toml";
/// Where an unreadable session file is moved, so the next save does not destroy it.
pub const BAD_FILE: &str = "session.toml.bad";
pub const LOCK_FILE: &str = "session.lock";

pub const MAX_BYTES: u64 = 1 << 20;
pub const MAX_ROOTS: usize = 64;
pub const MAX_TABS: usize = 200;
pub const MAX_EXPANDED: usize = 2_000;
pub const MAX_TERMINALS: usize = 32;
pub const MAX_AI_SESSIONS: usize = 32;
/// Longest tool, account or label kept in the AI section, in characters.
pub const MAX_AI_TEXT: usize = 256;
/// Cursor lines and columns and scroll positions are clamped to this on load (and to the
/// file's real size when the tab opens).
const MAX_POSITION: u64 = 1 << 40;
const MAX_LEFT: usize = 1 << 20;

/// How often a `--session` window compares what it has open with what it last saved.
pub const CHECK_EVERY: Duration = Duration::from_millis(500);
/// A change is written this long after it was first seen (later changes ride along).
pub const SAVE_DELAY: Duration = Duration::from_millis(750);
/// How long the quit waits for the last write: only a stuck state folder takes this long.
const QUIT_WAIT: Duration = Duration::from_secs(5);

const ANOTHER_WINDOW: &str =
    "Another vr-ide --session window keeps the session; this one is not saved";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    pub roots: Vec<PathBuf>,
    /// Index into `tabs`.
    pub active_tab: usize,
    pub expanded: Vec<PathBuf>,
    pub tabs: Vec<TabState>,
    pub terminal: TerminalLayout,
    pub ai: AiSection,
}

/// A file tab. Lines and columns count from 0, columns in characters.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TabState {
    pub path: PathBuf,
    pub line: u64,
    pub column: u64,
    pub top: u64,
    pub left: u64,
    pub edit: bool,
}

/// The terminal panel. Absent values keep the settings; each folder is one terminal, started
/// as a fresh shell there.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerminalLayout {
    pub shown: bool,
    pub position: Option<TerminalPosition>,
    pub height_percent: Option<u16>,
    pub width_percent: Option<u16>,
    pub folders: Vec<PathBuf>,
    /// Index into `folders`.
    pub active: usize,
}

/// The AI panel's part of the session (see the module docs).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSection {
    pub shown: bool,
    /// Panel width in percent of the screen, 10..=90 after load.
    pub width: Option<u16>,
    pub sessions: Vec<AiSession>,
}

/// One agent tab: the configured tool's name, the account name, its folder and its label.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSession {
    pub tool: String,
    pub account: String,
    pub folder: PathBuf,
    pub label: String,
}

/// The home folder (`$HOME` when absolute), where `--session` opens without a session.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .filter(|h| h.is_absolute())
}

// ---- start and restore -----------------------------------------------------------------------

/// What [`start`] found: the workspace to build the app on, and the rest for [`restore`].
pub struct Started {
    pub workspace: Workspace,
    pub restore: Restore,
}

pub struct Restore {
    keeper: Keeper,
    session: Option<Session>,
    notices: Vec<(NoticeLevel, String)>,
}

/// Takes the lock and reads the session kept in `dir`; without one (or with a file that is
/// ignored or unreadable) the workspace is `home` alone.
pub fn start(dir: &Path, home: &Path) -> Started {
    let file = dir.join(SESSION_FILE);
    let mut notices = Vec::new();
    let lock = match take_lock(dir) {
        Ok(Some(f)) => Some(f),
        Ok(None) => {
            notices.push((NoticeLevel::Warning, ANOTHER_WINDOW.to_string()));
            None
        }
        Err(e) => {
            notices.push((
                NoticeLevel::Warning,
                format!(
                    "Session not saved: cannot lock {}: {e}",
                    dir.join(LOCK_FILE).display()
                ),
            ));
            None
        }
    };
    if lock.is_some() {
        remove_stale_temps(dir);
    }
    let session = match load(&file) {
        Loaded::Missing => None,
        Loaded::Read(s) => Some(sanitize(*s)),
        Loaded::Ignored(reason) => {
            notices.push((
                NoticeLevel::Error,
                format!("Session file ignored ({reason}); opened ~/"),
            ));
            None
        }
        Loaded::Corrupt { line, handle } => {
            let at = line.map(|l| format!(" (line {l})")).unwrap_or_default();
            let text = if lock.is_none() {
                format!("Session file unreadable{at}; opened ~/")
            } else {
                match keep_bad(&file, &handle) {
                    Ok(()) => {
                        format!("Session file unreadable{at}; opened ~/, kept it as {BAD_FILE}")
                    }
                    Err(e) => format!(
                        "Session file unreadable{at}; opened ~/ (cannot keep it as {BAD_FILE}: {e})"
                    ),
                }
            };
            notices.push((NoticeLevel::Error, text));
            None
        }
    };
    let home = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
    let (workspace, refused) = match session.as_ref().filter(|s| !s.roots.is_empty()) {
        Some(s) => Workspace::from_paths(&s.roots)
            .unwrap_or_else(|_| (Workspace::single(home.clone()), Vec::new())),
        None => (Workspace::single(home), Vec::new()),
    };
    for e in refused {
        if !matches!(e, WorkspaceError::Duplicate { .. }) {
            notices.push((NoticeLevel::Warning, e.to_string()));
        }
    }
    Started {
        workspace,
        restore: Restore {
            keeper: Keeper::new(file, lock),
            session,
            notices,
        },
    }
}

/// Puts the session back into `app` (built on [`Started::workspace`], with the file watcher
/// started) and keeps it from now on. Files are read as they are on disk now.
pub fn restore(app: &mut App, r: Restore) {
    let Restore {
        mut keeper,
        session,
        notices,
    } = r;
    for (level, text) in notices {
        app.notify(level, text);
    }
    if let Some(s) = &session {
        keeper.ai = s.ai.clone();
        keeper.restored_ai = Some(s.ai.clone());
    }
    app.session = Some(keeper);
    let Some(s) = session else {
        return;
    };
    restore_explorer(app, &s.expanded);
    let skipped = restore_tabs(app, &s);
    restore_terminal(app, &s.terminal);
    if !skipped.is_empty() {
        app.warn(skipped_notice(&skipped));
    }
}

/// The AI section read at start, once (see the module docs).
pub fn take_restored_ai(app: &mut App) -> Option<AiSection> {
    app.session.as_mut()?.restored_ai.take()
}

/// The AI panel's current state; the next save writes it.
pub fn set_ai(app: &mut App, ai: AiSection) {
    if let Some(k) = app.session.as_mut() {
        k.ai = sanitize_ai(ai);
    }
}

fn restore_explorer(app: &mut App, expanded: &[PathBuf]) {
    let roots = app.workspace.paths();
    let mut dirs: Vec<&PathBuf> = expanded
        .iter()
        .filter(|d| plain_absolute(d) && !roots.contains(d))
        .filter(|d| roots.iter().any(|r| d.starts_with(r)))
        // A link (possibly leading out of the root) is never expanded from the file, neither
        // as the folder itself nor as a folder on the way to it.
        .filter(|d| std::fs::symlink_metadata(d).is_ok_and(|m| m.is_dir()))
        .filter(|d| {
            d.canonicalize()
                .is_ok_and(|c| roots.iter().any(|r| c.starts_with(r)))
        })
        .collect();
    if dirs.is_empty() {
        return;
    }
    crate::commands::explorer::ensure_loaded(app);
    dirs.sort_by_key(|d| d.components().count());
    for d in dirs {
        let _ = app.explorer.expand(d);
    }
}

/// Opens the tabs; returns the names of those skipped.
fn restore_tabs(app: &mut App, s: &Session) -> Vec<String> {
    let mut skipped = Vec::new();
    let mut active = None;
    for (i, t) in s.tabs.iter().enumerate() {
        let Some(path) = tab_file(app, &t.path) else {
            skipped.push(shown_name(app, &t.path));
            continue;
        };
        if app.docs.iter().any(|d| d.path.as_deref() == Some(&path)) {
            continue;
        }
        if app.open_path(&path).is_err() {
            skipped.push(shown_name(app, &t.path));
            continue;
        }
        let di = app.docs.len() - 1;
        place_cursor(app, di, t);
        if i == s.active_tab {
            active = Some((di, t.edit));
        }
    }
    if let Some((di, edit)) = active {
        app.set_active(di);
        app.sync_mode();
        if edit && app.active_doc().is_some_and(|d| !d.read_only) {
            app.enter_edit_mode();
        }
    }
    skipped
}

/// `p` when it names a regular file inside a root of the workspace (links resolved).
fn tab_file(app: &App, p: &Path) -> Option<PathBuf> {
    if !plain_absolute(p) {
        return None;
    }
    let root = app.workspace.root_of(p)?;
    let canon = p.canonicalize().ok()?;
    if !canon.starts_with(&root.path) {
        return None;
    }
    std::fs::metadata(&canon)
        .ok()?
        .is_file()
        .then(|| p.to_path_buf())
}

fn place_cursor(app: &mut App, di: usize, t: &TabState) {
    let (Some(doc), Some(view)) = (app.docs.get_mut(di), app.views.get_mut(di)) else {
        return;
    };
    let text = doc.buffer.text();
    let last = text.len_lines().saturating_sub(1);
    let line = clamp_usize(t.line, last);
    let slice = text.line(line);
    let mut len = slice.len_chars();
    while len > 0 && matches!(slice.char(len - 1), '\n' | '\r') {
        len -= 1;
    }
    let at = text.line_to_char(line) + clamp_usize(t.column, len);
    doc.buffer.set_selection(Selection::point(at));
    view.top = clamp_usize(t.top, last);
    view.left = clamp_usize(t.left, MAX_LEFT);
    view.follow_cursor = false;
}

fn restore_terminal(app: &mut App, t: &TerminalLayout) {
    let p = &mut app.terminal;
    if let Some(pos) = t.position {
        p.position = pos;
    }
    if let Some(h) = t.height_percent {
        p.height_percent = h;
    }
    if let Some(w) = t.width_percent {
        p.width_percent = w;
        p.width_cols = None;
    }
    for f in &t.folders {
        let dir = terminal_dir(app, f);
        crate::commands::terminal::add_tab_in(app, &dir);
    }
    let p = &mut app.terminal;
    if !p.tabs.is_empty() {
        p.active = t.active.min(p.tabs.len() - 1);
        p.visible = t.shown;
    }
}

/// A terminal's folder when it is an existing folder inside a root; else the first root
/// still on disk (the first root when none is).
fn terminal_dir(app: &App, f: &Path) -> PathBuf {
    let inside = plain_absolute(f)
        && app.workspace.root_of(f).is_some()
        && f.canonicalize()
            .is_ok_and(|c| app.workspace.root_of(&c).is_some() && c.is_dir());
    if inside {
        return f.to_path_buf();
    }
    app.workspace
        .roots()
        .iter()
        .map(|r| r.path.clone())
        .find(|p| p.is_dir())
        .unwrap_or_else(|| app.workspace.primary().path.clone())
}

fn shown_name(app: &App, p: &Path) -> String {
    app.display_path(p)
        .chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

/// UI spec S6: `Session restored; 2 tabs skipped, files not found: a, b` (names that do not
/// fit end with `… +N`).
fn skipped_notice(names: &[String]) -> String {
    const SHOWN: usize = 5;
    const WIDTH: usize = 100;
    let mut list = String::new();
    let mut shown = 0;
    for n in names {
        if shown == SHOWN || (shown > 0 && list.len() + n.len() + 2 > WIDTH) {
            break;
        }
        if shown > 0 {
            list.push_str(", ");
        }
        list.push_str(n);
        shown += 1;
    }
    let rest = names.len() - shown;
    let more = if rest > 0 {
        format!(" … +{rest}")
    } else {
        String::new()
    };
    let tabs = if names.len() == 1 { "tab" } else { "tabs" };
    format!(
        "Session restored; {} {tabs} skipped, files not found: {list}{more}",
        names.len()
    )
}

// ---- keeping it --------------------------------------------------------------------------

/// The `--session` window's store: compares what is open with what was saved and writes
/// changes off the UI thread. Without the lock it never writes.
pub struct Keeper {
    file: PathBuf,
    /// Held for the window's life; the lock goes with the file handle.
    lock: Option<File>,
    writer: Option<Writer>,
    saved: Option<Session>,
    /// When the unsaved change was first seen.
    pending: Option<Instant>,
    checked: Option<Instant>,
    ai: AiSection,
    restored_ai: Option<AiSection>,
    /// The active file tab and its edit mode while no dialog was open: the quit question takes
    /// the keys (read mode) before the quit save, which keeps the mode the user had.
    edit_seen: Option<(PathBuf, bool)>,
    /// The last write error, shared with the writer thread.
    error: Arc<Mutex<Option<String>>>,
    /// The error last shown, so a failing folder gives one notice, not one per change.
    reported: Option<String>,
}

impl Keeper {
    fn new(file: PathBuf, lock: Option<File>) -> Keeper {
        let error = Arc::new(Mutex::new(None));
        let writer = lock
            .as_ref()
            .and_then(|_| Writer::start(file.clone(), error.clone()));
        Keeper {
            file,
            lock,
            writer,
            saved: None,
            pending: None,
            checked: None,
            ai: AiSection::default(),
            restored_ai: None,
            edit_seen: None,
            error,
            reported: None,
        }
    }

    /// This window holds the lock and saves.
    pub fn saves(&self) -> bool {
        self.lock.is_some()
    }

    fn send(&mut self, s: Session) {
        let body = match toml::to_string(&s) {
            Ok(b) => format!("# vr-ide session, kept by `vr-ide --session`\n{b}"),
            Err(e) => {
                set(&self.error, format!("Session not saved: {e}"));
                return;
            }
        };
        self.saved = Some(s);
        match &self.writer {
            Some(w) => {
                let _ = w.tx.send(Msg::Save(body));
            }
            // No writer thread could be started: write here.
            None => {
                if let Err(e) = write_file(&self.file, body.as_bytes()) {
                    set(&self.error, write_error(&self.file, &e));
                }
            }
        }
    }

    /// Waits up to `wait` (`None`: as long as it takes) for what was sent before.
    fn wait(&self, wait: Option<Duration>) {
        let Some(w) = &self.writer else {
            return;
        };
        let (done, rx) = mpsc::channel();
        if w.tx.send(Msg::Flush(done)).is_ok() {
            let _ = match wait {
                Some(t) => rx.recv_timeout(t).map_err(|_| ()),
                None => rx.recv().map_err(|_| ()),
            };
        }
    }

    /// A write error since the last call, shown once per text. The failed session no longer
    /// counts as saved, so the next check writes it again.
    fn new_error(&mut self) -> Option<String> {
        let e = self
            .error
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()?;
        self.saved = None;
        if self.reported.as_ref() == Some(&e) {
            return None;
        }
        self.reported = Some(e.clone());
        Some(e)
    }
}

fn set(slot: &Mutex<Option<String>>, text: String) {
    *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(text);
}

fn write_error(file: &Path, e: &io::Error) -> String {
    format!("Session not saved: cannot write {}: {e}", file.display())
}

/// Called from [`App::tick`]: at most every [`CHECK_EVERY`], compares the window with the last
/// save and writes a change [`SAVE_DELAY`] after it was first seen.
pub fn tick(app: &mut App, now: Instant) {
    let Some(k) = app.session.as_ref() else {
        return;
    };
    if !k.saves() {
        return;
    }
    remember_edit_mode(app);
    let Some(k) = app.session.as_ref() else {
        return;
    };
    if k.checked
        .is_some_and(|t| now.saturating_duration_since(t) < CHECK_EVERY)
    {
        return;
    }
    crate::ai::restore::report(app);
    let snap = snapshot(app);
    let Some(k) = app.session.as_mut() else {
        return;
    };
    k.checked = Some(now);
    let error = k.new_error();
    if k.saved.as_ref() == Some(&snap) {
        k.pending = None;
    } else {
        match k.pending {
            None => k.pending = Some(now),
            Some(t) if now.saturating_duration_since(t) >= SAVE_DELAY => {
                k.pending = None;
                k.send(snap);
            }
            Some(_) => {}
        }
    }
    if let Some(e) = error {
        app.warn(e);
    }
}

/// Notes the active file tab's edit mode while no dialog holds the keys (see
/// `Keeper::edit_seen`); cheap, so it runs on every pass of the loop.
fn remember_edit_mode(app: &mut App) {
    // A quit question's answer takes the dialog out and leaves the tab in read mode before
    // the quit is seen: once quitting, the mode from before the question stays.
    if app.dialog.is_some() || app.quit {
        return;
    }
    let edit = app.edit_mode();
    let path = app.active_doc().and_then(|d| d.path.as_deref());
    let same = app.session.as_ref().is_some_and(|k| {
        k.edit_seen.as_ref().map(|(p, e)| (p.as_path(), *e)) == path.map(|p| (p, edit))
    });
    if same {
        return;
    }
    let now = path.map(|p| (p.to_path_buf(), edit));
    if let Some(k) = app.session.as_mut() {
        k.edit_seen = now;
    }
}

/// At quit, before the terminals are ended: writes what is open now (bounded wait).
pub fn save_on_quit(app: &mut App) {
    if !app.session.as_ref().is_some_and(Keeper::saves) {
        return;
    }
    crate::ai::restore::report(app);
    let snap = snapshot(app);
    if let Some(k) = app.session.as_mut() {
        k.send(snap);
        // Tests read the file right after; a busy disk must not make them flaky.
        k.wait(if cfg!(test) { None } else { Some(QUIT_WAIT) });
        if let Some(e) = k.new_error() {
            app.exit_messages.push(e);
        }
    }
}

/// Waits for every write sent so far (tests).
pub fn flush(app: &mut App) {
    if let Some(k) = app.session.as_ref() {
        k.wait(None);
    }
}

/// What the window has open now, as it would be saved.
pub fn snapshot(app: &App) -> Session {
    let roots = app
        .workspace
        .paths()
        .into_iter()
        .filter(|p| utf8(p))
        .take(MAX_ROOTS)
        .collect();
    let edit_seen = app.session.as_ref().and_then(|k| k.edit_seen.as_ref());
    let mut tabs = Vec::new();
    let mut active_tab = 0;
    for (i, (doc, view)) in app.docs.iter().zip(&app.views).enumerate() {
        // Untitled buffers and generated tabs have no file to reopen; their text is never kept.
        let Some(path) = doc.path.as_ref().filter(|p| utf8(p)) else {
            continue;
        };
        if view.title.is_some() || tabs.len() == MAX_TABS {
            continue;
        }
        if i == app.active {
            active_tab = tabs.len();
        }
        let text = doc.buffer.text();
        let head = doc.buffer.selection().primary().head.min(text.len_chars());
        let line = text.char_to_line(head);
        // While a dialog holds the keys (the quit question among them) the tab shows read mode;
        // the mode the user had before it is what is kept.
        let edit_before =
            (app.quit || app.dialog.is_some()) && edit_seen.is_some_and(|(p, e)| *e && p == path);
        tabs.push(TabState {
            path: path.clone(),
            line: line as u64,
            column: (head - text.line_to_char(line)) as u64,
            top: view.top as u64,
            left: view.left as u64,
            edit: i == app.active && (view.edit || edit_before),
        });
    }
    let mut expanded: Vec<PathBuf> = app
        .explorer
        .expanded
        .iter()
        .filter(|p| utf8(p))
        .cloned()
        .collect();
    expanded.sort();
    expanded.truncate(MAX_EXPANDED);
    let p = &app.terminal;
    let mut folders = Vec::new();
    let mut active = 0;
    // SSH sessions are not reopened (nothing is replayed).
    for (i, tab) in p.tabs.iter().enumerate() {
        if tab.ssh.is_some() || folders.len() == MAX_TERMINALS {
            continue;
        }
        if i == p.active {
            active = folders.len();
        }
        // An exited shell's pid may belong to another process by now.
        let dir = tab
            .running()
            .then(|| tab.pid())
            .flatten()
            .and_then(|pid| std::fs::read_link(format!("/proc/{pid}/cwd")).ok())
            .or_else(|| tab.cwd.clone())
            .filter(|d| utf8(d))
            .unwrap_or_else(|| app.workspace.primary().path.clone());
        folders.push(dir);
    }
    Session {
        roots,
        active_tab,
        expanded,
        tabs,
        terminal: TerminalLayout {
            shown: p.visible && !folders.is_empty(),
            position: Some(p.position),
            height_percent: Some(p.height_percent),
            width_percent: Some(p.width_percent),
            folders,
            active,
        },
        ai: app
            .session
            .as_ref()
            .map(|k| k.ai.clone())
            .unwrap_or_default(),
    }
}

/// Caps counts and clamps numbers of a session read from the file (SEC-6). Paths are checked
/// where they are used.
pub fn sanitize(mut s: Session) -> Session {
    s.roots.retain(|p| plain_absolute(p));
    s.roots.truncate(MAX_ROOTS);
    s.tabs.truncate(MAX_TABS);
    for t in &mut s.tabs {
        for n in [&mut t.line, &mut t.column, &mut t.top, &mut t.left] {
            *n = (*n).min(MAX_POSITION);
        }
    }
    s.active_tab = s.active_tab.min(s.tabs.len().saturating_sub(1));
    s.expanded.retain(|p| plain_absolute(p));
    s.expanded.truncate(MAX_EXPANDED);
    let t = &mut s.terminal;
    t.height_percent = t.height_percent.map(|v| v.clamp(10, 90));
    t.width_percent = t.width_percent.map(|v| v.clamp(10, 90));
    t.folders.truncate(MAX_TERMINALS);
    t.active = t.active.min(t.folders.len().saturating_sub(1));
    s.ai = sanitize_ai(s.ai);
    s
}

fn sanitize_ai(mut ai: AiSection) -> AiSection {
    let text_ok = |s: &str| s.chars().count() <= MAX_AI_TEXT && !s.chars().any(char::is_control);
    ai.sessions.retain(|a| {
        text_ok(&a.tool)
            && text_ok(&a.account)
            && text_ok(&a.label)
            && a.folder.to_str().is_some_and(text_ok)
            && plain_absolute(&a.folder)
    });
    ai.sessions.truncate(MAX_AI_SESSIONS);
    ai.width = ai.width.map(|v| v.clamp(10, 90));
    ai
}

// ---- the file ----------------------------------------------------------------------------

enum Loaded {
    Missing,
    Read(Box<Session>),
    Ignored(&'static str),
    /// Does not parse; `handle` is the file read, for [`keep_bad`].
    Corrupt {
        line: Option<usize>,
        handle: File,
    },
}

/// Reads the session file without following a link or opening anything but a regular file.
fn load(path: &Path) -> Loaded {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Loaded::Missing,
        Err(_) => return Loaded::Ignored("cannot read it"),
    };
    if let Some(reason) = refused(&meta) {
        return Loaded::Ignored(reason);
    }
    let flags = rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK;
    let Ok(mut f) = OpenOptions::new()
        .read(true)
        .custom_flags(flags.bits() as i32)
        .open(path)
    else {
        return Loaded::Ignored("cannot read it");
    };
    match f.metadata() {
        Ok(m) if m.dev() == meta.dev() && m.ino() == meta.ino() => {
            if let Some(reason) = refused(&m) {
                return Loaded::Ignored(reason);
            }
        }
        _ => return Loaded::Ignored("cannot read it"),
    }
    let mut bytes = Vec::new();
    if (&mut f)
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return Loaded::Ignored("cannot read it");
    }
    if bytes.len() as u64 > MAX_BYTES {
        return Loaded::Ignored("larger than 1 MiB");
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return Loaded::Corrupt {
            line: None,
            handle: f,
        };
    };
    match toml::from_str::<Session>(&text) {
        Ok(s) => Loaded::Read(Box::new(s)),
        Err(e) => Loaded::Corrupt {
            line: e.span().map(|r| {
                let end = r.start.min(text.len());
                text.as_bytes()[..end]
                    .iter()
                    .filter(|&&b| b == b'\n')
                    .count()
                    + 1
            }),
            handle: f,
        },
    }
}

fn refused(m: &Metadata) -> Option<&'static str> {
    if !m.file_type().is_file() {
        Some("not a regular file")
    } else if m.uid() != rustix::process::getuid().as_raw() {
        Some("not owned by you")
    } else if m.mode() & 0o022 != 0 {
        Some("writable by others")
    } else if m.len() > MAX_BYTES {
        Some("larger than 1 MiB")
    } else {
        None
    }
}

/// Moves the unreadable file aside as [`BAD_FILE`] (0600, replacing an older one).
fn keep_bad(file: &Path, handle: &File) -> io::Result<()> {
    handle.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    std::fs::rename(file, file.with_file_name(BAD_FILE))
}

/// The exclusive lock of the saving window; `None` when another window holds it.
fn take_lock(dir: &Path) -> io::Result<Option<File>> {
    private_dir(dir)?;
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(dir.join(LOCK_FILE))?;
    // Someone else's lock file could be held by them forever (or swapped meanwhile).
    let m = f.metadata()?;
    if !m.file_type().is_file() || m.uid() != rustix::process::getuid().as_raw() {
        return Err(io::Error::other("not your own regular file"));
    }
    // A child forked by another thread holds a copy of a just-released lock until it execs
    // (the descriptor is close-on-exec): a few short retries keep that from looking like
    // another window.
    for _ in 0..10 {
        match f.try_lock() {
            Ok(()) => return Ok(Some(f)),
            Err(std::fs::TryLockError::WouldBlock) => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(std::fs::TryLockError::Error(e)) => return Err(e),
        }
    }
    Ok(None)
}

/// The session's folder: created 0700 when missing. An existing one must be a real folder
/// (a link there is refused, never followed) owned by the user; group and other bits are
/// removed (other state files may have created it with the umask's mode).
fn private_dir(dir: &Path) -> io::Result<()> {
    create_private_dir(dir)?;
    let check = || -> io::Result<u32> {
        let m = std::fs::symlink_metadata(dir)?;
        if !m.file_type().is_dir() {
            return Err(io::Error::other(format!(
                "{} is not a folder (links are not followed)",
                dir.display()
            )));
        }
        if m.uid() != rustix::process::getuid().as_raw() {
            return Err(io::Error::other(format!(
                "{} is not your own folder",
                dir.display()
            )));
        }
        Ok(m.mode())
    };
    if check()? & 0o077 == 0 {
        return Ok(());
    }
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    if check()? & 0o077 != 0 {
        return Err(io::Error::other(format!(
            "cannot make {} private",
            dir.display()
        )));
    }
    Ok(())
}

/// Atomic, private write: a new 0600 temp file in the same folder, fsync, rename over `file`
/// (a link there is replaced, not followed), then the folder is synced.
fn write_file(file: &Path, body: &[u8]) -> io::Result<()> {
    let dir = file.parent().unwrap_or(Path::new("."));
    private_dir(dir)?;
    let mut n = 0u32;
    let (tmp, mut f) = loop {
        let tmp = dir.join(format!(".{SESSION_FILE}.{}-{n}.tmp", std::process::id()));
        match create_private_file(&tmp) {
            Ok(f) => break (tmp, f),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists && n < 100 => n += 1,
            Err(e) => return Err(e),
        }
    };
    let done = f
        .write_all(body)
        .and_then(|()| f.sync_all())
        .and_then(|()| std::fs::rename(&tmp, file));
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    done?;
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

/// Temp files of a write that never finished (the window was killed): with the lock held no
/// other window writes, so every one of them is stale.
fn remove_stale_temps(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let prefix = format!(".{SESSION_FILE}.");
    for e in entries.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(&prefix) && name.ends_with(".tmp") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

enum Msg {
    Save(String),
    Flush(Sender<()>),
}

/// Writes off the UI thread, so a slow or hung state folder never holds a frame.
struct Writer {
    tx: Sender<Msg>,
}

impl Writer {
    fn start(file: PathBuf, error: Arc<Mutex<Option<String>>>) -> Option<Writer> {
        let (tx, rx) = mpsc::channel::<Msg>();
        std::thread::Builder::new()
            .name("vr-session".into())
            .spawn(move || {
                for msg in rx {
                    match msg {
                        Msg::Save(body) => {
                            if let Err(e) = write_file(&file, body.as_bytes()) {
                                set(&error, write_error(&file, &e));
                            }
                        }
                        Msg::Flush(done) => {
                            let _ = done.send(());
                        }
                    }
                }
            })
            .ok()?;
        Some(Writer { tx })
    }
}

fn utf8(p: &Path) -> bool {
    p.to_str().is_some()
}

/// Absolute with no `.` or `..` parts.
fn plain_absolute(p: &Path) -> bool {
    p.is_absolute() && normalize(p) == p
}

fn clamp_usize(n: u64, max: usize) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX).min(max)
}

#[cfg(test)]
pub(crate) fn create_private_dir_for_tests(dir: &Path) {
    create_private_dir(dir).unwrap();
}

#[cfg(test)]
pub(crate) fn file_for_tests(app: &App) -> PathBuf {
    app.session
        .as_ref()
        .expect("a --session window")
        .file
        .clone()
}
