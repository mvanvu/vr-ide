//! Session store (D1–D3, SEC-6): the `--session` start, the debounced and quit saves, the
//! restore and the checks on the file.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use vr_config::{Keymap, Settings, TerminalPosition};

use crate::app::{App, NoticeLevel};
use crate::clipboard::MemoryClipboard;
use crate::session::{self, AiSection, AiSession, BAD_FILE, SESSION_FILE, Session, TabState};
use crate::workspace::Workspace;

struct Tree {
    _dir: tempfile::TempDir,
    base: PathBuf,
    state: PathBuf,
    home: PathBuf,
    api: PathBuf,
    web: PathBuf,
}

/// `home/`, `api/` (src/main.rs, src/lib.rs, docs/), `web/` (index.html) and an empty
/// state folder name under one temp dir.
fn tree() -> Tree {
    let d = tempfile::tempdir().unwrap();
    let base = d.path().canonicalize().unwrap();
    let home = base.join("home");
    let api = base.join("api");
    let web = base.join("web");
    for dir in [&home, &api.join("src"), &api.join("docs"), &web] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(api.join("src/main.rs"), "fn main() {\n    run();\n}\n").unwrap();
    std::fs::write(api.join("src/lib.rs"), "pub fn run() {}\n").unwrap();
    std::fs::write(web.join("index.html"), "<p>hi</p>\n").unwrap();
    Tree {
        state: base.join("state"),
        _dir: d,
        base,
        home,
        api,
        web,
    }
}

fn app_on(ws: Workspace) -> App {
    let mut app = App::with_workspace(
        ws,
        Keymap::load(None).0,
        Settings::default(),
        Box::new(MemoryClipboard::new()),
    );
    app.enter_settle = Duration::ZERO;
    app.enter_repeat_gap = Duration::ZERO;
    app.terminal.shell = Some(PathBuf::from("/bin/sh"));
    app
}

/// A `--session` window as `main` starts it: lock and load, the app on the session's
/// workspace, then the restore.
fn launch(state: &Path, home: &Path) -> App {
    let started = session::start(state, home);
    let mut app = app_on(started.workspace);
    session::restore(&mut app, started.restore);
    app
}

fn texts(app: &App) -> Vec<String> {
    app.notices.iter().map(|n| n.text.clone()).collect()
}

fn close(mut app: App) {
    crate::commands::terminal::shutdown(&mut app);
}

fn write_session(state: &Path, body: &str) {
    session::create_private_dir_for_tests(state);
    let p = state.join(SESSION_FILE);
    std::fs::write(&p, body).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
}

fn mode(p: &Path) -> u32 {
    std::fs::symlink_metadata(p).unwrap().permissions().mode() & 0o777
}

#[test]
fn session_without_a_saved_one_opens_home_only() {
    let t = tree();
    let app = launch(&t.state, &t.home);
    assert_eq!(app.workspace.paths(), std::slice::from_ref(&t.home));
    assert!(app.docs.is_empty());
    assert!(app.terminal.tabs.is_empty());
    assert!(app.notices.is_empty(), "{:?}", texts(&app));
    // Nothing is written before a change or the quit.
    assert!(!t.state.join(SESSION_FILE).exists());
    close(app);
}

