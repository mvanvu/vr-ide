//! T20: slash commands in the F1 command window (01_spec_T20.md, AC-20a..d), driven through
//! the `App` API; real terminals run `/bin/sh`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;

use crate::app::{App, Focus};
use crate::app_tests::{app_with_file, test_app};
use crate::commands::slash::{self, SLASH_COMMANDS, parse};

pub(crate) fn key(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

pub(crate) fn typ(app: &mut App, s: &str) {
    for c in s.chars() {
        key(app, KeyCode::Char(c));
    }
}

/// F1, then `text` typed into the window.
pub(crate) fn slash(app: &mut App, text: &str) {
    key(app, KeyCode::F(1));
    assert_eq!(app.focus, Focus::Palette, "f1 opens the command window");
    typ(app, text);
}

pub(crate) fn input(app: &App) -> String {
    app.palette.as_ref().unwrap().input.text().to_string()
}

pub(crate) fn error(app: &App) -> Option<String> {
    app.palette.as_ref().and_then(|p| p.error.clone())
}

pub(crate) fn titles(app: &App) -> Vec<String> {
    app.palette
        .as_ref()
        .unwrap()
        .items
        .iter()
        .map(|i| i.title.clone())
        .collect()
}

pub(crate) fn notices(app: &App) -> String {
    app.notices
        .iter()
        .map(|n| n.text.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

// ---- registry and parsing -------------------------------------------------------------------

#[test]
fn registry_has_the_phase_1_commands_once() {
    let want = [
        "save-as",
        "reload",
        "term right",
        "term bottom",
        "run",
        "ai",
        "ws add",
        "ws remove",
        "ws save",
        "ws open",
        "ws recent",
        "theme",
        "hidden",
        "ignored",
        "help",
        "q",
        "q!",
        // Phase 2 (T20b, slash2_tests).
        "rename",
        "copy-path",
        "copy-relpath",
        "recent",
        "sort",
        "unique",
        "upper",
        "lower",
        "trim",
        "indent",
        "eol",
        "lang",
        "set",
        "config",
        "keymap",
        "keys",
        "ssh",
    ];
    let names: Vec<&str> = SLASH_COMMANDS.iter().map(|c| c.name).collect();
    for w in want {
        assert_eq!(names.iter().filter(|n| **n == w).count(), 1, "{w}");
    }
    assert_eq!(names.len(), want.len(), "{names:?}");
    assert!(SLASH_COMMANDS.iter().all(|c| !c.description.is_empty()));
}

#[test]
fn parse_finds_the_command_and_its_argument() {
    let (c, a) = parse("/run  echo hi ").unwrap();
    assert_eq!((c.name, a.as_str()), ("run", "echo hi"));
    let (c, a) = parse("/ws add ../x").unwrap();
    assert_eq!((c.name, a.as_str()), ("ws add", "../x"));
    let (c, a) = parse("/q!").unwrap();
    assert_eq!((c.name, a.as_str()), ("q!", ""));
    let (c, _) = parse("/q").unwrap();
    assert_eq!(c.name, "q");
    let (c, a) = parse("/theme dracula").unwrap();
    assert_eq!((c.name, a.as_str()), ("theme", "dracula"));
}

#[test]
fn parse_reports_bad_commands_and_arguments() {
    assert_eq!(
        parse("/nope").unwrap_err(),
        "unknown command /nope (/help lists them)"
    );
    assert_eq!(
        parse("/term").unwrap_err(),
        "unknown command /term (/help lists them)"
    );
    assert_eq!(
        parse("/reload now").unwrap_err(),
        "/reload takes no argument"
    );
    assert_eq!(parse("/run").unwrap_err(), "/run needs <command>");
    assert_eq!(parse("/ws add   ").unwrap_err(), "/ws add needs <folder>");
    assert_eq!(
        parse("/theme pink").unwrap_err(),
        "/theme: pink is not one of synthwave, dracula, dark, light"
    );
    assert_eq!(parse("/").unwrap_err(), "type a command (/help lists them)");
}

// ---- the window ---------------------------------------------------------------------------

#[test]
fn slash_lists_matching_commands_with_arguments_and_descriptions() {
    let (mut app, _) = test_app(Path::new("/p"));
    slash(&mut app, "/");
    let all = titles(&app);
    assert_eq!(all.len(), SLASH_COMMANDS.len(), "{all:?}");
    assert_eq!(all[0], "/save-as [path]");
    let p = app.palette.as_ref().unwrap();
    assert_eq!(p.items[0].detail, SLASH_COMMANDS[0].description);
    assert!(p.items.iter().all(|i| i.keys.is_empty()));
    typ(&mut app, "te");
    assert_eq!(titles(&app), ["/term right", "/term bottom",]);
    typ(&mut app, "rm b");
    assert_eq!(titles(&app), ["/term bottom"]);
}

#[test]
fn without_a_slash_the_palette_lists_commands_as_before() {
    let (mut app, _) = test_app(Path::new("/p"));
    slash(&mut app, "save");
    let p = app.palette.as_ref().unwrap();
    assert_eq!(p.items[0].name, "file.save");
    assert!(p.items.iter().all(|i| !i.title.starts_with('/')));
}

#[test]
fn unknown_command_shows_an_error_and_keeps_the_text() {
    let (mut app, _) = test_app(Path::new("/p"));
    slash(&mut app, "/nope");
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.focus, Focus::Palette, "the window stays open");
    assert_eq!(input(&app), "/nope");
    assert_eq!(
        error(&app).as_deref(),
        Some("unknown command /nope (/help lists them)")
    );
    // Editing clears the error.
    key(&mut app, KeyCode::Backspace);
    assert_eq!(error(&app), None);
    assert_eq!(input(&app), "/nop");
}

#[test]
fn bad_argument_shows_an_error_and_keeps_the_text() {
    let (mut app, _) = test_app(Path::new("/p"));
    slash(&mut app, "/theme pink");
    key(&mut app, KeyCode::Enter);
    assert_eq!(input(&app), "/theme pink");
    assert_eq!(
        error(&app).as_deref(),
        Some("/theme: pink is not one of synthwave, dracula, dark, light")
    );
    assert_eq!(app.settings.theme, crate::theme::DEFAULT_THEME);
}

#[test]
fn esc_closes_the_window_and_returns_to_read_mode() {
    let (_d, _p, mut app, _) = app_with_file("a\n");
    assert!(app.edit_mode());
    slash(&mut app, "/rel");
    key(&mut app, KeyCode::Esc);
    assert!(app.palette.is_none());
    assert_eq!(app.focus, Focus::Editor);
    assert!(!app.edit_mode(), "Esc returns in read (normal) mode");
}

#[test]
fn space_space_opens_the_window_for_slash_commands() {
    let (_d, _p, mut app, _) = app_with_file("a\n");
    key(&mut app, KeyCode::Esc);
    assert!(!app.edit_mode());
    typ(&mut app, "  ");
    assert_eq!(app.focus, Focus::Palette);
    typ(&mut app, "/theme dracula");
    key(&mut app, KeyCode::Enter);
    assert!(app.palette.is_none());
    assert_eq!(app.settings.theme, "dracula");
}

#[test]
fn tab_completes_the_command_name() {
    let (mut app, _) = test_app(Path::new("/p"));
    slash(&mut app, "/ws a");
    key(&mut app, KeyCode::Tab);
    assert_eq!(
        input(&app),
        "/ws add ",
        "a command with an argument gets a space"
    );
    let (mut app, _) = test_app(Path::new("/p"));
    slash(&mut app, "/rel");
    key(&mut app, KeyCode::Tab);
    assert_eq!(input(&app), "/reload");
    // Several matches: the highlighted one.
    let (mut app, _) = test_app(Path::new("/p"));
    slash(&mut app, "/term");
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Tab);
    assert_eq!(input(&app), "/term bottom");
    assert_eq!(app.focus, Focus::Palette, "tab stays in the window");
}

#[test]
fn enter_on_a_partial_name_runs_the_highlighted_command() {
    let (mut app, _) = test_app(Path::new("/p"));
    let before = app.explorer.tree.show_hidden();
    slash(&mut app, "/hid");
    key(&mut app, KeyCode::Enter);
    assert!(app.palette.is_none(), "{:?}", error(&app));
    assert_eq!(
        app.explorer.tree.show_hidden(),
        !before,
        "{}",
        notices(&app)
    );
    // One that needs an argument is completed, not run.
    slash(&mut app, "/ru");
    key(&mut app, KeyCode::Enter);
    assert_eq!(input(&app), "/run ");
    assert_eq!(error(&app), None);
}

// ---- file, view, help, quit -------------------------------------------------------------------

#[test]
fn save_as_with_a_path_saves_there() {
    let (d, path, mut app, _) = app_with_file("a\n");
    typ(&mut app, "x");
    slash(&mut app, "/save-as sub/b.txt");
    std::fs::create_dir(d.path().join("sub")).unwrap();
    key(&mut app, KeyCode::Enter);
    assert!(app.palette.is_none(), "{:?}", error(&app));
    let b = d.path().join("sub/b.txt");
    assert_eq!(std::fs::read_to_string(&b).unwrap(), "xa\n");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "a\n");
    assert_eq!(app.active_doc().unwrap().path.as_deref(), Some(b.as_path()));
}

