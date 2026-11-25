//! The one place that spawns git (threat model SEC-1, SEC-2, SEC-3, SEC-7, SEC-8).

use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime};

use rustix::process::Pid;

use crate::{GitError, Result};

/// Limits and environment of the git runner.
#[derive(Debug, Clone)]
pub struct Options {
    /// Wall-clock limit of an automatic read (status, gutter, inline blame).
    pub timeout: Duration,
    /// Wall-clock limit of a read the user asked for (file blame, history, commit details).
    pub user_timeout: Duration,
    /// Byte cap on the captured output of one command.
    pub max_output: usize,
    /// Byte cap on one file's diff and on file contents read for the gutter.
    pub max_file: usize,
    /// At most this many git processes run at once for one repo.
    pub max_concurrent: usize,
    /// Environment git starts from (filtered, see [`child_env`]); `None` = this process's.
    pub env: Option<Vec<(OsString, OsString)>>,
    /// Wall-clock limit of a commit (hooks and signing run inside it).
    pub hook_timeout: Duration,
    /// Push/pull are killed after this long without any output from git.
    pub network_idle_timeout: Duration,
    /// Where discard keeps recovery copies (absolute). The app sets it to
    /// `vr_config::paths::state_dir().join("git-recovery")`; `None` = the same rule applied to
    /// [`Options::env`] (`VR_IDE_STATE_DIR`, else `XDG_STATE_HOME/vr-ide`, else
    /// `~/.local/state/vr-ide`). Inside a git work tree it is used only when that repository
    /// ignores it (see `Writer::discard`).
    pub recovery_dir: Option<PathBuf>,
    /// Discard refuses a target whose recovery copy would be larger than this many bytes.
    pub recovery_cap: u64,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(10),
            user_timeout: Duration::from_secs(30),
            max_output: 64 << 20,
            max_file: 8 << 20,
            max_concurrent: 2,
            env: None,
            hook_timeout: Duration::from_secs(600),
            network_idle_timeout: Duration::from_secs(120),
            recovery_dir: None,
            recovery_cap: 256 << 20,
        }
    }
}

/// Outer `GIT_*` variables that may reach git; every other one would redirect the repo, index
/// or config (`GIT_DIR`, `GIT_INDEX_FILE`, `GIT_CONFIG_PARAMETERS`, ...) or run programs.
const GIT_ENV_ALLOW: &[&str] = &[
    "GIT_AUTHOR_NAME",
    "GIT_AUTHOR_EMAIL",
    "GIT_AUTHOR_DATE",
    "GIT_COMMITTER_NAME",
    "GIT_COMMITTER_EMAIL",
    "GIT_COMMITTER_DATE",
    "GIT_SSH",
    "GIT_SSH_COMMAND",
    "GIT_CONFIG_GLOBAL",
    "GIT_CONFIG_NOSYSTEM",
    "GIT_CEILING_DIRECTORIES",
];

/// Variables set for every git child, after the outer ones are filtered.
const FIXED_ENV: &[(&str, &str)] = &[
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GIT_OPTIONAL_LOCKS", "0"),
    ("GIT_PAGER", "cat"),
    ("PAGER", "cat"),
    ("GIT_EDITOR", "true"),
    ("GIT_SEQUENCE_EDITOR", "true"),
    ("GIT_MERGE_AUTOEDIT", "no"),
    ("GIT_NO_LAZY_FETCH", "1"),
    ("GIT_LITERAL_PATHSPECS", "1"),
    // gpg-agent hands this tty to pinentry, which opens it by path (no controlling terminal
    // needed) and would draw over the TUI and read its keys while a signed commit asks for a
    // passphrase. A curses or tty pinentry now fails and git reports it; a graphical one still
    // works through DISPLAY / WAYLAND_DISPLAY (SEC-12, D305(1)).
    ("GPG_TTY", "/dev/null"),
];

/// Outer variables that only steer a pinentry to a terminal (`GPG_AGENT_INFO` is read by gpg
/// before 2.1, `PINENTRY_USER_DATA` picks curses mode in common pinentry wrappers).
const TTY_HINT_ENV: &[&str] = &["GPG_AGENT_INFO", "PINENTRY_USER_DATA"];

