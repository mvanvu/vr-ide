//! `PixelBackend`: a ratatui backend that paints cells into a [`Canvas`].
//!
//! Only the cells ratatui hands to `draw` are painted. The backend keeps its own copy of the grid
//! so it can repaint a cell after an overlay (cursor, IME preedit) moves away from it. Overlays
//! are painted in `flush`, which ratatui calls once per frame after positioning the cursor.

use std::io;

use ratatui::backend::{Backend, ClearType, WindowSize};
use ratatui::buffer::{Cell, CellDiffOption};
use ratatui::layout::{Position, Size};
use ratatui::style::Modifier;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::canvas::{Canvas, PixelRect, blend};
use crate::font::{CellMetrics, FontConfig, FontError, FontSet, GlyphStyle};
use crate::palette::{Palette, Rgb};

/// How the cursor is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorShape {
    #[default]
    Block,
    Bar,
    Underline,
    /// The block as an outline: the cursor of a window that is not focused.
    HollowBlock,
}

/// IME composing text, drawn underlined from a cell and not part of the grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preedit {
    pub text: String,
    pub col: u16,
    pub row: u16,
    /// The IME's caret, in columns from `col`; `None`: the IME hides it (no cursor drawn).
    pub caret: Option<u16>,
}

/// Placement of the grid inside the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridLayout {
    pub cols: u16,
    pub rows: u16,
    /// Top-left pixel of cell (0, 0); the rest of the canvas is padding.
    pub origin_x: u32,
    pub origin_y: u32,
    pub cell: CellMetrics,
}

impl GridLayout {
    fn new(width: u32, height: u32, padding: u32, cell: CellMetrics) -> Self {
        // A window smaller than one cell still reports a 1x1 grid; painting clips to the canvas.
        let fit = |len: u32, step: u32| {
            (len.saturating_sub(padding.saturating_mul(2)) / step).clamp(1, u32::from(u16::MAX))
                as u16
        };
        Self {
            cols: fit(width, cell.width),
            rows: fit(height, cell.height),
            origin_x: padding,
            origin_y: padding,
            cell,
        }
    }

    /// The pixels of `span` cells starting at (col, row), clipped to the grid.
    #[must_use]
    pub fn cell_rect(&self, col: u16, row: u16, span: u16) -> PixelRect {
        let span = span.min(self.cols.saturating_sub(col));
        PixelRect::new(
            self.origin_x + u32::from(col) * self.cell.width,
            self.origin_y + u32::from(row) * self.cell.height,
            u32::from(span) * self.cell.width,
            self.cell.height,
        )
    }

    /// The cell under a pixel, `None` in the padding.
    #[must_use]
    pub fn cell_at(&self, x: u32, y: u32) -> Option<(u16, u16)> {
        let col = x.checked_sub(self.origin_x)? / self.cell.width;
        let row = y.checked_sub(self.origin_y)? / self.cell.height;
        (col < u32::from(self.cols) && row < u32::from(self.rows))
            .then_some((col as u16, row as u16))
    }
}

#[derive(Debug, Clone)]
struct Slot {
    cell: Cell,
    /// The right half of the wide cell to its left.
    covered: bool,
}

/// Largest padding accepted, in logical pixels.
const MAX_PADDING: u32 = 1000;

