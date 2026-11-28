//! winit key events → crossterm key events, shaped like crossterm's decoding of the kitty
//! keyboard protocol (disambiguate + alternate keys + event types), which is what the app gets
//! in a terminal today.
//!
//! - Text keys without ctrl/alt/super arrive as their text, `SHIFT` only on uppercase letters
//!   (crossterm's legacy text decoding).
//! - ctrl/alt/super chords use the logical key when it is an ASCII letter or symbol, else the US
//!   name of the physical key, so chords keep working on non-Latin layouts. Letters are
//!   lowercase unless shift is held (caps lock never turns ctrl+a into ctrl+shift+a). Shift
//!   stays in the modifiers, so ctrl+shift+1 is `Char('!')` + ctrl + shift, which the app's
//!   chord reader turns into "ctrl+shift+1".
//! - Named keys keep every modifier; shift+tab is `BackTab` with shift, as crossterm reports it.
//! - Modifier-only and lock keys are dropped: the kitty flags the app uses do not report them,
//!   and the app treats every key event as input.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
use winit::event::ElementState;
use winit::keyboard::{
    Key, KeyCode as Code, KeyLocation, ModifiersState, NamedKey, PhysicalKey, SmolStr,
};

/// The fields of a winit `KeyEvent` the translation reads. winit's own type has a private
/// platform field and cannot be built outside winit, so tests build this instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyInput {
    pub logical: Key,
    pub physical: PhysicalKey,
    pub text: Option<SmolStr>,
    pub location: KeyLocation,
    pub state: ElementState,
    pub repeat: bool,
}

impl From<&winit::event::KeyEvent> for KeyInput {
    fn from(ev: &winit::event::KeyEvent) -> Self {
        Self {
            logical: ev.logical_key.clone(),
            physical: ev.physical_key,
            text: ev.text.clone(),
            location: ev.location,
            state: ev.state,
            repeat: ev.repeat,
        }
    }
}

/// crossterm modifiers for winit's modifier state.
#[must_use]
pub fn modifiers(mods: ModifiersState) -> KeyModifiers {
    let mut m = KeyModifiers::NONE;
    m.set(KeyModifiers::SHIFT, mods.shift_key());
    m.set(KeyModifiers::CONTROL, mods.control_key());
    m.set(KeyModifiers::ALT, mods.alt_key());
    m.set(KeyModifiers::SUPER, mods.super_key());
    m
}

/// The key event for one winit key event; `None` for keys the app does not get (modifier and
/// lock keys, dead keys, unknown keys) and for text of more than one character (see
/// [`key_events`]).
#[must_use]
pub fn translate_key(input: &KeyInput, mods: ModifiersState) -> Option<KeyEvent> {
    let mut events = key_events(input, mods);
    if events.len() == 1 {
        events.pop()
    } else {
        None
    }
}

/// Every key event for one winit key event: usually one, none for keys the app does not get,
/// and one per character when a key produces several (a dead key that did not combine).
#[must_use]
pub fn key_events(input: &KeyInput, mods: ModifiersState) -> Vec<KeyEvent> {
    let kind = match (input.state, input.repeat) {
        (ElementState::Released, _) => KeyEventKind::Release,
        (ElementState::Pressed, true) => KeyEventKind::Repeat,
        (ElementState::Pressed, false) => KeyEventKind::Press,
    };
    let state = if input.location == KeyLocation::Numpad {
        KeyEventState::KEYPAD
    } else {
        KeyEventState::NONE
    };
    let event = |code, m| KeyEvent::new_with_kind_and_state(code, m, kind, state);
    let chord = mods.control_key() || mods.alt_key() || mods.super_key();
    let all = modifiers(mods);
    match &input.logical {
        // SHIFT stays, as the kitty protocol reports shift+space, so both frontends name it
        // "shift+space"; typing a space ignores it.
        Key::Named(NamedKey::Space) if !chord => text_events(" ", kind, state)
            .into_iter()
            .map(|mut e| {
                e.modifiers = all;
                e
            })
            .collect(),
        Key::Named(named) => named_code(*named, input.location, mods.shift_key())
            .map(|code| vec![event(code, all)])
            .unwrap_or_default(),
        Key::Character(s) if chord => chord_char(s, input.physical, mods.shift_key())
            .map(|c| vec![event(KeyCode::Char(c), all)])
            .unwrap_or_default(),
        Key::Character(s) => {
            // `text` holds what was typed (it can differ from the logical key after a dead
            // key); releases carry no text.
            let text = input
                .text
                .as_deref()
                .filter(|t| !t.is_empty() && !t.chars().any(char::is_control))
                .unwrap_or(s);
            text_events(text, kind, state)
        }
        Key::Dead(_) | Key::Unidentified(_) if chord => us_key(input.physical, mods.shift_key())
            .map(|c| vec![event(KeyCode::Char(letter_case(c, mods.shift_key())), all)])
            .unwrap_or_default(),
        Key::Dead(_) | Key::Unidentified(_) => Vec::new(),
    }
}