/// Config forced on every command: nothing config-driven runs (SEC-3) and output keeps the
/// machine shape the parsers expect whatever the user's config says (SEC-4).
const FIXED_CONFIG: &[(&str, &str)] = &[
    ("core.fsmonitor", "false"),
    ("core.hooksPath", "/dev/null"),
    ("core.pager", "cat"),
    ("log.showSignature", "false"),
    ("diff.external", ""),
    ("protocol.allow", "never"),
    ("color.ui", "false"),
    ("core.quotePath", "false"),
    // A submodule's dirty check runs `git status` inside it with the submodule's own config,
    // whose filter drivers are not blanked; status and diff-files also pass the flag, which
    // wins over `submodule.<name>.ignore`.
    ("diff.ignoreSubmodules", "dirty"),
    ("diff.noprefix", "false"),
    ("diff.mnemonicPrefix", "false"),
    ("diff.relative", "false"),
    ("diff.renames", "true"),
    ("diff.suppressBlankEmpty", "false"),
    ("diff.interHunkContext", "0"),
    ("diff.submodule", "short"),
    ("status.relativePaths", "false"),
    ("status.renames", "true"),
    ("status.submoduleSummary", "false"),
    ("i18n.logOutputEncoding", "UTF-8"),
    ("blame.ignoreRevsFile", ""),
    ("log.follow", "false"),
    // Without it a root commit has no name-status, so `log --follow` drops it.
    ("log.showRoot", "true"),
    // Patches built from a shown diff must land exactly: no whitespace fixing or
    // whitespace-blind matching from user config.
    ("apply.whitespace", "nowarn"),
    ("apply.ignoreWhitespace", "no"),
];

/// Extra config of write commands: never an editor on the TUI's terminal, never an askpass
/// program (an empty `core.askPass` also hides `SSH_ASKPASS` from git's credential prompt).
const WRITE_CONFIG: &[(&str, &str)] = &[("core.editor", "true"), ("core.askPass", "")];

/// Drivers (`filter.<d>.*`, `diff.<d>.*`) whose program keys are blanked on every command.
const DRIVER_KEYS: &str = r"^(filter|diff)\..+\.(clean|smudge|process|textconv|command)$";

const STDERR_CAP: usize = 64 << 10;

/// Output of one git command.
#[derive(Debug)]
pub(crate) struct Output {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

/// What a command may do besides its own work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Profile {
    /// Automatic and user reads: no program named by config runs (driver keys blanked, hooks
    /// off), no transport.
    Read,
    /// A user-initiated index or worktree write (add, restore, apply): runs like the same
    /// command typed in a terminal (filters and hooks: accepted risk T18, made visible by
    /// SEC-19), no transport.
    Index,
    /// `commit`: like [`Profile::Index`] but without forced literal pathspecs, which hooks would
    /// inherit.
    Commit,
    /// push / pull: transports allowed, askpass disabled.
    Network,
}

/// How long and how much one call may take.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Limits {
    pub timeout: Duration,
    pub max_output: usize,
    /// Every profile but [`Profile::Network`] sets `GIT_ALLOW_PROTOCOL=none`: it stops a lazy
    /// fetch even where repo config sets `protocol.<name>.allow` (which beats
    /// `protocol.allow=never`) and git predates `GIT_NO_LAZY_FETCH`.
    pub profile: Profile,
}

impl Limits {
    pub fn new(timeout: Duration, max_output: usize) -> Limits {
        Limits {
            timeout,
            max_output,
            profile: Profile::Read,
        }
    }

    pub fn with(self, profile: Profile) -> Limits {
        Limits { profile, ..self }
    }
}

/// A cancel switch shared between the caller and a running write (push, pull, commit).
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn new() -> Cancel {
        Cancel::default()
    }

    /// Kill the running git with its process group; the call returns [`GitError::Cancelled`].
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Callback for each complete stderr line (progress of push/pull).
pub(crate) type LineSink = Arc<dyn Fn(&[u8]) + Send + Sync>;

/// Live control of one long call.
#[derive(Default, Clone)]
pub(crate) struct Ctl {
    pub cancel: Option<Cancel>,
    /// Kill when git prints nothing for this long.
    pub idle: Option<Duration>,
    pub on_line: Option<LineSink>,
    /// Variables set after the filtered environment (e.g. a default `GIT_SSH_COMMAND`).
    pub env: Vec<(OsString, OsString)>,
}