const EMPTY_SLOT: Slot = Slot {
    cell: Cell::EMPTY,
    covered: false,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct OverlayState {
    cursor: Option<(u16, u16, CursorShape)>,
    preedit: Option<Preedit>,
}

/// A ratatui backend drawing into an owned 0RGB [`Canvas`].
#[derive(Debug)]
pub struct PixelBackend {
    fonts: FontSet,
    palette: Palette,
    canvas: Canvas,
    /// Configured padding in logical pixels.
    padding: u32,
    layout: GridLayout,
    grid: Vec<Slot>,
    cursor: Position,
    cursor_visible: bool,
    cursor_blink_on: bool,
    cursor_shape: CursorShape,
    preedit: Option<Preedit>,
    /// Cells (col, row) the overlays were painted over, to restore when they move.
    overlay_cells: Vec<(u16, u16)>,
    overlay_painted: Option<OverlayState>,
    overlay_damaged: bool,
    /// Per grid row, the damaged pixel columns `[from, to)`.
    damage_rows: Vec<Option<(u32, u32)>>,
    damage_full: bool,
}

impl PixelBackend {
    /// A backend over a `width` x `height` pixel canvas; the scale factor comes from the font
    /// config.
    #[must_use]
    pub fn new(fonts: FontSet, palette: Palette, width: u32, height: u32) -> Self {
        let layout = GridLayout::new(width, height, 0, fonts.metrics());
        let mut backend = Self {
            canvas: Canvas::new(width, height, palette.background),
            fonts,
            palette,
            padding: 0,
            layout,
            grid: Vec::new(),
            cursor: Position::ORIGIN,
            cursor_visible: true,
            cursor_blink_on: true,
            cursor_shape: CursorShape::Block,
            preedit: None,
            overlay_cells: Vec::new(),
            overlay_painted: None,
            overlay_damaged: false,
            damage_rows: Vec::new(),
            damage_full: true,
        };
        backend.relayout(width, height);
        backend
    }

    #[must_use]
    pub fn canvas(&self) -> &Canvas {
        &self.canvas
    }

    #[must_use]
    pub fn layout(&self) -> GridLayout {
        self.layout
    }

    #[must_use]
    pub fn fonts(&self) -> &FontSet {
        &self.fonts
    }

    #[must_use]
    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// Re-lays the grid for a new window size and scale factor. The grid is blank afterwards;
    /// ratatui's resize clears and redraws every cell.
    pub fn resize(&mut self, width: u32, height: u32, scale: f32) -> Result<(), FontError> {
        let result = if (scale - self.fonts.config().scale).abs() <= f32::EPSILON {
            Ok(())
        } else {
            self.fonts.set_scale(scale)
        };
        self.relayout(width, height);
        result
    }

    /// Applies new font settings (reload); on error the old font stays.
    pub fn set_font_config(&mut self, config: &FontConfig) -> Result<(), FontError> {
        self.fonts.set_config(config)?;
        self.relayout(self.canvas.width(), self.canvas.height());
        Ok(())
    }

    /// Padding around the grid in logical pixels (multiplied by the scale factor).
    pub fn set_padding(&mut self, padding: u32) {
        self.padding = padding.min(MAX_PADDING);
        self.relayout(self.canvas.width(), self.canvas.height());
    }

    /// New theme colours; every cell is repainted.
    pub fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
        self.canvas.fill(self.palette.background);
        self.repaint_grid();
    }

    pub fn set_cursor_shape(&mut self, shape: CursorShape) {
        self.cursor_shape = shape;
    }

    #[must_use]
    pub fn cursor_shape(&self) -> CursorShape {
        self.cursor_shape
    }

    /// The blink phase: `false` hides a visible cursor until it is `true` again.
    pub fn set_cursor_blink_on(&mut self, on: bool) {
        self.cursor_blink_on = on;
    }

    #[must_use]
    pub fn cursor_visible(&self) -> bool {
        self.cursor_visible
    }

    /// Where ratatui last put the cursor (shown or not); `Backend::get_cursor_position` needs
    /// `&mut self`.
    #[must_use]
    pub fn cursor_position(&self) -> Position {
        self.cursor
    }

    /// Shows IME composing text from a cell (`None` removes it). Takes effect on the next flush.
    pub fn set_preedit(&mut self, preedit: Option<Preedit>) {
        self.preedit = preedit.filter(|p| !p.text.is_empty());
    }

    /// Repaints the overlays now (ratatui's `flush` does the same).
    pub fn render_overlays(&mut self) {
        let preedit_cells = self.preedit_cells();
        // While composing the caret is the IME's, inside the preedit.
        let at = match &self.preedit {
            Some(p) => p
                .caret
                .map(|c| Position::new(p.col.saturating_add(c), p.row)),
            None => Some(self.cursor),
        };
        let shown = self.cursor_visible && self.cursor_blink_on;
        let cursor = at.filter(|_| shown).map(|at| {
            let raw = (
                at.x.min(self.layout.cols - 1),
                at.y.min(self.layout.rows - 1),
            );
            // On a preedit cell the cursor follows the preedit's cells, not the grid's.
            let on_preedit = preedit_cells
                .iter()
                .find(|(c, r, cell)| *r == raw.1 && (*c..*c + cell_width(cell)).contains(&raw.0));
            let (col, row) =
                on_preedit.map_or_else(|| self.lead_cell(raw.0, raw.1), |p| (p.0, p.1));
            (col, row, self.cursor_shape)
        });
        let wanted = OverlayState {
            cursor,
            preedit: self.preedit.clone(),
        };
        if !self.overlay_damaged && self.overlay_painted.as_ref() == Some(&wanted) {
            return;
        }
        for (col, row) in std::mem::take(&mut self.overlay_cells) {
            self.restore(col, row);
        }
        for (col, row, cell) in &preedit_cells {
            self.paint(*col, *row, cell, None);
            for c in *col..(*col + cell_width(cell)).min(self.layout.cols) {
                self.overlay_cells.push((c, *row));
            }
        }
        if let Some((col, row, shape)) = wanted.cursor {
            let cell = preedit_cells
                .into_iter()
                .find(|(c, r, _)| (*c, *r) == (col, row))
                .map_or_else(|| self.slot(col, row).cell.clone(), |(_, _, cell)| cell);
            self.paint(col, row, &cell, Some(shape));
            self.overlay_cells.push((col, row));
            if cell_width(&cell) == 2 && col + 1 < self.layout.cols {
                self.overlay_cells.push((col + 1, row));
            }
        }
        self.overlay_painted = Some(wanted);
        self.overlay_damaged = false;
    }

    /// The cells the preedit paints: its graphemes, underlined, in the colours of the cells
    /// below, plus a blank in the background colour for each half of a wide grid cell the
    /// preedit cuts at its ends, so no half glyph shows next to it.
    fn preedit_cells(&self) -> Vec<(u16, u16, Cell)> {
        let Some(p) = &self.preedit else {
            return Vec::new();
        };
        if p.row >= self.layout.rows || p.col >= self.layout.cols {
            return Vec::new();
        }
        let row = p.row;
        let blank = |slot: &Slot| {
            let mut cell = Cell::EMPTY;
            cell.bg = slot.cell.bg;
            cell
        };
        let mut cells = Vec::new();
        if p.col > 0 && self.slot(p.col, row).covered {
            cells.push((p.col - 1, row, blank(self.slot(p.col - 1, row))));
        }
        let mut col = p.col;
        for grapheme in p.text.graphemes(true) {
            if col >= self.layout.cols {
                break;
            }
            let under = &self.slot(col, row).cell;
            let mut cell = Cell::EMPTY;
            cell.set_symbol(grapheme);
            cell.fg = under.fg;
            cell.bg = under.bg;
            cell.modifier = Modifier::UNDERLINED;
            let width = cell_width(&cell);
            cells.push((col, row, cell));
            col += width;
        }
        if col < self.layout.cols && self.slot(col, row).covered {
            cells.push((col, row, blank(self.slot(col, row))));
        }
        cells
    }

    /// The damaged areas since the last call, for presenting only those; one rect per grid row
    /// (or the whole canvas after a resize / clear).
    pub fn take_damage(&mut self) -> Vec<PixelRect> {
        let rows = std::mem::replace(&mut self.damage_rows, vec![None; self.layout.rows as usize]);
        if std::mem::take(&mut self.damage_full) {
            return vec![self.canvas.bounds()];
        }
        rows.iter()
            .enumerate()
            .filter_map(|(row, span)| {
                let (from, to) = (*span)?;
                let y = self.layout.origin_y + row as u32 * self.layout.cell.height;
                // The last row also owns the leftover pixels below it (see `paint`).
                let height = if row + 1 == self.layout.rows as usize {
                    self.canvas.height().saturating_sub(y)
                } else {
                    self.layout.cell.height
                };
                Some(PixelRect::new(from, y, to - from, height))
                    .map(|r| r.intersect(&self.canvas.bounds()))
                    .filter(|r| !r.is_empty())
            })
            .collect()
    }

    fn relayout(&mut self, width: u32, height: u32) {
        let padding = (self.padding as f32 * self.fonts.config().scale).round() as u32;
        let old = self.layout;
        self.layout = GridLayout::new(width, height, padding, self.fonts.metrics());
        self.canvas = Canvas::new(width, height, self.palette.background);
        self.damage_rows = vec![None; self.layout.rows as usize];
        if (old.cols, old.rows) == (self.layout.cols, self.layout.rows) && !self.grid.is_empty() {
            // ratatui redraws only when the grid size changes, so the backend repaints what it
            // holds at the new metrics.
            self.repaint_grid();
        } else {
            self.grid = vec![EMPTY_SLOT; self.layout.cols as usize * self.layout.rows as usize];
            self.damage_full = true;
            self.reset_overlays();
        }
    }

    /// Overlays were wiped together with the pixels under them.
    fn reset_overlays(&mut self) {
        self.overlay_cells.clear();
        self.overlay_painted = None;
    }

    fn repaint_grid(&mut self) {
        for row in 0..self.layout.rows {
            for col in 0..self.layout.cols {
                if !self.slot(col, row).covered {
                    let cell = self.slot(col, row).cell.clone();
                    self.paint(col, row, &cell, None);
                }
            }
        }
        self.damage_full = true;
        self.reset_overlays();
    }

    fn index(&self, col: u16, row: u16) -> usize {
        row as usize * self.layout.cols as usize + col as usize
    }

    fn slot(&self, col: u16, row: u16) -> &Slot {
        &self.grid[self.index(col, row)]
    }

    /// The cell whose pixels cover (col, row): its wide left neighbour when covered.
    fn lead_cell(&self, col: u16, row: u16) -> (u16, u16) {
        let col = col.min(self.layout.cols - 1);
        let row = row.min(self.layout.rows - 1);
        if col > 0 && self.slot(col, row).covered {
            (col - 1, row)
        } else {
            (col, row)
        }
    }

    /// Repaints a cell from the grid copy.
    fn restore(&mut self, col: u16, row: u16) {
        if col >= self.layout.cols || row >= self.layout.rows {
            return;
        }
        let (col, row) = self.lead_cell(col, row);
        let cell = self.slot(col, row).cell.clone();
        self.paint(col, row, &cell, None);
        // A narrow cell over a former wide cell's right half: that half was never repainted.
        if cell_width(&cell) == 1 && col + 1 < self.layout.cols && self.slot(col + 1, row).covered {
            let cover = self.slot(col + 1, row).cell.clone();
            self.paint(col + 1, row, &cover, None);
        }
    }

    /// Stores and paints one cell ratatui reports as changed.
    fn put(&mut self, col: u16, row: u16, cell: &Cell) {
        if col >= self.layout.cols || row >= self.layout.rows {
            return;
        }
        let width = cell_width(cell);
        let index = self.index(col, row);
        let was_wide = !self.grid[index].covered && cell_width(&self.grid[index].cell) == 2;
        self.grid[index] = Slot {
            cell: cell.clone(),
            covered: false,
        };
        if width == 2 && col + 1 < self.layout.cols {
            let mut cover = Cell::EMPTY;
            cover.bg = cell.bg;
            self.grid[index + 1] = Slot {
                cell: cover,
                covered: true,
            };
        } else if was_wide && col + 1 < self.layout.cols && self.grid[index + 1].covered {
            // The right half is no longer covered; ratatui does not resend an unchanged blank.
            self.grid[index + 1].covered = false;
            let cover = self.grid[index + 1].cell.clone();
            self.paint(col + 1, row, &cover, None);
        }
        self.paint(col, row, cell, None);
        if self
            .overlay_cells
            .iter()
            .any(|&(c, r)| r == row && (c == col || ((width == 2 || was_wide) && c == col + 1)))
        {
            self.overlay_damaged = true;
        }
    }

    /// Paints a cell: background, glyph, decorations, and a cursor if asked.
    fn paint(&mut self, col: u16, row: u16, cell: &Cell, cursor: Option<CursorShape>) {
        let width = cell_width(cell);
        let rect = self.layout.cell_rect(col, row, width);
        if rect.is_empty() {
            return;
        }
        let m = self.layout.cell;
        let modifier = cell.modifier;
        let mut fg = self.palette.fg(cell.fg);
        let mut bg = self.palette.bg(cell.bg);
        if modifier.contains(Modifier::REVERSED) {
            std::mem::swap(&mut fg, &mut bg);
        }
        if modifier.contains(Modifier::DIM) {
            fg = blend(bg, fg, 140);
        }
        self.fill_leftover(col + width, row, rect, bg);
        let cursor_color = self.palette.cursor.unwrap_or(fg);
        if cursor == Some(CursorShape::Block) {
            (fg, bg) = (bg, cursor_color);
        }
        self.canvas.fill_rect(rect, bg);
        let symbol = cell.symbol();
        if !modifier.contains(Modifier::HIDDEN) && !symbol.is_empty() && symbol != " " {
            let style = GlyphStyle {
                bold: modifier.contains(Modifier::BOLD),
                italic: modifier.contains(Modifier::ITALIC),
            };
            let (ox, oy) = (rect.x as i32, rect.y as i32);
            for image in self.fonts.glyph(symbol, style) {
                self.canvas.draw_glyph(image, ox, oy, rect, fg);
            }
        }
        let line = |offset: i32| {
            PixelRect::new(
                rect.x,
                (rect.y as i32 + offset).max(0) as u32,
                rect.width,
                m.line,
            )
            .intersect(&rect)
        };
        if modifier.contains(Modifier::UNDERLINED) {
            let color = self.palette.resolve_or(cell.underline_color, fg);
            self.canvas.fill_rect(line(m.underline), color);
        }
        if modifier.contains(Modifier::CROSSED_OUT) {
            self.canvas.fill_rect(line(m.strike), fg);
        }
        match cursor {
            Some(CursorShape::Bar) => {
                let bar = PixelRect::new(rect.x, rect.y, (m.line * 2).min(rect.width), rect.height);
                self.canvas.fill_rect(bar, cursor_color);
            }
            Some(CursorShape::Underline) => {
                let h = (m.line * 2).min(rect.height);
                let bar = PixelRect::new(rect.x, rect.bottom() - h, rect.width, h);
                self.canvas.fill_rect(bar, cursor_color);
            }
            Some(CursorShape::HollowBlock) => {
                let t = m.line.max(1);
                let (w, h) = (t.min(rect.width), t.min(rect.height));
                for edge in [
                    PixelRect::new(rect.x, rect.y, rect.width, h),
                    PixelRect::new(rect.x, rect.bottom() - h, rect.width, h),
                    PixelRect::new(rect.x, rect.y, w, rect.height),
                    PixelRect::new(rect.right() - w, rect.y, w, rect.height),
                ] {
                    self.canvas.fill_rect(edge, cursor_color);
                }
            }
            Some(CursorShape::Block) | None => {}
        }
        self.damage(row, rect);
    }

    /// The pixels between the last whole cell and the padding (a window size that is not a
    /// multiple of the cell) take the background of the last column's and last row's cells, so
    /// a status bar or a panel reaches the window's edge instead of leaving a strip.
    fn fill_leftover(&mut self, end_col: u16, row: u16, rect: PixelRect, bg: Rgb) {
        let right_edge = self.canvas.width().saturating_sub(self.layout.origin_x);
        let bottom_edge = self.canvas.height().saturating_sub(self.layout.origin_y);
        let below = if row + 1 >= self.layout.rows {
            bottom_edge.saturating_sub(rect.bottom())
        } else {
            0
        };
        if below > 0 {
            let strip = PixelRect::new(rect.x, rect.bottom(), rect.width, below);
            self.canvas.fill_rect(strip, bg);
        }
        if end_col >= self.layout.cols && right_edge > rect.right() {
            let strip = PixelRect::new(
                rect.right(),
                rect.y,
                right_edge - rect.right(),
                rect.height + below,
            );
            self.canvas.fill_rect(strip, bg);
            self.damage(row, strip);
        }
    }

    fn damage(&mut self, row: u16, rect: PixelRect) {
        if let Some(span) = self.damage_rows.get_mut(row as usize) {
            *span = Some(match *span {
                Some((from, to)) => (from.min(rect.x), to.max(rect.right())),
                None => (rect.x, rect.right()),
            });
        }
    }

    fn clear_cells(&mut self, from: usize, to: usize) {
        let cols = self.layout.cols as usize;
        for i in from..to.min(self.grid.len()) {
            self.put((i % cols) as u16, (i / cols) as u16, &Cell::EMPTY);
        }
    }
}

