//! AI panel behaviour (D7, D12 L1-A, D14, SEC-8): showing and hiding without stopping agents,
//! several sessions of one tool, the picker and its second step, the keys a focused session
//! keeps, `/ai`, closing, restarting, and restore. Agents are small shell scripts named by an
//! absolute path in `ai_tools`; the rendering tests use fake sessions.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use vr_config::{AiTool, TerminalPosition};

use crate::ai::spawn::StartMode;
use crate::app::{App, Focus};
use crate::app_tests::test_app;
use crate::commands::CmdArgs;
use crate::session::{AiSection, AiSession};

pub(crate) const WAIT: Duration = Duration::from_secs(10);

/// Prints its arguments and folder, then echoes what it is sent (an agent waiting for input).
/// The `ARGV:` line is one write, so a test that sees `ARGV:` sees the whole line.
const ECHO_AGENT: &str = "#!/bin/sh\nprintf 'ARGV:%s\\n' \"$(printf '%s|' \"$@\")\"; printf 'CWD:%s\\n' \"$PWD\"; \
                          printf 'VRIDE:%s\\n' \"$(env | grep -c '^VR_IDE_')\"; exec cat\n";
/// Prints its arguments and exits.
const QUICK_AGENT: &str = "#!/bin/sh\nprintf RAN:; printf '%s|' \"$@\"; echo\n";

pub(crate) fn script(dir: &Path, name: &str, text: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

pub(crate) fn tool(name: &str, command: &Path, resumes: bool) -> AiTool {
    AiTool {
        name: name.into(),
        command: command.display().to_string(),
        args: vec!["--base".into()],
        continue_args: if resumes {
            vec!["--continue".into()]
        } else {
            vec![]
        },
        resume_args: if resumes {
            vec!["--resume".into()]
        } else {
            vec![]
        },
    }
}

/// A project with `claude` (resumes) and `gemini` (does not) as echo agents, and `aider`
/// missing.
pub(crate) fn ai_app() -> (tempfile::TempDir, App) {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join(".bin");
    std::fs::create_dir(&bin).unwrap();
    let echo = script(&bin, "agent", ECHO_AGENT);
    let (mut app, _) = test_app(dir.path());
    app.settings.ai_tools = vec![
        tool("claude", &echo, true),
        tool("gemini", &echo, false),
        tool("aider", &bin.join("missing"), false),
    ];
    (dir, app)
}

fn key(app: &mut App, code: KeyCode, mods: KeyModifiers) {
    app.handle_key(KeyEvent::new(code, mods));
}

pub(crate) fn press(app: &mut App, code: KeyCode) {
    key(app, code, KeyModifiers::NONE);
}

pub(crate) fn wait_for(app: &App, i: usize, want: impl Fn(&str) -> bool) -> String {
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

pub(crate) fn wait_exit(app: &App, i: usize) {
    let deadline = Instant::now() + WAIT;
    while app.ai.sessions[i].running() {
        assert!(Instant::now() < deadline, "agent did not exit");
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(crate) fn canonical(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().canonicalize().unwrap()
}

#[test]
fn f12_opens_the_empty_picker_with_installed_tools_and_hides_again() {
    let (_d, mut app) = ai_app();
    press(&mut app, KeyCode::F(12));
    assert!(app.ai.visible);
    assert_eq!(app.focus, Focus::Ai);
    let p = app.ai.picker.as_ref().expect("empty state");
    assert_eq!(p.back, None);
    assert_eq!(p.missing, ["aider"]);
    assert_eq!(p.choice(), Some(("claude", None)), "the first default");
    assert_eq!(
        p.rows.len(),
        4,
        "claude header + default + add account, gemini row"
    );
    // Esc in the empty state hides the panel; f12 hides it too once shown and focused.
    press(&mut app, KeyCode::Esc);
    assert!(!app.ai.visible);
    assert_eq!(app.focus, Focus::Editor);
    press(&mut app, KeyCode::F(12));
    press(&mut app, KeyCode::F(12));
    assert!(!app.ai.visible);
}

/// D14: a tool that resumes asks New / Continue last / Choose earlier; each starts it with its
/// arguments from settings, as argv, in the active file's workspace folder (canonical), without
/// vr-ide's own variables (SEC-8). A tool without resume starts at once.
#[test]
fn picker_second_step_starts_each_mode_with_its_arguments() {
    let (d, mut app) = ai_app();
    press(&mut app, KeyCode::F(12));
    press(&mut app, KeyCode::Enter);
    let second = app.ai.picker.as_ref().unwrap().second.clone().unwrap();
    assert_eq!(
        second.modes,
        [StartMode::New, StartMode::Continue, StartMode::Resume]
    );
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.ai.sessions.len(), 1);
    assert!(app.ai.picker.is_none());
    let text = wait_for(&app, 0, |s| s.contains("VRIDE:"));
    assert!(text.contains("ARGV:--base|--continue|"), "{text}");
    assert!(
        text.contains(&format!("CWD:{}", canonical(&d).display())),
        "{text}"
    );
    assert!(text.contains("VRIDE:0"), "{text}");
    assert_eq!(app.ai.sessions[0].mode, StartMode::Continue);

    // Choose earlier.
    press(&mut app, KeyCode::F(12)); // focused: hides
    key(&mut app, KeyCode::F(12), KeyModifiers::CONTROL);
    assert!(app.ai.picker_active());
    assert_eq!(app.ai.picker.as_ref().unwrap().back, Some(0));
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    let text = wait_for(&app, 1, |s| s.contains("CWD:"));
    assert!(text.contains("ARGV:--base|--resume|"), "{text}");

    // gemini: no second step (below claude's `+ Add account…`).
    key(&mut app, KeyCode::F(12), KeyModifiers::CONTROL);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.ai.sessions.len(), 3);
    let text = wait_for(&app, 2, |s| s.contains("CWD:"));
    assert!(text.contains("ARGV:--base|\n"), "{text}");
    assert_eq!(app.ai.label(0), "claude");
    assert_eq!(app.ai.label(1), "claude·2");
    assert_eq!(app.ai.label(2), "gemini");
}

#[test]
fn esc_steps_back_through_the_picker() {
    let (_d, mut app) = ai_app();
    crate::ai::open_tool(&mut app, "claude").unwrap();
    key(&mut app, KeyCode::F(12), KeyModifiers::CONTROL);
    press(&mut app, KeyCode::Enter);
    assert!(app.ai.picker.as_ref().unwrap().second.is_some());
    press(&mut app, KeyCode::Esc);
    assert!(app.ai.picker.as_ref().unwrap().second.is_none());
    press(&mut app, KeyCode::Esc);
    assert!(app.ai.picker.is_none());
    assert_eq!(app.ai.active, 0);
    assert!(app.ai.visible, "back on the session");
}

/// Agents keep running while hidden; f12 from the editor gives the shown panel the keys; f3
/// gives them back; f7 / shift+f7 step; ctrl+f8 / ctrl+f9 change the width only in the panel.
#[test]
fn hide_show_step_focus_and_width() {
    let (_d, mut app) = ai_app();
    crate::ai::open_tool(&mut app, "gemini").unwrap();
    crate::ai::open_tool(&mut app, "gemini").unwrap();
    assert_eq!(app.ai.active, 1);
    wait_for(&app, 0, |s| s.contains("CWD:"));
    press(&mut app, KeyCode::F(12));
    assert!(!app.ai.visible);
    assert!(app.ai.sessions.iter().all(|s| s.running()), "still running");
    press(&mut app, KeyCode::F(12));
    assert_eq!(app.focus, Focus::Ai);
    press(&mut app, KeyCode::F(3));
    assert_eq!(app.focus, Focus::Editor);
    assert!(app.ai.visible);
    press(&mut app, KeyCode::F(12));
    assert_eq!(app.focus, Focus::Ai, "shown elsewhere: the keys");
    press(&mut app, KeyCode::F(7));
    assert_eq!(app.ai.active, 0);
    key(&mut app, KeyCode::F(7), KeyModifiers::SHIFT);
    assert_eq!(app.ai.active, 1);
    let w = app.ai.width_percent;
    key(&mut app, KeyCode::F(8), KeyModifiers::CONTROL);
    assert_eq!(app.ai.width_percent, w + 5);
    key(&mut app, KeyCode::F(9), KeyModifiers::CONTROL);
    assert_eq!(app.ai.width_percent, w);
    let h = app.terminal.height_percent;
    press(&mut app, KeyCode::F(3));
    key(&mut app, KeyCode::F(8), KeyModifiers::CONTROL);
    assert_eq!(
        app.ai.width_percent, w,
        "from the editor: the terminal's size"
    );
    assert_eq!(app.terminal.height_percent, h + 5);
}

/// A focused session keeps only the `[ai]` keys: ctrl+w, ctrl+n, f5 and shift+f5 reach the agent.
#[test]
fn a_focused_session_gives_other_keys_to_the_agent() {
    let (_d, mut app) = ai_app();
    crate::ai::open_tool(&mut app, "gemini").unwrap();
    // The last startup line: the tty echoes typed keys as they arrive, so keys sent while the agent
    // is still printing would split "hello" across lines.
    wait_for(&app, 0, |s| s.contains("VRIDE:"));
    app.ran.clear();
    for (code, mods) in [
        (KeyCode::Char('w'), KeyModifiers::CONTROL),
        (KeyCode::Char('n'), KeyModifiers::CONTROL),
        (KeyCode::F(5), KeyModifiers::NONE),
        (KeyCode::F(5), KeyModifiers::SHIFT),
    ] {
        key(&mut app, code, mods);
    }
    assert!(app.ran.is_empty(), "{:?}", app.ran);
    assert_eq!(app.ai.sessions.len(), 1);
    for c in "hello".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    press(&mut app, KeyCode::Enter);
    wait_for(&app, 0, |s| s.matches("hello").count() >= 2);
}

#[test]
fn close_asks_while_running_and_the_last_leaves_the_empty_state() {
    let (_d, mut app) = ai_app();
    crate::ai::open_tool(&mut app, "gemini").unwrap();
    app.run_command("ai.close", CmdArgs::None);
    assert!(app.dialog.is_some(), "asks while the agent runs");
    // Close is the first button; Cancel is selected.
    press(&mut app, KeyCode::Left);
    press(&mut app, KeyCode::Enter);
    assert!(app.ai.sessions.is_empty());
    assert!(app.ai.visible);
    assert!(app.ai.picker_active(), "empty state");
}

/// An exited session keeps its screen; Enter starts it again with the continue arguments.
#[test]
fn enter_restarts_an_exited_session_with_continue_args() {
    let (d, mut app) = ai_app();
    let quick = script(d.path(), "quick", QUICK_AGENT);
    // Not codex: codex without an id starts new (FR-1 C1, ai_switch_tests).
    app.settings
        .ai_tools
        .push(tool("cursor-agent", &quick, true));
    crate::ai::open_tool(&mut app, "cursor-agent").unwrap();
    wait_exit(&app, 0);
    wait_for(&app, 0, |s| s.contains("RAN:--base|"));
    press(&mut app, KeyCode::Enter);
    wait_for(&app, 0, |s| s.contains("RAN:--base|--continue|"));
    assert_eq!(app.ai.sessions[0].mode, StartMode::Continue);
}

#[test]
fn slash_ai_opens_in_the_panel_and_renames() {
    let (_d, mut app) = ai_app();
    assert_eq!(
        crate::ai::open_tool(&mut app, "echo hi").unwrap_err(),
        "/ai takes <tool> [@account]"
    );
    assert!(
        crate::ai::open_tool(&mut app, "bash")
            .unwrap_err()
            .contains("bash is not in ai_tools (claude, gemini, aider)")
    );
    assert!(
        crate::ai::open_tool(&mut app, "aider")
            .unwrap_err()
            .contains("aider not on PATH")
    );
    assert!(crate::ai::open_tool(&mut app, "claude @work").is_err());
    assert!(app.ai.sessions.is_empty());
    assert!(app.terminal.tabs.is_empty(), "never a terminal tab");
    app.run_command("app.palette", CmdArgs::None);
    for c in "/ai claude".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.ai.sessions.len(), 1);
    assert_eq!(app.focus, Focus::Ai);
    let text = wait_for(&app, 0, |s| s.contains("CWD:"));
    assert!(text.contains("ARGV:--base|\n"), "New: {text}");
    app.run_command("ai.rename", CmdArgs::None);
    for c in "review".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.ai.label(0), "review");
    assert!(crate::ai::rename(&mut app, "a\u{202e}b").is_err());
    assert!(crate::ai::rename(&mut app, "").is_err());
}

