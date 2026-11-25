//! Palette overlay: input row and the matching commands with their keys (AC-6), or the
//! matching files for go to file.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Span;
use ratatui::widgets::{Block, Clear, Widget};

use crate::app::App;
use crate::commands::palette::PaletteMode;
use crate::theme::Theme;

/// Widest the palette gets.
const MAX_WIDTH: u16 = 72;
/// Most rows the list shows.
const MAX_ROWS: u16 = 12;
/// Most rows a slash command's error takes.
const MAX_ERROR_ROWS: u16 = 3;
/// Cells of a slash command's description kept beside a title too wide for the row.
const SLASH_DESC_MIN: usize = 10;

pub fn render(frame: &mut Frame, area: Rect, app: &mut App, theme: &Theme) {
    let Some(p) = &mut app.palette else {
        return;
    };
    // Borders, the input and one list row, above the status row (D122).
    if area.width < 10 || area.height < 6 {
        return;
    }
    // A cell of margin each side when the screen allows, just under the tab bar.
    let w = area
        .width
        .saturating_sub(4)
        .clamp(area.width.min(20), MAX_WIDTH);
    // The status row stays visible: the box ends above it.
    let room = area.height - 2;
    // A slash command's error goes under the input (T20), wrapped to the box, in at most
    // [`MAX_ERROR_ROWS`] rows and never at the cost of the input or the one list row.
    let err_lines: Vec<String> = p.error.as_deref().map_or_else(Vec::new, |e| {
        let mut l = crate::ui::dialog::wrap(e, w.saturating_sub(4));
        l.truncate(usize::from(MAX_ERROR_ROWS.min(room.saturating_sub(4))));
        l
    });
    let err_rows = err_lines.len() as u16;
    let list_rows = (p.items.len().max(1) as u16)
        .min(MAX_ROWS)
        .min(room.saturating_sub(3 + err_rows).max(1));
    let h = (list_rows + 3 + err_rows).min(room);
    let rect = Rect::new(area.x + (area.width - w) / 2, area.y + 1, w, h);
    let buf = frame.buffer_mut();
    Clear.render(rect, buf);
    Block::bordered()
        .title(match p.mode {
            PaletteMode::Commands => " Command Palette ",
            PaletteMode::Files => " Go to File ",
        })
        .style(theme.popup)
        .border_style(theme.popup_border)
        .render(rect, buf);

    let inner_w = rect.width.saturating_sub(2);
    let x0 = rect.x + 1;
    let prompt = "> ";
    let pw = prompt.len() as u16;
    buf.set_style(Rect::new(x0, rect.y + 1, inner_w, 1), theme.input);
    buf.set_string(x0, rect.y + 1, prompt, theme.input);
    let field_w = inner_w.saturating_sub(pw);
    let (shown, cx) = p.input.view(field_w);
    buf.set_stringn(
        x0 + pw,
        rect.y + 1,
        shown,
        usize::from(field_w),
        theme.input,
    );
    let cursor = (x0 + pw + cx.min(field_w.saturating_sub(1)), rect.y + 1);

    // Keep the selection visible.
    let rows = usize::from(list_rows);
    if p.selected < p.top {
        p.top = p.selected;
    } else if p.selected >= p.top + rows {
        p.top = p.selected + 1 - rows;
    }
    for (i, e) in err_lines.iter().enumerate() {
        buf.set_stringn(
            x0 + 1,
            rect.y + 2 + i as u16,
            e,
            usize::from(inner_w.saturating_sub(2)),
            theme.popup.patch(theme.error),
        );
    }
    let list = Rect::new(x0, rect.y + 2 + err_rows, inner_w, list_rows);
    p.list_area = list;
    p.area = rect;
    let width = usize::from(inner_w.saturating_sub(2));
    let slash =
        p.mode == PaletteMode::Commands && crate::commands::palette::is_slash(p.input.text());
    if p.items.is_empty() {
        buf.set_stringn(
            x0 + 1,
            list.y,
            p.empty_text(),
            width,
            theme.popup.patch(theme.dim),
        );
    }
    for (row, (i, item)) in p
        .items
        .iter()
        .enumerate()
        .skip(p.top)
        .take(rows)
        .enumerate()
    {
        let y = list.y + row as u16;
        let base = if i == p.selected {
            theme.list_selected
        } else {
            theme.popup
        };
        buf.set_style(Rect::new(x0, y, inner_w, 1), base);
        let title_w = Span::raw(item.title.as_str()).width();
        let keys_w = Span::raw(item.keys.as_str()).width();
        let name_w = Span::raw(item.detail.as_str()).width();
        // Title first, then the keys (AC-6), then the command name when there is room.
        let show_keys = !item.keys.is_empty() && title_w + 2 + keys_w <= width;
        let used = title_w + if show_keys { keys_w + 2 } else { 0 };
        let show_name = used + 2 + name_w <= width;
        let end = x0 + 1 + width as u16;
        // A slash row too wide ends its title in "…" and keeps a short tail of the
        // description when there is room for both (D270).
        let title = if slash && !show_name {
            let keep = if item.detail.is_empty() || width < 2 + 2 * SLASH_DESC_MIN {
                width
            } else {
                width - 2 - SLASH_DESC_MIN.min(name_w)
            };
            ellipsize(&item.title, keep)
        } else {
            item.title.clone()
        };
        let mut x = x0 + 1;
        for (ci, ch) in title.chars().enumerate() {
            let s = ch.to_string();
            let cw = Span::raw(s.as_str()).width() as u16;
            if x + cw > end {
                break;
            }
            let style = if item.hits.contains(&ci) {
                base.patch(theme.matched)
            } else {
                base
            };
            buf.set_string(x, y, &s, style);
            x += cw;
        }
        if show_name {
            buf.set_string(x + 2, y, &item.detail, base.patch(theme.dim));
        } else if slash && !item.detail.is_empty() && x + 2 + 4 <= end {
            // A slash command keeps the start of its description (T20).
            let room = usize::from(end - x - 2);
            let cut = ellipsize(&item.detail, room);
            buf.set_string(x + 2, y, &cut, base.patch(theme.dim));
        }
        if show_keys {
            let kx = end - keys_w as u16;
            buf.set_string(kx, y, &item.keys, base.patch(theme.dim));
        }
    }
    frame.set_cursor_position(cursor);
}

/// `text` cut to `cells` display cells, ending in "…" when cut.
fn ellipsize(text: &str, cells: usize) -> String {
    if Span::raw(text).width() <= cells {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let cw = Span::raw(ch.to_string()).width();
        if used + cw + 1 > cells {
            break;
        }
        out.push(ch);
        used += cw;
    }
    out.push('…');
    out
}
