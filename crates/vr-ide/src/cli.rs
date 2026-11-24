//! Command-line handling: turns the launch arguments into a project root and an optional file.
use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub struct Launch {
    pub root: PathBuf,
    pub open_file: Option<PathBuf>,
}

#[derive(Debug)]
pub enum CliError {
    NotFound(PathBuf),
    TooManyArgs,
    Io(PathBuf, std::io::Error),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::NotFound(p) => write!(f, "path does not exist: {}", p.display()),
            CliError::TooManyArgs => write!(f, "usage: vr-ide [<dir>|<file>]"),
            CliError::Io(p, e) => write!(f, "cannot access {}: {e}", p.display()),
        }
    }
}

impl std::error::Error for CliError {}

/// Resolves launch arguments (without argv[0]) against `cwd`.
pub fn resolve(args: &[OsString], cwd: &Path) -> Result<Launch, CliError> {
    let target = match args {
        [] => {
            return Ok(Launch {
                root: canonical(cwd)?,
                open_file: None,
            });
        }
        [one] => cwd.join(one),
        _ => return Err(CliError::TooManyArgs),
    };
    match std::fs::metadata(&target) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(CliError::NotFound(target));
        }
        Err(e) => return Err(CliError::Io(target, e)),
    }
    let target = canonical(&target)?;
    if target.is_dir() {
        return Ok(Launch {
            root: target,
            open_file: None,
        });
    }
    // A file's project is its enclosing git work tree; without one, the file's own directory.
    let parent = target
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| target.clone());
    let root = parent
        .ancestors()
        .find(|dir| dir.join(".git").exists())
        .map(Path::to_path_buf)
        .unwrap_or(parent);
    Ok(Launch {
        root,
        open_file: Some(target),
    })
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

    #[test]
    fn cli_resolves_root() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().canonicalize().unwrap();

        // no arg -> cwd
        let l = resolve(&[], &base).unwrap();
        assert_eq!(
            l,
            Launch {
                root: base.clone(),
                open_file: None
            }
        );

        // dir arg -> canonical dir
        std::fs::create_dir_all(base.join("a/b")).unwrap();
        let l = resolve(&[os("a/../a/b")], &base).unwrap();
        assert_eq!(l.root, base.join("a/b"));
        assert_eq!(l.open_file, None);

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
        assert_eq!(l.root, repo);
        assert_eq!(l.open_file, Some(repo.join("src/x.rs")));

        // file outside git -> parent dir
        std::fs::write(base.join("a/note.txt"), "hi").unwrap();
        let l = resolve(&[os("a/note.txt")], &base).unwrap();
        assert_eq!(l.root, base.join("a"));
        assert_eq!(l.open_file, Some(base.join("a/note.txt")));

        // nonexistent -> Err with a message naming the path
        let err = resolve(&[os("missing.txt")], &base).unwrap_err();
        assert!(err.to_string().contains("missing.txt"));

        // more than one argument -> usage error
        assert!(resolve(&[os("a"), os("b")], &base).is_err());
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
