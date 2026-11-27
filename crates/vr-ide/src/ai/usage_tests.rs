//! The statusLine helper and its report (AC-3, SEC-22..SEC-25, SEC-39).

use std::ffi::{OsStr, OsString};
use std::fs::Permissions;
use std::os::fd::AsFd;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::process::Command;

use super::*;

const ID: &str = "0f8e2c1a-7b3d-4e5f-9a1b-2c3d4e5f6a7b";
/// 2026-09-21T14:13:20Z.
const NOW: i64 = 1_790_000_000;
const HEX: &str = "0123456789abcdef0123456789abcdef";

fn stem() -> String {
    format!("4242-1-{HEX}")
}

/// What claude 2.1.332 sends on stdin (its statusLine documentation), with fields the helper
/// must drop.
fn sample(now: i64) -> String {
    format!(
        r#"{{"session_id":"{ID}","transcript_path":"/home/u/.claude/projects/x.jsonl","cwd":"/w","model":{{"id":"claude-opus-5-5","display_name":"Opus 5.5"}},"workspace":{{"current_dir":"/w","project_dir":"/w"}},"version":"2.1.332","cost":{{"total_cost_usd":1.5}},"rate_limits":{{"five_hour":{{"used_percentage":63,"resets_at":{}}},"seven_day":{{"used_percentage":20.5,"resets_at":{}}}}}}}"#,
        now + 3600,
        now + 3 * 86_400
    )
}

/// `<tmp>/ai-usage` (0700) holding the settings file of [`stem`]; the report path beside it.
fn usage_dir() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join(DIR);
    std::fs::create_dir(&dir).unwrap();
    std::fs::set_permissions(&dir, Permissions::from_mode(0o700)).unwrap();
    std::fs::write(dir.join(format!("{}.json", stem())), "{}").unwrap();
    (tmp, dir.join(format!("{}.report", stem())))
}

fn input(tmp: &Path, text: &str) -> std::fs::File {
    static N: AtomicU64 = AtomicU64::new(0);
    let p = tmp.join(format!("stdin{}", N.fetch_add(1, Ordering::Relaxed)));
    std::fs::write(&p, text).unwrap();
    std::fs::File::open(p).unwrap()
}

fn run(report: &Path, stdin: &std::fs::File, now: i64) -> String {
    let mut out = Vec::new();
    helper(
        report,
        stdin.as_fd(),
        &mut out,
        Instant::now() + Duration::from_secs(5),
        now,
    );
    String::from_utf8(out).unwrap()
}

