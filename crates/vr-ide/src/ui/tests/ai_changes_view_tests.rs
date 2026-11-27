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
    // 31 cells: one row has no room for its folder (10 cells), so no row shows one and the
    // counts stay one column (mockup ai-changes-A-80x24).
    let narrow = |y: u16| row(&t, y).chars().skip(49).collect::<String>();
    assert_eq!(narrow(2), " M session.rs            +9 -3 ");
    assert_eq!(narrow(3), " A session_tests.rs        +88 ");
    assert_eq!(narrow(4), " D old-session.md          -40 ");
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
    // No line of one word left over by the wrap (mockup ai-revert-created-80x24).
    assert!(
        all.contains("│ (88 lines).") && !all.contains("│ lines)."),
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

/// SEC-13: while the start-up walk runs the list says it may be incomplete, and Enter does
/// not hide that.
#[test]
fn ai_changes_scanning_notice() {
    let mut fx = changed();
    fx.changes().resume();
    review::refresh(&mut fx.app);
    fx.app.explorer.visible = false;
    press(&mut fx.app, "f6");
    let t = draw(&mut fx.app, 80, 24);
    let panel = |t: &Terminal<TestBackend>, y: u16, skip: usize| -> String {
        row(t, y).chars().skip(skip).collect()
    };
    assert_eq!(panel(&t, 2, 49), " ! List may be incomplete      ");
    assert_eq!(panel(&t, 3, 49), "   (still scanning the folder) ");
    assert_eq!(panel(&t, 4, 49), " M session.rs            +9 -3 ");
    let theme = Theme::from_name(&fx.app.settings.theme);
    assert_eq!(t.backend().buffer()[(50, 2)].fg, theme.warning.fg.unwrap());
    // Enter on it changes nothing: the walk ends it.
    key(&mut fx.app, KeyCode::Up);
    key(&mut fx.app, KeyCode::Enter);
    fx.app.explorer.visible = true;
    let t = draw(&mut fx.app, 120, 40);
    assert_eq!(panel(&t, 2, 85), " ! List may be incomplete (still   ");
    assert_eq!(panel(&t, 3, 85), "   scanning the folder)            ");
    fx.changes().scan();
    review::refresh(&mut fx.app);
    let t = draw(&mut fx.app, 120, 40);
    assert_eq!(panel(&t, 2, 85), " M session.rs …es/vr-ide/src +9 -3 ");
}

/// ui-spec S3: the diff's code keeps its syntax colours on the added / removed backgrounds,
/// each side coloured as part of its own file (old text for `-`, new text for the rest).
#[test]
fn ai_diff_lines_have_syntax_colours() {
    for name in ["dracula", "homebrew"] {
        let mut fx = changed();
        fx.app.settings.theme = name.to_string();
        press(&mut fx.app, "f6");
        key(&mut fx.app, KeyCode::Enter);
        fx.app.explorer.visible = true;
        assert!(review::wait_syntax(&fx.app), "{name}");
        let t = draw(&mut fx.app, 120, 40);
        let theme = Theme::from_name(name);
        let buf = t.backend().buffer();
        let find = |text: &str| -> (u16, u16) {
            (0..40)
                .find_map(|y| {
                    let r = row(&t, y);
                    r.find(text).map(|at| (r[..at].chars().count() as u16, y))
                })
                .unwrap_or_else(|| panic!("{name}: no {text:?} on screen"))
        };
        let fg = |cap| theme.syntax(cap).fg.unwrap();
        let plain = theme.text.fg.unwrap();
        assert_ne!(fg(vr_syntax::Capture::Keyword), plain, "{name}");
        // Added: the keyword and the type in their colours, on the added background.
        let (x, y) = find("pub ai: Vec<AgentState>");
        assert_eq!(buf[(x, y)].fg, fg(vr_syntax::Capture::Keyword), "{name}");
        assert_eq!(buf[(x, y)].bg, theme.diff_added_bg, "{name}");
        assert_eq!(buf[(x + 8, y)].fg, fg(vr_syntax::Capture::Type), "{name}");
        // Removed: coloured from the earlier text, on the removed background.
        let (x, y) = find("/// Drops tabs whose file is gone.");
        assert_eq!(buf[(x, y)].fg, fg(vr_syntax::Capture::Comment), "{name}");
        assert_eq!(buf[(x, y)].bg, theme.diff_removed_bg, "{name}");
        // Context, and the hunk header stays a header.
        let (x, y) = find("impl Session {");
        assert_eq!(buf[(x, y)].fg, fg(vr_syntax::Capture::Keyword), "{name}");
        let (x, y) = find("@@ -9,6 +9,7 @@");
        assert_eq!(buf[(x, y)].fg, theme.mode_ai.bg.unwrap(), "{name}");
    }
    // A file without an earlier version is coloured as the file it is; a deleted file has
    // one side to colour, a closed tab none.
    let mut fx = changed();
    fx.changes().forget_earlier(Path::new(".env"));
    fx.agent_writes("crates/vr-ide/src/late.rs", "pub fn late() {}\n");
    fx.changes()
        .set_missing(Path::new("crates/vr-ide/src/late.rs"), NoEarlier::NotSeen);
    review::refresh(&mut fx.app);
    press(&mut fx.app, "f6");
    assert!(review::select(
        &mut fx.app,
        Path::new("crates/vr-ide/src/late.rs")
    ));
    key(&mut fx.app, KeyCode::Enter);
    assert!(review::wait_syntax(&fx.app));
    let t = draw(&mut fx.app, 120, 40);
    let theme = Theme::from_name(&fx.app.settings.theme);
    let y = (0..40)
        .find(|&y| row(&t, y).contains("pub fn late()"))
        .unwrap();
    let x = row(&t, y).find("pub fn").unwrap() as u16;
    assert_eq!(
        t.backend().buffer()[(x, y)].fg,
        theme.syntax(vr_syntax::Capture::Keyword).fg.unwrap()
    );
    assert!(review::select(&mut fx.app, Path::new(OLD_MD)));
    key(&mut fx.app, KeyCode::Enter);
    assert_eq!(
        review::diff_highlighters(&fx.app),
        1,
        "a deleted file has one side"
    );
    review::close_diff(&mut fx.app);
    assert_eq!(review::diff_highlighters(&fx.app), 0);
}
