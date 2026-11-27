//! FR-27 (D321): the blame annotation column of the editor — AC-B1..AC-B5. Temp repos come from
//! the git tests' hermetic [`Fixture`].

use std::path::PathBuf;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ropey::Rope;

use crate::app::App;
use crate::app_tests::space_i;
use crate::commands::CmdArgs;
use crate::git::Ann;
use crate::git_tests::{Fixture, draw, row, shim_git, single, single_with, wait, wait_marks};
use crate::ui::editor_view;

/// The fixture's first commit time (T0 + one minute), as the column shows it (UTC).
const DAY0: &str = "2023-11-14";

/// Ann wrote four lines; Bob rewrote the last two.
fn two_authors() -> (Fixture, PathBuf) {
    let f = Fixture::new();
    f.commit("a.txt", "one\ntwo\nthree\nfour\n", "first");
    f.write("a.txt", "one\ntwo\nTHREE\nFOUR\n");
    f.git_as(&f.dir, "Bob Builder-Longname", &["add", "a.txt"]);
    f.git_as(
        &f.dir,
        "Bob Builder-Longname",
        &["commit", "-q", "-m", "second"],
    );
    let p = f.dir.join("a.txt");
    (f, p)
}

fn toggle(app: &mut App) {
    app.run_command("editor.toggle_blame", CmdArgs::None);
}

/// The column of the active document is in step with its text.
fn wait_column(app: &mut App) {
    wait(app, "blame column", crate::git::annotations_ready);
}

fn notices(app: &App) -> Vec<String> {
    app.notices.iter().map(|n| n.text.clone()).collect()
}

fn key(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    app.handle_key(KeyEvent::new(code, mods));
}

fn typ(app: &mut App, s: &str) {
    for c in s.chars() {
        key(app, KeyCode::Char(c), KeyModifiers::NONE);
    }
}

#[test]
fn toggle_shows_author_and_date_in_read_and_edit_mode_and_hides_at_once() {
    let (f, p) = two_authors();
    let mut app = single(&f, &f.dir);
    app.settings.git_inline_blame = true;
    app.open_path(&p).unwrap();
    assert!(!crate::git::annotations_on(&app));
    toggle(&mut app);
    assert!(crate::git::annotations_on(&app));
    wait_column(&mut app);
    // Read mode (a file tab opens in read mode, D145).
    assert!(!app.edit_mode());
    let t = draw(&mut app, 120, 20);
    assert!(
        row(&t, 1).starts_with(&format!("Ann Author   {DAY0}   1 one ")),
        "{}",
        row(&t, 1)
    );
    // The rest of a run shows the thin continuation mark.
    assert!(
        row(&t, 2).starts_with("┊                         2 two"),
        "{}",
        row(&t, 2)
    );
    // A long author name is cut to fit.
    assert!(
        row(&t, 3).starts_with(&format!("Bob Builder… {DAY0}   3 THREE")),
        "{}",
        row(&t, 3)
    );
    assert!(row(&t, 4).starts_with("┊"));
    // The line after the last one has no annotation.
    assert!(
        row(&t, 5).starts_with("                          5 "),
        "{}",
        row(&t, 5)
    );
    // The text is drawn in the theme's blame style.
    let theme = crate::theme::Theme::from_name(&app.settings.theme);
    assert_eq!(t.backend().buffer()[(0, 1)].fg, theme.git_blame.fg.unwrap());

    // Edit mode keeps the column; inline blame and gutter marks still work beside it.
    space_i(&mut app);
    assert!(app.edit_mode());
    typ(&mut app, "X");
    let marks = wait_marks(&mut app);
    assert_eq!(marks.len(), 1, "{marks:?}");
    wait(&mut app, "inline blame", |a| {
        crate::git::inline_text(a).is_some()
    });
    let t = draw(&mut app, 120, 20);
    let r1 = row(&t, 1);
    assert!(r1.starts_with("Not committed yet"), "{r1}");
    assert!(r1.contains("  1▎Xone  You, uncommitted"), "{r1}");

    // Off: gone on the next frame, nothing to wait for.
    toggle(&mut app);
    let t = draw(&mut app, 120, 20);
    assert!(row(&t, 1).starts_with("  1▎Xone"), "{}", row(&t, 1));
}

