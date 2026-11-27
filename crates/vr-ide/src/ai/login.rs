//! Sign-in inside vr-ide (AC-9): claude opens its login page through `$BROWSER`, which vr-ide
//! points at itself, and the page shows in a vr-ide login window with web data of that account
//! only (`<state>/ai-login/<tool>-<account>/`), so the cookies of one account never meet
//! another's or the user's browser.
//!
//! How claude calls it (claude 2.1.332, read from its binary, D63): `execFile($BROWSER, [url])`
//! and it waits for that process. The whole value is one program path: nothing is split into
//! words, no shell runs. So `BROWSER` is a per-account launcher, the link
//! `<state>/ai-login/bin/vr-ide-login.<tool>.<account>` to this executable ([`browser_env`]),
//! and `main` knows the launcher by that name plus exactly one argument.
//!
//! The launcher ([`run_launcher`], D64) never shows a window itself. A claude login start URL
//! ([`login_start`]) starts the window as a detached `vr-ide --login-window <tool> <account>
//! <url>` ([`run_helper`]), waits a few seconds for its "window is up" byte and returns, as
//! `xdg-open` would. Every other argument (an agent's `xdg-open`, `gh auth login`, an MCP
//! sign-in: `BROWSER` is inherited by all of claude's children) goes unchanged to the system
//! opener, with `BROWSER` removed so that the opener cannot call the launcher again (D6).
//!
//! The window loads only what [`allowed`] accepts (SEC-35), checked on the URL the engine is
//! about to load (SEC-36). Accepted remainder (T-44): any program of this user can open the
//! window for an account with a genuine login URL of its own; the title names the account.

use std::ffi::{OsStr, OsString};
use std::os::fd::OwnedFd;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use super::accounts;

/// The folder of the web data folders, their locks and the launchers, under the state folder.
pub const DIR: &str = "ai-login";
/// The launchers' folder inside [`DIR`].
pub const BIN_DIR: &str = "bin";
/// A launcher is named `vr-ide-login.<tool>.<account>`.
pub const LAUNCHER_PREFIX: &str = "vr-ide-login.";
/// Longest URL the window takes (SEC-34).
pub const URL_MAX: usize = 8 * 1024;
/// A login window closes on its own after this long (SEC-34).
pub const LIFETIME: Duration = Duration::from_secs(15 * 60);
/// How long the launcher waits for the window to come up before it returns (D64).
pub const START_WAIT: Duration = Duration::from_secs(5);
/// After the callback was requested the window closes at the next loaded page, or after this.
pub const CALLBACK_GRACE: Duration = Duration::from_secs(5);
/// Built with the `login-window` feature.
pub const ENABLED: bool = cfg!(feature = "login-window");

/// The tools whose sign-in goes through the window. Fixed here (SEC-33): no setting adds one.
pub const TOOLS: [&str; 1] = ["claude"];

pub fn tool_ok(tool: &str) -> bool {
    TOOLS.contains(&tool)
}

/// `default`, or a name [`accounts::validate`] takes (SEC-15).
pub fn account_ok(account: &str) -> bool {
    account == accounts::DEFAULT || accounts::validate(account).is_ok()
}

/// The URL as the allow-list reads it: parsed the way a browser engine does (WHATWG). Refused
/// before parsing: over-long text, control characters, blanks, and `\`, which engines read as
/// `/` in the host part (`https://claude.ai\@evil.com`).
fn parse(url: &str) -> Option<url::Url> {
    let odd = |c: char| c.is_control() || c.is_whitespace() || c == '\\';
    if url.len() > URL_MAX || url.chars().any(odd) {
        return None;
    }
    url::Url::parse(url).ok()
}

/// Where claude's sign-in starts and ends. `claude.com` and `platform.claude.com` are the
/// hosts claude 2.1.332 itself opens (`https://claude.com/cai/oauth/authorize`,
/// `https://platform.claude.com/oauth/authorize`) and returns to
/// (`https://platform.claude.com/oauth/code/success`); `*.anthropic.com` is dot-anchored and
/// the apex is not needed.
fn first_party(host: &str) -> bool {
    matches!(host, "claude.ai" | "claude.com" | "platform.claude.com")
        || host
            .strip_suffix(".anthropic.com")
            .is_some_and(|sub| !sub.is_empty())
}

/// The identity providers of claude's login page (Google, Apple), by exact host.
fn identity_provider(host: &str) -> bool {
    matches!(host, "accounts.google.com" | "appleid.apple.com")
}

/// `http://localhost:<port>` or `http://127.0.0.1:<port>`, written exactly so: `127.1`, a
/// decimal address or another spelling an engine would also read as this machine is refused,
/// like every other local name (`0.0.0.0`, `[::1]`, `localhost.x`).
fn local_callback(raw: &str, u: &url::Url, port: u16) -> bool {
    let host = match u.host() {
        Some(url::Host::Domain("localhost")) => true,
        Some(url::Host::Ipv4(ip)) => ip == std::net::Ipv4Addr::LOCALHOST,
        _ => false,
    };
    let exact = |name: &str| {
        raw.strip_prefix(&format!("http://{name}:{port}"))
            .is_some_and(|rest| rest.is_empty() || rest.starts_with(['/', '?', '#']))
    };
    host && u.port() == Some(port) && (exact("localhost") || exact("127.0.0.1"))
}

fn allowed_url(raw: &str, u: &url::Url, callback_port: Option<u16>) -> bool {
    if !u.username().is_empty() || u.password().is_some() {
        return false;
    }
    match (u.scheme(), u.host()) {
        // A domain only (never an address), on the default port; a trailing dot, a longer or
        // shorter name and a punycode lookalike are other hosts.
        ("https", Some(url::Host::Domain(host))) => {
            u.port().is_none() && (first_party(host) || identity_provider(host))
        }
        ("http", Some(_)) => callback_port.is_some_and(|port| local_callback(raw, u, port)),
        _ => false,
    }
}

/// SEC-35: may the login window load `url`? Only `https` on `claude.ai`, `claude.com`,
/// `platform.claude.com`, `*.anthropic.com`, `accounts.google.com` and `appleid.apple.com`
/// (no user name, no port), and `http://localhost:<port>` / `http://127.0.0.1:<port>` for the
/// one callback port of the start URL. Every other scheme, host and port is refused.
pub fn allowed(url: &str, callback_port: Option<u16>) -> bool {
    parse(url).is_some_and(|u| allowed_url(url, &u, callback_port))
}

