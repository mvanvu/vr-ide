//! Variables describing the outer terminal do not reach the shell. Its own test binary because it
//! changes this process's environment.

mod common;

use std::ffi::OsString;
use std::time::Duration;

use common::*;

const OUTER: &[&str] = &[
    "TMUX",
    "TMUX_PANE",
    "STY",
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "TERM_SESSION_ID",
    "VTE_VERSION",
    "COLUMNS",
    "LINES",
    "WINDOWID",
    "KITTY_WINDOW_ID",
    "WEZTERM_PANE",
    "ALACRITTY_WINDOW_ID",
    "KONSOLE_VERSION",
    "GNOME_TERMINAL_SCREEN",
    "WT_SESSION",
    "ITERM_SESSION_ID",
    "LC_TERMINAL",
];

#[test]
fn outer_terminal_variables_are_removed_but_explicit_ones_win() {
    for key in OUTER {
        // SAFETY: the only test in this binary; no other thread reads the environment now.
        unsafe { std::env::set_var(key, "outer") };
    }
    let script = OUTER
        .iter()
        .map(|k| format!("printf '{k}=%s\\n' \"${{{k}-unset}}\""))
        .collect::<Vec<_>>()
        .join("; ")
        + "; echo ENV-DONE";
    let mut o = opts(&script, 80, 40);
    o.env
        .push((OsString::from("TERM_PROGRAM"), OsString::from("mine")));
    let (term, wake) = spawn_opts(o);
    let grid = wait_for(&term, &wake, Duration::from_secs(10), |g| {
        has_text(g, "ENV-DONE")
    });
    for key in OUTER {
        let want = if *key == "TERM_PROGRAM" {
            format!("{key}=mine")
        } else {
            format!("{key}=unset")
        };
        assert!(
            has_text(&grid, &want),
            "want {want}; screen:\n{}",
            screen(&grid)
        );
    }
}
