//! Read mode / edit mode per editor tab (D145) through the `App` (no terminal).

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::app::{App, Focus, View};
use crate::app_tests::test_app;
use crate::clipboard::{Clipboard, MemoryClipboard};

const HINT: &str = "Read mode: ctrl+i to edit";

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn with(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, mods)
}

/// ctrl+i as a terminal with the kitty protocol reports it (legacy terminals send Tab).
fn ctrl_i() -> KeyEvent {
    ctrl('i')
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

fn hints(app: &App) -> usize {
    app.notices.iter().filter(|n| n.text == HINT).count()
}

/// A temp project with `files` (name, content), the first one opened the way a user opens
/// it (no ctrl+i pressed).
fn project(files: &[(&str, &str)]) -> (tempfile::TempDir, Vec<PathBuf>, App, MemoryClipboard) {
    let dir = tempfile::tempdir().unwrap();
    let paths: Vec<PathBuf> = files
        .iter()
        .map(|(n, c)| {
            let p = dir.path().join(n);
            std::fs::write(&p, c).unwrap();
            p
        })
        .collect();
    let (mut app, clip) = test_app(dir.path());
    app.open_path(&paths[0]).unwrap();
    (dir, paths, app, clip)
}

fn one(content: &str) -> (tempfile::TempDir, PathBuf, App, MemoryClipboard) {
    let (d, mut p, app, clip) = project(&[("a.txt", content)]);
    (d, p.remove(0), app, clip)
}

fn draw(app: &mut App, w: u16, h: u16) -> Terminal<TestBackend> {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| crate::ui::render(f, app)).unwrap();
    t
}

fn status_row(app: &mut App, w: u16, h: u16) -> String {
    let t = draw(app, w, h);
    let buf = t.backend().buffer();
    let y = app.layout.status.y;
    (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
}

// ---- read mode on focus ----------------------------------------------------------------

#[test]
fn opened_file_is_in_read_mode_and_typing_shows_the_hint() {
    let (_d, _p, mut app, _) = one("ab\n");
    typ(&mut app, "xyz");
    assert_eq!(text(&app), "ab\n");
    assert!(!app.active_doc().unwrap().is_modified());
    // One hint for the burst, not one per key.
    assert_eq!(hints(&app), 1, "{:?}", app.notices);
}

#[test]
fn ctrl_i_enters_edit_mode_and_edits_and_undo_work() {
    let (_d, _p, mut app, _) = one("ab\n");
    app.handle_key(ctrl_i());
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Backspace));
    assert_eq!(text(&app), "ab\n");
    typ(&mut app, "x");
    assert_eq!(text(&app), "xab\n");
    app.handle_key(ctrl('z'));
    assert_eq!(text(&app), "ab\n");
    app.handle_key(ctrl('y'));
    assert_eq!(text(&app), "xab\n");
    assert_eq!(hints(&app), 0);
}

#[test]
fn legacy_tab_enters_edit_mode_without_inserting_and_then_indents() {
    let (_d, _p, mut app, _) = one("ab\n");
    // A terminal without the kitty protocol sends ctrl+i as Tab.
    app.handle_key(key(KeyCode::Tab));
    assert_eq!(text(&app), "ab\n");
    typ(&mut app, "x");
    assert_eq!(text(&app), "xab\n");
    app.handle_key(key(KeyCode::Tab));
    assert_eq!(text(&app), "x   ab\n");
}

#[test]
fn esc_returns_to_read_mode_and_clears_the_selection() {
    let (_d, _p, mut app, _) = one("abc\n");
    app.handle_key(ctrl_i());
    app.handle_key(with(KeyCode::Right, KeyModifiers::SHIFT));
    assert_eq!(selection(&app), (0, 1));
    app.handle_key(key(KeyCode::Esc));
    let (from, to) = selection(&app);
    assert_eq!(from, to, "selection kept");
    typ(&mut app, "x");
    assert_eq!(text(&app), "abc\n");
    assert_eq!(hints(&app), 1);
}

#[test]
fn esc_in_edit_mode_goes_to_read_even_with_a_notice_shown() {
    let (_d, _p, mut app, _) = one("abc\n");
    app.handle_key(ctrl_i());
    app.warn("something to read");
    app.handle_key(key(KeyCode::Esc));
    typ(&mut app, "x");
    assert_eq!(text(&app), "abc\n");
    // The next Esc dismisses the oldest notice as before.
    let before = app.notices.len();
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.notices.len(), before - 1);
}

