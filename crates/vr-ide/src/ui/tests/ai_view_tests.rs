//! The AI panel's screens (ui-spec S1, S2; mockups ai-panel-A-120x40 / -80x24, ai-panel-40x12,
//! ai-picker-120x40, ai-empty-80x24, ai-exited-empty-80x24) on fake sessions.

use std::path::{Path, PathBuf};

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use vr_config::AiTool;
use vr_term::{Cell, Cursor, CursorShape, Grid, TermStatus};

use super::*;
use crate::ai::AiSession;
use crate::ai::picker::Picker;
use crate::ai::spawn::StartMode;
use crate::app::Focus;
use crate::app_tests::test_app;
use crate::commands::terminal::TermTab;

const ROOT: &str = "/home/user/DEV/vr-ide";

fn grid(lines: &[&str], cols: usize, rows: usize) -> Grid {
    let mut cells = vec![
        Cell {
            ch: " ".into(),
            ..Cell::default()
        };
        cols * rows
    ];
    for (r, line) in lines.iter().enumerate().take(rows) {
        for (c, ch) in line.chars().enumerate().take(cols) {
            cells[r * cols + c].ch = ch.to_string();
        }
    }
    Grid {
        cols,
        rows,
        cells,
        cursor: Cursor {
            row: 0,
            col: 0,
            shape: CursorShape::Block,
            visible: false,
        },
        ..Grid::default()
    }
}

const CLAUDE: &[&str] = &[
    "> restore has to skip tabs whose",
    "  file is gone, with one notice",
    "",
    "● Read(src/session.rs)",
    "  └ 64 lines",
    "● Update(src/session.rs)",
    "  └ +12 -3",
];

fn session(tool: &str, account: Option<&str>, root: &str, status: TermStatus) -> AiSession {
    AiSession::new(
        TermTab::fake(grid(CLAUDE, 120, 40), tool, status),
        tool,
        account.map(str::to_string),
        PathBuf::from(root),
        StartMode::New,
    )
}

fn tools() -> Vec<AiTool> {
    let t = |name: &str, command: &str, resumes: bool| AiTool {
        name: name.into(),
        command: command.into(),
        args: vec![],
        continue_args: if resumes {
            vec!["--continue".into()]
        } else {
            vec![]
        },
        resume_args: if resumes {
            vec!["--resume".into()]
        } else {
            vec![]
        },
    };
    vec![
        t("claude", "/bin/sh", true),
        t("codex", "/bin/sh", true),
        t("gemini", "/bin/sh", false),
        t("aider", "/nonexistent/aider", false),
        t("cursor-agent", "/nonexistent/cursor-agent", false),
    ]
}

/// The mockup's three sessions: `1 claude`, `2 claude@work` (needs attention), `3 codex` in
/// another folder; the first one active and focused.
fn panel_app(n: usize) -> App {
    let (mut app, _) = test_app(Path::new(ROOT));
    app.settings.ai_tools = tools();
    let mut all = vec![
        session("claude", None, ROOT, TermStatus::Running),
        session("claude", Some("work"), ROOT, TermStatus::Running),
        session("codex", None, "/home/user/DEV/shop", TermStatus::Running),
        session("claude", None, ROOT, TermStatus::Running),
        session("claude", Some("work"), ROOT, TermStatus::Running),
        session("gemini", None, ROOT, TermStatus::Running),
    ];
    all[1].attention = Some(1);
    all.truncate(n);
    app.ai.sessions = all;
    app.ai.visible = true;
    app.focus = Focus::Ai;
    app
}

fn draw(app: &mut App, w: u16, h: u16) -> Terminal<TestBackend> {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| render(f, app)).unwrap();
    t
}

fn snap(name: &str, app: &mut App, sizes: &[(u16, u16)]) {
    for &(w, h) in sizes {
        let t = draw(app, w, h);
        insta::assert_snapshot!(format!("{name}_{w}x{h}"), t.backend());
    }
}

fn row(t: &Terminal<TestBackend>, y: u16) -> String {
    let buf = t.backend().buffer();
    (0..buf.area.width)
        .map(|x| buf[(x, y)].symbol().to_string())
        .collect()
}

