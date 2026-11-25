//! T20b: slash commands phase 2 (01_spec_T20.md "Phase 2"): file extras, text commands,
//! settings. Driven through the `App` API like `slash_tests`.

use std::path::{Path, PathBuf};

use crossterm::event::KeyCode;
use vr_core::Selection;

use crate::app::App;
use crate::app_tests::{app_with_file, test_app};
use crate::slash_tests::{error, input, key, notices, set, slash, titles, typ};

/// F1, `text`, Enter.
fn run(app: &mut App, text: &str) {
    slash(app, text);
    key(app, KeyCode::Enter);
}

/// `text` ran without an error (the window closed).
fn ran(app: &mut App, text: &str) {
    run(app, text);
    assert!(app.palette.is_none(), "{text}: {:?}", error(app));
}

/// `text` was refused with a message containing `want`; the text stays.
fn refused(app: &mut App, text: &str, want: &str) {
    run(app, text);
    let e = error(app).unwrap_or_else(|| panic!("{text}: no error; notices: {}", notices(app)));
    assert!(e.contains(want), "{text}: {e}");
    assert_eq!(input(app), text);
    key(app, KeyCode::Esc);
}

fn text(app: &App) -> String {
    app.active_doc().unwrap().buffer.text().to_string()
}

fn select(app: &mut App, anchor: usize, head: usize) {
    let doc = app.active_doc_mut().unwrap();
    doc.buffer.set_selection(Selection::single(anchor, head));
}

fn undo(app: &mut App) {
    app.run_command("edit.undo", crate::commands::CmdArgs::None);
}

// ---- registry -------------------------------------------------------------------------------

#[test]
fn registry_has_the_phase_2_commands() {
    let names: Vec<&str> = crate::commands::slash::SLASH_COMMANDS
        .iter()
        .map(|c| c.name)
        .collect();
    for w in [
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
    ] {
        assert_eq!(names.iter().filter(|n| **n == w).count(), 1, "{w}");
    }
}

// ---- file extras ----------------------------------------------------------------------------

#[test]
fn rename_renames_the_file_and_the_tab_follows() {
    let (d, path, mut app, _) = app_with_file("a\n");
    typ(&mut app, "x");
    ran(&mut app, "/rename b.txt");
    let b = d.path().join("b.txt");
    assert!(!path.exists());
    assert_eq!(
        std::fs::read_to_string(&b).unwrap(),
        "a\n",
        "on disk as saved"
    );
    let doc = app.active_doc().unwrap();
    assert_eq!(doc.path.as_deref(), Some(b.as_path()));
    assert_eq!(text(&app), "xa\n", "unsaved edits kept");
}

#[test]
fn rename_refuses_bad_names_taken_names_and_no_file() {
    let (d, path, mut app, _) = app_with_file("a\n");
    std::fs::write(d.path().join("taken.txt"), "keep").unwrap();
    refused(&mut app, "/rename sub/b.txt", "invalid name");
    refused(&mut app, "/rename ..", "invalid name");
    refused(&mut app, "/rename taken.txt", "already exists");
    refused(&mut app, "/rename", "/rename needs <name>");
    assert!(path.exists());
    assert_eq!(
        std::fs::read_to_string(d.path().join("taken.txt")).unwrap(),
        "keep"
    );
    // Same name: nothing to do, no error.
    ran(&mut app, "/rename a.txt");
    assert!(path.exists());
    let (mut app, _) = test_app(d.path());
    refused(&mut app, "/rename x.txt", "no file is open");
    app.docs.push(vr_core::Document::untitled());
    app.views.push(Default::default());
    refused(&mut app, "/rename x.txt", "has no file name yet");
}

