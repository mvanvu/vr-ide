//! Colours and text styles. `dark` and `light` start from the terminal's own colours
//! (`Color::Reset`) so the editor blends into the user's terminal theme; `synthwave` (the
//! default) and `dracula` paint their own background (`text` carries it and fills the whole frame first).

use ratatui::style::{Color, Modifier, Style};

mod synthwave;

/// The theme used when settings name none or an unknown one.
pub const DEFAULT_THEME: &str = "synthwave";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub text: Style,
    pub gutter: Style,
    pub gutter_current: Style,
    pub selection: Style,
    /// Placeholder drawn for control characters.
    pub control: Style,
    pub tab_bar: Style,
    pub tab_active: Style,
    pub tab_inactive: Style,
    pub tab_modified: Style,
    pub tab_warning: Style,
    pub status: Style,
    pub info: Style,
    pub warning: Style,
    pub error: Style,
    pub dim: Style,
    /// Dialogs and the palette.
    pub popup: Style,
    pub popup_border: Style,
    pub input: Style,
    pub button: Style,
    pub button_selected: Style,
    /// Selected palette row.
    pub list_selected: Style,
    /// Other matches of the find bar in the editor (the current one is selected).
    pub find_match: Style,
    /// Matched characters in palette rows.
    pub matched: Style,
    /// Separator of the focused side panel (explorer or search), so the user sees where keys go.
    pub focus: Style,
    /// Line number of the cursor line while the editor has the focus.
    pub gutter_active: Style,
    /// Status bar badge of the active tab's mode (D145): READ and EDIT, told apart by colour.
    pub mode_read: Style,
    pub mode_edit: Style,
}

impl Theme {
    /// The named theme, or `None` for an unknown name.
    pub fn named(name: &str) -> Option<Theme> {
        match name {
            "dark" => Some(Self::dark()),
            "light" => Some(Self::light()),
            "dracula" => Some(Self::dracula()),
            "synthwave" => Some(Self::synthwave()),
            _ => None,
        }
    }

    /// The named theme; unknown names fall back to the default (the startup notice reports them).
    pub fn from_name(name: &str) -> Theme {
        Self::named(name)
            .or_else(|| Self::named(DEFAULT_THEME))
            .unwrap_or_else(Self::dark)
    }

    pub fn dark() -> Theme {
        Theme {
            text: Style::default(),
            gutter: Style::default().fg(Color::DarkGray),
            gutter_current: Style::default().fg(Color::Gray),
            selection: Style::default().bg(Color::Indexed(239)),
            find_match: Style::default().bg(Color::Indexed(58)),
            control: Style::default().fg(Color::Red),
            tab_bar: Style::default().bg(Color::Indexed(235)),
            tab_active: Style::default()
                .bg(Color::Reset)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
            tab_inactive: Style::default().bg(Color::Indexed(235)).fg(Color::Gray),
            tab_modified: Style::default().fg(Color::Yellow),
            tab_warning: Style::default().fg(Color::Red),
            status: Style::default().bg(Color::Indexed(236)).fg(Color::Gray),
            info: Style::default().fg(Color::Cyan),
            warning: Style::default().fg(Color::Yellow),
            error: Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            dim: Style::default().fg(Color::DarkGray),
            popup: Style::default().bg(Color::Indexed(236)).fg(Color::White),
            popup_border: Style::default().fg(Color::Cyan),
            input: Style::default().bg(Color::Indexed(238)).fg(Color::White),
            button: Style::default().fg(Color::Gray),
            button_selected: Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
            list_selected: Style::default().bg(Color::Indexed(24)).fg(Color::White),
            matched: Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
            focus: Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
            gutter_active: Style::default().fg(Color::Gray),
            mode_read: Style::default()
                .bg(Color::Cyan)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
            mode_edit: Style::default()
                .bg(Color::Green)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        }
    }

