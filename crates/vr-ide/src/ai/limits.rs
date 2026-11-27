//! Usage-limit and login screens, and what vr-ide remembers per account (AC-5, SEC-26, SEC-27).
//!
//! A **limit event** is the tool showing its own limit message: the screen is untrusted (the
//! agent, a file it prints, or the user can put any text there), so a hit needs every one of
//! these (SEC-26):
//! - a fixed per-tool message, matched against a whole line after only the tool's own markers
//!   (claude's `⎿` result elbow, codex's `■` error mark, a box border on both sides) — never a
//!   substring of a longer line;
//! - among the last [`WINDOW`] non-empty rows of the visible screen, outside the input box (the
//!   rows between the last two rules, a rounded box, or a prompt row and its continuation);
//! - the last thing above the input box: rows after it may only belong to its own block
//!   (indented under it); output after it means the tool went on;
//! - the session quiet for [`QUIET`] when the screen is read ([`check`]).
//!
//! The patterns come from the installed binaries' strings (never from running them):
//! - claude 2.1.332 (`~/.local/share/claude/versions/2.1.332`): `function q(e,n,o,d){… return
//!   `You've hit your ${e}${n}${g}`}` with `e` one of `WFe` = {`5-hour limit`, `weekly limit`,
//!   `Opus limit`, `Sonnet limit`, `Fable limit`} or `limit`, `usage limit`, `session limit`
//!   (`d.startsWith("You've hit your session limit")`); `n` = ` · resets ${uA(resetsAt,!0)}`
//!   where `JN` prints `3pm` / `3:30pm` within a day, `Dec 3, 3pm` beyond (year when it differs),
//!   then ` (<zone>)`; `g` = ` · progress saved`. Also `Usage limit reached` with ` · limit
//!   resets ${t}` (`Twn.stopped`). Not counted: `You've hit your fast limit` (fast mode pauses,
//!   the tool goes on), `Context limit reached`, `Usage limit reached · wrapping up` (still
//!   running).
//!   Spend and credits (same `q`): names `monthly spend limit`, `org's monthly spend limit`,
//!   `channel's monthly spend limit`, `channel's monthly usage limit`, `team's pooled group
//!   budget`, with ` · resets …`, ` · your <window> resets …` or ` · … to raise it` parts; the
//!   sentences `You've hit your monthly spend limit.` / `You're out of usage credits.` followed by
//!   ` See Usage in Settings`, ` Switch to another model…`, ` Run /usage-credits…` or ` /model to
//!   switch models.`; and `You're out of usage credits` with ` · ` parts. These, and a model's
//!   own limit, are **sticky**: a usage report's 5-hour/weekly windows do not show them, so a
//!   report under 100 % does not end them.
//!   Unverified: the layout of claude's limit screen (which row the message lands on, the marker
//!   before it) could not be captured without a real limit; the rules here assume the message
//!   is a transcript row (`⎿` result elbow or plain) above the input box.
//! - codex 0.144.1 (`@openai/codex-linux-x64/vendor/x86_64-unknown-linux-musl/bin/codex`):
//!   `You've hit your usage limit.` followed by `Upgrade to Plus …`, `Upgrade to Pro …`, `Visit
//!   … to purchase more credits`, `To get more access now, send a request to your admin`, and
//!   ` Try again at ` / ` or try again at ` + `%-I:%M %p` (today) or `%b %-d<st|nd|rd|th>, %Y
//!   %-I:%M %p`; `You've hit your usage limit for …`; `Usage limit reached. You've reached your
//!   usage limit. …`.
//! - login: claude `Select login method:`; codex `Sign in with ChatGPT to use Codex as part of
//!   your paid plan` and `Finish signing in via your browser` (each a whole message, wrapped rows
//!   joined).
//!
//! Other tools have no verified patterns and never hit.
//!
//! A reset time on screen is local time: read in the zone vr-ide (and so the tool) runs in — `TZ`,
//! else `/etc/localtime` — with that zone's offset at the reset instant (summer time). A time
//! claude shows with a zone other than ours, or any time when our zone cannot be read, is not
//! believed (the report's reset is used instead).
//!
//! Per account, `<state>/ai-account-state` (JSON `{"claude/work": {"works": true,
//! "limited_until": <unix s>, "sticky": true}}`) keeps whether the account is known to work and until when it is
//! limited (SEC-27): 0600, written through a temp file and a rename, read without following a
//! link, at most [`STATE_MAX`] bytes, changed only under an exclusive lock on
//! `<state>/ai-account-state.lock`.

use std::collections::BTreeMap;
use std::io::{self, Read};
use std::os::fd::OwnedFd;
use std::path::Path;
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

use super::accounts;
use super::usage::{self, Usage, civil_from_days, days_from_civil};
use crate::app::App;
use crate::ui::ai_changes::parse_tzif;

pub const STATE_FILE: &str = "ai-account-state";
pub const STATE_LOCK: &str = "ai-account-state.lock";
/// A larger state file is not ours and is ignored.
pub const STATE_MAX: u64 = 64 * 1024;
/// The session must have printed nothing for this long before its screen is read.
pub const QUIET: Duration = Duration::from_secs(1);
/// At most one limit notice per session per this time.
pub const NOTICE_GAP: Duration = Duration::from_secs(60);
/// A reset time further out than this (seconds) is not believed.
pub const RESET_MAX: i64 = 8 * 86_400;
/// Only this many non-empty rows from the bottom are looked at.
pub const WINDOW: usize = 12;
/// A message is joined from at most this many wrapped rows.
const JOIN_MAX: usize = 4;
/// Cells a tool may leave free at the right edge when it wraps.
const WRAP_SLACK: usize = 4;
/// A clock time on screen that passed less than this long ago (seconds) is the limit that just
/// ended, not tomorrow's.
const PAST_GRACE: i64 = 3600;
/// Characters of the user's latest input kept to tell an echo of it from the tool's message.
pub(crate) const TYPED_MAX: usize = 2048;
/// A zone file larger than this is not one.
const ZONE_MAX: u64 = 256 * 1024;

