use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::desktop::{
    EntryForm, MARKER, build_entry, entry_form, find_in, install, migrate_on_window_start,
    path_note, quote_exec_arg, refresh_database_with, uninstall, validate_exe,
};

/// Undo value-level escaping, then split `Exec` per the Desktop Entry spec.
fn parse_exec(value: &str) -> Vec<String> {
    let mut unescaped = String::new();
    let mut it = value.chars();
    while let Some(c) = it.next() {
        if c == '\\' {
            unescaped.push(it.next().unwrap());
        } else {
            unescaped.push(c);
        }
    }
    let (mut args, mut cur, mut in_q, mut has) = (vec![], String::new(), false, false);
    let mut it = unescaped.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '"' => {
                in_q = !in_q;
                has = true;
            }
            '\\' if in_q => cur.push(it.next().unwrap()),
            '%' if it.peek() == Some(&'%') => {
                it.next();
                cur.push('%');
            }
            ' ' if !in_q => {
                if has {
                    args.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            _ => {
                cur.push(c);
                has = true;
            }
        }
    }
    if has {
        args.push(cur);
    }
    args
}

fn exec_of(entry: &str) -> &str {
    entry.lines().find_map(|l| l.strip_prefix("Exec=")).unwrap()
}

fn fake_exe(dir: &Path, rel: &str) -> PathBuf {
    let p = dir.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, "#!/bin/sh\n").unwrap();
    p
}

fn validate(path: &Path) {
    let Ok(out) = Command::new("desktop-file-validate").arg(path).output() else {
        return;
    };
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn exec_round_trips_hostile_path() {
    let exe = "/tmp/a b/\"x$y%z\\`w";
    for form in [
        EntryForm::Window,
        EntryForm::GnomeTerminal,
        EntryForm::Terminal,
    ] {
        let entry = build_entry(exe, form);
        let args = parse_exec(exec_of(&entry));
        match form {
            EntryForm::GnomeTerminal => {
                assert_eq!(
                    args,
                    ["gnome-terminal", "--title=vr-ide", "--", exe, "--session"]
                );
                assert!(entry.contains("Terminal=false"));
            }
            EntryForm::Terminal => {
                assert_eq!(args, [exe, "--session"]);
                assert!(entry.contains("Terminal=true"));
            }
            EntryForm::Window => {
                assert_eq!(args, [exe, "--session"]);
                assert!(entry.contains("\nTerminal=false\n"));
                assert!(entry.contains("\nStartupWMClass=vr-ide\n"));
            }
        }
        assert_eq!(entry.contains("StartupWMClass"), form == EntryForm::Window);
        assert!(entry.contains("\nIcon=vr-ide\n"));
        assert!(entry.contains("\nName=vr-ide\n"));
        assert!(entry.contains(MARKER));
    }
}

#[test]
fn percent_is_doubled() {
    assert_eq!(quote_exec_arg("50%"), "\"50%%\"");
}

#[test]
fn validate_refuses_bad_paths() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(validate_exe(Path::new("relative/vr-ide")).is_err());
    assert!(validate_exe(Path::new("/tmp/a\nb")).is_err());
    let deleted = fake_exe(tmp.path(), "vr-ide (deleted)");
    assert!(validate_exe(&deleted).is_err());
    assert!(validate_exe(&tmp.path().join("missing")).is_err());
    assert!(validate_exe(tmp.path()).is_err());
    assert!(validate_exe(&fake_exe(tmp.path(), "ok")).is_ok());
}

#[test]
fn install_writes_valid_files_and_uninstall_removes_them() {
    let tmp = tempfile::tempdir().unwrap();
    let exe = fake_exe(tmp.path(), "bin dir/\"x$y%z\\");
    let data = tmp.path().join("data");
    let written = install(&data, &exe, EntryForm::GnomeTerminal).unwrap();
    assert_eq!(written.len(), 2);
    let desktop = data.join("applications/vr-ide.desktop");
    let icon = data.join("icons/hicolor/scalable/apps/vr-ide.svg");
    assert!(desktop.is_file() && icon.is_file());
    assert_eq!(
        fs::metadata(&desktop).unwrap().permissions().mode() & 0o777,
        0o644
    );
    validate(&desktop);
    let entry = fs::read_to_string(&desktop).unwrap();
    let args = parse_exec(exec_of(&entry));
    assert_eq!(args[3], exe.to_str().unwrap());
    // Reinstalling is idempotent and leaves no temp file behind.
    install(&data, &exe, EntryForm::Terminal).unwrap();
    assert!(
        fs::read_to_string(&desktop)
            .unwrap()
            .contains("Terminal=true")
    );
    assert_eq!(fs::read_dir(data.join("applications")).unwrap().count(), 1);

    let removed = uninstall(&data).unwrap();
    assert_eq!(removed.len(), 2);
    assert!(!desktop.exists() && !icon.exists());
    assert!(uninstall(&data).unwrap().is_empty());
}

