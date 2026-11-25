//! The find box through the `App` (AC-10, T22 AC-22a..e): File scope (in-file find /
//! replace and its match list), the scope switch, Project scope from the box, the query
//! syntax, f4 / shift+f4 and Esc back to read mode.

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::app::{App, Focus};
use crate::app_tests::{app_with_file, legacy_event, test_app};
use crate::commands::find::{Field, Scope, count_text};
use crate::commands::search::Row;
use crate::theme::Theme;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn shift(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::SHIFT)
}

fn typ(app: &mut App, s: &str) {
    for c in s.chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
}

fn erase(app: &mut App) {
    for _ in 0..64 {
        app.handle_key(key(KeyCode::Backspace));
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

/// The rows of the box's list in the last frame.
fn list_rows(app: &App, t: &Terminal<TestBackend>) -> Vec<String> {
    let l = if app.find.scope == Scope::Project {
        app.search.list_area
    } else {
        app.find.list_area
    };
    (l.y..l.y + l.height)
        .map(|y| {
            row_text(t, y)[usize::from(l.x)..usize::from(l.x + l.width)]
                .trim_end()
                .to_string()
        })
        .collect()
}

/// Every row of the find box in the last frame (fields, notes and list).
fn box_rows(app: &App, t: &Terminal<TestBackend>) -> Vec<String> {
    let b = app.layout.find_bar;
    (b.y..b.y + b.height)
        .map(|y| {
            row_text(t, y)[usize::from(b.x)..usize::from(b.x + b.width)]
                .trim_end()
                .to_string()
        })
        .collect()
}

/// Runs the project search to its end (the worker streams into the app).
fn finish_search(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while app.search.running() || app.search.busy() {
        assert!(Instant::now() < deadline, "search did not finish");
        std::thread::sleep(Duration::from_millis(2));
        app.drain_search();
    }
    app.search.groups.sort_by(|a, b| a.path.cmp(&b.path));
}

/// AC-22a: ctrl+f in a file lists every matching line; the list follows the editor's
/// selection; Enter jumps to the match in read mode; Esc closes the box in read mode.
#[test]
fn file_scope_lists_every_match_enter_jumps_esc_reads() {
    let body = "fn foo() {}\nlet x = 1;\n    foo(); foo();\nbar\n";
    let (_d, _p, mut app, _) = app_with_file(body);
    app.handle_key(ctrl('f'));
    assert!(app.find.open);
    assert_eq!(app.find.scope, Scope::File);
    assert_eq!(app.focus, Focus::FindBar);
    typ(&mut app, "foo");
    assert_eq!(app.find.matches.len(), 3);
    // Typing selects the first match from the cursor.
    assert_eq!(count_text(&app), "1/3");
    let t = draw(&mut app, 80, 24);
    let rows = list_rows(&app, &t);
    assert_eq!(rows[0], "1  fn foo() {}");
    // Indentation is dropped; a line with two matches is listed for each of them.
    assert_eq!(rows[1], "3  foo(); foo();");
    assert_eq!(rows[2], "3  foo(); foo();");
    assert!(rows[3..].iter().all(String::is_empty), "{rows:?}");
    // The match is highlighted in its row, the selected row is the current match.
    let theme = Theme::from_name(&app.settings.theme);
    let l = app.find.list_area;
    let buf = t.backend().buffer();
    assert_eq!(
        buf[(l.x + 6, l.y)].fg,
        theme.matched.fg.unwrap(),
        "f of foo"
    );
    assert_eq!(buf[(l.x, l.y)].bg, theme.list_selected.bg.unwrap());
    assert_ne!(buf[(l.x, l.y + 1)].bg, theme.list_selected.bg.unwrap());

    // Up/Down move in the list; the editor shows the match; the keys stay in the box.
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Down));
    assert_eq!(count_text(&app), "3/3");
    assert_eq!(selection(&app), app.find.matches[2]);
    app.handle_key(key(KeyCode::Down));
    assert_eq!(count_text(&app), "3/3", "the list does not wrap");
    app.handle_key(key(KeyCode::Up));
    assert_eq!(count_text(&app), "2/3");
    assert_eq!(app.focus, Focus::FindBar);
    assert_eq!(text(&app), body, "the file is not touched");

    // Enter: the editor gets the keys in read mode on the match; the box stays open.
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.focus, Focus::Editor);
    assert!(!app.edit_mode());
    assert_eq!(selection(&app), app.find.matches[1]);
    assert!(app.find.open, "matches stay highlighted");
    let t = draw(&mut app, 80, 24);
    let ed = app.layout.editor;
    let gutter = crate::ui::editor_view::gutter_width(app.active_doc().unwrap().buffer.text());
    let row0 = ed.y; // the box is above: the editor shows line 1 first
    assert_eq!(
        t.backend().buffer()[(ed.x + gutter + 3, row0)].bg,
        theme.find_match.bg.unwrap(),
        "other matches stay highlighted"
    );

    // Esc in the box: closed, read mode, the selection on the match is kept.
    app.handle_key(ctrl('f'));
    assert_eq!(app.focus, Focus::FindBar);
    assert_eq!(
        app.find.scope,
        Scope::File,
        "from the editor ctrl+f is File"
    );
    app.handle_key(key(KeyCode::Esc));
    assert!(!app.find.open);
    assert_eq!(app.focus, Focus::Editor);
    assert!(!app.edit_mode());
    let t = draw(&mut app, 80, 24);
    assert_eq!(app.layout.find_bar.height, 0);
    let ed = app.layout.editor;
    assert_ne!(
        t.backend().buffer()[(ed.x + gutter + 3, ed.y)].bg,
        theme.find_match.bg.unwrap()
    );
}