#[test]
fn session_restores_roots_tabs_explorer_and_terminal_layout() {
    let t = tree();
    let mut app = launch(&t.state, &t.home);
    app.add_root(&t.api).unwrap();
    app.add_root(&t.web).unwrap();
    let main_rs = t.api.join("src/main.rs");
    app.open_path(&main_rs).unwrap();
    {
        let doc = &mut app.docs[0];
        let at = doc.buffer.text().line_to_char(1) + 4;
        doc.buffer.set_selection(vr_core::Selection::point(at));
        app.views[0].top = 1;
        app.views[0].left = 2;
    }
    app.open_path(&t.web.join("index.html")).unwrap();
    app.set_active(0);
    app.sync_mode();
    app.enter_edit_mode();
    crate::commands::explorer::ensure_loaded(&mut app);
    app.explorer.expand(&t.api.join("src")).unwrap();
    app.terminal.position = TerminalPosition::Right;
    app.terminal.width_percent = 40;
    app.terminal.height_percent = 25;
    assert!(crate::commands::terminal::add_tab_in(
        &mut app,
        &t.api.join("docs")
    ));
    app.terminal.visible = true;
    session::save_on_quit(&mut app);
    close(app);

    // Edited outside vr-ide since: the tab reads the file as it is now.
    std::fs::write(&main_rs, "fresh();\nsecond line here\nthird\n").unwrap();
    let app = launch(&t.state, &t.home);
    assert_eq!(
        app.workspace.paths(),
        [t.home.clone(), t.api.clone(), t.web.clone()]
    );
    let paths: Vec<PathBuf> = app.docs.iter().map(|d| d.path.clone().unwrap()).collect();
    assert_eq!(paths, [main_rs.clone(), t.web.join("index.html")]);
    assert_eq!(app.active, 0);
    assert!(
        app.docs[0]
            .buffer
            .text()
            .to_string()
            .starts_with("fresh();")
    );
    let text = app.docs[0].buffer.text();
    let head = app.docs[0].buffer.selection().primary().head;
    assert_eq!(head, text.line_to_char(1) + 4);
    assert_eq!((app.views[0].top, app.views[0].left), (1, 2));
    assert!(app.edit_mode(), "the active tab was in edit mode");
    assert!(app.explorer.expanded.contains(&t.api.join("src")));
    assert_eq!(app.terminal.position, TerminalPosition::Right);
    assert_eq!(app.terminal.width_percent, 40);
    assert_eq!(app.terminal.height_percent, 25);
    assert!(app.terminal.visible);
    assert_eq!(app.terminal.tabs.len(), 1);
    // A fresh shell in the terminal's folder.
    let pid = app.terminal.tabs[0].pid().unwrap();
    assert_eq!(
        std::fs::read_link(format!("/proc/{pid}/cwd")).unwrap(),
        t.api.join("docs")
    );
    assert!(app.notices.is_empty(), "{:?}", texts(&app));
    close(app);
}

#[test]
fn session_file_is_private_and_replaces_a_symlink_without_following_it() {
    let t = tree();
    let state = t.state.join("deep/er");
    // A temp file left by a window killed during a save goes when the lock is taken.
    session::create_private_dir_for_tests(&state);
    let stale = state.join(".session.toml.999999-0.tmp");
    std::fs::write(&stale, "half").unwrap();
    let mut app = launch(&state, &t.home);
    assert!(!stale.exists());
    app.open_path(&t.home.join("../api/src/lib.rs")).ok();
    session::save_on_quit(&mut app);
    let file = state.join(SESSION_FILE);
    assert_eq!(mode(&state), 0o700);
    assert_eq!(mode(&file), 0o600);
    let leftovers: Vec<_> = std::fs::read_dir(&state)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");

    let target = t.base.join("target.txt");
    std::fs::write(&target, "keep").unwrap();
    std::fs::remove_file(&file).unwrap();
    std::os::unix::fs::symlink(&target, &file).unwrap();
    session::save_on_quit(&mut app);
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep");
    assert!(std::fs::symlink_metadata(&file).unwrap().is_file());
    assert_eq!(mode(&file), 0o600);
    close(app);
}

#[test]
fn a_change_is_saved_within_two_seconds() {
    let t = tree();
    let mut app = launch(&t.state, &t.home);
    let file = t.state.join(SESSION_FILE);
    // First the window as it opened is saved, so the change below starts from a done save.
    let t0 = Instant::now();
    let mut at = t0;
    while !file.exists() && at < t0 + Duration::from_secs(5) {
        app.tick(at);
        session::flush(&mut app);
        at += Duration::from_millis(100);
    }
    let before = std::fs::read_to_string(&file).unwrap();
    assert!(!before.contains("index.html"), "{before}");
    // A change right after that save.
    let changed = at;
    app.open_path(&t.web.join("index.html")).unwrap();
    let mut saved_at = None;
    for step in 0..=20 {
        let now = changed + Duration::from_millis(100 * step);
        app.tick(now);
        session::flush(&mut app);
        if std::fs::read_to_string(&file).unwrap() != before {
            saved_at = Some(now - changed);
            break;
        }
    }
    let took = saved_at.expect("the change was never saved");
    assert!(took < Duration::from_secs(2), "{took:?}");
    let body = std::fs::read_to_string(&file).unwrap();
    assert!(body.contains("index.html"), "{body}");
    close(app);
}

