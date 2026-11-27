//! Reviewing AI changes in the app (ui-spec S3, L2-A): the list at the top of the session's
//! tab, the diff tab in the editor, Keep and Revert. The tracking itself is [`super::changes`].

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossterm::event::{KeyCode as K, KeyEvent};
use vr_core::FsEvent;

use super::changes::{
    Banner, Changes, Config, Diff, KeepError, Kind, LineKind, Record, RevertError, Reverted, Seen,
};
use crate::app::{App, Focus, View};
use crate::commands::dialog::DialogAction;
use crate::commands::file::CANCEL;
use crate::commands::registry::{CmdArgs, CmdResult, CommandSpec};
use crate::event_loop::chord_from_key;
use crate::ui::dialog::{ConfirmDialog, DialogWidget};

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "ai.changes",
        title: "AI: Show Changes",
        run: changes_cmd,
    },
    CommandSpec {
        name: "ai.keep",
        title: "AI: Keep Change",
        run: keep_cmd,
    },
    CommandSpec {
        name: "ai.revert",
        title: "AI: Revert Change",
        run: revert_cmd,
    },
];

/// Marks the diff tab's view; the tab bar shows `± <file name>`.
pub const DIFF_TITLE: &str = "±";

const REVERT: &str = "Revert";

/// The open changes list of the active session.
#[derive(Debug, Clone, Default)]
pub struct ChangeList {
    /// The list has the panel's keys (else the session below it).
    pub focused: bool,
    /// Index into [`rows`].
    pub selected: usize,
    /// First file row drawn.
    pub top: usize,
}

/// What the diff tab shows: one record of one session, as it was when the tab was filled.
pub struct DiffTab {
    /// [`super::AiSession::serial`].
    pub session: u64,
    pub rel: PathBuf,
    pub diff: Diff,
    pub syntax: DiffSyntax,
}

/// Highlighters of the two texts a diff compares, in the language of the file's name: a
/// removed line is coloured as part of the earlier text, the others as part of the file now
/// (ui-spec S3). They parse on their own worker threads, hold the texts in memory only, and
/// stop when the diff tab goes (closed, reloaded, its session closed, quit).
#[derive(Default)]
pub struct DiffSyntax {
    pub old: Option<vr_syntax::Highlighter>,
    pub new: Option<vr_syntax::Highlighter>,
}

impl DiffSyntax {
    fn of(app: &App, rel: &Path, diff: &Diff) -> DiffSyntax {
        let max = app.settings.max_highlight_bytes;
        let side = |text: &Option<Arc<str>>| {
            text.as_deref()
                .and_then(|t| app.syntax.standalone(rel, t, max))
        };
        DiffSyntax {
            old: side(&diff.old_text),
            new: side(&diff.new_text),
        }
    }
}

/// Waits until the diff tab's colours describe its texts (the UI never waits).
#[cfg(test)]
pub fn wait_syntax(app: &App) -> bool {
    let limit = std::time::Duration::from_secs(10);
    app.ai.diff.as_ref().is_none_or(|d| {
        [&d.syntax.old, &d.syntax.new]
            .into_iter()
            .flatten()
            .all(|hl| hl.wait(limit))
    })
}

/// Highlighters the diff tab holds.
#[cfg(test)]
pub fn diff_highlighters(app: &App) -> usize {
    app.ai.diff.as_ref().map_or(0, |d| {
        usize::from(d.syntax.old.is_some()) + usize::from(d.syntax.new.is_some())
    })
}

/// A selectable row of the list.
#[derive(Debug, Clone)]
pub enum Row {
    /// "List may be incomplete" (SEC-13); Enter hides it.
    Banner(Banner),
    File(Record),
}

/// A file's name as shown (SEC-13: no control or direction characters).
pub fn shown_name(rel: &Path) -> String {
    let name = rel.file_name().map_or_else(
        || rel.to_string_lossy().into_owned(),
        |n| n.to_string_lossy().into_owned(),
    );
    crate::ui::safe_text::sanitize(&name)
}

/// A root-relative path as shown.
pub fn shown_path(rel: &Path) -> String {
    crate::ui::safe_text::sanitize(&rel.to_string_lossy())
}

