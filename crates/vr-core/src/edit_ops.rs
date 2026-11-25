//! Line-oriented edit operations. Each one is a single `Transaction` (one undo step).
//! The lines an operation touches are every line a range covers; a range ending at column 0
//! of a line does not cover that line.

use ropey::Rope;

use crate::selection::{Pos, Range, Selection};
use crate::text::{
    Buffer, char_to_pos, default_line_break, is_line_break, line_break_str, line_content_end,
    pos_to_char,
};
use crate::transaction::{Change, Transaction, apply_changes, normalize};

/// Inclusive line spans covered by the selection, sorted, with touching spans merged.
fn line_blocks(rope: &Rope, sel: &Selection) -> Vec<(usize, usize)> {
    let mut blocks: Vec<(usize, usize)> = Vec::new();
    for r in sel.ranges() {
        let s = rope.char_to_line(r.from());
        let mut e = rope.char_to_line(r.to());
        if e > s && r.to() == rope.line_to_char(e) {
            e -= 1;
        }
        match blocks.last_mut() {
            Some(last) if s <= last.1 + 1 => last.1 = last.1.max(e),
            _ => blocks.push((s, e)),
        }
    }
    blocks
}

fn block_index(blocks: &[(usize, usize)], line: usize) -> usize {
    blocks
        .iter()
        .position(|&(_, e)| line <= e)
        .unwrap_or(blocks.len().saturating_sub(1))
}

fn line_start(rope: &Rope, line: usize) -> usize {
    rope.line_to_char(line)
}

/// Char offset where the line after `line` starts (document end for the last line).
fn after_line(rope: &Rope, line: usize) -> usize {
    if line + 1 < rope.len_lines() {
        rope.line_to_char(line + 1)
    } else {
        rope.len_chars()
    }
}

fn content(rope: &Rope, line: usize) -> String {
    rope.slice(line_start(rope, line)..line_content_end(rope, line))
        .to_string()
}

/// Byte length of `line` (one rope line) without its break, the way [`line_content_end`]
/// counts it: CRLF is one break.
fn content_len(line: &str) -> usize {
    match line.chars().next_back() {
        Some(c) if is_line_break(c) => {
            let n = line.len() - c.len_utf8();
            if c == '\n' && line[..n].ends_with('\r') {
                n - 1
            } else {
                n
            }
        }
        _ => line.len(),
    }
}

fn leading_blank_chars(s: &str) -> usize {
    s.chars().take_while(|c| matches!(c, ' ' | '\t')).count()
}

/// Applies `changes` as one transaction whose selection is rebuilt from line/column
/// positions: `f` maps each range's (anchor, head) positions in the old text to positions
/// in the new text.
fn apply_by_pos(buf: &mut Buffer, changes: Vec<Change>, f: impl Fn(Range, Pos, Pos) -> (Pos, Pos)) {
    let old = buf.text();
    let changes = normalize(changes, old.len_chars());
    if changes.is_empty() {
        return;
    }
    let mut new = old.clone();
    apply_changes(&mut new, &changes);
    let ranges = buf
        .selection()
        .ranges()
        .iter()
        .map(|&r| {
            let (a, h) = f(r, char_to_pos(old, r.anchor), char_to_pos(old, r.head));
            Range::new(pos_to_char(&new, a), pos_to_char(&new, h))
        })
        .collect();
    let sel = Selection::new(ranges, buf.selection().primary_index());
    buf.apply(Transaction::new(changes).with_selection(sel));
}

