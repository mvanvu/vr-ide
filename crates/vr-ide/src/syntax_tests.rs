//! Syntax highlighting in the editor (M4-3): documents get a highlighter by language, it
//! follows edits, undo, reloads and Save As, and the editor draws its spans in theme colours.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::style::{Color, Style};
use vr_syntax::Capture;

use crate::app::{App, Focus};
use crate::app_tests::test_app;
use crate::commands::CmdArgs;
use crate::theme::{ColorDepth, Theme};
use crate::ui::{editor_view, render_with};

const WAIT: Duration = Duration::from_secs(20);

fn theme() -> Theme {
    Theme::synthwave_with(ColorDepth::TrueColor)
}

/// A project with `name` holding `content`, opened in a tab.
fn open(name: &str, content: &str) -> (tempfile::TempDir, PathBuf, App) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    std::fs::write(&path, content).unwrap();
    let (mut app, _) = test_app(dir.path());
    app.open_path(&path).unwrap();
    app.focus = Focus::Editor;
    (dir, path, app)
}

fn draw(app: &mut App) -> Buffer {
    let mut t = Terminal::new(TestBackend::new(60, 12)).unwrap();
    t.draw(|f| render_with(f, app, &theme())).unwrap();
    t.backend().buffer().clone()
}

/// Draws, waits until the active document's spans describe its text, and draws again.
fn draw_settled(app: &mut App) -> Buffer {
    draw(app);
    assert!(app.syntax.wait(app.active, WAIT), "parse did not land");
    draw(app)
}

/// The cell of char `col` (no wide chars before it) on text line `line`.
fn cell(app: &App, buf: &Buffer, line: u16, col: u16) -> (String, Style) {
    let ed = app.layout.editor;
    let g = editor_view::gutter_width(app.active_doc().unwrap().buffer.text());
    let c = &buf[(ed.x + g + col, ed.y + line)];
    let style = Style::default().fg(c.fg).bg(c.bg).add_modifier(c.modifier);
    (c.symbol().to_string(), style)
}

fn fg(app: &App, buf: &Buffer, line: u16, col: u16) -> Color {
    cell(app, buf, line, col).1.fg.unwrap()
}

fn colour(c: Capture) -> Color {
    theme().syntax(c).fg.unwrap()
}

fn key(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    app.handle_key(KeyEvent::new(code, mods));
}

fn typ(app: &mut App, s: &str) {
    for c in s.chars() {
        key(app, KeyCode::Char(c), KeyModifiers::NONE);
    }
}

fn edit_mode(app: &mut App) {
    key(app, KeyCode::Char('i'), KeyModifiers::CONTROL);
    assert!(app.edit_mode());
}

const RUST: &str = "fn main() {\n    // note\n    let s = \"hi\";\n}\n";

#[test]
fn rust_file_is_drawn_in_the_theme_colours() {
    let (_d, _p, mut app) = open("main.rs", RUST);
    assert_eq!(app.syntax.language(0), Some("rust"));
    let buf = draw_settled(&mut app);
    assert_eq!(cell(&app, &buf, 0, 0).0, "f");
    assert_eq!(fg(&app, &buf, 0, 0), colour(Capture::Keyword));
    assert_eq!(fg(&app, &buf, 0, 3), colour(Capture::Function), "main");
    assert_eq!(fg(&app, &buf, 1, 4), colour(Capture::Comment), "//");
    assert_eq!(fg(&app, &buf, 2, 4), colour(Capture::Keyword), "let");
    assert_eq!(fg(&app, &buf, 2, 12), colour(Capture::String), "\"hi\"");
    // Background stays the editor's.
    assert_eq!(cell(&app, &buf, 0, 0).1.bg, theme().text.bg);
}

#[test]
fn json_keys_are_drawn_in_the_property_colour() {
    let (_d, _p, mut app) = open("a.json", "{\"key\": \"v\"}\n");
    let buf = draw_settled(&mut app);
    assert_eq!(cell(&app, &buf, 0, 2).0, "k");
    assert_eq!(fg(&app, &buf, 0, 2), colour(Capture::Property));
    assert_ne!(fg(&app, &buf, 0, 2), theme().text.fg.unwrap());
    assert_eq!(fg(&app, &buf, 0, 10), colour(Capture::String), "value");
}

#[test]
fn the_first_frame_does_not_wait_for_the_parse() {
    let big = "fn f() { let x = \"s\"; }\n".repeat(200_000);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.rs");
    std::fs::write(&path, &big).unwrap();
    let (mut app, _) = test_app(dir.path());
    // Opening creates the highlighter, so it is inside the timed part.
    let t = std::time::Instant::now();
    app.open_path(&path).unwrap();
    assert_eq!(app.syntax.language(0), Some("rust"));
    draw(&mut app);
    assert!(
        t.elapsed() < Duration::from_secs(2),
        "first frame took {:?}",
        t.elapsed()
    );
}