/// One event per character of typed text, `SHIFT` on uppercase letters only (as crossterm
/// decodes plain text).
pub(crate) fn text_events(text: &str, kind: KeyEventKind, state: KeyEventState) -> Vec<KeyEvent> {
    text.chars()
        .filter_map(|c| {
            let code = match c {
                '\r' | '\n' => KeyCode::Enter,
                '\t' => KeyCode::Tab,
                c if c.is_control() => return None,
                c => KeyCode::Char(c),
            };
            let m = if c.is_uppercase() {
                KeyModifiers::SHIFT
            } else {
                KeyModifiers::NONE
            };
            Some(KeyEvent::new_with_kind_and_state(code, m, kind, state))
        })
        .collect()
}

/// The character a ctrl/alt/super chord names. Non-ASCII letters go to the physical key too:
/// on a chord they come from Option on macOS (alt+a = å) or from layout keys (AZERTY é)
/// that keymaps cannot name.
fn chord_char(logical: &str, physical: PhysicalKey, shift: bool) -> Option<char> {
    let mut chars = logical.chars();
    let single = match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    };
    let c = single
        .filter(char::is_ascii_graphic)
        .or_else(|| us_key(physical, shift))
        .or(single)?;
    Some(letter_case(c, shift))
}

/// Letters follow shift (not caps lock); other characters stay as they are.
fn letter_case(c: char, shift: bool) -> char {
    fn single(mut it: impl Iterator<Item = char>) -> Option<char> {
        match (it.next(), it.next()) {
            (Some(m), None) => Some(m),
            _ => None,
        }
    }
    if !c.is_alphabetic() {
        return c;
    }
    let mapped = if shift {
        single(c.to_uppercase())
    } else {
        single(c.to_lowercase())
    };
    mapped.unwrap_or(c)
}

