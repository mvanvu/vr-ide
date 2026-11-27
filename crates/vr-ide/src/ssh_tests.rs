//! FR-25: SSH sessions from the command window (AC-S1..S6) and the threat model's requirements
//! that need the app (SEC-S2, S4, S7..S10, S12..S18), driven through the `App` API. `ssh` is a
//! shell script here: nothing in this file connects anywhere.
//!
//! The shim logs each call (`log`: `G …` for a configuration probe, `S …` for a session),
//! dumps a session's environment (`env.<pid>`), prints `pre` before and `post` after the
//! "login", runs the `LocalCommand` it was given through a shell as ssh does, and then is a
//! remote shell with the prompt `remote$ `. `mode` picks failures.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, Focus};
use crate::app_tests::test_app;
use crate::clipboard::{Clipboard, MemoryClipboard};
use crate::commands::CmdArgs;
use crate::commands::terminal as term;
use crate::slash_tests::{error, input, key, notices, slash, titles};
use crate::ssh::{MAX_SESSIONS, NOT_VALID, Target};
use crate::terminal_tests::{click, ctrl_backtick, draw, frame_text};
use vr_term::TermStatus;

const WAIT: Duration = Duration::from_secs(10);

const SHIM: &str = r#"#!/bin/sh
dir=$(dirname "$0")
mode=$(cat "$dir/mode" 2>/dev/null)
if [ "$1" = "-G" ]; then
  printf 'G %s\n' "$*" >> "$dir/log"
  case "$mode" in
    *usercmd*) echo "localcommand touch $dir/user-ran" ;;
    *ghang*) sleep 300 ;;
    *gfail*) exit 1 ;;
  esac
  echo "permitlocalcommand no"
  exit 0
fi
printf 'S %s\n' "$*" >> "$dir/log"
env > "$dir/env.$$"
lc=
for a in "$@"; do case "$a" in LocalCommand=*) lc=${a#LocalCommand=} ;; esac; done
[ -f "$dir/pre" ] && cat "$dir/pre"
case "$mode" in *authfail*) echo "Permission denied (publickey,password)."; exit 255 ;; esac
case "$mode" in *password*) printf 'password: '; stty -echo; read -r pw; stty echo; echo ;; esac
[ -n "$lc" ] && /bin/sh -c "$lc"
[ -f "$dir/post" ] && cat "$dir/post"
echo "Welcome to remote"
ENV=/dev/null PS1='remote$ ' exec /bin/sh -s "$dir"
"#;

struct Fx {
    dir: tempfile::TempDir,
    app: App,
    clip: MemoryClipboard,
}

impl Fx {
    fn path(&self, name: &str) -> PathBuf {
        self.dir.path().join(name)
    }

    fn set(&self, name: &str, bytes: &[u8]) {
        std::fs::write(self.path(name), bytes).unwrap();
    }

    fn log(&self) -> String {
        std::fs::read_to_string(self.path("log")).unwrap_or_default()
    }

    /// The argument lines of the sessions started so far.
    fn sessions(&self) -> Vec<String> {
        self.log()
            .lines()
            .filter_map(|l| l.strip_prefix("S ").map(str::to_string))
            .collect()
    }

    fn probes(&self) -> Vec<String> {
        self.log()
            .lines()
            .filter_map(|l| l.strip_prefix("G ").map(str::to_string))
            .collect()
    }

    fn recent_file(&self) -> String {
        std::fs::read_to_string(self.path("state/ssh-recent")).unwrap_or_default()
    }
}

/// An app whose `ssh` is the shim, with a home (and its ssh config `config`) and a state dir
/// in a temp folder, drawn once at 80x24.
fn fx_with(mode: &str, config: &str) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let shim = dir.path().join("ssh");
    std::fs::write(&shim, SHIM).unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(dir.path().join("mode"), mode).unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(home.join(".ssh")).unwrap();
    std::fs::write(home.join(".ssh/config"), config).unwrap();
    let project = dir.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let (mut app, clip) = test_app(&project);
    app.keyboard_enhanced = true;
    app.ssh.bin = Some(shim);
    app.ssh.home = Some(home);
    app.ssh.recent_file = Some(dir.path().join("state/ssh-recent"));
    draw(&mut app, 80, 24);
    Fx { dir, app, clip }
}

fn fx(mode: &str) -> Fx {
    fx_with(mode, "")
}

/// One pass of what the event loop does between events, then a frame.
fn pump(app: &mut App) -> String {
    if app.terminal.take_wake() {
        term::refresh(app);
    }
    frame_text(&draw(app, 80, 24))
}