#[derive(Default)]
struct Slots {
    busy: Mutex<usize>,
    freed: Condvar,
}

/// Config files whose change means the driver list must be read again.
type ConfigStamp = Vec<Option<(SystemTime, u64)>>;

#[derive(Default)]
struct Live {
    closed: bool,
    groups: Vec<Pid>,
}

struct Drivers {
    stamp: ConfigStamp,
    config: Vec<(OsString, OsString)>,
}

struct Inner {
    exe: PathBuf,
    env: Vec<(OsString, OsString)>,
    opts: Options,
    slots: Slots,
    /// Process groups of the running gits, killed by [`Git::shutdown`].
    live: Mutex<Live>,
    drivers: Mutex<Option<Drivers>>,
    /// Config files watched for driver changes (filled once the repo is known).
    config_files: Mutex<Vec<PathBuf>>,
}

/// The git runner of one repo (cheap to clone).
#[derive(Clone)]
pub(crate) struct Git {
    inner: Arc<Inner>,
}

impl Git {
    pub fn new(opts: Options) -> Result<Git> {
        let base: Vec<(OsString, OsString)> = match &opts.env {
            Some(env) => env.clone(),
            None => std::env::vars_os().collect(),
        };
        let exe = find_git(&base).ok_or(GitError::GitNotFound)?;
        let env = child_env(&base);
        Ok(Git {
            inner: Arc::new(Inner {
                exe,
                env,
                opts,
                slots: Slots::default(),
                live: Mutex::new(Live::default()),
                drivers: Mutex::new(None),
                config_files: Mutex::new(Vec::new()),
            }),
        })
    }

    pub fn opts(&self) -> &Options {
        &self.inner.opts
    }

    pub fn auto(&self) -> Limits {
        Limits::new(self.inner.opts.timeout, self.inner.opts.max_output)
    }

    pub fn user(&self) -> Limits {
        Limits::new(self.inner.opts.user_timeout, self.inner.opts.max_output)
    }

