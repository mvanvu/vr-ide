//! `file.*` commands. Every write to a user file goes through `Document::save*`.

use std::path::{Path, PathBuf};

use vr_core::{DiskState, DiskVersion, DocError};

use super::dialog::{DialogAction, DocRef, reopen_with_error};
use super::registry::{CmdArgs, CmdResult, CommandSpec};
use crate::app::App;
use crate::ui::dialog::{ConfirmDialog, DialogWidget, PromptDialog};

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "file.save",
        title: "Save",
        run: save,
    },
    CommandSpec {
        name: "file.save_overwrite",
        title: "Save (overwrite)",
        run: save_overwrite,
    },
    CommandSpec {
        name: "file.save_as",
        title: "Save As",
        run: save_as,
    },
    CommandSpec {
        name: "file.close",
        title: "Close File",
        run: close,
    },
    CommandSpec {
        name: "file.open",
        title: "Open File",
        run: open,
    },
    CommandSpec {
        name: "file.reload",
        title: "Reload File from Disk",
        run: reload,
    },
];

fn save(app: &mut App, _: CmdArgs) -> CmdResult {
    if !app.docs.is_empty() {
        save_doc(app, app.active);
    }
    Ok(())
}

/// What [`save_doc`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveOutcome {
    Saved,
    /// The file changed on disk: the external-change prompt is open instead (AC-7).
    Prompted,
    /// An error notice was shown.
    Failed,
}

/// Saves document `i` through `Document::save`. A file changed on disk is never replaced
/// here: the external-change prompt opens and the user decides (D64).
pub fn save_doc(app: &mut App, i: usize) -> SaveOutcome {
    let Some(doc) = app.docs.get(i) else {
        return SaveOutcome::Failed;
    };
    let name = doc
        .path
        .as_deref()
        .map_or_else(|| "untitled".to_string(), |p| app.display_path(p));
    let path = app.docs[i].path.clone();
    own_save(app, path.as_deref(), true);
    let doc = &mut app.docs[i];
    doc.buffer.break_undo_group();
    let result = doc.save();
    if matches!(result, Err(DocError::ChangedOnDisk)) {
        own_save(app, path.as_deref(), false);
    }
    match result {
        Ok(saved) => {
            if let Some(p) = app.docs[i].path.clone() {
                super::explorer::note_written(app, &p);
                // settings.toml / keymap.toml apply when saved (T24).
                app.config_file_changed(&p, true);
            }
            hard_link_notice(app, &name, saved);
            SaveOutcome::Saved
        }
        Err(DocError::ChangedOnDisk) => {
            prompt_external_change(app, i, true);
            SaveOutcome::Prompted
        }
        Err(DocError::NoPath) => {
            app.error(format!("{name}: has no file name; use Save As"));
            SaveOutcome::Failed
        }
        Err(e) => {
            app.error(format!("Save: {name}: {e}"));
            SaveOutcome::Failed
        }
    }
}

/// Tells the AI sessions' change trackers about a save of the editor: before it (`begins`),
/// and again when it replaced nothing.
fn own_save(app: &App, path: Option<&Path>, begins: bool) {
    if let Some(p) = path {
        crate::ai::review::own_save(app, p, begins);
    }
}

fn hard_link_notice(app: &mut App, name: &str, saved: vr_core::Saved) {
    if saved.other_hard_links > 0 {
        app.warn(format!(
            "{name} had {} other hard link(s); they keep the old content",
            saved.other_hard_links
        ));
    }
}

/// Opens the external-change prompt for document `i` (AC-7). The disk version is taken
/// first, before anything else is read, so "Keep mine" replaces only that version (D64).
pub fn prompt_external_change(app: &mut App, i: usize, on_save: bool) {
    prompt_external_change_with(app, i, on_save, None);
}

