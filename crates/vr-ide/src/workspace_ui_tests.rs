//! Multi-root workspace UI (W-2): the explorer's root nodes, confinement of file operations
//! to their root, the root menu, status bar / welcome roots, the terminal root pick, the
//! workspace palette commands and the W-1 review follow-ups.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use vr_config::{Keymap, Settings};

use crate::app::{App, Focus};
use crate::clipboard::MemoryClipboard;
use crate::commands::CmdArgs;
use crate::workspace::Workspace;

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
    let (ws, _refused) = Workspace::from_paths(roots).unwrap();
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

/// `ws_app` with the explorer shown, loaded and focused.
fn explorer_app(roots: &[PathBuf]) -> App {
    let mut app = ws_app(roots);
    app.run_command("explorer.focus", CmdArgs::None);
    app
}

fn draw(app: &mut App, w: u16, h: u16) -> Terminal<TestBackend> {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| crate::ui::render(f, app)).unwrap();
    t
}

fn key(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn typ(app: &mut App, s: &str) {
    for c in s.chars() {
        key(app, KeyCode::Char(c));
    }
}

/// Explorer rows as "  name" lines (two spaces per level).
fn tree(app: &App) -> Vec<String> {
    app.explorer
        .rows
        .iter()
        .map(|r| format!("{}{}", "  ".repeat(r.depth), r.name()))
        .collect()
}

fn notices(app: &App) -> String {
    app.notices
        .iter()
        .map(|n| n.text.clone())
        .collect::<Vec<_>>()
        .join(" | ")
}

/// Puts the explorer cursor on the row of `path`.
fn select(app: &mut App, path: &Path) {
    let i = app
        .explorer
        .rows
        .iter()
        .position(|r| r.path == path)
        .unwrap_or_else(|| panic!("no row {}: {:?}", path.display(), tree(app)));
    app.explorer.cursor = i;
    app.explorer.selected.clear();
}

fn mouse(app: &mut App, kind: MouseEventKind, column: u16, row: u16) {
    app.handle_mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    });
}

/// Right click on the explorer row of `path` (drawn at 80x24 first).
fn right_click(app: &mut App, path: &Path) {
    draw(app, 80, 24);
    let i = app
        .explorer
        .rows
        .iter()
        .position(|r| r.path == path)
        .unwrap();
    let a = app.explorer.area;
    let y = a.y + (i - app.explorer.drawn_top) as u16;
    mouse(app, MouseEventKind::Down(MouseButton::Right), a.x + 2, y);
    mouse(app, MouseEventKind::Up(MouseButton::Right), a.x + 2, y);
}

fn menu_labels(app: &App) -> Vec<(String, bool)> {
    let m = app.explorer.menu.as_ref().expect("menu open");
    m.items
        .iter()
        .map(|i| (i.label.to_string(), i.enabled))
        .collect()
}

fn choose_menu(app: &mut App, label: &str) {
    let m = app.explorer.menu.as_ref().expect("menu open");
    let i = m.items.iter().position(|i| i.label == label).unwrap();
    let (x, y) = (m.area.x + 2, m.area.y + 1 + i as u16);
    mouse(app, MouseEventKind::Down(MouseButton::Left), x, y);
    mouse(app, MouseEventKind::Up(MouseButton::Left), x, y);
}

// ---- explorer: one node per root ------------------------------------------------------------

#[test]
fn explorer_shows_one_expandable_node_per_root() {
    let (_d, roots) = folders(&[
        ("api", &[("src/main.rs", ""), ("Cargo.toml", "")]),
        ("web", &[("index.html", "")]),
    ]);
    let mut app = explorer_app(&roots);
    assert_eq!(
        tree(&app),
        ["api", "  src", "  Cargo.toml", "web", "  index.html"]
    );
    // A root collapses and expands like a folder (Left / Enter on its row).
    select(&mut app, &roots[1]);
    key(&mut app, KeyCode::Left);
    assert_eq!(tree(&app), ["api", "  src", "  Cargo.toml", "web"]);
    key(&mut app, KeyCode::Enter);
    assert_eq!(tree(&app).last().unwrap(), "  index.html");
    // Left on a child goes to its root row.
    let i = tree(&app).len() - 1;
    app.explorer.cursor = i;
    key(&mut app, KeyCode::Left);
    assert_eq!(app.explorer.current().unwrap().path, roots[1]);
    // One root: the root row stays open (as before W-2).
    let (_d1, one) = folders(&[("solo", &[("a.txt", "")])]);
    let mut app = explorer_app(&one);
    key(&mut app, KeyCode::Home);
    key(&mut app, KeyCode::Left);
    assert_eq!(tree(&app), ["solo", "  a.txt"]);
}

// ---- explorer: operations stay in their root (AC-W5) ------------------------------------------