pub fn delete_line(buf: &mut Buffer) {
    let rope = buf.text();
    let blocks = line_blocks(rope, buf.selection());
    let last = rope.len_lines().saturating_sub(1);
    let changes = blocks
        .iter()
        .map(|&(s, e)| {
            if e < last {
                Change::new(line_start(rope, s), line_start(rope, e + 1), "")
            } else if s > 0 {
                // The last line has no break of its own: remove the one before it.
                Change::new(line_content_end(rope, s - 1), rope.len_chars(), "")
            } else {
                Change::new(0, rope.len_chars(), "")
            }
        })
        .collect();
    let removed_before: Vec<usize> = blocks
        .iter()
        .scan(0, |acc, &(s, e)| {
            let before = *acc;
            *acc += e - s + 1;
            Some(before)
        })
        .collect();
    let rope = rope.clone();
    apply_by_pos(buf, changes, |r, _, head| {
        let b = block_index(&blocks, rope.char_to_line(r.from()));
        let p = Pos {
            line: blocks[b].0 - removed_before[b],
            col: head.col,
        };
        (p, p)
    });
}

pub fn duplicate_line(buf: &mut Buffer) {
    let rope = buf.text();
    let blocks = line_blocks(rope, buf.selection());
    let eol = default_line_break(rope);
    let changes = blocks
        .iter()
        .map(|&(s, e)| {
            let text = rope
                .slice(line_start(rope, s)..line_content_end(rope, e))
                .to_string();
            let brk = line_break_str(rope, e);
            let brk = if brk.is_empty() { eol.clone() } else { brk };
            Change::new(
                line_content_end(rope, e),
                line_content_end(rope, e),
                brk + &text,
            )
        })
        .collect();
    // Ranges move onto the copy, which sits right below the original block.
    let shift_through: Vec<usize> = blocks
        .iter()
        .scan(0, |acc, &(s, e)| {
            *acc += e - s + 1;
            Some(*acc)
        })
        .collect();
    let rope = rope.clone();
    apply_by_pos(buf, changes, |r, anchor, head| {
        let shift = shift_through[block_index(&blocks, rope.char_to_line(r.from()))];
        (
            Pos {
                line: anchor.line + shift,
                col: anchor.col,
            },
            Pos {
                line: head.line + shift,
                col: head.col,
            },
        )
    });
}

/// Rewrites lines `first..=last` with their contents in `order`; line breaks stay in place so
/// a last line without a trailing break still has none.
fn reorder_lines(rope: &Rope, first: usize, last: usize, order: &[usize]) -> Change {
    let contents: Vec<String> = (first..=last).map(|l| content(rope, l)).collect();
    let mut out = String::new();
    for (i, &src) in order.iter().enumerate() {
        out.push_str(&contents[src]);
        out.push_str(&line_break_str(rope, first + i));
    }
    Change::new(line_start(rope, first), after_line(rope, last), out)
}

fn move_lines(buf: &mut Buffer, down: bool) {
    let rope = buf.text();
    let blocks = line_blocks(rope, buf.selection());
    let last = rope.len_lines().saturating_sub(1);
    let at_edge = blocks
        .iter()
        .any(|&(s, e)| if down { e >= last } else { s == 0 });
    if blocks.is_empty() || at_edge {
        return;
    }
    let changes = blocks
        .iter()
        .map(|&(s, e)| {
            let n = e - s + 1;
            if down {
                let order: Vec<usize> = std::iter::once(n).chain(0..n).collect();
                reorder_lines(rope, s, e + 1, &order)
            } else {
                let order: Vec<usize> = (1..=n).chain(std::iter::once(0)).collect();
                reorder_lines(rope, s - 1, e, &order)
            }
        })
        .collect();
    let shift = |p: Pos| {
        let in_block = blocks.iter().any(|&(s, e)| (s..=e).contains(&p.line));
        match (in_block, down) {
            (false, _) => p,
            (true, true) => Pos {
                line: p.line + 1,
                col: p.col,
            },
            (true, false) => Pos {
                line: p.line - 1,
                col: p.col,
            },
        }
    };
    apply_by_pos(buf, changes, |_, anchor, head| (shift(anchor), shift(head)));
}

pub fn move_line_up(buf: &mut Buffer) {
    move_lines(buf, false);
}

pub fn move_line_down(buf: &mut Buffer) {
    move_lines(buf, true);
}