/// L1-A (D12): a right terminal is drawn at the bottom while the panel shows, back on the right
/// once it hides; ctrl+f7 while it shows says so.
#[test]
fn right_terminal_goes_below_the_editor_while_the_panel_shows() {
    let (_d, mut app) = ai_app();
    app.terminal.shell = Some(PathBuf::from("/bin/sh"));
    crate::commands::terminal::new_terminal(&mut app);
    app.run_command("terminal.toggle_position", CmdArgs::None);
    assert_eq!(app.terminal.position, TerminalPosition::Right);
    crate::terminal_tests::draw(&mut app, 120, 40);
    assert!(app.layout.terminal_sep.width > 0, "on the right");
    crate::ai::open_tool(&mut app, "gemini").unwrap();
    crate::terminal_tests::draw(&mut app, 120, 40);
    assert_eq!(app.layout.terminal_sep.width, 0, "at the bottom");
    assert!(app.layout.terminal.y > app.layout.editor.y);
    assert!(app.layout.terminal.x + app.layout.terminal.width <= app.layout.ai_sep.x);
    assert_eq!(
        app.terminal.position,
        TerminalPosition::Right,
        "setting kept"
    );
    app.run_command("terminal.toggle_position", CmdArgs::None);
    app.run_command("terminal.toggle_position", CmdArgs::None);
    assert!(
        app.notices
            .iter()
            .any(|n| n.text == "Terminal goes to the right when the AI panel hides (f12)"),
        "{:?}",
        app.notices
    );
    press(&mut app, KeyCode::F(12));
    crate::terminal_tests::draw(&mut app, 120, 40);
    assert!(app.layout.terminal_sep.width > 0, "back on the right");
}

#[test]
fn quit_counts_running_sessions() {
    let (_d, mut app) = ai_app();
    crate::ai::open_tool(&mut app, "gemini").unwrap();
    crate::ai::open_tool(&mut app, "gemini").unwrap();
    app.run_command("app.quit", CmdArgs::None);
    let d = app.dialog.as_ref().expect("asks");
    let text = format!("{:?}", d.widget);
    assert!(
        text.contains("2 AI sessions still running; quitting ends them."),
        "{text}"
    );
}

/// Restore (D7, SEC-8, SEC-18): each saved session reopens with its tool's continue arguments,
/// no input sent, the keys left where they were; a tool not in `ai_tools` by name or not on
/// `PATH`, an account, or a folder outside the workspace is not reopened, with one notice each,
/// and nothing runs.
#[test]
fn restore_reopens_with_continue_args_and_refuses_the_rest() {
    let (d, mut app) = ai_app();
    let marker = d.path().join("p");
    let root = canonical(&d);
    let other = tempfile::tempdir().unwrap();
    let entry = |tool: &str, folder: &Path, account: &str, label: &str| AiSession {
        tool: tool.into(),
        account: account.into(),
        folder: folder.to_path_buf(),
        label: label.into(),
        ..Default::default()
    };
    let state = AiSection {
        shown: true,
        width: Some(55),
        sessions: vec![
            entry(
                &format!("claude; touch {}", marker.display()),
                &root,
                "",
                "",
            ),
            entry("claude", &root, "old-team", ""),
            entry("gemini", other.path(), "", ""),
            entry("aider", &root, "", ""),
            entry("claude", &root, "", "review"),
        ],
    };
    crate::ai::restore::restore(&mut app, &state);
    assert_eq!(app.ai.sessions.len(), 1);
    assert_eq!(app.ai.label(0), "review");
    assert_eq!(app.ai.width_percent, 55);
    assert!(app.ai.visible);
    assert_eq!(app.focus, Focus::Editor, "keys stay");
    let text = wait_for(&app, 0, |s| s.contains("VRIDE:"));
    assert!(text.contains("ARGV:--base|--continue|"), "{text}");
    let errors: Vec<&str> = app.notices.iter().map(|n| n.text.as_str()).collect();
    assert_eq!(errors.len(), 4, "{errors:?}");
    assert!(errors[0].contains("is not in ai_tools"), "{errors:?}");
    assert_eq!(
        errors[1],
        "Not reopened: claude@old-team, account folder missing (no fallback)"
    );
    assert!(errors[2].ends_with("(folder missing)"), "{errors:?}");
    assert!(errors[3].ends_with("aider not on PATH"), "{errors:?}");
    std::thread::sleep(Duration::from_millis(100));
    assert!(!marker.exists(), "nothing ran");
    let screen = app.ai.sessions[0].tab.screen_text();
    assert_eq!(
        screen.trim_end().lines().count(),
        3,
        "no input sent: {screen}"
    );
    // What a session file keeps: names, folders and the layout.
    let saved = crate::ai::restore::capture(&app);
    assert_eq!(
        saved,
        AiSection {
            shown: true,
            width: Some(55),
            sessions: vec![entry("claude", &root, "", "review")],
        }
    );
}

/// A `--session` window as `main` starts it (lock, workspace, session restore, then the AI
/// panel's restore), with the test agents in `ai_tools`.
fn session_window(state: &Path, home: &Path, tools: &[AiTool]) -> App {
    let started = crate::session::start(state, home);
    let mut app = App::with_workspace(
        started.workspace,
        vr_config::Keymap::load(None).0,
        vr_config::Settings::default(),
        Box::new(crate::clipboard::MemoryClipboard::new()),
    );
    app.enter_settle = Duration::ZERO;
    app.enter_repeat_gap = Duration::ZERO;
    app.settings.ai_tools = tools.to_vec();
    crate::session::restore(&mut app, started.restore);
    crate::ai::restore::from_session(&mut app);
    app
}

fn quit_window(mut app: App) {
    crate::commands::terminal::shutdown(&mut app);
    crate::ai::shutdown(&mut app);
}

