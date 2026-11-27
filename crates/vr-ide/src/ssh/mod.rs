//! SSH sessions (FR-25, D317): everything that decides what is run, kept apart from the UI.
//!
//! vr-ide starts the system `ssh` in a terminal tab and adds nothing an outsider controls
//! (02_threat-model_SSH.md): a [`Target`] is the only text that reaches the command line and it
//! is validated first; the arguments are an argv, never a command string; the one option that
//! makes ssh run something locally is a constant apart from a random [`Nonce`], and it is only
//! added when the user's own configuration has no `LocalCommand` ([`probe`]).

pub mod config;
pub mod recent;

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// The notice for a refused target. Fixed: what was typed is never echoed (it may be a
/// password typed into the wrong field, SEC-S9).
pub const NOT_VALID: &str = "SSH: not a valid target";

/// Live ssh tabs at most (SEC-S16).
pub const MAX_SESSIONS: usize = 16;

/// How long `ssh -G` may take, and how much it may print (SEC-S4).
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
pub const PROBE_OUTPUT_CAP: usize = 256 * 1024;
/// How often a probe that closed its output is asked whether it has ended.
const EXIT_POLL: Duration = Duration::from_millis(5);

/// The private OSC number and tag of the login marker.
const MARKER_OSC: u32 = 7717;
const MARKER_TAG: &str = "vr-ide-ssh";

const USER_MAX: usize = 64;
const HOST_MAX: usize = 253;

/// Where a session goes: `[user@]host[:port]` or `[user@][v6]:port`, validated (SEC-S1).
/// Every target (typed, a config alias, a line of the recent file) is made by [`Target::parse`]
/// and nothing else, so text that could be read as an option, a token or a second word never
/// reaches ssh.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Target {
    user: Option<String>,
    /// A host name, or an IPv6 address without its brackets.
    host: String,
    v6: bool,
    port: Option<u16>,
    /// The text it was parsed from, which is also how it is shown.
    text: String,
}

impl Target {
    /// `None` for anything but ASCII `[user@]host[:port]` / `[user@][v6][:port]`: a leading `-`
    /// in any part, whitespace, control characters, `%`, `$`, `/`, `\`, quotes, a backtick,
    /// `;`, a second `@`, `ssh://` and non-ASCII are all outside the allowed characters.
    pub fn parse(s: &str) -> Option<Target> {
        let (user, rest) = match s.split_once('@') {
            Some((user, rest)) => (Some(user), rest),
            None => (None, s),
        };
        if let Some(user) = user {
            let first = *user.as_bytes().first()?;
            let ok = user.len() <= USER_MAX
                && (first.is_ascii_alphanumeric() || first == b'.' || first == b'_')
                && user.bytes().all(name_byte);
            if !ok {
                return None;
            }
        }
        let (host, v6, port) = if let Some(inner) = rest.strip_prefix('[') {
            let (addr, after) = inner.split_once(']')?;
            // Hex digits, colons and dots only: a zone (`%eth0`) is refused with the rest.
            if !addr
                .bytes()
                .all(|b| b.is_ascii_hexdigit() || b == b':' || b == b'.')
            {
                return None;
            }
            addr.parse::<std::net::Ipv6Addr>().ok()?;
            let port = match after {
                "" => None,
                _ => Some(after.strip_prefix(':')?),
            };
            (addr, true, port)
        } else {
            let (host, port) = match rest.split_once(':') {
                Some((host, port)) => (host, Some(port)),
                None => (rest, None),
            };
            let ok = host.len() <= HOST_MAX
                && host.as_bytes().first()?.is_ascii_alphanumeric()
                && host.bytes().all(name_byte);
            if !ok {
                return None;
            }
            (host, false, port)
        };
        let port = match port {
            None => None,
            Some(p) => {
                if p.is_empty()
                    || p.len() > 5
                    || p.starts_with('0')
                    || !p.bytes().all(|b| b.is_ascii_digit())
                {
                    return None;
                }
                Some(p.parse::<u16>().ok().filter(|&n| n > 0)?)
            }
        };
        Some(Target {
            user: user.map(str::to_string),
            host: host.to_string(),
            v6,
            port,
            text: s.to_string(),
        })
    }

