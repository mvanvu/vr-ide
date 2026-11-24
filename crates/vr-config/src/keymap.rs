//! Key chord -> command name, per focus context. Built-in `defaults/keymap.toml` (vr-ide's own
//! key set, USER-5, D250) overridden per key by the user's `keymap.toml`. Every binding is one
//! key; there are no key sequences (D250).

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::Path;

use crate::{ConfigWarning, KeyChord, read_user_file, toml_error_message};

const DEFAULT_KEYMAP: &str = include_str!("../defaults/keymap.toml");
const BUILT_IN: &str = "<built-in keymap>";

/// Keys already bound by the file being applied, by (context, chord), with the key text as written.
type Seen = HashMap<(Context, KeyChord), String>;

/// Command names bound by the defaults before the command exists (D8 `focus.editor`, M5);
/// [`Keymap::validate`] accepts them so the startup notice stays quiet.
pub const RESERVED_COMMANDS: &[&str] = &["focus.editor"];

/// Where a binding applies, like VS Code's `when`. Lookup in a context falls back to `Global`,
/// except `Terminal`: a focused terminal receives every key that it does not bind itself (D8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Context {
    Global,
    Editor,
    Explorer,
    Palette,
    Terminal,
}

impl Context {
    pub const ALL: [Context; 5] = [
        Context::Global,
        Context::Editor,
        Context::Explorer,
        Context::Palette,
        Context::Terminal,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Context::Global => "global",
            Context::Editor => "editor",
            Context::Explorer => "explorer",
            Context::Palette => "palette",
            Context::Terminal => "terminal",
        }
    }

    pub fn from_name(name: &str) -> Option<Context> {
        Context::ALL.into_iter().find(|c| c.name() == name)
    }
}

impl fmt::Display for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Keymap {
    bindings: BTreeMap<(Context, KeyChord), String>,
}

impl Keymap {
    /// Built-in defaults plus the user file at `user` (a missing file is not an error). Invalid TOML
    /// in the user file is reported and the whole file ignored; a bad entry is reported and skipped.
    pub fn load(user: Option<&Path>) -> (Keymap, Vec<ConfigWarning>) {
        let mut km = Keymap::default();
        let mut warnings = Vec::new();
        let builtin = Path::new(BUILT_IN);
        match toml::from_str::<toml::Table>(DEFAULT_KEYMAP) {
            Ok(t) => km.apply(t, builtin, &mut warnings),
            Err(e) => warnings.push(ConfigWarning::Malformed {
                file: builtin.to_owned(),
                message: toml_error_message(DEFAULT_KEYMAP, &e),
            }),
        }
        if let Some(path) = user
            && let Some(text) = read_user_file(path, &mut warnings)
        {
            let text = quote_bare_keys(&text);
            match toml::from_str::<toml::Table>(&text) {
                Ok(t) => km.apply(t, path, &mut warnings),
                Err(e) => warnings.push(ConfigWarning::Malformed {
                    file: path.to_owned(),
                    message: toml_error_message(&text, &e),
                }),
            }
        }
        (km, warnings)
    }

    /// Command for `k` when the editor has focus (editor bindings, then global ones).
    pub fn command_for(&self, k: &KeyChord) -> Option<&str> {
        self.command_for_in(Context::Editor, k)
    }

    /// Command for `k` in `ctx`: the context's own binding, then the global one (not for `Terminal`).
    pub fn command_for_in(&self, ctx: Context, k: &KeyChord) -> Option<&str> {
        let own = self.bindings.get(&(ctx, k.clone()));
        let fallback = || match ctx {
            Context::Global | Context::Terminal => None,
            _ => self.bindings.get(&(Context::Global, k.clone())),
        };
        own.or_else(fallback).map(String::as_str)
    }

    /// Every chord bound to `cmd` in any context, without duplicates (palette and docs show these).
    pub fn keys_for(&self, cmd: &str) -> Vec<KeyChord> {
        let mut keys: Vec<KeyChord> = Vec::new();
        for ((_, k), c) in &self.bindings {
            if c == cmd && !keys.contains(k) {
                keys.push(k.clone());
            }
        }
        keys
    }

    /// All bindings, ordered by context then chord.
    pub fn bindings(&self) -> impl Iterator<Item = (Context, &KeyChord, &str)> {
        self.bindings
            .iter()
            .map(|((ctx, k), c)| (*ctx, k, c.as_str()))
    }

    /// One warning per binding whose command is neither in `known` nor in [`RESERVED_COMMANDS`].
    pub fn validate(&self, known: &[&str]) -> Vec<ConfigWarning> {
        self.bindings()
            .filter(|(_, _, c)| !known.contains(c) && !RESERVED_COMMANDS.contains(c))
            .map(|(context, k, c)| ConfigWarning::UnknownCommand {
                context,
                key: k.to_string(),
                command: c.to_owned(),
            })
            .collect()
    }

    fn apply(&mut self, table: toml::Table, file: &Path, w: &mut Vec<ConfigWarning>) {
        let mut seen = Seen::new();
        for (key, value) in table {
            match value {
                toml::Value::String(cmd) => {
                    self.bind(Context::Global, &key, cmd, file, w, &mut seen)
                }
                toml::Value::Table(t) => {
                    if let Some(ctx) = Context::from_name(&key) {
                        self.apply_context(ctx, t, file, w, &mut seen);
                    } else if t.contains_key("command") {
                        self.apply_when_form(&key, t, file, w, &mut seen);
                    } else {
                        w.push(ConfigWarning::UnknownContext {
                            file: file.to_owned(),
                            name: key,
                        });
                    }
                }
                _ => w.push(invalid_value(file, &key, "expected a command name string")),
            }
        }
    }

