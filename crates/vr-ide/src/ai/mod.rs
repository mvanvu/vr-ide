//! The AI panel (D7, D14): a column right of the editor whose tabs are agent sessions (several
//! of one tool at once), each a terminal program from `ai_tools` run on vr-term through the
//! terminal panel's machinery ([`TermTab`]). The panel shows / hides without stopping them; a
//! picker in its own tab starts new ones; a focused session gets every key but the `[ai]`
//! table's.

pub mod accounts;
pub mod attention;
pub mod changes;
#[cfg(test)]
mod changes_tests;
pub mod history;
pub mod limits;
pub mod picker;
pub mod restore;
pub mod review;
pub mod spawn;
pub mod usage;

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
const REMOVE: &str = "Remove";

/// One agent session: its program in a terminal tab and what started it.
pub struct AiSession {
    pub tab: TermTab,
    /// The tool's name in `ai_tools`.
    pub tool: String,
    /// The account it runs on (D11, T8); `None`: the tool's own config (`default`).
    pub account: Option<String>,
    /// The hold on that account (D17): no window removes it while this session is open.
    pub account_lock: Option<accounts::AccountLock>,
    /// The canonical workspace folder it started in.
    pub root: PathBuf,
    /// Set by AI: Rename Session; replaces the tool name in the label.
    pub name: Option<String>,
    /// How it was started last.
    pub mode: StartMode,
    /// The agent asked for the user (D9, T7 sets it): the order of the mark, oldest first.
    pub attention: Option<u64>,
    /// Tells this session from every other one of this run (dialogs and the diff tab name it).
    pub serial: u64,
    /// What changed in its folder while it ran (D8); `None`: not recorded.
    pub changes: Option<changes::Changes>,
    /// The tracker's list and banner as last read ([`review::refresh`]).
    pub records: Vec<changes::Record>,
    pub banner: Option<changes::Banner>,
    pub(crate) changes_version: Option<u64>,
    /// Rate limit and text of the current notice (SEC-14).
    pub notify: attention::NotifyState,
    /// Its history is the tool's shared one (AC-2): the default account, or a named account
    /// whose history entry links there; false for a tool without one, or when a warning said
    /// why not.
    pub shared: bool,
    /// The statusLine report of a claude session (AC-3, SEC-22); `None`: none asked for.
    pub usage_file: Option<PathBuf>,
    /// What its last valid report said.
    pub usage: Option<usage::Usage>,
    /// When the report was last looked at (SEC-25, SEC-39).
    pub(crate) usage_poll: usage::Poll,
    /// The settings and report files of the current run; dropping them removes both.
    pub(crate) usage_files: Option<usage::SessionFiles>,
    /// The tool showed its limit message and this event is not over yet (AC-5, SEC-26): set on a
    /// new hit, cleared once the user typed and the limit line is gone, or by a restart.
    pub limit_seen: bool,
    /// When its screen is read for limit and login messages.
    pub(crate) limit_watch: limits::Watch,
}

