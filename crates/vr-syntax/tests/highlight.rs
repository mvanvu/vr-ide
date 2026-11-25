//! The highlight engine: captures per language, injections (AC-13a), incremental reparse equal
//! to a full parse (AC-13b), size limit (AC-13c), the worker never blocking the caller (AC-13d),
//! versioned results, timeouts.

use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use ropey::Rope;
use vr_config::Languages;
use vr_syntax::{
    Capture, Disabled, Edit, HighlightOptions, HighlightStatus, Highlighter, LanguageRegistry,
};

const PARSE_WAIT: Duration = Duration::from_secs(120);

/// One registry for the whole test binary: compiling every language's queries per highlighter
/// would dominate the random-edit tests.
fn registry() -> Arc<LanguageRegistry> {
    static REGISTRY: std::sync::OnceLock<Arc<LanguageRegistry>> = std::sync::OnceLock::new();
    REGISTRY
        .get_or_init(|| Arc::new(LanguageRegistry::builtin()))
        .clone()
}

/// Replaces the first `old` in `text` with `new` through an edit, waits for the reparse and
/// asserts the spans equal those of a fresh highlighter on the new text.
fn replace_equals_fresh(lang: &str, text: &str, old: &str, new: &str) {
    let mut rope = Rope::from_str(text);
    let mut h = highlighter(lang, text);
    let at = rope.byte_to_char(
        text.find(old)
            .unwrap_or_else(|| panic!("{old:?} not in text")),
    );
    let to = at + old.chars().count();
    let e = Edit::from_chars(&rope, at, to, new);
    rope.remove(at..to);
    rope.insert(at, new);
    h.edit(&[e], rope.clone());
    assert!(h.wait(PARSE_WAIT));
    let after = rope.to_string();
    let fresh = highlighter(lang, &after);
    assert_eq!(
        all_spans(&h, after.len()),
        all_spans(&fresh, after.len()),
        "{lang}: {old:?} → {new:?} differs from a full parse of {after:?}"
    );
}

/// A replace-all over a document with thousands of embedded layers costs about one fresh parse,
/// not changes × layers.
#[test]
fn a_many_change_transaction_reparses_in_about_a_fresh_parse() {
    let mut text = String::new();
    for i in 0..4000 {
        text.push_str(&format!("Paragraph {i} with a *word* and `code`.\n\n"));
    }
    let mut rope = Rope::from_str(&text);
    let mut h = highlighter("markdown", &text);
    let changes: Vec<(usize, usize, &str)> = text
        .match_indices("word")
        .map(|(b, _)| {
            let c = rope.byte_to_char(b);
            (c, c + 4, "term")
        })
        .collect();
    assert_eq!(changes.len(), 4000);
    let edits = Edit::from_changes(&rope, changes.iter().copied());
    for &(from, to, insert) in changes.iter().rev() {
        rope.remove(from..to);
        rope.insert(from, insert);
    }
    let t = Instant::now();
    h.edit(&edits, rope.clone());
    assert!(h.wait(PARSE_WAIT));
    let incremental = t.elapsed();
    let after = rope.to_string();
    let t = Instant::now();
    let fresh = highlighter("markdown", &after);
    let full = t.elapsed();
    eprintln!("4000-change transaction: reparse {incremental:?}, fresh parse {full:?}");
    assert_eq!(all_spans(&h, after.len()), all_spans(&fresh, after.len()));
    assert!(
        incremental <= full * 3 + Duration::from_millis(500),
        "reparse {incremental:?} vs fresh {full:?}"
    );
}

#[test]
fn changing_an_injection_language_drops_the_old_layer() {
    let md = "# T\n\n```rust\nfn main() { let x = (1); } // c\n```\n";
    replace_equals_fresh("markdown", md, "rust", "python");
    replace_equals_fresh("markdown", md, "rust", "nosuchlang");
    let vue = "<template>\n  <p>{{ a }}</p>\n</template>\n\n<script lang=\"ts\">\nconst n: number = 1;\ninterface I { a: string }\n</script>\n";
    replace_equals_fresh("vue", vue, "\"ts\"", "\"js\"");
    let html = "<script type=\"module\">\nconst a = 1;\n</script>\n";
    replace_equals_fresh("html", html, "module", "text/template");
}

fn highlighter(lang: &str, text: &str) -> Highlighter {
    let h = Highlighter::new(registry(), lang, Rope::from_str(text), options(), None);
    assert!(h.wait(PARSE_WAIT), "{lang}: parse did not land");
    assert_eq!(h.status(), HighlightStatus::Ready);
    h
}

/// The class covering the first byte of the `nth` occurrence of `needle`.
fn class_at(h: &Highlighter, text: &str, needle: &str, nth: usize) -> Option<Capture> {
    let at = text
        .match_indices(needle)
        .nth(nth)
        .unwrap_or_else(|| panic!("{needle:?} not in text"))
        .0;
    h.spans(at..at + needle.len())
        .into_iter()
        .find(|(r, _)| r.start <= at && at < r.end)
        .map(|(_, c)| c)
}

fn assert_class(h: &Highlighter, text: &str, needle: &str, want: Capture) {
    assert_eq!(
        class_at(h, text, needle, 0),
        Some(want),
        "{needle:?} in {}",
        h.language()
    );
}

/// The product options, except that a `spans()` call and a parse may take as long as the
/// machine needs. The 50 ms query budget (D162) cuts a list short on a loaded machine (a debug
/// build at load 12–18 lost every span past one point of a 262-byte Vue file), and the 10 s
/// parse limit leaves a 2 MB debug-build parse plain; a comparison against a cut or plain result
/// says nothing about the spans themselves. Both limits have their own tests
/// (`spans_call_is_bounded_by_the_query_timeout`, `parse_timeout_stays_plain_and_reports`).
fn options() -> HighlightOptions {
    HighlightOptions {
        query_timeout: PARSE_WAIT,
        parse_timeout: PARSE_WAIT,
        ..HighlightOptions::default()
    }
}