#[test]
fn copy_path_and_relpath_set_the_clipboard() {
    let (d, _p, mut app, clip) = app_with_file("a\n");
    std::fs::create_dir(d.path().join("src")).unwrap();
    let file = d.path().join("src/m.rs");
    std::fs::write(&file, "").unwrap();
    app.open_path(&file).unwrap();
    ran(&mut app, "/copy-path");
    assert_eq!(clip.contents().unwrap(), file.display().to_string());
    ran(&mut app, "/copy-relpath");
    assert_eq!(clip.contents().unwrap(), "src/m.rs");
    // In a second root: relative to that root, not the first one.
    let other = tempfile::tempdir().unwrap();
    let f2 = other.path().join("lib.rs");
    std::fs::write(&f2, "").unwrap();
    app.add_root(other.path()).unwrap();
    app.open_path(&f2).unwrap();
    ran(&mut app, "/copy-relpath");
    assert_eq!(clip.contents().unwrap(), "lib.rs");
    let (mut app, _) = test_app(d.path());
    refused(&mut app, "/copy-path", "no file is open");
    refused(&mut app, "/copy-relpath now", "takes no argument");
}

#[test]
fn copy_relpath_refuses_a_file_outside_the_workspace() {
    let (_d, _p, mut app, clip) = app_with_file("a\n");
    let out = tempfile::tempdir().unwrap();
    let f = out.path().join("x.txt");
    std::fs::write(&f, "").unwrap();
    app.open_path(&f).unwrap();
    refused(&mut app, "/copy-relpath", "not inside a workspace folder");
    assert_eq!(clip.contents(), None);
}

#[test]
fn recent_lists_opened_files_and_opens_by_number_or_name() {
    let (d, path, mut app, _) = app_with_file("a\n");
    let b = d.path().join("b.txt");
    std::fs::write(&b, "b").unwrap();
    app.open_path(&b).unwrap();
    run(&mut app, "/recent");
    assert_eq!(input(&app), "/recent ", "the window lists the files");
    assert_eq!(titles(&app), ["1  b.txt", "2  a.txt"]);
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Enter);
    assert!(app.palette.is_none(), "{:?}", error(&app));
    assert_eq!(
        app.active_doc().unwrap().path.as_deref(),
        Some(path.as_path())
    );
    ran(&mut app, "/recent 2");
    assert_eq!(app.active_doc().unwrap().path.as_deref(), Some(b.as_path()));
    ran(&mut app, "/recent a.t");
    assert_eq!(
        app.active_doc().unwrap().path.as_deref(),
        Some(path.as_path())
    );
    refused(&mut app, "/recent 9", "no recent file 9");
    refused(&mut app, "/recent zzz", "no recent file zzz");
}

#[test]
fn recent_files_are_kept_in_the_state_folder() {
    let state = tempfile::tempdir().unwrap();
    let list = state.path().join(vr_config::RECENT_FILES_FILE);
    let (d, _p, mut app, _) = app_with_file("a\n");
    app.recent_files_file = Some(list.clone());
    let b = d.path().join("b.txt");
    std::fs::write(&b, "b").unwrap();
    app.open_path(&b).unwrap();
    app.flush_recent_files();
    let (on_disk, w) = vr_config::RecentFiles::load(&list);
    assert!(w.is_none());
    assert_eq!(on_disk.entries[0], b);
    // Another vr-ide's entries are kept: the writer merges with the file.
    let mut other = on_disk.clone();
    other.push(PathBuf::from("/elsewhere/z.rs"));
    other.save(&list).unwrap();
    app.open_path(&d.path().join("a.txt")).unwrap();
    app.flush_recent_files();
    let (on_disk, _) = vr_config::RecentFiles::load(&list);
    assert_eq!(on_disk.entries[0], d.path().join("a.txt"));
    assert!(on_disk.entries.contains(&PathBuf::from("/elsewhere/z.rs")));
    assert!(on_disk.entries.len() <= vr_config::RECENT_FILES_MAX);
}

// ---- text commands --------------------------------------------------------------------------