#[test]
fn ai_panel_sessions() {
    let mut app = panel_app(3);
    snap("ai_panel", &mut app, &[(120, 40), (80, 24), (40, 12)]);
    app.explorer.visible = true;
    let t = draw(&mut app, 120, 40);
    assert_eq!(
        (app.layout.ai_sep.x, app.layout.ai.width),
        (84, 35),
        "mockup widths"
    );
    let bar = row(&t, 0);
    let panel: String = bar.chars().skip(85).collect();
    assert_eq!(
        panel, " 1 claude  2 @work ◆  3 codex    + ",
        "mockup ai-panel-A-120x40"
    );
    // The separator shows the focus; the mark is the attention colour.
    let theme = Theme::from_name(&app.settings.theme);
    let buf = t.backend().buffer();
    assert_eq!(buf[(84, 5)].symbol(), "│");
    assert_eq!(buf[(84, 5)].fg, theme.focus.fg.unwrap());
    let mark = panel.find('◆').unwrap();
    let mx = 85 + panel[..mark].chars().count() as u16;
    assert_eq!(buf[(mx, 0)].fg, theme.attention.fg.unwrap());
    // Status: the AI badge, the session and its folder, which of how many and the hide key.
    let status = row(&t, 39);
    assert!(
        status.starts_with(" AI  claude · /home/user/DEV/vr-ide"),
        "{status}"
    );
    assert!(status.trim_end().ends_with("1/3 · f12 hide"), "{status}");
    assert_eq!(buf[(1, 39)].bg, theme.mode_ai.bg.unwrap());
}

/// Six sessions at 80x24: the fit rule takes labels down to numbers, keeps the active tab whole
/// and the marked tab, and puts the rest into `+N`.
#[test]
fn ai_panel_six_sessions_fit() {
    let mut app = panel_app(6);
    app.ai.active = 3;
    snap("ai_panel_six", &mut app, &[(80, 24)]);
    let t = draw(&mut app, 80, 24);
    let bar = row(&t, 0);
    assert!(bar.contains(" 4 claude·2 "), "{bar}");
    assert!(bar.contains(" 2 ◆ "), "{bar}");
    let bar = crate::ui::ai_view::tab_bar(&app, 20);
    assert!(bar.tabs.iter().any(|t| t.index == 3), "active kept");
    assert!(bar.tabs.iter().any(|t| t.index == 1), "marked kept");
    assert!(bar.more.is_some());
}

#[test]
fn ai_panel_short_labels() {
    let mut app = panel_app(5);
    app.ai.sessions[1].attention = None;
    // claude active: other claude sessions show what tells them apart.
    assert_eq!(app.ai.short_label(1), "@work");
    assert_eq!(app.ai.short_label(3), "·2");
    assert_eq!(app.ai.short_label(4), "@work·2");
    assert_eq!(app.ai.short_label(2), "codex");
    assert_eq!(app.ai.label(4), "claude@work·2");
    assert_eq!(app.ai.folder(2).as_deref(), Some("shop"));
    assert_eq!(app.ai.folder(1), None);
    app.ai.sessions[3].name = Some("review".into());
    assert_eq!(app.ai.label(3), "review");
    assert_eq!(app.ai.short_label(3), "review");
}

#[test]
fn ai_picker() {
    let mut app = panel_app(3);
    let state = tempfile::tempdir().unwrap();
    crate::ai::accounts::create(state.path(), "claude", "work").unwrap();
    app.ai.state_dir = Some(state.path().to_path_buf());
    let picker = Picker::build(&app, PathBuf::from(ROOT), Some(("claude", None)), Some(0));
    app.ai.picker = Some(picker);
    app.ai.active = 3;
    snap("ai_picker", &mut app, &[(120, 40)]);
    let t = draw(&mut app, 120, 40);
    let text = format!("{:?}", t.backend());
    for want in [
        "New session in /home/user/DEV/vr-ide",
        "   default",
        "   work",
        "   + Add account…",
        "1 open",
        "gemini",
        "Not on PATH: aider, cursor-agent",
        "(ai_tools in settings)",
        "enter start  del remove  esc back",
        "new session · /home/user/DEV/vr-ide",
    ] {
        assert!(text.contains(want), "{want}:\n{text}");
    }
    // The second step (D14) for claude.
    app.handle_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    ));
    snap("ai_picker_second", &mut app, &[(120, 40), (40, 12)]);
    let t = draw(&mut app, 120, 40);
    let text = format!("{:?}", t.backend());
    for want in ["Continue last", "--continue", "Choose earlier", "--resume"] {
        assert!(text.contains(want), "{want}:\n{text}");
    }
}

