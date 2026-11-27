//! Explorer panel state and the `explorer.*`, `file.goto` and `focus.cycle` commands
//! (AC-1, AC-8, AC-9). Rendering is in `ui::explorer`.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};

use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use vr_fs::{Node, NodeKind, Tree};

use super::dialog::{DialogAction, reopen_with_error};
use super::registry::{CmdArgs, CmdResult, CommandSpec};
use crate::app::{App, Focus};
use crate::ui::dialog::{ConfirmDialog, DialogWidget, PromptDialog};

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "explorer.toggle",
        title: "Explorer: Toggle",
        run: toggle,
    },
    CommandSpec {
        name: "explorer.toggle_focus",
        title: "Explorer: Toggle and Focus",
        run: toggle_focus,
    },
    CommandSpec {
        name: "explorer.focus",
        title: "Explorer: Focus",
        run: focus,
    },
    CommandSpec {
        name: "explorer.reveal",
        title: "Explorer: Reveal Active File",
        run: reveal,
    },
    CommandSpec {
        name: "explorer.new",
        title: "Explorer: New File or Folder",
        run: new_entry,
    },
    CommandSpec {
        name: "explorer.new_file",
        title: "Explorer: New File",
        run: new_file,
    },
    CommandSpec {
        name: "explorer.new_folder",
        title: "Explorer: New Folder",
        run: new_folder,
    },
    CommandSpec {
        name: "explorer.rename",
        title: "Explorer: Rename",
        run: rename,
    },
    CommandSpec {
        name: "explorer.copy",
        title: "Explorer: Copy",
        run: copy,
    },
    CommandSpec {
        name: "explorer.cut",
        title: "Explorer: Cut",
        run: cut,
    },
    CommandSpec {
        name: "explorer.paste",
        title: "Explorer: Paste",
        run: paste,
    },
    CommandSpec {
        name: "explorer.move",
        title: "Explorer: Move",
        run: move_selection,
    },
    CommandSpec {
        name: "explorer.delete",
        title: "Explorer: Delete",
        run: delete,
    },
    CommandSpec {
        name: "explorer.search_folder",
        title: "Explorer: Search in Folder",
        run: search_folder,
    },
    CommandSpec {
        name: "explorer.replace_folder",
        title: "Explorer: Replace in Folder",
        run: replace_folder,
    },
    CommandSpec {
        name: "explorer.toggle_ignored",
        title: "Explorer: Toggle Ignored Files",
        run: toggle_ignored,
    },
    CommandSpec {
        name: "explorer.toggle_hidden",
        title: "Explorer: Toggle Hidden Files",
        run: toggle_hidden,
    },
    CommandSpec {
        name: "file.goto",
        title: "Go to File",
        run: goto_file,
    },
    CommandSpec {
        name: "focus.cycle",
        title: "Focus Next Part",
        run: focus_cycle,
    },
];

/// One visible line of the explorer. Each workspace root is a row of depth 0 (W-2); its
/// children have depth 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub path: PathBuf,
    pub kind: NodeKind,
    pub ignored: bool,
    pub depth: usize,
    /// A root row: its display name (D240).
    pub label: Option<String>,
    /// A root row whose folder is gone (listed so it can be removed).
    pub missing: bool,
}

/// A workspace root as the explorer shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplorerRoot {
    pub path: PathBuf,
    pub name: String,
    /// Its folder was not found the last time it was read (never a stat per frame).
    pub missing: bool,
}

impl Row {
    pub fn name(&self) -> String {
        if let Some(l) = &self.label {
            return l.clone();
        }
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }
}

/// The project tree as shown: which folders are expanded, the listing of each, the cursor and
/// the selection. Folders are read only when expanded (huge trees cost nothing until then).
#[derive(Debug, Clone)]
pub struct Explorer {
    /// The hidden / ignored toggles (one pair for every root) and the first root; each
    /// root's folders are listed with that root's own ignore files ([`Explorer::tree_for`]).
    pub tree: Tree,
    /// The workspace roots in order (W-2), kept in step by [`Explorer::set_roots`].
    pub roots: Vec<ExplorerRoot>,
    pub visible: bool,
    /// Expanded folders; the root is always in it once loaded. Their listings are in `children`.
    pub expanded: HashSet<PathBuf>,
    children: HashMap<PathBuf, Vec<Node>>,
    pub rows: Vec<Row>,
    /// Index into `rows`.
    pub cursor: usize,
    /// Selected paths (several with shift+Up/Down or ctrl+click); empty = the cursor row.
    pub selected: BTreeSet<PathBuf>,
    anchor: Option<usize>,
    /// First visible row.
    pub top: usize,
    /// The next frame scrolls the cursor into view; the wheel clears it so the view can move
    /// away from the cursor, keys and clicks set it again.
    pub follow_cursor: bool,
    /// `top` of the last frame drawn: clicks hit the rows that are on screen, even when a
    /// wheel event handled before the next frame moved `top` already.
    pub drawn_top: usize,
    /// Paths copied with `explorer.copy` or cut with `explorer.cut`, pasted by `explorer.paste`.
    pub copied: Vec<PathBuf>,
    /// `copied` came from a cut: paste moves them (once) instead of copying.
    pub cut: bool,
    /// Panel area in the last frame (mouse hit tests, paging).
    pub area: Rect,
    loaded: bool,
    /// Files a save wrote that their folder listing leaves out (hidden or gitignored): not
    /// stale, so saving them again re-reads nothing. A re-read of the folder forgets them.
    filtered: HashSet<PathBuf>,
    /// Expanded folders to re-read on the next flush (watcher events).
    dirty: HashSet<PathBuf>,
    /// Leftover temp files already named in a notice.
    reported_leftovers: HashSet<PathBuf>,
    /// Folders with a non-recursive watch: the expanded ones, once watching started (D110).
    pub watched: HashSet<PathBuf>,
    /// `App::watch_root` ran: from then on `watched` follows `expanded`.
    pub watching: bool,
    /// The right-click menu while open (T23); it takes keys and clicks first.
    pub menu: Option<super::explorer_menu::Menu>,
}

