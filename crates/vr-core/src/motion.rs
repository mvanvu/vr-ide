//! Cursor motions. Every motion applies to all ranges; `extend` keeps the anchor.
//! Horizontal motions step over whole grapheme clusters; vertical motions keep the display column.

use ropey::Rope;

use crate::selection::{Range, Selection};
use crate::text::{
    Buffer, char_at_display_col, display_col, is_line_break, line_content_end,
    next_grapheme_boundary, prev_grapheme_boundary,
};

fn move_heads(buf: &mut Buffer, extend: bool, f: impl Fn(&Rope, Range) -> usize) {
    let rope = buf.text();
    let sel = buf.selection().transform(|r| {
        let head = f(rope, r);
        if extend {
            Range::new(r.anchor, head)
        } else {
            Range::point(head)
        }
    });
    buf.set_selection(sel);
}

/// Like `move_heads`, but a non-extending move over a selection just collapses it.
fn move_horizontal(
    buf: &mut Buffer,
    extend: bool,
    collapse_to_end: bool,
    f: fn(&Rope, usize) -> usize,
) {
    move_heads(buf, extend, |rope, r| match (extend, r.is_empty()) {
        (false, false) => {
            if collapse_to_end {
                r.to()
            } else {
                r.from()
            }
        }
        _ => f(rope, r.head),
    });
}

pub fn move_left(buf: &mut Buffer, extend: bool) {
    move_horizontal(buf, extend, false, |rope, h| {
        prev_grapheme_boundary(rope.slice(..), h)
    });
}

pub fn move_right(buf: &mut Buffer, extend: bool) {
    move_horizontal(buf, extend, true, |rope, h| {
        next_grapheme_boundary(rope.slice(..), h)
    });
}

pub fn move_up(buf: &mut Buffer, extend: bool) {
    move_vertical(buf, extend, false);
}

pub fn move_down(buf: &mut Buffer, extend: bool) {
    move_vertical(buf, extend, true);
}

fn head_display_col(rope: &Rope, head: usize, tab_width: usize) -> usize {
    let line = rope.char_to_line(head);
    display_col(rope.line(line), head - rope.line_to_char(line), tab_width)
}

fn move_vertical(buf: &mut Buffer, extend: bool, down: bool) {
    let rope = buf.text();
    let tab_width = buf.options().tab_width;
    let ranges = buf.selection().ranges();
    let mut goals: Vec<usize> = match buf.goal_cols() {
        Some(g) if g.len() == ranges.len() => g.to_vec(),
        _ => ranges
            .iter()
            .map(|r| head_display_col(rope, r.head, tab_width))
            .collect(),
    };
    let last_line = rope.len_lines().saturating_sub(1);
    let mut moved = Vec::with_capacity(ranges.len());
    for (r, goal) in ranges.iter().zip(goals.iter_mut()) {
        let line = rope.char_to_line(r.head);
        let head = match (down, line) {
            // Past the first/last line the cursor goes to the document edge, and that
            // column becomes the new goal.
            (false, 0) => {
                *goal = 0;
                0
            }
            (true, l) if l >= last_line => {
                let end = rope.len_chars();
                *goal = head_display_col(rope, end, tab_width);
                end
            }
            (_, l) => {
                let target = if down { l + 1 } else { l - 1 };
                rope.line_to_char(target) + char_at_display_col(rope.line(target), *goal, tab_width)
            }
        };
        moved.push(if extend {
            Range::new(r.anchor, head)
        } else {
            Range::point(head)
        });
    }
    let sel = Selection::new(moved, buf.selection().primary_index());
    buf.set_selection_with_goal(sel, Some(goals));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Space,
    LineBreak,
    Word,
    Punct,
}

/// Class of the grapheme starting at `idx`, judged by its base char so combining marks
/// (Vietnamese diacritics) stay part of their word.
fn class_at(rope: &Rope, idx: usize) -> CharClass {
    let c = rope.char(idx);
    if is_line_break(c) {
        CharClass::LineBreak
    } else if c.is_whitespace() {
        CharClass::Space
    } else if c.is_alphanumeric() || c == '_' {
        CharClass::Word
    } else {
        CharClass::Punct
    }
}

