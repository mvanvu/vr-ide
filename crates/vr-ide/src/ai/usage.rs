//! claude's usage and conversation id through its statusLine (AC-3, SEC-22..SEC-25, SEC-39).
//!
//! A claude session vr-ide starts gets `--settings <state>/ai-usage/<stem>.json`, a file holding
//! only `{"statusLine":{"type":"command","command":"'<exe>' --ai-statusline '<report>'"}}`.
//! claude runs that command through its shell (D2: the single quoting is the control) with its
//! status JSON on stdin; the helper ([`run_helper`]) keeps a bounded, validated copy of a few
//! fields in `<stem>.report` and prints a short status line for claude to show. vr-ide reads the
//! report on its refresh ([`Poll`]), validating every field again: the agent can write the file.
//!
//! The status JSON as claude 2.1.332 documents it for statusLine commands (fields used here):
//! `{"session_id": "<uuid>", "model": {"display_name": "Opus 5.5"}, "rate_limits":
//! {"five_hour": {"used_percentage": 63, "resets_at": <unix seconds>}, "seven_day": {...}}}`;
//! `rate_limits` and each window are optional (subscribers only, after the first answer).

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rustix::fs::{AtFlags, FileType, Mode, OFlags};
use rustix::io::Errno;
use serde_json::{Map, Value, json};

use crate::cli::AI_STATUSLINE_FLAG;

/// The folder under the state folder holding every session's settings and report.
pub const DIR: &str = "ai-usage";
/// Status JSON larger than this is ignored (SEC-23).
pub const MAX_STATUS: usize = 64 * 1024;
/// A report larger than this is ignored (SEC-25); the helper writes a few hundred bytes.
pub const MAX_REPORT: u64 = 4096;
/// The helper gives up on stdin after this long (SEC-23).
pub const HELPER_DEADLINE: Duration = Duration::from_secs(2);
/// A report is read at most this often per session (SEC-25).
pub const READ_GAP: Duration = Duration::from_secs(1);
/// At most this many names are looked at by the startup sweep (SEC-39).
pub const SWEEP_LIMIT: usize = 1000;
/// A dead-looking window's files stay this long (seconds) after their last change: the pid may
/// belong to another pid namespace or host sharing the state folder.
pub const SWEEP_AGE: i64 = 86_400;
/// A report is looked at (`lstat`) at most this often per session (SEC-39).
pub const STAT_GAP: Duration = Duration::from_millis(250);

const MODEL_MAX: usize = 64;
const LINE_MAX: usize = 120;
/// `resets_at` is kept only within [now − 1 day, now + 35 days] (SEC-25).
const RESET_PAST: i64 = 86_400;
const RESET_FUTURE: i64 = 35 * 86_400;
const SETTINGS_EXT: &str = ".json";
const REPORT_EXT: &str = ".report";
const TEMP_MARK: &str = ".tmp";

/// One usage window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
    /// Finite, 0..=1000.
    pub used_percentage: f64,
    /// Unix seconds, within the accepted range.
    pub resets_at: Option<i64>,
}

/// What a report says; every field already validated.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Usage {
    /// A canonical lowercase UUID ([`valid_session_id`]).
    pub session_id: Option<String>,
    pub five_hour: Option<Window>,
    pub seven_day: Option<Window>,
    /// Sanitised, at most 64 characters.
    pub model: Option<String>,
}

/// SEC-24: a session id is a canonical lowercase UUID, so it can never read as an option when
/// it is passed on as an argument.
pub fn valid_session_id(id: &str) -> bool {
    let b = id.as_bytes();
    b.len() == 36
        && b.iter().enumerate().all(|(i, &c)| match i {
            8 | 13 | 18 | 23 => c == b'-',
            _ => c.is_ascii_digit() || (b'a'..=b'f').contains(&c),
        })
}

pub fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

pub fn parse_status(json: &[u8]) -> Option<Usage> {
    parse_status_at(json, now_secs())
}

