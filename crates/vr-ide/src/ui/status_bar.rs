//! Status bar (`READ/EDIT file · Ln x, Col y · LF/CRLF · UTF-8`) and the notice lines above it.
//! Notices get their own rows (up to three, wrapped) so a long startup notice stays readable
//! even at 40 columns; Escape dismisses the oldest.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Span;
use vr_core::{DiskState, LineEnding};

use crate::app::{App, Focus, NoticeLevel, encoding_name};
use crate::theme::Theme;

const MAX_NOTICE_ROWS: u16 = 3;

/// Esc dismisses the oldest notice only when nothing else takes the key first: a dialog,
/// the palette and the find box (either scope) use it themselves, and in edit mode the
/// first Esc leaves for read mode (D151). The on-screen hint must hold in every state.
fn esc_dismisses(app: &App) -> bool {
    app.dialog.is_none()
        && app.palette.is_none()
        && matches!(app.focus, Focus::Editor | Focus::Explorer)
        && !(app.focus == Focus::Editor && app.edit_mode())
}

/// Text of the oldest notice with a hint about the rest; its detail (if any) comes last.
pub fn notice_text(app: &App) -> Option<String> {
    let n = app.notices.front()?;
    let more = app.notices.len() - 1;
    let esc = esc_dismisses(app);
    // A focused terminal gives Esc to its program; a click on the notice dismisses it there.
    let click = crate::commands::terminal::has_keys(app) || crate::ai::has_keys(app);
    let key = if esc {
        Some("Esc")
    } else if click {
        Some("click")
    } else {
        None
    };
    let suffix = if more > 0 {
        let key = key.map_or_else(String::new, |k| format!(" · {k}"));
        format!("  (+{more} more{key})")
    } else if let Some(k) = key.filter(|_| n.level != NoticeLevel::Info) {
        format!("  ({k})")
    } else {
        String::new()
    };
    let detail = n
        .detail
        .as_ref()
        .map_or_else(String::new, |d| format!("  {d}"));
    Some(format!("{}{suffix}{detail}", n.text))
}

/// Greedy word wrap by display width; a word longer than a row is split.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = vec![String::new()];
    let mut used = 0;
    for word in text.split_inclusive(' ') {
        let mut word = word;
        if used > 0 && used + width_of(word.trim_end()) > width {
            lines.push(String::new());
            used = 0;
        }
        while width_of(word.trim_end()) > width {
            let mut cut = 0;
            let mut cw = 0;
            for c in word.chars() {
                let ch = width_of(c.encode_utf8(&mut [0; 4]));
                if cw + ch > width {
                    break;
                }
                cut += c.len_utf8();
                cw += ch;
            }
            if cut == 0 {
                // A single glyph wider than the row: drop it rather than loop.
                word = &word[word.chars().next().map_or(0, char::len_utf8)..];
                continue;
            }
            lines.last_mut().unwrap().push_str(&word[..cut]);
            lines.push(String::new());
            word = &word[cut..];
            used = 0;
        }
        lines.last_mut().unwrap().push_str(word);
        used += width_of(word);
    }
    lines.iter().map(|l| l.trim_end().to_string()).collect()
}

/// The notice as at most `rows` rows of `width` cells; a cut ends in `…`.
fn notice_lines(text: &str, width: u16, rows: u16) -> Vec<String> {
    let mut lines = wrap(text, usize::from(width));
    let rows = usize::from(rows.max(1));
    if lines.len() > rows {
        lines.truncate(rows);
        let last = lines.last_mut().unwrap();
        while !last.is_empty() && width_of(last) + 1 > usize::from(width.max(1)) {
            last.pop();
        }
        last.push('…');
    }
    lines
}

/// Rows the oldest notice needs at `width` (0 without notices).
pub fn notice_rows(app: &App, width: u16) -> u16 {
    let Some(text) = notice_text(app) else {
        return 0;
    };
    (wrap(&text, usize::from(width)).len() as u16).clamp(1, MAX_NOTICE_ROWS)
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
    let width = usize::from(area.width);
    for (i, line) in notice_lines(&text, area.width, area.height)
        .iter()
        .enumerate()
    {
        buf.set_stringn(area.x, area.y + i as u16, line, width, style);
    }
}

pub fn render(buf: &mut Buffer, area: Rect, app: &App, theme: &Theme) {
    // The bar reflows into the width the badge leaves (its tiers drop detail as usual).
    let badge = (area.height > 0)
        .then(|| crate::ai::attention::badge(app, area.width))
        .flatten();
    let Some(parts) = badge else {
        render_bar(buf, area, app, theme);
        return;
    };
    let total = (width_of(&parts.0) + width_of(&parts.1)) as u16;
    buf.set_style(area, theme.status);
    let left = Rect::new(area.x, area.y, area.width - total, area.height);
    render_bar(buf, left, app, theme);
    crate::ai::attention::render_badge(buf, area, theme, &parts);
}

fn render_bar(buf: &mut Buffer, area: Rect, app: &App, theme: &Theme) {
    if area.height == 0 {
        return;
    }
    buf.set_style(area, theme.status);
    if crate::ai::has_focus(app) {
        crate::ui::ai_view::render_status(buf, area, app, theme);
        return;
    }
    if crate::commands::terminal::has_focus(app) {
        crate::ui::terminal_view::render_status(buf, area, app, theme);
        return;
    }
    let width = usize::from(area.width);
    // Branch, ahead/behind and the parent-repo label (M7-4a); sanitized by `git`.
    let git = crate::git::status_label(app);
    let Some(doc) = app.active_doc() else {
        let mut root = format!(" {}", app.workspace.label());
        if let Some(g) = &git {
            let full = format!("{root} · {}", g.full);
            root = if width_of(&full) <= width {
                full
            } else {
                format!("{root} · {}", g.short)
            };
        }
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
    let plain = [
        format!(" · Ln {ln}, Col {col} · {eol} · {} ", encoding_name(doc)),
        format!(" · Ln {ln}, Col {col} "),
        format!(" Ln {ln}:{col} "),
    ];
    // With git, each position tier comes first with the full, then the short branch text; the
    // branch is given up before the file name, and only after every shorter form failed.
    let mut rights: Vec<String> = Vec::new();
    if let Some(g) = &git {
        for (i, r) in plain.iter().enumerate() {
            let r = if i == 2 {
                format!(" · {}", r.trim_start())
            } else {
                r.clone()
            };
            rights.push(format!(" · {}{r}", g.full));
            rights.push(format!(" · {}{r}", g.short));
        }
    }
    rights.extend(plain.iter().cloned());
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
        .find(|b| fits(b, &plain[0]))
        .or_else(|| badges.iter().find(|b| fits(b, &plain[2])))
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
        .unwrap_or((&plain[2], &short));
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
pub(crate) fn shorten_left(s: &str, room: usize) -> String {
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
