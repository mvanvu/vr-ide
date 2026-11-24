//! What each dialog's answer does. The widgets live in `ui::dialog`; commands open them with
//! `App::open_dialog(widget, action)` and get the answer here.

use std::path::PathBuf;

use crate::app::{App, Dialog};
use crate::ui::dialog::{DialogWidget, Outcome};

/// A document a dialog is about. Tabs can close or move while the dialog is open, so it is
/// found again by path (index first, as a fast path).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocRef {
    pub index: usize,
    pub path: Option<PathBuf>,
}

impl DocRef {
    pub fn of(app: &App, index: usize) -> DocRef {
        DocRef {
            index,
            path: app.docs.get(index).and_then(|d| d.path.clone()),
        }
    }

    /// Current index of the document, if it is still open.
    pub fn find(&self, app: &App) -> Option<usize> {
        if app
            .docs
            .get(self.index)
            .is_some_and(|d| d.path == self.path)
        {
            return Some(self.index);
        }
        self.path
            .as_ref()
            .and_then(|p| app.docs.iter().position(|d| d.path.as_ref() == Some(p)))
    }
}

/// The action behind an open dialog; each variant has a match arm in
/// [`answer`].
#[derive(Debug, Clone)]
pub enum DialogAction {
    /// Unsaved files on quit: Save all / Discard / Cancel.
    Quit,
    /// Unsaved file on close: Save / Discard / Cancel.
    CloseTab(DocRef),
    /// File name prompt for Save As.
    SaveAs(DocRef),
    /// Save As target exists: Replace / Cancel. `seen` is the target's version when the
    /// prompt opened; Replace overwrites only that one (D87). `None`: no version could be
    /// read (the file kept changing); Replace then writes nothing, reads again and asks again.
    ReplaceFile {
        doc: DocRef,
        target: PathBuf,
        seen: Option<vr_core::DiskVersion>,
    },
    /// File name prompt for Open File.
    OpenFile,
    /// line[:col] prompt.
    GotoLine,
    /// AC-7: the file changed on disk while the buffer has unsaved changes. `seen` is the
    /// disk version the prompt is about (D64); `on_save` when the user was saving. `None`:
    /// no version could be read; Keep mine then writes nothing, reads again and asks again.
    ExternalChange {
        doc: DocRef,
        seen: Option<vr_core::DiskVersion>,
        on_save: bool,
    },
    /// The file was deleted on disk while the buffer has unsaved changes.
    Deleted(DocRef),
    /// Explorer: name of a new file or folder in `dir`.
    NewEntry { dir: PathBuf, folder: bool },
    /// Explorer: new name for `path`.
    Rename(PathBuf),
    /// Explorer: folder to move `paths` into.
    MoveTo(Vec<PathBuf>),
    /// Explorer: Move to Trash / Cancel for `paths` (D9).
    Trash(Vec<PathBuf>),
    /// Search panel: Replace / Cancel for the checked files of the replace preview (AC-12).
    ReplaceInFiles,
}

pub fn answer(app: &mut App, dialog: Dialog, outcome: Outcome) {
    let Dialog { widget, action } = dialog;
    match (action, outcome) {
        (_, Outcome::Cancelled | Outcome::Pending) => {}
        (DialogAction::GotoLine, Outcome::Submitted(text)) => {
            if let Err(e) = super::nav::goto(app, &text) {
                reopen_with_error(app, widget, DialogAction::GotoLine, e);
            }
        }
        (DialogAction::Quit, Outcome::Chosen(b)) => super::app::answer_quit(app, b),
        (DialogAction::CloseTab(doc), Outcome::Chosen(b)) => {
            super::file::answer_close(app, &doc, b)
        }
        (DialogAction::SaveAs(doc), Outcome::Submitted(text)) => {
            super::file::answer_save_as(app, widget, doc, &text);
        }
        (DialogAction::ReplaceFile { doc, target, seen }, Outcome::Chosen(0)) => {
            super::file::answer_replace(app, &doc, target, seen.as_ref());
        }
        (DialogAction::OpenFile, Outcome::Submitted(text)) => {
            super::file::answer_open(app, widget, &text);
        }
        (DialogAction::ExternalChange { doc, seen, on_save }, Outcome::Chosen(b)) => {
            super::file::answer_external_change(app, &doc, seen.as_ref(), on_save, b)
        }
        (DialogAction::Deleted(doc), Outcome::Chosen(b)) => {
            super::file::answer_deleted(app, &doc, b)
        }
        (DialogAction::NewEntry { dir, folder }, Outcome::Submitted(text)) => {
            super::explorer::answer_new(app, widget, dir, folder, &text);
        }
        (DialogAction::Rename(path), Outcome::Submitted(text)) => {
            super::explorer::answer_rename(app, widget, path, &text);
        }
        (DialogAction::MoveTo(paths), Outcome::Submitted(text)) => {
            super::explorer::answer_move(app, widget, paths, &text);
        }
        (DialogAction::Trash(paths), Outcome::Chosen(0)) => {
            super::explorer::answer_trash(app, &paths);
        }
        (DialogAction::ReplaceInFiles, Outcome::Chosen(0)) => super::search::apply(app),
        _ => {}
    }
}

/// Puts a prompt back with `error` under its input (the typed text is kept).
pub fn reopen_with_error(
    app: &mut App,
    mut widget: DialogWidget,
    action: DialogAction,
    error: String,
) {
    if let DialogWidget::Prompt(p) = &mut widget {
        p.error = Some(error);
    }
    app.open_dialog(widget, action);
}