struct SessionDirs {
    _dir: tempfile::TempDir,
    state: PathBuf,
    home: PathBuf,
    tools: Vec<AiTool>,
}

fn session_dirs() -> SessionDirs {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let home = base.join("home");
    let bin = base.join("bin");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    let echo = script(&bin, "agent", ECHO_AGENT);
    SessionDirs {
        state: base.join("state"),
        tools: vec![tool("claude", &echo, true), tool("gemini", &echo, false)],
        _dir: dir,
        home,
    }
}

/// The panel goes into `session.toml` as it changes and comes back on the next `--session`
/// launch: shown, width, every session with its name, each started with its continue
/// arguments; the file holds no command line.
#[test]
fn the_panel_round_trips_through_the_session_file() {
    let t = session_dirs();
    let mut app = session_window(&t.state, &t.home, &t.tools);
    assert!(app.ai.sessions.is_empty());
    crate::ai::open_tool(&mut app, "claude").unwrap();
    crate::ai::rename(&mut app, "review").unwrap();
    crate::ai::open_tool(&mut app, "gemini").unwrap();
    app.run_command("ai.grow", CmdArgs::None);
    let width = app.ai.width_percent;
    assert_ne!(width, app.settings.ai_width_percent);
    // The debounced save sees the change (CHECK_EVERY + SAVE_DELAY later).
    let now = Instant::now();
    app.tick(now);
    app.tick(now + Duration::from_secs(1));
    app.tick(now + Duration::from_secs(2));
    crate::session::flush(&mut app);
    let body = std::fs::read_to_string(crate::session::file_for_tests(&app)).unwrap();
    assert!(body.contains("label = \"review\""), "{body}");
    assert!(body.contains("tool = \"gemini\""), "{body}");
    assert!(
        !body.contains("--base") && !body.contains("agent"),
        "{body}"
    );
    crate::session::save_on_quit(&mut app);
    quit_window(app);

    let mut app = session_window(&t.state, &t.home, &t.tools);
    assert_eq!(app.ai.sessions.len(), 2);
    assert!(app.ai.visible);
    assert_eq!(app.ai.width_percent, width);
    assert_eq!(app.ai.label(0), "review");
    assert_eq!(app.focus, Focus::Editor, "keys stay");
    let claude = wait_for(&app, 0, |s| s.contains("VRIDE:"));
    assert!(claude.contains("ARGV:--base|--continue|"), "{claude}");
    assert!(
        claude.contains(&format!("CWD:{}", t.home.display())),
        "{claude}"
    );
    let gemini = wait_for(&app, 1, |s| s.contains("VRIDE:"));
    assert!(gemini.contains("ARGV:--base|\n"), "{gemini}");
    // Hidden and one session closed: the next save says so.
    press(&mut app, KeyCode::F(12));
    crate::ai::close(&mut app, 1);
    crate::session::save_on_quit(&mut app);
    let body = std::fs::read_to_string(crate::session::file_for_tests(&app)).unwrap();
    assert!(body.contains("shown = false"), "{body}");
    assert!(!body.contains("gemini"), "{body}");
    quit_window(app);
}

/// Two `--session` windows (the launcher clicked twice): only the one that keeps the session
/// resumes the saved agents; the second starts none, so no conversation runs twice.
#[test]
fn a_second_session_window_resumes_no_agent() {
    let t = session_dirs();
    let mut app = session_window(&t.state, &t.home, &t.tools);
    crate::ai::open_tool(&mut app, "claude").unwrap();
    crate::session::save_on_quit(&mut app);
    quit_window(app);

    let keeper = session_window(&t.state, &t.home, &t.tools);
    assert_eq!(keeper.ai.sessions.len(), 1);
    let claude = wait_for(&keeper, 0, |s| s.contains("VRIDE:"));
    assert!(claude.contains("ARGV:--base|--continue|"), "{claude}");

    let mut second = session_window(&t.state, &t.home, &t.tools);
    assert!(second.ai.sessions.is_empty(), "nothing resumed twice");
    assert!(!second.ai.visible);
    let notices: Vec<&str> = second.notices.iter().map(|n| n.text.as_str()).collect();
    assert_eq!(
        notices,
        [
            "Another vr-ide --session window keeps the session; this one is not saved",
            "AI sessions not reopened: they run in the other vr-ide --session window",
        ]
    );
    // Agents opened by hand in the second window still work; the file is not touched.
    crate::ai::open_tool(&mut second, "gemini").unwrap();
    crate::session::save_on_quit(&mut second);
    quit_window(second);
    let mut keeper = keeper;
    crate::session::save_on_quit(&mut keeper);
    let body = std::fs::read_to_string(crate::session::file_for_tests(&keeper)).unwrap();
    assert!(
        body.contains("tool = \"claude\"") && !body.contains("gemini"),
        "{body}"
    );
    quit_window(keeper);
}

/// A session that cannot be reopened is left out of the file from then on; the others stay.
#[test]
fn a_refused_session_is_not_written_back() {
    let t = session_dirs();
    let mut app = session_window(&t.state, &t.home, &t.tools);
    crate::ai::open_tool(&mut app, "claude").unwrap();
    crate::ai::open_tool(&mut app, "gemini").unwrap();
    crate::session::save_on_quit(&mut app);
    quit_window(app);
    // gemini is gone from ai_tools at the next launch.
    let app = session_window(&t.state, &t.home, &t.tools[..1]);
    assert_eq!(app.ai.sessions.len(), 1);
    let notices: Vec<&str> = app.notices.iter().map(|n| n.text.as_str()).collect();
    assert_eq!(
        notices,
        [format!(
            "Not reopened: gemini in {}, gemini is not in ai_tools",
            crate::git::home_relative(&t.home)
        )]
    );
    let mut app = app;
    crate::session::save_on_quit(&mut app);
    let body = std::fs::read_to_string(crate::session::file_for_tests(&app)).unwrap();
    assert!(
        body.contains("claude") && !body.contains("gemini"),
        "{body}"
    );
    quit_window(app);
}

/// The AI sessions' quit question answers like the other quit questions: Quit anyway quits,
/// and the session keeps the edit mode the file tab had before the question.
#[test]
fn the_ai_quit_question_keeps_the_edit_mode() {
    let t = session_dirs();
    std::fs::write(t.home.join("notes.txt"), "a\n").unwrap();
    let mut app = session_window(&t.state, &t.home, &t.tools);
    crate::ai::open_tool(&mut app, "gemini").unwrap();
    app.open_path(&t.home.join("notes.txt")).unwrap();
    app.focus = Focus::Editor;
    app.sync_mode();
    app.enter_edit_mode();
    assert!(app.edit_mode());
    let now = Instant::now();
    app.tick(now);
    app.run_command("app.quit", CmdArgs::None);
    let d = app.dialog.take().expect("a running AI session asks");
    assert!(
        format!("{:?}", d.widget).contains("1 AI session still running"),
        "{:?}",
        d.widget
    );
    crate::commands::dialog::answer(&mut app, d, crate::ui::dialog::Outcome::Chosen(0));
    app.sync_mode();
    assert!(app.quit, "Quit anyway quits");
    app.tick(now + Duration::from_secs(3));
    crate::session::save_on_quit(&mut app);
    let body = std::fs::read_to_string(crate::session::file_for_tests(&app)).unwrap();
    assert!(body.contains("edit = true"), "{body}");
    quit_window(app);
}

/// An agent that printed `n` numbered lines and exits without a final newline.
fn lines_agent(dir: &Path, n: usize) -> PathBuf {
    script(
        dir,
        "lines",
        &format!(
            "#!/bin/sh\ni=1\nwhile [ $i -le {n} ]; do echo L$i; i=$((i+1)); done\nprintf LAST\n"
        ),
    )
}

#[test]
fn an_exited_session_scrolls_back() {
    let (d, mut app) = ai_app();
    let agent = lines_agent(d.path(), 200);
    app.settings.ai_tools.push(tool("codex", &agent, true));
    crate::ai::open_tool(&mut app, "codex").unwrap();
    wait_exit(&app, 0);
    wait_for(&app, 0, |s| s.contains("LAST"));
    key(&mut app, KeyCode::PageUp, KeyModifiers::SHIFT);
    assert!(app.ai.sessions[0].tab.scroll > 0, "scrolled back");
    key(&mut app, KeyCode::PageDown, KeyModifiers::SHIFT);
    assert_eq!(app.ai.sessions[0].tab.scroll, 0);
    assert_eq!(app.ai.sessions[0].mode, StartMode::New, "not restarted");
}

/// D15: the exited line has rows of its own below the agent's grid; nothing the agent printed
/// is drawn over.
#[test]
fn the_exited_line_is_drawn_below_the_agent_output() {
    let (d, mut app) = ai_app();
    let agent = lines_agent(d.path(), 60);
    app.settings.ai_tools.push(tool("codex", &agent, true));
    crate::terminal_tests::draw(&mut app, 120, 40);
    crate::ai::open_tool(&mut app, "codex").unwrap();
    wait_exit(&app, 0);
    wait_for(&app, 0, |s| s.contains("LAST"));
    crate::terminal_tests::draw(&mut app, 120, 40);
    let t = crate::terminal_tests::draw(&mut app, 120, 40);
    let rows: Vec<String> = (0..40)
        .map(|y| crate::terminal_tests::row_text(&t, y))
        .collect();
    let last = rows.iter().position(|r| r.contains("LAST"));
    let exited = rows.iter().position(|r| r.contains("[exited 0]"));
    assert!(
        matches!((last, exited), (Some(l), Some(e)) if l < e),
        "{}",
        rows.join("\n")
    );
    assert!(
        rows.iter().any(|r| r.contains("L60")),
        "{}",
        rows.join("\n")
    );
}