/// Inserts the indent unit at the start of every covered line; in a multi-line block,
/// empty lines are left alone so no trailing whitespace appears. A selection starting at
/// column 0 keeps its start there so it still covers the whole indented lines.
pub fn indent(buf: &mut Buffer) {
    let rope = buf.text();
    let unit = buf.options().indent_unit.clone();
    let unit_cols = unit.chars().count();
    let mut changes = Vec::new();
    let mut indented = Vec::new();
    for (s, e) in line_blocks(rope, buf.selection()) {
        for l in s..=e {
            if s < e && line_content_end(rope, l) == line_start(rope, l) {
                continue;
            }
            changes.push(Change::new(
                line_start(rope, l),
                line_start(rope, l),
                unit.clone(),
            ));
            indented.push(l);
        }
    }
    let shift = move |p: Pos, is_selection_start: bool| {
        let keep = !indented.contains(&p.line) || (p.col == 0 && is_selection_start);
        if keep {
            p
        } else {
            Pos {
                line: p.line,
                col: p.col + unit_cols,
            }
        }
    };
    apply_by_pos(buf, changes, |r, anchor, head| {
        let start_is_anchor = !r.is_empty() && r.anchor < r.head;
        let start_is_head = !r.is_empty() && r.head < r.anchor;
        (shift(anchor, start_is_anchor), shift(head, start_is_head))
    });
}

/// Removes one leading tab, or up to one indent width of leading spaces, from each covered line.
pub fn outdent(buf: &mut Buffer) {
    let rope = buf.text();
    let unit = &buf.options().indent_unit;
    let width = if unit.is_empty() || unit.contains('\t') {
        buf.options().tab_width.max(1)
    } else {
        unit.chars().count()
    };
    let mut changes = Vec::new();
    for (s, e) in line_blocks(rope, buf.selection()) {
        for l in s..=e {
            let start = line_start(rope, l);
            let text = content(rope, l);
            let n = if text.starts_with('\t') {
                1
            } else {
                text.chars().take(width).take_while(|&c| c == ' ').count()
            };
            if n > 0 {
                changes.push(Change::new(start, start + n, ""));
            }
        }
    }
    buf.apply(Transaction::new(changes));
}

/// Line comments: when every non-blank covered line starts with `token` (after its
/// indentation) the token and one following space are removed; otherwise `token ` is added
/// to every non-blank line at the smallest indentation, so relative indentation is kept.
pub fn toggle_comment(buf: &mut Buffer, token: &str) {
    if token.is_empty() {
        return;
    }
    let rope = buf.text();
    let lines: Vec<(usize, String)> = line_blocks(rope, buf.selection())
        .into_iter()
        .flat_map(|(s, e)| s..=e)
        .map(|l| (line_start(rope, l), content(rope, l)))
        .filter(|(_, text)| !text.trim().is_empty())
        .collect();
    if lines.is_empty() {
        return;
    }
    let commented = |text: &str| text.trim_start_matches([' ', '\t']).starts_with(token);
    let token_len = token.chars().count();
    let changes = if lines.iter().all(|(_, text)| commented(text)) {
        lines
            .iter()
            .map(|(start, text)| {
                let at = start + leading_blank_chars(text);
                let rest = &text[text.len() - text.trim_start_matches([' ', '\t']).len()..];
                let space = usize::from(rest[token.len()..].starts_with(' '));
                Change::new(at, at + token_len + space, "")
            })
            .collect()
    } else {
        let indent = lines
            .iter()
            .map(|(_, t)| leading_blank_chars(t))
            .min()
            .unwrap_or(0);
        let insert = format!("{token} ");
        lines
            .iter()
            .map(|(start, _)| Change::new(start + indent, start + indent, insert.clone()))
            .collect()
    };
    buf.apply(Transaction::new(changes));
}

// ---- text commands on the selection or the whole document (T20b) -----------------------------
//
// Each one is a single transaction (one undo step) and reports whether it changed the text.
// With a non-empty range they act on what the ranges cover (line commands: the covered lines);
// without one, on the whole document.

