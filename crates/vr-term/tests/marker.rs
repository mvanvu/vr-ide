//! The login marker of a session (SEC-S6, SEC-S8): taken out of the output before the emulator,
//! counted once, and only while the child runs.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use vr_term::{Grid, SpawnExtras, TermStatus, Terminal};

const NONCE: &str = "00112233445566778899aabbccddeeff";
const WAIT: Duration = Duration::from_secs(10);

fn marker(nonce: &str) -> Vec<u8> {
    format!("\x1b]7717;vr-ide-ssh;{nonce}\x07").into_bytes()
}

/// The marker as a `printf` argument of the test script.
fn printf_marker(nonce: &str) -> String {
    format!("\\033]7717;vr-ide-ssh;{nonce}\\007")
}

fn spawn_marked(script: &str) -> (Terminal, Arc<Wake>) {
    let wake = Arc::new(Wake::default());
    let term = Terminal::spawn_with(
        opts(script, 60, 8),
        SpawnExtras {
            marker: Some(marker(NONCE)),
            env_remove: Vec::new(),
        },
        Arc::new(|| {}),
    )
    .expect("spawn /bin/sh");
    (term, wake)
}

/// Every row of the screen and of the whole history.
fn all_text(term: &Terminal) -> String {
    let mut text = String::new();
    term.scroll(isize::MAX);
    loop {
        let g: Grid = term.snapshot();
        text.push_str(&screen(&g));
        text.push('\n');
        if g.scroll_offset == 0 {
            break;
        }
        term.scroll(-(g.rows as isize));
    }
    text
}

fn wait_until(what: &str, ok: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !ok() {
        assert!(Instant::now() < deadline, "waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_marker_raises_the_event_and_never_reaches_the_screen_or_the_history() {
    // Split across two writes, then enough lines to push the first ones into the history.
    let whole = printf_marker(NONCE);
    let (a, b) = whole.split_at(20);
    let script = format!(
        "printf 'before{a}'; sleep 0.1; printf '{b}after\\n'; \
         i=0; while [ $i -lt 30 ]; do echo line$i; i=$((i+1)); done; echo DONE; sleep 30"
    );
    let (term, _wake) = spawn_marked(&script);
    wait_until("the login marker", || term.marker_seen());
    wait_until("the output", || all_text(&term).contains("DONE"));
    let text = all_text(&term);
    assert!(text.contains("beforeafter"), "{text}");
    assert!(
        !text.contains("vr-ide-ssh") && !text.contains(NONCE),
        "{text}"
    );
    assert!(!text.contains("7717"), "{text}");
}

#[test]
fn another_nonce_a_short_nonce_and_a_plain_terminal_raise_nothing() {
    let wrong = printf_marker("ffffffffffffffffffffffffffffffff");
    let short = printf_marker(&NONCE[..31]);
    let script = format!("printf '{wrong}{short}'; echo DONE; sleep 30");
    let (term, _wake) = spawn_marked(&script);
    wait_until("the output", || all_text(&term).contains("DONE"));
    assert!(!term.marker_seen());
    // Without a marker to look for, the same bytes are just an unknown OSC to the emulator.
    let (plain, wake) = spawn(&format!(
        "printf '{}'; echo DONE; sleep 30",
        printf_marker(NONCE)
    ));
    wait_for(&plain, &wake, WAIT, |g| has_text(g, "DONE"));
    assert!(!plain.marker_seen());
}

#[test]
fn a_marker_printed_after_the_child_exited_raises_nothing() {
    // The child leaves a background process holding the PTY that prints the marker later.
    let script = format!(
        "(trap '' HUP; sleep 0.4; printf '{}LATE\\n') & exit 7",
        printf_marker(NONCE)
    );
    let (term, _wake) = spawn_marked(&script);
    assert_eq!(term.wait_exit(WAIT), TermStatus::Exited(7));
    // The kernel hangs the PTY up with its session leader, so the late bytes may never arrive;
    // arrived or not, they are no login.
    std::thread::sleep(Duration::from_millis(900));
    assert!(!term.marker_seen());
    assert!(!all_text(&term).contains("vr-ide-ssh"));
}

#[test]
fn an_unfinished_marker_start_is_shown_when_the_output_ends() {
    let script = "printf 'abc\\033]7717;vr-ide-ss'; printf 'x-END\\n'; sleep 30";
    let (term, _wake) = spawn_marked(script);
    // The held bytes reach the emulator in order: an OSC that swallows `x-END` up to its end.
    wait_until("the output", || all_text(&term).contains("abc"));
    assert!(!term.marker_seen());
}

#[test]
fn withheld_variables_do_not_reach_the_child() {
    let wake = Arc::new(Wake::default());
    let mut o = opts(
        "printf 'P=%s H=%s\\n' \"${PATH:+set}\" \"${HOME-unset}\"; sleep 30",
        60,
        8,
    );
    o.env.clear();
    let term = Terminal::spawn_with(
        o,
        SpawnExtras {
            marker: None,
            env_remove: vec!["HOME".into()],
        },
        Arc::new(|| {}),
    )
    .unwrap();
    let _ = wake;
    wait_until("the output", || all_text(&term).contains("P=set H=unset"));
}

/// SEC-S7: the marker holds the session's nonce; debug output of the spawn extras hides it.
#[test]
fn debug_output_of_the_spawn_extras_hides_the_marker() {
    let extras = SpawnExtras {
        marker: Some(marker(NONCE)),
        env_remove: vec!["VR_IDE_X".into()],
    };
    let text = format!("{extras:?}");
    assert!(
        !text.contains(NONCE) && !text.contains("7717") && !text.contains("27, 93"),
        "{text}"
    );
    assert!(
        text.contains("marker") && text.contains("VR_IDE_X"),
        "{text}"
    );
    let none = format!("{:?}", SpawnExtras::default());
    assert!(none.contains("marker: None"), "{none}");
}
