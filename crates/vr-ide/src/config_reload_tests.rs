//! T24 item 6 (D279 (6), D273 closed): settings.toml and keymap.toml are read again when saved,
//! in vr-ide or elsewhere; only the settings whose value in the file changed are applied (the
//! diff-apply of `/set`), and a file with an error keeps the old values and says why.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use vr_config::KeyChord;
use vr_core::FsEvent;

use crate::app::{App, NoticeLevel};
use crate::app_tests::{app_with_file, press, space_i};
use crate::slash_tests::{key, slash};

/// A project with one open file and a configuration folder `cfg/` holding `settings` and
/// `keymap` (None: no file).
fn setup(settings: Option<&str>, keymap: Option<&str>) -> (tempfile::TempDir, App, PathBuf) {
    let (d, _p, mut app, _) = app_with_file("one\n");
    let cfg = d.path().join("cfg");
    std::fs::create_dir(&cfg).unwrap();
    if let Some(s) = settings {
        std::fs::write(cfg.join("settings.toml"), s).unwrap();
    }
    if let Some(k) = keymap {
        std::fs::write(cfg.join("keymap.toml"), k).unwrap();
    }
    app.config_dir = Some(cfg.clone());
    (d, app, cfg)
}

/// The text of the error notices.
fn errors(app: &App) -> String {
    app.notices
        .iter()
        .filter(|n| n.level == NoticeLevel::Error)
        .map(|n| n.text.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

fn notices(app: &App) -> String {
    app.notices
        .iter()
        .map(|n| n.text.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Replaces the active file's text in edit mode and saves it with ctrl+s.
fn rewrite_and_save(app: &mut App, text: &str) {
    if !app.edit_mode() {
        space_i(app);
    }
    press(app, "ctrl+a");
    app.handle_paste(text);
    press(app, "ctrl+s");
    assert!(
        !app.active_doc().unwrap().is_modified(),
        "saved: {}",
        notices(app)
    );
}

/// Opens `name` of the configuration folder with /config or /keymap.
fn open_config(app: &mut App, slash_cmd: &str) {
    slash(app, slash_cmd);
    key(app, KeyCode::Enter);
    assert!(app.palette.is_none());
}

#[test]
fn saving_settings_in_vr_ide_applies_only_the_settings_that_changed_in_the_file() {
    let (_d, mut app, cfg) = setup(Some("tab_width = 4\n"), None);
    // A session-only change of another setting (as /set does) must survive the reload.
    slash(&mut app, "/set insert_spaces false");
    key(&mut app, KeyCode::Enter);
    assert!(!app.settings.insert_spaces);
    let hidden = app.explorer.tree.show_hidden();
    open_config(&mut app, "/config");
    let settings = cfg.join("settings.toml");
    assert_eq!(
        app.active_doc().unwrap().path.as_deref(),
        Some(settings.as_path())
    );
    assert!(
        notices(&app).contains("applied when you save it"),
        "{}",
        notices(&app)
    );
    app.notices.clear();
    rewrite_and_save(&mut app, "tab_width = 8\n");
    assert_eq!(app.settings.tab_width, 8, "{}", notices(&app));
    assert!(
        !app.settings.insert_spaces,
        "the /set of this session stays"
    );
    assert_eq!(app.explorer.tree.show_hidden(), hidden, "untouched");
    // Open files follow, as with /set.
    assert!(app.docs.iter().all(|d| d.buffer.options().tab_width == 8));
    assert!(
        notices(&app).contains("settings.toml") && notices(&app).contains("tab_width"),
        "{}",
        notices(&app)
    );
    // Saving again with nothing changed applies nothing (the session value stays).
    app.notices.clear();
    slash(&mut app, "/set tab_width 3");
    key(&mut app, KeyCode::Enter);
    rewrite_and_save(&mut app, "tab_width = 8\n# a comment\n");
    assert_eq!(
        app.settings.tab_width, 3,
        "tab_width did not change in the file"
    );
}

#[test]
fn a_bad_settings_file_keeps_the_old_values_and_shows_the_error() {
    let (_d, mut app, cfg) = setup(Some("tab_width = 4\n"), None);
    let before = app.settings.clone();
    open_config(&mut app, "/config");
    for (bad, what) in [
        ("tab_width = = 8\n", "settings.toml"),
        ("tab_width = 8\nno_such_setting = 1\n", "no_such_setting"),
        (
            "tab_width = 8\nterminal_height_percent = 300\n",
            "terminal_height_percent",
        ),
        ("tab_width = 8\ntheme = \"pink\"\n", "pink"),
    ] {
        app.notices.clear();
        rewrite_and_save(&mut app, bad);
        assert_eq!(app.settings, before, "{bad:?} changed nothing");
        let e = errors(&app);
        assert!(
            e.contains(what) && e.contains("old values kept"),
            "{bad:?}: {e}"
        );
    }
    // Fixed: applied, measured against what the file said last time it was applied.
    app.notices.clear();
    rewrite_and_save(&mut app, "tab_width = 8\n");
    assert_eq!(app.settings.tab_width, 8, "{}", notices(&app));
    assert!(errors(&app).is_empty(), "{}", errors(&app));
    assert!(cfg.join("settings.toml").exists());
}

#[test]
fn saving_the_keymap_replaces_the_keys_and_a_bad_one_keeps_them() {
    let (_d, mut app, _cfg) = setup(None, Some("\"ctrl+s\" = \"file.save\"\n"));
    let t: KeyChord = "ctrl+t".parse().unwrap();
    assert_eq!(app.keymap.command_for(&t), None);
    open_config(&mut app, "/keymap");
    app.notices.clear();
    rewrite_and_save(&mut app, "\"ctrl+t\" = \"file.goto\"\n");
    assert_eq!(app.keymap.command_for(&t), Some("file.goto"));
    assert!(notices(&app).contains("keymap.toml"), "{}", notices(&app));
    // An unknown command or a bad key: nothing changes, the error is shown.
    for (bad, what) in [
        ("\"ctrl+t\" = \"no.such\"\n", "no.such"),
        ("\"hyper+t\" = \"file.save\"\n", "hyper+t"),
        ("\"ctrl+t\" = \n", "keymap.toml"),
    ] {
        app.notices.clear();
        rewrite_and_save(&mut app, bad);
        assert_eq!(app.keymap.command_for(&t), Some("file.goto"), "{bad:?}");
        let e = errors(&app);
        assert!(
            e.contains(what) && e.contains("old values kept"),
            "{bad:?}: {e}"
        );
    }
    // The new key works at once.
    app.ran.clear();
    app.handle_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL));
    assert_eq!(app.ran, ["file.goto"]);
}

#[test]
fn a_watcher_event_for_a_config_file_reloads_it() {
    let (_d, mut app, cfg) = setup(Some("tab_width = 4\n"), None);
    let settings = cfg.join("settings.toml");
    std::fs::write(&settings, "tab_width = 6\n").unwrap();
    app.handle_fs_event(FsEvent::Modified(settings.clone()));
    assert_eq!(app.settings.tab_width, 6, "{}", notices(&app));
    // An event for another file of the folder changes nothing.
    std::fs::write(&settings, "tab_width = 7\n").unwrap();
    app.handle_fs_event(FsEvent::Modified(cfg.join("languages.toml")));
    assert_eq!(app.settings.tab_width, 6);
    // A rename over it (editors save so) counts too.
    app.handle_fs_event(FsEvent::Renamed(cfg.join("x.tmp"), settings.clone()));
    assert_eq!(app.settings.tab_width, 7);
}

/// The real watcher: a file edited by another program is applied within the debounce.
#[test]
fn settings_edited_elsewhere_apply_through_the_watcher() {
    let (_d, mut app, cfg) = setup(Some("tab_width = 4\n"), Some(""));
    app.start_watcher();
    // Atomic replace, as editors save.
    let tmp = cfg.join(".settings.toml.tmp");
    std::fs::write(&tmp, "tab_width = 5\n").unwrap();
    std::fs::rename(&tmp, cfg.join("settings.toml")).unwrap();
    std::fs::write(cfg.join("keymap.toml"), "\"ctrl+t\" = \"file.goto\"\n").unwrap();
    let t: KeyChord = "ctrl+t".parse().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.settings.tab_width != 5 || app.keymap.command_for(&t).is_none() {
        assert!(Instant::now() < deadline, "not applied: {}", notices(&app));
        std::thread::sleep(Duration::from_millis(20));
        app.drain_fs_events();
    }
}

/// A config folder that /config makes while vr-ide runs is watched from then on, also after
/// the file's tab is closed (closing a tab must not end the config watch).
#[test]
fn a_config_folder_made_by_config_is_watched() {
    let (d, _p, mut app, _) = app_with_file("one\n");
    let cfg = d.path().join("cfg-new");
    app.config_dir = Some(cfg.clone());
    app.start_watcher();
    open_config(&mut app, "/config");
    assert!(cfg.join("settings.toml").exists());
    let i = app.active;
    app.close_doc(i);
    std::fs::write(cfg.join("settings.toml"), "tab_width = 2\n").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.settings.tab_width != 2 {
        assert!(Instant::now() < deadline, "not applied: {}", notices(&app));
        std::thread::sleep(Duration::from_millis(20));
        app.drain_fs_events();
    }
}

/// Waits (draining watcher events) until `done` holds.
fn wait_for(app: &mut App, what: &str, done: impl Fn(&App) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done(app) {
        assert!(Instant::now() < deadline, "{what}: {}", notices(app));
        std::thread::sleep(Duration::from_millis(20));
        app.drain_fs_events();
    }
}

/// T24 review: settings.toml is a symlink into a dotfiles folder and the real file is open in a
/// tab. The tab's watch on the same file must not end the live reload: an external write, a
/// ctrl+s in vr-ide and an external write after the tab is closed all apply.
#[cfg(unix)]
#[test]
fn a_symlinked_settings_file_opened_by_its_real_path_still_applies() {
    let (d, _p, mut app, _) = app_with_file("one\n");
    let cfg = d.path().join("cfg");
    let dots = d.path().join("dotfiles");
    std::fs::create_dir(&cfg).unwrap();
    std::fs::create_dir(&dots).unwrap();
    let real = dots.join("settings.toml");
    std::fs::write(&real, "tab_width = 4\n").unwrap();
    std::os::unix::fs::symlink(&real, cfg.join("settings.toml")).unwrap();
    app.config_dir = Some(cfg.clone());
    assert!(
        app.is_config_file(&real),
        "the real file is the config file"
    );
    app.start_watcher();
    app.open_path(&real).unwrap();
    assert_eq!(
        app.active_doc().unwrap().path.as_deref(),
        Some(real.as_path())
    );

    std::fs::write(&real, "tab_width = 5\n").unwrap();
    wait_for(&mut app, "external write", |a| a.settings.tab_width == 5);

    rewrite_and_save(&mut app, "tab_width = 6\n");
    assert_eq!(app.settings.tab_width, 6, "ctrl+s: {}", notices(&app));

    let i = app.active;
    app.close_doc(i);
    std::fs::write(&real, "tab_width = 7\n").unwrap();
    wait_for(&mut app, "external write after close", |a| {
        a.settings.tab_width == 7
    });
}

/// T24 review: a writer that truncates first and writes later leaves an empty file for a
/// moment; a watcher event then must not apply every default. The content that follows applies.
#[test]
fn an_empty_settings_file_seen_by_the_watcher_is_not_applied() {
    let (_d, mut app, cfg) = setup(Some("tab_width = 6\ntheme = \"light\"\n"), None);
    let settings = cfg.join("settings.toml");
    app.handle_fs_event(FsEvent::Modified(settings.clone()));
    assert_eq!(app.settings.tab_width, 6, "{}", notices(&app));
    assert_eq!(app.settings.theme, "light");
    app.notices.clear();
    std::fs::write(&settings, "").unwrap();
    app.handle_fs_event(FsEvent::Modified(settings.clone()));
    assert_eq!(app.settings.tab_width, 6, "not reset: {}", notices(&app));
    assert_eq!(app.settings.theme, "light");
    assert!(app.notices.is_empty(), "{}", notices(&app));
    std::fs::write(&settings, "tab_width = 3\ntheme = \"light\"\n").unwrap();
    app.handle_fs_event(FsEvent::Modified(settings.clone()));
    assert_eq!(app.settings.tab_width, 3, "{}", notices(&app));
    // A parse error still says so in red.
    std::fs::write(&settings, "tab_width = \n").unwrap();
    app.handle_fs_event(FsEvent::Modified(settings.clone()));
    assert!(errors(&app).contains("not applied"), "{}", notices(&app));
    assert_eq!(app.settings.tab_width, 3);
}

/// The same for keymap.toml: an empty file seen by the watcher keeps the keys in use.
#[test]
fn an_empty_keymap_file_seen_by_the_watcher_is_not_applied() {
    let (_d, mut app, cfg) = setup(None, Some("\"ctrl+t\" = \"file.goto\"\n"));
    let keymap = cfg.join("keymap.toml");
    app.handle_fs_event(FsEvent::Modified(keymap.clone()));
    let t: KeyChord = "ctrl+t".parse().unwrap();
    assert!(app.keymap.command_for(&t).is_some(), "{}", notices(&app));
    std::fs::write(&keymap, "").unwrap();
    app.handle_fs_event(FsEvent::Modified(keymap.clone()));
    assert!(
        app.keymap.command_for(&t).is_some(),
        "kept: {}",
        notices(&app)
    );
}

/// Final review 2: a reloaded keymap whose only key for a command is one a legacy terminal
/// cannot send says so, as the startup check does.
#[test]
fn a_reloaded_keymap_names_keys_a_legacy_terminal_cannot_send() {
    let (_d, mut app, cfg) = setup(None, Some(""));
    let keymap = cfg.join("keymap.toml");
    app.keyboard_enhanced = false;
    std::fs::write(&keymap, "\"ctrl+shift+m\" = \"file.save_overwrite\"\n").unwrap();
    app.handle_fs_event(FsEvent::Modified(keymap.clone()));
    let m: KeyChord = "ctrl+shift+m".parse().unwrap();
    assert_eq!(app.keymap.command_for(&m), Some("file.save_overwrite"));
    let n = app
        .notices
        .iter()
        .find(|n| n.text.contains("cannot send"))
        .unwrap_or_else(|| panic!("no keyboard notice: {}", notices(&app)));
    assert_eq!(n.level, NoticeLevel::Warning);
    assert!(n.text.contains("keymap.toml applied"), "{}", n.text);
    assert!(
        n.detail
            .as_deref()
            .is_some_and(|d| d.contains("ctrl+shift+m")),
        "{:?}",
        n.detail
    );
    // With the kitty protocol every key arrives: just "applied".
    app.notices.clear();
    app.keyboard_enhanced = true;
    std::fs::write(
        &keymap,
        "\"ctrl+shift+m\" = \"file.save_overwrite\"\n\"ctrl+t\" = \"file.goto\"\n",
    )
    .unwrap();
    app.handle_fs_event(FsEvent::Modified(keymap));
    assert!(!notices(&app).contains("cannot send"), "{}", notices(&app));
    assert!(
        notices(&app).contains("keymap.toml applied"),
        "{}",
        notices(&app)
    );
}
