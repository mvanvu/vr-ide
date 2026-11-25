//! Draws the Git popup's History and Blame tabs and the commit view (M7-4b) inside the popup's
//! border: a title row, the rows (only the visible ones are built), the status row and, when
//! there is room, a key hint. Every git-derived string is sanitized (SEC-15) by `git_history`
//! or here.

use std::sync::Arc;
use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use vr_git::LineKind;
use vr_syntax::{Capture, LanguageRegistry, Notify};

use crate::git_history::{self, CvRow, short_age, utc_date_time};
use crate::git_popup::{Areas, GitPopup, Tab};
use crate::theme::Theme;
use crate::ui::git_popup::{fit_head, fit_tail, put, skip_cells, width};
use crate::ui::safe_text::sanitize;

/// What the body needs besides the popup.
pub struct Ctx<'a> {
    pub theme: &'a Theme,
    /// Unix seconds for ages.
    pub now: i64,
    pub at: Instant,
    pub registry: &'a Arc<LanguageRegistry>,
    pub notify: &'a Notify,
}

/// The body of a non-Changes tab in `inner` (the popup without its border).
pub fn render(buf: &mut Buffer, inner: Rect, p: &mut GitPopup, cx: &Ctx, areas: &mut Areas) {
    let theme = cx.theme;
    let (x, w) = (inner.x, inner.width);
    let top = inner.y;
    let mut bottom = inner.y + inner.height;
    if inner.height >= 16 {
        bottom -= 1;
        put(
            buf,
            x,
            bottom,
            &fit_head(hint(p), usize::from(w)),
            w,
            theme.popup.patch(theme.dim),
        );
    }
    bottom -= 1;
    let status = git_history::status_line(p, cx.now, cx.at);
    let busy = p.blame.running && p.tab == Tab::Blame && p.commit.is_none();
    let failed = p.history.error.is_some() && p.tab == Tab::History && p.commit.is_none();
    let style = if busy {
        theme.popup.patch(theme.info)
    } else if failed {
        theme.popup.patch(theme.error)
    } else {
        theme.popup.patch(theme.dim)
    };
    put(buf, x, bottom, &fit_head(&status, usize::from(w)), w, style);
    if bottom <= top + 1 {
        return;
    }
    let rows = Rect::new(x, top + 1, w, bottom - top - 1);
    areas.history = rows;
    let title_style = theme.popup.patch(theme.focus);
    let title = if p.commit.is_some() {
        commit_view(buf, rows, p, cx)
    } else {
        match p.tab {
            Tab::History => history(buf, rows, p, theme),
            Tab::Blame => blame(buf, rows, p, cx),
            Tab::Changes => String::new(),
        }
    };
    put(
        buf,
        x,
        top,
        &fit_tail(&title, usize::from(w)),
        w,
        title_style,
    );
}

fn hint(p: &GitPopup) -> &'static str {
    if p.commit.is_some() {
        return "↑↓ pgup pgdn scroll · ←→ scroll sideways · esc back";
    }
    match p.tab {
        Tab::History => "↑↓ move · enter show commit · r reload · 1 2 3 tabs · esc close",
        _ => "↑↓ move · ←→ scroll · enter show commit · r blame again · 1 2 3 tabs · esc close",
    }
}

/// Keeps `sel` inside rows `top..top + h`.
fn follow(top: &mut usize, sel: usize, h: usize, n: usize) {
    if sel < *top {
        *top = sel;
    } else if sel >= *top + h {
        *top = sel + 1 - h;
    }
    *top = (*top).min(n.saturating_sub(h));
}

fn message(buf: &mut Buffer, r: Rect, text: &str, style: Style) {
    if r.height > 0 {
        put(
            buf,
            r.x,
            r.y,
            &fit_head(text, usize::from(r.width)),
            r.width,
            style,
        );
    }
}

// ---- History ----------------------------------------------------------------------------------

