//! The terminal panel (M5): a tab bar row, then the active terminal's grid in the theme's ANSI
//! palette, with the selection, a scrollback indicator, the exited banner and the program's
//! cursor while the panel has the keys. Also the status bar while the terminal is focused.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use vr_term::{Cell, Color as TermColor, TermStatus};

use crate::app::App;
use crate::commands::terminal::{self as term, TerminalPanel};
use crate::theme::{AnsiPalette, Theme};

pub fn render(frame: &mut Frame, area: Rect, app: &mut App, theme: &Theme) {
    if area.height == 0 || area.width == 0 || !app.terminal.visible {
        return;
    }
    let grid_area = Rect::new(area.x, area.y + 1, area.width, area.height - 1);
    term::fit(app, grid_area);
    term::snapshot_active(app);
    let focused = term::has_keys(app);
    let hint = if focused {
        format!(
            "{} editor · {} hide ",
            first_key(app, "focus.editor"),
            first_key(app, "terminal.toggle")
        )
    } else {
        format!("{} focus ", first_key(app, "terminal.toggle"))
    };
    let buf = frame.buffer_mut();
    render_tab_bar(buf, Rect { height: 1, ..area }, &app.terminal, &hint, theme);
    if grid_area.height == 0 {
        return;
    }
    let close = first_key(app, "file.close");
    if let Some(pos) = render_grid(buf, grid_area, &app.terminal, &close, theme)
        && focused
    {
        frame.set_cursor_position(pos);
    }
}

/// The first key bound to `cmd` (hints never hard-code keys, USER-1).
fn first_key(app: &App, cmd: &str) -> String {
    app.keymap
        .keys_for(cmd)
        .first()
        .map_or_else(|| format!("run {cmd}"), ToString::to_string)
}

fn width_of(s: &str) -> usize {
    Span::raw(s).width()
}

fn render_tab_bar(buf: &mut Buffer, row: Rect, panel: &TerminalPanel, hint: &str, theme: &Theme) {
    buf.set_style(row, theme.tab_bar);
    let right = row.x + row.width;
    let mut x = row.x;
    for (i, label) in panel.labels().iter().enumerate() {
        if x >= right {
            break;
        }
        let style = if i == panel.active {
            theme.tab_active
        } else {
            theme.tab_inactive
        };
        let exited = panel
            .tabs
            .get(i)
            .is_some_and(|t| matches!(t.status(), TermStatus::Exited(c) if c != 0));
        let style = if exited {
            style.patch(theme.tab_warning)
        } else {
            style
        };
        let (nx, _) = buf.set_stringn(x, row.y, label, usize::from(right - x), style);
        x = nx;
    }
    let hw = width_of(hint) as u16;
    if x + 1 + hw <= right {
        buf.set_stringn(right - hw, row.y, hint, usize::from(hw), theme.dim);
    }
}

/// Draws the active terminal; returns where its cursor is when it should be shown.
fn render_grid(
    buf: &mut Buffer,
    area: Rect,
    panel: &TerminalPanel,
    close_key: &str,
    theme: &Theme,
) -> Option<(u16, u16)> {
    let palette = theme.ansi_palette();
    let blank = Cell {
        ch: " ".into(),
        ..Cell::default()
    };
    buf.set_style(area, cell_style(&blank, palette));
    let tab = panel.active_tab()?;
    let g = &tab.grid;
    let (w, h) = (usize::from(area.width), usize::from(area.height));
    for r in 0..g.rows.min(h) {
        let y = area.y + r as u16;
        for c in 0..g.cols.min(w) {
            let Some(cell) = g.cell(r, c) else {
                continue;
            };
            let x = area.x + c as u16;
            let mut style = cell_style(cell, palette);
            if panel.selection.is_some_and(|s| s.contains(r, c)) {
                style = style.patch(theme.selection);
            }
            // The spacer of a wide character is covered by it; a wide character that would
            // stick out of the panel is drawn as a blank.
            let blank = cell.flags.wide_spacer || cell.ch.is_empty();
            let symbol = if blank || (cell.flags.wide && c + 1 >= w) {
                " "
            } else {
                cell.ch.as_str()
            };
            buf[(x, y)].set_symbol(symbol).set_style(style);
        }
    }
    let status = tab.status();
    if tab.scroll > 0 {
        let text = format!(" ↑ {}/{} ", tab.scroll, g.scrollback_len);
        let tw = width_of(&text).min(w) as u16;
        let style = theme.popup.patch(theme.info);
        buf.set_stringn(area.x + area.width - tw, area.y, &text, w, style);
    }
    if let TermStatus::Exited(code) = status {
        let y = area.y + area.height - 1;
        let row = Rect::new(area.x, y, area.width, 1);
        buf.set_style(row, theme.popup);
        let text = format!(" [process exited {code}] — Enter to restart, {close_key} to close");
        buf.set_stringn(area.x, y, text, w, theme.popup);
        return None;
    }
    let cur = g.cursor;
    (tab.scroll == 0 && cur.visible && cur.row < h && cur.col < w)
        .then(|| (area.x + cur.col as u16, area.y + cur.row as u16))
}