#[test]
fn tab_cycle_reaches_the_panel() {
    let (_d, mut app) = ai_app();
    crate::ai::open_tool(&mut app, "gemini").unwrap();
    press(&mut app, KeyCode::F(3));
    app.run_command("focus.cycle", CmdArgs::None);
    assert_eq!(app.focus, Focus::Ai);
}

// ---- D9 notify (AC-10, SEC-14) ----------------------------------------------------------------

/// Reads commands from the terminal: `bell`, `osc <text>`, `big` (10 KiB OSC 9 with an escape)
/// and `flood` (a thousand bells); `ready` once it listens.
const NOTIFY_AGENT: &str = "#!/bin/sh\nprintf 'ready\\n'\nwhile read cmd arg; do\n  case $cmd in\n    bell) printf '\\007';;\n    osc) printf '\\033]9;%s\\007' \"$arg\";;\n    big) printf '\\033]9;\\033[2J'; head -c 10240 /dev/zero | tr '\\0' 'a'; printf '\\007';;\n    flood) i=0; while [ $i -lt 1000 ]; do printf '\\007'; i=$((i+1)); done;;\n  esac\ndone\n";

fn notify_app() -> (tempfile::TempDir, App) {
    let (d, mut app) = ai_app();
    let agent = script(&d.path().join(".bin"), "notify", NOTIFY_AGENT);
    app.settings.ai_tools = vec![tool("claude", &agent, false)];
    app.layout.status.width = 120;
    (d, app)
}

pub(crate) fn say(app: &App, i: usize, line: &str) {
    app.ai.sessions[i]
        .tab
        .write(format!("{line}\n").as_bytes())
        .unwrap();
}