fn wait(app: &mut App, what: &str, ok: impl Fn(&App, &str) -> bool) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        // The wake flag may have been taken by an earlier pass: look regardless.
        term::refresh(app);
        let screen = pump(app);
        if ok(app, &screen) {
            return screen;
        }
        assert!(
            Instant::now() < deadline,
            "waiting for {what}; notices: {}\nscreen:\n{screen}",
            notices(app)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// `/ssh <target>` and Enter in the command window.
fn connect(app: &mut App, target: &str) {
    slash(app, &format!("/ssh {target}"));
    key(app, KeyCode::Enter);
}

/// Connects and waits until the session is logged in and full screen.
fn login(f: &mut Fx, target: &str) -> String {
    connect(&mut f.app, target);
    assert!(f.app.palette.is_none(), "{:?}", error(&f.app));
    wait(&mut f.app, "full screen", |a, s| {
        term::full_screen(a) && s.contains("remote$")
    })
}

fn login_wait(f: &mut Fx) -> String {
    wait(&mut f.app, "full screen", |a, s| {
        term::full_screen(a) && s.contains("remote$")
    })
}

fn typ(app: &mut App, s: &str) {
    for c in s.chars() {
        let code = if c == '\n' {
            KeyCode::Enter
        } else {
            KeyCode::Char(c)
        };
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    }
}

fn session(app: &App, i: usize) -> &crate::commands::ssh::Session {
    app.terminal.tabs[i].ssh.as_ref().expect("an ssh tab")
}

/// The 32 hex characters of the nonce in a session's argument line.
fn nonce_in(args: &str) -> String {
    let at = args.find("vr-ide-ssh;").expect("a LocalCommand") + "vr-ide-ssh;".len();
    args[at..at + 32].to_string()
}

fn marker(nonce: &str) -> Vec<u8> {
    format!("\x1b]7717;vr-ide-ssh;{nonce}\x07").into_bytes()
}

/// Processes whose command line holds `needle` (/proc scan).
fn procs_with(needle: &str) -> Vec<u32> {
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_string_lossy().parse::<u32>().ok())
        .filter(|pid| {
            std::fs::read(format!("/proc/{pid}/cmdline"))
                .is_ok_and(|c| String::from_utf8_lossy(&c).contains(needle))
        })
        .collect()
}