impl AiSession {
    pub fn new(
        tab: TermTab,
        tool: &str,
        account: Option<String>,
        root: PathBuf,
        mode: StartMode,
    ) -> AiSession {
        static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        AiSession {
            tab,
            tool: tool.to_string(),
            account,
            account_lock: None,
            root,
            name: None,
            mode,
            attention: None,
            serial: SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            changes: None,
            records: Vec::new(),
            banner: None,
            changes_version: None,
            notify: Default::default(),
            shared: false,
            usage_file: None,
            usage: None,
            usage_poll: Default::default(),
            usage_files: None,
            limit_seen: false,
            limit_watch: Default::default(),
        }
    }

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
    /// `state_dir()/ai`, where sessions keep earlier versions (D8); `None`: changes are not
    /// recorded (tests that do not set it).
    pub changes_dir: Option<PathBuf>,
    /// Bytes of every session's store (the total cap is shared).
    pub changes_total: Arc<std::sync::atomic::AtomicU64>,
    pub changes_limits: changes::Limits,
    /// Folders never recorded: the state folder, the config folder, the trash.
    pub changes_excluded: Vec<PathBuf>,
    /// The changes list of the active session while open (f6).
    pub list: Option<review::ChangeList>,
    /// What the editor's diff tab shows.
    pub diff: Option<review::DiffTab>,
    /// Rows the list took at the top of the session in the last frame.
    pub list_rows: u16,
    /// The state folder holding `accounts/` (D11); `None`: no account can be used (tests that
    /// do not set one never touch the real state folder).
    pub state_dir: Option<PathBuf>,
    /// The shared history folder of each tool that has one (AC-2, SEC-21), set at startup by
    /// [`history::configure`]; empty: no account shares history (tests that do not set it).
    pub history_dirs: std::collections::BTreeMap<String, PathBuf>,
    /// The program claude's statusLine runs (`vr-ide --ai-statusline`); `None`: this executable.
    /// Tests point it at a stand-in, the unit-test binary not being vr-ide.
    pub usage_exe: Option<PathBuf>,
    /// Old windows' usage files were swept (once, before the first claude session).
    pub(crate) usage_swept: bool,
    /// The "usage not available" notice was shown (once per run).
    pub(crate) usage_warned: bool,
    /// Report reads so far (SEC-39 test seam).
    pub usage_reads: u64,
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
            changes_dir: None,
            changes_total: Arc::default(),
            changes_limits: changes::Limits::default(),
            changes_excluded: Vec::new(),
            list: None,
            diff: None,
            list_rows: 0,
            state_dir: None,
            history_dirs: Default::default(),
            usage_exe: None,
            usage_swept: false,
            usage_warned: false,
            usage_reads: 0,
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
    let account = match account {
        None => None,
        Some(a) => {
            let a = a.strip_prefix('@').ok_or("/ai takes <tool> [@account]")?;
            if a == accounts::DEFAULT {
                None
            } else {
                let a = accounts::validate(a)
                    .map_err(|e| format!("/ai: account name: {}", e.message()))?;
                Some(a.to_string())
            }
        }
    };
    let tool = find_tool(app, name)?;
    if let Some(a) = &account {
        if !picker::supports_accounts(&tool.name) {
            return Err(format!("/ai: {} cannot use accounts", tool.name));
        }
        let known = app
            .ai
            .state_dir
            .as_deref()
            .is_some_and(|s| accounts::for_tool(s, &tool.name).contains(a));
        if !known {
            return Err(format!("/ai: account {a} not found for {}", tool.name));
        }
    }
    let folder = start_folder(app);
    spawn_session(app, &tool, account, StartMode::New, &folder, None)?;
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
    // Before the hold: a first migration needs the account to itself (SEC-20).
    let shared = share_history(app, &tool.name, account.as_deref());
    // The hold comes first: the folder checks in `start_program` then see what a removal left.
    let account_lock = match (&account, app.ai.state_dir.as_deref()) {
        (None, _) => None,
        (Some(a), Some(state)) => {
            Some(accounts::hold(state, a).map_err(|e| format!("account {a}: {e}"))?)
        }
        (Some(a), None) => return Err(format!("account {a}: account folder missing")),
    };
    // The tracker's start time must precede the program's first write (SEC-9).
    let tracker = review::start(app, root);
    let (tab_term, program, output, usage_files) =
        start_program(app, tool, account.as_deref(), mode, root)?;
    let mut session = AiSession::new(
        TermTab::new(tab_term, program, output),
        &tool.name,
        account,
        root.to_path_buf(),
        mode,
    );
    session.account_lock = account_lock;
    session.shared = shared;
    session.name = name;
    session.changes = tracker;
    session.usage_file = usage_files.as_ref().map(|f| f.report.clone());
    session.usage_files = usage_files;
    app.ai.sessions.push(session);
    app.dirty = true;
    Ok(())
}

/// AC-2: a named claude / codex account's history entry is linked to the tool's shared folder
/// before its session starts ([`history::ensure`]); when it cannot be, the session still starts,
/// unshared, with one warning naming the reason. The default account is the shared folder.
fn share_history(app: &mut App, tool: &str, account: Option<&str>) -> bool {
    let Some(shared) = app.ai.history_dirs.get(tool).cloned() else {
        return false;
    };
    let Some(name) = account else {
        return true;
    };
    // A missing or invalid account is reported by the hold that follows.
    let Some(dir) = app
        .ai
        .state_dir
        .as_deref()
        .and_then(|s| accounts::path_of(s, name).ok())
        .filter(|d| d.symlink_metadata().is_ok())
    else {
        return false;
    };
    match history::ensure(&dir, tool, &shared) {
        history::Shared::Linked => true,
        history::Shared::NotShared(why) => {
            app.warn(format!("{tool}@{name}: history not shared ({why})"));
            false
        }
    }
}

