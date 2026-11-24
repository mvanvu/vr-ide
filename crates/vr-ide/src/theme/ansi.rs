//! The colours a program in the integrated terminal (M5) sees: the 16 ANSI colours and the
//! default foreground / background. `synthwave` and `dracula` bring their own so the panel
//! matches the editor; `dark` and `light` have none and leave the terminal's own colours.

use ratatui::style::Color;

use super::{ColorDepth, Theme};

/// ANSI colours 0–15 plus the terminal's default foreground and background.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnsiPalette {
    pub fg: Color,
    pub bg: Color,
    /// black, red, green, yellow, blue, magenta, cyan, white, then the bright eight.
    pub colors: [Color; 16],
}

impl Theme {
    /// The theme's terminal palette; `None` keeps the user's terminal colours (`dark`, `light`).
    pub fn ansi_palette(&self) -> Option<&AnsiPalette> {
        self.ansi.as_ref()
    }
}

/// Builds a palette: `fg` / `bg` are the editor's own text colours (already fitted to the depth),
/// the ANSI table is converted for the depth.
pub(super) fn palette(depth: ColorDepth, fg: Color, bg: Color, table: &[u32; 16]) -> AnsiPalette {
    AnsiPalette {
        fg,
        bg,
        colors: table.map(|rgb| depth.color(rgb)),
    }
}

/// SynthWave '84 tones from the theme's own palette; black is the raised surface so it still
/// shows on the editor background, bright black the comment purple.
pub(super) const SYNTHWAVE_ANSI: [u32; 16] = [
    0x2f1c49, 0xff6e7f, 0x72f1b8, 0xffe66d, 0x82aaff, 0xff7edb, 0x2de2e6, 0xe0d6f0, //
    0x8068b0, 0xff8a96, 0x9cf6cf, 0xfff09a, 0xa9c4ff, 0xffa3e6, 0x7ef0f2, 0xffffff,
];

/// The published Dracula terminal colours.
pub(super) const DRACULA_ANSI: [u32; 16] = [
    0x21222c, 0xff5555, 0x50fa7b, 0xf1fa8c, 0xbd93f9, 0xff79c6, 0x8be9fd, 0xf8f8f2, //
    0x6272a4, 0xff6e6e, 0x69ff94, 0xffffa5, 0xd6acff, 0xff92df, 0xa4ffff, 0xffffff,
];
