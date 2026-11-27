//! The AI panel (D7, D14): a column right of the editor whose tabs are agent sessions (several
//! of one tool at once), each a terminal program from `ai_tools` run on vr-term through the
//! terminal panel's machinery ([`TermTab`]). The panel shows / hides without stopping them; a
//! picker in its own tab starts new ones; a focused session gets every key but the `[ai]`
//! table's.

pub mod attention;
pub mod picker;
pub mod restore;
pub mod spawn;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crossterm::event::{
    KeyCode as K, KeyEvent, KeyModifiers, MouseButton as MB, MouseEvent, MouseEventKind as MK,
};
use ratatui::layout::Rect;
use vr_config::{AiTool, Context};
use vr_term::{SpawnExtras, TermStatus, encode_key, encode_mouse, encode_paste};

use crate::app::{App, Focus, WHEEL_LINES};
use crate::commands::dialog::DialogAction;
use crate::commands::file::CANCEL;
use crate::commands::registry::{CmdArgs, CmdResult, CommandSpec};
use crate::commands::terminal::{self as term, Selection, TermTab};
use crate::event_loop::chord_from_key;
use crate::ui::dialog::{ConfirmDialog, DialogWidget};
use picker::Picker;
use spawn::StartMode;

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "ai.toggle",
        title: "AI: Show / Hide Panel",
        run: toggle_cmd,
    },
    CommandSpec {
        name: "ai.new",
        title: "AI: New Session…",
        run: new_cmd,
    },
    CommandSpec {
        name: "ai.next",
        title: "AI: Next Session",
        run: next_cmd,
    },
    CommandSpec {
        name: "ai.prev",
        title: "AI: Previous Session",
        run: prev_cmd,
    },
    CommandSpec {
        name: "ai.grow",
        title: "AI: Increase Panel Width",
        run: grow_cmd,
    },
    CommandSpec {
        name: "ai.shrink",
        title: "AI: Decrease Panel Width",
        run: shrink_cmd,
    },
    CommandSpec {
        name: "ai.rename",
        title: "AI: Rename Session…",
        run: rename_cmd,
    },
    CommandSpec {
        name: "ai.close",
        title: "AI: Close Session",
        run: close_cmd,
    },
];

/// Width step of grow / shrink and the bounds of the setting, in percent.
pub const WIDTH_STEP: u16 = 5;
pub const WIDTH_MIN: u16 = 10;
pub const WIDTH_MAX: u16 = 90;

/// Longest name AI: Rename Session takes, in characters.
pub const NAME_MAX: usize = 32;

const CLOSE: &str = "Close";

/// One agent session: its program in a terminal tab and what started it.
pub struct AiSession {
    pub tab: TermTab,
    /// The tool's name in `ai_tools`.
    pub tool: String,
    /// The account it runs on (D11, T8); `None`: the tool's own config (`default`).
    pub account: Option<String>,
    /// The canonical workspace folder it started in.
    pub root: PathBuf,
    /// Set by AI: Rename Session; replaces the tool name in the label.
    pub name: Option<String>,
    /// How it was started last.
    pub mode: StartMode,
    /// The agent asked for the user (D9, T7 sets it): the order of the mark, oldest first.
    pub attention: Option<u64>,
    /// Rate limit and text of the current notice (SEC-14).
    pub notify: attention::NotifyState,
}

impl AiSession {
    pub fn running(&self) -> bool {
        self.tab.running()
    }
}

/// State of the AI panel.
pub struct AiPanel {
    pub sessions: Vec<AiSession>,
    /// The active tab: a session, or the picker at `sessions.len()` while it is open.
    pub active: usize,
    pub visible: bool,
    /// The picker's tab (`ctrl+f12`, `+`) or the empty state; `None` while closed.
    pub picker: Option<Picker>,
    /// Width in percent of the area right of the explorer (setting `ai_width_percent`).
    pub width_percent: u16,
    /// Exact width in columns (separator included) set by a drag; grow / shrink clear it.
    pub width_cols: Option<u16>,
    pub selection: Option<Selection>,
    /// What the session store last got from [`restore::report`]; `None` until
    /// [`restore::from_session`] ran (no `--session`, or not restored yet).
    pub(crate) reported: Option<crate::session::AiSection>,
    selecting: bool,
    /// The program got a mouse press; drags and the release go to it until that release.
    captured: bool,
    /// The separator is being dragged.
    resizing: bool,
}

impl AiPanel {
    pub fn new(width_percent: u16) -> AiPanel {
        AiPanel {
            sessions: Vec::new(),
            active: 0,
            visible: false,
            picker: None,
            width_percent: width_percent.clamp(WIDTH_MIN, WIDTH_MAX),
            width_cols: None,
            selection: None,
            reported: None,
            selecting: false,
            captured: false,
            resizing: false,
        }
    }

