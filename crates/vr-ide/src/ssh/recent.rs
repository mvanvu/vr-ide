//! Recently used ssh targets (AC-S1, SEC-S10): `<state dir>/ssh-recent`, one validated target
//! per line, most recent first. The file is a host inventory, so it is private (0600 in a 0700
//! folder) and replaced atomically; what is read back is validated again, since another
//! program of the same user may have written it.

use std::io::Write;
use std::path::Path;

use super::Target;
use crate::recovery::{create_private_dir, create_private_file};

/// Targets kept at most.
pub const MAX: usize = 20;
/// A larger file is not vr-ide's and is ignored.
const MAX_BYTES: u64 = 64 * 1024;
/// File name under the state dir.
pub const FILE: &str = "ssh-recent";

/// The valid targets of `path`, each once, at most [`MAX`]; empty when the file is missing,
/// not a regular file (a link is not followed) or too large.
pub fn load(path: &Path) -> Vec<Target> {
    let regular = std::fs::symlink_metadata(path)
        .is_ok_and(|m| m.file_type().is_file() && m.len() <= MAX_BYTES);
    if !regular {
        return Vec::new();
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut list: Vec<Target> = Vec::new();
    for target in text.lines().filter_map(Target::parse) {
        if list.len() >= MAX {
            break;
        }
        if !list.contains(&target) {
            list.push(target);
        }
    }
    list
}

/// Puts `target` first in the list at `path` and returns the new list. The new file is written
/// beside the old one and renamed over it, so a link at `path` is replaced, never written
/// through.
pub fn record(path: &Path, target: &Target) -> std::io::Result<Vec<Target>> {
    let mut list = load(path);
    list.retain(|t| t != target);
    list.insert(0, target.clone());
    list.truncate(MAX);
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("the recent file has no folder"))?;
    create_private_dir(dir)?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    // Left by a run that died between the write and the rename.
    let _ = std::fs::remove_file(&tmp);
    let written = (|| {
        let mut f = create_private_file(&tmp)?;
        for t in &list {
            writeln!(f, "{t}")?;
        }
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written.map(|()| list)
}