/// What the physical key types on a US layout.
fn us_key(physical: PhysicalKey, shift: bool) -> Option<char> {
    let PhysicalKey::Code(code) = physical else {
        return None;
    };
    let (base, shifted) = match code {
        Code::KeyA => ('a', 'A'),
        Code::KeyB => ('b', 'B'),
        Code::KeyC => ('c', 'C'),
        Code::KeyD => ('d', 'D'),
        Code::KeyE => ('e', 'E'),
        Code::KeyF => ('f', 'F'),
        Code::KeyG => ('g', 'G'),
        Code::KeyH => ('h', 'H'),
        Code::KeyI => ('i', 'I'),
        Code::KeyJ => ('j', 'J'),
        Code::KeyK => ('k', 'K'),
        Code::KeyL => ('l', 'L'),
        Code::KeyM => ('m', 'M'),
        Code::KeyN => ('n', 'N'),
        Code::KeyO => ('o', 'O'),
        Code::KeyP => ('p', 'P'),
        Code::KeyQ => ('q', 'Q'),
        Code::KeyR => ('r', 'R'),
        Code::KeyS => ('s', 'S'),
        Code::KeyT => ('t', 'T'),
        Code::KeyU => ('u', 'U'),
        Code::KeyV => ('v', 'V'),
        Code::KeyW => ('w', 'W'),
        Code::KeyX => ('x', 'X'),
        Code::KeyY => ('y', 'Y'),
        Code::KeyZ => ('z', 'Z'),
        Code::Digit1 => ('1', '!'),
        Code::Digit2 => ('2', '@'),
        Code::Digit3 => ('3', '#'),
        Code::Digit4 => ('4', '$'),
        Code::Digit5 => ('5', '%'),
        Code::Digit6 => ('6', '^'),
        Code::Digit7 => ('7', '&'),
        Code::Digit8 => ('8', '*'),
        Code::Digit9 => ('9', '('),
        Code::Digit0 => ('0', ')'),
        Code::Minus => ('-', '_'),
        Code::Equal => ('=', '+'),
        Code::BracketLeft => ('[', '{'),
        Code::BracketRight => (']', '}'),
        Code::Backslash => ('\\', '|'),
        Code::Semicolon => (';', ':'),
        Code::Quote => ('\'', '"'),
        Code::Backquote => ('`', '~'),
        Code::Comma => (',', '<'),
        Code::Period => ('.', '>'),
        Code::Slash => ('/', '?'),
        Code::Space => (' ', ' '),
        Code::Numpad0 => ('0', '0'),
        Code::Numpad1 => ('1', '1'),
        Code::Numpad2 => ('2', '2'),
        Code::Numpad3 => ('3', '3'),
        Code::Numpad4 => ('4', '4'),
        Code::Numpad5 => ('5', '5'),
        Code::Numpad6 => ('6', '6'),
        Code::Numpad7 => ('7', '7'),
        Code::Numpad8 => ('8', '8'),
        Code::Numpad9 => ('9', '9'),
        Code::NumpadAdd => ('+', '+'),
        Code::NumpadSubtract => ('-', '-'),
        Code::NumpadMultiply => ('*', '*'),
        Code::NumpadDivide => ('/', '/'),
        Code::NumpadDecimal => ('.', '.'),
        Code::NumpadComma => (',', ','),
        Code::NumpadEqual => ('=', '='),
        _ => return None,
    };
    Some(if shift { shifted } else { base })
}

/// crossterm code for a named key; `None` for keys the app does not get.
fn named_code(named: NamedKey, location: KeyLocation, shift: bool) -> Option<KeyCode> {
    use NamedKey as N;
    Some(match named {
        N::Enter => KeyCode::Enter,
        N::Tab if shift => KeyCode::BackTab,
        N::Tab => KeyCode::Tab,
        N::Space => KeyCode::Char(' '),
        N::Backspace => KeyCode::Backspace,
        N::Escape => KeyCode::Esc,
        N::ArrowUp => KeyCode::Up,
        N::ArrowDown => KeyCode::Down,
        N::ArrowLeft => KeyCode::Left,
        N::ArrowRight => KeyCode::Right,
        N::Home => KeyCode::Home,
        N::End => KeyCode::End,
        N::PageUp => KeyCode::PageUp,
        N::PageDown => KeyCode::PageDown,
        N::Insert => KeyCode::Insert,
        N::Delete => KeyCode::Delete,
        N::ContextMenu => KeyCode::Menu,
        N::PrintScreen => KeyCode::PrintScreen,
        N::Pause => KeyCode::Pause,
        // Keypad 5 with num lock off.
        N::Clear if location == KeyLocation::Numpad => KeyCode::KeypadBegin,
        other => KeyCode::F(function_number(other)?),
    })
}

fn function_number(named: NamedKey) -> Option<u8> {
    use NamedKey as N;
    const KEYS: [NamedKey; 35] = [
        N::F1,
        N::F2,
        N::F3,
        N::F4,
        N::F5,
        N::F6,
        N::F7,
        N::F8,
        N::F9,
        N::F10,
        N::F11,
        N::F12,
        N::F13,
        N::F14,
        N::F15,
        N::F16,
        N::F17,
        N::F18,
        N::F19,
        N::F20,
        N::F21,
        N::F22,
        N::F23,
        N::F24,
        N::F25,
        N::F26,
        N::F27,
        N::F28,
        N::F29,
        N::F30,
        N::F31,
        N::F32,
        N::F33,
        N::F34,
        N::F35,
    ];
    let i = KEYS.iter().position(|&k| k == named)?;
    u8::try_from(i + 1).ok()
}

#[cfg(test)]
mod tests;
