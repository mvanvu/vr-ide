//! Slash commands of the F1 command window (T20, D258/D263): input that starts with `/` in
//! the palette names one of [`SLASH_COMMANDS`]. The one registry feeds the palette list,
//! `/help` and the table in `docs/commands.md` (a test keeps that table equal to
//! [`docs_table`]).

use std::path::{Path, PathBuf};

use super::dialog::DocRef;
use super::palette;
use super::registry::CmdArgs;
use super::terminal::LineTarget;
use crate::app::App;
use crate::ui::dialog::{DialogWidget, PromptDialog};

/// What a slash command takes after its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgKind {
    /// Nothing; any text after the name is an error.
    None,
    /// A path, completed from the disk with Tab. `dirs`: only folders are offered.
    Path { required: bool, dirs: bool },
    /// Free text, sent as typed (`/run`, `/ai`).
    Text,
    /// One of a fixed list.
    Choice(&'static [&'static str]),
    /// Optional: a root of the workspace by name (default: the root of the active file).
    Root,
    /// Optional: the number of a recent workspace (no number: the list is shown).
    Recent,
    /// Optional: the number (or part of the path) of a recently opened file.
    RecentFile,
    /// A language id with a compiled-in grammar.
    Language,
    /// A setting name, then its value.
    Setting,
    /// Optional: an ssh argument line, the words that would follow `ssh ` in a terminal; the
    /// list shows open sessions, recently used lines and the aliases of the ssh config.
    Ssh,
}

impl ArgKind {
    pub fn required(self) -> bool {
        matches!(
            self,
            ArgKind::Path { required: true, .. }
                | ArgKind::Text
                | ArgKind::Choice(_)
                | ArgKind::Language
                | ArgKind::Setting
        )
    }

    /// An empty argument with Enter takes the highlighted value of the list.
    fn picks_from_list(self) -> bool {
        matches!(
            self,
            ArgKind::Choice(_)
                | ArgKind::Root
                | ArgKind::Recent
                | ArgKind::RecentFile
                | ArgKind::Language
                | ArgKind::Ssh
        )
    }
}

/// One slash command.
#[derive(Debug, Clone, Copy)]
pub struct SlashCommand {
    /// Without the slash; may hold a space ("ws add").
    pub name: &'static str,
    /// Argument hint shown after the name ("<dir>", "[path]"), "" for none.
    pub args: &'static str,
    pub arg: ArgKind,
    pub description: &'static str,
    /// Runs with the trimmed argument (already checked against `arg`). `Err` keeps the window
    /// open with the message under the input.
    pub run: fn(&mut App, &str) -> Result<(), String>,
}

impl SlashCommand {
    /// "/name args" as listed.
    pub fn usage(&self) -> String {
        if self.args.is_empty() {
            format!("/{}", self.name)
        } else {
            format!("/{} {}", self.name, self.args)
        }
    }
}

pub const THEMES: &[&str] = &["synthwave", "dracula", "dark", "light"];
pub const INDENTS: &[&str] = &["2", "4", "tab"];
pub const EOLS: &[&str] = &["lf", "crlf"];

const fn cmd(
    name: &'static str,
    args: &'static str,
    arg: ArgKind,
    description: &'static str,
    run: fn(&mut App, &str) -> Result<(), String>,
) -> SlashCommand {
    SlashCommand {
        name,
        args,
        arg,
        description,
        run,
    }
}

const NO_ARG: ArgKind = ArgKind::None;

/// Every slash command, in the order `/help` and the docs list them.
pub const SLASH_COMMANDS: &[SlashCommand] = &[
    cmd(
        "save-as",
        "[path]",
        ArgKind::Path {
            required: false,
            dirs: false,
        },
        "Save under another name (no path: the Save As prompt)",
        save_as,
    ),
    cmd("reload", "", NO_ARG, "Reload the file from disk", reload),
    cmd(
        "rename",
        "<name>",
        ArgKind::Text,
        "Rename the file on disk (the tab follows)",
        rename,
    ),
    cmd(
        "copy-path",
        "",
        NO_ARG,
        "Copy the file's full path",
        copy_path,
    ),
    cmd(
        "copy-relpath",
        "",
        NO_ARG,
        "Copy the path within its workspace folder",
        copy_relpath,
    ),
    cmd(
        "recent",
        "[n|name]",
        ArgKind::RecentFile,
        "Open a recent file (no argument: list them)",
        recent,
    ),
    cmd(
        "sort",
        "",
        NO_ARG,
        "Sort the selected lines (none: the file)",
        sort,
    ),
    cmd(
        "unique",
        "",
        NO_ARG,
        "Drop repeated lines (selection or file)",
        unique,
    ),
    cmd(
        "upper",
        "",
        NO_ARG,
        "Upper-case the selection (or the file)",
        upper,
    ),
    cmd(
        "lower",
        "",
        NO_ARG,
        "Lower-case the selection (or the file)",
        lower,
    ),
    cmd(
        "trim",
        "",
        NO_ARG,
        "Drop trailing spaces (selection or file)",
        trim,
    ),
    cmd(
        "indent",
        "2|4|tab",
        ArgKind::Choice(INDENTS),
        "Re-indent; set the file's indent unit",
        indent,
    ),
    cmd(
        "eol",
        "lf|crlf",
        ArgKind::Choice(EOLS),
        "Convert the file's line breaks",
        eol,
    ),
    cmd(
        "lang",
        "<id>",
        ArgKind::Language,
        "Highlight the file as another language",
        lang,
    ),
    cmd(
        "term right",
        "",
        NO_ARG,
        "Terminal panel on the right, for this session",
        term_right,
    ),
    cmd(
        "term bottom",
        "",
        NO_ARG,
        "Terminal panel at the bottom, for this session",
        term_bottom,
    ),
    cmd(
        "run",
        "<command>",
        ArgKind::Text,
        "Type the command and Enter into the terminal (opens one)",
        run,
    ),
    cmd(
        "ai",
        "<tool>",
        ArgKind::Text,
        "Run an AI tool (e.g. claude) in a new terminal tab",
        ai,
    ),
    cmd(
        "ssh",
        "[ssh args]",
        ArgKind::Ssh,
        "Open an SSH session, args as after ssh (none: list hosts)",
        ssh,
    ),
    cmd(
        "ws add",
        "<dir>",
        ArgKind::Path {
            required: true,
            dirs: true,
        },
        "Add a folder to the workspace",
        ws_add,
    ),
    cmd(
        "ws remove",
        "[root]",
        ArgKind::Root,
        "Remove a workspace folder (default: the active file's)",
        ws_remove,
    ),
    cmd(
        "ws save",
        "[file]",
        ArgKind::Path {
            required: false,
            dirs: false,
        },
        "Save the workspace folders to a .vr-workspace file",
        ws_save,
    ),
    cmd(
        "ws open",
        "<file>",
        ArgKind::Path {
            required: true,
            dirs: false,
        },
        "Open a .vr-workspace file",
        ws_open,
    ),
    cmd(
        "ws recent",
        "[n]",
        ArgKind::Recent,
        "Open a recent workspace (no number: list them)",
        ws_recent,
    ),
    cmd(
        "theme",
        "synthwave|dracula|dark|light",
        ArgKind::Choice(THEMES),
        "Switch the colour theme for this session",
        theme,
    ),
    cmd(
        "hidden",
        "",
        NO_ARG,
        "Show or hide hidden files in the explorer",
        hidden,
    ),
    cmd(
        "ignored",
        "",
        NO_ARG,
        "Show or hide ignored files in the explorer",
        ignored,
    ),
    cmd(
        "set",
        "<name> <value>",
        ArgKind::Setting,
        "Change a setting for this session",
        set,
    ),
    cmd(
        "config",
        "",
        NO_ARG,
        "Open settings.toml (made when missing)",
        config,
    ),
    cmd(
        "keymap",
        "",
        NO_ARG,
        "Open keymap.toml (made when missing)",
        keymap,
    ),
    cmd(
        "keys",
        "",
        NO_ARG,
        "Show the key table (read-only tab)",
        keys,
    ),
    cmd("help", "", NO_ARG, "List the slash commands", help),
    cmd("q", "", NO_ARG, "Quit (asks about unsaved files)", quit),
    cmd(
        "q!",
        "",
        NO_ARG,
        "Quit, discarding unsaved changes (copy kept in recovery)",
        quit_discard,
    ),
];

