//! Named commands, one file per group; the registry joins them.

pub mod app;
pub mod dialog;
pub mod edit;
pub mod explorer;
pub mod file;
pub mod find;
pub mod nav;
pub mod palette;
pub mod registry;
pub mod search;
pub mod tab;

pub use registry::{CmdArgs, CmdResult, CommandSpec, all_commands, command_names, find};
