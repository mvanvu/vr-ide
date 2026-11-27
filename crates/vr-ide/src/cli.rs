//! Command-line handling: turns the launch arguments into the workspace roots and the files
//! to open.
//!
//! `vr-ide` opens the current folder; `vr-ide dir1 dir2 …` opens each folder as a root;
//! `vr-ide file` opens the file and adds its project (git work tree, else its folder) as a
//! root unless a folder argument already holds it; `vr-ide x.vr-workspace` opens the roots
//! listed in that file (alone: a workspace file is not mixed with other arguments);
//! `vr-ide --session` (alone; the desktop launcher) opens the home folder, which the saved
//! session then replaces (`crate::session`).
//! `vr-ide --install-desktop` / `--uninstall-desktop` (alone) add or remove the app-menu entry.
use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::workspace::{Workspace, WorkspaceError};

#[derive(Debug, PartialEq, Eq)]
pub struct Launch {
    pub workspace: Workspace,
    /// Files named on the command line, opened in this order.
    pub open_files: Vec<PathBuf>,
    /// Folders left out (nested in another root), for the startup notice.
    pub notices: Vec<String>,
    /// `--session`: restore the saved session and keep it (D2). Every other launch leaves
    /// the session file alone.
    pub session: bool,
}

#[derive(Debug)]
pub enum CliError {
    NotFound(PathBuf),
    TooManyArgs,
    Io(PathBuf, std::io::Error),
    Workspace(PathBuf, WorkspaceError),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::NotFound(p) => write!(f, "path does not exist: {}", p.display()),
            CliError::TooManyArgs => write!(
                f,
                "usage: vr-ide [<dir>|<file>]... | vr-ide <file>.{} | vr-ide {SESSION_FLAG} | vr-ide {INSTALL_DESKTOP_FLAG} | vr-ide {UNINSTALL_DESKTOP_FLAG}",
                vr_config::WORKSPACE_EXT
            ),
            CliError::Io(p, e) => write!(f, "cannot access {}: {e}", p.display()),
            CliError::Workspace(p, e) => write!(f, "{}: {e}", p.display()),
        }
    }
}

impl std::error::Error for CliError {}

/// The launcher's flag: restore and keep the session.
pub const SESSION_FLAG: &str = "--session";
pub const INSTALL_DESKTOP_FLAG: &str = "--install-desktop";
pub const UNINSTALL_DESKTOP_FLAG: &str = "--uninstall-desktop";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopAction {
    Install,
    Uninstall,
}

/// `--install-desktop` / `--uninstall-desktop`: each only as the single argument.
pub fn desktop_action(args: &[OsString]) -> Result<Option<DesktopAction>, CliError> {
    let action = |a: &OsString| match a.to_str() {
        Some(INSTALL_DESKTOP_FLAG) => Some(DesktopAction::Install),
        Some(UNINSTALL_DESKTOP_FLAG) => Some(DesktopAction::Uninstall),
        _ => None,
    };
    match args.iter().find_map(action) {
        None => Ok(None),
        Some(_) if args.len() > 1 => Err(CliError::TooManyArgs),
        found => Ok(found),
    }
}

/// The statusLine helper (SEC-23): `vr-ide --ai-statusline <report>`, run by claude.
pub const AI_STATUSLINE_FLAG: &str = "--ai-statusline";

/// The helper's report path, only for exactly the two arguments `--ai-statusline <path>`.
pub fn ai_statusline(args: &[OsString]) -> Option<PathBuf> {
    match args {
        [flag, report] if flag == AI_STATUSLINE_FLAG => Some(PathBuf::from(report)),
        _ => None,
    }
}

/// codex's notify program (SEC-32): `vr-ide --ai-codex-notify <report> <json>`, run by codex.
pub const AI_CODEX_NOTIFY_FLAG: &str = "--ai-codex-notify";

/// The report path and codex's payload, only for exactly the three arguments
/// `--ai-codex-notify <report> <json>`.
pub fn ai_codex_notify(args: &[OsString]) -> Option<(PathBuf, OsString)> {
    match args {
        [flag, report, payload] if flag == AI_CODEX_NOTIFY_FLAG => {
            Some((PathBuf::from(report), payload.clone()))
        }
        _ => None,
    }
}

/// The login window process (SEC-34): `vr-ide --login-window <tool> <account> <url>`, started
/// by the launcher below, never by claude itself.
pub const LOGIN_WINDOW_FLAG: &str = "--login-window";