impl Explorer {
    pub fn new(root: &Path, show_hidden: bool, show_ignored: bool) -> Explorer {
        let name = root.file_name().map_or_else(
            || root.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        Explorer {
            tree: Tree::new(root, show_hidden, show_ignored),
            roots: vec![ExplorerRoot {
                path: root.to_path_buf(),
                name,
                missing: false,
            }],
            visible: false,
            expanded: HashSet::new(),
            children: HashMap::new(),
            rows: Vec::new(),
            cursor: 0,
            selected: BTreeSet::new(),
            anchor: None,
            top: 0,
            follow_cursor: true,
            drawn_top: 0,
            copied: Vec::new(),
            cut: false,
            area: Rect::default(),
            loaded: false,
            filtered: HashSet::new(),
            dirty: HashSet::new(),
            reported_leftovers: HashSet::new(),
            watched: HashSet::new(),
            watching: false,
            menu: None,
        }
    }

    pub fn root(&self) -> &Path {
        &self.tree.root
    }

    /// Takes the workspace roots `(path, name)` in order. A root that stays keeps its
    /// expanded folders, listings and selection; what belonged to a removed root is dropped
    /// (its watches end with the next `App::sync_explorer_watches`). Returns the new roots,
    /// for the caller to read when the explorer is loaded.
    pub fn set_roots(&mut self, roots: &[(PathBuf, String)]) -> Vec<PathBuf> {
        let old = std::mem::take(&mut self.roots);
        let mut added = Vec::new();
        for (path, name) in roots {
            let missing = match old.iter().find(|r| &r.path == path) {
                Some(r) => r.missing,
                None => {
                    added.push(path.clone());
                    false
                }
            };
            self.roots.push(ExplorerRoot {
                path: path.clone(),
                name: name.clone(),
                missing,
            });
        }
        if let Some((first, _)) = roots.first() {
            self.tree.root = first.clone();
        }
        let now: Vec<PathBuf> = self.roots.iter().map(|r| r.path.clone()).collect();
        let kept = |p: &Path| now.iter().any(|r| p.starts_with(r));
        self.expanded.retain(|p| kept(p));
        self.children.retain(|p, _| kept(p));
        self.selected.retain(|p| kept(p));
        self.copied.retain(|p| kept(p));
        if self.copied.is_empty() {
            self.cut = false;
        }
        self.filtered.retain(|p| kept(p));
        self.dirty.retain(|p| kept(p));
        if self.loaded {
            self.rebuild();
        }
        added
    }

    /// More than one root: roots collapse, show their names, and moves stay in their root.
    pub fn is_multi(&self) -> bool {
        self.roots.len() > 1
    }

    /// The root `p` lies in.
    pub fn root_of(&self, p: &Path) -> Option<&ExplorerRoot> {
        self.roots.iter().find(|r| p.starts_with(&r.path))
    }

    /// `p` is one of the roots.
    pub fn is_root(&self, p: &Path) -> bool {
        self.roots.iter().any(|r| r.path == p)
    }

    /// A tree listing folders of the root `p` is in: that root's ignore files with the
    /// shared hidden / ignored toggles.
    pub fn tree_for(&self, p: &Path) -> Option<Tree> {
        self.root_of(p)
            .map(|r| Tree::new(&r.path, self.tree.show_hidden(), self.tree.show_ignored()))
    }

    /// Lists `dir` with its root's tree; reading a root's own folder also updates its
    /// missing mark (the only place the explorer learns it, so no frame stats a root).
    fn list(&mut self, dir: &Path) -> io::Result<Vec<Node>> {
        let listed = match self.tree_for(dir) {
            Some(t) => t.children(dir),
            None => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} is outside every workspace folder", dir.display()),
            )),
        };
        if let Some(r) = self.roots.iter_mut().find(|r| r.path == dir) {
            r.missing = matches!(&listed, Err(e) if e.kind() == io::ErrorKind::NotFound);
        }
        listed
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// The row under the cursor.
    pub fn current(&self) -> Option<&Row> {
        self.rows.get(self.cursor)
    }

    /// Reads `dir` (again) and shows it expanded. On an error it stays as it was.
    pub fn expand(&mut self, dir: &Path) -> io::Result<()> {
        let nodes = match self.list(dir) {
            Ok(n) => n,
            Err(e) => {
                // A root found missing shows its mark at once.
                if self.loaded {
                    self.rebuild();
                }
                return Err(e);
            }
        };
        self.children.insert(dir.to_path_buf(), nodes);
        self.expanded.insert(dir.to_path_buf());
        self.rebuild();
        Ok(())
    }

    /// Collapses `dir` (never the only root); folders below it are read again when shown.
    pub fn collapse(&mut self, dir: &Path) {
        if !self.is_multi() && dir == self.root() {
            return;
        }
        self.expanded.retain(|p| !p.starts_with(dir));
        self.children.retain(|p, _| !p.starts_with(dir));
        self.rebuild();
    }

    /// Re-reads an expanded folder; one that cannot be read any more (removed, renamed) is
    /// collapsed with everything below it. Rows are rebuilt by the caller.
    fn reload(&mut self, dir: &Path) {
        if !self.expanded.contains(dir) {
            return;
        }
        #[cfg(test)]
        RELOADS.with(|c| c.set(c.get() + 1));
        self.filtered.retain(|p| p.parent() != Some(dir));
        match self.list(dir) {
            Ok(nodes) => {
                self.children.insert(dir.to_path_buf(), nodes);
            }
            Err(_) if self.is_root(dir) => {
                self.children.insert(dir.to_path_buf(), Vec::new());
            }
            Err(_) => {
                self.expanded.retain(|p| !p.starts_with(dir));
                self.children.retain(|p, _| !p.starts_with(dir));
            }
        }
    }

    /// Re-reads every expanded folder (settings toggles, rescans).
    pub fn reload_all(&mut self) {
        let mut dirs: Vec<PathBuf> = self.expanded.iter().cloned().collect();
        // Parents first, so a vanished parent drops its children before they are read.
        dirs.sort_by_key(|p| p.components().count());
        for d in dirs {
            self.reload(&d);
        }
        self.rebuild();
    }

    /// Flattens the expanded tree into rows. The cursor stays on its path (or near its old
    /// place when that path is gone); selected paths that are gone are dropped.
    pub fn rebuild(&mut self) {
        let keep = self.current().map(|r| r.path.clone());
        let mut rows = Vec::new();
        for root in &self.roots {
            rows.push(Row {
                path: root.path.clone(),
                kind: NodeKind::Dir,
                ignored: false,
                depth: 0,
                label: Some(root.name.clone()),
                missing: root.missing,
            });
            if self.expanded.contains(&root.path) {
                self.push_children(&root.path, &mut rows);
            }
        }
        let old = self.cursor;
        self.rows = rows;
        self.cursor = keep
            .and_then(|p| self.rows.iter().position(|r| r.path == p))
            .unwrap_or(old.min(self.rows.len() - 1));
        let shown: HashSet<&PathBuf> = self.rows.iter().map(|r| &r.path).collect();
        self.selected.retain(|p| shown.contains(p));
    }

    fn push_children(&self, dir: &Path, rows: &mut Vec<Row>) {
        let Some(nodes) = self.children.get(dir) else {
            return;
        };
        for n in nodes {
            rows.push(Row {
                path: n.path.clone(),
                kind: n.kind,
                ignored: n.ignored,
                depth: n.depth + 1,
                label: None,
                missing: false,
            });
            if n.kind.is_expandable() && self.expanded.contains(&n.path) {
                self.push_children(&n.path, rows);
            }
        }
    }
}

/// Reads the root listing the first time the explorer is needed.
pub fn ensure_loaded(app: &mut App) {
    if app.explorer.loaded {
        return;
    }
    app.explorer.loaded = true;
    let roots: Vec<PathBuf> = app.explorer.roots.iter().map(|r| r.path.clone()).collect();
    for root in roots {
        load_root(app, &root);
    }
    app.explorer.rebuild();
}

/// Reads a root's own folder and shows it expanded; a folder that cannot be read stays
/// listed (a missing one marked so) with nothing under it.
fn load_root(app: &mut App, root: &Path) {
    // Named already when the watches started before the explorer was shown (D292).
    let named = app
        .explorer
        .roots
        .iter()
        .any(|r| r.path == root && r.missing);
    if let Err(e) = app.explorer.expand(root) {
        app.explorer.expanded.insert(root.to_path_buf());
        app.explorer.children.insert(root.to_path_buf(), Vec::new());
        app.explorer.rebuild();
        // Only another root can stay when this one goes (D241).
        if app.explorer.is_multi()
            && app
                .explorer
                .roots
                .iter()
                .any(|r| r.path == root && r.missing)
        {
            if !named {
                missing_root_notice(app, root);
            }
        } else {
            app.warn(format!("explorer: cannot read {}: {e}", root.display()));
        }
        return;
    }
    report_leftovers(app, root);
}

/// The one notice for a workspace folder that is gone (D292: no watch error besides it).
pub(crate) fn missing_root_notice(app: &mut App, root: &Path) {
    app.warn(format!(
        "explorer: the workspace folder {} is missing; Remove Folder takes it out of the \
         workspace",
        root.display()
    ));
}

/// The workspace roots changed (W-2): the explorer takes the new list and, when loaded,
/// reads the roots that are new; the cursor stays on its row when that row is still shown.
pub fn sync_roots(app: &mut App) {
    let roots: Vec<(PathBuf, String)> = app
        .workspace
        .roots()
        .iter()
        .map(|r| (r.path.clone(), r.name.clone()))
        .collect();
    let added = app.explorer.set_roots(&roots);
    if app.explorer.loaded {
        for root in added {
            load_root(app, &root);
        }
        app.explorer.rebuild();
    }
    app.dirty = true;
}