/// The box takes its fields and a list of two fifths of the editor, keeps the editor
/// visible, and works at 40x12.
#[test]
fn box_leaves_the_editor_visible_at_each_size() {
    let (_d, _p, mut app, _) = app_with_file(&"foo\n".repeat(50));
    app.handle_key(ctrl('f'));
    typ(&mut app, "foo");
    for (w, h, list) in [(80, 24, 8), (120, 40, 14), (40, 12, 3)] {
        draw(&mut app, w, h);
        let b = app.layout.find_bar;
        assert_eq!(b.height, 1 + list, "{w}x{h}");
        assert_eq!(app.find.list_area.height, list, "{w}x{h}");
        assert!(
            app.layout.editor.height >= 5,
            "{w}x{h}: {:?}",
            app.layout.editor
        );
        assert_eq!(app.layout.editor.width, w, "{w}x{h}");
    }
    // Less to list: the list shrinks to it (3 rows at least), the editor gets the rest.
    typ(&mut app, "zz");
    draw(&mut app, 120, 40);
    assert_eq!(app.find.list_area.height, 3);
}

/// AC-22c: `/re/` is a regex, `…/i` ignores case, `\/` is a slash, a bad regex is an error
/// in the box and nothing is found.
#[test]
fn query_syntax_regex_ignore_case_slash_and_bad_regex() {
    let (_d, _p, mut app, _) = app_with_file("Foo foo fooo f.o a/b\n");
    app.handle_key(ctrl('f'));
    let cases: [(&str, Vec<(usize, usize)>); 7] = [
        ("foo", vec![(4, 7), (8, 11)]),
        ("foo/i", vec![(0, 3), (4, 7), (8, 11)]),
        ("/fo+/", vec![(4, 7), (8, 12)]),
        ("/fo+/i", vec![(0, 3), (4, 7), (8, 12)]),
        ("f.o", vec![(13, 16)]),
        (r"a\/b", vec![(17, 20)]),
        (r"/a\/b/", vec![(17, 20)]),
    ];
    for (q, want) in cases {
        erase(&mut app);
        typ(&mut app, q);
        assert_eq!(app.find.matches, want, "{q}");
        assert!(app.find.error.is_none(), "{q}");
    }
    erase(&mut app);
    typ(&mut app, "/fo(/");
    assert!(app.find.matches.is_empty());
    assert!(app.find.error.is_some());
    assert_eq!(count_text(&app), "invalid regex");
    // The whole error is shown in the box, under the fields.
    let t = draw(&mut app, 80, 24);
    let rows = box_rows(&app, &t);
    assert!(rows[0].ends_with("invalid regex"), "{rows:?}");
    assert!(
        rows[1].starts_with("invalid regex: regex parse error"),
        "{rows:?}"
    );
    // Enter does not jump anywhere.
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.focus, Focus::FindBar);
    // The old toggles are gone: the syntax replaces them.
    for cmd in ["find.toggle_case", "find.toggle_word", "find.toggle_regex"] {
        assert!(crate::commands::find(cmd).is_none(), "{cmd}");
    }
}

/// An empty box tells how to search and which key switches the scope.
#[test]
fn empty_box_shows_the_syntax_and_the_scope_key() {
    let (_d, _p, mut app, _) = app_with_file("x\n");
    app.handle_key(ctrl('f'));
    let t = draw(&mut app, 80, 24);
    let rows = box_rows(&app, &t).join(" ");
    assert!(rows.contains("ctrl+f: search the project"), "{rows}");
    assert!(
        rows.contains("/re/ regex") && rows.contains("/i ignore case"),
        "{rows}"
    );
    assert!(row_text(&t, app.layout.find_bar.y).starts_with(" File "));
    // In the box: "search the file" while a file is open.
    app.handle_key(ctrl('f'));
    let t = draw(&mut app, 80, 24);
    let rows = box_rows(&app, &t).join(" ");
    assert!(rows.contains("ctrl+f: search the file"), "{rows}");

    // No file open: the box is on the project and cannot switch, so no scope key.
    let (_d, mut app) = project_app();
    app.close_doc(0);
    app.handle_key(ctrl('f'));
    let t = draw(&mut app, 80, 24);
    let rows = box_rows(&app, &t).join(" ");
    assert!(!rows.contains("search the file"), "{rows}");
    assert!(rows.contains("/re/ regex"), "{rows}");
}

/// A selection seeds the box literally: slashes are typed as `\/`.
#[test]
fn selection_seeds_a_literal_query() {
    let (_d, _p, mut app, _) = app_with_file("a/i x a/i\n");
    app.active_doc_mut()
        .unwrap()
        .buffer
        .set_selection(vr_core::Selection::single(0, 3));
    app.handle_key(ctrl('f'));
    assert_eq!(app.find.input.text(), r"a\/i");
    assert_eq!(app.find.matches, vec![(0, 3), (6, 9)]);
}

