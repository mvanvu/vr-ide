//! Dispatch tests through the `App` API (no terminal).

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use vr_config::{Keymap, Settings};

use crate::app::{App, NoticeLevel};
use crate::clipboard::MemoryClipboard;
use crate::commands::CmdArgs;

pub(crate) fn test_app(root: &Path) -> (App, MemoryClipboard) {
    let clip = MemoryClipboard::new();
    let mut app = App::new(
        root.to_path_buf(),
        Keymap::load(None).0,
        Settings::default(),
        Box::new(clip.clone()),
    );
    // Timing of Enter is tested on its own (held_or_in_flight_enter_never_answers_a_dialog).
    app.enter_settle = std::time::Duration::ZERO;
    app.enter_repeat_gap = std::time::Duration::ZERO;
    (app, clip)
}

/// Read mode's built-in edit key (T21): space, then i.
pub(crate) fn space_i(app: &mut App) {
    app.handle_key(key(KeyCode::Char(' '), KeyModifiers::NONE));
    app.handle_key(key(KeyCode::Char('i'), KeyModifiers::NONE));
}

/// A temp project with one file, opened in the app and put in edit mode with space i (D145),
/// the state the editing tests start from; `mode_tests` covers read mode.
pub(crate) fn app_with_file(content: &str) -> (tempfile::TempDir, PathBuf, App, MemoryClipboard) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    std::fs::write(&path, content).unwrap();
    let (mut app, clip) = test_app(dir.path());
    app.open_path(&path).unwrap();
    space_i(&mut app);
    (dir, path, app, clip)
}

fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, mods)
}

fn ctrl(c: char) -> KeyEvent {
    key(KeyCode::Char(c), KeyModifiers::CONTROL)
}

/// The crossterm event for a key written as in the keymap ("ctrl+r", "shift+f9", "ctrl+up"),
/// as crossterm reports it from a legacy terminal: ctrl+/ arrives as the byte 0x1f, which
/// crossterm reports as ctrl+7; every other key used here arrives as itself.
pub(crate) fn key_event(text: &str) -> KeyEvent {
    use vr_config::KeyCode as C;
    let chord: vr_config::KeyChord = text.parse().unwrap();
    let mut mods = KeyModifiers::NONE;
    if chord.mods.ctrl {
        mods |= KeyModifiers::CONTROL;
    }
    if chord.mods.shift {
        mods |= KeyModifiers::SHIFT;
    }
    if chord.mods.alt {
        mods |= KeyModifiers::ALT;
    }
    let code = match chord.code {
        C::Char('/') if chord.mods.ctrl => KeyCode::Char('7'),
        C::Char(c) => KeyCode::Char(c),
        C::F(n) => KeyCode::F(n),
        C::Up => KeyCode::Up,
        C::Down => KeyCode::Down,
        C::Left => KeyCode::Left,
        C::Right => KeyCode::Right,
        C::Delete => KeyCode::Delete,
        C::Esc => KeyCode::Esc,
        C::Tab if chord.mods.shift => KeyCode::BackTab,
        C::Tab => KeyCode::Tab,
        C::Enter => KeyCode::Enter,
        other => panic!("the test helper cannot send {other:?}"),
    };
    key(code, mods)
}

/// Runs `cmd` as the palette would (for commands with no default key, T21).
pub(crate) fn run(app: &mut App, cmd: &str) {
    app.run_command(cmd, CmdArgs::None);
}

/// Presses the key written as in the keymap ("ctrl+r", "shift+f12").
pub(crate) fn press(app: &mut App, text: &str) {
    app.handle_key(key_event(text));
}

fn typ(app: &mut App, s: &str) {
    for c in s.chars() {
        let k = if c == '\n' {
            key(KeyCode::Enter, KeyModifiers::NONE)
        } else {
            key(KeyCode::Char(c), KeyModifiers::NONE)
        };
        app.handle_key(k);
    }
}

fn text(app: &App) -> String {
    app.active_doc().unwrap().buffer.text().to_string()
}

fn notices(app: &App) -> Vec<String> {
    app.notices.iter().map(|n| n.text.clone()).collect()
}

#[test]
fn dispatch_ctrl_s_saves() {
    let (_d, path, mut app, _) = app_with_file("hello\n");
    typ(&mut app, "x");
    app.handle_key(ctrl('s'));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "xhello\n");
    assert!(!app.active_doc().unwrap().is_modified());
}