/// [`prompt_external_change`] with `error` (why the last choice wrote nothing) shown first in
/// the prompt. When no version can be read now, the prompt still opens: Keep mine then writes
/// nothing and reads again.
fn prompt_external_change_with(app: &mut App, i: usize, on_save: bool, error: Option<String>) {
    let Some(path) = app.docs.get(i).and_then(|d| d.path.clone()) else {
        return;
    };
    let name = app.display_path(&path);
    let (seen, unread) = match doc_version(&app.docs[i]) {
        Ok(v) => (Some(v), None),
        Err(e) => (
            None,
            Some(format!(
                "{name}: {e}. Keep mine writes nothing until the file can be read."
            )),
        ),
    };
    if let Some(d) = app.docs.get_mut(i) {
        d.disk_state = DiskState::ChangedOnDisk;
    }
    let (message, buttons, default): (String, &[&str], usize) = if on_save {
        (
            format!(
                "{name} changed on disk since it was opened or saved. Keep mine replaces that \
                 version with yours; Reload takes the disk version (yours goes to the \
                 recovery folder first)."
            ),
            &[KEEP_MINE, RELOAD, CANCEL],
            2,
        )
    } else {
        (
            format!(
                "{name} changed on disk and you have unsaved changes. Reload takes the disk \
                 version (yours goes to the recovery folder first); Keep mine keeps your \
                 text, and saving asks again before replacing the file."
            ),
            &[RELOAD, KEEP_MINE],
            1,
        )
    };
    let message = with_errors(message, [error, unread]);
    let mut dialog = ConfirmDialog::new("File Changed on Disk", message, buttons);
    dialog.selected = default;
    app.open_dialog(
        DialogWidget::Confirm(dialog),
        DialogAction::ExternalChange {
            doc: DocRef::of(app, i),
            seen,
            on_save,
        },
    );
}

pub const KEEP_MINE: &str = "Keep mine";
pub const RELOAD: &str = "Reload";
pub const CANCEL: &str = "Cancel";

/// Same as Save: a file changed on disk is replaced only after the prompt (D64).
fn save_overwrite(app: &mut App, args: CmdArgs) -> CmdResult {
    save(app, args)
}

/// Without a path, asks for the file name ([`answer_save_as`]). With `CmdArgs::Path`, writes
/// there (the caller has confirmed replacing an existing file).
fn save_as(app: &mut App, args: CmdArgs) -> CmdResult {
    if app.docs.is_empty() {
        return Ok(());
    }
    let CmdArgs::Path(target) = args else {
        let name = app.active_name();
        app.open_dialog(
            DialogWidget::Prompt(PromptDialog::new(
                "Save As",
                "File name (relative to the project folder)",
                &name,
            )),
            DialogAction::SaveAs(DocRef::of(app, app.active)),
        );
        return Ok(());
    };
    let target = app.resolve(&target);
    if is_own_path(app, app.active, &target) {
        save_doc(app, app.active);
        return Ok(());
    }
    match save_as_doc(app, app.active, &target, None) {
        Ok(()) => Ok(()),
        Err(SaveAsError::Changed) => Err(anyhow::anyhow!(
            "{} changed on disk while saving; nothing was replaced",
            app.display_path(&target)
        )),
        Err(SaveAsError::Failed(e)) => Err(anyhow::Error::msg(e)),
    }
}

/// The typed name: refused when empty or a folder; an existing other file needs a
/// Replace / Cancel confirmation first.
pub fn answer_save_as(app: &mut App, widget: DialogWidget, doc: DocRef, text: &str) {
    let Some(i) = doc.find(app) else {
        return;
    };
    let retry = |app: &mut App, w: DialogWidget, e: String| {
        reopen_with_error(app, w, DialogAction::SaveAs(doc.clone()), e);
    };
    if text.trim().is_empty() {
        return retry(app, widget, "type a file name".to_string());
    }
    let target = app.resolve(std::path::Path::new(text.trim()));
    if target.is_dir() {
        return retry(
            app,
            widget,
            format!("{} is a folder", app.display_path(&target)),
        );
    }
    // The own path (also spelled differently or reached through a link) is a plain Save:
    // the snapshot check and the D81 prompt guard it like ctrl+s (D64).
    if is_own_path(app, i, &target) {
        save_doc(app, i);
        return;
    }
    if let Err(e) = check_other_tabs(app, i, &target) {
        return retry(app, widget, e);
    }
    // The version is taken before anything is shown: Replace (or the write of a new name)
    // replaces only this version and never one written later (D87).
    let seen = match take_version(&target) {
        Ok(v) => v,
        Err(e) => return retry(app, widget, format!("{}: {e}", app.display_path(&target))),
    };
    if target.symlink_metadata().is_ok() {
        return open_replace(app, i, target, Some(seen), None);
    }
    match save_as_doc(app, i, &target, Some(&seen)) {
        Ok(()) => {}
        // Created on disk since the name was typed: ask before replacing it.
        Err(SaveAsError::Changed) => {
            let msg = format!(
                "{} was created on disk meanwhile; nothing was replaced",
                app.display_path(&target)
            );
            app.warn(msg.clone());
            reopen_replace(app, i, target, msg);
        }
        Err(SaveAsError::Failed(e)) => retry(app, widget, e),
    }
}

