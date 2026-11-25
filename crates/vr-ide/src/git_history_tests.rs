//! M7-4b: the Git popup's History and Blame tabs and the commit view (AC-16c, AC-16e).
//! History is compared with `git log [--follow]`, blame with `git blame --porcelain`, the
//! commit view with `git show`; hostile authors, messages and file names are drawn safely
//! (SEC-15); user-typed revisions go through `rev-parse --verify` (SEC-5); a slow blame shows
//! "blaming…" while the keys keep working. Real git, hermetic environment.

use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

use crate::app::App;
use crate::app_tests::space_i;
use crate::git_history::CvRow;
use crate::git_popup::Tab;
use crate::git_popup_tests::{
    HOSTILE, assert_safe, click, draw, errors, fixed_env, g, key, notices, options, popup,
    popup_app, screen, wait,
};
use crate::git_tests::Fixture;

fn slash_run(app: &mut App, text: &str) {
    // F1 goes to the command window only while the popup is hidden.
    if let Some(p) = &mut app.git_popup {
        p.visible = false;
    }
    crate::slash_tests::slash(app, text);
    crate::slash_tests::key(app, KeyCode::Enter);
}

fn oids(list: &str) -> Vec<String> {
    list.lines().map(str::to_string).collect()
}

fn shown_oids(app: &App) -> Vec<String> {
    popup(app)
        .history
        .commits
        .iter()
        .map(|c| c.oid.clone())
        .collect()
}

fn history_loaded(a: &App) -> bool {
    a.git_popup
        .as_ref()
        .is_some_and(|p| p.tab == Tab::History && !p.history.loading && p.history.loaded())
}

fn blame_done(a: &App) -> bool {
    a.git_popup
        .as_ref()
        .is_some_and(|p| p.tab == Tab::Blame && !p.blame.running && p.blame.result.is_some())
}

fn commit_done(a: &App) -> bool {
    a.git_popup
        .as_ref()
        .and_then(|p| p.commit.as_ref())
        .is_some_and(|c| !c.loading)
}

/// A repo whose `b.txt` was `a.txt` before a rename, with commits to other files between.
fn renamed_repo() -> (Fixture, PathBuf) {
    let f = Fixture::new();
    f.commit("a.txt", "1\n", "add a");
    f.commit("other.txt", "x\n", "other");
    f.commit("a.txt", "1\n2\n", "edit a");
    f.git(&["mv", "a.txt", "b.txt"]);
    f.git(&["commit", "-q", "-m", "rename to b"]);
    let p = f.commit("b.txt", "1\n2\n3\n", "edit b");
    f.commit("other.txt", "y\n", "other 2");
    (f, p)
}

// ---- History (AC-16c: matches git log --follow) ------------------------------------------------

#[test]
fn history_of_the_repo_and_of_a_renamed_file_match_git_log() {
    let (f, p) = renamed_repo();
    let mut app = popup_app(&f);
    app.open_path(&p).unwrap();
    slash_run(&mut app, "/git log file");
    wait(&mut app, "file history", history_loaded);
    assert!(app.git_popup_shown());
    let want = oids(&g(
        &f,
        &f.dir,
        &["log", "--follow", "--format=%H", "--", "b.txt"],
    ));
    assert_eq!(want.len(), 4, "the rename is followed");
    assert_eq!(shown_oids(&app), want);
    assert!(popup(&app).history.complete);
    let t = draw(&mut app, 120, 40);
    let s = screen(&t);
    assert!(s.contains("History · b.txt"), "{s}");
    let short = g(&f, &f.dir, &["log", "-1", "--format=%h"]);
    assert!(s.contains("rename to b"), "{s}");
    assert!(s.contains("Ann Author"), "{s}");
    // The newest commit of the file (not of the repo) heads the list.
    assert!(
        !s.contains(short.trim()),
        "repo HEAD is not in the file's history\n{s}"
    );

    key(&mut app, KeyCode::Esc);
    slash_run(&mut app, "/git log");
    wait(&mut app, "repo history", |a| {
        history_loaded(a) && popup(a).history.path.is_none()
    });
    let want = oids(&g(&f, &f.dir, &["log", "--format=%H"]));
    assert_eq!(want.len(), 6);
    assert_eq!(shown_oids(&app), want);
    let t = draw(&mut app, 120, 40);
    assert!(screen(&t).contains(short.trim()), "{}", screen(&t));
}

