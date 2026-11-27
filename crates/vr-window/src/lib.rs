//! Window frontend pieces that know nothing about the app: the pixel renderer (a ratatui
//! backend drawing into a 0RGB buffer), and later the input translation and the window runner.

mod backend;
mod canvas;
mod font;
mod palette;

pub use backend::{CursorShape, GridLayout, PixelBackend, Preedit};
pub use canvas::{Canvas, PixelRect};
pub use font::{
    CellMetrics, FontConfig, FontError, FontSet, GlyphImage, GlyphStyle, system_font_system,
};
pub use palette::{Palette, Rgb, rgb};
