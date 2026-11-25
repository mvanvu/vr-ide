//! PTY behaviour with real `/bin/sh -c` children.

mod common;

use std::ffi::OsString;
use std::time::{Duration, Instant};

use common::*;
use vr_term::{
    Color, CursorShape, KILL_GRACE, KeyCode, KeyEvent, MAX_PENDING_INPUT, Modes, SpawnOptions,
    TermError, TermStatus, Terminal, encode_key,
};

const T: Duration = Duration::from_secs(10);

#[test]
fn sgr_colours_reach_the_cells() {
    let (term, wake) = spawn(
        r"printf '\033[31mR\033[0m\033[1;38;5;200mI\033[0m\033[48;2;1;2;3mT\033[0m\033[3;4;7;2mS\033[0m\n'",
    );
    let grid = wait_for(&term, &wake, T, |g| has_text(g, "RITS"));
    let row = find_row(&grid, "RITS").unwrap();
    let col = grid.row_text(row).find("RITS").unwrap();
    let r = grid.cell(row, col).unwrap();
    assert_eq!(
        (r.ch.as_str(), r.fg, r.bg),
        ("R", Color::Indexed(1), Color::Default)
    );
    let i = grid.cell(row, col + 1).unwrap();
    assert_eq!((i.fg, i.flags.bold), (Color::Indexed(200), true));
    let t = grid.cell(row, col + 2).unwrap();
    assert_eq!((t.fg, t.bg), (Color::Default, Color::Rgb(1, 2, 3)));
    let s = grid.cell(row, col + 3).unwrap();
    assert!(
        s.flags.italic && s.flags.underline && s.flags.inverse && s.flags.dim,
        "{s:?}"
    );
}

#[test]
fn shell_sees_term_colorterm_cwd_and_extra_env() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().canonicalize().unwrap();
    let mut o = opts(
        r#"printf '%s|%s|%s|%s\n' "$TERM" "$COLORTERM" "$(pwd -P)" "$VR_X""#,
        200,
        10,
    );
    o.cwd = dir.path().to_path_buf();
    o.env
        .push((OsString::from("COLORTERM"), OsString::from("truecolor")));
    o.env
        .push((OsString::from("VR_X"), OsString::from("extra")));
    let (term, wake) = spawn_opts(o);
    let want = format!("xterm-256color|truecolor|{}|extra", real.display());
    wait_for(&term, &wake, T, |g| has_text(g, &want));
}

#[test]
fn alt_screen_sets_the_mode_flag() {
    let (term, wake) = spawn(r"printf 'main\n'; read x; printf '\033[?1049hALT'; read y");
    let grid = wait_for(&term, &wake, T, |g| has_text(g, "main"));
    assert!(!grid.modes.alt_screen);
    term.write(b"\r").unwrap();
    let grid = wait_for(&term, &wake, T, |g| {
        g.modes.alt_screen && has_text(g, "ALT")
    });
    assert!(!has_text(&grid, "main"), "alternate screen starts blank");
    assert_eq!(grid.scrollback_len, 0);
}

