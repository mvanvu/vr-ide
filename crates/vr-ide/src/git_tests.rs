//! M7-4a: git in the editor — gutter marks, status bar branch / ahead-behind / parent repo,
//! inline blame of the cursor line, multi-root, shutdown, SEC-15 sanitizing (AC-16, AC-16a,
//! AC-16b, AC-16d, AC-16e, AC-1). Temp repos are made with the git CLI in a hermetic
//! environment (own HOME, no system config), as in vr-git's tests.

use std::ffi::{OsStr, OsString};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use vr_git::{ChangeKind, LineChange};

use crate::app::{App, NoticeLevel};
use crate::app_tests::{space_i, test_app};
use crate::commands::CmdArgs;
use crate::workspace::Workspace;

/// Commit times start here and step one minute per git call.
const T0: i64 = 1_700_000_000;
const DAY: i64 = 86_400;

pub(crate) struct Fixture {
    _tmp: tempfile::TempDir,
    pub base: PathBuf,
    pub dir: PathBuf,
    home: PathBuf,
    time: std::cell::Cell<i64>,
}

impl Fixture {
    /// An initialised repo on branch `main` under `<tmp>/repo`.
    pub fn new() -> Fixture {
        let f = Fixture::bare_dir("repo");
        f.git(&["init", "-q", "-b", "main"]);
        f
    }

    /// A temp folder `<tmp>/<name>` with a hermetic home and no repo.
    pub fn bare_dir(name: &str) -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let home = base.join("home");
        let dir = base.join(name);
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(home.join(".gitconfig"), "").unwrap();
        Fixture {
            _tmp: tmp,
            base,
            dir,
            home,
            time: std::cell::Cell::new(T0),
        }
    }

    pub fn env(&self) -> Vec<(OsString, OsString)> {
        vec![
            ("PATH".into(), std::env::var_os("PATH").unwrap_or_default()),
            ("HOME".into(), self.home.clone().into()),
            ("XDG_CONFIG_HOME".into(), self.home.join(".config").into()),
            (
                "GIT_CONFIG_GLOBAL".into(),
                self.home.join(".gitconfig").into(),
            ),
            ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
            ("LC_ALL".into(), "C".into()),
        ]
    }

    pub fn options(&self) -> vr_git::Options {
        vr_git::Options {
            env: Some(self.env()),
            ..vr_git::Options::default()
        }
    }

    /// Runs git in `dir` as "Ann Author"; the commit time moves one minute per call.
    pub fn git_in<S: AsRef<OsStr>>(&self, dir: &Path, args: &[S]) -> String {
        self.git_as(dir, "Ann Author", args)
    }

    pub fn git_as<S: AsRef<OsStr>>(&self, dir: &Path, author: &str, args: &[S]) -> String {
        let t = self.time.get() + 60;
        self.time.set(t);
        let date = format!("{t} +0000");
        let out = Command::new("git")
            .current_dir(dir)
            .env_clear()
            .envs(self.env())
            .env("GIT_AUTHOR_NAME", author)
            .env("GIT_AUTHOR_EMAIL", "ann@example.com")
            .env("GIT_COMMITTER_NAME", author)
            .env("GIT_COMMITTER_EMAIL", "ann@example.com")
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {:?}: {}",
            args.iter()
                .map(|a| a.as_ref().to_owned())
                .collect::<Vec<_>>(),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    pub fn git<S: AsRef<OsStr>>(&self, args: &[S]) -> String {
        self.git_in(&self.dir, args)
    }

    pub fn write(&self, name: &str, text: &str) -> PathBuf {
        let p = self.dir.join(name);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).unwrap();
        }
        std::fs::write(&p, text).unwrap();
        p
    }

    /// Writes, stages and commits `name`.
    pub fn commit(&self, name: &str, text: &str, msg: &str) -> PathBuf {
        let p = self.write(name, text);
        self.git(&["add", "--", name]);
        self.git(&["commit", "-q", "--cleanup=verbatim", "-m", msg]);
        p
    }

    /// The time of the last git call (the last commit's date).
    pub fn last_time(&self) -> i64 {
        self.time.get()
    }
}

/// An app on `ws` with git started on the fixture's environment; debounce and rest are zero
/// unless a test sets them (timing has its own tests).
fn git_app(f: &Fixture, ws: Workspace) -> App {
    git_app_with(ws, f.options())
}

fn git_app_with(ws: Workspace, options: vr_git::Options) -> App {
    let (mut app, _) = test_app(&ws.primary().path.clone());
    app.workspace = ws;
    app.roots_changed();
    app.git.options = options;
    app.git.debounce = Duration::ZERO;
    app.git.rest = Duration::ZERO;
    app.start_git();
    app
}

pub(crate) fn single(f: &Fixture, root: &Path) -> App {
    git_app(f, Workspace::single(root.to_path_buf()))
}

/// [`single`] with git run through `options` (a shim).
pub(crate) fn single_with(root: &Path, options: vr_git::Options) -> App {
    git_app_with(Workspace::single(root.to_path_buf()), options)
}

