//! FR-19 (AC-F3): terminal tabs reorder by a mouse drag along the panel's tab bar, at the
//! bottom and on the right, with overflowing tabs; a press without movement still switches, the
//! `+` and `x` keep working and a press on the empty bar still resizes a bottom panel.

use std::path::Path;

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use vr_config::TerminalPosition;
use vr_term::TermStatus;

use crate::app::{App, Focus};
use crate::app_tests::test_app;
use crate::commands::CmdArgs;
use crate::terminal_tests::{draw, fake_tab, row_text};
use crate::ui::terminal_view::tab_bar;

/// A shown panel with fake terminals titled `titles`, the first one active and focused.
fn tabs_app(titles: &[&str]) -> App {
    let (mut app, _) = test_app(Path::new("/home/user/DEV/shop"));
    for t in titles {
        app.terminal.tabs.push(fake_tab(t, TermStatus::Running));
    }
    app.terminal.visible = true;
    app.focus = Focus::Terminal;
    app
}

fn titles(app: &App) -> Vec<String> {
    app.terminal.tabs.iter().map(|t| t.title()).collect()
}

fn active_title(app: &App) -> String {
    app.terminal.tabs[app.terminal.active].title()
}

fn mouse(app: &mut App, kind: MouseEventKind, column: u16, row: u16) {
    app.handle_mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    });
}

/// Screen column of the middle of terminal `i`'s label as the bar is laid out now.
fn label_col(app: &App, i: usize) -> u16 {
    let bar = tab_bar(app, app.layout.terminal.width);
    let t = bar
        .tabs
        .iter()
        .find(|t| t.index == i)
        .unwrap_or_else(|| panic!("tab {i} is not drawn"));
    app.layout.terminal.x + t.x + 2
}

/// Press at `from`, move cell by cell to `to` on row `row` (as terminals report a drag), release.
fn drag(app: &mut App, from: u16, to: u16, row: u16) {
    mouse(app, MouseEventKind::Down(MouseButton::Left), from, row);
    let mut x = from;
    while x != to {
        x = if to > x { x + 1 } else { x - 1 };
        mouse(app, MouseEventKind::Drag(MouseButton::Left), x, row);
    }
    mouse(app, MouseEventKind::Up(MouseButton::Left), to, row);
}

#[test]
fn dragging_a_tab_right_moves_it_there_and_keeps_it_active() {
    let mut app = tabs_app(&["one", "two", "three"]);
    draw(&mut app, 80, 24);
    let height = app.terminal.height_percent;
    let y = app.layout.terminal.y;
    let (from, to) = (label_col(&app, 0), label_col(&app, 2));
    drag(&mut app, from, to, y);
    assert_eq!(titles(&app), ["two", "three", "one"]);
    assert_eq!(active_title(&app), "one");
    assert_eq!(app.terminal.active, 2);
    // A drag on a label moves the tab; it does not resize the panel.
    assert_eq!(app.terminal.height_percent, height);
    // The numbers follow the new order.
    let t = draw(&mut app, 80, 24);
    let bar = row_text(&t, app.layout.terminal.y);
    assert!(bar.starts_with(" 1 two x  2 three x  3 one x "), "{bar}");
}

#[test]
fn dragging_a_tab_left_moves_it_there() {
    let mut app = tabs_app(&["one", "two", "three"]);
    app.terminal.active = 2;
    draw(&mut app, 80, 24);
    let y = app.layout.terminal.y;
    let (from, to) = (label_col(&app, 2), label_col(&app, 0));
    drag(&mut app, from, to, y);
    assert_eq!(titles(&app), ["three", "one", "two"]);
    assert_eq!(active_title(&app), "three");
    assert_eq!(app.terminal.active, 0);
}

#[test]
fn a_press_and_release_without_movement_only_switches() {
    let mut app = tabs_app(&["one", "two", "three"]);
    draw(&mut app, 80, 24);
    let y = app.layout.terminal.y;
    let x = label_col(&app, 1);
    drag(&mut app, x, x, y);
    assert_eq!(titles(&app), ["one", "two", "three"]);
    assert_eq!(app.terminal.active, 1);
    // A drag that stays on the pressed tab moves nothing either.
    let x = label_col(&app, 2);
    drag(&mut app, x, x + 3, y);
    assert_eq!(titles(&app), ["one", "two", "three"]);
    assert_eq!(app.terminal.active, 2);
}

