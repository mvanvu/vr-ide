//! Accounts per agent session (D11, AC-12, SEC-15..18): the picker's account rows, adding an
//! account and starting on it, `/ai <tool> @account`, removing (asks, refused while in use,
//! deletes only its folder), the variable in that agent's environment only, and restore on the
//! same account or not at all. Agents are shell scripts that print their config variables; the
//! state folder is a temp folder.

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use vr_config::AiTool;

use crate::ai::accounts;
use crate::ai::picker::Row;
use crate::app::App;
use crate::app_tests::test_app;
use crate::commands::CmdArgs;
use crate::session::{AiSection, AiSession};

const WAIT: Duration = Duration::from_secs(10);

/// Prints its arguments and both config variables, then waits for input.
const ENV_AGENT: &str = "#!/bin/sh\nprintf 'ARGV:'; printf '%s|' \"$@\"; echo; \
                         printf 'CCD:%s|\\n' \"${CLAUDE_CONFIG_DIR-unset}\"; \
                         printf 'CXH:%s|\\n' \"${CODEX_HOME-unset}\"; exec cat\n";

struct Env {
    _dir: tempfile::TempDir,
    state: PathBuf,
    app: App,
}

fn script(dir: &Path, name: &str, text: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn tool(name: &str, command: &Path) -> AiTool {
    AiTool {
        name: name.into(),
        command: command.display().to_string(),
        args: vec![],
        continue_args: vec!["--continue".into()],
        resume_args: vec![],
    }
}

/// claude and codex (accounts) and gemini (none) as env agents; the state folder in a temp dir.
fn setup() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join(".bin");
    std::fs::create_dir(&bin).unwrap();
    let agent = script(&bin, "agent", ENV_AGENT);
    let state = dir.path().join(".state");
    std::fs::create_dir(&state).unwrap();
    let (mut app, _) = test_app(dir.path());
    app.settings.ai_tools = vec![
        tool("claude", &agent),
        tool("codex", &agent),
        tool("gemini", &agent),
    ];
    app.ai.state_dir = Some(state.clone());
    Env {
        _dir: dir,
        state,
        app,
    }
}

fn press(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        press(app, KeyCode::Char(c));
    }
}

