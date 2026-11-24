//! T8: explorer panel, file operations through it, go-to-file and focus, driven through the
//! `App` API (no terminal).

use std::fs;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, Focus};
use crate::app_tests::test_app;
use crate::commands::explorer;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

/// A project: src/{Controller/Order.php, main.rs}, tests/, composer.json, README.md.
fn project() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    fs::create_dir_all(r.join("src/Controller")).unwrap();
    fs::create_dir(r.join("tests")).unwrap();
    fs::write(r.join("src/Controller/Order.php"), "<?php\n").unwrap();
    fs::write(r.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(r.join("composer.json"), "{}\n").unwrap();
    fs::write(r.join("README.md"), "# shop\n").unwrap();
    let (mut app, _) = test_app(r);
    app.explorer.visible = true;
    explorer::ensure_loaded(&mut app);
    (dir, app)
}

/// Rows as "  name" with two spaces per depth level.
fn rows(app: &App) -> Vec<String> {
    app.explorer
        .rows
        .iter()
        .map(|r| format!("{}{}", "  ".repeat(r.depth), r.name()))
        .collect()
}

fn root_name(dir: &Path) -> String {
    dir.file_name().unwrap().to_string_lossy().into_owned()
}

#[test]
fn explorer_shows_root_tree() {
    let (d, app) = project();
    assert_eq!(
        rows(&app),
        [
            root_name(d.path()),
            "  src".into(),
            "  tests".into(),
            "  composer.json".into(),
            "  README.md".into(),
        ]
    );
    assert_eq!(app.explorer.rows[0].path, d.path());
    assert_eq!(app.focus, Focus::Editor);
}

fn cursor_name(app: &App) -> String {
    app.explorer.current().unwrap().name()
}

fn draw(app: &mut App, w: u16, h: u16) {
    let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
    t.draw(|f| crate::ui::render(f, app)).unwrap();
}

/// Left click at the explorer row showing `name`.
fn click_row(app: &mut App, name: &str, mods: KeyModifiers) {
    let i = app
        .explorer
        .rows
        .iter()
        .position(|r| r.name() == name)
        .unwrap_or_else(|| panic!("no row {name}: {:?}", rows(app)));
    let a = app.explorer.area;
    let y = a.y + (i - app.explorer.top) as u16;
    for kind in [
        crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left),
    ] {
        app.handle_mouse(crossterm::event::MouseEvent {
            kind,
            column: a.x + 3,
            row: y,
            modifiers: mods,
        });
    }
}

#[test]
fn explorer_expand_collapse_keyboard_and_mouse() {
    let (_d, mut app) = project();
    app.run_command("explorer.focus", crate::commands::CmdArgs::None);
    assert_eq!(app.focus, Focus::Explorer);
    // Keys go to the explorer, never into a buffer.
    app.handle_key(key(KeyCode::Down));
    assert_eq!(cursor_name(&app), "src");
    app.handle_key(key(KeyCode::Right));
    assert!(
        rows(&app).contains(&"    main.rs".to_string()),
        "{:?}",
        rows(&app)
    );
    app.handle_key(key(KeyCode::Right));
    assert_eq!(cursor_name(&app), "Controller");
    app.handle_key(key(KeyCode::Enter));
    assert!(rows(&app).contains(&"      Order.php".to_string()));
    app.handle_key(key(KeyCode::Left));
    assert!(!rows(&app).contains(&"      Order.php".to_string()));
    app.handle_key(key(KeyCode::Left));
    assert_eq!(cursor_name(&app), "src");
    app.handle_key(key(KeyCode::Left));
    assert_eq!(rows(&app).len(), 5, "{:?}", rows(&app));
    app.handle_key(key(KeyCode::End));
    assert_eq!(cursor_name(&app), "README.md");
    app.handle_key(key(KeyCode::Home));
    assert_eq!(app.explorer.cursor, 0);
    app.handle_key(key(KeyCode::Char('x')));
    assert!(app.docs.is_empty());
    // Enter on a file opens it and gives the editor the keys.
    app.handle_key(key(KeyCode::End));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.active_name(), "README.md");
    assert_eq!(app.focus, Focus::Editor);

    // Mouse: a click on a folder toggles it, on a file opens it; the explorer gets focus.
    draw(&mut app, 80, 24);
    click_row(&mut app, "src", KeyModifiers::NONE);
    assert_eq!(app.focus, Focus::Explorer);
    assert!(rows(&app).contains(&"    main.rs".to_string()));
    draw(&mut app, 80, 24);
    click_row(&mut app, "main.rs", KeyModifiers::NONE);
    assert_eq!(app.active_name(), "src/main.rs");
    assert_eq!(cursor_name(&app), "main.rs");
    draw(&mut app, 80, 24);
    click_row(&mut app, "src", KeyModifiers::NONE);
    assert!(!rows(&app).contains(&"    main.rs".to_string()));
    // A click in the editor gives it focus back.
    let e = app.layout.editor;
    app.handle_mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: e.x + 2,
        row: e.y,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.focus, Focus::Editor);
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn typ(app: &mut App, s: &str) {
    for c in s.chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
}

fn prompt(app: &App) -> Option<(String, Option<String>)> {
    match &app.dialog.as_ref()?.widget {
        crate::ui::dialog::DialogWidget::Prompt(p) => {
            Some((p.input.text().to_string(), p.error.clone()))
        }
        crate::ui::dialog::DialogWidget::Confirm(_) => None,
    }
}

/// Replaces the prompt's text with `s` and presses Enter.
fn submit(app: &mut App, s: &str) {
    let (text, _) = prompt(app).expect("a prompt");
    app.handle_key(key(KeyCode::End));
    for _ in 0..text.chars().count() {
        app.handle_key(key(KeyCode::Backspace));
    }
    typ(app, s);
    app.handle_key(key(KeyCode::Enter));
}

/// Puts the explorer cursor on the row at `rel` (expanding its folders) with focus.
fn select(app: &mut App, rel: &str) {
    app.run_command("explorer.focus", crate::commands::CmdArgs::None);
    let target = app.root.join(rel);
    for a in target.ancestors().collect::<Vec<_>>().into_iter().rev() {
        if a.starts_with(&app.root) && a != target && !app.explorer.expanded.contains(a) {
            app.explorer.expand(a).unwrap();
        }
    }
    let i = app
        .explorer
        .rows
        .iter()
        .position(|r| r.path == target)
        .unwrap_or_else(|| panic!("no row {rel}: {:?}", rows(app)));
    app.explorer.cursor = i;
    app.explorer.selected.clear();
}

