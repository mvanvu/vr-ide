//! Resource monitor behind the welcome screen: system CPU, memory, load, uptime and this
//! process's own CPU and RSS, read from Linux `/proc`.
//!
//! There is no thread and no timer: [`Monitor::tick`] runs from the event loop and reads the
//! (tiny) `/proc` files at most once a second, and only while the welcome screen is visible.
//! Every field is optional; a file or line that is missing or unreadable leaves it `None`
//! (drawn as "—") and never panics.

use std::collections::VecDeque;
use std::path::Path;
use std::time::{Duration, Instant};

/// Samples kept for the CPU history graphs (about a minute).
pub const HISTORY: usize = 60;

/// Time between two samples.
pub const INTERVAL: Duration = Duration::from_secs(1);

/// Jiffies of one CPU line of `/proc/stat`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CpuTimes {
    pub busy: u64,
    pub total: u64,
}

/// `/proc/stat`: the total line and one entry per core.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatSnapshot {
    pub total: CpuTimes,
    pub cores: Vec<CpuTimes>,
}

/// `/proc/meminfo` fields in KiB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MemInfo {
    pub total: Option<u64>,
    pub available: Option<u64>,
    pub swap_total: Option<u64>,
    pub swap_free: Option<u64>,
}

impl MemInfo {
    pub fn used(&self) -> Option<u64> {
        Some(self.total?.saturating_sub(self.available?))
    }

    pub fn swap_used(&self) -> Option<u64> {
        Some(self.swap_total?.saturating_sub(self.swap_free?))
    }
}

fn cpu_times(fields: &str) -> Option<CpuTimes> {
    // user nice system idle iowait irq softirq steal (guest time is already in user).
    let v: Vec<u64> = fields
        .split_whitespace()
        .take(8)
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    if v.len() < 4 {
        return None;
    }
    let total: u64 = v.iter().sum();
    let idle = v[3] + v.get(4).copied().unwrap_or(0);
    Some(CpuTimes {
        busy: total.saturating_sub(idle),
        total,
    })
}

pub fn parse_stat(text: &str) -> Option<StatSnapshot> {
    let mut total = None;
    let mut cores = Vec::new();
    for line in text.lines() {
        let Some((name, rest)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        if name == "cpu" {
            total = cpu_times(rest);
        } else if let Some(n) = name.strip_prefix("cpu")
            && n.chars().all(|c| c.is_ascii_digit())
            && !n.is_empty()
        {
            // A core line that does not parse keeps its place so indexes stay right.
            cores.push(cpu_times(rest).unwrap_or_default());
        }
    }
    Some(StatSnapshot {
        total: total?,
        cores,
    })
}

/// Busy share between two snapshots of the same CPU, 0..=100; `None` when no time passed.
pub fn cpu_percent(prev: CpuTimes, now: CpuTimes) -> Option<f32> {
    let total = now.total.checked_sub(prev.total).filter(|&t| t > 0)?;
    let busy = now.busy.saturating_sub(prev.busy).min(total);
    Some(busy as f32 * 100.0 / total as f32)
}

pub fn parse_meminfo(text: &str) -> MemInfo {
    let mut m = MemInfo::default();
    for line in text.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let value = rest
            .split_whitespace()
            .next()
            .and_then(|v| v.parse::<u64>().ok());
        match key {
            "MemTotal" => m.total = value,
            "MemAvailable" => m.available = value,
            "SwapTotal" => m.swap_total = value,
            "SwapFree" => m.swap_free = value,
            _ => {}
        }
    }
    m
}

pub fn parse_loadavg(text: &str) -> Option<[f32; 3]> {
    let mut it = text.split_whitespace().map(str::parse::<f32>);
    Some([it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?])
}

/// Whole seconds since boot.
pub fn parse_uptime(text: &str) -> Option<u64> {
    let secs: f64 = text.split_whitespace().next()?.parse().ok()?;
    (secs.is_finite() && secs >= 0.0).then_some(secs as u64)
}

