//! SEC-1..SEC-3 at the unit level: the split, the destination found with ssh's grammar, the
//! options that keep vr-ide's prefix out, and the pure helpers the command window uses.

use std::path::Path;

use super::*;

fn words(line: &str) -> Vec<String> {
    split(line, Some(Path::new("/home/u"))).unwrap_or_else(|e| panic!("{line:?}: {e:?}"))
}

fn line(text: &str) -> Line {
    Line::parse(text, Some(Path::new("/home/u"))).unwrap_or_else(|e| panic!("{text:?}: {e:?}"))
}

fn refused(text: &str) -> LineError {
    Line::parse(text, Some(Path::new("/home/u"))).expect_err(text)
}

// ---- SEC-1 ----------------------------------------------------------------------------------

#[test]
fn sec_1_split_table() {
    let table: &[(&str, &[&str])] = &[
        ("-i 'a b' h", &["-i", "a b", "h"]),
        (r#""x\"y""#, &[r#"x"y"#]),
        (r#""a\\b\`c\$d\e""#, &[r"a\b`c$d\e"]),
        (r"a\ b", &["a b"]),
        ("'$HOME'", &["$HOME"]),
        ("$HOME", &["$HOME"]),
        ("*", &["*"]),
        ("`id`", &["`id`"]),
        ("~/k", &["/home/u/k"]),
        ("~", &["/home/u"]),
        ("~root/k", &["~root/k"]),
        ("'~'/k", &["~/k"]),
        ("a~/k", &["a~/k"]),
        ("a;b", &["a;b"]),
        ("a|b>c&d", &["a|b>c&d"]),
        ("  a   b  ", &["a", "b"]),
        ("'' h", &["", "h"]),
        ("a'b c'd", &["ab cd"]),
    ];
    for (input, want) in table {
        assert_eq!(words(input), *want, "{input:?}");
    }
}

#[test]
fn sec_1_refusals_have_fixed_notices_that_do_not_echo() {
    let secret = "hunter2";
    let cases = [
        (format!("\"{secret}"), LineError::UnclosedDouble),
        (format!("'{secret}"), LineError::UnclosedSingle),
        (format!("{secret}\\"), LineError::TrailingBackslash),
        (format!("{secret}\nb"), LineError::Control),
        (format!("{secret}\0b"), LineError::Control),
        (format!("{secret}\tb"), LineError::Control),
        (format!("{secret}\u{85}b"), LineError::Control),
        ("a".repeat(MAX_BYTES + 1), LineError::TooLong),
        (vec!["h"; MAX_WORDS + 1].join(" "), LineError::TooManyWords),
        ("   ".to_string(), LineError::Empty),
    ];
    for (input, want) in cases {
        assert_eq!(split(&input, None), Err(want), "{input:?}");
        assert!(!want.notice().contains(secret));
    }
    assert_eq!(
        LineError::UnclosedDouble.notice(),
        "/ssh: unclosed \" (quote paths with spaces, or use \\ )"
    );
    assert!(split(&"a".repeat(MAX_BYTES), None).is_ok());
    assert!(split(&vec!["h"; MAX_WORDS].join(" "), None).is_ok());
}

/// No shell in this module (the words are assembled so this file does not match itself).
#[test]
fn sec_1_the_module_names_no_shell() {
    let src = include_str!("args.rs");
    for banned in [format!("\"{}\"", "sh"), format!("\"-{}\"", "c")] {
        assert!(!src.contains(&banned), "{banned}");
    }
}

// ---- SEC-2 ----------------------------------------------------------------------------------

#[test]
fn sec_2_destination_and_label() {
    let table = [
        ("h", "h"),
        ("-p bank evil", "evil"),
        ("-vp 22 h", "h"),
        ("-vp22 h", "h"),
        ("h -p 2222", "h"),
        ("-oPort=1 h ls -la", "h"),
        ("-J jump host", "host"),
        ("ssh://u@h:2", "u@h:2"),
        (
            "-i credentials/id_fixecom root@app.fixecom.vn",
            "root@app.fixecom.vn",
        ),
        ("-- h -v", "h"),
        ("h -- -v", "h"),
        ("-l bob h", "h"),
        ("u@::1", "u@::1"),
        ("-o ProxyCommand=x h", "h"),
    ];
    for (input, label) in table {
        assert_eq!(line(input).target().as_str(), label, "{input:?}");
    }
}

#[test]
fn sec_2_refused_destinations() {
    for input in [
        "-oProxyCommand=x",
        "-- -oProxyCommand=x",
        "h\u{202e}x",
        "-p",
        "-Z h",
        "-v",
        "%h",
        "u@h@h",
        "h:22 ls",
        "'-x'",
        "ssh://h/x",
        "-p 22",
    ] {
        let e = refused(input);
        assert!(
            matches!(e, LineError::NoDestination | LineError::BadOption),
            "{input:?}: {e:?}"
        );
    }
    assert_eq!(LineError::NoDestination.notice(), crate::ssh::NOT_VALID);
}

#[test]
fn sec_2_old_targets_with_a_port_mean_what_they_meant() {
    let l = line("bob@h:2222");
    assert_eq!(l.text(), "-p 2222 bob@h");
    assert_eq!(l.words(), ["-p", "2222", "bob@h"]);
    assert_eq!(l.target().as_str(), "bob@h");
    assert_eq!(line("u@[::1]:22").words(), ["-p", "22", "u@::1"]);
    assert_eq!(line("web1").words(), ["web1"]);
}

#[test]
fn target_parse_accepts_the_ssh_uri_form() {
    let t = Target::parse("ssh://u@h:2").unwrap();
    assert_eq!(t.as_str(), "u@h:2");
    assert!(Target::parse("ssh://ssh://h").is_none());
    assert!(Target::parse("ssh://-h").is_none());
    assert!(Target::parse("ssh://").is_none());
}

// ---- SEC-3 ----------------------------------------------------------------------------------

#[test]
fn sec_3_words_that_keep_vr_ide_s_prefix_out() {
    for input in [
        "-o LocalCommand=x h",
        "-oPermitLocalCommand=no h",
        "-M h",
        "-vM h",
        "-S /tmp/s h",
        "-O check h",
        "-G h",
        "-V h",
        "-Q cipher h",
        "-f h",
        "-o controlpath=/tmp/s h",
        "-o ControlMaster=auto h",
        "-o ControlPersist=1 h",
        "h -M",
    ] {
        assert!(line(input).keeps_own_options(), "{input:?}");
    }
    for input in ["h", "-p 22 h", "-i k h", "-J j h", "h ls -M"] {
        assert!(!line(input).keeps_own_options(), "{input:?}");
    }
}

#[test]
fn local_command_words_are_named() {
    let name = |s: &str| line(s).local_command();
    assert_eq!(
        name(r#"-o ProxyCommand="ssh -W %h:%p jump" deploy@db1"#).as_deref(),
        Some("ProxyCommand")
    );
    assert_eq!(
        name("-oKnownHostsCommand=x h").as_deref(),
        Some("KnownHostsCommand")
    );
    assert_eq!(
        name("-o PKCS11Provider=/x.so h").as_deref(),
        Some("PKCS11Provider")
    );
    assert_eq!(
        name("-o securitykeyprovider=x h").as_deref(),
        Some("securitykeyprovider")
    );
    assert_eq!(name("-I /x.so h").as_deref(), Some("-I"));
    // Any -I the getopt walk finds, bundled or attached (SEC-5).
    assert_eq!(name("-vI /x.so h").as_deref(), Some("-I"));
    assert_eq!(name("-qI/x.so h").as_deref(), Some("-I"));
    assert_eq!(name("h -vI /x.so").as_deref(), Some("-I"));
    // A config file of the user's can hold Match exec or ProxyCommand; `none` reads none.
    assert_eq!(name("-F cfg h").as_deref(), Some("-F"));
    assert_eq!(name("-vFcfg h").as_deref(), Some("-F"));
    assert_eq!(name("-F none h"), None);
    assert_eq!(name("-Fnone h"), None);
    assert_eq!(name("-i k h"), None);
    assert_eq!(name("-J jump h"), None);
}

// ---- display helpers ------------------------------------------------------------------------

#[test]
fn the_shown_command_resolves_relative_path_values_only() {
    let base = Path::new("/home/user/DEV/fixecom");
    let shown = |s: &str| shown_command(line(s).words(), base);
    assert_eq!(
        shown("-i credentials/id_fixecom root@app.fixecom.vn"),
        "ssh -i /home/user/DEV/fixecom/credentials/id_fixecom root@app.fixecom.vn"
    );
    assert_eq!(
        shown("-icredentials/k -F cfg -o IdentityFile=k2 -E log -S s h"),
        "ssh -i/home/user/DEV/fixecom/credentials/k -F /home/user/DEV/fixecom/cfg -o \
         IdentityFile=/home/user/DEV/fixecom/k2 -E /home/user/DEV/fixecom/log -S \
         /home/user/DEV/fixecom/s h"
    );
    assert_eq!(shown("-i /abs/k -p 22 h"), "ssh -i /abs/k -p 22 h");
    assert_eq!(shown("-i 'a b' h"), "ssh -i '/home/user/DEV/fixecom/a b' h");
    // The remote command is shown as typed, not as paths.
    assert_eq!(shown("h cat -i x"), "ssh h cat -i x");
    assert_eq!(shown("-i ~/k h"), "ssh -i /home/u/k h");
    // `none` is a value of its own, not a file name.
    assert_eq!(
        shown("-F none -i none -o UserKnownHostsFile=none h"),
        "ssh -F none -i none -o UserKnownHostsFile=none h"
    );
}

#[test]
fn the_command_wraps_at_spaces_and_after_slashes() {
    let text = "runs: ssh -i /home/user/DEV/fixecom/credentials/id_fixecom root@app.fixecom.vn";
    let lines = wrap_command(text, 52, 6);
    assert_eq!(
        lines,
        [
            "runs: ssh -i /home/user/DEV/fixecom/credentials/",
            "      id_fixecom root@app.fixecom.vn"
        ]
    );
    assert_eq!(wrap_command("runs: ssh h", 52, 6), ["runs: ssh h"]);
}

#[test]
fn a_cut_row_keeps_the_first_option_and_the_destination() {
    let l = r#"-o ProxyCommand="ssh -W %h:%p jump" deploy@db1"#;
    let cut = cut_middle(l, 40).unwrap();
    assert!(cut.starts_with("-o ProxyCommand="), "{cut}");
    assert!(cut.ends_with("… deploy@db1"), "{cut}");
    assert!(
        ratatui::text::Span::raw(cut.as_str()).width() <= 40,
        "{cut}"
    );
    assert_eq!(cut_middle("h", 40).as_deref(), Some("h"));
    // A remote command after the destination is what goes.
    let cut = cut_middle("-J bastion deploy@10.0.4.12 tail -f /var/log/syslog", 36).unwrap();
    assert!(cut.contains("deploy@10.0.4.12…"), "{cut}");
    assert!(cut.starts_with("-J"), "{cut}");
}

// ---- classify (Tab completion, T4) ----------------------------------------------------------

#[test]
fn classify_finds_path_values_and_the_destination() {
    let at_end = |s: &str| classify(s, s.len());
    assert_eq!(at_end("-i cred"), ArgPos::Path { start: 3 });
    assert_eq!(at_end("-i "), ArgPos::Path { start: 3 });
    assert_eq!(at_end("-icred"), ArgPos::Path { start: 2 });
    assert_eq!(at_end("-vi cr"), ArgPos::Path { start: 4 });
    assert_eq!(at_end("-F c"), ArgPos::Path { start: 3 });
    assert_eq!(at_end("-o IdentityFile=cr"), ArgPos::Path { start: 16 });
    assert_eq!(at_end("-oIdentityFile=cr"), ArgPos::Path { start: 15 });
    assert_eq!(at_end("-o Port=2"), ArgPos::Other);
    assert_eq!(at_end("-p 2"), ArgPos::Other);
    assert_eq!(at_end(""), ArgPos::Destination { start: 0 });
    assert_eq!(at_end("ro"), ArgPos::Destination { start: 0 });
    assert_eq!(at_end("-i k ro"), ArgPos::Destination { start: 5 });
    assert_eq!(at_end("-i k "), ArgPos::Destination { start: 5 });
    assert_eq!(at_end("-i 'a b' r"), ArgPos::Destination { start: 9 });
    assert_eq!(at_end("-i \"a b"), ArgPos::Path { start: 3 });
    assert_eq!(at_end("h -i k"), ArgPos::Path { start: 5 });
    assert_eq!(at_end("h ls"), ArgPos::Other);
    assert_eq!(at_end("h ls "), ArgPos::Other);
    assert_eq!(at_end("-v"), ArgPos::Other);
    assert_eq!(at_end("-- r"), ArgPos::Destination { start: 3 });
    // The cursor in the middle reads only what is before it.
    assert_eq!(classify("-i k h", 4), ArgPos::Path { start: 3 });
    assert_eq!(classify("-i k h", 99), ArgPos::Destination { start: 5 });
}