fn wait_marked(app: &mut App, i: usize) {
    let deadline = Instant::now() + WAIT;
    while app.ai.sessions[i].attention.is_none() {
        crate::ai::refresh(app);
        assert!(Instant::now() < deadline, "session {i} was not marked");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_hidden_session_is_marked_noticed_and_f12_shows_it_and_clears() {
    let (_d, mut app) = notify_app();
    crate::ai::open_tool(&mut app, "claude").unwrap();
    crate::ai::open_tool(&mut app, "claude").unwrap();
    wait_for(&app, 0, |s| s.contains("ready"));
    wait_for(&app, 1, |s| s.contains("ready"));
    press(&mut app, KeyCode::F(12));
    assert!(!app.ai.visible);
    say(&app, 0, "osc Allow Bash(cargo test)?");
    wait_marked(&mut app, 0);
    assert_eq!(app.ai.sessions[1].attention, None);
    let notice = app.notices.front().expect("a notice").text.clone();
    assert!(
        notice.starts_with("◆ claude (")
            && notice.contains("): Allow Bash(cargo test)?  f12 shows it"),
        "{notice}"
    );
    assert_eq!(
        app.notices.front().unwrap().expires,
        None,
        "stays until viewed"
    );
    // f12 opens the marked session, not the active one; viewing it clears mark and notice.
    app.ai.active = 1;
    press(&mut app, KeyCode::F(12));
    assert_eq!(app.ai.active, 0);
    crate::ai::attention::clear_viewed(&mut app);
    assert_eq!(app.ai.sessions[0].attention, None);
    assert!(app.notices.is_empty());
}

#[test]
fn the_bell_marks_with_its_own_words_and_a_viewed_tab_is_not_marked() {
    let (_d, mut app) = notify_app();
    crate::ai::open_tool(&mut app, "claude").unwrap();
    wait_for(&app, 0, |s| s.contains("ready"));
    assert!(app.ai.visible, "starting a session shows the panel");
    say(&app, 0, "bell");
    // Shown and active: the program's request is seen, nothing is marked.
    let deadline = Instant::now() + Duration::from_millis(500);
    while Instant::now() < deadline {
        crate::ai::refresh(&mut app);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(app.ai.sessions[0].attention, None);
    assert!(app.notices.is_empty());
    press(&mut app, KeyCode::F(12));
    say(&app, 0, "bell");
    wait_marked(&mut app, 0);
    assert!(app.notices.front().unwrap().text.contains("rang the bell"));
}

/// SEC-14: escape bytes, 10 KiB and a flood become one sanitized, capped notice.
#[test]
fn notification_text_is_sanitized_capped_and_rate_limited() {
    let (_d, mut app) = notify_app();
    crate::ai::open_tool(&mut app, "claude").unwrap();
    wait_for(&app, 0, |s| s.contains("ready"));
    press(&mut app, KeyCode::F(12));
    say(&app, 0, "big");
    wait_marked(&mut app, 0);
    let text = app.notices.front().unwrap().text.clone();
    assert!(!text.chars().any(|c| c.is_control()), "{text:?}");
    assert!(
        text.starts_with("◆ claude"),
        "prefixed with the label: {text}"
    );
    assert!(
        text.chars().count() < 400,
        "capped: {}",
        text.chars().count()
    );
    // A flood after the tab was viewed: still one notice.
    app.ai.active = 0;
    app.ai.visible = true;
    crate::ai::attention::clear_viewed(&mut app);
    app.ai.visible = false;
    app.notices.clear();
    // A second notice inside the gap is withheld; after it one comes.
    std::thread::sleep(Duration::from_millis(1100));
    say(&app, 0, "flood");
    wait_marked(&mut app, 0);
    std::thread::sleep(Duration::from_millis(300));
    crate::ai::refresh(&mut app);
    assert_eq!(app.notices.len(), 1, "one notice for a thousand bells");
}

// ---- workspace switch --------------------------------------------------------------------------

/// The message of the open confirm dialog, if one is open.
fn confirm_message(app: &App) -> Option<String> {
    match app.dialog.as_ref().map(|d| &d.widget) {
        Some(crate::ui::dialog::DialogWidget::Confirm(c)) => Some(c.message.clone()),
        _ => None,
    }
}

/// Waits until process `pid` is gone (ended and reaped).
fn wait_gone(pid: u32) {
    let deadline = Instant::now() + WAIT;
    while Path::new(&format!("/proc/{pid}")).exists() {
        assert!(Instant::now() < deadline, "process {pid} still there");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// `ai_app` with two shells in the terminal panel and one `gemini` session; the pids of all
/// three, and another folder to switch to.
fn busy_app() -> (tempfile::TempDir, tempfile::TempDir, App, Vec<u32>) {
    let (d, mut app) = ai_app();
    app.terminal.shell = Some(PathBuf::from("/bin/sh"));
    crate::commands::terminal::new_terminal(&mut app);
    crate::commands::terminal::new_terminal(&mut app);
    crate::ai::open_tool(&mut app, "gemini").unwrap();
    let mut pids: Vec<u32> = app.terminal.tabs.iter().filter_map(|t| t.pid()).collect();
    pids.extend(app.ai.sessions.iter().filter_map(|s| s.tab.pid()));
    assert_eq!(pids.len(), 3, "every program started");
    let other = tempfile::tempdir().unwrap();
    (d, other, app, pids)
}

fn other_ws(other: &tempfile::TempDir) -> crate::workspace::Workspace {
    crate::workspace::Workspace::single(canonical(other))
}

/// Terminals and AI sessions run in the old folders: the switch asks first, naming how many
/// close, Cancel selected; Continue ends them all (no process left) and switches, and the
/// session store no longer holds them.
#[test]
fn switching_workspace_asks_then_closes_terminals_and_ai_sessions() {
    let (d, other, mut app, pids) = busy_app();
    app.open_workspace(other_ws(&other));
    let msg = confirm_message(&app).expect("the switch asks first");
    assert!(
        msg.contains("Switching workspace closes 2 terminals and 1 AI session."),
        "{msg}"
    );
    assert_eq!(
        app.workspace.paths(),
        [canonical(&d)],
        "nothing switched yet"
    );
    assert_eq!(app.terminal.tabs.len(), 2);
    assert_eq!(app.ai.sessions.len(), 1);
    // Continue is the first button; Cancel is selected.
    press(&mut app, KeyCode::Left);
    press(&mut app, KeyCode::Enter);
    assert!(app.dialog.is_none());
    assert_eq!(app.workspace.paths(), [canonical(&other)], "switched");
    assert!(app.terminal.tabs.is_empty(), "terminals closed");
    assert!(app.ai.sessions.is_empty(), "AI sessions closed");
    assert!(app.ai.picker.is_none(), "no picker on the old folder");
    assert!(!app.terminal.visible && !app.ai.visible);
    for pid in pids {
        wait_gone(pid);
    }
    assert!(crate::ai::restore::capture(&app).sessions.is_empty());
    assert!(crate::session::snapshot(&app).terminal.folders.is_empty());
}

#[test]
fn cancel_keeps_the_workspace_terminals_and_ai_sessions() {
    let (d, other, mut app, pids) = busy_app();
    app.open_workspace(other_ws(&other));
    assert!(app.dialog.is_some());
    press(&mut app, KeyCode::Enter);
    assert!(app.dialog.is_none());
    assert_eq!(app.workspace.paths(), [canonical(&d)]);
    assert_eq!(app.terminal.tabs.len(), 2);
    assert_eq!(app.ai.sessions.len(), 1);
    assert!(app.terminal.tabs.iter().all(|t| t.running()));
    assert!(app.ai.sessions[0].running());
    for pid in pids {
        assert!(Path::new(&format!("/proc/{pid}")).exists(), "{pid} runs");
    }
    // Esc cancels too.
    app.open_workspace(other_ws(&other));
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.workspace.paths(), [canonical(&d)]);
    assert_eq!(app.ai.sessions.len(), 1);
}

/// Nothing open in the panels: the switch does not ask.
#[test]
fn switching_with_no_terminal_or_ai_session_does_not_ask() {
    let (_d, mut app) = ai_app();
    let other = tempfile::tempdir().unwrap();
    app.open_workspace(other_ws(&other));
    assert!(app.dialog.is_none());
    assert_eq!(app.workspace.paths(), [canonical(&other)]);
}

/// An exited session or terminal is still open (its output shows) and is counted.
#[test]
fn exited_sessions_and_terminals_count_as_open() {
    let (d, mut app) = ai_app();
    let quick = script(d.path(), "quick", QUICK_AGENT);
    app.settings.ai_tools.push(tool("codex", &quick, true));
    crate::ai::open_tool(&mut app, "codex").unwrap();
    crate::ai::open_tool(&mut app, "codex").unwrap();
    wait_exit(&app, 0);
    wait_exit(&app, 1);
    app.terminal
        .tabs
        .push(crate::commands::terminal::TermTab::fake(
            Default::default(),
            "sh",
            vr_term::TermStatus::Exited(0),
        ));
    let other = tempfile::tempdir().unwrap();
    app.open_workspace(other_ws(&other));
    let msg = confirm_message(&app).expect("asks");
    assert!(
        msg.contains("Switching workspace closes 1 terminal and 2 AI sessions."),
        "{msg}"
    );
}

/// The open-sessions question comes first and closes nothing; the unsaved-files question
/// follows, and Cancel there still leaves the terminals, sessions and edits as they were.
/// Discard then switches and closes them.
#[test]
fn the_sessions_question_comes_before_the_unsaved_files_one() {
    let (d, other, mut app, _) = busy_app();
    app.recovery_dir = Some(d.path().join(".recovery"));
    std::fs::write(d.path().join("a.txt"), "a\n").unwrap();
    app.open_path(&d.path().join("a.txt")).unwrap();
    app.focus = Focus::Editor;
    crate::app_tests::space_i(&mut app);
    press(&mut app, KeyCode::Char('x'));
    press(&mut app, KeyCode::Esc);
    assert!(app.docs[0].is_modified());
    app.open_workspace(other_ws(&other));
    let msg = confirm_message(&app).expect("asks");
    assert!(msg.contains("closes 2 terminals and 1 AI session"), "{msg}");
    press(&mut app, KeyCode::Left);
    press(&mut app, KeyCode::Enter);
    let msg = confirm_message(&app).expect("then asks about the unsaved file");
    assert!(msg.contains("unsaved file(s): a.txt"), "{msg}");
    assert_eq!(app.terminal.tabs.len(), 2, "nothing closed yet");
    assert_eq!(app.ai.sessions.len(), 1);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.workspace.paths(), [canonical(&d)]);
    assert_eq!(app.terminal.tabs.len(), 2);
    assert_eq!(app.ai.sessions.len(), 1);
    assert!(app.docs[0].is_modified(), "the edit is kept");
    // Again: Continue, then Discard.
    app.open_workspace(other_ws(&other));
    press(&mut app, KeyCode::Left);
    press(&mut app, KeyCode::Enter);
    assert!(confirm_message(&app).is_some_and(|m| m.contains("unsaved")));
    let Some(crate::ui::dialog::DialogWidget::Confirm(c)) =
        app.dialog.as_mut().map(|d| &mut d.widget)
    else {
        unreachable!()
    };
    c.selected = 1;
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.workspace.paths(), [canonical(&other)]);
    assert!(app.terminal.tabs.is_empty() && app.ai.sessions.is_empty());
    assert!(app.docs.iter().all(|d| !d.is_modified()));
}

/// `/ws open` of the folder already open changes nothing and asks nothing.
#[test]
fn reopening_the_open_folder_asks_nothing() {
    let (_d, _other, mut app, _) = busy_app();
    crate::slash_tests::set_input_and_enter(&mut app, "/ws open .");
    assert!(app.dialog.is_none());
    assert_eq!(app.terminal.tabs.len(), 2);
    assert_eq!(app.ai.sessions.len(), 1);
    assert!(
        crate::slash_tests::notices(&app).contains("is already the open workspace"),
        "{}",
        crate::slash_tests::notices(&app)
    );
}

// ---- usage report (AC-3, SEC-22, SEC-25, SEC-39) ----------------------------------------------

const USAGE_ID: &str = "0f8e2c1a-7b3d-4e5f-9a1b-2c3d4e5f6a7b";

/// A claude that does what claude does with `--settings`: runs the statusLine command through
/// `sh` with the status JSON on stdin, then prints its arguments (one write) and waits.
const STATUS_AGENT: &str = r#"#!/bin/sh
[ "$1" = --settings ] || { printf 'NOSETTINGS\nARGV:%s\n' "$(printf '%s|' "$@")"; exec cat; }
cmd=$(sed -n 's/.*"command":"\([^"]*\)".*/\1/p' "$2")
printf '%s' 'STATUS' | sh -c "$cmd"
printf '\nARGV:%s\n' "$(printf '%s|' "$@")"; exec cat
"#;

/// Stands in for `vr-ide --ai-statusline <report>`: the unit-test binary is not vr-ide (the real
/// helper runs in tests/statusline_helper.rs). Keeps the JSON as the report, prints a line.
const STAND_IN_HELPER: &str =
    "#!/bin/sh\n[ \"$1\" = --ai-statusline ] && cat > \"$2\" && printf 'HELPER-LINE'\n";

fn usage_app() -> (tempfile::TempDir, App, PathBuf) {
    let (d, mut app) = ai_app();
    let state = d.path().join(".state");
    std::fs::create_dir(&state).unwrap();
    app.ai.state_dir = Some(state);
    let bin = d.path().join(".bin");
    let helper = script(&bin, "helper", STAND_IN_HELPER);
    app.ai.usage_exe = Some(helper.clone());
    let now = crate::ai::usage::now_secs();
    let status = format!(
        r#"{{"session_id":"{USAGE_ID}","model":{{"display_name":"Opus 5.5"}},"rate_limits":{{"five_hour":{{"used_percentage":63,"resets_at":{}}}}}}}"#,
        now + 3600
    );
    let fake = script(&bin, "claude", &STATUS_AGENT.replace("STATUS", &status));
    app.settings.ai_tools[0] = tool("claude", &fake, true);
    (d, app, helper)
}

/// One pass of the event loop at `now`.
pub(crate) fn pass(app: &mut App, now: Instant) {
    crate::event_loop::background_pass(app, now);
}

fn start(app: &mut App, d: &tempfile::TempDir, i: usize) {
    let t = app.settings.ai_tools[i].clone();
    crate::ai::spawn_session(app, &t, None, StartMode::New, &canonical(d), None).unwrap();
}

#[test]
fn claude_session_reads_usage_report() {
    let (d, mut app, helper) = usage_app();
    start(&mut app, &d, 0);
    let text = wait_for(&app, 0, |s| s.contains("ARGV:"));
    assert!(text.contains("HELPER-LINE"), "{text}");
    let report = app.ai.sessions[0].usage_file.clone().expect("report path");
    let settings = report.with_extension("json");
    // The screen wraps the long line.
    assert!(
        text.replace('\n', "")
            .contains(&format!("ARGV:--settings|{}|--base|", settings.display())),
        "{text}"
    );
    use std::os::unix::fs::MetadataExt;
    assert_eq!(std::fs::metadata(&settings).unwrap().mode() & 0o777, 0o600);
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&settings).unwrap()).unwrap();
    assert_eq!(
        json["statusLine"]["command"],
        format!(
            "'{}' --ai-statusline '{}'",
            helper.display(),
            report.display()
        )
    );

    let t0 = Instant::now();
    app.dirty = false;
    pass(&mut app, t0);
    assert!(app.dirty, "a new report redraws");
    let u = app.ai.sessions[0].usage.clone().expect("usage read");
    assert_eq!(u.five_hour.unwrap().used_percentage, 63.0);
    assert_eq!(u.session_id.as_deref(), Some(USAGE_ID));
    assert_eq!(u.model.as_deref(), Some("Opus 5.5"));

    // Loop passes without a new report do not read it again (SEC-39).
    let reads = app.ai.usage_reads;
    assert_eq!(reads, 1);
    for i in 1..=20 {
        pass(&mut app, t0 + Duration::from_millis(300 * i));
    }
    assert_eq!(app.ai.usage_reads, reads);

    // Closing the session removes both files (SEC-22).
    crate::ai::close(&mut app, 0);
    assert!(!report.exists() && !settings.exists());
}

