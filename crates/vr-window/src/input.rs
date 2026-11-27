//! Window input → the crossterm events the app reads in a terminal: keys, mouse and IME.
//! Pure functions and small state machines over winit's event values; the window runner feeds
//! them.

pub mod ime;
pub mod key;
pub mod mouse;

pub use ime::{ImeInput, PreeditText};
pub use key::{KeyInput, key_events, modifiers, translate_key};
pub use mouse::{MouseInput, cell_at_clamped};
