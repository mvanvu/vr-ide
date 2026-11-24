//! Rendering. `render` draws the whole frame from `App` and records the layout it used.

pub mod dialog;
pub mod editor_view;
pub mod explorer;
pub mod find_bar;
pub mod layout;
pub mod palette;
pub mod search_panel;
pub mod status_bar;
pub mod tabs;

use ratatui::Frame;

use crate::app::App;
use crate::theme::Theme;

pub fn render(frame: &mut Frame, app: &mut App) {
    let theme = Theme::from_name(&app.settings.theme);
    render_with(frame, app, &theme);
}

/// `render` with an explicit theme (previews draw every theme from the same state).
pub fn render_with(frame: &mut Frame, app: &mut App, theme: &Theme) {
    let area = frame.area();
    // Paint the theme background everywhere first; widgets then only patch their own cells.
    frame.buffer_mut().set_style(area, theme.text);
    let notice_rows = status_bar::notice_rows(app, area.width);
    let side = if app.search.visible {
        layout::Side::Search
    } else if app.explorer.visible {
        crate::commands::explorer::ensure_loaded(app);
        layout::Side::Explorer
    } else {
        layout::Side::None
    };
    let layout = layout::compute(area, notice_rows, side, find_bar::rows(app));
    app.layout = layout;
    explorer::render(frame.buffer_mut(), layout.explorer, app, theme);
    search_panel::render(frame, layout.search, app, theme);
    tabs::render(frame.buffer_mut(), layout.tabs, app, theme);
    find_bar::render(frame, layout.find_bar, app, theme);
    editor_view::render(frame, layout.editor, app, theme);
    status_bar::render_notice(frame.buffer_mut(), layout.notice, app, theme);
    status_bar::render(frame.buffer_mut(), layout.status, app, theme);
    // Overlays last: they draw over the editor and set the cursor.
    if app.palette.is_some() {
        palette::render(frame, area, app, theme);
    }
    if let Some(d) = &mut app.dialog {
        d.widget.render(frame, area, theme);
    }
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

    mod preview;

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

    /// The explorer starts hidden, so the welcome screen names its key (from the keymap).
    #[test]
    fn welcome_names_the_explorer_key() {
        let (mut app, _) = test_app(std::path::Path::new("/home/user/DEV/shop"));
        let screen = format!("{:?}", draw(&mut app, 80, 24).backend());
        let line = screen
            .lines()
            .find(|l| l.contains("explorer"))
            .unwrap_or_else(|| panic!("no explorer hint:\n{screen}"));
        assert!(
            line.contains(&app.key_hint("explorer.toggle_focus")),
            "{line}"
        );
        assert!(line.contains("ctrl+e"), "{line}");
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
        let theme = Theme::from_name(&app.settings.theme);
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
        key(&mut app, KeyCode::Char('i'), KeyModifiers::CONTROL);
        key(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
        app.set_active(2);
        key(&mut app, KeyCode::Char('i'), KeyModifiers::CONTROL);
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
    fn render_find_bar() {
        let (_d, _p, mut app, _) =
            app_with_file("fn alpha() {}\nfn beta() {}\n// alpha again: alpha\nfn gamma() {}\n");
        app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL));
        for c in "alpha".chars() {
            key(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
        }
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        key(&mut app, KeyCode::Char('w'), KeyModifiers::ALT);
        key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
        for c in "omega".chars() {
            key(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
        }
        snap("render_find_bar", &mut app);
    }

    /// A project with matches in two files, searched through the panel.
    fn searched_app(replace: bool) -> (tempfile::TempDir, App) {
        let d = tempfile::tempdir().unwrap();
        let r = d.path().join("shop");
        std::fs::create_dir_all(r.join("src/Entity")).unwrap();
        std::fs::write(
            r.join("src/Entity/Order.php"),
            "<?php\nclass Order\n{\n    private int $total = 0; // total\n}\n",
        )
        .unwrap();
        std::fs::write(r.join("README.md"), "# shop\nOrder total in cents.\n").unwrap();
        let (mut app, _) = test_app(&r);
        app.explorer.visible = true;
        app.open_path(std::path::Path::new("README.md")).unwrap();
        let chord = if replace { 'H' } else { 'F' };
        app.handle_key(KeyEvent::new(
            KeyCode::Char(chord),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ));
        for c in "total".chars() {
            key(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
        }
        if replace {
            key(&mut app, KeyCode::Tab, KeyModifiers::NONE);
            for c in "sum".chars() {
                key(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
            }
        }
        key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        let deadline = Instant::now() + Duration::from_secs(20);
        while app.search.running() || app.search.busy() {
            assert!(Instant::now() < deadline, "search did not finish");
            std::thread::sleep(Duration::from_millis(2));
            app.drain_search();
        }
        // Files arrive in walk order; the snapshot needs one order.
        app.search.groups.sort_by(|a, b| a.path.cmp(&b.path));
        if let Some(p) = &mut app.search.preview {
            p.files.sort_by(|a, b| a.path.cmp(&b.path));
        }
        app.notices.clear();
        (d, app)
    }

    #[test]
    fn render_search_panel() {
        let (_d, mut app) = searched_app(false);
        key(&mut app, KeyCode::Down, KeyModifiers::NONE);
        key(&mut app, KeyCode::Down, KeyModifiers::NONE);
        snap("render_search_panel", &mut app);
    }

    #[test]
    fn render_replace_preview() {
        let (_d, mut app) = searched_app(true);
        assert!(app.search.preview.is_some());
        key(&mut app, KeyCode::Down, KeyModifiers::NONE);
        snap("render_replace_preview", &mut app);
    }

    #[test]
    fn render_palette() {
        let (_d, _p, mut app, _) = app_with_file("fn main() {}\n");
        app.run_command("app.palette", crate::commands::CmdArgs::None);
        for c in "sa".chars() {
            key(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
        }
        key(&mut app, KeyCode::Down, KeyModifiers::NONE);
        snap("render_palette", &mut app);
        // The cursor sits in the palette input, after "sa".
        let mut t = draw(&mut app, 80, 24);
        let pos = t.get_cursor_position().unwrap();
        let input = app.palette.as_ref().unwrap().area;
        assert_eq!(pos.y, input.y + 1, "{pos:?} {input:?}");
    }

    /// D122: the palette never covers the status row, however small the screen.
    #[test]
    fn palette_leaves_the_status_row_visible() {
        let (_d, _p, mut app, _) = app_with_file("fn main() {}\n");
        app.run_command("app.palette", crate::commands::CmdArgs::None);
        for h in 4..=14 {
            let t = draw(&mut app, 40, h);
            let buf = t.backend().buffer();
            let last: String = (0..40).map(|x| buf[(x, h - 1)].symbol()).collect();
            assert!(
                !last.contains('└') && !last.contains('│'),
                "height {h}: {last:?}"
            );
        }
    }

    #[test]
    fn render_confirm_dialog() {
        let (d, _p, mut app, _) = app_with_file("one\n");
        key(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
        std::fs::write(d.path().join("second.rs"), "fn a() {}\n").unwrap();
        app.open_path(std::path::Path::new("second.rs")).unwrap();
        key(&mut app, KeyCode::Char('i'), KeyModifiers::CONTROL);
        key(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);
        app.run_command("app.quit", crate::commands::CmdArgs::None);
        snap("render_confirm_dialog", &mut app);
    }

    #[test]
    fn palette_and_dialog_answer_the_mouse() {
        let (d, path, mut app, _) = app_with_file("one\n");
        key(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
        let click = |column, row| MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        };
        app.run_command("app.palette", crate::commands::CmdArgs::None);
        for c in "save".chars() {
            key(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
        }
        draw(&mut app, 40, 12);
        let list = app.palette.as_ref().unwrap().list_area;
        assert_eq!(app.palette.as_ref().unwrap().items[0].name, "file.save");
        app.handle_mouse(click(list.x + 2, list.y));
        assert!(app.palette.is_none());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "xone\n");
        // A click outside the palette closes it.
        app.run_command("app.palette", crate::commands::CmdArgs::None);
        draw(&mut app, 40, 12);
        app.handle_mouse(click(0, 11));
        assert!(app.palette.is_none());
        // Dialog buttons answer clicks; clicks elsewhere do nothing while it is open.
        std::fs::write(d.path().join("b.txt"), "b\n").unwrap();
        // Closed from outside: back in read mode (D145).
        key(&mut app, KeyCode::Char('i'), KeyModifiers::CONTROL);
        key(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);
        app.run_command("app.quit", crate::commands::CmdArgs::None);
        draw(&mut app, 40, 12);
        app.handle_mouse(click(0, 0));
        assert!(app.dialog.is_some());
        let crate::ui::dialog::DialogWidget::Confirm(c) = &app.dialog.as_ref().unwrap().widget
        else {
            panic!("confirm expected")
        };
        let save_all = c.button_areas[0];
        app.handle_mouse(click(save_all.x + 1, save_all.y));
        assert!(app.quit);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "xyone\n");
    }

    /// A project folder named "shop" with the explorer shown and focused on the open file.
    fn explorer_app() -> (tempfile::TempDir, App) {
        let d = tempfile::tempdir().unwrap();
        let r = d.path().join("shop");
        for dir in ["src/Controller", "src/Entity", "tests", "vendor"] {
            std::fs::create_dir_all(r.join(dir)).unwrap();
        }
        let files = [
            (
                "src/Controller/OrderController.php",
                "<?php\n\nclass OrderController\n{\n}\n",
            ),
            ("src/Controller/UserController.php", "<?php\n"),
            ("src/Entity/Order.php", "<?php\n"),
            ("composer.json", "{}\n"),
            ("README.md", "# shop\n"),
            (".gitignore", "vendor/\n"),
        ];
        for (f, body) in files {
            std::fs::write(r.join(f), body).unwrap();
        }
        let (mut app, _) = test_app(&r);
        app.explorer.visible = true;
        app.open_path(std::path::Path::new("src/Controller/OrderController.php"))
            .unwrap();
        app.run_command("explorer.reveal", crate::commands::CmdArgs::None);
        app.notices.clear();
        (d, app)
    }

    #[test]
    fn render_explorer() {
        let (_d, mut app) = explorer_app();
        let mut t = draw(&mut app, 80, 24);
        insta::assert_snapshot!("render_explorer_80x24", t.backend());
        // The panel is a left column; the editor starts right of it.
        assert_eq!(app.layout.explorer.width, layout::explorer_width(80));
        assert_eq!(app.layout.editor.x, app.layout.explorer.width);
        // Focus in the explorer: the cursor row is highlighted, the editor shows no cursor.
        let theme = Theme::from_name(&app.settings.theme);
        let row = app.layout.explorer.y + app.explorer.cursor as u16;
        assert_eq!(
            t.backend().buffer()[(0, row)].bg,
            theme.list_selected.bg.unwrap()
        );
        assert!(t.get_cursor_position().is_err() || app.focus != crate::app::Focus::Editor);
    }

    #[test]
    fn render_explorer_narrow() {
        let (_d, mut app) = explorer_app();
        let t = draw(&mut app, 40, 12);
        insta::assert_snapshot!("render_explorer_narrow", t.backend());
        // Too narrow for a panel: the editor gets the whole width.
        draw(&mut app, 24, 12);
        assert_eq!(app.layout.explorer.width, 0);
        assert_eq!(app.layout.editor.width, 24);
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
        key(&mut app, KeyCode::Char('i'), KeyModifiers::CONTROL);
        key(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
        app.docs[0].disk_state = vr_core::DiskState::ChangedOnDisk;
        for w in [40u16, 30, 80] {
            let row = status_row(&draw(&mut app, w, 12));
            // At 30 columns the badge keeps its space (D156), which costs the cut mark a cell.
            let name = if w == 30 { "andler.rs" } else { "handler.rs" };
            assert!(row.contains(name), "{w} cols: {row:?}");
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
