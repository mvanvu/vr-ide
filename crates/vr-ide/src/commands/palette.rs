//! Command palette state and input (AC-6), also used for go to file. Rendering is in
//! `ui::palette`.
//!
//! Built-in keys (keymap.toml `[palette]` says so): typing and line editing, Up/Down,
//! PageUp/PageDown, Enter runs the selected command, Esc closes.

use std::collections::HashMap;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use super::registry::{CmdArgs, CommandSpec, all_commands, is_pending};
use crate::app::{App, Focus};
use crate::ui::dialog::LineInput;

/// One listed command or (go to file) file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteItem {
    /// Command name ("" for a file).
    pub name: &'static str,
    /// Command title, or the file name.
    pub title: String,
    /// Go to file: the path as the index lists it (root-relative; `root-name/relative` with
    /// several roots), made absolute by the index when chosen.
    pub path: Option<PathBuf>,
    /// Shown dimmed after the title: the command name, or the file's folder.
    pub detail: String,
    /// Bound keys from the keymap ("ctrl+s"), "" when unbound.
    pub keys: String,
    /// Char indices of the title that matched the query (highlighted).
    pub hits: Vec<usize>,
}

/// What the palette lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteMode {
    Commands,
    /// Go to file (`file.goto`): project files from the background `FileIndex`.
    Files,
}

#[derive(Debug, Clone)]
pub struct Palette {
    pub mode: PaletteMode,
    /// Files mode: the index is still being built.
    pub indexing: bool,
    pub input: LineInput,
    pub items: Vec<PaletteItem>,
    pub selected: usize,
    /// First visible item.
    pub top: usize,
    /// Rows the list had in the last frame (paging, mouse).
    pub list_area: Rect,
    /// Whole palette box in the last frame.
    pub area: Rect,
    /// Focus to return to on close.
    pub prev_focus: Focus,
    /// Slash commands: why the last Enter ran nothing, shown under the input until the text
    /// changes (T20).
    pub error: Option<String>,
    /// Slash commands: the folder path completion read last (read again only for another
    /// folder; dropped with the window).
    pub dir_cache: Option<super::slash::DirListing>,
}

/// Commands the palette offers: every registered one except itself and the placeholders of
/// later tasks.
fn candidates() -> Vec<&'static CommandSpec> {
    all_commands()
        .iter()
        .filter(|c| c.name != "app.palette" && !is_pending(c.name))
        .collect()
}

/// Opens the palette with every command listed.
pub fn open(app: &mut App) {
    let prev_focus = match app.palette.take() {
        Some(p) => p.prev_focus,
        None => app.focus,
    };
    app.palette = Some(Palette {
        mode: PaletteMode::Commands,
        indexing: false,
        input: LineInput::default(),
        items: Vec::new(),
        selected: 0,
        top: 0,
        list_area: Rect::default(),
        area: Rect::default(),
        prev_focus,
        error: None,
        dir_cache: None,
    });
    app.modal_since = Some(app.now());
    app.focus = Focus::Palette;
    refilter(app);
}

/// Closes the palette (no-op when closed) and gives focus back.
pub fn close(app: &mut App) {
    if let Some(p) = app.palette.take() {
        app.focus = p.prev_focus;
        app.dirty = true;
    }
}

/// Recomputes the list for the current input; the selection goes back to the top.
pub fn refilter(app: &mut App) {
    relist(app, false);
}

/// Recomputes the list after the file index changed under the user (same input): the
/// highlighted file stays highlighted when it is still listed, so Enter opens what is shown.
pub fn refresh_keeping_selection(app: &mut App) {
    relist(app, true);
}

fn relist(app: &mut App, keep: bool) {
    let Some((query, mode, was)) = app.palette.as_ref().map(|p| {
        let was = p.items.get(p.selected).map(|i| (i.name, i.path.clone()));
        (p.input.text().to_string(), p.mode, was)
    }) else {
        return;
    };
    if mode == PaletteMode::Commands && is_slash(&query) {
        super::slash::prepare(app, &query);
    }
    let indexing = app.index_rx.is_some();
    let items = match mode {
        PaletteMode::Commands => command_items(app, &query),
        PaletteMode::Files => file_items(app, &query),
    };
    if let Some(p) = &mut app.palette {
        let selected = was
            .filter(|_| keep)
            .and_then(|(name, path)| items.iter().position(|i| i.name == name && i.path == path))
            .unwrap_or(0);
        p.items = items;
        p.selected = selected;
        p.top = if selected == 0 {
            0
        } else {
            p.top.min(selected)
        };
        p.indexing = indexing;
    }
    app.dirty = true;
}

