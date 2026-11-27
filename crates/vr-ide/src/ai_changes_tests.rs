//! AI changes in the app (AC-9, ui-spec S3, SEC-12, SEC-13): the list in the session's tab, the
//! diff tab, Keep and Revert with their keys, dialogs and notices. Most tests use a fake
//! session whose tracker is fed by hand; one runs a real agent script and the real watcher.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use vr_core::FsEvent;
use vr_term::{Grid, TermStatus};

use crate::ai::AiSession;
use crate::ai::changes::{Changes, Config, Incomplete, Limits};
use crate::ai::review;
use crate::ai::spawn::StartMode;
use crate::app::{App, Focus, NoticeLevel};
use crate::app_tests::{press, test_app};
use crate::commands::terminal::TermTab;

pub(crate) const MARGIN: Duration = Duration::from_millis(15);
/// The folder the fake session shows (the tracker watches a temp folder).
pub(crate) const SHOWN_ROOT: &str = "/home/user/DEV/vr-ide";
pub(crate) const SESSION_RS: &str = "crates/vr-ide/src/session.rs";
pub(crate) const TESTS_RS: &str = "crates/vr-ide/src/session_tests.rs";
pub(crate) const OLD_MD: &str = "docs/old-session.md";

const SESSION_OLD: &str = "\
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// What a `--session` instance restores (D1).
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Session {
    pub roots: Vec<PathBuf>,
    pub tabs: Vec<TabState>,
    pub active: usize,
    pub expanded: Vec<PathBuf>,
}

impl Session {
    pub fn load(path: &Path) -> Result<Session, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        toml::from_str(&text).map_err(|e| e.to_string())
    }

    /// Drops tabs whose file is gone.
    pub fn skip_missing(&mut self) {
        self.tabs.retain(|t| t.path.exists());
    }
}
";

const SESSION_NEW: &str = "\
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// What a `--session` instance restores (D1).
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Session {
    pub roots: Vec<PathBuf>,
    pub tabs: Vec<TabState>,
    pub active: usize,
    pub expanded: Vec<PathBuf>,
    pub ai: Vec<AgentState>,
}

impl Session {
    pub fn load(path: &Path) -> Result<Session, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        toml::from_str(&text).map_err(|e| e.to_string())
    }

    /// Drops tabs whose file is gone, for the notice.
    pub fn skip_missing(&mut self) -> Vec<PathBuf> {
        let (kept, gone): (Vec<_>, Vec<_>) = self
            .tabs
            .drain(..)
            .partition(|t| t.path.exists());
        self.tabs = kept;
        gone.into_iter().map(|t| t.path).collect()
    }
}
";

pub(crate) struct Fx {
    pub dir: tempfile::TempDir,
    /// The folder the tracker watches (canonical).
    pub root: PathBuf,
    pub app: App,
}

impl Fx {
    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    pub fn write(&self, rel: &str, text: &str) -> PathBuf {
        let p = self.path(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
        p
    }

    pub fn changes(&self) -> &Changes {
        self.app.ai.sessions[0].changes.as_ref().unwrap()
    }

    pub fn feed(&mut self, ev: FsEvent) {
        self.changes().feed(ev);
        review::refresh(&mut self.app);
    }

    /// The agent rewrites a file.
    pub fn agent_writes(&mut self, rel: &str, text: &str) {
        let p = self.write(rel, text);
        self.feed(FsEvent::Modified(p));
    }

    pub fn rels(&self) -> Vec<String> {
        self.app.ai.sessions[0]
            .records
            .iter()
            .map(|r| r.rel.display().to_string())
            .collect()
    }

    pub fn notices(&self) -> Vec<(NoticeLevel, String)> {
        self.app
            .notices
            .iter()
            .map(|n| (n.level, n.text.clone()))
            .collect()
    }

    pub fn trashed(&self) -> usize {
        std::fs::read_dir(self.dir.path().join("trash")).map_or(0, |d| d.count())
    }
}

fn limits() -> Limits {
    Limits {
        margin: Some(MARGIN),
        ..Limits::default()
    }
}

/// An app with one fake `claude` session on a temp workspace holding `session.rs`,
/// `docs/old-session.md` and `.env`; nothing has changed yet.
pub(crate) fn fixture() -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let root = base.join("ws");
    std::fs::create_dir_all(&root).unwrap();
    let (mut app, _) = test_app(&root);
    app.trash = Box::new(vr_fs::FakeTrash::new(base.join("trash")));
    app.ai.changes_dir = Some(base.join("state/ai"));
    app.ai.changes_limits = limits();
    let mut fx = Fx { dir, root, app };
    fx.write(SESSION_RS, SESSION_OLD);
    let forty: String = (1..=40).map(|i| format!("old line {i}\n")).collect();
    fx.write(OLD_MD, &forty);
    fx.write(".env", "APP_ENV=dev\nAPP_DEBUG=true\n");
    std::thread::sleep(MARGIN * 2);
    let changes = Changes::manual(Config {
        root: fx.root.clone(),
        store_base: base.join("state/ai"),
        total: Arc::clone(&fx.app.ai.changes_total),
        limits: limits(),
        excluded: vec![base.join("state")],
        own: None,
        wake: Arc::new(AtomicBool::new(false)),
    })
    .unwrap();
    changes.scan();
    let mut s = AiSession::new(
        TermTab::fake(Grid::default(), "claude", TermStatus::Running),
        "claude",
        None,
        PathBuf::from(SHOWN_ROOT),
        StartMode::New,
    );
    s.changes = Some(changes);
    fx.app.ai.sessions.push(s);
    fx.app.ai.visible = true;
    fx
}