/// Options whose fresh-parse check of text with errors starts right after each reparse.
fn no_idle() -> HighlightOptions {
    HighlightOptions {
        verify_idle: Duration::ZERO,
        ..options()
    }
}

fn all_spans(h: &Highlighter, len: usize) -> Vec<(Range<usize>, Capture)> {
    h.spans(0..len)
}

#[test]
fn rust_keywords_strings_comments_functions() {
    let text = "fn main() {\n    let s = \"hi\"; // note\n    println!(\"{s}\", 42);\n}\n";
    let h = highlighter("rust", text);
    assert_class(&h, text, "fn", Capture::Keyword);
    assert_class(&h, text, "let", Capture::Keyword);
    assert_class(&h, text, "\"hi\"", Capture::String);
    assert_class(&h, text, "// note", Capture::Comment);
    assert_class(&h, text, "main", Capture::Function);
    assert_class(&h, text, "42", Capture::Constant);
    let spans = all_spans(&h, text.len());
    assert!(
        spans.windows(2).all(|w| w[0].0.end <= w[1].0.start),
        "sorted, disjoint"
    );
}

#[test]
fn go_function_name_beats_the_generic_identifier_pattern() {
    // tree-sitter-go's query lists `(identifier) @variable` after the specific patterns.
    let text = "package main\n\nfunc greet(name string) string {\n\treturn name\n}\n";
    let h = highlighter("go", text);
    assert_class(&h, text, "greet", Capture::Function);
    assert_class(&h, text, "func", Capture::Keyword);
}

#[test]
fn json_keys_and_values() {
    let text = "{\"key\": \"value\", \"n\": 1, \"b\": true}";
    let h = highlighter("json", text);
    assert_class(&h, text, "\"key\"", Capture::Property);
    assert_class(&h, text, "\"value\"", Capture::String);
    assert_class(&h, text, "1", Capture::Number);
}

#[test]
fn html_script_and_style_use_their_own_languages() {
    let text = "<html>\n<script>\nconst n = 42;\n</script>\n<style>\np { color: red; }\n</style>\n</html>\n";
    let h = highlighter("html", text);
    assert_class(&h, text, "html", Capture::Tag);
    assert_class(&h, text, "const", Capture::Keyword);
    assert_class(&h, text, "42", Capture::Number);
    assert_class(&h, text, "color", Capture::Property);
}

#[test]
fn vue_template_script_and_style_blocks() {
    let text = "<template>\n  <div :title=\"msg\">{{ msg }}</div>\n</template>\n\n<script lang=\"ts\">\nconst count: number = 1;\n</script>\n\n<style scoped>\n.box { color: red; }\n</style>\n";
    let h = highlighter("vue", text);
    assert_class(&h, text, "div", Capture::Tag);
    assert_class(&h, text, "const", Capture::Keyword);
    assert_class(&h, text, "number", Capture::Type);
    assert_class(&h, text, "color", Capture::Property);
}

#[test]
fn php_mixed_with_html_and_a_script() {
    let text = "<html>\n<body>\n<?php echo \"hi\"; $x = 1; ?>\n<script>\nvar a = 2;\n</script>\n</body>\n</html>\n";
    let h = highlighter("php", text);
    assert_class(&h, text, "echo", Capture::Keyword);
    assert_class(&h, text, "\"hi\"", Capture::String);
    assert_class(&h, text, "body", Capture::Tag);
    // PHP → HTML (combined text) → JavaScript.
    assert_class(&h, text, "var", Capture::Keyword);
}

#[test]
fn markdown_fences_by_info_string() {
    let text = "# Title\n\nSome *text*.\n\n```rust\nfn main() {}\n```\n\n```rs\nlet x = 1;\n```\n\n```nosuchlang\nfn plain() {}\n```\n";
    let h = highlighter("markdown", text);
    assert_class(&h, text, "Title", Capture::Keyword);
    assert_class(&h, text, "fn", Capture::Keyword);
    assert_class(&h, text, "main", Capture::Function);
    assert_class(&h, text, "let", Capture::Keyword);
    // An unknown info string leaves the block in Markdown's own code colour.
    assert_ne!(class_at(&h, text, "fn", 1), Some(Capture::Keyword));
}

#[test]
fn blade_directives_have_their_own_capture() {
    let text =
        "<ul>\n@foreach ($users as $user)\n    <li>{{ $user->name }}</li>\n@endforeach\n</ul>\n";
    let h = highlighter("blade", text);
    let at = text.find("@foreach").unwrap();
    let spans = h.spans(at..at + "@foreach".len());
    assert!(
        spans
            .iter()
            .any(|(r, c)| r.start <= at && r.end >= at + "@foreach".len() && *c == Capture::Tag),
        "@foreach covered by the Blade directive capture: {spans:?}"
    );
    assert_class(&h, text, "li", Capture::Tag);
}

