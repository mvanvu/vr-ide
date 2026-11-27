//! AI changes on screen (ui-spec S3, L2-A): the list at the top of the session's tab, the diff
//! tab's header row and line decorations, and the `DIFF` status bar.

use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use crate::ai::AiPanel;
use crate::ai::changes::{DiffLine, Incomplete, Kind, Record};
use crate::ai::review;
use crate::app::App;
use crate::theme::Theme;

/// Screen lines of the "may be incomplete" banner.
const BANNER_LINES: u16 = 2;
/// Under this many cells for the folder of any drawn row, the list shows no folders.
const FOLDER_MIN: usize = 10;
/// Narrower than this, the diff header leaves the keys to the status bar.
const HEAD_KEYS_MIN: u16 = 50;

/// A space the banner's text is not broken at.
const GLUE: char = '\u{a0}';

/// The banner's text in lines for a panel `width` cells wide: one cell free on both sides,
/// lines after the first two cells further in.
fn banner_lines(text: &str, width: u16) -> Vec<String> {
    let wrap = crate::ui::dialog::wrap;
    let first = wrap(text, width.saturating_sub(2))
        .into_iter()
        .next()
        .unwrap_or_default();
    let rest = text[first.len()..].trim_start();
    let mut lines = vec![first];
    if !rest.is_empty() {
        lines.extend(wrap(rest, width.saturating_sub(4)));
    }
    lines.iter().map(|l| l.replace(GLUE, " ")).collect()
}

fn width_of(s: &str) -> usize {
    Span::raw(s).width()
}

/// Seconds east of UTC at `secs`, from the system's zone file (TZif: the last transition not
/// after the time decides); 0 when it cannot be read.
fn utc_offset(secs: i64) -> i64 {
    if cfg!(test) {
        return 0;
    }
    static ZONE: std::sync::OnceLock<Vec<(i64, i64)>> = std::sync::OnceLock::new();
    let zone = ZONE.get_or_init(|| {
        std::fs::read("/etc/localtime")
            .ok()
            .and_then(|b| parse_tzif(&b))
            .unwrap_or_default()
    });
    zone.iter()
        .rev()
        .find(|(at, _)| *at <= secs)
        .or(zone.first())
        .map_or(0, |(_, off)| *off)
}

/// (transition time, offset after it), oldest first; the first entry is the zone's base.
fn parse_tzif(b: &[u8]) -> Option<Vec<(i64, i64)>> {
    let be = |at: usize, n: usize| -> Option<i64> {
        let s = b.get(at..at + n)?;
        Some(match n {
            4 => i64::from(i32::from_be_bytes(s.try_into().ok()?)),
            _ => i64::from_be_bytes(s.try_into().ok()?),
        })
    };
    let header = |at: usize| -> Option<[usize; 6]> {
        if b.get(at..at + 4)? != b"TZif" {
            return None;
        }
        let mut c = [0usize; 6];
        for (i, v) in c.iter_mut().enumerate() {
            *v = usize::try_from(be(at + 20 + 4 * i, 4)?).ok()?;
        }
        Some(c)
    };
    // Counts: isut, isstd, leap, time, type, char.
    let mut at = 0;
    let mut c = header(at)?;
    let mut size = 4;
    if b.get(4).is_some_and(|v| *v >= b'2') {
        at += 44 + c[3] * 5 + c[4] * 6 + c[5] + c[2] * 8 + c[1] + c[0];
        c = header(at)?;
        size = 8;
    }
    let times = at + 44;
    let idx = times + c[3] * size;
    let types = idx + c[3];
    let offset = |t: usize| be(types + 6 * t, 4);
    let mut out = vec![(i64::MIN, offset(0)?)];
    for i in 0..c[3] {
        let t = usize::from(*b.get(idx + i)?);
        out.push((be(times + i * size, size)?, offset(t)?));
    }
    Some(out)
}

/// `14:32`, local time.
pub fn clock(t: SystemTime) -> String {
    let secs = t
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let local = (secs + utc_offset(secs)).rem_euclid(86_400);
    format!("{:02}:{:02}", local / 3600, local % 3600 / 60)
}

