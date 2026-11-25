//! Explorer panel: the project tree with folders expanded in place (AC-1). Each workspace
//! root is a top-level row (W-2; a missing one is marked); a column on the right separates
//! the panel from the editor.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use vr_fs::{NodeKind, SymlinkTarget};

use crate::app::{App, Focus};
use crate::commands::explorer::Row;
use crate::theme::Theme;

/// Cells of indentation per tree level.
const INDENT: usize = 2;

/// After the name of a row matched by .gitignore (shown with explorer.toggle_ignored).
const IGNORED_MARK: &str = " ·";

/// After the name of a workspace root whose folder is gone (W-2).
const MISSING_MARK: &str = " (missing)";

pub fn render(buf: &mut Buffer, area: Rect, app: &mut App, theme: &Theme) {
    app.explorer.area = area;
    if area.width < 3 || area.height == 0 {
        return;
    }
    let focused = app.focus == Focus::Explorer;
    let ex = &mut app.explorer;
    let height = usize::from(area.height);
    // Keep the cursor visible, unless the wheel moved the view away from it.
    if ex.follow_cursor {
        if ex.cursor < ex.top {
            ex.top = ex.cursor;
        } else if ex.cursor >= ex.top + height {
            ex.top = ex.cursor + 1 - height;
        }
    }
    ex.top = ex.top.min(ex.rows.len().saturating_sub(1));
    ex.drawn_top = ex.top;
    let inner = area.width - 1;
    buf.set_style(Rect::new(area.x, area.y, inner, area.height), theme.text);
    for (row_y, (i, row)) in ex
        .rows
        .iter()
        .enumerate()
        .skip(ex.top)
        .take(height)
        .enumerate()
    {
        let y = area.y + row_y as u16;
        let mut style = if ex.cursor == i && focused {
            theme.list_selected
        } else if ex.cursor == i || ex.selected.contains(&row.path) {
            theme.selection
        } else {
            theme.text
        };
        buf.set_style(Rect::new(area.x, y, inner, 1), style);
        if row.ignored
            || row.missing
            || matches!(
                row.kind,
                NodeKind::Symlink(SymlinkTarget::Broken | SymlinkTarget::Loop)
            )
        {
            style = style.patch(theme.dim);
        }
        if row.depth == 0 {
            style = style.add_modifier(Modifier::BOLD);
        }
        let text = format!(
            "{}{}{}",
            " ".repeat(INDENT * row.depth),
            icon(row, ex.expanded.contains(&row.path)),
            row.name()
        );
        // Ignored rows and missing roots are also marked in text, so the flag shows without
        // colours; the marker is kept when a long name is cut.
        let marker = if row.missing {
            MISSING_MARK
        } else if row.ignored {
            IGNORED_MARK
        } else {
            ""
        };
        let room = usize::from(inner).saturating_sub(marker.chars().count());
        let shown = format!("{}{marker}", fit(&text, room));
        buf.set_stringn(area.x, y, shown, usize::from(inner), style);
    }
    let sep = if focused { theme.focus } else { theme.dim };
    for y in area.y..area.y + area.height {
        buf.set_string(area.x + inner, y, "│", sep);
    }
}

fn icon(row: &Row, expanded: bool) -> &'static str {
    if row.missing {
        return "✗ ";
    }
    match row.kind {
        k if k.is_expandable() && expanded => "▾ ",
        k if k.is_expandable() => "▸ ",
        NodeKind::Symlink(SymlinkTarget::Loop) => "↻ ",
        NodeKind::Symlink(SymlinkTarget::Broken) => "✗ ",
        _ => "  ",
    }
}

/// `text` cut to `width` cells, ending in "…" when it did not fit.
fn fit(text: &str, width: usize) -> String {
    use ratatui::text::Span;
    if Span::raw(text).width() <= width {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let w = Span::raw(ch.to_string()).width();
        if used + w + 1 > width {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push('…');
    out
}
