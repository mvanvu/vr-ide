//! Editor settings: built-in `defaults/settings.toml` overridden key by key by the user file.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{ConfigWarning, read_user_file, toml_error_message};

/// The built-in `settings.toml` text.
pub const DEFAULT_SETTINGS: &str = include_str!("../defaults/settings.toml");

/// An AI agent CLI started in a terminal tab (M6).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AiPreset {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
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
    /// program instead of giving the keys back to the editor (D214, D230).
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

    /// The settings `/set` can change: every key of the built-in file with a single value
    /// (lists such as `ai_presets` are edited in the file), in file order.
    pub fn names() -> Vec<String> {
        // Read from the text, not a parsed table: the table does not keep the file's order.
        DEFAULT_SETTINGS
            .lines()
            .take_while(|l| !l.starts_with('['))
            .filter(|l| !l.starts_with('#'))
            .filter_map(|l| l.split_once(" = ").map(|(k, _)| k.trim().to_string()))
            .collect()
    }

    /// The value of setting `key` as TOML text (`4`, `true`, `"dark"`); `None` for an unknown
    /// key.
    pub fn value_text(&self, key: &str) -> Option<String> {
        if !Settings::names().iter().any(|n| n == key) {
            return None;
        }
        Some(self.table().get(key)?.to_string())
    }

    /// A copy with setting `key` set to `raw`, checked as when the file is loaded (type and
    /// range); `raw` is TOML (`4`, `true`) or, for a text setting, plain text too.
    pub fn with_value(&self, key: &str, raw: &str) -> Result<Settings, String> {
        let raw = raw.trim();
        if !Settings::names().iter().any(|n| n == key) {
            return Err(if key == "ai_presets" {
                "ai_presets is a list: change it in settings.toml (/config)".to_string()
            } else {
                format!("unknown setting {key}")
            });
        }
        if raw.is_empty() {
            return Err(format!("{key} needs a value"));
        }
        let mut table = self.table();
        let typed = toml::from_str::<toml::Table>(&format!("v = {raw}"))
            .ok()
            .and_then(|mut t| t.remove("v"));
        let value = match (table.get(key), typed) {
            (Some(toml::Value::String(_)), Some(v @ toml::Value::String(_))) => v,
            // Text settings take the word as typed: `/set theme dark`.
            (Some(toml::Value::String(_)), _) => toml::Value::String(raw.to_string()),
            (_, Some(v)) => v,
            (_, None) => return Err(format!("{key}: {raw} is not a valid value")),
        };
        table.insert(key.to_string(), value);
        parse(table).map_err(|e| {
            if e.contains(key) {
                e
            } else {
                format!("{key}: {e}")
            }
        })
    }

    fn table(&self) -> toml::Table {
        match toml::Value::try_from(self) {
            Ok(toml::Value::Table(t)) => t,
            _ => toml::Table::new(),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_the_single_value_keys_in_file_order() {
        let names = Settings::names();
        assert_eq!(names[0], "tab_width");
        assert!(names.contains(&"theme".to_string()));
        assert!(names.contains(&"terminal_pass_nul".to_string()));
        assert!(!names.contains(&"ai_presets".to_string()));
    }

    #[test]
    fn value_text_shows_toml() {
        let s = Settings::default();
        assert_eq!(s.value_text("tab_width").as_deref(), Some("4"));
        assert_eq!(s.value_text("insert_spaces").as_deref(), Some("true"));
        assert_eq!(s.value_text("theme").as_deref(), Some("\"synthwave\""));
        assert_eq!(s.value_text("nope"), None);
    }

    #[test]
    fn with_value_checks_name_type_and_range() {
        let s = Settings::default();
        assert_eq!(s.with_value("tab_width", "2").unwrap().tab_width, 2);
        assert!(
            !s.with_value("insert_spaces", "false")
                .unwrap()
                .insert_spaces
        );
        assert_eq!(s.with_value("theme", "dark").unwrap().theme, "dark");
        assert_eq!(s.with_value("theme", "\"light\"").unwrap().theme, "light");
        assert_eq!(
            s.with_value("nope", "1").unwrap_err(),
            "unknown setting nope"
        );
        assert!(s.with_value("tab_width", "abc").is_err());
        assert!(s.with_value("tab_width", "true").is_err());
        assert_eq!(
            s.with_value("tab_width", "0").unwrap_err(),
            "tab_width must be between 1 and 16"
        );
        assert!(s.with_value("insert_spaces", "yes").is_err());
        assert!(s.with_value("ai_presets", "[]").is_err());
        assert!(s.with_value("tab_width", "").is_err());
        // The original is untouched.
        assert_eq!(s, Settings::default());
    }
}
