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

/// Keyset v2 (T21, D261, spec 01_spec_T21.md): every default key, (context, key, command).
/// Each command has exactly this one key. Built-in keys (cursor movement, typing, tab, the
/// read-mode `space` sequences) are not in the keymap.
const KEYSET_V2: &[(&str, &str, &str)] = &[
    // Edit mode (editor table; refused with the read-mode hint in read mode).
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
    // Both modes.
    ("editor", "ctrl+a", "edit.select_all"),
    ("editor", "ctrl+c", "edit.copy"),
    ("editor", "ctrl+f", "find.open"),
    ("editor", "f4", "find.next"),
    ("editor", "shift+f4", "find.prev"),
    ("editor", "ctrl+r", "find.replace_open"),
    ("global", "ctrl+r", "search.replace_project"),
    ("global", "ctrl+s", "file.save"),
    ("global", "ctrl+o", "file.open"),
    ("global", "ctrl+p", "file.goto"),
    ("global", "ctrl+n", "explorer.new_file"),
    ("global", "ctrl+w", "file.close"),
    ("editor", "ctrl+g", "nav.goto_line"),
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
    ("global", "f1", "app.palette"),
    ("global", "ctrl+q", "app.quit"),
    // Explorer focus. ctrl+n there asks File / Folder first (user change to the spec).
    ("explorer", "f2", "explorer.rename"),
    ("explorer", "ctrl+n", "explorer.new"),
    ("explorer", "delete", "explorer.delete"),
    ("explorer", "ctrl+c", "explorer.copy"),
    ("explorer", "ctrl+x", "explorer.cut"),
    ("explorer", "ctrl+v", "explorer.paste"),
];

