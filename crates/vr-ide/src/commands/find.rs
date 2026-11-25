//! `find.*` commands and the find box (T22, D261): one box under the tab bar for the active
//! file and for the project, with a list of every match under it. Rendering: `ui::find_bar`.
//!
//! The box has a scope: **File** (the default; this module's matches of the active
//! document) or **Project** (the project search of `commands::search`, which keeps its own
//! worker, results, replace preview and confirm). `find.open` (ctrl+f) opens the box; pressed
//! again while the box has the keys it switches the scope. The query is one text field with
//! the syntax of `vr_search::Query::parse`: `/re/` is a regex, a trailing `/i` ignores case,
//! `\/` is a literal slash.
//!
//! Built-in keys while the box has the keys in File scope (like the palette's, not in the
//! keymap): typing and line editing, Up/Down/PageUp/PageDown move in the list (the editor
//! shows the match), Enter in the find field goes to the match and gives the editor the keys
//! in read mode, shift+Enter there selects the previous match (D116), Enter in the replace
//! field asks Replace: One / All / Cancel (D279, [`ask_replace`]), Tab / shift+Tab switch
//! between the fields, Esc closes the box and returns to read
//! mode. A key the keymap binds for the editor runs its command first (ctrl+f, f4, ctrl+r,
//! D111), except the `edit.*` commands, which would change the file behind the box: there
//! ctrl+v pastes into the field and the others do nothing.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use vr_config::Context;
use vr_core::ropey::Rope;
use vr_core::{Selection, Transaction};
use vr_search::Query;

use super::dialog::DialogAction;
use super::registry::{CmdArgs, CmdResult, CommandSpec};
use crate::app::{App, Focus};
use crate::event_loop::chord_from_key;
use crate::ui::dialog::{ConfirmDialog, DialogWidget, LineInput};

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "find.open",
        title: "Find",
        run: open,
    },
    CommandSpec {
        name: "find.next",
        title: "Find Next",
        run: next,
    },
    CommandSpec {
        name: "find.prev",
        title: "Find Previous",
        run: prev,
    },
    CommandSpec {
        name: "find.replace_open",
        title: "Replace",
        run: replace_open,
    },
    CommandSpec {
        name: "find.replace_one",
        title: "Replace One",
        run: replace_one,
    },
    CommandSpec {
        name: "find.replace_all",
        title: "Replace All",
        run: replace_all,
    },
];

/// The box's input field that has the cursor (File scope).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Field {
    #[default]
    Find,
    Replace,
}

/// What the find box searches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Scope {
    /// The active document.
    #[default]
    File,
    /// Every workspace root (`commands::search`).
    Project,
}

/// State of the find box. The matches belong to the active document and are recomputed
/// when the query, the document or its text changes; they are kept in Project scope too, so
/// the open file shows its matches while the project is searched.
#[derive(Debug, Clone, Default)]
pub struct FindBar {
    pub open: bool,
    pub scope: Scope,
    pub replace_mode: bool,
    pub field: Field,
    pub input: LineInput,
    pub replace: LineInput,
    /// Char ranges in the active document, in order.
    pub matches: Vec<(usize, usize)>,
    /// More matches exist than `matches` holds (it stopped at [`MATCH_CAP`]).
    pub capped: bool,
    /// Why the query finds nothing (an invalid regex).
    pub error: Option<String>,
    cache: Option<CacheKey>,
    pending: Option<Pending>,
    /// Where the cursor was when the box opened: typing finds the first match from here.
    origin: usize,
    /// Selected row of the File-scope list when the editor's selection is on no match, and
    /// the first visible row.
    pub selected: usize,
    pub top: usize,
    /// Box area and its list area in the last frame.
    pub area: Rect,
    pub list_area: Rect,
}

/// What `matches` were computed for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CacheKey {
    doc: usize,
    path: Option<PathBuf>,
    version: u64,
    len: usize,
    query: Query,
}

impl FindBar {
    pub fn query(&self) -> Query {
        Query::parse(self.input.text())
    }

    /// The box is open in Project scope.
    pub fn project(&self) -> bool {
        self.open && self.scope == Scope::Project
    }
}

/// A rescan waiting for typing to pause.
#[derive(Debug, Clone)]
struct Pending {
    key: CacheKey,
    due: Instant,
    /// Select the first match from where the box opened once the scan ran (the query
    /// changed).
    pick: bool,
}

