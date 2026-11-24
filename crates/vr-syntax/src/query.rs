//! Compiled queries of one language: highlight captures with their precedence, injection
//! patterns, and the predicates tree-sitter leaves to the caller (nvim's `lua-match?`,
//! `has-ancestor?`, `gsub!`, `offset!`).

use std::ops::Range;

use regex::Regex;
use ropey::Rope;
use tree_sitter::{Language, Node, Query, QueryMatch, QueryPredicateArg, TextProvider};

use crate::capture::{Paint, paint_for};
use crate::{Grammar, Precedence, Reparse};

/// Bits of a pattern priority below the query-part index.
const PART_SHIFT: u32 = 20;

pub(crate) struct LanguageConfig {
    pub id: String,
    pub language: Language,
    pub reparse: Reparse,
    pub highlights: Query,
    /// Per capture index.
    pub paints: Vec<Paint>,
    /// Per pattern: among patterns capturing the same node, the highest wins.
    pub priority: Vec<u32>,
    /// Per pattern.
    pub highlight_preds: Vec<Vec<Pred>>,
    pub injections: Option<Injections>,
}

pub(crate) struct Injections {
    pub query: Query,
    pub content: Option<u32>,
    pub language: Option<u32>,
    /// Per pattern.
    pub patterns: Vec<InjectionPattern>,
    pub preds: Vec<Vec<Pred>>,
}

#[derive(Default)]
pub(crate) struct InjectionPattern {
    /// `#set! injection.language "x"`.
    pub language: Option<String>,
    /// All matches of the pattern form one layer (`injection.combined`).
    pub combined: bool,
    pub include_children: bool,
    /// `injection.self` / `injection.parent`: the language of the layer itself.
    pub same_language: bool,
    /// `#offset! @injection.content 0 a 0 b`: byte shifts of the content's start and end.
    pub offset: Option<(isize, isize)>,
    /// `#gsub! @injection.language "pattern" "replacement"`.
    pub gsub: Option<(Regex, String)>,
}

/// A predicate tree-sitter does not evaluate itself.
pub(crate) enum Pred {
    Match {
        capture: u32,
        regex: Regex,
        negate: bool,
    },
    Ancestor {
        capture: u32,
        kinds: Vec<String>,
        parent_only: bool,
        negate: bool,
    },
    /// An unknown `?` predicate: the pattern never matches rather than over-matching.
    Reject,
}

impl LanguageConfig {
    pub fn compile(id: &str, grammar: &Grammar) -> Result<LanguageConfig, String> {
        let language = grammar.language();
        let mut text = String::new();
        let mut part_starts = Vec::new();
        for part in grammar.highlight_parts() {
            part_starts.push(text.len());
            text.push_str(&part);
            text.push('\n');
        }
        let highlights =
            Query::new(&language, &text).map_err(|e| format!("{id}: highlights query: {e}"))?;
        let count = highlights.pattern_count();
        let first_wins = grammar.precedence() == Precedence::First;
        let priority = (0..count)
            .map(|i| {
                let start = highlights.start_byte_for_pattern(i);
                let part = part_starts
                    .partition_point(|&s| s <= start)
                    .saturating_sub(1);
                let within = if first_wins { count - i } else { i };
                ((part as u32) << PART_SHIFT) | within as u32
            })
            .collect();
        let paints = highlights
            .capture_names()
            .iter()
            .map(|n| paint_for(n))
            .collect();
        let highlight_preds = (0..count).map(|i| predicates(&highlights, i)).collect();
        let injections = Injections::compile(&language, &grammar.injections_query());
        Ok(LanguageConfig {
            id: id.to_owned(),
            language,
            reparse: grammar.reparse(),
            highlights,
            paints,
            priority,
            highlight_preds,
            injections,
        })
    }
}

