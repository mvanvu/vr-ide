//! The integrated terminal panel (M5): terminal tabs, the panel's visibility and height, the
//! `terminal.*` and `focus.editor` commands, and how keys, paste and the mouse reach a focused
//! terminal (D8). The UI thread never blocks on a PTY: vr-term's reader thread feeds the
//! emulator, writes are queued, and each terminal's `Notify` only sets flags that the event loop
//! polls ([`TerminalPanel::take_wake`]).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::cursor::SetCursorStyle;
use crossterm::event::{
    KeyCode as K, KeyEvent, KeyModifiers, MouseButton as MB, MouseEvent, MouseEventKind as MK,
};
use ratatui::layout::Rect;
use vr_config::{Context, KeyChord, TerminalPosition};
use vr_term::{
    CursorShape, Grid, KeyCode as T, Modifiers, MouseButton, MouseKind, SpawnOptions, TermError,
    TermStatus, Terminal, encode_key, encode_mouse, encode_paste,
};

use super::dialog::DialogAction;
use super::file::CANCEL;
use super::registry::{CmdArgs, CmdResult, CommandSpec};
use crate::app::{App, Focus, WHEEL_LINES};
use crate::event_loop::chord_from_key;
use crate::ui::dialog::{ConfirmDialog, DialogWidget};

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "terminal.toggle",
        title: "Terminal: Toggle Terminal",
        run: toggle,
    },
    CommandSpec {
        name: "terminal.hide",
        title: "Terminal: Hide Terminal",
        run: hide_terminal,
    },
    CommandSpec {
        name: "terminal.next",
        title: "Terminal: Focus Next Terminal",
        run: next,
    },
    CommandSpec {
        name: "terminal.prev",
        title: "Terminal: Focus Previous Terminal",
        run: prev,
    },
    CommandSpec {
        name: "terminal.grow",
        title: "Terminal: Increase Panel Size",
        run: grow,
    },
    CommandSpec {
        name: "terminal.shrink",
        title: "Terminal: Decrease Panel Size",
        run: shrink,
    },
    CommandSpec {
        name: "terminal.toggle_position",
        title: "Terminal: Toggle Panel Position",
        run: toggle_position,
    },
    CommandSpec {
        name: "focus.editor",
        title: "Focus Editor",
        run: focus_editor,
    },
];

/// grow / shrink step and the bounds of the panel height, in percent of the screen.
pub const HEIGHT_STEP: u16 = 5;
pub const HEIGHT_MIN: u16 = 10;
pub const HEIGHT_MAX: u16 = 90;

/// The shell when `$SHELL` is unset or cannot be started.
const FALLBACK_SHELL: &str = "/bin/sh";

pub const QUIT_ANYWAY: &str = "Quit anyway";
const CLOSE: &str = "Close";

/// One terminal tab: the running (or exited) program and what the panel last showed of it.
pub struct TermTab {
    /// `None` only for a tab made by [`TermTab::fake`] (rendering tests).
    term: Option<Terminal>,
    shell: PathBuf,
    /// Lines scrolled back into the history; 0 follows the output.
    pub scroll: usize,
    /// The last snapshot: what the panel draws and what a selection copies.
    pub grid: Grid,
    /// Set by this terminal's `Notify` (new output, title or exit) until the loop looks.
    output: Arc<AtomicBool>,
    fake_status: TermStatus,
    fake_title: String,
}

impl TermTab {
    /// A tab without a process showing `grid` (deterministic rendering tests).
    pub fn fake(grid: Grid, title: &str, status: TermStatus) -> TermTab {
        TermTab {
            term: None,
            shell: PathBuf::from(title),
            scroll: 0,
            grid,
            output: Arc::default(),
            fake_status: status,
            fake_title: title.to_string(),
        }
    }

    pub fn status(&self) -> TermStatus {
        self.term
            .as_ref()
            .map_or(self.fake_status, Terminal::status)
    }

    pub fn running(&self) -> bool {
        self.status() == TermStatus::Running
    }

    pub fn pid(&self) -> Option<u32> {
        self.term.as_ref().and_then(Terminal::pid)
    }

    /// See [`Terminal::shell_in_foreground`]; false for a fake tab.
    pub fn shell_in_foreground(&self) -> bool {
        self.term
            .as_ref()
            .is_some_and(Terminal::shell_in_foreground)
    }

    /// Name of the program holding the terminal's foreground (`/proc/<pgid>/comm`).
    pub fn foreground_name(&self) -> Option<String> {
        let pgid = self.term.as_ref()?.foreground_pgid()?;
        let name = std::fs::read_to_string(format!("/proc/{pgid}/comm")).ok()?;
        let name = name.trim();
        (!name.is_empty()).then(|| name.to_string())
    }

    /// Columns and rows of the PTY (of the grid for a fake tab).
    pub fn size(&self) -> (u16, u16) {
        self.term.as_ref().map_or(
            (self.grid.cols as u16, self.grid.rows as u16),
            Terminal::size,
        )
    }

    /// A fresh snapshot at the tab's scroll position (the stored grid for a fake tab).
    pub fn grid_now(&self) -> Grid {
        match &self.term {
            Some(t) => t.snapshot(),
            None => self.grid.clone(),
        }
    }

    /// Takes a fresh snapshot into [`TermTab::grid`]; this also re-arms the terminal's `Notify`.
    pub fn refresh(&mut self) {
        if let Some(t) = &self.term {
            self.grid = t.snapshot();
            self.scroll = self.grid.scroll_offset;
        }
    }

    /// Moves the view `lines` up into the history (negative: down), clamped. A real
    /// terminal keeps the view on its lines while output arrives (AC-F2).
    pub fn scroll_by(&mut self, lines: isize) {
        self.scroll = match &self.term {
            Some(t) => t.scroll(lines),
            None => self
                .scroll
                .saturating_add_signed(lines)
                .min(self.grid.scrollback_len),
        };
    }

    /// Back to the bottom: the view follows the output again.
    pub fn scroll_to_bottom(&mut self) {
        if let Some(t) = &self.term {
            t.scroll_to_bottom();
        }
        self.scroll = 0;
    }