#[test]
fn parse_status_reads_limits_and_id() {
    let u = parse_status_at(sample(NOW).as_bytes(), NOW).unwrap();
    assert_eq!(
        u,
        Usage {
            session_id: Some(ID.into()),
            five_hour: Some(Window {
                used_percentage: 63.0,
                resets_at: Some(NOW + 3600),
            }),
            seven_day: Some(Window {
                used_percentage: 20.5,
                resets_at: Some(NOW + 3 * 86_400),
            }),
            model: Some("Opus 5.5".into()),
        }
    );
    // No rate_limits (an API-key login): the id alone.
    let u = parse_status_at(format!(r#"{{"session_id":"{ID}"}}"#).as_bytes(), NOW).unwrap();
    assert_eq!(u.session_id.as_deref(), Some(ID));
    assert_eq!((u.five_hour, u.seven_day, u.model), (None, None, None));
}

#[test]
fn parse_status_rejects_bad_id_and_oversize() {
    let upper = ID.to_uppercase();
    let newline = format!("{ID}\n");
    for id in [
        "--dangerously-skip-permissions",
        "-x",
        "abc-123",
        upper.as_str(),
        newline.as_str(),
        "",
        "0f8e2c1a7-b3d-4e5f-9a1b-2c3d4e5f6a7b",
        "0f8e2c1a-7b3d-4e5f-9a1b-2c3d4e5f6a7g",
        "0f8e2c1a-7b3d-4e5f-9a1b-2c3d4e5f6a7b0",
    ] {
        assert!(!valid_session_id(id), "{id:?}");
        let json = serde_json::json!({ "session_id": id }).to_string();
        let u = parse_status_at(json.as_bytes(), NOW).unwrap();
        assert_eq!(u.session_id, None, "{id:?}");
    }
    assert!(valid_session_id(ID));
    let u = parse_status_at(br#"{"session_id":12}"#, NOW).unwrap();
    assert_eq!(u.session_id, None);

    let mut big = format!(r#"{{"session_id":"{ID}","pad":""#);
    big.push_str(&"x".repeat(MAX_STATUS));
    big.push_str("\"}");
    assert_eq!(parse_status_at(big.as_bytes(), NOW), None);
    for bad in [&b"[]"[..], b"null", b"{", b"", b"\xff"] {
        assert_eq!(parse_status_at(bad, NOW), None, "{bad:?}");
    }

    let window = |pct: &str, resets: &str| {
        let json = format!(
            r#"{{"rate_limits":{{"five_hour":{{"used_percentage":{pct},"resets_at":{resets}}}}}}}"#
        );
        parse_status_at(json.as_bytes(), NOW).unwrap().five_hour
    };
    assert_eq!(window("1e308", "null").unwrap().used_percentage, 1000.0);
    assert_eq!(window("-5", "null").unwrap().used_percentage, 0.0);
    assert_eq!(window("\"NaN\"", "null"), None);
    assert_eq!(window("63", "253402300799").unwrap().resets_at, None);
    assert_eq!(
        window("63", &(NOW - 2 * 86_400).to_string())
            .unwrap()
            .resets_at,
        None
    );
    assert_eq!(
        window("63", &(NOW + 36 * 86_400).to_string())
            .unwrap()
            .resets_at,
        None
    );
    assert_eq!(
        window("63", &(NOW + 34 * 86_400).to_string())
            .unwrap()
            .resets_at,
        Some(NOW + 34 * 86_400)
    );

    let model = |name: &str| {
        let json = serde_json::json!({ "model": { "display_name": name } }).to_string();
        parse_status_at(json.as_bytes(), NOW).unwrap().model
    };
    let m = model("\u{1b}[2JOpus\u{7}").unwrap();
    assert!(!m.chars().any(char::is_control), "{m:?}");
    assert!(m.contains("Opus"), "{m:?}");
    assert_eq!(model(&"é".repeat(200)).unwrap().chars().count(), 64);
}

#[test]
fn resets_at_rfc3339_and_seconds() {
    let at = |resets: &str| {
        let json = format!(
            r#"{{"rate_limits":{{"seven_day":{{"used_percentage":1,"resets_at":{resets}}}}}}}"#
        );
        parse_status_at(json.as_bytes(), NOW)
            .unwrap()
            .seven_day
            .unwrap()
            .resets_at
    };
    assert_eq!(at(&NOW.to_string()), Some(NOW));
    assert_eq!(at(&format!("{NOW}.7")), Some(NOW));
    assert_eq!(at("\"2026-09-21T14:13:20Z\""), Some(NOW));
    assert_eq!(at("\"2026-09-21t14:13:20.250z\""), Some(NOW));
    assert_eq!(at("\"2026-09-21T21:13:20+07:00\""), Some(NOW));
    assert_eq!(at("\"2026-09-21T09:13:20-05:00\""), Some(NOW));
    for bad in [
        "\"2026-02-30T00:00:00Z\"",
        "\"2026-09-21T14:13:20\"",
        "\"2026-09-21 14:13:20Z \"",
        "\"yesterday\"",
        "\"2026-09-21T24:00:00Z\"",
        "true",
    ] {
        assert_eq!(at(bad), None, "{bad}");
    }
}

#[test]
fn status_line_format() {
    let u = parse_status_at(sample(NOW).as_bytes(), NOW).unwrap();
    assert_eq!(status_line(&u), "Opus 5.5 · 5h 63%");
    let no_model = Usage {
        model: None,
        ..u.clone()
    };
    assert_eq!(status_line(&no_model), "5h 63%");
    let no_window = Usage {
        five_hour: None,
        ..u.clone()
    };
    assert_eq!(status_line(&no_window), "Opus 5.5");
    assert_eq!(status_line(&Usage::default()), "");
    let wild = Usage {
        model: Some(format!("a\u{1b}b\nc{}", "m".repeat(300))),
        ..u
    };
    let line = status_line(&wild);
    assert!(line.chars().count() <= 120, "{line}");
    assert!(!line.chars().any(char::is_control), "{line:?}");
}

#[test]
fn report_roundtrip_private_and_atomic() {
    let (tmp, report) = usage_dir();
    let now = now_secs();
    let u = parse_status_at(sample(now).as_bytes(), now).unwrap();
    write_report(&report, &u).unwrap();
    assert_eq!(std::fs::metadata(&report).unwrap().mode() & 0o777, 0o600);
    assert_eq!(read_report(&report), Some(u.clone()));
    let text = std::fs::read_to_string(&report).unwrap();
    for dropped in ["transcript", "cwd", "cost", "workspace", "version"] {
        assert!(!text.contains(dropped), "{text}");
    }
    // A symlink at the report path is replaced; what it pointed at stays as it was.
    let victim = tmp.path().join("victim");
    std::fs::write(&victim, "keep").unwrap();
    std::fs::remove_file(&report).unwrap();
    symlink(&victim, &report).unwrap();
    write_report(&report, &u).unwrap();
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep");
    assert!(std::fs::symlink_metadata(&report).unwrap().is_file());
    // Settings + report, no temp file left behind.
    assert_eq!(
        std::fs::read_dir(report.parent().unwrap()).unwrap().count(),
        2
    );
}

#[test]
fn helper_writes_only_its_own_report() {
    let (tmp, report) = usage_dir();
    let now = now_secs();
    let stdin = input(tmp.path(), &sample(now));
    let dir = report.parent().unwrap().to_path_buf();

    // A file the helper must never replace (`vr-ide --ai-statusline ~/.bashrc`).
    let home = tmp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    std::fs::write(home.join(".bashrc"), "rc").unwrap();
    // A look-alike folder that is not private, one with another name, one through a link.
    let open = tmp.path().join("open").join(DIR);
    std::fs::create_dir_all(&open).unwrap();
    std::fs::set_permissions(&open, Permissions::from_mode(0o755)).unwrap();
    std::fs::write(open.join(format!("{}.json", stem())), "{}").unwrap();
    let other = tmp.path().join("other");
    std::fs::create_dir(&other).unwrap();
    std::fs::set_permissions(&other, Permissions::from_mode(0o700)).unwrap();
    std::fs::write(other.join(format!("{}.json", stem())), "{}").unwrap();
    let linked = tmp.path().join("linked");
    std::fs::create_dir(&linked).unwrap();
    symlink(&dir, linked.join(DIR)).unwrap();

    let name = format!("{}.report", stem());
    for target in [
        home.join(".bashrc"),
        PathBuf::from(format!("{DIR}/{name}")),
        dir.join("x.report"),
        dir.join(format!("4242-1-{HEX}.json")),
        dir.join(format!("4242-2-{HEX}.report")),
        open.join(&name),
        other.join(&name),
        linked.join(DIR).join(&name),
    ] {
        assert_eq!(run(&target, &stdin, now), "", "{}", target.display());
    }
    assert_eq!(std::fs::read_to_string(home.join(".bashrc")).unwrap(), "rc");
    assert!(!report.exists());
    assert!(!open.join(&name).exists());
    assert!(!other.join(&name).exists());
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);

    // Not status JSON: nothing written, nothing printed.
    assert_eq!(run(&report, &input(tmp.path(), "not json"), now), "");
    assert!(!report.exists());

    assert_eq!(run(&report, &stdin, now), "Opus 5.5 · 5h 63%\n");
    assert_eq!(
        read_report(&report).unwrap().session_id.as_deref(),
        Some(ID)
    );
}

#[test]
fn read_bounded_stops_at_deadline_and_limit() {
    let (r, w) = std::io::pipe().unwrap();
    let start = Instant::now();
    assert_eq!(
        read_bounded(r.as_fd(), 100, start + Duration::from_millis(200)),
        None
    );
    assert!(start.elapsed() < Duration::from_secs(2));
    drop(w);

    let (r, mut w) = std::io::pipe().unwrap();
    w.write_all(&[b'x'; 101]).unwrap();
    drop(w);
    let later = Instant::now() + Duration::from_secs(5);
    assert_eq!(read_bounded(r.as_fd(), 100, later), None);

    let (r, mut w) = std::io::pipe().unwrap();
    w.write_all(b"abc").unwrap();
    drop(w);
    assert_eq!(read_bounded(r.as_fd(), 100, later), Some(b"abc".to_vec()));
}

#[test]
fn read_report_refuses_links_fifos_and_big_files() {
    let (tmp, report) = usage_dir();
    let now = now_secs();
    let big = tmp.path().join("big");
    std::fs::write(&big, format!("{}{}", sample(now), " ".repeat(5000))).unwrap();
    symlink(&big, &report).unwrap();
    assert_eq!(read_report(&report), None);
    std::fs::remove_file(&report).unwrap();
    std::fs::copy(&big, &report).unwrap();
    assert_eq!(read_report(&report), None, "over 4 KiB");
    std::fs::remove_file(&report).unwrap();
    let st = Command::new("mkfifo").arg(&report).status().unwrap();
    assert!(st.success());
    let start = Instant::now();
    assert_eq!(read_report(&report), None);
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
fn status_command_quotes_paths_for_the_shell() {
    for s in ["plain", "it's", "a'b'c", "$(x) `y`", "'", ""] {
        let out = Command::new("sh")
            .arg("-c")
            .arg(format!("printf %s {}", shell_quote(s)))
            .output()
            .unwrap();
        assert_eq!(String::from_utf8(out.stdout).unwrap(), s);
    }

    let tmp = tempfile::tempdir().unwrap();
    let weird = tmp.path().join("a b");
    std::fs::create_dir(&weird).unwrap();
    let exe = weird.join("'x$y`;z\"");
    std::fs::write(&exe, "#!/bin/sh\nprintf '%s|' \"$0\" \"$@\"\n").unwrap();
    std::fs::set_permissions(&exe, Permissions::from_mode(0o755)).unwrap();
    let state = tmp.path().join("state");
    std::fs::create_dir(&state).unwrap();

    let files = prepare(&state, &exe).unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&files.settings).unwrap()).unwrap();
    let top = json.as_object().unwrap();
    assert_eq!(top.len(), 1, "{json}");
    let line = top["statusLine"].as_object().unwrap();
    assert_eq!(line.len(), 2, "{json}");
    assert_eq!(line["type"], "command");
    let command = line["command"].as_str().unwrap();
    let out = Command::new("sh").arg("-c").arg(command).output().unwrap();
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        format!(
            "{}|--ai-statusline|{}|",
            exe.display(),
            files.report.display()
        )
    );

    let mode = |p: &Path| std::fs::metadata(p).unwrap().mode() & 0o777;
    assert_eq!(mode(&files.settings), 0o600);
    assert_eq!(mode(&state.join(DIR)), 0o700);
    assert_eq!(files.settings.with_extension("report"), files.report);
    let name = files.report.file_name().unwrap().to_str().unwrap();
    assert!(
        name.starts_with(&format!("{}-", std::process::id())),
        "{name}"
    );
    assert!(name.ends_with(".report"), "{name}");

    // A second session gets other files; dropping one removes both of its files.
    let second = prepare(&state, &exe).unwrap();
    assert_ne!(second.settings, files.settings);
    std::fs::write(&files.report, "{}").unwrap();
    let (settings, report) = (files.settings.clone(), files.report.clone());
    drop(files);
    assert!(!settings.exists() && !report.exists());
    assert!(second.settings.exists());
}

#[test]
fn prepare_refuses_unsafe_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let state = tmp.path().join("state");
    std::fs::create_dir(&state).unwrap();
    let file = |name: &str| {
        let p = tmp.path().join(name);
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&p, Permissions::from_mode(0o755)).unwrap();
        p
    };
    let good = file("vr-ide");
    for exe in [
        PathBuf::from("vr-ide"),
        file("vr\nide"),
        file("vr\\ide"),
        file("vr-ide (deleted)"),
        tmp.path().to_path_buf(),
        tmp.path().join("missing"),
    ] {
        assert!(prepare(&state, &exe).is_err(), "{exe:?}");
    }
    let odd_state = tmp.path().join("st\nate");
    std::fs::create_dir(&odd_state).unwrap();
    assert!(prepare(&odd_state, &good).is_err());
    let linked_state = tmp.path().join("linked");
    std::fs::create_dir(&linked_state).unwrap();
    let elsewhere = tmp.path().join("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    symlink(&elsewhere, linked_state.join(DIR)).unwrap();
    assert!(prepare(&linked_state, &good).is_err());
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
    let made = |s: &Path| {
        std::fs::read_dir(s.join(DIR))
            .map(|d| d.count())
            .unwrap_or(0)
    };
    assert_eq!(made(&state), 0);
    assert_eq!(made(&odd_state), 0);
}

