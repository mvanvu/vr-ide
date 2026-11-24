//! Line-oriented edit operations. Each one is a single `Transaction` (one undo step).
//! The lines an operation touches are every line a range covers; a range ending at column 0
//! of a line does not cover that line.

use ropey::Rope;

use crate::selection::{Pos, Range, Selection};
use crate::text::{
    Buffer, char_to_pos, default_line_break, line_break_str, line_content_end, pos_to_char,
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
/// empty lines are left alone so no trailing whitespace appears.
pub fn indent(buf: &mut Buffer) {
    let rope = buf.text();
    let unit = buf.options().indent_unit.clone();
    let mut changes = Vec::new();
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
        }
    }
    buf.apply(Transaction::new(changes));
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