/// Documents up to this size are rescanned on every change; larger ones once typing pauses
/// for [`DEBOUNCE`] (D124).
const INSTANT_BYTES: usize = 1 << 20;
const DEBOUNCE: Duration = Duration::from_millis(150);

fn cache_key(app: &App) -> Option<CacheKey> {
    let doc = app.docs.get(app.active)?;
    Some(CacheKey {
        doc: app.active,
        path: doc.path.clone(),
        version: doc.buffer.version(),
        len: doc.buffer.text().len_chars(),
        query: app.find.query(),
    })
}

/// Brings the matches up to date for drawing: at once for a small document, after a pause
/// in typing for a large one (the old matches are drawn meanwhile). Commands call
/// [`refresh_now`] so they never act on matches of an earlier text.
pub fn refresh(app: &mut App) {
    let now = app.now();
    refresh_at(app, now, false);
}

fn refresh_at(app: &mut App, now: Instant, pick: bool) {
    let Some(key) = cache_key(app) else {
        app.find.matches.clear();
        app.find.cache = None;
        app.find.pending = None;
        return;
    };
    if app.find.cache.as_ref() == Some(&key) {
        if pick {
            pick_from_origin(app);
        }
        return;
    }
    if app.docs[app.active].buffer.text().len_bytes() <= INSTANT_BYTES {
        rescan(app, key);
        if pick {
            pick_from_origin(app);
        }
        return;
    }
    let f = &mut app.find;
    match &mut f.pending {
        Some(p) if p.key == key => p.pick |= pick,
        _ => {
            let pick = pick || f.pending.as_ref().is_some_and(|p| p.pick);
            f.pending = Some(Pending {
                key,
                due: now + DEBOUNCE,
                pick,
            });
        }
    }
}

/// Runs a rescan whose pause is over (called by `App::tick`).
pub fn poll(app: &mut App, now: Instant) {
    if !app.find.pending.as_ref().is_some_and(|p| p.due <= now) {
        return;
    }
    let pick = app.find.pending.take().is_some_and(|p| p.pick);
    refresh_now(app);
    if pick {
        pick_from_origin(app);
    }
    app.dirty = true;
}

fn rescan(app: &mut App, key: CacheKey) {
    app.find.pending = None;
    let doc = &app.docs[key.doc];
    match find_capped(doc.buffer.text(), &key.query, MATCH_CAP, CHUNK_BYTES) {
        Ok((m, capped)) => {
            app.find.matches = m;
            app.find.capped = capped;
            app.find.error = None;
        }
        Err(e) => {
            app.find.matches.clear();
            app.find.capped = false;
            app.find.error = Some(e.to_string());
        }
    }
    app.find.cache = Some(key);
}

/// The box stops collecting matches here (D124, VS Code stops near the same number): the
/// match list stays small whatever the file holds.
pub const MATCH_CAP: usize = 20_000;

/// Text handed to `vr_search::find_all` at a time: whole lines, about this many bytes.
const CHUNK_BYTES: usize = 256 * 1024;

/// The first `cap` matches of `q` in `text` and whether more were left out. The text is
/// searched in chunks of whole lines, which finds exactly what `find_all` finds on the
/// whole text (its matches never span lines), but copies one chunk at a time and stops at
/// the cap instead of copying and scanning the whole file.
fn find_capped(
    text: &Rope,
    q: &Query,
    cap: usize,
    chunk_bytes: usize,
) -> Result<(Vec<(usize, usize)>, bool), vr_search::QueryError> {
    let mut out = Vec::new();
    if q.pattern.is_empty() {
        return Ok((out, false));
    }
    let lines = text.len_lines();
    let mut line = 0;
    while line < lines {
        let start_byte = text.line_to_byte(line);
        let target = (start_byte + chunk_bytes).min(text.len_bytes());
        // The workspace builds ropey with `cr_lines` only, so its lines end at \n, \r and
        // \r\n exactly like vr-search's (U+2028 is no line break for either).
        let end_line = (text.byte_to_line(target) + 1).min(lines).max(line + 1);
        let (from, to) = (text.line_to_char(line), text.line_to_char(end_line));
        let chunk = Rope::from(text.slice(from..to));
        for (s, e) in vr_search::find_all(&chunk, q)? {
            if out.len() == cap {
                return Ok((out, true));
            }
            out.push((from + s, from + e));
        }
        line = end_line;
    }
    Ok((out, false))
}