/// The kept fields of status JSON (or of a report, which has the same shape), checked against
/// `now` (unix seconds). `None`: too big, not JSON, or not an object.
pub fn parse_status_at(json: &[u8], now: i64) -> Option<Usage> {
    if json.len() > MAX_STATUS {
        return None;
    }
    let v: Value = serde_json::from_slice(json).ok()?;
    let o = v.as_object()?;
    let session_id = o
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|s| valid_session_id(s))
        .map(str::to_owned);
    let model = o
        .get("model")
        .and_then(|m| m.get("display_name"))
        .and_then(Value::as_str)
        .map(clean_model)
        .filter(|m| !m.is_empty());
    let limits = o.get("rate_limits");
    let window = |key: &str| {
        limits
            .and_then(|l| l.get(key))
            .and_then(|w| parse_window(w, now))
    };
    Some(Usage {
        session_id,
        five_hour: window("five_hour"),
        seven_day: window("seven_day"),
        model,
    })
}

fn clean_model(name: &str) -> String {
    crate::ui::safe_text::sanitize(name.trim())
        .chars()
        .take(MODEL_MAX)
        .collect()
}

fn parse_window(w: &Value, now: i64) -> Option<Window> {
    let used_percentage = w
        .get("used_percentage")?
        .as_f64()
        .filter(|p| p.is_finite())?
        .clamp(0.0, 1000.0);
    let resets_at = w
        .get("resets_at")
        .and_then(reset_time)
        .filter(|t| (now - RESET_PAST..=now + RESET_FUTURE).contains(t));
    Some(Window {
        used_percentage,
        resets_at,
    })
}

/// Unix seconds (an integer or a fraction) or an RFC 3339 time.
fn reset_time(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| {
            n.as_f64()
                .filter(|f| f.is_finite() && f.abs() < 1e12)
                // In range by the filter; the fraction of a second is dropped.
                .map(|f| f.trunc() as i64)
        }),
        Value::String(s) => rfc3339(s),
        _ => None,
    }
}

