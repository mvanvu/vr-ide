//! In-buffer find and replace-all, plus the line-scoped matcher shared with project search.

use std::borrow::Cow;

use regex::{Regex, RegexBuilder};
use ropey::Rope;
use vr_core::{Change, Transaction};

use crate::{Query, QueryError};

/// A compiled query. It runs on one line at a time (without its line break), so a match
/// never spans lines and `^`/`$` mean line start/end.
#[derive(Debug, Clone)]
pub(crate) struct Matcher {
    re: Regex,
    expand_groups: bool,
}

impl Matcher {
    /// `None` for an empty pattern, which matches nothing.
    pub(crate) fn new(q: &Query) -> Result<Option<Self>, QueryError> {
        if q.pattern.is_empty() {
            return Ok(None);
        }
        let build = |source: &str| {
            RegexBuilder::new(source)
                .case_insensitive(!q.case_sensitive)
                .build()
                .map_err(|e| QueryError::InvalidRegex(e.to_string()))
        };
        let source = regex_source(q);
        let re = if q.whole_word {
            // Compiled alone first: wrapping could turn an unbalanced pattern like `a)(b`
            // into a valid one.
            build(&source)?;
            // Half boundaries: no word char (Unicode `\w`, marks included) right before the
            // match and none right after it. Inside the regex, so an alternative that fails
            // the boundary lets a longer one at the same start win.
            match build(&format!(r"\b{{start-half}}(?:{source})\b{{end-half}}")) {
                Ok(re) => re,
                // A valid pattern only fails here when it ends inside an `x`-mode comment
                // that swallowed the closing `)`. A newline ends the comment and, in `x`
                // mode, is otherwise ignored, so the pattern's meaning is unchanged.
                Err(first) => build(&format!("\\b{{start-half}}(?:{source}\n)\\b{{end-half}}"))
                    .map_err(|_| first)?,
            }
        } else {
            build(&source)?
        };
        Ok(Some(Self {
            re,
            expand_groups: q.regex,
        }))
    }

    /// Calls `f(start, end)` with the byte range of each match in `line`, left to right.
    ///
    /// Hand-rolled instead of `find_iter` so every round provably makes progress: an empty
    /// match advances by one char and is not reported where the previous match ended.
    pub(crate) fn each_match(&self, line: &str, mut f: impl FnMut(usize, usize)) {
        let mut pos = 0;
        let mut last_end = None;
        while pos <= line.len() {
            let Some(m) = self.re.find_at(line, pos) else {
                break;
            };
            let (start, end) = (m.start(), m.end());
            if start == end && last_end == Some(start) {
                match next_char(line, start) {
                    Some(next) => pos = next,
                    None => break,
                }
                continue;
            }
            f(start, end);
            last_end = Some(end);
            pos = if start == end {
                match next_char(line, end) {
                    Some(next) => next,
                    None => break,
                }
            } else {
                end
            };
        }
    }
}

impl Matcher {
    /// Appends the replacement for the match that `each_match` reported at `start` in `line`.
    /// Regex mode expands `$1`/`${name}`/`$0` (`$1_x` is group 1 then `_x`, see
    /// `numbered_groups_braced`); literal mode copies `replacement` verbatim.
    pub(crate) fn expand(&self, line: &str, start: usize, replacement: &str, out: &mut String) {
        if !self.expand_groups {
            out.push_str(replacement);
            return;
        }
        let replacement = numbered_groups_braced(replacement);
        // Leftmost-first matching from `start` picks the same match `find_at` reported there.
        match self.re.captures_at(line, start) {
            Some(caps) => caps.expand(&replacement, out),
            None => out.push_str(&replacement),
        }
    }
}

/// Rewrites `$12` to `${12}` so digits followed by a word char still name a numbered group
/// (`$1_old` → group 1 + `_old`, as in VS Code); the regex crate would read `1_old` as a
/// group name. `$$` stays an escaped dollar, `${..}` and `$name` are untouched.
fn numbered_groups_braced(replacement: &str) -> Cow<'_, str> {
    if !replacement.contains('$') {
        return Cow::Borrowed(replacement);
    }
    let mut out = String::with_capacity(replacement.len() + 4);
    let mut chars = replacement.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('$') => {
                chars.next();
                out.push_str("$$");
            }
            Some(d) if d.is_ascii_digit() => {
                out.push_str("${");
                while let Some(d) = chars.next_if(char::is_ascii_digit) {
                    out.push(d);
                }
                out.push('}');
            }
            _ => out.push('$'),
        }
    }
    Cow::Owned(out)
}

