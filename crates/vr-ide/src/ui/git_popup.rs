//! Draws the Git popup (M7-3): header with the repo, branch and tab, banners, the two file
//! lists and the diff (side by side, stacked when narrow), the message box, the toggles and
//! buttons, the progress / queue line and a key hint. Every git-derived string was passed
//! through `safe_text::sanitize` by `git_popup` or is sanitized here (SEC-15).

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use vr_git::{FileStatus, LineKind, StatusEntry};

use crate::app::App;
use crate::git_popup::{Areas, DiffRow, GitPopup, Pane, Tab};
use crate::theme::Theme;
use crate::ui::safe_text::sanitize;

/// Inner width from which the lists and the diff sit side by side.
const SIDE_BY_SIDE: u16 = 64;

pub(crate) fn width(s: &str) -> usize {
    Span::raw(s).width()
}

/// `s` cut from the left to `cells`, starting with "…" when cut (paths keep their end).
pub(crate) fn fit_tail(s: &str, cells: usize) -> String {
    if width(s) <= cells {
        return s.to_string();
    }
    if cells == 0 {
        return String::new();
    }
    let mut out: Vec<char> = Vec::new();
    let mut used = 0;
    for ch in s.chars().rev() {
        let cw = width(&ch.to_string());
        if used + cw + 1 > cells {
            break;
        }
        out.push(ch);
        used += cw;
    }
    out.push('…');
    out.into_iter().rev().collect()
}

/// The popup's rectangle: about 90 % of the screen above the status row, the whole of it
/// when small.
pub fn popup_rect(area: Rect) -> Rect {
    let avail = Rect::new(
        area.x,
        area.y,
        area.width,
        area.height.saturating_sub(1).max(1),
    );
    if avail.width >= 60 && avail.height >= 16 {
        let w = avail.width * 9 / 10;
        let h = avail.height * 9 / 10;
        Rect::new(
            avail.x + (avail.width - w) / 2,
            avail.y + (avail.height - h) / 2,
            w,
            h,
        )
    } else {
        avail
    }
}

pub fn status_letter(s: FileStatus) -> char {
    match s {
        FileStatus::Modified => 'M',
        FileStatus::Added => 'A',
        FileStatus::Deleted => 'D',
        FileStatus::Renamed => 'R',
        FileStatus::Copied => 'C',
        FileStatus::TypeChanged => 'T',
        FileStatus::Untracked => '?',
        FileStatus::Conflicted => 'U',
    }
}

fn entry_text(e: &StatusEntry) -> String {
    let path = sanitize(&e.path.to_string_lossy());
    match &e.orig_path {
        Some(o) => format!("{} → {path}", sanitize(&o.to_string_lossy())),
        None => path,
    }
}

pub(crate) fn put(buf: &mut Buffer, x: u16, y: u16, s: &str, w: u16, style: Style) {
    if w > 0 {
        buf.set_stringn(x, y, s, usize::from(w), style);
    }
}

