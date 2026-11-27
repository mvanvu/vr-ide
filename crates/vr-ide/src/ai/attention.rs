//! D9 notify: an agent session is marked, and a notice shows, when its program rings the bell or
//! sends OSC 9 / OSC 777 (SEC-14: the text is program output, so it is sanitized, capped and
//! rate-limited, and always shown behind the tab label).

use std::time::{Duration, Instant};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use crate::app::{App, Notice, NoticeLevel};
use crate::theme::Theme;
use crate::ui::safe_text::sanitize;

/// Longest message shown, in characters of the program's text.
const MESSAGE_MAX: usize = 200;
/// A session gets at most one notice per this time.
const NOTICE_GAP: Duration = Duration::from_secs(1);
/// Terminal width from which the wide form is used (S4: 120 cols wide, 80 narrow).
const WIDE_FROM: usize = 100;
const BELL_TEXT: &str = "rang the bell";

/// What a session remembers of its last notice.
#[derive(Default)]
pub struct NotifyState {
    last: Option<Instant>,
    /// The text of the notice that is up for the current mark.
    notice: Option<String>,
}

fn width_of(s: &str) -> usize {
    Span::raw(s).width()
}

/// The message of a bell or notification as shown: capped, made safe, never empty.
fn message(text: Option<&str>) -> String {
    let capped: String = text.unwrap_or("").chars().take(MESSAGE_MAX).collect();
    let safe = sanitize(capped.trim());
    if safe.is_empty() {
        BELL_TEXT.to_string()
    } else {
        safe
    }
}

/// `◆ <label> (<folder>): <message>  f12 shows it`; when that does not fit `width` the narrow
/// form `◆ <label>: <message> f12`, its message cut with `…` before the key.
fn notice_text(label: &str, folder: &str, msg: &str, key: &str, width: usize) -> String {
    let wide = format!("◆ {label} ({folder}): {msg}  {key} shows it");
    if width == 0 || (width >= WIDE_FROM && width_of(&wide) <= width) {
        return wide;
    }
    let (head, tail) = (format!("◆ {label}: "), format!(" {key}"));
    let room = width.saturating_sub(width_of(&head) + width_of(&tail));
    if width_of(msg) <= room {
        return format!("{head}{msg}{tail}");
    }
    let mut cut = String::new();
    for c in msg.chars() {
        if width_of(&cut) + width_of(c.encode_utf8(&mut [0; 4])) + 1 > room {
            break;
        }
        cut.push(c);
    }
    format!("{head}{cut}…{tail}")
}

/// Takes what the sessions' programs sent since the last call and marks the sessions that are
/// not being viewed. True when something changed on screen.
pub fn collect(app: &mut App) -> bool {
    let mut events = Vec::new();
    for (i, s) in app.ai.sessions.iter().enumerate() {
        events.extend(s.tab.take_attention().into_iter().map(|a| (i, a.text)));
    }
    let mut changed = false;
    for (i, text) in events {
        changed |= mark(app, i, text.as_deref(), Instant::now());
    }
    changed
}

pub(crate) fn mark(app: &mut App, i: usize, text: Option<&str>, now: Instant) -> bool {
    let p = &app.ai;
    let viewed = p.visible && !p.picker_active() && p.active == i;
    if viewed || p.sessions[i].attention.is_some() {
        return false;
    }
    let order = p
        .sessions
        .iter()
        .filter_map(|s| s.attention)
        .max()
        .map_or(0, |m| m + 1);
    let s = &mut app.ai.sessions[i];
    s.attention = Some(order);
    let rated = s
        .notify
        .last
        .is_some_and(|t| now.duration_since(t) < NOTICE_GAP);
    if !rated {
        s.notify.last = Some(now);
        let p = &app.ai;
        let folder = p.sessions[i]
            .root
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let width = usize::from(app.layout.status.width);
        let shown = notice_text(
            &p.label(i),
            &sanitize(&folder),
            &message(text),
            &app.key_hint("ai.toggle"),
            width,
        );
        app.ai.sessions[i].notify.notice = Some(shown.clone());
        app.push_notice(Notice {
            level: NoticeLevel::Info,
            text: shown,
            // Up until the tab is viewed (or Esc).
            expires: None,
            until_next: false,
            detail: None,
        });
    }
    app.dirty = true;
    true
}