#[test]
fn wide_chars_and_vietnamese_diacritics_land_in_cells() {
    let mut o = opts(r#"printf '%s\n' "$VR_TEXT""#, 80, 10);
    // Precomposed Vietnamese, a CJK wide char, and `e` + combining acute (U+0301).
    o.env.push((
        OsString::from("VR_TEXT"),
        OsString::from("中a Tiếng Việt e\u{301}!"),
    ));
    let (term, wake) = spawn_opts(o);
    let grid = wait_for(&term, &wake, T, |g| has_text(g, "Việt"));
    let row = find_row(&grid, "Việt").unwrap();
    let wide = grid.cell(row, 0).unwrap();
    assert_eq!(wide.ch, "中");
    assert!(wide.flags.wide);
    let spacer = grid.cell(row, 1).unwrap();
    assert!(spacer.flags.wide_spacer);
    assert_eq!(spacer.ch, "");
    assert_eq!(grid.cell(row, 2).unwrap().ch, "a");
    assert_eq!(grid.row_text(row), "中a Tiếng Việt e\u{301}!");
    // `ế` is one cell; the combining mark joins `e` in one cell.
    assert_eq!(grid.cell(row, 6).unwrap().ch, "ế");
    assert_eq!(grid.cell(row, 15).unwrap().ch, "e\u{301}");
    assert_eq!(grid.cell(row, 16).unwrap().ch, "!");
}

#[test]
fn title_cursor_shape_and_visibility() {
    let (term, wake) =
        spawn(r"printf '\033]0;my title\007\033[6 qX'; read x; printf '\033[?25l'; read y");
    let grid = wait_for(&term, &wake, T, |g| g.title.as_deref() == Some("my title"));
    let grid = if has_text(&grid, "X") {
        grid
    } else {
        wait_for(&term, &wake, T, |g| has_text(g, "X"))
    };
    assert_eq!(grid.cursor.shape, CursorShape::Beam);
    assert!(grid.cursor.visible);
    assert_eq!((grid.cursor.row, grid.cursor.col), (0, 1));
    term.write(b"\r").unwrap();
    wait_for(&term, &wake, T, |g| !g.cursor.visible);
}

#[test]
fn typed_input_reaches_the_program() {
    let (term, wake) = spawn(r#"read line; printf 'got:%s\n' "$line"; read z"#);
    for c in "hi ệ".chars() {
        term.write(&encode_key(
            KeyEvent::plain(KeyCode::Char(c)),
            Modes::default(),
        ))
        .unwrap();
    }
    term.write(&encode_key(
        KeyEvent::plain(KeyCode::Enter),
        Modes::default(),
    ))
    .unwrap();
    wait_for(&term, &wake, T, |g| has_text(g, "got:hi ệ"));
}

#[test]
fn emulator_replies_to_cursor_position_queries() {
    // The program asks where the cursor is and reads the reply from its input.
    let (term, wake) = spawn(
        r#"stty raw -echo; printf '\033[6n'; r=$(dd bs=1 count=6 2>/dev/null); stty sane; printf '\nREPLY:%s\n' "$r" | tr '\033' E; read z"#,
    );
    wait_for(&term, &wake, T, |g| has_text(g, "REPLY:E[1;1R"));
}

#[test]
fn scroll_offset_shows_history_and_scrollback_is_capped() {
    let mut o = opts("seq 1 200; read x", 20, 10);
    o.scrollback = 50;
    let (term, wake) = spawn_opts(o);
    let grid = wait_for(&term, &wake, T, |g| has_text(g, "200"));
    assert_eq!(
        grid.scrollback_len, 50,
        "history capped at the scrollback setting"
    );
    // Screen: 192..=200 and the cursor line.
    assert_eq!(grid.row_text(0), "192");
    let up = term.snapshot(5);
    assert_eq!((up.scroll_offset, up.row_text(0).as_str()), (5, "187"));
    assert!(!up.cursor.visible || up.cursor.row < up.rows);
    let top = term.snapshot(10_000);
    assert_eq!((top.scroll_offset, top.row_text(0).as_str()), (50, "142"));
    assert!(!top.cursor.visible, "cursor line is scrolled out of view");
}

#[test]
fn spawn_errors_are_reported() {
    let mut bad_shell = opts("true", 80, 24);
    bad_shell.shell = "/nonexistent/vr-term-shell".into();
    let err = Terminal::spawn(bad_shell, std::sync::Arc::new(|| {})).unwrap_err();
    assert!(matches!(err, TermError::Spawn { .. }), "{err:?}");

    let bad_cwd = SpawnOptions {
        cwd: "/nonexistent/dir".into(),
        ..opts("true", 80, 24)
    };
    let err = Terminal::spawn(bad_cwd, std::sync::Arc::new(|| {})).unwrap_err();
    assert!(matches!(err, TermError::BadCwd(_)), "{err:?}");

    let zero = opts("true", 0, 24);
    let err = Terminal::spawn(zero, std::sync::Arc::new(|| {})).unwrap_err();
    assert!(matches!(err, TermError::BadSize { .. }), "{err:?}");
}

#[test]
fn resize_reaches_the_program_and_the_grid() {
    let (term, wake) = spawn("stty size; read x; stty size; read y");
    wait_for(&term, &wake, T, |g| has_text(g, "24 80"));
    term.resize(100, 30).unwrap();
    assert_eq!(term.size(), (100, 30));
    term.write(b"\r").unwrap();
    let grid = wait_for(&term, &wake, T, |g| has_text(g, "30 100"));
    assert_eq!((grid.cols, grid.rows, grid.cells.len()), (100, 30, 3000));
    // Tiny sizes are fine; zero is refused.
    term.resize(1, 1).unwrap();
    assert_eq!((term.snapshot(0).cols, term.snapshot(0).rows), (1, 1));
    assert!(matches!(term.resize(0, 5), Err(TermError::BadSize { .. })));
}

#[test]
fn exit_code_is_reported() {
    let (term, _wake) = spawn("exit 3");
    assert_eq!(term.wait_exit(T), TermStatus::Exited(3));
    assert_eq!(term.status(), TermStatus::Exited(3));
    let (term, _wake) = spawn("kill -TERM $$");
    assert_eq!(term.wait_exit(T), TermStatus::Exited(128 + 15));
}

#[test]
fn exit_notifies_and_notifications_are_coalesced_until_a_snapshot() {
    let (term, wake) = spawn("seq 1 20000; exit 0");
    assert_eq!(term.wait_exit(T), TermStatus::Exited(0));
    // Let the reader drain the last output.
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(wake.count(), 1, "one Notify until the UI takes a snapshot");
    term.snapshot(0);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(wake.count(), 1, "nothing new after the snapshot");
}

#[test]
fn kill_hangs_up_the_whole_group_and_reaps() {
    let (term, wake) = spawn("sleep 100 & echo started; wait");
    wait_for(&term, &wake, T, |g| has_text(g, "started"));
    let pid = term.pid().unwrap();
    assert!(
        group_members(pid).len() >= 2,
        "sh and sleep run in the child's group"
    );
    term.kill();
    // SIGHUP is what sh dies of.
    assert_eq!(term.status(), TermStatus::Exited(128 + 1));
    assert_eq!(
        wait_group_gone(pid, T),
        vec![],
        "no process or zombie left in the group"
    );
    // A second kill is a no-op.
    term.kill();
}

#[test]
fn drop_kills_reaps_and_returns_promptly() {
    let (term, wake) = spawn("sleep 100 & echo started; wait");
    wait_for(&term, &wake, T, |g| has_text(g, "started"));
    let pid = term.pid().unwrap();
    let t0 = Instant::now();
    drop(term);
    let took = t0.elapsed();
    assert!(took < Duration::from_secs(1), "drop took {took:?}");
    assert_eq!(
        wait_group_gone(pid, T),
        vec![],
        "no process or zombie left in the group"
    );
    assert!(
        !std::path::Path::new(&format!("/proc/{pid}")).exists(),
        "child reaped"
    );
}

#[test]
fn a_child_ignoring_sighup_is_killed_after_the_grace_period() {
    let (term, wake) = spawn("trap '' HUP; echo ignoring; while :; do sleep 0.1; done");
    wait_for(&term, &wake, T, |g| has_text(g, "ignoring"));
    let pid = term.pid().unwrap();
    let t0 = Instant::now();
    drop(term);
    let took = t0.elapsed();
    assert!(took >= KILL_GRACE, "SIGHUP got its grace period ({took:?})");
    assert!(
        took < KILL_GRACE * 2 + Duration::from_millis(500),
        "bounded drop ({took:?})"
    );
    assert_eq!(
        wait_group_gone(pid, T),
        vec![],
        "SIGKILL reached the whole group"
    );
}

#[test]
fn replies_to_queries_from_a_program_that_never_reads_stay_under_the_input_cap() {
    // 750 000 device-attribute queries ask for ~3.7 MB of replies; the program never reads.
    // Raw mode, as full-screen programs use: the tty's input buffer then fills and holds the
    // replies back (canonical mode would discard them), and no echo hides the marker.
    let (term, wake) = spawn(
        r#"stty raw -echo; yes "$(printf '\033[c')" | head -c 3000000; printf 'QUERIES-DONE\r\n'; sleep 100"#,
    );
    wait_for(&term, &wake, Duration::from_secs(120), |g| {
        has_text(g, "QUERIES-DONE")
    });
    let pending = term.pending_input();
    assert!(
        pending <= MAX_PENDING_INPUT,
        "{pending} bytes of replies queued (cap {MAX_PENDING_INPUT})"
    );
}

#[test]
fn input_to_a_program_that_never_reads_is_bounded_and_never_blocks() {
    let (term, wake) = spawn("echo sleeping; sleep 100");
    wait_for(&term, &wake, T, |g| has_text(g, "sleeping"));
    let chunk = vec![b'x'; 64 * 1024];
    let t0 = Instant::now();
    let mut refused = None;
    for i in 0..64 {
        if let Err(e) = term.write(&chunk) {
            refused = Some((i, e));
            break;
        }
    }
    assert!(t0.elapsed() < Duration::from_secs(1), "write never blocks");
    let (i, err) = refused.expect("backlog refused");
    assert!(matches!(err, TermError::InputBacklog(_)), "{err:?}");
    // The cap counts bytes still queued; the writer drains some into the tty buffer meanwhile,
    // so the accepted total may exceed it. What must hold is the queue itself.
    let pending = term.pending_input();
    assert!(
        pending <= MAX_PENDING_INPUT,
        "{pending} bytes queued after refusal at chunk {i} (cap {MAX_PENDING_INPUT})"
    );
    // The emulator stays usable.
    let t0 = Instant::now();
    term.snapshot(0);
    assert!(t0.elapsed() < Duration::from_millis(100));
}

#[test]
fn writing_after_exit_fails_cleanly() {
    let (term, _wake) = spawn("exit 0");
    assert_eq!(term.wait_exit(T), TermStatus::Exited(0));
    // The writer thread ends once the pty is gone; at most a few writes are accepted first.
    let mut err = None;
    for _ in 0..50 {
        if let Err(e) = term.write(b"x") {
            err = Some(e);
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(matches!(err, Some(TermError::Closed)), "{err:?}");
}

#[test]
fn terminal_can_move_to_another_thread() {
    fn assert_send<T: Send>() {}
    assert_send::<Terminal>();
}

#[test]
fn shell_in_foreground_until_a_job_takes_the_terminal() {
    // The shell itself waits for input: it is the terminal's foreground process group.
    let (term, _wake) = spawn("read x");
    let deadline = Instant::now() + T;
    while !term.shell_in_foreground() {
        assert!(
            Instant::now() < deadline,
            "the shell never held the terminal"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    // With job control a program started from it gets the terminal (vim, cat, an agent).
    let (term, _wake) = spawn("set -m; cat; exit 0");
    let deadline = Instant::now() + T;
    while term.shell_in_foreground() {
        assert!(Instant::now() < deadline, "cat never took the terminal");
        std::thread::sleep(Duration::from_millis(10));
    }
}
