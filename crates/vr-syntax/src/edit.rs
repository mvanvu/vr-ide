//! Text edits in the form tree-sitter needs (byte offsets + row/column points), built from the
//! editor's rope, and the position mapping that keeps older spans in step with newer text.

use std::ops::Range;

use ropey::Rope;

/// Row and byte column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, PartialOrd, Ord)]
pub struct Point {
    pub row: usize,
    pub column: usize,
}

/// One replacement: bytes `start_byte..old_end_byte` of the text before it became
/// `start_byte..new_end_byte`. A list of edits is applied in order, each in the coordinates of
/// the text the previous one produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Edit {
    pub start_byte: usize,
    pub old_end_byte: usize,
    pub new_end_byte: usize,
    pub start_point: Point,
    pub old_end_point: Point,
    pub new_end_point: Point,
}

impl Edit {
    /// The edit for replacing chars `from..to` of `before` with `insert` (a vr-core `Change`).
    pub fn from_chars(before: &Rope, from: usize, to: usize, insert: &str) -> Edit {
        let len = before.len_chars();
        let (from, to) = (from.min(len), to.min(len).max(from.min(len)));
        let start_byte = before.char_to_byte(from);
        let old_end_byte = before.char_to_byte(to);
        let p = points_at(before, &[start_byte, old_end_byte]);
        Edit {
            start_byte,
            old_end_byte,
            new_end_byte: start_byte + insert.len(),
            start_point: p[0],
            old_end_point: p[1],
            new_end_point: advance(p[0], insert.as_bytes()),
        }
    }

    /// Edits for a vr-core transaction's changes (char offsets in the text before it, sorted,
    /// non-overlapping). They come back last change first, so each one is valid in the text the
    /// previous one leaves.
    pub fn from_changes<'a>(
        before: &Rope,
        changes: impl IntoIterator<Item = (usize, usize, &'a str)>,
    ) -> Vec<Edit> {
        let len = before.len_chars();
        let changes: Vec<(usize, usize, &str)> = changes
            .into_iter()
            .map(|(from, to, insert)| {
                let (from, to) = (from.min(len), to.min(len).max(from.min(len)));
                (before.char_to_byte(from), before.char_to_byte(to), insert)
            })
            .collect();
        // Points of every change in one pass (one pass per change is quadratic in a replace-all).
        let bytes: Vec<usize> = changes.iter().flat_map(|&(s, e, _)| [s, e]).collect();
        let points = points_at(before, &bytes);
        let mut edits: Vec<Edit> = changes
            .iter()
            .zip(points.chunks(2))
            .map(|(&(start_byte, old_end_byte, insert), p)| Edit {
                start_byte,
                old_end_byte,
                new_end_byte: start_byte + insert.len(),
                start_point: p[0],
                old_end_point: p[1],
                new_end_point: advance(p[0], insert.as_bytes()),
            })
            .collect();
        edits.reverse();
        edits
    }

    /// The single edit that turns `old` into `new` (common prefix and suffix kept), or `None`
    /// when they are equal. For undo, redo and reloads, where the caller has no change list.
    /// Ropes that share chunks (a clone edited in place) compare in time proportional to the
    /// number of chunks, not bytes.
    pub fn diff(old: &Rope, new: &Rope) -> Option<Edit> {
        let (old_len, new_len) = (old.len_bytes(), new.len_bytes());
        let mut prefix = common_prefix(old, new);
        if prefix == old_len && prefix == new_len {
            return None;
        }
        let max_suffix = old_len.min(new_len) - prefix;
        let mut suffix = common_suffix(old, new, max_suffix);
        // Keep both ends on char boundaries so spans never split a code point.
        while prefix > 0 && is_continuation(old, prefix) {
            prefix -= 1;
        }
        while suffix > 0 && is_continuation(old, old_len - suffix) {
            suffix -= 1;
        }
        let (old_end_byte, new_end_byte) = (old_len - suffix, new_len - suffix);
        let p = points_at(old, &[prefix, old_end_byte]);
        Some(Edit {
            start_byte: prefix,
            old_end_byte,
            new_end_byte,
            start_point: p[0],
            old_end_point: p[1],
            // The new end is the start advanced over the inserted text only.
            new_end_point: new
                .byte_slice(prefix..new_end_byte)
                .chunks()
                .fold(p[0], |at, c| advance(at, c.as_bytes())),
        })
    }