pub const REPLACE: &str = "Replace";

/// Replace / Cancel for an existing Save As target (default Cancel); `seen` is the version
/// the user is asked about, `error` why the last choice wrote nothing (shown first).
fn open_replace(
    app: &mut App,
    i: usize,
    target: PathBuf,
    seen: Option<DiskVersion>,
    error: Option<String>,
) {
    let name = app.display_path(&target);
    let message = match seen {
        Some(_) => format!("{name} already exists. Replace it?"),
        None => format!(
            "{name} already exists. Replace writes nothing until the file can be read; \
             choose it to read the file again."
        ),
    };
    let mut confirm = ConfirmDialog::new(
        "Replace File",
        with_errors(message, [error]),
        &[REPLACE, CANCEL],
    );
    confirm.selected = 1;
    app.open_dialog(
        DialogWidget::Confirm(confirm),
        DialogAction::ReplaceFile {
            doc: DocRef::of(app, i),
            target,
            seen,
        },
    );
}

/// The Replace prompt again with `error`, about the version on disk now; when none can be
/// read (the file keeps changing, D78) it says so and Replace stays refused.
fn reopen_replace(app: &mut App, i: usize, target: PathBuf, error: String) {
    match take_version(&target) {
        Ok(seen) => open_replace(app, i, target, Some(seen), Some(error)),
        Err(e) => {
            let error = format!("{error}\n{}: {e}", app.display_path(&target));
            open_replace(app, i, target, None, Some(error));
        }
    }
}

/// Replace chosen: writes only over the version the prompt was about. Any refusal (a newer
/// version, a conflict copy, an error, no version read) brings the prompt back with the
/// reason in it; nothing is written without a version the user was asked about.
pub fn answer_replace(app: &mut App, doc: &DocRef, target: PathBuf, seen: Option<&DiskVersion>) {
    let Some(i) = doc.find(app) else {
        return;
    };
    let name = app.display_path(&target);
    let Some(seen) = seen else {
        return reopen_replace(
            app,
            i,
            target,
            format!("{name}: read again; nothing was replaced"),
        );
    };
    match save_as_doc(app, i, &target, Some(seen)) {
        Ok(()) => {}
        Err(SaveAsError::Changed) => {
            let msg = format!("{name} changed on disk; nothing was replaced");
            app.warn(msg.clone());
            reopen_replace(app, i, target, msg);
        }
        Err(SaveAsError::Failed(e)) => reopen_replace(app, i, target, e),
    }
}

/// `message` with the non-empty `errors` in front (each on its own line).
fn with_errors<const N: usize>(message: String, errors: [Option<String>; N]) -> String {
    let mut lines: Vec<String> = errors.into_iter().flatten().collect();
    if lines.is_empty() {
        return message;
    }
    lines.push(String::new());
    lines.push(message);
    lines.join("\n")
}

/// The version of `path` now (`DiskVersion::of_path`; the test seam can make it fail).
fn take_version(path: &Path) -> Result<DiskVersion, DocError> {
    #[cfg(test)]
    if seam::version_fails() {
        return Err(keeps_changing());
    }
    DiskVersion::of_path(path)
}

/// The version of document `doc`'s file now (`Document::disk_version`).
fn doc_version(doc: &vr_core::Document) -> Result<DiskVersion, DocError> {
    #[cfg(test)]
    if seam::version_fails() {
        return Err(keeps_changing());
    }
    doc.disk_version()
}