    /// Sessions plus the picker's tab.
    pub fn tab_count(&self) -> usize {
        self.sessions.len() + usize::from(self.picker.is_some())
    }

    /// The picker's tab is the one shown.
    pub fn picker_active(&self) -> bool {
        self.picker.is_some() && self.active >= self.sessions.len()
    }

    pub fn active_session(&self) -> Option<&AiSession> {
        if self.picker_active() {
            return None;
        }
        self.sessions.get(self.active)
    }

    fn active_session_mut(&mut self) -> Option<&mut AiSession> {
        if self.picker_active() {
            return None;
        }
        self.sessions.get_mut(self.active)
    }

    /// Sessions whose program still runs.
    pub fn running(&self) -> usize {
        self.sessions.iter().filter(|s| s.running()).count()
    }

    /// `tool[@account][·k]`, or `name[@account]` once renamed: `·k` numbers the 2nd, 3rd …
    /// session of the same tool and account (ui-spec S1).
    pub fn label(&self, i: usize) -> String {
        let s = &self.sessions[i];
        let account = s
            .account
            .as_deref()
            .map_or_else(String::new, |a| format!("@{a}"));
        if let Some(name) = &s.name {
            return format!("{name}{account}");
        }
        format!("{}{account}{}", s.tool, self.ordinal(i))
    }

    /// `·k` for the k-th session (k ≥ 2) of the same tool and account, else nothing.
    fn ordinal(&self, i: usize) -> String {
        let s = &self.sessions[i];
        let k = 1 + self.sessions[..i]
            .iter()
            .filter(|o| o.tool == s.tool && o.account == s.account)
            .count();
        if k > 1 {
            format!("·{k}")
        } else {
            String::new()
        }
    }

    /// What tells an inactive tab apart once labels must shrink: `@account` / `·k` when its tool
    /// is the active session's, else the tool (or the given name).
    pub fn short_label(&self, i: usize) -> String {
        let s = &self.sessions[i];
        if s.name.is_some() {
            return self.label(i);
        }
        let same_tool = self.active_session().is_some_and(|a| a.tool == s.tool);
        let account = s
            .account
            .as_deref()
            .map_or_else(String::new, |a| format!("@{a}"));
        let rest = format!("{account}{}", self.ordinal(i));
        if same_tool && !rest.is_empty() {
            rest
        } else {
            format!("{}{rest}", s.tool)
        }
    }

    /// The folder's name when it differs from the first session's.
    pub fn folder(&self, i: usize) -> Option<String> {
        let first = &self.sessions.first()?.root;
        let root = &self.sessions[i].root;
        (root != first).then(|| {
            root.file_name().map_or_else(
                || root.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            )
        })
    }
}

// ---- state checks -----------------------------------------------------------------------------

/// The panel has the keys: focused, shown, no dialog or palette over it.
pub fn has_keys(app: &App) -> bool {
    app.focus == Focus::Ai && app.ai.visible && app.dialog.is_none() && app.palette.is_none()
}

/// The panel is where keys go, also while the palette is open over it (status bar).
pub fn has_focus(app: &App) -> bool {
    let focus = app.palette.as_ref().map_or(app.focus, |p| p.prev_focus);
    focus == Focus::Ai && app.ai.visible
}

/// The last frame gave the panel the whole area right of the explorer (no editor drawn).
pub fn covers_editor(app: &App) -> bool {
    app.ai.visible && app.layout.ai.width > 0 && app.layout.editor.width == 0
}

/// The session to show when `f12` opens the hidden panel: the oldest one marked (D9, T7).
pub fn attention_target(app: &App) -> Option<usize> {
    app.ai
        .sessions
        .iter()
        .enumerate()
        .filter_map(|(i, s)| s.attention.map(|order| (order, i)))
        .min()
        .map(|(_, i)| i)
}

// ---- commands ---------------------------------------------------------------------------------

/// f12: hidden → shown with the keys (on the session needing attention, else the active one; the
/// empty state when there is none); shown elsewhere → the keys; focused → hidden, the sessions
/// keep running and the keys go back to the editor.
fn toggle_cmd(app: &mut App, _: CmdArgs) -> CmdResult {
    if !app.ai.visible {
        if let Some(i) = attention_target(app) {
            app.ai.active = i;
        }
        reveal(app);
    } else if app.focus != Focus::Ai {
        reveal(app);
    } else {
        hide(app);
    }
    Ok(())
}

/// ctrl+f12: the picker in a new tab of the panel, shown with the keys.
fn new_cmd(app: &mut App, _: CmdArgs) -> CmdResult {
    open_picker(app);
    Ok(())
}

fn next_cmd(app: &mut App, _: CmdArgs) -> CmdResult {
    step(app, true);
    Ok(())
}

