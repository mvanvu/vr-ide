//! Application state and input dispatch. Everything here is driven without a terminal:
//! the event loop feeds it crossterm events and file-system events, and `ui::render` draws it.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use vr_config::{ConfigWarning, Context, Keymap, Settings};
use vr_core::{BufferOptions, DiskState, Document, FsEvent, Selection, Watcher};

use crate::clipboard::Clipboard;
use crate::commands::{self, CmdArgs};
use crate::event_loop::{chord_from_key, legacy_unreachable};
use crate::ui::layout::AppLayout;

/// Which part of the screen receives keys. T6/T8/T10 add palette, explorer, find bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Editor,
}

impl Focus {
    pub fn context(self) -> Context {
        match self {
            Focus::Editor => Context::Editor,
        }
    }
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
}

pub const INFO_NOTICE_TTL: Duration = Duration::from_secs(5);

/// How long the legacy-keyboard startup hint stays when nothing else is shown.
pub const KEYBOARD_NOTICE_TTL: Duration = Duration::from_secs(15);

/// A second press of a "press again" key counts only after this much quiet since the previous
/// press of it: legacy terminals report auto-repeat of a held key as more presses, every
/// 25-50 ms after a first delay of 250-660 ms on common systems.
pub const CONFIRM_REPEAT_GAP: Duration = Duration::from_millis(700);

/// A pending "press again" confirmation is forgotten after this long.
pub const CONFIRM_TTL: Duration = Duration::from_secs(30);

/// The key that dismisses a notice or cancels a pending confirmation. Built in like the cursor
/// keys (the plan keeps those out of the keymap); see decisions.md.
pub const DISMISS_KEY: KeyCode = KeyCode::Esc;

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
}

/// A pending "press again" confirmation.
#[derive(Debug, Clone)]
struct Confirm {
    cmd: &'static str,
    started: Instant,
    /// Last press of the key, auto-repeat included.
    last: Instant,
    /// Text of the warning it showed, removed when the confirmation ends.
    notice: String,
}

pub struct App {
    pub root: PathBuf,
    /// Open documents, one per tab; `views[i]` belongs to `docs[i]`.
    pub docs: Vec<Document>,
    pub views: Vec<View>,
    pub active: usize,
    pub focus: Focus,
    pub notices: VecDeque<Notice>,
    pub keymap: Keymap,
    pub settings: Settings,
    pub clipboard: Box<dyn Clipboard>,
    pub fs_rx: Option<Receiver<FsEvent>>,
    watcher: Option<Watcher>,
    /// The terminal reports keys with the kitty protocol (all chords reachable).
    pub keyboard_enhanced: bool,
    pub quit: bool,
    /// Something changed since the last frame.
    pub dirty: bool,
    /// Areas of the last frame, for mouse hit tests and paging.
    pub layout: AppLayout,
    /// A command that asked "press again to confirm"; any other input clears it.
    confirm: Option<Confirm>,
    /// Time and auto-repeat flag of the key being handled.
    input_at: Instant,
    input_repeat: bool,
    /// Mouse drag in the editor extends the selection.
    pub(crate) dragging: bool,
    /// Where unsaved buffers go when the user discards them (None: not kept).
    pub recovery_dir: Option<PathBuf>,
}

impl App {
    pub fn new(
        root: PathBuf,
        keymap: Keymap,
        settings: Settings,
        clipboard: Box<dyn Clipboard>,
    ) -> App {
        App {
            root,
            docs: Vec::new(),
            views: Vec::new(),
            active: 0,
            focus: Focus::Editor,
            notices: VecDeque::new(),
            keymap,
            settings,
            clipboard,
            fs_rx: None,
            watcher: None,
            keyboard_enhanced: false,
            quit: false,
            dirty: true,
            layout: AppLayout::default(),
            confirm: None,
            input_at: Instant::now(),
            input_repeat: false,
            dragging: false,
            recovery_dir: None,
        }
    }

    /// Loads keymap and settings from `config_dir()`; returns the load warnings for
    /// [`App::report_startup`].
    pub fn load(root: PathBuf, clipboard: Box<dyn Clipboard>) -> (App, Vec<ConfigWarning>) {
        let dir = vr_config::config_dir();
        let (keymap, mut warnings) = Keymap::load(Some(&dir.join("keymap.toml")));
        let (settings, w) = Settings::load(Some(&dir.join("settings.toml")));
        warnings.extend(w);
        let mut app = App::new(root, keymap, settings, clipboard);
        app.recovery_dir = Some(vr_config::state_dir().join("recovery"));
        (app, warnings)
    }

