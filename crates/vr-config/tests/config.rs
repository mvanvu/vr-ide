use std::fs;
use std::path::PathBuf;

use vr_config::{ConfigWarning, Context, KeyChord, KeyCode, Keymap, Mods, Settings};

fn chord(s: &str) -> KeyChord {
    s.parse()
        .unwrap_or_else(|e| panic!("{s:?} should parse: {e}"))
}

fn write_user(contents: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("user.toml");
    fs::write(&path, contents).expect("write user file");
    (dir, path)
}

/// Every command name the plan's registry lists, plus the M5 reserved one.
const KNOWN: &[&str] = &[
    "app.quit",
    "app.palette",
    "file.save",
    "file.save_as",
    "file.close",
    "file.open",
    "tab.next",
    "tab.prev",
    "edit.undo",
    "edit.redo",
    "edit.copy",
    "edit.cut",
    "edit.paste",
    "edit.select_all",
    "edit.delete_line",
    "edit.duplicate_line",
    "edit.move_line_up",
    "edit.move_line_down",
    "edit.indent",
    "edit.outdent",
    "edit.toggle_comment",
    "nav.goto_line",
    "file.reload",
    "explorer.toggle",
    "explorer.focus",
    "explorer.reveal",
    "explorer.new_file",
    "explorer.new_folder",
    "explorer.rename",
    "explorer.copy",
    "explorer.paste",
    "explorer.move",
    "explorer.delete",
    "explorer.toggle_ignored",
    "explorer.toggle_hidden",
    "file.goto",
    "find.open",
    "find.next",
    "find.prev",
    "find.replace_open",
    "find.replace_one",
    "find.replace_all",
    "search.project",
    "search.next_result",
    "search.prev_result",
    "search.replace_project",
    "focus.cycle",
];

#[test]
fn parse_chords() {
    let ctrl = Mods {
        ctrl: true,
        ..Mods::NONE
    };
    let cases: &[(&str, KeyCode, Mods)] = &[
        (
            "ctrl+shift+f",
            KeyCode::Char('f'),
            Mods {
                ctrl: true,
                shift: true,
                alt: false,
            },
        ),
        (
            "alt+up",
            KeyCode::Up,
            Mods {
                alt: true,
                ..Mods::NONE
            },
        ),
        ("ctrl+pagedown", KeyCode::PageDown, ctrl),
        ("ctrl+/", KeyCode::Char('/'), ctrl),
        ("ctrl+`", KeyCode::Char('`'), ctrl),
        ("ctrl+]", KeyCode::Char(']'), ctrl),
        ("ctrl+[", KeyCode::Char('['), ctrl),
        (
            "shift+alt+down",
            KeyCode::Down,
            Mods {
                shift: true,
                alt: true,
                ctrl: false,
            },
        ),
        ("f1", KeyCode::F(1), Mods::NONE),
        ("f12", KeyCode::F(12), Mods::NONE),
        (
            "shift+f3",
            KeyCode::F(3),
            Mods {
                shift: true,
                ..Mods::NONE
            },
        ),
        ("pageup", KeyCode::PageUp, Mods::NONE),
        ("delete", KeyCode::Delete, Mods::NONE),
        ("ctrl+space", KeyCode::Char(' '), ctrl),
        ("enter", KeyCode::Enter, Mods::NONE),
        ("escape", KeyCode::Esc, Mods::NONE),
        // Case and whitespace are not significant; letters are stored lowercase.
        (" Ctrl + S ", KeyCode::Char('s'), ctrl),
        ("ctrl+plus", KeyCode::Char('+'), ctrl),
    ];
    for (s, code, mods) in cases {
        let k: KeyChord = chord(s);
        assert_eq!(k.code, *code, "code of {s:?}");
        assert_eq!(k.mods, *mods, "mods of {s:?}");
    }
    for bad in [
        "",
        "ctrl+",
        "+",
        "ctrl+shift",
        "hyper+a",
        "f0",
        "f25",
        "ab",
        "ctrl+ctrl+a",
    ] {
        assert!(bad.parse::<KeyChord>().is_err(), "{bad:?} must not parse");
    }
}

