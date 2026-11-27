//! Auto-switch (AC-6, AC-7, SEC-28..SEC-31): a limited claude session moves to the next working
//! account in the same tab, resumes its own conversation by id and gets the continue prompt once.
//! The agents are scripts; the loop's time is given to each pass, so no test waits for a timer.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::KeyCode;

use crate::ai::limits::{self, AccountState, Zone};
use crate::ai::spawn::StartMode;
use crate::ai::{accounts, switch, usage};
use crate::ai_tests::{
    WAIT, ai_app, canonical, pass, press, say, script, tool, wait_exit, wait_for,
};
use crate::app::{App, Focus};
use crate::session::{AiSection, AiSession};

const ID: &str = "0f8e2c1a-7b3d-4e5f-9a1b-2c3d4e5f6a7b";
const ID2: &str = "11111111-2222-4333-8444-555555555555";
const PROMPT: &str = "Continue where you left off.";

/// A claude for the switch: names its account (from `CLAUDE_CONFIG_DIR`) and arguments, logs its
/// start, its end and every line it is sent in `log`, shows the idle input box (or a login
/// screen for the accounts listed in `login`; for those in `trust` the text of the file
/// `dialog.txt` above the input box), prints the limit line
/// on `limit` (for the accounts in `limited`: after the continue prompt too). Accounts in `slow` end
/// a second after the hang-up; those in `deaf` ignore it; those in `long` print 60 lines first
/// (something to scroll back to).
const AGENT: &str = r#"#!/bin/sh
dir=${0%/*}
case $CLAUDE_CONFIG_DIR in */.state/accounts/*) acct=${CLAUDE_CONFIG_DIR##*/};; *) acct=default;; esac
has() { grep -qx "$acct" "$dir/$1" 2>/dev/null; }
limit() { printf "  \342\216\277  You've hit your 5-hour limit \302\267 resets 3pm\n"; }
rule() { printf '\342\224\200\342\224\200\342\224\200\342\224\200\342\224\200\342\224\200\342\224\200\342\224\200\n'; }
echo "start $acct" >> "$dir/log"
if has slow; then trap 'sleep 1; echo "end $acct" >> "$dir/log"; exit 0' HUP; fi
if has deaf; then trap '' HUP; fi
printf 'ARGV:'; printf '%s|' "$@"; echo
printf 'ACCT:%s\n' "$acct"
if has long; then n=0; while [ $n -lt 60 ]; do n=$((n+1)); echo "line $n"; done; fi
if has login; then printf ' Select login method:\n'
elif has trust; then cat "$dir/dialog.txt"; rule; printf '\342\235\257 \n'; rule
else rule; printf '\342\235\257 \n'; rule; fi
while read cmd; do
  echo "got $acct $cmd" >> "$dir/log"
  case $cmd in
    limit) limit;;
    Continue*) printf 'GOT:%s\n' "$cmd"; if has limited; then limit; fi;;
    *) printf 'GOT:%s\n' "$cmd";;
  esac
done
"#;

struct Env {
    d: tempfile::TempDir,
    app: App,
    state: PathBuf,
    bin: PathBuf,
    /// The loop's clock.
    t: Instant,
}

impl Env {
    fn step(&mut self, ms: u64) {
        self.t += Duration::from_millis(ms);
        pass(&mut self.app, self.t);
    }