pub fn render(frame: &mut Frame, area: Rect, app: &mut App, theme: &Theme) {
    let now = crate::git_history::now_secs(app.git.now_secs);
    let at = app.now();
    let registry = app.syntax.registry().clone();
    let notify = app.syntax.notifier();
    let Some(p) = &mut app.git_popup else {
        return;
    };
    if !p.visible || area.width < 10 || area.height < 6 {
        return;
    }
    let rect = popup_rect(area);
    let mut areas = Areas {
        popup: rect,
        ..Areas::default()
    };
    let buf = frame.buffer_mut();
    ratatui::widgets::Widget::render(ratatui::widgets::Clear, rect, buf);
    let block = ratatui::widgets::Block::bordered()
        .style(theme.popup)
        .border_style(theme.popup_border);
    ratatui::widgets::Widget::render(block, rect, buf);
    header(buf, rect, p, theme, &mut areas);
    let inner = Rect::new(
        rect.x + 1,
        rect.y + 1,
        rect.width.saturating_sub(2),
        rect.height.saturating_sub(2),
    );
    if inner.width < 4 || inner.height < 4 {
        p.areas = areas;
        return;
    }
    if p.tab != Tab::Changes || p.commit.is_some() {
        let cx = crate::ui::git_history::Ctx {
            theme,
            now,
            at,
            registry: &registry,
            notify: &notify,
        };
        crate::ui::git_history::render(buf, inner, p, &cx, &mut areas);
        p.areas = areas;
        return;
    }
    let (x, w) = (inner.x, inner.width);
    let mut top = inner.y;
    let mut bottom = inner.y + inner.height;

    // Bottom up: hint, status line, buttons, message box.
    if inner.height >= 16 {
        bottom -= 1;
        // Cut with a marker, so a missing key is never silently gone (M7-3 QC).
        put(
            buf,
            x,
            bottom,
            &fit_head(&hint(p), usize::from(w)),
            w,
            theme.popup.patch(theme.dim),
        );
    }
    let status = p.status_line();
    let status_style = if p.job.is_some() || !p.queue.is_empty() {
        theme.popup.patch(theme.info)
    } else {
        theme.popup.patch(theme.dim)
    };
    // A small popup gives an empty status row to the lists and the diff.
    if !status.is_empty() || inner.height >= 12 {
        bottom -= 1;
        put(buf, x, bottom, &status, w, status_style);
    }
    let rows = button_rows(p, w);
    bottom = bottom.saturating_sub(rows.len() as u16);
    for (i, row) in rows.iter().enumerate() {
        let y = bottom + i as u16;
        let mut bx = x;
        for &(pane, ref label) in row {
            let lw = width(label) as u16;
            let style = if p.focus == pane {
                theme.button_selected
            } else {
                theme.button
            };
            put(buf, bx, y, label, lw.min(x + w - bx), style);
            areas
                .buttons
                .push((Rect::new(bx, y, lw.min(x + w - bx), 1), pane));
            bx += lw + 1;
        }
    }
    let mh: u16 = match inner.height {
        h if h >= 24 => 4,
        h if h >= 16 => 2,
        _ => 1,
    };
    bottom = bottom.saturating_sub(mh);
    let msg_rect = Rect::new(x, bottom, w, mh);
    areas.message = msg_rect;
    let cursor = message_box(buf, msg_rect, p, theme);
    // A rule with the box's name above it when there is room (as in the proposal's sketch).
    let rule = inner.height >= 12;
    if rule {
        bottom -= 1;
        let line: String = "─".repeat(usize::from(rect.width));
        put(buf, rect.x, bottom, &line, rect.width, theme.popup_border);
        put(buf, rect.x, bottom, "├", 1, theme.popup_border);
        put(
            buf,
            rect.x + rect.width - 1,
            bottom,
            "┤",
            1,
            theme.popup_border,
        );
        put(
            buf,
            x + 1,
            bottom,
            " Message ",
            w.saturating_sub(1),
            theme.popup_border,
        );
    }

    // Top down: banners.
    let max_banners = if inner.height >= 14 { 2 } else { 1 };
    for (error, text) in p.banners().into_iter().take(max_banners) {
        if top + 1 >= bottom {
            break;
        }
        let style = if error {
            theme.popup.patch(theme.error)
        } else {
            theme.popup.patch(theme.warning)
        };
        // A banner too wide for one row names its programs first (SEC-19 wants the names
        // seen) and wraps to a second row while the lists keep 3 rows; what is still left ends
        // with "…" (M7-3 QC).
        let rows_left = bottom.saturating_sub(top);
        let max_rows = if rows_left >= 5 { 2 } else { 1 };
        let mut rows = wrap(&format!("! {text}"), usize::from(w), max_rows);
        let cut = rows.last().is_some_and(|r| r.ends_with('…'));
        if cut && let Some(names) = text.strip_prefix(PROGRAMS_LEAD) {
            rows = wrap(
                &format!("! writes run programs: {names}"),
                usize::from(w),
                max_rows,
            );
        }
        for row in rows {
            buf.set_style(Rect::new(x, top, w, 1), style);
            put(buf, x, top, &row, w, style);
            top += 1;
        }
    }
    let body = Rect::new(x, top, w, bottom.saturating_sub(top));
    if body.height > 0 {
        let (lists, diff) = if w >= SIDE_BY_SIDE {
            let lw = (w * 2 / 5).clamp(22, 50);
            for y in body.y..body.y + body.height {
                buf.set_string(body.x + lw, y, "│", theme.popup_border);
            }
            if rule {
                buf.set_string(body.x + lw, body.y + body.height, "┴", theme.popup_border);
            }
            (
                Rect::new(body.x, body.y, lw, body.height),
                Rect::new(body.x + lw + 1, body.y, w - lw - 1, body.height),
            )
        } else {
            // Stacked: the diff gets the larger part while it has the keys.
            let dh = if p.focus == Pane::Diff {
                body.height * 2 / 3
            } else {
                body.height / 2
            }
            .max(1)
            .min(body.height);
            let lh = body.height - dh;
            (
                Rect::new(body.x, body.y, w, lh),
                Rect::new(body.x, body.y + lh, w, dh),
            )
        };
        file_lists(buf, lists, p, theme, &mut areas);
        diff_pane(buf, diff, p, theme, &mut areas);
    }
    p.areas = areas;
    if let Some(c) = cursor {
        frame.set_cursor_position(c);
    }
}

