//! T6: palette, dialogs, go-to-line, external-change prompt and the quit guard, driven
//! through the `App` API (no terminal).

use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, Focus};
use crate::app_tests::{app_with_file, test_app};
use crate::commands::CmdArgs;
use crate::commands::dialog::DialogAction;
use crate::ui::dialog::DialogWidget;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

fn typ(app: &mut App, s: &str) {
    for c in s.chars() {
        app.handle_key(key(KeyCode::Char(c)));
    }
}

fn text(app: &App) -> String {
    app.active_doc().unwrap().buffer.text().to_string()
}

fn notices(app: &App) -> String {
    app.notices
        .iter()
        .map(|n| n.text.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn palette_filters_by_fuzzy_name_and_shows_keys() {
    let (mut app, _) = test_app(Path::new("/p"));
    // Opened by its keymap keys (ctrl+shift+p, f1 by default), never a hard-coded one.
    app.handle_key(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));
    assert_eq!(app.focus, Focus::Palette);
    let all = app.palette.as_ref().unwrap().items.len();
    assert!(all > 10, "{all}");
    typ(&mut app, "sav");
    let p = app.palette.as_ref().unwrap();
    assert!(p.items.len() < all);
    let save = p.items.iter().find(|i| i.name == "file.save").unwrap();
    assert_eq!(save.keys, "ctrl+s");
    assert!(
        p.items.iter().all(|i| i.name != "edit.undo"),
        "{:?}",
        p.items
    );
    // Matched by command name as well as title.
    let p = app.palette.as_mut().unwrap();
    p.input.set("nav.goto");
    crate::commands::palette::refilter(&mut app);
    assert_eq!(app.palette.as_ref().unwrap().items[0].name, "nav.goto_line");
    // The search commands are listed by their names.
    app.palette
        .as_mut()
        .unwrap()
        .input
        .set("search.replace_project");
    crate::commands::palette::refilter(&mut app);
    assert_eq!(
        app.palette.as_ref().unwrap().items[0].name,
        "search.replace_project"
    );
}

#[test]
fn palette_enter_runs_command() {
    let (_d, path, mut app, _) = app_with_file("hello\n");
    typ(&mut app, "x");
    app.run_command("app.palette", CmdArgs::None);
    typ(&mut app, "save");
    // "Save" ranks above "Save As" for "save".
    assert_eq!(app.palette.as_ref().unwrap().items[0].name, "file.save");
    app.handle_key(key(KeyCode::Enter));
    assert!(app.palette.is_none());
    assert_eq!(app.focus, Focus::Editor);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "xhello\n");
    // Up/Down pick another row; Enter runs that one.
    app.run_command("app.palette", CmdArgs::None);
    typ(&mut app, "line");
    let names: Vec<&str> = app
        .palette
        .as_ref()
        .unwrap()
        .items
        .iter()
        .map(|i| i.name)
        .collect();
    let dup = names
        .iter()
        .position(|n| *n == "edit.duplicate_line")
        .unwrap();
    for _ in 0..dup {
        app.handle_key(key(KeyCode::Down));
    }
    app.handle_key(key(KeyCode::Up));
    app.handle_key(key(KeyCode::Down));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(text(&app), "xhello\nxhello\n");
}

#[test]
fn palette_escape_closes() {
    let (_d, _p, mut app, _) = app_with_file("a\n");
    app.handle_key(key(KeyCode::F(1)));
    assert!(app.palette.is_some(), "f1 opens the palette");
    typ(&mut app, "zz");
    app.handle_key(key(KeyCode::Esc));
    assert!(app.palette.is_none());
    assert_eq!(app.focus, Focus::Editor);
    assert_eq!(
        text(&app),
        "a\n",
        "typing went to the palette, not the buffer"
    );
}

fn prompt_text(app: &App) -> Option<String> {
    match &app.dialog.as_ref()?.widget {
        DialogWidget::Prompt(p) => Some(p.input.text().to_string()),
        DialogWidget::Confirm(_) => None,
    }
}

fn submit(app: &mut App, s: &str) {
    for _ in 0..prompt_text(app).unwrap().chars().count() {
        app.handle_key(key(KeyCode::Backspace));
    }
    typ(app, s);
    app.handle_key(key(KeyCode::Enter));
}

fn cursor(app: &App) -> vr_core::Pos {
    let doc = app.active_doc().unwrap();
    vr_core::text::char_to_pos(doc.buffer.text(), doc.buffer.selection().primary().head)
}

#[test]
fn goto_line_moves_cursor_and_clamps() {
    let (_d, _p, mut app, _) = app_with_file("one\ntwo\nthree\n");
    app.handle_key(ctrl('g'));
    assert!(matches!(
        app.dialog.as_ref().map(|d| &d.action),
        Some(DialogAction::GotoLine)
    ));
    submit(&mut app, "2");
    assert!(app.dialog.is_none());
    assert_eq!(cursor(&app), vr_core::Pos { line: 1, col: 0 });
    app.handle_key(ctrl('g'));
    submit(&mut app, "3:4");
    assert_eq!(cursor(&app), vr_core::Pos { line: 2, col: 3 });
    // Clamped to the last line and to the line's end.
    app.handle_key(ctrl('g'));
    submit(&mut app, "99:99");
    assert_eq!(cursor(&app), vr_core::Pos { line: 3, col: 0 });
    app.handle_key(ctrl('g'));
    submit(&mut app, "1:99");
    assert_eq!(cursor(&app), vr_core::Pos { line: 0, col: 3 });
    // Not a number: the prompt stays with an error.
    app.handle_key(ctrl('g'));
    submit(&mut app, "abc");
    match &app.dialog.as_ref().unwrap().widget {
        DialogWidget::Prompt(p) => assert!(p.error.is_some()),
        DialogWidget::Confirm(_) => panic!("prompt expected"),
    }
    app.handle_key(key(KeyCode::Esc));
    assert!(app.dialog.is_none());
    assert_eq!(cursor(&app), vr_core::Pos { line: 0, col: 3 });
}