/// S2 accounts (mockups ai-account-add-120x40, ai-account-remove-80x24,
/// ai-account-busy-80x24): the name field with its rule and its error, the remove question
/// (Cancel selected), the busy notice.
#[test]
fn ai_account_screens() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let press = |app: &mut App, code: KeyCode| {
        app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    };
    let mut app = panel_app(2);
    let state = tempfile::tempdir().unwrap();
    crate::ai::accounts::create(state.path(), "claude", "work").unwrap();
    crate::ai::accounts::create(state.path(), "claude", "old-team").unwrap();
    app.ai.state_dir = Some(state.path().to_path_buf());
    let mut picker = Picker::build(&app, PathBuf::from(ROOT), None, Some(0));
    picker.selected = picker
        .rows
        .iter()
        .position(|r| matches!(r, crate::ai::picker::Row::Add { tool } if tool == "claude"))
        .unwrap();
    app.ai.picker = Some(picker);
    app.ai.active = 2;
    press(&mut app, KeyCode::Enter);
    for c in "personal".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    snap("ai_account_add", &mut app, &[(120, 40)]);
    let text = format!("{:?}", draw(&mut app, 120, 40).backend());
    for want in [
        "   name personal▌",
        "   a-z 0-9 - _, for claude",
        "enter create + start  esc cancel",
        "new account · /home/user/DEV/vr-ide",
    ] {
        assert!(text.contains(want), "{want}:\n{text}");
    }
    // A refused name: the reason replaces the rule, the text stays.
    press(&mut app, KeyCode::Char('!'));
    press(&mut app, KeyCode::Enter);
    let text = format!("{:?}", draw(&mut app, 120, 40).backend());
    assert!(text.contains("   name personal!▌"), "{text}");
    assert!(text.contains("   use a-z 0-9 - _ only"), "{text}");
    press(&mut app, KeyCode::Esc);
    // Remove: the question, with a path that does not depend on the temp folder.
    app.ai.state_dir = Some(PathBuf::from("/home/user/.local/state/vr-ide"));
    let p = app.ai.picker.as_mut().unwrap();
    p.selected = p
        .rows
        .iter()
        .position(|r| matches!(r, crate::ai::picker::Row::Choice { account: Some(a), .. } if a == "old-team"))
        .unwrap();
    press(&mut app, KeyCode::Delete);
    assert!(app.dialog.is_some());
    snap("ai_account_remove", &mut app, &[(80, 24)]);
    let text = format!("{:?}", draw(&mut app, 80, 24).backend());
    for want in [
        "Remove account old-team?",
        "/home/user/.local/state/vr-ide/accounts/old-team/",
        "[Remove]",
    ] {
        assert!(text.contains(want), "{want}:\n{text}");
    }
    press(&mut app, KeyCode::Esc);
    // Busy: session 2 runs on work.
    let p = app.ai.picker.as_mut().unwrap();
    p.selected = p
        .rows
        .iter()
        .position(
            |r| matches!(r, crate::ai::picker::Row::Choice { account: Some(a), .. } if a == "work"),
        )
        .unwrap();
    press(&mut app, KeyCode::Delete);
    assert!(app.dialog.is_none());
    snap("ai_account_busy", &mut app, &[(80, 24)]);
    footer_has_whole_pairs(&draw(&mut app, 80, 24));
    let text = format!("{:?}", draw(&mut app, 80, 24).backend());
    assert!(
        text.contains("Not removed: close the session on work first, tab 2"),
        "{text}"
    );
    // Narrow: the pairs that do not fit go whole (no notice over the footer row).
    app.notices.clear();
    footer_has_whole_pairs(&draw(&mut app, 40, 12));
}

#[test]
fn ai_empty_state() {
    let (mut app, _) = test_app(Path::new(ROOT));
    app.settings.ai_tools = tools();
    let picker = Picker::build(&app, PathBuf::from(ROOT), None, None);
    app.ai.picker = Some(picker);
    app.ai.visible = true;
    app.focus = Focus::Ai;
    snap("ai_empty", &mut app, &[(80, 24)]);
    let t = draw(&mut app, 80, 24);
    let text = format!("{:?}", t.backend());
    for want in [
        " AI ",
        "Start in vr-ide",
        "enter start  esc hide",
        "no session",
    ] {
        assert!(text.contains(want), "{want}:\n{text}");
    }
    // No tool installed at all.
    for t in &mut app.settings.ai_tools {
        t.command = "/nonexistent/x".into();
    }
    app.ai.picker = Some(Picker::build(&app, PathBuf::from(ROOT), None, None));
    let t = draw(&mut app, 120, 40);
    let text = format!("{:?}", t.backend());
    assert!(text.contains("No AI tool found on PATH"), "{text}");
}