#[test]
fn tab_switch_returns_to_read_mode() {
    let (_d, paths, mut app, _) = project(&[("a.txt", "a\n"), ("b.txt", "b\n")]);
    app.open_path(&paths[1]).unwrap();
    app.handle_key(ctrl_i());
    typ(&mut app, "1");
    assert_eq!(text(&app), "1b\n");
    crate::app_tests::press(&mut app, "shift+f9");
    typ(&mut app, "2");
    assert_eq!(text(&app), "a\n", "the other tab is in read mode");
    crate::app_tests::press(&mut app, "f9");
    typ(&mut app, "3");
    assert_eq!(
        text(&app),
        "1b\n",
        "back on the edited tab: read mode again"
    );
}

#[test]
fn back_from_the_explorer_is_read_mode() {
    let (_d, _p, mut app, _) = one("a\n");
    app.handle_key(ctrl_i());
    app.handle_key(ctrl('e'));
    assert_eq!(app.focus, Focus::Explorer);
    app.handle_key(ctrl('e'));
    assert_eq!(app.focus, Focus::Editor);
    typ(&mut app, "x");
    assert_eq!(text(&app), "a\n");
}

#[test]
fn palette_esc_returns_to_read_mode() {
    let (_d, _p, mut app, _) = one("a\n");
    app.handle_key(ctrl_i());
    app.handle_key(key(KeyCode::F(1)));
    assert_eq!(app.focus, Focus::Palette);
    app.handle_key(key(KeyCode::Esc));
    assert!(app.palette.is_none(), "Esc closes the palette as before");
    typ(&mut app, "x");
    assert_eq!(text(&app), "a\n");
}

#[test]
fn palette_click_outside_returns_to_read_mode() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let (_d, _p, mut app, _) = one("a\n");
    app.handle_key(ctrl_i());
    app.handle_key(key(KeyCode::F(1)));
    draw(&mut app, 80, 24);
    let area = app.palette.as_ref().unwrap().area;
    assert!(area.y + area.height < 24, "a cell below the palette exists");
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 0,
        row: 23,
        modifiers: KeyModifiers::NONE,
    });
    assert!(app.palette.is_none(), "a click outside closes the palette");
    assert!(!app.edit_mode(), "and the tab is back in read mode");
    typ(&mut app, "x");
    assert_eq!(text(&app), "a\n");
}

#[test]
fn palette_go_to_file_on_the_active_tab_lands_in_read_mode() {
    let (_d, _p, mut app, _) = one("a\n");
    app.handle_key(ctrl_i());
    crate::commands::palette::open_files(&mut app);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while app.index_rx.is_some() {
        assert!(std::time::Instant::now() < deadline, "index never finished");
        std::thread::sleep(std::time::Duration::from_millis(5));
        app.drain_index();
    }
    typ(&mut app, "a.txt");
    app.handle_key(key(KeyCode::Enter));
    assert!(app.palette.is_none());
    assert!(!app.edit_mode(), "picking a file lands in READ (D145 (1))");
    typ(&mut app, "x");
    assert_eq!(text(&app), "a\n");
}

#[test]
fn edit_mode_runs_from_the_palette() {
    let (_d, _p, mut app, _) = one("a\n");
    app.run_command("app.palette", crate::commands::CmdArgs::None);
    typ(&mut app, "edit mode");
    app.handle_key(key(KeyCode::Enter));
    assert!(app.palette.is_none());
    typ(&mut app, "x");
    assert_eq!(text(&app), "xa\n");
}

#[test]
fn untitled_buffer_starts_in_edit_mode() {
    let dir = tempfile::tempdir().unwrap();
    let (mut app, _) = test_app(dir.path());
    app.docs.push(vr_core::Document::untitled());
    app.views.push(View::default());
    app.set_active(0);
    typ(&mut app, "x");
    assert_eq!(text(&app), "x");
}

#[test]
fn new_file_from_ctrl_n_opens_in_edit_mode() {
    let (d, _p, mut app, _) = one("a\n");
    app.handle_key(ctrl('n'));
    assert!(app.dialog.is_some(), "name prompt");
    typ(&mut app, "new.txt");
    app.handle_key(key(KeyCode::Enter));
    assert!(app.dialog.is_none());
    assert_eq!(app.active_name(), "new.txt");
    typ(&mut app, "x");
    assert_eq!(text(&app), "x");
    assert!(d.path().join("new.txt").exists());
}

// ---- blocked in read mode ----------------------------------------------------------------

