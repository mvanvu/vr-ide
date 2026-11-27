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
    /// The header row of the AI diff tab (kind, file, counts, keys), over the editor; empty
    /// for every other tab.
    pub diff_head: Rect,
    pub editor: Rect,
    /// The terminal panel (M5) under the editor, or right of it (T19): its tab bar row, then
    /// the terminal grid. Empty while the panel is hidden.
    pub terminal: Rect,
    /// The one-column separator left of a panel on the right (its drag handle); empty while
    /// the panel is at the bottom or hidden.
    pub terminal_sep: Rect,
    /// The AI panel (D7): a column right of the editor from the top row to the notices, its tab
    /// bar row then the session. Empty while hidden.
    pub ai: Rect,
    /// The one-column separator left of the AI panel (its drag handle); empty while hidden or
    /// while the panel takes the whole area right of the explorer.
    pub ai_sep: Rect,
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
        diff_head: Rect::default(),
        editor,
        terminal: Rect::default(),
        terminal_sep: Rect::default(),
        ai: Rect::default(),
        ai_sep: Rect::default(),
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

/// Takes the diff tab's header row from the top of the editor (after the find box and the
/// terminal took theirs); an editor of one row keeps it.
pub fn place_diff_head(layout: &mut AppLayout) {
    let ed = layout.editor;
    if ed.height < 2 || ed.width == 0 {
        return;
    }
    layout.diff_head = Rect::new(ed.x, ed.y, ed.width, 1);
    layout.editor.y += 1;
    layout.editor.height -= 1;
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

/// Narrowest editor column (the screen right of the explorer) that gets the panel on the
/// right; a narrower one shows it at the bottom (D227).
pub const TERMINAL_RIGHT_MIN_COLUMNS: u16 = 60;

/// Fewest columns of a panel on the right.
pub const TERMINAL_MIN_COLS: u16 = 20;

/// Editor columns kept left of a wide panel on the right.
pub const EDITOR_MIN_COLS: u16 = 20;

/// An editor column `width` cells wide has room for the panel on the right.
pub fn right_fits(width: u16) -> bool {
    width >= TERMINAL_RIGHT_MIN_COLUMNS
}

/// Takes the terminal panel and its separator from the right of the editor, the editor's full
/// height: `percent` of the editor column's width, at least [`TERMINAL_MIN_COLS`], leaving the
/// editor at least [`EDITOR_MIN_COLS`]. `exact` (a drag) replaces the percent with a column
/// count. Call it only when [`right_fits`] the editor width.
pub fn place_terminal_right(layout: &mut AppLayout, percent: u16, exact: Option<u16>) {
    let ed = layout.editor;
    let cols = match exact {
        Some(c) => c
            .max(TERMINAL_MIN_COLS)
            .min(ed.width.saturating_sub(EDITOR_MIN_COLS + 1)),
        None => right_cols(ed.width, percent),
    };
    layout.terminal = Rect::new(ed.x + ed.width - cols, ed.y, cols, ed.height);
    layout.terminal_sep = Rect::new(layout.terminal.x - 1, ed.y, 1, ed.height);
    layout.editor.width -= cols + 1;
}

/// Panel columns for an editor column `width` wide (at least [`TERMINAL_RIGHT_MIN_COLUMNS`]).
pub fn right_cols(width: u16, percent: u16) -> u16 {
    let want = (u32::from(width) * u32::from(percent) + 50) / 100;
    let max = width.saturating_sub(EDITOR_MIN_COLS + 1);
    (want as u16).max(TERMINAL_MIN_COLS).min(max)
}

/// Narrowest area right of the explorer that keeps the editor beside a shown AI panel; a
/// narrower one gives the panel all of it (ui-spec S1).
pub const AI_SIDE_MIN_COLUMNS: u16 = 60;

/// Fewest columns of the AI panel, its separator included.
pub const AI_MIN_COLS: u16 = 30;

/// Editor columns kept left of the AI panel.
pub const AI_EDITOR_MIN_COLS: u16 = 30;

/// Takes the AI panel and its separator from the right of the tab bar and editor, from the top
/// row to the notices: `percent` of the area right of the explorer (separator included), at
/// least [`AI_MIN_COLS`], leaving the editor [`AI_EDITOR_MIN_COLS`]; `exact` (a drag) replaces
/// the percent with a column count. Under [`AI_SIDE_MIN_COLUMNS`] the panel takes the whole
/// area and the editor gets none. Call it before the find box and the terminal are placed.
pub fn place_ai(layout: &mut AppLayout, percent: u16, exact: Option<u16>) {
    let width = layout.tabs.width;
    let top = layout.tabs.y;
    let height = (layout.editor.y + layout.editor.height).saturating_sub(top);
    if width == 0 || height == 0 {
        return;
    }
    if width < AI_SIDE_MIN_COLUMNS {
        layout.ai = Rect::new(layout.tabs.x, top, width, height);
        layout.tabs.width = 0;
        layout.editor.width = 0;
        layout.find_bar.width = 0;
        return;
    }
    let total = exact
        .unwrap_or_else(|| ((u32::from(width) * u32::from(percent) + 50) / 100) as u16)
        .clamp(AI_MIN_COLS, width - AI_EDITOR_MIN_COLS);
    let sep_x = layout.tabs.x + width - total;
    layout.ai_sep = Rect::new(sep_x, top, 1, height);
    layout.ai = Rect::new(sep_x + 1, top, total - 1, height);
    layout.tabs.width -= total;
    layout.editor.width -= total;
    layout.find_bar.width = layout.find_bar.width.min(layout.editor.width);
}

#[cfg(test)]
mod ai_tests {
    use super::*;

    fn at(w: u16, h: u16, side: Side, percent: u16) -> AppLayout {
        let mut l = compute(Rect::new(0, 0, w, h), 0, side);
        place_ai(&mut l, percent, None);
        l
    }

    /// The mockups' widths: 120x40 with the explorer (editor 54, panel 35 + separator),
    /// 80x24 without (editor 48, panel 31 + separator), 80x24 with (editor 30, panel 30),
    /// 40x12 with the explorer (the panel takes the rest, no separator).
    #[test]
    fn ai_panel_widths_follow_the_spec() {
        let l = at(120, 40, Side::Explorer, 40);
        assert_eq!((l.editor.width, l.ai_sep.x, l.ai.width), (54, 84, 35));
        assert_eq!((l.ai.y, l.ai.height), (0, 39));
        assert_eq!(l.tabs.width, 54);
        let l = at(80, 24, Side::None, 40);
        assert_eq!((l.editor.width, l.ai.width), (48, 31));
        let l = at(80, 24, Side::Explorer, 40);
        assert_eq!(
            (l.explorer.width, l.editor.width, l.ai.width + 1),
            (20, 30, 30)
        );
        let l = at(80, 24, Side::Explorer, 90);
        assert_eq!(l.editor.width, AI_EDITOR_MIN_COLS);
        let l = at(40, 12, Side::Explorer, 40);
        assert_eq!(
            (l.editor.width, l.ai_sep.width, l.ai.x, l.ai.width),
            (0, 0, 16, 24)
        );
        let mut l = compute(Rect::new(0, 0, 120, 40), 0, Side::Explorer);
        place_ai(&mut l, 40, Some(50));
        assert_eq!(l.ai.width, 49);
    }
}