fn history(buf: &mut Buffer, r: Rect, p: &mut GitPopup, theme: &Theme) -> String {
    let h = &mut p.history;
    let what = match &h.path {
        Some(path) => {
            let rel = path.strip_prefix(&p.workdir).unwrap_or(path);
            format!("{} (following renames)", sanitize(&rel.to_string_lossy()))
        }
        None => p.repo_label.clone(),
    };
    let count = if h.commits.is_empty() {
        String::new()
    } else if h.complete {
        format!(" · {} commits", h.commits.len())
    } else {
        format!(" · {}+ commits", h.commits.len())
    };
    let title = format!("History · {what}{count}");
    // A failed first load shows the error here; a failed later page keeps the list and says
    // so on the status row.
    if let Some(e) = h.error.as_ref().filter(|_| h.commits.is_empty()) {
        message(buf, r, &format!("git: {e}"), theme.popup.patch(theme.error));
        return title;
    }
    if h.commits.is_empty() {
        let text = if h.loading || !h.loaded() {
            "loading commits…"
        } else {
            "no commits yet"
        };
        message(buf, r, text, theme.popup.patch(theme.dim));
        return title;
    }
    let rows = usize::from(r.height);
    follow(&mut h.top, h.selected, rows, h.commits.len());
    let w = usize::from(r.width);
    // Columns by width: id, date, author, subject; narrow screens keep id and subject (the
    // status row shows the selected commit's author and date).
    let (date, author) = match w {
        w if w >= 100 => (true, 18),
        w if w >= 70 => (true, 12),
        w if w >= 50 => (true, 0),
        _ => (false, 0),
    };
    for (i, c) in h.commits.iter().enumerate().skip(h.top).take(rows) {
        let y = r.y + (i - h.top) as u16;
        let selected = i == h.selected;
        let base = if selected {
            theme.list_selected
        } else {
            theme.popup
        };
        buf.set_style(Rect::new(r.x, y, r.width, 1), base);
        let mut cells: Vec<(String, Style)> = vec![(
            format!("{} ", sanitize(&c.short)),
            if selected {
                base
            } else {
                base.patch(theme.git_modified)
            },
        )];
        if date {
            let d = utc_date_time(c.author.time);
            cells.push((
                format!("{} ", &d[..10]),
                if selected {
                    base
                } else {
                    base.patch(theme.dim)
                },
            ));
        }
        if author > 0 {
            let a = fit_head(&sanitize(&c.author.name), author);
            cells.push((format!("{a:<author$} "), base));
        }
        cells.push((sanitize(&c.summary), base));
        draw_cells(buf, r.x, y, w, &cells);
    }
    title
}

/// Draws `(text, style)` pieces left to right within `w` cells; the last piece that does not fit
/// ends with "…".
fn draw_cells(buf: &mut Buffer, x: u16, y: u16, w: usize, cells: &[(String, Style)]) {
    let mut used = 0usize;
    for (text, style) in cells {
        if used >= w {
            return;
        }
        let shown = fit_head(text, w - used);
        let sw = width(&shown);
        put(buf, x + used as u16, y, &shown, (w - used) as u16, *style);
        used += sw;
    }
}

// ---- Blame ------------------------------------------------------------------------------------