/// [`fixture`] after the agent changed `session.rs`, created `session_tests.rs` and deleted
/// `old-session.md`; times are fixed (14:32 UTC) for the screens.
pub(crate) fn changed() -> Fx {
    let mut fx = fixture();
    fx.agent_writes(SESSION_RS, SESSION_NEW);
    let tests: String = (1..=88).map(|i| format!("// test line {i}\n")).collect();
    let p = fx.write(TESTS_RS, &tests);
    fx.feed(FsEvent::Created(p));
    let gone = fx.path(OLD_MD);
    std::fs::remove_file(&gone).unwrap();
    fx.feed(FsEvent::Removed(gone));
    fix_times(&mut fx);
    fx
}

pub(crate) fn fix_times(fx: &mut Fx) {
    let at = UNIX_EPOCH + Duration::from_secs(14 * 3600 + 32 * 60);
    fx.changes().set_times(at);
    review::refresh(&mut fx.app);
}

fn key(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn diff_text(app: &App) -> String {
    let i = review::diff_index(app).expect("a diff tab");
    app.docs[i].buffer.text().to_string()
}

/// Answers the open dialog with its first button (Revert).
fn confirm(app: &mut App) {
    assert!(app.dialog.is_some(), "a dialog is open");
    key(app, KeyCode::Left);
    key(app, KeyCode::Enter);
}

#[test]
fn f6_opens_the_list_and_enter_shows_a_diff_in_the_editor() {
    let mut fx = changed();
    fx.app.focus = Focus::Editor;
    assert_eq!(fx.rels(), [SESSION_RS, TESTS_RS, OLD_MD]);
    press(&mut fx.app, "f6");
    assert!(fx.app.ran.contains(&"ai.changes".to_string()));
    assert_eq!(fx.app.focus, Focus::Ai);
    assert!(review::list_has_keys(&fx.app));
    assert_eq!(review::diff_index(&fx.app), None);
    // Enter opens the diff tab; the list keeps the keys, so the next file is one key away.
    key(&mut fx.app, KeyCode::Enter);
    let i = review::diff_index(&fx.app).expect("diff tab");
    assert_eq!(fx.app.active, i);
    assert!(review::list_has_keys(&fx.app));
    let text = diff_text(&fx.app);
    assert!(text.contains("@@ -9,"), "{text}");
    assert!(text.contains("    pub ai: Vec<AgentState>,"));
    assert!(text.contains("    /// Drops tabs whose file is gone."));
    let d = fx.app.ai.diff.as_ref().unwrap();
    assert_eq!((d.diff.added, d.diff.removed), (9, 3));
    // The same tab shows the next file.
    key(&mut fx.app, KeyCode::Down);
    key(&mut fx.app, KeyCode::Enter);
    assert_eq!(fx.app.docs.len(), 1);
    assert!(diff_text(&fx.app).starts_with("@@ -0,0 +1,88 @@ new file\n// test line 1\n"));
    key(&mut fx.app, KeyCode::Down);
    key(&mut fx.app, KeyCode::Down);
    key(&mut fx.app, KeyCode::Enter);
    assert!(diff_text(&fx.app).starts_with("@@ -1,40 +0,0 @@ deleted\nold line 1\n"));
    // Esc and f6 close the list; the session has the keys again.
    key(&mut fx.app, KeyCode::Esc);
    assert!(fx.app.ai.list.is_none());
    assert_eq!(fx.app.focus, Focus::Ai);
    press(&mut fx.app, "f6");
    assert!(review::list_has_keys(&fx.app));
    press(&mut fx.app, "f6");
    assert!(fx.app.ai.list.is_none());
}

#[test]
fn keep_drops_the_record_without_asking() {
    let mut fx = changed();
    press(&mut fx.app, "f6");
    key(&mut fx.app, KeyCode::Enter);
    press(&mut fx.app, "f5");
    assert!(fx.app.dialog.is_none());
    assert_eq!(fx.rels(), [TESTS_RS, OLD_MD]);
    assert_eq!(review::diff_index(&fx.app), None, "its diff tab closes");
    assert_eq!(
        fx.notices(),
        [(NoticeLevel::Info, "Kept session.rs".to_string())]
    );
    assert_eq!(
        std::fs::read_to_string(fx.path(SESSION_RS)).unwrap(),
        SESSION_NEW
    );
    // From the diff tab with the editor focused.
    key(&mut fx.app, KeyCode::Enter);
    press(&mut fx.app, "f3");
    assert_eq!(fx.app.focus, Focus::Editor);
    press(&mut fx.app, "f5");
    assert_eq!(fx.rels(), [OLD_MD]);
    assert!(fx.app.docs.is_empty());
}

#[test]
fn revert_asks_first_and_writes_the_earlier_version_back() {
    let mut fx = changed();
    press(&mut fx.app, "f6");
    key(&mut fx.app, KeyCode::Enter);
    press(&mut fx.app, "shift+f5");
    let d = fx.app.dialog.as_ref().expect("asks first");
    let crate::ui::dialog::DialogWidget::Confirm(c) = &d.widget else {
        panic!("a confirm dialog");
    };
    assert_eq!(c.title, "Revert session.rs?");
    assert_eq!(
        c.message,
        "Writes back session.rs as it was before claude changed it (14:32). Today's text, +9 -3, is lost."
    );
    assert_eq!(
        c.note.as_deref(),
        Some("Edits by other programs since 14:32 count as claude's.")
    );
    assert_eq!(
        (c.buttons.join("|"), c.selected),
        ("Revert|Cancel".into(), 1)
    );
    // Cancel is the default: Enter changes nothing.
    key(&mut fx.app, KeyCode::Enter);
    assert_eq!(
        std::fs::read_to_string(fx.path(SESSION_RS)).unwrap(),
        SESSION_NEW
    );
    assert_eq!(fx.rels().len(), 3);
    press(&mut fx.app, "shift+f5");
    confirm(&mut fx.app);
    assert_eq!(
        std::fs::read_to_string(fx.path(SESSION_RS)).unwrap(),
        SESSION_OLD
    );
    assert_eq!(fx.rels(), [TESTS_RS, OLD_MD]);
    assert_eq!(review::diff_index(&fx.app), None);
    assert_eq!(
        fx.notices().last().unwrap(),
        &(NoticeLevel::Info, "Reverted session.rs".to_string())
    );
    // A created file goes to the trash; a deleted one comes back.
    press(&mut fx.app, "shift+f5");
    let crate::ui::dialog::DialogWidget::Confirm(c) = &fx.app.dialog.as_ref().unwrap().widget
    else {
        panic!()
    };
    assert_eq!(
        c.message,
        "claude created this file. Revert moves it to the trash (88 lines)."
    );
    assert_eq!(
        c.note.as_deref(),
        Some("Other programs' edits there count as claude's.")
    );
    confirm(&mut fx.app);
    assert!(!fx.path(TESTS_RS).exists());
    assert_eq!(fx.trashed(), 1);
    press(&mut fx.app, "shift+f5");
    let crate::ui::dialog::DialogWidget::Confirm(c) = &fx.app.dialog.as_ref().unwrap().widget
    else {
        panic!()
    };
    assert_eq!(
        c.message,
        "Restores old-session.md (40 lines) that claude deleted."
    );
    confirm(&mut fx.app);
    assert!(fx.path(OLD_MD).exists());
    assert!(fx.rels().is_empty());
}

#[test]
fn revert_is_refused_when_the_file_changed_since_it_was_shown() {
    let mut fx = changed();
    press(&mut fx.app, "f6");
    key(&mut fx.app, KeyCode::Enter);
    press(&mut fx.app, "shift+f5");
    // The agent writes again while the dialog is open.
    let newer = format!("{SESSION_NEW}// one more\n");
    fx.write(SESSION_RS, &newer);
    confirm(&mut fx.app);
    assert_eq!(std::fs::read_to_string(fx.path(SESSION_RS)).unwrap(), newer);
    assert_eq!(
        fx.notices().last().unwrap(),
        &(
            NoticeLevel::Warning,
            "Not reverted: session.rs changed since shown; diff updated".to_string()
        )
    );
    assert_eq!(fx.rels().len(), 3);
    assert!(
        diff_text(&fx.app).contains("// one more"),
        "the diff reloads"
    );
    // Looked at again, it reverts.
    press(&mut fx.app, "shift+f5");
    confirm(&mut fx.app);
    assert_eq!(
        std::fs::read_to_string(fx.path(SESSION_RS)).unwrap(),
        SESSION_OLD
    );
}

#[test]
fn revert_refuses_unsaved_edits_and_a_clean_tab_reloads() {
    let mut fx = changed();
    let p = fx.path(SESSION_RS);
    fx.app.open_path(&p).unwrap();
    crate::app_tests::space_i(&mut fx.app);
    key(&mut fx.app, KeyCode::Char('x'));
    assert!(fx.app.docs[0].is_modified());
    press(&mut fx.app, "f6");
    press(&mut fx.app, "shift+f5");
    assert!(fx.app.dialog.is_none());
    assert_eq!(
        fx.notices().last().unwrap(),
        &(
            NoticeLevel::Warning,
            "Not reverted: session.rs has unsaved edits in the editor".to_string()
        )
    );
    assert_eq!(std::fs::read_to_string(&p).unwrap(), SESSION_NEW);
    // Without the edit the open tab shows the earlier version after the revert.
    fx.app.docs[0].reload().unwrap();
    assert!(!fx.app.docs[0].is_modified());
    press(&mut fx.app, "shift+f5");
    confirm(&mut fx.app);
    assert_eq!(std::fs::read_to_string(&p).unwrap(), SESSION_OLD);
    assert_eq!(fx.app.docs[0].buffer.text().to_string(), SESSION_OLD);
}

#[test]
fn without_an_earlier_version_only_keep_works() {
    let mut fx = fixture();
    // As for a file the walk could not copy (first seen after its change, too large, a cap).
    fx.changes().forget_earlier(Path::new(".env"));
    let big = fx.write(".env", "APP_ENV=prod\n");
    fx.feed(FsEvent::Modified(big));
    assert_eq!(fx.rels(), [".env"]);
    press(&mut fx.app, "f6");
    key(&mut fx.app, KeyCode::Enter);
    let text = diff_text(&fx.app);
    assert_eq!(
        text,
        "vr-ide saw .env only after claude changed it:\n\
         revert is off; this is the file as it is now.\n\nAPP_ENV=prod\n"
    );
    press(&mut fx.app, "shift+f5");
    assert!(fx.app.dialog.is_none());
    assert_eq!(
        fx.notices().last().unwrap(),
        &(
            NoticeLevel::Warning,
            "No earlier version of .env kept; Keep only".to_string()
        )
    );
    press(&mut fx.app, "f5");
    assert!(fx.rels().is_empty());
}

#[test]
fn f5_and_shift_f5_do_nothing_outside_the_list_and_a_diff_tab() {
    let mut fx = changed();
    let p = fx.path(SESSION_RS);
    fx.app.open_path(&p).unwrap();
    fx.app.focus = Focus::Editor;
    press(&mut fx.app, "f5");
    press(&mut fx.app, "shift+f5");
    assert!(fx.app.dialog.is_none());
    assert_eq!(fx.rels().len(), 3);
    // The list is open but the session below it has the keys: the keys are the program's.
    press(&mut fx.app, "f6");
    fx.app.ai.list.as_mut().unwrap().focused = false;
    fx.app.ran.clear();
    press(&mut fx.app, "f5");
    press(&mut fx.app, "shift+f5");
    assert!(fx.app.dialog.is_none());
    assert_eq!(fx.rels().len(), 3);
    assert!(
        !fx.app
            .ran
            .iter()
            .any(|c| c == "ai.keep" || c == "ai.revert")
    );
    // f6 from the session gives the list the keys again.
    press(&mut fx.app, "f6");
    assert!(review::list_has_keys(&fx.app));
}

#[test]
fn sec13_the_diff_tab_is_read_only_and_kept_out_of_recovery_and_recent() {
    let mut fx = changed();
    press(&mut fx.app, "f6");
    key(&mut fx.app, KeyCode::Enter);
    press(&mut fx.app, "f3");
    let i = review::diff_index(&fx.app).unwrap();
    let before = diff_text(&fx.app);
    crate::app_tests::space_i(&mut fx.app);
    key(&mut fx.app, KeyCode::Char('x'));
    press(&mut fx.app, "ctrl+d");
    assert_eq!(diff_text(&fx.app), before);
    let doc = &fx.app.docs[i];
    assert!(doc.read_only && doc.path.is_none() && !doc.is_modified());
    assert!(fx.app.recent.is_empty());
    assert!(fx.app.recent_files.entries.is_empty());
    // What a panic or a discarded quit writes: modified documents only.
    let state = crate::recovery::RecoveryState::default();
    state.publish(&fx.app.docs);
    let out = fx.dir.path().join("recovery");
    assert!(crate::recovery::write_recovery(&state, &out).is_empty());
    // Find works in it.
    press(&mut fx.app, "ctrl+f");
    for c in "AgentState".chars() {
        key(&mut fx.app, KeyCode::Char(c));
    }
    assert_eq!(fx.app.find.matches.len(), 1);
}

#[test]
fn sec13_names_are_sanitized_and_a_watcher_error_shows_a_banner() {
    let mut fx = fixture();
    let name = "a\u{1b}[2Jb\u{202e}c.txt";
    let p = fx.write(name, "x\n");
    fx.feed(FsEvent::Created(p));
    press(&mut fx.app, "f6");
    let rows = review::rows(&fx.app);
    assert_eq!(rows.len(), 1);
    let shown = review::shown_name(&fx.app.ai.sessions[0].records[0].rel);
    assert!(
        !shown.contains('\u{1b}') && !shown.contains('\u{202e}'),
        "{shown:?}"
    );
    press(&mut fx.app, "shift+f5");
    let crate::ui::dialog::DialogWidget::Confirm(c) = &fx.app.dialog.as_ref().unwrap().widget
    else {
        panic!()
    };
    assert!(!c.title.contains('\u{1b}') && !c.title.contains('\u{202e}'));
    key(&mut fx.app, KeyCode::Esc);
    // SEC-13: a watcher error marks the list until Enter on the banner hides it.
    fx.feed(FsEvent::Error("too many watches".into()));
    let rows = review::rows(&fx.app);
    assert!(matches!(rows[0], review::Row::Banner(b) if b.why == Incomplete::Watcher));
    assert_eq!(
        fx.app.ai.list.as_ref().unwrap().selected,
        1,
        "the file stays selected"
    );
    key(&mut fx.app, KeyCode::Up);
    key(&mut fx.app, KeyCode::Enter);
    assert_eq!(review::rows(&fx.app).len(), 1);
    assert_eq!(review::diff_index(&fx.app), None);
}

#[test]
fn the_mouse_opens_the_list_and_a_row() {
    let mut fx = changed();
    fx.app.focus = Focus::Editor;
    let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
    t.draw(|f| crate::ui::render(f, &mut fx.app)).unwrap();
    let area = fx.app.layout.ai;
    let bar: String = (area.x..area.x + area.width)
        .map(|x| t.backend().buffer()[(x, area.y)].symbol().to_string())
        .collect();
    let at = bar.find("±3").expect("the count in the tab bar") as u16;
    let x = area.x + bar[..usize::from(at)].chars().count() as u16;
    let click = |app: &mut App, column: u16, row: u16| {
        app.handle_mouse(MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row,
            modifiers: KeyModifiers::NONE,
        });
    };
    click(&mut fx.app, x, area.y);
    assert!(review::list_has_keys(&fx.app));
    t.draw(|f| crate::ui::render(f, &mut fx.app)).unwrap();
    // Row 0 is the tab bar, 1 the list's header, then the files.
    click(&mut fx.app, area.x + 3, area.y + 3);
    assert_eq!(fx.app.ai.list.as_ref().unwrap().selected, 1);
    assert!(diff_text(&fx.app).contains("// test line 1"));
    // A click on the session below gives it the keys; the list stays open.
    click(&mut fx.app, area.x + 3, area.y + 20);
    assert!(fx.app.ai.list.is_some());
    assert!(!review::list_has_keys(&fx.app));
}

