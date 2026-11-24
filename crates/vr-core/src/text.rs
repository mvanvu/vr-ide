//! The editable buffer (rope + selection + history) and grapheme / display-width helpers.

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use ropey::str_utils::byte_to_char_idx;
use ropey::{Rope, RopeSlice};
use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete, UnicodeSegmentation};
use unicode_width::UnicodeWidthStr;

use crate::history::{Clock, EditKind, History, Step, SystemClock};
use crate::selection::{Pos, Range, Selection};
use crate::transaction::{Change, Transaction, apply_changes, invert_changes, normalize};

/// Per-buffer editing settings (filled from vr-config settings by the app).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferOptions {
    pub tab_width: usize,
    /// Text inserted by `indent` (spaces or a tab).
    pub indent_unit: String,
}

impl Default for BufferOptions {
    fn default() -> Self {
        Self {
            tab_width: 4,
            indent_unit: "    ".to_string(),
        }
    }
}

#[derive(Clone)]
pub struct Buffer {
    text: Rope,
    selection: Selection,
    history: History,
    version: u64,
    options: BufferOptions,
    clock: Arc<dyn Clock>,
    /// Display columns vertical motion aims for, one per range; cleared by any other move.
    goal_cols: Option<Vec<usize>>,
}

impl fmt::Debug for Buffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Buffer")
            .field("len_chars", &self.text.len_chars())
            .field("selection", &self.selection)
            .field("version", &self.version)
            .field("revision", &self.revision())
            .finish_non_exhaustive()
    }
}

impl Default for Buffer {
    fn default() -> Self {
        Self::from_str("")
    }
}

