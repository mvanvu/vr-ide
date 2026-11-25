//! The `synthwave` theme: deep purple surfaces with hot pink, orange and cyan accents. Built on
//! the same `fit` machinery as `Theme::dracula_with`, driven by one palette table.

use ratatui::style::{Color, Modifier, Style};

use vr_syntax::Capture;

use super::{ColorDepth, Theme, fit, fit_floors, syntax_floors, syntax_table};

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
    /// Status bar badge of read mode and of edit mode (text is the editor background).
    read: u32,
    edit: u32,
}

pub(super) fn build(depth: ColorDepth) -> Theme {
    let p = &SYNTHWAVE;
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
    let (read, edit) = (accent(p.read), accent(p.edit));
    let bold = Modifier::BOLD;
    let floors = syntax_floors(bg, line, found);
    let code = |c: Capture| {
        let (rgb, modifier) = syntax_colour(c);
        Style::default()
            .fg(fit_floors(depth, rgb, &floors))
            .add_modifier(modifier)
    };
    let captures = syntax_table(code);
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
        ok: Style::default().fg(green),
        popup: Style::default().bg(raised).fg(fg),
        popup_border: Style::default().fg(pick(p.border)),
        input: Style::default().bg(input).fg(fg),
        button: Style::default().fg(muted),
        button_selected: Style::default().bg(active).fg(bg).add_modifier(bold),
        list_selected: Style::default().bg(picked).fg(fg),
        matched: Style::default().fg(yellow).add_modifier(bold),
        focus: Style::default().fg(pick(p.focus)).add_modifier(bold),
        mode_read: Style::default().bg(read).fg(bg).add_modifier(bold),
        mode_edit: Style::default().bg(edit).fg(bg).add_modifier(bold),
        syntax_on_highlight: true,
        captures,
        mode_term: Style::default().bg(pink).fg(bg).add_modifier(bold),
        git_added: Style::default().fg(green),
        git_modified: Style::default().fg(cyan),
        git_deleted: Style::default().fg(red),
        git_blame: Style::default().fg(dim).add_modifier(Modifier::ITALIC),
        ansi: Some(super::ansi::palette(
            depth,
            fg,
            bg,
            &super::ansi::SYNTHWAVE_ANSI,
        )),
    }
}

/// Syntax colours in the spirit of SynthWave '84: pink-lavender keywords, amber strings,
/// cyan functions, yellow types, green tags, blue property names. Each is light enough for
/// 4.5:1 on the editor and 3:1 on the selection and find-match surfaces (the comment and
/// keyword tones are lifted from the palette's comment purple and hot pink for that). Strings
/// are lighter than the orange of warnings and the EDIT badge so code does not read as chrome.
fn syntax_colour(c: Capture) -> (u32, Modifier) {
    let none = Modifier::empty();
    match c {
        Capture::Keyword | Capture::Operator => (0xff7edb, none),
        Capture::String => (0xffb454, none),
        Capture::Comment => (0x9884c9, Modifier::ITALIC),
        Capture::Function => (0x2de2e6, none),
        Capture::Type => (0xffe66d, none),
        Capture::Number | Capture::Constant => (0xf97e72, none),
        Capture::VariableBuiltin => (0xc77dff, Modifier::ITALIC),
        Capture::Property => (0x82aaff, none),
        Capture::Tag => (0x72f1b8, none),
        Capture::Attribute => (0xffe66d, Modifier::ITALIC),
        Capture::Punctuation => (0xb9a6e6, none),
        Capture::Escape => (0xff6e7f, Modifier::BOLD),
        Capture::Other => (0xf6eefc, none),
    }
}

const SYNTHWAVE: Palette = Palette {
    bg: (0x1a1028, 234),
    bar: (0x0a0512, 16),
    raised: (0x2f1c49, 53),
    line: (0x43296a, 54),
    // Between the editor background (find and search fields sit on it) and the selected row,
    // so it reads as a field on both and is told apart from the selected palette row.
    input: (0x3a2760, 54),
    picked: (0x7433a6, 92),
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
    read: 0x2de2e6,
    edit: 0xff9d3c,
};