#[test]
fn uninstall_keeps_foreign_files() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path();
    let desktop = data.join("applications/vr-ide.desktop");
    let icon = data.join("icons/hicolor/scalable/apps/vr-ide.svg");
    for p in [&desktop, &icon] {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, "hand made\n").unwrap();
    }
    assert!(uninstall(data).unwrap().is_empty());
    assert!(desktop.exists() && icon.exists());
}

#[test]
fn install_refuses_unmarked_targets_and_writes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let exe = fake_exe(tmp.path(), "vr-ide");
    let victim = tmp.path().join("victim");
    for (target_is_desktop, as_symlink) in
        [(true, false), (false, false), (true, true), (false, true)]
    {
        let data = tmp
            .path()
            .join(format!("data-{target_is_desktop}-{as_symlink}"));
        let desktop = data.join("applications/vr-ide.desktop");
        let icon = data.join("icons/hicolor/scalable/apps/vr-ide.svg");
        let hand = if target_is_desktop { &desktop } else { &icon };
        let other = if target_is_desktop { &icon } else { &desktop };
        fs::create_dir_all(hand.parent().unwrap()).unwrap();
        fs::write(&victim, "precious\n").unwrap();
        if as_symlink {
            symlink(&victim, hand).unwrap();
        } else {
            fs::write(hand, "hand\n").unwrap();
        }
        let err = install(&data, &exe, EntryForm::GnomeTerminal)
            .unwrap_err()
            .to_string();
        assert!(err.contains(hand.to_str().unwrap()), "{err}");
        assert!(err.contains("not written by vr-ide"), "{err}");
        assert!(!other.exists(), "nothing written");
        assert_eq!(fs::read_to_string(&victim).unwrap(), "precious\n");
        if !as_symlink {
            assert_eq!(fs::read_to_string(hand).unwrap(), "hand\n");
        }
    }
}

#[test]
fn install_replaces_our_own_files_but_uninstall_needs_the_exact_marker() {
    let tmp = tempfile::tempdir().unwrap();
    let exe = fake_exe(tmp.path(), "vr-ide");
    let data = tmp.path().join("data");
    install(&data, &exe, EntryForm::GnomeTerminal).unwrap();
    install(&data, &exe, EntryForm::Terminal).unwrap();

    let other = tmp.path().join("other");
    let desktop = other.join("applications/vr-ide.desktop");
    let icon = other.join("icons/hicolor/scalable/apps/vr-ide.svg");
    for p in [&desktop, &icon] {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
    }
    fs::write(&desktop, format!("Comment={MARKER} not really\n")).unwrap();
    fs::write(&icon, format!("<svg><!-- {MARKER} in text --></svg>\n")).unwrap();
    assert!(uninstall(&other).unwrap().is_empty());
    assert!(install(&other, &exe, EntryForm::GnomeTerminal).is_err());
}

#[test]
fn failed_desktop_write_removes_the_icon_this_run_created() {
    let tmp = tempfile::tempdir().unwrap();
    let exe = fake_exe(tmp.path(), "vr-ide");
    let data = tmp.path().join("data");
    let apps = data.join("applications");
    fs::create_dir_all(&apps).unwrap();
    fs::set_permissions(&apps, fs::Permissions::from_mode(0o555)).unwrap();
    // Root bypasses mode bits, so the failure cannot be produced there.
    let bypassed = fs::write(apps.join("probe"), "").is_ok();
    let res = install(&data, &exe, EntryForm::GnomeTerminal);
    fs::set_permissions(&apps, fs::Permissions::from_mode(0o755)).unwrap();
    if bypassed {
        return;
    }
    assert!(res.is_err());
    assert!(!data.join("icons/hicolor/scalable/apps/vr-ide.svg").exists());
}

#[test]
fn path_note_names_a_binary_that_is_not_the_one_on_path() {
    let tmp = tempfile::tempdir().unwrap();
    let a = fake_exe(tmp.path(), "a/vr-ide");
    let b = fake_exe(tmp.path(), "b/vr-ide");
    assert_eq!(path_note(&a, Some(&a)), None);
    assert!(
        path_note(&a, Some(&b))
            .unwrap()
            .contains(a.to_str().unwrap())
    );
    assert!(path_note(&a, None).unwrap().contains(a.to_str().unwrap()));
}