#[test]
fn explorer_new_file_opens_it() {
    let (d, mut app) = project();
    let r = d.path();
    select(&mut app, "src/main.rs");
    app.handle_key(ctrl('n'));
    assert!(prompt(&app).is_some(), "ctrl+n opens the name prompt");
    // Names with a slash, `..`, or an existing name are refused in the prompt.
    for bad in ["a/b.rs", "..", "main.rs"] {
        submit(&mut app, bad);
        let (_, err) = prompt(&app).unwrap_or_else(|| panic!("{bad}: prompt closed"));
        assert!(err.is_some(), "{bad}: no error shown");
    }
    assert_eq!(
        fs::read_to_string(r.join("src/main.rs")).unwrap(),
        "fn main() {}\n"
    );
    assert!(!r.join("a").exists() && !r.join("src/a").exists());
    submit(&mut app, "lib.rs");
    assert!(app.dialog.is_none());
    assert_eq!(fs::read_to_string(r.join("src/lib.rs")).unwrap(), "");
    assert_eq!(app.active_name(), "src/lib.rs");
    assert_eq!(app.focus, Focus::Editor);
    assert_eq!(cursor_name(&app), "lib.rs");

    // New folder in the folder under the cursor.
    select(&mut app, "tests");
    app.run_command("explorer.new_folder", crate::commands::CmdArgs::None);
    submit(&mut app, "unit");
    assert!(r.join("tests/unit").is_dir());
    assert_eq!(cursor_name(&app), "unit");
    assert_eq!(app.focus, Focus::Explorer);
}

fn notices(app: &App) -> String {
    app.notices
        .iter()
        .map(|n| n.text.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

fn tab_paths(app: &App) -> Vec<String> {
    app.docs
        .iter()
        .map(|d| app.display_path(d.path.as_deref().unwrap()))
        .collect()
}

#[test]
fn explorer_rename_open_file_updates_tab() {
    let (d, mut app) = project();
    let r = d.path().to_path_buf();
    app.open_path(Path::new("src/main.rs")).unwrap();
    typ(&mut app, "x");
    app.open_path(Path::new("README.md")).unwrap();
    select(&mut app, "src/main.rs");
    app.handle_key(key(KeyCode::F(2)));
    assert_eq!(
        prompt(&app).expect("f2 opens the rename prompt").0,
        "main.rs"
    );
    // An existing name is refused; nothing moves.
    submit(&mut app, "lib.rs.tmp/x");
    assert!(prompt(&app).unwrap().1.is_some());
    fs::write(r.join("src/taken.rs"), "taken\n").unwrap();
    submit(&mut app, "taken.rs");
    assert!(prompt(&app).unwrap().1.is_some());
    assert_eq!(
        fs::read_to_string(r.join("src/taken.rs")).unwrap(),
        "taken\n"
    );
    submit(&mut app, "app.rs");
    assert!(app.dialog.is_none());
    assert!(!r.join("src/main.rs").exists());
    assert_eq!(
        fs::read_to_string(r.join("src/app.rs")).unwrap(),
        "fn main() {}\n"
    );
    assert_eq!(tab_paths(&app), ["src/app.rs", "README.md"]);
    // The unsaved edit stays in the buffer and is saved under the new name.
    assert!(app.docs[0].is_modified());
    assert_eq!(app.docs[0].buffer.text().to_string(), "xfn main() {}\n");
    assert_eq!(cursor_name(&app), "app.rs");

    // Renaming a folder moves every tab under it.
    app.open_path(Path::new("src/Controller/Order.php"))
        .unwrap();
    select(&mut app, "src");
    app.handle_key(key(KeyCode::F(2)));
    submit(&mut app, "source");
    assert_eq!(
        tab_paths(&app),
        ["source/app.rs", "README.md", "source/Controller/Order.php"]
    );
    assert!(r.join("source/Controller/Order.php").is_file());
    app.set_active(0);
    app.run_command("file.save", crate::commands::CmdArgs::None);
    assert_eq!(
        fs::read_to_string(r.join("source/app.rs")).unwrap(),
        "xfn main() {}\n"
    );
    assert!(!r.join("src").exists());

    // The root is never renamed.
    select(&mut app, "");
    app.handle_key(key(KeyCode::F(2)));
    assert!(app.dialog.is_none());
    assert!(
        notices(&app).contains("project folder"),
        "{}",
        notices(&app)
    );
}

/// FakeTrash shared with the test after the app owns its box.
struct SharedTrash(std::rc::Rc<vr_fs::FakeTrash>);

impl vr_fs::Trash for SharedTrash {
    fn trash(&self, paths: &[PathBuf]) -> vr_fs::BatchResult {
        self.0.trash(paths)
    }
}

fn fake_trash(app: &mut App, holding: &Path) -> std::rc::Rc<vr_fs::FakeTrash> {
    let t = std::rc::Rc::new(vr_fs::FakeTrash::new(holding));
    app.trash = Box::new(SharedTrash(t.clone()));
    t
}

fn confirm(app: &App) -> Option<crate::ui::dialog::ConfirmDialog> {
    match &app.dialog.as_ref()?.widget {
        crate::ui::dialog::DialogWidget::Confirm(c) => Some(c.clone()),
        crate::ui::dialog::DialogWidget::Prompt(_) => None,
    }
}

#[test]
fn explorer_delete_confirms_then_trashes() {
    let (d, mut app) = project();
    let r = d.path().to_path_buf();
    let side = tempfile::tempdir().unwrap();
    let trash = fake_trash(&mut app, &side.path().join("trash"));
    app.recovery_dir = Some(side.path().join("recovery"));
    app.open_path(Path::new("README.md")).unwrap();
    typ(&mut app, "unsaved ");
    app.open_path(Path::new("composer.json")).unwrap();
    app.open_path(Path::new("src/main.rs")).unwrap();

    // src (expanded) down to README.md: src/Controller and src/main.rs sit under src.
    select(&mut app, "src/main.rs");
    select(&mut app, "src");
    app.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::SHIFT));
    assert_eq!(
        app.explorer.selected.len(),
        6,
        "{:?}",
        app.explorer.selected
    );
    app.handle_key(key(KeyCode::Delete));
    let c = confirm(&app).expect("delete asks first (D9)");
    assert_eq!(c.buttons, ["Move to Trash", "Cancel"]);
    assert_eq!(
        c.buttons[c.selected], "Cancel",
        "the default button is Cancel"
    );
    assert!(c.message.contains("README.md"), "{}", c.message);
    assert!(c.message.contains("recovery"), "{}", c.message);
    app.handle_key(key(KeyCode::Enter));
    assert!(trash.trashed().is_empty());
    assert!(r.join("README.md").exists());

    app.handle_key(key(KeyCode::Delete));
    app.handle_key(key(KeyCode::Left));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(
        trash.trashed(),
        [
            r.join("src"),
            r.join("tests"),
            r.join("composer.json"),
            r.join("README.md")
        ],
        "nested paths are dropped before the batch"
    );
    assert!(!r.join("src").exists() && !r.join("README.md").exists());
    // The unsaved buffer went to the recovery folder first; trashed files' tabs are closed.
    let rec: Vec<String> = fs::read_dir(side.path().join("recovery"))
        .unwrap()
        .map(|e| fs::read_to_string(e.unwrap().path()).unwrap())
        .collect();
    assert_eq!(rec, ["unsaved # shop\n"]);
    assert!(app.docs.is_empty(), "{:?}", tab_paths(&app));
    assert_eq!(rows(&app).len(), 1, "{:?}", rows(&app));

    // The root is never trashed.
    select(&mut app, "");
    app.handle_key(key(KeyCode::Delete));
    assert!(app.dialog.is_none());
    assert!(notices(&app).contains("project folder"));

    // Per-path results: one item vanished before the confirmation, the other is trashed.
    fs::write(r.join("a.txt"), "a\n").unwrap();
    fs::write(r.join("b.txt"), "b\n").unwrap();
    app.explorer.reload_all();
    select(&mut app, "a.txt");
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT));
    app.handle_key(key(KeyCode::Delete));
    fs::remove_file(r.join("a.txt")).unwrap();
    app.handle_key(key(KeyCode::Left));
    app.handle_key(key(KeyCode::Enter));
    assert!(!r.join("b.txt").exists());
    let n = notices(&app);
    assert!(n.contains("a.txt") && n.contains("does not exist"), "{n}");
}

