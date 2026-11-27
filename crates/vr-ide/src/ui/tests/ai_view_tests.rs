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
    AiSession {
        tab: TermTab::fake(grid(CLAUDE, 120, 40), tool, status),
        tool: tool.into(),
        account: account.map(str::to_string),
        account_lock: None,
        root: PathBuf::from(root),
        name: None,
        mode: StartMode::New,
        attention: None,
    }
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
