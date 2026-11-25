//! Screen split: the explorer on the left (when shown), tab bar, find box (while open:
//! its fields and its result list, [`place_find_box`]) and editor on the right, notice
//! lines (only while a notice is shown) and the status bar across the whole width.

use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AppLayout {
    pub explorer: Rect,
    pub tabs: Rect,
    /// The find box (fields and result list), between the tab bar and the editor.
    pub find_bar: Rect,
    pub editor: Rect,
    /// The terminal panel (M5) under the editor: its tab bar row, then the terminal grid.
    /// Empty while the panel is hidden.
    pub terminal: Rect,
    pub notice: Rect,
    pub status: Rect,
}

/// What the side column shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    None,
    Explorer,
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

/// `notice_rows` is capped so the editor keeps at least a third of the screen; the side
/// column ([`explorer_width`]) is taken from the left of the tab bar and editor.
pub fn compute(area: Rect, notice_rows: u16, side: Side) -> AppLayout {
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
    let sw = match side {
        Side::None => 0,
        Side::Explorer => explorer_width(area.width).min(area.width / 2),
    };
    let explorer = Rect::new(area.x, area.y, sw, notice.y - area.y);
    let right = |y: u16, h: u16| Rect::new(area.x + sw, y, area.width - sw, h);
    let tabs = right(area.y, 1);
    let body = notice.y - (area.y + 1);
    let find_bar = right(area.y + 1, 0);
    let editor = right(area.y + 1, body);
    AppLayout {
        explorer,
        tabs,
        find_bar,
        editor,
        terminal: Rect::default(),
        notice,
        status,
    }
}

/// Takes the find box's field rows from the top of the editor, before the terminal panel
/// takes its rows (D275: the panel shrinks rather than the fields, as with the old find
/// bar). The fields never take the last row of the editor column.
pub fn place_find_box(layout: &mut AppLayout, rows: u16) {
    let ed = layout.editor;
    let rows = rows.min(ed.height.saturating_sub(1));
    layout.find_bar = Rect::new(ed.x, ed.y, ed.width, rows);
    layout.editor.y += rows;
    layout.editor.height -= rows;
}

/// After the terminal panel took its rows: the box grows by its list's `rows` from the top
/// of the editor, which keeps at least one row.
pub fn grow_find_box(layout: &mut AppLayout, rows: u16) {
    let rows = rows.min(layout.editor.height.saturating_sub(1));
    layout.find_bar.height += rows;
    layout.editor.y += rows;
    layout.editor.height -= rows;
}

/// Fewest rows the terminal panel gets (tab bar included) when the screen has them.
pub const TERMINAL_MIN_ROWS: u16 = 5;

/// Editor rows kept above a tall panel when the screen has room for both.
pub const EDITOR_MIN_ROWS: u16 = 3;

/// Takes the terminal panel from the bottom of the editor: `percent` of the screen height, at
/// least [`TERMINAL_MIN_ROWS`], at most the editor minus [`EDITOR_MIN_ROWS`] (a tiny screen shows only the panel).
/// The panel sits beside the side column, like VS Code's panel under its editor groups.
pub fn place_terminal(layout: &mut AppLayout, screen_height: u16, percent: u16) {
    let ed = layout.editor;
    let want = (u32::from(screen_height) * u32::from(percent) + 50) / 100;
    let mut rows = (want as u16).max(TERMINAL_MIN_ROWS).min(ed.height);
    if ed.height >= TERMINAL_MIN_ROWS + EDITOR_MIN_ROWS {
        rows = rows.min(ed.height - EDITOR_MIN_ROWS);
    }
    layout.terminal = Rect::new(ed.x, ed.y + ed.height - rows, ed.width, rows);
    layout.editor.height -= rows;
}