#[test]
fn typing_marks_modified_and_save_clears() {
    let (_d, path, mut app, _) = app_with_file("ab\n");
    assert!(!app.active_doc().unwrap().is_modified());
    typ(&mut app, "x");
    assert_eq!(text(&app), "xab\n");
    assert!(app.active_doc().unwrap().is_modified());
    app.handle_key(key(KeyCode::Right, KeyModifiers::NONE));
    app.handle_key(key(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(text(&app), "xb\n");
    app.handle_key(key(KeyCode::Delete, KeyModifiers::NONE));
    assert_eq!(text(&app), "x\n");
    assert!(app.active_doc().unwrap().is_modified());
    app.run_command("file.save", CmdArgs::None);
    assert!(!app.active_doc().unwrap().is_modified());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "x\n");
}

#[test]
fn undo_redo_via_commands() {
    let (_d, _p, mut app, _) = app_with_file("one\n");
    typ(&mut app, "ab");
    app.handle_key(key(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(text(&app), "ab\none\n");
    app.handle_key(ctrl('z'));
    assert_eq!(text(&app), "abone\n");
    app.handle_key(ctrl('z'));
    assert_eq!(text(&app), "one\n");
    assert!(!app.active_doc().unwrap().is_modified());
    app.handle_key(ctrl('y'));
    assert_eq!(text(&app), "abone\n");
    app.handle_key(ctrl('z'));
    assert_eq!(text(&app), "one\n");
}

#[test]
fn unknown_command_in_keymap_shows_notice_not_crash() {
    let dir = tempfile::tempdir().unwrap();
    let km = dir.path().join("keymap.toml");
    std::fs::write(&km, "ctrl+s = \"file.save\"\n\"f10\" = \"nope.cmd\"\n").unwrap();
    let (keymap, warnings) = Keymap::load(Some(&km));
    let mut app = App::new(
        dir.path().to_path_buf(),
        keymap,
        Settings::default(),
        Box::new(MemoryClipboard::new()),
    );
    app.report_startup(warnings, true);
    let all = notices(&app).join("\n");
    assert!(all.contains("nope.cmd"), "{all}");
    assert!(app.notices.iter().any(|n| n.level == NoticeLevel::Warning));
    // The rest of the keymap still works and the unknown key does nothing.
    std::fs::write(dir.path().join("f.txt"), "1").unwrap();
    app.open_path(Path::new("f.txt")).unwrap();
    space_i(&mut app);
    app.handle_key(key(KeyCode::F(10), KeyModifiers::NONE));
    typ(&mut app, "x");
    app.handle_key(ctrl('s'));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "x1"
    );
}

#[test]
fn copy_paste_uses_clipboard() {
    let (_d, _p, mut app, clip) = app_with_file("hello world\n");
    // Select "hello" with shift+right x5.
    for _ in 0..5 {
        app.handle_key(key(KeyCode::Right, KeyModifiers::SHIFT));
    }
    app.handle_key(ctrl('c'));
    assert_eq!(clip.contents().as_deref(), Some("hello"));
    assert_eq!(text(&app), "hello world\n");
    app.handle_key(key(KeyCode::End, KeyModifiers::NONE));
    app.handle_key(ctrl('v'));
    assert_eq!(text(&app), "hello worldhello\n");
    // Text another application put on the clipboard is pasted too.
    let mut other = clip.clone();
    crate::clipboard::Clipboard::set(&mut other, "Xin chào").unwrap();
    app.handle_key(ctrl('v'));
    assert_eq!(text(&app), "hello worldhelloXin chào\n");
}

#[test]
fn cut_removes_selection() {
    let (_d, _p, mut app, clip) = app_with_file("abc def\nnext\n");
    app.handle_key(key(
        KeyCode::Right,
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    app.handle_key(ctrl('x'));
    assert_eq!(clip.contents().as_deref(), Some("abc"));
    assert_eq!(text(&app), " def\nnext\n");
    // Without a selection, cut takes the whole line (VS Code).
    app.handle_key(ctrl('x'));
    assert_eq!(clip.contents().as_deref(), Some(" def\n"));
    assert_eq!(text(&app), "next\n");
    app.handle_key(ctrl('z'));
    assert_eq!(text(&app), " def\nnext\n");
}

#[test]
fn select_all_and_line_commands() {
    let (_d, _p, mut app, clip) = app_with_file("a\nb\n");
    app.run_command("edit.duplicate_line", CmdArgs::None);
    assert_eq!(text(&app), "a\na\nb\n");
    app.run_command("edit.move_line_down", CmdArgs::None);
    app.run_command("edit.delete_line", CmdArgs::None);
    app.run_command("edit.indent", CmdArgs::None);
    app.run_command("edit.toggle_comment", CmdArgs::None);
    app.run_command("edit.outdent", CmdArgs::None);
    app.run_command("edit.move_line_up", CmdArgs::None);
    app.run_command("edit.select_all", CmdArgs::None);
    app.run_command("edit.copy", CmdArgs::None);
    let all = text(&app);
    assert_eq!(clip.contents().as_deref(), Some(all.as_str()));
    assert_ne!(all, "a\na\nb\n");
}

#[test]
fn external_modify_unmodified_doc_reloads() {
    let (_d, path, mut app, _) = app_with_file("old\n");
    std::fs::write(&path, "new from agent\n").unwrap();
    app.handle_fs_event(vr_core::FsEvent::Modified(path.clone()));
    assert_eq!(text(&app), "new from agent\n");
    assert!(!app.active_doc().unwrap().is_modified());
}

#[test]
fn external_modify_modified_doc_keeps_buffer_and_marks_tab() {
    let (_d, path, mut app, _) = app_with_file("old\n");
    typ(&mut app, "mine ");
    std::fs::write(&path, "theirs\n").unwrap();
    app.handle_fs_event(vr_core::FsEvent::Modified(path.clone()));
    assert_eq!(text(&app), "mine old\n");
    let doc = app.active_doc().unwrap();
    assert_eq!(doc.disk_state, vr_core::DiskState::ChangedOnDisk);
}

#[test]
fn external_delete_keeps_buffer_marked_deleted() {
    let (_d, path, mut app, _) = app_with_file("keep me\n");
    std::fs::remove_file(&path).unwrap();
    app.handle_fs_event(vr_core::FsEvent::Removed(path.clone()));
    assert_eq!(text(&app), "keep me\n");
    assert_eq!(
        app.active_doc().unwrap().disk_state,
        vr_core::DiskState::DeletedOnDisk
    );
}

#[test]
fn external_modify_reloads_within_one_second_with_real_watcher() {
    let (_d, path, mut app, _) = app_with_file("v1\n");
    app.start_watcher();
    // Re-open so the watcher sees the file (open_path watches when a watcher exists).
    app.close_doc(0);
    app.open_path(&path).unwrap();
    let start = std::time::Instant::now();
    std::fs::write(&path, "v2\n").unwrap();
    while text(&app) != "v2\n" && start.elapsed() < std::time::Duration::from_secs(3) {
        let ev = app
            .fs_rx
            .as_ref()
            .unwrap()
            .recv_timeout(std::time::Duration::from_millis(100));
        if let Ok(ev) = ev {
            app.handle_fs_event(ev);
        }
    }
    let took = start.elapsed();
    assert_eq!(text(&app), "v2\n");
    assert!(took < std::time::Duration::from_secs(1), "took {took:?}");
    // Our own save is not reported back as an external change.
    typ(&mut app, "x");
    app.handle_key(ctrl('s'));
    let echo = app
        .fs_rx
        .as_ref()
        .unwrap()
        .recv_timeout(std::time::Duration::from_millis(600));
    assert!(echo.is_err(), "own save echoed: {echo:?}");
}

#[test]
fn open_tabs_switch_and_reopen_activates() {
    let dir = tempfile::tempdir().unwrap();
    for n in ["a.txt", "b.txt", "c.txt"] {
        std::fs::write(dir.path().join(n), n).unwrap();
    }
    let (mut app, _) = test_app(dir.path());
    for n in ["a.txt", "b.txt", "c.txt"] {
        app.run_command("file.open", CmdArgs::Path(n.into()));
    }
    assert_eq!(app.docs.len(), 3);
    assert_eq!(app.active, 2);
    press(&mut app, "f9");
    assert_eq!(app.active, 0);
    press(&mut app, "shift+f9");
    assert_eq!(app.active, 2);
    app.run_command("tab.prev", CmdArgs::None);
    assert_eq!(app.active, 1);
    app.run_command("file.open", CmdArgs::Path(dir.path().join("a.txt")));
    assert_eq!((app.docs.len(), app.active), (3, 0));
    // A missing file is an error notice, not a crash.
    app.run_command("file.open", CmdArgs::Path("missing.txt".into()));
    assert!(app.notices.iter().any(|n| n.level == NoticeLevel::Error));
    assert_eq!(app.docs.len(), 3);
}

#[test]
fn save_as_with_path_and_reload() {
    let (d, path, mut app, _) = app_with_file("one\n");
    typ(&mut app, "x");
    let target = d.path().join("copy.txt");
    app.run_command("file.save_as", CmdArgs::Path(target.clone()));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "xone\n");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\n");
    assert_eq!(
        app.active_doc().unwrap().path.as_deref(),
        Some(target.as_path())
    );
    typ(&mut app, "y");
    app.run_command("file.reload", CmdArgs::None);
    assert_eq!(text(&app), "xone\n");
    app.handle_key(ctrl('z'));
    assert_eq!(text(&app), "xyone\n");
}

#[test]
fn read_only_doc_refuses_edits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("latin1.txt");
    std::fs::write(&path, b"caf\xe9\n").unwrap();
    let (mut app, _) = test_app(dir.path());
    app.open_path(&path).unwrap();
    assert!(app.active_doc().unwrap().read_only);
    let before = text(&app);
    typ(&mut app, "x");
    app.handle_paste("pasted");
    app.handle_key(key(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(text(&app), before);
    assert!(notices(&app).iter().any(|n| n.contains("read-only")));
}

#[test]
fn bracketed_paste_is_one_undo_step() {
    let (_d, _p, mut app, _) = app_with_file("\n");
    app.handle_paste("line1\nline2");
    assert_eq!(text(&app), "line1\nline2\n");
    app.handle_key(ctrl('z'));
    assert_eq!(text(&app), "\n");
}

/// T22: with no file open there is nothing to find in: ctrl+f opens the find box on the
/// project; Esc closes it and then dismisses the oldest notice.
#[test]
fn find_without_a_file_searches_the_project() {
    let dir = tempfile::tempdir().unwrap();
    let (mut app, _) = test_app(dir.path());
    app.handle_key(ctrl('f'));
    assert!(app.find.project());
    assert_eq!(app.focus, crate::app::Focus::Search);
    app.handle_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!app.find.open);
    app.info("a notice");
    let n = app.notices.len();
    app.handle_key(key(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(app.notices.len(), n - 1);
}

/// The go-to-file index is rebuilt on every ctrl+p (D115): the "first N files" notice of a
/// project above the cap is shown once per session, not on every open.
#[test]
fn index_cap_notice_once_per_session() {
    let dir = tempfile::tempdir().unwrap();
    let (mut app, _) = test_app(dir.path());
    app.note_index_cap(200_000);
    app.note_index_cap(200_000);
    let shown = notices(&app)
        .iter()
        .filter(|n| n.contains("first 200000 files"))
        .count();
    assert_eq!(shown, 1, "{:?}", notices(&app));
}

/// Every plan command is registered and offered in the palette.
#[test]
fn find_and_search_commands_are_in_the_palette() {
    let dir = tempfile::tempdir().unwrap();
    let (mut app, _) = test_app(dir.path());
    app.run_command("app.palette", CmdArgs::None);
    let names: Vec<&str> = app
        .palette
        .as_ref()
        .unwrap()
        .items
        .iter()
        .map(|i| i.name)
        .collect();
    for want in [
        "find.open",
        "find.replace_all",
        "search.project",
        "search.replace_project",
    ] {
        assert!(names.contains(&want), "{want} missing");
    }
}

#[test]
fn title_shows_project_root() {
    let dir = tempfile::tempdir().unwrap();
    let (app, _) = test_app(dir.path());
    assert!(app.title().contains(&dir.path().display().to_string()));
}

#[test]
fn legacy_terminal_notice_lists_only_unreachable_user_keys() {
    let dir = tempfile::tempdir().unwrap();
    // The default keymap is deliverable by every terminal (USER-5): no notice.
    let (mut app, _) = test_app(dir.path());
    app.report_startup(Vec::new(), false);
    assert!(app.notices.is_empty(), "{:?}", notices(&app));
    // A user key that needs the kitty protocol is listed, with the palette as the way out.
    let app = legacy_notice_app(dir.path());
    let all = format!(
        "{}\n{}",
        notices(&app).join("\n"),
        app.notices[0].detail.as_deref().unwrap_or("")
    );
    assert!(all.contains("ctrl+shift+m"), "{all}");
    assert!(all.contains("(f1)"), "palette key: {all}");
    assert!(!all.contains("ctrl+shift+p"), "has f1: {all}");
    assert!(!all.contains("(app.palette)"), "key names only: {all}");
    let (mut app, _) = test_app(dir.path());
    app.report_startup(Vec::new(), true);
    assert!(app.notices.is_empty(), "{:?}", notices(&app));
}

/// An app whose user keymap binds keys a legacy terminal cannot send: one that is the only key
/// of its command, and one whose command also has a reachable key. The startup notice is up.
pub(crate) fn legacy_notice_app(root: &Path) -> App {
    let km = root.join("keymap.toml");
    std::fs::write(
        &km,
        "\"ctrl+shift+m\" = \"file.save_overwrite\"\n\"ctrl+shift+p\" = \"app.palette\"\n",
    )
    .unwrap();
    let (keymap, warnings) = Keymap::load(Some(&km));
    let mut app = App::new(
        root.to_path_buf(),
        keymap,
        Settings::default(),
        Box::new(MemoryClipboard::new()),
    );
    app.report_startup(warnings, false);
    app
}

// ---- fix round 1 -------------------------------------------------------------------------

#[test]
fn paste_uses_the_document_line_ending() {
    let (_d, _p, mut app, _) = app_with_file("x\n");
    // VTE sends bracketed paste lines with CR; Windows clipboards use CRLF.
    app.handle_paste("a\rb\r\nc\n");
    assert_eq!(text(&app), "a\nb\nc\nx\n");
    let (_d, _p, mut app, clip) = app_with_file("x\r\ny\r\n");
    use crate::clipboard::Clipboard;
    clip.clone().set("1\n2\r3").unwrap();
    app.handle_key(ctrl('v'));
    assert_eq!(text(&app), "1\r\n2\r\n3x\r\ny\r\n");
}

#[test]
fn rescan_renamed_and_error_events() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.txt");
    let b = dir.path().join("b.txt");
    std::fs::write(&a, "a1\n").unwrap();
    std::fs::write(&b, "b1\n").unwrap();
    let (mut app, _) = test_app(dir.path());
    app.open_path(&a).unwrap();
    app.open_path(&b).unwrap();
    space_i(&mut app);
    typ(&mut app, "mine ");
    std::fs::write(&a, "a2\n").unwrap();
    std::fs::write(&b, "b2\n").unwrap();
    app.handle_fs_event(vr_core::FsEvent::Rescan(dir.path().to_path_buf()));
    assert_eq!(
        app.docs[0].buffer.text().to_string(),
        "a2\n",
        "unmodified reloads"
    );
    assert_eq!(app.docs[1].buffer.text().to_string(), "mine b1\n");
    assert_eq!(app.docs[1].disk_state, vr_core::DiskState::ChangedOnDisk);

    // An atomic save by another tool: temp file renamed over ours.
    let tmp = dir.path().join(".a.txt.tmp");
    std::fs::write(&tmp, "a3\n").unwrap();
    std::fs::rename(&tmp, &a).unwrap();
    app.handle_fs_event(vr_core::FsEvent::Renamed(tmp, a.clone()));
    assert_eq!(app.docs[0].buffer.text().to_string(), "a3\n");

    // Renamed away: the buffer stays, marked deleted.
    let moved = dir.path().join("moved.txt");
    std::fs::rename(&a, &moved).unwrap();
    app.handle_fs_event(vr_core::FsEvent::Renamed(a.clone(), moved));
    assert_eq!(app.docs[0].disk_state, vr_core::DiskState::DeletedOnDisk);
    assert_eq!(app.docs[0].buffer.text().to_string(), "a3\n");

    app.notices.clear();
    app.handle_fs_event(vr_core::FsEvent::Error("inotify watch limit".into()));
    let n = notices(&app).join("\n");
    assert!(
        n.contains("inotify watch limit") && n.contains("missed"),
        "{n}"
    );
}

#[test]
fn dotdot_path_opens_the_same_tab() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("a.txt"), "a").unwrap();
    let (mut app, _) = test_app(dir.path());
    app.open_path(Path::new("a.txt")).unwrap();
    app.open_path(Path::new("sub/../a.txt")).unwrap();
    app.open_path(Path::new("./a.txt")).unwrap();
    app.open_path(&dir.path().join("sub/./../a.txt")).unwrap();
    assert_eq!(app.docs.len(), 1);
    assert_eq!(app.active_name(), "a.txt");
}

#[test]
fn legacy_keyboard_notice_does_not_hide_later_notices() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = legacy_notice_app(dir.path());
    assert!(!app.notices.is_empty());
    app.warn("a.txt changed on disk; not saved");
    assert_eq!(
        app.notices.front().unwrap().text,
        "a.txt changed on disk; not saved"
    );
    // Alone, it goes away by itself.
    let mut app = legacy_notice_app(dir.path());
    app.tick(std::time::Instant::now() + std::time::Duration::from_secs(60));
    assert!(app.notices.is_empty(), "{:?}", notices(&app));
}

#[cfg(target_os = "linux")]
#[test]
fn monitor_samples_only_while_no_file_is_open() {
    use std::time::{Duration, Instant};
    let (_d, path, mut app, _) = app_with_file("x\n");
    let t0 = Instant::now();
    app.tick(t0);
    assert_eq!(app.monitor.samples_taken, 0);
    assert!(!app.monitor.running());
    app.close_doc(0);
    app.dirty = false;
    app.tick(t0 + Duration::from_secs(1));
    assert_eq!(app.monitor.samples_taken, 1);
    assert!(app.dirty, "a new reading needs a redraw");
    app.open_path(&path).unwrap();
    app.tick(t0 + Duration::from_secs(3));
    assert_eq!(app.monitor.samples_taken, 1, "no sampling behind a file");
    assert!(!app.monitor.running());
}

#[test]
fn recent_files_are_most_recent_first_unique_and_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let (mut app, _) = test_app(dir.path());
    assert!(app.recent.is_empty());
    let paths: Vec<PathBuf> = (0..10)
        .map(|i| {
            let p = dir.path().join(format!("f{i}.txt"));
            std::fs::write(&p, "x").unwrap();
            p
        })
        .collect();
    for p in &paths {
        app.open_path(p).unwrap();
    }
    assert_eq!(app.recent.len(), crate::app::RECENT_MAX);
    assert_eq!(app.recent[0], paths[9]);
    // Re-opening (activating) an older one moves it to the front once.
    app.open_path(&paths[5]).unwrap();
    assert_eq!(app.recent[0], paths[5]);
    assert_eq!(app.recent.iter().filter(|p| **p == paths[5]).count(), 1);
    // A path that cannot be opened is not remembered.
    assert!(app.open_path(&dir.path().join("missing.txt")).is_err());
    assert!(!app.recent.iter().any(|p| p.ends_with("missing.txt")));
}

