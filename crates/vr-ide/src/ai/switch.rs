//! Auto-switch (AC-6, AC-7): a claude session whose screen shows the limit message is moved to
//! the next account that works, in the same tab, on the same conversation.
//!
//! One switch is a small state machine kept on the session and advanced on every loop pass
//! ([`tick`], after [`limits::check`]); nothing here blocks the loop:
//!
//! ```text
//! limit event ──on_limit──▶ Ending ──old run exited──▶ Starting ──input box ready──▶ done
//!   (pick B, hold B,          │  5 s: SIGKILL            │  login screen: B not working,
//!    hang up A)               │  +2 s: given up          │    pick C, hang up B ─▶ Ending
//!                             ▼                          │  dialog / 30 s: "type to continue"
//!                       start failed: tab exited,        ▼
//!                       no hold left                 prompt typed once
//! ```
//!
//! - **Order (SEC-28).** The target is chosen, its history link checked, its hold taken and its
//!   folder checked before the old run is touched; a target failing any of these is skipped. The
//!   old run's process group is hung up and must have exited before the new one is started (the
//!   two would write one transcript): after [`END_WAIT`] the group is killed, and when it is
//!   still there [`KILL_WAIT`] later the switch is given up with the target's hold dropped. The
//!   old account's hold is released only once its run has exited; `account` and `account_lock`
//!   change together.
//! - **Arguments (SEC-24, SEC-31).** The new run gets the tool's `args`, `--resume` and the
//!   session's own id as separate arguments ([`spawn::args_by_id`], which checks the id again);
//!   never the continue arguments, which would pick the folder's latest conversation.
//! - **No loops (SEC-29).** One switch per limit event; each account at most once per event; at
//!   most [`SWITCH_MAX`] starts per session in [`SWITCH_WINDOW`], then auto-switch turns itself
//!   off for the session; a limit within [`SETTLE`] of a switch counts only once the new run sent
//!   a validated report; an exited session is never started by this module.
//! - **Continue prompt (SEC-30).** Written to the switched session's own terminal (the state
//!   lives on the session and names the new run's pid), once, only when its screen has been
//!   quiet for a second and shows the tool's idle input box and no dialog, and only when no key,
//!   paste or mouse report reached the session since the limit. Otherwise nothing is typed; a
//!   dialog, or no input box within [`PROMPT_WAIT`], gives a notice instead.
//!
//! Only claude switches here: its id comes through the usage report (T3). Codex is T7.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use super::accounts::AccountLock;
use super::limits::{self, AccountState, Screen, Tool, Zone};
use super::spawn::{self, StartMode};
use super::{AiSession, accounts, usage};
use crate::app::App;

/// How long a hung-up run gets to end before its process group is killed.
pub const END_WAIT: Duration = Duration::from_secs(5);
/// How long a killed run gets to be gone before the switch is given up.
pub const KILL_WAIT: Duration = Duration::from_secs(2);
/// How long the new run gets to show its input box.
pub const PROMPT_WAIT: Duration = Duration::from_secs(30);
/// Starts by a switch allowed per session in [`SWITCH_WINDOW`].
pub const SWITCH_MAX: usize = 3;
pub const SWITCH_WINDOW: Duration = Duration::from_secs(30 * 60);
/// A limit this soon after a switch is a new event only once the new run reported.
pub const SETTLE: Duration = Duration::from_secs(60);
/// Follows the tab's label while auto-switch is off for a session that could switch.
pub const OFF_MARK: &str = " ⊘";

/// What a session keeps for its switches.
#[derive(Default)]
pub(crate) struct State {
    /// The current limit event was acted on (one switch per event).
    handled: bool,
    /// A key, paste or mouse report reached the session since its limit event.
    pub(crate) typed: bool,
    pending: Option<Pending>,
    /// When a switch started a run in this tab (SEC-29).
    times: Vec<Instant>,
    /// The run in the tab was started by id and has not reported yet: whether the tool found
    /// that conversation is not known.
    pub(crate) unconfirmed: bool,
}

impl State {
    /// A switch is under way: the tab's run is being replaced.
    pub(crate) fn busy(&self) -> bool {
        self.pending.is_some()
    }

    /// The old run is being ended (its limit event is the one being handled).
    fn ending(&self) -> bool {
        matches!(
            self.pending,
            Some(Pending {
                phase: Phase::Ending { .. },
                ..
            })
        )
    }
}