/// Sets the modification time of `p` (not of what a link points at).
fn age(p: &Path, when: &str) {
    let st = Command::new("touch")
        .args(["-h", "-d", when])
        .arg(p)
        .status()
        .unwrap();
    assert!(st.success());
}

/// D? (T3): a pid that is not alive here is not proof the window ended (another pid namespace,
/// a state folder shared over the network), so a stem goes only when its newest file is older
/// than a day as well.
#[test]
fn sweep_removes_only_files_of_ended_windows() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join(DIR);
    std::fs::create_dir(&dir).unwrap();
    std::fs::set_permissions(&dir, Permissions::from_mode(0o700)).unwrap();
    let dead = {
        let mut c = Command::new("true").spawn().unwrap();
        let pid = c.id();
        c.wait().unwrap();
        pid
    };
    let me = std::process::id();
    let victim = tmp.path().join("victim");
    std::fs::write(&victim, "keep").unwrap();
    let old = |n: &str| {
        std::fs::write(dir.join(n), "x").unwrap();
        age(&dir.join(n), "2 days ago");
    };
    let fresh = |n: &str| std::fs::write(dir.join(n), "x").unwrap();
    // This window's files, old or not.
    old(&format!("{me}-1-{HEX}.json"));
    fresh(&format!("{me}-1-{HEX}.report"));
    // Not usage files.
    old("notes.txt");
    old(&format!("{dead}-1-short.json"));
    old(&format!("{dead}-1-{HEX}.jsonx"));
    // A dead pid, but recent files: maybe a window this pid namespace cannot see.
    fresh(&format!("{dead}-3-{HEX}.json"));
    // Old settings with a recent report: the same.
    old(&format!("{dead}-4-{HEX}.json"));
    fresh(&format!("{dead}-4-{HEX}.report"));
    let kept: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    // An ended window: dead pid, everything older than a day.
    let gone = [
        format!("{dead}-1-{HEX}.json"),
        format!("{dead}-1-{HEX}.report"),
        format!("{dead}-1-{HEX}.report.tmp77"),
    ];
    for n in &gone {
        old(n);
    }
    let link = format!("{dead}-2-{HEX}.json");
    symlink(&victim, dir.join(&link)).unwrap();
    age(&dir.join(&link), "2 days ago");

    sweep(tmp.path());
    for n in &kept {
        assert!(dir.join(n).exists(), "{n} kept");
    }
    for n in gone.iter().chain([&link]) {
        assert!(
            std::fs::symlink_metadata(dir.join(n)).is_err(),
            "{n} removed"
        );
    }
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep");
}