/// The line blocks the non-empty ranges cover; with none, every line of the document except
/// the empty line after a final line break (it is the end of the file, not a line to sort).
fn text_blocks(rope: &Rope, sel: &Selection) -> (Vec<(usize, usize)>, bool) {
    if sel.ranges().iter().any(|r| !r.is_empty()) {
        let ranges: Vec<Range> = sel
            .ranges()
            .iter()
            .copied()
            .filter(|r| !r.is_empty())
            .collect();
        let only = Selection::new(ranges, 0);
        return (line_blocks(rope, &only), true);
    }
    let mut last = rope.len_lines().saturating_sub(1);
    if last > 0 && line_start(rope, last) == rope.len_chars() {
        last -= 1;
    }
    if rope.len_chars() == 0 {
        return (Vec::new(), false);
    }
    (vec![(0, last)], false)
}

/// Applies one change per block; a selection becomes the rewritten blocks, a cursor stays at
/// its line and column (clamped).
fn apply_blocks(buf: &mut Buffer, blocks: &[(usize, usize, String)], selected: bool) -> bool {
    let rope = buf.text();
    let changes: Vec<Change> = blocks
        .iter()
        .filter(|(from, to, new)| rope.slice(*from..*to) != new.as_str())
        .map(|(from, to, new)| Change::new(*from, *to, new.clone()))
        .collect();
    if changes.is_empty() {
        return false;
    }
    if selected {
        let mut shift: isize = 0;
        let ranges = blocks
            .iter()
            .map(|(from, to, text)| {
                let start = (*from as isize + shift) as usize;
                let len = text.chars().count();
                shift += len as isize - (*to - *from) as isize;
                Range::new(start, start + len)
            })
            .collect();
        buf.apply(Transaction::new(changes).with_selection(Selection::new(ranges, 0)));
        return true;
    }
    // The line and column are read before and placed after the one rebuild `apply` does;
    // building the new text a second time only for the cursor doubled the cost on big files.
    let p = char_to_pos(rope, buf.selection().primary().head);
    buf.apply(Transaction::new(changes));
    let at = pos_to_char(buf.text(), p);
    buf.set_selection(Selection::point(at));
    true
}

/// Replaces the lines of each block (selection, or the whole document) by `f` of them. Line
/// breaks stay in place: the n-th break of a block still ends its n-th line, so a block keeps
/// its own breaks and a last line without one still has none.
pub fn rewrite_lines(buf: &mut Buffer, f: impl Fn(Vec<String>) -> Vec<String>) -> bool {
    let rope = buf.text();
    let (blocks, selected) = text_blocks(rope, buf.selection());
    let rewritten: Vec<(usize, usize, String)> = blocks
        .iter()
        .map(|&(s, e)| {
            // One pass over the lines (no per-line lookups): a 1M-line file spent most of
            // its /sort time finding each line and its break again.
            let n = e - s + 1;
            let mut lines = Vec::with_capacity(n);
            let mut breaks = Vec::with_capacity(n);
            for l in rope.lines_at(s).take(n) {
                let mut text = l.to_string();
                let cut = content_len(&text);
                breaks.push(text.split_off(cut));
                lines.push(text);
            }
            let lines = f(lines);
            let mut out = String::with_capacity(lines.iter().map(|l| l.len() + 2).sum());
            for (i, line) in lines.iter().enumerate() {
                if i > 0 {
                    // Break n of the block ends line n; extra lines take the block's last one.
                    let at = (s + i - 1).min(e.saturating_sub(1)).max(s);
                    let brk = &breaks[at - s];
                    out.push_str(if brk.is_empty() { "\n" } else { brk });
                }
                out.push_str(line);
            }
            (line_start(rope, s), line_content_end(rope, e), out)
        })
        .collect();
    apply_blocks(buf, &rewritten, selected)
}