/// One switch under way.
struct Pending {
    /// The account being left, for the notice.
    from: Option<String>,
    /// Until when that account is limited, when known.
    until: Option<i64>,
    /// The conversation, a canonical UUID.
    id: String,
    /// Accounts already used or refused for this limit event, the first one included.
    tried: Vec<Option<String>>,
    phase: Phase,
}

enum Phase {
    /// The old run was hung up; `to` (held by `lock`) starts once it has exited.
    Ending {
        to: Option<String>,
        lock: Option<AccountLock>,
        since: Instant,
        killed: Option<Instant>,
    },
    /// The new run (`pid`) is starting; `read`: the output its screen was last read after.
    Starting {
        pid: Option<u32>,
        since: Instant,
        read: Option<Option<Instant>>,
    },
}

/// The account to try after `current`: the default one (`None`) then `all` in their order,
/// starting after `current` and wrapping; never `current`, one in `tried`, one not known to
/// work, or one limited beyond `now` (unix seconds). Whether its history is shared is checked
/// when it is taken ([`on_limit`]), with its hold.
pub fn next_account(
    current: Option<&str>,
    all: &[String],
    state: &BTreeMap<String, AccountState>,
    tool: &str,
    now: i64,
    tried: &[Option<String>],
) -> Option<Option<String>> {
    let order: Vec<Option<&str>> = std::iter::once(None)
        .chain(all.iter().map(|a| Some(a.as_str())))
        .collect();
    let at = order.iter().position(|a| *a == current).unwrap_or(0);
    (1..=order.len())
        .map(|k| order[(at + k) % order.len()])
        .find(|cand| {
            *cand != current
                && !tried.iter().any(|t| t.as_deref() == *cand)
                && limits::state_key(tool, *cand)
                    .and_then(|key| state.get(&key))
                    .is_some_and(|a| a.works && a.limited_until.is_none_or(|t| t <= now))
        })
        .map(|cand| cand.map(str::to_string))
}

/// The earliest time after `now` an account of `tool` stops being limited.
fn earliest_reset(
    all: &[String],
    state: &BTreeMap<String, AccountState>,
    tool: &str,
    now: i64,
) -> Option<i64> {
    std::iter::once(None)
        .chain(all.iter().map(|a| Some(a.as_str())))
        .filter_map(|a| limits::state_key(tool, a))
        .filter_map(|key| state.get(&key)?.limited_until)
        .filter(|t| *t > now)
        .min()
}

fn account_word(account: Option<&str>) -> &str {
    account.unwrap_or(accounts::DEFAULT)
}

/// ` ⊘` for a session that could switch and has auto-switch off, else nothing.
pub fn off_mark(s: &AiSession) -> &'static str {
    if !s.auto_switch && spawn::resume_words(&s.tool).is_some() {
        OFF_MARK
    } else {
        ""
    }
}

/// `/ai autoswitch on|off` for the active session.
pub fn command(app: &mut App, rest: &str) -> Result<(), String> {
    let on = match rest.trim() {
        "on" => true,
        "off" => false,
        _ => return Err("/ai autoswitch takes on or off".into()),
    };
    let s = app
        .ai
        .active_session_mut()
        .ok_or("/ai autoswitch: no AI session is active")?;
    s.auto_switch = on;
    if on {
        // Turned on by hand: the count that turned it off starts again.
        s.switch.times.clear();
    }
    let who = usage::account_name(&s.tool, s.account.as_deref());
    app.info(format!(
        "auto-switch {} for {who}",
        if on { "on" } else { "off" }
    ));
    app.dirty = true;
    Ok(())
}

/// Every loop pass, after the limit check: acts once on each new limit event and advances the
/// switches under way. True when something to draw changed.
pub(crate) fn tick(app: &mut App, now: Instant) -> bool {
    let mut changed = false;
    for i in 0..app.ai.sessions.len() {
        let s = &mut app.ai.sessions[i];
        if !s.limit_seen {
            s.switch.handled = false;
        } else if !s.switch.handled && !s.switch.ending() {
            // The new run hit a limit of its own while its start was watched: that start is
            // over, and the event is acted on now rather than used up.
            s.switch.pending = None;
            s.switch.handled = true;
            s.switch.typed = false;
            on_limit(app, i, now);
            changed = true;
            continue;
        }
        if app.ai.sessions[i].switch.pending.is_some() {
            changed |= advance(app, i, now);
        }
    }
    changed
}