fn wait_no_procs(needle: &str) {
    let deadline = Instant::now() + WAIT;
    loop {
        let left = procs_with(needle);
        if left.is_empty() {
            return;
        }
        assert!(Instant::now() < deadline, "still running: {left:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn tag(f: &Fx) -> String {
    f.dir
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

// ---- AC-S1: the list --------------------------------------------------------------------------

#[test]
fn ac_s1_the_list_shows_open_sessions_recent_targets_and_config_hosts() {
    let mut f = fx_with(
        "",
        "Host web1 db-*.example !bad \"quo ted\"\n  HostName 10.0.0.1\nHost git.example.com\nHost *\n  User me\n",
    );
    f.app.ssh.recent = vec![Target::parse("bob@old:2222").unwrap()];
    slash(&mut f.app, "/ssh ");
    assert_eq!(
        titles(&f.app),
        [
            "bob@old:2222  recent",
            "web1  ssh config",
            "git.example.com  ssh config"
        ]
    );
    // Typing filters; a typed target of its own is the first row.
    typ(&mut f.app, "web");
    assert_eq!(titles(&f.app), ["web  connect", "web1  ssh config"]);
    typ(&mut f.app, "1");
    assert_eq!(titles(&f.app), ["web1  ssh config"]);
    key(&mut f.app, KeyCode::Esc);
    // The palette lists the command under `ssh`, and `ssh` + space is the list itself.
    key(&mut f.app, KeyCode::F(1));
    typ(&mut f.app, "ssh");
    assert!(
        titles(&f.app).iter().any(|t| t == "SSH: Connect to Host"),
        "{:?}",
        titles(&f.app)
    );
    typ(&mut f.app, " ");
    assert_eq!(input(&f.app), "/ssh ");
    assert_eq!(titles(&f.app).len(), 3);
    key(&mut f.app, KeyCode::Esc);
    // The palette command leads to the same list.
    f.app.run_command("ssh.connect", CmdArgs::None);
    assert_eq!(input(&f.app), "/ssh ");
    // Nothing was run to build any of these lists (SEC-S4).
    assert_eq!(f.log(), "");
}

#[test]
fn sec_s4_fifty_listed_hosts_run_no_probe() {
    let config: String = (0..50).map(|i| format!("Host h{i}\n")).collect();
    let mut f = fx_with("", &config);
    let t0 = Instant::now();
    slash(&mut f.app, "/ssh ");
    assert!(
        t0.elapsed() < Duration::from_millis(200),
        "{:?}",
        t0.elapsed()
    );
    assert_eq!(titles(&f.app).len(), 50);
    typ(&mut f.app, "h4");
    assert_eq!(titles(&f.app).len(), 11);
    assert_eq!(f.log(), "", "the list runs nothing");
}

#[test]
fn ac_s1_sec_s9_a_refused_target_runs_nothing_and_is_not_echoed() {
    let mut f = fx("");
    let secret = "hunter2 -oProxyCommand=evil";
    for bad in [
        secret,
        "-oProxyCommand=x",
        "a\tb",
        "%h",
        "u@h@h",
        "ssh://h:2",
    ] {
        slash(&mut f.app, &format!("/ssh {bad}"));
        key(&mut f.app, KeyCode::Enter);
        assert_eq!(error(&f.app).as_deref(), Some(NOT_VALID), "{bad:?}");
        key(&mut f.app, KeyCode::Esc);
    }
    pump(&mut f.app);
    assert!(f.app.terminal.tabs.is_empty());
    assert!(!f.app.ssh.starting());
    assert_eq!(f.log(), "", "the shim never ran");
    assert!(!notices(&f.app).contains("hunter2"));
    assert!(f.app.ssh.recent.is_empty());
    assert_eq!(f.recent_file(), "");
    // The window keeps no history of what was typed.
    key(&mut f.app, KeyCode::F(1));
    assert_eq!(input(&f.app), "");
}

// ---- AC-S2, AC-S3: start, prompts, full screen --------------------------------------------------

#[test]
fn ac_s2_ac_s3_a_session_starts_as_a_tab_and_goes_full_screen_after_login() {
    let mut f = fx("password");
    connect(&mut f.app, "bob@web1:2222");
    let screen = wait(&mut f.app, "the password prompt", |_, s| {
        s.contains("password:")
    });
    // A normal panel tab with the keys: the editor layout is still there.
    assert!(!term::full_screen(&f.app));
    assert!(f.app.terminal.visible && f.app.focus == Focus::Terminal);
    assert!(screen.contains(" 1 ssh bob@web1:2222 x "), "{screen}");
    assert!(f.app.layout.editor.height > 0);
    assert!(!session(&f.app, 0).logged_in);
    assert!(f.app.ssh.recent.is_empty(), "not recorded before the login");
    // The probe ran without options of ours; the session with exactly the argv of SEC-S2.
    assert_eq!(f.probes(), ["-G -l bob -p 2222 -- web1"]);
    let args = f.sessions().pop().unwrap();
    let nonce = nonce_in(&args);
    assert_eq!(
        args,
        format!(
            "-l bob -p 2222 -o ControlMaster=no -o PermitLocalCommand=yes -o LocalCommand=printf \
             '\\033]7717;vr-ide-ssh;{nonce}\\007' -- web1"
        )
    );
    // The password goes to the tab; then the marker comes and the tab takes the window.
    typ(&mut f.app, "s3cret-pw\n");
    let screen = wait(&mut f.app, "full screen", |a, s| {
        term::full_screen(a) && s.contains("remote$")
    });
    let rows: Vec<&str> = screen.lines().collect();
    assert_eq!(rows.len(), 24);
    assert!(rows[0].starts_with("password:"), "{screen}");
    assert!(screen.contains("Welcome to remote"), "{screen}");
    assert!(rows[23].starts_with(" SSH  bob@web1:2222"), "{}", rows[23]);
    assert!(
        rows[23].trim_end().ends_with("ctrl+` editor"),
        "{}",
        rows[23]
    );
    assert!(!screen.contains(" 1 ssh "), "no tab bar in full screen");
    assert_eq!(f.app.terminal.tabs[0].size(), (80, 23));
    assert_eq!(f.app.layout.editor.height, 0);
    assert!(session(&f.app, 0).logged_in);
    // SEC-S6, SEC-S7, SEC-S9: neither the marker nor the nonce nor the password is anywhere
    // vr-ide shows or keeps.
    assert_eq!(f.recent_file(), "bob@web1:2222\n");
    let state: String = [f.recent_file(), notices(&f.app), screen.clone()].concat();
    for hidden in [nonce.as_str(), "vr-ide-ssh", "7717", "s3cret-pw"] {
        assert!(!state.contains(hidden), "{hidden} shows in:\n{state}");
    }
    let debug = format!("{:?}", session(&f.app, 0));
    assert!(!debug.contains(&nonce), "{debug}");
}

#[test]
fn ac_s3_no_full_screen_when_the_user_has_a_local_command() {
    let mut f = fx("usercmd");
    connect(&mut f.app, "web1");
    let screen = wait(&mut f.app, "the session", |_, s| s.contains("remote$"));
    // SEC-S4: neither option is passed, so the user's own PermitLocalCommand stays in force
    // and their command was not run by us.
    assert_eq!(f.sessions(), ["-o ControlMaster=no -- web1"]);
    assert!(!f.path("user-ran").exists());
    assert!(!term::full_screen(&f.app) && !session(&f.app, 0).logged_in);
    assert!(screen.contains(" 1 ssh web1 x "), "{screen}");
    assert!(
        notices(&f.app).contains("SSH web1: your ssh config sets LocalCommand"),
        "{}",
        notices(&f.app)
    );
}

#[test]
fn d320_ssh_fullscreen_off_starts_plain_without_a_probe() {
    let mut f = fx("");
    f.app.settings.ssh_fullscreen = false;
    connect(&mut f.app, "web1");
    wait(&mut f.app, "the session", |_, s| s.contains("remote$"));
    assert_eq!(f.log(), "S -o ControlMaster=no -- web1\n");
    assert!(!term::full_screen(&f.app));
    assert_eq!(notices(&f.app), "");
    // A clean exit still makes the target a recent one.
    typ(&mut f.app, "exit\n");
    wait(&mut f.app, "the exit", |a, _| {
        !a.terminal.tabs[0].running() && a.ssh.recent.len() == 1
    });
    assert_eq!(f.recent_file(), "web1\n");
}

#[test]
fn sec_s4_a_hanging_probe_is_killed_and_the_tab_opens_plain() {
    let mut f = fx("ghang");
    f.app.ssh.probe_timeout = Duration::from_millis(300);
    connect(&mut f.app, "web1");
    assert!(f.app.ssh.starting() && f.app.terminal.tabs.is_empty());
    // SEC-S16: one probe at a time.
    connect(&mut f.app, "web2");
    assert!(error(&f.app).unwrap().contains("still starting"));
    key(&mut f.app, KeyCode::Esc);
    wait(&mut f.app, "the session", |_, s| s.contains("remote$"));
    assert_eq!(f.sessions(), ["-o ControlMaster=no -- web1"]);
    assert!(notices(&f.app).contains("ssh -G failed or took too long"));
    assert!(!term::full_screen(&f.app));
    // The probe is gone (its helpers go with its process group: `ssh::tests`).
    wait_no_procs(&format!("{}/ssh -G", tag(&f)));
}

/// A probe whose thread never answers blocks later sessions only until it is overdue.
#[test]
fn sec_s16_a_stuck_probe_is_given_up_after_its_grace_period() {
    let mut f = fx("");
    let _tx = f
        .app
        .ssh
        .stall_probe(Target::parse("stuck").unwrap(), Duration::from_secs(1));
    connect(&mut f.app, "web1");
    assert!(error(&f.app).unwrap().contains("still starting"));
    key(&mut f.app, KeyCode::Esc);
    let _tx = f
        .app
        .ssh
        .stall_probe(Target::parse("stuck").unwrap(), Duration::from_secs(60));
    connect(&mut f.app, "web1");
    assert!(f.app.palette.is_none(), "{:?}", error(&f.app));
    assert!(notices(&f.app).contains("SSH stuck: reading the ssh configuration did not finish"));
    login_wait(&mut f);
    assert_eq!(f.sessions().len(), 1);
    assert_eq!(session(&f.app, 0).target.as_str(), "web1");
}

#[test]
fn sec_s4_a_failing_probe_opens_a_plain_tab() {
    let mut f = fx("gfail");
    connect(&mut f.app, "web1");
    wait(&mut f.app, "the session", |_, s| s.contains("remote$"));
    assert_eq!(f.sessions(), ["-o ControlMaster=no -- web1"]);
    assert!(!term::full_screen(&f.app));
}

// ---- AC-S4: keys, leaving and coming back -------------------------------------------------------

#[test]
fn ac_s4_ctrl_backtick_leaves_and_choosing_the_session_returns() {
    let mut f = fx("");
    login(&mut f, "web1");
    // Keys go to the session.
    typ(&mut f.app, "echo k-$((40+2))\n");
    wait(&mut f.app, "the echo", |_, s| s.contains("k-42"));
    // ctrl+` gives the editor layout back; the session lives on.
    ctrl_backtick(&mut f.app);
    let screen = pump(&mut f.app);
    assert!(!term::full_screen(&f.app) && !f.app.terminal.visible);
    assert_eq!(f.app.focus, Focus::Editor);
    assert!(f.app.layout.editor.height > 0 && !screen.contains(" SSH "));
    assert!(f.app.terminal.tabs[0].running());
    // ctrl+` again shows the panel, the session a normal tab there.
    ctrl_backtick(&mut f.app);
    let screen = pump(&mut f.app);
    assert!(f.app.terminal.visible && !term::full_screen(&f.app));
    assert!(screen.contains(" 1 ssh web1 x "), "{screen}");
    // A click on its tab: full screen again.
    let area = f.app.layout.terminal;
    click(&mut f.app, area.x + 4, area.y);
    let screen = pump(&mut f.app);
    assert!(term::full_screen(&f.app), "{screen}");
    assert!(screen.lines().last().unwrap().starts_with(" SSH  web1"));
    // Leave again; the ssh list names the open session first and choosing it returns.
    ctrl_backtick(&mut f.app);
    slash(&mut f.app, "/ssh ");
    assert_eq!(titles(&f.app)[0], "web1  open session");
    key(&mut f.app, KeyCode::Enter);
    pump(&mut f.app);
    assert!(term::full_screen(&f.app));
    assert_eq!(f.sessions().len(), 1, "no second ssh was started");
    assert_eq!(f.app.terminal.tabs.len(), 1);
}

#[test]
fn ac_s4_resize_follows_the_window_and_f1_opens_over_the_session() {
    let mut f = fx("");
    login(&mut f, "web1");
    let t = draw(&mut f.app, 100, 30);
    assert_eq!(f.app.terminal.tabs[0].size(), (100, 29));
    let text = frame_text(&t);
    assert!(text.lines().nth(29).unwrap().starts_with(" SSH  web1"));
    // F1 is kept by the terminal: the command window opens over the session, which stays
    // full screen behind it.
    key(&mut f.app, KeyCode::F(1));
    assert_eq!(f.app.focus, Focus::Palette);
    let text = frame_text(&draw(&mut f.app, 100, 30));
    assert!(term::full_screen(&f.app));
    assert!(text.lines().nth(29).unwrap().starts_with(" SSH  web1"));
    key(&mut f.app, KeyCode::Esc);
    assert!(term::full_screen(&f.app) && f.app.focus == Focus::Terminal);
    // A window of one row cannot show the session and its status row: the IDE layout stays.
    draw(&mut f.app, 80, 1);
    draw(&mut f.app, 80, 24);
    assert!(term::full_screen(&f.app));
}

/// SEC-S14: whatever modes the remote side sets, the reserved keys are matched before any
/// byte is encoded for it.
#[test]
fn sec_s14_reserved_keys_never_reach_a_session_that_set_keyboard_modes() {
    let mut f = fx("");
    // kitty keyboard flags, application cursor and keypad, mouse reporting, bracketed paste.
    f.set(
        "post",
        b"\x1b[>31u\x1b[?1h\x1b=\x1b[?1000h\x1b[?1006h\x1b[?2004h",
    );
    login(&mut f, "web1");
    wait(&mut f.app, "the modes", |a, _| {
        let m = a.terminal.tabs[0].grid.modes;
        m.app_cursor && m.mouse_reporting && m.bracketed_paste
    });
    // The remote reads raw bytes into a file until `q`.
    typ(
        &mut f.app,
        "stty raw -echo; echo rea-dy; dd bs=1 count=4 of=keys 2>/dev/null; stty sane\n",
    );
    wait(&mut f.app, "the reader", |_, s| s.contains("rea-dy"));
    for (code, mods) in [
        (KeyCode::F(1), KeyModifiers::NONE),
        (KeyCode::Esc, KeyModifiers::NONE),
        (KeyCode::F(8), KeyModifiers::CONTROL),
        (KeyCode::F(9), KeyModifiers::CONTROL),
    ] {
        f.app.handle_key(KeyEvent::new(code, mods));
    }
    // (Esc above closed the command window F1 opened; it went to the palette.)
    assert!(term::full_screen(&f.app));
    // A paste is bracketed and loses its ESC.
    f.app.handle_paste("a\x1bb");
    ctrl_backtick(&mut f.app);
    assert!(!term::full_screen(&f.app) && f.app.focus == Focus::Editor);
    let keys = f.path("home/keys");
    let deadline = Instant::now() + WAIT;
    while std::fs::metadata(&keys).map_or(0, |m| m.len()) < 4 {
        assert!(Instant::now() < deadline, "the paste did not arrive");
        std::thread::sleep(Duration::from_millis(20));
    }
    // Only the paste arrived: its start is `ESC [ 2 0 0 ~`, no key byte before it.
    assert_eq!(std::fs::read(&keys).unwrap(), b"\x1b[20");
    // ctrl+f7 is kept too (it moves the panel; no byte for the session).
    ctrl_backtick(&mut f.app);
    let before = f.app.terminal.position;
    f.app
        .handle_key(KeyEvent::new(KeyCode::F(7), KeyModifiers::CONTROL));
    assert_ne!(f.app.terminal.position, before);
}

// ---- AC-S5, SEC-S18: exit and restart -----------------------------------------------------------

#[test]
fn ac_s5_exit_ends_full_screen_and_enter_reconnects_with_a_new_nonce() {
    let mut f = fx("");
    login(&mut f, "bob@web1");
    let first = nonce_in(&f.sessions()[0]);
    typ(&mut f.app, "exit 3\n");
    let screen = wait(&mut f.app, "the exit", |a, _| {
        a.terminal.tabs[0].status() == TermStatus::Exited(3)
    });
    let screen = {
        let _ = screen;
        pump(&mut f.app)
    };
    assert!(!term::full_screen(&f.app) && !f.app.terminal.full);
    assert!(f.app.layout.editor.height > 0);
    assert!(
        screen.contains("[process exited 3] — Enter to restart"),
        "{screen}"
    );
    assert!(screen.contains(" 1 ssh bob@web1 [3] x "), "{screen}");
    // The first nonce printed into the new stream is no login.
    f.set("pre", &marker(&first));
    f.set("mode", b"password");
    typ(&mut f.app, "\n");
    let screen = wait(&mut f.app, "the new prompt", |_, s| s.contains("password:"));
    assert_eq!(f.app.terminal.tabs.len(), 1, "the same tab");
    assert!(!term::full_screen(&f.app) && !session(&f.app, 0).logged_in);
    assert!(!screen.contains("vr-ide-ssh"));
    // SEC-S18: the whole start again: a probe, the validated target, a different nonce.
    assert_eq!(f.probes().len(), 2);
    let second = nonce_in(&f.sessions()[1]);
    assert_ne!(first, second);
    assert_eq!(
        f.sessions()[1].replace(&second, "N"),
        f.sessions()[0].replace(&first, "N")
    );
    typ(&mut f.app, "pw\n");
    wait(&mut f.app, "full screen again", |a, _| term::full_screen(a));
}

#[test]
fn sec_s18_sec_s10_a_failed_login_stays_a_plain_tab_and_is_not_recorded() {
    let mut f = fx("authfail");
    connect(&mut f.app, "web1");
    let screen = wait(&mut f.app, "the exit", |a, s| {
        a.terminal
            .tabs
            .first()
            .is_some_and(|t| t.status() == TermStatus::Exited(255))
            && s.contains("Permission denied")
            && s.contains("[process exited 255]")
    });
    assert!(screen.contains("Permission denied"), "{screen}");
    assert!(screen.contains("[process exited 255]"), "{screen}");
    assert!(!term::full_screen(&f.app) && !session(&f.app, 0).logged_in);
    // No timeout fallback either.
    std::thread::sleep(Duration::from_millis(300));
    pump(&mut f.app);
    assert!(!f.app.terminal.full);
    assert!(f.app.ssh.recent.is_empty());
    assert_eq!(f.recent_file(), "");
}

// ---- SEC-S6, S8: the marker ---------------------------------------------------------------------

#[test]
fn sec_s8_a_forged_marker_from_the_remote_side_is_no_login() {
    let mut f = fx("password");
    // Before the login the "server" prints marker-shaped bytes with a nonce it made up.
    let mut pre = marker("00000000000000000000000000000000");
    pre.extend_from_slice(b"\x1b]7717;vr-ide-ssh;\x07banner\r\n");
    f.set("pre", &pre);
    connect(&mut f.app, "web1");
    wait(&mut f.app, "the password prompt", |_, s| {
        s.contains("password:")
    });
    std::thread::sleep(Duration::from_millis(200));
    pump(&mut f.app);
    assert!(!term::full_screen(&f.app) && !session(&f.app, 0).logged_in);
    assert!(f.app.ssh.recent.is_empty());
}

#[test]
fn sec_s8_a_marker_changes_only_the_tab_whose_own_stream_carried_it() {
    let mut f = fx("password");
    connect(&mut f.app, "hosta");
    wait(&mut f.app, "A's prompt", |a, s| {
        a.terminal.tabs.len() == 1 && s.contains("password:")
    });
    // B logs in at once and gets a file with A's marker to print.
    f.set("mode", b"");
    connect(&mut f.app, "hostb");
    wait(&mut f.app, "B full screen", |a, _| {
        a.terminal.tabs.len() == 2 && term::full_screen(a)
    });
    let nonce_a = nonce_in(&f.sessions()[0]);
    assert_ne!(nonce_a, nonce_in(&f.sessions()[1]));
    f.set("a-marker", &marker(&nonce_a));
    let line = format!("cat {}; echo printed\n", f.path("a-marker").display());
    typ(&mut f.app, &line);
    wait(&mut f.app, "B's output", |_, s| s.contains("printed"));
    std::thread::sleep(Duration::from_millis(200));
    pump(&mut f.app);
    assert!(!session(&f.app, 0).logged_in, "A is still at its prompt");
    assert!(session(&f.app, 1).logged_in);
    assert_eq!(f.app.terminal.active, 1);
    // A, when it logs in while B has the window, does not take it: a notice instead.
    f.app.terminal.tabs[0]
        .type_bytes(b"pw\r")
        .expect("A reads its password");
    wait(&mut f.app, "A's login", |a, _| session(a, 0).logged_in);
    assert_eq!(f.app.terminal.active, 1);
    assert!(term::full_screen(&f.app), "B keeps the window");
    assert!(
        notices(&f.app).contains("SSH hosta: logged in; choose its tab for full screen"),
        "{}",
        notices(&f.app)
    );
}

// ---- SEC-S12, S13: what the remote side prints --------------------------------------------------

#[test]
fn sec_s12_labels_come_from_the_target_and_the_status_row_cannot_be_drawn_over() {
    let mut f = fx("");
    // A title that imitates another host, a BIDI override in a title, and text addressed at
    // the last row of any screen.
    f.set(
        "post",
        "\x1b]2;ssh bank-prod\x07\x1b]0;\u{202e}evil\x07\x1b[999;1HFAKE-STATUS-ROW\x1b[1;1H"
            .as_bytes(),
    );
    let screen = login(&mut f, "web1");
    let rows: Vec<&str> = screen.lines().collect();
    assert!(rows[23].starts_with(" SSH  web1"), "{}", rows[23]);
    assert!(!rows[23].contains("FAKE"), "{}", rows[23]);
    assert!(screen.contains("FAKE-STATUS-ROW"), "{screen}");
    // Now that the grid is the window minus the status row, row 999 is the grid's last row.
    typ(&mut f.app, "printf '\\033[999;1HFAKE2\\033[1;1H'\n");
    let screen = wait(&mut f.app, "the addressed text", |_, s| s.contains("FAKE2"));
    let rows: Vec<&str> = screen.lines().collect();
    assert!(rows[22].contains("FAKE2"), "{screen}");
    assert!(rows[23].starts_with(" SSH  web1") && !rows[23].contains("FAKE"));
    assert_eq!(f.app.terminal.tabs[0].title(), "ssh web1");
    ctrl_backtick(&mut f.app);
    ctrl_backtick(&mut f.app);
    let screen = pump(&mut f.app);
    assert!(screen.contains(" 1 ssh web1 x "), "{screen}");
    assert!(!screen.contains("bank-prod") && !screen.contains("evil"));
}

#[test]
fn sec_s12_osc_titles_of_shell_tabs_are_sanitized() {
    let (_d, mut app, _) = crate::terminal_tests::term_app();
    ctrl_backtick(&mut app);
    draw(&mut app, 80, 24);
    crate::terminal_tests::typ(
        &mut app,
        "printf '\\033]2;a\\342\\200\\256b\\302\\205c\\007'; echo done\n",
    );
    let deadline = Instant::now() + WAIT;
    loop {
        let screen = frame_text(&draw(&mut app, 80, 24));
        let title = app.terminal.tabs[0].title();
        if title.starts_with('a') {
            assert_eq!(title, "a<U+202E>b<U+0085>c");
            assert!(screen.contains("a<U+202E>b<U+0085>c"), "{screen}");
            assert!(
                !screen
                    .chars()
                    .any(|c| c != '\n' && crate::ui::safe_text::is_unsafe_char(c)),
                "{screen:?}"
            );
            break;
        }
        assert!(Instant::now() < deadline, "no title; screen:\n{screen}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn sec_s13_a_session_cannot_write_the_clipboard_and_query_floods_stay_bounded() {
    let mut f = fx("");
    f.clip.set("mine").unwrap();
    f.set(
        "post",
        b"\x1b]52;c;ZXZpbA==\x07\x1b]8;;http://evil.example\x07link\x1b]8;;\x07\r\n",
    );
    let screen = login(&mut f, "web1");
    assert!(screen.contains("link"), "{screen}");
    assert_eq!(f.clip.get().as_deref(), Some("mine"));
    // A click on the link text opens nothing: no dialog, no tab, no new process of ours.
    let row = screen.lines().position(|l| l.starts_with("link")).unwrap() as u16;
    click(&mut f.app, 1, row);
    assert!(f.app.dialog.is_none() && f.app.terminal.tabs.len() == 1);
    assert!(term::full_screen(&f.app));
    f.clip.set("mine").unwrap();
    // A flood of cursor-position queries: the answers queue up to the cap and no further.
    typ(
        &mut f.app,
        "stty -echo; i=0; while [ $i -lt 2000 ]; do printf '\\033[6n\\033[6n\\033[6n\\033[6n'; i=$((i+1)); done; echo flood-done\n",
    );
    let t0 = Instant::now();
    wait(&mut f.app, "the flood", |_, s| s.contains("flood-done"));
    assert!(t0.elapsed() < WAIT);
    assert!(f.app.terminal.tabs[0].pending_input() <= vr_term::MAX_PENDING_INPUT);
    assert_eq!(f.clip.get().as_deref(), Some("mine"));
}

// ---- SEC-S15: environment -----------------------------------------------------------------------

#[test]
fn sec_s15_the_session_gets_the_terminal_environment_without_vr_ide_or_nonce() {
    let mut f = fx("");
    login(&mut f, "bob@web1");
    let nonce = nonce_in(&f.sessions()[0]);
    let env_file = std::fs::read_dir(f.dir.path())
        .unwrap()
        .flatten()
        .find(|e| e.file_name().to_string_lossy().starts_with("env."))
        .expect("the shim dumped its environment")
        .path();
    let env = std::fs::read_to_string(env_file).unwrap();
    let get = |k: &str| {
        env.lines()
            .find_map(|l| l.strip_prefix(&format!("{k}=")).map(str::to_string))
    };
    assert_eq!(get("TERM").as_deref(), Some("xterm-256color"));
    assert_eq!(get("SSH_AUTH_SOCK"), std::env::var("SSH_AUTH_SOCK").ok());
    assert_eq!(get("DISPLAY"), std::env::var("DISPLAY").ok());
    for l in env.lines() {
        let name = l.split('=').next().unwrap();
        assert!(
            !name.starts_with("VR_IDE_")
                && !name.starts_with("KITTY_")
                && !name.starts_with("WEZTERM_")
                && name != "TMUX",
            "{l}"
        );
        assert!(!l.contains(&nonce) && !l.contains("vr-ide-ssh"), "{l}");
        assert!(!l.contains("bob@web1"), "{l}");
    }
    // It starts in the home folder, not in the project.
    typ(&mut f.app, "pwd\n");
    let home = f.path("home").canonicalize().unwrap();
    wait(&mut f.app, "pwd", |_, s| {
        s.replace('\n', "").contains(&home.display().to_string())
    });
}

// ---- AC-S6, SEC-S16, S17: several sessions, close and quit ----------------------------------------

#[test]
fn ac_s6_sec_s16_sixteen_sessions_at_most() {
    let mut f = fx("");
    // Plain sessions start without a probe: quick, and the limit is the same.
    f.app.settings.ssh_fullscreen = false;
    for i in 0..MAX_SESSIONS {
        connect(&mut f.app, &format!("host{i}"));
        assert!(f.app.palette.is_none(), "{:?}", error(&f.app));
    }
    assert_eq!(f.app.terminal.tabs.len(), MAX_SESSIONS);
    wait(&mut f.app, "the sessions", |a, _| {
        a.terminal.tabs.iter().all(|t| t.running())
    });
    connect(&mut f.app, "onemore");
    assert_eq!(
        error(&f.app).as_deref(),
        Some("SSH: 16 sessions are open; close one first")
    );
    key(&mut f.app, KeyCode::Esc);
    assert_eq!(f.app.terminal.tabs.len(), MAX_SESSIONS);
    assert_eq!(f.sessions().len(), MAX_SESSIONS);
    // One tab each, labelled by its target.
    assert_eq!(f.app.terminal.tabs[15].title(), "ssh host15");
    // An exited session frees its place.
    typ(&mut f.app, "exit\n");
    wait(&mut f.app, "the exit", |a, _| {
        !a.terminal.tabs[15].running()
    });
    connect(&mut f.app, "onemore");
    assert_eq!(f.app.terminal.tabs.len(), MAX_SESSIONS + 1);
    term::shutdown(&mut f.app);
    wait_no_procs(&tag(&f));
}

#[test]
fn ac_s6_sec_s17_quit_asks_about_a_live_session_and_ends_it() {
    let mut f = fx("");
    login(&mut f, "web1");
    assert!(!procs_with(&tag(&f)).is_empty());
    f.app.run_command("app.quit", CmdArgs::None);
    // Full screen gives way so the question is seen over the IDE layout.
    let screen = pump(&mut f.app);
    assert!(f.app.dialog.is_some() && !f.app.quit);
    assert!(!term::full_screen(&f.app));
    assert!(screen.contains("1 terminal still running"), "{screen}");
    assert!(f.app.layout.editor.height > 0);
    // Cancel: the session has the window again.
    key(&mut f.app, KeyCode::Esc);
    pump(&mut f.app);
    assert!(term::full_screen(&f.app));
    // Quit anyway: no process of the session is left.
    f.app.run_command("app.quit", CmdArgs::None);
    key(&mut f.app, KeyCode::Left);
    key(&mut f.app, KeyCode::Enter);
    assert!(f.app.quit);
    term::shutdown(&mut f.app);
    wait_no_procs(&tag(&f));
}

#[test]
fn sec_s17_closing_a_live_session_asks_and_a_typed_line_never_goes_to_it() {
    let mut f = fx("");
    login(&mut f, "web1");
    ctrl_backtick(&mut f.app);
    ctrl_backtick(&mut f.app);
    pump(&mut f.app);
    // The tab's x.
    let bar = crate::ui::terminal_view::tab_bar(&f.app, f.app.layout.terminal.width);
    let t = &bar.tabs[0];
    let x = f.app.layout.terminal.x + t.x + t.close.unwrap();
    let y = f.app.layout.terminal.y;
    click(&mut f.app, x, y);
    let screen = pump(&mut f.app);
    assert!(f.app.dialog.is_some(), "{screen}");
    assert!(screen.contains("Close terminal?"), "{screen}");
    key(&mut f.app, KeyCode::Esc);
    assert_eq!(f.app.terminal.tabs.len(), 1);
    // `/run` types into an idle local shell only: here it opens a new terminal tab.
    f.app.terminal.shell = Some(PathBuf::from("/bin/sh"));
    slash(&mut f.app, "/run echo local-only");
    key(&mut f.app, KeyCode::Enter);
    assert_eq!(f.app.terminal.tabs.len(), 2);
    assert!(f.app.terminal.tabs[1].ssh.is_none());
    assert_eq!(f.app.terminal.active, 1);
    // Close, confirmed: the session's processes go.
    f.app.terminal.active = 0;
    pump(&mut f.app);
    let y = f.app.layout.terminal.y;
    click(&mut f.app, x, y);
    key(&mut f.app, KeyCode::Left);
    key(&mut f.app, KeyCode::Enter);
    assert_eq!(f.app.terminal.tabs.len(), 1);
    assert!(f.app.terminal.tabs[0].ssh.is_none());
    wait_no_procs(&tag(&f));
}

// ---- SEC-S2: PATH, SEC-S10: stale state -----------------------------------------------------------

#[test]
fn sec_s10_a_tampered_recent_file_lists_only_valid_targets() {
    let f = fx("");
    std::fs::create_dir_all(f.path("state")).unwrap();
    std::fs::write(
        f.path("state/ssh-recent"),
        "-oProxyCommand=x\na b\ngood@host\n%h\n",
    )
    .unwrap();
    let state = crate::commands::ssh::SshState::load(&f.path("state"));
    let listed: Vec<&str> = state.recent.iter().map(Target::as_str).collect();
    assert_eq!(listed, ["good@host"]);
    assert_eq!(state.recent_file, Some(f.path("state/ssh-recent")));
}

#[test]
fn sec_s2_no_ssh_program_is_an_error_and_starts_nothing() {
    let mut f = fx("");
    f.app.ssh.bin = Some(Path::new("/nonexistent/ssh").to_path_buf());
    f.app.settings.ssh_fullscreen = false;
    connect(&mut f.app, "web1");
    pump(&mut f.app);
    assert!(f.app.terminal.tabs.is_empty());
    assert!(
        notices(&f.app).contains("SSH: could not start"),
        "{}",
        notices(&f.app)
    );
}

#[test]
fn full_screen_render_snapshot() {
    let mut f = fx("");
    login(&mut f, "bob@web1:2222");
    draw(&mut f.app, 60, 8);
    // Printed at the final size, so the screen does not depend on when the resize landed.
    typ(
        &mut f.app,
        "printf '\\033[2J\\033[H\\033[1;32mgreen\\033[0m plain\\n'\n",
    );
    let deadline = Instant::now() + WAIT;
    let t = loop {
        term::refresh(&mut f.app);
        let t = draw(&mut f.app, 60, 8);
        let text = frame_text(&t);
        if text.starts_with("green plain") && text.contains("remote$") {
            break t;
        }
        assert!(Instant::now() < deadline, "{text}");
        std::thread::sleep(Duration::from_millis(10));
    };
    insta::assert_snapshot!("ssh_full_screen_60x8", t.backend());
}