/// Rescans now, whatever the size of the document (a pending rescan is dropped).
pub fn refresh_now(app: &mut App) {
    match cache_key(app) {
        Some(key) => rescan(app, key),
        None => {
            app.find.matches.clear();
            app.find.cache = None;
            app.find.pending = None;
        }
    }
}

/// Index of the match the primary selection covers exactly.
pub fn current(app: &App) -> Option<usize> {
    let r = app.active_doc()?.buffer.selection().primary();
    let want = (r.from(), r.to());
    app.find.matches.binary_search(&want).ok()
}

/// The selected row of the File-scope list: the match under the editor's selection, else
/// the last row moved to.
pub fn selected_row(app: &App) -> usize {
    current(app)
        .unwrap_or(app.find.selected)
        .min(app.find.matches.len().saturating_sub(1))
}

/// "3/12": the match under the selection and the number of matches; "?/12" when the
/// selection is on none, "No results" without matches.
pub fn count_text(app: &App) -> String {
    if app.find.error.is_some() {
        return "invalid regex".to_string();
    }
    if app.find.input.text().is_empty() {
        return String::new();
    }
    let n = app.find.matches.len();
    if n == 0 {
        return "No results".to_string();
    }
    let more = if app.find.capped { "+" } else { "" };
    match current(app) {
        Some(i) => format!("{}/{n}{more}", i + 1),
        None => format!("?/{n}{more}"),
    }
}

/// The editor's selection when it is one line of at most 1000 chars.
fn selection_text(app: &App) -> Option<String> {
    let doc = app.active_doc()?;
    let r = doc.buffer.selection().primary();
    (!r.is_empty() && r.to() - r.from() <= 1000)
        .then(|| doc.buffer.text().slice(r.from()..r.to()).to_string())
        .filter(|s| !s.contains(['\n', '\r']))
}

/// Opens the box in File scope (with the replace field when `replace`) and gives it the
/// keys. A selection on one line becomes the query (typed so that it searches literally),
/// unless it is a match of the current query (reopening keeps what was searched for).
fn open_bar(app: &mut App, replace: bool) {
    let Some(doc) = app.active_doc() else {
        app.info("Find: no file is open");
        return;
    };
    let origin = doc.buffer.selection().primary().from();
    let seed = selection_text(app).and_then(|s| vr_search::literal_syntax(&s));
    refresh_now(app);
    if let Some(seed) = seed
        && current(app).is_none()
    {
        app.find.input.set(&seed);
    }
    let f = &mut app.find;
    f.open = true;
    f.scope = Scope::File;
    f.replace_mode = replace;
    f.field = if replace && !f.input.text().is_empty() {
        Field::Replace
    } else {
        Field::Find
    };
    f.origin = origin;
    app.search.visible = false;
    app.focus = Focus::FindBar;
    refresh_now(app);
    app.dirty = true;
}

/// Opens the box in Project scope (the project replace when `replace`) and gives it the
/// keys. The open file keeps showing the query's matches.
pub fn open_project(app: &mut App, replace: bool) {
    super::search::show(app, replace);
    app.find.open = true;
    app.find.scope = Scope::Project;
    mirror_project_fields(app);
}

/// Project scope: the box's text lives in the search panel's fields; the File-scope fields
/// follow them so the open file highlights the same query and a switch keeps the text.
pub(crate) fn mirror_project_fields(app: &mut App) {
    let (q, r) = (
        app.search.input.text().to_string(),
        app.search.replace.text().to_string(),
    );
    if app.find.input.text() != q {
        app.find.input.set(&q);
    }
    if app.find.replace.text() != r {
        app.find.replace.set(&r);
    }
}

