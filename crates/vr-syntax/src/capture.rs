//! Capture classes: the small set of colours a theme maps, from tree-sitter capture names.

/// A highlight class. Themes map each one to a style (`Theme::syntax`, vr-ide).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Capture {
    Keyword,
    String,
    Comment,
    Function,
    Type,
    Number,
    Constant,
    VariableBuiltin,
    Property,
    Tag,
    Attribute,
    Operator,
    Punctuation,
    Escape,
    Other,
}

impl Capture {
    pub const ALL: [Capture; 15] = [
        Capture::Keyword,
        Capture::String,
        Capture::Comment,
        Capture::Function,
        Capture::Type,
        Capture::Number,
        Capture::Constant,
        Capture::VariableBuiltin,
        Capture::Property,
        Capture::Tag,
        Capture::Attribute,
        Capture::Operator,
        Capture::Punctuation,
        Capture::Escape,
        Capture::Other,
    ];

    /// The class of a capture name, by the longest dotted prefix in the table (classic
    /// tree-sitter names and nvim-treesitter names). Unknown names are `Other`.
    pub fn from_name(name: &str) -> Capture {
        let mut prefix = name;
        loop {
            if let Some((_, c)) = PREFIXES.iter().find(|(p, _)| *p == prefix) {
                return *c;
            }
            match prefix.rfind('.') {
                Some(dot) => prefix = &prefix[..dot],
                None => return Capture::Other,
            }
        }
    }
}

use Capture as C;

/// Capture-name prefixes. A longer entry beats a shorter one (`string.escape` over `string`).
const PREFIXES: &[(&str, Capture)] = &[
    ("keyword", C::Keyword),
    ("conditional", C::Keyword),
    ("repeat", C::Keyword),
    ("include", C::Keyword),
    ("exception", C::Keyword),
    ("storageclass", C::Keyword),
    ("define", C::Keyword),
    ("preproc", C::Keyword),
    ("type.qualifier", C::Keyword),
    ("string", C::String),
    ("string.escape", C::Escape),
    ("string.special.key", C::Property),
    ("string.special.symbol", C::Constant),
    ("character", C::String),
    ("character.special", C::Escape),
    ("escape", C::Escape),
    ("comment", C::Comment),
    ("function", C::Function),
    ("method", C::Function),
    ("constructor", C::Type),
    ("type", C::Type),
    ("module", C::Type),
    ("namespace", C::Type),
    ("number", C::Number),
    ("float", C::Number),
    ("boolean", C::Constant),
    ("constant", C::Constant),
    ("variable", C::Other),
    ("variable.builtin", C::VariableBuiltin),
    ("variable.member", C::Property),
    ("property", C::Property),
    ("field", C::Property),
    ("tag", C::Tag),
    ("tag.attribute", C::Attribute),
    ("tag.delimiter", C::Punctuation),
    ("attribute", C::Attribute),
    ("operator", C::Operator),
    ("punctuation", C::Punctuation),
    // Markdown: nvim `markup.*` and the older `text.*` names.
    ("markup.heading", C::Keyword),
    ("text.title", C::Keyword),
    ("markup.raw", C::String),
    ("text.literal", C::String),
    ("markup.math", C::String),
    ("markup.link", C::Function),
    ("text.reference", C::Function),
    ("markup.link.url", C::Constant),
    ("text.uri", C::Constant),
    ("markup.list", C::Punctuation),
    ("markup.quote", C::Comment),
];

/// What a capture does to the text it covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Paint {
    Colour(Capture),
    /// nvim's `@none`: no colour, also not the enclosing node's.
    Clear,
    /// Not a highlight (private `@_x`, spell checking, conceal, injection or locals captures).
    Ignore,
}