/// `n` commits of `log.txt` through fast-import (one git call).
fn many_commits(f: &Fixture, n: usize) {
    let mut data = String::new();
    for i in 1..=n {
        let text = format!("{i}\n");
        let t = 1_700_000_000 + i * 60;
        data.push_str(&format!(
            "commit refs/heads/main\nauthor Ann Author <ann@example.com> {t} +0000\ncommitter Ann Author <ann@example.com> {t} +0000\ndata {}\nc{i}\n",
            format!("c{i}\n").len()
        ));
        data.push_str(&format!(
            "M 100644 inline log.txt\ndata {}\n{text}\n",
            text.len()
        ));
    }
    let mut child = Command::new("git")
        .current_dir(&f.dir)
        .env_clear()
        .envs(fixed_env(f))
        .args(["fast-import", "--quiet"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(data.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    g(f, &f.dir, &["reset", "-q", "--hard", "main"]);
}

#[test]
fn history_loads_more_commits_while_scrolling() {
    let f = Fixture::new();
    many_commits(&f, 450);
    let mut app = popup_app(&f);
    slash_run(&mut app, "/git log");
    wait(&mut app, "first page", history_loaded);
    let page = crate::git_history::PAGE;
    assert_eq!(popup(&app).history.commits.len(), page);
    assert!(!popup(&app).history.complete);
    // End goes to the last loaded row; the next page loads behind it.
    key(&mut app, KeyCode::End);
    wait(&mut app, "second page", |a| {
        history_loaded(a) && popup(a).history.commits.len() == 2 * page
    });
    assert_eq!(
        popup(&app).history.selected,
        page - 1,
        "the selection stays"
    );
    // The mouse wheel at the end loads the rest.
    key(&mut app, KeyCode::End);
    wait(&mut app, "all", |a| {
        history_loaded(a) && popup(a).history.complete
    });
    assert_eq!(
        shown_oids(&app),
        oids(&g(&f, &f.dir, &["log", "--format=%H"]))
    );
    // Only the visible rows are drawn (a virtual list): the screen shows the selection.
    let t = draw(&mut app, 80, 24);
    assert!(
        screen(&t).contains(&format!("c{}", 450 - (2 * page - 1))),
        "{}",
        screen(&t)
    );
}

// ---- Blame (AC-16c: matches git blame --porcelain) -----------------------------------------------

/// Line number (1-based) -> (commit, author) of `git blame --porcelain`.
fn porcelain(f: &Fixture, file: &str, contents: Option<&Path>) -> HashMap<u32, (String, String)> {
    let mut args = vec!["blame", "--porcelain"];
    if let Some(c) = contents {
        args.extend(["--contents", c.to_str().unwrap()]);
    }
    args.extend(["--", file]);
    let out = g(f, &f.dir, &args);
    let mut authors: HashMap<String, String> = HashMap::new();
    let mut lines = HashMap::new();
    let mut cur: Option<(String, u32)> = None;
    for l in out.lines() {
        let parts: Vec<&str> = l.split(' ').collect();
        if parts[0].len() == 40
            && parts[0].bytes().all(|b| b.is_ascii_hexdigit())
            && parts.len() >= 3
        {
            cur = Some((parts[0].to_string(), parts[2].parse().unwrap()));
        } else if let Some(a) = l.strip_prefix("author ") {
            authors.insert(cur.as_ref().unwrap().0.clone(), a.to_string());
        } else if l.starts_with('\t') {
            let (oid, n) = cur.take().unwrap();
            lines.insert(n, (oid, String::new()));
        }
    }
    for v in lines.values_mut() {
        v.1 = authors[&v.0].clone();
    }
    lines
}

fn blame_repo() -> (Fixture, PathBuf) {
    let f = Fixture::new();
    let p = f.commit("b.rs", "a\nb\nc\nd\n", "first");
    f.write("b.rs", "a\nB\nc\nd\n");
    f.git(&["add", "b.rs"]);
    f.git_as(&f.dir, "Bea Blame", &["commit", "-q", "-m", "bea"]);
    f.write("b.rs", "a\nB\nc\nd\ne\n");
    f.git(&["add", "b.rs"]);
    f.git_as(&f.dir, "Cid Commit", &["commit", "-q", "-m", "cid"]);
    (f, p)
}

fn shown_blame(app: &App) -> Vec<(String, String, bool)> {
    let b = popup(app).blame.result.as_ref().unwrap().as_ref().unwrap();
    let b = b.as_ref().expect("tracked file");
    b.blame
        .lines
        .iter()
        .map(|l| {
            let c = &b.blame.commits[l.commit];
            (c.oid.clone(), c.author.name.clone(), c.uncommitted)
        })
        .collect()
}

#[test]
fn blame_matches_git_blame_porcelain_and_marks_uncommitted_lines() {
    let (f, p) = blame_repo();
    // A worktree change not committed: git blame shows it as "Not Committed Yet".
    f.write("b.rs", "a\nB\nc\nnew\nd\ne\n");
    let mut app = popup_app(&f);
    app.git.now_secs = Some(f.last_time() + 3 * 86_400);
    app.open_path(&p).unwrap();
    slash_run(&mut app, "/git blame");
    wait(&mut app, "blame", blame_done);
    let want = porcelain(&f, "b.rs", None);
    let shown = shown_blame(&app);
    assert_eq!(shown.len(), want.len());
    for (i, (oid, author, uncommitted)) in shown.iter().enumerate() {
        let (woid, wauthor) = &want[&(i as u32 + 1)];
        assert_eq!(oid, woid, "line {}", i + 1);
        if *uncommitted {
            assert_eq!(wauthor, "Not Committed Yet");
        } else {
            assert_eq!(author, wauthor, "line {}", i + 1);
        }
    }
    assert!(shown[3].2, "the new line is uncommitted");
    let t = draw(&mut app, 120, 40);
    let s = screen(&t);
    assert!(s.contains("Blame · b.rs"), "{s}");
    assert!(s.contains("Bea Blame"), "{s}");
    assert!(s.contains("You, uncommitted"), "{s}");
    assert!(s.contains("3d"), "relative age\n{s}");
}

#[test]
fn blame_of_unsaved_edits_and_of_the_file_selected_in_changes() {
    let (f, p) = blame_repo();
    f.write("other.txt", "o\n");
    f.git(&["add", "other.txt"]);
    f.git(&["commit", "-q", "-m", "other"]);
    f.write("other.txt", "o\nmore\n");
    let mut app = popup_app(&f);
    app.open_path(&p).unwrap();
    // Unsaved: a line typed at the top of the buffer.
    space_i(&mut app);
    for c in "top\n".chars() {
        let code = if c == '\n' {
            KeyCode::Enter
        } else {
            KeyCode::Char(c)
        };
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }
    slash_run(&mut app, "/git blame");
    wait(&mut app, "blame of the buffer", blame_done);
    let shown = shown_blame(&app);
    assert_eq!(shown.len(), 6);
    assert!(shown[0].2, "the typed line is uncommitted");
    assert!(shown[1..].iter().all(|l| !l.2), "{shown:?}");
    assert_eq!(
        std::fs::read_to_string(&p).unwrap(),
        "a\nB\nc\nd\ne\n",
        "nothing saved"
    );

    // From Changes: the selected file is blamed, not the active one.
    key(&mut app, KeyCode::Char('1'));
    assert_eq!(popup(&app).tab, Tab::Changes);
    crate::git_popup_tests::wait_idle(&mut app);
    assert_eq!(
        popup(&app).selected_entry().map(|e| e.path.clone()),
        Some(PathBuf::from("other.txt"))
    );
    key(&mut app, KeyCode::Char('3'));
    wait(&mut app, "blame of other.txt", |a| {
        blame_done(a) && popup(a).blame.path.as_deref() == Some(&*f.dir.join("other.txt"))
    });
    let shown = shown_blame(&app);
    assert_eq!(shown.len(), 2);
    assert!(!shown[0].2 && shown[1].2, "{shown:?}");
}

// ---- AC-16e: a slow blame never blocks ---------------------------------------------------------

/// A `git` that sleeps `secs` before every blame (the real git does the work).
fn slow_blame_git(f: &Fixture, secs: u32) -> PathBuf {
    let real = std::env::var_os("PATH")
        .and_then(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join("git"))
                .find(|g| g.is_file())
        })
        .expect("git in PATH");
    let dir = f.base.join("shim");
    std::fs::create_dir_all(&dir).unwrap();
    let shim = dir.join("git");
    std::fs::write(
        &shim,
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do [ \"$a\" = blame ] && sleep {secs} && break; done\nexec '{}' \"$@\"\n",
            real.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    dir
}

#[test]
fn a_slow_blame_shows_blaming_and_never_blocks_the_keys() {
    let (f, p) = blame_repo();
    let shim = slow_blame_git(&f, 2);
    let mut opts = options(&f);
    let env = opts.env.as_mut().unwrap();
    for (k, v) in env.iter_mut() {
        if k == "PATH" {
            let mut path = shim.clone().into_os_string();
            path.push(":");
            path.push(&*v);
            *v = path;
        }
    }
    let (mut app, _) = crate::app_tests::test_app(&f.dir);
    app.git.options = opts;
    app.git.debounce = Duration::ZERO;
    app.git.rest = Duration::ZERO;
    app.start_git();
    let root = f.dir.clone();
    wait(&mut app, "repo found", |a| {
        a.git.repo_info_of_root(&root).is_some()
    });
    app.open_path(&p).unwrap();
    let t0 = Instant::now();
    slash_run(&mut app, "/git blame");
    assert!(
        t0.elapsed() < Duration::from_millis(500),
        "the command returns at once"
    );
    wait(&mut app, "blame running", |a| {
        a.git_popup.as_ref().is_some_and(|p| p.blame.running)
    });
    for (w, h) in [(120, 40), (40, 12)] {
        let t = draw(&mut app, w, h);
        assert!(screen(&t).contains("blaming…"), "{}", screen(&t));
    }
    // Keys work while git blames: tabs switch at once.
    let t1 = Instant::now();
    key(&mut app, KeyCode::Char('2'));
    assert_eq!(popup(&app).tab, Tab::History);
    key(&mut app, KeyCode::Char('3'));
    assert_eq!(popup(&app).tab, Tab::Blame);
    assert!(
        t1.elapsed() < Duration::from_millis(200),
        "{:?}",
        t1.elapsed()
    );
    assert!(popup(&app).blame.running, "still blaming");
    wait(&mut app, "blame done", blame_done);
    let t = draw(&mut app, 120, 40);
    assert!(!screen(&t).contains("blaming…"));
    assert!(screen(&t).contains("Cid Commit"), "{}", screen(&t));
}

// ---- Commit view (AC-16c: matches git show) -----------------------------------------------------

/// `+`/`-`/` ` lines of the hunks of `git show` for `oid`.
fn show_lines(f: &Fixture, oid: &str) -> Vec<String> {
    let out = g(f, &f.dir, &["show", "--format=", "--no-color", "-M", oid]);
    let mut v = Vec::new();
    let mut in_hunk = false;
    for l in out.lines() {
        if l.starts_with("diff --git") {
            in_hunk = false;
        } else if l.starts_with("@@") {
            in_hunk = true;
        } else if in_hunk {
            v.push(l.to_string());
        }
    }
    v
}

fn cv_lines(app: &App) -> Vec<String> {
    let c = popup(app).commit.as_ref().unwrap();
    c.rows
        .iter()
        .filter_map(|r| match r {
            CvRow::Line(..) => Some(c.row_text(r)),
            _ => None,
        })
        .collect()
}

fn cv_texts(app: &App, pick: fn(&CvRow) -> bool) -> Vec<String> {
    let c = popup(app).commit.as_ref().unwrap();
    c.rows
        .iter()
        .filter(|r| pick(r))
        .map(|r| c.row_text(r))
        .collect()
}

#[test]
fn the_commit_view_matches_git_show() {
    let f = Fixture::new();
    f.commit("keep.txt", "k\n", "keep");
    f.commit("src/lib.rs", "fn a() {}\nfn b() {}\n", "lib");
    f.write("src/lib.rs", "fn a() {}\nfn c() {}\nfn d() {}\n");
    f.write("new.txt", "fresh\n");
    std::fs::remove_file(f.dir.join("keep.txt")).unwrap();
    f.git(&["add", "-A"]);
    f.git(&[
        "commit",
        "-q",
        "--cleanup=verbatim",
        "-m",
        "Change lib\n\nThe body says why.\nSecond line.",
    ]);
    let oid = g(&f, &f.dir, &["rev-parse", "HEAD"]).trim().to_string();
    let mut app = popup_app(&f);
    slash_run(&mut app, "/git log");
    wait(&mut app, "history", history_loaded);
    assert_eq!(popup(&app).history.selected, 0);
    key(&mut app, KeyCode::Enter);
    wait(&mut app, "commit view", commit_done);
    let c = popup(&app).commit.as_ref().unwrap();
    assert_eq!(c.oid.as_deref(), Some(oid.as_str()));
    assert!(c.error.is_none(), "{:?}", c.error);
    let meta = cv_texts(&app, |r| matches!(r, CvRow::Meta(..)));
    assert!(meta.contains(&format!("commit {oid}")), "{meta:?}");
    assert!(
        meta.contains(&"Author: Ann Author <ann@example.com>".to_string()),
        "{meta:?}"
    );
    let date = Command::new("git")
        .current_dir(&f.dir)
        .env_clear()
        .envs(fixed_env(&f))
        .env("TZ", "UTC")
        .args([
            "show",
            "-s",
            "--date=format-local:%Y-%m-%d %H:%M",
            "--format=%ad",
            &oid,
        ])
        .output()
        .unwrap();
    let date = String::from_utf8(date.stdout).unwrap();
    assert!(
        meta.iter()
            .any(|m| m.starts_with(&format!("Date:   {} UTC", date.trim()))),
        "{meta:?} {date}"
    );
    let message = cv_texts(&app, |r| matches!(r, CvRow::Message(..)));
    let want = g(&f, &f.dir, &["show", "-s", "--format=%B", &oid]);
    assert_eq!(message.join("\n"), want.trim_end());
    let files = cv_texts(&app, |r| matches!(r, CvRow::FileStat(..)));
    let want: Vec<String> = g(
        &f,
        &f.dir,
        &["show", "--format=", "--name-status", "-M", &oid],
    )
    .lines()
    .map(|l| l.replace('\t', " "))
    .collect();
    assert_eq!(files, want);
    assert_eq!(cv_lines(&app), show_lines(&f, &oid));

    // Esc goes back to the list, a second Esc closes the popup.
    key(&mut app, KeyCode::Esc);
    assert!(popup(&app).commit.is_none());
    assert_eq!(popup(&app).tab, Tab::History);
    assert!(app.git_popup_shown());
    key(&mut app, KeyCode::Esc);
    assert!(!app.git_popup_shown());
}

#[test]
fn the_commit_view_colours_the_diff_by_syntax() {
    let f = Fixture::new();
    f.commit("src/lib.rs", "fn a() {}\n", "lib");
    f.commit(
        "src/lib.rs",
        "fn a() {}\npub fn bee() -> u32 { 1 }\n",
        "bee",
    );
    let mut app = popup_app(&f);
    slash_run(&mut app, "/git show HEAD");
    wait(&mut app, "commit view", commit_done);
    let theme = crate::theme::Theme::from_name(&app.settings.theme);
    let keyword = theme.syntax(vr_syntax::Capture::Keyword).fg;
    assert!(keyword.is_some());
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        let t = draw(&mut app, 120, 40);
        let buf = t.backend().buffer();
        let s = screen(&t);
        let row = s.lines().position(|l| l.contains("+pub fn bee")).expect(&s);
        let col = s.lines().nth(row).unwrap().find("+pub").unwrap();
        // Column of `pub` in cells (ASCII line, so bytes = cells after the border).
        let x = s.lines().nth(row).unwrap()[..col].chars().count() as u16 + 1;
        if buf[(x, row as u16)].fg == keyword.unwrap() {
            break;
        }
        assert!(Instant::now() < end, "no syntax colour on `pub`\n{s}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn blame_enter_opens_the_lines_commit() {
    let (f, p) = blame_repo();
    let mut app = popup_app(&f);
    app.open_path(&p).unwrap();
    slash_run(&mut app, "/git blame");
    wait(&mut app, "blame", blame_done);
    // Line 2 ("B") is Bea's.
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Enter);
    wait(&mut app, "commit view", commit_done);
    let bea = g(&f, &f.dir, &["log", "-1", "--format=%H", "--author=Bea"]);
    assert_eq!(
        popup(&app).commit.as_ref().unwrap().oid.as_deref(),
        Some(bea.trim())
    );
    key(&mut app, KeyCode::Esc);
    assert!(popup(&app).commit.is_none() && app.git_popup_shown());
    assert_eq!(popup(&app).tab, Tab::Blame, "back to the blame");
    assert_eq!(popup(&app).blame.cursor, 1);
}

// ---- SEC-5: /git show takes only what rev-parse --verify resolves --------------------------------

#[test]
fn git_show_refuses_unknown_ambiguous_and_option_like_revisions() {
    let f = Fixture::new();
    many_commits(&f, 400);
    let mut app = popup_app(&f);
    app.git.now_secs = Some(1_700_100_000);
    // Unknown.
    slash_run(&mut app, "/git show nope");
    wait(&mut app, "refusal", |a| errors(a).contains("nope"));
    assert!(errors(&app).contains("no such commit"), "{}", notices(&app));
    assert!(popup(&app).commit.is_none());
    // An option is never passed to git as one.
    slash_run(&mut app, "/git show --output=x");
    wait(&mut app, "refusal", |a| errors(a).contains("--output=x"));
    assert!(!f.dir.join("x").exists());
    // A short id shared by two commits.
    let all = g(&f, &f.dir, &["log", "--format=%H"]);
    let mut seen: HashMap<&str, usize> = HashMap::new();
    let mut prefix = None;
    for l in all.lines() {
        let n = seen.entry(&l[..4]).or_default();
        *n += 1;
        if *n == 2 {
            prefix = Some(l[..4].to_string());
            break;
        }
    }
    let prefix = prefix.expect("400 commits share a 4-digit prefix");
    slash_run(&mut app, &format!("/git show {prefix}"));
    wait(&mut app, "ambiguous", |a| errors(a).contains(&prefix));
    assert!(errors(&app).contains("more than one"), "{}", notices(&app));
    // A valid one opens.
    slash_run(&mut app, "/git show HEAD~3");
    wait(&mut app, "commit view", commit_done);
    let want = g(&f, &f.dir, &["rev-parse", "HEAD~3"]);
    assert_eq!(
        popup(&app).commit.as_ref().unwrap().oid.as_deref(),
        Some(want.trim())
    );
}

// ---- SEC-15: hostile authors, messages, names ---------------------------------------------------

#[test]
fn hostile_authors_messages_and_names_are_drawn_safely_in_every_view() {
    let f = Fixture::new();
    let name = format!("evil{HOSTILE}.rs");
    f.write(&name, &format!("line{HOSTILE}\n"));
    f.git(&["add", "-A"]);
    f.git_as(
        &f.dir,
        &format!("Eve{HOSTILE}"),
        &[
            "commit",
            "-q",
            "--cleanup=verbatim",
            "-m",
            &format!("subject{HOSTILE}\n\nbody{HOSTILE}"),
        ],
    );
    let p = f.dir.join(&name);
    let mut app = popup_app(&f);
    app.open_path(&p).unwrap();
    slash_run(&mut app, "/git log file");
    wait(&mut app, "history", history_loaded);
    for (w, h) in [(120, 40), (80, 24), (40, 12)] {
        assert_safe(&draw(&mut app, w, h), "history");
    }
    let s = screen(&draw(&mut app, 120, 40));
    assert!(
        s.contains("subject^[[2J^[]52;c;ZXZpbA==^G<U+009B>31m^M<U+202E>"),
        "{s}"
    );
    key(&mut app, KeyCode::Enter);
    wait(&mut app, "commit view", commit_done);
    for (w, h) in [(120, 40), (80, 24), (40, 12)] {
        assert_safe(&draw(&mut app, w, h), "commit view");
    }
    let s = screen(&draw(&mut app, 120, 40));
    assert!(s.contains("Eve^[[2J"), "{s}");
    assert!(s.contains("body^[[2J"), "{s}");
    assert!(s.contains("+line^[[2J"), "{s}");
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Char('3'));
    wait(&mut app, "blame", blame_done);
    for (w, h) in [(120, 40), (80, 24), (40, 12)] {
        assert_safe(&draw(&mut app, w, h), "blame");
    }
    let s = screen(&draw(&mut app, 120, 40));
    assert!(s.contains("Eve^[[2J"), "{s}");
    assert!(s.contains("line^[[2J"), "{s}");
    for n in &app.notices {
        assert!(!n.text.chars().any(crate::ui::safe_text::is_unsafe_char));
    }
}

// ---- keys and mouse -------------------------------------------------------------------------------

#[test]
fn number_keys_switch_tabs_but_type_in_the_message_box() {
    let (f, p) = blame_repo();
    f.write("b.rs", "changed\n");
    let mut app = popup_app(&f);
    app.open_path(&p).unwrap();
    crate::git_popup_tests::open(&mut app);
    assert_eq!(popup(&app).tab, Tab::Changes);
    key(&mut app, KeyCode::Char('2'));
    assert_eq!(popup(&app).tab, Tab::History);
    wait(&mut app, "history", history_loaded);
    assert!(
        popup(&app).history.path.is_none(),
        "2 shows the repo's history"
    );
    key(&mut app, KeyCode::Char('3'));
    assert_eq!(popup(&app).tab, Tab::Blame);
    key(&mut app, KeyCode::Char('1'));
    assert_eq!(popup(&app).tab, Tab::Changes);
    key(&mut app, KeyCode::Char('c'));
    for c in "fix 123".chars() {
        key(&mut app, KeyCode::Char(c));
    }
    assert_eq!(popup(&app).tab, Tab::Changes);
    assert_eq!(popup(&app).message.text(), "fix 123");
    // The tabs are clickable.
    key(&mut app, KeyCode::Tab);
    let t = draw(&mut app, 120, 40);
    let s = screen(&t);
    let y = popup(&app).areas.popup.y;
    let row = s.lines().nth(usize::from(y)).unwrap();
    let x = row.find("History").expect(row);
    let x = row[..x].chars().count() as u16;
    click(&mut app, x + 1, y);
    assert_eq!(popup(&app).tab, Tab::History, "{s}");
    // The wheel moves the history selection.
    wait(&mut app, "history", history_loaded);
    draw(&mut app, 120, 40);
    let a = popup(&app).areas.history;
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: a.x + 1,
        row: a.y + 1,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(popup(&app).history.selected, 1);
}

// ---- M7-3 QC minor: banner and hint never cut without a marker -----------------------------------

#[test]
fn the_program_banner_and_the_hint_end_visibly_on_small_screens() {
    let f = Fixture::new();
    f.commit("a.txt", "one\n", "a");
    f.write("a.txt", "two\n");
    f.git(&["config", "filter.x.clean", "cat"]);
    let hook = f.dir.join(".git/hooks/pre-push");
    std::fs::write(&hook, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut app = popup_app(&f);
    crate::git_popup_tests::open(&mut app);
    // 80x24: both names shown (the banner wraps to a second row).
    let s = screen(&draw(&mut app, 80, 24));
    assert!(s.contains("filter.x.clean"), "{s}");
    assert!(s.contains("hook pre-push"), "{s}");
    // The hint row is cut with a marker.
    let hint = s
        .lines()
        .find(|l| l.contains("space stage"))
        .expect("hint row");
    assert!(hint.contains('…'), "{hint}");
    // 40x12: the names come first; what does not fit ends with "…".
    let s = screen(&draw(&mut app, 40, 12));
    assert!(s.contains("filter.x"), "{s}");
    let rows: Vec<&str> = s.lines().filter(|l| l.contains('!')).collect();
    assert!(!rows.is_empty(), "{s}");
    let banner_text: String = s
        .lines()
        .skip_while(|l| !l.contains('!'))
        .take(2)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        banner_text.contains("hook pre-push") || banner_text.contains('…'),
        "{banner_text}"
    );
}

// ---- snapshots ----------------------------------------------------------------------------------

fn snapshot_app() -> (Fixture, App) {
    let (f, p) = blame_repo();
    f.write("b.rs", "a\nB\nc\nd\ne\nf\n");
    let mut app = popup_app(&f);
    app.git.now_secs = Some(f.last_time() + 5 * 86_400);
    app.open_path(&p).unwrap();
    (f, app)
}

#[test]
fn render_git_popup_history_blame_and_commit() {
    let (_f, mut app) = snapshot_app();
    slash_run(&mut app, "/git log");
    wait(&mut app, "history", history_loaded);
    wait(&mut app, "scan", |a| {
        popup(a).scan.is_some() && !popup(a).scanning
    });
    app.git_popup.as_mut().unwrap().repo_label = "~/repo".into();
    for (w, h) in [(120, 40), (80, 24), (40, 12)] {
        let t = draw(&mut app, w, h);
        assert_safe(&t, "history");
        insta::assert_snapshot!(format!("render_git_popup_history_{w}x{h}"), t.backend());
    }
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Enter);
    wait(&mut app, "commit view", commit_done);
    for (w, h) in [(120, 40), (80, 24), (40, 12)] {
        let t = draw(&mut app, w, h);
        assert_safe(&t, "commit");
        insta::assert_snapshot!(format!("render_git_popup_commit_{w}x{h}"), t.backend());
    }
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Char('3'));
    wait(&mut app, "blame", blame_done);
    for (w, h) in [(120, 40), (80, 24), (40, 12)] {
        let t = draw(&mut app, w, h);
        assert_safe(&t, "blame");
        insta::assert_snapshot!(format!("render_git_popup_blame_{w}x{h}"), t.backend());
    }
}

