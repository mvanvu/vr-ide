use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::desktop::{MARKER, build_entry, install, quote_exec_arg, uninstall, validate_exe};

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
    for gnome in [true, false] {
        let entry = build_entry(exe, gnome);
        let args = parse_exec(exec_of(&entry));
        if gnome {
            assert_eq!(
                args,
                ["gnome-terminal", "--title=vr-ide", "--", exe, "--session"]
            );
            assert!(entry.contains("Terminal=false"));
        } else {
            assert_eq!(args, [exe, "--session"]);
            assert!(entry.contains("Terminal=true"));
        }
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
    let written = install(&data, &exe, true).unwrap();
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
    install(&data, &exe, false).unwrap();
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
fn symlinks_are_not_followed() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let exe = fake_exe(tmp.path(), "vr-ide");
    let victim = tmp.path().join("victim");
    fs::write(&victim, "precious\n").unwrap();
    let apps = data.join("applications");
    fs::create_dir_all(&apps).unwrap();
    symlink(&victim, apps.join("vr-ide.desktop")).unwrap();
    install(&data, &exe, true).unwrap();
    assert_eq!(fs::read_to_string(&victim).unwrap(), "precious\n");
    assert!(
        !fs::symlink_metadata(apps.join("vr-ide.desktop"))
            .unwrap()
            .is_symlink()
    );

    // A symlink carrying the marker through its target is not removed.
    fs::remove_file(apps.join("vr-ide.desktop")).unwrap();
    let marked = tmp.path().join("marked");
    fs::write(&marked, format!("{MARKER}\n")).unwrap();
    symlink(&marked, apps.join("vr-ide.desktop")).unwrap();
    uninstall(&data).unwrap();
    assert!(marked.exists());
    assert!(fs::symlink_metadata(apps.join("vr-ide.desktop")).is_ok());
}

#[test]
fn icon_is_self_contained_svg() {
    let svg = include_str!("../../../assets/vr-ide.svg");
    assert!(svg.contains("<svg") && svg.contains(MARKER));
    assert!(!svg.contains("href") && !svg.contains("<text"));
}
