//! The one display filter for text that comes from git (SEC-15, 02_threat-model_M7.md T7).
//!
//! Branch names, authors, commit subjects, paths and git's own messages are written by
//! whoever made the repo. Drawn as-is, an `ESC [2J`, an OSC 52 clipboard write, a C1 CSI, a
//! lone CR or a BIDI override (Trojan Source) could repaint or reorder what the user sees.
//! Every such character becomes a visible placeholder instead, so nothing reaches the
//! terminal that it would act on and nothing is silently hidden.

/// Columns a TAB is expanded to (relative to the start of the text).
const TAB_STOP: usize = 4;

/// `s` with every control and direction-changing character made visible:
/// C0 controls as caret notation (`^[` for ESC, `^M` for CR, `^J` for LF), DEL as `^?`,
/// C1 controls (U+0080..=U+009F) and BIDI controls (U+202A..=U+202E, U+2066..=U+2069,
/// U+200E, U+200F, U+061C) as `<U+XXXX>`, and the line / paragraph separators the same way.
/// TAB is expanded to spaces. Invalid UTF-8 is already U+FFFD in a `&str` made with
/// `from_utf8_lossy`, which is how vr-git builds its strings.
pub fn sanitize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut col = 0usize;
    for c in s.chars() {
        let before = out.len();
        match c {
            '\t' => {
                let n = TAB_STOP - col % TAB_STOP;
                out.extend(std::iter::repeat_n(' ', n));
            }
            '\u{0}'..='\u{1f}' => {
                out.push('^');
                out.push(char::from(c as u8 + 0x40));
            }
            '\u{7f}' => out.push_str("^?"),
            c if hidden(c) => out.push_str(&format!("<U+{:04X}>", c as u32)),
            c => out.push(c),
        }
        // Columns only matter for TAB stops; wide chars count as one here, which keeps
        // the expansion stable and never shorter than one space.
        col += out[before..].chars().count();
    }
    out
}

/// C1 controls and the characters that change the display order or break lines without
/// being a line break the renderer knows about.
fn hidden(c: char) -> bool {
    matches!(c,
        '\u{80}'..='\u{9f}'
        | '\u{200e}' | '\u{200f}' | '\u{061c}'
        | '\u{202a}'..='\u{202e}'
        | '\u{2066}'..='\u{2069}'
        | '\u{2028}' | '\u{2029}')
}

/// True when `s` holds a character [`sanitize`] replaces (tests and debug checks).
pub fn is_unsafe_char(c: char) -> bool {
    (c.is_control() && c != '\t') || hidden(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_become_visible_placeholders() {
        assert_eq!(sanitize("a\x1b[2Jb"), "a^[[2Jb");
        assert_eq!(sanitize("\x1b]52;c;ZXZpbA==\x07"), "^[]52;c;ZXZpbA==^G");
        assert_eq!(sanitize("x\u{9b}31m"), "x<U+009B>31m");
        assert_eq!(sanitize("evil\rgood"), "evil^Mgood");
        assert_eq!(sanitize("one\ntwo"), "one^Jtwo");
        assert_eq!(sanitize("del\u{7f}"), "del^?");
        assert_eq!(sanitize("abc\u{202e}fed"), "abc<U+202E>fed");
        for c in [
            '\u{202a}', '\u{202d}', '\u{2066}', '\u{2069}', '\u{200e}', '\u{200f}', '\u{061c}',
        ] {
            let s = sanitize(&format!("a{c}b"));
            assert!(!s.contains(c), "{c:?}: {s}");
            assert!(s.contains("<U+"), "{s}");
        }
    }

    #[test]
    fn plain_text_and_unicode_pass_unchanged() {
        for s in [
            "main",
            "feature/x-1",
            "Nguyễn Văn A",
            "漢字 👨‍👩‍👧",
            "a \u{fffd} b",
        ] {
            assert_eq!(sanitize(s), s);
        }
    }

    #[test]
    fn tab_expands_to_the_next_stop() {
        assert_eq!(sanitize("\tx"), "    x");
        assert_eq!(sanitize("ab\tx"), "ab  x");
    }

    #[test]
    fn nothing_unsafe_survives_any_char() {
        for cp in (0u32..0x3000).chain(0xfe00..0x10000) {
            let Some(c) = char::from_u32(cp) else {
                continue;
            };
            let s = sanitize(&format!("a{c}b"));
            assert!(!s.chars().any(is_unsafe_char), "U+{cp:04X} -> {s:?}");
        }
    }
}
