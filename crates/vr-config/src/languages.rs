//! Language table: built-in `defaults/languages.toml` overridden field by field by the user file.
//!
//! Only the shape is validated here; whether a grammar is compiled in is checked by vr-syntax,
//! which owns the grammar table (AC-13e).

use std::path::Path;

use crate::{ConfigWarning, read_user_file, toml_error_message};

/// File name of the user override inside [`crate::config_dir`].
pub const LANGUAGES_FILE: &str = "languages.toml";

const DEFAULT_LANGUAGES: &str = include_str!("../defaults/languages.toml");

/// One `[id]` table of `languages.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LanguageDef {
    pub id: String,
    /// Compiled-in grammar id; the language id when the table does not name one.
    pub grammar: String,
    /// Without the dot; may hold inner dots (`blade.php`).
    pub extensions: Vec<String>,
    /// File names or `*` / `?` globs matched against the file name.
    pub files: Vec<String>,
    /// Other names in embedded code (Markdown fence info strings, `<script lang>`).
    pub aliases: Vec<String>,
    /// Language server command and arguments, reserved for M8.
    pub lsp: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Languages {
    /// Built-in languages first, then the ones the user file adds.
    pub languages: Vec<LanguageDef>,
}

impl Default for Languages {
    fn default() -> Self {
        let mut warnings = Vec::new();
        let table: toml::Table =
            toml::from_str(DEFAULT_LANGUAGES).expect("built-in defaults/languages.toml is valid");
        let mut langs = Languages {
            languages: Vec::new(),
        };
        langs.merge(table, Path::new("defaults/languages.toml"), &mut warnings);
        // Covered by `defaults_cover_every_d7_language`.
        debug_assert!(warnings.is_empty(), "{warnings:?}");
        langs
    }
}

impl Languages {
    /// Built-in defaults overridden by `user` (if it exists). Each bad table, field or value is
    /// reported and skipped; the default stays for it.
    pub fn load(user: Option<&Path>) -> (Languages, Vec<ConfigWarning>) {
        let mut warnings = Vec::new();
        let mut langs = Languages::default();
        if let Some(path) = user
            && let Some(text) = read_user_file(path, &mut warnings)
        {
            match toml::from_str::<toml::Table>(&text) {
                Err(e) => warnings.push(ConfigWarning::Malformed {
                    file: path.to_owned(),
                    message: toml_error_message(&text, &e),
                }),
                Ok(table) => langs.merge(table, path, &mut warnings),
            }
        }
        (langs, warnings)
    }

    pub fn get(&self, id: &str) -> Option<&LanguageDef> {
        self.languages.iter().find(|l| l.id == id)
    }

    fn merge(&mut self, table: toml::Table, file: &Path, warnings: &mut Vec<ConfigWarning>) {
        let invalid = |key: String, message: &str| ConfigWarning::InvalidValue {
            file: file.to_owned(),
            key,
            message: message.to_owned(),
        };
        for (id, value) in table {
            let toml::Value::Table(fields) = value else {
                warnings.push(invalid(id, "expected a [language] table"));
                continue;
            };
            if !valid_id(&id) {
                warnings.push(invalid(
                    id,
                    "language id must be letters, digits, '_' or '-'",
                ));
                continue;
            }
            let index = match self.languages.iter().position(|l| l.id == id) {
                Some(i) => i,
                None => {
                    self.languages.push(LanguageDef {
                        grammar: id.clone(),
                        id: id.clone(),
                        ..LanguageDef::default()
                    });
                    self.languages.len() - 1
                }
            };
            let def = &mut self.languages[index];
            for (field, value) in fields {
                let key = format!("{id}.{field}");
                let result = match field.as_str() {
                    "grammar" => match value.as_str() {
                        Some(g) if valid_id(g) => {
                            def.grammar = g.to_owned();
                            Ok(())
                        }
                        _ => Err("expected a grammar id string"),
                    },
                    "extensions" => list(&value, |s| {
                        !s.is_empty() && !s.contains('/') && !s.starts_with('.')
                    })
                    .map(|v| def.extensions = v)
                    .ok_or("expected a list of extensions without the dot"),
                    "files" => list(&value, |s| !s.is_empty() && !s.contains('/'))
                        .map(|v| def.files = v)
                        .ok_or("expected a list of file names or globs (no '/')"),
                    "aliases" => list(&value, |s| !s.trim().is_empty())
                        .map(|v| def.aliases = v)
                        .ok_or("expected a list of names"),
                    "lsp" => list(&value, |_| true)
                        .map(|v| def.lsp = v)
                        .ok_or("expected a command and its arguments as a list of strings"),
                    _ => {
                        warnings.push(ConfigWarning::UnknownSetting {
                            file: file.to_owned(),
                            key,
                        });
                        continue;
                    }
                };
                if let Err(message) = result {
                    warnings.push(invalid(key, message));
                }
            }
            self.take_claims(index);
        }
    }

    /// Removes the extensions, file names and aliases of `languages[index]` from every other
    /// language, so the table just merged wins a name another language had (a user's `[jsonc]`
    /// with `extensions = ["json"]` takes `.json` from the built-in `json`).
    fn take_claims(&mut self, index: usize) {
        let claimed = self.languages[index].clone();
        // Extensions and aliases are matched case-insensitively, file names exactly.
        let has = |list: &[String], x: &String| list.iter().any(|c| c.eq_ignore_ascii_case(x));
        for (i, other) in self.languages.iter_mut().enumerate() {
            if i != index {
                other.extensions.retain(|e| !has(&claimed.extensions, e));
                other.files.retain(|f| !claimed.files.contains(f));
                other.aliases.retain(|a| !has(&claimed.aliases, a));
            }
        }
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// A list of strings where every item passes `ok`; `None` rejects the whole value.
fn list(value: &toml::Value, ok: impl Fn(&str) -> bool) -> Option<Vec<String>> {
    value
        .as_array()?
        .iter()
        .map(|v| v.as_str().filter(|s| ok(s)).map(str::to_owned))
        .collect()
}