/// The program banner's lead-in (`GitPopup::banners`); narrow screens use a shorter one.
const PROGRAMS_LEAD: &str = "this repo's config runs programs on writes: ";

/// `s` in at most `rows` rows of `w` cells, broken at spaces (continuation rows indented by
/// two); the last row ends with "…" when text is left over.
fn wrap(s: &str, w: usize, rows: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = s.to_string();
    while !rest.is_empty() && out.len() < rows {
        let lead = if out.is_empty() { "" } else { "  " };
        let room = w.saturating_sub(lead.len());
        if width(&rest) <= room {
            out.push(format!("{lead}{rest}"));
            rest.clear();
            break;
        }
        if out.len() + 1 == rows {
            out.push(format!("{lead}{}", fit_head(&rest, room)));
            rest.clear();
            break;
        }
        // The longest head of whole words that fits; a single long word is cut.
        let head = fit_head(&rest, room + 1);
        let head = head.strip_suffix('…').unwrap_or(&head).to_string();
        if head.is_empty() {
            break;
        }
        // Between list items first ("a, b"), so a name such as `hook pre-push` stays whole.
        // An item ending exactly at the edge (`…, name,` then a space) is whole too.
        let at_edge = head.ends_with(',') && rest[head.len()..].starts_with(' ');
        let cut = match (head.rfind(", "), head.rfind(' ')) {
            _ if at_edge => head.len(),
            (Some(i), _) if i > 0 => i + 1,
            (_, Some(i)) if i > 0 => i,
            _ => head.len(),
        };
        out.push(format!("{lead}{}", &rest[..cut]));
        rest = rest[cut..].trim_start().to_string();
    }
    out
}

/// Top border: ` Git · ~/repo · main ↑1 ↓0 · parent repo of <root> ` and the tabs on the
/// right (all three, or the shown one alone when narrow).
fn header(buf: &mut Buffer, rect: Rect, p: &GitPopup, theme: &Theme, areas: &mut Areas) {
    let names = [
        (Tab::Changes, " Changes "),
        (Tab::History, " History "),
        (Tab::Blame, " Blame "),
    ];
    let all: u16 = names.iter().map(|(_, n)| width(n) as u16).sum();
    let room = rect.width.saturating_sub(4);
    let shown: Vec<(Tab, &str)> = if room >= all + 16 {
        names.to_vec()
    } else {
        names.iter().copied().filter(|(t, _)| *t == p.tab).collect()
    };
    let tw: u16 = shown.iter().map(|(_, n)| width(n) as u16).sum();
    let mut title = format!(" Git · {}", p.repo_label);
    let branch = p.branch_text();
    if !branch.is_empty() {
        title.push_str(&format!(" · {branch}"));
    }
    if let Some(root) = &p.parent_of {
        title.push_str(&format!(" · parent repo of {root}"));
    }
    title.push(' ');
    let show_tab = room >= tw + 12;
    let title_room = if show_tab { room - tw - 1 } else { room };
    let title = fit_tail(&title, usize::from(title_room));
    put(
        buf,
        rect.x + 2,
        rect.y,
        &title,
        title_room,
        theme.popup_border,
    );
    if show_tab {
        let mut tx = rect.x + rect.width - 2 - tw;
        for (tab, name) in shown {
            let nw = width(name) as u16;
            let style = if tab == p.tab {
                theme.tab_active
            } else {
                theme.tab_inactive
            };
            put(buf, tx, rect.y, name, nw, style);
            areas.tabs.push((Rect::new(tx, rect.y, nw, 1), tab));
            tx += nw;
        }
    }
}

fn hint(p: &GitPopup) -> String {
    match p.focus {
        Pane::Unstaged | Pane::Staged => {
            "space stage/unstage · enter hunk · d discard · c message · p push · r rescan · tab next · esc close"
        }
        Pane::Diff => {
            "↑↓ move · ←→ scroll · shift+↑↓ lines · enter stage/unstage · d discard · tab next · esc close"
        }
        Pane::Message => "ctrl+s commit · tab next · esc close",
        _ => "space/enter choose · ←→ move · tab next · esc close",
    }
    .to_string()
}