#[test]
fn move_between_roots_is_refused_and_copy_works() {
    let (_d, roots) = folders(&[
        ("api", &[("a.txt", "A\n"), ("src/b.rs", "")]),
        ("web", &[("www/index.html", "")]),
    ]);
    let (api, web) = (roots[0].clone(), roots[1].clone());
    let mut app = explorer_app(&roots);
    // Cut in api, paste on web's root row: refused, the file stays.
    select(&mut app, &api.join("a.txt"));
    app.run_command("explorer.cut", CmdArgs::None);
    select(&mut app, &web);
    app.run_command("explorer.paste", CmdArgs::None);
    assert!(api.join("a.txt").is_file());
    assert!(!web.join("a.txt").exists());
    let n = notices(&app);
    assert!(n.contains("between workspace folders"), "{n}");
    // The Move prompt with a folder of the other root (typed as shown: `web/www`): refused.
    select(&mut app, &api.join("a.txt"));
    app.run_command("explorer.move", CmdArgs::None);
    let d = app.dialog.as_mut().expect("move prompt");
    if let crate::ui::dialog::DialogWidget::Prompt(p) = &mut d.widget {
        p.input.set("web/www");
    }
    key(&mut app, KeyCode::Enter);
    assert!(api.join("a.txt").is_file());
    assert!(!web.join("www/a.txt").exists());
    let err = match app.dialog.as_ref().map(|d| &d.widget) {
        Some(crate::ui::dialog::DialogWidget::Prompt(p)) => p.error.clone().unwrap_or_default(),
        _ => notices(&app),
    };
    assert!(err.contains("between workspace folders"), "{err}");
    key(&mut app, KeyCode::Esc);
    // Inside its own root the Move prompt takes `api/src`.
    select(&mut app, &api.join("a.txt"));
    app.run_command("explorer.move", CmdArgs::None);
    let d = app.dialog.as_mut().expect("move prompt");
    if let crate::ui::dialog::DialogWidget::Prompt(p) = &mut d.widget {
        p.input.set("api/src");
    }
    key(&mut app, KeyCode::Enter);
    assert!(api.join("src/a.txt").is_file(), "{}", notices(&app));
    // Copy from api into web's folder works.
    select(&mut app, &api.join("src/b.rs"));
    app.run_command("explorer.copy", CmdArgs::None);
    select(&mut app, &web.join("www"));
    app.run_command("explorer.paste", CmdArgs::None);
    assert!(web.join("www/b.rs").is_file(), "{}", notices(&app));
    assert!(api.join("src/b.rs").is_file());
    // Rename and trash act inside the root of the row.
    select(&mut app, &web.join("www/b.rs"));
    app.run_command("explorer.rename", CmdArgs::None);
    let d = app.dialog.as_mut().expect("rename prompt");
    if let crate::ui::dialog::DialogWidget::Prompt(p) = &mut d.widget {
        p.input.set("c.rs");
    }
    key(&mut app, KeyCode::Enter);
    assert!(web.join("www/c.rs").is_file(), "{}", notices(&app));
    let held = tempfile::tempdir().unwrap();
    app.trash = Box::new(vr_fs::FakeTrash::new(held.path()));
    select(&mut app, &web.join("www/c.rs"));
    app.run_command("explorer.delete", CmdArgs::None);
    key(&mut app, KeyCode::Left);
    key(&mut app, KeyCode::Enter);
    assert!(!web.join("www/c.rs").exists(), "{}", notices(&app));
    // A root row is never renamed, moved or trashed.
    select(&mut app, &web);
    app.run_command("explorer.rename", CmdArgs::None);
    assert!(app.dialog.is_none());
    assert!(web.is_dir());
}

#[test]
fn each_root_hides_what_its_own_gitignore_lists() {
    let (_d, roots) = folders(&[
        (
            "api",
            &[
                (".gitignore", "secret.txt\n"),
                ("secret.txt", ""),
                ("x.rs", ""),
            ],
        ),
        ("web", &[("secret.txt", ""), (".env", "")]),
    ]);
    let mut app = explorer_app(&roots);
    // Hidden files are shown by default.
    assert_eq!(
        tree(&app),
        [
            "api",
            "  .gitignore",
            "  x.rs",
            "web",
            "  .env",
            "  secret.txt"
        ]
    );
    // The toggle applies to every root, each with its own rules.
    app.run_command("explorer.toggle_ignored", CmdArgs::None);
    assert_eq!(
        tree(&app),
        [
            "api",
            "  .gitignore",
            "  secret.txt",
            "  x.rs",
            "web",
            "  .env",
            "  secret.txt"
        ]
    );
    let ignored: Vec<String> = app
        .explorer
        .rows
        .iter()
        .filter(|r| r.ignored)
        .map(|r| app.display_path(&r.path))
        .collect();
    assert_eq!(ignored, ["api/secret.txt"]);
}

// ---- explorer: missing roots and Remove Folder ------------------------------------------------