/// Tool, account and URL, only for exactly the four arguments `--login-window <tool> <account>
/// <url>` with a tool that has a login window and `default` or a valid account name.
pub fn login_window(args: &[OsString]) -> Option<(String, String, String)> {
    use crate::ai::login;
    let [flag, tool, account, url] = args else {
        return None;
    };
    let (tool, account, url) = (tool.to_str()?, account.to_str()?, url.to_str()?);
    let valid = flag == LOGIN_WINDOW_FLAG && login::tool_ok(tool) && login::account_ok(account);
    valid.then(|| (tool.to_string(), account.to_string(), url.to_string()))
}

/// A call through an account's launcher (D63): what claude's `$BROWSER <url>` becomes.
#[derive(Debug, PartialEq, Eq)]
pub struct LoginLauncher {
    /// The state folder the launcher lies in.
    pub state: PathBuf,
    pub tool: String,
    pub account: String,
    /// The one argument: a URL, or whatever a program below claude wants opened.
    pub target: OsString,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LauncherCall {
    /// Not started under a launcher name.
    No,
    /// A launcher name in any other than the one accepted form: the caller exits; a launcher
    /// never starts the editor.
    Refused,
    Run(LoginLauncher),
}

/// claude runs `$BROWSER` as one program with the URL as its only argument, so the tool and
/// account are in the program's name: `<state>/ai-login/bin/vr-ide-login.<tool>.<account>`,
/// an absolute path, plus exactly one argument.
pub fn login_launcher(argv0: &std::ffi::OsStr, args: &[OsString]) -> LauncherCall {
    use crate::ai::login;
    let path = Path::new(argv0);
    let Some(rest) = path
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix(login::LAUNCHER_PREFIX))
    else {
        return LauncherCall::No;
    };
    let named =
        |dir: Option<&Path>, name: &str| dir.and_then(Path::file_name) == Some(name.as_ref());
    let bin = path.parent();
    let root = bin.and_then(Path::parent);
    let state = root.and_then(Path::parent);
    let account = rest
        .split_once('.')
        .filter(|(tool, account)| login::tool_ok(tool) && login::account_ok(account));
    match (state, account, args) {
        (Some(state), Some((tool, account)), [target])
            if path.is_absolute() && named(bin, login::BIN_DIR) && named(root, login::DIR) =>
        {
            LauncherCall::Run(LoginLauncher {
                state: state.to_path_buf(),
                tool: tool.to_string(),
                account: account.to_string(),
                target: target.clone(),
            })
        }
        _ => LauncherCall::Refused,
    }
}

/// Resolves launch arguments (without argv[0]) against `cwd`.
pub fn resolve(args: &[OsString], cwd: &Path) -> Result<Launch, CliError> {
    if args.iter().any(|a| a == SESSION_FLAG) {
        if args.len() > 1 {
            return Err(CliError::TooManyArgs);
        }
        let home = crate::session::home_dir().unwrap_or_else(|| cwd.to_path_buf());
        return Ok(Launch {
            workspace: Workspace::single(canonical(&home)?),
            open_files: Vec::new(),
            notices: Vec::new(),
            session: true,
        });
    }
    if args.is_empty() {
        return Ok(Launch {
            workspace: Workspace::single(canonical(cwd)?),
            open_files: Vec::new(),
            notices: Vec::new(),
            session: false,
        });
    }
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for arg in args {
        let target = cwd.join(arg);
        match std::fs::metadata(&target) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(CliError::NotFound(target));
            }
            Err(e) => return Err(CliError::Io(target, e)),
        }
        let target = canonical(&target)?;
        if target.is_dir() {
            dirs.push(target);
        } else if vr_config::is_workspace_file(&target) {
            if args.len() > 1 {
                return Err(CliError::TooManyArgs);
            }
            let (workspace, refused) = Workspace::from_file(&target)
                .map_err(|e| CliError::Workspace(target.clone(), e))?;
            return Ok(Launch {
                workspace,
                open_files: Vec::new(),
                notices: notices(refused),
                session: false,
            });
        } else {
            files.push(target);
        }
    }
    // Folders first, in order; a file adds its project only when no root holds it yet.
    let mut roots = dirs;
    for f in &files {
        if !roots.iter().any(|r| f.starts_with(r)) {
            roots.push(project_of(f));
        }
    }
    let (workspace, refused) =
        Workspace::from_paths(&roots).map_err(|e| CliError::Workspace(cwd.to_path_buf(), e))?;
    Ok(Launch {
        workspace,
        open_files: files,
        notices: notices(refused),
        session: false,
    })
}