/// Shows or hides the panel; hiding it while focused gives the editor the keys.
fn toggle(app: &mut App, _: CmdArgs) -> CmdResult {
    app.explorer.visible = !app.explorer.visible;
    app.explorer.menu = None;
    if app.explorer.visible {
        ensure_loaded(app);
    } else if app.focus == Focus::Explorer {
        app.focus = Focus::Editor;
    }
    Ok(())
}

/// Shown explorer: hides it and the editor gets the keys. Hidden: shows it and gives it the
/// keys, so the arrows move in the tree at once.
fn toggle_focus(app: &mut App, _: CmdArgs) -> CmdResult {
    if app.explorer.visible {
        app.explorer.visible = false;
        app.explorer.menu = None;
        app.focus = Focus::Editor;
    } else {
        show_and_focus(app);
    }
    Ok(())
}

/// Shows the panel and gives it the keys.
fn focus(app: &mut App, _: CmdArgs) -> CmdResult {
    show_and_focus(app);
    Ok(())
}

fn show_and_focus(app: &mut App) {
    app.explorer.visible = true;
    ensure_loaded(app);
    app.focus = Focus::Explorer;
    app.dirty = true;
}

/// Moves the cursor to row `i` (clamped). `extend` grows the selection from the anchor
/// (shift+arrows); otherwise the selection is just the cursor row.
fn move_cursor(app: &mut App, i: isize, extend: bool) {
    let ex = &mut app.explorer;
    if ex.rows.is_empty() {
        return;
    }
    let last = ex.rows.len() as isize - 1;
    let to = i.clamp(0, last) as usize;
    if extend {
        let anchor = *ex.anchor.get_or_insert(ex.cursor);
        let (a, b) = (anchor.min(to), anchor.max(to));
        ex.selected = ex.rows[a..=b].iter().map(|r| r.path.clone()).collect();
    } else {
        ex.anchor = None;
        ex.selected.clear();
    }
    ex.cursor = to;
    ex.follow_cursor = true;
    app.dirty = true;
}

/// Enter / click on a row: a folder opens or closes, a file opens in a tab (`to_editor`
/// then gives the editor the keys).
fn activate(app: &mut App, to_editor: bool) {
    let Some(row) = app.explorer.current().cloned() else {
        return;
    };
    if row.kind.is_expandable() {
        if app.explorer.expanded.contains(&row.path) {
            app.explorer.collapse(&row.path);
        } else {
            expand_dir(app, &row.path);
        }
        return;
    }
    match row.kind {
        NodeKind::File | NodeKind::Symlink(vr_fs::SymlinkTarget::File) => {
            match app.open_path(&row.path) {
                Ok(()) => {
                    if to_editor {
                        app.focus = Focus::Editor;
                    }
                }
                Err(e) => super::search::report_open_error(app, &e),
            }
        }
        _ => app.warn(format!(
            "{} is a broken or looping link; it cannot be opened",
            app.display_path(&row.path)
        )),
    }
}

/// Expands `dir`, reporting a folder that cannot be read.
fn expand_dir(app: &mut App, dir: &Path) {
    if let Err(e) = app.explorer.expand(dir) {
        let msg = format!("cannot open folder {}: {e}", app.display_path(dir));
        app.warn(msg);
    } else {
        report_leftovers(app, dir);
    }
}

/// Built-in explorer keys (arrows, shift+Up/Down, Enter, Home/End, PageUp/PageDown, Esc);
/// true when used. Other keys go to the keymap (`[explorer]`, then global); the rest are
/// swallowed so typing never reaches a buffer from the explorer.
pub fn handle_builtin_key(app: &mut App, key: &KeyEvent) -> bool {
    let m = key.modifiers;
    if m.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
        return false;
    }
    let shift = m.contains(KeyModifiers::SHIFT);
    let cur = app.explorer.cursor as isize;
    let page = (app.explorer.area.height.max(2) - 1) as isize;
    match key.code {
        KeyCode::Up => move_cursor(app, cur - 1, shift),
        KeyCode::Down => move_cursor(app, cur + 1, shift),
        KeyCode::PageUp => move_cursor(app, cur - page, shift),
        KeyCode::PageDown => move_cursor(app, cur + page, shift),
        KeyCode::Home => move_cursor(app, 0, shift),
        KeyCode::End => move_cursor(app, isize::MAX, shift),
        KeyCode::Right => {
            let Some(row) = app.explorer.current().cloned() else {
                return true;
            };
            if row.kind.is_expandable() {
                if app.explorer.expanded.contains(&row.path) {
                    // Already open: step onto its first child, if any.
                    let next = app.explorer.rows.get(app.explorer.cursor + 1);
                    if next.is_some_and(|n| n.path.parent() == Some(&row.path)) {
                        move_cursor(app, cur + 1, false);
                    }
                } else {
                    expand_dir(app, &row.path);
                }
            }
        }
        KeyCode::Left => {
            let Some(row) = app.explorer.current().cloned() else {
                return true;
            };
            if (row.depth > 0 || app.explorer.is_multi())
                && row.kind.is_expandable()
                && app.explorer.expanded.contains(&row.path)
            {
                app.explorer.collapse(&row.path);
            } else if let Some(parent) = row.path.parent()
                && let Some(i) = app.explorer.rows.iter().position(|r| r.path == parent)
            {
                move_cursor(app, i as isize, false);
            }
        }
        KeyCode::Enter if key.kind == KeyEventKind::Repeat => {}
        KeyCode::Enter => activate(app, true),
        KeyCode::Esc => app.focus = Focus::Editor,
        // Tab moves the focus on, as in the reading editor (T21).
        KeyCode::Tab if m.is_empty() => {
            app.run_command("focus.cycle", CmdArgs::None);
            return true;
        }
        KeyCode::Char(_) | KeyCode::Backspace | KeyCode::Tab | KeyCode::BackTab
            if m.difference(KeyModifiers::SHIFT).is_empty() =>
        {
            // Nothing in the explorer takes text; never let it fall through to an editor.
            return true;
        }
        _ => return false,
    }
    app.explorer.follow_cursor = true;
    app.dirty = true;
    true
}

/// Click selects (a folder toggles, a file opens and the explorer keeps the keys),
/// ctrl+click adds to or removes from the selection, shift+click selects a range, the wheel
/// scrolls. A right press selects its row (the root below the last row) and opens the
/// menu of the explorer's key actions there (T23).
pub fn handle_mouse(app: &mut App, ev: &MouseEvent) {
    let area = app.explorer.area;
    match ev.kind {
        MouseEventKind::Down(MouseButton::Right) => {
            app.focus = Focus::Explorer;
            let i = app.explorer.drawn_top + usize::from(ev.row.saturating_sub(area.y));
            let in_selection = app
                .explorer
                .rows
                .get(i)
                .is_some_and(|r| app.explorer.selected.contains(&r.path));
            if i >= app.explorer.rows.len() {
                // Below the last row: the root those rows belong to (the last one).
                let last_root = app.explorer.rows.iter().rposition(|r| r.depth == 0);
                move_cursor(app, last_root.unwrap_or(0) as isize, false);
            } else if in_selection {
                // A row of a multi-selection: the actions apply to the whole selection, as
                // its keys do.
                app.explorer.cursor = i;
                app.explorer.follow_cursor = true;
            } else {
                move_cursor(app, i as isize, false);
            }
            super::explorer_menu::open(app, ev.column, ev.row);
        }
        MouseEventKind::Down(MouseButton::Left) => {
            app.focus = Focus::Explorer;
            let i = app.explorer.drawn_top + usize::from(ev.row.saturating_sub(area.y));
            if i >= app.explorer.rows.len() {
                return;
            }
            app.explorer.follow_cursor = true;
            if ev.modifiers.contains(KeyModifiers::CONTROL) {
                let ex = &mut app.explorer;
                if ex.selected.is_empty()
                    && let Some(r) = ex.current()
                {
                    let p = r.path.clone();
                    ex.selected.insert(p);
                }
                let p = ex.rows[i].path.clone();
                if !ex.selected.remove(&p) {
                    ex.selected.insert(p);
                }
                ex.cursor = i;
                ex.anchor = Some(i);
            } else if ev.modifiers.contains(KeyModifiers::SHIFT) {
                move_cursor(app, i as isize, true);
            } else {
                move_cursor(app, i as isize, false);
                activate(app, false);
            }
        }
        MouseEventKind::ScrollDown => {
            let ex = &mut app.explorer;
            let last_top = ex
                .rows
                .len()
                .saturating_sub(usize::from(area.height).max(1));
            ex.top = (ex.top + crate::app::WHEEL_LINES).min(last_top);
            ex.follow_cursor = false;
        }
        MouseEventKind::ScrollUp => {
            let ex = &mut app.explorer;
            ex.top = ex.top.saturating_sub(crate::app::WHEEL_LINES);
            ex.follow_cursor = false;
        }
        _ => {}
    }
    app.dirty = true;
}