/// ctrl+f while the box has the keys: File → Project → File. The text, the replace row and
/// the field with the cursor go along.
fn switch_scope(app: &mut App) {
    match app.find.scope {
        Scope::File => {
            let s = &mut app.search;
            let changed = s.input.text() != app.find.input.text()
                || s.replace.text() != app.find.replace.text();
            s.input.set(app.find.input.text());
            s.replace.set(app.find.replace.text());
            s.replace_mode = app.find.replace_mode;
            s.field = match app.find.field {
                Field::Find => super::search::Field::Query,
                Field::Replace => super::search::Field::Replace,
            };
            app.find.scope = Scope::Project;
            s.visible = true;
            app.focus = Focus::Search;
            // A preview the fields no longer describe goes, as when they are typed in.
            if changed {
                super::search::fields_changed(app);
            }
            // Switching asks for the project's results of the query: search now, unless
            // they are already shown (an emptied query clears the old ones).
            let q = app.search.query();
            let stale = !q.pattern.is_empty() || !app.search.groups.is_empty();
            if stale && app.search.query.as_ref() != Some(&q) {
                super::search::start(app);
            }
        }
        Scope::Project => {
            if super::search::replacing(app) {
                app.info("Replace in Project: the replace is running; Esc cancels it");
                return;
            }
            if app.active_doc().is_none() {
                app.info("Find in file: no file is open");
                return;
            }
            mirror_project_fields(app);
            let replace = app.search.replace_mode;
            let field = app.search.field;
            app.search.visible = false;
            app.find.scope = Scope::File;
            app.find.replace_mode = replace;
            app.find.field = if replace && field == super::search::Field::Replace {
                Field::Replace
            } else {
                Field::Find
            };
            app.focus = Focus::FindBar;
            if replace && !enter_edit_for_replace(app) {
                app.find.replace_mode = false;
                app.find.field = Field::Find;
            }
            app.find.origin = app
                .active_doc()
                .map_or(0, |d| d.buffer.selection().primary().from());
            query_changed(app);
        }
    }
    app.dirty = true;
}

/// In-file replace needs edit mode; a read-only file warns and stays in read mode (false).
fn enter_edit_for_replace(app: &mut App) -> bool {
    if app.active_doc().is_some_and(|d| d.read_only) {
        // Warns and keeps read mode.
        app.enter_edit_mode();
        return false;
    }
    app.enter_edit_mode();
    true
}

/// The box has the keys (either scope).
fn box_focused(app: &App) -> bool {
    app.find.open
        && match app.find.scope {
            Scope::File => app.focus == Focus::FindBar,
            Scope::Project => app.focus == Focus::Search && app.search.visible,
        }
}

/// The active tab back in read mode, its selection (the match) kept.
fn read_mode_keeping_selection(app: &mut App) {
    if let Some(v) = app.views.get_mut(app.active) {
        v.edit = false;
    }
}

/// Esc in the box (either scope): closes it; the editor gets the keys in read mode.
pub fn close_box(app: &mut App) {
    app.find.pending = None;
    app.find.open = false;
    app.search.visible = false;
    if matches!(app.focus, Focus::FindBar | Focus::Search) {
        app.focus = Focus::Editor;
        read_mode_keeping_selection(app);
    }
    app.dirty = true;
}

/// ctrl+f: opens the box in File scope (Project when no file is open); in the box it
/// switches the scope.
fn open(app: &mut App, _: CmdArgs) -> CmdResult {
    if box_focused(app) {
        switch_scope(app);
    } else if app.active_doc().is_none() || super::search::replacing(app) {
        // A running project replace keeps its progress and its Esc in view.
        open_project(app, false);
    } else {
        open_bar(app, false);
    }
    Ok(())
}

/// ctrl+r in the editor (T21): the replace field of the box in its scope. File scope: in
/// edit mode; a read-only file only warns and keeps read mode, with no replace row. Project
/// scope (the box open on the project, or no file open): the project replace.
fn replace_open(app: &mut App, _: CmdArgs) -> CmdResult {
    if app.find.project() || app.active_doc().is_none() {
        open_project(app, true);
        return Ok(());
    }
    if !enter_edit_for_replace(app) {
        return Ok(());
    }
    open_bar(app, true);
    Ok(())
}

fn select(app: &mut App, i: usize) {
    let Some(&(from, to)) = app.find.matches.get(i) else {
        return;
    };
    app.find.selected = i;
    if let Some(doc) = app.active_doc_mut() {
        doc.buffer.set_selection(Selection::single(from, to));
    }
    app.cursor_moved();
}