    pub fn light() -> Theme {
        Theme {
            text: Style::default(),
            gutter: Style::default().fg(Color::Gray),
            gutter_current: Style::default().fg(Color::Black),
            selection: Style::default().bg(Color::Indexed(153)),
            find_match: Style::default().bg(Color::Indexed(229)),
            control: Style::default().fg(Color::Red),
            tab_bar: Style::default().bg(Color::Indexed(254)),
            tab_active: Style::default()
                .bg(Color::Reset)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
            tab_inactive: Style::default().bg(Color::Indexed(254)).fg(Color::DarkGray),
            tab_modified: Style::default().fg(Color::Indexed(130)),
            tab_warning: Style::default().fg(Color::Red),
            status: Style::default().bg(Color::Indexed(252)).fg(Color::Black),
            info: Style::default().fg(Color::Blue),
            warning: Style::default().fg(Color::Indexed(130)),
            error: Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            dim: Style::default().fg(Color::Gray),
            popup: Style::default().bg(Color::Indexed(254)).fg(Color::Black),
            popup_border: Style::default().fg(Color::Blue),
            input: Style::default().bg(Color::White).fg(Color::Black),
            button: Style::default().fg(Color::DarkGray),
            button_selected: Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
            list_selected: Style::default().bg(Color::Indexed(153)).fg(Color::Black),
            matched: Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
            focus: Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
            gutter_active: Style::default().fg(Color::Black),
            mode_read: Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
            mode_edit: Style::default()
                .bg(Color::Indexed(28))
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        }
    }
}

/// How many colours the terminal can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorDepth {
    TrueColor,
    /// The xterm 256-colour palette.
    Indexed256,
}

impl ColorDepth {
    /// Read once per run: the terminal does not change under us.
    pub fn detect() -> ColorDepth {
        static DEPTH: std::sync::OnceLock<ColorDepth> = std::sync::OnceLock::new();
        *DEPTH.get_or_init(|| Self::from_colorterm(std::env::var("COLORTERM").ok().as_deref()))
    }

    pub fn from_colorterm(value: Option<&str>) -> ColorDepth {
        match value {
            Some(v) if v.eq_ignore_ascii_case("truecolor") || v.eq_ignore_ascii_case("24bit") => {
                ColorDepth::TrueColor
            }
            _ => ColorDepth::Indexed256,
        }
    }