/// A claude login start URL and the port of the local callback it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Start {
    /// As received; the window's own check sees what the engine makes of it.
    pub url: String,
    /// From `redirect_uri` alone; `None`: no local URL loads at all.
    pub port: Option<u16>,
}

/// SEC-34: `url` as the start of a claude sign-in, the only thing a login window opens for:
/// an allowed `https` URL on one of claude's own hosts whose path ends in `/oauth/authorize`.
/// The callback port is the one in its `redirect_uri` when that is `http://localhost:<port>/…`
/// or `http://127.0.0.1:<port>/…` with a port from 1024 up. More than one `redirect_uri` is no
/// start: which one the server reads is not known here.
pub fn login_start(url: &str) -> Option<Start> {
    let u = parse(url)?;
    let Some(url::Host::Domain(host)) = u.host() else {
        return None;
    };
    let start = u.scheme() == "https"
        && allowed_url(url, &u, None)
        && first_party(host)
        && u.path().ends_with("/oauth/authorize");
    if !start {
        return None;
    }
    let mut redirects = u.query_pairs().filter(|(key, _)| key == "redirect_uri");
    let first = redirects.next();
    if redirects.next().is_some() {
        return None;
    }
    Some(Start {
        url: url.to_string(),
        port: first.and_then(|(_, redirect)| callback_port(&redirect)),
    })
}

fn callback_port(redirect: &str) -> Option<u16> {
    let u = parse(redirect)?;
    let port = u.port().filter(|p| *p >= 1024)?;
    let plain = u.scheme() == "http" && u.username().is_empty() && u.password().is_none();
    (plain && local_callback(redirect, &u, port)).then_some(port)
}

/// What the window does with a navigation the engine is about to make (SEC-36).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Nav {
    Allow,
    /// Allowed, and it is the local callback: the sign-in is done.
    Callback,
    /// Cancelled; the text is [`label`], safe to show.
    Refuse(String),
}

pub fn decide(url: &str, callback_port: Option<u16>) -> Nav {
    match parse(url) {
        Some(u) if allowed_url(url, &u, callback_port) => {
            if u.scheme() == "http" {
                Nav::Callback
            } else {
                Nav::Allow
            }
        }
        _ => Nav::Refuse(label(url)),
    }
}

/// How a refused URL is named (SEC-34, T-46): scheme and host only, never the path or query
/// (they carry the OAuth `code` and `state`), plain ASCII, capped.
pub fn label(url: &str) -> String {
    const MAX: usize = 80;
    let Some(u) = parse(url) else {
        return "invalid URL".into();
    };
    let text = match u.host_str() {
        Some(host) => format!("{}://{host}", u.scheme()),
        None => format!("{}:", u.scheme()),
    };
    text.chars()
        .filter(|c| c.is_ascii_alphanumeric() || ".:/+-_[]".contains(*c))
        .take(MAX)
        .collect()
}

/// At most one refusal line per second (SEC-34).
#[derive(Debug, Default)]
pub struct Throttle(Option<Instant>);

impl Throttle {
    pub fn pass(&mut self, now: Instant) -> bool {
        let quiet = self
            .0
            .is_none_or(|last| now.saturating_duration_since(last) >= Duration::from_secs(1));
        if quiet {
            self.0 = Some(now);
        }
        quiet
    }
}

/// Linux needs `DISPLAY` or `WAYLAND_DISPLAY`; elsewhere a window can always be asked for.
pub fn display_in(display: Option<&OsStr>, wayland: Option<&OsStr>) -> bool {
    let set = |v: Option<&OsStr>| v.is_some_and(|v| !v.is_empty());
    !cfg!(target_os = "linux") || set(display) || set(wayland)
}

pub fn display_available() -> bool {
    display_in(
        std::env::var_os("DISPLAY").as_deref(),
        std::env::var_os("WAYLAND_DISPLAY").as_deref(),
    )
}

const NOT_VALID: &str = "tool or account not valid";
const FOLDER: &str = "login folder unusable";
const BUSY: &str = "a login window for this account is open: close it, then run /login again";

/// The account as the launcher and the window name it: `default` is the tool's own config.
fn account_opt(account: &str) -> Option<Option<&str>> {
    if account == accounts::DEFAULT {
        return Some(None);
    }
    accounts::validate(account).ok().map(Some)
}

/// `<tool>-<account|default>`: the one name an account's web data, lock and launcher go by.
/// Both parts are checked here, so it is always one plain path component.
fn slot(tool: &str, account: Option<&str>) -> Option<String> {
    let name = match account {
        None => accounts::DEFAULT,
        Some(a) => accounts::validate(a).ok()?,
    };
    tool_ok(tool).then(|| format!("{tool}-{name}"))
}

/// Makes `dir` 0700 and checks it with [`accounts::secure`] (a real folder of this user; a
/// link is refused before anything is made behind it).
fn private_dir(dir: &Path) -> Result<(), &'static str> {
    if std::fs::symlink_metadata(dir).is_ok() {
        accounts::secure(dir).map_err(|_| FOLDER)?;
    }
    crate::recovery::create_private_dir(dir).map_err(|_| FOLDER)?;
    accounts::secure(dir).map_err(|_| FOLDER)
}

/// SEC-37: the web data folder of `account` (`None`: default), `<state>/ai-login/<tool>-
/// <account>`, made 0700 and checked now. All website data of the window goes below it.
pub fn data_dir(state: &Path, tool: &str, account: Option<&str>) -> Result<PathBuf, &'static str> {
    let slot = slot(tool, account).ok_or(NOT_VALID)?;
    let root = state.join(DIR);
    private_dir(&root)?;
    let dir = root.join(slot);
    private_dir(&dir)?;
    Ok(dir)
}

/// A window's hold on its account (SEC-34): an exclusive lock on `<state>/ai-login/<tool>-
/// <account>.lock` for as long as the window lives. Removing the account needs the same lock.
#[derive(Debug)]
pub struct WindowLock(OwnedFd);

impl Drop for WindowLock {
    fn drop(&mut self) {
        accounts::unlock(&self.0);
    }
}