// ---- M7-4b review round 1: bounded frames, blame on its own runner, log once ------------------

/// A `git` shim: `body` runs first (sh, `$@` are git's arguments), then the real git.
fn git_shim(f: &Fixture, body: &str) -> PathBuf {
    let real = std::env::var_os("PATH")
        .and_then(|p| {
            std::env::split_paths(&p)
                .map(|d| d.join("git"))
                .find(|g| g.is_file())
        })
        .expect("git in PATH");
    let dir = f.base.join("shim");
    std::fs::create_dir_all(&dir).unwrap();
    let shim = dir.join("git");
    std::fs::write(
        &shim,
        format!("#!/bin/sh\n{body}\nexec '{}' \"$@\"\n", real.display()),
    )
    .unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    dir
}

/// A popup app whose git runs through the shim in `shim`.
fn shim_app(f: &Fixture, shim: &Path) -> App {
    let mut opts = options(f);
    for (k, v) in opts.env.as_mut().unwrap().iter_mut() {
        if k == "PATH" {
            let mut path = shim.to_path_buf().into_os_string();
            path.push(":");
            path.push(&*v);
            *v = path;
        }
    }
    let (mut app, _) = crate::app_tests::test_app(&f.dir);
    app.git.options = opts;
    app.git.debounce = Duration::ZERO;
    app.git.rest = Duration::ZERO;
    app.start_git();
    let root = f.dir.clone();
    wait(&mut app, "repo found", |a| {
        a.git.repo_info_of_root(&root).is_some()
    });
    app
}

