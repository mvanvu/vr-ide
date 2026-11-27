//! FR-25: SSH sessions from the command window (AC-S1..S6) and the threat model's requirements
//! that need the app (SEC-S2, S4, S7..S10, S12..S18), driven through the `App` API. `ssh` is a
//! shell script here: nothing in this file connects anywhere.
//!
//! The shim logs each call (`log`: `G …` for a configuration probe, `S …` for a session),
//! dumps a session's environment (`env.<pid>`), prints `pre` before and `post` after the
//! "login", runs the `LocalCommand` it was given through a shell as ssh does, and then is a
//! remote shell with the prompt `remote$ `. `mode` picks failures.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, Focus};
use crate::app_tests::test_app;
use crate::clipboard::{Clipboard, MemoryClipboard};
use crate::commands::CmdArgs;
use crate::commands::terminal as term;
use crate::slash_tests::{error, input, key, notices, slash, titles};
use crate::ssh::recent::Entry;
use crate::ssh::{MAX_SESSIONS, NOT_VALID};
use crate::terminal_tests::{click, ctrl_backtick, draw, frame_text};
use vr_term::TermStatus;

const WAIT: Duration = Duration::from_secs(10);

const SHIM: &str = r#"#!/bin/sh
dir=$(dirname "$0")
mode=$(cat "$dir/mode" 2>/dev/null)
if [ "$1" = "-G" ]; then
  printf 'G %s\n' "$*" >> "$dir/log"
  pwd > "$dir/gcwd"
  case "$mode" in
    *usercmd*) echo "localcommand touch $dir/user-ran" ;;
    *ghang*) sleep 300 ;;
    *gslow*) sleep 1 ;;
    *gfail*) exit 1 ;;
  esac
  echo "permitlocalcommand no"
  exit 0
fi
printf 'S %s\n' "$*" >> "$dir/log"
printf '[%s]' "$@" > "$dir/argv"
pwd > "$dir/cwd"
env > "$dir/env.$$"
lc=
for a in "$@"; do case "$a" in LocalCommand=*) lc=${a#LocalCommand=} ;; esac; done
[ -f "$dir/pre" ] && cat "$dir/pre"
case "$mode" in *authfail*) echo "Permission denied (publickey,password)."; exit 255 ;; esac
# Echo goes off before the prompt shows, as in ssh: a password typed at the prompt never echoes.
case "$mode" in *password*) stty -echo; printf 'password: '; read -r pw; stty echo; echo ;; esac
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

    /// The canonical project folder: where ssh runs.
    fn project(&self) -> String {
        self.path("project")
            .canonicalize()
            .unwrap()
            .display()
            .to_string()
    }

    fn recent_file(&self) -> String {
        std::fs::read_to_string(self.path("state/ssh-recent")).unwrap_or_default()
    }
}

/// Writes an executable script that can be started at once. Written in this process, its
/// write descriptor is inherited by every child another test thread forks until that child
/// execs, and starting the script in that window fails with ETXTBSY. `install` copies it from a
/// plain file in its own process instead, so this process never holds the script open for
/// writing.
pub(crate) fn write_executable(path: &Path, contents: &str) {
    let src = path.with_extension("src");
    std::fs::write(&src, contents).unwrap();
    let st = std::process::Command::new("install")
        .arg("-m")
        .arg("0755")
        .arg(&src)
        .arg(path)
        .status()
        .unwrap();
    assert!(st.success(), "install {}", path.display());
    std::fs::remove_file(&src).unwrap();
}