#[test]
fn icon_is_self_contained_svg() {
    let svg = include_str!("../../../assets/vr-ide.svg");
    assert!(svg.contains("<svg") && svg.contains(MARKER));
    assert!(!svg.contains("href") && !svg.contains("<text"));
}

/// A `.` (relative) PATH entry never supplies the program: the resolved absolute path is run.
#[test]
fn update_desktop_database_runs_the_resolved_absolute_path() {
    let tmp = tempfile::tempdir().unwrap();
    let bin = tmp.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    let mark = tmp.path().join("ran");
    crate::ssh_tests::write_executable(
        &bin.join("update-desktop-database"),
        &format!("#!/bin/sh\necho \"$0\" > {}\n", mark.display()),
    );
    let data = tmp.path().join("data");
    let path = std::env::join_paths([Path::new("."), bin.as_path()]).unwrap();
    refresh_database_with(&data, Some(&path));
    assert_eq!(
        std::fs::read_to_string(&mark).unwrap().trim(),
        bin.join("update-desktop-database").to_str().unwrap()
    );
}

#[test]
fn update_desktop_database_ignores_a_relative_only_path() {
    let path = std::ffi::OsString::from(".");
    assert_eq!(find_in(Some(&path), "update-desktop-database"), None);
}

fn desktop_of(data: &Path) -> PathBuf {
    data.join("applications/vr-ide.desktop")
}

#[test]
fn install_window_form_writes_a_direct_launch_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let exe = fake_exe(tmp.path(), "my bin/vr-ide");
    let data = tmp.path().join("data");
    install(&data, &exe, EntryForm::Window).unwrap();
    let desktop = desktop_of(&data);
    validate(&desktop);
    let entry = fs::read_to_string(&desktop).unwrap();
    assert!(entry.contains("\nTerminal=false\n"));
    assert!(entry.contains("\nStartupWMClass=vr-ide\n"));
    assert!(!entry.contains("gnome-terminal"));
    let exec = exec_of(&entry);
    assert!(exec.starts_with('"'), "path with a space is quoted: {exec}");
    assert_eq!(parse_exec(exec), [exe.to_str().unwrap(), "--session"]);
    assert_eq!(
        fs::metadata(&desktop).unwrap().permissions().mode() & 0o777,
        0o644
    );
}

#[test]
fn migration_rewrites_an_old_entry_of_ours_once() {
    let tmp = tempfile::tempdir().unwrap();
    let exe = fake_exe(tmp.path(), "my bin/vr-ide");
    let data = tmp.path().join("data");
    install(&data, &exe, EntryForm::GnomeTerminal).unwrap();
    let desktop = desktop_of(&data);

    let notice = migrate_on_window_start(&data, &exe).expect("a notice");
    assert!(notice.contains("own window"), "{notice}");
    assert_eq!(
        fs::read_to_string(&desktop).unwrap(),
        build_entry(exe.to_str().unwrap(), EntryForm::Window)
    );
    validate(&desktop);
    assert_eq!(fs::read_dir(data.join("applications")).unwrap().count(), 1);

    // Idempotent: the second start changes nothing and says nothing.
    let before = fs::metadata(&desktop).unwrap().modified().unwrap();
    assert_eq!(migrate_on_window_start(&data, &exe), None);
    assert_eq!(fs::metadata(&desktop).unwrap().modified().unwrap(), before);
}

#[test]
fn migration_leaves_new_and_absent_entries_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let exe = fake_exe(tmp.path(), "vr-ide");
    let data = tmp.path().join("data");
    assert_eq!(migrate_on_window_start(&data, &exe), None);
    assert!(!data.exists(), "nothing created for an absent entry");

    install(&data, &exe, EntryForm::Window).unwrap();
    let before = fs::read(desktop_of(&data)).unwrap();
    assert_eq!(migrate_on_window_start(&data, &exe), None);
    assert_eq!(fs::read(desktop_of(&data)).unwrap(), before);
}

#[test]
fn migration_never_touches_a_foreign_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let exe = fake_exe(tmp.path(), "vr-ide");
    let data = tmp.path().join("data");
    let desktop = desktop_of(&data);
    fs::create_dir_all(desktop.parent().unwrap()).unwrap();
    // The old form without the exact marker line, and with the marker only inside a value.
    let old = build_entry(exe.to_str().unwrap(), EntryForm::GnomeTerminal);
    let unmarked = old.replace(&format!("{MARKER}\n"), "");
    let fake_marker = old.replace(&format!("{MARKER}\n"), &format!("Comment={MARKER}\n"));
    for text in [unmarked, fake_marker] {
        fs::write(&desktop, &text).unwrap();
        assert_eq!(migrate_on_window_start(&data, &exe), None);
        assert_eq!(fs::read_to_string(&desktop).unwrap(), text);
    }
}

