//! Changes and transactions in char offsets.

use ropey::Rope;

use crate::selection::Selection;

/// Replace `from..to` (char offsets in the text before the transaction) with `insert`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub from: usize,
    pub to: usize,
    pub insert: String,
}

impl Change {
    pub fn new(from: usize, to: usize, insert: impl Into<String>) -> Self {
        Self {
            from,
            to,
            insert: insert.into(),
        }
    }

    fn inserted_chars(&self) -> usize {
        self.insert.chars().count()
    }
}

/// A batch of non-overlapping changes, sorted by `from`, applied and undone as one step.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Transaction {
    pub changes: Vec<Change>,
    pub selection_after: Option<Selection>,
}

impl Transaction {
    pub fn new(changes: Vec<Change>) -> Self {
        Self {
            changes,
            selection_after: None,
        }
    }

    pub fn with_selection(mut self, selection: Selection) -> Self {
        self.selection_after = Some(selection);
        self
    }
}

/// Sorts changes, clamps them to `len`, and drops no-ops and changes overlapping an earlier one,
/// so a malformed transaction can never corrupt the rope. Overlaps are a caller bug: debug
/// builds assert on them, release builds drop them.
pub fn normalize(mut changes: Vec<Change>, len: usize) -> Vec<Change> {
    // Sorting by (from, to) puts a pure insertion before a span starting at the same offset,
    // so the result never depends on the order the caller listed them in.
    changes.sort_by_key(|c| (c.from.min(c.to), c.from.max(c.to)));
    let mut out: Vec<Change> = Vec::with_capacity(changes.len());
    for mut c in changes {
        if c.from > c.to {
            std::mem::swap(&mut c.from, &mut c.to);
        }
        c.from = c.from.min(len);
        c.to = c.to.min(len);
        if c.from == c.to && c.insert.is_empty() {
            continue;
        }
        if out.last().is_some_and(|prev| c.from < prev.to) {
            tracing::warn!(from = c.from, to = c.to, "dropping overlapping change");
            debug_assert!(false, "overlapping change {}..{}", c.from, c.to);
            continue;
        }
        out.push(c);
    }
    out
}

/// Applies normalized changes. Reverse order keeps the earlier offsets valid.
pub fn apply_changes(rope: &mut Rope, changes: &[Change]) {
    for c in changes.iter().rev() {
        if c.to > c.from {
            rope.remove(c.from..c.to);
        }
        if !c.insert.is_empty() {
            rope.insert(c.from, &c.insert);
        }
    }
}

/// The changes that undo `changes`, expressed in offsets of the text after them.
pub fn invert_changes(before: &Rope, changes: &[Change]) -> Vec<Change> {
    let (mut inserted, mut removed) = (0usize, 0usize);
    changes
        .iter()
        .map(|c| {
            let ins = c.inserted_chars();
            let from = c.from - removed + inserted;
            let inverse = Change::new(from, from + ins, before.slice(c.from..c.to).to_string());
            inserted += ins;
            removed += c.to - c.from;
            inverse
        })
        .collect()
}

/// Maps an offset through normalized changes. A cursor at a pure insertion point ends up
/// after the inserted text (typing); one inside a replaced span lands at the end of the
/// replacement; one at the start of a deletion stays there.
pub fn map_pos(changes: &[Change], pos: usize) -> usize {
    let (mut inserted, mut removed) = (0usize, 0usize);
    for c in changes {
        if pos < c.from || (pos == c.from && c.from < c.to) {
            break;
        }
        let ins = c.inserted_chars();
        if pos < c.to {
            return c.from - removed + inserted + ins;
        }
        inserted += ins;
        removed += c.to - c.from;
    }
    pos - removed + inserted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_then_invert_restores_text() {
        let before = Rope::from_str("one tiếng 中文 three");
        let changes = normalize(
            vec![
                Change::new(13, 18, "3"),
                Change::new(0, 3, "1"),
                Change::new(4, 9, "việt"),
                Change::new(10, 10, "+"),
            ],
            before.len_chars(),
        );
        let mut rope = before.clone();
        apply_changes(&mut rope, &changes);
        assert_eq!(rope.to_string(), "1 việt +中文 3");
        let inverse = invert_changes(&before, &changes);
        apply_changes(&mut rope, &inverse);
        assert_eq!(rope, before);
    }

    #[test]
    fn normalize_sorts_clamps_and_drops_noops() {
        let out = normalize(
            vec![
                Change::new(6, 99, "z"),
                Change::new(1, 4, "a"),
                Change::new(5, 5, ""),
            ],
            8,
        );
        assert_eq!(out, vec![Change::new(1, 4, "a"), Change::new(6, 8, "z")]);
    }

    #[test]
    fn normalize_result_does_not_depend_on_list_order() {
        let delete = Change::new(5, 8, "");
        let insert = Change::new(5, 5, "x");
        let expected = vec![insert.clone(), delete.clone()];
        assert_eq!(
            normalize(vec![delete.clone(), insert.clone()], 10),
            expected
        );
        assert_eq!(normalize(vec![insert, delete], 10), expected);
    }

    // Debug builds fail loudly on a caller bug; release builds drop the overlap instead.
    #[test]
    #[cfg_attr(debug_assertions, should_panic(expected = "overlapping change"))]
    fn normalize_overlap_asserts_in_debug_and_drops_in_release() {
        let out = normalize(
            vec![Change::new(1, 4, "a"), Change::new(2, 3, "overlap")],
            8,
        );
        assert_eq!(out, vec![Change::new(1, 4, "a")]);
    }

    #[test]
    fn map_pos_counts_chars() {
        let changes = vec![Change::new(0, 2, "ế"), Change::new(5, 5, "中文")];
        assert_eq!(map_pos(&changes, 4), 3);
        assert_eq!(map_pos(&changes, 5), 6);
        assert_eq!(map_pos(&changes, 9), 10);
    }
}
