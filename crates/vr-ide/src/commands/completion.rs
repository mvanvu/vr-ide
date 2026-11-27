//! Tab completion in the palette input (D6, AC-7, 02_ui-spec.md S5).
//!
//! Sources: after `/` the command names; path arguments (`/ws add|open|save`, `/save-as`,
//! `/ssh -i|-F|-E|-S|-o …File=`, `/run` file arguments); the `/ssh` destination (ssh config
//! hosts and recent targets); `/ai` tool names; the other slash arguments complete from the
//! values the slash list already offers.
//!
//! One candidate completes; several insert their common part (phase [`Phase::Common`]); a Tab
//! with nothing to add opens the list ([`Phase::List`]), where Tab / Shift+Tab step, Enter takes
//! the candidate (nothing runs), Esc restores the text from before the list and any edit
//! closes it.

use std::path::{Path, PathBuf};

use super::palette::{self, PaletteItem};
use super::slash::{self, ArgKind};
use crate::app::App;
use crate::ui::safe_text;

/// Paths offered at most for one Tab.
pub const SHOWN_LIMIT: usize = 100;

/// What a candidate is; folders are drawn in the folder colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandKind {
    Folder,
    File,
    Other,
}

/// One candidate: the whole input it puts in place, its row label and where it comes from
/// (drawn right-aligned: `recent`, `~/.ssh/config`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cand {
    pub fill: String,
    pub label: String,
    pub kind: CandKind,
    pub source: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// The common part of several candidates was inserted; the next Tab opens the list.
    Common,
    /// The list is open on this candidate.
    List(usize),
}

/// What the area under the list shows while a completion is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    /// The key hint (`tab next  shift+tab previous  enter take  esc back to …`).
    Keys,
    /// A fixed line (the folder `/ssh` paths are taken from).
    Text(String),
    /// `/ssh` destinations: the command the input runs (the `/ssh` window's own detail).
    SshRuns,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub cands: Vec<Cand>,
    pub phase: Phase,
    /// The input when the completion started (Esc in the list goes back to it).
    pub before: String,
    /// The typed word, for "esc back to …".
    pub back: String,
    pub note: Note,
    /// Completing a slash command's argument: while the common part shows, the list row is
    /// the command's own.
    pub in_arg: bool,
}

impl Completion {
    /// Right of the input: `tab: list (N)` or `i/N`.
    pub fn tag(&self) -> String {
        match self.phase {
            Phase::Common => format!("tab: list ({})", self.cands.len()),
            Phase::List(i) => format!("{}/{}", i + 1, self.cands.len()),
        }
    }

    /// A shorter tag for a narrow input (`tab (N)`); `None` when there is none.
    pub fn short_tag(&self) -> Option<String> {
        match self.phase {
            Phase::Common => Some(format!("tab ({})", self.cands.len())),
            _ => None,
        }
    }

    /// The text under the list, before wrapping (the ssh line is built by the renderer).
    pub fn detail(&self) -> String {
        match (self.phase, &self.note) {
            (Phase::Common, _) => {
                let names: Vec<String> = self.cands.iter().map(short_label).collect();
                format!("Common part completed: {}", names.join("  "))
            }
            (Phase::List(_), Note::Text(t)) => t.clone(),
            (Phase::List(_), _) => {
                let back = if self.back.is_empty() {
                    self.before.trim_end()
                } else {
                    self.back.as_str()
                };
                format!(
                    "tab next  shift+tab previous  enter take  esc back to {}",
                    safe_text::sanitize(back)
                )
            }
        }
    }
}

/// The last name of a path candidate (`fixecom/`), else the label.
fn short_label(c: &Cand) -> String {
    match c.kind {
        CandKind::Other => c.label.clone(),
        CandKind::Folder | CandKind::File => {
            let t = c.label.trim_end_matches('/');
            let name = t.rsplit('/').next().unwrap_or(t);
            let slash = if c.kind == CandKind::Folder { "/" } else { "" };
            format!("{name}{slash}")
        }
    }
}

/// Longest prefix every fill shares (whole chars).
pub fn common_prefix(fills: &[&str]) -> String {
    let Some(first) = fills.first() else {
        return String::new();
    };
    let mut end = first.len();
    for f in &fills[1..] {
        end = first[..end]
            .char_indices()
            .zip(f.chars())
            .find(|((_, a), b)| a != b)
            .map_or_else(|| end.min(f.len()), |((i, _), _)| i);
    }
    first[..end].to_string()
}

// ---- /ssh argument positions -------------------------------------------------------------------