/// Replaces the selected text (or the whole document) by `f` of it.
pub fn rewrite_selection(buf: &mut Buffer, f: impl Fn(&str) -> String) -> bool {
    let rope = buf.text();
    let mut ranges: Vec<Range> = buf
        .selection()
        .ranges()
        .iter()
        .copied()
        .filter(|r| !r.is_empty())
        .collect();
    let selected = !ranges.is_empty();
    if !selected {
        ranges.push(Range::new(0, rope.len_chars()));
    }
    let rewritten: Vec<(usize, usize, String)> = ranges
        .iter()
        .map(|r| {
            let text = rope.slice(r.from()..r.to()).to_string();
            (r.from(), r.to(), f(&text))
        })
        .collect();
    apply_blocks(buf, &rewritten, selected)
}

/// Makes every CR, LF and CRLF line break `brk`; the cursor keeps its line and column.
pub fn convert_line_breaks(buf: &mut Buffer, brk: &str) -> bool {
    let rope = buf.text();
    let changes: Vec<Change> = (0..rope.len_lines().saturating_sub(1))
        .filter_map(|l| {
            let old = line_break_str(rope, l);
            (matches!(old.as_str(), "\n" | "\r\n" | "\r") && old != brk)
                .then(|| Change::new(line_content_end(rope, l), line_start(rope, l + 1), brk))
        })
        .collect();
    if changes.is_empty() {
        return false;
    }
    apply_by_pos(buf, changes, |_, anchor, head| (anchor, head));
    true
}

#[cfg(test)]
mod text_command_tests {
    use super::*;

    /// Timing of a whole-file /sort on 1,000,000 lines (60 MB), by phase. Run with
    /// `cargo test --release -p vr-core -- --ignored sort_timing --nocapture`.
    #[test]
    #[ignore]
    fn sort_timing_on_a_million_lines() {
        let mut text = String::new();
        for i in 0..1_000_000 {
            text.push_str(&format!(
                "line {:07} some text here to pad the line out ab\n",
                999_999 - i
            ));
        }
        let mut buf = Buffer::from_str(&text);
        let t = std::time::Instant::now();
        assert!(rewrite_lines(&mut buf, |mut l| {
            l.sort();
            l
        }));
        eprintln!("rewrite_lines sort: {:?}", t.elapsed());
    }

    fn buf_at(text: &str, line: usize, col: usize) -> Buffer {
        let mut buf = Buffer::from_str(text);
        let at = pos_to_char(buf.text(), Pos { line, col });
        buf.set_selection(Selection::point(at));
        buf
    }

    fn head_pos(buf: &Buffer) -> Pos {
        char_to_pos(buf.text(), buf.selection().primary().head)
    }

    fn assert_one_undo(buf: &mut Buffer, original: &str) {
        assert!(buf.undo());
        assert_eq!(buf.text().to_string(), original);
        assert!(!buf.history().can_undo());
    }

    fn sel(buf: &mut Buffer, anchor: usize, head: usize) {
        buf.set_selection(Selection::single(anchor, head));
    }

    #[test]
    fn rewrite_lines_sorts_the_whole_document_keeping_the_final_break() {
        let mut buf = buf_at("b\nc\na\n", 1, 1);
        assert!(rewrite_lines(&mut buf, |mut v| {
            v.sort();
            v
        }));
        assert_eq!(buf.text().to_string(), "a\nb\nc\n");
        assert_eq!(head_pos(&buf), Pos { line: 1, col: 1 });
        assert_one_undo(&mut buf, "b\nc\na\n");
        // Already sorted: nothing changes, no undo step.
        let mut buf = Buffer::from_str("a\nb");
        assert!(!rewrite_lines(&mut buf, |mut v| {
            v.sort();
            v
        }));
        assert!(!buf.history().can_undo());
    }

