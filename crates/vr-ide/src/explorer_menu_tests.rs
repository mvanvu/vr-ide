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

// FR-31 (D328): Search / Replace in Folder.

const SEARCH_IN: &str = "Search in Folder";
const REPLACE_IN: &str = "Replace in Folder";

/// src/{a.txt, sub/b.txt}, other/c.txt, top.txt: each holding `needle`.
fn needle_project() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path().canonicalize().unwrap();
    for f in ["src/a.txt", "src/sub/b.txt", "other/c.txt", "top.txt"] {
        fs::create_dir_all(r.join(f).parent().unwrap()).unwrap();
        fs::write(r.join(f), "a needle here\n").unwrap();
    }
    let (mut app, _) = test_app(&r);
    app.explorer.visible = true;
    explorer::ensure_loaded(&mut app);
    draw(&mut app, 100, 30);
    (dir, app)
}

fn labels(app: &App) -> Vec<String> {
    items(app).into_iter().map(|(l, _, _)| l).collect()
}

fn type_text(app: &mut App, s: &str) {
    for c in s.chars() {
        key(app, KeyCode::Char(c));
    }
}

fn wait_search(app: &mut App) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        app.drain_search();
        if !app.search.running() && !app.search.busy() {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "search did not finish"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

fn found(app: &App) -> Vec<String> {
    let mut v: Vec<String> = app
        .search
        .groups
        .iter()
        .map(|g| app.display_path(&g.path))
        .collect();
    v.sort();
    v
}

fn choose_with_mouse(app: &mut App, row_name: &str, label: &str) {
    right_click_row(app, row_name);
    let (x, y) = item_cell(app, label);
    left_click(app, x, y);
    assert!(app.explorer.menu.is_none());
}

fn choose_with_keys(app: &mut App, row_name: &str, label: &str) {
    right_click_row(app, row_name);
    while selected_label(app) != label {
        key(app, KeyCode::Down);
    }
    key(app, KeyCode::Enter);
    assert!(app.explorer.menu.is_none());
}

#[test]
fn menu_offers_folder_search_for_a_root_and_a_folder_but_not_a_file() {
    let (d, mut app) = needle_project();
    let root = d.path().file_name().unwrap().to_string_lossy().into_owned();
    for name in [root.as_str(), "src"] {
        right_click_row(&mut app, name);
        assert!(enabled(&app, SEARCH_IN), "{name}");
        assert!(enabled(&app, REPLACE_IN), "{name}");
        key(&mut app, KeyCode::Esc);
    }
    right_click_row(&mut app, "top.txt");
    let l = labels(&app);
    assert!(
        !l.iter().any(|x| x == SEARCH_IN || x == REPLACE_IN),
        "{l:?}"
    );
}

#[test]
fn search_in_folder_by_mouse_and_keys_limits_results_to_the_folder() {
    for by_mouse in [true, false] {
        let (d, mut app) = needle_project();
        if by_mouse {
            choose_with_mouse(&mut app, "src", SEARCH_IN);
        } else {
            choose_with_keys(&mut app, "src", SEARCH_IN);
        }
        assert!(app.find.open);
        assert_eq!(app.find.scope, crate::commands::find::Scope::Project);
        assert_eq!(app.focus, Focus::Search);
        assert_eq!(
            app.search.folder.as_deref(),
            Some(d.path().canonicalize().unwrap().join("src").as_path())
        );
        assert_eq!(app.search.field, crate::commands::search::Field::Query);
        assert!(!app.search.replace_mode);
        // The scope shows the folder.
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        t.draw(|f| crate::ui::render(f, &mut app)).unwrap();
        let a = app.layout.find_bar;
        let buf = t.backend().buffer();
        let row0: String = (a.x..a.x + a.width)
            .map(|x| buf[(x, a.y)].symbol().to_string())
            .collect();
        assert!(row0.contains("src"), "{row0}");
        type_text(&mut app, "needle");
        key(&mut app, KeyCode::Enter);
        wait_search(&mut app);
        assert_eq!(
            found(&app),
            ["src/a.txt", "src/sub/b.txt"],
            "mouse={by_mouse}"
        );
    }
}

#[test]
fn replace_in_folder_previews_and_writes_only_inside_the_folder() {
    let (d, mut app) = needle_project();
    choose_with_mouse(&mut app, "src", REPLACE_IN);
    assert_eq!(app.focus, Focus::Search);
    assert!(app.search.replace_mode);
    assert_eq!(app.search.field, crate::commands::search::Field::Query);
    type_text(&mut app, "needle");
    key(&mut app, KeyCode::Tab);
    type_text(&mut app, "pin");
    key(&mut app, KeyCode::Enter);
    wait_search(&mut app);
    let p = app.search.preview.as_ref().expect("preview");
    let mut names: Vec<String> = p.files.iter().map(|f| app.display_path(&f.path)).collect();
    names.sort();
    assert_eq!(names, ["src/a.txt", "src/sub/b.txt"]);
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Left);
    key(&mut app, KeyCode::Enter);
    wait_search(&mut app);
    let read = |f: &str| fs::read_to_string(d.path().join(f)).unwrap();
    assert_eq!(read("src/a.txt"), "a pin here\n");
    assert_eq!(read("src/sub/b.txt"), "a pin here\n");
    assert_eq!(read("other/c.txt"), "a needle here\n");
    assert_eq!(read("top.txt"), "a needle here\n");
}