/// Where the end of an `/ssh` argument line is, for completion (byte offsets into the line).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SshSlot {
    /// A path argument starts at this offset (`-i`, `-F`, `-E`, `-S`, `-o …File=`).
    Path(usize),
    /// The destination starts at this offset.
    Host(usize),
    /// Anything else: an option, its non-path value, the remote command.
    Other,
}

/// The position at the end of an `/ssh` argument line, read with the `/ssh` parser's own
/// grammar (`ssh::args::classify`), so completion and the command agree.
pub fn ssh_slot_at_end(line: &str) -> SshSlot {
    match crate::ssh::args::classify(line, line.len()) {
        // `-i` with nothing after it is the option still being typed, not its value yet.
        crate::ssh::args::ArgPos::Path { start }
            if start == line.len() && !line.ends_with([' ', '=', '"', '\'']) =>
        {
            SshSlot::Other
        }
        crate::ssh::args::ArgPos::Path { start } => SshSlot::Path(start),
        crate::ssh::args::ArgPos::Destination { start } => SshSlot::Host(start),
        crate::ssh::args::ArgPos::Other => SshSlot::Other,
    }
}

/// Words of a shell-like line with their byte start; `\ ` stays inside a word. A line that
/// ends in a space has an empty last word at its end (`/run` arguments).
fn words(line: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let mut escaped = false;
    for (i, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            start.get_or_insert(i);
        } else if ch.is_whitespace() {
            if let Some(s) = start.take() {
                out.push((s, &line[s..i]));
            }
        } else {
            start.get_or_insert(i);
        }
    }
    match start {
        Some(s) => out.push((s, &line[s..])),
        None => out.push((line.len(), "")),
    }
    out
}

// ---- words and paths -------------------------------------------------------------------------

/// Characters a shell-like split would read specially; escaped with `\` in a completed word.
const SPECIAL: &str = " \t\"'\\$`;&|()<>*?[]{}#!~";

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for (i, ch) in s.char_indices() {
        // A leading `~` is the user's home on purpose; elsewhere it is a plain character.
        if SPECIAL.contains(ch) && !(ch == '~' && i == 0) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            if let Some(n) = chars.next() {
                out.push(n);
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
}

/// `p` made absolute against `base`, `~` and `~/…` as the home folder.
fn resolve_from(base: &Path, p: &str) -> PathBuf {
    let expanded = match (p.strip_prefix('~'), home()) {
        (Some(rest), Some(h)) if rest.is_empty() || rest.starts_with('/') => {
            h.join(rest.trim_start_matches('/'))
        }
        _ => PathBuf::from(p),
    };
    crate::workspace::normalize(&base.join(expanded))
}

/// `~/…` for a path in `home`, sanitized for display.
fn tilde(p: &Path, home: Option<&Path>) -> String {
    let shown = match home.and_then(|h| p.strip_prefix(h).ok().map(Path::to_path_buf)) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~/{}", rest.display()),
        None => p.display().to_string(),
    };
    safe_text::sanitize(&shown)
}

/// Which entries a path argument offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Dirs,
    /// Folders and `.vr-workspace` files (`/ws open`).
    Workspaces,
}

/// How a completed path word is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Style {
    /// The whole rest of a one-argument command, as typed (`/ws add my dir/`).
    Plain,
    /// One word of a shell-like line: special characters escaped, a finished file gets a space.
    Word,
}

/// Path candidates for the typed `word` (relative to `base`): `(fill, label, kind)` where the
/// fill is `lead` + the completed word.
fn path_cands(
    app: &mut App,
    base: &Path,
    lead: &str,
    word: &str,
    filter: Filter,
    style: Style,
) -> Vec<Cand> {
    let typed = match style {
        Style::Plain => word.to_string(),
        Style::Word => unescape(word),
    };
    let (raw_dir, typed_dir, prefix) = match (word.rfind('/'), typed.rfind('/')) {
        (Some(i), Some(j)) => (&word[..=i], &typed[..=j], &typed[j + 1..]),
        _ => ("", "", typed.as_str()),
    };
    let dir = if typed_dir.is_empty() {
        base.to_path_buf()
    } else {
        resolve_from(base, typed_dir)
    };
    let entries = slash::cached_listing(app, &dir);
    entries
        .iter()
        .filter(|(name, is_dir)| {
            name.starts_with(prefix)
                && (!name.starts_with('.') || prefix.starts_with('.'))
                && !name.chars().any(safe_text::is_unsafe_char)
                && match filter {
                    Filter::All => true,
                    Filter::Dirs => *is_dir,
                    Filter::Workspaces => {
                        *is_dir || vr_config::is_workspace_file(Path::new(name.as_str()))
                    }
                }
        })
        .take(SHOWN_LIMIT)
        .map(|(name, is_dir)| {
            let slash = if *is_dir { "/" } else { "" };
            let (written, space) = match style {
                Style::Plain => (name.clone(), ""),
                Style::Word => (escape(name), if *is_dir { "" } else { " " }),
            };
            Cand {
                fill: format!("{lead}{raw_dir}{written}{slash}{space}"),
                label: safe_text::sanitize(&format!("{typed_dir}{name}{slash}")),
                kind: if *is_dir {
                    CandKind::Folder
                } else {
                    CandKind::File
                },
                source: "",
            }
        })
        .collect()
}