    /// The screen as text, one line per row (tests and selection).
    pub fn screen_text(&self) -> String {
        let g = self.grid_now();
        (0..g.rows)
            .map(|r| g.row_text(r))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The program's title (OSC 0/2), else the shell's name.
    pub fn title(&self) -> String {
        if self.term.is_none() {
            return self.fake_title.clone();
        }
        match self.grid.title.as_deref().map(str::trim) {
            Some(t) if !t.is_empty() => t.to_string(),
            _ => self
                .shell
                .file_name()
                .map_or_else(|| "shell".to_string(), |n| n.to_string_lossy().into_owned()),
        }
    }

    fn write(&self, bytes: &[u8]) -> Result<(), TermError> {
        match &self.term {
            Some(t) if !bytes.is_empty() => t.write(bytes),
            _ => Ok(()),
        }
    }
}

/// A mouse selection in grid cells of the active terminal (row, col), as dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: (usize, usize),
    pub head: (usize, usize),
}

impl Selection {
    /// Start and end in reading order.
    pub fn ordered(&self) -> ((usize, usize), (usize, usize)) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }

    pub fn contains(&self, row: usize, col: usize) -> bool {
        let (s, e) = self.ordered();
        (row, col) >= s && (row, col) <= e
    }
}

/// State of the terminal panel.
pub struct TerminalPanel {
    pub tabs: Vec<TermTab>,
    pub active: usize,
    pub visible: bool,
    /// Panel height in percent of the screen (setting `terminal_height_percent`).
    pub height_percent: u16,
    /// Where the panel sits this session (setting `terminal_position`, T19).
    pub position: TerminalPosition,
    /// Width on the right in percent of the editor column (`terminal_width_percent`).
    pub width_percent: u16,
    /// Exact panel width in columns set by a drag, so the separator stays under the mouse on
    /// wide columns where one percent is several cells; grow / shrink and a changed setting
    /// clear it.
    pub width_cols: Option<u16>,
    /// Program to run instead of `$SHELL` (tests).
    pub shell: Option<PathBuf>,
    /// Extra environment for new terminals (tests).
    pub env: Vec<(OsString, OsString)>,
    pub scrollback: usize,
    pub selection: Option<Selection>,
    selecting: bool,
    /// The program got a mouse press; drags and the release go to it until that release.
    captured: bool,
    /// The panel's edge is being dragged: its tab bar at the bottom, the separator on the right.
    resizing: bool,
    /// "Quit anyway" was chosen; the next quit does not ask about terminals or a running git
    /// write again (one shot).
    quit_confirmed: bool,
    /// Set by any terminal's `Notify`.
    wake: Arc<AtomicBool>,
}

impl TerminalPanel {
    pub fn new(height_percent: u16, scrollback: usize) -> TerminalPanel {
        TerminalPanel {
            tabs: Vec::new(),
            active: 0,
            visible: false,
            height_percent: height_percent.clamp(HEIGHT_MIN, HEIGHT_MAX),
            position: TerminalPosition::Bottom,
            width_percent: 40,
            width_cols: None,
            shell: None,
            env: Vec::new(),
            scrollback,
            selection: None,
            selecting: false,
            captured: false,
            resizing: false,
            quit_confirmed: false,
            wake: Arc::default(),
        }
    }

    /// A terminal had something new since the last call.
    pub fn take_wake(&self) -> bool {
        self.wake.swap(false, Ordering::AcqRel)
    }

    pub fn active_tab(&self) -> Option<&TermTab> {
        self.tabs.get(self.active)
    }

    fn active_mut(&mut self) -> Option<&mut TermTab> {
        self.tabs.get_mut(self.active)
    }

    /// Terminals whose program still runs.
    pub fn running(&self) -> usize {
        self.tabs.iter().filter(|t| t.running()).count()
    }
}

// ---- commands ------------------------------------------------------------------------------

/// ctrl+`, the one key a focused terminal keeps (D280): hidden, show the panel and give it the
/// keys (a terminal starts when there is none); shown, hide it, whatever has the keys. The
/// terminals keep running while hidden.
fn toggle(app: &mut App, _: CmdArgs) -> CmdResult {
    if app.terminal.visible {
        hide(app);
    } else {
        show(app);
    }
    Ok(())
}

/// Hides the panel (shift+f12); the terminals keep running.
fn hide_terminal(app: &mut App, _: CmdArgs) -> CmdResult {
    hide(app);
    Ok(())
}

/// The tab bar's '+' (D280, D291; no key, no palette entry): a new terminal tab, shown with
/// the keys, its shell started in the active file's root (the first root still on disk when
/// that one is gone).
pub fn new_terminal(app: &mut App) {
    let cwd = new_tab_dir(app);
    if add_tab_in(app, &cwd) {
        reveal(app);
    }
}

/// Where the '+' starts a shell: the active file's root, else the first root whose folder
/// exists; the active root when none does (the start then fails with a notice). One stat per
/// click.
fn new_tab_dir(app: &App) -> PathBuf {
    let active = app.active_root().path.clone();
    if active.is_dir() {
        return active;
    }
    app.workspace
        .roots()
        .iter()
        .map(|r| r.path.clone())
        .find(|p| p.is_dir())
        .unwrap_or(active)
}

/// A tab's 'x' in the tab bar (D280, D296): closes that terminal (its process group is hung
/// up) at once, unless a program other than the shell holds its foreground: then "Close
/// terminal?" asks first, Cancel selected. When the foreground cannot be told on a running
/// terminal it asks too, naming "A program"; an exited terminal closes at once.
pub fn close_tab_asking(app: &mut App, i: usize) {
    let Some(tab) = app.terminal.tabs.get(i) else {
        return;
    };
    if !tab.running() || tab.shell_in_foreground() {
        close_tab(app, i);
        return;
    }
    let program = tab.foreground_name().unwrap_or_else(|| "A program".into());
    let message = format!("{program} is running; closing the terminal ends it.");
    let mut dialog = ConfirmDialog::new("Close terminal?", message, &[CLOSE, CANCEL]);
    dialog.selected = 1;
    let pid = tab.pid();
    app.open_dialog(
        DialogWidget::Confirm(dialog),
        DialogAction::CloseTerminal(pid),
    );
}

/// Close was chosen: the terminal asked about goes (found by pid; already gone = nothing).
pub fn answer_close(app: &mut App, pid: Option<u32>) {
    if let Some(i) = app.terminal.tabs.iter().position(|t| t.pid() == pid) {
        close_tab(app, i);
    }
}

