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
    /// Explorer: File / Folder for a new entry in `dir` (ctrl+n in the explorer).
    NewKind { dir: PathBuf },
    /// Explorer: new name for `path`.
    Rename(PathBuf),
    /// Explorer: folder to move `paths` into.
    MoveTo(Vec<PathBuf>),
    /// Explorer: Move to Trash / Cancel for `paths` (D9).
    Trash(Vec<PathBuf>),
    /// Search panel: Replace / Cancel for the checked files of the replace preview (AC-12).
    ReplaceInFiles,
    /// Quit while a cancelled project replace has not stopped (its worker is blocked in
    /// I/O): Quit anyway / Cancel.
    QuitStuckReplace,
    /// Quit while terminals run (M5) or a git push, pull or commit runs (D309): Quit anyway /
    /// Cancel, Cancel first.
    QuitTerminals,
    /// `/q!` (T20, D268): [`DialogAction::QuitStuckReplace`] whose Quit anyway goes on
    /// discarding the unsaved files (recovery copy kept), never asking about them.
    QuitStuckReplaceDiscard,
    /// `/q!`: [`DialogAction::QuitTerminals`] whose Quit anyway goes on discarding.
    QuitTerminalsDiscard,
    /// A terminal tab's 'x' while a program holds it (D296): Close / Cancel; the tab's pid.
    CloseTerminal(Option<u32>),
    /// Find box, File scope (D279): Replace: One / All / Cancel.
    ReplaceInFile,
    /// Unsaved files before the roots are replaced (W-1): Save all / Discard / Cancel.
    OpenWorkspace(Box<crate::workspace::Workspace>),
    /// Git popup (M7-3): Cancel / Discard of `plan` (Cancel first, SEC-16).
    GitDiscard {
        session: u64,
        plan: Box<vr_git::DiscardPlan>,
    },
    /// Git popup: Push / Cancel to the shown target.
    GitPush {
        session: u64,
        target: Box<vr_git::PushTarget>,
    },
    /// Git popup: Keep running / Cancel it for a running push or pull.
    GitCancelJob { session: u64 },
    /// Git popup: Keep running / Cancel it for a running commit (ctrl+s again).
    GitCancelCommit { session: u64 },
    /// Git popup: Cancel / Amend anyway when the last commit is already on the upstream.
    GitAmendPushed {
        session: u64,
        commit: Box<crate::git_popup::PendingCommit>,
    },
    /// Git popup: a leftover of an interrupted discard: Keep / Restore as… or Delete.
    GitLeftover {
        session: u64,
        leftover: vr_git::LeftoverDiscard,
    },
    /// Git popup: the name to restore a leftover `.old` as.
    GitLeftoverRestore {
        session: u64,
        path: std::path::PathBuf,
    },
}

pub fn answer(app: &mut App, dialog: Dialog, outcome: Outcome) {
    let Dialog { widget, action } = dialog;
    match (action, outcome) {
        (_, Outcome::Pending) => {}
        // Every answer, Esc included, gives the replace row its edit mode back.
        (DialogAction::ReplaceInFile, o) => {
            let button = match o {
                Outcome::Chosen(b) => Some(b),
                _ => None,
            };
            super::find::answer_replace(app, button);
        }
        (_, Outcome::Cancelled) => {}
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
        (DialogAction::NewKind { dir }, Outcome::Chosen(b)) => {
            super::explorer::prompt_new_in(app, dir, b == 1);
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
        (DialogAction::QuitStuckReplace, Outcome::Chosen(0)) => super::app::answer_quit_stuck(app),
        (DialogAction::QuitTerminals, Outcome::Chosen(b)) => super::terminal::answer_quit(app, b),
        (DialogAction::CloseTerminal(pid), Outcome::Chosen(0)) => {
            super::terminal::answer_close(app, pid);
        }
        (DialogAction::QuitStuckReplaceDiscard, Outcome::Chosen(0)) => {
            super::search::allow_quit(app);
            super::app::quit_discarding(app);
        }
        (DialogAction::QuitTerminalsDiscard, Outcome::Chosen(0)) => {
            super::terminal::confirm_quit(app);
            super::app::quit_discarding(app);
        }
        (DialogAction::OpenWorkspace(ws), Outcome::Chosen(b)) => app.answer_open_workspace(*ws, b),
        (DialogAction::GitDiscard { session, plan }, Outcome::Chosen(b)) => {
            crate::git_popup::answer_discard(app, session, *plan, b);
        }
        (DialogAction::GitPush { session, target }, Outcome::Chosen(b)) => {
            crate::git_popup::answer_push(app, session, *target, b);
        }
        (DialogAction::GitCancelJob { session }, Outcome::Chosen(b)) => {
            crate::git_popup::answer_cancel_job(app, session, b);
        }
        (DialogAction::GitCancelCommit { session }, Outcome::Chosen(b)) => {
            crate::git_popup::answer_cancel_commit(app, session, b);
        }
        (DialogAction::GitAmendPushed { session, commit }, Outcome::Chosen(b)) => {
            crate::git_popup::answer_amend_pushed(app, session, *commit, b);
        }
        (DialogAction::GitLeftover { session, leftover }, Outcome::Chosen(b)) => {
            crate::git_popup::answer_leftover(app, session, leftover, b);
        }
        (DialogAction::GitLeftoverRestore { session, path }, Outcome::Submitted(text)) => {
            crate::git_popup::answer_leftover_restore(app, session, path, &text);
        }
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