#[test]
fn a_corrupt_file_gives_one_notice_opens_home_and_is_kept_as_bad() {
    let t = tree();
    let body = format!("roots = [\"{}\"]\n\n\n[[tabs]\n", t.api.display());
    write_session(&t.state, &body);
    let app = launch(&t.state, &t.home);
    assert_eq!(app.workspace.paths(), std::slice::from_ref(&t.home));
    assert_eq!(
        texts(&app),
        ["Session file unreadable (line 4); opened ~/, kept it as session.toml.bad"]
    );
    assert_eq!(app.notices[0].level, NoticeLevel::Error);
    let bad = t.state.join(BAD_FILE);
    assert_eq!(std::fs::read_to_string(&bad).unwrap(), body);
    assert_eq!(mode(&bad), 0o600);
    assert!(!t.state.join(SESSION_FILE).exists());
    close(app);

    // A number no field can hold is a parse error, never a panic.
    write_session(&t.state, "[[tabs]]\npath = \"/x\"\nline = 1e30\n");
    let app = launch(&t.state, &t.home);
    assert_eq!(app.workspace.paths(), std::slice::from_ref(&t.home));
    assert_eq!(app.notices.len(), 1, "{:?}", texts(&app));
    assert!(texts(&app)[0].starts_with("Session file unreadable (line 3)"));
    close(app);
}

/// Puts one kind of unsafe session file in place.
type MakeFile<'a> = Box<dyn Fn() + 'a>;

#[test]
fn unsafe_session_files_are_ignored_with_one_notice() {
    let t = tree();
    let good = format!("roots = [\"{}\"]\n", t.api.display());
    let file = t.state.join(SESSION_FILE);
    let cases: Vec<(&str, MakeFile)> = vec![
        (
            "larger than 1 MiB",
            Box::new(|| {
                let mut big = good.clone();
                big.push_str(&format!("# {}\n", "x".repeat(2 << 20)));
                write_session(&t.state, &big);
            }),
        ),
        (
            "writable by others",
            Box::new(|| {
                write_session(&t.state, &good);
                std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o664)).unwrap();
            }),
        ),
        (
            "not a regular file",
            Box::new(|| {
                let real = t.base.join("real.toml");
                std::fs::write(&real, &good).unwrap();
                std::os::unix::fs::symlink(&real, &file).unwrap();
            }),
        ),
        (
            "not a regular file",
            Box::new(|| {
                let st = std::process::Command::new("mkfifo")
                    .arg(&file)
                    .status()
                    .unwrap();
                assert!(st.success());
            }),
        ),
    ];
    for (reason, make) in cases {
        let _ = std::fs::remove_file(&file);
        make();
        let app = launch(&t.state, &t.home);
        assert_eq!(
            app.workspace.paths(),
            std::slice::from_ref(&t.home),
            "{reason}"
        );
        assert_eq!(
            texts(&app),
            [format!("Session file ignored ({reason}); opened ~/")]
        );
        close(app);
    }
}

