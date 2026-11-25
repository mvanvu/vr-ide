//! Small parsing helpers shared by the query modules.

use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;

use crate::GitError;

pub(crate) fn path_from(bytes: &[u8]) -> PathBuf {
    PathBuf::from(OsString::from_vec(bytes.to_vec()))
}

pub(crate) fn os(s: &str) -> &OsStr {
    OsStr::new(s)
}

pub(crate) fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

pub(crate) fn parse_err(cmd: &str, what: impl Into<String>) -> GitError {
    GitError::Parse {
        cmd: cmd.to_string(),
        what: what.into(),
    }
}

/// A full or abbreviated object id as printed by git.
pub(crate) fn is_oid(s: &str) -> bool {
    (4..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// Undo git's C-style quoting of a path (`"a\tb\303\251"`); an unquoted name is returned as is.
pub(crate) fn unquote(s: &[u8]) -> Vec<u8> {
    if s.len() < 2 || s[0] != b'"' || s[s.len() - 1] != b'"' {
        return s.to_vec();
    }
    let s = &s[1..s.len() - 1];
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] != b'\\' || i + 1 >= s.len() {
            out.push(s[i]);
            i += 1;
            continue;
        }
        let c = s[i + 1];
        i += 2;
        match c {
            b'a' => out.push(7),
            b'b' => out.push(8),
            b't' => out.push(b'\t'),
            b'n' => out.push(b'\n'),
            b'v' => out.push(11),
            b'f' => out.push(12),
            b'r' => out.push(b'\r'),
            b'0'..=b'3' if i + 2 <= s.len() => {
                let oct = [c, s[i], s[i + 1]];
                if oct.iter().all(|d| (b'0'..=b'7').contains(d)) {
                    out.push((oct[0] - b'0') * 64 + (oct[1] - b'0') * 8 + (oct[2] - b'0'));
                    i += 2;
                } else {
                    out.push(c);
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Split `-z` output into its NUL-terminated fields.
pub(crate) fn fields(out: &[u8]) -> Vec<&[u8]> {
    let mut v: Vec<&[u8]> = out.split(|&b| b == 0).collect();
    if v.last().is_some_and(|l| l.is_empty()) {
        v.pop();
    }
    v
}

pub(crate) fn as_bytes(p: &std::path::Path) -> &[u8] {
    p.as_os_str().as_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unquote_handles_escapes_and_octal() {
        assert_eq!(unquote(b"plain name"), b"plain name");
        assert_eq!(unquote(br#""a\tb\nc\"d\\e""#), b"a\tb\nc\"d\\e");
        assert_eq!(unquote(br#""caf\303\251""#), "café".as_bytes());
        assert_eq!(unquote(br#""\377x""#), b"\xffx");
    }

    #[test]
    fn oid_shape() {
        assert!(is_oid("abcd"));
        assert!(is_oid(&"a".repeat(40)));
        assert!(!is_oid("abc"));
        assert!(!is_oid("--output=x"));
        assert!(!is_oid("ABCD"));
        assert!(!is_oid(&"a".repeat(65)));
    }
}
