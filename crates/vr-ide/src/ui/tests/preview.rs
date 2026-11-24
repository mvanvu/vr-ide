//! Writes self-contained HTML previews of representative screens (real cell colours) when
//! `VR_PREVIEW_DIR` is set; otherwise the test is a no-op. Not part of the product.

use std::fmt::Write as _;

use ratatui::buffer::Buffer;
use ratatui::style::{Color, Modifier};

use super::*;
use crate::app::Focus;
use crate::theme::ColorDepth;

fn indexed_rgb(i: u8) -> (u8, u8, u8) {
    const BASE: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (205, 49, 49),
        (13, 188, 121),
        (229, 229, 16),
        (36, 114, 200),
        (188, 63, 188),
        (17, 168, 205),
        (229, 229, 229),
        (102, 102, 102),
        (241, 76, 76),
        (35, 209, 139),
        (245, 245, 67),
        (59, 142, 234),
        (214, 112, 214),
        (41, 184, 219),
        (255, 255, 255),
    ];
    match i {
        0..=15 => BASE[usize::from(i)],
        16..=231 => {
            let n = i - 16;
            let lv = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
            (lv(n / 36), lv(n / 6 % 6), lv(n % 6))
        }
        _ => {
            let v = 8 + 10 * (i - 232);
            (v, v, v)
        }
    }
}

/// CSS colour; `Reset` is the terminal's own colour, shown as a typical dark terminal.
fn css(c: Color, is_bg: bool) -> String {
    let (r, g, b) = match c {
        Color::Rgb(r, g, b) => (r, g, b),
        Color::Indexed(i) => indexed_rgb(i),
        Color::Reset => {
            if is_bg {
                (30, 30, 30)
            } else {
                (204, 204, 204)
            }
        }
        Color::Black => indexed_rgb(0),
        Color::Red => indexed_rgb(1),
        Color::Green => indexed_rgb(2),
        Color::Yellow => indexed_rgb(3),
        Color::Blue => indexed_rgb(4),
        Color::Magenta => indexed_rgb(5),
        Color::Cyan => indexed_rgb(6),
        Color::Gray => indexed_rgb(7),
        Color::DarkGray => indexed_rgb(8),
        Color::LightRed => indexed_rgb(9),
        Color::LightGreen => indexed_rgb(10),
        Color::LightYellow => indexed_rgb(11),
        Color::LightBlue => indexed_rgb(12),
        Color::LightMagenta => indexed_rgb(13),
        Color::LightCyan => indexed_rgb(14),
        Color::White => indexed_rgb(15),
    };
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;")
}

fn grid_html(buf: &Buffer) -> String {
    let mut out = String::from("<pre class=\"g\">");
    for y in 0..buf.area.height {
        let mut run = String::new();
        let mut cur = String::new();
        let mut skip = 0usize;
        let flush = |out: &mut String, style: &str, run: &mut String| {
            if !run.is_empty() {
                let _ = write!(out, "<span style=\"{style}\">{}</span>", esc(run));
                run.clear();
            }
        };
        for x in 0..buf.area.width {
            let cell = &buf[(x, y)];
            let (mut fg, mut bg) = (css(cell.fg, false), css(cell.bg, true));
            if cell.modifier.contains(Modifier::REVERSED) {
                std::mem::swap(&mut fg, &mut bg);
            }
            let mut style = format!("color:{fg};background:{bg};");
            if cell.modifier.contains(Modifier::BOLD) {
                style.push_str("font-weight:700;");
            }
            if cell.modifier.contains(Modifier::UNDERLINED) {
                style.push_str("text-decoration:underline;");
            }
            if style != cur {
                flush(&mut out, &cur, &mut run);
                cur = style;
            }
            if skip > 0 {
                skip -= 1;
                continue;
            }
            run.push_str(cell.symbol());
            skip = ratatui::text::Span::raw(cell.symbol())
                .width()
                .saturating_sub(1);
        }
        flush(&mut out, &cur, &mut run);
        out.push('\n');
    }
    out.push_str("</pre>");
    out
}