/// Session `i` hit its limit (a new event): with auto-switch on, a known id and a usable next
/// account, its run is hung up and the switch begins; else a notice says why not, where the
/// limit notice does not already.
pub fn on_limit(app: &mut App, i: usize, now: Instant) {
    let Some(s) = app.ai.sessions.get_mut(i) else {
        return;
    };
    // A session without the shared history has nothing to resume elsewhere (its start said so).
    if !s.auto_switch
        || s.switch.pending.is_some()
        || !s.running()
        || !s.shared
        || spawn::resume_words(&s.tool).is_none()
    {
        return;
    }
    let tool = s.tool.clone();
    let from = s.account.clone();
    let who = usage::account_name(&tool, from.as_deref());
    let recent = s
        .switch
        .times
        .last()
        .is_some_and(|t| now.saturating_duration_since(*t) < SETTLE);
    if recent && !s.limit_watch.works_marked {
        app.warn(format!(
            "{who} hit its limit right after the switch; not switching again"
        ));
        return;
    }
    let Some(id) = s.session_id().map(str::to_string) else {
        app.info(format!(
            "{who}: session id not known; not switching accounts"
        ));
        return;
    };
    if !room(s, now) {
        turn_off(app, i);
        return;
    }
    let until = app
        .ai
        .state_dir
        .as_deref()
        .zip(limits::state_key(&tool, from.as_deref()))
        .and_then(|(state, key)| limits::load_state(state).get(&key)?.limited_until);
    let mut tried = vec![from.clone()];
    let Some((to, lock)) = choose(app, &tool, from.as_deref(), &mut tried) else {
        none_left(app, &tool);
        return;
    };
    let s = &mut app.ai.sessions[i];
    s.tab.hang_up();
    s.switch.pending = Some(Pending {
        from,
        until,
        id,
        tried,
        phase: Phase::Ending {
            to,
            lock,
            since: now,
            killed: None,
        },
    });
}

/// Another start by a switch fits the session's window (SEC-29).
fn room(s: &mut AiSession, now: Instant) -> bool {
    s.switch
        .times
        .retain(|t| now.saturating_duration_since(*t) < SWITCH_WINDOW);
    s.switch.times.len() < SWITCH_MAX
}

fn turn_off(app: &mut App, i: usize) {
    let s = &mut app.ai.sessions[i];
    s.auto_switch = false;
    let who = usage::account_name(&s.tool, s.account.as_deref());
    app.warn(format!(
        "{who}: {SWITCH_MAX} account switches in 30 minutes; auto-switch is off for this session"
    ));
}

/// The next account that can be used now, with its hold (SEC-28): its history is the shared
/// one, no window is removing it, its folder passes the account checks. One that fails goes
/// into `tried` like one that is taken.
fn choose(
    app: &mut App,
    tool: &str,
    current: Option<&str>,
    tried: &mut Vec<Option<String>>,
) -> Option<(Option<String>, Option<AccountLock>)> {
    let state = app.ai.state_dir.clone()?;
    let all = accounts::for_tool(&state, tool);
    let known = limits::load_state(&state);
    let now = usage::now_secs();
    loop {
        let cand = next_account(current, &all, &known, tool, now, tried)?;
        tried.push(cand.clone());
        // The link first: making it needs the account to itself (as a session start does).
        if !super::share_history(app, tool, cand.as_deref()) {
            continue;
        }
        let Some(name) = cand.as_deref() else {
            return Some((None, None));
        };
        let Ok(lock) = accounts::hold(&state, name) else {
            continue;
        };
        if accounts::ready(Some(&state), tool, name).is_ok() {
            return Some((cand, Some(lock)));
        }
    }
}

/// No account to switch to: the earliest reset when one is known. A tool with only its default
/// account says nothing more than its limit notice.
fn none_left(app: &mut App, tool: &str) {
    let Some(state) = app.ai.state_dir.clone() else {
        return;
    };
    let all = accounts::for_tool(&state, tool);
    if all.is_empty() {
        return;
    }
    let now = usage::now_secs();
    let text = match earliest_reset(&all, &limits::load_state(&state), tool, now) {
        Some(t) => format!(
            "all {tool} accounts are limited until {}",
            usage::reset_clock(t, now, Zone::local())
        ),
        None => format!("no other {tool} account to switch to"),
    };
    app.warn(text);
}

