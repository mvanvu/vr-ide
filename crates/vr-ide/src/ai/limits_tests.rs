use std::collections::BTreeMap;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use super::*;
use crate::ai::usage::days_from_civil;

/// 2026-11-27 10:00 UTC.
fn now() -> i64 {
    days_from_civil(2026, 11, 27) * 86_400 + 10 * 3600
}

fn at(day: i64, h: i64, m: i64) -> i64 {
    days_from_civil(2026, 11, day) * 86_400 + h * 3600 + m * 60
}

/// A claude screen as 2.1.332 draws it: the transcript, the input box between two rules, the
/// footer with the statusLine below.
fn claude_screen(transcript: &[&str]) -> Vec<String> {
    let mut rows: Vec<String> = transcript.iter().map(|s| s.to_string()).collect();
    rows.extend(
        [
            "",
            "────────────────────────────────────────────────────────────────────────────────",
            "> ",
            "────────────────────────────────────────────────────────────────────────────────",
            "  ? for shortcuts                                       Opus 5.5 · 5h 100%",
        ]
        .map(String::from),
    );
    rows
}

fn utc() -> Zone {
    Zone::fixed("UTC", 0)
}

fn reset(resets_at: Option<i64>) -> Option<LimitHit> {
    Some(LimitHit {
        resets_at,
        sticky: false,
    })
}

fn hit(tool: &str, rows: &[String]) -> Option<LimitHit> {
    hit_cols(tool, rows, 80)
}

fn hit_cols(tool: &str, rows: &[String], cols: usize) -> Option<LimitHit> {
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    limit_hit_at(tool, &refs, cols, now(), &utc())
}

// ---- limit screens (SEC-26) -------------------------------------------------------------------

/// The lines claude 2.1.332 composes (`You've hit your ${name}${" · resets " + time}`, names from
/// its `WFe` map; times from its `JN` formatter).
#[test]
fn claude_limit_lines_from_the_binary() {
    let cases = [
        (
            "  ⎿  You've hit your 5-hour limit · resets 3pm (UTC)",
            Some(at(27, 15, 0)),
            false,
        ),
        // Shown in another zone than ours: the time is not believed (the report's is used).
        (
            "  ⎿  You've hit your 5-hour limit · resets 3pm (Europe/Berlin)",
            None,
            false,
        ),
        (
            "  ⎿  You've hit your session limit · resets 3:30pm",
            Some(at(27, 15, 30)),
            false,
        ),
        (
            "  ⎿  You've hit your weekly limit · resets Dec 3, 3pm (UTC)",
            Some(at(3 + 30, 15, 0)),
            false,
        ),
        (
            "  ⎿  You've hit your Opus limit · resets 3pm · progress saved",
            Some(at(27, 15, 0)),
            true,
        ),
        (
            "  ⎿  You've hit your usage limit · resets 15:00",
            Some(at(27, 15, 0)),
            false,
        ),
        (
            "  ⎿  You've hit your limit · resets 9am",
            Some(at(28, 9, 0)),
            false,
        ),
        ("  ⎿  You've hit your Sonnet limit", None, true),
        ("  ⎿  You've hit your Fable limit", None, true),
        (
            "Usage limit reached · limit resets 3pm",
            Some(at(27, 15, 0)),
            false,
        ),
        ("Usage limit reached", None, false),
    ];
    for (line, resets_at, sticky) in cases {
        let got = hit("claude", &claude_screen(&["> fix the tests", "", line]));
        assert_eq!(got, Some(LimitHit { resets_at, sticky }), "{line}");
    }
}

/// claude 2.1.332's spend and credit stops (`strings` of the binary; see the module comment).
#[test]
fn claude_spend_and_credit_lines_from_the_binary() {
    let cases = [
        (
            "  ⎿  You've hit your monthly spend limit · /usage-credits to raise it",
            None,
        ),
        (
            "  ⎿  You've hit your monthly spend limit. See Usage in Settings",
            None,
        ),
        ("  ⎿  You've hit your monthly spend limit.", None),
        ("  ⎿  You've hit your channel's monthly spend limit.", None),
        (
            "  ⎿  You're out of usage credits · /usage-credits to buy more",
            None,
        ),
        (
            "  ⎿  You're out of usage credits. See Usage in Settings",
            None,
        ),
        (
            "  ⎿  You're out of usage credits · resets 3pm",
            Some(at(27, 15, 0)),
        ),
    ];
    for (line, resets_at) in cases {
        let got = hit("claude", &claude_screen(&["> fix the tests", "", line]));
        assert_eq!(
            got,
            Some(LimitHit {
                resets_at,
                sticky: true
            }),
            "{line}"
        );
    }
    // Still whole lines only.
    for row in [
        "● You're out of usage credits · /usage-credits to buy more",
        "  ⎿  echo You've hit your monthly spend limit.",
        "  ⎿  You're out of usage credits, said the log",
    ] {
        assert_eq!(hit("claude", &claude_screen(&[row])), None, "{row}");
    }
}

