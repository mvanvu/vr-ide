# Languages

vr-ide colours files with [tree-sitter](https://tree-sitter.github.io/) grammars compiled into the
binary. The table below lists every language, the file names it is picked for by default (the
built-in table is `crates/vr-config/defaults/languages.toml`), where its grammar comes from and
under which licence. Every grammar is MIT-licensed.

| Language id | Extensions | File names / globs | Grammar source | Version |
|---|---|---|---|---|
| `php` | php, phtml, php3, php4, php5, php7, phps | | crate `tree-sitter-php` | 0.25.2 |
| `php_only` | none (see below) | | crate `tree-sitter-php` (PHP without the surrounding HTML) | 0.25.2 |
| `blade` | | `*.blade.php` | vendored [EmranMR/tree-sitter-blade](https://github.com/EmranMR/tree-sitter-blade), layered on the HTML queries | commit b5291d1 |
| `twig` | twig | | vendored [gbprod/tree-sitter-twig](https://github.com/gbprod/tree-sitter-twig) | commit 2208d2a |
| `javascript` | js, mjs, cjs, jsx | | crate `tree-sitter-javascript` | 0.25.0 |
| `typescript` | ts, mts, cts | | crate `tree-sitter-typescript` | 0.23.2 |
| `tsx` | tsx | | crate `tree-sitter-typescript` | 0.23.2 |
| `vue` | vue | | vendored [tree-sitter-grammars/tree-sitter-vue](https://github.com/tree-sitter-grammars/tree-sitter-vue) | commit ce8011a |
| `html` | html, htm, xhtml | | crate `tree-sitter-html` | 0.23.2 |
| `css` | css | | crate `tree-sitter-css` | 0.25.0 |
| `scss` | scss | | crate `tree-sitter-scss` (CSS queries underneath) | 1.0.0 |
| `dart` | dart | | crate `tree-sitter-dart` | 0.3.0 |
| `go` | go | | crate `tree-sitter-go` | 0.25.0 |
| `python` | py, pyi, pyw | `SConstruct`, `SConscript` | crate `tree-sitter-python` | 0.25.0 |
| `rust` | rs | | crate `tree-sitter-rust` | 0.24.2 |
| `json` | json, jsonc, json5, webmanifest | `.prettierrc`, `.babelrc`, `.eslintrc`, `composer.lock`, `*.code-workspace` | crate `tree-sitter-json` | 0.24.8 |
| `yaml` | yaml, yml | | crate `tree-sitter-yaml` | 0.7.2 |
| `toml` | toml | `Cargo.lock`, `Pipfile`, `poetry.lock` | crate `tree-sitter-toml-ng` | 0.7.0 |
| `markdown` | md, markdown, mdx | | crate `tree-sitter-md` (block grammar) | 0.5.4 |
| `markdown_inline` | none (see below) | | crate `tree-sitter-md` (inline grammar) | 0.5.4 |

Notes:

- `php_only` and `markdown_inline` are the halves that other languages embed (Blade directives,
  Markdown paragraphs). They have no extension of their own: a `.php` file is `php`, which
  handles HTML around `<?php ... ?>`, so a snippet without `<?php` shows as HTML. To open a file
  as one of them, name it in `languages.toml` (see below).
- A longer extension wins over a shorter one (`blade.php` before `php`), and a file name or glob
  wins over an extension.
- Embedded code is coloured in its own language: `<script>` and `<style>` in HTML and Vue, HTML
  around PHP, fenced code in Markdown (by its info string or an alias such as `js`, `ts`, `py`).
- A file with an unknown extension opens as plain text, with no notice.
- The grammars are linked into the binary (they account for about 14.7 MB of the release size) and each is
  loaded on first use, so a session that opens no source file pays nothing at startup. A build
  can leave groups out with `--no-default-features --features <list>` (one cargo feature per
  grammar group, see `crates/vr-syntax/Cargo.toml`); a language in `languages.toml` whose grammar
  is missing is named in a startup notice and its files open plain.

## Changing the mapping: `languages.toml`

`~/.config/vr-ide/languages.toml` (or `$VR_IDE_CONFIG_DIR/languages.toml`) overrides the built-in
table field by field; a new `[id]` table adds a language that uses a compiled-in grammar.

```toml
# Colour .es6 files as JavaScript (extensions replaces the built-in list, so repeat it).
[javascript]
extensions = ["js", "mjs", "cjs", "jsx", "es6"]

# CakePHP templates are PHP.
[php]
files = ["*.ctp"]

# Colour the snippets in this folder as bare PHP (no `<?php` needed).
[php_only]
files = ["snippet-*.php"]

# A new language backed by an existing grammar.
[jinja]
grammar = "twig"
extensions = ["j2", "jinja"]
```

Fields: `extensions` (without the dot), `files` (exact names or `*` / `?` globs), `aliases` (names
used by embedded code), `grammar` (defaults to the id).

A table you write takes its extensions, file names and aliases away from every other language (a `[jsonc]` with `extensions = ["json"]` removes `json` from the built-in `json`). A bad table or field is reported at startup and skipped; the built-in value stays.

## Large files: `max_highlight_bytes`

`max_highlight_bytes` in `~/.config/vr-ide/settings.toml` (default 10 MiB) is the largest file
that is highlighted. A larger file opens as plain text with a one-line notice, so the 100 MB
case stays fast. Smaller files are parsed in the background: the first frame is plain and the
colours arrive when the parse lands.

```toml
# ~/.config/vr-ide/settings.toml
max_highlight_bytes = 2097152   # 2 MiB
```

## Tests

`crates/vr-ide/src/language_snapshot_tests.rs` opens each sample of
`crates/vr-syntax/tests/samples/` at 80x24 in the synthwave theme and snapshots a class map
(`k` keyword, `s` string, `c` comment, `f` function, `t` type, `n` number, `C` constant,
`v` builtin variable, `p` property, `g` tag, `a` attribute, `o` operator, `u` punctuation,
`e` escape, `.` plain) under `crates/vr-ide/src/snapshots/`.