/// Moves the switch of session `i` on; true when something changed on screen.
fn advance(app: &mut App, i: usize, now: Instant) -> bool {
    let s = &mut app.ai.sessions[i];
    let Some(p) = &mut s.switch.pending else {
        return false;
    };
    let Phase::Ending { since, killed, .. } = &mut p.phase else {
        return watch(app, i, now);
    };
    if !s.tab.running() {
        start(app, i, now);
        return true;
    }
    match *killed {
        None if now.saturating_duration_since(*since) >= END_WAIT => {
            s.tab.force_kill();
            *killed = Some(now);
        }
        Some(at) if now.saturating_duration_since(at) >= KILL_WAIT => {
            // Dropping the state drops the target's hold; the session stays on its account.
            s.switch.pending = None;
            let who = usage::account_name(&s.tool, s.account.as_deref());
            app.warn(format!("{who} did not end; not switched"));
            return true;
        }
        _ => {}
    }
    false
}

/// The old run has exited: the target's run goes into the tab, on the same conversation.
fn start(app: &mut App, i: usize, now: Instant) {
    let s = &mut app.ai.sessions[i];
    let Some(mut p) = s.switch.pending.take() else {
        return;
    };
    let Phase::Ending { to, lock, .. } = p.phase else {
        return;
    };
    let (tool_name, root) = (s.tool.clone(), s.root.clone());
    // The tracker goes on with the new run (the old one's exit paused it).
    if let Some(c) = &s.changes {
        c.resume();
    }
    let started = super::find_tool(app, &tool_name).and_then(|tool| {
        super::start_program(app, &tool, to.as_deref(), spawn::Start::Id(&p.id), &root)
    });
    let s = &mut app.ai.sessions[i];
    let target = usage::account_name(&tool_name, to.as_deref());
    match started {
        Err(e) => {
            // Both holds go: the old run has ended, the new one never ran. No further attempt.
            s.account_lock = None;
            drop(lock);
            app.error(format!("AI: switch to {target} failed: {e}"));
        }
        Ok((t, program, output, usage_files)) => {
            // The run it replaces has exited, so nothing of it is left to end.
            s.tab.replace(t, program, output);
            s.account = to.clone();
            s.account_lock = lock;
            s.shared = true;
            s.mode = StartMode::Continue;
            // The windows and the warnings shown belong to the account left (D36).
            s.usage = None;
            s.high_warned = Default::default();
            s.usage_file = usage_files.as_ref().map(|f| f.report.clone());
            s.usage_files = usage_files;
            s.usage_poll = Default::default();
            s.limit_seen = false;
            s.limit_watch.restarted();
            s.started(spawn::Start::Id(&p.id));
            s.switch.handled = false;
            s.switch.times.push(now);
            let pid = s.tab.pid();
            let text = match p.until {
                Some(t) => format!(
                    "switched {tool_name} from {from} to {} ({from} limited until {})",
                    account_word(to.as_deref()),
                    usage::reset_clock(t, usage::now_secs(), Zone::local()),
                    from = account_word(p.from.as_deref()),
                ),
                None => format!(
                    "switched {tool_name} from {} to {}",
                    account_word(p.from.as_deref()),
                    account_word(to.as_deref())
                ),
            };
            p.phase = Phase::Starting {
                pid,
                since: now,
                read: None,
            };
            s.switch.pending = Some(p);
            // Not an info notice: the user may be away, and it must still be there after.
            app.warn(text);
        }
    }
    app.dirty = true;
}

