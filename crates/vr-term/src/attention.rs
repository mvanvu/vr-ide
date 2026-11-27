//! Finds the program's requests for the user in its raw output: the bell (BEL) and the desktop
//! notifications OSC 9 and OSC 777;notify. The emulator drops the notifications, so the bytes
//! are watched here; nothing is removed from the stream.

/// Longest OSC body kept, in bytes; the rest of a longer one is skipped.
const MAX_BODY: usize = 1024;

/// What the program asked with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attention {
    /// The notification's text; `None` for the bell.
    pub text: Option<String>,
}

#[derive(Default)]
enum State {
    #[default]
    Ground,
    /// After ESC.
    Esc,
    /// Inside OSC; the body so far is in `body`.
    Osc,
    /// ESC inside OSC (ST may follow).
    OscEsc,
    /// Inside DCS / SOS / PM / APC: skipped to ST.
    Skip,
    SkipEsc,
}

#[derive(Default)]
pub(crate) struct AttentionScan {
    state: State,
    body: Vec<u8>,
}

impl AttentionScan {
    pub fn feed(&mut self, input: &[u8], out: &mut Vec<Attention>) {
        for &b in input {
            self.state = match std::mem::take(&mut self.state) {
                State::Ground => match b {
                    0x07 => {
                        out.push(Attention { text: None });
                        State::Ground
                    }
                    0x1b => State::Esc,
                    _ => State::Ground,
                },
                State::Esc => self.after_esc(b),
                State::Osc => match b {
                    // BEL ends the string; it is not a bell.
                    0x07 => self.finish(out),
                    0x1b => State::OscEsc,
                    // CAN and SUB abort the string.
                    0x18 | 0x1a => State::Ground,
                    _ => {
                        if self.body.len() < MAX_BODY {
                            self.body.push(b);
                        }
                        State::Osc
                    }
                },
                State::OscEsc => match b {
                    b'\\' => self.finish(out),
                    _ => self.after_esc(b),
                },
                // DCS, SOS, PM and APC end only at ST, CAN or SUB; a BEL inside is payload (as in
                // the emulator's own parser).
                State::Skip => match b {
                    0x18 | 0x1a => State::Ground,
                    0x1b => State::SkipEsc,
                    _ => State::Skip,
                },
                State::SkipEsc => match b {
                    b'\\' => State::Ground,
                    _ => self.after_esc(b),
                },
            };
        }
    }

    /// The state after ESC and `b`: a new string starts, or the sequence is not one of ours.
    fn after_esc(&mut self, b: u8) -> State {
        match b {
            b']' => {
                self.body.clear();
                State::Osc
            }
            b'P' | b'X' | b'^' | b'_' => State::Skip,
            0x1b => State::Esc,
            _ => State::Ground,
        }
    }

    fn finish(&mut self, out: &mut Vec<Attention>) -> State {
        if let Some(text) = notification(&self.body) {
            out.push(Attention { text: Some(text) });
        }
        self.body.clear();
        State::Ground
    }
}

/// The text of an OSC 9 or OSC 777;notify body; `None` for any other OSC (titles, hyperlinks)
/// and for OSC 9;4 (progress reports, not a request).
fn notification(body: &[u8]) -> Option<String> {
    let text = if let Some(rest) = body.strip_prefix(b"9;") {
        // ConEmu sub-commands (9;4 progress, 9;9 cwd, ...) are no message.
        let digits = rest.iter().take_while(|c| c.is_ascii_digit()).count();
        if (1..=2).contains(&digits) && matches!(rest.get(digits), None | Some(b';')) {
            return None;
        }
        String::from_utf8_lossy(rest).into_owned()
    } else {
        let rest = body.strip_prefix(b"777;notify;")?;
        // `title;body`: both parts are the message.
        let rest = String::from_utf8_lossy(rest);
        match rest.split_once(';') {
            Some((title, text)) if !title.is_empty() && !text.is_empty() => {
                format!("{title}: {text}")
            }
            Some((title, text)) => format!("{title}{text}"),
            None => rest.into_owned(),
        }
    };
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(chunks: &[&[u8]]) -> Vec<Option<String>> {
        let mut scan = AttentionScan::default();
        let mut out = Vec::new();
        for c in chunks {
            scan.feed(c, &mut out);
        }
        out.into_iter().map(|a| a.text).collect()
    }

    #[test]
    fn bell_and_both_notifications_are_found() {
        assert_eq!(run(&[b"a\x07b"]), vec![None]);
        assert_eq!(
            run(&[b"\x1b]9;Allow it?\x07"]),
            vec![Some("Allow it?".into())]
        );
        assert_eq!(
            run(&[b"\x1b]9;Allow it?\x1b\\"]),
            vec![Some("Allow it?".into())]
        );
        assert_eq!(
            run(&[b"\x1b]777;notify;claude;needs you\x07"]),
            vec![Some("claude: needs you".into())]
        );
    }

    #[test]
    fn the_bell_that_ends_a_title_or_progress_report_is_not_a_bell() {
        assert!(run(&[b"\x1b]0;title\x07\x1b]9;4;1;50\x07\x1b]8;;http://x\x07"]).is_empty());
        assert!(run(&[b"\x1bPdata\x07more\x1b\\"]).is_empty());
    }

    #[test]
    fn a_split_read_changes_nothing() {
        let stream = b"x\x1b]9;hello\x07y\x07z\x1b]777;notify;t;b\x1b\\";
        let whole = run(&[stream]);
        assert_eq!(whole.len(), 3);
        for cut in 0..=stream.len() {
            assert_eq!(run(&[&stream[..cut], &stream[cut..]]), whole, "cut {cut}");
        }
    }

    #[test]
    fn a_long_body_is_capped() {
        let mut long = b"\x1b]9;".to_vec();
        long.extend(std::iter::repeat_n(b'a', 100_000));
        long.push(0x07);
        let got = run(&[&long]);
        assert_eq!(got.len(), 1);
        assert!(got[0].as_ref().unwrap().len() <= MAX_BODY);
    }

    #[test]
    fn an_aborted_string_gives_no_text_and_its_bell_byte_counts_as_a_bell() {
        // CAN ends the OSC without a message; the BEL after it is then an ordinary bell.
        assert_eq!(run(&[b"\x1b]9;half\x18 \x07"]), vec![None]);
    }

    #[test]
    fn bells_inside_a_dcs_are_payload() {
        assert!(run(&[b"\x1bPq\x07ab\x07cd\x1b\\"]).is_empty());
        // ... and after its ST the bell is a bell again.
        assert_eq!(run(&[b"\x1bPq\x07\x1b\\\x07"]), vec![None]);
        // A string cut off by ESC P: the new DCS keeps its BELs too.
        assert!(run(&[b"\x1b]9;x\x1bPq\x07\x1b\\"]).is_empty());
    }

    #[test]
    fn conemu_sub_commands_are_no_notification() {
        assert!(run(&[b"\x1b]9;4;1;50\x07\x1b]9;9;/tmp\x07\x1b]9;12\x07"]).is_empty());
        assert_eq!(
            run(&[b"\x1b]9;5 files done\x07"]),
            vec![Some("5 files done".into())]
        );
    }
}
