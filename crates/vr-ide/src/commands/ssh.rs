//! SSH sessions in the terminal panel (FR-25, D317): the `ssh` entry of the command window,
//! starting the system `ssh` in a tab, the login event that gives a session the whole window,
//! reconnecting and the recent list. What is run and with which arguments is decided in
//! [`crate::ssh`]; this module only wires it to the panel.
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
use crate::ssh::{self, MAX_SESSIONS, NOT_VALID, Nonce, Probe, Target};

pub const COMMANDS: &[CommandSpec] = &[CommandSpec {
    name: "ssh.connect",
    title: "SSH: Connect to Host",
    run: open_list,
}];

/// The slash command the palette entry and a typed `ssh ` lead to.
pub const SLASH: &str = "/ssh ";

/// The SSH session of a terminal tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// Stays with the tab across reconnects.
    pub id: u64,
    /// Where it goes: the source of the tab label and the status row.
    pub target: Target,
    /// Its own login marker came through its own output (this start of ssh).
    pub logged_in: bool,
    /// ssh was started with vr-ide's `LocalCommand`, so a marker can come at all.
    marked: bool,
    /// The target is in the recent list for this start.
    recorded: bool,
}

/// SSH state of the app that belongs to no single tab.
pub struct SshState {
    /// Program run instead of the `ssh` found on `PATH` (tests).
    pub bin: Option<PathBuf>,
    /// The home folder whose `.ssh/config` lists hosts and where ssh starts; `None`: no list
    /// (tests), ssh starts in `/`.
    pub home: Option<PathBuf>,
    /// Where the recent list is kept; `None`: in memory only (tests).
    pub recent_file: Option<PathBuf>,
    /// Recently used targets, most recent first.
    pub recent: Vec<Target>,
    /// How long the configuration probe may take.
    pub probe_timeout: Duration,
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

    /// A probe for `target` that never answers, started `age` ago (tests). The returned sender
    /// keeps its channel open.
    #[cfg(test)]
    pub(crate) fn stall_probe(&mut self, target: Target, age: Duration) -> mpsc::Sender<Probe> {
        let (tx, rx) = mpsc::channel();
        self.pending = Some(Pending {
            launch: Launch {
                target,
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
    target: Target,
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
/// target of its own), then the open sessions, the recently used targets and the config's
/// aliases, each target once.
pub fn rows(app: &App, arg: &str, hosts: &[String]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let add = |out: &mut Vec<(String, String)>, value: &str, note: &str| {
        if value.contains(arg) && !out.iter().any(|(v, _)| v == value) {
            out.push((value.to_string(), format!("{value}  {note}")));
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
            add(&mut out, s.target.as_str(), note);
        }
    }
    for t in &app.ssh.recent {
        add(&mut out, t.as_str(), "recent");
    }
    for h in hosts {
        add(&mut out, h, "ssh config");
    }
    if Target::parse(arg).is_some() && !out.iter().any(|(v, _)| v == arg) {
        out.insert(0, (arg.to_string(), format!("{arg}  connect")));
    }
    out
}

/// `/ssh <target>`: the open session to that target gets the keys (full screen again when it
/// is logged in); otherwise a new session starts. Err: nothing was started (the text for the
/// command window).
pub fn connect(app: &mut App, text: &str) -> Result<(), String> {
    // Refused input is never echoed: it may be a password typed into the wrong place.
    let target = Target::parse(text).ok_or_else(|| NOT_VALID.to_string())?;
    let open = app
        .terminal
        .tabs
        .iter()
        .position(|t| t.running() && t.ssh.as_ref().is_some_and(|s| s.target == target));
    if let Some(i) = open {
        terminal::select_tab(app, i);
        terminal::enter_full(app);
        return Ok(());
    }
    start(app, target, None)
}

/// Enter on an exited session: the same validated target again, from the start (probe, new
/// nonce), in the same tab.
pub fn reconnect(app: &mut App, i: usize) {
    let Some(session) = app.terminal.tabs.get(i).and_then(|t| t.ssh.clone()) else {
        return;
    };
    if let Err(e) = start(app, session.target, Some(session.id)) {
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

fn start(app: &mut App, target: Target, replace: Option<u64>) -> Result<(), String> {
    if let Some(p) = &app.ssh.pending {
        if p.since.elapsed() < app.ssh.probe_timeout + PROBE_GRACE {
            return Err("SSH: another session is still starting; try again in a moment".into());
        }
        let stuck = app.ssh.pending.take().map(|p| p.launch.target);
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
    let bin = app
        .ssh
        .bin
        .clone()
        .or_else(|| ssh::find_in_path(&std::env::var_os("PATH")?, "ssh"))
        .ok_or_else(|| "SSH: no ssh program was found on PATH".to_string())?;
    let launch = Launch {
        target,
        bin,
        replace,
    };
    let keys = KeysAt::now(app);
    // Without full screen there is no marker to add, so the user's configuration need not be
    // asked (and `ssh -G` not run) at all.
    if !app.settings.ssh_fullscreen {
        open(app, launch, None, keys);
        return Ok(());
    }
    let (tx, rx) = mpsc::channel();
    let (bin, target, timeout) = (
        launch.bin.clone(),
        launch.target.clone(),
        app.ssh.probe_timeout,
    );
    let wake = app.terminal.wake_handle();
    let started = std::thread::Builder::new()
        .name("vr-ssh-probe".into())
        .spawn(move || {
            let _ = tx.send(ssh::probe(&bin, &target, timeout));
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
/// keys); a login or a clean exit puts the target in the recent list. True when something
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
            used.push(s.target.clone());
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
    for target in used {
        record(app, &target);
    }
    let was_full = app.terminal.full;
    terminal::sync_full(app);
    changed | (was_full != app.terminal.full)
}

/// Puts `target` first in the recent list (and its file, when there is one).
fn record(app: &mut App, target: &Target) {
    let list = &mut app.ssh.recent;
    list.retain(|t| t != target);
    list.insert(0, target.clone());
    list.truncate(ssh::recent::MAX);
    if let Some(file) = &app.ssh.recent_file {
        match ssh::recent::record(file, target) {
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
/// full screen is switched off and nothing was asked. `keys`: where the keys were when the
/// session was asked for.
fn open(app: &mut App, launch: Launch, probe: Option<Probe>, keys: KeysAt) {
    let Launch {
        target,
        bin,
        replace,
    } = launch;
    // A fresh nonce for every start; without one (or without a clean probe) the session is a
    // plain tab: `PermitLocalCommand=yes` is never passed without vr-ide's own command.
    let nonce = match probe {
        Some(Probe::Clean { .. }) => Nonce::new(),
        _ => None,
    };
    let why_plain = match probe {
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
    let args = ssh::argv(&target, local_command.as_deref());
    let extras = SpawnExtras {
        marker: nonce.as_ref().map(Nonce::marker),
        env_remove: withheld_env(),
        attention: false,
    };
    let cwd = app
        .ssh
        .home
        .clone()
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .filter(|h| h.is_dir())
        .unwrap_or_else(|| PathBuf::from("/"));
    let output = Arc::new(AtomicBool::new(false));
    let term = match terminal::spawn_program(app, &output, &bin, args, &cwd, extras) {
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
