# Spec — M4 Syntax highlighting

flags: has_ui=true (terminal UI), security_sensitive=false, platforms=linux
source: 01_spec.md scope item 4, D7, C2, AC-13, AC-18; user 2026-11-24 22:05 ("follow your suggestions", D147)

## Goal
Files of the D7 languages are coloured by syntax in the editor, using tree-sitter grammars compiled
into the binary, with colours taken from the active theme. Large files and unknown extensions stay
fast and plain.

## Scope
- Languages (D7): PHP, Blade and Twig (where a usable grammar exists), JavaScript, TypeScript, TSX,
  Vue, HTML, CSS, SCSS, Dart, Go, Python, Rust, JSON, YAML, TOML, Markdown.
- `languages.toml` (built-in defaults in vr-config, user override like settings/keymap): extension
  and file-name patterns → language id → grammar; adding a language whose grammar is compiled in is
  config only. The LSP command column is reserved for M8 (parsed, unused).
- Embedded languages through tree-sitter injections where the grammar ships them: HTML `<script>` /
  `<style>`, Vue SFC blocks, PHP inside HTML, Markdown fenced code (by info string).
- Incremental reparse on every edit (Transaction), off the UI thread for the initial parse.
- Theme: every theme (dracula and its T15 replacement, dark, light) maps highlight captures
  (keyword, string, comment, function, type, number, constant, variable.builtin, property, tag,
  attribute, operator, punctuation, escape, …) to styles; selection, find matches and the cursor line
  stay readable on top of syntax colours.
- Setting `max_highlight_bytes` (existing, default 10 MiB): larger files open as plain text with a
  one-line notice.

## Out of scope
Semantic (LSP) colouring (M8), folding, indentation by grammar, bracket matching, user-supplied
grammar libraries loaded at runtime.

## Acceptance criteria
- AC-13: Given a sample file of each D7 language that has a compiled grammar, When it is opened,
  Then keywords, strings, comments and at least one more capture class render in their theme
  colours; an unknown extension opens as plain text with no error.
- AC-13a: Given an HTML file with `<script>`/`<style>`, a Vue SFC, a PHP file mixing HTML, and a
  Markdown file with a fenced `rust` block, Then the embedded parts use their own language colours.
- AC-13b: Given a highlighted file, When the user types, deletes, pastes or undoes, Then colours are
  correct for the changed region on the next frame, and keystroke-to-frame stays within the existing
  long-line / large-file budgets (render_large_file_fast, long_line_keystroke_to_frame_fast).
- AC-13c: Given a file larger than `max_highlight_bytes`, Then it opens plain with a notice; AC-18
  (100 MB file opens and scrolls without freezing) still passes.
- AC-13d: Given a 5 MB source file under the limit, When opened, Then the first frame is drawn
  without waiting for the parse (plain first, coloured when the background parse lands), and the UI
  never blocks on parsing.
- AC-13e: Given a language listed in `languages.toml` whose grammar is not compiled in, Then a startup
  notice names it and files open plain.
- AC-1 still holds: startup first frame < 100 ms and RSS < 60 MB on the bench project (grammars are
  loaded lazily, per language, on first use).

## Edge cases
Invalid UTF-8 / mixed encodings (document already decodes), CRLF files, a 10 MB single line, a parse
that times out (tree-sitter timeout → stay plain for that file, notice), files changed on disk and
reloaded, project replace into an open buffer, read mode (T14) — colours do not depend on mode.

## Open questions
None (D147: defaults taken).
