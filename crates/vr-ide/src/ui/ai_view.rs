//! The AI panel (ui-spec S1, S2): a tab bar row of agent sessions, then the active session's
//! grid on the terminal panel's colours (the same renderer) or the session picker; its
//! separator; the status bar while it has the keys.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;

use crate::ai::{self, AiPanel, picker};
use crate::app::App;
use crate::theme::Theme;

/// The '+' at the right end of the bar (opens the picker).
pub const PLUS: &str = " + ";
pub const PLUS_WIDTH: u16 = 3;
/// The active tab's close button and its space.
const CLOSE: &str = "x ";
/// The attention mark (D9) after a label.
const MARK: &str = "◆ ";
/// Picker rows start on this line of the grid area (a blank line, the title, a blank line).
pub const PICKER_ROWS_AT: usize = 3;

fn width_of(s: &str) -> usize {
    Span::raw(s).width()
}

pub fn render(frame: &mut Frame, area: Rect, app: &mut App, theme: &Theme) {
    if area.height == 0 || area.width == 0 || !app.ai.visible {
        return;
    }
    let below_bar = Rect::new(area.x, area.y + 1, area.width, area.height - 1);
    // L2-A: the open changes list takes the top of the session's area.
    let list_rows = if app.ai.picker_active() {
        0
    } else {
        crate::ui::ai_changes::render_list(frame.buffer_mut(), below_bar, app, theme)
    };
    app.ai.list_rows = list_rows;
    let grid = Rect::new(
        below_bar.x,
        below_bar.y + list_rows,
        below_bar.width,
        below_bar.height - list_rows,
    );
    ai::fit(app, grid);
    ai::snapshot_active(app);
    let focused = ai::has_keys(app) && !ai::review::list_has_keys(app);
    let bar = tab_bar(app, area.width);
    let buf = frame.buffer_mut();
    render_tab_bar(buf, Rect { height: 1, ..area }, &app.ai, &bar, theme);
    if grid.height == 0 {
        return;
    }
    if app.ai.picker_active() {
        render_picker(buf, grid, app, theme);
        return;
    }
    let Some(s) = app.ai.active_session() else {
        return;
    };
    let reserved = reserved_rows(app, s, grid);
    let cells = Rect {
        height: grid.height - reserved,
        ..grid
    };
    let cursor =
        crate::ui::terminal_view::render_cells(buf, cells, Some(&s.tab), app.ai.selection, theme);
    let exited = exited_lines(app, s, grid.width);
    if !exited.is_empty() {
        let below = if reserved > 0 {
            Rect::new(grid.x, cells.y + cells.height, grid.width, reserved)
        } else {
            cells
        };
        render_exited(buf, below, &exited, theme);
        return;
    }
    if let Some(pos) = cursor
        && focused
    {
        frame.set_cursor_position(pos);
    }
}

/// The column left of the panel, dim, in the focus colour while the panel has the keys. With
/// the panel over the whole area the explorer's edge shows the focus instead.
pub fn render_separator(buf: &mut Buffer, app: &App, theme: &Theme) {
    let l = app.layout;
    let focus = ai::has_keys(app);
    if l.ai_sep.width > 0 {
        let style = if focus { theme.focus } else { theme.dim };
        for y in l.ai_sep.y..l.ai_sep.y + l.ai_sep.height {
            buf.set_string(l.ai_sep.x, y, "│", style);
        }
    } else if focus && l.ai.width > 0 && l.explorer.width > 0 {
        let x = l.explorer.x + l.explorer.width - 1;
        for y in l.explorer.y..l.explorer.y + l.explorer.height {
            buf.set_style(Rect::new(x, y, 1, 1), theme.focus);
        }
    }
}

/// `[exited 0] enter: start again (claude --continue)`, wrapped to `width`; empty while the
/// session's program runs.
pub(crate) fn exited_lines(app: &App, s: &ai::AiSession, width: u16) -> Vec<String> {
    let Some(code) = ai::exit_code(s) else {
        return Vec::new();
    };
    let again = app
        .settings
        .ai_tools
        .iter()
        .find(|t| t.name == s.tool)
        .map(|t| {
            format!(
                " ({})",
                ai::spawn::command_line(t, ai::spawn::again_mode(t))
            )
        })
        .unwrap_or_default();
    wrap(
        &format!("[exited {code}] enter: start again{again}"),
        usize::from(width),
    )
}

