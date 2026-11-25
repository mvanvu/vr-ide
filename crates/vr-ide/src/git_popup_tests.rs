//! M7-3: the Git popup, Changes tab. Every action is compared with the same git command run on
//! a twin repo built by the same command sequence (AC-16f: `status --porcelain=v2`,
//! `diff --cached`, `log -1`); discard data safety, hostile names and messages, push / pull
//! against a bare remote, SEC-15..SEC-20, the three screen sizes. Real git, hermetic
//! environment (`git_tests::Fixture`).

use std::ffi::OsString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use vr_git::LineKind;

use crate::app::{App, NoticeLevel};
use crate::app_tests::test_app;
use crate::git_popup::{DiffRow, Pane};
use crate::git_tests::Fixture;
use crate::ui::safe_text::is_unsafe_char;
use crate::workspace::Workspace;

/// Identity and dates of every commit the tests make, through the popup or the CLI twin, so
/// the twins' commit ids are equal when the commits are.
pub(crate) const DATE: &str = "1700000000 +0000";

pub(crate) fn fixed_env(f: &Fixture) -> Vec<(OsString, OsString)> {
    let mut env = f.env();
    for (k, v) in [
        ("GIT_AUTHOR_NAME", "Ann Author"),
        ("GIT_AUTHOR_EMAIL", "ann@example.com"),
        ("GIT_COMMITTER_NAME", "Ann Author"),
        ("GIT_COMMITTER_EMAIL", "ann@example.com"),
        ("GIT_AUTHOR_DATE", DATE),
        ("GIT_COMMITTER_DATE", DATE),
    ] {
        env.push((k.into(), v.into()));
    }
    env
}