#[test]
fn explorer_copy_paste_and_move() {
    let (d, mut app) = project();
    let r = d.path().to_path_buf();
    select(&mut app, "src/main.rs");
    app.handle_key(ctrl('c'));
    select(&mut app, "tests");
    app.handle_key(ctrl('v'));
    assert_eq!(
        fs::read_to_string(r.join("tests/main.rs")).unwrap(),
        "fn main() {}\n"
    );
    // Pasting where the name exists never replaces it: a free "copy" name is used.
    app.handle_key(ctrl('v'));
    assert!(r.join("tests/main copy.rs").is_file());
    app.handle_key(ctrl('v'));
    assert!(r.join("tests/main copy 2.rs").is_file());
    assert_eq!(
        fs::read_to_string(r.join("tests/main.rs")).unwrap(),
        "fn main() {}\n"
    );
    // A folder copies with everything in it; into itself is refused with its name.
    select(&mut app, "src");
    app.handle_key(ctrl('c'));
    select(&mut app, "tests");
    app.handle_key(ctrl('v'));
    assert!(r.join("tests/src/Controller/Order.php").is_file());
    select(&mut app, "src/Controller");
    app.handle_key(ctrl('v'));
    assert!(notices(&app).contains("into itself"), "{}", notices(&app));
    assert!(!r.join("src/Controller/src").exists());

    // Move: README.md (open in a tab) and composer.json into tests/.
    app.open_path(Path::new("README.md")).unwrap();
    app.active_doc_mut().unwrap().buffer.insert_text("unsaved ");
    select(&mut app, "composer.json");
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT));
    app.run_command("explorer.move", crate::commands::CmdArgs::None);
    submit(&mut app, "no/such/dir");
    assert!(prompt(&app).unwrap().1.is_some());
    submit(&mut app, "../outside");
    assert!(
        prompt(&app)
            .unwrap()
            .1
            .unwrap()
            .contains("outside the project")
    );
    submit(&mut app, "tests");
    assert!(app.dialog.is_none());
    assert!(r.join("tests/README.md").is_file() && r.join("tests/composer.json").is_file());
    assert!(!r.join("README.md").exists());
    assert_eq!(tab_paths(&app), ["tests/README.md"]);
    assert!(app.docs[0].is_modified(), "a move keeps the unsaved buffer");
    assert_eq!(app.docs[0].buffer.text().to_string(), "unsaved # shop\n");
    // A name taken in the destination: that item stays, the error names it.
    fs::write(r.join("main.rs"), "top\n").unwrap();
    app.explorer.reload_all();
    select(&mut app, "main.rs");
    app.run_command("explorer.move", crate::commands::CmdArgs::None);
    submit(&mut app, "tests");
    assert_eq!(fs::read_to_string(r.join("main.rs")).unwrap(), "top\n");
    assert_eq!(
        fs::read_to_string(r.join("tests/main.rs")).unwrap(),
        "fn main() {}\n"
    );
    assert!(
        notices(&app).contains("tests/main.rs already exists"),
        "{}",
        notices(&app)
    );
    // The root is never moved.
    select(&mut app, "");
    app.run_command("explorer.move", crate::commands::CmdArgs::None);
    assert!(app.dialog.is_none());
}

#[test]
fn explorer_reveal_active_file() {
    let (_d, mut app) = project();
    app.open_path(Path::new("src/Controller/Order.php"))
        .unwrap();
    app.explorer.visible = false;
    app.run_command("explorer.reveal", crate::commands::CmdArgs::None);
    assert!(app.explorer.visible);
    assert_eq!(app.focus, Focus::Explorer);
    assert_eq!(cursor_name(&app), "Order.php");
    assert!(rows(&app).contains(&"      Order.php".to_string()));
}