const HELP_HINT: &str = "(/help lists them)";
const NO_FILE: &str = "no file is open";
const SAVE_AS_LABEL: &str = "File name (relative to the project folder)";

/// The command `input` names (the longest name it starts with, followed by a space or the
/// end) and its trimmed argument.
pub fn lookup(input: &str) -> Option<(&'static SlashCommand, &str)> {
    let body = input.strip_prefix('/')?;
    SLASH_COMMANDS
        .iter()
        .filter(|c| {
            body.strip_prefix(c.name)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
        })
        .max_by_key(|c| c.name.len())
        .map(|c| (c, body[c.name.len()..].trim()))
}

/// The command `input` names and its trimmed argument checked against the command's
/// [`ArgKind`], or the message to show under the input.
pub fn parse(input: &str) -> Result<(&'static SlashCommand, String), String> {
    let Some((c, arg)) = lookup(input) else {
        let typed = input.trim();
        if typed == "/" {
            return Err(format!("type a command {HELP_HINT}"));
        }
        return Err(format!("unknown command {typed} {HELP_HINT}"));
    };
    match c.arg {
        ArgKind::None if !arg.is_empty() => Err(format!("/{} takes no argument", c.name)),
        k if k.required() && arg.is_empty() => Err(format!("/{} needs {}", c.name, c.args)),
        ArgKind::Choice(list) if !list.contains(&arg) => Err(format!(
            "/{}: {arg} is not one of {}",
            c.name,
            list.join(", ")
        )),
        _ => Ok((c, arg.to_string())),
    }
}

// ---- list, Tab, Enter -----------------------------------------------------------------------

/// One row of the window in slash mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub title: String,
    pub detail: String,
    /// The input Tab (or Enter on a partial name) puts in place.
    pub fill: String,
}

/// The command whose argument is being typed: a known name followed by a space.
fn in_argument(input: &str) -> Option<(&'static SlashCommand, &str)> {
    let (c, _) = lookup(input)?;
    let rest = &input[1 + c.name.len()..];
    rest.starts_with(' ').then(|| (c, rest.trim_start()))
}

fn command_row(c: &SlashCommand) -> Row {
    let space = if c.arg == ArgKind::None { "" } else { " " };
    Row {
        title: c.usage(),
        detail: c.description.to_string(),
        fill: format!("/{}{space}", c.name),
    }
}

/// Rows for `input`: the commands whose name starts with what is typed, or, while the
/// argument of a known command is typed, the values Tab can complete (else that command).
pub fn rows(app: &App, input: &str) -> Vec<Row> {
    if let Some((c, arg)) = in_argument(input) {
        let values = candidates(app, c.arg, arg);
        if values.is_empty() {
            return vec![command_row(c)];
        }
        return values
            .into_iter()
            .map(|(value, label)| Row {
                title: label,
                detail: String::new(),
                fill: format!("/{} {value}", c.name),
            })
            .collect();
    }
    let typed = input.strip_prefix('/').unwrap_or(input).trim_start();
    SLASH_COMMANDS
        .iter()
        .filter(|c| c.name.starts_with(typed))
        .map(command_row)
        .collect()
}

/// The input after Tab: the highlighted command's name, or for an argument the only value,
/// else the longest prefix all values share, else the highlighted value.
pub fn complete(input: &str, rows: &[Row], selected: usize) -> Option<String> {
    let chosen = rows.get(selected).map(|r| r.fill.clone());
    let Some((c, _)) = in_argument(input) else {
        return chosen;
    };
    if c.arg == ArgKind::Text
        || c.arg == ArgKind::None
        || rows.len() == 1 && rows[0].fill == command_row(c).fill
    {
        return None;
    }
    if rows.len() == 1 {
        return chosen;
    }
    let common = rows
        .iter()
        .map(|r| r.fill.as_str())
        .reduce(common_prefix)
        .unwrap_or("");
    if common.len() > input.len() {
        Some(common.to_string())
    } else {
        chosen
    }
}

