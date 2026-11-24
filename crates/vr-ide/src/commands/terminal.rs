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
use vr_config::{Context, KeyChord};
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
        name: "terminal.new",
        title: "Terminal: New Terminal",
        run: new_terminal,
    },
    CommandSpec {
        name: "terminal.close",
        title: "Terminal: Close Terminal",
        run: close_terminal,
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
        title: "Terminal: Increase Panel Height",
        run: grow,
    },
    CommandSpec {
        name: "terminal.shrink",
        title: "Terminal: Decrease Panel Height",
        run: shrink,
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
            Some(t) => t.snapshot(self.scroll),
            None => self.grid.clone(),
        }
    }

    /// Takes a fresh snapshot into [`TermTab::grid`]; this also re-arms the terminal's `Notify`.
    pub fn refresh(&mut self) {
        if let Some(t) = &self.term {
            self.grid = t.snapshot(self.scroll);
            self.scroll = self.grid.scroll_offset;
        }
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
    /// Program to run instead of `$SHELL` (tests).
    pub shell: Option<PathBuf>,
    /// Extra environment for new terminals (tests).
    pub env: Vec<(OsString, OsString)>,
    pub scrollback: usize,
    pub selection: Option<Selection>,
    selecting: bool,
    /// The panel's top edge (its tab bar) is being dragged.
    resizing: bool,
    /// "Quit anyway" was chosen; the next quit does not ask about terminals again (one shot).
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
            shell: None,
            env: Vec::new(),
            scrollback,
            selection: None,
            selecting: false,
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

    /// Tab labels as the tab bar draws them: " 1 bash ", " 2 htop [0] " (exited with code 0).
    pub fn labels(&self) -> Vec<String> {
        self.tabs
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let mut title: String = t.title().chars().take(24).collect();
                if let TermStatus::Exited(code) = t.status() {
                    title.push_str(&format!(" [{code}]"));
                }
                format!(" {} {title} ", i + 1)
            })
            .collect()
    }
}

// ---- commands ------------------------------------------------------------------------------

/// Hidden: show and focus (a terminal starts when there is none); shown and focused: hide, the
/// editor gets the keys; shown elsewhere: focus it (D185).
fn toggle(app: &mut App, _: CmdArgs) -> CmdResult {
    if app.terminal.visible && app.focus == Focus::Terminal {
        hide(app);
    } else {
        show(app);
    }
    Ok(())
}

fn new_terminal(app: &mut App, _: CmdArgs) -> CmdResult {
    if add_tab(app) {
        reveal(app);
    }
    Ok(())
}

fn close_terminal(app: &mut App, _: CmdArgs) -> CmdResult {
    close_active(app);
    Ok(())
}

fn next(app: &mut App, _: CmdArgs) -> CmdResult {
    step(app, true);
    Ok(())
}

fn prev(app: &mut App, _: CmdArgs) -> CmdResult {
    step(app, false);
    Ok(())
}

fn grow(app: &mut App, _: CmdArgs) -> CmdResult {
    let p = &mut app.terminal;
    p.height_percent = (p.height_percent + HEIGHT_STEP).min(HEIGHT_MAX);
    Ok(())
}

fn shrink(app: &mut App, _: CmdArgs) -> CmdResult {
    let p = &mut app.terminal;
    p.height_percent = p.height_percent.saturating_sub(HEIGHT_STEP).max(HEIGHT_MIN);
    Ok(())
}

/// alt+q in a focused terminal (D8): the editor gets the keys; the panel stays.
fn focus_editor(app: &mut App, _: CmdArgs) -> CmdResult {
    super::search::focus_editor(app);
    Ok(())
}

/// Shows the panel with the keys on it, starting a terminal when there is none.
fn show(app: &mut App) {
    if app.terminal.tabs.is_empty() && !add_tab(app) {
        return;
    }
    reveal(app);
}

fn reveal(app: &mut App) {
    app.terminal.visible = true;
    app.focus = Focus::Terminal;
    app.dirty = true;
}

/// Hides the panel; terminals keep running. The editor gets the keys if the panel had them.
fn hide(app: &mut App) {
    let p = &mut app.terminal;
    p.visible = false;
    p.selection = None;
    p.selecting = false;
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
    reveal(app);
}

