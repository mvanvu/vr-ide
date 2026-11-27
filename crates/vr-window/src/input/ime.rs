//! IME composition: the preedit is held for drawing and never sent; key events are held back
//! while composing; committed text goes out once, one key press per character.

use crossterm::event::{KeyEvent, KeyEventKind, KeyEventState};
use unicode_width::UnicodeWidthStr;
use winit::event::{ElementState, Ime};
use winit::keyboard::{Key, ModifiersState, NamedKey};

use super::key::{KeyInput, key_events, text_events};
use crate::Preedit;

/// The composing text and the IME's cursor in it (byte offsets, as winit gives them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreeditText {
    pub text: String,
    /// Start and end of the IME's cursor or selection; `None` hides the cursor.
    pub cursor: Option<(usize, usize)>,
}

impl PreeditText {
    /// The cursor as display columns from the preedit's first cell. Offsets outside the text or
    /// inside a character are moved back to the previous character boundary.
    #[must_use]
    pub fn cursor_columns(&self) -> Option<(usize, usize)> {
        let col = |byte: usize| {
            let mut b = byte.min(self.text.len());
            while !self.text.is_char_boundary(b) {
                b -= 1;
            }
            self.text[..b].width()
        };
        self.cursor.map(|(start, end)| (col(start), col(end)))
    }

    /// The renderer's preedit placed at a cell (the app's cursor).
    #[must_use]
    pub fn at(&self, col: u16, row: u16) -> Preedit {
        Preedit {
            text: self.text.clone(),
            col,
            row,
        }
    }
}

/// IME and key events → the key events the app gets, plus the preedit to draw.
#[derive(Debug, Default, Clone)]
pub struct ImeInput {
    enabled: bool,
    preedit: Option<PreeditText>,
    cancel_requested: bool,
}

impl ImeInput {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The text being composed, for drawing at the cursor.
    #[must_use]
    pub fn preedit(&self) -> Option<&PreeditText> {
        self.preedit.as_ref()
    }

    /// Whether a composition is in progress (key events are held back).
    #[must_use]
    pub fn is_composing(&self) -> bool {
        self.preedit.is_some()
    }

    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// True once after Esc dropped a composition the IME still holds: the window should reset
    /// the IME (disallow and allow it again) so it does not commit the dropped text later.
    pub fn take_cancel_request(&mut self) -> bool {
        std::mem::take(&mut self.cancel_requested)
    }

    /// A `WindowEvent::Ime`. Returns the key events to send: one press per committed
    /// character, nothing for the other events.
    pub fn ime(&mut self, event: &Ime) -> Vec<KeyEvent> {
        match event {
            Ime::Enabled => {
                self.enabled = true;
                Vec::new()
            }
            Ime::Preedit(text, _) if text.is_empty() => {
                self.preedit = None;
                Vec::new()
            }
            Ime::Preedit(text, cursor) => {
                self.preedit = Some(PreeditText {
                    text: text.clone(),
                    cursor: *cursor,
                });
                Vec::new()
            }
            Ime::Commit(text) => {
                self.preedit = None;
                // Presses only: the app ignores releases, and an embedded terminal writes the
                // bytes of each press.
                text_events(text, KeyEventKind::Press, KeyEventState::NONE)
            }
            Ime::Disabled => {
                self.enabled = false;
                self.preedit = None;
                Vec::new()
            }
        }
    }

    /// A `WindowEvent::KeyboardInput`. While composing, keys belong to the IME and nothing is
    /// sent; Esc there also drops the composition (and asks for an IME reset). Otherwise the
    /// translated key events.
    pub fn key(&mut self, input: &KeyInput, mods: ModifiersState) -> Vec<KeyEvent> {
        if self.preedit.is_some() {
            if input.state == ElementState::Pressed && input.logical == Key::Named(NamedKey::Escape)
            {
                self.preedit = None;
                self.cancel_requested = true;
            }
            return Vec::new();
        }
        key_events(input, mods)
    }
}

#[cfg(test)]
mod tests;
