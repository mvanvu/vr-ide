//! The `Host` aliases of the user's ssh configuration, for the list of the `ssh` command
//! (AC-S1). A bounded reader (SEC-S11): the file is whatever dotfiles repos and tools wrote, so
//! every size and count has a cap, only regular files are read, `Match` is never evaluated and
//! nothing is run. ssh itself, not this list, decides what a chosen alias means.

use std::collections::HashSet;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use super::Target;

/// Aliases listed at most.
pub const MAX_ALIASES: usize = 1000;
/// `Include` nesting followed at most.
pub const MAX_DEPTH: usize = 16;
/// Files read at most, the main file included.
pub const MAX_FILES: usize = 64;
/// Files one `Include` pattern may add.
pub const MAX_GLOB_MATCHES: usize = 256;
/// A larger file is skipped.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// Folder entries one `Include` pattern may look at while matching.
const MAX_GLOB_ENTRIES: usize = 10_000;

/// The concrete aliases of `<home>/.ssh/config` and the files it includes, in file order,
/// each once. Patterns (`*`, `?`, `!`), quoted names and names that are no valid target are
/// left out.
pub fn hosts(home: &Path) -> Vec<String> {
    let dir = home.join(".ssh");
    let mut reader = Reader {
        home,
        dir: &dir,
        seen: HashSet::new(),
        files: 0,
        hosts: Vec::new(),
        listed: HashSet::new(),
    };
    reader.read(&dir.join("config"), 0);
    reader.hosts
}

struct Reader<'a> {
    home: &'a Path,
    dir: &'a Path,
    /// Canonical paths already read: each file once, so Include loops end.
    seen: HashSet<PathBuf>,
    files: usize,
    hosts: Vec<String>,
    listed: HashSet<String>,
}

impl Reader<'_> {
    fn full(&self) -> bool {
        self.hosts.len() >= MAX_ALIASES
    }

    fn read(&mut self, path: &Path, depth: usize) {
        if depth > MAX_DEPTH || self.files >= MAX_FILES || self.full() {
            return;
        }
        let Some(text) = read_regular(path) else {
            return;
        };
        let Ok(canonical) = path.canonicalize() else {
            return;
        };
        if !self.seen.insert(canonical) {
            return;
        }
        self.files += 1;
        for line in text.lines() {
            if self.full() {
                return;
            }
            let (keyword, args) = split_keyword(line);
            if keyword.eq_ignore_ascii_case("host") {
                for (token, quoted) in tokens(args) {
                    self.add(&token, quoted);
                }
            } else if keyword.eq_ignore_ascii_case("include") {
                for (pattern, _) in tokens(args) {
                    for file in self.expand(&pattern) {
                        self.read(&file, depth + 1);
                    }
                }
            }
        }
    }

    fn add(&mut self, token: &str, quoted: bool) {
        if quoted || self.full() {
            return;
        }
        // The validator's host characters hold no pattern character, so `*`, `?` and `!`
        // tokens fall out here with everything else that is no plain name.
        if Target::parse(token).is_some_and(|t| t.is_alias()) && self.listed.insert(token.into()) {
            self.hosts.push(token.to_string());
        }
    }

    /// The files an `Include` argument names: `~` is the home folder, a relative path starts
    /// at `~/.ssh`, `*` and `?` match within one path component. Arguments with ssh tokens or
    /// variables are not understood and name nothing.
    fn expand(&self, pattern: &str) -> Vec<PathBuf> {
        if pattern.contains(['%', '$']) {
            return Vec::new();
        }
        let path = if pattern == "~" {
            self.home.to_path_buf()
        } else if let Some(rest) = pattern.strip_prefix("~/") {
            self.home.join(rest)
        } else if Path::new(pattern).is_absolute() {
            PathBuf::from(pattern)
        } else {
            self.dir.join(pattern)
        };
        let mut budget = MAX_GLOB_ENTRIES;
        let mut found = vec![PathBuf::new()];
        for part in path.components() {
            let name = part.as_os_str().to_string_lossy();
            if !name.contains(['*', '?']) {
                for f in &mut found {
                    f.push(part.as_os_str());
                }
                continue;
            }
            let mut next = Vec::new();
            for base in &found {
                let Ok(entries) = std::fs::read_dir(base) else {
                    continue;
                };
                for entry in entries.flatten() {
                    if budget == 0 {
                        break;
                    }
                    budget -= 1;
                    let entry_name = entry.file_name();
                    let entry_name = entry_name.to_string_lossy();
                    // As in a shell, a wildcard does not match a leading dot.
                    let hidden = entry_name.starts_with('.') && !name.starts_with('.');
                    if !hidden && wild_match(name.as_bytes(), entry_name.as_bytes()) {
                        next.push(base.join(entry.file_name()));
                    }
                }
            }
            // ssh reads matches in name order.
            next.sort();
            next.truncate(MAX_GLOB_MATCHES);
            found = next;
        }
        found.truncate(MAX_GLOB_MATCHES);
        found
    }
}

/// The text of a regular file of at most [`MAX_FILE_BYTES`]; `None` for anything else. The
/// metadata is checked before the open (a FIFO or a device is never opened), and again on the
/// open file, which is opened without blocking in case it was swapped in between.
fn read_regular(path: &Path) -> Option<String> {
    let fits = |m: &std::fs::Metadata| m.is_file() && m.len() <= MAX_FILE_BYTES;
    if !std::fs::metadata(path).is_ok_and(|m| fits(&m)) {
        return None;
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags((rustix::fs::OFlags::NONBLOCK | rustix::fs::OFlags::NOCTTY).bits() as i32)
        .open(path)
        .ok()?;
    if !file.metadata().is_ok_and(|m| fits(&m)) {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= MAX_FILE_BYTES).then(|| String::from_utf8_lossy(&bytes).into_owned())
}

/// A config line as (keyword, arguments): `Keyword args`, `Keyword=args`, `Keyword = args`.
/// Comments and blank lines give an empty keyword.
fn split_keyword(line: &str) -> (&str, &str) {
    let line = line.trim();
    if line.starts_with('#') {
        return ("", "");
    }
    let end = line
        .find(|c: char| c.is_whitespace() || c == '=')
        .unwrap_or(line.len());
    let rest = line[end..].trim_start();
    let rest = rest.strip_prefix('=').unwrap_or(rest).trim_start();
    (&line[..end], rest)
}

/// Whitespace-separated arguments; a double-quoted one may hold spaces. Each with whether it
/// was quoted.
fn tokens(args: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut rest = args.trim_start();
    while !rest.is_empty() {
        if let Some(quoted) = rest.strip_prefix('"') {
            let end = quoted.find('"').unwrap_or(quoted.len());
            out.push((quoted[..end].to_string(), true));
            rest = quoted.get(end + 1..).unwrap_or("").trim_start();
        } else if rest.starts_with('#') {
            // OpenSSH 8.7+ reads the rest of the line as a comment.
            break;
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            out.push((rest[..end].to_string(), false));
            rest = rest[end..].trim_start();
        }
    }
    out
}

/// `*` (any run) and `?` (one byte) within a file name.
fn wild_match(pattern: &[u8], name: &[u8]) -> bool {
    let (mut p, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == name[n]) {
            p += 1;
            n += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some((p, n));
            p += 1;
        } else if let Some((sp, sn)) = star {
            p = sp + 1;
            n = sn + 1;
            star = Some((sp, sn + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&b| b == b'*')
}
