//! Workspace commands of the palette (W-2, spec W): each opens the matching `/ws` slash
//! command in the F1 window, which holds the path completion, the root and recent-workspace
//! lists and the checks. None has a default key (keys are the user's choice).

use super::registry::{CmdArgs, CmdResult, CommandSpec};
use crate::app::{App, Focus};

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "workspace.add_folder",
        title: "Workspace: Add Folder…",
        run: add_folder,
    },
    CommandSpec {
        name: "workspace.remove_folder",
        title: "Workspace: Remove Folder",
        run: remove_folder,
    },
    CommandSpec {
        name: "workspace.save_as",
        title: "Workspace: Save As…",
        run: save_as,
    },
    CommandSpec {
        name: "workspace.open",
        title: "Workspace: Open…",
        run: open,
    },
    CommandSpec {
        name: "workspace.open_recent",
        title: "Workspace: Open Recent",
        run: open_recent,
    },
];

/// Opens the F1 window with `text` typed (a slash command waiting for its argument).
pub fn open_slash(app: &mut App, text: &str) {
    super::palette::open(app);
    super::slash::set_input(app, text);
}

fn add_folder(app: &mut App, _: CmdArgs) -> CmdResult {
    open_slash(app, "/ws add ");
    Ok(())
}

/// The root under the explorer cursor (its row's menu, or the command with the explorer
/// focused on a root row); otherwise the list of roots to pick from. Nothing on disk is
/// touched and no tab closes (W-1).
fn remove_folder(app: &mut App, _: CmdArgs) -> CmdResult {
    let on_root = (app.focus == Focus::Explorer)
        .then(|| app.explorer.current())
        .flatten()
        .filter(|r| r.depth == 0)
        .map(|r| r.path.clone());
    let Some(path) = on_root else {
        open_slash(app, "/ws remove ");
        return Ok(());
    };
    let Some(id) = app
        .workspace
        .roots()
        .iter()
        .find(|r| r.path == path)
        .map(|r| r.id)
    else {
        return Ok(());
    };
    match app.remove_root(id) {
        Ok(root) => app.info(format!(
            "workspace: removed {} (nothing on disk was changed)",
            root.name
        )),
        Err(e) => app.warn(e.to_string()),
    }
    Ok(())
}

fn save_as(app: &mut App, _: CmdArgs) -> CmdResult {
    open_slash(app, "/ws save ");
    Ok(())
}

fn open(app: &mut App, _: CmdArgs) -> CmdResult {
    open_slash(app, "/ws open ");
    Ok(())
}

fn open_recent(app: &mut App, _: CmdArgs) -> CmdResult {
    open_slash(app, "/ws recent ");
    Ok(())
}