#[test]
fn numbers_are_clamped_and_counts_capped() {
    let mut s = Session {
        roots: (0..100).map(|i| PathBuf::from(format!("/r/{i}"))).collect(),
        tabs: (0..300)
            .map(|i| TabState {
                path: PathBuf::from(format!("/r/0/{i}.rs")),
                ..TabState::default()
            })
            .collect(),
        expanded: (0..3000)
            .map(|i| PathBuf::from(format!("/r/0/{i}")))
            .collect(),
        active_tab: 999,
        ..Session::default()
    };
    s.terminal.height_percent = Some(500);
    s.terminal.width_percent = Some(1);
    s.terminal.folders = (0..100).map(|i| PathBuf::from(format!("/r/{i}"))).collect();
    s.terminal.active = 77;
    let s = session::sanitize(s);
    assert_eq!(s.roots.len(), session::MAX_ROOTS);
    assert_eq!(s.tabs.len(), session::MAX_TABS);
    assert_eq!(s.expanded.len(), session::MAX_EXPANDED);
    assert_eq!(s.active_tab, session::MAX_TABS - 1);
    assert_eq!(s.terminal.height_percent, Some(90));
    assert_eq!(s.terminal.width_percent, Some(10));
    assert_eq!(s.terminal.folders.len(), session::MAX_TERMINALS);
    assert_eq!(s.terminal.active, session::MAX_TERMINALS - 1);

    // A cursor far past the end of a file lands at its end.
    let t = tree();
    let body = format!(
        "roots = [\"{api}\"]\n[[tabs]]\npath = \"{api}/src/lib.rs\"\nline = 4000000000\n\
         column = 4000000000\ntop = 4000000000\nleft = 4000000000\n",
        api = t.api.display()
    );
    write_session(&t.state, &body);
    let app = launch(&t.state, &t.home);
    assert_eq!(app.docs.len(), 1);
    let len = app.docs[0].buffer.text().len_chars();
    assert!(app.docs[0].buffer.selection().primary().head <= len);
    assert!(app.views[0].top < app.docs[0].buffer.text().len_lines());
    close(app);
}

#[test]
fn missing_file_tabs_are_skipped_with_one_notice() {
    let t = tree();
    let api = t.api.display();
    let body = format!(
        "roots = [\"{api}\"]\nactive_tab = 1\n\
         [[tabs]]\npath = \"{api}/src/old_layout.rs\"\n\
         [[tabs]]\npath = \"{api}/src/main.rs\"\n\
         [[tabs]]\npath = \"{api}/docs/plan.md\"\n"
    );
    write_session(&t.state, &body);
    let app = launch(&t.state, &t.home);
    assert_eq!(app.docs.len(), 1);
    assert_eq!(
        app.docs[0].path.as_deref(),
        Some(t.api.join("src/main.rs").as_path())
    );
    assert_eq!(
        texts(&app),
        ["Session restored; 2 tabs skipped, files not found: src/old_layout.rs, docs/plan.md"]
    );
    assert_eq!(app.notices[0].level, NoticeLevel::Warning);
    close(app);

    // More names than fit end with a count.
    let mut body = format!("roots = [\"{api}\"]\n");
    for i in 0..9 {
        body.push_str(&format!("[[tabs]]\npath = \"{api}/gone{i}.rs\"\n"));
    }
    write_session(&t.state, &body);
    let app = launch(&t.state, &t.home);
    assert_eq!(texts(&app).len(), 1);
    let text = &texts(&app)[0];
    assert!(
        text.starts_with("Session restored; 9 tabs skipped, files not found: gone0.rs, "),
        "{text}"
    );
    assert!(text.ends_with("… +4"), "{text}");
    close(app);
}

#[test]
fn tabs_outside_the_roots_or_not_regular_files_are_skipped() {
    let t = tree();
    std::fs::write(t.base.join("outside.txt"), "x").unwrap();
    std::os::unix::fs::symlink(t.base.join("outside.txt"), t.api.join("link.txt")).unwrap();
    let st = std::process::Command::new("mkfifo")
        .arg(t.api.join("pipe"))
        .status()
        .unwrap();
    assert!(st.success());
    let api = t.api.display();
    let base = t.base.display();
    let body = format!(
        "roots = [\"{api}\", \"relative/root\"]\n\
         [[tabs]]\npath = \"{base}/outside.txt\"\n\
         [[tabs]]\npath = \"{api}/link.txt\"\n\
         [[tabs]]\npath = \"{api}/pipe\"\n\
         [[tabs]]\npath = \"{api}/src/../../outside.txt\"\n\
         [[tabs]]\npath = \"{api}/src\"\n\
         [[tabs]]\npath = \"{api}/src/lib.rs\"\n"
    );
    write_session(&t.state, &body);
    let app = launch(&t.state, &t.home);
    assert_eq!(app.workspace.paths(), std::slice::from_ref(&t.api));
    assert_eq!(app.docs.len(), 1);
    assert_eq!(texts(&app).len(), 1, "{:?}", texts(&app));
    assert!(texts(&app)[0].starts_with("Session restored; 5 tabs skipped"));
    close(app);
}