/// Ignore rules changed: files that were filtered out may be listed now.
fn is_ignore_file_name(p: &Path) -> bool {
    p.file_name()
        .is_some_and(|n| n == ".gitignore" || n == ".ignore")
}

fn is_ignore_file(ev: &vr_core::FsEvent) -> bool {
    use vr_core::FsEvent;
    match ev {
        FsEvent::Created(p) | FsEvent::Removed(p) | FsEvent::Modified(p) => is_ignore_file_name(p),
        FsEvent::Renamed(a, b) => is_ignore_file_name(a) || is_ignore_file_name(b),
        _ => false,
    }
}

/// Notes the expanded folders a watcher event touched; [`flush`] re-reads them (once per
/// batch of events). Content changes of listed files cost nothing.
pub fn on_fs_event(app: &mut App, ev: &vr_core::FsEvent) {
    use vr_core::FsEvent;
    let ex = &mut app.explorer;
    if !ex.loaded {
        return;
    }
    if is_ignore_file(ev) {
        ex.filtered.clear();
    }
    let touch = |p: &Path, ex: &mut Explorer| {
        if let Some(parent) = p.parent()
            && ex.expanded.contains(parent)
        {
            ex.dirty.insert(parent.to_path_buf());
        }
        if ex.expanded.contains(p) {
            ex.dirty.insert(p.to_path_buf());
        }
    };
    match ev {
        FsEvent::Created(p) | FsEvent::Removed(p) => {
            touch(p, ex);
        }
        FsEvent::Renamed(a, b) => {
            touch(a, ex);
            touch(b, ex);
        }
        // A path the listing does not know yet (a create reported as a change).
        FsEvent::Modified(p) => {
            let known = p
                .parent()
                .and_then(|d| ex.children.get(d))
                .is_none_or(|nodes| nodes.iter().any(|n| &n.path == p));
            if !known {
                touch(p, ex);
            }
        }
        FsEvent::Rescan(dir) => {
            let under: Vec<PathBuf> = ex
                .expanded
                .iter()
                .filter(|e| e.starts_with(dir) || dir.starts_with(e))
                .cloned()
                .collect();
            ex.dirty.extend(under);
        }
        FsEvent::Error(_) => {}
    }
}

/// Re-reads the folders noted by [`on_fs_event`], keeping expansion, cursor and selection.
pub fn flush(app: &mut App) {
    let ex = &mut app.explorer;
    if ex.dirty.is_empty() {
        return;
    }
    let mut dirs: Vec<PathBuf> = ex.dirty.drain().collect();
    dirs.sort_by_key(|p| p.components().count());
    for d in dirs {
        ex.reload(&d);
    }
    ex.rebuild();
    app.dirty = true;
}