/// git in `dir` with the fixed identity and dates; panics on failure.
pub(crate) fn g(f: &Fixture, dir: &Path, args: &[&str]) -> String {
    let out = run_git(f, dir, args);
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

pub(crate) fn run_git(f: &Fixture, dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .current_dir(dir)
        .env_clear()
        .envs(fixed_env(f))
        .args(args)
        .output()
        .unwrap()
}

/// What AC-16f compares: status with blob ids and modes, the staged diff, the last commit.
fn repo_state(f: &Fixture, dir: &Path) -> (String, String, String) {
    (
        g(
            f,
            dir,
            &["status", "--porcelain=v2", "--untracked-files=all"],
        ),
        g(f, dir, &["diff", "--cached", "--no-color"]),
        run_git(f, dir, &["log", "-1", "--format=%H%n%B"])
            .stdout
            .iter()
            .map(|&b| b as char)
            .collect(),
    )
}

pub(crate) fn options(f: &Fixture) -> vr_git::Options {
    vr_git::Options {
        env: Some(fixed_env(f)),
        recovery_dir: Some(f.base.join("state/git-recovery")),
        ..vr_git::Options::default()
    }
}

pub(crate) fn app_on(f: &Fixture, ws: Workspace) -> App {
    let (mut app, _) = test_app(&ws.primary().path.clone());
    app.workspace = ws;
    app.roots_changed();
    app.git.options = options(f);
    app.git.debounce = Duration::ZERO;
    app.git.rest = Duration::ZERO;
    app.start_git();
    let roots = app.workspace.paths();
    wait(&mut app, "repos found", |a| {
        roots.iter().all(|r| a.git.repo_info_of_root(r).is_some())
    });
    app
}

pub(crate) fn popup_app(f: &Fixture) -> App {
    app_on(f, Workspace::single(f.dir.clone()))
}

pub(crate) fn wait(app: &mut App, what: &str, mut cond: impl FnMut(&App) -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        app.tick(Instant::now());
        if cond(app) {
            return;
        }
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Nothing pending: scanned, no write queued or running, no job, the diff answered.
pub(crate) fn idle(a: &App) -> bool {
    a.git_popup.as_ref().is_some_and(|p| {
        p.scan.is_some()
            && !p.scanning
            && p.rescan_at.is_none()
            && p.queue.is_empty()
            && p.job.is_none()
            && p.diff_pending == 0
            && (p.selected_entry().is_none() || p.diff.is_some())
    })
}

pub(crate) fn wait_idle(app: &mut App) {
    let end = Instant::now() + Duration::from_secs(20);
    while !idle(app) {
        app.tick(Instant::now());
        if Instant::now() > end {
            let p = popup(app);
            panic!(
                "popup not idle: scan {} scanning {} rescan_at {:?} queue {:?} job {} diff_pending {} selected {:?} diff {} notices {}",
                p.scan.is_some(),
                p.scanning,
                p.rescan_at,
                p.queue,
                p.job.is_some(),
                p.diff_pending,
                p.selected_entry().map(|e| e.path.clone()),
                p.diff.is_some(),
                notices(app)
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub(crate) fn open(app: &mut App) {
    crate::git_popup::open(app, "").unwrap();
    wait_idle(app);
}

pub(crate) fn key(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

pub(crate) fn shift(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::SHIFT));
}

pub(crate) fn ctrl(app: &mut App, c: char) {
    app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
}

pub(crate) fn typ(app: &mut App, s: &str) {
    for c in s.chars() {
        if c == '\n' {
            key(app, KeyCode::Enter);
        } else {
            key(app, KeyCode::Char(c));
        }
    }
}

pub(crate) fn popup(app: &App) -> &crate::git_popup::GitPopup {
    app.git_popup.as_ref().expect("popup open")
}

/// Focus list `staged` and move its selection to `name` with the arrow keys.
fn select(app: &mut App, staged: bool, name: &str) {
    to_list(app, staged, name);
    wait_idle(app);
    assert_eq!(popup(app).selected_entry().unwrap().path, Path::new(name));
    assert_eq!(
        popup(app).diff.as_ref().map(|d| d.path.clone()),
        Some(PathBuf::from(name))
    );
}

/// The arrow keys of the lists: to the top of the one column, then down to `name` in the
/// unstaged (`staged` false) or staged section.
fn to_list(app: &mut App, staged: bool, name: &str) {
    let pane = if staged { Pane::Staged } else { Pane::Unstaged };
    for _ in 0..20 {
        if matches!(popup(app).focus, Pane::Unstaged | Pane::Staged) {
            break;
        }
        key(app, KeyCode::Tab);
    }
    let w = usize::from(staged);
    assert!(
        popup(app).list(w).iter().any(|e| e.path == Path::new(name)),
        "{name} not listed"
    );
    for _ in 0..100 {
        key(app, KeyCode::Up);
    }
    for _ in 0..200 {
        let p = popup(app);
        if p.focus == pane
            && p.list(w)
                .get(p.selected[w])
                .is_some_and(|e| e.path == Path::new(name))
        {
            return;
        }
        key(app, KeyCode::Down);
    }
    panic!("could not reach {name}");
}

pub(crate) fn focus(app: &mut App, pane: Pane) {
    for _ in 0..20 {
        if popup(app).focus == pane {
            return;
        }
        key(app, KeyCode::Tab);
    }
    panic!("no focus {pane:?}");
}

/// Moves the diff cursor to row `row` (Home, then Down).
fn diff_to(app: &mut App, row: usize) {
    focus(app, Pane::Diff);
    key(app, KeyCode::Home);
    for _ in 0..row {
        key(app, KeyCode::Down);
    }
}

/// Row of the first diff line of `kind` whose text is `text`.
fn row_of(app: &App, kind: LineKind, text: &str) -> usize {
    let p = popup(app);
    let d = p.diff.as_ref().unwrap().diff().unwrap();
    p.diff_rows()
        .iter()
        .position(|r| match *r {
            DiffRow::Line(h, l) => {
                let line = &d.hunks[h].lines[l];
                line.kind == kind && line.text == text.as_bytes()
            }
            _ => false,
        })
        .unwrap_or_else(|| panic!("no {kind:?} line {text}"))
}

pub(crate) fn notices(app: &App) -> String {
    app.notices
        .iter()
        .map(|n| n.text.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn errors(app: &App) -> String {
    app.notices
        .iter()
        .filter(|n| n.level != NoticeLevel::Info)
        .map(|n| n.text.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn draw(app: &mut App, w: u16, h: u16) -> Terminal<TestBackend> {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| crate::ui::render(f, app)).unwrap();
    t
}

pub(crate) fn screen(t: &Terminal<TestBackend>) -> String {
    let buf = t.backend().buffer();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Twin repos made by the same command sequence (equal commit ids).
fn twins(setup: impl Fn(&Fixture)) -> (Fixture, Fixture) {
    let a = Fixture::new();
    let b = Fixture::new();
    setup(&a);
    setup(&b);
    assert_eq!(repo_state(&a, &a.dir), repo_state(&b, &b.dir));
    (a, b)
}

fn twenty() -> String {
    (1..=20).map(|i| format!("line {i}\n")).collect()
}

// ---- AC-16f: file, hunk and line actions ---------------------------------------------------

#[test]
fn file_stage_and_unstage_match_git_add_and_restore() {
    let (a, b) = twins(|f| {
        f.commit("a.txt", "one\ntwo\n", "first");
        f.commit("b.txt", "bee\n", "second");
        f.write("a.txt", "one\nTWO\n");
        f.write("dir/c.txt", "new\n");
        std::fs::remove_file(f.dir.join("b.txt")).unwrap();
    });
    let mut app = popup_app(&a);
    open(&mut app);
    for name in ["a.txt", "dir/c.txt", "b.txt"] {
        select(&mut app, false, name);
        key(&mut app, KeyCode::Char(' '));
        wait_idle(&mut app);
    }
    g(&b, &b.dir, &["add", "--all", "--", "a.txt"]);
    g(&b, &b.dir, &["add", "--all", "--", "dir/c.txt"]);
    g(&b, &b.dir, &["add", "--all", "--", "b.txt"]);
    assert_eq!(repo_state(&a, &a.dir), repo_state(&b, &b.dir));
    assert!(errors(&app).is_empty(), "{}", errors(&app));

    select(&mut app, true, "a.txt");
    key(&mut app, KeyCode::Char(' '));
    wait_idle(&mut app);
    g(&b, &b.dir, &["restore", "--staged", "--", "a.txt"]);
    assert_eq!(repo_state(&a, &a.dir), repo_state(&b, &b.dir));
    // The lists follow: a.txt is unstaged again.
    assert!(
        popup(&app)
            .list(0)
            .iter()
            .any(|e| e.path == Path::new("a.txt"))
    );
}

#[test]
fn hunk_and_line_staging_match_git_apply_cached() {
    let base = twenty();
    let edited = base
        .replace("line 2\n", "line two\n")
        .replace("line 18\n", "X\nY\n");
    let (a, b) = twins(|f| {
        f.commit("f.txt", &base, "base");
        f.write("f.txt", &edited);
    });
    let mut app = popup_app(&a);
    open(&mut app);
    select(&mut app, false, "f.txt");
    assert_eq!(
        popup(&app)
            .diff
            .as_ref()
            .unwrap()
            .diff()
            .unwrap()
            .hunks
            .len(),
        2
    );
    // Enter on the first hunk's header stages that hunk.
    diff_to(&mut app, 0);
    key(&mut app, KeyCode::Enter);
    wait_idle(&mut app);
    let first = base.replace("line 2\n", "line two\n");
    b.write("f.txt", &first);
    g(&b, &b.dir, &["add", "--", "f.txt"]);
    b.write("f.txt", &edited);
    assert_eq!(repo_state(&a, &a.dir), repo_state(&b, &b.dir));

    // Lines: the removed "line 18" and the added "X" only (not "Y").
    select(&mut app, false, "f.txt");
    let r = row_of(&app, LineKind::Removed, "line 18");
    assert_eq!(row_of(&app, LineKind::Added, "X"), r + 1);
    diff_to(&mut app, r);
    shift(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Enter);
    wait_idle(&mut app);
    b.write("f.txt", &first.replace("line 18\n", "X\n"));
    g(&b, &b.dir, &["add", "--", "f.txt"]);
    b.write("f.txt", &edited);
    assert_eq!(repo_state(&a, &a.dir), repo_state(&b, &b.dir));

    // Unstage the first hunk of the staged diff (the "line two" change).
    select(&mut app, true, "f.txt");
    diff_to(&mut app, 0);
    key(&mut app, KeyCode::Enter);
    wait_idle(&mut app);
    b.write("f.txt", &base.replace("line 18\n", "X\n"));
    g(&b, &b.dir, &["add", "--", "f.txt"]);
    b.write("f.txt", &edited);
    assert_eq!(repo_state(&a, &a.dir), repo_state(&b, &b.dir));
    assert!(errors(&app).is_empty(), "{}", errors(&app));
}

#[test]
fn a_line_range_across_two_hunks_is_refused() {
    let f = Fixture::new();
    f.commit("f.txt", &twenty(), "base");
    f.write(
        "f.txt",
        &twenty()
            .replace("line 2\n", "two\n")
            .replace("line 18\n", "eighteen\n"),
    );
    let before = g(&f, &f.dir, &["ls-files", "-s"]);
    let mut app = popup_app(&f);
    open(&mut app);
    select(&mut app, false, "f.txt");
    diff_to(&mut app, 0);
    let n = popup(&app).diff_rows().len();
    for _ in 0..n {
        shift(&mut app, KeyCode::Down);
    }
    key(&mut app, KeyCode::Enter);
    wait_idle(&mut app);
    assert!(errors(&app).contains("one hunk"), "{}", notices(&app));
    assert_eq!(g(&f, &f.dir, &["ls-files", "-s"]), before);
}

// ---- discard (SEC-11, SEC-16, D305) ------------------------------------------------------------

fn discard_dialog(app: &mut App) -> String {
    key(app, KeyCode::Char('d'));
    wait(app, "discard dialog", |a| a.dialog.is_some());
    match &app.dialog.as_ref().unwrap().widget {
        crate::ui::dialog::DialogWidget::Confirm(c) => {
            assert_eq!(c.buttons, ["Cancel", "Discard"]);
            assert_eq!(c.selected, 0, "Cancel is the default");
            c.message.clone()
        }
        _ => panic!("a confirm dialog"),
    }
}

#[test]
fn discard_asks_with_cancel_first_then_keeps_a_recovery_copy() {
    let f = Fixture::new();
    f.commit("a.txt", "one\n", "base");
    f.write("a.txt", "one\nmine\n");
    std::fs::set_permissions(f.dir.join("a.txt"), std::fs::Permissions::from_mode(0o644)).unwrap();
    let mut app = popup_app(&f);
    open(&mut app);
    select(&mut app, false, "a.txt");
    let msg = discard_dialog(&mut app);
    assert!(msg.contains("a.txt") && msg.contains("1 hunk"), "{msg}");
    // Enter on the default button: nothing happens.
    key(&mut app, KeyCode::Enter);
    wait_idle(&mut app);
    assert_eq!(
        std::fs::read_to_string(f.dir.join("a.txt")).unwrap(),
        "one\nmine\n"
    );
    // Esc too.
    discard_dialog(&mut app);
    key(&mut app, KeyCode::Esc);
    assert!(
        app.git_popup_shown(),
        "Esc closes the dialog, not the popup"
    );
    wait_idle(&mut app);
    assert_eq!(
        std::fs::read_to_string(f.dir.join("a.txt")).unwrap(),
        "one\nmine\n"
    );

    discard_dialog(&mut app);
    key(&mut app, KeyCode::Right);
    key(&mut app, KeyCode::Enter);
    wait_idle(&mut app);
    assert_eq!(
        std::fs::read_to_string(f.dir.join("a.txt")).unwrap(),
        "one\n"
    );
    let n = notices(&app);
    let copy = n
        .split_whitespace()
        .find(|w| w.contains("git-recovery"))
        .unwrap_or_else(|| panic!("notice names the copy: {n}"));
    let copy = PathBuf::from(copy);
    assert!(copy.starts_with(f.base.join("state/git-recovery")));
    assert_eq!(std::fs::read_to_string(&copy).unwrap(), "one\nmine\n");
    assert_eq!(
        std::fs::metadata(&copy).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn discard_of_a_hunk_from_the_diff_pane() {
    let f = Fixture::new();
    f.commit("f.txt", &twenty(), "base");
    let edited = twenty()
        .replace("line 2\n", "two\n")
        .replace("line 18\n", "eighteen\n");
    f.write("f.txt", &edited);
    let mut app = popup_app(&f);
    open(&mut app);
    select(&mut app, false, "f.txt");
    let r = row_of(&app, LineKind::Added, "eighteen");
    diff_to(&mut app, r);
    let msg = discard_dialog(&mut app);
    assert!(msg.contains("hunk 2"), "{msg}");
    key(&mut app, KeyCode::Right);
    key(&mut app, KeyCode::Enter);
    wait_idle(&mut app);
    assert_eq!(
        std::fs::read_to_string(f.dir.join("f.txt")).unwrap(),
        twenty().replace("line 2\n", "two\n")
    );
}

#[test]
fn discard_is_refused_while_the_file_has_unsaved_edits() {
    let f = Fixture::new();
    let p = f.commit("a.txt", "one\n", "base");
    f.write("a.txt", "one\ndisk\n");
    let mut app = popup_app(&f);
    app.open_path(&p).unwrap();
    crate::app_tests::space_i(&mut app);
    typ(&mut app, "typed ");
    assert!(app.docs[0].is_modified());
    key(&mut app, KeyCode::Esc);
    open(&mut app);
    select(&mut app, false, "a.txt");
    discard_dialog(&mut app);
    key(&mut app, KeyCode::Right);
    key(&mut app, KeyCode::Enter);
    wait_idle(&mut app);
    assert!(
        errors(&app).contains("unsaved changes"),
        "{}",
        notices(&app)
    );
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "one\ndisk\n");
}

#[test]
fn a_save_waits_while_a_discard_of_that_file_runs() {
    let f = Fixture::new();
    let p = f.commit("a.txt", "one\n", "base");
    let mut app = popup_app(&f);
    app.open_path(&p).unwrap();
    crate::app_tests::space_i(&mut app);
    typ(&mut app, "x");
    open(&mut app);
    // As while the writer thread runs the discard (D305(4)).
    app.git_popup.as_mut().unwrap().discarding.push(p.clone());
    key(&mut app, KeyCode::Esc);
    app.run_command("file.save", crate::commands::CmdArgs::None);
    assert!(
        errors(&app).contains("discard of this file is running"),
        "{}",
        notices(&app)
    );
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "one\n");
    app.git_popup.as_mut().unwrap().discarding.clear();
    app.run_command("file.save", crate::commands::CmdArgs::None);
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "xone\n");
}

// ---- commit, amend, sign off (SEC-12, AC-16f) ------------------------------------------------

#[test]
fn commit_amend_and_signoff_match_git_commit() {
    let (a, b) = twins(|f| {
        f.commit("a.txt", "one\n", "base");
        f.write("a.txt", "one\ntwo\n");
        f.git(&["add", "a.txt"]);
    });
    let mut app = popup_app(&a);
    open(&mut app);
    key(&mut app, KeyCode::Char('c'));
    assert_eq!(popup(&app).focus, Pane::Message);
    // Popup keys are text in the message box.
    typ(&mut app, "Fix the thing\n\nBody: p d r #1");
    ctrl(&mut app, 's');
    wait_idle(&mut app);
    std::fs::write(b.base.join("msg"), "Fix the thing\n\nBody: p d r #1").unwrap();
    let msg = b.base.join("msg");
    g(
        &b,
        &b.dir,
        &[
            "commit",
            "--cleanup=whitespace",
            "-F",
            msg.to_str().unwrap(),
        ],
    );
    assert_eq!(repo_state(&a, &a.dir), repo_state(&b, &b.dir));
    assert_eq!(
        popup(&app).message.text(),
        "",
        "the box empties after a commit"
    );
    assert!(notices(&app).contains("committed"), "{}", notices(&app));

    // Amend: the box takes the last message; Sign off adds the trailer.
    for f in [&a, &b] {
        f.write("a.txt", "one\ntwo\nthree\n");
        g(f, &f.dir, &["add", "a.txt"]);
    }
    wait_idle(&mut app);
    // What the popup shows is what a commit takes (SEC-18): the view catches up first.
    app.git_popup.as_mut().unwrap().rescan();
    wait_idle(&mut app);
    focus(&mut app, Pane::Amend);
    key(&mut app, KeyCode::Char(' '));
    wait(&mut app, "last message", |a| {
        popup(a).message.text().starts_with("Fix the thing")
    });
    focus(&mut app, Pane::SignOff);
    key(&mut app, KeyCode::Enter);
    assert!(popup(&app).amend && popup(&app).signoff);
    focus(&mut app, Pane::Message);
    ctrl(&mut app, 's');
    wait_idle(&mut app);
    let last = g(&b, &b.dir, &["log", "-1", "--format=%B"]);
    std::fs::write(&msg, last.trim_end()).unwrap();
    g(
        &b,
        &b.dir,
        &[
            "commit",
            "--amend",
            "--signoff",
            "--cleanup=whitespace",
            "-F",
            msg.to_str().unwrap(),
        ],
    );
    assert_eq!(repo_state(&a, &a.dir), repo_state(&b, &b.dir));
    assert!(g(&a, &a.dir, &["log", "-1", "--format=%B"]).contains("Signed-off-by: Ann Author"));
    assert!(!popup(&app).amend, "Amend is off after the commit");
}

#[test]
fn amend_off_gives_the_draft_back_and_an_empty_message_is_refused() {
    let f = Fixture::new();
    f.commit("a.txt", "one\n", "old message");
    let mut app = popup_app(&f);
    open(&mut app);
    key(&mut app, KeyCode::Char('c'));
    ctrl(&mut app, 's');
    assert!(errors(&app).contains("commit message"), "{}", notices(&app));
    typ(&mut app, "draft");
    focus(&mut app, Pane::Amend);
    key(&mut app, KeyCode::Char(' '));
    wait(&mut app, "no change", |a| {
        a.git_popup.as_ref().unwrap().message.text() == "draft"
    });
    // A draft is never replaced; with an empty box the last message comes in.
    key(&mut app, KeyCode::Char(' '));
    focus(&mut app, Pane::Message);
    for _ in 0..5 {
        key(&mut app, KeyCode::Backspace);
    }
    focus(&mut app, Pane::Amend);
    key(&mut app, KeyCode::Char(' '));
    wait(&mut app, "last message", |a| {
        a.git_popup
            .as_ref()
            .unwrap()
            .message
            .text()
            .starts_with("old message")
    });
    key(&mut app, KeyCode::Char(' '));
    assert_eq!(popup(&app).message.text(), "");
}

#[test]
fn the_write_queue_is_shown_while_a_commit_hook_runs() {
    let f = Fixture::new();
    f.commit("a.txt", "one\n", "base");
    f.write("a.txt", "one\ntwo\n");
    f.write("b.txt", "bee\n");
    f.git(&["add", "a.txt"]);
    let flag = f.base.join("go");
    let hook = f.dir.join(".git/hooks/pre-commit");
    std::fs::write(
        &hook,
        format!(
            "#!/bin/sh\nwhile [ ! -e '{}' ]; do sleep 0.05; done\n",
            flag.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut app = popup_app(&f);
    open(&mut app);
    key(&mut app, KeyCode::Char('c'));
    typ(&mut app, "msg");
    ctrl(&mut app, 's');
    select_no_wait(&mut app, "b.txt");
    key(&mut app, KeyCode::Char(' '));
    let line = popup(&app).status_line();
    assert_eq!(line, "running: commit · 1 waiting");
    let t = draw(&mut app, 120, 40);
    assert!(screen(&t).contains("running: commit · 1 waiting"));
    assert!(screen(&t).contains("[Cancel commit]"));
    std::fs::write(&flag, "").unwrap();
    wait_idle(&mut app);
    assert_eq!(g(&f, &f.dir, &["log", "-1", "--format=%s"]).trim(), "msg");
    assert!(g(&f, &f.dir, &["status", "--porcelain"]).contains("A  b.txt"));
}

/// [`select`] without waiting for the queue (a write is running).
fn select_no_wait(app: &mut App, name: &str) {
    to_list(app, false, name);
    wait(app, "diff", |a| {
        popup(a)
            .diff
            .as_ref()
            .is_some_and(|d| d.path == Path::new(name))
    });
}

// ---- push and pull (SEC-13, SEC-14, SEC-16) ---------------------------------------------------

/// `f` with a bare remote `origin` that has `main`.
fn with_remote(f: &Fixture) -> PathBuf {
    f.commit("a.txt", "one\n", "base");
    let remote = f.base.join("remote.git");
    g(
        f,
        &f.base,
        &[
            "init",
            "-q",
            "--bare",
            "-b",
            "main",
            remote.to_str().unwrap(),
        ],
    );
    g(
        f,
        &f.dir,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    g(f, &f.dir, &["push", "-q", "-u", "origin", "main"]);
    remote
}

/// A second clone that pushes a commit changing a.txt to `text`.
fn other_pushes(f: &Fixture, remote: &Path, text: &str) {
    let other = f.base.join("other");
    g(
        f,
        &f.base,
        &[
            "clone",
            "-q",
            remote.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    std::fs::write(other.join("a.txt"), text).unwrap();
    g(f, &other, &["commit", "-q", "-am", "theirs"]);
    g(f, &other, &["push", "-q", "origin", "main"]);
}

fn push_dialog(app: &mut App) -> String {
    key(app, KeyCode::Char('p'));
    wait(app, "push dialog", |a| a.dialog.is_some());
    match &app.dialog.as_ref().unwrap().widget {
        crate::ui::dialog::DialogWidget::Confirm(c) => c.message.clone(),
        _ => panic!("confirm"),
    }
}

#[test]
fn push_shows_its_target_first_then_runs_in_the_background() {
    let f = Fixture::new();
    let remote = with_remote(&f);
    f.commit("a.txt", "one\ntwo\n", "second");
    let mut app = popup_app(&f);
    open(&mut app);
    let msg = push_dialog(&mut app);
    assert!(msg.contains("main") && msg.contains("origin/main"), "{msg}");
    key(&mut app, KeyCode::Esc);
    assert!(popup(&app).job.is_none(), "nothing runs before Push");
    push_dialog(&mut app);
    key(&mut app, KeyCode::Enter);
    assert!(popup(&app).job.is_some());
    assert!(popup(&app).status_line().starts_with("push → origin/main"));
    wait_idle(&mut app);
    let head = g(&f, &f.dir, &["rev-parse", "HEAD"]);
    let theirs = g(&f, &remote, &["rev-parse", "main"]);
    assert_eq!(head, theirs);
    assert!(
        notices(&app).contains("push → origin/main done"),
        "{}",
        notices(&app)
    );
}

#[test]
fn a_rejected_push_shows_gits_words() {
    let f = Fixture::new();
    let remote = with_remote(&f);
    other_pushes(&f, &remote, "theirs\n");
    f.commit("b.txt", "mine\n", "mine");
    let mut app = popup_app(&f);
    open(&mut app);
    push_dialog(&mut app);
    key(&mut app, KeyCode::Enter);
    wait_idle(&mut app);
    let e = errors(&app);
    assert!(e.contains("git push"), "{e}");
    let out = popup(&app).output.join("\n");
    assert!(out.contains("rejected"), "{out}");
    let t = draw(&mut app, 120, 40);
    assert!(screen(&t).contains("rejected"));
    assert_ne!(
        g(&f, &f.dir, &["rev-parse", "HEAD"]),
        g(&f, &remote, &["rev-parse", "main"])
    );
}

#[test]
fn a_conflicting_pull_shows_the_banner_and_blocks_the_conflicted_file() {
    let f = Fixture::new();
    let remote = with_remote(&f);
    // Without it git 2.27+ refuses to pull divergent branches (also shown verbatim).
    f.git(&["config", "pull.rebase", "false"]);
    other_pushes(&f, &remote, "theirs\n");
    f.commit("a.txt", "mine\n", "mine");
    let mut app = popup_app(&f);
    open(&mut app);
    crate::git_popup::pull(&mut app).unwrap();
    assert!(popup(&app).status_line().starts_with("pull ← origin/main"));
    wait_idle(&mut app);
    let p = popup(&app);
    assert!(
        p.banners()
            .iter()
            .any(|(_, t)| t.contains("merge in progress")),
        "{:?}",
        p.banners()
    );
    assert!(errors(&app).contains("pull stopped"), "{}", notices(&app));
    let index = g(&f, &f.dir, &["ls-files", "-s"]);
    select(&mut app, false, "a.txt");
    key(&mut app, KeyCode::Char(' '));
    wait_idle(&mut app);
    assert!(errors(&app).contains("conflicted"), "{}", notices(&app));
    key(&mut app, KeyCode::Char('c'));
    typ(&mut app, "merge");
    ctrl(&mut app, 's');
    wait_idle(&mut app);
    assert!(errors(&app).contains("conflict"), "{}", notices(&app));
    assert_eq!(g(&f, &f.dir, &["ls-files", "-s"]), index);
    let t = draw(&mut app, 80, 24);
    assert!(screen(&t).contains("merge in progress"), "{}", screen(&t));
}

#[test]
fn slash_git_push_and_pull_run_through_the_popup() {
    let f = Fixture::new();
    let remote = with_remote(&f);
    f.commit("a.txt", "one\ntwo\n", "second");
    let mut app = popup_app(&f);
    crate::slash_tests::slash(&mut app, "/git push");
    crate::slash_tests::key(&mut app, KeyCode::Enter);
    wait(&mut app, "push dialog", |a| a.dialog.is_some());
    key(&mut app, KeyCode::Enter);
    wait_idle(&mut app);
    assert_eq!(
        g(&f, &f.dir, &["rev-parse", "HEAD"]),
        g(&f, &remote, &["rev-parse", "main"])
    );
    key(&mut app, KeyCode::Esc);
    other_pushes(&f, &remote, "theirs\n");
    crate::slash_tests::slash(&mut app, "/git pull");
    crate::slash_tests::key(&mut app, KeyCode::Enter);
    wait_idle(&mut app);
    assert_eq!(
        std::fs::read_to_string(f.dir.join("a.txt")).unwrap(),
        "theirs\n"
    );
}

// ---- SEC-15: hostile names and messages -------------------------------------------------------

pub(crate) const HOSTILE: &str = "\x1b[2J\x1b]52;c;ZXZpbA==\x07\u{9b}31m\r\u{202e}";

pub(crate) fn assert_safe(t: &Terminal<TestBackend>, what: &str) {
    let buf = t.backend().buffer();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            let s = buf[(x, y)].symbol();
            assert!(
                !s.chars().any(is_unsafe_char),
                "{what}: unsafe cell {s:?} at {x},{y}\n{}",
                screen(t)
            );
        }
    }
}

#[test]
fn hostile_names_messages_and_git_output_are_drawn_safely() {
    let f = Fixture::new();
    let name = format!("evil{HOSTILE}.txt");
    f.commit(
        &name,
        &format!("a{HOSTILE}\n"),
        &format!("subject{HOSTILE}"),
    );
    f.write(&name, &format!("b{HOSTILE}\n"));
    f.write(&format!("new{HOSTILE}"), "x\n");
    // A hook that fails with an OSC 52 clipboard write and a BIDI override.
    let hook = f.dir.join(".git/hooks/pre-commit");
    std::fs::write(
        &hook,
        format!("#!/bin/sh\nprintf 'no{HOSTILE}\\n' >&2\nexit 1\n"),
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut app = popup_app(&f);
    open(&mut app);
    select(&mut app, false, &name);
    // The last message (hostile) loaded by Amend.
    focus(&mut app, Pane::Amend);
    key(&mut app, KeyCode::Char(' '));
    wait(&mut app, "last message", |a| {
        popup(a).message.text().starts_with("subject")
    });
    for (w, h) in [(120, 40), (80, 24), (40, 12)] {
        let t = draw(&mut app, w, h);
        assert_safe(&t, "lists, diff, message");
    }
    let t = draw(&mut app, 120, 40);
    assert!(screen(&t).contains("evil^[[2J^[]52;c;ZXZpbA==^G<U+009B>31m^M<U+202E>.txt"));
    // ratatui drops control characters by itself; SEC-15 wants them visible: the list row
    // (only the untracked file is nowhere else) and the message box show placeholders.
    let s = screen(&t);
    assert!(s.contains("^G<U+009B>31m^M<U+202E>\u{2502}"), "{s}");
    assert!(s.contains("subject^[[2J^[]52;c;ZXZpbA==^G"), "{s}");
    // A failing commit: git's (hook's) words in the notice and the output view.
    focus(&mut app, Pane::Message);
    ctrl(&mut app, 's');
    wait_idle(&mut app);
    assert!(errors(&app).contains("git commit"), "{}", notices(&app));
    for (w, h) in [(120, 40), (80, 24), (40, 12)] {
        let t = draw(&mut app, w, h);
        assert_safe(&t, "error output");
    }
    for n in &app.notices {
        assert!(!n.text.chars().any(is_unsafe_char), "{:?}", n.text);
    }
}

// ---- SEC-17: one repo per popup --------------------------------------------------------------

#[test]
fn the_popup_stays_on_its_repo_and_slash_git_takes_root_names_only() {
    let fa = Fixture::new();
    fa.commit("a.txt", "one\n", "a");
    fa.write("a.txt", "one\ntwo\n");
    let fb = Fixture::bare_dir("other");
    fb.git(&["init", "-q", "-b", "main"]);
    fb.commit("b.txt", "bee\n", "b");
    fb.write("b.txt", "bee\nmore\n");
    let ws = Workspace::from_paths(&[fa.dir.clone(), fb.dir.clone()])
        .unwrap()
        .0;
    let mut app = app_on(&fa, ws);
    open(&mut app);
    assert_eq!(popup(&app).workdir, fa.dir);
    // Focus moves to a file of root B (e.g. opened by another route) while the popup shows.
    app.open_path(&fb.dir.join("b.txt")).unwrap();
    select(&mut app, false, "a.txt");
    key(&mut app, KeyCode::Char(' '));
    wait_idle(&mut app);
    assert!(g(&fa, &fa.dir, &["status", "--porcelain"]).contains("M  a.txt"));
    assert!(g(&fb, &fb.dir, &["status", "--porcelain"]).contains(" M b.txt"));

    key(&mut app, KeyCode::Esc);
    crate::slash_tests::slash(&mut app, "/git ../x");
    crate::slash_tests::key(&mut app, KeyCode::Enter);
    assert!(
        crate::slash_tests::error(&app).is_some_and(|e| e.contains("no such root")),
        "{:?}",
        crate::slash_tests::error(&app)
    );
    key(&mut app, KeyCode::Esc);
    let name_b = app.workspace.roots()[1].name.clone();
    crate::slash_tests::slash(&mut app, &format!("/git {name_b}"));
    crate::slash_tests::key(&mut app, KeyCode::Enter);
    wait_idle(&mut app);
    assert_eq!(popup(&app).workdir, fb.dir);
    let t = draw(&mut app, 120, 40);
    assert!(screen(&t).contains("other"), "{}", screen(&t));
}

#[test]
fn a_parent_repo_is_named_in_the_header() {
    let f = Fixture::new();
    f.commit("sub/a.txt", "one\n", "a");
    f.write("sub/a.txt", "two\n");
    let mut app = app_on(&f, Workspace::single(f.dir.join("sub")));
    open(&mut app);
    let t = draw(&mut app, 120, 40);
    assert!(screen(&t).contains("parent repo of sub"), "{}", screen(&t));
}

// ---- SEC-18: stale view ------------------------------------------------------------------------

#[test]
fn an_action_on_a_changed_file_is_refused_and_the_view_refreshes() {
    let f = Fixture::new();
    f.commit("f.txt", &twenty(), "base");
    f.write("f.txt", &twenty().replace("line 2\n", "two\n"));
    let index = g(&f, &f.dir, &["ls-files", "-s"]);
    let mut app = popup_app(&f);
    open(&mut app);
    select(&mut app, false, "f.txt");
    diff_to(&mut app, 0);
    // Changed from outside after it was shown; no rescan yet.
    f.write("f.txt", &twenty().replace("line 2\n", "TWO\n"));
    key(&mut app, KeyCode::Enter);
    wait_idle(&mut app);
    assert!(
        errors(&app).contains("changed, rescan"),
        "{}",
        notices(&app)
    );
    assert_eq!(g(&f, &f.dir, &["ls-files", "-s"]), index);
    // The view shows the new content now.
    let d = popup(&app).diff.as_ref().unwrap().diff().unwrap().clone();
    assert!(d.hunks[0].lines.iter().any(|l| l.text == b"TWO"));

    // The same for a whole-file stage.
    app.notices.clear();
    f.write("f.txt", &twenty().replace("line 2\n", "2\n"));
    focus(&mut app, Pane::Unstaged);
    key(&mut app, KeyCode::Char(' '));
    wait_idle(&mut app);
    assert!(
        errors(&app).contains("changed, rescan"),
        "{}",
        notices(&app)
    );
    assert_eq!(g(&f, &f.dir, &["ls-files", "-s"]), index);
}

// ---- SEC-19: program-running config -------------------------------------------------------

#[test]
fn program_running_config_is_named_in_a_banner() {
    let f = Fixture::new();
    f.commit("a.txt", "one\n", "a");
    let mut app = popup_app(&f);
    open(&mut app);
    assert!(
        popup(&app).banners().is_empty(),
        "{:?}",
        popup(&app).banners()
    );
    key(&mut app, KeyCode::Esc);

    f.git(&["config", "filter.x.clean", "cat"]);
    let hook = f.dir.join(".git/hooks/pre-push");
    std::fs::write(&hook, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    open(&mut app);
    let banners = popup(&app).banners();
    assert_eq!(banners.len(), 1, "{banners:?}");
    assert!(banners[0].1.contains("filter.x.clean"), "{banners:?}");
    assert!(banners[0].1.contains("hook pre-push"), "{banners:?}");
    let t = draw(&mut app, 80, 24);
    assert!(screen(&t).contains("runs programs"), "{}", screen(&t));
}

// ---- SEC-20 ------------------------------------------------------------------------------------

#[test]
fn the_popup_logs_nothing() {
    // vr-ide installs no log subscriber; the popup code does not even emit events, so no
    // message, diff or URL can reach a log (SEC-20).
    let src = include_str!("git_popup.rs");
    assert!(
        !src.contains("tracing::"),
        "git_popup.rs emits tracing events"
    );
    assert!(!include_str!("main.rs").contains("tracing_subscriber"));
}

// ---- leftovers ----------------------------------------------------------------------------

#[test]
fn leftovers_of_an_interrupted_discard_are_offered_on_open() {
    let f = Fixture::new();
    f.commit("a.txt", "one\n", "a");
    f.write(".vr-ide-discard-1-2-3.new", "unused\n");
    f.write("d/.vr-ide-discard-4-5-6.old", "earlier\n");
    f.write("d/taken.txt", "taken\n");
    let mut app = popup_app(&f);
    open(&mut app);
    wait(&mut app, "leftover dialog", |a| a.dialog.is_some());
    let text = |a: &App| match &a.dialog.as_ref().unwrap().widget {
        crate::ui::dialog::DialogWidget::Confirm(c) => (c.message.clone(), c.buttons.clone()),
        crate::ui::dialog::DialogWidget::Prompt(p) => (p.label.clone(), Vec::new()),
    };
    let (m, b) = text(&app);
    assert!(m.contains(".vr-ide-discard-1-2-3.new"), "{m}");
    assert_eq!(b, ["Keep", "Delete"]);
    key(&mut app, KeyCode::Right);
    key(&mut app, KeyCode::Enter);
    wait(&mut app, "next leftover", |a| a.dialog.is_some());
    wait_idle(&mut app);
    assert!(!f.dir.join(".vr-ide-discard-1-2-3.new").exists());
    let (m, b) = text(&app);
    assert!(m.contains("d/.vr-ide-discard-4-5-6.old"), "{m}");
    assert_eq!(b, ["Keep", "Restore as…"]);
    key(&mut app, KeyCode::Right);
    key(&mut app, KeyCode::Enter);
    // The prompt starts with the leftover's folder; an existing name is never replaced.
    typ(&mut app, "taken.txt");
    key(&mut app, KeyCode::Enter);
    wait_idle(&mut app);
    assert!(errors(&app).contains("already exists"), "{}", notices(&app));
    assert_eq!(
        std::fs::read_to_string(f.dir.join("d/taken.txt")).unwrap(),
        "taken\n"
    );
    assert!(f.dir.join("d/.vr-ide-discard-4-5-6.old").exists());
    // Offered again on the next open; restored under a free name.
    key(&mut app, KeyCode::Esc);
    open(&mut app);
    wait(&mut app, "leftover dialog", |a| a.dialog.is_some());
    key(&mut app, KeyCode::Right);
    key(&mut app, KeyCode::Enter);
    typ(&mut app, "back.txt");
    key(&mut app, KeyCode::Enter);
    wait_idle(&mut app);
    assert_eq!(
        std::fs::read_to_string(f.dir.join("d/back.txt")).unwrap(),
        "earlier\n"
    );
    assert!(!f.dir.join("d/.vr-ide-discard-4-5-6.old").exists());
}

// ---- keys, mouse, editor refresh ----------------------------------------------------------

#[test]
fn keys_stay_in_the_popup_and_esc_keeps_the_draft() {
    let f = Fixture::new();
    f.commit("a.txt", "one\n", "a");
    f.write("a.txt", "two\n");
    let mut app = popup_app(&f);
    open(&mut app);
    let ring: Vec<Pane> = (0..crate::git_popup::RING.len())
        .map(|_| {
            let p = popup(&app).focus;
            key(&mut app, KeyCode::Tab);
            p
        })
        .collect();
    assert_eq!(ring, crate::git_popup::RING);
    assert_eq!(popup(&app).focus, Pane::Unstaged);
    key(&mut app, KeyCode::BackTab);
    assert_eq!(popup(&app).focus, Pane::Rescan);
    // A global key (ctrl+q quits elsewhere) does nothing here.
    ctrl(&mut app, 'q');
    assert!(!app.quit);
    key(&mut app, KeyCode::Char('c'));
    typ(&mut app, "draft");
    key(&mut app, KeyCode::Esc);
    assert!(!app.git_popup_shown());
    open(&mut app);
    assert_eq!(popup(&app).message.text(), "draft");
    // `2` and `3` switch to History and Blame (M7-4b), `1` comes back to the same pane.
    focus(&mut app, Pane::Unstaged);
    key(&mut app, KeyCode::Char('2'));
    assert_eq!(popup(&app).tab, crate::git_popup::Tab::History);
    key(&mut app, KeyCode::Char('3'));
    assert_eq!(popup(&app).tab, crate::git_popup::Tab::Blame);
    key(&mut app, KeyCode::Char('1'));
    assert!(app.git_popup_shown());
    assert_eq!(popup(&app).tab, crate::git_popup::Tab::Changes);
    assert_eq!(popup(&app).focus, Pane::Unstaged);
}

pub(crate) fn click(app: &mut App, x: u16, y: u16) {
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: KeyModifiers::NONE,
    });
}

#[test]
fn the_mouse_selects_files_and_presses_buttons() {
    let f = Fixture::new();
    f.commit("a.txt", "one\n", "a");
    f.write("a.txt", "two\n");
    f.write("b.txt", "bee\n");
    let mut app = popup_app(&f);
    open(&mut app);
    draw(&mut app, 120, 40);
    let a = popup(&app).areas.clone();
    // Second row of the unstaged list (row 0 is its header).
    click(&mut app, a.unstaged.x + 2, a.unstaged.y + 2);
    wait_idle(&mut app);
    assert_eq!(
        popup(&app).selected_entry().unwrap().path,
        Path::new("b.txt")
    );
    key(&mut app, KeyCode::Char(' '));
    wait_idle(&mut app);
    draw(&mut app, 120, 40);
    let a = popup(&app).areas.clone();
    click(&mut app, a.message.x + 1, a.message.y);
    assert_eq!(popup(&app).focus, Pane::Message);
    typ(&mut app, "by mouse");
    let commit = a
        .buttons
        .iter()
        .find(|(_, p)| *p == Pane::Commit)
        .unwrap()
        .0;
    let amend = a
        .buttons
        .iter()
        .find(|(_, p)| *p == Pane::SignOff)
        .unwrap()
        .0;
    click(&mut app, amend.x, amend.y);
    assert!(popup(&app).signoff);
    click(&mut app, commit.x + 1, commit.y);
    wait_idle(&mut app);
    let log = g(&f, &f.dir, &["log", "-1", "--format=%B"]);
    assert!(log.starts_with("by mouse"), "{log}");
    assert!(log.contains("Signed-off-by"), "{log}");
    // A click on the diff pane focuses it and moves its cursor.
    select(&mut app, false, "a.txt");
    draw(&mut app, 120, 40);
    let a = popup(&app).areas.clone();
    click(&mut app, a.diff.x + 1, a.diff.y + 2);
    assert_eq!(popup(&app).focus, Pane::Diff);
    assert_eq!(popup(&app).diff_cursor, 2);
}

#[test]
fn editor_marks_follow_a_write_of_the_popup() {
    let f = Fixture::new();
    let p = f.commit("a.txt", "one\n", "a");
    f.write("a.txt", "one\ntwo\n");
    let mut app = popup_app(&f);
    app.open_path(&p).unwrap();
    wait(&mut app, "marks", |a| {
        a.git.marks_for(&p).is_some_and(|(_, m)| !m.is_empty())
    });
    open(&mut app);
    select(&mut app, false, "a.txt");
    key(&mut app, KeyCode::Char(' '));
    wait_idle(&mut app);
    wait(&mut app, "marks cleared", |a| {
        a.git.marks_for(&p).is_some_and(|(_, m)| m.is_empty())
    });
}

// ---- snapshots at the three sizes -------------------------------------------------------

#[test]
fn render_git_popup_changes() {
    let f = Fixture::new();
    f.commit("README.md", "# readme\n", "readme");
    let p = f.commit("src/app.rs", "fn main() {\n    old();\n}\n", "app");
    f.write("README.md", "# readme\n\nmore\n");
    f.git(&["add", "README.md"]);
    f.write("src/app.rs", "fn main() {\n    new();\n    more();\n}\n");
    f.write("notes.md", "notes\n");
    let mut app = popup_app(&f);
    app.open_path(&p).unwrap();
    open(&mut app);
    app.git_popup.as_mut().unwrap().repo_label = "~/repo".into();
    select(&mut app, false, "src/app.rs");
    key(&mut app, KeyCode::Char('c'));
    typ(&mut app, "Fix the find box");
    for (w, h) in [(120, 40), (80, 24), (40, 12)] {
        let t = draw(&mut app, w, h);
        assert_safe(&t, "snapshot");
        insta::assert_snapshot!(format!("render_git_popup_changes_{w}x{h}"), t.backend());
    }
    // The diff pane with the keys, at the smallest size (stacked).
    focus(&mut app, Pane::Diff);
    key(&mut app, KeyCode::Down);
    shift(&mut app, KeyCode::Down);
    let t = draw(&mut app, 40, 12);
    insta::assert_snapshot!("render_git_popup_diff_40x12", t.backend());
}

// ---- review round 1 (D307) --------------------------------------------------------------

/// Text of the open confirm dialog and its selected button.
fn confirm(app: &App) -> (String, Vec<String>, usize) {
    match &app.dialog.as_ref().expect("dialog").widget {
        crate::ui::dialog::DialogWidget::Confirm(c) => {
            (c.message.clone(), c.buttons.clone(), c.selected)
        }
        _ => panic!("confirm"),
    }
}

/// Two roots, alpha (with a remote, one commit ahead) and beta.
fn alpha_beta() -> (Fixture, PathBuf, Fixture, App) {
    let fa = Fixture::new();
    let remote = with_remote(&fa);
    fa.commit("a.txt", "one\ntwo\n", "second");
    let fb = Fixture::bare_dir("other");
    fb.git(&["init", "-q", "-b", "main"]);
    fb.commit("b.txt", "bee\n", "b");
    let ws = Workspace::from_paths(&[fa.dir.clone(), fb.dir.clone()])
        .unwrap()
        .0;
    let app = app_on(&fa, ws);
    (fa, remote, fb, app)
}

#[test]
fn push_inside_the_popup_uses_its_own_repo_and_keeps_the_draft() {
    let (fa, remote, fb, mut app) = alpha_beta();
    open(&mut app);
    let alpha = popup(&app).repo_label.clone();
    let beta = crate::git::home_relative(&fb.dir);
    key(&mut app, KeyCode::Char('c'));
    typ(&mut app, "draft");
    // The active file is in beta now; the popup stays on alpha (SEC-17).
    app.open_path(&fb.dir.join("b.txt")).unwrap();
    focus(&mut app, Pane::Unstaged);
    let msg = push_dialog(&mut app);
    assert!(msg.contains(&alpha) && !msg.contains(&beta), "{msg}");
    assert_eq!(popup(&app).workdir, fa.dir);
    assert_eq!(popup(&app).message.text(), "draft");
    key(&mut app, KeyCode::Esc);
    // The Push button: the same repo; Push runs it.
    focus(&mut app, Pane::Push);
    key(&mut app, KeyCode::Enter);
    wait(&mut app, "push dialog", |a| a.dialog.is_some());
    let (msg, ..) = confirm(&app);
    assert!(msg.contains(&alpha) && !msg.contains(&beta), "{msg}");
    key(&mut app, KeyCode::Enter);
    wait_idle(&mut app);
    assert_eq!(
        g(&fa, &fa.dir, &["rev-parse", "HEAD"]),
        g(&fa, &remote, &["rev-parse", "main"])
    );
    assert_eq!(popup(&app).message.text(), "draft");
    assert_eq!(popup(&app).workdir, fa.dir);
    // `/git push` binds to the active file's repo (beta); alpha's draft waits for alpha.
    key(&mut app, KeyCode::Esc);
    crate::slash_tests::slash(&mut app, "/git push");
    crate::slash_tests::key(&mut app, KeyCode::Enter);
    wait(&mut app, "beta session", |a| {
        a.git_popup.as_ref().is_some_and(|p| p.workdir == fb.dir)
    });
    wait_idle(&mut app);
    assert_eq!(popup(&app).message.text(), "");
    app.dialog = None;
    key(&mut app, KeyCode::Esc);
    let name_a = app.workspace.roots()[0].name.clone();
    crate::slash_tests::slash(&mut app, &format!("/git {name_a}"));
    crate::slash_tests::key(&mut app, KeyCode::Enter);
    wait_idle(&mut app);
    assert_eq!(popup(&app).workdir, fa.dir);
    assert_eq!(popup(&app).message.text(), "draft");
}

#[test]
fn a_commit_is_refused_when_head_or_the_staged_set_moved_since_it_was_shown() {
    let f = Fixture::new();
    f.commit("a.txt", "one\n", "base");
    f.write("a.txt", "one\ntwo\n");
    f.git(&["add", "a.txt"]);
    f.write("b.txt", "bee\n");
    let mut app = popup_app(&f);
    open(&mut app);
    key(&mut app, KeyCode::Char('c'));
    typ(&mut app, "mine");
    let subject = |f: &Fixture| {
        g(f, &f.dir, &["log", "-1", "--format=%s"])
            .trim()
            .to_string()
    };
    // Staged from outside after the view was drawn, before ctrl+s (no rescan in between).
    g(&f, &f.dir, &["add", "b.txt"]);
    ctrl(&mut app, 's');
    wait_idle(&mut app);
    assert!(
        errors(&app).contains("changed, rescan"),
        "{}",
        notices(&app)
    );
    assert_eq!(subject(&f), "base");
    assert_eq!(popup(&app).message.text(), "mine", "the draft stays");
    // HEAD moved from outside.
    app.notices.clear();
    g(&f, &f.dir, &["commit", "-q", "-m", "terminal"]);
    ctrl(&mut app, 's');
    wait_idle(&mut app);
    assert!(
        errors(&app).contains("changed, rescan"),
        "{}",
        notices(&app)
    );
    assert_eq!(subject(&f), "terminal");
    // Amend: HEAD's message loaded, then a commit from outside before ctrl+s.
    app.notices.clear();
    for _ in 0..4 {
        key(&mut app, KeyCode::Backspace);
    }
    focus(&mut app, Pane::Amend);
    key(&mut app, KeyCode::Char(' '));
    wait(&mut app, "last message", |a| {
        popup(a).message.text().starts_with("terminal")
    });
    f.write("c.txt", "c\n");
    g(&f, &f.dir, &["add", "c.txt"]);
    g(&f, &f.dir, &["commit", "-q", "-m", "terminal commit"]);
    focus(&mut app, Pane::Message);
    ctrl(&mut app, 's');
    wait_idle(&mut app);
    assert!(
        errors(&app).contains("changed, rescan"),
        "{}",
        notices(&app)
    );
    assert_eq!(subject(&f), "terminal commit");
    assert!(
        !g(&f, &f.dir, &["reflog", "-1"]).contains("amend"),
        "the outside commit is not amended"
    );
    // Amend off and on again loads the new HEAD; then it commits.
    app.notices.clear();
    focus(&mut app, Pane::Amend);
    key(&mut app, KeyCode::Char(' '));
    key(&mut app, KeyCode::Char(' '));
    wait(&mut app, "new last message", |a| {
        popup(a).message.text().starts_with("terminal commit")
    });
    focus(&mut app, Pane::Message);
    typ(&mut app, " v2");
    ctrl(&mut app, 's');
    wait_idle(&mut app);
    assert_eq!(subject(&f), "terminal commit v2", "{}", notices(&app));
    assert_eq!(g(&f, &f.dir, &["rev-list", "--count", "HEAD"]).trim(), "3");
}

#[test]
fn enter_twice_quickly_in_the_message_box_keeps_the_blank_line() {
    let f = Fixture::new();
    f.commit("a.txt", "one\n", "base");
    let mut app = popup_app(&f);
    app.enter_settle = crate::app::ENTER_SETTLE;
    app.enter_repeat_gap = crate::app::ENTER_REPEAT_GAP;
    open(&mut app);
    let t0 = Instant::now() + Duration::from_secs(2);
    let mut ms = 0;
    let mut at = |app: &mut App, k: KeyEvent| {
        ms += 40;
        app.handle_key_at(k, t0 + Duration::from_millis(ms));
    };
    let plain = |c: KeyCode| KeyEvent::new(c, KeyModifiers::NONE);
    at(&mut app, plain(KeyCode::Char('c')));
    for k in [
        KeyCode::Char('S'),
        KeyCode::Enter,
        KeyCode::Enter,
        KeyCode::Char('b'),
    ] {
        at(&mut app, plain(k));
    }
    assert_eq!(popup(&app).message.text(), "S\n\nb");
    // A labelled key repeat still types nothing.
    let mut held = plain(KeyCode::Enter);
    held.kind = crossterm::event::KeyEventKind::Repeat;
    at(&mut app, held);
    assert_eq!(popup(&app).message.text(), "S\n\nb");
}

#[test]
fn writes_waiting_behind_a_push_are_shown() {
    let f = Fixture::new();
    with_remote(&f);
    f.commit("a.txt", "one\ntwo\n", "second");
    f.write("b.txt", "bee\n");
    let flag = f.base.join("go");
    let hook = f.dir.join(".git/hooks/pre-push");
    std::fs::write(
        &hook,
        format!(
            "#!/bin/sh\nwhile [ ! -e '{}' ]; do sleep 0.05; done\n",
            flag.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut app = popup_app(&f);
    open(&mut app);
    push_dialog(&mut app);
    key(&mut app, KeyCode::Enter);
    assert!(popup(&app).job.is_some());
    select_no_wait(&mut app, "b.txt");
    key(&mut app, KeyCode::Char(' '));
    let line = popup(&app).status_line();
    assert!(line.starts_with("push → origin/main"), "{line}");
    assert!(line.ends_with("· 1 waiting: stage b.txt"), "{line}");
    let t = draw(&mut app, 80, 24);
    assert!(
        screen(&t).contains("1 waiting: stage b.txt"),
        "{}",
        screen(&t)
    );
    std::fs::write(&flag, "").unwrap();
    wait_idle(&mut app);
    assert!(g(&f, &f.dir, &["status", "--porcelain"]).contains("A  b.txt"));
}

#[test]
fn a_second_ctrl_s_during_a_commit_asks_before_cancelling() {
    let f = Fixture::new();
    f.commit("a.txt", "one\n", "base");
    f.write("a.txt", "one\ntwo\n");
    f.git(&["add", "a.txt"]);
    let flag = f.base.join("go");
    let hook = f.dir.join(".git/hooks/pre-commit");
    std::fs::write(
        &hook,
        format!(
            "#!/bin/sh\nwhile [ ! -e '{}' ]; do sleep 0.05; done\n",
            flag.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut app = popup_app(&f);
    open(&mut app);
    key(&mut app, KeyCode::Char('c'));
    typ(&mut app, "msg");
    ctrl(&mut app, 's');
    ctrl(&mut app, 's');
    let (msg, buttons, sel) = confirm(&app);
    assert!(msg.contains("commit"), "{msg}");
    assert_eq!(buttons, ["Keep running", "Cancel it"]);
    assert_eq!(sel, 0);
    key(&mut app, KeyCode::Enter);
    assert!(app.dialog.is_none());
    assert_eq!(popup(&app).button_label(Pane::Commit), "Cancel commit");
    ctrl(&mut app, 's');
    key(&mut app, KeyCode::Right);
    key(&mut app, KeyCode::Enter);
    wait_idle(&mut app);
    assert!(errors(&app).contains("cancelled"), "{}", notices(&app));
    assert_eq!(g(&f, &f.dir, &["log", "-1", "--format=%s"]).trim(), "base");
}

#[test]
fn amending_a_commit_already_on_the_upstream_asks_first() {
    let f = Fixture::new();
    let remote = with_remote(&f);
    let mut app = popup_app(&f);
    open(&mut app);
    focus(&mut app, Pane::Amend);
    key(&mut app, KeyCode::Char(' '));
    wait(&mut app, "last message", |a| {
        popup(a).message.text().starts_with("base")
    });
    focus(&mut app, Pane::Message);
    key(&mut app, KeyCode::End);
    typ(&mut app, " v2");
    ctrl(&mut app, 's');
    let (msg, buttons, sel) = confirm(&app);
    assert!(msg.contains("already on origin/main"), "{msg}");
    assert_eq!(buttons[sel], "Cancel");
    key(&mut app, KeyCode::Enter);
    wait_idle(&mut app);
    let head = || g(&f, &f.dir, &["rev-parse", "HEAD"]);
    assert_eq!(head(), g(&f, &remote, &["rev-parse", "main"]));
    assert!(popup(&app).amend, "Cancel keeps the box as it was");
    ctrl(&mut app, 's');
    let i = confirm(&app).1.iter().position(|b| b != "Cancel").unwrap();
    for _ in 0..i {
        key(&mut app, KeyCode::Right);
    }
    key(&mut app, KeyCode::Enter);
    wait_idle(&mut app);
    assert_eq!(
        g(&f, &f.dir, &["log", "-1", "--format=%s"]).trim(),
        "base v2"
    );
    assert_ne!(head(), g(&f, &remote, &["rev-parse", "main"]));
}

#[test]
fn restore_as_stays_inside_the_work_tree() {
    let f = Fixture::new();
    f.commit("a.txt", "one\n", "a");
    f.write("d/.vr-ide-discard-4-5-6.old", "earlier\n");
    let outside = f.base.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, f.dir.join("link")).unwrap();
    std::os::unix::fs::symlink(&outside, f.dir.join("d/inner")).unwrap();
    let left = Path::new("d/.vr-ide-discard-4-5-6.old");
    for to in [
        ".git/hooks/post-commit",
        "d/.git/x",
        "d/.GIT/x",
        "link/x.txt",
        "d/inner/x.txt",
        "../x.txt",
        "/tmp/x.txt",
        "nodir/x.txt",
    ] {
        let r = crate::git_popup::leftover(&f.dir, left, Some(Path::new(to)));
        assert!(r.is_err(), "{to} accepted");
    }
    assert!(!f.dir.join(".git/hooks/post-commit").exists());
    assert!(std::fs::read_dir(&outside).unwrap().next().is_none());
    assert!(f.dir.join(left).exists());
    std::fs::write(f.dir.join("d/taken.txt"), "taken\n").unwrap();
    let r = crate::git_popup::leftover(&f.dir, left, Some(Path::new("d/taken.txt")));
    assert!(r.unwrap_err().to_string().contains("already exists"));
    crate::git_popup::leftover(&f.dir, left, Some(Path::new("d/back.txt"))).unwrap();
    assert_eq!(
        std::fs::read_to_string(f.dir.join("d/back.txt")).unwrap(),
        "earlier\n"
    );
}

#[test]
fn render_git_popup_wide_diff() {
    let f = Fixture::new();
    f.commit("w.rs", "short\n", "w");
    f.write(
        "w.rs",
        &format!("short\nlet wide = \"{}\"; // END\n", "x".repeat(80)),
    );
    let mut app = popup_app(&f);
    app.open_path(&f.dir.join("w.rs")).unwrap();
    open(&mut app);
    app.git_popup.as_mut().unwrap().repo_label = "~/repo".into();
    select(&mut app, false, "w.rs");
    let row = row_of(
        &app,
        LineKind::Added,
        &format!("let wide = \"{}\"; // END", "x".repeat(80)),
    );
    diff_to(&mut app, row);
    for (w, h) in [(80, 24), (40, 12)] {
        let t = draw(&mut app, w, h);
        let s = screen(&t);
        assert!(s.contains("+let wide = \"xxx"), "{s}");
        assert!(s.contains("x…"), "a cut line ends with …\n{s}");
        assert!(!s.contains("END"), "{s}");
        insta::assert_snapshot!(format!("render_git_popup_wide_diff_{w}x{h}"), t.backend());
    }
    // right / left scroll the focused diff pane sideways.
    let mut seen_end = false;
    for _ in 0..40 {
        key(&mut app, KeyCode::Right);
        if screen(&draw(&mut app, 80, 24)).contains("// END") {
            seen_end = true;
            break;
        }
    }
    assert!(seen_end, "right reaches the end of the line");
    assert!(popup(&app).diff_left > 0);
    assert_eq!(popup(&app).focus, Pane::Diff);
    let t = draw(&mut app, 80, 24);
    let s = screen(&t);
    assert!(s.contains("col ") && s.contains("+"), "{s}");
    assert!(s.contains("+…"), "hidden text on the left is marked\n{s}");
    insta::assert_snapshot!("render_git_popup_wide_diff_scrolled_80x24", t.backend());
    for _ in 0..40 {
        key(&mut app, KeyCode::Left);
    }
    assert_eq!(popup(&app).diff_left, 0);
}

// ---- a dead writer thread, an orphaned session's draft --------------------------------------

#[test]
fn a_dead_writer_thread_ends_the_session_instead_of_hanging_it() {
    let f = Fixture::new();
    f.commit("a.txt", "one\n", "base");
    f.write("a.txt", "two\n");
    let mut app = popup_app(&f);
    open(&mut app);
    select(&mut app, false, "a.txt");
    app.git_popup.as_mut().unwrap().kill_writer();
    key(&mut app, KeyCode::Char(' '));
    {
        let p = popup(&app);
        assert!(p.queue.is_empty(), "the queue entry is removed");
        assert!(p.orphan && !p.busy());
    }
    app.tick(Instant::now());
    assert!(app.git_popup.is_none(), "the dead session is closed");
    assert!(errors(&app).contains("writer stopped"), "{}", notices(&app));
    assert!(g(&f, &f.dir, &["status", "--porcelain"]).contains(" M a.txt"));
    // /git works again, and a save of the file is not blocked.
    open(&mut app);
    assert!(app.git_popup.is_some());
}

#[test]
fn a_session_closed_on_root_removal_keeps_its_message_draft() {
    let fa = Fixture::new();
    fa.commit("a.txt", "one\n", "a");
    fa.write("a.txt", "one\ntwo\n");
    let fb = Fixture::bare_dir("other");
    fb.git(&["init", "-q", "-b", "main"]);
    fb.commit("b.txt", "bee\n", "b");
    let ws = Workspace::from_paths(&[fa.dir.clone(), fb.dir.clone()])
        .unwrap()
        .0;
    let mut app = app_on(&fa, ws);
    open(&mut app);
    key(&mut app, KeyCode::Char('c'));
    typ(&mut app, "half-written");
    let id = app.workspace.roots()[0].id;
    app.remove_root(id).unwrap();
    app.tick(Instant::now());
    assert!(app.git_popup.is_none());
    assert!(
        app.git_drafts
            .iter()
            .any(|(w, d)| *w == fa.dir && d == "half-written"),
        "{:?}",
        app.git_drafts
    );
}

// ---- final review 2 (D309): quit while a git write runs ----------------------------------

/// A `hook` in `f` that waits until the returned flag file exists.
fn blocking_hook(f: &Fixture, hook: &str) -> PathBuf {
    let flag = f.base.join("go");
    let path = f.dir.join(".git/hooks").join(hook);
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\nwhile [ ! -e '{}' ]; do sleep 0.05; done\n",
            flag.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    flag
}

/// The status bar row of a 120x40 frame.
fn status_bar(app: &mut App) -> String {
    let t = draw(app, 120, 40);
    let s = screen(&t);
    s.lines().last().unwrap_or_default().to_string()
}

#[test]
fn quit_during_a_push_asks_and_quit_anyway_says_it_was_cancelled() {
    let f = Fixture::new();
    let remote = with_remote(&f);
    f.commit("a.txt", "one\ntwo\n", "second");
    let flag = blocking_hook(&f, "pre-push");
    let mut app = popup_app(&f);
    open(&mut app);
    push_dialog(&mut app);
    key(&mut app, KeyCode::Enter);
    assert!(popup(&app).job.is_some());
    // Hidden, the popup's running job is in the status bar.
    key(&mut app, KeyCode::Esc);
    assert!(!popup(&app).visible);
    let bar = status_bar(&mut app);
    assert!(bar.contains("push → origin/main…"), "{bar}");
    // ctrl+q asks, Cancel first; Cancel keeps vr-ide and the push.
    ctrl(&mut app, 'q');
    assert!(!app.quit);
    let (msg, buttons, sel) = confirm(&app);
    assert!(msg.contains("A git push is running"), "{msg}");
    assert_eq!(buttons, ["Quit anyway", "Cancel"]);
    assert_eq!(sel, 1);
    key(&mut app, KeyCode::Enter);
    assert!(!app.quit && app.dialog.is_none());
    assert!(popup(&app).job.is_some());
    // Quit anyway quits; the exit cancels the push and says so.
    ctrl(&mut app, 'q');
    key(&mut app, KeyCode::Left);
    key(&mut app, KeyCode::Enter);
    assert!(app.quit);
    crate::git_popup::shutdown(&mut app);
    let msgs = app.exit_messages.join(" | ");
    assert!(
        msgs.contains("git push → origin/main was cancelled"),
        "{msgs}"
    );
    std::fs::write(&flag, "").unwrap();
    assert_ne!(
        g(&f, &f.dir, &["rev-parse", "HEAD"]),
        g(&f, &remote, &["rev-parse", "main"]),
        "the push was cancelled"
    );
}

#[test]
fn slash_q_bang_during_a_commit_asks_and_names_the_cancelled_commit() {
    let f = Fixture::new();
    f.commit("a.txt", "one\n", "base");
    f.write("a.txt", "one\ntwo\n");
    f.git(&["add", "a.txt"]);
    let flag = blocking_hook(&f, "pre-commit");
    let mut app = popup_app(&f);
    open(&mut app);
    key(&mut app, KeyCode::Char('c'));
    typ(&mut app, "held");
    ctrl(&mut app, 's');
    key(&mut app, KeyCode::Esc);
    assert!(!popup(&app).visible);
    let bar = status_bar(&mut app);
    assert!(bar.contains("commit…"), "{bar}");
    crate::commands::app::quit_discarding(&mut app);
    assert!(!app.quit);
    let (msg, _, sel) = confirm(&app);
    assert!(msg.contains("A git commit is running"), "{msg}");
    assert_eq!(sel, 1);
    key(&mut app, KeyCode::Left);
    key(&mut app, KeyCode::Enter);
    assert!(app.quit);
    crate::git_popup::shutdown(&mut app);
    let msgs = app.exit_messages.join(" | ");
    assert!(msgs.contains("git commit was cancelled"), "{msgs}");
    std::fs::write(&flag, "").unwrap();
    assert_eq!(g(&f, &f.dir, &["log", "-1", "--format=%s"]).trim(), "base");
}

#[test]
fn quit_with_no_git_write_running_does_not_ask() {
    let f = Fixture::new();
    f.commit("a.txt", "one\n", "base");
    let mut app = popup_app(&f);
    open(&mut app);
    key(&mut app, KeyCode::Esc);
    ctrl(&mut app, 'q');
    assert!(app.quit, "{:?}", app.dialog.as_ref().map(|d| &d.action));
    crate::git_popup::shutdown(&mut app);
    assert!(app.exit_messages.is_empty(), "{:?}", app.exit_messages);
}