#[test]
fn ai_exited_session() {
    let mut app = panel_app(2);
    app.ai.sessions[0] = session("claude", None, ROOT, TermStatus::Exited(0));
    snap("ai_exited", &mut app, &[(80, 24)]);
    let t = draw(&mut app, 120, 40);
    let text = format!("{:?}", t.backend());
    assert!(
        text.contains("[exited 0] enter: start again") && text.contains("--continue)"),
        "{text}"
    );
    assert!(text.contains("claude [exited 0]"), "{text}");
    assert!(text.contains("1/2 · enter restart"), "{text}");
}

/// The new tokens exist in every theme and differ from the other badges.
#[test]
fn ai_tokens_in_every_theme() {
    for name in crate::commands::slash::THEMES {
        let t = Theme::from_name(name);
        assert_ne!(t.mode_ai, t.mode_term, "{name}");
        assert_ne!(t.mode_ai, t.mode_read, "{name}");
        assert_ne!(t.mode_ai, t.mode_edit, "{name}");
        assert!(t.attention.fg.is_some() && t.note.fg.is_some(), "{name}");
        assert_ne!(t.diff_added_bg, t.diff_removed_bg, "{name}");
    }
}

/// The picker's footer keeps whole key pairs: one that does not fit is left out, never cut.
fn footer_has_whole_pairs(t: &Terminal<TestBackend>) {
    let height = t.backend().buffer().area.height;
    let line = (0..height)
        .map(|y| row(t, y))
        .find(|l| l.contains("enter start"))
        .unwrap_or_else(|| panic!("footer row: {:?}", t.backend()));
    let footer = &line[line.find("enter start").unwrap()..];
    let footer = footer.trim_end();
    assert!(
        [
            "enter start",
            "enter start  del remove",
            "enter start  del remove  esc back"
        ]
        .contains(&footer),
        "cut footer: {footer:?}"
    );
}

/// S4 with the panel hidden: the notice row and the status bar's badge (mockups
/// ai-hidden-notify-120x40 / 80x24).
#[test]
fn ai_hidden_notify() {
    let mut app = panel_app(3);
    app.ai.visible = false;
    app.focus = Focus::Editor;
    for (w, h) in [(120u16, 40u16), (80, 24)] {
        app.notices.clear();
        app.layout.status.width = w;
        app.ai.sessions[1].notify = Default::default();
        app.ai.sessions[1].attention = None;
        let msg = Some("Allow Bash(cargo test -p vr-ide)?");
        crate::ai::attention::mark(&mut app, 1, msg, std::time::Instant::now());
        let t = draw(&mut app, w, h);
        insta::assert_snapshot!(format!("ai_hidden_notify_{w}x{h}"), t.backend());
        let status = row(&t, h - 1);
        assert!(
            status.trim_end().ends_with("◆ claude@work  f12"),
            "{status}"
        );
        let notice = row(&t, h - 2);
        assert!(notice.starts_with("◆ claude@work"), "{notice}");
        assert!(
            notice.contains("Allow Bash(cargo test -p vr-ide)?"),
            "{notice}"
        );
        let theme = Theme::from_name(&app.settings.theme);
        let at = status.find('◆').unwrap();
        let x = status[..at].chars().count() as u16;
        assert_eq!(
            t.backend().buffer()[(x, h - 1)].bg,
            theme.attention.fg.unwrap()
        );
    }
    // Viewing clears the badge and the notice.
    app.ai.visible = true;
    app.ai.active = 1;
    let t = draw(&mut app, 120, 40);
    assert_eq!(app.ai.sessions[1].attention, None);
    assert!(app.notices.is_empty());
    assert!(!row(&t, 39).contains("◆"), "{}", row(&t, 39));
}