#[cfg(test)]
thread_local! {
    static RELOADS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Folder re-reads done by this thread so far (tests count them).
#[cfg(test)]
pub fn reload_count() -> usize {
    RELOADS.with(std::cell::Cell::get)
}

/// A save of ours wrote `path`. The watcher drops events of our own writes (folder watches
/// included), so a file a save created (Save As to a new name, a save recreating a file
/// deleted on disk) would never get its row: the shown folder holding it is re-read when
/// its listing lacks the file. Content saves of listed files cost nothing.
pub fn note_written(app: &mut App, path: &Path) {
    let ex = &mut app.explorer;
    if is_ignore_file_name(path) {
        ex.filtered.clear();
    }
    let Some(dir) = path.parent() else {
        return;
    };
    if !ex.loaded || !ex.expanded.contains(dir) {
        return;
    }
    let listed = ex
        .children
        .get(dir)
        .is_some_and(|nodes| nodes.iter().any(|n| n.path == path));
    if !listed && !ex.filtered.contains(path) {
        ex.reload(dir);
        let now_listed = ex
            .children
            .get(dir)
            .is_some_and(|nodes| nodes.iter().any(|n| n.path == path));
        if !now_listed {
            ex.filtered.insert(path.to_path_buf());
        }
        ex.rebuild();
        app.dirty = true;
    }
}

// ---- file operations ------------------------------------------------------------------------
//
// Every path an operation acts on goes through `ensure_within(root, ..)` and the operation
// uses the path it returns. Typed names pass `validate_name` (one component: no '/', no
// `..`). The root itself is never renamed, moved or trashed. Writes happen only through
// `vr_fs::ops`, which never replace an existing path.

/// The workspace root `p` is in (W-2: each operation is confined to its own root, D109);
/// the first root for a path in none (then `ensure_within` refuses it).
fn root_for(app: &App, p: &Path) -> PathBuf {
    app.explorer
        .root_of(p)
        .map_or_else(|| app.explorer.root().to_path_buf(), |r| r.path.clone())
}

/// `p` confined to its workspace root, as the path to act on.
fn within(app: &App, p: &Path) -> Result<PathBuf, String> {
    vr_fs::ensure_within(&root_for(app, p), p).map_err(|e| describe(app, &e))
}

/// Explorer moves never cross workspace roots (spec W); a copy does.
fn cross_root(app: &App, from: &Path) -> String {
    format!(
        "{} was not moved: moving between workspace folders is refused; copy and paste it \
         instead",
        app.display_path(from)
    )
}

/// `dest` (lexically in `root`) resolves into another workspace root, e.g. through a
/// folder link `api/link -> ../web/sub`. A path that cannot be resolved, or that leads
/// outside every root, is left to the existing checks (D109).
fn really_in_another_root(app: &App, dest: &Path, root: &Path) -> bool {
    let Ok(real) = std::fs::canonicalize(dest) else {
        return false;
    };
    app.explorer.roots.iter().any(|r| {
        r.path != root
            && !r.missing
            && std::fs::canonicalize(&r.path).is_ok_and(|rp| real.starts_with(rp))
    })
}

/// The folder new entries and pastes go to: the cursor row when it is a folder, else its
/// parent.
fn target_dir(app: &App) -> PathBuf {
    match app.explorer.current() {
        Some(r) if r.kind.is_expandable() => r.path.clone(),
        Some(r) => r
            .path
            .parent()
            .map_or_else(|| root_for(app, &r.path), Path::to_path_buf),
        None => app.explorer.root().to_path_buf(),
    }
}

/// "in src/Controller" / "in the project folder", for prompt labels.
fn place(app: &App, dir: &Path) -> String {
    let rel = app.display_path(dir);
    if rel.is_empty() {
        "in the project folder".to_string()
    } else {
        format!("in {rel}")
    }
}

/// Puts the cursor on `path` when it is shown (selection = that row).
fn select_path(app: &mut App, path: &Path) {
    if let Some(i) = app.explorer.rows.iter().position(|r| r.path == path) {
        app.explorer.cursor = i;
        app.explorer.selected.clear();
        app.explorer.anchor = None;
        app.explorer.follow_cursor = true;
    }
}

fn new_file(app: &mut App, _: CmdArgs) -> CmdResult {
    prompt_new(app, false);
    Ok(())
}

fn new_folder(app: &mut App, _: CmdArgs) -> CmdResult {
    prompt_new(app, true);
    Ok(())
}

/// ctrl+n in the explorer: File / Folder (File first), then the name prompt.
fn new_entry(app: &mut App, _: CmdArgs) -> CmdResult {
    ensure_loaded(app);
    let dir = target_dir(app);
    let message = format!("Create a file or a folder {}?", place(app, &dir));
    app.open_dialog(
        DialogWidget::Confirm(ConfirmDialog::new("New", message, &["File", "Folder"])),
        DialogAction::NewKind { dir },
    );
    Ok(())
}

fn prompt_new(app: &mut App, folder: bool) {
    ensure_loaded(app);
    let dir = target_dir(app);
    prompt_new_in(app, dir, folder);
}

/// The name prompt for a new file or folder in `dir`.
pub fn prompt_new_in(app: &mut App, dir: PathBuf, folder: bool) {
    let (title, what) = if folder {
        ("New Folder", "Folder name")
    } else {
        ("New File", "File name")
    };
    let label = format!("{what} ({})", place(app, &dir));
    app.open_dialog(
        DialogWidget::Prompt(PromptDialog::new(title, label, "")),
        DialogAction::NewEntry { dir, folder },
    );
}

/// Creates the typed file (then opens it) or folder in `dir`.
pub fn answer_new(app: &mut App, widget: DialogWidget, dir: PathBuf, folder: bool, text: &str) {
    let action = DialogAction::NewEntry {
        dir: dir.clone(),
        folder,
    };
    let path = match vr_fs::validate_name(text)
        .map_err(|e| describe(app, &e))
        .and_then(|()| within(app, &dir.join(text)))
    {
        Ok(p) => p,
        Err(e) => return reopen_with_error(app, widget, action, e),
    };
    let made = if folder {
        vr_fs::ops::create_dir(&path)
    } else {
        vr_fs::ops::create_file(&path)
    };
    if let Err(e) = made {
        let msg = describe(app, &e);
        return reopen_with_error(app, widget, action, msg);
    }
    expand_dir(app, &dir);
    select_path(app, &path);
    if folder {
        app.focus = Focus::Explorer;
    } else {
        match app.open_path(&path) {
            // A new file is made to be written: it opens in edit mode (D145, ctrl+n).
            Ok(()) => {
                app.focus = Focus::Editor;
                app.enter_edit_mode();
            }
            Err(e) => super::search::report_open_error(app, &e),
        }
    }
}

const ROOT_REFUSED: &str =
    "the project folder itself cannot be renamed, moved or deleted from the explorer";

/// `path` confined to the project and strictly below its root: what rename, move and trash
/// act on. `ensure_within` is lexical, so a path inside a linked folder that leads outside
/// the project passes it; those are refused here (D109), the link itself is not (its folder
/// is inside).
fn below_root(app: &App, path: &Path) -> Result<PathBuf, String> {
    below_root_of(app, &root_for(app, path), path)
}

/// [`below_root`] for the workspace root `root` (`/rename` of a file in any root).
fn below_root_of(app: &App, root: &Path, path: &Path) -> Result<PathBuf, String> {
    let within = |p: &Path| vr_fs::ensure_within(root, p).map_err(|e| describe(app, &e));
    let p = within(path)?;
    let root = within(Path::new(""))?;
    if p == root {
        return Err(ROOT_REFUSED.to_string());
    }
    if let Some(parent) = p.parent()
        && let (Ok(real_root), Ok(real_parent)) =
            (std::fs::canonicalize(&root), std::fs::canonicalize(parent))
        && !real_parent.starts_with(&real_root)
    {
        return Err(format!(
            "{} is in a linked folder outside the project ({}); rename, move and delete are \
             refused there",
            app.display_path(&p),
            real_parent.display()
        ));
    }
    Ok(p)
}

fn rename(app: &mut App, _: CmdArgs) -> CmdResult {
    ensure_loaded(app);
    let Some(row) = app.explorer.current().cloned() else {
        return Ok(());
    };
    if row.depth == 0 {
        app.warn(ROOT_REFUSED);
        return Ok(());
    }
    let label = format!("New name for {}", app.display_path(&row.path));
    app.open_dialog(
        DialogWidget::Prompt(PromptDialog::new("Rename", label, &row.name())),
        DialogAction::Rename(row.path),
    );
    Ok(())
}

/// Renames `path` to the typed name in the same folder; open tabs follow.
pub fn answer_rename(app: &mut App, widget: DialogWidget, path: PathBuf, text: &str) {
    let action = DialogAction::Rename(path.clone());
    let root = root_for(app, &path);
    match rename_in(app, &root, &path, text) {
        Ok(Some(to)) => select_path(app, &to),
        Ok(None) => {}
        Err(e) => reopen_with_error(app, widget, action, e),
    }
}

/// Renames `path` (strictly below the workspace root `root`) to the name `text` in the same
/// folder, with the explorer's checks: one valid name, inside the root, never over an
/// existing path or a path an open tab holds. Open tabs follow. `Ok(None)`: the same name.
pub fn rename_in(
    app: &mut App,
    root: &Path,
    path: &Path,
    text: &str,
) -> Result<Option<PathBuf>, String> {
    vr_fs::validate_name(text).map_err(|e| describe(app, &e))?;
    let from = below_root_of(app, root, path)?;
    let parent = from.parent().map(Path::to_path_buf).unwrap_or_default();
    let to = vr_fs::ensure_within(root, &parent.join(text)).map_err(|e| describe(app, &e))?;
    if from == to {
        return Ok(None);
    }
    check_tab_free(app, &from, &to)?;
    vr_fs::ops::rename(&from, &to).map_err(|e| describe(app, &e))?;
    retarget(app, &from, &to);
    refresh_after(app, &[&from, &to]);
    Ok(Some(to))
}

/// Tabs of `from` (a file, or every file under a folder) now point at `to`, unsaved edits
/// included, and the watcher and the project search results follow them; so do the
/// recent-files list and the git marks (the old path's are dropped, the new path asks
/// afresh). Every rename and move (F2, the menu, `/rename`, cut + paste, Move to) ends here.
fn retarget(app: &mut App, from: &Path, to: &Path) {
    let moved_path = |old: &Path| -> Option<PathBuf> {
        let rest = old.strip_prefix(from).ok()?;
        Some(if rest.as_os_str().is_empty() {
            to.to_path_buf()
        } else {
            to.join(rest)
        })
    };
    let mut moved = Vec::new();
    for doc in &mut app.docs {
        let Some(old) = doc.path.clone() else {
            continue;
        };
        let Some(new) = moved_path(&old) else {
            continue;
        };
        doc.path = Some(new.clone());
        moved.push((old, new));
    }
    for (old, new) in &moved {
        app.rewatch(Some(old), new);
        app.git.forget_doc(old);
        app.git.forget_doc(new);
    }
    // Oldest first, so the moved entries keep their order at the front of the list.
    let recent: Vec<(PathBuf, PathBuf)> = app
        .recent_files
        .entries
        .iter()
        .rev()
        .filter_map(|old| moved_path(old).map(|new| (old.clone(), new)))
        .collect();
    for (old, new) in recent {
        app.recent_file_renamed(&old, &new);
    }
    super::search::paths_moved(app, from, to);
    app.dirty = true;
}

/// Refuses moving `from` to `to` while a tab not under `from` holds `to` or a path under it
/// (a tab whose file was deleted on disk keeps its path, D82): one path never gets two tabs
/// (D95), as Save As refuses too.
fn check_tab_free(app: &App, from: &Path, to: &Path) -> Result<(), String> {
    // An existing `to` is refused by vr-fs as taken; only a name free on disk can still be
    // held by a tab.
    if to.symlink_metadata().is_ok() {
        return Ok(());
    }
    let held = app
        .docs
        .iter()
        .filter_map(|d| d.path.as_deref())
        .find(|p| p.starts_with(to) && !p.starts_with(from));
    match held {
        Some(p) => Err(format!(
            "{} is open in a tab (its file is not on disk); close that tab first",
            app.display_path(p)
        )),
        None => Ok(()),
    }
}

/// Re-reads the shown folders holding `paths` after an operation of ours.
fn refresh_after(app: &mut App, paths: &[&Path]) {
    for p in paths {
        if let Some(parent) = p.parent() {
            app.explorer.reload(parent);
        }
        app.explorer.reload(p);
    }
    app.explorer.rebuild();
}

/// Remembers the selection for `explorer.paste` (the root is never copied).
fn copy(app: &mut App, _: CmdArgs) -> CmdResult {
    remember(app, false);
    Ok(())
}

/// Like copy, but paste moves the paths (D108).
fn cut(app: &mut App, _: CmdArgs) -> CmdResult {
    remember(app, true);
    Ok(())
}

fn remember(app: &mut App, cut: bool) {
    ensure_loaded(app);
    let (paths, had_root) = targets(app);
    if paths.is_empty() {
        if had_root {
            app.warn(if cut {
                ROOT_REFUSED
            } else {
                "the project folder itself cannot be copied"
            });
        }
        return;
    }
    let msg = format!(
        "{} {}; paste into a folder with {}",
        if cut { "cut" } else { "copied" },
        list(app, &paths),
        app.key_hint("explorer.paste")
    );
    app.explorer.copied = paths;
    app.explorer.cut = cut;
    app.info(msg);
}

/// `name` for the `n`-th try: "main.rs", "main copy.rs", "main copy 2.rs", ...
fn copy_name(name: &str, n: usize) -> String {
    if n == 0 {
        return name.to_string();
    }
    let p = Path::new(name);
    let (stem, ext) = match (p.file_stem(), p.extension()) {
        (Some(s), Some(e)) => (s.to_string_lossy(), format!(".{}", e.to_string_lossy())),
        _ => (name.into(), String::new()),
    };
    if n == 1 {
        format!("{stem} copy{ext}")
    } else {
        format!("{stem} copy {n}{ext}")
    }
}

/// Copies under a free name in `dest`: an existing name is never replaced (vr-fs refuses it
/// atomically), the next "copy" name is tried instead.
fn copy_into(app: &App, src: &Path, dest: &Path) -> Result<PathBuf, String> {
    let from = within(app, src)?;
    let name = from
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| format!("{} has no file name", app.display_path(&from)))?;
    for n in 0..1000 {
        let to = within(app, &dest.join(copy_name(&name, n)))?;
        // Pasting into the source's own folder: its own name is taken by itself, and vr-fs
        // would refuse a folder as "into itself" before reporting the name as taken.
        if to == from {
            continue;
        }
        match vr_fs::ops::copy(&from, &to) {
            Ok(()) => return Ok(to),
            Err(vr_fs::FsOpError::AlreadyExists(p)) if p == to => continue,
            Err(e) => return Err(describe(app, &e)),
        }
    }
    Err(format!("no free name for a copy of {name}"))
}