#[test]
fn display_roundtrip() {
    for s in [
        "ctrl+shift+f",
        "alt+up",
        "ctrl+pagedown",
        "ctrl+/",
        "ctrl+`",
        "ctrl+]",
        "shift+alt+down",
        "f12",
        "pageup",
        "delete",
        "ctrl+space",
        "ctrl+plus",
        "ctrl+shift+alt+enter",
    ] {
        let k = chord(s);
        let shown = k.to_string();
        assert_eq!(chord(&shown), k, "{s:?} -> {shown:?}");
    }
    assert_eq!(chord("shift+ctrl+P").to_string(), "ctrl+shift+p");
    assert_eq!(chord("ctrl+ ").to_string(), "ctrl+space");
}

#[test]
fn chord_new_normalizes_uppercase_letters() {
    // Terminals report ctrl+shift+z as an uppercase 'Z'; the keymap stores lowercase.
    let k = KeyChord::new(
        KeyCode::Char('Z'),
        Mods {
            ctrl: true,
            shift: true,
            alt: false,
        },
    );
    assert_eq!(k, chord("ctrl+shift+z"));
}

#[test]
fn defaults_load_without_user_file() {
    let (km, warnings) = Keymap::load(None);
    assert!(warnings.is_empty(), "{warnings:?}");
    let expect = [
        ("ctrl+s", "file.save"),
        ("ctrl+shift+s", "file.save_as"),
        ("ctrl+shift+p", "app.palette"),
        ("f1", "app.palette"),
        ("ctrl+p", "file.goto"),
        ("ctrl+shift+f", "search.project"),
        ("ctrl+shift+h", "search.replace_project"),
        ("ctrl+q", "app.quit"),
        ("ctrl+w", "file.close"),
        ("ctrl+pagedown", "tab.next"),
        ("ctrl+pageup", "tab.prev"),
        ("ctrl+b", "explorer.toggle"),
        ("ctrl+shift+e", "explorer.focus"),
        ("ctrl+z", "edit.undo"),
        ("ctrl+shift+z", "edit.redo"),
        ("ctrl+y", "edit.redo"),
        ("f3", "find.next"),
        ("shift+f3", "find.prev"),
        ("ctrl+]", "edit.indent"),
        ("ctrl+[", "edit.outdent"),
        ("shift+alt+down", "edit.duplicate_line"),
        ("ctrl+shift+k", "edit.delete_line"),
        ("ctrl+/", "edit.toggle_comment"),
        ("ctrl+g", "nav.goto_line"),
    ];
    for (k, cmd) in expect {
        assert_eq!(km.command_for(&chord(k)), Some(cmd), "{k}");
    }
    // Explorer-only keys act in the explorer and not in the editor.
    assert_eq!(
        km.command_for_in(Context::Explorer, &chord("f2")),
        Some("explorer.rename")
    );
    assert_eq!(
        km.command_for_in(Context::Explorer, &chord("delete")),
        Some("explorer.delete")
    );
    assert_eq!(km.command_for(&chord("f2")), None);
    assert_eq!(km.command_for(&chord("delete")), None);
    // Context bindings shadow global/editor ones; globals still reach the explorer.
    assert_eq!(
        km.command_for_in(Context::Explorer, &chord("ctrl+c")),
        Some("explorer.copy")
    );
    assert_eq!(km.command_for(&chord("ctrl+c")), Some("edit.copy"));
    assert_eq!(
        km.command_for_in(Context::Explorer, &chord("ctrl+s")),
        Some("file.save")
    );
    // D8: a focused terminal gets every key except alt+q.
    assert_eq!(
        km.command_for_in(Context::Terminal, &chord("alt+q")),
        Some("focus.editor")
    );
    assert_eq!(km.command_for_in(Context::Terminal, &chord("ctrl+s")), None);
    // Defaults validate clean against the M1-M3 registry (reserved names are accepted).
    assert!(km.validate(KNOWN).is_empty(), "{:?}", km.validate(KNOWN));
}

