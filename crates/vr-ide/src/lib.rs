//! vr-ide application: state, commands, rendering and the terminal event loop.
//!
//! The binary (`main.rs`) only sets up the terminal; everything else lives here so that
//! commands and rendering can be driven in tests without a terminal.

pub mod app;
pub mod cli;
pub mod clipboard;
pub mod commands;
pub mod event_loop;
pub mod recovery;
pub mod syntax;
pub mod theme;
pub mod ui;

#[cfg(test)]
mod app_tests;
#[cfg(test)]
mod dialog_tests;
#[cfg(test)]
mod explorer_tests;
#[cfg(test)]
mod find_tests;
#[cfg(test)]
mod language_snapshot_tests;
#[cfg(test)]
mod mode_tests;
#[cfg(test)]
mod search_tests;
#[cfg(test)]
mod syntax_tests;