#[test]
fn f4_and_shift_f4_step_and_wrap() {
    let (_d, _p, mut app, _) = app_with_file("x foo\nfoo\nfoo y\n");
    app.handle_key(ctrl('f'));
    typ(&mut app, "foo");
    assert_eq!(count_text(&app), "1/3");
    // In the box: shift+f4 previous, f4 next, both wrap.
    app.handle_key(shift(KeyCode::F(4)));
    assert_eq!(count_text(&app), "3/3");
    app.handle_key(key(KeyCode::F(4)));
    assert_eq!(count_text(&app), "1/3");
    assert_eq!(app.focus, Focus::FindBar);
    // shift+Enter in the find field: the previous match, the box keeps the keys (D116, D275).
    app.handle_key(shift(KeyCode::Enter));
    assert_eq!(count_text(&app), "3/3");
    assert_eq!(app.focus, Focus::FindBar);
    app.handle_key(shift(KeyCode::Enter));
    assert_eq!(count_text(&app), "2/3");
    app.handle_key(key(KeyCode::F(4)));
    assert_eq!(count_text(&app), "3/3");
    app.handle_key(key(KeyCode::F(4)));
    assert_eq!(count_text(&app), "1/3");
    // From the editor (box open or closed) f4 / shift+f4 keep the last query.
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.focus, Focus::Editor);
    app.handle_key(key(KeyCode::F(4)));
    assert_eq!(count_text(&app), "2/3");
    app.handle_key(ctrl('f'));
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(key(KeyCode::F(4)));
    app.handle_key(key(KeyCode::F(4)));
    assert_eq!(count_text(&app), "1/3");
    app.handle_key(shift(KeyCode::F(4)));
    assert_eq!(count_text(&app), "3/3");
    assert_eq!(selection(&app), (10, 13));
    // A cursor between matches: next is the first one after it, the count says "?".
    app.active_doc_mut()
        .unwrap()
        .buffer
        .set_selection(vr_core::Selection::point(7));
    assert_eq!(count_text(&app), "?/3");
    app.handle_key(key(KeyCode::F(4)));
    assert_eq!(selection(&app), (10, 13));
}

/// The Replace dialog (D279): (title, message, buttons, default).
fn replace_dialog(app: &App) -> Option<(String, String, Vec<String>, usize)> {
    match &app.dialog.as_ref()?.widget {
        crate::ui::dialog::DialogWidget::Confirm(c) => Some((
            c.title.clone(),
            c.message.clone(),
            c.buttons.clone(),
            c.selected,
        )),
        _ => None,
    }
}

/// AC-22d (File scope): ctrl+r opens the replace row in edit mode; Enter in the replace field
/// asks Replace: One / All / Cancel (D279, One first), each replacement one undo step; Esc
/// returns to read mode.
#[test]
fn replace_one_and_all_single_undo() {
    const ORIGINAL: &str = "fn alpha() {}\nfn beta() {}\nfn gamma() {}\n";
    const ALL: &str = "fn new_alpha() {}\nfn new_beta() {}\nfn new_gamma() {}\n";
    let (_d, _p, mut app, _) = app_with_file(ORIGINAL);
    app.handle_key(ctrl('r'));
    assert!(app.find.open && app.find.replace_mode);
    assert!(app.edit_mode());
    assert_eq!(app.find.field, Field::Find);
    typ(&mut app, r"/fn (\w+)/");
    assert_eq!(app.find.matches.len(), 3);
    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.find.field, Field::Replace);
    typ(&mut app, "fn new_$1");
    // ctrl+a no longer replaces anything (it would select the file behind the box: nothing).
    app.handle_key(ctrl('a'));
    assert_eq!(text(&app), ORIGINAL);
    assert!(app.dialog.is_none());
    // Enter in the replace field asks; Esc and Cancel replace nothing.
    app.handle_key(key(KeyCode::Enter));
    let (title, message, buttons, selected) = replace_dialog(&app).expect("the Replace dialog");
    assert_eq!(title, "Replace");
    assert_eq!(buttons, ["One", "All", "Cancel"]);
    assert_eq!(buttons[selected], "One", "One is the default");
    assert!(message.contains("3 matches"), "{message}");
    assert!(message.contains("fn new_$1"), "{message}");
    app.handle_key(key(KeyCode::Esc));
    assert!(app.dialog.is_none());
    assert_eq!(text(&app), ORIGINAL, "Esc replaces nothing");
    assert_eq!(app.focus, Focus::FindBar, "the box keeps the keys");
    assert_eq!(app.find.field, Field::Replace);
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Left));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(text(&app), ORIGINAL, "Cancel replaces nothing");
    // One: the current match is replaced and the next one selected.
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Enter));
    assert!(app.dialog.is_none());
    assert_eq!(app.focus, Focus::FindBar);
    assert_eq!(
        text(&app),
        "fn new_alpha() {}\nfn beta() {}\nfn gamma() {}\n"
    );
    // "fn new_alpha" matches the regex too: the cursor moved on to "fn beta", the second.
    assert_eq!(count_text(&app), "2/3");
    app.handle_key(key(KeyCode::Esc));
    assert!(!app.edit_mode(), "Esc returns to read mode");
    crate::app_tests::space_i(&mut app);
    app.handle_key(ctrl('z'));
    assert_eq!(text(&app), ORIGINAL, "replace one is one undo step");
    assert!(!app.active_doc().unwrap().buffer.history().can_undo());

    // AC-10: replace all with the regex is one undo step.
    app.handle_key(ctrl('r'));
    assert_eq!(app.find.field, Field::Replace);
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Right));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(text(&app), ALL);
    assert!(app.notices.iter().any(|n| n.text.contains('3')));
    app.handle_key(key(KeyCode::Esc));
    crate::app_tests::space_i(&mut app);
    app.handle_key(ctrl('z'));
    assert_eq!(text(&app), ORIGINAL, "replace all is one undo step");
    assert!(!app.active_doc().unwrap().buffer.history().can_undo());
    // Redo brings the whole replacement back.
    app.handle_key(ctrl('y'));
    assert_eq!(text(&app), ALL);
}

