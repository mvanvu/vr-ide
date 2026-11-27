# Plan — M4 Syntax highlighting

Spec: `.ai-dev-team/workspace/01_spec_M4.md`. Follows 02_plan.md Global Constraints (no blocking I/O on
the UI thread, thiserror in libraries, insta snapshots, VS Code keys).

## Architecture
- New crate `crates/vr-syntax` (library, no UI types):
  - `LanguageRegistry`: built from `languages.toml` (vr-config) + the compiled-in grammar table
    (cargo feature per language group, all on by default, C2); lazy `Language` + `HighlightConfiguration`
    per language on first use.
  - `Highlighter` per document: owns the tree-sitter `Tree`, applies `InputEdit`s from vr-core
    transactions, reparses incrementally; exposes `spans(byte_range) -> impl Iterator<(Range, Capture)>`
    for the visible window, including injections.
  - Parsing runs on a worker (one per document or a small pool) with versioned results; the UI keeps
    the last spans that match the current text version, shifted by pending edits, and draws plain for
    regions not yet parsed. Parse timeout per call; over `max_highlight_bytes` → disabled.
- vr-config: `languages.toml` defaults + loader (same override rules as settings/keymap), validation
  notices.
- vr-ide: document → highlighter wiring (open, edit, reload, Save As rename changes language, close),
  `editor_view` draws spans under selection / find-match / cursor line; `Theme` gains a capture →
  style map for every theme; preview screens gain a highlighted file.

## Contracts
- `vr_syntax::Capture` — a small enum of capture classes (Keyword, String, Comment, Function, Type,
  Number, Constant, VariableBuiltin, Property, Tag, Attribute, Operator, Punctuation, Escape, Other),
  mapped from tree-sitter capture names by prefix (`keyword.*` → Keyword …).
- `Theme::syntax(Capture) -> Style` (vr-ide).
- `vr_core` edit notification: the existing Transaction carries enough to build `InputEdit`
  (byte offsets + row/col before/after); if not, M4-2 adds an additive accessor (BASE-2: list it).

## Decisions
- tree-sitter via the `tree-sitter` + `tree-sitter-highlight` crates (highlight queries and injections
  for free); grammar crates from crates.io, pinned, all at one tree-sitter ABI. Grammars without a
  maintained crate (likely Blade, Twig, Vue, SCSS, Dart) are vendored only if a licence-compatible
  source builds with the `cc` crate; otherwise listed as "not compiled in" (AC-13e) and reported to the
  user — M4-1 decides per language with evidence.
- Lazy per-language loading keeps AC-1; binary size growth is reported in the bench.

## Tasks (board rows M4-1 … M4-5)
1. **M4-1 (be)** vr-syntax scaffold + grammar inventory: workspace member, tree-sitter version,
   one test per D7 language proving its grammar parses a sample and its highlights query compiles;
   a table in the notes (language → crate/vendored → licence → status). Root `Cargo.toml`,
   `Cargo.lock`, `crates/vr-syntax/**`, samples under `crates/vr-syntax/tests/samples/`.
2. **M4-2 (be)** Highlighter engine: registry, `languages.toml` (vr-config), incremental reparse from
   transactions, worker + versioning, timeout, size limit, injections, `spans()`; racing/perf tests
   (5 MB parse off-thread, edit-to-spans latency).
3. **M4-3 (fe)** Editor wiring + theme captures: document lifecycle hooks, editor_view drawing,
   `Theme::syntax` for dracula (post-T15 colours) / dark / light with the T13 contrast tests extended
   to syntax colours on bg and on selection, previews with a highlighted file, notices (AC-13c, 13e).
4. **M4-4 (fe)** Language samples and per-language snapshots (AC-13, 13a), README/docs
   (languages.toml, list of languages), bench (AC-1 startup, RSS, binary size).
5. Sign-off qc + Phase 5 (correctness + ui).

Order: M4-1 → M4-2 → M4-3 → M4-4. M4-1 may start before T14/T15 merge (own crate, root manifest
only; neither T14 nor T15 edits Cargo.toml/Cargo.lock). M4-3 waits for T14 and the T15 implement step
(theme.rs, editor_view.rs).

## Risks
- Grammar ABI mismatches between crates → pin one tree-sitter version, vendor where needed.
- Highlight cost on long lines / huge files → visible-window spans only, size limit, timeout.
- Injection-heavy files (Vue, PHP+HTML) are the slowest; measured in M4-2.