/// Closes terminal `i` without asking: its process group is hung up; the last one hides the
/// panel.
pub fn close_tab(app: &mut App, i: usize) {
    let p = &mut app.terminal;
    if i >= p.tabs.len() {
        return;
    }
    let tab = p.tabs.remove(i);
    drop_later(tab.term);
    p.selection = None;
    p.captured = false;
    if p.tabs.is_empty() {
        p.active = 0;
        hide(app);
    } else if i < p.active || p.active >= p.tabs.len() {
        p.active = p.active.saturating_sub(1).min(p.tabs.len() - 1);
    }
    app.dirty = true;
}

fn next(app: &mut App, _: CmdArgs) -> CmdResult {
    step(app, true);
    Ok(())
}

fn prev(app: &mut App, _: CmdArgs) -> CmdResult {
    step(app, false);
    Ok(())
}

/// grow / shrink: the panel's height at the bottom, its width on the right (T19). The stored
/// share is clamped to what can be drawn, so the next key press changes the panel at once.
fn grow(app: &mut App, _: CmdArgs) -> CmdResult {
    step_size(app, true);
    Ok(())
}

fn shrink(app: &mut App, _: CmdArgs) -> CmdResult {
    step_size(app, false);
    Ok(())
}

fn step_size(app: &mut App, up: bool) {
    use crate::ui::layout::{EDITOR_MIN_ROWS, TERMINAL_MIN_ROWS, right_cols};
    let right = right_now(app);
    let column = app.layout.tabs.width;
    let screen = app.layout.status.y + app.layout.status.height;
    // Drawable range in percent; the whole 10..=90 before the first frame.
    let (lo, hi) = if right && column > 0 {
        (
            share(right_cols(column, HEIGHT_MIN), column),
            share(right_cols(column, HEIGHT_MAX), column),
        )
    } else if !right && app.terminal.visible && screen > 0 && app.layout.terminal.height > 0 {
        let total = app.layout.terminal.height + app.layout.editor.height;
        let max_rows = if total >= TERMINAL_MIN_ROWS + EDITOR_MIN_ROWS {
            total - EDITOR_MIN_ROWS
        } else {
            total
        };
        (
            share(TERMINAL_MIN_ROWS.min(total), screen),
            share(max_rows, screen),
        )
    } else {
        (HEIGHT_MIN, HEIGHT_MAX)
    };
    let lo = lo.clamp(HEIGHT_MIN, HEIGHT_MAX);
    let hi = hi.clamp(lo, HEIGHT_MAX);
    if right {
        app.terminal.width_cols = None;
    }
    let p = &mut app.terminal;
    let v = if right {
        &mut p.width_percent
    } else {
        &mut p.height_percent
    };
    // A stored share outside the drawable range counts as the nearest drawn one.
    let now = (*v).clamp(lo, hi);
    *v = if up {
        (now + HEIGHT_STEP).min(hi)
    } else {
        now.saturating_sub(HEIGHT_STEP).max(lo)
    };
}

/// `cells` of `total` as a rounded percent.
fn share(cells: u16, total: u16) -> u16 {
    ((u32::from(cells) * 100 + u32::from(total) / 2) / u32::from(total.max(1))) as u16
}

/// ctrl+f7: bottom ↔ right for this session (D227). A hidden panel stays hidden.
fn toggle_position(app: &mut App, _: CmdArgs) -> CmdResult {
    let to = match app.terminal.position {
        TerminalPosition::Bottom => TerminalPosition::Right,
        TerminalPosition::Right => TerminalPosition::Bottom,
    };
    set_position(app, to);
    Ok(())
}

/// Puts the panel at `to` for this session (ctrl+f7, `/term right|bottom`) and says so: where
/// it is shown when the editor column is too narrow for the right, and the setting that keeps
/// the choice when it differs from settings.toml (the file is never written, D227).
pub fn set_position(app: &mut App, to: TerminalPosition) {
    let name = |p: TerminalPosition| match p {
        TerminalPosition::Bottom => "bottom",
        TerminalPosition::Right => "right",
    };
    let place = match to {
        TerminalPosition::Bottom => "at the bottom",
        TerminalPosition::Right => "on the right",
    };
    if app.terminal.position == to {
        app.info(format!("Terminal panel is already {place}"));
        return;
    }
    let p = &mut app.terminal;
    p.position = to;
    // The grid changes shape: a selection or a drag of the old edge no longer applies.
    p.selection = None;
    p.selecting = false;
    p.resizing = false;
    app.dirty = true;
    let mut text = format!("Terminal panel {place}");
    if to == TerminalPosition::Right && !right_now(app) {
        text.push_str(&format!(
            " (shown at the bottom while the editor is under {} columns wide)",
            crate::ui::layout::TERMINAL_RIGHT_MIN_COLUMNS
        ));
    }
    if app.settings.terminal_position != to {
        text.push_str(&format!(
            "; to keep it, set terminal_position = \"{}\" in settings.toml (/config)",
            name(to)
        ));
    }
    app.info(text);
}

/// The panel goes on the right in an editor column `width` cells wide (the setting, unless the
/// column is too narrow).
pub fn right_in(app: &App, width: u16) -> bool {
    app.terminal.position == TerminalPosition::Right && crate::ui::layout::right_fits(width)
}

/// The panel is (or, shown, would be) on the right at the last frame's size. The tab bar spans
/// the editor column; before the first frame the setting alone decides.
fn right_now(app: &App) -> bool {
    let column = app.layout.tabs.width;
    if app.layout.status.height == 0 {
        return app.terminal.position == TerminalPosition::Right;
    }
    right_in(app, column)
}

/// The editor gets the keys; the panel stays (`focus.editor`, also what a click on the editor does).
fn focus_editor(app: &mut App, _: CmdArgs) -> CmdResult {
    // A panel that covers the whole editor keeps the keys: nothing would show them.
    if editor_covered(app) && has_keys(app) {
        return Ok(());
    }
    super::search::focus_editor(app);
    Ok(())
}

/// The last frame left the editor no rows (a window too short for both editor and panel).
/// Before the first frame nothing is known and the editor counts as shown.
pub fn editor_covered(app: &App) -> bool {
    app.layout.status.height > 0 && app.layout.editor.height == 0
}

