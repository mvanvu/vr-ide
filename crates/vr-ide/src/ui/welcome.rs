//! The welcome screen (no file open): project facts, the keys to start with and a live
//! resource monitor. Wide terminals get a two-column dashboard, narrow ones a short single
//! column; what does not fit is dropped, never wrapped.
//!
//! Drawing only reads what the app already holds (`App::monitor`, the file index, the recent
//! list); nothing here touches the file system or `/proc`.

use std::path::Path;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;
use vr_syntax::Capture;

use crate::app::App;
use crate::monitor::{HISTORY, MemInfo, Monitor, Ring};
use crate::theme::Theme;

const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
/// Rows of the CPU history graph in the full layout.
const GRAPH_ROWS: u16 = 6;
const WARN_AT: f32 = 60.0;
const CRIT_AT: f32 = 85.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// One column: name, root, CPU and RAM lines, a few keys.
    Compact,
    /// Two columns, one-row CPU history, per-core glyph row.
    Medium,
    /// Two columns with the history graph and per-core bars.
    Full,
}

fn mode_for(area: Rect) -> Mode {
    if area.width < 64 || area.height < 16 {
        Mode::Compact
    } else if area.width >= 110 && area.height >= 26 {
        Mode::Full
    } else {
        Mode::Medium
    }
}

struct Palette {
    name: Style,
    text: Style,
    dim: Style,
    head: Style,
    own_head: Style,
    sep: Style,
    line: Style,
    ok: Style,
    warn: Style,
    crit: Style,
}

fn fg_only(s: Style) -> Style {
    Style {
        fg: s.fg,
        ..Style::default()
    }
}

impl Palette {
    fn new(t: &Theme) -> Palette {
        let bold = Modifier::BOLD;
        Palette {
            name: fg_only(t.tab_active).add_modifier(bold),
            text: fg_only(t.text),
            dim: fg_only(t.dim),
            head: fg_only(t.info).add_modifier(bold),
            own_head: fg_only(t.syntax(Capture::VariableBuiltin)).add_modifier(bold),
            sep: fg_only(t.gutter),
            line: Style {
                fg: t.selection.bg,
                ..Style::default()
            },
            ok: fg_only(t.ok),
            warn: fg_only(t.warning),
            crit: fg_only(t.error),
        }
    }

    /// Colour of a meter value: green below 60 %, orange to 84 %, red from 85 %.
    fn level(&self, pct: f32) -> Style {
        if pct >= CRIT_AT {
            self.crit
        } else if pct >= WARN_AT {
            self.warn
        } else {
            self.ok
        }
    }

    /// Colour of a history glyph: the normal tone, then the two warning levels.
    fn trace(&self, pct: f32) -> Style {
        if pct >= CRIT_AT {
            self.crit
        } else if pct >= WARN_AT {
            self.warn
        } else {
            self.head
        }
    }
}

/// Draws inside one column; everything is clipped to its right edge and the bottom.
struct Pen<'a> {
    buf: &'a mut Buffer,
    left: u16,
    width: u16,
    top: u16,
    bottom: u16,
}

impl Pen<'_> {
    /// Writes `s` at column `x` (relative to the column) of row `row`; returns the next x.
    fn put(&mut self, x: u16, row: u16, s: &str, style: Style) -> u16 {
        let y = self.top + row;
        if y >= self.bottom || x >= self.width {
            return x;
        }
        let (nx, _) = self
            .buf
            .set_stringn(self.left + x, y, s, usize::from(self.width - x), style);
        nx - self.left
    }

    /// Writes `s` ending at the right edge of the column.
    fn put_right(&mut self, row: u16, s: &str, style: Style) {
        let w = text_width(s);
        self.put(self.width.saturating_sub(w), row, s, style);
    }

    fn rule(&mut self, row: u16, label: &str, label_style: Style, line: Style) {
        let x = self.put(0, row, label, label_style) + 1;
        let fill = "─".repeat(usize::from(self.width.saturating_sub(x)));
        self.put(x, row, &fill, line);
    }

    fn rows(&self) -> u16 {
        self.bottom.saturating_sub(self.top)
    }
}

