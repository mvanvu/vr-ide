//! The editor area: line-number gutter and text. Only the visible lines are read from the
//! rope, so frame time does not depend on file size (AC-18).
//!
//! Columns are display cells measured per grapheme cluster (combining marks take none,
//! CJK and emoji take two, tabs run to the next tab stop): `vr_core::display_col`.
//!
//! A line longer than [`LONG_LINE`] chars (minified code, a one-line data file) is never
//! measured from its start: there the view's `left` is a char offset and cells are counted
//! from it, so a keystroke on a 10 MB line costs as much as on a short one. Tab stops on such
//! a line are counted from `left`.

use std::borrow::Cow;

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use vr_core::ropey::{Rope, RopeSlice};
use vr_core::text::{next_grapheme_boundary, prev_grapheme_boundary};

use crate::app::{App, Focus};
use crate::theme::Theme;

/// Lines with more chars than this are drawn from the scroll offset, not from their start.
pub const LONG_LINE: usize = 16 * 1024;

/// A grapheme cluster of a line and where it lands on screen.
pub struct Cell<'a> {
    /// Char offsets relative to the line start.
    pub start: usize,
    pub end: usize,
    /// Display column (from where the walk started) and width in cells.
    pub col: usize,
    pub width: usize,
    pub symbol: Cow<'a, str>,
}

/// Chars of `line` before its line break.
pub fn content_len(line: RopeSlice) -> usize {
    let mut n = line.len_chars();
    if n > 0 && line.char(n - 1) == '\n' {
        n -= 1;
    }
    if n > 0 && line.char(n - 1) == '\r' {
        n -= 1;
    }
    n
}

pub fn is_long(line: RopeSlice) -> bool {
    line.len_chars() > LONG_LINE
}

pub fn is_control(g: &str) -> bool {
    g.starts_with(char::is_control)
}

/// Cells a grapheme takes at display column `col` (the same widths as vr-core).
pub fn grapheme_cells(g: &str, col: usize, tab_width: usize) -> usize {
    if g == "\t" {
        let tw = tab_width.max(1);
        tw - col % tw
    } else {
        Span::raw(g).width()
    }
}

/// Start of the grapheme holding char `i` of `line` (the content end when past it).
pub fn grapheme_start(line: RopeSlice, i: usize) -> usize {
    let len = content_len(line);
    if i >= len {
        len
    } else {
        prev_grapheme_boundary(line, i + 1)
    }
}

/// Calls `f` for each grapheme of `line` from char `from` (a grapheme boundary) to the line
/// break, until it returns false. Columns count from `from`. Returns the column and char
/// offset after the last grapheme visited.
pub fn walk_from(
    line: RopeSlice,
    from: usize,
    tab_width: usize,
    mut f: impl FnMut(Cell) -> bool,
) -> (usize, usize) {
    let len = content_len(line);
    let (mut i, mut col) = (from.min(len), 0);
    while i < len {
        let j = next_grapheme_boundary(line, i).clamp(i + 1, len);
        let symbol: Cow<str> = line.slice(i..j).into();
        let width = grapheme_cells(&symbol, col, tab_width);
        let go_on = f(Cell {
            start: i,
            end: j,
            col,
            width,
            symbol,
        });
        col += width;
        i = j;
        if !go_on {
            break;
        }
    }
    (col, i)
}

/// [`walk_from`] the line start.
pub fn walk_line(line: RopeSlice, tab_width: usize, f: impl FnMut(Cell) -> bool) -> usize {
    walk_from(line, 0, tab_width, f).0
}

/// Char offset (relative to the line start) under screen column `x` of the text area, for a
/// view scrolled to `left`; past the end gives the content end.
pub fn char_at_screen_col(line: RopeSlice, left: usize, x: usize, tab_width: usize) -> usize {
    if !is_long(line) {
        return vr_core::text::char_at_display_col(line, left + x, tab_width);
    }
    let mut found = None;
    walk_from(line, grapheme_start(line, left), tab_width, |c| {
        if c.col + c.width > x {
            found = Some(c.start);
            false
        } else {
            true
        }
    });
    found.unwrap_or_else(|| content_len(line))
}