/// `±N` for the active session's changes (tab bar, status bar); nothing without any.
pub fn badge(app: &App) -> Option<String> {
    let n = app.ai.active_session()?.records.len();
    (n > 0).then(|| format!("±{n}"))
}

/// The counts column of a row: `+12 -3`, `+88`, `-40`, or why there is nothing to compare.
fn counts(r: &Record) -> String {
    if let Some(why) = r.missing {
        return why.short().to_string();
    }
    match (r.kind, r.added, r.removed) {
        (Kind::Created, Some(a), _) => format!("+{a}"),
        (Kind::Deleted, _, Some(d)) => format!("-{d}"),
        (Kind::Modified, Some(a), Some(d)) => format!("+{a} -{d}"),
        (Kind::Created, ..) => "new".to_string(),
        (Kind::Deleted, ..) => "deleted".to_string(),
        (Kind::Modified, ..) => "changed".to_string(),
    }
}

fn kind_style(kind: Kind, theme: &Theme) -> Style {
    match kind {
        Kind::Modified => theme.git_modified,
        Kind::Created => theme.git_added,
        Kind::Deleted => theme.git_deleted,
    }
    .add_modifier(Modifier::BOLD)
}

/// Keeps the end of `s` within `room` cells, the cut marked with `…`.
fn cut_left(s: &str, room: usize) -> String {
    crate::ui::status_bar::shorten_left(s, room)
}

/// Keeps the start of `s` within `room` cells, the cut marked with `…`.
fn cut_right(s: &str, room: usize) -> String {
    if width_of(s) <= room {
        return s.to_string();
    }
    let mut out = String::new();
    for c in s.chars() {
        if width_of(&out) + width_of(c.encode_utf8(&mut [0; 4])) + 1 > room {
            break;
        }
        out.push(c);
    }
    out.push('…');
    out
}

// ---- the list ---------------------------------------------------------------------------------

/// How the open list is laid out over a session area `height` rows tall.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListLayout {
    /// Rows taken from the top of the session, the rule included.
    pub total: u16,
    pub banner: bool,
    /// File rows drawn and the first one.
    pub files: usize,
    pub top: usize,
    /// No records: one line says so, the statement is left out.
    pub empty: bool,
}

/// `None` while the list is closed or the picker shows.
pub fn layout(app: &App, height: u16) -> Option<ListLayout> {
    let list = app.ai.list.as_ref()?;
    let s = app.ai.active_session()?;
    let (n, banner) = (s.records.len(), s.banner.is_some());
    // Up to a third of the panel, at least three rows; the session keeps a row.
    let mut files = n.min(usize::from((height / 3).max(3)));
    let fixed = 2 + if banner { BANNER_LINES } else { 0 };
    let body = |files: usize| if n == 0 { 1 } else { files as u16 + 2 };
    while files > 1 && fixed + body(files) > height.saturating_sub(1) {
        files -= 1;
    }
    let selected = list.selected.saturating_sub(usize::from(banner));
    let mut top = list.top.min(n.saturating_sub(files));
    if selected < top {
        top = selected;
    } else if files > 0 && selected >= top + files {
        top = selected + 1 - files;
    }
    Some(ListLayout {
        total: (fixed + body(files)).min(height),
        banner,
        files,
        top,
        empty: n == 0,
    })
}

/// The row of [`review::rows`] drawn on line `line` of the list (0 = its header).
pub fn row_at(app: &App, line: u16) -> Option<usize> {
    let l = layout(app, app.layout.ai.height.saturating_sub(1))?;
    let mut at = line.checked_sub(1)?;
    if l.banner {
        if at < BANNER_LINES {
            return Some(0);
        }
        at -= BANNER_LINES;
    }
    (usize::from(at) < l.files).then(|| usize::from(l.banner) + l.top + usize::from(at))
}