fn common_prefix<'a>(a: &'a str, b: &str) -> &'a str {
    let end = a
        .char_indices()
        .zip(b.chars())
        .find(|((_, x), y)| x != y)
        .map_or_else(|| a.len().min(b.len()), |((i, _), _)| i);
    &a[..end]
}

/// What Enter does with the input.
#[derive(Debug, Clone)]
pub enum Submit {
    /// Run this command with this argument.
    Run(&'static SlashCommand, String),
    /// Put this text in the input (a partial name of a command that needs an argument).
    Fill(String),
    /// Show this message under the input.
    Error(String),
}

/// Enter: a complete command runs; a partial name takes the highlighted command (run when it
/// needs no argument); an empty choice takes the highlighted value.
pub fn submit(input: &str, rows: &[Row], selected: usize) -> Submit {
    // A listed choice with nothing typed yet ("/ws recent "): the highlighted value. The ssh
    // list always runs its highlighted row: what was typed is its first row when it is a target.
    if let Some((c, typed)) = in_argument(input)
        && (typed.is_empty() || c.arg == ArgKind::Ssh)
        && c.arg.picks_from_list()
        && let Some(row) = rows.get(selected)
        && let Ok((c, arg)) = parse(&row.fill)
        && !arg.is_empty()
    {
        // A listed ssh line that makes ssh run a local program is put in the input to be
        // read first; Enter on it then runs it (SEC-5).
        if c.arg == ArgKind::Ssh
            && arg != typed.trim()
            && crate::ssh::args::split(&arg, None)
                .ok()
                .and_then(|w| crate::ssh::args::local_command_word(&w))
                .is_some()
        {
            return Submit::Fill(row.fill.clone());
        }
        return Submit::Run(c, arg);
    }
    let err = match parse(input) {
        Ok((c, arg)) => return Submit::Run(c, arg),
        Err(e) => e,
    };
    let picks = match lookup(input) {
        // Nothing typed after the slash: no command is guessed.
        None => input.trim() != "/",
        Some((c, arg)) => arg.is_empty() && c.arg.picks_from_list(),
    };
    match rows.get(selected) {
        Some(row) if picks && row.fill != input => match parse(&row.fill) {
            Ok((c, arg)) => Submit::Run(c, arg),
            Err(_) => Submit::Fill(row.fill.clone()),
        },
        _ => Submit::Error(err),
    }
}

/// Values the argument of `kind` can take for the partial `arg`: (value, label).
fn candidates(app: &App, kind: ArgKind, arg: &str) -> Vec<(String, String)> {
    match kind {
        ArgKind::None | ArgKind::Text => Vec::new(),
        ArgKind::Path { dirs, .. } => path_candidates(app, arg, dirs),
        ArgKind::Choice(list) => list
            .iter()
            .filter(|v| v.starts_with(arg))
            .map(|v| (v.to_string(), v.to_string()))
            .collect(),
        ArgKind::Root => app
            .workspace
            .roots()
            .iter()
            .filter(|r| r.name.starts_with(arg))
            .map(|r| {
                let label = format!("{}  {}", r.name, r.path.display());
                (r.name.clone(), label)
            })
            .collect(),
        ArgKind::Recent => app
            .recent_workspaces
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| ((i + 1).to_string(), recent_label(i, e)))
            .filter(|(v, _)| v.starts_with(arg))
            .collect(),
        ArgKind::RecentFile => app
            .recent_files
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                let n = (i + 1).to_string();
                let shown = app.display_path(p);
                (n.starts_with(arg) || shown.contains(arg))
                    .then(|| (n.clone(), format!("{n}  {shown}")))
            })
            .collect(),
        ArgKind::Language => app
            .syntax
            .language_ids()
            .into_iter()
            .filter(|id| id.starts_with(arg))
            .map(|id| (id.clone(), id))
            .collect(),
        ArgKind::Setting => setting_candidates(app, arg),
        ArgKind::Ssh => {
            let held = app.palette.as_ref().and_then(|p| p.ssh_hosts.as_deref());
            super::ssh::rows(app, arg, held.unwrap_or_default())
        }
    }
}

/// `/set`: setting names (with their value now) until a space, then the values a setting of
/// a fixed set takes (true / false, theme names).
fn setting_candidates(app: &App, arg: &str) -> Vec<(String, String)> {
    let Some((name, value)) = arg.split_once(' ') else {
        return vr_config::Settings::names()
            .into_iter()
            .filter(|n| n.starts_with(arg))
            .map(|n| {
                let now = app.settings.value_text(&n).unwrap_or_default();
                (format!("{n} "), format!("{n}  {now}"))
            })
            .collect();
    };
    let value = value.trim_start();
    let choices: &[&str] = match app.settings.value_text(name).as_deref() {
        Some("true" | "false") => &["true", "false"],
        _ if name == "theme" => THEMES,
        _ => &[],
    };
    choices
        .iter()
        .filter(|v| v.starts_with(value))
        .map(|v| (format!("{name} {v}"), v.to_string()))
        .collect()
}

fn recent_label(i: usize, e: &vr_config::RecentWorkspace) -> String {
    let what = match &e.file {
        Some(f) => f.display().to_string(),
        None => e
            .roots
            .iter()
            .map(|r| r.display().to_string())
            .collect::<Vec<_>>()
            .join(", "),
    };
    format!("{}  {what}", i + 1)
}

/// One folder's entries as path completion needs them: (name, is a folder), sorted by name.
/// The window keeps the last one read while it is open, so typing in the same folder, an
/// index update or a Tab does not read the folder again (a large or slow folder is read once).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirListing {
    base: PathBuf,
    entries: Vec<(String, bool)>,
}

impl DirListing {
    fn read(base: &Path) -> DirListing {
        #[cfg(test)]
        LISTINGS.with(|c| c.set(c.get() + 1));
        let mut entries: Vec<(String, bool)> = std::fs::read_dir(base)
            .map(|rd| {
                rd.filter_map(Result::ok)
                    .filter_map(|e| {
                        let name = e.file_name().into_string().ok()?;
                        let kind = e.file_type().ok()?;
                        // Only a link costs a stat: it completes like what it points to.
                        let is_dir = if kind.is_symlink() {
                            std::fs::metadata(e.path()).is_ok_and(|m| m.is_dir())
                        } else {
                            kind.is_dir()
                        };
                        Some((name, is_dir))
                    })
                    .collect()
            })
            .unwrap_or_default();
        entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        DirListing {
            base: base.to_path_buf(),
            entries,
        }
    }
}