#[test]
fn a_missing_root_is_listed_and_the_root_menu_removes_it_without_touching_files() {
    let (d, roots) = folders(&[("api", &[("a.txt", "A\n")]), ("web", &[("b.txt", "B\n")])]);
    let gone = d.path().canonicalize().unwrap().join("gone");
    let mut all = roots.clone();
    all.push(gone.clone());
    let mut app = explorer_app(&all);
    let last = app.explorer.rows.last().unwrap().clone();
    assert_eq!((last.path.clone(), last.missing), (gone.clone(), true));
    let t = draw(&mut app, 80, 24);
    let screen = format!("{}", t.backend());
    assert!(screen.contains("gone (missing)"), "{screen}");
    // The root row's menu: New and Paste as on any root, plus Remove Folder.
    right_click(&mut app, &gone);
    let items = menu_labels(&app);
    assert!(
        items.contains(&("Remove Folder from Workspace".to_string(), true)),
        "{items:?}"
    );
    assert!(items.contains(&("Delete".to_string(), false)), "{items:?}");
    choose_menu(&mut app, "Remove Folder from Workspace");
    assert_eq!(app.workspace.paths(), roots);
    assert_eq!(tree(&app), ["api", "  a.txt", "web", "  b.txt"]);
    // A root that exists: removed from the workspace, nothing on disk changes, an open tab
    // of it stays.
    app.open_path(&roots[1].join("b.txt")).unwrap();
    right_click(&mut app, &roots[1]);
    choose_menu(&mut app, "Remove Folder from Workspace");
    assert_eq!(app.workspace.paths(), [roots[0].clone()]);
    assert_eq!(
        std::fs::read_to_string(roots[1].join("b.txt")).unwrap(),
        "B\n"
    );
    assert_eq!(app.docs.len(), 1);
    assert_eq!(tree(&app), ["api", "  a.txt"]);
    // The last root has no Remove item (D241).
    right_click(&mut app, &roots[0]);
    assert!(
        !menu_labels(&app)
            .iter()
            .any(|(l, _)| l.starts_with("Remove Folder")),
        "{:?}",
        menu_labels(&app)
    );
}

// ---- palette commands -------------------------------------------------------------------------

/// Runs the palette entry titled `title` as the user would: F1, the title typed, Enter.
fn palette_run(app: &mut App, title: &str) {
    key(app, KeyCode::F(1));
    typ(app, title);
    let p = app.palette.as_ref().expect("palette open");
    assert_eq!(
        p.items.get(p.selected).map(|i| i.title.as_str()),
        Some(title),
        "{:?}",
        p.items.iter().map(|i| &i.title).collect::<Vec<_>>()
    );
    key(app, KeyCode::Enter);
}

#[test]
fn workspace_palette_commands_open_their_slash_command() {
    let (_d, roots) = folders(&[("api", &[]), ("web", &[])]);
    let mut app = ws_app(&roots);
    for (title, text) in [
        ("Workspace: Add Folder…", "/ws add "),
        ("Workspace: Remove Folder", "/ws remove "),
        ("Workspace: Save As…", "/ws save "),
        ("Workspace: Open…", "/ws open "),
        ("Workspace: Open Recent", "/ws recent "),
    ] {
        palette_run(&mut app, title);
        assert_eq!(app.focus, Focus::Palette, "{title}");
        assert_eq!(crate::slash_tests::input(&app), text, "{title}");
        key(&mut app, KeyCode::Esc);
    }
    // No default key for any of them (keys are the user's choice).
    for c in crate::commands::workspace::COMMANDS {
        assert_eq!(app.display_key(c.name), None, "{}", c.name);
    }
}

// ---- terminal: the root a new terminal starts in (AC-W6) ----------------------------------------

fn use_sh(app: &mut App) {
    app.terminal.shell = Some(PathBuf::from("/bin/sh"));
    app.terminal.env = vec![
        ("PS1".into(), "$ ".into()),
        ("ENV".into(), "/dev/null".into()),
    ];
}