#[test]
fn f8_and_shift_f8_follow_the_new_order() {
    let mut app = tabs_app(&["one", "two", "three"]);
    draw(&mut app, 80, 24);
    let y = app.layout.terminal.y;
    let (from, to) = (label_col(&app, 0), label_col(&app, 1));
    drag(&mut app, from, to, y);
    assert_eq!(titles(&app), ["two", "one", "three"]);
    assert_eq!(active_title(&app), "one");
    app.run_command("terminal.next", CmdArgs::None);
    assert_eq!(active_title(&app), "three");
    app.run_command("terminal.next", CmdArgs::None);
    assert_eq!(active_title(&app), "two");
    app.run_command("terminal.prev", CmdArgs::None);
    assert_eq!(active_title(&app), "three");
    // The status bar counts in the new order.
    app.run_command("terminal.prev", CmdArgs::None);
    let t = draw(&mut app, 80, 24);
    assert!(row_text(&t, 23).contains(" 2/3 "), "{}", row_text(&t, 23));
}

/// Moving a wide label over a narrow one does not swap it back on the next cell: the tab only
/// moves once the mouse would lie on it at its new place.
#[test]
fn a_wide_tab_over_a_narrow_one_does_not_flip_back() {
    let mut app = tabs_app(&["a-rather-long-title", "b", "c"]);
    draw(&mut app, 100, 24);
    let y = app.layout.terminal.y;
    let start = label_col(&app, 0);
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), start, y);
    let mut seen = Vec::new();
    let end = label_col(&app, 2) + 1;
    for x in start + 1..=end {
        mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), x, y);
        let a = app
            .terminal
            .tabs
            .iter()
            .position(|t| t.title() == "a-rather-long-title")
            .unwrap();
        if seen.last() != Some(&a) {
            seen.push(a);
        }
    }
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), end, y);
    // It only ever moves right, one place at a time.
    assert!(seen.windows(2).all(|w| w[1] == w[0] + 1), "{seen:?}");
    assert_eq!(seen.last(), Some(&2), "{seen:?}");
    assert_eq!(active_title(&app), "a-rather-long-title");
}

#[test]
fn a_drag_released_outside_the_panel_ends_the_move() {
    let mut app = tabs_app(&["one", "two", "three"]);
    draw(&mut app, 80, 24);
    let y = app.layout.terminal.y;
    let from = label_col(&app, 0);
    let to = label_col(&app, 1);
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), from, y);
    for x in from + 1..=to {
        mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), x, y);
    }
    // Up over the editor: the move stays, the drag is over.
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), to, 3);
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), to, 3);
    assert_eq!(titles(&app), ["two", "one", "three"]);
    let before = titles(&app);
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), 2, y);
    assert_eq!(
        titles(&app),
        before,
        "a stray drag after the release moved a tab"
    );
    // The editor gets the next click.
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 5, 3);
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), 5, 3);
    assert_ne!(app.focus, Focus::Terminal);
}

#[test]
fn the_empty_bar_still_resizes_and_plus_and_x_still_work() {
    let mut app = tabs_app(&["one", "two", "three"]);
    // Exited, so its 'x' closes it without asking (a fake terminal has no foreground to tell).
    app.terminal.tabs[0] = fake_tab("one", TermStatus::Exited(0));
    draw(&mut app, 80, 24);
    let area = app.layout.terminal;
    let height = app.terminal.height_percent;
    // Empty part of the bar (left of the hint), dragged up: a resize.
    let bar = tab_bar(&app, area.width);
    let empty = area.x + bar.plus.unwrap() + crate::ui::terminal_view::PLUS_WIDTH + 2;
    mouse(
        &mut app,
        MouseEventKind::Down(MouseButton::Left),
        empty,
        area.y,
    );
    mouse(
        &mut app,
        MouseEventKind::Drag(MouseButton::Left),
        empty,
        area.y - 4,
    );
    mouse(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        empty,
        area.y - 4,
    );
    assert!(app.terminal.height_percent > height);
    assert_eq!(titles(&app), ["one", "two", "three"]);
    draw(&mut app, 80, 24);
    // After a move the 'x' of the moved tab closes that terminal.
    let y = app.layout.terminal.y;
    let (from, to) = (label_col(&app, 0), label_col(&app, 2));
    drag(&mut app, from, to, y);
    assert_eq!(titles(&app), ["two", "three", "one"]);
    draw(&mut app, 80, 24);
    let bar = tab_bar(&app, app.layout.terminal.width);
    let t = bar.tabs.iter().find(|t| t.index == 2).unwrap();
    let x = app.layout.terminal.x + t.x + t.close.unwrap();
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), x, y);
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), x, y);
    assert_eq!(titles(&app), ["two", "three"]);
}