/// Takes the hold, at once or not at all: a second window for the account is refused.
pub fn lock(state: &Path, tool: &str, account: &str) -> Result<WindowLock, &'static str> {
    use rustix::fs::{AtFlags, FlockOperation, Mode, OFlags};
    let slot = account_opt(account)
        .and_then(|a| slot(tool, a))
        .ok_or(NOT_VALID)?;
    let root = state.join(DIR);
    private_dir(&root)?;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let dir = rustix::fs::open(&root, flags, Mode::empty()).map_err(|_| FOLDER)?;
    let name = format!("{slot}.lock");
    let flags = OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let fd = rustix::fs::openat(&dir, &name, flags, Mode::RUSR | Mode::WUSR).map_err(|_| FOLDER)?;
    rustix::fs::flock(&fd, FlockOperation::NonBlockingLockExclusive).map_err(|_| BUSY)?;
    let lock = WindowLock(fd);
    // A removal between the open and the lock unlinked this file: the hold is on nothing.
    let held = rustix::fs::fstat(&lock.0).map_err(|_| FOLDER)?;
    let named = rustix::fs::statat(&dir, &name, AtFlags::SYMLINK_NOFOLLOW);
    if !named.is_ok_and(|n| n.st_dev == held.st_dev && n.st_ino == held.st_ino) {
        return Err(BUSY);
    }
    Ok(lock)
}

/// `<state>/ai-login/bin/vr-ide-login.<tool>.<account|default>`.
pub fn launcher_path(state: &Path, tool: &str, account: Option<&str>) -> Option<PathBuf> {
    let name = match account {
        None => accounts::DEFAULT,
        Some(a) => accounts::validate(a).ok()?,
    };
    tool_ok(tool).then(|| {
        state
            .join(DIR)
            .join(BIN_DIR)
            .join(format!("{LAUNCHER_PREFIX}{tool}.{name}"))
    })
}

/// A path that may stand in `BROWSER` (SEC-33): absolute and made of `[A-Za-z0-9/._+-]` only.
/// Other programs that read `BROWSER` split it at blanks and `:`, expand `%s`, or hand it to a
/// shell; none of that can change such a path.
fn plain(p: &Path) -> Option<&str> {
    let ok = |c: char| c.is_ascii_alphanumeric() || "/._+-".contains(c);
    p.to_str().filter(|s| p.is_absolute() && s.chars().all(ok))
}

/// SEC-33 (D63): the `BROWSER` value for a claude session on `account`: the account's launcher,
/// a link to `exe` (this executable) in the private folder `<state>/ai-login/bin/`, made or
/// corrected now. For that session's environment only. An error (why the window cannot be
/// used) means: set no `BROWSER`; claude then prints the URL and asks for the code.
pub fn browser_env(
    state: &Path,
    exe: &Path,
    tool: &str,
    account: Option<&str>,
) -> Result<OsString, &'static str> {
    const EXE: &str = "vr-ide path unusable";
    const LAUNCHER: &str = "launcher unusable";
    if !ENABLED {
        return Err("built without the login window");
    }
    let link = launcher_path(state, tool, account).ok_or(NOT_VALID)?;
    plain(exe)
        .filter(|_| std::fs::metadata(exe).is_ok_and(|m| m.is_file()))
        .ok_or(EXE)?;
    plain(&link).ok_or("state folder path unusable")?;
    private_dir(&state.join(DIR))?;
    private_dir(&state.join(DIR).join(BIN_DIR))?;
    match std::fs::read_link(&link) {
        Ok(target) if target == exe => return Ok(link.into_os_string()),
        // Another program (vr-ide moved or updated): only a link is ever replaced.
        Ok(_) => std::fs::remove_file(&link).map_err(|_| LAUNCHER)?,
        Err(_) if std::fs::symlink_metadata(&link).is_ok() => return Err(LAUNCHER),
        Err(_) => {}
    }
    match std::os::unix::fs::symlink(exe, &link) {
        Ok(()) => Ok(link.into_os_string()),
        // Another window made it meanwhile.
        Err(_) if std::fs::read_link(&link).is_ok_and(|t| t == exe) => Ok(link.into_os_string()),
        Err(_) => Err(LAUNCHER),
    }
}

/// SEC-38: an inherited `BROWSER` that is a launcher of ours (or the explicit window form):
/// it belongs to one claude session and is dropped for everything else vr-ide starts.
pub fn is_helper_browser(value: &OsStr) -> bool {
    let text = value.to_string_lossy();
    let launcher = |part: &str| {
        Path::new(part)
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with(LAUNCHER_PREFIX))
    };
    text.contains(crate::cli::LOGIN_WINDOW_FLAG) || text.split(':').any(launcher)
}

/// The system opener, as claude would have used it without vr-ide.
const OPENER: &str = if cfg!(target_os = "macos") {
    "open"
} else {
    "xdg-open"
};

/// D6: `target` for the system opener, unchanged, as one argument and without a shell.
/// `BROWSER` is removed: `xdg-open` reads it too and would start the launcher again.
pub fn opener_command(program: &str, target: &OsStr) -> Command {
    let mut c = Command::new(program);
    c.arg(target).env_remove("BROWSER");
    c
}

/// Everything before the window, in the order that creates nothing for a refused start.
pub(crate) struct Prepared {
    pub start: Start,
    pub data: PathBuf,
    pub lock: WindowLock,
}

pub(crate) fn prepare(
    state: &Path,
    tool: &str,
    account: &str,
    url: &str,
    display: bool,
) -> Result<Prepared, &'static str> {
    let named = account_opt(account)
        .filter(|_| tool_ok(tool))
        .ok_or(NOT_VALID)?;
    let start = login_start(url).ok_or("not a claude login URL")?;
    if !display {
        return Err("no display");
    }
    if !ENABLED {
        return Err("built without the login window");
    }
    // The lock first: the folder is made only by the window that will use it.
    let lock = lock(state, tool, account)?;
    let data = data_dir(state, tool, named)?;
    Ok(Prepared { start, data, lock })
}

/// What the launcher does with its one argument.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Launch {
    /// A claude login start: the window, as this program.
    Window(PathBuf),
    /// Anything else: the system opener.
    Opener,
    Fail,
}

pub(crate) fn launch(
    tool: &str,
    account: &str,
    target: &OsStr,
    display: bool,
    exe: Option<&Path>,
) -> Launch {
    if !tool_ok(tool) || account_opt(account).is_none() {
        return Launch::Fail;
    }
    // `xdg-open` reads a leading `-` as an option and accepts no `--`: never pass one on.
    if target.as_encoded_bytes().first() == Some(&b'-') {
        return Launch::Fail;
    }
    if target.to_str().and_then(login_start).is_none() {
        return Launch::Opener;
    }
    if !ENABLED || !display {
        return Launch::Fail;
    }
    let usable = |e: &PathBuf| {
        e.is_absolute()
            && !e.to_string_lossy().ends_with(" (deleted)")
            && std::fs::metadata(e).is_ok_and(|m| m.is_file())
    };
    exe.map(Path::to_path_buf)
        .or_else(|| std::env::current_exe().ok())
        .filter(usable)
        .map_or(Launch::Fail, Launch::Window)
}