fn word_right_of(rope: &Rope, head: usize) -> usize {
    let slice = rope.slice(..);
    let len = rope.len_chars();
    if head >= len {
        return len;
    }
    if class_at(rope, head) == CharClass::LineBreak {
        return next_grapheme_boundary(slice, head);
    }
    let mut i = head;
    while i < len && class_at(rope, i) == CharClass::Space {
        i = next_grapheme_boundary(slice, i);
    }
    if i < len {
        let kind = class_at(rope, i);
        if matches!(kind, CharClass::Word | CharClass::Punct) {
            while i < len && class_at(rope, i) == kind {
                i = next_grapheme_boundary(slice, i);
            }
        }
    }
    i
}

fn word_left_of(rope: &Rope, head: usize) -> usize {
    let slice = rope.slice(..);
    let class_before = |i: usize| class_at(rope, prev_grapheme_boundary(slice, i));
    if head == 0 {
        return 0;
    }
    if class_before(head) == CharClass::LineBreak {
        return prev_grapheme_boundary(slice, head);
    }
    let mut i = head;
    while i > 0 && class_before(i) == CharClass::Space {
        i = prev_grapheme_boundary(slice, i);
    }
    if i > 0 {
        let kind = class_before(i);
        if matches!(kind, CharClass::Word | CharClass::Punct) {
            while i > 0 && class_before(i) == kind {
                i = prev_grapheme_boundary(slice, i);
            }
        }
    }
    i
}

pub fn word_left(buf: &mut Buffer, extend: bool) {
    move_heads(buf, extend, |rope, r| word_left_of(rope, r.head));
}

pub fn word_right(buf: &mut Buffer, extend: bool) {
    move_heads(buf, extend, |rope, r| word_right_of(rope, r.head));
}

/// Smart home: first non-blank char of the line, or column 0 when already there.
pub fn line_start(buf: &mut Buffer, extend: bool) {
    move_heads(buf, extend, |rope, r| {
        let line = rope.char_to_line(r.head);
        let start = rope.line_to_char(line);
        let end = line_content_end(rope, line);
        let first_non_blank = (start..end)
            .find(|&i| !matches!(rope.char(i), ' ' | '\t'))
            .unwrap_or(end);
        if r.head == first_non_blank {
            start
        } else {
            first_non_blank
        }
    });
}

pub fn line_end(buf: &mut Buffer, extend: bool) {
    move_heads(buf, extend, |rope, r| {
        line_content_end(rope, rope.char_to_line(r.head))
    });
}

pub fn doc_start(buf: &mut Buffer, extend: bool) {
    move_heads(buf, extend, |_, _| 0);
}