    /// Passes at the current time until `done` (real processes need real time).
    fn until(&mut self, what: &str, done: impl Fn(&App) -> bool) {
        let deadline = Instant::now() + WAIT;
        loop {
            pass(&mut self.app, self.t);
            if done(&self.app) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "timed out: {what}; notices {:?}; log {:?}; shared {:?}; seen {:?}",
                self.notices(),
                self.log(),
                self.app
                    .ai
                    .sessions
                    .iter()
                    .map(|s| s.shared)
                    .collect::<Vec<_>>(),
                self.app
                    .ai
                    .sessions
                    .iter()
                    .map(|s| s.limit_seen)
                    .collect::<Vec<_>>()
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Session `i` printed `want`; the loop sees it and then a quiet second.
    fn settle(&mut self, i: usize, want: &str) {
        wait_for(&self.app, i, |s| s.contains(want));
        self.step(0);
        self.step(1100);
    }

    /// Session `i` shows its limit line for a quiet second: the limit event.
    fn limit(&mut self, i: usize) {
        let before = self.app.ai.sessions[i]
            .tab
            .screen_text()
            .matches("hit your")
            .count();
        say(&self.app, i, "limit");
        wait_for(&self.app, i, |s| s.matches("hit your").count() > before);
        self.step(0);
        self.step(1100);
    }

    /// The user types `line` and Enter into session `i`; its answer is seen, then a quiet second.
    fn user_says(&mut self, i: usize, line: &str) {
        self.app.ai.visible = true;
        self.app.ai.active = i;
        self.app.focus = Focus::Ai;
        for c in line.chars() {
            press(&mut self.app, KeyCode::Char(c));
        }
        press(&mut self.app, KeyCode::Enter);
        self.settle(i, &format!("GOT:{line}"));
    }

    fn flag(&self, file: &str, account: &str) {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.bin.join(file))
            .unwrap();
        writeln!(f, "{account}").unwrap();
    }

    /// `account` starts with `text` above its input box.
    fn dialog(&self, account: &str, text: &str) {
        std::fs::write(self.bin.join("dialog.txt"), text).unwrap();
        self.flag("trust", account);
    }

    fn log(&self) -> Vec<String> {
        std::fs::read_to_string(self.bin.join("log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// A session on `account` whose conversation id is `id`.
    fn open(&mut self, account: Option<&str>, id: Option<&str>) -> usize {
        let t = self.app.settings.ai_tools[0].clone();
        let root = canonical(&self.d);
        crate::ai::spawn_session(
            &mut self.app,
            &t,
            account.map(str::to_string),
            StartMode::New,
            &root,
            None,
        )
        .unwrap();
        let i = self.app.ai.sessions.len() - 1;
        self.app.ai.sessions[i].usage = id.map(|id| usage::Usage {
            session_id: Some(id.to_string()),
            ..Default::default()
        });
        wait_for(&self.app, i, |s| s.contains("ACCT:"));
        i
    }

    fn account(&self, i: usize) -> Option<&str> {
        self.app.ai.sessions[i].account.as_deref()
    }

    fn notices(&self) -> Vec<String> {
        self.app.notices.iter().map(|n| n.text.clone()).collect()
    }

    fn known(&self) -> BTreeMap<String, AccountState> {
        limits::load_state(&self.state)
    }

    /// The switch of session `i` to `account` happened and its program printed.
    fn switched(&mut self, i: usize, account: Option<&str>) -> String {
        let want = account.map(str::to_string);
        self.until("switch", |app| app.ai.sessions[i].account == want);
        let name = account.unwrap_or("default");
        wait_for(&self.app, i, |s| s.contains(&format!("ACCT:{name}")))
    }
}

/// claude with the accounts `named` (known to work, sharing the default history; the default
/// account itself was never used: the order after the last named one wraps to it), and a
/// state folder.
fn setup(named: &[&str]) -> Env {
    setup_tool("claude", AGENT, named)
}

/// [`setup`] for `tool` (claude or codex) played by the script `agent`.
fn setup_tool(name: &str, agent: &str, named: &[&str]) -> Env {
    let (d, mut app) = ai_app();
    let state = d.path().join(".state");
    std::fs::create_dir(&state).unwrap();
    app.ai.state_dir = Some(state.clone());
    let bin = d.path().join(".bin");
    let agent = script(&bin, name, agent);
    app.settings.ai_tools[0] = tool(name, &agent, true);
    let history = if name == "codex" {
        ".home/.codex/sessions"
    } else {
        ".home/.claude/projects"
    };
    let shared = d.path().join(history);
    std::fs::create_dir_all(&shared).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o700)).unwrap();
    app.ai
        .history_dirs
        .insert(name.into(), shared.canonicalize().unwrap());
    let mut known = BTreeMap::new();
    for account in named.iter().copied().chain(["default"]) {
        if account != "default" {
            accounts::create(&state, name, account).unwrap();
        }
        known.insert(
            format!("{name}/{account}"),
            AccountState {
                works: account != "default",
                ..Default::default()
            },
        );
    }
    limits::save_state(&state, &known).unwrap();
    Env {
        d,
        app,
        state,
        bin,
        t: Instant::now(),
    }
}

fn clock(at: i64) -> String {
    usage::reset_clock(at, usage::now_secs(), Zone::local())
}

fn count(text: &str, what: &str) -> usize {
    text.matches(what).count()
}

/// AC-6: the limit screen ends the run on `work`, the same tab runs `personal` with `--resume`
/// and the session's own id, the notice names both, and the continue prompt is typed once.
#[test]
fn switches_to_next_working_account_and_resumes() {
    let mut e = setup(&["work", "personal"]);
    let i = e.open(Some("work"), Some(ID));
    let serial = e.app.ai.sessions[i].serial;
    e.app.ai.sessions[i].name = Some("fix".into());
    // Its 90 % warning was shown on `work`.
    e.app.ai.sessions[i].usage = Some(usage::Usage {
        session_id: Some(ID.into()),
        five_hour: Some(usage::Window {
            used_percentage: 95.0,
            resets_at: Some(usage::now_secs() + 3600),
        }),
        ..Default::default()
    });
    usage::warn_high(&mut e.app, i, usage::now_secs());
    assert_ne!(e.app.ai.sessions[i].high_warned, Default::default());

    e.limit(i);
    let text = e.switched(i, Some("personal"));
    assert!(text.contains(&format!("--base|--resume|{ID}|")), "{text}");
    assert!(!text.contains("--continue"), "{text}");
    assert_eq!(e.app.ai.sessions.len(), 1, "the same tab");
    assert_eq!(e.app.ai.sessions[i].serial, serial);
    assert_eq!(e.app.ai.label(i), "fix@personal");
    assert_eq!(e.app.ai.sessions[i].root, canonical(&e.d));
    let until = e.known()["claude/work"].limited_until.unwrap();
    let want = format!(
        "switched claude from work to personal (work limited until {})",
        clock(until)
    );
    assert!(e.notices().contains(&want), "{:?}", e.notices());
    // The new account's windows are not the old one's (D36).
    let s = &e.app.ai.sessions[i];
    assert_eq!(s.high_warned, Default::default());
    let u = s.usage.as_ref().unwrap();
    assert_eq!(u.session_id.as_deref(), Some(ID));
    assert!(u.five_hour.is_none(), "work's usage is not personal's");
    assert!(!s.limit_seen);
    // The locks follow the account: `work` is free, `personal` is held.
    assert!(accounts::hold_exclusive(&e.state, "work").is_ok());
    assert!(accounts::hold_exclusive(&e.state, "personal").is_err());

    // Not before the input box has been there for a quiet second.
    wait_for(&e.app, i, |s| s.contains('❯'));
    e.step(0);
    e.step(900);
    assert_eq!(count(&e.log().join("\n"), "got personal"), 0);
    e.step(200);
    wait_for(&e.app, i, |s| s.contains(&format!("GOT:{PROMPT}")));
    for _ in 0..5 {
        e.step(1100);
    }
    let log = e.log();
    assert_eq!(
        log,
        [
            "start work".to_string(),
            "got work limit".into(),
            "start personal".into(),
            format!("got personal {PROMPT}"),
        ],
        "one run after the other, the prompt once"
    );
}

/// Review focus 2 (SEC-31): two tabs of one account in one folder; the limited one resumes its
/// own id, the other one is not touched.
#[test]
fn switch_resumes_own_session_id_with_two_tabs() {
    let mut e = setup(&["work", "personal"]);
    let a = e.open(Some("work"), Some(ID));
    let b = e.open(Some("work"), Some(ID2));
    let pid_a = e.app.ai.sessions[a].tab.pid();
    e.limit(b);
    let text = e.switched(b, Some("personal"));
    assert!(text.contains(&format!("--resume|{ID2}|")), "{text}");
    assert!(!text.contains(ID), "{text}");
    assert_eq!(
        e.app.ai.sessions[a].tab.pid(),
        pid_a,
        "the other tab runs on"
    );
    assert_eq!(e.account(a), Some("work"));
    e.settle(b, "❯");
    wait_for(&e.app, b, |s| s.contains(&format!("GOT:{PROMPT}")));
    let other = e.app.ai.sessions[a].tab.screen_text();
    assert!(
        !other.contains("GOT:") && !other.contains("--resume"),
        "{other}"
    );
    assert!(!e.log().iter().any(|l| l.starts_with("got work Continue")));
}

/// Review focus 1 (AC-5): 100 % in the report without the limit screen (extra usage) is no limit
/// event: nothing is ended, nothing switched.
#[test]
fn no_switch_at_100_percent_without_limit_screen() {
    let mut e = setup(&["work", "personal"]);
    let i = e.open(Some("work"), Some(ID));
    let pid = e.app.ai.sessions[i].tab.pid();
    e.app.ai.sessions[i].usage = Some(usage::Usage {
        session_id: Some(ID.into()),
        five_hour: Some(usage::Window {
            used_percentage: 100.0,
            resets_at: Some(usage::now_secs() + 3600),
        }),
        ..Default::default()
    });
    say(&e.app, i, "work on");
    e.settle(i, "GOT:work on");
    for _ in 0..5 {
        e.step(1100);
    }
    assert_eq!(e.app.ai.sessions[i].tab.pid(), pid);
    assert!(e.app.ai.sessions[i].running());
    assert_eq!(e.account(i), Some("work"));
    assert!(!e.notices().iter().any(|n| n.contains("switched")));
    assert_eq!(e.log(), ["start work", "got work work on"]);
}

/// Review focus 3 (SEC-30 c): a key that reached the session after the limit — here while the
/// switch runs — and no prompt is typed.
#[test]
fn no_continue_prompt_after_user_input() {
    let mut e = setup(&["work", "personal"]);
    let i = e.open(Some("work"), Some(ID));
    e.limit(i);
    e.app.ai.visible = true;
    e.app.ai.active = i;
    e.app.focus = Focus::Ai;
    press(&mut e.app, KeyCode::Char('x'));
    e.switched(i, Some("personal"));
    e.settle(i, "❯");
    for _ in 0..3 {
        e.step(1100);
    }
    e.step(30_000);
    assert!(
        !e.log().iter().any(|l| l.contains("Continue")),
        "{:?}",
        e.log()
    );
    assert!(!e.notices().iter().any(|n| n.contains("type to continue")));
}

/// A new run's screen seen while scrolled back is not read; back at the bottom it is read again
/// without waiting for more output, and the ready box gets the prompt.
#[test]
fn a_screen_skipped_while_scrolled_back_is_read_again() {
    let mut e = setup(&["work", "personal"]);
    e.flag("long", "personal");
    let i = e.open(Some("work"), Some(ID));
    e.limit(i);
    // Its `ACCT:` line has scrolled off: wait for the box under the 60 lines instead.
    e.until("switch", |app| {
        app.ai.sessions[i].account.as_deref() == Some("personal")
    });
    wait_for(&e.app, i, |s| s.contains("line 60") && s.contains("❯"));
    e.app.ai.sessions[i].tab.scroll_by(10);
    e.step(0);
    e.step(1100);
    e.step(1100);
    assert!(
        e.app.ai.sessions[i].switch.busy(),
        "nothing read while scrolled"
    );
    assert!(!e.log().iter().any(|l| l.starts_with("got personal")));
    e.app.ai.sessions[i].tab.scroll_to_bottom();
    // The view change may count as output: a quiet second after it, no new output from the run.
    e.step(1100);
    e.step(1100);
    wait_for(&e.app, i, |s| s.contains(&format!("GOT:{PROMPT}")));
}

/// The same with the switch alone (no limit check in front of it to re-arm the quiet spell):
/// the scrolled-back rows are never read as the screen, and the screen is read once back at
/// the bottom although nothing new was printed.
#[test]
fn the_switch_alone_never_reads_a_scrolled_back_screen() {
    let mut e = setup(&["work", "personal"]);
    e.flag("long", "personal");
    let i = e.open(Some("work"), Some(ID));
    e.limit(i);
    e.until("switch", |app| {
        app.ai.sessions[i].account.as_deref() == Some("personal")
    });
    wait_for(&e.app, i, |s| s.contains("line 60") && s.contains("❯"));
    assert!(e.app.ai.sessions[i].switch.busy());
    e.app.ai.sessions[i].tab.scroll_by(10);
    let t = e.t;
    switch::tick(&mut e.app, t + Duration::from_millis(1100));
    switch::tick(&mut e.app, t + Duration::from_millis(2200));
    assert!(
        e.app.ai.sessions[i].switch.busy(),
        "not read while scrolled"
    );
    assert!(!e.log().iter().any(|l| l.starts_with("got personal")));
    e.app.ai.sessions[i].tab.scroll_to_bottom();
    switch::tick(&mut e.app, t + Duration::from_millis(3300));
    wait_for(&e.app, i, |s| s.contains(&format!("GOT:{PROMPT}")));
}

/// A limit of the new run while its start is still watched (the user typed into it before the
/// continue prompt) is an event like any other: it is acted on, not used up silently.
#[test]
fn a_limit_while_the_new_run_starts_is_acted_on() {
    let mut e = setup(&["work", "personal"]);
    let i = e.open(Some("work"), Some(ID));
    e.limit(i);
    e.switched(i, Some("personal"));
    assert!(e.app.ai.sessions[i].switch.busy(), "the start is watched");
    e.app.ai.visible = true;
    e.app.ai.active = i;
    e.app.focus = Focus::Ai;
    let before = e.app.ai.sessions[i]
        .tab
        .screen_text()
        .matches("hit your")
        .count();
    for c in "limit".chars() {
        press(&mut e.app, KeyCode::Char(c));
    }
    press(&mut e.app, KeyCode::Enter);
    wait_for(&e.app, i, |s| s.matches("hit your").count() > before);
    e.step(0);
    e.step(1100);
    let again = "claude@personal hit its limit right after the switch; not switching again";
    assert_eq!(
        count(&e.notices().join("\n"), again),
        1,
        "{:?}",
        e.notices()
    );
    assert!(!e.app.ai.sessions[i].switch.busy());
}

/// Review focus 4 (AC-7): the other accounts are limited or were never used: one notice with the
/// earliest reset, the session stays as it is.
#[test]
fn all_limited_notifies_and_keeps_session() {
    let mut e = setup(&["work", "personal", "fresh"]);
    let now = usage::now_secs();
    limits::update_state(&e.state, |m| {
        m.get_mut("claude/personal").unwrap().limited_until = Some(now + 600);
        let d = m.entry("claude/default".into()).or_default();
        d.works = true;
        d.limited_until = Some(now + 7200);
        m.remove("claude/fresh");
    })
    .unwrap();
    let i = e.open(Some("work"), Some(ID));
    let pid = e.app.ai.sessions[i].tab.pid();
    e.limit(i);
    for _ in 0..3 {
        e.step(1100);
    }
    let want = format!("all claude accounts are limited until {}", clock(now + 600));
    assert_eq!(
        count(&e.notices().join("\n"), &want),
        1,
        "{:?}",
        e.notices()
    );
    assert_eq!(e.app.ai.sessions[i].tab.pid(), pid);
    assert!(e.app.ai.sessions[i].running());
    assert_eq!(e.account(i), Some("work"));
    assert_eq!(e.log(), ["start work", "got work limit"]);
    assert!(
        accounts::hold_exclusive(&e.state, "personal").is_ok(),
        "no hold left"
    );
}

/// AC-7, SEC-24: without an id, or with one that is not a canonical UUID, the limit is only
/// noticed.
#[test]
fn no_session_id_notifies_only() {
    for id in [
        None,
        Some("--dangerously-skip-permissions"),
        Some("-x"),
        Some("abc-123"),
        Some("0F8E2C1A-7B3D-4E5F-9A1B-2C3D4E5F6A7B"),
        Some("0f8e2c1a-7b3d-4e5f-9a1b-2c3d4e5f6a7b\n"),
    ] {
        let mut e = setup(&["work", "personal"]);
        let i = e.open(Some("work"), id);
        let pid = e.app.ai.sessions[i].tab.pid();
        e.limit(i);
        for _ in 0..3 {
            e.step(1100);
        }
        assert_eq!(e.app.ai.sessions[i].tab.pid(), pid, "{id:?}");
        assert_eq!(e.account(i), Some("work"));
        let notices = e.notices();
        assert!(notices.iter().any(|n| n.contains("hit its usage limit")));
        assert!(
            notices.contains(&"claude@work: session id not known; not switching accounts".into()),
            "{notices:?}"
        );
        assert_eq!(e.log(), ["start work", "got work limit"]);
    }
}

/// AC-7: `/ai autoswitch off` for the session, or the setting, leaves the limit a notice; the
/// tab's label carries the mark while it is off.
#[test]
fn autoswitch_off_per_session() {
    for by_setting in [false, true] {
        let mut e = setup(&["work", "personal"]);
        e.app.settings.ai_auto_switch = !by_setting;
        let i = e.open(Some("work"), Some(ID));
        e.app.ai.active = i;
        assert_eq!(e.app.ai.sessions[i].auto_switch, !by_setting);
        if !by_setting {
            assert_eq!(switch::off_mark(&e.app.ai.sessions[i]), "");
            slash(&mut e.app, "/ai autoswitch off").unwrap();
            assert!(
                e.notices()
                    .contains(&"auto-switch off for claude@work".into())
            );
        }
        assert_eq!(switch::off_mark(&e.app.ai.sessions[i]), " ⊘");
        let pid = e.app.ai.sessions[i].tab.pid();
        e.limit(i);
        for _ in 0..3 {
            e.step(1100);
        }
        assert_eq!(e.app.ai.sessions[i].tab.pid(), pid);
        assert_eq!(e.account(i), Some("work"));
        assert!(
            e.notices()
                .iter()
                .any(|n| n.contains("hit its usage limit")),
            "{:?}",
            e.notices()
        );
        assert!(!e.notices().iter().any(|n| n.contains("switched")));
        // On again: the next limit event switches.
        slash(&mut e.app, "/ai autoswitch on").unwrap();
        assert_eq!(switch::off_mark(&e.app.ai.sessions[i]), "");
        e.user_says(i, "go");
        assert!(
            !e.app.ai.sessions[i].limit_seen,
            "answered: the event is over"
        );
        e.limit(i);
        e.switched(i, Some("personal"));
    }
    let mut e = setup(&[]);
    assert_eq!(
        slash(&mut e.app, "/ai autoswitch off").unwrap_err(),
        "/ai autoswitch: no AI session is active"
    );
    let i = e.open(None, None);
    e.app.ai.active = i;
    assert_eq!(
        slash(&mut e.app, "/ai autoswitch maybe").unwrap_err(),
        "/ai autoswitch takes on or off"
    );
}

/// AC-7, SEC-29: the target shows its login screen: it is marked not working, nothing is typed
/// into it, and the next account takes over.
#[test]
fn login_screen_target_is_skipped() {
    let mut e = setup(&["work", "personal", "third"]);
    e.flag("login", "personal");
    let i = e.open(Some("work"), Some(ID));
    e.limit(i);
    e.switched(i, Some("personal"));
    e.settle(i, "Select login method:");
    let text = e.switched(i, Some("third"));
    assert!(text.contains(&format!("--resume|{ID}|")), "{text}");
    assert!(!e.known().get("claude/personal").is_some_and(|a| a.works));
    e.settle(i, "❯");
    wait_for(&e.app, i, |s| s.contains(&format!("GOT:{PROMPT}")));
    let log = e.log();
    assert!(
        !log.iter().any(|l| l.starts_with("got personal")),
        "{log:?}"
    );
    assert_eq!(count(&log.join("\n"), "got third"), 1, "{log:?}");
    let notices = e.notices();
    assert!(
        notices.contains(&"claude@personal is not logged in; trying the next account".into()),
        "{notices:?}"
    );
    assert!(
        notices.contains(&"switched claude from personal to third".into()),
        "{notices:?}"
    );
    assert!(accounts::hold_exclusive(&e.state, "personal").is_ok());
    assert!(accounts::hold_exclusive(&e.state, "third").is_err());
}

/// SEC-28: the old run has ended before the new one starts, however long it takes to end.
#[test]
fn the_old_run_ends_before_the_new_one_starts() {
    let mut e = setup(&["work", "personal"]);
    e.flag("slow", "work");
    let i = e.open(Some("work"), Some(ID));
    let pid = e.app.ai.sessions[i].tab.pid();
    e.limit(i);
    // Hung up, still ending: nothing else runs, the account has not changed, Enter starts
    // nothing.
    e.step(100);
    assert_eq!(e.account(i), Some("work"));
    assert_eq!(e.app.ai.sessions[i].tab.pid(), pid);
    assert_eq!(e.log(), ["start work", "got work limit"]);
    e.switched(i, Some("personal"));
    assert_eq!(
        e.log(),
        ["start work", "got work limit", "end work", "start personal"]
    );
}

/// SEC-28: a run that ignores the hang-up is ended by force after five seconds, then the new one
/// starts.
#[test]
fn a_run_ignoring_the_hang_up_is_killed_after_five_seconds() {
    let mut e = setup(&["work", "personal"]);
    e.flag("deaf", "work");
    let i = e.open(Some("work"), Some(ID));
    e.limit(i);
    std::thread::sleep(Duration::from_millis(300));
    e.step(4800);
    assert!(
        e.app.ai.sessions[i].running(),
        "its five seconds are not over"
    );
    assert_eq!(e.account(i), Some("work"));
    e.step(300);
    e.switched(i, Some("personal"));
    assert_eq!(e.log(), ["start work", "got work limit", "start personal"]);
}

/// SEC-28: a run still there two seconds after the kill (a process the kill cannot reach, here
/// a tab whose kill does nothing) is not replaced: the switch is given up with a notice, the
/// session stays on its account and the target's hold is dropped.
#[test]
fn a_run_that_outlives_the_kill_is_not_switched() {
    use crate::commands::terminal::TermTab;
    use vr_term::{Grid, TermStatus};
    let mut e = setup(&["work", "personal"]);
    e.flag("deaf", "work");
    let i = e.open(Some("work"), Some(ID));
    e.limit(i);
    assert!(e.app.ai.sessions[i].switch.busy(), "switching");
    let fake = TermTab::fake(Grid::default(), "claude", TermStatus::Running);
    let real = std::mem::replace(&mut e.app.ai.sessions[i].tab, fake);
    e.step(5000);
    e.step(1900);
    assert!(e.app.ai.sessions[i].switch.busy(), "two seconds not over");
    e.step(100);
    assert!(!e.app.ai.sessions[i].switch.busy());
    assert_eq!(e.account(i), Some("work"));
    let gone = "claude@work did not end; not switched";
    assert_eq!(count(&e.notices().join("\n"), gone), 1, "{:?}", e.notices());
    assert!(
        accounts::hold_exclusive(&e.state, "personal").is_ok(),
        "the target's hold is dropped"
    );
    e.step(60_000);
    assert_eq!(e.account(i), Some("work"), "no second attempt");
    assert!(
        !e.log().iter().any(|l| l.contains("personal")),
        "{:?}",
        e.log()
    );
    real.force_kill();
}

/// SEC-28: an account another window is removing (or setting up) cannot be held: the next one is
/// used; when the new run cannot start, the tab stays exited with a notice and no hold is left.
#[test]
fn a_target_that_cannot_be_held_is_skipped_and_a_failed_start_leaves_no_lock() {
    let mut e = setup(&["work", "personal", "third"]);
    let other_window = accounts::hold_exclusive(&e.state, "personal").unwrap();
    let i = e.open(Some("work"), Some(ID));
    e.limit(i);
    e.switched(i, Some("third"));
    drop(other_window);

    let mut e = setup(&["work", "personal"]);
    e.flag("slow", "work");
    let i = e.open(Some("work"), Some(ID));
    e.limit(i);
    // The program is gone when the old run has ended.
    std::fs::remove_file(e.bin.join("claude")).unwrap();
    e.until("old run over", |app| !app.ai.sessions[0].running());
    e.step(10);
    e.step(10);
    assert!(!e.app.ai.sessions[i].running(), "nothing started");
    assert_eq!(e.account(i), Some("work"));
    let notices = e.notices();
    assert!(
        notices
            .iter()
            .any(|n| n.starts_with("AI: switch to claude@personal failed: ")),
        "{notices:?}"
    );
    assert!(accounts::hold_exclusive(&e.state, "personal").is_ok());
    assert!(
        accounts::hold_exclusive(&e.state, "work").is_ok(),
        "no hold left"
    );
    assert_eq!(e.log().iter().filter(|l| l.starts_with("start")).count(), 1);
    // Enter starts it again by hand, on its account and conversation, with the hold back.
    script(&e.bin, "claude", AGENT);
    e.app.ai.visible = true;
    e.app.ai.active = i;
    e.app.focus = Focus::Ai;
    press(&mut e.app, KeyCode::Enter);
    let text = wait_for(&e.app, i, |s| s.contains("ACCT:work"));
    assert!(text.contains(&format!("--resume|{ID}|")), "{text}");
    assert!(
        accounts::hold_exclusive(&e.state, "work").is_err(),
        "held again"
    );
    crate::ai::close(&mut e.app, i);
    assert!(accounts::remove(&e.state, "personal").is_ok());
    assert!(
        accounts::remove(&e.state, "work").is_ok(),
        "a closed session holds nothing"
    );
}

/// SEC-29: limits on every account: three switches in thirty minutes, then auto-switch is off
/// for the session, with its mark and one notice; a limit right after a switch, before the new
/// account has reported, is not a new event.
#[test]
fn three_switches_in_thirty_minutes_then_notice_only() {
    let mut e = setup(&["work", "a", "b", "c", "d"]);
    for name in ["a", "b", "c", "d"] {
        e.flag("limited", name);
    }
    let i = e.open(Some("work"), Some(ID));
    e.limit(i);
    e.switched(i, Some("a"));
    // The prompt is answered with the limit line at once: within a minute of the switch and
    // without a report from `a`, it does not count.
    e.settle(i, "❯");
    wait_for(&e.app, i, |s| {
        s.contains("GOT:Continue") && s.contains("hit your")
    });
    e.step(0);
    e.step(1100);
    e.step(1100);
    assert_eq!(e.account(i), Some("a"));
    assert!(
        e.notices()
            .contains(&"claude@a hit its limit right after the switch; not switching again".into()),
        "{:?}",
        e.notices()
    );
    // A later one counts: b.
    e.user_says(i, "go");
    e.step(61_000);
    e.limit(i);
    e.switched(i, Some("b"));
    // `b` has reported: its limit right after the prompt is a new event at once: c.
    e.app.ai.sessions[i].limit_watch.works_marked = true;
    e.settle(i, "❯");
    wait_for(&e.app, i, |s| {
        s.contains("GOT:Continue") && s.contains("hit your")
    });
    e.step(0);
    e.step(1100);
    e.switched(i, Some("c"));
    e.settle(i, "❯");
    wait_for(&e.app, i, |s| {
        s.contains("GOT:Continue") && s.contains("hit your")
    });
    e.step(0);
    e.step(1100);
    // And then no more.
    e.user_says(i, "go");
    e.step(61_000);
    e.limit(i);
    for _ in 0..3 {
        e.step(1100);
    }
    assert_eq!(e.account(i), Some("c"), "no fourth switch");
    assert!(e.app.ai.sessions[i].running());
    assert!(!e.app.ai.sessions[i].auto_switch);
    assert_eq!(switch::off_mark(&e.app.ai.sessions[i]), " ⊘");
    let off = "claude@c: 3 account switches in 30 minutes; auto-switch is off for this session";
    assert_eq!(count(&e.notices().join("\n"), off), 1, "{:?}", e.notices());
    assert_eq!(e.log().iter().filter(|l| l.starts_with("start")).count(), 4);
}

/// SEC-30: a folder trust question on the new account: nothing is typed, a notice says to.
/// With a tab of another agent active meanwhile, the prompt still lands only in the switched
/// session.
#[test]
fn the_prompt_goes_only_to_a_ready_input_box_of_that_session() {
    no_prompt_under(TRUST, "Quick safety");

    let mut e = setup(&["work", "personal"]);
    let other = e.open(None, None);
    let i = e.open(Some("work"), Some(ID));
    e.limit(i);
    // The user looks at the other tab while the switch runs.
    e.app.ai.visible = true;
    e.app.ai.active = other;
    e.app.focus = Focus::Ai;
    e.switched(i, Some("personal"));
    e.settle(i, "❯");
    wait_for(&e.app, i, |s| s.contains(&format!("GOT:{PROMPT}")));
    let log = e.log();
    assert!(!log.iter().any(|l| l.starts_with("got default")), "{log:?}");
}

/// The folder trust dialog of claude 2.1.332.
const TRUST: &str = " Accessing workspace:\n\n /home/me/project\n\n Quick safety check: Is this a \
                     project you created or one you trust?\n\n \u{276f} 1. Yes, I trust this \
                     folder\n   2. No, exit\n";

/// The switch target starts with `dialog` (seen once `seen` is on screen) above its input box:
/// the continue prompt is never typed and one notice asks the user to.
fn no_prompt_under(dialog: &str, seen: &str) {
    let mut e = setup(&["work", "personal"]);
    e.dialog("personal", dialog);
    let i = e.open(Some("work"), Some(ID));
    e.limit(i);
    e.switched(i, Some("personal"));
    e.settle(i, seen);
    e.step(1100);
    e.step(30_000);
    let log = e.log();
    assert!(
        !log.iter().any(|l| l.starts_with("got personal")),
        "{dialog}: {log:?}"
    );
    let wait = "claude@personal is waiting: type to continue";
    let notices = e.notices().join("\n");
    assert_eq!(count(&notices, wait), 1, "{dialog}: {notices}");
}

/// SEC-30: the dialogs claude 2.1.332 can draw where its input box is (wording from its
/// strings), shown with no numbered choice: none of them gets the continue prompt.
#[test]
fn no_continue_prompt_while_a_dialog_is_on_screen() {
    for (dialog, seen) in [
        (
            " Quick safety check: Is this a project you created or one you trust?\n",
            "Quick safety",
        ),
        (" Accessing workspace:\n\n /home/me/project\n", "Accessing"),
        (
            " Security notes:\n  Claude can make mistakes.\n",
            "Security notes",
        ),
        (
            " WARNING: Claude Code running in Bypass Permissions mode\n",
            "WARNING",
        ),
        ("   No, exit\n \u{276f} Yes, I accept\n", "I accept"),
        (
            " Allow external CLAUDE.md file imports?\n",
            "Allow external",
        ),
        (" Use Claude Code's terminal setup?\n", "terminal setup"),
        (" New MCP server found in this project: db\n", "New MCP"),
    ] {
        no_prompt_under(dialog, seen);
    }
}

/// The resumed conversation shows its tail again, the old limit line included: that is no new
/// limit event until something was sent to the new run.
#[test]
fn a_replayed_limit_line_is_no_event_before_input() {
    let (d, mut app) = ai_app();
    let state = d.path().join(".state");
    std::fs::create_dir(&state).unwrap();
    app.ai.state_dir = Some(state.clone());
    let agent = script(
        &d.path().join(".bin"),
        "replay",
        "#!/bin/sh\nprintf \"  \\342\\216\\277  You've hit your 5-hour limit \\302\\267 resets 3pm\\n\"\n\
         while read l; do printf \"GOT\\n  \\342\\216\\277  You've hit your 5-hour limit \\302\\267 resets 3pm\\n\"; done\n",
    );
    app.settings.ai_tools[0] = tool("claude", &agent, true);
    let t = app.settings.ai_tools[0].clone();
    crate::ai::spawn_session(
        &mut app,
        &t,
        None,
        StartMode::Continue,
        &canonical(&d),
        None,
    )
    .unwrap();
    wait_for(&app, 0, |s| s.contains("hit your"));
    let t0 = Instant::now();
    pass(&mut app, t0);
    pass(&mut app, t0 + Duration::from_millis(1100));
    assert!(!app.ai.sessions[0].limit_seen, "the earlier run's line");
    assert!(limits::load_state(&state).is_empty());
    // After the user's own line it is the tool's answer.
    app.ai.visible = true;
    app.focus = Focus::Ai;
    press(&mut app, KeyCode::Char('g'));
    press(&mut app, KeyCode::Enter);
    wait_for(&app, 0, |s| s.contains("GOT"));
    pass(&mut app, t0 + Duration::from_millis(2000));
    pass(&mut app, t0 + Duration::from_millis(3100));
    assert!(app.ai.sessions[0].limit_seen);
}

/// A start on "Choose earlier" opens the tool's own list: the keys and the Enter that pick a
/// conversation are no line sent to it, so the replayed tail's limit line is still no event;
/// a limit after the user's first own line is.
#[test]
fn a_replayed_limit_line_after_the_resume_list_is_no_event() {
    let (d, mut app) = ai_app();
    let state = d.path().join(".state");
    std::fs::create_dir(&state).unwrap();
    app.ai.state_dir = Some(state.clone());
    let agent = script(
        &d.path().join(".bin"),
        "picker",
        "#!/bin/sh\nlimit() { printf \"  \\342\\216\\277  You've hit your 5-hour limit \\302\\267 resets 3pm\\n\"; }\n\
         printf ' Resume a conversation\\n\\n \\342\\235\\257 earlier work\\n   other work\\n'\n\
         read pick\nprintf 'PICKED\\n'; limit\n\
         while read l; do [ -z \"$l\" ] && continue; printf 'GOT\\n'; limit; done\n",
    );
    app.settings.ai_tools[0] = tool("claude", &agent, true);
    let t = app.settings.ai_tools[0].clone();
    crate::ai::spawn_session(&mut app, &t, None, StartMode::Resume, &canonical(&d), None).unwrap();
    wait_for(&app, 0, |s| s.contains("other work"));
    // claude ignores an empty line, as this agent does: a second Enter in the list is no line.
    app.ai.visible = true;
    app.focus = Focus::Ai;
    let t0 = Instant::now();
    // Moving in the list, then a quiet read of it.
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Up);
    pass(&mut app, t0);
    pass(&mut app, t0 + Duration::from_millis(1100));
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Enter);
    wait_for(&app, 0, |s| s.contains("PICKED") && s.contains("hit your"));
    pass(&mut app, t0 + Duration::from_millis(2000));
    pass(&mut app, t0 + Duration::from_millis(3100));
    pass(&mut app, t0 + Duration::from_millis(5000));
    assert!(!app.ai.sessions[0].limit_seen, "the earlier run's line");
    assert!(limits::load_state(&state).is_empty());
    assert!(
        app.notices.is_empty(),
        "{:?}",
        app.notices.iter().map(|n| &n.text).collect::<Vec<_>>()
    );
    // The user's own line: what follows is this run's.
    press(&mut app, KeyCode::Char('g'));
    press(&mut app, KeyCode::Enter);
    wait_for(&app, 0, |s| s.contains("GOT"));
    pass(&mut app, t0 + Duration::from_millis(6000));
    pass(&mut app, t0 + Duration::from_millis(7100));
    assert!(app.ai.sessions[0].limit_seen);
}

