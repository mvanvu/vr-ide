//! Project search and replace panel through the `App` (AC-11, AC-12).

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use vr_search::{ProjectMatch, SearchMsg};

use crate::app::{App, Focus};
use crate::app_tests::test_app;
use crate::commands::dialog::DialogAction;
use crate::commands::search::{self, Field, Row, Status, Target};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn ctrl_shift(c: char) -> KeyEvent {
    KeyEvent::new(
        KeyCode::Char(c.to_ascii_uppercase()),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    )
}

fn typ(app: &mut App, s: &str) {
    for c in s.chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
}

/// A project with `files` (root-relative path, content) and the app on it.
fn project(files: &[(&str, &str)]) -> (tempfile::TempDir, App) {
    let d = tempfile::tempdir().unwrap();
    for (p, body) in files {
        let path = d.path().join(p);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    let (app, _) = test_app(d.path());
    (d, app)
}

/// Takes search results until the search is done (test helper; the app never blocks).
fn wait_search(app: &mut App) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        app.drain_search();
        if !app.search.running() {
            return;
        }
        assert!(Instant::now() < deadline, "search did not finish");
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Opens the panel, types `query` and runs it to the end.
fn search_for(app: &mut App, query: &str) {
    app.handle_key(ctrl_shift('f'));
    typ(app, query);
    app.handle_key(key(KeyCode::Enter));
    wait_search(app);
}

fn rel(app: &App, p: &Path) -> String {
    app.display_path(p)
}

fn group_counts(app: &App) -> Vec<(String, usize)> {
    let mut v: Vec<(String, usize)> = app
        .search
        .groups
        .iter()
        .map(|g| (rel(app, &g.path), g.matches.len()))
        .collect();
    v.sort();
    v
}

fn draw(app: &mut App, w: u16, h: u16) -> Terminal<TestBackend> {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| crate::ui::render(f, app)).unwrap();
    t
}