/// Toggles and buttons wrapped into rows of `w` cells.
fn button_rows(p: &GitPopup, w: u16) -> Vec<Vec<(Pane, String)>> {
    let mut rows: Vec<Vec<(Pane, String)>> = vec![Vec::new()];
    let mut used = 0usize;
    for pane in [
        Pane::Amend,
        Pane::SignOff,
        Pane::Commit,
        Pane::Push,
        Pane::Pull,
        Pane::Rescan,
    ] {
        let label = match pane {
            Pane::Amend | Pane::SignOff => p.button_label(pane),
            _ => format!("[{}]", p.button_label(pane)),
        };
        let lw = width(&label);
        let last = rows.last_mut().expect("one row");
        if !last.is_empty() && used + 1 + lw > usize::from(w) {
            rows.push(vec![(pane, label)]);
            used = lw;
        } else {
            used += if last.is_empty() { lw } else { lw + 1 };
            last.push((pane, label));
        }
    }
    rows
}

/// The commit message box; returns the terminal cursor when it has the focus.
fn message_box(buf: &mut Buffer, r: Rect, p: &GitPopup, theme: &Theme) -> Option<(u16, u16)> {
    let focused = p.focus == Pane::Message;
    let style = if focused {
        theme.input
    } else {
        theme.popup.patch(theme.input)
    };
    buf.set_style(r, style);
    let text = p.message.text();
    if text.is_empty() && !focused {
        put(
            buf,
            r.x,
            r.y,
            "commit message (c to write, ctrl+s to commit)",
            r.width,
            style.patch(theme.dim),
        );
        return None;
    }
    let (line, col) = p.message.position();
    let lines: Vec<&str> = text.split('\n').collect();
    let h = usize::from(r.height);
    let first = (line + 1).saturating_sub(h);
    // The cursor column in cells of the shown (sanitized) text.
    let cur_line = lines.get(line).copied().unwrap_or("");
    let prefix: String = cur_line.chars().take(col).collect();
    let cx = width(&sanitize(&prefix));
    let shift = (cx + 1).saturating_sub(usize::from(r.width));
    for (i, l) in lines.iter().skip(first).take(h).enumerate() {
        let shown = sanitize(l);
        let shown: String = if i + first == line && shift > 0 {
            let mut skip = 0;
            shown
                .chars()
                .skip_while(|c| {
                    skip += width(&c.to_string());
                    skip <= shift
                })
                .collect()
        } else {
            shown
        };
        put(buf, r.x, r.y + i as u16, &shown, r.width, style);
    }
    focused.then(|| {
        (
            r.x + (cx - shift).min(usize::from(r.width.saturating_sub(1))) as u16,
            r.y + (line - first) as u16,
        )
    })
}

fn file_lists(buf: &mut Buffer, r: Rect, p: &mut GitPopup, theme: &Theme, areas: &mut Areas) {
    if r.height == 0 {
        return;
    }
    // The list with the keys (or the one the diff is of) gets the larger half.
    let staged_first = p.list_index() == 1;
    let big = r.height.div_ceil(2);
    let small = r.height - big;
    let (hu, hs) = if staged_first {
        (small, big)
    } else {
        (big, small)
    };
    let unstaged = Rect::new(r.x, r.y, r.width, hu);
    let staged = Rect::new(r.x, r.y + hu, r.width, hs);
    areas.unstaged = unstaged;
    areas.staged = staged;
    for (which, rect) in [(0usize, unstaged), (1, staged)] {
        if rect.height == 0 {
            continue;
        }
        let focused = p.focus
            == if which == 0 {
                Pane::Unstaged
            } else {
                Pane::Staged
            };
        let n = p.list(which).len();
        let name = if which == 0 { "Unstaged" } else { "Staged" };
        let head_style = if focused {
            theme.popup.patch(theme.focus)
        } else {
            theme.popup.patch(theme.dim)
        };
        put(
            buf,
            rect.x,
            rect.y,
            &format!("{name} ({n})"),
            rect.width,
            head_style,
        );
        let rows = usize::from(rect.height - 1);
        if rows == 0 {
            continue;
        }
        let sel = p.selected[which];
        let top = &mut p.list_top[which];
        if sel < *top {
            *top = sel;
        } else if sel >= *top + rows {
            *top = sel + 1 - rows;
        }
        *top = (*top).min(n.saturating_sub(rows));
        let top = *top;
        for (row, e) in p.list(which).iter().enumerate().skip(top).take(rows) {
            let y = rect.y + 1 + (row - top) as u16;
            let shown_list = usize::from(p.showing_staged) == which;
            let base = if row == sel && focused {
                theme.list_selected
            } else if row == sel && shown_list {
                theme.popup.patch(theme.selection)
            } else {
                theme.popup
            };
            buf.set_style(Rect::new(rect.x, y, rect.width, 1), base);
            let letter = status_letter(e.status);
            let lstyle = match e.status {
                FileStatus::Added | FileStatus::Untracked => base.patch(theme.git_added),
                FileStatus::Deleted | FileStatus::Conflicted => base.patch(theme.git_deleted),
                _ => base.patch(theme.git_modified),
            };
            put(buf, rect.x + 1, y, &letter.to_string(), 1, lstyle);
            let room = rect.width.saturating_sub(3);
            let text = fit_tail(&entry_text(e), usize::from(room));
            put(buf, rect.x + 3, y, &text, room, base);
        }
    }
}

