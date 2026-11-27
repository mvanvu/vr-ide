//! Tab bar: one tab per open document with the modified marker `●`, `!` when the file
//! changed on disk under unsaved edits and `(deleted)` when it is gone.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use vr_core::DiskState;

use crate::app::App;
use crate::theme::Theme;

/// The close button's glyph; it sits two cells before the label's end (`x` + a space).
const CLOSE: &str = "x";

fn close_style(active: bool, base: Style, theme: &Theme) -> Style {
    match (active, theme.dim.fg) {
        (false, Some(fg)) => base.fg(fg),
        _ => base,
    }
}

/// The pieces of tab `i`'s label with their styles.
fn label<'a>(app: &'a App, i: usize, theme: &Theme) -> Vec<Span<'a>> {
    let doc = &app.docs[i];
    let base = if i == app.active {
        theme.tab_active
    } else {
        theme.tab_inactive
    };
    let name = match app.views.get(i).and_then(|v| v.title) {
        Some(title) => title.into(),
        None => doc
            .path
            .as_deref()
            .and_then(|p| p.file_name())
            .map_or_else(|| "untitled".into(), |n| n.to_string_lossy()),
    };
    let mut spans = vec![Span::styled(format!(" {name}"), base)];
    if doc.is_modified() {
        spans.push(Span::styled(" ●", base.patch(theme.tab_modified)));
    }
    match doc.disk_state {
        DiskState::ChangedOnDisk => spans.push(Span::styled(" !", base.patch(theme.tab_warning))),
        DiskState::DeletedOnDisk => {
            spans.push(Span::styled(" (deleted)", base.patch(theme.tab_warning)))
        }
        DiskState::InSync => {}
    }
    // The close button: dim on inactive tabs like the terminal tabs' 'x'.
    spans.push(Span::styled(" ", base));
    spans.push(Span::styled(
        CLOSE,
        close_style(i == app.active, base, theme),
    ));
    spans.push(Span::styled(" ", base));
    spans
}

fn label_width(app: &App, i: usize) -> u16 {
    let w: usize = label(app, i, &Theme::dark()).iter().map(Span::width).sum();
    w.min(usize::from(u16::MAX)) as u16
}

/// Visible tabs as `(index, x offset, width)`: the leftmost start that keeps the active
/// tab on screen.
pub fn positions(app: &App, width: u16) -> Vec<(usize, u16, u16)> {
    let widths: Vec<u16> = (0..app.docs.len()).map(|i| label_width(app, i)).collect();
    let active = app.active.min(widths.len().saturating_sub(1));
    let mut first = 0;
    while first < active
        && widths[first..=active]
            .iter()
            .map(|w| u32::from(*w))
            .sum::<u32>()
            > u32::from(width)
    {
        first += 1;
    }
    let mut out = Vec::new();
    let mut x = 0u16;
    for (i, w) in widths.iter().enumerate().skip(first) {
        if x >= width {
            break;
        }
        out.push((i, x, (*w).min(width - x)));
        x = x.saturating_add(*w);
    }
    out
}

/// Tab under column `x` (relative to the tab bar).
pub fn hit(app: &App, x: u16, width: u16) -> Option<usize> {
    positions(app, width)
        .into_iter()
        .find(|(_, start, w)| x >= *start && x < start + w)
        .map(|(i, ..)| i)
}

/// Offset (from the bar's left edge) of tab `i`'s `x`; `None` when the tab is off screen or
/// cut before its `x`, so a cut label never closes on a click.
pub fn close_offset(app: &App, i: usize, width: u16) -> Option<u16> {
    positions(app, width)
        .into_iter()
        .find(|(j, ..)| *j == i)
        .and_then(|(_, start, w)| {
            let full = label_width(app, i);
            (w == full).then(|| start + full - 2)
        })
}

/// What a click at bar column `x` lands on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Label(usize),
    Close(usize),
}

pub fn hit_part(app: &App, x: u16, width: u16) -> Option<Part> {
    let i = hit(app, x, width)?;
    Some(if close_offset(app, i, width) == Some(x) {
        Part::Close(i)
    } else {
        Part::Label(i)
    })
}

pub fn render(buf: &mut Buffer, area: Rect, app: &App, theme: &Theme) {
    if area.height == 0 {
        return;
    }
    buf.set_style(area, theme.tab_bar);
    if app.docs.is_empty() {
        let root = format!(" {}", app.workspace.label());
        buf.set_stringn(area.x, area.y, root, usize::from(area.width), theme.dim);
        return;
    }
    for (i, x, w) in positions(app, area.width) {
        let mut cx = area.x + x;
        let end = cx + w;
        for span in label(app, i, theme) {
            if cx >= end {
                break;
            }
            let style: Style = span.style;
            let (nx, _) = buf.set_stringn(cx, area.y, &span.content, usize::from(end - cx), style);
            cx = nx;
        }
    }
}
