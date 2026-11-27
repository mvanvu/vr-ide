//! The AI changes screens (ui-spec S3; mockups ai-changes-A-120x40 / -80x24,
//! ai-changes-limits-120x40, ai-revert-confirm-120x40, ai-revert-created-80x24,
//! ai-revert-refused-80x24, ai-exited-empty-80x24).

use std::path::Path;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use vr_core::FsEvent;
use vr_term::{Grid, TermStatus};

use super::*;
use crate::ai::changes::NoEarlier;
use crate::ai::review;
use crate::ai_changes_tests::{Fx, OLD_MD, SESSION_RS, TESTS_RS, changed, fix_times, fixture};
use crate::app::Focus;
use crate::app_tests::press;
use crate::commands::terminal::TermTab;

fn draw(app: &mut App, w: u16, h: u16) -> Terminal<TestBackend> {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| render(f, app)).unwrap();
    t
}

fn row(t: &Terminal<TestBackend>, y: u16) -> String {
    let buf = t.backend().buffer();
    (0..buf.area.width)
        .map(|x| buf[(x, y)].symbol().to_string())
        .collect()
}

fn key(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

/// The list open with the keys and the first file's diff in the editor.
fn reviewing() -> Fx {
    let mut fx = changed();
    press(&mut fx.app, "f6");
    key(&mut fx.app, KeyCode::Enter);
    fx
}

#[test]
fn ai_changes_list_and_diff() {
    let mut fx = reviewing();
    fx.app.explorer.visible = false;
    let t = draw(&mut fx.app, 80, 24);
    insta::assert_snapshot!("ai_changes_80x24", t.backend());
    fx.app.explorer.visible = true;
    let t = draw(&mut fx.app, 120, 40);
    insta::assert_snapshot!("ai_changes_120x40", t.backend());
    let panel = |y: u16| row(&t, y).chars().skip(85).collect::<String>();
    // The mockup's rows: header, files with folder and counts, the D8 statement, the rule.
    assert_eq!(panel(1), " Changes · claude · 3 files     f6 ");
    assert_eq!(panel(2), " M session.rs …es/vr-ide/src +9 -3 ");
    assert_eq!(panel(3), " A session_tests.rs …r-ide/src +88 ");
    assert_eq!(panel(4), " D old-session.md docs         -40 ");
    assert_eq!(panel(5), " Counts edits by any program in    ");
    assert_eq!(panel(6), " vr-ide while claude runs          ");
    assert_eq!(panel(7), "─".repeat(35));
    let editor: String = row(&t, 0).chars().skip(30).take(54).collect();
    assert!(editor.starts_with(" ± session.rs x"), "{editor}");
    let head: String = row(&t, 1).chars().skip(30).take(54).collect();
    assert!(head.starts_with(" M session.rs  +9 -3"), "{head}");
    assert!(head.ends_with("f5 keep  shift+f5 revert"), "{head}");
    let status = row(&t, 39);
    assert!(
        status.starts_with(" DIFF  crates/vr-ide/src/session.rs · claude · since 14:32"),
        "{status}"
    );
    assert!(
        status
            .trim_end()
            .ends_with("1/3 · f5 keep · shift+f5 revert"),
        "{status}"
    );
    // Signs, never colour alone; the row backgrounds and the letters use the theme's tokens.
    let theme = Theme::from_name(&fx.app.settings.theme);
    let buf = t.backend().buffer();
    let line = (0..40)
        .find(|&y| row(&t, y).contains("pub ai: Vec<AgentState>"))
        .unwrap();
    assert!(
        row(&t, line).contains(" 12 +    pub ai"),
        "{}",
        row(&t, line)
    );
    assert_eq!(buf[(40, line)].bg, theme.diff_added_bg);
    let removed = (0..40)
        .find(|&y| row(&t, y).contains("-    /// Drops tabs whose file is gone."))
        .unwrap();
    assert_eq!(buf[(40, removed)].bg, theme.diff_removed_bg);
    assert_eq!(buf[(86, 2)].fg, theme.git_modified.fg.unwrap());
    assert_eq!(buf[(86, 3)].fg, theme.git_added.fg.unwrap());
    assert_eq!(buf[(86, 4)].fg, theme.git_deleted.fg.unwrap());
    assert_eq!(buf[(1, 39)].bg, theme.mode_ai.bg.unwrap());
    assert_eq!(buf[(86, 2)].bg, theme.list_selected.bg.unwrap());
}

/// The count in the tab bar and the status bar while the list is closed (S1).
#[test]
fn ai_changes_count_in_the_tab_bar_and_status() {
    let mut fx = changed();
    fx.app.explorer.visible = true;
    fx.app.focus = Focus::Ai;
    let t = draw(&mut fx.app, 120, 40);
    let bar: String = row(&t, 0).chars().skip(85).collect();
    assert_eq!(bar, " 1 claude x                   ±3 + ");
    let theme = Theme::from_name(&fx.app.settings.theme);
    let x = 85 + bar.chars().position(|c| c == '±').unwrap() as u16;
    assert_eq!(
        t.backend().buffer()[(x, 0)].fg,
        theme.git_modified.fg.unwrap()
    );
    let status = row(&t, 39);
    assert!(
        status.trim_end().ends_with("±3 f6 · 1/1 · f12 hide"),
        "{status}"
    );
    // Narrow: the count gives way with the 'x' (fit step 2); the session keeps its label.
    fx.app.explorer.visible = false;
    for _ in 0..5 {
        let mut s = crate::ai::AiSession::new(
            TermTab::fake(Grid::default(), "claude", TermStatus::Running),
            "claude",
            None,
            fx.app.ai.sessions[0].root.clone(),
            crate::ai::spawn::StartMode::New,
        );
        s.attention = None;
        fx.app.ai.sessions.push(s);
    }
    let t = draw(&mut fx.app, 80, 24);
    let bar: String = row(&t, 0).chars().skip(49).collect();
    assert!(bar.starts_with(" 1 claude ") && !bar.contains('±'), "{bar}");
}

#[test]
fn ai_changes_without_an_earlier_version() {
    let mut fx = changed();
    fx.changes().forget_earlier(Path::new(".env"));
    fx.agent_writes(
        ".env",
        "APP_ENV=dev\nAPP_DEBUG=true\nDATABASE_URL=postgres://app@localhost/shop\n",
    );
    for (rel, why) in [
        ("dump.sql", NoEarlier::TooLarge),
        ("Cargo.lock", NoEarlier::Limit),
    ] {
        fx.agent_writes(rel, "x\n");
        fx.changes().set_missing(Path::new(rel), why);
    }
    fx.feed(FsEvent::Rescan(fx.root.clone()));
    fix_times(&mut fx);
    fx.app.explorer.visible = true;
    press(&mut fx.app, "f6");
    assert!(review::select(&mut fx.app, Path::new(".env")));
    key(&mut fx.app, KeyCode::Enter);
    let t = draw(&mut fx.app, 120, 40);
    insta::assert_snapshot!("ai_changes_limits_120x40", t.backend());
    let panel = |y: u16| row(&t, y).chars().skip(85).collect::<String>();
    assert_eq!(panel(1), " Changes · claude · 6 files     f6 ");
    assert_eq!(panel(2), " ! List may be incomplete (watcher ");
    assert_eq!(panel(3), "   rescan at 14:32) · enter hides  ");
    let rows: Vec<String> = (4..10).map(panel).collect();
    assert!(
        rows.iter()
            .any(|r| r == " M .env                 no earlier "),
        "{rows:?}"
    );
    assert!(
        rows.iter()
            .any(|r| r == " A dump.sql              too large "),
        "{rows:?}"
    );
    assert!(
        rows.iter()
            .any(|r| r == " A Cargo.lock                limit "),
        "{rows:?}"
    );
    let head: String = row(&t, 1).chars().skip(30).take(54).collect();
    assert!(
        head.starts_with(" M .env  changed, no earlier version kept"),
        "{head}"
    );
    assert!(
        head.ends_with("f5 keep") && !head.contains("revert"),
        "{head}"
    );
    let text: String = (2..8).map(|y| row(&t, y)).collect();
    assert!(text.contains("vr-ide saw .env only after claude changed it:"));
    assert!(text.contains("revert is off; this is the file as it is now."));
    assert!(text.contains("  1  APP_ENV=dev"), "no +/- signs: {text}");
    let status = row(&t, 39);
    assert!(
        status.starts_with(" DIFF  .env · claude · changed, no earlier version kept"),
        "{status}"
    );
    assert!(
        status.trim_end().ends_with("· f5 keep · revert off"),
        "{status}"
    );
    let theme = Theme::from_name(&fx.app.settings.theme);
    assert_eq!(t.backend().buffer()[(86, 2)].fg, theme.warning.fg.unwrap());
}

#[test]
fn ai_revert_dialogs_and_refusal() {
    let mut fx = reviewing();
    fx.app.explorer.visible = true;
    press(&mut fx.app, "shift+f5");
    let t = draw(&mut fx.app, 120, 40);
    insta::assert_snapshot!("ai_revert_confirm_120x40", t.backend());
    let all: String = (0..40).map(|y| row(&t, y) + "\n").collect();
    assert!(all.contains(" Revert session.rs? "), "{all}");
    assert!(all.contains("Edits by other programs since 14:32 count as claude's."));
    assert!(all.contains("[Revert] [Cancel]"));
    // Refused: the file changed while the dialog was open.
    let newer = std::fs::read_to_string(fx.path(SESSION_RS)).unwrap() + "// more\n";
    fx.write(SESSION_RS, &newer);
    key(&mut fx.app, KeyCode::Left);
    key(&mut fx.app, KeyCode::Enter);
    fx.app.explorer.visible = false;
    let t = draw(&mut fx.app, 80, 24);
    insta::assert_snapshot!("ai_revert_refused_80x24", t.backend());
    assert!(
        row(&t, 22).starts_with("Not reverted: session.rs changed since shown; diff updated"),
        "{}",
        row(&t, 22)
    );
    // A created file.
    fx.app.notices.clear();
    assert!(review::select(&mut fx.app, Path::new(TESTS_RS)));
    key(&mut fx.app, KeyCode::Enter);
    press(&mut fx.app, "shift+f5");
    let t = draw(&mut fx.app, 80, 24);
    insta::assert_snapshot!("ai_revert_created_80x24", t.backend());
    let all: String = (0..24).map(|y| row(&t, y) + "\n").collect();
    assert!(
        all.contains("claude created this file. Revert moves it"),
        "{all}"
    );
    assert!(all.contains("Other programs' edits there count as claude's."));
    assert!(row(&t, 1).starts_with(" A session_tests.rs  new · +88 · claude"));
    let _ = OLD_MD;
}

#[test]
fn ai_changes_empty_on_an_exited_session() {
    let mut fx = fixture();
    let file = fx.path(SESSION_RS);
    fx.app.open_path(&file).unwrap();
    fx.app.ai.sessions[0].tab = TermTab::fake(Grid::default(), "claude", TermStatus::Exited(0));
    fx.app.explorer.visible = false;
    press(&mut fx.app, "f6");
    let t = draw(&mut fx.app, 80, 24);
    insta::assert_snapshot!("ai_changes_empty_80x24", t.backend());
    let panel = |y: u16| row(&t, y).chars().skip(49).collect::<String>();
    assert_eq!(panel(1), " Changes · claude · 0 files f6 ");
    assert_eq!(panel(2), " No changes since it started   ");
    assert_eq!(panel(3), "─".repeat(31));
    let status = row(&t, 23);
    assert!(status.starts_with(" AI  claude [exited 0]"), "{status}");
    assert!(status.trim_end().ends_with("enter restart"), "{status}");
}