/// Folder reads done by path completion on this thread (tests).
#[cfg(test)]
pub fn dir_listings() -> usize {
    LISTINGS.with(std::cell::Cell::get)
}

#[cfg(test)]
thread_local! {
    static LISTINGS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Paths listed at most for one Tab.
const PATH_LIMIT: usize = 100;

/// `~` and `~/…` name the home folder, as in a shell.
fn expand_home(arg: &str) -> PathBuf {
    let home = std::env::var_os("HOME").filter(|h| !h.is_empty());
    match (arg.strip_prefix('~'), home) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => {
            PathBuf::from(home).join(rest.trim_start_matches('/'))
        }
        _ => PathBuf::from(arg),
    }
}

/// The path `arg` names: `~` expanded, relative to the active root.
pub fn resolve_arg(app: &App, arg: &str) -> PathBuf {
    app.resolve(&expand_home(arg))
}

/// `arg` split into its folder part (up to the last `/`) and the name typed after it, with the
/// folder that part names.
fn split_path_arg<'a>(app: &App, arg: &'a str) -> (&'a str, &'a str, PathBuf) {
    let (dir_part, prefix) = match arg.rfind('/') {
        Some(i) => (&arg[..=i], &arg[i + 1..]),
        None => ("", arg),
    };
    let base = if dir_part.is_empty() {
        app.active_root().path.clone()
    } else {
        resolve_arg(app, dir_part)
    };
    (dir_part, prefix, base)
}

/// Reads the folder a path argument in `input` completes from, unless the window already
/// holds that folder's listing. Called before the rows are built.
pub fn prepare(app: &mut App, input: &str) {
    // An edited line is a typed line: the folder of the listed line it came from is dropped.
    if let Some(p) = &mut app.palette
        && p.ssh_listed.as_ref().is_some_and(|(t, _)| t != input)
    {
        p.ssh_listed = None;
    }
    let Some((c, arg)) = in_argument(input) else {
        return;
    };
    // The ssh config is read once per window, not per key (and `ssh -G` never, SEC-S4).
    if c.arg == ArgKind::Ssh {
        if app.palette.as_ref().is_some_and(|p| p.ssh_hosts.is_none()) {
            let hosts = app
                .ssh
                .home
                .as_deref()
                .map(crate::ssh::config::hosts)
                .unwrap_or_default();
            if let Some(p) = &mut app.palette {
                p.ssh_hosts = Some(hosts);
            }
        }
        return;
    }
    if !matches!(c.arg, ArgKind::Path { .. }) {
        return;
    }
    let (_, _, base) = split_path_arg(app, arg);
    let held = app
        .palette
        .as_ref()
        .and_then(|p| p.dir_cache.as_ref())
        .is_some_and(|l| l.base == base);
    if !held {
        let listing = DirListing::read(&base);
        if let Some(p) = &mut app.palette {
            p.dir_cache = Some(listing);
        }
    }
}

/// Entries of the folder part of `arg` whose name starts with the rest; folders end in `/`.
/// Hidden entries only when the typed name starts with a dot.
fn path_candidates(app: &App, arg: &str, dirs_only: bool) -> Vec<(String, String)> {
    let (dir_part, prefix, base) = split_path_arg(app, arg);
    let held = app
        .palette
        .as_ref()
        .and_then(|p| p.dir_cache.as_ref())
        .filter(|l| l.base == base);
    let read;
    let listing = match held {
        Some(l) => l,
        None => {
            read = DirListing::read(&base);
            &read
        }
    };
    listing
        .entries
        .iter()
        .filter(|(name, is_dir)| {
            name.starts_with(prefix)
                && (!name.starts_with('.') || prefix.starts_with('.'))
                && (*is_dir || !dirs_only)
        })
        .take(PATH_LIMIT)
        .map(|(name, is_dir)| {
            let slash = if *is_dir { "/" } else { "" };
            let value = format!("{dir_part}{name}{slash}");
            (value.clone(), value)
        })
        .collect()
}

// ---- the window -----------------------------------------------------------------------------

fn window_state(app: &App) -> Option<(String, usize)> {
    app.palette
        .as_ref()
        .map(|p| (p.input.text().to_string(), p.selected))
}

pub(crate) fn set_input(app: &mut App, text: &str) {
    if let Some(p) = &mut app.palette {
        p.input.set(text);
        p.error = None;
    }
    palette::refilter(app);
}

/// Tab in slash mode.
pub fn tab(app: &mut App) {
    let Some((text, selected)) = window_state(app) else {
        return;
    };
    prepare(app, &text);
    let picked = super::ssh::picked(app);
    if let Some(next) = complete(&text, &rows(app, &text), selected)
        && next != text
    {
        set_input(app, &next);
        remember_listed(app, &next, picked);
    }
}

/// A listed ssh line put in the input keeps the folder it brings until the input is edited
/// (SEC-4, SEC-5).
fn remember_listed(app: &mut App, input: &str, picked: Option<(String, PathBuf)>) {
    if let (Some((line, base)), Some(p)) = (picked, &mut app.palette)
        && input.strip_prefix(super::ssh::SLASH) == Some(line.as_str())
    {
        p.ssh_listed = Some((input.to_string(), base));
    }
}

/// Enter in slash mode: runs the command after closing the window; a refused input or a
/// command that fails leaves the window open with the text and the message under it.
pub fn enter(app: &mut App) {
    let Some((text, selected)) = window_state(app) else {
        return;
    };
    prepare(app, &text);
    let picked = super::ssh::picked(app);
    match submit(&text, &rows(app, &text), selected) {
        Submit::Run(c, arg) => {
            palette::close(app);
            app.ssh.picked = picked.clone().filter(|_| c.arg == ArgKind::Ssh);
            let ran = (c.run)(app, &arg);
            app.ssh.picked = None;
            if let Err(e) = ran {
                reopen(app, &text, e);
                remember_listed(app, &text, picked);
            }
            app.dirty = true;
        }
        Submit::Fill(fill) => {
            set_input(app, &fill);
            remember_listed(app, &fill, picked);
        }
        Submit::Error(e) => show_error(app, e),
    }
}