fn confirm_buttons(app: &App) -> Vec<String> {
    match &app.dialog.as_ref().expect("a dialog").widget {
        DialogWidget::Confirm(c) => c.buttons.clone(),
        DialogWidget::Prompt(_) => panic!("confirm dialog expected"),
    }
}

/// Chooses the button labelled `label` with the keyboard.
fn choose(app: &mut App, label: &str) {
    let i = confirm_buttons(app)
        .iter()
        .position(|b| b == label)
        .unwrap_or_else(|| panic!("no {label:?} in {:?}", confirm_buttons(app)));
    for _ in 0..8 {
        app.handle_key(key(KeyCode::Left));
    }
    let selected = match &app.dialog.as_ref().unwrap().widget {
        DialogWidget::Confirm(c) => c.selected,
        DialogWidget::Prompt(_) => unreachable!(),
    };
    let n = confirm_buttons(app).len();
    for _ in 0..(i + n - selected) % n {
        app.handle_key(key(KeyCode::Right));
    }
    app.handle_key(key(KeyCode::Enter));
}

fn two_modified() -> (tempfile::TempDir, App) {
    let (d, _p, mut app, _) = app_with_file("a\n");
    std::fs::write(d.path().join("b.txt"), "b\n").unwrap();
    typ(&mut app, "1");
    app.open_path(Path::new("b.txt")).unwrap();
    crate::app_tests::space_i(&mut app);
    typ(&mut app, "2");
    (d, app)
}

#[test]
fn quit_with_unsaved_asks() {
    // Cancel: nothing happens.
    let (d, mut app) = two_modified();
    app.handle_key(ctrl('q'));
    assert!(!app.quit);
    assert_eq!(confirm_buttons(&app), ["Save all", "Discard", "Cancel"]);
    choose(&mut app, "Cancel");
    assert!(!app.quit && app.dialog.is_none());
    // Esc is Cancel too; a held ctrl+q does not answer the dialog.
    app.handle_key(ctrl('q'));
    app.handle_key(ctrl('q'));
    assert!(!app.quit);
    app.handle_key(key(KeyCode::Esc));
    assert!(!app.quit && app.dialog.is_none());

    // Save all writes every modified file, then quits.
    app.handle_key(ctrl('q'));
    choose(&mut app, "Save all");
    assert!(app.quit);
    assert_eq!(
        std::fs::read_to_string(d.path().join("a.txt")).unwrap(),
        "1a\n"
    );
    assert_eq!(
        std::fs::read_to_string(d.path().join("b.txt")).unwrap(),
        "2b\n"
    );

    // Discard keeps the buffers in recovery and says where after the terminal is restored.
    let state = tempfile::tempdir().unwrap();
    let (d, mut app) = two_modified();
    app.recovery_dir = Some(state.path().join("recovery"));
    app.handle_key(ctrl('q'));
    choose(&mut app, "Discard");
    assert!(app.quit);
    assert_eq!(
        std::fs::read_to_string(d.path().join("a.txt")).unwrap(),
        "a\n"
    );
    let kept = std::fs::read_dir(state.path().join("recovery"))
        .unwrap()
        .count();
    assert_eq!(kept, 2);
    let msg = app.exit_messages.join("\n");
    assert!(
        msg.contains(&state.path().join("recovery").display().to_string()),
        "{msg}"
    );

    // Recovery cannot be written: the app does not quit and shows the error.
    let (_d, mut app) = two_modified();
    let blocker = tempfile::NamedTempFile::new().unwrap();
    app.recovery_dir = Some(blocker.path().join("recovery"));
    app.handle_key(ctrl('q'));
    choose(&mut app, "Discard");
    assert!(!app.quit);
    assert_eq!(app.docs.len(), 2);
    assert!(
        notices(&app).contains("nothing was discarded"),
        "{}",
        notices(&app)
    );

    // Nothing unsaved: quit at once.
    let (_d, _p, mut app, _) = app_with_file("a\n");
    app.handle_key(ctrl('q'));
    assert!(app.quit && app.dialog.is_none());
}

