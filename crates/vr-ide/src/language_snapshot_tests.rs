//! One snapshot per language (M4-4): each sample of `vr-syntax` is opened in the editor, drawn
//! at 80x24 in the synthwave theme once its highlighter is ready, and the editor's text area is
//! captured as a class map (one letter per cell) instead of colours, so a theme tweak does not
//! churn the snapshots.

use std::collections::BTreeSet;
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use vr_syntax::Capture;

use crate::app::{App, Focus};
use crate::app_tests::test_app;
use crate::theme::{ColorDepth, Theme};
use crate::ui::{editor_view, render_with};

const WAIT: Duration = Duration::from_secs(20);

fn theme() -> Theme {
    Theme::synthwave_with(ColorDepth::TrueColor)
}

/// The letter of a class in the map; `.` is plain text.
fn letter(c: Capture) -> char {
    match c {
        Capture::Keyword => 'k',
        Capture::String => 's',
        Capture::Comment => 'c',
        Capture::Function => 'f',
        Capture::Type => 't',
        Capture::Number => 'n',
        Capture::Constant => 'C',
        Capture::VariableBuiltin => 'v',
        Capture::Property => 'p',
        Capture::Tag => 'g',
        Capture::Attribute => 'a',
        Capture::Operator => 'o',
        Capture::Punctuation => 'u',
        Capture::Escape => 'e',
        Capture::Other => '.',
    }
}

struct Shot {
    /// Rendered text of each editor row (gutter cut off), right-trimmed.
    text: Vec<String>,
    /// The class map rows, same length as `text`, right-trimmed.
    map: Vec<String>,
}

impl Shot {
    /// The classes drawn anywhere.
    fn classes(&self) -> BTreeSet<char> {
        self.map
            .iter()
            .flat_map(|r| r.chars())
            .filter(|&c| c != '.' && c != ' ')
            .collect()
    }

    /// The class letters of the first occurrence of `needle` in the drawn text.
    fn at(&self, needle: &str) -> String {
        for (t, m) in self.text.iter().zip(&self.map) {
            if let Some(byte) = t.find(needle) {
                let col = t[..byte].chars().count();
                return m.chars().skip(col).take(needle.chars().count()).collect();
            }
        }
        panic!("{needle:?} is not on screen:\n{}", self.render());
    }

    /// Source line, then its map line: reviewable without a colour table.
    fn render(&self) -> String {
        let mut out = String::new();
        for (t, m) in self.text.iter().zip(&self.map) {
            out.push_str(&format!("{t}\n{m}\n"));
        }
        out
    }
}

/// Opens `content` as `name` (with an optional `languages.toml` body), waits for the parse and
/// captures the editor area of an 80x24 screen.
fn shoot(name: &str, content: &str, languages: Option<&str>) -> Shot {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    std::fs::write(&path, content).unwrap();
    let (mut app, _) = test_app(dir.path());
    if let Some(toml) = languages {
        let file = dir.path().join("languages.toml");
        std::fs::write(&file, toml).unwrap();
        let warnings = app.load_languages(Some(&file));
        assert!(warnings.is_empty(), "{warnings:?}");
    }
    app.open_path(&path).unwrap();
    app.focus = Focus::Editor;
    assert!(app.syntax.language(0).is_some(), "{name} has no language");

    let mut term = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let mut draw = |app: &mut App| -> Buffer {
        term.draw(|f| render_with(f, app, &theme())).unwrap();
        term.backend().buffer().clone()
    };
    draw(&mut app);
    assert!(app.syntax.wait(app.active, WAIT), "parse did not land");
    let buf = draw(&mut app);

    let theme = theme();
    let ed = app.layout.editor;
    let gutter = editor_view::gutter_width(app.active_doc().unwrap().buffer.text());
    let doc = app.active_doc().unwrap();
    let rope = doc.buffer.text();
    let tab_width = doc.buffer.options().tab_width;
    let width = (ed.width - gutter) as usize;
    let lines = content.lines().count().min(ed.height as usize);
    let (mut text, mut map) = (Vec::new(), Vec::new());
    for y in 0..lines {
        // The class of a cell comes from the highlighter's spans (several classes share a
        // synthwave colour, so colours alone are ambiguous); the drawn colour of the same cell
        // is then checked against the theme's colour for that class.
        let line = rope.line(y);
        let start = rope.line_to_byte(y);
        let spans = app
            .syntax
            .spans(app.active, start..start + line.len_bytes());
        let (mut t, mut m) = (vec![' '; width], vec!['.'; width]);
        editor_view::walk_line(line, tab_width, |cell| {
            if cell.col >= width {
                return false;
            }
            let byte = start + line.char_to_byte(cell.start);
            let class = spans
                .iter()
                .find(|(r, _)| r.contains(&byte))
                .map_or(Capture::Other, |s| s.1);
            let drawn = &buf[(ed.x + gutter + cell.col as u16, ed.y + y as u16)];
            let want = match class {
                Capture::Other => theme.text.fg.unwrap_or(drawn.fg),
                c => theme.syntax(c).fg.unwrap(),
            };
            assert_eq!(
                drawn.fg, want,
                "{name} line {y} col {}: {class:?}",
                cell.col
            );
            t[cell.col] = cell
                .symbol
                .chars()
                .next()
                .filter(|c| !c.is_control())
                .unwrap_or(' ');
            m[cell.col] = letter(class);
            true
        });
        text.push(t.iter().collect::<String>().trim_end().to_string());
        map.push(
            m.iter()
                .collect::<String>()
                .trim_end_matches('.')
                .to_string(),
        );
    }
    Shot { text, map }
}