#[test]
fn a_second_session_window_runs_without_saving() {
    let t = tree();
    let mut first = launch(&t.state, &t.home);
    let mut second = launch(&t.state, &t.home);
    assert_eq!(
        texts(&second),
        ["Another vr-ide --session window keeps the session; this one is not saved"]
    );
    second.open_path(&t.web.join("index.html")).unwrap();
    session::save_on_quit(&mut second);
    assert!(!t.state.join(SESSION_FILE).exists());
    close(second);
    first.add_root(&t.api).unwrap();
    first.open_path(&t.api.join("src/lib.rs")).unwrap();
    session::save_on_quit(&mut first);
    let body = std::fs::read_to_string(t.state.join(SESSION_FILE)).unwrap();
    assert!(
        body.contains("lib.rs") && !body.contains("index.html"),
        "{body}"
    );
    close(first);
    // The lock goes with the window that held it.
    let third = launch(&t.state, &t.home);
    assert!(third.notices.is_empty(), "{:?}", texts(&third));
    close(third);
}

/// A child forked by another thread right now, kept between fork and exec for `pause`: until
/// it execs it holds a copy of every descriptor this process has open (close-on-exec ones
/// included), as a terminal or agent started meanwhile by another window or thread does.
/// Returns once the child exists; join the handle to wait for it.
pub(crate) fn child_paused_before_exec(pause: Duration) -> std::thread::JoinHandle<()> {
    use std::io::Read;
    use std::os::fd::{AsRawFd, BorrowedFd};
    use std::os::unix::process::CommandExt;
    let (mut forked, tell) = std::io::pipe().unwrap();
    let child = std::thread::spawn(move || {
        let fd = tell.as_raw_fd();
        let mut cmd = std::process::Command::new("/bin/true");
        // SAFETY: only async-signal-safe calls (write, nanosleep) run in the forked child.
        unsafe {
            cmd.pre_exec(move || {
                let _ = rustix::io::write(BorrowedFd::borrow_raw(fd), b"x");
                std::thread::sleep(pause);
                Ok(())
            });
        }
        cmd.status().unwrap();
        drop(tell);
    });
    let mut b = [0u8; 1];
    forked.read_exact(&mut b).unwrap();
    child
}

/// The lock is free as soon as its window quits, even while a child forked before that (by
/// another window's terminal, say) has not started its program yet.
#[test]
fn the_lock_is_free_once_its_window_quits_despite_a_forked_child() {
    let t = tree();
    let first = launch(&t.state, &t.home);
    assert!(first.notices.is_empty(), "{:?}", texts(&first));
    let child = child_paused_before_exec(Duration::from_millis(400));
    close(first);
    let second = launch(&t.state, &t.home);
    assert!(second.notices.is_empty(), "{:?}", texts(&second));
    close(second);
    child.join().unwrap();
}