fn blame(buf: &mut Buffer, r: Rect, p: &mut GitPopup, cx: &Ctx) -> String {
    let theme = cx.theme;
    let b = &mut p.blame;
    let name = b
        .path
        .as_ref()
        .map(|path| {
            let rel = path.strip_prefix(&p.workdir).unwrap_or(path);
            sanitize(&rel.to_string_lossy())
        })
        .unwrap_or_default();
    let mut title = format!("Blame · {name}");
    if b.running {
        title.push_str(" · blaming…");
    }
    let shown = match &b.result {
        Some(Ok(Some(s))) => s,
        Some(Ok(None)) => {
            message(
                buf,
                r,
                "not tracked by git: nothing to blame",
                theme.popup.patch(theme.dim),
            );
            return title;
        }
        Some(Err(e)) => {
            message(buf, r, e, theme.popup.patch(theme.error));
            return title;
        }
        None => {
            let text = if b.running {
                format!("blaming {name}…")
            } else {
                String::new()
            };
            message(buf, r, &text, theme.popup.patch(theme.dim));
            return title;
        }
    };
    let n = shown.blame.lines.len();
    if n == 0 {
        message(buf, r, "empty file", theme.popup.patch(theme.dim));
        return title;
    }
    let rows = usize::from(r.height);
    b.cursor = b.cursor.min(n - 1);
    follow(&mut b.top, b.cursor, rows, n);
    let w = usize::from(r.width);
    let author_w = match w {
        w if w >= 100 => 18,
        w if w >= 70 => 16,
        w if w >= 50 => 10,
        _ => 0,
    };
    let ln_w = n.to_string().len();
    let gutter = 4 + 1 + if author_w > 0 { author_w + 1 } else { 0 } + ln_w + 1;
    let room = w.saturating_sub(gutter);
    // Ages coloured from the oldest to the newest commit of this file.
    let times: Vec<i64> = shown
        .blame
        .commits
        .iter()
        .filter(|c| !c.uncommitted)
        .map(|c| c.author.time)
        .collect();
    let (old, new) = (
        times.iter().copied().min().unwrap_or(0),
        times.iter().copied().max().unwrap_or(0),
    );
    let widest = shown.widest;
    let far = if widest > room { widest + 1 - room } else { 0 };
    b.left = b.left.min(far);
    for i in b.top..(b.top + rows).min(n) {
        let y = r.y + (i - b.top) as u16;
        let line = shown.blame.lines[i];
        let c = &shown.blame.commits[line.commit];
        let first = i == 0 || shown.blame.lines[i - 1].commit != line.commit;
        let selected = i == b.cursor;
        let base = if selected {
            theme.list_selected
        } else {
            theme.popup
        };
        buf.set_style(Rect::new(r.x, y, r.width, 1), base);
        let age_style = if selected {
            base
        } else if c.uncommitted {
            base.patch(theme.git_added)
        } else {
            age_style(theme, base, c.author.time, old, new)
        };
        let mut cells: Vec<(String, Style)> = Vec::new();
        if first {
            let (age, who) = if c.uncommitted {
                ("+".to_string(), "You, uncommitted".to_string())
            } else {
                (short_age(cx.now - c.author.time), sanitize(&c.author.name))
            };
            cells.push((format!("{age:>4} "), age_style));
            if author_w > 0 {
                let who = fit_head(&who, author_w);
                cells.push((format!("{who:<author_w$} "), age_style));
            }
        } else {
            let bar = if author_w > 0 { author_w + 1 } else { 0 };
            cells.push((format!("{:>4} {:bar$}", "│", ""), age_style));
        }
        cells.push((
            format!("{:>ln_w$} ", i + 1),
            if selected {
                base
            } else {
                base.patch(theme.dim)
            },
        ));
        let text = shown.lines.get(i).map_or("", String::as_str);
        let text = if b.left > 0 && !text.is_empty() {
            format!(
                "…{}",
                fit_head(skip_cells(text, b.left), room.saturating_sub(1))
            )
        } else {
            fit_head(text, room)
        };
        cells.push((text, base));
        draw_cells(buf, r.x, y, w, &cells);
    }
    title
}

/// Newest brightest: the top third of the file's time span bold in the "changed" colour, the
/// middle in the text colour, the oldest in the quiet blame colour (theme colours only).
fn age_style(theme: &Theme, base: Style, t: i64, old: i64, new: i64) -> Style {
    let frac = if new > old {
        (t - old) as f64 / (new - old) as f64
    } else {
        1.0
    };
    if frac >= 2.0 / 3.0 {
        base.patch(theme.git_modified).add_modifier(Modifier::BOLD)
    } else if frac >= 1.0 / 3.0 {
        base
    } else {
        base.patch(theme.git_blame)
    }
}

// ---- Commit view ------------------------------------------------------------------------------

