//! Project search and replace panel through the `App` (AC-11, AC-12).

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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

/// Takes search results until the search and the preview / replace worker are done (test
/// helper; the app never blocks).
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

/// Enter on the preview, then Replace in the confirm dialog (Cancel is the default); waits
/// for the replace.
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
    wait_search(app);
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
    app.handle_key(ctrl('i'));
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
    app.handle_key(ctrl('i'));
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
                    wait_search(&mut app);
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

/// D123: a preview is for the query, replacement and options it was built with; changing
/// any of them drops it, and Enter on the list then writes nothing.
#[test]
fn project_replace_preview_dropped_when_fields_change() {
    type Edit = (&'static str, fn(&mut App));
    let edits: [Edit; 4] = [
        ("replace field", |app| {
            app.search.field = Field::Replace;
            typ(app, "z");
        }),
        ("query field", |app| {
            app.search.field = Field::Query;
            app.handle_key(key(KeyCode::Backspace));
        }),
        ("paste", |app| {
            app.search.field = Field::Replace;
            app.handle_paste("zz");
        }),
        ("option", |app| {
            app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::ALT));
        }),
    ];
    for (what, edit) in edits {
        let (d, mut app) = project(&[("a.txt", "foo\n"), ("b.txt", "foo\n")]);
        preview(&mut app, "foo", "bar");
        edit(&mut app);
        assert!(app.search.preview.is_none(), "{what}: preview kept");
        app.search.field = Field::Results;
        app.handle_key(key(KeyCode::Enter));
        assert!(
            app.dialog.is_none(),
            "{what}: confirm without a current preview"
        );
        wait_search(&mut app);
        assert_eq!(read(&d, "a.txt"), "foo\n", "{what}");
        assert_eq!(read(&d, "b.txt"), "foo\n", "{what}");
    }
}

/// The confirm itself refuses a preview the fields no longer match, however they changed.
#[test]
fn project_replace_confirm_refuses_a_stale_preview() {
    let (d, mut app) = project(&[("a.txt", "foo\n")]);
    preview(&mut app, "foo", "bar");
    app.search.replace.set("baz");
    app.handle_key(key(KeyCode::Enter));
    assert!(app.dialog.is_none(), "confirm of a stale preview");
    assert!(app.search.preview.is_none());
    assert_eq!(read(&d, "a.txt"), "foo\n");
}

/// Below 40 columns the panel takes the whole width; clicking a result opens it, hides
/// the panel and gives the editor the keys (never a hidden panel).
#[test]
fn narrow_click_on_result_gives_keys_to_the_editor() {
    use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
    let (_d, mut app) = project(&[("a.txt", "needle\n")]);
    search_for(&mut app, "needle");
    draw(&mut app, 36, 12);
    assert_eq!(app.layout.editor.width, 0, "panel takes the whole width");
    let list = app.search.list_area;
    let match_row = app
        .search
        .rows()
        .iter()
        .position(|r| matches!(r, Row::Match(..)))
        .unwrap();
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: list.x + 2,
        row: list.y + match_row as u16,
        modifiers: KeyModifiers::NONE,
    });
    assert!(!app.search.visible);
    assert_eq!(app.focus, Focus::Editor);
    app.handle_key(key(KeyCode::End));
    app.handle_key(ctrl('i'));
    typ(&mut app, "X");
    assert!(
        app.active_doc()
            .unwrap()
            .buffer
            .text()
            .to_string()
            .contains('X'),
        "typing reaches the editor"
    );
}

/// A preview asked for while the search runs is dropped by Esc and by a new search: it
/// never appears unasked when that search ends.
#[test]
fn pending_preview_cleared_by_esc_and_new_search() {
    for how in ["esc", "new search"] {
        let (d, mut app) = project(&[]);
        let root = d.path().to_path_buf();
        app.handle_key(ctrl_shift('h'));
        typ(&mut app, "foo");
        app.handle_key(key(KeyCode::Tab));
        typ(&mut app, "bar");
        let (tx, rx) = mpsc::channel();
        search::attach(&mut app, rx, Arc::new(AtomicBool::new(false)));
        app.search.query = Some(app.search.query());
        app.handle_key(key(KeyCode::Enter));
        assert!(app.search.preview_pending, "{how}");
        match how {
            "esc" => app.handle_key(key(KeyCode::Esc)),
            _ => {
                app.search.field = Field::Query;
                app.handle_key(key(KeyCode::Enter));
            }
        }
        assert!(!app.search.preview_pending, "{how}: still pending");
        if how == "esc" {
            tx.send(SearchMsg::Batch(vec![pm(&root, "a.txt", 1, "foo")]))
                .unwrap();
            tx.send(SearchMsg::Done {
                files: 1,
                matches: 1,
                elapsed: Duration::ZERO,
            })
            .unwrap();
        }
        wait_search(&mut app);
        assert!(app.search.preview.is_none(), "{how}: preview unasked");
    }
}