/// The status text reflows into what the badge leaves: no token is cut by the badge at 80
/// columns with a long path.
#[test]
fn badge_reflows_the_status_text() {
    let dir = tempfile::tempdir().unwrap();
    let deep = dir
        .path()
        .join("a_rather_long_directory/another_long_directory/third_directory");
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(deep.join("a_long_file_name_for_the_status_bar.rs"), "x\n").unwrap();
    let (mut app, _) = test_app(dir.path());
    app.settings.ai_tools = tools();
    app.run_command(
        "file.open",
        crate::commands::CmdArgs::Path(
            "a_rather_long_directory/another_long_directory/third_directory/a_long_file_name_for_the_status_bar.rs".into(),
        ),
    );
    app.ai.sessions = vec![session("claude", Some("work"), ROOT, TermStatus::Running)];
    app.ai.visible = false;
    app.layout.status.width = 80;
    crate::ai::attention::mark(&mut app, 0, None, std::time::Instant::now());
    let t = draw(&mut app, 80, 24);
    let status = row(&t, 23);
    assert!(
        status.trim_end().ends_with("◆ claude@work  f12"),
        "{status}"
    );
    let before: String = status.chars().take_while(|c| *c != '◆').collect();
    let before = before.trim_end();
    // Whole position text, and the file name is kept (the directory goes first).
    assert!(
        before.contains("Ln 1, Col 1") || before.contains("Ln 1:1"),
        "{status}"
    );
    assert!(before.contains("status_bar.rs"), "{status}");
    assert!(
        !before.ends_with("Co") && !before.ends_with("Ln"),
        "{status}"
    );
}

/// The picker title is width-aware: the folder is left-shortened with an ellipsis so its
/// name (the tail) stays; it is never cut at the right edge.
#[test]
fn ai_picker_title_keeps_the_folder_name() {
    let mut app = panel_app(3);
    let picker = Picker::build(&app, PathBuf::from(ROOT), Some(("claude", None)), Some(0));
    app.ai.picker = Some(picker);
    app.ai.active = 3;
    let title = |w: u16, h: u16, app: &mut App| {
        let t = draw(app, w, h);
        let text = (0..h)
            .map(|y| row(&t, y))
            .find(|l| l.contains("New session in"))
            .expect("title row");
        text[text.find("New session in").unwrap()..]
            .trim_end()
            .to_string()
    };
    assert_eq!(
        title(120, 40, &mut app),
        "New session in /home/user/DEV/vr-ide"
    );
    assert_eq!(
        title(40, 12, &mut app),
        "New session in /home/user/DEV/vr-ide"
    );
    let narrow = title(80, 24, &mut app);
    assert!(narrow.ends_with("DEV/vr-ide"), "{narrow:?}");
    assert!(narrow.starts_with("New session in …"), "{narrow:?}");
}

// ---- usage in the tab label (AC-4) ------------------------------------------------------------

fn usage(five: f64, weekly: Option<f64>) -> Option<crate::ai::usage::Usage> {
    let w = |p: f64| crate::ai::usage::Window {
        used_percentage: p,
        resets_at: None,
    };
    Some(crate::ai::usage::Usage {
        five_hour: Some(w(five)),
        seven_day: weekly.map(w),
        ..Default::default()
    })
}

/// `1 claude 63%`, `2 claude@work 95%`: the second one active.
fn usage_app() -> App {
    let mut app = panel_app(2);
    app.ai.sessions[1].attention = None;
    app.ai.sessions[0].usage = usage(63.0, Some(20.0));
    app.ai.sessions[1].usage = usage(95.0, None);
    app.ai.active = 1;
    app
}

/// Every drawn tab ends inside the bar, and the usage suffixes hide no tab and no `+` the plain
/// labels keep.
fn bar_fits(app: &mut App, width: u16) {
    let bar = crate::ui::ai_view::tab_bar(app, width);
    let saved: Vec<_> = app.ai.sessions.iter_mut().map(|s| s.usage.take()).collect();
    let plain = crate::ui::ai_view::tab_bar(app, width);
    for (s, u) in app.ai.sessions.iter_mut().zip(saved) {
        s.usage = u;
    }
    let labels =
        |b: &crate::ui::ai_view::TabBar| b.tabs.iter().map(|t| t.label.clone()).collect::<Vec<_>>();
    let end = bar.tabs.iter().map(|t| t.x + t.width).max().unwrap();
    assert!(end <= width, "{width}: tabs end at {end}");
    assert!(
        bar.tabs.iter().any(|t| t.index == app.ai.active),
        "{width}: active kept"
    );
    // The percents hide nothing; only a `!` mark (one column per tab at 90 % or more, D39)
    // may cost a tab or the `+`.
    let marks = labels(&bar).iter().filter(|l| l.contains('!')).count();
    assert!(
        bar.tabs.len() + marks >= plain.tabs.len(),
        "{width}: {:?} vs {:?}",
        labels(&bar),
        labels(&plain)
    );
    if marks == 0 {
        assert_eq!(
            bar.plus.is_some(),
            plain.plus.is_some(),
            "{width}: {:?}",
            labels(&bar)
        );
    }
    assert!(
        bar.plus.is_none_or(|p| p >= end),
        "{width}: + over the tabs"
    );
}

