//! T23 (D262): the explorer's right-click menu, driven through `App::handle_mouse` /
//! `App::handle_key` as the event loop does.

use std::fs;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use crate::app::{App, Focus};
use crate::app_tests::test_app;
use crate::commands::dialog::DialogAction;
use crate::commands::explorer;

/// A project: src/{main.rs, lib.rs}, tests/, README.md.
fn project() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    fs::create_dir_all(r.join("src")).unwrap();
    fs::create_dir(r.join("tests")).unwrap();
    fs::write(r.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(r.join("src/lib.rs"), "\n").unwrap();
    fs::write(r.join("README.md"), "# shop\n").unwrap();
    let (mut app, _) = test_app(r);
    app.explorer.visible = true;
    explorer::ensure_loaded(&mut app);
    draw(&mut app, 80, 24);
    (dir, app)
}

fn draw(app: &mut App, w: u16, h: u16) {
    let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
    t.draw(|f| crate::ui::render(f, app)).unwrap();
}

fn mouse(app: &mut App, kind: MouseEventKind, column: u16, row: u16) {
    app.handle_mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    });
}

fn key(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

/// Screen row of the explorer row named `name`.
fn row_y(app: &App, name: &str) -> u16 {
    let i = app
        .explorer
        .rows
        .iter()
        .position(|r| r.name() == name)
        .unwrap_or_else(|| panic!("no row {name}"));
    app.explorer.area.y + (i - app.explorer.drawn_top) as u16
}

/// Right press and release at (column, row).
fn right_click(app: &mut App, column: u16, row: u16) {
    mouse(app, MouseEventKind::Down(MouseButton::Right), column, row);
    mouse(app, MouseEventKind::Up(MouseButton::Right), column, row);
}

fn right_click_row(app: &mut App, name: &str) {
    let y = row_y(app, name);
    let x = app.explorer.area.x + 2;
    right_click(app, x, y);
}

fn left_click(app: &mut App, column: u16, row: u16) {
    mouse(app, MouseEventKind::Down(MouseButton::Left), column, row);
    mouse(app, MouseEventKind::Up(MouseButton::Left), column, row);
}

/// (label, key, enabled) of each item of the open menu.
fn items(app: &App) -> Vec<(String, String, bool)> {
    let m = app.explorer.menu.as_ref().expect("menu open");
    m.items
        .iter()
        .map(|i| (i.label.to_string(), i.key.clone(), i.enabled))
        .collect()
}

fn enabled(app: &App, label: &str) -> bool {
    items(app)
        .into_iter()
        .find(|(l, _, _)| l == label)
        .unwrap_or_else(|| panic!("no item {label}"))
        .2
}

/// Screen cell of the menu item `label`.
fn item_cell(app: &App, label: &str) -> (u16, u16) {
    let m = app.explorer.menu.as_ref().expect("menu open");
    let i = m.items.iter().position(|i| i.label == label).unwrap();
    (m.area.x + 2, m.area.y + 1 + i as u16)
}

fn selected_label(app: &App) -> &'static str {
    let m = app.explorer.menu.as_ref().unwrap();
    m.items[m.selected].label
}

fn cursor_name(app: &App) -> String {
    app.explorer.current().unwrap().name()
}

#[test]
fn right_click_on_a_file_selects_it_and_opens_the_menu_at_the_pointer() {
    let (_d, mut app) = project();
    assert_eq!(app.focus, Focus::Editor);
    let y = row_y(&app, "README.md");
    let x = app.explorer.area.x + 4;
    right_click(&mut app, x, y);
    assert_eq!(app.focus, Focus::Explorer);
    assert_eq!(cursor_name(&app), "README.md");
    // A right click never opens the file.
    assert!(app.docs.is_empty());
    let m = app.explorer.menu.as_ref().expect("menu open");
    // Just under the clicked row, which stays readable.
    assert_eq!((m.area.x, m.area.y), (x, y + 1));
    // Every explorer key action, with its key from the keymap; Paste is dimmed with nothing
    // copied.
    assert_eq!(
        items(&app),
        [
            ("New…".into(), "ctrl+n".into(), true),
            ("Rename…".into(), "f2".into(), true),
            ("Copy".into(), "ctrl+c".into(), true),
            ("Cut".into(), "ctrl+x".into(), true),
            ("Paste".into(), "ctrl+v".into(), false),
            ("Delete".into(), "delete".into(), true),
        ]
    );
    assert_eq!(selected_label(&app), "New…");
    // Esc closes it and keeps the explorer focus and row.
    key(&mut app, KeyCode::Esc);
    assert!(app.explorer.menu.is_none());
    assert_eq!(app.focus, Focus::Explorer);
    assert_eq!(cursor_name(&app), "README.md");
}