/// A search that stopped at the match limit: the confirm says the replace covers only part
/// of the project.
#[test]
fn confirm_names_a_truncated_search() {
    let (d, mut app) = project(&[("a.txt", "foo\n")]);
    let root = d.path().to_path_buf();
    app.handle_key(ctrl_shift('h'));
    typ(&mut app, "foo");
    app.handle_key(key(KeyCode::Tab));
    typ(&mut app, "bar");
    let (tx, rx) = mpsc::channel();
    search::attach(&mut app, rx, Arc::new(AtomicBool::new(false)));
    app.search.query = Some(app.search.query());
    tx.send(SearchMsg::Batch(vec![pm(&root, "a.txt", 1, "foo")]))
        .unwrap();
    tx.send(SearchMsg::Error(
        "more than 100000 matches; search stopped".into(),
    ))
    .unwrap();
    tx.send(SearchMsg::Done {
        files: 1,
        matches: 100_000,
        elapsed: Duration::ZERO,
    })
    .unwrap();
    wait_search(&mut app);
    app.handle_key(key(KeyCode::Enter));
    wait_search(&mut app);
    assert!(app.search.preview.is_some(), "{}", notices(&app));
    app.handle_key(key(KeyCode::Enter));
    let d = app.dialog.as_ref().expect("confirm dialog");
    let crate::ui::dialog::DialogWidget::Confirm(c) = &d.widget else {
        panic!("confirm expected")
    };
    assert!(
        c.message.contains("only part of the project"),
        "{}",
        c.message
    );
}

/// Waits for the search only (a held worker keeps the preview / replace running).
fn wait_search_only(app: &mut App) {
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

fn many_files(n: usize) -> Vec<(String, String)> {
    (0..n)
        .map(|i| (format!("f{i:03}.txt"), "foo\n".to_string()))
        .collect()
}

/// D123: the preview is built on a worker: Enter returns before any result file is read,
/// the panel shows the progress, and Esc cancels it.
#[test]
fn project_replace_preview_runs_on_a_worker_with_progress_and_cancel() {
    let files = many_files(80);
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    for cancel in [false, true] {
        let (_d, mut app) = project(&refs);
        let hold = Arc::new(AtomicUsize::new(0));
        app.search.hold = Some(hold.clone());
        app.handle_key(ctrl_shift('h'));
        typ(&mut app, "foo");
        app.handle_key(key(KeyCode::Tab));
        typ(&mut app, "bar");
        app.handle_key(key(KeyCode::Enter));
        wait_search_only(&mut app);
        let p = app
            .search
            .preview
            .as_ref()
            .expect("preview shown while it is built");
        assert!(p.building(), "the UI thread planned the files itself");
        assert!(p.files.iter().all(|f| f.count == 0 && f.hunks.is_empty()));
        let t = draw(&mut app, 80, 24);
        let text = panel_text(&app, &t);
        assert!(text.contains("Preparing preview 0/80"), "{text}");
        // Enter cannot confirm a preview that is not built.
        app.handle_key(key(KeyCode::Enter));
        assert!(app.dialog.is_none());
        if cancel {
            app.handle_key(key(KeyCode::Esc));
            assert!(app.search.preview.is_none(), "Esc cancels the preview");
            hold.store(usize::MAX, Ordering::SeqCst);
            wait_search(&mut app);
            assert!(app.search.preview.is_none());
        } else {
            hold.store(usize::MAX, Ordering::SeqCst);
            wait_search(&mut app);
            let p = app.search.preview.as_ref().unwrap();
            assert!(!p.building());
            assert_eq!(p.files.len(), 80);
            assert!(
                p.files.iter().all(|f| f.count == 1 && f.checked),
                "{:?}",
                p.files[0]
            );
        }
    }
}

/// D123: the replace runs on a worker with progress; Esc cancels the files not yet written
/// and each of them says so.
#[test]
fn project_replace_apply_runs_on_a_worker_with_progress_and_cancel() {
    let files = many_files(80);
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let (d, mut app) = project(&refs);
    preview(&mut app, "foo", "bar");
    wait_search(&mut app);
    let hold = Arc::new(AtomicUsize::new(0));
    app.search.hold = Some(hold.clone());
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Left));
    app.handle_key(key(KeyCode::Enter));
    assert!(app.dialog.is_none());
    let p = app.search.preview.as_ref().unwrap();
    assert!(p.applying(), "the replace runs on a worker");
    let t = draw(&mut app, 80, 24);
    let text = panel_text(&app, &t);
    assert!(text.contains("Replacing 0/80"), "{text}");
    // Editing the fields while it runs keeps the preview: it shows this replace's results.
    app.search.field = Field::Replace;
    typ(&mut app, "z");
    assert!(app.search.preview.is_some());
    app.handle_key(key(KeyCode::Esc));
    hold.store(usize::MAX, Ordering::SeqCst);
    wait_search(&mut app);
    let p = app.search.preview.as_ref().unwrap();
    assert!(p.applied);
    for (name, _) in &files {
        assert_eq!(read(&d, name), "foo\n", "{name} written after cancel");
        let r = result(&app, name).unwrap().unwrap_err();
        assert!(r.contains("cancelled"), "{name}: {r}");
    }
    assert!(
        notices(&app).contains("80 files not replaced"),
        "{}",
        notices(&app)
    );
}