#[test]
fn sort_sorts_the_selected_lines_or_the_file_in_one_undo_step() {
    let (_d, _p, mut app, _) = app_with_file("c\nb\na\nz\n");
    // Lines 0..=1 ("c", "b").
    select(&mut app, 0, 3);
    ran(&mut app, "/sort");
    assert_eq!(text(&app), "b\nc\na\nz\n");
    undo(&mut app);
    assert_eq!(text(&app), "c\nb\na\nz\n");
    select(&mut app, 0, 0);
    ran(&mut app, "/sort");
    assert_eq!(text(&app), "a\nb\nc\nz\n");
    assert!(app.active_doc().unwrap().is_modified());
    undo(&mut app);
    assert_eq!(text(&app), "c\nb\na\nz\n");
}

#[test]
fn unique_upper_lower_trim() {
    let (_d, _p, mut app, _) = app_with_file("b\na  \nb\t\nA\n");
    ran(&mut app, "/trim");
    assert_eq!(text(&app), "b\na\nb\nA\n");
    ran(&mut app, "/unique");
    assert_eq!(text(&app), "b\na\nA\n");
    ran(&mut app, "/upper");
    assert_eq!(text(&app), "B\nA\nA\n");
    select(&mut app, 0, 1);
    ran(&mut app, "/lower");
    assert_eq!(text(&app), "b\nA\nA\n");
    undo(&mut app);
    assert_eq!(text(&app), "B\nA\nA\n", "one undo step each");
    // Nothing to change: a notice, no error, no undo step added.
    ran(&mut app, "/upper");
    assert!(
        notices(&app).contains("nothing to change"),
        "{}",
        notices(&app)
    );
    refused(&mut app, "/sort now", "/sort takes no argument");
}

#[test]
fn text_commands_need_edit_mode_and_a_file() {
    let (d, path, _app, _) = app_with_file("b\na\n");
    let (mut app, _) = test_app(d.path());
    app.open_path(&path).unwrap();
    assert!(!app.edit_mode(), "a file opens in read mode");
    for cmd in [
        "/sort",
        "/unique",
        "/upper",
        "/lower",
        "/trim",
        "/indent 2",
        "/eol crlf",
    ] {
        run(&mut app, cmd);
        assert_eq!(text(&app), "b\na\n", "{cmd}");
        assert!(
            notices(&app).contains("Read mode"),
            "{cmd}: {}",
            notices(&app)
        );
    }
    let (mut app, _) = test_app(d.path());
    refused(&mut app, "/sort", "no file is open");
}

#[test]
fn indent_reindents_and_sets_the_indent_unit() {
    let (_d, _p, mut app, _) = app_with_file("a\n    b\n        c\n     d\n");
    ran(&mut app, "/indent 2");
    assert_eq!(
        text(&app),
        "a\n  b\n    c\n   d\n",
        "levels kept, the rest kept as spaces"
    );
    assert_eq!(app.active_doc().unwrap().buffer.options().indent_unit, "  ");
    ran(&mut app, "/indent tab");
    assert_eq!(text(&app), "a\n\tb\n\t\tc\n\t d\n");
    assert_eq!(app.active_doc().unwrap().buffer.options().indent_unit, "\t");
    // Only the selected lines.
    select(&mut app, 2, 3);
    ran(&mut app, "/indent 4");
    assert_eq!(text(&app), "a\n    b\n\t\tc\n\t d\n");
    refused(&mut app, "/indent 3", "/indent: 3 is not one of 2, 4, tab");
    // A list argument left empty: Enter completes the name (D268), it does not guess.
    run(&mut app, "/indent");
    assert_eq!((input(&app), error(&app)), ("/indent ".to_string(), None));
    key(&mut app, KeyCode::Esc);
}

