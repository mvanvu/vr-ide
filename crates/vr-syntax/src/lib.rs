//! Compiled-in tree-sitter grammars for the D7 languages.
//!
//! The table maps a language id to its `tree_sitter::Language` and the highlights / injections /
//! locals query sources. Each grammar group sits behind a cargo feature (all on by default, C2).
//! Nothing here is loaded eagerly: `Grammar::language` and the query getters are cheap calls the
//! registry (M4-2) makes on first use of a language.

use tree_sitter::Language;

/// Every D7 language id, in the order the inventory reports them.
pub const D7_LANGUAGES: &[&str] = &[
    "php",
    "php_only",
    "blade",
    "twig",
    "javascript",
    "typescript",
    "tsx",
    "vue",
    "html",
    "css",
    "scss",
    "dart",
    "go",
    "python",
    "rust",
    "json",
    "yaml",
    "toml",
    "markdown",
    "markdown_inline",
];

/// Where a grammar and its queries come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Source {
    /// crates.io crate name, or `vendor/<dir>` for a vendored generated parser.
    pub origin: &'static str,
    pub version: &'static str,
    pub licence: &'static str,
    /// Where the query files come from (the grammar crate or the upstream repo).
    pub queries: &'static str,
}

/// A compiled-in grammar.
#[derive(Debug)]
pub struct Grammar {
    pub id: &'static str,
    /// The cargo feature that compiles it in.
    pub feature: &'static str,
    language: fn() -> Language,
    highlights: &'static [&'static str],
    injections: &'static [&'static str],
    locals: &'static [&'static str],
}

impl Grammar {
    pub fn language(&self) -> Language {
        (self.language)()
    }

    /// Kept in one table (SOURCES) so a grammar and the inventory cannot disagree.
    pub fn source(&self) -> Source {
        source_of(self.id)
    }

    /// Highlights query. Layered grammars (TypeScript on JavaScript, Blade on HTML, Vue on
    /// html_tags, SCSS on CSS) join their parts base first: tree-sitter-highlight 0.27 gives the
    /// last pattern matching a node precedence, so the more specific part must come last.
    ///
    /// nvim-style queries tag nodes with a second, colourless capture (`(comment) @comment
    /// @spell`); tree-sitter-highlight would let that later capture replace the colour, so those
    /// spell-check markers are removed.
    pub fn highlights_query(&self) -> String {
        strip_spell_captures(&self.highlights.join("\n"))
    }

    pub fn injections_query(&self) -> String {
        self.injections.join("\n")
    }

    pub fn locals_query(&self) -> String {
        self.locals.join("\n")
    }
}

/// Removes the `@spell` / `@nospell` captures, leaving the rest of each pattern as is.
fn strip_spell_captures(query: &str) -> String {
    let mut out = String::with_capacity(query.len());
    let mut rest = query;
    while let Some(at) = rest.find('@') {
        let (before, from_at) = rest.split_at(at);
        let name_len = from_at[1..]
            .find(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '.' | '-')))
            .unwrap_or(from_at.len() - 1);
        let name = &from_at[1..1 + name_len];
        if matches!(name, "spell" | "nospell") {
            out.push_str(before.trim_end_matches([' ', '\t']));
        } else {
            out.push_str(before);
            out.push('@');
            out.push_str(name);
        }
        rest = &from_at[1 + name_len..];
    }
    out.push_str(rest);
    out
}

/// Build status of a D7 language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    CompiledIn,
    /// Its cargo feature is off in this build.
    FeatureOff {
        feature: &'static str,
    },
}

/// One inventory row per D7 language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub id: &'static str,
    pub status: Status,
    pub source: Source,
}

// Generated parsers vendored under `vendor/` and compiled by build.rs.
#[cfg(feature = "blade")]
unsafe extern "C" {
    fn tree_sitter_blade() -> *const ();
}
#[cfg(feature = "twig")]
unsafe extern "C" {
    fn tree_sitter_twig() -> *const ();
}
#[cfg(feature = "vue")]
unsafe extern "C" {
    fn tree_sitter_vue() -> *const ();
}

