//! T19 (D227): the terminal panel on the right of the editor. Layout at the review sizes and
//! the narrow fallback, the position command and key, the separator drag, grow / shrink of the
//! width, the find box beside the panel, the PTY following every layout change and snapshots.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use vr_config::TerminalPosition;
use vr_term::TermStatus;

use crate::app::{App, Focus};
use crate::app_tests::{app_with_file, legacy_event, test_app};
use crate::commands::CmdArgs;
use crate::terminal_tests::{
    ctrl_backtick, draw, fake_tab, frame_text, row_text, snapshot_app, tab, term_app, typ,
    wait_text,
};
use crate::ui::layout::{self, EDITOR_MIN_COLS, TERMINAL_MIN_COLS};

fn right(app: &mut App) {
    app.terminal.position = TerminalPosition::Right;
}

/// A panel with one fake terminal, shown, on the right.
fn right_app() -> (tempfile::TempDir, App) {
    let (d, mut app, _) = term_app();
    app.terminal.tabs.push(fake_tab("sh", TermStatus::Running));
    app.terminal.visible = true;
    right(&mut app);
    (d, app)
}

fn mouse(app: &mut App, kind: MouseEventKind, column: u16, row: u16) {
    app.handle_mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    });
}

fn last_notice(app: &App) -> String {
    app.notices
        .back()
        .map(|n| n.text.clone())
        .unwrap_or_default()
}

// ---- layout -------------------------------------------------------------------------------

/// The pure split: `percent` of the editor column, at least 20 columns, the editor keeps at
/// least 20 columns and the separator one.
#[test]
fn place_terminal_right_splits_the_editor_column() {
    let split = |width: u16, height: u16, percent: u16| {
        let mut l = layout::compute(Rect::new(0, 0, width, height), 0, layout::Side::None);
        layout::place_terminal_right(&mut l, percent, None);
        l
    };
    let l = split(120, 40, 40);
    assert_eq!(l.terminal, Rect::new(72, 1, 48, 38));
    assert_eq!(l.terminal_sep, Rect::new(71, 1, 1, 38));
    assert_eq!(l.editor, Rect::new(0, 1, 71, 38));
    let l = split(80, 24, 40);
    assert_eq!(l.terminal, Rect::new(48, 1, 32, 22));
    assert_eq!(l.editor.width, 47);
    // Smallest share: still 20 columns.
    assert_eq!(split(120, 40, 10).terminal.width, TERMINAL_MIN_COLS);
    // Largest share: the editor keeps its columns.
    let l = split(60, 20, 90);
    assert_eq!(l.editor.width, EDITOR_MIN_COLS);
    assert_eq!(l.terminal.width, 60 - EDITOR_MIN_COLS - 1);
    assert!(layout::right_fits(60));
    assert!(!layout::right_fits(59));
}

/// 120x40 and 80x24: the panel is right of the editor, from under the tab bar to above the
/// status bar; the tab bar keeps the whole editor column.
#[test]
fn right_panel_takes_the_full_editor_height() {
    for (w, h, cols) in [(120u16, 40u16, 48u16), (80, 24, 32)] {
        let (_d, mut app) = right_app();
        let t = draw(&mut app, w, h);
        let l = app.layout;
        assert_eq!(l.terminal, Rect::new(w - cols, 1, cols, h - 2), "{w}x{h}");
        assert_eq!(
            l.terminal_sep,
            Rect::new(w - cols - 1, 1, 1, h - 2),
            "{w}x{h}"
        );
        assert_eq!(l.editor, Rect::new(0, 1, w - cols - 1, h - 2), "{w}x{h}");
        assert_eq!(l.tabs.width, w, "{w}x{h}");
        // The separator is drawn on every row of the panel.
        for y in l.terminal_sep.y..l.terminal_sep.y + l.terminal_sep.height {
            let row: Vec<char> = row_text(&t, y).chars().collect();
            assert_eq!(row[usize::from(l.terminal_sep.x)], '│', "{w}x{h} row {y}");
        }
    }
}

