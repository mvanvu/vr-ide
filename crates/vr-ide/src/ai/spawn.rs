//! How an agent starts (SEC-8): the program and its arguments come from `ai_tools` only, the
//! program is found in the absolute `PATH` entries (or given as an absolute path), it is started
//! as argv without a shell, in a canonical workspace folder, and its environment loses vr-ide's
//! own variables and the relative `PATH` entries.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use vr_config::AiTool;

/// Which conversation a session starts on (the picker's second step, D14).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StartMode {
    /// A new conversation: the tool's `args` only.
    #[default]
    New,
    /// The last conversation: `args` then `continue_args` (restore and restart use it too).
    Continue,
    /// An earlier one, chosen in the tool's own list: `args` then `resume_args`.
    Resume,
}

impl StartMode {
    /// The picker's wording.
    pub fn title(self) -> &'static str {
        match self {
            StartMode::New => "New",
            StartMode::Continue => "Continue last",
            StartMode::Resume => "Choose earlier",
        }
    }
}

/// The modes `tool` offers, New first; a tool without resume arguments offers only New.
pub fn modes(tool: &AiTool) -> Vec<StartMode> {
    let mut out = vec![StartMode::New];
    if !tool.continue_args.is_empty() {
        out.push(StartMode::Continue);
    }
    if !tool.resume_args.is_empty() {
        out.push(StartMode::Resume);
    }
    out
}

/// The arguments for a start in `mode`; a mode the tool has no arguments for is a plain start.
pub fn args(tool: &AiTool, mode: StartMode) -> Vec<String> {
    let extra: &[String] = match mode {
        StartMode::New => &[],
        StartMode::Continue => &tool.continue_args,
        StartMode::Resume => &tool.resume_args,
    };
    tool.args.iter().chain(extra).cloned().collect()
}

/// The mode a restored or exited session starts again in: Continue when the tool has it.
pub fn again_mode(tool: &AiTool) -> StartMode {
    if tool.continue_args.is_empty() {
        StartMode::New
    } else {
        StartMode::Continue
    }
}

/// The start as a line for the screen (`claude --continue`); a word with a space or a quote is
/// shown quoted. Display only: nothing ever runs this text.
pub fn command_line(tool: &AiTool, mode: StartMode) -> String {
    std::iter::once(tool.command.clone())
        .chain(args(tool, mode))
        .map(|w| {
            if w.is_empty() || w.contains([' ', '"', '\'', '\\']) {
                format!("{w:?}")
            } else {
                w
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Where `command` is: an absolute path to an executable file, or a bare name found in an
/// absolute entry of `path`. A relative path (`./claude`, `bin/claude`) and a name found only
/// in a relative entry are never used: they would run a program of the workspace (SEC-8).
pub fn resolve(command: &str, path: Option<&OsStr>) -> Option<PathBuf> {
    if command.is_empty() || command.chars().any(char::is_control) {
        return None;
    }
    if command.contains('/') {
        let p = Path::new(command);
        let executable = p
            .metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0);
        return (p.is_absolute() && executable).then(|| p.to_path_buf());
    }
    crate::ssh::find_in_path(path?, command)
}

/// `PATH` without its relative entries (empty ones included: they mean the current folder).
pub fn absolute_path(path: &OsStr) -> OsString {
    let kept: Vec<&[u8]> = path
        .as_bytes()
        .split(|&b| b == b':')
        .filter(|d| d.first() == Some(&b'/'))
        .collect();
    OsString::from(OsStr::from_bytes(&kept.join(&b':')))
}

/// vr-ide's own variables in `vars`: an agent never gets them.
pub fn internal_vars(vars: impl Iterator<Item = (OsString, OsString)>) -> Vec<OsString> {
    vars.map(|(k, _)| k)
        .filter(|k| k.as_bytes().starts_with(b"VR_IDE_"))
        .collect()
}

/// Variables an account adds to its session's environment (D11, SEC-18): the tool's own variable
/// pointing at the account folder, checked now; nothing for `None` (the tool's own config). Only
/// the spawn of that session gets them.
pub fn account_env(
    state: Option<&Path>,
    tool: &AiTool,
    account: Option<&str>,
) -> Result<Vec<(OsString, OsString)>, String> {
    super::accounts::env(state, &tool.name, account)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool() -> AiTool {
        AiTool {
            name: "codex".into(),
            command: "codex".into(),
            args: vec!["--model".into(), "o4 mini".into()],
            continue_args: vec!["resume".into(), "--last".into()],
            resume_args: vec!["resume".into()],
        }
    }

    #[test]
    fn args_per_mode_and_display() {
        let t = tool();
        assert_eq!(args(&t, StartMode::New), ["--model", "o4 mini"]);
        assert_eq!(
            args(&t, StartMode::Continue),
            ["--model", "o4 mini", "resume", "--last"]
        );
        assert_eq!(
            args(&t, StartMode::Resume),
            ["--model", "o4 mini", "resume"]
        );
        assert_eq!(
            command_line(&t, StartMode::Continue),
            "codex --model \"o4 mini\" resume --last"
        );
        assert_eq!(
            modes(&t),
            [StartMode::New, StartMode::Continue, StartMode::Resume]
        );
        assert_eq!(again_mode(&t), StartMode::Continue);
        let plain = AiTool {
            continue_args: vec![],
            resume_args: vec![],
            ..t
        };
        assert_eq!(modes(&plain), [StartMode::New]);
        assert_eq!(again_mode(&plain), StartMode::New);
        assert_eq!(args(&plain, StartMode::Continue), ["--model", "o4 mini"]);
    }

    #[test]
    fn resolve_uses_absolute_entries_only() {
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        let exe = bin.join("agent");
        std::fs::write(&exe, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        let plain = bin.join("notexec");
        std::fs::write(&plain, "x").unwrap();
        let path = OsString::from(format!("bin:.::{}", bin.display()));
        assert_eq!(resolve("agent", Some(&path)), Some(exe.clone()));
        assert_eq!(resolve("notexec", Some(&path)), None);
        // Only a relative entry has it: refused.
        let rel = OsString::from("bin:.");
        assert_eq!(resolve("agent", Some(&rel)), None);
        // Paths: absolute and executable only.
        assert_eq!(resolve(exe.to_str().unwrap(), None), Some(exe.clone()));
        assert_eq!(resolve("./agent", Some(&path)), None);
        assert_eq!(resolve("bin/agent", Some(&path)), None);
        assert_eq!(resolve(plain.to_str().unwrap(), None), None);
        assert_eq!(resolve("agent; touch /tmp/p", Some(&path)), None);
        assert_eq!(resolve("", Some(&path)), None);
        assert_eq!(resolve("agent", None), None);
    }

    #[test]
    fn path_and_env_are_cleaned() {
        assert_eq!(
            absolute_path(OsStr::new("/usr/bin::.:bin:/opt/x/bin:")),
            OsString::from("/usr/bin:/opt/x/bin")
        );
        let vars = [
            ("VR_IDE_LOG", "1"),
            ("HOME", "/home/u"),
            ("VR_IDEX", "kept"),
            ("VR_IDE_", "x"),
        ]
        .map(|(k, v)| (OsString::from(k), OsString::from(v)));
        assert_eq!(
            internal_vars(vars.into_iter()),
            [OsString::from("VR_IDE_LOG"), OsString::from("VR_IDE_")]
        );
    }
}