/// Draws the list at the top of `area` (the session's area) and returns the rows it took.
pub fn render_list(buf: &mut Buffer, area: Rect, app: &mut App, theme: &Theme) -> u16 {
    let Some(l) = layout(app, area.height) else {
        return 0;
    };
    if let Some(list) = &mut app.ai.list {
        list.top = l.top;
    }
    let (Some(s), Some(list)) = (app.ai.active_session(), app.ai.list.as_ref()) else {
        return 0;
    };
    let w = usize::from(area.width);
    let bottom = area.y + l.total;
    let label = app.ai.label(app.ai.active);
    let focused = review::list_has_keys(app);
    let line = |buf: &mut Buffer, y: u16, style: Style| {
        buf.set_style(Rect::new(area.x, y, area.width, 1), style);
    };
    let mut y = area.y;
    // Header on the raised surface.
    line(buf, y, theme.popup);
    let n = s.records.len();
    let head = format!(
        " Changes · {label} · {n} file{}",
        if n == 1 { "" } else { "s" }
    );
    let key = format!("{} ", app.key_hint("ai.changes"));
    let room = w.saturating_sub(width_of(&key) + 1);
    buf.set_stringn(
        area.x,
        y,
        cut_right(&head, room),
        w,
        theme.popup.add_modifier(Modifier::BOLD),
    );
    if w > width_of(&key) {
        let kx = area.x + (w - width_of(&key)) as u16;
        buf.set_stringn(kx, y, &key, w, theme.popup.patch(theme.dim));
    }
    y += 1;
    let picked = |row: usize| {
        (row == list.selected).then_some(if focused {
            theme.list_selected
        } else {
            theme.selection
        })
    };
    if let Some(b) = s.banner.filter(|_| y + BANNER_LINES <= bottom) {
        // Short enough for the hint to stay within the two lines at 80 columns; the last word,
        // the time and the hint are not broken apart, so a line never starts with the separator.
        let hint = format!("·{GLUE}enter{GLUE}hides");
        let text = match b.why {
            // Ends with the walk, not by a key.
            Incomplete::Scanning => format!("! List may be incomplete ({})", b.why.text()),
            // What a walk did not see in time.
            Incomplete::Unwalked | Incomplete::WalkedLate => {
                format!("! Incomplete: {}{GLUE}{hint}", b.why.text())
            }
            _ => format!(
                "! Incomplete: {} at{GLUE}{}{GLUE}{hint}",
                b.why.text(),
                clock(b.at)
            ),
        };
        let mut lines = banner_lines(&text, area.width);
        lines.truncate(usize::from(BANNER_LINES));
        for (i, text) in lines.iter().enumerate() {
            let style = picked(0).map_or(theme.warning, |p| theme.warning.patch(bg_of(p)));
            line(buf, y, theme.text.patch(style));
            let x = area.x + if i == 0 { 1 } else { 3 };
            buf.set_stringn(x, y, text, w.saturating_sub(2), style);
            y += 1;
        }
        y = area.y + 1 + BANNER_LINES;
    }
    if l.empty && y < bottom {
        line(buf, y, theme.text);
        let style = theme.note.add_modifier(Modifier::ITALIC);
        buf.set_stringn(area.x + 1, y, "No changes since it started", w - 1, style);
        y += 1;
    }
    // ` M name [folder] counts `: the name gives way last. The folder is a column of the
    // list: shown when every drawn row that has one leaves it its minimum, else in no row.
    let name_room = |r: &Record| w.saturating_sub(3 + width_of(&counts(r)) + 2);
    let folder_room = |r: &Record| {
        let name = cut_right(&review::shown_name(&r.rel), name_room(r));
        w.saturating_sub(3 + width_of(&name) + 1 + width_of(&counts(r)) + 2)
    };
    let has_folder = |r: &Record| r.rel.parent().is_some_and(|p| !p.as_os_str().is_empty());
    let folders = s
        .records
        .iter()
        .skip(l.top)
        .take(l.files)
        .filter(|r| has_folder(r))
        .all(|r| folder_room(r) >= FOLDER_MIN);
    for (i, r) in s.records.iter().enumerate().skip(l.top).take(l.files) {
        if y >= bottom {
            break;
        }
        let sel = picked(i + usize::from(l.banner));
        let base = sel.map_or(theme.text, |p| theme.text.patch(p));
        line(buf, y, base);
        let on = |s: Style| match sel {
            Some(p) => s.patch(bg_of(p)),
            None => s,
        };
        let right = counts(r);
        let rw = width_of(&right);
        let name_room = name_room(r);
        let name = cut_right(&review::shown_name(&r.rel), name_room);
        buf.set_stringn(
            area.x + 1,
            y,
            r.kind.letter(),
            1,
            on(kind_style(r.kind, theme)),
        );
        buf.set_stringn(area.x + 3, y, &name, name_room, base);
        let used = 3 + width_of(&name);
        let folder = r
            .rel
            .parent()
            .map(review::shown_path)
            .filter(|f| !f.is_empty());
        let room = w.saturating_sub(used + 1 + rw + 2);
        if let Some(f) = folder.filter(|_| folders) {
            let f = cut_left(&f, room);
            let style = theme.note.fg.map_or(base, |fg| base.fg(fg));
            buf.set_stringn(area.x + used as u16 + 1, y, &f, room, on(style));
        }
        if w > rw + 1 {
            let style = if r.missing.is_some() {
                theme.warning
            } else {
                theme.dim
            };
            let x = area.x + (w - rw - 1) as u16;
            buf.set_stringn(x, y, &right, rw, on(base.patch(style)));
        }
        y += 1;
    }
    if !l.empty {
        // D8: what the list counts, always in view.
        let folder = s.root.file_name().map_or_else(
            || s.root.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let folder = crate::ui::safe_text::sanitize(&folder);
        let style = theme.note.add_modifier(Modifier::ITALIC);
        for text in [
            "Counts edits by any program in".to_string(),
            format!("{folder} while {label} runs"),
        ] {
            if y < bottom.saturating_sub(1) {
                line(buf, y, theme.text);
                buf.set_stringn(area.x + 1, y, cut_right(&text, w - 1), w - 1, style);
                y += 1;
            }
        }
    }
    if bottom > area.y {
        let y = bottom - 1;
        line(buf, y, theme.text);
        buf.set_stringn(area.x, y, "─".repeat(w), w, theme.dim);
    }
    l.total
}

fn bg_of(s: Style) -> Style {
    s.bg.map_or(Style::default(), |bg| Style::default().bg(bg))
}

// ---- the diff tab -----------------------------------------------------------------------------

/// The lines of the diff tab when the view titled `title` is it.
pub fn decor<'a>(ai: &'a AiPanel, title: Option<&str>) -> Option<&'a [DiffLine]> {
    (title == Some(review::DIFF_TITLE))
        .then_some(ai.diff.as_ref())
        .flatten()
        .map(|d| d.diff.lines.as_slice())
}

