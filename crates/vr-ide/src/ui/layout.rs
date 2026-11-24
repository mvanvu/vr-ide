//! Screen split: tab bar, editor, notice lines (only while a notice is shown), status bar.

use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AppLayout {
    pub tabs: Rect,
    pub editor: Rect,
    pub notice: Rect,
    pub status: Rect,
}

/// `notice_rows` is capped so the editor keeps at least a third of the screen.
pub fn compute(area: Rect, notice_rows: u16) -> AppLayout {
    let row = |y: u16, h: u16| Rect::new(area.x, y, area.width, h);
    if area.height < 3 {
        return AppLayout {
            editor: area,
            ..AppLayout::default()
        };
    }
    let notice_rows = notice_rows.min((area.height - 2) / 3);
    let status = row(area.y + area.height - 1, 1);
    let notice = row(status.y - notice_rows, notice_rows);
    let tabs = row(area.y, 1);
    let editor = row(area.y + 1, notice.y - (area.y + 1));
    AppLayout {
        tabs,
        editor,
        notice,
        status,
    }
}
