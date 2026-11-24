//! The project search / replace panel in the side column (AC-11, AC-12): title and
//! toggles, the query (and replacement) fields, a status line, the search errors (files too
//! large, the match limit, an invalid regex) and the list: results grouped by file, or the
//! replace preview with each file's checkbox, changed lines and result.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::app::{App, Focus};
use crate::commands::search::{Field, Preview, PreviewRow, Row, SearchPanel};
use crate::theme::Theme;
use crate::ui::dialog::{LineInput, wrap};

pub fn render(frame: &mut Frame, area: Rect, app: &mut App, theme: &Theme) {
    app.search.area = area;
    if area.width < 4 || area.height < 3 {
        app.search.list_area = Rect::default();
        return;
    }
    let focused = app.focus == Focus::Search && app.dialog.is_none();
    let root = app.root.clone();
    let s = &mut app.search;
    let buf = frame.buffer_mut();
    let inner = area.width - 1;
    let x0 = area.x;
    let bottom = area.y + area.height;
    buf.set_style(Rect::new(x0, area.y, inner, area.height), theme.text);
    for y in area.y..bottom {
        buf.set_string(x0 + inner, y, "│", theme.dim);
    }
    let mut y = area.y;
    let mut cursor = None;

    // Title and toggles.
    let title = if s.replace_mode { "REPLACE" } else { "SEARCH" };
    buf.set_stringn(
        x0,
        y,
        title,
        usize::from(inner),
        theme.text.add_modifier(Modifier::BOLD),
    );
    let toggles = [
        ("Aa", s.case_sensitive, false),
        (".*", s.regex, false),
        ("ab", s.whole_word, true),
    ];
    if inner >= title.len() as u16 + 10 {
        let mut x = x0 + inner - 9;
        for (text, on, underline) in toggles {
            let mut style = if on { theme.button_selected } else { theme.dim };
            if underline {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            buf.set_string(x, y, text, style);
            x += 3;
        }
    }
    y += 1;

    // Fields.
    let field_row = |buf: &mut Buffer, y: u16, label: &str, input: &LineInput| {
        buf.set_string(x0, y, label, theme.dim);
        let w = inner.saturating_sub(label.len() as u16).max(1);
        let x = x0 + label.len() as u16;
        buf.set_style(Rect::new(x, y, w, 1), theme.input);
        let (shown, cx) = input.view(w);
        buf.set_stringn(x, y, shown, usize::from(w), theme.input);
        (x + cx.min(w - 1), y)
    };
    let pos = field_row(buf, y, "> ", &s.input);
    if focused && s.field == Field::Query {
        cursor = Some(pos);
    }
    y += 1;
    if s.replace_mode && y < bottom {
        let pos = field_row(buf, y, "R ", &s.replace);
        if focused && s.field == Field::Replace {
            cursor = Some(pos);
        }
        y += 1;
    }

    // Status, then the errors (wrapped: they must be read).
    let status = match &s.preview {
        Some(p) if p.applied => "Replaced; Esc searches again".to_string(),
        Some(_) => "Space toggles, Enter replaces".to_string(),
        None => s.status_text(),
    };
    if y < bottom && !status.is_empty() {
        buf.set_stringn(x0, y, &status, usize::from(inner), theme.dim);
        y += 1;
    }
    for e in &s.errors {
        for line in wrap(e, inner) {
            if y >= bottom {
                break;
            }
            buf.set_stringn(x0, y, &line, usize::from(inner), theme.warning);
            y += 1;
        }
    }

    // The list.
    let list = Rect::new(x0, y, inner, bottom.saturating_sub(y));
    s.list_area = list;
    if list.height > 0 {
        let show_sel = s.field == Field::Results;
        let sel_style = if focused {
            theme.list_selected
        } else {
            theme.selection
        };
        if let Some(p) = &mut s.preview {
            render_preview(buf, list, p, &root, show_sel.then_some(sel_style), theme);
        } else {
            render_results(buf, list, s, &root, show_sel.then_some(sel_style), theme);
        }
    }
    if let Some(pos) = cursor {
        frame.set_cursor_position(pos);
    }
}

/// Keeps `selected` within the `rows` visible from `top`.
fn scroll(top: &mut usize, selected: usize, rows: usize, len: usize) {
    if selected < *top {
        *top = selected;
    } else if selected >= *top + rows {
        *top = selected + 1 - rows;
    }
    *top = (*top).min(len.saturating_sub(1));
}

fn file_label(path: &std::path::Path, root: &std::path::Path) -> (String, String) {
    let rel = path.strip_prefix(root).unwrap_or(path);
    let name = rel
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let dir = rel
        .parent()
        .map_or_else(String::new, |d| d.display().to_string());
    (name, dir)
}

/// Draws `text` from `x` to `end`, returning the column after it.
fn put(buf: &mut Buffer, x: u16, y: u16, end: u16, text: &str, style: Style) -> u16 {
    if x >= end {
        return x;
    }
    buf.set_stringn(x, y, text, usize::from(end - x), style).0
}

fn render_results(
    buf: &mut Buffer,
    list: Rect,
    s: &mut SearchPanel,
    root: &std::path::Path,
    sel: Option<Style>,
    theme: &Theme,
) {
    let rows = s.rows();
    let height = usize::from(list.height);
    scroll(&mut s.top, s.selected, height, rows.len());
    let end = list.x + list.width;
    for (n, (i, row)) in rows.iter().enumerate().skip(s.top).take(height).enumerate() {
        let y = list.y + n as u16;
        let base = match sel {
            Some(st) if i == s.selected => st,
            _ => theme.text,
        };
        buf.set_style(Rect::new(list.x, y, list.width, 1), base);
        match *row {
            Row::File(gi) => {
                let g = &s.groups[gi];
                let (name, dir) = file_label(&g.path, root);
                let count = g.matches.len().to_string();
                let cw = count.len() as u16;
                let text_end = end.saturating_sub(cw + 1).max(list.x);
                let x = put(
                    buf,
                    list.x,
                    y,
                    text_end,
                    &name,
                    base.add_modifier(Modifier::BOLD),
                );
                put(buf, x + 1, y, text_end, &dir, base.patch(theme.dim));
                if list.width > cw + 4 {
                    buf.set_string(end - cw, y, &count, base.patch(theme.dim));
                }
            }
            Row::Match(gi, mi) => {
                let m = &s.groups[gi].matches[mi];
                let x = put(
                    buf,
                    list.x,
                    y,
                    end,
                    &format!("  {}: ", m.line),
                    base.patch(theme.dim),
                );
                let chars: Vec<char> = m.line_text.chars().collect();
                let (ms, me) = (
                    m.match_range.0.min(chars.len()),
                    m.match_range.1.min(chars.len()),
                );
                // Leading indentation is dropped; a match far right is shown with a little
                // context before it.
                let indent = chars[..ms].iter().take_while(|c| c.is_whitespace()).count();
                let room = usize::from(end.saturating_sub(x));
                let start = if ms - indent > room / 2 {
                    ms - 8.min(ms)
                } else {
                    indent
                };
                let mut x = x;
                if start > indent {
                    x = put(buf, x, y, end, "…", base.patch(theme.dim));
                }
                let seg = |a: usize, b: usize| chars[a..b].iter().collect::<String>();
                x = put(buf, x, y, end, &seg(start, ms), base);
                x = put(buf, x, y, end, &seg(ms, me), base.patch(theme.matched));
                put(buf, x, y, end, &seg(me, chars.len()), base);
            }
        }
    }
}

fn render_preview(
    buf: &mut Buffer,
    list: Rect,
    p: &mut Preview,
    root: &std::path::Path,
    sel: Option<Style>,
    theme: &Theme,
) {
    let rows = p.rows();
    let height = usize::from(list.height);
    scroll(&mut p.top, p.selected, height, rows.len());
    let end = list.x + list.width;
    for (n, (i, row)) in rows.iter().enumerate().skip(p.top).take(height).enumerate() {
        let y = list.y + n as u16;
        let base = match sel {
            Some(st) if i == p.selected => st,
            _ => theme.text,
        };
        buf.set_style(Rect::new(list.x, y, list.width, 1), base);
        match *row {
            PreviewRow::File(fi) => {
                let f = &p.files[fi];
                let mark = if f.error.is_some() {
                    "[!] "
                } else if f.checked {
                    "[x] "
                } else {
                    "[ ] "
                };
                let (name, dir) = file_label(&f.path, root);
                let count = f.count.to_string();
                let cw = count.len() as u16;
                let text_end = end.saturating_sub(cw + 1).max(list.x);
                let x = put(buf, list.x, y, text_end, mark, base);
                let x = put(
                    buf,
                    x,
                    y,
                    text_end,
                    &name,
                    base.add_modifier(Modifier::BOLD),
                );
                put(buf, x + 1, y, text_end, &dir, base.patch(theme.dim));
                if f.count > 0 && list.width > cw + 8 {
                    buf.set_string(end - cw, y, &count, base.patch(theme.dim));
                }
            }
            PreviewRow::Note(fi) => {
                let f = &p.files[fi];
                let (text, style) = match (&f.result, &f.error) {
                    (Some(Ok(n)), _) => (format!("    replaced {n}"), theme.info),
                    (Some(Err(e)), _) => (format!("    skipped: {e}"), theme.warning),
                    (None, Some(e)) => (format!("    {e}"), theme.warning),
                    (None, None) => (String::new(), theme.text),
                };
                put(buf, list.x, y, end, &text, base.patch(style));
            }
            PreviewRow::Before(fi, h) => {
                let hk = &p.files[fi].hunks[h];
                let text = format!("  -{} {}", hk.line, hk.before.trim_start());
                put(buf, list.x, y, end, &text, base.patch(theme.dim));
            }
            PreviewRow::After(fi, h) => {
                let hk = &p.files[fi].hunks[h];
                let text = format!("  +{} {}", hk.line, hk.after.trim_start());
                put(buf, list.x, y, end, &text, base.patch(theme.matched));
            }
        }
    }
}