fn prev_cmd(app: &mut App, _: CmdArgs) -> CmdResult {
    step(app, false);
    Ok(())
}

fn grow_cmd(app: &mut App, _: CmdArgs) -> CmdResult {
    step_width(app, true);
    Ok(())
}

fn shrink_cmd(app: &mut App, _: CmdArgs) -> CmdResult {
    step_width(app, false);
    Ok(())
}

/// The palette with `/ai rename ` typed, for the active session.
fn rename_cmd(app: &mut App, _: CmdArgs) -> CmdResult {
    if app.ai.active_session().is_none() {
        anyhow::bail!("no AI session is active");
    }
    crate::commands::palette::open(app);
    crate::commands::slash::set_input(app, "/ai rename ");
    Ok(())
}

/// The active session's 'x': asks first while its program runs; on the picker, closes it.
fn close_cmd(app: &mut App, _: CmdArgs) -> CmdResult {
    if app.ai.picker_active() {
        close_picker(app);
        return Ok(());
    }
    if app.ai.sessions.is_empty() {
        anyhow::bail!("no AI session is open");
    }
    let i = app.ai.active;
    close_asking(app, i);
    Ok(())
}

/// Shows the panel with the keys; with no session the picker is its empty state.
pub fn reveal(app: &mut App) {
    if !app.ai.visible {
        // Hidden sessions were not snapshot after a wake (their Notify stays disarmed until
        // the next snapshot): take one of each, which also re-arms every Notify.
        for s in &mut app.ai.sessions {
            s.tab.refresh();
        }
    }
    if app.ai.sessions.is_empty() && app.ai.picker.is_none() {
        let folder = start_folder(app);
        app.ai.picker = Some(Picker::build(app, folder, None, None));
        app.ai.active = 0;
    }
    app.ai.active = app.ai.active.min(app.ai.tab_count().saturating_sub(1));
    app.ai.visible = true;
    app.focus = Focus::Ai;
    app.dirty = true;
}

/// Hides the panel; sessions keep running. The editor gets the keys if the panel had them.
pub fn hide(app: &mut App) {
    let p = &mut app.ai;
    p.visible = false;
    p.selection = None;
    p.selecting = false;
    p.captured = false;
    p.resizing = false;
    // The empty state is rebuilt (the tools on PATH looked up again) when the panel shows.
    if p.sessions.is_empty() {
        p.picker = None;
    }
    if app.focus == Focus::Ai {
        crate::commands::search::focus_editor(app);
    }
    app.dirty = true;
}

/// The folder a new session starts in: the active file's workspace folder (D7), canonical.
fn start_folder(app: &App) -> PathBuf {
    let dir = term::new_tab_dir(app);
    dir.canonicalize().unwrap_or(dir)
}

/// The picker in its own tab (or the open one again), preselecting the active session's tool.
pub fn open_picker(app: &mut App) {
    let prefer = app
        .ai
        .active_session()
        .map(|s| (s.tool.clone(), s.account.clone()));
    let back = match &app.ai.picker {
        Some(p) => p.back,
        None => (!app.ai.sessions.is_empty()).then_some(app.ai.active),
    };
    let folder = start_folder(app);
    let picker = Picker::build(
        app,
        folder,
        prefer.as_ref().map(|(t, a)| (t.as_str(), a.as_deref())),
        back,
    );
    app.ai.picker = Some(picker);
    app.ai.active = app.ai.sessions.len();
    reveal(app);
}

/// Esc in the picker: back to the session it was opened from; the empty state hides the panel.
fn close_picker(app: &mut App) {
    let Some(picker) = app.ai.picker.take() else {
        return;
    };
    match picker.back {
        Some(b) if !app.ai.sessions.is_empty() => {
            app.ai.active = b.min(app.ai.sessions.len() - 1);
            app.dirty = true;
        }
        _ => {
            app.ai.picker = Some(picker);
            hide(app);
        }
    }
}

fn step(app: &mut App, forward: bool) {
    let n = app.ai.tab_count();
    if n == 0 {
        reveal(app);
        return;
    }
    let p = &mut app.ai;
    p.active = if forward {
        (p.active + 1) % n
    } else {
        (p.active + n - 1) % n
    };
    p.selection = None;
    p.captured = false;
    reveal(app);
}

/// Width of the area the panel shares with the editor (the screen right of the explorer), from
/// the last frame; 0 before it.
fn region_width(app: &App) -> u16 {
    let l = &app.layout;
    if l.ai.width == 0 {
        return 0;
    }
    l.tabs.width + l.ai_sep.width + l.ai.width
}