/// D64: starts the window as its own process (`<exe> --login-window <tool> <account> <url>`,
/// own process group, no terminal) and waits up to `wait` for its "window is up" byte: 0 on the
/// byte or when the wait ran out (still starting), 1 when it ended or could not start. claude,
/// which waits for the launcher, is neither held by the window nor able to end it.
pub(crate) fn spawn_window(
    exe: &Path,
    state: &Path,
    tool: &str,
    account: &str,
    url: &str,
    wait: Duration,
) -> i32 {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    use rustix::io::Errno;
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    let spawned = Command::new(exe)
        .arg(crate::cli::LOGIN_WINDOW_FLAG)
        .args([tool, account, url])
        // The window process works on this window's state folder, whatever it would find.
        .env("VR_IDE_STATE_DIR", state)
        .env_remove("BROWSER")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn();
    let Ok(mut child) = spawned else {
        return 1;
    };
    let Some(out) = child.stdout.take() else {
        return 1;
    };
    let deadline = Instant::now() + wait;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return 0;
        }
        let timeout = Timespec {
            tv_sec: i64::try_from(left.as_secs()).unwrap_or(i64::MAX),
            tv_nsec: i64::from(left.subsec_nanos()),
        };
        let mut fds = [PollFd::new(&out, PollFlags::IN)];
        match poll(&mut fds, Some(&timeout)) {
            Ok(0) | Err(Errno::INTR) => continue,
            Ok(_) => {}
            Err(_) => return 1,
        }
        let mut byte = [0u8; 1];
        match rustix::io::read(&out, &mut byte) {
            Ok(0) => {
                let _ = child.try_wait();
                return 1;
            }
            Ok(_) => return 0,
            Err(Errno::INTR | Errno::AGAIN) => {}
            Err(_) => return 1,
        }
    }
}

/// The launcher (`$BROWSER <argument>`, run by claude or by any program below it): a claude
/// login start opens the login window of this launcher's account; anything else is handed to
/// the system opener, which replaces this process. The exit code is the opener's, 0 once the
/// window is coming up, or 1 (claude then shows the URL and asks for the code).
pub fn run_launcher(state: &Path, tool: &str, account: &str, target: &OsStr) -> i32 {
    match launch(tool, account, target, display_available(), None) {
        Launch::Fail => 1,
        Launch::Opener => {
            use std::os::unix::process::CommandExt;
            let failed = opener_command(OPENER, target).exec();
            eprintln!("vr-ide: cannot run {OPENER}: {failed}");
            1
        }
        Launch::Window(exe) => {
            let url = target.to_str().unwrap_or_default();
            spawn_window(&exe, state, tool, account, url, START_WAIT)
        }
    }
}

/// The window process (`vr-ide --login-window <tool> <account> <url>`): 1 without a window
/// when the URL is no claude login start, there is no display, the build has no web view, the
/// account already has a window, or the web view fails; otherwise the window, until the
/// callback page was requested (0), it is closed, or [`LIFETIME`] passed (1).
pub fn run_helper(tool: &str, account: &str, url: &str) -> i32 {
    run_window(
        &vr_config::state_dir(),
        tool,
        account,
        url,
        display_available(),
    )
}

fn run_window(state: &Path, tool: &str, account: &str, url: &str, display: bool) -> i32 {
    match prepare(state, tool, account, url, display) {
        Ok(prepared) => show(prepared, tool, account),
        Err(why) => {
            eprintln!("vr-ide: login window not opened ({why})");
            1
        }
    }
}

#[cfg(not(feature = "login-window"))]
fn show(prepared: Prepared, _tool: &str, _account: &str) -> i32 {
    // Not reached: `prepare` refuses in a build without the window.
    let Prepared { start, data, lock } = prepared;
    drop((start, data, lock));
    1
}

#[cfg(feature = "login-window")]
fn show(prepared: Prepared, tool: &str, account: &str) -> i32 {
    // The title always names the tool and the account (SEC-34, T-44).
    let who = super::usage::account_name(tool, account_opt(account).flatten());
    window::run(prepared, format!("vr-ide login - {who}"))
}

/// The window itself: one tao window holding one wry web view. Checked by hand (task notes);
/// everything it decides is [`decide`], which the tests cover.
#[cfg(feature = "login-window")]
mod window {
    use std::io::Write;
    use std::time::Instant;

    use tao::event::{Event, WindowEvent};
    use tao::event_loop::{ControlFlow, EventLoopBuilder};
    use tao::window::WindowBuilder;
    use wry::{NewWindowResponse, PageLoadEvent, PermissionResponse, WebContext, WebViewBuilder};

    use super::{CALLBACK_GRACE, LIFETIME, Nav, Prepared, Throttle, decide};

    /// From the web view's callbacks to the window's loop.
    enum Signal {
        Refused(String),
        /// A new-window request for an allowed URL: loaded in the same view.
        Open(String),
        Callback,
        Loaded,
    }

