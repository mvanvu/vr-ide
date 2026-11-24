//! Language lookup by file name and by embedded-code name; grammars missing from the build.

use std::path::Path;

use vr_config::Languages;
use vr_syntax::{LanguageRegistry, RegistryWarning};

fn user(text: &str) -> Languages {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("languages.toml");
    std::fs::write(&path, text).unwrap();
    let (langs, warnings) = Languages::load(Some(&path));
    assert!(warnings.is_empty(), "{warnings:?}");
    langs
}

#[test]
fn builtin_table_maps_files_to_languages() {
    let reg = LanguageRegistry::builtin();
    let cases = [
        ("src/main.rs", Some("rust")),
        ("/p/resources/views/users.blade.php", Some("blade")),
        ("index.php", Some("php")),
        ("App.vue", Some("vue")),
        ("README.MD", Some("markdown")),
        ("Cargo.lock", Some("toml")),
        ("types.d.ts", Some("typescript")),
        ("view.tsx", Some("tsx")),
        ("page.html.twig", Some("twig")),
        (".prettierrc", Some("json")),
        ("a.code-workspace", Some("json")),
        ("x.yml", Some("yaml")),
        ("main.dart", Some("dart")),
        ("Makefile", None),
        ("notes.xyz", None),
        ("noext", None),
    ];
    for (path, want) in cases {
        assert_eq!(reg.language_for_path(Path::new(path)), want, "{path}");
    }
    let (_, warnings) = LanguageRegistry::new(&Languages::default());
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn a_user_language_takes_an_extension_or_alias_a_builtin_had() {
    let langs = user(
        "[jsonc]\ngrammar = \"json\"\nextensions = [\"json\"]\n\n[rusty]\ngrammar = \"rust\"\naliases = [\"rs\"]\n",
    );
    let (reg, warnings) = LanguageRegistry::new(&langs);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(reg.language_for_path(Path::new("a.json")), Some("jsonc"));
    assert_eq!(reg.language_for_path(Path::new("a.jsonc")), Some("json"));
    assert_eq!(reg.language_for_name("rs"), Some("rusty"));
    assert_eq!(reg.language_for_path(Path::new("a.rs")), Some("rust"));
}

#[test]
fn globs_and_exact_names_beat_extensions() {
    let langs = user("[jake]\ngrammar = \"javascript\"\nfiles = [\"Jake?ile\", \"*.tpl.php\"]\n");
    let (reg, warnings) = LanguageRegistry::new(&langs);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(reg.language_for_path(Path::new("Jakefile")), Some("jake"));
    assert_eq!(
        reg.language_for_path(Path::new("x/a.tpl.php")),
        Some("jake")
    );
    assert_eq!(reg.language_for_path(Path::new("a.php")), Some("php"));
    assert!(reg.has_grammar("jake"));
}

#[test]
fn embedded_names_resolve_by_id_alias_and_extension() {
    let reg = LanguageRegistry::builtin();
    let cases = [
        ("rust", Some("rust")),
        ("rs", Some("rust")),
        ("Rust", Some("rust")),
        ("rust,ignore", Some("rust")),
        ("JS", Some("javascript")),
        ("jsx", Some("javascript")),
        ("ts", Some("typescript")),
        ("py", Some("python")),
        ("golang", Some("go")),
        ("yml", Some("yaml")),
        ("markdown_inline", Some("markdown_inline")),
        ("php_only", Some("php_only")),
        ("bash", None),
        ("", None),
    ];
    for (name, want) in cases {
        assert_eq!(reg.language_for_name(name), want, "{name:?}");
    }
}

#[test]
fn language_without_a_compiled_grammar_is_reported_and_opens_plain() {
    let langs = user(
        "[elixir]\nextensions = [\"ex\"]\n[es]\ngrammar = \"ecmascript\"\nextensions = [\"es\"]\n",
    );
    let (reg, warnings) = LanguageRegistry::new(&langs);
    assert_eq!(
        warnings,
        vec![
            RegistryWarning::GrammarNotCompiledIn {
                language: "elixir".into(),
                grammar: "elixir".into(),
                feature: None
            },
            RegistryWarning::GrammarNotCompiledIn {
                language: "es".into(),
                grammar: "ecmascript".into(),
                feature: None
            },
        ]
    );
    let text = warnings[1].to_string();
    assert!(
        text.contains("es") && text.contains("ecmascript") && text.contains("plain"),
        "{text}"
    );
    assert_eq!(reg.language_for_path(Path::new("a.ex")), Some("elixir"));
    assert!(!reg.has_grammar("elixir"));
    assert!(reg.has_grammar("rust"));
}

#[test]
fn lsp_column_is_kept_for_m8() {
    let reg = LanguageRegistry::builtin();
    assert_eq!(reg.lsp("rust"), Some(&["rust-analyzer".to_string()][..]));
    assert_eq!(reg.lsp("nope"), None);
}
