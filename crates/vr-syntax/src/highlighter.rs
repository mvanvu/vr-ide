//! Per-document highlighter: a worker thread owns the parser and the syntax trees, the caller
//! (UI thread) only ever reads the latest published result.
//!
//! Versions: every edit bumps the highlighter's version. The worker parses the newest text it
//! has received and publishes a [`Snapshot`] tagged with that version. `spans()` uses the newest
//! snapshot; when it is older than the text, its spans are carried through the edits made since
//! (`edit::map_forward`) and dropped where text changed, so no span is ever returned for text it
//! was not computed for.

use std::cell::Cell;
use std::cmp::Reverse;
use std::collections::hash_map::Entry as MapEntry;
use std::collections::{HashMap, VecDeque};
use std::ops::{ControlFlow, Range};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use ropey::Rope;
use tree_sitter::{
    Language, Node, ParseOptions, ParseState, Parser, QueryCursor, QueryCursorOptions,
    QueryCursorState, StreamingIterator, Tree,
};

use crate::capture::Paint;
use crate::edit::{map_back, map_forward, point_at};
use crate::query::{InjectionPattern, LanguageConfig, RopeText, node_text, predicates_hold};
use crate::{Capture, Edit, LanguageRegistry, Reparse};

/// Nesting limit of embedded languages (PHP → HTML → JavaScript is 2).
const MAX_DEPTH: u8 = 4;
/// Upper bound of embedded-language layers per document (Markdown has one per paragraph);
/// layers past it stay in the enclosing language's colours.
const MAX_LAYERS: usize = 10_000;
/// Texts up to this size whose incremental trees have errors are parsed again from scratch
/// (`Worker::verify`); above it the incremental result stands, a fresh parse per pause in
/// typing would cost seconds of worker time.
const VERIFY_MAX_BYTES: usize = 1024 * 1024;
const NO_CAPTURE: u8 = u8::MAX;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightOptions {
    /// `max_highlight_bytes`: larger texts are not highlighted (AC-13c).
    pub max_bytes: u64,
    /// Limit of one parse call; a root parse over it leaves the document plain.
    pub parse_timeout: Duration,
    /// Limit of one `spans()` call on the caller's thread; it returns what it has by then.
    pub query_timeout: Duration,
    /// Quiet time (no new edit) before a text with syntax errors gets its fresh-parse check, so
    /// a burst of typing never starts one.
    pub verify_idle: Duration,
}

impl Default for HighlightOptions {
    fn default() -> Self {
        HighlightOptions {
            max_bytes: 10 * 1024 * 1024,
            parse_timeout: Duration::from_secs(10),
            query_timeout: Duration::from_millis(50),
            verify_idle: Duration::from_millis(150),
        }
    }
}

/// Why a document is shown without colours. `Display` is the one-line notice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disabled {
    TooLarge { bytes: u64, limit: u64 },
    Timeout { after: Duration },
    NoGrammar { language: String },
    Query { language: String, message: String },
    WorkerFailed,
}