#[test]
fn read_mode_blocks_every_text_changing_key_and_command() {
    let body = "fn a() {\n    b();\n}\n";
    let (_d, _p, mut app, mut clip) = one(body);
    clip.set("PASTED").unwrap();
    // Undo and redo history first (q stays, r is undone), then leave edit mode.
    app.handle_key(ctrl_i());
    typ(&mut app, "q");
    app.handle_key(key(KeyCode::Right));
    typ(&mut app, "r");
    app.handle_key(ctrl('z'));
    let body = format!("q{body}");
    assert_eq!(text(&app), body);
    app.handle_key(key(KeyCode::Esc));
    let blocked: Vec<(&str, KeyEvent)> = vec![
        ("Enter", key(KeyCode::Enter)),
        ("Backspace", key(KeyCode::Backspace)),
        ("Delete", key(KeyCode::Delete)),
        ("ctrl+v", ctrl('v')),
        ("ctrl+x", ctrl('x')),
        ("ctrl+z", ctrl('z')),
        ("ctrl+y", ctrl('y')),
        ("ctrl+]", ctrl(']')),
        ("shift+tab", with(KeyCode::BackTab, KeyModifiers::SHIFT)),
    ];
    for (name, k) in blocked {
        // Start every key on the second line so Backspace / Delete have something to remove.
        app.handle_key(key(KeyCode::Down));
        app.handle_key(key(KeyCode::Right));
        app.notices.clear();
        app.handle_key(k);
        assert_eq!(text(&app), body, "{name} changed the text in read mode");
        assert_eq!(hints(&app), 1, "{name}: no hint {:?}", app.notices);
    }
    // The line keys that change text are refused with the same hint (D250).
    for k in ["ctrl+d", "ctrl+l", "ctrl+/", "ctrl+up", "ctrl+down"] {
        app.handle_key(key(KeyCode::Down));
        app.notices.clear();
        crate::app_tests::press(&mut app, k);
        assert_eq!(text(&app), body, "{k} changed the text in read mode");
        assert_eq!(hints(&app), 1, "{k}: {:?}", app.notices);
    }
    assert_eq!(
        clip.get().as_deref(),
        Some("PASTED"),
        "cut reached the clipboard"
    );
    // Bracketed paste from the terminal.
    app.notices.clear();
    app.handle_paste("from the terminal");
    assert_eq!(text(&app), body);
    assert_eq!(hints(&app), 1);
}

#[test]
fn find_works_in_read_mode_and_replace_is_refused() {
    let (_d, _p, mut app, _) = one("foo bar foo\n");
    app.handle_key(ctrl('f'));
    typ(&mut app, "bar");
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(selection(&app), (4, 7), "find selects the match");
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.focus, Focus::Editor, "Esc closes the bar");
    assert!(!app.find.open);
    // Replace: field switch and typing in the bar still work, the replace itself is refused.
    // With a query already typed, ctrl+u puts the keys in the replace field.
    crate::app_tests::press(&mut app, "ctrl+u");
    typ(&mut app, "X");
    app.notices.clear();
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(text(&app), "foo bar foo\n", "replace one in read mode");
    crate::app_tests::press(&mut app, "shift+f7");
    assert_eq!(text(&app), "foo bar foo\n", "replace all in read mode");
    assert!(hints(&app) >= 1, "{:?}", app.notices);
    // ctrl+i from the bar enters edit mode; the replace then works.
    app.handle_key(ctrl_i());
    crate::app_tests::press(&mut app, "shift+f7");
    assert_eq!(text(&app), "foo X foo\n");
}

#[test]
fn esc_in_the_find_bar_keeps_edit_mode() {
    let (_d, _p, mut app, _) = one("ab\n");
    app.handle_key(ctrl_i());
    app.handle_key(ctrl('f'));
    app.handle_key(key(KeyCode::Esc));
    assert!(!app.find.open, "Esc closes the bar");
    assert_eq!(app.focus, Focus::Editor);
    typ(&mut app, "x");
    assert_eq!(text(&app), "xab\n");
}

// ---- allowed in read mode ----------------------------------------------------------------

#[test]
fn read_mode_allows_moving_selecting_copying_and_saving() {
    let (_d, path, mut app, mut clip) = one("abc\ndef\n");
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::End));
    assert_eq!(selection(&app), (7, 7));
    app.handle_key(with(KeyCode::Home, KeyModifiers::SHIFT));
    assert_eq!(selection(&app), (4, 7));
    app.handle_key(ctrl('c'));
    assert_eq!(clip.get().as_deref(), Some("def"));
    app.handle_key(ctrl('a'));
    assert_eq!(selection(&app), (0, 8));
    assert_eq!(hints(&app), 0, "{:?}", app.notices);
    // An edit made in edit mode is saved from read mode.
    app.handle_key(ctrl_i());
    typ(&mut app, "z");
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(ctrl('s'));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "z");
    assert!(!app.active_doc().unwrap().is_modified());
}