#[test]
fn explorer_toggle_ignored() {
    let (d, mut app) = project();
    fs::write(d.path().join(".gitignore"), "target/\n*.log\n").unwrap();
    fs::create_dir(d.path().join("target")).unwrap();
    fs::write(d.path().join("debug.log"), "x\n").unwrap();
    app.explorer.reload_all();
    assert!(
        !rows(&app)
            .iter()
            .any(|r| r.contains("target") || r.contains("debug.log"))
    );
    app.run_command("explorer.toggle_ignored", crate::commands::CmdArgs::None);
    let shown: Vec<_> = app
        .explorer
        .rows
        .iter()
        .filter(|r| r.ignored)
        .map(|r| r.name())
        .collect();
    assert_eq!(shown, ["target", "debug.log"]);
    app.run_command("explorer.toggle_ignored", crate::commands::CmdArgs::None);
    assert!(!rows(&app).iter().any(|r| r.contains("target")));
    // Dotfiles follow explorer.toggle_hidden (shown by default).
    assert!(rows(&app).contains(&"  .gitignore".to_string()));
    app.run_command("explorer.toggle_hidden", crate::commands::CmdArgs::None);
    assert!(!rows(&app).contains(&"  .gitignore".to_string()));
}

#[test]
fn explorer_refreshes_on_fs_events() {
    use vr_core::FsEvent;
    let (d, mut app) = project();
    let r = d.path().to_path_buf();
    select(&mut app, "src/Controller/Order.php");
    select(&mut app, "tests");
    app.explorer.expand(&r.join("tests")).unwrap();
    select(&mut app, "src/main.rs");
    app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::SHIFT));
    let selected = app.explorer.selected.clone();
    assert_eq!(selected.len(), 2);

    // Created outside the editor (an AI agent): appears; expansion and selection are kept.
    fs::write(r.join("src/added.rs"), "").unwrap();
    app.handle_fs_event(FsEvent::Created(r.join("src/added.rs")));
    assert!(
        rows(&app).contains(&"    added.rs".to_string()),
        "{:?}",
        rows(&app)
    );
    assert!(rows(&app).contains(&"      Order.php".to_string()));
    assert_eq!(app.explorer.selected, selected);
    assert_eq!(cursor_name(&app), "tests");

    fs::rename(r.join("src/added.rs"), r.join("src/renamed.rs")).unwrap();
    app.handle_fs_event(FsEvent::Renamed(
        r.join("src/added.rs"),
        r.join("src/renamed.rs"),
    ));
    assert!(rows(&app).contains(&"    renamed.rs".to_string()));
    assert!(!rows(&app).contains(&"    added.rs".to_string()));
    fs::remove_file(r.join("src/renamed.rs")).unwrap();
    app.handle_fs_event(FsEvent::Removed(r.join("src/renamed.rs")));
    assert!(!rows(&app).contains(&"    renamed.rs".to_string()));

    // Lost events (queue overflow): Rescan re-reads every expanded folder under it.
    fs::write(r.join("tests/t1.rs"), "").unwrap();
    fs::write(r.join("src/Controller/User.php"), "").unwrap();
    app.handle_fs_event(FsEvent::Rescan(r.clone()));
    assert!(
        rows(&app).contains(&"    t1.rs".to_string()),
        "{:?}",
        rows(&app)
    );
    assert!(rows(&app).contains(&"      User.php".to_string()));
    assert_eq!(app.explorer.selected, selected);

    // A folder removed outside: its rows go, the cursor stays in place.
    fs::remove_dir_all(r.join("src")).unwrap();
    app.handle_fs_event(FsEvent::Removed(r.join("src")));
    assert_eq!(
        rows(&app),
        [
            root_name(&r),
            "  tests".into(),
            "    t1.rs".into(),
            "  composer.json".into(),
            "  README.md".into()
        ]
    );
}

#[test]
fn explorer_follows_the_real_watcher() {
    let (d, mut app) = project();
    app.start_watcher();
    app.watch_root();
    let added = d.path().join("from_agent.txt");
    fs::write(&added, "x\n").unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !rows(&app).contains(&"  from_agent.txt".to_string()) {
        assert!(std::time::Instant::now() < deadline, "{:?}", rows(&app));
        std::thread::sleep(std::time::Duration::from_millis(20));
        app.drain_fs_events();
    }
}

#[test]
fn focus_cycle_editor_explorer() {
    let (_d, mut app) = project();
    app.keyboard_enhanced = true;
    app.open_path(Path::new("README.md")).unwrap();
    // f6 (focus.cycle): editor → explorer → editor.
    app.handle_key(key(KeyCode::F(6)));
    assert_eq!(app.focus, Focus::Explorer);
    app.handle_key(key(KeyCode::F(6)));
    assert_eq!(app.focus, Focus::Editor);
    // ctrl+shift+e focuses the explorer, ctrl+b hides it and the editor gets the keys back.
    app.handle_key(KeyEvent::new(
        KeyCode::Char('E'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    assert_eq!(app.focus, Focus::Explorer);
    app.handle_key(ctrl('b'));
    assert!(!app.explorer.visible);
    assert_eq!(app.focus, Focus::Editor);
    // Hidden: focus.cycle stays in the editor; ctrl+b shows the panel again.
    app.handle_key(key(KeyCode::F(6)));
    assert_eq!(app.focus, Focus::Editor);
    app.handle_key(ctrl('b'));
    assert!(app.explorer.visible);
    // Esc in the explorer returns to the editor.
    app.handle_key(key(KeyCode::F(6)));
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.focus, Focus::Editor);
}

/// Drains index messages until the go-to-file index is built (5 s at most).
fn wait_index(app: &mut App) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while app.index_rx.is_some() {
        assert!(std::time::Instant::now() < deadline, "index never finished");
        std::thread::sleep(std::time::Duration::from_millis(5));
        app.drain_index();
    }
}

fn items(app: &App) -> Vec<String> {
    app.palette
        .as_ref()
        .unwrap()
        .items
        .iter()
        .map(|i| i.title.clone())
        .collect()
}

#[test]
fn goto_file_opens_selected() {
    let (d, mut app) = project();
    fs::write(d.path().join(".gitignore"), "target/\n").unwrap();
    fs::create_dir(d.path().join("target")).unwrap();
    fs::write(d.path().join("target/OrderGen.php"), "").unwrap();
    app.handle_key(ctrl('p'));
    assert_eq!(app.focus, Focus::Palette);
    wait_index(&mut app);
    typ(&mut app, "order");
    let p = app.palette.as_ref().unwrap();
    assert_eq!(
        p.items[0].path.as_deref(),
        Some(Path::new("src/Controller/Order.php"))
    );
    assert_eq!(
        items(&app),
        ["Order.php"],
        "gitignored files are not offered (AC-9)"
    );
    app.handle_key(key(KeyCode::Enter));
    assert!(app.palette.is_none());
    assert_eq!(app.active_name(), "src/Controller/Order.php");
    assert_eq!(app.focus, Focus::Editor);

    // A file created later is found once the watcher reported it.
    fs::write(d.path().join("src/OrderService.php"), "").unwrap();
    app.handle_fs_event(vr_core::FsEvent::Created(
        d.path().join("src/OrderService.php"),
    ));
    app.handle_key(ctrl('p'));
    wait_index(&mut app);
    typ(&mut app, "ordersvc");
    assert_eq!(
        items(&app).first().map(String::as_str),
        Some("OrderService.php")
    );
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.focus, Focus::Editor);
}

