//! Colour palette: ratatui colours to 0RGB pixels.

use ratatui::style::Color;

/// A 0RGB pixel colour (`0x00RRGGBB`), the format softbuffer presents.
pub type Rgb = u32;

/// Builds an 0RGB pixel from its channels.
#[must_use]
pub const fn rgb(r: u8, g: u8, b: u8) -> Rgb {
    ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

/// The 16 named colours (xterm defaults), indexes 0..16 of the 256-colour table.
const NAMED: [Rgb; 16] = [
    0x00_0000, 0xcd_0000, 0x00_cd00, 0xcd_cd00, 0x00_00ee, 0xcd_00cd, 0x00_cdcd, 0xe5_e5e5,
    0x7f_7f7f, 0xff_0000, 0x00_ff00, 0xff_ff00, 0x5c_5cff, 0xff_00ff, 0x00_ffff, 0xff_ffff,
];

/// Theme colours for everything ratatui does not give as `Color::Rgb`.
///
/// `Color::Rgb` is drawn as is; the named colours are indexes 0..16 of `indexed`; `Color::Reset`
/// takes `foreground` / `background`. The cursor uses `cursor`, or the cell's foreground.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub indexed: [Rgb; 256],
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Option<Rgb>,
}

impl Default for Palette {
    fn default() -> Self {
        let mut indexed = [0; 256];
        indexed[..16].copy_from_slice(&NAMED);
        // xterm 6x6x6 cube, then the 24-step grey ramp.
        const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
        for i in 0..216 {
            indexed[16 + i] = rgb(LEVELS[i / 36], LEVELS[(i / 6) % 6], LEVELS[i % 6]);
        }
        for i in 0..24u8 {
            let v = 8 + 10 * i;
            indexed[232 + usize::from(i)] = rgb(v, v, v);
        }
        Self {
            indexed,
            foreground: 0xe5_e5e5,
            background: 0x1e_1e1e,
            cursor: None,
        }
    }
}

impl Palette {
    /// The pixel colour of a foreground `Color`.
    #[must_use]
    pub fn fg(&self, color: Color) -> Rgb {
        self.resolve(color).unwrap_or(self.foreground)
    }

    /// The pixel colour of a background `Color`.
    #[must_use]
    pub fn bg(&self, color: Color) -> Rgb {
        self.resolve(color).unwrap_or(self.background)
    }

    /// `None` for `Color::Reset`.
    fn resolve(&self, color: Color) -> Option<Rgb> {
        let index = match color {
            Color::Reset => return None,
            Color::Rgb(r, g, b) => return Some(rgb(r, g, b)),
            Color::Indexed(i) => i,
            Color::Black => 0,
            Color::Red => 1,
            Color::Green => 2,
            Color::Yellow => 3,
            Color::Blue => 4,
            Color::Magenta => 5,
            Color::Cyan => 6,
            Color::Gray => 7,
            Color::DarkGray => 8,
            Color::LightRed => 9,
            Color::LightGreen => 10,
            Color::LightYellow => 11,
            Color::LightBlue => 12,
            Color::LightMagenta => 13,
            Color::LightCyan => 14,
            Color::White => 15,
        };
        Some(self.indexed[usize::from(index)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_is_drawn_as_is_and_reset_takes_the_theme_colours() {
        let p = Palette::default();
        assert_eq!(p.fg(Color::Rgb(0x12, 0x34, 0x56)), 0x12_3456);
        assert_eq!(p.fg(Color::Reset), p.foreground);
        assert_eq!(p.bg(Color::Reset), p.background);
    }

    #[test]
    fn named_and_indexed_colours_come_from_the_table_and_can_be_overridden() {
        let mut p = Palette::default();
        assert_eq!(p.fg(Color::Red), 0xcd_0000);
        assert_eq!(p.fg(Color::White), 0xff_ffff);
        assert_eq!(p.fg(Color::Indexed(16)), 0);
        assert_eq!(p.fg(Color::Indexed(231)), 0xff_ffff);
        assert_eq!(p.fg(Color::Indexed(232)), rgb(8, 8, 8));
        assert_eq!(p.fg(Color::Indexed(255)), rgb(238, 238, 238));
        p.indexed[1] = 0xff_5555;
        assert_eq!(p.bg(Color::Red), 0xff_5555);
        assert_eq!(p.bg(Color::Indexed(1)), 0xff_5555);
    }
}