#[test]
fn save_as_without_a_path_opens_the_prompt_and_an_existing_target_asks() {
    let (d, _p, mut app, _) = app_with_file("a\n");
    slash(&mut app, "/save-as");
    key(&mut app, KeyCode::Enter);
    let dlg = app.dialog.as_ref().expect("the Save As prompt");
    assert!(matches!(
        dlg.action,
        crate::commands::dialog::DialogAction::SaveAs(_)
    ));
    key(&mut app, KeyCode::Esc);
    std::fs::write(d.path().join("taken.txt"), "keep\n").unwrap();
    slash(&mut app, "/save-as taken.txt");
    key(&mut app, KeyCode::Enter);
    assert!(
        matches!(
            app.dialog.as_ref().map(|d| &d.action),
            Some(crate::commands::dialog::DialogAction::ReplaceFile { .. })
        ),
        "an existing file is replaced only when confirmed"
    );
    assert_eq!(
        std::fs::read_to_string(d.path().join("taken.txt")).unwrap(),
        "keep\n"
    );
}

#[test]
fn save_as_refuses_a_folder_and_no_file() {
    let (d, _p, mut app, _) = app_with_file("a\n");
    std::fs::create_dir(d.path().join("sub")).unwrap();
    slash(&mut app, "/save-as sub");
    key(&mut app, KeyCode::Enter);
    assert_eq!(error(&app).as_deref(), Some("sub is a folder"));
    assert_eq!(input(&app), "/save-as sub");
    let (mut app, _) = test_app(d.path());
    slash(&mut app, "/save-as x.txt");
    key(&mut app, KeyCode::Enter);
    assert_eq!(error(&app).as_deref(), Some("no file is open"));
}

