//! Compiles the vendored grammars (no maintained crates.io crate at tree-sitter ABI 15).

use std::path::Path;

/// (cargo feature = directory under `vendor/`, has an external scanner)
const VENDORED: &[(&str, bool)] = &[("blade", true), ("twig", false), ("vue", true)];

fn main() {
    for &(name, scanner) in VENDORED {
        let feature = format!("CARGO_FEATURE_{}", name.to_uppercase());
        if std::env::var_os(&feature).is_none() {
            continue;
        }
        let src = Path::new("vendor").join(name).join("src");
        let mut build = cc::Build::new();
        build
            .include(&src)
            .std("c11")
            // Generated upstream code, unmodified: its compiler warnings are not ours to fix.
            .warnings(false)
            .file(src.join("parser.c"));
        if scanner {
            build.file(src.join("scanner.c"));
        }
        build.compile(&format!("tree-sitter-{name}"));
        println!("cargo:rerun-if-changed={}", src.display());
    }
    println!("cargo:rerun-if-changed=build.rs");
}
