//! Syntax highlighting of open documents (M4): one `vr_syntax::Highlighter` per document whose
//! language has a compiled grammar. The buffer is changed in many places (vr-core edit
//! operations, undo, find / project replace, reloads), so the highlighter is brought in step
//! with it once before each frame ([`Syntax::sync`], called by `ui::render_with`): the text it
//! last saw against the buffer's text now.
//!
//! - one transaction since the last frame whose changes vr-ide handed over
//!   ([`Syntax::expect_changes`], find replace-all) → `Edit::from_changes_in`, verified against
//!   the buffer;
//! - any other change (typing, deleting, pasting, line operations, undo, redo) →
//!   `Edit::diff_in`, which is the exact edit for a single change and one covering edit
//!   otherwise;
//! - reload from disk and project replace into the buffer ([`Syntax::reset`]) → `reset`;
//! - a new path (Save As) → the language is resolved again;
//! - closing the tab ([`Syntax::remove`]) drops the highlighter, which stops its worker.
//!
//! Edit points use the rope's line index while the document has no lone CR
//! (`LineBreaks::NoLoneCr`, O(log n)); the flag is found at open and kept by looking at the
//! bytes each edit touched.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use ropey::Rope;
use vr_core::Document;
use vr_syntax::{
    Capture, Edit, HighlightOptions, HighlightStatus, Highlighter, LanguageRegistry, LineBreaks,
    Notify, has_lone_cr, lone_cr_in,
};

/// The built-in language table, built once per process (`App::new`; `App::load` replaces it
/// with the user's).
pub fn builtin_registry() -> Arc<LanguageRegistry> {
    static BUILTIN: OnceLock<Arc<LanguageRegistry>> = OnceLock::new();
    BUILTIN
        .get_or_init(|| Arc::new(LanguageRegistry::builtin()))
        .clone()
}

/// One change of a transaction: chars `from..to` of the text before it replaced by the text.
pub type Change = (usize, usize, String);

/// How a document's highlighter was kept in step (tests and the notes).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    /// Transactions handed over as a change list.
    pub changes: u64,
    /// Changes found by comparing the texts.
    pub diffs: u64,
    pub resets: u64,
    /// Whole-text lone-CR scans.
    pub scans: u64,
}

/// A notice to show for a document (once).
pub struct SyntaxNotice {
    pub warning: bool,
    pub text: String,
}

pub struct Syntax {
    registry: Arc<LanguageRegistry>,
    /// One per `App::docs` entry, same index; grown on demand (tests push documents directly).
    docs: Vec<DocSyntax>,
    /// Set by the highlighters' workers when spans land; the event loop redraws.
    wake: Arc<AtomicBool>,
    /// languages.toml problems not shown yet (AC-13e).
    startup: Vec<String>,
    /// A `spans` call since the last frame ran out of its query budget (D162).
    cut: AtomicBool,
    /// The coming frame is the one redraw asked for after a cut frame; if it is cut too, no
    /// further one is asked for, so a document whose every query is cut does not redraw in a
    /// loop.
    follow_up: bool,
    /// Query budget for highlighters (tests); `None` keeps vr-syntax's default.
    query_timeout: Option<Duration>,
}

#[derive(Default)]
struct DocSyntax {
    /// The document's path when its language was resolved; `None` before that.
    resolved: Option<Option<PathBuf>>,
    /// The language chosen with `/lang`, used instead of the one the path gives.
    forced: Option<String>,
    hl: Option<Highlighter>,
    /// The text the highlighter has and the buffer version it had.
    seen: Rope,
    seen_version: u64,
    /// `seen` may hold a CR without its LF.
    lone_cr: bool,
    /// A change list for the transaction applied at this buffer version.
    expected: Option<(u64, Vec<Change>)>,
    reset: bool,
    noticed: bool,
    stats: Stats,
}

impl Syntax {
    pub fn new(registry: Arc<LanguageRegistry>) -> Syntax {
        Syntax {
            registry,
            docs: Vec::new(),
            wake: Arc::default(),
            startup: Vec::new(),
            cut: AtomicBool::new(false),
            follow_up: false,
            query_timeout: None,
        }
    }