/// Status bar while the terminal has the focus: the TERM badge (read / edit mode do not apply
/// inside a terminal), its title, and which terminal of how many at what size.
pub fn render_status(buf: &mut Buffer, area: Rect, app: &App, theme: &Theme) {
    let width = usize::from(area.width);
    let p = &app.terminal;
    let Some(tab) = p.active_tab() else {
        return;
    };
    let badge = if width >= 20 { " TERM " } else { " T " };
    buf.set_stringn(area.x, area.y, badge, width, theme.mode_term);
    let bw = width_of(badge).min(width);
    let (cols, rows) = tab.size();
    let state = match tab.status() {
        TermStatus::Running => String::new(),
        TermStatus::Exited(code) => format!(" [exited {code}]"),
    };
    let right = format!(" {}/{} · {cols}×{rows} ", p.active + 1, p.tabs.len());
    let left = format!(" {}{state}", tab.title());
    let room = width - bw;
    let rw = width_of(&right);
    let x = area.x + bw as u16;
    if width_of(&left) + rw <= room {
        buf.set_stringn(x, area.y, &left, room, theme.status);
        buf.set_stringn(
            area.x + (width - rw) as u16,
            area.y,
            &right,
            rw,
            theme.status,
        );
    } else {
        buf.set_stringn(x, area.y, &left, room, theme.status);
    }
}

/// Style of one terminal cell: ANSI 0–15 and the default colours through the theme's palette
/// (the terminal's own colours without one), 256-colour and true-colour values as they are.
pub fn cell_style(cell: &Cell, palette: Option<&AnsiPalette>) -> Style {
    let resolve = |c: TermColor, fg: bool| match (c, palette) {
        (TermColor::Default, Some(p)) => {
            if fg {
                p.fg
            } else {
                p.bg
            }
        }
        (TermColor::Default, None) => Color::Reset,
        (TermColor::Indexed(i), Some(p)) if i < 16 => p.colors[usize::from(i)],
        (TermColor::Indexed(i), _) => Color::Indexed(i),
        (TermColor::Rgb(r, g, b), _) => Color::Rgb(r, g, b),
    };
    let (mut fg, mut bg) = (resolve(cell.fg, true), resolve(cell.bg, false));
    let f = cell.flags;
    let mut m = Modifier::empty();
    if f.inverse {
        // The terminal's own default colours are unknown here; it swaps them itself.
        if fg == Color::Reset || bg == Color::Reset {
            m |= Modifier::REVERSED;
        } else {
            std::mem::swap(&mut fg, &mut bg);
        }
    }
    if f.hidden {
        fg = bg;
    }
    for (on, flag) in [
        (f.bold, Modifier::BOLD),
        (f.italic, Modifier::ITALIC),
        (f.underline, Modifier::UNDERLINED),
        (f.dim, Modifier::DIM),
        (f.strikeout, Modifier::CROSSED_OUT),
    ] {
        if on {
            m |= flag;
        }
    }
    Style::default().fg(fg).bg(bg).add_modifier(m)
}