/// ctrl+f8 / ctrl+f9 while the panel has the keys: the stored share is clamped to what can be
/// drawn first, so each press changes the panel at once.
fn step_width(app: &mut App, up: bool) {
    use crate::ui::layout::{AI_EDITOR_MIN_COLS, AI_MIN_COLS};
    let region = region_width(app);
    let (lo, hi) = if region > AI_MIN_COLS + AI_EDITOR_MIN_COLS {
        (
            term::share(AI_MIN_COLS, region),
            term::share(region - AI_EDITOR_MIN_COLS, region),
        )
    } else {
        (WIDTH_MIN, WIDTH_MAX)
    };
    let lo = lo.clamp(WIDTH_MIN, WIDTH_MAX);
    let hi = hi.clamp(lo, WIDTH_MAX);
    let p = &mut app.ai;
    p.width_cols = None;
    let now = p.width_percent.clamp(lo, hi);
    p.width_percent = if up {
        (now + WIDTH_STEP).min(hi)
    } else {
        now.saturating_sub(WIDTH_STEP).max(lo)
    };
    app.dirty = true;
}

/// Dragging the separator: the panel reaches from the column after the mouse to the right edge.
fn resize_to(app: &mut App, column: u16) {
    use crate::ui::layout::{AI_EDITOR_MIN_COLS, AI_MIN_COLS};
    let region = region_width(app);
    if region < AI_MIN_COLS + AI_EDITOR_MIN_COLS {
        return;
    }
    let right = app.layout.ai.x + app.layout.ai.width;
    let total = right
        .saturating_sub(column)
        .clamp(AI_MIN_COLS, region - AI_EDITOR_MIN_COLS);
    app.ai.width_cols = Some(total);
    app.ai.width_percent = term::share(total, region).clamp(WIDTH_MIN, WIDTH_MAX);
    app.dirty = true;
}

// ---- sessions ---------------------------------------------------------------------------------

/// `/ai <tool> [@account]`: a new session of a tool of `ai_tools` in this panel (New).
pub fn open_tool(app: &mut App, arg: &str) -> Result<(), String> {
    let mut words = arg.split_whitespace();
    let name = words.next().ok_or("/ai needs <tool>")?;
    let account = words.next();
    if words.next().is_some() {
        return Err("/ai takes <tool> [@account]".into());
    }
    match account {
        None | Some("@default") => {}
        Some(a) if a.starts_with('@') => {
            return Err(format!(
                "/ai: account {} not found; only default is available",
                crate::ui::safe_text::sanitize(&a[1..])
            ));
        }
        Some(_) => return Err("/ai takes <tool> [@account]".into()),
    }
    let tool = find_tool(app, name)?;
    let folder = start_folder(app);
    spawn_session(app, &tool, None, StartMode::New, &folder, None)?;
    app.ai.picker = None;
    app.ai.active = app.ai.sessions.len() - 1;
    reveal(app);
    Ok(())
}

/// `/ai rename <name>`: the active session's label shows `name`.
pub fn rename(app: &mut App, name: &str) -> Result<(), String> {
    let name = clean_name(name)?;
    let s = app
        .ai
        .active_session_mut()
        .ok_or("/ai rename: no AI session is active")?;
    s.name = Some(name);
    app.dirty = true;
    Ok(())
}

/// A session name as shown: one line, no control or direction characters, at most
/// [`NAME_MAX`] characters.
pub fn clean_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("/ai rename needs <name>".into());
    }
    if name.chars().count() > NAME_MAX {
        return Err(format!("/ai rename: at most {NAME_MAX} characters"));
    }
    let clean = crate::ui::safe_text::sanitize(name);
    if clean != name || name.chars().any(char::is_control) {
        return Err("/ai rename: the name has characters that cannot be shown".into());
    }
    Ok(clean)
}

fn find_tool(app: &App, name: &str) -> Result<AiTool, String> {
    app.settings
        .ai_tools
        .iter()
        .find(|t| t.name == name)
        .cloned()
        .ok_or_else(|| {
            let names: Vec<&str> = app
                .settings
                .ai_tools
                .iter()
                .map(|t| t.name.as_str())
                .collect();
            format!(
                "/ai: {} is not in ai_tools ({})",
                crate::ui::safe_text::sanitize(name),
                names.join(", ")
            )
        })
}

/// Starts `tool` as a new session in `root` (canonical) and adds it after the others; the
/// caller decides what becomes active. SEC-8: the program and its arguments come from
/// `ai_tools` only, found in the absolute `PATH` entries, started without a shell; its
/// environment loses `VR_IDE_*` and the relative `PATH` entries.
pub(crate) fn spawn_session(
    app: &mut App,
    tool: &AiTool,
    account: Option<String>,
    mode: StartMode,
    root: &Path,
    name: Option<String>,
) -> Result<(), String> {
    let (tab_term, program, output) = start_program(app, tool, account.as_deref(), mode, root)?;
    app.ai.sessions.push(AiSession {
        tab: TermTab::new(tab_term, program, output),
        tool: tool.name.clone(),
        account,
        root: root.to_path_buf(),
        name,
        mode,
        attention: None,
        notify: Default::default(),
    });
    app.dirty = true;
    Ok(())
}