#[test]
fn eol_converts_the_line_breaks_and_marks_the_file_changed() {
    let (_d, path, mut app, _) = app_with_file("a\nb\n");
    ran(&mut app, "/eol crlf");
    assert_eq!(text(&app), "a\r\nb\r\n");
    let doc = app.active_doc().unwrap();
    assert!(doc.is_modified());
    assert_eq!(doc.line_ending, vr_core::document::LineEnding::CrLf);
    app.run_command("file.save", crate::commands::CmdArgs::None);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "a\r\nb\r\n");
    ran(&mut app, "/eol lf");
    assert_eq!(text(&app), "a\nb\n");
    ran(&mut app, "/eol lf");
    assert!(notices(&app).contains("already"), "{}", notices(&app));
    refused(&mut app, "/eol cr", "/eol: cr is not one of lf, crlf");
}

// ---- language and settings ------------------------------------------------------------------

#[test]
fn lang_overrides_the_language_and_completes_ids() {
    let (_d, _p, mut app, _) = app_with_file("fn main() {}\n");
    app.sync_syntax();
    assert_eq!(
        app.syntax.language(app.active),
        None,
        "a .txt file is plain"
    );
    slash(&mut app, "/lang ru");
    assert!(
        titles(&app).contains(&"rust".to_string()),
        "{:?}",
        titles(&app)
    );
    key(&mut app, KeyCode::Esc);
    ran(&mut app, "/lang rust");
    app.sync_syntax();
    assert_eq!(app.syntax.language(app.active), Some("rust"));
    refused(&mut app, "/lang klingon", "no language klingon");
    run(&mut app, "/lang");
    assert_eq!((input(&app), error(&app)), ("/lang ".to_string(), None));
    key(&mut app, KeyCode::Esc);
    // Works in read mode too: it does not change the text.
    app.enter_read_mode();
    ran(&mut app, "/lang python");
    app.sync_syntax();
    assert_eq!(app.syntax.language(app.active), Some("python"));
}

#[test]
fn set_changes_a_setting_for_the_session_with_type_checks() {
    let (_d, _p, mut app, _) = app_with_file("a\n");
    let cfg = tempfile::tempdir().unwrap();
    app.config_dir = Some(cfg.path().to_path_buf());
    ran(&mut app, "/set tab_width 2");
    assert_eq!(app.settings.tab_width, 2);
    let opts = app.active_doc().unwrap().buffer.options().clone();
    assert_eq!((opts.tab_width, opts.indent_unit.as_str()), (2, "  "));
    assert!(
        !cfg.path().join("settings.toml").exists(),
        "session only: settings.toml is not written"
    );
    ran(&mut app, "/set theme dracula");
    assert_eq!(app.settings.theme, "dracula");
    ran(&mut app, "/set show_hidden false");
    assert!(!app.explorer.tree.show_hidden());
    ran(&mut app, "/set terminal_height_percent 30");
    assert_eq!(app.terminal.height_percent, 30);
    refused(&mut app, "/set nope 1", "unknown setting nope");
    refused(&mut app, "/set tab_width abc", "tab_width");
    refused(&mut app, "/set tab_width 0", "between 1 and 16");
    refused(&mut app, "/set insert_spaces maybe", "insert_spaces");
    refused(&mut app, "/set theme pink", "theme: pink is not one of");
    refused(&mut app, "/set tab_width", "tab_width needs a value");
    refused(&mut app, "/set", "/set needs <name> <value>");
    assert_eq!(app.settings.tab_width, 2, "refused values change nothing");
}

#[test]
fn set_completes_names_then_values() {
    let (mut app, _) = test_app(Path::new("/p"));
    slash(&mut app, "/set tab");
    assert_eq!(titles(&app), ["tab_width  4"]);
    key(&mut app, KeyCode::Tab);
    assert_eq!(input(&app), "/set tab_width ");
    set(&mut app, "/set insert_spaces ");
    assert_eq!(titles(&app), ["true", "false"]);
    set(&mut app, "/set theme d");
    assert_eq!(titles(&app), ["dracula", "dark"]);
    key(&mut app, KeyCode::Tab);
    assert_eq!(
        input(&app),
        "/set theme dracula",
        "nothing more is shared: the highlighted one"
    );
}