#[test]
fn migration_never_follows_or_replaces_a_symlinked_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let exe = fake_exe(tmp.path(), "vr-ide");
    let data = tmp.path().join("data");
    let desktop = desktop_of(&data);
    fs::create_dir_all(desktop.parent().unwrap()).unwrap();
    // The link target is a genuine old entry of ours: still not migrated through the link.
    let target = tmp.path().join("elsewhere.desktop");
    let old = build_entry(exe.to_str().unwrap(), EntryForm::GnomeTerminal);
    fs::write(&target, &old).unwrap();
    symlink(&target, &desktop).unwrap();
    assert_eq!(migrate_on_window_start(&data, &exe), None);
    assert!(
        fs::symlink_metadata(&desktop)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), old);
}

#[test]
fn migration_skips_an_entry_that_starts_another_binary() {
    let tmp = tempfile::tempdir().unwrap();
    let installed = fake_exe(tmp.path(), "installed/vr-ide");
    let dev = fake_exe(tmp.path(), "dev/vr-ide");
    let data = tmp.path().join("data");
    install(&data, &installed, EntryForm::GnomeTerminal).unwrap();
    let before = fs::read(desktop_of(&data)).unwrap();
    assert_eq!(migrate_on_window_start(&data, &dev), None);
    assert_eq!(fs::read(desktop_of(&data)).unwrap(), before);
}

#[test]
fn migration_write_failure_returns_a_warning_and_keeps_the_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let exe = fake_exe(tmp.path(), "vr-ide");
    let data = tmp.path().join("data");
    install(&data, &exe, EntryForm::GnomeTerminal).unwrap();
    let apps = data.join("applications");
    let before = fs::read(desktop_of(&data)).unwrap();
    fs::set_permissions(&apps, fs::Permissions::from_mode(0o555)).unwrap();
    // Root bypasses mode bits, so the failure cannot be produced there.
    let bypassed = fs::write(apps.join("probe"), "").is_ok();
    let res = migrate_on_window_start(&data, &exe);
    fs::set_permissions(&apps, fs::Permissions::from_mode(0o755)).unwrap();
    if bypassed {
        return;
    }
    let warning = res.expect("a warning");
    assert!(warning.starts_with("warning:"), "{warning}");
    assert_eq!(fs::read(desktop_of(&data)).unwrap(), before);
}

#[test]
fn uninstall_removes_both_forms() {
    let tmp = tempfile::tempdir().unwrap();
    let exe = fake_exe(tmp.path(), "vr-ide");
    for form in [EntryForm::GnomeTerminal, EntryForm::Window] {
        let data = tmp.path().join(format!("data-{form:?}"));
        install(&data, &exe, form).unwrap();
        assert_eq!(uninstall(&data).unwrap().len(), 2);
        assert!(!desktop_of(&data).exists());
    }
}

#[test]
fn a_build_without_the_window_picks_a_terminal_form() {
    assert_eq!(entry_form(false, true), EntryForm::GnomeTerminal);
    assert_eq!(entry_form(false, false), EntryForm::Terminal);
    assert_eq!(entry_form(true, true), EntryForm::Window);
    assert_eq!(entry_form(true, false), EntryForm::Window);
}

#[test]
fn migration_rewrites_our_terminal_form_entry_but_not_a_changed_exec() {
    let tmp = tempfile::tempdir().unwrap();
    let exe = fake_exe(tmp.path(), "my bin/vr-ide");
    let data = tmp.path().join("data");
    install(&data, &exe, EntryForm::Terminal).unwrap();
    let old = fs::read_to_string(desktop_of(&data)).unwrap();

    let notice = migrate_on_window_start(&data, &exe).expect("a notice");
    assert!(notice.contains("own window"), "{notice}");
    let new = build_entry(exe.to_str().unwrap(), EntryForm::Window);
    assert_eq!(fs::read_to_string(desktop_of(&data)).unwrap(), new);
    assert_eq!(migrate_on_window_start(&data, &exe), None);

    // An entry of ours whose Exec no longer is `<exe> --session` is left as it is.
    let edited = old.replace(" --session\n", " --session --tui\n");
    assert_ne!(edited, old);
    fs::write(desktop_of(&data), &edited).unwrap();
    assert_eq!(migrate_on_window_start(&data, &exe), None);
    assert_eq!(fs::read_to_string(desktop_of(&data)).unwrap(), edited);
}
