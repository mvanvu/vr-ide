//! The single list of named commands. Keys reach commands only through the keymap.
//!
//! Each group file exposes `COMMANDS: &[CommandSpec]`; later tasks add their group to
//! [`GROUPS`] and remove their names from [`PENDING`].

use std::path::PathBuf;
use std::sync::OnceLock;

use crate::app::App;

#[derive(Debug, Clone, Copy)]
pub struct CommandSpec {
    pub name: &'static str,
    pub title: &'static str,
    pub run: fn(&mut App, CmdArgs) -> CmdResult,
}

/// Arguments a caller (palette, dialog, explorer) can pass; keys always pass `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CmdArgs {
    #[default]
    None,
    Path(PathBuf),
}

/// An `Err` becomes an error notice (`<title>: <message>`).
pub type CmdResult = anyhow::Result<()>;

const GROUPS: &[&[CommandSpec]] = &[
    super::app::COMMANDS,
    super::file::COMMANDS,
    super::edit::COMMANDS,
    super::tab::COMMANDS,
    super::nav::COMMANDS,
    super::explorer::COMMANDS,
    super::find::COMMANDS,
    super::search::COMMANDS,
    super::terminal::COMMANDS,
    PENDING,
];

pub fn all_commands() -> &'static [CommandSpec] {
    static ALL: OnceLock<Vec<CommandSpec>> = OnceLock::new();
    ALL.get_or_init(|| GROUPS.iter().flat_map(|g| g.iter().copied()).collect())
}

pub fn find(name: &str) -> Option<&'static CommandSpec> {
    all_commands().iter().find(|c| c.name == name)
}

/// Every registered name, for `Keymap::validate`.
pub fn command_names() -> Vec<&'static str> {
    all_commands().iter().map(|c| c.name).collect()
}

/// True for a placeholder of a later task (not offered in the palette).
pub fn is_pending(name: &str) -> bool {
    PENDING.iter().any(|c| c.name == name)
}

/// Placeholders of commands a later task builds: registered early so the default keymap
/// validates cleanly, hidden from the palette. Every plan command is registered, so it is empty.
const PENDING: &[CommandSpec] = &[];

#[cfg(test)]
mod tests {
    use super::*;

    /// The plan's command list (names are used by keymaps and must stay stable).
    const PLAN: &str = "app.quit, app.palette, file.save, file.save_as, file.close, file.open, \
        tab.next, tab.prev, edit.undo, edit.redo, edit.copy, edit.cut, edit.paste, edit.select_all, \
        edit.delete_line, edit.duplicate_line, edit.move_line_up, edit.move_line_down, edit.indent, \
        edit.outdent, edit.toggle_comment, nav.goto_line, file.reload, explorer.toggle, \
        explorer.focus, explorer.reveal, explorer.new_file, explorer.new_folder, explorer.rename, \
        explorer.copy, explorer.paste, explorer.move, explorer.delete, explorer.toggle_ignored, \
        explorer.toggle_hidden, file.goto, find.open, find.next, find.prev, find.replace_open, \
        find.replace_one, find.replace_all, search.project, search.next_result, \
        search.prev_result, search.replace_project, focus.cycle";

    #[test]
    fn registry_has_every_plan_command_once() {
        let names = command_names();
        for want in PLAN.split(',').map(str::trim) {
            assert_eq!(names.iter().filter(|n| **n == want).count(), 1, "{want}");
        }
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len(), "duplicate command names");
        assert!(all_commands().iter().all(|c| !c.title.is_empty()));
    }

    #[test]
    fn default_keymap_validates_cleanly() {
        let (km, warnings) = vr_config::Keymap::load(None);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(km.validate(&command_names()), Vec::new());
    }
}
