//! `app.*` commands.

use super::registry::{CmdArgs, CmdResult, CommandSpec};
use crate::app::App;

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

/// With unsaved changes the first press only warns; pressing quit again (after letting go of
/// the key) quits, and the unsaved buffers are kept in the recovery directory.
/// (The save / discard dialog replaces this in T6.)
fn quit(app: &mut App, _: CmdArgs) -> CmdResult {
    let unsaved = app.docs.iter().filter(|d| d.is_modified()).count();
    if unsaved > 0 {
        let warning = format!(
            "{unsaved} unsaved file(s): {} again to quit without saving",
            app.key_hint("app.quit")
        );
        if !app.confirm_twice("app.quit", warning) {
            return Ok(());
        }
        let all: Vec<usize> = (0..app.docs.len()).collect();
        app.keep_discarded(&all);
    }
    app.quit = true;
    Ok(())
}

fn palette(app: &mut App, _: CmdArgs) -> CmdResult {
    app.info("Command palette is not available yet");
    Ok(())
}