fn show_error(app: &mut App, e: String) {
    if let Some(p) = &mut app.palette {
        p.error = Some(e);
    }
    app.dirty = true;
}

/// The window again with `text` and the message `e` (a command that failed).
fn reopen(app: &mut App, text: &str, e: String) {
    // A dialog the command opened (Save As asking to replace) speaks for itself.
    if app.dialog.is_some() {
        app.error(e);
        return;
    }
    palette::open(app);
    set_input(app, text);
    show_error(app, e);
}

// ---- handlers -------------------------------------------------------------------------------

/// Text for a terminal is one line: a line break or another control byte would run more than
/// what the window showed.
fn one_line(name: &str, text: &str) -> Result<(), String> {
    if text.chars().any(char::is_control) {
        return Err(format!(
            "/{name}: the text must be one line without control characters"
        ));
    }
    Ok(())
}

fn save_as(app: &mut App, arg: &str) -> Result<(), String> {
    if app.docs.is_empty() {
        return Err(NO_FILE.into());
    }
    if arg.is_empty() {
        app.run_command("file.save_as", CmdArgs::None);
        return Ok(());
    }
    let target = resolve_arg(app, arg);
    if target.is_dir() {
        return Err(format!("{} is a folder", app.display_path(&target)));
    }
    // The Save As prompt's own path: an existing file asks before it is replaced, and a
    // refusal reopens that prompt with the message.
    let widget = DialogWidget::Prompt(PromptDialog::new("Save As", SAVE_AS_LABEL, arg));
    let doc = DocRef::of(app, app.active);
    super::file::answer_save_as(app, widget, doc, &target.to_string_lossy());
    Ok(())
}

fn reload(app: &mut App, _: &str) -> Result<(), String> {
    if app.docs.is_empty() {
        return Err(NO_FILE.into());
    }
    app.run_command("file.reload", CmdArgs::None);
    Ok(())
}

/// `/ssh [ssh args]`: no argument shows the list.
fn ssh(app: &mut App, arg: &str) -> Result<(), String> {
    if arg.is_empty() {
        palette::open(app);
        set_input(app, super::ssh::SLASH);
        return Ok(());
    }
    super::ssh::connect(app, arg)
}

fn term_right(app: &mut App, _: &str) -> Result<(), String> {
    super::terminal::set_position(app, vr_config::TerminalPosition::Right);
    Ok(())
}

fn term_bottom(app: &mut App, _: &str) -> Result<(), String> {
    super::terminal::set_position(app, vr_config::TerminalPosition::Bottom);
    Ok(())
}

/// Only the user's Enter in the window gets here; the text is typed once, as written.
fn run(app: &mut App, line: &str) -> Result<(), String> {
    one_line("run", line)?;
    super::terminal::type_line(app, line, LineTarget::Active)
}

/// The tool runs through the user's shell in a new tab, so a missing one shows that shell's
/// own error.
fn ai(app: &mut App, tool: &str) -> Result<(), String> {
    one_line("ai", tool)?;
    super::terminal::type_line(app, tool, LineTarget::NewTab)
}

/// The W-1 checks apply: a missing folder, a file, a repeat or a nested folder is refused.
fn ws_add(app: &mut App, arg: &str) -> Result<(), String> {
    let path = resolve_arg(app, arg);
    // The checks run off the UI thread (W-2); a slow answer comes later as a notice.
    match app.add_root_checked(&path) {
        Some(Err(e)) => Err(e.to_string()),
        Some(Ok(_)) | None => Ok(()),
    }
}

/// Nothing on disk is touched and no tab is closed (W-1).
fn ws_remove(app: &mut App, arg: &str) -> Result<(), String> {
    let id = if arg.is_empty() {
        app.active_root().id
    } else {
        let path = resolve_arg(app, arg);
        app.workspace
            .roots()
            .iter()
            .find(|r| r.name == arg || r.path == path)
            .map(|r| r.id)
            .ok_or_else(|| format!("/ws remove: no workspace folder named {arg}"))?
    };
    let root = app.remove_root(id).map_err(|e| e.to_string())?;
    app.info(format!("workspace: removed {}", root.name));
    Ok(())
}

/// No name: the workspace's own file, replacing only the version opened or saved last (a
/// later change is refused). A name gets the `.vr-workspace` extension when it has none;
/// another existing file is never replaced.
fn ws_save(app: &mut App, arg: &str) -> Result<(), String> {
    let own = app.workspace.file().map(Path::to_path_buf);
    let path = if arg.is_empty() {
        own.clone()
            .ok_or("the workspace has no file yet: /ws save <file>")?
    } else {
        let p = resolve_arg(app, arg);
        if vr_config::is_workspace_file(&p) {
            p
        } else {
            let mut s = p.into_os_string();
            s.push(format!(".{}", vr_config::WORKSPACE_EXT));
            PathBuf::from(s)
        }
    };
    let is_own = own.is_some_and(|f| f == path || path.canonicalize().is_ok_and(|p| p == f));
    // Its own file: only the version read or written last is replaced (D244); a hand edit or
    // another window's save since then is refused, never overwritten.
    let seen = if is_own {
        Some(
            *app.workspace
                .file_version()
                .ok_or_else(|| changed_on_disk(&path))?,
        )
    } else {
        None
    };
    app.save_workspace(&path, seen.as_ref()).map_err(|e| {
        if is_own && e.ends_with(&vr_core::DocError::ChangedOnDisk.to_string()) {
            changed_on_disk(&path)
        } else {
            e
        }
    })?;
    app.info(format!("workspace saved to {}", path.display()));
    Ok(())
}

fn changed_on_disk(path: &Path) -> String {
    format!(
        "/ws save: {} changed on disk since the workspace was opened or saved; /ws open it again or save under another name",
        path.display()
    )
}