#[test]
fn missing_user_file_is_not_a_warning() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (km, warnings) = Keymap::load(Some(&dir.path().join("keymap.toml")));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(km.command_for(&chord("ctrl+s")), Some("file.save"));
}

#[test]
fn user_overrides_single_key() {
    let (_d, path) = write_user("\"ctrl+s\" = \"file.save_as\"\n");
    let (defaults, _) = Keymap::load(None);
    let (km, warnings) = Keymap::load(Some(&path));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(km.command_for(&chord("ctrl+s")), Some("file.save_as"));
    // Nothing else changed.
    let mut a: Vec<_> = defaults
        .bindings()
        .filter(|(_, k, _)| **k != chord("ctrl+s"))
        .map(|(c, k, v)| (c, k.clone(), v.to_owned()))
        .collect();
    let mut b: Vec<_> = km
        .bindings()
        .filter(|(_, k, _)| **k != chord("ctrl+s"))
        .map(|(c, k, v)| (c, k.clone(), v.to_owned()))
        .collect();
    a.sort_by_key(|(c, k, v)| (*c, k.to_string(), v.clone()));
    b.sort_by_key(|(c, k, v)| (*c, k.to_string(), v.clone()));
    assert_eq!(a, b);
}

#[test]
fn ac5_unquoted_key_like_the_spec_example_resolves() {
    // AC-5 writes the binding without TOML quotes; the loader accepts it.
    let (_d, path) = write_user(
        "# my keys\nctrl+s = \"file.save\"\nctrl+= = \"edit.indent\"\n[explorer]\nshift+delete = \"explorer.delete\"\n",
    );
    let (km, warnings) = Keymap::load(Some(&path));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(km.command_for(&chord("ctrl+s")), Some("file.save"));
    assert_eq!(km.command_for(&chord("ctrl+plus")), None);
    assert_eq!(
        km.command_for(&KeyChord::new(
            KeyCode::Char('='),
            Mods {
                ctrl: true,
                ..Mods::NONE
            }
        )),
        Some("edit.indent")
    );
    assert_eq!(
        km.command_for_in(Context::Explorer, &chord("shift+delete")),
        Some("explorer.delete")
    );
}

#[test]
fn when_field_form_binds_in_context() {
    let (_d, path) =
        write_user("\"ctrl+alt+r\" = { command = \"explorer.reveal\", when = \"explorer\" }\n");
    let (km, warnings) = Keymap::load(Some(&path));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(
        km.command_for_in(Context::Explorer, &chord("ctrl+alt+r")),
        Some("explorer.reveal")
    );
    assert_eq!(km.command_for(&chord("ctrl+alt+r")), None);
}

#[test]
fn user_unbinds_with_empty_string() {
    let (_d, path) = write_user("\"ctrl+q\" = \"\"\n[explorer]\n\"f2\" = \"\"\n");
    let (km, warnings) = Keymap::load(Some(&path));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(km.command_for(&chord("ctrl+q")), None);
    assert_eq!(km.command_for_in(Context::Explorer, &chord("f2")), None);
    assert!(km.keys_for("app.quit").is_empty());
    assert_eq!(km.command_for(&chord("ctrl+s")), Some("file.save"));
}

#[test]
fn unknown_command_reported() {
    let (_d, path) = write_user("\"ctrl+k\" = \"file.saev\"\n");
    let (km, warnings) = Keymap::load(Some(&path));
    assert!(
        warnings.is_empty(),
        "load itself does not know the registry"
    );
    let w = km.validate(KNOWN);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(
        matches!(&w[0], ConfigWarning::UnknownCommand { key, command, .. }
        if key == "ctrl+k" && command == "file.saev")
    );
    let text = w[0].to_string();
    assert!(
        text.contains("ctrl+k") && text.contains("file.saev"),
        "{text}"
    );
}