// ---- codex (AC-8, SEC-32) ---------------------------------------------------------------------

/// A codex for the switch: names its account (from `CODEX_HOME`) and arguments, logs its start
/// and every line it is sent in `log`; on `turn <id>` it runs the `-c notify=[…]` program it was
/// given the way codex 0.144.1 does (argv, the JSON as the last item) with `<id>` as the
/// thread id; on `limit` it prints codex's limit line; on `quit` it ends. Accounts in `login`
/// show the sign-in screen. Resumed with an id listed in `missing`, it prints codex 0.144.1's
/// not-found sentence (as its binary holds it, no prefix) and exits; with one listed in `tail`, the conversation it shows again ends
/// with that line as text, then its input box follows.
const CODEX: &str = r#"#!/bin/sh
dir=${0%/*}
case $CODEX_HOME in */.state/accounts/*) acct=${CODEX_HOME##*/};; *) acct=default;; esac
has() { grep -qx "$acct" "$dir/$1" 2>/dev/null; }
echo "start $acct" >> "$dir/log"
printf 'ARGV:'; printf '%s|' "$@"; echo
printf 'ACCT:%s\n' "$acct"
notify=; prev=; resumed=
for a in "$@"; do
  if [ "$prev" = -c ]; then case $a in notify=*) notify=${a#notify=};; esac; fi
  if [ "$prev" = resume ]; then resumed=$a; fi
  prev=$a
done
item() { printf '%s' "$notify" | grep -o '"[^"]*"' | sed -n "$1p" | tr -d '"'; }
if [ -n "$resumed" ] && grep -qx "$resumed" "$dir/missing" 2>/dev/null; then
  printf 'No saved session found with ID %s. Run `codex resume` without an ID to choose from existing sessions.\n' "$resumed"
  exit 1
fi
if [ -n "$resumed" ] && grep -qx "$resumed" "$dir/tail" 2>/dev/null; then
  printf 'Error: No saved session found with ID %s. Run `codex resume` without an ID to choose from existing sessions.\n\n' "$resumed"
fi
if has login; then printf '  Sign in with ChatGPT to use Codex as part of your paid plan\n'
else printf '\342\200\272 Ask Codex to do anything\n'; fi
while read cmd; do
  echo "got $acct $cmd" >> "$dir/log"
  case $cmd in
    limit) printf "\342\226\240 You've hit your usage limit. Try again at 3:00 PM.\n";;
    quit) exit 0;;
    turn\ *) id=${cmd#turn }
      "$(item 1)" "$(item 2)" "$(item 3)" "{\"type\":\"agent-turn-complete\",\"thread-id\":\"$id\",\"cwd\":\"/w\",\"input-messages\":[\"secret\"]}"
      printf 'TURN:%s\n' "$id";;
    *) printf 'GOT:%s\n' "$cmd";;
  esac