fn command_items(app: &App, query: &str) -> Vec<PaletteItem> {
    if is_slash(query) {
        return super::slash::rows(app, query)
            .into_iter()
            .map(|r| PaletteItem {
                name: "",
                title: r.title,
                path: None,
                detail: r.detail,
                keys: String::new(),
                hits: Vec::new(),
            })
            .collect();
    }
    let cmds = candidates();
    rank(query, &cmds)
        .into_iter()
        .map(|(i, hits)| {
            let c = cmds[i];
            PaletteItem {
                name: c.name,
                title: c.title.to_string(),
                path: None,
                detail: c.name.to_string(),
                // The command's one key (USER-5, D250).
                keys: app.display_key(c.name).unwrap_or_default(),
                hits,
            }
        })
        .collect()
}

/// Runs the selected command (a command may open a dialog) or opens the selected file,
/// after closing the palette.
pub fn accept(app: &mut App) {
    if slash_mode(app) {
        super::slash::enter(app);
        return;
    }
    let Some(item) = app
        .palette
        .as_ref()
        .and_then(|p| p.items.get(p.selected))
        .cloned()
    else {
        return;
    };
    close(app);
    match item.path {
        Some(rel) => {
            let path = app
                .file_index
                .as_ref()
                .map_or_else(|| rel.clone(), |index| index.absolute(&rel));
            match app.open_path(&path) {
                Ok(()) => app.refocus_editor_in_read_mode(),
                Err(e) => super::search::report_open_error(app, &e),
            }
        }
        // From the palette a new terminal asks which root it starts in when there are
        // several (spec W); its key keeps the active file's root.
        None if item.name == "terminal.new" && app.workspace.is_multi() => {
            super::slash::pick_terminal_root(app);
        }
        None => {
            app.run_command(item.name, CmdArgs::None);
        }
    }
}

fn move_selection(app: &mut App, delta: isize) {
    if let Some(p) = &mut app.palette
        && !p.items.is_empty()
    {
        let last = p.items.len() as isize - 1;
        p.selected = (p.selected as isize + delta).clamp(0, last) as usize;
    }
}

/// Built-in palette keys; true when used. Other keys go to the keymap (`[palette]`, then
/// global), so the palette's own keys reopen it.
pub fn handle_key(app: &mut App, key: &KeyEvent) -> bool {
    let page = app
        .palette
        .as_ref()
        .map_or(1, |p| p.list_area.height.max(1) as isize);
    match key.code {
        KeyCode::Esc => close(app),
        KeyCode::Enter if key.kind == KeyEventKind::Repeat => {}
        KeyCode::Enter => accept(app),
        KeyCode::Up => move_selection(app, -1),
        KeyCode::Down => move_selection(app, 1),
        KeyCode::PageUp => move_selection(app, -page),
        KeyCode::PageDown => move_selection(app, page),
        KeyCode::Tab if key.modifiers.is_empty() && slash_mode(app) => super::slash::tab(app),
        _ => {
            let Some(p) = &mut app.palette else {
                return false;
            };
            let before = p.input.text().to_string();
            if !p.input.handle_key(key) {
                return false;
            }
            if p.input.text() != before {
                p.error = None;
                refilter(app);
            }
        }
    }
    app.dirty = true;
    true
}

/// Click on a row runs it, the wheel moves the selection, a click outside closes.
pub fn handle_mouse(app: &mut App, ev: &MouseEvent) {
    let Some(p) = &app.palette else {
        return;
    };
    let inside = |r: Rect| {
        ev.column >= r.x && ev.column < r.x + r.width && ev.row >= r.y && ev.row < r.y + r.height
    };
    let (list, area, top) = (p.list_area, p.area, p.top);
    match ev.kind {
        MouseEventKind::Down(MouseButton::Left) if inside(list) => {
            let i = top + usize::from(ev.row - list.y);
            if let Some(p) = &mut app.palette
                && i < p.items.len()
            {
                p.selected = i;
                accept(app);
            }
        }
        MouseEventKind::Down(MouseButton::Left) if !inside(area) => close(app),
        MouseEventKind::ScrollDown => move_selection(app, 1),
        MouseEventKind::ScrollUp => move_selection(app, -1),
        _ => {}
    }
}