// Unused only in a build with every grammar feature off.
#[allow(unused_macros)]
macro_rules! vendored_query {
    ($dir:literal, $file:literal) => {
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/vendor/",
            $dir,
            "/queries/",
            $file
        ))
    };
}

const fn crate_source(origin: &'static str, version: &'static str) -> Source {
    Source {
        origin,
        version,
        licence: "MIT",
        queries: "grammar crate",
    }
}

const fn vendored_source(origin: &'static str, version: &'static str) -> Source {
    Source {
        origin,
        version,
        licence: "MIT",
        queries: "upstream repo (vendored with the parser)",
    }
}

/// Source and feature of every D7 language, compiled in or not (the inventory needs both).
const SOURCES: &[(&str, &str, Source)] = &[
    ("php", "php", crate_source("tree-sitter-php", "0.25.2")),
    ("php_only", "php", crate_source("tree-sitter-php", "0.25.2")),
    (
        "blade",
        "blade",
        vendored_source("vendor/blade (EmranMR/tree-sitter-blade)", "b5291d1"),
    ),
    (
        "twig",
        "twig",
        vendored_source("vendor/twig (gbprod/tree-sitter-twig)", "2208d2a"),
    ),
    (
        "javascript",
        "javascript",
        crate_source("tree-sitter-javascript", "0.25.0"),
    ),
    (
        "typescript",
        "typescript",
        crate_source("tree-sitter-typescript", "0.23.2"),
    ),
    (
        "tsx",
        "typescript",
        crate_source("tree-sitter-typescript", "0.23.2"),
    ),
    (
        "vue",
        "vue",
        vendored_source(
            "vendor/vue (tree-sitter-grammars/tree-sitter-vue)",
            "ce8011a",
        ),
    ),
    ("html", "html", crate_source("tree-sitter-html", "0.23.2")),
    ("css", "css", crate_source("tree-sitter-css", "0.25.0")),
    ("scss", "scss", crate_source("tree-sitter-scss", "1.0.0")),
    ("dart", "dart", crate_source("tree-sitter-dart", "0.3.0")),
    ("go", "go", crate_source("tree-sitter-go", "0.25.0")),
    (
        "python",
        "python",
        crate_source("tree-sitter-python", "0.25.0"),
    ),
    ("rust", "rust", crate_source("tree-sitter-rust", "0.24.2")),
    ("json", "json", crate_source("tree-sitter-json", "0.24.8")),
    ("yaml", "yaml", crate_source("tree-sitter-yaml", "0.7.2")),
    ("toml", "toml", crate_source("tree-sitter-toml-ng", "0.7.0")),
    (
        "markdown",
        "markdown",
        crate_source("tree-sitter-md", "0.5.4"),
    ),
    (
        "markdown_inline",
        "markdown",
        crate_source("tree-sitter-md", "0.5.4"),
    ),
];

fn source_of(id: &str) -> Source {
    SOURCES
        .iter()
        .find(|(i, _, _)| *i == id)
        .map(|(_, _, s)| *s)
        .expect("every grammar id is in SOURCES")
}

// Unused only in a build with every grammar feature off.
#[allow(unused_macros)]
macro_rules! grammar {
    ($id:literal, $feature:literal, $lang:expr, [$($h:expr),*], [$($i:expr),*], [$($l:expr),*]) => {
        Grammar {
            id: $id,
            feature: $feature,
            language: || $lang,
            highlights: &[$($h),*],
            injections: &[$($i),*],
            locals: &[$($l),*],
        }
    };
}

