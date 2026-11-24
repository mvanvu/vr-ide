//! Cursor positions, ranges and multi-range selections.

use crate::transaction::{Change, map_pos};

/// A line/column position. `col` counts grapheme clusters from the line start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

/// A selected span in char offsets. `head` is the cursor; `anchor` stays put while extending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub struct Range {
    pub anchor: usize,
    pub head: usize,
}

impl Range {
    pub fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head }
    }

    pub fn point(pos: usize) -> Self {
        Self::new(pos, pos)
    }

    pub fn from(&self) -> usize {
        self.anchor.min(self.head)
    }

    pub fn to(&self) -> usize {
        self.anchor.max(self.head)
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.head
    }

    pub fn map(&self, changes: &[Change]) -> Self {
        Self::new(map_pos(changes, self.anchor), map_pos(changes, self.head))
    }

    pub fn clamp(&self, len: usize) -> Self {
        Self::new(self.anchor.min(len), self.head.min(len))
    }
}

/// One or more ranges, kept sorted by start and non-overlapping, with one primary range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    ranges: Vec<Range>,
    primary: usize,
}

impl Default for Selection {
    fn default() -> Self {
        Self::point(0)
    }
}

impl Selection {
    /// Sorts the ranges and merges overlapping ones (and cursors at the same spot), so
    /// edits built from a selection never produce overlapping changes.
    pub fn new(ranges: Vec<Range>, primary: usize) -> Self {
        if ranges.is_empty() {
            return Self {
                ranges: vec![Range::point(0)],
                primary: 0,
            };
        }
        let primary = primary.min(ranges.len() - 1);
        let mut indexed: Vec<(usize, Range)> = ranges.into_iter().enumerate().collect();
        indexed.sort_by_key(|(_, r)| (r.from(), r.to()));

        let mut out: Vec<Range> = Vec::with_capacity(indexed.len());
        let mut new_primary = 0;
        for (idx, r) in indexed {
            match out.last_mut() {
                Some(last) if r.from() < last.to() || r.from() == last.from() => {
                    let (from, to) = (last.from(), last.to().max(r.to()));
                    *last = if last.head < last.anchor {
                        Range::new(to, from)
                    } else {
                        Range::new(from, to)
                    };
                }
                _ => out.push(r),
            }
            if idx == primary {
                new_primary = out.len() - 1;
            }
        }
        Self {
            ranges: out,
            primary: new_primary,
        }
    }

    pub fn single(anchor: usize, head: usize) -> Self {
        Self::new(vec![Range::new(anchor, head)], 0)
    }

    pub fn point(pos: usize) -> Self {
        Self::single(pos, pos)
    }

    pub fn ranges(&self) -> &[Range] {
        &self.ranges
    }

    pub fn primary(&self) -> Range {
        self.ranges[self.primary]
    }

    pub fn primary_index(&self) -> usize {
        self.primary
    }

    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    pub fn map(&self, changes: &[Change]) -> Self {
        self.transform(|r| r.map(changes))
    }

    pub fn clamp(&self, len: usize) -> Self {
        self.transform(|r| r.clamp(len))
    }

    /// Maps every range and re-normalizes, keeping the primary range's identity.
    pub fn transform(&self, f: impl FnMut(Range) -> Range) -> Self {
        Self::new(self.ranges.iter().copied().map(f).collect(), self.primary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_maps_through_changes() {
        // "hello world": replace "hello" with "hi", insert "!" at the end.
        let changes = vec![Change::new(0, 5, "hi"), Change::new(11, 11, "!")];
        // Cursor before a change stays, after a change shifts, inside a replaced span lands at its end.
        let sel = Selection::new(
            vec![
                Range::point(2),   // inside "hello"
                Range::new(6, 11), // "world", ends at the insertion point
                Range::point(0),   // at the start of a replacement
            ],
            1,
        );
        let mapped = sel.map(&changes);
        assert_eq!(
            mapped.ranges(),
            &[Range::point(0), Range::point(2), Range::new(3, 9)]
        );
        // Primary follows its range through the re-sort.
        assert_eq!(mapped.primary(), Range::new(3, 9));
        // A pure insertion at the cursor pushes the cursor after the inserted text.
        assert_eq!(
            Range::point(4).map(&[Change::new(4, 4, "ab")]),
            Range::point(6)
        );
        // A deletion that covers the cursor collapses it to the deletion start.
        assert_eq!(
            Range::point(5).map(&[Change::new(2, 8, "")]),
            Range::point(2)
        );
        // Multi-codepoint inserts count chars, not bytes.
        assert_eq!(
            Range::point(1).map(&[Change::new(0, 0, "ế中")]),
            Range::point(3)
        );
    }

    #[test]
    fn selection_merges_overlapping_ranges() {
        let sel = Selection::new(
            vec![
                Range::new(5, 8),
                Range::new(0, 2),
                Range::new(7, 10),
                Range::point(0),
            ],
            2,
        );
        assert_eq!(sel.ranges(), &[Range::new(0, 2), Range::new(5, 10)]);
        assert_eq!(sel.primary_index(), 1);
        assert_eq!(Range::new(9, 3).from(), 3);
        assert_eq!(Range::new(9, 3).to(), 9);
        assert!(Range::point(4).is_empty());
        // An empty list falls back to a cursor at the start.
        assert_eq!(Selection::new(vec![], 3), Selection::point(0));
    }
}
