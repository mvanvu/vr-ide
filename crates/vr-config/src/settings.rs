//! Editor settings: built-in `defaults/settings.toml` overridden key by key by the user file.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{ConfigWarning, read_user_file, toml_error_message};

/// The built-in `settings.toml` text.
pub const DEFAULT_SETTINGS: &str = include_str!("../defaults/settings.toml");

/// An AI agent CLI of the AI panel (D7, D14). Started as argv, never through a shell (SEC-8).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AiTool {
    /// What the panel shows and a saved session names (`[A-Za-z0-9][A-Za-z0-9._-]{0,31}`).
    pub name: String,
    /// A bare name looked up in the absolute `PATH` entries, or an absolute path.
    pub command: String,
    /// Passed on every start.
    #[serde(default)]
    pub args: Vec<String>,
    /// Added to continue the last conversation (picker, restore, restart); empty: none.
    #[serde(default)]
    pub continue_args: Vec<String>,
    /// Added to choose an earlier conversation in the tool's own list; empty: none.
    #[serde(default)]
    pub resume_args: Vec<String>,
}

impl AiTool {
    /// The tool can pick up an earlier conversation (the picker's second step, D14).
    pub fn resumes(&self) -> bool {
        !self.continue_args.is_empty() || !self.resume_args.is_empty()
    }
}

/// Where the terminal panel sits (D227).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TerminalPosition {
    /// Under the editor, across the editor column.
    #[default]
    Bottom,
    /// Right of the editor, the editor's full height under the tab bar.
    Right,
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
    /// Where the terminal panel sits (T19).
    pub terminal_position: TerminalPosition,
    /// Width of the panel on the right in percent of the editor column, 10..=90 (T19).
    pub terminal_width_percent: u16,
    /// Without the kitty protocol, NUL (what ctrl+` sends) in a focused terminal goes to the
    /// program instead of giving the keys back to the editor (D214, D230).
    pub terminal_pass_nul: bool,
    /// A logged-in SSH session takes the whole window (FR-25, D320).
    pub ssh_fullscreen: bool,
    /// Inline blame of the cursor line (M7-4a).
    pub git_inline_blame: bool,
    /// Width of the AI panel in percent of the screen right of the explorer, 10..=90 (D7).
    pub ai_width_percent: u16,
    pub ai_tools: Vec<AiTool>,
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
                        // The list was called ai_presets before the AI panel (D7).
                        let key = if key == "ai_presets" {
                            "ai_tools".to_string()
                        } else {
                            key
                        };
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
    /// (lists such as `ai_tools` are edited in the file), in file order.
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
            return Err(if key == "ai_tools" {
                "ai_tools is a list: change it in settings.toml (/config)".to_string()
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
    if !(10..=90).contains(&s.terminal_width_percent) {
        return Err("terminal_width_percent must be between 10 and 90".into());
    }
    if !(10..=90).contains(&s.ai_width_percent) {
        return Err("ai_width_percent must be between 10 and 90".into());
    }
    for (i, t) in s.ai_tools.iter().enumerate() {
        if !valid_tool_name(&t.name) {
            return Err(format!(
                "ai tool {:?}: a name is 1 to 32 of A-Z a-z 0-9 . _ - (letter or digit first)",
                t.name
            ));
        }
        if t.command.trim().is_empty() {
            return Err(format!("ai tool {:?} has an empty command", t.name));
        }
        if s.ai_tools[..i].iter().any(|o| o.name == t.name) {
            return Err(format!("ai tool {:?} is listed twice", t.name));
        }
    }
    Ok(s)
}

/// A tool name as `ai_tools` and a saved session hold it: shown in tab labels and matched on
/// restore, so no spaces, control or direction characters (SEC-8).
pub fn valid_tool_name(name: &str) -> bool {
    let b = name.as_bytes();
    (1..=32).contains(&b.len())
        && b[0].is_ascii_alphanumeric()
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
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
        assert!(!names.contains(&"ai_tools".to_string()));
        assert!(names.contains(&"ai_width_percent".to_string()));
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
        assert!(s.with_value("ai_tools", "[]").is_err());
        assert!(s.with_value("tab_width", "").is_err());
        // The original is untouched.
        assert_eq!(s, Settings::default());
    }
}