/// Rows kept below an exited session's grid for its exited line (D15), so the line covers
/// nothing the program printed; the program keeps at least one row.
pub(crate) fn reserved_rows(app: &App, s: &ai::AiSession, grid: Rect) -> u16 {
    let n = exited_lines(app, s, grid.width).len();
    u16::try_from(n)
        .unwrap_or(u16::MAX)
        .min(grid.height.saturating_sub(1))
}

/// The exited line in `area`, the rows reserved for it (the grid's last row when there are
/// none).
fn render_exited(buf: &mut Buffer, area: Rect, lines: &[String], theme: &Theme) {
    let n = lines.len().min(usize::from(area.height.max(1)));
    let top = (area.y + area.height).saturating_sub(n as u16);
    for (i, line) in lines.iter().take(n).enumerate() {
        let y = top + i as u16;
        buf.set_style(Rect::new(area.x, y, area.width, 1), theme.popup);
        buf.set_stringn(area.x, y, line, usize::from(area.width), theme.popup);
    }
}

/// Greedy wrap at spaces; a word longer than a row is cut.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = vec![String::new()];
    for word in text.split(' ') {
        let last = lines.last_mut().expect("one line");
        let need = width_of(last) + usize::from(!last.is_empty()) + width_of(word);
        if need > width && !last.is_empty() {
            lines.push(word.to_string());
        } else {
            if !last.is_empty() {
                last.push(' ');
            }
            last.push_str(word);
        }
    }
    lines
}

// ---- tab bar ----------------------------------------------------------------------------------

/// One drawn tab: which one (a session, or the picker at `sessions.len()`), its text, where it
/// sits (offset from the bar's left edge), the offset of its 'x' and of its folder.
pub struct BarTab {
    pub index: usize,
    pub label: String,
    pub x: u16,
    pub width: u16,
    pub close: Option<u16>,
    folder: Option<(u16, u16)>,
    mark: Option<u16>,
}

/// The tab bar laid out for a width; rendering and the mouse hit test use it.
pub struct TabBar {
    pub tabs: Vec<BarTab>,
    /// `+N` (`+N◆`) for tabs left out, and its offset.
    pub more: Option<(String, u16)>,
    /// Offset of the '+' (absent while the picker is open).
    pub plus: Option<u16>,
    /// `±N`, the active session's change count, and its offset (a click opens the list).
    pub changes: Option<(String, u16)>,
}

/// A tab's text, its folder (offset, width), its mark's offset and its 'x''s offset.
type TabText = (String, Option<(u16, u16)>, Option<u16>, Option<u16>);

struct Item {
    index: usize,
    num: usize,
    full: String,
    short: String,
    folder: Option<String>,
    mark: bool,
    active: bool,
}

/// The text of one tab at fit level `level` (ui-spec S1): 0 full; 1 inactive tabs drop the
/// folder; 2 the active tab drops its 'x' and folder, inactive tabs show what tells them apart;
/// 3 inactive tabs show their number only.
fn tab_text(it: &Item, level: u8) -> TabText {
    let label = match (it.active, level) {
        (false, 3) => String::new(),
        (false, 2) => it.short.clone(),
        _ => it.full.clone(),
    };
    let folder = it
        .folder
        .as_ref()
        .filter(|_| level == 0 || (it.active && level < 2));
    let mut s = format!(" {} ", it.num);
    s.push_str(&label);
    let mut folder_at = None;
    if let Some(f) = folder {
        s.push(' ');
        folder_at = Some((width_of(&s) as u16, width_of(f) as u16));
        s.push_str(f);
    }
    if !label.is_empty() {
        s.push(' ');
    }
    let mut mark_at = None;
    if it.mark {
        mark_at = Some(width_of(&s) as u16);
        s.push_str(MARK);
    }
    let mut close = None;
    if it.active && level < 2 {
        close = Some(width_of(&s) as u16);
        s.push_str(CLOSE);
    }
    (s, folder_at, mark_at, close)
}

