use super::*;
use crate::CellMetrics;
use crossterm::event::KeyModifiers;
use winit::dpi::PhysicalPosition;

const NONE: ModifiersState = ModifiersState::empty();

/// 80x24 cells of 10x20 px behind 8 px of padding.
fn layout() -> GridLayout {
    GridLayout {
        cols: 80,
        rows: 24,
        origin_x: 8,
        origin_y: 8,
        cell: CellMetrics {
            width: 10,
            height: 20,
            baseline: 15,
            underline: 17,
            strike: 10,
            line: 1,
        },
    }
}

#[test]
fn pixels_map_to_cells_clamped_to_the_grid() {
    let l = layout();
    let table: [((f64, f64), (u16, u16)); 14] = [
        ((8.0, 8.0), (0, 0)),
        ((17.9, 27.9), (0, 0)),
        ((18.0, 28.0), (1, 1)),
        (
            (8.0 + 79.0 * 10.0 + 9.5, 8.0 + 23.0 * 20.0 + 19.5),
            (79, 23),
        ),
        // Padding edge maps to the edge cell.
        ((0.0, 0.0), (0, 0)),
        ((7.9, 100.0), (0, 4)),
        ((8.0 + 800.0, 8.0 + 480.0), (79, 23)),
        ((8.0 + 800.0 + 7.0, 10.0), (79, 0)),
        // Outside the window while dragging.
        ((-50.0, -3.0), (0, 0)),
        ((5000.0, 5000.0), (79, 23)),
        ((1e12, -1e12), (79, 0)),
        ((f64::INFINITY, f64::NEG_INFINITY), (79, 0)),
        ((f64::NAN, f64::NAN), (0, 0)),
        ((413.0, 233.0), (40, 11)),
    ];
    for ((x, y), want) in table {
        assert_eq!(cell_at_clamped(&l, x, y), want, "pixel ({x}, {y})");
    }
}

#[test]
fn a_degenerate_layout_never_panics() {
    let mut l = layout();
    l.cols = 0;
    l.rows = 0;
    l.cell.width = 0;
    l.cell.height = 0;
    assert_eq!(cell_at_clamped(&l, 100.0, 100.0), (0, 0));
}

#[test]
fn motion_reports_only_cell_changes_and_drag_while_a_button_is_held() {
    let l = layout();
    let mut m = MouseInput::new();
    assert_eq!(
        m.button(WinitButton::Left, ElementState::Pressed, NONE),
        None,
        "no position yet"
    );
    m.focus_lost();

    let ev = m.moved(&l, 20.0, 30.0, NONE).unwrap();
    assert_eq!((ev.kind, ev.column, ev.row), (MouseEventKind::Moved, 1, 1));
    assert_eq!(m.moved(&l, 25.0, 35.0, NONE), None, "same cell");

    let down = m
        .button(
            WinitButton::Left,
            ElementState::Pressed,
            ModifiersState::SHIFT,
        )
        .unwrap();
    assert_eq!(
        (down.kind, down.column, down.row),
        (MouseEventKind::Down(MouseButton::Left), 1, 1)
    );
    assert_eq!(down.modifiers, KeyModifiers::SHIFT);

    let drag = m.moved(&l, -40.0, 30.0, ModifiersState::CONTROL).unwrap();
    assert_eq!(
        (drag.kind, drag.column, drag.row),
        (MouseEventKind::Drag(MouseButton::Left), 0, 1)
    );
    assert_eq!(drag.modifiers, KeyModifiers::CONTROL);

    let up = m
        .button(WinitButton::Left, ElementState::Released, NONE)
        .unwrap();
    assert_eq!(
        (up.kind, up.column, up.row),
        (MouseEventKind::Up(MouseButton::Left), 0, 1)
    );
    assert_eq!(
        m.moved(&l, 40.0, 30.0, NONE).unwrap().kind,
        MouseEventKind::Moved
    );

    // Right and middle buttons; back / forward have no crossterm name.
    let right = m
        .button(WinitButton::Right, ElementState::Pressed, NONE)
        .unwrap();
    assert_eq!(right.kind, MouseEventKind::Down(MouseButton::Right));
    assert_eq!(
        m.moved(&l, 60.0, 30.0, NONE).unwrap().kind,
        MouseEventKind::Drag(MouseButton::Right)
    );
    assert_eq!(
        m.button(WinitButton::Back, ElementState::Pressed, NONE),
        None
    );
    assert_eq!(
        m.button(WinitButton::Middle, ElementState::Pressed, NONE)
            .unwrap()
            .kind,
        MouseEventKind::Down(MouseButton::Middle)
    );
    m.focus_lost();
    assert_eq!(
        m.moved(&l, 80.0, 30.0, NONE).unwrap().kind,
        MouseEventKind::Moved,
        "focus loss drops held buttons"
    );
}