    /// A variable of the filtered environment children start from.
    pub fn env_var(&self, key: &str) -> Option<&OsStr> {
        self.inner
            .env
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_os_str())
    }

    /// Kill every running git of this repo (all clones of the handle) with its process group,
    /// and refuse later runs with [`GitError::ShutDown`].
    pub fn shutdown(&self) {
        let mut live = self.inner.live.lock().unwrap();
        live.closed = true;
        for &pid in &live.groups {
            kill_group(pid);
        }
    }

    /// Remember which config files may define drivers, once the repo's git dirs are known.
    pub fn watch_config(&self, git_dir: &Path, common_dir: &Path) {
        let mut files = vec![common_dir.join("config"), git_dir.join("config.worktree")];
        let get = |k: &str| {
            self.inner
                .env
                .iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| PathBuf::from(v))
        };
        if let Some(global) = get("GIT_CONFIG_GLOBAL") {
            files.push(global);
        }
        if let Some(home) = get("HOME") {
            files.push(home.join(".gitconfig"));
            files.push(home.join(".config/git/config"));
        }
        if let Some(xdg) = get("XDG_CONFIG_HOME") {
            files.push(xdg.join("git/config"));
        }
        files.push(PathBuf::from("/etc/gitconfig"));
        *self.inner.config_files.lock().unwrap() = files;
        *self.inner.drivers.lock().unwrap() = None;
    }

    /// Run git in `cwd`; a non-zero exit is returned in [`Output::code`], not as an error.
    pub fn run(
        &self,
        cwd: &Path,
        args: &[&OsStr],
        stdin: Option<&[u8]>,
        lim: Limits,
    ) -> Result<Output> {
        self.run_ctl(cwd, args, stdin, lim, &Ctl::default())
    }

    /// [`Git::run`] with cancel, inactivity limit and progress lines.
    pub fn run_ctl(
        &self,
        cwd: &Path,
        args: &[&OsStr],
        stdin: Option<&[u8]>,
        lim: Limits,
        ctl: &Ctl,
    ) -> Result<Output> {
        // Writes run like terminal git (accepted T18); only reads blank the driver programs.
        let drivers = if lim.profile == Profile::Read {
            self.driver_config(cwd)?
        } else {
            Vec::new()
        };
        self.spawn(cwd, args, stdin, lim, &drivers, ctl)
    }

    /// Like [`Git::run`] but a non-zero exit becomes [`GitError::Failed`].
    pub fn run_ok(
        &self,
        cwd: &Path,
        args: &[&OsStr],
        stdin: Option<&[u8]>,
        lim: Limits,
    ) -> Result<Vec<u8>> {
        let out = self.run(cwd, args, stdin, lim)?;
        if out.code != 0 {
            return Err(GitError::Failed {
                cmd: cmd_name(args),
                code: out.code,
                stderr: out.stderr,
            });
        }
        Ok(out.stdout)
    }

    /// Blank the program keys of every configured filter/diff driver. An empty
    /// `filter.<d>.process` / `clean` / `smudge` makes git skip the driver, and `required=false`
    /// keeps a required one from failing the read (proved by the hostile-config tests).
    fn driver_config(&self, cwd: &Path) -> Result<Vec<(OsString, OsString)>> {
        let stamp = self.config_stamp();
        if let Some(d) = self.inner.drivers.lock().unwrap().as_ref()
            && !stamp.is_empty()
            && d.stamp == stamp
        {
            return Ok(d.config.clone());
        }
        let args: [&OsStr; 4] = [
            "config".as_ref(),
            "--null".as_ref(),
            "--get-regexp".as_ref(),
            DRIVER_KEYS.as_ref(),
        ];
        let out = self.spawn(cwd, &args, None, self.auto(), &[], &Ctl::default())?;
        // Exit 1 = no such key; anything else unexpected is an error rather than an unprotected run.
        if out.code != 0 && out.code != 1 {
            return Err(GitError::Failed {
                cmd: "config".into(),
                code: out.code,
                stderr: out.stderr,
            });
        }
        let mut config = Vec::new();
        let mut seen: Vec<Vec<u8>> = Vec::new();
        for rec in out.stdout.split(|&b| b == 0).filter(|r| !r.is_empty()) {
            let key = rec.split(|&b| b == b'\n').next().unwrap_or_default();
            let Some(first) = key.iter().position(|&b| b == b'.') else {
                continue;
            };
            let Some(last) = key.iter().rposition(|&b| b == b'.') else {
                continue;
            };
            if last <= first {
                continue;
            }
            let section_and_name = key[..last].to_vec();
            if seen.contains(&section_and_name) {
                continue;
            }
            let blank: &[&str] = if key.starts_with(b"filter.") {
                &["clean", "smudge", "process"]
            } else {
                &["textconv", "command"]
            };
            for k in blank {
                config.push((join_key(&section_and_name, k), OsString::new()));
            }
            if key.starts_with(b"filter.") {
                config.push((
                    join_key(&section_and_name, "required"),
                    OsString::from("false"),
                ));
            }
            seen.push(section_and_name);
        }
        *self.inner.drivers.lock().unwrap() = Some(Drivers {
            stamp,
            config: config.clone(),
        });
        Ok(config)
    }

    fn config_stamp(&self) -> ConfigStamp {
        let files = self.inner.config_files.lock().unwrap();
        files
            .iter()
            .map(|f| {
                std::fs::metadata(f)
                    .ok()
                    .map(|m| (m.modified().unwrap_or(SystemTime::UNIX_EPOCH), m.len()))
            })
            .collect()
    }

    fn spawn(
        &self,
        cwd: &Path,
        args: &[&OsStr],
        stdin: Option<&[u8]>,
        lim: Limits,
        drivers: &[(OsString, OsString)],
        ctl: &Ctl,
    ) -> Result<Output> {
        let _slot = self.acquire();
        if self.inner.live.lock().unwrap().closed {
            return Err(GitError::ShutDown);
        }
        let profile = lim.profile;
        let mut cmd = Command::new(&self.inner.exe);
        cmd.current_dir(cwd).env_clear().envs(
            self.inner
                .env
                .iter()
                .filter(|(k, _)| !profile_drops(profile, k))
                .map(|(k, v)| (k, v)),
        );
        let extra: &[(&str, &str)] = if profile == Profile::Read {
            &[]
        } else {
            WRITE_CONFIG
        };
        let config: Vec<(OsString, OsString)> = FIXED_CONFIG
            .iter()
            .filter(|(k, _)| !config_dropped(profile, k))
            .chain(extra)
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .chain(drivers.iter().cloned())
            .collect();
        // GIT_CONFIG_COUNT carries keys verbatim, so driver names with '=' or spaces stay safe
        // (a `-c` argument would split at the first '=').
        cmd.env("GIT_CONFIG_COUNT", config.len().to_string());
        for (i, (k, v)) in config.iter().enumerate() {
            cmd.env(format!("GIT_CONFIG_KEY_{i}"), k)
                .env(format!("GIT_CONFIG_VALUE_{i}"), v);
        }
        if profile == Profile::Network {
            // ssh must not start a graphical askpass either (SEC-13).
            cmd.env("SSH_ASKPASS_REQUIRE", "never");
        } else {
            // Beats every protocol.* config key; "none" names no transport.
            cmd.env("GIT_ALLOW_PROTOCOL", "none");
        }
        cmd.envs(ctl.env.iter().map(|(k, v)| (k, v)));
        cmd.arg("--no-pager");
        if matches!(profile, Profile::Read | Profile::Index) {
            cmd.arg("--literal-pathspecs");
        }
        cmd.args(args);
        cmd.stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
        // SAFETY: setsid is async-signal-safe and touches no memory of the parent.
        unsafe {
            cmd.pre_exec(|| {
                rustix::process::setsid()
                    .map(|_| ())
                    .map_err(std::io::Error::from)
            });
        }
        let name = cmd_name(args);
        let mut child = cmd.spawn()?;
        let group = Pid::from_child(&child);
        {
            // Registered under the same lock shutdown() takes, so no git escapes it.
            let mut live = self.inner.live.lock().unwrap();
            if live.closed {
                drop(live);
                kill_group(group);
                let _ = child.wait();
                return Err(GitError::ShutDown);
            }
            live.groups.push(group);
        }

        let writer = match (stdin, child.stdin.take()) {
            (Some(data), Some(mut pipe)) => {
                let data = data.to_vec();
                Some(std::thread::spawn(move || {
                    let _ = pipe.write_all(&data);
                }))
            }
            _ => None,
        };
        let started = Instant::now();
        // Milliseconds after `started` of the last byte git printed (inactivity limit).
        let activity = Arc::new(AtomicU64::new(0));
        let mut err_pipe = child.stderr.take().expect("piped stderr");
        let (err_activity, on_line) = (activity.clone(), ctl.on_line.clone());
        let err_reader = std::thread::spawn(move || {
            let mut kept = Vec::new();
            let mut line = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                match err_pipe.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        err_activity.store(started.elapsed().as_millis() as u64, Ordering::SeqCst);
                        let room = STDERR_CAP.saturating_sub(kept.len());
                        kept.extend_from_slice(&buf[..n.min(room)]);
                        if let Some(sink) = &on_line {
                            // Progress meters redraw with CR: each redraw is a line.
                            for &b in &buf[..n] {
                                if b == b'\n' || b == b'\r' {
                                    if !line.is_empty() {
                                        sink(&line);
                                        line.clear();
                                    }
                                } else if line.len() < 4096 {
                                    line.push(b);
                                }
                            }
                        }
                    }
                }
            }
            if let Some(sink) = &on_line
                && !line.is_empty()
            {
                sink(&line);
            }
            kept
        });
        let mut out_pipe = child.stdout.take().expect("piped stdout");
        let (tx, rx) = mpsc::channel();
        let cap = lim.max_output;
        let out_activity = activity.clone();
        let out_reader = std::thread::spawn(move || {
            let mut out = Vec::new();
            let mut buf = [0u8; 8192];
            let res = loop {
                match out_pipe.read(&mut buf) {
                    Ok(0) => break Ok(out),
                    Ok(n) => {
                        out_activity.store(started.elapsed().as_millis() as u64, Ordering::SeqCst);
                        if out.len() + n > cap {
                            break Err(None);
                        }
                        out.extend_from_slice(&buf[..n]);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(e) => break Err(Some(e.to_string())),
                }
            };
            let _ = tx.send(res);
        });

        let tick = Duration::from_millis(50);
        let stdout = loop {
            let elapsed = started.elapsed();
            if elapsed >= lim.timeout {
                break Err(GitError::Timeout(name.clone()));
            }
            if ctl.cancel.as_ref().is_some_and(Cancel::is_cancelled) {
                break Err(GitError::Cancelled);
            }
            if let Some(idle) = ctl.idle {
                let last = Duration::from_millis(activity.load(Ordering::SeqCst));
                if elapsed.saturating_sub(last) >= idle {
                    break Err(GitError::Timeout(name.clone()));
                }
            }
            match rx.recv_timeout(tick.min(lim.timeout - elapsed)) {
                Ok(Ok(out)) => break Ok(out),
                Ok(Err(None)) => {
                    break Err(GitError::TooLarge {
                        cmd: name.clone(),
                        limit: cap,
                    });
                }
                Ok(Err(Some(e))) => break Err(GitError::Io(e)),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    break Err(GitError::Io("output reader stopped".into()));
                }
            }
        };
        if stdout.is_err() {
            kill_group(group);
        }
        let status = child.wait();
        let shut_down = {
            let mut live = self.inner.live.lock().unwrap();
            live.groups.retain(|&g| g != group);
            live.closed
        };
        // Anything git left in its session (a hung helper) goes with it.
        kill_group(group);
        let status = status?;
        let _ = out_reader.join();
        let stderr = err_reader.join().unwrap_or_default();
        if let Some(w) = writer {
            let _ = w.join();
        }
        if shut_down {
            return Err(GitError::ShutDown);
        }
        let stdout = stdout?;
        Ok(Output {
            code: status.code().unwrap_or(-1),
            stdout,
            stderr: String::from_utf8_lossy(&stderr).trim_end().to_string(),
        })
    }

    fn acquire(&self) -> SlotGuard<'_> {
        let slots = &self.inner.slots;
        let max = self.inner.opts.max_concurrent.max(1);
        let mut busy = slots.busy.lock().unwrap();
        while *busy >= max {
            busy = slots.freed.wait(busy).unwrap();
        }
        *busy += 1;
        SlotGuard(slots)
    }
}