pub fn tab_bar(app: &App, width: u16) -> TabBar {
    let p = &app.ai;
    let width = usize::from(width);
    // The empty state: one tab, `AI`.
    if p.sessions.is_empty() {
        let label = " AI ".to_string();
        let w = width_of(&label) as u16;
        return TabBar {
            tabs: vec![BarTab {
                index: 0,
                label,
                x: 0,
                width: w,
                close: None,
                folder: None,
                mark: None,
            }],
            more: None,
            plus: None,
            changes: None,
        };
    }
    let mut items: Vec<Item> = (0..p.sessions.len())
        .map(|i| Item {
            index: i,
            num: i + 1,
            full: p.label(i),
            short: p.short_label(i),
            folder: p.folder(i),
            mark: p.sessions[i].attention.is_some(),
            active: i == p.active && !p.picker_active(),
        })
        .collect();
    if p.picker.is_some() {
        let n = p.sessions.len();
        items.push(Item {
            index: n,
            num: n + 1,
            full: "new".into(),
            short: "new".into(),
            folder: None,
            mark: false,
            active: p.picker_active(),
        });
    }
    let plus_w = if p.picker.is_none() {
        usize::from(PLUS_WIDTH)
    } else {
        0
    };
    let avail = width.saturating_sub(plus_w);
    let texts = |level: u8, keep: &[bool]| {
        items
            .iter()
            .zip(keep)
            .filter(|(_, k)| **k)
            .map(|(it, _)| (it.index, tab_text(it, level)))
            .collect::<Vec<_>>()
    };
    let total = |t: &[(usize, TabText)]| t.iter().map(|(_, (s, ..))| width_of(s)).sum::<usize>();
    let all = vec![true; items.len()];
    // The change count keeps its place only while the labels are whole (fit steps 0 and 1).
    let badge = changes_badge(app);
    let badge_w = badge.as_deref().map_or(0, |b| width_of(b) + 1);
    let mut show_badge = false;
    let mut chosen = None;
    for level in 0..=3u8 {
        let t = texts(level, &all);
        let room = if level < 2 {
            avail.saturating_sub(badge_w)
        } else {
            avail
        };
        if total(&t) <= room {
            show_badge = level < 2;
            chosen = Some((t, None));
            break;
        }
    }
    // Level 4: the last inactive, unmarked tabs go into `+N`; the active and marked tabs stay.
    let (tabs, more) = chosen.unwrap_or_else(|| {
        let mut keep = all.clone();
        let mut hidden = 0usize;
        loop {
            let t = texts(3, &keep);
            let more = (hidden > 0).then(|| format!("+{hidden} "));
            let w = total(&t) + more.as_deref().map_or(0, width_of);
            let next = items
                .iter()
                .zip(&keep)
                .rposition(|(it, k)| *k && !it.active && !it.mark);
            match next {
                Some(j) if w > avail => {
                    keep[j] = false;
                    hidden += 1;
                }
                _ => return (t, more),
            }
        }
    });
    let mut x = 0u16;
    let tabs: Vec<BarTab> = tabs
        .into_iter()
        .map(|(index, (label, folder, mark, close))| {
            let w = width_of(&label) as u16;
            let close = close.filter(|c| usize::from(x + c) < width);
            let t = BarTab {
                index,
                label,
                x,
                width: w,
                close,
                folder,
                mark,
            };
            x += w;
            t
        })
        .collect();
    let more = more.map(|m| (m, x));
    let plus = (plus_w > 0 && width >= usize::from(x) + plus_w)
        .then(|| (width - plus_w) as u16)
        .filter(|&p| p >= x + more.as_ref().map_or(0, |(m, _)| width_of(m) as u16));
    let changes = badge.filter(|_| show_badge).map(|b| {
        let end = plus.map_or(width.saturating_sub(1), usize::from);
        let at = end.saturating_sub(width_of(&b)) as u16;
        (b, at)
    });
    TabBar {
        tabs,
        more,
        plus,
        changes,
    }
}

/// `±N` for the tab bar: the active session's changes, while their list is closed.
fn changes_badge(app: &App) -> Option<String> {
    crate::ui::ai_changes::badge(app).filter(|_| app.ai.list.is_none())
}

