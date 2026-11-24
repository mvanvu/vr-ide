//! `find.*` commands and the in-file find / replace bar (AC-10). Rendering: `ui::find_bar`.
//!
//! Built-in keys while the bar has focus (like the palette's, not in the keymap): typing and
//! line editing, Enter = next match (in the replace field: replace one), shift+Enter =
//! previous, Tab / shift+Tab switch between the fields, Esc closes the bar, alt+c / alt+r /
//! alt+w toggle match case, regex and whole word (VS Code's defaults). A chord the keymap
//! binds for the editor runs its command first (f3, ctrl+h, ctrl+alt+enter, D111), except
//! the `edit.*` commands, which would change the file behind the bar: there ctrl+v pastes
//! into the field and the others do nothing.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use vr_config::Context;
use vr_core::{Selection, Transaction};
use vr_search::Query;

use super::registry::{CmdArgs, CmdResult, CommandSpec};
use crate::app::{App, Focus};
use crate::event_loop::chord_from_key;
use crate::ui::dialog::LineInput;

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

/// The bar's input field that has the cursor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Field {
    #[default]
    Find,
    Replace,
}

/// State of the find / replace bar. The matches belong to the active document and are
/// recomputed when the query, the document or its text changes.
#[derive(Debug, Clone, Default)]
pub struct FindBar {
    pub open: bool,
    pub replace_mode: bool,
    pub field: Field,
    pub input: LineInput,
    pub replace: LineInput,
    pub case_sensitive: bool,
    pub regex: bool,
    pub whole_word: bool,
    /// Char ranges in the active document, in order.
    pub matches: Vec<(usize, usize)>,
    /// Why the query finds nothing (an invalid regex).
    pub error: Option<String>,
    cache: Option<CacheKey>,
    /// Where the cursor was when the bar opened: typing finds the first match from here.
    origin: usize,
    /// Bar area in the last frame.
    pub area: Rect,
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
        Query {
            pattern: self.input.text().to_string(),
            regex: self.regex,
            case_sensitive: self.case_sensitive,
            whole_word: self.whole_word,
        }
    }
}

/// Recomputes the matches when the query or the active document changed. Used for drawing;
/// commands call [`refresh_now`] so they never act on matches of an earlier text.
pub fn refresh(app: &mut App) {
    let Some(doc) = app.docs.get(app.active) else {
        app.find.matches.clear();
        app.find.cache = None;
        return;
    };
    let key = CacheKey {
        doc: app.active,
        path: doc.path.clone(),
        version: doc.buffer.version(),
        len: doc.buffer.text().len_chars(),
        query: app.find.query(),
    };
    if app.find.cache.as_ref() == Some(&key) {
        return;
    }
    match vr_search::find_all(doc.buffer.text(), &key.query) {
        Ok(m) => {
            app.find.matches = m;
            app.find.error = None;
        }
        Err(e) => {
            app.find.matches.clear();
            app.find.error = Some(e.to_string());
        }
    }
    app.find.cache = Some(key);
}

/// [`refresh`] without the cache.
pub fn refresh_now(app: &mut App) {
    app.find.cache = None;
    refresh(app);
}

/// Index of the match the primary selection covers exactly.
pub fn current(app: &App) -> Option<usize> {
    let r = app.active_doc()?.buffer.selection().primary();
    let want = (r.from(), r.to());
    app.find.matches.binary_search(&want).ok()
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
    match current(app) {
        Some(i) => format!("{}/{n}", i + 1),
        None => format!("?/{n}"),
    }
}

