//! Undo history of grouped, invertible edits.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::selection::Selection;
use crate::transaction::Change;

/// Typing pauses longer than this start a new undo group.
pub const GROUP_IDLE: Duration = Duration::from_secs(1);

/// Time source for undo grouping; injectable so tests never sleep.
pub trait Clock: Send + Sync {
    fn now(&self) -> Instant;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// A clock that only moves when told to. Clones share the same time.
#[derive(Debug, Clone)]
pub struct ManualClock(Arc<Mutex<Instant>>);

impl ManualClock {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(Instant::now())))
    }

    pub fn advance(&self, d: Duration) {
        let mut now = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        *now += d;
    }
}

impl Default for ManualClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Instant {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// What produced an edit; consecutive edits of the same non-`Other` kind may share a group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    Insert,
    DeleteBackward,
    DeleteForward,
    Other,
}

#[derive(Debug, Clone)]
pub(crate) struct Step {
    pub(crate) forward: Vec<Change>,
    pub(crate) inverse: Vec<Change>,
}

#[derive(Debug, Clone)]
pub(crate) struct Group {
    id: u64,
    pub(crate) steps: Vec<Step>,
    pub(crate) selection_before: Selection,
    pub(crate) selection_after: Selection,
    kind: EditKind,
    last_edit: Instant,
}

#[derive(Debug, Clone, Default)]
pub struct History {
    undo: Vec<Group>,
    redo: Vec<Group>,
    next_id: u64,
    /// Set after undo/redo, cursor moves and explicit breaks: the next edit opens a new group.
    sealed: bool,
}

impl History {
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Identifies the current text state: equal values mean equal text, so a document can
    /// compare it with the value recorded at save time to know whether it is modified.
    /// 0 is the state before any recorded edit.
    pub fn revision(&self) -> u64 {
        self.undo.last().map_or(0, |g| g.id)
    }

    /// Makes the next edit start a new undo group.
    pub fn break_group(&mut self) {
        self.sealed = true;
    }

    fn fresh_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    pub(crate) fn record(
        &mut self,
        step: Step,
        kind: EditKind,
        before: &Selection,
        after: &Selection,
        now: Instant,
    ) {
        self.redo.clear();
        let sealed = std::mem::replace(&mut self.sealed, false);
        let id = self.fresh_id();
        if let Some(top) = self.undo.last_mut()
            && !sealed
            && kind != EditKind::Other
            && top.kind == kind
            && top.selection_after == *before
            && now.saturating_duration_since(top.last_edit) < GROUP_IDLE
        {
            top.steps.push(step);
            top.selection_after = after.clone();
            top.last_edit = now;
            // A grown group is a new text state, so it gets a new revision.
            top.id = id;
            return;
        }
        self.undo.push(Group {
            id,
            steps: vec![step],
            selection_before: before.clone(),
            selection_after: after.clone(),
            kind,
            last_edit: now,
        });
    }

    /// Moves the newest group to the redo stack and returns it for the caller to revert.
    pub(crate) fn undo(&mut self) -> Option<&Group> {
        let group = self.undo.pop()?;
        self.redo.push(group);
        self.sealed = true;
        self.redo.last()
    }