/// Shows the panel with the keys on it, starting a terminal when there is none.
fn show(app: &mut App) {
    if app.terminal.tabs.is_empty() && !add_tab(app) {
        return;
    }
    reveal(app);
}

fn reveal(app: &mut App) {
    if !app.terminal.visible {
        // While hidden, tabs were not snapshot after a wake (their Notify stays disarmed until
        // the next snapshot): take one of each so titles and exit markers are current, which
        // also re-arms every Notify.
        for tab in &mut app.terminal.tabs {
            tab.refresh();
        }
    }
    // A side column that took the whole width would leave the panel no cells.
    if app.layout.editor.width == 0 && app.layout.explorer.width > 0 {
        app.explorer.visible = false;
    }
    app.terminal.visible = true;
    app.focus = Focus::Terminal;
    app.dirty = true;
}

/// The last frame left the panel no cells (a side column took the whole width, or the editor has
/// no rows): keys must not go to a terminal nobody can see.
fn covered(app: &App) -> bool {
    let l = &app.layout;
    l.status.height > 0
        && (l.terminal.width == 0 || l.terminal.height == 0)
        && (l.editor.width == 0 || l.editor.height == 0)
}

/// Hides the panel; terminals keep running. The editor gets the keys if the panel had them.
fn hide(app: &mut App) {
    let p = &mut app.terminal;
    p.visible = false;
    p.selection = None;
    p.selecting = false;
    p.captured = false;
    p.resizing = false;
    if app.focus == Focus::Terminal {
        super::search::focus_editor(app);
    }
    app.dirty = true;
}

fn step(app: &mut App, forward: bool) {
    let n = app.terminal.tabs.len();
    if n == 0 {
        return;
    }
    let p = &mut app.terminal;
    p.active = if forward {
        (p.active + 1) % n
    } else {
        (p.active + n - 1) % n
    };
    p.selection = None;
    // A release now would reach another program.
    p.captured = false;
    reveal(app);
}

/// Starts a terminal in a new tab and makes it active; a failure becomes an error notice.
/// A new terminal tab in the root of the active file, else the first root (spec W).
fn add_tab(app: &mut App) -> bool {
    let cwd = new_tab_dir(app);
    add_tab_in(app, &cwd)
}

/// A new terminal tab whose shell starts in `cwd`.
fn add_tab_in(app: &mut App, cwd: &Path) -> bool {
    let output = Arc::new(AtomicBool::new(false));
    match spawn(app, &output, cwd) {
        Ok((term, shell)) => {
            app.terminal.tabs.push(TermTab {
                term: Some(term),
                shell,
                scroll: 0,
                grid: Grid::default(),
                output,
                fake_status: TermStatus::Running,
                fake_title: String::new(),
            });
            app.terminal.active = app.terminal.tabs.len() - 1;
            app.terminal.selection = None;
            app.terminal.captured = false;
            true
        }
        Err(e) => {
            app.error(format!("Terminal: {e}"));
            false
        }
    }
}

/// Enter on an exited terminal: a new shell in the same tab.
fn restart(app: &mut App) {
    let i = app.terminal.active;
    let output = Arc::new(AtomicBool::new(false));
    let cwd = app.active_root().path.clone();
    match spawn(app, &output, &cwd) {
        Ok((term, shell)) => {
            let tab = &mut app.terminal.tabs[i];
            let old = tab.term.replace(term);
            tab.shell = shell;
            tab.output = output;
            tab.scroll = 0;
            tab.grid = Grid::default();
            drop_later(old);
        }
        Err(e) => app.error(format!("Terminal: {e}")),
    }
}

/// Closes the active terminal (its process group is hung up); the last one hides the panel.
fn close_active(app: &mut App) {
    let i = app.terminal.active;
    close_tab(app, i);
}

/// Drops a terminal off the UI thread: vr-term's Drop waits up to a grace period for a child
/// that ignores SIGHUP.
fn drop_later(term: Option<Terminal>) {
    let Some(term) = term else {
        return;
    };
    // When no thread can be started the closure, and the terminal with it, is dropped here.
    let _ = std::thread::Builder::new()
        .name("vr-term-close".into())
        .spawn(move || drop(term));
}

/// `$SHELL` (or the test override), falling back to `/bin/sh` when it cannot start.
fn spawn(
    app: &App,
    output: &Arc<AtomicBool>,
    cwd: &Path,
) -> Result<(Terminal, PathBuf), TermError> {
    let p = &app.terminal;
    let preferred = p.shell.clone().or_else(|| {
        std::env::var_os("SHELL")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
    });
    let (cols, rows) = spawn_size(app);
    let start = |shell: &Path| {
        let (o, w) = (output.clone(), p.wake.clone());
        let notify: vr_term::Notify = Arc::new(move || {
            o.store(true, Ordering::Release);
            w.store(true, Ordering::Release);
        });
        Terminal::spawn(
            SpawnOptions {
                shell: shell.to_path_buf(),
                args: Vec::new(),
                cwd: cwd.to_path_buf(),
                env: child_env(&p.env, crate::theme::ColorDepth::detect()),
                cols,
                rows,
                scrollback: p.scrollback,
            },
            notify,
        )
        .map(|t| (t, shell.to_path_buf()))
    };
    let fallback = Path::new(FALLBACK_SHELL);
    match preferred {
        Some(shell) if shell != fallback => match start(&shell) {
            Err(TermError::Spawn { .. }) => start(fallback),
            other => other,
        },
        _ => start(fallback),
    }
}

/// Environment of a new terminal: `COLORTERM=truecolor` only when vr-ide itself can show true
/// colour (its cells are otherwise downsampled, so a program must not be told it may use it);
/// the user's own entries come last and win.
pub(crate) fn child_env(
    user: &[(OsString, OsString)],
    depth: crate::theme::ColorDepth,
) -> Vec<(OsString, OsString)> {
    let mut env = Vec::new();
    if depth == crate::theme::ColorDepth::TrueColor {
        env.push((OsString::from("COLORTERM"), OsString::from("truecolor")));
    }
    env.extend(user.iter().cloned());
    env
}

