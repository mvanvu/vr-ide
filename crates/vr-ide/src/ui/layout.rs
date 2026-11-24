//! Screen split: explorer on the left (when shown), tab bar and editor on the right, notice
//! lines (only while a notice is shown) and the status bar across the whole width.

use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AppLayout {
    pub explorer: Rect,
    pub tabs: Rect,
    pub editor: Rect,
    pub notice: Rect,
    pub status: Rect,
}

/// Narrowest screen that still gets an explorer next to the editor.
pub const EXPLORER_MIN_SCREEN: u16 = 30;

/// Explorer width for a screen `width` columns wide (0 = no room).
pub fn explorer_width(width: u16) -> u16 {
    if width < EXPLORER_MIN_SCREEN {
        return 0;
    }
    (width / 4).clamp(16, 40).min(width / 2)
}

/// `notice_rows` is capped so the editor keeps at least a third of the screen;
/// `explorer_cols` (see [`explorer_width`]) is taken from the left of the tab bar and editor.
pub fn compute(area: Rect, notice_rows: u16, explorer_cols: u16) -> AppLayout {
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
    let ew = explorer_cols.min(area.width / 2);
    let explorer = Rect::new(area.x, area.y, ew, notice.y - area.y);
    let right = |y: u16, h: u16| Rect::new(area.x + ew, y, area.width - ew, h);
    let tabs = right(area.y, 1);
    let editor = right(area.y + 1, notice.y - (area.y + 1));
    AppLayout {
        explorer,
        tabs,
        editor,
        notice,
        status,
    }
}