/// utime + stime of `/proc/self/stat` in jiffies. The command name is in parentheses and may
/// hold spaces and parentheses, so the fields are counted from the last `)`.
pub fn parse_self_stat(text: &str) -> Option<u64> {
    let rest = &text[text.rfind(')')? + 1..];
    // After the name: state is field 3, utime 14 and stime 15 (1-based).
    let mut f = rest.split_whitespace().skip(11);
    let utime: u64 = f.next()?.parse().ok()?;
    let stime: u64 = f.next()?.parse().ok()?;
    Some(utime + stime)
}

/// `VmRSS` of `/proc/self/status` in KiB.
pub fn parse_self_rss(text: &str) -> Option<u64> {
    text.lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// Where the numbers come from: `/proc` in the product, canned text in tests.
pub trait ProcSource {
    /// The content of `/proc/<name>` (for example `stat` or `self/status`).
    fn read(&self, name: &str) -> Option<String>;
}

pub struct ProcFs;

impl ProcSource for ProcFs {
    fn read(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(Path::new("/proc").join(name)).ok()
    }
}

/// A bounded history, oldest first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ring {
    samples: VecDeque<f32>,
}

impl Ring {
    pub fn push(&mut self, v: f32) {
        if self.samples.len() == HISTORY {
            self.samples.pop_front();
        }
        self.samples.push_back(v);
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = f32> + '_ {
        self.samples.iter().copied()
    }
}

/// The latest numbers; `None` is "—".
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Reading {
    pub cpu: Option<f32>,
    pub cores: Vec<f32>,
    pub mem: MemInfo,
    pub load: Option<[f32; 3]>,
    pub uptime: Option<u64>,
    pub own_cpu: Option<f32>,
    pub own_rss: Option<u64>,
}

pub struct Monitor {
    source: Box<dyn ProcSource>,
    supported: bool,
    /// A baseline exists, i.e. sampling is on (welcome visible).
    running: bool,
    last: Option<Instant>,
    prev_stat: Option<StatSnapshot>,
    prev_self: Option<u64>,
    pub reading: Reading,
    pub cpu_history: Ring,
    pub own_history: Ring,
    /// Git branch of the project, refreshed with each sample.
    pub branch: Option<String>,
    /// How many samples were taken (tests assert that none are taken while hidden).
    pub samples_taken: u64,
}

impl Default for Monitor {
    fn default() -> Self {
        Monitor::new(Box::new(ProcFs), cfg!(target_os = "linux"))
    }
}

impl Monitor {
    pub fn new(source: Box<dyn ProcSource>, supported: bool) -> Monitor {
        Monitor {
            source,
            supported,
            running: false,
            last: None,
            prev_stat: None,
            prev_self: None,
            reading: Reading::default(),
            cpu_history: Ring::default(),
            own_history: Ring::default(),
            branch: None,
            samples_taken: 0,
        }
    }

    /// A monitor that shows `reading` and never samples (snapshot tests).
    pub fn fixed(reading: Reading, cpu: &[f32], own: &[f32]) -> Monitor {
        let mut m = Monitor::new(Box::new(ProcFs), true);
        m.reading = reading;
        for &v in cpu {
            m.cpu_history.push(v);
        }
        for &v in own {
            m.own_history.push(v);
        }
        m
    }

    /// False off Linux: the welcome screen shows "monitor: Linux only".
    pub fn supported(&self) -> bool {
        self.supported
    }

    pub fn running(&self) -> bool {
        self.running
    }