/// A reset part that is not ASCII is not a time, and never a panic.
#[test]
fn non_ascii_reset_text_is_not_a_time() {
    for line in [
        "  ⎿  You've hit your limit · resets naïve",
        "  ⎿  You've hit your limit · resets aaé",
        "  ⎿  You've hit your 5-hour limit · resets Déc 3, 3pm",
        "  ⎿  You've hit your limit · resets 3pm (Europe/Zürich)",
    ] {
        // claude's reset part must be a time, else the line is not its message.
        let _ = hit("claude", &claude_screen(&[line]));
    }
    assert_eq!(
        hit(
            "claude",
            &claude_screen(&["  ⎿  You've hit your limit · resets naïve"])
        ),
        None
    );
    for line in [
        "■ You've hit your usage limit. Try again at aaé",
        "■ You've hit your usage limit. Try again at naïve 3:00 PM.",
        "■ You've hit your usage limit. Try again at Dé 3rd, 2026 3:00 PM.",
        "■ You've hit your usage limit. Try again at 3:00\u{202f}ＰＭ.",
    ] {
        let rows = [line, "", "› "].map(String::from).to_vec();
        assert_eq!(hit("codex", &rows), reset(None), "{line}");
    }
    for s in [
        "naïve",
        "aaé",
        "é",
        "ñññ 3, 3pm",
        "Déc 3, 3pm",
        "3pm (Zürich)",
        "😀",
    ] {
        assert_eq!(parse_when(s, now(), &utc()), None, "{s}");
    }
}

/// Small deterministic generator (xorshift) so the run is the same every time.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 16) as usize
    }

    fn pick<'a>(&mut self, from: &[&'a str]) -> &'a str {
        from[self.next() % from.len()]
    }
}

/// No screen text can panic the readers: combining marks, wide characters, emoji, zero-width
/// and control characters, replacement characters, mixed with the tools' own words.
#[test]
fn arbitrary_unicode_screens_never_panic() {
    let parts = [
        "You've hit your ",
        "You're out of usage credits",
        "limit",
        "monthly spend limit",
        "5-hour limit",
        " · resets ",
        " · ",
        "Usage limit reached",
        "You've hit your usage limit.",
        " Try again at ",
        "Select login method:",
        "Sign in with ChatGPT to use Codex as part of your paid plan",
        "naïve",
        "aaé",
        "é",
        "e\u{301}",
        "\u{301}",
        "\u{301}\u{302}\u{303}",
        "漢字",
        "ｗｉｄｅ",
        "😀",
        "👨‍👩‍👧",
        "\u{200b}",
        "\u{200d}",
        "\u{202f}",
        "\u{a0}",
        "\u{202e}",
        "\u{fffd}",
        "\u{fffd}\u{fffd}",
        "\u{10ffff}",
        "\u{0}",
        "\u{1b}[31m",
        "\t",
        "Dec",
        "Déc",
        "3pm",
        "3:00 PM",
        "12:",
        ":",
        "3rd,",
        "2026",
        "(",
        ")",
        " (Europe/Berlin)",
        "١٢",
        "ß",
        "İ",
        "ǅ",
        " ",
        "   ",
        "⎿",
        "■",
        "│",
        "─────",
        "╭",
        "╯",
        "›",
        ">",
        "❯",
        "▎",
        ".",
    ];
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let zone = Zone::fixed("UTC", 0);
    for _ in 0..4000 {
        let rows: Vec<String> = (0..rng.next() % 20)
            .map(|_| (0..rng.next() % 9).map(|_| rng.pick(&parts)).collect())
            .collect();
        let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
        let cols = [1, 2, 3, 10, 40, 80, usize::MAX][rng.next() % 7];
        let now = now() + (rng.next() % 200_000) as i64;
        for tool in ["claude", "codex", "gemini"] {
            let _ = limit_hit_at(tool, &refs, cols, now, &zone);
        }
        for kind in [Tool::Claude, Tool::Codex] {
            let _ = login_in(kind, &Screen::new(&refs, cols), "");
            let _ = login_in(kind, &Screen::new(&refs, cols), &rows.concat());
        }
        let _ = parse_when(&rows.concat(), now, &zone);
        let mut w = Watch::default();
        w.typed_input(rows.concat().as_bytes());
    }
}

