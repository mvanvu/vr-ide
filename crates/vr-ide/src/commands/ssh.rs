//! SSH sessions in the terminal panel (FR-25, D317, D5): the `ssh` entry of the command window,
//! starting the system `ssh` in a tab, the login event that gives a session the whole window,
//! reconnecting and the recent list. What is run and with which arguments is decided in
//! [`crate::ssh`]; this module only wires it to the panel.
//!
//! `/ssh` takes the words that would follow `ssh ` in a terminal ([`crate::ssh::args`]), and
//! ssh runs in the workspace folder of the active file, so relative paths in them resolve as
//! they would in a terminal opened there (SEC-4).
//!
//! A session starts as an ordinary tab, so host-key questions and password prompts are answered
//! there; vr-ide never reads, stores or logs what is typed into it. Full screen follows the
//! marker ssh's `LocalCommand` prints after a successful login, never anything the remote side
//! sends (SEC-S6, SEC-S8).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use vr_term::SpawnExtras;

use super::registry::{CmdArgs, CmdResult, CommandSpec};
use super::terminal;
use crate::app::App;
use crate::ssh::args::{self, Line};
use crate::ssh::recent::Entry;
use crate::ssh::{self, MAX_SESSIONS, Nonce, Probe};

pub const COMMANDS: &[CommandSpec] = &[CommandSpec {
    name: "ssh.connect",
    title: "SSH: Connect to Host",
    run: open_list,
}];

/// The slash command the palette entry and a typed `ssh ` lead to.
pub const SLASH: &str = "/ssh ";

/// The notice when the folder ssh would run in is gone (SEC-4): never another folder instead.
pub const BASE_GONE: &str = "SSH: the workspace folder of the active file is missing; not started";
/// The same for a recent line run again: it runs where it ran before, or not at all (SEC-5).
pub const RECENT_BASE_GONE: &str =
    "SSH: the folder this recent line ran in is missing; not started";

/// The SSH session of a terminal tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// Stays with the tab across reconnects.
    pub id: u64,
    /// Where it goes: the source of the tab label and the status row (the line's validated
    /// destination, SEC-2).
    pub target: ssh::Target,
    /// What was typed after `/ssh `, checked.
    pub line: Line,
    /// The folder ssh runs in (canonical when it started).
    pub base: PathBuf,
    /// Its own login marker came through its own output (this start of ssh).
    pub logged_in: bool,
    /// ssh was started with vr-ide's `LocalCommand`, so a marker can come at all.
    marked: bool,
    /// The line is in the recent list for this start.
    recorded: bool,
}

/// SSH state of the app that belongs to no single tab.
pub struct SshState {
    /// Program run instead of the `ssh` found on `PATH` (tests).
    pub bin: Option<PathBuf>,
    /// The home folder whose `.ssh/config` lists hosts and that `~` names in a line; `None`:
    /// no list (tests), `~` stays as typed.
    pub home: Option<PathBuf>,
    /// Where the recent list is kept; `None`: in memory only (tests).
    pub recent_file: Option<PathBuf>,
    /// Recently used lines, most recent first.
    pub recent: Vec<Entry>,
    /// How long the configuration probe may take.
    pub probe_timeout: Duration,
    /// The line the command window is about to run when it was picked from the list, with
    /// the folder it brings ([`picked`]); taken by [`connect`].
    pub picked: Option<(String, PathBuf)>,
    /// The one probe running (SEC-S16), with what to start when it answers.
    pending: Option<Pending>,
    next_id: u64,
}

impl Default for SshState {
    fn default() -> Self {
        SshState {
            bin: None,
            home: None,
            recent_file: None,
            recent: Vec::new(),
            probe_timeout: ssh::PROBE_TIMEOUT,
            picked: None,
            pending: None,
            next_id: 1,
        }
    }
}

impl SshState {
    /// State for the real app: the user's home and the recent list under `state_dir`.
    pub fn load(state_dir: &Path) -> SshState {
        let file = state_dir.join(ssh::recent::FILE);
        SshState {
            home: std::env::var_os("HOME")
                .filter(|h| !h.is_empty())
                .map(PathBuf::from),
            recent: ssh::recent::load(&file),
            recent_file: Some(file),
            ..SshState::default()
        }
    }