/// The byte offset after the char at `i`, or `None` at the end of `s`.
fn next_char(s: &str, i: usize) -> Option<usize> {
    s[i..].chars().next().map(|c| i + c.len_utf8())
}

/// The regex source for `q`: the pattern itself in regex mode, escaped in literal mode.
pub(crate) fn regex_source(q: &Query) -> String {
    if q.regex {
        q.pattern.clone()
    } else {
        regex::escape(&q.pattern)
    }
}

/// Byte ranges `(start, content_end)` of each line of `s`, without the line break. Breaks are
/// `\n`, `\r\n` and a lone `\r` (D31); no line follows a final line break.
pub(crate) fn line_spans(s: &str) -> impl Iterator<Item = (usize, usize)> + '_ {
    let bytes = s.as_bytes();
    let mut start = 0;
    std::iter::from_fn(move || {
        if start >= bytes.len() {
            return None;
        }
        let line_start = start;
        match bytes[start..]
            .iter()
            .position(|&b| b == b'\n' || b == b'\r')
        {
            Some(i) => {
                let end = start + i;
                let crlf = bytes[end] == b'\r' && bytes.get(end + 1) == Some(&b'\n');
                start = end + if crlf { 2 } else { 1 };
                Some((line_start, end))
            }
            None => {
                start = bytes.len();
                Some((line_start, bytes.len()))
            }
        }
    })
}

/// Byte ranges of every match in `s`, line by line.
fn byte_matches(s: &str, m: &Matcher) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (start, end) in line_spans(s) {
        m.each_match(&s[start..end], |a, b| out.push((start + a, start + b)));
    }
    out
}

/// Char ranges of every match, in order. An empty pattern finds nothing.
pub fn find_all(text: &Rope, q: &Query) -> Result<Vec<(usize, usize)>, QueryError> {
    let Some(m) = Matcher::new(q)? else {
        return Ok(Vec::new());
    };
    // One contiguous copy lets the regex engine scan without chunk seams.
    let s = text.to_string();
    Ok(byte_matches(&s, &m)
        .into_iter()
        .map(|(a, b)| (text.byte_to_char(a), text.byte_to_char(b)))
        .collect())
}