fn commit_view(buf: &mut Buffer, r: Rect, p: &mut GitPopup, cx: &Ctx) -> String {
    let theme = cx.theme;
    let Some(c) = &mut p.commit else {
        return String::new();
    };
    let title = match &c.info {
        Some(i) => format!(
            "Commit {} · {}",
            sanitize(&i.oid[..i.oid.len().min(7)]),
            sanitize(i.summary())
        ),
        None => format!("Commit {}", c.rev),
    };
    if c.loading {
        message(
            buf,
            r,
            &format!("loading {}…", c.rev),
            theme.popup.patch(theme.dim),
        );
        return title;
    }
    let h = usize::from(r.height);
    let n = c.rows.len();
    c.top = c.top.min(n.saturating_sub(h));
    c.begin_frame();
    let w = usize::from(r.width);
    let rows: Vec<CvRow> = c.rows.iter().skip(c.top).take(h).cloned().collect();
    // Sideways scroll up to the widest row on screen (widths measured once per row).
    let widest = (c.top..c.top + rows.len())
        .map(|i| c.row_width(i))
        .max()
        .unwrap_or(0);
    let far = if widest > w { widest + 1 - w } else { 0 };
    c.left = c.left.min(far);
    let left = c.left;
    for (k, row) in rows.iter().enumerate() {
        let y = r.y + k as u16;
        match row {
            CvRow::Line(f, hk, l) => {
                let line = &c.diffs[*f].hunks[*hk].lines[*l];
                let (mark, st) = match line.kind {
                    LineKind::Added => ('+', theme.popup.patch(theme.git_added)),
                    LineKind::Removed => ('-', theme.popup.patch(theme.git_deleted)),
                    LineKind::Context => (' ', theme.popup),
                };
                // Only the cells on screen are built and styled: a minified line of half a
                // megabyte costs one row of cells, not one per char of the line.
                let win = window(
                    &String::from_utf8_lossy(&line.text),
                    left,
                    w.saturating_sub(1),
                );
                let styles = match win.bytes() {
                    Some(range) => {
                        let spans = c.spans((*f, *hk, *l), range, cx.registry, cx.notify);
                        span_styles(&win, spans.as_deref(), st, theme)
                    }
                    None => Vec::new(),
                };
                c.work.cells += win.cells.len() as u64;
                put(buf, r.x, y, &mark.to_string(), 1, st);
                draw_window(buf, r.x + 1, y, w.saturating_sub(1), &win, &styles);
            }
            _ => {
                let (text, style) = match row {
                    CvRow::Meta(s) if s.starts_with("commit ") => {
                        (s.clone(), theme.popup.patch(theme.git_modified))
                    }
                    CvRow::Meta(s) => (s.clone(), theme.popup),
                    CvRow::Message(s) => (format!("    {s}"), theme.popup),
                    CvRow::Info(s) => (s.clone(), theme.popup.patch(theme.dim)),
                    CvRow::FileHead(_) => (
                        format!("── {}", c.row_text(row)),
                        theme.popup.patch(theme.focus),
                    ),
                    CvRow::Hunk(..) => (c.row_text(row), theme.popup.patch(theme.dim)),
                    CvRow::FileStat(i) => {
                        let st = match c.diffs[*i].status {
                            vr_git::FileStatus::Added => theme.git_added,
                            vr_git::FileStatus::Deleted => theme.git_deleted,
                            _ => theme.git_modified,
                        };
                        (c.row_text(row), theme.popup.patch(st))
                    }
                    _ => (String::new(), theme.popup),
                };
                let win = window(&text, left, w);
                let styles = vec![style; win.cells.len()];
                draw_window(buf, r.x, y, w, &win, &styles);
            }
        }
    }
    title
}

/// The cells of a line that fall in columns `left..left + w`, built without walking the line
/// past them: control characters as placeholders (SEC-15), TAB to the next stop of 4.
struct Window {
    /// (cell, byte offset of the char it shows) of the visible part, up to one cell past `w`.
    cells: Vec<(String, usize)>,
    /// Bytes of the char each cell shows (parallel to `cells`).
    lens: Vec<usize>,
    /// Cells were skipped on the left.
    hidden_left: bool,
    /// The visible part plus a leading "…" does not fit in `w`.
    cut: bool,
}

impl Window {
    /// The bytes of the line the visible cells show.
    fn bytes(&self) -> Option<std::ops::Range<usize>> {
        let first = self.cells.first()?.1;
        let last = self.cells.len() - 1;
        Some(first..self.cells[last].1 + self.lens[last])
    }
}

