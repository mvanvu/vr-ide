//! Key, paste and mouse events → the bytes a program in the terminal expects (xterm conventions).

use crate::types::Modes;

/// A key, independent of the UI toolkit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCode {
    /// A printable character, already shifted by the caller (`A`, not `shift+a`).
    Char(char),
    Enter,
    Tab,
    /// shift+tab as one key.
    BackTab,
    Backspace,
    Esc,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    /// F1–F12.
    F(u8),
}

/// Modifier keys held with a key or mouse event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

impl Modifiers {
    pub const NONE: Modifiers = Modifiers {
        shift: false,
        alt: false,
        ctrl: false,
    };
    pub const SHIFT: Modifiers = Modifiers {
        shift: true,
        alt: false,
        ctrl: false,
    };
    pub const ALT: Modifiers = Modifiers {
        shift: false,
        alt: true,
        ctrl: false,
    };
    pub const CTRL: Modifiers = Modifiers {
        shift: false,
        alt: false,
        ctrl: true,
    };

    fn any(self) -> bool {
        self.shift || self.alt || self.ctrl
    }

    /// xterm's modifier parameter: 1 + shift + 2·alt + 4·ctrl.
    fn param(self) -> u8 {
        1 + u8::from(self.shift) + 2 * u8::from(self.alt) + 4 * u8::from(self.ctrl)
    }
}

/// A key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub mods: Modifiers,
}

impl KeyEvent {
    pub fn new(code: KeyCode, mods: Modifiers) -> Self {
        Self { code, mods }
    }

    pub fn plain(code: KeyCode) -> Self {
        Self {
            code,
            mods: Modifiers::NONE,
        }
    }
}

/// Mouse buttons that can be reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

/// What happened with the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseKind {
    Press(MouseButton),
    Release(MouseButton),
    /// Motion with a button held.
    Drag(MouseButton),
    /// Motion with no button held.
    Move,
    ScrollUp,
    ScrollDown,
}

/// A mouse event at a 0-based cell of the terminal grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MouseEvent {
    pub kind: MouseKind,
    pub col: u16,
    pub row: u16,
    pub mods: Modifiers,
}

