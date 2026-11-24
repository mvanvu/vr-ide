//! Find / replace bar through the `App` (AC-10).

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::app::{App, Focus};
use crate::app_tests::app_with_file;
use crate::commands::find::{Field, count_text};
use crate::theme::Theme;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn alt(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT)
}

fn shift(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::SHIFT)
}

fn typ(app: &mut App, s: &str) {
    for c in s.chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
}

fn text(app: &App) -> String {
    app.active_doc().unwrap().buffer.text().to_string()
}

fn selection(app: &App) -> (usize, usize) {
    let r = app.active_doc().unwrap().buffer.selection().primary();
    (r.from(), r.to())
}

fn draw(app: &mut App, w: u16, h: u16) -> Terminal<TestBackend> {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| crate::ui::render(f, app)).unwrap();
    t
}

fn row_text(t: &Terminal<TestBackend>, y: u16) -> String {
    let buf = t.backend().buffer();
    (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
}

#[test]
fn find_bar_highlights_matches_and_counts() {
    let (_d, _p, mut app, _) = app_with_file(&"foo bar\n".repeat(12));
    app.handle_key(ctrl('f'));
    assert!(app.find.open);
    assert_eq!(app.focus, Focus::FindBar);
    typ(&mut app, "foo");
    assert_eq!(app.find.matches.len(), 12);
    // Typing selects the first match from the cursor.
    assert_eq!(count_text(&app), "1/12");
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(count_text(&app), "3/12");
    assert_eq!(selection(&app), app.find.matches[2]);
    // The file is not touched by typing in the bar.
    assert_eq!(text(&app), "foo bar\n".repeat(12));

    let mut t = draw(&mut app, 80, 24);
    let bar = app.layout.find_bar;
    assert_eq!(bar.height, 1);
    assert!(
        row_text(&t, bar.y).contains("3/12"),
        "{}",
        row_text(&t, bar.y)
    );
    let theme = Theme::dark();
    let ed = app.layout.editor;
    let gutter = crate::ui::editor_view::gutter_width(app.active_doc().unwrap().buffer.text());
    {
        let buf = t.backend().buffer();
        let at = |line: u16, col: u16| buf[(ed.x + gutter + col, ed.y + line)].bg;
        assert_eq!(at(0, 0), theme.find_match.bg.unwrap(), "other match");
        assert_eq!(at(0, 2), theme.find_match.bg.unwrap(), "other match");
        assert_ne!(at(0, 4), theme.find_match.bg.unwrap(), "not a match");
        assert_eq!(at(2, 0), theme.selection.bg.unwrap(), "current match");
    }
    // The cursor is in the bar's input.
    let pos = t.get_cursor_position().unwrap();
    assert_eq!(pos.y, bar.y);

    // Esc closes the bar: no highlights, keys go to the editor again.
    app.handle_key(key(KeyCode::Esc));
    assert!(!app.find.open);
    assert_eq!(app.focus, Focus::Editor);
    let t = draw(&mut app, 80, 24);
    assert_eq!(app.layout.find_bar.height, 0);
    let ed = app.layout.editor;
    assert_ne!(
        t.backend().buffer()[(ed.x + gutter, ed.y)].bg,
        theme.find_match.bg.unwrap()
    );
}

#[test]
fn find_next_prev_wraps() {
    let (_d, _p, mut app, _) = app_with_file("x foo\nfoo\nfoo y\n");
    app.handle_key(ctrl('f'));
    typ(&mut app, "foo");
    assert_eq!(count_text(&app), "1/3");
    // In the bar: Enter next, shift+Enter previous, both wrap.
    app.handle_key(shift(KeyCode::Enter));
    assert_eq!(count_text(&app), "3/3");
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(count_text(&app), "1/3");
    // From the editor with the bar closed: f3 / shift+f3 keep the last query.
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.focus, Focus::Editor);
    app.handle_key(key(KeyCode::F(3)));
    assert_eq!(count_text(&app), "2/3");
    app.handle_key(key(KeyCode::F(3)));
    app.handle_key(key(KeyCode::F(3)));
    assert_eq!(count_text(&app), "1/3");
    app.handle_key(shift(KeyCode::F(3)));
    assert_eq!(count_text(&app), "3/3");
    assert_eq!(selection(&app), (10, 13));
    // A cursor between matches: next is the first one after it, the count says "?".
    app.active_doc_mut()
        .unwrap()
        .buffer
        .set_selection(vr_core::Selection::point(7));
    assert_eq!(count_text(&app), "?/3");
    app.handle_key(key(KeyCode::F(3)));
    assert_eq!(selection(&app), (10, 13));
}