/// Waits until the active terminal's screen satisfies `ok`.
fn wait_screen(app: &App, ok: impl Fn(&str) -> bool) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let tab = &app.terminal.tabs[app.terminal.active];
        let s = tab.screen_text();
        if ok(&s) {
            return s;
        }
        assert!(Instant::now() < deadline, "screen:\n{s}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// D291: the palette lists no Terminal: New / Close; the tab bar's '+' starts the shell in the
/// active file's root (no pick list), ctrl+` the first terminal likewise.
#[test]
fn the_plus_button_starts_a_terminal_in_the_active_files_root() {
    let (_d, roots) = folders(&[
        ("rootA", &[("a.txt", "a\n")]),
        ("rootB", &[("f.txt", "x\n")]),
    ]);
    let mut app = ws_app(&roots);
    use_sh(&mut app);
    app.keyboard_enhanced = true;
    key(&mut app, KeyCode::F(1));
    typ(&mut app, "Terminal");
    let titles: Vec<String> = app
        .palette
        .as_ref()
        .unwrap()
        .items
        .iter()
        .map(|i| i.title.clone())
        .collect();
    assert!(
        titles
            .iter()
            .all(|t| !t.contains("New Terminal") && !t.contains("Close Terminal")),
        "{titles:?}"
    );
    key(&mut app, KeyCode::Esc);
    let pwd_of_active = |app: &mut App| {
        draw(app, 120, 30);
        wait_screen(app, |s| s.contains('$'));
        typ(app, "pwd\n");
        wait_screen(app, |s| s.contains("/root"))
    };
    app.open_path(&roots[1].join("f.txt")).unwrap();
    draw(&mut app, 120, 30);
    crate::terminal_tests::ctrl_backtick(&mut app);
    assert_eq!(app.terminal.tabs.len(), 1);
    let s = pwd_of_active(&mut app);
    assert!(s.contains("rootB") && !s.contains("rootA"), "{s}");
    // The file of rootA active, then '+'.
    app.open_path(&roots[0].join("a.txt")).unwrap();
    draw(&mut app, 120, 30);
    let bar = crate::ui::terminal_view::tab_bar(&app, app.layout.terminal.width);
    let plus = app.layout.terminal.x + bar.plus.expect("the + is drawn");
    let y = app.layout.terminal.y;
    crate::terminal_tests::click(&mut app, plus + 1, y);
    assert_eq!(app.terminal.tabs.len(), 2);
    assert_eq!(app.terminal.active, 1);
    assert_eq!(app.focus, Focus::Terminal);
    let s = pwd_of_active(&mut app);
    assert!(s.contains("rootA") && !s.contains("rootB"), "{s}");
    crate::commands::terminal::shutdown(&mut app);
}

#[test]
fn terminal_and_relative_paths_skip_a_missing_first_root() {
    let (d, roots) = folders(&[("api", &[])]);
    let gone = d.path().canonicalize().unwrap().join("gone");
    let app = ws_app(&[gone.clone(), roots[0].clone()]);
    assert_eq!(app.workspace.paths(), [gone, roots[0].clone()]);
    assert_eq!(app.active_root().path, roots[0]);
}

// ---- W-1 review follow-ups --------------------------------------------------------------------

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

/// Results shown for the old workspace whose query field was edited since (so they are not
/// searched again) are dropped when another workspace opens.
#[test]
fn open_workspace_drops_results_of_the_old_roots() {
    let (_d, roots) = folders(&[("a", &[("x.rs", "foo\n")]), ("b", &[("y.rs", "foo\n")])]);
    let mut app = ws_app(&roots[..1]);
    crate::app_tests::run(&mut app, "search.project");
    typ(&mut app, "foo");
    key(&mut app, KeyCode::Enter);
    wait_search(&mut app);
    assert_eq!(app.search.groups.len(), 1);
    // Edited, not searched: the field no longer holds the shown query.
    typ(&mut app, "x");
    let (ws, _) = Workspace::from_paths(&roots[1..]).unwrap();
    app.open_workspace(ws);
    assert_eq!(app.workspace.paths(), roots[1..]);
    assert!(app.search.groups.is_empty(), "stale results kept");
    assert!(!app.search.running());
    assert_eq!(app.search.query, None);
}

#[test]
fn saving_a_workspace_through_a_linked_folder_keeps_relative_roots() {
    let (d, roots) = folders(&[("p/api", &[]), ("p/web", &[])]);
    let link = d.path().canonicalize().unwrap().join("link");
    std::os::unix::fs::symlink(roots[0].parent().unwrap(), &link).unwrap();
    let mut app = ws_app(&roots);
    let file = link.join("team.vr-workspace");
    app.save_workspace(&file, None).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(
        text.contains("\"api\"") && text.contains("\"web\""),
        "roots must be relative to the file's folder: {text}"
    );
}

#[test]
fn a_root_with_a_non_utf8_name_is_refused_for_files_and_lists() {
    use std::os::unix::ffi::OsStrExt;
    let (d, roots) = folders(&[("api", &[])]);
    let odd = d
        .path()
        .canonicalize()
        .unwrap()
        .join(std::ffi::OsStr::from_bytes(b"caf\xe9"));
    std::fs::create_dir(&odd).unwrap();
    // Add Folder refuses it with a notice.
    let mut app = ws_app(&roots);
    crate::slash_tests::slash(&mut app, &format!("/ws add {}", odd.display()));
    // Typed text is UTF-8: the lossy name does not exist, so go through the API as well.
    key(&mut app, KeyCode::Esc);
    let e = app
        .add_root_checked(&odd)
        .expect("checked in time")
        .unwrap_err();
    assert!(e.to_string().contains("not UTF-8"), "{e}");
    assert_eq!(app.workspace.len(), 1);
    // A workspace that holds one (CLI): saving refuses and names it; the recent list skips it.
    let mut app = ws_app(&[roots[0].clone(), odd.clone()]);
    app.recent_workspaces_file = Some(d.path().join("recent.toml"));
    let err = app
        .save_workspace(&d.path().join("x.vr-workspace"), None)
        .unwrap_err();
    assert!(err.contains("not UTF-8"), "{err}");
    assert!(!d.path().join("x.vr-workspace").exists());
    app.remember_workspace();
    assert!(notices(&app).contains("not listed"), "{}", notices(&app));
    assert!(!notices(&app).contains("cannot write"), "{}", notices(&app));
    assert!(app.recent_workspaces.entries.is_empty());
}

/// A check that outlasts the wait: the window closes, the result comes in later.
#[test]
fn add_folder_checks_run_off_the_ui_thread() {
    let (d, roots) = folders(&[("api", &[]), ("web", &[])]);
    let mut app = ws_app(&roots[..1]);
    app.ws_job_wait = Duration::ZERO;
    crate::slash_tests::slash(&mut app, &format!("/ws add {}", roots[1].display()));
    key(&mut app, KeyCode::Enter);
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.workspace.len() < 2 {
        assert!(Instant::now() < deadline, "{}", notices(&app));
        std::thread::sleep(Duration::from_millis(5));
        app.drain_ws_job();
    }
    assert!(notices(&app).contains("added web"), "{}", notices(&app));
    // A refusal that comes late is a notice.
    crate::slash_tests::slash(
        &mut app,
        &format!("/ws add {}", d.path().join("none").display()),
    );
    key(&mut app, KeyCode::Enter);
    while app.ws_job.is_some() {
        std::thread::sleep(Duration::from_millis(5));
        app.drain_ws_job();
    }
    assert!(
        notices(&app).contains("does not exist"),
        "{}",
        notices(&app)
    );
}

/// A FIFO named like a workspace file never blocks Open Workspace.
#[test]
fn opening_a_fifo_as_a_workspace_file_is_refused() {
    let (d, roots) = folders(&[("api", &[])]);
    let fifo = d.path().join("x.vr-workspace");
    let ok = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        eprintln!("mkfifo not available; skipped");
        return;
    }
    let mut app = ws_app(&roots);
    let t0 = Instant::now();
    crate::slash_tests::slash(&mut app, &format!("/ws open {}", fifo.display()));
    key(&mut app, KeyCode::Enter);
    assert!(t0.elapsed() < Duration::from_secs(2));
    let e = crate::slash_tests::error(&app).unwrap_or_default();
    assert!(e.contains("not a regular file"), "{e}");
    assert_eq!(app.workspace.paths(), roots);
}