// ---- buffers change under read mode -------------------------------------------------------

#[test]
fn external_reload_updates_a_buffer_in_read_mode() {
    let (_d, path, mut app, _) = one("old\n");
    std::fs::write(&path, "new\n").unwrap();
    app.handle_fs_event(vr_core::FsEvent::Modified(path.clone()));
    assert_eq!(text(&app), "new\n");
}

// ---- status bar ------------------------------------------------------------------------

#[test]
fn narrow_status_bar_shrinks_the_badge_before_dropping_eol_and_encoding() {
    for edit in [false, true] {
        let (_d, _p, mut app, _) = one("abc\n");
        if edit {
            app.handle_key(ctrl_i());
        }
        let mut dropped = false;
        for w in (28..=50).rev() {
            let row = status_row(&mut app, w, 12);
            let badge = row.trim_start();
            let lead = row.len() - badge.len();
            let word = badge.split(' ').next().unwrap();
            assert!(
                matches!(word, "READ" | "EDIT" | "R" | "E")
                    && badge[word.len()..].starts_with(' ')
                    && !badge[word.len()..].starts_with("  "),
                "{w}: a space between the badge and the path ({lead}): {row:?}"
            );
            let has = row.contains("UTF-8");
            assert!(!(dropped && has), "{w}: UTF-8 came back: {row:?}");
            // ' R ' + the long right side + the name and its cut mark need 37 cells.
            assert!(
                w < 37 || has,
                "{w}: encoding dropped while the badge could shrink: {row:?}"
            );
            dropped |= !has;
        }
        let at40 = status_row(&mut app, 40, 12);
        assert!(
            at40.contains("UTF-8"),
            "40 cols keep the encoding: {at40:?}"
        );
        assert!(at40.contains(" a.txt"), "{at40:?}");
    }
}

#[test]
fn status_bar_shows_read_and_edit() {
    for (w, h) in [(80, 24), (40, 12)] {
        let (_d, _p, mut app, _) = one("abc\n");
        let row = status_row(&mut app, w, h);
        assert!(row.contains("READ"), "{w}x{h}: {row:?}");
        app.handle_key(ctrl_i());
        let row = status_row(&mut app, w, h);
        assert!(
            row.contains("EDIT") && !row.contains("READ"),
            "{w}x{h}: {row:?}"
        );
        assert!(row.contains("a.txt"), "{w}x{h}: file name lost {row:?}");
    }
}

#[test]
fn cursor_is_a_block_in_read_mode_and_a_bar_where_text_is_typed() {
    let (_d, _p, mut app, _) = one("abc\n");
    assert!(!app.wants_bar_cursor(), "read mode: block");
    app.handle_key(ctrl_i());
    assert!(app.wants_bar_cursor(), "edit mode: bar");
    app.handle_key(key(KeyCode::Esc));
    assert!(!app.wants_bar_cursor());
    // Input fields take text in either mode.
    app.handle_key(ctrl('f'));
    assert!(app.wants_bar_cursor(), "find bar: bar");
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(key(KeyCode::F(1)));
    assert!(app.wants_bar_cursor(), "palette: bar");
}

// ---- the mode across close, Save As, an external change prompt and quit --------------------

#[test]
fn closing_the_edited_tab_leaves_the_next_one_in_read_mode() {
    let (_d, paths, mut app, _) = project(&[("a.txt", "a\n"), ("b.txt", "b\n")]);
    app.open_path(&paths[1]).unwrap();
    app.handle_key(ctrl_i());
    app.handle_key(ctrl('w'));
    assert_eq!(app.docs.len(), 1);
    typ(&mut app, "x");
    assert_eq!(text(&app), "a\n");
    app.handle_key(ctrl_i());
    typ(&mut app, "x");
    assert_eq!(text(&app), "xa\n");
}