/// Size for a new terminal: the panel's grid when laid out, else where the next frame puts it.
fn spawn_size(app: &App) -> (u16, u16) {
    let grid = |t: Rect| (t.height > 1 && t.width > 0).then(|| (t.width, t.height - 1));
    if let Some(size) = grid(app.layout.terminal) {
        return size;
    }
    let mut l = app.layout;
    let screen = l.status.y + l.status.height;
    if screen > 0 {
        if right_in(app, l.editor.width) {
            crate::ui::layout::place_terminal_right(
                &mut l,
                app.terminal.width_percent,
                app.terminal.width_cols,
            );
        } else {
            crate::ui::layout::place_terminal(&mut l, screen, app.terminal.height_percent);
        }
        if let Some(size) = grid(l.terminal) {
            return size;
        }
    }
    (80, 24)
}

// ---- event loop, layout ----------------------------------------------------------------------

/// After a wake: the tab bar's titles of the other tabs are refreshed. Output leaves a
/// scrolled-back view where it is (AC-F2): only a key, a paste or a scroll to the bottom returns
/// to it. True when the panel must be redrawn.
pub fn refresh(app: &mut App) -> bool {
    let p = &mut app.terminal;
    let mut changed = false;
    for (i, tab) in p.tabs.iter_mut().enumerate() {
        if !tab.output.swap(false, Ordering::AcqRel) {
            continue;
        }
        if i == p.active {
            p.selection = None;
        }
        // The active tab is snapshot by the next frame; a hidden panel draws nothing, and not
        // re-arming its Notify keeps a busy background program from waking the loop.
        if p.visible && i != p.active {
            tab.refresh();
        }
        changed |= p.visible;
    }
    changed
}

/// Gives every terminal the panel's grid size (after each layout change; a no-op when equal).
pub fn fit(app: &mut App, grid: Rect) {
    if grid.width == 0 || grid.height == 0 {
        return;
    }
    for tab in &app.terminal.tabs {
        if let Some(t) = &tab.term
            && t.size() != (grid.width, grid.height)
        {
            // An exited terminal refuses (`Closed`); its last screen keeps the old size.
            let _ = t.resize(grid.width, grid.height);
        }
    }
}

/// Snapshot of the active terminal for this frame.
pub fn snapshot_active(app: &mut App) {
    if let Some(tab) = app.terminal.active_mut() {
        tab.refresh();
    }
}

/// The terminal has the keys: focused, shown, no dialog or palette over it.
pub fn has_keys(app: &App) -> bool {
    app.focus == Focus::Terminal
        && app.terminal.visible
        && app.dialog.is_none()
        && app.palette.is_none()
}

/// The terminal is where keys go, also while the palette is open over it (status badge).
pub fn has_focus(app: &App) -> bool {
    let focus = app.palette.as_ref().map_or(app.focus, |p| p.prev_focus);
    focus == Focus::Terminal && app.terminal.visible && !app.terminal.tabs.is_empty()
}

/// The cursor shape the focused terminal's program asked for; `None` when the terminal does not
/// have the keys (the editor's own shapes apply then).
pub fn cursor_style(app: &App) -> Option<SetCursorStyle> {
    if !has_keys(app) {
        return None;
    }
    let tab = app.terminal.active_tab()?;
    Some(match tab.grid.cursor.shape {
        CursorShape::Block | CursorShape::HollowBlock => SetCursorStyle::SteadyBlock,
        CursorShape::Underline => SetCursorStyle::SteadyUnderScore,
        CursorShape::Beam => SetCursorStyle::SteadyBar,
    })
}

// ---- keys, paste ------------------------------------------------------------------------------

/// Keys while the terminal has the focus (D8): the `[terminal]` table runs its commands; every
/// other key goes to the program. Built in: shift+pageup/pagedown scroll back (main screen),
/// ctrl+shift+v pastes; on an exited terminal Enter restarts it and the close-file keys close
/// it. Returns false when the terminal does not have the focus.
pub fn handle_key(app: &mut App, key: &KeyEvent) -> bool {
    if app.focus != Focus::Terminal || app.dialog.is_some() {
        return false;
    }
    if !app.terminal.visible || app.terminal.tabs.is_empty() {
        // Keys never go to a terminal that is not shown.
        super::search::focus_editor(app);
        return false;
    }
    if covered(app) {
        super::search::focus_editor(app);
        return false;
    }
    let chord = chord_from_key(key, app.keyboard_enhanced);
    if let Some(cmd) = chord
        .as_ref()
        .and_then(|c| app.keymap.command_for_in(Context::Terminal, c))
    {
        let cmd = cmd.to_owned();
        app.run_command(&cmd, CmdArgs::None);
        return true;
    }
    // D214: a legacy terminal sends ctrl+` as NUL; here too it means ctrl+` (a [terminal]
    // ctrl+space binding was looked up first), unless the user wants NUL for the program.
    if nul_toggles_in_terminal(app)
        && is_nul(key)
        && let Some(cmd) = app.keymap.command_for_in(Context::Terminal, &tick_chord())
    {
        let cmd = cmd.to_owned();
        app.run_command(&cmd, CmdArgs::None);
        return true;
    }
    app.terminal.selection = None;
    if let Some(tab) = app.terminal.active_mut() {
        tab.refresh();
    }
    let Some(tab) = app.terminal.active_tab() else {
        return true;
    };
    if !tab.running() {
        let closes = chord
            .as_ref()
            .is_some_and(|c| app.keymap.keys_for("file.close").contains(c));
        if key.code == K::Enter && key.modifiers.is_empty() {
            restart(app);
        } else if closes {
            close_active(app);
        }
        return true;
    }
    let modes = tab.grid.modes;
    let shift_only = key.modifiers == KeyModifiers::SHIFT;
    if shift_only && matches!(key.code, K::PageUp | K::PageDown) && !modes.alt_screen {
        let page = tab.grid.rows.saturating_sub(1).max(1);
        let lines = page as isize;
        let up = key.code == K::PageUp;
        if let Some(tab) = app.terminal.active_mut() {
            tab.scroll_by(if up { lines } else { -lines });
        }
        return true;
    }
    if chord.as_ref() == Some(&paste_chord()) {
        let text = app.clipboard.get();
        if let Some(text) = text {
            paste(app, &text);
        }
        return true;
    }
    if let Some(k) = term_key(key) {
        let bytes = encode_key(k, modes);
        if let Some(tab) = app.terminal.active_mut() {
            tab.scroll_to_bottom();
        }
        send(app, &bytes);
    }
    true
}