static GRAMMARS: &[Grammar] = &[
    #[cfg(feature = "php")]
    grammar!(
        "php",
        "php",
        tree_sitter_php::LANGUAGE_PHP.into(),
        [tree_sitter_php::HIGHLIGHTS_QUERY],
        [tree_sitter_php::INJECTIONS_QUERY],
        []
    ),
    #[cfg(feature = "php")]
    grammar!(
        "php_only",
        "php",
        tree_sitter_php::LANGUAGE_PHP_ONLY.into(),
        [tree_sitter_php::HIGHLIGHTS_QUERY],
        [tree_sitter_php::INJECTIONS_QUERY],
        []
    ),
    // Blade extends the HTML grammar; its queries say `inherits: html`.
    #[cfg(feature = "blade")]
    grammar!(
        "blade",
        "blade",
        // SAFETY: the symbol is the generated parser's entry point compiled by build.rs.
        unsafe { tree_sitter_language::LanguageFn::from_raw(tree_sitter_blade) }.into(),
        [
            tree_sitter_html::HIGHLIGHTS_QUERY,
            vendored_query!("blade", "highlights.scm")
        ],
        [
            tree_sitter_html::INJECTIONS_QUERY,
            vendored_query!("blade", "injections.scm")
        ],
        []
    ),
    #[cfg(feature = "twig")]
    grammar!(
        "twig",
        "twig",
        // SAFETY: as above.
        unsafe { tree_sitter_language::LanguageFn::from_raw(tree_sitter_twig) }.into(),
        [vendored_query!("twig", "highlights.scm")],
        [vendored_query!("twig", "injections.scm")],
        []
    ),
    #[cfg(feature = "javascript")]
    grammar!(
        "javascript",
        "javascript",
        tree_sitter_javascript::LANGUAGE.into(),
        [
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
        ],
        [tree_sitter_javascript::INJECTIONS_QUERY],
        [tree_sitter_javascript::LOCALS_QUERY]
    ),
    // TypeScript's queries only add to the JavaScript ones (as tree-sitter-typescript documents).
    #[cfg(feature = "typescript")]
    grammar!(
        "typescript",
        "typescript",
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        [
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_typescript::HIGHLIGHTS_QUERY
        ],
        [tree_sitter_javascript::INJECTIONS_QUERY],
        [
            tree_sitter_javascript::LOCALS_QUERY,
            tree_sitter_typescript::LOCALS_QUERY
        ]
    ),
    #[cfg(feature = "typescript")]
    grammar!(
        "tsx",
        "typescript",
        tree_sitter_typescript::LANGUAGE_TSX.into(),
        [
            tree_sitter_javascript::HIGHLIGHT_QUERY,
            tree_sitter_javascript::JSX_HIGHLIGHT_QUERY,
            tree_sitter_typescript::HIGHLIGHTS_QUERY
        ],
        [tree_sitter_javascript::INJECTIONS_QUERY],
        [
            tree_sitter_javascript::LOCALS_QUERY,
            tree_sitter_typescript::LOCALS_QUERY
        ]
    ),
    // Vue's queries say `inherits: html_tags`; both parts come from the upstream repo.
    #[cfg(feature = "vue")]
    grammar!(
        "vue",
        "vue",
        // SAFETY: as above.
        unsafe { tree_sitter_language::LanguageFn::from_raw(tree_sitter_vue) }.into(),
        [
            vendored_query!("vue", "html_tags/highlights.scm"),
            vendored_query!("vue", "vue/highlights.scm")
        ],
        [
            vendored_query!("vue", "html_tags/injections.scm"),
            vendored_query!("vue", "vue/injections.scm")
        ],
        []
    ),
    #[cfg(feature = "html")]
    grammar!(
        "html",
        "html",
        tree_sitter_html::LANGUAGE.into(),
        [tree_sitter_html::HIGHLIGHTS_QUERY],
        [tree_sitter_html::INJECTIONS_QUERY],
        []
    ),
    #[cfg(feature = "css")]
    grammar!(
        "css",
        "css",
        tree_sitter_css::LANGUAGE.into(),
        [tree_sitter_css::HIGHLIGHTS_QUERY],
        [],
        []
    ),
    // tree-sitter-scss 1.0.0 still exports the older `language()` binding (ABI 14).
    #[cfg(feature = "scss")]
    grammar!(
        "scss",
        "scss",
        tree_sitter_scss::language(),
        [
            tree_sitter_css::HIGHLIGHTS_QUERY,
            tree_sitter_scss::HIGHLIGHTS_QUERY
        ],
        [],
        []
    ),
    #[cfg(feature = "dart")]
    grammar!(
        "dart",
        "dart",
        tree_sitter_dart::LANGUAGE.into(),
        [tree_sitter_dart::HIGHLIGHTS_QUERY],
        [],
        [tree_sitter_dart::LOCALS_QUERY]
    ),
    #[cfg(feature = "go")]
    grammar!(
        "go",
        "go",
        tree_sitter_go::LANGUAGE.into(),
        [tree_sitter_go::HIGHLIGHTS_QUERY],
        [],
        []
    ),
    #[cfg(feature = "python")]
    grammar!(
        "python",
        "python",
        tree_sitter_python::LANGUAGE.into(),
        [tree_sitter_python::HIGHLIGHTS_QUERY],
        [],
        []
    ),
    #[cfg(feature = "rust")]
    grammar!(
        "rust",
        "rust",
        tree_sitter_rust::LANGUAGE.into(),
        [tree_sitter_rust::HIGHLIGHTS_QUERY],
        [tree_sitter_rust::INJECTIONS_QUERY],
        []
    ),
    #[cfg(feature = "json")]
    grammar!(
        "json",
        "json",
        tree_sitter_json::LANGUAGE.into(),
        [tree_sitter_json::HIGHLIGHTS_QUERY],
        [],
        []
    ),
    #[cfg(feature = "yaml")]
    grammar!(
        "yaml",
        "yaml",
        tree_sitter_yaml::LANGUAGE.into(),
        [tree_sitter_yaml::HIGHLIGHTS_QUERY],
        [],
        []
    ),
    #[cfg(feature = "toml")]
    grammar!(
        "toml",
        "toml",
        tree_sitter_toml_ng::LANGUAGE.into(),
        [tree_sitter_toml_ng::HIGHLIGHTS_QUERY],
        [],
        []
    ),
    // Markdown is two grammars: the block one injects `markdown_inline` into paragraphs.
    #[cfg(feature = "markdown")]
    grammar!(
        "markdown",
        "markdown",
        tree_sitter_md::LANGUAGE.into(),
        [tree_sitter_md::HIGHLIGHT_QUERY_BLOCK],
        [tree_sitter_md::INJECTION_QUERY_BLOCK],
        []
    ),
    #[cfg(feature = "markdown")]
    grammar!(
        "markdown_inline",
        "markdown",
        tree_sitter_md::INLINE_LANGUAGE.into(),
        [tree_sitter_md::HIGHLIGHT_QUERY_INLINE],
        [tree_sitter_md::INJECTION_QUERY_INLINE],
        []
    ),
];