    /// A session is waiting for its configuration probe.
    pub fn starting(&self) -> bool {
        self.pending.is_some()
    }

    /// A probe for `line` that never answers, started `age` ago (tests). The returned sender
    /// keeps its channel open.
    #[cfg(test)]
    pub(crate) fn stall_probe(&mut self, line: &str, age: Duration) -> mpsc::Sender<Probe> {
        let (tx, rx) = mpsc::channel();
        self.pending = Some(Pending {
            launch: Launch {
                line: Line::parse(line, None).expect("a valid line"),
                base: PathBuf::from("/"),
                bin: PathBuf::new(),
                replace: None,
            },
            rx,
            since: Instant::now() - age,
            keys: KeysAt {
                input: 0,
                focus: crate::app::Focus::Editor,
                palette: false,
                dialog: false,
                active: 0,
            },
        });
        tx
    }
}

/// What to start once the probe answered.
struct Launch {
    line: Line,
    base: PathBuf,
    bin: PathBuf,
    /// The session id of the exited tab to reconnect in; `None`: a new tab.
    replace: Option<u64>,
}

struct Pending {
    launch: Launch,
    rx: Receiver<Probe>,
    since: Instant,
    /// Where the keys were when the session was asked for.
    keys: KeysAt,
}

/// Where the keys go at one moment. A session whose tab opens later takes the keys only when
/// this is unchanged: whatever the user typed meanwhile was meant for something else, and the
/// next keys must not land in ssh's password or host-key prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct KeysAt {
    input: u64,
    focus: crate::app::Focus,
    palette: bool,
    dialog: bool,
    active: usize,
}

impl KeysAt {
    fn now(app: &App) -> KeysAt {
        KeysAt {
            input: app.input_count,
            focus: app.focus,
            palette: app.palette.is_some(),
            dialog: app.dialog.is_some(),
            active: app.terminal.active,
        }
    }

    /// Nothing happened since, and nothing is open over the window.
    fn unchanged(&self, app: &App) -> bool {
        *self == KeysAt::now(app) && !self.palette && !self.dialog
    }
}

/// A probe is bounded by its timeout; one that has not answered this long after it (its
/// thread is stuck in a call that does not return, such as a `ControlPath` on a dead network
/// mount) is given up so it cannot block every later session.
const PROBE_GRACE: Duration = Duration::from_secs(2);

/// `SSH: Connect to Host`: the command window with the list of sessions and hosts.
fn open_list(app: &mut App, _: CmdArgs) -> CmdResult {
    super::workspace::open_slash(app, SLASH);
    Ok(())
}

/// Rows of the `ssh` list for the typed `arg`, as (value, label): what was typed (when it is a
/// valid line of its own), then the open sessions, the recently used lines and the config's
/// aliases, each line once.
pub fn rows(app: &App, arg: &str, hosts: &[String]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let add = |out: &mut Vec<(String, String)>, value: &str, note: &str| {
        if value.contains(arg) && !out.iter().any(|(v, _)| v == value) {
            // A stored line can hold any printable text (a remote command): shown made safe.
            let shown = crate::ui::safe_text::sanitize(value);
            out.push((value.to_string(), format!("{shown}  {note}")));
        }
    };
    for tab in &app.terminal.tabs {
        if let Some(s) = &tab.ssh
            && tab.running()
        {
            let note = if s.logged_in {
                "open session"
            } else {
                "open session, not logged in yet"
            };
            add(&mut out, s.line.text(), note);
        }
    }
    for e in &app.ssh.recent {
        add(&mut out, &e.text, "recent");
    }
    for h in hosts {
        add(&mut out, h, "ssh config");
    }
    if Line::parse(arg, app.ssh.home.as_deref()).is_ok() && !out.iter().any(|(v, _)| v == arg) {
        let shown = crate::ui::safe_text::sanitize(arg);
        out.insert(0, (arg.to_string(), format!("{shown}  connect")));
    }
    out
}

/// A listed row's title cut to `cells` keeping the first option and the destination of its
/// line visible (SEC-5: a cut row never hides where it goes); `None` for a row that is not
/// an argument line.
pub fn fit_row(title: &str, cells: usize) -> Option<String> {
    let (value, note) = title.rsplit_once("  ")?;
    let note_w = ratatui::text::Span::raw(note).width();
    let cut = args::cut_middle(value, cells.checked_sub(note_w + 2)?)?;
    Some(format!("{cut}  {note}"))
}