/// Starts tracking `root` for a session about to start (before its program does). `None`
/// when the store cannot be made: the session runs, its changes are not recorded.
pub fn start(app: &mut App, root: &Path) -> Option<Changes> {
    let base = app.ai.changes_dir.clone()?;
    let cfg = Config {
        root: root.to_path_buf(),
        store_base: base,
        total: Arc::clone(&app.ai.changes_total),
        limits: app.ai.changes_limits.clone(),
        excluded: app.ai.changes_excluded.clone(),
        own: app.watcher.as_ref().map(|w| w.own_writes()),
        wake: app.terminal.wake_handle(),
    };
    match Changes::start(cfg) {
        Ok(c) => Some(c),
        Err(e) => {
            let text = format!("AI changes are not recorded: {e}");
            if !app.notices.iter().any(|n| n.text == text) {
                app.warn(text);
            }
            None
        }
    }
}

/// Brings the sessions' lists in step with their trackers; an exited program's tracker stops
/// attributing. True when something shown changed.
pub fn refresh(app: &mut App) -> bool {
    let mut changed = false;
    let had_banner = app.ai.active_session().is_some_and(|s| s.banner.is_some());
    for s in &mut app.ai.sessions {
        let Some(c) = &s.changes else {
            continue;
        };
        if !s.tab.running() && c.running() {
            c.pause();
        }
        let v = c.version();
        if s.changes_version != Some(v) {
            s.changes_version = Some(v);
            s.records = c.records();
            s.banner = c.banner();
            changed = true;
        }
    }
    if app.ai.diff.is_some() && diff_index(app).is_none() {
        app.ai.diff = None;
    }
    if app.ai.active_session().is_none() && app.ai.list.is_some() && app.ai.sessions.is_empty() {
        app.ai.list = None;
    }
    let n = rows(app).len();
    let has_banner = app.ai.active_session().is_some_and(|s| s.banner.is_some());
    if let Some(l) = &mut app.ai.list {
        // The banner is the first row: the selected file stays the selected file.
        if has_banner && !had_banner {
            l.selected += 1;
        } else if had_banner && !has_banner {
            l.selected = l.selected.saturating_sub(1);
        }
        l.selected = l.selected.min(n.saturating_sub(1));
    }
    changed
}

/// Rows of the active session's list: the banner (if any), then its files.
pub fn rows(app: &App) -> Vec<Row> {
    let Some(s) = app.ai.active_session() else {
        return Vec::new();
    };
    s.banner
        .map(Row::Banner)
        .into_iter()
        .chain(s.records.iter().cloned().map(Row::File))
        .collect()
}

/// The list is open on a session and gets the panel's keys.
pub fn list_has_keys(app: &App) -> bool {
    super::has_keys(app)
        && app.ai.active_session().is_some()
        && app.ai.list.as_ref().is_some_and(|l| l.focused)
}

/// Index of the diff tab among the documents.
pub fn diff_index(app: &App) -> Option<usize> {
    app.ai.diff.as_ref()?;
    app.views.iter().position(|v| v.title == Some(DIFF_TITLE))
}

/// The diff tab is the active editor tab.
pub fn diff_shown(app: &App) -> bool {
    diff_index(app) == Some(app.active)
}

fn session_at(app: &App, serial: u64) -> Option<usize> {
    app.ai.sessions.iter().position(|s| s.serial == serial)
}

/// The record Keep / Revert act on: the list's selected file while the list has the keys,
/// the diff tab's while the editor has them on it; nothing anywhere else.
fn target(app: &App) -> Option<(usize, PathBuf)> {
    if list_has_keys(app) {
        let l = app.ai.list.as_ref()?;
        return match rows(app).into_iter().nth(l.selected)? {
            Row::File(r) => Some((app.ai.active, r.rel)),
            Row::Banner(_) => None,
        };
    }
    let on_editor = app.dialog.is_none() && matches!(app.focus, Focus::Editor | Focus::FindBar);
    if on_editor && diff_shown(app) {
        let d = app.ai.diff.as_ref()?;
        return Some((session_at(app, d.session)?, d.rel.clone()));
    }
    None
}

/// The record the status bar describes, with its place in its session's list.
pub fn status_target(app: &App) -> Option<(usize, Record, usize)> {
    let focus = app.palette.as_ref().map_or(app.focus, |p| p.prev_focus);
    let (si, rel) = if focus == Focus::Ai && app.ai.visible {
        let l = app.ai.list.as_ref().filter(|l| l.focused)?;
        match rows(app).into_iter().nth(l.selected)? {
            Row::File(r) => (app.ai.active, r.rel),
            Row::Banner(_) => return None,
        }
    } else if matches!(focus, Focus::Editor | Focus::FindBar) && diff_shown(app) {
        let d = app.ai.diff.as_ref()?;
        (session_at(app, d.session)?, d.rel.clone())
    } else {
        return None;
    };
    let records = &app.ai.sessions.get(si)?.records;
    let at = records.iter().position(|r| r.rel == rel)?;
    Some((si, records[at].clone(), at))
}

