//! Candidate palettes for the "bolder theme" choice (preview and contrast tests only; nothing
//! here ships). Built on the same `Theme` fields and `fit` machinery as `Theme::dracula_with`.

use ratatui::style::{Color, Modifier, Style};

use super::{ColorDepth, Theme, fit};

/// A surface: the true-colour value and a hand-picked xterm-256 index for the fallback.
type Surface = (u32, u8);

struct Palette {
    bg: Surface,
    bar: Surface,
    raised: Surface,
    line: Surface,
    input: Surface,
    picked: Surface,
    found: Surface,
    fg: u32,
    comment: u32,
    dim: u32,
    cyan: u32,
    green: u32,
    orange: u32,
    pink: u32,
    purple: u32,
    red: u32,
    yellow: u32,
    /// Accent of the active tab and the selected button.
    active: u32,
    /// Accent of the popup frame.
    border: u32,
    /// Accent of the focused panel separator.
    focus: u32,
    /// Accent of the cursor line number.
    cursor_line: u32,
    /// Accent of notices at info level.
    info: u32,
}

fn build(p: &Palette, depth: ColorDepth) -> Theme {
    let surface = |s: Surface| match depth {
        ColorDepth::TrueColor => depth.color(s.0),
        ColorDepth::Indexed256 => Color::Indexed(s.1),
    };
    let (bg, bar, raised) = (surface(p.bg), surface(p.bar), surface(p.raised));
    let (line, input) = (surface(p.line), surface(p.input));
    let (picked, found) = (surface(p.picked), surface(p.found));
    let fit = |rgb, on: &[Color], min| fit(depth, rgb, on, min);
    let fg = fit(p.fg, &[bg, bar, raised, line, input, picked, found], 4.5);
    let comment = fit(p.comment, &[bg], 3.0);
    let dim = fit(p.dim, &[bg, bar, raised, line, picked], 3.0);
    let muted = fit(p.dim, &[bg, bar, raised], 4.5);
    let base = [bg, bar, raised];
    let accent = |rgb| fit(rgb, &base, 4.5);
    let orange = fit(p.orange, &[bg, bar, raised, line, found], 4.5);
    let yellow = fit(p.yellow, &[bg, raised, line, picked], 4.5);
    let red = accent(p.red);
    let active = accent(p.active);
    let (cyan, green, pink, purple) = (
        accent(p.cyan),
        accent(p.green),
        accent(p.pink),
        accent(p.purple),
    );
    let pick = |rgb: u32| match rgb {
        x if x == p.cyan => cyan,
        x if x == p.green => green,
        x if x == p.pink => pink,
        x if x == p.purple => purple,
        _ => accent(rgb),
    };
    let bold = Modifier::BOLD;
    Theme {
        text: Style::default().bg(bg).fg(fg),
        gutter: Style::default().fg(comment),
        gutter_current: Style::default().fg(dim),
        gutter_active: Style::default().fg(pick(p.cursor_line)).add_modifier(bold),
        selection: Style::default().bg(line),
        find_match: Style::default().bg(found),
        control: Style::default().fg(orange),
        tab_bar: Style::default().bg(bar),
        tab_active: Style::default().bg(bg).fg(active).add_modifier(bold),
        tab_inactive: Style::default().bg(bar).fg(muted),
        tab_modified: Style::default().fg(orange),
        tab_warning: Style::default().fg(red),
        status: Style::default().bg(bar).fg(fg),
        info: Style::default().fg(pick(p.info)),
        warning: Style::default().fg(orange),
        error: Style::default().fg(red).add_modifier(bold),
        dim: Style::default().fg(dim),
        popup: Style::default().bg(raised).fg(fg),
        popup_border: Style::default().fg(pick(p.border)),
        input: Style::default().bg(input).fg(fg),
        button: Style::default().fg(muted),
        button_selected: Style::default().bg(active).fg(bg).add_modifier(bold),
        list_selected: Style::default().bg(picked).fg(fg),
        matched: Style::default().fg(yellow).add_modifier(bold),
        focus: Style::default().fg(pick(p.focus)).add_modifier(bold),
    }
}

/// A: Dracula family, darker and punchier.
const MIDNIGHT: Palette = Palette {
    bg: (0x1b1c26, 234),
    bar: (0x0b0c12, 16),
    raised: (0x2a2c3f, 237),
    line: (0x3c405c, 239),
    input: (0x3c405c, 240),
    picked: (0x5a36a0, 60),
    found: (0x5a4a14, 58),
    fg: 0xf8f8f2,
    comment: 0x6f7bb0,
    dim: 0x9ea9d8,
    cyan: 0x5ee7ff,
    green: 0x5dff8a,
    orange: 0xffa94d,
    pink: 0xff5fb8,
    purple: 0xc792ff,
    red: 0xff6b6b,
    yellow: 0xf7ff7a,
    active: 0xff5fb8,
    border: 0xc792ff,
    focus: 0x5dff8a,
    cursor_line: 0x5ee7ff,
    info: 0x5ee7ff,
};

/// B: blue-black with neon cyan and magenta.
const NEON: Palette = Palette {
    bg: (0x0f111a, 233),
    bar: (0x010203, 16),
    raised: (0x19243a, 235),
    line: (0x263a57, 238),
    input: (0x2c4163, 240),
    picked: (0x0b5a78, 24),
    found: (0x5c4400, 58),
    fg: 0xe6edf3,
    comment: 0x5f7396,
    dim: 0x8fa5c9,
    cyan: 0x00e5ff,
    green: 0x39ff88,
    orange: 0xffb000,
    pink: 0xff2bd6,
    purple: 0xb48cff,
    red: 0xff5470,
    yellow: 0xffe94d,
    active: 0x00e5ff,
    border: 0xff2bd6,
    focus: 0x39ff88,
    cursor_line: 0xff2bd6,
    info: 0x00e5ff,
};

/// C: deep purple with hot pink, orange and cyan.
const SYNTHWAVE: Palette = Palette {
    bg: (0x1a1028, 234),
    bar: (0x0a0512, 16),
    raised: (0x2f1c49, 53),
    line: (0x43296a, 54),
    input: (0x4d3078, 55),
    picked: (0x6a2f96, 91),
    found: (0x5e3b0c, 94),
    fg: 0xf6eefc,
    comment: 0x8068b0,
    dim: 0xb9a6e6,
    cyan: 0x2de2e6,
    green: 0x72f1b8,
    orange: 0xff9d3c,
    pink: 0xff2e97,
    purple: 0xc77dff,
    red: 0xff6e7f,
    yellow: 0xffe66d,
    active: 0xff2e97,
    border: 0x2de2e6,
    focus: 0x2de2e6,
    cursor_line: 0xff9d3c,
    info: 0x2de2e6,
};

/// The candidates by preview file name, with the label shown on the comparison page.
pub(crate) const NAMES: [(&str, &str); 3] = [
    ("a-midnight", "A Midnight"),
    ("b-neon", "B Neon"),
    ("c-synthwave", "C Synthwave"),
];

pub(crate) fn by_file_name(name: &str, depth: ColorDepth) -> Theme {
    let p = match name {
        "a-midnight" => &MIDNIGHT,
        "b-neon" => &NEON,
        _ => &SYNTHWAVE,
    };
    build(p, depth)
}