/// What the command window shows under the `/ssh` list for the line Enter would run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detail {
    /// `runs: ssh …` (wrapped by the window), or the local-command hint, then `in <folder>`
    /// when the line runs somewhere else than the active file's folder.
    pub text: Vec<String>,
    /// The hint: drawn as a warning.
    pub warn: bool,
    /// The destination as the command shows it: a cut command keeps it visible.
    pub dest: String,
}

/// The line a row (or what is typed, `row` = `None`) stands for in the `/ssh` window, with
/// the folder it brings. A session or recent line brings its folder only when it is picked:
/// highlighted over a different typed text, or put in the input from the list and not
/// edited since. A line typed by hand runs in the active file's folder even when it equals
/// a listed line (SEC-4, SEC-5).
fn line_of(app: &App, row: Option<&str>) -> Option<(String, Option<PathBuf>)> {
    let p = app.palette.as_ref()?;
    let input = p.input.text();
    let typed = input.strip_prefix(SLASH)?;
    match row
        .and_then(|r| r.strip_prefix(SLASH))
        .filter(|v| !v.trim().is_empty())
    {
        Some(v) if v.trim() != typed.trim() => Some((v.to_string(), stored_base(app, v))),
        _ => {
            let filled = p
                .ssh_listed
                .as_ref()
                .filter(|(t, _)| t == input)
                .map(|(_, b)| b.clone());
            Some((typed.to_string(), filled))
        }
    }
}

/// The line Enter would run now and the folder it brings (see [`line_of`]).
fn highlighted(app: &App) -> Option<(String, Option<PathBuf>)> {
    let p = app.palette.as_ref()?;
    let rows = super::slash::rows(app, p.input.text());
    line_of(app, rows.get(p.selected).map(|r| r.fill.as_str()))
}

/// The highlighted line when it brings a folder of its own: what the window hands to
/// [`connect`] through [`SshState::picked`].
pub fn picked(app: &App) -> Option<(String, PathBuf)> {
    let (text, base) = highlighted(app)?;
    Some((text, base?))
}

/// The folder an open session or a recent line with this text runs in.
fn stored_base(app: &App, text: &str) -> Option<PathBuf> {
    let text = text.trim_matches(' ');
    let session = app.terminal.tabs.iter().find_map(|t| {
        t.ssh
            .as_ref()
            .filter(|s| t.running() && s.line.text() == text)
            .map(|s| s.base.clone())
    });
    session.or_else(|| {
        app.ssh
            .recent
            .iter()
            .find(|e| e.text == text)
            .and_then(|e| e.base.clone())
    })
}

/// The workspace folder of the active file, else the first folder: where a typed line runs
/// (SEC-4). One rule for what is shown and what runs; reads nothing from the disk, so a
/// missing folder is shown and then refused, never swapped for another.
fn active_base(app: &App) -> PathBuf {
    let file = app.active_doc().and_then(|d| d.path.as_deref());
    file.and_then(|f| app.workspace.root_of(f))
        .unwrap_or_else(|| app.workspace.primary())
        .path
        .clone()
}

/// The detail of the `/ssh` window (UI spec S5): for the highlighted row (or what is typed),
/// the exact command with its relative paths shown resolved, or the hint that the line runs a
/// local program. Reads nothing from the disk: drawn every frame.
pub fn palette_detail(app: &App) -> Option<Detail> {
    let (value, base) = highlighted(app)?;
    detail_for(app, &value, base)
}

/// The detail of every listed row (and of what is typed): the window keeps the rows the
/// longest one needs, so arrowing never resizes it (D323).
pub fn palette_details(app: &App) -> Vec<Detail> {
    let Some(p) = app.palette.as_ref() else {
        return Vec::new();
    };
    let rows = super::slash::rows(app, p.input.text());
    std::iter::once(None)
        .chain(rows.iter().map(|r| Some(r.fill.as_str())))
        .filter_map(|r| line_of(app, r))
        .filter_map(|(value, base)| detail_for(app, &value, base))
        .collect()
}

