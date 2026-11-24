//! Rendering. `render` draws the whole frame from `App` and records the layout it used.

pub mod editor_view;
pub mod layout;
pub mod status_bar;
pub mod tabs;

use ratatui::Frame;

use crate::app::App;
use crate::theme::Theme;

pub fn render(frame: &mut Frame, app: &mut App) {
    let theme = Theme::from_name(&app.settings.theme);
    let area = frame.area();
    let notice_rows = status_bar::notice_rows(app, area.width);
    let layout = layout::compute(area, notice_rows);
    app.layout = layout;
    tabs::render(frame.buffer_mut(), layout.tabs, app, &theme);
    editor_view::render(frame, layout.editor, app, &theme);
    status_bar::render_notice(frame.buffer_mut(), layout.notice, app, &theme);
    status_bar::render(frame.buffer_mut(), layout.status, app, &theme);
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crossterm::event::{
        KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
    };
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use vr_core::{Document, Selection};

    use super::*;
    use crate::app_tests::{app_with_file, test_app};

    const SIZES: [(u16, u16); 2] = [(80, 24), (40, 12)];

    fn draw(app: &mut App, w: u16, h: u16) -> Terminal<TestBackend> {
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| render(f, app)).unwrap();
        t
    }

    fn snap(name: &str, app: &mut App) {
        for (w, h) in SIZES {
            let t = draw(app, w, h);
            insta::assert_snapshot!(format!("{name}_{w}x{h}"), t.backend());
        }
    }

    fn key(app: &mut App, code: KeyCode, mods: KeyModifiers) {
        app.handle_key(KeyEvent::new(code, mods));
    }

    #[test]
    fn render_empty_project() {
        let (mut app, _) = test_app(std::path::Path::new("/home/user/DEV/shop"));
        snap("render_empty_project", &mut app);
    }

    #[test]
    fn render_file_with_selection() {
        let (_d, _p, mut app, _) =
            app_with_file("fn main() {\n    let x = 1;\n    println!(\"{x}\");\n}\n");
        key(&mut app, KeyCode::Down, KeyModifiers::NONE);
        key(&mut app, KeyCode::End, KeyModifiers::SHIFT);
        snap("render_file_with_selection", &mut app);
        // Selected cells carry the selection style; the rest do not.
        let t = draw(&mut app, 80, 24);
        let theme = Theme::dark();
        let buf = t.backend().buffer();
        let gutter = editor_view::gutter_width(app.active_doc().unwrap().buffer.text());
        assert_eq!(buf[(gutter, 2)].bg, theme.selection.bg.unwrap());
        assert_ne!(buf[(gutter, 1)].bg, theme.selection.bg.unwrap());
    }

    #[test]
    fn render_wide_chars() {
        // Decomposed Vietnamese (combining marks), precomposed Vietnamese, CJK, an emoji ZWJ
        // sequence, a flag and a tab.
        let text = "Tie\u{302}\u{301}ng Việt\n漢字x\n👨‍👩‍👧 🇻🇳 ok\n\tTab|\n";
        let (_d, _p, mut app, _) = app_with_file(text);
        key(&mut app, KeyCode::Down, KeyModifiers::NONE);
        for _ in 0..3 {
            key(&mut app, KeyCode::Right, KeyModifiers::NONE);
        }
        snap("render_wide_chars", &mut app);
        let mut t = draw(&mut app, 80, 24);
        let gutter = editor_view::gutter_width(app.active_doc().unwrap().buffer.text());
        // After 漢字x the cursor sits 5 cells right of the gutter.
        let pos = t.get_cursor_position().unwrap();
        assert_eq!((pos.x, pos.y), (gutter + 5, 2));
        // One Right steps over the whole ZWJ family (one grapheme, two cells).
        key(&mut app, KeyCode::Down, KeyModifiers::NONE);
        key(&mut app, KeyCode::Home, KeyModifiers::NONE);
        key(&mut app, KeyCode::Right, KeyModifiers::NONE);
        t.draw(|f| render(f, &mut app)).unwrap();
        let pos = t.get_cursor_position().unwrap();
        assert_eq!((pos.x, pos.y), (gutter + 2, 3));
    }

    #[test]
    fn render_tabs_modified() {
        let (d, _p, mut app, _) = app_with_file("first\n");
        for (n, body) in [("second.rs", "fn a() {}\n"), ("third.md", "# t\n")] {
            let p = d.path().join(n);
            std::fs::write(&p, body).unwrap();
            app.open_path(&p).unwrap();
        }
        // second.rs: modified; third.md: active, changed on disk while modified.
        app.set_active(1);
        key(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
        app.set_active(2);
        key(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);
        app.docs[2].disk_state = vr_core::DiskState::ChangedOnDisk;
        app.notices.clear();
        snap("render_tabs_modified", &mut app);
    }

    #[test]
    fn render_long_line_horizontal_scroll() {
        let long: String = (0..30).map(|i| format!("w{i:02} ")).collect();
        let (_d, _p, mut app, _) = app_with_file(&format!("short\n{long}end\nlast\n"));
        key(&mut app, KeyCode::Down, KeyModifiers::NONE);
        key(&mut app, KeyCode::End, KeyModifiers::NONE);
        snap("render_long_line_horizontal_scroll", &mut app);
        let mut t = draw(&mut app, 40, 12);
        let pos = t.get_cursor_position().unwrap();
        assert!(pos.x < 40 && pos.y == 2, "{pos:?}");
    }

    #[test]
    fn render_notice_and_read_only_status() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("l1.txt"), b"caf\xe9\n").unwrap();
        let (mut app, _) = test_app(dir.path());
        app.open_path(std::path::Path::new("l1.txt")).unwrap();
        app.warn("second notice");
        snap("render_notice_and_read_only_status", &mut app);
    }

    fn status_row(t: &Terminal<TestBackend>) -> String {
        let buf = t.backend().buffer();
        let y = buf.area.height - 1;
        (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
    }

    #[test]
    fn status_bar_keeps_the_file_name_at_40_columns() {
        let dir = tempfile::tempdir().unwrap();
        let deep = dir.path().join("src/very/deeply/nested/module");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("handler.rs"), "fn a() {}\n").unwrap();
        std::fs::write(dir.path().join("l1.txt"), b"caf\xe9\n").unwrap();
        let (mut app, _) = test_app(dir.path());
        app.open_path(&deep.join("handler.rs")).unwrap();
        key(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
        app.docs[0].disk_state = vr_core::DiskState::ChangedOnDisk;
        for w in [40u16, 30, 80] {
            let row = status_row(&draw(&mut app, w, 12));
            assert!(row.contains("handler.rs"), "{w} cols: {row:?}");
            assert!(row.contains("Ln 1"), "{w} cols: {row:?}");
        }
        let row = status_row(&draw(&mut app, 80, 12));
        assert!(row.contains("[changed on disk]"), "{row:?}");
        app.open_path(std::path::Path::new("l1.txt")).unwrap();
        let row = status_row(&draw(&mut app, 40, 12));
        assert!(row.contains("l1.txt"), "{row:?}");
    }

    #[test]
    fn mouse_click_drag_and_wheel() {
        let body: String = (0..100).map(|i| format!("line {i}\n")).collect();
        let (_d, _p, mut app, _) = app_with_file(&body);
        draw(&mut app, 80, 24);
        let ed = app.layout.editor;
        let gutter = editor_view::gutter_width(app.active_doc().unwrap().buffer.text());
        let ev = |kind, col, row| MouseEvent {
            kind,
            column: col,
            row,
            modifiers: KeyModifiers::NONE,
        };
        // Click on line 2, column 3 ("line 2" -> before 'e').
        app.handle_mouse(ev(
            MouseEventKind::Down(MouseButton::Left),
            ed.x + gutter + 3,
            ed.y + 2,
        ));
        let rope = app.active_doc().unwrap().buffer.text().clone();
        let head = app.active_doc().unwrap().buffer.selection().primary().head;
        assert_eq!(head, rope.line_to_char(2) + 3);
        app.handle_mouse(ev(
            MouseEventKind::Drag(MouseButton::Left),
            ed.x + gutter + 2,
            ed.y + 4,
        ));
        let sel = app.active_doc().unwrap().buffer.selection().primary();
        assert_eq!(
            (sel.anchor, sel.head),
            (rope.line_to_char(2) + 3, rope.line_to_char(4) + 2)
        );
        app.handle_mouse(ev(
            MouseEventKind::Up(MouseButton::Left),
            ed.x + gutter + 2,
            ed.y + 4,
        ));
        // Wheel scrolls the view, not the cursor.
        app.handle_mouse(ev(MouseEventKind::ScrollDown, ed.x + 5, ed.y + 5));
        assert_eq!(app.views[0].top, 3);
        draw(&mut app, 80, 24);
        assert_eq!(
            app.views[0].top, 3,
            "a render must not snap back to the cursor"
        );
        // Clicking a tab activates it.
        let p2 = _d.path().join("b.txt");
        std::fs::write(&p2, "b").unwrap();
        app.open_path(&p2).unwrap();
        draw(&mut app, 80, 24);
        app.handle_mouse(ev(MouseEventKind::Down(MouseButton::Left), 1, 0));
        assert_eq!(app.active, 0);
    }

    /// AC-18 / M3: a single 10 MB line (minified code) keeps keystroke-to-frame bounded with the
    /// cursor anywhere on it. Release only:
    /// `cargo test -p vr-ide --release long_line_keystroke_to_frame_fast -- --ignored`.
    #[test]
    #[ignore]
    fn long_line_keystroke_to_frame_fast() {
        let unit = "var a=\"Tiếng Việt\",b=[1,2,3];function f(x){return x*2}\t/*漢字*/";
        let line = unit.repeat(10 * 1024 * 1024 / unit.len());
        let mut doc = Document::untitled();
        doc.buffer = vr_core::Buffer::from_str(&format!("{line}\nshort\n"));
        let (mut app, _) = test_app(std::path::Path::new("/p"));
        app.docs.push(doc);
        app.views.push(Default::default());
        let mut t = Terminal::new(TestBackend::new(120, 40)).unwrap();
        t.draw(|f| render(f, &mut app)).unwrap();
        let ops: [(&str, KeyCode, KeyModifiers); 6] = [
            ("end", KeyCode::End, KeyModifiers::NONE),
            ("type", KeyCode::Char('x'), KeyModifiers::NONE),
            ("left", KeyCode::Left, KeyModifiers::NONE),
            ("right", KeyCode::Right, KeyModifiers::NONE),
            ("backspace", KeyCode::Backspace, KeyModifiers::NONE),
            ("home+end", KeyCode::Home, KeyModifiers::NONE),
        ];
        let mut worst = (Duration::ZERO, "");
        for (name, code, mods) in ops {
            let mut times = Vec::new();
            for i in 0..11 {
                let s = Instant::now();
                key(&mut app, code, mods);
                t.draw(|f| render(f, &mut app)).unwrap();
                let took = s.elapsed();
                assert!(took < Duration::from_secs(1), "{name} #{i}: {took:?}");
                times.push(took);
                if name == "home+end" {
                    key(&mut app, KeyCode::End, KeyModifiers::NONE);
                }
            }
            times.sort();
            let median = times[times.len() / 2];
            if median > worst.0 {
                worst = (median, name);
            }
        }
        let pos = t.get_cursor_position().unwrap();
        assert!(pos.x < 120 && pos.y == 1, "cursor off screen: {pos:?}");
        eprintln!(
            "long_line_keystroke_to_frame_fast 10 MB line: worst median {:?} ({})",
            worst.0, worst.1
        );
        assert!(worst.0 < Duration::from_millis(5), "{worst:?}");
    }

    /// AC-18: frame time does not depend on file size. Release only:
    /// `cargo test -p vr-ide --release render_large_file_fast -- --ignored`.
    #[test]
    #[ignore]
    fn render_large_file_fast() {
        let line = "0123456789 abcdefghij ABCDEFGHIJ 0123456789 abcdefghij ABCDEFGHIJ 0123456789\n";
        for (label, lines) in [
            ("100k lines", 100_000usize),
            ("100 MB", 100 * 1024 * 1024 / line.len()),
        ] {
            let mut doc = Document::untitled();
            doc.buffer = vr_core::Buffer::from_str(&line.repeat(lines));
            let (mut app, _) = test_app(std::path::Path::new("/p"));
            app.docs.push(doc);
            app.views.push(Default::default());
            let mut t = Terminal::new(TestBackend::new(120, 40)).unwrap();
            let mut worst = Duration::ZERO;
            for target in [0, lines / 2, lines - 1] {
                let rope = app.docs[0].buffer.text().clone();
                let at = rope.line_to_char(target);
                app.docs[0].buffer.set_selection(Selection::point(at));
                app.cursor_moved();
                let mut times: Vec<Duration> = (0..20)
                    .map(|_| {
                        let s = Instant::now();
                        t.draw(|f| render(f, &mut app)).unwrap();
                        s.elapsed()
                    })
                    .collect();
                times.sort();
                worst = worst.max(times[times.len() / 2]);
            }
            eprintln!("render_large_file_fast {label}: median frame (worst position) {worst:?}");
            assert!(worst < Duration::from_millis(5), "{label}: {worst:?}");
        }
    }
}