/// Starts a terminal in a new tab and makes it active; a failure becomes an error notice.
fn add_tab(app: &mut App) -> bool {
    let output = Arc::new(AtomicBool::new(false));
    match spawn(app, &output) {
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
    match spawn(app, &output) {
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
    let p = &mut app.terminal;
    if p.active >= p.tabs.len() {
        return;
    }
    let tab = p.tabs.remove(p.active);
    drop_later(tab.term);
    p.selection = None;
    if p.tabs.is_empty() {
        p.active = 0;
        hide(app);
    } else {
        p.active = p.active.min(p.tabs.len() - 1);
    }
    app.dirty = true;
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
fn spawn(app: &App, output: &Arc<AtomicBool>) -> Result<(Terminal, PathBuf), TermError> {
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
                cwd: app.root.clone(),
                env: p.env.clone(),
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

/// Size for a new terminal: the panel's grid when laid out, else where the next frame puts it.
fn spawn_size(app: &App) -> (u16, u16) {
    let grid = |t: Rect| (t.height > 1 && t.width > 0).then(|| (t.width, t.height - 1));
    if let Some(size) = grid(app.layout.terminal) {
        return size;
    }
    let mut l = app.layout;
    let screen = l.status.y + l.status.height;
    if screen > 0 {
        crate::ui::layout::place_terminal(&mut l, screen, app.terminal.height_percent);
        if let Some(size) = grid(l.terminal) {
            return size;
        }
    }
    (80, 24)
}

// ---- event loop, layout ----------------------------------------------------------------------

/// After a wake: terminals with new output return to the bottom (spec: any output does) and
/// the tab bar's titles of the other tabs are refreshed. True when the panel must be redrawn.
pub fn refresh(app: &mut App) -> bool {
    let p = &mut app.terminal;
    let mut changed = false;
    for (i, tab) in p.tabs.iter_mut().enumerate() {
        if !tab.output.swap(false, Ordering::AcqRel) {
            continue;
        }
        tab.scroll = 0;
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
    let chord = chord_from_key(key, app.keyboard_enhanced);
    if let Some(cmd) = chord
        .as_ref()
        .and_then(|c| app.keymap.command_for_in(Context::Terminal, c))
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
        let max = tab.grid.scrollback_len;
        let up = key.code == K::PageUp;
        if let Some(tab) = app.terminal.active_mut() {
            tab.scroll = if up {
                (tab.scroll + page).min(max)
            } else {
                tab.scroll.saturating_sub(page)
            };
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
            tab.scroll = 0;
        }
        send(app, &bytes);
    }
    true
}

fn paste_chord() -> KeyChord {
    "ctrl+shift+v".parse().expect("valid chord")
}

/// A legacy terminal sends ctrl+` as NUL, which crossterm reports as ctrl+space: outside a
/// focused terminal (and the palette) it runs what ctrl+` is bound to, unless the keymap binds
/// ctrl+space there itself (D185). With the kitty protocol ctrl+` arrives as itself.
pub fn legacy_toggle(app: &mut App, key: &KeyEvent) -> bool {
    if app.keyboard_enhanced
        || app.dialog.is_some()
        || matches!(app.focus, Focus::Terminal | Focus::Palette)
    {
        return false;
    }
    let nul =
        key.code == K::Null || (key.code == K::Char(' ') && key.modifiers == KeyModifiers::CONTROL);
    if !nul {
        return false;
    }
    let ctx = app.focus.context();
    let space: KeyChord = "ctrl+space".parse().expect("valid chord");
    if app.keymap.command_for_in(ctx, &space).is_some() {
        return false;
    }
    let tick: KeyChord = "ctrl+`".parse().expect("valid chord");
    let Some(cmd) = app.keymap.command_for_in(ctx, &tick) else {
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
    tab.scroll = 0;
    let bytes = encode_paste(text, tab.grid.modes);
    send(app, &bytes);
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
            MK::Drag(MB::Left) => resize_to(app, ev.row),
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
    if !app.terminal.visible || !inside(area) {
        return false;
    }
    if ev.row == area.y {
        if ev.kind == MK::Down(MB::Left) {
            match tab_at(app, ev.column - area.x) {
                Some(i) => {
                    app.terminal.active = i;
                    app.terminal.selection = None;
                    app.focus = Focus::Terminal;
                }
                None => app.terminal.resizing = true,
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
        if let Some(kind) = mouse_kind(ev.kind) {
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
            let bytes = encode_mouse(m, modes);
            send(app, &bytes);
        }
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
                let max = tab.grid.scrollback_len;
                tab.scroll = if up {
                    (tab.scroll + WHEEL_LINES).min(max)
                } else {
                    tab.scroll.saturating_sub(WHEEL_LINES)
                };
                app.terminal.selection = None;
            }
        }
        _ => {}
    }
    true
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

/// The tab under column `x` of the tab bar.
fn tab_at(app: &App, x: u16) -> Option<usize> {
    let mut start = 0usize;
    for (i, label) in app.terminal.labels().iter().enumerate() {
        let w = ratatui::text::Span::raw(label.as_str()).width();
        if usize::from(x) >= start && usize::from(x) < start + w {
            return Some(i);
        }
        start += w;
    }
    None
}

/// Dragging the top edge: the panel reaches from the mouse row to its bottom.
fn resize_to(app: &mut App, row: u16) {
    let area = app.layout.terminal;
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

/// Quit while terminals run asks first: "N terminals still running" — Quit anyway / Cancel,
/// Cancel selected (AC-14d). True when quitting may go on.
pub fn quit_allowed(app: &mut App) -> bool {
    if std::mem::take(&mut app.terminal.quit_confirmed) {
        return true;
    }
    let n = app.terminal.running();
    if n == 0 {
        return true;
    }
    let message = if n == 1 {
        "1 terminal still running; quitting ends it.".to_string()
    } else {
        format!("{n} terminals still running; quitting ends them.")
    };
    let mut dialog = ConfirmDialog::new("Quit", message, &[QUIT_ANYWAY, CANCEL]);
    dialog.selected = 1;
    app.open_dialog(DialogWidget::Confirm(dialog), DialogAction::QuitTerminals);
    false
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