/// USER-5 / D261: the default keymap is one separate key set. One key per command (no
/// exception), every key a single key deliverable by a legacy terminal, none taken by GNOME
/// Terminal or the desktop, none that shares a byte with another key (except ctrl+h, approved
/// by the user: GNOME Terminal's Backspace sends 0x7f, ctrl+h 0x08), no alt and no ctrl+shift
/// anywhere; F keys alone or with ctrl or shift. The other approved exception, the `space`
/// prefix in read mode, is built in and not in the keymap.
#[test]
fn default_keymap_obeys_user_5() {
    use crate::event_loop::legacy_unreachable;
    use std::collections::BTreeMap;
    use vr_config::{KeyChord, KeyCode as C};
    const FORBIDDEN: [&str; 14] = [
        "ctrl+pageup",
        "ctrl+pagedown",
        "ctrl+plus",
        "ctrl+-",
        "ctrl+0",
        "f10",
        "f11",
        "alt+f1",
        "alt+f2",
        "alt+f4",
        "alt+f6",
        "alt+f7",
        "alt+f8",
        "alt+space",
    ];
    // Share a byte with Tab, Enter, Escape, Enter (ctrl+j) or NUL. ctrl+h is the approved
    // exception (T21).
    const AMBIGUOUS: [&str; 7] = [
        "ctrl+m",
        "ctrl+[",
        "ctrl+i",
        "ctrl+j",
        "ctrl+2",
        "ctrl+space",
        "ctrl+@",
    ];
    let check_first = |k: &KeyChord, what: &str| {
        let s = k.to_string();
        assert!(!k.mods.alt, "{s} ({what}) uses alt");
        assert!(
            !legacy_unreachable(k),
            "{s} ({what}) is not legacy-deliverable"
        );
        assert!(
            !FORBIDDEN.contains(&s.as_str()),
            "{s} ({what}) is forbidden"
        );
        assert!(
            !AMBIGUOUS.contains(&s.as_str()),
            "{s} ({what}) is ambiguous"
        );
        assert!(
            !(k.mods.ctrl && k.mods.shift),
            "{s} ({what}) is a ctrl+shift chord"
        );
        if let C::F(n) = k.code {
            assert!(n != 10 && n != 11, "{s} ({what}): F10/F11 belong to GNOME");
        }
    };
    let dir = tempfile::tempdir().unwrap();
    let (app, _) = test_app(dir.path());
    let mut per_cmd: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (_, k, cmd) in app.keymap.bindings() {
        check_first(k, cmd);
        let keys = per_cmd.entry(cmd.to_string()).or_default();
        if !keys.contains(&k.to_string()) {
            keys.push(k.to_string());
        }
    }
    for (cmd, keys) in &per_cmd {
        assert_eq!(keys.len(), 1, "{cmd} has {keys:?}");
    }
}