    /// `0xRRGGBB` as a true colour, or its nearest xterm-256 index.
    pub fn color(self, rgb: u32) -> Color {
        let (r, g, b) = ((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
        match self {
            ColorDepth::TrueColor => Color::Rgb(r, g, b),
            ColorDepth::Indexed256 => Color::Indexed(nearest_256(r, g, b)),
        }
    }
}

/// Component levels of the xterm 6x6x6 colour cube.
const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// RGB of an xterm-256 index in 16..=255 (the first 16 depend on the user's terminal theme).
pub(crate) fn indexed_rgb(i: u8) -> (u8, u8, u8) {
    if i >= 232 {
        let v = 8 + 10 * (i - 232);
        (v, v, v)
    } else {
        let n = usize::from(i.saturating_sub(16));
        (LEVELS[n / 36], LEVELS[n / 6 % 6], LEVELS[n % 6])
    }
}

/// RGB of a theme colour; terminal-defined colours (named, reset) have no known RGB.
pub(crate) fn to_rgb(c: Color) -> Option<(u8, u8, u8)> {
    match c {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        Color::Indexed(i) if i >= 16 => Some(indexed_rgb(i)),
        _ => None,
    }
}

/// WCAG relative luminance.
pub(crate) fn luminance((r, g, b): (u8, u8, u8)) -> f64 {
    let f = |v: u8| {
        let v = f64::from(v) / 255.0;
        if v <= 0.03928 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b)
}

/// WCAG contrast ratio between two colours.
pub(crate) fn contrast(a: (u8, u8, u8), b: (u8, u8, u8)) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Nearest of the 6x6x6 cube and the grey ramp (indices 16..=255; the first 16 are left alone
/// because terminals let users redefine them).
fn nearest_256(r: u8, g: u8, b: u8) -> u8 {
    let dist = |a: (u8, u8, u8)| {
        let d = |x: u8, y: u8| (i32::from(x) - i32::from(y)).pow(2);
        d(a.0, r) + d(a.1, g) + d(a.2, b)
    };
    let level = |v: u8| {
        (0..6)
            .min_by_key(|&i| (i32::from(LEVELS[i]) - i32::from(v)).abs())
            .unwrap_or(0)
    };
    let (ci, cj, ck) = (level(r), level(g), level(b));
    let cube = (16 + 36 * ci + 6 * cj + ck) as u8;
    let cube_d = dist((LEVELS[ci], LEVELS[cj], LEVELS[ck]));
    let avg = (u16::from(r) + u16::from(g) + u16::from(b)) / 3;
    let gi = (0..24u16)
        .min_by_key(|&i| (8 + 10 * i as i32 - avg as i32).abs())
        .unwrap_or(0);
    let gv = (8 + 10 * gi) as u8;
    if dist((gv, gv, gv)) < cube_d {
        (232 + gi) as u8
    } else {
        cube
    }
}

impl Theme {
    /// Deep purple with hot pink, orange and cyan: the default theme.
    pub fn synthwave() -> Theme {
        Self::synthwave_with(ColorDepth::detect())
    }

    pub fn synthwave_with(depth: ColorDepth) -> Theme {
        synthwave::build(depth)
    }

    /// Dracula palette on its own background.
    pub fn dracula() -> Theme {
        Self::dracula_with(ColorDepth::detect())
    }

    pub fn dracula_with(depth: ColorDepth) -> Theme {
        // Surfaces: the published colour in true colour, a hand-picked grey/tint index in the
        // 256-colour fallback (nearest-index mapping would merge the dark greys).
        let surface = |rgb: u32, idx: u8| match depth {
            ColorDepth::TrueColor => depth.color(rgb),
            ColorDepth::Indexed256 => Color::Indexed(idx),
        };
        let bg = surface(0x282a36, 236);
        let bar = surface(0x21222c, 234);
        let raised = surface(0x343746, 238);
        let line = surface(0x44475a, 239);
        let input = surface(0x44475a, 241);
        let picked = surface(0x4a3f7a, 60);
        let found = surface(0x55501e, 58);
        // Foregrounds keep the published colour in true colour; the fallback takes the nearest
        // index that still reads on the surfaces the colour is drawn on.
        let fit = |rgb, on: &[Color], min| fit(depth, rgb, on, min);
        let fg = fit(
            0xf8f8f2,
            &[bg, bar, raised, line, input, picked, found],
            4.5,
        );
        let comment = fit(0x6272a4, &[bg], 3.0);
        // Lighter tone of the comment colour so it reads on every surface.
        let dim = fit(0x9aa8d4, &[bg, bar, raised, line, picked], 3.0);
        let muted = fit(0x9aa8d4, &[bg, bar, raised], 4.5);
        let cyan = fit(0x8be9fd, &[bg, bar], 4.5);
        let green = fit(0x50fa7b, &[bg], 4.5);
        let orange = fit(0xffb86c, &[bg, bar, raised, line, found], 4.5);
        let pink = fit(0xff79c6, &[bg], 4.5);
        let purple = fit(0xbd93f9, &[bg, bar, raised], 4.5);
        // The palette red, tinted lighter so it also reads on the popup.
        let red = fit(0xff7878, &[bg, bar, raised], 4.5);
        let yellow = fit(0xf1fa8c, &[bg, raised, line, picked], 4.5);
        Theme {
            text: Style::default().bg(bg).fg(fg),
            gutter: Style::default().fg(comment),
            gutter_current: Style::default().fg(dim),
            gutter_active: Style::default().fg(pink).add_modifier(Modifier::BOLD),
            selection: Style::default().bg(line),
            find_match: Style::default().bg(found),
            control: Style::default().fg(orange),
            tab_bar: Style::default().bg(bar),
            tab_active: Style::default()
                .bg(bg)
                .fg(purple)
                .add_modifier(Modifier::BOLD),
            tab_inactive: Style::default().bg(bar).fg(muted),
            tab_modified: Style::default().fg(orange),
            tab_warning: Style::default().fg(red),
            status: Style::default().bg(bar).fg(fg),
            info: Style::default().fg(cyan),
            warning: Style::default().fg(orange),
            error: Style::default().fg(red).add_modifier(Modifier::BOLD),
            dim: Style::default().fg(dim),
            popup: Style::default().bg(raised).fg(fg),
            popup_border: Style::default().fg(purple),
            input: Style::default().bg(input).fg(fg),
            button: Style::default().fg(muted),
            button_selected: Style::default()
                .bg(purple)
                .fg(bg)
                .add_modifier(Modifier::BOLD),
            list_selected: Style::default().bg(picked).fg(fg),
            matched: Style::default().fg(yellow).add_modifier(Modifier::BOLD),
            focus: Style::default().fg(green).add_modifier(Modifier::BOLD),
            // Badges in the palette colours with the editor background as text: purple for
            // reading (like the active tab), green for typing.
            mode_read: Style::default()
                .bg(purple)
                .fg(bg)
                .add_modifier(Modifier::BOLD),
            mode_edit: Style::default()
                .bg(green)
                .fg(bg)
                .add_modifier(Modifier::BOLD),
        }
    }
}

/// `rgb` as a colour that reads at `min` contrast on every surface in `on`: the palette value in
/// true colour, otherwise the nearest 256-colour index that does (the best-contrast index when
/// none does).
fn fit(depth: ColorDepth, rgb: u32, on: &[Color], min: f64) -> Color {
    if depth == ColorDepth::TrueColor {
        return depth.color(rgb);
    }
    let want = ((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8);
    let surfaces: Vec<_> = on.iter().filter_map(|&c| to_rgb(c)).collect();
    let worst = |i: u8| {
        surfaces
            .iter()
            .map(|&s| contrast(indexed_rgb(i), s))
            .fold(f64::MAX, f64::min)
    };
    let dist = |i: u8| {
        let (r, g, b) = indexed_rgb(i);
        let d = |x: u8, y: u8| (i32::from(x) - i32::from(y)).pow(2);
        d(r, want.0) + d(g, want.1) + d(b, want.2)
    };
    let best = (16..=255u8)
        .filter(|&i| worst(i) >= min)
        .min_by_key(|&i| dist(i))
        .or_else(|| (16..=255u8).max_by(|&a, &b| worst(a).total_cmp(&worst(b))));
    Color::Indexed(best.unwrap_or(231))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOTH: [ColorDepth; 2] = [ColorDepth::TrueColor, ColorDepth::Indexed256];

    fn rgb(c: Color) -> (u8, u8, u8) {
        to_rgb(c).unwrap_or_else(|| panic!("no rgb for {c:?}"))
    }

    fn ratio(a: Color, b: Color) -> f64 {
        contrast(rgb(a), rgb(b))
    }

    fn styles(t: &Theme) -> Vec<Style> {
        vec![
            t.text,
            t.gutter,
            t.gutter_current,
            t.selection,
            t.control,
            t.tab_bar,
            t.tab_active,
            t.tab_inactive,
            t.tab_modified,
            t.tab_warning,
            t.status,
            t.info,
            t.warning,
            t.error,
            t.dim,
            t.popup,
            t.popup_border,
            t.input,
            t.button,
            t.button_selected,
            t.list_selected,
            t.find_match,
            t.matched,
            t.focus,
            t.gutter_active,
            t.mode_read,
            t.mode_edit,
        ]
    }

    #[test]
    fn colorterm_decides_the_depth() {
        let d = ColorDepth::from_colorterm;
        assert_eq!(d(Some("truecolor")), ColorDepth::TrueColor);
        assert_eq!(d(Some("TrueColor")), ColorDepth::TrueColor);
        assert_eq!(d(Some("24bit")), ColorDepth::TrueColor);
        assert_eq!(d(Some("yes")), ColorDepth::Indexed256);
        assert_eq!(d(None), ColorDepth::Indexed256);
    }

    #[test]
    fn dracula_is_named_and_unknown_names_are_not() {
        assert!(Theme::named("dracula").is_some());
        assert!(Theme::named("draculaa").is_none());
        assert!(Theme::named("dark").is_some() && Theme::named("light").is_some());
    }

    #[test]
    fn unknown_names_fall_back_to_the_default_theme() {
        assert_eq!(DEFAULT_THEME, "synthwave");
        assert_eq!(Theme::from_name("nope"), Theme::from_name(DEFAULT_THEME));
    }

    #[test]
    fn dracula_paints_the_published_palette_in_true_colour() {
        let t = Theme::dracula_with(ColorDepth::TrueColor);
        let rgb = |c: u32| Some(Color::Rgb((c >> 16) as u8, (c >> 8) as u8, c as u8));
        assert_eq!(t.text.bg, rgb(0x282a36));
        assert_eq!(t.text.fg, rgb(0xf8f8f2));
        assert_eq!(t.selection.bg, rgb(0x44475a));
        assert_eq!(t.input.bg, rgb(0x44475a));
        assert_eq!(t.gutter.fg, rgb(0x6272a4));
        assert_eq!(t.info.fg, rgb(0x8be9fd));
        assert_eq!(t.warning.fg, rgb(0xffb86c));
        assert_eq!(t.tab_modified.fg, rgb(0xffb86c));
        assert_eq!(t.control.fg, rgb(0xffb86c));
        assert_eq!(t.matched.fg, rgb(0xf1fa8c));
        assert_eq!(t.tab_active.fg, rgb(0xbd93f9));
        assert_eq!(t.popup_border.fg, rgb(0xbd93f9));
        assert_eq!(t.button_selected.bg, rgb(0xbd93f9));
        assert_eq!(t.gutter_active.fg, rgb(0xff79c6));
        assert_eq!(t.focus.fg, rgb(0x50fa7b));
        assert_eq!(t.tab_bar.bg, rgb(0x21222c));
        assert_eq!(t.status.bg, rgb(0x21222c));
        assert_eq!(t.popup.bg, rgb(0x343746));
        assert_eq!(t.list_selected.bg, rgb(0x4a3f7a));
        assert_eq!(t.find_match.bg, rgb(0x55501e));
        assert_eq!(t.dim.fg, rgb(0x9aa8d4));
        // Error red is the palette red tinted lighter so it reads on the popup too.
        assert_eq!(t.error.fg, rgb(0xff7878));
        assert_eq!(t.tab_warning.fg, t.error.fg);
    }

    #[test]
    fn dracula_without_true_colour_uses_only_indexed_colours() {
        let t = Theme::dracula_with(ColorDepth::Indexed256);
        for s in styles(&t) {
            for c in [s.fg, s.bg].into_iter().flatten() {
                assert!(matches!(c, Color::Indexed(16..=255)), "{c:?}");
            }
        }
        assert_eq!(t.text.bg, Some(Color::Indexed(236)));
        // Pure cube corners and greys map exactly.
        assert_eq!(nearest_256(255, 255, 255), 231);
        assert_eq!(nearest_256(0, 0, 0), 16);
        assert_eq!(nearest_256(255, 0, 0), 196);
        // A middle cube level (135) and the table that pins the cube levels.
        assert_eq!(nearest_256(135, 95, 215), 98);
        assert_eq!(indexed_rgb(98), (135, 95, 215));
        assert_eq!(indexed_rgb(244), (128, 128, 128));
    }

    #[test]
    fn notice_levels_and_roles_are_distinct() {
        for depth in BOTH {
            let t = Theme::dracula_with(depth);
            assert_ne!(t.info.fg, t.warning.fg);
            assert_ne!(t.warning.fg, t.error.fg);
            assert_ne!(t.info.fg, t.error.fg);
            assert_ne!(t.selection.bg, t.list_selected.bg);
            assert_ne!(t.focus.fg, t.dim.fg);
        }
    }

    /// Every foreground-on-background pair the UI draws, with its floor: body text 4.5:1,
    /// dim and gutter text 3:1.
    fn pairs(t: &Theme) -> Vec<(&'static str, Color, Color, f64)> {
        let fg = |s: Style| s.fg.unwrap();
        let bg = |s: Style| s.bg.unwrap();
        let editor = bg(t.text);
        let mut v = vec![];
        for (name, surface) in [
            ("text/editor", editor),
            ("text/selection", bg(t.selection)),
            ("text/find_match", bg(t.find_match)),
            ("text/list_selected", bg(t.list_selected)),
            ("text/popup", bg(t.popup)),
            ("text/bar", bg(t.status)),
        ] {
            v.push((name, fg(t.text), surface, 4.5));
        }
        v.push(("input", fg(t.input), bg(t.input), 4.5));
        v.push(("status", fg(t.status), bg(t.status), 4.5));
        v.push(("tab_active", fg(t.tab_active), bg(t.tab_active), 4.5));
        v.push(("tab_inactive", fg(t.tab_inactive), bg(t.tab_inactive), 4.5));
        for (name, s) in [
            ("info", t.info),
            ("warning", t.warning),
            ("error", t.error),
            ("tab_modified", t.tab_modified),
            ("tab_warning", t.tab_warning),
        ] {
            v.push((name, fg(s), editor, 4.5));
            v.push((name, fg(s), bg(t.tab_bar), 4.5));
        }
        v.push(("error/popup", fg(t.error), bg(t.popup), 4.5));
        v.push(("error/status", fg(t.error), bg(t.status), 4.5));
        for (name, s) in [("control", t.control), ("warning", t.warning)] {
            for surface in [bg(t.selection), bg(t.find_match)] {
                v.push((name, fg(s), surface, 4.5));
            }
        }
        for surface in [editor, bg(t.selection), bg(t.list_selected), bg(t.popup)] {
            v.push(("matched", fg(t.matched), surface, 4.5));
        }
        v.push(("button", fg(t.button), bg(t.popup), 4.5));
        v.push((
            "button_selected",
            fg(t.button_selected),
            bg(t.button_selected),
            4.5,
        ));
        v.push(("focus", fg(t.focus), editor, 4.5));
        v.push(("gutter_active", fg(t.gutter_active), editor, 4.5));
        v.push(("mode_read", fg(t.mode_read), bg(t.mode_read), 4.5));
        v.push(("mode_edit", fg(t.mode_edit), bg(t.mode_edit), 4.5));
        v.push(("popup_border", fg(t.popup_border), bg(t.popup), 4.5));
        for (name, surface) in [
            ("dim/editor", editor),
            ("dim/selection", bg(t.selection)),
            ("dim/list_selected", bg(t.list_selected)),
            ("dim/popup", bg(t.popup)),
            ("dim/status", bg(t.status)),
            ("dim/tab_bar", bg(t.tab_bar)),
        ] {
            v.push((name, fg(t.dim), surface, 3.0));
        }
        v.push(("gutter", fg(t.gutter), editor, 3.0));
        v.push(("gutter_current", fg(t.gutter_current), editor, 3.0));
        v
    }

    #[test]
    fn dracula_contrast_meets_the_floor_in_both_depths() {
        for depth in BOTH {
            let t = Theme::dracula_with(depth);
            for (name, f, b, floor) in pairs(&t) {
                assert!(
                    ratio(f, b) >= floor,
                    "{depth:?} {name}: {f:?} on {b:?} is {:.2}",
                    ratio(f, b)
                );
            }
        }
    }

    #[test]
    fn synthwave_is_named() {
        assert_eq!(Theme::named("synthwave"), Some(Theme::synthwave()));
        assert!(Theme::named("dracula").is_some());
    }

    #[test]
    fn synthwave_contrast_meets_the_floor_in_both_depths() {
        for depth in BOTH {
            let t = Theme::synthwave_with(depth);
            for (name, f, b, floor) in pairs(&t) {
                assert!(
                    ratio(f, b) >= floor,
                    "{depth:?} {name}: {f:?} on {b:?} is {:.2}",
                    ratio(f, b)
                );
            }
        }
    }

    #[test]
    fn synthwave_without_true_colour_uses_only_indexed_colours() {
        let t = Theme::synthwave_with(ColorDepth::Indexed256);
        for s in styles(&t) {
            for c in [s.fg, s.bg].into_iter().flatten() {
                assert!(matches!(c, Color::Indexed(16..=255)), "{c:?}");
            }
        }
    }

    #[test]
    fn synthwave_surfaces_and_roles_stay_distinguishable_in_both_depths() {
        for depth in BOTH {
            let t = Theme::synthwave_with(depth);
            let bg = t.text.bg.unwrap();
            for (name, s) in [
                ("bar", t.tab_bar.bg.unwrap()),
                ("popup", t.popup.bg.unwrap()),
                ("selection", t.selection.bg.unwrap()),
                ("list_selected", t.list_selected.bg.unwrap()),
            ] {
                assert!(ratio(s, bg) >= 1.1, "{depth:?} {name} vs bg");
            }
            assert!(ratio(t.popup.bg.unwrap(), t.tab_bar.bg.unwrap()) >= 1.1);
            assert!(ratio(t.input.bg.unwrap(), t.popup.bg.unwrap()) >= 1.1);
            // Find, replace and search inputs sit on the editor background.
            assert!(
                ratio(t.input.bg.unwrap(), bg) >= 1.3,
                "{depth:?} input vs editor"
            );
            // The palette input and the selected row sit next to each other.
            let (input, picked) = (t.input.bg.unwrap(), t.list_selected.bg.unwrap());
            assert!(
                ratio(input, picked) >= 1.6,
                "{depth:?} {:.2}",
                ratio(input, picked)
            );
            assert_ne!(t.selection.bg, t.list_selected.bg);
            assert_ne!(t.info.fg, t.warning.fg);
            assert_ne!(t.warning.fg, t.error.fg);
            assert_ne!(t.focus.fg, t.dim.fg);
        }
    }

    #[test]
    fn dracula_surfaces_stay_distinguishable_in_both_depths() {
        for depth in BOTH {
            let t = Theme::dracula_with(depth);
            let bg = t.text.bg.unwrap();
            for (name, s) in [
                ("bar", t.tab_bar.bg.unwrap()),
                ("popup", t.popup.bg.unwrap()),
                ("selection", t.selection.bg.unwrap()),
            ] {
                assert!(ratio(s, bg) >= 1.1, "{depth:?} {name} vs bg");
            }
            assert!(ratio(t.popup.bg.unwrap(), t.tab_bar.bg.unwrap()) >= 1.1);
            assert!(ratio(t.input.bg.unwrap(), t.popup.bg.unwrap()) >= 1.1);
        }
    }

    #[test]
    fn read_and_edit_badges_have_distinct_colours_in_every_theme() {
        let mut themes = vec![Theme::dark(), Theme::light()];
        themes.extend(BOTH.map(Theme::dracula_with));
        themes.extend(BOTH.map(Theme::synthwave_with));
        for t in themes {
            let (r, e) = (t.mode_read, t.mode_edit);
            assert!(r.bg.is_some() && e.bg.is_some(), "{r:?} {e:?}");
            assert_ne!(r.bg, e.bg);
            assert_ne!(r.bg, t.status.bg, "READ does not stand out of the bar");
            assert_ne!(e.bg, t.status.bg, "EDIT does not stand out of the bar");
            assert!(r.fg.is_some() && e.fg.is_some());
            // The badge text must not vanish into its own background.
            assert_ne!(r.fg, r.bg);
            assert_ne!(e.fg, e.bg);
        }
    }

    #[test]
    fn terminal_colour_badges_do_not_collide_with_the_bar_or_the_text() {
        for t in [Theme::dark(), Theme::light()] {
            for badge in [t.mode_read, t.mode_edit] {
                assert_ne!(badge.bg, t.status.bg);
                assert_ne!(badge.bg, t.status.fg);
                assert_ne!(badge.bg, t.tab_bar.bg);
            }
        }
    }

    #[test]
    fn terminal_colour_themes_show_the_focus_with_palette_colours_only() {
        for t in [Theme::dark(), Theme::light()] {
            assert_ne!(t.focus.fg, t.dim.fg);
            assert_eq!(t.focus.bg, None);
        }
    }
}