fn work(app: &App) -> crate::git_history::Work {
    popup(app).commit.as_ref().unwrap().work
}

/// Bytes of one minified line (under the 512 KiB colour limit, so it is highlighted).
const LONG_LINE: usize = 470 * 1024;

#[test]
fn a_long_diff_line_costs_only_the_visible_cells() {
    let f = Fixture::new();
    f.commit("a.txt", "a\n", "a");
    let line = "var a=1;".repeat(LONG_LINE / 8);
    f.commit("min.js", &format!("{line}\n"), "minified");
    let mut app = popup_app(&f);
    slash_run(&mut app, "/git show HEAD");
    wait(&mut app, "commit view", commit_done);
    {
        let c = app.git_popup.as_mut().unwrap().commit.as_mut().unwrap();
        c.top = c
            .rows
            .iter()
            .position(|r| matches!(r, CvRow::Hunk(..)))
            .unwrap();
    }
    let (w, h) = (200u16, 70u16);
    // A frame styles at most one cell per screen cell and asks the highlighter for at most the
    // bytes those cells hold (4 per char).
    let bound = u64::from(w) * u64::from(h);
    let before = work(&app);
    draw(&mut app, w, h);
    let first = work(&app);
    assert!(first.cells - before.cells <= bound, "{first:?}");
    assert!(first.queried - before.queried <= 4 * bound, "{first:?}");
    // The same frame again measures no row a second time.
    draw(&mut app, w, h);
    assert_eq!(work(&app).measured, first.measured, "row widths are kept");
    for k in [
        KeyCode::Right,
        KeyCode::Right,
        KeyCode::Down,
        KeyCode::End,
        KeyCode::Home,
        KeyCode::Left,
    ] {
        let before = work(&app);
        key(&mut app, k);
        let t = draw(&mut app, w, h);
        let after = work(&app);
        assert!(after.cells - before.cells <= bound, "{k:?} {after:?}");
        assert!(
            after.queried - before.queried <= 4 * bound,
            "{k:?} {after:?}"
        );
        assert!(
            after.measured - before.measured <= u64::from(h),
            "{k:?} {after:?}"
        );
        assert!(after.hl_made <= 1, "{after:?}");
        let _ = t;
    }
    // Scrolled 8 columns: the row starts with "…" and shows the line from column 8 on, cut
    // with "…" at the right edge.
    key(&mut app, KeyCode::Right);
    let s = screen(&draw(&mut app, w, h));
    let row = s
        .lines()
        .find(|l| l.contains(&format!("+…{}", &line[8..60])))
        .unwrap_or_else(|| panic!("{s}"));
    assert!(row.trim_end().trim_end_matches('│').ends_with('…'), "{row}");
}