#[test]
fn close_with_unsaved_asks() {
    let state = tempfile::tempdir().unwrap();
    let (d, mut app) = two_modified();
    app.recovery_dir = Some(state.path().join("recovery"));
    app.handle_key(ctrl('w'));
    assert_eq!(confirm_buttons(&app), ["Save", "Discard", "Cancel"]);
    choose(&mut app, "Cancel");
    assert_eq!(app.docs.len(), 2);
    // Save writes and closes the tab.
    app.handle_key(ctrl('w'));
    choose(&mut app, "Save");
    assert_eq!(app.docs.len(), 1);
    assert_eq!(
        std::fs::read_to_string(d.path().join("b.txt")).unwrap(),
        "2b\n"
    );
    // Recovery write fails: the tab stays open.
    app.recovery_dir = Some(d.path().join("a.txt").join("not-a-dir"));
    app.handle_key(ctrl('w'));
    choose(&mut app, "Discard");
    assert_eq!(app.docs.len(), 1);
    assert!(
        notices(&app).contains("nothing was discarded"),
        "{}",
        notices(&app)
    );
    // Discard keeps a recovery copy and closes.
    app.recovery_dir = Some(state.path().join("recovery"));
    app.handle_key(ctrl('w'));
    choose(&mut app, "Discard");
    assert!(app.docs.is_empty());
    assert_eq!(
        std::fs::read_to_string(d.path().join("a.txt")).unwrap(),
        "a\n"
    );
    let files: Vec<_> = std::fs::read_dir(state.path().join("recovery"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(std::fs::read_to_string(&files[0]).unwrap(), "1a\n");
    assert!(notices(&app).contains("recovery"), "{}", notices(&app));
}

fn prompt_error(app: &App) -> Option<String> {
    match &app.dialog.as_ref()?.widget {
        DialogWidget::Prompt(p) => p.error.clone(),
        DialogWidget::Confirm(_) => None,
    }
}

#[test]
fn save_as_prompt() {
    let (d, path, mut app, _) = app_with_file("one\n");
    typ(&mut app, "x");
    crate::app_tests::run(&mut app, "file.save_as");
    assert_eq!(
        prompt_text(&app).as_deref(),
        Some("a.txt"),
        "starts from the current name"
    );
    // A folder or an empty name is refused in the prompt.
    std::fs::create_dir(d.path().join("sub")).unwrap();
    submit(&mut app, "sub");
    assert!(prompt_error(&app).is_some(), "folder accepted");
    submit(&mut app, "  ");
    assert!(prompt_error(&app).is_some(), "empty name accepted");
    submit(&mut app, "sub/copy.txt");
    assert!(app.dialog.is_none());
    assert_eq!(
        std::fs::read_to_string(d.path().join("sub/copy.txt")).unwrap(),
        "xone\n"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\n");
    assert_eq!(app.active_name(), "sub/copy.txt");
    // An existing file is replaced only after a confirmation.
    std::fs::write(d.path().join("other.txt"), "keep\n").unwrap();
    crate::app_tests::space_i(&mut app);
    typ(&mut app, "y");
    app.run_command("file.save_as", CmdArgs::None);
    submit(&mut app, "other.txt");
    assert_eq!(confirm_buttons(&app), ["Replace", "Cancel"]);
    choose(&mut app, "Cancel");
    assert_eq!(
        std::fs::read_to_string(d.path().join("other.txt")).unwrap(),
        "keep\n"
    );
    app.run_command("file.save_as", CmdArgs::None);
    submit(&mut app, "other.txt");
    choose(&mut app, "Replace");
    assert_eq!(
        std::fs::read_to_string(d.path().join("other.txt")).unwrap(),
        "xyone\n"
    );
    assert_eq!(app.active_name(), "other.txt");
}

#[test]
fn open_file_prompt() {
    let (d, _p, mut app, _) = app_with_file("a\n");
    std::fs::write(d.path().join("b.txt"), "b\n").unwrap();
    app.handle_key(ctrl('o'));
    submit(&mut app, "missing.txt");
    assert!(prompt_error(&app).is_some());
    submit(&mut app, "b.txt");
    assert!(app.dialog.is_none());
    assert_eq!(app.active_name(), "b.txt");
}

fn ev_modified(app: &mut App, path: &Path) {
    app.handle_fs_event(vr_core::FsEvent::Modified(path.to_path_buf()));
}

fn is_external(app: &App, want_on_save: bool) -> bool {
    matches!(
        app.dialog.as_ref().map(|d| &d.action),
        Some(DialogAction::ExternalChange { on_save, .. }) if *on_save == want_on_save
    )
}

#[test]
fn modified_doc_changed_on_disk_prompts() {
    // Reload: the disk text replaces the buffer as one undo step.
    let (_d, path, mut app, _) = app_with_file("old\n");
    typ(&mut app, "mine ");
    std::fs::write(&path, "theirs\n").unwrap();
    ev_modified(&mut app, &path);
    assert!(is_external(&app, false));
    assert_eq!(confirm_buttons(&app), ["Reload", "Keep mine"]);
    choose(&mut app, "Reload");
    assert_eq!(text(&app), "theirs\n");
    assert!(!app.active_doc().unwrap().is_modified());
    crate::app_tests::space_i(&mut app);
    app.handle_key(ctrl('z'));
    assert_eq!(text(&app), "mine old\n");

    // Keep mine: the buffer stays, the tab is marked, and saving asks before replacing.
    let (_d, path, mut app, _) = app_with_file("old\n");
    typ(&mut app, "mine ");
    std::fs::write(&path, "theirs\n").unwrap();
    ev_modified(&mut app, &path);
    choose(&mut app, "Keep mine");
    assert!(app.dialog.is_none());
    assert_eq!(text(&app), "mine old\n");
    assert_eq!(
        app.active_doc().unwrap().disk_state,
        vr_core::DiskState::ChangedOnDisk
    );
    app.handle_key(ctrl('s'));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "theirs\n");
    assert!(is_external(&app, true));
    assert_eq!(confirm_buttons(&app), ["Keep mine", "Reload", "Cancel"]);
    choose(&mut app, "Cancel");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "theirs\n");
    app.handle_key(ctrl('s'));
    choose(&mut app, "Keep mine");
    assert!(app.dialog.is_none());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "mine old\n");
    assert!(!app.active_doc().unwrap().is_modified());
    assert_eq!(
        app.active_doc().unwrap().disk_state,
        vr_core::DiskState::InSync
    );
}

#[test]
fn keep_mine_replaces_only_the_version_shown() {
    let (_d, path, mut app, _) = app_with_file("old\n");
    typ(&mut app, "mine ");
    std::fs::write(&path, "v2\n").unwrap();
    app.handle_key(ctrl('s'));
    assert!(is_external(&app, true));
    // A newer version lands while the prompt is open (same length: only content differs).
    std::fs::write(&path, "v3\n").unwrap();
    choose(&mut app, "Keep mine");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "v3\n",
        "unseen version replaced"
    );
    assert!(
        is_external(&app, true),
        "asks again about the newer version"
    );
    assert!(notices(&app).contains("changed again"), "{}", notices(&app));
    choose(&mut app, "Keep mine");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "mine old\n");
    // The file deleted while the prompt is open is a newer version too.
    typ(&mut app, "x");
    std::fs::write(&path, "v4\n").unwrap();
    app.handle_key(ctrl('s'));
    std::fs::remove_file(&path).unwrap();
    choose(&mut app, "Keep mine");
    assert!(!path.exists());
    assert!(is_external(&app, true));
}

