//! The find box between the tab bar and the editor (T22): its scope (File / Project), the
//! query with the match count (File) or the search status (Project), the replacement row
//! (with how to replace), then the list of results over the full width of the editor
//! column. File scope lists every match as `line  text`; Project scope lists the results
//! grouped by root and file, or the replace preview (`ui::search_panel`).

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;

use crate::app::{App, Focus};
use crate::commands::find::{self, Field, Scope};
use crate::commands::search;
use crate::theme::Theme;
use crate::ui::dialog::{LineInput, wrap};
use crate::ui::search_panel;

/// Below this width the labels shrink to one letter.
const NARROW: u16 = 36;

/// Shown at the end of the replace row: Enter asks whether to replace this match or all of them.
const REPLACE_HELP: &str = "enter replace";
/// Project scope: Enter in the replace field shows the preview.
const PREVIEW_HELP: &str = "enter preview";

/// Fewest list rows the box shows (when the editor has room for them and one row).
const LIST_MIN_ROWS: u16 = 3;

/// Rows of the box's fields: 0 closed, 1 find, 2 find and replace.
pub fn field_rows(app: &App) -> u16 {
    if !app.find.open {
        return 0;
    }
    let replace = match app.find.scope {
        Scope::File if app.active_doc().is_none() => return 0,
        Scope::File => app.find.replace_mode,
        Scope::Project => app.search.replace_mode,
    };
    1 + u16::from(replace)
}

/// List rows of a box whose fields and editor share `editor_rows` rows `width` wide: as
/// long as what it shows, from [`LIST_MIN_ROWS`] up to two fifths of the rows under the
/// fields; one editor row always stays.
pub fn list_rows(app: &App, editor_rows: u16, width: u16) -> u16 {
    let fields = field_rows(app);
    if fields == 0 {
        return 0;
    }
    let room = editor_rows.saturating_sub(fields + 1);
    let (notes, results) = content_rows(app, width);
    let to_u16 = |n: usize| u16::try_from(n).unwrap_or(u16::MAX);
    // The notes (a status that does not fit row 0, errors, the hint) come on top of the
    // results' share, so they never leave the list without a row (the replace preview's
    // "Space toggles" over the checkbox row it is about).
    let most = (editor_rows.saturating_sub(fields) * 2 / 5)
        .max(LIST_MIN_ROWS)
        .saturating_add(to_u16(notes));
    let want = to_u16(notes).saturating_add(to_u16(results));
    want.clamp(LIST_MIN_ROWS, most).min(room)
}

/// Rows the list would fill: its notes (a status that does not fit row 0, errors, the hint
/// of an empty box) and its results.
fn content_rows(app: &App, width: u16) -> (usize, usize) {
    let wrapped = |text: &str| wrap(text, width).len();
    let (errors, empty, results) = match app.find.scope {
        Scope::File => (
            app.find.error.as_slice(),
            app.find.input.text().is_empty(),
            app.find.matches.len(),
        ),
        Scope::Project => {
            let s = &app.search;
            let results = s
                .preview
                .as_ref()
                .map_or_else(|| s.row_count(), |p| p.rows().len());
            (
                s.errors.as_slice(),
                s.input.text().is_empty() && s.preview.is_none(),
                results,
            )
        }
    };
    let notes: usize = errors.iter().map(|e| wrapped(e)).sum();
    let status = match app.find.scope {
        Scope::Project => {
            let s = search_status(app);
            let w = Span::raw(s.as_str()).width() as u16;
            let scope = scope_label(true, width < NARROW);
            if w > 0 && !count_fits(width, scope, w) {
                wrapped(&s)
            } else {
                0
            }
        }
        Scope::File => 0,
    };
    let hint = if empty { wrapped(&hint(app)) } else { 0 };
    (status + notes + hint, results)
}

/// The scope label at the start of row 0.
fn scope_label(project: bool, narrow: bool) -> &'static str {
    match (project, narrow) {
        (false, false) => " File ",
        (false, true) => " F ",
        (true, false) => " Project ",
        (true, true) => " P ",
    }
}

/// Row 0 of a box `width` wide has room for a count / status `count_w` wide after the scope
/// label and a query field of at least 6 cells.
fn count_fits(width: u16, scope: &str, count_w: u16) -> bool {
    width > scope.len() as u16 + 1 + count_w + 2 + 6
}

