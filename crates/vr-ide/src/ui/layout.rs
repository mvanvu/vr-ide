//! Screen split: a side panel on the left (the explorer or the search panel, when shown),
//! tab bar, find bar (while open) and editor on the right, notice lines (only while a notice
//! is shown) and the status bar across the whole width.

use ratatui::layout::Rect;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AppLayout {
    pub explorer: Rect,
    /// The project search panel; it takes the side column instead of the explorer.
    pub search: Rect,
    pub tabs: Rect,
    /// The find / replace bar, between the tab bar and the editor.
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
    Search,
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

/// Below this screen width the search panel takes the whole width (results need room).
pub const SEARCH_MIN_SCREEN: u16 = 40;

/// Search panel width: wider than the explorer because result lines need room.
pub fn search_width(width: u16) -> u16 {
    if width < SEARCH_MIN_SCREEN {
        return width;
    }
    (width * 2 / 5).clamp(24, 60).min(width * 3 / 5)
}

/// `notice_rows` is capped so the editor keeps at least a third of the screen; the side
/// column ([`explorer_width`] / [`search_width`]) is taken from the left of the tab bar and
/// editor; `find_rows` (0 when the bar is closed) from the top of the editor.
pub fn compute(area: Rect, notice_rows: u16, side: Side, find_rows: u16) -> AppLayout {
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
        Side::Search => search_width(area.width),
    };
    let column = Rect::new(area.x, area.y, sw, notice.y - area.y);
    let (explorer, search) = match side {
        Side::Search => (Rect::default(), column),
        _ => (column, Rect::default()),
    };
    let right = |y: u16, h: u16| Rect::new(area.x + sw, y, area.width - sw, h);
    let tabs = right(area.y, 1);
    let body = notice.y - (area.y + 1);
    // The bar never takes the last editor row.
    let find_rows = find_rows.min(body.saturating_sub(1));
    let find_bar = right(area.y + 1, find_rows);
    let editor = right(area.y + 1 + find_rows, body - find_rows);
    AppLayout {
        explorer,
        search,
        tabs,
        find_bar,
        editor,
        terminal: Rect::default(),
        notice,
        status,
    }
}

/// Fewest rows the terminal panel gets (tab bar included) when the screen has them.
pub const TERMINAL_MIN_ROWS: u16 = 5;

/// Takes the terminal panel from the bottom of the editor: `percent` of the screen height, at
/// least [`TERMINAL_MIN_ROWS`], at most the whole editor (a tiny screen shows only the panel).
/// The panel sits beside the side column, like VS Code's panel under its editor groups.
pub fn place_terminal(layout: &mut AppLayout, screen_height: u16, percent: u16) {
    let ed = layout.editor;
    let want = (u32::from(screen_height) * u32::from(percent) + 50) / 100;
    let rows = (want as u16).max(TERMINAL_MIN_ROWS).min(ed.height);
    layout.terminal = Rect::new(ed.x, ed.y + ed.height - rows, ed.width, rows);
    layout.editor.height -= rows;
}