/// `YYYY-MM-DDTHH:MM:SS[.frac](Z|±HH:MM)` → unix seconds.
fn rfc3339(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if !(20..=40).contains(&b.len()) {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<i64> {
        let digits = b.get(from..to)?;
        if !digits.iter().all(u8::is_ascii_digit) {
            return None;
        }
        std::str::from_utf8(digits).ok()?.parse().ok()
    };
    let at = |i: usize, set: &[u8]| b.get(i).is_some_and(|c| set.contains(c));
    if !(at(4, b"-") && at(7, b"-") && at(10, b"Tt") && at(13, b":") && at(16, b":")) {
        return None;
    }
    let (year, month, day) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (hour, minute, second) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&month)
        || day < 1
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let mut i = 19;
    if b.get(i) == Some(&b'.') {
        i += 1;
        let from = i;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        if i == from {
            return None;
        }
    }
    let offset = match b.get(i)? {
        b'Z' | b'z' if i + 1 == b.len() => 0,
        sign @ (b'+' | b'-') if i + 6 == b.len() && at(i + 3, b":") => {
            let (h, m) = (num(i + 1, i + 3)?, num(i + 4, i + 6)?);
            if h > 23 || m > 59 {
                return None;
            }
            let o = h * 3600 + m * 60;
            if *sign == b'-' { -o } else { o }
        }
        _ => return None,
    };
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 of a proleptic Gregorian date (H. Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The line the helper prints for claude: `Opus 5.5 · 5h 63%`, from the kept fields only.
pub fn status_line(u: &Usage) -> String {
    let mut parts = Vec::new();
    if let Some(m) = &u.model {
        parts.push(m.clone());
    }
    if let Some(w) = &u.five_hour {
        parts.push(format!("5h {:.0}%", w.used_percentage));
    }
    parts
        .join(" · ")
        .chars()
        .filter(|c| !c.is_control())
        .take(LINE_MAX)
        .collect()
}

/// The report: the status JSON shape holding the kept fields only.
fn report_json(u: &Usage) -> Vec<u8> {
    let mut o = Map::new();
    if let Some(id) = &u.session_id {
        o.insert("session_id".into(), json!(id));
    }
    if let Some(m) = &u.model {
        o.insert("model".into(), json!({ "display_name": m }));
    }
    let mut limits = Map::new();
    for (key, w) in [("five_hour", &u.five_hour), ("seven_day", &u.seven_day)] {
        if let Some(w) = w {
            limits.insert(
                key.into(),
                json!({ "used_percentage": w.used_percentage, "resets_at": w.resets_at }),
            );
        }
    }
    if !limits.is_empty() {
        o.insert("rate_limits".into(), Value::Object(limits));
    }
    serde_json::to_vec(&Value::Object(o)).unwrap_or_default()
}

/// Writes `u` as the report at `path` (SEC-23): a private temp file in the same folder, fsync,
/// rename over it (a symlink there is replaced, never followed). `path` must pass the helper's
/// rules ([`open_report_dir`]).
pub fn write_report(path: &Path, u: &Usage) -> io::Result<()> {
    let (dir, name) = open_report_dir(path)?;
    write_at(&dir, &name, &report_json(u))
}

fn write_at(dir: &OwnedFd, name: &str, body: &[u8]) -> io::Result<()> {
    let tmp = format!("{name}{TEMP_MARK}{}", std::process::id());
    let _ = rustix::fs::unlinkat(dir, tmp.as_str(), AtFlags::empty());
    let flags = OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let fd = rustix::fs::openat(dir, tmp.as_str(), flags, Mode::RUSR | Mode::WUSR)?;
    let mut f = File::from(fd);
    let res = f
        .write_all(body)
        .and_then(|()| f.sync_all())
        .and_then(|()| rustix::fs::renameat(dir, tmp.as_str(), dir, name).map_err(io::Error::from));
    if res.is_err() {
        let _ = rustix::fs::unlinkat(dir, tmp.as_str(), AtFlags::empty());
    }
    res
}

/// Writes the report of a session whose settings file was there when the helper started; when
/// the session closed meanwhile (its settings file is gone), the report is taken back so none
/// outlives its session.
pub(crate) fn commit_report(dir: &OwnedFd, name: &str, body: &[u8]) -> io::Result<()> {
    write_at(dir, name, body)?;
    let settings = format!("{}{SETTINGS_EXT}", stem_of(name));
    if rustix::fs::statat(dir, settings.as_str(), AtFlags::SYMLINK_NOFOLLOW).is_err() {
        let _ = rustix::fs::unlinkat(dir, name, AtFlags::empty());
        return Err(io::ErrorKind::NotFound.into());
    }
    Ok(())
}

/// SEC-23: the report path is absolute, its name `<pid>-<n>-<32 hex>.report`, its folder a real
/// folder named `ai-usage` owned by this user without group / other bits, and the session's
/// settings file is still there (a session that closed gets no new report). Returns the folder,
/// opened without following a link, and the file name.
fn open_report_dir(path: &Path) -> io::Result<(OwnedFd, String)> {
    let refused = || io::Error::from(io::ErrorKind::PermissionDenied);
    if !path.is_absolute() {
        return Err(refused());
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(refused)?;
    let stem = name.strip_suffix(REPORT_EXT).ok_or_else(refused)?;
    if !is_stem(stem) {
        return Err(refused());
    }
    let parent = path.parent().ok_or_else(refused)?;
    if parent.file_name().and_then(|n| n.to_str()) != Some(DIR) {
        return Err(refused());
    }
    let dir = open_private_dir(parent)?;
    let settings = rustix::fs::statat(
        &dir,
        format!("{stem}{SETTINGS_EXT}").as_str(),
        AtFlags::SYMLINK_NOFOLLOW,
    )?;
    if FileType::from_raw_mode(settings.st_mode) != FileType::RegularFile {
        return Err(refused());
    }
    Ok((dir, name.to_string()))
}

/// A real folder (not a link) owned by this user with no group / other bits.
fn open_private_dir(dir: &Path) -> io::Result<OwnedFd> {
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let fd = rustix::fs::open(dir, flags, Mode::empty())?;
    let st = rustix::fs::fstat(&fd)?;
    if st.st_uid != rustix::process::geteuid().as_raw() || st.st_mode & 0o077 != 0 {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(fd)
}

/// `<digits>-<digits>-<32 lowercase hex>`.
fn is_stem(s: &str) -> bool {
    let mut parts = s.split('-');
    let digits = |p: Option<&str>, max: usize| {
        p.is_some_and(|p| !p.is_empty() && p.len() <= max && p.bytes().all(|c| c.is_ascii_digit()))
    };
    digits(parts.next(), 10)
        && digits(parts.next(), 20)
        && parts.next().is_some_and(|h| {
            h.len() == 32 && h.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'))
        })
        && parts.next().is_none()
}

pub fn read_report(path: &Path) -> Option<Usage> {
    read_report_at(path, now_secs())
}

/// SEC-25: opened without following a link and without blocking (a FIFO), a regular file of
/// this user of at most 4 KiB, every field validated again.
pub fn read_report_at(path: &Path, now: i64) -> Option<Usage> {
    let flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
    let fd = rustix::fs::open(path, flags, Mode::empty()).ok()?;
    let st = rustix::fs::fstat(&fd).ok()?;
    if FileType::from_raw_mode(st.st_mode) != FileType::RegularFile
        || st.st_uid != rustix::process::geteuid().as_raw()
        || u64::try_from(st.st_size).map_or(true, |n| n > MAX_REPORT)
    {
        return None;
    }
    let mut buf = Vec::new();
    File::from(fd)
        .take(MAX_REPORT + 1)
        .read_to_end(&mut buf)
        .ok()?;
    if buf.len() as u64 > MAX_REPORT {
        return None;
    }
    parse_status_at(&buf, now)
}

/// `vr-ide --ai-statusline <report>` (SEC-23): runs before anything else in `main`, touches
/// no file but the report, prints nothing on bad input. Always exit code 0.
pub fn run_helper(report: &Path) -> i32 {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    helper(
        report,
        stdin.as_fd(),
        &mut out,
        Instant::now() + HELPER_DEADLINE,
        now_secs(),
    );
    let _ = out.flush();
    0
}

pub(crate) fn helper(
    report: &Path,
    input: BorrowedFd<'_>,
    out: &mut dyn Write,
    deadline: Instant,
    now: i64,
) {
    let Ok((dir, name)) = open_report_dir(report) else {
        return;
    };
    let Some(bytes) = read_bounded(input, MAX_STATUS, deadline) else {
        return;
    };
    let Some(u) = parse_status_at(&bytes, now) else {
        return;
    };
    if commit_report(&dir, &name, &report_json(&u)).is_err() {
        return;
    }
    let line = status_line(&u);
    if !line.is_empty() {
        let _ = writeln!(out, "{line}");
    }
}

/// Everything until end of input, or `None` past `limit` bytes or `deadline`.
pub(crate) fn read_bounded(fd: BorrowedFd<'_>, limit: usize, deadline: Instant) -> Option<Vec<u8>> {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let left = deadline.checked_duration_since(Instant::now())?;
        if left.is_zero() {
            return None;
        }
        let timeout = Timespec {
            tv_sec: i64::try_from(left.as_secs()).unwrap_or(i64::MAX),
            tv_nsec: i64::from(left.subsec_nanos()),
        };
        let mut fds = [PollFd::new(&fd, PollFlags::IN)];
        match poll(&mut fds, Some(&timeout)) {
            Ok(0) => return None,
            Ok(_) => {}
            Err(Errno::INTR) => continue,
            Err(_) => return None,
        }
        match rustix::io::read(fd, &mut chunk) {
            Ok(0) => return Some(buf),
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if buf.len() > limit {
                    return None;
                }
            }
            Err(Errno::INTR | Errno::AGAIN) => {}
            Err(_) => return None,
        }
    }
}

/// POSIX single quoting: the shell gives `s` back as one word, whatever it holds.
pub(crate) fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// A session's settings and report files; dropping this removes both (the session closed or
/// started again with new ones).
#[derive(Debug)]
pub struct SessionFiles {
    pub settings: PathBuf,
    pub report: PathBuf,
}

impl Drop for SessionFiles {
    fn drop(&mut self) {
        // remove_file never follows a link.
        let _ = std::fs::remove_file(&self.settings);
        let _ = std::fs::remove_file(&self.report);
    }
}

/// A path that goes into the statusLine command: valid UTF-8, absolute, without control
/// characters or backslashes (some shells read `\` inside single quotes).
fn command_path(p: &Path) -> Result<&str, &'static str> {
    p.to_str()
        .filter(|s| p.is_absolute() && !s.chars().any(|c| c.is_control() || c == '\\'))
        .ok_or("vr-ide path unusable")
}

/// SEC-22: makes `<state>/ai-usage/` (0700, real folder of this user) and a new settings file
/// in it naming `exe` as the statusLine helper. `exe` must be an absolute, existing regular
/// file, not one replaced since start (` (deleted)`).
pub fn prepare(state: &Path, exe: &Path) -> Result<SessionFiles, &'static str> {
    let exe_text = command_path(exe)?;
    if exe_text.ends_with(" (deleted)") || !std::fs::metadata(exe).is_ok_and(|m| m.is_file()) {
        return Err("vr-ide path unusable");
    }
    command_path(state).map_err(|_| "state folder path unusable")?;
    const DIR_UNUSABLE: &str = "usage folder unusable";
    let dir_path = state.join(DIR);
    crate::recovery::create_private_dir(&dir_path).map_err(|_| DIR_UNUSABLE)?;
    // Drops group / other bits, refuses a link or another owner.
    super::accounts::secure(&dir_path).map_err(|_| DIR_UNUSABLE)?;
    let dir = open_private_dir(&dir_path).map_err(|_| DIR_UNUSABLE)?;
    static SERIAL: AtomicU64 = AtomicU64::new(1);
    let stem = format!(
        "{}-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed),
        random_hex().ok_or("no random bytes")?
    );
    let settings_name = format!("{stem}{SETTINGS_EXT}");
    let report = dir_path.join(format!("{stem}{REPORT_EXT}"));
    let command = format!(
        "{} {AI_STATUSLINE_FLAG} {}",
        shell_quote(exe_text),
        shell_quote(command_path(&report).map_err(|_| "state folder path unusable")?)
    );
    let body = json!({ "statusLine": { "type": "command", "command": command } }).to_string();
    let flags = OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let fd = rustix::fs::openat(&dir, settings_name.as_str(), flags, Mode::RUSR | Mode::WUSR)
        .map_err(|_| DIR_UNUSABLE)?;
    let files = SessionFiles {
        settings: dir_path.join(settings_name),
        report,
    };
    rustix::fs::fchmod(&fd, Mode::RUSR | Mode::WUSR).map_err(|_| DIR_UNUSABLE)?;
    File::from(fd)
        .write_all(body.as_bytes())
        .map_err(|_| DIR_UNUSABLE)?;
    Ok(files)
}