#[test]
fn deleted_on_disk_marks_tab() {
    // Unmodified: marked, no prompt.
    let (_d, path, mut app, _) = app_with_file("keep\n");
    std::fs::remove_file(&path).unwrap();
    app.handle_fs_event(vr_core::FsEvent::Removed(path.clone()));
    assert!(app.dialog.is_none());
    assert_eq!(
        app.active_doc().unwrap().disk_state,
        vr_core::DiskState::DeletedOnDisk
    );

    // Modified: Save (recreate) / Close.
    let (_d, path, mut app, _) = app_with_file("keep\n");
    typ(&mut app, "x");
    std::fs::remove_file(&path).unwrap();
    app.handle_fs_event(vr_core::FsEvent::Removed(path.clone()));
    assert_eq!(confirm_buttons(&app), ["Save (recreate)", "Close", "Keep"]);
    choose(&mut app, "Save (recreate)");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "xkeep\n");
    assert_eq!(
        app.active_doc().unwrap().disk_state,
        vr_core::DiskState::InSync
    );

    let state = tempfile::tempdir().unwrap();
    let (_d, path, mut app, _) = app_with_file("keep\n");
    app.recovery_dir = Some(state.path().join("recovery"));
    typ(&mut app, "x");
    std::fs::remove_file(&path).unwrap();
    app.handle_fs_event(vr_core::FsEvent::Removed(path.clone()));
    choose(&mut app, "Close");
    assert!(app.docs.is_empty());
    assert_eq!(
        std::fs::read_dir(state.path().join("recovery"))
            .unwrap()
            .count(),
        1
    );
    // Esc keeps the buffer (marked).
    let (_d, path, mut app, _) = app_with_file("keep\n");
    typ(&mut app, "x");
    std::fs::remove_file(&path).unwrap();
    app.handle_fs_event(vr_core::FsEvent::Removed(path.clone()));
    app.handle_key(key(KeyCode::Esc));
    assert_eq!(app.docs.len(), 1);
    assert_eq!(text(&app), "xkeep\n");
}

/// Races a writer against one Enter that writes the file: `prepare` leaves `shown` on disk
/// and the dialog ready for Enter. Asserts the strict D77 rule (conflict copies count as
/// kept), that a refused write asks again, and that both outcomes occurred.
fn race_enter(name: &str, shown: &[u8], prepare: impl Fn(&mut App, &Path)) {
    use vr_core::testutil::WriterMode;
    race_enter_on(
        name,
        &[WriterMode::InPlace, WriterMode::TempRename],
        shown,
        std::time::Duration::ZERO,
        |app, path| {
            prepare(app, path);
            path.to_path_buf()
        },
        |app| is_external(app, true),
    );
}

/// [`race_enter`] on the file `prepare` returns (it may be another file, or one that does
/// not exist: `shown` empty), with the writer started up to `lead` before Enter, while the
/// prompt is still open. Success means the active document is saved to that file; a refusal
/// must leave `asks_again` true.
fn race_enter_on(
    name: &str,
    modes: &[vr_core::testutil::WriterMode],
    shown: &[u8],
    lead: std::time::Duration,
    prepare: impl Fn(&mut App, &Path) -> std::path::PathBuf,
    asks_again: impl Fn(&App) -> bool,
) {
    use std::time::Duration;
    use vr_core::testutil::{
        RacingWriter, assert_exchange_proofs_known, assert_no_lost_write_swapping, sweep_offsets,
        timed,
    };
    for &mode in modes {
        let (mut ok_count, mut refused) = (0, 0);
        for offset in sweep_offsets(40, Duration::from_micros(300)) {
            let (d, own, mut app, _) = app_with_file("old\n");
            let path = prepare(&mut app, &own);
            if shown.is_empty() {
                assert!(!path.exists());
            } else {
                assert_eq!(std::fs::read(&path).unwrap(), shown);
            }
            let ours = text(&app).into_bytes();
            let writer =
                RacingWriter::start_after(offset, &path, mode, Duration::from_micros(300), 6);
            // A lead makes some writes land before Enter, while the prompt is still open.
            std::thread::sleep(lead);
            let (_, run) = timed(
                || {
                    app.handle_key(key(KeyCode::Enter));
                    let doc = app.active_doc().unwrap();
                    !doc.is_modified() && doc.path.as_deref() == Some(path.as_path())
                },
                |saved| *saved,
            );
            let versions = writer.stop();
            assert_exchange_proofs_known(d.path(), &versions);
            let final_bytes = std::fs::read(&path).unwrap();
            let kept: Vec<Vec<u8>> = std::fs::read_dir(d.path())
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| p.to_string_lossy().contains(".vr-ide-conflict-"))
                .map(|p| std::fs::read(p).unwrap())
                .collect();
            assert_no_lost_write_swapping(shown, &final_bytes, &ours, &versions, run, &kept);
            if run.ok {
                ok_count += 1;
            } else {
                refused += 1;
                assert!(
                    asks_again(&app),
                    "a refused write asks again; dialog: {:?}; notices: {}",
                    app.dialog.as_ref().map(|d| &d.widget),
                    notices(&app)
                );
            }
        }
        eprintln!("{name} {mode:?}: ok={ok_count} refused={refused}");
        assert!(
            ok_count > 0 && refused > 0,
            "{name} {mode:?}: both outcomes must occur (ok={ok_count} refused={refused})"
        );
    }
}