/// Copies the remembered paths into the folder under the cursor.
fn paste(app: &mut App, _: CmdArgs) -> CmdResult {
    ensure_loaded(app);
    if app.explorer.copied.is_empty() {
        let msg = format!(
            "nothing to paste; copy files in the explorer first ({})",
            app.key_hint("explorer.copy")
        );
        app.info(msg);
        return Ok(());
    }
    let dest = match within(app, &target_dir(app)) {
        Ok(d) => d,
        Err(e) => anyhow::bail!(e),
    };
    if app.explorer.cut {
        // A cut is pasted once; whatever did not move stays where it was.
        let paths = std::mem::take(&mut app.explorer.copied);
        app.explorer.cut = false;
        let (moved, errors) = move_paths(app, paths, &dest);
        finish_batch(app, &dest, &moved, &errors, "moved", "not moved");
        return Ok(());
    }
    let mut made = Vec::new();
    let mut errors = Vec::new();
    for src in app.explorer.copied.clone() {
        match copy_into(app, &src, &dest) {
            Ok(to) => made.push(to),
            Err(e) => errors.push(e),
        }
    }
    finish_batch(app, &dest, &made, &errors, "pasted", "not pasted");
    Ok(())
}

/// After a paste or move: re-read `dest`, show it open with the first new item selected,
/// and report what worked and each failure.
fn finish_batch(
    app: &mut App,
    dest: &Path,
    made: &[PathBuf],
    errors: &[String],
    done: &str,
    failed: &str,
) {
    app.explorer.reload_all();
    if !made.is_empty() {
        expand_dir(app, dest);
        select_path(app, &made[0]);
        let msg = format!("{done} {}", list(app, made));
        app.info(msg);
    }
    if !errors.is_empty() {
        app.error(format!("{failed}: {}", errors.join("; ")));
    }
}

/// Asks for the folder to move the selection into.
fn move_selection(app: &mut App, _: CmdArgs) -> CmdResult {
    ensure_loaded(app);
    let (paths, had_root) = targets(app);
    if paths.is_empty() {
        if had_root {
            app.warn(ROOT_REFUSED);
        }
        return Ok(());
    }
    let from_dir = paths[0].parent().map(Path::to_path_buf).unwrap_or_default();
    let label = if app.explorer.is_multi() {
        format!(
            "Folder to move {} into (as shown: folder name/path, the same workspace folder)",
            list(app, &paths)
        )
    } else {
        format!(
            "Folder to move {} into (relative to the project folder)",
            list(app, &paths)
        )
    };
    let initial = app.display_path(&from_dir);
    app.open_dialog(
        DialogWidget::Prompt(PromptDialog::new("Move", label, &initial)),
        DialogAction::MoveTo(paths),
    );
    Ok(())
}

/// Moves each path into the typed folder (never over an existing name); tabs follow.
pub fn answer_move(app: &mut App, widget: DialogWidget, paths: Vec<PathBuf>, text: &str) {
    let action = DialogAction::MoveTo(paths.clone());
    let from_root = paths
        .first()
        .map_or_else(|| app.explorer.root().to_path_buf(), |p| root_for(app, p));
    let typed = typed_folder(app, text, &from_root);
    let dest = match within(app, &typed) {
        Ok(d) if root_for(app, &d) != from_root => {
            let msg = cross_root(app, &paths[0]);
            return reopen_with_error(app, widget, action, msg);
        }
        Ok(d) if d.is_dir() => d,
        Ok(d) => {
            let msg = format!("{} is not a folder", app.display_path(&d));
            return reopen_with_error(app, widget, action, msg);
        }
        Err(e) => return reopen_with_error(app, widget, action, e),
    };
    let (moved, errors) = move_paths(app, paths, &dest);
    finish_batch(app, &dest, &moved, &errors, "moved", "not moved");
}

/// A folder typed in the Move prompt as the explorer shows paths: relative to the project
/// folder with one root; with several, `root-name/path` (a path that does not start with a
/// root name stays in `from_root`). Absolute paths are taken as typed.
fn typed_folder(app: &App, text: &str, from_root: &Path) -> PathBuf {
    let typed = Path::new(text);
    if typed.is_absolute() {
        return typed.to_path_buf();
    }
    if app.explorer.is_multi() {
        let mut parts = typed.components();
        if let Some(first) = parts.next()
            && let Some(r) = app
                .explorer
                .roots
                .iter()
                .find(|r| first.as_os_str() == r.name.as_str())
        {
            return r.path.join(parts.as_path());
        }
    }
    from_root.join(typed)
}