/// A window that quits with a save still queued (its state folder hangs) keeps the lock until
/// that write is done: a window started meanwhile does not save, and the late write never
/// lands after another window took over.
#[test]
fn a_quit_window_keeps_the_lock_while_its_last_write_is_pending() {
    let t = tree();
    let mut first = launch(&t.state, &t.home);
    assert!(first.notices.is_empty(), "{:?}", texts(&first));
    let go = session::pause_writer_for_tests(&first);
    first.add_root(&t.api).unwrap();
    session::queue_save_for_tests(&mut first);
    close(first);
    let second = launch(&t.state, &t.home);
    assert_eq!(
        texts(&second),
        ["Another vr-ide --session window keeps the session; this one is not saved"]
    );
    close(second);
    // The folder answers again: the write lands, then the lock is free.
    drop(go);
    let lock = std::fs::File::open(t.state.join(session::LOCK_FILE)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while lock.try_lock().is_err() {
        assert!(Instant::now() < deadline, "the lock was never freed");
        std::thread::sleep(Duration::from_millis(5));
    }
    lock.unlock().unwrap();
    let body = std::fs::read_to_string(t.state.join(SESSION_FILE)).unwrap();
    assert!(body.contains(&*t.api.to_string_lossy()), "{body}");
    let third = launch(&t.state, &t.home);
    assert!(third.notices.is_empty(), "{:?}", texts(&third));
    close(third);
}

#[test]
fn the_file_holds_no_buffer_text_and_no_commands() {
    let t = tree();
    let mut app = launch(&t.state, &t.home);
    app.add_root(&t.api).unwrap();
    app.open_path(&t.api.join("src/lib.rs")).unwrap();
    app.docs[0]
        .buffer
        .set_selection(vr_core::Selection::point(0));
    let mut untitled = vr_core::Document::untitled();
    untitled.buffer = vr_core::Buffer::from_str("TOP-SECRET-TEXT");
    app.docs.push(untitled);
    app.views.push(crate::app::View::default());
    assert!(crate::commands::terminal::add_tab_in(&mut app, &t.api));
    crate::commands::terminal::type_line(
        &mut app,
        "echo TYPED-COMMAND",
        crate::commands::terminal::LineTarget::Active,
    )
    .ok();
    session::save_on_quit(&mut app);
    let body = std::fs::read_to_string(t.state.join(SESSION_FILE)).unwrap();
    assert!(!body.contains("TOP-SECRET-TEXT"), "{body}");
    assert!(!body.contains("TYPED-COMMAND"), "{body}");
    assert!(!body.contains("ssh"), "{body}");
    assert_eq!(body.matches("[[tabs]]").count(), 1, "{body}");
    close(app);
}

#[test]
fn the_ai_section_is_kept_for_the_ai_panel() {
    let t = tree();
    let api = t.api.display();
    let body = format!(
        "roots = [\"{api}\"]\n[ai]\nshown = true\nwidth = 35\n\
         [[ai.sessions]]\ntool = \"claude\"\naccount = \"work\"\nfolder = \"{api}\"\nlabel = \"fix\"\n\
         [[ai.sessions]]\ntool = \"bad\\u001b\"\naccount = \"x\"\nfolder = \"{api}\"\nlabel = \"y\"\n"
    );
    write_session(&t.state, &body);
    let mut app = launch(&t.state, &t.home);
    let ai = session::take_restored_ai(&mut app).unwrap();
    assert!(ai.shown);
    assert_eq!(ai.width, Some(35));
    assert_eq!(
        ai.sessions,
        [AiSession {
            tool: "claude".into(),
            account: "work".into(),
            folder: t.api.clone(),
            label: "fix".into(),
        }]
    );
    assert!(session::take_restored_ai(&mut app).is_none(), "given once");
    // Written back as read until the panel reports its own state.
    session::save_on_quit(&mut app);
    let saved = std::fs::read_to_string(t.state.join(SESSION_FILE)).unwrap();
    assert!(saved.contains("label = \"fix\""), "{saved}");
    session::set_ai(
        &mut app,
        AiSection {
            shown: false,
            width: None,
            sessions: Vec::new(),
        },
    );
    session::save_on_quit(&mut app);
    let saved = std::fs::read_to_string(t.state.join(SESSION_FILE)).unwrap();
    assert!(!saved.contains("claude"), "{saved}");
    close(app);
}

#[test]
fn console_and_path_launches_never_touch_the_session() {
    let t = tree();
    let l = crate::cli::resolve(&[], &t.api).unwrap();
    assert!(!l.session);
    let l = crate::cli::resolve(&[t.web.clone().into_os_string()], &t.api).unwrap();
    assert!(!l.session);
    // Without `--session` main never starts a keeper: ticks and the quit write nothing.
    let mut app = app_on(l.workspace);
    assert!(app.session.is_none());
    app.open_path(&t.web.join("index.html")).unwrap();
    let t0 = Instant::now();
    app.tick(t0);
    app.tick(t0 + Duration::from_secs(5));
    session::save_on_quit(&mut app);
    assert!(!t.state.exists());
    close(app);
}

#[test]
fn an_open_existing_state_dir_is_tightened_to_0700() {
    let t = tree();
    std::fs::create_dir_all(&t.state).unwrap();
    std::fs::set_permissions(&t.state, std::fs::Permissions::from_mode(0o775)).unwrap();
    let mut app = launch(&t.state, &t.home);
    assert!(app.notices.is_empty(), "{:?}", texts(&app));
    assert_eq!(mode(&t.state), 0o700);
    session::save_on_quit(&mut app);
    assert_eq!(mode(&t.state.join(SESSION_FILE)), 0o600);
    close(app);
}

#[test]
fn a_symlinked_state_dir_is_never_written() {
    let t = tree();
    let real = t.base.join("real-state");
    session::create_private_dir_for_tests(&real);
    std::os::unix::fs::symlink(&real, &t.state).unwrap();
    let mut app = launch(&t.state, &t.home);
    assert_eq!(texts(&app).len(), 1, "{:?}", texts(&app));
    assert!(
        texts(&app)[0].starts_with("Session not saved: "),
        "{:?}",
        texts(&app)
    );
    app.open_path(&t.home.join("../web/index.html")).ok();
    let t0 = Instant::now();
    for step in 0..30 {
        app.tick(t0 + Duration::from_millis(100 * step));
    }
    session::save_on_quit(&mut app);
    let left: Vec<_> = std::fs::read_dir(&real).unwrap().collect();
    assert!(left.is_empty(), "{left:?}");
    close(app);
}

#[test]
fn an_exited_terminal_keeps_the_folder_it_started_in() {
    let t = tree();
    let mut app = launch(&t.state, &t.home);
    app.add_root(&t.api).unwrap();
    app.terminal.shell = Some(PathBuf::from("/bin/true"));
    assert!(crate::commands::terminal::add_tab_in(
        &mut app,
        &t.api.join("docs")
    ));
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.terminal.tabs[0].running() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!app.terminal.tabs[0].running());
    let s = session::snapshot(&app);
    assert_eq!(s.terminal.folders, [t.api.join("docs")]);
    close(app);
}

