//! Explorer name colours (AC-F5): one token per kind of entry, told apart at a glance.

use ratatui::style::{Color, Modifier, Style};

use super::{ColorDepth, fit_floors};

/// What an explorer row is, for colouring its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExplorerKind {
    Folder,
    /// Compiled and systems languages (Rust, C, Go, Java, ...).
    CodeNative,
    /// Scripting, web and markup languages (JS, TS, Python, PHP, HTML, CSS, shell, ...).
    CodeScript,
    /// Config and data files (toml, json, yaml, ...).
    Config,
    /// Documents and plain text.
    Docs,
    /// Images, audio, video, fonts.
    Media,
    /// Archives and binaries.
    Archive,
    /// Lock files and generated output.
    Generated,
    /// Dotfiles without a more specific kind.
    Hidden,
    /// Anything else: the plain text colour.
    Plain,
}

impl ExplorerKind {
    pub const ALL: [ExplorerKind; 10] = [
        ExplorerKind::Folder,
        ExplorerKind::CodeNative,
        ExplorerKind::CodeScript,
        ExplorerKind::Config,
        ExplorerKind::Docs,
        ExplorerKind::Media,
        ExplorerKind::Archive,
        ExplorerKind::Generated,
        ExplorerKind::Hidden,
        ExplorerKind::Plain,
    ];
}

pub(super) type Table = [Style; ExplorerKind::ALL.len()];

/// A table from a style per kind.
pub(super) fn table(style: impl Fn(ExplorerKind) -> Style) -> Table {
    let mut out = [Style::default(); ExplorerKind::ALL.len()];
    for k in ExplorerKind::ALL {
        out[k as usize] = style(k);
    }
    out
}

/// Colours in `ExplorerKind` order (without `Plain`): 4.5:1 on the explorer background, 3:1 on
/// the selection row, where the name stays coloured. `Plain` uses `text`; the focused row uses
/// the selected-row foreground and keeps no kind colour.
pub(super) fn fitted(
    depth: ColorDepth,
    rgb: [u32; 9],
    text: Style,
    bg: Color,
    line: Color,
) -> Table {
    let floors = [(bg, 4.5), (line, 3.0)];
    table(|k| match k {
        ExplorerKind::Plain => text,
        _ => {
            let style = Style::default().fg(fit_floors(depth, rgb[k as usize], &floors));
            if k == ExplorerKind::Hidden {
                style.add_modifier(Modifier::ITALIC)
            } else {
                style
            }
        }
    })
}

/// ANSI colours for `dark` / `light`: the user's terminal palette defines them.
pub(super) fn ansi(colours: [Color; 9], hidden_italic: bool) -> Table {
    table(|k| match k {
        ExplorerKind::Plain => Style::default(),
        _ => {
            let style = Style::default().fg(colours[k as usize]);
            if k == ExplorerKind::Hidden && hidden_italic {
                style.add_modifier(Modifier::ITALIC)
            } else {
                style
            }
        }
    })
}