type Scene = (&'static str, Box<dyn Fn() -> (Vec<tempfile::TempDir>, App)>);

fn scenes() -> Vec<Scene> {
    vec![
        (
            "Editor focused, explorer open, warning notice",
            Box::new(|| {
                let (d, mut app) = explorer_app();
                app.focus = Focus::Editor;
                app.warn("settings: example warning notice");
                (vec![d], app)
            }),
        ),
        (
            "Edit mode (EDIT badge; the others show READ)",
            Box::new(|| {
                let (d, _p, mut app, _) = app_with_file("fn main() {\n    let x = 1;\n}\n");
                app.focus = Focus::Editor;
                app.enter_edit_mode();
                (vec![d], app)
            }),
        ),
        (
            "Explorer focused (focus separator)",
            Box::new(|| {
                let (d, app) = explorer_app();
                (vec![d], app)
            }),
        ),
        (
            "Command palette",
            Box::new(|| {
                let (d, _p, mut app, _) = app_with_file("fn main() {}\n");
                app.run_command("app.palette", crate::commands::CmdArgs::None);
                for c in "sa".chars() {
                    key(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
                }
                key(&mut app, KeyCode::Down, KeyModifiers::NONE);
                (vec![d], app)
            }),
        ),
        (
            "Find bar with matches",
            Box::new(|| {
                let (d, _p, mut app, _) = app_with_file(
                    "fn alpha() {}\nfn beta() {}\n// alpha again: alpha\nfn gamma() {}\n",
                );
                app.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::CONTROL));
                for c in "alpha".chars() {
                    key(&mut app, KeyCode::Char(c), KeyModifiers::NONE);
                }
                key(&mut app, KeyCode::Enter, KeyModifiers::NONE);
                (vec![d], app)
            }),
        ),
        (
            "Search panel with results",
            Box::new(|| {
                let (d, mut app) = searched_app(false);
                key(&mut app, KeyCode::Down, KeyModifiers::NONE);
                key(&mut app, KeyCode::Down, KeyModifiers::NONE);
                (vec![d], app)
            }),
        ),
        (
            "Confirm dialog",
            Box::new(|| {
                let (d, _p, mut app, _) = app_with_file("one\n");
                key(&mut app, KeyCode::Char('x'), KeyModifiers::NONE);
                std::fs::write(d.path().join("second.rs"), "fn a() {}\n").unwrap();
                app.open_path(std::path::Path::new("second.rs")).unwrap();
                key(&mut app, KeyCode::Char('y'), KeyModifiers::NONE);
                app.run_command("app.quit", crate::commands::CmdArgs::None);
                (vec![d], app)
            }),
        ),
    ]
}