/// A previewed replace of `n` files ("foo" → "bar") confirmed with a held worker that may
/// pass `permits` batches of 32 files.
fn held_apply(n: usize, permits: usize) -> (tempfile::TempDir, App, Arc<AtomicUsize>, Vec<String>) {
    let files = many_files(n);
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let (d, mut app) = project(&refs);
    preview(&mut app, "foo", "bar");
    let hold = Arc::new(AtomicUsize::new(permits));
    app.search.hold = Some(hold.clone());
    app.handle_key(key(KeyCode::Enter));
    app.handle_key(key(KeyCode::Left));
    app.handle_key(key(KeyCode::Enter));
    assert!(app.search.preview.as_ref().unwrap().applying());
    (
        d,
        app,
        hold,
        files.into_iter().map(|(name, _)| name).collect(),
    )
}

/// Takes the worker's messages until it reports `done` files.
fn wait_done(app: &mut App, done: usize) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while app
        .search
        .preview
        .as_ref()
        .and_then(|p| p.job.as_ref())
        .map(|j| j.done)
        != Some(done)
    {
        assert!(Instant::now() < deadline, "worker did not reach {done}");
        app.drain_search();
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Every file has exactly one result, and the disk agrees with it: replaced files hold
/// "bar", the others still "foo". Returns (replaced, cancelled).
fn check_results(app: &App, d: &tempfile::TempDir, names: &[String]) -> (usize, usize) {
    let (mut ok, mut cancelled) = (0, 0);
    for name in names {
        match result(app, name) {
            Some(Ok(1)) => {
                assert_eq!(read(d, name), "bar\n", "{name}");
                ok += 1;
            }
            Some(Err(e)) => {
                assert!(e.contains("cancelled"), "{name}: {e}");
                assert_eq!(read(d, name), "foo\n", "{name}");
                cancelled += 1;
            }
            r => panic!("{name}: {r:?}"),
        }
    }
    assert_eq!(ok + cancelled, names.len());
    (ok, cancelled)
}

/// Quitting never kills a running replace: it is refused with a notice (no quit dialog
/// either); once the replace is over, quit works.
#[test]
fn quit_is_refused_while_a_replace_runs() {
    let (d, mut app, hold, names) = held_apply(40, 0);
    app.open_path(Path::new("f000.txt")).unwrap();
    app.focus = Focus::Editor;
    app.handle_key(ctrl('i'));
    app.handle_key(key(KeyCode::Char('x')));
    app.run_command("app.quit", crate::commands::CmdArgs::None);
    assert!(!app.quit, "quit during a replace");
    assert!(app.dialog.is_none(), "no quit dialog during a replace");
    assert!(
        notices(&app).contains("a replace is running"),
        "{}",
        notices(&app)
    );
    hold.store(usize::MAX, Ordering::SeqCst);
    wait_search(&mut app);
    // f000.txt was opened after the preview: it is skipped with that reason.
    let others: Vec<String> = names.iter().filter(|n| *n != "f000.txt").cloned().collect();
    assert_eq!(check_results(&app, &d, &others), (39, 0));
    let idx = app.docs.iter().position(|doc| doc.is_modified()).unwrap();
    app.close_doc(idx);
    app.run_command("app.quit", crate::commands::CmdArgs::None);
    assert!(app.quit, "quit after the replace");
}

/// The exit path (normal or after a terminal error) cancels a running replace, lets the
/// batch in hand finish and waits (bounded) for the worker to end: every file has its
/// result and none is half-written.
#[test]
fn shutdown_joins_a_running_replace() {
    let (d, mut app, _hold, names) = held_apply(80, 1);
    wait_done(&mut app, 32);
    search::shutdown(&mut app);
    assert!(!app.search.busy(), "worker still attached");
    assert_eq!(check_results(&app, &d, &names), (32, 48));
    let tmp = std::fs::read_dir(d.path())
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp")
        })
        .count();
    assert_eq!(tmp, 0, "temp file left behind");
}

