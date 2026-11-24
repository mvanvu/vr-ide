//! AC-14c: 100 MB of output streams through without unbounded memory, and snapshots stay fast
//! while it does. Its own test binary so the RSS figures are this test's alone.
//!
//! Runner assumption (BASE-42): wall-clock limits depend on the host. The default test keeps
//! only limits a loaded shared runner meets (memory, a loose p99); the strict snapshot budget
//! is the `#[ignore]` test, run by QC / bench on a quiet host with
//! `cargo test -p vr-term --release --test flood -- --ignored`.

mod common;

use std::time::{Duration, Instant};

use common::*;

const BYTES: u64 = 100 * 1024 * 1024;

fn rss_kb(field: &str) -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    status
        .lines()
        .find(|l| l.starts_with(field))
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|v| v.parse().ok())
        .unwrap()
}

struct Stats {
    p99: Duration,
    max: Duration,
    rss_growth_kb: u64,
}

fn flood() -> Stats {
    let mut o = opts(
        "yes | head -c 104857600; printf '\\nFLOOD-END\\n'; read x",
        120,
        40,
    );
    o.scrollback = 10_000;
    let rss_before = rss_kb("VmRSS:");
    let (term, _wake) = spawn_opts(o);

    let t0 = Instant::now();
    let mut times = Vec::new();
    let deadline = t0 + Duration::from_secs(300);
    loop {
        let s = Instant::now();
        let grid = term.snapshot(0);
        times.push(s.elapsed());
        if has_text(&grid, "FLOOD-END") {
            break;
        }
        assert!(Instant::now() < deadline, "flood did not finish");
        // A UI redraws at most every few ms; sample at that rate.
        std::thread::sleep(Duration::from_millis(2));
    }
    let elapsed = t0.elapsed();
    let rss_peak = rss_kb("VmHWM:");
    assert_eq!(term.snapshot(0).scrollback_len, 10_000);

    times.sort();
    let max = *times.last().unwrap();
    let p99 = times[times.len() * 99 / 100];
    let median = times[times.len() / 2];
    let mbps = BYTES as f64 / 1024.0 / 1024.0 / elapsed.as_secs_f64();
    eprintln!(
        "flood: {BYTES} bytes in {elapsed:?} = {mbps:.1} MB/s; {} snapshots: median {median:?}, p99 {p99:?}, max {max:?}; RSS before {rss_before} kB, peak {rss_peak} kB",
        times.len()
    );
    Stats {
        p99,
        max,
        rss_growth_kb: rss_peak - rss_before,
    }
}

#[test]
fn hundred_megabytes_of_output_keeps_memory_bounded_and_snapshots_responsive() {
    let stats = flood();
    // 10 000 lines x 120 cols of 24-byte cells is ~29 MB; allow the emulator's slack on top.
    assert!(
        stats.rss_growth_kb < 96 * 1024,
        "memory grew by {} kB",
        stats.rss_growth_kb
    );
    // Far above the 5 ms budget, far below a frozen UI (AC-14c: keys within 100 ms).
    assert!(
        stats.p99 < Duration::from_millis(50),
        "p99 snapshot {:?}",
        stats.p99
    );
}

#[test]
#[ignore = "strict timing: run with --release on a quiet host (QC / bench)"]
fn hundred_megabytes_of_output_strict_snapshot_budget() {
    let stats = flood();
    assert!(
        stats.p99 < Duration::from_millis(5),
        "p99 snapshot {:?}",
        stats.p99
    );
    assert!(
        stats.max < Duration::from_millis(20),
        "max snapshot {:?}",
        stats.max
    );
}