// ---- snapshots: explorer with several roots and the status bar ---------------------------------

/// `api` (with src/ open), `web`, and `docs` gone from disk; web/src/app.ts open.
fn snapshot_app() -> (tempfile::TempDir, App) {
    let (d, roots) = folders(&[
        (
            "api",
            &[
                ("src/main.rs", "fn main() {}\n"),
                ("src/lib.rs", "\n"),
                ("Cargo.toml", "[package]\n"),
            ],
        ),
        (
            "web",
            &[
                ("src/app.ts", "export const app = 1;\n"),
                ("index.html", "\n"),
            ],
        ),
    ]);
    let gone = d.path().canonicalize().unwrap().join("docs");
    let mut all = roots.clone();
    all.push(gone);
    let mut app = explorer_app(&all);
    app.open_path(&roots[1].join("src/app.ts")).unwrap();
    app.run_command("explorer.reveal", CmdArgs::None);
    app.notices.clear();
    (d, app)
}

#[test]
fn status_bar_shows_the_root_name_with_several_roots() {
    let (_d, mut app) = snapshot_app();
    let t = draw(&mut app, 120, 40);
    let text = format!("{}", t.backend());
    let status = text.lines().last().unwrap();
    assert!(status.contains("web/src/app.ts"), "{status}");
}

#[test]
fn render_multi_root_explorer() {
    for (w, h) in [(120, 40), (80, 24), (40, 12)] {
        let (_d, mut app) = snapshot_app();
        let t = draw(&mut app, w, h);
        insta::assert_snapshot!(format!("render_multi_root_explorer_{w}x{h}"), t.backend());
    }
}

// ---- W-2 review round 1: pending workspace jobs ------------------------------------------

/// A job that answers only when the returned sender is used (or dropped).
fn held_job(
    app: &mut App,
    what: &str,
    done: impl FnOnce() -> crate::app_workspace::JobDone + Send + 'static,
) -> std::sync::mpsc::Sender<()> {
    let (go, wait) = std::sync::mpsc::channel::<()>();
    let r = app.run_ws_job(what.to_string(), move || {
        let _ = wait.recv();
        done()
    });
    assert!(r.is_none(), "the job must still be pending");
    go
}

/// A pending job replaced by a new one is named in a notice, never dropped in silence.
#[test]
fn a_replaced_workspace_job_is_named_in_a_notice() {
    let (_d, roots) = folders(&[("api", &[]), ("web", &[])]);
    let mut app = ws_app(&roots[..1]);
    app.ws_job_wait = Duration::ZERO;
    let web = roots[1].clone();
    let _first = held_job(
        &mut app,
        "Open Workspace /mnt/team.vr-workspace",
        move || crate::app_workspace::JobDone::Add(Ok(web)),
    );
    let _second = held_job(&mut app, "Add Folder /x", || {
        crate::app_workspace::JobDone::Add(Ok(PathBuf::from("/nonexistent")))
    });
    let n = notices(&app);
    assert!(
        n.contains("Open Workspace /mnt/team.vr-workspace cancelled by Add Folder /x"),
        "{n}"
    );
}