impl Palette {
    fn resolve_or(&self, color: ratatui::style::Color, default: Rgb) -> Rgb {
        if color == ratatui::style::Color::Reset {
            default
        } else {
            self.fg(color)
        }
    }
}

/// Cells a symbol occupies, as ratatui counts them (1 or 2).
fn cell_width(cell: &Cell) -> u16 {
    let width = match cell.diff_option {
        CellDiffOption::ForcedWidth(n) => usize::from(n.get()),
        _ => cell.symbol().width(),
    };
    width.clamp(1, 2) as u16
}

impl Backend for PixelBackend {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        for (col, row, cell) in content {
            if cell.diff_option != CellDiffOption::Skip {
                self.put(col, row, cell);
            }
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.cursor_visible = false;
        Ok(())
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.cursor_visible = true;
        Ok(())
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        Ok(self.cursor)
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        self.cursor = position.into();
        Ok(())
    }

    fn clear(&mut self) -> io::Result<()> {
        self.canvas.fill(self.palette.background);
        self.grid.fill(EMPTY_SLOT);
        self.damage_full = true;
        self.reset_overlays();
        Ok(())
    }

    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        let cols = self.layout.cols as usize;
        let (col, row) = (
            usize::from(self.cursor.x).min(cols - 1),
            usize::from(self.cursor.y).min(self.layout.rows as usize - 1),
        );
        let at = row * cols + col;
        match clear_type {
            ClearType::All => return self.clear(),
            ClearType::AfterCursor => self.clear_cells(at, self.grid.len()),
            ClearType::BeforeCursor => self.clear_cells(0, at + 1),
            ClearType::CurrentLine => self.clear_cells(row * cols, (row + 1) * cols),
            ClearType::UntilNewLine => self.clear_cells(at, (row + 1) * cols),
        }
        Ok(())
    }

    fn size(&self) -> io::Result<Size> {
        Ok(Size::new(self.layout.cols, self.layout.rows))
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        let px = |v: u32| u16::try_from(v).unwrap_or(u16::MAX);
        Ok(WindowSize {
            columns_rows: Size::new(self.layout.cols, self.layout.rows),
            pixels: Size::new(px(self.canvas.width()), px(self.canvas.height())),
        })
    }

    fn flush(&mut self) -> io::Result<()> {
        self.render_overlays();
        Ok(())
    }
}

#[cfg(test)]
mod tests;