/// The tool showed its limit message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LimitHit {
    /// When it said the limit resets, within [now, now + 8 days].
    pub resets_at: Option<i64>,
    /// Not a limit the usage report's 5-hour/weekly windows show (a model's own limit, a spend
    /// cap, credits): a report under 100 % does not end it.
    pub sticky: bool,
}

/// What is known of one account (`tool/account`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AccountState {
    /// A validated report with a session id came from the tool running on it.
    pub works: bool,
    /// Its last limit, until the reset it showed (≤ now + 8 days).
    pub limited_until: Option<i64>,
    /// That limit is [`LimitHit::sticky`]: it lasts until `limited_until` whatever a report says.
    pub sticky: bool,
}

/// What a line of the tool's limit message says.
struct Found<'a> {
    reset: Option<&'a str>,
    sticky: bool,
}

// ---- screens ------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub(super) enum Tool {
    Claude,
    Codex,
}

impl Tool {
    pub(super) fn of(name: &str) -> Option<Tool> {
        match name {
            "claude" => Some(Tool::Claude),
            "codex" => Some(Tool::Codex),
            _ => None,
        }
    }

    /// What `text` says when it is wholly one of the tool's limit messages.
    fn limit(self, text: &str) -> Option<Found<'_>> {
        match self {
            Tool::Claude => claude_limit(text),
            Tool::Codex => codex_limit(text),
        }
    }

    fn login(self, text: &str) -> bool {
        match self {
            Tool::Claude => text == "Select login method:",
            Tool::Codex => {
                text == "Sign in with ChatGPT to use Codex as part of your paid plan"
                    || text == "Finish signing in via your browser"
            }
        }
    }
}

const CLAUDE_HEAD: &str = "You've hit your ";
/// Limits a usage report's windows show.
const CLAUDE_WINDOWS: &[&str] = &[
    "5-hour limit",
    "weekly limit",
    "session limit",
    "usage limit",
    "limit",
];
/// Limits it does not: a model's own, spend caps, a pooled budget.
const CLAUDE_STICKY: &[&str] = &[
    "Opus limit",
    "Sonnet limit",
    "Fable limit",
    "monthly spend limit",
    "org's monthly spend limit",
    "channel's monthly spend limit",
    "channel's monthly usage limit",
    "team's pooled group budget",
];
const CLAUDE_CREDITS: &str = "You're out of usage credits";
/// What claude says after the full stop of a spend or credits sentence.
const CLAUDE_SENTENCES: &[&str] = &[
    "See Usage in Settings",
    "Switch to another model",
    "Run /usage-credits",
    "/model to switch models",
];
const CLAUDE_REACHED: &str = "Usage limit reached";
/// claude's separator between the parts of one message.
const SEP: &str = " \u{b7} ";

fn claude_limit(text: &str) -> Option<Found<'_>> {
    let parts_or_stop = |t: &str| t.is_empty() || t.starts_with(SEP) || t.starts_with('.');
    let (tail, sticky) = if let Some(rest) = text.strip_prefix(CLAUDE_HEAD) {
        let names = CLAUDE_WINDOWS
            .iter()
            .map(|n| (n, false))
            .chain(CLAUDE_STICKY.iter().map(|n| (n, true)));
        names
            .filter_map(|(n, sticky)| Some((rest.strip_prefix(n)?, sticky)))
            .find(|(t, _)| parts_or_stop(t))?
    } else if let Some(rest) = text.strip_prefix(CLAUDE_CREDITS) {
        (rest, true)
    } else {
        let tail = text.strip_prefix(CLAUDE_REACHED)?;
        // Still finishing the turn: the tool goes on.
        if tail.starts_with(&format!("{SEP}wrapping up"))
            || tail.starts_with(&format!("{SEP}brief"))
        {
            return None;
        }
        (tail, false)
    };
    if let Some(after) = tail.strip_prefix('.') {
        // Only spend and credits come as a sentence, followed by one of claude's own.
        let next = after.strip_prefix(' ').unwrap_or(after);
        let own = after.is_empty()
            || (after.starts_with(' ') && CLAUDE_SENTENCES.iter().any(|s| next.starts_with(s)));
        return (sticky && own).then_some(Found {
            reset: None,
            sticky,
        });
    }
    if !tail.is_empty() && !tail.starts_with(SEP) {
        return None;
    }
    let reset = tail.split(SEP).find_map(|part| {
        part.strip_prefix("resets ")
            .or_else(|| part.strip_prefix("limit resets "))
            .or_else(|| {
                // ` · your 5-hour limit resets 3pm` after a spend limit.
                let (_, at) = part.strip_prefix("your ")?.split_once(" resets ")?;
                Some(at)
            })
    });
    // claude's reset part is a time and nothing else.
    if reset.is_some_and(|t| local_time(t, 0, &Zone::utc()).is_none()) {
        return None;
    }
    Some(Found { reset, sticky })
}

const CODEX_HEADS: &[&str] = &[
    "You've hit your usage limit.",
    "Usage limit reached. You've reached your usage limit.",
];
/// A limit of one model, which the usage report does not show.
const CODEX_MODEL: &str = "You've hit your usage limit for ";