/// D279: with no match the replace field's Enter says so instead of asking; the box keeps the
/// keys.
#[test]
fn replace_enter_without_matches_does_not_ask() {
    let (_d, _p, mut app, _) = app_with_file("one two\n");
    app.handle_key(ctrl('r'));
    typ(&mut app, "zzz");
    app.handle_key(key(KeyCode::Tab));
    typ(&mut app, "y");
    app.handle_key(key(KeyCode::Enter));
    assert!(app.dialog.is_none());
    assert!(
        app.notices.iter().any(|n| n.text.contains("no results")),
        "{:?}",
        app.notices
    );
    assert_eq!(app.focus, Focus::FindBar);
    assert_eq!(text(&app), "one two\n");
}

/// Snapshot of the Replace dialog over the find box (D279), 80x24.
#[test]
fn render_replace_dialog_80x24() {
    let (_d, _p, mut app, _) = app_with_file("fn alpha() {}\nfn beta() {}\n");
    app.handle_key(ctrl('r'));
    typ(&mut app, "fn");
    app.handle_key(key(KeyCode::Tab));
    typ(&mut app, "pub fn");
    app.handle_key(key(KeyCode::Enter));
    let mut t = Terminal::new(TestBackend::new(80, 24)).unwrap();
    t.draw(|f| crate::ui::render(f, &mut app)).unwrap();
    insta::assert_snapshot!("render_replace_dialog_80x24", t.backend());
}

#[test]
fn find_box_keeps_edit_keys_off_the_file() {
    let (_d, _p, mut app, clip) = app_with_file("one two\n");
    let mut clip = clip;
    crate::clipboard::Clipboard::set(&mut clip, "two").unwrap();
    app.handle_key(ctrl('f'));
    app.handle_key(ctrl('v'));
    assert_eq!(app.find.input.text(), "two");
    assert_eq!(app.find.matches, vec![(4, 7)]);
    app.handle_paste(" more");
    assert_eq!(app.find.input.text(), "two more");
    // Undo, cut and select-all would act on the file behind the box: they do nothing.
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
    let (mut app, _) = test_app(dir.path());
    app.open_path(std::path::Path::new("l1.txt")).unwrap();
    app.notices.clear();
    crate::app_tests::run(&mut app, "find.replace_open");
    assert!(!app.find.open, "a read-only file gets no replace row");
    assert!(app.notices.iter().any(|n| n.text.contains("read-only")));
    app.notices.clear();
    app.handle_key(ctrl('f'));
    typ(&mut app, "caf");
    assert_eq!(
        app.find.matches.len(),
        2,
        "finding works in a read-only file"
    );
    crate::app_tests::run(&mut app, "find.replace_all");
    assert!(!app.active_doc().unwrap().is_modified());
    assert!(app.notices.iter().any(|n| n.text.contains("read-only")));
}

/// D124: the box collects at most 20 000 matches and says there are more.
#[test]
fn find_caps_matches_at_20000() {
    let (_d, _p, mut app, _) = app_with_file(&"e\n".repeat(30_000));
    app.handle_key(ctrl('f'));
    typ(&mut app, "e");
    assert_eq!(app.find.matches.len(), 20_000);
    assert_eq!(count_text(&app), "1/20000+");
    app.handle_key(shift(KeyCode::F(4)));
    assert_eq!(
        count_text(&app),
        "20000/20000+",
        "previous wraps to the last known"
    );
    // The list shows the rows around the selection only.
    let t = draw(&mut app, 80, 24);
    let rows = list_rows(&app, &t);
    assert_eq!(rows.last().unwrap(), "20000  e");
    app.handle_key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE));
    assert_eq!(count_text(&app), "19992/20000+");
}