/// Below 60 columns of editor column the panel goes to the bottom (the setting is kept);
/// with the explorer shown the editor column is what counts.
#[test]
fn a_narrow_editor_column_falls_back_to_the_bottom() {
    let (_d, mut app) = right_app();
    draw(&mut app, 59, 20);
    assert_eq!(app.layout.terminal_sep, Rect::default());
    assert_eq!(app.layout.terminal.width, 59);
    assert!(app.layout.terminal.y > 1, "{:?}", app.layout);
    assert_eq!(app.terminal.position, TerminalPosition::Right);
    draw(&mut app, 60, 20);
    assert_eq!(app.layout.terminal.width, 24);
    assert_eq!(app.layout.terminal_sep.x, 35);
    assert_eq!(app.layout.terminal.y, 1);
    draw(&mut app, 40, 12);
    assert_eq!(app.layout.terminal_sep, Rect::default());
    assert_eq!(app.layout.terminal.width, 40);
    assert_eq!(app.layout.terminal.height, 5);
    // Explorer shown: 80 columns leave a 60-column editor column, 78 leave 59.
    app.explorer.visible = true;
    draw(&mut app, 80, 24);
    assert_eq!(app.layout.terminal.y, 1, "{:?}", app.layout);
    assert_eq!(
        app.layout.terminal.x + app.layout.terminal.width,
        80,
        "{:?}",
        app.layout
    );
    draw(&mut app, 78, 24);
    assert_eq!(app.layout.terminal_sep, Rect::default(), "{:?}", app.layout);
    assert!(app.layout.terminal.y > 1);
}

/// D275 on the right: the find box keeps its field rows (and its list) over the editor, left
/// of the separator; the panel keeps the full height.
#[test]
fn the_find_box_sits_over_the_editor_beside_the_panel() {
    let (_d, _p, mut app, _) = app_with_file("foo bar\nfoo\n");
    app.terminal.tabs.push(fake_tab("sh", TermStatus::Running));
    app.terminal.visible = true;
    right(&mut app);
    draw(&mut app, 80, 24);
    crate::app_tests::press(&mut app, "ctrl+f");
    typ(&mut app, "foo");
    let t = draw(&mut app, 80, 24);
    let l = app.layout;
    assert!(l.find_bar.height >= 2, "{l:?}");
    assert_eq!(l.find_bar.x, 0);
    assert_eq!(l.find_bar.x + l.find_bar.width, l.terminal_sep.x, "{l:?}");
    assert_eq!(l.editor.y, l.find_bar.y + l.find_bar.height);
    assert_eq!(l.terminal.y, 1);
    assert_eq!(l.terminal.height, 22);
    assert!(row_text(&t, 1).starts_with(" File "), "{}", row_text(&t, 1));
    // A short window: the fields still get their rows.
    let t = draw(&mut app, 80, 9);
    assert!(app.layout.find_bar.height >= 1, "{:?}", app.layout);
    assert!(row_text(&t, 1).starts_with(" File "), "{}", frame_text(&t));
}

// ---- command, key, slash ------------------------------------------------------------------

/// terminal.toggle_position flips the session's position and says how to keep it; a hidden
/// panel stays hidden (no shell is started for a layout change).
#[test]
fn toggle_position_flips_and_names_the_setting() {
    let (_d, mut app, _) = term_app();
    draw(&mut app, 120, 40);
    app.run_command("terminal.toggle_position", CmdArgs::None);
    assert_eq!(app.terminal.position, TerminalPosition::Right);
    assert!(!app.terminal.visible);
    assert!(app.terminal.tabs.is_empty());
    let n = last_notice(&app);
    assert!(n.contains("right"), "{n}");
    assert!(n.contains("terminal_position = \"right\""), "{n}");
    app.run_command("terminal.toggle_position", CmdArgs::None);
    assert_eq!(app.terminal.position, TerminalPosition::Bottom);
    let n = last_notice(&app);
    assert!(n.contains("bottom") && !n.contains("settings.toml"), "{n}");
    // Too narrow for the right: the notice says where it is shown.
    draw(&mut app, 50, 20);
    app.run_command("terminal.toggle_position", CmdArgs::None);
    let n = last_notice(&app);
    assert!(n.contains("60 columns"), "{n}");
}

/// ctrl+f7 runs it from the editor, as the kitty protocol reports it and as GNOME Terminal
/// sends it without (ESC [ 18 ; 5 ~); a focused terminal keeps the key for its program.
#[test]
fn ctrl_f7_toggles_the_position() {
    for enhanced in [true, false] {
        let (_d, mut app) = right_app();
        app.terminal.position = TerminalPosition::Bottom;
        app.keyboard_enhanced = enhanced;
        draw(&mut app, 120, 40);
        let (bytes, ev) = if enhanced {
            (Vec::new(), Some(crate::app_tests::kitty_event("ctrl+f7")))
        } else {
            legacy_event("ctrl+f7")
        };
        if !enhanced {
            assert_eq!(bytes, b"\x1b[18;5~");
        }
        app.handle_key(ev.unwrap());
        assert_eq!(app.terminal.position, TerminalPosition::Right, "{enhanced}");
        draw(&mut app, 120, 40);
        assert_eq!(app.layout.terminal.y, 1);
        assert_eq!(
            app.display_key("terminal.toggle_position").as_deref(),
            Some("ctrl+f7")
        );
    }
    let (_d, mut app) = right_app();
    app.focus = Focus::Terminal;
    draw(&mut app, 120, 40);
    app.handle_key(KeyEvent::new(KeyCode::F(7), KeyModifiers::CONTROL));
    assert_eq!(
        app.terminal.position,
        TerminalPosition::Right,
        "went to the program"
    );
}