fn paste_chord() -> KeyChord {
    "ctrl+shift+v".parse().expect("valid chord")
}

fn tick_chord() -> KeyChord {
    "ctrl+`".parse().expect("valid chord")
}

/// NUL as crossterm reports it (ctrl+space; `Null` on some platforms).
fn is_nul(key: &KeyEvent) -> bool {
    key.code == K::Null || (key.code == K::Char(' ') && key.modifiers == KeyModifiers::CONTROL)
}

/// Inside a focused terminal NUL stands for ctrl+` too: only without the kitty protocol (there
/// ctrl+` arrives as itself) and unless `terminal_pass_nul` gives NUL to the program (D214).
/// A panel that covers the whole editor leaves no editor to click, so there NUL always hides
/// the panel: it is the only way out from the keyboard.
pub fn nul_toggles_in_terminal(app: &App) -> bool {
    !app.keyboard_enhanced && (!app.settings.terminal_pass_nul || editor_covered(app))
}

/// A focused terminal's toggle key can hide the panel: always with the kitty protocol, and in a
/// legacy terminal while NUL is not passed to the program (the panel hint follows this).
pub fn hide_key_works(app: &App) -> bool {
    app.keyboard_enhanced || nul_toggles_in_terminal(app)
}

/// A legacy terminal sends ctrl+` as NUL, which crossterm reports as ctrl+space: outside a
/// focused terminal (and the palette) it runs what ctrl+` is bound to, unless the keymap binds
/// ctrl+space there itself (D185). With the kitty protocol ctrl+` arrives as itself. A focused
/// terminal handles NUL itself ([`handle_key`], D214).
pub fn legacy_toggle(app: &mut App, key: &KeyEvent) -> bool {
    if app.keyboard_enhanced
        || app.dialog.is_some()
        || matches!(app.focus, Focus::Terminal | Focus::Palette)
    {
        return false;
    }
    if !is_nul(key) {
        return false;
    }
    let ctx = app.focus.context();
    let space: KeyChord = "ctrl+space".parse().expect("valid chord");
    if app.keymap.command_for_in(ctx, &space).is_some() {
        return false;
    }
    let Some(cmd) = app.keymap.command_for_in(ctx, &tick_chord()) else {
        return false;
    };
    let cmd = cmd.to_owned();
    app.run_command(&cmd, CmdArgs::None);
    true
}

/// Pasted text for the focused terminal, bracketed when the program enabled it.
pub fn paste(app: &mut App, text: &str) {
    let Some(tab) = app.terminal.active_mut() else {
        return;
    };
    tab.refresh();
    if !tab.running() {
        return;
    }
    tab.scroll_to_bottom();
    let bytes = encode_paste(text, tab.grid.modes);
    send(app, &bytes);
}

/// Where [`type_line`] types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineTarget {
    /// The active terminal when its shell is in the foreground; a new one when there is none,
    /// its shell has exited or a program it started holds the terminal (D269).
    Active,
    /// Always a new terminal tab.
    NewTab,
}

/// Types `line` and Enter into a terminal, shown with the keys on it (`/run`, `/ai`, T20).
/// The bytes go to the terminal's own shell as if typed: vr-ide adds no shell of its own, so
/// the shell reads, expands and reports errors as usual. A new shell reads the line once it
/// starts (the PTY holds it). Err: no terminal could be started (also an error notice).
pub fn type_line(app: &mut App, line: &str, target: LineTarget) -> Result<(), String> {
    let active = app
        .terminal
        .active_tab()
        .filter(|t| t.term.is_some() && t.running());
    // Only the shell itself reads a command line: a program it started (an agent, an editor,
    // a `sudo` prompt) would take the line as its own input (D269).
    let busy = target == LineTarget::Active && active.is_some_and(|t| !t.shell_in_foreground());
    let reuse = target == LineTarget::Active && active.is_some() && !busy;
    if !reuse && !add_tab(app) {
        return Err("no terminal could be started".into());
    }
    if busy {
        app.info(
            "the active terminal is running a program: the command runs in a new terminal tab",
        );
    }
    reveal(app);
    if let Some(tab) = app.terminal.active_mut() {
        tab.scroll_to_bottom();
    }
    let mut bytes = line.as_bytes().to_vec();
    bytes.push(b'\r');
    send(app, &bytes);
    Ok(())
}

/// Writes to the active terminal; a program that stopped reading its input gets one warning.
fn send(app: &mut App, bytes: &[u8]) {
    let Some(tab) = app.terminal.active_tab() else {
        return;
    };
    match tab.write(bytes) {
        Ok(()) | Err(TermError::Closed) => {}
        Err(e) => {
            let text = format!("Terminal: {e}");
            if !app.notices.iter().any(|n| n.text == text) {
                app.warn(text);
            }
        }
    }
}

/// crossterm → vr-term key; `None` for keys a terminal program has no bytes for.
fn term_key(key: &KeyEvent) -> Option<vr_term::KeyEvent> {
    let m = key.modifiers;
    let mods = Modifiers {
        shift: m.contains(KeyModifiers::SHIFT),
        alt: m.contains(KeyModifiers::ALT),
        ctrl: m.contains(KeyModifiers::CONTROL),
    };
    let code = match key.code {
        K::Char(c) => T::Char(c),
        K::Null => return Some(vr_term::KeyEvent::new(T::Char(' '), Modifiers::CTRL)),
        K::Enter => T::Enter,
        K::Tab => T::Tab,
        K::BackTab => T::BackTab,
        K::Backspace => T::Backspace,
        K::Esc => T::Esc,
        K::Left => T::Left,
        K::Right => T::Right,
        K::Up => T::Up,
        K::Down => T::Down,
        K::Home => T::Home,
        K::End => T::End,
        K::PageUp => T::PageUp,
        K::PageDown => T::PageDown,
        K::Insert => T::Insert,
        K::Delete => T::Delete,
        K::F(n) if (1..=12).contains(&n) => T::F(n),
        _ => return None,
    };
    Some(vr_term::KeyEvent::new(code, mods))
}

// ---- mouse ----------------------------------------------------------------------------------

