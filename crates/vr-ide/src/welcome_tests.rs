//! Welcome screen on demand (D186): ctrl+shift+w shows it over the editor while tabs stay open.

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::app::{App, Focus};
use crate::app_tests::test_app;

fn press(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    app.handle_key(KeyEvent::new(code, mods));
}

fn welcome_key(app: &mut App) {
    press(
        app,
        KeyCode::Char('w'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
}

/// Two files open (b.txt active, in edit mode).
fn two_tabs() -> (tempfile::TempDir, Vec<PathBuf>, App) {
    let dir = tempfile::tempdir().unwrap();
    let paths: Vec<PathBuf> = ["a.txt", "b.txt"]
        .iter()
        .map(|n| {
            let p = dir.path().join(n);
            std::fs::write(&p, format!("{n} text\n")).unwrap();
            p
        })
        .collect();
    let (mut app, _) = test_app(dir.path());
    app.keyboard_enhanced = true;
    for p in &paths {
        app.open_path(p).unwrap();
    }
    press(&mut app, KeyCode::Char('i'), KeyModifiers::CONTROL);
    (dir, paths, app)
}

fn screen(app: &mut App, w: u16, h: u16) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| crate::ui::render(f, app)).unwrap();
    format!("{:?}", t.backend())
}

fn text(app: &App) -> String {
    app.active_doc().unwrap().buffer.text().to_string()
}

#[test]
fn shortcut_shows_welcome_with_tabs_kept_and_again_hides_it() {
    let (_d, _p, mut app) = two_tabs();
    assert!(!app.welcome);
    welcome_key(&mut app);
    assert!(app.welcome);
    assert_eq!(app.docs.len(), 2);
    let s = screen(&mut app, 80, 24);
    assert!(s.contains("Command palette"), "{s}");
    assert!(
        s.contains("a.txt") && s.contains("b.txt"),
        "tabs kept:\n{s}"
    );
    assert!(!s.contains("Ln 1"), "no file position:\n{s}");
    welcome_key(&mut app);
    assert!(!app.welcome);
    assert!(screen(&mut app, 80, 24).contains("Ln 1"));
}

#[test]
fn esc_hides_welcome() {
    let (_d, _p, mut app) = two_tabs();
    welcome_key(&mut app);
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!app.welcome);
    assert_eq!(app.docs.len(), 2);
}

#[test]
fn keys_typed_while_welcome_is_up_never_edit_the_document() {
    let (_d, _p, mut app) = two_tabs();
    let before = text(&app);
    welcome_key(&mut app);
    for c in "xyz".chars() {
        press(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
    }
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    press(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
    press(&mut app, KeyCode::Delete, KeyModifiers::NONE);
    press(&mut app, KeyCode::Tab, KeyModifiers::NONE);
    press(&mut app, KeyCode::Char('x'), KeyModifiers::CONTROL);
    press(&mut app, KeyCode::Char('z'), KeyModifiers::CONTROL);
    press(
        &mut app,
        KeyCode::Char('d'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    );
    app.handle_paste("pasted");
    assert_eq!(text(&app), before);
    assert!(!app.active_doc().unwrap().is_modified());
    assert!(app.welcome, "typing does not close it");
}

#[test]
fn opening_a_file_hides_welcome() {
    let (d, p, mut app) = two_tabs();
    welcome_key(&mut app);
    app.open_path(&p[0]).unwrap();
    assert!(!app.welcome);
    welcome_key(&mut app);
    let c = d.path().join("c.txt");
    std::fs::write(&c, "c\n").unwrap();
    app.open_path(&c).unwrap();
    assert!(!app.welcome);
}

#[test]
fn switching_tabs_hides_welcome() {
    let (_d, _p, mut app) = two_tabs();
    welcome_key(&mut app);
    press(&mut app, KeyCode::PageUp, KeyModifiers::CONTROL);
    assert!(!app.welcome);
    welcome_key(&mut app);
    press(&mut app, KeyCode::PageDown, KeyModifiers::CONTROL);
    assert!(!app.welcome);
    // A click on a tab, including the active one.
    welcome_key(&mut app);
    screen(&mut app, 80, 24);
    let t = app.layout.tabs;
    app.handle_mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: t.x + 2,
        row: t.y,
        modifiers: KeyModifiers::NONE,
    });
    assert!(!app.welcome);
}

#[test]
fn closing_the_last_tab_shows_the_plain_welcome() {
    let (_d, _p, mut app) = two_tabs();
    welcome_key(&mut app);
    app.close_doc(1);
    app.close_doc(0);
    assert!(screen(&mut app, 80, 24).contains("Command palette"));
}

#[test]
fn palette_lists_welcome_and_runs_it() {
    let (_d, _p, mut app) = two_tabs();
    assert!(crate::commands::find("app.welcome").is_some_and(|c| c.title == "Welcome"));
    press(&mut app, KeyCode::F(1), KeyModifiers::NONE);
    for c in "welcome".chars() {
        press(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
    }
    let items = &app.palette.as_ref().unwrap().items;
    assert!(items.iter().any(|i| i.name == "app.welcome"));
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.welcome);
    assert_eq!(app.focus, Focus::Editor);
}

#[test]
fn welcome_shown_with_no_tabs_is_harmless() {
    let dir = tempfile::tempdir().unwrap();
    let (mut app, _) = test_app(dir.path());
    app.run_command("app.welcome", crate::commands::CmdArgs::None);
    app.run_command("app.welcome", crate::commands::CmdArgs::None);
    assert!(!app.welcome);
}

fn selection(app: &App) -> (usize, usize) {
    let r = app.active_doc().unwrap().buffer.selection().primary();
    (r.from(), r.to())
}

#[test]
fn mouse_in_the_editor_area_does_nothing_while_welcome_is_up() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let (_d, _p, mut app) = two_tabs();
    welcome_key(&mut app);
    screen(&mut app, 80, 24);
    let before = selection(&app);
    let e = app.layout.editor;
    let ev = |kind, column| MouseEvent {
        kind,
        column,
        row: e.y + 1,
        modifiers: KeyModifiers::NONE,
    };
    app.handle_mouse(ev(MouseEventKind::Down(MouseButton::Left), e.x + 8));
    app.handle_mouse(ev(MouseEventKind::Drag(MouseButton::Left), e.x + 10));
    app.handle_mouse(ev(MouseEventKind::Up(MouseButton::Left), e.x + 10));
    assert_eq!(selection(&app), before);
    assert!(!app.dragging);
    assert!(app.welcome);
}