// ---- the list ---------------------------------------------------------------------------------

/// f6: opens the list on the active session with the keys; with the keys already, closes it.
fn changes_cmd(app: &mut App, _: CmdArgs) -> CmdResult {
    if app.ai.sessions.is_empty() {
        let key = app.key_hint("ai.new");
        app.info(format!("AI: no session is open ({key} starts one)"));
        return Ok(());
    }
    if list_has_keys(app) {
        close_list(app);
        return Ok(());
    }
    open_list(app);
    Ok(())
}

pub fn open_list(app: &mut App) {
    if app.ai.picker_active() {
        app.ai.active = app.ai.active.min(app.ai.sessions.len().saturating_sub(1));
        app.ai.picker = None;
    }
    refresh(app);
    let first_file = usize::from(app.ai.active_session().is_some_and(|s| s.banner.is_some()));
    let list = app.ai.list.get_or_insert_with(|| ChangeList {
        selected: first_file,
        ..ChangeList::default()
    });
    list.focused = true;
    super::reveal(app);
    refresh(app);
}

pub fn close_list(app: &mut App) {
    app.ai.list = None;
    app.dirty = true;
}

/// Keys of the focused list: up / down, Enter (diff, or hide the banner), Esc, and the Keep /
/// Revert keys of the keymap. Every other key stays here: it is not typed into the session.
pub fn handle_key(app: &mut App, key: &KeyEvent) -> bool {
    if !list_has_keys(app) {
        return false;
    }
    let chord = chord_from_key(key, app.keyboard_enhanced);
    for cmd in ["ai.keep", "ai.revert"] {
        if chord
            .as_ref()
            .is_some_and(|c| app.keymap.keys_for(cmd).contains(c))
        {
            app.run_command(cmd, CmdArgs::None);
            return true;
        }
    }
    if !key.modifiers.is_empty() {
        return true;
    }
    let all = rows(app);
    let Some(list) = &mut app.ai.list else {
        return true;
    };
    match key.code {
        K::Up => list.selected = list.selected.saturating_sub(1),
        K::Down => list.selected = (list.selected + 1).min(all.len().saturating_sub(1)),
        K::Esc => close_list(app),
        K::Enter => match all.into_iter().nth(list.selected) {
            Some(Row::Banner(_)) => dismiss_banner(app),
            Some(Row::File(r)) => {
                let i = app.ai.active;
                open_diff(app, i, &r.rel);
            }
            None => {}
        },
        _ => {}
    }
    app.dirty = true;
    true
}

fn dismiss_banner(app: &mut App) {
    if let Some(c) = app.ai.active_session().and_then(|s| s.changes.as_ref()) {
        c.dismiss_banner();
    }
    refresh(app);
}

/// A click on row `row` of [`rows`]: selected, and a file's diff opens.
pub fn click_row(app: &mut App, row: usize) {
    let all = rows(app);
    if let Some(l) = &mut app.ai.list {
        l.focused = true;
        if row < all.len() {
            l.selected = row;
        }
    }
    app.focus = Focus::Ai;
    if let Some(Row::File(r)) = all.into_iter().nth(row) {
        let i = app.ai.active;
        open_diff(app, i, &r.rel);
    }
    app.dirty = true;
}

/// Selects `rel` in the open list (tests pick a file by name).
#[cfg(test)]
pub fn select(app: &mut App, rel: &Path) -> bool {
    refresh(app);
    let at = rows(app)
        .iter()
        .position(|r| matches!(r, Row::File(f) if f.rel == rel));
    match (at, &mut app.ai.list) {
        (Some(i), Some(l)) => {
            l.selected = i;
            true
        }
        _ => false,
    }
}

// ---- the diff tab -----------------------------------------------------------------------------

/// The text of the diff tab's buffer: one buffer line per diff line.
fn diff_text(d: &Diff, label: &str) -> (String, Diff) {
    let mut d = d.clone();
    if d.missing.is_some() {
        let name = shown_name(&d.rel);
        let note = |text: String| super::changes::DiffLine {
            kind: LineKind::Note,
            num: None,
            text,
            at: None,
        };
        let head = [
            note(format!("vr-ide saw {name} only after {label} changed it:")),
            note("revert is off; this is the file as it is now.".to_string()),
            note(String::new()),
        ];
        d.lines.splice(0..0, head);
    }
    let mut text = String::new();
    for l in &d.lines {
        text.push_str(&l.text);
        text.push('\n');
    }
    (text, d)
}