#[test]
fn reload_reads_the_file_again() {
    let (_d, path, mut app, _) = app_with_file("a\n");
    std::fs::write(&path, "fresh\n").unwrap();
    slash(&mut app, "/reload");
    key(&mut app, KeyCode::Enter);
    assert!(app.palette.is_none(), "{:?}", error(&app));
    assert_eq!(
        app.active_doc().unwrap().buffer.text().to_string(),
        "fresh\n"
    );
    let (mut app, _) = test_app(Path::new("/p"));
    slash(&mut app, "/reload");
    key(&mut app, KeyCode::Enter);
    assert_eq!(error(&app).as_deref(), Some("no file is open"));
}

#[test]
fn theme_switches_every_theme_and_completes_names() {
    let (mut app, _) = test_app(Path::new("/p"));
    for t in slash::THEMES {
        slash(&mut app, &format!("/theme {t}"));
        key(&mut app, KeyCode::Enter);
        assert!(app.palette.is_none());
        assert_eq!(app.settings.theme, *t);
    }
    slash(&mut app, "/theme dr");
    key(&mut app, KeyCode::Tab);
    assert_eq!(input(&app), "/theme dracula");
    // An empty choice lists the themes; Enter takes the highlighted one.
    let (mut app, _) = test_app(Path::new("/p"));
    slash(&mut app, "/theme ");
    assert_eq!(titles(&app), slash::THEMES);
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.settings.theme, "dracula");
}

#[test]
fn hidden_and_ignored_toggle_the_explorer() {
    let d = tempfile::tempdir().unwrap();
    let (mut app, _) = test_app(d.path());
    let (h, i) = (
        app.explorer.tree.show_hidden(),
        app.explorer.tree.show_ignored(),
    );
    slash(&mut app, "/hidden");
    key(&mut app, KeyCode::Enter);
    slash(&mut app, "/ignored");
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.explorer.tree.show_hidden(), !h);
    assert_eq!(app.explorer.tree.show_ignored(), !i);
    slash(&mut app, "/hidden x");
    key(&mut app, KeyCode::Enter);
    assert_eq!(error(&app).as_deref(), Some("/hidden takes no argument"));
}

/// T19: `/term right` and `/term bottom` set the panel position for the session; a hidden
/// panel stays hidden, the settings file is not written.
#[test]
fn term_right_and_bottom_set_the_panel_position() {
    use vr_config::TerminalPosition;
    let (mut app, _) = test_app(Path::new("/p"));
    slash(&mut app, "/term right");
    key(&mut app, KeyCode::Enter);
    assert!(app.palette.is_none(), "{:?}", error(&app));
    assert_eq!(app.terminal.position, TerminalPosition::Right);
    assert!(!app.terminal.visible);
    assert!(
        notices(&app).contains("terminal_position = \"right\""),
        "{}",
        notices(&app)
    );
    slash(&mut app, "/term right");
    key(&mut app, KeyCode::Enter);
    assert!(notices(&app).contains("already"), "{}", notices(&app));
    slash(&mut app, "/term bottom");
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.terminal.position, TerminalPosition::Bottom);
    assert_eq!(app.settings.terminal_position, TerminalPosition::Bottom);
    slash(&mut app, "/term right x");
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        error(&app).as_deref(),
        Some("/term right takes no argument")
    );
    key(&mut app, KeyCode::Esc);
    // /set changes the setting and the panel at once.
    slash(&mut app, "/set terminal_position right");
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.terminal.position, TerminalPosition::Right);
    slash(&mut app, "/set terminal_width_percent 55");
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.terminal.width_percent, 55);
    slash(&mut app, "/set terminal_width_percent 5");
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        error(&app).as_deref(),
        Some("terminal_width_percent must be between 10 and 90")
    );
    key(&mut app, KeyCode::Esc);
    slash(&mut app, "/set terminal_position left");
    key(&mut app, KeyCode::Enter);
    assert!(
        error(&app).unwrap().contains("terminal_position"),
        "{:?}",
        error(&app)
    );
}