fn render_tab_bar(buf: &mut Buffer, row: Rect, p: &AiPanel, bar: &TabBar, theme: &Theme) {
    buf.set_style(row, theme.tab_bar);
    let right = row.x + row.width;
    let active = if p.sessions.is_empty() { 0 } else { p.active };
    for t in &bar.tabs {
        let x = row.x + t.x;
        if x >= right {
            break;
        }
        let mut style = if t.index == active {
            theme.tab_active
        } else {
            theme.tab_inactive
        };
        if p.sessions
            .get(t.index)
            .is_some_and(|s| ai::exit_code(s).is_some())
        {
            style = style.patch(theme.tab_warning);
        }
        buf.set_stringn(x, row.y, &t.label, usize::from(right - x), style);
        let paint = |buf: &mut Buffer, at: u16, w: u16, s: Style| {
            let from = x + at;
            if from < right {
                buf.set_style(Rect::new(from, row.y, w.min(right - from), 1), s);
            }
        };
        if let (Some((at, w)), Some(fg)) = (t.folder, theme.note.fg) {
            paint(buf, at, w, Style::default().fg(fg));
        }
        if let Some(at) = t.mark {
            paint(buf, at, 1, theme.attention);
        }
        if let (Some(at), Some(fg)) = (t.close, theme.dim.fg) {
            paint(buf, at, 1, Style::default().fg(fg));
        }
    }
    if let Some((more, at)) = &bar.more {
        let x = row.x + at;
        if x < right {
            buf.set_stringn(x, row.y, more, usize::from(right - x), theme.dim);
        }
    }
    if let Some((text, at)) = &bar.changes {
        let x = row.x + at;
        if x < right {
            let style = theme.tab_bar.patch(theme.git_modified);
            buf.set_stringn(x, row.y, text, usize::from(right - x), style);
        }
    }
    if let Some(at) = bar.plus {
        let x = row.x + at;
        if x < right {
            buf.set_stringn(x, row.y, PLUS, usize::from(right - x), theme.tab_inactive);
        }
    }
}

// ---- picker -----------------------------------------------------------------------------------