fn text_width(s: &str) -> u16 {
    Span::raw(s).width().min(usize::from(u16::MAX)) as u16
}

/// `s` cut from the left with a leading `…` when it is wider than `w`.
fn clip_left(s: &str, w: u16) -> String {
    if text_width(s) <= w {
        return s.to_string();
    }
    let mut kept: Vec<char> = Vec::new();
    let mut used = 1;
    for c in s.chars().rev() {
        let cw = text_width(c.encode_utf8(&mut [0; 4]));
        if used + cw > w {
            break;
        }
        used += cw;
        kept.push(c);
    }
    kept.push('…');
    kept.iter().rev().collect()
}

fn pct_cell(p: Option<f32>) -> String {
    match p {
        Some(v) => format!("{:>3}%", v.round() as u32),
        None => format!("{:>4}", "—"),
    }
}

fn gib(kib: u64) -> f64 {
    kib as f64 / (1024.0 * 1024.0)
}

fn mem_text(used: Option<u64>, total: Option<u64>) -> String {
    match (used, total) {
        (Some(u), Some(t)) => format!("{:.1}/{:.1} GiB", gib(u), gib(t)),
        _ => "—".to_string(),
    }
}

fn mem_pct(used: Option<u64>, total: Option<u64>) -> Option<f32> {
    let t = total.filter(|&t| t > 0)?;
    Some(used? as f32 * 100.0 / t as f32)
}

fn rss_text(kib: Option<u64>) -> String {
    match kib {
        Some(k) if k >= 1024 * 1024 => format!("{:.1} GiB", gib(k)),
        Some(k) => format!("{} MiB", k / 1024),
        None => "—".to_string(),
    }
}

fn uptime_text(secs: Option<u64>) -> String {
    let Some(s) = secs else {
        return "—".to_string();
    };
    let (d, h, m) = (s / 86_400, s / 3600 % 24, s / 60 % 60);
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m")
    } else {
        format!("{s}s")
    }
}

fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn block_for(pct: f32) -> char {
    BLOCKS[((pct.clamp(0.0, 100.0) / 100.0 * 7.0).round() as usize).min(7)]
}

/// One glyph per sample, newest at the right edge; older samples fall off the left.
fn spark(pen: &mut Pen, x: u16, row: u16, w: u16, ring: &Ring, scale: f32, p: &Palette) {
    let n = ring.len().min(usize::from(w));
    let skip = ring.len() - n;
    let start = w - n as u16;
    for (i, v) in ring.iter().skip(skip).enumerate() {
        pen.put(
            x + start + i as u16,
            row,
            block_for(v * 100.0 / scale).encode_utf8(&mut [0; 4]),
            p.trace(v),
        );
    }
}

/// A true percentage: filled cells in the level colour, the rest as a track.
fn bar(pen: &mut Pen, x: u16, row: u16, w: u16, pct: Option<f32>, p: &Palette) {
    let filled = pct.map_or(0, |v| {
        ((v.clamp(0.0, 100.0) / 100.0 * f32::from(w)).round() as u16).min(w)
    });
    let style = pct.map_or(p.dim, |v| p.level(v));
    pen.put(x, row, &"█".repeat(usize::from(filled)), style);
    pen.put(
        x + filled,
        row,
        &"░".repeat(usize::from(w - filled)),
        p.line,
    );
}

fn pct_value(pen: &mut Pen, x: u16, row: u16, v: Option<f32>, p: &Palette) {
    let style = v.map_or(p.dim, |v| p.level(v));
    pen.put(x, row, &pct_cell(v), style);
}