/// Mouse on the panel: a click on a tab activates it, a drag of the tab bar resizes the panel;
/// in the grid the mouse goes to the program when it asked for it (shift bypasses that, as in
/// xterm), otherwise a drag selects and copies, the wheel scrolls back (on the alternate screen
/// it sends arrow keys, as terminals do). Returns false for events outside the panel.
pub fn handle_mouse(app: &mut App, ev: &MouseEvent) -> bool {
    let area = app.layout.terminal;
    if app.terminal.resizing {
        match ev.kind {
            MK::Drag(MB::Left) => resize_to(app, ev.column, ev.row),
            MK::Up(MB::Left) => app.terminal.resizing = false,
            _ => {}
        }
        return true;
    }
    let grid_area = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        area.height.saturating_sub(1),
    );
    // A press the program got owns the mouse until its release, wherever that happens: drags
    // and the release reach the program clamped to its grid, as in any terminal emulator. A
    // new press ends it (the release was let go where vr-ide could not see it).
    if matches!(ev.kind, MK::Down(_)) {
        app.terminal.captured = false;
    }
    if app.terminal.captured && matches!(ev.kind, MK::Drag(_) | MK::Up(_)) {
        let (row, col) = clamp_cell(grid_area, ev.column, ev.row);
        if matches!(ev.kind, MK::Up(_)) {
            app.terminal.captured = false;
        }
        forward_mouse(app, ev, col as u16, row as u16);
        return true;
    }
    if app.terminal.selecting && matches!(ev.kind, MK::Drag(MB::Left) | MK::Up(MB::Left)) {
        let cell = clamp_cell(grid_area, ev.column, ev.row);
        if let Some(sel) = &mut app.terminal.selection {
            sel.head = cell;
        }
        if matches!(ev.kind, MK::Up(_)) {
            app.terminal.selecting = false;
            copy_selection(app);
        }
        return true;
    }
    let inside = |r: Rect| {
        ev.column >= r.x && ev.column < r.x + r.width && ev.row >= r.y && ev.row < r.y + r.height
    };
    // The separator of a panel on the right is its drag handle (T19).
    if app.terminal.visible && inside(app.layout.terminal_sep) {
        if ev.kind == MK::Down(MB::Left) {
            app.terminal.resizing = true;
        }
        return true;
    }
    if !app.terminal.visible || !inside(area) {
        return false;
    }
    if ev.row == area.y {
        if ev.kind == MK::Down(MB::Left) {
            match bar_hit(app, ev.column - area.x) {
                Some(BarHit::New) => new_terminal(app),
                Some(BarHit::Close(i)) => close_tab_asking(app, i),
                Some(BarHit::Tab(i)) => {
                    app.terminal.active = i;
                    app.terminal.selection = None;
                    app.terminal.captured = false;
                    app.focus = Focus::Terminal;
                }
                // At the bottom the tab bar is the top edge; on the right the separator is.
                None if app.layout.terminal_sep.width == 0 => app.terminal.resizing = true,
                None => {}
            }
        }
        return true;
    }
    if matches!(ev.kind, MK::Down(_)) {
        app.focus = Focus::Terminal;
    }
    let Some(tab) = app.terminal.active_mut() else {
        return true;
    };
    tab.refresh();
    let modes = tab.grid.modes;
    let (col, row) = (ev.column - grid_area.x, ev.row - grid_area.y);
    let reporting = modes.mouse_reporting && tab.scroll == 0 && tab.running();
    if reporting && !ev.modifiers.contains(KeyModifiers::SHIFT) {
        if matches!(ev.kind, MK::Down(_)) {
            app.terminal.captured = true;
        }
        forward_mouse(app, ev, col, row);
        return true;
    }
    match ev.kind {
        MK::Down(MB::Left) => {
            let cell = (usize::from(row), usize::from(col));
            app.terminal.selection = Some(Selection {
                anchor: cell,
                head: cell,
            });
            app.terminal.selecting = true;
        }
        MK::ScrollUp | MK::ScrollDown => {
            let up = ev.kind == MK::ScrollUp;
            if modes.alt_screen {
                if tab.running() {
                    let k = vr_term::KeyEvent::plain(if up { T::Up } else { T::Down });
                    let bytes = encode_key(k, modes).repeat(WHEEL_LINES);
                    send(app, &bytes);
                }
            } else {
                let lines = WHEEL_LINES as isize;
                tab.scroll_by(if up { lines } else { -lines });
                app.terminal.selection = None;
            }
        }
        _ => {}
    }
    true
}

/// Sends a mouse event at grid cell (`col`, `row`) to the active terminal's program, encoded
/// for the mouse mode it asked for.
fn forward_mouse(app: &mut App, ev: &MouseEvent, col: u16, row: u16) {
    let (Some(kind), Some(tab)) = (mouse_kind(ev.kind), app.terminal.active_tab()) else {
        return;
    };
    let m = vr_term::MouseEvent {
        kind,
        col,
        row,
        mods: Modifiers {
            shift: false,
            alt: ev.modifiers.contains(KeyModifiers::ALT),
            ctrl: ev.modifiers.contains(KeyModifiers::CONTROL),
        },
    };
    let bytes = encode_mouse(m, tab.grid.modes);
    send(app, &bytes);
}

fn mouse_kind(kind: MK) -> Option<MouseKind> {
    let b = |b: MB| match b {
        MB::Left => MouseButton::Left,
        MB::Middle => MouseButton::Middle,
        MB::Right => MouseButton::Right,
    };
    Some(match kind {
        MK::Down(x) => MouseKind::Press(b(x)),
        MK::Up(x) => MouseKind::Release(b(x)),
        MK::Drag(x) => MouseKind::Drag(b(x)),
        MK::Moved => MouseKind::Move,
        MK::ScrollUp => MouseKind::ScrollUp,
        MK::ScrollDown => MouseKind::ScrollDown,
        _ => return None,
    })
}

fn clamp_cell(grid: Rect, column: u16, row: u16) -> (usize, usize) {
    let r = row.clamp(grid.y, (grid.y + grid.height).max(grid.y + 1) - 1) - grid.y;
    let c = column.clamp(grid.x, (grid.x + grid.width).max(grid.x + 1) - 1) - grid.x;
    (usize::from(r), usize::from(c))
}

/// What a click on the tab bar hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BarHit {
    /// The '+' after the tabs: a new terminal.
    New,
    /// The 'x' of terminal `i`'s tab.
    Close(usize),
    /// The rest of terminal `i`'s tab.
    Tab(usize),
}