#[test]
fn startup_lists_leftover_temp_files_and_keeps_them() {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    fs::create_dir(r.join("src")).unwrap();
    let leftovers = [
        r.join(".vr-ide-save-4242-0.tmp"),
        r.join("a.txt.vr-ide-conflict-4242-1"),
        r.join("src/.main.rs.vr-tmp-4242-99-0"),
    ];
    for p in &leftovers {
        fs::write(p, "kept\n").unwrap();
    }
    fs::write(r.join("a.txt"), "a\n").unwrap();
    let (mut app, _) = test_app(r);
    app.explorer.visible = true;
    explorer::ensure_loaded(&mut app);
    let n = notices(&app);
    assert!(n.contains(".vr-ide-save-4242-0.tmp"), "{n}");
    assert!(n.contains("a.txt.vr-ide-conflict-4242-1"), "{n}");
    assert!(!n.contains("vr-tmp"), "src is not expanded yet: {n}");
    app.notices.clear();
    select(&mut app, "src/.main.rs.vr-tmp-4242-99-0");
    let src = app.root.join("src");
    app.explorer.collapse(&src);
    app.run_command("explorer.focus", crate::commands::CmdArgs::None);
    let i = app
        .explorer
        .rows
        .iter()
        .position(|row| row.path == src)
        .unwrap();
    app.explorer.cursor = i;
    app.handle_key(key(KeyCode::Right));
    assert!(
        notices(&app).contains("src/.main.rs.vr-tmp-4242-99-0"),
        "{}",
        notices(&app)
    );
    for p in &leftovers {
        assert_eq!(
            fs::read_to_string(p).unwrap(),
            "kept\n",
            "{} was touched",
            p.display()
        );
    }
}

#[test]
fn operation_errors_name_their_paths() {
    use vr_fs::FsOpError as E;
    let (d, app) = project();
    let p = |s: &str| d.path().join(s);
    let io = || std::io::Error::other("EPERM");
    let cases = [
        (
            E::CrossDevice {
                from: p("a.txt"),
                to: p("mnt/a.txt"),
            },
            &[
                "a.txt",
                "mnt/a.txt",
                "different file systems",
                "nothing was moved",
                "copy",
            ][..],
        ),
        (
            E::PartialCopy {
                to: p("dst"),
                source: Box::new(E::Io {
                    path: p("dst/x"),
                    source: io(),
                }),
            },
            &["dst", "incomplete", "dst/x", "original is untouched"][..],
        ),
        (
            E::BothNamesLeft {
                from: p("a.txt"),
                to: p("b.txt"),
                source: io(),
            },
            &["a.txt", "b.txt", "both names", "nothing was deleted"][..],
        ),
        (
            E::LeftAt {
                path: p(".a.txt.vr-tmp-1-2-3"),
                intended: p("A.txt"),
                source: Box::new(E::AlreadyExists(p("A.txt"))),
            },
            &[".a.txt.vr-tmp-1-2-3", "A.txt", "safe"][..],
        ),
        (
            E::TrashRefused {
                path: p("link"),
                reason: "another device".into(),
            },
            &["link", "not moved to the trash", "another device"][..],
        ),
        (
            E::AlreadyExists(p("src/x.rs")),
            &["src/x.rs", "already exists", "nothing was replaced"][..],
        ),
    ];
    for (e, want) in cases {
        let msg = explorer::describe(&app, &e);
        for w in want {
            assert!(msg.contains(w), "{msg:?} lacks {w:?}");
        }
        assert!(
            !msg.contains(&d.path().display().to_string()),
            "absolute path in {msg:?}"
        );
    }
}

#[test]
fn trash_of_unsaved_file_stops_when_recovery_fails() {
    let (d, mut app) = project();
    let side = tempfile::tempdir().unwrap();
    let trash = fake_trash(&mut app, &side.path().join("trash"));
    // A file where the recovery folder should be: writing there fails.
    fs::write(side.path().join("blocked"), "").unwrap();
    app.recovery_dir = Some(side.path().join("blocked/recovery"));
    app.open_path(Path::new("README.md")).unwrap();
    typ(&mut app, "mine ");
    select(&mut app, "README.md");
    app.handle_key(key(KeyCode::Delete));
    app.handle_key(key(KeyCode::Left));
    app.handle_key(key(KeyCode::Enter));
    assert!(trash.trashed().is_empty());
    assert!(d.path().join("README.md").exists());
    assert_eq!(app.docs.len(), 1);
    assert!(app.docs[0].is_modified());
    assert!(
        notices(&app).contains("nothing was discarded"),
        "{}",
        notices(&app)
    );
}

/// Rename (f2) or move (explorer.move) through the explorer while a writer keeps rewriting
/// the file: no version the writer completed may be lost (`assert_versions_survive`), and an
/// open tab follows the file.
fn race_explorer(op: &str) {
    use vr_core::testutil::{RacingWriter, WriterMode, assert_versions_survive, sweep_offsets};
    const INITIAL: &[u8] = b"initial\n";
    let step = std::time::Duration::from_micros(25);
    for mode in [WriterMode::InPlace, WriterMode::TempRename] {
        for offset in sweep_offsets(20, step) {
            let dir = tempfile::tempdir().unwrap();
            let r = dir.path();
            fs::create_dir(r.join("dest")).unwrap();
            let src = r.join("src.txt");
            fs::write(&src, INITIAL).unwrap();
            let (mut app, _) = test_app(r);
            app.explorer.visible = true;
            explorer::ensure_loaded(&mut app);
            app.open_path(&src).unwrap();
            select(&mut app, "src.txt");
            let to = if op == "rename" {
                app.handle_key(key(KeyCode::F(2)));
                r.join("renamed.txt")
            } else if op == "cut" {
                app.run_command("explorer.cut", crate::commands::CmdArgs::None);
                select(&mut app, "dest");
                r.join("dest/src.txt")
            } else {
                app.run_command("explorer.move", crate::commands::CmdArgs::None);
                r.join("dest/src.txt")
            };
            if op != "cut" {
                let typed = if op == "rename" {
                    "renamed.txt"
                } else {
                    "dest"
                };
                let (text, _) = prompt(&app).unwrap();
                app.handle_key(key(KeyCode::End));
                for _ in 0..text.chars().count() {
                    app.handle_key(key(KeyCode::Backspace));
                }
                typ(&mut app, typed);
            }
            let writer = RacingWriter::start_after(
                offset,
                &src,
                mode,
                std::time::Duration::from_micros(200),
                12,
            );
            if op == "cut" {
                app.run_command("explorer.paste", crate::commands::CmdArgs::None);
            } else {
                app.handle_key(key(KeyCode::Enter));
            }
            let versions = writer.stop();
            assert!(app.dialog.is_none(), "{op}: {}", notices(&app));
            let found: Vec<Vec<u8>> = [&src, &to]
                .iter()
                .filter_map(|p| fs::read(p).ok())
                .collect();
            assert_versions_survive(INITIAL, &found, &versions);
            assert_eq!(
                app.docs[0].path.as_deref(),
                Some(to.as_path()),
                "the tab follows"
            );
        }
    }
}