/// One transaction replacing every match, so "replace all" is one undo step.
pub fn replace_all_tx(
    text: &Rope,
    q: &Query,
    replacement: &str,
) -> Result<Transaction, QueryError> {
    let Some(m) = Matcher::new(q)? else {
        return Ok(Transaction::default());
    };
    let s = text.to_string();
    let mut changes = Vec::new();
    for (start, end) in line_spans(&s) {
        let line = &s[start..end];
        m.each_match(line, |a, b| {
            let mut insert = String::new();
            m.expand(line, a, replacement, &mut insert);
            changes.push(Change::new(
                text.byte_to_char(start + a),
                text.byte_to_char(start + b),
                insert,
            ));
        });
    }
    Ok(Transaction::new(changes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use vr_core::Buffer;
    use vr_core::transaction::apply_changes;

    fn applied(rope: &Rope, query: &Query, replacement: &str) -> String {
        let tx = replace_all_tx(rope, query, replacement).unwrap();
        let mut out = rope.clone();
        apply_changes(&mut out, &tx.changes);
        out.to_string()
    }

    fn q(pattern: &str) -> Query {
        Query {
            pattern: pattern.into(),
            ..Query::default()
        }
    }

    fn texts(rope: &Rope, ranges: &[(usize, usize)]) -> Vec<String> {
        ranges
            .iter()
            .map(|&(a, b)| rope.slice(a..b).to_string())
            .collect()
    }

    #[test]
    fn find_plain_case_insensitive() {
        let rope = Rope::from_str("Foo foo FOO f.o");
        let found = find_all(&rope, &q("foo")).unwrap();
        assert_eq!(found, [(0, 3), (4, 7), (8, 11)]);
        let exact = Query {
            case_sensitive: true,
            ..q("foo")
        };
        assert_eq!(find_all(&rope, &exact).unwrap(), [(4, 7)]);
        // Literal mode: regex metacharacters match themselves.
        assert_eq!(texts(&rope, &find_all(&rope, &q("f.o")).unwrap()), ["f.o"]);
    }

    #[test]
    fn find_whole_word() {
        let rope = Rope::from_str("cat concat cat_x cat. (cat) Cat");
        let whole = Query {
            whole_word: true,
            ..q("cat")
        };
        assert_eq!(
            find_all(&rope, &whole).unwrap(),
            [(0, 3), (17, 20), (23, 26), (28, 31)]
        );
        // Overlapping candidates: the rejected "aa" at 0 must not hide the word at 4.
        let rope = Rope::from_str("aaa aa");
        let whole = Query {
            whole_word: true,
            ..q("aa")
        };
        assert_eq!(find_all(&rope, &whole).unwrap(), [(4, 6)]);
        // Whole word applies to regex patterns too, and non-ASCII letters are word chars.
        let rope = Rope::from_str("việt viet");
        let whole = Query {
            whole_word: true,
            regex: true,
            ..q(r"vi\w*")
        };
        assert_eq!(find_all(&rope, &whole).unwrap(), [(0, 4), (5, 9)]);
        let whole = Query {
            whole_word: true,
            ..q("vi")
        };
        assert_eq!(find_all(&rope, &whole).unwrap(), []);
    }

    #[test]
    fn find_regex_multiline_off() {
        let re = |p: &str| Query {
            regex: true,
            case_sensitive: true,
            ..q(p)
        };
        // \n, \r\n and a lone \r all end a line (D31); no match crosses one.
        let rope = Rope::from_str("a\nb a\r\nb a\rb");
        assert_eq!(find_all(&rope, &re(r"a\s*b")).unwrap(), []);
        assert_eq!(find_all(&rope, &re(r"a[^x]b")).unwrap(), []);
        assert_eq!(
            find_all(&rope, &re("^b")).unwrap(),
            [(2, 3), (7, 8), (11, 12)]
        );
        assert_eq!(
            find_all(&rope, &re("a$")).unwrap(),
            [(0, 1), (4, 5), (9, 10)]
        );
        // `.` never eats the \r of a CRLF.
        let found = find_all(&rope, &re("b.*")).unwrap();
        assert_eq!(texts(&rope, &found), ["b a", "b a", "b"]);
    }

    #[test]
    fn invalid_regex_is_error() {
        let rope = Rope::from_str("text");
        let bad = Query {
            regex: true,
            ..q("fn (")
        };
        assert!(matches!(
            find_all(&rope, &bad),
            Err(QueryError::InvalidRegex(_))
        ));
        assert!(matches!(
            replace_all_tx(&rope, &bad, "x"),
            Err(QueryError::InvalidRegex(_))
        ));
        // The same text is fine as a literal.
        assert_eq!(find_all(&rope, &q("fn (")).unwrap(), []);
    }

    #[test]
    fn replace_all_regex_captures() {
        let src = "fn alpha() {}\nfn beta(x: u8) {}\r\n// fn gamma\n";
        let mut buf = Buffer::from_str(src);
        let query = Query {
            regex: true,
            case_sensitive: true,
            ..q(r"fn (\w+)")
        };
        let tx = replace_all_tx(buf.text(), &query, "fn new_$1").unwrap();
        assert_eq!(tx.changes.len(), 3);
        buf.apply(tx);
        assert_eq!(
            buf.text().to_string(),
            "fn new_alpha() {}\nfn new_beta(x: u8) {}\r\n// fn new_gamma\n"
        );
        // AC-10: one undo step restores everything; nothing else is left to undo.
        assert!(buf.undo());
        assert_eq!(buf.text().to_string(), src);
        assert!(!buf.undo());
        // Named groups and $0 expand; literal mode keeps `$` verbatim.
        let named = Query {
            regex: true,
            case_sensitive: true,
            ..q(r"fn (?<name>\w+)")
        };
        let rope = Rope::from_str("fn a");
        assert_eq!(applied(&rope, &named, "[$0|${name}]"), "[fn a|a]");
        assert_eq!(applied(&rope, &q("fn a"), "$1 cost"), "$1 cost");
    }

    #[test]
    fn replace_all_zero_length_no_loop() {
        let re = |p: &str| Query {
            regex: true,
            ..q(p)
        };
        let src = "a\r\n\nb\rc\n";
        let mut buf = Buffer::from_str(src);
        let tx = replace_all_tx(buf.text(), &re("^"), "// ").unwrap();
        buf.apply(tx);
        // Every line, empty ones included, gets one prefix; no line after the final break.
        assert_eq!(buf.text().to_string(), "// a\r\n// \n// b\r// c\n");
        assert!(buf.undo());
        assert_eq!(buf.text().to_string(), src);
        assert!(!buf.undo());
        // `$` and an empty-or-more pattern: one empty match per gap, none right after a match.
        assert_eq!(
            applied(&Rope::from_str("ab\ncd"), &re("$"), ";"),
            "ab;\ncd;"
        );
        assert_eq!(applied(&Rope::from_str("axxb"), &re("x*"), "-"), "-a-b-");
        assert_eq!(
            applied(&Rope::from_str("tiếng"), &re("(?:)"), "|"),
            "|t|i|ế|n|g|"
        );
        // An empty pattern finds and replaces nothing.
        assert_eq!(applied(&Rope::from_str("abc"), &q(""), "x"), "abc");
    }

    #[test]
    fn replace_all_unicode_offsets() {
        let rope = Rope::from_str("Tiếng Việt 中文 🙂 fn x\nđã fn y");
        let query = Query {
            regex: true,
            ..q(r"fn (\w)")
        };
        let found = find_all(&rope, &query).unwrap();
        assert_eq!(found, [(16, 20), (24, 28)]);
        assert_eq!(texts(&rope, &found), ["fn x", "fn y"]);
        assert_eq!(
            applied(&rope, &query, "fn ${1}2"),
            "Tiếng Việt 中文 🙂 fn x2\nđã fn y2"
        );
        let found = find_all(&rope, &q("VIỆT")).unwrap();
        assert_eq!(texts(&rope, &found), ["Việt"]);
    }

    #[test]
    fn replace_digit_group_before_word_char() {
        let query = Query {
            regex: true,
            case_sensitive: true,
            ..q(r"fn (\w+)")
        };
        let rope = Rope::from_str("fn alpha");
        assert_eq!(applied(&rope, &query, "fn $1_old"), "fn alpha_old");
        assert_eq!(applied(&rope, &query, "fn $1x$$1"), "fn alphax$1");
        let named = Query {
            regex: true,
            case_sensitive: true,
            ..q(r"fn (?<n>\w+)")
        };
        assert_eq!(applied(&rope, &named, "${n}_v $n"), "alpha_v alpha");
    }

    #[test]
    fn whole_word_agrees_with_word_boundaries() {
        // A combining mark (NFD) continues the word, as `\w` says.
        let rope = Rope::from_str("vie\u{301}t vie");
        let whole = Query {
            whole_word: true,
            ..q("vie")
        };
        assert_eq!(find_all(&rope, &whole).unwrap(), [(6, 9)]);
        // An alternative that fails the boundary does not hide a longer one at the same start.
        let rope = Rope::from_str("abc ab");
        let alt = Query {
            whole_word: true,
            regex: true,
            ..q("ab|abc")
        };
        assert_eq!(find_all(&rope, &alt).unwrap(), [(0, 3), (4, 6)]);
        // Wrapping must not make an invalid pattern valid.
        let bad = Query {
            whole_word: true,
            regex: true,
            ..q("a)(b")
        };
        assert!(find_all(&rope, &bad).is_err());
    }

    #[test]
    fn whole_word_with_extended_mode_comment() {
        let rope = Rope::from_str("foo food");
        let query = Query {
            whole_word: true,
            regex: true,
            ..q("(?x)foo # trailing comment")
        };
        assert_eq!(find_all(&rope, &query).unwrap(), [(0, 3)]);
    }
}