#[test]
fn many_files_on_screen_keep_their_highlighters() {
    let f = Fixture::new();
    f.commit("keep.txt", "k\n", "keep");
    for i in 0..14 {
        f.write(&format!("src/f{i:02}.rs"), "fn a() {}\n");
    }
    f.git(&["add", "-A"]);
    f.git(&["commit", "-q", "-m", "fourteen files"]);
    let mut app = popup_app(&f);
    slash_run(&mut app, "/git show HEAD");
    wait(&mut app, "commit view", commit_done);
    {
        let c = app.git_popup.as_mut().unwrap().commit.as_mut().unwrap();
        c.top = c
            .rows
            .iter()
            .position(|r| matches!(r, CvRow::FileHead(0)))
            .unwrap();
    }
    let (w, h) = (200u16, 70u16);
    draw(&mut app, w, h);
    let visible = {
        let p = popup(&app);
        let c = p.commit.as_ref().unwrap();
        let mut files: Vec<usize> = c
            .rows
            .iter()
            .skip(c.top)
            .take(usize::from(p.areas.history.height))
            .filter_map(|r| match r {
                CvRow::Line(f, ..) => Some(*f),
                _ => None,
            })
            .collect();
        files.dedup();
        files.len() as u64
    };
    assert!(
        visible > 8,
        "more files on screen than highlighters kept: {visible}"
    );
    assert_eq!(work(&app).hl_made, visible);
    // Their spans land and redraw: no highlighter is made again.
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(20));
        app.tick(Instant::now());
        draw(&mut app, w, h);
    }
    assert_eq!(work(&app).hl_made, visible, "highlighters made again");
}