/// 32 lowercase hex characters from the OS.
fn random_hex() -> Option<String> {
    let mut bytes = [0u8; 16];
    let mut filled = 0;
    while filled < bytes.len() {
        match rustix::rand::getrandom(&mut bytes[filled..], rustix::rand::GetRandomFlags::empty()) {
            Ok(0) => return None,
            Ok(n) => filled += n,
            Err(Errno::INTR) => {}
            Err(_) => return None,
        }
    }
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// SEC-22, SEC-39: removes the usage files of vr-ide windows that ended: their pid is not a live
/// process of this user and the newest file of their stem is older than [`SWEEP_AGE`]. Looks at
/// no more than [`SWEEP_LIMIT`] names; nothing is followed.
pub fn sweep(state: &Path) {
    let Ok(dir) = open_private_dir(&state.join(DIR)) else {
        return;
    };
    let Ok(entries) = rustix::fs::Dir::read_from(&dir) else {
        return;
    };
    let me = std::process::id();
    let candidates: Vec<String> = entries
        .take(SWEEP_LIMIT)
        .map_while(Result::ok)
        .filter_map(|e| e.file_name().to_str().ok().map(str::to_owned))
        .filter(|n| owner_pid(n).is_some_and(|pid| pid != me && !alive(pid)))
        .collect();
    // The newest file of each stem: a live window this pid namespace cannot see (a container,
    // a state folder on a network share) keeps its files recent; idle ones go after a day.
    let mut newest: std::collections::HashMap<&str, i64> = Default::default();
    for name in &candidates {
        // `st_mtime` is narrower than i64 on some targets.
        #[allow(clippy::useless_conversion)]
        let mtime = rustix::fs::statat(&dir, name.as_str(), AtFlags::SYMLINK_NOFOLLOW)
            .map_or(i64::MAX, |st| i64::from(st.st_mtime));
        let e = newest.entry(stem_of(name)).or_insert(i64::MIN);
        *e = (*e).max(mtime);
    }
    let cutoff = now_secs() - SWEEP_AGE;
    for name in &candidates {
        if newest.get(stem_of(name)).is_some_and(|&t| t < cutoff) {
            let _ = rustix::fs::unlinkat(&dir, name.as_str(), AtFlags::empty());
        }
    }
}

fn stem_of(name: &str) -> &str {
    name.split('.').next().unwrap_or(name)
}

/// The vr-ide pid a usage file name starts with: `<stem>.json`, `<stem>.report` or a helper's
/// `<stem>.report.tmp<pid>`.
fn owner_pid(name: &str) -> Option<u32> {
    let (stem, rest) = name.split_at(name.find('.')?);
    if !is_stem(stem) {
        return None;
    }
    let temp = rest
        .strip_prefix(REPORT_EXT)
        .and_then(|r| r.strip_prefix(TEMP_MARK))
        .is_some_and(|p| !p.is_empty() && p.bytes().all(|c| c.is_ascii_digit()));
    if !(rest == SETTINGS_EXT || rest == REPORT_EXT || temp) {
        return None;
    }
    stem.split('-').next()?.parse().ok()
}

/// A live process this user may signal; one of another user does not count.
fn alive(pid: u32) -> bool {
    i32::try_from(pid)
        .ok()
        .and_then(rustix::process::Pid::from_raw)
        .is_some_and(|p| rustix::process::test_kill_process(p).is_ok())
}

/// When a session's report was last looked at (SEC-25, SEC-39).
#[derive(Debug, Default)]
pub struct Poll {
    /// (mtime s, mtime ns, size, inode) at the last read.
    seen: Option<(i64, i64, u64, u64)>,
    read_at: Option<Instant>,
    checked_at: Option<Instant>,
}

impl Poll {
    /// One `lstat` at most every [`STAT_GAP`]; a read only when the file changed and the last
    /// read is [`READ_GAP`] ago. `Some(result)` when it read (`None` inside: unusable report).
    pub fn check(&mut self, path: &Path, now: Instant) -> Option<Option<Usage>> {
        if self
            .checked_at
            .is_some_and(|t| now.saturating_duration_since(t) < STAT_GAP)
        {
            return None;
        }
        self.checked_at = Some(now);
        let m = std::fs::symlink_metadata(path).ok()?;
        let key = (m.mtime(), m.mtime_nsec(), m.size(), m.ino());
        if self.seen == Some(key) {
            return None;
        }
        if self
            .read_at
            .is_some_and(|t| now.saturating_duration_since(t) < READ_GAP)
        {
            return None;
        }
        self.seen = Some(key);
        self.read_at = Some(now);
        Some(read_report(path))
    }
}

#[cfg(test)]
#[path = "usage_tests.rs"]
mod tests;