fn codex_limit(text: &str) -> Option<Found<'_>> {
    let sticky = text.starts_with(CODEX_MODEL);
    if !sticky && !CODEX_HEADS.iter().any(|h| text.starts_with(h)) {
        return None;
    }
    const AT: &str = "ry again at ";
    let reset = text.find(AT).map(|at| {
        let rest = &text[at + AT.len()..];
        rest.split(". ").next().unwrap_or(rest)
    });
    Some(Found { reset, sticky })
}

const RULES: &[char] = &['─', '━', '═', '╌', '┄'];
const MARKS: &[char] = &['⎿', '■'];
pub(super) const PROMPTS: &[char] = &['>', '❯', '›'];

/// A horizontal rule, or the top / bottom of a rounded or square box.
pub(super) fn is_rule(row: &str) -> bool {
    let t = row.trim();
    let inner = t
        .strip_prefix(['╭', '╰', '┌', '└'])
        .and_then(|r| r.strip_suffix(['╮', '╯', '┐', '┘']))
        .unwrap_or(t);
    inner.chars().count() >= 3 && inner.chars().all(|c| RULES.contains(&c))
}

/// The text of a row without the tool's markers, and the column it starts at (characters).
pub(super) fn strip(row: &str) -> (usize, &str) {
    let mut col = indent(row);
    let mut t = row.trim();
    // A box border counts only on both sides: a bar on the left alone is a quote.
    if let Some(inner) = t
        .strip_prefix(['│', '┃'])
        .and_then(|r| r.strip_suffix(['│', '┃']))
    {
        let rest = inner.trim_start();
        col += 1 + inner.chars().count() - rest.chars().count();
        t = rest.trim_end();
    }
    if let Some(rest) = t.strip_prefix(MARKS) {
        let text = rest.trim_start();
        col += 1 + rest.chars().count() - text.chars().count();
        t = text;
    }
    (col, t)
}

fn indent(row: &str) -> usize {
    row.chars().take_while(|c| c.is_whitespace()).count()
}

pub(super) fn is_prompt(text: &str) -> bool {
    text.strip_prefix(PROMPTS)
        .is_some_and(|r| r.is_empty() || r.starts_with(' '))
}

/// The rows of one screen and which of them are the input box.
pub(super) struct Screen<'a> {
    pub(super) rows: &'a [&'a str],
    cols: usize,
    input: Vec<bool>,
}

impl<'a> Screen<'a> {
    pub(super) fn new(rows: &'a [&'a str], cols: usize) -> Screen<'a> {
        let mut input = vec![false; rows.len()];
        let rules: Vec<usize> = (0..rows.len()).filter(|&r| is_rule(rows[r])).collect();
        if let [.., top, bottom] = rules[..] {
            input[top..=bottom].iter_mut().for_each(|x| *x = true);
        }
        let mut r = 0;
        while r < rows.len() {
            let (col, text) = strip(rows[r]);
            if is_prompt(text) {
                input[r] = true;
                r += 1;
                // What the user typed over several rows.
                while r < rows.len() && !rows[r].trim().is_empty() && indent(rows[r]) > col {
                    input[r] = true;
                    r += 1;
                }
                continue;
            }
            r += 1;
        }
        Screen { rows, cols, input }
    }

    pub(super) fn blank(&self, r: usize) -> bool {
        self.rows[r].trim().is_empty()
    }

    /// The first row of the last `n` non-empty ones.
    pub(super) fn window_start(&self, n: usize) -> usize {
        let mut seen = 0;
        for r in (0..self.rows.len()).rev() {
            if !self.blank(r) {
                seen += 1;
                if seen == n {
                    return r;
                }
            }
        }
        0
    }

    /// Row `next` carries on row `prev`'s text: the tool broke it because the next word did not
    /// fit, not because a new line began.
    fn wraps(&self, prev: usize, next: usize, col: usize) -> bool {
        if self.blank(next) || self.input[next] || indent(self.rows[next]) < col {
            return false;
        }
        let (col_next, text) = strip(self.rows[next]);
        if col_next != indent(self.rows[next]) {
            // It starts with a marker of its own: a new message.
            return false;
        }
        let word = text.split(' ').next().unwrap_or("").chars().count();
        let used = self.rows[prev].trim_end().chars().count();
        used + 1 + word > self.cols.saturating_sub(WRAP_SLACK)
    }

    /// The message starting at row `r`: its column, its text joined over wrapped rows, and its
    /// last row.
    fn message(&self, r: usize) -> (usize, String, usize) {
        let (col, first) = strip(self.rows[r]);
        let mut text = first.to_string();
        let mut last = r;
        while last + 1 < self.rows.len()
            && last + 1 - r < JOIN_MAX
            && self.wraps(last, last + 1, col)
        {
            last += 1;
            text.push(' ');
            text.push_str(self.rows[last].trim());
        }
        (col, text, last)
    }

    /// Nothing but blank rows or rows indented under column `col` from `after` down to the input
    /// box (or the end of the screen).
    fn last_content(&self, after: usize, col: usize) -> bool {
        (after + 1..self.rows.len())
            .take_while(|&r| !self.input[r])
            .all(|r| self.blank(r) || indent(self.rows[r]) >= col.max(1))
    }
}

/// The limit message on `screen` (rows joined by `\n`, width unknown), checked against the
/// current time.
pub fn limit_hit(tool: &str, screen: &str) -> Option<LimitHit> {
    let rows: Vec<&str> = screen.lines().collect();
    // The width is not known: no row is taken for a wrap of the one above.
    let cols = usize::MAX;
    limit_hit_at(tool, &rows, cols, usage::now_secs(), Zone::local())
}