#[test]
fn the_palette_title_and_default_key_run_the_toggle() {
    let spec = crate::commands::registry::find("editor.toggle_blame").expect("registered");
    assert_eq!(spec.title, "Toggle Blame Annotations");
    assert!(!spec.name.starts_with('/') && !spec.name.starts_with("git"));
    let (f, p) = two_authors();
    let mut app = single(&f, &f.dir);
    app.open_path(&p).unwrap();
    // shift+f2 (the FR-27 default key, USER-5), in read mode and in edit mode.
    key(&mut app, KeyCode::F(2), KeyModifiers::SHIFT);
    assert!(crate::git::annotations_on(&app));
    space_i(&mut app);
    key(&mut app, KeyCode::F(2), KeyModifiers::SHIFT);
    assert!(!crate::git::annotations_on(&app));
    // The palette finds it by its title.
    crate::slash_tests::slash(&mut app, "Toggle Blame");
    crate::slash_tests::key(&mut app, KeyCode::Enter);
    assert!(crate::git::annotations_on(&app));
}

#[test]
fn edits_keep_annotations_on_their_lines_until_the_save_reads_blame_again() {
    let (f, p) = two_authors();
    let mut app = single(&f, &f.dir);
    app.open_path(&p).unwrap();
    toggle(&mut app);
    wait_column(&mut app);
    let asked = app.git.file_blame_requests();
    // A new first line, and a line typed under "two".
    space_i(&mut app);
    typ(&mut app, "new\n");
    key(&mut app, KeyCode::Down, KeyModifiers::NONE);
    key(&mut app, KeyCode::End, KeyModifiers::NONE);
    typ(&mut app, "\nmid");
    for _ in 0..5 {
        app.tick(Instant::now());
    }
    let t = draw(&mut app, 120, 20);
    let rows: Vec<String> = (1..8).map(|y| row(&t, y)).collect();
    assert!(rows[0].starts_with("Not committed yet"), "{rows:#?}");
    assert!(
        rows[1].starts_with(&format!("Ann Author   {DAY0}   2 one")),
        "{rows:#?}"
    );
    assert!(
        rows[2].starts_with("┊                         3 two"),
        "{rows:#?}"
    );
    assert!(rows[3].starts_with("Not committed yet"), "{rows:#?}");
    assert!(rows[4].starts_with("Bob Builder…"), "{rows:#?}");
    assert_eq!(
        app.git.file_blame_requests(),
        asked,
        "edits do not run git blame again"
    );
    // Undo all of it: every line is back on its commit.
    for _ in 0..12 {
        app.run_command("edit.undo", CmdArgs::None);
    }
    assert_eq!(
        app.active_doc().unwrap().buffer.text().to_string(),
        "one\ntwo\nTHREE\nFOUR\n"
    );
    let t = draw(&mut app, 120, 20);
    assert!(row(&t, 1).starts_with("Ann Author"), "{}", row(&t, 1));

    // Save: blame is read again from git.
    typ(&mut app, "top\n");
    app.run_command("file.save", CmdArgs::None);
    wait(&mut app, "blame after save", |a| {
        a.git.file_blame_requests() > asked && crate::git::annotations_ready(a)
    });
    let t = draw(&mut app, 120, 20);
    assert!(
        row(&t, 1).starts_with("Not committed yet"),
        "{}",
        row(&t, 1)
    );
    assert!(row(&t, 2).starts_with("Ann Author"), "{}", row(&t, 2));
}