/// Unsaved files ask first (Save all / Discard / Cancel), as for quit.
fn ws_open(app: &mut App, arg: &str) -> Result<(), String> {
    let file = resolve_arg(app, arg);
    // Read off the UI thread (W-2): a FIFO or a dead mount never hangs the window.
    app.open_workspace_from(Some(file), Vec::new())
        .map_err(|e| e.to_string())
}

fn ws_recent(app: &mut App, arg: &str) -> Result<(), String> {
    if arg.is_empty() {
        palette::open(app);
        set_input(app, "/ws recent ");
        return Ok(());
    }
    let entry = arg
        .parse::<usize>()
        .ok()
        .and_then(|n| n.checked_sub(1))
        .and_then(|i| app.recent_workspaces.entries.get(i))
        .cloned()
        .ok_or_else(|| format!("/ws recent: no recent workspace {arg}"))?;
    open_recent_workspace(app, &entry)
}

/// Opens a recent-list entry (`/ws recent n`, a click on the welcome screen); its file or
/// folders are read off the UI thread.
pub fn open_recent_workspace(
    app: &mut App,
    entry: &vr_config::RecentWorkspace,
) -> Result<(), String> {
    app.open_workspace_from(entry.file.clone(), entry.roots.clone())
        .map_err(|e| e.to_string())
}

fn theme(app: &mut App, name: &str) -> Result<(), String> {
    app.settings.theme = name.to_string();
    app.info(format!(
        "theme {name} for this session (settings.toml `theme` keeps it)"
    ));
    Ok(())
}

fn hidden(app: &mut App, _: &str) -> Result<(), String> {
    app.run_command("explorer.toggle_hidden", CmdArgs::None);
    Ok(())
}

fn ignored(app: &mut App, _: &str) -> Result<(), String> {
    app.run_command("explorer.toggle_ignored", CmdArgs::None);
    Ok(())
}

fn help(app: &mut App, _: &str) -> Result<(), String> {
    palette::open(app);
    set_input(app, "/");
    Ok(())
}

fn quit(app: &mut App, _: &str) -> Result<(), String> {
    app.run_command("app.quit", CmdArgs::None);
    Ok(())
}

/// Quits without the unsaved-files question; the buffers go to the recovery folder first
/// (D57) and nothing is discarded when that write fails. Running terminals and a running
/// project replace still ask or refuse, as for `/q`; their Quit anyway still discards.
fn quit_discard(app: &mut App, _: &str) -> Result<(), String> {
    super::app::quit_discarding(app);
    Ok(())
}

// ---- phase 2: file extras (T20b) --------------------------------------------------------------

/// The active tab's file, or why there is none.
fn active_file(app: &App) -> Result<PathBuf, String> {
    let doc = app.active_doc().ok_or(NO_FILE)?;
    doc.path
        .clone()
        .ok_or_else(|| "this tab has no file name yet: save it first".to_string())
}

/// The explorer's rename (same checks, open tabs follow) for the active file, in the
/// workspace folder that holds it.
fn rename(app: &mut App, name: &str) -> Result<(), String> {
    let path = active_file(app)?;
    let root = app
        .workspace
        .root_of(&path)
        .map(|r| r.path.clone())
        .ok_or_else(|| {
            format!(
                "{} is outside the workspace folders: /rename works inside them",
                path.display()
            )
        })?;
    // `rename_in` moves the recent-files entry too.
    if let Some(to) = super::explorer::rename_in(app, &root, &path, name)? {
        let shown = app.display_path(&to);
        app.info(format!("renamed to {shown}"));
    }
    Ok(())
}

fn copy_text(app: &mut App, text: String) -> Result<(), String> {
    app.clipboard
        .set(&text)
        .map_err(|e| format!("clipboard: {e}"))?;
    app.info(format!("copied {text}"));
    Ok(())
}

fn copy_path(app: &mut App, _: &str) -> Result<(), String> {
    let path = active_file(app)?;
    copy_text(app, path.display().to_string())
}

/// Relative to the workspace folder holding the file (with several roots, its own root).
fn copy_relpath(app: &mut App, _: &str) -> Result<(), String> {
    let path = active_file(app)?;
    let rel = app
        .workspace
        .root_of(&path)
        .and_then(|r| path.strip_prefix(&r.path).ok())
        .map(|p| p.display().to_string())
        .ok_or_else(|| format!("{} is not inside a workspace folder", path.display()))?;
    copy_text(app, rel)
}

/// No argument: the window lists the recent files; a number opens that entry, other text the
/// newest entry whose shown path contains it.
fn recent(app: &mut App, arg: &str) -> Result<(), String> {
    if arg.is_empty() {
        palette::open(app);
        set_input(app, "/recent ");
        return Ok(());
    }
    let entries = &app.recent_files.entries;
    let picked = match arg.parse::<usize>() {
        Ok(n) => n.checked_sub(1).and_then(|i| entries.get(i)),
        Err(_) => entries.iter().find(|p| app.display_path(p).contains(arg)),
    }
    .cloned()
    .ok_or_else(|| format!("/recent: no recent file {arg}"))?;
    app.open_path(&picked).map_err(|e| e.to_string())?;
    app.focus = crate::app::Focus::Editor;
    Ok(())
}

// ---- phase 2: text commands -----------------------------------------------------------------

/// Runs `f` on the active buffer when it may be edited (edit mode, not read-only: else
/// `editable_doc` shows why and nothing changes). `f` returns whether the text changed.
fn edit_text(
    app: &mut App,
    name: &str,
    f: impl FnOnce(&mut vr_core::Document) -> bool,
) -> Result<(), String> {
    if app.docs.is_empty() {
        return Err(NO_FILE.into());
    }
    let Some(doc) = app.editable_doc() else {
        return Ok(());
    };
    if !f(doc) {
        app.info(format!("/{name}: nothing to change"));
    }
    Ok(())
}

fn sort(app: &mut App, _: &str) -> Result<(), String> {
    edit_text(app, "sort", |d| {
        vr_core::edit_ops::rewrite_lines(&mut d.buffer, |mut lines| {
            lines.sort();
            lines
        })
    })
}

