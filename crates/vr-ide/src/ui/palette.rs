//! Palette overlay: input row and the matching commands with their keys (AC-6), or the
//! matching files for go to file.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Span;
use ratatui::widgets::{Block, Clear, Widget};

use crate::app::App;
use crate::commands::completion;
use crate::commands::palette::{PaletteItem, PaletteMode};
use crate::theme::Theme;

/// Widest the palette gets.
const MAX_WIDTH: u16 = 72;
/// Most rows the list shows.
const MAX_ROWS: u16 = 12;
/// Most rows a slash command's error takes.
const MAX_ERROR_ROWS: u16 = 3;
/// Cells of a slash command's description kept beside a title too wide for the row.
const SLASH_DESC_MIN: usize = 10;
/// Most rows the selected row's description takes under the list.
const MAX_DETAIL_ROWS: usize = 3;

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
    // The selected row's full description under the list, behind a thin separator, when
    // its row cannot show it all (AC-P1). It never costs the input or the selected row.
    let text_w = w.saturating_sub(4);
    // The rows are reserved while any listed row is cut, so arrowing never resizes the
    // box (D323); the text is the selected row's, blank while that row fits.
    let row_w = usize::from(w.saturating_sub(4));
    // Lines the most demanding cut row needs (at most [`MAX_DETAIL_ROWS`]); 0 = none cut.
    let need = p
        .items
        .iter()
        .filter(|it| RowFit::of(it, row_w).is_cut(it, row_w))
        .map(|it| {
            crate::ui::dialog::wrap(&row_text(it), text_w)
                .len()
                .min(MAX_DETAIL_ROWS)
        })
        .max()
        .unwrap_or(0);
    let mut detail_lines: Vec<String> = match p.items.get(p.selected) {
        Some(it) if RowFit::of(it, row_w).is_cut(it, row_w) => {
            wrap_cut(&row_text(it), text_w, MAX_DETAIL_ROWS)
        }
        _ => Vec::new(),
    };
    // Tab completion (D6) owns the area under the list while it is open.
    let comp = p.completion.clone();
    let need = match &comp {
        Some(c) => {
            detail_lines = completion_detail(c, p.input.text(), text_w);
            detail_lines.len()
        }
        None => need,
    };
    let spare = room.saturating_sub(3 + err_rows + 1);
    let reserved = need.min(usize::from(spare.saturating_sub(1)));
    detail_lines.truncate(reserved);
    let detail_rows = if reserved == 0 {
        0
    } else {
        reserved as u16 + 1
    };
    let list_rows = (p.items.len().max(1) as u16)
        .min(MAX_ROWS)
        .min(room.saturating_sub(3 + err_rows + detail_rows).max(1));
    let h = (list_rows + 3 + err_rows + detail_rows).min(room);
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
    // Right of the input while completing: `tab: list (N)` or `i/N`. The typed text and the
    // cursor after it keep their room: a narrow box takes the short tag or none; the list
    // counter always stays.
    let typed_w = Span::raw(p.input.text()).width() as u16 + 1;
    let avail = inner_w.saturating_sub(pw);
    let fits = |t: &str| {
        let w = Span::raw(t).width() as u16;
        !t.is_empty() && w + 1 + typed_w <= avail
    };
    let tag = match &comp {
        None => String::new(),
        Some(c) => {
            let full = c.tag();
            if fits(&full) {
                full
            } else if let Some(s) = c.short_tag().filter(|s| fits(s)) {
                s
            } else if matches!(c.phase, completion::Phase::List(_)) {
                full
            } else {
                String::new()
            }
        }
    };
    let tag_w = Span::raw(tag.as_str()).width() as u16;
    let tag_room = if tag.is_empty() || tag_w + 2 > avail {
        0
    } else {
        tag_w + 1
    };
    let field_w = inner_w.saturating_sub(pw + tag_room);
    if tag_room > 0 {
        let tx = x0 + inner_w - tag_w - 1;
        buf.set_string(tx, rect.y + 1, &tag, theme.input.patch(theme.dim));
    }
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
    // Candidate folders in the folder colour (S5).
    let folders = comp
        .as_ref()
        .is_some_and(|c| matches!(c.phase, completion::Phase::List(_)));
    let folder = ratatui::style::Style::default().fg(theme.popup_border.fg.unwrap_or_default());
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
        // Title first, then the keys (AC-6), then the command name when there is room.
        let RowFit {
            keys_w,
            name_w,
            show_keys,
            show_name,
            ..
        } = RowFit::of(item, width);
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
            } else if folders && item.title.ends_with('/') {
                base.patch(folder)
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
    if detail_rows > 0 {
        let dim = theme.popup.patch(theme.dim);
        let sy = list.y + list_rows;
        buf.set_stringn(x0 + 1, sy, "─".repeat(width), width, dim);
        for (i, l) in detail_lines.iter().enumerate() {
            buf.set_stringn(x0 + 1, sy + 1 + i as u16, l, width, dim);
        }
    }
    frame.set_cursor_position(cursor);
}

