//! SEC-S1..S5, S10, S11 at the unit level: the target validator, the argv, the fixed
//! LocalCommand, the bounded `ssh -G` probe, the recent file and the config parser. Nothing here
//! connects anywhere: `ssh -G` only prints a configuration, and the probe tests run shims.

use std::ffi::OsString;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::*;

fn t(s: &str) -> Target {
    Target::parse(s).unwrap_or_else(|| panic!("{s:?} should be a target"))
}

fn strs(v: &[OsString]) -> Vec<String> {
    v.iter().map(|s| s.to_string_lossy().into_owned()).collect()
}

/// An executable script in `dir`.
fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

// ---- SEC-S1 ---------------------------------------------------------------------------------

#[test]
fn sec_s1_refused_targets() {
    let long_host = "a".repeat(254);
    let long_user = format!("{}@h", "u".repeat(65));
    let refused = [
        "",
        " ",
        "-oProxyCommand=x",
        "-F/tmp/c",
        "a -p 1",
        "a\tb",
        "a\x1b[2Jb",
        "a\nb",
        "a\u{7f}",
        "a\u{85}b",
        "%h",
        "a%d",
        "a$HOME",
        "${HOME}",
        "u@-x",
        "-u@h",
        "u@h@h",
        "@h",
        "u@",
        "ssh://h:2",
        "h:0",
        "h:65536",
        "h:+22",
        "h:-1",
        "h:",
        "h:22x",
        "h:000022",
        "\u{202e}h",
        "hé",
        "h/x",
        "h\\x",
        "h'x",
        "h\"x",
        "h`x`",
        "h;x",
        "h x",
        ".h",
        "_h",
        "::1",
        "[::1",
        "[]",
        "[h]",
        "[::1]x",
        "[::1%eth0]:22",
        "[::1]:0",
        "u@[::1]:99999",
        "h:22:22",
        long_host.as_str(),
        long_user.as_str(),
    ];
    for s in refused {
        assert!(Target::parse(s).is_none(), "{s:?} must be refused");
    }
}

#[test]
fn sec_s1_accepted_targets_and_their_text() {
    for s in [
        "h",
        "bob@h:2222",
        "[::1]:22",
        "u@[fe80::1]:2200",
        "[::1]",
        "web-1.example.com",
        "my_host",
        "10.0.0.5:22",
        "_svc.x@h",
        "h:65535",
        "h:1",
    ] {
        assert_eq!(t(s).as_str(), s);
    }
    assert!(t("web1").is_alias());
    assert!(!t("bob@web1").is_alias() && !t("web1:22").is_alias() && !t("[::1]").is_alias());
}

// ---- SEC-S2, SEC-S5 -------------------------------------------------------------------------

#[test]
fn sec_s2_argv_for_each_target_shape() {
    let lc = "printf 'x'";
    assert_eq!(
        strs(&argv(&t("h"), None)),
        ["-o", "ControlMaster=no", "--", "h"]
    );
    assert_eq!(
        strs(&argv(&t("bob@h:2222"), Some(lc))),
        [
            "-l",
            "bob",
            "-p",
            "2222",
            "-o",
            "ControlMaster=no",
            "-o",
            "PermitLocalCommand=yes",
            "-o",
            "LocalCommand=printf 'x'",
            "--",
            "h"
        ]
    );
    // The address goes without its brackets, after `--`.
    assert_eq!(
        strs(&argv(&t("u@[::1]:22"), None)),
        ["-l", "u", "-p", "22", "-o", "ControlMaster=no", "--", "::1"]
    );
    assert_eq!(
        strs(&probe_argv(&t("bob@h:2222"))),
        ["-G", "-l", "bob", "-p", "2222", "--", "h"]
    );
    assert_eq!(strs(&probe_argv(&t("h"))), ["-G", "--", "h"]);
}