#[test]
fn outside_a_repo_and_untracked_files_say_so_and_show_nothing() {
    let f = Fixture::bare_dir("plain");
    let p = f.write("a.txt", "x\n");
    let mut app = single(&f, &f.dir);
    wait(&mut app, "discovery", |a| !a.git.discovering());
    app.open_path(&p).unwrap();
    toggle(&mut app);
    app.tick(Instant::now());
    assert!(
        notices(&app)
            .iter()
            .any(|n| n.contains("a.txt") && n.contains("not in a git repository")),
        "{:?}",
        notices(&app)
    );
    let t = draw(&mut app, 120, 20);
    assert!(row(&t, 1).starts_with("  1 x"), "{}", row(&t, 1));

    let (g, _) = two_authors();
    let u = g.write("new.txt", "y\n");
    let mut app = single(&g, &g.dir);
    wait(&mut app, "discovery", |a| !a.git.discovering());
    app.open_path(&u).unwrap();
    toggle(&mut app);
    wait(&mut app, "untracked notice", |a| {
        notices(a)
            .iter()
            .any(|n| n.contains("new.txt") && n.contains("not tracked by git"))
    });
    // The reason replaces the plain "on" notice, so the notice line shows it.
    assert!(
        !notices(&app).iter().any(|n| n == "Blame annotations on"),
        "{:?}",
        notices(&app)
    );
    let t = draw(&mut app, 120, 20);
    assert!(row(&t, 1).starts_with("  1 y"), "{}", row(&t, 1));
}

#[test]
fn a_file_over_the_cap_says_too_large_once() {
    let (f, p) = two_authors();
    let mut app = single(&f, &f.dir);
    app.git.options.max_file = 8;
    app.open_path(&p).unwrap();
    toggle(&mut app);
    for _ in 0..20 {
        app.tick(Instant::now());
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let n = notices(&app)
        .iter()
        .filter(|n| n.contains("too large"))
        .count();
    assert_eq!(n, 1, "{:?}", notices(&app));
    assert!(!crate::git::annotations_ready(&app));
}

#[test]
fn the_column_shrinks_then_hides_as_the_editor_narrows_and_the_mouse_follows_it() {
    let (f, p) = two_authors();
    let mut app = single(&f, &f.dir);
    app.open_path(&p).unwrap();
    toggle(&mut app);
    wait_column(&mut app);
    let g = editor_view::gutter_width(app.active_doc().unwrap().buffer.text());
    assert_eq!(crate::git::blame_width(&app, 120, g), 24);
    assert_eq!(crate::git::blame_width(&app, 80, g), 24);
    // Compact: a short author and the date.
    assert_eq!(crate::git::blame_width(&app, 60, g), 18);
    let t = draw(&mut app, 60, 12);
    assert!(
        row(&t, 1).starts_with(&format!("Ann A… {DAY0}   1 one")),
        "{}",
        row(&t, 1)
    );
    // 40 columns: hidden, the editor keeps its width for code.
    assert_eq!(crate::git::blame_width(&app, 40, g), 0);
    let t = draw(&mut app, 40, 12);
    assert!(row(&t, 1).starts_with("  1 one"), "{}", row(&t, 1));
    // A click lands on the text under the pointer with the column shown.
    draw(&mut app, 120, 20);
    let ed = app.layout.editor;
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: ed.x + 24 + g + 2,
        row: ed.y + 2,
        modifiers: KeyModifiers::NONE,
    });
    let doc = app.active_doc().unwrap();
    let head = doc.buffer.selection().primary().head;
    assert_eq!(head, doc.buffer.text().line_to_char(2) + 2);
}

#[test]
fn the_state_holds_for_every_tab_of_the_session() {
    let (f, p) = two_authors();
    let q = f.commit("b.txt", "bee\n", "third");
    let mut app = single(&f, &f.dir);
    app.open_path(&p).unwrap();
    app.open_path(&q).unwrap();
    toggle(&mut app);
    wait_column(&mut app);
    let t = draw(&mut app, 120, 20);
    assert!(row(&t, 1).starts_with("Ann Author"), "{}", row(&t, 1));
    app.run_command("tab.next", CmdArgs::None);
    assert_eq!(app.active_doc().unwrap().path.as_deref(), Some(p.as_path()));
    wait_column(&mut app);
    let t = draw(&mut app, 120, 20);
    assert!(row(&t, 1).starts_with("Ann Author"), "{}", row(&t, 1));
}

