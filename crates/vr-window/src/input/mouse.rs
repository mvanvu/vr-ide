//! Pointer events in physical pixels → crossterm mouse events on the cell grid.

use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use winit::event::{ElementState, MouseButton as WinitButton, MouseScrollDelta};
use winit::keyboard::ModifiersState;

use super::key::modifiers;
use crate::GridLayout;

/// Most wheel steps sent for one winit event; a bogus delta must not flood the app.
const MAX_STEPS: u32 = 100;

/// The cell under a pixel (physical px, may lie outside the window while dragging), clamped to
/// the grid: padding and anything beyond the edges map to the nearest edge cell.
#[must_use]
pub fn cell_at_clamped(layout: &GridLayout, x: f64, y: f64) -> (u16, u16) {
    let axis = |pos: f64, origin: u32, step: u32, count: u16| -> u16 {
        let offset = (pos - f64::from(origin)) / f64::from(step.max(1));
        if offset.is_nan() || offset < 0.0 {
            0
        } else {
            // `as` saturates, so +inf and huge offsets land on the last cell too.
            (offset.floor() as u16).min(count.saturating_sub(1))
        }
    };
    (
        axis(x, layout.origin_x, layout.cell.width, layout.cols),
        axis(y, layout.origin_y, layout.cell.height, layout.rows),
    )
}

/// Pointer state between winit events: position, held buttons and the wheel remainder.
#[derive(Debug, Default, Clone)]
pub struct MouseInput {
    cell: Option<(u16, u16)>,
    /// Held buttons in press order.
    held: Vec<MouseButton>,
    /// Wheel motion not yet sent, in lines (x, y).
    wheel: (f64, f64),
}

impl MouseInput {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The cell under the pointer, once it has moved over the window.
    #[must_use]
    pub fn cell(&self) -> Option<(u16, u16)> {
        self.cell
    }

    /// `CursorMoved`: a `Moved` or `Drag` event when the pointer enters another cell.
    pub fn moved(
        &mut self,
        layout: &GridLayout,
        x: f64,
        y: f64,
        mods: ModifiersState,
    ) -> Option<MouseEvent> {
        let cell = cell_at_clamped(layout, x, y);
        if self.cell == Some(cell) {
            return None;
        }
        self.cell = Some(cell);
        let kind = match self.held.first() {
            Some(&button) => MouseEventKind::Drag(button),
            None => MouseEventKind::Moved,
        };
        Some(event(kind, cell, mods))
    }

    /// `MouseInput`: `Down` / `Up` at the pointer's cell. Buttons crossterm has no name for
    /// (back, forward, others) and clicks before the pointer was ever seen give nothing.
    pub fn button(
        &mut self,
        button: WinitButton,
        state: ElementState,
        mods: ModifiersState,
    ) -> Option<MouseEvent> {
        let button = match button {
            WinitButton::Left => MouseButton::Left,
            WinitButton::Right => MouseButton::Right,
            WinitButton::Middle => MouseButton::Middle,
            WinitButton::Back | WinitButton::Forward | WinitButton::Other(_) => return None,
        };
        self.held.retain(|&b| b != button);
        let kind = match state {
            ElementState::Pressed => {
                self.held.push(button);
                MouseEventKind::Down(button)
            }
            ElementState::Released => MouseEventKind::Up(button),
        };
        Some(event(kind, self.cell?, mods))
    }

    /// The window lost focus: buttons released elsewhere never reach it, so none count as held.
    pub fn focus_lost(&mut self) {
        self.held.clear();
        self.wheel = (0.0, 0.0);
    }

    /// `MouseWheel`: whole line steps, pixel deltas counted in cells (a cell's height is one
    /// line, its width one column). The remainder is kept for the next event and dropped when
    /// the direction turns. Positive y scrolls up, positive x scrolls left (winit's convention:
    /// the content moves down / right).
    pub fn wheel(
        &mut self,
        layout: &GridLayout,
        delta: MouseScrollDelta,
        mods: ModifiersState,
    ) -> Vec<MouseEvent> {
        let (dx, dy) = match delta {
            MouseScrollDelta::LineDelta(x, y) => (f64::from(x), f64::from(y)),
            MouseScrollDelta::PixelDelta(p) => (
                p.x / f64::from(layout.cell.width.max(1)),
                p.y / f64::from(layout.cell.height.max(1)),
            ),
        };
        let (sx, rx) = steps(self.wheel.0, dx);
        let (sy, ry) = steps(self.wheel.1, dy);
        self.wheel = (rx, ry);
        let Some(cell) = self.cell else {
            return Vec::new();
        };
        let repeat = |n: i32, pos: MouseEventKind, neg: MouseEventKind| {
            let kind = if n > 0 { pos } else { neg };
            std::iter::repeat_n(event(kind, cell, mods), n.unsigned_abs() as usize)
        };
        repeat(sy, MouseEventKind::ScrollUp, MouseEventKind::ScrollDown)
            .chain(repeat(
                sx,
                MouseEventKind::ScrollLeft,
                MouseEventKind::ScrollRight,
            ))
            .collect()
    }
}

/// Whole steps in `remainder + delta` and what is left over.
fn steps(remainder: f64, delta: f64) -> (i32, f64) {
    if !delta.is_finite() {
        return (0, remainder);
    }
    let start = if remainder * delta < 0.0 {
        0.0
    } else {
        remainder
    };
    let total = start + delta;
    let whole = total.trunc();
    let capped = whole.clamp(-f64::from(MAX_STEPS), f64::from(MAX_STEPS));
    let rest = if whole == capped { total - whole } else { 0.0 };
    (capped as i32, rest)
}

fn event(kind: MouseEventKind, (column, row): (u16, u16), mods: ModifiersState) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: modifiers(mods),
    }
}

#[cfg(test)]
mod tests;
