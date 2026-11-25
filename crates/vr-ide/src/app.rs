//! Application state and input dispatch. Everything here is driven without a terminal:
//! the event loop feeds it crossterm events and file-system events, and `ui::render` draws it.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

/// Two presses on the same tab within this gap are a double click.
const TAB_DOUBLE_CLICK: Duration = Duration::from_millis(400);

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use vr_config::{ConfigWarning, Context, Keymap, Settings};
use vr_core::{BufferOptions, DiskState, Document, FsEvent, Selection, Watcher};

use crate::clipboard::Clipboard;
use crate::commands::dialog::DialogAction;
use crate::commands::explorer::Explorer;
use crate::commands::palette::Palette;
use crate::commands::{self, CmdArgs};
use crate::event_loop::{chord_from_key, legacy_unreachable};
use crate::ui::dialog::{DialogWidget, Outcome};
use crate::ui::layout::AppLayout;
use crate::workspace::{Root, Workspace};

/// Which part of the screen receives keys. A dialog is modal and takes every key while
/// open, whatever the focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Editor,
    Explorer,
    Palette,
    /// The find box in File scope (find / replace in the active file).
    FindBar,
    /// The find box in Project scope (project search / replace).
    Search,
    /// The integrated terminal panel (M5): every key but the `[terminal]` ones goes to it (D8).
    Terminal,
}

impl Focus {
    /// Keymap context. The find box in File scope sits on the editor, so the editor's keys
    /// (f4, ctrl+r) work in it; in Project scope it gets the global keys (ctrl+r there is the
    /// project replace, T21) plus the box's own editor keys (`search::handle_key`).
    pub fn context(self) -> Context {
        match self {
            Focus::Editor | Focus::FindBar => Context::Editor,
            Focus::Explorer => Context::Explorer,
            Focus::Palette => Context::Palette,
            Focus::Search => Context::Global,
            Focus::Terminal => Context::Terminal,
        }
    }
}

/// An open modal dialog: the widget and what its answer does.
#[derive(Debug, Clone)]
pub struct Dialog {
    pub widget: DialogWidget,
    pub action: DialogAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NoticeLevel {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub level: NoticeLevel,
    pub text: String,
    /// Info notices go away by themselves; warnings and errors stay until Escape.
    pub expires: Option<Instant>,
    /// Removed as soon as another notice arrives (a startup hint must not hide later news).
    pub until_next: bool,
    /// Shown after the dismiss hint and cut first (with an ellipsis) when the rows run out.
    pub detail: Option<String>,
}

pub const INFO_NOTICE_TTL: Duration = Duration::from_secs(5);

/// Enter is not taken by a dialog or the palette for this long after it opened: an Enter
/// typed for the editor just as a prompt appears must not answer it (D80).
pub const ENTER_SETTLE: Duration = Duration::from_millis(500);

/// An Enter press this soon after the previous one is taken as auto-repeat: legacy terminals
/// report a held key's repeats as presses (initial delay up to ~660 ms on X11), D65/D80.
pub const ENTER_REPEAT_GAP: Duration = Duration::from_millis(700);

/// How long the legacy-keyboard startup hint stays when nothing else is shown.
pub const KEYBOARD_NOTICE_TTL: Duration = Duration::from_secs(15);

/// Files kept in [`App::recent`].
pub const RECENT_MAX: usize = 8;

/// The key that dismisses a notice. Built in like the cursor keys (the plan keeps those out
/// of the keymap); see decisions.md.
pub const DISMISS_KEY: KeyCode = KeyCode::Esc;

/// Shown for `space` + a key that is not a read-mode sequence.
pub const SPACE_HINT: &str = "space i: edit · space space: commands";

/// Lines one wheel notch scrolls.
pub const WHEEL_LINES: usize = 3;

/// Per-tab scroll state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct View {
    /// First visible line.
    pub top: usize,
    /// First visible display column.
    pub left: usize,
    /// Scroll so the cursor is visible on the next render (set by edits and cursor moves,
    /// not by wheel scrolling).
    pub follow_cursor: bool,
    /// Edit mode (D145): text-changing keys and commands work only while this is set. A file
    /// tab drops back to read mode whenever the editor gains the keys on it.
    pub edit: bool,
    /// Tab name of a generated read-only tab without a file (`/keys`); `None` for documents.
    pub title: Option<&'static str>,
}

pub struct App {
    /// The project folders (roots) of this window (D226); the explorer lists each one (W-2).
    pub workspace: Workspace,
    /// Recently opened workspaces and the file they are kept in (None: not kept, tests).
    pub recent_workspaces: vr_config::RecentWorkspaces,
    pub recent_workspaces_file: Option<PathBuf>,
    /// Open documents, one per tab; `views[i]` belongs to `docs[i]`.
    pub docs: Vec<Document>,
    pub views: Vec<View>,
    pub active: usize,
    pub focus: Focus,
    pub notices: VecDeque<Notice>,
    pub keymap: Keymap,
    pub settings: Settings,
    /// What settings.toml said when it was last applied (start, live reload): a reload applies
    /// only the settings whose value in the file differs from this (T24).
    pub(crate) settings_file: Settings,
    pub clipboard: Box<dyn Clipboard>,
    pub fs_rx: Option<Receiver<FsEvent>>,
    pub(crate) watcher: Option<Watcher>,
    /// The terminal reports keys with the kitty protocol (all chords reachable).
    pub keyboard_enhanced: bool,
    /// Commands run, by name, in order (tests check which command a key ran).
    #[cfg(test)]
    pub ran: Vec<String>,
    pub quit: bool,
    /// Something changed since the last frame.
    pub dirty: bool,
    /// Areas of the last frame, for mouse hit tests and paging.
    pub layout: AppLayout,
    /// Mouse drag in the editor extends the selection.
    pub(crate) dragging: bool,
    /// Where unsaved buffers go when the user discards them (None: not kept).
    pub recovery_dir: Option<PathBuf>,
    /// The open modal dialog (takes all input).
    pub dialog: Option<Dialog>,
    /// The command palette while open (`Focus::Palette`).
    pub palette: Option<Palette>,
    /// Printed to stderr after the terminal is restored (e.g. where discarded buffers went).
    pub exit_messages: Vec<String>,
    /// [`ENTER_SETTLE`] and [`ENTER_REPEAT_GAP`]; tests that do not exercise timing set zero.
    pub enter_settle: Duration,
    pub enter_repeat_gap: Duration,
    /// When the open dialog or the palette appeared.
    pub(crate) modal_since: Option<Instant>,
    /// Time of the last Enter press or repeat, in any context.
    last_enter: Option<Instant>,
    /// Time of the key event being handled (injectable clock for tests).
    event_time: Option<Instant>,
    /// Last left press on a tab bar tab (index, time): the first half of a double click.
    last_tab_click: Option<(usize, Instant)>,
    /// The project tree panel (AC-1, AC-8).
    pub explorer: Explorer,
    /// Where `explorer.delete` sends paths (D9); tests swap in `vr_fs::FakeTrash`.
    pub trash: Box<dyn vr_fs::Trash>,
    /// Go-to-file index (built in the background) and its build in progress.
    pub file_index: Option<vr_fs::FileIndex>,
    pub(crate) index_rx: Option<Receiver<vr_fs::IndexMsg>>,
    /// The "go to file lists the first N files" notice was shown (once per session: the
    /// index is rebuilt on every go-to-file open).
    index_cap_noticed: bool,
    /// In-file find / replace bar (AC-10).
    pub find: commands::find::FindBar,
    /// Project search / replace panel (AC-11, AC-12).
    pub search: commands::search::SearchPanel,
    /// The tab that had the editor focus at the last [`App::sync_mode`] (None: the keys were
    /// elsewhere), to tell when the editor gains the keys on a tab.
    editor_seen: Option<usize>,
    /// Read mode got a `space` and waits for the second key (T21: `space i`, `space space`).
    pub(crate) space_pending: bool,
    /// Syntax highlighting of the open documents (M4).
    pub syntax: crate::syntax::Syntax,
    /// Resource monitor of the welcome screen; samples only while that screen is shown.
    pub monitor: crate::monitor::Monitor,
    /// Files opened this session, newest first (at most [`RECENT_MAX`]).
    pub recent: Vec<PathBuf>,
    /// Recently opened files of every vr-ide (`/recent`), newest first, and the state file
    /// they are kept in (None: not kept, tests). The file is written off the UI thread.
    pub recent_files: vr_config::RecentFiles,
    pub recent_files_file: Option<PathBuf>,
    recent_writer: Option<crate::recent_files::RecentWriter>,
    /// The folder of settings.toml and keymap.toml (`/config`, `/keymap`); None in tests.
    pub config_dir: Option<PathBuf>,
    /// The integrated terminal panel (M5); no terminal runs until the user opens one (AC-1).
    pub terminal: commands::terminal::TerminalPanel,
    /// Branch and missing mark of each root for the welcome screen (W-2), read with the
    /// monitor's samples and when the roots change, never while drawing.
    pub root_info: Vec<RootInfo>,
    /// Files of each root in the go-to-file index, by root name (several roots), counted
    /// once when the index is ready.
    pub root_files: std::collections::HashMap<String, usize>,
    /// Rows of the welcome screen that open a recent workspace when clicked (screen area,
    /// index into `recent_workspaces.entries`), from the last frame.
    pub welcome_hits: Vec<(ratatui::layout::Rect, usize)>,
    /// Add Folder / Open Workspace checks running off the UI thread (W-2).
    pub(crate) ws_job: Option<crate::app_workspace::WsJob>,
    /// How long the UI thread waits for such a check before it goes on and the result comes
    /// in later ([`crate::app_workspace::JOB_WAIT`]; tests set zero).
    pub ws_job_wait: Duration,
}

