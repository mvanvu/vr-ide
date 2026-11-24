//! Command-line handling: turns the launch arguments into the workspace roots and the files
//! to open.
//!
//! `vr-ide` opens the current folder; `vr-ide dir1 dir2 …` opens each folder as a root;
//! `vr-ide file` opens the file and adds its project (git work tree, else its folder) as a
//! root unless a folder argument already holds it; `vr-ide x.vr-workspace` opens the roots
//! listed in that file (alone: a workspace file is not mixed with other arguments).
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
                "usage: vr-ide [<dir>|<file>]... | vr-ide <file>.{}",
                vr_config::WORKSPACE_EXT
            ),
            CliError::Io(p, e) => write!(f, "cannot access {}: {e}", p.display()),
            CliError::Workspace(p, e) => write!(f, "{}: {e}", p.display()),
        }
    }
}

impl std::error::Error for CliError {}

/// Resolves launch arguments (without argv[0]) against `cwd`.
pub fn resolve(args: &[OsString], cwd: &Path) -> Result<Launch, CliError> {
    if args.is_empty() {
        return Ok(Launch {
            workspace: Workspace::single(canonical(cwd)?),
            open_files: Vec::new(),
            notices: Vec::new(),
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