/// D64 through the App: "Keep mine" on the save prompt, with a writer rewriting the file while
/// the choice is carried out, never replaces a version the user was not shown.
#[test]
fn keep_mine_never_loses_a_concurrent_write() {
    const SHOWN: &[u8] = b"agent v1\n";
    race_enter("keep_mine", SHOWN, |app, path| {
        typ(app, "mine ");
        std::fs::write(path, SHOWN).unwrap();
        app.handle_key(ctrl('s'));
        assert!(is_external(app, true));
        // Select Keep mine now so only Enter runs while the writer is active.
        app.handle_key(key(KeyCode::Left));
        app.handle_key(key(KeyCode::Left));
        assert_eq!(
            match &app.dialog.as_ref().unwrap().widget {
                DialogWidget::Confirm(c) => c.buttons[c.selected].as_str(),
                DialogWidget::Prompt(_) => "",
            },
            "Keep mine"
        );
    });
}

// ---- fix round 1 -------------------------------------------------------------------------

/// Save As with the prefilled name (the document's own path) is a Save: a file changed on
/// disk is never replaced without the external-change prompt (D64).
#[test]
fn save_as_to_own_path_checks_freshness_like_save() {
    let (_d, path, mut app, _) = app_with_file("old\n");
    typ(&mut app, "mine ");
    std::fs::write(&path, "agent\n").unwrap();
    app.run_command("file.save_as", CmdArgs::None);
    assert_eq!(prompt_text(&app).as_deref(), Some("a.txt"));
    app.handle_key(key(KeyCode::Enter));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "agent\n",
        "Save As replaced an unseen version"
    );
    assert!(is_external(&app, true), "the D81 prompt opens");
    // The same path spelled differently is still the own path.
    app.handle_key(key(KeyCode::Esc));
    app.run_command("file.save_as", CmdArgs::None);
    submit(&mut app, "./a.txt");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "agent\n");
    assert!(is_external(&app, true));
    choose(&mut app, "Keep mine");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "mine old\n");
    // Unchanged on disk: saved at once.
    typ(&mut app, "x");
    app.run_command("file.save_as", CmdArgs::None);
    app.handle_key(key(KeyCode::Enter));
    assert!(app.dialog.is_none());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text(&app));
    assert!(!app.active_doc().unwrap().is_modified());
}

/// Save As + Enter on the prefilled name with a writer racing it: same rule as Save.
#[test]
fn save_as_to_own_path_never_loses_a_concurrent_write() {
    race_enter("save_as_own", b"old\n", |app, _| {
        typ(app, "mine ");
        app.run_command("file.save_as", CmdArgs::None);
        assert_eq!(prompt_text(app).as_deref(), Some("a.txt"));
    });
}

fn recovery_files(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap())
                .collect()
        })
        .unwrap_or_default()
}

/// Reload replaces unsaved edits only after they are in the recovery folder; a failed
/// recovery write leaves the buffer as it was (M1).
#[test]
fn reload_keeps_unsaved_edits_in_recovery() {
    let state = tempfile::tempdir().unwrap();
    let rec = state.path().join("recovery");
    // From the event prompt.
    let (_d, path, mut app, _) = app_with_file("old\n");
    app.recovery_dir = Some(rec.clone());
    typ(&mut app, "mine ");
    std::fs::write(&path, "theirs\n").unwrap();
    ev_modified(&mut app, &path);
    choose(&mut app, "Reload");
    assert_eq!(text(&app), "theirs\n");
    assert_eq!(recovery_files(&rec), ["mine old\n"]);
    assert!(notices(&app).contains("recovery"), "{}", notices(&app));
    // A later quit has nothing unsaved, and the edits are still in recovery.
    app.handle_key(ctrl('q'));
    assert!(app.quit);

    // From the file.reload command.
    let rec2 = state.path().join("recovery2");
    let (_d, path, mut app, _) = app_with_file("old\n");
    app.recovery_dir = Some(rec2.clone());
    typ(&mut app, "x");
    std::fs::write(&path, "new\n").unwrap();
    app.run_command("file.reload", CmdArgs::None);
    assert_eq!(text(&app), "new\n");
    assert_eq!(recovery_files(&rec2), ["xold\n"]);

    // Recovery cannot be written: no reload.
    let (_d, path, mut app, _) = app_with_file("old\n");
    let blocker = tempfile::NamedTempFile::new().unwrap();
    app.recovery_dir = Some(blocker.path().join("recovery"));
    typ(&mut app, "mine ");
    std::fs::write(&path, "theirs\n").unwrap();
    app.handle_key(ctrl('s'));
    choose(&mut app, "Reload");
    assert_eq!(text(&app), "mine old\n");
    assert!(app.active_doc().unwrap().is_modified());
    assert!(
        notices(&app).contains("nothing was discarded"),
        "{}",
        notices(&app)
    );
    app.run_command("file.reload", CmdArgs::None);
    assert_eq!(text(&app), "mine old\n");
}

fn at(app: &mut App, k: KeyEvent, t: std::time::Instant) {
    app.handle_key_at(k, t);
}