// ---- mouse, grow / shrink -----------------------------------------------------------------

/// Dragging the separator sets the width; the editor and the panel keep their minimums.
#[test]
fn dragging_the_separator_resizes_the_panel() {
    let (_d, mut app) = right_app();
    app.focus = Focus::Terminal;
    draw(&mut app, 120, 40);
    let sep = app.layout.terminal_sep;
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        sep.x,
        sep.y + 5,
    );
    mouse(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        59,
        sep.y + 7,
    );
    mouse(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        59,
        sep.y + 7,
    );
    assert_eq!(app.terminal.width_percent, 50);
    draw(&mut app, 120, 40);
    assert_eq!(app.layout.terminal_sep.x, 59);
    assert_eq!(app.layout.terminal.width, 60);
    // Far left: the editor keeps 20 columns; far right: the panel keeps 20.
    let sep = app.layout.terminal_sep;
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        sep.x,
        sep.y,
    );
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), 2, sep.y);
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), 2, sep.y);
    draw(&mut app, 120, 40);
    assert_eq!(app.layout.editor.width, EDITOR_MIN_COLS, "{:?}", app.layout);
    let sep = app.layout.terminal_sep;
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        sep.x,
        sep.y,
    );
    mouse(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        119,
        sep.y,
    );
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), 119, sep.y);
    draw(&mut app, 120, 40);
    assert_eq!(
        app.layout.terminal.width, TERMINAL_MIN_COLS,
        "{:?}",
        app.layout
    );
    // Presses on the separator leave the focus where it was.
    assert_eq!(app.focus, Focus::Terminal);
}

/// On the right a press on the panel's tab bar (outside the tabs) does not start a height
/// drag; the editor still gets clicks left of the separator.
#[test]
fn the_tab_bar_is_not_a_drag_handle_on_the_right() {
    let (_d, mut app) = right_app();
    draw(&mut app, 120, 40);
    let a = app.layout.terminal;
    let before = (app.terminal.height_percent, app.terminal.width_percent);
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        a.x + a.width - 1,
        a.y,
    );
    mouse(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        a.x + 2,
        a.y + 10,
    );
    mouse(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        a.x + 2,
        a.y + 10,
    );
    assert_eq!(
        (app.terminal.height_percent, app.terminal.width_percent),
        before
    );
    app.focus = Focus::Terminal;
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 5, 5);
    assert_eq!(app.focus, Focus::Editor);
}

/// grow / shrink (ctrl+f8 / ctrl+f9) change the width on the right and the height at the
/// bottom, including the narrow fallback.
#[test]
fn grow_and_shrink_change_the_width_on_the_right() {
    let (_d, mut app) = right_app();
    draw(&mut app, 120, 40);
    crate::app_tests::press(&mut app, "ctrl+f8");
    assert_eq!(
        (app.terminal.width_percent, app.terminal.height_percent),
        (45, 40)
    );
    draw(&mut app, 120, 40);
    assert_eq!(app.layout.terminal.width, 54);
    crate::app_tests::press(&mut app, "ctrl+f9");
    crate::app_tests::press(&mut app, "ctrl+f9");
    assert_eq!(
        (app.terminal.width_percent, app.terminal.height_percent),
        (35, 40)
    );
    for _ in 0..20 {
        app.run_command("terminal.grow", CmdArgs::None);
    }
    // The stored share stops at what is drawn, so one shrink changes the panel at once.
    draw(&mut app, 80, 24);
    for _ in 0..20 {
        app.run_command("terminal.grow", CmdArgs::None);
    }
    draw(&mut app, 80, 24);
    let widest = app.layout.terminal.width;
    assert_eq!(widest, layout::right_cols(app.layout.tabs.width, 90));
    assert!(
        app.terminal.width_percent < 90,
        "{}",
        app.terminal.width_percent
    );
    app.run_command("terminal.shrink", CmdArgs::None);
    draw(&mut app, 80, 24);
    assert!(app.layout.terminal.width < widest);
    // Same at the bottom: grown past the screen, one shrink shows at once.
    draw(&mut app, 50, 20);
    for _ in 0..20 {
        app.run_command("terminal.grow", CmdArgs::None);
    }
    draw(&mut app, 50, 20);
    let tallest = app.layout.terminal.height;
    app.run_command("terminal.shrink", CmdArgs::None);
    draw(&mut app, 50, 20);
    assert!(app.layout.terminal.height < tallest);
}