/// How a row's parts share the row's width; the render and the cut check read the same.
struct RowFit {
    title_w: usize,
    keys_w: usize,
    name_w: usize,
    show_keys: bool,
    show_name: bool,
}

impl RowFit {
    fn of(item: &PaletteItem, width: usize) -> Self {
        let title_w = Span::raw(item.title.as_str()).width();
        let keys_w = Span::raw(item.keys.as_str()).width();
        let name_w = Span::raw(item.detail.as_str()).width();
        let show_keys = !item.keys.is_empty() && title_w + 2 + keys_w <= width;
        let used = title_w + if show_keys { keys_w + 2 } else { 0 };
        Self {
            title_w,
            keys_w,
            name_w,
            show_keys,
            show_name: used + 2 + name_w <= width,
        }
    }

    /// Whether any part of the row is cut. A command's id is secondary, not a description.
    fn is_cut(&self, item: &PaletteItem, width: usize) -> bool {
        let has_detail = !item.detail.is_empty() && item.name.is_empty();
        self.title_w > width
            || (!item.keys.is_empty() && !self.show_keys)
            || (has_detail && !self.show_name)
    }
}

/// The whole text of a row: title, description (not a command id) and keys.
fn row_text(item: &PaletteItem) -> String {
    let detail = if item.name.is_empty() {
        item.detail.as_str()
    } else {
        ""
    };
    [item.title.as_str(), detail, item.keys.as_str()]
        .iter()
        .filter(|s| !s.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join("  ")
}

/// `text` wrapped on word boundaries to `width` cells, at most `rows` lines; a longer text
/// ends its last line in "…".
fn wrap_cut(text: &str, width: u16, rows: usize) -> Vec<String> {
    let mut lines = crate::ui::dialog::wrap(text, width);
    if lines.len() > rows {
        lines.truncate(rows);
        if let Some(last) = lines.last_mut() {
            let mut cut = String::new();
            let mut used = 0;
            for ch in last.chars() {
                let cw = Span::raw(ch.to_string()).width();
                if used + cw + 1 > usize::from(width) {
                    break;
                }
                cut.push(ch);
                used += cw;
            }
            cut.push('…');
            *last = cut;
        }
    }
    lines
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

/// The lines under the list while a completion is open: the common part's candidates on one
/// line, the `/ssh` command the input runs (wrapped at spaces and after `/`), the folder paths
/// come from, or the key hint.
fn completion_detail(c: &completion::Completion, input: &str, width: u16) -> Vec<String> {
    match (c.phase, &c.note) {
        (completion::Phase::Common, _) => vec![ellipsize(&c.detail(), usize::from(width))],
        (completion::Phase::List(_) | completion::Phase::Done, completion::Note::SshRuns(base)) => {
            let arg = input.strip_prefix("/ssh").unwrap_or(input).trim_start();
            wrap_runs(
                &completion::ssh_runs_line(arg, base),
                usize::from(width),
                MAX_DETAIL_ROWS,
            )
        }
        (completion::Phase::Done, _) => Vec::new(),
        _ => wrap_cut(&c.detail(), width, MAX_DETAIL_ROWS),
    }
}

/// `text` (`runs: ssh …`) in lines of `width` cells, broken only after a space or a `/`
/// (never inside a name); continuation lines are indented under the command. More than
/// `rows` lines: the last one ends in "…".
fn wrap_runs(text: &str, width: usize, rows: usize) -> Vec<String> {
    let indent = " ".repeat("runs: ".len());
    let mut pieces: Vec<String> = Vec::new();
    let mut cur = String::new();
    for ch in text.chars() {
        cur.push(ch);
        if ch == ' ' || ch == '/' {
            pieces.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        pieces.push(cur);
    }
    let cells = |s: &str| Span::raw(s).width();
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for piece in pieces {
        if !line.trim().is_empty() && cells(&line) + cells(piece.trim_end()) > width {
            lines.push(line.trim_end().to_string());
            line = indent.clone();
        }
        line.push_str(&piece);
    }
    if !line.trim().is_empty() {
        lines.push(line.trim_end().to_string());
    }
    if lines.len() > rows {
        lines.truncate(rows);
        if let Some(last) = lines.last_mut() {
            *last = ellipsize(&format!("{last} …"), width);
            if !last.ends_with('…') {
                last.push('…');
            }
        }
    }
    lines
}