#[test]
fn config_and_keymap_open_the_user_files_creating_them_from_the_defaults() {
    let (d, _p, mut app, _) = app_with_file("a\n");
    let cfg = d.path().join("cfg");
    app.config_dir = Some(cfg.clone());
    ran(&mut app, "/config");
    let settings = cfg.join("settings.toml");
    assert_eq!(
        app.active_doc().unwrap().path.as_deref(),
        Some(settings.as_path())
    );
    let made = std::fs::read_to_string(&settings).unwrap();
    assert!(made.contains("# tab_width = 4"), "{made}");
    ran(&mut app, "/keymap");
    let keymap = cfg.join("keymap.toml");
    assert_eq!(
        app.active_doc().unwrap().path.as_deref(),
        Some(keymap.as_path())
    );
    // An existing file is opened as it is.
    std::fs::write(&settings, "tab_width = 3\n").unwrap();
    app.close_doc(
        app.docs
            .iter()
            .position(|d| d.path.as_deref() == Some(settings.as_path()))
            .unwrap(),
    );
    ran(&mut app, "/config");
    assert_eq!(
        std::fs::read_to_string(&settings).unwrap(),
        "tab_width = 3\n"
    );
    assert_eq!(text(&app), "tab_width = 3\n");
    let (mut app, _) = test_app(d.path());
    refused(&mut app, "/config", "no configuration folder");
}

#[test]
fn keys_opens_a_read_only_table_from_the_keymap() {
    let (_d, _p, mut app, _) = app_with_file("a\n");
    ran(&mut app, "/keys");
    let i = app.active;
    assert_eq!(app.views[i].title, Some("keys"));
    let doc = app.active_doc().unwrap();
    assert!(doc.read_only && doc.path.is_none());
    let table = text(&app);
    assert!(
        table.contains("ctrl+s") && table.contains("file.save"),
        "{table}"
    );
    for (ctx, k, cmd) in app.keymap.bindings() {
        assert!(
            table.lines().any(|l| l.contains(&k.to_string())
                && l.contains(cmd)
                && l.contains(&ctx.to_string())),
            "{ctx} {k} {cmd}"
        );
    }
    // The tab bar names it.
    let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
    t.draw(|f| crate::ui::render(f, &mut app)).unwrap();
    let screen = format!("{:?}", t.backend());
    assert!(screen.contains(" keys "), "{screen}");
    // Not editable.
    app.handle_paste("x");
    typ(&mut app, "y");
    assert_eq!(text(&app), table);
    assert!(!app.active_doc().unwrap().is_modified());
    // A second /keys shows the same tab again.
    app.set_active(0);
    ran(&mut app, "/keys");
    assert_eq!(app.docs.len(), 2);
    assert_eq!(app.active, i);
    // Closing needs no question.
    app.run_command("file.close", crate::commands::CmdArgs::None);
    assert_eq!(app.docs.len(), 1);
    assert!(app.dialog.is_none());
}

#[test]
fn ws_save_onto_another_existing_file_says_it_is_not_replaced() {
    let d = tempfile::tempdir().unwrap();
    let (mut app, _) = test_app(d.path());
    std::fs::write(d.path().join("x.vr-workspace"), "keep").unwrap();
    refused(
        &mut app,
        "/ws save x",
        "/ws save does not replace another file",
    );
}

// ---- review round 1 (05_review_T20b.md) -----------------------------------------------------

#[test]
fn indent_ignores_block_comment_leads_when_finding_the_step() {
    let c = "/**\n * doc\n */\nint f() {\n    if (x) {\n        y();\n    }\n}\n";
    let (_d, _p, mut app, _) = app_with_file(c);
    ran(&mut app, "/indent 2");
    assert_eq!(
        text(&app),
        "/**\n * doc\n */\nint f() {\n  if (x) {\n    y();\n  }\n}\n"
    );
    // A doc comment inside a class: one level plus the comment's column.
    let java = "class A {\n    /**\n     * x\n     */\n    void f() {\n        g();\n    }\n}\n";
    let (_d, _p, mut app, _) = app_with_file(java);
    ran(&mut app, "/indent 2");
    assert_eq!(
        text(&app),
        "class A {\n  /**\n   * x\n   */\n  void f() {\n    g();\n  }\n}\n"
    );
}