#[test]
fn tabs_reorder_on_the_right_too() {
    let mut app = tabs_app(&["one", "two", "three"]);
    app.terminal.position = TerminalPosition::Right;
    draw(&mut app, 160, 40);
    assert!(app.layout.terminal_sep.width > 0, "panel is on the right");
    let before = (app.terminal.height_percent, app.terminal.width_percent);
    let y = app.layout.terminal.y;
    let (from, to) = (label_col(&app, 0), label_col(&app, 2));
    drag(&mut app, from, to, y);
    assert_eq!(titles(&app), ["two", "three", "one"]);
    assert_eq!(active_title(&app), "one");
    let (from, to) = (label_col(&app, 2), label_col(&app, 1));
    drag(&mut app, from, to, y);
    assert_eq!(titles(&app), ["two", "one", "three"]);
    assert_eq!(
        (app.terminal.height_percent, app.terminal.width_percent),
        before
    );
}

/// With more tabs than fit, the active one stays in view behind `+N`: dragging it onto the
/// marker carries it past the hidden tabs one place per cell, dragging it left past the first
/// drawn tab carries it back.
#[test]
fn an_overflowing_bar_carries_the_tab_past_hidden_ones() {
    let names: Vec<String> = (1..=6)
        .map(|i| format!("user@host: ~/DEV/project-{i}"))
        .collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut app = tabs_app(&refs);
    draw(&mut app, 50, 24);
    let bar = tab_bar(&app, app.layout.terminal.width);
    assert!(bar.more.is_some(), "the bar overflows at 50 columns");
    let y = app.layout.terminal.y;
    let first = names[0].clone();
    let from = label_col(&app, 0);
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), from, y);
    // Onto the `+N` marker and a few cells along it.
    let bar = tab_bar(&app, app.layout.terminal.width);
    let last = bar.tabs.last().unwrap();
    let marker = app.layout.terminal.x + last.x + last.width;
    for x in from + 1..=marker + 8 {
        mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), x, y);
        draw(&mut app, 50, 24);
    }
    mouse(
        &mut app,
        MouseEventKind::Up(MouseButton::Left),
        marker + 8,
        y,
    );
    assert_eq!(app.terminal.tabs.last().unwrap().title(), first);
    assert_eq!(active_title(&app), first);
    // Back to the front: drag left from the tab to the bar's first column, a cell at a time.
    draw(&mut app, 50, 24);
    let from = label_col(&app, 5);
    let x0 = app.layout.terminal.x;
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), from, y);
    let mut x = from;
    let mut moves = 0;
    while app.terminal.active > 0 && moves < 200 {
        x = if x > x0 { x - 1 } else { x0 };
        mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), x, y);
        draw(&mut app, 50, 24);
        moves += 1;
    }
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), x, y);
    assert_eq!(app.terminal.tabs[0].title(), first, "{:?}", titles(&app));
    assert_eq!(active_title(&app), first);
}

/// A narrow panel on the right draws only the active tab, with the others hidden before it:
/// dragging it left onto the panel's edge and over the editor walks it toward the front, one
/// place per cell, as `+N` does to the right.
#[test]
fn a_lone_drawn_tab_walks_left_past_hidden_ones() {
    let names: Vec<String> = (1..=6)
        .map(|i| format!("user@host: ~/DEV/a-much-longer-title-{i}"))
        .collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let mut app = tabs_app(&refs);
    app.terminal.position = TerminalPosition::Right;
    app.terminal.active = 5;
    draw(&mut app, 80, 24);
    assert!(app.layout.terminal_sep.width > 0, "panel is on the right");
    let bar = tab_bar(&app, app.layout.terminal.width);
    assert_eq!(bar.tabs.len(), 1, "one tab drawn");
    assert!(bar.more.is_some(), "the others are hidden");
    let y = app.layout.terminal.y;
    let from = label_col(&app, 5);
    let edge = app.layout.terminal.x;
    // Onto the label's number without leaving the panel: one place toward the front.
    drag(&mut app, from, edge + 1, y);
    assert_eq!(app.terminal.active, 4, "{:?}", titles(&app));
    assert_eq!(active_title(&app), names[5]);
    draw(&mut app, 80, 24);
    let from = label_col(&app, 4);
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), from, y);
    let mut x = from;
    while x > edge - 8 {
        x -= 1;
        mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), x, y);
        draw(&mut app, 80, 24);
    }
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), x, y);
    assert_eq!(active_title(&app), names[5]);
    assert_eq!(app.terminal.active, 0, "{:?}", titles(&app));
    // The others keep their order.
    assert_eq!(titles(&app)[1..], names[..5]);
}