/// Every grammar declares which of two patterns capturing one node wins; one case per grammar
/// pins a specific capture over the generic one. Cases marked `decides` turn into the generic
/// class when the declared precedence is flipped (checked when the field was introduced); in the
/// others no two patterns of that grammar's query disagree on the node.
#[test]
fn every_grammar_declares_its_precedence_and_specific_patterns_win() {
    use vr_syntax::Precedence::{First, Last};
    // (language, declared precedence, sample file or inline text, needle, nth, class, decides)
    let cases: &[(
        &str,
        vr_syntax::Precedence,
        &str,
        &str,
        usize,
        Capture,
        bool,
    )] = &[
        (
            "php",
            Last,
            "<?php\nfunction greet() { return strlen($n); }\n",
            "strlen",
            0,
            Capture::Function,
            false,
        ),
        (
            "php_only",
            Last,
            "function greet() { return strlen($n); }\n",
            "greet",
            0,
            Capture::Function,
            false,
        ),
        (
            "blade",
            Last,
            "<div>@if($x) {{ $user->name }} @endif</div>\n",
            "name",
            0,
            Capture::Property,
            false,
        ),
        (
            "twig",
            Last,
            "{{ user.name|upper }}\n",
            "upper",
            0,
            Capture::Function,
            false,
        ),
        (
            "javascript",
            Last,
            "file:javascript.js",
            "Counter",
            0,
            Capture::Type,
            true,
        ),
        (
            "typescript",
            Last,
            "file:typescript.ts",
            "greet",
            0,
            Capture::Function,
            true,
        ),
        (
            "tsx",
            Last,
            "function greet(n: string) { return <div>{n}</div>; }\n",
            "greet",
            0,
            Capture::Function,
            true,
        ),
        (
            "vue",
            Last,
            "file:vue.vue",
            "ref",
            1,
            Capture::Function,
            true,
        ),
        (
            "html",
            Last,
            "file:html.html",
            "log",
            0,
            Capture::Function,
            true,
        ),
        (
            "css",
            Last,
            "file:css.css",
            "hover",
            0,
            Capture::Attribute,
            true,
        ),
        (
            "scss",
            Last,
            "file:scss.scss",
            "rounded",
            1,
            Capture::Function,
            true,
        ),
        (
            "dart",
            Last,
            "file:dart.dart",
            "greet",
            2,
            Capture::Function,
            true,
        ),
        (
            "go",
            First,
            "file:go.go",
            "greet",
            1,
            Capture::Function,
            true,
        ),
        (
            "python",
            Last,
            "file:python.py",
            "__init__",
            0,
            Capture::Function,
            true,
        ),
        (
            "rust",
            Last,
            "file:rust.rs",
            "to_string",
            0,
            Capture::Function,
            true,
        ),
        (
            "json",
            First,
            "file:json.json",
            "\"name\"",
            0,
            Capture::Property,
            true,
        ),
        (
            "yaml",
            Last,
            "file:yaml.yaml",
            "name",
            0,
            Capture::Property,
            true,
        ),
        (
            "toml",
            Last,
            "[[bin]]\na.b = 1\n",
            "a",
            0,
            Capture::Property,
            true,
        ),
        (
            "markdown",
            Last,
            "# Title\n\n[ref]: http://a.b \"t\"\n",
            "[ref]",
            0,
            Capture::Function,
            false,
        ),
        (
            "markdown_inline",
            Last,
            "Some [link](http://a.b) \\*\n",
            "\\*",
            0,
            Capture::Escape,
            false,
        ),
        // The PI target is a child of the `(PI) @embedded` node, so no two patterns capture the
        // same node: flipping the order changes nothing (checked with a flip probe).
        (
            "xml",
            Last,
            "<?php echo 1; ?>\n<a/>\n",
            "php",
            0,
            Capture::Keyword,
            false,
        ),
        // No two DTD patterns disagree on a node in the probes, so the case does not decide.
        (
            "dtd",
            Last,
            "<!ENTITY a \"b\">\n<!ELEMENT e (#PCDATA)>\n",
            "#PCDATA",
            0,
            Capture::Type,
            false,
        ),
    ];
    for g in vr_syntax::grammars() {
        assert!(
            cases.iter().any(|c| c.0 == g.id),
            "{}: no precedence case",
            g.id
        );
    }
    for &(lang, precedence, source, needle, nth, want, _) in cases {
        let Some(grammar) = vr_syntax::grammar(lang) else {
            continue; // feature off
        };
        assert_eq!(grammar.precedence(), precedence, "{lang}");
        let text = match source.strip_prefix("file:") {
            Some(file) => sample(file),
            None => source.to_owned(),
        };
        let h = highlighter(lang, &text);
        assert_eq!(
            class_at(&h, &text, needle, nth),
            Some(want),
            "{lang}: {needle:?} #{nth}"
        );
    }
}

/// A tiny deterministic generator (no rand dependency).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn incremental_equals_full(lang: &str, sample: &str, seed: u64, steps: usize) {
    let snippets = [
        "",
        "fn ",
        "let x = 1;",
        "\"str\"",
        "// c\n",
        "{",
        "}",
        "(",
        ")",
        "\n",
        " ",
        "/*",
        "*/",
        "é",
        "中",
        "<div>",
        "</div>",
        "<script>a=1</script>",
        "```rust\nfn a(){}\n```\n",
        "# h\n",
        "<?php echo 1; ?>",
    ];
    let mut rng = Lcg(seed);
    let mut rope = Rope::from_str(sample);
    let mut h = Highlighter::new(registry(), lang, rope.clone(), options(), None);
    for step in 1..=steps {
        let len = rope.len_chars();
        let from = rng.below(len + 1);
        let to = (from + rng.below(6)).min(len);
        let insert = snippets[rng.below(snippets.len())];
        match step % 7 {
            // Undo / reload path: the caller only has the new text.
            0 => {
                rope.remove(from..to);
                rope.insert(from, insert);
                h.update(rope.clone());
            }
            // A two-change transaction (multi-cursor).
            3 if to + 2 <= len => {
                let before = rope.clone();
                let edits = Edit::from_changes(&before, [(from, to, insert), (to + 1, to + 2, "")]);
                rope.remove(to + 1..to + 2);
                rope.remove(from..to);
                rope.insert(from, insert);
                h.edit(&edits, rope.clone());
            }
            _ => {
                let e = Edit::from_chars(&rope, from, to, insert);
                rope.remove(from..to);
                rope.insert(from, insert);
                h.edit(&[e], rope.clone());
            }
        }
        if step % 25 == 0 || step == steps {
            assert!(h.wait(PARSE_WAIT), "{lang} step {step}: parse did not land");
            let text = rope.to_string();
            let fresh = highlighter(lang, &text);
            let (got, want) = (all_spans(&h, text.len()), all_spans(&fresh, text.len()));
            assert!(
                got == want,
                "{lang} seed {seed} step {step}: incremental spans differ from a full parse\n\
                 text: {text:?}\nfirst difference: {:?}",
                got.iter().zip(&want).find(|(a, b)| a != b)
            );
        }
    }
}

fn sample(file: &str) -> String {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/samples")
            .join(file),
    )
    .unwrap()
}