/// vr-ide passes nothing that weakens or changes the user's ssh settings, and builds no shell
/// command line (the words are assembled here so this file does not match itself).
#[test]
fn sec_s2_the_ssh_code_names_no_shell_and_no_weakening_option() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = [
        dir.join("ssh/mod.rs"),
        dir.join("ssh/config.rs"),
        dir.join("ssh/recent.rs"),
        dir.join("commands/ssh.rs"),
    ];
    let banned = [
        format!("\"{}\"", "sh"),
        format!("\"-{}\"", "c"),
        ["Strict", "HostKeyChecking"].concat(),
        ["UserKnown", "HostsFile"].concat(),
        ["Batch", "Mode"].concat(),
        format!("\"-{}\"", "F"),
        format!("\"-{}\"", "i"),
    ];
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        for b in &banned {
            assert!(!text.contains(b.as_str()), "{} holds {b}", f.display());
        }
    }
}

#[test]
fn sec_s2_ssh_is_found_on_absolute_path_entries_only() {
    let d = tempfile::tempdir().unwrap();
    let abs = d.path().join("bin");
    std::fs::create_dir(&abs).unwrap();
    let shim = script(&abs, "ssh", "exit 0");
    // Not executable, a directory: skipped.
    let other = d.path().join("other");
    std::fs::create_dir_all(other.join("ssh")).unwrap();
    let noexec = d.path().join("noexec");
    std::fs::create_dir(&noexec).unwrap();
    std::fs::write(noexec.join("ssh"), "x").unwrap();
    let path = format!(
        ".::rel:{}:{}:{}",
        other.display(),
        noexec.display(),
        abs.display()
    );
    assert_eq!(find_in_path(path.as_ref(), "ssh"), Some(shim));
    assert_eq!(find_in_path(".::rel".as_ref(), "ssh"), None);
    assert_eq!(find_in_path("".as_ref(), "ssh"), None);
}

// ---- SEC-S3 ---------------------------------------------------------------------------------

fn nonce_of(lc: &str) -> &str {
    &lc["printf '\\033]7717;vr-ide-ssh;".len()..lc.len() - "\\007'".len()]
}

fn is_local_command(lc: &str) -> bool {
    let Some(rest) = lc.strip_prefix("printf '\\033]7717;vr-ide-ssh;") else {
        return false;
    };
    let Some(nonce) = rest.strip_suffix("\\007'") else {
        return false;
    };
    nonce.len() == 32
        && nonce
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[test]
fn sec_s3_ten_thousand_local_commands_are_fixed_apart_from_a_unique_hex_nonce() {
    let mut seen = std::collections::HashSet::new();
    for _ in 0..10_000 {
        let n = Nonce::new().expect("the OS random source");
        let lc = n.local_command();
        assert!(is_local_command(&lc), "{lc}");
        for bad in ['%', '$', '`', '"', '\n'] {
            assert!(!lc.contains(bad), "{lc}");
        }
        let nonce = nonce_of(&lc).to_string();
        assert_eq!(
            n.marker(),
            format!("\x1b]7717;vr-ide-ssh;{nonce}\x07").into_bytes()
        );
        assert!(seen.insert(nonce), "a nonce came twice");
    }
}

#[test]
fn sec_s3_shells_print_exactly_the_marker() {
    let n = Nonce::new().unwrap();
    let mut ran = 0;
    for shell in ["sh", "bash", "dash", "zsh", "fish"] {
        let Some(exe) = find_in_path(std::env::var_os("PATH").unwrap().as_ref(), shell) else {
            continue;
        };
        let out = std::process::Command::new(exe)
            .arg(["-", "c"].concat())
            .arg(n.local_command())
            .output()
            .unwrap();
        assert_eq!(out.stdout, n.marker(), "{shell}");
        ran += 1;
    }
    assert!(ran >= 2, "sh and at least one more shell are installed");
}

/// The real `ssh -G` (prints the configuration, connects nowhere) keeps the value unchanged.
#[test]
fn sec_s3_ssh_echoes_the_local_command_unchanged() {
    let Some(ssh) = find_in_path(std::env::var_os("PATH").unwrap().as_ref(), "ssh") else {
        eprintln!("not-run: no ssh installed");
        return;
    };
    let n = Nonce::new().unwrap();
    let out = std::process::Command::new(ssh)
        .arg("-G")
        .arg(["-", "F"].concat())
        .arg("/dev/null")
        .args(argv(&t("h"), Some(&n.local_command())))
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.lines()
            .any(|l| l == format!("localcommand {}", n.local_command())),
        "{text}"
    );
    assert!(
        text.lines().any(|l| l == "permitlocalcommand yes"),
        "{text}"
    );
    assert!(text.lines().any(|l| l == "controlmaster false"), "{text}");
}