/// Moves each path into `dest` (never over an existing name; one already in `dest` stays);
/// tabs follow. `paths` come from `targets` (nested paths dropped). Returns the new paths and
/// one message per failure. A path of another workspace root than `dest` is not moved.
fn move_paths(app: &mut App, paths: Vec<PathBuf>, dest: &Path) -> (Vec<PathBuf>, Vec<String>) {
    let mut moved = Vec::new();
    let mut errors = Vec::new();
    let dest_root = root_for(app, dest);
    // `dest` is lexical: a folder link inside this root can lead into another root, where
    // the move would land (D292). One canonicalize per batch.
    if really_in_another_root(app, dest, &dest_root) {
        for p in &paths {
            errors.push(cross_root(app, p));
        }
        return (moved, errors);
    }
    for p in paths {
        let from = match below_root(app, &p) {
            Ok(f) => f,
            Err(e) => {
                errors.push(e);
                continue;
            }
        };
        if root_for(app, &from) != dest_root {
            errors.push(cross_root(app, &from));
            continue;
        }
        if from.parent() == Some(dest) {
            continue;
        }
        if let Some(name) = from.file_name()
            && let Err(e) = check_tab_free(app, &from, &dest.join(name))
        {
            errors.push(e);
            continue;
        }
        match vr_fs::ops::move_to(&from, dest) {
            Ok(to) => {
                retarget(app, &from, &to);
                moved.push(to);
            }
            Err(e) => errors.push(describe(app, &e)),
        }
    }
    (moved, errors)
}

pub const MOVE_TO_TRASH: &str = "Move to Trash";
pub const CANCEL: &str = "Cancel";

/// Paths shown in a confirmation before "and N more".
const LIST_MAX: usize = 5;

/// Drops every path that lies under another path of the list (a trashed or moved folder
/// takes them along) and repeats; order is kept. O(n log n): sorted by components, a
/// folder's descendants follow it directly, so one pass against the last kept path does.
pub fn drop_nested(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut sorted: Vec<&PathBuf> = paths.iter().collect();
    sorted.sort();
    let mut keep: HashSet<PathBuf> = HashSet::new();
    let mut last: Option<&PathBuf> = None;
    for p in sorted {
        if last.is_some_and(|l| p.starts_with(l)) {
            continue;
        }
        keep.insert(p.clone());
        last = Some(p);
    }
    paths.into_iter().filter(|p| keep.remove(p)).collect()
}

/// What a multi-item operation acts on: the selection (or the cursor row) in row order,
/// without the root and without paths nested in another selected one. The flag says the
/// root was asked for.
pub(crate) fn targets(app: &App) -> (Vec<PathBuf>, bool) {
    let ex = &app.explorer;
    let rows: Vec<&Row> = if ex.selected.is_empty() {
        ex.current().into_iter().collect()
    } else {
        ex.rows
            .iter()
            .filter(|r| ex.selected.contains(&r.path))
            .collect()
    };
    let had_root = rows.iter().any(|r| r.depth == 0);
    let paths = rows
        .into_iter()
        .filter(|r| r.depth > 0)
        .map(|r| r.path.clone())
        .collect();
    (drop_nested(paths), had_root)
}

/// "a.txt, src/b.rs and 3 more".
fn list(app: &App, paths: &[PathBuf]) -> String {
    let mut names: Vec<String> = paths
        .iter()
        .take(LIST_MAX)
        .map(|p| app.display_path(p))
        .collect();
    if paths.len() > LIST_MAX {
        names.push(format!("and {} more", paths.len() - LIST_MAX));
    }
    names.join(", ")
}

/// Open tabs of `path` or of files under it.
fn tabs_under(app: &App, paths: &[PathBuf]) -> Vec<usize> {
    (0..app.docs.len())
        .filter(|&i| {
            app.docs[i]
                .path
                .as_deref()
                .is_some_and(|d| paths.iter().any(|p| d.starts_with(p)))
        })
        .collect()
}

/// Asks before moving the selection to the trash (D9); Cancel is the default button.
fn delete(app: &mut App, _: CmdArgs) -> CmdResult {
    ensure_loaded(app);
    let (paths, had_root) = targets(app);
    if paths.is_empty() {
        if had_root {
            app.warn(ROOT_REFUSED);
        }
        return Ok(());
    }
    let what = if paths.len() == 1 {
        app.display_path(&paths[0])
    } else {
        format!("{} items ({})", paths.len(), list(app, &paths))
    };
    let mut msg = format!("Move {what} to the trash?");
    let unsaved: Vec<PathBuf> = tabs_under(app, &paths)
        .into_iter()
        .filter(|&i| app.docs[i].is_modified())
        .filter_map(|i| app.docs[i].path.clone())
        .collect();
    if !unsaved.is_empty() {
        msg.push_str(&format!(
            " Unsaved changes in {} go to the recovery folder first.",
            list(app, &unsaved)
        ));
    }
    if had_root {
        msg.push_str(" (The project folder itself is not trashed.)");
    }
    let mut dialog = ConfirmDialog::new("Delete", msg, &[MOVE_TO_TRASH, CANCEL]);
    dialog.selected = 1;
    app.open_dialog(DialogWidget::Confirm(dialog), DialogAction::Trash(paths));
    Ok(())
}

/// Move to Trash: unsaved buffers of the paths go to recovery first (nothing is trashed
/// when that fails); then each path is trashed and reported on its own. `paths` come from
/// `targets`, nested paths already dropped.
pub fn answer_trash(app: &mut App, paths: &[PathBuf]) {
    let mut errors = Vec::new();
    let mut checked = Vec::new();
    for p in paths {
        match below_root(app, p) {
            Ok(c) => checked.push(c),
            Err(e) => errors.push(e),
        }
    }
    let open = tabs_under(app, &checked);
    let kept = match app.keep_discarded(&open) {
        Ok(files) => files,
        Err(e) => {
            app.error(e);
            return;
        }
    };
    let results = app.trash.trash(&checked);
    let mut done: Vec<PathBuf> = Vec::new();
    for (p, res) in results {
        match res {
            Ok(()) => done.push(p),
            Err(e) => errors.push(describe(app, &e)),
        }
    }
    let mut close = tabs_under(app, &done);
    close.sort_unstable();
    for i in close.into_iter().rev() {
        app.close_doc(i);
    }
    let refs: Vec<&Path> = done.iter().map(PathBuf::as_path).collect();
    refresh_after(app, &refs);
    if !done.is_empty() {
        let mut msg = format!("moved {} to the trash", list(app, &done));
        if !kept.is_empty() {
            let files: Vec<String> = kept.iter().map(|f| f.display().to_string()).collect();
            msg.push_str(&format!("; unsaved changes kept in {}", files.join(", ")));
        }
        app.info(msg);
    }
    if !errors.is_empty() {
        app.error(format!("not moved to the trash: {}", errors.join("; ")));
    }
}

/// A message for an operation error that names the paths (project-relative) and says what
/// state things were left in.
pub fn describe(app: &App, e: &vr_fs::FsOpError) -> String {
    use vr_fs::FsOpError as E;
    let p = |path: &Path| app.display_path(path);
    match e {
        E::AlreadyExists(path) => format!("{} already exists; nothing was replaced", p(path)),
        E::NotFound(path) => format!("{} does not exist (any more)", p(path)),
        E::IntoItself { from, to } => {
            format!("cannot move or copy {} into itself ({})", p(from), p(to))
        }
        E::NoFileName(path) => format!("{} has no file name", p(path)),
        E::CrossDevice { from, to } => format!(
            "{} and {} are on different file systems, so nothing was moved; copy it instead",
            p(from),
            p(to)
        ),
        E::PartialCopy { to, source } => format!(
            "copy stopped partway: {} is incomplete ({}); the original is untouched",
            p(to),
            describe(app, source)
        ),
        E::InvalidName(name) => {
            format!("invalid name {name:?}: use one name without '/' (not '.' or '..')")
        }
        E::OutsideRoot(path) => format!("{} is outside the project", path.display()),
        E::TrashRefused { path, reason } => {
            format!("{} was not moved to the trash: {reason}", p(path))
        }
        E::LeftAt {
            path,
            intended,
            source,
        } => format!(
            "the file is safe but was left at {}; renaming it to {} failed: {}",
            p(path),
            p(intended),
            describe(app, source)
        ),
        E::BothNamesLeft { from, to, source } => format!(
            "the file now has both names, {} and {}; nothing was deleted (removing {} failed: {source})",
            p(from),
            p(to),
            p(from)
        ),
        E::Io { path, source } => format!("{}: {source}", p(path)),
        E::Trash(msg) => format!("trash failed: {msg}"),
    }
}