/// Keyset v2 (T21, D261): every default key, (context, key, command); each entry is the
/// command's one key. "editor" keys run in the editor, "global" ones anywhere a panel does not
/// bind the key, "explorer" ones with the explorer focused.
pub(crate) const KEYSET_V2: &[(&str, &str, &str)] = &[
    ("editor", "ctrl+z", "edit.undo"),
    ("editor", "ctrl+y", "edit.redo"),
    ("editor", "ctrl+x", "edit.cut"),
    ("editor", "ctrl+v", "edit.paste"),
    ("editor", "ctrl+d", "edit.delete_line"),
    ("editor", "ctrl+l", "edit.duplicate_line"),
    ("editor", "ctrl+up", "edit.move_line_up"),
    ("editor", "ctrl+down", "edit.move_line_down"),
    ("editor", "ctrl+/", "edit.toggle_comment"),
    ("editor", "shift+tab", "edit.outdent"),
    ("editor", "escape", "editor.read_mode"),
    ("editor", "ctrl+a", "edit.select_all"),
    ("editor", "ctrl+c", "edit.copy"),
    ("editor", "ctrl+f", "find.open"),
    ("editor", "f4", "find.next"),
    ("editor", "shift+f4", "find.prev"),
    ("editor", "ctrl+r", "find.replace_open"),
    ("editor", "ctrl+g", "nav.goto_line"),
    ("global", "ctrl+r", "search.replace_project"),
    ("global", "ctrl+s", "file.save"),
    ("global", "ctrl+o", "file.open"),
    ("global", "ctrl+p", "file.goto"),
    ("global", "ctrl+n", "explorer.new_file"),
    ("global", "ctrl+w", "file.close"),
    ("global", "f9", "tab.next"),
    ("global", "shift+f9", "tab.prev"),
    ("global", "ctrl+e", "explorer.toggle_focus"),
    ("global", "ctrl+b", "explorer.focus"),
    ("global", "ctrl+k", "explorer.reveal"),
    ("global", "ctrl+h", "explorer.toggle_hidden"),
    ("global", "ctrl+f6", "explorer.toggle_ignored"),
    ("global", "ctrl+`", "terminal.toggle"),
    ("global", "shift+f12", "terminal.hide"),
    ("global", "f8", "terminal.next"),
    ("global", "shift+f8", "terminal.prev"),
    ("global", "ctrl+f8", "terminal.grow"),
    ("global", "ctrl+f9", "terminal.shrink"),
    ("global", "ctrl+f7", "terminal.toggle_position"),
    ("global", "shift+f2", "editor.toggle_blame"),
    ("global", "f1", "app.palette"),
    ("global", "ctrl+q", "app.quit"),
    ("explorer", "f2", "explorer.rename"),
    ("explorer", "ctrl+n", "explorer.new"),
    ("explorer", "delete", "explorer.delete"),
    ("explorer", "ctrl+c", "explorer.copy"),
    ("explorer", "ctrl+x", "explorer.cut"),
    ("explorer", "ctrl+v", "explorer.paste"),
];