/// Viewing the active tab of a shown panel clears its mark and its notice; called before each
/// frame's layout.
pub fn clear_viewed(app: &mut App) {
    let p = &mut app.ai;
    if !p.visible || p.picker_active() {
        return;
    }
    let Some(s) = p.sessions.get_mut(p.active) else {
        return;
    };
    if s.attention.take().is_none() {
        return;
    }
    if let Some(text) = s.notify.notice.take() {
        app.notices.retain(|n| n.text != text);
    }
    app.dirty = true;
}

/// Marked sessions whose tab is not on screen (panel hidden, or inside `+N`): how many, and the
/// label of the oldest.
fn unseen(app: &App) -> Option<(usize, String)> {
    let p = &app.ai;
    let shown: Vec<usize> = if p.visible {
        crate::ui::ai_view::tab_bar(app, app.layout.ai.width)
            .tabs
            .iter()
            .map(|t| t.index)
            .collect()
    } else {
        Vec::new()
    };
    let marked: Vec<(u64, usize)> = p
        .sessions
        .iter()
        .enumerate()
        .filter(|(i, _)| !shown.contains(i))
        .filter_map(|(i, s)| s.attention.map(|o| (o, i)))
        .collect();
    let (_, oldest) = *marked.iter().min()?;
    Some((marked.len(), p.label(oldest)))
}

/// The status bar's right end (S4): ` ◆ claude@work ` (`+N` when several) and ` f12 `, or ` ◆ ` and
/// ` f12 ` when the bar is narrow. `None` without an unseen mark or when even the short form would
/// leave the bar under 8 cells. The bar draws into what is left.
pub fn badge(app: &App, width: u16) -> Option<(String, String)> {
    let (n, label) = unseen(app)?;
    let more = if n > 1 {
        format!(" +{}", n - 1)
    } else {
        String::new()
    };
    let key = format!(" {} ", app.key_hint("ai.toggle"));
    let full = format!(" ◆ {label}{more} ");
    let compact = format!(" ◆{more} ");
    let width = usize::from(width);
    if width_of(&full) + width_of(&key) + 20 <= width {
        Some((full, key))
    } else if width_of(&compact) + width_of(&key) + 8 <= width {
        Some((compact, key))
    } else {
        None
    }
}

pub fn render_badge(buf: &mut Buffer, area: Rect, theme: &Theme, parts: &(String, String)) {
    let (badge, key) = parts;
    let total = (width_of(badge) + width_of(key)) as u16;
    let x = area.x + area.width - total;
    let mut style = Style::default().add_modifier(Modifier::BOLD);
    style.fg = theme.status.bg;
    style.bg = theme.attention.fg;
    buf.set_string(x, area.y, badge, style);
    buf.set_string(x + width_of(badge) as u16, area.y, key, theme.status);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notice_forms_follow_the_mockups() {
        let msg = "Allow Bash(cargo test -p vr-ide)?";
        assert_eq!(
            notice_text("claude@work", "vr-ide", msg, "f12", 120),
            "◆ claude@work (vr-ide): Allow Bash(cargo test -p vr-ide)?  f12 shows it"
        );
        let narrow = notice_text("claude@work", "vr-ide", msg, "f12", 60);
        assert_eq!(
            narrow,
            "◆ claude@work: Allow Bash(cargo test -p vr-ide)? f12"
        );
        let cut = notice_text("claude@work", "vr-ide", msg, "f12", 40);
        assert!(cut.ends_with("… f12") && width_of(&cut) <= 40, "{cut}");
    }

    #[test]
    fn message_is_sanitized_capped_and_never_empty() {
        assert_eq!(message(None), "rang the bell");
        assert_eq!(message(Some("  ")), "rang the bell");
        let m = message(Some("\x1b[2Jhi"));
        assert!(!m.contains('\x1b') && m.ends_with("hi"), "{m:?}");
        assert!(message(Some(&"a".repeat(10_000))).chars().count() <= MESSAGE_MAX);
    }
}
