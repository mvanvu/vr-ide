# Review M4-5 — XML + DTD highlighting

Verdict: CHANGES_REQUESTED (one major, process only; code is sound)

## Findings
[major] .ai-dev-team/workspace/04_dev_M4-5.md:1 — developer notes have no evidence and no `## Rule compliance` section
Scenario: the notes hold only a one-line "Progress" entry. There is no test/lint output, no Rule compliance table, and no statement that the oracle was run. The checklist treats this as a major finding.
Fix: add the command output (`cargo test -p vr-syntax -p vr-config -p vr-ide`, clippy/fmt), the oracle run, and a `## Rule compliance` section. The code itself needs no change.

[minor] crates/vr-syntax/tests/highlight.rs:439-461 — the two new precedence rows have `false` (no deciding case)
Scenario: both rows are probes where the order does not change the result. They only record `Last`; they cannot catch a wrong value. The developer says so in the comments. Acceptable, informational.

## Checks
- Incremental reparse: sufficient. The oracle reads every file in tests/samples, so sample.xml and sample.dtd are covered. Besides the release run, the default (non-ignored) test also covers them with seeds 987654321, 42 and a transaction seed. I ran `every_sample_equals_a_fresh_parse*` with `--release --include-ignored`: 2 passed (36 s), the full seed set included. The engine also has a fresh-parse check on text with syntax errors, so a divergence would be healed. Full is not needed (it is for tree-sitter-md inline only).
- Crate: tree-sitter-xml =0.7.0, registry Cargo.toml `license = "MIT"`; Cargo.lock adds only tree-sitter-xml (deps cc, tree-sitter-language already present) plus the vr-syntax dependency line.
- Grammar table: feature `xml` is in default, `dep:` gating, SOURCES, D7_LANGUAGES, the inventory test and the default-build feature list all updated consistently. Precedence `Last` matches the crate's nvim-order queries; the snapshots confirm specific captures win.
- languages.toml: the extensions equal D184 exactly. No clash: `xhtml` stays with html, and svg, props and plist are not owned by another language. Plain `pom.xml` is matched by `.xml`.
- Snapshot XML: tag (g), attribute (p), string (s), comment (c), keyword (k) and entity (C) classes are all present. The unit test asserts them. The DTD snapshot shows keywords (ELEMENT, ATTLIST, ENTITY), tags, types (#PCDATA, ID), attribute and string classes.
- docs/languages.md and README match the table (the licence line, "Every grammar is MIT", holds).

## Files read
Cargo.toml read; Cargo.lock read (generated, diff only); crates/vr-config/defaults/languages.toml read; crates/vr-syntax/Cargo.toml read; crates/vr-syntax/src/lib.rs read; crates/vr-syntax/tests/grammars.rs read; crates/vr-syntax/tests/highlight.rs read; crates/vr-syntax/tests/samples/sample.xml read; crates/vr-syntax/tests/samples/sample.dtd read; crates/vr-ide/src/language_snapshot_tests.rs read; both new .snap files read; docs/languages.md read; README.md read; 04_dev_M4-5.md read.

## Rule check M4-5
| Rule | Result | Note |
|---|---|---|
| BASE-1 | pass | spec/D184 followed; extensions equal |
| BASE-8 | pass | one crate, pinned exact, MIT, lock diff only that crate |
| BASE-3 | fail | notes carry no test or lint output |
| BASE-6 | pass | tests added (grammar, highlight, snapshot); oracle passes |
| BASE-25 | fail | no `## Rule compliance` section in the notes |