fn start_program(
    app: &App,
    tool: &AiTool,
    account: Option<&str>,
    mode: StartMode,
    root: &Path,
) -> Result<(vr_term::Terminal, PathBuf, Arc<AtomicBool>), String> {
    let path = std::env::var_os("PATH");
    let program = spawn::resolve(&tool.command, path.as_deref())
        .ok_or_else(|| format!("{} not on PATH (ai_tools in settings)", tool.name))?;
    let args: Vec<OsString> = spawn::args(tool, mode)
        .into_iter()
        .map(OsString::from)
        .collect();
    let mut env = Vec::new();
    if let Some(p) = &path {
        env.push((OsString::from("PATH"), spawn::absolute_path(p)));
    }
    env.extend(spawn::account_env(tool, account));
    let extras = SpawnExtras {
        marker: None,
        env_remove: spawn::internal_vars(std::env::vars_os()),
        attention: true,
    };
    let output = Arc::new(AtomicBool::new(false));
    let size = grid_size(app);
    let t = term::spawn_sized(app, &output, &program, args, root, extras, env, size)
        .map_err(|e| e.to_string())?;
    Ok((t, program, output))
}

/// The session grid's size: the panel's last frame, else where the next frame puts it.
fn grid_size(app: &App) -> (u16, u16) {
    let grid = |r: Rect| (r.height > 1 && r.width > 0).then(|| (r.width, r.height - 1));
    if let Some(size) = grid(app.layout.ai) {
        return size;
    }
    let mut l = app.layout;
    if l.status.height > 0 {
        crate::ui::layout::place_ai(&mut l, app.ai.width_percent, app.ai.width_cols);
        if let Some(size) = grid(l.ai) {
            return size;
        }
    }
    (80, 24)
}

/// Starts the picker's choice: the second step when the tool resumes, else a new session.
fn picker_enter(app: &mut App) {
    let Some(p) = &app.ai.picker else {
        return;
    };
    let folder = p.folder.clone();
    let (tool, account, mode) = if let Some(s) = &p.second {
        (s.tool.clone(), s.account.clone(), s.modes[s.selected])
    } else {
        let Some((tool, account)) = p.choice() else {
            return;
        };
        let (tool, account) = (tool.to_string(), account.map(str::to_string));
        let Ok(t) = find_tool(app, &tool) else {
            return;
        };
        let modes = spawn::modes(&t);
        if modes.len() > 1 {
            if let Some(p) = &mut app.ai.picker {
                p.second = Some(picker::Second {
                    tool,
                    account,
                    modes,
                    selected: 0,
                });
            }
            app.dirty = true;
            return;
        }
        (tool, account, StartMode::New)
    };
    let started =
        find_tool(app, &tool).and_then(|t| spawn_session(app, &t, account, mode, &folder, None));
    match started {
        Ok(()) => {
            app.ai.picker = None;
            app.ai.active = app.ai.sessions.len() - 1;
            reveal(app);
        }
        Err(e) => app.error(format!("AI: {e}")),
    }
}

/// Enter on an exited session: its tool again in the same tab, with the continue arguments
/// when the tool has them.
fn restart(app: &mut App) {
    let Some(s) = app.ai.active_session() else {
        return;
    };
    let (name, account, root) = (s.tool.clone(), s.account.clone(), s.root.clone());
    let result = find_tool(app, &name).and_then(|tool| {
        let mode = spawn::again_mode(&tool);
        start_program(app, &tool, account.as_deref(), mode, &root).map(|r| (r, mode))
    });
    match result {
        Ok(((t, program, output), mode)) => {
            if let Some(s) = app.ai.active_session_mut() {
                s.tab.replace(t, program, output);
                s.mode = mode;
            }
            app.dirty = true;
        }
        Err(e) => app.error(format!("AI: {e}")),
    }
}

/// Closes session `i` at once when its program has exited; else "Close session?" asks first,
/// Cancel selected.
pub fn close_asking(app: &mut App, i: usize) {
    let Some(s) = app.ai.sessions.get(i) else {
        return;
    };
    if !s.running() {
        close(app, i);
        return;
    }
    let message = format!(
        "{} is running; closing the session ends it.",
        app.ai.label(i)
    );
    let mut dialog = ConfirmDialog::new("Close session?", message, &[CLOSE, CANCEL]);
    dialog.selected = 1;
    let pid = s.tab.pid();
    app.open_dialog(
        DialogWidget::Confirm(dialog),
        DialogAction::CloseAiSession(pid),
    );
}