/// Shows the active file in the tree: its folders are expanded and the cursor is put on it.
fn reveal(app: &mut App, _: CmdArgs) -> CmdResult {
    show_and_focus(app);
    let Some(path) = app.active_doc().and_then(|d| d.path.clone()) else {
        return Ok(());
    };
    reveal_path(app, &path);
    Ok(())
}

/// Expands every folder between the root and `path` and selects `path` when it is shown.
pub fn reveal_path(app: &mut App, path: &Path) {
    let root = root_for(app, path);
    if !path.starts_with(&root) {
        let msg = format!("{} is outside the project", path.display());
        app.info(msg);
        return;
    }
    // Root first: each step must be listed in its parent before the next folder is opened,
    // so a folder the tree hides is never expanded (nor watched).
    let mut steps: Vec<&Path> = path
        .ancestors()
        .take_while(|a| a.starts_with(&root) && *a != root)
        .collect();
    steps.reverse();
    for step in steps {
        let Some(parent) = step.parent() else {
            return;
        };
        if !app.explorer.expanded.contains(parent) {
            expand_dir(app, parent);
            if !app.explorer.expanded.contains(parent) {
                // Unreadable: expand_dir said so.
                return;
            }
        }
        if !listed(app, parent, step) {
            // The listing may predate the file; read it once more before giving up.
            app.explorer.reload(parent);
            app.explorer.rebuild();
        }
        if !listed(app, parent, step) {
            let msg = not_shown_reason(app, path);
            app.info(msg);
            return;
        }
    }
    select_path(app, path);
}

/// `child` is in the listing of the expanded folder `dir`.
fn listed(app: &App, dir: &Path, child: &Path) -> bool {
    app.explorer
        .children
        .get(dir)
        .is_some_and(|nodes| nodes.iter().any(|n| n.path == child))
}

/// Why `path` has no row, and how to show it (reveal never turns a filter off itself).
fn not_shown_reason(app: &App, path: &Path) -> String {
    let rel = app.display_path(path);
    let names: Vec<String> = path
        .strip_prefix(root_for(app, path))
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let how = |cmd: &str| {
        if app.display_key(cmd).is_none() {
            format!("the command {cmd}")
        } else {
            app.key_hint(cmd)
        }
    };
    if names.iter().any(|n| n == ".git") {
        format!("{rel} is inside .git, which the explorer never lists")
    } else if path.symlink_metadata().is_err() {
        format!("{rel} is not on disk, so the explorer has no row for it")
    } else if !app.explorer.tree.show_hidden() && names.iter().any(|n| n.starts_with('.')) {
        format!(
            "{rel} is hidden in the explorer (hidden files are off); show them with {}",
            how("explorer.toggle_hidden")
        )
    } else {
        format!(
            "{rel} is hidden in the explorer (ignored by .gitignore); show ignored files with {}",
            how("explorer.toggle_ignored")
        )
    }
}

/// Shows or hides paths matched by .gitignore (AC-9); shown ones are drawn dimmed.
fn toggle_ignored(app: &mut App, _: CmdArgs) -> CmdResult {
    ensure_loaded(app);
    let on = !app.explorer.tree.show_ignored();
    app.explorer.tree.set_show_ignored(on);
    app.explorer.reload_all();
    app.info(if on {
        "explorer: ignored files shown (marked ·)"
    } else {
        "explorer: ignored files hidden"
    });
    Ok(())
}

/// The folder row under the cursor (a root or a folder; never a file, a link or a root whose
/// folder is gone), as a canonical path for the search scope.
pub fn folder_under_cursor(app: &App) -> Option<PathBuf> {
    let row = app.explorer.current()?;
    if row.kind != NodeKind::Dir || row.missing {
        return None;
    }
    let canon = row.path.canonicalize().ok()?;
    // A child of an expanded link can resolve outside the workspace.
    super::search::in_workspace(app, &canon).then_some(canon)
}

fn folder_search(app: &mut App, replace: bool) -> CmdResult {
    ensure_loaded(app);
    match folder_under_cursor(app) {
        Some(f) => super::search::open_folder(app, &f, replace),
        None => app.info("Select a folder in the explorer first"),
    }
    Ok(())
}

/// Search in Folder (D328): the find box in Project scope limited to the folder under the cursor.
fn search_folder(app: &mut App, _: CmdArgs) -> CmdResult {
    folder_search(app, false)
}

/// Replace in Folder (D328): as [`search_folder`], with the replace row.
fn replace_folder(app: &mut App, _: CmdArgs) -> CmdResult {
    folder_search(app, true)
}

/// Shows or hides dotfiles (`.git` is never listed).
fn toggle_hidden(app: &mut App, _: CmdArgs) -> CmdResult {
    ensure_loaded(app);
    let on = !app.explorer.tree.show_hidden();
    app.explorer.tree.set_show_hidden(on);
    app.explorer.reload_all();
    app.info(if on {
        "explorer: hidden files shown"
    } else {
        "explorer: hidden files hidden"
    });
    Ok(())
}

/// Editor → explorer (when shown) → terminal (when shown with a terminal in it) → AI panel
/// (when shown) → editor
/// (D266). The find box (either scope) is not in the cycle: it keeps Tab for its own fields
/// and is reached with ctrl+f. A focused terminal keeps Tab for its program; ctrl+` gives
/// the keys back.
fn focus_cycle(app: &mut App, _: CmdArgs) -> CmdResult {
    let terminal = app.terminal.visible && !app.terminal.tabs.is_empty();
    let ai = app.ai.visible;
    app.focus = match app.focus {
        Focus::Editor | Focus::FindBar if app.explorer.visible => {
            ensure_loaded(app);
            Focus::Explorer
        }
        Focus::Editor | Focus::FindBar | Focus::Search | Focus::Explorer if terminal => {
            Focus::Terminal
        }
        Focus::Editor | Focus::FindBar | Focus::Search | Focus::Explorer | Focus::Terminal
            if ai =>
        {
            Focus::Ai
        }
        _ => {
            super::search::focus_editor(app);
            return Ok(());
        }
    };
    Ok(())
}

/// Go to file (ctrl+p): the palette lists project files by fuzzy name (AC-9: ignored files
/// are not in the index).
fn goto_file(app: &mut App, _: CmdArgs) -> CmdResult {
    super::palette::open_files(app);
    Ok(())
}

/// Temp files of an interrupted save (`.vr-ide-save-*.tmp`), conflict copies a refused save
/// kept (`*.vr-ide-conflict-*`) and stranded rename temps (`.*.vr-tmp-*`).
pub fn is_leftover(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    vr_core::document::is_save_temp(path)
        || name.contains(".vr-ide-conflict-")
        || (name.starts_with('.') && name.contains(".vr-tmp-"))
}

/// Names leftover files in `dir` that no notice named yet (D76.4). They may hold the only
/// copy of someone's data, so they are never deleted automatically.
fn report_leftovers(app: &mut App, dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut found: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| is_leftover(p) && !app.explorer.reported_leftovers.contains(p))
        .collect();
    if found.is_empty() {
        return;
    }
    found.sort();
    let msg = format!(
        "leftover files from interrupted or refused saves and moves (not deleted; check and \
         remove them yourself): {}",
        list(app, &found)
    );
    app.explorer.reported_leftovers.extend(found);
    app.warn(msg);
}