/// `s` with the regex metacharacters escaped (a selection seeds a regex query literally).
fn escape_regex(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Opens the bar (with the replace field when `replace`) and gives it the keys. A
/// selection on one line becomes the query, unless it is a match of the current query
/// (reopening keeps what was searched for).
fn open_bar(app: &mut App, replace: bool) {
    let Some(doc) = app.active_doc() else {
        app.info("Find: no file is open");
        return;
    };
    let rope = doc.buffer.text();
    let r = doc.buffer.selection().primary();
    let seed = (!r.is_empty() && r.to() - r.from() <= 1000)
        .then(|| rope.slice(r.from()..r.to()).to_string())
        .filter(|s| !s.contains(['\n', '\r']));
    let origin = r.from();
    refresh_now(app);
    if let Some(seed) = seed
        && current(app).is_none()
    {
        let text = if app.find.regex {
            escape_regex(&seed)
        } else {
            seed
        };
        app.find.input.set(&text);
    }
    let f = &mut app.find;
    f.open = true;
    f.replace_mode = replace;
    f.field = if replace && !f.input.text().is_empty() {
        Field::Replace
    } else {
        Field::Find
    };
    f.origin = origin;
    app.focus = Focus::FindBar;
    refresh_now(app);
    app.dirty = true;
}

fn close_bar(app: &mut App) {
    app.find.open = false;
    if app.focus == Focus::FindBar {
        app.focus = Focus::Editor;
    }
    app.dirty = true;
}

fn open(app: &mut App, _: CmdArgs) -> CmdResult {
    open_bar(app, false);
    Ok(())
}

fn replace_open(app: &mut App, _: CmdArgs) -> CmdResult {
    open_bar(app, true);
    Ok(())
}

fn select(app: &mut App, i: usize) {
    let Some(&(from, to)) = app.find.matches.get(i) else {
        return;
    };
    if let Some(doc) = app.active_doc_mut() {
        doc.buffer.set_selection(Selection::single(from, to));
    }
    app.cursor_moved();
}

/// Selects the next (or previous) match, wrapping at the end of the file. From a selection
/// that is no match, the next one is the first starting at or after it.
fn step(app: &mut App, forward: bool) {
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
        doc.buffer.break_undo_group();
        doc.buffer.apply(tx);
        doc.buffer.break_undo_group();
    }
    refresh_now(app);
    if n == 0 {
        let msg = format!("Replace All: no results for {:?}", q.pattern);
        app.info(msg);
    } else {
        app.info(format!(
            "Replaced {n} occurrence{}",
            if n == 1 { "" } else { "s" }
        ));
    }
    Ok(())
}

/// After the query changed: the first match from where the bar opened is selected (the
/// cursor goes back there when nothing matches).
fn query_changed(app: &mut App) {
    refresh_now(app);
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

fn toggle(app: &mut App, c: char) {
    let f = &mut app.find;
    match c {
        'c' => f.case_sensitive = !f.case_sensitive,
        'r' => f.regex = !f.regex,
        _ => f.whole_word = !f.whole_word,
    }
    query_changed(app);
}

/// Keys while the bar has focus; true when used.
pub fn handle_key(app: &mut App, key: &KeyEvent) -> bool {
    // The last tab closed under the bar: the bar goes with it.
    if app.active_doc().is_none() {
        close_bar(app);
        return false;
    }
    let bound = chord_from_key(key, app.keyboard_enhanced).and_then(|c| {
        app.keymap
            .command_for_in(Context::Editor, &c)
            .map(str::to_owned)
    });
    if bound.as_deref().is_some_and(|c| !c.starts_with("edit.")) {
        // The keymap runs it (find.next, find.replace_all, ...).
        return false;
    }
    let m = key.modifiers;
    let plain = m.is_empty();
    let shift_only = m == KeyModifiers::SHIFT;
    match key.code {
        KeyCode::Esc if plain => close_bar(app),
        KeyCode::Enter if plain && app.find.field == Field::Replace => {
            let _ = run(app, replace_one);
        }
        KeyCode::Enter if plain => step(app, true),
        KeyCode::Enter if shift_only => step(app, false),
        KeyCode::Tab | KeyCode::BackTab if m.difference(KeyModifiers::SHIFT).is_empty() => {
            if app.find.replace_mode {
                app.find.field = match app.find.field {
                    Field::Find => Field::Replace,
                    Field::Replace => Field::Find,
                };
            }
        }
        KeyCode::Char(c @ ('c' | 'r' | 'w')) if m == KeyModifiers::ALT => toggle(app, c),
        _ => match bound.as_deref() {
            Some("edit.paste") => {
                if let Some(text) = app.clipboard.get() {
                    paste(app, &text);
                }
            }
            // Other edit commands would change the file behind the bar.
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