#[test]
fn a_failed_write_is_retried_at_the_next_check() {
    let t = tree();
    let mut app = launch(&t.state, &t.home);
    app.add_root(&t.api).unwrap();
    // The state folder becomes a file: writes fail until it is a folder again.
    std::fs::remove_dir_all(&t.state).unwrap();
    std::fs::write(&t.state, "not a folder").unwrap();
    app.open_path(&t.api.join("src/lib.rs")).unwrap();
    let t0 = Instant::now();
    for step in 0..20 {
        app.tick(t0 + Duration::from_millis(100 * step));
        session::flush(&mut app);
    }
    assert!(
        texts(&app)
            .iter()
            .any(|n| n.starts_with("Session not saved: ")),
        "{:?}",
        texts(&app)
    );
    std::fs::remove_file(&t.state).unwrap();
    let t1 = t0 + Duration::from_secs(10);
    for step in 0..20 {
        app.tick(t1 + Duration::from_millis(100 * step));
        session::flush(&mut app);
    }
    let body = std::fs::read_to_string(t.state.join(SESSION_FILE)).unwrap();
    assert!(body.contains("lib.rs"), "{body}");
    close(app);
}

#[test]
fn the_explorer_never_expands_through_a_link_out_of_the_root() {
    let t = tree();
    std::fs::create_dir_all(t.base.join("out/x")).unwrap();
    std::os::unix::fs::symlink(t.base.join("out"), t.api.join("inlink")).unwrap();
    let api = t.api.display();
    write_session(
        &t.state,
        &format!("roots = [\"{api}\"]\nexpanded = [\"{api}/inlink/x\", \"{api}/src\"]\n"),
    );
    let app = launch(&t.state, &t.home);
    assert!(app.explorer.expanded.contains(&t.api.join("src")));
    assert!(!app.explorer.expanded.contains(&t.api.join("inlink/x")));
    close(app);
}