impl Buffer {
    // Infallible, unlike `FromStr::from_str`, so it stays an inherent constructor.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        Self {
            text: Rope::from_str(s),
            selection: Selection::point(0),
            history: History::default(),
            version: 0,
            options: BufferOptions::default(),
            clock: Arc::new(SystemClock),
            goal_cols: None,
        }
    }

    pub fn text(&self) -> &Rope {
        &self.text
    }

    /// Increments on every text change, including undo and redo.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Returns to an earlier value when undo/redo brings the text back to that state.
    pub fn revision(&self) -> u64 {
        self.history.revision()
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// Moving the cursor elsewhere ends the current typing group.
    pub fn set_selection(&mut self, s: Selection) {
        self.set_selection_with_goal(s, None);
    }

    pub(crate) fn goal_cols(&self) -> Option<&[usize]> {
        self.goal_cols.as_deref()
    }

    pub(crate) fn set_selection_with_goal(&mut self, s: Selection, goal: Option<Vec<usize>>) {
        let s = s.clamp(self.text.len_chars());
        if s != self.selection {
            self.history.break_group();
        }
        self.selection = s;
        self.goal_cols = goal;
    }

    pub fn set_clock(&mut self, clock: Arc<dyn Clock>) {
        self.clock = clock;
    }

    pub fn options(&self) -> &BufferOptions {
        &self.options
    }

    pub fn set_options(&mut self, o: BufferOptions) {
        self.options = o;
    }

    /// Ends the current typing group (e.g. on save or focus change).
    pub fn break_undo_group(&mut self) {
        self.history.break_group();
    }

    /// Applies all changes as one undo group.
    pub fn apply(&mut self, tx: Transaction) {
        self.commit(tx, EditKind::Other);
    }

    pub(crate) fn commit(&mut self, tx: Transaction, kind: EditKind) {
        let changes = normalize(tx.changes, self.text.len_chars());
        if changes.is_empty() {
            if let Some(sel) = tx.selection_after {
                self.set_selection(sel);
            }
            return;
        }
        let inverse = invert_changes(&self.text, &changes);
        apply_changes(&mut self.text, &changes);
        let len = self.text.len_chars();
        let after = match tx.selection_after {
            Some(sel) => sel.clamp(len),
            None => self.selection.map(&changes).clamp(len),
        };
        let before = std::mem::replace(&mut self.selection, after);
        let now = self.clock.now();
        let step = Step {
            forward: changes,
            inverse,
        };
        self.history
            .record(step, kind, &before, &self.selection, now);
        self.goal_cols = None;
        self.version += 1;
    }

    pub fn undo(&mut self) -> bool {
        let Some(group) = self.history.undo() else {
            return false;
        };
        for step in group.steps.iter().rev() {
            apply_changes(&mut self.text, &step.inverse);
        }
        self.selection = group.selection_before.clamp(self.text.len_chars());
        self.goal_cols = None;
        self.version += 1;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(group) = self.history.redo() else {
            return false;
        };
        for step in &group.steps {
            apply_changes(&mut self.text, &step.forward);
        }
        self.selection = group.selection_after.clamp(self.text.len_chars());
        self.goal_cols = None;
        self.version += 1;
        true
    }

    /// Replaces every range with `s` and leaves a cursor after each insertion.
    pub fn insert_text(&mut self, s: &str) {
        let ins = s.chars().count();
        let mut changes = Vec::with_capacity(self.selection.len());
        let mut heads = Vec::with_capacity(self.selection.len());
        let (mut inserted, mut removed) = (0usize, 0usize);
        for r in self.selection.ranges() {
            changes.push(Change::new(r.from(), r.to(), s));
            heads.push(Range::point(r.from() - removed + inserted + ins));
            inserted += ins;
            removed += r.to() - r.from();
        }
        // Single typed graphemes coalesce into one undo group; pastes and newlines stand alone.
        let typed = !s.chars().any(is_line_break) && s.graphemes(true).count() == 1;
        let kind = if typed {
            EditKind::Insert
        } else {
            EditKind::Other
        };
        let sel = Selection::new(heads, self.selection.primary_index());
        self.commit(Transaction::new(changes).with_selection(sel), kind);
    }

    /// Deletes each selection, or the grapheme before each cursor.
    pub fn delete_backward(&mut self) {
        self.delete(EditKind::DeleteBackward, |rope, head| {
            (prev_grapheme_boundary(rope.slice(..), head), head)
        });
    }

    /// Deletes each selection, or the grapheme after each cursor.
    pub fn delete_forward(&mut self) {
        self.delete(EditKind::DeleteForward, |rope, head| {
            (head, next_grapheme_boundary(rope.slice(..), head))
        });
    }

    fn delete(&mut self, kind: EditKind, span: impl Fn(&Rope, usize) -> (usize, usize)) {
        let all_points = self.selection.ranges().iter().all(Range::is_empty);
        let mut changes: Vec<Change> = Vec::with_capacity(self.selection.len());
        for r in self.selection.ranges() {
            let (from, to) = if r.is_empty() {
                span(&self.text, r.head)
            } else {
                (r.from(), r.to())
            };
            // A cursor's grapheme span can reach into a neighbouring range; trim it to
            // what the earlier change has not already deleted.
            let from = changes.last().map_or(from, |prev| from.max(prev.to));
            if from < to {
                changes.push(Change::new(from, to, ""));
            }
        }
        let kind = if all_points { kind } else { EditKind::Other };
        self.commit(Transaction::new(changes), kind);
    }
}

/// Line breaks: LF, CR and CRLF only (D31; ropey built with `cr_lines`, without `unicode_lines`).
pub fn is_line_break(c: char) -> bool {
    matches!(c, '\n' | '\r')
}

/// Screen column of `char_idx` within `line` (a line slice; offsets relative to its start).
/// Tabs advance to the next multiple of `tab_width`; wide chars take two columns and
/// combining marks none, because width is measured per grapheme cluster.
pub fn display_col(line: RopeSlice, char_idx: usize, tab_width: usize) -> usize {
    let end = char_idx.min(line.len_chars());
    let prefix: Cow<str> = line.slice(..end).into();
    prefix
        .graphemes(true)
        .fold(0, |col, g| col + grapheme_width(g, col, tab_width))
}

fn grapheme_width(g: &str, col: usize, tab_width: usize) -> usize {
    if g == "\t" {
        let tw = tab_width.max(1);
        tw - col % tw
    } else if g.chars().next().is_some_and(is_line_break) {
        0
    } else {
        g.width()
    }
}