#[test]
fn poll_reads_only_after_a_change_at_most_once_a_second() {
    let (_tmp, report) = usage_dir();
    let now = now_secs();
    let first = parse_status_at(sample(now).as_bytes(), now).unwrap();
    let second = Usage {
        model: Some("Sonnet".into()),
        ..first.clone()
    };
    let third = Usage {
        model: Some("Haiku".into()),
        ..first.clone()
    };
    let mut poll = Poll::default();
    let t0 = Instant::now();
    let at = |ms: u64| t0 + Duration::from_millis(ms);
    assert_eq!(poll.check(&report, t0), None, "no report yet");
    write_report(&report, &first).unwrap();
    assert_eq!(poll.check(&report, at(100)), None, "looked at 100 ms ago");
    assert_eq!(poll.check(&report, at(300)), Some(Some(first.clone())));
    assert_eq!(poll.check(&report, at(5000)), None, "unchanged");
    // A second report right after a read is read once the second is over, without any
    // other event.
    write_report(&report, &second).unwrap();
    assert_eq!(poll.check(&report, at(5300)), Some(Some(second)));
    write_report(&report, &third).unwrap();
    assert_eq!(poll.check(&report, at(5800)), None, "read 500 ms ago");
    assert_eq!(poll.check(&report, at(6300)), Some(Some(third)));
    assert_eq!(poll.check(&report, at(9000)), None);
}

