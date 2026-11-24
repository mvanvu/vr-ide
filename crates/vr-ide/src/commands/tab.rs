//! `tab.*` commands.

use super::registry::{CmdArgs, CmdResult, CommandSpec};
use crate::app::App;

pub const COMMANDS: &[CommandSpec] = &[
    CommandSpec {
        name: "tab.next",
        title: "Next Tab",
        run: next,
    },
    CommandSpec {
        name: "tab.prev",
        title: "Previous Tab",
        run: prev,
    },
];

fn next(app: &mut App, _: CmdArgs) -> CmdResult {
    step(app, 1);
    Ok(())
}

fn prev(app: &mut App, _: CmdArgs) -> CmdResult {
    step(app, -1);
    Ok(())
}

/// Moves to the neighbouring tab, wrapping around at both ends.
fn step(app: &mut App, delta: isize) {
    let n = app.docs.len();
    if n > 1 {
        let i = (app.active as isize + delta).rem_euclid(n as isize) as usize;
        app.set_active(i);
    }
}