#[test]
fn document_keys_and_commands_do_nothing_while_welcome_is_up() {
    let (_d, _p, mut app) = two_tabs();
    app.run_command("editor.read_mode", crate::commands::CmdArgs::None);
    welcome_key(&mut app);
    let before = (selection(&app), app.active_doc().unwrap().is_modified());
    let mode_before = app.edit_mode();
    for (c, mods) in [
        ('f', KeyModifiers::CONTROL),
        ('g', KeyModifiers::CONTROL),
        ('s', KeyModifiers::CONTROL),
        ('h', KeyModifiers::CONTROL),
        ('a', KeyModifiers::CONTROL),
        ('c', KeyModifiers::CONTROL),
        ('i', KeyModifiers::CONTROL),
    ] {
        press(&mut app, KeyCode::Char(c), mods);
        assert!(app.welcome, "ctrl+{c}");
        assert!(!app.find.open, "ctrl+{c} opened the find bar");
        assert!(app.dialog.is_none(), "ctrl+{c} opened a dialog");
        assert_eq!(app.focus, Focus::Editor, "ctrl+{c}");
    }
    assert_eq!(app.edit_mode(), mode_before, "ctrl+i changed the mode");
    assert_eq!(
        (selection(&app), app.active_doc().unwrap().is_modified()),
        before
    );
    for name in [
        "find.open",
        "nav.goto_line",
        "edit.select_all",
        "edit.copy",
        "file.save",
        "file.close",
        "editor.edit_mode",
        "file.save_overwrite",
    ] {
        app.run_command(name, crate::commands::CmdArgs::None);
        assert!(
            !app.find.open && app.dialog.is_none() && app.docs.len() == 2,
            "{name}"
        );
    }
    assert_eq!(selection(&app), before.0);
}

#[test]
fn palette_commands_do_not_reach_the_hidden_document() {
    let (_d, _p, mut app) = two_tabs();
    welcome_key(&mut app);
    for query in ["find", "go to line", "select all", "close file"] {
        press(&mut app, KeyCode::F(1), KeyModifiers::NONE);
        for c in query.chars() {
            press(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
        }
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(!app.find.open, "{query}");
        assert!(app.dialog.is_none(), "{query}");
        assert_eq!(app.docs.len(), 2, "{query}");
        assert!(app.welcome, "{query}");
    }
}

#[test]
fn the_welcome_filter_allows_only_commands_that_do_not_touch_the_document() {
    use crate::commands::app::allowed_over_welcome;
    for ok in [
        "app.welcome",
        "app.palette",
        "app.quit",
        "tab.next",
        "explorer.toggle_focus",
        "file.open",
        "file.goto",
        "focus.cycle",
        "search.project",
    ] {
        assert!(allowed_over_welcome(ok), "{ok}");
    }
    for no in [
        "edit.undo",
        "find.open",
        "nav.goto_line",
        "file.save",
        "file.save_as",
        "file.save_overwrite",
        "file.close",
        "file.reload",
        "editor.edit_mode",
        "editor.read_mode",
        "unknown.command",
    ] {
        assert!(!allowed_over_welcome(no), "{no}");
    }
}

#[test]
fn ctrl_alt_w_toggles_welcome_too() {
    let (_d, _p, mut app) = two_tabs();
    let ctrl_alt = KeyModifiers::CONTROL | KeyModifiers::ALT;
    press(&mut app, KeyCode::Char('w'), ctrl_alt);
    assert!(app.welcome);
    press(&mut app, KeyCode::Char('w'), ctrl_alt);
    assert!(!app.welcome);
}