/// [`limit_hit`] on a screen `cols` wide at `now` (unix seconds), screen times read in `zone`.
pub fn limit_hit_at(
    tool: &str,
    rows: &[&str],
    cols: usize,
    now: i64,
    zone: &Zone,
) -> Option<LimitHit> {
    find_limit(Tool::of(tool)?, &Screen::new(rows, cols), now, zone).map(|(hit, _)| hit)
}

/// The limit message on `screen` and its text (markers stripped, wrapped rows joined).
fn find_limit(
    kind: Tool,
    screen: &Screen<'_>,
    now: i64,
    zone: &Zone,
) -> Option<(LimitHit, String)> {
    let first = screen.window_start(WINDOW);
    for r in (first..screen.rows.len()).rev() {
        if screen.blank(r) || screen.input[r] {
            continue;
        }
        let (col, text, last) = screen.message(r);
        let Some(found) = kind.limit(&text) else {
            continue;
        };
        if !screen.last_content(last, col) {
            return None;
        }
        let hit = LimitHit {
            resets_at: found.reset.and_then(|t| parse_when(t, now, zone)),
            sticky: found.sticky,
        };
        return Some((hit, text));
    }
    None
}

/// A login screen instead of the tool's prompt, under the limit line's rules (SEC-26).
pub fn login_screen(tool: &str, screen: &str) -> bool {
    let Some(kind) = Tool::of(tool) else {
        return false;
    };
    let rows: Vec<&str> = screen.lines().collect();
    // The width is not known: no row is taken for a wrap of the one above.
    let cols = usize::MAX;
    login_in(kind, &Screen::new(&rows, cols), "")
}

/// The tool's login message as a whole line among the last [`WINDOW`] rows, the last content
/// above the input box, and not something the user `typed`. The typed prompt row keeps its
/// marker, so it never equals the message; a login screen may be drawn in a box, so box rows are
/// not left out.
pub(super) fn login_in(kind: Tool, screen: &Screen<'_>, typed: &str) -> bool {
    let first = screen.window_start(WINDOW);
    (first..screen.rows.len())
        .rev()
        .filter(|&r| !screen.blank(r))
        .find_map(|r| {
            let (col, text, last) = screen.message(r);
            kind.login(&text).then_some((col, text, last))
        })
        .is_some_and(|(col, text, last)| screen.last_content(last, col) && !typed_has(typed, &text))
}

/// `text` (whitespace aside) is part of what the user typed.
fn typed_has(typed: &str, text: &str) -> bool {
    let words = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let text = words(text);
    !text.is_empty() && words(typed).contains(&text)
}

// ---- time zone ------------------------------------------------------------------------------------

/// The zone local times are read in.
#[derive(Debug, Clone, Default)]
pub struct Zone {
    /// Its name as the tools print it (`Europe/Berlin`), when known.
    pub name: Option<String>,
    /// (transition time, offset after it), oldest first; empty is UTC.
    offsets: Vec<(i64, i64)>,
    /// The zone was read: without it no local time on screen is believed.
    pub known: bool,
}

impl Zone {
    pub fn fixed(name: &str, offset: i64) -> Zone {
        Zone::with_offsets(name, vec![(i64::MIN, offset)])
    }

    pub fn with_offsets(name: &str, offsets: Vec<(i64, i64)>) -> Zone {
        Zone {
            name: Some(name.to_string()),
            offsets,
            known: true,
        }
    }

    fn utc() -> Zone {
        Zone::fixed("UTC", 0)
    }

    pub fn unknown() -> Zone {
        Zone::default()
    }

    /// The zone vr-ide runs in (`TZ`, else `/etc/localtime`), read once; UTC under test.
    pub fn local() -> &'static Zone {
        static LOCAL: std::sync::OnceLock<Zone> = std::sync::OnceLock::new();
        LOCAL.get_or_init(|| {
            if cfg!(test) {
                return Zone::utc();
            }
            load_zone(std::env::var("TZ").ok().as_deref(), Path::new("/"))
        })
    }

    /// Seconds east of UTC at the instant `utc`.
    pub fn offset(&self, utc: i64) -> i64 {
        self.offsets
            .iter()
            .rev()
            .find(|(at, _)| *at <= utc)
            .or(self.offsets.first())
            .map_or(0, |(_, off)| *off)
    }

    /// The instant a local wall time names, with the offset in force then (summer time).
    fn to_utc(&self, local: i64) -> i64 {
        let guess = local - self.offset(local);
        local - self.offset(guess)
    }

    /// `shown` (a zone printed beside a time) is this zone.
    pub fn is(&self, shown: &str) -> bool {
        let norm = |n: &str| {
            let n = n.strip_prefix("Etc/").unwrap_or(n);
            match n {
                "UCT" | "Universal" | "Zulu" => "UTC".to_string(),
                n => n.to_string(),
            }
        };
        self.name.as_deref().is_some_and(|n| norm(n) == norm(shown))
    }
}

/// A zone name that may name a file under the zoneinfo folder: no absolute path, no `..`, no
/// POSIX rule string.
fn valid_zone_name(name: &str) -> bool {
    name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_-+".contains(&b))
        && name
            .split('/')
            .all(|c| !c.is_empty() && !c.starts_with('.'))
}