    /// The validated text: what the tab label, the status row and the recent file show.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// A bare host name, as a `Host` alias of the ssh config is.
    pub fn is_alias(&self) -> bool {
        self.user.is_none() && self.port.is_none() && !self.v6
    }

    /// `[-l user] [-p port]`.
    fn login_args(&self) -> Vec<OsString> {
        let mut args = Vec::new();
        if let Some(user) = &self.user {
            args.extend(["-l".into(), user.into()]);
        }
        if let Some(port) = self.port {
            args.extend(["-p".into(), port.to_string().into()]);
        }
        args
    }
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text)
    }
}

fn name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')
}

/// The secret of one session start: 128 bits from the OS, as 32 lowercase hex characters.
/// Whoever can print it into the session's own output can claim "logged in", so it lives only
/// here, in the ssh argv and in what `LocalCommand` prints; it is never logged or shown
/// (SEC-S7), which is why `Debug` hides it and nothing else gives it out.
pub struct Nonce(String);

impl std::fmt::Debug for Nonce {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Nonce(..)")
    }
}

impl Nonce {
    /// A fresh nonce; `None` when the OS gives no random bytes (the session then starts
    /// without a marker).
    pub fn new() -> Option<Nonce> {
        let mut bytes = [0u8; 16];
        let mut filled = 0;
        while filled < bytes.len() {
            match rustix::rand::getrandom(
                &mut bytes[filled..],
                rustix::rand::GetRandomFlags::empty(),
            ) {
                Ok(0) => return None,
                Ok(n) => filled += n,
                Err(rustix::io::Errno::INTR) => {}
                Err(_) => return None,
            }
        }
        Some(Nonce(bytes.iter().map(|b| format!("{b:02x}")).collect()))
    }

    /// The `LocalCommand` value (SEC-S3): ssh hands it to the user's shell after a successful
    /// login. A constant apart from the hex nonce: no target text, no `%` token, nothing a
    /// shell expands.
    pub fn local_command(&self) -> String {
        format!("printf '\\033]{MARKER_OSC};{MARKER_TAG};{}\\007'", self.0)
    }

    /// The bytes that command prints: what the session's output is searched for.
    pub fn marker(&self) -> Vec<u8> {
        format!("\x1b]{MARKER_OSC};{MARKER_TAG};{}\x07", self.0).into_bytes()
    }
}

/// The arguments of the session's ssh (SEC-S2, SEC-S5): exactly
/// `[-l user] [-p port] -o ControlMaster=no [-o PermitLocalCommand=yes -o LocalCommand=…] -- host`.
/// `ControlMaster=no` keeps vr-ide's ssh from becoming a connection-sharing master that would
/// outlive the tab; nothing else of the user's configuration is overridden.
pub fn argv(target: &Target, local_command: Option<&str>) -> Vec<OsString> {
    let mut args = target.login_args();
    args.extend(["-o".into(), "ControlMaster=no".into()]);
    if let Some(command) = local_command {
        args.extend(["-o".into(), "PermitLocalCommand=yes".into()]);
        args.extend(["-o".into(), format!("LocalCommand={command}").into()]);
    }
    args.extend(["--".into(), (&target.host).into()]);
    args
}

/// The arguments of the configuration probe: `-G [-l user] [-p port] -- host`, without any
/// option of vr-ide's own, so the output is what the user's configuration says.
pub fn probe_argv(target: &Target) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec!["-G".into()];
    args.extend(target.login_args());
    args.extend(["--".into(), (&target.host).into()]);
    args
}