/// Esc after some batches were written: the rest is cancelled, every file is reported
/// once, and the warning names a few files, not all of them.
#[test]
fn partial_apply_cancel_reports_every_file_once() {
    let (d, mut app, hold, names) = held_apply(80, 1);
    wait_done(&mut app, 32);
    app.handle_key(key(KeyCode::Esc));
    hold.store(usize::MAX, Ordering::SeqCst);
    wait_search(&mut app);
    assert_eq!(check_results(&app, &d, &names), (32, 48));
    let warn = app
        .notices
        .iter()
        .map(|n| n.text.clone())
        .find(|t| t.contains("not replaced"))
        .expect("warning");
    assert!(warn.contains("48 files not replaced"), "{warn}");
    assert!(warn.contains("and 43 more"), "{warn}");
    assert!(warn.len() < 400, "{} chars", warn.len());
}

/// While a replace runs: option toggles, a new search and a new preview are refused, so
/// the toggles always match the results and the running replace keeps its preview.
#[test]
fn requests_are_refused_while_replacing() {
    let (_d, mut app, hold, _) = held_apply(40, 0);
    let before = (
        app.search.case_sensitive,
        app.search.regex,
        app.search.whole_word,
    );
    for c in ['c', 'r', 'w'] {
        app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT));
    }
    assert_eq!(
        (
            app.search.case_sensitive,
            app.search.regex,
            app.search.whole_word
        ),
        before,
        "toggles changed during a replace"
    );
    app.search.field = Field::Query;
    app.handle_key(key(KeyCode::Enter));
    app.search.field = Field::Replace;
    app.handle_key(key(KeyCode::Enter));
    assert!(!app.search.running(), "a search started during a replace");
    assert!(app.search.preview.as_ref().unwrap().applying());
    assert!(
        notices(&app).contains("replace is running"),
        "{}",
        notices(&app)
    );
    hold.store(usize::MAX, Ordering::SeqCst);
    wait_search(&mut app);
    assert!(app.search.preview.as_ref().unwrap().applied);
}

/// A file that fails in the middle of the run (read-only) is reported with its reason and
/// left as it was; the batches after it are still written.
#[test]
fn apply_reports_a_file_failing_halfway() {
    use std::os::unix::fs::PermissionsExt;
    let files = many_files(80);
    let refs: Vec<(&str, &str)> = files
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let (d, mut app) = project(&refs);
    preview(&mut app, "foo", "bar");
    let ro = d.path().join("f040.txt");
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o444)).unwrap();
    confirm(&mut app);
    for (name, _) in &files {
        match result(&app, name) {
            Some(Ok(1)) => assert_eq!(read(&d, name), "bar\n", "{name}"),
            Some(Err(e)) => {
                assert_eq!(name, "f040.txt", "{name}: {e}");
                assert!(e.contains("read-only"), "{e}");
                assert_eq!(read(&d, name), "foo\n");
            }
            r => panic!("{name}: {r:?}"),
        }
    }
    assert!(
        notices(&app).contains("1 file not replaced: f040.txt"),
        "{}",
        notices(&app)
    );
}