/// `LABEL spark  pct` over the column width.
fn cpu_trace_row(pen: &mut Pen, row: u16, m: &Monitor, p: &Palette) {
    pen.put(0, row, "CPU ", p.dim);
    let w = pen.width.saturating_sub(4 + 1 + 4);
    spark(pen, 4, row, w, &m.cpu_history, 100.0, p);
    pct_value(pen, pen.width.saturating_sub(4), row, m.reading.cpu, p);
}

/// `LABEL bar used/total  pct`; `mem_w` is the width of the figures column.
fn mem_row(
    pen: &mut Pen,
    row: u16,
    label: &str,
    used: Option<u64>,
    total: Option<u64>,
    mem_w: u16,
    p: &Palette,
) {
    let pct = mem_pct(used, total);
    pen.put(0, row, label, p.dim);
    let bar_w = pen.width.saturating_sub(4 + 1 + mem_w + 1 + 4);
    bar(pen, 4, row, bar_w, pct, p);
    let text = mem_text(used, total);
    let tx = 4 + bar_w + 1 + mem_w.saturating_sub(text_width(&text));
    pen.put(tx, row, &text, p.text);
    pct_value(pen, pen.width.saturating_sub(4), row, pct, p);
}

fn mem_widths(mem: &MemInfo) -> u16 {
    text_width(&mem_text(mem.used(), mem.total))
        .max(text_width(&mem_text(mem.swap_used(), mem.swap_total)))
}

struct Hint {
    key: String,
    title: &'static str,
    /// Lower is kept first when rows run out.
    priority: u8,
}

/// The keys that start things, from the keymap (USER-5); a command without a key is left out.
fn hints(app: &App) -> Vec<Hint> {
    const ALL: [(&str, &str, u8); 6] = [
        ("file.goto", "Go to file", 0),
        ("app.palette", "Command palette", 1),
        ("explorer.toggle_focus", "Explorer", 2),
        ("search.project", "Search", 4),
        ("terminal.toggle", "Terminal", 5),
        ("app.quit", "Quit", 3),
    ];
    ALL.iter()
        .filter_map(|&(cmd, title, priority)| {
            // The display key (D250); without the kitty protocol a key the terminal cannot send
            // is not recommended, so a command with only such keys is left out (D223).
            let key = app.display_key(cmd)?;
            if !app.keyboard_enhanced
                && key
                    .parse::<vr_config::KeyChord>()
                    .is_ok_and(|k| crate::event_loop::legacy_unreachable(&k))
            {
                return None;
            }
            Some(Hint {
                key,
                title,
                priority,
            })
        })
        .collect()
}

/// The first `n` hints by priority, in display order.
fn keep(hints: Vec<Hint>, n: usize) -> Vec<Hint> {
    if n >= hints.len() {
        return hints;
    }
    let mut by_priority: Vec<u8> = hints.iter().map(|h| h.priority).collect();
    by_priority.sort_unstable();
    let limit = by_priority.get(n.saturating_sub(1)).copied();
    hints
        .into_iter()
        .filter(|h| n > 0 && limit.is_some_and(|l| h.priority <= l))
        .collect()
}

fn key_rows(pen: &mut Pen, row: u16, hints: &[Hint], p: &Palette) {
    let kw = hints.iter().map(|h| text_width(&h.key)).max().unwrap_or(0);
    for (i, h) in hints.iter().enumerate() {
        let r = row + i as u16;
        pen.put(0, r, &h.key, p.head);
        pen.put(kw + 2, r, h.title, p.text);
    }
}

fn name_line(pen: &mut Pen, row: u16, p: &Palette) {
    let x = pen.put(0, row, "vr-ide", p.name);
    pen.put(x, row, concat!(" v", env!("CARGO_PKG_VERSION")), p.dim);
}

fn root_line(pen: &mut Pen, row: u16, root: &Path, p: &Palette) {
    let s = clip_left(&root.display().to_string(), pen.width);
    pen.put(0, row, &s, p.dim);
}