#[test]
fn ctrl_f_leaves_the_folder_scope() {
    let (_d, mut app) = needle_project();
    // A file must be open for the box to switch to File scope.
    let top = app.workspace.paths()[0].join("top.txt");
    app.open_path(&top).unwrap();
    choose_with_mouse(&mut app, "src", SEARCH_IN);
    type_text(&mut app, "needle");
    key(&mut app, KeyCode::Enter);
    wait_search(&mut app);
    assert_eq!(found(&app).len(), 2);
    app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
    assert_eq!(app.find.scope, crate::commands::find::Scope::File);
    assert!(app.search.folder.is_none());
    app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));
    assert_eq!(app.find.scope, crate::commands::find::Scope::Project);
    wait_search(&mut app);
    assert_eq!(
        found(&app).len(),
        4,
        "whole project again: {:?}",
        found(&app)
    );
}

#[test]
fn folder_in_the_second_root_searches_only_that_folder() {
    let (d, mut app) = needle_project();
    let b = tempfile::tempdir().unwrap();
    let br = b.path().canonicalize().unwrap();
    for f in ["lib/x.txt", "doc/y.txt"] {
        fs::create_dir_all(br.join(f).parent().unwrap()).unwrap();
        fs::write(br.join(f), "needle\n").unwrap();
    }
    app.add_root(&br).unwrap();
    explorer::sync_roots(&mut app);
    draw(&mut app, 100, 30);
    choose_with_mouse(&mut app, "lib", SEARCH_IN);
    type_text(&mut app, "needle");
    key(&mut app, KeyCode::Enter);
    wait_search(&mut app);
    assert_eq!(app.search.groups.len(), 1);
    assert!(app.search.groups[0].path.ends_with("lib/x.txt"));
    assert!(app.search.groups[0].root.is_none());
    drop(d);
}

#[test]
fn a_second_folder_choice_replaces_the_first_and_drops_its_results() {
    let (_d, mut app) = needle_project();
    choose_with_mouse(&mut app, "src", SEARCH_IN);
    type_text(&mut app, "needle");
    key(&mut app, KeyCode::Enter);
    wait_search(&mut app);
    assert_eq!(found(&app).len(), 2);
    choose_with_mouse(&mut app, "other", SEARCH_IN);
    wait_search(&mut app);
    assert_eq!(found(&app), ["other/c.txt"]);
}

#[test]
fn a_result_outside_the_folder_is_refused_in_the_preview_and_never_written() {
    use std::sync::{Arc, atomic::AtomicBool, mpsc};
    let (d, mut app) = needle_project();
    let root = d.path().canonicalize().unwrap();
    choose_with_mouse(&mut app, "src", REPLACE_IN);
    type_text(&mut app, "needle");
    key(&mut app, KeyCode::Tab);
    type_text(&mut app, "pin");
    // Feed a match from a file outside the folder, as a buggy source could.
    let (tx, rx) = mpsc::channel();
    crate::commands::search::attach(&mut app, rx, Arc::new(AtomicBool::new(false)));
    let m = |p: &str| vr_search::ProjectMatch {
        path: root.join(p),
        line: 1,
        col: 3,
        line_text: "a needle here".into(),
        match_range: (2, 8),
    };
    tx.send(vr_search::SearchMsg::Batch(vec![
        m("src/a.txt"),
        m("other/c.txt"),
    ]))
    .unwrap();
    tx.send(vr_search::SearchMsg::Done {
        files: 2,
        matches: 2,
        elapsed: std::time::Duration::ZERO,
    })
    .unwrap();
    app.search.query = Some(app.search.query());
    wait_search(&mut app);
    key(&mut app, KeyCode::Enter);
    wait_search(&mut app);
    let p = app.search.preview.as_ref().expect("preview");
    let outside = p
        .files
        .iter()
        .find(|f| f.path.ends_with("other/c.txt"))
        .unwrap();
    assert!(outside.error.is_some(), "outside file must be refused");
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Left);
    key(&mut app, KeyCode::Enter);
    wait_search(&mut app);
    assert_eq!(
        fs::read_to_string(root.join("other/c.txt")).unwrap(),
        "a needle here\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("src/a.txt")).unwrap(),
        "a pin here\n"
    );
}