/// The new run is starting: reads its screen once after each quiet second and types the
/// continue prompt, moves on from a login screen, or gives up with a notice.
fn watch(app: &mut App, i: usize, now: Instant) -> bool {
    let s = &mut app.ai.sessions[i];
    let Some(p) = &mut s.switch.pending else {
        return false;
    };
    let Phase::Starting { pid, since, read } = &mut p.phase else {
        return false;
    };
    // Exited, or another run took the tab: nothing is typed, nothing is started.
    if !s.tab.running() || s.tab.pid() != *pid {
        s.switch.pending = None;
        return false;
    }
    let who = usage::account_name(&s.tool, s.account.as_deref());
    if now.saturating_duration_since(*since) >= PROMPT_WAIT {
        let typed = s.switch.typed;
        s.switch.pending = None;
        if !typed {
            app.warn(waiting(&who));
        }
        return !typed;
    }
    let out = s.limit_watch.last_output();
    if !s.limit_watch.quiet(now)
        || *read == Some(out)
        || now.saturating_duration_since(*since) < limits::QUIET
    {
        return false;
    }
    let grid = s.tab.grid_now();
    if grid.scroll_offset != 0 {
        // Scrolled back: the rows are not the screen; read it again once back at the bottom.
        return false;
    }
    *read = Some(out);
    let rows: Vec<String> = (0..grid.rows).map(|r| grid.row_text(r)).collect();
    let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
    match start_screen(&s.tool, &rows, grid.cols) {
        Shown::Busy => false,
        Shown::Login => {
            next_after_login(app, i, now);
            true
        }
        Shown::Dialog => {
            let typed = s.switch.typed;
            s.switch.pending = None;
            if !typed {
                app.warn(waiting(&who));
            }
            !typed
        }
        Shown::Ready => {
            let typed = s.switch.typed;
            s.switch.pending = None;
            if typed {
                return false;
            }
            let text = app.settings.ai_auto_switch_prompt.clone();
            let fits = (1..=200).contains(&text.chars().count());
            if !fits || text.chars().any(char::is_control) {
                app.warn(waiting(&who));
                return true;
            }
            let s = &mut app.ai.sessions[i];
            let mut bytes = vr_term::encode_paste(&text, grid.modes);
            bytes.push(b'\r');
            // What follows on the screen is this run's answer, not the conversation's tail.
            s.limit_watch.typed_input(&bytes);
            if s.tab.write(&bytes).is_err() {
                app.warn(waiting(&who));
            }
            true
        }
    }
}

fn waiting(who: &str) -> String {
    format!("{who} is waiting: type to continue")
}

/// The new run shows its login screen: the account is not working (SEC-29); the next one is
/// taken and this run hung up, or the session stays on the login screen with a notice.
fn next_after_login(app: &mut App, i: usize, now: Instant) {
    let s = &mut app.ai.sessions[i];
    let (tool, at) = (s.tool.clone(), s.account.clone());
    let who = usage::account_name(&tool, at.as_deref());
    if let Some(state) = app.ai.state_dir.clone()
        && let Some(key) = limits::state_key(&tool, at.as_deref())
    {
        let _ = limits::update_state(&state, |m| {
            if let Some(a) = m.get_mut(&key) {
                a.works = false;
            }
        });
    }
    let s = &mut app.ai.sessions[i];
    let Some(mut p) = s.switch.pending.take() else {
        return;
    };
    if !room(s, now) {
        turn_off(app, i);
        return;
    }
    app.warn(format!("{who} is not logged in; trying the next account"));
    let Some((to, lock)) = choose(app, &tool, at.as_deref(), &mut p.tried) else {
        none_left(app, &tool);
        return;
    };
    let s = &mut app.ai.sessions[i];
    s.tab.hang_up();
    p.from = at;
    p.until = None;
    p.phase = Phase::Ending {
        to,
        lock,
        since: now,
        killed: None,
    };
    s.switch.pending = Some(p);
}

/// What a starting run's quiet screen shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shown {
    /// The tool's login screen.
    Login,
    /// A question that typed text and Enter would answer.
    Dialog,
    /// The idle input box, and no question.
    Ready,
    /// Neither yet.
    Busy,
}

/// Lines a claude dialog starts with: folder trust, login and first-run screens, the API key
/// question, the choice between a summary and the full conversation, a permission question,
/// the Bypass Permissions warning, MCP server and external import approvals, terminal setup.
/// Wording checked against the strings of claude 2.1.332; entries of older versions stay (an
/// extra entry only keeps the prompt from being typed).
const CLAUDE_DIALOGS: &[&str] = &[
    "Accessing workspace:",
    "Quick safety check",
    "Is this a project you created or one you trust?",
    "Do you trust the files in this folder?",
    "Yes, I trust",
    "Yes, proceed",
    "No, exit",
    "Security notes:",
    "WARNING: Claude Code running in Bypass Permissions mode",
    "In Bypass Permissions mode",
    "Yes, I accept",
    "New MCP server found in this project",
    "new MCP servers found in this project",
    "Allow external CLAUDE.md file imports?",
    "Use Claude Code's terminal setup?",
    "Do you want to ",
    "Select login method",
    "Choose the text style",
    "Let's get started",
    "Press Enter to continue",
    "Enter to confirm",
    "Detected a custom API key",
    "Resume from summary",
    "Resume full session",
    "Resume Session",
    "Browser didn't open?",
    "Paste code here",
];