impl Injections {
    /// `None` when the grammar has no injections or its query does not compile (the language
    /// still highlights, only without embedded languages).
    fn compile(language: &Language, source: &str) -> Option<Injections> {
        if source.trim().is_empty() {
            return None;
        }
        let query = Query::new(language, source).ok()?;
        let content = query.capture_index_for_name("injection.content");
        let lang_capture = query.capture_index_for_name("injection.language");
        let count = query.pattern_count();
        let mut patterns = Vec::with_capacity(count);
        for i in 0..count {
            let mut p = InjectionPattern::default();
            for prop in query.property_settings(i) {
                match &*prop.key {
                    "injection.language" => p.language = prop.value.as_deref().map(str::to_owned),
                    "injection.combined" => p.combined = true,
                    "injection.include-children" => p.include_children = true,
                    "injection.self" | "injection.parent" => p.same_language = true,
                    _ => {}
                }
            }
            for pred in query.general_predicates(i) {
                let strings: Vec<&str> = pred
                    .args
                    .iter()
                    .filter_map(|a| match a {
                        QueryPredicateArg::String(s) => Some(&**s),
                        QueryPredicateArg::Capture(_) => None,
                    })
                    .collect();
                let capture = pred.args.iter().find_map(|a| match a {
                    QueryPredicateArg::Capture(c) => Some(*c),
                    QueryPredicateArg::String(_) => None,
                });
                match &*pred.operator {
                    "offset!" if capture == content => {
                        let n: Vec<isize> = strings.iter().filter_map(|s| s.parse().ok()).collect();
                        if let [0, a, 0, b] = n[..] {
                            p.offset = Some((a, b));
                        }
                    }
                    "gsub!" if capture == lang_capture => {
                        if let [pattern, replacement] = strings[..]
                            && let Ok(re) = Regex::new(&lua_to_regex(pattern))
                        {
                            p.gsub = Some((re, lua_replacement(replacement)));
                        }
                    }
                    _ => {}
                }
            }
            patterns.push(p);
        }
        let preds = (0..count).map(|i| predicates(&query, i)).collect();
        Some(Injections {
            query,
            content,
            language: lang_capture,
            patterns,
            preds,
        })
    }
}

fn predicates(query: &Query, pattern: usize) -> Vec<Pred> {
    query
        .general_predicates(pattern)
        .iter()
        .filter_map(|p| {
            let op = &*p.operator;
            if op.ends_with('!') {
                return None; // directives: handled where they matter (injections)
            }
            let (negate, base) = match op.strip_prefix("not-") {
                Some(b) => (true, b),
                None => (false, op),
            };
            let capture = match p.args.first() {
                Some(QueryPredicateArg::Capture(c)) => *c,
                _ => return Some(Pred::Reject),
            };
            let strings: Vec<&str> = p.args[1..]
                .iter()
                .filter_map(|a| match a {
                    QueryPredicateArg::String(s) => Some(&**s),
                    QueryPredicateArg::Capture(_) => None,
                })
                .collect();
            Some(match base {
                "lua-match?" => match strings.first().map(|s| Regex::new(&lua_to_regex(s))) {
                    Some(Ok(regex)) => Pred::Match {
                        capture,
                        regex,
                        negate,
                    },
                    _ => Pred::Reject,
                },
                "has-ancestor?" | "has-parent?" => Pred::Ancestor {
                    capture,
                    kinds: strings.iter().map(|s| s.to_string()).collect(),
                    parent_only: base == "has-parent?",
                    negate,
                },
                _ => Pred::Reject,
            })
        })
        .collect()
}

/// Whether every caller-side predicate of a match holds.
pub(crate) fn predicates_hold(preds: &[Pred], m: &QueryMatch, text: &Rope) -> bool {
    preds.iter().all(|pred| match pred {
        Pred::Reject => false,
        Pred::Match {
            capture,
            regex,
            negate,
        } => m
            .captures()
            .iter()
            .filter(|c| c.index == *capture)
            .all(|c| regex.is_match(&node_text(text, c.node.byte_range())) != *negate),
        Pred::Ancestor {
            capture,
            kinds,
            parent_only,
            negate,
        } => m
            .captures()
            .iter()
            .filter(|c| c.index == *capture)
            .all(|c| has_ancestor(c.node, kinds, *parent_only) != *negate),
    })
}

fn has_ancestor(node: Node, kinds: &[String], parent_only: bool) -> bool {
    let mut cur = node.parent();
    while let Some(n) = cur {
        if kinds.iter().any(|k| k == n.kind()) {
            return true;
        }
        if parent_only {
            return false;
        }
        cur = n.parent();
    }
    false
}

/// Text of a byte range; lossy at a range that does not fall on char boundaries.
pub(crate) fn node_text(text: &Rope, range: Range<usize>) -> String {
    let bytes: Vec<u8> = RopeChunks::new(text, range).flatten().copied().collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// The chunks of a rope covering a byte range, cut at the range ends (any byte offsets).
pub(crate) struct RopeChunks<'a> {
    chunks: ropey::iter::Chunks<'a>,
    /// Byte offset of the next chunk the iterator yields.
    at: usize,
    range: Range<usize>,
}

impl<'a> RopeChunks<'a> {
    pub fn new(text: &'a Rope, range: Range<usize>) -> Self {
        let len = text.len_bytes();
        let range = range.start.min(len)..range.end.min(len);
        let (chunks, at, _, _) = text.chunks_at_byte(range.start);
        RopeChunks { chunks, at, range }
    }
}