/// Bytes of one line the colours are asked for (a minified line is coloured at its start).
const LINE_SPAN_MAX: usize = 16 * 1024;

/// Syntax spans of the diff tab's rows `top..top + rows`, in bytes of its buffer `rope`
/// (sorted, disjoint): every code line is looked up in the text it is from, a removed line in
/// the earlier one. Empty for plain text and while the highlighters still parse.
pub fn diff_spans(
    app: &App,
    rope: &ropey::Rope,
    top: usize,
    rows: usize,
) -> Vec<(std::ops::Range<usize>, vr_syntax::Capture)> {
    use crate::ai::changes::LineKind as L;
    let Some(d) = app.ai.diff.as_ref() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let last = (top + rows).min(d.diff.lines.len()).min(rope.len_lines());
    for (ln, l) in d.diff.lines.iter().enumerate().take(last).skip(top) {
        let hl = match l.kind {
            L::Removed => d.syntax.old.as_ref(),
            L::Context | L::Added => d.syntax.new.as_ref(),
            L::Hunk | L::Note => None,
        };
        let (Some(hl), Some(at)) = (hl, l.at) else {
            continue;
        };
        let (start, len) = (rope.line_to_byte(ln), l.text.len().min(LINE_SPAN_MAX));
        for (r, cap) in app.syntax.spans_of(hl, at..at + len) {
            let (from, to) = (r.start.max(at), r.end.min(at + len));
            if from < to {
                out.push((start + from - at..start + to - at, cap));
            }
        }
    }
    out
}