pub fn doc_end(buf: &mut Buffer, extend: bool) {
    move_heads(buf, extend, |rope, _| rope.len_chars());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::selection::{Range, Selection};

    fn heads(buf: &mut Buffer, steps: usize, f: fn(&mut Buffer, bool)) -> Vec<usize> {
        (0..steps)
            .map(|_| {
                f(buf, false);
                buf.selection().primary().head
            })
            .collect()
    }

    #[test]
    fn grapheme_motion_combining() {
        // Decomposed Vietnamese: "ế" = e + U+0302 + U+0301, "ệ" = e + U+0323 + U+0302.
        let mut buf = Buffer::from_str("Tie\u{0302}\u{0301}ng Vie\u{0323}\u{0302}t");
        let right = heads(&mut buf, 11, move_right);
        assert_eq!(right, vec![1, 2, 5, 6, 7, 8, 9, 10, 13, 14, 14]);
        let left = heads(&mut buf, 4, move_left);
        assert_eq!(left, vec![13, 10, 9, 8]);
        // Backspace removes the whole cluster.
        buf.set_selection(Selection::point(5));
        buf.delete_backward();
        assert_eq!(buf.text().to_string(), "Ting Vie\u{0323}\u{0302}t");
        // Precomposed text steps one char per grapheme.
        let mut buf = Buffer::from_str("Tiếng");
        assert_eq!(heads(&mut buf, 5, move_right), vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn grapheme_motion_emoji_zwj() {
        let mut buf =
            Buffer::from_str("a\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}b\u{1F1FB}\u{1F1F3}");
        assert_eq!(heads(&mut buf, 4, move_right), vec![1, 6, 7, 9]);
        assert_eq!(heads(&mut buf, 3, move_left), vec![7, 6, 1]);
        buf.set_selection(Selection::point(6));
        buf.delete_backward();
        assert_eq!(buf.text().to_string(), "ab\u{1F1FB}\u{1F1F3}");
        buf.delete_forward();
        assert_eq!(buf.text().to_string(), "a\u{1F1FB}\u{1F1F3}");
        buf.delete_forward();
        assert_eq!(buf.text().to_string(), "a", "flag pair is one cluster");
    }

    #[test]
    fn extend_and_collapse() {
        let mut buf = Buffer::from_str("abcd");
        buf.set_selection(Selection::point(1));
        move_right(&mut buf, true);
        move_right(&mut buf, true);
        assert_eq!(buf.selection(), &Selection::single(1, 3));
        move_left(&mut buf, false);
        assert_eq!(
            buf.selection(),
            &Selection::point(1),
            "left collapses to the start"
        );
        buf.set_selection(Selection::single(3, 1));
        move_right(&mut buf, false);
        assert_eq!(
            buf.selection(),
            &Selection::point(3),
            "right collapses to the end"
        );
        doc_end(&mut buf, true);
        assert_eq!(buf.selection(), &Selection::single(3, 4));
        doc_start(&mut buf, false);
        assert_eq!(buf.selection(), &Selection::point(0));
    }

    #[test]
    fn vertical_motion_keeps_display_column() {
        // Line 1 has a wide char, so column 4 on line 0 lands after "中中" (4 cols).
        let mut buf = Buffer::from_str("abcdef\n中中x\nab\nabcdefg");
        buf.set_selection(Selection::point(4));
        move_down(&mut buf, false);
        assert_eq!(buf.selection(), &Selection::point(9));
        move_down(&mut buf, false);
        assert_eq!(
            buf.selection(),
            &Selection::point(13),
            "short line clamps to its end"
        );
        move_down(&mut buf, false);
        assert_eq!(
            buf.selection(),
            &Selection::point(18),
            "goal column survives the short line"
        );
        move_down(&mut buf, false);
        assert_eq!(
            buf.selection(),
            &Selection::point(21),
            "last line goes to its end"
        );
        move_up(&mut buf, false);
        move_up(&mut buf, false);
        move_up(&mut buf, false);
        // Column 7 on "abcdefg" was the goal after the end jump.
        assert_eq!(buf.selection(), &Selection::point(6));
        move_up(&mut buf, false);
        assert_eq!(
            buf.selection(),
            &Selection::point(0),
            "first line goes to doc start"
        );
        // A column inside a wide char lands before it.
        buf.set_selection(Selection::point(1));
        move_down(&mut buf, true);
        assert_eq!(buf.selection(), &Selection::single(1, 7));
    }

    #[test]
    fn line_start_smart_home_and_end() {
        let mut buf = Buffer::from_str("x\n    foo bar\r\ny");
        buf.set_selection(Selection::point(10));
        line_start(&mut buf, false);
        assert_eq!(
            buf.selection(),
            &Selection::point(6),
            "first non-blank first"
        );
        line_start(&mut buf, false);
        assert_eq!(buf.selection(), &Selection::point(2), "then column 0");
        line_start(&mut buf, false);
        assert_eq!(buf.selection(), &Selection::point(6));
        line_end(&mut buf, true);
        assert_eq!(buf.selection(), &Selection::single(6, 13));
    }

    #[test]
    fn word_motion_unicode() {
        // "xin chào" precomposed, "thế" decomposed (e + U+0302 + U+0301), CJK word, punctuation.
        let text = "xin chào the\u{0302}\u{0301} 中文, ok";
        let mut buf = Buffer::from_str(text);
        let right = heads(&mut buf, 6, word_right);
        assert_eq!(right, vec![3, 8, 14, 17, 18, 21]);
        let left = heads(&mut buf, 6, word_left);
        assert_eq!(left, vec![19, 17, 15, 9, 4, 0]);
        // Word motion crosses line breaks one break at a time.
        let mut buf = Buffer::from_str("ab  \r\n  cd");
        buf.set_selection(Selection::point(2));
        assert_eq!(heads(&mut buf, 3, word_right), vec![4, 6, 10]);
        assert_eq!(heads(&mut buf, 4, word_left), vec![8, 6, 4, 0]);
        // Multi-range: every range moves.
        let mut buf = Buffer::from_str("aa bb");
        buf.set_selection(Selection::new(vec![Range::point(0), Range::point(3)], 0));
        word_right(&mut buf, false);
        assert_eq!(
            buf.selection().ranges(),
            &[Range::point(2), Range::point(5)]
        );
    }
}