#[test]
fn typing_recolours_and_undo_restores() {
    let (_d, _p, mut app) = open("a.rs", "fn a() {}\nx\n");
    edit_mode(&mut app);
    draw_settled(&mut app);
    // "x" on line 2 becomes a comment.
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    typ(&mut app, "// ");
    let buf = draw_settled(&mut app);
    for col in 0..4 {
        assert_eq!(
            fg(&app, &buf, 1, col),
            colour(Capture::Comment),
            "col {col}"
        );
    }
    assert_eq!(fg(&app, &buf, 0, 0), colour(Capture::Keyword));
    key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    let buf = draw_settled(&mut app);
    assert_eq!(cell(&app, &buf, 1, 0).0, "x");
    assert_ne!(fg(&app, &buf, 1, 0), colour(Capture::Comment));
    assert_eq!(fg(&app, &buf, 0, 0), colour(Capture::Keyword));
    // Redo brings the comment back.
    key(&mut app, KeyCode::Char('y'), KeyModifiers::CONTROL);
    let buf = draw_settled(&mut app);
    assert_eq!(fg(&app, &buf, 1, 0), colour(Capture::Comment));
    let s = app.syntax.stats(0);
    assert!(s.diffs >= 3 && s.resets == 0, "{s:?}");
}

#[test]
fn replace_all_hands_its_changes_to_the_highlighter() {
    let (_d, _p, mut app) = open("a.rs", "fn a() { b(); }\nfn b() {}\nfn c() { b(); }\n");
    edit_mode(&mut app);
    draw_settled(&mut app);
    key(&mut app, KeyCode::Char('h'), KeyModifiers::CONTROL);
    typ(&mut app, "fn");
    key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
    typ(&mut app, "// fn");
    app.run_command("find.replace_all", CmdArgs::None);
    let buf = draw_settled(&mut app);
    assert_eq!(
        app.active_doc().unwrap().buffer.text().line(1).to_string(),
        "// fn b() {}\n"
    );
    for line in 0..3 {
        assert_eq!(fg(&app, &buf, line, 0), colour(Capture::Comment), "{line}");
    }
    assert_eq!(app.syntax.stats(0).changes, 1, "{:?}", app.syntax.stats(0));
}

#[test]
fn reload_from_disk_resets_the_highlighter() {
    let (_d, path, mut app) = open("a.rs", "fn a() {}\n");
    draw_settled(&mut app);
    std::fs::write(&path, "// all comment now\n").unwrap();
    app.handle_fs_event(vr_core::FsEvent::Modified(path.clone()));
    assert_eq!(
        app.active_doc().unwrap().buffer.text().to_string(),
        "// all comment now\n"
    );
    let buf = draw_settled(&mut app);
    assert_eq!(fg(&app, &buf, 0, 0), colour(Capture::Comment));
    assert_eq!(app.syntax.stats(0).resets, 1);
}

#[test]
fn save_as_another_extension_switches_the_language() {
    let (d, _p, mut app) = open("a.rs", "# note\nx = 1\n");
    let buf = draw_settled(&mut app);
    assert_ne!(
        fg(&app, &buf, 0, 0),
        colour(Capture::Comment),
        "rust: not a comment"
    );
    let target = d.path().join("a.py");
    app.run_command("file.save_as", CmdArgs::Path(target));
    assert_eq!(
        app.active_doc().unwrap().path.as_deref(),
        Some(d.path().join("a.py").as_path())
    );
    let buf = draw_settled(&mut app);
    assert_eq!(app.syntax.language(0), Some("python"));
    assert_eq!(fg(&app, &buf, 0, 0), colour(Capture::Comment));
}

fn notices(app: &App) -> Vec<String> {
    app.notices.iter().map(|n| n.text.clone()).collect()
}

#[test]
fn files_over_max_highlight_bytes_open_plain_with_one_notice() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.rs");
    std::fs::write(&path, RUST).unwrap();
    let (mut app, _) = test_app(dir.path());
    app.settings.max_highlight_bytes = 10;
    app.open_path(&path).unwrap();
    let buf = draw_settled(&mut app);
    draw(&mut app);
    let found: Vec<String> = notices(&app)
        .into_iter()
        .filter(|n| n.contains("max_highlight_bytes"))
        .collect();
    assert_eq!(found.len(), 1, "{:?}", notices(&app));
    assert!(
        found[0].contains("big.rs") && found[0].contains("plain text"),
        "{found:?}"
    );
    assert_eq!(fg(&app, &buf, 0, 0), theme().text.fg.unwrap());
}

