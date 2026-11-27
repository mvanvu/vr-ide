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
use crate::theme::{AnsiPalette, ColorDepth, Theme};

pub fn render(frame: &mut Frame, area: Rect, app: &mut App, theme: &Theme) {
    if area.height == 0 || area.width == 0 || !app.terminal.visible {
        return;
    }
    let grid_area = Rect::new(area.x, area.y + 1, area.width, area.height - 1);
    term::fit(app, grid_area);
    term::snapshot_active(app);
    let focused = term::has_keys(app);
    let bar = tab_bar(app, area.width);
    let buf = frame.buffer_mut();
    render_tab_bar(buf, Rect { height: 1, ..area }, &app.terminal, &bar, theme);
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

/// The column left of a panel on the right (T19), drawn like the explorer's edge: in the focus
/// colour while the terminal has the keys. Dragging it sets the panel's width.
pub fn render_separator(buf: &mut Buffer, area: Rect, app: &App, theme: &Theme) {
    if area.width == 0 || !app.terminal.visible {
        return;
    }
    let style = if term::has_keys(app) {
        theme.focus
    } else {
        theme.dim
    };
    for y in area.y..area.y + area.height {
        buf.set_string(area.x, y, "│", style);
    }
}

/// The key shown for `cmd` (hints never hard-code keys, USER-1; D261).
fn first_key(app: &App, cmd: &str) -> String {
    app.key_hint(cmd)
}

fn width_of(s: &str) -> usize {
    Span::raw(s).width()
}

/// How a focused terminal gives the keys back (D280): ctrl+` hides the panel; it reaches vr-ide
/// from a focused terminal always with the kitty protocol and, in a legacy terminal, while NUL is
/// not passed to the program (D214); otherwise only a click on the editor does.
fn leave_text(app: &App) -> String {
    if term::hide_key_works(app) {
        format!("{} hide", first_key(app, "terminal.toggle"))
    } else {
        "click editor".to_string()
    }
}

/// What the panel's key hint says, in the state the panel is in: ctrl+` hides a shown panel
/// wherever the keys are (D280).
fn hint(app: &App) -> String {
    if !term::has_keys(app) {
        format!("{} hide ", first_key(app, "terminal.toggle"))
    } else {
        format!("{} ", leave_text(app))
    }
}

/// One drawn tab of the bar: which terminal, its label and where it sits (offset from the
/// bar's left edge, in cells); `close` is the offset of its 'x' inside the label, `None` when
/// the label is cut before it.
pub struct BarTab {
    pub index: usize,
    pub label: String,
    pub x: u16,
    pub width: u16,
    pub close: Option<u16>,
}

/// The tab bar laid out for a width: the tabs drawn, a `+N` marker for those left out, the '+'
/// (new terminal, D280) and the hint when it fits. Rendering and the mouse hit test both use
/// it.
pub struct TabBar {
    pub tabs: Vec<BarTab>,
    pub more: Option<String>,
    /// Offset of the '+' button from the bar's left edge, [`PLUS_WIDTH`] cells wide.
    pub plus: Option<u16>,
    pub hint: Option<String>,
}

/// The '+' button after the tabs.
pub const PLUS: &str = " + ";
pub const PLUS_WIDTH: u16 = 3;
/// The end of each tab label: its close button and a space.
const CLOSE: &str = "x ";

/// A label never gets narrower than this while another layout is possible (" 3 …/alpha x ").
const MIN_LABEL: usize = 14;
/// A title alone never takes more than this, so one long title does not fill the bar.
const TITLE_MAX: usize = 32;

/// " 1 bash x ", " 2 htop [0] x " (exited with code 0; 'x' closes it); the title is cut keeping
/// its end (the directory of `user@host: dir`) so the label is at most `cap` cells wide.
fn tab_label(n: usize, title: &str, exit: &str, cap: usize) -> String {
    let fixed = width_of(&format!(" {n} {exit} {CLOSE}"));
    let room = cap.saturating_sub(fixed).min(TITLE_MAX);
    let title = crate::ui::status_bar::shorten_left(title, room);
    format!(" {n} {title}{exit} {CLOSE}")
}

/// Labels for tabs `range` within `avail` cells, all cut to one common cap (the widest that
/// fits); `None` when that cap would be under [`MIN_LABEL`].
fn fit_labels(
    items: &[(String, String)],
    range: std::ops::Range<usize>,
    avail: usize,
) -> Option<Vec<String>> {
    let natural = |i: usize, cap: usize| {
        let (title, exit) = &items[i];
        tab_label(i + 1, title, exit, cap)
    };
    let widest = range
        .clone()
        .map(|i| width_of(&natural(i, usize::MAX)))
        .max()
        .unwrap_or(0);
    let mut cap = widest;
    loop {
        let labels: Vec<String> = range.clone().map(|i| natural(i, cap)).collect();
        if labels.iter().map(|l| width_of(l)).sum::<usize>() <= avail {
            return Some(labels);
        }
        if cap <= MIN_LABEL {
            return None;
        }
        cap -= 1;
    }
}

pub fn tab_bar(app: &App, width: u16) -> TabBar {
    let panel = &app.terminal;
    let items: Vec<(String, String)> = panel
        .tabs
        .iter()
        .map(|t| {
            let exit = match t.status() {
                TermStatus::Exited(code) => format!(" [{code}]"),
                TermStatus::Running => String::new(),
            };
            (t.title(), exit)
        })
        .collect();
    let n = items.len();
    let width = usize::from(width);
    let active = panel.active.min(n.saturating_sub(1));
    let hint = hint(app);
    let plus_w = usize::from(PLUS_WIDTH);
    let build = |range: std::ops::Range<usize>,
                 labels: Vec<String>,
                 more: Option<String>,
                 hint: Option<String>| {
        let mut x = 0u16;
        let tabs: Vec<BarTab> = range
            .zip(labels)
            .map(|(index, label)| {
                let w = width_of(&label) as u16;
                // The 'x' is clickable only when the whole label is drawn.
                let close = (label.ends_with(CLOSE) && usize::from(x + w) <= width)
                    .then(|| w - width_of(CLOSE) as u16);
                let t = BarTab {
                    index,
                    label,
                    x,
                    width: w,
                    close,
                };
                x += w;
                t
            })
            .collect();
        let end = usize::from(x) + more.as_deref().map_or(0, width_of);
        let plus = (end + plus_w <= width).then_some(end as u16);
        TabBar {
            tabs,
            more,
            plus,
            hint,
        }
    };
    // The '+' always keeps its cells. Detail gives way in this order: long titles (cut keeping
    // the end), the hint, then tabs other than the active one (behind a `+N` marker), then the
    // hint again.
    let avail_with = |hint: &Option<String>| {
        width.saturating_sub(plus_w + hint.as_ref().map_or(0, |h| width_of(h) + 1))
    };
    for hint in [Some(hint.clone()), None] {
        if let Some(labels) = fit_labels(&items, 0..n, avail_with(&hint)) {
            return build(0..n, labels, None, hint);
        }
    }
    for hint in [Some(hint), None] {
        let avail = avail_with(&hint);
        for k in (1..n).rev() {
            let more = format!(" +{} ", n - k);
            let Some(room) = avail.checked_sub(width_of(&more)) else {
                continue;
            };
            let start = (active + 1).saturating_sub(k).min(n - k);
            if let Some(labels) = fit_labels(&items, start..start + k, room) {
                return build(start..start + k, labels, Some(more), hint);
            }
        }
    }
    // Narrower than one short label: the active tab alone, cut by the bar's edge (the '+' too
    // when it does not fit after it).
    let labels = items
        .get(active)
        .map(|(title, exit)| {
            vec![tab_label(
                active + 1,
                title,
                exit,
                width.saturating_sub(plus_w),
            )]
        })
        .unwrap_or_default();
    build(active..active + labels.len(), labels, None, None)
}

fn render_tab_bar(buf: &mut Buffer, row: Rect, panel: &TerminalPanel, bar: &TabBar, theme: &Theme) {
    buf.set_style(row, theme.tab_bar);
    let right = row.x + row.width;
    let mut x = row.x;
    for t in &bar.tabs {
        x = row.x + t.x;
        if x >= right {
            break;
        }
        let style = if t.index == panel.active {
            theme.tab_active
        } else {
            theme.tab_inactive
        };
        let exited = panel
            .tabs
            .get(t.index)
            .is_some_and(|t| matches!(t.status(), TermStatus::Exited(c) if c != 0));
        let style = if exited {
            style.patch(theme.tab_warning)
        } else {
            style
        };
        let (nx, _) = buf.set_stringn(x, row.y, &t.label, usize::from(right - x), style);
        // The close button in the muted colour, apart from the title (" 1 tmux x ").
        if let (Some(c), Some(fg)) = (t.close, theme.dim.fg)
            && x + c < right
        {
            buf.set_style(Rect::new(x + c, row.y, 1, 1), Style::default().fg(fg));
        }
        x = nx;
    }
    if let Some(more) = &bar.more
        && x < right
    {
        let (nx, _) = buf.set_stringn(x, row.y, more, usize::from(right - x), theme.dim);
        x = nx;
    }
    if let Some(p) = bar.plus {
        let px = row.x + p;
        if px < right {
            let (nx, _) =
                buf.set_stringn(px, row.y, PLUS, usize::from(right - px), theme.tab_inactive);
            x = nx;
        }
    }
    if let Some(hint) = &bar.hint {
        let hw = width_of(hint) as u16;
        if x + 1 + hw <= right {
            buf.set_stringn(right - hw, row.y, hint, usize::from(hw), theme.dim);
        }
    }
}

/// Draws the active terminal; returns where its cursor is when it should be shown.
pub(crate) fn render_grid(
    buf: &mut Buffer,
    area: Rect,
    panel: &TerminalPanel,
    close_key: &str,
    theme: &Theme,
) -> Option<(u16, u16)> {
    let palette = theme.ansi_palette();
    let depth = ColorDepth::detect();
    let blank = Cell {
        ch: " ".into(),
        ..Cell::default()
    };
    buf.set_style(area, cell_style(&blank, palette, depth));
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
            let mut style = cell_style(cell, palette, depth);
            if panel.selection.is_some_and(|s| s.contains(r, c)) {
                style = style.patch(theme.selection);
            }
            // The spacer of a wide character is covered by it; a wide character that would
            // stick out of the panel is drawn as a blank.
            // Concealed text (SGR 8) is not drawn at all: with the terminal's own default
            // colours (dark / light) "fg = bg" cannot hide it.
            let blank = cell.flags.wide_spacer || cell.ch.is_empty() || cell.flags.hidden;
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
        // Both actions stay visible: the wording shortens before anything is cut.
        let forms = [
            format!(" [process exited {code}] — Enter to restart, {close_key} to close"),
            format!(" exited {code} · Enter restart · {close_key} close"),
            format!(" exited {code} · Enter · {close_key}"),
        ];
        let text = forms.iter().find(|t| width_of(t) <= w).unwrap_or(&forms[2]);
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
    let (i, n) = (p.active + 1, p.tabs.len());
    // The way out of a focused terminal outranks its size: the size gives way first.
    let out = leave_text(app);
    let rights = [
        (format!(" {i}/{n} · {cols}×{rows} · {out} "), 6),
        (format!(" {i}/{n} · {out} "), 2),
        (format!(" {i}/{n} · {cols}×{rows} "), 6),
        (format!(" {i}/{n} "), 0),
    ];
    let room = width - bw;
    let x = area.x + bw as u16;
    // Which terminal of how many always shows (the size first gives way); the title is cut
    // keeping its end, the directory of `user@host: dir`.
    let right = rights
        .iter()
        .find(|(r, title_min)| width_of(r) + title_min <= room)
        .or_else(|| rights.iter().find(|(r, _)| width_of(r) <= room))
        .map(|(r, _)| r);
    let rw = right.map_or(0, |r| width_of(r));
    let title = format!("{}{state}", tab.title());
    let left = format!(
        " {}",
        crate::ui::status_bar::shorten_left(&title, (room - rw).saturating_sub(1))
    );
    buf.set_stringn(x, area.y, &left, room - rw, theme.status);
    if let Some(right) = right {
        buf.set_stringn(
            area.x + (width - rw) as u16,
            area.y,
            right,
            rw,
            theme.status,
        );
    }
}

/// Style of one terminal cell: ANSI 0–15 and the default colours through the theme's palette
/// (the terminal's own colours without one), 256-colour values as they are, true-colour values
/// at the depth of vr-ide's own terminal.
pub fn cell_style(cell: &Cell, palette: Option<&AnsiPalette>, depth: ColorDepth) -> Style {
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
        (TermColor::Rgb(r, g, b), _) => {
            depth.color(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b))
        }
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