/// On a wide column one percent is several cells: a drag keeps the separator on the mouse.
#[test]
fn dragging_follows_the_mouse_column_on_a_wide_editor() {
    let (_d, mut app) = right_app();
    app.focus = Focus::Terminal;
    draw(&mut app, 220, 40);
    let sep = app.layout.terminal_sep;
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        sep.x,
        sep.y,
    );
    for col in [150u16, 151, 152, 153, 160] {
        mouse(
            &mut app,
            MouseEventKind::Drag(MouseButton::Left),
            col,
            sep.y,
        );
        draw(&mut app, 220, 40);
        assert_eq!(app.layout.terminal_sep.x, col);
    }
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), 160, sep.y);
    draw(&mut app, 220, 40);
    assert_eq!(app.layout.terminal_sep.x, 160);
    // grow / shrink go back to percents.
    app.run_command("terminal.grow", CmdArgs::None);
    draw(&mut app, 220, 40);
    assert!(app.terminal.width_cols.is_none());
    assert!(app.layout.terminal.width > 220 - 161);
}

// ---- PTY, snapshots -----------------------------------------------------------------------

/// The PTY gets the panel's grid size on the right, after a drag and after going back to the
/// bottom.
#[test]
fn the_pty_follows_every_layout_change() {
    let (_d, mut app, _) = term_app();
    app.keyboard_enhanced = true;
    right(&mut app);
    draw(&mut app, 120, 40);
    ctrl_backtick(&mut app);
    let check = |app: &mut App, what: &str| {
        draw(app, 120, 40);
        let a = app.layout.terminal;
        assert_eq!(tab(app).size(), (a.width, a.height - 1), "{what}");
        typ(app, "clear; stty size\n");
        wait_text(app, &format!("{} {}", a.height - 1, a.width));
    };
    check(&mut app, "right");
    assert_eq!(tab(&app).size(), (48, 37));
    let sep = app.layout.terminal_sep;
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        sep.x,
        sep.y,
    );
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), 59, sep.y);
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), 59, sep.y);
    app.focus = Focus::Terminal;
    check(&mut app, "dragged");
    assert_eq!(tab(&app).size(), (60, 37));
    app.run_command("terminal.toggle_position", CmdArgs::None);
    app.focus = Focus::Terminal;
    check(&mut app, "bottom");
    assert_eq!(tab(&app).size(), (120, 15));
}

/// A terminal started while the position is right starts at the right panel's size.
#[test]
fn a_new_terminal_starts_at_the_right_panel_size() {
    let (_d, mut app, _) = term_app();
    right(&mut app);
    draw(&mut app, 120, 40);
    app.run_command("terminal.new", CmdArgs::None);
    assert_eq!(tab(&app).size(), (48, 37));
}

#[test]
fn render_terminal_panel_right() {
    for (w, h) in [(120, 40), (80, 24)] {
        let (_d, mut app) = snapshot_app();
        right(&mut app);
        let t = draw(&mut app, w, h);
        insta::assert_snapshot!(format!("render_terminal_panel_right_{w}x{h}"), t.backend());
    }
    // The editor focused: the separator is dim, the panel hint names the focus key.
    let (_d, mut app) = snapshot_app();
    right(&mut app);
    app.focus = Focus::Editor;
    let t = draw(&mut app, 80, 24);
    insta::assert_snapshot!("render_terminal_panel_right_editor_80x24", t.backend());
}

/// The welcome screen of an empty project fits the narrowed editor without spilling into
/// the panel.
#[test]
fn nothing_is_drawn_over_the_panel() {
    let (mut app, _) = test_app(std::path::Path::new("/home/user/DEV/shop"));
    app.terminal.tabs.push(fake_tab("sh", TermStatus::Running));
    app.terminal.visible = true;
    right(&mut app);
    let t = draw(&mut app, 80, 24);
    let a = app.layout.terminal;
    for y in a.y + 1..a.y + a.height {
        let row: String = row_text(&t, y).chars().skip(usize::from(a.x)).collect();
        assert!(!row.contains("Explorer"), "row {y}: {row}");
    }
}