/// Legacy terminals report a held key's auto-repeat as presses, and an Enter typed just as
/// a prompt opens was meant for the editor: neither answers a dialog (M2, D80).
#[test]
fn held_or_in_flight_enter_never_answers_a_dialog() {
    use std::time::{Duration as D, Instant};
    let enter = key(KeyCode::Enter);

    // Palette "close" + Enter held: the Close dialog opens, repeats choose nothing.
    let (_d, path, mut app, _) = app_with_file("a\n");
    app.enter_settle = crate::app::ENTER_SETTLE;
    app.enter_repeat_gap = crate::app::ENTER_REPEAT_GAP;
    typ(&mut app, "x");
    let t0 = Instant::now();
    at(
        &mut app,
        KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE),
        t0,
    );
    for c in "close file".chars() {
        at(&mut app, key(KeyCode::Char(c)), t0 + D::from_secs(1));
    }
    let mut t = t0 + D::from_secs(2);
    at(&mut app, enter, t);
    assert_eq!(confirm_buttons(&app), ["Save", "Discard", "Cancel"]);
    t += D::from_millis(500);
    for _ in 0..40 {
        at(&mut app, enter, t);
        t += D::from_millis(33);
    }
    assert!(
        app.dialog.is_some(),
        "a held Enter answered the Close dialog"
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "a\n");
    // Released, then pressed on purpose.
    at(&mut app, enter, t + D::from_secs(1));
    assert!(app.dialog.is_none());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "xa\n");

    // The palette ignores an Enter that arrives right after it opened.
    let (_d, _path, mut app, _) = app_with_file("a\n");
    app.enter_settle = crate::app::ENTER_SETTLE;
    app.enter_repeat_gap = crate::app::ENTER_REPEAT_GAP;
    typ(&mut app, "x");
    let t0 = Instant::now();
    at(
        &mut app,
        KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE),
        t0,
    );
    // An Enter right after the palette opened (meant for the editor) runs nothing.
    at(&mut app, enter, t0 + D::from_millis(100));
    assert!(app.palette.is_some());

    // A watcher prompt opening while the user types Enter in the editor.
    let (_d, path2, mut app2, _) = app_with_file("keep\n");
    app2.enter_settle = crate::app::ENTER_SETTLE;
    app2.enter_repeat_gap = crate::app::ENTER_REPEAT_GAP;
    typ(&mut app2, "x");
    let t0 = Instant::now();
    at(&mut app2, enter, t0);
    std::fs::remove_file(&path2).unwrap();
    app2.handle_fs_event(vr_core::FsEvent::Removed(path2.clone()));
    assert!(app2.dialog.is_some());
    at(&mut app2, enter, t0 + D::from_millis(150));
    assert!(
        app2.dialog.is_some(),
        "an in-flight Enter answered the prompt"
    );
    assert!(
        !path2.exists(),
        "an in-flight Enter recreated a deleted file"
    );
    // The default of a prompt that opens by itself writes nothing.
    at(&mut app2, enter, t0 + D::from_secs(2));
    assert!(app2.dialog.is_none());
    assert!(!path2.exists());
    assert_eq!(app2.docs.len(), 1);
}

/// An open dialog takes every key, paste and chord: none of them reaches the buffer.
#[test]
fn input_while_a_dialog_is_open_never_edits_the_buffer() {
    let (_d, _p, mut app, _) = app_with_file("a\n");
    typ(&mut app, "x");
    app.handle_key(ctrl('q'));
    assert!(app.dialog.is_some());
    typ(&mut app, "abc");
    app.handle_paste("pasted");
    app.handle_key(ctrl('z'));
    app.handle_key(ctrl('v'));
    app.handle_key(key(KeyCode::Backspace));
    assert_eq!(text(&app), "xa\n");
    assert!(app.dialog.is_some() && !app.quit);
    // The same for a prompt: the text goes into its input only.
    app.handle_key(key(KeyCode::Esc));
    app.run_command("file.save_as", CmdArgs::None);
    app.handle_paste("zz");
    typ(&mut app, "q");
    app.handle_key(ctrl('z'));
    assert_eq!(prompt_text(&app).as_deref(), Some("a.txtzzq"));
    assert_eq!(text(&app), "xa\n");
}

/// Save As onto a file open in another tab never leaves two tabs on one path: an unmodified
/// tab is closed after the write; a modified one makes the prompt refuse the name.
#[test]
fn save_as_onto_a_file_open_in_another_tab() {
    let (d, _p, mut app, _) = app_with_file("a\n");
    let b = d.path().join("b.txt");
    std::fs::write(&b, "b\n").unwrap();
    app.open_path(&b).unwrap();
    app.set_active(0);
    crate::app_tests::space_i(&mut app);
    typ(&mut app, "x");
    app.run_command("file.save_as", CmdArgs::None);
    submit(&mut app, "b.txt");
    choose(&mut app, "Replace");
    assert_eq!(std::fs::read_to_string(&b).unwrap(), "xa\n");
    let paths: Vec<_> = app.docs.iter().map(|d| d.path.clone().unwrap()).collect();
    assert_eq!(paths, std::slice::from_ref(&b), "one tab per path");
    assert_eq!(text(&app), "xa\n");

    // The other tab has unsaved changes: refused, nothing written.
    let (d, _p, mut app, _) = app_with_file("a\n");
    let b = d.path().join("b.txt");
    std::fs::write(&b, "b\n").unwrap();
    app.open_path(&b).unwrap();
    crate::app_tests::space_i(&mut app);
    typ(&mut app, "y");
    app.set_active(0);
    crate::app_tests::space_i(&mut app);
    typ(&mut app, "x");
    app.run_command("file.save_as", CmdArgs::None);
    submit(&mut app, "b.txt");
    assert!(
        prompt_error(&app).is_some_and(|e| e.contains("unsaved")),
        "{:?}",
        prompt_error(&app)
    );
    assert_eq!(std::fs::read_to_string(&b).unwrap(), "b\n");
    assert_eq!(app.docs.len(), 2);
}