/// Only claude gets `--settings`, and only with a state folder; an unusable helper path gives
/// one notice per run and a plain start.
#[test]
fn usage_settings_only_for_claude_with_a_usable_path() {
    let (d, mut app, _) = usage_app();
    start(&mut app, &d, 1);
    let text = wait_for(&app, 0, |s| s.contains("ARGV:"));
    assert!(text.contains("ARGV:--base|"), "gemini: {text}");
    assert!(app.ai.sessions[0].usage_file.is_none());

    let odd = d.path().join("vr\nide");
    std::fs::write(&odd, "").unwrap();
    app.ai.usage_exe = Some(odd);
    start(&mut app, &d, 0);
    start(&mut app, &d, 0);
    for i in [1, 2] {
        let text = wait_for(&app, i, |s| s.contains("ARGV:"));
        assert!(text.contains("NOSETTINGS"), "{text}");
        assert!(app.ai.sessions[i].usage_file.is_none());
    }
    let said: Vec<&str> = app
        .notices
        .iter()
        .map(|n| n.text.as_str())
        .filter(|t| t.contains("usage"))
        .collect();
    assert_eq!(said, ["claude usage not available (vr-ide path unusable)"]);
}

/// Finding R1-1: the loop reads a new report whether or not the panel shows or the program
/// prints, including one written just after a read and followed by silence.
#[test]
fn usage_is_read_without_output_or_a_shown_panel() {
    let (d, mut app, _) = usage_app();
    start(&mut app, &d, 0);
    wait_for(&app, 0, |s| s.contains("ARGV:"));
    assert!(!app.ai.visible);
    let t0 = Instant::now();
    pass(&mut app, t0);
    let report = app.ai.sessions[0].usage_file.clone().unwrap();
    let mut u = app.ai.sessions[0].usage.clone().expect("first report");
    // The agent is now silent: a report it wrote 200 ms after that read (no PTY output).
    u.five_hour.as_mut().unwrap().used_percentage = 80.0;
    crate::ai::usage::write_report(&report, &u).unwrap();
    app.dirty = false;
    pass(&mut app, t0 + Duration::from_millis(500));
    assert_eq!(
        app.ai.sessions[0]
            .usage
            .as_ref()
            .unwrap()
            .five_hour
            .unwrap()
            .used_percentage,
        63.0,
        "at most one read a second"
    );
    pass(&mut app, t0 + Duration::from_millis(1100));
    assert_eq!(
        app.ai.sessions[0]
            .usage
            .as_ref()
            .unwrap()
            .five_hour
            .unwrap()
            .used_percentage,
        80.0
    );
    assert!(app.dirty);
}

/// Reports its run number: a new session id and 60 + n percent each start; then exits.
const RUN_AGENT: &str = r#"#!/bin/sh
cmd=$(sed -n 's/.*"command":"\([^"]*\)".*/\1/p' "$2")
n=$(cat "$0.count" 2>/dev/null || echo 0); n=$((n+1)); echo $n > "$0.count"
printf '{"session_id":"0f8e2c1a-7b3d-4e5f-9a1b-00000000000%s","rate_limits":{"five_hour":{"used_percentage":%s,"resets_at":RESET}}}' $n $((60+n)) | sh -c "$cmd"
echo; printf ARGV:; printf '%s|' "$@"; echo DONE
"#;

const RUN_ID: &str = "0f8e2c1a-7b3d-4e5f-9a1b-00000000000";

/// Findings R1-2 and R1-5 (BASE-23: the restart path): Enter on an exited claude replaces its
/// settings and report, reads the new run's report; a New-mode start forgets the old id at once,
/// a Continue start keeps it until the new run reports.
#[test]
fn restart_replaces_usage_files_and_forgets_a_new_runs_old_id() {
    for continues in [true, false] {
        let (d, mut app, _) = usage_app();
        let reset = crate::ai::usage::now_secs() + 3600;
        let bin = d.path().join(".bin");
        let agent = script(
            &bin,
            "runs",
            &RUN_AGENT.replace("RESET", &reset.to_string()),
        );
        app.settings.ai_tools[0] = tool("claude", &agent, continues);
        start(&mut app, &d, 0);
        wait_exit(&app, 0);
        wait_for(&app, 0, |s| s.contains("DONE"));
        let t0 = Instant::now();
        pass(&mut app, t0);
        let usage = |app: &App| app.ai.sessions[0].usage.clone().unwrap();
        assert_eq!(usage(&app).session_id, Some(format!("{RUN_ID}1")));
        let old_report = app.ai.sessions[0].usage_file.clone().unwrap();
        let old_settings = old_report.with_extension("json");

        app.ai.visible = true;
        app.ai.active = 0;
        app.focus = Focus::Ai;
        press(&mut app, KeyCode::Enter);
        assert!(
            !old_report.exists() && !old_settings.exists(),
            "old files gone"
        );
        let report = app.ai.sessions[0].usage_file.clone().unwrap();
        assert_ne!(report, old_report);
        let held = usage(&app).session_id;
        if continues {
            assert_eq!(held, Some(format!("{RUN_ID}1")), "Continue keeps the id");
        } else {
            assert_eq!(held, None, "a New run is another conversation");
            assert_eq!(usage(&app).five_hour.unwrap().used_percentage, 61.0);
        }
        wait_exit(&app, 0);
        wait_for(&app, 0, |s| s.contains("DONE"));
        pass(&mut app, t0 + Duration::from_millis(300));
        assert_eq!(usage(&app).session_id, Some(format!("{RUN_ID}2")));
        assert_eq!(usage(&app).five_hour.unwrap().used_percentage, 62.0);
        crate::ai::close(&mut app, 0);
        assert!(!report.exists() && !report.with_extension("json").exists());
    }
}

// ---- limit screens and account state (AC-5, SEC-26, SEC-27) ----------------------------------

/// Prints claude's limit line or a reply on request, then waits like a stopped claude.
const LIMIT_AGENT: &str = r#"#!/bin/sh
printf 'ready\n'
while read cmd; do
  case $cmd in
    limit) printf "  \342\216\277  You've hit your 5-hour limit \302\267 resets 3pm\n";;
    reply) printf '\342\227\217 Done.\n';;
    bare) printf "  \342\216\277  Usage limit reached\n";;
    login) printf ' Select login method:\n';;
  esac
done
"#;

fn limit_app() -> (tempfile::TempDir, App, PathBuf) {
    let (d, mut app) = ai_app();
    let state = d.path().join(".state");
    std::fs::create_dir(&state).unwrap();
    app.ai.state_dir = Some(state.clone());
    let agent = script(&d.path().join(".bin"), "limits", LIMIT_AGENT);
    app.settings.ai_tools = vec![tool("claude", &agent, true)];
    start(&mut app, &d, 0);
    wait_for(&app, 0, |s| s.contains("ready"));
    (d, app, state)
}

fn limit_notices(app: &App) -> Vec<String> {
    app.notices
        .iter()
        .map(|n| n.text.clone())
        .filter(|t| t.contains("usage limit"))
        .collect()
}

/// The agent prints `cmd`'s line; the loop sees the output at `at`.
fn agent_says(app: &mut App, cmd: &str, want: &str, at: Instant) {
    let before = app.ai.sessions[0].tab.screen_text().matches(want).count();
    say(app, 0, cmd);
    wait_for(app, 0, |s| s.matches(want).count() > before);
    pass(app, at);
}

#[test]
fn ai_limit_counts_after_a_quiet_second_with_one_notice_per_event() {
    let (_d, mut app, state) = limit_app();
    let t0 = Instant::now();
    let s = |ms: u64| t0 + Duration::from_millis(ms);
    agent_says(&mut app, "limit", "hit your", s(0));
    pass(&mut app, s(900));
    assert!(limit_notices(&app).is_empty(), "not quiet for a second yet");
    assert!(!app.ai.sessions[0].limit_seen);
    pass(&mut app, s(1000));
    assert_eq!(
        limit_notices(&app),
        ["claude hit its usage limit (resets 15:00)"]
    );
    assert!(app.ai.sessions[0].limit_seen);
    assert!(
        app.ai.sessions[0].running(),
        "a limit never ends the process"
    );
    let until = crate::ai::limits::load_state(&state)["claude/default"].limited_until;
    let now = crate::ai::usage::now_secs();
    assert!(
        until.is_some_and(|t| t > now && t <= now + 86_400),
        "{until:?}"
    );
    // The same event: no second notice however long it stays.
    pass(&mut app, s(70_000));
    assert_eq!(limit_notices(&app).len(), 1);

    // The user types, the tool answers and the line is gone: the event is over. A new limit
    // within the minute is an event, but without a second notice.
    app.ai.sessions[0].limit_watch.typed = true;
    agent_says(&mut app, "reply", "Done.", s(5000));
    pass(&mut app, s(6000));
    assert!(!app.ai.sessions[0].limit_seen);
    agent_says(&mut app, "limit", "hit your", s(10_000));
    pass(&mut app, s(11_000));
    assert!(app.ai.sessions[0].limit_seen);
    assert_eq!(limit_notices(&app).len(), 1, "at most one notice a minute");
    // Without typing the event does not end, even when the line goes.
    agent_says(&mut app, "reply", "Done.", s(20_000));
    pass(&mut app, s(21_000));
    assert!(app.ai.sessions[0].limit_seen);
    app.ai.sessions[0].limit_watch.typed = true;
    pass(&mut app, s(21_500));
    agent_says(&mut app, "reply", "Done.", s(22_000));
    pass(&mut app, s(23_000));
    assert!(!app.ai.sessions[0].limit_seen);
    agent_says(&mut app, "limit", "hit your", s(70_000));
    pass(&mut app, s(71_000));
    assert_eq!(limit_notices(&app).len(), 2);
}