/// Start lines the shim logged: one pid per blame git started.
fn blame_starts(log: &Path) -> Vec<u32> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.strip_prefix("start ")?.parse().ok())
        .collect()
}

fn alive(pid: u32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
        !s.rsplit(')')
            .next()
            .unwrap_or("")
            .trim_start()
            .starts_with('Z')
    })
}

fn wait_gone(app: &mut App, pid: u32) {
    wait(app, &format!("git {pid} killed"), |_| !alive(pid));
}

#[test]
fn a_newer_blame_kills_the_old_one_and_history_never_waits_for_blame() {
    let f = Fixture::new();
    let slow = f.commit("slow.rs", "s\n", "slow");
    let fast = f.commit("fast.rs", "f\n", "fast");
    let log = f.base.join("blames.log");
    let shim = git_shim(
        &f,
        &format!(
            "for a in \"$@\"; do if [ \"$a\" = blame ]; then echo \"start $$\" >> '{}'; case \"$*\" in *slow.rs*) sleep 30;; esac; break; fi; done",
            log.display()
        ),
    );
    let mut app = shim_app(&f, &shim);
    app.open_path(&slow).unwrap();
    slash_run(&mut app, "/git blame");
    wait(&mut app, "slow blame started", |_| {
        blame_starts(&log).len() == 1
    });
    let first = blame_starts(&log)[0];
    // Another file: its blame answers without waiting for the slow one, which is killed.
    app.open_path(&fast).unwrap();
    slash_run(&mut app, "/git blame");
    wait(&mut app, "fast blame", |a| {
        blame_done(a) && popup(a).blame.path.as_deref() == Some(&*fast)
    });
    assert!(matches!(popup(&app).blame.result, Some(Ok(Some(_)))));
    wait_gone(&mut app, first);
    // A blame runs while History and a commit load.
    app.open_path(&slow).unwrap();
    slash_run(&mut app, "/git blame");
    wait(&mut app, "slow blame again", |_| {
        blame_starts(&log).len() == 3
    });
    let third = blame_starts(&log)[2];
    key(&mut app, KeyCode::Char('2'));
    wait(&mut app, "history", history_loaded);
    assert_eq!(popup(&app).history.commits.len(), 2);
    key(&mut app, KeyCode::Enter);
    wait(&mut app, "commit view", commit_done);
    assert!(alive(third), "the blame is still running");
    assert!(popup(&app).blame.running);
    // Closing the session kills it.
    crate::git_popup::shutdown(&mut app);
    wait_gone(&mut app, third);
}