fn page(title: &str, theme: &Theme) -> String {
    let mut html = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>vr-ide theme preview: {title}</title>\
<style>body{{background:#111;color:#ccc;font:14px sans-serif;margin:16px}}\
h2{{font-size:15px;margin:24px 0 4px}}\
.g{{font:14px/1.15 'DejaVu Sans Mono',Menlo,monospace;margin:0 0 8px;display:block;overflow-x:auto;width:max-content;max-width:100%}}\
.g span{{white-space:pre}}</style><h1>vr-ide theme: {title}</h1>"
    );
    for (name, build) in scenes() {
        for (w, h) in [(120u16, 40u16), (80, 24)] {
            let (_keep, mut app) = build();
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            t.draw(|f| render_with(f, &mut app, theme)).unwrap();
            let _ = write!(
                html,
                "<h2>{} ({w}x{h})</h2>{}",
                esc(name),
                grid_html(t.backend().buffer())
            );
        }
    }
    html
}

fn rgb_hex(c: Color) -> String {
    css(c, false)
}

/// Role, colour and its contrast on the editor background, for the comparison page.
fn token_table(theme: &Theme) -> String {
    let bg = theme.text.bg.unwrap();
    let rows = [
        ("text", theme.text.fg, theme.text.bg),
        ("gutter", theme.gutter.fg, None),
        ("dim", theme.dim.fg, None),
        ("tab_active", theme.tab_active.fg, None),
        ("focus", theme.focus.fg, None),
        ("popup_border", theme.popup_border.fg, None),
        ("info", theme.info.fg, None),
        ("warning", theme.warning.fg, None),
        ("error", theme.error.fg, None),
        ("matched", theme.matched.fg, None),
        ("gutter_active", theme.gutter_active.fg, None),
        ("mode_read bg", theme.mode_read.bg, None),
        ("mode_edit bg", theme.mode_edit.bg, None),
        ("bar bg", theme.tab_bar.bg, None),
        ("popup bg", theme.popup.bg, None),
        ("selection bg", theme.selection.bg, None),
        ("list_selected bg", theme.list_selected.bg, None),
    ];
    let mut out = String::from("<table>");
    for (role, c, _) in rows {
        let c = c.unwrap();
        let ratio = crate::theme::contrast(
            crate::theme::to_rgb(c).unwrap(),
            crate::theme::to_rgb(bg).unwrap(),
        );
        let _ = write!(
            out,
            "<tr><td><i style=\"background:{0}\"></i></td><td>{role}</td><td>{0}</td><td>{ratio:.1}:1</td></tr>",
            rgb_hex(c)
        );
    }
    out.push_str("</table>");
    out
}

fn compare_page(themes: &[(String, Theme)]) -> String {
    let mut html = String::from(
        "<!doctype html><meta charset=\"utf-8\"><title>vr-ide theme options</title>\
<style>body{background:#111;color:#ccc;font:14px sans-serif;margin:16px}\
.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(min(100%,980px),1fr));gap:24px}\
h2{font-size:16px;margin:0 0 6px}\
.g{font:14px/1.15 'DejaVu Sans Mono',Menlo,monospace;margin:0 0 8px;display:block;overflow-x:auto;width:max-content;max-width:100%}\
.g span{white-space:pre}\
table{border-collapse:collapse;font-size:12px}td{padding:1px 8px}\
i{display:inline-block;width:18px;height:12px;border:1px solid #666}</style>\
<h1>vr-ide theme options (editor + explorer, 120x40)</h1><div class=\"grid\">",
    );
    let (_, build) = scenes().into_iter().next().unwrap();
    for (name, theme) in themes {
        let (_keep, mut app) = build();
        let t = draw_with(&mut app, theme, 120, 40);
        let _ = write!(
            html,
            "<section><h2>{}</h2>{}{}</section>",
            esc(name),
            grid_html(t.backend().buffer()),
            token_table(theme)
        );
    }
    html.push_str("</div>");
    html
}

#[test]
fn write_theme_previews() {
    let Ok(dir) = std::env::var("VR_PREVIEW_DIR") else {
        return;
    };
    std::fs::create_dir_all(&dir).unwrap();
    let themes = vec![
        (
            "synthwave (default)".to_string(),
            "synthwave",
            Theme::synthwave_with(ColorDepth::TrueColor),
        ),
        (
            "dracula".to_string(),
            "dracula",
            Theme::dracula_with(ColorDepth::TrueColor),
        ),
    ];
    for (label, file, theme) in &themes {
        std::fs::write(format!("{dir}/{file}.html"), page(label, theme)).unwrap();
    }
    let pairs: Vec<_> = themes.into_iter().map(|(l, _, t)| (l, t)).collect();
    std::fs::write(format!("{dir}/compare.html"), compare_page(&pairs)).unwrap();
}

/// Every cell of every screen carries the theme background (no terminal-default holes).
#[test]
fn dracula_paints_its_background_on_every_cell() {
    let theme = Theme::dracula_with(ColorDepth::TrueColor);
    for (name, build) in scenes() {
        for (w, h) in [(120u16, 40u16), (80, 24), (40, 12)] {
            let (_keep, mut app) = build();
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            t.draw(|f| render_with(f, &mut app, &theme)).unwrap();
            let buf = t.backend().buffer();
            for y in 0..h {
                for x in 0..w {
                    let bg = buf[(x, y)].bg;
                    assert!(
                        matches!(bg, Color::Rgb(..)),
                        "{name} {w}x{h} at ({x},{y}): {bg:?}"
                    );
                }
            }
        }
    }
}

/// The focused side panel is marked by its separator, in a style the unfocused one lacks.
#[test]
fn focused_panel_separator_differs() {
    let theme = Theme::dracula_with(ColorDepth::TrueColor);
    let (_d, mut app) = explorer_app();
    let fg_at = |focus: Focus, app: &mut App| {
        app.focus = focus;
        let mut t = Terminal::new(TestBackend::new(80, 24)).unwrap();
        t.draw(|f| render_with(f, app, &theme)).unwrap();
        let x = app.layout.explorer.width - 1;
        t.backend().buffer()[(x, 3)].fg
    };
    let focused = fg_at(Focus::Explorer, &mut app);
    let other = fg_at(Focus::Editor, &mut app);
    assert_eq!(focused, theme.focus.fg.unwrap());
    assert_eq!(other, theme.dim.fg.unwrap());
}

#[test]
fn default_theme_is_synthwave() {
    let (app, _) = test_app(std::path::Path::new("/tmp"));
    assert_eq!(app.settings.theme, "synthwave");
}

fn scene(name: &str) -> (Vec<tempfile::TempDir>, App) {
    let (_, build) = scenes().into_iter().find(|(n, _)| *n == name).unwrap();
    build()
}

fn draw_with(app: &mut App, theme: &Theme, w: u16, h: u16) -> Terminal<TestBackend> {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| render_with(f, app, theme)).unwrap();
    t
}