pub fn render(frame: &mut Frame, area: Rect, app: &mut App, theme: &Theme) {
    app.find.area = area;
    app.find.list_area = Rect::default();
    if app.find.scope == Scope::Project {
        app.search.area = area;
        app.search.list_area = Rect::default();
    }
    // D275: a replace field the window has no row for gives the cursor to the query, so
    // nothing is typed into it or replaced unseen.
    if app.find.open && (area.height < 2 || area.width < 4) {
        match app.find.scope {
            Scope::File if app.find.field == Field::Replace => app.find.field = Field::Find,
            Scope::Project if app.search.field == search::Field::Replace => {
                app.search.field = search::Field::Query;
            }
            _ => {}
        }
    }
    if area.height == 0 || area.width < 4 {
        return;
    }
    // The open file shows the query's matches in both scopes.
    find::refresh(app);
    let fields = field_rows(app).min(area.height);
    let narrow = area.width < NARROW;
    let project = app.find.scope == Scope::Project;
    let focused = app.dialog.is_none()
        && if project {
            app.focus == Focus::Search
        } else {
            app.focus == Focus::FindBar
        };
    let mut cursor = None;
    let buf = frame.buffer_mut();
    buf.set_style(Rect::new(area.x, area.y, area.width, fields), theme.status);

    // Row 0: scope, query, count (File) or status (Project).
    let y = area.y;
    let scope = scope_label(project, narrow);
    let scope_style = if focused {
        theme.button_selected
    } else {
        theme.button
    };
    let mut x = buf
        .set_stringn(area.x, y, scope, usize::from(area.width), scope_style)
        .0
        + 1;
    let (input, query_field) = if project {
        (&app.search.input, app.search.field == search::Field::Query)
    } else {
        (&app.find.input, app.find.field == Field::Find)
    };
    let (count, count_style) = if project {
        (search_status(app), theme.status)
    } else if app.find.error.is_some() {
        (find::count_text(app), theme.status.patch(theme.error))
    } else {
        (find::count_text(app), theme.status)
    };
    let end = area.x + area.width;
    let count_w = Span::raw(count.as_str()).width() as u16;
    let show_count = count_w > 0 && count_fits(area.width, scope, count_w);
    let field_w = end
        .saturating_sub(x + if show_count { count_w + 2 } else { 0 })
        .max(1);
    if let Some(c) = draw_input(buf, x, y, field_w, input, theme)
        && focused
        && query_field
    {
        cursor = Some((c, y));
    }
    x += field_w;
    if show_count {
        buf.set_stringn(x + 1, y, &count, usize::from(count_w), count_style);
    }
    // A project status too long for row 0 heads the list instead: it says what Enter and
    // Space do in the preview.
    let status_note = (project && count_w > 0 && !show_count).then_some(count);

    // Row 1: the replacement.
    if fields > 1 {
        let y = area.y + 1;
        let label = if narrow { "R " } else { "Replace " };
        let label_w = label.len() as u16;
        buf.set_string(area.x, y, label, theme.status);
        let help = match (narrow, project) {
            (true, _) => "",
            (false, false) => REPLACE_HELP,
            (false, true) => PREVIEW_HELP,
        };
        let help_w = Span::raw(help).width() as u16;
        let show_help = !help.is_empty() && area.width >= label_w + help_w + 2 + 10;
        let reserved = if show_help { help_w + 2 } else { 1 };
        let w = area.width.saturating_sub(label_w + reserved).max(1);
        if show_help {
            buf.set_string(end - help_w, y, help, theme.status.patch(theme.dim));
        }
        let (replace, on) = if project {
            (
                &app.search.replace,
                app.search.field == search::Field::Replace,
            )
        } else {
            (&app.find.replace, app.find.field == Field::Replace)
        };
        if let Some(c) = draw_input(buf, area.x + label_w, y, w, replace, theme)
            && focused
            && on
        {
            cursor = Some((c, y));
        }
    }

    // The list.
    let list = Rect::new(area.x, area.y + fields, area.width, area.height - fields);
    if list.height > 0 {
        buf.set_style(list, theme.popup);
        if project {
            render_project_list(buf, list, app, status_note, focused, theme);
        } else {
            render_file_list(buf, list, app, focused, theme);
        }
    }
    if let Some(pos) = cursor {
        frame.set_cursor_position(pos);
    }
}

/// Project scope, row 0: the search status, or what the preview / replace is doing.
fn search_status(app: &App) -> String {
    match &app.search.preview {
        Some(p) if p.job.is_some() => p.progress_text().unwrap_or_default(),
        Some(p) if p.applied => "Replaced; Esc searches again".to_string(),
        Some(_) => "Space toggles, Enter replaces".to_string(),
        None => app.search.status_text(),
    }
}

/// Lines shown in an empty list: how to search, and the key that switches the scope (not
/// with no file open: the project box cannot switch to a file then).
fn hint(app: &App) -> String {
    const SYNTAX: &str = "/re/ regex · …/i ignore case · \\/ a slash";
    let other = match app.find.scope {
        Scope::File => "project",
        Scope::Project if app.active_doc().is_some() => "file",
        Scope::Project => return SYNTAX.to_string(),
    };
    let key = app.key_hint("find.open");
    format!("{key}: search the {other} · {SYNTAX}")
}

/// Notes above the list (a status, errors, the hint of an empty box), each wrapped: they
/// must be read; returns the rows left for the list.
fn notes(buf: &mut Buffer, list: Rect, notes: &[(String, Style)]) -> Rect {
    let mut y = list.y;
    let bottom = list.y + list.height;
    let lines = notes
        .iter()
        .flat_map(|(text, style)| wrap(text, list.width).into_iter().map(move |l| (l, *style)));
    for (line, style) in lines {
        if y >= bottom {
            break;
        }
        buf.set_stringn(list.x, y, &line, usize::from(list.width), style);
        y += 1;
    }
    Rect::new(list.x, y, list.width, bottom - y)
}

