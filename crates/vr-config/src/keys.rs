//! Key chords independent of any terminal library; `vr-ide` converts crossterm events into these.

use std::fmt;
use std::str::FromStr;

/// The non-modifier part of a chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KeyCode {
    /// A printable key; ASCII letters are always stored lowercase (shift lives in [`Mods`]).
    Char(char),
    /// Function key `F1`..=`F24`.
    F(u8),
    Enter,
    Esc,
    Tab,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Mods {
    pub const NONE: Mods = Mods {
        ctrl: false,
        shift: false,
        alt: false,
    };
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct KeyChord {
    pub code: KeyCode,
    pub mods: Mods,
}

impl KeyChord {
    /// Builds a chord in canonical form (ASCII letters lowercased), so `Char('Z')`+shift from a
    /// terminal equals the parsed `"shift+z"`. With ctrl or alt, a US shifted symbol becomes
    /// shift plus its base key (`ctrl+{` is `ctrl+shift+[`), whichever way it was written or
    /// reported (kitty terminals may drop SHIFT for it); US layout, the one VS Code's default
    /// keys assume.
    pub fn new(code: KeyCode, mut mods: Mods) -> Self {
        let code = match code {
            KeyCode::Char(c) if mods.ctrl || mods.alt => {
                let base = unshift_us(c);
                mods.shift |= base != c;
                KeyCode::Char(base.to_ascii_lowercase())
            }
            KeyCode::Char(c) => KeyCode::Char(c.to_ascii_lowercase()),
            other => other,
        };
        KeyChord { code, mods }
    }
}

fn unshift_us(c: char) -> char {
    const SHIFTED: &str = "!@#$%^&*()_+{}|:\"<>?~";
    const BASE: &str = "1234567890-=[]\\;',./`";
    SHIFTED
        .chars()
        .position(|s| s == c)
        .and_then(|i| BASE.chars().nth(i))
        .unwrap_or(c)
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyParseError {
    #[error("empty key chord")]
    Empty,
    #[error("missing key after modifiers")]
    MissingKey,
    #[error("unknown modifier {0:?}")]
    UnknownModifier(String),
    #[error("modifier {0:?} given twice")]
    DuplicateModifier(String),
    #[error("unknown key {0:?}")]
    UnknownKey(String),
}

fn named_key(name: &str) -> Option<KeyCode> {
    Some(match name {
        "enter" | "return" => KeyCode::Enter,
        "esc" | "escape" => KeyCode::Esc,
        "tab" => KeyCode::Tab,
        "backspace" => KeyCode::Backspace,
        "delete" | "del" => KeyCode::Delete,
        "insert" | "ins" => KeyCode::Insert,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" | "pgup" => KeyCode::PageUp,
        "pagedown" | "pgdn" => KeyCode::PageDown,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "space" => KeyCode::Char(' '),
        // '+' is the separator, so the plus key needs a name.
        "plus" => KeyCode::Char('+'),
        _ => {
            let n = name.strip_prefix('f')?.parse::<u8>().ok()?;
            if (1..=24).contains(&n) {
                KeyCode::F(n)
            } else {
                return None;
            }
        }
    })
}

fn parse_key(raw: &str) -> Result<KeyCode, KeyParseError> {
    // A bare space survives only as the key itself ("ctrl+ "); anything else is trimmed.
    if raw == " " {
        return Ok(KeyCode::Char(' '));
    }
    let key = raw.trim();
    if key.is_empty() {
        return Err(KeyParseError::MissingKey);
    }
    let mut chars = key.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return Ok(KeyCode::Char(c.to_ascii_lowercase()));
    }
    let lower = key.to_ascii_lowercase();
    named_key(&lower).ok_or(KeyParseError::UnknownKey(key.to_owned()))
}

impl FromStr for KeyChord {
    type Err = KeyParseError;

    /// Parses `"ctrl+shift+f"`, `"shift+alt+down"`, `"ctrl+/"`, `"f12"`; modifiers in any order,
    /// case-insensitive, spaces around parts ignored.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.trim().is_empty() && s != " " {
            return Err(KeyParseError::Empty);
        }
        let parts: Vec<&str> = s.split('+').collect();
        let (key, mod_parts) = parts.split_last().ok_or(KeyParseError::Empty)?;
        let mut mods = Mods::NONE;
        for m in mod_parts {
            let name = m.trim().to_ascii_lowercase();
            let slot = match name.as_str() {
                "ctrl" | "control" => &mut mods.ctrl,
                "shift" => &mut mods.shift,
                "alt" | "option" => &mut mods.alt,
                "" => return Err(KeyParseError::MissingKey),
                _ => return Err(KeyParseError::UnknownModifier(m.trim().to_owned())),
            };
            if *slot {
                return Err(KeyParseError::DuplicateModifier(name));
            }
            *slot = true;
        }
        let code = parse_key(key)?;
        Ok(KeyChord::new(code, mods))
    }
}

impl fmt::Display for KeyCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            KeyCode::Char(' ') => "space",
            KeyCode::Char('+') => "plus",
            KeyCode::Char(c) => return write!(f, "{c}"),
            KeyCode::F(n) => return write!(f, "f{n}"),
            KeyCode::Enter => "enter",
            KeyCode::Esc => "escape",
            KeyCode::Tab => "tab",
            KeyCode::Backspace => "backspace",
            KeyCode::Delete => "delete",
            KeyCode::Insert => "insert",
            KeyCode::Home => "home",
            KeyCode::End => "end",
            KeyCode::PageUp => "pageup",
            KeyCode::PageDown => "pagedown",
            KeyCode::Up => "up",
            KeyCode::Down => "down",
            KeyCode::Left => "left",
            KeyCode::Right => "right",
        };
        f.write_str(name)
    }
}

impl fmt::Display for KeyChord {
    /// Canonical VS Code order: `ctrl+shift+alt+<key>`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.mods.ctrl {
            f.write_str("ctrl+")?;
        }
        if self.mods.shift {
            f.write_str("shift+")?;
        }
        if self.mods.alt {
            f.write_str("alt+")?;
        }
        write!(f, "{}", self.code)
    }
}
