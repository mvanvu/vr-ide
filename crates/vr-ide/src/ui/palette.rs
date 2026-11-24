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

pub fn render(frame: &mut Frame, area: Rect, app: &mut App, theme: &Theme) {
    let Some(p) = &mut app.palette else {
        return;
    };
    if area.width < 10 || area.height < 4 {
        return;
    }
    // A cell of margin each side when the screen allows, just under the tab bar.
    let w = area
        .width
        .saturating_sub(4)
        .clamp(area.width.min(20), MAX_WIDTH);
    let room = area.height - 1;
    let list_rows = (p.items.len().max(1) as u16)
        .min(MAX_ROWS)
        .min(room.saturating_sub(3).max(1));
    let h = (list_rows + 3).min(room);
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
    let list = Rect::new(x0, rect.y + 2, inner_w, list_rows);
    p.list_area = list;
    p.area = rect;
    let width = usize::from(inner_w.saturating_sub(2));
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
        let mut x = x0 + 1;
        for (ci, ch) in item.title.chars().enumerate() {
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
        }
        if show_keys {
            let kx = end - keys_w as u16;
            buf.set_string(kx, y, &item.keys, base.patch(theme.dim));
        }
    }
    frame.set_cursor_position(cursor);
}