#[test]
fn indent_on_a_selection_takes_the_step_from_the_whole_file() {
    let py = "def f():\n    if a:\n        b()\n        c()\n";
    let (_d, _p, mut app, _) = app_with_file(py);
    // Lines 2..=3 ("        b()", "        c()").
    let start = "def f():\n    if a:\n".len();
    select(&mut app, start, py.len() - 1);
    ran(&mut app, "/indent 2");
    assert_eq!(text(&app), "def f():\n    if a:\n    b()\n    c()\n");
    // The whole file gives the same levels.
    let (_d, _p, mut app, _) = app_with_file(py);
    ran(&mut app, "/indent 2");
    assert_eq!(text(&app), "def f():\n  if a:\n    b()\n    c()\n");
}

#[test]
fn set_of_an_unrelated_key_keeps_the_explorer_toggles_and_terminal_height() {
    let (_d, _p, mut app, _) = app_with_file("a\n");
    let hidden = app.explorer.tree.show_hidden();
    let ignored = app.explorer.tree.show_ignored();
    app.run_command("explorer.toggle_hidden", crate::commands::CmdArgs::None);
    app.run_command("explorer.toggle_ignored", crate::commands::CmdArgs::None);
    let grown = app.terminal.height_percent + 10;
    app.terminal.height_percent = grown;
    ran(&mut app, "/set tab_width 2");
    ran(&mut app, "/set theme dracula");
    assert_eq!(
        app.explorer.tree.show_hidden(),
        !hidden,
        "ctrl+h state kept"
    );
    assert_eq!(app.explorer.tree.show_ignored(), !ignored);
    assert_eq!(app.terminal.height_percent, grown, "grow/shrink kept");
    assert!(
        !notices(&app).contains("hidden files shown"),
        "{}",
        notices(&app)
    );
    // The changed key itself still applies.
    ran(&mut app, "/set terminal_height_percent 30");
    assert_eq!(app.terminal.height_percent, 30);
}

#[test]
fn rename_updates_the_recent_files_list() {
    let state = tempfile::tempdir().unwrap();
    let list = state.path().join(vr_config::RECENT_FILES_FILE);
    let (d, a, mut app, _) = app_with_file("a\n");
    app.recent_files_file = Some(list.clone());
    app.open_path(&a).unwrap();
    ran(&mut app, "/rename b.txt");
    let b = d.path().join("b.txt");
    assert_eq!(app.recent_files.entries.first(), Some(&b));
    assert!(
        !app.recent_files.entries.contains(&a),
        "{:?}",
        app.recent_files
    );
    app.flush_recent_files();
    let (on_disk, _) = vr_config::RecentFiles::load(&list);
    assert_eq!(on_disk.entries.first(), Some(&b));
    assert!(!on_disk.entries.contains(&a), "{:?}", on_disk.entries);
}

#[test]
fn eol_on_a_file_without_line_breaks_sets_the_style_for_new_lines() {
    let (_d, path, mut app, _) = app_with_file("one line");
    ran(&mut app, "/eol crlf");
    assert!(
        notices(&app).contains("no line breaks yet"),
        "{}",
        notices(&app)
    );
    let doc = app.active_doc().unwrap();
    assert_eq!(doc.line_ending, vr_core::document::LineEnding::CrLf);
    key(&mut app, KeyCode::End);
    key(&mut app, KeyCode::Enter);
    assert_eq!(text(&app), "one line\r\n");
    app.run_command("file.save", crate::commands::CmdArgs::None);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "one line\r\n");
}