/// Bytes for a key press. Unknown combinations fall back to the unmodified key.
pub fn encode_key(key: KeyEvent, modes: Modes) -> Vec<u8> {
    let mods = key.mods;
    let mut out = Vec::new();
    match key.code {
        KeyCode::Char(c) => {
            if mods.alt {
                out.push(0x1b);
            }
            match (mods.ctrl, ctrl_code(c)) {
                (true, Some(byte)) => out.push(byte),
                _ => {
                    let mut buf = [0u8; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                }
            }
        }
        KeyCode::Enter => alt_prefixed(&mut out, mods, b"\r"),
        KeyCode::Tab if mods.shift => out.extend_from_slice(b"\x1b[Z"),
        KeyCode::Tab => alt_prefixed(&mut out, mods, b"\t"),
        KeyCode::BackTab => out.extend_from_slice(b"\x1b[Z"),
        // ctrl+backspace sends BS so shells and editors can tell it apart (delete word).
        KeyCode::Backspace if mods.ctrl => alt_prefixed(&mut out, mods, b"\x08"),
        KeyCode::Backspace => alt_prefixed(&mut out, mods, b"\x7f"),
        KeyCode::Esc => alt_prefixed(&mut out, mods, b"\x1b"),
        KeyCode::Up => cursor_key(&mut out, b'A', mods, modes),
        KeyCode::Down => cursor_key(&mut out, b'B', mods, modes),
        KeyCode::Right => cursor_key(&mut out, b'C', mods, modes),
        KeyCode::Left => cursor_key(&mut out, b'D', mods, modes),
        KeyCode::Home => cursor_key(&mut out, b'H', mods, modes),
        KeyCode::End => cursor_key(&mut out, b'F', mods, modes),
        KeyCode::Insert => tilde_key(&mut out, 2, mods),
        KeyCode::Delete => tilde_key(&mut out, 3, mods),
        KeyCode::PageUp => tilde_key(&mut out, 5, mods),
        KeyCode::PageDown => tilde_key(&mut out, 6, mods),
        KeyCode::F(n @ 1..=4) => {
            let last = b"PQRS"[usize::from(n - 1)];
            if mods.any() {
                out.extend_from_slice(format!("\x1b[1;{}", mods.param()).as_bytes());
                out.push(last);
            } else {
                out.extend_from_slice(&[0x1b, b'O', last]);
            }
        }
        KeyCode::F(n @ 5..=12) => {
            const CODES: [u8; 8] = [15, 17, 18, 19, 20, 21, 23, 24];
            tilde_key(&mut out, CODES[usize::from(n - 5)], mods);
        }
        KeyCode::F(_) => {}
    }
    out
}

/// The C0 byte ctrl+`c` sends in xterm, if any.
fn ctrl_code(c: char) -> Option<u8> {
    Some(match c {
        'a'..='z' => c as u8 - b'a' + 1,
        'A'..='Z' => c as u8 - b'A' + 1,
        // ctrl+` is NUL in legacy terminals too, which is what programs expect from it.
        ' ' | '@' | '2' | '`' => 0,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' | '~' => 0x1e,
        '_' | '/' | '7' | '-' => 0x1f,
        '8' | '?' => 0x7f,
        _ => return None,
    })
}

fn alt_prefixed(out: &mut Vec<u8>, mods: Modifiers, bytes: &[u8]) {
    if mods.alt {
        out.push(0x1b);
    }
    out.extend_from_slice(bytes);
}

/// Arrows, Home and End: `CSI x`, `SS3 x` in application cursor mode, `CSI 1;m x` when modified.
fn cursor_key(out: &mut Vec<u8>, last: u8, mods: Modifiers, modes: Modes) {
    if mods.any() {
        out.extend_from_slice(format!("\x1b[1;{}", mods.param()).as_bytes());
    } else if modes.app_cursor {
        out.extend_from_slice(b"\x1bO");
    } else {
        out.extend_from_slice(b"\x1b[");
    }
    out.push(last);
}

/// `CSI n ~`, or `CSI n;m ~` when modified.
fn tilde_key(out: &mut Vec<u8>, n: u8, mods: Modifiers) {
    if mods.any() {
        out.extend_from_slice(format!("\x1b[{n};{}~", mods.param()).as_bytes());
    } else {
        out.extend_from_slice(format!("\x1b[{n}~").as_bytes());
    }
}

/// Bytes for pasted text: line breaks become CR (what Enter sends); with bracketed paste on, the
/// text is wrapped in `ESC [200~` … `ESC [201~` and any ESC inside it is dropped so the paste
/// cannot end the bracket early and smuggle commands in.
pub fn encode_paste(text: &str, modes: Modes) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() + 12);
    if modes.bracketed_paste {
        out.extend_from_slice(b"\x1b[200~");
    }
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push(b'\r');
            }
            '\n' => out.push(b'\r'),
            '\x1b' if modes.bracketed_paste => {}
            _ => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    if modes.bracketed_paste {
        out.extend_from_slice(b"\x1b[201~");
    }
    out
}

/// Bytes reporting a mouse event, or nothing when the program has not asked for this kind of
/// event in SGR (1006) encoding. Legacy X10 encoding is not produced: it cannot address columns
/// past 223 and every program that wants the mouse today asks for SGR.
pub fn encode_mouse(event: MouseEvent, modes: Modes) -> Vec<u8> {
    if !(modes.mouse_reporting && modes.sgr_mouse) {
        return Vec::new();
    }
    fn button(b: MouseButton) -> u16 {
        match b {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
        }
    }
    let (code, release) = match event.kind {
        MouseKind::Press(b) => (button(b), false),
        MouseKind::Release(b) => (button(b), true),
        MouseKind::Drag(b) if modes.mouse_drag || modes.mouse_motion => (32 + button(b), false),
        MouseKind::Move if modes.mouse_motion => (35, false),
        MouseKind::Drag(_) | MouseKind::Move => return Vec::new(),
        MouseKind::ScrollUp => (64, false),
        MouseKind::ScrollDown => (65, false),
    };
    let mods = event.mods;
    let code =
        code + 4 * u16::from(mods.shift) + 8 * u16::from(mods.alt) + 16 * u16::from(mods.ctrl);
    let col = u32::from(event.col) + 1;
    let row = u32::from(event.row) + 1;
    let end = if release { 'm' } else { 'M' };
    format!("\x1b[<{code};{col};{row}{end}").into_bytes()
}

#[cfg(test)]
mod tests;
