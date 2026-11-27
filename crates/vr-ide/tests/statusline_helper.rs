//! End-to-end: the real binary as claude's statusLine helper (AC-3, SEC-23), on temp folders
//! only.
use std::io::Write;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const ID: &str = "0f8e2c1a-7b3d-4e5f-9a1b-2c3d4e5f6a7b";
const STEM: &str = "4242-1-0123456789abcdef0123456789abcdef";

fn usage_dir(tmp: &Path) -> PathBuf {
    let dir = tmp.join("ai-usage");
    std::fs::create_dir(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(dir.join(format!("{STEM}.json")), "{}").unwrap();
    dir.join(format!("{STEM}.report"))
}

fn status() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    format!(
        r#"{{"session_id":"{ID}","cwd":"/w","model":{{"display_name":"Opus 5.5"}},"rate_limits":{{"five_hour":{{"used_percentage":63,"resets_at":{}}}}}}}"#,
        now + 3600
    )
}

fn helper(home: &Path, report: &Path, stdin: &str) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_vr-ide"))
        .arg("--ai-statusline")
        .arg(report)
        .env("HOME", home)
        .env("VR_IDE_STATE_DIR", home.join("state"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn helper_writes_the_report_and_prints_a_line() {
    let tmp = tempfile::tempdir().unwrap();
    let report = usage_dir(tmp.path());
    let out = helper(tmp.path(), &report, &status());
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout), "Opus 5.5 · 5h 63%\n");
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(std::fs::metadata(&report).unwrap().mode() & 0o777, 0o600);
    let text = std::fs::read_to_string(&report).unwrap();
    assert!(text.contains(ID) && !text.contains("cwd"), "{text}");
    // Helper mode starts nothing else: no state folder, no recovery folder.
    assert!(!tmp.path().join("state").exists());
}

#[test]
fn helper_never_writes_another_file() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join(".bashrc"), "rc").unwrap();
    let out = helper(tmp.path(), &tmp.path().join(".bashrc"), &status());
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert_eq!(
        std::fs::read_to_string(tmp.path().join(".bashrc")).unwrap(),
        "rc"
    );
}

/// claude never closing stdin: the helper gives up after its 2 s deadline, writing nothing.
#[test]
fn helper_gives_up_on_open_stdin() {
    let tmp = tempfile::tempdir().unwrap();
    let report = usage_dir(tmp.path());
    let start = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_vr-ide"))
        .arg("--ai-statusline")
        .arg(&report)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"{\"session_id\":").unwrap();
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if start.elapsed() > Duration::from_secs(10) {
            let _ = child.kill();
            panic!("helper still running after 10 s");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    drop(stdin);
    assert!(status.success());
    assert!(
        start.elapsed() < Duration::from_secs(4),
        "{:?}",
        start.elapsed()
    );
    assert!(!report.exists());
}

/// codex's notify program (AC-8, SEC-32): `vr-ide --ai-codex-notify <report> <json>` as codex
/// 0.144.1 runs it — argv, the payload last, stdin closed — keeps the thread id alone, and
/// starts nothing else.
#[test]
fn codex_notify_helper_keeps_only_the_thread_id() {
    let tmp = tempfile::tempdir().unwrap();
    let report = usage_dir(tmp.path());
    let run = |target: &Path, payload: &str| {
        Command::new(env!("CARGO_BIN_EXE_vr-ide"))
            .arg("--ai-codex-notify")
            .arg(target)
            .arg(payload)
            .env("HOME", tmp.path())
            .env("VR_IDE_STATE_DIR", tmp.path().join("state"))
            .stdin(Stdio::null())
            .output()
            .unwrap()
    };
    let payload = |id: &str| {
        format!(
            r#"{{"type":"agent-turn-complete","thread-id":"{id}","turn-id":"t","cwd":"/w","client":"codex-tui","input-messages":["my secret"],"last-assistant-message":"ok"}}"#
        )
    };
    std::fs::write(tmp.path().join(".bashrc"), "rc").unwrap();
    for (target, body) in [
        (tmp.path().join(".bashrc"), payload(ID)),
        (
            report.clone(),
            payload("--dangerously-bypass-approvals-and-sandbox"),
        ),
        (report.clone(), "not json".to_string()),
    ] {
        let out = run(&target, &body);
        assert!(out.status.success());
        assert!(out.stdout.is_empty() && out.stderr.is_empty());
    }
    assert_eq!(
        std::fs::read_to_string(tmp.path().join(".bashrc")).unwrap(),
        "rc"
    );
    assert!(!report.exists());

    let out = run(&report, &payload(ID));
    assert!(out.status.success());
    assert!(out.stdout.is_empty() && out.stderr.is_empty());
    assert_eq!(
        std::fs::read_to_string(&report).unwrap(),
        format!(r#"{{"session_id":"{ID}"}}"#)
    );
    assert_eq!(std::fs::metadata(&report).unwrap().mode() & 0o777, 0o600);
    assert!(!tmp.path().join("state").exists());
}