/// Keys the spec removed (T21; ctrl+f2, f12 and f5 by the user's later changes; f3 / shift+f3,
/// ctrl+f4 and ctrl+f12 by D279 / D280). ctrl+f7, kept free for T19, is the panel position key.
pub(crate) const REMOVED_V2: &[&str] = &[
    "ctrl+i", "ctrl+]", "ctrl+u", "ctrl+t", "shift+f3", "f6", "ctrl+f1", "ctrl+f2", "ctrl+f3",
    "ctrl+f5", "f5", "ctrl+f4",
];

/// The bytes GNOME Terminal (VTE) sends for `k` without the kitty protocol, and the event
/// crossterm 0.29 parses from them (event/sys/unix/parse.rs): C0 bytes 0x01..=0x1a are
/// ctrl+letter except 0x09 (Tab) and 0x0d (Enter), 0x1c..=0x1f are ctrl+4..7, NUL is
/// ctrl+space, 0x7f is Backspace; escape sequences (F keys, arrows, delete) carry their
/// modifiers as the `key_event` helper builds them, except a CSI sequence ending in `R` with
/// parameters (shift+f3 = ESC [ 1 ; 2 R), which crossterm reads as a cursor position report:
/// no key reaches the app (`None`).
pub(crate) fn legacy_event(k: &str) -> (Vec<u8>, Option<KeyEvent>) {
    let plain = KeyModifiers::NONE;
    let ctrl_byte = |b: u8| match b {
        0x00 => key(KeyCode::Char(' '), KeyModifiers::CONTROL),
        0x09 => key(KeyCode::Tab, plain),
        0x0d => key(KeyCode::Enter, plain),
        0x1b => key(KeyCode::Esc, plain),
        0x7f => key(KeyCode::Backspace, plain),
        0x01..=0x1a => key(KeyCode::Char((b - 1 + b'a') as char), KeyModifiers::CONTROL),
        0x1c..=0x1f => key(
            KeyCode::Char((b - 0x1c + b'4') as char),
            KeyModifiers::CONTROL,
        ),
        _ => key(KeyCode::Char(b as char), plain),
    };
    let byte = match k {
        "ctrl+`" | "ctrl+space" => Some(0x00),
        "ctrl+/" => Some(0x1f),
        "escape" => Some(0x1b),
        "backspace" => Some(0x7f),
        "tab" => Some(0x09),
        "space" => Some(b' '),
        _ => k
            .strip_prefix("ctrl+")
            .filter(|c| c.len() == 1 && c.as_bytes()[0].is_ascii_lowercase())
            .map(|c| c.as_bytes()[0] - b'a' + 1),
    };
    if let Some(b) = byte {
        return (vec![b], Some(ctrl_byte(b)));
    }
    if k.len() == 1 {
        return (k.as_bytes().to_vec(), Some(ctrl_byte(k.as_bytes()[0])));
    }
    let seq = match k {
        "shift+tab" => "\x1b[Z",
        "delete" => "\x1b[3~",
        "ctrl+up" => "\x1b[1;5A",
        "ctrl+down" => "\x1b[1;5B",
        "f1" => "\x1bOP",
        "f2" => "\x1bOQ",
        "shift+f2" => "\x1b[1;2Q",
        "f3" => "\x1bOR",
        "shift+f3" => "\x1b[1;2R",
        "f4" => "\x1bOS",
        "shift+f4" => "\x1b[1;2S",
        "f8" => "\x1b[19~",
        "shift+f8" => "\x1b[19;2~",
        "ctrl+f8" => "\x1b[19;5~",
        "f9" => "\x1b[20~",
        "shift+f9" => "\x1b[20;2~",
        "ctrl+f9" => "\x1b[20;5~",
        "ctrl+f4" => "\x1b[1;5S",
        "ctrl+f6" => "\x1b[17;5~",
        "ctrl+f7" => "\x1b[18;5~",
        "ctrl+f12" => "\x1b[24;5~",
        "shift+f12" => "\x1b[24;2~",
        other => panic!("no legacy bytes for {other}"),
    };
    let cursor_report = seq.starts_with("\x1b[") && seq.ends_with('R');
    (
        seq.as_bytes().to_vec(),
        (!cursor_report).then(|| key_event(k)),
    )
}