    pub(super) fn run(prepared: Prepared, title: String) -> i32 {
        // The toolkit panics when it cannot start (no usable display after all): that is
        // "no window", exit 1, like every other failure before the window shows.
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| open(prepared, title)))
            .unwrap_or(1)
    }

    fn open(prepared: Prepared, title: String) -> i32 {
        let Prepared { start, data, lock } = prepared;
        let port = start.port;
        let event_loop = EventLoopBuilder::<Signal>::with_user_event().build();
        let built = WindowBuilder::new()
            .with_title(&title)
            .with_inner_size(tao::dpi::LogicalSize::new(520.0, 760.0))
            .build(&event_loop);
        let Ok(window) = built else {
            return 1;
        };
        // SEC-37: cookies, storage and cache all live below the account's folder.
        let mut context = WebContext::new(Some(data));
        let (navigation, popup, loaded) = (
            event_loop.create_proxy(),
            event_loop.create_proxy(),
            event_loop.create_proxy(),
        );
        // SEC-36: no IPC handler, no custom protocol, no initialization script; every
        // navigation the engine asks about (redirects and frames included) and every new-window
        // request is decided by the allow-list; downloads and permission requests are refused.
        let builder = WebViewBuilder::new_with_web_context(&mut context)
            .with_url(start.url)
            .with_devtools(cfg!(debug_assertions))
            .with_clipboard(false)
            .with_navigation_handler(move |url| match decide(&url, port) {
                Nav::Allow => true,
                Nav::Callback => {
                    let _ = navigation.send_event(Signal::Callback);
                    true
                }
                Nav::Refuse(what) => {
                    let _ = navigation.send_event(Signal::Refused(what));
                    false
                }
            })
            .with_new_window_req_handler(move |url, _| {
                let _ = popup.send_event(match decide(&url, port) {
                    Nav::Refuse(what) => Signal::Refused(what),
                    Nav::Allow | Nav::Callback => Signal::Open(url),
                });
                NewWindowResponse::Deny
            })
            .with_download_started_handler(|_, _| false)
            .with_permission_handler(|_| PermissionResponse::Deny)
            .with_on_page_load_handler(move |event, _| {
                if matches!(event, PageLoadEvent::Finished) {
                    let _ = loaded.send_event(Signal::Loaded);
                }
            });
        #[cfg(target_os = "linux")]
        let webview = {
            use tao::platform::unix::WindowExtUnix;
            use wry::WebViewBuilderExtUnix;
            let Some(vbox) = window.default_vbox() else {
                return 1;
            };
            builder.build_gtk(vbox)
        };
        #[cfg(not(target_os = "linux"))]
        let webview = builder.build(&window);
        let Ok(webview) = webview else {
            return 1;
        };
        // The launcher waits for this byte (D64).
        let mut out = std::io::stdout();
        let _ = out.write_all(b"1").and_then(|()| out.flush());

        let end = Instant::now() + LIFETIME;
        let mut close_at: Option<Instant> = None;
        let mut throttle = Throttle::default();
        event_loop.run(move |event, _, flow| {
            // Held until the process ends: the account's lock and the web data's context.
            let _held = (&lock, &context);
            let now = Instant::now();
            match event {
                Event::UserEvent(Signal::Refused(what)) => {
                    if throttle.pass(now) {
                        eprintln!("vr-ide login: refused {what}");
                        window.set_title(&format!("{title} - blocked {what}"));
                    }
                }
                Event::UserEvent(Signal::Open(url)) => {
                    let _ = webview.load_url(&url);
                }
                Event::UserEvent(Signal::Callback) => close_at = Some(now + CALLBACK_GRACE),
                // The first page that finishes after the callback was requested: claude has
                // answered it, the sign-in is done.
                Event::UserEvent(Signal::Loaded) if close_at.is_some() => close_at = Some(now),
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                } => {
                    *flow = ControlFlow::ExitWithCode(1);
                    return;
                }
                _ => {}
            }
            *flow = match close_at {
                Some(at) if now >= at => ControlFlow::ExitWithCode(0),
                _ if now >= end => ControlFlow::ExitWithCode(1),
                Some(at) => ControlFlow::WaitUntil(at.min(end)),
                None => ControlFlow::WaitUntil(end),
            };
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    const PORT: u16 = 54545;
    /// The form claude 2.1.332 opens (`CLAUDE_AI_AUTHORIZE_URL` + its query).
    const START: &str = "https://claude.com/cai/oauth/authorize?code=true&client_id=c&response_type=code&redirect_uri=http%3A%2F%2Flocalhost%3A54545%2Fcallback&scope=user%3Aprofile&state=s";

    fn mode(p: &Path) -> u32 {
        std::fs::symlink_metadata(p).unwrap().mode() & 0o777
    }

    fn fake_exe(dir: &Path, name: &str, text: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, text).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    /// SEC-35: the table of the threat model, plus the hosts claude 2.1.332 starts and ends on.
    #[test]
    fn allowed_urls() {
        for ok in [
            "https://claude.ai/login",
            "https://CLAUDE.AI/x",
            "https://claude.ai:443/x",
            "https://console.anthropic.com/oauth",
            "https://a.b.anthropic.com/",
            "https://accounts.google.com/o/oauth2",
            "https://appleid.apple.com/auth/authorize",
            "https://claude.com/cai/oauth/authorize?x=1",
            "https://platform.claude.com/oauth/code/success?app=claude-code",
            "http://localhost:54545/callback?code=x",
            "http://127.0.0.1:54545/callback?code=x",
            "http://localhost:54545",
        ] {
            assert!(allowed(ok, Some(PORT)), "{ok}");
        }
        let long = format!("https://claude.ai/{}", "a".repeat(URL_MAX));
        for bad in [
            "http://claude.ai/",
            "https://claude.ai.evil.com/",
            "https://evilanthropic.com/",
            "https://anthropic.com/",
            "https://x.anthropic.com.evil.com/",
            "https://claude.ai@evil.com/",
            "https://u:p@claude.ai/",
            "https://u@claude.ai/",
            "https://claude.ai%2F@evil.com/",
            "https://claude.ai\\@evil.com/",
            "https://claude.ai:8443/",
            "https://claude.ai./",
            "https://xn--laude-3ve.ai/",
            "https://\u{441}laude.ai/",
            "https://evil.com/?r=https://claude.ai/",
            "https://evil.com/#https://claude.ai/",
            "https://evilclaude.com/",
            "https://x.claude.com/",
            "https://claude.com.evil.com/",
            "https://accounts.google.com.evil.com/",
            "https://google.com/",
            "https://1.2.3.4/",
            "https://[::1]/",
            "http://localhost:1/",
            "http://localhost/",
            "http://localhost:54546/callback",
            "http://localhost.evil.com:54545/",
            "http://0.0.0.0:54545/",
            "http://[::1]:54545/",
            "http://127.1:54545/",
            "http://2130706433:54545/",
            "http://LOCALHOST:54545/",
            "http://u@localhost:54545/",
            "http://localhost:54545@evil.com/",
            "https://localhost:54545/",
            "javascript:alert(1)",
            "data:text/html,x",
            "file:///etc/passwd",
            "about:blank",
            "blob:https://claude.ai/x",
            "view-source:https://claude.ai/",
            "ws://claude.ai/",
            "wss://claude.ai/",
            "ftp://claude.ai/",
            "claude.ai",
            "//claude.ai/",
            "",
            " https://claude.ai/",
            "https://claude.ai/\n",
            "https://claude.ai/\u{1b}[31m",
            &long,
        ] {
            assert!(!allowed(bad, Some(PORT)), "{bad:?}");
        }
        // Without a callback port no local page loads at all.
        assert!(!allowed("http://localhost:54545/callback", None));
        assert!(!allowed("http://127.0.0.1:54545/callback", None));
        assert!(allowed("https://claude.ai/login", None));
    }

    /// SEC-34: only a claude login start URL opens a window; the callback port comes from its
    /// `redirect_uri` alone.
    #[test]
    fn start_url_and_callback_port() {
        let s = login_start(START).expect("start");
        assert_eq!(s.port, Some(PORT));
        assert_eq!(s.url, START);
        let with = |redirect: &str| {
            format!("https://claude.ai/oauth/authorize?client_id=c&redirect_uri={redirect}&state=s")
        };
        let port = |redirect: &str| login_start(&with(redirect)).map(|s| s.port);
        assert_eq!(
            port("http%3A%2F%2F127.0.0.1%3A40000%2Fcb"),
            Some(Some(40000))
        );
        assert_eq!(port("http://localhost:1024/callback"), Some(Some(1024)));
        assert_eq!(port("http://localhost:65535/callback"), Some(Some(65535)));
        // No usable local callback: the page may load, no local URL will.
        for none in [
            "https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback",
            "http://localhost:1023/callback",
            "http://localhost:80/callback",
            "http://localhost/callback",
            "http://localhost:99999/callback",
            "http://0.0.0.0:54545/callback",
            "http://[::1]:54545/callback",
            "http://127.1:54545/callback",
            "http://localhost.evil.com:54545/callback",
            "http://u@localhost:54545/callback",
            "https://localhost:54545/callback",
            "",
        ] {
            assert_eq!(port(none), Some(None), "{none}");
        }
        assert_eq!(
            login_start("https://platform.claude.com/oauth/authorize?x=1").map(|s| s.port),
            Some(None)
        );
        // Two `redirect_uri`: which one the server reads is not ours to guess.
        let twice = format!(
            "{}&redirect_uri=http://localhost:40001/cb",
            with("http://localhost:40000/cb")
        );
        assert_eq!(login_start(&twice), None);
        let long = format!("{START}&x={}", "a".repeat(URL_MAX));
        for not_a_start in [
            "https://evil.com/oauth/authorize?redirect_uri=http://localhost:54545/callback",
            "http://claude.ai/oauth/authorize",
            "https://claude.ai/upgrade",
            "https://claude.ai/",
            "https://claude.ai/oauth/authorize/x",
            "https://accounts.google.com/oauth/authorize",
            "https://appleid.apple.com/oauth/authorize",
            "https://claude.ai@evil.com/oauth/authorize",
            "http://localhost:54545/oauth/authorize",
            "file:///oauth/authorize",
            "javascript:alert(1)",
            "https://claude.ai/oauth/authorize\n",
            "",
            &long,
        ] {
            assert_eq!(login_start(not_a_start), None, "{not_a_start:?}");
        }
    }

    /// SEC-34, T-46: a refusal names scheme and host only; the path and query (OAuth `code`,
    /// `state`) and any escape character never show.
    #[test]
    fn refusals_show_scheme_and_host_only() {
        assert_eq!(
            decide("https://evil.com/p/SECRET?code=SECRET#SECRET", Some(PORT)),
            Nav::Refuse("https://evil.com".into())
        );
        assert_eq!(
            decide("http://localhost:1/x?code=SECRET", Some(PORT)),
            Nav::Refuse("http://localhost".into())
        );
        assert_eq!(
            decide("https://claude.ai/x?code=SECRET", Some(PORT)),
            Nav::Allow
        );
        assert_eq!(
            decide("http://localhost:54545/callback?code=SECRET", Some(PORT)),
            Nav::Callback
        );
        assert_eq!(
            decide("http://localhost:54545/callback?code=SECRET", None),
            Nav::Refuse("http://localhost".into())
        );
        for (url, shown) in [
            ("javascript:alert('SECRET')", "javascript:"),
            ("data:text/html,SECRET", "data:"),
            ("about:blank", "about:"),
            ("file:///home/SECRET", "file:"),
            ("https://u:SECRET@evil.com/", "https://evil.com"),
            ("https://evil.com/\u{1b}[31mSECRET", "invalid URL"),
            ("SECRET", "invalid URL"),
            ("", "invalid URL"),
            ("\u{1b}]0;SECRET\u{7}://x", "invalid URL"),
        ] {
            let l = label(url);
            assert_eq!(l, shown, "{url:?}");
            assert!(!l.contains("SECRET") && !l.chars().any(char::is_control));
        }
        let long = label(&format!("https://{}.com/", "a".repeat(300)));
        assert!(long.chars().count() <= 80, "{long}");
    }

    /// SEC-34: at most one refusal line per second.
    #[test]
    fn refusal_lines_are_rate_limited() {
        let mut t = Throttle::default();
        let t0 = Instant::now();
        assert!(t.pass(t0));
        assert!(!t.pass(t0));
        assert!(!t.pass(t0 + Duration::from_millis(999)));
        assert!(t.pass(t0 + Duration::from_millis(1000)));
        assert!(!t.pass(t0 + Duration::from_millis(1500)));
    }

    #[test]
    fn display_needs_a_variable_on_linux() {
        let set = Some(OsStr::new(":0"));
        let empty = Some(OsStr::new(""));
        assert!(display_in(set, None));
        assert!(display_in(None, Some(OsStr::new("wayland-0"))));
        if cfg!(target_os = "linux") {
            assert!(!display_in(None, None));
            assert!(!display_in(empty, empty));
        } else {
            assert!(display_in(None, None));
        }
    }

    fn refused(state: &Path, tool: &str, account: &str, url: &str, display: bool) -> &'static str {
        match prepare(state, tool, account, url, display) {
            Ok(_) => panic!("{tool} {account} {url}: prepared"),
            Err(why) => why,
        }
    }

    /// SEC-34: no display, another tool, a crafted account or a URL that is no claude login
    /// start: no window, and nothing is created.
    #[test]
    fn no_display_exits_1() {
        let d = tempfile::tempdir().unwrap();
        let state = d.path();
        assert_eq!(refused(state, "claude", "work", START, false), "no display");
        assert_eq!(
            refused(state, "codex", "work", START, true),
            "tool or account not valid"
        );
        for account in ["../x", "", "a/b", "Work", "."] {
            assert_eq!(
                refused(state, "claude", account, START, true),
                "tool or account not valid",
                "{account:?}"
            );
        }
        for url in [
            "https://evil.com/",
            "https://evil.com/oauth/authorize",
            "https://claude.ai/",
            "file:///etc/passwd",
            "",
        ] {
            assert_eq!(
                refused(state, "claude", "work", url, true),
                "not a claude login URL",
                "{url}"
            );
        }
        assert!(
            !state.join(DIR).exists(),
            "nothing made for a refused start"
        );
        // The process claude starts and the window process, without a display.
        if !ENABLED {
            assert_eq!(
                refused(state, "claude", "work", START, true),
                "built without the login window"
            );
        }
    }

    /// SEC-37: one private folder per account, checked before each use.
    #[test]
    fn data_dir_per_account_0700() {
        let d = tempfile::tempdir().unwrap();
        let state = d.path().join("state");
        let work = data_dir(&state, "claude", Some("work")).unwrap();
        let default = data_dir(&state, "claude", None).unwrap();
        assert_eq!(work, state.join("ai-login/claude-work"));
        assert_eq!(default, state.join("ai-login/claude-default"));
        assert_eq!(
            data_dir(&state, "claude", Some("alt")).unwrap(),
            state.join("ai-login/claude-alt")
        );
        for p in [&state.join("ai-login"), &work, &default] {
            assert!(p.is_dir());
            assert_eq!(mode(p), 0o700, "{}", p.display());
        }
        // Group / other bits are dropped again on the next use.
        std::fs::set_permissions(&work, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::set_permissions(
            state.join("ai-login"),
            std::fs::Permissions::from_mode(0o777),
        )
        .unwrap();
        data_dir(&state, "claude", Some("work")).unwrap();
        assert_eq!(mode(&work), 0o700);
        assert_eq!(mode(&state.join("ai-login")), 0o700);
        // Crafted names never become a path.
        for bad in ["../x", "a/b", "", ".", "Work", "default"] {
            assert!(data_dir(&state, "claude", Some(bad)).is_err(), "{bad:?}");
        }
        assert!(data_dir(&state, "codex", Some("work")).is_err());
        assert!(data_dir(&state, "../claude", None).is_err());
        // A link in place of the folder is refused and its target left alone.
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("keep"), "x").unwrap();
        std::os::unix::fs::symlink(outside.path(), state.join("ai-login/claude-lnk")).unwrap();
        assert!(data_dir(&state, "claude", Some("lnk")).is_err());
        assert!(outside.path().join("keep").exists());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 1);
        // The same for `ai-login/` itself.
        let other = d.path().join("other");
        std::fs::create_dir(&other).unwrap();
        std::os::unix::fs::symlink(outside.path(), other.join("ai-login")).unwrap();
        assert!(data_dir(&other, "claude", Some("work")).is_err());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 1);
    }

    /// SEC-34: one window per account; another account is not held up.
    #[test]
    fn one_window_per_account() {
        let d = tempfile::tempdir().unwrap();
        let state = d.path();
        let first = lock(state, "claude", "work").unwrap();
        assert_eq!(lock(state, "claude", "work").unwrap_err(), BUSY);
        // A second /login while the first window is open: the user is told what to do.
        assert!(BUSY.contains("close it") && BUSY.contains("/login"));
        let other = lock(state, "claude", "default").unwrap();
        assert_eq!(mode(&state.join("ai-login/claude-work.lock")), 0o600);
        drop(first);
        let again = lock(state, "claude", "work").unwrap();
        drop((again, other));
        assert!(lock(state, "claude", "../x").is_err());
        assert!(lock(state, "codex", "work").is_err());
        if ENABLED {
            // The whole start: the second window of an account is refused before any window.
            let held = lock(state, "claude", "work").unwrap();
            assert_eq!(refused(state, "claude", "work", START, true), BUSY);
            drop(held);
            let p = prepare(state, "claude", "work", START, true).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(p.start.port, Some(PORT));
            assert_eq!(p.data, state.join("ai-login/claude-work"));
            assert_eq!(mode(&p.data), 0o700);
            assert_eq!(refused(state, "claude", "work", START, true), BUSY);
        }
    }

    /// SEC-33 as amended by D63: `BROWSER` is one path, the account's launcher link to vr-ide.
    #[test]
    fn browser_env_is_the_launcher_link() {
        if !ENABLED {
            let d = tempfile::tempdir().unwrap();
            let exe = fake_exe(&d.path().join("bin"), "vr-ide", "");
            assert!(browser_env(d.path(), &exe, "claude", None).is_err());
            assert!(!d.path().join(DIR).exists());
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let state = d.path().join("state");
        let exe = fake_exe(&d.path().join("bin"), "vr-ide", "");
        let work = browser_env(&state, &exe, "claude", Some("work")).unwrap();
        let link = state.join("ai-login/bin/vr-ide-login.claude.work");
        assert_eq!(work, link.clone().into_os_string());
        assert_eq!(
            launcher_path(&state, "claude", Some("work")),
            Some(link.clone())
        );
        assert_eq!(std::fs::read_link(&link).unwrap(), exe);
        assert_eq!(mode(&state.join("ai-login")), 0o700);
        assert_eq!(mode(&state.join("ai-login/bin")), 0o700);
        let default = browser_env(&state, &exe, "claude", None).unwrap();
        assert_eq!(
            PathBuf::from(default),
            state.join("ai-login/bin/vr-ide-login.claude.default")
        );
        // Kept when right, replaced when it names another program (an updated vr-ide).
        browser_env(&state, &exe, "claude", Some("work")).unwrap();
        let newer = fake_exe(&d.path().join("bin2"), "vr-ide", "");
        browser_env(&state, &newer, "claude", Some("work")).unwrap();
        assert_eq!(std::fs::read_link(&link).unwrap(), newer);
        // Something that is not a link is not ours to replace.
        let taken = state.join("ai-login/bin/vr-ide-login.claude.alt");
        std::fs::write(&taken, "x").unwrap();
        assert_eq!(
            browser_env(&state, &exe, "claude", Some("alt")),
            Err("launcher unusable")
        );
        assert_eq!(std::fs::read(&taken).unwrap(), b"x");
        // Only claude, only valid accounts.
        assert!(browser_env(&state, &exe, "codex", Some("work")).is_err());
        assert!(browser_env(&state, &exe, "gemini", None).is_err());
        for bad in ["../x", "a b", "", "default", "Work"] {
            assert!(
                browser_env(&state, &exe, "claude", Some(bad)).is_err(),
                "{bad:?}"
            );
        }
        // A program path consumers of `BROWSER` could split or expand is never used.
        for odd in [
            "a b", "a:b", "a%sb", "a$b", "a'b", "a\"b", "a`b", "a\\b", "a\nb", "é",
        ] {
            let exe = fake_exe(&d.path().join(odd), "vr-ide", "");
            assert_eq!(
                browser_env(&state, &exe, "claude", Some("work")),
                Err("vr-ide path unusable"),
                "{odd:?}"
            );
            let state = d.path().join(odd).join("state");
            let plain = d.path().join("bin/vr-ide");
            assert_eq!(
                browser_env(&state, &plain, "claude", Some("work")),
                Err("state folder path unusable"),
                "{odd:?}"
            );
            assert!(!state.exists());
        }
        assert_eq!(
            std::fs::read_link(&link).unwrap(),
            newer,
            "kept after refusals"
        );
        for gone in [
            d.path().join("bin/missing"),
            d.path().join("bin"),
            PathBuf::from("bin/vr-ide"),
        ] {
            assert_eq!(
                browser_env(&state, &gone, "claude", Some("work")),
                Err("vr-ide path unusable")
            );
        }
        // `ai-login/bin` swapped for a link: refused, nothing made behind it.
        let outside = tempfile::tempdir().unwrap();
        let other = d.path().join("other");
        std::fs::create_dir_all(other.join("ai-login")).unwrap();
        std::os::unix::fs::symlink(outside.path(), other.join("ai-login/bin")).unwrap();
        assert!(browser_env(&other, &exe, "claude", Some("work")).is_err());
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
    }

    /// SEC-38: what an inherited `BROWSER` of ours looks like.
    #[test]
    fn helper_browser_values_are_recognised() {
        for ours in [
            "/s/ai-login/bin/vr-ide-login.claude.work",
            "vr-ide-login.claude.default",
            "/usr/bin/vr-ide --login-window claude work",
            "firefox:/s/ai-login/bin/vr-ide-login.claude.work",
            "--login-window",
        ] {
            assert!(is_helper_browser(OsStr::new(ours)), "{ours}");
        }
        for other in [
            "",
            "firefox",
            "/usr/bin/xdg-open",
            "/opt/vr-ide/bin/vr-ide",
            "true",
        ] {
            assert!(!is_helper_browser(OsStr::new(other)), "{other}");
        }
    }

    /// D6: the system opener gets the argument unchanged and no `BROWSER`, so it cannot come
    /// back to the launcher.
    #[test]
    fn opener_gets_the_argument_without_browser() {
        let c = opener_command("xdg-open", OsStr::new("https://example.com/a?b=c d"));
        assert_eq!(c.get_program(), "xdg-open");
        let args: Vec<&OsStr> = c.get_args().collect();
        assert_eq!(args, [OsStr::new("https://example.com/a?b=c d")]);
        let envs: Vec<_> = c.get_envs().collect();
        assert_eq!(envs, [(OsStr::new("BROWSER"), None)]);
    }

    /// D64: the launcher starts the window as its own process with the explicit arguments, and
    /// returns on its "window is up" byte, on its early end, or after the wait.
    #[test]
    fn launcher_waits_for_the_window_byte() {
        let d = tempfile::tempdir().unwrap();
        let bin = d.path().join("bin");
        let state = d.path().join("state");
        let seen = d.path().join("seen");
        let up = fake_exe(
            &bin,
            "up",
            &format!(
                "#!/bin/sh\nprintf '%s|' \"$@\" > '{0}'\nprintf '\\nSTATE=%s BROWSER=%s' \"$VR_IDE_STATE_DIR\" \"${{BROWSER-unset}}\" >> '{0}'\nprintf 1\nexec sleep 1\n",
                seen.display()
            ),
        );
        let t0 = Instant::now();
        assert_eq!(
            spawn_window(
                &up,
                &state,
                "claude",
                "work",
                START,
                Duration::from_secs(20)
            ),
            0
        );
        assert!(
            t0.elapsed() < Duration::from_secs(10),
            "returned on the byte"
        );
        let said = std::fs::read_to_string(&seen).unwrap();
        assert_eq!(
            said,
            format!(
                "--login-window|claude|work|{START}|\nSTATE={} BROWSER=unset",
                state.display()
            )
        );
        let failed = fake_exe(&bin, "failed", "#!/bin/sh\nexit 1\n");
        assert_eq!(
            spawn_window(
                &failed,
                &state,
                "claude",
                "work",
                START,
                Duration::from_secs(20)
            ),
            1
        );
        let silent = fake_exe(&bin, "silent", "#!/bin/sh\nexec sleep 1\n");
        let t0 = Instant::now();
        assert_eq!(
            spawn_window(
                &silent,
                &state,
                "claude",
                "work",
                START,
                Duration::from_millis(100)
            ),
            0
        );
        assert!(
            t0.elapsed() < Duration::from_millis(900),
            "returned after the wait"
        );
        assert_eq!(
            spawn_window(
                &bin.join("missing"),
                &state,
                "claude",
                "work",
                START,
                Duration::from_secs(1)
            ),
            1
        );
    }

    /// A claude login URL without a display, or in a build without the window, makes the
    /// launcher exit 1 (claude then shows the URL); it never reaches the opener.
    #[test]
    fn launcher_refuses_a_login_without_window() {
        let d = tempfile::tempdir().unwrap();
        if !ENABLED {
            assert_eq!(
                run_launcher(d.path(), "claude", "work", OsStr::new(START)),
                1
            );
        }
        assert_eq!(
            launch("claude", "work", OsStr::new(START), false, None),
            Launch::Fail
        );
        assert_eq!(
            launch("codex", "work", OsStr::new(START), true, None),
            Launch::Fail
        );
        assert_eq!(
            launch("claude", "../x", OsStr::new(START), true, None),
            Launch::Fail
        );
        // xdg-open reads an argument starting with `-` as an option and takes no `--`.
        for option in ["--help", "-oProxyCommand=x", "-https://x/--version", "-"] {
            assert_eq!(
                launch("claude", "work", OsStr::new(option), true, None),
                Launch::Fail,
                "{option}"
            );
        }
        for other in [
            "https://example.com/",
            "http://localhost:3000/",
            "/tmp/report.html",
            "https://claude.ai/upgrade",
        ] {
            assert_eq!(
                launch("claude", "work", OsStr::new(other), true, None),
                Launch::Opener,
                "{other}"
            );
        }
        if ENABLED {
            let exe = fake_exe(&d.path().join("bin"), "vr-ide", "");
            assert_eq!(
                launch("claude", "work", OsStr::new(START), true, Some(&exe)),
                Launch::Window(exe)
            );
            let odd = fake_exe(&d.path().join("bin"), "vr-ide (deleted)", "");
            assert_eq!(
                launch("claude", "work", OsStr::new(START), true, Some(&odd)),
                Launch::Fail
            );
        }
        assert!(!d.path().join(DIR).exists());
    }
}