impl std::fmt::Display for Disabled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Disabled::TooLarge { bytes, limit } => write!(
                f,
                "file is larger than max_highlight_bytes ({bytes} > {limit} bytes); shown as plain text"
            ),
            Disabled::Timeout { after } => write!(
                f,
                "syntax parse took longer than {} s; shown as plain text",
                after.as_secs_f32()
            ),
            Disabled::NoGrammar { language } => write!(
                f,
                "no grammar for {language} in this build; shown as plain text"
            ),
            Disabled::Query { language, message } => write!(
                f,
                "highlight queries of {language} do not compile ({message}); shown as plain text"
            ),
            Disabled::WorkerFailed => {
                f.write_str("the syntax highlighter stopped; shown as plain text")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HighlightStatus {
    /// The text has changed since the last published parse (or none has landed yet).
    Parsing,
    /// Spans describe the current text.
    Ready,
    Disabled(Disabled),
}

/// Called on the worker thread whenever new spans (or a `Disabled` state) are published, so the
/// UI can schedule a redraw. Must not block.
pub type Notify = Arc<dyn Fn() + Send + Sync>;

/// One parsed language region of a snapshot: the root document or an embedded block.
struct Layer {
    config: Arc<LanguageConfig>,
    tree: Tree,
    /// Sorted, non-overlapping byte ranges of the text this layer covers.
    ranges: Vec<tree_sitter::Range>,
    depth: u8,
}

/// A parse result: the trees of one text version, immutable once published.
struct Snapshot {
    version: u64,
    text: Rope,
    layers: Vec<Layer>,
}

#[derive(Default)]
struct Published {
    snapshot: Option<Arc<Snapshot>>,
    /// False while a snapshot whose trees have errors waits for its fresh-parse check: its
    /// spans are shown, but the status stays `Parsing`.
    exact: Arc<AtomicBool>,
    disabled: Option<Disabled>,
}

impl Published {
    fn ready_at(&self, version: u64) -> bool {
        self.snapshot
            .as_ref()
            .is_some_and(|s| s.version >= version && self.exact.load(Ordering::Relaxed))
    }
}

#[derive(Default)]
struct Shared {
    state: Mutex<Published>,
    changed: Condvar,
    /// The caller's newest version; a fresh-parse check of an older one stops early.
    latest: AtomicU64,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Published> {
        // A panic while holding the lock leaves plain data behind; keep using it.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn publish(&self, snapshot: Snapshot, exact: Arc<AtomicBool>) {
        let mut state = self.lock();
        if state.disabled.is_none() {
            state.snapshot = Some(Arc::new(snapshot));
            state.exact = exact;
        }
        drop(state);
        self.changed.notify_all();
    }

    /// The published snapshot stands as it is (its fresh-parse check could not finish).
    fn mark_exact(&self, exact: &AtomicBool) {
        // Under the lock, so a `wait` between its check and its sleep cannot miss it.
        let state = self.lock();
        exact.store(true, Ordering::Relaxed);
        drop(state);
        self.changed.notify_all();
    }

    fn disable(&self, reason: Disabled) {
        let mut state = self.lock();
        if state.disabled.is_none() {
            state.disabled = Some(reason);
            state.snapshot = None;
        }
        drop(state);
        self.changed.notify_all();
    }
}

enum Msg {
    Edit {
        version: u64,
        edits: Vec<Edit>,
        text: Rope,
    },
    Reset {
        version: u64,
        text: Rope,
    },
}

/// The caller's end of a worker. Dropping it stops the worker: the channel closes and an
/// in-flight parse is cancelled at its next progress check.
struct WorkerLink {
    tx: Sender<Msg>,
    cancel: Arc<AtomicBool>,
}

impl Drop for WorkerLink {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Syntax highlighting of one document. Cheap calls only: parsing happens on its worker.
pub struct Highlighter {
    registry: Arc<LanguageRegistry>,
    language: String,
    options: HighlightOptions,
    notify: Option<Notify>,
    version: u64,
    text: Rope,
    /// Edits not yet covered by a published snapshot, with the version each produced.
    pending: VecDeque<(u64, Vec<Edit>)>,
    /// Replaced on `reset`, so a worker of the old text can never publish into the new one.
    shared: Arc<Shared>,
    worker: Option<WorkerLink>,
}

impl Highlighter {
    /// Starts highlighting `text` as `language` (a registry id). Never parses on the calling
    /// thread: the first spans arrive when the worker's parse lands (`notify`).
    pub fn new(
        registry: Arc<LanguageRegistry>,
        language: &str,
        text: Rope,
        options: HighlightOptions,
        notify: Option<Notify>,
    ) -> Highlighter {
        let mut h = Highlighter {
            registry,
            language: language.to_owned(),
            options,
            notify,
            version: 0,
            text,
            pending: VecDeque::new(),
            shared: Arc::default(),
            worker: None,
        };
        h.start();
        h
    }

    pub fn language(&self) -> &str {
        &self.language
    }

    /// Increments on every `edit`, effective `update` and `reset`.
    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn status(&self) -> HighlightStatus {
        let state = self.shared.lock();
        match (&state.disabled, &state.snapshot) {
            (Some(d), _) => HighlightStatus::Disabled(d.clone()),
            (None, Some(s)) if s.version == self.version && state.ready_at(self.version) => {
                HighlightStatus::Ready
            }
            _ => HighlightStatus::Parsing,
        }
    }

    /// The text changed by `edits` (applied in order) into `text`.
    pub fn edit(&mut self, edits: &[Edit], text: Rope) {
        self.version += 1;
        self.text = text;
        let Some(link) = &self.worker else {
            return;
        };
        let bytes = self.text.len_bytes() as u64;
        if bytes > self.options.max_bytes {
            self.stop(Disabled::TooLarge {
                bytes,
                limit: self.options.max_bytes,
            });
            return;
        }
        let published = self.shared.lock().snapshot.as_ref().map(|s| s.version);
        while self
            .pending
            .front()
            .is_some_and(|(v, _)| Some(*v) <= published)
        {
            self.pending.pop_front();
        }
        self.pending.push_back((self.version, edits.to_vec()));
        self.shared.latest.store(self.version, Ordering::Relaxed);
        let msg = Msg::Edit {
            version: self.version,
            edits: edits.to_vec(),
            text: self.text.clone(),
        };
        if link.tx.send(msg).is_err() {
            // The worker ended on its own (it published why) or died.
            self.stop(Disabled::WorkerFailed);
        }
    }

    /// The text is now `text` (undo, redo, any change without an edit list): the difference
    /// to the previous text becomes one edit.
    pub fn update(&mut self, text: Rope) {
        match Edit::diff(&self.text, &text) {
            Some(edit) => self.edit(&[edit], text),
            None => self.text = text,
        }
    }

    /// Starts over on `text` (reload from disk, project replace into the buffer): nothing of
    /// the old parse is shown or reused, and a disabled highlighter tries again.
    pub fn reset(&mut self, text: Rope) {
        self.version += 1;
        self.text = text;
        self.start();
    }

    /// Spans of the current text inside `range` (bytes), sorted and disjoint. Regions not
    /// parsed yet are left out (plain).
    pub fn spans(&self, range: Range<usize>) -> Vec<(Range<usize>, Capture)> {
        let snapshot = {
            let state = self.shared.lock();
            if state.disabled.is_some() {
                return Vec::new();
            }
            match &state.snapshot {
                Some(s) => s.clone(),
                None => return Vec::new(),
            }
        };
        let len = self.text.len_bytes();
        let end = range.end.min(len);
        let start = range.start.min(end);
        if start == end {
            return Vec::new();
        }
        let edits: Vec<Edit> = self
            .pending
            .iter()
            .filter(|(v, _)| *v > snapshot.version)
            .flat_map(|(_, e)| e.iter().copied())
            .collect();
        if edits.is_empty() {
            return snapshot.spans(start..end, self.options.query_timeout);
        }
        let old_start = map_back(start, &edits, false);
        let old_end = map_back(end, &edits, true).max(old_start);
        let spans = snapshot.spans(old_start..old_end, self.options.query_timeout);
        map_forward(spans, &edits)
            .into_iter()
            .filter_map(|(r, c)| {
                let r = r.start.max(start)..r.end.min(end);
                (r.start < r.end).then_some((r, c))
            })
            .collect()
    }

    /// Blocks up to `timeout` until spans describe the current text (or highlighting is
    /// disabled); true when they do. For tests and callers that accept a short bounded wait.
    /// Text with syntax errors is ready only after its fresh-parse check, which starts once the
    /// document has had no edit for `verify_idle`.
    pub fn wait(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut state = self.shared.lock();
        loop {
            if state.disabled.is_some() || state.ready_at(self.version) {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            state = self
                .shared
                .changed
                .wait_timeout(state, deadline - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    fn stop(&mut self, reason: Disabled) {
        self.worker = None;
        self.pending.clear();
        self.shared.disable(reason);
    }

    fn start(&mut self) {
        self.worker = None;
        self.pending.clear();
        self.shared = Arc::default();
        self.shared.latest.store(self.version, Ordering::Relaxed);
        let bytes = self.text.len_bytes() as u64;
        if bytes > self.options.max_bytes {
            self.shared.disable(Disabled::TooLarge {
                bytes,
                limit: self.options.max_bytes,
            });
            return;
        }
        if !self.registry.has_grammar(&self.language) {
            self.shared.disable(Disabled::NoGrammar {
                language: self.language.clone(),
            });
            return;
        }
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker = Worker {
            registry: self.registry.clone(),
            language: self.language.clone(),
            timeout: self.options.parse_timeout,
            shared: self.shared.clone(),
            cancel: cancel.clone(),
            notify: self.notify.clone(),
            parser: Parser::new(),
            text: Rope::new(),
            applied: Rope::new(),
            queued: Vec::new(),
            version: 0,
            tree: None,
            old_layers: Vec::new(),
            root_records: None,
            verifying: false,
            verify_idle: self.options.verify_idle,
            edited: Vec::new(),
        };
        let spawned = std::thread::Builder::new()
            .name(format!("vr-syntax {}", self.language))
            .spawn(move || worker.run(rx));
        if spawned.is_err() {
            self.shared.disable(Disabled::WorkerFailed);
            return;
        }
        let _ = tx.send(Msg::Reset {
            version: self.version,
            text: self.text.clone(),
        });
        self.worker = Some(WorkerLink { tx, cancel });
    }
}

impl std::fmt::Debug for Highlighter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Highlighter")
            .field("language", &self.language)
            .field("version", &self.version)
            .field("status", &self.status())
            .finish()
    }
}

enum Stop {
    Cancelled,
    Timeout,
    Failed,
    /// A fresh-parse check stopped because newer text arrived.
    Superseded,
}

/// An embedded layer kept from the previous cycle for incremental reuse.
struct OldLayer {
    language: String,
    ranges: Vec<tree_sitter::Range>,
    tree: Tree,
    /// An edit touched the text of `ranges`. `Tree::has_changes` cannot tell: an edit anywhere
    /// before the layer's first range changes the root's leading padding.
    dirty: bool,
}

/// Above this many edits between two cycles, they are applied as one edit covering them all:
/// each edit costs a pass over every layer and record, and one covering edit reparses no more
/// than a fresh parse would.
const MAX_SEPARATE_EDITS: usize = 16;

struct Worker {
    registry: Arc<LanguageRegistry>,
    language: String,
    timeout: Duration,
    shared: Arc<Shared>,
    cancel: Arc<AtomicBool>,
    notify: Option<Notify>,
    parser: Parser,
    /// The newest text received.
    text: Rope,
    /// The text the trees, layers and records are in step with; `queued` turns it into `text`.
    applied: Rope,
    queued: Vec<Edit>,
    version: u64,
    tree: Option<Tree>,
    old_layers: Vec<OldLayer>,
    /// Injection matches of the root layer from the last cycle, kept in step with edits; `None`
    /// means the next cycle queries the whole document.
    root_records: Option<Vec<Record>>,
    /// Inside `verify`: parses stop when the caller has newer text.
    verifying: bool,
    /// `HighlightOptions::verify_idle`.
    verify_idle: Duration,
    /// Byte ranges (current text) edited since the last cycle.
    edited: Vec<(usize, usize)>,
}

impl Worker {
    fn run(mut self, rx: Receiver<Msg>) {
        let shared = self.shared.clone();
        let notify = self.notify.clone();
        if catch_unwind(AssertUnwindSafe(|| self.serve(&rx))).is_err() {
            shared.disable(Disabled::WorkerFailed);
            if let Some(n) = &notify {
                n();
            }
        }
    }

    fn serve(&mut self, rx: &Receiver<Msg>) {
        let config = match self.registry.config(&self.language) {
            Ok(c) => c,
            Err(message) => {
                return self.finish(Disabled::Query {
                    language: self.language.clone(),
                    message,
                });
            }
        };
        // A message that arrived while waiting for a pause before the check.
        let mut next = None;
        loop {
            let msg = match next.take() {
                Some(msg) => msg,
                None => match rx.recv() {
                    Ok(msg) => msg,
                    Err(_) => return,
                },
            };
            self.absorb(msg);
            // Coalesce: parse once for everything typed meanwhile.
            while let Ok(msg) = rx.try_recv() {
                self.absorb(msg);
            }
            if self.cancel.load(Ordering::Relaxed) {
                return;
            }
            // A cycle with no old tree, layers or records already is a fresh parse.
            let from_scratch =
                self.tree.is_none() && self.old_layers.is_empty() && self.root_records.is_none();
            match self.cycle(&config) {
                Ok(snapshot) => {
                    let check = !from_scratch
                        && snapshot.text.len_bytes() <= VERIFY_MAX_BYTES
                        && snapshot
                            .layers
                            .iter()
                            .any(|l| l.tree.root_node().has_error());
                    let exact = Arc::new(AtomicBool::new(!check));
                    self.shared.publish(snapshot, exact.clone());
                    if let Some(n) = &self.notify {
                        n();
                    }
                    if check {
                        // Typing bursts never start a check: the next keystroke's cycle
                        // replaces this snapshot anyway, and a check per keystroke kept a core
                        // busy. The check runs once the document has been quiet for a moment.
                        match rx.recv_timeout(self.verify_idle) {
                            Ok(msg) => {
                                next = Some(msg);
                                continue;
                            }
                            Err(RecvTimeoutError::Disconnected) => return,
                            Err(RecvTimeoutError::Timeout) => {}
                        }
                        if self.cancel.load(Ordering::Relaxed) {
                            return;
                        }
                        match self.verify(&config) {
                            Ok(Some(fresh)) => self.shared.publish(fresh, Arc::new(true.into())),
                            // Newer text arrived: its cycle comes next.
                            Ok(None) => continue,
                            Err(Stop::Cancelled) => return,
                            // The fresh parse could not finish: the incremental result stands.
                            Err(_) => self.shared.mark_exact(&exact),
                        }
                        if let Some(n) = &self.notify {
                            n();
                        }
                    }
                }
                Err(Stop::Cancelled) => return,
                Err(Stop::Timeout) => {
                    return self.finish(Disabled::Timeout {
                        after: self.timeout,
                    });
                }
                Err(Stop::Failed) => return self.finish(Disabled::WorkerFailed),
                // Only `verify` parses can be superseded.
                Err(Stop::Superseded) => {}
            }
        }
    }

    /// The text again, from scratch. tree-sitter's incremental reparse equals a fresh parse
    /// for valid text, but its error recovery can settle differently once it reuses old
    /// subtrees (raw probe: php_only and twig trees differ after some edits, only inside ERROR
    /// nodes). So when the incremental result has errors, a fresh parse replaces it; it stops as
    /// soon as newer text arrives (`Ok(None)`), and the incremental state is then kept.
    fn verify(&mut self, config: &Arc<LanguageConfig>) -> Result<Option<Snapshot>, Stop> {
        let saved = (
            self.tree.take(),
            std::mem::take(&mut self.old_layers),
            self.root_records.take(),
        );
        self.verifying = true;
        let result = self.cycle(config);
        self.verifying = false;
        match result {
            Ok(snapshot) => Ok(Some(snapshot)),
            Err(stop) => {
                (self.tree, self.old_layers, self.root_records) = saved;
                match stop {
                    Stop::Superseded => Ok(None),
                    other => Err(other),
                }
            }
        }
    }

    fn finish(&self, reason: Disabled) {
        self.shared.disable(reason);
        if let Some(n) = &self.notify {
            n();
        }
    }

    fn absorb(&mut self, msg: Msg) {
        match msg {
            Msg::Edit {
                version,
                edits,
                text,
            } => {
                self.queued.extend(edits);
                self.text = text;
                self.version = version;
            }
            Msg::Reset { version, text } => {
                self.tree = None;
                self.old_layers.clear();
                self.root_records = None;
                self.edited.clear();
                self.queued.clear();
                self.applied = text.clone();
                self.text = text;
                self.version = version;
            }
        }
    }

    /// Brings the trees, layers and records from `applied` to `text`.
    fn apply_queued(&mut self) {
        let mut edits = std::mem::take(&mut self.queued);
        if edits.len() > MAX_SEPARATE_EDITS {
            edits = vec![covering_edit(&edits, &self.applied, &self.text)];
        }
        for e in &edits {
            let ie = e.input_edit();
            if let Some(tree) = &mut self.tree {
                tree.edit(&ie);
            }
            for layer in &mut self.old_layers {
                layer.dirty |= layer
                    .ranges
                    .iter()
                    .any(|r| e.start_byte <= r.end_byte && r.start_byte <= e.old_end_byte);
                layer.tree.edit(&ie);
                for r in &mut layer.ranges {
                    ie.edit_range(r);
                }
            }
            if let Some(records) = &mut self.root_records {
                // A match touching the edit is found again by the region query.
                records.retain(|r| !r.touches(e.start_byte, e.old_end_byte));
                for r in records.iter_mut() {
                    for range in &mut r.ranges {
                        ie.edit_range(range);
                    }
                    r.extent = (shift(r.extent.0, e, false), shift(r.extent.1, e, true));
                }
            }
            for (s, end) in &mut self.edited {
                *s = shift(*s, e, false);
                *end = shift(*end, e, true);
            }
            self.edited.push((e.start_byte, e.new_end_byte));
        }
        self.applied = self.text.clone();
    }

    fn cycle(&mut self, config: &Arc<LanguageConfig>) -> Result<Snapshot, Stop> {
        self.apply_queued();
        let old = match config.reparse {
            Reparse::Incremental => self.tree.take(),
            Reparse::Full => None,
        };
        let newer = self
            .verifying
            .then_some((&self.shared.latest, self.version));
        let root = parse(
            &mut self.parser,
            &self.text,
            &config.language,
            &[],
            old.as_ref(),
            Limits {
                timeout: self.timeout,
                cancel: &self.cancel,
                newer,
            },
        )?;
        // Where the injections of the root layer may have changed: the edited text and the
        // nodes whose structure changed.
        let mut regions = std::mem::take(&mut self.edited);
        if let Some(old) = &old {
            regions.extend(
                old.changed_ranges(&root)
                    .map(|r| (r.start_byte, r.end_byte)),
            );
        } else {
            self.root_records = None;
        }
        self.tree = Some(root.clone());
        let layers = self.layers(config.clone(), root, &regions)?;
        Ok(Snapshot {
            version: self.version,
            text: self.text.clone(),
            layers,
        })
    }

    /// The root layer and every embedded layer under it, breadth first (so a layer always comes
    /// after the one it is embedded in).
    fn layers(
        &mut self,
        root_config: Arc<LanguageConfig>,
        root: Tree,
        regions: &[(usize, usize)],
    ) -> Result<Vec<Layer>, Stop> {
        let len = self.text.len_bytes();
        let end = point_at(&self.text, len);
        let mut layers = vec![Layer {
            config: root_config,
            tree: root,
            ranges: vec![tree_sitter::Range {
                start_byte: 0,
                end_byte: len,
                start_point: tree_sitter::Point::new(0, 0),
                end_point: tree_sitter::Point::new(end.row, end.column),
            }],
            depth: 0,
        }];
        let mut old: HashMap<(String, usize), OldLayer> = self
            .old_layers
            .drain(..)
            .filter(|l| !l.ranges.is_empty())
            .map(|l| ((l.language.clone(), l.ranges[0].start_byte), l))
            .collect();
        let mut i = 0;
        while i < layers.len() && layers.len() < MAX_LAYERS {
            if layers[i].depth < MAX_DEPTH {
                let depth = layers[i].depth + 1;
                let records = if i == 0 {
                    self.root_injections(&layers[0], regions)?
                } else {
                    let deadline = Instant::now() + self.timeout;
                    match injection_records(
                        &layers[i],
                        &self.text,
                        &self.registry,
                        None,
                        &self.cancel,
                        deadline,
                    ) {
                        Ok(records) => records,
                        Err(Stop::Cancelled) => return Err(Stop::Cancelled),
                        Err(Stop::Superseded) => return Err(Stop::Superseded),
                        // Blocks inside this embedded layer stay in its colours.
                        Err(_) => Vec::new(),
                    }
                };
                for spec in group(&records) {
                    if layers.len() >= MAX_LAYERS {
                        break;
                    }
                    let Ok(config) = self.registry.config(&spec.language) else {
                        continue;
                    };
                    let previous = old.remove(&(spec.language.clone(), spec.ranges[0].start_byte));
                    let tree = match previous {
                        // Unchanged block (the common case while typing elsewhere): no parse.
                        // Bytes only: row/column of a range are labels, the text is the same.
                        Some(p) if !p.dirty && same_bytes(&p.ranges, &spec.ranges) => Ok(p.tree),
                        p => {
                            let base = match config.reparse {
                                Reparse::Incremental => p.as_ref().map(|p| &p.tree),
                                Reparse::Full => None,
                            };
                            let newer = self
                                .verifying
                                .then_some((&self.shared.latest, self.version));
                            parse(
                                &mut self.parser,
                                &self.text,
                                &config.language,
                                &spec.ranges,
                                base,
                                Limits {
                                    timeout: self.timeout,
                                    cancel: &self.cancel,
                                    newer,
                                },
                            )
                        }
                    };
                    match tree {
                        Ok(tree) => layers.push(Layer {
                            config,
                            tree,
                            ranges: spec.ranges,
                            depth,
                        }),
                        Err(Stop::Cancelled) => return Err(Stop::Cancelled),
                        Err(Stop::Superseded) => return Err(Stop::Superseded),
                        // An embedded block that cannot be parsed stays in its host's colours.
                        Err(_) => {}
                    }
                }
            }
            i += 1;
        }
        self.old_layers = layers[1..]
            .iter()
            .map(|l| OldLayer {
                language: l.config.id.clone(),
                ranges: l.ranges.clone(),
                tree: l.tree.clone(),
                dirty: false,
            })
            .collect();
        Ok(layers)
    }

    /// The root layer's injection matches: the previous ones outside `regions` kept, the
    /// query run again only over `regions` (a full-document query per keystroke costs hundreds
    /// of milliseconds on a multi-megabyte file).
    fn root_injections(
        &mut self,
        root: &Layer,
        regions: &[(usize, usize)],
    ) -> Result<Vec<Record>, Stop> {
        let deadline = Instant::now() + self.timeout;
        let records = match self.root_records.take() {
            Some(mut kept) => {
                // One byte wider on each side: the query finds a match only when its pattern's
                // top node overlaps the range, and a node ending exactly where an edit starts
                // does not overlap the edit.
                let len = self.text.len_bytes();
                let wide: Vec<(usize, usize)> = regions
                    .iter()
                    .map(|&(s, e)| (s.saturating_sub(1), (e + 1).min(len)))
                    .collect();
                let regions = merge_regions(&wide);
                kept.retain(|r| {
                    !regions
                        .iter()
                        .any(|&(s, e)| r.extent.0 < e && s < r.extent.1)
                });
                let found = injection_records(
                    root,
                    &self.text,
                    &self.registry,
                    Some(&regions),
                    &self.cancel,
                    deadline,
                )?;
                // A match found again replaces any kept one over the same text.
                kept.retain(|k| !found.iter().any(|f| k.overlaps(f)));
                kept.extend(found);
                sort_records(&mut kept);
                kept
            }
            None => injection_records(
                root,
                &self.text,
                &self.registry,
                None,
                &self.cancel,
                deadline,
            )?,
        };
        self.root_records = Some(records.clone());
        Ok(records)
    }
}

/// Document order, so the incremental and the full query give the same layer order.
fn sort_records(records: &mut [Record]) {
    records.sort_by_key(|r| (r.ranges[0].start_byte, r.ranges[0].end_byte, r.pattern));
}

/// Sorted, overlapping or touching regions joined, each at least one byte (a deletion leaves an
/// empty one).
fn merge_regions(regions: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut sorted: Vec<(usize, usize)> = regions.iter().map(|&(s, e)| (s, e.max(s + 1))).collect();
    sorted.sort_unstable();
    let mut out: Vec<(usize, usize)> = Vec::with_capacity(sorted.len());
    for (s, e) in sorted {
        match out.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => out.push((s, e)),
        }
    }
    out
}

/// A position of the text before `e` in the text after it; inside the replaced range it goes to
/// the range's start, or its new end for the end of a region.
fn shift(pos: usize, e: &Edit, end: bool) -> usize {
    if pos <= e.start_byte {
        pos
    } else if pos >= e.old_end_byte {
        pos - e.old_end_byte + e.new_end_byte
    } else if end {
        e.new_end_byte
    } else {
        e.start_byte
    }
}

/// One edit turning `before` into `after` that covers every edit of `edits` (applied in order,
/// each in the coordinates the previous one leaves).
fn covering_edit(edits: &[Edit], before: &Rope, after: &Rope) -> Edit {
    // (start, old end, new end) of the covered range so far.
    let (mut start, mut old_end, mut new_end) = (usize::MAX, 0, 0);
    for (i, e) in edits.iter().enumerate() {
        if i == 0 {
            (start, old_end, new_end) = (e.start_byte, e.old_end_byte, e.new_end_byte);
            continue;
        }
        // Past the covered range, the text before this edit is the original text shifted by
        // the covered range's growth.
        if e.old_end_byte > new_end {
            old_end += e.old_end_byte - new_end;
        }
        new_end = new_end.max(e.old_end_byte) - e.old_end_byte + e.new_end_byte;
        start = start.min(e.start_byte);
    }
    Edit {
        start_byte: start,
        old_end_byte: old_end,
        new_end_byte: new_end,
        start_point: point_at(before, start),
        old_end_point: point_at(before, old_end),
        new_end_point: point_at(after, new_end),
    }
}

/// The same bytes, whatever the row/column labels say.
fn same_bytes(a: &[tree_sitter::Range], b: &[tree_sitter::Range]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x.start_byte == y.start_byte && x.end_byte == y.end_byte)
}

/// When a parse call stops early.
struct Limits<'a> {
    timeout: Duration,
    cancel: &'a AtomicBool,
    /// (the caller's newest version, the version being parsed): stop once they differ.
    newer: Option<(&'a AtomicU64, u64)>,
}

/// One parse call, bounded by `limits.timeout` and cancellable from the caller's side.
fn parse(
    parser: &mut Parser,
    text: &Rope,
    language: &Language,
    ranges: &[tree_sitter::Range],
    old: Option<&Tree>,
    limits: Limits,
) -> Result<Tree, Stop> {
    let Limits {
        timeout,
        cancel,
        newer,
    } = limits;
    parser.set_language(language).map_err(|_| Stop::Failed)?;
    parser
        .set_included_ranges(ranges)
        .map_err(|_| Stop::Failed)?;
    let deadline = Instant::now() + timeout;
    let timed_out = Cell::new(false);
    let superseded = || newer.is_some_and(|(latest, v)| latest.load(Ordering::Relaxed) != v);
    let mut progress = |_: &ParseState| {
        if cancel.load(Ordering::Relaxed) || superseded() {
            ControlFlow::Break(())
        } else if Instant::now() >= deadline {
            timed_out.set(true);
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    let len = text.len_bytes();
    let mut input = |byte: usize, _: tree_sitter::Point| -> &[u8] {
        if byte >= len {
            return &[];
        }
        let (chunk, start, _, _) = text.chunk_at_byte(byte);
        &chunk.as_bytes()[byte - start..]
    };
    let options = ParseOptions::new().progress_callback(&mut progress);
    match parser.parse_with_options(&mut input, old, Some(options)) {
        Some(tree) => Ok(tree),
        None => {
            // A stopped parse would otherwise resume on the next call.
            parser.reset();
            if cancel.load(Ordering::Relaxed) {
                Err(Stop::Cancelled)
            } else if superseded() {
                Err(Stop::Superseded)
            } else if timed_out.get() {
                Err(Stop::Timeout)
            } else {
                Err(Stop::Failed)
            }
        }
    }
}

struct Spec {
    language: String,
    ranges: Vec<tree_sitter::Range>,
}

/// One injection match: an embedded region before combined patterns are grouped into layers.
#[derive(Clone)]
struct Record {
    pattern: usize,
    language: String,
    combined: bool,
    ranges: Vec<tree_sitter::Range>,
    /// Bytes of every node the match captured (the language name, attributes, the content):
    /// an edit anywhere in it can change the match.
    extent: (usize, usize),
}

impl Record {
    fn touches(&self, start: usize, end: usize) -> bool {
        self.extent.0 <= end && start <= self.extent.1
    }

    /// Whether the two embed some of the same text.
    fn overlaps(&self, other: &Record) -> bool {
        self.ranges.iter().any(|a| {
            other
                .ranges
                .iter()
                .any(|b| a.start_byte < b.end_byte && b.start_byte < a.end_byte)
        })
    }
}

/// The injection matches of one layer, from its injections query: over the whole layer, or
/// only over `regions` (a match whose top node overlaps a region is found as a whole). Bounded
/// by `deadline` and stopped by `cancel`.
fn injection_records(
    layer: &Layer,
    text: &Rope,
    registry: &LanguageRegistry,
    regions: Option<&[(usize, usize)]>,
    cancel: &AtomicBool,
    deadline: Instant,
) -> Result<Vec<Record>, Stop> {
    let Some(inj) = &layer.config.injections else {
        return Ok(Vec::new());
    };
    let Some(content) = inj.content else {
        return Ok(Vec::new());
    };
    let (first, last) = (layer.ranges[0], layer.ranges[layer.ranges.len() - 1]);
    let whole = [(first.start_byte, last.end_byte)];
    let mut records: Vec<Record> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut cursor = QueryCursor::new();
    let stop: Cell<Option<Stop>> = Cell::new(None);
    let mut progress = |_: &QueryCursorState| {
        if cancel.load(Ordering::Relaxed) {
            stop.set(Some(Stop::Cancelled));
            ControlFlow::Break(())
        } else if Instant::now() >= deadline {
            stop.set(Some(Stop::Timeout));
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    };
    for &(start, end) in regions.unwrap_or(&whole) {
        cursor.set_byte_range(start..end);
        let options = QueryCursorOptions::new().progress_callback(&mut progress);
        let mut matches = cursor.matches_with_options(
            &inj.query,
            layer.tree.root_node(),
            RopeText(text),
            options,
        );
        while let Some(m) = matches.next() {
            if !predicates_hold(&inj.preds[m.pattern_index], m, text) {
                continue;
            }
            let pattern = &inj.patterns[m.pattern_index];
            let mut name = pattern.language.clone();
            let mut nodes = Vec::new();
            let mut extent = (usize::MAX, 0);
            for c in m.captures() {
                extent = (
                    extent.0.min(c.node.start_byte()),
                    extent.1.max(c.node.end_byte()),
                );
                if c.index == content {
                    nodes.push(c.node);
                } else if Some(c.index) == inj.language {
                    let mut t = node_text(text, c.node.byte_range());
                    if let Some((re, replacement)) = &pattern.gsub {
                        t = re.replace(&t, replacement.as_str()).into_owned();
                    }
                    name = Some(t);
                }
            }
            if pattern.same_language {
                name = Some(layer.config.id.clone());
            }
            let Some(language) = name.as_deref().and_then(|n| registry.language_for_name(n)) else {
                continue;
            };
            if !registry.has_grammar(language) {
                continue;
            }
            let mut ranges = Vec::new();
            for node in nodes {
                content_ranges(node, pattern, &layer.ranges, &mut ranges);
            }
            if ranges.is_empty() {
                continue;
            }
            normalize(&mut ranges);
            // Two regions can report a match spanning both.
            let key = (
                m.pattern_index,
                extent,
                ranges[0].start_byte,
                nodes_end(&ranges),
            );
            if regions.is_some() && !seen.insert(key) {
                continue;
            }
            records.push(Record {
                pattern: m.pattern_index,
                language: language.to_owned(),
                combined: pattern.combined,
                ranges,
                extent,
            });
        }
        if let Some(why) = stop.take() {
            return Err(why);
        }
    }
    sort_records(&mut records);
    Ok(records)
}

fn nodes_end(ranges: &[tree_sitter::Range]) -> usize {
    ranges.last().map_or(0, |r| r.end_byte)
}

/// Layers to parse: one per plain match, one per (pattern, language) of combined matches.
fn group(records: &[Record]) -> Vec<Spec> {
    let mut specs = Vec::new();
    let mut combined: Vec<(usize, &str, Vec<tree_sitter::Range>)> = Vec::new();
    for r in records {
        if r.combined {
            match combined
                .iter_mut()
                .find(|(p, l, _)| *p == r.pattern && *l == r.language)
            {
                Some((_, _, ranges)) => ranges.extend_from_slice(&r.ranges),
                None => combined.push((r.pattern, &r.language, r.ranges.clone())),
            }
        } else {
            specs.push(Spec {
                language: r.language.clone(),
                ranges: r.ranges.clone(),
            });
        }
    }
    for (_, language, mut ranges) in combined {
        normalize(&mut ranges);
        specs.push(Spec {
            language: language.to_owned(),
            ranges,
        });
    }
    specs
}

/// The ranges of an injection's content node: without its children unless the pattern says
/// `injection.include-children`, shifted by `#offset!`, clipped to the host layer's ranges.
fn content_ranges(
    node: Node,
    pattern: &InjectionPattern,
    host: &[tree_sitter::Range],
    out: &mut Vec<tree_sitter::Range>,
) {
    let mut start = (node.start_byte(), node.start_position());
    let mut end = (node.end_byte(), node.end_position());
    if let Some((a, b)) = pattern.offset {
        let shift = |(byte, mut p): (usize, tree_sitter::Point), d: isize| {
            p.column = (p.column as isize + d).max(0) as usize;
            ((byte as isize + d).max(0) as usize, p)
        };
        start = shift(start, a);
        end = shift(end, b);
        if start.0 >= end.0 {
            return;
        }
    }
    let mut pieces = Vec::new();
    if pattern.include_children {
        pieces.push(range(start, end));
    } else {
        let mut at = start;
        let mut cursor = node.walk();
        // Named children only: grammars such as Markdown lex brackets inside a code block as
        // anonymous tokens, which are part of the embedded text.
        for child in node.named_children(&mut cursor) {
            if child.start_byte() > at.0 {
                pieces.push(range(at, (child.start_byte(), child.start_position())));
            }
            if child.end_byte() > at.0 {
                at = (child.end_byte(), child.end_position());
            }
        }
        if end.0 > at.0 {
            pieces.push(range(at, end));
        }
    }
    for p in pieces {
        let from = host.partition_point(|h| h.end_byte <= p.start_byte);
        for h in &host[from..] {
            if h.start_byte >= p.end_byte {
                break;
            }
            let (s, sp) = if p.start_byte >= h.start_byte {
                (p.start_byte, p.start_point)
            } else {
                (h.start_byte, h.start_point)
            };
            let (e, ep) = if p.end_byte <= h.end_byte {
                (p.end_byte, p.end_point)
            } else {
                (h.end_byte, h.end_point)
            };
            if s < e {
                out.push(tree_sitter::Range {
                    start_byte: s,
                    end_byte: e,
                    start_point: sp,
                    end_point: ep,
                });
            }
        }
    }
}

fn range(
    start: (usize, tree_sitter::Point),
    end: (usize, tree_sitter::Point),
) -> tree_sitter::Range {
    tree_sitter::Range {
        start_byte: start.0,
        end_byte: end.0.max(start.0),
        start_point: start.1,
        end_point: end.1,
    }
}

/// Sorted and non-overlapping, as `Parser::set_included_ranges` requires.
fn normalize(ranges: &mut Vec<tree_sitter::Range>) {
    ranges.sort_by_key(|r| r.start_byte);
    let mut out: Vec<tree_sitter::Range> = Vec::with_capacity(ranges.len());
    for mut r in ranges.drain(..) {
        if let Some(prev) = out.last() {
            if r.end_byte <= prev.end_byte {
                continue;
            }
            if r.start_byte < prev.end_byte {
                r.start_byte = prev.end_byte;
                r.start_point = prev.end_point;
            }
        }
        out.push(r);
    }
    *ranges = out;
}

/// A capture chosen for one node.
struct Painted {
    start: usize,
    end: usize,
    priority: u32,
    paint: Paint,
}

impl Snapshot {
    /// Spans of this snapshot's text in `range`: each layer paints its captures (outer nodes
    /// first, so inner ones win), embedded layers after their hosts.
    fn spans(&self, range: Range<usize>, timeout: Duration) -> Vec<(Range<usize>, Capture)> {
        let len = self.text.len_bytes();
        let range = range.start.min(len)..range.end.min(len);
        if range.is_empty() {
            return Vec::new();
        }
        let deadline = Instant::now() + timeout;
        let mut buf = vec![NO_CAPTURE; range.len()];
        let mut cursor = QueryCursor::new();
        for layer in &self.layers {
            let (first, last) = (&layer.ranges[0], &layer.ranges[layer.ranges.len() - 1]);
            if first.start_byte >= range.end || last.end_byte <= range.start {
                continue;
            }
            if !layer.paint(&mut cursor, &self.text, &range, &mut buf, deadline) {
                break;
            }
        }
        let mut out: Vec<(Range<usize>, Capture)> = Vec::new();
        let mut i = 0;
        while i < buf.len() {
            let v = buf[i];
            let mut j = i + 1;
            while j < buf.len() && buf[j] == v {
                j += 1;
            }
            if v != NO_CAPTURE {
                out.push((range.start + i..range.start + j, Capture::ALL[v as usize]));
            }
            i = j;
        }
        out
    }
}

impl Layer {
    /// Paints this layer's captures into `buf` (bytes of `range`). False when the query ran
    /// out of time; what was found until then is painted.
    fn paint(
        &self,
        cursor: &mut QueryCursor,
        text: &Rope,
        range: &Range<usize>,
        buf: &mut [u8],
        deadline: Instant,
    ) -> bool {
        let config = &*self.config;
        let timed_out = Cell::new(false);
        let mut progress = |_: &QueryCursorState| {
            if Instant::now() >= deadline {
                timed_out.set(true);
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        };
        cursor.set_byte_range(range.clone());
        let mut painted: Vec<Painted> = Vec::new();
        let mut by_node: HashMap<usize, usize> = HashMap::new();
        {
            let options = QueryCursorOptions::new().progress_callback(&mut progress);
            let mut captures = cursor.captures_with_options(
                &config.highlights,
                self.tree.root_node(),
                RopeText(text),
                options,
            );
            while let Some((m, index)) = captures.next() {
                let c = m.captures()[*index];
                let paint = config.paints[c.index as usize];
                if paint == Paint::Ignore {
                    continue;
                }
                let preds = &config.highlight_preds[m.pattern_index];
                if !preds.is_empty() && !predicates_hold(preds, m, text) {
                    continue;
                }
                let priority = config.priority[m.pattern_index];
                match by_node.entry(c.node.id()) {
                    MapEntry::Occupied(o) => {
                        let p = &mut painted[*o.get()];
                        if priority >= p.priority {
                            p.priority = priority;
                            p.paint = paint;
                        }
                    }
                    MapEntry::Vacant(v) => {
                        v.insert(painted.len());
                        painted.push(Painted {
                            start: c.node.start_byte(),
                            end: c.node.end_byte(),
                            priority,
                            paint,
                        });
                    }
                }
            }
        }
        // Stable: nodes with the same extent keep the query's order (parent before child).
        painted.sort_by_key(|p| (p.start, Reverse(p.end)));
        for p in &painted {
            let value = match p.paint {
                Paint::Colour(c) => c as u8,
                Paint::Clear | Paint::Ignore => NO_CAPTURE,
            };
            let (s, e) = (p.start.max(range.start), p.end.min(range.end));
            if s >= e {
                continue;
            }
            // An embedded layer's node can span text of the host between its ranges.
            let from = self.ranges.partition_point(|r| r.end_byte <= s);
            for r in &self.ranges[from..] {
                if r.start_byte >= e {
                    break;
                }
                let (a, b) = (s.max(r.start_byte), e.min(r.end_byte));
                if a < b {
                    buf[a - range.start..b - range.start].fill(value);
                }
            }
        }
        !timed_out.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn markdown_layer(registry: &LanguageRegistry, text: &Rope) -> Layer {
        let config = registry.config("markdown").unwrap();
        let mut parser = Parser::new();
        let cancel = AtomicBool::new(false);
        let tree = parse(
            &mut parser,
            text,
            &config.language,
            &[],
            None,
            Limits {
                timeout: Duration::from_secs(60),
                cancel: &cancel,
                newer: None,
            },
        )
        .ok()
        .unwrap();
        let end = point_at(text, text.len_bytes());
        Layer {
            config,
            tree,
            ranges: vec![tree_sitter::Range {
                start_byte: 0,
                end_byte: text.len_bytes(),
                start_point: tree_sitter::Point::new(0, 0),
                end_point: tree_sitter::Point::new(end.row, end.column),
            }],
            depth: 0,
        }
    }

    #[test]
    fn injection_queries_stop_on_cancel_and_at_their_deadline() {
        let registry = LanguageRegistry::builtin();
        let text = Rope::from_str(&"Some *text* here.\n\n".repeat(20_000));
        let layer = markdown_layer(&registry, &text);
        let later = Instant::now() + Duration::from_secs(60);
        let all = injection_records(
            &layer,
            &text,
            &registry,
            None,
            &AtomicBool::new(false),
            later,
        );
        assert_eq!(all.ok().map(|r| r.len()), Some(20_000));
        let cancelled = injection_records(
            &layer,
            &text,
            &registry,
            None,
            &AtomicBool::new(true),
            later,
        );
        assert!(matches!(cancelled, Err(Stop::Cancelled)));
        let past = Instant::now();
        let late = injection_records(
            &layer,
            &text,
            &registry,
            None,
            &AtomicBool::new(false),
            past,
        );
        assert!(matches!(late, Err(Stop::Timeout)));
    }

    #[test]
    fn many_edits_fold_into_one_covering_edit() {
        let before = Rope::from_str("one two\nthree four\nfive");
        // Replace "two" (4..7) with "2", "four" (14..18) with "4!", insert "x" at 0.
        let edits = Edit::from_changes(&before, [(0, 0, "x"), (4, 7, "2"), (14, 18, "4!")]);
        let after = Rope::from_str("xone 2\nthree 4!\nfive");
        let e = covering_edit(&edits, &before, &after);
        assert_eq!((e.start_byte, e.old_end_byte, e.new_end_byte), (0, 18, 15));
        assert_eq!(e.old_end_point, point_at(&before, 18));
        assert_eq!(e.new_end_point, point_at(&after, 15));
        // The text outside the covered range is the same before and after.
        assert_eq!(before.byte_slice(18..), after.byte_slice(15..));
        // One edit only: the same as itself.
        let one = Edit::from_chars(&before, 4, 7, "2");
        let after_one = Rope::from_str("one 2\nthree four\nfive");
        assert_eq!(covering_edit(&[one], &before, &after_one), one);
    }
}