fn wait_for(app: &App, i: usize, want: impl Fn(&str) -> bool) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        let text = app.ai.sessions[i].tab.screen_text();
        if want(&text) {
            return text;
        }
        assert!(Instant::now() < deadline, "timed out; screen:\n{text}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The line the agent printed for its claude variable.
fn ccd(app: &App, i: usize) -> String {
    let text = wait_for(app, i, |s| s.contains("CXH:"));
    text.lines()
        .find_map(|l| l.strip_prefix("CCD:"))
        .unwrap()
        .trim_end()
        .to_string()
}

fn cxh(app: &App, i: usize) -> String {
    let text = wait_for(app, i, |s| s.contains("CXH:"));
    text.lines()
        .find_map(|l| l.strip_prefix("CXH:"))
        .unwrap()
        .trim_end()
        .to_string()
}

fn inherited(var: &str) -> String {
    format!(
        "{}|",
        std::env::var(var).unwrap_or_else(|_| "unset".to_string())
    )
}

/// The `--settings` file a claude session was started with (T3).
fn settings_of(app: &App, i: usize) -> String {
    let report = app.ai.sessions[i]
        .usage_file
        .as_ref()
        .expect("claude usage file");
    report.with_extension("json").display().to_string()
}

fn notices(app: &App) -> Vec<String> {
    app.notices.iter().map(|n| n.text.clone()).collect()
}

fn select(app: &mut App, tool: &str, account: Option<&str>) {
    let p = app.ai.picker.as_mut().unwrap();
    p.selected = p
        .rows
        .iter()
        .position(|r| {
            matches!(r, Row::Choice { tool: t, account: a, .. }
            if t == tool && a.as_deref() == account)
        })
        .unwrap_or_else(|| panic!("no row {tool} {account:?}: {:?}", p.rows));
}

fn select_add(app: &mut App, tool: &str) {
    let p = app.ai.picker.as_mut().unwrap();
    p.selected = p
        .rows
        .iter()
        .position(|r| matches!(r, Row::Add { tool: t } if t == tool))
        .unwrap();
}

/// S2: tools with a variable list `default`, their own accounts, then `+ Add account…`; a tool
/// without one is a single row. An account is listed under the tool it was added under only.
#[test]
fn picker_groups_accounts_per_tool() {
    let mut e = setup();
    accounts::create(&e.state, "claude", "work").unwrap();
    accounts::create(&e.state, "codex", "cx").unwrap();
    press(&mut e.app, KeyCode::F(12));
    let rows = &e.app.ai.picker.as_ref().unwrap().rows;
    let shown: Vec<String> = rows
        .iter()
        .map(|r| match r {
            Row::Header { tool } => format!("[{tool}]"),
            Row::Choice {
                account, single, ..
            } if *single => "single".into(),
            Row::Choice { account, .. } => account.clone().unwrap_or("default".into()),
            Row::Add { tool } => format!("+{tool}"),
        })
        .collect();
    assert_eq!(
        shown,
        [
            "[claude]", "default", "work", "+claude", "[codex]", "default", "cx", "+codex",
            "single"
        ]
    );
}

/// S2 add: Enter on `+ Add account…` opens the name field; a bad or taken name shows the error
/// and keeps the text, nothing is created; a good one makes the 0700 folder and starts the
/// session on it (New, no second step) with the /login notice and the `@account` label.
#[test]
fn add_account_creates_the_folder_and_starts_on_it() {
    let mut e = setup();
    accounts::create(&e.state, "codex", "taken").unwrap();
    press(&mut e.app, KeyCode::F(12));
    select_add(&mut e.app, "claude");
    press(&mut e.app, KeyCode::Enter);
    assert!(e.app.ai.picker.as_ref().unwrap().adding.is_some());
    type_text(&mut e.app, "../x");
    press(&mut e.app, KeyCode::Enter);
    let add = e.app.ai.picker.as_ref().unwrap().adding.clone().unwrap();
    assert_eq!(add.text, "../x", "text kept");
    assert_eq!(add.error.as_deref(), Some("use a-z 0-9 - _ only"));
    assert!(e.app.ai.sessions.is_empty());
    assert_eq!(accounts::folders(&e.state), ["taken"]);
    for _ in 0..4 {
        press(&mut e.app, KeyCode::Backspace);
    }
    type_text(&mut e.app, "taken");
    press(&mut e.app, KeyCode::Enter);
    let add = e.app.ai.picker.as_ref().unwrap().adding.clone().unwrap();
    assert_eq!(add.error.as_deref(), Some("taken is taken (codex)"));
    for _ in 0..5 {
        press(&mut e.app, KeyCode::Backspace);
    }
    // Esc cancels the field only.
    press(&mut e.app, KeyCode::Esc);
    assert!(e.app.ai.picker.as_ref().unwrap().adding.is_none());
    assert!(e.app.ai.visible);
    press(&mut e.app, KeyCode::Enter);
    type_text(&mut e.app, "personal");
    press(&mut e.app, KeyCode::Enter);
    assert_eq!(e.app.ai.sessions.len(), 1, "{:?}", notices(&e.app));
    assert!(e.app.ai.picker.is_none());
    let folder = e.state.join("accounts/personal");
    assert_eq!(std::fs::metadata(&folder).unwrap().mode() & 0o777, 0o700);
    assert_eq!(e.app.ai.label(0), "claude@personal");
    assert_eq!(ccd(&e.app, 0), format!("{}|", folder.display()));
    let text = wait_for(&e.app, 0, |s| s.contains("ARGV:"));
    // claude gets its statusLine settings (T3) before its own (here no) arguments.
    assert!(
        text.replace('\n', "")
            .contains(&format!("ARGV:--settings|{}|CCD:", settings_of(&e.app, 0))),
        "New: {text}"
    );
    assert!(
        notices(&e.app)
            .contains(&"Account personal created; sign in with /login in its tab".to_string()),
        "{:?}",
        notices(&e.app)
    );
    assert_eq!(accounts::for_tool(&e.state, "claude"), ["personal"]);
}

/// AC-12, SEC-18: two claude sessions on two accounts side by side, each with its own folder;
/// a Default session and a codex one get nothing new; vr-ide's own environment never changes.
#[test]
fn sessions_on_accounts_get_their_own_variable_only() {
    let mut e = setup();
    let before = (
        std::env::var_os("CLAUDE_CONFIG_DIR"),
        std::env::var_os("CODEX_HOME"),
    );
    let work = accounts::create(&e.state, "claude", "work").unwrap();
    let alt = accounts::create(&e.state, "claude", "alt").unwrap();
    let cx = accounts::create(&e.state, "codex", "cx").unwrap();
    crate::ai::open_tool(&mut e.app, "claude @work").unwrap();
    crate::ai::open_tool(&mut e.app, "claude @alt").unwrap();
    crate::ai::open_tool(&mut e.app, "claude").unwrap();
    crate::ai::open_tool(&mut e.app, "codex @cx").unwrap();
    crate::ai::open_tool(&mut e.app, "claude @default").unwrap();
    assert_eq!(ccd(&e.app, 0), format!("{}|", work.display()));
    assert_eq!(ccd(&e.app, 1), format!("{}|", alt.display()));
    assert_eq!(ccd(&e.app, 2), inherited("CLAUDE_CONFIG_DIR"));
    assert_eq!(cxh(&e.app, 3), format!("{}|", cx.display()));
    assert_eq!(ccd(&e.app, 3), inherited("CLAUDE_CONFIG_DIR"));
    assert_eq!(ccd(&e.app, 4), inherited("CLAUDE_CONFIG_DIR"));
    assert_eq!(cxh(&e.app, 0), inherited("CODEX_HOME"));
    let labels: Vec<String> = (0..5).map(|i| e.app.ai.label(i)).collect();
    assert_eq!(
        labels,
        [
            "claude@work",
            "claude@alt",
            "claude",
            "codex@cx",
            "claude·2"
        ]
    );
    assert_eq!(
        (
            std::env::var_os("CLAUDE_CONFIG_DIR"),
            std::env::var_os("CODEX_HOME")
        ),
        before,
        "never set in vr-ide itself"
    );
    // Refusals: no mapping, unknown, another tool's, invalid, reserved in another case.
    let err = |app: &mut App, a: &str| crate::ai::open_tool(app, a).unwrap_err();
    assert_eq!(
        err(&mut e.app, "gemini @work"),
        "/ai: gemini cannot use accounts"
    );
    assert_eq!(
        err(&mut e.app, "claude @nope"),
        "/ai: account nope not found for claude"
    );
    assert_eq!(
        err(&mut e.app, "claude @cx"),
        "/ai: account cx not found for claude"
    );
    assert_eq!(
        err(&mut e.app, "claude @../x"),
        "/ai: account name: use a-z 0-9 - _ only"
    );
    assert_eq!(
        err(&mut e.app, "claude @Default"),
        "/ai: account name: default is reserved"
    );
    assert_eq!(e.app.ai.sessions.len(), 5);
    // A folder made unsafe after it was added is refused at start, never run on Default.
    std::fs::remove_dir(&alt).unwrap();
    std::os::unix::fs::symlink(&work, &alt).unwrap();
    assert_eq!(
        err(&mut e.app, "claude @alt"),
        "/ai: account alt not found for claude",
        "a link is not listed"
    );
    let claude = e.app.settings.ai_tools[0].clone();
    let root = e.app.workspace.roots()[0].path.clone();
    let started = crate::ai::spawn_session(
        &mut e.app,
        &claude,
        Some("alt".into()),
        crate::ai::spawn::StartMode::New,
        &root,
        None,
    );
    assert_eq!(
        started.unwrap_err(),
        "account alt: account folder is not a real folder"
    );
    assert_eq!(e.app.ai.sessions.len(), 5);
}

/// S2 / SEC-17: Delete on a named account asks (Cancel first) unless a session is open on it;
/// Remove deletes only that folder; `default` cannot be removed.
#[test]
fn remove_account_asks_refuses_while_in_use_and_deletes_only_it() {
    let mut e = setup();
    let work = accounts::create(&e.state, "claude", "work").unwrap();
    let alt = accounts::create(&e.state, "claude", "alt").unwrap();
    crate::ai::open_tool(&mut e.app, "claude @work").unwrap();
    crate::ai::open_tool(&mut e.app, "claude").unwrap();
    crate::ai::open_tool(&mut e.app, "claude @work").unwrap();
    e.app.run_command("ai.new", CmdArgs::None);
    select(&mut e.app, "claude", Some("work"));
    press(&mut e.app, KeyCode::Delete);
    assert!(e.app.dialog.is_none(), "not asked while in use");
    assert_eq!(
        notices(&e.app).last().unwrap(),
        "Not removed: close the 2 sessions on work first, tabs 1, 3"
    );
    crate::ai::close(&mut e.app, 2);
    select(&mut e.app, "claude", Some("work"));
    press(&mut e.app, KeyCode::Delete);
    assert_eq!(
        notices(&e.app).last().unwrap(),
        "Not removed: close the session on work first, tab 1"
    );
    crate::ai::close(&mut e.app, 0);
    assert!(work.is_dir());
    // default: nothing to remove.
    select(&mut e.app, "claude", None);
    press(&mut e.app, KeyCode::Delete);
    assert!(e.app.dialog.is_none());
    assert_eq!(
        notices(&e.app).last().unwrap(),
        "default is claude's own config"
    );
    // Asks; Cancel is the default.
    select(&mut e.app, "claude", Some("work"));
    press(&mut e.app, KeyCode::Delete);
    let text = format!("{:?}", e.app.dialog.as_ref().expect("asks").widget);
    for want in [
        "Remove account work?",
        "accounts/work/",
        "for good: its claude login, settings and history.",
        "run /logout in a session on work first.",
        "Other accounts and claude's own config stay.",
    ] {
        assert!(text.contains(want), "{want}: {text}");
    }
    press(&mut e.app, KeyCode::Enter);
    assert!(e.app.dialog.is_none());
    assert!(work.is_dir(), "Cancel keeps it");
    // Remove.
    select(&mut e.app, "claude", Some("work"));
    press(&mut e.app, KeyCode::Delete);
    press(&mut e.app, KeyCode::Left);
    press(&mut e.app, KeyCode::Enter);
    assert!(!work.exists(), "{:?}", notices(&e.app));
    assert!(alt.is_dir(), "sibling stays");
    assert!(e.state.join("accounts").is_dir());
    assert_eq!(notices(&e.app).last().unwrap(), "Account work removed");
    let rows = &e.app.ai.picker.as_ref().unwrap().rows;
    assert!(
        !rows
            .iter()
            .any(|r| matches!(r, Row::Choice { account: Some(a), .. } if a == "work")),
        "{rows:?}"
    );
    // A session opened on it after the question is asked still stops the removal.
    select(&mut e.app, "claude", Some("alt"));
    press(&mut e.app, KeyCode::Delete);
    assert!(e.app.dialog.is_some());
    crate::ai::open_tool(&mut e.app, "claude @alt").unwrap();
    crate::ai::answer_remove_account(&mut e.app, "alt");
    assert!(alt.is_dir());
}

/// AC-12, SEC-18: restore reopens each session on its own account; a missing, unsafe, invalid
/// or reserved account is not reopened, one notice each, never on Default; a wide folder is made
/// private before use.
#[test]
fn restore_reopens_on_the_same_account_or_not_at_all() {
    let mut e = setup();
    let root = e.app.workspace.roots()[0].path.canonicalize().unwrap();
    let work = accounts::create(&e.state, "claude", "work").unwrap();
    let alt = accounts::create(&e.state, "claude", "alt").unwrap();
    accounts::create(&e.state, "codex", "cx").unwrap();
    std::fs::set_permissions(&work, std::fs::Permissions::from_mode(0o755)).unwrap();
    let s = |tool: &str, account: Option<&str>| AiSession {
        tool: tool.into(),
        folder: root.clone(),
        account: account.unwrap_or_default().to_string(),
        ..Default::default()
    };
    let state = AiSection {
        shown: true,
        sessions: vec![
            s("claude", Some("work")),
            s("claude", None),
            s("claude", Some("old-team")),
            s("claude", Some("alt")),
            s("claude", Some("../x")),
            s("claude", Some("Default")),
            s("claude", Some("cx")),
            s("gemini", Some("work")),
        ],
        ..Default::default()
    };
    crate::ai::restore::restore(&mut e.app, &state);
    assert_eq!(e.app.ai.sessions.len(), 3, "{:?}", notices(&e.app));
    assert_eq!(e.app.ai.label(0), "claude@work");
    assert_eq!(e.app.ai.label(1), "claude");
    assert_eq!(e.app.ai.label(2), "claude@alt");
    assert_eq!(ccd(&e.app, 0), format!("{}|", work.display()));
    assert_eq!(
        ccd(&e.app, 1),
        inherited("CLAUDE_CONFIG_DIR"),
        "default: no variable"
    );
    assert_eq!(ccd(&e.app, 2), format!("{}|", alt.display()));
    let text = wait_for(&e.app, 0, |s| s.contains("ARGV:"));
    assert!(
        text.replace('\n', "").contains(&format!(
            "ARGV:--settings|{}|--continue|",
            settings_of(&e.app, 0)
        )),
        "{text}"
    );
    assert_eq!(
        std::fs::metadata(&work).unwrap().mode() & 0o777,
        0o700,
        "made private"
    );
    assert_eq!(
        notices(&e.app),
        [
            "Not reopened: claude@old-team, account folder missing (no fallback)",
            "Not reopened: claude@../x, account name invalid (no fallback)",
            "Not reopened: claude@Default, account name invalid (no fallback)",
            "Not reopened: claude@cx, account belongs to codex (no fallback)",
            "Not reopened: gemini@work, gemini cannot use accounts (no fallback)",
        ]
    );
    // The saved state keeps each account.
    let saved = crate::ai::restore::capture(&e.app);
    let kept: Vec<&str> = saved.sessions.iter().map(|s| s.account.as_str()).collect();
    assert_eq!(kept, ["work", "", "alt"]);
}

/// Restart of an exited session on an account whose folder went away is refused, not run on
/// Default.
#[test]
fn restart_checks_the_account_again() {
    let mut e = setup();
    let quick = script(e.state.parent().unwrap(), "quick", "#!/bin/sh\necho RAN\n");
    e.app.settings.ai_tools[0] = tool("claude", &quick);
    let work = accounts::create(&e.state, "claude", "work").unwrap();
    crate::ai::open_tool(&mut e.app, "claude @work").unwrap();
    let deadline = Instant::now() + WAIT;
    while e.app.ai.sessions[0].running() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    // Deleted behind vr-ide's back (the open tab still holds the account).
    std::fs::remove_dir_all(&work).unwrap();
    press(&mut e.app, KeyCode::Enter);
    assert!(
        notices(&e.app)
            .last()
            .unwrap()
            .contains("account work: account folder missing"),
        "{:?}",
        notices(&e.app)
    );
    assert!(!e.app.ai.sessions[0].running());
}

/// D17: an open session holds its account for every window: a removal from elsewhere is
/// refused until the tab is closed.
#[test]
fn an_open_session_holds_its_account_for_other_windows() {
    let mut e = setup();
    let work = accounts::create(&e.state, "claude", "work").unwrap();
    crate::ai::open_tool(&mut e.app, "claude @work").unwrap();
    assert_eq!(
        accounts::remove(&e.state, "work").unwrap_err(),
        "in use in another window"
    );
    assert!(work.is_dir());
    crate::ai::close(&mut e.app, 0);
    accounts::remove(&e.state, "work").unwrap();
    assert!(!work.exists());
    // The answer path names it when another window holds it.
    let alt = accounts::create(&e.state, "claude", "alt").unwrap();
    let other = accounts::hold(&e.state, "alt").unwrap();
    crate::ai::answer_remove_account(&mut e.app, "alt");
    assert_eq!(
        notices(&e.app).last().unwrap(),
        "Not removed: alt, in use in another window"
    );
    assert!(alt.is_dir());
    drop(other);
}

/// AC-2: a session on a named claude account starts with its history linked to the tool's
/// shared folder (an old real folder merged into it); one whose entry cannot be linked starts
/// unshared with one warning and its entry untouched; the default session is on the shared
/// folder itself; a tool without a shared folder is not shared.
#[test]
fn named_sessions_share_the_tools_history() {
    let mut e = setup();
    let work = accounts::create(&e.state, "claude", "work").unwrap();
    let alt = accounts::create(&e.state, "claude", "alt").unwrap();
    let shared = e.state.parent().unwrap().join("home/.claude/projects");
    e.app
        .ai
        .history_dirs
        .insert("claude".into(), shared.clone());
    std::fs::create_dir_all(work.join("projects/-p")).unwrap();
    std::fs::write(work.join("projects/-p/a.jsonl"), "A").unwrap();
    std::fs::write(alt.join("projects"), "file").unwrap();
    crate::ai::open_tool(&mut e.app, "claude @work").unwrap();
    crate::ai::open_tool(&mut e.app, "claude @alt").unwrap();
    crate::ai::open_tool(&mut e.app, "claude").unwrap();
    crate::ai::open_tool(&mut e.app, "codex").unwrap();
    assert_eq!(
        std::fs::read_link(work.join("projects")).unwrap(),
        std::fs::canonicalize(&shared).unwrap()
    );
    assert_eq!(
        std::fs::read_to_string(shared.join("-p/a.jsonl")).unwrap(),
        "A"
    );
    assert_eq!(
        std::fs::read_to_string(alt.join("projects")).unwrap(),
        "file"
    );
    let flags: Vec<bool> = e.app.ai.sessions.iter().map(|s| s.shared).collect();
    assert_eq!(flags, [true, false, true, false]);
    let warned: Vec<String> = notices(&e.app)
        .into_iter()
        .filter(|n| n.contains("history"))
        .collect();
    assert_eq!(
        warned,
        ["claude@alt: history not shared (history entry is not a folder; left as it is)"]
    );
    // A second session on the linked account keeps the link.
    crate::ai::open_tool(&mut e.app, "claude @work").unwrap();
    assert!(e.app.ai.sessions[4].shared);
    assert_eq!(ccd(&e.app, 4), format!("{}|", work.display()));
}