#[test]
fn explorer_rename_never_loses_a_concurrent_write() {
    race_explorer("rename");
}

#[test]
fn explorer_move_never_loses_a_concurrent_write() {
    race_explorer("move");
}

#[test]
fn paste_into_own_parent_makes_a_copy_name() {
    let (d, mut app) = project();
    let r = d.path().to_path_buf();
    // A folder pasted next to itself (target = the project root) gets "src copy".
    select(&mut app, "src");
    app.handle_key(ctrl('c'));
    select(&mut app, "README.md");
    app.handle_key(ctrl('v'));
    assert!(
        r.join("src copy/Controller/Order.php").is_file(),
        "{}",
        notices(&app)
    );
    // The paste selected "src copy" (a folder); paste into the root again.
    select(&mut app, "README.md");
    app.handle_key(ctrl('v'));
    assert!(r.join("src copy 2/main.rs").is_file(), "{}", notices(&app));
    // A file pasted into its own folder.
    select(&mut app, "README.md");
    app.handle_key(ctrl('c'));
    app.handle_key(ctrl('v'));
    assert_eq!(
        fs::read_to_string(r.join("README copy.md")).unwrap(),
        "# shop\n"
    );
    // The originals are untouched.
    assert_eq!(fs::read_to_string(r.join("README.md")).unwrap(), "# shop\n");
    assert_eq!(
        fs::read_to_string(r.join("src/main.rs")).unwrap(),
        "fn main() {}\n"
    );
}

fn wheel(app: &mut App, down: bool) {
    let a = app.explorer.area;
    app.handle_mouse(crossterm::event::MouseEvent {
        kind: if down {
            crossterm::event::MouseEventKind::ScrollDown
        } else {
            crossterm::event::MouseEventKind::ScrollUp
        },
        column: a.x + 2,
        row: a.y + 1,
        modifiers: KeyModifiers::NONE,
    });
}

fn click_at(app: &mut App, row_y: u16) {
    let a = app.explorer.area;
    app.handle_mouse(crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
        column: a.x + 3,
        row: a.y + row_y,
        modifiers: KeyModifiers::NONE,
    });
}

#[test]
fn explorer_wheel_scrolls_freely_and_clicks_hit_drawn_rows() {
    let (d, mut app) = project();
    for i in 0..60 {
        fs::write(d.path().join(format!("f{i:02}.txt")), "").unwrap();
    }
    app.explorer.reload_all();
    app.run_command("explorer.focus", crate::commands::CmdArgs::None);
    app.handle_key(key(KeyCode::Down));
    draw(&mut app, 80, 24);
    // The wheel scrolls past the cursor (row 1) and the view stays there after a redraw.
    for _ in 0..4 {
        wheel(&mut app, true);
    }
    draw(&mut app, 80, 24);
    assert_eq!(app.explorer.cursor, 1);
    assert_eq!(
        app.explorer.top, 12,
        "the redraw pulled the view back to the cursor"
    );
    // A click picks the row drawn under it.
    let want = app.explorer.rows[12 + 2].name();
    click_at(&mut app, 2);
    assert_eq!(cursor_name(&app), want);
    // A wheel tick handled before the next frame: the click still hits what is on screen.
    draw(&mut app, 80, 24);
    let top = app.explorer.top;
    let want = app.explorer.rows[top + 3].name();
    wheel(&mut app, true);
    click_at(&mut app, 3);
    assert_eq!(
        cursor_name(&app),
        want,
        "click hit a row that was not drawn there"
    );
    // Keys bring the cursor back into view.
    draw(&mut app, 80, 24);
    for _ in 0..3 {
        wheel(&mut app, false);
    }
    app.handle_key(key(KeyCode::Down));
    draw(&mut app, 80, 24);
    let ex = &app.explorer;
    assert!(
        ex.cursor >= ex.top && ex.cursor < ex.top + usize::from(ex.area.height),
        "cursor {} not in view from {}",
        ex.cursor,
        ex.top
    );
}

#[test]
fn user_explorer_bindings_of_plain_keys_win_over_built_ins() {
    let (d, mut app) = project();
    let km = d.path().join("keymap.toml");
    fs::write(
        &km,
        "[explorer]\n\"space\" = \"explorer.toggle_ignored\"\n\"backspace\" = \"explorer.toggle_hidden\"\n",
    )
    .unwrap();
    let (keymap, warnings) = vr_config::Keymap::load(Some(&km));
    assert!(warnings.is_empty(), "{warnings:?}");
    app.keymap = keymap;
    app.run_command("explorer.focus", crate::commands::CmdArgs::None);
    let ignored = app.explorer.tree.show_ignored();
    let hidden = app.explorer.tree.show_hidden();
    app.handle_key(key(KeyCode::Char(' ')));
    app.handle_key(key(KeyCode::Backspace));
    assert_eq!(
        app.explorer.tree.show_ignored(),
        !ignored,
        "space binding never ran"
    );
    assert_eq!(
        app.explorer.tree.show_hidden(),
        !hidden,
        "backspace binding never ran"
    );
    // Unbound typing is still swallowed, and navigation keys stay built in.
    app.handle_key(key(KeyCode::Char('q')));
    assert!(app.docs.is_empty());
    app.handle_key(key(KeyCode::Down));
    assert_eq!(app.explorer.cursor, 1);
}