// ---- sources ---------------------------------------------------------------------------------

/// The candidates for an input, or `None` when the position completes nothing.
struct Found {
    cands: Vec<Cand>,
    back: String,
    note: Note,
    in_arg: bool,
}

/// The tools `/ai` completes: the names of the `ai_tools` setting, in its order, each once.
pub fn ai_tool_names(app: &App) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in &app.settings.ai_tools {
        if !t.name.chars().any(safe_text::is_unsafe_char) && !out.contains(&t.name) {
            out.push(t.name.clone());
        }
    }
    out
}

fn find(app: &mut App, input: &str) -> Option<Found> {
    slash::prepare(app, input);
    let Some((c, arg)) = slash::in_argument(input) else {
        let cands = slash::rows(app, input)
            .into_iter()
            .map(|r| Cand {
                fill: r.fill,
                label: r.title,
                kind: CandKind::Other,
                source: "",
            })
            .collect();
        return Some(Found {
            cands,
            back: input.to_string(),
            note: Note::Keys,
            in_arg: false,
        });
    };
    let head = &input[..input.len() - arg.len()];
    let found = |cands, back: &str, note| Found {
        cands,
        back: back.to_string(),
        note,
        in_arg: true,
    };
    match (c.name, c.arg) {
        ("ssh", _) => ssh_found(app, head, arg),
        ("run", _) => {
            // The first word is the program; file arguments follow it.
            let ws = words(arg);
            let &(start, word) = ws.last()?;
            if ws.len() < 2 {
                return None;
            }
            let base = app.active_root().path.clone();
            let lead = format!("{head}{}", &arg[..start]);
            let cands = path_cands(app, &base, &lead, word, Filter::All, Style::Word);
            Some(found(cands, word, Note::Keys))
        }
        ("ai", _) => {
            if let Some((tool, rest)) = arg.split_once(' ') {
                // `/ai <tool> @…`: the accounts added under that tool (D11), `@default` first;
                // names come from the validated listing only.
                let word = rest.trim_start();
                let plain = word.is_empty() || word.starts_with('@');
                if word.contains(char::is_whitespace)
                    || !plain
                    || !crate::ai::picker::supports_accounts(tool)
                    || !ai_tool_names(app).iter().any(|t| t == tool)
                {
                    return None;
                }
                let state = app.ai.state_dir.clone()?;
                let lead = &input[..input.len() - word.len()];
                let typed = word.strip_prefix('@').unwrap_or("");
                let cands = std::iter::once(crate::ai::accounts::DEFAULT.to_string())
                    .chain(crate::ai::accounts::for_tool(&state, tool))
                    .filter(|n| n.starts_with(typed))
                    .map(|n| Cand {
                        fill: format!("{lead}@{n}"),
                        label: format!("@{n}"),
                        kind: CandKind::Other,
                        source: "",
                    })
                    .collect();
                return Some(found(cands, word, Note::Keys));
            }
            let cands = ai_tool_names(app)
                .into_iter()
                .filter(|t| t.starts_with(arg))
                .map(|t| Cand {
                    fill: format!("{head}{t} "),
                    label: t,
                    kind: CandKind::Other,
                    source: "",
                })
                .collect();
            Some(found(cands, arg, Note::Keys))
        }
        (name, ArgKind::Path { dirs, .. }) => {
            let filter = match (dirs, name) {
                (true, _) => Filter::Dirs,
                (false, "ws open") => Filter::Workspaces,
                _ => Filter::All,
            };
            let base = app.active_root().path.clone();
            let cands = path_cands(app, &base, head, arg, filter, Style::Plain);
            Some(found(cands, arg, Note::Keys))
        }
        (_, ArgKind::None | ArgKind::Text) => None,
        _ => {
            let rows = slash::rows(app, input);
            // Nothing to offer: the slash list shows the command itself.
            if rows.len() == 1 && rows[0].title == c.usage() {
                return None;
            }
            let cands = rows
                .into_iter()
                .map(|r| Cand {
                    fill: r.fill,
                    label: r.title,
                    kind: CandKind::Other,
                    source: "",
                })
                .collect();
            Some(found(cands, arg, Note::Keys))
        }
    }
}