    /// Moves the newest undone group back and returns it for the caller to re-apply.
    pub(crate) fn redo(&mut self) -> Option<&Group> {
        let group = self.redo.pop()?;
        self.undo.push(group);
        self.sealed = true;
        self.undo.last()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;
    use crate::selection::Selection;
    use crate::text::Buffer;
    use crate::transaction::{Change, Transaction};

    fn buffer_with_clock(text: &str) -> (Buffer, ManualClock) {
        let clock = ManualClock::new();
        let mut buf = Buffer::from_str(text);
        buf.set_clock(Arc::new(clock.clone()));
        (buf, clock)
    }

    fn type_str(buf: &mut Buffer, clock: &ManualClock, s: &str) {
        for ch in s.chars() {
            buf.insert_text(&ch.to_string());
            clock.advance(Duration::from_millis(100));
        }
    }

    #[test]
    fn undo_redo_groups() {
        let (mut buf, clock) = buffer_with_clock("");
        type_str(&mut buf, &clock, "abc");
        assert_eq!(buf.text().to_string(), "abc");
        assert!(buf.undo());
        assert_eq!(
            buf.text().to_string(),
            "",
            "consecutive typing is one undo group"
        );
        assert!(buf.redo());
        assert_eq!(buf.text().to_string(), "abc");
        assert_eq!(buf.selection(), &Selection::point(3));

        // Idle longer than the group window starts a new group.
        let (mut buf, clock) = buffer_with_clock("");
        type_str(&mut buf, &clock, "abc");
        clock.advance(Duration::from_secs(2));
        type_str(&mut buf, &clock, "d");
        assert!(buf.undo());
        assert_eq!(buf.text().to_string(), "abc");

        // A cursor jump starts a new group even within the window.
        let (mut buf, clock) = buffer_with_clock("");
        type_str(&mut buf, &clock, "xy");
        buf.set_selection(Selection::point(0));
        buf.set_selection(Selection::point(2));
        type_str(&mut buf, &clock, "z");
        assert!(buf.undo());
        assert_eq!(buf.text().to_string(), "xy");
        assert!(buf.undo());
        assert_eq!(buf.text().to_string(), "");
        assert!(!buf.undo(), "nothing left to undo");

        // Typing and deleting are separate groups; a paste is its own group.
        let (mut buf, clock) = buffer_with_clock("");
        type_str(&mut buf, &clock, "ab");
        buf.delete_backward();
        buf.insert_text("pasted text");
        assert_eq!(buf.text().to_string(), "apasted text");
        assert!(buf.undo());
        assert_eq!(buf.text().to_string(), "a");
        assert!(buf.undo());
        assert_eq!(buf.text().to_string(), "ab");
    }

    #[test]
    fn ac3_type_delete_undo_redo_steps() {
        let (mut buf, clock) = buffer_with_clock("");
        let v0 = buf.version();
        type_str(&mut buf, &clock, "hello");
        assert_eq!(buf.text().to_string(), "hello");
        let v1 = buf.version();
        assert!(v1 > v0, "version tracks every edit");
        clock.advance(Duration::from_secs(2));
        buf.delete_backward();
        buf.delete_backward();
        assert_eq!(buf.text().to_string(), "hel");
        assert!(buf.undo());
        assert_eq!(buf.text().to_string(), "hello");
        assert!(buf.undo());
        assert_eq!(buf.text().to_string(), "");
        assert!(buf.redo());
        assert_eq!(buf.text().to_string(), "hello");
        assert!(buf.redo());
        assert_eq!(buf.text().to_string(), "hel");
        assert!(!buf.redo());
        assert!(buf.version() > v1);
        // A new edit after undo drops the redo branch.
        assert!(buf.undo());
        buf.insert_text("!");
        assert!(!buf.redo());
        assert_eq!(buf.text().to_string(), "hello!");
    }

    #[test]
    fn revision_returns_to_saved_state_after_undo() {
        let (mut buf, clock) = buffer_with_clock("x");
        let saved = buf.revision();
        type_str(&mut buf, &clock, "ab");
        assert_ne!(buf.revision(), saved);
        let after_ab = buf.revision();
        type_str(&mut buf, &clock, "c");
        assert_ne!(
            buf.revision(),
            after_ab,
            "coalesced typing still changes the revision"
        );
        assert!(buf.undo());
        assert_eq!(buf.revision(), saved);
        assert!(buf.redo());
        assert_ne!(buf.revision(), saved);
        // Selection-only changes do not bump version or revision.
        let (v, r) = (buf.version(), buf.revision());
        buf.set_selection(Selection::point(0));
        assert_eq!((buf.version(), buf.revision()), (v, r));
    }

    #[test]
    fn apply_multi_change_one_undo() {
        let (mut buf, _clock) = buffer_with_clock("one two three");
        let v = buf.version();
        buf.apply(Transaction::new(vec![
            Change::new(0, 3, "1"),
            Change::new(4, 7, "2"),
            Change::new(8, 13, "3"),
        ]));
        assert_eq!(buf.text().to_string(), "1 2 3");
        assert_eq!(buf.version(), v + 1);
        assert!(buf.undo());
        assert_eq!(buf.text().to_string(), "one two three");
        assert!(
            !buf.history().can_undo(),
            "the batch was exactly one undo step"
        );
        assert!(buf.redo());
        assert_eq!(buf.text().to_string(), "1 2 3");

        // selection_after is honoured and restored by redo; undo restores the selection before.
        let (mut buf, _clock) = buffer_with_clock("abc");
        buf.set_selection(Selection::point(1));
        let tx = Transaction {
            changes: vec![Change::new(0, 0, ">"), Change::new(3, 3, "<")],
            selection_after: Some(Selection::single(0, 5)),
        };
        buf.apply(tx);
        assert_eq!(buf.text().to_string(), ">abc<");
        assert_eq!(buf.selection(), &Selection::single(0, 5));
        assert!(buf.undo());
        assert_eq!(buf.selection(), &Selection::point(1));
        assert!(buf.redo());
        assert_eq!(buf.selection(), &Selection::single(0, 5));
    }
}