pub fn paste(app: &mut App, text: &str) {
    if let Some(p) = &mut app.palette {
        p.input.insert_str(text);
        p.error = None;
        refilter(app);
    }
}

type TextOf = fn(&CommandSpec) -> &'static str;

/// Commands ranked for `query`: (index into `cmds`, title char hits), best first. Titles and
/// names are both matched (nucleo fuzzy matching through `vr_fs::FileIndex`, smart case); a
/// command scores its better one. An empty query lists every command by title.
pub fn rank(query: &str, cmds: &[&CommandSpec]) -> Vec<(usize, Vec<usize>)> {
    if query.trim().is_empty() {
        let mut all: Vec<usize> = (0..cmds.len()).collect();
        all.sort_by_key(|&i| (cmds[i].title, cmds[i].name));
        return all.into_iter().map(|i| (i, Vec::new())).collect();
    }
    let mut best: HashMap<usize, (u32, Vec<usize>)> = HashMap::new();
    let fields: [(bool, TextOf); 2] = [(true, |c| c.title), (false, |c| c.name)];
    for (is_title, text) in fields {
        // Titles and names hold no '/', so each one is a single "file name" to the matcher.
        let mut by_text: HashMap<&'static str, Vec<usize>> = HashMap::new();
        for (i, c) in cmds.iter().enumerate() {
            by_text.entry(text(c)).or_default().push(i);
        }
        let index = vr_fs::FileIndex::from_paths("", by_text.keys().map(PathBuf::from).collect());
        for (path, score, idx) in index.query(query, usize::MAX) {
            let key = path.to_string_lossy();
            for &i in by_text.get(key.as_ref()).into_iter().flatten() {
                let e = best.entry(i).or_insert((0, Vec::new()));
                e.0 = e.0.max(score);
                if is_title {
                    e.1 = idx.iter().map(|&x| x as usize).collect();
                }
            }
        }
    }
    let mut out: Vec<(usize, u32, Vec<usize>)> =
        best.into_iter().map(|(i, (s, h))| (i, s, h)).collect();
    out.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| cmds[a.0].title.cmp(cmds[b.0].title))
    });
    out.into_iter().map(|(i, _, h)| (i, h)).collect()
}

impl Palette {
    /// Shown instead of an empty list.
    pub fn empty_text(&self) -> &'static str {
        match self.mode {
            PaletteMode::Commands => "No matching commands",
            PaletteMode::Files if self.indexing => "Indexing files…",
            PaletteMode::Files => "No matching files",
        }
    }
}

/// Files listed at most (the best matches).
const FILE_LIMIT: usize = 200;

/// Opens the palette as go to file. The index is rebuilt in the background on each open:
/// only shown folders are watched (D110), so changes deeper in the tree are never reported.
/// The previous index lists meanwhile; the highlighted file stays when the new one arrives.
pub fn open_files(app: &mut App) {
    app.start_index();
    open(app);
    if let Some(p) = &mut app.palette {
        p.mode = PaletteMode::Files;
    }
    refilter(app);
}

/// Go-to-file rows: file name (with its matched chars) and the folder, dimmed.
fn file_items(app: &App, query: &str) -> Vec<PaletteItem> {
    let Some(index) = &app.file_index else {
        return Vec::new();
    };
    index
        .query(query, FILE_LIMIT)
        .into_iter()
        .map(|(rel, _, hits)| {
            let text = rel.to_string_lossy().into_owned();
            let base_start = text.rfind('/').map_or(0, |b| b + 1);
            let offset = text[..base_start].chars().count();
            PaletteItem {
                name: "",
                title: text[base_start..].to_string(),
                detail: text[..base_start].trim_end_matches('/').to_string(),
                path: Some(rel),
                keys: String::new(),
                hits: hits
                    .into_iter()
                    .map(|h| h as usize)
                    .filter(|&h| h >= offset)
                    .map(|h| h - offset)
                    .collect(),
            }
        })
        .collect()
}

/// Input that starts with `/` is a slash command (T20).
pub fn is_slash(query: &str) -> bool {
    query.starts_with('/')
}

/// The command list shows slash commands.
pub fn slash_mode(app: &App) -> bool {
    app.palette
        .as_ref()
        .is_some_and(|p| p.mode == PaletteMode::Commands && is_slash(p.input.text()))
}