/// The zone the C library would use with `TZ` = `tz`, files read under `root`.
fn load_zone(tz: Option<&str>, root: &Path) -> Zone {
    let read = |p: &Path| -> Option<Vec<(i64, i64)>> {
        let file = std::fs::File::open(p).ok()?;
        if !file.metadata().ok()?.is_file() {
            return None;
        }
        let mut bytes = Vec::new();
        file.take(ZONE_MAX).read_to_end(&mut bytes).ok()?;
        parse_tzif(&bytes)
    };
    let zoneinfo = root.join("usr/share/zoneinfo");
    match tz.map(|t| t.strip_prefix(':').unwrap_or(t)) {
        Some("") => Zone::utc(),
        Some(name) if valid_zone_name(name) => match read(&zoneinfo.join(name)) {
            Some(offsets) => Zone::with_offsets(name, offsets),
            None => Zone::unknown(),
        },
        Some(_) => Zone::unknown(),
        None => {
            let link = root.join("etc/localtime");
            if link.symlink_metadata().is_err() {
                // No zone file: the C library runs in UTC.
                return Zone::utc();
            }
            let Some(offsets) = read(&link) else {
                return Zone::unknown();
            };
            let from_link = std::fs::read_link(&link).ok().and_then(|t| {
                let t = t.to_str()?;
                let (_, name) = t.split_once("zoneinfo/")?;
                let name = name
                    .strip_prefix("posix/")
                    .or_else(|| name.strip_prefix("right/"))
                    .unwrap_or(name);
                Some(name.to_string())
            });
            let name = from_link
                .or_else(|| {
                    let text = std::fs::read_to_string(root.join("etc/timezone")).ok()?;
                    Some(text.trim().to_string())
                })
                .filter(|n| valid_zone_name(n));
            Zone {
                name,
                offsets,
                known: true,
            }
        }
    }
}

// ---- reset times ----------------------------------------------------------------------------------

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// `3pm`, `3:30pm`, `3:00 PM`, `15:00` → seconds into the day.
fn clock(s: &str) -> Option<i64> {
    let s: String = s
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    let (num, half) = match s.strip_suffix("am").or_else(|| s.strip_suffix("pm")) {
        Some(n) => (n, Some(s.ends_with("pm"))),
        None => (s.as_str(), None),
    };
    let (h, m) = match num.split_once(':') {
        Some((h, m)) if m.len() == 2 => (h, m),
        Some(_) => return None,
        None if half.is_some() => (num, "00"),
        None => return None,
    };
    let digits = |x: &str| !x.is_empty() && x.len() <= 2 && x.bytes().all(|b| b.is_ascii_digit());
    if !digits(h) || !digits(m) {
        return None;
    }
    let (mut h, m): (i64, i64) = (h.parse().ok()?, m.parse().ok()?);
    if m > 59 {
        return None;
    }
    match half {
        Some(pm) => {
            if !(1..=12).contains(&h) {
                return None;
            }
            h %= 12;
            if pm {
                h += 12;
            }
        }
        None if h > 23 => return None,
        None => {}
    }
    Some(h * 3600 + m * 60)
}

fn day_of(s: &str) -> Option<i64> {
    let s = s.trim_end_matches(',');
    let s = ["st", "nd", "rd", "th"]
        .iter()
        .find_map(|x| s.strip_suffix(x))
        .unwrap_or(s);
    let d: i64 = (!s.is_empty() && s.len() <= 2 && s.bytes().all(|b| b.is_ascii_digit()))
        .then(|| s.parse().ok())??;
    (1..=31).contains(&d).then_some(d)
}

/// A time and the zone printed after it: `3pm (Europe/Berlin)` → (`3pm`, `Europe/Berlin`).
fn split_zone(text: &str) -> (&str, Option<&str>) {
    let mut s = text.trim().trim_end_matches('.').trim();
    s = s.strip_prefix("at ").unwrap_or(s);
    if let Some(inner) = s.strip_suffix(')')
        && let Some((time, zone)) = inner.rsplit_once(" (")
    {
        return (time.trim_end(), Some(zone));
    }
    (s, None)
}

/// A reset time as claude or codex prints it (`3pm`, `at 3:30pm (Europe/Berlin)`, `Dec 3, 3pm`,
/// `Dec 3rd, 2026 3:00 PM.`), as a local time in `zone`, as unix seconds; `None` unless it falls
/// within [now, now + 8 days], is printed in our zone (or without one) and our zone is known.
pub fn parse_when(text: &str, now: i64, zone: &Zone) -> Option<i64> {
    if !zone.known || split_zone(text).1.is_some_and(|shown| !zone.is(shown)) {
        return None;
    }
    let unix = local_time(text, now, zone)?;
    (now..=now + RESET_MAX).contains(&unix).then_some(unix)
}

/// [`parse_when`] without the range and zone checks: the next such time from `now` (a clock
/// time that passed less than [`PAST_GRACE`] ago stays today's).
fn local_time(text: &str, now: i64, zone: &Zone) -> Option<i64> {
    let s = split_zone(text).0.replace(['\u{202f}', '\u{a0}'], " ");
    let words: Vec<&str> = s.split_whitespace().collect();
    let local_now = now + zone.offset(now);
    let month = words.first().and_then(|w| {
        let w = w.to_ascii_lowercase();
        // `get`: a word of non-ASCII letters has no character boundary at byte 3.
        let head = w.get(..3)?;
        MONTHS
            .iter()
            .position(|m| m.starts_with(head) && w.len() <= 9)
    });
    let local = match month {
        Some(m) => {
            let day = day_of(words.get(1)?)?;
            let (mut rest, mut year) = (&words[2..], None);
            if let Some(y) = rest.first().map(|w| w.trim_end_matches(','))
                && y.len() == 4
                && y.bytes().all(|b| b.is_ascii_digit())
            {
                year = Some(y.parse::<i64>().ok()?);
                rest = &rest[1..];
            }
            let secs = clock(&rest.join(" "))?;
            let month = m as i64 + 1;
            let this_year = civil_from_days(local_now.div_euclid(86_400)).0;
            let at = |y: i64| days_from_civil(y, month, day) * 86_400 + secs;
            match year {
                Some(y) => at(y),
                // Without a year a date well behind us is next year's.
                None if at(this_year) < local_now - 86_400 => at(this_year + 1),
                None => at(this_year),
            }
        }
        None => {
            let secs = clock(&s)?;
            let today = local_now - local_now.rem_euclid(86_400) + secs;
            if local_now - today >= PAST_GRACE {
                today + 86_400
            } else {
                today
            }
        }
    };
    Some(zone.to_utc(local))
}

