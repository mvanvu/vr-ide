//! Copies the emulator's screen into vr-term's own [`Grid`].

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell as TermCell, Flags};
use alacritty_terminal::term::{Term, TermMode};
use alacritty_terminal::vte::ansi::{Color as TermColor, CursorShape as TermShape, NamedColor};

use crate::types::{Cell, CellFlags, Color, Cursor, CursorShape, Grid, Modes};

/// The emulator's cells copied as they are: taken under the emulator lock, converted after it is
/// released so the reader is held up only for a plain copy.
pub(crate) struct Capture {
    raw: Vec<TermCell>,
    grid: Grid,
}

impl Capture {
    pub(crate) fn into_grid(self, title: Option<String>) -> Grid {
        let mut grid = self.grid;
        grid.cells = self.raw.iter().map(cell).collect();
        grid.title = title;
        grid
    }
}

pub(crate) fn capture<T: EventListener>(term: &Term<T>, scroll_offset: usize) -> Capture {
    let g = term.grid();
    let rows = g.screen_lines();
    let cols = g.columns();
    let history = g.history_size();
    let offset = scroll_offset.min(history);

    let mut raw = Vec::with_capacity(rows * cols);
    for r in 0..rows {
        let row = &g[Line(r as i32 - offset as i32)];
        raw.extend((0..cols).map(|c| row[Column(c)].clone()));
    }

    let content = term.renderable_content();
    let point = content.cursor.point;
    let row = point.line.0 + offset as i32;
    let shape = match content.cursor.shape {
        TermShape::Block | TermShape::Hidden => CursorShape::Block,
        TermShape::Underline => CursorShape::Underline,
        TermShape::Beam => CursorShape::Beam,
        TermShape::HollowBlock => CursorShape::HollowBlock,
    };
    let visible = content.cursor.shape != TermShape::Hidden && row >= 0 && (row as usize) < rows;
    let cursor = Cursor {
        row: row.max(0) as usize,
        col: point.column.0,
        shape,
        visible,
    };

    let grid = Grid {
        cols,
        rows,
        cells: Vec::new(),
        cursor,
        title: None,
        modes: modes(*term.mode()),
        scrollback_len: history,
        scroll_offset: offset,
    };
    Capture { raw, grid }
}

pub(crate) fn modes(mode: TermMode) -> Modes {
    Modes {
        alt_screen: mode.contains(TermMode::ALT_SCREEN),
        mouse_reporting: mode.intersects(TermMode::MOUSE_MODE),
        mouse_drag: mode.contains(TermMode::MOUSE_DRAG),
        mouse_motion: mode.contains(TermMode::MOUSE_MOTION),
        sgr_mouse: mode.contains(TermMode::SGR_MOUSE),
        bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
        app_cursor: mode.contains(TermMode::APP_CURSOR),
        app_keypad: mode.contains(TermMode::APP_KEYPAD),
    }
}

fn cell(c: &TermCell) -> Cell {
    let f = c.flags;
    let wide_spacer = f.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER);
    let ch = if wide_spacer {
        String::new()
    } else {
        let mut s = String::from(c.c);
        if let Some(marks) = c.zerowidth() {
            s.extend(marks);
        }
        s
    };
    Cell {
        ch,
        fg: color(c.fg),
        bg: color(c.bg),
        flags: CellFlags {
            bold: f.contains(Flags::BOLD),
            italic: f.contains(Flags::ITALIC),
            underline: f.intersects(Flags::ALL_UNDERLINES),
            inverse: f.contains(Flags::INVERSE),
            dim: f.contains(Flags::DIM),
            hidden: f.contains(Flags::HIDDEN),
            strikeout: f.contains(Flags::STRIKEOUT),
            wide: f.contains(Flags::WIDE_CHAR),
            wide_spacer,
        },
    }
}

fn color(c: TermColor) -> Color {
    match c {
        TermColor::Spec(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
        TermColor::Indexed(i) => Color::Indexed(i),
        TermColor::Named(n) => {
            let n = n as usize;
            if n < 16 {
                Color::Indexed(n as u8)
            } else if (NamedColor::DimBlack as usize..=NamedColor::DimWhite as usize).contains(&n) {
                // Dim variants are the base colour; the DIM flag carries the dimming.
                Color::Indexed((n - NamedColor::DimBlack as usize) as u8)
            } else {
                Color::Default
            }
        }
    }
}