impl<'a> Iterator for RopeChunks<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        if self.at >= self.range.end {
            return None;
        }
        let chunk = self.chunks.next()?.as_bytes();
        let start = self.at;
        self.at += chunk.len();
        let from = self.range.start.saturating_sub(start);
        let to = (self.range.end - start).min(chunk.len());
        Some(&chunk[from..to])
    }
}

/// Node text for tree-sitter's own text predicates (`#eq?`, `#match?`, `#any-of?`).
pub(crate) struct RopeText<'a>(pub &'a Rope);

impl<'a> TextProvider<&'a [u8]> for RopeText<'a> {
    type I = RopeChunks<'a>;

    fn text(&mut self, node: Node) -> Self::I {
        RopeChunks::new(self.0, node.byte_range())
    }
}

/// A Lua pattern (nvim queries) as a regex. Covers what queries use: `%` classes and escapes,
/// sets, anchors, `-` (lazy repetition) and captures.
pub(crate) fn lua_to_regex(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len() * 2);
    let mut in_set = false;
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '%' => {
                let Some(n) = chars.next() else {
                    out.push('%');
                    break;
                };
                let class = match n.to_ascii_lowercase() {
                    'a' => Some("alpha"),
                    'd' => Some("digit"),
                    'l' => Some("lower"),
                    'u' => Some("upper"),
                    's' => Some("space"),
                    'w' => Some("alnum"),
                    'x' => Some("xdigit"),
                    'p' => Some("punct"),
                    'c' => Some("cntrl"),
                    _ => None,
                };
                match class {
                    Some(name) => {
                        let neg = if n.is_ascii_uppercase() { "^" } else { "" };
                        if in_set {
                            out.push_str(&format!("[:{neg}{name}:]"));
                        } else {
                            out.push_str(&format!("[[:{neg}{name}:]]"));
                        }
                    }
                    None => out.push_str(&regex::escape(&n.to_string())),
                }
            }
            '[' if !in_set => {
                in_set = true;
                out.push('[');
                if chars.peek() == Some(&'^') {
                    out.push(chars.next().unwrap_or('^'));
                }
            }
            ']' if in_set => {
                in_set = false;
                out.push(']');
            }
            '-' if !in_set => out.push_str("*?"),
            '.' | '^' | '$' | '*' | '+' | '?' | '(' | ')' if !in_set => out.push(c),
            '-' => out.push('-'),
            _ => out.push_str(&regex::escape(&c.to_string())),
        }
    }
    out
}

/// Lua's `%1` group references as regex `${1}`.
fn lua_replacement(replacement: &str) -> String {
    let mut out = String::new();
    let mut chars = replacement.chars();
    while let Some(c) = chars.next() {
        match (c, chars.clone().next()) {
            ('%', Some(d)) if d.is_ascii_digit() => {
                chars.next();
                out.push_str(&format!("${{{d}}}"));
            }
            ('%', Some(p)) => {
                chars.next();
                out.push(p);
            }
            ('$', _) => out.push_str("$$"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lua_patterns_become_regexes() {
        let re = |p: &str| Regex::new(&lua_to_regex(p)).unwrap();
        assert!(re("%slang%s*=").is_match("<script lang =\"ts\">"));
        assert!(!re("%slang%s*=").is_match("<script>"));
        assert!(re("%${").is_match("a ${b}"));
        assert!(!re("%${").is_match("a {b}"));
        assert!(re("^on[a-z]+$").is_match("onclick"));
        assert!(!re("^on[a-z]+$").is_match("on-click"));
        assert!(re("^[%a_][%w_]*$").is_match("_x1"));
        assert!(!re("^[%a_][%w_]*$").is_match("1x"));
        assert!(re("^a.-b$").is_match("axxb"));
        assert!(re("%S").is_match("x") && !re("^%S$").is_match(" "));
    }

    #[test]
    fn gsub_replacement_uses_regex_groups() {
        let re = Regex::new(&lua_to_regex("(.+)/(.+)")).unwrap();
        let out = re.replace("text/javascript", lua_replacement("%2").as_str());
        assert_eq!(out, "javascript");
    }

    #[test]
    fn rope_chunks_cut_any_range() {
        let s: String = (0..5000)
            .map(|i| char::from(b'a' + (i % 26) as u8))
            .collect();
        let rope = Rope::from_str(&s);
        for (a, b) in [(0, 0), (0, 5000), (17, 4093), (1000, 1001), (4999, 6000)] {
            let got: Vec<u8> = RopeChunks::new(&rope, a..b).flatten().copied().collect();
            assert_eq!(got, s.as_bytes()[a.min(5000)..b.min(5000)], "{a}..{b}");
        }
    }
}
