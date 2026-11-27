use super::*;
use crossterm::event::{KeyCode, KeyModifiers};
use winit::keyboard::{KeyCode as Code, KeyLocation, PhysicalKey, SmolStr};

const NONE: ModifiersState = ModifiersState::empty();

fn key(logical: Key, code: Code, state: ElementState) -> KeyInput {
    let text = match (&logical, state) {
        (Key::Character(s), ElementState::Pressed) => Some(s.clone()),
        _ => None,
    };
    KeyInput {
        logical,
        physical: PhysicalKey::Code(code),
        text,
        location: KeyLocation::Standard,
        state,
        repeat: false,
    }
}

fn ch(s: &str, code: Code) -> KeyInput {
    key(Key::Character(SmolStr::new(s)), code, ElementState::Pressed)
}

fn esc(state: ElementState) -> KeyInput {
    key(Key::Named(NamedKey::Escape), Code::Escape, state)
}

fn preedit(text: &str) -> Ime {
    Ime::Preedit(text.to_owned(), Some((text.len(), text.len())))
}

fn codes(events: &[KeyEvent]) -> Vec<(KeyCode, KeyModifiers, KeyEventKind)> {
    events
        .iter()
        .map(|e| (e.code, e.modifiers, e.kind))
        .collect()
}

fn presses(text: &str) -> Vec<(KeyCode, KeyModifiers, KeyEventKind)> {
    text.chars()
        .map(|c| {
            let m = if c.is_uppercase() {
                KeyModifiers::SHIFT
            } else {
                KeyModifiers::NONE
            };
            (KeyCode::Char(c), m, KeyEventKind::Press)
        })
        .collect()
}

#[test]
fn telex_viet_composes_then_commits_once() {
    let mut ime = ImeInput::new();
    assert!(ime.ime(&Ime::Enabled).is_empty());
    assert!(ime.is_enabled());

    // The first key reaches the IME; winit sends no key event for it, only the preedit.
    let steps = ["v", "vi", "vie", "viê", "việ", "việt"];
    for s in steps {
        assert!(ime.ime(&preedit(s)).is_empty(), "preedit {s} is not sent");
        assert_eq!(ime.preedit().map(|p| p.text.as_str()), Some(s));
        // Any key event that still arrives while composing is held back.
        assert!(ime.key(&ch("t", Code::KeyT), NONE).is_empty());
    }
    assert!(ime.is_composing());

    // winit clears the preedit right before the commit.
    assert!(ime.ime(&Ime::Preedit(String::new(), None)).is_empty());
    let sent = ime.ime(&Ime::Commit("việt".into()));
    assert_eq!(codes(&sent), presses("việt"));
    assert!(ime.preedit().is_none());
    assert!(!ime.take_cancel_request());

    // Composition over: keys flow again.
    assert_eq!(codes(&ime.key(&ch("a", Code::KeyA), NONE)), presses("a"));
}

#[test]
fn esc_cancels_the_preedit_without_sending_anything() {
    let mut ime = ImeInput::new();
    ime.ime(&Ime::Enabled);
    ime.ime(&preedit("việ"));
    assert!(ime.key(&esc(ElementState::Pressed), NONE).is_empty());
    assert!(ime.preedit().is_none(), "Esc drops the preedit");
    assert!(ime.take_cancel_request(), "the window resets the IME");
    assert!(!ime.take_cancel_request(), "once");
    // The IME's own clearing and the Esc release send nothing the app acts on.
    assert!(ime.ime(&Ime::Preedit(String::new(), None)).is_empty());
    let release = ime.key(&esc(ElementState::Released), NONE);
    assert!(release.iter().all(|e| e.kind == KeyEventKind::Release));
    // The next Esc is a plain Esc again.
    assert_eq!(
        codes(&ime.key(&esc(ElementState::Pressed), NONE)),
        [(KeyCode::Esc, KeyModifiers::NONE, KeyEventKind::Press)]
    );
}

#[test]
fn an_ime_that_clears_the_preedit_itself_cancels_too() {
    let mut ime = ImeInput::new();
    ime.ime(&Ime::Enabled);
    ime.ime(&preedit("nihao"));
    assert!(ime.ime(&Ime::Preedit(String::new(), None)).is_empty());
    assert!(ime.preedit().is_none());
    assert!(!ime.take_cancel_request(), "nothing to reset");
}

#[test]
fn commit_without_preedit_and_of_several_characters() {
    let mut ime = ImeInput::new();
    ime.ime(&Ime::Enabled);
    assert_eq!(codes(&ime.ime(&Ime::Commit("é".into()))), presses("é"));
    let sent = ime.ime(&Ime::Commit("你好 Việt Nam".into()));
    assert_eq!(codes(&sent), presses("你好 Việt Nam"));
    assert!(sent.iter().all(|e| e.state == KeyEventState::NONE));
    assert!(ime.ime(&Ime::Commit(String::new())).is_empty());
    // Line breaks and tabs become their keys; other control characters are dropped.
    let sent = ime.ime(&Ime::Commit("a\n\u{7}\tb".into()));
    assert_eq!(
        codes(&sent),
        [
            (KeyCode::Char('a'), KeyModifiers::NONE, KeyEventKind::Press),
            (KeyCode::Enter, KeyModifiers::NONE, KeyEventKind::Press),
            (KeyCode::Tab, KeyModifiers::NONE, KeyEventKind::Press),
            (KeyCode::Char('b'), KeyModifiers::NONE, KeyEventKind::Press),
        ]
    );
}

#[test]
fn keys_flow_when_the_ime_is_disabled_or_idle() {
    let mut ime = ImeInput::new();
    assert_eq!(
        codes(&ime.key(&ch("x", Code::KeyX), ModifiersState::CONTROL)),
        [(
            KeyCode::Char('x'),
            KeyModifiers::CONTROL,
            KeyEventKind::Press
        )]
    );
    ime.ime(&Ime::Enabled);
    assert_eq!(codes(&ime.key(&ch("b", Code::KeyB), NONE)), presses("b"));
    ime.ime(&preedit("b"));
    assert!(ime.key(&ch("a", Code::KeyA), NONE).is_empty());
    // Disabling drops a composition in progress; keys flow again.
    ime.ime(&Ime::Disabled);
    assert!(!ime.is_enabled() && ime.preedit().is_none());
    assert_eq!(codes(&ime.key(&ch("a", Code::KeyA), NONE)), presses("a"));
    assert_eq!(
        codes(&ime.key(&esc(ElementState::Pressed), NONE)),
        [(KeyCode::Esc, KeyModifiers::NONE, KeyEventKind::Press)]
    );
    assert!(!ime.take_cancel_request());
}

#[test]
fn preedit_cursor_is_in_display_columns() {
    let p = |text: &str, cursor| PreeditText {
        text: text.to_owned(),
        cursor,
    };
    assert_eq!(
        p("việt", Some((4, 4))).cursor_columns(),
        Some((2, 2)),
        "byte 4 is inside ệ: back to 2"
    );
    assert_eq!(p("việt", Some((5, 5))).cursor_columns(), Some((3, 3)));
    assert_eq!(p("你好", Some((3, 6))).cursor_columns(), Some((2, 4)));
    assert_eq!(p("ab", Some((0, 99))).cursor_columns(), Some((0, 2)));
    assert_eq!(p("ab", None).cursor_columns(), None);
    assert_eq!(
        p("việt", None).at(3, 7),
        Preedit {
            text: "việt".to_owned(),
            col: 3,
            row: 7
        }
    );
}
