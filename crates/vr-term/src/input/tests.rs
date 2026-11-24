use super::*;

fn m(shift: bool, alt: bool, ctrl: bool) -> Modifiers {
    Modifiers { shift, alt, ctrl }
}

fn key(code: KeyCode, mods: Modifiers) -> Vec<u8> {
    encode_key(KeyEvent::new(code, mods), Modes::default())
}

fn app_cursor() -> Modes {
    Modes {
        app_cursor: true,
        ..Modes::default()
    }
}

#[test]
fn key_table_matches_xterm() {
    use KeyCode::*;
    let none = Modifiers::NONE;
    let table: &[(KeyCode, Modifiers, &[u8])] = &[
        (Char('a'), none, b"a"),
        (Char('A'), Modifiers::SHIFT, b"A"),
        (Char('ệ'), none, "ệ".as_bytes()),
        (Enter, none, b"\r"),
        (Enter, Modifiers::ALT, b"\x1b\r"),
        (Tab, none, b"\t"),
        (BackTab, none, b"\x1b[Z"),
        (Tab, Modifiers::SHIFT, b"\x1b[Z"),
        (Backspace, none, b"\x7f"),
        (Backspace, Modifiers::CTRL, b"\x08"),
        (Backspace, Modifiers::ALT, b"\x1b\x7f"),
        (Esc, none, b"\x1b"),
        (Up, none, b"\x1b[A"),
        (Down, none, b"\x1b[B"),
        (Right, none, b"\x1b[C"),
        (Left, none, b"\x1b[D"),
        (Up, Modifiers::SHIFT, b"\x1b[1;2A"),
        (Left, Modifiers::ALT, b"\x1b[1;3D"),
        (Right, Modifiers::CTRL, b"\x1b[1;5C"),
        (Down, m(true, false, true), b"\x1b[1;6B"),
        (Up, m(true, true, true), b"\x1b[1;8A"),
        (Home, none, b"\x1b[H"),
        (End, none, b"\x1b[F"),
        (Home, Modifiers::CTRL, b"\x1b[1;5H"),
        (End, Modifiers::SHIFT, b"\x1b[1;2F"),
        (Insert, none, b"\x1b[2~"),
        (Delete, none, b"\x1b[3~"),
        (PageUp, none, b"\x1b[5~"),
        (PageDown, none, b"\x1b[6~"),
        (Delete, Modifiers::CTRL, b"\x1b[3;5~"),
        (PageUp, Modifiers::SHIFT, b"\x1b[5;2~"),
        (F(1), none, b"\x1bOP"),
        (F(2), none, b"\x1bOQ"),
        (F(3), none, b"\x1bOR"),
        (F(4), none, b"\x1bOS"),
        (F(5), none, b"\x1b[15~"),
        (F(6), none, b"\x1b[17~"),
        (F(7), none, b"\x1b[18~"),
        (F(8), none, b"\x1b[19~"),
        (F(9), none, b"\x1b[20~"),
        (F(10), none, b"\x1b[21~"),
        (F(11), none, b"\x1b[23~"),
        (F(12), none, b"\x1b[24~"),
        (F(1), Modifiers::SHIFT, b"\x1b[1;2P"),
        (F(4), Modifiers::CTRL, b"\x1b[1;5S"),
        (F(5), Modifiers::CTRL, b"\x1b[15;5~"),
        (F(12), Modifiers::ALT, b"\x1b[24;3~"),
        // ctrl + letter → C0 control; case does not matter.
        (Char('a'), Modifiers::CTRL, b"\x01"),
        (Char('c'), Modifiers::CTRL, b"\x03"),
        (Char('d'), Modifiers::CTRL, b"\x04"),
        (Char('z'), Modifiers::CTRL, b"\x1a"),
        (Char('C'), m(true, false, true), b"\x03"),
        // ctrl + space / @ / 2 / ` → NUL.
        (Char(' '), Modifiers::CTRL, b"\x00"),
        (Char('@'), Modifiers::CTRL, b"\x00"),
        (Char('2'), Modifiers::CTRL, b"\x00"),
        (Char('`'), Modifiers::CTRL, b"\x00"),
        // Other C0 punctuation and xterm's ctrl+digit legacy mapping.
        (Char('['), Modifiers::CTRL, b"\x1b"),
        (Char('\\'), Modifiers::CTRL, b"\x1c"),
        (Char(']'), Modifiers::CTRL, b"\x1d"),
        (Char('^'), Modifiers::CTRL, b"\x1e"),
        (Char('_'), Modifiers::CTRL, b"\x1f"),
        (Char('/'), Modifiers::CTRL, b"\x1f"),
        (Char('3'), Modifiers::CTRL, b"\x1b"),
        (Char('6'), Modifiers::CTRL, b"\x1e"),
        (Char('8'), Modifiers::CTRL, b"\x7f"),
        (Char('?'), Modifiers::CTRL, b"\x7f"),
        // alt prefixes ESC, also on ctrl combos.
        (Char('x'), Modifiers::ALT, b"\x1bx"),
        (Char('é'), Modifiers::ALT, "\x1bé".as_bytes()),
        (Char('a'), m(false, true, true), b"\x1b\x01"),
        (Char(' '), m(false, true, true), b"\x1b\x00"),
        // ctrl on a character without a control code sends the character.
        (Char('ệ'), Modifiers::CTRL, "ệ".as_bytes()),
    ];
    let mut failures = Vec::new();
    for (code, mods, want) in table {
        let got = key(*code, *mods);
        if got != *want {
            failures.push(format!("{code:?} {mods:?}: got {got:?}, want {want:?}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn app_cursor_mode_switches_unmodified_cursor_keys_to_ss3() {
    use KeyCode::*;
    let table: &[(KeyCode, Modifiers, &[u8])] = &[
        (Up, Modifiers::NONE, b"\x1bOA"),
        (Down, Modifiers::NONE, b"\x1bOB"),
        (Right, Modifiers::NONE, b"\x1bOC"),
        (Left, Modifiers::NONE, b"\x1bOD"),
        (Home, Modifiers::NONE, b"\x1bOH"),
        (End, Modifiers::NONE, b"\x1bOF"),
        // Modified keys keep the CSI form in app mode, as xterm does.
        (Up, Modifiers::CTRL, b"\x1b[1;5A"),
        // Keys outside the cursor set are unaffected.
        (Delete, Modifiers::NONE, b"\x1b[3~"),
        (F(1), Modifiers::NONE, b"\x1bOP"),
    ];
    for (code, mods, want) in table {
        assert_eq!(
            encode_key(KeyEvent::new(*code, *mods), app_cursor()),
            *want,
            "{code:?} {mods:?}"
        );
    }
}

#[test]
fn app_keypad_mode_leaves_main_keys_alone() {
    // vr-term receives no keypad-specific key codes, so DECKPAM changes nothing it sends.
    let modes = Modes {
        app_keypad: true,
        ..Modes::default()
    };
    assert_eq!(encode_key(KeyEvent::plain(KeyCode::Enter), modes), b"\r");
    assert_eq!(encode_key(KeyEvent::plain(KeyCode::Char('5')), modes), b"5");
}

#[test]
fn paste_without_brackets_sends_text_with_cr_line_breaks() {
    let modes = Modes::default();
    assert_eq!(encode_paste("ls -la", modes), b"ls -la");
    assert_eq!(encode_paste("a\nb\r\nc\rd", modes), b"a\rb\rc\rd");
    assert_eq!(encode_paste("Tiếng Việt", modes), "Tiếng Việt".as_bytes());
    assert_eq!(encode_paste("", modes), b"");
}

#[test]
fn paste_is_bracketed_when_enabled_and_cannot_close_the_bracket_early() {
    let modes = Modes {
        bracketed_paste: true,
        ..Modes::default()
    };
    assert_eq!(
        encode_paste("echo hi\n", modes),
        b"\x1b[200~echo hi\r\x1b[201~"
    );
    // An embedded end marker must not end the paste and run what follows.
    assert_eq!(
        encode_paste("x\x1b[201~rm -rf ~\n", modes),
        b"\x1b[200~x[201~rm -rf ~\r\x1b[201~"
    );
    assert_eq!(encode_paste("", modes), b"\x1b[200~\x1b[201~");
}

fn mouse(kind: MouseKind, col: u16, row: u16, mods: Modifiers) -> MouseEvent {
    MouseEvent {
        kind,
        col,
        row,
        mods,
    }
}

#[test]
fn mouse_reports_nothing_unless_sgr_reporting_is_on() {
    let press = mouse(MouseKind::Press(MouseButton::Left), 0, 0, Modifiers::NONE);
    assert!(encode_mouse(press, Modes::default()).is_empty());
    let legacy = Modes {
        mouse_reporting: true,
        ..Modes::default()
    };
    assert!(encode_mouse(press, legacy).is_empty(), "no X10 encoding");
    let sgr_only = Modes {
        sgr_mouse: true,
        ..Modes::default()
    };
    assert!(
        encode_mouse(press, sgr_only).is_empty(),
        "1006 alone does not enable reporting"
    );
}

#[test]
fn mouse_sgr_encoding() {
    use MouseButton::*;
    use MouseKind::*;
    let click = Modes {
        mouse_reporting: true,
        sgr_mouse: true,
        ..Modes::default()
    };
    let drag = Modes {
        mouse_drag: true,
        ..click
    };
    let motion = Modes {
        mouse_motion: true,
        ..click
    };
    let none = Modifiers::NONE;
    let cases: &[(MouseEvent, Modes, &[u8])] = &[
        (mouse(Press(Left), 0, 0, none), click, b"\x1b[<0;1;1M"),
        (mouse(Release(Left), 0, 0, none), click, b"\x1b[<0;1;1m"),
        (mouse(Press(Middle), 9, 4, none), click, b"\x1b[<1;10;5M"),
        (
            mouse(Press(Right), 299, 99, none),
            click,
            b"\x1b[<2;300;100M",
        ),
        (mouse(ScrollUp, 2, 3, none), click, b"\x1b[<64;3;4M"),
        (mouse(ScrollDown, 2, 3, none), click, b"\x1b[<65;3;4M"),
        (
            mouse(Press(Left), 0, 0, Modifiers::SHIFT),
            click,
            b"\x1b[<4;1;1M",
        ),
        (
            mouse(Press(Left), 0, 0, Modifiers::ALT),
            click,
            b"\x1b[<8;1;1M",
        ),
        (
            mouse(Press(Left), 0, 0, Modifiers::CTRL),
            click,
            b"\x1b[<16;1;1M",
        ),
        // Drags only in 1002/1003, plain motion only in 1003.
        (mouse(Drag(Left), 5, 5, none), click, b""),
        (mouse(Drag(Left), 5, 5, none), drag, b"\x1b[<32;6;6M"),
        (mouse(Drag(Right), 5, 5, none), motion, b"\x1b[<34;6;6M"),
        (mouse(Move, 5, 5, none), drag, b""),
        (mouse(Move, 5, 5, none), motion, b"\x1b[<35;6;6M"),
    ];
    for (event, modes, want) in cases {
        assert_eq!(encode_mouse(*event, *modes), *want, "{event:?} {modes:?}");
    }
}
