//! Keymap and settings loading.
//!
//! Built-in defaults are compiled in (`defaults/*.toml`); user files under [`config_dir`] override them.
//! Nothing here panics on bad user input: problems come back as [`ConfigWarning`]s for a startup notice.

mod keymap;
mod keys;
mod languages;
mod paths;
mod recent;
mod settings;
mod workspace_file;

use std::path::PathBuf;

pub use keymap::{Context, Keymap, RESERVED_COMMANDS};
pub use keys::{KeyChord, KeyCode, KeyParseError, Mods};
pub use languages::{LANGUAGES_FILE, LanguageDef, Languages};
pub use paths::{config_dir, state_dir};
pub use recent::{
    RECENT_FILE, RECENT_WORKSPACES_MAX, RecentWorkspace, RecentWorkspaces, write_atomic,
};
pub use settings::{AiPreset, Settings};
pub use workspace_file::{
    WORKSPACE_EXT, format_workspace, is_workspace_file, parse_workspace, read_workspace,
};

/// A non-fatal problem found while loading configuration; the offending entry is skipped.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigWarning {
    #[error("{}: cannot read: {message}", file.display())]
    Io { file: PathBuf, message: String },
    #[error("{}: invalid TOML, built-in defaults kept: {message}", file.display())]
    Malformed { file: PathBuf, message: String },
    #[error("{}: [{name}] is not a keymap context (editor, explorer, palette, terminal)", file.display())]
    UnknownContext { file: PathBuf, name: String },
    #[error("{}: key {key:?} is not a valid key chord: {reason}", file.display())]
    InvalidKey {
        file: PathBuf,
        key: String,
        reason: String,
    },
    #[error("{}: {key}: {message}", file.display())]
    InvalidValue {
        file: PathBuf,
        key: String,
        message: String,
    },
    #[error("{}: unknown setting {key:?}", file.display())]
    UnknownSetting { file: PathBuf, key: String },
    #[error("{}: {first:?} and {second:?} are the same key ({context}); {second:?} ignored", file.display())]
    DuplicateKey {
        file: PathBuf,
        context: Context,
        first: String,
        second: String,
    },
    #[error("keymap: {key} is bound to unknown command {command:?} ({context})")]
    UnknownCommand {
        context: Context,
        key: String,
        command: String,
    },
}

/// Reads an optional user file: a missing file is normal (no warning), other read errors are warned.
pub(crate) fn read_user_file(
    path: &std::path::Path,
    warnings: &mut Vec<ConfigWarning>,
) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            warnings.push(ConfigWarning::Io {
                file: path.to_owned(),
                message: e.to_string(),
            });
            None
        }
    }
}

/// One-line TOML error with its line number, for the startup notice.
pub(crate) fn toml_error_message(text: &str, e: &toml::de::Error) -> String {
    match e.span() {
        Some(span) => {
            let line = text[..span.start.min(text.len())].matches('\n').count() + 1;
            format!("line {line}: {}", e.message())
        }
        None => e.message().to_owned(),
    }
}