/// Selects the next (or previous) match, wrapping at the end of the file. From a selection
/// that is no match, the next one is the first starting at or after it. In Project scope
/// (the box open on the project) it opens the next (previous) project result.
fn step(app: &mut App, forward: bool) {
    if app.find.project() {
        super::search::step_result(app, forward);
        return;
    }
    let Some(doc) = app.active_doc() else {
        app.info("Find: no file is open");
        return;
    };
    let from = doc.buffer.selection().primary().from();
    if app.find.input.text().is_empty() {
        open_bar(app, app.find.replace_mode && app.find.open);
        return;
    }
    refresh_now(app);
    if let Some(e) = app.find.error.clone() {
        app.warn(format!("Find: {e}"));
        return;
    }
    let m = &app.find.matches;
    let n = m.len();
    if n == 0 {
        let msg = format!("Find: no results for {:?}", app.find.input.text());
        app.info(msg);
        return;
    }
    let i = match current(app) {
        Some(i) if forward => (i + 1) % n,
        Some(i) => (i + n - 1) % n,
        None if forward => m.iter().position(|&(s, _)| s >= from).unwrap_or(0),
        None => m.iter().rposition(|&(s, _)| s < from).unwrap_or(n - 1),
    };
    select(app, i);
}

fn next(app: &mut App, _: CmdArgs) -> CmdResult {
    step(app, true);
    Ok(())
}

fn prev(app: &mut App, _: CmdArgs) -> CmdResult {
    step(app, false);
    Ok(())
}

/// Up/Down/PageUp/PageDown in the File-scope list: the editor shows the match (no wrap).
fn move_in_list(app: &mut App, delta: isize) {
    refresh_now(app);
    let n = app.find.matches.len();
    if n == 0 {
        return;
    }
    let cur = selected_row(app) as isize;
    let i = (cur + delta).clamp(0, n as isize - 1) as usize;
    select(app, i);
}

/// Enter in the find field: the editor gets the keys in read mode on the selected match.
fn jump(app: &mut App) {
    refresh_now(app);
    if let Some(e) = app.find.error.clone() {
        app.warn(format!("Find: {e}"));
        return;
    }
    if app.find.matches.is_empty() {
        if !app.find.input.text().is_empty() {
            let msg = format!("Find: no results for {:?}", app.find.input.text());
            app.info(msg);
        }
        return;
    }
    let i = selected_row(app);
    select(app, i);
    app.focus = Focus::Editor;
    read_mode_keeping_selection(app);
}

/// Replaces the match under the selection (one undo step) and selects the next one; with
/// no match selected it only moves to the next match, as VS Code does.
fn replace_one(app: &mut App, _: CmdArgs) -> CmdResult {
    if app.active_doc().is_none() || app.find.input.text().is_empty() {
        open_bar(app, true);
        return Ok(());
    }
    refresh_now(app);
    let Some(i) = current(app) else {
        step(app, true);
        return Ok(());
    };
    let (from, to) = app.find.matches[i];
    let q = app.find.query();
    let replacement = app.find.replace.text().to_string();
    let Some(doc) = app.editable_doc() else {
        return Ok(());
    };
    let tx = vr_search::replace_all_tx(doc.buffer.text(), &q, &replacement)?;
    let Some(change) = tx
        .changes
        .into_iter()
        .find(|c| c.from == from && c.to == to)
    else {
        return Ok(());
    };
    let end = from + change.insert.chars().count();
    doc.buffer.break_undo_group();
    doc.buffer
        .apply(Transaction::new(vec![change]).with_selection(Selection::point(end)));
    doc.buffer.break_undo_group();
    step(app, true);
    Ok(())
}

/// Replaces every match in the file as one transaction: one undo step (AC-10).
fn replace_all(app: &mut App, _: CmdArgs) -> CmdResult {
    if app.active_doc().is_none() || app.find.input.text().is_empty() {
        open_bar(app, true);
        return Ok(());
    }
    let q = app.find.query();
    let replacement = app.find.replace.text().to_string();
    let Some(doc) = app.editable_doc() else {
        return Ok(());
    };
    let tx = vr_search::replace_all_tx(doc.buffer.text(), &q, &replacement)?;
    let n = tx.changes.len();
    if n > 0 {
        // The highlighter gets the match list as edits rather than one edit over all of them.
        let version = doc.buffer.version();
        let changes: Vec<(usize, usize, String)> = tx
            .changes
            .iter()
            .map(|c| (c.from, c.to, c.insert.clone()))
            .collect();
        doc.buffer.break_undo_group();
        doc.buffer.apply(tx);
        doc.buffer.break_undo_group();
        let active = app.active;
        app.syntax.expect_changes(active, version, changes);
    }
    refresh_now(app);
    if n == 0 {
        let msg = format!("Replace All: no results for {:?}", app.find.input.text());
        app.info(msg);
    } else {
        app.info(format!(
            "Replaced {n} occurrence{}",
            if n == 1 { "" } else { "s" }
        ));
    }
    Ok(())
}