/// Shows the diff of `rel` (session `si`) in the editor's diff tab, read-only: a buffer
/// without a file, so it never reaches recovery files, recent files or the session (SEC-13).
/// The focus stays where it is.
pub fn open_diff(app: &mut App, si: usize, rel: &Path) {
    let Some(s) = app.ai.sessions.get(si) else {
        return;
    };
    let Some(c) = &s.changes else {
        return;
    };
    let diff = match c.diff(rel) {
        Ok(d) => d,
        Err(e) => {
            app.error(format!("AI changes: {}: {e}", shown_name(rel)));
            return;
        }
    };
    let serial = s.serial;
    let (text, diff) = diff_text(&diff, &app.ai.label(si));
    let syntax = DiffSyntax::of(app, rel, &diff);
    close_diff(app);
    let mut doc = vr_core::Document::untitled();
    doc.buffer = vr_core::Buffer::from_str(&text);
    doc.read_only = true;
    app.docs.push(doc);
    app.views.push(View {
        follow_cursor: true,
        title: Some(DIFF_TITLE),
        ..Default::default()
    });
    app.ai.diff = Some(DiffTab {
        session: serial,
        rel: rel.to_path_buf(),
        diff,
        syntax,
    });
    app.set_active(app.docs.len() - 1);
    app.dirty = true;
}

/// Closes the diff tab, if any.
pub fn close_diff(app: &mut App) {
    if let Some(i) = app.views.iter().position(|v| v.title == Some(DIFF_TITLE)) {
        app.close_doc(i);
    }
    app.ai.diff = None;
}

fn shows(app: &App, serial: u64, rel: &Path) -> bool {
    app.ai
        .diff
        .as_ref()
        .is_some_and(|d| d.session == serial && d.rel == rel)
}

/// A session is being closed: its diff tab goes with its records.
pub fn session_closing(app: &mut App, serial: u64) {
    if app.ai.diff.as_ref().is_some_and(|d| d.session == serial) {
        close_diff(app);
    }
    app.ai.list = None;
}

// ---- keep, revert -----------------------------------------------------------------------------

/// f5: Keep drops the record without asking.
fn keep_cmd(app: &mut App, _: CmdArgs) -> CmdResult {
    let Some((si, rel)) = target(app) else {
        return Ok(());
    };
    let Some(s) = app.ai.sessions.get(si) else {
        return Ok(());
    };
    let serial = s.serial;
    let Some(c) = &s.changes else {
        return Ok(());
    };
    let name = shown_name(&rel);
    // What the diff tab shows is what is kept; without a tab, the file as it is now.
    let seen = match &app.ai.diff {
        Some(d) if d.session == serial && d.rel == rel => d.diff.seen,
        _ => {
            c.diff(&rel)
                .map_err(|e| anyhow::anyhow!("{name}: {e}"))?
                .seen
        }
    };
    match c.keep(&rel, &seen) {
        Ok(()) => {}
        Err(KeepError::Gone) => return Ok(()),
        Err(KeepError::Changed) => {
            open_diff(app, si, &rel);
            app.warn(format!(
                "Not kept: {name} changed since shown; diff updated"
            ));
            refresh(app);
            return Ok(());
        }
        Err(KeepError::Failed(e)) => {
            app.error(format!("Not kept: {name}: {e}"));
            return Ok(());
        }
    }
    if shows(app, serial, &rel) {
        close_diff(app);
    }
    refresh(app);
    app.info(format!("Kept {}", shown_name(&rel)));
    Ok(())
}

/// The open documents on the file `abs` (found by the real folder, whatever the spelling).
fn docs_on(app: &App, abs: &Path) -> Vec<usize> {
    let real = |p: &Path| {
        let dir = p.parent()?.canonicalize().ok()?;
        Some(dir.join(p.file_name()?))
    };
    let want = real(abs);
    app.docs
        .iter()
        .enumerate()
        .filter(|(_, d)| {
            d.path
                .as_deref()
                .is_some_and(|p| p == abs || (want.is_some() && real(p) == want))
        })
        .map(|(i, _)| i)
        .collect()
}

fn unsaved(app: &App, abs: &Path) -> bool {
    docs_on(app, abs)
        .into_iter()
        .any(|i| app.docs[i].is_modified())
}

