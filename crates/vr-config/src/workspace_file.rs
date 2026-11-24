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
pub fn read_workspace(file: &Path) -> Result<Vec<PathBuf>, ConfigWarning> {
    let text = std::fs::read_to_string(file).map_err(|e| ConfigWarning::Io {
        file: file.to_owned(),
        message: e.to_string(),
    })?;
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
}