/// Text of the search panel in the last frame, rows joined by spaces, runs of spaces
/// collapsed (wrapped messages read as one line).
fn panel_text(app: &App, t: &Terminal<TestBackend>) -> String {
    let a = app.layout.search;
    let buf = t.backend().buffer();
    let mut s = String::new();
    for y in a.y..a.y + a.height {
        // The last column is the separator line.
        for x in a.x..(a.x + a.width).saturating_sub(1) {
            s.push_str(buf[(x, y)].symbol());
        }
        s.push(' ');
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn pm(root: &Path, file: &str, line: usize, text: &str) -> ProjectMatch {
    ProjectMatch {
        path: root.join(file),
        line,
        col: 1,
        line_text: text.to_string(),
        match_range: (0, text.chars().count()),
    }
}

fn notices(app: &App) -> String {
    app.notices
        .iter()
        .map(|n| n.text.clone())
        .collect::<Vec<_>>()
        .join(" | ")
}

#[test]
fn project_search_lists_results_grouped_by_file() {
    let (_d, mut app) = project(&[
        ("src/a.rs", "let needle = 1;\nno\nneedle();\n"),
        ("src/b/c.rs", "// needle\n"),
        ("d.txt", "nothing\n"),
    ]);
    app.handle_key(ctrl_shift('f'));
    assert!(app.search.visible);
    assert_eq!(app.focus, Focus::Search);
    assert_eq!(app.search.field, Field::Query);
    typ(&mut app, "needle");
    app.handle_key(key(KeyCode::Enter));
    wait_search(&mut app);
    assert_eq!(
        group_counts(&app),
        vec![("src/a.rs".to_string(), 2), ("src/b/c.rs".to_string(), 1)]
    );
    assert!(matches!(app.search.status, Status::Done { matches: 3, .. }));
    // Each file row is followed by its matches.
    let rows = app.search.rows();
    assert_eq!(rows.len(), 5);
    assert!(matches!(rows[0], Row::File(_)));
    let t = draw(&mut app, 80, 24);
    let text = panel_text(&app, &t);
    assert!(text.contains("a.rs") && text.contains("c.rs"), "{text}");
    assert!(text.contains("3 results in 2 files"), "{text}");
    assert!(text.contains("needle();"), "{text}");
}

#[test]
fn project_search_streams_batches() {
    let (d, mut app) = project(&[]);
    let root = d.path().to_path_buf();
    app.handle_key(ctrl_shift('f'));
    let (tx, rx) = mpsc::channel();
    search::attach(&mut app, rx, Arc::new(AtomicBool::new(false)));
    assert_eq!(app.search.status, Status::Running);
    tx.send(SearchMsg::Batch(vec![pm(&root, "a.rs", 1, "x")]))
        .unwrap();
    app.drain_search();
    // The first batch is listed before the search is done.
    assert_eq!(group_counts(&app), vec![("a.rs".to_string(), 1)]);
    assert_eq!(app.search.status, Status::Running);
    let t = draw(&mut app, 80, 24);
    assert!(panel_text(&app, &t).contains("a.rs"));
    tx.send(SearchMsg::Batch(vec![
        pm(&root, "b.rs", 2, "y"),
        pm(&root, "a.rs", 3, "z"),
    ]))
    .unwrap();
    app.drain_search();
    assert_eq!(
        group_counts(&app),
        vec![("a.rs".to_string(), 2), ("b.rs".to_string(), 1)]
    );
    tx.send(SearchMsg::Done {
        files: 9,
        matches: 3,
        elapsed: Duration::from_millis(5),
    })
    .unwrap();
    app.drain_search();
    assert!(!app.search.running());
    assert!(matches!(app.search.status, Status::Done { matches: 3, .. }));
}

#[test]
fn enter_on_result_opens_file_at_line_col() {
    let (_d, mut app) = project(&[("a.rs", "x\n  let needle = 1;\n"), ("b.rs", "needle\n")]);
    search_for(&mut app, "needle");
    // Down leaves the query field for the results; walk to the match in a.rs.
    let want = |app: &App, r: Row| match r {
        Row::Match(g, _) => app.search.groups[g].path.ends_with("a.rs"),
        Row::File(_) => false,
    };
    for _ in 0..6 {
        let rows = app.search.rows();
        if app.search.field == Field::Results && want(&app, rows[app.search.selected]) {
            break;
        }
        app.handle_key(key(KeyCode::Down));
    }
    assert_eq!(app.search.field, Field::Results);
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(app.focus, Focus::Editor);
    assert!(app.search.visible, "the panel stays");
    let doc = app.active_doc().unwrap();
    assert!(doc.path.as_ref().unwrap().ends_with("a.rs"));
    let r = doc.buffer.selection().primary();
    // Line 2, column 7: the match is selected.
    assert_eq!((r.from(), r.to()), (2 + 6, 2 + 6 + 6));
    // f4 / shift+f4 walk the results from the editor, wrapping.
    app.handle_key(key(KeyCode::F(4)));
    let name = |app: &App| app.active_name();
    let other = name(&app);
    assert_ne!(other, "a.rs");
    assert_eq!(
        app.active_doc()
            .unwrap()
            .buffer
            .selection()
            .primary()
            .from(),
        0
    );
    app.handle_key(key(KeyCode::F(4)));
    assert_eq!(name(&app), "a.rs");
    app.handle_key(KeyEvent::new(KeyCode::F(4), KeyModifiers::SHIFT));
    assert_eq!(name(&app), other);
}

#[test]
fn search_panel_cancel_on_new_query() {
    // Through the channel: a new search drops what the old one still sends.
    let (d, mut app) = project(&[]);
    let root = d.path().to_path_buf();
    app.handle_key(ctrl_shift('f'));
    let (tx1, rx1) = mpsc::channel();
    let cancel1 = Arc::new(AtomicBool::new(false));
    search::attach(&mut app, rx1, cancel1.clone());
    let (tx2, rx2) = mpsc::channel();
    search::attach(&mut app, rx2, Arc::new(AtomicBool::new(false)));
    assert!(
        cancel1.load(Ordering::SeqCst),
        "the old search is cancelled"
    );
    let _ = tx1.send(SearchMsg::Batch(vec![pm(&root, "old.rs", 1, "old")]));
    tx2.send(SearchMsg::Batch(vec![pm(&root, "new.rs", 1, "new")]))
        .unwrap();
    app.drain_search();
    assert_eq!(group_counts(&app), vec![("new.rs".to_string(), 1)]);

    // For real: Enter on a new query cancels the running walk.
    let files: Vec<(String, String)> = (0..400)
        .map(|i| {
            let body = if i % 2 == 0 { "aaa\n" } else { "bbb\n" };
            (format!("f{i:03}.txt"), body.to_string())
        })
        .collect();
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let (_d, mut app) = project(&refs);
    app.handle_key(ctrl_shift('f'));
    typ(&mut app, "aaa");
    app.handle_key(key(KeyCode::Enter));
    let first = app.search.run.as_ref().unwrap().cancel.clone();
    for _ in 0..3 {
        app.handle_key(key(KeyCode::Backspace));
    }
    typ(&mut app, "bbb");
    app.handle_key(key(KeyCode::Enter));
    assert!(first.load(Ordering::SeqCst));
    wait_search(&mut app);
    let lines: Vec<&str> = app
        .search
        .groups
        .iter()
        .flat_map(|g| g.matches.iter().map(|m| m.line_text.as_str()))
        .collect();
    assert_eq!(lines.len(), 200);
    assert!(
        lines.iter().all(|l| *l == "bbb"),
        "results of the old query"
    );
}

#[test]
fn search_errors_are_shown() {
    let (_d, mut app) = project(&[]);
    app.handle_key(ctrl_shift('f'));
    let (tx, rx) = mpsc::channel();
    search::attach(&mut app, rx, Arc::new(AtomicBool::new(false)));
    for e in [
        "1 file larger than 32 MB not searched",
        "more than 100000 matches; search stopped",
    ] {
        tx.send(SearchMsg::Error(e.to_string())).unwrap();
    }
    tx.send(SearchMsg::Done {
        files: 1,
        matches: 100_000,
        elapsed: Duration::ZERO,
    })
    .unwrap();
    app.drain_search();
    assert_eq!(app.search.errors.len(), 2);
    let n = notices(&app);
    assert!(n.contains("larger than 32 MB not searched"), "{n}");
    assert!(n.contains("search stopped"), "{n}");
    app.notices.clear();
    let t = draw(&mut app, 80, 24);
    let text = panel_text(&app, &t);
    assert!(text.contains("larger than 32 MB not searched"), "{text}");
    assert!(text.contains("search stopped"), "{text}");

    // A real file over 32 MB.
    let (d, mut app) = project(&[("small.txt", "needle\n")]);
    let mut big = vec![b'x'; 33 * 1024 * 1024];
    big[..7].copy_from_slice(b"needle\n");
    std::fs::write(d.path().join("big.txt"), big).unwrap();
    search_for(&mut app, "needle");
    assert_eq!(group_counts(&app), vec![("small.txt".to_string(), 1)]);
    assert!(
        app.search.errors.iter().any(|e| e.contains("32 MB")),
        "{:?}",
        app.search.errors
    );
}

/// Opens the replace panel, searches `query`, types `replacement` and asks for the
/// preview (Enter in the replace field); waits for it.
fn preview(app: &mut App, query: &str, replacement: &str) {
    app.handle_key(ctrl_shift('h'));
    assert!(app.search.visible && app.search.replace_mode);
    typ(app, query);
    app.handle_key(key(KeyCode::Tab));
    assert_eq!(app.search.field, Field::Replace);
    typ(app, replacement);
    app.handle_key(key(KeyCode::Enter));
    wait_search(app);
    assert!(app.search.preview.is_some(), "no preview: {}", notices(app));
}

fn preview_index(app: &App, name: &str) -> usize {
    app.search
        .preview
        .as_ref()
        .unwrap()
        .files
        .iter()
        .position(|f| f.path.ends_with(name))
        .unwrap_or_else(|| panic!("{name} not in the preview"))
}

/// Enter on the preview, then Replace in the confirm dialog (Cancel is the default).
fn confirm(app: &mut App) {
    app.handle_key(key(KeyCode::Enter));
    let d = app.dialog.as_ref().expect("confirm dialog");
    assert!(matches!(d.action, DialogAction::ReplaceInFiles));
    let crate::ui::dialog::DialogWidget::Confirm(c) = &d.widget else {
        panic!("confirm expected")
    };
    assert_eq!(c.buttons[c.selected], "Cancel", "Cancel is the default");
    app.handle_key(key(KeyCode::Left));
    app.handle_key(key(KeyCode::Enter));
    assert!(app.dialog.is_none());
}

fn result(app: &App, name: &str) -> Option<Result<usize, String>> {
    let i = preview_index(app, name);
    app.search.preview.as_ref().unwrap().files[i].result.clone()
}

fn read(d: &tempfile::TempDir, name: &str) -> String {
    std::fs::read_to_string(d.path().join(name)).unwrap()
}

#[test]
fn project_replace_preview_and_confirm() {
    let (d, mut app) = project(&[
        ("a.txt", "foo 1\nfoo 2\n"),
        ("b.txt", "foo b\n"),
        ("c.txt", "foo c\n"),
    ]);
    app.open_path(Path::new("c.txt")).unwrap();
    preview(&mut app, "foo", "bar");
    let p = app.search.preview.as_ref().unwrap();
    assert_eq!(p.files.len(), 3);
    assert!(p.files.iter().all(|f| f.checked && f.error.is_none()));
    let a = &p.files[preview_index(&app, "a.txt")];
    assert_eq!(a.count, 2);
    assert_eq!(a.hunks[0].before, "foo 1");
    assert_eq!(a.hunks[0].after, "bar 1");
    assert!(matches!(a.target, Target::Disk { .. }));
    let c = &p.files[preview_index(&app, "c.txt")];
    assert!(matches!(c.target, Target::Buffer { .. }), "open file");
    // Nothing is written by the preview.
    assert_eq!(read(&d, "a.txt"), "foo 1\nfoo 2\n");
    let t = draw(&mut app, 80, 24);
    let text = panel_text(&app, &t);
    assert!(text.contains("bar 1"), "{text}");

    // Uncheck b.txt with Space on its row.
    let b = preview_index(&app, "b.txt");
    for _ in 0..20 {
        let p = app.search.preview.as_ref().unwrap();
        if p.rows()[p.selected] == search::PreviewRow::File(b) {
            break;
        }
        app.handle_key(key(KeyCode::Down));
    }
    app.handle_key(key(KeyCode::Char(' ')));
    assert!(!app.search.preview.as_ref().unwrap().files[b].checked);
    confirm(&mut app);

    assert_eq!(read(&d, "a.txt"), "bar 1\nbar 2\n");
    assert_eq!(read(&d, "b.txt"), "foo b\n", "unchecked file untouched");
    assert_eq!(
        read(&d, "c.txt"),
        "foo c\n",
        "open file not written behind its tab"
    );
    assert_eq!(result(&app, "a.txt"), Some(Ok(2)));
    assert_eq!(result(&app, "b.txt"), None);
    assert_eq!(result(&app, "c.txt"), Some(Ok(1)));
    let ci = app
        .docs
        .iter()
        .position(|doc| doc.path.as_ref().unwrap().ends_with("c.txt"))
        .unwrap();
    assert_eq!(app.docs[ci].buffer.text().to_string(), "bar c\n");
    assert!(app.docs[ci].is_modified());
    assert!(notices(&app).contains("Replaced 3"), "{}", notices(&app));
    // The open file's change is one undo step.
    app.set_active(ci);
    app.focus = Focus::Editor;
    app.handle_key(ctrl('z'));
    assert_eq!(app.docs[ci].buffer.text().to_string(), "foo c\n");
    assert!(!app.docs[ci].buffer.history().can_undo());
}

#[test]
fn project_replace_skips_file_changed_since_preview() {
    let (d, mut app) = project(&[("a.txt", "foo\n"), ("b.txt", "foo\n")]);
    preview(&mut app, "foo", "bar");
    std::fs::write(d.path().join("a.txt"), "agent foo\n").unwrap();
    confirm(&mut app);
    assert_eq!(read(&d, "a.txt"), "agent foo\n");
    assert_eq!(read(&d, "b.txt"), "bar\n");
    let r = result(&app, "a.txt").unwrap().unwrap_err();
    assert!(r.contains("changed"), "{r}");
    let n = notices(&app);
    assert!(n.contains("a.txt") && n.contains("changed"), "{n}");
    let t = draw(&mut app, 80, 24);
    assert!(panel_text(&app, &t).contains("changed"));
}

#[test]
fn project_replace_never_writes_behind_a_tab() {
    let (d, mut app) = project(&[
        ("a.txt", "foo a\n"),
        ("b.txt", "foo b\n"),
        ("c.txt", "foo c\n"),
    ]);
    app.open_path(Path::new("b.txt")).unwrap();
    app.open_path(Path::new("c.txt")).unwrap();
    preview(&mut app, "foo", "bar");
    // a.txt is opened after the preview; b.txt is edited; c.txt is closed.
    app.open_path(Path::new("a.txt")).unwrap();
    let bi = app
        .docs
        .iter()
        .position(|doc| doc.path.as_ref().unwrap().ends_with("b.txt"))
        .unwrap();
    app.set_active(bi);
    app.focus = Focus::Editor;
    typ(&mut app, "mine ");
    let ci = app
        .docs
        .iter()
        .position(|doc| doc.path.as_ref().unwrap().ends_with("c.txt"))
        .unwrap();
    app.close_doc(ci);
    app.focus = Focus::Search;
    app.search.field = Field::Results;
    confirm(&mut app);
    for name in ["a.txt", "b.txt", "c.txt"] {
        assert!(
            read(&d, name).starts_with("foo"),
            "{name} written: {}",
            read(&d, name)
        );
        let r = result(&app, name).unwrap();
        assert!(r.is_err(), "{name}: {r:?}");
    }
    assert!(
        result(&app, "a.txt")
            .unwrap()
            .unwrap_err()
            .contains("opened")
    );
    assert!(
        result(&app, "b.txt")
            .unwrap()
            .unwrap_err()
            .contains("edited")
    );
    assert!(
        result(&app, "c.txt")
            .unwrap()
            .unwrap_err()
            .contains("closed")
    );
    let b = app
        .docs
        .iter()
        .find(|doc| doc.path.as_ref().unwrap().ends_with("b.txt"))
        .unwrap();
    assert_eq!(b.buffer.text().to_string(), "mine foo b\n");
    let a = app
        .docs
        .iter()
        .find(|doc| doc.path.as_ref().unwrap().ends_with("a.txt"))
        .unwrap();
    assert_eq!(a.buffer.text().to_string(), "foo a\n");
}

#[test]
fn project_replace_routes_a_symlinked_tab_to_its_buffer() {
    let (d, mut app) = project(&[("real.txt", "foo\n")]);
    std::os::unix::fs::symlink(d.path().join("real.txt"), d.path().join("link.txt")).unwrap();
    app.open_path(Path::new("link.txt")).unwrap();
    preview(&mut app, "foo", "bar");
    let i = preview_index(&app, "real.txt");
    assert!(matches!(
        app.search.preview.as_ref().unwrap().files[i].target,
        Target::Buffer { .. }
    ));
    confirm(&mut app);
    assert_eq!(
        read(&d, "real.txt"),
        "foo\n",
        "written behind the link's tab"
    );
    assert_eq!(app.docs[0].buffer.text().to_string(), "bar\n");
}

/// AC-12 data safety: a writer rewriting a confirmed file while the replace runs never
/// loses a version (strict `_swapping` rule, D96/D99).
#[test]
fn project_replace_never_loses_a_concurrent_write() {
    use vr_core::testutil::{
        RacingWriter, WriterMode, assert_exchange_proofs_known, assert_no_lost_write_swapping,
        sweep_offsets, timed,
    };
    const INITIAL: &[u8] = b"foo\n";
    const OURS: &[u8] = b"bar\n";
    for mode in [WriterMode::InPlace, WriterMode::TempRename] {
        let (mut ok_count, mut refused) = (0, 0);
        for offset in sweep_offsets(40, Duration::from_micros(300)) {
            let (d, mut app) = project(&[("a.txt", "foo\n")]);
            let path = d.path().join("a.txt");
            preview(&mut app, "foo", "bar");
            app.handle_key(key(KeyCode::Enter));
            app.handle_key(key(KeyCode::Left));
            let writer =
                RacingWriter::start_after(offset, &path, mode, Duration::from_micros(300), 6);
            let (_, run) = timed(
                || {
                    app.handle_key(key(KeyCode::Enter));
                    result(&app, "a.txt")
                },
                |r| matches!(r, Some(Ok(_))),
            );
            let versions = writer.stop();
            assert_exchange_proofs_known(d.path(), &versions);
            let final_bytes = std::fs::read(&path).unwrap();
            let kept: Vec<Vec<u8>> = std::fs::read_dir(d.path())
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| p.to_string_lossy().contains(".vr-ide-conflict-"))
                .map(|p| std::fs::read(p).unwrap())
                .collect();
            assert_no_lost_write_swapping(INITIAL, &final_bytes, OURS, &versions, run, &kept);
            if run.ok {
                ok_count += 1;
            } else {
                refused += 1;
                let why = result(&app, "a.txt").unwrap().unwrap_err();
                assert!(!why.is_empty());
                assert!(notices(&app).contains("a.txt"), "{}", notices(&app));
            }
        }
        eprintln!("project_replace {mode:?}: ok={ok_count} refused={refused}");
        assert!(
            ok_count > 0 && refused > 0,
            "{mode:?}: both outcomes must occur (ok={ok_count} refused={refused})"
        );
    }
}

#[test]
fn search_panel_takes_the_side_column() {
    let (_d, mut app) = project(&[("a.txt", "x\n")]);
    app.explorer.visible = true;
    app.handle_key(ctrl_shift('f'));
    draw(&mut app, 80, 24);
    assert!(app.layout.search.width > 0);
    assert_eq!(app.layout.explorer.width, 0);
    assert_eq!(app.layout.editor.x, app.layout.search.width);
    // Esc gives the keys back to the editor; the panel stays.
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.focus, Focus::Editor);
    assert!(app.search.visible);
    // Explorer: Focus brings the explorer back in the column.
    app.handle_key(ctrl_shift('e'));
    assert!(!app.search.visible);
    assert_eq!(app.focus, Focus::Explorer);
    draw(&mut app, 80, 24);
    assert!(app.layout.explorer.width > 0);
    // f6 cycles editor -> side panel -> editor with the search panel too.
    app.handle_key(ctrl_shift('f'));
    app.handle_key(key(KeyCode::Esc));
    app.handle_key(key(KeyCode::F(6)));
    assert_eq!(app.focus, Focus::Search);
}