    pub(crate) fn input_edit(&self) -> tree_sitter::InputEdit {
        let p = |p: Point| tree_sitter::Point::new(p.row, p.column);
        tree_sitter::InputEdit {
            start_byte: self.start_byte,
            old_end_byte: self.old_end_byte,
            new_end_byte: self.new_end_byte,
            start_position: p(self.start_point),
            old_end_position: p(self.old_end_point),
            new_end_position: p(self.new_end_point),
        }
    }
}

fn is_continuation(rope: &Rope, byte: usize) -> bool {
    byte < rope.len_bytes() && rope.byte(byte) & 0xC0 == 0x80
}

/// The point of `byte` as tree-sitter counts it: rows end at LF only. (The rope's own line index
/// also breaks at a lone CR; a point that disagreed with tree-sitter's would shift the columns
/// of reused nodes, which indentation-sensitive scanners read.)
///
/// The rope's line index cannot answer that, so LFs are counted: a linear pass (about 1 ms per
/// 10 MB, a few microseconds for usual files), small next to the reparse every edit causes.
pub(crate) fn point_at(rope: &Rope, byte: usize) -> Point {
    points_at(rope, &[byte])[0]
}

/// The points of ascending byte offsets, in one pass over the text before the last one.
pub(crate) fn points_at(rope: &Rope, bytes: &[usize]) -> Vec<Point> {
    let len = rope.len_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let (mut at, mut row, mut line_start) = (0, 0, 0);
    for &byte in bytes {
        let byte = byte.min(len).max(at);
        let mut pos = at;
        for chunk in rope.byte_slice(at..byte).chunks() {
            let b = chunk.as_bytes();
            let lfs = b.iter().filter(|&&c| c == b'\n').count();
            if lfs > 0 {
                row += lfs;
                line_start = pos + b.iter().rposition(|&c| c == b'\n').unwrap_or(0) + 1;
            }
            pos += b.len();
        }
        at = byte;
        out.push(Point {
            row,
            column: byte - line_start,
        });
    }
    out
}

fn advance(mut p: Point, inserted: &[u8]) -> Point {
    match inserted.iter().rposition(|&b| b == b'\n') {
        Some(last) => {
            p.row += inserted.iter().filter(|&&b| b == b'\n').count();
            p.column = inserted.len() - last - 1;
        }
        None => p.column += inserted.len(),
    }
    p
}

fn common_prefix(a: &Rope, b: &Rope) -> usize {
    let (mut ai, mut bi) = (a.chunks(), b.chunks());
    let (mut ac, mut bc): (&[u8], &[u8]) = (&[], &[]);
    let mut n = 0;
    loop {
        if ac.is_empty() {
            match ai.next() {
                Some(c) => ac = c.as_bytes(),
                None => return n,
            }
            continue;
        }
        if bc.is_empty() {
            match bi.next() {
                Some(c) => bc = c.as_bytes(),
                None => return n,
            }
            continue;
        }
        if std::ptr::eq(ac, bc) {
            n += ac.len();
            (ac, bc) = (&[], &[]);
            continue;
        }
        let k = ac.iter().zip(bc).take_while(|(x, y)| x == y).count();
        n += k;
        if k < ac.len().min(bc.len()) {
            return n;
        }
        (ac, bc) = (&ac[k..], &bc[k..]);
    }
}

fn common_suffix(a: &Rope, b: &Rope, max: usize) -> usize {
    let mut ai = a.chunks_at_byte(a.len_bytes()).0.reversed();
    let mut bi = b.chunks_at_byte(b.len_bytes()).0.reversed();
    let (mut ac, mut bc): (&[u8], &[u8]) = (&[], &[]);
    let mut n = 0;
    while n < max {
        if ac.is_empty() {
            match ai.next() {
                Some(c) => ac = c.as_bytes(),
                None => break,
            }
            continue;
        }
        if bc.is_empty() {
            match bi.next() {
                Some(c) => bc = c.as_bytes(),
                None => break,
            }
            continue;
        }
        if std::ptr::eq(ac, bc) {
            n += ac.len();
            (ac, bc) = (&[], &[]);
            continue;
        }
        let k = ac
            .iter()
            .rev()
            .zip(bc.iter().rev())
            .take_while(|(x, y)| x == y)
            .count();
        n += k;
        if k < ac.len().min(bc.len()) {
            break;
        }
        (ac, bc) = (&ac[..ac.len() - k], &bc[..bc.len() - k]);
    }
    n.min(max)
}

