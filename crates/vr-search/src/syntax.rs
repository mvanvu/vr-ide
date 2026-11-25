//! The find box's query syntax (T22): one text field instead of option toggles.
//!
//! - `/pattern/` is a regular expression; anything else is literal text.
//! - A trailing `/i` (`text/i`, `/pattern/i`) ignores case; without it the search is
//!   case-sensitive.
//! - `\/` is a slash that is neither the regex delimiter nor the `/i` marker; the backslash
//!   is dropped. No other escape exists: every other character, backslashes included, is
//!   taken as typed (a regex keeps its own `\d`, `\.`, ...).

use crate::find::Matcher;
use crate::{Query, QueryError};

impl Query {
    /// The query a find box text stands for. Never fails: an invalid regex is reported by
    /// [`Query::check`] (and by every search with it).
    pub fn parse(text: &str) -> Query {
        // With the /i marker, `body` keeps its slash: it may close a regex.
        let (body, ignore_case) = match text.strip_suffix("/i") {
            Some(rest) if !rest.ends_with('\\') => (&text[..text.len() - 1], true),
            _ => (text, false),
        };
        let regex_body = body
            .strip_prefix('/')
            .and_then(|r| r.strip_suffix('/'))
            .filter(|inner| !inner.ends_with('\\'));
        let (pattern, regex) = match regex_body {
            Some(inner) => (inner, true),
            None if ignore_case => (&body[..body.len() - 1], false),
            None => (body, false),
        };
        Query {
            pattern: pattern.replace("\\/", "/"),
            regex,
            case_sensitive: !ignore_case,
            whole_word: false,
        }
    }

    /// Whether the query can run: an invalid regex is an error, before any search starts.
    pub fn check(&self) -> Result<(), QueryError> {
        Matcher::new(self).map(drop)
    }
}

/// `text` as the find box types it so that [`Query::parse`] gives back the literal,
/// case-sensitive search for `text` (a selection seeding the box): every slash becomes
/// `\/`. `None` when that is impossible (the text has a backslash right before a slash).
pub fn literal_syntax(text: &str) -> Option<String> {
    if text.contains("\\/") {
        return None;
    }
    Some(text.replace('/', "\\/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(pattern: &str, regex: bool, case_sensitive: bool) -> Query {
        Query {
            pattern: pattern.to_string(),
            regex,
            case_sensitive,
            whole_word: false,
        }
    }

    #[test]
    fn parse_examples() {
        let cases = [
            ("foo", q("foo", false, true)),
            ("/fo+/", q("fo+", true, true)),
            ("foo/i", q("foo", false, false)),
            ("/fo+/i", q("fo+", true, false)),
            ("FOO/i", q("FOO", false, false)),
            // An escaped slash is a literal slash, in both modes.
            (r"a\/b", q("a/b", false, true)),
            (r"/a\/b/", q("a/b", true, true)),
            (r"/a\/b/i", q("a/b", true, false)),
            (r"\/fo+\/", q("/fo+/", false, true)),
            // The /i marker escaped: literal text ending in "/i".
            (r"and\/i", q("and/i", false, true)),
            // The closing slash escaped: no regex, literal text.
            (r"/fo+\/", q("/fo+/", false, true)),
            // Unclosed regex while typing: literal for now.
            ("/fo+", q("/fo+", false, true)),
            ("/", q("/", false, true)),
            ("//", q("", true, true)),
            ("/i", q("", false, false)),
            ("", q("", false, true)),
            // Slashes inside a regex need no escape; other escapes stay as typed.
            (r"/a/b\d/", q(r"a/b\d", true, true)),
            (r"C:\temp", q(r"C:\temp", false, true)),
            ("path/to/file", q("path/to/file", false, true)),
        ];
        for (text, want) in cases {
            assert_eq!(Query::parse(text), want, "{text:?}");
        }
    }

    #[test]
    fn check_reports_a_bad_regex_and_accepts_the_rest() {
        assert!(matches!(
            Query::parse("/fo(/").check(),
            Err(QueryError::InvalidRegex(_))
        ));
        assert!(Query::parse("/fo+/").check().is_ok());
        // Literal text is never an invalid regex.
        assert!(Query::parse("fo(").check().is_ok());
        assert!(Query::parse("").check().is_ok());
    }

    #[test]
    fn literal_syntax_round_trips() {
        for text in ["plain", "a/b", "/x/", "ends/i", "/fo+/i", r"back\slash", ""] {
            let typed = literal_syntax(text).unwrap();
            assert_eq!(Query::parse(&typed), q(text, false, true), "{text:?}");
        }
        assert_eq!(literal_syntax(r"a\/b"), None);
    }

    #[test]
    fn parsed_queries_search_as_described() {
        let rope = ropey::Rope::from_str("Foo foo fooo a/b\n");
        let find = |t: &str| crate::find_all(&rope, &Query::parse(t)).unwrap();
        assert_eq!(find("foo"), vec![(4, 7), (8, 11)]);
        assert_eq!(find("foo/i"), vec![(0, 3), (4, 7), (8, 11)]);
        assert_eq!(find("/fo+/"), vec![(4, 7), (8, 12)]);
        assert_eq!(find("/fo+/i"), vec![(0, 3), (4, 7), (8, 12)]);
        assert_eq!(find(r"a\/b"), vec![(13, 16)]);
        assert_eq!(find(r"/a\/b/"), vec![(13, 16)]);
    }
}