/// Ticks (real time) until `cond` holds; fails after 15 s.
pub(crate) fn wait(app: &mut App, what: &str, mut cond: impl FnMut(&App) -> bool) {
    let end = Instant::now() + Duration::from_secs(15);
    loop {
        app.tick(Instant::now());
        if cond(app) {
            return;
        }
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The marks of the active document for its current text, once they arrived.
fn current_marks(app: &App) -> Option<Vec<LineChange>> {
    let doc = app.active_doc()?;
    let (version, marks) = app.git.marks_for(doc.path.as_deref()?)?;
    (version == doc.buffer.version()).then(|| marks.to_vec())
}

pub(crate) fn wait_marks(app: &mut App) -> Vec<LineChange> {
    let mut got = None;
    wait(app, "gutter marks", |a| {
        got = current_marks(a);
        got.is_some()
    });
    got.unwrap()
}

/// Gutter marks from `git diff -U0` (index vs worktree) of `name`.
fn git_diff_marks(f: &Fixture, name: &str) -> Vec<LineChange> {
    let out = f.git(&["diff", "-U0", "--no-color", "--", name]);
    let num = |s: &str| -> (u32, u32) {
        match s.split_once(',') {
            Some((a, b)) => (a.parse().unwrap(), b.parse().unwrap()),
            None => (s.parse().unwrap(), 1),
        }
    };
    out.lines()
        .filter_map(|l| l.strip_prefix("@@ -"))
        .map(|l| {
            let mut it = l.split(' ');
            let (_, old_len) = num(it.next().unwrap());
            let (new_start, new_len) = num(it.next().unwrap().trim_start_matches('+'));
            if new_len == 0 {
                LineChange {
                    kind: ChangeKind::Deleted,
                    start: new_start,
                    len: old_len,
                }
            } else if old_len == 0 {
                LineChange {
                    kind: ChangeKind::Added,
                    start: new_start - 1,
                    len: new_len,
                }
            } else {
                LineChange {
                    kind: ChangeKind::Modified,
                    start: new_start - 1,
                    len: new_len,
                }
            }
        })
        .collect()
}

fn key(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    app.handle_key(KeyEvent::new(code, mods));
}

fn typ(app: &mut App, s: &str) {
    for c in s.chars() {
        key(app, KeyCode::Char(c), KeyModifiers::NONE);
    }
}

pub(crate) fn draw(app: &mut App, w: u16, h: u16) -> Terminal<TestBackend> {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| crate::ui::render(f, app)).unwrap();
    t
}

pub(crate) fn row(t: &Terminal<TestBackend>, y: u16) -> String {
    let buf = t.backend().buffer();
    (0..buf.area.width)
        .map(|x| buf[(x, y)].symbol().to_string())
        .collect()
}

fn status_row(app: &mut App, w: u16) -> String {
    let t = draw(app, w, 24);
    row(&t, 23)
}

fn status_text(app: &App) -> String {
    crate::git::status_label(app).map_or_else(String::new, |l| l.full)
}

fn slash_run(app: &mut App, text: &str) {
    crate::slash_tests::slash(app, text);
    crate::slash_tests::key(app, KeyCode::Enter);
    assert!(
        app.palette.is_none(),
        "{text}: {:?}",
        crate::slash_tests::error(app)
    );
}

fn no_warnings(app: &App) {
    let bad: Vec<_> = app
        .notices
        .iter()
        .filter(|n| n.level != NoticeLevel::Info)
        .collect();
    assert!(bad.is_empty(), "{bad:?}");
}

// ---- gutter marks (AC-16a) ---------------------------------------------------------------

#[test]
fn marks_match_git_diff_for_saved_and_unsaved_edits() {
    let f = Fixture::new();
    let p = f.commit("a.txt", "one\ntwo\nthree\nfour\nfive\nsix\n", "init");
    f.write("a.txt", "one\nTWO\nthree\nfour\nfive\nsix\nseven\n");
    let mut app = single(&f, &f.dir);
    app.open_path(&p).unwrap();
    let saved = wait_marks(&mut app);
    assert_eq!(saved, git_diff_marks(&f, "a.txt"));
    assert_eq!(saved.len(), 2, "{saved:?}");

    // Unsaved: type on line 1, delete line 5 (the worktree is not touched).
    space_i(&mut app);
    typ(&mut app, "X");
    for _ in 0..4 {
        key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    }
    key(&mut app, KeyCode::Home, KeyModifiers::NONE);
    key(&mut app, KeyCode::Down, KeyModifiers::SHIFT);
    key(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
    assert_eq!(
        std::fs::read_to_string(&p).unwrap(),
        "one\nTWO\nthree\nfour\nfive\nsix\nseven\n"
    );
    let unsaved = wait_marks(&mut app);
    // Saving the buffer turns the same text into the worktree file git diffs.
    app.run_command("file.save", CmdArgs::None);
    assert_eq!(
        std::fs::read_to_string(&p).unwrap(),
        "Xone\nTWO\nthree\nfour\nsix\nseven\n"
    );
    assert_eq!(unsaved, git_diff_marks(&f, "a.txt"));
    assert!(
        unsaved.iter().any(|c| c.kind == ChangeKind::Deleted),
        "{unsaved:?}"
    );
    no_warnings(&app);
}

#[test]
fn marks_follow_git_line_ending_conversion() {
    let f = Fixture::new();
    f.commit(".gitattributes", "*.txt text=auto\n", "attrs");
    let p = f.commit("a.txt", "a\nb\nc\n", "init");
    // Checked out with CRLF (as on Windows): git diff sees no change.
    f.write("a.txt", "a\r\nb\r\nc\r\n");
    assert_eq!(git_diff_marks(&f, "a.txt"), vec![]);
    let mut app = single(&f, &f.dir);
    app.open_path(&p).unwrap();
    assert_eq!(
        app.active_doc().unwrap().buffer.text().to_string(),
        "a\r\nb\r\nc\r\n"
    );
    assert_eq!(wait_marks(&mut app), vec![]);
    space_i(&mut app);
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    typ(&mut app, "X");
    let marks = wait_marks(&mut app);
    app.run_command("file.save", CmdArgs::None);
    assert_eq!(marks, git_diff_marks(&f, "a.txt"));
    assert_eq!(
        marks,
        vec![LineChange {
            kind: ChangeKind::Modified,
            start: 1,
            len: 1
        }]
    );
}

#[test]
fn gutter_draws_the_three_marks_in_the_theme_colours() {
    let f = Fixture::new();
    let p = f.commit("a.txt", "one\ntwo\nthree\nfour\nfive\n", "init");
    // two modified, four deleted, six added.
    f.write("a.txt", "one\nTWO\nthree\nfive\nsix\n");
    let mut app = single(&f, &f.dir);
    app.open_path(&p).unwrap();
    wait_marks(&mut app);
    let t = draw(&mut app, 40, 12);
    let theme = crate::theme::Theme::from_name(&app.settings.theme);
    let gutter = crate::ui::editor_view::gutter_width(app.active_doc().unwrap().buffer.text());
    let buf = t.backend().buffer();
    let cell = |y: u16| &buf[(gutter - 1, y)];
    // Row 0 is the tab bar.
    assert_eq!(cell(1).symbol(), " ", "{}", row(&t, 1));
    assert_eq!(cell(2).symbol(), crate::ui::editor_view::MARK_CHANGE);
    assert_eq!(cell(2).fg, theme.git_modified.fg.unwrap());
    // "four" was removed after line 3: a bar at the bottom of line 3.
    assert_eq!(cell(3).symbol(), crate::ui::editor_view::MARK_DELETED_BELOW);
    assert_eq!(cell(3).fg, theme.git_deleted.fg.unwrap());
    assert_eq!(cell(4).symbol(), " ");
    assert_eq!(cell(5).symbol(), crate::ui::editor_view::MARK_CHANGE);
    assert_eq!(cell(5).fg, theme.git_added.fg.unwrap());
    // A deletion at the top of the file marks line 1 from above.
    let rows = crate::ui::editor_view::mark_rows(
        &[LineChange {
            kind: ChangeKind::Deleted,
            start: 0,
            len: 2,
        }],
        0,
        3,
    );
    assert_eq!(
        rows[0].map(|(s, _)| s),
        Some(crate::ui::editor_view::MARK_DELETED_ABOVE)
    );
    for name in ["synthwave", "dracula", "dark", "light"] {
        let th = crate::theme::Theme::named(name).unwrap();
        let colours = [th.git_added.fg, th.git_modified.fg, th.git_deleted.fg];
        assert!(colours.iter().all(Option::is_some), "{name}");
        assert!(
            colours[0] != colours[1] && colours[1] != colours[2] && colours[0] != colours[2],
            "{name}: {colours:?}"
        );
    }
}

#[test]
fn marks_follow_index_changes_made_outside() {
    let f = Fixture::new();
    let p = f.commit("a.txt", "one\ntwo\n", "init");
    f.write("a.txt", "one\nTWO\n");
    let mut app = single(&f, &f.dir);
    app.open_path(&p).unwrap();
    assert_eq!(wait_marks(&mut app).len(), 1);
    // Staged in the terminal: the buffer now equals the index (the .git watcher invalidates).
    f.git(&["add", "a.txt"]);
    wait(&mut app, "marks gone after git add", |a| {
        current_marks(a).is_some_and(|m| m.is_empty())
    });
}

#[test]
fn edits_are_debounced_before_the_marks_are_asked_for() {
    let f = Fixture::new();
    let p = f.commit("a.txt", "one\n", "init");
    let mut app = single(&f, &f.dir);
    app.git.debounce = Duration::from_millis(250);
    app.open_path(&p).unwrap();
    // A newly shown file is asked for at once.
    assert_eq!(wait_marks(&mut app), vec![]);
    space_i(&mut app);
    typ(&mut app, "X");
    let t0 = Instant::now();
    app.tick(t0);
    let asked = app.git.requests_sent();
    app.tick(t0 + Duration::from_millis(100));
    assert_eq!(app.git.requests_sent(), asked, "asked while typing");
    typ(&mut app, "Y");
    app.tick(t0 + Duration::from_millis(200));
    app.tick(t0 + Duration::from_millis(400));
    assert_eq!(
        app.git.requests_sent(),
        asked,
        "the pause restarts at each edit"
    );
    app.tick(t0 + Duration::from_millis(460));
    assert_eq!(app.git.requests_sent(), asked + 1);
}

// ---- status bar (AC-16b, D298) -----------------------------------------------------------

#[test]
fn status_bar_shows_branch_and_ahead_behind_like_git_status() {
    let f = Fixture::new();
    let p = f.commit("a.txt", "1\n", "c1");
    f.git(&["branch", "up"]);
    f.git(&["branch", "--set-upstream-to=up"]);
    f.commit("a.txt", "2\n", "c2");
    f.commit("a.txt", "3\n", "c3");
    f.git(&["checkout", "-q", "up"]);
    f.commit("b.txt", "b\n", "c4");
    f.git(&["checkout", "-q", "main"]);
    let sb = f.git(&["status", "-sb"]);
    assert!(sb.starts_with("## main...up [ahead 2, behind 1]"), "{sb}");
    let mut app = single(&f, &f.dir);
    app.open_path(&p).unwrap();
    wait(&mut app, "branch", |a| status_text(a).contains("↓1"));
    assert_eq!(status_text(&app), "main ↑2 ↓1");
    let bar = status_row(&mut app, 80);
    assert!(bar.contains("main ↑2 ↓1 · Ln 1, Col 1"), "{bar}");

    // A commit made in the terminal moves HEAD and the ref: the watcher refreshes.
    f.git(&["commit", "-q", "--allow-empty", "-m", "c5"]);
    wait(&mut app, "ahead 3", |a| status_text(a) == "main ↑3 ↓1");
    f.git(&["checkout", "-q", "-b", "feature"]);
    wait(&mut app, "new branch", |a| status_text(a) == "feature");
}

#[test]
fn detached_head_shows_the_short_sha() {
    let f = Fixture::new();
    let p = f.commit("a.txt", "1\n", "c1");
    f.commit("a.txt", "2\n", "c2");
    f.git(&["checkout", "-q", "--detach", "HEAD~1"]);
    let short = f.git(&["rev-parse", "--short", "HEAD"]).trim().to_string();
    let mut app = single(&f, &f.dir);
    app.open_path(&p).unwrap();
    wait(&mut app, "detached", |a| status_text(a).contains(&short));
    assert_eq!(status_text(&app), format!("detached {short}"));
    let label = crate::git::status_label(&app).unwrap();
    assert_eq!(label.short, short);
}

#[test]
fn parent_repo_is_labelled() {
    let f = Fixture::new();
    f.commit("sub/a.txt", "1\n", "c1");
    let root = f.dir.join("sub");
    let mut app = single(&f, &root);
    app.open_path(&root.join("a.txt")).unwrap();
    wait(&mut app, "branch", |a| !status_text(a).is_empty());
    let label = crate::git::status_label(&app).unwrap();
    assert!(label.full.starts_with("main"), "{}", label.full);
    assert!(label.full.contains("parent repo: "), "{}", label.full);
    assert!(label.full.ends_with("/repo"), "{}", label.full);
    assert!(label.short.contains("parent repo"), "{}", label.short);
    // The root that is the repo itself has no label.
    let mut own = single(&f, &f.dir);
    own.open_path(&root.join("a.txt")).unwrap();
    wait(&mut own, "branch", |a| !status_text(a).is_empty());
    assert_eq!(status_text(&own), "main");
}

#[test]
fn status_bar_without_a_file_shows_the_first_roots_branch() {
    let f = Fixture::new();
    f.commit("a.txt", "1\n", "c1");
    let mut app = single(&f, &f.dir);
    wait(&mut app, "branch", |a| status_text(a) == "main");
    let bar = status_row(&mut app, 80);
    assert!(bar.contains("repo · main"), "{bar}");
}

// ---- no repo, untracked, multi-root (AC-16d) ---------------------------------------------

#[test]
fn folder_without_git_shows_nothing_and_no_errors() {
    let f = Fixture::bare_dir("plain");
    let out = Command::new("git")
        .current_dir(&f.dir)
        .envs(f.env())
        .args(["rev-parse", "--git-dir"])
        .output()
        .unwrap();
    assert!(!out.status.success(), "the temp folder is inside a repo");
    let p = f.dir.join("a.txt");
    std::fs::write(&p, "x\n").unwrap();
    let mut app = single(&f, &f.dir);
    app.open_path(&p).unwrap();
    slash_run(&mut app, "/set git_inline_blame true");
    wait(&mut app, "discovery", |a| !a.git.discovering());
    let t = Instant::now();
    for i in 0..5 {
        app.tick(t + Duration::from_millis(400 * i));
    }
    assert_eq!(crate::git::status_label(&app), None);
    assert_eq!(app.git.marks_for(&p), None);
    assert_eq!(crate::git::inline_text(&app), None);
    assert_eq!(app.git.repo_count(), 0);
    no_warnings(&app);
    let bar = status_row(&mut app, 80);
    assert!(bar.contains("a.txt · Ln 1, Col 1"), "{bar}");
}

#[test]
fn untracked_file_and_empty_repo_show_no_marks() {
    let f = Fixture::new();
    let p = f.write("new.txt", "x\n");
    let mut app = single(&f, &f.dir);
    app.open_path(&p).unwrap();
    assert_eq!(wait_marks(&mut app), vec![]);
    // Unborn branch: the name still shows.
    wait(&mut app, "branch", |a| status_text(a) == "main");
    no_warnings(&app);
}

#[test]
fn each_root_shows_its_own_repo() {
    let a = Fixture::new();
    let pa = a.commit("a.txt", "a\n", "a1");
    let b = Fixture::new();
    b.git(&["checkout", "-q", "-b", "dev"]);
    let pb = b.commit("b.txt", "b\n", "b1");
    let c = Fixture::bare_dir("plain");
    let pc = c.dir.join("c.txt");
    std::fs::write(&pc, "c\n").unwrap();
    let (ws, refused) =
        Workspace::from_paths(&[a.dir.clone(), b.dir.clone(), c.dir.clone()]).unwrap();
    assert!(refused.is_empty());
    let mut app = git_app(&a, ws);
    app.open_path(&pa).unwrap();
    wait(&mut app, "root a", |x| status_text(x) == "main");
    // Its marks too: a's first status answer may still be in flight when b opens.
    assert_eq!(wait_marks(&mut app), vec![]);
    app.open_path(&pb).unwrap();
    wait(&mut app, "root b", |x| status_text(x) == "dev");
    std::fs::write(&pb, "B\n").unwrap();
    app.handle_fs_event(vr_core::FsEvent::Modified(pb.clone()));
    let marks = wait_marks(&mut app);
    assert_eq!(marks.len(), 1);
    app.open_path(&pc).unwrap();
    wait(&mut app, "discovery", |x| !x.git.discovering());
    assert_eq!(crate::git::status_label(&app), None);
    assert_eq!(app.git.marks_for(&pc), None);
    // Drawn marks: a late .git event of repo a (under load) may mark them not fresh, and
    // only the active file is asked again; what is drawn for a stays its own.
    assert_eq!(app.git.shown_marks(&pa).map(<[_]>::len), Some(0));
    assert_eq!(app.git.repo_count(), 2);
}

#[test]
fn reopened_file_changed_on_disk_while_closed_gets_fresh_marks() {
    let f = Fixture::new();
    let p = f.commit("sub/deep/c.txt", "one\ntwo\n", "init");
    let mut app = single(&f, &f.dir);
    app.open_path(&p).unwrap();
    assert_eq!(wait_marks(&mut app), vec![]);
    let i = app.active;
    app.close_doc(i);
    app.tick(Instant::now());
    // The folder is not expanded and the tab is closed: no watch sees this change.
    f.write("sub/deep/c.txt", "one\ntwo\nthree\n");
    app.open_path(&p).unwrap();
    let marks = wait_marks(&mut app);
    assert_eq!(marks, git_diff_marks(&f, "sub/deep/c.txt"));
    assert_eq!(marks.len(), 1, "{marks:?}");
}

#[test]
fn reopened_file_changed_on_disk_while_closed_gets_a_fresh_blame() {
    let f = Fixture::new();
    let p = f.commit("c.txt", "alpha\n", "Add alpha");
    let mut app = single(&f, &f.dir);
    app.settings.git_inline_blame = true;
    app.open_path(&p).unwrap();
    wait(&mut app, "blame", |a| crate::git::inline_text(a).is_some());
    let first = crate::git::inline_text(&app).unwrap();
    assert!(first.starts_with("Ann Author"), "{first}");
    let i = app.active;
    app.close_doc(i);
    app.tick(Instant::now());
    f.write("c.txt", "new\nalpha\n");
    app.open_path(&p).unwrap();
    wait(&mut app, "blame", |a| crate::git::inline_text(a).is_some());
    assert_eq!(
        crate::git::inline_text(&app).as_deref(),
        Some("You, uncommitted")
    );
}

#[test]
fn a_root_that_becomes_a_repo_later_is_found_and_lost_again() {
    let f = Fixture::bare_dir("plain");
    let p = f.dir.join("a.txt");
    std::fs::write(&p, "one\n").unwrap();
    let mut app = single(&f, &f.dir);
    app.open_path(&p).unwrap();
    wait(&mut app, "discovery", |a| !a.git.discovering());
    assert_eq!(app.git.repo_count(), 0);
    // Another entry of the root is no reason to look again.
    app.handle_fs_event(vr_core::FsEvent::Created(f.dir.join(".gitignore")));
    assert!(!app.git.discovering());

    // `git init` and a commit in the integrated terminal: the root's watch sees `.git` appear.
    f.git(&["init", "-q", "-b", "main"]);
    f.git(&["add", "a.txt"]);
    f.git(&["commit", "-q", "-m", "init"]);
    app.handle_fs_event(vr_core::FsEvent::Created(f.dir.join(".git")));
    wait(&mut app, "branch", |a| status_text(a) == "main");
    std::fs::write(&p, "one\ntwo\n").unwrap();
    app.handle_fs_event(vr_core::FsEvent::Modified(p.clone()));
    let marks = wait_marks(&mut app);
    assert_eq!(marks, git_diff_marks(&f, "a.txt"));
    assert_eq!(marks.len(), 1);

    // `.git` removed: the root has no repo again and its worker ends.
    std::fs::remove_dir_all(f.dir.join(".git")).unwrap();
    app.handle_fs_event(vr_core::FsEvent::Removed(f.dir.join(".git")));
    wait(&mut app, "repo gone", |a| {
        !a.git.discovering() && a.git.repo_count() == 0
    });
    assert_eq!(crate::git::status_label(&app), None);
    assert_eq!(app.git.marks_for(&p), None);
}

#[test]
fn a_git_folder_seen_before_git_init_finished_is_found_when_done() {
    let f = Fixture::bare_dir("plain");
    let p = f.dir.join("a.txt");
    std::fs::write(&p, "one\n").unwrap();
    let mut app = single(&f, &f.dir);
    app.open_path(&p).unwrap();
    wait(&mut app, "discovery", |a| !a.git.discovering());
    // The root's watch reports `.git` as soon as `git init` / `git clone` makes the folder,
    // before git takes it as a repo.
    std::fs::create_dir(f.dir.join(".git")).unwrap();
    app.handle_fs_event(vr_core::FsEvent::Created(f.dir.join(".git")));
    wait(&mut app, "discovery", |a| !a.git.discovering());
    assert_eq!(app.git.repo_count(), 0);
    // git finishes inside `.git`: no event of the root's watch, only the `.git` folder's.
    f.git(&["init", "-q", "-b", "main"]);
    f.git(&["add", "a.txt"]);
    f.git(&["commit", "-q", "-m", "init"]);
    wait(&mut app, "branch", |a| status_text(a) == "main");
    assert_eq!(app.git.repo_count(), 1);
}

// ---- shutdown --------------------------------------------------------------------------------

#[test]
fn removing_a_root_and_exit_shut_the_git_workers_down() {
    let a = Fixture::new();
    a.commit("a.txt", "a\n", "a1");
    let b = Fixture::new();
    b.commit("b.txt", "b\n", "b1");
    let (ws, _) = Workspace::from_paths(&[a.dir.clone(), b.dir.clone()]).unwrap();
    let mut app = git_app(&a, ws);
    wait(&mut app, "both repos", |x| x.git.repo_count() == 2);
    // Reader clones share the worker's runner: removing the root kills their reads too.
    let ra = app.git.repo_of_root(&a.dir).unwrap();
    let rb = app.git.repo_of_root(&b.dir).unwrap();
    let id = app.workspace.root_of(&b.dir).unwrap().id;
    app.remove_root(id).unwrap();
    assert_eq!(app.git.repo_count(), 1);
    assert_eq!(rb.head(), Err(vr_git::GitError::ShutDown));
    assert!(ra.head().is_ok());
    let state = tempfile::tempdir().unwrap();
    let r = crate::event_loop::exit(
        Ok(()),
        &mut app,
        &state.path().join("recovery"),
        &mut || {},
        Duration::ZERO,
    );
    assert!(r.is_ok());
    assert_eq!(ra.head(), Err(vr_git::GitError::ShutDown));
}

// ---- AC-1 / AC-16e: nothing blocks the UI thread -----------------------------------------------

/// A `git` that sleeps 2 s before every command.
fn slow_git(f: &Fixture) -> vr_git::Options {
    shim_git(f, "sleep 2\nexec {git} \"$@\"\n")
}

/// Options whose `git` is the shell script `body` (`{git}` = the real git), first in PATH.
pub(crate) fn shim_git(f: &Fixture, body: &str) -> vr_git::Options {
    let bin = f.base.join("slowbin");
    std::fs::create_dir_all(&bin).unwrap();
    let real = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .unwrap();
    let real = String::from_utf8_lossy(&real.stdout).trim().to_string();
    let shim = bin.join("git");
    let body = body.replace("{git}", &real);
    std::fs::write(&shim, format!("#!/bin/sh\n{body}")).unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut opts = f.options();
    let env = opts.env.as_mut().unwrap();
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut p = OsString::from(bin.as_os_str());
    p.push(":");
    p.push(path);
    env[0] = ("PATH".into(), p);
    opts
}

/// `pid` runs (exists and is not a zombie).
fn alive(pid: u32) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    let state = stat
        .rsplit_once(')')
        .and_then(|(_, r)| r.trim_start().chars().next());
    !matches!(state, Some('Z' | 'X') | None)
}

#[test]
fn a_hung_discovery_git_is_killed_at_exit() {
    let f = Fixture::new();
    f.commit("a.txt", "a\n", "c1");
    let pidfile = f.base.join("hung.pid");
    // Every git hangs (a dead network mount); its child shows that the whole group dies.
    let opts = shim_git(
        &f,
        &format!("sleep 60 &\necho $! > '{}'\nwait\n", pidfile.display()),
    );
    let (mut app, _) = test_app(&f.dir);
    app.git.options = opts;
    app.start_git();
    let end = Instant::now() + Duration::from_secs(15);
    let pid = loop {
        if let Some(pid) = std::fs::read_to_string(&pidfile)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
        {
            break pid;
        }
        assert!(Instant::now() < end, "the discovery git never started");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(app.git.discovering() && alive(pid));
    let state = tempfile::tempdir().unwrap();
    let t = Instant::now();
    let r = crate::event_loop::exit(
        Ok(()),
        &mut app,
        &state.path().join("recovery"),
        &mut || {},
        Duration::ZERO,
    );
    assert!(r.is_ok());
    assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
    let end = Instant::now() + Duration::from_secs(5);
    while alive(pid) && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(10));
    }
    let survived = alive(pid);
    if survived {
        let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
    }
    assert!(!survived, "the discovery git outlived the exit");
}