#[test]
fn codex_limit_lines_from_the_binary() {
    // codex 0.144.1 wraps its error cell (`■ `) itself; continuation rows are indented.
    let rows: Vec<String> = [
        "› fix the tests",
        "",
        "■ You've hit your usage limit. Upgrade to Pro (https://chatgpt.com/explore/pro),",
        "  visit https://chatgpt.com/codex/settings/usage to purchase more credits or try",
        "  again at 3:00 PM.",
        "",
        "› Ask Codex to do anything",
        "",
        "  100% context left · ? for shortcuts",
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(hit("codex", &rows), reset(Some(at(27, 15, 0))));
    let with_date = [
        "■ You've hit your usage limit. Try again at Dec 3rd, 2026 3:00 PM.",
        "",
        "› Ask Codex to do anything",
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(hit("codex", &with_date), reset(Some(at(33, 15, 0))));
    let far = [
        "■ You've hit your usage limit. Try again at 2099-01-01.",
        "› ",
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(hit("codex", &far), reset(None));
    // A limit for one model is not one the usage report shows.
    let model = [
        "■ You've hit your usage limit for GPT-5-Codex. Try again at 3:00 PM.",
        "› ",
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(
        hit("codex", &model),
        Some(LimitHit {
            resets_at: Some(at(27, 15, 0)),
            sticky: true
        })
    );
}

/// A narrow panel wraps claude's line; the rows are joined only where the break was a wrap.
#[test]
fn a_wrapped_claude_line_is_joined() {
    let rows = claude_screen(&[
        "  ⎿  You've hit your 5-hour limit ·",
        "     resets 3pm (UTC)",
    ]);
    assert_eq!(hit_cols("claude", &rows, 40), reset(Some(at(27, 15, 0))));
}

#[test]
fn ignores_old_lines() {
    let mut transcript = vec!["  ⎿  You've hit your 5-hour limit · resets 3pm"];
    // Indented under it (its own block), so only the 12-line window rules it out.
    transcript.extend(["       line"; 12]);
    assert_eq!(hit("claude", &claude_screen(&transcript)), None);
}

/// SEC-26: anything after the limit line that is not its own block means the tool went on.
#[test]
fn more_output_after_the_line_is_no_hit() {
    let rows = claude_screen(&[
        "  ⎿  You've hit your 5-hour limit · resets 3pm",
        "",
        "● Done. The tests pass.",
    ]);
    assert_eq!(hit("claude", &rows), None);
}

#[test]
fn the_input_box_is_ignored() {
    let typed = [
        "● Ready.",
        "",
        "────────────────────────────────────────",
        "> You've hit your 5-hour limit · resets 3pm",
        "────────────────────────────────────────",
        "  ? for shortcuts",
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(hit("claude", &typed), None);
    let rounded = [
        "● Ready.",
        "╭──────────────────────────────────────────────╮",
        "│ > You've hit your 5-hour limit · resets 3pm  │",
        "│   You've hit your 5-hour limit · resets 3pm  │",
        "╰──────────────────────────────────────────────╯",
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(hit("claude", &rounded), None);
    let codex = [
        "• Ready.",
        "",
        "› You've hit your usage limit. Try again at 3:00 PM.",
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(hit("codex", &codex), None);
}

/// Text the agent shows is matched by the same whole-line rule: only a line that is exactly the
/// tool's message, after the tool's own markers, counts.
#[test]
fn spoofed_lines_in_agent_output() {
    let line = "You've hit your 5-hour limit · resets 3pm";
    let none = [
        // The agent's own reply starts with its bullet.
        format!("● {line}"),
        // Quoted (claude draws a bar, markdown keeps `>`).
        format!("  ▎ {line}"),
        format!("  > {line}"),
        // Inside a longer line: a shell command, a log line, a sentence.
        format!("  ⎿  $ echo \"{line}\""),
        format!("  ⎿  12:00:01 ERROR {line}"),
        format!("  The tool said: {line}."),
        // Not one of the limits that stop the tool.
        "  ⎿  Context limit reached · /compact or /clear to continue".to_string(),
        "  ⎿  You've hit your fast limit · resets in 5m".to_string(),
        "Usage limit reached · wrapping up".to_string(),
        format!("  ⎿  {line} because the test said so"),
    ];
    for row in none {
        assert_eq!(hit("claude", &claude_screen(&[&row])), None, "{row}");
    }
    // Accepted risk (threat model): a code block holding exactly the message, as the last thing
    // on screen, matches; SEC-26's quiet rule and T6's restart cap bound what it can do.
    let block = claude_screen(&["● Here is the message:", "", &format!("  {line}")]);
    assert!(hit("claude", &block).is_some());
    // Tools without verified patterns never match.
    assert_eq!(
        hit("gemini", &claude_screen(&[&format!("  ⎿  {line}")])),
        None
    );
}

#[test]
fn reset_time_parsing() {
    let p = |s: &str| parse_when(s, now(), &utc());
    assert_eq!(p("3pm"), Some(at(27, 15, 0)));
    assert_eq!(p("at 3pm"), Some(at(27, 15, 0)));
    assert_eq!(p("3:30pm (UTC)"), Some(at(27, 15, 30)));
    assert_eq!(p("3:30pm (Etc/UTC)"), Some(at(27, 15, 30)));
    assert_eq!(p("12am"), Some(at(28, 0, 0)));
    assert_eq!(p("12pm"), Some(at(27, 12, 0)));
    // Already past today: tomorrow.
    assert_eq!(p("9am"), Some(at(28, 9, 0)));
    assert_eq!(p("15:00"), Some(at(27, 15, 0)));
    assert_eq!(p("3:00 PM."), Some(at(27, 15, 0)));
    assert_eq!(p("3:00\u{202f}PM"), Some(at(27, 15, 0)));
    assert_eq!(p("Dec 3, 3pm"), Some(at(33, 15, 0)));
    assert_eq!(p("Dec 3rd, 2026 3:00 PM"), Some(at(33, 15, 0)));
    // Within [now, now + 8 days] only.
    assert_eq!(p("Dec 6, 3pm"), None);
    assert_eq!(p("Jan 2, 2027, 3pm"), None);
    assert_eq!(p("Nov 26, 3pm"), None);
    assert_eq!(p("2099-01-01"), None);
    for bad in [
        "",
        "soon",
        "25:00",
        "13pm",
        "3:61pm",
        "Foo 3, 3pm",
        "Dec 40, 3pm",
        "0am",
    ] {
        assert_eq!(p(bad), None, "{bad}");
    }
    // A local zone: 15:00 at UTC+7 is 08:00 UTC, tomorrow from 10:00 UTC.
    assert_eq!(
        parse_when("3pm", now(), &Zone::fixed("Asia/Bangkok", 7 * 3600)),
        Some(at(28, 8, 0))
    );
}

/// A time that passed moments ago is the limit that just ended, not tomorrow's.
#[test]
fn a_time_just_passed_is_not_tomorrow() {
    let p = |s: &str, now: i64| parse_when(s, now, &utc());
    // Read at 10:00:05.
    assert_eq!(p("10am", now() + 5), None);
    assert_eq!(p("at 10:00 AM.", now() + 5), None);
    assert_eq!(p("9:30am", now()), None);
    assert_eq!(p("9:01am", now()), None);
    assert_eq!(p("10am", now()), Some(now()));
    // An hour or more ago: the next one.
    assert_eq!(p("9am", now()), Some(at(28, 9, 0)));
    let rows = claude_screen(&["  ⎿  You've hit your 5-hour limit · resets 10am"]);
    let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
    assert_eq!(
        limit_hit_at("claude", &refs, 80, now() + 5, &utc()),
        reset(None)
    );
}

/// Berlin around the end of summer time 2026 (25 Oct, 01:00 UTC: +2 h → +1 h).
fn berlin() -> Zone {
    let end = days_from_civil(2026, 10, 25) * 86_400 + 3600;
    let start = days_from_civil(2026, 3, 29) * 86_400 + 3600;
    Zone::with_offsets(
        "Europe/Berlin",
        vec![(i64::MIN, 3600), (start, 7200), (end, 3600)],
    )
}

#[test]
fn reset_time_across_dst_and_zones() {
    // 02:30 summer time, half an hour before the change: 3pm is 15:00 winter time = 14:00 UTC.
    let now = days_from_civil(2026, 10, 25) * 86_400 + 30 * 60;
    let day = days_from_civil(2026, 10, 25) * 86_400;
    assert_eq!(parse_when("3pm", now, &berlin()), Some(day + 14 * 3600));
    assert_eq!(
        parse_when("3pm (Europe/Berlin)", now, &berlin()),
        Some(day + 14 * 3600)
    );
    // Before the change, read the day before: 23:00 summer time = 21:00 UTC.
    let eve = day - 86_400 + 12 * 3600;
    assert_eq!(parse_when("11pm", eve, &berlin()), Some(day - 3 * 3600));
    // A date beyond the change.
    assert_eq!(
        parse_when("Oct 26, 9am", now, &berlin()),
        Some(day + 86_400 + 8 * 3600)
    );
    // A zone on screen that is not ours: the time is not believed.
    assert_eq!(parse_when("3pm (Asia/Tokyo)", now, &berlin()), None);
    assert_eq!(parse_when("3pm (UTC)", now, &berlin()), None);
    // Our zone not known: no screen time at all.
    assert_eq!(parse_when("3pm", now, &Zone::unknown()), None);
    // UTC's names.
    for name in ["UTC", "Etc/UTC", "Etc/UCT", "Universal", "Zulu"] {
        assert_eq!(
            parse_when("3pm (UTC)", now, &Zone::fixed(name, 0)),
            Some(day + 15 * 3600),
            "{name}"
        );
    }
}

/// A TZif v1 file with one transition from `before` to `after` at `at`.
fn tzif(before: i32, after: i32, at: i32) -> Vec<u8> {
    let mut b = b"TZif".to_vec();
    b.extend([0u8; 16]);
    for n in [0u32, 0, 0, 1, 2, 4] {
        b.extend(n.to_be_bytes());
    }
    b.extend(at.to_be_bytes());
    b.push(1);
    for off in [before, after] {
        b.extend(off.to_be_bytes());
        b.extend([0, 0]);
    }
    b.extend(b"AAA\0");
    b
}

#[test]
fn local_zone_follows_tz_and_localtime() {
    let root = state_dir();
    let zi = root.path().join("usr/share/zoneinfo");
    std::fs::create_dir_all(zi.join("Test")).unwrap();
    std::fs::write(zi.join("Test/Plus2"), tzif(3600, 7200, 1000)).unwrap();
    std::fs::write(zi.join("Test/Plus5"), tzif(18_000, 18_000, 0)).unwrap();
    std::fs::create_dir(root.path().join("etc")).unwrap();
    // No /etc/localtime and no TZ: the C library's UTC.
    let z = load_zone(None, root.path());
    assert!(z.known && z.is("UTC") && z.offset(5000) == 0);
    std::os::unix::fs::symlink(zi.join("Test/Plus5"), root.path().join("etc/localtime")).unwrap();
    let z = load_zone(None, root.path());
    assert_eq!(z.name.as_deref(), Some("Test/Plus5"));
    assert_eq!(z.offset(5000), 18_000);
    // TZ wins over /etc/localtime, with or without the leading colon.
    for tz in ["Test/Plus2", ":Test/Plus2"] {
        let z = load_zone(Some(tz), root.path());
        assert!(z.known && z.is("Test/Plus2"), "{tz}");
        assert_eq!((z.offset(500), z.offset(5000)), (3600, 7200));
    }
    let z = load_zone(Some(""), root.path());
    assert!(z.known && z.is("UTC"));
    // A zone we cannot read is not guessed.
    for tz in [
        "Test/Missing",
        "../etc/passwd",
        "/usr/share/zoneinfo/Test/Plus2",
        "Test/../Test/Plus2",
        "CET-1CEST,M3.5.0,M10.5.0/3",
    ] {
        let z = load_zone(Some(tz), root.path());
        assert!(!z.known, "{tz}");
    }
}

#[test]
fn login_screen_detected() {
    let claude = [
        " Claude Code can be used with your Claude subscription or billed based on API usage",
        "",
        " Select login method:",
        "",
        " ❯ 1. Claude account with subscription · Individual, Team, or Enterprise",
        "   2. Anthropic Console account · API usage billing",
    ]
    .join("\n");
    assert!(login_screen("claude", &claude));
    let codex = [
        "  Welcome to Codex",
        "",
        "  Sign in with ChatGPT to use Codex as part of your paid plan",
        "  or connect an API key for usage-based billing",
        "",
        "› 1. Sign in with ChatGPT",
        "  2. Sign in with Device Code",
    ]
    .join("\n");
    assert!(login_screen("codex", &codex));
    // A codex answer, a typed prompt and another tool are not a login screen.
    assert!(!login_screen(
        "codex",
        "■ Sign in with ChatGPT to use /usage.\n› "
    ));
    let typed = "● hi\n────────\n> Select login method:\n────────\n";
    assert!(!login_screen("claude", typed));
    assert!(!login_screen(
        "claude",
        "● Select login method: is the heading"
    ));
    assert!(!login_screen("gemini", &claude));
}

/// The login line is held to the limit line's rules: among the last rows, the last content, not
/// something the user typed.
#[test]
fn login_line_is_as_strict_as_a_limit_line() {
    // The agent printed it, then went on.
    let answered = claude_screen(&[
        "● The onboarding screen shows:",
        "",
        " Select login method:",
        "",
        "● That is where you pick an account.",
    ]);
    let s = |rows: &[String], typed: &str| {
        let refs: Vec<&str> = rows.iter().map(String::as_str).collect();
        login_in(Tool::Claude, &Screen::new(&refs, 80), typed)
    };
    assert!(!s(&answered, ""));
    // Far above the last 12 rows.
    let mut old = vec![" Select login method:"];
    old.extend(["   line"; 12]);
    assert!(!s(&claude_screen(&old), ""));
    // A program echoing what the user typed.
    let echoed = [" Select login method:".to_string(), String::new()];
    assert!(s(&echoed, ""));
    assert!(!s(&echoed, "Select login method:\n"));
    // The real screen still counts.
    let claude = [
        " Claude Code can be used with your Claude subscription or billed based on API usage",
        "",
        " Select login method:",
        "",
        " ❯ 1. Claude account with subscription · Individual, Team, or Enterprise",
        "   2. Anthropic Console account · API usage billing",
        "   3. 3rd-party platform · Amazon Bedrock, Microsoft Foundry, or Vertex AI",
    ]
    .map(String::from)
    .to_vec();
    assert!(s(&claude, "x\n"));
}

/// What the user typed (keys and pastes as sent to the tool) is kept as text, so an echo of it
/// is never taken for the tool's message (QC F1).
#[test]
fn watch_debug_never_shows_typed_text() {
    let mut w = Watch::default();
    w.typed_input(b"sk-secret-key-1234\r");
    let shown = format!("{w:?} {w:#?}");
    assert!(!shown.contains("secret"), "{shown}");
    assert!(shown.contains("typed_text"), "{shown}");
}

/// A later, shorter limit never shortens a longer one already recorded (QP1).
#[test]
fn a_shorter_limit_keeps_the_longer_one() {
    let mut e = entry(true, None);
    record_limit(&mut e, 5 * 86_400, true);
    // A 5-hour limit while the Opus weekly one still holds.
    record_limit(&mut e, 3 * 3600, false);
    assert_eq!((e.limited_until, e.sticky), (Some(5 * 86_400), true));
    // A later reset wins with its own kind.
    record_limit(&mut e, 6 * 86_400, false);
    assert_eq!((e.limited_until, e.sticky), (Some(6 * 86_400), false));
    // The same reset: sticky when either is.
    record_limit(&mut e, 6 * 86_400, true);
    assert_eq!((e.limited_until, e.sticky), (Some(6 * 86_400), true));
}

/// FR-1 M2: the state lock is never waited for on the UI thread: while another program holds
/// it, an update gives up within 200 ms and writes nothing; once free it works again.
#[test]
fn a_held_state_lock_is_given_up_quickly() {
    use rustix::fs::{FlockOperation, Mode, OFlags};
    let d = state_dir();
    let flags = OFlags::RDWR | OFlags::CREATE | OFlags::CLOEXEC;
    let other =
        rustix::fs::open(d.path().join(STATE_LOCK), flags, Mode::RUSR | Mode::WUSR).unwrap();
    rustix::fs::flock(&other, FlockOperation::LockExclusive).unwrap();
    let t0 = std::time::Instant::now();
    let r = update_state(d.path(), |m| {
        m.insert("claude/default".into(), entry(true, None));
    });
    let took = t0.elapsed();
    assert!(r.is_err(), "{r:?}");
    assert!(took < std::time::Duration::from_millis(200), "{took:?}");
    assert!(load_state(d.path()).is_empty());
    drop(other);
    assert!(
        update_state(d.path(), |m| {
            m.insert("claude/default".into(), entry(true, None));
        })
        .unwrap()
    );
}

/// FR-1 M3: a guessed limit roundtrips, is never sticky, and a real reset replaces it whether
/// earlier or later; a guess never replaces a real one.
#[test]
fn a_guessed_limit_gives_way_to_a_real_one() {
    let d = state_dir();
    let soon = crate::ai::usage::now_secs() + 3600;
    let mut map = BTreeMap::new();
    let mut guess = entry(true, Some(soon));
    guess.guessed = true;
    map.insert("claude/work".to_string(), guess);
    save_state(d.path(), &map).unwrap();
    assert_eq!(load_state(d.path()), map);
    let mut e = guess;
    record_limit(&mut e, soon - 600, false);
    assert_eq!((e.limited_until, e.guessed), (Some(soon - 600), false));
    let mut e = entry(true, Some(soon));
    record_guess(&mut e, soon + 600);
    assert_eq!((e.limited_until, e.guessed), (Some(soon), false));
    let mut e = entry(true, None);
    record_guess(&mut e, soon);
    assert_eq!(
        (e.limited_until, e.guessed, e.sticky),
        (Some(soon), true, false)
    );
}

/// FR-1 M1: forgetting an account drops its entry for every tool, and only its own.
#[test]
fn forget_drops_only_that_accounts_entries() {
    let d = state_dir();
    let mut map = BTreeMap::new();
    map.insert("claude/work".to_string(), entry(true, None));
    map.insert("codex/work".to_string(), entry(true, None));
    map.insert("claude/workx".to_string(), entry(true, None));
    map.insert("claude/default".to_string(), entry(true, None));
    save_state(d.path(), &map).unwrap();
    forget(d.path(), "work").unwrap();
    let keys: Vec<String> = load_state(d.path()).into_keys().collect();
    assert_eq!(keys, ["claude/default", "claude/workx"]);
}

#[test]
fn typed_input_is_kept_as_text() {
    let mut w = Watch::default();
    w.typed_input(b"You've hit yourr\x7f limit \xc2\xb7 resets 3pm\r");
    // Arrows, a bracketed paste's markers, alt+key: not text.
    w.typed_input(b"\x1b[A\x1bOB\x1b[200~pasted\x1b[201~\x1bx");
    assert_eq!(w.typed_text, "You've hit your limit · resets 3pm\npasted");
    assert!(typed_has(
        &w.typed_text,
        "You've hit your limit · resets 3pm"
    ));
    assert!(typed_has(
        &w.typed_text,
        "You've hit your  limit ·\nresets 3pm"
    ));
    assert!(!typed_has(&w.typed_text, "You've hit your 5-hour limit"));
    assert!(!typed_has(&w.typed_text, ""));
    // Bounded: only the latest input is kept.
    w.typed_input("é".repeat(TYPED_MAX * 2).as_bytes());
    assert!(w.typed_text.chars().count() <= TYPED_MAX);
    assert!(!w.typed_text.contains("pasted"));
    // A new run forgets it.
    w.restarted();
    assert!(w.typed_text.is_empty());
}

// ---- account state (SEC-27) -------------------------------------------------------------------

fn state_dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn entry(works: bool, limited_until: Option<i64>) -> AccountState {
    AccountState {
        works,
        limited_until,
        sticky: false,
        guessed: false,
    }
}

#[test]
fn state_roundtrip_and_mode_0600() {
    let d = state_dir();
    let soon = crate::ai::usage::now_secs() + 3600;
    let mut map = BTreeMap::new();
    map.insert("claude/default".to_string(), entry(true, None));
    map.insert("claude/work".to_string(), entry(true, Some(soon)));
    map.insert("codex/personal".to_string(), entry(false, Some(soon)));
    map.insert(
        "codex/team".to_string(),
        AccountState {
            works: false,
            limited_until: Some(soon),
            sticky: true,
            guessed: false,
        },
    );
    save_state(d.path(), &map).unwrap();
    let file = d.path().join(STATE_FILE);
    assert_eq!(std::fs::metadata(&file).unwrap().mode() & 0o777, 0o600);
    assert_eq!(load_state(d.path()), map);
    // No temp file is left beside it.
    let names: Vec<String> = std::fs::read_dir(d.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        names.iter().all(|n| n == STATE_FILE || n == STATE_LOCK),
        "{names:?}"
    );
}

#[test]
fn state_drops_bad_keys_and_clamps() {
    let d = state_dir();
    let now = crate::ai::usage::now_secs();
    let text = serde_json::json!({
        "claude/work": {"works": true, "limited_until": now + 100 * 86_400},
        "claude/../x": {"works": true},
        "claude/Work": {"works": true},
        "gemini/default": {"works": true},
        "claude": {"works": true},
        "codex/default": {"works": "yes", "limited_until": now - 10},
        "claude/a/b": {"works": true},
    });
    std::fs::write(d.path().join(STATE_FILE), text.to_string()).unwrap();
    let map = load_state(d.path());
    let keys: Vec<&str> = map.keys().map(String::as_str).collect();
    assert_eq!(keys, ["claude/work"]);
    let until = map["claude/work"].limited_until.unwrap();
    assert!(until <= now + RESET_MAX + 5 && until >= now + RESET_MAX - 5);
    assert_eq!(state_key("claude", Some("../x")), None);
    assert_eq!(state_key("gemini", None), None);
    assert_eq!(state_key("codex", None).as_deref(), Some("codex/default"));
}

#[test]
fn state_ignores_oversize_or_garbage() {
    let d = state_dir();
    let file = d.path().join(STATE_FILE);
    for junk in [
        b"not json".to_vec(),
        b"[1,2]".to_vec(),
        vec![0xff; 10],
        format!(
            "{{\"claude/work\":{{\"works\":true}},\"pad\":\"{}\"}}",
            "a".repeat(70_000)
        )
        .into_bytes(),
    ] {
        std::fs::write(&file, &junk).unwrap();
        assert!(load_state(d.path()).is_empty());
        // Garbage was read under the lock: the next update replaces it.
        let changed = update_state(d.path(), |m| {
            m.insert("claude/work".into(), entry(true, None));
        })
        .unwrap();
        assert!(changed);
        assert_eq!(load_state(d.path()).len(), 1);
    }
    // A link or a FIFO in its place is not read (no hang) and not written through.
    std::fs::remove_file(&file).unwrap();
    let target = d.path().join("elsewhere");
    std::fs::write(&target, r#"{"claude/work":{"works":true}}"#).unwrap();
    std::os::unix::fs::symlink(&target, &file).unwrap();
    assert!(load_state(d.path()).is_empty());
    assert!(update_state(d.path(), |m| m.clear()).is_err());
    assert!(std::fs::symlink_metadata(&file).unwrap().is_symlink());
    std::fs::remove_file(&file).unwrap();
    rustix::fs::mknodat(
        rustix::fs::CWD,
        &file,
        rustix::fs::FileType::Fifo,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        0,
    )
    .unwrap();
    assert!(load_state(d.path()).is_empty());
    assert!(update_state(d.path(), |m| m.clear()).is_err());
}

/// Two writers (threads stand in for windows) updating different accounts keep both.
#[test]
fn state_two_writers_keep_both() {
    let d = state_dir();
    let base = crate::ai::usage::now_secs() + 60;
    let run = |key: &'static str, dir: &Path| {
        let dir = dir.to_path_buf();
        std::thread::spawn(move || {
            for n in 0..40 {
                // The lock is given up after LOCK_WAIT (the UI thread must not hang): a writer
                // that found it busy under load writes again, as the next report would. Each
                // write must still land, both keys with their last value.
                let mut tries = 0;
                loop {
                    match update_state(&dir, |m| {
                        m.insert(key.to_string(), entry(true, Some(base + n)));
                    }) {
                        Ok(_) => break,
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock && tries < 100 => {
                            tries += 1;
                        }
                        Err(e) => panic!("write {n} of {key}: {e}"),
                    }
                }
            }
        })
    };
    let a = run("claude/a", d.path());
    let b = run("claude/b", d.path());
    a.join().unwrap();
    b.join().unwrap();
    let map = load_state(d.path());
    assert_eq!(map.get("claude/a"), Some(&entry(true, Some(base + 39))));
    assert_eq!(map.get("claude/b"), Some(&entry(true, Some(base + 39))));
}

#[test]
fn state_file_in_a_private_folder_keeps_0600() {
    let d = state_dir();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    update_state(d.path(), |m| {
        m.insert("codex/default".into(), entry(true, None));
    })
    .unwrap();
    let lock = d.path().join(STATE_LOCK);
    assert_eq!(std::fs::metadata(&lock).unwrap().mode() & 0o777, 0o600);
    // Nothing to change: no write.
    let before = std::fs::metadata(d.path().join(STATE_FILE)).unwrap().ino();
    assert!(!update_state(d.path(), |_| {}).unwrap());
    assert_eq!(
        std::fs::metadata(d.path().join(STATE_FILE)).unwrap().ino(),
        before
    );
}