/// `branch · N files` (either part may be missing).
fn project_line(pen: &mut Pen, row: u16, app: &App, p: &Palette) {
    let files = match &app.file_index {
        Some(i) => Some(format!(
            "{}{} files",
            thousands(i.len()),
            if i.truncated() { "+" } else { "" }
        )),
        None if app.index_rx.is_some() => Some("indexing…".to_string()),
        None => None,
    };
    let mut x = 0;
    if let Some(b) = &app.monitor.branch {
        x = pen.put(x, row, b, p.ok);
        if files.is_some() {
            x = pen.put(x, row, " · ", p.sep);
        }
    }
    if let Some(f) = files {
        pen.put(x, row, &f, p.dim);
    }
}

fn recent_rows(pen: &mut Pen, row: u16, app: &App, n: usize, p: &Palette) {
    for (i, path) in app.recent.iter().take(n).enumerate() {
        let rel = path.strip_prefix(app.root()).unwrap_or(path);
        let shown = clip_left(&rel.display().to_string(), pen.width);
        let cut = shown.rfind('/').map_or(0, |i| i + 1);
        let r = row + i as u16;
        pen.put(0, r, &shown[..cut], p.dim);
        pen.put(text_width(&shown[..cut]), r, &shown[cut..], p.text);
    }
}

pub fn render(buf: &mut Buffer, area: Rect, app: &App, theme: &Theme) {
    let p = Palette::new(theme);
    let mode = mode_for(area);
    if mode == Mode::Compact {
        compact(buf, area, app, &p);
    } else {
        columns(buf, area, app, &p, mode);
    }
}

fn compact(buf: &mut Buffer, area: Rect, app: &App, p: &Palette) {
    let pad = if area.width < 50 { 1 } else { 2 };
    let mut pen = Pen {
        buf,
        left: area.x + pad,
        width: area.width.saturating_sub(2 * pad),
        top: area.y,
        bottom: area.y + area.height,
    };
    let m = &app.monitor;
    name_line(&mut pen, 0, p);
    if let Some(b) = &m.branch {
        pen.put(
            text_width(concat!("vr-ide v", env!("CARGO_PKG_VERSION"))) + 2,
            0,
            b,
            p.ok,
        );
    }
    // The root is in the status bar; its row goes to the hints.
    let mut row = 2;
    if m.supported() {
        cpu_trace_row(&mut pen, row, m, p);
        let (used, total) = (m.reading.mem.used(), m.reading.mem.total);
        mem_row(
            &mut pen,
            row + 1,
            "RAM ",
            used,
            total,
            text_width(&mem_text(used, total)),
            p,
        );
        row += 3;
    } else {
        pen.put(0, row, "monitor: Linux only", p.dim);
        row += 2;
    }
    let free = usize::from(pen.rows().saturating_sub(row));
    key_rows(&mut pen, row, &keep(hints(app), free), p);
}

fn columns(buf: &mut Buffer, area: Rect, app: &App, p: &Palette, mode: Mode) {
    let divider = if area.width >= 110 {
        58
    } else {
        area.width / 2
    };
    let bottom = area.y + area.height;
    for y in area.y..bottom {
        buf.set_stringn(area.x + divider, y, "│", 1, p.line);
    }
    let mut left = Pen {
        buf,
        left: area.x + 2,
        width: divider.saturating_sub(4),
        top: area.y + 1,
        bottom,
    };
    left_column(&mut left, app, p, mode);
    let rx = divider + 2;
    let width = area.width.saturating_sub(rx + 2).min(64);
    let mut right = Pen {
        buf,
        left: area.x + rx,
        width,
        top: area.y + 1,
        bottom,
    };
    right_column(&mut right, app, p, mode);
}