/// D279 / D290: /wrap, /term new and /term close are gone; Enter on them says so and starts
/// or closes nothing (new / close: the '+' and 'x' of the terminal tab bar).
#[test]
fn removed_commands_are_unknown_and_change_nothing() {
    let d = tempfile::tempdir().unwrap();
    let (mut app, _) = test_app(d.path());
    use_sh(&mut app);
    for gone in ["/wrap", "/term new", "/term close"] {
        assert!(
            parse(gone).unwrap_err().starts_with("unknown command"),
            "{gone}"
        );
        slash(&mut app, gone);
        assert!(
            titles(&app).iter().all(|t| !t.starts_with(gone)),
            "{gone}: {:?}",
            titles(&app)
        );
        key(&mut app, KeyCode::Esc);
    }
    crate::commands::terminal::new_terminal(&mut app);
    slash(&mut app, "/term close");
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.terminal.tabs.len(), 1, "nothing closed");
    assert!(app.terminal.visible);
}

#[test]
fn help_lists_every_slash_command() {
    let (mut app, _) = test_app(Path::new("/p"));
    slash(&mut app, "/help");
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.focus, Focus::Palette, "help stays in the window");
    assert_eq!(input(&app), "/");
    let want: Vec<String> = SLASH_COMMANDS.iter().map(|c| c.usage()).collect();
    assert_eq!(titles(&app), want);
}

#[test]
fn q_keeps_the_unsaved_guard() {
    let (_d, _p, mut app, _) = app_with_file("a\n");
    typ(&mut app, "x");
    slash(&mut app, "/q");
    key(&mut app, KeyCode::Enter);
    assert!(!app.quit);
    assert!(
        matches!(
            app.dialog.as_ref().map(|d| &d.action),
            Some(crate::commands::dialog::DialogAction::Quit)
        ),
        "unsaved changes ask first"
    );
    let (mut app, _) = test_app(Path::new("/p"));
    slash(&mut app, "/q");
    key(&mut app, KeyCode::Enter);
    assert!(app.quit, "nothing unsaved: quits");
}

#[test]
fn q_bang_discards_with_a_recovery_copy() {
    let (d, _p, mut app, _) = app_with_file("a\n");
    let rec = d.path().join("recovery");
    app.recovery_dir = Some(rec.clone());
    typ(&mut app, "x");
    slash(&mut app, "/q!");
    key(&mut app, KeyCode::Enter);
    assert!(app.quit);
    assert!(app.dialog.is_none());
    let kept: Vec<_> = std::fs::read_dir(&rec).unwrap().collect();
    assert_eq!(
        kept.len(),
        1,
        "the unsaved buffer is in the recovery folder"
    );
}

fn use_sh(app: &mut App) {
    app.terminal.shell = Some(PathBuf::from("/bin/sh"));
    app.terminal.env = vec![
        (OsString::from("PS1"), OsString::from("$ ")),
        (OsString::from("ENV"), OsString::from("/dev/null")),
    ];
}

// ---- /run and /ai -----------------------------------------------------------------------------

const WAIT: Duration = Duration::from_secs(10);

fn screen(app: &App, tab: usize) -> String {
    app.terminal.tabs[tab].screen_text()
}

fn wait_for(app: &App, tab: usize, ok: impl Fn(&str) -> bool) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        let s = screen(app, tab);
        if ok(&s) {
            return s;
        }
        assert!(Instant::now() < deadline, "screen:\n{s}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn sh_app() -> (tempfile::TempDir, App) {
    let d = tempfile::tempdir().unwrap();
    let (mut app, _) = test_app(d.path());
    use_sh(&mut app);
    (d, app)
}

#[test]
fn run_opens_a_terminal_and_types_the_command_once() {
    let (_d, mut app) = sh_app();
    slash(&mut app, "/run echo hi");
    assert!(
        app.terminal.tabs.is_empty(),
        "nothing starts before the user's Enter"
    );
    key(&mut app, KeyCode::Enter);
    assert!(app.palette.is_none(), "{:?}", error(&app));
    assert_eq!(app.terminal.tabs.len(), 1);
    assert!(app.terminal.visible);
    assert_eq!(app.focus, Focus::Terminal);
    let s = wait_for(&app, 0, |s| printed(s, "hi"));
    assert_eq!(s.matches("echo hi").count(), 1, "sent exactly once:\n{s}");
    // The next /run goes to the same terminal.
    app.run_command("app.palette", crate::commands::CmdArgs::None);
    typ(&mut app, "/run echo 'a  b'");
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.terminal.tabs.len(), 1);
    let s = wait_for(&app, 0, |s| printed(s, "a  b"));
    assert_eq!(s.matches("echo hi").count(), 1, "{s}");
}

#[test]
fn run_without_enter_or_with_esc_starts_nothing() {
    let (_d, mut app) = sh_app();
    slash(&mut app, "/run echo hi");
    key(&mut app, KeyCode::Tab);
    key(&mut app, KeyCode::Esc);
    assert!(app.terminal.tabs.is_empty());
    assert!(app.palette.is_none());
}

#[test]
fn run_and_ai_refuse_control_characters() {
    let (_d, mut app) = sh_app();
    for name in ["run", "ai"] {
        let c = SLASH_COMMANDS.iter().find(|c| c.name == name).unwrap();
        assert_eq!(
            (c.run)(&mut app, "echo a\necho b"),
            Err(format!(
                "/{name}: the text must be one line without control characters"
            ))
        );
    }
    assert!(app.terminal.tabs.is_empty());
}

