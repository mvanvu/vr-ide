//! Text from git made safe to show: control and BIDI characters made visible (the character rules of SEC-15; tab expansion and width
//! truncation are the UI's).

/// One line for display: C0 controls but TAB, DEL and C1 become caret/`<U+..>` placeholders,
/// BIDI controls become `<U+..>`, so remote or repo text cannot drive the terminal
/// (OSC 52, cursor moves, Trojan Source).
pub fn sanitize_line(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        let u = c as u32;
        match u {
            0x09 => out.push(c),
            0x00..=0x1f => {
                out.push('^');
                out.push(char::from(b'@' + u as u8));
            }
            0x7f => out.push_str("^?"),
            0x80..=0x9f | 0x202a..=0x202e | 0x2066..=0x2069 | 0x200e | 0x200f | 0x061c => {
                out.push_str(&format!("<U+{u:04X}>"))
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_become_visible() {
        assert_eq!(
            sanitize_line("remote: \x1b]52;c;ZXZpbA==\x07ok\r\u{9b}\u{202e}\tx"),
            "remote: ^[]52;c;ZXZpbA==^Gok^M<U+009B><U+202E>\tx"
        );
    }
}