struct SlotGuard<'a>(&'a Slots);

impl Drop for SlotGuard<'_> {
    fn drop(&mut self) {
        *self.0.busy.lock().unwrap() -= 1;
        self.0.freed.notify_one();
    }
}

fn kill_group(group: Pid) {
    let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
}

fn join_key(section_and_name: &[u8], key: &str) -> OsString {
    let mut v = section_and_name.to_vec();
    v.push(b'.');
    v.extend_from_slice(key.as_bytes());
    OsString::from_vec(v)
}

fn cmd_name(args: &[&OsStr]) -> String {
    args.first()
        .map(|a| a.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Outer variables a profile does not pass on: commit and push/pull run hooks, which must see
/// ordinary pathspecs; push/pull must not reach an askpass program (SEC-13).
fn profile_drops(profile: Profile, key: &OsStr) -> bool {
    let k = key.as_bytes();
    match profile {
        Profile::Read | Profile::Index => false,
        Profile::Commit => k == b"GIT_LITERAL_PATHSPECS",
        Profile::Network => matches!(
            k,
            b"GIT_LITERAL_PATHSPECS" | b"SSH_ASKPASS" | b"GIT_ASKPASS" | b"SSH_ASKPASS_REQUIRE"
        ),
    }
}

/// Forced read config a write profile leaves to the user: hooks run on writes (D298(1)), and
/// push/pull need transports.
fn config_dropped(profile: Profile, key: &str) -> bool {
    match profile {
        Profile::Read => false,
        Profile::Index | Profile::Commit => key == "core.hooksPath",
        Profile::Network => key == "core.hooksPath" || key == "protocol.allow",
    }
}

/// Find `git` in `PATH`, skipping empty and relative entries so a `git` inside the opened repo
/// (the child's cwd) can never be picked.
fn find_git(env: &[(OsString, OsString)]) -> Option<PathBuf> {
    let path = env
        .iter()
        .find(|(k, _)| k == "PATH")
        .map(|(_, v)| v.clone())?;
    path.as_bytes()
        .split(|&b| b == b':')
        .map(|d| PathBuf::from(OsStr::from_bytes(d)))
        .filter(|d| d.is_absolute())
        .map(|d| d.join("git"))
        .find(|f| {
            f.metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
}

/// The child environment: outer variables minus every `GIT_*` not in [`GIT_ENV_ALLOW`], plus
/// [`FIXED_ENV`], minus [`TTY_HINT_ENV`]. The repo is chosen by the child's cwd only.
pub(crate) fn child_env(base: &[(OsString, OsString)]) -> Vec<(OsString, OsString)> {
    let mut env: Vec<(OsString, OsString)> = base
        .iter()
        .filter(|(k, _)| {
            let k = k.as_bytes();
            !k.starts_with(b"GIT_") || GIT_ENV_ALLOW.iter().any(|a| a.as_bytes() == k)
        })
        .filter(|(k, _)| !FIXED_ENV.iter().any(|(f, _)| f.as_bytes() == k.as_bytes()))
        .filter(|(k, _)| !TTY_HINT_ENV.iter().any(|h| h.as_bytes() == k.as_bytes()))
        .cloned()
        .collect();
    env.extend(
        FIXED_ENV
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v))),
    );
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect()
    }

    #[test]
    fn child_env_drops_outer_git_vars_except_the_allowlist() {
        let out = child_env(&env(&[
            ("PATH", "/usr/bin"),
            ("GIT_DIR", "/x"),
            ("GIT_WORK_TREE", "/x"),
            ("GIT_INDEX_FILE", "/x"),
            ("GIT_CONFIG_PARAMETERS", "'core.fsmonitor'='x'"),
            ("GIT_CONFIG_COUNT", "1"),
            ("GIT_TRACE", "/tmp/t"),
            ("GIT_EXTERNAL_DIFF", "x"),
            ("GIT_TERMINAL_PROMPT", "1"),
            ("GIT_CONFIG_GLOBAL", "/h/.gitconfig"),
            ("GIT_AUTHOR_NAME", "A"),
        ]));
        let keys: Vec<String> = out
            .iter()
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        for gone in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_CONFIG_PARAMETERS",
            "GIT_TRACE",
            "GIT_EXTERNAL_DIFF",
        ] {
            assert!(!keys.contains(&gone.to_string()), "{gone} must be removed");
        }
        assert!(keys.contains(&"GIT_CONFIG_GLOBAL".to_string()));
        assert!(keys.contains(&"PATH".to_string()));
        let prompt: Vec<_> = out
            .iter()
            .filter(|(k, _)| k == "GIT_TERMINAL_PROMPT")
            .collect();
        assert_eq!(prompt.len(), 1);
        assert_eq!(prompt[0].1, "0");
        assert!(
            out.iter()
                .any(|(k, v)| k == "GIT_OPTIONAL_LOCKS" && v == "0")
        );
    }

    #[test]
    fn find_git_skips_relative_and_empty_path_entries() {
        let dir = tempfile::tempdir().unwrap();
        let rel = dir.path().join("rel");
        std::fs::create_dir(&rel).unwrap();
        let shim = rel.join("git");
        std::fs::write(&shim, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
        let found = find_git(&env(&[("PATH", ".::rel:/usr/bin:/bin")]));
        let found = found.expect("system git");
        assert!(found.is_absolute());
        assert_ne!(found, shim);
    }

    #[test]
    fn child_env_never_passes_the_terminal_to_gpg() {
        let out = child_env(&env(&[
            ("PATH", "/usr/bin"),
            ("GPG_TTY", "/dev/pts/7"),
            ("GPG_AGENT_INFO", "/run/S.gpg-agent:0:1"),
            ("PINENTRY_USER_DATA", "USE_CURSES=1"),
        ]));
        let get = |k: &str| {
            out.iter()
                .filter(|(n, _)| n == k)
                .map(|(_, v)| v.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(get("GPG_TTY"), vec![OsString::from("/dev/null")]);
        assert!(get("GPG_AGENT_INFO").is_empty());
        assert!(get("PINENTRY_USER_DATA").is_empty());
    }
}