#[test]
fn explorer_cut_then_paste_moves() {
    let (d, mut app) = project();
    let r = d.path().to_path_buf();
    assert!(crate::commands::find("explorer.cut").is_some());
    // ctrl+x is the default explorer key (D108).
    app.open_path(Path::new("README.md")).unwrap();
    app.active_doc_mut().unwrap().buffer.insert_text("unsaved ");
    select(&mut app, "README.md");
    app.handle_key(ctrl('x'));
    assert!(r.join("README.md").is_file(), "cut alone moves nothing");
    select(&mut app, "tests");
    app.handle_key(ctrl('v'));
    assert!(!r.join("README.md").exists(), "{}", notices(&app));
    assert_eq!(
        fs::read_to_string(r.join("tests/README.md")).unwrap(),
        "# shop\n"
    );
    assert_eq!(tab_paths(&app), ["tests/README.md"]);
    assert!(app.docs[0].is_modified(), "the tab keeps its unsaved edit");
    // A cut is pasted once: the paths are gone from where they were.
    app.handle_key(ctrl('v'));
    assert!(!r.join("tests/README copy.md").exists());
    assert!(!r.join("tests/tests").exists());
    // Pasting into the folder it is already in leaves it there.
    select(&mut app, "src/main.rs");
    app.handle_key(ctrl('x'));
    select(&mut app, "src");
    app.handle_key(ctrl('v'));
    assert_eq!(
        fs::read_to_string(r.join("src/main.rs")).unwrap(),
        "fn main() {}\n"
    );
    // A name taken in the destination is never replaced.
    fs::write(r.join("tests/main.rs"), "other\n").unwrap();
    select(&mut app, "src/main.rs");
    app.handle_key(ctrl('x'));
    select(&mut app, "tests");
    app.handle_key(ctrl('v'));
    assert_eq!(
        fs::read_to_string(r.join("tests/main.rs")).unwrap(),
        "other\n"
    );
    assert!(r.join("src/main.rs").is_file());
    assert!(
        notices(&app).contains("already exists"),
        "{}",
        notices(&app)
    );
    // A folder into itself is refused; the root is never cut.
    select(&mut app, "src");
    app.handle_key(ctrl('x'));
    select(&mut app, "src/Controller");
    app.handle_key(ctrl('v'));
    assert!(r.join("src/Controller/Order.php").is_file());
    assert!(notices(&app).contains("into itself"), "{}", notices(&app));
    select(&mut app, "");
    app.handle_key(ctrl('x'));
    assert!(app.explorer.copied.is_empty());
}

#[test]
fn goto_file_keeps_selection_when_index_arrives() {
    let (d, mut app) = project();
    app.handle_key(ctrl('p'));
    wait_index(&mut app);
    app.handle_key(key(KeyCode::Esc));
    // A create makes the index stale: the next ctrl+p rebuilds it while the old one lists.
    fs::write(d.path().join("src/Other.rs"), "").unwrap();
    app.handle_fs_event(vr_core::FsEvent::Created(d.path().join("src/Other.rs")));
    app.handle_key(ctrl('p'));
    assert!(app.index_rx.is_some(), "a rebuild is running");
    typ(&mut app, "r");
    app.handle_key(key(KeyCode::Down));
    let p = app.palette.as_ref().unwrap();
    assert!(p.selected > 0, "{:?}", items(&app));
    let chosen = p.items[p.selected].path.clone();
    wait_index(&mut app);
    let p = app.palette.as_ref().unwrap();
    assert_eq!(p.items[p.selected].path, chosen, "{:?}", items(&app));
}

#[test]
fn drop_nested_keeps_order_and_scales() {
    let p = |s: &str| PathBuf::from(s);
    // Shift+End over a big expanded tree: one folder and every row below it.
    let mut many = vec![p("/r/big")];
    many.extend((0..20_000).map(|i| PathBuf::from(format!("/r/big/d{}/f{i}.txt", i % 100))));
    many.extend((0..20_000).map(|i| PathBuf::from(format!("/r/other{i}.txt"))));
    let t = std::time::Instant::now();
    let kept = explorer::drop_nested(many);
    assert!(
        t.elapsed() < std::time::Duration::from_secs(1),
        "{:?}",
        t.elapsed()
    );
    assert_eq!(kept.len(), 20_001);
    // Order is kept; a path listed twice is acted on once.
    assert_eq!(
        explorer::drop_nested(vec![
            p("/r/src/main.rs"),
            p("/r/b"),
            p("/r/src"),
            p("/r/a b"),
            p("/r/a"),
            p("/r/a/x"),
            p("/r/b"),
        ]),
        [p("/r/b"), p("/r/src"), p("/r/a b"), p("/r/a")]
    );
}

#[test]
fn explorer_watches_only_shown_folders() {
    let (d, mut app) = project();
    let r = d.path().to_path_buf();
    app.start_watcher();
    app.watch_root();
    let set = |v: &[&str]| {
        v.iter()
            .map(|s| if s.is_empty() { r.clone() } else { r.join(s) })
            .collect::<std::collections::HashSet<PathBuf>>()
    };
    assert_eq!(
        app.explorer.watched,
        set(&[""]),
        "the root only, not recursive"
    );
    app.run_command("explorer.focus", crate::commands::CmdArgs::None);
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Right));
    assert_eq!(cursor_name(&app), "src");
    assert_eq!(app.explorer.watched, set(&["", "src"]));
    // The expanded folder's own watch reports a create in it.
    fs::write(r.join("src/new.rs"), "").unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !rows(&app).contains(&"    new.rs".to_string()) {
        assert!(std::time::Instant::now() < deadline, "{:?}", rows(&app));
        std::thread::sleep(std::time::Duration::from_millis(20));
        app.drain_fs_events();
    }
    // Collapsing ends the watch of the folder and of folders below it.
    app.handle_key(key(KeyCode::Right));
    app.handle_key(key(KeyCode::Right));
    assert_eq!(cursor_name(&app), "Controller");
    assert_eq!(app.explorer.watched, set(&["", "src", "src/Controller"]));
    app.handle_key(key(KeyCode::Up));
    app.handle_key(key(KeyCode::Left));
    assert_eq!(cursor_name(&app), "src");
    assert_eq!(app.explorer.watched, set(&[""]));
}