#[test]
fn label_shows_usage_suffix() {
    let mut app = usage_app();
    app.ai.sessions[1].usage = usage(63.0, Some(41.0));
    let bar = crate::ui::ai_view::tab_bar(&app, 120);
    let tab = |i: usize| {
        bar.tabs
            .iter()
            .find(|t| t.index == i)
            .unwrap()
            .label
            .clone()
    };
    assert_eq!(tab(1), " 2 claude@work 63% x ");
    assert_eq!(tab(0), " 1 claude 63% ");
    // The label itself (notices, the status bar, diff tab names) keeps its plain form.
    assert_eq!(app.ai.label(1), "claude@work");
    app.ai.sessions[0].usage = None;
    let bar = crate::ui::ai_view::tab_bar(&app, 120);
    assert_eq!(bar.tabs[0].label, " 1 claude ", "no report: no suffix");
    for w in [120, 80, 40, 30, 25, 20, 16] {
        bar_fits(&mut app, w);
    }
}

/// The weekly window when it is the higher one, in the warning colour of every theme (only the
/// percent; the rest of the tab keeps its own style).
#[test]
fn label_weekly_when_higher() {
    let mut app = usage_app();
    app.ai.sessions[1].usage = usage(40.0, Some(91.0));
    for name in crate::commands::slash::THEMES {
        app.settings.theme = (*name).to_string();
        let theme = Theme::from_name(name);
        let warning = theme.warning.fg.expect("warning colour");
        let t = draw(&mut app, 120, 40);
        let bar = row(&t, 0);
        let at = bar.find("91%w").unwrap_or_else(|| panic!("{name}: {bar}"));
        let x0 = bar[..at].chars().count() as u16;
        let buf = t.backend().buffer();
        for x in x0..x0 + 4 {
            assert_eq!(buf[(x, 0)].fg, warning, "{name}: percent cell {x}");
            assert_eq!(
                buf[(x, 0)].bg,
                theme.tab_active.bg.unwrap_or(buf[(x, 0)].bg)
            );
        }
        assert_ne!(
            Some(warning),
            theme.tab_active.bg,
            "{name}: readable on the tab"
        );
        let label = bar.find("claude@work").unwrap();
        let lx = bar[..label].chars().count() as u16;
        assert_ne!(
            buf[(lx, 0)].fg,
            warning,
            "{name}: the name keeps its colour"
        );
        // 63 % on the other tab: no warning.
        let other = bar.find("63%").unwrap();
        let ox = bar[..other].chars().count() as u16;
        assert_ne!(buf[(ox, 0)].fg, warning, "{name}: 63% is not a warning");
    }
}

/// The profile sizes with a 63 % and a 95 % session; at 40 columns the inactive tab shows its
/// number only, in the warning colour when its account is at 90 % or more.
#[test]
fn ai_usage_label_sizes() {
    let mut app = usage_app();
    snap("ai_usage", &mut app, &[(120, 40), (80, 24), (40, 12)]);
    for w in [120, 80, 40] {
        bar_fits(&mut app, w);
    }
    // The 95 % tab inactive at a narrow width: its number gets a `!` in the warning colour
    // (D39: not colour alone).
    app.ai.active = 0;
    let theme = Theme::from_name(&app.settings.theme);
    app.ai
        .sessions
        .push(session("codex", None, ROOT, TermStatus::Running));
    app.ai
        .sessions
        .push(session("claude", None, ROOT, TermStatus::Running));
    let term = draw(&mut app, 80, 24);
    let area = app.layout.ai;
    let bar = crate::ui::ai_view::tab_bar(&app, area.width);
    let t = bar.tabs.iter().find(|t| t.index == 1).expect("kept");
    assert_eq!(t.label, " 2! ", "{}", row(&term, area.y));
    let buf = term.backend().buffer();
    let x = area.x + t.x + 2;
    assert_eq!(buf[(x, area.y)].symbol(), "!");
    assert_eq!(buf[(x, area.y)].fg, theme.warning.fg.unwrap());
    let calm = bar.tabs.iter().find(|t| t.index == 2).expect("kept");
    assert_eq!(calm.label, " 3 ", "no report: no mark");
    // 63 %: a number-only tab without the mark.
    app.ai.active = 1;
    let bar = crate::ui::ai_view::tab_bar(&app, area.width);
    assert_eq!(bar.tabs.iter().find(|t| t.index == 0).unwrap().label, " 1 ");
}