/// FR-5: a target file opened in a tab while the replace runs (before the worker reached
/// it) is never written by the worker: it is reported "opened in a tab during the replace",
/// the disk keeps its text and the tab its edits; the other files are replaced.
#[test]
fn tab_opened_during_a_replace_is_never_written() {
    let (d, mut app, hold, names) = held_apply(40, 0);
    app.open_path(Path::new("f005.txt")).unwrap();
    app.focus = Focus::Editor;
    app.handle_key(ctrl('i'));
    app.handle_key(key(KeyCode::Char('x')));
    let i = app.active;
    hold.store(usize::MAX, Ordering::SeqCst);
    wait_search(&mut app);
    assert_eq!(read(&d, "f005.txt"), "foo\n", "written behind the tab");
    let e = result(&app, "f005.txt").unwrap().unwrap_err();
    assert!(e.contains("opened in a tab during the replace"), "{e}");
    assert_eq!(app.docs[i].buffer.text().to_string(), "xfoo\n");
    let others: Vec<String> = names.iter().filter(|n| *n != "f005.txt").cloned().collect();
    assert_eq!(check_results(&app, &d, &others), (39, 0));
    let out = crate::commands::file::save_doc(&mut app, i);
    assert_eq!(out, crate::commands::file::SaveOutcome::Saved);
    assert_eq!(read(&d, "f005.txt"), "xfoo\n");
}

/// Renames `rel` in the explorer to `name` (F2 prompt).
fn explorer_rename(app: &mut App, rel: &str, name: &str) {
    let target = app.root.join(rel);
    app.explorer.visible = true;
    crate::commands::explorer::ensure_loaded(app);
    for a in target.ancestors().collect::<Vec<_>>().into_iter().rev() {
        if a.starts_with(&app.root) && a != target && !app.explorer.expanded.contains(a) {
            app.explorer.expand(a).unwrap();
        }
    }
    app.explorer.cursor = app
        .explorer
        .rows
        .iter()
        .position(|r| r.path == target)
        .unwrap();
    app.run_command("explorer.rename", crate::commands::CmdArgs::None);
    let crate::ui::dialog::DialogWidget::Prompt(p) = &app.dialog.as_ref().unwrap().widget else {
        panic!("rename prompt expected")
    };
    let n = p.input.text().chars().count();
    app.handle_key(key(KeyCode::End));
    for _ in 0..n {
        app.handle_key(key(KeyCode::Backspace));
    }
    typ(app, name);
    app.handle_key(key(KeyCode::Enter));
    assert!(app.dialog.is_none(), "rename refused: {}", notices(app));
}

/// FR-5: results follow an explorer rename (a file, or a folder holding results): Enter on
/// a result opens the file under its new name instead of failing with "No such file".
#[test]
fn search_results_follow_an_explorer_rename() {
    let (_d, mut app) = project(&[("src/a.rs", "needle\n"), ("src/sub/b.rs", "needle\n")]);
    search_for(&mut app, "needle");
    explorer_rename(&mut app, "src/a.rs", "c.rs");
    explorer_rename(&mut app, "src/sub", "lib");
    let mut paths: Vec<String> = app
        .search
        .groups
        .iter()
        .flat_map(|g| {
            std::iter::once(rel(&app, &g.path)).chain(g.matches.iter().map(|m| rel(&app, &m.path)))
        })
        .collect();
    paths.sort();
    paths.dedup();
    assert_eq!(paths, ["src/c.rs", "src/lib/b.rs"]);
    app.focus = Focus::Search;
    app.search.field = Field::Results;
    for i in 0..app.search.rows().len() {
        app.focus = Focus::Search;
        app.search.visible = true;
        app.search.selected = i;
        app.handle_key(key(KeyCode::Enter));
        assert!(!notices(&app).contains("No such file"), "{}", notices(&app));
    }
    assert_eq!(app.docs.len(), 2);
}