    fn apply_context(
        &mut self,
        ctx: Context,
        table: toml::Table,
        file: &Path,
        w: &mut Vec<ConfigWarning>,
        seen: &mut Seen,
    ) {
        for (key, value) in table {
            match value {
                toml::Value::String(cmd) => self.bind(ctx, &key, cmd, file, w, seen),
                _ => w.push(invalid_value(file, &key, "expected a command name string")),
            }
        }
    }

    /// `"f2" = { command = "explorer.rename", when = "explorer" }`
    fn apply_when_form(
        &mut self,
        key: &str,
        t: toml::Table,
        file: &Path,
        w: &mut Vec<ConfigWarning>,
        seen: &mut Seen,
    ) {
        if let Some(extra) = t.keys().find(|k| *k != "command" && *k != "when") {
            w.push(invalid_value(
                file,
                key,
                &format!("unexpected field {extra:?}"),
            ));
            return;
        }
        let Some(toml::Value::String(cmd)) = t.get("command") else {
            w.push(invalid_value(file, key, "command must be a string"));
            return;
        };
        let ctx = match t.get("when") {
            None => Context::Global,
            Some(toml::Value::String(name)) => match Context::from_name(name) {
                Some(c) => c,
                None => {
                    w.push(ConfigWarning::UnknownContext {
                        file: file.to_owned(),
                        name: name.clone(),
                    });
                    return;
                }
            },
            Some(_) => {
                w.push(invalid_value(file, key, "when must be a string"));
                return;
            }
        };
        self.bind(ctx, key, cmd.clone(), file, w, seen);
    }

    fn bind(
        &mut self,
        ctx: Context,
        key: &str,
        cmd: String,
        file: &Path,
        w: &mut Vec<ConfigWarning>,
        seen: &mut Seen,
    ) {
        let chord = match key.parse::<KeyChord>() {
            Ok(c) => c,
            Err(e) => {
                w.push(ConfigWarning::InvalidKey {
                    file: file.to_owned(),
                    key: key.to_owned(),
                    reason: e.to_string(),
                });
                return;
            }
        };
        // "ctrl+s" and "ctrl+S" in one file land on the same chord; keep the first and say so.
        if let Some(first) = seen.get(&(ctx, chord.clone())) {
            w.push(ConfigWarning::DuplicateKey {
                file: file.to_owned(),
                context: ctx,
                first: first.clone(),
                second: key.to_owned(),
            });
            return;
        }
        seen.insert((ctx, chord.clone()), key.to_owned());
        let cmd = cmd.trim();
        if cmd.is_empty() {
            self.bindings.remove(&(ctx, chord));
        } else {
            self.bindings.insert((ctx, chord), cmd.to_owned());
        }
    }
}

fn invalid_value(file: &Path, key: &str, message: &str) -> ConfigWarning {
    ConfigWarning::InvalidValue {
        file: file.to_owned(),
        key: key.to_owned(),
        message: message.to_owned(),
    }
}

/// Lets users write `ctrl+s = "file.save"` (as in AC-5) although TOML needs `"ctrl+s"`: a line whose
/// key holds characters a bare TOML key cannot have gets that key quoted. Headers, comments and
/// quoted keys pass through.
fn quote_bare_keys(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    for line in text.split_inclusive('\n') {
        out.push_str(&quote_line(line).unwrap_or_else(|| line.to_owned()));
    }
    out
}

fn quote_line(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with(['#', '[', '"', '\'']) {
        return None;
    }
    // The separator is the first `=` whose next non-space character opens a value (quote or brace):
    // the key may itself hold `=`, `'`, `"` or `{` (`ctrl+= = "x"`, `ctrl+' = "x"`), and an inline
    // table value holds later `= "` pairs of its own.
    let (eq, value_at) = line.match_indices('=').find_map(|(i, _)| {
        let rest = &line[i + 1..];
        let v = i + 1 + (rest.len() - rest.trim_start().len());
        line[v..].starts_with(['"', '\'', '{']).then_some((i, v))
    })?;
    let key = line[..eq].trim();
    let bare = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '-';
    if key.is_empty() || key.chars().all(bare) {
        return None;
    }
    let indent = &line[..line.len() - trimmed.len()];
    let escaped = key.replace('\\', "\\\\").replace('"', "\\\"");
    Some(format!("{indent}\"{escaped}\" = {}", &line[value_at..]))
}

#[cfg(test)]
mod tests {
    use super::quote_bare_keys;

    #[test]
    fn quotes_only_keys_that_need_it() {
        let src = "# c\n[explorer]\nctrl+s = \"a\"\n\"f2\" = \"b\"\nf3 = \"c\"\n  ctrl+\\ = \"d\"\nctrl+= = \"e\"\n";
        let want = "# c\n[explorer]\n\"ctrl+s\" = \"a\"\n\"f2\" = \"b\"\nf3 = \"c\"\n  \"ctrl+\\\\\" = \"d\"\n\"ctrl+=\" = \"e\"\n";
        assert_eq!(quote_bare_keys(src), want);
    }
}