/// Keeps the first of equal lines, wherever the others are.
fn unique(app: &mut App, _: &str) -> Result<(), String> {
    edit_text(app, "unique", |d| {
        vr_core::edit_ops::rewrite_lines(&mut d.buffer, |lines| {
            let mut seen = std::collections::HashSet::new();
            lines
                .into_iter()
                .filter(|l| seen.insert(l.clone()))
                .collect()
        })
    })
}

fn upper(app: &mut App, _: &str) -> Result<(), String> {
    edit_text(app, "upper", |d| {
        vr_core::edit_ops::rewrite_selection(&mut d.buffer, str::to_uppercase)
    })
}

fn lower(app: &mut App, _: &str) -> Result<(), String> {
    edit_text(app, "lower", |d| {
        vr_core::edit_ops::rewrite_selection(&mut d.buffer, str::to_lowercase)
    })
}

/// Trailing spaces and tabs of each line.
fn trim(app: &mut App, _: &str) -> Result<(), String> {
    edit_text(app, "trim", |d| {
        vr_core::edit_ops::rewrite_lines(&mut d.buffer, |lines| {
            lines
                .into_iter()
                .map(|l| l.trim_end_matches([' ', '\t']).to_string())
                .collect()
        })
    })
}

/// Display columns of `lead` (spaces and tabs) with tab stops every `tab_width`.
fn lead_cols(lead: &str, tab_width: usize) -> usize {
    lead.chars().fold(0, |cols, c| {
        if c == '\t' {
            (cols / tab_width + 1) * tab_width
        } else {
            cols + 1
        }
    })
}

/// The indent step of the whole document in columns: the most common increase between the
/// indents of consecutive lines (the smaller one on a tie). Blank lines and block-comment
/// continuations (` * text`, ` */`) are skipped, and 1-column increases count only when there
/// are no others, so a doc comment's lead or an aligned line cannot turn every column into
/// a level. `fallback` when no line is indented deeper than the one before it.
fn indent_step(rope: &ropey::Rope, tw: usize, fallback: usize) -> usize {
    let mut counts = std::collections::BTreeMap::<usize, usize>::new();
    let mut prev: Option<usize> = None;
    for line in rope.lines() {
        let line: std::borrow::Cow<str> = line.into();
        let body = line.trim_start_matches([' ', '\t']);
        let body = body.trim_end_matches(['\r', '\n']);
        if body.is_empty() || body.starts_with('*') {
            continue;
        }
        let cols = lead_cols(
            &line[..line.len() - line.trim_start_matches([' ', '\t']).len()],
            tw,
        );
        if let Some(p) = prev.filter(|p| cols > *p) {
            *counts.entry(cols - p).or_default() += 1;
        }
        prev = Some(cols);
    }
    let best = |wide: bool| {
        counts
            .iter()
            .filter(|(d, _)| (**d > 1) == wide)
            .max_by(|a, b| a.1.cmp(b.1).then(b.0.cmp(a.0)))
            .map(|(d, _)| *d)
    };
    best(true).or_else(|| best(false)).unwrap_or(fallback)
}

/// Re-indents the lines (selection or file) by the document's indent step ([`indent_step`],
/// taken from the whole file even for a selection, so selected lines keep their level
/// against the rest). Each step becomes the new unit; columns left over stay as spaces. The
/// new unit is the file's indent unit from now on.
fn indent(app: &mut App, arg: &str) -> Result<(), String> {
    let unit = match arg {
        "2" => "  ",
        "4" => "    ",
        _ => "\t",
    };
    edit_text(app, "indent", |d| {
        let opts = d.buffer.options().clone();
        let tw = opts.tab_width.max(1);
        let step = indent_step(d.buffer.text(), tw, lead_cols(&opts.indent_unit, tw).max(1));
        let changed = vr_core::edit_ops::rewrite_lines(&mut d.buffer, |lines| {
            lines
                .into_iter()
                .map(|l| {
                    let body = l.trim_start_matches([' ', '\t']);
                    if body.is_empty() {
                        return l;
                    }
                    let cols = lead_cols(&l[..l.len() - body.len()], tw);
                    format!(
                        "{}{}{body}",
                        unit.repeat(cols / step),
                        " ".repeat(cols % step)
                    )
                })
                .collect()
        });
        d.buffer.set_options(vr_core::BufferOptions {
            tab_width: opts.tab_width,
            indent_unit: unit.to_string(),
        });
        changed || opts.indent_unit != unit
    })
}

/// Every line break becomes LF or CRLF; the file is changed (saved with them).
fn eol(app: &mut App, arg: &str) -> Result<(), String> {
    use vr_core::document::LineEnding;
    let (ending, name) = if arg == "crlf" {
        (LineEnding::CrLf, "CRLF")
    } else {
        (LineEnding::Lf, "LF")
    };
    let mut already = false;
    let mut no_breaks = false;
    edit_text(app, "eol", |d| {
        let changed = vr_core::edit_ops::convert_line_breaks(&mut d.buffer, ending.as_str());
        no_breaks = d.buffer.text().len_lines() == 1;
        already = !changed && !no_breaks && d.line_ending == ending;
        d.line_ending = ending;
        // Nothing to report as "nothing to change": the notices below say more.
        true
    })?;
    if no_breaks {
        app.info(format!(
            "/eol: no line breaks yet; new lines use {name} (Enter, paste)"
        ));
    } else if already {
        app.info(format!("/eol: the line breaks are already {name}"));
    }
    Ok(())
}

/// Works in read mode too: the text does not change.
fn lang(app: &mut App, id: &str) -> Result<(), String> {
    if app.docs.is_empty() {
        return Err(NO_FILE.into());
    }
    if !app.syntax.set_language(app.active, id) {
        return Err(format!("/lang: no language {id} (Tab lists them)"));
    }
    app.sync_syntax();
    app.info(format!("highlighted as {id} until the tab closes"));
    Ok(())
}

// ---- phase 2: settings ----------------------------------------------------------------------