/// Close was chosen: the session asked about goes (found by pid; already gone = nothing).
pub fn answer_close(app: &mut App, pid: Option<u32>) {
    if let Some(i) = app.ai.sessions.iter().position(|s| s.tab.pid() == pid) {
        close(app, i);
    }
}

/// Closes session `i` without asking (its process group is hung up). The last one leaves the
/// empty state while the panel shows.
pub fn close(app: &mut App, i: usize) {
    let p = &mut app.ai;
    if i >= p.sessions.len() {
        return;
    }
    let s = p.sessions.remove(i);
    s.tab.close_later();
    p.selection = None;
    p.captured = false;
    if let Some(picker) = &mut p.picker
        && let Some(b) = &mut picker.back
    {
        *b = b.saturating_sub(usize::from(i <= *b && *b > 0));
    }
    if p.sessions.is_empty() {
        p.active = 0;
        if p.picker.is_none() && p.visible {
            reveal(app);
        }
    } else if i < p.active || p.active >= p.tab_count() {
        p.active = p.active.saturating_sub(1);
    }
    app.dirty = true;
}

// ---- event loop, layout -----------------------------------------------------------------------

/// After a wake: other sessions' tabs are refreshed while the panel shows (the active one is
/// snapshot by the next frame). True when the panel must be redrawn.
pub fn refresh(app: &mut App) -> bool {
    let mut changed = attention::collect(app);
    let p = &mut app.ai;
    let active = if p.picker_active() {
        usize::MAX
    } else {
        p.active
    };
    for (i, s) in p.sessions.iter_mut().enumerate() {
        if !s.tab.take_output() {
            continue;
        }
        if i == active {
            p.selection = None;
        }
        // A hidden panel draws nothing; not re-arming the Notify keeps a busy agent from
        // waking the loop.
        if p.visible && i != active {
            s.tab.refresh();
        }
        changed |= p.visible;
    }
    changed
}

/// Gives every session the grid size, an exited one less the rows of its exited line (after
/// each layout change; a no-op when equal).
pub fn fit(app: &mut App, grid: Rect) {
    if grid.width == 0 || grid.height == 0 {
        return;
    }
    for s in &app.ai.sessions {
        let reserved = crate::ui::ai_view::reserved_rows(app, s, grid);
        s.tab.resize(grid.width, grid.height - reserved);
    }
}

/// Snapshot of the active session for this frame.
pub fn snapshot_active(app: &mut App) {
    if let Some(s) = app.ai.active_session_mut() {
        s.tab.refresh();
    }
}

/// Ends every session and waits for them, in parallel.
pub fn shutdown(app: &mut App) {
    let sessions = std::mem::take(&mut app.ai.sessions);
    app.ai.visible = false;
    let handles: Vec<_> = sessions
        .into_iter()
        .filter_map(|s| s.tab.into_terminal())
        .filter_map(|t| {
            std::thread::Builder::new()
                .name("vr-term-quit".into())
                .spawn(move || drop(t))
                .ok()
        })
        .collect();
    for h in handles {
        let _ = h.join();
    }
}

// ---- keys, paste ------------------------------------------------------------------------------

/// Keys while the panel has the focus: the `[ai]` table runs its commands; in the picker the
/// arrows, Enter and Esc work it; every other key goes to the session's program. Built in:
/// shift+pageup / pagedown scroll back, ctrl+shift+v pastes; on an exited session Enter starts
/// it again and the close-file keys close it. False when the panel does not have the focus.
pub fn handle_key(app: &mut App, key: &KeyEvent) -> bool {
    if app.dialog.is_some() {
        return false;
    }
    // A panel over the whole editor area leaves nothing else to type into.
    if covers_editor(app) && matches!(app.focus, Focus::Editor | Focus::FindBar | Focus::Search) {
        app.focus = Focus::Ai;
    }
    if app.focus != Focus::Ai {
        return false;
    }
    if !app.ai.visible {
        crate::commands::search::focus_editor(app);
        return false;
    }
    let chord = chord_from_key(key, app.keyboard_enhanced);
    if let Some(cmd) = chord
        .as_ref()
        .and_then(|c| app.keymap.command_for_in(Context::Ai, c))
    {
        let cmd = cmd.to_owned();
        app.run_command(&cmd, CmdArgs::None);
        return true;
    }
    if term::nul_toggles_in_terminal(app)
        && term::is_nul(key)
        && let Some(cmd) = app.keymap.command_for_in(Context::Ai, &term::tick_chord())
    {
        let cmd = cmd.to_owned();
        app.run_command(&cmd, CmdArgs::None);
        return true;
    }
    if app.ai.picker_active() {
        picker_key(app, key);
        return true;
    }
    app.ai.selection = None;
    let Some(s) = app.ai.active_session_mut() else {
        return true;
    };
    s.tab.refresh();
    let modes = s.tab.grid.modes;
    let shift_only = key.modifiers == KeyModifiers::SHIFT;
    // Scrolling back works on an exited session too: its output stays to be read.
    if shift_only && matches!(key.code, K::PageUp | K::PageDown) && !modes.alt_screen {
        let lines = s.tab.grid.rows.saturating_sub(1).max(1) as isize;
        let up = key.code == K::PageUp;
        s.tab.scroll_by(if up { lines } else { -lines });
        app.dirty = true;
        return true;
    }
    if !s.running() {
        let closes = chord
            .as_ref()
            .is_some_and(|c| app.keymap.keys_for("file.close").contains(c));
        if key.code == K::Enter && key.modifiers.is_empty() {
            restart(app);
        } else if closes {
            let i = app.ai.active;
            close(app, i);
        }
        return true;
    }
    if chord.as_ref() == Some(&term::paste_chord()) {
        if let Some(text) = app.clipboard.get() {
            paste(app, &text);
        }
        return true;
    }
    if let Some(k) = term::term_key(key) {
        let bytes = encode_key(k, modes);
        s.tab.scroll_to_bottom();
        send(app, &bytes);
    }
    true
}

