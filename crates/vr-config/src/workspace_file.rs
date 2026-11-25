//! `.vr-workspace` files: a TOML list of project folders (roots).
//!
//! ```toml
//! roots = ["api", "web", "/srv/shared"]
//! ```
//!
//! A root inside the file's own folder is written relative to it, so a workspace file kept in
//! a repository keeps working wherever the repository is checked out; any other root is
//! written absolute. Relative entries are read against the file's folder.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ConfigWarning;

/// File name extension of workspace files (without the dot).
pub const WORKSPACE_EXT: &str = "vr-workspace";

/// A workspace file lists folders; anything larger is not one (and is not read whole).
pub const MAX_WORKSPACE_FILE: u64 = 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileFormat {
    roots: Vec<String>,
}

/// True when `path` names a workspace file (by its extension).
pub fn is_workspace_file(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == WORKSPACE_EXT)
}

/// Reads the roots of the workspace file `file` (absolute, `.` and `..` removed lexically,
/// in file order). Missing folders are returned too: the caller shows them as missing.
///
/// Only a regular file of at most [`MAX_WORKSPACE_FILE`] bytes is read: a FIFO or device
/// would block the read (or never end), so it is refused before the open and again on the
/// opened handle. A FIFO swapped in between those two can still block the open, which is
/// why the app calls this off its UI thread.
pub fn read_workspace(file: &Path) -> Result<Vec<PathBuf>, ConfigWarning> {
    use std::io::Read;
    let io = |message: String| ConfigWarning::Io {
        file: file.to_owned(),
        message,
    };
    let not_regular = || io("not a regular file".to_string());
    if !std::fs::metadata(file)
        .map_err(|e| io(e.to_string()))?
        .is_file()
    {
        return Err(not_regular());
    }
    let handle = std::fs::File::open(file).map_err(|e| io(e.to_string()))?;
    if !handle.metadata().map_err(|e| io(e.to_string()))?.is_file() {
        return Err(not_regular());
    }
    let mut bytes = Vec::new();
    handle
        .take(MAX_WORKSPACE_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| io(e.to_string()))?;
    if bytes.len() as u64 > MAX_WORKSPACE_FILE {
        return Err(io(format!(
            "too large for a workspace file (over {} KiB)",
            MAX_WORKSPACE_FILE / 1024
        )));
    }
    let text = String::from_utf8(bytes).map_err(|_| io("not UTF-8 text".to_string()))?;
    parse_workspace(&text, file)
}

/// Parses workspace file text; `file` is where it was read from (relative roots are taken
/// from its folder).
pub fn parse_workspace(text: &str, file: &Path) -> Result<Vec<PathBuf>, ConfigWarning> {
    let parsed: FileFormat = toml::from_str(text).map_err(|e| ConfigWarning::Malformed {
        file: file.to_owned(),
        message: crate::toml_error_message(text, &e),
    })?;
    if parsed.roots.is_empty() {
        return Err(ConfigWarning::InvalidValue {
            file: file.to_owned(),
            key: "roots".to_string(),
            message: "lists no folder".to_string(),
        });
    }
    let base = file.parent().unwrap_or(Path::new("/"));
    let mut roots = Vec::with_capacity(parsed.roots.len());
    for r in parsed.roots {
        if r.is_empty() {
            return Err(ConfigWarning::InvalidValue {
                file: file.to_owned(),
                key: "roots".to_string(),
                message: "an entry is empty".to_string(),
            });
        }
        roots.push(normalize(&base.join(r)));
    }
    Ok(roots)
}

/// The TOML text of a workspace file at `file` listing `roots` (absolute paths).
pub fn format_workspace(file: &Path, roots: &[PathBuf]) -> String {
    let base = file.parent().map(normalize).unwrap_or_default();
    let entries: Vec<String> = roots
        .iter()
        .map(|r| {
            let r = normalize(r);
            match r.strip_prefix(&base) {
                Ok(rel) if !base.as_os_str().is_empty() && rel.as_os_str().is_empty() => {
                    ".".to_string()
                }
                Ok(rel) if !base.as_os_str().is_empty() => rel.to_string_lossy().into_owned(),
                _ => r.to_string_lossy().into_owned(),
            }
        })
        .collect();
    let body = toml::to_string(&FileFormat { roots: entries }).unwrap_or_default();
    format!("# vr-ide workspace: the project folders of this window\n{body}")
}

