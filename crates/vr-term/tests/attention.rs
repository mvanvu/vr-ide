//! The bell and the OSC 9 / 777 notifications of a program reach the UI as events, also when it
//! has not looked at the terminal since the last output.

mod common;

use std::time::{Duration, Instant};

use common::*;
use std::sync::Arc;
use vr_term::{SpawnExtras, Terminal};

fn watched(script: &str, attention: bool) -> (Terminal, Arc<Wake>) {
    let extras = SpawnExtras {
        attention,
        ..SpawnExtras::default()
    };
    spawn_extras(opts(script, 80, 24), extras)
}

fn wait_until(what: &str, mut ok: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ok() {
        assert!(Instant::now() < deadline, "waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn the_bell_and_notifications_are_queued_and_taken_once() {
    let (term, _wake) = watched(
        "printf 'x\\007\\033]9;Allow it?\\007\\033]777;notify;t;b\\033\\\\'; sleep 5",
        true,
    );
    let mut got = Vec::new();
    wait_until("three events", || {
        got.extend(term.take_attention());
        got.len() >= 3
    });
    let texts: Vec<_> = got.into_iter().map(|a| a.text).collect();
    assert_eq!(
        texts,
        vec![
            None,
            Some("Allow it?".to_string()),
            Some("t: b".to_string())
        ]
    );
    assert!(term.take_attention().is_empty());
}

#[test]
fn a_flood_keeps_a_bounded_queue_and_still_wakes_the_ui() {
    // Output the UI never snapshots: the coalesced notify fired once; events must still wake.
    let (term, wake) = watched(
        "sleep 1; i=0; while [ $i -lt 3000 ]; do printf '\\007'; i=$((i+1)); done; sleep 3; printf '\\007'; sleep 5",
        true,
    );
    // The queue is only taken after the whole flood arrived: bounded, not the 3000.
    std::thread::sleep(Duration::from_millis(2000));
    let first = term.take_attention();
    assert!(!first.is_empty() && first.len() <= 64, "{}", first.len());
    let before = wake.count();
    // After the take the next bell (not coalesced with output) wakes again.
    wait_until("a new wake", || wake.count() > before);
    assert_eq!(term.take_attention().len(), 1);
}

#[test]
fn a_terminal_that_is_not_watched_queues_nothing_and_does_not_wake_for_it() {
    let (term, _wake) = watched("printf 'x\\007\\033]9;hi\\007'; sleep 1", false);
    std::thread::sleep(Duration::from_millis(500));
    assert!(term.take_attention().is_empty());
}