/// Char offset (relative to `line`) of the grapheme where display column `target` falls.
/// A target inside a wide char lands before it; a target past the end lands at the content end.
pub fn char_at_display_col(line: RopeSlice, target: usize, tab_width: usize) -> usize {
    let content: Cow<str> = line.into();
    let mut col = 0;
    let mut chars = 0;
    for g in content.graphemes(true) {
        if g.chars().next().is_some_and(is_line_break) {
            break;
        }
        let w = grapheme_width(g, col, tab_width);
        if col + w > target {
            break;
        }
        col += w;
        chars += g.chars().count();
    }
    chars
}

/// Char offset just past the content of `line`, before its line break.
pub fn line_content_end(rope: &Rope, line: usize) -> usize {
    let line = line.min(rope.len_lines().saturating_sub(1));
    let start = rope.line_to_char(line);
    let slice = rope.line(line);
    let mut n = slice.len_chars();
    if n > 0 && is_line_break(slice.char(n - 1)) {
        n -= 1;
        if n > 0 && slice.char(n) == '\n' && slice.char(n - 1) == '\r' {
            n -= 1;
        }
    }
    start + n
}

/// The break ending `line` ("" for the last line).
pub fn line_break_str(rope: &Rope, line: usize) -> String {
    let end = if line + 1 < rope.len_lines() {
        rope.line_to_char(line + 1)
    } else {
        rope.len_chars()
    };
    rope.slice(line_content_end(rope, line)..end).to_string()
}

/// The break new lines should use: the one ending the first line, else "\n".
pub fn default_line_break(rope: &Rope) -> String {
    let brk = line_break_str(rope, 0);
    if brk.is_empty() {
        "\n".to_string()
    } else {
        brk
    }
}

pub fn char_to_pos(rope: &Rope, char_idx: usize) -> Pos {
    let idx = char_idx.min(rope.len_chars());
    let line = rope.char_to_line(idx);
    let start = rope.line_to_char(line);
    let prefix: Cow<str> = rope.slice(start..idx).into();
    Pos {
        line,
        col: prefix.graphemes(true).count(),
    }
}

/// Clamps the line to the document and the column to the line's content.
pub fn pos_to_char(rope: &Rope, pos: Pos) -> usize {
    let line = pos.line.min(rope.len_lines().saturating_sub(1));
    let start = rope.line_to_char(line);
    let content: Cow<str> = rope.slice(start..line_content_end(rope, line)).into();
    start
        + content
            .graphemes(true)
            .take(pos.col)
            .map(|g| g.chars().count())
            .sum::<usize>()
}

/// Start of the grapheme cluster ending at or containing `char_idx - 1`.
pub fn prev_grapheme_boundary(slice: RopeSlice, char_idx: usize) -> usize {
    let char_idx = char_idx.min(slice.len_chars());
    if char_idx == 0 {
        return 0;
    }
    let byte_idx = slice.char_to_byte(char_idx);
    let (mut chunk, mut chunk_byte, mut chunk_char, _) = slice.chunk_at_byte(byte_idx);
    let mut gc = GraphemeCursor::new(byte_idx, slice.len_bytes(), true);
    loop {
        match gc.prev_boundary(chunk, chunk_byte) {
            Ok(None) => return 0,
            Ok(Some(n)) => return chunk_char + byte_to_char_idx(chunk, n - chunk_byte),
            Err(GraphemeIncomplete::PrevChunk) => {
                (chunk, chunk_byte, chunk_char, _) = slice.chunk_at_byte(chunk_byte - 1);
            }
            Err(GraphemeIncomplete::PreContext(n)) => {
                let ctx = slice.chunk_at_byte(n - 1).0;
                gc.provide_context(ctx, n - ctx.len());
            }
            Err(_) => return char_idx - 1,
        }
    }
}