#[test]
fn the_first_git_log_runs_once() {
    let f = Fixture::new();
    f.commit("a.txt", "1\n", "one");
    f.commit("a.txt", "2\n", "two");
    let mut app = popup_app(&f);
    slash_run(&mut app, "/git log");
    wait(&mut app, "history", history_loaded);
    wait(&mut app, "scan", |a| {
        popup(a).scan.is_some() && !popup(a).scanning
    });
    for _ in 0..20 {
        app.tick(Instant::now());
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(popup(&app).history.asked, 1);
    assert_eq!(popup(&app).history.commits.len(), 2);
}

#[test]
fn a_failed_page_keeps_the_list_and_retries_only_on_r() {
    let f = Fixture::new();
    many_commits(&f, 250);
    let mark = f.base.join("fail-log");
    let shim = git_shim(
        &f,
        &format!(
            "for a in \"$@\"; do if [ \"$a\" = log ] && [ -e '{}' ]; then echo 'fatal: log broke' >&2; exit 128; fi; done",
            mark.display()
        ),
    );
    let mut app = shim_app(&f, &shim);
    slash_run(&mut app, "/git log");
    wait(&mut app, "history", history_loaded);
    assert_eq!(popup(&app).history.commits.len(), crate::git_history::PAGE);
    std::fs::write(&mark, "").unwrap();
    key(&mut app, KeyCode::End);
    wait(&mut app, "page error", |a| {
        history_loaded(a) && popup(a).history.error.is_some()
    });
    let asked = popup(&app).history.asked;
    for k in [
        KeyCode::Up,
        KeyCode::Down,
        KeyCode::End,
        KeyCode::PageUp,
        KeyCode::End,
        KeyCode::Home,
    ] {
        key(&mut app, k);
        app.tick(Instant::now());
    }
    assert_eq!(
        popup(&app).history.asked,
        asked,
        "arrows never rerun a failed log"
    );
    assert_eq!(popup(&app).history.commits.len(), crate::git_history::PAGE);
    let s = screen(&draw(&mut app, 120, 40));
    assert!(s.contains("c250"), "the loaded commits stay: {s}");
    assert!(s.contains("log broke"), "the error shows: {s}");
    std::fs::remove_file(&mark).unwrap();
    key(&mut app, KeyCode::Char('r'));
    wait(&mut app, "reloaded", |a| {
        history_loaded(a) && popup(a).history.commits.len() == 250
    });
    assert!(popup(&app).history.error.is_none());
    assert!(popup(&app).history.complete);
}