/// The compiled-in grammars.
pub fn grammars() -> &'static [Grammar] {
    GRAMMARS
}

/// The compiled-in grammar for a language id.
pub fn grammar(id: &str) -> Option<&'static Grammar> {
    GRAMMARS.iter().find(|g| g.id == id)
}

/// Every D7 language with its build status.
pub fn inventory() -> Vec<Entry> {
    D7_LANGUAGES
        .iter()
        .map(|&id| {
            let (_, feature, source) = *SOURCES
                .iter()
                .find(|(i, _, _)| *i == id)
                .expect("every D7 id is in SOURCES");
            let status = if grammar(id).is_some() {
                Status::CompiledIn
            } else {
                Status::FeatureOff { feature }
            };
            Entry { id, status, source }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::strip_spell_captures;

    #[test]
    fn spell_captures_are_removed_and_other_captures_kept() {
        let q =
            "(comment) @comment @spell\n(tag_name) @tag @nospell\n(text) @spelling\n(x) @_spell";
        assert_eq!(
            strip_spell_captures(q),
            "(comment) @comment\n(tag_name) @tag\n(text) @spelling\n(x) @_spell"
        );
    }

    #[test]
    fn query_without_captures_is_unchanged() {
        assert_eq!(strip_spell_captures("(a (b))"), "(a (b))");
    }
}