/// The program, its tab output flag, and the usage files of a claude session (AC-3).
type Started = (
    vr_term::Terminal,
    PathBuf,
    Arc<AtomicBool>,
    Option<usage::SessionFiles>,
);

fn start_program(
    app: &mut App,
    tool: &AiTool,
    account: Option<&str>,
    mode: StartMode,
    root: &Path,
) -> Result<Started, String> {
    let path = std::env::var_os("PATH");
    let program = spawn::resolve(&tool.command, path.as_deref())
        .ok_or_else(|| format!("{} not on PATH (ai_tools in settings)", tool.name))?;
    let mut args: Vec<OsString> = spawn::args(tool, mode)
        .into_iter()
        .map(OsString::from)
        .collect();
    // SEC-22: two argv elements, before the tool's own arguments.
    let usage_files = usage_files(app, tool);
    if let Some(f) = &usage_files {
        args.splice(
            0..0,
            [
                OsString::from("--settings"),
                f.settings.clone().into_os_string(),
            ],
        );
    }
    let mut env = Vec::new();
    if let Some(p) = &path {
        env.push((OsString::from("PATH"), spawn::absolute_path(p)));
    }
    env.extend(spawn::account_env(
        app.ai.state_dir.as_deref(),
        tool,
        account,
    )?);
    let mut env_remove = spawn::internal_vars(std::env::vars_os());
    // SEC-38: an inherited variable pointing at one of vr-ide's accounts never reaches an agent
    // started without that account (a named account's own variable is set after the removal).
    if let Some(state) = app.ai.state_dir.as_deref() {
        env_remove.extend(history::stray_vars(state, std::env::vars_os()));
    }
    let extras = SpawnExtras {
        marker: None,
        env_remove,
        attention: true,
    };
    let output = Arc::new(AtomicBool::new(false));
    let size = grid_size(app);
    let t = term::spawn_sized(app, &output, &program, args, root, extras, env, size)
        .map_err(|e| e.to_string())?;
    Ok((t, program, output, usage_files))
}

/// AC-3, SEC-22: claude with a state folder reports through the statusLine helper; the first
/// one of a run sweeps what ended windows left. A path the helper cannot be named by gives one
/// notice per run and a start without it.
fn usage_files(app: &mut App, tool: &AiTool) -> Option<usage::SessionFiles> {
    if tool.name != "claude" {
        return None;
    }
    let state = app.ai.state_dir.clone()?;
    if !app.ai.usage_swept {
        app.ai.usage_swept = true;
        usage::sweep(&state);
    }
    let exe = match app.ai.usage_exe.clone() {
        Some(e) => Ok(e),
        None => std::env::current_exe().map_err(|_| "vr-ide path unusable"),
    };
    match exe.and_then(|e| usage::prepare(&state, &e)) {
        Ok(files) => Some(files),
        Err(why) => {
            if !app.ai.usage_warned {
                app.ai.usage_warned = true;
                app.warn(format!("claude usage not available ({why})"));
            }
            None
        }
    }
}