fn render_picker(buf: &mut Buffer, area: Rect, app: &App, theme: &Theme) {
    let Some(pk) = &app.ai.picker else {
        return;
    };
    buf.set_style(area, theme.text);
    let w = usize::from(area.width);
    let bottom = area.y + area.height;
    let put = |buf: &mut Buffer, y: u16, x: u16, text: &str, style: Style| {
        if y < bottom && x < area.x + area.width {
            let room = usize::from(area.x + area.width - x);
            buf.set_stringn(x, y, text, room, style);
        }
    };
    let empty = app.ai.sessions.is_empty();
    let (lead, folder) = if empty && pk.back.is_none() {
        let name = pk.folder.file_name().map_or_else(
            || pk.folder.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        ("Start in ", crate::ui::safe_text::sanitize(&name))
    } else {
        ("New session in ", crate::git::home_relative(&pk.folder))
    };
    put(buf, area.y + 1, area.x + 1, lead, theme.text);
    put(
        buf,
        area.y + 1,
        area.x + 1 + width_of(lead) as u16,
        &folder,
        theme.info,
    );
    // Tool names in the active tab's accent (pink in synthwave), bold.
    let mut header = Style::default().add_modifier(ratatui::style::Modifier::BOLD);
    if let Some(fg) = theme.tab_active.fg {
        header = header.fg(fg);
    }
    let mut y = area.y + PICKER_ROWS_AT as u16;
    let row_line = |buf: &mut Buffer, y: u16, left: &str, right: &str, style: Style, sel: bool| {
        if y >= bottom {
            return;
        }
        if sel {
            buf.set_style(Rect::new(area.x, y, area.width, 1), theme.list_selected);
        }
        let base = if sel { theme.list_selected } else { theme.text };
        put(buf, y, area.x, left, base.patch(style));
        let rw = width_of(right);
        if rw + width_of(left) < w {
            let rs = if sel { theme.list_selected } else { theme.dim };
            put(buf, y, area.x + (w - rw - 1) as u16, right, rs);
        }
    };
    if let Some(sec) = &pk.second {
        put(buf, y, area.x + 1, &sec.tool, header);
        y += 1;
        let tool = app.settings.ai_tools.iter().find(|t| t.name == sec.tool);
        for (i, mode) in sec.modes.iter().enumerate() {
            let extra = tool
                .map(|t| match mode {
                    ai::spawn::StartMode::New => String::new(),
                    ai::spawn::StartMode::Continue => t.continue_args.join(" "),
                    ai::spawn::StartMode::Resume => t.resume_args.join(" "),
                })
                .unwrap_or_default();
            let left = format!("   {}", mode.title());
            row_line(buf, y, &left, &extra, Style::default(), i == sec.selected);
            y += 1;
        }
    } else {
        for (i, row) in pk.rows.iter().enumerate() {
            match row {
                picker::Row::Header { tool } => put(buf, y, area.x + 1, tool, header),
                picker::Row::Choice {
                    tool,
                    account,
                    open,
                    single,
                } => {
                    let right = if *open > 0 {
                        format!("{open} open")
                    } else {
                        "new".to_string()
                    };
                    let (left, style) = if *single {
                        (format!(" {tool}"), header)
                    } else {
                        (
                            format!("   {}", account.as_deref().unwrap_or("default")),
                            Style::default(),
                        )
                    };
                    row_line(buf, y, &left, &right, style, i == pk.selected);
                }
            }
            y += 1;
        }
        y += u16::from(!pk.rows.is_empty());
        let mut notes = Vec::new();
        if pk.rows.is_empty() {
            notes.push(format!(
                "No AI tool found on PATH (ai_tools: {})",
                pk.configured.join(", ")
            ));
        } else if !pk.missing.is_empty() {
            notes.push(format!("Not on PATH: {}", pk.missing.join(", ")));
            notes.push("(ai_tools in settings)".to_string());
        }
        for note in notes {
            for line in wrap(&note, w.saturating_sub(2)) {
                put(buf, y, area.x + 1, &line, theme.note);
                y += 1;
            }
        }
    }
    // The footer: keys in the warning colour, words dim.
    let fy = bottom - 1;
    let esc = if empty && pk.back.is_none() && pk.second.is_none() {
        "hide"
    } else {
        "back"
    };
    let mut x = area.x + 1;
    for (k, word) in [("enter", "start"), ("esc", esc)] {
        put(buf, fy, x, k, theme.warning);
        x += width_of(k) as u16 + 1;
        put(buf, fy, x, word, theme.dim);
        x += width_of(word) as u16 + 2;
    }
}

// ---- status bar -------------------------------------------------------------------------------

/// Status bar while the panel has the keys: the `AI` badge, the session (or the picker) and its
/// folder, then which tab of how many and the key that hides the panel (or restarts an exited
/// session). The folder gives way first, then the hide word.
pub fn render_status(buf: &mut Buffer, area: Rect, app: &App, theme: &Theme) {
    let width = usize::from(area.width);
    let p = &app.ai;
    let badge = if width >= 50 { " AI " } else { " A " };
    buf.set_stringn(area.x, area.y, badge, width, theme.mode_ai);
    let bw = width_of(badge).min(width);
    let hide = app.key_hint("ai.toggle");
    let (what, folder) = match p.active_session() {
        Some(s) => {
            let state = ai::exit_code(s).map_or_else(String::new, |c| format!(" [exited {c}]"));
            (
                format!("{}{state}", p.label(p.active)),
                crate::git::home_relative(&s.root),
            )
        }
        None => {
            let folder = p
                .picker
                .as_ref()
                .map(|k| crate::git::home_relative(&k.folder))
                .unwrap_or_default();
            let what = if p.sessions.is_empty() {
                "no session"
            } else {
                "new session"
            };
            (what.to_string(), folder)
        }
    };
    let exited = p
        .active_session()
        .is_some_and(|s| ai::exit_code(s).is_some());
    let pos = (!p.sessions.is_empty()).then(|| format!("{}/{}", p.active + 1, p.tab_count()));
    // `±N f6 · ` leads the right part while the session has changes.
    let changes = crate::ui::ai_changes::badge(app)
        .map(|b| format!("{b} {} · ", app.key_hint("ai.changes")))
        .unwrap_or_default();
    let tail = |words: bool| {
        let action = if exited {
            if words { "enter restart" } else { "enter" }.to_string()
        } else if words {
            format!("{hide} hide")
        } else {
            hide.clone()
        };
        match &pos {
            Some(n) if words => format!(" {changes}{n} · {action} "),
            Some(n) => format!(" {n} · {action} "),
            None => format!(" {action} "),
        }
    };
    let room = width - bw;
    let lefts = [format!(" {what} · {folder}"), format!(" {what}")];
    let rights = [tail(true), tail(false)];
    let mut chosen = None;
    'fit: for r in &rights {
        for l in &lefts {
            if width_of(l) + width_of(r) <= room {
                chosen = Some((l.clone(), r.clone()));
                break 'fit;
            }
        }
    }
    let (left, right) = chosen.unwrap_or_else(|| {
        let r = rights[1].clone();
        let l = crate::ui::status_bar::shorten_left(&lefts[1], room.saturating_sub(width_of(&r)));
        (l, r)
    });
    let x = area.x + bw as u16;
    buf.set_stringn(x, area.y, &left, room, theme.status);
    let rw = width_of(&right).min(room);
    buf.set_stringn(
        area.x + (width - rw) as u16,
        area.y,
        &right,
        rw,
        theme.status,
    );
}