/// shift+f5: Revert asks first, Cancel selected. The version of the file the diff tab shows
/// (taken now when no tab shows it) is what the answer may replace (SEC-12).
fn revert_cmd(app: &mut App, _: CmdArgs) -> CmdResult {
    let Some((si, rel)) = target(app) else {
        return Ok(());
    };
    refresh(app);
    let Some(s) = app.ai.sessions.get(si) else {
        return Ok(());
    };
    let (Some(c), Some(record)) = (&s.changes, s.records.iter().find(|r| r.rel == rel)) else {
        return Ok(());
    };
    let name = shown_name(&rel);
    if !record.can_revert() {
        app.warn(format!("No earlier version of {name} kept; Keep only"));
        return Ok(());
    }
    if unsaved(app, &c.root().join(&rel)) {
        app.warn(format!(
            "Not reverted: {name} has unsaved edits in the editor"
        ));
        return Ok(());
    }
    let serial = s.serial;
    let diff = match &app.ai.diff {
        Some(d) if d.session == serial && d.rel == rel => d.diff.clone(),
        _ => c.diff(&rel).map_err(|e| anyhow::anyhow!("{name}: {e}"))?,
    };
    let label = app.ai.label(si);
    let at = crate::ui::ai_changes::clock(diff.since);
    let since = format!("Edits by other programs since {at} count as {label}'s.");
    let (message, note) = match diff.kind {
        Kind::Modified => (
            format!(
                "Writes back {name} as it was before {label} changed it ({at}). Today's text, \
                 +{} -{}, is lost.",
                diff.added, diff.removed
            ),
            since,
        ),
        Kind::Created => (
            format!(
                "{label} created this file. Revert moves it to the trash ({} lines).",
                diff.added
            ),
            format!("Other programs' edits there count as {label}'s."),
        ),
        Kind::Deleted => (
            format!(
                "Restores {name} ({} lines) that {label} deleted.",
                diff.removed
            ),
            since,
        ),
    };
    let mut dialog = ConfirmDialog::new(format!("Revert {name}?"), message, &[REVERT, CANCEL]);
    dialog.note = Some(note);
    dialog.selected = 1;
    app.open_dialog(
        DialogWidget::Confirm(dialog),
        DialogAction::AiRevert {
            session: serial,
            rel,
            seen: diff.seen,
        },
    );
    Ok(())
}

/// Revert was chosen: the file is replaced only while it is still the version `seen`.
pub fn answer_revert(app: &mut App, serial: u64, rel: &Path, seen: &Seen) {
    let Some(si) = session_at(app, serial) else {
        return;
    };
    let name = shown_name(rel);
    let Some(c) = &app.ai.sessions[si].changes else {
        return;
    };
    let abs = c.root().join(rel);
    if unsaved(app, &abs) {
        app.warn(format!(
            "Not reverted: {name} has unsaved edits in the editor"
        ));
        return;
    }
    let result = c.revert(rel, seen, &*app.trash, &mut |_| {});
    match result {
        Ok(done) => {
            if shows(app, serial, rel) {
                close_diff(app);
            }
            // An open tab of the file follows like after any outside change.
            for i in docs_on(app, &abs) {
                if let Some(p) = app.docs.get(i).and_then(|d| d.path.clone()) {
                    app.handle_fs_event(match done {
                        Reverted::Restored => FsEvent::Modified(p),
                        Reverted::Trashed
                        | Reverted::TrashedChanged
                        | Reverted::TrashedUnchecked => FsEvent::Removed(p),
                    });
                }
            }
            match done {
                Reverted::Restored => app.info(format!("Reverted {name}")),
                Reverted::Trashed => app.info(format!("Reverted {name}: moved to the trash")),
                Reverted::TrashedChanged => app.warn(format!(
                    "Reverted {name}: it was written to while it moved to the trash; the trash \
                     holds that newer version"
                )),
                Reverted::TrashedUnchecked => app.warn(format!(
                    "Reverted {name}: moved to the trash, but it was replaced or copied on the \
                     way; the trash may hold a version that was not shown"
                )),
            }
        }
        Err(RevertError::Folder) => {
            app.warn(format!("Not reverted: a folder is now at {name}"));
        }
        Err(RevertError::Changed) => {
            open_diff(app, si, rel);
            app.warn(format!(
                "Not reverted: {name} changed since shown; diff updated"
            ));
        }
        Err(RevertError::NoEarlier) => {
            app.warn(format!("No earlier version of {name} kept; Keep only"));
        }
        Err(RevertError::Outside) => app.error(format!(
            "Not reverted: the folder of {name} is no longer inside the session's folder"
        )),
        Err(RevertError::Gone) => {}
        Err(RevertError::Failed(e)) => app.error(format!("Not reverted: {name}: {e}")),
    }
    refresh(app);
}