/// What the welcome screen shows of one root besides its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootInfo {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub missing: bool,
}

impl App {
    /// An app on one project root.
    pub fn new(
        root: PathBuf,
        keymap: Keymap,
        settings: Settings,
        clipboard: Box<dyn Clipboard>,
    ) -> App {
        App::with_workspace(Workspace::single(root), keymap, settings, clipboard)
    }

    pub fn with_workspace(
        workspace: Workspace,
        keymap: Keymap,
        settings: Settings,
        clipboard: Box<dyn Clipboard>,
    ) -> App {
        let mut explorer = Explorer::new(
            &workspace.primary().path,
            settings.show_hidden,
            settings.show_ignored,
        );
        let roots: Vec<(PathBuf, String)> = workspace
            .roots()
            .iter()
            .map(|r| (r.path.clone(), r.name.clone()))
            .collect();
        explorer.set_roots(&roots);
        let mut terminal = commands::terminal::TerminalPanel::new(
            settings.terminal_height_percent,
            settings.scrollback,
        );
        terminal.position = settings.terminal_position;
        terminal.width_percent = settings.terminal_width_percent;
        App {
            workspace,
            recent_workspaces: Default::default(),
            recent_workspaces_file: None,
            docs: Vec::new(),
            views: Vec::new(),
            active: 0,
            focus: Focus::Editor,
            notices: VecDeque::new(),
            keymap,
            settings_file: settings.clone(),
            settings,
            clipboard,
            fs_rx: None,
            watcher: None,
            keyboard_enhanced: false,
            #[cfg(test)]
            ran: Vec::new(),
            quit: false,
            dirty: true,
            layout: AppLayout::default(),
            dragging: false,
            recovery_dir: None,
            dialog: None,
            palette: None,
            exit_messages: Vec::new(),
            enter_settle: ENTER_SETTLE,
            enter_repeat_gap: ENTER_REPEAT_GAP,
            modal_since: None,
            last_enter: None,
            event_time: None,
            last_tab_click: None,
            explorer,
            trash: Box::new(vr_fs::SystemTrash),
            file_index: None,
            index_rx: None,
            index_cap_noticed: false,
            find: Default::default(),
            search: Default::default(),
            editor_seen: None,
            space_pending: false,
            syntax: crate::syntax::Syntax::new(crate::syntax::builtin_registry()),
            monitor: Default::default(),
            recent: Vec::new(),
            recent_files: Default::default(),
            recent_files_file: None,
            recent_writer: None,
            config_dir: None,
            terminal,
            root_info: Vec::new(),
            root_files: Default::default(),
            welcome_hits: Vec::new(),
            ws_job: None,
            ws_job_wait: crate::app_workspace::JOB_WAIT,
        }
    }

    /// Builds the language table from the built-in `languages.toml` overridden by `user`
    /// (AC-13e: a language whose grammar is not compiled in becomes a startup notice, shown by
    /// [`App::report_config`]); returns the file's own warnings.
    pub fn load_languages(&mut self, user: Option<&Path>) -> Vec<ConfigWarning> {
        let (languages, warnings) = vr_config::Languages::load(user);
        self.syntax.set_languages(&languages);
        warnings
    }

    /// Loads keymap and settings from `config_dir()`; returns the load warnings for
    /// [`App::report_startup`].
    pub fn load(root: PathBuf, clipboard: Box<dyn Clipboard>) -> (App, Vec<ConfigWarning>) {
        App::load_workspace(Workspace::single(root), clipboard)
    }

    /// [`App::load`] for a workspace; also reads the recent-workspaces list (a corrupt one
    /// is a warning and starts fresh).
    pub fn load_workspace(
        workspace: Workspace,
        clipboard: Box<dyn Clipboard>,
    ) -> (App, Vec<ConfigWarning>) {
        let dir = vr_config::config_dir();
        let (keymap, mut warnings) = Keymap::load(Some(&dir.join("keymap.toml")));
        let (settings, w) = Settings::load(Some(&dir.join("settings.toml")));
        warnings.extend(w);
        let mut app = App::with_workspace(workspace, keymap, settings, clipboard);
        warnings.extend(app.load_languages(Some(&dir.join(vr_config::LANGUAGES_FILE))));
        let state = vr_config::state_dir();
        app.recovery_dir = Some(state.join("recovery"));
        let recent = state.join(vr_config::RECENT_FILE);
        let (list, w) = vr_config::RecentWorkspaces::load(&recent);
        warnings.extend(w);
        app.recent_workspaces = list;
        app.recent_workspaces_file = Some(recent);
        let files = state.join(vr_config::RECENT_FILES_FILE);
        let (list, w) = vr_config::RecentFiles::load(&files);
        warnings.extend(w);
        app.recent_files = list;
        app.recent_files_file = Some(files);
        app.config_dir = Some(dir);
        (app, warnings)
    }

    /// Terminal title (AC-1).
    pub fn title(&self) -> String {
        format!("vr-ide — {}", self.workspace.label())
    }

    /// The first root: the explorer's tree (one root: the project root).
    pub fn root(&self) -> &Path {
        &self.workspace.primary().path
    }