/// Horizontal scroll for a long line: the new `left` (a char offset) that shows `head`
/// within `text_w` cells, and the cursor's column from it. Walks at most one screen of
/// graphemes.
fn follow_long(
    line: RopeSlice,
    left: usize,
    head: usize,
    text_w: usize,
    tw: usize,
) -> (usize, usize) {
    let start = grapheme_start(line, left);
    if head < start || text_w == 0 {
        return (head, 0);
    }
    let x_from = |from: usize| {
        let mut x = None;
        let (end_col, end) = walk_from(line, from, tw, |c| {
            if c.start >= head {
                x = Some(c.col);
                return false;
            }
            c.col + c.width < text_w
        });
        x.or((end == head).then_some(end_col))
    };
    if let Some(x) = x_from(start).filter(|&x| x < text_w) {
        return (start, x);
    }
    // Off to the right: put the cursor in the last column.
    let mut s = head;
    let mut cells = 0;
    while s > 0 {
        let p = prev_grapheme_boundary(line, s);
        let g: Cow<str> = line.slice(p..s).into();
        let w = if g == "\t" {
            tw.max(1)
        } else {
            grapheme_cells(&g, 0, tw)
        };
        if cells + w >= text_w {
            break;
        }
        cells += w;
        s = p;
    }
    let x = x_from(s).unwrap_or(0).min(text_w - 1);
    (s, x)
}

/// Line-number column plus one space; at least three digits so the text does not shift
/// for small files.
pub fn gutter_width(rope: &Rope) -> u16 {
    let digits = rope.len_lines().max(1).ilog10() as u16 + 1;
    digits.max(3) + 1
}