/// D124: in a large file the box rescans once typing pauses, not on every key in the field
/// or in the editor; the first match from the cursor is selected when the scan runs.
#[test]
fn find_debounces_rescans_in_a_large_file() {
    let ms = Duration::from_millis;
    let (_d, _p, mut app, _) = app_with_file(&"ee\n".repeat(400_000));
    let t0 = Instant::now();
    app.handle_key_at(ctrl('f'), t0);
    app.handle_key_at(key(KeyCode::Char('e')), t0);
    assert!(app.find.matches.is_empty(), "no rescan per keystroke");
    draw(&mut app, 80, 24);
    assert!(
        app.find.matches.is_empty(),
        "drawing does not rescan either"
    );
    app.handle_key_at(key(KeyCode::Char('e')), t0 + ms(100));
    app.tick(t0 + ms(200));
    assert!(
        app.find.matches.is_empty(),
        "the pause restarts with each key"
    );
    app.tick(t0 + ms(400));
    assert_eq!(app.find.matches.len(), 20_000);
    assert_eq!(count_text(&app), "1/20000+");
    assert_eq!(selection(&app), (0, 2));

    // Typing in the editor: the old matches stay until the pause.
    app.focus = Focus::Editor;
    // `draw` is not a key event, so the refresh it schedules reads the real clock (as in
    // production); the ticks below are anchored to real instants taken around the draw so
    // the outcome does not depend on how long the test took.
    app.handle_key_at(key(KeyCode::Char('x')), t0 + ms(1000));
    let before_draw = Instant::now();
    draw(&mut app, 80, 24);
    let after_draw = Instant::now();
    assert_eq!(app.find.matches[0], (0, 2), "no rescan per editor key");
    app.tick(before_draw + ms(100));
    assert_eq!(app.find.matches[0], (0, 2), "the pause has not elapsed yet");
    app.tick(after_draw + ms(300));
    assert_eq!(app.find.matches[0], (2, 4));
}

/// A project of two roots-worth of files for the Project scope tests: `src/a.rs` and
/// `b.txt` hold "needle", the open file `src/a.rs` once.
fn project_app() -> (tempfile::TempDir, App) {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("shop");
    std::fs::create_dir_all(r.join("src")).unwrap();
    std::fs::write(r.join("src/a.rs"), "fn a() {}\n// needle here\n").unwrap();
    std::fs::write(r.join("b.txt"), "x\nNeedle one\nneedle two\n").unwrap();
    let (mut app, _) = test_app(&r);
    app.open_path(std::path::Path::new("src/a.rs")).unwrap();
    app.notices.clear();
    (d, app)
}

/// AC-22b: ctrl+f in the box switches to Project scope (text kept, searched at once);
/// results are grouped by file; Enter opens the file at the match in read mode; ctrl+f
/// switches back to File scope.
#[test]
fn ctrl_f_switches_scope_project_results_open_at_the_match() {
    let (_d, mut app) = project_app();
    app.handle_key(ctrl('f'));
    typ(&mut app, "needle/i");
    assert_eq!(app.find.matches.len(), 1, "File scope: the open file");
    app.handle_key(ctrl('f'));
    assert_eq!(app.find.scope, Scope::Project);
    assert_eq!(app.focus, Focus::Search);
    assert!(app.search.visible);
    assert_eq!(app.search.input.text(), "needle/i");
    finish_search(&mut app);
    assert_eq!(app.search.total, 3, "case ignored: Needle counts");
    let t = draw(&mut app, 80, 24);
    let head = row_text(&t, app.layout.find_bar.y);
    assert!(head.starts_with(" Project "), "{head}");
    assert!(head.trim_end().ends_with("3 results in 2 files"), "{head}");
    let rows = list_rows(&app, &t);
    assert!(rows[0].starts_with("b.txt"), "{rows:?}");
    assert!(rows[1].contains("2: Needle one"), "{rows:?}");
    assert!(rows[2].contains("3: needle two"), "{rows:?}");
    assert!(rows[3].starts_with("a.rs src"), "{rows:?}");
    // The open file keeps its highlights while the project is searched.
    assert_eq!(app.find.matches.len(), 1);

    // Down into the list, Enter on "needle two": b.txt opens there, read mode.
    for _ in 0..3 {
        app.handle_key(key(KeyCode::Down));
    }
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.focus, Focus::Editor);
    assert!(!app.edit_mode());
    let doc = app.active_doc().unwrap();
    assert!(doc.path.as_ref().unwrap().ends_with("b.txt"));
    assert_eq!(selection(&app), (13, 19), "line 3, the match");
    assert!(app.find.project(), "the box stays on the project");

    // f4 / shift+f4 from the editor step through the project results.
    app.handle_key(key(KeyCode::F(4)));
    assert!(
        app.active_doc()
            .unwrap()
            .path
            .as_ref()
            .unwrap()
            .ends_with("src/a.rs")
    );
    app.handle_key(shift(KeyCode::F(4)));
    assert_eq!(selection(&app), (13, 19));

    // ctrl+f from the editor: File scope on the open file; in the box again: Project.
    app.handle_key(ctrl('f'));
    assert_eq!(app.find.scope, Scope::File);
    assert_eq!(app.focus, Focus::FindBar);
    assert!(!app.search.visible);
    app.handle_key(ctrl('f'));
    assert_eq!(app.find.scope, Scope::Project);
    assert!(
        app.search.running() || app.search.total == 3,
        "same query: kept"
    );
    app.handle_key(ctrl('f'));
    assert_eq!(app.find.scope, Scope::File);
    assert_eq!(app.find.input.text(), "needle/i");
    // Esc from either scope: closed, read mode.
    app.handle_key(ctrl('f'));
    app.handle_key(key(KeyCode::Esc));
    assert!(!app.find.open && !app.search.visible);
    assert_eq!(app.focus, Focus::Editor);
    assert!(!app.edit_mode());
}