// ---- account state ------------------------------------------------------------------------------

/// `tool/account` for a tool with accounts and a valid account name (`default` for none).
pub fn state_key(tool: &str, account: Option<&str>) -> Option<String> {
    accounts::var_for(tool)?;
    let account = match account {
        None => accounts::DEFAULT,
        Some(a) => accounts::validate(a).ok()?,
    };
    Some(format!("{tool}/{account}"))
}

fn valid_key(key: &str) -> bool {
    key.split_once('/').is_some_and(|(tool, account)| {
        let account = (account != accounts::DEFAULT).then_some(account);
        state_key(tool, account).as_deref() == Some(key)
    })
}

/// Drops bad keys, past or empty entries, and clamps `limited_until` to now + 8 days.
fn clean(map: &mut BTreeMap<String, AccountState>, now: i64) {
    map.retain(|k, v| {
        v.limited_until = v
            .limited_until
            .filter(|t| *t > now)
            .map(|t| t.min(now + RESET_MAX));
        v.sticky &= v.limited_until.is_some();
        valid_key(k) && *v != AccountState::default()
    });
}

fn parse_state(bytes: &[u8], now: i64) -> BTreeMap<String, AccountState> {
    let Ok(Value::Object(o)) = serde_json::from_slice::<Value>(bytes) else {
        return BTreeMap::new();
    };
    let mut map: BTreeMap<String, AccountState> = o
        .iter()
        .filter_map(|(k, v)| {
            let v = v.as_object()?;
            Some((
                k.clone(),
                AccountState {
                    works: v.get("works").and_then(Value::as_bool).unwrap_or(false),
                    limited_until: v.get("limited_until").and_then(Value::as_i64),
                    sticky: v.get("sticky").and_then(Value::as_bool).unwrap_or(false),
                },
            ))
        })
        .collect();
    clean(&mut map, now);
    map
}

/// The state as stored: a missing file is empty, so are garbage and an oversize file (read and
/// found not ours); a link, a FIFO or any read error is an error, so nobody writes over it.
fn read_strict(state: &Path, now: i64) -> io::Result<BTreeMap<String, AccountState>> {
    use rustix::fs::{Mode, OFlags};
    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
    let fd = match rustix::fs::open(state.join(STATE_FILE), flags, Mode::empty()) {
        Err(rustix::io::Errno::NOENT) => return Ok(BTreeMap::new()),
        r => r?,
    };
    let file = std::fs::File::from(fd);
    if !file.metadata()?.is_file() {
        return Err(io::Error::other("not a file"));
    }
    let mut bytes = Vec::new();
    file.take(STATE_MAX + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > STATE_MAX {
        return Ok(BTreeMap::new());
    }
    Ok(parse_state(&bytes, now))
}

/// What every window knows of the accounts; empty when unreadable.
pub fn load_state(state: &Path) -> BTreeMap<String, AccountState> {
    read_strict(state, usage::now_secs()).unwrap_or_default()
}

/// Held while the state is read, changed and written.
struct StateLock(OwnedFd);

impl Drop for StateLock {
    fn drop(&mut self) {
        accounts::unlock(&self.0);
    }
}

fn lock(state: &Path) -> io::Result<StateLock> {
    use rustix::fs::{FlockOperation, Mode, OFlags};
    let flags = OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let fd = rustix::fs::open(state.join(STATE_LOCK), flags, Mode::RUSR | Mode::WUSR)?;
    rustix::fs::flock(&fd, FlockOperation::LockExclusive)?;
    Ok(StateLock(fd))
}

fn write(state: &Path, _lock: &StateLock, map: &BTreeMap<String, AccountState>) -> io::Result<()> {
    use rustix::fs::{Mode, OFlags};
    let body: Map<String, Value> = map
        .iter()
        .map(|(k, v)| {
            let mut o = Map::new();
            o.insert("works".into(), Value::Bool(v.works));
            if let Some(t) = v.limited_until {
                o.insert("limited_until".into(), Value::from(t));
            }
            if v.sticky {
                o.insert("sticky".into(), Value::Bool(true));
            }
            (k.clone(), Value::Object(o))
        })
        .collect();
    let body = serde_json::to_vec(&Value::Object(body)).map_err(io::Error::other)?;
    if body.len() as u64 > STATE_MAX {
        return Err(io::Error::other("too large"));
    }
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC;
    let dir = rustix::fs::open(state, flags, Mode::empty())?;
    usage::write_at(&dir, STATE_FILE, &body)
}

/// Replaces the state with `map` (cleaned), under the lock.
pub fn save_state(state: &Path, map: &BTreeMap<String, AccountState>) -> io::Result<()> {
    let lock = lock(state)?;
    let mut map = map.clone();
    clean(&mut map, usage::now_secs());
    write(state, &lock, &map)
}

/// Reads, changes with `f` and writes the state under the lock, so windows changing different
/// accounts keep each other's changes. True when it wrote; nothing is written when the file
/// could not be read (a link, a FIFO, an I/O error).
pub fn update_state(
    state: &Path,
    f: impl FnOnce(&mut BTreeMap<String, AccountState>),
) -> io::Result<bool> {
    let lock = lock(state)?;
    let now = usage::now_secs();
    let mut map = read_strict(state, now)?;
    let before = map.clone();
    f(&mut map);
    clean(&mut map, now);
    if map == before && state.join(STATE_FILE).exists() {
        return Ok(false);
    }
    write(state, &lock, &map)?;
    Ok(true)
}

// ---- sessions -----------------------------------------------------------------------------------

/// What a session's limit watch remembers between loop passes.
#[derive(Default)]
pub struct Watch {
    /// The program printed since the last pass.
    output: bool,
    /// The pass that saw its last output.
    output_at: Option<Instant>,
    /// Its screen has not been read since that output.
    pending: bool,
    /// The user typed into it since the current limit event.
    pub(crate) typed: bool,
    notice_at: Option<Instant>,
    /// This run's first report with an id set `works`: the run works, and a login line on its
    /// screen no longer says otherwise.
    pub(crate) works_marked: bool,
    /// This run already cleared (or found no) `limited_until` from a report under 100 %.
    clear_checked: bool,
    /// This run's login screen was recorded.
    login_marked: bool,
    /// The latest of what the user typed this run (at most [`TYPED_MAX`] characters, lines split
    /// by `\n`): a program echoing it must not pass for the tool's message.
    pub(crate) typed_text: String,
    /// The run resumed an earlier conversation and shows its tail again: a limit line there is
    /// the earlier run's, not an event, until a line was sent to this run.
    pub(crate) resumed: bool,
    /// The run opened on the tool's own list of conversations: the first line sent picks one
    /// (its tail is shown after it), so it does not yet end [`Watch::resumed`].
    pub(crate) picking: bool,
}

/// Shows how much the user typed, never what: it may be a pasted secret.
impl std::fmt::Debug for Watch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Watch")
            .field("output", &self.output)
            .field("output_at", &self.output_at)
            .field("pending", &self.pending)
            .field("typed", &self.typed)
            .field("notice_at", &self.notice_at)
            .field("works_marked", &self.works_marked)
            .field("clear_checked", &self.clear_checked)
            .field("login_marked", &self.login_marked)
            .field("resumed", &self.resumed)
            .field("picking", &self.picking)
            .field(
                "typed_text",
                &format_args!("<{} chars>", self.typed_text.chars().count()),
            )
            .finish()
    }
}