/// The event a terminal with the kitty protocol reports for `k`.
pub(crate) fn kitty_event(k: &str) -> KeyEvent {
    match k {
        "ctrl+/" => key(KeyCode::Char('/'), KeyModifiers::CONTROL),
        "ctrl+`" => key(KeyCode::Char('`'), KeyModifiers::CONTROL),
        "escape" => key(KeyCode::Esc, KeyModifiers::NONE),
        _ => key_event(k),
    }
}

/// AC-21a: each key of the T21 table runs its command, as GNOME Terminal sends it without the
/// kitty protocol and as the kitty protocol reports it: editor keys in edit and in read mode
/// (an edit key in read mode runs its command, which refuses with the hint), global keys from
/// the editor and from the explorer (unless the explorer binds the key), explorer keys with
/// the explorer focused.
#[test]
fn every_keyset_v2_key_runs_its_command() {
    let binds = |ctx: &str, k: &str| KEYSET_V2.iter().any(|&(c, key, _)| c == ctx && key == k);
    let mut lost = Vec::new();
    for enhanced in [false, true] {
        for &(ctx, k, cmd) in KEYSET_V2 {
            // A global key runs wherever the focused part does not bind it; the search panel has
            // no table of its own (AC-21c: ctrl+r there is the project replace).
            let places: Vec<&str> = match ctx {
                "explorer" => vec!["explorer"],
                "editor" => vec!["edit", "read"],
                _ => ["edit", "read", "explorer", "search"]
                    .into_iter()
                    .filter(|p| match *p {
                        "explorer" => !binds("explorer", k),
                        "search" => true,
                        _ => !binds("editor", k),
                    })
                    .collect(),
            };
            for place in places {
                let (_d, _p, mut app, _) = app_with_file("one\ntwo\n");
                app.terminal.shell = Some(PathBuf::from("/bin/sh"));
                app.terminal.env = vec![("ENV".into(), "/dev/null".into())];
                app.keyboard_enhanced = enhanced;
                match place {
                    "explorer" => {
                        app.explorer.visible = true;
                        app.focus = crate::app::Focus::Explorer;
                    }
                    "read" => {
                        app.run_command("editor.read_mode", CmdArgs::None);
                    }
                    "search" => {
                        app.run_command("search.project", CmdArgs::None);
                        assert_eq!(app.focus, crate::app::Focus::Search);
                    }
                    _ => {}
                }
                app.ran.clear();
                let (bytes, ev) = if enhanced {
                    (Vec::new(), Some(kitty_event(k)))
                } else {
                    legacy_event(k)
                };
                let Some(ev) = ev else {
                    lost.push(format!("{k} in {place}"));
                    continue;
                };
                app.handle_key(ev);
                assert_eq!(
                    app.ran.first().map(String::as_str),
                    Some(cmd),
                    "{ctx} {k} in {place} (kitty {enhanced}, bytes {bytes:02x?}): ran {:?}",
                    app.ran
                );
            }
        }
    }
    // D264 closed by D279: find.prev moved from shift+f3 (ESC [ 1 ; 2 R, read by crossterm 0.29
    // as a cursor position report) to shift+f4 (ESC [ 1 ; 2 S), which arrives: no key is lost.
    assert!(lost.is_empty(), "{lost:?}");
}