/// The scope switch also works with the bytes a terminal without the kitty protocol sends
/// for ctrl+f (0x06), f4 and Esc.
#[test]
fn legacy_ctrl_f_switches_scope() {
    let (_d, mut app) = project_app();
    let (bytes, ev) = legacy_event("ctrl+f");
    assert_eq!(bytes, vec![0x06]);
    let ev = ev.expect("ctrl+f reaches vr-ide");
    app.handle_key(ev);
    assert_eq!(app.find.scope, Scope::File);
    app.handle_key(ev);
    assert_eq!(app.find.scope, Scope::Project);
    app.handle_key(ev);
    assert_eq!(app.find.scope, Scope::File);
    let (_, esc) = legacy_event("escape");
    app.handle_key(esc.unwrap());
    assert!(!app.find.open);
}

/// With no file open ctrl+f searches the project (there is no file); an invalid regex is
/// shown in the box and nothing is searched.
#[test]
fn no_file_opens_project_scope_bad_regex_does_not_search() {
    let (_d, mut app) = project_app();
    app.close_doc(0);
    app.handle_key(ctrl('f'));
    assert_eq!(app.find.scope, Scope::Project);
    assert_eq!(app.focus, Focus::Search);
    typ(&mut app, "/ne(/");
    app.handle_key(key(KeyCode::Enter));
    assert!(!app.search.running(), "no search for a bad regex");
    assert!(
        app.search.errors[0].contains("invalid regex"),
        "{:?}",
        app.search.errors
    );
    let t = draw(&mut app, 80, 24);
    let rows = box_rows(&app, &t);
    assert!(rows[1].starts_with("invalid regex"), "{rows:?}");
    // ctrl+f has no file to switch to: it says so and stays.
    app.handle_key(ctrl('f'));
    assert_eq!(app.find.scope, Scope::Project);
    // A valid regex searches on Enter.
    erase(&mut app);
    typ(&mut app, "/ne+dle/");
    app.handle_key(key(KeyCode::Enter));
    finish_search(&mut app);
    assert_eq!(app.search.total, 2, "case-sensitive regex");
}

/// AC-22d (Project scope): ctrl+r in the project box opens its replace row; Enter there
/// shows the existing preview + confirm flow, which writes the files.
#[test]
fn ctrl_r_in_project_scope_previews_and_replaces() {
    let (d, mut app) = project_app();
    app.close_doc(0);
    app.handle_key(ctrl('f'));
    typ(&mut app, "needle");
    app.handle_key(ctrl('r'));
    assert!(app.search.replace_mode && app.find.project());
    assert_eq!(app.search.field, crate::commands::search::Field::Replace);
    typ(&mut app, "pin");
    app.handle_key(key(KeyCode::Enter));
    finish_search(&mut app);
    let p = app.search.preview.as_ref().expect("preview");
    assert_eq!(p.files.len(), 2);
    let t = draw(&mut app, 80, 24);
    let rows = list_rows(&app, &t).join("\n");
    assert!(
        rows.contains("[x]") && rows.contains("+3 pin two"),
        "{rows}"
    );
    // Confirm: the list, Enter, the dialog's Replace.
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));
    assert!(app.dialog.is_some(), "Replace / Cancel");
    app.handle_key(key(KeyCode::Left));
    app.handle_key(key(KeyCode::Enter));
    finish_search(&mut app);
    let root = d.path().join("shop");
    assert_eq!(
        std::fs::read_to_string(root.join("b.txt")).unwrap(),
        "x\nNeedle one\npin two\n"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("src/a.rs")).unwrap(),
        "fn a() {}\n// pin here\n"
    );
}

/// ctrl+r from the editor keeps the box's scope: on the project it is the project replace.
#[test]
fn ctrl_r_from_the_editor_keeps_the_project_scope() {
    let (_d, mut app) = project_app();
    app.handle_key(ctrl('f'));
    typ(&mut app, "needle");
    app.handle_key(ctrl('f'));
    finish_search(&mut app);
    app.focus = Focus::Editor;
    app.handle_key(ctrl('r'));
    assert!(app.find.project());
    assert!(app.search.replace_mode);
    assert_eq!(app.focus, Focus::Search);
    assert!(
        !app.edit_mode(),
        "project replace does not edit the open tab"
    );
}

/// ctrl+r after opening a project result keeps the box's query: the selection is the
/// result the box put there, not a selection of the user's (review T22 major 1). A
/// selection the user makes afterwards still seeds the query.
#[test]
fn ctrl_r_after_opening_a_result_keeps_the_project_query() {
    let (_d, mut app) = project_app();
    app.handle_key(ctrl('f'));
    app.handle_key(ctrl('f'));
    typ(&mut app, "/ne+dle/i");
    app.handle_key(key(KeyCode::Enter));
    finish_search(&mut app);
    assert_eq!(app.search.total, 3);
    for _ in 0..3 {
        app.handle_key(key(KeyCode::Down));
    }
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.focus, Focus::Editor);
    assert_eq!(selection(&app), (13, 19), "the result is selected");
    app.handle_key(ctrl('r'));
    assert!(app.find.project());
    assert_eq!(app.search.input.text(), "/ne+dle/i", "query kept");
    assert_eq!(app.search.total, 3, "results kept");
    // f4 opens a result the same way: still no seed.
    app.focus = Focus::Editor;
    app.handle_key(key(KeyCode::F(4)));
    app.handle_key(ctrl('r'));
    assert_eq!(app.search.input.text(), "/ne+dle/i");
    // A selection of the user's own seeds the query as before.
    app.focus = Focus::Editor;
    app.active_doc_mut()
        .unwrap()
        .buffer
        .set_selection(vr_core::Selection::single(0, 1));
    app.handle_key(ctrl('r'));
    assert_eq!(app.search.input.text(), "f", "a.rs starts with fn");
}