/// Reads the reports that changed, on every loop pass (claude may report while silent or while
/// the panel is hidden; SEC-25 / SEC-39 bound it: an `lstat` per session at most every 250 ms, a
/// read at most once a second); true when a session's usage changed.
pub(crate) fn read_usage(app: &mut App, now: std::time::Instant) -> bool {
    let p = &mut app.ai;
    let mut reported = Vec::new();
    for (i, s) in p.sessions.iter_mut().enumerate() {
        let Some(path) = &s.usage_file else {
            continue;
        };
        let Some(read) = s.usage_poll.check(path, now) else {
            continue;
        };
        p.usage_reads += 1;
        // A bad or missing report is ignored, without a notice (SEC-25).
        if let Some(u) = read
            && s.usage.as_ref() != Some(&u)
        {
            s.usage = Some(u);
            reported.push(i);
        }
    }
    // Whether the account works and is limited (SEC-27).
    for &i in &reported {
        limits::on_report(app, i);
    }
    !reported.is_empty()
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
    let Some(p) = &mut app.ai.picker else {
        return;
    };
    if p.adding.is_some() {
        add_account(app);
        return;
    }
    if let Some(tool) = p.add_tool() {
        p.adding = Some(picker::AddField {
            tool: tool.to_string(),
            text: String::new(),
            error: None,
        });
        app.dirty = true;
        return;
    }
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
    // A new run: what the folder holds now is its starting point.
    if let Some(c) = &s.changes {
        c.resume();
    }
    let result = find_tool(app, &name).and_then(|tool| {
        let mode = spawn::again_mode(&tool);
        start_program(app, &tool, account.as_deref(), mode, &root).map(|r| (r, mode))
    });
    match result {
        Ok(((t, program, output, usage_files), mode)) => {
            if let Some(s) = app.ai.active_session_mut() {
                s.tab.replace(t, program, output);
                s.mode = mode;
                // The last run's files go; the usage it reported stays until a new report, but
                // a New run is another conversation: the old id must not be resumed for it.
                if mode == StartMode::New
                    && let Some(u) = &mut s.usage
                {
                    u.session_id = None;
                }
                s.usage_file = usage_files.as_ref().map(|f| f.report.clone());
                s.usage_files = usage_files;
                s.usage_poll = Default::default();
                s.limit_seen = false;
                s.limit_watch.restarted();
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
    let Some(serial) = app.ai.sessions.get(i).map(|s| s.serial) else {
        return;
    };
    review::session_closing(app, serial);
    let p = &mut app.ai;
    // Dropping the session ends its tracker and deletes its store (SEC-10).
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

// ---- accounts (D11) ---------------------------------------------------------------------------

/// Enter in the name field: the account folder is made (SEC-15, SEC-16) and a session starts on
/// it (New); a refused name stays in the field with the reason under it.
fn add_account(app: &mut App) {
    let Some(p) = &app.ai.picker else {
        return;
    };
    let Some(field) = &p.adding else {
        return;
    };
    let (tool, name, folder) = (field.tool.clone(), field.text.clone(), p.folder.clone());
    let made = match app.ai.state_dir.as_deref() {
        Some(state) => accounts::create(state, &tool, &name),
        None => Err("account folders are not available".to_string()),
    };
    if let Err(e) = made {
        if let Some(f) = app.ai.picker.as_mut().and_then(|p| p.adding.as_mut()) {
            f.error = Some(e);
        }
        app.dirty = true;
        return;
    }
    let started = find_tool(app, &tool)
        .and_then(|t| spawn_session(app, &t, Some(name.clone()), StartMode::New, &folder, None));
    match started {
        Ok(()) => {
            app.ai.picker = None;
            app.ai.active = app.ai.sessions.len() - 1;
            reveal(app);
            app.info(format!(
                "Account {name} created; sign in with /login in its tab"
            ));
        }
        Err(e) => {
            rebuild_picker(app);
            app.error(format!("AI: {e}"));
        }
    }
}

/// Why account `name` cannot be removed now: sessions are open on it (running or not; names are
/// unique across tools).
fn account_busy(app: &App, name: &str) -> Option<String> {
    let tabs: Vec<String> = app
        .ai
        .sessions
        .iter()
        .enumerate()
        .filter(|(_, s)| s.account.as_deref() == Some(name))
        .map(|(i, _)| (i + 1).to_string())
        .collect();
    match tabs.as_slice() {
        [] => None,
        [one] => Some(format!(
            "Not removed: close the session on {name} first, tab {one}"
        )),
        _ => Some(format!(
            "Not removed: close the {} sessions on {name} first, tabs {}",
            tabs.len(),
            tabs.join(", ")
        )),
    }
}

/// Delete on a picker row: a named account asks first (Cancel selected, SEC-17) unless a
/// session is open on it; `default` is the tool's own config and stays.
fn ask_remove_account(app: &mut App) {
    let Some(p) = &app.ai.picker else {
        return;
    };
    if p.second.is_some() {
        return;
    }
    let Some((tool, account)) = p.choice() else {
        return;
    };
    let tool = tool.to_string();
    let Some(name) = account.map(str::to_string) else {
        if picker::supports_accounts(&tool) {
            app.info(format!("default is {tool}'s own config"));
        }
        return;
    };
    if let Some(why) = account_busy(app, &name) {
        app.warn(why);
        return;
    }
    let Some(path) = app
        .ai
        .state_dir
        .as_deref()
        .and_then(|s| accounts::path_of(s, &name).ok())
    else {
        return;
    };
    let shown = crate::git::home_relative(&path);
    let message = format!(
        "Deletes {shown}/ for good: its {tool} login, settings and history.\n\
         The sign-in stays valid at the provider; to end it, run /logout in a session on \
         {name} first.\n\
         Other accounts and {tool}'s own config stay."
    );
    let mut dialog = ConfirmDialog::new(
        format!("Remove account {name}?"),
        message,
        &[REMOVE, CANCEL],
    );
    dialog.selected = 1;
    app.open_dialog(
        DialogWidget::Confirm(dialog),
        DialogAction::RemoveAiAccount(name),
    );
}

/// Remove was chosen: checked again (a session may have started on it meanwhile), then the
/// folder is deleted for good (SEC-17) and the picker lists what is left.
pub fn answer_remove_account(app: &mut App, name: &str) {
    if let Some(why) = account_busy(app, name) {
        app.warn(why);
        return;
    }
    let Some(state) = app.ai.state_dir.clone() else {
        return;
    };
    match accounts::remove(&state, name) {
        Ok(()) => app.info(format!("Account {name} removed")),
        Err(e) => app.error(format!("Not removed: {name}, {e}")),
    }
    rebuild_picker(app);
}

/// The open picker listed again (accounts changed), on the same tool's `default`.
fn rebuild_picker(app: &mut App) {
    let Some(old) = app.ai.picker.take() else {
        return;
    };
    let tool = old.rows.get(old.selected).and_then(|r| match r {
        picker::Row::Choice { tool, .. } | picker::Row::Add { tool } => Some(tool.clone()),
        picker::Row::Header { .. } => None,
    });
    let prefer = tool.as_deref().map(|t| (t, None));
    let picker = Picker::build(app, old.folder, prefer, old.back);
    app.ai.picker = Some(picker);
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
        s.limit_watch.output();
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
    let listed = review::refresh(app);
    changed || listed
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
    if review::handle_key(app, key) {
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
        send_typed(app, &bytes);
    }
    true
}

fn picker_key(app: &mut App, key: &KeyEvent) {
    if !key.modifiers.is_empty() && key.modifiers != KeyModifiers::SHIFT {
        return;
    }
    if let Some(field) = app.ai.picker.as_mut().and_then(|p| p.adding.as_mut()) {
        match key.code {
            K::Char(c) if field.text.chars().count() < picker::FIELD_MAX => {
                field.text.push(c);
                field.error = None;
            }
            K::Backspace => {
                field.text.pop();
                field.error = None;
            }
            K::Enter => add_account(app),
            K::Esc => {
                if let Some(p) = &mut app.ai.picker {
                    p.adding = None;
                }
            }
            _ => {}
        }
        app.dirty = true;
        return;
    }
    match key.code {
        K::Delete => ask_remove_account(app),
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
    if app.ai.picker_active()
        && let Some(field) = app.ai.picker.as_mut().and_then(|p| p.adding.as_mut())
    {
        // One line, as typed; the validator judges the rest.
        let line = text.lines().next().unwrap_or("");
        let room = picker::FIELD_MAX.saturating_sub(field.text.chars().count());
        field.text.extend(line.chars().take(room));
        field.error = None;
        app.dirty = true;
        return;
    }
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
    send_typed(app, &bytes);
}

/// Keys or a paste for the active session: input that ends a limit event once answered.
fn send_typed(app: &mut App, bytes: &[u8]) {
    if let Some(s) = app.ai.active_session_mut() {
        s.limit_watch.typed_input(bytes);
    }
    send(app, bytes);
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
    /// The `±N` count: opens the changes list.
    Changes,
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
    // The open changes list sits between the tab bar and the session.
    let list_rows = if app.ai.list.is_some() && !app.ai.picker_active() {
        app.ai.list_rows.min(area.height.saturating_sub(1))
    } else {
        0
    };
    let grid_area = Rect::new(
        area.x,
        area.y + 1 + list_rows,
        area.width,
        area.height.saturating_sub(1 + list_rows),
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
                Some(BarHit::Changes) => review::open_list(app),
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
    if ev.row < grid_area.y {
        if ev.kind == MK::Down(MB::Left)
            && let Some(row) = crate::ui::ai_changes::row_at(app, ev.row - area.y - 1)
        {
            review::click_row(app, row);
        }
        return true;
    }
    if matches!(ev.kind, MK::Down(_)) {
        app.focus = Focus::Ai;
        if let Some(l) = &mut app.ai.list {
            l.focused = false;
        }
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
        .changes
        .as_ref()
        .is_some_and(|(text, at)| x >= *at && usize::from(x - *at) < text.chars().count())
    {
        return Some(BarHit::Changes);
    }
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
    // The name field shifts the rows under it; clicks wait until it is closed.
    if p.adding.is_some() {
        return;
    }
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
        if !matches!(
            p.rows.get(i),
            Some(picker::Row::Choice { .. } | picker::Row::Add { .. })
        ) {
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