#[test]
fn root_row_and_empty_space_offer_only_new_and_paste() {
    let (d, mut app) = project();
    let root = d.path().file_name().unwrap().to_string_lossy().into_owned();
    right_click_row(&mut app, &root);
    assert_eq!(app.explorer.cursor, 0);
    for label in ["Rename…", "Copy", "Cut", "Delete"] {
        assert!(!enabled(&app, label), "{label} on the root");
    }
    assert!(enabled(&app, "New…"));
    assert!(!enabled(&app, "Paste"));
    key(&mut app, KeyCode::Esc);

    // Something copied: Paste is offered; below the last row the root is the target.
    right_click_row(&mut app, "README.md");
    let (x, y) = item_cell(&app, "Copy");
    left_click(&mut app, x, y);
    assert!(app.explorer.menu.is_none());
    assert_eq!(app.explorer.copied, [d.path().join("README.md")]);
    draw(&mut app, 80, 24);
    let a = app.explorer.area;
    let below = a.y + app.explorer.rows.len() as u16 + 2;
    assert!(below < a.y + a.height);
    right_click(&mut app, a.x + 3, below);
    assert_eq!(app.explorer.cursor, 0, "the root row is the target");
    assert!(app.explorer.selected.is_empty());
    assert!(enabled(&app, "Paste"));
    assert!(!enabled(&app, "Rename…"));
    // Enter on Paste copies into the project folder.
    while selected_label(&app) != "Paste" {
        key(&mut app, KeyCode::Down);
    }
    key(&mut app, KeyCode::Enter);
    assert!(app.explorer.menu.is_none());
    assert!(d.path().join("README copy.md").is_file());
    assert_eq!(app.ran.last().map(String::as_str), Some("explorer.paste"));
}