/// Columns of the diff gutter: line number, a space, the `+` / `-` sign.
pub fn diff_gutter(lines: &[DiffLine]) -> u16 {
    let max = lines.iter().filter_map(|l| l.num).max().unwrap_or(1).max(1);
    (max.ilog10() as u16 + 1).max(3) + 2
}

/// The diff tab's label in the editor tab bar.
pub fn tab_name(app: &App) -> String {
    let name = app
        .ai
        .diff
        .as_ref()
        .map_or_else(String::new, |d| review::shown_name(&d.rel));
    format!("{} {name}", review::DIFF_TITLE)
}

/// The header row over the diff: kind, file, counts and session; the Keep / Revert keys at the
/// right while there is room.
pub fn render_diff_head(buf: &mut Buffer, area: Rect, app: &App, theme: &Theme) {
    let Some(d) = app.ai.diff.as_ref().filter(|_| area.height > 0) else {
        return;
    };
    buf.set_style(area, theme.popup);
    let w = usize::from(area.width);
    let label = app
        .ai
        .sessions
        .iter()
        .position(|s| s.serial == d.session)
        .map_or_else(String::new, |i| app.ai.label(i));
    let name = review::shown_name(&d.rel);
    let what = match (d.diff.missing, d.diff.kind) {
        (Some(why), _) => why.long().to_string(),
        (None, Kind::Modified) => format!("+{} -{}", d.diff.added, d.diff.removed),
        (None, Kind::Created) => format!("new · +{}", d.diff.added),
        (None, Kind::Deleted) => format!("deleted · -{}", d.diff.removed),
    };
    let keep = format!("{} keep", app.key_hint("ai.keep"));
    let keys = if d.diff.can_revert {
        format!("{keep}  {} revert", app.key_hint("ai.revert"))
    } else {
        keep
    };
    let full = format!("{what} · {label}");
    let fits = |text: &str, with_keys: bool| {
        let right = if with_keys { 2 + width_of(&keys) } else { 0 };
        5 + width_of(&name) + width_of(text) + right <= w
    };
    let with_keys = area.width >= HEAD_KEYS_MIN;
    let (text, show_keys) = if with_keys && fits(&full, true) {
        (full, true)
    } else if with_keys && fits(&what, true) {
        (what, true)
    } else if fits(&full, false) {
        (full, false)
    } else {
        (what, false)
    };
    buf.set_stringn(
        area.x + 1,
        area.y,
        d.diff.kind.letter(),
        1,
        theme.popup.patch(kind_style(d.diff.kind, theme)),
    );
    let room = w.saturating_sub(3);
    let name = cut_right(&name, room);
    buf.set_stringn(area.x + 3, area.y, &name, room, theme.popup);
    let x = 3 + width_of(&name) + 2;
    if x < w {
        let style = if d.diff.missing.is_some() {
            theme.warning
        } else {
            theme.dim
        };
        buf.set_stringn(
            area.x + x as u16,
            area.y,
            &text,
            w - x,
            theme.popup.patch(style),
        );
    }
    if show_keys {
        let kw = width_of(&keys);
        let kx = area.x + (w - kw) as u16;
        buf.set_stringn(kx, area.y, &keys, kw, theme.popup.patch(theme.warning));
    }
}

/// The style of a diff line's text and the background of its row.
pub fn line_style(l: &DiffLine, theme: &Theme) -> (Style, Option<ratatui::style::Color>) {
    use crate::ai::changes::LineKind as L;
    match l.kind {
        L::Hunk => {
            let mut s = Style::default().add_modifier(Modifier::ITALIC);
            if let Some(c) = theme.mode_ai.bg {
                s = s.fg(c);
            }
            (theme.text.patch(s), None)
        }
        L::Note => (
            theme.text.patch(theme.note).add_modifier(Modifier::ITALIC),
            None,
        ),
        L::Context => (theme.text, None),
        L::Added => (
            theme.text.bg(theme.diff_added_bg),
            Some(theme.diff_added_bg),
        ),
        L::Removed => (
            theme.text.bg(theme.diff_removed_bg),
            Some(theme.diff_removed_bg),
        ),
    }
}