#[test]
fn replace_in_folder_gives_the_keys_to_the_search_field_even_with_a_kept_query() {
    let (_d, mut app) = needle_project();
    choose_with_mouse(&mut app, "src", SEARCH_IN);
    type_text(&mut app, "needle");
    key(&mut app, KeyCode::Esc);
    choose_with_mouse(&mut app, "other", REPLACE_IN);
    assert_eq!(app.search.field, crate::commands::search::Field::Query);
    assert!(app.search.replace_mode);
    assert_eq!(app.search.input.text(), "needle");
}

// FR-31 fix round 1.

fn menu_has_folder_items(app: &App) -> bool {
    items(app).iter().any(|(l, _, _)| l == SEARCH_IN)
}

#[test]
fn a_subfolder_under_a_symlinked_dir_is_not_a_folder_scope() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    let out = tempfile::tempdir().unwrap();
    let out_r = out.path().canonicalize().unwrap();
    fs::create_dir_all(out_r.join("sub")).unwrap();
    fs::write(out_r.join("sub/x.txt"), "needle\n").unwrap();
    std::os::unix::fs::symlink(&out_r, root.join("link")).unwrap();
    let (mut app, _) = test_app(&root);
    app.explorer.visible = true;
    explorer::ensure_loaded(&mut app);
    app.explorer.expand(&root.join("link")).unwrap();
    draw(&mut app, 100, 30);
    right_click_row(&mut app, "sub");
    assert!(app.explorer.current().unwrap().path.ends_with("link/sub"));
    assert!(
        !menu_has_folder_items(&app),
        "link/sub resolves outside the workspace"
    );
    key(&mut app, KeyCode::Esc);
    // The command refuses too.
    app.run_command("explorer.replace_folder", crate::commands::CmdArgs::None);
    assert!(app.search.folder.is_none());
    assert_eq!(
        fs::read_to_string(out_r.join("sub/x.txt")).unwrap(),
        "needle\n"
    );
}

#[test]
fn a_folder_outside_every_root_ends_the_scope_before_search_and_preview() {
    let (d, mut app) = needle_project();
    let root = d.path().canonicalize().unwrap();
    let out = tempfile::tempdir().unwrap();
    let out_r = out.path().canonicalize().unwrap();
    fs::write(out_r.join("x.txt"), "needle\n").unwrap();
    choose_with_mouse(&mut app, "src", REPLACE_IN);
    // The scope is moved outside the workspace behind the panel's back (root removed during
    // a replace, then the replace ended).
    app.search.folder = Some(out_r.clone());
    type_text(&mut app, "needle");
    key(&mut app, KeyCode::Enter);
    wait_search(&mut app);
    assert!(app.search.folder.is_none(), "scope ended");
    assert!(app.search.groups.is_empty(), "nothing searched outside");
    assert!(
        app.notices
            .iter()
            .any(|n| n.text.contains("outside the workspace")),
        "a notice says why"
    );
    assert_eq!(fs::read_to_string(out_r.join("x.txt")).unwrap(), "needle\n");
    assert_eq!(
        fs::read_to_string(root.join("src/a.txt")).unwrap(),
        "a needle here\n"
    );
}

#[test]
fn removing_the_root_of_the_folder_scope_ends_it() {
    let (_d, mut app) = needle_project();
    let b = tempfile::tempdir().unwrap();
    let br = b.path().canonicalize().unwrap();
    fs::create_dir_all(br.join("lib")).unwrap();
    fs::write(br.join("lib/x.txt"), "needle\n").unwrap();
    let id = app.add_root(&br).unwrap();
    explorer::sync_roots(&mut app);
    draw(&mut app, 100, 30);
    choose_with_mouse(&mut app, "lib", SEARCH_IN);
    assert!(app.search.folder.is_some());
    app.remove_root(id).unwrap();
    assert!(app.search.folder.is_none());
}

