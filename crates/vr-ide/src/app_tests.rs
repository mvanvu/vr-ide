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

/// A temp project with one file, opened in the app and put in edit mode with ctrl+i (D145),
/// the state the editing tests start from; `mode_tests` covers read mode.
pub(crate) fn app_with_file(content: &str) -> (tempfile::TempDir, PathBuf, App, MemoryClipboard) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.txt");
    std::fs::write(&path, content).unwrap();
    let (mut app, clip) = test_app(dir.path());
    app.open_path(&path).unwrap();
    app.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL));
    (dir, path, app, clip)
}

fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
    KeyEvent::new(code, mods)
}

fn ctrl(c: char) -> KeyEvent {
    key(KeyCode::Char(c), KeyModifiers::CONTROL)
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
    // ctrl+shift+z as an enhanced terminal reports it (uppercase letter).
    app.handle_key(key(
        KeyCode::Char('Z'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    assert_eq!(text(&app), "ab\none\n");
}

#[test]
fn unknown_command_in_keymap_shows_notice_not_crash() {
    let dir = tempfile::tempdir().unwrap();
    let km = dir.path().join("keymap.toml");
    std::fs::write(&km, "ctrl+s = \"file.save\"\n\"ctrl+k\" = \"nope.cmd\"\n").unwrap();
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
    app.handle_key(ctrl('i'));
    app.handle_key(ctrl('k'));
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
    app.handle_key(key(KeyCode::PageDown, KeyModifiers::CONTROL));
    assert_eq!(app.active, 0);
    app.handle_key(key(KeyCode::PageUp, KeyModifiers::CONTROL));
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

#[test]
fn find_without_a_file_shows_a_notice() {
    let dir = tempfile::tempdir().unwrap();
    let (mut app, _) = test_app(dir.path());
    app.handle_key(ctrl('f'));
    assert!(!app.find.open);
    assert!(notices(&app).iter().any(|n| n.contains("no file is open")));
    // Escape dismisses the oldest notice.
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
fn legacy_terminal_notice_lists_unreachable_default_keys() {
    let dir = tempfile::tempdir().unwrap();
    let (mut app, _) = test_app(dir.path());
    app.report_startup(Vec::new(), false);
    let all = format!(
        "{}\n{}",
        notices(&app).join("\n"),
        app.notices[0].detail.as_deref().unwrap_or("")
    );
    // Chords with a reachable key (f1, shift+tab, ctrl+alt+<key>) are not lost.
    assert!(all.contains("ctrl+alt+enter"), "{all}");
    assert!(
        !all.contains("ctrl+shift+p") && !all.contains("ctrl+["),
        "{all}"
    );
    assert!(!all.contains("(app.palette)"), "key names only: {all}");
    assert!(all.contains("f1"), "palette fallback: {all}");
    assert!(!all.contains("ctrl+s (file.save)"), "{all}");
    let (mut app, _) = test_app(dir.path());
    app.report_startup(Vec::new(), true);
    assert!(app.notices.is_empty(), "{:?}", notices(&app));
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
    app.handle_key(ctrl('i'));
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
    let (mut app, _) = test_app(dir.path());
    app.report_startup(Vec::new(), false);
    assert!(!app.notices.is_empty());
    app.warn("a.txt changed on disk; not saved");
    assert_eq!(
        app.notices.front().unwrap().text,
        "a.txt changed on disk; not saved"
    );
    // Alone, it goes away by itself.
    let (mut app, _) = test_app(dir.path());
    app.report_startup(Vec::new(), false);
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

/// D220 / D221: every chord a legacy terminal cannot send or an emulator keeps has a
/// ctrl+alt alternative. The legacy byte form ESC + ctrl-letter is reported by crossterm as
/// the very same event (letter + CONTROL + ALT), so one event covers both.
#[test]
fn ctrl_alt_alternatives_run_the_same_commands() {
    use crate::event_loop::{chord_from_key, legacy_unreachable};
    let dir = tempfile::tempdir().unwrap();
    let (app, _) = test_app(dir.path());
    let ca = KeyModifiers::CONTROL | KeyModifiers::ALT;
    let cases = [
        ('f', "search.project", "ctrl+shift+f"),
        ('h', "search.replace_project", "ctrl+shift+h"),
        ('s', "file.save_as", "ctrl+shift+s"),
        ('e', "explorer.focus", "ctrl+shift+e"),
        ('k', "edit.delete_line", "ctrl+shift+k"),
        ('`', "terminal.new", "ctrl+shift+`"),
        ('1', "find.replace_one", "ctrl+shift+1"),
    ];
    for (c, cmd, original) in cases {
        for enhanced in [true, false] {
            let ev = KeyEvent::new(KeyCode::Char(c), ca);
            let chord = chord_from_key(&ev, enhanced).unwrap();
            assert_eq!(app.keymap.command_for(&chord), Some(cmd), "{chord}");
        }
        // The original chord stays, and the command now has two keys.
        let keys = app.keymap.keys_for(cmd);
        assert!(keys.iter().any(|k| k.to_string() == original), "{cmd}");
        assert_eq!(keys.len(), 2, "{cmd}: {keys:?}");
        // Letters are reachable on a legacy keyboard; ctrl+alt+1 and ctrl+alt+` are not (a
        // legacy terminal sends ESC 1 and ESC NUL).
        let reachable = c.is_ascii_lowercase();
        let k = keys
            .iter()
            .find(|k| k.to_string() == format!("ctrl+alt+{c}"));
        assert_eq!(!legacy_unreachable(k.unwrap()), reachable, "{c}");
    }
}

/// The legacy notice lists only the chords that stay unreachable once their alternative exists.
#[test]
fn legacy_notice_lists_only_chords_without_a_reachable_alternative() {
    let dir = tempfile::tempdir().unwrap();
    let (mut app, _) = test_app(dir.path());
    app.report_startup(Vec::new(), false);
    let detail = app.notices[0].detail.clone().unwrap();
    for gone in [
        "ctrl+shift+f",
        "ctrl+shift+h",
        "ctrl+shift+s",
        "ctrl+shift+e",
        "ctrl+shift+k",
    ] {
        assert!(!detail.contains(gone), "{gone} has ctrl+alt: {detail}");
    }
    for kept in ["ctrl+shift+1", "ctrl+shift+`", "ctrl+alt+enter"] {
        assert!(detail.contains(kept), "{kept}: {detail}");
    }
}