fn detail_for(app: &App, value: &str, base: Option<PathBuf>) -> Option<Detail> {
    let line = Line::parse(value, app.ssh.home.as_deref()).ok()?;
    let active = active_base(app);
    let base = base.unwrap_or_else(|| active.clone());
    let dest = crate::ui::safe_text::sanitize(&args::quote(line.target().as_str()));
    let (mut text, warn) = match line.local_command() {
        Some(name) => {
            let first = if name == format!("-{}", args::CONFIG_FILE) {
                "Uses its own ssh config file (-F):".to_string()
            } else {
                format!("Has a local-command option ({name}):")
            };
            (vec![first, "check it; enter runs it".to_string()], true)
        }
        None => (
            vec![format!(
                "runs: {}",
                crate::ui::safe_text::sanitize(&args::shown_command(line.words(), &base))
            )],
            false,
        ),
    };
    if base != active {
        text.push(format!(
            "in {}",
            crate::ui::safe_text::sanitize(&base.to_string_lossy())
        ));
    }
    Some(Detail { text, warn, dest })
}

/// The folder the line `text` (as typed after `/ssh `) would run in, without touching the
/// disk: for display, and the folder Tab completion lists relative paths from (SEC-4).
pub fn shown_base(app: &App, text: &str) -> PathBuf {
    match picked(app) {
        Some((t, b)) if t.trim() == text.trim() => b,
        _ => active_base(app),
    }
}

/// Where ssh runs for `line` (SEC-4, SEC-5): the folder a picked session or recent line
/// brings (`picked`), else the workspace folder of the active file, made canonical. A
/// missing folder refuses the start.
fn base_for(app: &App, picked: Option<PathBuf>) -> Result<PathBuf, String> {
    match picked {
        Some(b) => args::canonical_base(&b).ok_or_else(|| RECENT_BASE_GONE.to_string()),
        None => args::canonical_base(&active_base(app)).ok_or_else(|| BASE_GONE.to_string()),
    }
}

/// `/ssh <argument line>`: the open session with the same line and folder gets the keys (full
/// screen again when it is logged in); otherwise a new session starts. Err: nothing was
/// started (the text for the command window).
pub fn connect(app: &mut App, text: &str) -> Result<(), String> {
    // Refused input is never echoed: it may be a password typed into the wrong place.
    let line = Line::parse(text, app.ssh.home.as_deref()).map_err(|e| e.notice().to_string())?;
    // The folder the window handed over for exactly this line, if any; taken once.
    let picked = app
        .ssh
        .picked
        .take()
        .filter(|(t, _)| t.trim_matches(' ') == line.text())
        .map(|(_, b)| b);
    let base = base_for(app, picked)?;
    let open = app.terminal.tabs.iter().position(|t| {
        t.running()
            && t.ssh
                .as_ref()
                .is_some_and(|s| s.line.text() == line.text() && s.base == base)
    });
    if let Some(i) = open {
        terminal::select_tab(app, i);
        terminal::enter_full(app);
        return Ok(());
    }
    start(app, line, base, None)
}

/// Enter on an exited session: the same line again, in the same folder, from the start
/// (probe, new nonce), in the same tab.
pub fn reconnect(app: &mut App, i: usize) {
    let Some(session) = app.terminal.tabs.get(i).and_then(|t| t.ssh.clone()) else {
        return;
    };
    if let Err(e) = start(app, session.line, session.base, Some(session.id)) {
        app.warn(e);
    }
}

fn live_sessions(app: &App) -> usize {
    app.terminal
        .tabs
        .iter()
        .filter(|t| t.ssh.is_some() && t.running())
        .count()
}

/// The `PATH` ssh and its probe get: the one a terminal tab gets (the user's `terminal_env`
/// value, else vr-ide's own) without empty and relative entries (SEC-4).
fn session_path(app: &App) -> OsString {
    let from_settings = app
        .terminal
        .env
        .iter()
        .rev()
        .find(|(k, _)| k == "PATH")
        .map(|(_, v)| v.clone());
    let path = from_settings
        .or_else(|| std::env::var_os("PATH"))
        .unwrap_or_default();
    ssh::child_path(&path)
}

