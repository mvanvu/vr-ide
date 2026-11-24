//! Every compiled-in grammar parses its sample without errors and its queries build a highlight
//! configuration that yields the expected capture classes on that sample.
// The helpers are unused only in a build with every grammar feature off.
#![allow(dead_code)]

use std::collections::BTreeSet;
use std::path::Path;

use tree_sitter::Parser;
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};
use vr_syntax::{D7_LANGUAGES, Status, grammar, inventory};

fn sample(file: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/samples")
        .join(file);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Capture names hit while highlighting `src` with the language's own queries (no injections).
fn captures(id: &str, src: &str) -> BTreeSet<String> {
    let g = grammar(id).unwrap_or_else(|| panic!("{id} is not in the grammar table"));
    let mut config = HighlightConfiguration::new(
        g.language(),
        id,
        &g.highlights_query(),
        &g.injections_query(),
        &g.locals_query(),
    )
    .unwrap_or_else(|e| panic!("{id}: highlight configuration failed: {e:?}"));
    // nvim-style queries add non-colour captures (spell checking, `@none`, `@_private`); an
    // engine never configures them, so neither does this test.
    let names: Vec<String> = config
        .names()
        .iter()
        .filter(|n| !matches!(**n, "spell" | "nospell" | "none" | "conceal") && !n.starts_with('_'))
        .map(|n| n.to_string())
        .collect();
    config.configure(&names);
    let mut highlighter = Highlighter::new();
    let events = highlighter
        .highlight(&config, src.as_bytes(), None, None, |_| None)
        .unwrap_or_else(|e| panic!("{id}: highlight failed: {e:?}"));
    let mut hit = BTreeSet::new();
    for event in events {
        if let HighlightEvent::HighlightStart(h) = event.unwrap_or_else(|e| panic!("{id}: {e:?}")) {
            hit.insert(names[h.0].clone());
        }
    }
    hit
}

fn check(id: &str, file: &str, expected: &[&str]) {
    let src = sample(file);
    let g = grammar(id).unwrap_or_else(|| panic!("{id} is not in the grammar table"));
    let mut parser = Parser::new();
    parser
        .set_language(&g.language())
        .unwrap_or_else(|e| panic!("{id}: {e}"));
    let tree = parser.parse(&src, None).expect("parse returns a tree");
    assert!(
        !tree.root_node().has_error(),
        "{id}: ERROR/MISSING nodes in {file}: {}",
        tree.root_node().to_sexp()
    );

    let hit = captures(id, &src);
    for class in expected {
        assert!(
            hit.iter()
                .any(|n| n == class || n.starts_with(&format!("{class}."))),
            "{id}: no `{class}` capture on {file}; got {hit:?}"
        );
    }
}

macro_rules! lang_test {
    ($name:ident, $feature:literal, $id:literal, $file:literal, [$($class:literal),+]) => {
        #[test]
        #[cfg(feature = $feature)]
        fn $name() {
            check($id, $file, &[$($class),+]);
        }
    };
}

// keyword/string/comment where the language has them; JSON, YAML, TOML, Markdown, HTML, Vue and
// Blade have no keyword class in their queries, so other classes stand in.
lang_test!(
    php,
    "php",
    "php",
    "php.php",
    ["keyword", "string", "comment", "function", "tag"]
);
lang_test!(
    php_only,
    "php",
    "php_only",
    "php_only.php",
    ["keyword", "string", "comment", "variable"]
);
lang_test!(
    blade,
    "blade",
    "blade",
    "blade.blade.php",
    ["tag", "string", "comment", "punctuation"]
);
lang_test!(
    twig,
    "twig",
    "twig",
    "twig.html.twig",
    ["keyword", "string", "comment", "variable"]
);
lang_test!(
    javascript,
    "javascript",
    "javascript",
    "javascript.js",
    ["keyword", "string", "comment", "function"]
);
lang_test!(
    typescript,
    "typescript",
    "typescript",
    "typescript.ts",
    ["keyword", "string", "comment", "type"]
);
lang_test!(
    tsx,
    "typescript",
    "tsx",
    "tsx.tsx",
    ["keyword", "string", "comment", "tag"]
);
lang_test!(
    vue,
    "vue",
    "vue",
    "vue.vue",
    ["tag", "string", "comment", "punctuation"]
);
lang_test!(
    html,
    "html",
    "html",
    "html.html",
    ["tag", "string", "comment", "attribute"]
);
lang_test!(
    css,
    "css",
    "css",
    "css.css",
    ["keyword", "string", "comment", "property"]
);
lang_test!(
    scss,
    "scss",
    "scss",
    "scss.scss",
    ["keyword", "string", "comment", "variable"]
);
lang_test!(
    dart,
    "dart",
    "dart",
    "dart.dart",
    ["keyword", "string", "comment", "type"]
);
// Go: `function` is not asserted — its query is ordered first-match-wins and tree-sitter-highlight
// 0.27 lets the later `(identifier) @variable` win (see the M4-1 notes, left to the engine, M4-2).
lang_test!(
    go,
    "go",
    "go",
    "go.go",
    ["keyword", "string", "comment", "type"]
);
lang_test!(
    python,
    "python",
    "python",
    "python.py",
    ["keyword", "string", "comment", "function"]
);
lang_test!(
    rust,
    "rust",
    "rust",
    "rust.rs",
    ["keyword", "string", "comment", "function"]
);
lang_test!(
    json,
    "json",
    "json",
    "json.json",
    ["string", "number", "constant"]
);
lang_test!(
    yaml,
    "yaml",
    "yaml",
    "yaml.yaml",
    ["string", "comment", "property", "boolean"]
);
lang_test!(
    toml,
    "toml",
    "toml",
    "toml.toml",
    ["string", "comment", "property", "boolean"]
);
lang_test!(
    markdown,
    "markdown",
    "markdown",
    "markdown.md",
    ["text", "punctuation"]
);
lang_test!(
    markdown_inline,
    "markdown",
    "markdown_inline",
    "markdown_inline.md",
    ["text", "punctuation"]
);

#[test]
fn inventory_lists_every_d7_language_with_a_status() {
    let expected = [
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
    assert_eq!(D7_LANGUAGES, expected);
    let inv = inventory();
    let ids: Vec<&str> = inv.iter().map(|e| e.id).collect();
    assert_eq!(ids, expected);
    for entry in &inv {
        match entry.status {
            Status::CompiledIn => assert!(
                grammar(entry.id).is_some(),
                "{} compiled in but not in the table",
                entry.id
            ),
            Status::FeatureOff { feature } => {
                assert!(
                    grammar(entry.id).is_none(),
                    "{} off but in the table",
                    entry.id
                );
                assert!(!feature.is_empty());
            }
        }
        assert!(
            !entry.source.origin.is_empty() && !entry.source.licence.is_empty(),
            "{}",
            entry.id
        );
    }
}

#[test]
fn default_build_compiles_every_d7_language_in() {
    // All grammar features are on by default (C2); this pins that no D7 language silently dropped out.
    if cfg!(all(
        feature = "php",
        feature = "blade",
        feature = "twig",
        feature = "javascript",
        feature = "typescript",
        feature = "vue",
        feature = "html",
        feature = "css",
        feature = "scss",
        feature = "dart",
        feature = "go",
        feature = "python",
        feature = "rust",
        feature = "json",
        feature = "yaml",
        feature = "toml",
        feature = "markdown"
    )) {
        assert_eq!(inventory().len(), D7_LANGUAGES.len());
        let missing: Vec<&str> = inventory()
            .iter()
            .filter(|e| e.status != Status::CompiledIn)
            .map(|e| e.id)
            .collect();
        assert!(missing.is_empty(), "not compiled in: {missing:?}");
    }
}

#[test]
fn unknown_language_is_not_in_the_table() {
    assert!(grammar("cobol").is_none());
}
