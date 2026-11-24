//! Status bar (`READ/EDIT file · Ln x, Col y · LF/CRLF · UTF-8`) and the notice lines above it.
//! Notices get their own rows (up to three, wrapped) so a long startup notice stays readable
//! even at 40 columns; Escape dismisses the oldest.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};
use vr_core::{DiskState, LineEnding};

use crate::app::{App, Focus, NoticeLevel, encoding_name};
use crate::theme::Theme;

const MAX_NOTICE_ROWS: u16 = 3;

/// Esc dismisses the oldest notice only when nothing else takes the key first: a dialog,
/// the palette, the find bar and the search panel use it themselves, and in edit mode the
/// first Esc leaves for read mode (D151). The on-screen hint must hold in every state.
fn esc_dismisses(app: &App) -> bool {
    app.dialog.is_none()
        && app.palette.is_none()
        && matches!(app.focus, Focus::Editor | Focus::Explorer)
        && !(app.focus == Focus::Editor && app.edit_mode())
}

/// Text of the oldest notice with a hint about the rest.
pub fn notice_text(app: &App) -> Option<String> {
    let n = app.notices.front()?;
    let more = app.notices.len() - 1;
    let esc = esc_dismisses(app);
    let suffix = if more > 0 {
        let key = if esc { " · Esc" } else { "" };
        format!("  (+{more} more{key})")
    } else if n.level == NoticeLevel::Info || !esc {
        String::new()
    } else {
        "  (Esc)".to_string()
    };
    Some(format!("{}{suffix}", n.text))
}

/// Rows the oldest notice needs at `width` (0 without notices).
pub fn notice_rows(app: &App, width: u16) -> u16 {
    let Some(text) = notice_text(app) else {
        return 0;
    };
    let w = Span::raw(text).width().max(1);
    let width = usize::from(width.max(1));
    (w.div_ceil(width) as u16).clamp(1, MAX_NOTICE_ROWS)
}

pub fn render_notice(buf: &mut Buffer, area: Rect, app: &App, theme: &Theme) {
    let (Some(text), Some(n)) = (notice_text(app), app.notices.front()) else {
        return;
    };
    if area.height == 0 {
        return;
    }
    let style = match n.level {
        NoticeLevel::Info => theme.info,
        NoticeLevel::Warning => theme.warning,
        NoticeLevel::Error => theme.error,
    };
    Paragraph::new(Line::styled(text, style))
        .wrap(Wrap { trim: false })
        .render(area, buf);
}

pub fn render(buf: &mut Buffer, area: Rect, app: &App, theme: &Theme) {
    if area.height == 0 {
        return;
    }
    buf.set_style(area, theme.status);
    let width = usize::from(area.width);
    let Some(doc) = app.active_doc().filter(|_| !app.welcome) else {
        let root = format!(" {}", app.root.display());
        buf.set_stringn(area.x, area.y, root, width, theme.status);
        return;
    };
    let rope = doc.buffer.text();
    let head = doc.buffer.selection().primary().head;
    let line = rope.char_to_line(head);
    // On a long line counting graphemes from its start would cost a walk of the whole line
    // per frame; there Col counts chars instead.
    let pos = if crate::ui::editor_view::is_long(rope.line(line)) {
        vr_core::Pos {
            line,
            col: head - rope.line_to_char(line),
        }
    } else {
        vr_core::text::char_to_pos(rope, head)
    };
    let eol = match doc.line_ending {
        LineEnding::Lf => "LF",
        LineEnding::CrLf => "CRLF",
        LineEnding::Cr => "CR",
    };
    let (ln, col) = (pos.line + 1, pos.col + 1);
    let rights = [
        format!(" · Ln {ln}, Col {col} · {eol} · {} ", encoding_name(doc)),
        format!(" · Ln {ln}, Col {col} "),
        format!(" Ln {ln}:{col} "),
    ];
    let (mut long, mut short) = (String::new(), String::new());
    if doc.read_only {
        long.push_str(" [read-only]");
        short.push_str(" [RO]");
    }
    match doc.disk_state {
        DiskState::ChangedOnDisk => {
            long.push_str(" [changed on disk]");
            short.push_str(" [changed]");
        }
        DiskState::DeletedOnDisk => {
            long.push_str(" [deleted on disk]");
            short.push_str(" [deleted]");
        }
        DiskState::InSync => {}
    }
    let path = app.active_name();
    let file = doc
        .path
        .as_deref()
        .and_then(|p| p.file_name())
        .map_or_else(String::new, |f| f.to_string_lossy().into_owned());
    // Room for the file name and a cut mark.
    let need = width_of(&file) + 1;
    // The mode badge (D145) comes first and also separates the path from the bar's edge; it
    // shrinks to its initial when the full word would leave no room for the file name with
    // the shortest position.
    let (badges, badge_style) = if app.edit_mode() {
        ([" EDIT ", " E ", "E "], theme.mode_edit)
    } else {
        ([" READ ", " R ", "R "], theme.mode_read)
    };
    // The badge shrinks to its initial before EOL and encoding are dropped (D156); the last
    // tier still keeps a space before the path.
    let fits = |b: &&str, r: &str| width_of(b) + width_of(r) + width_of(&short) + need <= width;
    let badge = badges[..2]
        .iter()
        .find(|b| fits(b, &rights[0]))
        .or_else(|| badges.iter().find(|b| fits(b, &rights[2])))
        .copied()
        .unwrap_or(badges[2]);
    let bw = width_of(badge).min(width);
    buf.set_stringn(area.x, area.y, badge, width, badge_style);
    let (x0, width) = (area.x + bw as u16, width - bw);
    // Give up detail in this order: the directory part of the path, the long flag text, EOL
    // and encoding, then "Col"; the file name itself is cut only when nothing else is left.
    let pick = rights
        .iter()
        .flat_map(|r| [(r, &long), (r, &short)])
        .find(|(r, f)| width_of(r) + width_of(f) + need <= width)
        .unwrap_or((&rights[2], &short));
    let (right, flags) = pick;
    let room = width.saturating_sub(width_of(right) + width_of(flags));
    let left = format!("{}{flags}", shorten_left(&path, room));
    let left_w = width_of(&left).min(width);
    buf.set_stringn(x0, area.y, &left, width, theme.status);
    let rx = x0 + left_w as u16;
    buf.set_stringn(rx, area.y, right, width - left_w, theme.status);
}

fn width_of(s: &str) -> usize {
    Span::raw(s).width()
}

/// Keeps the end of `s` (the file name) within `room` cells, marking the cut with `…`.
fn shorten_left(s: &str, room: usize) -> String {
    if Span::raw(s).width() <= room {
        return s.to_string();
    }
    if room == 0 {
        return String::new();
    }
    let mut kept: Vec<char> = Vec::new();
    let mut w = 1;
    for c in s.chars().rev() {
        let cw = Span::raw(c.to_string()).width();
        if w + cw > room {
            break;
        }
        w += cw;
        kept.push(c);
    }
    std::iter::once('…').chain(kept.into_iter().rev()).collect()
}