    /// Query budget of every highlighter, open and later ones (tests: 0 cuts every query).
    #[cfg(test)]
    pub(crate) fn set_query_timeout(&mut self, timeout: Duration) {
        self.query_timeout = Some(timeout);
        for hl in self.docs.iter_mut().filter_map(|d| d.hl.as_mut()) {
            hl.set_query_timeout(timeout);
        }
    }

    /// Called once a frame is drawn: when a `spans` call of it was cut by the query budget,
    /// asks the event loop for one more frame (through the wake flag), unless this frame was
    /// already that follow-up. True when it asked.
    pub fn frame_drawn(&mut self) -> bool {
        let cut = self.cut.swap(false, Ordering::AcqRel);
        let was_follow_up = std::mem::take(&mut self.follow_up);
        if cut && !was_follow_up {
            self.follow_up = true;
            self.wake.store(true, Ordering::Release);
            return true;
        }
        false
    }

    /// Uses `languages` (built-in table plus the user's file) for documents opened from now on.
    pub fn set_languages(&mut self, languages: &vr_config::Languages) {
        let (registry, warnings) = LanguageRegistry::new(languages);
        self.registry = Arc::new(registry);
        self.startup = warnings.iter().map(ToString::to_string).collect();
    }

    pub fn registry(&self) -> &Arc<LanguageRegistry> {
        &self.registry
    }

    /// The startup notices (languages whose grammar is not compiled in), once.
    pub fn take_startup_notices(&mut self) -> Vec<String> {
        std::mem::take(&mut self.startup)
    }

    /// A notify for a highlighter made outside the documents (the Git commit view): its spans
    /// landing redraw like a document's.
    pub fn notifier(&self) -> Notify {
        let wake = self.wake.clone();
        Arc::new(move || wake.store(true, Ordering::Release))
    }

    /// True once after a highlighter published new spans (or stopped): redraw.
    pub fn take_wake(&self) -> bool {
        self.wake.swap(false, Ordering::AcqRel)
    }

    pub fn len(&self) -> usize {
        self.docs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    /// Highlights document `i` as language `id` from now on, whatever its path says
    /// (`/lang`); `false` when no compiled-in grammar has that id.
    pub fn set_language(&mut self, i: usize, id: &str) -> bool {
        if !self.registry.has_grammar(id) {
            return false;
        }
        if self.docs.len() <= i {
            self.docs.resize_with(i + 1, DocSyntax::default);
        }
        let d = &mut self.docs[i];
        d.forced = Some(id.to_string());
        // The next sync resolves the language again.
        d.resolved = None;
        true
    }

    /// Ids of the languages that can be highlighted, sorted (`/lang` completion).
    pub fn language_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .registry
            .ids()
            .filter(|id| self.registry.has_grammar(id))
            .map(str::to_string)
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }

    /// The highlighted language of document `i` (None: plain text).
    pub fn language(&self, i: usize) -> Option<&str> {
        self.docs.get(i)?.hl.as_ref().map(Highlighter::language)
    }

    pub fn stats(&self, i: usize) -> Stats {
        self.docs.get(i).map(|d| d.stats).unwrap_or_default()
    }

    /// Waits up to `timeout` until document `i`'s spans describe the text it was last synced
    /// to (tests and previews; the UI never waits).
    pub fn wait(&self, i: usize, timeout: Duration) -> bool {
        match self.docs.get(i).and_then(|d| d.hl.as_ref()) {
            Some(hl) => hl.wait(timeout),
            None => true,
        }
    }

    /// Spans of document `i` inside `bytes`, sorted and disjoint (empty for plain text). A
    /// call cut by the query budget is remembered for [`Syntax::frame_drawn`].
    pub fn spans(
        &self,
        i: usize,
        bytes: std::ops::Range<usize>,
    ) -> Vec<(std::ops::Range<usize>, Capture)> {
        match self.docs.get(i).and_then(|d| d.hl.as_ref()) {
            Some(hl) => {
                let q = hl.query(bytes);
                if q.truncated {
                    self.cut.store(true, Ordering::Release);
                }
                q.spans
            }
            None => Vec::new(),
        }
    }