#[test]
fn author_names_from_git_are_sanitized() {
    let f = Fixture::new();
    f.write("a.txt", "x\n");
    f.git(&["add", "a.txt"]);
    f.git_as(
        &f.dir,
        "Eve\u{202e}x\u{1b}]52",
        &["commit", "-q", "-m", "c"],
    );
    let mut app = single(&f, &f.dir);
    app.open_path(&f.dir.join("a.txt")).unwrap();
    toggle(&mut app);
    wait_column(&mut app);
    let t = draw(&mut app, 120, 20);
    let r = row(&t, 1);
    assert!(r.starts_with("Eve<U+202E>…"), "{r}");
    let buf = t.backend().buffer();
    for x in 0..24 {
        let s = buf[(x, 1)].symbol();
        assert!(
            !s.chars().any(crate::ui::safe_text::is_unsafe_char),
            "{s:?}"
        );
    }
}

#[test]
fn remap_keeps_lines_moves_runs_and_marks_new_lines() {
    use Ann::{Blank, Commit, Uncommitted};
    let old = Rope::from_str("a\nb\nc\n");
    let anns = vec![Commit(0), Commit(0), Commit(1), Blank];
    let map = |new: &str| crate::git::remap(&old, &anns, &Rope::from_str(new));
    assert_eq!(map("a\nb\nc\n"), anns);
    // Enter at the end of "a": only the new line is not committed.
    assert_eq!(
        map("a\n\nb\nc\n"),
        [Commit(0), Uncommitted, Commit(0), Commit(1), Blank]
    );
    // Typing inside "b" changes that line only.
    assert_eq!(
        map("a\nbX\nc\n"),
        [Commit(0), Uncommitted, Commit(1), Blank]
    );
    // A deleted line takes its annotation with it.
    assert_eq!(map("a\nc\n"), [Commit(0), Commit(1), Blank]);
    // Two edits far apart (one covering change): the line between keeps its commit.
    assert_eq!(
        map("A\nb\nC\n"),
        [Uncommitted, Commit(0), Uncommitted, Blank]
    );
    // Text without a final line break: the last line is a real one.
    assert_eq!(map("a\nb\nc"), [Commit(0), Commit(0), Uncommitted]);
    assert_eq!(map(""), [Blank]);
}

#[test]
fn utc_dates() {
    assert_eq!(crate::git::utc_date(0), "1970-01-01");
    assert_eq!(crate::git::utc_date(1_700_000_060), DAY0);
    assert_eq!(crate::git::utc_date(951_782_400), "2000-02-29");
    assert_eq!(crate::git::utc_date(-86_400), "1969-12-31");
}

#[test]
fn render_blame_column() {
    let (f, p) = two_authors();
    let mut app = single(&f, &f.dir);
    app.open_path(&p).unwrap();
    toggle(&mut app);
    wait_column(&mut app);
    space_i(&mut app);
    typ(&mut app, "// new\n");
    wait_marks(&mut app);
    // The toggle's notice expires on a real clock: not part of the picture.
    app.notices.clear();
    for (w, h) in [(120, 40), (80, 24), (40, 12)] {
        let t = draw(&mut app, w, h);
        insta::assert_snapshot!(format!("render_blame_column_{w}x{h}"), t.backend());
    }
}

/// FR-27: shift+f2 arrives from a terminal without the kitty protocol (ESC [ 1 ; 2 Q).
#[test]
fn legacy_shift_f2_reaches_the_toggle() {
    let (bytes, ev) = crate::app_tests::legacy_event("shift+f2");
    assert_eq!(bytes, b"\x1b[1;2Q");
    let ev = ev.expect("shift+f2 is a key event");
    let (f, p) = two_authors();
    let mut app = single(&f, &f.dir);
    app.open_path(&p).unwrap();
    app.keyboard_enhanced = false;
    app.handle_key(ev);
    assert!(crate::git::annotations_on(&app));
}