    #[test]
    fn rewrite_lines_acts_on_the_selected_lines_and_keeps_crlf() {
        let text = "z\r\nc\r\nb\r\na\r\n";
        let mut buf = Buffer::from_str(text);
        // From inside line 1 to the start of line 3: lines 1 and 2.
        sel(&mut buf, 4, 9);
        assert!(rewrite_lines(&mut buf, |mut v| {
            v.sort();
            v
        }));
        assert_eq!(buf.text().to_string(), "z\r\nb\r\nc\r\na\r\n");
        let r = buf.selection().primary();
        assert_eq!(
            (r.from(), r.to()),
            (3, 7),
            "the rewritten lines are selected"
        );
        // Fewer lines (unique): the block's own breaks, the last line keeps none.
        let mut buf = Buffer::from_str("a\nb\na");
        assert!(rewrite_lines(&mut buf, |v| {
            let mut out: Vec<String> = Vec::new();
            for l in v {
                if !out.contains(&l) {
                    out.push(l);
                }
            }
            out
        }));
        assert_eq!(buf.text().to_string(), "a\nb");
    }

    #[test]
    fn rewrite_selection_changes_the_selected_text_or_everything() {
        let mut buf = Buffer::from_str("abc def");
        sel(&mut buf, 4, 7);
        assert!(rewrite_selection(&mut buf, str::to_uppercase));
        assert_eq!(buf.text().to_string(), "abc DEF");
        let r = buf.selection().primary();
        assert_eq!((r.from(), r.to()), (4, 7));
        let mut buf = buf_at("ab\nßc", 1, 1);
        assert!(rewrite_selection(&mut buf, str::to_uppercase));
        assert_eq!(buf.text().to_string(), "AB\nSSC");
        assert_one_undo(&mut buf, "ab\nßc");
        assert!(!rewrite_selection(
            &mut Buffer::from_str("x"),
            str::to_lowercase
        ));
    }