    /// Tab `i` closed: its highlighter is dropped (its worker stops) and later ones move up.
    pub fn remove(&mut self, i: usize) {
        if i < self.docs.len() {
            self.docs.remove(i);
        }
    }

    /// Document `i` was replaced from outside the editor (reload from disk, project replace):
    /// the next sync starts the highlighter over instead of diffing.
    pub fn reset(&mut self, i: usize) {
        if let Some(d) = self.docs.get_mut(i) {
            d.reset = true;
        }
    }

    /// The transaction about to change document `i` from buffer version `version` has these
    /// changes (chars of the text before it, sorted): the next sync hands them over as edits
    /// when nothing else changed the buffer in between.
    pub fn expect_changes(&mut self, i: usize, version: u64, changes: Vec<Change>) {
        if let Some(d) = self.docs.get_mut(i) {
            d.expected = Some((version, changes));
        }
    }

    /// Brings document `i`'s highlighter in step with `doc`; returns the notice to show when
    /// highlighting of that document is off for a reason the user should know (once per
    /// document).
    pub fn sync(&mut self, i: usize, doc: &Document, max_bytes: u64) -> Option<SyntaxNotice> {
        if self.docs.len() <= i {
            self.docs.resize_with(i + 1, DocSyntax::default);
        }
        let registry = self.registry.clone();
        let wake = self.wake.clone();
        let d = &mut self.docs[i];
        let rope = doc.buffer.text();
        let version = doc.buffer.version();
        if d.resolved.as_ref() != Some(&doc.path) {
            d.resolve(&registry, doc, max_bytes, &wake, self.query_timeout);
        } else if d.hl.is_some() {
            d.step(rope, version, max_bytes);
        }
        d.expected = None;
        d.reset = false;
        let hl = d.hl.as_ref()?;
        if d.noticed {
            return None;
        }
        let HighlightStatus::Disabled(why) = hl.status() else {
            return None;
        };
        d.noticed = true;
        Some(SyntaxNotice {
            warning: !matches!(why, vr_syntax::Disabled::TooLarge { .. }),
            text: why.to_string(),
        })
    }
}

impl DocSyntax {
    /// Picks the language for the document's path; keeps a running highlighter whose
    /// language stays (Save As to the same extension).
    fn resolve(
        &mut self,
        registry: &Arc<LanguageRegistry>,
        doc: &Document,
        max_bytes: u64,
        wake: &Arc<AtomicBool>,
        query_timeout: Option<Duration>,
    ) {
        self.resolved = Some(doc.path.clone());
        let language = match &self.forced {
            Some(id) => registry.ids().find(|known| known == id),
            None => doc.path.as_deref().and_then(|p| language_for(registry, p)),
        };
        let rope = doc.buffer.text();
        let version = doc.buffer.version();
        match (&self.hl, language) {
            (Some(hl), Some(lang)) if hl.language() == lang => {
                self.step(rope, version, max_bytes);
            }
            (_, None) => self.hl = None,
            (_, Some(lang)) => {
                let wake = wake.clone();
                let notify: Notify = Arc::new(move || wake.store(true, Ordering::Release));
                let defaults = HighlightOptions::default();
                let options = HighlightOptions {
                    max_bytes,
                    query_timeout: query_timeout.unwrap_or(defaults.query_timeout),
                    ..defaults
                };
                // Too large: the highlighter disables itself; no scan of the text.
                self.lone_cr = self.scan(rope, max_bytes);
                self.hl = Some(Highlighter::new(
                    registry.clone(),
                    lang,
                    rope.clone(),
                    options,
                    Some(notify),
                ));
                self.seen = rope.clone();
                self.seen_version = version;
                self.noticed = false;
            }
        }
    }