done
"#;

/// Stands in for `vr-ide --ai-codex-notify <report> <json>` (the real one runs in
/// tests/statusline_helper.rs): writes the thread id, whatever it is, as the report's
/// `session_id`, so vr-ide's own checks on reading are what is tested here.
const CODEX_NOTIFY_STAND_IN: &str = r#"#!/bin/sh
[ $# -eq 3 ] && [ "$1" = --ai-codex-notify ] || exit 0
id=$(printf '%s' "$3" | sed -n 's/.*"thread-id":"\([^"]*\)".*/\1/p')
printf '{"session_id":"%s"}' "$id" > "$2"
"#;

fn setup_codex(named: &[&str]) -> Env {
    let mut e = setup_tool("codex", CODEX, named);
    let t = &mut e.app.settings.ai_tools[0];
    t.continue_args = vec!["resume".into(), "--last".into()];
    t.resume_args = vec!["resume".into()];
    e.app.ai.usage_exe = Some(script(&e.bin, "helper", CODEX_NOTIFY_STAND_IN));
    e
}

impl Env {
    /// Session `i` completes a turn whose notify payload names `id`; the loop reads the report.
    fn codex_turn(&mut self, i: usize, id: &str) {
        let reads = self.app.ai.usage_reads;
        say(&self.app, i, &format!("turn {id}"));
        wait_for(&self.app, i, |s| s.contains(&format!("TURN:{id}")));
        for _ in 0..20 {
            self.step(1100);
            if self.app.ai.usage_reads > reads {
                return;
            }
        }
        panic!("no report read; notices {:?}", self.notices());
    }

    /// Enter on session `i` (exited: its tool starts again), once no switch owns the tab.
    fn enter(&mut self, i: usize) {
        self.step(0);
        self.app.ai.visible = true;
        self.app.ai.active = i;
        self.app.focus = Focus::Ai;
        press(&mut self.app, KeyCode::Enter);
    }
}

/// AC-8, SEC-32: codex's id comes from the notify program vr-ide names in `-c notify=[…]`
/// (argv, the report under `ai-usage/`); a limit moves the tab to the next account with
/// `resume <id>` — never `resume --last` — and, codex's input box not being known, nothing is
/// typed: a notice says to type.
#[test]
fn codex_switch_resumes_by_id() {
    let mut e = setup_codex(&["work", "personal"]);
    let i = e.open(Some("work"), None);
    let first = e.app.ai.sessions[i].tab.screen_text().replace('\n', "");
    let report = e.app.ai.sessions[i]
        .usage_file
        .clone()
        .expect("a report path");
    assert!(report.starts_with(e.state.join(usage::DIR)), "{report:?}");
    let helper = e.app.ai.usage_exe.clone().unwrap();
    assert!(
        first.contains(&format!(
            "ARGV:-c|notify=[\"{}\", \"--ai-codex-notify\", \"{}\"]|--base|",
            helper.display(),
            report.display()
        )),
        "{first}"
    );
    assert_eq!(e.app.ai.sessions[i].session_id(), None);
    e.codex_turn(i, ID);
    assert_eq!(e.app.ai.sessions[i].session_id(), Some(ID));
    assert!(e.known()["codex/work"].works);

    e.limit(i);
    let text = e.switched(i, Some("personal")).replace('\n', "");
    assert!(text.contains(&format!("|--base|resume|{ID}|")), "{text}");
    assert!(!text.contains("--last"), "{text}");
    let new_report = e.app.ai.sessions[i].usage_file.clone().unwrap();
    assert_ne!(new_report, report, "each run has its own report");
    assert!(!report.exists(), "the old run's files are gone");
    assert!(
        text.contains(&format!("\"{}\"]|", new_report.display())),
        "{text}"
    );
    let notices = e.notices();
    assert!(
        notices
            .iter()
            .any(|n| n.starts_with("switched codex from work to personal")),
        "{notices:?}"
    );
    assert_eq!(e.app.ai.sessions[i].session_id(), Some(ID));
    for _ in 0..30 {
        e.step(1100);
    }
    assert!(
        e.notices()
            .contains(&"codex@personal is waiting: type to continue".into()),
        "{:?}",
        e.notices()
    );
    assert_eq!(
        e.log(),
        [
            "start work".to_string(),
            format!("got work turn {ID}"),
            "got work limit".into(),
            "start personal".into(),
        ],
        "nothing typed into the new run"
    );
}

/// SEC-24, SEC-32: a thread id that is no canonical UUID (here an option codex would obey) is
/// never kept, so the limit is a notice only.
#[test]
fn codex_forged_thread_id_is_not_resumed() {
    const FORGED: &str = "--dangerously-bypass-approvals-and-sandbox";
    let mut e = setup_codex(&["work", "personal"]);
    let i = e.open(Some("work"), None);
    let pid = e.app.ai.sessions[i].tab.pid();
    e.codex_turn(i, FORGED);
    assert_eq!(e.app.ai.sessions[i].session_id(), None);
    e.limit(i);
    // An info notice: read before the loop's clock runs past its 5 s.
    assert!(
        e.notices()
            .contains(&"codex@work: session id not known; not switching accounts".into()),
        "{:?}",
        e.notices()
    );
    for _ in 0..3 {
        e.step(1100);
    }
    assert_eq!(e.app.ai.sessions[i].tab.pid(), pid);
    assert_eq!(e.account(i), Some("work"));
    assert_eq!(e.log().len(), 3, "{:?}", e.log());
}

/// A codex target showing its sign-in screen is not working: the next account is taken.
#[test]
fn codex_login_screen_target_is_skipped() {
    let mut e = setup_codex(&["work", "personal", "third"]);
    e.flag("login", "personal");
    let i = e.open(Some("work"), None);
    e.codex_turn(i, ID);
    e.limit(i);
    e.switched(i, Some("personal"));
    e.settle(i, "Sign in with ChatGPT");
    let text = e.switched(i, Some("third")).replace('\n', "");
    assert!(text.contains(&format!("|resume|{ID}|")), "{text}");
    assert!(!e.known().get("codex/personal").is_some_and(|a| a.works));
}

/// D62: codex reports only after a finished turn, so a run resumed by id that ends before one is
/// no sign the conversation is missing: Enter and the session file keep the id.
#[test]
fn codex_keeps_its_id_when_a_resumed_run_ends_without_a_turn() {
    let mut e = setup_codex(&["work", "personal"]);
    let i = e.open(Some("work"), None);
    e.codex_turn(i, ID);
    e.limit(i);
    e.switched(i, Some("personal"));
    say(&e.app, i, "quit");
    wait_exit(&e.app, i);
    e.step(0);
    assert_eq!(
        crate::ai::restore::capture(&e.app).sessions[i].session_id,
        ID
    );
    e.enter(i);
    let text = wait_for(&e.app, i, |s| s.contains("ACCT:")).replace('\n', "");
    assert!(text.contains(&format!("|--base|resume|{ID}|")), "{text}");
    assert!(!text.contains("--last"), "{text}");
    assert_eq!(count(&e.log().join("\n"), "start personal"), 2);
}

/// D62: codex's own not-found line drops the id; the next start is a new conversation, never
/// `resume --last` (which could pick another tab's conversation). The line is recognised at
/// every panel width, also where a wrap falls on one of its spaces (a row loses its trailing
/// blanks).
#[test]
fn codex_drops_an_id_it_did_not_find_and_starts_new() {
    for width in [10, 16, 24, 30, 35, 80] {
        let mut e = setup_codex(&["work", "personal"]);
        let i = e.open(Some("work"), None);
        e.codex_turn(i, ID);
        e.flag("missing", ID);
        // The limit is read on the wide screen; the run on `personal` starts `width` wide.
        say(&e.app, i, "limit");
        wait_for(&e.app, i, |s| s.contains("hit your"));
        e.app.layout.ai = ratatui::layout::Rect::new(0, 0, width, 81);
        e.step(0);
        e.step(1100);
        e.until("switch", |app| {
            app.ai.sessions[i].account.as_deref() == Some("personal")
        });
        wait_for(&e.app, i, |s| {
            squeeze(s).contains(&squeeze(&format!("No saved session found with ID {ID}.")))
        });
        wait_exit(&e.app, i);
        e.step(0);
        assert_eq!(
            crate::ai::restore::capture(&e.app).sessions[i].session_id,
            "",
            "width {width}"
        );
        e.enter(i);
        let text = wait_for(&e.app, i, |s| s.contains("ACCT:")).replace('\n', "");
        assert!(text.contains("|--base|"), "width {width}: {text}");
        assert!(
            !text.contains("resume"),
            "width {width}, neither the id nor --last: {text}"
        );
        assert_eq!(e.app.ai.sessions[i].session_id(), None, "width {width}");
        assert_eq!(e.app.ai.sessions[i].mode, StartMode::New, "width {width}");
    }
}

/// D62: only codex's exit message counts. A resumed conversation whose tail merely shows the
/// not-found line, quit before an answer, keeps its id.
#[test]
fn codex_keeps_its_id_when_only_the_conversation_mentions_not_found() {
    let mut e = setup_codex(&["work", "personal"]);
    let i = e.open(Some("work"), None);
    e.codex_turn(i, ID);
    e.flag("tail", ID);
    e.limit(i);
    e.switched(i, Some("personal"));
    say(&e.app, i, "quit");
    wait_exit(&e.app, i);
    e.step(0);
    assert!(
        e.app.ai.sessions[i]
            .tab
            .screen_text()
            .contains("No saved session found with ID"),
        "the tail is on screen"
    );
    assert_eq!(
        crate::ai::restore::capture(&e.app).sessions[i].session_id,
        ID
    );
    e.enter(i);
    let text = wait_for(&e.app, i, |s| s.contains("ACCT:")).replace('\n', "");
    assert!(text.contains(&format!("|--base|resume|{ID}|")), "{text}");
}

/// `text` with every blank removed.
fn squeeze(text: &str) -> String {
    text.split_whitespace().collect()
}

// ---- restart and restore by id (SEC-31, D5) ---------------------------------------------------

const QUICK: &str = "#!/bin/sh\nprintf RAN:; printf '%s|' \"$@\"; echo\n";

/// Enter on an exited session whose id is known resumes that conversation, not the folder's
/// latest; a run started by id that ended without reporting is not tried by id again.
#[test]
fn restart_resumes_by_the_known_id_and_falls_back_after_a_failed_resume() {
    let (d, mut app) = ai_app();
    let quick = script(d.path(), "quick", QUICK);
    app.settings.ai_tools[0] = tool("claude", &quick, true);
    crate::ai::open_tool(&mut app, "claude").unwrap();
    wait_exit(&app, 0);
    wait_for(&app, 0, |s| s.contains("RAN:--base|"));
    app.ai.sessions[0].usage = Some(usage::Usage {
        session_id: Some(ID.into()),
        ..Default::default()
    });
    press(&mut app, KeyCode::Enter);
    let text = wait_for(&app, 0, |s| s.contains("RAN:--base|--"));
    assert!(
        text.contains(&format!("RAN:--base|--resume|{ID}|")),
        "{text}"
    );
    assert!(!text.contains("--continue"), "{text}");
    assert_eq!(app.ai.sessions[0].mode, StartMode::Continue);
    // That run ended without a report (claude: "No conversation found"): the id is given up.
    wait_exit(&app, 0);
    assert_eq!(crate::ai::restore::capture(&app).sessions[0].session_id, "");
    press(&mut app, KeyCode::Enter);
    wait_for(&app, 0, |s| s.contains("RAN:--base|--continue|"));
    assert_eq!(app.ai.sessions[0].usage.as_ref().unwrap().session_id, None);

    // An id that is no canonical UUID never reaches the arguments.
    let (d, mut app) = ai_app();
    let quick = script(d.path(), "quick", QUICK);
    app.settings.ai_tools[0] = tool("claude", &quick, true);
    crate::ai::open_tool(&mut app, "claude").unwrap();
    wait_exit(&app, 0);
    app.ai.sessions[0].usage = Some(usage::Usage {
        session_id: Some("--dangerously-skip-permissions".into()),
        ..Default::default()
    });
    press(&mut app, KeyCode::Enter);
    let text = wait_for(&app, 0, |s| s.contains("RAN:--base|--"));
    assert!(text.contains("RAN:--base|--continue|"), "{text}");
    assert!(!text.contains("dangerously"), "{text}");
}

/// SEC-31 verify: two tabs in one folder with ids X and Y come back each on its own
/// conversation; the id is kept in the session file's section and dropped when it is not a
/// canonical UUID.
#[test]
fn restore_resumes_each_tab_by_its_stored_id() {
    let (d, mut app) = ai_app();
    let root = canonical(&d);
    let entry = |id: &str, label: &str| AiSession {
        tool: "claude".into(),
        folder: root.clone(),
        label: label.into(),
        session_id: id.into(),
        ..Default::default()
    };
    let state = AiSection {
        shown: true,
        width: None,
        sessions: vec![entry(ID, "x"), entry(ID2, "y"), entry("", "z")],
    };
    crate::ai::restore::restore(&mut app, &state);
    assert_eq!(app.ai.sessions.len(), 3);
    for (i, want) in [
        format!("ARGV:--base|--resume|{ID}|"),
        format!("ARGV:--base|--resume|{ID2}|"),
        "ARGV:--base|--continue|".to_string(),
    ]
    .iter()
    .enumerate()
    {
        let text = wait_for(&app, i, |s| s.contains("VRIDE:"));
        assert!(text.contains(want), "{text}");
    }
    // Until a run reports, the id it was started with is what the file keeps.
    let saved = crate::ai::restore::capture(&app);
    let ids: Vec<&str> = saved
        .sessions
        .iter()
        .map(|s| s.session_id.as_str())
        .collect();
    assert_eq!(ids, [ID, ID2, ""]);
}

/// Runs a slash command as the palette's Enter does.
fn slash(app: &mut App, input: &str) -> Result<(), String> {
    let (c, arg) = crate::commands::slash::parse(input)?;
    (c.run)(app, &arg)
}
