//! The explorer's right-click menu (T23): a small bordered box at the pointer, one action
//! per row with its key on the right; actions the target refuses are dimmed.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Span;
use ratatui::widgets::{Block, Clear, Widget};

use crate::app::App;
use crate::theme::Theme;

pub fn render(buf: &mut Buffer, screen: Rect, app: &mut App, theme: &Theme) {
    if !app.explorer.visible {
        app.explorer.menu = None;
    }
    let Some(menu) = &mut app.explorer.menu else {
        return;
    };
    menu.place(screen);
    let rect = menu.area;
    if rect.width < 4 || rect.height < 3 {
        return;
    }
    Clear.render(rect, buf);
    Block::bordered()
        .style(theme.popup)
        .border_style(theme.popup_border)
        .render(rect, buf);
    let x0 = rect.x + 1;
    let inner_w = rect.width - 2;
    let end = x0 + inner_w;
    for (i, item) in menu
        .items
        .iter()
        .enumerate()
        .take(usize::from(rect.height - 2))
    {
        let y = rect.y + 1 + i as u16;
        let base = if !item.enabled {
            theme.popup.patch(theme.dim)
        } else if i == menu.selected {
            theme.list_selected
        } else {
            theme.popup
        };
        buf.set_style(Rect::new(x0, y, inner_w, 1), base);
        let key_w = Span::raw(item.key.as_str()).width() as u16;
        // The key is right-aligned with a space of margin; a box cut by a tiny screen keeps
        // the label.
        let key_x = end.saturating_sub(key_w + 1);
        buf.set_stringn(
            x0 + 1,
            y,
            item.label,
            usize::from(inner_w.saturating_sub(1)),
            base,
        );
        let label_end = x0 + 1 + Span::raw(item.label).width() as u16;
        if key_w > 0 && key_x > label_end {
            buf.set_string(key_x, y, &item.key, base.patch(theme.dim));
        }
    }
}