/// The gutter of a diff line: its number, then `+` / `-` (never colour alone).
pub fn gutter_text(
    l: Option<&DiffLine>,
    width: u16,
    theme: &Theme,
) -> (String, &'static str, Style) {
    use crate::ai::changes::LineKind as L;
    let w = usize::from(width.saturating_sub(2));
    let num = l
        .and_then(|l| l.num)
        .map_or_else(|| " ".repeat(w), |n| format!("{n:>w$}"));
    let (sign, style) = match l.map(|l| l.kind) {
        Some(L::Added) => ("+", theme.git_added.add_modifier(Modifier::BOLD)),
        Some(L::Removed) => ("-", theme.git_deleted.add_modifier(Modifier::BOLD)),
        _ => (" ", theme.text),
    };
    (format!("{num} "), sign, style)
}

// ---- status bar -------------------------------------------------------------------------------

/// The `DIFF` status bar while the list has the keys on a file, or the editor on the diff tab:
/// path, session and since when; which file of how many and the Keep / Revert keys. False
/// when neither is the case.
pub fn render_status(buf: &mut Buffer, area: Rect, app: &App, theme: &Theme) -> bool {
    let Some((si, r, at)) = review::status_target(app) else {
        return false;
    };
    let width = usize::from(area.width);
    let badge = if width >= 50 { " DIFF " } else { " D " };
    buf.set_stringn(area.x, area.y, badge, width, theme.mode_ai);
    let bw = width_of(badge).min(width);
    let label = app.ai.label(si);
    let n = app.ai.sessions[si].records.len();
    let (path, name) = (review::shown_path(&r.rel), review::shown_name(&r.rel));
    let tail = match r.missing {
        Some(why) => why.long().to_string(),
        None => format!("since {}", clock(r.since)),
    };
    let keep = format!("{} keep", app.key_hint("ai.keep"));
    let act = if r.can_revert() {
        format!("{keep} · {} revert", app.key_hint("ai.revert"))
    } else {
        format!("{keep} · revert off")
    };
    let lefts = [
        format!(" {path} · {label} · {tail}"),
        format!(" {path} · {label}"),
        format!(" {name} · {label}"),
        format!(" {name}"),
    ];
    let rights = [format!(" {}/{n} · {act} ", at + 1), format!(" {act} ")];
    let room = width - bw;
    let mut chosen = None;
    'fit: for l in &lefts {
        for r in &rights {
            if width_of(l) + width_of(r) <= room {
                chosen = Some((l.clone(), r.clone()));
                break 'fit;
            }
        }
    }
    let (left, right) = chosen.unwrap_or_else(|| {
        let r = rights[1].clone();
        let l = cut_right(&lefts[3], room.saturating_sub(width_of(&r)));
        (l, r)
    });
    buf.set_stringn(area.x + bw as u16, area.y, &left, room, theme.status);
    let rw = width_of(&right).min(room);
    buf.set_stringn(
        area.x + (width - rw) as u16,
        area.y,
        &right,
        rw,
        theme.status,
    );
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A version-1 zone file: base offset 0, one transition at t = 1000 to +07:00.
    #[test]
    fn zone_file_offsets_are_read_and_damaged_files_refused() {
        let mut b = b"TZif".to_vec();
        b.extend([0u8; 16]);
        for count in [0u32, 0, 0, 1, 2, 4] {
            b.extend(count.to_be_bytes());
        }
        b.extend(1000i32.to_be_bytes());
        b.push(1);
        for off in [0i32, 25_200] {
            b.extend(off.to_be_bytes());
            b.extend([0, 0]);
        }
        b.extend(b"UTC\0");
        assert_eq!(parse_tzif(&b), Some(vec![(i64::MIN, 0), (1000, 25_200)]));
        assert_eq!(parse_tzif(&b[..50]), None);
        assert_eq!(parse_tzif(b"not a zone file"), None);
        assert_eq!(
            clock(UNIX_EPOCH + std::time::Duration::from_secs(52_320)),
            "14:32"
        );
    }
}
