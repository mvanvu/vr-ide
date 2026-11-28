use super::*;

const NONE: ModifiersState = ModifiersState::empty();
const S: ModifiersState = ModifiersState::SHIFT;
const C: ModifiersState = ModifiersState::CONTROL;
const A: ModifiersState = ModifiersState::ALT;
const SUP: ModifiersState = ModifiersState::SUPER;

const KS: KeyModifiers = KeyModifiers::SHIFT;
const KC: KeyModifiers = KeyModifiers::CONTROL;
const KA: KeyModifiers = KeyModifiers::ALT;
const KN: KeyModifiers = KeyModifiers::NONE;

fn ch(s: &str) -> Key {
    Key::Character(SmolStr::new(s))
}

fn press(logical: Key, physical: Code, text: Option<&str>) -> KeyInput {
    KeyInput {
        logical,
        physical: PhysicalKey::Code(physical),
        text: text.map(SmolStr::new),
        location: KeyLocation::Standard,
        state: ElementState::Pressed,
        repeat: false,
    }
}

fn numpad(logical: Key, physical: Code, text: Option<&str>) -> KeyInput {
    KeyInput {
        location: KeyLocation::Numpad,
        ..press(logical, physical, text)
    }
}

/// Every field, since crossterm's `PartialEq` folds case and shift together.
fn parts(ev: &KeyEvent) -> (KeyCode, KeyModifiers, KeyEventKind, KeyEventState) {
    (ev.code, ev.modifiers, ev.kind, ev.state)
}

struct Row {
    name: &'static str,
    input: KeyInput,
    mods: ModifiersState,
    want: Option<(KeyCode, KeyModifiers)>,
}

fn row(
    name: &'static str,
    input: KeyInput,
    mods: ModifiersState,
    want: Option<(KeyCode, KeyModifiers)>,
) -> Row {
    Row {
        name,
        input,
        mods,
        want,
    }
}