fn start(app: &mut App, line: Line, base: PathBuf, replace: Option<u64>) -> Result<(), String> {
    if let Some(p) = &app.ssh.pending {
        if p.since.elapsed() < app.ssh.probe_timeout + PROBE_GRACE {
            return Err("SSH: another session is still starting; try again in a moment".into());
        }
        let stuck = app
            .ssh
            .pending
            .take()
            .map(|p| p.launch.line.target().clone());
        if let Some(target) = stuck {
            app.warn(format!(
                "SSH {target}: reading the ssh configuration did not finish; not started"
            ));
        }
    }
    if live_sessions(app) >= MAX_SESSIONS {
        return Err(format!(
            "SSH: {MAX_SESSIONS} sessions are open; close one first"
        ));
    }
    if !base.is_dir() {
        return Err(BASE_GONE.to_string());
    }
    let bin = app
        .ssh
        .bin
        .clone()
        .or_else(|| ssh::find_in_path(&std::env::var_os("PATH")?, "ssh"))
        .ok_or_else(|| "SSH: no ssh program was found on PATH".to_string())?;
    let launch = Launch {
        line,
        base,
        bin,
        replace,
    };
    let keys = KeysAt::now(app);
    // Without full screen there is no marker to add, and with the user's own LocalCommand or
    // connection-sharing words none may be added: the configuration need not be asked (and
    // `ssh -G` not run) at all.
    if !app.settings.ssh_fullscreen || launch.line.keeps_own_options() {
        open(app, launch, None, keys);
        return Ok(());
    }
    let (tx, rx) = mpsc::channel();
    let (bin, line, cwd, path, timeout) = (
        launch.bin.clone(),
        launch.line.clone(),
        launch.base.clone(),
        session_path(app),
        app.ssh.probe_timeout,
    );
    let wake = app.terminal.wake_handle();
    let started = std::thread::Builder::new()
        .name("vr-ssh-probe".into())
        .spawn(move || {
            let _ = tx.send(ssh::probe(&bin, &line, &cwd, &path, timeout));
            wake.store(true, Ordering::Release);
        });
    match started {
        Ok(_) => {
            app.ssh.pending = Some(Pending {
                launch,
                rx,
                since: Instant::now(),
                keys,
            });
        }
        Err(_) => open(app, launch, Some(Probe::Failed), keys),
    }
    Ok(())
}

/// Looks at what happened in the background since the last call: a finished probe starts its
/// session; a login marker makes its tab logged in (and full screen when that tab has the
/// keys); a login or a clean exit puts the line in the recent list. True when something
/// changed on screen.
pub fn poll(app: &mut App) -> bool {
    let mut changed = false;
    if let Some(p) = &app.ssh.pending {
        let probe = match p.rx.try_recv() {
            Ok(probe) => Some(probe),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Probe::Failed),
        };
        if let (Some(probe), Some(p)) = (probe, app.ssh.pending.take_if(|_| probe.is_some())) {
            open(app, p.launch, Some(probe), p.keys);
            changed = true;
        }
    }
    let mut logged_in = Vec::new();
    let mut used = Vec::new();
    for (i, tab) in app.terminal.tabs.iter_mut().enumerate() {
        let (seen, status) = (tab.marker_seen(), tab.status());
        let Some(s) = &mut tab.ssh else {
            continue;
        };
        if s.marked && !s.logged_in && seen {
            s.logged_in = true;
            logged_in.push(i);
        }
        if !s.recorded && (s.logged_in || status == vr_term::TermStatus::Exited(0)) {
            s.recorded = true;
            used.push((s.line.text().to_string(), s.base.clone()));
        }
    }
    for i in logged_in {
        changed = true;
        // Only the session the user is looking at takes the window: keys typed elsewhere
        // must not suddenly go to a remote shell.
        if i == app.terminal.active && terminal::has_focus(app) && app.dialog.is_none() {
            terminal::enter_full(app);
        } else if let Some(s) = &app.terminal.tabs[i].ssh {
            app.info(format!(
                "SSH {}: logged in; choose its tab for full screen",
                s.target
            ));
        }
    }
    for (text, base) in used {
        record(app, &text, base);
    }
    let was_full = app.terminal.full;
    terminal::sync_full(app);
    changed | (was_full != app.terminal.full)
}