/// Replace overwrites only the version of the target that existed when the Replace prompt
/// opened; a newer one asks again (B2, D87).
#[test]
fn replace_never_overwrites_a_version_written_after_the_prompt() {
    let (d, _p, mut app, _) = app_with_file("a\n");
    let other = d.path().join("other.txt");
    std::fs::write(&other, "keep\n").unwrap();
    typ(&mut app, "x");
    app.run_command("file.save_as", CmdArgs::None);
    submit(&mut app, "other.txt");
    assert_eq!(confirm_buttons(&app), ["Replace", "Cancel"]);
    std::fs::write(&other, "agent v2\n").unwrap();
    choose(&mut app, "Replace");
    assert_eq!(
        std::fs::read_to_string(&other).unwrap(),
        "agent v2\n",
        "Replace overwrote a version the user was not asked about"
    );
    assert!(
        matches!(
            app.dialog.as_ref().map(|d| &d.action),
            Some(DialogAction::ReplaceFile { .. })
        ),
        "asks again"
    );
    assert!(notices(&app).contains("changed"), "{}", notices(&app));
    assert_eq!(app.active_name(), "a.txt");
    choose(&mut app, "Replace");
    assert_eq!(std::fs::read_to_string(&other).unwrap(), "xa\n");
    assert_eq!(app.active_name(), "other.txt");
}

fn is_replace(app: &App) -> bool {
    matches!(
        app.dialog.as_ref().map(|d| &d.action),
        Some(DialogAction::ReplaceFile { .. })
    )
}

/// Replace with a writer rewriting the target while the choice is carried out: only the
/// version the prompt was about is ever replaced; a refusal asks again (B2, D87).
#[test]
fn replace_never_loses_a_concurrent_write() {
    use vr_core::testutil::WriterMode;
    const SHOWN: &[u8] = b"agent v1\n";
    race_enter_on(
        "replace",
        &[WriterMode::InPlace, WriterMode::TempRename],
        SHOWN,
        std::time::Duration::from_millis(3),
        |app, own| {
            let other = own.with_file_name("other.txt");
            std::fs::write(&other, SHOWN).unwrap();
            typ(app, "mine ");
            app.run_command("file.save_as", CmdArgs::None);
            submit(app, "other.txt");
            assert!(is_replace(app));
            app.handle_key(key(KeyCode::Left));
            assert_eq!(
                match &app.dialog.as_ref().unwrap().widget {
                    DialogWidget::Confirm(c) => c.buttons[c.selected].as_str(),
                    DialogWidget::Prompt(_) => "",
                },
                "Replace"
            );
            other
        },
        is_replace,
    );
}

/// Save As to a new name with a writer creating that file while Enter is handled: a file
/// created meanwhile is never replaced; the Replace prompt asks about it. Any refusal leaves a
/// prompt open.
#[test]
fn save_as_new_name_never_loses_a_concurrent_create() {
    use vr_core::testutil::WriterMode;
    race_enter_on(
        "save_as_new",
        &[WriterMode::TempRename],
        b"",
        std::time::Duration::from_millis(1),
        |app, own| {
            typ(app, "mine ");
            app.run_command("file.save_as", CmdArgs::None);
            for _ in 0.."a.txt".len() {
                app.handle_key(key(KeyCode::Backspace));
            }
            typ(app, "new.txt");
            own.with_file_name("new.txt")
        },
        // Back in a prompt with the reason: Replace (created meanwhile) or the name prompt
        // with its error (the version could not be read at submit, or the create failed).
        |app| is_replace(app) || prompt_error(app).is_some(),
    );
}

/// A target that is not a regular file (a device) is refused in the prompt.
#[test]
fn save_as_refuses_a_non_regular_target() {
    let (_d, path, mut app, _) = app_with_file("a\n");
    typ(&mut app, "x");
    app.run_command("file.save_as", CmdArgs::None);
    submit(&mut app, "/dev/null");
    assert!(prompt_error(&app).is_some(), "{:?}", app.dialog);
    assert_eq!(app.active_name(), "a.txt");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "a\n");
}

fn confirm_message(app: &App) -> String {
    match &app.dialog.as_ref().expect("a dialog").widget {
        DialogWidget::Confirm(c) => c.message.clone(),
        DialogWidget::Prompt(_) => panic!("confirm dialog expected"),
    }
}

/// (a) After a refusal no fresh version can be read (the file keeps changing): the prompt
/// comes back with the error in it, the choice writes nothing until a version is taken.
#[test]
fn refused_choice_reprompts_when_no_version_can_be_taken() {
    use crate::commands::file::seam;
    // Replace.
    let (d, _p, mut app, _) = app_with_file("a\n");
    let other = d.path().join("other.txt");
    std::fs::write(&other, "keep\n").unwrap();
    typ(&mut app, "x");
    app.run_command("file.save_as", CmdArgs::None);
    submit(&mut app, "other.txt");
    std::fs::write(&other, "agent v2\n").unwrap();
    seam::fail_next_versions(1);
    choose(&mut app, "Replace");
    assert!(is_replace(&app), "no prompt; notices: {}", notices(&app));
    assert!(
        confirm_message(&app).contains("keeps changing"),
        "{}",
        confirm_message(&app)
    );
    // Without a version Replace writes nothing; it takes one and asks again.
    choose(&mut app, "Replace");
    assert_eq!(std::fs::read_to_string(&other).unwrap(), "agent v2\n");
    assert!(is_replace(&app), "no prompt; notices: {}", notices(&app));
    choose(&mut app, "Replace");
    assert_eq!(std::fs::read_to_string(&other).unwrap(), "xa\n");
    assert!(app.dialog.is_none());

    // Keep mine.
    let (_d, path, mut app, _) = app_with_file("old\n");
    typ(&mut app, "mine ");
    std::fs::write(&path, "v2\n").unwrap();
    app.handle_key(ctrl('s'));
    std::fs::write(&path, "v3 longer\n").unwrap();
    seam::fail_next_versions(1);
    choose(&mut app, "Keep mine");
    assert!(
        is_external(&app, true),
        "no prompt; notices: {}",
        notices(&app)
    );
    assert!(
        confirm_message(&app).contains("keeps changing"),
        "{}",
        confirm_message(&app)
    );
    choose(&mut app, "Keep mine");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "v3 longer\n");
    assert!(
        is_external(&app, true),
        "no prompt; notices: {}",
        notices(&app)
    );
    choose(&mut app, "Keep mine");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "mine old\n");

    // The save prompt itself cannot read a version: it opens anyway, writing nothing.
    let (_d, path, mut app, _) = app_with_file("old\n");
    typ(&mut app, "mine ");
    std::fs::write(&path, "v2\n").unwrap();
    seam::fail_next_versions(1);
    app.handle_key(ctrl('s'));
    assert!(
        is_external(&app, true),
        "no prompt; notices: {}",
        notices(&app)
    );
    choose(&mut app, "Keep mine");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "v2\n");
    assert!(
        is_external(&app, true),
        "no prompt; notices: {}",
        notices(&app)
    );
}

