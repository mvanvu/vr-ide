//! The result list of the find box in Project scope (AC-11, AC-12, T22): results grouped by
//! root (several workspace roots) and file, or the replace preview with each file's
//! checkbox, changed lines and result. The box's fields, status and errors are drawn by
//! `ui::find_bar`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};

use crate::app::App;
use crate::commands::search::{Field, Preview, PreviewRow, Row, SearchPanel, Target};
use crate::theme::Theme;

/// Draws the results (or the preview) in `list`; the selected row is highlighted while the
/// list has the keys (Down or Tab from the fields).
pub fn render_list(buf: &mut Buffer, list: Rect, app: &mut App, focused: bool, theme: &Theme) {
    let root = app.workspace.clone();
    let s = &mut app.search;
    let show_sel = s.field == Field::Results;
    let sel_style = if focused {
        theme.list_selected
    } else {
        theme.selection
    };
    let sel = show_sel.then_some(sel_style);
    if let Some(p) = &mut s.preview {
        render_preview(buf, list, p, &root, sel, theme);
    } else {
        render_results(buf, list, s, &root, sel, theme);
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

/// File name and folder of a result; the folder starts with the root's name when the
/// workspace has several roots.
fn file_label(path: &std::path::Path, ws: &crate::workspace::Workspace) -> (String, String) {
    let shown = std::path::PathBuf::from(ws.display_path(path));
    let rel = shown.as_path();
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
    root: &crate::workspace::Workspace,
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
            _ => theme.popup,
        };
        buf.set_style(Rect::new(list.x, y, list.width, 1), base);
        match *row {
            Row::Root(gi) => {
                let name = s.groups[gi].root.as_deref().unwrap_or_default();
                let style = base.patch(theme.info).add_modifier(Modifier::BOLD);
                put(buf, list.x, y, end, name, style);
            }
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
    root: &crate::workspace::Workspace,
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
            _ => theme.popup,
        };
        buf.set_style(Rect::new(list.x, y, list.width, 1), base);
        match *row {
            PreviewRow::File(fi) => {
                let f = &p.files[fi];
                let mark = if matches!(f.target, Target::Unknown) {
                    "[…] "
                } else if f.error.is_some() {
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
                    (None, None) => (String::new(), theme.popup),
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
