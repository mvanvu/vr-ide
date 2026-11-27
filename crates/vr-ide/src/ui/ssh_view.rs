//! A logged-in SSH session in full screen (FR-25, AC-S3): the session's grid over the whole
//! window, drawn by the terminal panel's own grid renderer (so it has the panel's colours),
//! and one status row that vr-ide draws below it.
//!
//! The PTY is sized to the window minus that row, so nothing the remote side prints can reach
//! it, and its text comes from the validated target only (SEC-S12): it is the one part of the
//! screen the remote side cannot imitate.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Span;

use super::layout::AppLayout;
use super::terminal_view;
use crate::app::App;
use crate::commands::terminal as term;
use crate::theme::Theme;

/// The session's grid on every row but the last; the status row there. No other part of the
/// IDE is laid out, so the mouse reaches only the session.
pub fn layout(area: Rect) -> AppLayout {
    let rows = area.height.saturating_sub(1);
    AppLayout {
        terminal: Rect::new(area.x, area.y, area.width, rows),
        status: Rect::new(area.x, area.y + rows, area.width, area.height - rows),
        ..AppLayout::default()
    }
}

pub fn render(frame: &mut Frame, area: Rect, app: &mut App, theme: &Theme) {
    let layout = layout(area);
    app.layout = layout;
    let grid = layout.terminal;
    if grid.width > 0 && grid.height > 0 {
        term::fit(app, grid);
        term::snapshot_active(app);
        let close = app.key_hint("file.close");
        let cursor =
            terminal_view::render_grid(frame.buffer_mut(), grid, &app.terminal, &close, theme);
        if let Some(pos) = cursor
            && term::has_keys(app)
        {
            frame.set_cursor_position(pos);
        }
    }
    render_status(frame.buffer_mut(), layout.status, app, theme);
}

/// ` SSH  <target>` and, on the right, the key that gives the editor back.
fn render_status(buf: &mut Buffer, area: Rect, app: &App, theme: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let width = usize::from(area.width);
    buf.set_style(area, theme.status);
    let badge = " SSH ";
    buf.set_stringn(area.x, area.y, badge, width, theme.mode_term);
    let used = Span::raw(badge).width().min(width);
    let room = width - used;
    let target = app
        .terminal
        .active_tab()
        .and_then(|t| t.ssh.as_ref())
        .map(|s| s.target.to_string())
        .unwrap_or_default();
    // The way back outranks a long target: the target is cut first.
    let hint = format!(" {} editor ", app.key_hint("terminal.toggle"));
    let hint_w = Span::raw(hint.as_str()).width();
    let hint_w = if hint_w + 2 <= room { hint_w } else { 0 };
    // Notices wait for the IDE layout (D325); the count says there is something to read.
    let count = match app.notices.len() {
        0 => String::new(),
        1 => " 1 notice ".to_string(),
        n => format!(" {n} notices "),
    };
    let count_w = Span::raw(count.as_str()).width();
    // The way back outranks the count: no count without the hint.
    let count_w = if hint_w > 0 && hint_w + count_w + 2 <= room {
        count_w
    } else {
        0
    };
    let right_w = hint_w + count_w;
    let name = format!(
        " {}",
        super::status_bar::shorten_left(&target, (room - right_w).saturating_sub(1))
    );
    buf.set_stringn(
        area.x + used as u16,
        area.y,
        &name,
        room - right_w,
        theme.status,
    );
    if count_w > 0 {
        buf.set_stringn(
            area.x + (width - right_w) as u16,
            area.y,
            &count,
            count_w,
            theme.mode_term,
        );
    }
    if hint_w > 0 {
        buf.set_stringn(
            area.x + (width - hint_w) as u16,
            area.y,
            &hint,
            hint_w,
            theme.status,
        );
    }
}