/// After the query changed: the first match from where the box opened is selected (the
/// cursor goes back there when nothing matches) — once the matches are known.
fn query_changed(app: &mut App) {
    let now = app.now();
    refresh_at(app, now, true);
}

fn pick_from_origin(app: &mut App) {
    let origin = app.find.origin;
    let m = &app.find.matches;
    let pick = m
        .iter()
        .position(|&(s, _)| s >= origin)
        .or((!m.is_empty()).then_some(0));
    match pick {
        Some(i) => select(app, i),
        None => {
            if let Some(doc) = app.active_doc_mut() {
                doc.buffer.set_selection(Selection::point(origin));
            }
            app.cursor_moved();
        }
    }
}

/// Keys while the box has the keys in File scope; true when used.
pub fn handle_key(app: &mut App, key: &KeyEvent) -> bool {
    // The last tab closed under the box: it goes with it.
    if app.active_doc().is_none() {
        close_box(app);
        return false;
    }
    let bound = chord_from_key(key, app.keyboard_enhanced).and_then(|c| {
        app.keymap
            .command_for_in(Context::Editor, &c)
            .map(str::to_owned)
    });
    // Esc keeps closing the box although the editor table binds it to editor.read_mode.
    if bound
        .as_deref()
        .is_some_and(|c| !c.starts_with("edit.") && c != "editor.read_mode")
    {
        // The keymap runs it (find.open, find.next, find.replace_open, ...).
        return false;
    }
    let m = key.modifiers;
    let plain = m.is_empty();
    let page = app.find.list_area.height.max(1) as isize;
    match key.code {
        KeyCode::Esc if plain => close_box(app),
        KeyCode::Enter if plain && app.find.field == Field::Replace => ask_replace(app),
        KeyCode::Enter if plain => jump(app),
        KeyCode::Enter if m == KeyModifiers::SHIFT && app.find.field == Field::Find => {
            step(app, false);
        }
        KeyCode::Up if plain => move_in_list(app, -1),
        KeyCode::Down if plain => move_in_list(app, 1),
        KeyCode::PageUp if plain => move_in_list(app, -page),
        KeyCode::PageDown if plain => move_in_list(app, page),
        KeyCode::Tab | KeyCode::BackTab if m.difference(KeyModifiers::SHIFT).is_empty() => {
            if app.find.replace_mode {
                app.find.field = match app.find.field {
                    Field::Find => Field::Replace,
                    Field::Replace => Field::Find,
                };
            }
        }
        _ => match bound.as_deref() {
            Some("edit.paste") => {
                if let Some(text) = app.clipboard.get() {
                    paste(app, &text);
                }
            }
            // Other edit commands would change the file behind the box.
            Some(_) => {}
            None => {
                let field = app.find.field;
                let input = match field {
                    Field::Find => &mut app.find.input,
                    Field::Replace => &mut app.find.replace,
                };
                let before = input.text().to_string();
                if !input.handle_key(key) {
                    return false;
                }
                if field == Field::Find && app.find.input.text() != before {
                    query_changed(app);
                }
            }
        },
    }
    app.dirty = true;
    true
}

pub const ONE: &str = "One";
pub const ALL: &str = "All";

/// Enter in the replace field (D279): asks Replace: One / All / Cancel. One is the default:
/// an in-file replacement is one undo step and Enter, Enter goes on to the next match. With no
/// match it only says so; the box keeps the keys either way.
fn ask_replace(app: &mut App) {
    refresh_now(app);
    let n = app.find.matches.len();
    if n == 0 {
        let msg = format!("Replace: no results for {:?}", app.find.input.text());
        app.info(msg);
        return;
    }
    let name = app.active_name();
    let message = format!(
        "Replace {:?} with {:?} in {name} ({n} match{})?",
        app.find.input.text(),
        app.find.replace.text(),
        if n == 1 { "" } else { "es" }
    );
    let dialog = ConfirmDialog::new("Replace", message, &[ONE, ALL, super::file::CANCEL]);
    app.open_dialog(DialogWidget::Confirm(dialog), DialogAction::ReplaceInFile);
}

