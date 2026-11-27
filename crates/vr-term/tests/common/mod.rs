#![allow(dead_code)]

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use vr_term::{Grid, SpawnOptions, Terminal};

/// Counts `Notify` calls and lets a test sleep until the next one.
#[derive(Default)]
pub struct Wake {
    count: Mutex<usize>,
    cv: Condvar,
}

impl Wake {
    pub fn count(&self) -> usize {
        *self.count.lock().unwrap()
    }

    fn wait(&self, seen: usize, timeout: Duration) {
        let guard = self.count.lock().unwrap();
        let _ = self
            .cv
            .wait_timeout_while(guard, timeout, |c| *c == seen)
            .unwrap();
    }
}

pub fn opts(script: &str, cols: u16, rows: u16) -> SpawnOptions {
    SpawnOptions {
        shell: PathBuf::from("/bin/sh"),
        args: vec![OsString::from("-c"), OsString::from(script)],
        cwd: PathBuf::from("/"),
        env: Vec::new(),
        cols,
        rows,
        scrollback: 1000,
    }
}

pub fn spawn_opts(opts: SpawnOptions) -> (Terminal, Arc<Wake>) {
    let wake = Arc::new(Wake::default());
    let w = wake.clone();
    let term = Terminal::spawn(
        opts,
        Arc::new(move || {
            *w.count.lock().unwrap() += 1;
            w.cv.notify_all();
        }),
    )
    .expect("spawn /bin/sh");
    (term, wake)
}

pub fn spawn(script: &str) -> (Terminal, Arc<Wake>) {
    spawn_opts(opts(script, 80, 24))
}

/// Snapshots after every `Notify` until `pred` holds; panics with the screen after `timeout`.
pub fn wait_for(
    term: &Terminal,
    wake: &Wake,
    timeout: Duration,
    pred: impl Fn(&Grid) -> bool,
) -> Grid {
    let deadline = Instant::now() + timeout;
    loop {
        let seen = wake.count();
        let grid = term.snapshot();
        if pred(&grid) {
            return grid;
        }
        let now = Instant::now();
        if now >= deadline {
            panic!(
                "condition not met in {timeout:?}; screen:\n{}",
                screen(&grid)
            );
        }
        // Notify re-arms on snapshot; the timeout also covers changes that raise no output.
        wake.wait(seen, (deadline - now).min(Duration::from_millis(100)));
    }
}

pub fn screen(grid: &Grid) -> String {
    (0..grid.rows)
        .map(|r| grid.row_text(r))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn has_text(grid: &Grid, needle: &str) -> bool {
    (0..grid.rows).any(|r| grid.row_text(r).contains(needle))
}

pub fn find_row(grid: &Grid, needle: &str) -> Option<usize> {
    (0..grid.rows).find(|&r| grid.row_text(r).contains(needle))
}

/// Pids of live (non-zombie) processes in process group `pgid`, from /proc.
pub fn group_members(pgid: u32) -> Vec<(u32, char)> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return out;
    };
    for entry in dir.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
            continue;
        };
        // Fields after the parenthesised comm: state ppid pgrp ...
        let Some(rest) = stat.rfind(')').map(|i| &stat[i + 2..]) else {
            continue;
        };
        let fields: Vec<&str> = rest.split_whitespace().collect();
        if fields.len() > 2 && fields[2].parse::<u32>().ok() == Some(pgid) {
            out.push((pid, fields[0].chars().next().unwrap_or('?')));
        }
    }
    out
}

/// Waits (bounded) until no process of group `pgid` exists, live or zombie.
pub fn wait_group_gone(pgid: u32, timeout: Duration) -> Vec<(u32, char)> {
    let deadline = Instant::now() + timeout;
    loop {
        let members = group_members(pgid);
        if members.is_empty() || Instant::now() >= deadline {
            return members;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