/// `p` with `.` removed and `..` applied lexically (links are not resolved).
pub(crate) fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push(c);
                }
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_file_round_trips_relative_inside_absolute_outside() {
        let file = Path::new("/home/u/work/team.vr-workspace");
        let roots = vec![
            PathBuf::from("/home/u/work/api"),
            PathBuf::from("/home/u/work"),
            PathBuf::from("/srv/shared lib"),
            PathBuf::from("/home/u/work/web/app"),
        ];
        let text = format_workspace(file, &roots);
        assert!(text.contains("\"api\""), "{text}");
        assert!(text.contains("\".\""), "{text}");
        assert!(text.contains("\"/srv/shared lib\""), "{text}");
        assert!(text.contains("\"web/app\""), "{text}");
        assert_eq!(parse_workspace(&text, file).unwrap(), roots);
        // Moved with its folder, the relative roots follow.
        let moved = Path::new("/mnt/copy/team.vr-workspace");
        assert_eq!(
            parse_workspace(&text, moved).unwrap(),
            vec![
                PathBuf::from("/mnt/copy/api"),
                PathBuf::from("/mnt/copy"),
                PathBuf::from("/srv/shared lib"),
                PathBuf::from("/mnt/copy/web/app"),
            ]
        );
    }

    #[test]
    fn workspace_file_reads_dot_dot_and_refuses_bad_files() {
        let file = Path::new("/w/x/ws.vr-workspace");
        assert_eq!(
            parse_workspace("roots = [\"../y\", \"./z/.\"]", file).unwrap(),
            vec![PathBuf::from("/w/y"), PathBuf::from("/w/x/z")]
        );
        assert!(matches!(
            parse_workspace("roots = [", file),
            Err(ConfigWarning::Malformed { .. })
        ));
        assert!(matches!(
            parse_workspace("roots = []", file),
            Err(ConfigWarning::InvalidValue { .. })
        ));
        assert!(matches!(
            parse_workspace("roots = [\"\"]", file),
            Err(ConfigWarning::InvalidValue { .. })
        ));
        assert!(matches!(
            parse_workspace("folders = [\"a\"]", file),
            Err(ConfigWarning::Malformed { .. })
        ));
        assert!(is_workspace_file(Path::new("a/b.vr-workspace")));
        assert!(!is_workspace_file(Path::new("a/b.toml")));
    }

    #[test]
    fn read_workspace_reports_a_missing_file() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("none.vr-workspace");
        assert!(matches!(read_workspace(&f), Err(ConfigWarning::Io { .. })));
        std::fs::write(&f, "roots = [\"a\"]\n").unwrap();
        assert_eq!(read_workspace(&f).unwrap(), vec![d.path().join("a")]);
    }

    #[test]
    fn read_workspace_refuses_a_fifo_a_folder_and_a_huge_file() {
        let d = tempfile::tempdir().unwrap();
        let dir = d.path().join("dir.vr-workspace");
        std::fs::create_dir(&dir).unwrap();
        let e = read_workspace(&dir).unwrap_err().to_string();
        assert!(e.contains("not a regular file"), "{e}");
        let big = d.path().join("big.vr-workspace");
        let mut text = String::from("roots = [\"a\"]\n");
        text.push_str(&"#".repeat(MAX_WORKSPACE_FILE as usize));
        std::fs::write(&big, text).unwrap();
        let e = read_workspace(&big).unwrap_err().to_string();
        assert!(e.contains("too large"), "{e}");
        let fifo = d.path().join("f.vr-workspace");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .is_ok_and(|s| s.success());
        if made {
            let e = read_workspace(&fifo).unwrap_err().to_string();
            assert!(e.contains("not a regular file"), "{e}");
        }
    }
}