/// D275: the box's fields take their rows before the terminal panel (the panel shrinks), so
/// at 40x9 with the terminal shown the query and replace rows are both drawn. A field the
/// box cannot draw (a window too short for it) never takes the keys.
#[test]
fn the_box_fields_keep_their_rows_with_the_terminal_shown() {
    let (_d, _p, mut app, _) = app_with_file("foo bar\nfoo\n");
    app.terminal.visible = true;
    draw(&mut app, 40, 9);
    app.handle_key(ctrl('f'));
    typ(&mut app, "foo");
    app.handle_key(ctrl('r'));
    assert_eq!(app.find.field, Field::Replace);
    typ(&mut app, "ZZ");
    let t = draw(&mut app, 40, 9);
    assert!(app.layout.terminal.height > 0, "{:?}", app.layout);
    let rows = box_rows(&app, &t);
    assert!(rows.len() >= 2, "{rows:?}");
    assert!(
        rows[0].starts_with(" File ") && rows[0].contains("foo"),
        "{rows:?}"
    );
    assert!(rows[1].contains("ZZ"), "the replace row is drawn: {rows:?}");
    assert!(
        app.layout.find_bar.y + app.layout.find_bar.height <= app.layout.terminal.y,
        "{:?}",
        app.layout
    );
    // Project scope keeps both rows too.
    app.handle_key(ctrl('f'));
    assert_eq!(app.find.scope, Scope::Project);
    let t = draw(&mut app, 40, 9);
    let rows = box_rows(&app, &t);
    assert!(rows[0].starts_with(" Project "), "{rows:?}");
    assert!(rows[1].contains("ZZ"), "{rows:?}");

    // 40x4: one row for the box. The replace field is not drawn, so it gets no keys:
    // typing goes to the query and Enter does not replace.
    let (_d, _p, mut app, _) = app_with_file("foo bar\nfoo\n");
    app.terminal.visible = true;
    draw(&mut app, 40, 4);
    app.handle_key(ctrl('f'));
    typ(&mut app, "foo");
    app.handle_key(ctrl('r'));
    draw(&mut app, 40, 4);
    assert_eq!(app.layout.find_bar.height, 1, "{:?}", app.layout);
    assert_eq!(
        app.find.field,
        Field::Find,
        "the hidden field lost the cursor"
    );
    typ(&mut app, "ZZ");
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(text(&app), "foo bar\nfoo\n", "nothing replaced unseen");
    assert_eq!(app.find.replace.text(), "");
    // Project scope, same window: the query field gets the keys.
    app.handle_key(ctrl('f'));
    app.handle_key(ctrl('r'));
    draw(&mut app, 40, 4);
    assert_eq!(app.search.field, crate::commands::search::Field::Query);
}

/// AC-22b: with several workspace roots the results are grouped by root, then file.
#[test]
fn project_results_are_grouped_by_root() {
    let d = tempfile::tempdir().unwrap();
    let (one, two) = (d.path().join("api"), d.path().join("web"));
    for (r, f) in [(&one, "x.rs"), (&two, "y.rs")] {
        std::fs::create_dir_all(r).unwrap();
        std::fs::write(r.join(f), "needle\n").unwrap();
    }
    let (mut app, _) = test_app(&one);
    app.add_root(&two).unwrap();
    app.handle_key(ctrl('f'));
    typ(&mut app, "needle");
    app.handle_key(key(KeyCode::Enter));
    finish_search(&mut app);
    let rows = app.search.rows();
    assert!(matches!(rows[0], Row::Root(_)), "{rows:?}");
    assert!(matches!(rows[1], Row::File(_)));
    assert!(matches!(rows[3], Row::Root(_)), "{rows:?}");
    let t = draw(&mut app, 80, 24);
    let shown = list_rows(&app, &t);
    assert_eq!(shown[0], "api");
    assert!(shown[1].starts_with("x.rs api"), "{shown:?}");
    assert_eq!(shown[3], "web");
    // Enter on a root row opens its first match.
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));
    assert!(
        app.active_doc()
            .unwrap()
            .path
            .as_ref()
            .unwrap()
            .ends_with("x.rs")
    );
}

/// Clicks: a list row selects that match (File scope); the box keeps the keys.
#[test]
fn clicking_a_file_scope_row_selects_the_match() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let (_d, _p, mut app, _) = app_with_file("foo\nbar\nfoo\n");
    app.handle_key(ctrl('f'));
    typ(&mut app, "foo");
    draw(&mut app, 80, 24);
    let l = app.find.list_area;
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: l.x + 4,
        row: l.y + 1,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(selection(&app), (8, 11));
    assert_eq!(app.focus, Focus::FindBar);
}