/// FR-1 M3: a limit with no reset on screen and none in a report is recorded for an hour, marked
/// as a guess (not sticky), so a switch does not hop back onto it; a real reset replaces it.
#[test]
fn ai_limit_without_a_reset_is_recorded_for_an_hour() {
    let (_d, mut app, state) = limit_app();
    let t0 = Instant::now();
    agent_says(&mut app, "bare", "Usage limit reached", t0);
    pass(&mut app, t0 + Duration::from_millis(1000));
    assert_eq!(limit_notices(&app), ["claude hit its usage limit"]);
    let got = crate::ai::limits::load_state(&state)["claude/default"];
    let now = crate::ai::usage::now_secs();
    let until = got.limited_until.expect("a provisional limit");
    assert!(
        (now + 3600 - 5..=now + 3600).contains(&until),
        "{until} vs {now}"
    );
    assert!(got.guessed && !got.sticky, "{got:?}");
    // The next event shows its reset (3pm): it replaces the guess, even when it is earlier.
    app.ai.sessions[0].limit_watch.typed = true;
    agent_says(&mut app, "reply", "Done.", t0 + Duration::from_secs(5));
    pass(&mut app, t0 + Duration::from_secs(6));
    assert!(!app.ai.sessions[0].limit_seen);
    agent_says(&mut app, "limit", "hit your", t0 + Duration::from_secs(10));
    pass(&mut app, t0 + Duration::from_secs(11));
    let got = crate::ai::limits::load_state(&state)["claude/default"];
    assert!(!got.guessed, "{got:?}");
    assert_ne!(got.limited_until, Some(until));
}

/// SEC-26: output after the limit line within the second means the tool went on.
#[test]
fn ai_limit_line_followed_by_output_is_no_event() {
    let (_d, mut app, state) = limit_app();
    let t0 = Instant::now();
    agent_says(&mut app, "limit", "hit your", t0);
    agent_says(&mut app, "reply", "Done.", t0 + Duration::from_millis(500));
    pass(&mut app, t0 + Duration::from_millis(1200));
    pass(&mut app, t0 + Duration::from_millis(3000));
    assert!(limit_notices(&app).is_empty());
    assert!(!app.ai.sessions[0].limit_seen);
    assert!(crate::ai::limits::load_state(&state).is_empty());
}

/// A typed key marks the session as answered by the user (the event ends once the line goes).
#[test]
fn ai_limit_typing_marks_the_session() {
    let (_d, mut app, _) = limit_app();
    app.ai.visible = true;
    app.ai.active = 0;
    app.focus = Focus::Ai;
    assert!(!app.ai.sessions[0].limit_watch.typed);
    press(&mut app, KeyCode::Char('x'));
    assert!(app.ai.sessions[0].limit_watch.typed);
}

/// SEC-27: a validated report with a session id marks the account working; one under 100 %
/// ends a 5-hour/weekly limit but not a sticky one (a model's own limit, spend, credits: the
/// report does not show those); 100 % alone neither ends the process nor makes an event (AC-5).
#[test]
fn ai_limit_state_follows_usage_reports() {
    for (percent, sticky) in [(63, false), (63, true), (100, false)] {
        let (d, mut app, _) = usage_app();
        let state = app.ai.state_dir.clone().unwrap();
        let now = crate::ai::usage::now_secs();
        let mut seeded = std::collections::BTreeMap::new();
        seeded.insert(
            "claude/default".to_string(),
            crate::ai::limits::AccountState {
                works: false,
                limited_until: Some(now + 3600),
                sticky,
                guessed: false,
            },
        );
        crate::ai::limits::save_state(&state, &seeded).unwrap();
        if percent == 100 {
            let bin = d.path().join(".bin");
            let text = std::fs::read_to_string(bin.join("claude")).unwrap();
            std::fs::write(
                bin.join("claude"),
                text.replace("\"used_percentage\":63", "\"used_percentage\":100"),
            )
            .unwrap();
        }
        start(&mut app, &d, 0);
        wait_for(&app, 0, |s| s.contains("ARGV:"));
        let t0 = Instant::now();
        pass(&mut app, t0);
        pass(&mut app, t0 + Duration::from_secs(2));
        let got = crate::ai::limits::load_state(&state)["claude/default"];
        assert!(got.works, "{percent}%");
        if percent == 100 {
            assert_eq!(got.limited_until, Some(now + 3600));
            assert!(app.ai.sessions[0].running());
            assert!(!app.ai.sessions[0].limit_seen);
            assert!(limit_notices(&app).is_empty());
        } else if sticky {
            assert_eq!(got.limited_until, Some(now + 3600), "sticky kept");
            assert!(got.sticky);
        } else {
            assert_eq!(got.limited_until, None);
        }
    }
}

/// QC F1: a program with no input box echoes what the user typed at column 0; that text is
/// never the tool's limit message.
#[test]
fn ai_limit_typed_text_echoed_is_no_event() {
    let (_d, mut app, state) = limit_app();
    app.ai.visible = true;
    app.ai.active = 0;
    app.focus = Focus::Ai;
    let line = "You've hit your limit · resets 3pm";
    crate::ai::paste(&mut app, line);
    press(&mut app, KeyCode::Enter);
    wait_for(&app, 0, |s| s.contains("resets 3pm"));
    let t0 = Instant::now();
    pass(&mut app, t0);
    pass(&mut app, t0 + Duration::from_secs(2));
    assert!(limit_notices(&app).is_empty());
    assert!(!app.ai.sessions[0].limit_seen);
    assert!(crate::ai::limits::load_state(&state).is_empty());
    // The tool's own line still counts.
    agent_says(
        &mut app,
        "limit",
        "hit your 5-hour",
        t0 + Duration::from_secs(3),
    );
    pass(&mut app, t0 + Duration::from_secs(5));
    assert!(app.ai.sessions[0].limit_seen);
}

/// A login screen on a quiet session marks its account as not working (once per run).
#[test]
fn ai_limit_login_screen_marks_the_account_not_working() {
    let (_d, mut app, state) = limit_app();
    let mut seeded = std::collections::BTreeMap::new();
    seeded.insert(
        "claude/default".to_string(),
        crate::ai::limits::AccountState {
            works: true,
            limited_until: None,
            sticky: false,
            guessed: false,
        },
    );
    crate::ai::limits::save_state(&state, &seeded).unwrap();
    let t0 = Instant::now();
    agent_says(&mut app, "login", "login method", t0);
    pass(&mut app, t0 + Duration::from_millis(500));
    assert!(crate::ai::limits::load_state(&state)["claude/default"].works);
    pass(&mut app, t0 + Duration::from_millis(1000));
    assert!(!crate::ai::limits::load_state(&state).contains_key("claude/default"));
    assert!(limit_notices(&app).is_empty());
}

/// A run that already sent a validated report with an id works: a login line on its screen
/// (printed by the agent, say) does not mark it not working.
#[test]
fn ai_limit_login_line_after_a_report_keeps_the_account_working() {
    let (_d, mut app, state) = limit_app();
    crate::ai::limits::update_state(&state, |m| {
        m.entry("claude/default".into()).or_default().works = true;
    })
    .unwrap();
    app.ai.sessions[0].limit_watch.works_marked = true;
    let t0 = Instant::now();
    agent_says(&mut app, "login", "login method", t0);
    pass(&mut app, t0 + Duration::from_secs(2));
    assert!(crate::ai::limits::load_state(&state)["claude/default"].works);
}

// ---- usage display (AC-4) ---------------------------------------------------------------------

fn usage_notices(app: &App) -> Vec<String> {
    app.notices
        .iter()
        .map(|n| n.text.clone())
        .filter(|t| t.contains("% of its"))
        .collect()
}

fn windows(five: (f64, i64), weekly: (f64, i64)) -> crate::ai::usage::Usage {
    let w = |(p, r): (f64, i64)| crate::ai::usage::Window {
        used_percentage: p,
        resets_at: Some(r),
    };
    crate::ai::usage::Usage {
        session_id: Some(USAGE_ID.into()),
        five_hour: Some(w(five)),
        seven_day: Some(w(weekly)),
        model: None,
    }
}

