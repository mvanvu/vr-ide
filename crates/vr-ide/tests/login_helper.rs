//! End-to-end: the real binary as an account's login launcher (claude's `$BROWSER`, D63) and
//! as the login window process (SEC-34), on temp folders only. Every run here has no display
//! (`DISPLAY` / `WAYLAND_DISPLAY` removed) and a stand-in `xdg-open`: no window and no browser
//! can open.
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const START: &str = "https://claude.com/cai/oauth/authorize?code=true&client_id=c&redirect_uri=http%3A%2F%2Flocalhost%3A54545%2Fcallback&state=SECRET";

struct Fx {
    tmp: tempfile::TempDir,
    state: PathBuf,
    /// What the stand-in opener was called with.
    opened: PathBuf,
}

fn fx() -> Fx {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state");
    let bin = tmp.path().join("path");
    std::fs::create_dir_all(&bin).unwrap();
    let opened = tmp.path().join("opened");
    for opener in ["xdg-open", "open"] {
        let p = bin.join(opener);
        let text = format!(
            "#!/bin/sh\nprintf 'ARG=%s\\n' \"$@\" > '{0}'\nprintf 'BROWSER=%s\\n' \"${{BROWSER-unset}}\" >> '{0}'\nexit 7\n",
            opened.display()
        );
        std::fs::write(&p, text).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    Fx { tmp, state, opened }
}

fn launcher(f: &Fx, name: &str) -> PathBuf {
    let bin = f.state.join("ai-login/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let link = bin.join(name);
    if std::fs::symlink_metadata(&link).is_err() {
        std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_vr-ide"), &link).unwrap();
    }
    link
}

fn run(f: &Fx, program: &Path, args: &[&str]) -> Output {
    let path = format!("{}:/usr/bin:/bin", f.tmp.path().join("path").display());
    Command::new(program)
        .args(args)
        .env("HOME", f.tmp.path())
        .env("VR_IDE_STATE_DIR", &f.state)
        .env("PATH", path)
        // As claude's children inherit it.
        .env("BROWSER", launcher(f, "vr-ide-login.claude.work"))
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap()
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// D6: what is not a claude login goes to the system opener unchanged, as one argument, with
/// `BROWSER` removed (the opener cannot come back to the launcher); the exit code is its own.
#[test]
fn launcher_hands_other_urls_to_the_system_opener() {
    let f = fx();
    let link = launcher(&f, "vr-ide-login.claude.work");
    for target in [
        "https://example.com/a?b=c d&e",
        "http://localhost:3000/",
        "https://claude.ai/upgrade",
        "/tmp/report.html",
    ] {
        let out = run(&f, &link, &[target]);
        assert_eq!(out.status.code(), Some(7), "{target}: {}", text(&out));
        assert_eq!(
            std::fs::read_to_string(&f.opened).unwrap(),
            format!("ARG={target}\nBROWSER=unset\n"),
            "{target}"
        );
        assert!(out.stdout.is_empty(), "never the editor");
    }
    assert!(!f.state.join("ai-login/claude-work").exists());
}

/// A claude login without a display: exit 1 (claude then shows the URL), the opener is not
/// used for it, nothing is created, and the URL's query shows nowhere.
#[test]
fn launcher_exits_1_for_a_login_without_a_display() {
    let f = fx();
    let link = launcher(&f, "vr-ide-login.claude.work");
    let out = run(&f, &link, &[START]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out));
    assert!(!f.opened.exists(), "a login never goes to the opener");
    assert!(!text(&out).contains("SECRET"));
    assert!(!f.state.join("ai-login/claude-work").exists());
    assert!(!f.state.join("ai-login/claude-work.lock").exists());
}

/// An argument that starts with `-` would be read by a real `xdg-open` as an option.
#[test]
fn launcher_never_passes_an_option_to_the_opener() {
    let f = fx();
    let link = launcher(&f, "vr-ide-login.claude.work");
    for target in ["--help", "-oProxyCommand=x", "-https://x/--version"] {
        let out = run(&f, &link, &[target]);
        assert_eq!(out.status.code(), Some(1), "{target}: {}", text(&out));
        assert!(!f.opened.exists(), "{target}: the opener was run");
    }
}

/// `--login-window` in any other than the four-argument form says how it is used.
#[test]
fn login_window_flag_with_wrong_arguments_prints_usage() {
    let f = fx();
    let out = run(
        &f,
        &PathBuf::from(env!("CARGO_BIN_EXE_vr-ide")),
        &["--login-window", "claude"],
    );
    assert_eq!(out.status.code(), Some(2), "{}", text(&out));
    assert!(
        text(&out).contains("usage: vr-ide --login-window"),
        "{}",
        text(&out)
    );
    assert!(out.stdout.is_empty());
}

/// Under a launcher name the editor never starts: any other form exits 2 at once.
#[test]
fn launcher_takes_exactly_one_argument() {
    let f = fx();
    let work = launcher(&f, "vr-ide-login.claude.work");
    let codex = launcher(&f, "vr-ide-login.codex.work");
    let odd = launcher(&f, "vr-ide-login.claude.Work");
    for (program, args) in [
        (&work, &[][..]),
        (&work, &[START, "x"][..]),
        (&codex, &[START][..]),
        (&odd, &[START][..]),
    ] {
        let out = run(&f, program, args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", text(&out));
        assert!(out.stdout.is_empty(), "no screen was set up");
        assert!(!text(&out).contains("SECRET"));
    }
    assert!(!f.opened.exists());
}

/// SEC-34: the window process opens nothing for a URL that is no claude login start, for a
/// crafted account, or without a display; a refusal never prints the URL.
#[test]
fn window_process_exits_1_without_a_window() {
    let f = fx();
    let exe = Path::new(env!("CARGO_BIN_EXE_vr-ide"));
    for url in [
        "https://evil.com/?code=SECRET",
        "https://evil.com/oauth/authorize?code=SECRET",
        "https://claude.ai/?code=SECRET",
        "javascript:alert('SECRET')",
        START,
    ] {
        let out = run(&f, exe, &["--login-window", "claude", "work", url]);
        assert_eq!(out.status.code(), Some(1), "{url}: {}", text(&out));
        assert!(
            text(&out).contains("login window not opened"),
            "{}",
            text(&out)
        );
        assert!(!text(&out).contains("SECRET"), "{}", text(&out));
        assert!(out.stdout.is_empty());
    }
    assert!(
        !f.state.join("ai-login/claude-work").exists(),
        "nothing made"
    );
    assert!(!f.opened.exists());
    // Not the helper's form: the usual argument error, never a window, never the editor.
    for args in [
        &["--login-window", "claude", "../x", START][..],
        &["--login-window", "codex", "work", START][..],
        &["--login-window", "claude", "work"][..],
    ] {
        let out = run(&f, exe, args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", text(&out));
        assert!(out.stdout.is_empty());
    }
}