/// D279: shift+f4 arrives from a terminal without the kitty protocol (ESC [ 1 ; 2 S) and steps
/// back through the matches; shift+f3 (ESC [ 1 ; 2 R) is still read as a cursor report and is
/// bound to nothing anyway.
#[test]
fn legacy_shift_f4_reaches_find_prev() {
    let (bytes, ev) = legacy_event("shift+f4");
    assert_eq!(bytes, b"\x1b[1;2S");
    let ev = ev.expect("shift+f4 is a key event");
    assert_eq!(ev, key_event("shift+f4"));
    assert!(legacy_event("shift+f3").1.is_none());
    let (_d, _p, mut app, _) = app_with_file("ab ab ab\n");
    app.keyboard_enhanced = false;
    app.run_command("find.open", CmdArgs::None);
    app.handle_paste("ab");
    app.ran.clear();
    app.handle_key(legacy_event("f4").1.unwrap());
    app.handle_key(ev);
    assert_eq!(app.ran, ["find.next", "find.prev"]);
}

/// AC-21d: the removed keys run nothing, in either mode and in the explorer.
#[test]
fn removed_keys_run_nothing() {
    for k in REMOVED_V2 {
        for place in ["edit", "read", "explorer"] {
            let (_d, _p, mut app, _) = app_with_file("one\ntwo\n");
            app.keyboard_enhanced = true;
            match place {
                "explorer" => {
                    app.explorer.visible = true;
                    app.focus = crate::app::Focus::Explorer;
                }
                "read" => {
                    app.run_command("editor.read_mode", CmdArgs::None);
                }
                _ => {}
            }
            app.ran.clear();
            app.handle_key(key_event(k));
            assert!(app.ran.is_empty(), "{k} in {place} ran {:?}", app.ran);
        }
    }
}