/// One warning when a window reaches 90 %; later reports of the same window add none; a new
/// window (another reset time) or the other window warns again.
#[test]
fn notice_once_at_90_percent() {
    let (d, mut app, _) = usage_app();
    start(&mut app, &d, 0);
    wait_for(&app, 0, |s| s.contains("ARGV:"));
    let report = app.ai.sessions[0].usage_file.clone().unwrap();
    let now = crate::ai::usage::now_secs();
    let (reset, week) = (now + 3600, now + 3 * 86_400);
    let clock = |t: i64| crate::ai::usage::reset_clock(t, now, crate::ai::limits::Zone::local());
    let t0 = Instant::now();
    let at = |s: u64| t0 + Duration::from_secs(s);
    pass(&mut app, at(0));
    assert_eq!(
        app.ai.sessions[0]
            .usage
            .as_ref()
            .unwrap()
            .five_hour
            .unwrap()
            .used_percentage,
        63.0
    );
    assert!(usage_notices(&app).is_empty(), "63 %: no notice");
    let report_at = |app: &mut App, u: crate::ai::usage::Usage, s: u64| {
        crate::ai::usage::write_report(&report, &u).unwrap();
        pass(app, at(s));
    };
    report_at(&mut app, windows((91.0, reset), (20.0, week)), 2);
    let first = format!(
        "claude used 91% of its 5-hour limit (resets {})",
        clock(reset)
    );
    assert_eq!(usage_notices(&app), std::slice::from_ref(&first));
    report_at(&mut app, windows((95.0, reset), (20.0, week)), 4);
    report_at(&mut app, windows((89.0, reset), (20.0, week)), 6);
    report_at(&mut app, windows((92.0, reset), (20.0, week)), 8);
    assert_eq!(
        usage_notices(&app),
        std::slice::from_ref(&first),
        "same window: once"
    );
    report_at(
        &mut app,
        windows((90.0, reset + 5 * 3600), (93.0, week)),
        10,
    );
    assert_eq!(
        usage_notices(&app),
        [
            first,
            format!(
                "claude used 90% of its 5-hour limit (resets {})",
                clock(reset + 5 * 3600)
            ),
            format!(
                "claude used 93% of its weekly limit (resets {})",
                clock(week)
            ),
        ]
    );
}

#[test]
fn ai_usage_command_lists_windows() {
    let (d, mut app, _) = usage_app();
    start(&mut app, &d, 0);
    let now = crate::ai::usage::now_secs();
    let (reset, week) = (now + 3600, now + 3 * 86_400);
    app.ai.sessions[0].account = Some("work".into());
    app.ai.sessions[0].usage = Some(windows((63.0, reset), (41.0, week)));
    crate::slash_tests::slash(&mut app, "/ai usage");
    crate::slash_tests::key(&mut app, KeyCode::Enter);
    assert_eq!(crate::slash_tests::error(&app), None);
    let hhmm = |t: i64| {
        format!(
            "{:02}:{:02}",
            t.rem_euclid(86_400) / 3600,
            t.rem_euclid(3600) / 60
        )
    };
    let said = crate::slash_tests::notices(&app);
    let want = format!(
        "claude@work: 5-hour 63% (resets {}) · weekly 41% (resets ",
        hhmm(reset)
    );
    assert!(said.contains(&want), "{said}");
    assert!(said.contains(&format!(" {})", hhmm(week))), "{said}");
    assert_eq!(app.ai.sessions.len(), 1, "no session started");
}

#[test]
fn ai_usage_without_data() {
    let (d, mut app) = ai_app();
    crate::slash_tests::slash(&mut app, "/ai usage");
    crate::slash_tests::key(&mut app, KeyCode::Enter);
    assert_eq!(
        crate::slash_tests::error(&app).as_deref(),
        Some("/ai usage: no AI session is active")
    );
    crate::slash_tests::key(&mut app, KeyCode::Esc);
    start(&mut app, &d, 0);
    crate::slash_tests::slash(&mut app, "/ai usage");
    crate::slash_tests::key(&mut app, KeyCode::Enter);
    assert_eq!(
        crate::slash_tests::error(&app).as_deref(),
        Some("/ai usage: no usage reported yet for claude")
    );
    // `/ai usage x` is not a tool name either.
    crate::slash_tests::key(&mut app, KeyCode::Esc);
    crate::slash_tests::slash(&mut app, "/ai usage now");
    crate::slash_tests::key(&mut app, KeyCode::Enter);
    assert_eq!(
        crate::slash_tests::error(&app).as_deref(),
        Some("/ai usage takes no argument")
    );
    assert_eq!(app.ai.sessions.len(), 1);
    // FR-1 M4: a tool without a usage reporter says so rather than "not yet".
    crate::slash_tests::key(&mut app, KeyCode::Esc);
    crate::ai::open_tool(&mut app, "gemini").unwrap();
    crate::slash_tests::slash(&mut app, "/ai usage");
    crate::slash_tests::key(&mut app, KeyCode::Enter);
    assert_eq!(
        crate::slash_tests::error(&app).as_deref(),
        Some("/ai usage: gemini does not report usage")
    );
}

// ---- login window (AC-9, SEC-33, SEC-38) ------------------------------------------------------

/// Prints the `BROWSER` it was started with, as claude would read it.
const BROWSER_AGENT: &str = "#!/bin/sh\nprintf 'BROWSER=[%s]\\n' \"${BROWSER-unset}\"\nexec cat\n";

fn login_app() -> (tempfile::TempDir, App, PathBuf) {
    let (d, mut app, helper) = usage_app();
    let fake = script(&d.path().join(".bin"), "browser-agent", BROWSER_AGENT);
    app.settings.ai_tools[0] = tool("claude", &fake, true);
    app.settings.ai_tools[1] = tool("gemini", &fake, false);
    app.ai.login_display = Some(true);
    (d, app, helper)
}

/// What session `i` got as `BROWSER` (the screen wraps a long path).
fn browser_of(app: &App, i: usize) -> String {
    let text = wait_for(app, i, |s| s.contains("BROWSER=[") && s.contains(']'));
    let flat = text.replace('\n', "");
    let rest = flat.split_once("BROWSER=[").unwrap().1;
    rest.split_once(']').unwrap().0.to_string()
}

fn login_notices(app: &App) -> Vec<&str> {
    app.notices
        .iter()
        .map(|n| n.text.as_str())
        .filter(|t| t.contains("login window"))
        .collect()
}

/// SEC-33 (D63): a claude session gets `BROWSER` = its account's launcher, a link to vr-ide by
/// absolute path; no other tool does, and vr-ide's own environment is never changed.
#[test]
fn claude_gets_browser_env() {
    use crate::ai::login;
    let (d, mut app, helper) = login_app();
    let state = app.ai.state_dir.clone().unwrap();
    let inherited = std::env::var("BROWSER").unwrap_or_else(|_| "unset".into());
    crate::ai::accounts::create(&state, "claude", "work").unwrap();
    let claude = app.settings.ai_tools[0].clone();
    let root = canonical(&d);
    crate::ai::spawn_session(
        &mut app,
        &claude,
        Some("work".into()),
        StartMode::New,
        &root,
        None,
    )
    .unwrap();
    start(&mut app, &d, 0);
    start(&mut app, &d, 1);
    let (work, default, gemini) = (
        browser_of(&app, 0),
        browser_of(&app, 1),
        browser_of(&app, 2),
    );
    assert_eq!(gemini, inherited, "only claude gets the launcher");
    assert_eq!(
        std::env::var("BROWSER").unwrap_or_else(|_| "unset".into()),
        inherited,
        "never set in vr-ide's own environment"
    );
    if !login::ENABLED {
        assert_eq!((work, default), (inherited.clone(), inherited));
        return;
    }
    let bin = state.join("ai-login/bin");
    assert_eq!(PathBuf::from(&work), bin.join("vr-ide-login.claude.work"));
    assert_eq!(
        PathBuf::from(&default),
        bin.join("vr-ide-login.claude.default")
    );
    assert!(Path::new(&work).is_absolute());
    for link in [&work, &default] {
        assert_eq!(std::fs::read_link(link).unwrap(), helper, "{link}");
    }
    use std::os::unix::fs::MetadataExt;
    assert_eq!(std::fs::metadata(&bin).unwrap().mode() & 0o777, 0o700);
    assert!(login_notices(&app).is_empty(), "{:?}", login_notices(&app));

    // Without a display claude keeps its own way of showing the URL: no launcher, no notice.
    app.ai.login_display = Some(false);
    start(&mut app, &d, 0);
    assert_eq!(browser_of(&app, 3), inherited);
    assert!(login_notices(&app).is_empty());
}

/// SEC-33: a vr-ide path that programs reading `BROWSER` could split (`a b`, `a:b`) is never
/// put there; one notice per run says why.
#[test]
fn an_unusable_vr_ide_path_gives_no_browser_and_one_notice() {
    if !crate::ai::login::ENABLED {
        return;
    }
    let (d, mut app, _) = login_app();
    let inherited = std::env::var("BROWSER").unwrap_or_else(|_| "unset".into());
    for (i, odd) in ["a b", "a:b"].into_iter().enumerate() {
        let dir = d.path().join(odd);
        std::fs::create_dir(&dir).unwrap();
        app.ai.usage_exe = Some(script(&dir, "vr-ide", STAND_IN_HELPER));
        start(&mut app, &d, 0);
        assert_eq!(browser_of(&app, i), inherited, "{odd}");
    }
    assert_eq!(
        login_notices(&app),
        ["claude login window not available (vr-ide path unusable)"]
    );
    assert!(!app.ai.state_dir.clone().unwrap().join("ai-login").exists());
}