#[test]
fn edit_mode_left_for_the_quit_question_is_saved_as_it_was() {
    let t = tree();
    let mut app = launch(&t.state, &t.home);
    app.add_root(&t.api).unwrap();
    app.open_path(&t.api.join("src/lib.rs")).unwrap();
    app.sync_mode();
    app.enter_edit_mode();
    app.tick(Instant::now());
    // The quit question took the keys (read mode), then quit was chosen.
    app.views[0].edit = false;
    app.quit = true;
    session::save_on_quit(&mut app);
    let body = std::fs::read_to_string(t.state.join(SESSION_FILE)).unwrap();
    assert!(body.contains("edit = true"), "{body}");

    // Read mode chosen by the user before the quit stays read mode.
    app.quit = false;
    app.tick(Instant::now() + Duration::from_secs(1));
    app.quit = true;
    session::save_on_quit(&mut app);
    let body = std::fs::read_to_string(t.state.join(SESSION_FILE)).unwrap();
    assert!(body.contains("edit = false"), "{body}");
    close(app);
}

/// Answers the open dialog the way a key press does: the dialog is taken out, answered, and
/// the mode rule runs at the end of the input.
fn answer_open_dialog(app: &mut App, button: usize) {
    let d = app.dialog.take().expect("a dialog is open");
    crate::commands::dialog::answer(app, d, crate::ui::dialog::Outcome::Chosen(button));
    app.sync_mode();
}

/// A file tab in edit mode, seen by the loop.
fn editing(t: &Tree) -> App {
    let mut app = launch(&t.state, &t.home);
    app.add_root(&t.api).unwrap();
    app.open_path(&t.api.join("src/lib.rs")).unwrap();
    app.sync_mode();
    app.enter_edit_mode();
    assert!(app.edit_mode());
    app.tick(Instant::now());
    app
}

fn quit_through_the_loop(app: &mut App) -> String {
    assert!(app.quit, "the answer quits");
    // The loop ticks once more before it sees the quit.
    app.tick(Instant::now() + Duration::from_secs(3));
    session::save_on_quit(app);
    std::fs::read_to_string(app_session_file(app)).unwrap()
}

fn app_session_file(app: &App) -> PathBuf {
    session::file_for_tests(app)
}

#[test]
fn edit_mode_is_kept_through_the_unsaved_files_question() {
    let t = tree();
    let mut app = editing(&t);
    app.handle_paste("x");
    assert!(app.docs[0].is_modified());
    app.tick(Instant::now() + Duration::from_millis(600));
    app.run_command("app.quit", crate::commands::CmdArgs::None);
    assert!(app.dialog.is_some(), "unsaved files ask first");
    answer_open_dialog(&mut app, 0);
    assert!(!app.edit_mode(), "the question left the tab in read mode");
    let body = quit_through_the_loop(&mut app);
    assert!(body.contains("edit = true"), "{body}");
    close(app);
}

#[test]
fn edit_mode_is_kept_through_the_running_terminals_question() {
    let t = tree();
    let mut app = editing(&t);
    assert!(crate::commands::terminal::add_tab_in(&mut app, &t.api));
    app.focus = crate::app::Focus::Editor;
    app.sync_mode();
    app.enter_edit_mode();
    app.tick(Instant::now() + Duration::from_millis(600));
    app.run_command("app.quit", crate::commands::CmdArgs::None);
    assert!(app.dialog.is_some(), "a running terminal asks first");
    answer_open_dialog(&mut app, 0);
    let body = quit_through_the_loop(&mut app);
    assert!(body.contains("edit = true"), "{body}");
    close(app);
}