/// Recent destinations first (the destination of each recent line), then the ssh config's
/// hosts, each once.
fn ssh_hosts(app: &App) -> Vec<(String, &'static str)> {
    let mut out: Vec<(String, &'static str)> = Vec::new();
    let mut add = |h: &str, source| {
        if !h.is_empty() && !out.iter().any(|(v, _)| v == h) {
            out.push((h.to_string(), source));
        }
    };
    let home = app.ssh.home.as_deref();
    for e in &app.ssh.recent {
        if let Ok(line) = crate::ssh::args::Line::parse(&e.text, home) {
            add(line.target().as_str(), "recent");
        }
    }
    let config = app.palette.as_ref().and_then(|p| p.ssh_hosts.as_deref());
    for h in config.unwrap_or_default() {
        add(h, "~/.ssh/config");
    }
    out
}

/// The folder `/ssh` resolves relative paths in (SEC-4): the one the window shows for this
/// line, made canonical; `None` when it is gone (nothing is completed, never another folder).
fn ssh_base(app: &App, arg: &str) -> Option<PathBuf> {
    super::ssh::shown_base(app, arg).canonicalize().ok()
}

fn ssh_found(app: &mut App, head: &str, arg: &str) -> Option<Found> {
    let base = ssh_base(app, arg)?;
    let found = |cands, back: &str, note| Found {
        cands,
        back: back.to_string(),
        note,
        in_arg: true,
    };
    match ssh_slot_at_end(arg) {
        SshSlot::Path(start) => {
            let word = &arg[start..];
            let lead = format!("{head}{}", &arg[..start]);
            let cands = path_cands(app, &base, &lead, word, Filter::All, Style::Word);
            let note = Note::Text(format!(
                "files in {} (workspace folder of the active file)",
                tilde(&base, app.ssh.home.as_deref())
            ));
            Some(found(cands, word, note))
        }
        SshSlot::Host(start) => {
            let word = &arg[start..];
            let cands = ssh_hosts(app)
                .into_iter()
                .filter(|(h, _)| h.starts_with(word) && !h.chars().any(safe_text::is_unsafe_char))
                .map(|(h, source)| Cand {
                    fill: format!("{head}{}{} ", &arg[..start], escape(&h)),
                    label: h,
                    kind: CandKind::Other,
                    source,
                })
                .collect();
            Some(found(cands, word, Note::SshRuns))
        }
        SshSlot::Other => None,
    }
}

// ---- the palette -----------------------------------------------------------------------------

/// Whether the palette's list currently shows completion candidates.
pub fn list_open(app: &App) -> bool {
    app.palette
        .as_ref()
        .and_then(|p| p.completion.as_ref())
        .is_some_and(|c| matches!(c.phase, Phase::List(_)))
}

/// Tab (`back`: Shift+Tab) in the palette's slash mode.
pub fn tab(app: &mut App, back: bool) {
    let Some(p) = &app.palette else {
        return;
    };
    match p.completion.as_ref().map(|c| c.phase) {
        Some(Phase::List(_)) => return step(app, if back { -1 } else { 1 }),
        Some(Phase::Common) => return open_list(app, back),
        None => {}
    }
    if back {
        return;
    }
    let text = p.input.text().to_string();
    let kind = slash::in_argument(&text).map(|(c, _)| c.arg);
    // A row the user highlighted with the arrows is an explicit choice: Tab takes it.
    if p.moved && !matches!(kind, Some(ArgKind::Text | ArgKind::None)) && take_row(app, &text) {
        return;
    }
    let found = find(app, &text).filter(|f| !f.cands.is_empty());
    let Some(found) = found else {
        // `/ssh` where no word completes (an option being typed): the highlighted listed
        // line goes in the input, with the folder it brings.
        if kind == Some(ArgKind::Ssh) {
            take_row(app, &text);
        }
        return;
    };
    let mut c = Completion {
        cands: found.cands,
        phase: Phase::Common,
        before: text.clone(),
        back: found.back,
        note: found.note,
        in_arg: found.in_arg,
    };
    match c.cands.len() {
        0 => {}
        1 => {
            let fill = c.cands[0].fill.clone();
            set_input(app, &fill);
        }
        _ => {
            let fills: Vec<&str> = c.cands.iter().map(|x| x.fill.as_str()).collect();
            let common = common_prefix(&fills);
            if common.len() > text.len() && common.starts_with(&text) {
                c.back.push_str(&common[text.len()..]);
                set_input(app, &common);
                c.before = common;
                show_common(app, c);
            } else {
                set(app, c);
                open_list(app, false);
            }
        }
    }
}

/// The highlighted row of the slash list in the input; false when there is none or it is
/// what is typed already.
fn take_row(app: &mut App, text: &str) -> bool {
    let Some(selected) = app.palette.as_ref().map(|p| p.selected) else {
        return false;
    };
    match slash::rows(app, text).get(selected) {
        Some(row) if row.fill != text => {
            let fill = row.fill.clone();
            set_input(app, &fill);
            true
        }
        _ => false,
    }
}

/// Puts `text` in the input; a listed `/ssh` line put there keeps the folder it brings until
/// the input is edited (SEC-4, SEC-5).
fn set_input(app: &mut App, text: &str) {
    let picked = super::ssh::picked(app);
    slash::set_input(app, text);
    slash::remember_listed(app, text, picked);
}

fn set(app: &mut App, c: Completion) {
    if let Some(p) = &mut app.palette {
        p.completion = Some(c);
    }
}

/// The common part is in the input: the row is the command's own while an argument is
/// completed (the candidates are in the detail line).
fn show_common(app: &mut App, c: Completion) {
    if c.in_arg
        && let Some(p) = &app.palette
        && let Some((cmd, _)) = slash::lookup(p.input.text())
    {
        let item = PaletteItem {
            name: "",
            title: cmd.usage(),
            path: None,
            detail: cmd.description.to_string(),
            keys: String::new(),
            hits: Vec::new(),
        };
        if let Some(p) = &mut app.palette {
            p.items = vec![item];
            p.selected = 0;
            p.top = 0;
        }
    }
    set(app, c);
    app.dirty = true;
}

fn open_list(app: &mut App, back: bool) {
    let Some(p) = &mut app.palette else {
        return;
    };
    let Some(c) = &mut p.completion else {
        return;
    };
    c.before = p.input.text().to_string();
    let i = if back { c.cands.len() - 1 } else { 0 };
    c.phase = Phase::List(i);
    p.items = c
        .cands
        .iter()
        .map(|x| PaletteItem {
            name: "",
            title: x.label.clone(),
            path: None,
            detail: String::new(),
            keys: x.source.to_string(),
            hits: Vec::new(),
        })
        .collect();
    p.top = 0;
    show(app, i);
}

/// Selects candidate `i`: the input shows it.
fn show(app: &mut App, i: usize) {
    let Some(p) = &mut app.palette else {
        return;
    };
    let Some(c) = &mut p.completion else {
        return;
    };
    let Some(cand) = c.cands.get(i) else {
        return;
    };
    p.input.set(&cand.fill);
    c.phase = Phase::List(i);
    p.selected = i;
    p.error = None;
    app.dirty = true;
}

/// Tab / Shift+Tab (and the arrows) in the list, wrapping.
pub fn step(app: &mut App, delta: isize) {
    let Some((i, n)) = app.palette.as_ref().and_then(|p| {
        let c = p.completion.as_ref()?;
        match c.phase {
            Phase::List(i) => Some((i, c.cands.len())),
            Phase::Common => None,
        }
    }) else {
        return;
    };
    if n == 0 {
        return;
    }
    let next = (i as isize + delta).rem_euclid(n as isize) as usize;
    show(app, next);
}

/// Selects candidate `i` (a page key or a click moved the list's highlight).
pub fn select(app: &mut App, i: usize) {
    show(app, i);
}

/// Enter in the list: the candidate stays in the input, the list closes, nothing runs.
pub fn take(app: &mut App) {
    if let Some(p) = &mut app.palette {
        p.completion = None;
    }
    palette::refilter(app);
}

/// Esc in the list: the text from before the list, the list closed.
pub fn cancel(app: &mut App) {
    let before = app
        .palette
        .as_mut()
        .and_then(|p| p.completion.take())
        .map(|c| c.before);
    if let Some(text) = before {
        set_input(app, &text);
    }
}

/// Any edit of the input ends the completion.
pub fn clear(app: &mut App) -> bool {
    app.palette
        .as_mut()
        .and_then(|p| p.completion.take())
        .is_some()
}
