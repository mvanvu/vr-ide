//! Closing a terminal releases its threads and PTY even when a detached grandchild keeps the
//! slave open. Its own test binary (one test) so thread and fd counts are this test's alone.

mod common;

use std::ffi::OsString;
use std::time::{Duration, Instant};

use common::*;
use vr_term::{SpawnOptions, TermStatus};

const T: Duration = Duration::from_secs(10);

fn vr_term_threads() -> usize {
    std::fs::read_dir("/proc/self/task")
        .unwrap()
        .flatten()
        .filter(|t| {
            std::fs::read_to_string(t.path().join("comm"))
                .is_ok_and(|comm| comm.starts_with("vr-term-"))
        })
        .count()
}

fn open_fds() -> usize {
    std::fs::read_dir("/proc/self/fd").unwrap().count()
}

/// Runs a program that leaves a grandchild (printed as `gc:<pid>`) holding the PTY slave after
/// the session leader exits, drops the terminal and returns what was still left 2 s later.
fn left_after_drop(o: SpawnOptions) -> (Duration, usize, usize) {
    let (threads_before, fds_before) = (vr_term_threads(), open_fds());
    let (term, wake) = spawn_opts(o);
    let grid = wait_for(&term, &wake, T, |g| has_text(g, "gc:"));
    let row = grid.row_text(find_row(&grid, "gc:").unwrap());
    let grandchild: u32 = row.trim_start_matches("gc:").trim().parse().unwrap();
    assert_eq!(term.wait_exit(T), TermStatus::Exited(0));

    let t0 = Instant::now();
    drop(term);
    let took = t0.elapsed();

    let deadline = Instant::now() + Duration::from_secs(2);
    let (mut threads, mut fds) = (vr_term_threads(), open_fds());
    while (threads, fds) != (threads_before, fds_before) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
        (threads, fds) = (vr_term_threads(), open_fds());
    }
    // The grandchild is ours; stop it by pid before any assert can fail.
    let _ = std::process::Command::new("kill")
        .args(["-KILL", &grandchild.to_string()])
        .status();
    (took, threads - threads_before, fds - fds_before)
}

#[test]
fn drop_releases_threads_and_the_pty_while_a_detached_grandchild_holds_the_slave() {
    // A shell job moved to its own session: the shell's exit hangs the PTY up.
    let (took, threads, fds) = left_after_drop(opts(r#"setsid sleep 30 & echo "gc:$!""#, 80, 24));
    assert!(
        took < Duration::from_secs(1),
        "setsid job: drop took {took:?}"
    );
    assert_eq!((threads, fds), (0, 0), "setsid job: threads / fds left");

    // A daemon-style leader that gives up its controlling tty (TIOCNOTTY) before exiting, so
    // the kernel never hangs the PTY up; only closing every master fd releases it.
    let script = r#"
import fcntl, os, signal, termios
signal.signal(signal.SIGHUP, signal.SIG_IGN)
pid = os.fork()
if pid == 0:
    os.setsid()
    os.execvp("sleep", ["sleep", "30"])
fcntl.ioctl(0, termios.TIOCNOTTY)
print("gc:%d" % pid, flush=True)
"#;
    let daemon = SpawnOptions {
        shell: "python3".into(),
        args: vec![
            OsString::from("-I"),
            OsString::from("-c"),
            OsString::from(script),
        ],
        ..opts("", 80, 24)
    };
    let (took, threads, fds) = left_after_drop(daemon.clone());
    assert!(took < Duration::from_secs(1), "daemon: drop took {took:?}");
    assert_eq!((threads, fds), (0, 0), "daemon: threads / fds left");

    // The same daemon writing faster than the emulator parses: the reader never sees EAGAIN,
    // so it must notice the close between reads.
    let flooding = script.replace(r#"["sleep", "30"]"#, r#"["yes", "flood"]"#);
    let flooder = SpawnOptions {
        args: vec![
            OsString::from("-I"),
            OsString::from("-c"),
            OsString::from(flooding),
        ],
        ..daemon
    };
    let (took, threads, fds) = left_after_drop(flooder);
    assert!(took < Duration::from_secs(1), "flooder: drop took {took:?}");
    assert_eq!((threads, fds), (0, 0), "flooder: threads / fds left");
}