#[test]
fn a_lone_cr_keeps_every_annotation_on_its_line() {
    // git sees "one\rstill one" as one line; the editor shows it as two (D326).
    let f = Fixture::new();
    f.commit("a.txt", "one\rstill one\ntwo\nthree\n", "first");
    f.write("a.txt", "one\rstill one\ntwo\nTHREE\n");
    f.git_as(&f.dir, "Bob Builder-Longname", &["add", "a.txt"]);
    f.git_as(
        &f.dir,
        "Bob Builder-Longname",
        &["commit", "-q", "-m", "second"],
    );
    let p = f.dir.join("a.txt");
    let mut app = single(&f, &f.dir);
    app.open_path(&p).unwrap();
    assert_eq!(app.active_doc().unwrap().buffer.text().len_lines(), 5);
    toggle(&mut app);
    wait_column(&mut app);
    let t = draw(&mut app, 120, 20);
    assert!(
        row(&t, 1).starts_with(&format!("Ann Author   {DAY0}   1 one")),
        "{}",
        row(&t, 1)
    );
    for y in [2, 3] {
        assert!(row(&t, y).starts_with("┊ "), "{}", row(&t, y));
    }
    assert!(
        row(&t, 4).starts_with(&format!("Bob Builder… {DAY0}   4 THREE")),
        "{}",
        row(&t, 4)
    );
    assert!(
        row(&t, 5).starts_with("                          5"),
        "{}",
        row(&t, 5)
    );
}

#[test]
fn a_reply_for_a_closed_tab_is_not_kept() {
    let (f, p) = two_authors();
    let mut app = single(&f, &f.dir);
    app.open_path(&p).unwrap();
    toggle(&mut app);
    wait(&mut app, "blame asked", |a| {
        a.git.file_blame_requests() == 1
    });
    app.run_command("file.close", CmdArgs::None);
    assert!(app.docs.is_empty());
    wait(&mut app, "blame answered", |a| {
        a.git.file_blames_pending() == 0
    });
    assert!(
        !app.git.has_file_blame(&p),
        "a late answer came back for a closed tab"
    );
}

#[test]
fn a_slow_blame_of_another_tab_never_holds_back_this_one() {
    let f = Fixture::new();
    let big = f.commit("big.txt", "b\n", "c1");
    let small = f.commit("small.txt", "s\n", "c2");
    let opts = shim_git(
        &f,
        "case \"$*\" in *blame*big.txt*) sleep 30;; esac\nexec {git} \"$@\"\n",
    );
    let mut app = single_with(&f.dir, opts);
    app.open_path(&big).unwrap();
    toggle(&mut app);
    wait(&mut app, "big asked", |a| a.git.file_blame_requests() == 1);
    let t = Instant::now();
    app.open_path(&small).unwrap();
    wait_column(&mut app);
    space_i(&mut app);
    typ(&mut app, "X");
    let marks = wait_marks(&mut app);
    assert_eq!(marks.len(), 1, "{marks:?}");
    assert!(
        t.elapsed() < std::time::Duration::from_secs(5),
        "this tab waited for the other tab's blame: {:?}",
        t.elapsed()
    );
    // The blame of the tab left behind was dropped, so going back asks again.
    wait(&mut app, "big dropped", |a| {
        a.git.file_blames_pending() == 0
    });
    app.run_command("tab.prev", CmdArgs::None);
    assert_eq!(
        app.active_doc().unwrap().path.as_deref(),
        Some(big.as_path())
    );
    wait(&mut app, "big asked again", |a| {
        a.git.file_blame_requests() == 3
    });
    app.git.shutdown();
}

#[test]
fn a_busy_git_folder_never_keeps_a_slow_blame_from_showing() {
    let f = Fixture::new();
    let big = f.commit("big.txt", "b\n", "c1");
    let opts = shim_git(
        &f,
        "case \"$*\" in *blame*big.txt*) sleep 2;; esac\nexec {git} \"$@\"\n",
    );
    let mut app = single_with(&f.dir, opts);
    app.open_path(&big).unwrap();
    toggle(&mut app);
    wait(&mut app, "big asked", |a| a.git.file_blame_requests() == 1);
    // Something writes under .git faster than the blame runs: each write asks again.
    let touch = f.dir.join(".git").join("vr-busy");
    let end = Instant::now() + std::time::Duration::from_secs(12);
    let mut n = 0;
    while !crate::git::annotations_ready(&app) {
        assert!(Instant::now() < end, "the column never appeared");
        n += 1;
        std::fs::write(&touch, n.to_string()).unwrap();
        for _ in 0..30 {
            app.tick(Instant::now());
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    assert!(
        app.git.file_blame_requests() > 2,
        "the test did not ask again"
    );
    app.git.shutdown();
}