/// A late answer waits while a dialog (or the palette) is open and is applied after.
#[test]
fn a_late_workspace_answer_waits_for_the_open_dialog() {
    let (_d, roots) = folders(&[("api", &[("a.txt", "A\n")]), ("web", &[])]);
    let mut app = explorer_app(&roots[..1]);
    app.ws_job_wait = Duration::ZERO;
    let web = roots[1].clone();
    let go = held_job(&mut app, "Add Folder web", move || {
        crate::app_workspace::JobDone::Add(Ok(web))
    });
    // The user starts a Rename and types a name.
    select(&mut app, &roots[0].join("a.txt"));
    app.run_command("explorer.rename", CmdArgs::None);
    let d = app.dialog.as_mut().expect("rename prompt");
    if let crate::ui::dialog::DialogWidget::Prompt(p) = &mut d.widget {
        p.input.set("b.txt");
    }
    go.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    // The answer has come in, but the prompt stays as it was.
    while app.ws_job.as_ref().is_some_and(|j| !j.has_answer()) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
        app.drain_ws_job();
    }
    for _ in 0..3 {
        app.drain_ws_job();
    }
    assert_eq!(app.workspace.len(), 1, "applied under the dialog");
    match app.dialog.as_ref().map(|d| &d.widget) {
        Some(crate::ui::dialog::DialogWidget::Prompt(p)) => assert_eq!(p.input.text(), "b.txt"),
        _ => panic!("the rename prompt was replaced"),
    }
    // The prompt is answered; the held result is applied on the next pass.
    key(&mut app, KeyCode::Enter);
    assert!(roots[0].join("b.txt").is_file(), "{}", notices(&app));
    app.drain_ws_job();
    assert_eq!(app.workspace.len(), 2, "{}", notices(&app));
    assert!(app.ws_job.is_none());
    // The same holds for the palette.
    let (_d2, more) = folders(&[("lib", &[])]);
    let lib = more[0].clone();
    let go = held_job(&mut app, "Add Folder lib", move || {
        crate::app_workspace::JobDone::Add(Ok(lib))
    });
    app.run_command("app.palette", CmdArgs::None);
    assert!(app.palette.is_some());
    go.send(()).unwrap();
    while app.ws_job.as_ref().is_some_and(|j| !j.has_answer()) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
        app.drain_ws_job();
    }
    app.drain_ws_job();
    assert_eq!(app.workspace.len(), 2);
    assert!(app.palette.is_some());
    key(&mut app, KeyCode::Esc);
    app.drain_ws_job();
    assert_eq!(app.workspace.len(), 3, "{}", notices(&app));
}

/// A folder link into another root is that other root for moves (D292); a copy still works.
#[test]
fn moving_into_a_folder_link_to_another_root_is_refused() {
    let (_d, roots) = folders(&[
        ("api", &[("x.txt", "X\n"), ("y.txt", "Y\n")]),
        ("web", &[("sub/keep", "")]),
    ]);
    let (api, web) = (roots[0].clone(), roots[1].clone());
    std::os::unix::fs::symlink(web.join("sub"), api.join("link")).unwrap();
    let mut app = explorer_app(&roots);
    // Cut + paste on the link row: refused, the file stays.
    select(&mut app, &api.join("x.txt"));
    app.run_command("explorer.cut", CmdArgs::None);
    select(&mut app, &api.join("link"));
    app.run_command("explorer.paste", CmdArgs::None);
    assert!(api.join("x.txt").is_file());
    assert!(!web.join("sub/x.txt").exists());
    let n = notices(&app);
    assert!(n.contains("between workspace folders"), "{n}");
    // The Move prompt with `api/link`: refused the same way.
    select(&mut app, &api.join("x.txt"));
    app.run_command("explorer.move", CmdArgs::None);
    let d = app.dialog.as_mut().expect("move prompt");
    if let crate::ui::dialog::DialogWidget::Prompt(p) = &mut d.widget {
        p.input.set("api/link");
    }
    key(&mut app, KeyCode::Enter);
    assert!(api.join("x.txt").is_file());
    assert!(!web.join("sub/x.txt").exists());
    let err = match app.dialog.as_ref().map(|d| &d.widget) {
        Some(crate::ui::dialog::DialogWidget::Prompt(p)) => p.error.clone().unwrap_or_default(),
        _ => notices(&app),
    };
    assert!(err.contains("between workspace folders"), "{err}");
    key(&mut app, KeyCode::Esc);
    // Copy into the link still works.
    select(&mut app, &api.join("y.txt"));
    app.run_command("explorer.copy", CmdArgs::None);
    select(&mut app, &api.join("link"));
    app.run_command("explorer.paste", CmdArgs::None);
    assert!(web.join("sub/y.txt").is_file(), "{}", notices(&app));
    assert!(api.join("y.txt").is_file());
}

/// A root marked missing is not watched: one notice says it is missing, no watch error
/// stacks on it (D292).
#[test]
fn a_missing_root_is_not_watched() {
    let (d, roots) = folders(&[("api", &[("a.txt", "A\n")])]);
    let gone = d.path().canonicalize().unwrap().join("gone");
    let mut app = explorer_app(&[roots[0].clone(), gone.clone()]);
    app.start_watcher();
    app.watch_root();
    let n = notices(&app);
    assert_eq!(n.matches("gone").count(), 1, "{n}");
    assert!(n.contains("is missing"), "{n}");
    assert!(!app.explorer.watched.contains(&gone));
    assert!(app.explorer.watched.contains(&roots[0]));
}