#[test]
fn run_in_an_exited_terminal_opens_a_new_one() {
    let (_d, mut app) = sh_app();
    slash(&mut app, "/run exit");
    key(&mut app, KeyCode::Enter);
    let deadline = Instant::now() + WAIT;
    while app.terminal.tabs[0].running() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    app.run_command("app.palette", crate::commands::CmdArgs::None);
    typ(&mut app, "/run echo again");
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.terminal.tabs.len(), 2);
    assert_eq!(app.terminal.active, 1);
    wait_for(&app, 1, |s| printed(s, "again"));
}

#[test]
fn ai_starts_only_tools_of_ai_tools_never_through_a_shell() {
    let (_d, mut app) = sh_app();
    crate::commands::terminal::new_terminal(&mut app);
    // Not a tool of ai_tools: refused, nothing typed into a shell (SEC-8; the AI panel runs the
    // tools, ai_tests covers it).
    slash_needs(&mut app, "/ai echo tool-ran", "/ai takes <tool> [@account]");
    slash_needs(
        &mut app,
        "/ai vr-ide-no-such-tool",
        "/ai: vr-ide-no-such-tool is not in ai_tools (claude, codex, gemini, aider, cursor-agent)",
    );
    assert_eq!(app.terminal.tabs.len(), 1, "no new terminal tab");
    assert!(app.ai.sessions.is_empty());
    slash_needs(&mut app, "/ai", "/ai needs <tool> [@account]");
}

/// Enter on `text` (in a fresh window) shows `message`.
fn slash_needs(app: &mut App, text: &str, message: &str) {
    app.run_command("app.palette", crate::commands::CmdArgs::None);
    typ(app, text);
    key(app, KeyCode::Enter);
    assert_eq!(error(app).as_deref(), Some(message));
    key(app, KeyCode::Esc);
}

#[test]
fn run_needs_a_command() {
    let (_d, mut app) = sh_app();
    slash_needs(&mut app, "/run   ", "/run needs <command>");
    assert!(app.terminal.tabs.is_empty());
}

/// A line of `s` shows `out` as output. A new shell's prompt can land in front of it: the
/// tty echoes a line typed before the shell started, then the shell prints its prompt.
fn printed(s: &str, out: &str) -> bool {
    s.lines()
        .any(|l| l.trim_end() == out || l.trim_end() == format!("$ {out}"))
}

// ---- workspace --------------------------------------------------------------------------------

/// A temp parent holding `main/` (the root) and `other/`, `other2/`, `file.txt`.
fn ws_app() -> (tempfile::TempDir, App) {
    let d = tempfile::tempdir().unwrap();
    for f in ["main", "other", "other2", ".hid"] {
        std::fs::create_dir(d.path().join(f)).unwrap();
    }
    std::fs::write(d.path().join("file.txt"), "x").unwrap();
    let (mut app, _) = test_app(&d.path().join("main"));
    app.recent_workspaces_file = Some(d.path().join("recent.toml"));
    (d, app)
}

fn root_paths(app: &App) -> Vec<PathBuf> {
    app.workspace.paths()
}

#[test]
fn ws_add_adds_a_folder_and_refuses_bad_ones() {
    let (d, mut app) = ws_app();
    slash(&mut app, "/ws add ../other");
    key(&mut app, KeyCode::Enter);
    assert!(app.palette.is_none(), "{:?}", error(&app));
    let other = d.path().join("other").canonicalize().unwrap();
    assert_eq!(root_paths(&app).len(), 2);
    assert_eq!(root_paths(&app)[1], other);
    // The W-1 checks: a repeat, a missing folder, a file.
    for (arg, want) in [
        ("../other", "is already in the workspace"),
        ("../missing", "does not exist"),
        ("../file.txt", "is not a folder"),
    ] {
        slash(&mut app, &format!("/ws add {arg}"));
        key(&mut app, KeyCode::Enter);
        let e = error(&app).unwrap_or_default();
        assert!(e.contains(want), "{arg}: {e}");
        assert_eq!(input(&app), format!("/ws add {arg}"));
        key(&mut app, KeyCode::Esc);
    }
    assert_eq!(root_paths(&app).len(), 2);
}

#[test]
fn ws_add_tab_completes_folders() {
    let (_d, mut app) = ws_app();
    slash(&mut app, "/ws add ../oth");
    assert_eq!(
        titles(&app),
        ["../other/", "../other2/"],
        "folders only, no file.txt"
    );
    key(&mut app, KeyCode::Tab);
    assert_eq!(input(&app), "/ws add ../other", "the shared prefix first");
    // D6: a second Tab opens the candidate list, the next one steps to the second.
    key(&mut app, KeyCode::Tab);
    assert_eq!(input(&app), "/ws add ../other/");
    key(&mut app, KeyCode::Tab);
    assert_eq!(input(&app), "/ws add ../other2/");
    // Hidden folders only when the name starts with a dot.
    set(&mut app, "/ws add ../");
    assert!(!titles(&app).iter().any(|t| t.contains(".hid")));
    set(&mut app, "/ws add ../.h");
    assert_eq!(titles(&app), ["../.hid/"]);
}

pub(crate) fn set(app: &mut App, text: &str) {
    app.palette.as_mut().unwrap().input.set(text);
    crate::commands::palette::refilter(app);
}

