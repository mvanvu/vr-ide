//! Multi-root workspace through the `App` API (W-1): go to file, project search and
//! replace, watches, add / remove / open / save of roots, recent workspaces, recovery.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use vr_config::{Keymap, Settings};

use crate::app::App;
use crate::clipboard::MemoryClipboard;
use crate::commands::dialog::DialogAction;
use crate::ui::dialog::DialogWidget;
use crate::workspace::{Workspace, WorkspaceError};

/// Folders `names` (each holding `files`) under one temp dir.
fn folders(spec: &[(&str, &[(&str, &str)])]) -> (tempfile::TempDir, Vec<PathBuf>) {
    let d = tempfile::tempdir().unwrap();
    let base = d.path().canonicalize().unwrap();
    let mut roots = Vec::new();
    for (name, files) in spec {
        let root = base.join(name);
        std::fs::create_dir_all(&root).unwrap();
        for (rel, body) in *files {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }
        roots.push(root);
    }
    (d, roots)
}

fn ws_app(roots: &[PathBuf]) -> App {
    let (ws, refused) = Workspace::from_paths(roots).unwrap();
    assert!(refused.is_empty(), "{refused:?}");
    let mut app = App::with_workspace(
        ws,
        Keymap::load(None).0,
        Settings::default(),
        Box::new(MemoryClipboard::new()),
    );
    app.enter_settle = Duration::ZERO;
    app.enter_repeat_gap = Duration::ZERO;
    app
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn typ(app: &mut App, s: &str) {
    for c in s.chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
}

fn wait_index(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.index_rx.is_some() {
        assert!(Instant::now() < deadline, "index never finished");
        std::thread::sleep(Duration::from_millis(5));
        app.drain_index();
    }
}

fn wait_search(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        app.drain_search();
        if !app.search.running() && !app.search.busy() {
            return;
        }
        assert!(Instant::now() < deadline, "search did not finish");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn index_files(app: &App) -> Vec<String> {
    app.file_index
        .as_ref()
        .expect("index built")
        .files()
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

fn groups(app: &App) -> Vec<String> {
    app.search
        .groups
        .iter()
        .map(|g| app.display_path(&g.path))
        .collect()
}

fn notices(app: &App) -> String {
    app.notices
        .iter()
        .map(|n| n.text.clone())
        .collect::<Vec<_>>()
        .join(" | ")
}

fn choose(app: &mut App, label: &str) {
    let Some(DialogWidget::Confirm(c)) = app.dialog.as_ref().map(|d| &d.widget) else {
        panic!("no confirm dialog: {}", notices(app));
    };
    let i = c.buttons.iter().position(|b| b == label).unwrap();
    let (sel, n) = (c.selected, c.buttons.len());
    for _ in 0..(i + n - sel) % n {
        app.handle_key(key(KeyCode::Right));
    }
    app.handle_key(key(KeyCode::Enter));
}

#[test]
fn go_to_file_lists_every_root_by_name_and_opens_from_the_second() {
    let (_d, roots) = folders(&[
        ("api", &[("src/main.rs", "fn main() {}\n")]),
        ("web", &[("src/app.ts", "x\n"), ("src/main.rs", "y\n")]),
    ]);
    let mut app = ws_app(&roots);
    app.handle_key(ctrl('p'));
    wait_index(&mut app);
    assert_eq!(
        index_files(&app),
        ["api/src/main.rs", "web/src/app.ts", "web/src/main.rs"]
    );
    typ(&mut app, "main");
    let p = app.palette.as_ref().unwrap();
    let shown: Vec<(String, String)> = p
        .items
        .iter()
        .map(|i| (i.title.clone(), i.detail.clone()))
        .collect();
    assert_eq!(
        shown,
        [
            ("main.rs".to_string(), "api/src".to_string()),
            ("main.rs".to_string(), "web/src".to_string())
        ]
    );
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));
    let doc = app.active_doc().expect("opened");
    assert_eq!(
        doc.path.as_deref(),
        Some(roots[1].join("src/main.rs").as_path())
    );
    // The status bar / tab names show the root.
    assert_eq!(app.active_name(), "web/src/main.rs");
    assert_eq!(app.active_root().path, roots[1]);
    assert_eq!(app.title(), "vr-ide — api, web");
}

#[test]
fn project_search_and_replace_cover_both_roots_each_with_its_own_gitignore() {
    let (d, roots) = folders(&[
        (
            "a",
            &[
                (".gitignore", "*.log\n"),
                ("x.rs", "old\n"),
                ("skip.log", "old\n"),
            ],
        ),
        ("b", &[("keep.log", "old old\n"), ("y.rs", "old\n")]),
    ]);
    let mut app = ws_app(&roots);
    app.handle_key(ctrl('r'));
    typ(&mut app, "old");
    app.handle_key(key(KeyCode::Tab));
    typ(&mut app, "new");
    app.handle_key(key(KeyCode::Enter));
    wait_search(&mut app);
    // Grouped by root (a, then b), then by file; a's .gitignore does not apply in b.
    let g = groups(&app);
    assert_eq!(g.len(), 3, "{g:?}");
    assert_eq!(g[0], "a/x.rs");
    let mut rest = g[1..].to_vec();
    rest.sort();
    assert_eq!(rest, ["b/keep.log", "b/y.rs"]);
    assert_eq!(app.search.total, 4);

    // Replace previews and applies across both roots.
    let preview = app.search.preview.as_ref().expect("preview");
    assert_eq!(preview.files.len(), 3);
    assert!(
        preview.files.iter().all(|f| f.error.is_none()),
        "{:?}",
        preview.files
    );
    app.handle_key(key(KeyCode::Enter));
    assert!(matches!(
        app.dialog.as_ref().map(|d| &d.action),
        Some(DialogAction::ReplaceInFiles)
    ));
    choose(&mut app, "Replace");
    wait_search(&mut app);
    let read = |p: &Path| std::fs::read_to_string(p).unwrap();
    assert_eq!(read(&roots[0].join("x.rs")), "new\n");
    assert_eq!(read(&roots[0].join("skip.log")), "old\n");
    assert_eq!(read(&roots[1].join("keep.log")), "new new\n");
    assert_eq!(read(&roots[1].join("y.rs")), "new\n");
    drop(d);
}

#[test]
fn add_and_remove_root_update_index_search_and_watches_live() {
    let (_d, roots) = folders(&[
        ("a", &[("one.txt", "needle\n")]),
        ("b", &[("two.txt", "needle\n")]),
    ]);
    let mut app = ws_app(&roots[..1]);
    app.start_watcher();
    app.watch_root();
    app.start_index();
    wait_index(&mut app);
    assert_eq!(index_files(&app), ["one.txt"]);
    crate::app_tests::run(&mut app, "search.project");
    typ(&mut app, "needle");
    app.handle_key(key(KeyCode::Enter));
    wait_search(&mut app);
    assert_eq!(groups(&app), ["one.txt"]);
    assert!(app.explorer.watched.contains(&roots[0]));

    let id = app.add_root(&roots[1]).unwrap();
    wait_index(&mut app);
    assert_eq!(index_files(&app), ["a/one.txt", "b/two.txt"]);
    wait_search(&mut app);
    assert_eq!(
        groups(&app),
        ["a/one.txt", "b/two.txt"],
        "the shown search reran"
    );
    assert!(app.explorer.watched.contains(&roots[1]));
    assert_eq!(app.title(), "vr-ide — a, b");

    // A tab from the root survives its removal and keeps working.
    let two = roots[1].join("two.txt");
    app.open_path(&two).unwrap();
    app.refocus_editor_in_read_mode();
    crate::app_tests::space_i(&mut app);
    typ(&mut app, "x");
    let removed = app.remove_root(id).unwrap();
    assert_eq!(removed.path, roots[1]);
    assert_eq!(app.docs.len(), 1);
    wait_index(&mut app);
    assert_eq!(index_files(&app), ["one.txt"]);
    wait_search(&mut app);
    assert_eq!(groups(&app), ["one.txt"]);
    assert!(!app.explorer.watched.contains(&roots[1]));
    // Outside every root now: shown with its full path, still saved in place.
    assert_eq!(app.active_name(), two.display().to_string());
    app.handle_key(ctrl('s'));
    assert_eq!(std::fs::read_to_string(&two).unwrap(), "xneedle\n");
    assert!(
        two.exists() && roots[1].is_dir(),
        "nothing on disk is removed"
    );
}

#[test]
fn add_root_refuses_nested_and_duplicate_folders_and_the_last_root_stays() {
    let (_d, roots) = folders(&[("a", &[("sub/f.txt", "")])]);
    let mut app = ws_app(&roots);
    assert!(matches!(
        app.add_root(&roots[0].join("sub")),
        Err(WorkspaceError::Nested { .. })
    ));
    assert!(matches!(
        app.add_root(&roots[0]),
        Err(WorkspaceError::Duplicate { .. })
    ));
    assert!(matches!(
        app.add_root(roots[0].parent().unwrap()),
        Err(WorkspaceError::Contains { .. })
    ));
    let only = app.workspace.primary().id;
    assert_eq!(app.remove_root(only).unwrap_err(), WorkspaceError::LastRoot);
    assert_eq!(app.workspace.len(), 1);
}

#[test]
fn removing_the_first_root_moves_the_explorer_to_the_next() {
    let (_d, roots) = folders(&[("a", &[("one.txt", "")]), ("b", &[("two.txt", "")])]);
    let mut app = ws_app(&roots);
    app.start_watcher();
    app.watch_root();
    crate::commands::explorer::ensure_loaded(&mut app);
    let first = app.workspace.primary().id;
    app.remove_root(first).unwrap();
    assert_eq!(app.explorer.root(), roots[1]);
    assert!(app.explorer.is_loaded());
    assert!(
        app.explorer
            .rows
            .iter()
            .any(|r| r.path == roots[1].join("two.txt")),
        "the new first root is listed"
    );
    assert!(!app.explorer.watched.contains(&roots[0]));
    assert!(app.explorer.watched.contains(&roots[1]));
}

#[test]
fn open_workspace_asks_for_unsaved_files_then_replaces_the_roots() {
    let (d, roots) = folders(&[
        ("a", &[("one.txt", "1\n")]),
        ("b", &[("two.txt", "2\n")]),
        ("c", &[("three.txt", "3\n")]),
    ]);
    let state = tempfile::tempdir().unwrap();
    let mut app = ws_app(&roots[..1]);
    app.recovery_dir = Some(state.path().join("recovery"));
    app.recent_workspaces_file = Some(state.path().join("recent.toml"));
    app.open_path(&roots[0].join("one.txt")).unwrap();
    crate::app_tests::space_i(&mut app);
    typ(&mut app, "x");

    let target = Workspace::from_paths(&roots[1..]).unwrap().0;
    // Cancel keeps everything.
    app.open_workspace(target.clone());
    choose(&mut app, "Cancel");
    assert_eq!(app.workspace.paths(), roots[..1]);
    assert_eq!(app.docs.len(), 1);

    // Discard keeps the buffer in the recovery folder, closes the tabs, switches.
    app.open_workspace(target.clone());
    let Some(DialogWidget::Confirm(c)) = app.dialog.as_ref().map(|d| &d.widget) else {
        panic!("unsaved files are asked about: {}", notices(&app));
    };
    assert!(c.message.contains("one.txt"), "{}", c.message);
    choose(&mut app, "Discard");
    assert_eq!(app.workspace.paths(), roots[1..]);
    assert!(app.docs.is_empty());
    assert_eq!(app.explorer.root(), roots[1]);
    assert_eq!(
        std::fs::read_to_string(roots[0].join("one.txt")).unwrap(),
        "1\n"
    );
    let kept: Vec<String> = std::fs::read_dir(state.path().join("recovery"))
        .unwrap()
        .map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap())
        .collect();
    assert_eq!(kept, ["x1\n"]);
    assert!(notices(&app).contains("recovery"), "{}", notices(&app));

    // Recorded in the recent list (and its file), newest first.
    let (recent, w) = vr_config::RecentWorkspaces::load(&state.path().join("recent.toml"));
    assert!(w.is_none());
    assert_eq!(recent.entries[0].roots, roots[1..]);
    assert_eq!(app.recent_workspaces, recent);

    // Without unsaved changes it switches at once; Save all saves first.
    app.open_workspace(Workspace::single(roots[0].clone()));
    assert!(app.dialog.is_none());
    assert_eq!(app.workspace.paths(), roots[..1]);
    app.open_path(&roots[0].join("one.txt")).unwrap();
    crate::app_tests::space_i(&mut app);
    typ(&mut app, "y");
    app.open_workspace(target);
    choose(&mut app, "Save all");
    assert_eq!(
        std::fs::read_to_string(roots[0].join("one.txt")).unwrap(),
        "y1\n"
    );
    assert_eq!(app.workspace.paths(), roots[1..]);
    drop(d);
}