#[test]
fn sec_s7_the_nonce_does_not_show_in_debug_output() {
    let n = Nonce::new().unwrap();
    let lc = n.local_command();
    let shown = format!("{n:?}");
    assert!(!shown.contains(nonce_of(&lc)), "{shown}");
}

// ---- SEC-S4 ---------------------------------------------------------------------------------

/// A HOME whose ssh config is `config`, and a shim that runs the real ssh with it.
fn real_ssh_with_config(config: &str) -> Option<(tempfile::TempDir, PathBuf)> {
    let real = find_in_path(std::env::var_os("PATH")?.as_ref(), "ssh")?;
    let d = tempfile::tempdir().unwrap();
    let cfg = d.path().join("config");
    std::fs::write(&cfg, config).unwrap();
    let shim = script(
        d.path(),
        "ssh",
        &format!("exec {} -{} {} \"$@\"", real.display(), 'F', cfg.display()),
    );
    Some((d, shim))
}

const PROBE_WAIT: Duration = Duration::from_secs(5);

#[test]
fn sec_s4_real_ssh_g_output_is_parsed() {
    let Some((d, ssh)) = real_ssh_with_config(
        "Host plain\n  HostName 127.0.0.1\nHost mine\n  PermitLocalCommand no\n  LocalCommand touch /tmp/never\n",
    ) else {
        eprintln!("not-run: no ssh installed");
        return;
    };
    assert_eq!(
        probe(&ssh, &t("plain"), PROBE_WAIT),
        Probe::Clean { shared: false }
    );
    assert_eq!(
        probe(&ssh, &t("bob@plain:2222"), PROBE_WAIT),
        Probe::Clean { shared: false }
    );
    assert_eq!(probe(&ssh, &t("mine"), PROBE_WAIT), Probe::UserLocalCommand);
    // An existing control socket: the session may ride on a shared connection.
    let sock = d.path().join("cm.sock");
    let _listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
    std::fs::write(
        d.path().join("config"),
        format!("Host shared\n  ControlPath {}\n", sock.display()),
    )
    .unwrap();
    assert_eq!(
        probe(&ssh, &t("shared"), PROBE_WAIT),
        Probe::Clean { shared: true }
    );
    assert_eq!(
        probe(&ssh, &t("plain"), PROBE_WAIT),
        Probe::Clean { shared: false }
    );
}

#[test]
fn sec_s4_a_failing_probe_is_failed() {
    let d = tempfile::tempdir().unwrap();
    let ssh = script(d.path(), "ssh", "echo 'localcommand x'; exit 3");
    assert_eq!(probe(&ssh, &t("h"), PROBE_WAIT), Probe::Failed);
    assert_eq!(
        probe(&d.path().join("missing"), &t("h"), PROBE_WAIT),
        Probe::Failed
    );
}

#[test]
fn sec_s4_the_probe_gets_no_stdin_no_terminal_and_only_the_probe_argv() {
    let d = tempfile::tempdir().unwrap();
    let log = d.path().join("log");
    let ssh = script(
        d.path(),
        "ssh",
        &format!(
            "{{ echo \"argv:$*\"; if read x; then echo stdin:open; else echo stdin:eof; fi; \
             if (: </dev/tty) 2>/dev/null; then echo tty:yes; else echo tty:no; fi; \
             echo \"sid:$(ps -o sid= -p $$ | tr -d ' ') pid:$$\"; }} > {}",
            log.display()
        ),
    );
    assert_eq!(
        probe(&ssh, &t("bob@h:2222"), PROBE_WAIT),
        Probe::Clean { shared: false }
    );
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(text.contains("argv:-G -l bob -p 2222 -- h\n"), "{text}");
    assert!(text.contains("stdin:eof"), "{text}");
    assert!(text.contains("tty:no"), "{text}");
    let ids = text.lines().find(|l| l.starts_with("sid:")).unwrap();
    let (sid, pid) = ids[4..].split_once(" pid:").unwrap();
    assert_eq!(sid, pid, "the probe leads its own session: {ids}");
}