#[test]
fn find_toggles_regex_case_word() {
    let (_d, _p, mut app, _) = app_with_file("Foo foo food f.o\n");
    app.handle_key(ctrl('f'));
    typ(&mut app, "foo");
    assert_eq!(app.find.matches.len(), 3, "case-insensitive by default");
    app.handle_key(alt('c'));
    assert!(app.find.case_sensitive);
    assert_eq!(app.find.matches, vec![(4, 7), (8, 11)]);
    app.handle_key(alt('w'));
    assert!(app.find.whole_word);
    assert_eq!(app.find.matches, vec![(4, 7)]);
    app.handle_key(alt('c'));
    assert_eq!(app.find.matches, vec![(0, 3), (4, 7)]);
    app.handle_key(alt('w'));
    assert_eq!(app.find.matches.len(), 3);
    for _ in 0..3 {
        app.handle_key(key(KeyCode::Backspace));
    }
    typ(&mut app, "f.o");
    assert_eq!(app.find.matches, vec![(13, 16)], "literal by default");
    app.handle_key(alt('r'));
    assert!(app.find.regex);
    assert_eq!(app.find.matches.len(), 4);
    typ(&mut app, "(");
    assert!(app.find.matches.is_empty());
    assert!(app.find.error.is_some());
    assert!(count_text(&app).contains("regex"), "{}", count_text(&app));
    // The toggles are drawn in the bar.
    let t = draw(&mut app, 80, 24);
    let row = row_text(&t, app.layout.find_bar.y);
    assert!(row.contains("Aa") && row.contains(".*"), "{row}");
}

#[test]
fn replace_one_and_all_single_undo() {
    const ORIGINAL: &str = "fn alpha() {}\nfn beta() {}\nfn gamma() {}\n";
    const ALL: &str = "fn new_alpha() {}\nfn new_beta() {}\nfn new_gamma() {}\n";
    let (_d, _p, mut app, _) = app_with_file(ORIGINAL);
    app.handle_key(ctrl('h'));
    assert!(app.find.open && app.find.replace_mode);
    assert_eq!(app.find.field, Field::Find);
    app.handle_key(alt('r'));
    typ(&mut app, r"fn (\w+)");
    assert_eq!(app.find.matches.len(), 3);
    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.find.field, Field::Replace);
    typ(&mut app, "fn new_$1");
    // Enter in the replace field replaces the current match and moves to the next one.
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(
        text(&app),
        "fn new_alpha() {}\nfn beta() {}\nfn gamma() {}\n"
    );
    // "fn new_alpha" matches the regex too: the cursor moved on to "fn beta", the second.
    assert_eq!(count_text(&app), "2/3");
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(ctrl('z'));
    assert_eq!(text(&app), ORIGINAL, "replace one is one undo step");
    assert!(!app.active_doc().unwrap().buffer.history().can_undo());

    // AC-10: replace all with the regex is one undo step.
    app.handle_key(ctrl('h'));
    assert_eq!(app.find.field, Field::Replace);
    app.handle_key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ));
    assert_eq!(text(&app), ALL);
    assert!(app.notices.iter().any(|n| n.text.contains('3')));
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(ctrl('z'));
    assert_eq!(text(&app), ORIGINAL, "replace all is one undo step");
    assert!(!app.active_doc().unwrap().buffer.history().can_undo());
    // Redo brings the whole replacement back.
    app.handle_key(ctrl('y'));
    assert_eq!(text(&app), ALL);
}

#[test]
fn find_bar_keeps_edit_keys_off_the_file() {
    let (_d, _p, mut app, clip) = app_with_file("one two\n");
    let mut clip = clip;
    crate::clipboard::Clipboard::set(&mut clip, "two").unwrap();
    app.handle_key(ctrl('f'));
    app.handle_key(ctrl('v'));
    assert_eq!(app.find.input.text(), "two");
    assert_eq!(app.find.matches, vec![(4, 7)]);
    app.handle_paste(" more");
    assert_eq!(app.find.input.text(), "two more");
    // Undo, cut and select-all would act on the file behind the bar: they do nothing.
    for c in ['z', 'x', 'a'] {
        app.handle_key(ctrl(c));
    }
    assert_eq!(text(&app), "one two\n");
    assert!(!app.active_doc().unwrap().is_modified());
}

#[test]
fn replace_in_read_only_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("l1.txt"), b"caf\xe9 caf\xe9\n").unwrap();
    let (mut app, _) = crate::app_tests::test_app(dir.path());
    app.open_path(std::path::Path::new("l1.txt")).unwrap();
    app.notices.clear();
    app.handle_key(ctrl('h'));
    typ(&mut app, "caf");
    assert_eq!(
        app.find.matches.len(),
        2,
        "finding works in a read-only file"
    );
    app.handle_key(key(KeyCode::Tab));
    typ(&mut app, "X");
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    ));
    assert!(!app.active_doc().unwrap().is_modified());
    assert!(app.notices.iter().any(|n| n.text.contains("read-only")));
}