fn left_column(pen: &mut Pen, app: &App, p: &Palette, mode: Mode) {
    name_line(pen, 0, p);
    root_line(pen, 1, app.root(), p);
    project_line(pen, 2, app, p);
    let keys = hints(app);
    let n = keys.len() as u16;
    pen.rule(4, "START", p.head, p.line);
    key_rows(pen, 5, &keys, p);
    let row = 5 + n + 1;
    if !app.recent.is_empty() {
        pen.rule(row, "RECENT", p.head, p.line);
        let cap = if mode == Mode::Full { 5 } else { 3 };
        let room = usize::from(pen.rows().saturating_sub(row + 1));
        recent_rows(pen, row + 1, app, cap.min(room), p);
    }
}

fn right_column(pen: &mut Pen, app: &App, p: &Palette, mode: Mode) {
    pen.rule(0, "SYSTEM", p.head, p.line);
    let m = &app.monitor;
    if !m.supported() {
        pen.put(0, 1, "monitor: Linux only", p.dim);
        return;
    }
    let r = &m.reading;
    let mem_w = mem_widths(&r.mem);
    let mut row = 1;
    if mode == Mode::Full {
        pen.put(0, row, "CPU ", p.dim);
        let now = r
            .cpu
            .map_or("—".to_string(), |v| format!("{}%", v.round() as u32));
        pen.put(4, row, &now, r.cpu.map_or(p.dim, |v| p.level(v)));
        pen.put_right(
            row,
            &format!("{} s", HISTORY.min(usize::from(pen.width))),
            p.dim,
        );
        row += 1;
        graph(pen, row, m, p);
        row += GRAPH_ROWS + 1;
        row += core_bars(pen, row, &r.cores, p);
    } else {
        cpu_trace_row(pen, row, m, p);
        row += 1;
        core_glyphs(pen, row, &r.cores, p);
        row += 1;
    }
    mem_row(pen, row, "RAM ", r.mem.used(), r.mem.total, mem_w, p);
    mem_row(
        pen,
        row + 1,
        "SWP ",
        r.mem.swap_used(),
        r.mem.swap_total,
        mem_w,
        p,
    );
    row += 2;
    let load = match r.load {
        Some([a, b, c]) => format!("{a:.2} {b:.2} {c:.2}"),
        None => "—".to_string(),
    };
    let up = uptime_text(r.uptime);
    if mode == Mode::Full {
        row += 1;
        pen.put(0, row, "LOAD ", p.dim);
        pen.put(5, row, &load, p.text);
        pen.put_right(row, &up, p.text);
        pen.put(
            pen.width.saturating_sub(text_width(&up) + 3),
            row,
            "UP",
            p.dim,
        );
        row += 2;
    } else {
        pen.put(0, row, "LOAD", p.dim);
        pen.put_right(row, &load, p.text);
        pen.put(0, row + 1, "UP", p.dim);
        pen.put_right(row + 1, &up, p.text);
        row += 3;
    }
    pen.rule(row, "VR-IDE", p.own_head, p.line);
    row += 1;
    let own = r.own_cpu.map_or("—".to_string(), |v| format!("{v:.1}%"));
    let own_style = r.own_cpu.map_or(p.dim, |v| p.level(v));
    if mode == Mode::Full {
        pen.put(0, row, "CPU ", p.dim);
        let w = pen.width.saturating_sub(4 + 1 + 5);
        // Own load is a few percent: scaled to its recent peak (at least 5 %) so the trace shows.
        let peak = m.own_history.iter().fold(5.0_f32, f32::max);
        spark(pen, 4, row, w, &m.own_history, peak, p);
        pen.put_right(row, &format!("{own:>5}"), own_style);
    } else {
        pen.put(0, row, "CPU", p.dim);
        pen.put_right(row, &own, own_style);
    }
    pen.put(0, row + 1, "RSS", p.dim);
    let rss = rss_text(r.own_rss);
    if mode == Mode::Full {
        let share =
            mem_pct(r.own_rss, r.mem.total).map_or(String::new(), |v| format!("{v:.1}% of RAM"));
        let sw = text_width(&share);
        pen.put_right(row + 1, &share, p.dim);
        let gap = if sw > 0 { sw + 2 } else { 0 };
        pen.put(
            pen.width.saturating_sub(gap + text_width(&rss)),
            row + 1,
            &rss,
            p.text,
        );
    } else {
        pen.put_right(row + 1, &rss, p.text);
    }
}

