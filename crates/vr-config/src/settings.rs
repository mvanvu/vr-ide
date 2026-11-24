//! Editor settings: built-in `defaults/settings.toml` overridden key by key by the user file.

use std::path::Path;

use serde::Deserialize;

use crate::{ConfigWarning, read_user_file, toml_error_message};

const DEFAULT_SETTINGS: &str = include_str!("../defaults/settings.toml");

/// An AI agent CLI started in a terminal tab (M6).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AiPreset {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub tab_width: usize,
    pub insert_spaces: bool,
    pub theme: String,
    pub show_hidden: bool,
    pub show_ignored: bool,
    pub max_highlight_bytes: u64,
    pub scrollback: usize,
    /// Height of the terminal panel in percent of the screen, 10..=90 (M5).
    pub terminal_height_percent: u16,
    /// Without the kitty protocol, NUL (what ctrl+` sends) in a focused terminal goes to the
    /// program instead of hiding the panel (D214).
    pub terminal_pass_nul: bool,
    pub ai_presets: Vec<AiPreset>,
}

impl Default for Settings {
    fn default() -> Self {
        // The embedded file is fixed at compile time and covered by `settings_defaults_and_override`.
        toml::from_str(DEFAULT_SETTINGS).expect("built-in defaults/settings.toml is valid")
    }
}

impl Settings {
    /// Built-in defaults overridden by `user` (if it exists). Unknown keys, values of the wrong type
    /// and invalid TOML are reported and skipped; the default value stays for each of them.
    pub fn load(user: Option<&Path>) -> (Settings, Vec<ConfigWarning>) {
        let mut warnings = Vec::new();
        let mut merged: toml::Table =
            toml::from_str(DEFAULT_SETTINGS).expect("built-in defaults/settings.toml is valid");
        if let Some(path) = user
            && let Some(text) = read_user_file(path, &mut warnings)
        {
            match toml::from_str::<toml::Table>(&text) {
                Err(e) => warnings.push(ConfigWarning::Malformed {
                    file: path.to_owned(),
                    message: toml_error_message(&text, &e),
                }),
                Ok(user_table) => {
                    for (key, value) in user_table {
                        if !merged.contains_key(&key) {
                            warnings.push(ConfigWarning::UnknownSetting {
                                file: path.to_owned(),
                                key,
                            });
                            continue;
                        }
                        // Try each key alone so one bad value does not discard the others.
                        let mut candidate = merged.clone();
                        candidate.insert(key.clone(), value);
                        match parse(candidate.clone()) {
                            Ok(_) => merged = candidate,
                            Err(message) => warnings.push(ConfigWarning::InvalidValue {
                                file: path.to_owned(),
                                key,
                                message,
                            }),
                        }
                    }
                }
            }
        }
        let settings = parse(merged).unwrap_or_default();
        (settings, warnings)
    }
}

fn parse(table: toml::Table) -> Result<Settings, String> {
    let s: Settings = toml::Value::Table(table)
        .try_into()
        .map_err(|e: toml::de::Error| e.message().to_owned())?;
    if s.tab_width == 0 || s.tab_width > 16 {
        return Err("tab_width must be between 1 and 16".into());
    }
    if !(10..=90).contains(&s.terminal_height_percent) {
        return Err("terminal_height_percent must be between 10 and 90".into());
    }
    if let Some(p) = s.ai_presets.iter().find(|p| p.command.trim().is_empty()) {
        return Err(format!("ai preset {:?} has an empty command", p.name));
    }
    Ok(s)
}