/// Watches start before the explorer is shown (welcome screen): the missing root is still
/// named once, and showing the explorer later does not name it again (D292).
#[test]
fn a_missing_root_is_named_once_when_watches_start_before_the_explorer() {
    let (d, roots) = folders(&[("api", &[("a.txt", "A\n")])]);
    let gone = d.path().canonicalize().unwrap().join("gone");
    let mut app = ws_app(&[roots[0].clone(), gone.clone()]);
    app.start_watcher();
    app.watch_root();
    app.run_command("explorer.focus", CmdArgs::None);
    let n = notices(&app);
    assert_eq!(n.matches("gone").count(), 1, "{n}");
    assert!(n.contains("is missing"), "{n}");
    assert!(!app.explorer.watched.contains(&gone));
    let last = app.explorer.rows.last().unwrap();
    assert_eq!((last.path.clone(), last.missing), (gone, true));
}

// ---- tab bar: double click reveals the file in the explorer (T25) ---------------------------

/// Left press + release on the tab of document `i` at time `at` (drawn at 100x30 first).
fn tab_click(app: &mut App, i: usize, at: Instant) {
    draw(app, 100, 30);
    let t = app.layout.tabs;
    let (_, x, _) = crate::ui::tabs::positions(app, t.width)
        .into_iter()
        .find(|(j, ..)| *j == i)
        .unwrap();
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        app.handle_mouse_at(
            MouseEvent {
                kind,
                column: t.x + x + 1,
                row: t.y,
                modifiers: KeyModifiers::NONE,
            },
            at,
        );
    }
}

fn two_tabs() -> (tempfile::TempDir, Vec<PathBuf>, App) {
    let (d, roots) = folders(&[
        ("api", &[("src/a.rs", "a\n")]),
        ("web", &[("src/deep/b.ts", "b\n"), ("c.ts", "c\n")]),
    ]);
    let mut app = ws_app(&roots);
    app.open_path(&roots[0].join("src/a.rs")).unwrap();
    app.open_path(&roots[1].join("src/deep/b.ts")).unwrap();
    app.explorer.visible = false;
    app.focus = Focus::Editor;
    (d, roots, app)
}

#[test]
fn double_click_on_a_tab_reveals_its_file_in_the_hidden_explorer() {
    let (_d, roots, mut app) = two_tabs();
    let t0 = Instant::now();
    tab_click(&mut app, 0, t0);
    assert!(!app.explorer.visible, "a single click leaves the explorer");
    assert_eq!(app.active, 0);
    tab_click(&mut app, 0, t0 + Duration::from_millis(300));
    assert!(app.explorer.visible);
    assert_eq!(app.focus, Focus::Explorer);
    let cur = &app.explorer.rows[app.explorer.cursor];
    assert_eq!(cur.path, roots[0].join("src/a.rs"));
}

#[test]
fn double_click_picks_the_root_of_the_tab_and_activates_it() {
    let (_d, roots, mut app) = two_tabs();
    app.run_command("explorer.focus", CmdArgs::None);
    let t0 = Instant::now();
    tab_click(&mut app, 0, t0);
    tab_click(&mut app, 1, t0 + Duration::from_millis(100));
    assert_eq!(
        app.focus,
        Focus::Editor,
        "different tabs are two single clicks"
    );
    tab_click(&mut app, 1, t0 + Duration::from_millis(200));
    assert_eq!(app.active, 1);
    assert_eq!(app.focus, Focus::Explorer);
    let cur = &app.explorer.rows[app.explorer.cursor];
    assert_eq!(cur.path, roots[1].join("src/deep/b.ts"));
    assert!(
        tree(&app).iter().any(|r| r.trim() == "deep"),
        "{:?}",
        tree(&app)
    );
}

#[test]
fn slow_second_click_is_a_single_click_and_a_triple_is_not_two_doubles() {
    let (_d, _r, mut app) = two_tabs();
    let t0 = Instant::now();
    tab_click(&mut app, 0, t0);
    tab_click(&mut app, 0, t0 + Duration::from_millis(401));
    assert!(!app.explorer.visible);
    // That second press started a new pair; the third completes it.
    tab_click(&mut app, 0, t0 + Duration::from_millis(500));
    assert!(app.explorer.visible);
    app.explorer.visible = false;
    app.focus = Focus::Editor;
    tab_click(&mut app, 0, t0 + Duration::from_millis(600));
    assert!(!app.explorer.visible, "a double click consumes its presses");
}

#[test]
fn double_click_on_a_tab_without_a_file_does_nothing_extra() {
    let (_d, _r, mut app) = two_tabs();
    let (cmd, arg) = crate::commands::slash::parse("/keys").unwrap();
    (cmd.run)(&mut app, &arg).unwrap();
    let keys = app.docs.len() - 1;
    assert!(app.docs[keys].path.is_none());
    app.explorer.visible = false;
    app.focus = Focus::Editor;
    let t0 = Instant::now();
    tab_click(&mut app, keys, t0);
    tab_click(&mut app, keys, t0 + Duration::from_millis(100));
    assert_eq!(app.active, keys);
    assert!(!app.explorer.visible);
    assert_eq!(app.focus, Focus::Editor);
}

