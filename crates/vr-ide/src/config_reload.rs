//! Live reload of `settings.toml` and `keymap.toml` (T24, D279 (6)): saved in vr-ide (the save
//! hook in `commands::file`) or by another program (a watch on each of the two files), the file
//! is read again at once.
//!
//! Settings: only the settings whose value *in the file* changed since it was last applied are
//! applied, through the same diff-apply as `/set` (`commands::slash::apply_settings`), so a
//! `/set`, a ctrl+h or a terminal resize of this session on another setting stays. Keymap: the
//! keys in use are replaced. A file with any problem (bad TOML, an unknown name, a wrong value, a
//! bad key, an unknown command) changes nothing: the old values stay and an error notice names
//! the problems.

use std::path::{Path, PathBuf};

use vr_config::{ConfigWarning, Keymap, Settings};

use crate::app::App;
use crate::workspace::normalize;

pub const SETTINGS_FILE: &str = "settings.toml";
pub const KEYMAP_FILE: &str = "keymap.toml";

/// The file `p` names, for comparing two spellings: symlinks resolved, and for a file that does
/// not exist (yet) its folder resolved (as `vr_core::Watcher` keys its watches).
fn identity(p: &Path) -> PathBuf {
    std::fs::canonicalize(p)
        .ok()
        .or_else(|| {
            let parent = p.parent().filter(|d| !d.as_os_str().is_empty())?;
            Some(std::fs::canonicalize(parent).ok()?.join(p.file_name()?))
        })
        .unwrap_or_else(|| normalize(p))
}

/// What a reloaded file had wrong, as shown in the notice.
fn problem_text(w: &ConfigWarning) -> String {
    match w {
        // Its own text says "built-in defaults kept", which is not what a reload does.
        ConfigWarning::Malformed { file, message } => {
            format!("{}: invalid TOML: {message}", file.display())
        }
        other => other.to_string(),
    }
}

impl App {
    /// The configuration file `name` of the configuration folder, when one is known.
    fn config_file(&self, name: &str) -> Option<PathBuf> {
        self.config_dir.as_ref().map(|d| normalize(&d.join(name)))
    }

    /// `p` is settings.toml or keymap.toml of the configuration folder, by any name (a symlink
    /// or its target).
    pub(crate) fn is_config_file(&self, p: &Path) -> bool {
        self.config_kind(p).is_some()
    }

    /// Which configuration file `p` is, compared by the file it names: the watcher keys files
    /// by their real path, so a tab on a symlink's target reports events under that path.
    fn config_kind(&self, p: &Path) -> Option<&'static str> {
        let p = identity(p);
        [SETTINGS_FILE, KEYMAP_FILE]
            .into_iter()
            .find(|n| self.config_file(n).is_some_and(|c| identity(&c) == p))
    }

    /// Watches settings.toml and keymap.toml (existing or not yet made) so a change by another
    /// program is applied. Quiet when the folder does not exist yet: `/config` and `/keymap`
    /// call this again after making it.
    pub(crate) fn watch_config_files(&mut self) {
        let files: Vec<PathBuf> = [SETTINGS_FILE, KEYMAP_FILE]
            .iter()
            .filter_map(|n| self.config_file(n))
            .collect();
        let Some(w) = &mut self.watcher else {
            return;
        };
        for f in files {
            if f.parent().is_some_and(Path::is_dir) {
                let _ = w.watch(&f);
            }
        }
    }

    /// After a save of ours (`saved`) or a watcher event for `p`: a configuration file is read
    /// again. "Nothing changed" is said only after a save of ours: a watcher event can be the
    /// file `/config` just made, or a touch.
    pub(crate) fn config_file_changed(&mut self, p: &Path, saved: bool) {
        let p = normalize(p);
        let Some(kind) = self.config_kind(&p) else {
            return;
        };
        // Gone for a moment (an editor's save) or deleted: the values in use stay. Empty after
        // another program's write: most likely truncated and not written yet (a slow writer);
        // the write that follows brings another event. A save of ours is taken as it is.
        match std::fs::metadata(&p) {
            Ok(m) if m.is_file() && (saved || m.len() > 0) => {}
            _ => return,
        }
        if kind == SETTINGS_FILE {
            self.reload_settings(&p, saved);
        } else {
            self.reload_keymap(&p, saved);
        }
    }

    fn reload_settings(&mut self, path: &Path, saved: bool) {
        let (file, warnings) = Settings::load(Some(path));
        let mut problems: Vec<String> = warnings.iter().map(problem_text).collect();
        if crate::theme::Theme::named(&file.theme).is_none() {
            problems.push(format!(
                "{}: theme {:?} is not one of {}",
                path.display(),
                file.theme,
                crate::commands::slash::THEMES.join(", ")
            ));
        }
        if !problems.is_empty() {
            self.error(format!(
                "{SETTINGS_FILE} not applied, old values kept: {}",
                problems.join("; ")
            ));
            return;
        }
        let old = std::mem::replace(&mut self.settings_file, file.clone());
        let mut next = self.settings.clone();
        let mut changed: Vec<String> = Vec::new();
        for name in Settings::names() {
            let value = file.value_text(&name);
            if old.value_text(&name) == value {
                continue;
            }
            // The file's value already passed load's checks; with_value repeats them.
            if let Some(v) = value
                && let Ok(s) = next.with_value(&name, &v)
            {
                next = s;
                changed.push(name);
            }
        }
        if old.ai_tools != file.ai_tools {
            next.ai_tools = file.ai_tools.clone();
            changed.push("ai_tools".to_string());
        }
        if changed.is_empty() {
            if saved {
                self.info(format!("{SETTINGS_FILE}: no setting changed"));
            }
            return;
        }
        crate::commands::slash::apply_settings(self, next);
        self.info(format!("{SETTINGS_FILE} applied: {}", changed.join(", ")));
    }

    fn reload_keymap(&mut self, path: &Path, saved: bool) {
        let (keymap, warnings) = Keymap::load(Some(path));
        let names = crate::commands::command_names();
        let mut problems: Vec<String> = warnings.iter().map(problem_text).collect();
        problems.extend(keymap.validate(&names).iter().map(problem_text));
        if !problems.is_empty() {
            self.error(format!(
                "{KEYMAP_FILE} not applied, old values kept: {}",
                problems.join("; ")
            ));
            return;
        }
        if keymap == self.keymap {
            if saved {
                self.info(format!("{KEYMAP_FILE}: no key changed"));
            }
            return;
        }
        self.keymap = keymap;
        self.dirty = true;
        // The startup check (`report_keyboard`) again: a key a legacy terminal cannot send
        // is named rather than silently applied.
        match self.unreachable_keys_notice() {
            Some(mut n) => {
                n.text = format!("{KEYMAP_FILE} applied. {}", n.text);
                self.push_notice(n);
            }
            None => self.info(format!("{KEYMAP_FILE} applied")),
        }
    }
}