#[test]
fn save_as_tab_completes_paths() {
    let (d, _p, mut app, _) = app_with_file("a\n");
    std::fs::create_dir(d.path().join("src")).unwrap();
    std::fs::write(d.path().join("src/main.rs"), "").unwrap();
    slash(&mut app, "/save-as sr");
    key(&mut app, KeyCode::Tab);
    assert_eq!(input(&app), "/save-as src/");
    typ(&mut app, "m");
    key(&mut app, KeyCode::Tab);
    assert_eq!(input(&app), "/save-as src/main.rs");
}

#[test]
fn ws_remove_removes_the_named_or_active_root() {
    let (d, mut app) = ws_app();
    app.add_root(&d.path().join("other")).unwrap();
    app.add_root(&d.path().join("other2")).unwrap();
    slash(&mut app, "/ws remove ");
    assert_eq!(titles(&app).len(), 3, "the roots are listed");
    set(&mut app, "/ws remove other2");
    key(&mut app, KeyCode::Enter);
    assert!(app.palette.is_none(), "{:?}", error(&app));
    assert_eq!(root_paths(&app).len(), 2);
    slash(&mut app, "/ws remove nope");
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        error(&app).as_deref(),
        Some("/ws remove: no workspace folder named nope")
    );
    key(&mut app, KeyCode::Esc);
    // No name: the root of the active file.
    std::fs::write(d.path().join("other/f.txt"), "").unwrap();
    app.open_path(&d.path().join("other/f.txt")).unwrap();
    slash(&mut app, "/ws remove");
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        root_paths(&app),
        [d.path().join("main").canonicalize().unwrap()]
    );
    // The last root stays.
    slash(&mut app, "/ws remove");
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        error(&app).as_deref(),
        Some("the last folder of the workspace cannot be removed")
    );
}

#[test]
fn ws_save_and_open_round_trip() {
    let (d, mut app) = ws_app();
    app.add_root(&d.path().join("other")).unwrap();
    slash(&mut app, "/ws save");
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        error(&app).as_deref(),
        Some("the workspace has no file yet: /ws save <file>")
    );
    set(&mut app, "/ws save ../team");
    key(&mut app, KeyCode::Enter);
    assert!(app.palette.is_none(), "{:?}", error(&app));
    let file = d.path().join("team.vr-workspace");
    assert!(file.is_file(), "the extension is added");
    // Saving again to the workspace's own file.
    slash(&mut app, "/ws save");
    key(&mut app, KeyCode::Enter);
    assert!(app.palette.is_none(), "{:?}", error(&app));
    // An existing other file is not replaced.
    std::fs::write(d.path().join("keep.vr-workspace"), "roots = []\n").unwrap();
    slash(&mut app, "/ws save ../keep.vr-workspace");
    key(&mut app, KeyCode::Enter);
    assert!(
        error(&app).unwrap_or_default().contains("exists"),
        "{:?}",
        error(&app)
    );
    assert_eq!(
        std::fs::read_to_string(d.path().join("keep.vr-workspace")).unwrap(),
        "roots = []\n"
    );
    key(&mut app, KeyCode::Esc);
    // Open it from a one-root app.
    let (mut app2, _) = test_app(&d.path().join("other2"));
    app2.recent_workspaces_file = Some(d.path().join("recent.toml"));
    slash(&mut app2, "/ws open ../team.vr-workspace");
    key(&mut app2, KeyCode::Enter);
    assert!(app2.palette.is_none(), "{:?}", error(&app2));
    assert_eq!(root_paths(&app2).len(), 2);
    slash(&mut app2, "/ws open ../nope.vr-workspace");
    key(&mut app2, KeyCode::Enter);
    assert!(error(&app2).is_some());
}

#[test]
fn ws_recent_lists_and_opens_by_number() {
    let (d, mut app) = ws_app();
    app.add_root(&d.path().join("other")).unwrap();
    app.remember_workspace();
    let (mut app2, _) = test_app(&d.path().join("other2"));
    app2.recent_workspaces_file = Some(d.path().join("recent.toml"));
    app2.recent_workspaces = app.recent_workspaces.clone();
    slash(&mut app2, "/ws recent");
    key(&mut app2, KeyCode::Enter);
    assert_eq!(app2.focus, Focus::Palette, "no number: the list");
    assert_eq!(input(&app2), "/ws recent ");
    assert_eq!(titles(&app2).len(), 1, "{:?}", titles(&app2));
    assert!(titles(&app2)[0].starts_with("1  "));
    key(&mut app2, KeyCode::Enter);
    assert!(app2.palette.is_none(), "{:?}", error(&app2));
    assert_eq!(root_paths(&app2).len(), 2);
    slash(&mut app2, "/ws recent 9");
    key(&mut app2, KeyCode::Enter);
    assert_eq!(
        error(&app2).as_deref(),
        Some("/ws recent: no recent workspace 9")
    );
}

// ---- docs, snapshots --------------------------------------------------------------------------

const DOCS_START: &str =
    "<!-- slash-commands:start (generated from the registry; a test checks it) -->";
const DOCS_END: &str = "<!-- slash-commands:end -->";