#[cfg(test)]
fn keeps_changing() -> DocError {
    DocError::Io(std::io::Error::other(
        "the file keeps changing on disk; try again",
    ))
}

/// An error the test seam injects in place of a commit's result.
fn injected_commit_error() -> Option<DocError> {
    #[cfg(test)]
    if let Some(msg) = seam::commit_error() {
        return Some(DocError::Io(std::io::Error::other(msg)));
    }
    None
}

/// Why [`save_as_doc`] wrote nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveAsError {
    /// The target is no longer the version that was seen; nothing was replaced.
    Changed,
    /// The message to show.
    Failed(String),
}

/// True when `target` names the file of document `i`: the same path, or the same location
/// once links and `.`/`..` are resolved.
fn is_own_path(app: &App, i: usize, target: &Path) -> bool {
    let Some(own) = app.docs.get(i).and_then(|d| d.path.as_deref()) else {
        return false;
    };
    own == target || location(own).is_some_and(|l| location(target) == Some(l))
}

/// Other tabs open on `target` (the same location).
fn other_tabs_on(app: &App, i: usize, target: &Path) -> Vec<usize> {
    (0..app.docs.len())
        .filter(|&j| j != i && is_own_path(app, j, target))
        .collect()
}

/// Save As must not replace a file whose other tab holds unsaved changes.
fn check_other_tabs(app: &App, i: usize, target: &Path) -> Result<(), String> {
    match other_tabs_on(app, i, target)
        .into_iter()
        .find(|&j| app.docs[j].is_modified())
    {
        Some(_) => Err(format!(
            "{} is open with unsaved changes; save or close that tab first",
            app.display_path(target)
        )),
        None => Ok(()),
    }
}

/// Canonical location of a path whose file may not exist (its folder must).
fn location(p: &Path) -> Option<PathBuf> {
    p.canonicalize()
        .ok()
        .or_else(|| Some(p.parent()?.canonicalize().ok()?.join(p.file_name()?)))
}

/// Writes document `i` to `target` and makes it the document's path. Another (unmodified)
/// tab on `target` is closed afterwards, so one path never has two tabs. With `seen`, only
/// that version of `target` is replaced (`Document::save_as_seen`); without, the version
/// found when the write begins (`Document::save_as`, D89).
pub fn save_as_doc(
    app: &mut App,
    i: usize,
    target: &Path,
    seen: Option<&DiskVersion>,
) -> Result<(), SaveAsError> {
    let name = app.display_path(target);
    check_other_tabs(app, i, target).map_err(SaveAsError::Failed)?;
    if i >= app.docs.len() {
        return Ok(());
    }
    // The tab takes `target`: a running project replace must leave that file alone.
    let claim = super::search::before_tab_opens(app, target).map_err(SaveAsError::Failed)?;
    own_save(app, Some(target), true);
    let doc = &mut app.docs[i];
    let old = doc.path.clone();
    doc.buffer.break_undo_group();
    let result = match (seen, injected_commit_error()) {
        (Some(_), Some(e)) => Err(e),
        (Some(seen), None) => doc.save_as_seen(target, seen),
        (None, _) => doc.save_as(target),
    };
    if matches!(result, Err(DocError::ChangedOnDisk)) {
        own_save(app, Some(target), false);
    }
    if result.is_err()
        && let Some((gate, c)) = &claim
    {
        gate.open_failed(c);
    }
    let saved = match result {
        Ok(saved) => saved,
        Err(DocError::ChangedOnDisk) => return Err(SaveAsError::Changed),
        Err(e) => return Err(SaveAsError::Failed(format!("{name}: {e}"))),
    };
    // Close the other tabs before watching the new path: closing one unwatches its path.
    for j in other_tabs_on(app, i, target).into_iter().rev() {
        app.close_doc(j);
    }
    app.rewatch(old.as_deref(), target);
    super::explorer::note_written(app, target);
    hard_link_notice(app, &name, saved);
    app.config_file_changed(target, true);
    app.dirty = true;
    Ok(())
}

/// Closing a tab with unsaved changes asks: Save / Discard / Cancel ([`answer_close`]).
fn close(app: &mut App, _: CmdArgs) -> CmdResult {
    request_close(app, app.active);
    Ok(())
}

