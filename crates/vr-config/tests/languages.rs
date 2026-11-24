//! `languages.toml`: built-in defaults, user override field by field, validation warnings.

use std::path::PathBuf;

use vr_config::{ConfigWarning, Languages};

fn user_file(text: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("languages.toml");
    std::fs::write(&path, text).unwrap();
    (dir, path)
}

const D7_IDS: &[&str] = &[
    "php",
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
];

#[test]
fn defaults_cover_every_d7_language() {
    let (langs, warnings) = Languages::load(None);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(langs, Languages::default());
    for id in D7_IDS {
        let def = langs
            .get(id)
            .unwrap_or_else(|| panic!("{id} missing from defaults"));
        assert_eq!(def.grammar, *id, "{id}: grammar defaults to the id");
        assert!(
            !def.extensions.is_empty() || !def.files.is_empty(),
            "{id}: no file pattern"
        );
    }
    let rust = langs.get("rust").unwrap();
    assert_eq!(rust.extensions, ["rs"]);
    assert_eq!(rust.lsp, ["rust-analyzer"], "LSP column parsed (M8)");
    assert!(rust.aliases.iter().any(|a| a == "rs"));
    assert_eq!(langs.get("blade").unwrap().files, ["*.blade.php"]);
    // Injection-only grammars have an entry (aliases for injections) but no file pattern.
    let inline = langs.get("markdown_inline").unwrap();
    assert!(inline.extensions.is_empty() && inline.files.is_empty());
}

#[test]
fn user_file_overrides_fields_and_adds_languages() {
    let (_d, path) = user_file(
        "[rust]\nextensions = [\"rs\", \"rs.in\"]\n\n[es6]\ngrammar = \"javascript\"\nextensions = [\"es6\"]\nfiles = [\"Jakefile\"]\n",
    );
    let (langs, warnings) = Languages::load(Some(&path));
    assert!(warnings.is_empty(), "{warnings:?}");
    let rust = langs.get("rust").unwrap();
    assert_eq!(rust.extensions, ["rs", "rs.in"]);
    assert_eq!(
        rust.lsp,
        ["rust-analyzer"],
        "fields left out keep the default"
    );
    let es6 = langs.get("es6").unwrap();
    assert_eq!(es6.grammar, "javascript");
    assert_eq!(es6.files, ["Jakefile"]);
    assert!(es6.aliases.is_empty() && es6.lsp.is_empty());
    // Defaults keep their order; user additions come after them.
    assert_eq!(langs.languages.last().unwrap().id, "es6");
}

/// Each extension, file name and alias belongs to one language; one the user file gives a
/// language is taken from whichever language had it.
#[test]
fn user_claims_take_extensions_files_and_aliases_from_other_languages() {
    let claims = |langs: &Languages| {
        let mut all: Vec<String> = langs
            .languages
            .iter()
            .flat_map(|l| {
                l.extensions
                    .iter()
                    .map(|e| format!("ext {e}"))
                    .chain(l.files.iter().map(|f| format!("file {f}")))
                    .chain(l.aliases.iter().map(|a| format!("alias {a}")))
            })
            .collect();
        all.sort();
        let n = all.len();
        all.dedup();
        assert_eq!(all.len(), n, "a name claimed twice");
    };
    claims(&Languages::default());

    let (_d, path) = user_file(
        "[jsonc]\ngrammar = \"json\"\nextensions = [\"json\"]\naliases = [\"md\"]\nfiles = [\".babelrc\"]\n\n[javascript]\nextensions = [\"ts\"]\n",
    );
    let (langs, warnings) = Languages::load(Some(&path));
    assert!(warnings.is_empty(), "{warnings:?}");
    claims(&langs);
    let json = langs.get("json").unwrap();
    assert!(!json.extensions.contains(&"json".to_owned()));
    assert!(
        json.extensions.contains(&"jsonc".to_owned()),
        "other claims stay"
    );
    assert!(!json.files.contains(&".babelrc".to_owned()));
    assert!(
        !langs
            .get("markdown")
            .unwrap()
            .aliases
            .contains(&"md".to_owned())
    );
    assert!(
        !langs
            .get("typescript")
            .unwrap()
            .extensions
            .contains(&"ts".to_owned())
    );
    assert_eq!(langs.get("javascript").unwrap().extensions, ["ts"]);
}

#[test]
fn new_language_without_grammar_uses_its_id() {
    let (_d, path) = user_file("[elixir]\nextensions = [\"ex\", \"exs\"]\n");
    let (langs, warnings) = Languages::load(Some(&path));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(langs.get("elixir").unwrap().grammar, "elixir");
}

#[test]
fn bad_entries_are_warned_and_skipped() {
    let (_d, path) = user_file(
        "top = 1\n[rust]\nfoo = 1\nextensions = \"rs\"\naliases = [\"rust2\"]\n[go]\nextensions = [\"\", \"a/b\"]\nfiles = [\"*.go.tmpl\"]\n",
    );
    let (langs, warnings) = Languages::load(Some(&path));
    // The TOML table is a sorted map, so warnings come in key order, not file order.
    let mut keys: Vec<String> = warnings
        .iter()
        .map(|w| match w {
            ConfigWarning::UnknownSetting { key, .. } => format!("unknown {key}"),
            ConfigWarning::InvalidValue { key, .. } => format!("invalid {key}"),
            other => format!("{other:?}"),
        })
        .collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "invalid go.extensions",
            "invalid rust.extensions",
            "invalid top",
            "unknown rust.foo",
        ]
    );
    let rust = langs.get("rust").unwrap();
    assert_eq!(rust.extensions, ["rs"], "bad value keeps the default");
    assert_eq!(
        rust.aliases,
        ["rust2"],
        "good fields of the same table apply"
    );
    let go = langs.get("go").unwrap();
    assert_eq!(go.extensions, ["go"]);
    assert_eq!(go.files, ["*.go.tmpl"]);
    assert!(
        warnings
            .iter()
            .any(|w| w.to_string().contains("rust.extensions: expected a list"))
    );
}

#[test]
fn malformed_file_keeps_defaults() {
    let (_d, path) = user_file("[rust\nextensions = [");
    let (langs, warnings) = Languages::load(Some(&path));
    assert_eq!(langs, Languages::default());
    assert!(matches!(
        warnings.as_slice(),
        [ConfigWarning::Malformed { .. }]
    ));
}

#[test]
fn missing_user_file_is_silent() {
    let dir = tempfile::tempdir().unwrap();
    let (langs, warnings) = Languages::load(Some(&dir.path().join("languages.toml")));
    assert!(warnings.is_empty());
    assert_eq!(langs, Languages::default());
}

#[test]
fn file_name_constant() {
    assert_eq!(vr_config::LANGUAGES_FILE, "languages.toml");
}