    /// Terminal title (AC-1).
    pub fn title(&self) -> String {
        format!("vr-ide — {}", self.root.display())
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
        if crate::theme::Theme::named(&self.settings.theme).is_none() {
            let msg = format!(
                "settings: unknown theme {:?}; using dark",
                self.settings.theme
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
            let lost: Vec<String> = self
                .keymap
                .bindings()
                .filter(|(_, k, c)| legacy_unreachable(k) && names.contains(c))
                .map(|(_, k, c)| format!("{k} ({c})"))
                .collect();
            if !lost.is_empty() {
                let palette: Vec<String> = self
                    .keymap
                    .keys_for("app.palette")
                    .iter()
                    .filter(|k| !legacy_unreachable(k))
                    .map(ToString::to_string)
                    .collect();
                let palette = if palette.is_empty() {
                    String::new()
                } else {
                    format!("; use the command palette ({})", palette.join(" / "))
                };
                self.push_notice(Notice {
                    level: NoticeLevel::Warning,
                    text: format!(
                        "this terminal does not report these keys: {}{palette}",
                        lost.join(", ")
                    ),
                    expires: Some(Instant::now() + KEYBOARD_NOTICE_TTL),
                    until_next: true,
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
                for e in failed {
                    self.warn(format!("file watching: {e}"));
                }
            }
            Err(e) => self.warn(format!(
                "file watching is off ({e}); external changes are not detected"
            )),
        }
    }

    // ---- documents ---------------------------------------------------------------------

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
            return Ok(());
        }
        let mut doc = Document::open(&path, self.settings.max_highlight_bytes)
            .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
        doc.buffer.set_options(self.buffer_options());
        if let Some(w) = &mut self.watcher {
            doc.set_own_writes(w.own_writes());
            if let Err(e) = w.watch(&path) {
                let msg = format!("file watching: {e}");
                self.warn(msg);
            }
        }
        if doc.read_only {
            let enc = encoding_name(&doc);
            let what = if enc == "non-UTF-8" {
                "is not valid UTF-8".to_string()
            } else {
                format!("is {enc}, not UTF-8")
            };
            self.warn(format!(
                "{} {what}; opened read-only",
                self.display_path(&path)
            ));
        }
        self.docs.push(doc);
        self.views.push(View {
            follow_cursor: true,
            ..View::default()
        });
        self.set_active(self.docs.len() - 1);
        Ok(())
    }

    /// `path` made absolute (relative to the project root) with `.` and `..` removed
    /// lexically, so one file never gets two tabs.
    pub fn resolve(&self, path: &Path) -> PathBuf {
        use std::path::Component;
        let joined = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        let mut out = PathBuf::new();
        for c in joined.components() {
            match c {
                Component::CurDir => {}
                Component::ParentDir => {
                    if !out.pop() {
                        out.push(c);
                    }
                }
                other => out.push(other),
            }
        }
        out
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
        if let (Some(w), Some(p)) = (&mut self.watcher, &doc.path) {
            w.unwatch(p);
        }
        if self.active > i || self.active >= self.docs.len() {
            self.active = self.active.saturating_sub(1);
        }
        self.dirty = true;
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

    /// The active document if it may be edited; a read-only one gets a notice instead.
    /// The view follows the cursor afterwards.
    pub fn editable_doc(&mut self) -> Option<&mut Document> {
        let read_only = self.active_doc()?.read_only;
        if read_only {
            self.warn("file is read-only (not UTF-8)");
            return None;
        }
        self.cursor_moved();
        self.docs.get_mut(self.active)
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

    /// Path relative to the project root when inside it.
    pub fn display_path(&self, p: &Path) -> String {
        p.strip_prefix(&self.root)
            .unwrap_or(p)
            .display()
            .to_string()
    }

    // ---- notices -----------------------------------------------------------------------

    pub fn notify(&mut self, level: NoticeLevel, text: impl Into<String>) {
        let expires = (level == NoticeLevel::Info).then(|| Instant::now() + INFO_NOTICE_TTL);
        self.push_notice(Notice {
            level,
            text: text.into(),
            expires,
            until_next: false,
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

    /// Drops expired info notices.
    pub fn tick(&mut self, now: Instant) {
        let before = self.notices.len();
        self.notices.retain(|n| n.expires.is_none_or(|t| t > now));
        if self.notices.len() != before {
            self.dirty = true;
        }
    }

    /// The keys bound to `cmd`, for hints in notices ("F1"), never hard-coded (USER-1).
    pub fn key_hint(&self, cmd: &str) -> String {
        let keys = self.keymap.keys_for(cmd);
        if keys.is_empty() {
            format!("run {cmd}")
        } else {
            keys.iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" / ")
        }
    }

    /// "Press again to confirm" for `cmd`: true on a deliberate second press, false after
    /// showing `warning`. Auto-repeat of a held key never confirms, and a confirmation older
    /// than [`CONFIRM_TTL`] starts over.
    pub fn confirm_twice(&mut self, cmd: &'static str, warning: String) -> bool {
        let now = self.input_at;
        if let Some(c) = &mut self.confirm
            && c.cmd == cmd
            && now.saturating_duration_since(c.started) <= CONFIRM_TTL
        {
            if self.input_repeat || now.saturating_duration_since(c.last) < CONFIRM_REPEAT_GAP {
                c.last = now;
                return false;
            }
            self.cancel_confirm();
            return true;
        }
        self.cancel_confirm();
        self.warn(warning.clone());
        self.confirm = Some(Confirm {
            cmd,
            started: now,
            last: now,
            notice: warning,
        });
        false
    }

    /// Forgets a pending confirmation and removes its warning.
    pub fn cancel_confirm(&mut self) {
        if let Some(c) = self.confirm.take()
            && let Some(i) = self
                .notices
                .iter()
                .position(|n| n.level == NoticeLevel::Warning && n.text == c.notice)
        {
            self.notices.remove(i);
            self.dirty = true;
        }
    }

    /// Writes the unsaved buffers of `idxs` to the recovery directory before they are
    /// discarded, and says where.
    pub fn keep_discarded(&mut self, idxs: &[usize]) {
        let Some(dir) = self.recovery_dir.clone() else {
            return;
        };
        let docs: Vec<&Document> = idxs
            .iter()
            .filter_map(|&i| self.docs.get(i))
            .filter(|d| d.is_modified())
            .collect();
        if docs.is_empty() {
            return;
        }
        let written = crate::recovery::write_docs(&docs, &dir);
        if written.len() == docs.len() {
            self.info(format!("unsaved changes kept in {}", dir.display()));
        } else {
            self.error(format!(
                "could not write every unsaved buffer to recovery ({})",
                dir.display()
            ));
        }
    }

    // ---- input -------------------------------------------------------------------------

    /// Runs a command by name; errors become notices. Returns false for unknown names.
    pub fn run_command(&mut self, name: &str, args: CmdArgs) -> bool {
        let Some(spec) = commands::find(name) else {
            self.warn(format!("unknown command {name:?}"));
            return false;
        };
        if self.confirm.as_ref().is_some_and(|c| c.cmd != spec.name) {
            self.cancel_confirm();
        }
        if let Err(e) = (spec.run)(self, args) {
            self.error(format!("{}: {e}", spec.title));
        }
        self.dirty = true;
        true
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        self.handle_key_at(key, Instant::now());
    }

    /// [`App::handle_key`] with the time the key arrived (confirmations compare times).
    pub fn handle_key_at(&mut self, key: KeyEvent, now: Instant) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        self.input_at = now;
        self.input_repeat = key.kind == KeyEventKind::Repeat;
        self.dirty = true;
        if key.code == DISMISS_KEY && key.modifiers.is_empty() {
            if self.confirm.is_some() {
                self.cancel_confirm();
                return;
            }
            if !self.notices.is_empty() {
                self.notices.pop_front();
                return;
            }
        }
        // Built-in cursor and typing keys come first: the plan keeps them out of the keymap.
        if self.focus == Focus::Editor && commands::edit::handle_builtin_key(self, &key) {
            self.cancel_confirm();
            return;
        }
        let Some(chord) = chord_from_key(&key, self.keyboard_enhanced) else {
            return;
        };
        match self.keymap.command_for_in(self.focus.context(), &chord) {
            Some(cmd) => {
                let cmd = cmd.to_owned();
                self.run_command(&cmd, CmdArgs::None);
            }
            None => self.cancel_confirm(),
        }
    }

    /// Bracketed paste from the terminal.
    pub fn handle_paste(&mut self, text: &str) {
        self.cancel_confirm();
        if self.focus == Focus::Editor {
            commands::edit::insert_pasted(self, text);
        }
    }

    /// Click places the cursor (shift+click extends), drag selects, the wheel scrolls the
    /// view without moving the cursor, a click on a tab activates it.
    pub fn handle_mouse(&mut self, ev: MouseEvent) {
        let inside = |r: ratatui::layout::Rect| {
            ev.column >= r.x
                && ev.column < r.x + r.width
                && ev.row >= r.y
                && ev.row < r.y + r.height
        };
        let l = self.layout;
        match ev.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.cancel_confirm();
                if inside(l.tabs) {
                    if let Some(i) = crate::ui::tabs::hit(self, ev.column - l.tabs.x, l.tabs.width)
                    {
                        self.set_active(i);
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
    /// the buffer and its tab is marked (T6 adds the reload / keep prompt).
    pub fn handle_fs_event(&mut self, ev: FsEvent) {
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
        let msg = match state {
            DiskState::ChangedOnDisk if !doc.is_modified() => match doc.reload() {
                Ok(()) => None,
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

    /// Handles every pending watcher event without blocking.
    pub fn drain_fs_events(&mut self) {
        let events: Vec<FsEvent> = match &self.fs_rx {
            Some(rx) => rx.try_iter().collect(),
            None => return,
        };
        for ev in events {
            self.handle_fs_event(ev);
        }
    }
}

pub fn encoding_name(doc: &Document) -> String {
    match &doc.encoding {
        vr_core::Encoding::Utf8 => "UTF-8".to_string(),
        vr_core::Encoding::Utf8Bom => "UTF-8 BOM".to_string(),
        vr_core::Encoding::Other(name) => name.clone(),
    }
}