/// A disabled highlighter (here: too large) only tracks the buffer version: typing does not
/// diff the text and a reload does not scan it for lone CRs.
#[test]
fn a_disabled_document_is_neither_diffed_nor_scanned() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.json");
    std::fs::write(&path, "{\"a\": 1}\n").unwrap();
    let (mut app, _) = test_app(dir.path());
    app.settings.max_highlight_bytes = 4;
    app.open_path(&path).unwrap();
    app.focus = Focus::Editor;
    draw_settled(&mut app);
    edit_mode(&mut app);
    typ(&mut app, "xy");
    draw(&mut app);
    key(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    draw(&mut app);
    std::fs::write(&path, "{\"b\": 2}\n").unwrap();
    app.handle_fs_event(vr_core::FsEvent::Modified(path.clone()));
    draw(&mut app);
    let s = app.syntax.stats(0);
    assert_eq!((s.diffs, s.changes, s.scans), (0, 0, 0), "{s:?}");
    assert_eq!(s.resets, 1, "{s:?}");
    // A reload that brings the file under the limit scans it and highlights it again.
    std::fs::write(&path, "[3]\n").unwrap();
    app.handle_fs_event(vr_core::FsEvent::Modified(path.clone()));
    let buf = draw_settled(&mut app);
    assert_eq!(app.syntax.stats(0).scans, 1);
    assert_eq!(cell(&app, &buf, 0, 1).0, "3");
    assert_eq!(fg(&app, &buf, 0, 1), colour(Capture::Number));
}

#[test]
fn unknown_extension_opens_plain_without_a_notice() {
    let (_d, _p, mut app) = open("notes.xyz", "fn main() {}\n");
    assert_eq!(app.syntax.language(0), None);
    let buf = draw(&mut app);
    assert!(app.notices.is_empty(), "{:?}", notices(&app));
    assert_eq!(fg(&app, &buf, 0, 0), theme().text.fg.unwrap());
}

#[test]
fn a_language_without_a_compiled_grammar_is_a_startup_notice() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("languages.toml");
    std::fs::write(
        &file,
        "[cobol]\ngrammar = \"cobol\"\nextensions = [\"cbl\"]\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("x.cbl"), "IDENTIFICATION DIVISION.\n").unwrap();
    let (mut app, _) = test_app(dir.path());
    let warnings = app.load_languages(Some(&file));
    app.report_config(warnings);
    let all = notices(&app);
    assert!(
        all.iter()
            .any(|n| n.contains("[cobol]") && n.contains("not compiled")),
        "{all:?}"
    );
    let before = app.notices.len();
    app.open_path(Path::new("x.cbl")).unwrap();
    draw(&mut app);
    assert_eq!(app.syntax.language(0), None, "opens plain");
    assert_eq!(app.notices.len(), before, "{:?}", notices(&app));
}

#[test]
fn read_mode_and_edit_mode_draw_the_same_colours() {
    let (_d, _p, mut app) = open("main.rs", RUST);
    let read = draw_settled(&mut app);
    // Colours are there to compare.
    assert_eq!(fg(&app, &read, 0, 0), colour(Capture::Keyword));
    edit_mode(&mut app);
    let edit = draw(&mut app);
    let ed = app.layout.editor;
    for y in ed.y..ed.y + 4 {
        for x in ed.x..ed.x + ed.width {
            assert_eq!(read[(x, y)].fg, edit[(x, y)].fg, "({x},{y})");
        }
    }
}