#[test]
fn double_click_on_a_hidden_file_tab_keeps_the_reveal_rule() {
    let (d, roots) = folders(&[("p", &[(".github/ci.yml", "x\n"), ("a.txt", "a\n")])]);
    let _ = &d;
    let mut app = ws_app(&roots);
    app.open_path(&roots[0].join(".github/ci.yml")).unwrap();
    app.run_command("explorer.toggle_hidden", CmdArgs::None);
    assert!(!app.explorer.tree.show_hidden());
    app.explorer.visible = false;
    app.focus = Focus::Editor;
    app.notices.clear();
    let t0 = Instant::now();
    tab_click(&mut app, 0, t0);
    tab_click(&mut app, 0, t0 + Duration::from_millis(50));
    assert!(app.explorer.visible);
    assert_eq!(app.focus, Focus::Explorer);
    assert!(!app.explorer.expanded.contains(&roots[0].join(".github")));
    assert!(notices(&app).contains("ctrl+h"), "{}", notices(&app));
}

/// One mouse event of `kind` at a cell, at time `at`.
fn mouse_at(app: &mut App, kind: MouseEventKind, column: u16, row: u16, at: Instant) {
    app.handle_mouse_at(
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        },
        at,
    );
}

#[test]
fn a_drag_the_wheel_or_a_press_elsewhere_between_two_tab_presses_breaks_the_pair() {
    let (_d, _r, mut app) = two_tabs();
    let ms = |n: u64| Duration::from_millis(n);
    let t0 = Instant::now();
    let editor = |app: &App| (app.layout.editor.x + 8, app.layout.editor.y + 1);
    // A press on the tab that starts a drag, then a press on it again (review T25 C1).
    tab_click(&mut app, 0, t0);
    let (ex, ey) = editor(&app);
    mouse_at(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        ex,
        ey,
        t0 + ms(30),
    );
    tab_click(&mut app, 0, t0 + ms(60));
    assert!(!app.explorer.visible, "drag in between");
    // The wheel in the editor in between.
    tab_click(&mut app, 0, t0 + ms(1000));
    mouse_at(&mut app, MouseEventKind::ScrollDown, ex, ey, t0 + ms(1030));
    tab_click(&mut app, 0, t0 + ms(1060));
    assert!(!app.explorer.visible, "wheel in between");
    // A press in the editor in between.
    tab_click(&mut app, 0, t0 + ms(2000));
    mouse_at(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        ex,
        ey,
        t0 + ms(2030),
    );
    mouse_at(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        ex,
        ey,
        t0 + ms(2040),
    );
    tab_click(&mut app, 0, t0 + ms(2060));
    assert!(
        !app.explorer.visible,
        "press outside the tab bar in between"
    );
    // A pointer move in between is not a click: the pair stands.
    tab_click(&mut app, 0, t0 + ms(3000));
    mouse_at(&mut app, MouseEventKind::Moved, ex, ey, t0 + ms(3030));
    tab_click(&mut app, 0, t0 + ms(3060));
    assert!(app.explorer.visible, "a move does not break the pair");
}

#[test]
fn a_tab_scrolled_by_the_first_press_still_takes_the_second() {
    let names: Vec<String> = (0..12)
        .map(|i| format!("a_rather_long_file_name_{i:02}.txt"))
        .collect();
    let files: Vec<(&str, &str)> = names.iter().map(|n| (n.as_str(), "x\n")).collect();
    let (_d, roots) = folders(&[("p", &files)]);
    let mut app = ws_app(&roots);
    for n in &names {
        app.open_path(&roots[0].join(n)).unwrap();
    }
    app.explorer.visible = false;
    app.focus = Focus::Editor;
    app.active = 0;
    draw(&mut app, 100, 30);
    let width = app.layout.tabs.width;
    // The last tab on screen is cut at the right edge.
    let before = crate::ui::tabs::positions(&app, width);
    let &(k, x, w) = before.last().unwrap();
    assert!(x + w == width && k + 1 < app.docs.len(), "{before:?}");
    let t0 = Instant::now();
    tab_click(&mut app, k, t0);
    assert_eq!(app.active, k);
    let after = crate::ui::tabs::positions(&app, width);
    let moved = after.iter().find(|(j, ..)| *j == k).unwrap().1;
    assert_ne!(moved, x, "the first press scrolled the bar");
    // The second press on that tab, where it is now, completes the double click.
    tab_click(&mut app, k, t0 + Duration::from_millis(200));
    assert_eq!(app.active, k);
    assert!(app.explorer.visible);
    assert_eq!(app.focus, Focus::Explorer);
    let cur = &app.explorer.rows[app.explorer.cursor];
    assert_eq!(cur.path, roots[0].join(&names[k]));
}
