//! vr-ide application: state, commands, rendering and the terminal event loop.
//!
//! The binary (`main.rs`) only sets up the terminal; everything else lives here so that
//! commands and rendering can be driven in tests without a terminal.

pub mod ai;
pub mod app;
mod app_workspace;
pub mod cli;
pub mod clipboard;
pub mod commands;
mod config_reload;
pub mod desktop;
pub mod event_loop;
pub mod git;
pub mod monitor;
mod recent_files;
pub mod recovery;
pub mod session;
pub mod ssh;
pub mod syntax;
pub mod theme;
pub mod ui;
pub mod workspace;

#[cfg(test)]
mod ai_changes_tests;
#[cfg(test)]
mod ai_tests;
#[cfg(test)]
mod app_tests;
#[cfg(test)]
mod blame_column_tests;
#[cfg(test)]
mod config_reload_tests;
#[cfg(test)]
mod desktop_tests;
#[cfg(test)]
mod dialog_tests;
#[cfg(test)]
mod explorer_menu_tests;
#[cfg(test)]
mod explorer_tests;
#[cfg(test)]
mod find_tests;
#[cfg(test)]
mod git_tests;
#[cfg(test)]
mod language_snapshot_tests;
#[cfg(test)]
mod mode_tests;
#[cfg(test)]
mod search_tests;
#[cfg(test)]
mod session_tests;
#[cfg(test)]
mod slash2_tests;
#[cfg(test)]
mod slash_tests;
#[cfg(test)]
mod ssh_tests;
#[cfg(test)]
mod syntax_tests;
#[cfg(test)]
mod terminal_right_tests;
#[cfg(test)]
mod terminal_tab_drag_tests;
#[cfg(test)]
mod terminal_tests;
#[cfg(test)]
mod workspace_tests;
#[cfg(test)]
mod workspace_ui_tests;