    /// Brings a running highlighter to buffer version `version`.
    fn step(&mut self, rope: &Rope, version: u64, max_bytes: u64) {
        if self.reset || version < self.seen_version {
            self.restart(rope, version, max_bytes);
        } else if version == self.seen_version {
            // Nothing changed since the last frame.
        } else if self
            .hl
            .as_ref()
            .is_some_and(|hl| matches!(hl.status(), HighlightStatus::Disabled(_)))
        {
            // A disabled highlighter takes no edits (only a reset starts it again), so the
            // change is not looked at: on a file over the limit a diff alone costs more than
            // the frame budget.
            self.seen = rope.clone();
            self.seen_version = version;
        } else {
            self.follow(rope, version);
        }
    }

    fn restart(&mut self, rope: &Rope, version: u64, max_bytes: u64) {
        if let Some(hl) = &mut self.hl {
            hl.reset(rope.clone());
        }
        self.lone_cr = self.scan(rope, max_bytes);
        self.seen = rope.clone();
        self.seen_version = version;
        self.stats.resets += 1;
    }

    /// Whether `rope` holds a lone CR; a text over `max_bytes` is not scanned (its highlighter
    /// disables itself, so no edit points are needed).
    fn scan(&mut self, rope: &Rope, max_bytes: u64) -> bool {
        if rope.len_bytes() as u64 > max_bytes {
            return false;
        }
        self.stats.scans += 1;
        has_lone_cr(rope)
    }

    /// The buffer moved on from `seen` to `rope`.
    fn follow(&mut self, rope: &Rope, version: u64) {
        let breaks = if self.lone_cr {
            LineBreaks::Unknown
        } else {
            LineBreaks::NoLoneCr
        };
        let listed = match self.expected.take() {
            Some((at, changes)) if at == self.seen_version && version == at + 1 => {
                listed_edits(&self.seen, rope, &changes, breaks)
            }
            _ => None,
        };
        let edits = match listed {
            Some(edits) => {
                self.stats.changes += 1;
                edits
            }
            None => {
                self.stats.diffs += 1;
                Edit::diff_in(&self.seen, rope, breaks)
                    .into_iter()
                    .collect()
            }
        };
        if !edits.is_empty() {
            if !self.lone_cr {
                self.lone_cr = inserted_ranges(&edits)
                    .into_iter()
                    .any(|r| lone_cr_in(rope, r));
            }
            if let Some(hl) = &mut self.hl {
                hl.edit(&edits, rope.clone());
            }
        }
        self.seen = rope.clone();
        self.seen_version = version;
    }
}

/// The language of `path` when its grammar is compiled in; otherwise plain text (a missing
/// grammar was named at startup, AC-13e).
fn language_for<'a>(registry: &'a LanguageRegistry, path: &Path) -> Option<&'a str> {
    registry
        .language_for_path(path)
        .filter(|id| registry.has_grammar(id))
}

/// The edits of a change list, when applying it to `before` gives exactly `after` (a change
/// list that does not describe the buffer's transaction falls back to a diff).
fn listed_edits(
    before: &Rope,
    after: &Rope,
    changes: &[Change],
    breaks: LineBreaks,
) -> Option<Vec<Edit>> {
    let mut rebuilt = before.clone();
    let mut last = usize::MAX;
    for (from, to, insert) in changes.iter().rev() {
        if from > to || *to > last.min(rebuilt.len_chars()) {
            return None;
        }
        rebuilt.remove(*from..*to);
        rebuilt.insert(*from, insert);
        last = *from;
    }
    if Edit::diff(&rebuilt, after).is_some() {
        return None;
    }
    Some(Edit::from_changes_in(
        before,
        changes.iter().map(|(f, t, s)| (*f, *t, s.as_str())),
        breaks,
    ))
}

/// Byte ranges of the final text each edit inserted. `edits` come last change of the text
/// first (each in the text before it), so in the final text an edit's range moves by the size
/// changes of the edits that lie before it.
fn inserted_ranges(edits: &[Edit]) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::with_capacity(edits.len());
    let mut shift: isize = 0;
    for e in edits.iter().rev() {
        let start = (e.start_byte as isize + shift) as usize;
        out.push(start..start + (e.new_end_byte - e.start_byte));
        shift += e.new_end_byte as isize - e.old_end_byte as isize;
    }
    out
}