#[test]
fn arrows_skip_dimmed_items_and_enter_runs_the_command() {
    let (d, mut app) = project();
    right_click_row(&mut app, "README.md");
    // New → Rename → Copy → Cut → (Paste dimmed) → Delete, and no further.
    let mut seen = vec![selected_label(&app)];
    for _ in 0..6 {
        key(&mut app, KeyCode::Down);
        seen.push(selected_label(&app));
    }
    assert_eq!(
        seen,
        [
            "New…",
            "Rename…",
            "Copy",
            "Cut",
            "Delete",
            "Delete",
            "Delete"
        ]
    );
    key(&mut app, KeyCode::Up);
    assert_eq!(selected_label(&app), "Cut");
    key(&mut app, KeyCode::Home);
    assert_eq!(selected_label(&app), "New…");
    key(&mut app, KeyCode::End);
    assert_eq!(selected_label(&app), "Delete");
    key(&mut app, KeyCode::Enter);
    assert!(app.explorer.menu.is_none());
    // The same confirmation as the delete key, for the row right-clicked.
    match &app.dialog.as_ref().expect("delete asks first").action {
        DialogAction::Trash(paths) => assert_eq!(paths, &[d.path().join("README.md")]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn rename_and_new_open_the_same_prompts_as_their_keys() {
    let (d, mut app) = project();
    right_click_row(&mut app, "src");
    let (x, y) = item_cell(&app, "Rename…");
    left_click(&mut app, x, y);
    match &app.dialog.as_ref().expect("rename prompt").action {
        DialogAction::Rename(p) => assert_eq!(p, &d.path().join("src")),
        other => panic!("{other:?}"),
    }
    key(&mut app, KeyCode::Esc);
    assert!(app.dialog.is_none());

    right_click_row(&mut app, "src");
    let (x, y) = item_cell(&app, "New…");
    left_click(&mut app, x, y);
    match &app.dialog.as_ref().expect("File / Folder").action {
        DialogAction::NewKind { dir } => assert_eq!(dir, &d.path().join("src")),
        other => panic!("{other:?}"),
    }
    assert_eq!(app.ran.last().map(String::as_str), Some("explorer.new"));
}

#[test]
fn cut_and_paste_through_the_menu_move_a_file_into_a_folder() {
    let (d, mut app) = project();
    right_click_row(&mut app, "README.md");
    let (x, y) = item_cell(&app, "Cut");
    left_click(&mut app, x, y);
    assert!(app.explorer.cut);
    draw(&mut app, 80, 24);
    right_click_row(&mut app, "tests");
    let (x, y) = item_cell(&app, "Paste");
    left_click(&mut app, x, y);
    assert!(d.path().join("tests/README.md").is_file());
    assert!(!d.path().join("README.md").exists());
}

#[test]
fn a_click_on_a_dimmed_item_does_nothing_and_a_click_outside_closes() {
    let (d, mut app) = project();
    let root = d.path().file_name().unwrap().to_string_lossy().into_owned();
    right_click_row(&mut app, &root);
    let (x, y) = item_cell(&app, "Delete");
    left_click(&mut app, x, y);
    assert!(
        app.explorer.menu.is_some(),
        "a dimmed item keeps the menu open"
    );
    assert!(app.dialog.is_none());
    assert!(!app.ran.iter().any(|c| c == "explorer.delete"));

    // A click in the editor only closes the menu; the focus stays in the explorer.
    let e = app.layout.editor;
    left_click(&mut app, e.x + e.width - 2, e.y + e.height - 2);
    assert!(app.explorer.menu.is_none());
    assert_eq!(app.focus, Focus::Explorer);
}

#[test]
fn pointer_moves_highlight_enabled_items() {
    let (_d, mut app) = project();
    right_click_row(&mut app, "README.md");
    let (x, y) = item_cell(&app, "Cut");
    mouse(&mut app, MouseEventKind::Moved, x, y);
    assert_eq!(selected_label(&app), "Cut");
    let (x, y) = item_cell(&app, "Paste");
    mouse(&mut app, MouseEventKind::Moved, x, y);
    assert_eq!(
        selected_label(&app),
        "Cut",
        "a dimmed item is never highlighted"
    );
}

#[test]
fn right_click_inside_a_multi_selection_keeps_it() {
    let (d, mut app) = project();
    app.run_command("explorer.focus", crate::commands::CmdArgs::None);
    // Cursor on tests, then shift+Down onto README.md.
    while cursor_name(&app) != "tests" {
        key(&mut app, KeyCode::Down);
    }
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT));
    assert_eq!(app.explorer.selected.len(), 2);
    draw(&mut app, 80, 24);
    right_click_row(&mut app, "tests");
    assert_eq!(app.explorer.selected.len(), 2);
    let (x, y) = item_cell(&app, "Delete");
    left_click(&mut app, x, y);
    match &app.dialog.as_ref().expect("delete asks first").action {
        DialogAction::Trash(paths) => {
            assert_eq!(paths, &[d.path().join("tests"), d.path().join("README.md")])
        }
        other => panic!("{other:?}"),
    }
    // A right click on a row outside the selection selects just that row.
    key(&mut app, KeyCode::Esc);
    draw(&mut app, 80, 24);
    right_click_row(&mut app, "src");
    assert!(app.explorer.selected.is_empty());
    assert_eq!(cursor_name(&app), "src");
}

#[test]
fn the_menu_stays_on_screen_near_the_edges() {
    let (_d, mut app) = project();
    draw(&mut app, 40, 12);
    let a = app.explorer.area;
    // Bottom of the explorer at a narrow, short screen: the menu opens above the pointer's
    // row and stays inside.
    let (px, py) = (a.x + a.width - 1, a.y + a.height - 1);
    right_click(&mut app, px, py);
    let m = app.explorer.menu.as_ref().unwrap().area;
    assert!(m.x + m.width <= 40 && m.y + m.height <= 12, "{m:?}");
    assert!(m.y + m.height <= py, "the clicked row stays visible: {m:?}");
    assert!(m.height >= 8, "every item fits: {m:?}");
    draw(&mut app, 40, 12);
    let m2 = app.explorer.menu.as_ref().unwrap().area;
    assert_eq!(m, m2, "the drawn menu is where clicks hit it");
}

#[test]
fn other_keys_close_the_menu_and_run_as_usual() {
    let (d, mut app) = project();
    right_click_row(&mut app, "README.md");
    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(app.explorer.menu.is_none());
    assert_eq!(app.explorer.copied, [d.path().join("README.md")]);
}

#[test]
fn a_dialog_or_a_hidden_explorer_closes_the_menu() {
    let (_d, mut app) = project();
    right_click_row(&mut app, "README.md");
    app.run_command("explorer.toggle", crate::commands::CmdArgs::None);
    assert!(!app.explorer.visible);
    assert!(app.explorer.menu.is_none());
    app.run_command("explorer.toggle", crate::commands::CmdArgs::None);
    draw(&mut app, 80, 24);
    right_click_row(&mut app, "README.md");
    app.run_command("explorer.new", crate::commands::CmdArgs::None);
    assert!(app.dialog.is_some());
    assert!(app.explorer.menu.is_none());
}
