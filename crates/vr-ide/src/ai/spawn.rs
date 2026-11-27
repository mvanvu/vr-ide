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

/// What a start runs: one of the picker's modes, or one known conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start<'a> {
    Mode(StartMode),
    /// The conversation with this id ([`args_by_id`]); the session counts as continued.
    Id(&'a str),
}

impl Start<'_> {
    pub fn mode(self) -> StartMode {
        match self {
            Start::Mode(m) => m,
            Start::Id(_) => StartMode::Continue,
        }
    }
}

/// The words `tool` resumes one conversation by its id with (the id follows as its own
/// argument). Fixed per tool like the account variables, never from settings: claude
/// `--resume <id>`, codex `resume <id>` (its subcommand, codex 0.144.1; SEC-32).
pub fn resume_words(tool: &str) -> Option<&'static [&'static str]> {
    match tool {
        "claude" => Some(&["--resume"]),
        "codex" => Some(&["resume"]),
        _ => None,
    }
}

/// What codex 0.144.1 prints (and then exits) when `resume <id>` names no saved conversation.
pub const CODEX_NOT_FOUND: &str = "No saved session found with ID";

/// Most non-blank characters codex's not-found line has after the id: the rest of its one
/// sentence ("<id>. Run `codex resume` without an ID to choose from existing sessions.").
const NOT_FOUND_REST_MAX: usize = 120;

/// `screen` (rows joined by `\n`) ends with codex's exit message saying it found no conversation
/// `id`: with every blank removed (a row loses its trailing blanks and a wrap may fall on a
/// space), the rows after the last blank one hold [`CODEX_NOT_FOUND`] + `id`, followed by no
/// more than the rest of that sentence. Whatever precedes it (codex's binary holds the sentence
/// alone; a caller may add `Error: `) does not matter. A conversation shown again that merely
/// quotes the line is not that message.
pub fn codex_not_found(screen: &str, id: &str) -> bool {
    let squeeze = |s: &str| s.split_whitespace().collect::<String>();
    let rows: Vec<&str> = screen.lines().collect();
    let Some(end) = rows.iter().rposition(|r| !r.trim().is_empty()) else {
        return false;
    };
    let start = rows[..end]
        .iter()
        .rposition(|r| r.trim().is_empty())
        .map_or(0, |r| r + 1);
    let block = squeeze(&rows[start..=end].concat());
    let want = format!("{}{id}", squeeze(CODEX_NOT_FOUND));
    block
        .rfind(&want)
        .is_some_and(|at| block[at + want.len()..].chars().count() <= NOT_FOUND_REST_MAX)
}

/// The arguments resuming conversation `id` (SEC-24, SEC-31): the tool's `args`, its resume
/// words, the id — each its own argument, never with `continue_args`. `None` unless the id is a
/// canonical lowercase UUID (it could read as an option otherwise) and the tool resumes by id.
pub fn args_by_id(tool: &AiTool, id: &str) -> Option<Vec<String>> {
    let words = resume_words(&tool.name)?;
    if !super::usage::valid_session_id(id) {
        return None;
    }
    Some(
        tool.args
            .iter()
            .cloned()
            .chain(words.iter().map(|w| w.to_string()))
            .chain([id.to_string()])
            .collect(),
    )
}

/// How a restored or exited session starts again (SEC-31): by its id when one is known and
/// usable, in place of the continue arguments, which with a shared history would pick the
/// folder's latest conversation of any tab and account; else [`again_mode`].
pub fn again<'a>(tool: &AiTool, id: Option<&'a str>) -> Start<'a> {
    let mode = again_mode(tool);
    match id {
        Some(id) if mode == StartMode::Continue && args_by_id(tool, id).is_some() => Start::Id(id),
        _ => Start::Mode(mode),
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

    /// SEC-24, SEC-31: only a canonical lowercase UUID becomes an argument, as its own word
    /// after the tool's fixed resume word, never beside the continue arguments.
    #[test]
    fn resume_by_id_takes_only_a_canonical_uuid() {
        const ID: &str = "0f8e2c1a-7b3d-4e5f-9a1b-2c3d4e5f6a7b";
        let mut claude = tool();
        claude.name = "claude".into();
        assert_eq!(
            args_by_id(&claude, ID).unwrap(),
            ["--model", "o4 mini", "--resume", ID]
        );
        assert_eq!(again(&claude, Some(ID)), Start::Id(ID));
        assert_eq!(again(&claude, None), Start::Mode(StartMode::Continue));
        for bad in [
            "--dangerously-skip-permissions",
            "-x",
            "abc-123",
            "",
            "0F8E2C1A-7B3D-4E5F-9A1B-2C3D4E5F6A7B",
            "0f8e2c1a-7b3d-4e5f-9a1b-2c3d4e5f6a7b\n",
            " 0f8e2c1a-7b3d-4e5f-9a1b-2c3d4e5f6a7b",
        ] {
            assert_eq!(args_by_id(&claude, bad), None, "{bad:?}");
            assert_eq!(
                again(&claude, Some(bad)),
                Start::Mode(StartMode::Continue),
                "{bad:?}"
            );
        }
        // codex (SEC-32): `resume <id>` as two arguments, never `resume --last` beside it.
        assert_eq!(
            args_by_id(&tool(), ID).unwrap(),
            ["--model", "o4 mini", "resume", ID]
        );
        assert_eq!(again(&tool(), Some(ID)), Start::Id(ID));
        assert_eq!(
            args_by_id(&tool(), "--dangerously-bypass-approvals-and-sandbox"),
            None
        );
        // A tool without resume-by-id words and one that cannot continue keep their mode.
        let mut gemini = tool();
        gemini.name = "gemini".into();
        assert_eq!(args_by_id(&gemini, ID), None);
        assert_eq!(again(&gemini, Some(ID)), Start::Mode(StartMode::Continue));
        claude.continue_args.clear();
        assert_eq!(again(&claude, Some(ID)), Start::Mode(StartMode::New));
    }

    /// `line` the way a terminal `width` wide shows it: cut into rows, each without its trailing
    /// blanks.
    fn wrapped(line: &str, width: usize) -> String {
        let chars: Vec<char> = line.chars().collect();
        chars
            .chunks(width)
            .map(|c| c.iter().collect::<String>().trim_end().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn codex_not_found_line_at_every_width_and_only_as_the_exit_message() {
        const ID: &str = "0f8e2c1a-7b3d-4e5f-9a1b-2c3d4e5f6a7b";
        // codex 0.144.1's sentence as its binary holds it; whatever prefix may precede it.
        let line = format!(
            "No saved session found with ID {ID}. Run `codex resume` without an ID to choose \
             from existing sessions."
        );
        for prefix in ["", "Error: ", "! "] {
            // From 2: one column wide, each space of the sentence would be a blank row.
            for width in 2..=200 {
                let screen = format!(
                    "ARGV:-c|notify=…|resume|{ID}|\nACCT:x\n{}\n\n",
                    wrapped(&format!("{prefix}{line}"), width)
                );
                assert!(codex_not_found(&screen, ID), "width {width}:\n{screen}");
            }
        }
        // Another conversation's id.
        let other = line.replace(ID, "11111111-2222-4333-8444-555555555555");
        assert!(!codex_not_found(&other, ID));
        // A conversation's tail quoting it, then codex's own rows.
        let tail = format!("{line}\n\n› Ask Codex to do anything\nquit\n");
        assert!(!codex_not_found(&tail, ID));
        let long = format!("{line} {}", "more ".repeat(40));
        assert!(!codex_not_found(&long, ID));
        assert!(!codex_not_found("", ID));
    }
}
