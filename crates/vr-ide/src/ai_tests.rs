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

const WAIT: Duration = Duration::from_secs(10);

/// Prints its arguments and folder, then echoes what it is sent (an agent waiting for input).
const ECHO_AGENT: &str = "#!/bin/sh\nprintf ARGV:; printf '%s|' \"$@\"; echo; printf 'CWD:%s\\n' \"$PWD\"; \
                          printf 'VRIDE:%s\\n' \"$(env | grep -c '^VR_IDE_')\"; exec cat\n";
/// Prints its arguments and exits.
const QUICK_AGENT: &str = "#!/bin/sh\nprintf RAN:; printf '%s|' \"$@\"; echo\n";

fn script(dir: &Path, name: &str, text: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn tool(name: &str, command: &Path, resumes: bool) -> AiTool {
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
fn ai_app() -> (tempfile::TempDir, App) {
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

fn press(app: &mut App, code: KeyCode) {
    key(app, code, KeyModifiers::NONE);
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

fn wait_exit(app: &App, i: usize) {
    let deadline = Instant::now() + WAIT;
    while app.ai.sessions[i].running() {
        assert!(Instant::now() < deadline, "agent did not exit");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn canonical(dir: &tempfile::TempDir) -> PathBuf {
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

/// A focused session keeps only the `[ai]` keys: ctrl+w, ctrl+n and f5 reach the agent.
#[test]
fn a_focused_session_gives_other_keys_to_the_agent() {
    let (_d, mut app) = ai_app();
    crate::ai::open_tool(&mut app, "gemini").unwrap();
    wait_for(&app, 0, |s| s.contains("CWD:"));
    app.ran.clear();
    for (code, mods) in [
        (KeyCode::Char('w'), KeyModifiers::CONTROL),
        (KeyCode::Char('n'), KeyModifiers::CONTROL),
        (KeyCode::F(5), KeyModifiers::NONE),
        (KeyCode::F(6), KeyModifiers::NONE),
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
    app.settings.ai_tools.push(tool("codex", &quick, true));
    crate::ai::open_tool(&mut app, "codex").unwrap();
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