#[test]
fn key_table() {
    use KeyCode as K;
    use NamedKey as N;
    let rows = [
        // Plain text, as crossterm decodes it in a terminal.
        row(
            "a",
            press(ch("a"), Code::KeyA, Some("a")),
            NONE,
            Some((K::Char('a'), KN)),
        ),
        row(
            "shift+a",
            press(ch("A"), Code::KeyA, Some("A")),
            S,
            Some((K::Char('A'), KS)),
        ),
        row(
            "caps a",
            press(ch("A"), Code::KeyA, Some("A")),
            NONE,
            Some((K::Char('A'), KS)),
        ),
        row(
            "shift+/ is ?",
            press(ch("?"), Code::Slash, Some("?")),
            S,
            Some((K::Char('?'), KN)),
        ),
        row(
            "shift+1 is !",
            press(ch("!"), Code::Digit1, Some("!")),
            S,
            Some((K::Char('!'), KN)),
        ),
        row(
            "cyrillic ф",
            press(ch("ф"), Code::KeyA, Some("ф")),
            NONE,
            Some((K::Char('ф'), KN)),
        ),
        row(
            "cyrillic Ф",
            press(ch("Ф"), Code::KeyA, Some("Ф")),
            S,
            Some((K::Char('Ф'), KS)),
        ),
        row(
            "dead-key text é",
            press(ch("e"), Code::KeyE, Some("é")),
            NONE,
            Some((K::Char('é'), KN)),
        ),
        row(
            "vietnamese ệ",
            press(ch("ệ"), Code::KeyE, Some("ệ")),
            NONE,
            Some((K::Char('ệ'), KN)),
        ),
        row(
            "space",
            press(Key::Named(N::Space), Code::Space, Some(" ")),
            NONE,
            Some((K::Char(' '), KN)),
        ),
        row(
            "shift+space",
            press(Key::Named(N::Space), Code::Space, Some(" ")),
            S,
            Some((K::Char(' '), KS)),
        ),
        // ctrl chords: the ambiguous legacy ones stay distinct.
        row(
            "ctrl+a",
            press(ch("a"), Code::KeyA, Some("\u{1}")),
            C,
            Some((K::Char('a'), KC)),
        ),
        row(
            "ctrl+shift+a",
            press(ch("A"), Code::KeyA, None),
            C | S,
            Some((K::Char('A'), KC | KS)),
        ),
        row(
            "caps ctrl+a",
            press(ch("A"), Code::KeyA, None),
            C,
            Some((K::Char('a'), KC)),
        ),
        row(
            "ctrl+[",
            press(ch("["), Code::BracketLeft, Some("\u{1b}")),
            C,
            Some((K::Char('['), KC)),
        ),
        row(
            "esc",
            press(Key::Named(N::Escape), Code::Escape, Some("\u{1b}")),
            NONE,
            Some((K::Esc, KN)),
        ),
        row(
            "ctrl+h",
            press(ch("h"), Code::KeyH, Some("\u{8}")),
            C,
            Some((K::Char('h'), KC)),
        ),
        row(
            "backspace",
            press(Key::Named(N::Backspace), Code::Backspace, Some("\u{8}")),
            NONE,
            Some((K::Backspace, KN)),
        ),
        row(
            "ctrl+i",
            press(ch("i"), Code::KeyI, Some("\t")),
            C,
            Some((K::Char('i'), KC)),
        ),
        row(
            "tab",
            press(Key::Named(N::Tab), Code::Tab, Some("\t")),
            NONE,
            Some((K::Tab, KN)),
        ),
        row(
            "shift+tab",
            press(Key::Named(N::Tab), Code::Tab, None),
            S,
            Some((K::BackTab, KS)),
        ),
        row(
            "ctrl+tab",
            press(Key::Named(N::Tab), Code::Tab, None),
            C,
            Some((K::Tab, KC)),
        ),
        row(
            "ctrl+/",
            press(ch("/"), Code::Slash, None),
            C,
            Some((K::Char('/'), KC)),
        ),
        row(
            "ctrl+shift+/",
            press(ch("?"), Code::Slash, None),
            C | S,
            Some((K::Char('?'), KC | KS)),
        ),
        row(
            "ctrl+shift+1",
            press(ch("!"), Code::Digit1, None),
            C | S,
            Some((K::Char('!'), KC | KS)),
        ),
        row(
            "ctrl+`",
            press(ch("`"), Code::Backquote, None),
            C,
            Some((K::Char('`'), KC)),
        ),
        row(
            "ctrl+space",
            press(Key::Named(N::Space), Code::Space, None),
            C,
            Some((K::Char(' '), KC)),
        ),
        row(
            "ctrl+enter",
            press(Key::Named(N::Enter), Code::Enter, None),
            C,
            Some((K::Enter, KC)),
        ),
        row(
            "shift+enter",
            press(Key::Named(N::Enter), Code::Enter, None),
            S,
            Some((K::Enter, KS)),
        ),
        // alt and super chords.
        row(
            "alt+x",
            press(ch("x"), Code::KeyX, Some("x")),
            A,
            Some((K::Char('x'), KA)),
        ),
        row(
            "alt+shift+1",
            press(ch("!"), Code::Digit1, Some("!")),
            A | S,
            Some((K::Char('!'), KA | KS)),
        ),
        row(
            "ctrl+alt+shift+`",
            press(ch("~"), Code::Backquote, Some("~")),
            C | A | S,
            Some((K::Char('~'), KC | KA | KS)),
        ),
        row(
            "alt+shift+x",
            press(ch("X"), Code::KeyX, Some("X")),
            A | S,
            Some((K::Char('X'), KA | KS)),
        ),
        row(
            "macos option+a = å",
            press(ch("å"), Code::KeyA, Some("å")),
            A,
            Some((K::Char('a'), KA)),
        ),
        row(
            "super+a",
            press(ch("a"), Code::KeyA, None),
            SUP,
            Some((K::Char('a'), KeyModifiers::SUPER)),
        ),
        // Non-Latin layouts: the physical key's US name.
        row(
            "ctrl+ф (russian)",
            press(ch("ф"), Code::KeyA, None),
            C,
            Some((K::Char('a'), KC)),
        ),
        row(
            "ctrl+shift+Ф",
            press(ch("Ф"), Code::KeyA, None),
            C | S,
            Some((K::Char('A'), KC | KS)),
        ),
        row(
            "ctrl+ю is ctrl+.",
            press(ch("ю"), Code::Period, None),
            C,
            Some((K::Char('.'), KC)),
        ),
        row(
            "ctrl+ς (greek)",
            press(ch("ς"), Code::KeyW, None),
            C,
            Some((K::Char('w'), KC)),
        ),
        row(
            "alt+λ (greek)",
            press(ch("λ"), Code::KeyL, Some("λ")),
            A,
            Some((K::Char('l'), KA)),
        ),
        row(
            "ctrl+é (azerty)",
            press(ch("é"), Code::Digit2, None),
            C,
            Some((K::Char('2'), KC)),
        ),
        row(
            "ctrl+dead key",
            press(Key::Dead(Some('^')), Code::BracketLeft, None),
            C,
            Some((K::Char('['), KC)),
        ),
        // Named keys keep every modifier.
        row(
            "ctrl+shift+up",
            press(Key::Named(N::ArrowUp), Code::ArrowUp, None),
            C | S,
            Some((K::Up, KC | KS)),
        ),
        row(
            "down",
            press(Key::Named(N::ArrowDown), Code::ArrowDown, None),
            NONE,
            Some((K::Down, KN)),
        ),
        row(
            "alt+left",
            press(Key::Named(N::ArrowLeft), Code::ArrowLeft, None),
            A,
            Some((K::Left, KA)),
        ),
        row(
            "right",
            press(Key::Named(N::ArrowRight), Code::ArrowRight, None),
            NONE,
            Some((K::Right, KN)),
        ),
        row(
            "home",
            press(Key::Named(N::Home), Code::Home, None),
            NONE,
            Some((K::Home, KN)),
        ),
        row(
            "shift+end",
            press(Key::Named(N::End), Code::End, None),
            S,
            Some((K::End, KS)),
        ),
        row(
            "ctrl+pageup",
            press(Key::Named(N::PageUp), Code::PageUp, None),
            C,
            Some((K::PageUp, KC)),
        ),
        row(
            "pagedown",
            press(Key::Named(N::PageDown), Code::PageDown, None),
            NONE,
            Some((K::PageDown, KN)),
        ),
        row(
            "insert",
            press(Key::Named(N::Insert), Code::Insert, None),
            NONE,
            Some((K::Insert, KN)),
        ),
        row(
            "delete",
            press(Key::Named(N::Delete), Code::Delete, Some("\u{7f}")),
            NONE,
            Some((K::Delete, KN)),
        ),
        row(
            "menu",
            press(Key::Named(N::ContextMenu), Code::ContextMenu, None),
            NONE,
            Some((K::Menu, KN)),
        ),
        row(
            "f1",
            press(Key::Named(N::F1), Code::F1, None),
            NONE,
            Some((K::F(1), KN)),
        ),
        row(
            "shift+f5",
            press(Key::Named(N::F5), Code::F5, None),
            S,
            Some((K::F(5), KS)),
        ),
        row(
            "f12",
            press(Key::Named(N::F12), Code::F12, None),
            NONE,
            Some((K::F(12), KN)),
        ),
        row(
            "f13",
            press(Key::Named(N::F13), Code::F13, None),
            NONE,
            Some((K::F(13), KN)),
        ),
        row(
            "ctrl+f24",
            press(Key::Named(N::F24), Code::F24, None),
            C,
            Some((K::F(24), KC)),
        ),
        // Keys the app does not get.
        row(
            "shift alone",
            press(Key::Named(N::Shift), Code::ShiftLeft, None),
            S,
            None,
        ),
        row(
            "ctrl alone",
            press(Key::Named(N::Control), Code::ControlLeft, None),
            C,
            None,
        ),
        row(
            "alt alone",
            press(Key::Named(N::Alt), Code::AltLeft, None),
            A,
            None,
        ),
        row(
            "super alone",
            press(Key::Named(N::Super), Code::SuperLeft, None),
            SUP,
            None,
        ),
        row(
            "caps lock",
            press(Key::Named(N::CapsLock), Code::CapsLock, None),
            NONE,
            None,
        ),
        row(
            "dead key",
            press(Key::Dead(Some('´')), Code::Quote, None),
            NONE,
            None,
        ),
    ];
    let mut failures = Vec::new();
    for r in &rows {
        let got = translate_key(&r.input, r.mods).map(|ev| {
            assert_eq!(ev.kind, KeyEventKind::Press, "{}", r.name);
            (ev.code, ev.modifiers)
        });
        if got != r.want {
            failures.push(format!("{}: got {got:?}, want {:?}", r.name, r.want));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn keypad_keys_carry_the_keypad_state() {
    let enter = numpad(Key::Named(NamedKey::Enter), Code::NumpadEnter, Some("\r"));
    let one = numpad(ch("1"), Code::Numpad1, Some("1"));
    let begin = numpad(Key::Named(NamedKey::Clear), Code::Numpad5, None);
    let left = numpad(Key::Named(NamedKey::ArrowLeft), Code::Numpad4, None);
    let ctrl_plus = numpad(ch("+"), Code::NumpadAdd, None);
    let p = KeyEventKind::Press;
    let kp = KeyEventState::KEYPAD;
    let t = |i: &KeyInput, m| translate_key(i, m).as_ref().map(parts);
    assert_eq!(t(&enter, NONE), Some((KeyCode::Enter, KN, p, kp)));
    assert_eq!(t(&one, NONE), Some((KeyCode::Char('1'), KN, p, kp)));
    assert_eq!(t(&begin, NONE), Some((KeyCode::KeypadBegin, KN, p, kp)));
    assert_eq!(t(&left, NONE), Some((KeyCode::Left, KN, p, kp)));
    assert_eq!(t(&ctrl_plus, C), Some((KeyCode::Char('+'), KC, p, kp)));
}

#[test]
fn repeat_and_release_are_reported_with_the_same_key() {
    let mut held = press(ch("a"), Code::KeyA, Some("a"));
    held.repeat = true;
    let ev = translate_key(&held, C).unwrap();
    assert_eq!(
        parts(&ev),
        (
            KeyCode::Char('a'),
            KC,
            KeyEventKind::Repeat,
            KeyEventState::NONE
        )
    );

    // Releases carry no text; the logical key names them.
    let released = KeyInput {
        state: ElementState::Released,
        text: None,
        ..press(ch("A"), Code::KeyA, None)
    };
    let ev = translate_key(&released, S).unwrap();
    assert_eq!(
        parts(&ev),
        (
            KeyCode::Char('A'),
            KS,
            KeyEventKind::Release,
            KeyEventState::NONE
        )
    );

    let q = KeyInput {
        state: ElementState::Released,
        ..press(ch("q"), Code::KeyQ, None)
    };
    assert_eq!(
        translate_key(&q, C).map(|e| (e.code, e.modifiers, e.kind)),
        Some((KeyCode::Char('q'), KC, KeyEventKind::Release))
    );

    let mut enter = press(Key::Named(NamedKey::Enter), Code::Enter, Some("\r"));
    enter.repeat = true;
    assert_eq!(
        translate_key(&enter, NONE).unwrap().kind,
        KeyEventKind::Repeat
    );
}

#[test]
fn text_of_several_characters_is_one_event_each() {
    // Windows: a dead key that does not combine types itself and the next character.
    let input = press(ch("x"), Code::KeyX, Some("^x"));
    assert_eq!(translate_key(&input, NONE), None);
    let events: Vec<_> = key_events(&input, NONE).iter().map(parts).collect();
    let p = KeyEventKind::Press;
    let n = KeyEventState::NONE;
    assert_eq!(
        events,
        [
            (KeyCode::Char('^'), KN, p, n),
            (KeyCode::Char('x'), KN, p, n)
        ]
    );
}

#[test]
fn every_function_key_has_its_number() {
    use NamedKey as N;
    let keys = [
        N::F1,
        N::F2,
        N::F3,
        N::F4,
        N::F5,
        N::F6,
        N::F7,
        N::F8,
        N::F9,
        N::F10,
        N::F11,
        N::F12,
        N::F13,
        N::F14,
        N::F15,
        N::F16,
        N::F17,
        N::F18,
        N::F19,
        N::F20,
        N::F21,
        N::F22,
        N::F23,
        N::F24,
    ];
    for (i, k) in keys.into_iter().enumerate() {
        let ev = translate_key(&press(Key::Named(k), Code::F1, None), NONE).unwrap();
        assert_eq!(ev.code, KeyCode::F(u8::try_from(i + 1).unwrap()));
    }
}

#[test]
fn modifiers_map_one_to_one() {
    assert_eq!(modifiers(NONE), KN);
    assert_eq!(
        modifiers(C | A | S | SUP),
        KC | KA | KS | KeyModifiers::SUPER
    );
}
