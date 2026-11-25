//! Single-hunk patches for staging, unstaging and discarding a hunk or some of its lines
//! (threat model SEC-10). The file header is git's own (copied from the diff output, never
//! re-quoted); only the hunk is rebuilt, the way `git add -p` / `git gui` do it.

use crate::diff::{Hunk, LineKind};

/// How the patch is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Direction {
    /// `git apply --cached`: the index (old side) gets the selected changes.
    Forward,
    /// `git apply -R` (index or worktree): the new side loses the selected changes.
    Reverse,
}

/// The file header of a one-file diff: everything before its first `@@` line.
pub(crate) fn file_header(raw: &[u8]) -> Option<&[u8]> {
    if !raw.starts_with(b"diff --git ") {
        return None;
    }
    // A second file in the output would make the header ambiguous.
    if raw.windows(12).skip(1).any(|w| w == b"\ndiff --git ") {
        return None;
    }
    let at = raw.windows(4).position(|w| w == b"\n@@ ")?;
    Some(&raw[..at + 1])
}

/// A patch with `header` and the part of `hunk` the selection keeps. `selected[i]` says
/// whether line `i` of the hunk (a `+` or `-` line) is applied; context lines ignore it.
///
/// Forward: an unselected `-` stays as context, an unselected `+` is left out. Reverse (the
/// patch is applied with `-R`, so its new side is what exists): an unselected `+` stays as
/// context, an unselected `-` is left out. Errors name a selection git could not apply.
pub(crate) fn build(
    header: &[u8],
    hunk: &Hunk,
    selected: &[bool],
    dir: Direction,
) -> Result<Vec<u8>, String> {
    if selected.len() != hunk.lines.len() {
        return Err("selection does not match the hunk".into());
    }
    let mut out_lines: Vec<(u8, &[u8], bool)> = Vec::with_capacity(hunk.lines.len());
    let mut changes = 0usize;
    for (l, &sel) in hunk.lines.iter().zip(selected) {
        let sign = match (l.kind, dir, sel) {
            (LineKind::Context, _, _) => Some(b' '),
            (LineKind::Added, _, true) => Some(b'+'),
            (LineKind::Removed, _, true) => Some(b'-'),
            (LineKind::Added, Direction::Forward, false) => None,
            (LineKind::Added, Direction::Reverse, false) => Some(b' '),
            (LineKind::Removed, Direction::Forward, false) => Some(b' '),
            (LineKind::Removed, Direction::Reverse, false) => None,
        };
        if let Some(s) = sign {
            if s != b' ' {
                changes += 1;
            }
            out_lines.push((s, &l.text, l.no_newline));
        }
    }
    if changes == 0 {
        return Err("no changed line selected".into());
    }
    // "\ No newline at end of file" may only follow the last line of each side.
    for side in *b"-+" {
        let side_lines: Vec<bool> = out_lines
            .iter()
            .filter(|(s, _, _)| *s == b' ' || *s == side)
            .map(|&(_, _, no_nl)| no_nl)
            .collect();
        if let Some(pos) = side_lines.iter().position(|&n| n)
            && pos + 1 != side_lines.len()
        {
            return Err(
                "the selection adds lines after the file's last line without a newline; \
                 select the change of that last line too"
                    .into(),
            );
        }
    }
    let count = |side: u8| {
        out_lines
            .iter()
            .filter(|(s, _, _)| *s == b' ' || *s == side)
            .count() as u32
    };
    let (old_len, new_len) = (count(b'-'), count(b'+'));
    // In unified diffs a side of length 0 names the line before the change.
    let first = |start: u32, len: u32| if len == 0 { start + 1 } else { start };
    let at = |pos: u32, len: u32| if len == 0 { pos - 1 } else { pos };
    let (old_start, new_start) = match dir {
        Direction::Forward => {
            let pos = first(hunk.old_start, hunk.old_len);
            (hunk.old_start, at(pos, new_len))
        }
        Direction::Reverse => {
            let pos = first(hunk.new_start, hunk.new_len);
            (at(pos, old_len), hunk.new_start)
        }
    };
    let mut patch = header.to_vec();
    patch.extend_from_slice(
        format!("@@ -{old_start},{old_len} +{new_start},{new_len} @@\n").as_bytes(),
    );
    for (sign, text, no_nl) in out_lines {
        patch.push(sign);
        patch.extend_from_slice(text);
        patch.push(b'\n');
        if no_nl {
            patch.extend_from_slice(b"\\ No newline at end of file\n");
        }
    }
    Ok(patch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::parse_diff;

    const RAW: &[u8] = b"diff --git a/f b/f\nindex 1111111111111111111111111111111111111111..2222222222222222222222222222222222222222 100644\n--- a/f\n+++ b/f\n@@ -2,3 +2,3 @@ ctx\n a\n-b\n+B\n c\n";

    fn hunk() -> Hunk {
        parse_diff(RAW).unwrap().remove(0).hunks.remove(0)
    }

    #[test]
    fn header_is_copied_up_to_the_first_hunk() {
        let h = file_header(RAW).unwrap();
        assert!(h.ends_with(b"+++ b/f\n"));
        assert!(file_header(b"diff --git a/x b/x\n").is_none());
        let two = [RAW, RAW].concat();
        assert!(file_header(&two).is_none());
    }

    #[test]
    fn whole_hunk_equals_git_hunk_body() {
        let p = build(b"H\n", &hunk(), &[true; 4], Direction::Forward).unwrap();
        assert_eq!(p, b"H\n@@ -2,3 +2,3 @@\n a\n-b\n+B\n c\n");
    }

    #[test]
    fn forward_keeps_unselected_removal_as_context() {
        let p = build(
            b"",
            &hunk(),
            &[false, false, true, false],
            Direction::Forward,
        )
        .unwrap();
        assert_eq!(p, b"@@ -2,3 +2,4 @@\n a\n b\n+B\n c\n");
    }

    #[test]
    fn reverse_keeps_unselected_addition_as_context() {
        let p = build(
            b"",
            &hunk(),
            &[false, true, false, false],
            Direction::Reverse,
        )
        .unwrap();
        assert_eq!(p, b"@@ -2,4 +2,3 @@\n a\n-b\n B\n c\n");
    }

    #[test]
    fn empty_selection_and_lines_after_a_missing_newline_are_refused() {
        assert!(build(b"", &hunk(), &[false; 4], Direction::Forward).is_err());
        let raw = b"diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1 +1,2 @@\n-a\n\\ No newline at end of file\n+a\n+b\n";
        let h = parse_diff(raw).unwrap().remove(0).hunks.remove(0);
        // Adding "b" while "a" keeps no newline cannot be expressed.
        assert!(build(b"", &h, &[false, false, true], Direction::Forward).is_err());
        let p = build(b"", &h, &[true, true, false], Direction::Forward).unwrap();
        assert_eq!(
            p,
            b"@@ -1,1 +1,1 @@\n-a\n\\ No newline at end of file\n+a\n"
        );
    }

    #[test]
    fn pure_insertion_positions() {
        let raw = b"diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -3,0 +4,2 @@\n+x\n+y\n";
        let h = parse_diff(raw).unwrap().remove(0).hunks.remove(0);
        let p = build(b"", &h, &[false, true], Direction::Forward).unwrap();
        assert_eq!(p, b"@@ -3,0 +4,1 @@\n+y\n");
        let p = build(b"", &h, &[true, false], Direction::Reverse).unwrap();
        assert_eq!(p, b"@@ -4,1 +4,2 @@\n+x\n y\n");
    }
}