#[test]
fn esc_ends_the_folder_scope_so_the_next_project_search_is_whole() {
    let (_d, mut app) = needle_project();
    choose_with_mouse(&mut app, "src", SEARCH_IN);
    type_text(&mut app, "needle");
    key(&mut app, KeyCode::Enter);
    wait_search(&mut app);
    assert_eq!(found(&app).len(), 2);
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Esc);
    assert!(!app.find.open);
    assert!(app.search.folder.is_none());
    crate::app_tests::run(&mut app, "search.project");
    key(&mut app, KeyCode::Enter);
    wait_search(&mut app);
    assert_eq!(found(&app).len(), 4, "{:?}", found(&app));
}

#[test]
fn ctrl_f_through_the_editor_path_also_ends_the_folder_scope() {
    let (_d, mut app) = needle_project();
    let top = app.workspace.paths()[0].join("top.txt");
    app.open_path(&top).unwrap();
    choose_with_mouse(&mut app, "src", SEARCH_IN);
    type_text(&mut app, "needle");
    key(&mut app, KeyCode::Enter);
    wait_search(&mut app);
    // The box stays open but the editor has the keys: ctrl+f enters File scope from there.
    app.focus = Focus::Editor;
    let ctrl_f = KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL);
    app.handle_key(ctrl_f);
    assert_eq!(app.find.scope, crate::commands::find::Scope::File);
    assert!(app.search.folder.is_none());
    app.handle_key(ctrl_f);
    assert_eq!(app.find.scope, crate::commands::find::Scope::Project);
    wait_search(&mut app);
    assert_eq!(found(&app).len(), 4, "{:?}", found(&app));
}

#[test]
fn open_folder_is_refused_while_a_replace_writes() {
    let (d, mut app) = needle_project();
    let root = d.path().canonicalize().unwrap();
    choose_with_mouse(&mut app, "src", REPLACE_IN);
    type_text(&mut app, "needle");
    key(&mut app, KeyCode::Tab);
    type_text(&mut app, "pin");
    key(&mut app, KeyCode::Enter);
    wait_search(&mut app);
    assert!(app.search.preview.is_some());
    // Hold the apply worker so the replace is running.
    let hold = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    app.search.hold = Some(hold.clone());
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Left);
    key(&mut app, KeyCode::Enter);
    assert!(crate::commands::search::replacing(&app));
    let before = app.search.folder.clone();
    crate::commands::search::open_folder(&mut app, &root.join("other"), false);
    assert_eq!(
        app.search.folder, before,
        "scope unchanged during the replace"
    );
    hold.fetch_add(100, std::sync::atomic::Ordering::SeqCst);
    wait_search(&mut app);
    assert_eq!(
        fs::read_to_string(root.join("other/c.txt")).unwrap(),
        "a needle here\n"
    );
}

#[test]
fn changing_the_folder_drops_the_old_preview() {
    let (_d, mut app) = needle_project();
    choose_with_mouse(&mut app, "src", REPLACE_IN);
    type_text(&mut app, "needle");
    key(&mut app, KeyCode::Tab);
    type_text(&mut app, "pin");
    key(&mut app, KeyCode::Enter);
    wait_search(&mut app);
    assert!(app.search.preview.is_some());
    choose_with_mouse(&mut app, "other", SEARCH_IN);
    assert!(app.search.preview.is_none());
    wait_search(&mut app);
    assert!(app.search.preview.is_none());
}

#[test]
fn renaming_the_scoped_folder_moves_the_scope_with_it() {
    let (d, mut app) = needle_project();
    let root = d.path().canonicalize().unwrap();
    choose_with_mouse(&mut app, "src", SEARCH_IN);
    crate::commands::search::paths_moved(&mut app, &root.join("src"), &root.join("lib"));
    assert_eq!(
        app.search.folder.as_deref(),
        Some(root.join("lib").as_path())
    );
}

#[test]
fn narrow_boxes_keep_a_folder_marker_and_labels_cut_by_cell_width() {
    let (_d, mut app) = needle_project();
    choose_with_mouse(&mut app, "src", SEARCH_IN);
    let row0 = |app: &mut App, w: u16| {
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, 30)).unwrap();
        t.draw(|f| crate::ui::render(f, app)).unwrap();
        let a = app.layout.find_bar;
        let buf = t.backend().buffer();
        (a.x..a.x + a.width)
            .map(|x| buf[(x, a.y)].symbol().to_string())
            .collect::<String>()
    };
    let narrow = row0(&mut app, 30);
    assert!(narrow.contains("src"), "{narrow}");
    // Wide (2-cell) characters: the label never exceeds the budget in cells.
    app.search.folder = Some(app.workspace.paths()[0].join("日本語日本語日本語日本語"));
    let label = crate::commands::search::folder_label(&app, 10).unwrap();
    assert!(
        ratatui::text::Span::raw(label.as_str()).width() <= 10,
        "{label}"
    );
}