pub fn render(frame: &mut Frame, area: Rect, app: &mut App, theme: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    if app.docs.is_empty() {
        app.welcome_hits = super::welcome::render(frame.buffer_mut(), area, app, theme);
        return;
    }
    let active = app.active;
    let doc = &app.docs[active];
    let rope = doc.buffer.text();
    let tw = doc.buffer.options().tab_width.max(1);
    let gutter = gutter_width(rope).min(area.width);
    let text_w = usize::from(area.width - gutter);
    let rows = usize::from(area.height);
    let ranges: Vec<(usize, usize)> = doc
        .buffer
        .selection()
        .ranges()
        .iter()
        .map(|r| (r.from(), r.to()))
        .collect();
    let head = doc.buffer.selection().primary().head;
    let head_line = rope.char_to_line(head);
    let head_slice = rope.line(head_line);
    let head_in_line = head - rope.line_to_char(head_line);
    let last_line = rope.len_lines().saturating_sub(1);

    let view = &mut app.views[active];
    if view.follow_cursor {
        if head_line < view.top {
            view.top = head_line;
        } else if head_line >= view.top + rows {
            view.top = head_line + 1 - rows;
        }
        view.follow_cursor = false;
    }
    // Horizontal scrolling has no other driver than the cursor, so it always follows it
    // (this also re-fits the view after a resize).
    let cursor_x = if is_long(head_slice) {
        let (left, x) = follow_long(head_slice, view.left, head_in_line, text_w, tw);
        view.left = left;
        x
    } else {
        let head_col = vr_core::display_col(head_slice, head_in_line, tw);
        if head_col < view.left {
            view.left = head_col;
        } else if text_w > 0 && head_col >= view.left + text_w {
            view.left = head_col + 1 - text_w;
        }
        head_col - view.left
    };
    view.top = view.top.min(last_line);
    let (top, left) = (view.top, view.left);

    let buf = frame.buffer_mut();
    let x0 = area.x + gutter;
    let selected = |abs: usize| ranges.iter().any(|&(f, t)| abs >= f && abs < t);
    // Find bar matches (sorted, non-overlapping): a binary search per drawn cell.
    let matches: &[(usize, usize)] = if app.find.open {
        &app.find.matches
    } else {
        &[]
    };
    let in_match = |abs: usize| {
        let i = matches.partition_point(|&(_, e)| e <= abs);
        matches.get(i).is_some_and(|&(s, _)| s <= abs)
    };
    let spans = visible_spans(app, active, top, left, rows, text_w);
    for row in 0..rows {
        let ln = top + row;
        if ln > last_line {
            break;
        }
        let y = area.y + row as u16;
        let num_style = if ln == head_line && app.focus == Focus::Editor && app.dialog.is_none() {
            theme.gutter_active
        } else if ln == head_line {
            theme.gutter_current
        } else {
            theme.gutter
        };
        let num = format!("{:>w$} ", ln + 1, w = usize::from(gutter.saturating_sub(1)));
        buf.set_stringn(area.x, y, num, usize::from(gutter), num_style);

        let line = rope.line(ln);
        let line_start = rope.line_to_char(ln);
        // Short lines: `left` is a display column; long lines start drawing at char `left`.
        let (from, skip) = if is_long(line) {
            (grapheme_start(line, left), 0)
        } else {
            (0, left)
        };
        let put = |buf: &mut Buffer, col: usize, sym: &str, w: usize, style: Style| {
            let x = x0 + (col - skip) as u16;
            buf.set_stringn(x, y, sym, w, style);
        };
        // Byte offset of each cell, for the syntax spans (bytes of the document).
        let mut at = if spans.is_empty() {
            0
        } else {
            rope.char_to_byte(line_start + from)
        };
        let mut k = spans.partition_point(|(r, _)| r.end <= at);
        let (end_col, end) = walk_from(line, from, tw, |c| {
            let byte = at;
            at += c.symbol.len();
            if c.col + c.width <= skip && c.width > 0 || c.col < skip && c.width == 0 {
                return true;
            }
            if c.col >= skip + text_w {
                return false;
            }
            let (sel, found) = (
                selected(line_start + c.start),
                in_match(line_start + c.start),
            );
            let plain = if sel {
                theme.selection
            } else if found {
                theme.text.patch(theme.find_match)
            } else {
                theme.text
            };
            while spans.get(k).is_some_and(|(r, _)| r.end <= byte) {
                k += 1;
            }
            let capture = spans
                .get(k)
                .filter(|(r, _)| r.start <= byte)
                .filter(|_| theme.syntax_on_highlight || !(sel || found))
                .map(|&(_, cap)| cap);
            // Syntax colours stay under the selection and find matches when the theme allows
            // it; only the background says which one it is.
            let style = match capture {
                Some(cap) => {
                    let code = theme.text.patch(theme.syntax(cap));
                    if sel {
                        code.patch(theme.selection)
                    } else if found {
                        code.patch(theme.find_match)
                    } else {
                        code
                    }
                }
                None => plain,
            };
            let clipped = c.col < skip || c.col + c.width > skip + text_w;
            if clipped || c.symbol == "\t" {
                // Tabs and wide chars cut by the view edge show as blank cells.
                for col in c.col.max(skip)..(c.col + c.width).min(skip + text_w) {
                    put(buf, col, " ", 1, style);
                }
            } else if is_control(&c.symbol) {
                put(buf, c.col, "?", 1, theme.control.patch(plain));
            } else if c.width > 0 {
                put(buf, c.col, &c.symbol, c.width, style);
            }
            true
        });
        // A selection running over the line break shows one selected cell after the text.
        let content = content_len(line);
        let brk = line_start + content;
        let has_break = content < line.len_chars();
        if has_break
            && end == content
            && selected(brk)
            && end_col >= skip
            && end_col < skip + text_w
        {
            put(buf, end_col, " ", 1, theme.selection);
        }
    }

    if app.focus == Focus::Editor
        && app.dialog.is_none()
        && head_line >= top
        && head_line < top + rows
        && cursor_x < text_w.max(1)
    {
        let x = x0 + cursor_x as u16;
        let y = area.y + (head_line - top) as u16;
        frame.set_cursor_position((x, y));
    }
}