/// (b) The commit ends with a conflict copy (or any other error): the same prompt comes back
/// with the error in it.
#[test]
fn refused_choice_with_an_error_reprompts_with_it() {
    use crate::commands::file::seam;
    let (d, _p, mut app, _) = app_with_file("a\n");
    let other = d.path().join("other.txt");
    std::fs::write(&other, "keep\n").unwrap();
    typ(&mut app, "x");
    app.run_command("file.save_as", CmdArgs::None);
    submit(&mut app, "other.txt");
    seam::fail_next_commit("another version was kept at other.txt.vr-ide-conflict-1-1");
    choose(&mut app, "Replace");
    assert!(is_replace(&app), "no prompt; notices: {}", notices(&app));
    assert!(
        confirm_message(&app).contains("kept at"),
        "{}",
        confirm_message(&app)
    );
    assert_eq!(std::fs::read_to_string(&other).unwrap(), "keep\n");
    choose(&mut app, "Replace");
    assert_eq!(std::fs::read_to_string(&other).unwrap(), "xa\n");

    let (_d, path, mut app, _) = app_with_file("old\n");
    typ(&mut app, "mine ");
    std::fs::write(&path, "v2\n").unwrap();
    app.handle_key(ctrl('s'));
    seam::fail_next_commit("another version was kept at a.txt.vr-ide-conflict-1-1");
    choose(&mut app, "Keep mine");
    assert!(
        is_external(&app, true),
        "no prompt; notices: {}",
        notices(&app)
    );
    assert!(
        confirm_message(&app).contains("kept at"),
        "{}",
        confirm_message(&app)
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "v2\n");
}

/// FR-8: Keep mine that recreates a file deleted while the prompt was open brings its
/// explorer row back although the watcher drops the event of that (own) write.
#[test]
fn keep_mine_recreating_a_deleted_file_brings_its_row_back() {
    let (d, path, mut app, _) = app_with_file("old\n");
    app.explorer.visible = true;
    crate::commands::explorer::ensure_loaded(&mut app);
    app.start_watcher();
    app.watch_root();
    let listed = |app: &App| app.explorer.rows.iter().any(|r| r.path == path);
    assert!(listed(&app));
    typ(&mut app, "mine ");
    std::fs::write(&path, "agent v2\n").unwrap();
    app.handle_key(ctrl('s'));
    assert!(is_external(&app, true), "notices: {}", notices(&app));
    std::fs::remove_file(&path).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while listed(&app) {
        assert!(std::time::Instant::now() < deadline, "row never dropped");
        std::thread::sleep(std::time::Duration::from_millis(20));
        app.drain_fs_events();
    }
    // The first choice is refused (the version changed), the second recreates the file.
    choose(&mut app, "Keep mine");
    assert!(is_external(&app, true), "notices: {}", notices(&app));
    choose(&mut app, "Keep mine");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "mine old\n");
    assert!(listed(&app), "no row after Keep mine recreated the file");
    drop(d);
}

/// T21: the palette's key column shows each command's one key, in both keyboard modes;
/// palette-only commands show none; the read-mode built-ins show theirs.
#[test]
fn palette_key_column_shows_the_one_key() {
    for enhanced in [true, false] {
        let (mut app, _) = test_app(Path::new("/p"));
        app.keyboard_enhanced = enhanced;
        app.run_command("app.palette", CmdArgs::None);
        let items = app.palette.as_ref().unwrap().items.clone();
        let keys = |name: &str| items.iter().find(|i| i.name == name).unwrap().keys.clone();
        // The palette does not list itself.
        for &(_, k, cmd) in crate::app_tests::KEYSET_V2
            .iter()
            .filter(|e| e.2 != "app.palette")
        {
            assert_eq!(keys(cmd), k, "{cmd} {enhanced}");
        }
        assert_eq!(keys("editor.edit_mode"), "space i", "{enhanced}");
        assert_eq!(keys("focus.cycle"), "tab", "{enhanced}");
        for cmd in [
            "search.project",
            "file.save_as",
            "file.reload",
            "explorer.new_folder",
        ] {
            assert_eq!(keys(cmd), "", "{cmd} {enhanced}");
        }
        assert_eq!(keys("explorer.toggle"), "", "{enhanced}");
        assert_eq!(keys("terminal.toggle"), "ctrl+`", "{enhanced}");
        assert_eq!(keys("edit.redo"), "ctrl+y", "{enhanced}");
        assert_eq!(keys("file.save"), "ctrl+s", "{enhanced}");
        assert!(items.iter().all(|i| !i.keys.contains(" / ")));
        assert!(items.iter().all(|i| !i.keys.contains("alt")));
    }
}