#[test]
fn goto_file_rebuilds_its_index_each_time_it_opens() {
    let (d, mut app) = project();
    app.handle_key(ctrl('p'));
    wait_index(&mut app);
    app.handle_key(key(KeyCode::Esc));
    // A file made where no folder is watched (nothing reported it).
    fs::create_dir_all(d.path().join("deep/er")).unwrap();
    fs::write(d.path().join("deep/er/Unseen.rs"), "").unwrap();
    app.handle_key(ctrl('p'));
    assert!(
        app.index_rx.is_some(),
        "opening go to file rebuilds the index"
    );
    wait_index(&mut app);
    typ(&mut app, "unseen");
    assert_eq!(items(&app).first().map(String::as_str), Some("Unseen.rs"));
}

#[cfg(unix)]
#[test]
fn linked_folder_outside_the_project_refuses_destructive_ops() {
    let (d, mut app) = project();
    let r = d.path().to_path_buf();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("x.txt"), "keep\n").unwrap();
    std::os::unix::fs::symlink(outside.path(), r.join("link")).unwrap();
    let side = tempfile::tempdir().unwrap();
    let trash = fake_trash(&mut app, &side.path().join("trash"));
    app.explorer.reload_all();
    // Rename inside the linked folder: refused, nothing renamed.
    select(&mut app, "link/x.txt");
    app.handle_key(key(KeyCode::F(2)));
    submit(&mut app, "y.txt");
    let err = prompt(&app)
        .and_then(|p| p.1)
        .unwrap_or_else(|| notices(&app));
    assert!(err.contains("outside the project"), "{err}");
    app.dialog = None;
    assert!(outside.path().join("x.txt").is_file());
    // Trash: refused, nothing trashed.
    select(&mut app, "link/x.txt");
    app.handle_key(key(KeyCode::Delete));
    app.handle_key(key(KeyCode::Left));
    app.handle_key(key(KeyCode::Enter));
    assert!(trash.trashed().is_empty());
    assert!(outside.path().join("x.txt").is_file());
    assert!(
        notices(&app).contains("outside the project"),
        "{}",
        notices(&app)
    );
    // Move (cut + paste) out of it: refused.
    select(&mut app, "link/x.txt");
    app.handle_key(ctrl('x'));
    select(&mut app, "tests");
    app.handle_key(ctrl('v'));
    assert!(outside.path().join("x.txt").is_file());
    assert!(!r.join("tests/x.txt").exists());
    // New file and paste into it stay allowed; the link itself can be trashed.
    select(&mut app, "link/x.txt");
    app.handle_key(ctrl('n'));
    submit(&mut app, "new.txt");
    assert!(outside.path().join("new.txt").is_file());
    select(&mut app, "README.md");
    app.handle_key(ctrl('c'));
    select(&mut app, "link/x.txt");
    app.handle_key(ctrl('v'));
    assert!(
        outside.path().join("README.md").is_file(),
        "{}",
        notices(&app)
    );
    select(&mut app, "link");
    app.handle_key(key(KeyCode::Delete));
    app.handle_key(key(KeyCode::Left));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(trash.trashed().len(), 1, "{}", notices(&app));
    assert!(outside.path().join("x.txt").is_file());
}

#[test]
fn ignored_rows_carry_a_text_marker() {
    let (d, mut app) = project();
    fs::write(d.path().join(".gitignore"), "target/\n").unwrap();
    fs::create_dir(d.path().join("target")).unwrap();
    app.run_command("explorer.toggle_ignored", crate::commands::CmdArgs::None);
    assert!(
        notices(&app).contains('·'),
        "the notice says what the marker means"
    );
    let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    t.draw(|f| crate::ui::render(f, &mut app)).unwrap();
    let buf = t.backend().buffer().clone();
    let a = app.explorer.area;
    let line = |y: u16| -> String {
        (a.x..a.x + a.width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect()
    };
    let lines: Vec<String> = (a.y..a.y + a.height).map(line).collect();
    let target = lines
        .iter()
        .find(|l| l.contains("target"))
        .expect("target row");
    assert!(target.contains("target ·"), "{target:?}");
    let src = lines.iter().find(|l| l.contains("src")).unwrap();
    assert!(!src.contains('·'), "{src:?}");
}

#[test]
fn default_layout_with_explorer_clicks_editor_and_tabs() {
    let (d, mut app) = project();
    fs::write(d.path().join("a.txt"), "abcdef\nghijkl\n").unwrap();
    app.open_path(Path::new("README.md")).unwrap();
    app.open_path(Path::new("a.txt")).unwrap();
    let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    t.draw(|f| crate::ui::render(f, &mut app)).unwrap();
    let l = app.layout;
    assert!(l.explorer.width > 0 && l.editor.x > 0, "{l:?}");
    let gutter = crate::ui::editor_view::gutter_width(app.active_doc().unwrap().buffer.text());
    let mouse = |kind, column, row| crossterm::event::MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    };
    use crossterm::event::{MouseButton, MouseEventKind as K};
    // Click at line 2, column 3 of the editor, then drag to line 1, column 2.
    app.handle_mouse(mouse(
        K::Down(MouseButton::Left),
        l.editor.x + gutter + 2,
        l.editor.y + 1,
    ));
    assert_eq!(app.focus, Focus::Editor);
    let sel = app.active_doc().unwrap().buffer.selection().primary();
    assert_eq!((sel.anchor, sel.head), (9, 9));
    app.handle_mouse(mouse(
        K::Drag(MouseButton::Left),
        l.editor.x + gutter + 1,
        l.editor.y,
    ));
    app.handle_mouse(mouse(
        K::Up(MouseButton::Left),
        l.editor.x + gutter + 1,
        l.editor.y,
    ));
    let sel = app.active_doc().unwrap().buffer.selection().primary();
    assert_eq!((sel.anchor, sel.head), (9, 1));
    // A click on the first tab's name activates it.
    let buf = t.backend().buffer().clone();
    let row: String = (0..80)
        .map(|x| buf[(x, l.tabs.y)].symbol().to_string())
        .collect();
    let byte = row.find("README.md").expect("tab drawn");
    let x = row[..byte].chars().count() as u16;
    app.handle_mouse(mouse(K::Down(MouseButton::Left), x + 1, l.tabs.y));
    assert_eq!(app.active_name(), "README.md");
}

#[test]
fn explorer_cut_paste_never_loses_a_concurrent_write() {
    race_explorer("cut");
}