/// CPU history as stacked blocks, one column per sample, newest at the right.
fn graph(pen: &mut Pen, row: u16, m: &Monitor, p: &Palette) {
    let units_total = f32::from(GRAPH_ROWS) * 8.0;
    let n = m.cpu_history.len().min(usize::from(pen.width));
    let skip = m.cpu_history.len() - n;
    let start = pen.width - n as u16;
    for (i, v) in m.cpu_history.iter().skip(skip).enumerate() {
        let mut units = (v.clamp(0.0, 100.0) / 100.0 * units_total).round() as i32;
        if v > 0.0 {
            units = units.max(1);
        }
        for r in 0..GRAPH_ROWS {
            let cell = (units - i32::from(r) * 8).clamp(0, 8);
            if cell == 0 {
                break;
            }
            let y = row + GRAPH_ROWS - 1 - r;
            pen.put(
                start + i as u16,
                y,
                BLOCKS[cell as usize - 1].encode_utf8(&mut [0; 4]),
                p.trace(v),
            );
        }
    }
}

/// Two columns of `Cn bar  pct`; returns the rows used (a blank row follows).
fn core_bars(pen: &mut Pen, row: u16, cores: &[f32], p: &Palette) -> u16 {
    if cores.is_empty() {
        pen.put(0, row, "CORE —", p.dim);
        return 2;
    }
    let cell = pen.width.saturating_sub(2) / 2;
    // "C0 " or, from ten cores on, "C10 ".
    let label_w = 1 + (cores.len() - 1).to_string().len() as u16 + 1;
    let bar_w = cell.saturating_sub(label_w + 1 + 4);
    let rows = (cores.len() as u16).div_ceil(2).min(6);
    for (i, &v) in cores.iter().take(usize::from(rows) * 2).enumerate() {
        let (r, c) = (row + i as u16 / 2, i as u16 % 2);
        let x = c * (cell + 2);
        pen.put(x, r, &format!("C{i}"), p.dim);
        bar(pen, x + label_w, r, bar_w, Some(v), p);
        pct_value(pen, x + label_w + bar_w + 1, r, Some(v), p);
    }
    let drawn = usize::from(rows) * 2;
    if cores.len() > drawn {
        pen.put(0, row + rows, &format!("+{}", cores.len() - drawn), p.dim);
    }
    rows + 1
}

/// `CORE 0▄ 1▂ …`: one glyph per core, cut at the last whole entry that fits.
fn core_glyphs(pen: &mut Pen, row: u16, cores: &[f32], p: &Palette) {
    let mut x = pen.put(0, row, "CORE", p.dim);
    if cores.is_empty() {
        pen.put(x + 1, row, "—", p.dim);
        return;
    }
    // Room for a trailing " +NN" is kept back whenever some cores will not fit.
    let marker = 4;
    let mut shown = 0;
    for (i, &v) in cores.iter().enumerate() {
        let idx = i.to_string();
        let need = 1 + text_width(&idx) + 1;
        let reserve = if i + 1 < cores.len() { marker } else { 0 };
        if x + need + reserve > pen.width {
            break;
        }
        x = pen.put(x + 1, row, &idx, p.dim);
        x = pen.put(x, row, block_for(v).encode_utf8(&mut [0; 4]), p.trace(v));
        shown += 1;
    }
    if shown < cores.len() {
        pen.put(x + 1, row, &format!("+{}", cores.len() - shown), p.dim);
    }
}