/// A selected choice of a list (`❯ 1. Yes`): the tool waits for an answer.
fn is_choice(text: &str) -> bool {
    limits::is_prompt(text)
        && text
            .chars()
            .skip(1)
            .collect::<String>()
            .trim_start()
            .split_once('.')
            .is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// The words of a row a dialog line is matched on: without a box border or a title drawn into
/// it (`╭─ Accessing workspace: ─╮`), a choice mark (lists may hide their numbers) and a leading
/// count (`3 new MCP servers …`).
fn dialog_text(row: &str) -> &str {
    let text = limits::strip(row)
        .1
        .trim_start_matches(|c: char| ('\u{2500}'..='\u{257f}').contains(&c) || c.is_whitespace());
    let text = text
        .strip_prefix(limits::PROMPTS)
        .map_or(text, str::trim_start);
    match text.split_once(' ') {
        Some((n, rest)) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => rest,
        _ => text,
    }
}

/// Reads a starting run's screen (SEC-30): [`Shown::Ready`] needs the tool's own input box — a
/// prompt row right under a rule with a rule below it — among the last rows, and no line of a
/// dialog there. Only claude's screens are known; any other tool is never ready.
pub fn start_screen(tool: &str, rows: &[&str], cols: usize) -> Shown {
    let Some(kind @ Tool::Claude) = Tool::of(tool) else {
        return Shown::Busy;
    };
    let screen = Screen::new(rows, cols);
    if limits::login_in(kind, &screen, "") {
        return Shown::Login;
    }
    let first = screen.window_start(limits::WINDOW);
    let last: Vec<usize> = (first..rows.len()).filter(|&r| !screen.blank(r)).collect();
    let dialog = last.iter().any(|&r| {
        let text = limits::strip(rows[r]).1;
        let words = dialog_text(rows[r]);
        is_choice(text) || CLAUDE_DIALOGS.iter().any(|d| words.starts_with(d))
    });
    if dialog {
        return Shown::Dialog;
    }
    let boxed = |r: usize| {
        r > 0
            && limits::is_rule(rows[r - 1])
            && rows[r + 1..].iter().take(4).any(|row| limits::is_rule(row))
    };
    let ready = last
        .iter()
        .any(|&r| limits::is_prompt(limits::strip(rows[r]).1) && boxed(r));
    if ready { Shown::Ready } else { Shown::Busy }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known(entries: &[(&str, bool, Option<i64>)]) -> BTreeMap<String, AccountState> {
        entries
            .iter()
            .map(|(name, works, until)| {
                (
                    format!("claude/{name}"),
                    AccountState {
                        works: *works,
                        limited_until: *until,
                        sticky: false,
                    },
                )
            })
            .collect()
    }

    /// Default then the accounts in order, after the current one, wrapping; an account that is
    /// tried, not known to work, or limited now is left out.
    #[test]
    fn next_account_order_and_skips() {
        let all: Vec<String> = ["a", "b", "c"].map(String::from).to_vec();
        let now = 1_000;
        let state = known(&[
            ("default", true, None),
            ("a", true, None),
            ("b", true, None),
            ("c", true, None),
        ]);
        let next =
            |current: Option<&str>, state: &BTreeMap<String, AccountState>, tried: &[&str]| {
                let tried: Vec<Option<String>> = tried
                    .iter()
                    .map(|t| (*t != "default").then(|| t.to_string()))
                    .collect();
                next_account(current, &all, state, "claude", now, &tried)
                    .map(|a| a.unwrap_or_else(|| "default".into()))
            };
        assert_eq!(next(None, &state, &[]).as_deref(), Some("a"));
        assert_eq!(next(Some("a"), &state, &[]).as_deref(), Some("b"));
        assert_eq!(next(Some("c"), &state, &[]).as_deref(), Some("default"));
        assert_eq!(next(Some("b"), &state, &["c"]).as_deref(), Some("default"));
        assert_eq!(
            next(Some("b"), &state, &["c", "default"]).as_deref(),
            Some("a")
        );
        assert_eq!(next(Some("b"), &state, &["c", "default", "a"]), None);
        // An account removed meanwhile is not in the list: the order starts at the top.
        assert_eq!(next(Some("gone"), &state, &[]).as_deref(), Some("a"));
        let state = known(&[
            ("default", false, None),
            ("a", true, Some(now + 1)),
            ("b", true, Some(now)),
        ]);
        assert_eq!(
            next(Some("c"), &state, &[]).as_deref(),
            Some("b"),
            "not working, limited, never used (c is the current one)"
        );
        assert_eq!(next(Some("b"), &state, &[]), None);
        assert_eq!(next_account(None, &[], &state, "claude", now, &[]), None);
        // A tool without accounts has no state.
        assert_eq!(next_account(None, &all, &state, "gemini", now, &[]), None);
        assert_eq!(earliest_reset(&all, &state, "claude", now), Some(now + 1));
        assert_eq!(earliest_reset(&all, &state, "claude", now + 1), None);
    }

    fn shown(screen: &str) -> Shown {
        let rows: Vec<&str> = screen.lines().collect();
        start_screen("claude", &rows, 80)
    }

    /// SEC-30: ready is the idle input box and nothing that asks; a choice list, a trust or
    /// login question, or a prompt mark outside a box is not.
    #[test]
    fn only_the_idle_input_box_is_ready() {
        let rule = "────────────────────";
        let idle = format!("● Earlier answer.\n\n{rule}\n❯ \n{rule}\n  ? for shortcuts");
        assert_eq!(shown(&idle), Shown::Ready);
        let boxed = "╭──────────────╮\n│ >            │\n╰──────────────╯\n  ? for shortcuts";
        assert_eq!(shown(boxed), Shown::Ready);
        let hint = format!("{rule}\n❯ Try \"fix the tests\"\n{rule}");
        assert_eq!(shown(&hint), Shown::Ready);
        assert_eq!(shown("Resuming conversation…"), Shown::Busy);
        assert_eq!(shown("❯ \n"), Shown::Busy, "a mark without its box");
        assert_eq!(shown(&format!("{rule}\n❯ ")), Shown::Busy, "no rule below");
        let trust =
            " Do you trust the files in this folder?\n\n ❯ 1. Yes, proceed\n   2. No, exit\n";
        assert_eq!(shown(trust), Shown::Dialog);
        let choice = format!("{rule}\n❯ \n{rule}\n\n ❯ 2. No");
        assert_eq!(shown(&choice), Shown::Dialog, "a list under the box");
        let permission = format!(" Do you want to proceed?\n{rule}\n❯ \n{rule}");
        assert_eq!(shown(&permission), Shown::Dialog);
        let summary = "  Resume from summary (recommended)\n  Resume full session as-is";
        assert_eq!(shown(summary), Shown::Dialog);
        assert_eq!(shown(" Select login method:\n"), Shown::Login);
        // Dialogs of claude 2.1.332 (from its strings), each over an idle box and with no
        // numbered choice (some lists hide their numbers): still a dialog.
        for dialog in [
            " Quick safety check: Is this a project you created or one you trust?",
            " Accessing workspace:\n\n /home/me/project",
            "╭─ Accessing workspace: ─────────╮",
            " Security notes:\n  Claude can make mistakes.",
            " WARNING: Claude Code running in Bypass Permissions mode",
            " In Bypass Permissions mode, Claude Code will not ask for your approval",
            " ❯ No, exit\n   Yes, I accept",
            "   No, exit\n ❯ Yes, I accept",
            " Yes, I trust these settings",
            " Allow external CLAUDE.md file imports?",
            " Use Claude Code's terminal setup?",
            " New MCP server found in this project: db",
            " 3 new MCP servers found in this project",
        ] {
            let screen = format!("{dialog}\n{rule}\n❯ \n{rule}");
            assert_eq!(shown(&screen), Shown::Dialog, "{dialog}");
        }
        let rows: Vec<&str> = idle.lines().collect();
        assert_eq!(start_screen("codex", &rows, 80), Shown::Busy);
        assert_eq!(start_screen("gemini", &rows, 80), Shown::Busy);
    }
}