/// Legacy ctrl+h (0x08) toggles hidden files; Backspace (0x7f) deletes, in edit mode.
#[test]
fn legacy_ctrl_h_and_backspace_are_different_keys() {
    let (_d, _p, mut app, _) = app_with_file("ab\n");
    app.keyboard_enhanced = false;
    app.handle_key(key(KeyCode::End, KeyModifiers::NONE));
    let hidden = app.explorer.tree.show_hidden();
    let (bytes, ev) = legacy_event("ctrl+h");
    assert_eq!(bytes, [0x08]);
    app.handle_key(ev.unwrap());
    assert_ne!(
        app.explorer.tree.show_hidden(),
        hidden,
        "0x08 toggles hidden files"
    );
    assert_eq!(text(&app), "ab\n", "0x08 deletes nothing");
    let (bytes, ev) = legacy_event("backspace");
    assert_eq!(bytes, [0x7f]);
    app.handle_key(ev.unwrap());
    assert_eq!(text(&app), "a\n", "0x7f deletes");
}

/// T21: every place shows a command's one key; the read-mode built-ins show theirs.
#[test]
fn display_key_is_the_one_key_of_a_command() {
    let dir = tempfile::tempdir().unwrap();
    let (mut app, _) = test_app(dir.path());
    let mut cases: Vec<(&str, &str)> = KEYSET_V2.iter().map(|&(_, k, c)| (c, k)).collect();
    cases.extend([
        ("editor.edit_mode", "space i"),
        ("focus.cycle", "tab"),
        ("edit.indent", "tab"),
    ]);
    for enhanced in [true, false] {
        app.keyboard_enhanced = enhanced;
        for (cmd, key) in &cases {
            assert_eq!(
                app.display_key(cmd).as_deref(),
                Some(*key),
                "{cmd} {enhanced}"
            );
        }
    }
    assert_eq!(app.display_key("explorer.toggle"), None, "palette only");
    assert_eq!(app.display_key("no.such"), None);
    assert_eq!(app.key_hint("no.such"), "run no.such");
}