/// ctrl+w on tab `i` and the tab's x button: closes it or asks first; the active tab stays.
pub fn request_close(app: &mut App, i: usize) {
    let Some(doc) = app.docs.get(i) else {
        return;
    };
    if !doc.is_modified() {
        app.close_doc(i);
        return;
    }
    let name = doc
        .path
        .as_deref()
        .map(|p| app.display_path(p))
        .unwrap_or_default();
    let message =
        format!("{name} has unsaved changes. Discard keeps a copy in the recovery folder.");
    app.open_dialog(
        DialogWidget::Confirm(ConfirmDialog::new(
            "Close File",
            message,
            &[SAVE, crate::commands::app::DISCARD, CANCEL],
        )),
        DialogAction::CloseTab(DocRef::of(app, i)),
    );
}

pub const SAVE: &str = "Save";

pub fn answer_close(app: &mut App, doc: &DocRef, button: usize) {
    let Some(i) = doc.find(app) else {
        return;
    };
    match button {
        0 => {
            if save_doc(app, i) == SaveOutcome::Saved {
                app.close_doc(i);
            }
        }
        1 => discard_and_close(app, i),
        _ => {}
    }
}

/// Closes tab `i` after writing its unsaved changes to the recovery folder; when that write
/// fails the tab stays open and the error is shown.
pub fn discard_and_close(app: &mut App, i: usize) {
    match app.keep_discarded(&[i]) {
        Ok(files) => {
            if let Some(f) = files.first() {
                app.info(format!("unsaved changes kept in {}", f.display()));
            }
            app.close_doc(i);
        }
        Err(e) => app.error(e),
    }
}

/// Without a path, asks for one ([`answer_open`]).
fn open(app: &mut App, args: CmdArgs) -> CmdResult {
    match args {
        CmdArgs::Path(p) => {
            if let Err(e) = app.open_path(&p) {
                super::search::report_open_error(app, &e);
            }
            Ok(())
        }
        CmdArgs::None => {
            app.open_dialog(
                DialogWidget::Prompt(PromptDialog::new(
                    "Open File",
                    "File path (relative to the project folder)",
                    "",
                )),
                DialogAction::OpenFile,
            );
            Ok(())
        }
    }
}

pub fn answer_open(app: &mut App, widget: DialogWidget, text: &str) {
    let err = if text.trim().is_empty() {
        Some("type a file path".to_string())
    } else {
        app.open_path(std::path::Path::new(text.trim()))
            .err()
            .map(|e| e.to_string())
    };
    if let Some(e) = err {
        reopen_with_error(app, widget, DialogAction::OpenFile, e);
    }
}

/// Replaces the buffer with the file on disk ([`reload_doc`]).
fn reload(app: &mut App, _: CmdArgs) -> CmdResult {
    if !app.docs.is_empty() {
        reload_doc(app, app.active);
    }
    Ok(())
}

/// Answer of the external-change prompt (AC-7). From an event: Reload / Keep mine (keeps the
/// buffer, the tab stays marked). On save: Keep mine (replaces only the version shown, D64;
/// a newer one asks again) / Reload / Cancel.
pub fn answer_external_change(
    app: &mut App,
    doc: &DocRef,
    seen: Option<&DiskVersion>,
    on_save: bool,
    button: usize,
) {
    let Some(i) = doc.find(app) else {
        return;
    };
    let (reload, keep) = if on_save { (1, 0) } else { (0, 1) };
    if button == reload {
        reload_doc(app, i);
    } else if button == keep && on_save {
        keep_mine(app, i, seen);
    }
}