#[test]
fn bad_entries_reported_rest_kept() {
    let (_d, path) = write_user(
        "\"ctrl+\" = \"file.save\"\n\"ctrl+k\" = 3\n\"ctrl+j\" = \"edit.indent\"\n[sidebar]\n\"f2\" = \"x\"\n",
    );
    let (km, warnings) = Keymap::load(Some(&path));
    assert_eq!(warnings.len(), 3, "{warnings:?}");
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, ConfigWarning::InvalidKey { key, .. } if key == "ctrl+"))
    );
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, ConfigWarning::InvalidValue { key, .. } if key == "ctrl+k"))
    );
    assert!(
        warnings
            .iter()
            .any(|w| matches!(w, ConfigWarning::UnknownContext { name, .. } if name == "sidebar"))
    );
    assert_eq!(km.command_for(&chord("ctrl+j")), Some("edit.indent"));
}

#[test]
fn malformed_toml_reported_defaults_kept() {
    let (_d, path) = write_user("\"ctrl+s\" = \"file.save_as\"\n[[[ not toml\n");
    let (km, warnings) = Keymap::load(Some(&path));
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(matches!(&warnings[0], ConfigWarning::Malformed { file, .. } if *file == path));
    assert_eq!(km.command_for(&chord("ctrl+s")), Some("file.save"));
    let (defaults, _) = Keymap::load(None);
    assert_eq!(km.bindings().count(), defaults.bindings().count());

    let (_d2, spath) = write_user("tab_width = [\n");
    let (settings, w) = Settings::load(Some(&spath));
    assert_eq!(w.len(), 1, "{w:?}");
    assert_eq!(settings, Settings::default());
}

#[test]
fn keys_for_lists_all_bindings() {
    let (km, _) = Keymap::load(None);
    let mut palette: Vec<String> = km
        .keys_for("app.palette")
        .iter()
        .map(ToString::to_string)
        .collect();
    palette.sort();
    assert_eq!(palette, ["ctrl+shift+p", "f1"]);
    let redo: Vec<String> = km
        .keys_for("edit.redo")
        .iter()
        .map(ToString::to_string)
        .collect();
    assert!(redo.contains(&"ctrl+shift+z".to_owned()) && redo.contains(&"ctrl+y".to_owned()));
    assert!(km.keys_for("no.such").is_empty());
}

#[test]
fn settings_defaults_and_override() {
    let (s, w) = Settings::load(None);
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(s, Settings::default());
    assert_eq!(s.tab_width, 4);
    assert!(s.insert_spaces);
    assert_eq!(s.scrollback, 10_000);
    let names: Vec<&str> = s.ai_presets.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["claude", "cursor-agent"]);
    assert_eq!(s.ai_presets[0].command, "claude");

    let (_d, path) = write_user(
        "tab_width = 2\nshow_hidden = false\nbogus = 1\ntheme = 7\n\n[[ai_presets]]\nname = \"aider\"\ncommand = \"aider\"\nargs = [\"--no-auto-commits\"]\n",
    );
    let (s2, w2) = Settings::load(Some(&path));
    assert_eq!(s2.tab_width, 2);
    assert!(!s2.show_hidden);
    assert_eq!(s2.theme, s.theme, "bad value keeps the default");
    assert_eq!(s2.insert_spaces, s.insert_spaces);
    assert_eq!(s2.ai_presets.len(), 1);
    assert_eq!(s2.ai_presets[0].args, ["--no-auto-commits"]);
    assert_eq!(w2.len(), 2, "{w2:?}");
    assert!(
        w2.iter()
            .any(|w| matches!(w, ConfigWarning::UnknownSetting { key, .. } if key == "bogus"))
    );
    assert!(
        w2.iter()
            .any(|w| matches!(w, ConfigWarning::InvalidValue { key, .. } if key == "theme"))
    );
}