/// A helper that checked the settings file before its session closed does not leave a report
/// behind (SEC-22: no file outlives its session).
#[test]
fn report_never_outlives_its_session() {
    let (_tmp, report) = usage_dir();
    let (dir, name) = open_report_dir(&report).unwrap();
    // The session closes between the helper's check and its rename.
    std::fs::remove_file(report.with_extension("json")).unwrap();
    let _ = commit_report(&dir, &name, b"{}");
    assert!(std::fs::symlink_metadata(&report).is_err());
    assert_eq!(
        std::fs::read_dir(report.parent().unwrap()).unwrap().count(),
        0
    );
}

// ---- codex's notify channel (AC-8, SEC-32) ---------------------------------------------------

/// What codex 0.144.1 appends to the notify program's argv after a turn (fields from its
/// binary), with the user's text the helper must drop.
fn notify_payload(id: &str) -> String {
    format!(
        r#"{{"type":"agent-turn-complete","thread-id":"{id}","turn-id":"t1","cwd":"/w","client":"codex-tui","input-messages":["my secret"],"last-assistant-message":"done"}}"#
    )
}

#[test]
fn codex_notify_keeps_only_a_valid_thread_id() {
    assert_eq!(parse_codex_notify(&notify_payload(ID)).as_deref(), Some(ID));
    let other_type = notify_payload(ID).replace("agent-turn-complete", "approval-requested");
    let no_type = notify_payload(ID).replace(r#""type":"agent-turn-complete","#, "");
    let big = format!(
        r#"{{"type":"agent-turn-complete","thread-id":"{ID}","input-messages":["{}"]}}"#,
        "x".repeat(MAX_NOTIFY)
    );
    for bad in [
        notify_payload("--dangerously-bypass-approvals-and-sandbox"),
        notify_payload("-x"),
        notify_payload("abc-123"),
        notify_payload(&ID.to_uppercase()),
        notify_payload(&format!("{ID}\\n")),
        notify_payload(""),
        other_type,
        no_type,
        r#"{"type":"agent-turn-complete","thread-id":7}"#.to_string(),
        "not json".into(),
        "[]".into(),
        big,
    ] {
        assert_eq!(parse_codex_notify(&bad), None, "{bad:.120}");
    }
    // Kept as a report holding the id alone: none of the user's text.
    let u = Usage {
        session_id: parse_codex_notify(&notify_payload(ID)),
        ..Usage::default()
    };
    assert_eq!(
        String::from_utf8(report_json(&u)).unwrap(),
        format!(r#"{{"session_id":"{ID}"}}"#)
    );
}

#[test]
fn codex_notify_helper_writes_only_its_own_report() {
    let (tmp, report) = usage_dir();
    let home = tmp.path().join("home");
    std::fs::create_dir(&home).unwrap();
    std::fs::write(home.join(".bashrc"), "rc").unwrap();
    let dir = report.parent().unwrap().to_path_buf();
    for target in [
        home.join(".bashrc"),
        dir.join("x.report"),
        dir.join(format!("4242-2-{HEX}.report")),
    ] {
        codex_helper(&target, OsStr::new(&notify_payload(ID)));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "{target:?}");
    }
    assert_eq!(std::fs::read_to_string(home.join(".bashrc")).unwrap(), "rc");
    // A forged or other payload writes nothing.
    codex_helper(&report, OsStr::new(&notify_payload("--x")));
    codex_helper(
        &report,
        std::os::unix::ffi::OsStrExt::from_bytes(b"{\"type\":\xff}"),
    );
    assert!(!report.exists());

    codex_helper(&report, OsStr::new(&notify_payload(ID)));
    let text = std::fs::read_to_string(&report).unwrap();
    assert_eq!(text, format!(r#"{{"session_id":"{ID}"}}"#));
    assert_eq!(std::fs::metadata(&report).unwrap().mode() & 0o777, 0o600);
    assert_eq!(
        read_report(&report).unwrap().session_id.as_deref(),
        Some(ID)
    );
}

/// SEC-32: codex is told the helper through `-c notify=<TOML array>` built by a serializer —
/// two argv items, no shell — naming the exe, the flag and the report; the marker file beside the
/// report is private and goes with the session.
#[test]
fn codex_channel_names_the_helper_in_argv() {
    let tmp = tempfile::tempdir().unwrap();
    let weird = tmp.path().join("a b");
    std::fs::create_dir(&weird).unwrap();
    let exe = weird.join("'x$y`;z\"]");
    std::fs::write(&exe, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&exe, Permissions::from_mode(0o755)).unwrap();
    let state = tmp.path().join("state");
    std::fs::create_dir(&state).unwrap();

    let files = prepare_codex(&state, &exe).unwrap();
    assert_eq!(files.args.len(), 2, "{:?}", files.args);
    assert_eq!(files.args[0], "-c");
    let value = files.args[1]
        .to_str()
        .unwrap()
        .strip_prefix("notify=")
        .unwrap();
    let table: toml::Table = toml::from_str(&format!("notify = {value}")).unwrap();
    let argv: Vec<&str> = table["notify"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(
        argv,
        [
            exe.to_str().unwrap(),
            "--ai-codex-notify",
            files.report.to_str().unwrap()
        ]
    );
    let mode = |p: &Path| std::fs::metadata(p).unwrap().mode() & 0o777;
    assert_eq!(mode(&files.settings), 0o600);
    assert_eq!(files.settings.with_extension("report"), files.report);
    // The helper accepts the report it was given.
    codex_helper(&files.report, OsStr::new(&notify_payload(ID)));
    assert_eq!(
        read_report(&files.report).unwrap().session_id.as_deref(),
        Some(ID)
    );
    let (settings, report) = (files.settings.clone(), files.report.clone());
    drop(files);
    assert!(!settings.exists() && !report.exists());

    // claude's files name their channel too: `--settings <file>`.
    let claude = prepare(&state, &exe).unwrap();
    assert_eq!(
        claude.args,
        [OsString::from("--settings"), claude.settings.clone().into()]
    );
    // The same path rules as claude's helper.
    for bad in ["vr\nide", "vr\\ide"] {
        let p = tmp.path().join(bad);
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&p, Permissions::from_mode(0o755)).unwrap();
        assert!(prepare_codex(&state, &p).is_err(), "{bad:?}");
    }
}

// ---- display (AC-4) -------------------------------------------------------------------------

fn window(pct: f64, resets_at: Option<i64>) -> Option<Window> {
    Some(Window {
        used_percentage: pct,
        resets_at,
    })
}

#[test]
fn usage_suffix_takes_the_higher_window() {
    let mut u = Usage::default();
    assert_eq!(usage_suffix(&u), None, "no window known");
    u.five_hour = window(63.0, None);
    assert_eq!(usage_suffix(&u), Some((" 63%".into(), false)));
    u.seven_day = window(63.0, None);
    assert_eq!(
        usage_suffix(&u),
        Some((" 63%".into(), false)),
        "a tie: 5-hour"
    );
    u.seven_day = window(91.0, None);
    assert_eq!(usage_suffix(&u), Some((" 91%w".into(), true)));
    u.five_hour = None;
    u.seven_day = window(20.0, None);
    assert_eq!(
        usage_suffix(&u),
        Some((" 20%w".into(), false)),
        "weekly only"
    );
    // Shown whole-number part, so the number never says 90 without the warning.
    u.seven_day = window(89.6, None);
    assert_eq!(usage_suffix(&u), Some((" 89%w".into(), false)));
    u.seven_day = window(90.0, None);
    assert_eq!(usage_suffix(&u), Some((" 90%w".into(), true)));
    u.seven_day = window(130.0, None);
    assert_eq!(
        usage_suffix(&u),
        Some((" 130%w".into(), true)),
        "extra usage"
    );
}

/// D26: `HH:MM` within a day, `Mon D HH:MM` beyond; ` UTC` when the zone is unknown; local
/// time with the zone's offset at the reset instant.
#[test]
fn ai_usage_reset_clock_formats() {
    let utc = crate::ai::limits::Zone::fixed("UTC", 0);
    assert_eq!(reset_clock(NOW + 3600, NOW, &utc), "15:13");
    assert_eq!(reset_clock(NOW + 3 * 86_400, NOW, &utc), "Sep 24 14:13");
    let berlin = crate::ai::limits::Zone::fixed("Europe/Berlin", 7200);
    assert_eq!(reset_clock(NOW + 3600, NOW, &berlin), "17:13");
    let unknown = crate::ai::limits::Zone::unknown();
    assert_eq!(reset_clock(NOW + 3600, NOW, &unknown), "15:13 UTC");
}

#[test]
fn ai_usage_text_lists_both_windows() {
    let utc = crate::ai::limits::Zone::fixed("UTC", 0);
    let u = Usage {
        five_hour: window(63.0, Some(NOW + 3600)),
        seven_day: window(41.0, Some(NOW + 3 * 86_400)),
        ..Usage::default()
    };
    assert_eq!(
        usage_text("claude@work", &u, NOW, &utc),
        "claude@work: 5-hour 63% (resets 15:13) · weekly 41% (resets Sep 24 14:13)"
    );
    let u = Usage {
        five_hour: window(63.0, None),
        ..Usage::default()
    };
    assert_eq!(
        usage_text("claude", &u, NOW, &utc),
        "claude: 5-hour 63% · weekly not reported"
    );
}
