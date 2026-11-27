//! The colours a program in the integrated terminal (M5) sees: the 16 ANSI colours and the
//! default foreground / background. Every theme draws the panel as black with Homebrew-green
//! text (FR-32); `synthwave` and `dracula` bring their own ANSI table so programs match the
//! editor, `dark` and `light` share a neutral one fitted to black.

use ratatui::style::Color;

use super::{ColorDepth, Theme, fit};

/// ANSI colours 0–15 plus the terminal's default foreground and background.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnsiPalette {
    pub fg: Color,
    pub bg: Color,
    /// black, red, green, yellow, blue, magenta, cyan, white, then the bright eight.
    pub colors: [Color; 16],
}

impl Theme {
    /// The theme's terminal palette: every built-in theme has one; `None` (tests only) leaves
    /// programs' colours to the host terminal.
    pub fn ansi_palette(&self) -> Option<&AnsiPalette> {
        self.ansi.as_ref()
    }
}

/// Pure black panel background; xterm index 16 is the cube's #000000, so it stays black rather
/// than grey on a 256-colour terminal.
const PANEL_BG: (u32, u8) = (0x000000, 16);
/// Text colour of the macOS Terminal "Homebrew" profile.
const PANEL_FG: u32 = 0x28fe14;
/// Dark green selection: clearly lifted from black, and the green text still reads on it.
const PANEL_SELECTION: (u32, u8) = (0x005f00, 22);

/// Default colours of the terminal panel at `depth`: text, background, selected-text background.
pub(super) fn panel_colors(depth: ColorDepth) -> (Color, Color, Color) {
    let surface = |s: (u32, u8)| match depth {
        ColorDepth::TrueColor => depth.color(s.0),
        ColorDepth::Indexed256 => Color::Indexed(s.1),
    };
    let bg = surface(PANEL_BG);
    (
        fit(depth, PANEL_FG, &[bg], 7.0),
        bg,
        surface(PANEL_SELECTION),
    )
}

/// ANSI table of `dark` and `light`: tones that read on the black panel, with black and bright
/// black lifted so they still show on it.
pub(super) const NEUTRAL_ANSI: [u32; 16] = [
    0x585858, 0xff5f5f, 0x5fd75f, 0xd7d75f, 0x5f87ff, 0xd75fd7, 0x5fd7d7, 0xd0d0d0, //
    0x8a8a8a, 0xff8787, 0x87ff87, 0xffff87, 0x87afff, 0xff87ff, 0x87ffff, 0xffffff,
];

/// Builds a palette: `fg` / `bg` are the panel's default colours (already fitted to the depth),
/// the ANSI table is converted for the depth.
pub(super) fn palette(depth: ColorDepth, fg: Color, bg: Color, table: &[u32; 16]) -> AnsiPalette {
    AnsiPalette {
        fg,
        bg,
        colors: table.map(|rgb| depth.color(rgb)),
    }
}

/// SynthWave '84 tones from the theme's own palette; black is a lifted violet so it still shows
/// on the black panel, bright black the comment purple.
pub(super) const SYNTHWAVE_ANSI: [u32; 16] = [
    0x5a4a82, 0xff6e7f, 0x72f1b8, 0xffe66d, 0x82aaff, 0xff7edb, 0x2de2e6, 0xe0d6f0, //
    0x8068b0, 0xff8a96, 0x9cf6cf, 0xfff09a, 0xa9c4ff, 0xffa3e6, 0x7ef0f2, 0xffffff,
];

/// The published Dracula terminal colours, with black lifted to Dracula's line grey so it shows
/// on the black panel.
pub(super) const DRACULA_ANSI: [u32; 16] = [
    0x44475a, 0xff5555, 0x50fa7b, 0xf1fa8c, 0xbd93f9, 0xff79c6, 0x8be9fd, 0xf8f8f2, //
    0x6272a4, 0xff6e6e, 0x69ff94, 0xffffa5, 0xd6acff, 0xff92df, 0xa4ffff, 0xffffff,
];