/// Processes whose command line holds `needle` (/proc scan).
fn procs_with(needle: &str) -> Vec<u32> {
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_string_lossy().parse::<u32>().ok())
        .filter(|pid| {
            std::fs::read(format!("/proc/{pid}/cmdline"))
                .is_ok_and(|c| String::from_utf8_lossy(&c).contains(needle))
        })
        .collect()
}

#[test]
fn sec_s4_a_hanging_probe_is_killed_with_its_group_at_the_timeout() {
    let d = tempfile::tempdir().unwrap();
    // A helper of the probe (as `Match exec` would start) that outlives a plain kill of ssh.
    let ssh = script(
        d.path(),
        "ssh",
        "sleep 301 &\nsleep 302\necho 'localcommand late'",
    );
    let tag = d.path().file_name().unwrap().to_string_lossy().into_owned();
    let t0 = Instant::now();
    assert_eq!(
        probe(&ssh, &t("h"), Duration::from_millis(400)),
        Probe::Failed
    );
    let took = t0.elapsed();
    assert!(
        took >= Duration::from_millis(400) && took < Duration::from_secs(3),
        "{took:?}"
    );
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let left = procs_with(&tag);
        if left.is_empty() {
            break;
        }
        assert!(Instant::now() < deadline, "left running: {left:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(PROBE_TIMEOUT, Duration::from_secs(5));
}

#[test]
fn sec_s4_probe_output_over_the_cap_is_failed() {
    let d = tempfile::tempdir().unwrap();
    let ssh = script(
        d.path(),
        "ssh",
        "head -c 400000 /dev/zero | tr '\\0' 'a'; echo; sleep 303",
    );
    let t0 = Instant::now();
    assert_eq!(probe(&ssh, &t("h"), PROBE_WAIT), Probe::Failed);
    assert!(t0.elapsed() < Duration::from_secs(3), "{:?}", t0.elapsed());
    assert_eq!(PROBE_OUTPUT_CAP, 256 * 1024);
}

// ---- SEC-S10 --------------------------------------------------------------------------------

#[test]
fn sec_s10_recent_file_is_private_bounded_and_most_recent_first() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("state/vr-ide");
    let file = dir.join("ssh-recent");
    // A permissive umask must not widen the modes.
    let old = rustix::process::umask(rustix::fs::Mode::from_raw_mode(0o022));
    let listed = recent::record(&file, &t("bob@h:2222")).unwrap();
    rustix::process::umask(old);
    assert_eq!(listed, [t("bob@h:2222")]);
    assert_eq!(std::fs::metadata(&file).unwrap().mode() & 0o777, 0o600);
    assert_eq!(std::fs::metadata(&dir).unwrap().mode() & 0o777, 0o700);
    recent::record(&file, &t("web1")).unwrap();
    recent::record(&file, &t("bob@h:2222")).unwrap();
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "bob@h:2222\nweb1\n"
    );
    for i in 0..30 {
        recent::record(&file, &t(&format!("host{i}"))).unwrap();
    }
    let all = recent::load(&file);
    assert_eq!(all.len(), recent::MAX);
    assert_eq!(all[0], t("host29"));
    assert_eq!(std::fs::metadata(&file).unwrap().mode() & 0o777, 0o600);
    // No temp file is left beside it.
    let names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["ssh-recent"]);
}

#[test]
fn sec_s10_loading_revalidates_every_line_and_ignores_a_big_file() {
    let d = tempfile::tempdir().unwrap();
    let file = d.path().join("ssh-recent");
    std::fs::write(
        &file,
        "-oProxyCommand=x\na b\nok1\n\nu@h@h\nok1\nok2:22\n%h\n",
    )
    .unwrap();
    assert_eq!(recent::load(&file), [t("ok1"), t("ok2:22")]);
    let many: String = (0..10_000).map(|i| format!("host{i}\n")).collect();
    assert!(many.len() > 64 * 1024);
    std::fs::write(&file, &many).unwrap();
    assert!(recent::load(&file).is_empty(), "over 64 KiB is ignored");
    let fits: String = (0..5_000).map(|i| format!("h{i}\n")).collect();
    assert!(fits.len() <= 64 * 1024);
    std::fs::write(&file, &fits).unwrap();
    assert_eq!(recent::load(&file).len(), recent::MAX);
    assert!(recent::load(&d.path().join("missing")).is_empty());
    assert!(recent::load(d.path()).is_empty(), "a directory is no list");
}