/// AC-22e timing (run in release: `cargo test --release -p vr-ide -- --ignored
/// find_box_in_1m_line_file`): typing, moving in the list and drawing stay fast in a file of
/// one million lines.
#[test]
#[ignore = "release timing test; run with --release -- --ignored"]
fn find_box_in_1m_line_file_stays_responsive() {
    let line = "the quick brown fox jumps over the lazy dog\n";
    let (_d, _p, mut app, _) = app_with_file(&line.repeat(1_000_000));
    app.handle_key(ctrl('f'));
    let mut worst = Duration::ZERO;
    for c in "fox".chars() {
        let t = Instant::now();
        app.handle_key_at(key(KeyCode::Char(c)), t);
        draw(&mut app, 120, 40);
        worst = worst.max(t.elapsed());
    }
    let t = Instant::now();
    app.tick(t + Duration::from_secs(1));
    let scan = t.elapsed();
    assert_eq!(count_text(&app), "1/20000+");
    for code in [KeyCode::PageDown, KeyCode::Down, KeyCode::PageUp] {
        for _ in 0..20 {
            let t = Instant::now();
            app.handle_key(key(code));
            draw(&mut app, 120, 40);
            worst = worst.max(t.elapsed());
        }
    }
    // Last match: line 20 000 of 1M, then f4 wraps.
    app.handle_key(shift(KeyCode::F(4)));
    let t = Instant::now();
    draw(&mut app, 120, 40);
    worst = worst.max(t.elapsed());
    eprintln!("1M lines: worst key+draw {worst:?}, capped scan {scan:?}");
    assert!(worst < Duration::from_millis(50), "key + draw {worst:?}");
    assert!(scan < Duration::from_millis(200), "capped scan {scan:?}");
}

/// D124 timing in a 100 MB file with a common letter (run in release:
/// `cargo test --release -p vr-ide -- --ignored find_in_100mb`).
#[test]
#[ignore = "release timing test; run with --release -- --ignored"]
fn find_in_100mb_file_stays_responsive() {
    let line = "the quick brown fox jumps over the lazy dog\n";
    let (_d, _p, mut app, _) = app_with_file(&line.repeat(100 * 1024 * 1024 / line.len()));
    app.handle_key(ctrl('f'));
    let mut worst = Duration::ZERO;
    for c in "e".chars() {
        let t = Instant::now();
        app.handle_key_at(key(KeyCode::Char(c)), t);
        draw(&mut app, 120, 40);
        worst = worst.max(t.elapsed());
    }
    let t = Instant::now();
    app.tick(t + Duration::from_secs(1));
    let capped_scan = t.elapsed();
    assert_eq!(count_text(&app), "1/20000+");

    // Typing in the editor while the box is open: no rescan per key.
    app.focus = Focus::Editor;
    for _ in 0..20 {
        let t = Instant::now();
        app.handle_key_at(key(KeyCode::Char('x')), t);
        draw(&mut app, 120, 40);
        worst = worst.max(t.elapsed());
    }
    // A query with no match scans the whole file once (an explicit command, e.g. f4).
    app.focus = Focus::FindBar;
    app.find.input.set("zqzq");
    let t = Instant::now();
    crate::commands::find::refresh_now(&mut app);
    let full_scan = t.elapsed();
    assert!(app.find.matches.is_empty());
    eprintln!(
        "100 MB: worst key+draw {worst:?}, capped scan {capped_scan:?}, full scan {full_scan:?}"
    );
    assert!(worst < Duration::from_millis(50), "key + draw {worst:?}");
    assert!(
        capped_scan < Duration::from_millis(200),
        "capped scan {capped_scan:?}"
    );
    assert!(
        full_scan < Duration::from_secs(3),
        "full scan {full_scan:?}"
    );
}

/// AC-22e timing (`cargo test --release -p vr-ide -- --ignored find_box_in_50k_file_project`):
/// a project of 50 000 files is searched off the UI thread; every frame (drain + draw)
/// while results stream in stays fast.
#[test]
#[ignore = "release timing test; run with --release -- --ignored"]
fn find_box_in_50k_file_project_stays_responsive() {
    let d = tempfile::tempdir().unwrap();
    let r = d.path().join("big");
    for i in 0..50_000 {
        let dir = r.join(format!("d{:03}", i / 500));
        if i % 500 == 0 {
            std::fs::create_dir_all(&dir).unwrap();
        }
        let body = if i % 10 == 0 {
            "let needle = 1;\n"
        } else {
            "nothing here\n"
        };
        std::fs::write(dir.join(format!("f{i}.rs")), body).unwrap();
    }
    let (mut app, _) = test_app(&r);
    app.handle_key(ctrl('f'));
    typ(&mut app, "needle");
    let start = Instant::now();
    app.handle_key(key(KeyCode::Enter));
    let enter = start.elapsed();
    let mut worst = Duration::ZERO;
    let deadline = Instant::now() + Duration::from_secs(120);
    while app.search.running() {
        assert!(Instant::now() < deadline, "search did not finish");
        let t = Instant::now();
        app.drain_search();
        draw(&mut app, 120, 40);
        worst = worst.max(t.elapsed());
        std::thread::sleep(Duration::from_millis(16));
    }
    let total = start.elapsed();
    assert_eq!(app.search.total, 5_000);
    // Moving in the full list.
    app.handle_key(key(KeyCode::Down));
    for _ in 0..20 {
        let t = Instant::now();
        app.handle_key(key(KeyCode::PageDown));
        draw(&mut app, 120, 40);
        worst = worst.max(t.elapsed());
    }
    eprintln!("50k files: Enter {enter:?}, worst frame {worst:?}, search {total:?}");
    assert!(enter < Duration::from_millis(50), "Enter {enter:?}");
    assert!(worst < Duration::from_millis(50), "frame {worst:?}");
}