/// The same folder twice is ignored silently; nesting is said (spec W edge cases).
fn notices(refused: Vec<WorkspaceError>) -> Vec<String> {
    refused
        .into_iter()
        .filter(|e| !matches!(e, WorkspaceError::Duplicate { .. }))
        .map(|e| e.to_string())
        .collect()
}

/// A file's project is its enclosing git work tree; without one, the file's own directory.
fn project_of(file: &Path) -> PathBuf {
    let parent = file
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| file.to_path_buf());
    parent
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .map(Path::to_path_buf)
        .unwrap_or(parent)
}

fn canonical(p: &Path) -> Result<PathBuf, CliError> {
    p.canonicalize()
        .map_err(|e| CliError::Io(p.to_path_buf(), e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn os(s: &str) -> OsString {
        OsString::from(s)
    }

    fn roots(l: &Launch) -> Vec<PathBuf> {
        l.workspace.paths()
    }

    #[test]
    fn cli_resolves_root() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();

        // no arg -> cwd
        let l = resolve(&[], &base).unwrap();
        assert_eq!(roots(&l), std::slice::from_ref(&base));
        assert!(l.open_files.is_empty());

        // dir arg -> canonical dir
        std::fs::create_dir_all(base.join("a/b")).unwrap();
        let l = resolve(&[os("a/../a/b")], &base).unwrap();
        assert_eq!(roots(&l), [base.join("a/b")]);
        assert!(l.open_files.is_empty());

        // file inside a git repo -> repo root + file
        let repo = base.join("repo");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        let st = Command::new("git")
            .arg("init")
            .arg("-q")
            .arg(&repo)
            .status()
            .unwrap();
        assert!(st.success());
        std::fs::write(repo.join("src/x.rs"), "fn main() {}").unwrap();
        let l = resolve(&[os("repo/src/x.rs")], &base).unwrap();
        assert_eq!(roots(&l), std::slice::from_ref(&repo));
        assert_eq!(l.open_files, [repo.join("src/x.rs")]);

        // file outside git -> parent dir
        std::fs::write(base.join("a/note.txt"), "hi").unwrap();
        let l = resolve(&[os("a/note.txt")], &base).unwrap();
        assert_eq!(roots(&l), [base.join("a")]);
        assert_eq!(l.open_files, [base.join("a/note.txt")]);

        // nonexistent -> Err with a message naming the path
        let err = resolve(&[os("missing.txt")], &base).unwrap_err();
        assert!(err.to_string().contains("missing.txt"));
    }

    #[test]
    fn cli_opens_each_folder_as_a_root() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        for d in ["api/src", "web", "lib"] {
            std::fs::create_dir_all(base.join(d)).unwrap();
        }
        std::fs::write(base.join("api/src/main.rs"), "").unwrap();
        std::fs::write(base.join("lib/x.txt"), "").unwrap();

        let l = resolve(&[os("api"), os("web")], &base).unwrap();
        assert_eq!(roots(&l), [base.join("api"), base.join("web")]);
        assert!(l.notices.is_empty());

        // A file inside a folder argument adds no root; one elsewhere adds its folder.
        let l = resolve(&[os("api/src/main.rs"), os("api"), os("lib/x.txt")], &base).unwrap();
        assert_eq!(roots(&l), [base.join("api"), base.join("lib")]);
        assert_eq!(
            l.open_files,
            [base.join("api/src/main.rs"), base.join("lib/x.txt")]
        );

        // The same folder twice (also through a link) is ignored silently.
        #[cfg(unix)]
        std::os::unix::fs::symlink(base.join("web"), base.join("web-link")).unwrap();
        let l = resolve(&[os("web"), os("./web/"), os("web-link")], &base).unwrap();
        assert_eq!(roots(&l), [base.join("web")]);
        assert!(l.notices.is_empty(), "{:?}", l.notices);

        // A nested folder is refused with a notice, whichever comes first.
        let l = resolve(&[os("api"), os("api/src")], &base).unwrap();
        assert_eq!(roots(&l), [base.join("api")]);
        assert_eq!(l.notices.len(), 1);
        assert!(l.notices[0].contains("nested"), "{:?}", l.notices);
        let l = resolve(&[os("api/src"), os("api")], &base).unwrap();
        assert_eq!(roots(&l), [base.join("api/src")]);
        assert_eq!(l.notices.len(), 1);
    }

    #[test]
    fn cli_opens_a_workspace_file_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        std::fs::create_dir_all(base.join("a")).unwrap();
        std::fs::create_dir_all(base.join("b")).unwrap();
        std::fs::write(
            base.join("team.vr-workspace"),
            "roots = [\"a\", \"b\", \"gone\"]\n",
        )
        .unwrap();
        let l = resolve(&[os("team.vr-workspace")], &base).unwrap();
        assert_eq!(
            roots(&l),
            [base.join("a"), base.join("b"), base.join("gone")]
        );
        assert!(l.workspace.roots()[2].missing());
        assert_eq!(
            l.workspace.file(),
            Some(base.join("team.vr-workspace").as_path())
        );
        assert!(matches!(
            resolve(&[os("team.vr-workspace"), os("a")], &base),
            Err(CliError::TooManyArgs)
        ));
        std::fs::write(base.join("bad.vr-workspace"), "roots = [").unwrap();
        let err = resolve(&[os("bad.vr-workspace")], &base).unwrap_err();
        assert!(err.to_string().contains("bad.vr-workspace"), "{err}");
    }

    #[test]
    fn cli_session_flag_opens_home_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let l = resolve(&[os("--session")], &base).unwrap();
        assert!(l.session);
        let home = crate::session::home_dir().unwrap_or(base.clone());
        assert_eq!(roots(&l), [home.canonicalize().unwrap()]);
        assert!(l.open_files.is_empty());
        assert!(matches!(
            resolve(&[os("--session"), os(".")], &base),
            Err(CliError::TooManyArgs)
        ));
        assert!(!resolve(&[], &base).unwrap().session);
    }

    #[test]
    fn cli_desktop_action_flags_are_alone() {
        assert_eq!(desktop_action(&[]).unwrap(), None);
        assert_eq!(desktop_action(&[os(".")]).unwrap(), None);
        assert_eq!(
            desktop_action(&[os("--install-desktop")]).unwrap(),
            Some(DesktopAction::Install)
        );
        assert_eq!(
            desktop_action(&[os("--uninstall-desktop")]).unwrap(),
            Some(DesktopAction::Uninstall)
        );
        for args in [
            vec![os("--install-desktop"), os(".")],
            vec![os("--session"), os("--uninstall-desktop")],
            vec![os("--install-desktop"), os("--uninstall-desktop")],
        ] {
            assert!(matches!(desktop_action(&args), Err(CliError::TooManyArgs)));
        }
    }

    #[test]
    fn ai_statusline_flag_alone_with_one_arg() {
        assert_eq!(
            ai_statusline(&[os("--ai-statusline"), os("/s/ai-usage/1-1-x.report")]),
            Some(PathBuf::from("/s/ai-usage/1-1-x.report"))
        );
        for args in [
            vec![],
            vec![os("--ai-statusline")],
            vec![os("--ai-statusline"), os("a"), os("b")],
            vec![os("a"), os("--ai-statusline")],
            vec![os("--session"), os("--ai-statusline")],
            vec![os("--ai-statusline=a"), os("b")],
        ] {
            assert_eq!(ai_statusline(&args), None, "{args:?}");
        }
    }

    #[test]
    fn ai_codex_notify_flag_with_exactly_two_args() {
        assert_eq!(
            ai_codex_notify(&[
                os("--ai-codex-notify"),
                os("/s/ai-usage/1-1-x.report"),
                os("{}")
            ]),
            Some((PathBuf::from("/s/ai-usage/1-1-x.report"), os("{}")))
        );
        for args in [
            vec![],
            vec![os("--ai-codex-notify")],
            vec![os("--ai-codex-notify"), os("/r")],
            vec![os("--ai-codex-notify"), os("/r"), os("{}"), os("x")],
            vec![os("/r"), os("--ai-codex-notify"), os("{}")],
            vec![os("--ai-statusline"), os("/r"), os("{}")],
        ] {
            assert_eq!(ai_codex_notify(&args), None, "{args:?}");
        }
        // Never the statusLine helper's form either.
        assert_eq!(ai_statusline(&[os("--ai-codex-notify"), os("/r")]), None);
    }

    /// SEC-34: the window process is exactly `--login-window <tool> <account> <url>`.
    #[test]
    fn login_window_flag_form() {
        let url = "https://claude.ai/oauth/authorize?x=1";
        let call = |words: &[&str]| login_window(&words.iter().map(|w| os(w)).collect::<Vec<_>>());
        assert_eq!(
            call(&["--login-window", "claude", "work", url]),
            Some(("claude".into(), "work".into(), url.into()))
        );
        assert_eq!(
            call(&["--login-window", "claude", "default", url]).map(|c| c.1),
            Some("default".into())
        );
        for words in [
            &[][..],
            &["--login-window"],
            &["--login-window", "claude", "work"],
            &["--login-window", "claude", "work", url, "x"],
            &["claude", "--login-window", "work", url],
            &["--login-window", "claude", "../x", url],
            &["--login-window", "claude", "", url],
            &["--login-window", "claude", "a b", url],
            &["--login-window", "claude", "Work", url],
            &["--login-window", "codex", "work", url],
            &["--login-window", "../claude", "work", url],
            &["--ai-statusline", "claude", "work", url],
        ] {
            assert_eq!(call(words), None, "{words:?}");
        }
        // The other helpers never take its form, nor it theirs.
        let args: Vec<_> = ["--login-window", "claude", "work", url].map(os).into();
        assert_eq!(ai_statusline(&args), None);
        assert_eq!(ai_codex_notify(&args), None);
        assert_eq!(call(&["--ai-codex-notify", "/r", "{}"]), None);
    }

    /// D63: claude runs `$BROWSER <url>` as one program, so the launcher is known by its name
    /// and takes exactly one argument; under that name vr-ide never starts as the editor.
    #[test]
    fn login_launcher_name_form() {
        let url = os("https://claude.ai/oauth/authorize?x=1");
        let call = |argv0: &str, args: &[OsString]| login_launcher(argv0.as_ref(), args);
        assert_eq!(
            call(
                "/s t/ai-login/bin/vr-ide-login.claude.work",
                std::slice::from_ref(&url)
            ),
            LauncherCall::Run(LoginLauncher {
                state: PathBuf::from("/s t"),
                tool: "claude".into(),
                account: "work".into(),
                target: url.clone(),
            })
        );
        assert!(matches!(
            call("/s/ai-login/bin/vr-ide-login.claude.default", &[os("/tmp/a.html")]),
            LauncherCall::Run(l) if l.account == "default" && l.target == "/tmp/a.html"
        ));
        for argv0 in [
            "vr-ide",
            "/usr/bin/vr-ide",
            "",
            "/s/ai-login/bin/vr-ide",
            "vr-ide-login",
        ] {
            assert_eq!(
                call(argv0, std::slice::from_ref(&url)),
                LauncherCall::No,
                "{argv0}"
            );
            assert_eq!(call(argv0, &[]), LauncherCall::No, "{argv0}");
        }
        let two = [url.clone(), os("x")];
        for (argv0, args) in [
            ("/s/ai-login/bin/vr-ide-login.claude.work", &[][..]),
            ("/s/ai-login/bin/vr-ide-login.claude.work", &two[..]),
            (
                "/s/ai-login/bin/vr-ide-login.codex.work",
                std::slice::from_ref(&url),
            ),
            (
                "/s/ai-login/bin/vr-ide-login.claude.Work",
                std::slice::from_ref(&url),
            ),
            (
                "/s/ai-login/bin/vr-ide-login.claude.a.b",
                std::slice::from_ref(&url),
            ),
            (
                "/s/ai-login/bin/vr-ide-login.claude.",
                std::slice::from_ref(&url),
            ),
            (
                "/s/ai-login/bin/vr-ide-login.claude",
                std::slice::from_ref(&url),
            ),
            ("/s/ai-login/bin/vr-ide-login.", std::slice::from_ref(&url)),
            (
                "/s/ai-login/vr-ide-login.claude.work",
                std::slice::from_ref(&url),
            ),
            (
                "/s/other/bin/vr-ide-login.claude.work",
                std::slice::from_ref(&url),
            ),
            ("/bin/vr-ide-login.claude.work", std::slice::from_ref(&url)),
            (
                "ai-login/bin/vr-ide-login.claude.work",
                std::slice::from_ref(&url),
            ),
            ("vr-ide-login.claude.work", std::slice::from_ref(&url)),
        ] {
            assert_eq!(call(argv0, args), LauncherCall::Refused, "{argv0} {args:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn cli_reports_permission_error_as_io() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();
        let locked = base.join("locked");
        std::fs::create_dir_all(&locked).unwrap();
        std::fs::write(locked.join("f.txt"), "x").unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        // Root bypasses mode bits, so the scenario cannot be produced there.
        let bypassed = std::fs::metadata(locked.join("f.txt")).is_ok();
        let res = resolve(&[os("locked/f.txt")], &base);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        if bypassed {
            return;
        }
        match res {
            Err(CliError::Io(p, e)) => {
                assert!(p.ends_with("locked/f.txt"));
                assert_eq!(e.kind(), std::io::ErrorKind::PermissionDenied);
            }
            other => panic!("expected CliError::Io, got {other:?}"),
        }
    }
}