/// An app whose `ssh` is the shim, with a home (and its ssh config `config`) and a state dir
/// in a temp folder, drawn once at 80x24.
fn fx_with(mode: &str, config: &str) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let shim = dir.path().join("ssh");
    write_executable(&shim, SHIM);
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
    f.app.ssh.recent = vec![Entry::new("bob@old:2222", None).unwrap()];
    slash(&mut f.app, "/ssh ");
    assert_eq!(
        titles(&f.app),
        [
            "-p 2222 bob@old  recent",
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
    for (bad, notice) in [
        ("-oProxyCommand=hunter2", NOT_VALID),
        ("-- -oProxyCommand=x", NOT_VALID),
        ("%h", NOT_VALID),
        ("u@h@h", NOT_VALID),
        ("h\u{202e}x", NOT_VALID),
        (
            "-Z hunter2",
            "/ssh: an option ssh does not know, or one without its value",
        ),
        (
            "-p",
            "/ssh: an option ssh does not know, or one without its value",
        ),
        (
            "-i \"hunter2 h",
            "/ssh: unclosed \" (quote paths with spaces, or use \\ )",
        ),
        (
            "-i 'hunter2 h",
            "/ssh: unclosed ' (quote paths with spaces, or use \\ )",
        ),
        ("hunter2\\", "/ssh: a \\ at the end escapes nothing"),
    ] {
        slash(&mut f.app, &format!("/ssh {bad}"));
        key(&mut f.app, KeyCode::Enter);
        assert_eq!(error(&f.app).as_deref(), Some(notice), "{bad:?}");
        key(&mut f.app, KeyCode::Esc);
    }
    // The window takes no control characters; the line check refuses them all the same.
    for bad in ["a\tb", "a\nb", "a\0b"] {
        assert_eq!(
            crate::commands::ssh::connect(&mut f.app, bad),
            Err("/ssh: the line must be one line without control characters".to_string())
        );
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
    assert!(screen.contains(" 1 ssh bob@web1 x "), "{screen}");
    assert!(f.app.layout.editor.height > 0);
    assert!(!session(&f.app, 0).logged_in);
    assert!(f.app.ssh.recent.is_empty(), "not recorded before the login");
    // The probe ran without options of ours; the session with exactly the argv of SEC-S2.
    assert_eq!(f.probes(), ["-G -p 2222 bob@web1"]);
    let args = f.sessions().pop().unwrap();
    let nonce = nonce_in(&args);
    assert_eq!(
        args,
        format!(
            "-o ControlMaster=no -o PermitLocalCommand=yes -o LocalCommand=printf \
             '\\033]7717;vr-ide-ssh;{nonce}\\007' -p 2222 bob@web1"
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
    assert!(rows[23].starts_with(" SSH  bob@web1"), "{}", rows[23]);
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
    assert_eq!(
        f.recent_file(),
        format!("{}\t-p 2222 bob@web1\n", f.project())
    );
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
    assert_eq!(f.sessions(), ["-o ControlMaster=no web1"]);
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
    assert_eq!(f.log(), "S -o ControlMaster=no web1\n");
    assert!(!term::full_screen(&f.app));
    assert_eq!(notices(&f.app), "");
    // A clean exit still makes the target a recent one.
    typ(&mut f.app, "exit\n");
    wait(&mut f.app, "the exit", |a, _| {
        !a.terminal.tabs[0].running() && a.ssh.recent.len() == 1
    });
    assert_eq!(f.recent_file(), format!("{}\tweb1\n", f.project()));
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
    assert_eq!(f.sessions(), ["-o ControlMaster=no web1"]);
    assert!(notices(&f.app).contains("ssh -G failed or took too long"));
    assert!(!term::full_screen(&f.app));
    // The probe is gone (its helpers go with its process group: `ssh::tests`).
    wait_no_procs(&format!("{}/ssh -G", tag(&f)));
}

/// A probe whose thread never answers blocks later sessions only until it is overdue.
#[test]
fn sec_s16_a_stuck_probe_is_given_up_after_its_grace_period() {
    let mut f = fx("");
    let _tx = f.app.ssh.stall_probe("stuck", Duration::from_secs(1));
    connect(&mut f.app, "web1");
    assert!(error(&f.app).unwrap().contains("still starting"));
    key(&mut f.app, KeyCode::Esc);
    let _tx = f.app.ssh.stall_probe("stuck", Duration::from_secs(60));
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
    assert_eq!(f.sessions(), ["-o ControlMaster=no web1"]);
    assert!(!term::full_screen(&f.app));
}

// ---- a probe that answers late (`gslow`: `ssh -G` takes a second) -----------------------------

/// The user goes on in the editor while the configuration is read: the session's tab opens
/// without the keys, so nothing meant for the editor reaches ssh's password prompt.
#[test]
fn ac_s2_a_late_probe_answer_never_takes_keys_typed_elsewhere() {
    let mut f = fx("gslow,password");
    connect(&mut f.app, "web1");
    assert!(f.app.ssh.starting() && f.app.terminal.tabs.is_empty());
    assert_eq!(f.app.focus, Focus::Editor);
    typ(&mut f.app, "x");
    let screen = wait(&mut f.app, "the password prompt", |_, s| {
        s.contains("password:")
    });
    assert_eq!(f.app.focus, Focus::Editor, "{screen}");
    assert!(
        notices(&f.app).contains("SSH web1 ready — click its tab"),
        "{}",
        notices(&f.app)
    );
    // Keys typed now stay out of the session: no login follows.
    typ(&mut f.app, "hunter2\n");
    std::thread::sleep(Duration::from_millis(300));
    let screen = pump(&mut f.app);
    assert!(
        !session(&f.app, 0).logged_in && !screen.contains("Welcome"),
        "{screen}"
    );
    assert!(!term::full_screen(&f.app));
    // Choosing the session gives it the keys; the login then works as usual.
    connect(&mut f.app, "web1");
    assert_eq!(f.app.focus, Focus::Terminal);
    typ(&mut f.app, "pw\n");
    login_wait(&mut f);
}

/// The command window opened while the probe ran: it stays, and closing it returns to where
/// the user was, not to the session.
#[test]
fn ac_s2_a_late_probe_answer_leaves_an_open_command_window_alone() {
    let mut f = fx("gslow");
    connect(&mut f.app, "web1");
    key(&mut f.app, KeyCode::F(1));
    assert_eq!(f.app.focus, Focus::Palette);
    wait(&mut f.app, "the tab", |a, _| a.terminal.tabs.len() == 1);
    assert_eq!(f.app.focus, Focus::Palette);
    key(&mut f.app, KeyCode::Esc);
    assert_eq!(f.app.focus, Focus::Editor);
    assert!(notices(&f.app).contains("SSH web1 ready — click its tab"));
}

/// Typing in another terminal tab while the probe runs: that tab keeps the keys.
#[test]
fn ac_s2_a_late_probe_answer_does_not_switch_the_tab_the_user_types_in() {
    let mut f = fx("gslow");
    ctrl_backtick(&mut f.app);
    assert_eq!(f.app.focus, Focus::Terminal);
    connect(&mut f.app, "web1");
    assert_eq!(f.app.focus, Focus::Terminal);
    typ(&mut f.app, "echo typed-$((1+1))\n");
    wait(&mut f.app, "the second tab", |a, _| {
        a.terminal.tabs.len() == 2
    });
    assert_eq!(f.app.terminal.active, 0);
    assert_eq!(f.app.focus, Focus::Terminal);
    wait(&mut f.app, "the echo in the shell", |_, s| {
        s.contains("typed-2")
    });
}

/// Nothing happened while the probe ran: the session's tab gets the keys and goes full screen
/// after the login.
#[test]
fn ac_s2_a_late_probe_answer_takes_the_keys_when_nothing_happened_meanwhile() {
    let mut f = fx("gslow");
    connect(&mut f.app, "web1");
    assert!(f.app.ssh.starting());
    login_wait(&mut f);
    assert_eq!(f.app.focus, Focus::Terminal);
    assert!(!notices(&f.app).contains("ready"));
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
    let keys = f.path("project/keys");
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
    // Split by quotes, the echoed command never reads FAKE2: only its output does.
    typ(&mut f.app, "printf '\\033[999;1HFA''KE2\\033[1;1H'\n");
    let screen = wait(&mut f.app, "the addressed text", |_, s| {
        s.lines().nth(22).is_some_and(|r| r.contains("FAKE2"))
    });
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
    // It starts in the workspace folder of the active file (SEC-4), as a terminal would.
    typ(&mut f.app, "pwd\n");
    let project = f.project();
    wait(&mut f.app, "pwd", |_, s| {
        s.replace('\n', "").contains(&project)
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
    // A running shim has not necessarily logged its start yet: wait for the log, then make
    // sure the refused one never adds a line.
    let log = f.path("log");
    wait(&mut f.app, "every session logged", move |_, _| {
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        text.lines().filter(|l| l.starts_with("S ")).count() >= MAX_SESSIONS
    });
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
    std::fs::set_permissions(
        f.path("state/ssh-recent"),
        std::os::unix::fs::PermissionsExt::from_mode(0o600),
    )
    .unwrap();
    let state = crate::commands::ssh::SshState::load(&f.path("state"));
    let listed: Vec<&str> = state.recent.iter().map(|e| e.text.as_str()).collect();
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

// ---- FR-29: full screen and the rest of the IDE (F4-1..F4-4) ------------------------------------

fn mouse(app: &mut App, kind: crossterm::event::MouseEventKind, column: u16, row: u16) {
    app.handle_mouse(crossterm::event::MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    });
}

/// D325: a notice raised in full screen is counted on the status row, does not age while it
/// cannot be seen, and is drawn once the IDE layout is back for a whole TTL.
#[test]
fn f4_1_a_notice_raised_in_full_screen_waits_for_the_ide_layout() {
    let mut f = fx("");
    login(&mut f, "web1");
    f.app.notices.clear();
    f.app.info("held-note");
    let screen = pump(&mut f.app);
    let last = screen.lines().last().unwrap().to_string();
    assert!(last.contains("1 notice"), "{last}");
    assert!(!screen.contains("held-note"));
    let t0 = Instant::now();
    f.app.tick(t0 + Duration::from_secs(30));
    assert!(notices(&f.app).contains("held-note"), "expired unseen");
    ctrl_backtick(&mut f.app);
    f.app
        .tick(t0 + Duration::from_secs(30) + Duration::from_millis(10));
    let screen = pump(&mut f.app);
    assert!(screen.contains("held-note"), "{screen}");
    f.app.tick(t0 + Duration::from_secs(34));
    assert!(notices(&f.app).contains("held-note"));
    f.app.tick(t0 + Duration::from_secs(36));
    assert!(!notices(&f.app).contains("held-note"));
}

/// FR-19 drag on a logged-in SSH tab: the tab moves, full screen does not start and the remote
/// program (mouse reporting on) gets no byte of the drag; a click still gives full screen.
#[test]
fn f4_2_dragging_a_logged_in_ssh_tab_moves_it_and_sends_nothing() {
    use crossterm::event::{MouseButton as B, MouseEventKind as K};
    let mut f = fx("");
    // Press/release and button-motion reporting, SGR encoding.
    f.set("post", b"\x1b[?1002h\x1b[?1006h");
    login(&mut f, "web1");
    wait(&mut f.app, "the modes", |a, _| {
        a.terminal.tabs[0].grid.modes.mouse_reporting
    });
    typ(
        &mut f.app,
        "stty raw -echo; echo rea-dy; dd bs=1 count=1 of=keys 2>/dev/null; stty sane\n",
    );
    wait(&mut f.app, "the reader", |_, s| s.contains("rea-dy"));
    f.set("post", b"");
    connect(&mut f.app, "web2");
    wait(&mut f.app, "web2 full screen", |a, _| {
        a.terminal.tabs.len() == 2 && a.terminal.active == 1 && term::full_screen(a)
    });
    ctrl_backtick(&mut f.app);
    ctrl_backtick(&mut f.app);
    pump(&mut f.app);
    assert!(f.app.terminal.visible && !term::full_screen(&f.app));
    let area = f.app.layout.terminal;
    let bar = crate::ui::terminal_view::tab_bar(&f.app, area.width);
    let from = area.x + bar.tabs[0].x + 2;
    let to = area.x + area.width - 2;
    mouse(&mut f.app, K::Down(B::Left), from, area.y);
    pump(&mut f.app);
    assert!(!term::full_screen(&f.app), "the press alone keeps the bar");
    for x in from + 1..=to {
        mouse(&mut f.app, K::Drag(B::Left), x, area.y);
        pump(&mut f.app);
    }
    mouse(&mut f.app, K::Up(B::Left), to, area.y);
    let screen = pump(&mut f.app);
    assert!(!term::full_screen(&f.app), "{screen}");
    assert_eq!(f.app.terminal.active, 1);
    assert_eq!(f.app.terminal.tabs[1].title(), "ssh web1");
    // The next byte web1's reader gets is the one typed now, not a mouse report.
    typ(&mut f.app, "q");
    let keys = f.path("project/keys");
    let deadline = Instant::now() + WAIT;
    while std::fs::metadata(&keys).map_or(0, |m| m.len()) < 1 {
        assert!(Instant::now() < deadline, "the key did not arrive");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(std::fs::read(&keys).unwrap(), b"q");
    // A click (press and release in place) on the tab gives it the window.
    let bar = crate::ui::terminal_view::tab_bar(&f.app, area.width);
    let at = area.x + bar.tabs.iter().find(|t| t.index == 1).unwrap().x + 2;
    mouse(&mut f.app, K::Down(B::Left), at, area.y);
    assert!(!term::full_screen(&f.app));
    mouse(&mut f.app, K::Up(B::Left), at, area.y);
    assert!(term::full_screen(&f.app));
}

/// F1 > Focus Editor leaves full screen to the editor, as ctrl+` does.
#[test]
fn f4_3_focus_editor_from_full_screen_gives_the_editor_back() {
    let mut f = fx("");
    login(&mut f, "web1");
    f.app.run_command("focus.editor", CmdArgs::None);
    let screen = pump(&mut f.app);
    assert!(!term::full_screen(&f.app), "{screen}");
    assert_eq!(f.app.focus, Focus::Editor);
    assert!(f.app.layout.editor.height > 0 && !screen.contains(" SSH  web1"));
    assert!(f.app.terminal.tabs[0].running());
}

/// ctrl+f8 / ctrl+f9 / ctrl+f7 in full screen leave the panel's size and place alone and say so
/// (the notice waits for the IDE layout).
#[test]
fn f4_4_panel_size_and_place_keys_do_nothing_in_full_screen() {
    let mut f = fx("");
    login(&mut f, "web1");
    f.app.notices.clear();
    let (h, w, pos) = (
        f.app.terminal.height_percent,
        f.app.terminal.width_percent,
        f.app.terminal.position,
    );
    for cmd in [
        "terminal.grow",
        "terminal.shrink",
        "terminal.toggle_position",
    ] {
        f.app.run_command(cmd, CmdArgs::None);
    }
    assert_eq!(f.app.terminal.height_percent, h);
    assert_eq!(f.app.terminal.width_percent, w);
    assert_eq!(f.app.terminal.position, pos);
    assert!(term::full_screen(&f.app));
    // Three presses, one notice: copies would wait in the queue for the IDE layout.
    let held = f
        .app
        .notices
        .iter()
        .filter(|n| n.text.contains("IDE layout"))
        .count();
    assert_eq!(held, 1, "{}", notices(&f.app));
}

/// Several notices wait in full screen: once the IDE layout is back each one is in front (the
/// row draws the front one) for a whole TTL, not all gone after the first one's.
#[test]
fn f4_1_each_waiting_notice_gets_its_own_ttl_after_full_screen() {
    let mut f = fx("");
    login(&mut f, "web1");
    f.app.notices.clear();
    f.app.info("first-note");
    f.app.info("second-note");
    let screen = pump(&mut f.app);
    assert!(screen.lines().last().unwrap().contains("2 notices"));
    let t0 = Instant::now();
    f.app.tick(t0);
    f.app.tick(t0 + Duration::from_secs(30));
    ctrl_backtick(&mut f.app);
    f.app
        .tick(t0 + Duration::from_secs(30) + Duration::from_millis(10));
    assert!(pump(&mut f.app).contains("first-note"));
    f.app.tick(t0 + Duration::from_secs(36));
    let screen = pump(&mut f.app);
    assert!(!notices(&f.app).contains("first-note"));
    assert!(screen.contains("second-note"), "{screen}");
    f.app.tick(t0 + Duration::from_secs(41));
    assert!(f.app.notices.is_empty(), "{}", notices(&f.app));
}

/// The key back outranks the notice count on a narrow status row (AC-S3).
#[test]
fn f4_1_the_notice_count_never_takes_the_key_back_s_place() {
    let mut f = fx("");
    login(&mut f, "web1");
    f.app.info("a-note");
    for w in 17..=21 {
        let row = frame_text(&draw(&mut f.app, w, 8))
            .lines()
            .last()
            .unwrap()
            .to_string();
        assert!(!row.contains("notice"), "{w}: {row:?}");
    }
    let row = frame_text(&draw(&mut f.app, 40, 8))
        .lines()
        .last()
        .unwrap()
        .to_string();
    assert!(
        row.contains("1 notice") && row.contains("editor"),
        "{row:?}"
    );
}

/// A press on a logged-in SSH tab that moves off its cell before the release is a drag, not a
/// click, even in the same column: no full screen.
#[test]
fn f4_2_a_press_that_moved_off_its_cell_is_no_click() {
    use crossterm::event::{MouseButton as B, MouseEventKind as K};
    let mut f = fx("");
    login(&mut f, "web1");
    ctrl_backtick(&mut f.app);
    ctrl_backtick(&mut f.app);
    pump(&mut f.app);
    let area = f.app.layout.terminal;
    let x = area.x + 4;
    mouse(&mut f.app, K::Down(B::Left), x, area.y);
    mouse(&mut f.app, K::Drag(B::Left), x, area.y + 1);
    mouse(&mut f.app, K::Drag(B::Left), x, area.y + 3);
    mouse(&mut f.app, K::Up(B::Left), x, area.y + 3);
    pump(&mut f.app);
    assert!(!term::full_screen(&f.app));
    // Back on the press cell before the release still counts as moved.
    mouse(&mut f.app, K::Down(B::Left), x, area.y);
    mouse(&mut f.app, K::Drag(B::Left), x, area.y + 1);
    mouse(&mut f.app, K::Drag(B::Left), x, area.y);
    mouse(&mut f.app, K::Up(B::Left), x, area.y);
    assert!(!term::full_screen(&f.app));
    // A plain click still gives the window.
    mouse(&mut f.app, K::Down(B::Left), x, area.y);
    mouse(&mut f.app, K::Up(B::Left), x, area.y);
    assert!(term::full_screen(&f.app));
}

// ---- AC-6, SEC-1..SEC-5: the argument line (D5) -------------------------------------------------

impl Fx {
    /// The argv of the last session, one `[word]` each.
    fn argv(&self) -> String {
        std::fs::read_to_string(self.path("argv")).unwrap_or_default()
    }

    /// The folder the last session (`cwd`) or probe (`gcwd`) ran in.
    fn ran_in(&self, name: &str) -> String {
        std::fs::read_to_string(self.path(name))
            .unwrap_or_default()
            .trim_end()
            .to_string()
    }
}

fn canonical(p: &Path) -> String {
    p.canonicalize().unwrap().display().to_string()
}

#[test]
fn ac_6_sec_4_relative_paths_resolve_in_the_workspace_folder_of_the_active_file() {
    let mut f = fx("");
    f.app.settings.ssh_fullscreen = false;
    let b = f.path("rootb");
    for root in [f.path("project"), b.clone()] {
        std::fs::create_dir_all(root.join("credentials")).unwrap();
        std::fs::write(root.join("credentials/id_fixecom"), "key").unwrap();
    }
    std::fs::write(b.join("main.rs"), "fn main() {}\n").unwrap();
    f.app.add_root(&b).unwrap();
    f.app.open_path(&b.join("main.rs")).unwrap();
    connect(&mut f.app, "-i credentials/id_fixecom root@app.fixecom.vn");
    assert!(f.app.palette.is_none(), "{:?}", error(&f.app));
    let screen = wait(&mut f.app, "the session", |_, s| s.contains("remote$"));
    assert_eq!(
        f.argv(),
        "[-o][ControlMaster=no][-i][credentials/id_fixecom][root@app.fixecom.vn]"
    );
    assert_eq!(f.ran_in("cwd"), canonical(&b));
    assert!(screen.contains(" 1 ssh root@app.fixecom.vn x "), "{screen}");
    // A clean exit records the line with the folder it ran in.
    typ(&mut f.app, "exit\n");
    wait(&mut f.app, "the exit", |a, _| a.ssh.recent.len() == 1);
    assert_eq!(
        f.recent_file(),
        format!(
            "{}\t-i credentials/id_fixecom root@app.fixecom.vn\n",
            canonical(&b)
        )
    );
}

#[test]
fn ac_6_sec_1_quotes_backslashes_ports_and_jump_hosts_reach_ssh_as_typed() {
    let mut f = fx("");
    f.app.settings.ssh_fullscreen = false;
    connect(
        &mut f.app,
        r#"-p 2222 -J jump -i "my key" -o 'User=a b' user@host echo a\ b '$HOME' ~/x a;b"#,
    );
    assert!(f.app.palette.is_none(), "{:?}", error(&f.app));
    let screen = wait(&mut f.app, "the session", |_, s| s.contains("remote$"));
    let home = f.path("home").display().to_string();
    assert_eq!(
        f.argv(),
        format!(
            "[-o][ControlMaster=no][-p][2222][-J][jump][-i][my key][-o][User=a b][user@host]\
             [echo][a b][$HOME][{home}/x][a;b]"
        )
    );
    assert!(screen.contains(" 1 ssh user@host x "), "{screen}");
}

#[test]
fn ac_6_sec_3_full_screen_status_row_and_probe_with_the_user_s_words() {
    let mut f = fx("");
    let screen = login(&mut f, "-p 2222 -F cfg -o ServerAliveInterval=5 user@host");
    let rows: Vec<&str> = screen.lines().collect();
    assert!(rows[23].starts_with(" SSH  user@host "), "{}", rows[23]);
    // The probe sees the user's `-F` and `-o` words, in the folder the session runs in.
    assert_eq!(
        f.probes(),
        ["-G -p 2222 -F cfg -o ServerAliveInterval=5 user@host"]
    );
    assert_eq!(f.ran_in("gcwd"), f.project());
    assert_eq!(f.ran_in("cwd"), f.project());
    assert!(
        f.argv()
            .ends_with("[-p][2222][-F][cfg][-o][ServerAliveInterval=5][user@host]")
    );
    assert_eq!(
        f.recent_file(),
        format!(
            "{}\t-p 2222 -F cfg -o ServerAliveInterval=5 user@host\n",
            f.project()
        )
    );
    // Plain `/ssh host` of FR-25 still finds its open session.
    ctrl_backtick(&mut f.app);
    connect(
        &mut f.app,
        "-p 2222 -F cfg -o ServerAliveInterval=5 user@host",
    );
    pump(&mut f.app);
    assert_eq!(f.app.terminal.tabs.len(), 1);
    assert!(term::full_screen(&f.app));
}

#[test]
fn sec_3_the_user_s_own_local_command_or_sharing_words_keep_the_prefix_out() {
    let mut f = fx("");
    let lines = [
        "-M h1",
        "-o LocalCommand=true h2",
        "-oPermitLocalCommand=no h3",
        "-o controlpath=/tmp/none h4",
    ];
    let log = f.path("log");
    let started = move || {
        std::fs::read_to_string(&log)
            .unwrap_or_default()
            .lines()
            .filter(|l| l.starts_with("S "))
            .count()
    };
    for (i, line) in lines.iter().enumerate() {
        connect(&mut f.app, line);
        assert!(f.app.palette.is_none(), "{:?}", error(&f.app));
        wait(&mut f.app, "the session", |_, _| started() == i + 1);
    }
    assert_eq!(f.probes(), Vec::<String>::new(), "no probe for these lines");
    assert_eq!(f.sessions(), lines);
    assert!(!term::full_screen(&f.app));
    assert!(
        notices(&f.app).contains("SSH h1: your ssh options set LocalCommand or connection sharing"),
        "{}",
        notices(&f.app)
    );
}

#[test]
fn sec_4_the_session_path_has_no_empty_or_relative_entries() {
    let mut f = fx("");
    f.app.settings.ssh_fullscreen = false;
    let shim_dir = f.dir.path().display().to_string();
    f.app.terminal.env.push((
        "PATH".into(),
        format!(".::rel:{shim_dir}:/usr/bin:/bin").into(),
    ));
    connect(&mut f.app, "web1");
    wait(&mut f.app, "the session", |_, s| s.contains("remote$"));
    let env_file = std::fs::read_dir(f.dir.path())
        .unwrap()
        .flatten()
        .find(|e| e.file_name().to_string_lossy().starts_with("env."))
        .unwrap()
        .path();
    let env = std::fs::read_to_string(env_file).unwrap();
    let path = env.lines().find_map(|l| l.strip_prefix("PATH="));
    assert_eq!(path, Some(format!("{shim_dir}:/usr/bin:/bin").as_str()));
}

#[test]
fn sec_4_a_missing_workspace_folder_refuses_the_start() {
    let mut f = fx("");
    std::fs::remove_dir_all(f.path("project")).unwrap();
    connect(&mut f.app, "web1");
    assert_eq!(
        error(&f.app).as_deref(),
        Some(crate::commands::ssh::BASE_GONE)
    );
    pump(&mut f.app);
    assert!(f.app.terminal.tabs.is_empty() && !f.app.ssh.starting());
    assert_eq!(f.log(), "");
}

#[test]
fn sec_5_a_recent_line_runs_in_the_folder_it_ran_in() {
    let mut f = fx("");
    f.app.settings.ssh_fullscreen = false;
    let a = f.path("roota");
    std::fs::create_dir(&a).unwrap();
    f.app.ssh.recent = vec![
        Entry::new("-i credentials/k web1", Some(a.clone())).unwrap(),
        Entry::new("-i credentials/k gone", Some(f.path("gone"))).unwrap(),
    ];
    slash(&mut f.app, "/ssh ");
    assert_eq!(
        titles(&f.app),
        [
            "-i credentials/k web1  recent",
            "-i credentials/k gone  recent"
        ]
    );
    key(&mut f.app, KeyCode::Enter);
    wait(&mut f.app, "the session", |_, s| s.contains("remote$"));
    assert_eq!(f.ran_in("cwd"), canonical(&a), "not the active root");
    // Its folder is gone: refused, never run somewhere else.
    slash(&mut f.app, "/ssh ");
    key(&mut f.app, KeyCode::Down);
    key(&mut f.app, KeyCode::Enter);
    assert_eq!(
        error(&f.app).as_deref(),
        Some(crate::commands::ssh::RECENT_BASE_GONE)
    );
    assert_eq!(f.sessions().len(), 1);
}

#[test]
fn sec_5_a_recent_line_with_a_local_command_only_fills_the_input() {
    let mut f = fx("");
    f.app.settings.ssh_fullscreen = false;
    let line = r#"-o ProxyCommand="ssh -W %h:%p jump" deploy@db1"#;
    let base = Some(PathBuf::from(f.project()));
    f.app.ssh.recent = vec![
        Entry::new(line, base.clone()).unwrap(),
        Entry::new("web1", base).unwrap(),
    ];
    slash(&mut f.app, "/ssh ");
    key(&mut f.app, KeyCode::Enter);
    assert_eq!(input(&f.app), format!("/ssh {line}"));
    let screen = pump(&mut f.app);
    assert!(
        screen.contains("Has a local-command option (ProxyCommand):"),
        "{screen}"
    );
    assert!(screen.contains("check it; enter runs it"), "{screen}");
    assert_eq!(f.log(), "", "nothing ran yet");
    key(&mut f.app, KeyCode::Enter);
    assert!(f.app.palette.is_none(), "{:?}", error(&f.app));
    wait(&mut f.app, "the session", |_, s| s.contains("remote$"));
    assert!(
        f.argv()
            .contains("[-o][ProxyCommand=ssh -W %h:%p jump][deploy@db1]"),
        "{}",
        f.argv()
    );
}

#[test]
fn ui_s5_the_window_shows_what_runs_and_cuts_long_rows_in_the_middle() {
    let mut f = fx("");
    let long =
        r#"-o ProxyCommand="ssh -W %h:%p jump-host.a-rather-long-name.example.com" deploy@db1"#;
    f.app.ssh.recent = vec![Entry::new(long, Some(PathBuf::from(f.project()))).unwrap()];
    slash(
        &mut f.app,
        "/ssh -i credentials/id_fixecom root@app.fixecom.vn",
    );
    let screen = pump(&mut f.app);
    let runs = screen
        .lines()
        .position(|l| l.contains("runs: ssh -i /"))
        .unwrap_or_else(|| panic!("{screen}"));
    let next = screen.lines().nth(runs + 1).unwrap();
    assert!(
        screen.lines().nth(runs).unwrap().contains("credentials/") || next.contains("credentials/"),
        "{screen}"
    );
    assert!(
        screen.contains("id_fixecom root@app.fixecom.vn"),
        "{screen}"
    );
    key(&mut f.app, KeyCode::Esc);
    slash(&mut f.app, "/ssh ");
    let screen = pump(&mut f.app);
    let row = screen
        .lines()
        .find(|l| l.contains("-o ProxyCommand="))
        .unwrap_or_else(|| panic!("{screen}"));
    assert!(row.contains("… deploy@db1"), "{row}");
    assert!(row.contains("recent"), "{row}");
    assert_eq!(f.log(), "");
}

/// Waits until the `n`-th session tab shows the remote prompt.
fn wait_session(f: &mut Fx, n: usize) {
    wait(&mut f.app, "the session", |a, s| {
        a.terminal.tabs.len() == n && s.contains("remote$")
    });
}

/// Ends the session in the active tab with a clean exit and waits for it.
fn exit_session(f: &mut Fx) {
    typ(&mut f.app, "exit\n");
    wait(&mut f.app, "the exit", |a, _| {
        !a.terminal.tabs[a.terminal.active].running()
    });
}

#[test]
fn sec_4_sec_5_a_recent_line_s_folder_applies_only_to_a_line_picked_from_the_list() {
    let mut f = fx("");
    f.app.settings.ssh_fullscreen = false;
    let (a, b) = (f.path("roota"), f.path("rootb"));
    std::fs::create_dir(&a).unwrap();
    std::fs::create_dir(&b).unwrap();
    std::fs::write(b.join("main.rs"), "fn main() {}\n").unwrap();
    f.app.add_root(&b).unwrap();
    f.app.open_path(&b.join("main.rs")).unwrap();
    let (ca, cb) = (canonical(&a), canonical(&b));
    let line = "-i credentials/k web1";
    f.app.ssh.recent = vec![Entry::new(line, Some(PathBuf::from(&ca))).unwrap()];
    let in_a = format!("in {ca}");

    // Highlighted in the list: it runs where it ran, and the window says so.
    slash(&mut f.app, "/ssh ");
    let screen = pump(&mut f.app);
    assert!(
        screen.contains(&format!("{ca}/credentials/k web1")),
        "{screen}"
    );
    assert!(screen.contains(&in_a), "{screen}");
    key(&mut f.app, KeyCode::Enter);
    wait_session(&mut f, 1);
    assert_eq!(f.ran_in("cwd"), ca);
    exit_session(&mut f);

    // Put in the input from the list: still that folder, until the line is edited.
    slash(&mut f.app, "/ssh -i");
    key(&mut f.app, KeyCode::Tab);
    assert_eq!(input(&f.app), format!("/ssh {line}"));
    assert!(pump(&mut f.app).contains(&in_a));
    key(&mut f.app, KeyCode::Backspace);
    let screen = pump(&mut f.app);
    assert!(
        screen.contains(&format!("{cb}/credentials/k web")),
        "{screen}"
    );
    assert!(!screen.contains(&in_a), "{screen}");
    typ(&mut f.app, "1");
    let screen = pump(&mut f.app);
    assert!(
        !screen.contains(&in_a),
        "an edited line is a typed line: {screen}"
    );
    key(&mut f.app, KeyCode::Esc);
    slash(&mut f.app, "/ssh -i");
    key(&mut f.app, KeyCode::Tab);
    key(&mut f.app, KeyCode::Enter);
    wait_session(&mut f, 2);
    assert_eq!(f.ran_in("cwd"), ca);
    exit_session(&mut f);

    // The same line typed by hand: the active file's folder, as any typed line.
    slash(&mut f.app, &format!("/ssh {line}"));
    let screen = pump(&mut f.app);
    assert!(
        screen.contains(&format!("{cb}/credentials/k web1")),
        "{screen}"
    );
    assert!(!screen.contains(&in_a), "{screen}");
    key(&mut f.app, KeyCode::Enter);
    assert!(f.app.palette.is_none(), "{:?}", error(&f.app));
    wait_session(&mut f, 3);
    assert_eq!(f.ran_in("cwd"), cb, "not the recent line's folder");
}

#[test]
fn sec_4_the_shown_folder_and_the_folder_ssh_runs_in_follow_one_rule() {
    let mut f = fx("");
    f.app.settings.ssh_fullscreen = false;
    let b = f.path("rootb");
    std::fs::create_dir(&b).unwrap();
    f.app.add_root(&b).unwrap();
    let project = f.project();
    std::fs::remove_dir_all(f.path("project")).unwrap();
    // No active file and the first folder is gone: shown there, and refused there, never
    // moved to another folder.
    slash(&mut f.app, "/ssh -i k web1");
    let screen = pump(&mut f.app);
    assert!(screen.contains(&format!("{project}/k web1")), "{screen}");
    key(&mut f.app, KeyCode::Enter);
    assert_eq!(
        error(&f.app).as_deref(),
        Some(crate::commands::ssh::BASE_GONE)
    );
    assert_eq!(f.log(), "");
}

#[test]
fn sec_5_recent_lines_with_a_bundled_i_or_a_config_file_only_fill_the_input() {
    let mut f = fx("");
    f.app.settings.ssh_fullscreen = false;
    let base = Some(PathBuf::from(f.project()));
    for (line, hint) in [
        ("-vI /x.so h1", "Has a local-command option (-I):"),
        ("-F cfg h2", "Uses its own ssh config file (-F):"),
    ] {
        f.app.ssh.recent = vec![Entry::new(line, base.clone()).unwrap()];
        slash(&mut f.app, "/ssh ");
        key(&mut f.app, KeyCode::Enter);
        assert_eq!(input(&f.app), format!("/ssh {line}"));
        let screen = pump(&mut f.app);
        assert!(screen.contains(hint), "{screen}");
        assert!(screen.contains("check it; enter runs it"), "{screen}");
        assert_eq!(f.log(), "", "nothing ran yet");
        key(&mut f.app, KeyCode::Esc);
    }
    // `-F none` reads no file: it runs at once.
    f.app.ssh.recent = vec![Entry::new("-F none h3", base).unwrap()];
    slash(&mut f.app, "/ssh ");
    key(&mut f.app, KeyCode::Enter);
    assert!(f.app.palette.is_none(), "{:?}", error(&f.app));
    wait_session(&mut f, 1);
    assert!(f.argv().ends_with("[-F][none][h3]"), "{}", f.argv());
}

#[test]
fn ui_s5_a_long_command_keeps_its_destination_and_the_box_keeps_its_height() {
    let mut f = fx("");
    let root = f.path("a-folder-with-a-long-name/and-another-long-one/fixecom");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("main.rs"), "fn main() {}\n").unwrap();
    f.app.add_root(&root).unwrap();
    f.app.open_path(&root.join("main.rs")).unwrap();
    // The reviewer's line without `-F` (a line with `-F` shows the config-file hint instead).
    let long = "-i credentials/id_fixecom -o IdentityFile=config/second_key -o \
                UserKnownHostsFile=config/known_hosts root@app.fixecom.vn";
    let base = Some(PathBuf::from(canonical(&root)));
    f.app.ssh.recent = vec![
        Entry::new("web1", base.clone()).unwrap(),
        Entry::new(long, base).unwrap(),
    ];
    slash(&mut f.app, "/ssh ");
    let screen = pump(&mut f.app);
    let short_h = f.app.palette.as_ref().unwrap().area.height;
    assert!(screen.contains("runs: ssh web1"), "{screen}");
    key(&mut f.app, KeyCode::Down);
    let screen = pump(&mut f.app);
    assert_eq!(
        f.app.palette.as_ref().unwrap().area.height,
        short_h,
        "arrowing never resizes the box"
    );
    let rows: Vec<&str> = screen.lines().collect();
    let runs = rows
        .iter()
        .position(|l| l.contains("runs: ssh -i /"))
        .unwrap_or_else(|| panic!("{screen}"));
    let detail: Vec<&str> = rows[runs..runs + 3].to_vec();
    assert!(
        detail.iter().any(|l| l.contains("root@app.fixecom.vn")),
        "the destination stays: {screen}"
    );
    assert!(
        detail.iter().any(|l| l.contains('…')),
        "the cut is marked: {screen}"
    );
    assert!(!rows[runs + 3].contains("fixecom"), "{screen}");
}

/// The detail rows under the list's separator in `screen`.
fn detail_rows(screen: &str) -> Vec<String> {
    screen
        .lines()
        .skip_while(|l| !l.contains("│ ───"))
        .skip(1)
        .take_while(|l| !l.contains('└'))
        .map(|l| {
            // Between the box's borders: what is drawn beside the box is not the box's.
            let inner = match (l.find('│'), l.rfind('│')) {
                (Some(a), Some(b)) if a < b => &l[a + '│'.len_utf8()..b],
                _ => l,
            };
            inner.trim().to_string()
        })
        .collect()
}

#[test]
fn ui_s5_a_cut_command_keeps_the_real_destination_not_the_same_text_later_on() {
    let mut f = fx("");
    let a = "a".repeat(60);
    let b = "b".repeat(60);
    slash(
        &mut f.app,
        &format!(
            "/ssh -o UserKnownHostsFile=config/known_hosts-of-the-team -o \
             IdentityFile=config/ids/a-key-with-a-long-name deploy@db1 printf {a} {b} deploy@db1"
        ),
    );
    let rows = detail_rows(&pump(&mut f.app));
    assert_eq!(rows.len(), 3, "{rows:#?}");
    assert!(
        rows.iter().any(|r| r.contains("deploy@db1 printf")),
        "the destination's own line stays: {rows:#?}"
    );
    assert!(rows.iter().any(|r| r.contains('…')), "{rows:#?}");
}

#[test]
fn ui_s5_one_detail_row_still_names_the_other_folder() {
    let mut f = fx("");
    let a = f.path("roota");
    std::fs::create_dir(&a).unwrap();
    let ca = canonical(&a);
    f.app.ssh.recent = vec![Entry::new("-i credentials/k web1", Some(PathBuf::from(&ca))).unwrap()];
    slash(&mut f.app, "/ssh ");
    let screen = frame_text(&draw(&mut f.app, 80, 8));
    let rows = detail_rows(&screen);
    assert_eq!(rows.len(), 1, "{screen}");
    assert!(rows[0].contains("web1"), "{screen}");
    assert!(
        rows[0].contains("in …") || rows[0].contains(&format!("in {ca}")),
        "{screen}"
    );
    assert!(rows[0].ends_with("roota"), "{screen}");
}