/// The window of a long line whose spans are fetched: graphemes are at most a few chars per
/// cell, so this covers the screen without reading the rest of a 10 MB line.
const LONG_LINE_SPAN_CHARS: usize = 64;

/// Above this many bytes between the first and the last visible char (long lines in view), the
/// spans are fetched per row instead of for the whole window.
const SPAN_WINDOW_BYTES: usize = 256 * 1024;

/// Syntax spans of the visible part of document `i` (sorted, bytes of the document); empty for
/// plain text. Only the drawn rows are asked for, so cost follows the screen, not the file.
fn visible_spans(
    app: &App,
    i: usize,
    top: usize,
    left: usize,
    rows: usize,
    text_w: usize,
) -> Vec<(std::ops::Range<usize>, vr_syntax::Capture)> {
    if app.syntax.language(i).is_none() {
        return Vec::new();
    }
    let rope = app.docs[i].buffer.text();
    let last = rope.len_lines().saturating_sub(1);
    let segments: Vec<std::ops::Range<usize>> = (top..=(top + rows).min(last))
        .take(rows)
        .map(|ln| {
            let line = rope.line(ln);
            let start = rope.line_to_char(ln);
            let len = content_len(line);
            let (from, to) = if is_long(line) {
                let from = grapheme_start(line, left);
                (from, (from + text_w * 4 + LONG_LINE_SPAN_CHARS).min(len))
            } else {
                (0, len)
            };
            rope.char_to_byte(start + from)..rope.char_to_byte(start + to)
        })
        .collect();
    let (Some(first), Some(end)) = (segments.first(), segments.last()) else {
        return Vec::new();
    };
    if end.end - first.start <= SPAN_WINDOW_BYTES {
        return app.syntax.spans(i, first.start..end.end);
    }
    segments
        .into_iter()
        .flat_map(|seg| app.syntax.spans(i, seg))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The cells the renderer draws sit at vr-core's columns (motions, Tab and rendering
    /// agree), and a screen column maps back to vr-core's char.
    #[test]
    fn columns_match_vr_core() {
        for text in [
            "a\tb\t\tc\n",
            "Tie\u{302}\u{301}ng Việt\n",
            "漢字x👨‍👩‍👧🇻🇳\r\n",
            "nul\u{0}bel\u{7}x\tz",
            "\u{1b}[31m\tred",
        ] {
            let rope = Rope::from_str(text);
            let line = rope.line(0);
            for tw in [1, 4, 8] {
                walk_line(line, tw, |c| {
                    assert_eq!(
                        c.col,
                        vr_core::display_col(line, c.start, tw),
                        "col of {} in {text:?} (tab {tw})",
                        c.start
                    );
                    true
                });
                for x in 0..40 {
                    assert_eq!(
                        char_at_screen_col(line, 0, x, tw),
                        vr_core::text::char_at_display_col(line, x, tw),
                        "char at col {x} in {text:?} (tab {tw})"
                    );
                }
            }
        }
    }

    #[test]
    fn long_line_follow_and_hit_test() {
        let line_text = "ab漢\t".repeat(LONG_LINE);
        let rope = Rope::from_str(&line_text);
        let line = rope.line(0);
        assert!(is_long(line));
        let len = content_len(line);
        // Cursor at the end: the view starts less than a screen before it.
        let (left, x) = follow_long(line, 0, len, 40, 4);
        assert!(left < len && len - left <= 40 && x < 40, "{left} {x}");
        assert_eq!(char_at_screen_col(line, left, x, 4), len);
        // Moving back inside the view keeps it; before it scrolls to the cursor.
        let (l2, x2) = follow_long(line, left, len - 1, 40, 4);
        assert_eq!(l2, left);
        assert!(x2 < x);
        assert_eq!(follow_long(line, left, 5, 40, 4), (5, 0));
        assert_eq!(char_at_screen_col(line, 5, 0, 4), 5);
    }
}