/// Maps a position of the text after `edits` back to the text before them. A position inside
/// replaced text goes to the start of the replaced range (`end = false`) or its end (`end = true`),
/// so a mapped range covers everything that may have produced the requested one.
pub(crate) fn map_back(mut pos: usize, edits: &[Edit], end: bool) -> usize {
    for e in edits.iter().rev() {
        pos = if pos <= e.start_byte && !(end && pos == e.start_byte && e.new_end_byte > pos) {
            pos
        } else if pos >= e.new_end_byte {
            pos - e.new_end_byte + e.old_end_byte
        } else if end {
            e.old_end_byte
        } else {
            e.start_byte
        };
    }
    pos
}

/// Carries spans computed for an older text through `edits`: the parts outside every edited
/// range move with the text, the parts inside one are dropped (that text has not been parsed).
pub(crate) fn map_forward<T: Copy>(
    mut spans: Vec<(Range<usize>, T)>,
    edits: &[Edit],
) -> Vec<(Range<usize>, T)> {
    for e in edits {
        let delta = e.new_end_byte as isize - e.old_end_byte as isize;
        let mut out = Vec::with_capacity(spans.len() + 1);
        for (r, v) in spans {
            if r.start < e.start_byte {
                let end = r.end.min(e.start_byte);
                out.push((r.start..end, v));
            }
            if r.end > e.old_end_byte {
                let start = r.start.max(e.old_end_byte);
                let shift = |x: usize| (x as isize + delta) as usize;
                out.push((shift(start)..shift(r.end), v));
            }
        }
        spans = out;
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(text: &str, e: &Edit, insert: &str) -> String {
        format!(
            "{}{}{}",
            &text[..e.start_byte],
            insert,
            &text[e.old_end_byte..]
        )
    }

    #[test]
    fn from_chars_gives_bytes_and_points() {
        let rope = Rope::from_str("ab\ncđe\nf");
        // Replace "đe\nf" (chars 4..8) with "x\nyz".
        let e = Edit::from_chars(&rope, 4, 8, "x\nyz");
        assert_eq!((e.start_byte, e.old_end_byte, e.new_end_byte), (4, 9, 8));
        assert_eq!(e.start_point, Point { row: 1, column: 1 });
        assert_eq!(e.old_end_point, Point { row: 2, column: 1 });
        assert_eq!(e.new_end_point, Point { row: 2, column: 2 });
        assert_eq!(apply("ab\ncđe\nf", &e, "x\nyz"), "ab\ncx\nyz");
    }

    #[test]
    fn points_count_rows_at_lf_only_as_tree_sitter_does() {
        // A lone CR is not a line break for tree-sitter; CRLF is one (at its LF).
        let text = "ab\rcd\r\nef\ng";
        let rope = Rope::from_str(text);
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&crate::grammar("json").unwrap().language())
            .unwrap();
        for (byte, want) in [
            (0, (0, 0)),
            (3, (0, 3)),
            (5, (0, 5)),
            (7, (1, 0)),
            (9, (1, 2)),
            (10, (2, 0)),
            (11, (2, 1)),
        ] {
            assert_eq!(
                point_at(&rope, byte),
                Point {
                    row: want.0,
                    column: want.1
                },
                "byte {byte}"
            );
        }
        // The same as tree-sitter's own point for the end of the text.
        let tree = parser.parse(text, None).unwrap();
        let end = tree.root_node().end_position();
        assert_eq!(
            point_at(&rope, text.len()),
            Point {
                row: end.row,
                column: end.column
            }
        );
        // The edit for typing after the lone CR carries that point.
        let e = Edit::from_chars(&rope, 3, 3, "x");
        assert_eq!(e.start_point, Point { row: 0, column: 3 });
    }

    #[test]
    fn from_changes_lists_last_change_first() {
        let rope = Rope::from_str("one two three");
        let edits = Edit::from_changes(&rope, [(0, 3, "1"), (8, 13, "3")]);
        assert_eq!(edits.len(), 2);
        let mut text = "one two three".to_string();
        text = apply(&text, &edits[0], "3");
        text = apply(&text, &edits[1], "1");
        assert_eq!(text, "1 two 3");
    }

    #[test]
    fn diff_finds_the_changed_middle() {
        let old = Rope::from_str("hello world");
        let new = Rope::from_str("hello brave world");
        let e = Edit::diff(&old, &new).unwrap();
        assert_eq!((e.start_byte, e.old_end_byte, e.new_end_byte), (6, 6, 12));
        assert!(Edit::diff(&old, &old.clone()).is_none());
        // Repeated text: prefix and suffix never overlap.
        let e = Edit::diff(&Rope::from_str("aaaa"), &Rope::from_str("aaa")).unwrap();
        assert_eq!((e.start_byte, e.old_end_byte, e.new_end_byte), (3, 4, 3));
    }

    #[test]
    fn diff_points_match_the_points_of_both_texts() {
        let old = Rope::from_str("a\r\nbc\rd\nef");
        for new in ["a\r\nbX\nY\rd\nef", "a\r\nef", "a\r\nbc\rd\nZZ\n\nef", "Q"] {
            let new = Rope::from_str(new);
            let e = Edit::diff(&old, &new).unwrap();
            assert_eq!(e.start_point, point_at(&old, e.start_byte), "{new}");
            assert_eq!(e.old_end_point, point_at(&old, e.old_end_byte), "{new}");
            assert_eq!(e.new_end_point, point_at(&new, e.new_end_byte), "{new}");
        }
    }

    #[test]
    fn diff_stays_on_char_boundaries() {
        // "é" (C3 A9) → "è" (C3 A8): the shared lead byte must not split the char.
        let e = Edit::diff(&Rope::from_str("aéb"), &Rope::from_str("aèb")).unwrap();
        assert_eq!((e.start_byte, e.old_end_byte, e.new_end_byte), (1, 3, 3));
    }

    #[test]
    fn diff_of_a_large_shared_rope_is_one_small_edit() {
        let mut big = String::new();
        for i in 0..100_000 {
            big.push_str(&format!("line {i}\n"));
        }
        let old = Rope::from_str(&big);
        let mut new = old.clone();
        new.insert(300_000, "XYZ");
        let e = Edit::diff(&old, &new).unwrap();
        let at = new.char_to_byte(300_000);
        assert_eq!(
            (e.start_byte, e.old_end_byte, e.new_end_byte),
            (at, at, at + 3)
        );
    }

    #[test]
    fn spans_move_with_the_text_and_drop_edited_parts() {
        // "let x = 1;" → insert "yy" at 4 (inside nothing) and replace "1" (8..9) with "22".
        let rope = Rope::from_str("let x = 1;");
        let edits = Edit::from_changes(&rope, [(4, 4, "yy"), (8, 9, "22")]);
        let spans = vec![(0..3, 'k'), (4..5, 'v'), (8..9, 'n'), (2..10, 'w')];
        let out = map_forward(spans, &edits);
        // After: "let yyx = 22;"
        assert_eq!(
            out,
            vec![
                (0..3, 'k'),
                (6..7, 'v'),
                (2..4, 'w'),
                (6..10, 'w'),
                (12..13, 'w')
            ]
        );
    }

    #[test]
    fn positions_map_back_to_the_text_before_the_edits() {
        let rope = Rope::from_str("let x = 1;");
        let edits = Edit::from_changes(&rope, [(4, 4, "yy"), (8, 9, "22")]);
        // In "let yyx = 22;": 5 is inside "yy", 11 inside "22", 13 the end.
        assert_eq!(map_back(5, &edits, false), 4);
        assert_eq!(map_back(5, &edits, true), 4);
        assert_eq!(map_back(11, &edits, false), 8);
        assert_eq!(map_back(11, &edits, true), 9);
        assert_eq!(map_back(13, &edits, true), 10);
        assert_eq!(map_back(2, &edits, false), 2);
    }
}