#[test]
fn save_as_and_the_external_change_prompt_return_in_read_mode_and_keep_the_edits() {
    let (d, path, mut app, _) = one("old\n");
    app.handle_key(ctrl_i());
    typ(&mut app, "mine ");
    std::fs::write(&path, "theirs\n").unwrap();
    app.handle_fs_event(vr_core::FsEvent::Modified(path.clone()));
    assert!(app.dialog.is_some(), "the external change prompt");
    // Move to "Keep mine" (buttons: Reload, Keep mine) and confirm.
    let crate::ui::dialog::DialogWidget::Confirm(c) = &app.dialog.as_ref().unwrap().widget else {
        panic!("confirm expected")
    };
    let keep = c.buttons.iter().position(|b| b == "Keep mine").unwrap();
    let steps = (keep + c.buttons.len() - c.selected) % c.buttons.len();
    for _ in 0..steps {
        app.handle_key(key(KeyCode::Right));
    }
    app.handle_key(key(KeyCode::Enter));
    assert!(app.dialog.is_none());
    assert_eq!(
        text(&app),
        "mine old\n",
        "the user's edits survive the prompt"
    );
    typ(&mut app, "x");
    assert_eq!(text(&app), "mine old\n", "back from the dialog: read mode");
    // Save As from read mode writes the buffer and returns in read mode.
    crate::app_tests::press(&mut app, "f12");
    assert!(app.dialog.is_some(), "save as prompt");
    for _ in 0..10 {
        app.handle_key(key(KeyCode::Backspace));
    }
    typ(&mut app, "copy.txt");
    app.handle_key(key(KeyCode::Enter));
    assert!(app.dialog.is_none(), "{:?}", app.notices);
    assert_eq!(
        std::fs::read_to_string(d.path().join("copy.txt")).unwrap(),
        "mine old\n"
    );
    typ(&mut app, "x");
    assert_eq!(text(&app), "mine old\n");
}

#[test]
fn quit_from_edit_mode_still_asks_about_unsaved_changes() {
    let (_d, _p, mut app, _) = one("a\n");
    app.handle_key(ctrl_i());
    typ(&mut app, "x");
    app.handle_key(ctrl('q'));
    assert!(!app.quit && app.dialog.is_some(), "quit guard");
}

// ---- read-only documents never enter edit mode (FR-11) ---------------------------------

fn read_only_app() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("latin1.txt");
    std::fs::write(&path, b"caf\xe9\n").unwrap();
    let (mut app, _) = test_app(dir.path());
    app.open_path(&path).unwrap();
    assert!(app.active_doc().unwrap().read_only);
    (dir, app)
}

const RO_WARNING: &str = "latin1.txt: read-only (not valid UTF-8)";

fn has_notice(app: &App, text: &str) -> bool {
    app.notices.iter().any(|n| n.text == text)
}

#[test]
fn read_only_file_stays_in_read_mode_on_ctrl_i_tab_and_palette() {
    let (_d, mut app) = read_only_app();
    app.handle_key(ctrl_i());
    assert!(!app.edit_mode(), "ctrl+i");
    assert!(!app.wants_bar_cursor(), "block cursor stays");
    assert!(has_notice(&app, RO_WARNING), "{:?}", app.notices);
    app.notices.clear();
    app.handle_key(key(KeyCode::Tab));
    assert!(!app.edit_mode(), "Tab");
    assert!(has_notice(&app, RO_WARNING), "{:?}", app.notices);
    app.notices.clear();
    app.run_command("app.palette", crate::commands::CmdArgs::None);
    typ(&mut app, "edit mode");
    app.handle_key(key(KeyCode::Enter));
    assert!(!app.edit_mode(), "palette");
    assert!(has_notice(&app, RO_WARNING), "{:?}", app.notices);
}

#[test]
fn notice_esc_hint_is_only_shown_when_esc_dismisses_it() {
    use crate::ui::status_bar::notice_text;
    let (_d, _p, mut app, _) = one("abc\n");
    app.warn("careful");
    assert_eq!(notice_text(&app).unwrap(), "careful  (Esc)");
    // In edit mode the first Esc leaves it (D151): no hint that promises a dismissal.
    app.handle_key(ctrl_i());
    assert_eq!(notice_text(&app).unwrap(), "careful");
    app.warn("second");
    assert_eq!(notice_text(&app).unwrap(), "careful  (+1 more)");
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(notice_text(&app).unwrap(), "careful  (+1 more · Esc)");
    // The find bar closes on Esc before anything else.
    app.run_command("find.open", crate::commands::CmdArgs::None);
    assert_eq!(app.focus, Focus::FindBar);
    assert_eq!(notice_text(&app).unwrap(), "careful  (+1 more)");
}

#[test]
fn read_only_warning_names_the_real_encoding() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("wide.txt");
    std::fs::write(&path, b"\xff\xfea\0b\0\n\0").unwrap();
    let (mut app, _) = test_app(dir.path());
    app.open_path(&path).unwrap();
    assert!(app.active_doc().unwrap().read_only);
    app.notices.clear();
    app.handle_key(ctrl_i());
    assert!(!app.edit_mode());
    let n = &app.notices.front().expect("a warning").text;
    assert_eq!(n, "wide.txt: read-only (UTF-16LE, not UTF-8)");
}