#[test]
fn docs_table_matches_the_registry() {
    let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/commands.md");
    let text = std::fs::read_to_string(&docs).unwrap();
    let start = text.find(DOCS_START).expect("start marker") + DOCS_START.len();
    let end = text.find(DOCS_END).expect("end marker");
    assert_eq!(text[start..end].trim(), slash::docs_table().trim());
    for c in SLASH_COMMANDS {
        assert!(
            slash::docs_table().contains(&format!("`/{}", c.name)),
            "{}",
            c.name
        );
    }
}

/// Prints the docs table (`cargo test -p vr-ide --lib print_docs_table -- --ignored --nocapture`).
#[test]
#[ignore = "prints the table for docs/commands.md"]
fn print_docs_table() {
    print!("{}", slash::docs_table());
}

fn draw(app: &mut App, w: u16, h: u16) -> Terminal<TestBackend> {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| crate::ui::render(f, app)).unwrap();
    t
}

fn row_text(t: &Terminal<TestBackend>, y: u16) -> String {
    let buf = t.backend().buffer();
    (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
}

#[test]
fn the_error_line_is_drawn_under_the_input() {
    let (mut app, _) = test_app(Path::new("/home/user/DEV/shop"));
    slash(&mut app, "/nope");
    key(&mut app, KeyCode::Enter);
    let t = draw(&mut app, 80, 24);
    let area = app.palette.as_ref().unwrap().area;
    assert!(row_text(&t, area.y + 1).contains("> /nope"));
    let line = row_text(&t, area.y + 2);
    assert!(line.contains("unknown command /nope"), "{line}");
    // The list starts below it.
    assert_eq!(app.palette.as_ref().unwrap().list_area.y, area.y + 3);
}

#[test]
fn render_slash_mode() {
    for (w, h) in [(80, 24), (120, 40), (40, 12)] {
        let (mut app, _) = test_app(Path::new("/home/user/DEV/shop"));
        slash(&mut app, "/");
        let t = draw(&mut app, w, h);
        insta::assert_snapshot!(format!("render_slash_list_{w}x{h}"), t.backend());
        typ(&mut app, "theme pink");
        key(&mut app, KeyCode::Enter);
        let t = draw(&mut app, w, h);
        insta::assert_snapshot!(format!("render_slash_error_{w}x{h}"), t.backend());
    }
}

#[test]
fn enter_on_a_bare_slash_runs_nothing() {
    let (_d, _p, mut app, _) = app_with_file("a\n");
    slash(&mut app, "/");
    key(&mut app, KeyCode::Enter);
    assert!(app.dialog.is_none(), "the first listed command did not run");
    assert_eq!(
        error(&app).as_deref(),
        Some("type a command (/help lists them)")
    );
}

// ---- review round 1 ---------------------------------------------------------------------------

#[test]
fn q_bang_with_a_running_terminal_asks_once_and_discards() {
    let (d, path, mut app, _) = app_with_file("a\n");
    use_sh(&mut app);
    let rec = d.path().join("recovery");
    app.recovery_dir = Some(rec.clone());
    typ(&mut app, "x");
    crate::commands::terminal::new_terminal(&mut app);
    assert_eq!(app.terminal.running(), 1);
    app.run_command("app.palette", crate::commands::CmdArgs::None);
    typ(&mut app, "/q!");
    key(&mut app, KeyCode::Enter);
    assert!(app.dialog.is_some(), "running terminals ask first");
    // Quit anyway (Cancel is highlighted first).
    key(&mut app, KeyCode::Left);
    key(&mut app, KeyCode::Enter);
    assert!(app.dialog.is_none(), "no second question");
    assert!(app.quit);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "a\n",
        "file untouched"
    );
    let kept: Vec<_> = std::fs::read_dir(&rec).unwrap().collect();
    assert_eq!(
        kept.len(),
        1,
        "the unsaved buffer is in the recovery folder"
    );
}

#[test]
fn q_bang_cancelled_leaves_no_discard_behind() {
    let (_d, path, mut app, _) = app_with_file("a\n");
    use_sh(&mut app);
    typ(&mut app, "x");
    crate::commands::terminal::new_terminal(&mut app);
    app.run_command("app.palette", crate::commands::CmdArgs::None);
    typ(&mut app, "/q!");
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Enter); // Cancel
    assert!(app.dialog.is_none() && !app.quit);
    // A later /q still asks about the unsaved file after Quit anyway.
    app.run_command("app.palette", crate::commands::CmdArgs::None);
    typ(&mut app, "/q");
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Left);
    key(&mut app, KeyCode::Enter);
    assert!(!app.quit);
    assert!(matches!(
        app.dialog.as_ref().map(|d| &d.action),
        Some(crate::commands::dialog::DialogAction::Quit)
    ));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "a\n");
}