fn picker_key(app: &mut App, key: &KeyEvent) {
    if !key.modifiers.is_empty() && key.modifiers != KeyModifiers::SHIFT {
        return;
    }
    match key.code {
        K::Up | K::Down => {
            if let Some(p) = &mut app.ai.picker {
                p.step(key.code == K::Down);
            }
        }
        K::Enter => picker_enter(app),
        K::Esc => {
            let second = app.ai.picker.as_mut().and_then(|p| p.second.take());
            if second.is_none() {
                close_picker(app);
            }
        }
        _ => {}
    }
    app.dirty = true;
}

/// Pasted text for the focused session, bracketed when the program enabled it.
pub fn paste(app: &mut App, text: &str) {
    let Some(s) = app.ai.active_session_mut() else {
        return;
    };
    s.tab.refresh();
    if !s.running() {
        return;
    }
    s.tab.scroll_to_bottom();
    let bytes = encode_paste(text, s.tab.grid.modes);
    app.ai.selection = None;
    send(app, &bytes);
}

/// Writes to the active session; a program that stopped reading gets one warning.
fn send(app: &mut App, bytes: &[u8]) {
    let Some(s) = app.ai.active_session() else {
        return;
    };
    match s.tab.write(bytes) {
        Ok(()) | Err(vr_term::TermError::Closed) => {}
        Err(e) => {
            let text = format!("AI: {e}");
            if !app.notices.iter().any(|n| n.text == text) {
                app.warn(text);
            }
        }
    }
}

// ---- mouse ------------------------------------------------------------------------------------

/// What a click on the tab bar hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarHit {
    New,
    Close(usize),
    Tab(usize),
}

