//! End-to-end: the real binary installs into a temp XDG_DATA_HOME only.
use std::process::Command;

fn run(data: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_vr-ide"))
        .args(args)
        .env("XDG_DATA_HOME", data)
        .output()
        .unwrap()
}

#[test]
fn install_and_uninstall_desktop_flags() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path();
    let out = run(data, &["--install-desktop"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let desktop = data.join("applications/vr-ide.desktop");
    assert!(desktop.is_file());
    assert!(
        data.join("icons/hicolor/scalable/apps/vr-ide.svg")
            .is_file()
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("vr-ide.desktop"));
    let entry = std::fs::read_to_string(&desktop).unwrap();
    assert!(entry.contains(env!("CARGO_BIN_EXE_vr-ide")));

    let out = run(data, &["--uninstall-desktop"]);
    assert!(out.status.success());
    assert!(!desktop.exists());
    assert!(!data.join("icons/hicolor/scalable/apps/vr-ide.svg").exists());
}

#[test]
fn desktop_flags_must_be_alone() {
    let tmp = tempfile::tempdir().unwrap();
    for args in [
        &["--install-desktop", "."][..],
        &["--uninstall-desktop", "--session"],
        &["--install-desktop", "--uninstall-desktop"],
    ] {
        let out = run(tmp.path(), args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains("usage"));
    }
    assert!(!tmp.path().join("applications").exists());
}

#[test]
fn install_refuses_a_hand_made_entry_with_exit_1() {
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path();
    let desktop = data.join("applications/vr-ide.desktop");
    let icon = data.join("icons/hicolor/scalable/apps/vr-ide.svg");
    for p in [&desktop, &icon] {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "hand").unwrap();
    }
    let out = run(data, &["--install-desktop"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("not written by vr-ide"));
    assert_eq!(std::fs::read_to_string(&desktop).unwrap(), "hand");
    assert_eq!(std::fs::read_to_string(&icon).unwrap(), "hand");
}
