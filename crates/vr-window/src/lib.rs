//! Window frontend pieces that know nothing about the app: the pixel renderer (a ratatui
//! backend drawing into a 0RGB buffer), the input translation
//! (winit events → crossterm events) and the window runner ([`app`]).

pub mod app;
mod backend;
mod canvas;
mod font;
pub mod input;
mod palette;

pub use backend::{CursorShape, GridLayout, PixelBackend, Preedit};
pub use canvas::{Canvas, PixelRect};
pub use font::{
    CellMetrics, FontConfig, FontError, FontSet, GlyphImage, GlyphStyle, system_font_system,
};
pub use palette::{Palette, Rgb, rgb};