/// FR-5: a replace preview built before an explorer rename of one of its files is dropped
/// with a notice (its paths are stale); it is never confirmed with the old paths.
#[test]
fn replace_preview_dropped_after_an_explorer_rename() {
    let (d, mut app) = project(&[("a.txt", "foo\n"), ("b.txt", "foo\n")]);
    preview(&mut app, "foo", "bar");
    explorer_rename(&mut app, "a.txt", "c.txt");
    assert!(app.search.preview.is_none(), "stale preview kept");
    assert!(
        notices(&app).contains("search again") || notices(&app).contains("preview"),
        "{}",
        notices(&app)
    );
    assert_eq!(read(&d, "c.txt"), "foo\n");
    assert_eq!(read(&d, "b.txt"), "foo\n");
}

/// FR-5: below 40 columns the panel takes the whole width; Esc or focus.cycle from it give
/// the editor the keys only with the panel out of the way, never to an editor of width 0.
#[test]
fn narrow_esc_and_focus_cycle_never_type_into_a_hidden_editor() {
    for how in ["esc", "cycle"] {
        let (_d, mut app) = project(&[("a.txt", "needle\n")]);
        app.open_path(Path::new("a.txt")).unwrap();
        search_for(&mut app, "needle");
        draw(&mut app, 36, 12);
        assert_eq!(app.layout.editor.width, 0, "panel takes the whole width");
        assert_eq!(app.focus, Focus::Search);
        match how {
            "esc" => app.handle_key(key(KeyCode::Esc)),
            _ => {
                app.run_command("focus.cycle", crate::commands::CmdArgs::None);
            }
        }
        typ(&mut app, "X");
        let typed_into_editor = app
            .active_doc()
            .unwrap()
            .buffer
            .text()
            .to_string()
            .contains('X');
        if app.search.visible {
            assert!(!typed_into_editor, "{how}: typed into an editor of width 0");
            assert_eq!(app.focus, Focus::Search, "{how}");
        } else {
            draw(&mut app, 36, 12);
            assert!(app.layout.editor.width > 0, "{how}");
        }
    }
}

/// Save As of another tab onto a file the running replace has not reached yet: the worker
/// never writes it (the tab holds it now), even when its bytes still equal the preview.
#[test]
fn save_as_onto_a_pending_replace_file_is_never_written() {
    let (d, mut app, hold, names) = held_apply(40, 0);
    std::fs::write(d.path().join("other.txt"), "foo\n").unwrap();
    app.open_path(Path::new("other.txt")).unwrap();
    app.run_command(
        "file.save_as",
        crate::commands::CmdArgs::Path(d.path().join("f005.txt")),
    );
    assert_eq!(app.active_name(), "f005.txt", "{}", notices(&app));
    hold.store(usize::MAX, Ordering::SeqCst);
    wait_search(&mut app);
    assert_eq!(read(&d, "f005.txt"), "foo\n", "written behind the tab");
    let e = result(&app, "f005.txt").unwrap().unwrap_err();
    assert!(e.contains("opened in a tab during the replace"), "{e}");
    let others: Vec<String> = names.iter().filter(|n| *n != "f005.txt").cloned().collect();
    assert_eq!(check_results(&app, &d, &others), (39, 0));
}

/// A search still running when the explorer renames a file can send the old path later:
/// the panel says files moved and to search again.
#[test]
fn rename_during_a_running_search_says_search_again() {
    let (d, mut app) = project(&[("a.rs", "needle\n")]);
    let (tx, rx) = mpsc::channel();
    app.search.visible = true;
    search::attach(&mut app, rx, Arc::new(AtomicBool::new(false)));
    explorer_rename(&mut app, "a.rs", "b.rs");
    tx.send(SearchMsg::Batch(vec![pm(d.path(), "a.rs", 1, "needle")]))
        .unwrap();
    tx.send(SearchMsg::Done {
        files: 1,
        matches: 1,
        elapsed: Duration::ZERO,
    })
    .unwrap();
    app.drain_search();
    let shown = app.search.errors.join(" | ");
    assert!(shown.contains("search again"), "{shown}");
}