    /// The root of the active file, else the first root (spec W): where a new terminal
    /// starts and relative paths are taken from.
    pub fn active_root(&self) -> &Root {
        let file = self.active_doc().and_then(|d| d.path.as_deref());
        self.workspace.active_root(file)
    }

    /// Startup notice: config warnings, unknown command names in the keymap (AC-5) and,
    /// without keyboard enhancement, the default chords the terminal cannot deliver.
    pub fn report_startup(&mut self, warnings: Vec<ConfigWarning>, keyboard_enhanced: bool) {
        self.report_config(warnings);
        self.report_keyboard(keyboard_enhanced);
    }

    /// Config warnings and unknown command names in the keymap (AC-5).
    pub fn report_config(&mut self, warnings: Vec<ConfigWarning>) {
        let names = commands::command_names();
        let unknown = self.keymap.validate(&names);
        for w in warnings.iter().chain(&unknown) {
            self.warn(w.to_string());
        }
        for w in self.syntax.take_startup_notices() {
            self.warn(w);
        }
        if crate::theme::Theme::named(&self.settings.theme).is_none() {
            let msg = format!(
                "settings: unknown theme {:?}; using {}",
                self.settings.theme,
                crate::theme::DEFAULT_THEME
            );
            self.warn(msg);
        }
    }

    /// Records whether the terminal reports keys with the kitty protocol; without it, lists
    /// the bound chords it cannot deliver.
    pub fn report_keyboard(&mut self, keyboard_enhanced: bool) {
        self.keyboard_enhanced = keyboard_enhanced;
        let names = commands::command_names();
        if !keyboard_enhanced {
            // The default keymap is deliverable everywhere (USER-5); this lists the keys a user
            // keymap binds that this terminal cannot send, unless the command has another one.
            let mut lost: Vec<String> = Vec::new();
            for (_, k, c) in self.keymap.bindings() {
                let has_reachable = self
                    .keymap
                    .keys_for(c)
                    .iter()
                    .any(|k| !legacy_unreachable(k));
                let name = k.to_string();
                if legacy_unreachable(k)
                    && names.contains(&c)
                    && !has_reachable
                    && !lost.contains(&name)
                {
                    lost.push(name);
                }
            }
            if !lost.is_empty() {
                let palette = self
                    .display_key("app.palette")
                    .filter(|s| s.parse().is_ok_and(|k| !legacy_unreachable(&k)));
                let remedy = match palette {
                    Some(key) => format!("use the command palette ({key})"),
                    None => "run them from the command palette".to_string(),
                };
                self.push_notice(Notice {
                    level: NoticeLevel::Warning,
                    text: format!("Keyboard: this terminal cannot send some keys; {remedy}."),
                    expires: Some(Instant::now() + KEYBOARD_NOTICE_TTL),
                    until_next: true,
                    detail: Some(format!("Unavailable: {}", lost.join(", "))),
                });
            }
        }
    }

    /// Starts the file watcher; open documents and later opens are watched.
    pub fn start_watcher(&mut self) {
        let (tx, rx) = mpsc::channel();
        match Watcher::new(tx) {
            Ok(mut w) => {
                let own = w.own_writes();
                let mut failed = Vec::new();
                for doc in &mut self.docs {
                    doc.set_own_writes(own.clone());
                    if let Some(p) = &doc.path
                        && let Err(e) = w.watch(p)
                    {
                        failed.push(e.to_string());
                    }
                }
                self.watcher = Some(w);
                self.fs_rx = Some(rx);
                self.watch_config_files();
                for e in failed {
                    self.warn(format!("file watching: {e}"));
                }
            }
            Err(e) => self.warn(format!(
                "file watching is off ({e}); external changes are not detected"
            )),
        }
    }

    /// Starts the explorer's folder watches (D110): the project root plus each expanded
    /// folder, one non-recursive watch each, so the cost follows what is shown and not the
    /// size of the tree (node_modules, target). Later expands and collapses keep them in step
    /// through [`App::sync_explorer_watches`].
    pub fn watch_root(&mut self) {
        self.explorer.watching = true;
        self.sync_explorer_watches();
    }

    /// Watches folders that became shown and ends the watches of folders no longer shown.
    /// A folder that cannot be watched is named once and counted as done, so it is not
    /// retried on every event (it still refreshes after our own operations).
    pub(crate) fn sync_explorer_watches(&mut self) {
        let ex = &self.explorer;
        if !ex.watching {
            return;
        }
        // Every root's own folder is watched (its top level is always listed), plus each
        // expanded folder.
        // A root marked missing already has its one notice; a watch would only fail and
        // add a second one (D292).
        let missing: Vec<&PathBuf> = ex
            .roots
            .iter()
            .filter(|r| r.missing)
            .map(|r| &r.path)
            .collect();
        let roots = self.workspace.paths();
        let shown = |p: &PathBuf| roots.contains(p) || ex.expanded.contains(p);
        let gone: Vec<PathBuf> = ex.watched.iter().filter(|p| !shown(p)).cloned().collect();
        let mut new: Vec<PathBuf> = ex
            .expanded
            .iter()
            .filter(|p| !ex.watched.contains(*p) && !missing.contains(p))
            .cloned()
            .collect();
        for root in &roots {
            if !ex.watched.contains(root) && !ex.expanded.contains(root) && !missing.contains(&root)
            {
                new.push(root.clone());
            }
        }
        // The watches can start before the explorer has listed anything (welcome screen):
        // a root not found here is marked missing and named once, instead of a watch error.
        // One stat per root about to be watched, never per frame.
        let mut found_missing = Vec::new();
        if self.workspace.is_multi() {
            new.retain(|p| {
                let lost = roots.contains(p)
                    && matches!(std::fs::metadata(p), Err(e) if e.kind() == std::io::ErrorKind::NotFound);
                if lost {
                    found_missing.push(p.clone());
                }
                !lost
            });
        }
        for p in found_missing {
            if let Some(r) = self.explorer.roots.iter_mut().find(|r| r.path == p) {
                r.missing = true;
            }
            commands::explorer::missing_root_notice(self, &p);
        }
        if gone.is_empty() && new.is_empty() {
            return;
        }
        let mut failed = Vec::new();
        if let Some(w) = &mut self.watcher {
            for p in &gone {
                w.unwatch_dir(p);
            }
            for p in &new {
                if let Err(e) = w.watch_dir(p) {
                    failed.push(e.to_string());
                }
            }
        }
        for p in &gone {
            self.explorer.watched.remove(p);
        }
        self.explorer.watched.extend(new);
        for e in failed {
            self.warn(format!(
                "explorer: {e}; that folder refreshes only after your own actions"
            ));
        }
    }

    // ---- documents ---------------------------------------------------------------------

    fn remember_recent(&mut self, path: &Path) {
        self.recent.retain(|p| p != path);
        self.recent.insert(0, path.to_path_buf());
        self.recent.truncate(RECENT_MAX);
        self.recent_files.push(path.to_path_buf());
        let Some(file) = self.recent_files_file.clone() else {
            return;
        };
        let writer = self
            .recent_writer
            .get_or_insert_with(|| crate::recent_files::RecentWriter::start(file));
        writer.record(path.to_path_buf());
        if let Some(e) = writer.take_error() {
            self.warn(format!("recent files: {e}"));
        }
    }

    /// Waits until the recent-files writer has written what was recorded so far (tests).
    pub fn flush_recent_files(&mut self) {
        if let Some(w) = &self.recent_writer {
            w.flush();
        }
    }