#[test]
fn save_workspace_round_trips_and_never_overwrites_an_unseen_version() {
    let (d, roots) = folders(&[("a", &[]), ("b", &[])]);
    let state = tempfile::tempdir().unwrap();
    let mut app = ws_app(&roots);
    app.recent_workspaces_file = Some(state.path().join("recent.toml"));
    let file = d.path().canonicalize().unwrap().join("team.vr-workspace");
    app.save_workspace(&file, None).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("\"a\"") && text.contains("\"b\""), "{text}");
    assert_eq!(app.workspace.file(), Some(file.as_path()));
    assert_eq!(app.title(), "vr-ide — team");
    assert_eq!(
        app.recent_workspaces.entries[0].file.as_deref(),
        Some(file.as_path())
    );
    let (back, _) = Workspace::from_file(&file).unwrap();
    assert_eq!(back.paths(), roots);

    // An existing file is replaced only over the version the user was shown.
    let err = app.save_workspace(&file, None).unwrap_err();
    assert!(err.contains("exists"), "{err}");
    let seen = vr_core::DiskVersion::of_path(&file).unwrap();
    std::fs::write(&file, "roots = [\"someone else\"]\n").unwrap();
    let err = app.save_workspace(&file, Some(&seen)).unwrap_err();
    assert!(err.contains("changed"), "{err}");
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "roots = [\"someone else\"]\n",
        "a concurrent writer's version is never overwritten"
    );
    let seen = vr_core::DiskVersion::of_path(&file).unwrap();
    app.save_workspace(&file, Some(&seen)).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
}