/// What column `x` of the tab bar holds, as the bar is drawn at the panel's width.
fn bar_hit(app: &App, x: u16) -> Option<BarHit> {
    let bar = crate::ui::terminal_view::tab_bar(app, app.layout.terminal.width);
    if bar
        .plus
        .is_some_and(|p| x >= p && x < p + crate::ui::terminal_view::PLUS_WIDTH)
    {
        return Some(BarHit::New);
    }
    bar.tabs
        .iter()
        .find(|t| x >= t.x && x < t.x + t.width)
        .map(|t| match t.close {
            Some(c) if x == t.x + c => BarHit::Close(t.index),
            _ => BarHit::Tab(t.index),
        })
}

/// Dragging the edge: at the bottom the panel reaches from the mouse row to its bottom; on the
/// right from the column after the mouse to the editor column's right edge, within the
/// minimums of panel and editor so the share always matches what is drawn.
fn resize_to(app: &mut App, column: u16, row: u16) {
    let area = app.layout.terminal;
    if app.layout.terminal_sep.width > 0 {
        let editor_column = app.layout.tabs.width;
        if editor_column == 0 {
            return;
        }
        let right_edge = area.x + area.width;
        let want = right_edge.saturating_sub(column + 1);
        let cols = want
            .min(crate::ui::layout::right_cols(editor_column, HEIGHT_MAX))
            .max(crate::ui::layout::right_cols(editor_column, HEIGHT_MIN));
        app.terminal.width_percent = share(cols, editor_column).clamp(HEIGHT_MIN, HEIGHT_MAX);
        app.terminal.width_cols = Some(cols);
        return;
    }
    let screen = app.layout.status.y + app.layout.status.height;
    if screen == 0 || area.height == 0 {
        return;
    }
    let bottom = area.y + area.height;
    let rows = bottom.saturating_sub(row).max(1);
    let percent = (u32::from(rows) * 100 + u32::from(screen) / 2) / u32::from(screen);
    app.terminal.height_percent = (percent as u16).clamp(HEIGHT_MIN, HEIGHT_MAX);
}

/// Copies the selected cells of the active terminal: lines joined by `\n`, trailing blanks
/// dropped, wide-character spacers skipped.
fn copy_selection(app: &mut App) {
    let (Some(sel), Some(tab)) = (app.terminal.selection, app.terminal.active_tab()) else {
        return;
    };
    let text = selected_text(&tab.grid, sel);
    if text.is_empty() {
        app.terminal.selection = None;
        return;
    }
    if let Err(e) = app.clipboard.set(&text) {
        app.error(format!("Terminal: copy failed: {e}"));
    }
}

pub fn selected_text(grid: &Grid, sel: Selection) -> String {
    let ((r0, c0), (r1, c1)) = sel.ordered();
    let mut lines = Vec::new();
    for r in r0..=r1.min(grid.rows.saturating_sub(1)) {
        let from = if r == r0 { c0 } else { 0 };
        let to = if r == r1 {
            c1
        } else {
            grid.cols.saturating_sub(1)
        };
        let mut line = String::new();
        for c in from..=to.min(grid.cols.saturating_sub(1)) {
            if let Some(cell) = grid.cell(r, c)
                && !cell.flags.wide_spacer
            {
                line.push_str(if cell.ch.is_empty() { " " } else { &cell.ch });
            }
        }
        line.truncate(line.trim_end().len());
        lines.push(line);
    }
    lines.join("\n")
}

// ---- quit -----------------------------------------------------------------------------------

/// Quit while terminals run, or while the Git popup runs a push, pull or commit (D309), asks
/// first in one question: "N terminals still running" / "A git push is running" — Quit
/// anyway / Cancel, Cancel selected (AC-14d). True when quitting may go on.
pub fn quit_allowed(app: &mut App) -> bool {
    quit_allowed_for(app, DialogAction::QuitTerminals)
}

/// [`quit_allowed`] whose question is answered by `action` (`/q!` keeps its discard).
pub fn quit_allowed_for(app: &mut App, action: DialogAction) -> bool {
    if std::mem::take(&mut app.terminal.quit_confirmed) {
        return true;
    }
    let n = app.terminal.running();
    let git = crate::git_popup::running_write(app);
    let mut parts = Vec::new();
    match n {
        0 => {}
        1 => parts.push("1 terminal still running; quitting ends it.".to_string()),
        n => parts.push(format!("{n} terminals still running; quitting ends them.")),
    }
    if let Some(w) = git {
        let what = if w.label == w.op {
            String::new()
        } else {
            format!(" ({})", w.label)
        };
        parts.push(format!(
            "A git {} is running{what}; quitting cancels it.",
            w.op
        ));
    }
    if parts.is_empty() {
        return true;
    }
    let message = parts.join(" ");
    let mut dialog = ConfirmDialog::new("Quit", message, &[QUIT_ANYWAY, CANCEL]);
    dialog.selected = 1;
    app.open_dialog(DialogWidget::Confirm(dialog), action);
    false
}

/// Quit anyway was chosen: the next quit does not ask about the terminals or a running git
/// write again (one shot).
pub fn confirm_quit(app: &mut App) {
    app.terminal.quit_confirmed = true;
}

/// The answer to [`quit_allowed`]'s question: Quit anyway goes on with the quit (unsaved files
/// still ask).
pub fn answer_quit(app: &mut App, button: usize) {
    if button == 0 {
        app.terminal.quit_confirmed = true;
        app.run_command("app.quit", CmdArgs::None);
    }
}

/// Ends every terminal (SIGHUP to each process group, SIGKILL after the grace period) and waits
/// for them, in parallel so the exit waits for the slowest one only.
pub fn shutdown(app: &mut App) {
    let tabs = std::mem::take(&mut app.terminal.tabs);
    app.terminal.visible = false;
    let handles: Vec<_> = tabs
        .into_iter()
        .filter_map(|t| t.term)
        .filter_map(|term| {
            // No thread: the terminal is dropped (and waited for) right here.
            std::thread::Builder::new()
                .name("vr-term-quit".into())
                .spawn(move || drop(term))
                .ok()
        })
        .collect();
    for h in handles {
        let _ = h.join();
    }
}