    /// At exit: waits up to `wait` for the recent-files writer.
    pub fn close_recent_files(&mut self, wait: std::time::Duration) {
        if let Some(w) = &self.recent_writer {
            w.close(wait);
        }
    }

    /// The file `from` is now `to`: the recent-files list follows (in memory and on disk).
    pub fn recent_file_renamed(&mut self, from: &Path, to: &Path) {
        self.recent_files.renamed(from, to.to_path_buf());
        let Some(file) = self.recent_files_file.clone() else {
            return;
        };
        let writer = self
            .recent_writer
            .get_or_insert_with(|| crate::recent_files::RecentWriter::start(file));
        writer.renamed(from.to_path_buf(), to.to_path_buf());
        if let Some(e) = writer.take_error() {
            self.warn(format!("recent files: {e}"));
        }
    }

    /// Opens `path` (relative paths are taken from the project root) in a new tab, or
    /// activates its tab when it is already open.
    pub fn open_path(&mut self, path: &Path) -> anyhow::Result<()> {
        let path = self.resolve(path);
        if let Some(i) = self
            .docs
            .iter()
            .position(|d| d.path.as_deref() == Some(&path))
        {
            self.set_active(i);
            self.remember_recent(&path);
            return Ok(());
        }
        // A running project replace never writes a file a tab opens on.
        let claim = commands::search::before_tab_opens(self, &path)
            .map_err(|m| anyhow::Error::new(commands::search::BeingReplaced(m)))?;
        let mut doc = match Document::open(&path, self.settings.max_highlight_bytes) {
            Ok(d) => d,
            Err(e) => {
                if let Some((gate, c)) = claim {
                    gate.open_failed(&c);
                }
                anyhow::bail!("{}: {e}", path.display());
            }
        };
        doc.buffer.set_options(self.buffer_options());
        if let Some(w) = &mut self.watcher {
            doc.set_own_writes(w.own_writes());
            if let Err(e) = w.watch(&path) {
                let msg = format!("file watching: {e}");
                self.warn(msg);
            }
        }
        if doc.read_only {
            let what = not_utf8_reason(&doc);
            self.warn(format!(
                "{} is {what}; opened read-only",
                self.display_path(&path)
            ));
        }
        self.remember_recent(&path);
        self.docs.push(doc);
        self.views.push(View {
            follow_cursor: true,
            ..View::default()
        });
        self.set_active(self.docs.len() - 1);
        // The highlighter starts now (its parse runs on a worker), not at the first frame.
        self.sync_syntax();
        self.sync_mode();
        Ok(())
    }

