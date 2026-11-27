# Review M4-4 — APPROVED (3 minor)

## Findings
[minor] docs/languages.md:~57 — override rules incomplete
Scenario: a user writes `[jsonc] extensions = ["json"]`; vr-config `take_claims` (languages.rs) removes `json` from the built-in `json` (also files and aliases), and a bad field is warned and skipped. The doc says only "field by field".
Fix: add after the Fields line: "A table you write takes its extensions, file names and aliases away from every other language (a `[jsonc]` with `extensions = [\"json\"]` removes `json` from the built-in `json`). A bad table or field is reported at startup and skipped; the built-in value stays."

[minor] README.md:~381 — bench section out of date
Scenario: it says the script "prints time to first frame and max RSS" and the table (5 000-file project, 22 ms / 6 MB) is the pre-M4 number; M4-4 only measured an empty project (23-25 ms, 5 MB), so AC-1 on the bench project is not evidenced after M4.
Fix: reword to "prints time to first frame and max RSS for an empty project, then for a generated 200 KB Rust file (first frame, RSS at the first frame, max RSS after the colours landed)"; either re-run `scripts/bench-startup.sh` on the 5 000-file project and update the table, or label the table "before M4 (no grammars loaded)".

[minor] docs/languages.md:~42 / README.md:~396 — "20 grammars"
Scenario: there are 20 language ids but 17 grammar groups and 14 crates + 3 vendored; `php_only`, `markdown_inline`, `tsx` share a crate.
Fix: "the grammars account for about 14.7 MB of it" (M4-1 figure: 14.7 MB stripped delta).

## Verified
- Mutations caught: grammar missing (`language(0).is_some()` assert + snapshot); injection removed or wrong (snapshot and `at()` asserts, e.g. html "margin"="pppppp", vue "import"="kkkkkk", markdown "fn main"="kk.ffff"); renderer drawing a class in another class's colour (per-cell `drawn.fg == theme.syntax(class)` assert). Class map is letters from highlighter spans (disjoint, so `find` is exact), so a synthwave colour tweak changes nothing in the snapshots; colour is checked against the live theme, not a literal.
- php_only / markdown_inline: opened via languages.toml `files`, warnings asserted empty, doc and notes say so honestly; php_only asserts keyword and string.
- docs/languages.md: extensions, files, grammar ids, versions (Cargo.lock), MIT licences (crate manifests, vendor/*/SOURCE commits b5291d1 / 2208d2a / ce8011a), config dir (XDG / VR_IDE_CONFIG_DIR), max_highlight_bytes 10485760 = 10 MiB, file-over-extension and longest-extension rules, missing-grammar notice all match the code. Binary 19,596,336 B = 18.7 MiB, 23,613,456 B = 22.5 MiB correct; `strip = true` is the release default.
- Bench script: temp dir via mktemp -d with EXIT trap rm -rf; the `sleep 4; ^Q` pipe ends on its own (ctrl+q = app.quit); no background processes; failures of RSS parse exit 1.
- BASE-1 pass (lib.rs change is a `#[cfg(test)] mod` line; no product code). BASE-3 pass (notes: 20/20 red before snapshots). BASE-6 pass. BASE-25 pass (notes: fmt + clippy; not rerun).

## Files read
README.md, crates/vr-ide/src/lib.rs, scripts/bench-startup.sh, crates/vr-ide/src/language_snapshot_tests.rs, docs/languages.md: read. crates/vr-ide/src/snapshots/*.snap (20): skipped — generated, one opened (php_only header). Context: vr-config languages.rs, defaults/languages.toml, settings.rs, paths.rs, vr-syntax Cargo.toml, vendor SOURCE files, registry.rs.

## Rule check M4-4
| Rule | Result |
|---|---|
| BASE-1 | pass |
| BASE-3 | pass |
| BASE-6 | pass |
| BASE-25 | pass |
| AC-13 | pass (20 samples, >= 3 classes each) |
| AC-13a | pass |
| AC-1 | pass with finding 2 (empty-project numbers only) |