fn sample(file: &str) -> String {
    let path = format!(
        "{}/../vr-syntax/tests/samples/{file}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Snapshot + at least three classes for one sample; returns the shot for extra assertions.
fn check(snapshot: &str, file: &str, as_name: &str, languages: Option<&str>) -> Shot {
    let shot = shoot(as_name, &sample(file), languages);
    assert!(
        shot.classes().len() >= 3,
        "{file}: only {:?}\n{}",
        shot.classes(),
        shot.render()
    );
    insta::assert_snapshot!(snapshot, shot.render());
    shot
}

macro_rules! plain_sample {
    ($test:ident, $file:literal) => {
        #[test]
        fn $test() {
            check(stringify!($test), $file, $file, None);
        }
    };
}

plain_sample!(css, "css.css");
plain_sample!(dart, "dart.dart");
plain_sample!(go, "go.go");
plain_sample!(javascript, "javascript.js");
plain_sample!(json, "json.json");
plain_sample!(python, "python.py");
plain_sample!(rust, "rust.rs");
plain_sample!(scss, "scss.scss");
plain_sample!(toml, "toml.toml");
plain_sample!(tsx, "tsx.tsx");
plain_sample!(typescript, "typescript.ts");
plain_sample!(yaml, "yaml.yaml");
plain_sample!(blade, "blade.blade.php");
plain_sample!(twig, "twig.html.twig");

#[test]
fn xml() {
    let shot = check("xml", "sample.xml", "sample.xml", None);
    assert_eq!(
        shot.at("catalog xmlns").chars().next(),
        Some('g'),
        "element name"
    );
    assert_eq!(shot.at("currency"), "pppppppp", "attribute name");
    assert_eq!(
        shot.at("\"USD\"").chars().nth(1),
        Some('s'),
        "attribute value"
    );
    assert_eq!(shot.at("<!-- A").chars().nth(1), Some('c'), "comment");
}

/// DTD files open as the `dtd` language through their own extension.
#[test]
fn dtd() {
    let shot = check("dtd", "sample.dtd", "sample.dtd", None);
    assert_eq!(shot.at("ELEMENT"), "kkkkkkk");
}

/// The built-in table has no extension for `php_only` (it is the Blade / embedded-snippet
/// grammar), so the test names it in a `languages.toml`; a plain `.php` file would be `php`.
#[test]
fn php_only() {
    let toml = "[php_only]\nfiles = [\"php_only.sample\"]\n";
    let shot = check("php_only", "php_only.php", "php_only.sample", Some(toml));
    assert_eq!(shot.at("function"), "kkkkkkkk");
    assert_eq!(shot.at("\"Hello, world\"").chars().next(), Some('s'));
}

/// Inline Markdown is the injected half of `markdown`; opened on its own by id.
#[test]
fn markdown_inline() {
    let toml = "[markdown_inline]\nfiles = [\"markdown_inline.sample\"]\n";
    check(
        "markdown_inline",
        "markdown_inline.md",
        "markdown_inline.sample",
        Some(toml),
    );
}

// Embedded languages (AC-13a): the cells of the embedded part carry the embedded language's
// classes.

#[test]
fn html_embeds_javascript_and_css() {
    let shot = check("html", "html.html", "html.html", None);
    assert_eq!(shot.at("margin"), "pppppp", "css property");
    assert_eq!(shot.at("\"hi\"").chars().next(), Some('s'), "js string");
    assert_eq!(shot.at("0;"), "nu", "css number then punctuation");
}

#[test]
fn vue_embeds_typescript_and_css() {
    let shot = check("vue", "vue.vue", "vue.vue", None);
    assert_eq!(shot.at("import"), "kkkkkk", "script setup: typescript");
    assert_eq!(shot.at("const"), "kkkkk");
    assert_eq!(shot.at("\"vue\"").chars().next(), Some('s'));
    assert_eq!(shot.at("color"), "ppppp", "style: css property");
    assert_eq!(shot.at("button").chars().next(), Some('g'), "template tag");
}

#[test]
fn php_embeds_html_around_the_code() {
    let shot = check("php", "php.php", "php.php", None);
    assert_eq!(shot.at("body").chars().next(), Some('g'), "html tag");
    assert_eq!(shot.at("function"), "kkkkkkkk", "php keyword");
    assert_eq!(shot.at("// Greets").chars().next(), Some('c'));
}

#[test]
fn markdown_embeds_a_rust_fence() {
    let shot = check("markdown", "markdown.md", "markdown.md", None);
    assert_eq!(shot.at("fn main"), "kk.ffff");
}