fn render_project_list(
    buf: &mut Buffer,
    list: Rect,
    app: &mut App,
    status: Option<String>,
    focused: bool,
    theme: &Theme,
) {
    let dim = theme.popup.patch(theme.dim);
    let warn = theme.popup.patch(theme.warning);
    let mut lines: Vec<(String, Style)> = status.into_iter().map(|s| (s, dim)).collect();
    lines.extend(app.search.errors.iter().map(|e| (e.clone(), warn)));
    if app.search.input.text().is_empty() && app.search.preview.is_none() {
        lines.push((hint(app), dim));
    }
    let rest = notes(buf, list, &lines);
    app.search.list_area = rest;
    if rest.height > 0 {
        search_panel::render_list(buf, rest, app, focused, theme);
    }
}

/// File scope: one row per match, `line  text` with the match highlighted; only the rows in
/// view are read from the document (the list is virtual).
fn render_file_list(buf: &mut Buffer, list: Rect, app: &mut App, focused: bool, theme: &Theme) {
    let warn = theme.popup.patch(theme.warning);
    let mut lines: Vec<(String, Style)> =
        app.find.error.iter().map(|e| (e.clone(), warn)).collect();
    if app.find.input.text().is_empty() {
        lines.push((hint(app), theme.popup.patch(theme.dim)));
    }
    let rest = notes(buf, list, &lines);
    app.find.list_area = rest;
    let n = app.find.matches.len();
    if rest.height == 0 || n == 0 {
        return;
    }
    let Some(doc) = app.active_doc() else {
        return;
    };
    let rope = &doc.buffer.text().clone();
    let selected = find::selected_row(app);
    let height = usize::from(rest.height);
    let f = &mut app.find;
    if selected < f.top {
        f.top = selected;
    } else if selected >= f.top + height {
        f.top = selected + 1 - height;
    }
    f.top = f.top.min(n.saturating_sub(1));
    let last_line = rope.char_to_line(f.matches[n - 1].0) + 1;
    let num_w = last_line.to_string().len();
    let sel_style = if focused {
        theme.list_selected
    } else {
        theme.selection
    };
    let end = rest.x + rest.width;
    for (row, i) in (f.top..n).take(height).enumerate() {
        let y = rest.y + row as u16;
        let base = if i == selected {
            sel_style
        } else {
            theme.popup
        };
        buf.set_style(Rect::new(rest.x, y, rest.width, 1), base);
        let (s, e) = f.matches[i];
        let line = rope.char_to_line(s);
        let x = put(
            buf,
            rest.x,
            y,
            end,
            &format!("{:>num_w$}  ", line + 1),
            base.patch(theme.dim),
        );
        let room = usize::from(end.saturating_sub(x));
        let (before, hit, after) = line_window(rope, line, s, e, room);
        let x = put(buf, x, y, end, &before, base);
        let x = put(buf, x, y, end, &hit, base.patch(theme.matched));
        put(buf, x, y, end, &after, base);
    }
}

/// The part of line `line` to show for the match `s..e` in `room` cells: leading indentation
/// dropped, a match far right shown with a little context before it ("…" marks the cut).
/// Reads at most about `room` chars of the line, so a very long line costs no more than a
/// short one.
fn line_window(
    rope: &vr_core::ropey::Rope,
    line: usize,
    s: usize,
    e: usize,
    room: usize,
) -> (String, String, String) {
    let start = rope.line_to_char(line);
    let stop = vr_core::text::line_content_end(rope, line);
    let (s, e) = (s.clamp(start, stop), e.clamp(start, stop));
    let indent = rope
        .slice(start..s)
        .chars()
        .take(256)
        .take_while(|c| c.is_whitespace())
        .count();
    let from = if s - start - indent > room / 2 {
        s - 8.min(s - start)
    } else {
        start + indent
    };
    let cut = from > start + indent;
    let to = (from + room + 1).min(stop);
    let text = |a: usize, b: usize| -> String {
        rope.slice(a.min(b)..b)
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect()
    };
    let mut before = text(from, s.min(to));
    if cut {
        before.insert(0, '…');
    }
    (before, text(s.min(to), e.min(to)), text(e.min(to), to))
}

/// Draws `text` from `x` to `end`, returning the column after it.
fn put(buf: &mut Buffer, x: u16, y: u16, end: u16, text: &str, style: Style) -> u16 {
    if x >= end {
        return x;
    }
    buf.set_stringn(x, y, text, usize::from(end - x), style).0
}

/// Draws `input` in `w` cells at (`x`, `y`); returns the cursor's screen column.
fn draw_input(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    w: u16,
    input: &LineInput,
    theme: &Theme,
) -> Option<u16> {
    if w == 0 {
        return None;
    }
    buf.set_style(Rect::new(x, y, w, 1), theme.input);
    let (shown, cx) = input.view(w);
    buf.set_stringn(x, y, shown, usize::from(w), theme.input);
    Some(x + cx.min(w - 1))
}