#[test]
fn closing_the_session_closes_its_diff_tab_and_removes_its_store() {
    let mut fx = changed();
    press(&mut fx.app, "f6");
    key(&mut fx.app, KeyCode::Enter);
    let store = fx.changes().store_dir();
    assert!(store.exists());
    crate::ai::close(&mut fx.app, 0);
    assert!(!store.exists());
    assert_eq!(review::diff_index(&fx.app), None);
    assert!(fx.app.ai.diff.is_none() && fx.app.ai.list.is_none());
    // f6 without a session says so.
    press(&mut fx.app, "f6");
    assert_eq!(
        fx.notices().last().unwrap().1,
        "AI: no session is open (ctrl+f12 starts one)"
    );
}

// ---- a real agent -----------------------------------------------------------------------------

/// Waits for a line, then changes one file, creates one and deletes one.
const EDIT_AGENT: &str = "#!/bin/sh\nread go\nprintf 'new\\n' > a.txt\nprintf 'made\\n' > made.txt\n\
                          rm gone.txt\necho DONE\nexec cat\n";

fn wait(what: &str, mut cond: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    while !cond() {
        assert!(Instant::now() < end, "timed out: {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_real_agents_changes_are_listed_kept_and_reverted() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().canonicalize().unwrap();
    let (root, bin) = (base.join("ws"), base.join("bin"));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    let agent = bin.join("agent");
    std::fs::write(&agent, EDIT_AGENT).unwrap();
    std::fs::set_permissions(&agent, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(root.join("a.txt"), "old\n").unwrap();
    std::fs::write(root.join("gone.txt"), "bye\n").unwrap();
    std::thread::sleep(MARGIN * 2);
    let (mut app, _) = test_app(&root);
    app.trash = Box::new(vr_fs::FakeTrash::new(base.join("trash")));
    app.ai.changes_dir = Some(base.join("state/ai"));
    app.ai.changes_limits = limits();
    app.settings.ai_tools = vec![vr_config::AiTool {
        name: "claude".into(),
        command: agent.display().to_string(),
        args: vec![],
        continue_args: vec![],
        resume_args: vec![],
    }];
    crate::ai::open_tool(&mut app, "claude").unwrap();
    let started = SystemTime::now();
    wait("the start-up walk", || {
        !app.ai.sessions[0].changes.as_ref().unwrap().scanning()
    });
    assert!(started.elapsed().unwrap() < Duration::from_secs(10));
    // A focused session gets Enter (and f5): the agent goes on.
    key(&mut app, KeyCode::Enter);
    wait("the agent's edits", || {
        app.ai.sessions[0].tab.screen_text().contains("DONE")
    });
    wait("three records", || {
        review::refresh(&mut app);
        app.ai.sessions[0].records.len() == 3
    });
    press(&mut app, "f6");
    assert!(review::select(&mut app, Path::new("a.txt")));
    press(&mut app, "shift+f5");
    confirm(&mut app);
    assert_eq!(
        std::fs::read_to_string(root.join("a.txt")).unwrap(),
        "old\n"
    );
    assert!(review::select(&mut app, Path::new("made.txt")));
    press(&mut app, "shift+f5");
    confirm(&mut app);
    assert!(!root.join("made.txt").exists());
    assert!(base.join("trash").read_dir().unwrap().count() == 1);
    assert!(review::select(&mut app, Path::new("gone.txt")));
    press(&mut app, "f5");
    assert!(!root.join("gone.txt").exists());
    // vr-ide's own writes (the revert) do not come back as changes.
    std::thread::sleep(vr_core::DEBOUNCE * 3);
    review::refresh(&mut app);
    assert!(app.ai.sessions[0].records.is_empty());
    // The program ends: later edits in the folder are nobody's.
    let store = app.ai.sessions[0].changes.as_ref().unwrap().store_dir();
    crate::ai::shutdown(&mut app);
    assert!(!store.exists());
}
