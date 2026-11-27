//! Recently used ssh argument lines (AC-S1, AC-6, SEC-S10, SEC-5): `<state dir>/ssh-recent`,
//! one entry per line, most recent first: `<absolute folder>\t<argument line>`, the folder
//! being where ssh ran (its relative paths resolve there); an empty folder marks an old entry
//! that never had one. A line without a tab is an old bare target and is read as one
//! (`bob@h:2222` becomes `-p 2222 bob@h`).
//!
//! The file is a host inventory, so it is private (0600 in a 0700 folder) and replaced
//! atomically; what is read back is checked again (SEC-1, SEC-2), since another program of
//! the same user may have written it, and a file that others could have written is ignored.

use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use super::args::Line;
use crate::recovery::{create_private_dir, create_private_file};

/// Entries kept at most.
pub const MAX: usize = 20;
/// A larger file is not vr-ide's and is ignored; a list is cut to fit it.
const MAX_BYTES: u64 = 64 * 1024;
/// File name under the state dir.
pub const FILE: &str = "ssh-recent";

/// One recent entry: a checked argument line and the folder it ran in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The argument line ([`Line::text`]): it splits and has a valid destination.
    pub text: String,
    /// Absolute; `None` for an old entry that was a bare target (it has no relative path).
    pub base: Option<PathBuf>,
}

impl Entry {
    /// `None` when `text` is not a valid line or `base` is not absolute.
    pub fn new(text: &str, base: Option<PathBuf>) -> Option<Entry> {
        let line = Line::parse(text, None).ok()?;
        if base.as_ref().is_some_and(|b| !b.is_absolute()) {
            return None;
        }
        Some(Entry {
            text: line.text().to_string(),
            base,
        })
    }

    /// The entry as one line of the file; `None` when the folder cannot be written on one
    /// line (a tab or a line break in its name, or not UTF-8).
    fn to_record(&self) -> Option<String> {
        match &self.base {
            None => Some(format!("\t{}", self.text)),
            Some(b) => {
                let b = b.to_str()?;
                (!b.chars().any(char::is_control)).then(|| format!("{b}\t{}", self.text))
            }
        }
    }

    fn from_record(record: &str) -> Option<Entry> {
        match record.split_once('\t') {
            Some(("", text)) => Entry::new(text, None),
            Some((base, text)) => Entry::new(text, Some(PathBuf::from(base))),
            // Without a folder only an old bare target is vr-ide's.
            None => super::Target::parse(record).and_then(|_| Entry::new(record, None)),
        }
    }
}

/// The valid entries of `path`, each line once, at most [`MAX`]; empty when the file is
/// missing, not a regular file (a link is not followed), too large, not the user's own or
/// writable by group or others.
pub fn load(path: &Path) -> Vec<Entry> {
    let ok = std::fs::symlink_metadata(path).is_ok_and(|m| {
        m.file_type().is_file()
            && m.len() <= MAX_BYTES
            && m.uid() == rustix::process::getuid().as_raw()
            && m.mode() & 0o022 == 0
    });
    if !ok {
        return Vec::new();
    }
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut list: Vec<Entry> = Vec::new();
    for entry in text.lines().filter_map(Entry::from_record) {
        if list.len() >= MAX {
            break;
        }
        if !list.iter().any(|e| e.text == entry.text) {
            list.push(entry);
        }
    }
    list
}

/// Puts `entry` first in the list at `path` (an older entry with the same line goes) and
/// returns the new list. The new file is written beside the old one and renamed over it, so
/// a link at `path` is replaced, never written through. An entry whose folder cannot be
/// written on one line is not recorded.
pub fn record(path: &Path, entry: &Entry) -> std::io::Result<Vec<Entry>> {
    let mut list = load(path);
    if entry.to_record().is_none() {
        return Ok(list);
    }
    list.retain(|e| e.text != entry.text);
    list.insert(0, entry.clone());
    list.truncate(MAX);
    let mut records: Vec<String> = list.iter().filter_map(Entry::to_record).collect();
    // Long lines could make the file larger than a load accepts: the oldest go first.
    while records.iter().map(|r| r.len() as u64 + 1).sum::<u64>() > MAX_BYTES {
        records.pop();
        list.pop();
    }
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
        for r in &records {
            writeln!(f, "{r}")?;
        }
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    written.map(|()| list)
}
