//! The find / replace bar between the tab bar and the editor (AC-10): the query, the match
//! count ("3/12") and the case / regex / whole-word toggles; a second row holds the
//! replacement and how to replace one or all. Toggles that are on are drawn like a selected
//! button, so they read on monochrome terminals too.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::Span;

use crate::app::{App, Focus};
use crate::commands::find::{self, Field};
use crate::theme::Theme;
use crate::ui::dialog::LineInput;

/// Below this width the labels shrink to one letter.
const NARROW: u16 = 36;

/// Shown at the end of the replace row: Enter replaces the current match, ctrl+a every match.
const REPLACE_HELP: &str = "enter one · ctrl+a all";

/// Rows the bar takes: 0 closed, 1 find, 2 find and replace.
pub fn rows(app: &App) -> u16 {
    if !app.find.open || app.active_doc().is_none() {
        0
    } else if app.find.replace_mode {
        2
    } else {
        1
    }
}

pub fn render(frame: &mut Frame, area: Rect, app: &mut App, theme: &Theme) {
    app.find.area = area;
    if area.height == 0 || area.width < 4 {
        return;
    }
    find::refresh(app);
    let narrow = area.width < NARROW;
    let count = find::count_text(app);
    let f = &app.find;
    let focused = app.focus == Focus::FindBar;
    let buf = frame.buffer_mut();
    buf.set_style(area, theme.status);

    // Row 0: label, query, count, toggles.
    let label = if narrow { "F " } else { "Find " };
    let toggles: [(&str, bool, bool); 3] = [
        ("Aa", f.case_sensitive, false),
        (".*", f.regex, false),
        ("ab", f.whole_word, true),
    ];
    let toggles_w: u16 = 3 * 3;
    let count_w = Span::raw(count.as_str()).width() as u16 + 2;
    let label_w = label.len() as u16;
    let fixed = label_w + toggles_w;
    let show_count = area.width > fixed + count_w + 4;
    let field_w = area
        .width
        .saturating_sub(fixed + if show_count { count_w } else { 0 })
        .max(1);
    let y = area.y;
    buf.set_string(area.x, y, label, theme.status);
    let mut cursor = None;
    let x = area.x + label_w;
    if let Some(c) = draw_input(buf, x, y, field_w, &f.input, theme)
        && focused
        && f.field == Field::Find
    {
        cursor = Some((c, y));
    }
    let mut x = x + field_w;
    if show_count {
        let style = if f.error.is_some() {
            theme.status.patch(theme.error)
        } else {
            theme.status
        };
        buf.set_stringn(x + 1, y, &count, usize::from(count_w - 2), style);
        x += count_w;
    }
    for (text, on, underline) in toggles {
        if x + 2 > area.x + area.width {
            break;
        }
        let mut style = if on {
            theme.button_selected
        } else {
            theme.status.patch(theme.dim)
        };
        if underline {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        buf.set_string(x + 1, y, text, style);
        x += 3;
    }

    // Row 1: the replacement.
    if f.replace_mode && area.height > 1 {
        let y = area.y + 1;
        let label = if narrow { "R " } else { "Replace " };
        let label_w = label.len() as u16;
        buf.set_string(area.x, y, label, theme.status);
        // How to replace one or all (T21), when the row has room for it and a usable field.
        let help = if narrow { "" } else { REPLACE_HELP };
        let help_w = Span::raw(help).width() as u16;
        let show_help = !help.is_empty() && area.width >= label_w + help_w + 2 + 10;
        let reserved = if show_help { help_w + 2 } else { 1 };
        let w = area.width.saturating_sub(label_w + reserved).max(1);
        if show_help {
            let hx = area.x + area.width - help_w;
            buf.set_string(hx, y, help, theme.status.patch(theme.dim));
        }
        if let Some(c) = draw_input(buf, area.x + label_w, y, w, &f.replace, theme)
            && focused
            && f.field == Field::Replace
        {
            cursor = Some((c, y));
        }
    }
    if let Some(pos) = cursor
        && app.dialog.is_none()
    {
        frame.set_cursor_position(pos);
    }
}

/// Draws `input` in `w` cells at (`x`, `y`); returns the cursor's screen column.
fn draw_input(
    buf: &mut ratatui::buffer::Buffer,
    x: u16,
    y: u16,
    w: u16,
    input: &LineInput,
    theme: &Theme,
) -> Option<u16> {
    if w == 0 {
        return None;
    }
    buf.set_style(Rect::new(x, y, w, 1), theme.input);
    let (shown, cx) = input.view(w);
    buf.set_stringn(x, y, shown, usize::from(w), theme.input);
    Some(x + cx.min(w - 1))
}