fn window(text: &str, left: usize, w: usize) -> Window {
    let mut win = Window {
        cells: Vec::new(),
        lens: Vec::new(),
        hidden_left: false,
        cut: false,
    };
    let (mut col, mut skipped, mut total) = (0usize, 0usize, 0usize);
    let mut piece = String::new();
    'line: for (i, ch) in text.char_indices() {
        piece.clear();
        match ch {
            '\t' => piece.extend(std::iter::repeat_n(' ', 4 - col % 4)),
            ' '..='~' => piece.push(ch),
            _ => piece.push_str(&sanitize(ch.encode_utf8(&mut [0; 4]))),
        }
        for s in piece.chars() {
            col += 1;
            let cw = width(s.encode_utf8(&mut [0; 4]));
            if skipped < left {
                skipped += cw;
                win.hidden_left = true;
                // A wide char cut by the edge leaves its right half visible: blank cells keep
                // the rest of the line at its column.
                for _ in left..skipped {
                    win.cells.push((" ".to_string(), i));
                    win.lens.push(ch.len_utf8());
                    total += 1;
                }
                continue;
            }
            win.cells.push((s.to_string(), i));
            win.lens.push(ch.len_utf8());
            total += cw;
            if total > w {
                // More than fits: enough is known to draw it.
                break 'line;
            }
        }
    }
    let lead = usize::from(win.hidden_left && !win.cells.is_empty());
    win.cut = lead + total > w;
    win
}

/// The style of each cell of `win`: the syntax colour of the span over its char, walking the
/// sorted spans with one moving index.
fn span_styles(
    win: &Window,
    spans: Option<&[(std::ops::Range<usize>, Capture)]>,
    base: Style,
    theme: &Theme,
) -> Vec<Style> {
    let spans = spans.unwrap_or(&[]);
    let mut at = 0usize;
    win.cells
        .iter()
        .map(|(_, i)| {
            while at < spans.len() && spans[at].0.end <= *i {
                at += 1;
            }
            match spans.get(at) {
                Some((r, cap)) if r.start <= *i => base.patch(theme.syntax(*cap)),
                _ => base,
            }
        })
        .collect()
}

/// Draws `win` within `w` cells: "…" first when something is hidden on the left, "…" last
/// when the rest does not fit.
fn draw_window(buf: &mut Buffer, x: u16, y: u16, w: usize, win: &Window, styles: &[Style]) {
    if w == 0 || win.cells.is_empty() {
        return;
    }
    let mut used = 0usize;
    if win.hidden_left {
        put(buf, x, y, "…", 1, styles[0]);
        used = 1;
    }
    let room = if win.cut { w - 1 } else { w };
    let mut last = styles[0];
    for ((s, _), style) in win.cells.iter().zip(styles) {
        let cw = width(s);
        if used + cw > room {
            last = *style;
            break;
        }
        put(buf, x + used as u16, y, s, cw as u16, *style);
        used += cw;
        last = *style;
    }
    if win.cut {
        put(buf, x + used as u16, y, "…", 1, last);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cells `draw_window` paints for `text` scrolled by `left`, as a string.
    fn drawn(text: &str, left: usize, w: usize) -> String {
        let win = window(text, left, w);
        let styles = vec![Style::default(); win.cells.len()];
        let mut buf = Buffer::empty(Rect::new(0, 0, w as u16, 1));
        draw_window(&mut buf, 0, 0, w, &win, &styles);
        let mut out = String::new();
        let mut x = 0u16;
        while x < w as u16 {
            let s = buf[(x, 0)].symbol();
            x += width(s).max(1) as u16;
            out.push_str(s);
        }
        out
    }

    #[test]
    fn a_wide_char_cut_by_the_left_edge_keeps_the_rest_at_its_column() {
        // "日本語abc": cells 0-1 日, 2-3 本, 4-5 語, 6 a. Scrolled by 1 the right half of 日
        // is the first visible cell: a blank keeps 本 one column
        // right of where it is with the even scroll.
        let even = drawn("日本語abc", 2, 12);
        let odd = drawn("日本語abc", 1, 12);
        assert!(even.starts_with("…本語abc"), "{even:?}");
        assert!(odd.starts_with("… 本語abc"), "{odd:?}");
        // 本 is one column further right than with the even scroll (it is one cell further
        // from the left edge), not on the same column as before the fix.
        let cols = |s: &str| s.chars().position(|c| c == '本').unwrap();
        assert_eq!(cols(&odd), cols(&even) + 1);
    }
}