fn diff_pane(buf: &mut Buffer, r: Rect, p: &mut GitPopup, theme: &Theme, areas: &mut Areas) {
    if r.height == 0 {
        return;
    }
    let focused = p.focus == Pane::Diff;
    let head_style = if focused {
        theme.popup.patch(theme.focus)
    } else {
        theme.popup.patch(theme.dim)
    };
    let rows_rect = Rect::new(r.x, r.y + 1, r.width, r.height - 1);
    areas.diff = rows_rect;
    if p.show_output && !p.output.is_empty() {
        put(buf, r.x, r.y, "git output", r.width, head_style);
        let h = usize::from(rows_rect.height);
        let start = p.output.len().saturating_sub(h);
        for (i, l) in p.output[start..].iter().enumerate() {
            put(
                buf,
                r.x,
                rows_rect.y + i as u16,
                &sanitize(l),
                r.width,
                theme.popup,
            );
        }
        return;
    }
    let title = match &p.diff {
        Some(d) => {
            let side = if d.staged { "staged" } else { "unstaged" };
            format!("{} ({side})", sanitize(&d.path.to_string_lossy()))
        }
        None if p.list(0).is_empty() && p.list(1).is_empty() && p.scan.is_some() => {
            "nothing to commit".to_string()
        }
        None => String::new(),
    };
    // Lines wider than the pane end in "…"; left / right scroll them (never cut unseen).
    let room = usize::from(r.width).saturating_sub(1);
    let widest = p.diff.as_ref().and_then(|d| d.diff()).map_or(0, |d| {
        d.hunks
            .iter()
            .flat_map(|h| &h.lines)
            .map(|l| width(&line_text(l)))
            .max()
            .unwrap_or(0)
    });
    // Scrolled lines spend one cell on the leading `…`.
    let far = if widest > room { widest + 1 - room } else { 0 };
    p.diff_left = p.diff_left.min(far);
    let mut title = title;
    if p.diff_left > 0 {
        title.push_str(&format!(" · col {}+", p.diff_left + 1));
    }
    put(
        buf,
        r.x,
        r.y,
        &fit_tail(&title, usize::from(r.width)),
        r.width,
        head_style,
    );
    let rows = p.diff_rows();
    let h = usize::from(rows_rect.height);
    if h == 0 || rows.is_empty() {
        return;
    }
    p.diff_cursor = p.diff_cursor.min(rows.len() - 1);
    if focused {
        if p.diff_cursor < p.diff_top {
            p.diff_top = p.diff_cursor;
        } else if p.diff_cursor >= p.diff_top + h {
            p.diff_top = p.diff_cursor + 1 - h;
        }
    }
    p.diff_top = p.diff_top.min(rows.len().saturating_sub(1));
    let range = p
        .diff_anchor
        .map(|a| (a.min(p.diff_cursor), a.max(p.diff_cursor)));
    let fd = p.diff.as_ref().and_then(|d| d.diff());
    for (i, row) in rows.iter().enumerate().skip(p.diff_top).take(h) {
        let y = rows_rect.y + (i - p.diff_top) as u16;
        let (text, style) = match *row {
            DiffRow::Info(_) => (p.diff_info(), theme.popup.patch(theme.dim)),
            DiffRow::Header(hk) => (
                fd.map(|d| sanitize(&d.hunks[hk].header))
                    .unwrap_or_default(),
                theme.popup.patch(theme.dim),
            ),
            DiffRow::Line(hk, l) => {
                let Some(line) = fd.map(|d| &d.hunks[hk].lines[l]) else {
                    continue;
                };
                let (mark, st) = match line.kind {
                    LineKind::Added => ('+', theme.popup.patch(theme.git_added)),
                    LineKind::Removed => ('-', theme.popup.patch(theme.git_deleted)),
                    LineKind::Context => (' ', theme.popup),
                };
                let text = line_text(line);
                if p.diff_left > 0 && width(&text) > 0 {
                    // Marks hidden text on the left, as `…` marks a cut on the right.
                    let shown = fit_head(skip_cells(&text, p.diff_left), room.saturating_sub(1));
                    (format!("{mark}…{shown}"), st)
                } else {
                    let shown = fit_head(skip_cells(&text, p.diff_left), room);
                    (format!("{mark}{shown}"), st)
                }
            }
        };
        let style = if i == p.diff_cursor && focused {
            theme.list_selected
        } else if range.is_some_and(|(a, b)| i >= a && i <= b) {
            style.patch(theme.selection)
        } else {
            style
        };
        buf.set_style(Rect::new(r.x, y, r.width, 1), style);
        let text = fit_head(&text, usize::from(r.width));
        put(buf, r.x, y, &text, r.width, style);
    }
}

