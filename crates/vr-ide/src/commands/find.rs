//! `find.*` commands and the in-file find / replace bar (AC-10). Rendering: `ui::find_bar`.
//!
//! Built-in keys while the bar has focus (like the palette's, not in the keymap): typing and
//! line editing, Enter = next match (in the replace field: replace one), shift+Enter =
//! previous, Tab / shift+Tab switch between the fields, Esc closes the bar. Match case, whole
//! word and regex are the `find.toggle_*` commands (ctrl+f1 / ctrl+f2 / ctrl+f3, D250), which act
//! on the open bar wherever the keys are. A key the keymap binds for the editor runs its command
//! first (f3, ctrl+u, f7, shift+f7, D111), except
//! the `edit.*` commands, which would change the file behind the bar: there ctrl+v pastes
//! into the field and the others do nothing.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use vr_config::Context;
use vr_core::ropey::Rope;
use vr_core::{Selection, Transaction};
use vr_search::Query;

use super::registry::{CmdArgs, CmdResult, CommandSpec};
use crate::app::{App, Focus};
use crate::event_loop::chord_from_key;
use crate::ui::dialog::LineInput;

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "find.toggle_case",
        title: "Find: Toggle Match Case",
        run: toggle_case,
    },
    CommandSpec {
        name: "find.toggle_word",
        title: "Find: Toggle Whole Word",
        run: toggle_word,
    },
    CommandSpec {
        name: "find.toggle_regex",
        title: "Find: Toggle Regular Expression",
        run: toggle_regex,
    },
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
    /// More matches exist than `matches` holds (it stopped at [`MATCH_CAP`]).
    pub capped: bool,
    /// Why the query finds nothing (an invalid regex).
    pub error: Option<String>,
    cache: Option<CacheKey>,
    pending: Option<Pending>,
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

/// A rescan waiting for typing to pause.
#[derive(Debug, Clone)]
struct Pending {
    key: CacheKey,
    due: Instant,
    /// Select the first match from where the bar opened once the scan ran (the query
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

/// The bar stops collecting matches here (D124, VS Code stops near the same number): the
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
    app.find.pending = None;
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

/// The option toggles (ctrl+f1 / ctrl+f2 / ctrl+f3, D250) act on the panel that has the keys;
/// from elsewhere on the open find bar, else on the shown project search panel. With neither,
/// a hint names the key that opens Find.
fn toggle_option_key(app: &mut App, c: char) -> CmdResult {
    let search_shown = app.search.visible;
    match app.focus {
        Focus::FindBar => toggle(app, c),
        Focus::Search if search_shown => super::search::toggle_option(app, c),
        _ if app.find.open => toggle(app, c),
        _ if search_shown => super::search::toggle_option(app, c),
        _ => {
            let key = app.key_hint("find.open");
            app.info(format!("Open Find ({key}) or the project search first"));
        }
    }
    Ok(())
}

fn toggle_case(app: &mut App, _: CmdArgs) -> CmdResult {
    toggle_option_key(app, 'c')
}

fn toggle_word(app: &mut App, _: CmdArgs) -> CmdResult {
    toggle_option_key(app, 'w')
}

fn toggle_regex(app: &mut App, _: CmdArgs) -> CmdResult {
    toggle_option_key(app, 'r')
}

pub(crate) fn toggle(app: &mut App, c: char) {
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
    // Esc keeps closing the bar although the editor table binds it to editor.read_mode.
    if bound
        .as_deref()
        .is_some_and(|c| !c.starts_with("edit.") && c != "editor.read_mode")
    {
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