/// The first executable `name` in the absolute entries of `path` (a `PATH` value). Empty and
/// relative entries are skipped, so a program in the working directory can never be picked:
/// the rule vr-git applies to `git`.
pub fn find_in_path(path: &OsStr, name: &str) -> Option<PathBuf> {
    path.as_bytes()
        .split(|&b| b == b':')
        .map(|d| PathBuf::from(OsStr::from_bytes(d)))
        .filter(|d| d.is_absolute())
        .map(|d| d.join(name))
        .find(|f| {
            f.metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
}

/// What the user's ssh configuration says about a target (SEC-S4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// No `LocalCommand` of the user's: vr-ide may add its own. `shared`: a connection-sharing
    /// socket exists at the target's `ControlPath`; a session riding on it runs no
    /// `LocalCommand`.
    Clean { shared: bool },
    /// The user has a `LocalCommand`: vr-ide adds none and does not enable theirs.
    UserLocalCommand,
    /// `ssh -G` failed, printed too much or did not finish in time.
    Failed,
}

/// Runs `ssh -G` for `target` and reads the effective configuration. Blocks up to `timeout`
/// (call it off the UI thread). The probe has no stdin and no controlling terminal, so nothing
/// it starts (`Match exec`) can prompt; past the timeout or [`PROBE_OUTPUT_CAP`] its whole
/// session is killed.
pub fn probe(ssh: &Path, target: &Target, timeout: Duration) -> Probe {
    let mut cmd = Command::new(ssh);
    cmd.args(probe_argv(target))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // SAFETY: setsid is async-signal-safe and touches no memory of the parent.
    unsafe {
        cmd.pre_exec(|| {
            rustix::process::setsid()
                .map(|_| ())
                .map_err(std::io::Error::from)
        });
    }
    let Ok(mut child) = cmd.spawn() else {
        return Probe::Failed;
    };
    let group = rustix::process::Pid::from_child(&child);
    let Some(mut pipe) = child.stdout.take() else {
        return Probe::Failed;
    };
    let (tx, rx) = mpsc::channel();
    // When no reader can start the channel disconnects and the probe counts as failed.
    let _ = std::thread::Builder::new()
        .name("vr-ssh-probe-read".into())
        .spawn(move || {
            let mut out = Vec::new();
            let mut buf = [0u8; 8192];
            let complete = loop {
                match pipe.read(&mut buf) {
                    Ok(0) => break true,
                    Ok(n) if out.len() + n > PROBE_OUTPUT_CAP => break false,
                    Ok(n) => out.extend_from_slice(&buf[..n]),
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break false,
                }
            };
            let _ = tx.send(complete.then_some(out));
        });
    let deadline = Instant::now() + timeout;
    let output = rx.recv_timeout(timeout).ok().flatten();
    // A probe that closed its output but does not end is waited for until the deadline only.
    let mut exited = None;
    while output.is_some() && exited.is_none() {
        match child.try_wait() {
            Ok(Some(status)) => exited = Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(EXIT_POLL),
            _ => break,
        }
    }
    let Some(status) = exited else {
        // Still unreaped, so the pid (and with it the group) is still ours.
        let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
        let _ = child.wait();
        return Probe::Failed;
    };
    match output {
        Some(out) if status.success() => parse_probe(&out),
        _ => Probe::Failed,
    }
}

/// `ssh -G` prints one `keyword value` line per option, keywords in lower case.
fn parse_probe(out: &[u8]) -> Probe {
    let text = String::from_utf8_lossy(out);
    let value_of = |key: &str| {
        text.lines()
            .filter_map(|l| l.split_once(' '))
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.trim())
            .filter(|v| !v.is_empty())
    };
    if value_of("localcommand").is_some() {
        return Probe::UserLocalCommand;
    }
    let shared = value_of("controlpath")
        .filter(|p| *p != "none")
        .and_then(|p| std::fs::metadata(p).ok())
        .is_some_and(|m| m.file_type().is_socket());
    Probe::Clean { shared }
}

#[cfg(test)]
mod tests;