/// Cells inside the popup frame (first top-left corner to the first bottom-right corner).
fn popup_cells(buf: &Buffer) -> Vec<(u16, u16)> {
    let find = |sym: &str| {
        (0..buf.area.height)
            .flat_map(|y| (0..buf.area.width).map(move |x| (x, y)))
            .find(|&(x, y)| buf[(x, y)].symbol() == sym)
            .unwrap()
    };
    let (x0, y0) = find("┌");
    let (x1, y1) = find("┘");
    (y0..=y1)
        .flat_map(|y| (x0..=x1).map(move |x| (x, y)))
        .collect()
}

#[test]
fn popups_never_show_the_editor_background_inside_them() {
    for depth in [ColorDepth::TrueColor, ColorDepth::Indexed256] {
        let theme = Theme::dracula_with(depth);
        for name in ["Confirm dialog", "Command palette"] {
            for (w, h) in [(120u16, 40u16), (80, 24)] {
                let (_keep, mut app) = scene(name);
                let t = draw_with(&mut app, &theme, w, h);
                let buf = t.backend().buffer();
                let cells = popup_cells(buf);
                assert!(cells.len() > 20, "{name}");
                for (x, y) in cells {
                    assert_ne!(
                        Some(buf[(x, y)].bg),
                        theme.text.bg,
                        "{depth:?} {name} {w}x{h} ({x},{y}) {:?}",
                        buf[(x, y)].symbol()
                    );
                }
            }
        }
    }
}

#[test]
fn prompt_dialog_body_keeps_the_popup_background() {
    let theme = Theme::dracula_with(ColorDepth::TrueColor);
    let (_d, _p, mut app, _) = app_with_file("one\n");
    app.run_command("file.save_as", crate::commands::CmdArgs::None);
    assert!(app.dialog.is_some());
    let t = draw_with(&mut app, &theme, 80, 24);
    let buf = t.backend().buffer();
    for (x, y) in popup_cells(buf) {
        assert_ne!(Some(buf[(x, y)].bg), theme.text.bg, "({x},{y})");
    }
}

#[test]
fn search_panel_separator_marks_the_focus() {
    let theme = Theme::dracula_with(ColorDepth::TrueColor);
    let (_d, mut app) = scene("Search panel with results");
    for (focus, want) in [
        (Focus::Search, theme.focus.fg),
        (Focus::Editor, theme.dim.fg),
    ] {
        app.focus = focus;
        let t = draw_with(&mut app, &theme, 80, 24);
        let x = app.layout.search.x + app.layout.search.width - 1;
        assert_eq!(t.backend().buffer()[(x, 3)].fg, want.unwrap(), "{focus:?}");
    }
}

#[test]
fn cursor_line_number_is_active_only_with_editor_focus_and_no_dialog() {
    let theme = Theme::dracula_with(ColorDepth::TrueColor);
    let (_d, _p, mut app, _) = app_with_file("one\ntwo\n");
    let digit_fg = |app: &mut App| {
        let t = draw_with(app, &theme, 80, 24);
        let ed = app.layout.editor;
        let g = editor_view::gutter_width(app.active_doc().unwrap().buffer.text());
        t.backend().buffer()[(ed.x + g - 2, ed.y)].fg
    };
    app.focus = Focus::Editor;
    assert_eq!(digit_fg(&mut app), theme.gutter_active.fg.unwrap());
    app.focus = Focus::Explorer;
    assert_eq!(digit_fg(&mut app), theme.gutter_current.fg.unwrap());
    app.focus = Focus::Editor;
    app.run_command("file.save_as", crate::commands::CmdArgs::None);
    assert!(app.dialog.is_some());
    assert_eq!(digit_fg(&mut app), theme.gutter_current.fg.unwrap());
}

#[test]
fn unknown_theme_name_warns_and_names_the_fallback() {
    let (mut app, _) = test_app(std::path::Path::new("/tmp"));
    app.settings.theme = "draculaa".into();
    app.report_config(vec![]);
    let text = &app.notices.back().unwrap().text;
    assert!(text.contains("\"draculaa\""), "{text}");
    assert!(text.contains("using synthwave"), "{text}");
}