/// The Replace dialog's answer: `Some(0)` = One, `Some(1)` = All; Cancel and Esc replace
/// nothing. The dialog took the keys and with them edit mode: the replace row gets it back
/// whatever the answer, as the box was before the question.
pub fn answer_replace(app: &mut App, button: Option<usize>) {
    if app.find.open && app.focus == Focus::FindBar && app.active_doc().is_some() {
        app.enter_edit_mode();
    }
    match button {
        Some(0) => {
            let _ = run(app, replace_one);
        }
        Some(1) => {
            let _ = run(app, replace_all);
        }
        _ => {}
    }
}

/// Runs a command function, turning an error into a notice like `App::run_command`.
fn run(app: &mut App, f: fn(&mut App, CmdArgs) -> CmdResult) -> CmdResult {
    if let Err(e) = f(app, CmdArgs::None) {
        app.error(format!("Replace: {e}"));
    }
    Ok(())
}

/// Bracketed paste (or ctrl+v) into the focused field.
pub fn paste(app: &mut App, text: &str) {
    match app.find.field {
        Field::Find => {
            app.find.input.insert_str(text);
            query_changed(app);
        }
        Field::Replace => app.find.replace.insert_str(text),
    }
    app.dirty = true;
}

/// Clicks on the box: a field row gives it the keys there, a list row selects that result
/// (File scope: the editor shows the match; Project scope: the result opens, the box keeps
/// the keys), the wheel moves the selection.
pub fn handle_mouse(app: &mut App, ev: &MouseEvent) {
    if app.find.scope == Scope::Project {
        super::search::handle_mouse(app, ev);
        return;
    }
    let list = app.find.list_area;
    let in_list = ev.column >= list.x
        && ev.column < list.x + list.width
        && ev.row >= list.y
        && ev.row < list.y + list.height;
    match ev.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            app.focus = Focus::FindBar;
            if in_list {
                let i = app.find.top + usize::from(ev.row - list.y);
                if i < app.find.matches.len() {
                    select(app, i);
                }
            } else {
                app.find.field = match ev.row.saturating_sub(app.find.area.y) {
                    1 if app.find.replace_mode => Field::Replace,
                    _ => Field::Find,
                };
            }
        }
        MouseEventKind::ScrollDown => move_in_list(app, 1),
        MouseEventKind::ScrollUp => move_in_list(app, -1),
        _ => {}
    }
    app.dirty = true;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(pattern: &str, regex: bool, whole_word: bool) -> Query {
        Query {
            pattern: pattern.to_string(),
            regex,
            case_sensitive: false,
            whole_word,
        }
    }

    /// Searching in chunks of lines finds exactly what one `find_all` over the text finds,
    /// zero-length matches at chunk edges included.
    #[test]
    fn chunked_find_equals_find_all() {
        let texts = [
            "foo bar\nfoo\n\nbaz foo\r\nfoo",
            "é foo ü\nfoo\n",
            "a\u{2028}b foo\u{2028}x\ra\u{85}b\n",
            "\n\n\n",
            "line without break",
            "",
        ];
        let queries = [
            q("foo", false, false),
            q("foo", false, true),
            q("^", true, false),
            q("x*", true, false),
            q("$", true, false),
            q(r"\w+", true, false),
            q("a.b", true, false),
        ];
        for t in texts {
            let rope = Rope::from_str(t);
            for query in &queries {
                let want = vr_search::find_all(&rope, query).unwrap();
                for chunk in [1, 3, 7, 1 << 20] {
                    let (got, capped) = find_capped(&rope, query, usize::MAX, chunk).unwrap();
                    assert_eq!(got, want, "{t:?} {:?} chunk {chunk}", query.pattern);
                    assert!(!capped);
                }
            }
        }
    }

    #[test]
    fn capped_find_stops_and_says_so() {
        let rope = Rope::from_str(&"ab\n".repeat(10));
        let (m, capped) = find_capped(&rope, &q("a", false, false), 4, 5).unwrap();
        assert_eq!(m, vec![(0, 1), (3, 4), (6, 7), (9, 10)]);
        assert!(capped);
        let (m, capped) = find_capped(&rope, &q("a", false, false), 10, 5).unwrap();
        assert_eq!(m.len(), 10);
        assert!(!capped, "exactly the cap is not more");
    }
}
