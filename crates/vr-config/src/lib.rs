//! Keymap and settings loading.
//!
//! Built-in defaults are compiled in (`defaults/*.toml`); user files under [`config_dir`] override them.
//! Nothing here panics on bad user input: problems come back as [`ConfigWarning`]s for a startup notice.

mod keymap;
mod keys;
mod languages;
mod paths;
mod recent;
mod recent_files;
mod settings;
mod workspace_file;

use std::path::PathBuf;

pub use keymap::{Context, DEFAULT_KEYMAP, Keymap, RESERVED_COMMANDS};
pub use keys::{KeyChord, KeyCode, KeyParseError, Mods};
pub use languages::{LANGUAGES_FILE, LanguageDef, Languages};
pub use paths::{config_dir, state_dir};
pub use recent::{
    RECENT_FILE, RECENT_WORKSPACES_MAX, RecentWorkspace, RecentWorkspaces, write_atomic,
};
pub use recent_files::{RECENT_FILES_FILE, RECENT_FILES_MAX, RecentFiles};
pub use settings::{AiPreset, DEFAULT_SETTINGS, Settings, TerminalPosition};
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

/// A user file made from built-in defaults (`/config`, `/keymap`): every setting line is
/// commented out, so the file changes nothing until a line is uncommented, and later changes
/// of the defaults still reach the user. `[table]` headers stay, so an uncommented key lands
/// in its table; `[[array]]` headers are commented (an empty entry would be invalid).
pub fn user_file_template(defaults: &str) -> String {
    let mut out = String::from(
        "# Made by vr-ide from the built-in defaults. Every line is commented out: remove the\n\
         # \"# \" in front of a line to change that value; the rest keep the built-in values.\n\n",
    );
    for line in defaults.lines() {
        let keep = line.is_empty()
            || line.starts_with('#')
            || (line.starts_with('[') && !line.starts_with("[["));
        if !keep {
            out.push_str("# ");
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_file_templates_change_nothing_until_a_line_is_uncommented() {
        let d = tempfile::tempdir().unwrap();
        let settings = d.path().join("settings.toml");
        std::fs::write(&settings, user_file_template(DEFAULT_SETTINGS)).unwrap();
        let (s, w) = Settings::load(Some(&settings));
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(s, Settings::default());
        let text = std::fs::read_to_string(&settings).unwrap();
        assert!(text.contains("\n# tab_width = 4\n"), "{text}");
        assert!(text.contains("\n# [[ai_presets]]\n"), "{text}");
        std::fs::write(&settings, text.replace("# tab_width = 4", "tab_width = 2")).unwrap();
        assert_eq!(Settings::load(Some(&settings)).0.tab_width, 2);

        let keymap = d.path().join("keymap.toml");
        let text = user_file_template(DEFAULT_KEYMAP);
        assert!(text.contains("\n[editor]\n"), "table headers stay");
        std::fs::write(&keymap, &text).unwrap();
        let (k, w) = Keymap::load(Some(&keymap));
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(k, Keymap::load(None).0);
    }
}