/// Keys the spec removed (T21 "Removed"; ctrl+f2 by the user's later change; f3 / shift+f3,
/// ctrl+f4 and ctrl+f12 by D279 / D280): bound nowhere. ctrl+f7, kept free for T19, is the
/// panel position key now.
const REMOVED_V2: &[&str] = &[
    "ctrl+i", "ctrl+]", "ctrl+u", "f7", "shift+f7", "ctrl+t", "f3", "shift+f3", "f6", "ctrl+f1",
    "ctrl+f2", "ctrl+f3", "ctrl+f5", "f12", "f5", "ctrl+f4", "ctrl+f12",
];

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
    "editor.edit_mode",
    "editor.read_mode",
    "nav.goto_line",
    "file.reload",
    "explorer.toggle",
    "explorer.toggle_focus",
    "explorer.focus",
    "explorer.reveal",
    "explorer.new",
    "explorer.new_file",
    "explorer.new_folder",
    "explorer.rename",
    "explorer.copy",
    "explorer.cut",
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
    "focus.editor",
    "terminal.toggle",
    "terminal.next",
    "terminal.prev",
    "terminal.grow",
    "terminal.hide",
    "terminal.shrink",
    "terminal.toggle_position",
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
    // T21: each command has exactly this one key, in this context.
    for &(ctx, k, cmd) in KEYSET_V2 {
        let ctx = Context::from_name(ctx).unwrap();
        assert_eq!(km.command_for_in(ctx, &chord(k)), Some(cmd), "{ctx} {k}");
        assert_eq!(km.keys_for(cmd), [chord(k)], "{cmd} has one key");
    }
    // Nothing is bound that the table does not list (the terminal table repeats ctrl+`, D8, and
    // f1, D279).
    for (ctx, k, cmd) in km.bindings() {
        let listed = KEYSET_V2.iter().any(|&(c, key, command)| {
            Context::from_name(c) == Some(ctx) && chord(key) == *k && command == cmd
        });
        let terminal_repeat = ctx == Context::Terminal
            && [("ctrl+`", "terminal.toggle"), ("f1", "app.palette")]
                .iter()
                .any(|&(key, command)| chord(key) == *k && command == cmd);
        assert!(
            listed || terminal_repeat,
            "{ctx} {k} = {cmd} is not in the T21 table"
        );
    }
    // AC-21d: removed keys are free everywhere.
    for k in REMOVED_V2 {
        for ctx in [Context::Editor, Context::Explorer, Context::Global] {
            assert_eq!(km.command_for_in(ctx, &chord(k)), None, "{ctx} {k}");
        }
    }
    // Commands that lost their key run from the palette; the read-mode keys are built in.
    for cmd in [
        "search.project",
        "search.next_result",
        "search.prev_result",
        "find.replace_one",
        "find.replace_all",
        "explorer.new_folder",
        "edit.indent",
        "focus.cycle",
        "file.save_as",
        "file.reload",
        "editor.edit_mode",
    ] {
        assert!(km.keys_for(cmd).is_empty(), "{cmd} has no key");
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
    // D108: VS Code's explorer cut; the editor keeps edit.cut.
    assert_eq!(
        km.command_for_in(Context::Explorer, &chord("ctrl+x")),
        Some("explorer.cut")
    );
    assert_eq!(km.command_for(&chord("ctrl+x")), Some("edit.cut"));
    assert_eq!(
        km.command_for_in(Context::Explorer, &chord("ctrl+s")),
        Some("file.save")
    );
    // D8/D230: a focused terminal gets every key except ctrl+` and f1 (D279: the command window
    // opens from the terminal too).
    assert_eq!(
        km.command_for_in(Context::Terminal, &chord("f1")),
        Some("app.palette")
    );
    assert_eq!(
        km.command_for_in(Context::Terminal, &chord("shift+f1")),
        None
    );
    assert_eq!(km.command_for_in(Context::Terminal, &chord("alt+q")), None);
    assert_eq!(km.command_for_in(Context::Terminal, &chord("ctrl+s")), None);
    // M5 (D185): ctrl+` toggles the panel everywhere, in a focused terminal too; ctrl+space
    // (the legacy NUL) is not bound in the terminal. Only ctrl+` is reserved there.
    for ctx in [Context::Editor, Context::Explorer, Context::Terminal] {
        assert_eq!(
            km.command_for_in(ctx, &chord("ctrl+`")),
            Some("terminal.toggle")
        );
    }
    assert_eq!(km.command_for_in(Context::Terminal, &chord("alt+t")), None);
    assert_eq!(
        km.command_for_in(Context::Terminal, &chord("ctrl+space")),
        None
    );
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
    assert_eq!(palette, ["f1"]);
    let redo: Vec<String> = km
        .keys_for("edit.redo")
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(redo, ["ctrl+y"]);
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
    assert_eq!(s.terminal_height_percent, 40);
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

/// M5: the terminal panel height is a share of the screen, 10..=90 %.
#[test]
fn terminal_height_percent_is_bounded() {
    let (_d, path) = write_user("terminal_height_percent = 60\n");
    assert_eq!(Settings::load(Some(&path)).0.terminal_height_percent, 60);
    for bad in ["0", "5", "95", "-1", "\"big\""] {
        let (_d, path) = write_user(&format!("terminal_height_percent = {bad}\n"));
        let (s, w) = Settings::load(Some(&path));
        assert_eq!(s.terminal_height_percent, 40, "{bad}");
        assert!(
            w.iter().any(|w| matches!(w, ConfigWarning::InvalidValue { key, .. } if key == "terminal_height_percent")),
            "{bad}: {w:?}"
        );
    }
}

/// T19 (D227): the panel position is bottom or right (bottom by default) and its width on the
/// right a share of the editor column, 10..=90 %; a bad value keeps the default with a warning.
#[test]
fn terminal_position_and_width_are_checked() {
    use vr_config::TerminalPosition;
    let s = Settings::default();
    assert_eq!(s.terminal_position, TerminalPosition::Bottom);
    assert_eq!(s.terminal_width_percent, 40);
    let (_d, path) = write_user("terminal_position = \"right\"\nterminal_width_percent = 55\n");
    let (s, w) = Settings::load(Some(&path));
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(s.terminal_position, TerminalPosition::Right);
    assert_eq!(s.terminal_width_percent, 55);
    for (key, bad) in [
        ("terminal_position", "\"left\""),
        ("terminal_position", "\"Right\""),
        ("terminal_position", "1"),
        ("terminal_width_percent", "9"),
        ("terminal_width_percent", "91"),
        ("terminal_width_percent", "\"wide\""),
    ] {
        let (_d, path) = write_user(&format!("{key} = {bad}\n"));
        let (s, w) = Settings::load(Some(&path));
        assert_eq!(s, Settings::default(), "{key} = {bad}");
        assert!(
            w.iter()
                .any(|w| matches!(w, ConfigWarning::InvalidValue { key: k, .. } if k == key)),
            "{key} = {bad}: {w:?}"
        );
    }
    // /set takes the word as typed and checks it the same way.
    let d = Settings::default();
    let right = d.with_value("terminal_position", "right").unwrap();
    assert_eq!(right.terminal_position, TerminalPosition::Right);
    assert!(d.with_value("terminal_position", "left").is_err());
    let wide = d.with_value("terminal_width_percent", "90").unwrap();
    assert_eq!(wide.terminal_width_percent, 90);
    assert_eq!(
        d.with_value("terminal_width_percent", "95").unwrap_err(),
        "terminal_width_percent must be between 10 and 90"
    );
    assert_eq!(
        d.value_text("terminal_position").as_deref(),
        Some("\"bottom\"")
    );
}

/// D214: a legacy terminal's NUL toggles the panel from inside a terminal unless the user asks
/// for it to reach the program.
#[test]
fn terminal_pass_nul_defaults_to_off() {
    assert!(!Settings::load(None).0.terminal_pass_nul);
    let (_d, path) = write_user("terminal_pass_nul = true\n");
    let (s, w) = Settings::load(Some(&path));
    assert!(w.is_empty(), "{w:?}");
    assert!(s.terminal_pass_nul);
}

/// M7-4a: inline blame is off until the user turns it on; `/set` and the file both know it.
#[test]
fn git_inline_blame_defaults_to_off_and_is_settable() {
    assert!(!Settings::load(None).0.git_inline_blame);
    assert!(Settings::names().contains(&"git_inline_blame".to_string()));
    let (_d, path) = write_user("git_inline_blame = true\n");
    let (s, w) = Settings::load(Some(&path));
    assert!(w.is_empty(), "{w:?}");
    assert!(s.git_inline_blame);
    let off = s.with_value("git_inline_blame", "false").unwrap();
    assert!(!off.git_inline_blame);
}

#[test]
fn unquoted_keys_with_quote_or_brace_parse() {
    let (_d, path) = write_user(
        "ctrl+' = \"edit.indent\"\nctrl+{ = \"edit.outdent\"\nctrl+\" = { command = \"explorer.reveal\", when = \"explorer\" }\nctrl+s = \"file.save_as\"\n",
    );
    let (km, warnings) = Keymap::load(Some(&path));
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(km.command_for(&chord("ctrl+'")), Some("edit.indent"));
    assert_eq!(km.command_for(&chord("ctrl+{")), Some("edit.outdent"));
    assert_eq!(
        km.command_for_in(Context::Explorer, &chord("ctrl+\"")),
        Some("explorer.reveal")
    );
    assert_eq!(km.command_for(&chord("ctrl+s")), Some("file.save_as"));
}

#[test]
fn same_chord_twice_in_one_context_is_reported() {
    let (_d, path) = write_user(
        "\"ctrl+s\" = \"file.save\"\n\"ctrl+S\" = \"file.save_as\"\n[explorer]\n\"f2\" = \"explorer.rename\"\n\"F2\" = \"explorer.move\"\n[editor]\n\"ctrl+s\" = \"file.save\"\n",
    );
    let (km, warnings) = Keymap::load(Some(&path));
    assert_eq!(warnings.len(), 2, "{warnings:?}");
    let texts: Vec<String> = warnings.iter().map(ToString::to_string).collect();
    assert!(
        texts
            .iter()
            .any(|t| t.contains("\"ctrl+s\"") && t.contains("\"ctrl+S\"") && t.contains("global")),
        "{texts:?}"
    );
    assert!(
        texts
            .iter()
            .any(|t| t.contains("\"f2\"") && t.contains("\"F2\"") && t.contains("explorer")),
        "{texts:?}"
    );
    // One of the two bindings is kept; the editor binding of ctrl+s is a different context.
    let global = km.command_for_in(Context::Global, &chord("ctrl+s"));
    assert!(matches!(global, Some("file.save") | Some("file.save_as")));
    assert_eq!(km.command_for(&chord("ctrl+s")), Some("file.save"));
}

/// D250: keys are single keys; there are no sequences and no leader. A key with a space in it
/// ("ctrl+d f", "leader f") is an invalid key, reported and skipped.
#[test]
fn a_key_sequence_is_an_invalid_key() {
    let (_d, path) = write_user(
        "\"leader f\" = \"file.reload\"\n\"ctrl+d f\" = \"file.save\"\n\"f5\" = \"file.reload\"\n",
    );
    let (km, w) = Keymap::load(Some(&path));
    let mut invalid: Vec<&String> = w
        .iter()
        .filter_map(|w| match w {
            ConfigWarning::InvalidKey { key, .. } => Some(key),
            _ => None,
        })
        .collect();
    invalid.sort();
    assert_eq!(invalid, ["ctrl+d f", "leader f"], "{w:?}");
    assert_eq!(w.len(), 2, "{w:?}");
    assert_eq!(km.command_for(&chord("f5")), Some("file.reload"));
    assert_eq!(km.command_for(&chord("ctrl+d")), Some("edit.delete_line"));
}

/// D250: the leader_key setting is gone; a user file that still sets it gets the usual
/// unknown-setting warning.
#[test]
fn leader_key_is_not_a_setting() {
    let (_d, path) = write_user("leader_key = \"ctrl+d\"\n");
    let (_, w) = Settings::load(Some(&path));
    assert!(
        matches!(w.as_slice(), [ConfigWarning::UnknownSetting { key, .. }] if key == "leader_key"),
        "{w:?}"
    );
}

/// The keymap examples in docs/keymap.md and the README parse without a warning and name real
/// commands (review: documented syntax must be valid).
#[test]
fn documented_keymap_examples_are_valid() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let mut blocks = Vec::new();
    for doc in ["docs/keymap.md", "README.md"] {
        let text = fs::read_to_string(format!("{root}/{doc}")).unwrap();
        let mut rest = text.as_str();
        while let Some(start) = rest.find("```toml\n") {
            let body = &rest[start + 8..];
            let end = body.find("```").unwrap();
            let block = &body[..end];
            let is_keymap = doc == "docs/keymap.md" || block.contains("keymap.toml");
            if is_keymap {
                blocks.push((doc, block.to_string()));
            }
            rest = &body[end + 3..];
        }
    }
    assert!(blocks.len() >= 2, "{blocks:?}");
    for (doc, block) in blocks {
        let (_d, path) = write_user(&block);
        let (km, w) = Keymap::load(Some(&path));
        assert!(w.is_empty(), "{doc}: {w:?}\n{block}");
        assert!(km.validate(KNOWN).is_empty(), "{doc}: {block}");
    }
}
