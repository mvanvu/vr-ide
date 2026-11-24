//! `app.*` commands.

use super::dialog::DialogAction;
use super::file::{CANCEL, SaveOutcome, save_doc};
use super::registry::{CmdArgs, CmdResult, CommandSpec};
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
/// replaced and nothing reported.
fn quit(app: &mut App, _: CmdArgs) -> CmdResult {
    if super::search::replacing(app) {
        app.warn("Quit: a replace is running; Esc in the search panel cancels it");
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
    if super::search::replacing(app) {
        app.warn("Quit: a replace is running; Esc in the search panel cancels it");
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

fn palette(app: &mut App, _: CmdArgs) -> CmdResult {
    super::palette::open(app);
    Ok(())
}