#[test]
fn recovery_entries_record_the_root_of_each_file() {
    let (_d, roots) = folders(&[("api", &[("x.txt", "1\n")]), ("web", &[("x.txt", "2\n")])]);
    let state = tempfile::tempdir().unwrap();
    let mut app = ws_app(&roots);
    app.recovery_dir = Some(state.path().join("recovery"));
    for r in &roots {
        app.open_path(&r.join("x.txt")).unwrap();
        crate::app_tests::space_i(&mut app);
        typ(&mut app, "z");
    }
    let written = app.keep_discarded(&[0, 1]).unwrap();
    let mut names: Vec<String> = written
        .iter()
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert!(
        names[0].contains("-api+") && names[0].ends_with("x.txt"),
        "{names:?}"
    );
    assert!(
        names[1].contains("-web+") && names[1].ends_with("x.txt"),
        "{names:?}"
    );
}

#[test]
fn open_workspace_switches_nothing_when_unsaved_work_cannot_be_kept() {
    let (_d, roots) = folders(&[("a", &[("one.txt", "1\n")]), ("b", &[])]);
    let mut app = ws_app(&roots[..1]);
    // The recovery folder cannot be made (a file is in the way): Discard must not discard.
    let blocker = tempfile::NamedTempFile::new().unwrap();
    app.recovery_dir = Some(blocker.path().join("recovery"));
    app.open_path(&roots[0].join("one.txt")).unwrap();
    crate::app_tests::space_i(&mut app);
    typ(&mut app, "x");
    app.open_workspace(Workspace::single(roots[1].clone()));
    choose(&mut app, "Discard");
    assert_eq!(app.workspace.paths(), roots[..1]);
    assert_eq!(app.docs.len(), 1);
    assert!(app.docs[0].is_modified());
    assert!(
        notices(&app).contains("nothing was discarded"),
        "{}",
        notices(&app)
    );

    // A save that does not go through (the file became read-only) does not switch either.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let f = roots[0].join("one.txt");
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o444)).unwrap();
        let writable = std::fs::OpenOptions::new().write(true).open(&f).is_ok();
        if !writable {
            app.open_workspace(Workspace::single(roots[1].clone()));
            choose(&mut app, "Save all");
            assert_eq!(app.workspace.paths(), roots[..1]);
            assert!(app.docs[0].is_modified());
        }
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
}

#[test]
fn roots_do_not_change_while_a_project_replace_writes() {
    let (_d, roots) = folders(&[("a", &[]), ("b", &[])]);
    let mut app = ws_app(&roots[..1]);
    crate::commands::search::tests::stuck_job(&mut app, crate::commands::search::JobKind::Apply);
    app.open_workspace(Workspace::single(roots[1].clone()));
    assert_eq!(app.workspace.paths(), roots[..1]);
    assert!(
        notices(&app).contains("replace is running"),
        "{}",
        notices(&app)
    );
}