fn kinds(events: &[MouseEvent]) -> Vec<MouseEventKind> {
    events.iter().map(|e| e.kind).collect()
}

#[test]
fn wheel_lines_follow_crossterm_directions() {
    let l = layout();
    let mut m = MouseInput::new();
    m.moved(&l, 20.0, 30.0, NONE);
    let up = m.wheel(
        &l,
        MouseScrollDelta::LineDelta(0.0, 1.0),
        ModifiersState::CONTROL,
    );
    assert_eq!(kinds(&up), [MouseEventKind::ScrollUp]);
    assert_eq!(
        (up[0].column, up[0].row, up[0].modifiers),
        (1, 1, KeyModifiers::CONTROL)
    );
    assert_eq!(
        kinds(&m.wheel(&l, MouseScrollDelta::LineDelta(0.0, -3.0), NONE)),
        [MouseEventKind::ScrollDown; 3]
    );
    assert_eq!(
        kinds(&m.wheel(&l, MouseScrollDelta::LineDelta(1.0, 0.0), NONE)),
        [MouseEventKind::ScrollLeft]
    );
    assert_eq!(
        kinds(&m.wheel(&l, MouseScrollDelta::LineDelta(-2.0, 0.0), NONE)),
        [MouseEventKind::ScrollRight; 2]
    );
}

#[test]
fn small_pixel_deltas_accumulate_into_lines() {
    let l = layout();
    let mut m = MouseInput::new();
    m.moved(&l, 20.0, 30.0, NONE);
    let px = |x, y| MouseScrollDelta::PixelDelta(PhysicalPosition::new(x, y));
    // 7 px steps on a 20 px line: lines at 21 px and 42 px.
    let mut sent = Vec::new();
    for _ in 0..6 {
        sent.push(m.wheel(&l, px(0.0, 7.0), NONE).len());
    }
    assert_eq!(sent, [0, 0, 1, 0, 0, 1]);
    // Turning around drops the remainder instead of eating the first lines back.
    assert!(m.wheel(&l, px(0.0, 5.0), NONE).is_empty());
    assert!(m.wheel(&l, px(0.0, -19.0), NONE).is_empty());
    assert_eq!(
        kinds(&m.wheel(&l, px(0.0, -1.0), NONE)),
        [MouseEventKind::ScrollDown]
    );
    // Horizontal pixels count in cell widths; fractional line deltas accumulate too.
    assert_eq!(
        kinds(&m.wheel(&l, px(-25.0, 0.0), NONE)),
        [MouseEventKind::ScrollRight; 2]
    );
    assert!(
        m.wheel(&l, MouseScrollDelta::LineDelta(0.0, 0.5), NONE)
            .is_empty()
    );
    assert_eq!(
        kinds(&m.wheel(&l, MouseScrollDelta::LineDelta(0.0, 0.5), NONE)),
        [MouseEventKind::ScrollUp]
    );
}

#[test]
fn bogus_wheel_deltas_are_bounded() {
    let l = layout();
    let mut m = MouseInput::new();
    assert!(
        m.wheel(&l, MouseScrollDelta::LineDelta(0.0, 2.0), NONE)
            .is_empty(),
        "no position yet"
    );
    m.moved(&l, 20.0, 30.0, NONE);
    assert_eq!(
        m.wheel(&l, MouseScrollDelta::LineDelta(0.0, 1e9), NONE)
            .len(),
        MAX_STEPS as usize
    );
    assert!(
        m.wheel(
            &l,
            MouseScrollDelta::LineDelta(f32::NAN, f32::INFINITY),
            NONE
        )
        .is_empty()
    );
    assert_eq!(
        m.wheel(&l, MouseScrollDelta::LineDelta(0.0, 1.0), NONE)
            .len(),
        1
    );
}