    #[test]
    fn convert_line_breaks_makes_every_break_the_same() {
        let mut buf = buf_at("a\nb\r\nc\rd", 2, 1);
        assert!(convert_line_breaks(&mut buf, "\r\n"));
        assert_eq!(buf.text().to_string(), "a\r\nb\r\nc\r\nd");
        assert_eq!(head_pos(&buf), Pos { line: 2, col: 1 });
        assert_one_undo(&mut buf, "a\nb\r\nc\rd");
        let mut buf = Buffer::from_str("a\nb\n");
        assert!(!convert_line_breaks(&mut buf, "\n"));
        assert!(convert_line_breaks(&mut buf, "\r\n"));
        assert_eq!(buf.text().to_string(), "a\r\nb\r\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::{Pos, Range, Selection};
    use crate::text::{BufferOptions, char_to_pos, pos_to_char};

    fn buf_at(text: &str, line: usize, col: usize) -> Buffer {
        let mut buf = Buffer::from_str(text);
        let at = pos_to_char(buf.text(), Pos { line, col });
        buf.set_selection(Selection::point(at));
        buf
    }

    fn head_pos(buf: &Buffer) -> Pos {
        char_to_pos(buf.text(), buf.selection().primary().head)
    }

    fn assert_one_undo(buf: &mut Buffer, original: &str) {
        assert!(buf.undo());
        assert_eq!(buf.text().to_string(), original);
        assert!(!buf.history().can_undo(), "op must be a single undo step");
    }

    #[test]
    fn delete_line_last_line_no_newline() {
        let mut buf = buf_at("a\nb\nc", 2, 1);
        delete_line(&mut buf);
        assert_eq!(buf.text().to_string(), "a\nb");
        assert_eq!(head_pos(&buf), Pos { line: 1, col: 1 });
        assert_one_undo(&mut buf, "a\nb\nc");

        let mut buf = buf_at("one\ntwo\nthree", 1, 2);
        delete_line(&mut buf);
        assert_eq!(buf.text().to_string(), "one\nthree");
        assert_eq!(head_pos(&buf), Pos { line: 1, col: 2 });

        let mut buf = buf_at("only", 0, 2);
        delete_line(&mut buf);
        assert_eq!(buf.text().to_string(), "");
        assert_eq!(buf.selection(), &Selection::point(0));

        // A selection spanning lines deletes every touched line; a selection ending at
        // column 0 of the next line does not touch that line.
        let mut buf = Buffer::from_str("a\r\nb\r\nc\r\nd");
        buf.set_selection(Selection::single(3, 6));
        delete_line(&mut buf);
        assert_eq!(buf.text().to_string(), "a\r\nc\r\nd");
    }

    #[test]
    fn duplicate_line() {
        let mut buf = buf_at("ab\ncd", 0, 1);
        super::duplicate_line(&mut buf);
        assert_eq!(buf.text().to_string(), "ab\nab\ncd");
        assert_eq!(
            head_pos(&buf),
            Pos { line: 1, col: 1 },
            "cursor moves onto the copy"
        );
        assert_one_undo(&mut buf, "ab\ncd");

        let mut buf = buf_at("ab\ncd", 1, 2);
        super::duplicate_line(&mut buf);
        assert_eq!(buf.text().to_string(), "ab\ncd\ncd");
        assert_eq!(head_pos(&buf), Pos { line: 2, col: 2 });

        let mut buf = buf_at("x\r\ny", 1, 0);
        super::duplicate_line(&mut buf);
        assert_eq!(
            buf.text().to_string(),
            "x\r\ny\r\ny",
            "uses the file's line ending"
        );

        // Multi-line selection duplicates the block and keeps the selection on the copy.
        let mut buf = Buffer::from_str("a\nb\nc");
        buf.set_selection(Selection::single(0, 3));
        super::duplicate_line(&mut buf);
        assert_eq!(buf.text().to_string(), "a\nb\na\nb\nc");
        assert_eq!(buf.selection(), &Selection::single(4, 7));
    }

    #[test]
    fn move_line_up_down_bounds() {
        let mut buf = buf_at("a\nb\nc", 0, 0);
        let v = buf.version();
        move_line_up(&mut buf);
        assert_eq!(buf.text().to_string(), "a\nb\nc");
        assert_eq!(buf.version(), v, "top line cannot move up");
        assert!(!buf.history().can_undo());

        let mut buf = buf_at("a\nb\nc", 2, 1);
        move_line_down(&mut buf);
        assert_eq!(
            buf.text().to_string(),
            "a\nb\nc",
            "bottom line cannot move down"
        );

        move_line_up(&mut buf);
        assert_eq!(
            buf.text().to_string(),
            "a\nc\nb",
            "trailing-newline-less last line moves up"
        );
        assert_eq!(head_pos(&buf), Pos { line: 1, col: 1 });
        assert_one_undo(&mut buf, "a\nb\nc");

        let mut buf = buf_at("one\ntwo\nthree\n", 0, 2);
        move_line_down(&mut buf);
        assert_eq!(buf.text().to_string(), "two\none\nthree\n");
        assert_eq!(head_pos(&buf), Pos { line: 1, col: 2 });
        move_line_down(&mut buf);
        assert_eq!(buf.text().to_string(), "two\nthree\none\n");

        // A two-line block moves as one, CRLF kept.
        let mut buf = Buffer::from_str("a\r\nb\r\nc\r\nd");
        buf.set_selection(Selection::single(3, 7));
        move_line_up(&mut buf);
        assert_eq!(buf.text().to_string(), "b\r\nc\r\na\r\nd");
        assert_eq!(buf.selection(), &Selection::single(0, 4));
    }

    #[test]
    fn indent_outdent_selection() {
        let mut buf = Buffer::from_str("a\n\nb\nc");
        buf.set_options(BufferOptions {
            tab_width: 4,
            indent_unit: "  ".into(),
        });
        buf.set_selection(Selection::single(1, 4));
        indent(&mut buf);
        assert_eq!(
            buf.text().to_string(),
            "  a\n\n  b\nc",
            "blank lines in a block are skipped"
        );
        assert_one_undo(&mut buf, "a\n\nb\nc");
        assert!(buf.redo());
        outdent(&mut buf);
        assert_eq!(buf.text().to_string(), "a\n\nb\nc");

        let mut buf = Buffer::from_str("\tx\n   y\n z\nw");
        buf.set_selection(Selection::single(0, 12));
        outdent(&mut buf);
        assert_eq!(
            buf.text().to_string(),
            "x\ny\nz\nw",
            "one tab or up to one indent of spaces"
        );

        // Default options indent with four spaces; a single cursor indents its line.
        let mut buf = buf_at("ab\ncd", 1, 1);
        indent(&mut buf);
        assert_eq!(buf.text().to_string(), "ab\n    cd");
        assert_eq!(head_pos(&buf), Pos { line: 1, col: 5 });
        outdent(&mut buf);
        outdent(&mut buf);
        assert_eq!(buf.text().to_string(), "ab\ncd");
    }

    #[test]
    fn indent_keeps_selection_start_at_column_0() {
        // Lines 0..1 fully selected from column 0 (forward and backward): the start stays
        // at column 0 so the selection still covers the whole indented lines.
        for (anchor, head) in [(0, 3), (3, 0)] {
            let mut buf = Buffer::from_str("a\nb\nc");
            buf.set_selection(Selection::single(anchor, head));
            indent(&mut buf);
            assert_eq!(buf.text().to_string(), "    a\n    b\nc");
            let r = buf.selection().primary();
            assert_eq!((r.from(), r.to()), (0, 11));
            assert_eq!(r.head < r.anchor, head < anchor, "direction kept");
            indent(&mut buf);
            assert_eq!(buf.text().to_string(), "        a\n        b\nc");
            let r = buf.selection().primary();
            assert_eq!((r.from(), r.to()), (0, 19));
        }
        // A selection ending at column 0 of the next line leaves that line and the end alone.
        let mut buf = Buffer::from_str("a\nb\nc");
        buf.set_selection(Selection::single(0, 4));
        indent(&mut buf);
        assert_eq!(buf.text().to_string(), "    a\n    b\nc");
        assert_eq!(buf.selection(), &Selection::single(0, 12));
    }

    #[test]
    fn toggle_comment_php() {
        let original = "<?php\n    $a = 1;\n\n        $b = 2;\n";
        let mut buf = Buffer::from_str(original);
        buf.set_selection(Selection::single(6, 33));
        toggle_comment(&mut buf, "//");
        assert_eq!(
            buf.text().to_string(),
            "<?php\n    // $a = 1;\n\n    //     $b = 2;\n",
            "token at the smallest indent, blank lines untouched"
        );
        toggle_comment(&mut buf, "//");
        assert_eq!(
            buf.text().to_string(),
            original,
            "all commented → uncommented"
        );
        assert!(buf.undo());
        assert!(buf.undo());
        assert_eq!(buf.text().to_string(), original);

        // Mixed lines get commented; "//x" without a space is still uncommented.
        let mut buf = Buffer::from_str("//x\ny");
        buf.set_selection(Selection::single(0, 5));
        toggle_comment(&mut buf, "//");
        assert_eq!(buf.text().to_string(), "// //x\n// y");
        let mut buf = buf_at("  //x", 0, 4);
        toggle_comment(&mut buf, "//");
        assert_eq!(buf.text().to_string(), "  x");
        assert_eq!(head_pos(&buf), Pos { line: 0, col: 2 });

        // Several cursors on separate lines each toggle their line.
        let mut buf = Buffer::from_str("a\nb\nc");
        buf.set_selection(Selection::new(vec![Range::point(0), Range::point(4)], 0));
        toggle_comment(&mut buf, "#");
        assert_eq!(buf.text().to_string(), "# a\nb\n# c");
        assert_one_undo(&mut buf, "a\nb\nc");
    }
}