#[test]
fn incremental_reparse_equals_full_reparse_on_random_edits() {
    incremental_equals_full("rust", &sample("rust.rs"), 0x5eed_0001, 150);
    incremental_equals_full("html", &sample("html.html"), 0x5eed_0002, 100);
    incremental_equals_full("markdown", &sample("markdown.md"), 0x5eed_0003, 100);
    incremental_equals_full("php", &sample("php.php"), 0x5eed_0004, 100);
    incremental_equals_full("vue", &sample("vue.vue"), 0x5eed_0005, 100);
    incremental_equals_full("blade", &sample("blade.blade.php"), 0x5eed_0006, 100);
}

/// The registry id a sample file is highlighted as.
fn sample_language(file: &str) -> String {
    let reg = registry();
    let stem = file.split('.').next().unwrap();
    if reg.has_grammar(stem) {
        return stem.to_owned();
    }
    reg.language_for_path(std::path::Path::new(file))
        .unwrap_or_else(|| panic!("{file}: no language"))
        .to_owned()
}

/// AC-13b oracle: after every seeded single-character edit, once the reparse has landed, the
/// spans equal those of a fresh highlighter on the same text — on every shipped sample, for
/// several seeds (one seed alone missed cases). The single-edit runs use the same generator
/// sequence as QC's probe (`seed ^ file_index * 7919`); the transaction runs add a 2–40-change
/// edit every 10th step.
///
/// The default run keeps the two seeds that found real mismatches (987654321: php_only,
/// 42: blade) plus one transaction run; `VR_SYNTAX_FUZZ_SEED=<n>` runs one seed instead. The
/// full set is `every_sample_equals_a_fresh_parse_full_seed_set` (ignored; run it in release).
#[test]
fn every_sample_equals_a_fresh_parse_after_each_random_char_edit() {
    let failures = match std::env::var("VR_SYNTAX_FUZZ_SEED") {
        Ok(s) => {
            let seed = s.parse().expect("VR_SYNTAX_FUZZ_SEED is a number");
            fuzz_seeds(&[seed], &[seed])
        }
        Err(_) => fuzz_seeds(&[987654321, 42], &[0xC0FFEE]),
    };
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The oracle over every seed: 5 single-edit runs and 2 transaction runs (40 s in release,
/// over 2 min in debug). QC: `cargo test -p vr-syntax --release --test highlight -- --ignored`.
#[test]
#[ignore = "full fuzz seed set: run in release with --ignored"]
fn every_sample_equals_a_fresh_parse_full_seed_set() {
    let failures = fuzz_seeds(
        &[0xC0FFEE, 987654321, 0x5EED_0001, 0xDEAD_BEEF, 42],
        &[0xC0FFEE, 987654321],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn fuzz_seeds(single: &[u64], transactions: &[u64]) -> Vec<String> {
    let mut failures = Vec::new();
    for &seed in single {
        failures.extend(fuzz_against_fresh(seed, false));
    }
    for &seed in transactions {
        failures.extend(fuzz_against_fresh(seed, true));
    }
    failures
}

/// Text with syntax errors gets a fresh-parse check after each reparse; edits arriving during
/// it stop the check and are parsed next, and the end result still equals a fresh parse.
#[test]
fn edits_during_the_fresh_parse_check_are_not_lost() {
    let text = sample("php_only.php").replace("function", "func tion {{ (");
    let mut rope = Rope::from_str(&text);
    // No pause before the check, so checks start between these edits and get superseded.
    let mut h = Highlighter::new(registry(), "php_only", rope.clone(), no_idle(), None);
    for i in 0..60 {
        let at = (i * 7) % rope.len_chars();
        let insert = ["`", "(", "x", "\"", "\n"][i % 5];
        let e = Edit::from_chars(&rope, at, at, insert);
        rope.insert(at, insert);
        h.edit(&[e], rope.clone());
    }
    assert!(h.wait(PARSE_WAIT));
    assert_eq!(h.status(), HighlightStatus::Ready);
    let after = rope.to_string();
    let fresh = highlighter("php_only", &after);
    assert_eq!(all_spans(&h, after.len()), all_spans(&fresh, after.len()));
}

/// The fresh-parse check of text with errors waits for a pause in typing: during a burst of
/// keystrokes 30 ms apart the spans stay provisional (`Parsing`), and the check lands only once
/// the document has been idle for `verify_idle`.
#[test]
fn the_fresh_parse_check_waits_until_typing_pauses() {
    let idle = HighlightOptions::default().verify_idle;
    assert!(idle >= Duration::from_millis(100), "{idle:?}");
    let text = sample("php_only.php").replace("function", "func tion {{ (");
    let mut rope = Rope::from_str(&text);
    let mut h = highlighter("php_only", &text);
    let at = rope.len_chars() / 2;
    let mut last = Instant::now();
    let mut ready_mid_burst = Vec::new();
    for i in 0..20 {
        // Under load a sleep can overshoot past the idle time; only short gaps count.
        if i > 0 && last.elapsed() < idle && h.status() == HighlightStatus::Ready {
            ready_mid_burst.push(i);
        }
        let e = Edit::from_chars(&rope, at + i, at + i, "x");
        rope.insert(at + i, "x");
        h.edit(&[e], rope.clone());
        last = Instant::now();
        std::thread::sleep(Duration::from_millis(30));
    }
    assert!(
        ready_mid_burst.is_empty(),
        "the check ran during typing before keystrokes {ready_mid_burst:?}"
    );
    assert!(h.wait(PARSE_WAIT));
    assert_eq!(h.status(), HighlightStatus::Ready);
    assert!(
        last.elapsed() >= idle,
        "Ready {:?} after the last edit",
        last.elapsed()
    );
    let after = rope.to_string();
    let fresh = highlighter("php_only", &after);
    assert_eq!(all_spans(&h, after.len()), all_spans(&fresh, after.len()));
}

fn fuzz_against_fresh(seed: u64, transactions: bool) -> Vec<String> {
    let chars = [
        "a", "x", " ", "\n", "\"", "'", "<", ">", "{", "}", "(", ")", "/", "*", "#", "`", "é",
        "中", ";", "@", "$", "?", "-",
    ];
    let steps: usize = std::env::var("VR_SYNTAX_FUZZ_STEPS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(200);
    let mut files: Vec<String> =
        std::fs::read_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/samples"))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
    files.sort();
    let mut failures = Vec::new();
    for (fi, file) in files.iter().enumerate() {
        let lang = sample_language(file);
        let mut rope = Rope::from_str(&sample(file));
        // The oracle waits for the check after every edit; the pause before it would only add
        // `verify_idle` per step, the result is the same.
        let mut h = Highlighter::new(registry(), &lang, rope.clone(), no_idle(), None);
        let mut rng = Lcg(seed ^ (fi as u64 * 7919));
        let mut bad = 0;
        let mut first = None;
        for step in 1..=steps {
            let len = rope.len_chars();
            // A multi-cursor transaction: up to 40 changes, past the point where the engine
            // folds them into one covering edit.
            let count = if transactions && step % 10 == 0 {
                2 + rng.below(39)
            } else {
                1
            };
            let mut changes: Vec<(usize, usize, &str)> = Vec::new();
            let mut starts: Vec<usize> = (0..count).map(|_| rng.below(len + 1)).collect();
            starts.sort_unstable();
            starts.dedup();
            for (k, &from) in starts.iter().enumerate() {
                let limit = starts.get(k + 1).copied().unwrap_or(len);
                let (to, insert) = match rng.below(3) {
                    0 => (from, chars[rng.below(chars.len())]),
                    1 => ((from + 1).min(limit), ""),
                    _ => ((from + 1).min(limit), chars[rng.below(chars.len())]),
                };
                changes.push((from, to, insert));
            }
            let (from, to, insert) = changes[0];
            let edits = Edit::from_changes(&rope, changes.iter().copied());
            for &(f, t, ins) in changes.iter().rev() {
                rope.remove(f..t);
                rope.insert(f, ins);
            }
            h.edit(&edits, rope.clone());
            assert!(h.wait(PARSE_WAIT), "{file} step {step}: parse did not land");
            assert_eq!(h.status(), HighlightStatus::Ready, "{file} step {step}");
            let text = rope.to_string();
            let fresh = highlighter(&lang, &text);
            let (got, want) = (all_spans(&h, text.len()), all_spans(&fresh, text.len()));
            if got != want {
                bad += 1;
                if first.is_none() {
                    let only_got: Vec<_> = got.iter().filter(|s| !want.contains(s)).collect();
                    let only_want: Vec<_> = want.iter().filter(|s| !got.contains(s)).collect();
                    first = Some(format!(
                        "step {step} edit {from}..{to} {insert:?}\n  text {text:?}\n  \
                         only incremental {only_got:?}\n  only fresh {only_want:?}"
                    ));
                }
            }
        }
        if bad > 0 {
            failures.push(format!(
                "seed {seed} transactions {transactions}: {file} ({lang}): {bad} of {steps} \
                 checks differ; first: {}",
                first.unwrap()
            ));
        }
    }
    failures
}

fn rust_source(bytes: usize) -> String {
    let mut s = String::with_capacity(bytes + 200);
    let mut i = 0;
    while s.len() < bytes {
        s.push_str(&format!(
            "/// Doc {i}.\nfn f{i}(x: u32) -> u32 {{\n    // comment {i}\n    let s = \"text {i}\";\n    x + {i}\n}}\n\n"
        ));
        i += 1;
    }
    s
}

#[test]
fn parse_timeout_stays_plain_and_reports() {
    let text = rust_source(2_000_000);
    let options = HighlightOptions {
        parse_timeout: Duration::from_micros(1),
        ..HighlightOptions::default()
    };
    let h = Highlighter::new(registry(), "rust", Rope::from_str(&text), options, None);
    assert!(h.wait(PARSE_WAIT));
    assert!(
        matches!(
            h.status(),
            HighlightStatus::Disabled(Disabled::Timeout { .. })
        ),
        "{:?}",
        h.status()
    );
    assert!(h.spans(0..text.len()).is_empty());
    assert!(
        Disabled::Timeout {
            after: Duration::from_secs(10)
        }
        .to_string()
        .contains("plain")
    );
}

#[test]
fn files_over_max_highlight_bytes_are_disabled() {
    let options = HighlightOptions {
        max_bytes: 10,
        ..HighlightOptions::default()
    };
    let h = Highlighter::new(
        registry(),
        "rust",
        Rope::from_str("fn main(){}"),
        options.clone(),
        None,
    );
    assert_eq!(
        h.status(),
        HighlightStatus::Disabled(Disabled::TooLarge {
            bytes: 11,
            limit: 10
        })
    );
    assert!(h.wait(Duration::ZERO), "nothing to wait for");
    assert!(h.spans(0..11).is_empty());
    assert!(
        h.status().to_string_lossy().contains("plain"),
        "notice text says the file is plain"
    );

    // Growing past the limit while editing disables it too.
    let mut rope = Rope::from_str("fn a(){}");
    let mut h = Highlighter::new(registry(), "rust", rope.clone(), options, None);
    assert!(h.wait(PARSE_WAIT));
    assert!(!h.spans(0..8).is_empty());
    let e = Edit::from_chars(&rope, 8, 8, "\nfn b(){}");
    rope.insert(8, "\nfn b(){}");
    h.edit(&[e], rope);
    assert!(matches!(
        h.status(),
        HighlightStatus::Disabled(Disabled::TooLarge { .. })
    ));
    assert!(h.spans(0..17).is_empty());
}

trait StatusText {
    fn to_string_lossy(&self) -> String;
}

impl StatusText for HighlightStatus {
    fn to_string_lossy(&self) -> String {
        match self {
            HighlightStatus::Disabled(d) => d.to_string(),
            other => format!("{other:?}"),
        }
    }
}

#[test]
fn worker_never_blocks_the_caller_on_a_5mb_file() {
    let text = rust_source(5_000_000);
    let rope = Rope::from_str(&text);
    // What is under test is the caller's side; how long the worker's parse takes depends on the
    // build and the machine's load, so it gets no time limit here (the timeout has its own test).
    let options = HighlightOptions {
        parse_timeout: Duration::from_secs(3600),
        ..HighlightOptions::default()
    };
    let t = Instant::now();
    let h = Highlighter::new(registry(), "rust", rope, options, None);
    let created = t.elapsed();
    let t = Instant::now();
    let first = h.spans(0..4000);
    let first_spans = t.elapsed();
    let status_while_parsing = h.status();
    eprintln!("5 MB: new {created:?}, first spans() {first_spans:?} ({status_while_parsing:?})");
    assert!(created < Duration::from_millis(50), "new took {created:?}");
    assert!(
        first_spans < Duration::from_millis(20),
        "spans took {first_spans:?}"
    );
    assert_eq!(status_while_parsing, HighlightStatus::Parsing);
    assert!(first.is_empty(), "plain until the parse lands");
    let t = Instant::now();
    assert!(h.wait(Duration::from_secs(3600)));
    eprintln!("5 MB: initial parse landed after {:?}", t.elapsed());
    assert_eq!(h.status(), HighlightStatus::Ready);
    assert!(h.spans(0..4000).iter().any(|(_, c)| *c == Capture::Keyword));
}

#[test]
fn edit_during_the_initial_parse_never_returns_stale_spans() {
    let text = rust_source(2_000_000);
    let mut rope = Rope::from_str(&text);
    // Compile the Rust queries first (cached in the registry), so the worker starts parsing
    // version 0 right away.
    let reg = registry();
    let warm = Highlighter::new(
        reg.clone(),
        "rust",
        Rope::from_str("fn a() {}"),
        options(),
        None,
    );
    assert!(warm.wait(PARSE_WAIT));
    let mut h = Highlighter::new(reg, "rust", rope.clone(), options(), None);
    // Let the worker start on version 0, so the edit arrives while that parse runs (otherwise
    // the worker would take both messages at once and only ever parse version 1).
    std::thread::sleep(Duration::from_millis(50));
    // Insert spaces between two items in the middle while version 0 is still being parsed.
    let at = text[1_000_000..].find("\n\n").unwrap() + 1_000_001;
    let insert = "   ";
    let e = Edit::from_chars(&rope, at, at, insert);
    rope.insert(at, insert);
    h.edit(&[e], rope.clone());
    assert_eq!(h.status(), HighlightStatus::Parsing);
    let window = at - 2000..at + 2000;
    // Every spans() result seen until the edit's reparse lands, and whether it came from the
    // older version (status still Parsing after the call).
    let mut seen = Vec::new();
    let deadline = Instant::now() + PARSE_WAIT;
    loop {
        let got = h.spans(window.clone());
        let parsing = h.status() == HighlightStatus::Parsing;
        // Keep distinct results only (the loop spins for the whole parse).
        if seen.last() != Some(&(got.clone(), parsing)) {
            seen.push((got, parsing));
        }
        if !parsing || Instant::now() > deadline {
            break;
        }
    }
    let fresh = highlighter("rust", &rope.to_string());
    let want = fresh.spans(window.clone());
    let from_old_version = seen
        .iter()
        .filter(|(got, parsing)| *parsing && !got.is_empty())
        .count();
    for (got, _) in &seen {
        // Whatever version the spans come from, none covers the inserted text...
        assert!(
            got.iter()
                .all(|(r, _)| r.end <= at || r.start >= at + insert.len()),
            "a span covers the inserted text: {got:?}"
        );
        // ...and every span describes the text it is drawn on (here: equal to a fresh parse,
        // since the inserted spaces change no token).
        assert!(
            got.iter().all(|s| want.contains(s)),
            "stale span: {:?}",
            got.iter().find(|s| !want.contains(s))
        );
    }
    assert!(h.wait(PARSE_WAIT));
    assert_eq!(h.spans(window.clone()), want);
    eprintln!(
        "edit during initial parse: {} distinct results, {from_old_version} with shifted version-0 spans",
        seen.len()
    );
    assert!(
        from_old_version > 0,
        "version 0 was never seen before version 1 landed"
    );
}

#[test]
fn edits_after_a_parse_shift_old_spans_until_the_reparse_lands() {
    let text = rust_source(1_000_000);
    let mut rope = Rope::from_str(&text);
    let mut h = Highlighter::new(registry(), "rust", rope.clone(), options(), None);
    assert!(h.wait(PARSE_WAIT));
    let mut stale_checks = 0;
    for round in 0..20 {
        let text = rope.to_string();
        let at = text[400_000 + round * 1000..].find("let").unwrap() + 400_000 + round * 1000;
        let window = at - 500..at + 500;
        let before = h.spans(window.clone());
        // Replace `let` by `let ` + `mut ` (inserted after the keyword).
        let e = Edit::from_chars(&rope, at + 4, at + 4, "mut ");
        rope.insert(at + 4, "mut ");
        h.edit(&[e], rope.clone());
        let got = h.spans(at - 500..at + 504);
        if h.status() == HighlightStatus::Parsing {
            stale_checks += 1;
            let shifted: Vec<_> = before
                .iter()
                .filter_map(|(r, c)| {
                    if r.end <= at + 4 {
                        Some((r.clone(), *c))
                    } else if r.start >= at + 4 {
                        Some((r.start + 4..r.end + 4, *c))
                    } else {
                        None
                    }
                })
                .collect();
            assert_eq!(got, shifted, "round {round}: old spans moved with the text");
            assert!(
                got.iter()
                    .all(|(r, _)| r.end <= at + 4 || r.start >= at + 8),
                "nothing on the inserted text"
            );
        }
        assert!(h.wait(PARSE_WAIT));
        assert_eq!(
            class_at(&h, &rope.to_string(), "mut", round),
            Some(Capture::Keyword),
            "round {round}: new text coloured after the reparse"
        );
    }
    eprintln!("stale-window checks while a reparse was in flight: {stale_checks}/20");
    assert!(stale_checks > 0, "no edit was observed before its reparse");
}

#[test]
fn dropping_the_highlighter_stops_its_worker() {
    let text = rust_source(5_000_000);
    let notify: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
    let h = Highlighter::new(
        registry(),
        "rust",
        Rope::from_str(&text),
        HighlightOptions::default(),
        Some(notify.clone()),
    );
    assert_eq!(h.status(), HighlightStatus::Parsing);
    let t = Instant::now();
    drop(h);
    assert!(t.elapsed() < Duration::from_millis(50), "drop never waits");
    let deadline = Instant::now() + Duration::from_secs(10);
    // The worker holds the only other reference to the notifier.
    while Arc::strong_count(&notify) > 1 && Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert_eq!(
        Arc::strong_count(&notify),
        1,
        "worker still running 10 s after drop"
    );
}

#[test]
fn notifier_runs_when_spans_land() {
    let count = Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    let mut rope = Rope::from_str("fn a() {}\n");
    let mut h = Highlighter::new(
        registry(),
        "rust",
        rope.clone(),
        HighlightOptions::default(),
        Some(Arc::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
        })),
    );
    assert!(h.wait(PARSE_WAIT));
    let deadline = Instant::now() + Duration::from_secs(10);
    while count.load(Ordering::SeqCst) < 1 && Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(count.load(Ordering::SeqCst) >= 1);
    let e = Edit::from_chars(&rope, 10, 10, "fn b() {}\n");
    rope.insert(10, "fn b() {}\n");
    h.edit(&[e], rope);
    assert!(h.wait(PARSE_WAIT));
    while count.load(Ordering::SeqCst) < 2 && Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(count.load(Ordering::SeqCst) >= 2);
}

#[test]
fn reset_never_shows_spans_of_the_old_text() {
    let mut h = highlighter("rust", "fn a() { let x = \"s\"; }\n");
    h.reset(Rope::from_str("// only a comment line here\n"));
    let got = h.spans(0..28);
    assert!(
        got.iter().all(|(_, c)| *c == Capture::Comment),
        "old spans after reset: {got:?}"
    );
    assert!(h.wait(PARSE_WAIT));
    assert_eq!(h.spans(0..28), vec![(0..27, Capture::Comment)]);
}

#[test]
fn language_without_grammar_stays_plain() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("languages.toml");
    std::fs::write(&path, "[elixir]\nextensions = [\"ex\"]\n").unwrap();
    let (langs, _) = Languages::load(Some(&path));
    let (reg, _) = LanguageRegistry::new(&langs);
    let h = Highlighter::new(
        Arc::new(reg),
        "elixir",
        Rope::from_str("defmodule A do end"),
        HighlightOptions::default(),
        None,
    );
    assert_eq!(
        h.status(),
        HighlightStatus::Disabled(Disabled::NoGrammar {
            language: "elixir".into()
        })
    );
    assert!(h.spans(0..18).is_empty());
}

// Perf: run with `cargo test -p vr-syntax --release --test highlight -- --ignored --nocapture`.

fn edit_to_spans(label: &str, lang: &str, text: String, at: usize, window: Range<usize>) {
    let mut rope = Rope::from_str(&text);
    drop(text);
    let t = Instant::now();
    let mut h = Highlighter::new(
        registry(),
        lang,
        rope.clone(),
        HighlightOptions::default(),
        None,
    );
    assert!(h.wait(PARSE_WAIT));
    eprintln!("{label}: initial parse {:?}", t.elapsed());
    let mut worst = Duration::ZERO;
    let mut worst_spans = Duration::ZERO;
    let mut worst_edit = Duration::ZERO;
    let mut total = Duration::ZERO;
    let rounds = 20;
    for i in 0..rounds {
        let t = Instant::now();
        let pos = rope.byte_to_char(at + i);
        let e = Edit::from_chars(&rope, pos, pos, "x");
        worst_edit = worst_edit.max(t.elapsed());
        rope.insert(pos, "x");
        h.edit(&[e], rope.clone());
        let ts = Instant::now();
        let _ = h.spans(window.clone());
        worst_spans = worst_spans.max(ts.elapsed());
        assert!(h.wait(PARSE_WAIT));
        let ts = Instant::now();
        let spans = h.spans(window.clone());
        worst_spans = worst_spans.max(ts.elapsed());
        assert!(!spans.is_empty());
        let d = t.elapsed();
        worst = worst.max(d);
        total += d;
    }
    eprintln!(
        "{label}: edit → coloured spans avg {:?}, worst {worst:?}; spans() worst {worst_spans:?}; \
         Edit::from_chars worst {worst_edit:?}",
        total / rounds as u32
    );
}

#[test]
#[ignore = "perf: run in release with --ignored"]
fn perf_edit_to_spans_5mb_file() {
    let text = rust_source(5_000_000);
    let at = 2_500_000;
    edit_to_spans("5 MB rust", "rust", text, at, at - 3000..at + 3000);
}

#[test]
#[ignore = "perf: run in release with --ignored"]
fn perf_edit_to_spans_10mb_single_line() {
    let mut text = String::with_capacity(10_000_100);
    let mut i = 0;
    while text.len() < 10_000_000 {
        text.push_str(&format!(
            "var a{i}=\"s{i}\";function f{i}(x){{return x+{i}}}"
        ));
        i += 1;
    }
    let at = 5_000_000;
    edit_to_spans(
        "10 MB single line js",
        "javascript",
        text,
        at,
        at - 200..at + 200,
    );
}

#[test]
fn edits_after_a_timeout_stay_plain_and_reset_tries_again() {
    let text = rust_source(2_000_000);
    let mut rope = Rope::from_str(&text);
    let options = HighlightOptions {
        parse_timeout: Duration::from_micros(1),
        ..HighlightOptions::default()
    };
    let mut h = Highlighter::new(registry(), "rust", rope.clone(), options, None);
    assert!(h.wait(PARSE_WAIT));
    let e = Edit::from_chars(&rope, 0, 0, "// x\n");
    rope.insert(0, "// x\n");
    h.edit(&[e], rope.clone());
    assert!(matches!(
        h.status(),
        HighlightStatus::Disabled(Disabled::Timeout { .. })
    ));
    assert!(h.spans(0..100).is_empty());
    // A reload of a small file gets a fresh worker (still with the 1 µs limit, so a tiny text).
    h.reset(Rope::from_str("fn a() {}"));
    assert!(h.wait(PARSE_WAIT));
    assert!(
        matches!(
            h.status(),
            HighlightStatus::Ready | HighlightStatus::Disabled(Disabled::Timeout { .. })
        ),
        "{:?}",
        h.status()
    );
}

#[test]
fn spans_call_is_bounded_by_the_query_timeout() {
    let text = rust_source(1_000_000);
    let options = HighlightOptions {
        query_timeout: Duration::ZERO,
        ..HighlightOptions::default()
    };
    let h = Highlighter::new(registry(), "rust", Rope::from_str(&text), options, None);
    assert!(h.wait(PARSE_WAIT));
    // The whole document in one call with no time: returns at once with what it has.
    let t = Instant::now();
    let spans = h.spans(0..text.len());
    assert!(
        t.elapsed() < Duration::from_secs(1),
        "spans took {:?}",
        t.elapsed()
    );
    assert!(spans.windows(2).all(|w| w[0].0.end <= w[1].0.start));
}

#[test]
fn a_query_cut_by_its_budget_says_so() {
    let text = rust_source(20_000);
    let options = HighlightOptions {
        query_timeout: Duration::ZERO,
        ..HighlightOptions::default()
    };
    let mut h = Highlighter::new(registry(), "rust", Rope::from_str(&text), options, None);
    assert!(h.wait(PARSE_WAIT));
    let cut = h.query(0..text.len());
    assert!(cut.truncated, "no time at all: the query is cut");
    h.set_query_timeout(PARSE_WAIT);
    let full = h.query(0..text.len());
    assert!(!full.truncated);
    assert!(full.spans.len() > cut.spans.len());
    assert_eq!(h.spans(0..text.len()), full.spans);
    // Nothing to look at is never cut.
    assert_eq!(h.query(5..5), vr_syntax::SpanQuery::default());
}

#[test]
#[ignore = "perf: run in release with --ignored"]
fn perf_edit_to_spans_typical_files() {
    let text = rust_source(200_000);
    edit_to_spans("200 KB rust", "rust", text, 100_000, 97_000..103_000);
    let mut html = String::new();
    let mut i = 0;
    while html.len() < 200_000 {
        html.push_str(&format!(
            "<div class=\"c{i}\"><p>Item {i}</p>\n<script>const v{i} = {i};</script>\n<style>.c{i} {{ color: red; }}</style></div>\n"
        ));
        i += 1;
    }
    edit_to_spans(
        "200 KB html + script/style",
        "html",
        html,
        100_000,
        97_000..103_000,
    );
}

/// CPU time (ns) of each live highlighter worker thread of this process, by thread id (Linux).
fn worker_cpu_ns() -> std::collections::HashMap<String, u64> {
    let mut cpu = std::collections::HashMap::new();
    let Ok(tasks) = std::fs::read_dir("/proc/self/task") else {
        return cpu;
    };
    for task in tasks.flatten() {
        let comm = std::fs::read_to_string(task.path().join("comm")).unwrap_or_default();
        // Thread names are cut to 15 bytes: "vr-syntax php_o".
        if !comm.starts_with("vr-syntax php_o") {
            continue;
        }
        let stat = std::fs::read_to_string(task.path().join("schedstat")).unwrap_or_default();
        if let Some(ns) = stat.split_whitespace().next().and_then(|s| s.parse().ok()) {
            cpu.insert(task.file_name().to_string_lossy().into_owned(), ns);
        }
    }
    cpu
}

/// Worker CPU while someone types 200 characters at 30 ms intervals into a 200 KB PHP file that
/// keeps a syntax error: the fresh-parse check must not run during the burst. `verify_idle: 0`
/// is the behaviour without the pause (a check after every reparse). Run it alone (`--exact`):
/// another php_only worker started meanwhile would make the thread ambiguous.
#[test]
#[ignore = "perf: run in release with --ignored"]
fn perf_worker_cpu_while_typing_into_text_with_an_error() {
    let mut text = String::from("<?php\nfunction broken( {{\n");
    while text.len() < 200_000 {
        text.push_str(&sample("php_only.php"));
    }
    for options in [no_idle(), HighlightOptions::default()] {
        let label = format!("verify_idle {:?}", options.verify_idle);
        type_into_text_with_an_error(&label, &text, options);
    }
}

fn type_into_text_with_an_error(label: &str, text: &str, options: HighlightOptions) {
    let mut rope = Rope::from_str(text);
    let before = worker_cpu_ns();
    let mut h = Highlighter::new(registry(), "php_only", rope.clone(), options, None);
    assert!(h.wait(PARSE_WAIT));
    let new: Vec<String> = worker_cpu_ns()
        .into_keys()
        .filter(|tid| !before.contains_key(tid))
        .collect();
    let [tid] = new.as_slice() else {
        panic!("{} new php_only workers: run this test alone", new.len());
    };
    let ours = || worker_cpu_ns()[tid];
    let start_cpu = ours();
    let t = Instant::now();
    let at = rope.len_chars() / 2;
    for i in 0..200 {
        let e = Edit::from_chars(&rope, at + i, at + i, "x");
        rope.insert(at + i, "x");
        h.edit(&[e], rope.clone());
        std::thread::sleep(Duration::from_millis(30));
    }
    let typing_cpu = ours() - start_cpu;
    let typing = t.elapsed();
    assert!(h.wait(PARSE_WAIT));
    let total_cpu = ours() - start_cpu;
    eprintln!(
        "200 KB php_only with an error, 200 chars at 30 ms, {label}: worker CPU during typing {:?} over \
         {typing:?} ({:.0}% of a core); including the final check {:?}",
        Duration::from_nanos(typing_cpu),
        typing_cpu as f64 / typing.as_nanos() as f64 * 100.0,
        Duration::from_nanos(total_cpu)
    );
}