impl Watch {
    /// The program printed (called from [`super::refresh`]).
    pub(crate) fn output(&mut self) {
        self.output = true;
    }

    /// The pass that saw the program's last output; `None`: it has not printed.
    pub(crate) fn last_output(&self) -> Option<Instant> {
        self.output_at
    }

    /// Nothing was printed for [`QUIET`] before `now` (as of the last [`check`]).
    pub(crate) fn quiet(&self, now: Instant) -> bool {
        !self.output
            && self
                .output_at
                .is_none_or(|t| now.saturating_duration_since(t) >= QUIET)
    }

    /// Keys or a paste sent to the program, as the bytes sent: its text is kept, escape
    /// sequences (arrows, paste markers, alt+key) left out, backspace applied.
    pub(crate) fn typed_input(&mut self, bytes: &[u8]) {
        self.typed = true;
        let text = String::from_utf8_lossy(bytes);
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            match c {
                '\u{1b}' => match chars.next() {
                    // CSI: parameters up to a final byte.
                    Some('[') => {
                        for c in chars.by_ref() {
                            if ('@'..='~').contains(&c) {
                                break;
                            }
                        }
                    }
                    // SS3: one more character.
                    Some('O') => {
                        chars.next();
                    }
                    _ => {}
                },
                '\u{7f}' | '\u{8}' => {
                    self.typed_text.pop();
                }
                '\r' | '\n' => {
                    let line = self.typed_text.rsplit('\n').next().unwrap_or("");
                    let empty = line.trim().is_empty();
                    self.typed_text.push('\n');
                    // A line was sent: what the tool prints next is this run's, unless the line
                    // picked the conversation in the tool's list. The tool ignores an empty
                    // line, so outside the list one is no line sent.
                    if self.picking {
                        self.picking = false;
                    } else if !empty {
                        self.resumed = false;
                    }
                }
                c if c.is_control() => {}
                c => self.typed_text.push(c),
            }
        }
        let n = self.typed_text.chars().count();
        if n > TYPED_MAX {
            let cut = self
                .typed_text
                .char_indices()
                .nth(n - TYPED_MAX)
                .map_or(0, |(i, _)| i);
            self.typed_text.drain(..cut);
        }
    }

    /// A new run in the tab: a new event can come; the notice rate stays.
    pub(crate) fn restarted(&mut self) {
        *self = Watch {
            notice_at: self.notice_at,
            ..Watch::default()
        };
    }
}

/// The latest reset of a window the report shows full, within [now, now + 8 days].
fn report_reset(u: Option<&Usage>, now: i64) -> Option<i64> {
    let u = u?;
    [u.five_hour, u.seven_day]
        .into_iter()
        .flatten()
        .filter(|w| w.used_percentage >= 100.0)
        .filter_map(|w| w.resets_at)
        .filter(|t| (now..=now + RESET_MAX).contains(t))
        .max()
}

/// Every known window of the report is under 100 % (and at least one is known).
fn below_full(u: &Usage) -> bool {
    let windows: Vec<f64> = [u.five_hour, u.seven_day]
        .into_iter()
        .flatten()
        .map(|w| w.used_percentage)
        .collect();
    !windows.is_empty() && windows.iter().all(|p| *p < 100.0)
}