/// dark and light paint on the terminal's own background, where ANSI syntax colours cannot be
/// checked against the selection and find-match surfaces (D179): cells there are drawn in the
/// plain text colour, as before syntax highlighting; the rest of the line keeps its colours.
#[test]
fn terminal_colour_themes_draw_plain_text_under_the_selection_and_find_matches() {
    for (name, t) in [("dark", Theme::dark()), ("light", Theme::light())] {
        let (_d, _p, mut app) = open("main.rs", RUST);
        let mut term = Terminal::new(TestBackend::new(60, 12)).unwrap();
        let mut frame = |app: &mut App| {
            term.draw(|f| render_with(f, app, &t)).unwrap();
            term.backend().buffer().clone()
        };
        let plain = t.text.fg.unwrap_or(Color::Reset);
        // Select "fn".
        key(&mut app, KeyCode::Right, KeyModifiers::SHIFT);
        key(&mut app, KeyCode::Right, KeyModifiers::SHIFT);
        frame(&mut app);
        assert!(app.syntax.wait(0, WAIT), "{name}: parse");
        let buf = frame(&mut app);
        let (sym, s) = cell(&app, &buf, 0, 0);
        assert_eq!(sym, "f", "{name}");
        assert_eq!(
            (s.fg, s.bg),
            (Some(plain), t.selection.bg),
            "{name}: selection"
        );
        assert_eq!(
            fg(&app, &buf, 0, 3),
            t.syntax(Capture::Function).fg.unwrap(),
            "{name}: outside the selection"
        );
        // A find match on "let" (the current match is selected, the other one only found).
        key(&mut app, KeyCode::Left, KeyModifiers::NONE);
        key(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
        typ(&mut app, "let");
        let buf = frame(&mut app);
        let (sym, s) = cell(&app, &buf, 2, 4);
        assert_eq!(sym, "l", "{name}");
        assert_eq!(s.fg, Some(plain), "{name}: find match");
        let bg = s.bg.unwrap();
        assert!(
            Some(bg) == t.find_match.bg || Some(bg) == t.selection.bg,
            "{name}: {bg:?}"
        );
    }
}

#[test]
fn selection_and_find_matches_keep_the_syntax_colour() {
    let (_d, _p, mut app) = open("main.rs", RUST);
    // Select "fn".
    key(&mut app, KeyCode::Right, KeyModifiers::SHIFT);
    key(&mut app, KeyCode::Right, KeyModifiers::SHIFT);
    let buf = draw_settled(&mut app);
    let (_, s) = cell(&app, &buf, 0, 0);
    assert_eq!(
        (s.fg, s.bg),
        (theme().syntax(Capture::Keyword).fg, theme().selection.bg)
    );
    // A find match on "let" (no selection, so the find field starts empty).
    key(&mut app, KeyCode::Left, KeyModifiers::NONE);
    key(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
    typ(&mut app, "let");
    let buf = draw_settled(&mut app);
    let (sym, s) = cell(&app, &buf, 2, 4);
    assert_eq!(sym, "l");
    assert_eq!(s.fg, theme().syntax(Capture::Keyword).fg);
    let bg = s.bg.unwrap();
    assert!(
        Some(bg) == theme().find_match.bg || Some(bg) == theme().selection.bg,
        "{bg:?}"
    );
}

#[test]
fn spans_follow_horizontal_scroll_wide_chars_and_tabs() {
    let long = format!("\tlet s = \"漢字{}\";\n", "a".repeat(120));
    let (_d, _p, mut app) = open("w.rs", &long);
    let buf = draw_settled(&mut app);
    // Tab stop 4: "let" starts at column 4; 漢 (two cells) at 12.
    assert_eq!(cell(&app, &buf, 0, 4).0, "l");
    assert_eq!(fg(&app, &buf, 0, 4), colour(Capture::Keyword));
    assert_eq!(cell(&app, &buf, 0, 13).0, "漢");
    assert_eq!(fg(&app, &buf, 0, 13), colour(Capture::String));
    assert_eq!(fg(&app, &buf, 0, 15), colour(Capture::String), "字");
    // Scrolled to the end: every visible 'a' is still in the string colour.
    key(&mut app, KeyCode::End, KeyModifiers::NONE);
    let buf = draw_settled(&mut app);
    assert!(app.views[0].left > 0);
    let w = app.layout.editor.width
        - editor_view::gutter_width(app.active_doc().unwrap().buffer.text());
    let mut seen = 0;
    for col in 0..w {
        let (sym, s) = cell(&app, &buf, 0, col);
        if sym == "a" {
            seen += 1;
            assert_eq!(s.fg, theme().syntax(Capture::String).fg, "col {col}");
        }
    }
    assert!(seen > 20, "{seen}");
}

#[test]
fn closing_a_tab_drops_its_highlighter_and_keeps_the_others_aligned() {
    let (d, _p, mut app) = open("a.rs", "fn a() {}\n");
    std::fs::write(d.path().join("b.py"), "# c\n").unwrap();
    app.open_path(Path::new("b.py")).unwrap();
    draw(&mut app);
    assert_eq!(app.syntax.language(1), Some("python"));
    app.close_doc(0);
    assert_eq!(app.syntax.len(), 1);
    assert_eq!(app.syntax.language(0), Some("python"));
    let buf = draw_settled(&mut app);
    assert_eq!(fg(&app, &buf, 0, 0), colour(Capture::Comment));
}

#[test]
fn a_landed_parse_asks_the_event_loop_to_redraw() {
    let (_d, _p, mut app) = open("main.rs", RUST);
    draw(&mut app);
    assert!(app.syntax.wait(0, WAIT));
    assert!(app.syntax.take_wake(), "notify did not fire");
    assert!(!app.syntax.take_wake(), "taken once");
}

fn median_and_worst(mut times: Vec<Duration>) -> (Duration, Duration) {
    times.sort();
    (times[times.len() / 2], times[times.len() - 1])
}

/// AC-13b / AC-18 with highlighting on: keystroke-to-frame on highlighted multi-MB files stays
/// in the editor's budget (the reparse runs on the worker; the frame shows the old spans
/// shifted); a known-language file over `max_highlight_bytes` costs no more than a plain one.
/// Also the UI-thread cost of the edit points with and without the lone-CR flag.
/// Release only: `cargo test -p vr-ide --release highlighted_keystroke -- --ignored --nocapture`.
#[test]
#[ignore = "release timing test; run with --release -- --ignored"]
fn highlighted_keystroke_to_frame_fast() {
    use std::time::Instant;
    use vr_syntax::{Edit, LineBreaks};
    let rust_unit = "fn item(x: u32) -> u32 { let s = \"Tiếng Việt\"; x * 2 } // c\n";
    let js_unit = "var a=\"Tiếng Việt\",b=[1,2,3];function f(x){return x*2}\t/*漢字*/";
    let json_unit = "{\"key\": \"Tiếng Việt\", \"n\": [1, 2.5, true, null]},\n";
    let cases = [
        (
            "5 MB rust",
            "big.rs",
            rust_unit.repeat(5 * 1024 * 1024 / rust_unit.len()),
        ),
        (
            "9.5 MB rust",
            "big.rs",
            rust_unit.repeat(19 * 512 * 1024 / rust_unit.len()),
        ),
        (
            "9 MB single-line js",
            "min.js",
            format!(
                "{}\nshort\n",
                js_unit.repeat(9 * 1024 * 1024 / js_unit.len())
            ),
        ),
        // Over max_highlight_bytes: a known language whose highlighter is disabled.
        (
            "100 MB json (disabled)",
            "big.json",
            json_unit.repeat(100 * 1024 * 1024 / json_unit.len()),
        ),
    ];
    for (label, name, text) in cases {
        let (_d, _p, mut app) = open(name, &text);
        assert!(app.syntax.language(0).is_some(), "{label}");
        let mut t = Terminal::new(TestBackend::new(120, 40)).unwrap();
        t.draw(|f| render_with(f, &mut app, &theme())).unwrap();
        assert!(
            app.syntax.wait(0, Duration::from_secs(120)),
            "{label}: parse"
        );
        edit_mode(&mut app);
        // The middle of the text (end of the line for the single-line file).
        let rope = app.docs[0].buffer.text().clone();
        let mid = if rope.len_lines() > 3 {
            rope.line_to_char(rope.len_lines() / 2) + 10
        } else {
            vr_core::text::line_content_end(&rope, 0)
        };
        app.docs[0]
            .buffer
            .set_selection(vr_core::Selection::point(mid));
        app.cursor_moved();
        t.draw(|f| render_with(f, &mut app, &theme())).unwrap();
        let mut times = Vec::new();
        for i in 0..41 {
            let code = if i % 2 == 0 {
                KeyCode::Char('x')
            } else {
                KeyCode::Backspace
            };
            let s = Instant::now();
            key(&mut app, code, KeyModifiers::NONE);
            t.draw(|f| render_with(f, &mut app, &theme())).unwrap();
            times.push(s.elapsed());
        }
        let (median, worst) = median_and_worst(times);
        if name == "big.json" {
            assert_eq!(
                app.syntax.stats(0).diffs,
                0,
                "{label}: disabled, never diffed"
            );
        }
        // The edit points of one keystroke at `mid`, scanned vs from the line index.
        let rope = app.docs[0].buffer.text().clone();
        let point = |breaks| {
            let times: Vec<Duration> = (0..21)
                .map(|_| {
                    let s = Instant::now();
                    std::hint::black_box(Edit::from_chars_in(&rope, mid, mid, "x", breaks));
                    s.elapsed()
                })
                .collect();
            median_and_worst(times).0
        };
        let (scan, indexed) = (point(LineBreaks::Unknown), point(LineBreaks::NoLoneCr));
        eprintln!(
            "highlighted_keystroke_to_frame_fast {label}: keystroke+frame median {median:?} \
             worst {worst:?}; edit points scan {scan:?} vs no-lone-CR index {indexed:?}"
        );
        assert!(median < Duration::from_millis(5), "{label}: {median:?}");
    }
}