/// A diff line as drawn after its mark, sanitized.
fn line_text(line: &vr_git::DiffLine) -> String {
    let mut t = sanitize(&line.text_lossy());
    if line.no_newline {
        t.push_str("  (no newline at end)");
    }
    t
}

/// `s` without its first `cells` columns (a wide character cut in half goes too).
pub(crate) fn skip_cells(s: &str, cells: usize) -> &str {
    let mut used = 0;
    for (i, ch) in s.char_indices() {
        if used >= cells {
            return &s[i..];
        }
        used += width(ch.encode_utf8(&mut [0; 4]));
    }
    ""
}

/// `s` cut to `cells` columns, ending with "…" when cut.
/// Walks `s` only until it is known not to fit, so a long line costs what is shown.
pub(crate) fn fit_head(s: &str, cells: usize) -> String {
    let mut used = 0;
    // End of the head that fits with the "…" after it.
    let mut keep = 0;
    for (i, ch) in s.char_indices() {
        let cw = width(ch.encode_utf8(&mut [0; 4]));
        if used + cw < cells {
            keep = i + ch.len_utf8();
        }
        used += cw;
        if used > cells {
            let mut out = s[..keep].to_string();
            if cells > 0 {
                out.push('…');
            }
            return out;
        }
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_fit_keeps_the_end() {
        assert_eq!(fit_tail("src/deep/file.rs", 9), "…/file.rs");
        assert_eq!(fit_tail("a.rs", 9), "a.rs");
        assert_eq!(fit_tail("abc", 0), "");
    }

    #[test]
    fn banners_wrap_between_names_and_end_with_an_ellipsis() {
        assert_eq!(
            wrap("! runs: filter.x.clean, hook pre-push", 30, 2),
            vec!["! runs: filter.x.clean,", "  hook pre-push"]
        );
        assert_eq!(wrap("short", 30, 2), vec!["short"]);
        let one = wrap("! runs: filter.x.clean, hook pre-push", 20, 1);
        assert_eq!(one.len(), 1);
        assert!(one[0].ends_with('…') && width(&one[0]) <= 20, "{one:?}");
        let two = wrap("! a b c d e f g h i j k l m n o p", 8, 2);
        assert_eq!(two.len(), 2);
        assert!(two[1].ends_with('…'), "{two:?}");
        assert!(wrap("abc", 0, 2).len() <= 1);
        // A list item ending exactly at the row edge: the row keeps it, the next row starts
        // with the next item (M7-4b review: `hook pre-push` was cut at 40x12).
        assert_eq!(
            wrap("abc: one, two, three four", 14, 2),
            vec!["abc: one, two,", "  three four"]
        );
    }

    #[test]
    fn small_screens_use_everything_but_the_status_row() {
        assert_eq!(popup_rect(Rect::new(0, 0, 40, 12)), Rect::new(0, 0, 40, 11));
        let r = popup_rect(Rect::new(0, 0, 120, 40));
        assert_eq!((r.width, r.height), (108, 35));
    }
}