#[test]
fn git_never_runs_on_the_ui_thread() {
    let f = Fixture::new();
    let p = f.commit("a.txt", "a\n", "c1");
    let (mut app, _) = test_app(&f.dir);
    app.git.options = slow_git(&f);
    app.git.debounce = Duration::ZERO;
    app.settings.git_inline_blame = true;
    // Nothing starts before start_git (main calls it after the first frame).
    app.tick(Instant::now());
    assert!(!app.git.discovering() && app.git.repo_count() == 0);
    let t = Instant::now();
    app.start_git();
    app.open_path(&p).unwrap();
    for _ in 0..20 {
        app.tick(Instant::now());
        let _ = draw(&mut app, 80, 24);
    }
    assert!(
        t.elapsed() < Duration::from_millis(1500),
        "the UI thread waited for git: {:?}",
        t.elapsed()
    );
    app.git.shutdown();
}

// ---- inline blame (AC-16) --------------------------------------------------------------------

/// Ticks at the fake time `at` until the blame of the active document's current text has
/// arrived (real time passes meanwhile; the fake clock does not move).
fn wait_blame(app: &mut App, at: Instant) {
    let end = Instant::now() + Duration::from_secs(15);
    while !app.git.blame_ready() {
        app.tick(at);
        assert!(Instant::now() < end, "no blame");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn blame_line_shows_author_age_and_subject_after_a_rest() {
    let f = Fixture::new();
    let p = f.commit("a.txt", "alpha\n", "Add alpha\n\nThe body is not shown.");
    let when = f.last_time();
    f.write("a.txt", "alpha\nbeta\n");
    f.git(&["add", "a.txt"]);
    f.git_as(&f.dir, "Bob Builder", &["commit", "-q", "-m", "Add beta"]);
    let mut app = single(&f, &f.dir);
    app.git.debounce = Duration::from_millis(250);
    app.git.rest = Duration::from_millis(300);
    app.git.now_secs = Some(when + 3 * DAY + 3600);
    assert!(!app.settings.git_inline_blame, "off by default");
    app.open_path(&p).unwrap();
    slash_run(&mut app, "/set git_inline_blame true");
    assert!(app.settings.git_inline_blame);
    let t0 = Instant::now();
    wait_blame(&mut app, t0);
    app.tick(t0 + Duration::from_millis(200));
    assert_eq!(crate::git::inline_text(&app), None, "shown before the rest");
    app.tick(t0 + Duration::from_millis(320));
    assert_eq!(
        crate::git::inline_text(&app).as_deref(),
        Some("Ann Author, 3 days ago • Add alpha")
    );
    let screen = row(&draw(&mut app, 80, 24), 1);
    assert!(
        screen.contains("alpha  Ann Author, 3 days ago • Add alpha"),
        "{screen}"
    );
    // Another line: hidden until the cursor rests there.
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    app.tick(t0 + Duration::from_millis(400));
    assert_eq!(crate::git::inline_text(&app), None);
    app.tick(t0 + Duration::from_millis(710));
    assert_eq!(
        crate::git::inline_text(&app).as_deref(),
        Some("Bob Builder, 3 days ago • Add beta")
    );
    // Typing hides it; after the pause the line is the user's own.
    space_i(&mut app);
    typ(&mut app, "x");
    app.tick(t0 + Duration::from_millis(800));
    assert_eq!(crate::git::inline_text(&app), None, "shown while typing");
    let t1 = t0 + Duration::from_millis(1100);
    wait_blame(&mut app, t1);
    app.tick(t1 + Duration::from_millis(300));
    assert_eq!(
        crate::git::inline_text(&app).as_deref(),
        Some("You, uncommitted")
    );
    slash_run(&mut app, "/set git_inline_blame false");
    assert!(!app.settings.git_inline_blame);
    app.tick(t1 + Duration::from_millis(400));
    assert_eq!(crate::git::inline_text(&app), None);
}

#[test]
fn inline_blame_is_switched_by_the_setting_and_there_is_no_git_slash_command() {
    assert!(
        crate::commands::slash::SLASH_COMMANDS
            .iter()
            .all(|c| !c.name.starts_with("git")),
        "AC-F4: no /git command"
    );
    let (mut app, _) = test_app(Path::new("/nonexistent-root"));
    crate::slash_tests::slash(&mut app, "/set git_inline_blame true");
    crate::slash_tests::key(&mut app, KeyCode::Enter);
    assert!(app.settings.git_inline_blame);
}

#[test]
fn relative_ages_read_like_gitlens() {
    use crate::git::relative_age;
    assert_eq!(relative_age(5), "just now");
    assert_eq!(relative_age(60), "1 minute ago");
    assert_eq!(relative_age(3 * 3600 + 5), "3 hours ago");
    assert_eq!(relative_age(DAY), "1 day ago");
    assert_eq!(relative_age(3 * DAY + 3600), "3 days ago");
    assert_eq!(relative_age(45 * DAY), "1 month ago");
    assert_eq!(relative_age(800 * DAY), "2 years ago");
    // A commit dated in the future (clock skew).
    assert_eq!(relative_age(-100), "just now");
}

// ---- SEC-15 ------------------------------------------------------------------------------------

#[test]
fn hostile_git_strings_are_drawn_without_control_or_bidi_chars() {
    let f = Fixture::new();
    let evil = "x\x1b[2J\x1b]52;c;ZXZpbA==\x07\u{9b}31m\r\u{202e}y";
    let p = f.write("a.txt", "line one\n");
    f.git(&["add", "a.txt"]);
    f.git_as(
        &f.dir,
        &format!("Mallory {evil}"),
        &[
            "commit",
            "-q",
            "--cleanup=verbatim",
            "-m",
            &format!("subj {evil}"),
        ],
    );
    // git refuses ASCII controls in ref names but takes C1 and BIDI ones.
    f.git(&["checkout", "-q", "-b", "fix\u{9b}\u{202e}gnp.exe"]);
    let mut app = single(&f, &f.dir);
    app.git.now_secs = Some(f.last_time() + 60);
    app.settings.git_inline_blame = true;
    app.open_path(&p).unwrap();
    wait(&mut app, "branch", |a| status_text(a).starts_with("fix"));
    wait(&mut app, "inline blame", |a| {
        crate::git::inline_text(a).is_some()
    });
    let text = crate::git::inline_text(&app).unwrap();
    assert!(
        text.contains("^[[2J") && text.contains("<U+202E>"),
        "{text}"
    );
    for (w, h) in [(120, 40), (80, 24), (40, 12)] {
        let t = draw(&mut app, w, h);
        let buf = t.backend().buffer();
        let mut seen = String::new();
        for y in 0..h {
            for x in 0..w {
                let s = buf[(x, y)].symbol();
                assert!(
                    !s.chars().any(crate::ui::safe_text::is_unsafe_char),
                    "{w}x{h} ({x},{y}) {s:?}"
                );
                seen.push_str(s);
            }
        }
        assert!(seen.contains("<U+202E>") || w == 40, "{w}x{h}");
    }
}

// ---- snapshots (gutter + status bar + inline blame) ---------------------------------------------

#[test]
fn render_git_gutter_status_and_blame() {
    let f = Fixture::new();
    let p = f.commit(
        "src/lib.rs",
        "fn one() {}\nfn two() {}\nfn three() {}\nfn four() {}\nfn five() {}\n",
        "Add the numbers",
    );
    f.git(&["branch", "up"]);
    f.git(&["branch", "--set-upstream-to=up"]);
    f.commit(
        "src/lib.rs",
        "fn one() {}\nfn two() {}\nfn three() {}\nfn four() {}\nfn five() {}\nfn six() {}\n",
        "Add six",
    );
    let when = f.last_time();
    // Worktree: two changed, four removed, seven added.
    f.write(
        "src/lib.rs",
        "fn one() {}\nfn two(x: u8) {}\nfn three() {}\nfn five() {}\nfn six() {}\nfn seven() {}\n",
    );
    let mut app = single(&f, &f.dir);
    app.git.now_secs = Some(when + 2 * 3600);
    app.settings.git_inline_blame = true;
    app.open_path(&p).unwrap();
    wait_marks(&mut app);
    wait(&mut app, "branch", |a| status_text(a) == "main ↑1");
    wait(&mut app, "blame", |a| crate::git::inline_text(a).is_some());
    for (w, h) in [(120, 40), (80, 24), (40, 12)] {
        let t = draw(&mut app, w, h);
        insta::assert_snapshot!(
            format!("render_git_gutter_status_and_blame_{w}x{h}"),
            t.backend()
        );
    }
}

// ---- final review 2 (FR-16) ---------------------------------------------------------------

#[test]
fn an_explorer_rename_and_move_update_recent_files_and_drop_the_old_marks() {
    let f = Fixture::new();
    let p = f.commit("a.txt", "one\ntwo\n", "init");
    f.write("a.txt", "one\nTWO\n");
    std::fs::create_dir(f.dir.join("sub")).unwrap();
    let state = f.base.join("state");
    std::fs::create_dir(&state).unwrap();
    let list = state.join(vr_config::RECENT_FILES_FILE);
    let mut app = single(&f, &f.dir);
    app.recent_files_file = Some(list.clone());
    app.open_path(&p).unwrap();
    wait_marks(&mut app);
    // Rename (F2 / the menu's Rename / `/rename` all go through `rename_in`).
    let b = crate::commands::explorer::rename_in(&mut app, &f.dir.clone(), &p, "b.txt")
        .unwrap()
        .unwrap();
    assert_eq!(app.git.marks_for(&p), None, "the old path's marks are gone");
    app.flush_recent_files();
    let (on_disk, _) = vr_config::RecentFiles::load(&list);
    assert_eq!(on_disk.entries.first(), Some(&b), "{:?}", on_disk.entries);
    assert!(!on_disk.entries.contains(&p), "{:?}", on_disk.entries);
    assert!(app.recent_files.entries.contains(&b) && !app.recent_files.entries.contains(&p));
    // The new path gets its own marks, which the move below must drop again.
    wait_marks(&mut app);
    // Cut + paste moves it into sub/.
    app.explorer.visible = true;
    crate::commands::explorer::ensure_loaded(&mut app);
    app.run_command("explorer.focus", CmdArgs::None);
    let at = |app: &mut App, path: &Path| {
        crate::commands::explorer::ensure_loaded(app);
        let i = app
            .explorer
            .rows
            .iter()
            .position(|r| r.path == path)
            .expect("explorer row");
        app.explorer.cursor = i;
        app.explorer.selected.clear();
    };
    at(&mut app, &b);
    key(&mut app, KeyCode::Char('x'), KeyModifiers::CONTROL);
    at(&mut app, &f.dir.join("sub"));
    key(&mut app, KeyCode::Char('v'), KeyModifiers::CONTROL);
    let c = f.dir.join("sub/b.txt");
    assert!(c.is_file() && !b.exists());
    assert_eq!(app.git.marks_for(&b), None, "the old path's marks are gone");
    app.flush_recent_files();
    let (on_disk, _) = vr_config::RecentFiles::load(&list);
    assert_eq!(on_disk.entries.first(), Some(&c), "{:?}", on_disk.entries);
    assert!(!on_disk.entries.contains(&b), "{:?}", on_disk.entries);
    // The new path gets its own marks.
    wait_marks(&mut app);
}

#[test]
fn the_welcome_branch_comes_from_vr_git_not_a_planted_parent_head() {
    // A `.git` above the root that git does not accept as a repository.
    let f = Fixture::bare_dir("proj");
    let planted = f.base.join(".git");
    std::fs::create_dir(&planted).unwrap();
    std::fs::write(planted.join("HEAD"), "ref: refs/heads/planted\u{1b}[31m\n").unwrap();
    let mut app = single(&f, &f.dir);
    wait(&mut app, "discovery", |a| !a.git.discovering());
    assert_eq!(app.git.branch_of_root(&f.dir), None);
    let t = Instant::now();
    for i in 0..3 {
        app.tick(t + Duration::from_secs(5 * i));
    }
    assert_eq!(app.monitor.branch, None);
    // A real repo root: the branch vr-git read, sanitized.
    let g = Fixture::new();
    g.commit("a.txt", "1\n", "c1");
    // git refuses control characters in branch names; the read is sanitized all the same.
    g.git(&["checkout", "-q", "-b", "feat/x"]);
    let mut app = single(&g, &g.dir);
    wait(&mut app, "branch", |a| {
        a.git.branch_of_root(&g.dir).is_some()
    });
    let b = app.git.branch_of_root(&g.dir).unwrap();
    assert_eq!(b, "feat/x");
    let end = Instant::now() + Duration::from_secs(15);
    while app.monitor.branch.as_deref() != Some(b.as_str()) {
        assert!(
            Instant::now() < end,
            "welcome branch {:?}",
            app.monitor.branch
        );
        app.tick(Instant::now());
        std::thread::sleep(Duration::from_millis(20));
    }
}