    /// Samples when `visible` and a second passed since the last sample. Returns whether a new
    /// reading arrived (the screen needs a redraw). Hiding stops sampling and drops the
    /// baseline, so a later show never computes a delta over the time it was hidden.
    pub fn tick(&mut self, now: Instant, visible: bool) -> bool {
        if !self.supported {
            return false;
        }
        if !visible {
            if self.running {
                self.running = false;
                self.last = None;
                self.prev_stat = None;
                self.prev_self = None;
            }
            return false;
        }
        if self
            .last
            .is_some_and(|t| now.saturating_duration_since(t) < INTERVAL)
        {
            return false;
        }
        self.running = true;
        self.last = Some(now);
        self.sample();
        true
    }

    fn sample(&mut self) {
        self.samples_taken += 1;
        let stat = self.source.read("stat").and_then(|t| parse_stat(&t));
        let own = self
            .source
            .read("self/stat")
            .and_then(|t| parse_self_stat(&t));
        let r = &mut self.reading;
        r.cpu = None;
        r.cores.clear();
        r.own_cpu = None;
        if let (Some(prev), Some(now)) = (&self.prev_stat, &stat) {
            r.cpu = cpu_percent(prev.total, now.total);
            r.cores = now
                .cores
                .iter()
                .zip(&prev.cores)
                .map(|(n, p)| cpu_percent(*p, *n).unwrap_or(0.0))
                .collect();
            // Own jiffies against one core's share of the system jiffies in the same window.
            let window = now.total.total.saturating_sub(prev.total.total);
            if let (Some(a), Some(b)) = (self.prev_self, own)
                && window > 0
            {
                let one_core = window as f32 / now.cores.len().max(1) as f32;
                r.own_cpu = Some((b.saturating_sub(a) as f32 * 100.0 / one_core).max(0.0));
            }
        }
        if let Some(v) = r.cpu {
            self.cpu_history.push(v);
        }
        if let Some(v) = r.own_cpu {
            self.own_history.push(v);
        }
        self.prev_stat = stat;
        self.prev_self = own;
        r.mem = self
            .source
            .read("meminfo")
            .map(|t| parse_meminfo(&t))
            .unwrap_or_default();
        r.load = self.source.read("loadavg").and_then(|t| parse_loadavg(&t));
        r.uptime = self.source.read("uptime").and_then(|t| parse_uptime(&t));
        r.own_rss = self
            .source
            .read("self/status")
            .and_then(|t| parse_self_rss(&t));
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    use super::*;

    const STAT: &str = "cpu  100 0 100 700 100 0 0 0 0 0\n\
cpu0 50 0 50 350 50 0 0 0 0 0\n\
cpu1 50 0 50 350 50 0 0 0 0 0\n\
intr 1\n";
    const STAT2: &str = "cpu  200 0 100 800 100 0 0 0 0 0\n\
cpu0 100 0 50 400 50 0 0 0 0 0\n\
cpu1 100 0 50 400 50 0 0 0 0 0\n";
    const MEMINFO: &str = "MemTotal:       16000000 kB\nMemFree: 1 kB\nMemAvailable:    6000000 kB\nSwapTotal:       2000000 kB\nSwapFree:        1500000 kB\n";
    // The command name holds a space and a parenthesis.
    const SELF_STAT: &str = "42 (vr (ide) x) S 1 2 3 4 5 6 7 8 9 10 30 20 0 0 20 0 1 0 5 100 200\n";

    #[test]
    fn stat_parses_total_and_cores() {
        let s = parse_stat(STAT).unwrap();
        assert_eq!(s.cores.len(), 2);
        assert_eq!(
            s.total,
            CpuTimes {
                busy: 200,
                total: 1000
            }
        );
        assert_eq!(
            s.cores[1],
            CpuTimes {
                busy: 100,
                total: 500
            }
        );
    }

    #[test]
    fn stat_without_total_line_is_none_and_a_bad_core_keeps_its_slot() {
        assert!(parse_stat("cpu0 1 2 3 4\n").is_none());
        assert!(parse_stat("").is_none());
        let s = parse_stat("cpu 1 2 3 4\ncpu0 x y\ncpu1 1 1 1 1\n").unwrap();
        assert_eq!(s.cores.len(), 2);
        assert_eq!(s.cores[0], CpuTimes::default());
    }

    #[test]
    fn cpu_percent_from_two_snapshots() {
        let a = parse_stat(STAT).unwrap();
        let b = parse_stat(STAT2).unwrap();
        // total +200 jiffies, idle +100, busy +100 -> 50 %.
        assert_eq!(cpu_percent(a.total, b.total), Some(50.0));
        assert_eq!(cpu_percent(a.cores[0], b.cores[0]), Some(50.0));
        assert_eq!(cpu_percent(a.total, a.total), None);
        // Counters that went backwards (a reset) give nothing rather than a wrap.
        assert_eq!(cpu_percent(b.total, a.total), None);
    }

    #[test]
    fn meminfo_parses_and_degrades() {
        let m = parse_meminfo(MEMINFO);
        assert_eq!(m.used(), Some(10_000_000));
        assert_eq!(m.swap_used(), Some(500_000));
        let partial = parse_meminfo("MemTotal: 5 kB\nSwapTotal: bad\n");
        assert_eq!(partial.used(), None);
        assert_eq!(partial.swap_used(), None);
        assert_eq!(parse_meminfo("garbage"), MemInfo::default());
    }

    #[test]
    fn load_uptime_and_self_parse() {
        assert_eq!(
            parse_loadavg("1.42 1.18 0.97 2/700 1234\n"),
            Some([1.42, 1.18, 0.97])
        );
        assert_eq!(parse_loadavg("1.0 2.0"), None);
        assert_eq!(parse_loadavg(""), None);
        assert_eq!(parse_uptime("356400.55 1000.0\n"), Some(356_400));
        assert_eq!(parse_uptime("-1 2"), None);
        assert_eq!(parse_uptime("nan 2"), None);
        assert_eq!(parse_self_stat(SELF_STAT), Some(50));
        assert_eq!(parse_self_stat("1 (a) S 1 2"), None);
        assert_eq!(parse_self_stat("no parenthesis"), None);
        assert_eq!(
            parse_self_rss("Name:\tx\nVmRSS:\t   49152 kB\n"),
            Some(49152)
        );
        assert_eq!(parse_self_rss("Name:\tx\n"), None);
    }

    #[test]
    fn ring_is_bounded() {
        let mut r = Ring::default();
        for i in 0..(HISTORY + 25) {
            r.push(i as f32);
        }
        assert_eq!(r.len(), HISTORY);
        assert_eq!(r.iter().next(), Some(25.0));
        assert_eq!(r.iter().last(), Some((HISTORY + 24) as f32));
    }

    #[derive(Clone, Default)]
    struct Fake(Rc<RefCell<HashMap<String, String>>>);

    impl Fake {
        fn set(&self, name: &str, text: &str) {
            self.0.borrow_mut().insert(name.into(), text.into());
        }
    }

    impl ProcSource for Fake {
        fn read(&self, name: &str) -> Option<String> {
            self.0.borrow().get(name).cloned()
        }
    }

    fn full_fake() -> Fake {
        let f = Fake::default();
        f.set("stat", STAT);
        f.set("self/stat", SELF_STAT);
        f.set("self/status", "VmRSS: 2048 kB\n");
        f.set("meminfo", MEMINFO);
        f.set("loadavg", "0.5 0.4 0.3 1/2 3\n");
        f.set("uptime", "100.0 1.0\n");
        f
    }

    #[test]
    fn sampler_does_not_run_while_hidden() {
        let mut m = Monitor::new(Box::new(full_fake()), true);
        let t0 = Instant::now();
        for i in 0..10 {
            assert!(!m.tick(t0 + Duration::from_secs(i), false));
        }
        assert_eq!(m.samples_taken, 0);
        assert!(!m.running());
        assert!(m.cpu_history.is_empty());
    }

    #[test]
    fn sampler_samples_once_a_second_while_visible() {
        let fake = full_fake();
        let mut m = Monitor::new(Box::new(fake.clone()), true);
        let t0 = Instant::now();
        assert!(m.tick(t0, true));
        assert_eq!(m.reading.cpu, None, "no delta from the first sample");
        assert_eq!(m.reading.mem.used(), Some(10_000_000));
        assert_eq!(m.reading.own_rss, Some(2048));
        assert_eq!(m.reading.uptime, Some(100));
        assert!(!m.tick(t0 + Duration::from_millis(900), true));
        fake.set("stat", STAT2);
        fake.set("self/stat", &SELF_STAT.replace("30 20", "40 30"));
        assert!(m.tick(t0 + Duration::from_millis(1000), true));
        assert_eq!(m.reading.cpu, Some(50.0));
        assert_eq!(m.reading.cores, vec![50.0, 50.0]);
        // 20 own jiffies over a 200-jiffy window shared by 2 cores = 100 jiffies per core.
        assert_eq!(m.reading.own_cpu, Some(20.0));
        assert_eq!(m.cpu_history.len(), 1);
        assert_eq!(m.samples_taken, 2);
    }

    #[test]
    fn hiding_stops_sampling_and_drops_the_baseline() {
        let fake = full_fake();
        let mut m = Monitor::new(Box::new(fake.clone()), true);
        let t0 = Instant::now();
        m.tick(t0, true);
        assert!(m.running());
        m.tick(t0 + Duration::from_secs(1), false);
        assert!(!m.running());
        let taken = m.samples_taken;
        fake.set("stat", STAT2);
        // Shown again later: a fresh baseline, so no delta over the hidden time.
        assert!(m.tick(t0 + Duration::from_secs(60), true));
        assert_eq!(m.samples_taken, taken + 1);
        assert_eq!(m.reading.cpu, None);
    }

    #[test]
    fn missing_files_degrade_to_none() {
        let mut m = Monitor::new(Box::new(Fake::default()), true);
        assert!(m.tick(Instant::now(), true));
        assert_eq!(m.reading, Reading::default());
        let f = Fake::default();
        f.set("stat", "cpu 1 2");
        f.set("meminfo", "\u{0}\u{0}");
        f.set("uptime", "");
        f.set("self/status", "VmRSS: x kB");
        let mut m = Monitor::new(Box::new(f), true);
        assert!(m.tick(Instant::now(), true));
        assert_eq!(m.reading, Reading::default());
    }

    #[test]
    fn unsupported_platform_never_samples() {
        let mut m = Monitor::new(Box::new(full_fake()), false);
        assert!(!m.supported());
        assert!(!m.tick(Instant::now(), true));
        assert_eq!(m.samples_taken, 0);
    }

    #[test]
    fn history_of_a_long_run_stays_bounded() {
        let fake = full_fake();
        let mut m = Monitor::new(Box::new(fake.clone()), true);
        let t0 = Instant::now();
        for i in 0..200u64 {
            let t = format!(
                "cpu {} 0 0 {} 0\ncpu0 {} 0 0 {} 0\n",
                i * 10,
                i * 10,
                i * 10,
                i * 10
            );
            fake.set("stat", &t);
            m.tick(t0 + Duration::from_secs(i), true);
        }
        assert_eq!(m.cpu_history.len(), HISTORY);
        assert_eq!(m.own_history.len(), HISTORY);
    }

    #[test]
    fn real_proc_reads_on_linux() {
        if !cfg!(target_os = "linux") {
            return;
        }
        let mut m = Monitor::default();
        let t0 = Instant::now();
        assert!(m.tick(t0, true));
        assert!(m.reading.own_rss.is_some_and(|k| k > 0));
        assert!(m.reading.mem.total.is_some_and(|k| k > 0));
        assert!(m.reading.uptime.is_some());
        assert!(m.reading.load.is_some());
    }
}