    /// `path` made absolute (relative to the active root) with `.` and `..` removed
    /// lexically, so one file never gets two tabs.
    pub fn resolve(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            crate::workspace::normalize(path)
        } else {
            crate::workspace::normalize(&self.active_root().path.join(path))
        }
    }

    /// After Save As: the watcher follows the document to its new path.
    pub fn rewatch(&mut self, old: Option<&Path>, new: &Path) {
        if old == Some(new) {
            return;
        }
        let still_open = |p: &Path| {
            self.docs
                .iter()
                .filter(|d| d.path.as_deref() == Some(p))
                .count()
        };
        let drop_old = old
            .filter(|p| still_open(p) == 0 && !self.is_config_file(p))
            .map(Path::to_path_buf);
        let Some(w) = &mut self.watcher else {
            return;
        };
        if let Some(p) = drop_old {
            w.unwatch(&p);
        }
        if let Err(e) = w.watch(new) {
            let msg = format!("file watching: {e}");
            self.warn(msg);
        }
    }

    pub fn buffer_options(&self) -> BufferOptions {
        let tab_width = self.settings.tab_width.max(1);
        BufferOptions {
            tab_width,
            indent_unit: if self.settings.insert_spaces {
                " ".repeat(tab_width)
            } else {
                "\t".to_string()
            },
        }
    }

    pub fn set_active(&mut self, i: usize) {
        if i < self.docs.len() {
            if i != self.active
                && let Some(d) = self.docs.get_mut(self.active)
            {
                d.buffer.break_undo_group();
            }
            self.active = i;
            self.dirty = true;
        }
    }

    /// Removes tab `i` without asking (callers check for unsaved changes).
    pub fn close_doc(&mut self, i: usize) {
        if i >= self.docs.len() {
            return;
        }
        let doc = self.docs.remove(i);
        self.views.remove(i);
        self.syntax.remove(i);
        // The watch is per path: another tab still on it keeps needing it.
        let shared = doc
            .path
            .as_deref()
            .is_some_and(|p| self.docs.iter().any(|d| d.path.as_deref() == Some(p)))
            // settings.toml and keymap.toml stay watched for the live reload (T24).
            || doc.path.as_deref().is_some_and(|p| self.is_config_file(p));
        if let (Some(w), Some(p), false) = (&mut self.watcher, &doc.path, shared) {
            w.unwatch(p);
        }
        if self.active > i || self.active >= self.docs.len() {
            self.active = self.active.saturating_sub(1);
        }
        // Keep pointing at the same tab; the closed one is forgotten, so the tab that takes
        // its place starts in read mode.
        self.editor_seen = match self.editor_seen {
            Some(s) if s == i => None,
            Some(s) if s > i => Some(s - 1),
            other => other,
        };
        self.dirty = true;
    }

    /// Brings the active document's highlighter in step with its buffer (before each frame;
    /// other tabs catch up when they are shown) and shows why highlighting is off, once.
    pub fn sync_syntax(&mut self) {
        let i = self.active;
        let Some(doc) = self.docs.get(i) else {
            return;
        };
        let notice = self.syntax.sync(i, doc, self.settings.max_highlight_bytes);
        if let Some(n) = notice {
            let name = doc
                .path
                .as_deref()
                .map(|p| self.display_path(p))
                .unwrap_or_default();
            let level = if n.warning {
                NoticeLevel::Warning
            } else {
                NoticeLevel::Info
            };
            self.notify(level, format!("{name}: {}", n.text));
        }
    }

    pub fn active_doc(&self) -> Option<&Document> {
        self.docs.get(self.active)
    }

    pub fn active_doc_mut(&mut self) -> Option<&mut Document> {
        self.docs.get_mut(self.active)
    }

    pub fn active_view_mut(&mut self) -> Option<&mut View> {
        self.views.get_mut(self.active)
    }

    /// The active document if it may be edited; a read-only one, or a tab in read mode
    /// (D145), gets a notice instead. Every text-changing key and command of the user goes
    /// through here; project replace and reloads from disk do not, so they still update
    /// buffers in read mode. The view follows the cursor afterwards.
    pub fn editable_doc(&mut self) -> Option<&mut Document> {
        let read_only = self.active_doc()?.read_only;
        if read_only {
            self.read_only_warning();
            return None;
        }
        self.sync_mode();
        if !self.edit_mode() {
            self.read_mode_hint();
            return None;
        }
        self.cursor_moved();
        self.docs.get_mut(self.active)
    }

    // ---- read / edit mode (D145) ---------------------------------------------------------

    /// The active tab is in edit mode.
    pub fn edit_mode(&self) -> bool {
        self.views.get(self.active).is_some_and(|v| v.edit)
    }

    /// Applies the mode rule after focus or the active tab changed: when the editor gains
    /// the keys on a tab (open, tab switch, back from the explorer, palette, find box or
    /// a dialog), file tabs are in read mode and an untitled buffer in edit mode. The find
    /// box sits on the editor and keeps the mode; so does the palette opened over it while
    /// it is open, so a command picked there acts in the mode the tab had. Closing the
    /// palette without a command (Esc, a click outside) returns in read mode, see
    /// [`App::palette_dismissed`]. Called at the end of every input and
    /// watcher event, so the many places that move the focus need no hook of their own.
    pub(crate) fn sync_mode(&mut self) {
        let focus = match &self.palette {
            Some(p) => p.prev_focus,
            None => self.focus,
        };
        let on_editor = self.dialog.is_none() && matches!(focus, Focus::Editor | Focus::FindBar);
        let here = Some(self.active).filter(|&i| on_editor && i < self.docs.len());
        if here == self.editor_seen {
            return;
        }
        // Only the tab with the keys can be in edit mode, so leaving resets them all.
        for v in &mut self.views {
            v.edit = false;
        }
        if let Some(i) = here
            && let (Some(d), Some(v)) = (self.docs.get(i), self.views.get_mut(i))
        {
            // An untitled buffer is for typing; a generated read-only tab (`/keys`) is not.
            v.edit = d.path.is_none() && !d.read_only;
        }
        self.editor_seen = here;
        self.dirty = true;
    }

    /// Warning for an edit attempt on a document that is open read-only (not UTF-8).
    fn read_only_warning(&mut self) {
        let Some(doc) = self.active_doc() else {
            return;
        };
        if let Some(title) = self.views.get(self.active).and_then(|v| v.title) {
            self.warn(format!("{title}: read-only (generated by vr-ide)"));
            return;
        }
        let what = not_utf8_reason(doc);
        let name = doc
            .path
            .clone()
            .map(|p| self.display_path(&p))
            .unwrap_or_else(|| "file".to_string());
        self.warn(format!("{name}: read-only ({what})"));
    }

    /// Edit mode on the active tab (space i in read mode, T21); a read-only document stays in read mode.
    pub fn enter_edit_mode(&mut self) {
        if self.active_doc().is_some_and(|d| d.read_only) {
            self.read_only_warning();
            return;
        }
        self.sync_mode();
        if let Some(v) = self.views.get_mut(self.active) {
            v.edit = true;
        }
        self.dirty = true;
    }

    /// Read mode on the active tab; the selection collapses to the cursor (Esc).
    pub fn enter_read_mode(&mut self) {
        self.sync_mode();
        if let Some(v) = self.views.get_mut(self.active) {
            v.edit = false;
        }
        if let Some(doc) = self.docs.get_mut(self.active) {
            let head = doc.buffer.selection().primary().head;
            doc.buffer.set_selection(Selection::point(head));
        }
        self.dirty = true;
    }

    /// The short non-modal hint for an edit attempt in read mode: one notice per burst of
    /// attempts (a repeat only extends its time).
    fn read_mode_hint(&mut self) {
        let text = format!("Read mode: {} to edit", self.key_hint("editor.edit_mode"));
        let expires = Some(Instant::now() + INFO_NOTICE_TTL);
        if let Some(n) = self.notices.iter_mut().find(|n| n.text == text) {
            n.expires = expires;
            self.dirty = true;
            return;
        }
        self.push_notice(Notice {
            level: NoticeLevel::Info,
            text,
            expires,
            until_next: true,
            detail: None,
        });
    }

    /// Built-in keys of the editor in read (normal) mode (T21, D261), also on the welcome
    /// screen: `space i` edits, `space space` opens the palette, `space` + another key only
    /// shows [`SPACE_HINT`]; Tab moves the focus to the next part. These are not keymap
    /// sequences (a user keymap still has single keys only). True when the key was used.
    fn read_mode_key(&mut self, key: &KeyEvent) -> bool {
        if self.focus != Focus::Editor || self.edit_mode() {
            return false;
        }
        if std::mem::take(&mut self.space_pending) {
            // Esc only cancels the space, then does what Esc does (dismiss a notice).
            if key.code == KeyCode::Esc {
                return false;
            }
            match key.code {
                KeyCode::Char('i') if key.modifiers.is_empty() => {
                    self.run_command("editor.edit_mode", CmdArgs::None);
                }
                KeyCode::Char(' ') if key.modifiers.is_empty() => {
                    self.run_command("app.palette", CmdArgs::None);
                }
                _ => self.info(SPACE_HINT),
            }
            return true;
        }
        match key.code {
            KeyCode::Char(' ') if key.modifiers.is_empty() => {
                self.space_pending = true;
                true
            }
            KeyCode::Tab if key.modifiers.is_empty() => {
                self.run_command("focus.cycle", CmdArgs::None);
                true
            }
            _ => false,
        }
    }

    /// After a palette key or click: a palette closed without running anything (`dismissed`)
    /// hands the editor back in read mode (D145).
    fn palette_dismissed(&mut self, dismissed: bool) {
        if dismissed && self.palette.is_none() {
            self.editor_seen = None;
        }
    }

    /// The editor gets the keys again as if freshly focused, so the tab is in read mode even
    /// when it was already the active one (Go to File on the open tab, D145 (1)).
    pub(crate) fn refocus_editor_in_read_mode(&mut self) {
        self.focus = Focus::Editor;
        self.editor_seen = None;
    }

    /// The terminal cursor should be a bar (typing goes into text) rather than a block (the
    /// editor in read mode).
    pub fn wants_bar_cursor(&self) -> bool {
        let reading = self.dialog.is_none()
            && self.palette.is_none()
            && self.focus == Focus::Editor
            && self.active_doc().is_some()
            && !self.edit_mode();
        !reading
    }

    /// The terminal was resized: redraw.
    pub fn on_resize(&mut self) {
        self.cursor_moved();
    }

    /// Marks the frame dirty and scrolls to the cursor on the next render.
    pub fn cursor_moved(&mut self) {
        if let Some(v) = self.views.get_mut(self.active) {
            v.follow_cursor = true;
        }
        self.dirty = true;
    }

    /// Display path of the active document ("" without one).
    pub fn active_name(&self) -> String {
        self.active_doc()
            .and_then(|d| d.path.as_deref())
            .map(|p| self.display_path(p))
            .unwrap_or_default()
    }

    /// Path relative to the project root when inside it; `root-name/relative` with several
    /// roots.
    pub fn display_path(&self, p: &Path) -> String {
        self.workspace.display_path(p)
    }

    // ---- notices -----------------------------------------------------------------------

    pub fn notify(&mut self, level: NoticeLevel, text: impl Into<String>) {
        let expires = (level == NoticeLevel::Info).then(|| Instant::now() + INFO_NOTICE_TTL);
        self.push_notice(Notice {
            level,
            text: text.into(),
            expires,
            until_next: false,
            detail: None,
        });
    }

    pub fn push_notice(&mut self, n: Notice) {
        self.notices.retain(|old| !old.until_next);
        self.notices.push_back(n);
        self.dirty = true;
    }

    pub fn info(&mut self, text: impl Into<String>) {
        self.notify(NoticeLevel::Info, text);
    }

    pub fn warn(&mut self, text: impl Into<String>) {
        self.notify(NoticeLevel::Warning, text);
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.notify(NoticeLevel::Error, text);
    }

    /// Drops expired info notices and runs a find rescan whose typing pause is over.
    pub fn tick(&mut self, now: Instant) {
        commands::find::poll(self, now);
        // The welcome screen is the only reader of the monitor: nothing is read behind a file or a panel that covers it.
        // Before the first frame nothing is known: sample as usual.
        let covered = self.layout.status.height > 0 && self.layout.editor.height == 0;
        let welcome_drawn = self.docs.is_empty() && !covered;
        if self.monitor.tick(now, welcome_drawn) {
            self.monitor.branch = self.active_root().branch();
            if self.workspace.is_multi() {
                self.refresh_root_info();
            }
            self.dirty = true;
        }
        let before = self.notices.len();
        self.notices.retain(|n| n.expires.is_none_or(|t| t > now));
        if self.notices.len() != before {
            self.dirty = true;
        }
    }

    /// The one key every place shows for `cmd` (USER-5, D261). The default keymap has one key
    /// per command; with a user keymap that adds more, the shortest key this terminal can send
    /// wins (a key it cannot send only when nothing else is bound). Without a keymap key, the
    /// built-in key of the editor (T21: `space i`, `tab`); `None` when there is neither.
    pub fn display_key(&self, cmd: &str) -> Option<String> {
        // Each key with whether a legacy terminal sends it.
        let keys: Vec<(bool, String)> = self
            .keymap
            .keys_for(cmd)
            .iter()
            .map(|k| (!legacy_unreachable(k), k.to_string()))
            .collect();
        let pick = |only_reachable: bool| {
            keys.iter()
                .filter(|(ok, _)| !only_reachable || *ok)
                .map(|(_, s)| s.clone())
                .min_by_key(String::len)
        };
        pick(!self.keyboard_enhanced)
            .or_else(|| pick(false))
            .or_else(|| builtin_key(cmd).map(str::to_owned))
    }

    /// The key to name in a notice or hint for `cmd` (never hard-coded, USER-1).
    pub fn key_hint(&self, cmd: &str) -> String {
        self.display_key(cmd)
            .unwrap_or_else(|| format!("run {cmd}"))
    }

    /// Writes the unsaved buffers of `idxs` to the recovery directory before they are
    /// discarded. Returns the files written; `Err` (with the message to show) when any buffer
    /// could not be written, and then the caller must not discard anything.
    pub fn keep_discarded(&mut self, idxs: &[usize]) -> Result<Vec<PathBuf>, String> {
        let Some(dir) = self.recovery_dir.clone() else {
            return Ok(Vec::new());
        };
        let docs: Vec<&Document> = idxs
            .iter()
            .filter_map(|&i| self.docs.get(i))
            .filter(|d| d.is_modified())
            .collect();
        if docs.is_empty() {
            return Ok(Vec::new());
        }
        let written = crate::recovery::write_docs_in(&docs, &self.workspace, &dir);
        if written.len() == docs.len() {
            Ok(written)
        } else {
            Err(format!(
                "could not write the unsaved changes to {}; nothing was discarded",
                dir.display()
            ))
        }
    }

    // ---- dialogs -----------------------------------------------------------------------

    /// Opens a modal dialog; it replaces any open one and closes the palette.
    pub fn open_dialog(&mut self, widget: DialogWidget, action: DialogAction) {
        commands::palette::close(self);
        self.explorer.menu = None;
        self.dialog = Some(Dialog { widget, action });
        self.modal_since = Some(self.now());
        self.dirty = true;
    }

    /// Hands a dialog's answer to its action (which may open the next dialog).
    fn answer_dialog(&mut self, outcome: Outcome) {
        if outcome == Outcome::Pending {
            return;
        }
        if let Some(d) = self.dialog.take() {
            commands::dialog::answer(self, d, outcome);
        }
    }

    // ---- input -------------------------------------------------------------------------

    /// Runs a command by name; errors become notices. Returns false for unknown names.
    pub fn run_command(&mut self, name: &str, args: CmdArgs) -> bool {
        let Some(spec) = commands::find(name) else {
            self.warn(format!("unknown command {name:?}"));
            return false;
        };
        #[cfg(test)]
        self.ran.push(name.to_owned());
        if let Err(e) = (spec.run)(self, args) {
            self.error(format!("{}: {e}", spec.title));
        }
        self.sync_mode();
        self.sync_explorer_watches();
        self.dirty = true;
        true
    }

    /// Routing: an open dialog takes every key; then the palette's built-in keys; Esc
    /// dismisses a notice; the editor's built-in keys; finally the keymap for the focus.
    pub fn handle_key(&mut self, key: KeyEvent) {
        self.handle_key_at(key, Instant::now());
    }

    /// [`App::handle_key`] at time `now`.
    pub fn handle_key_at(&mut self, key: KeyEvent, now: Instant) {
        self.event_time = Some(now);
        self.route_key(key, now);
        self.event_time = None;
        self.sync_mode();
        self.sync_explorer_watches();
    }

    fn route_key(&mut self, key: KeyEvent, now: Instant) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        self.dirty = true;
        // A pending read-mode space only pairs with the next key the reading editor gets.
        if self.focus != Focus::Editor || self.edit_mode() || self.dialog.is_some() {
            self.space_pending = false;
        }
        let enter_ignored = key.code == KeyCode::Enter && self.enter_is_not_deliberate(&key, now);
        if key.code == KeyCode::Enter {
            self.last_enter = Some(now);
        }
        let modal =
            self.dialog.is_some() || self.focus == Focus::Palette || self.explorer.menu.is_some();
        if modal && enter_ignored {
            return;
        }
        if let Some(d) = &mut self.dialog {
            let outcome = d.widget.handle_key(&key);
            self.answer_dialog(outcome);
            return;
        }
        // The explorer's right-click menu takes the keys it uses (T23).
        if self.explorer.menu.is_some() {
            if self.focus != Focus::Explorer || !self.explorer.visible {
                self.explorer.menu = None;
            } else if commands::explorer_menu::handle_key(self, &key) {
                return;
            }
        }
        let palette_esc = key.code == KeyCode::Esc;
        if self.focus == Focus::Palette && commands::palette::handle_key(self, &key) {
            self.palette_dismissed(palette_esc);
            return;
        }
        // A focused terminal takes every key but its own table (D8); elsewhere a legacy
        // terminal's NUL stands for ctrl+` (D185).
        if commands::terminal::handle_key(self, &key)
            || commands::terminal::legacy_toggle(self, &key)
        {
            return;
        }
        // The find box uses Esc itself in either scope (close / back to the editor).
        // Keys never go to a Project-scope box that is not shown.
        if self.focus == Focus::Search && !self.search.visible {
            self.focus = Focus::Editor;
        }
        if self.focus == Focus::FindBar && commands::find::handle_key(self, &key) {
            return;
        }
        if self.focus == Focus::Search && commands::search::handle_key(self, &key) {
            return;
        }
        // The key that leaves edit mode (Esc) does that before it dismisses a notice: the
        // user is typing, the notice can wait for the next press.
        if self.focus == Focus::Editor && self.edit_mode() {
            let leaves = chord_from_key(&key, self.keyboard_enhanced).is_some_and(|c| {
                self.keymap.command_for_in(Context::Editor, &c) == Some("editor.read_mode")
            });
            if leaves {
                self.run_command("editor.read_mode", CmdArgs::None);
                return;
            }
        }
        if self.read_mode_key(&key) {
            return;
        }
        if key.code == DISMISS_KEY && key.modifiers.is_empty() && !self.notices.is_empty() {
            self.notices.pop_front();
            return;
        }
        // The explorer's navigation keys are built in like the editor's cursor keys, but a
        // chord the keymap binds for the explorer is the user's (USER-1) and goes to it.
        if self.focus == Focus::Explorer {
            let bound = chord_from_key(&key, self.keyboard_enhanced)
                .is_some_and(|c| self.keymap.command_for_in(Context::Explorer, &c).is_some());
            if !bound && commands::explorer::handle_builtin_key(self, &key) {
                return;
            }
        }
        // Built-in cursor and typing keys come first: the plan keeps them out of the keymap.
        if self.focus == Focus::Editor && commands::edit::handle_builtin_key(self, &key) {
            return;
        }
        let Some(chord) = chord_from_key(&key, self.keyboard_enhanced) else {
            return;
        };
        if let Some(cmd) = self.keymap.command_for_in(self.focus.context(), &chord) {
            let cmd = cmd.to_owned();
            // A command started from the palette's context acts on the editor.
            commands::palette::close(self);
            self.run_command(&cmd, CmdArgs::None);
        }
    }

    /// An Enter that a dialog or the palette must not take: a labelled repeat, one within
    /// [`ENTER_SETTLE`] of the dialog opening (typed for what was there before), or one within
    /// [`ENTER_REPEAT_GAP`] of the previous Enter (legacy auto-repeat of a held key).
    fn enter_is_not_deliberate(&self, key: &KeyEvent, now: Instant) -> bool {
        let within = |t: Option<Instant>, d: Duration| {
            t.is_some_and(|t| now.saturating_duration_since(t) < d)
        };
        key.kind == KeyEventKind::Repeat
            || within(self.modal_since, self.enter_settle)
            || within(self.last_enter, self.enter_repeat_gap)
    }

    /// Time of the event being handled.
    pub(crate) fn now(&self) -> Instant {
        self.event_time.unwrap_or_else(Instant::now)
    }

    /// Bracketed paste from the terminal.
    pub fn handle_paste(&mut self, text: &str) {
        self.dirty = true;
        // A paste is not the second key of a read-mode `space` sequence.
        self.space_pending = false;
        if let Some(d) = &mut self.dialog {
            d.widget.handle_paste(text);
            return;
        }
        match self.focus {
            Focus::Palette => commands::palette::paste(self, text),
            Focus::FindBar => commands::find::paste(self, text),
            Focus::Search => commands::search::paste(self, text),
            // Nothing in the explorer takes text.
            Focus::Explorer => {}
            Focus::Editor => commands::edit::insert_pasted(self, text),
            Focus::Terminal => commands::terminal::paste(self, text),
        }
    }

    /// Click places the cursor (shift+click extends), drag selects, the wheel scrolls the
    /// view without moving the cursor, a click on a tab activates it.
    pub fn handle_mouse(&mut self, ev: MouseEvent) {
        self.handle_mouse_at(ev, Instant::now());
    }

    /// `handle_mouse` with an injectable clock (double click timing).
    pub fn handle_mouse_at(&mut self, ev: MouseEvent, now: Instant) {
        self.event_time = Some(now);
        self.route_mouse(ev);
        self.event_time = None;
        self.sync_mode();
        self.sync_explorer_watches();
    }

    fn route_mouse(&mut self, ev: MouseEvent) {
        self.space_pending = false;
        let inside = |r: ratatui::layout::Rect| {
            ev.column >= r.x
                && ev.column < r.x + r.width
                && ev.row >= r.y
                && ev.row < r.y + r.height
        };
        self.dirty = true;
        if let Some(d) = &mut self.dialog {
            let outcome = d.widget.handle_mouse(&ev);
            self.answer_dialog(outcome);
            return;
        }
        // An open explorer menu sees every mouse event first (T23).
        if commands::explorer_menu::handle_mouse(self, &ev) {
            return;
        }
        if self.palette.is_some() {
            let outside = self
                .palette
                .as_ref()
                .is_some_and(|p| matches!(ev.kind, MouseEventKind::Down(_)) && !inside(p.area));
            commands::palette::handle_mouse(self, &ev);
            self.palette_dismissed(outside);
            return;
        }
        if commands::terminal::handle_mouse(self, &ev) {
            return;
        }
        // A click on the notice dismisses it without moving the focus: a focused terminal
        // keeps Esc for its program.
        if inside(self.layout.notice) {
            if matches!(ev.kind, MouseEventKind::Down(MouseButton::Left)) {
                self.notices.pop_front();
            }
            return;
        }
        let l = self.layout;
        if inside(l.explorer) {
            commands::explorer::handle_mouse(self, &ev);
            return;
        }
        if inside(l.find_bar) {
            commands::find::handle_mouse(self, &ev);
            return;
        }
        if matches!(ev.kind, MouseEventKind::Down(_))
            && matches!(
                self.focus,
                Focus::Explorer | Focus::Search | Focus::FindBar | Focus::Terminal
            )
        {
            self.focus = Focus::Editor;
        }
        match ev.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if inside(l.tabs) {
                    if let Some(i) = crate::ui::tabs::hit(self, ev.column - l.tabs.x, l.tabs.width)
                    {
                        self.set_active(i);
                        let now = self.now();
                        let double = self.last_tab_click.is_some_and(|(j, t)| {
                            j == i && now.saturating_duration_since(t) <= TAB_DOUBLE_CLICK
                        });
                        if double {
                            self.last_tab_click = None;
                            if self.docs[i].path.is_some() {
                                self.run_command("explorer.reveal", CmdArgs::None);
                            }
                        } else {
                            self.last_tab_click = Some((i, now));
                        }
                    }
                } else if self.docs.is_empty() && inside(l.editor) {
                    // The welcome screen: a recent workspace row opens it (W-2).
                    let hit = self.welcome_hits.iter().find(|(r, _)| {
                        ev.column >= r.x
                            && ev.column < r.x + r.width
                            && ev.row >= r.y
                            && ev.row < r.y + r.height
                    });
                    if let Some(entry) = hit
                        .and_then(|(_, i)| self.recent_workspaces.entries.get(*i))
                        .cloned()
                        && let Err(e) = commands::slash::open_recent_workspace(self, &entry)
                    {
                        self.warn(format!("workspace: {e}"));
                    }
                } else if inside(l.editor)
                    && let Some(pos) = self.char_at_screen(ev.column, ev.row)
                {
                    let extend = ev.modifiers.contains(KeyModifiers::SHIFT);
                    if let Some(doc) = self.active_doc_mut() {
                        let anchor = if extend {
                            doc.buffer.selection().primary().anchor
                        } else {
                            pos
                        };
                        doc.buffer.set_selection(Selection::single(anchor, pos));
                    }
                    self.dragging = true;
                    self.cursor_moved();
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragging => {
                let bottom = (l.editor.y + l.editor.height).max(1) - 1;
                let row = ev.row.clamp(l.editor.y, bottom.max(l.editor.y));
                if let Some(pos) = self.char_at_screen(ev.column, row)
                    && let Some(doc) = self.active_doc_mut()
                {
                    let anchor = doc.buffer.selection().primary().anchor;
                    doc.buffer.set_selection(Selection::single(anchor, pos));
                    self.cursor_moved();
                }
            }
            MouseEventKind::Up(MouseButton::Left) => self.dragging = false,
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp if inside(l.editor) => {
                let down = ev.kind == MouseEventKind::ScrollDown;
                let last = self
                    .active_doc()
                    .map_or(0, |d| d.buffer.text().len_lines().saturating_sub(1));
                if let Some(v) = self.views.get_mut(self.active) {
                    v.top = if down {
                        (v.top + WHEEL_LINES).min(last)
                    } else {
                        v.top.saturating_sub(WHEEL_LINES)
                    };
                    v.follow_cursor = false;
                    self.dirty = true;
                }
            }
            _ => {}
        }
    }

    /// Char offset under a screen cell of the editor area.
    fn char_at_screen(&self, column: u16, row: u16) -> Option<usize> {
        let doc = self.active_doc()?;
        let view = self.views.get(self.active)?;
        let area = self.layout.editor;
        let rope = doc.buffer.text();
        let gutter = crate::ui::editor_view::gutter_width(rope);
        let last = rope.len_lines().saturating_sub(1);
        let line = (view.top + usize::from(row.saturating_sub(area.y))).min(last);
        let x = usize::from(column.saturating_sub(area.x + gutter));
        let tw = doc.buffer.options().tab_width;
        let slice = rope.line(line);
        Some(
            rope.line_to_char(line)
                + crate::ui::editor_view::char_at_screen_col(slice, view.left, x, tw),
        )
    }

    /// Applies a watcher event (AC-7): an unmodified document reloads; a modified one keeps
    /// the buffer, its tab is marked and the Reload / Keep mine prompt opens.
    pub fn handle_fs_event(&mut self, ev: FsEvent) {
        self.apply_fs_event(ev);
        commands::explorer::flush(self);
        self.sync_explorer_watches();
        self.sync_mode();
    }

    /// [`App::handle_fs_event`] without re-reading the explorer folders it touched.
    fn apply_fs_event(&mut self, ev: FsEvent) {
        commands::explorer::on_fs_event(self, &ev);
        // settings.toml / keymap.toml changed by another program (T24): applied at once. A file
        // removed or renamed away is not read (editors do that for a moment while saving).
        match &ev {
            FsEvent::Modified(p) | FsEvent::Created(p) | FsEvent::Renamed(_, p) => {
                self.config_file_changed(p, false)
            }
            FsEvent::Removed(_) | FsEvent::Rescan(_) | FsEvent::Error(_) => {}
        }
        match ev {
            FsEvent::Modified(p) | FsEvent::Created(p) | FsEvent::Removed(p) => self.recheck(&p),
            FsEvent::Renamed(from, to) => {
                self.recheck(&from);
                self.recheck(&to);
            }
            FsEvent::Rescan(dir) => {
                let paths: Vec<PathBuf> = self
                    .docs
                    .iter()
                    .filter_map(|d| d.path.clone())
                    .filter(|p| p.starts_with(&dir))
                    .collect();
                for p in paths {
                    self.recheck(&p);
                }
            }
            FsEvent::Error(msg) => self.warn(format!(
                "file watching: {msg}; external changes may be missed"
            )),
        }
    }

    /// Compares the open document at `path` with the disk: reload when unmodified, otherwise
    /// keep the buffer and mark the tab.
    fn recheck(&mut self, path: &Path) {
        let Some(i) = self
            .docs
            .iter()
            .position(|d| d.path.as_deref() == Some(path))
        else {
            return;
        };
        let name = self.display_path(path);
        let doc = &mut self.docs[i];
        let before = doc.disk_state;
        let state = doc.check_disk();
        let modified = doc.is_modified();
        // A modified buffer asks (AC-7) unless another dialog is open; then the tab mark and
        // the notice tell, and saving asks.
        if modified && before != state && self.dialog.is_none() {
            match state {
                DiskState::ChangedOnDisk => {
                    commands::file::prompt_external_change(self, i, false);
                    return;
                }
                DiskState::DeletedOnDisk => {
                    commands::file::prompt_deleted(self, i);
                    return;
                }
                DiskState::InSync => {}
            }
        }
        let doc = &mut self.docs[i];
        let msg = match state {
            DiskState::ChangedOnDisk if !modified => match doc.reload() {
                Ok(()) => {
                    self.syntax.reset(i);
                    None
                }
                Err(e) => Some((NoticeLevel::Error, format!("{name}: reload failed: {e}"))),
            },
            DiskState::ChangedOnDisk if before != state => Some((
                NoticeLevel::Warning,
                format!("{name} changed on disk; your unsaved changes are kept"),
            )),
            DiskState::DeletedOnDisk if before != state => Some((
                NoticeLevel::Warning,
                format!("{name} was deleted on disk; the buffer is kept"),
            )),
            _ => None,
        };
        if let Some((level, text)) = msg {
            self.notify(level, text);
        }
        self.dirty = true;
    }

    /// Takes the go-to-file index once its background build finished.
    pub fn drain_index(&mut self) {
        use std::sync::mpsc::TryRecvError;
        let Some(rx) = &self.index_rx else {
            return;
        };
        let done = loop {
            match rx.try_recv() {
                Ok(vr_fs::IndexMsg::Progress(_)) => {}
                Ok(vr_fs::IndexMsg::Ready(index)) => break Ok(index),
                Ok(vr_fs::IndexMsg::Error(e)) => break Err(e),
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => break Err("the file index stopped".to_string()),
            }
        };
        self.index_rx = None;
        match done {
            Ok(index) => {
                if index.truncated() {
                    self.note_index_cap(index.len());
                }
                self.file_index = Some(index);
                self.count_root_files();
            }
            Err(e) => self.warn(format!("go to file: {e}")),
        }
        commands::palette::refresh_keeping_selection(self);
    }

    /// Says that go to file lists only the first `listed` files of a project above the
    /// index cap.
    pub(crate) fn note_index_cap(&mut self, listed: usize) {
        if !self.index_cap_noticed {
            self.index_cap_noticed = true;
            self.info(format!(
                "go to file lists the first {listed} files of this project"
            ));
        }
    }

    /// Takes the results the project search sent so far (see `commands::search::drain`).
    pub fn drain_search(&mut self) {
        commands::search::drain(self);
    }

    /// Builds the go-to-file index on a background thread; [`App::drain_index`] takes it.
    /// A build already running is kept.
    pub fn start_index(&mut self) {
        if self.index_rx.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let roots = self
            .workspace
            .roots()
            .iter()
            .map(|r| (r.path.clone(), r.name.clone()))
            .collect();
        // Detached: the walk ends when it is done or when `index_rx` is dropped.
        drop(vr_fs::FileIndex::build_roots(roots, tx));
        self.index_rx = Some(rx);
    }

    /// Handles every pending watcher event without blocking.
    pub fn drain_fs_events(&mut self) {
        let events: Vec<FsEvent> = match &self.fs_rx {
            Some(rx) => rx.try_iter().collect(),
            None => return,
        };
        for ev in events {
            self.apply_fs_event(ev);
        }
        commands::explorer::flush(self);
        self.sync_explorer_watches();
        self.sync_mode();
    }
}

/// Why a document is open read-only: "not valid UTF-8" or "<encoding>, not UTF-8".
fn not_utf8_reason(doc: &Document) -> String {
    let enc = encoding_name(doc);
    if enc == "non-UTF-8" {
        "not valid UTF-8".to_string()
    } else {
        format!("{enc}, not UTF-8")
    }
}

pub fn encoding_name(doc: &Document) -> String {
    match &doc.encoding {
        vr_core::Encoding::Utf8 => "UTF-8".to_string(),
        vr_core::Encoding::Utf8Bom => "UTF-8 BOM".to_string(),
        vr_core::Encoding::Other(name) => name.clone(),
    }
}

/// Keys built into the editor that run a command (T21): read mode's `space i` and Tab, edit
/// mode's Tab. Shown where a command has no keymap key.
fn builtin_key(cmd: &str) -> Option<&'static str> {
    match cmd {
        "editor.edit_mode" => Some("space i"),
        "focus.cycle" | "edit.indent" => Some("tab"),
        _ => None,
    }
}