/// What one read of a quiet screen found.
struct Seen {
    hit: Option<LimitHit>,
    login: bool,
}

/// Reads the screen of each session that has been quiet for [`QUIET`] since its last output
/// (once per quiet spell), and acts on a limit or login screen. Called on every loop pass with
/// its time. True when a notice was shown.
pub(crate) fn check(app: &mut App, now: Instant) -> bool {
    let mut seen = Vec::new();
    for (i, s) in app.ai.sessions.iter_mut().enumerate() {
        let w = &mut s.limit_watch;
        if std::mem::take(&mut w.output) {
            w.output_at = Some(now);
            w.pending = true;
        }
        let quiet = w
            .output_at
            .is_none_or(|t| now.saturating_duration_since(t) >= QUIET);
        if !w.pending || !quiet {
            continue;
        }
        w.pending = false;
        if !s.running() || Tool::of(&s.tool).is_none() {
            continue;
        }
        let grid = s.tab.grid_now();
        if grid.scroll_offset != 0 {
            // Scrolled back: the rows are not the screen; look again a while later.
            s.limit_watch.output_at = Some(now);
            s.limit_watch.pending = true;
            continue;
        }
        let Some(kind) = Tool::of(&s.tool) else {
            continue;
        };
        let rows: Vec<String> = (0..grid.rows).map(|r| grid.row_text(r)).collect();
        let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
        let screen = Screen::new(&rows, grid.cols);
        let typed = &s.limit_watch.typed_text;
        // What the user typed, echoed back, is never the tool's message (QC F1).
        let hit = find_limit(kind, &screen, usage::now_secs(), Zone::local())
            .filter(|(_, text)| !typed_has(typed, text))
            .map(|(hit, _)| hit);
        let login = login_in(kind, &screen, typed);
        seen.push((i, Seen { hit, login }));
    }
    let mut noticed = false;
    for (i, found) in seen {
        noticed |= act(app, i, found, now);
    }
    noticed
}

fn act(app: &mut App, i: usize, found: Seen, now: Instant) -> bool {
    let state = app.ai.state_dir.clone();
    let s = &mut app.ai.sessions[i];
    let key = state_key(&s.tool, s.account.as_deref());
    // A run whose own validated report carried an id works, whatever its screen shows.
    if found.login && !s.limit_watch.login_marked && !s.limit_watch.works_marked {
        s.limit_watch.login_marked = true;
        if let (Some(state), Some(key)) = (&state, &key) {
            let _ = update_state(state, |m| {
                if let Some(e) = m.get_mut(key) {
                    e.works = false;
                }
            });
        }
    }
    // A resumed conversation's tail, shown again, is not this run's limit.
    let Some(hit) = found.hit.filter(|_| !s.limit_watch.resumed) else {
        // The user's input was answered and the limit line is gone: a later one is a new event.
        if s.limit_seen && s.limit_watch.typed {
            s.limit_seen = false;
            s.limit_watch.typed = false;
        }
        return false;
    };
    if s.limit_seen {
        return false;
    }
    s.limit_seen = true;
    s.limit_watch.typed = false;
    s.limit_watch.clear_checked = false;
    let secs = usage::now_secs();
    let resets = hit
        .resets_at
        .or_else(|| report_reset(s.usage.as_ref(), secs));
    if let (Some(state), Some(key), Some(t)) = (&state, &key, resets) {
        let _ = update_state(state, |m| {
            record_limit(m.entry(key.clone()).or_default(), t, hit.sticky);
        });
    }
    let rated = s
        .limit_watch
        .notice_at
        .is_some_and(|t| now.saturating_duration_since(t) < NOTICE_GAP);
    if rated {
        return false;
    }
    s.limit_watch.notice_at = Some(now);
    let label = usage::account_name(&s.tool, s.account.as_deref());
    let text = match resets {
        Some(t) => format!(
            "{label} hit its usage limit (resets {})",
            usage::reset_clock(t, secs, Zone::local())
        ),
        None => format!("{label} hit its usage limit"),
    };
    app.warn(text);
    true
}

/// A new limit until `t` on an account entry: the later reset wins with its own kind, so a short
/// window limit never shortens a longer (sticky) one still in force.
fn record_limit(e: &mut AccountState, t: i64, sticky: bool) {
    match e.limited_until {
        Some(old) if old > t => {}
        Some(old) if old == t => e.sticky |= sticky,
        _ => {
            e.limited_until = Some(t);
            e.sticky = sticky;
        }
    }
}

/// A new validated report for session `i` (SEC-27): its first one with a session id marks the
/// account working; one with every window under 100 % ends the account's limit, unless this
/// session is at its limit now or the limit is sticky (not one the windows show). At most one
/// locked update per run for each.
pub(crate) fn on_report(app: &mut App, i: usize) {
    let Some(state) = app.ai.state_dir.clone() else {
        return;
    };
    let s = &mut app.ai.sessions[i];
    let (Some(key), Some(u)) = (state_key(&s.tool, s.account.as_deref()), &s.usage) else {
        return;
    };
    let works = u.session_id.is_some() && !s.limit_watch.works_marked;
    let clear = !s.limit_seen && !s.limit_watch.clear_checked && below_full(u);
    if !works && !clear {
        return;
    }
    s.limit_watch.works_marked |= works;
    s.limit_watch.clear_checked |= clear;
    let _ = update_state(&state, |m| {
        let e = m.entry(key).or_default();
        e.works |= works;
        if clear && !e.sticky {
            e.limited_until = None;
        }
    });
}

#[cfg(test)]
#[path = "limits_tests.rs"]
mod tests;
