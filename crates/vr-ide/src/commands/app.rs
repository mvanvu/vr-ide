//! `app.*` commands.

use super::dialog::DialogAction;
use super::file::{CANCEL, SaveOutcome, save_doc};
use super::registry::{CmdArgs, CmdResult, CommandSpec};
use super::search::QuitBlock;
use crate::app::App;
use crate::ui::dialog::{ConfirmDialog, DialogWidget};

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "app.quit",
        title: "Quit",
        run: quit,
    },
    CommandSpec {
        name: "app.palette",
        title: "Command Palette",
        run: palette,
    },
];

/// With unsaved changes, asks: Save all / Discard / Cancel (the answer is in [`answer_quit`]).
/// Refused while a project replace writes files: exiting would stop it with some files
/// replaced and nothing reported. A cancelled replace that has not stopped after
/// `CANCEL_WAIT` (a worker blocked in I/O) asks first, naming the file.
fn quit(app: &mut App, _: CmdArgs) -> CmdResult {
    if !quit_unblocked(app) || !super::terminal::quit_allowed(app) {
        return Ok(());
    }
    let unsaved: Vec<String> = app
        .docs
        .iter()
        .filter(|d| d.is_modified())
        .map(|d| {
            d.path
                .as_deref()
                .map_or_else(|| "untitled".to_string(), |p| app.display_path(p))
        })
        .collect();
    if unsaved.is_empty() {
        app.quit = true;
        return Ok(());
    }
    let message = format!(
        "{} unsaved file(s): {}. Discard keeps a copy in the recovery folder.",
        unsaved.len(),
        unsaved.join(", ")
    );
    app.open_dialog(
        DialogWidget::Confirm(ConfirmDialog::new(
            "Quit",
            message,
            &[SAVE_ALL, DISCARD, CANCEL],
        )),
        DialogAction::Quit,
    );
    Ok(())
}

pub const SAVE_ALL: &str = "Save all";
pub const DISCARD: &str = "Discard";

/// Save all: quits once every modified file is saved (stops at a failure or a prompt).
/// Discard: quits once the buffers are in the recovery folder (D57), and prints where after
/// the terminal is restored; when that write fails nothing is discarded.
pub fn answer_quit(app: &mut App, button: usize) {
    if !quit_unblocked(app) {
        return;
    }
    match button {
        0 => {
            for i in 0..app.docs.len() {
                if app.docs[i].is_modified() && save_doc(app, i) != SaveOutcome::Saved {
                    return;
                }
            }
            app.quit = true;
        }
        1 => {
            let all: Vec<usize> = (0..app.docs.len()).collect();
            match app.keep_discarded(&all) {
                Ok(files) => {
                    if !files.is_empty() {
                        let list: Vec<String> =
                            files.iter().map(|f| f.display().to_string()).collect();
                        app.exit_messages.push(format!(
                            "unsaved changes were kept in the recovery folder: {}",
                            list.join(", ")
                        ));
                    }
                    app.quit = true;
                }
                Err(e) => app.error(e),
            }
        }
        _ => {}
    }
}

/// True when no project replace is in the way of quitting; otherwise says why (or asks).
fn quit_unblocked(app: &mut App) -> bool {
    quit_unblocked_for(app, DialogAction::QuitStuckReplace)
}

/// [`quit_unblocked`] whose question (a replace that does not stop) is answered by `action`.
fn quit_unblocked_for(app: &mut App, action: DialogAction) -> bool {
    match super::search::quit_block(app) {
        None => true,
        Some(QuitBlock::Replacing) => {
            app.warn("Quit: a replace is running; Esc in the search panel cancels it");
            false
        }
        Some(QuitBlock::Stuck(file)) => {
            super::search::ask_quit_stuck_for(app, file.as_deref(), action);
            false
        }
    }
}

/// `/q!` (D268): quits without the unsaved-files question, the buffers kept in the recovery
/// folder first ([`answer_quit`]'s Discard). A replace that does not stop and running
/// terminals still ask; their Quit anyway comes back here, so the discard is never lost to a
/// Save all question.
pub fn quit_discarding(app: &mut App) {
    if quit_unblocked_for(app, DialogAction::QuitStuckReplaceDiscard)
        && super::terminal::quit_allowed_for(app, DialogAction::QuitTerminalsDiscard)
    {
        answer_quit(app, 1);
    }
}

/// "Quit anyway" on a replace that does not stop: quit goes on (unsaved files still ask).
pub fn answer_quit_stuck(app: &mut App) {
    super::search::allow_quit(app);
    let _ = quit(app, CmdArgs::None);
}

fn palette(app: &mut App, _: CmdArgs) -> CmdResult {
    super::palette::open(app);
    Ok(())
}