/// The setting changes for this session only; settings.toml is not written.
fn set(app: &mut App, arg: &str) -> Result<(), String> {
    let (name, value) = arg
        .split_once(char::is_whitespace)
        .map_or((arg, ""), |(n, v)| (n, v.trim()));
    let new = app.settings.with_value(name, value)?;
    if name == "theme" && !THEMES.contains(&new.theme.as_str()) {
        return Err(format!(
            "theme: {} is not one of {}",
            new.theme,
            THEMES.join(", ")
        ));
    }
    apply_settings(app, new);
    let now = app.settings.value_text(name).unwrap_or_default();
    app.info(format!(
        "{name} = {now} for this session (settings.toml keeps its value)"
    ));
    Ok(())
}

/// Puts `new` in place and brings what reads settings only at startup in step, for the keys
/// that changed only: the open files' tab width (and their indent unit unless `/indent` set
/// their own), the explorer's hidden / ignored toggles and the terminal panel's height, width
/// and position. A key that did not change leaves the current view alone (ctrl+h, terminal
/// grow / shrink, ctrl+f7).
/// `scrollback` and `max_highlight_bytes` apply to terminals and files opened from now on.
pub(crate) fn apply_settings(app: &mut App, new: vr_config::Settings) {
    let before = app.buffer_options();
    let old = std::mem::replace(&mut app.settings, new);
    let after = app.buffer_options();
    if before != after {
        for doc in &mut app.docs {
            let mut o = doc.buffer.options().clone();
            o.tab_width = after.tab_width;
            if o.indent_unit == before.indent_unit {
                o.indent_unit = after.indent_unit.clone();
            }
            doc.buffer.set_options(o);
        }
    }
    let new = &app.settings;
    let hidden =
        old.show_hidden != new.show_hidden && app.explorer.tree.show_hidden() != new.show_hidden;
    let ignored = old.show_ignored != new.show_ignored
        && app.explorer.tree.show_ignored() != new.show_ignored;
    if old.terminal_height_percent != new.terminal_height_percent {
        app.terminal.height_percent = new.terminal_height_percent;
    }
    if old.terminal_width_percent != new.terminal_width_percent {
        app.terminal.width_percent = new.terminal_width_percent;
        app.terminal.width_cols = None;
    }
    if old.terminal_position != new.terminal_position {
        app.terminal.position = new.terminal_position;
        app.dirty = true;
    }
    if hidden {
        app.run_command("explorer.toggle_hidden", CmdArgs::None);
    }
    if ignored {
        app.run_command("explorer.toggle_ignored", CmdArgs::None);
    }
}

/// Opens the user file `name` of the configuration folder, first writing it from the
/// built-in defaults (every line commented out) when it does not exist.
fn open_user_file(app: &mut App, name: &str, defaults: &str) -> Result<(), String> {
    let dir = app
        .config_dir
        .clone()
        .ok_or("no configuration folder is known")?;
    let path = dir.join(name);
    if path.symlink_metadata().is_err() {
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        // A folder made just now was not there to watch at start (T24 live reload).
        app.watch_config_files();
        let made = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .and_then(|mut f| {
                use std::io::Write;
                f.write_all(vr_config::user_file_template(defaults).as_bytes())
            });
        match made {
            Ok(()) => app.info(format!(
                "{} made from the built-in defaults",
                path.display()
            )),
            // Made meanwhile (another vr-ide): open that one.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
    }
    app.open_path(&path).map_err(|e| e.to_string())?;
    app.focus = crate::app::Focus::Editor;
    app.info(format!("{name}: changes are applied when you save it"));
    Ok(())
}

fn config(app: &mut App, _: &str) -> Result<(), String> {
    open_user_file(app, "settings.toml", vr_config::DEFAULT_SETTINGS)
}

fn keymap(app: &mut App, _: &str) -> Result<(), String> {
    open_user_file(app, "keymap.toml", vr_config::DEFAULT_KEYMAP)
}

const KEYS_TITLE: &str = "keys";

/// The key table of the keymap in use: context, key, command, title; keys a terminal without
/// the kitty protocol does not send are marked.
pub fn key_table(app: &App) -> String {
    let rows: Vec<[String; 4]> = app
        .keymap
        .bindings()
        .map(|(ctx, k, cmd)| {
            let title = super::registry::find(cmd).map_or("", |c| c.title);
            let note = if crate::event_loop::legacy_unreachable(k) {
                " (kitty keyboard only)"
            } else {
                ""
            };
            [
                ctx.to_string(),
                k.to_string(),
                cmd.to_string(),
                format!("{title}{note}"),
            ]
        })
        .collect();
    let head = ["context", "key", "command", "title"].map(String::from);
    let width = |i: usize| {
        rows.iter()
            .chain(std::iter::once(&head))
            .map(|r| r[i].chars().count())
            .max()
            .unwrap_or(0)
    };
    let (w0, w1, w2) = (width(0), width(1), width(2));
    let mut out = String::from(
        "Key table of the keymap in use (read-only; /keymap opens keymap.toml to change it)\n\n",
    );
    for r in std::iter::once(&head).chain(rows.iter()) {
        let line = format!("{:w0$}  {:w1$}  {:w2$}  {}", r[0], r[1], r[2], r[3]);
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// A read-only tab without a file holding [`key_table`]; a second `/keys` shows it again.
fn keys(app: &mut App, _: &str) -> Result<(), String> {
    let table = key_table(app);
    if let Some(i) = app.views.iter().position(|v| v.title == Some(KEYS_TITLE)) {
        app.set_active(i);
    } else {
        let mut doc = vr_core::Document::untitled();
        doc.buffer = vr_core::Buffer::from_str(&table);
        doc.read_only = true;
        app.docs.push(doc);
        app.views.push(crate::app::View {
            follow_cursor: true,
            title: Some(KEYS_TITLE),
            ..Default::default()
        });
        app.set_active(app.docs.len() - 1);
    }
    app.focus = crate::app::Focus::Editor;
    Ok(())
}

/// The markdown table of `docs/commands.md`.
pub fn docs_table() -> String {
    let mut out = String::from("| Command | What it does |\n|---|---|\n");
    for c in SLASH_COMMANDS {
        // A `|` inside a table cell must be escaped, even in code.
        let usage = c.usage().replace('|', "\\|");
        out.push_str(&format!("| `{usage}` | {} |\n", c.description));
    }
    out
}
