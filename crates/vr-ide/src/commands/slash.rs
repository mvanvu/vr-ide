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
use crate::workspace::Workspace;

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
}

impl ArgKind {
    pub fn required(self) -> bool {
        matches!(
            self,
            ArgKind::Path { required: true, .. } | ArgKind::Text | ArgKind::Choice(_)
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
    cmd("term new", "", NO_ARG, "Open a new terminal tab", term_new),
    cmd(
        "term close",
        "",
        NO_ARG,
        "Close the active terminal tab",
        term_close,
    ),
    cmd(
        "term right",
        "",
        NO_ARG,
        "Terminal panel on the right (not available yet)",
        term_position,
    ),
    cmd(
        "term bottom",
        "",
        NO_ARG,
        "Terminal panel at the bottom (not available yet)",
        term_position,
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
        "wrap",
        "",
        NO_ARG,
        "Toggle soft line wrap (not available yet)",
        wrap,
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
    // A listed choice with nothing typed yet ("/ws recent "): the highlighted value.
    if let Some((c, "")) = in_argument(input)
        && matches!(c.arg, ArgKind::Choice(_) | ArgKind::Root | ArgKind::Recent)
        && let Some(row) = rows.get(selected)
        && let Ok((c, arg)) = parse(&row.fill)
        && !arg.is_empty()
    {
        return Submit::Run(c, arg);
    }
    let err = match parse(input) {
        Ok((c, arg)) => return Submit::Run(c, arg),
        Err(e) => e,
    };
    let picks = match lookup(input) {
        // Nothing typed after the slash: no command is guessed.
        None => input.trim() != "/",
        Some((c, arg)) => {
            arg.is_empty() && matches!(c.arg, ArgKind::Choice(_) | ArgKind::Root | ArgKind::Recent)
        }
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
    }
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
    let Some((c, arg)) = in_argument(input) else {
        return;
    };
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

fn set_input(app: &mut App, text: &str) {
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
    if let Some(next) = complete(&text, &rows(app, &text), selected)
        && next != text
    {
        set_input(app, &next);
    }
}

/// Enter in slash mode: runs the command after closing the window; a refused input or a
/// command that fails leaves the window open with the text and the message under it.
pub fn enter(app: &mut App) {
    let Some((text, selected)) = window_state(app) else {
        return;
    };
    prepare(app, &text);
    match submit(&text, &rows(app, &text), selected) {
        Submit::Run(c, arg) => {
            palette::close(app);
            if let Err(e) = (c.run)(app, &arg) {
                reopen(app, &text, e);
            }
            app.dirty = true;
        }
        Submit::Fill(fill) => set_input(app, &fill),
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

fn term_new(app: &mut App, _: &str) -> Result<(), String> {
    app.run_command("terminal.new", CmdArgs::None);
    Ok(())
}

fn term_close(app: &mut App, _: &str) -> Result<(), String> {
    if app.terminal.tabs.is_empty() {
        return Err("no terminal is open".into());
    }
    app.run_command("terminal.close", CmdArgs::None);
    Ok(())
}

/// Listed now; the panel position is task T19's.
fn term_position(_: &mut App, _: &str) -> Result<(), String> {
    Err("available after the terminal position feature".into())
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
    let id = app.add_root(&path).map_err(|e| e.to_string())?;
    if let Some(r) = app.workspace.get(id) {
        let msg = format!("workspace: added {}", r.name);
        app.info(msg);
    }
    Ok(())
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
    let (ws, refused) = Workspace::from_file(&resolve_arg(app, arg)).map_err(|e| e.to_string())?;
    open_workspace(app, ws, refused);
    Ok(())
}

fn open_workspace(app: &mut App, ws: Workspace, refused: Vec<crate::workspace::WorkspaceError>) {
    for e in refused {
        app.warn(format!("workspace: {e}"));
    }
    app.open_workspace(ws);
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
    let (ws, refused) = match &entry.file {
        Some(f) => Workspace::from_file(f),
        None => Workspace::from_paths(&entry.roots),
    }
    .map_err(|e| e.to_string())?;
    open_workspace(app, ws, refused);
    Ok(())
}

fn theme(app: &mut App, name: &str) -> Result<(), String> {
    app.settings.theme = name.to_string();
    app.info(format!(
        "theme {name} for this session (settings.toml `theme` keeps it)"
    ));
    Ok(())
}

/// Listed now; the editor has no soft wrap yet.
fn wrap(_: &mut App, _: &str) -> Result<(), String> {
    Err("available after the soft wrap feature".into())
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