/// End of the grapheme cluster starting at `char_idx`.
pub fn next_grapheme_boundary(slice: RopeSlice, char_idx: usize) -> usize {
    let len = slice.len_chars();
    if char_idx >= len {
        return len;
    }
    let byte_idx = slice.char_to_byte(char_idx);
    let (mut chunk, mut chunk_byte, mut chunk_char, _) = slice.chunk_at_byte(byte_idx);
    let mut gc = GraphemeCursor::new(byte_idx, slice.len_bytes(), true);
    loop {
        match gc.next_boundary(chunk, chunk_byte) {
            Ok(None) => return len,
            Ok(Some(n)) => return chunk_char + byte_to_char_idx(chunk, n - chunk_byte),
            Err(GraphemeIncomplete::NextChunk) => {
                chunk_byte += chunk.len();
                let (c, _, cc, _) = slice.chunk_at_byte(chunk_byte);
                chunk = c;
                chunk_char = cc;
            }
            Err(GraphemeIncomplete::PreContext(n)) => {
                let ctx = slice.chunk_at_byte(n - 1).0;
                gc.provide_context(ctx, n - ctx.len());
            }
            Err(_) => return char_idx + 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::motion;

    #[test]
    fn insert_and_delete_ascii() {
        let mut buf = Buffer::from_str("hello");
        buf.set_selection(Selection::point(5));
        buf.insert_text(" world");
        assert_eq!(buf.text().to_string(), "hello world");
        assert_eq!(buf.selection(), &Selection::point(11));
        buf.delete_backward();
        assert_eq!(buf.text().to_string(), "hello worl");
        buf.set_selection(Selection::point(0));
        buf.delete_forward();
        assert_eq!(buf.text().to_string(), "ello worl");
        assert_eq!(buf.selection(), &Selection::point(0));
        // Edges are no-ops and do not bump the version.
        let v = buf.version();
        buf.delete_backward();
        buf.set_selection(Selection::point(9));
        buf.delete_forward();
        assert_eq!(buf.text().to_string(), "ello worl");
        assert_eq!(buf.version(), v);
        // Typing over a selection replaces it; deleting a selection removes it.
        buf.set_selection(Selection::single(0, 4));
        buf.insert_text("E");
        assert_eq!(buf.text().to_string(), "E worl");
        buf.set_selection(Selection::single(6, 2));
        buf.delete_backward();
        assert_eq!(buf.text().to_string(), "E ");
        assert_eq!(buf.selection(), &Selection::point(2));
    }

    #[test]
    fn multi_cursor_insert_and_delete() {
        let mut buf = Buffer::from_str("ab\ncd");
        buf.set_selection(Selection::new(
            vec![crate::Range::point(1), crate::Range::point(4)],
            0,
        ));
        buf.insert_text("ế");
        assert_eq!(buf.text().to_string(), "aếb\ncếd");
        assert_eq!(
            buf.selection().ranges(),
            &[crate::Range::point(2), crate::Range::point(6)]
        );
        buf.delete_backward();
        assert_eq!(buf.text().to_string(), "ab\ncd");
        assert!(buf.undo());
        assert!(buf.undo());
        assert_eq!(buf.text().to_string(), "ab\ncd");
    }

    #[test]
    fn delete_with_cursor_touching_a_selection_does_not_overlap() {
        // The cursor's backspace span would reach into the selection ending at it.
        let mut buf = Buffer::from_str("abcdef");
        buf.set_selection(Selection::new(
            vec![crate::Range::new(1, 3), crate::Range::point(3)],
            0,
        ));
        buf.delete_backward();
        assert_eq!(buf.text().to_string(), "adef");
        // Same for delete forward with a cursor right before a selection.
        let mut buf = Buffer::from_str("abcdef");
        buf.set_selection(Selection::new(
            vec![crate::Range::point(2), crate::Range::new(3, 5)],
            0,
        ));
        buf.delete_forward();
        assert_eq!(buf.text().to_string(), "abf");
    }

    #[test]
    fn display_col_wide() {
        let rope = Rope::from_str("中a\n\ta\nab\tc\nTie\u{0302}\u{0301}ng");
        assert_eq!(
            display_col(rope.line(0), 1, 4),
            2,
            "CJK is two columns wide"
        );
        assert_eq!(display_col(rope.line(0), 2, 4), 3);
        assert_eq!(
            display_col(rope.line(1), 1, 4),
            4,
            "tab advances to the next stop"
        );
        assert_eq!(display_col(rope.line(2), 3, 4), 4);
        assert_eq!(display_col(rope.line(2), 4, 4), 5);
        assert_eq!(display_col(rope.line(2), 3, 8), 8);
        assert_eq!(
            display_col(rope.line(3), 5, 4),
            3,
            "combining marks take no column"
        );
        let emoji = Rope::from_str("a\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}b");
        assert_eq!(
            display_col(emoji.line(0), 6, 4),
            3,
            "ZWJ family is one wide cluster"
        );
        // Past the end clamps to the line width.
        assert_eq!(display_col(rope.line(0), 99, 4), 3);
    }

    #[test]
    fn pos_roundtrip_uses_grapheme_columns() {
        let rope = Rope::from_str("ab\r\nTie\u{0302}\u{0301}ng 中");
        assert_eq!(char_to_pos(&rope, 9), Pos { line: 1, col: 3 });
        assert_eq!(pos_to_char(&rope, Pos { line: 1, col: 3 }), 9);
        assert_eq!(
            pos_to_char(&rope, Pos { line: 0, col: 99 }),
            2,
            "clamped before the break"
        );
        assert_eq!(
            pos_to_char(&rope, Pos { line: 99, col: 0 }),
            4,
            "clamped to the last line"
        );
        assert_eq!(line_content_end(&rope, 0), 2);
        assert_eq!(line_content_end(&rope, 1), rope.len_chars());
    }

    #[test]
    fn crlf_text_treated_as_one_line_break() {
        let mut buf = Buffer::from_str("ab\r\ncd");
        assert_eq!(buf.text().len_lines(), 2);
        buf.set_selection(Selection::point(2));
        motion::move_right(&mut buf, false);
        assert_eq!(buf.selection(), &Selection::point(4), "CRLF is one step");
        motion::move_left(&mut buf, false);
        assert_eq!(buf.selection(), &Selection::point(2));
        motion::line_end(&mut buf, false);
        assert_eq!(buf.selection(), &Selection::point(2));
        assert_eq!(char_to_pos(buf.text(), 4), Pos { line: 1, col: 0 });
        buf.set_selection(Selection::point(4));
        buf.delete_backward();
        assert_eq!(
            buf.text().to_string(),
            "abcd",
            "backspace removes the whole CRLF"
        );
        buf.set_selection(Selection::point(2));
        buf.insert_text("\r\n");
        buf.set_selection(Selection::point(2));
        buf.delete_forward();
        assert_eq!(buf.text().to_string(), "abcd");
    }

    #[test]
    fn only_lf_crlf_cr_break_lines() {
        // Form feed, VT, NEL and U+2028/2029 stay inside the line (D31: matches git, grep -n).
        let rope = Rope::from_str("a\u{0C}b\u{0B}c\u{85}d\u{2028}e\u{2029}f\ng\r\nh\ri");
        assert_eq!(rope.len_lines(), 4);
        assert_eq!(line_content_end(&rope, 0), 11);
        assert!(!is_line_break('\u{0C}'));
        assert!(!is_line_break('\u{2028}'));
        assert!(is_line_break('\n') && is_line_break('\r'));
    }

    #[test]
    fn long_line_grapheme_boundaries_match_across_chunks() {
        // A single long line spans many rope chunks, so clusters straddle chunk edges.
        let unit = "e\u{0301}\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}中\r\n\u{1F1FB}\u{1F1F3}x";
        let s = unit.repeat(3000).replace("\r\n", "");
        let rope = Rope::from_str(&s);
        assert!(rope.chunks().count() > 10);
        let expected: Vec<usize> = s
            .graphemes(true)
            .scan(0, |end, g| {
                *end += g.chars().count();
                Some(*end)
            })
            .collect();
        let mut forward = Vec::new();
        let mut i = 0;
        while i < rope.len_chars() {
            i = next_grapheme_boundary(rope.slice(..), i);
            forward.push(i);
        }
        assert_eq!(forward, expected);
        let mut backward = Vec::new();
        let mut i = rope.len_chars();
        while i > 0 {
            backward.push(i);
            i = prev_grapheme_boundary(rope.slice(..), i);
        }
        backward.reverse();
        assert_eq!(backward, expected);
        let line = rope.line(0);
        assert_eq!(display_col(line, line.len_chars(), 4), 3000 * 8);
    }
}