/// Keep mine on the save prompt: replaces only `seen`. Any refusal (a newer version, a
/// conflict copy, an error, no version read) brings the prompt back with the reason in it.
fn keep_mine(app: &mut App, i: usize, seen: Option<&DiskVersion>) {
    let name = app.docs[i]
        .path
        .as_deref()
        .map(|p| app.display_path(p))
        .unwrap_or_default();
    let Some(seen) = seen else {
        let msg = format!("{name}: read again; nothing was replaced");
        return prompt_external_change_with(app, i, true, Some(msg));
    };
    let path = app.docs[i].path.clone();
    own_save(app, path.as_deref(), true);
    let doc = &mut app.docs[i];
    doc.buffer.break_undo_group();
    let result = match injected_commit_error() {
        Some(e) => Err(e),
        None => doc.save_overwrite_seen(seen),
    };
    if matches!(result, Err(DocError::ChangedOnDisk)) {
        own_save(app, path.as_deref(), false);
    }
    match result {
        Ok(saved) => {
            if let Some(p) = app.docs[i].path.clone() {
                super::explorer::note_written(app, &p);
                // settings.toml / keymap.toml apply when saved (T24).
                app.config_file_changed(&p, true);
            }
            hard_link_notice(app, &name, saved);
        }
        Err(DocError::ChangedOnDisk) => {
            let msg = format!("{name} changed again on disk; nothing was replaced");
            app.warn(msg.clone());
            prompt_external_change_with(app, i, true, Some(msg));
        }
        Err(e) => prompt_external_change_with(app, i, true, Some(format!("Save: {name}: {e}"))),
    }
}

/// Replaces buffer `i` with the file on disk as one undo step. Unsaved edits are written to
/// the recovery folder first (undo alone is lost on close or quit); when that write fails
/// nothing is reloaded.
pub fn reload_doc(app: &mut App, i: usize) {
    match app.keep_discarded(&[i]) {
        Ok(files) => {
            if let Some(f) = files.first() {
                app.info(format!("unsaved changes kept in {}", f.display()));
            }
        }
        Err(e) => return app.error(e),
    }
    if let Some(doc) = app.docs.get_mut(i) {
        match doc.reload() {
            Ok(()) => app.syntax.reset(i),
            Err(e) => app.error(format!("Reload: {e}")),
        }
    }
    app.cursor_moved();
}

/// The file of modified document `i` was deleted on disk: Save (recreate) / Close.
pub fn prompt_deleted(app: &mut App, i: usize) {
    let name = app.docs[i]
        .path
        .as_deref()
        .map(|p| app.display_path(p))
        .unwrap_or_default();
    let message = format!(
        "{name} was deleted on disk and you have unsaved changes. Close keeps a copy in the \
         recovery folder; Keep (or Esc) keeps the tab open and writes nothing."
    );
    // The prompt opens by itself, so its default writes nothing (D80).
    let mut dialog = ConfirmDialog::new("File Deleted on Disk", message, &[RECREATE, CLOSE, KEEP]);
    dialog.selected = 2;
    app.open_dialog(
        DialogWidget::Confirm(dialog),
        DialogAction::Deleted(DocRef::of(app, i)),
    );
}

pub const RECREATE: &str = "Save (recreate)";
pub const CLOSE: &str = "Close";
pub const KEEP: &str = "Keep";

pub fn answer_deleted(app: &mut App, doc: &DocRef, button: usize) {
    let Some(i) = doc.find(app) else {
        return;
    };
    match button {
        0 => {
            save_doc(app, i);
        }
        1 => discard_and_close(app, i),
        _ => {}
    }
}

/// Test seam: makes the next version reads or the next commit fail, as a writer that keeps
/// rewriting a file (D78) or a commit that ends with a conflict copy would.
#[cfg(test)]
pub(crate) mod seam {
    use std::cell::{Cell, RefCell};

    thread_local! {
        static FAIL_VERSIONS: Cell<u32> = const { Cell::new(0) };
        static COMMIT_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
    }

    pub(crate) fn fail_next_versions(n: u32) {
        FAIL_VERSIONS.with(|c| c.set(n));
    }

    pub(crate) fn fail_next_commit(msg: &str) {
        COMMIT_ERROR.with(|c| *c.borrow_mut() = Some(msg.to_string()));
    }

    pub(super) fn version_fails() -> bool {
        FAIL_VERSIONS.with(|c| {
            let n = c.get();
            c.set(n.saturating_sub(1));
            n > 0
        })
    }

    pub(super) fn commit_error() -> Option<String> {
        COMMIT_ERROR.with(|c| c.borrow_mut().take())
    }
}