/// Puts the line first in the recent list, with the folder it ran in (and in its file, when
/// there is one).
fn record(app: &mut App, text: &str, base: PathBuf) {
    let Some(entry) = Entry::new(text, Some(base)) else {
        return;
    };
    let list = &mut app.ssh.recent;
    list.retain(|e| e.text != entry.text);
    list.insert(0, entry.clone());
    list.truncate(ssh::recent::MAX);
    if let Some(file) = &app.ssh.recent_file {
        match ssh::recent::record(file, &entry) {
            Ok(list) => app.ssh.recent = list,
            Err(e) => tracing::debug!(error = %e, "ssh recent list not written"),
        }
    }
}

/// Inherited variables ssh must not get: vr-ide's own (SEC-S15).
fn withheld_env() -> Vec<OsString> {
    std::env::vars_os()
        .map(|(k, _)| k)
        .filter(|k| k.as_encoded_bytes().starts_with(b"VR_IDE_"))
        .collect()
}

/// Starts ssh for `launch` in a tab. `probe`: what the user's configuration said; `None` when
/// nothing was asked (full screen switched off, or the line keeps its own options). `keys`:
/// where the keys were when the session was asked for.
fn open(app: &mut App, launch: Launch, probe: Option<Probe>, keys: KeysAt) {
    let Launch {
        line,
        base,
        bin,
        replace,
    } = launch;
    let target = line.target().clone();
    // A fresh nonce for every start; without one (or without a clean probe) the session is a
    // plain tab: `PermitLocalCommand=yes` is never passed without vr-ide's own command.
    let nonce = match probe {
        Some(Probe::Clean { .. }) => Nonce::new(),
        _ => None,
    };
    let why_plain = match probe {
        None if app.settings.ssh_fullscreen && line.keeps_own_options() => Some(
            "your ssh options set LocalCommand or connection sharing (Control…, -M -S -O -G -V -Q -f), so vr-ide adds none of its own: the session stays a panel tab",
        ),
        Some(Probe::UserLocalCommand) => Some(
            "your ssh config sets LocalCommand, so vr-ide adds none of its own: the session stays a panel tab",
        ),
        Some(Probe::Failed) => Some(
            "the ssh configuration could not be read (ssh -G failed or took too long): the session stays a panel tab",
        ),
        Some(Probe::Clean { shared: true }) => Some(
            "a shared connection (ControlPath) exists; a session that uses it stays a panel tab",
        ),
        Some(Probe::Clean { shared: false }) if nonce.is_none() => {
            Some("no random bytes from the system: the session stays a panel tab")
        }
        _ => None,
    };
    let local_command = nonce.as_ref().map(Nonce::local_command);
    let argv = ssh::argv(&line, local_command.as_deref());
    let extras = SpawnExtras {
        marker: nonce.as_ref().map(Nonce::marker),
        env_remove: withheld_env(),
    };
    let env = vec![(OsString::from("PATH"), session_path(app))];
    let output = Arc::new(AtomicBool::new(false));
    let term = match terminal::spawn_program_env(app, &output, &bin, argv, &base, extras, env) {
        Ok(term) => term,
        Err(e) => {
            app.error(format!("SSH: {e}"));
            return;
        }
    };
    let slot = replace.and_then(|id| {
        app.terminal
            .tabs
            .iter()
            .position(|t| t.ssh.as_ref().is_some_and(|s| s.id == id) && !t.running())
    });
    let id = match (replace, slot) {
        (Some(id), Some(_)) => id,
        _ => {
            app.ssh.next_id += 1;
            app.ssh.next_id - 1
        }
    };
    let session = Session {
        id,
        target: target.clone(),
        line,
        base,
        logged_in: false,
        marked: nonce.is_some(),
        recorded: false,
    };
    match slot {
        Some(i) => {
            terminal::replace_term(app, i, term, bin, output);
            app.terminal.tabs[i].ssh = Some(session);
        }
        None if keys.unchanged(app) => {
            terminal::push_tab(app, term, bin, output, Some(session), true);
            // The prompts of ssh are answered in the tab: show it, with the keys.
            terminal::reveal_panel(app);
        }
        None => {
            // The user went on elsewhere while the probe ran: the tab waits to be chosen.
            terminal::push_tab(app, term, bin, output, Some(session), false);
            terminal::show_panel(app);
            app.info(format!("SSH {target} ready — click its tab"));
        }
    }
    if let Some(why) = why_plain {
        app.info(format!("SSH {target}: {why}"));
    }
    app.dirty = true;
}