#[test]
fn run_opens_a_new_tab_when_a_program_holds_the_active_terminal() {
    let (_d, mut app) = sh_app();
    // `set -m`: job control, as an interactive shell has, so cat gets the terminal.
    slash(&mut app, "/run set -m; cat");
    key(&mut app, KeyCode::Enter);
    let deadline = Instant::now() + WAIT;
    while app.terminal.tabs[0].shell_in_foreground() {
        assert!(Instant::now() < deadline, "cat never took the terminal");
        std::thread::sleep(Duration::from_millis(10));
    }
    app.run_command("app.palette", crate::commands::CmdArgs::None);
    typ(&mut app, "/run echo REVIEW_RUN");
    key(&mut app, KeyCode::Enter);
    assert!(app.palette.is_none(), "{:?}", error(&app));
    assert_eq!(app.terminal.tabs.len(), 2, "a new tab");
    assert_eq!(app.terminal.active, 1);
    wait_for(&app, 1, |s| printed(s, "REVIEW_RUN"));
    assert!(
        !screen(&app, 0).contains("REVIEW_RUN"),
        "nothing typed into cat:\n{}",
        screen(&app, 0)
    );
    assert!(
        notices(&app).contains("new terminal tab"),
        "{}",
        notices(&app)
    );
}

#[test]
fn ws_save_refuses_a_workspace_file_changed_since_it_was_opened() {
    let (d, mut app) = ws_app();
    app.add_root(&d.path().join("other")).unwrap();
    set_input_and_enter(&mut app, "/ws save ../team");
    let file = d.path().join("team.vr-workspace");
    // Opened from the file, then the file is edited by hand (or by another window).
    let (mut app2, _) = test_app(&d.path().join("other2"));
    app2.recent_workspaces_file = Some(d.path().join("recent.toml"));
    set_input_and_enter(&mut app2, "/ws open ../team.vr-workspace");
    assert!(app2.palette.is_none(), "{:?}", error(&app2));
    let edited = format!("{}# hand edit\n", std::fs::read_to_string(&file).unwrap());
    std::fs::write(&file, &edited).unwrap();
    set_input_and_enter(&mut app2, "/ws save");
    let e = error(&app2).unwrap_or_default();
    assert!(e.contains("changed on disk"), "{e}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), edited, "kept");
    key(&mut app2, KeyCode::Esc);
    // The first window saved it last; its own save is refused the same way.
    set_input_and_enter(&mut app, "/ws save");
    assert!(error(&app).unwrap_or_default().contains("changed on disk"));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), edited);
    key(&mut app, KeyCode::Esc);
    // Opened again: the version is the one shown, so the save goes through.
    set_input_and_enter(&mut app2, "/ws open ../team.vr-workspace");
    set_input_and_enter(&mut app2, "/ws save");
    assert!(app2.palette.is_none(), "{:?}", error(&app2));
    // And a second save onto its own last save needs no reopen.
    set_input_and_enter(&mut app2, "/ws save");
    assert!(app2.palette.is_none(), "{:?}", error(&app2));
}

/// F1 (or the palette command when a terminal has the keys), `text`, Enter.
pub(crate) fn set_input_and_enter(app: &mut App, text: &str) {
    app.run_command("app.palette", crate::commands::CmdArgs::None);
    typ(app, text);
    key(app, KeyCode::Enter);
}

#[test]
fn slash_titles_too_wide_end_in_an_ellipsis_and_keep_a_description_tail() {
    let (mut app, _) = test_app(Path::new("/home/user/DEV/shop"));
    slash(&mut app, "/the");
    let t = draw(&mut app, 40, 12);
    let list_y = app.palette.as_ref().unwrap().list_area.y;
    let row = row_text(&t, list_y);
    assert!(row.contains("/theme synthwave"), "{row}");
    assert!(
        row.contains('…'),
        "the title is cut with an ellipsis: {row}"
    );
    assert!(row.contains("Swi"), "a short description tail: {row}");
    // Wide enough for the title (the window is at most 72 cells): only the description is cut.
    let t = draw(&mut app, 120, 40);
    let row = row_text(&t, app.palette.as_ref().unwrap().list_area.y);
    assert!(
        row.contains("/theme synthwave|dracula|dark|light  Switch the colour"),
        "{row}"
    );
}

#[test]
fn path_completion_lists_a_folder_once_while_the_window_is_open() {
    let (d, mut app) = ws_app();
    let big = d.path().join("big");
    std::fs::create_dir(&big).unwrap();
    for i in 0..50 {
        std::fs::write(big.join(format!("f{i:02}")), "").unwrap();
    }
    std::fs::create_dir(big.join("dir")).unwrap();
    std::os::unix::fs::symlink(big.join("dir"), big.join("link")).unwrap();
    slash(&mut app, "/save-as ../big/");
    assert!(titles(&app).contains(&"../big/dir/".to_string()));
    assert!(
        titles(&app).contains(&"../big/link/".to_string()),
        "a link to a folder"
    );
    let listed = crate::commands::slash::dir_listings();
    typ(&mut app, "f");
    key(&mut app, KeyCode::Backspace);
    typ(&mut app, "f0");
    crate::commands::palette::refresh_keeping_selection(&mut app);
    assert_eq!(
        crate::commands::slash::dir_listings(),
        listed,
        "no new read of the folder per key or index update"
    );
    assert_eq!(titles(&app).len(), 10);
    // Another folder is read once.
    key(&mut app, KeyCode::Backspace);
    key(&mut app, KeyCode::Backspace);
    typ(&mut app, "dir/");
    assert_eq!(crate::commands::slash::dir_listings(), listed + 1);
}