#[test]
fn sec_s10_a_symlink_at_the_path_is_replaced_not_followed() {
    let d = tempfile::tempdir().unwrap();
    let victim = d.path().join("victim");
    std::fs::write(&victim, "keep me\n").unwrap();
    let file = d.path().join("ssh-recent");
    symlink(&victim, &file).unwrap();
    assert!(recent::load(&file).is_empty(), "a link is not read");
    recent::record(&file, &t("web1")).unwrap();
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep me\n");
    let meta = std::fs::symlink_metadata(&file).unwrap();
    assert!(meta.file_type().is_file());
    assert_eq!(meta.mode() & 0o777, 0o600);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "web1\n");
}

// ---- SEC-S11 --------------------------------------------------------------------------------

fn home_with(config: &str) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join(".ssh")).unwrap();
    std::fs::write(d.path().join(".ssh/config"), config).unwrap();
    d
}

#[test]
fn sec_s11_only_concrete_valid_host_tokens_are_listed() {
    let d = home_with(
        "# comment\nHost -oProxyCommand=x a*b !c \"d e\" ok\n  HostName 10.0.0.1\n\
         host=second third\n\tHOST fourth\nMatch host ok exec \"touch /tmp/never\"\n\
         Host fifth ? * u@h h:22 %h ok\nHostName notahost\nHostKeyAlias nope\n",
    );
    assert_eq!(
        config::hosts(d.path()),
        ["ok", "second", "third", "fourth", "fifth"]
    );
    assert!(config::hosts(&d.path().join("no-home")).is_empty());
}

#[test]
fn sec_s11_includes_are_followed_within_bounds() {
    let d = home_with("Include extra conf.d/*.conf ~/.ssh/tilde \"quoted name\"\nHost main\n");
    let ssh = d.path().join(".ssh");
    std::fs::write(
        ssh.join("extra"),
        "Host extra1\nInclude config\nInclude extra\n",
    )
    .unwrap();
    std::fs::create_dir(ssh.join("conf.d")).unwrap();
    std::fs::write(ssh.join("conf.d/b.conf"), "Host b1\n").unwrap();
    std::fs::write(ssh.join("conf.d/a.conf"), "Host a1\nInclude b.conf\n").unwrap();
    std::fs::write(ssh.join("conf.d/skip.txt"), "Host skipped\n").unwrap();
    std::fs::write(ssh.join("tilde"), "Host tilde1\n").unwrap();
    std::fs::write(ssh.join("quoted name"), "Host quoted1\n").unwrap();
    // Each file once (self-Include, A<->B loop), globs in name order, relative to ~/.ssh.
    assert_eq!(
        config::hosts(d.path()),
        ["extra1", "a1", "b1", "tilde1", "quoted1", "main"]
    );
}

#[test]
fn sec_s11_hostile_configs_neither_hang_nor_flood() {
    let d = home_with("");
    let ssh = d.path().join(".ssh");
    // A FIFO nobody writes to, a device, a directory, a huge file, a glob over `/`.
    rustix::fs::mknodat(
        rustix::fs::CWD,
        ssh.join("fifo"),
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::from_raw_mode(0o600),
        0,
    )
    .unwrap();
    std::fs::create_dir(ssh.join("dir")).unwrap();
    let big = std::fs::File::create(ssh.join("big")).unwrap();
    big.set_len(50 * 1024 * 1024).unwrap();
    let mut over = String::from("Host over-limit\n");
    over.push_str(&"#".repeat(1024 * 1024));
    std::fs::write(ssh.join("over"), over).unwrap();
    let many: String = (0..5_000).map(|i| format!("Host many{i}\n")).collect();
    std::fs::write(ssh.join("many"), many).unwrap();
    std::fs::write(
        ssh.join("config"),
        "Include /dev/zero fifo dir big over /* ${HOME}/x %d/y\nInclude config\nHost ok\nInclude many\n",
    )
    .unwrap();
    let t0 = Instant::now();
    let hosts = config::hosts(d.path());
    let took = t0.elapsed();
    assert!(took < Duration::from_millis(200), "{took:?}");
    assert_eq!(hosts[0], "ok");
    assert_eq!(hosts.len(), config::MAX_ALIASES);
    // A config that is itself a FIFO.
    std::fs::remove_file(ssh.join("config")).unwrap();
    std::fs::rename(ssh.join("fifo"), ssh.join("config")).unwrap();
    let t0 = Instant::now();
    assert!(config::hosts(d.path()).is_empty());
    assert!(t0.elapsed() < Duration::from_millis(200));
}