/// Mouse on the panel: the separator resizes it; a tab is chosen, its 'x' closes it, '+' opens
/// the picker; in the picker a click starts a row; in a session the mouse goes to the program
/// when it asked for it (shift bypasses that), otherwise a drag selects and copies and the wheel
/// scrolls back. False for events outside the panel.
pub fn handle_mouse(app: &mut App, ev: &MouseEvent) -> bool {
    if app.ai.resizing {
        match ev.kind {
            MK::Drag(MB::Left) => resize_to(app, ev.column),
            MK::Up(MB::Left) => app.ai.resizing = false,
            _ => {}
        }
        return true;
    }
    let area = app.layout.ai;
    let grid_area = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        area.height.saturating_sub(1),
    );
    if matches!(ev.kind, MK::Down(_)) {
        app.ai.captured = false;
    }
    if app.ai.captured && matches!(ev.kind, MK::Drag(_) | MK::Up(_)) {
        let (row, col) = term::clamp_cell(grid_area, ev.column, ev.row);
        if matches!(ev.kind, MK::Up(_)) {
            app.ai.captured = false;
        }
        forward_mouse(app, ev, col as u16, row as u16);
        return true;
    }
    if app.ai.selecting && matches!(ev.kind, MK::Drag(MB::Left) | MK::Up(MB::Left)) {
        let cell = term::clamp_cell(grid_area, ev.column, ev.row);
        if let Some(sel) = &mut app.ai.selection {
            sel.head = cell;
        }
        if matches!(ev.kind, MK::Up(_)) {
            app.ai.selecting = false;
            copy_selection(app);
        }
        app.dirty = true;
        return true;
    }
    let inside = |r: Rect| {
        ev.column >= r.x && ev.column < r.x + r.width && ev.row >= r.y && ev.row < r.y + r.height
    };
    if !app.ai.visible {
        return false;
    }
    if inside(app.layout.ai_sep) {
        if ev.kind == MK::Down(MB::Left) {
            app.ai.resizing = true;
        }
        return true;
    }
    if !inside(area) {
        return false;
    }
    app.dirty = true;
    if ev.row == area.y {
        if ev.kind == MK::Down(MB::Left) {
            match bar_hit(app, ev.column - area.x) {
                Some(BarHit::New) => open_picker(app),
                Some(BarHit::Close(i)) if i >= app.ai.sessions.len() => close_picker(app),
                Some(BarHit::Close(i)) => close_asking(app, i),
                Some(BarHit::Tab(i)) => {
                    app.ai.active = i;
                    app.ai.selection = None;
                    reveal(app);
                }
                None => app.focus = Focus::Ai,
            }
        }
        return true;
    }
    if matches!(ev.kind, MK::Down(_)) {
        app.focus = Focus::Ai;
    }
    if app.ai.picker_active() {
        if ev.kind == MK::Down(MB::Left) {
            picker_click(app, ev.row.saturating_sub(grid_area.y));
        }
        return true;
    }
    let Some(s) = app.ai.active_session_mut() else {
        return true;
    };
    s.tab.refresh();
    let modes = s.tab.grid.modes;
    let (col, row) = (ev.column - grid_area.x, ev.row - grid_area.y);
    let reporting = modes.mouse_reporting && s.tab.scroll == 0 && s.running();
    if reporting && !ev.modifiers.contains(KeyModifiers::SHIFT) {
        if matches!(ev.kind, MK::Down(_)) {
            app.ai.captured = true;
        }
        forward_mouse(app, ev, col, row);
        return true;
    }
    match ev.kind {
        MK::Down(MB::Left) => {
            let cell = (usize::from(row), usize::from(col));
            app.ai.selection = Some(Selection {
                anchor: cell,
                head: cell,
            });
            app.ai.selecting = true;
        }
        MK::ScrollUp | MK::ScrollDown => {
            let up = ev.kind == MK::ScrollUp;
            if modes.alt_screen {
                if s.running() {
                    let k = vr_term::KeyEvent::plain(if up {
                        vr_term::KeyCode::Up
                    } else {
                        vr_term::KeyCode::Down
                    });
                    let bytes = encode_key(k, modes).repeat(WHEEL_LINES);
                    send(app, &bytes);
                }
            } else {
                let lines = WHEEL_LINES as isize;
                s.tab.scroll_by(if up { lines } else { -lines });
                app.ai.selection = None;
            }
        }
        _ => {}
    }
    true
}

fn bar_hit(app: &App, x: u16) -> Option<BarHit> {
    let bar = crate::ui::ai_view::tab_bar(app, app.layout.ai.width);
    if bar
        .plus
        .is_some_and(|p| x >= p && x < p + crate::ui::ai_view::PLUS_WIDTH)
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

/// A click on picker line `line` (0 = the grid's first row) selects that row and starts it.
fn picker_click(app: &mut App, line: u16) {
    let Some(p) = &mut app.ai.picker else {
        return;
    };
    let Some(i) = usize::from(line).checked_sub(crate::ui::ai_view::PICKER_ROWS_AT) else {
        return;
    };
    if let Some(s) = &mut p.second {
        // The tool's name heads the second step.
        let Some(m) = i.checked_sub(1).filter(|&m| m < s.modes.len()) else {
            return;
        };
        s.selected = m;
    } else {
        if !matches!(p.rows.get(i), Some(picker::Row::Choice { .. })) {
            return;
        }
        p.selected = i;
    }
    picker_enter(app);
}

fn forward_mouse(app: &mut App, ev: &MouseEvent, col: u16, row: u16) {
    let (Some(kind), Some(s)) = (term::mouse_kind(ev.kind), app.ai.active_session()) else {
        return;
    };
    let m = vr_term::MouseEvent {
        kind,
        col,
        row,
        mods: vr_term::Modifiers {
            shift: false,
            alt: ev.modifiers.contains(KeyModifiers::ALT),
            ctrl: ev.modifiers.contains(KeyModifiers::CONTROL),
        },
    };
    let bytes = encode_mouse(m, s.tab.grid.modes);
    send(app, &bytes);
}

fn copy_selection(app: &mut App) {
    let (Some(sel), Some(s)) = (app.ai.selection, app.ai.active_session()) else {
        return;
    };
    let text = term::selected_text(&s.tab.grid, sel);
    if text.is_empty() {
        app.ai.selection = None;
        return;
    }
    if let Err(e) = app.clipboard.set(&text) {
        app.error(format!("AI: copy failed: {e}"));
    }
}

/// An exited session's status (`[exited 0]`), `None` while it runs.
pub fn exit_code(s: &AiSession) -> Option<i32> {
    match s.tab.status() {
        TermStatus::Exited(c) => Some(c),
        TermStatus::Running => None,
    }
}