pub(crate) fn paint_for(name: &str) -> Paint {
    let first = name.split('.').next().unwrap_or(name);
    if name.starts_with('_')
        || matches!(
            first,
            "spell" | "nospell" | "conceal" | "injection" | "local" | "fold" | "indent"
        )
    {
        Paint::Ignore
    } else if name == "none" {
        Paint::Clear
    } else {
        Paint::Colour(Capture::from_name(name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_and_nvim_names_map_by_longest_prefix() {
        let cases = [
            ("keyword", Capture::Keyword),
            ("keyword.function", Capture::Keyword),
            ("keyword.operator", Capture::Keyword),
            ("conditional", Capture::Keyword),
            ("repeat", Capture::Keyword),
            ("include", Capture::Keyword),
            ("string", Capture::String),
            ("string.special", Capture::String),
            ("string.escape", Capture::Escape),
            ("string.special.key", Capture::Property),
            ("character", Capture::String),
            ("escape", Capture::Escape),
            ("comment", Capture::Comment),
            ("comment.documentation", Capture::Comment),
            ("function", Capture::Function),
            ("function.method", Capture::Function),
            ("function.builtin", Capture::Function),
            ("method.call", Capture::Function),
            ("constructor", Capture::Type),
            ("type", Capture::Type),
            ("type.builtin", Capture::Type),
            ("module", Capture::Type),
            ("namespace", Capture::Type),
            ("number", Capture::Number),
            ("number.float", Capture::Number),
            ("float", Capture::Number),
            ("boolean", Capture::Constant),
            ("constant", Capture::Constant),
            ("constant.builtin", Capture::Constant),
            ("variable", Capture::Other),
            ("variable.parameter", Capture::Other),
            ("variable.builtin", Capture::VariableBuiltin),
            ("variable.member", Capture::Property),
            ("property", Capture::Property),
            ("field", Capture::Property),
            ("tag", Capture::Tag),
            ("tag.builtin", Capture::Tag),
            ("tag.attribute", Capture::Attribute),
            ("tag.delimiter", Capture::Punctuation),
            ("attribute", Capture::Attribute),
            ("operator", Capture::Operator),
            ("punctuation", Capture::Punctuation),
            ("punctuation.bracket", Capture::Punctuation),
            ("punctuation.special", Capture::Punctuation),
            ("markup.heading.1", Capture::Keyword),
            ("text.title", Capture::Keyword),
            ("markup.raw.block", Capture::String),
            ("text.literal", Capture::String),
            ("markup.link.url", Capture::Constant),
            ("text.uri", Capture::Constant),
            ("markup.link.label", Capture::Function),
            ("text.reference", Capture::Function),
            ("markup.list", Capture::Punctuation),
            ("markup.quote", Capture::Comment),
            ("label", Capture::Other),
            ("embedded", Capture::Other),
            ("something.new", Capture::Other),
        ];
        for (name, want) in cases {
            assert_eq!(Capture::from_name(name), want, "{name}");
        }
    }

    #[test]
    fn prefix_matches_only_at_a_dot() {
        // `keywords` is not `keyword.*`, `typed` is not `type.*`.
        assert_eq!(Capture::from_name("keywords"), Capture::Other);
        assert_eq!(Capture::from_name("typed"), Capture::Other);
    }

    #[test]
    fn non_colour_captures_are_ignored_or_clear() {
        for name in [
            "_name",
            "_tag",
            "spell",
            "nospell",
            "conceal",
            "injection.content",
            "injection.language",
            "local.scope",
            "local.definition",
        ] {
            assert_eq!(paint_for(name), Paint::Ignore, "{name}");
        }
        assert_eq!(paint_for("none"), Paint::Clear);
        assert_eq!(paint_for("keyword"), Paint::Colour(Capture::Keyword));
        assert_eq!(paint_for("variable"), Paint::Colour(Capture::Other));
    }

    #[test]
    fn all_lists_every_class_once_in_discriminant_order() {
        for (i, c) in Capture::ALL.iter().enumerate() {
            assert_eq!(*c as usize, i);
        }
    }
}