#[test]
fn sec_s11_file_count_depth_and_glob_matches_are_capped() {
    // A chain deeper than the depth cap.
    let d = home_with("Include chain0\n");
    let ssh = d.path().join(".ssh");
    for i in 0..40 {
        std::fs::write(
            ssh.join(format!("chain{i}")),
            format!("Host c{i}\nInclude chain{}\n", i + 1),
        )
        .unwrap();
    }
    let hosts = config::hosts(d.path());
    assert_eq!(hosts.len(), config::MAX_DEPTH, "{hosts:?}");
    // More files than the file cap, through globs that each match more than the glob cap.
    let d = home_with("Include a/* b/*\n");
    let ssh = d.path().join(".ssh");
    for sub in ["a", "b"] {
        std::fs::create_dir(ssh.join(sub)).unwrap();
        for i in 0..300 {
            std::fs::write(
                ssh.join(format!("{sub}/f{i:03}")),
                format!("Host {sub}{i}\n"),
            )
            .unwrap();
        }
    }
    let hosts = config::hosts(d.path());
    // The main file counts as one of the files.
    assert_eq!(hosts.len(), config::MAX_FILES - 1, "{}", hosts.len());
    assert_eq!(
        (config::MAX_GLOB_MATCHES, config::MAX_FILE_BYTES),
        (256, 1024 * 1024)
    );
}

// ---- SEC-S7, SEC-S9 -------------------------------------------------------------------------

/// Nothing typed into or printed by a terminal can reach a log: the emulator crate logs
/// nothing at all, and the panel and the ssh code log only the one I/O error of the recent
/// file (vr-ide installs no log sink either).
#[test]
fn sec_s9_terminal_and_ssh_code_log_no_keys_bytes_or_nonce() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let logs = |file: &Path| {
        let text = std::fs::read_to_string(file).unwrap_or_else(|e| panic!("{file:?}: {e}"));
        text.lines()
            .filter(|l| {
                ["tracing::", "eprintln!", "println!", "dbg!"]
                    .iter()
                    .any(|m| l.contains(m))
            })
            .map(|l| l.trim().to_string())
            .collect::<Vec<_>>()
    };
    for entry in std::fs::read_dir(crates.join("vr-term/src"))
        .unwrap()
        .flatten()
    {
        if entry.path().extension().is_some_and(|e| e == "rs") {
            assert_eq!(logs(&entry.path()), Vec::<String>::new(), "{entry:?}");
        }
    }
    let src = crates.join("vr-ide/src");
    for f in [
        "commands/terminal.rs",
        "ssh/mod.rs",
        "ssh/config.rs",
        "ssh/recent.rs",
    ] {
        assert_eq!(logs(&src.join(f)), Vec::<String>::new(), "{f}");
    }
    assert_eq!(
        logs(&src.join("commands/ssh.rs")),
        ["Err(e) => tracing::debug!(error = %e, \"ssh recent list not written\"),"]
    );
}

#[test]
fn sec_s4_a_probe_that_closes_its_output_but_stays_is_killed_at_the_timeout() {
    let d = tempfile::tempdir().unwrap();
    let ssh = script(
        d.path(),
        "ssh",
        "echo 'permitlocalcommand no'\nexec >/dev/null\nsleep 304",
    );
    let tag = d.path().file_name().unwrap().to_string_lossy().into_owned();
    let t0 = Instant::now();
    assert_eq!(
        probe(&ssh, &t("h"), Duration::from_millis(300)),
        Probe::Failed
    );
    assert!(t0.elapsed() < Duration::from_secs(3), "{:?}", t0.elapsed());
    let deadline = Instant::now() + Duration::from_secs(3);
    while !procs_with(&tag).is_empty() {
        assert!(Instant::now() < deadline, "left running");
        std::thread::sleep(Duration::from_millis(20));
    }
}
