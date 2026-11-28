//! The loop step every frontend drives ([`step`]), the terminal loop ([`run`]) and the
//! crossterm → `KeyChord` conversion.

use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::layout::Rect;
use vr_config::{KeyChord, Mods};

use crate::app::App;
use crate::frontend::{CursorShape, DrawnCursor, Frontend};
use crate::recovery;
use crate::ui;

/// How long the loop waits for terminal input before checking watcher events and timers.
const POLL: Duration = Duration::from_millis(50);

/// Converts a terminal key event to a keymap chord. Without keyboard enhancement
/// (`enhanced` false) the legacy control bytes crossterm reports as ctrl+4/5/7 are mapped
/// back to the keys that produce them (ctrl+\, ctrl+], ctrl+/).
pub fn chord_from_key(key: &KeyEvent, enhanced: bool) -> Option<KeyChord> {
    use crossterm::event::KeyCode as K;
    use vr_config::KeyCode as C;
    let m = key.modifiers;
    let mut mods = Mods {
        ctrl: m.contains(KeyModifiers::CONTROL),
        shift: m.contains(KeyModifiers::SHIFT),
        alt: m.contains(KeyModifiers::ALT),
    };
    let code = match key.code {
        K::Char(mut c) => {
            if c.is_ascii_uppercase() {
                mods.shift = true;
            }
            if !enhanced && mods.ctrl {
                c = match c {
                    '4' => '\\',
                    '5' => ']',
                    '7' => '/',
                    other => other,
                };
            }
            // `KeyChord::new` names a shifted symbol with ctrl/alt as shift + its base key.
            C::Char(c)
        }
        K::F(n) => C::F(n),
        K::Enter => C::Enter,
        K::Esc => C::Esc,
        K::Tab => C::Tab,
        K::BackTab => {
            mods.shift = true;
            C::Tab
        }
        K::Backspace => C::Backspace,
        K::Delete => C::Delete,
        K::Insert => C::Insert,
        K::Home => C::Home,
        K::End => C::End,
        K::PageUp => C::PageUp,
        K::PageDown => C::PageDown,
        K::Up => C::Up,
        K::Down => C::Down,
        K::Left => C::Left,
        K::Right => C::Right,
        _ => return None,
    };
    Some(KeyChord::new(code, mods))
}

/// Chords a terminal without keyboard enhancement cannot deliver as themselves.
/// Legacy encoding has one control byte per letter (shift is lost), only `[ \ ] /` and space
/// among the other keys (ctrl+[ is Escape itself), and no modified Enter or Tab.
pub fn legacy_unreachable(chord: &KeyChord) -> bool {
    use vr_config::KeyCode as C;
    let m = chord.mods;
    match chord.code {
        C::Char(c) if m.ctrl => {
            // ctrl+` arrives as NUL, which the app takes as ctrl+` outside a terminal (D185).
            // With alt it arrives as ESC NUL, which is ctrl+alt+space.
            (m.alt && c == '`')
                || (m.shift && !(c == ' '))
                || !(c.is_ascii_lowercase() || matches!(c, ' ' | '\\' | ']' | '/' | '`'))
        }
        C::Enter => m.ctrl || m.shift,
        C::Tab => m.ctrl || m.alt,
        _ => false,
    }
}

/// Publishes the modified buffers for the panic hook, then draws one frame with `render`.
/// Publishing first keeps the last batch of edits recoverable when drawing panics.
pub fn draw_frame<B: Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    state: &recovery::RecoveryState,
    render: impl FnOnce(&mut ratatui::Frame, &mut App),
) -> anyhow::Result<()>
where
    B::Error: Send + Sync + 'static,
{
    state.publish_in(&app.docs, &app.workspace);
    terminal.draw(|f| render(f, app))?;
    app.dirty = false;
    Ok(())
}

/// Passes on the result of [`run`]; when the loop failed (e.g. the terminal went away), the
/// modified buffers are written to the recovery directory `dir` first and the error names
/// the files.
pub fn finish(result: anyhow::Result<()>, app: &App, dir: &std::path::Path) -> anyhow::Result<()> {
    let Err(e) = result else {
        return Ok(());
    };
    let docs: Vec<&vr_core::Document> = app.docs.iter().filter(|d| d.is_modified()).collect();
    let written = recovery::write_docs_in(&docs, &app.workspace, dir);
    if written.is_empty() {
        return Err(e);
    }
    let list: Vec<String> = written.iter().map(|p| p.display().to_string()).collect();
    Err(e.context(format!("unsaved buffers written to {}", list.join(", "))))
}

/// The exit sequence after [`run`]: `restore_terminal`, [`finish`] (recovery), then the
/// project replace's bounded wait (`wait`), then the exit messages on stderr.
pub fn exit(
    result: anyhow::Result<()>,
    app: &mut App,
    dir: &std::path::Path,
    restore_terminal: &mut dyn FnMut(),
    wait: Duration,
) -> anyhow::Result<()> {
    exit_with(result, app, dir, restore_terminal, &mut || {}, wait)
}

/// [`exit`] calling `after_recovery` once the unsaved buffers are written (the window gives
/// SIGTERM its default action back there).
pub fn exit_with(
    result: anyhow::Result<()>,
    app: &mut App,
    dir: &std::path::Path,
    restore_terminal: &mut dyn FnMut(),
    after_recovery: &mut dyn FnMut(),
    wait: Duration,
) -> anyhow::Result<()> {
    use crate::recovery::note;
    // Nothing may wait before these two: a worker blocked in I/O would leave the terminal
    // in the alternate screen and the unsaved buffers unwritten.
    restore_terminal();
    crate::recovery::GLOBAL.set_tui_active(false);
    let result = finish(result, app, dir);
    after_recovery();
    // A file opened just before quitting is still queued for the recent-files list.
    app.close_recent_files(Duration::from_millis(500));
    // Before the terminals go: their shells name the folders the session keeps.
    crate::session::save_on_quit(app);
    // Terminals get SIGHUP (SIGKILL after a grace period) and are reaped before vr-ide exits
    // (AC-14d); each wait is bounded by vr-term.
    crate::commands::terminal::shutdown(app);
    crate::ai::shutdown(app);
    // A git read stuck on a slow disk must not outlive vr-ide (SEC-1).
    crate::git::shutdown(app);
    // After the recovery write: printing can block on a full stderr. A closed or hung-up
    // stderr drops the lines (`note`); it never cuts the sequence short.
    for m in crate::recovery::GLOBAL.take_deferred() {
        note(format_args!("{m}"));
    }
    if crate::commands::search::replacing(app) {
        note(format_args!(
            "stopping the project replace (waiting up to {} s)…",
            wait.as_secs_f32()
        ));
    }
    crate::commands::search::shutdown_within(app, wait);
    for m in &app.exit_messages {
        note(format_args!("{m}"));
    }
    result
}

/// How long to wait for input before looking at the wake flags again. A shown terminal panel
/// with a running program is polled more often so a program's output (an echo of typed keys) appears within a frame;
/// still a timed wait, not a busy loop.
const TERMINAL_POLL: Duration = Duration::from_millis(16);

pub(crate) fn poll_interval(app: &App) -> Duration {
    let running = app
        .terminal
        .tabs
        .iter()
        .any(|t| t.status() == vr_term::TermStatus::Running);
    let agents = app
        .ai
        .sessions
        .iter()
        .any(|s| s.tab.status() == vr_term::TermStatus::Running);
    if (app.terminal.visible && running) || (app.ai.visible && agents) {
        TERMINAL_POLL
    } else {
        POLL
    }
}

/// The focused terminal's program picks its cursor shape (M5); otherwise block cursor in read
/// mode, bar where typing goes into text (D145).
fn cursor_style(app: &App) -> CursorShape {
    crate::commands::terminal::cursor_style(app)
        .map(CursorShape::from_crossterm)
        .unwrap_or(if app.wants_bar_cursor() {
            CursorShape::BlinkingBar
        } else {
            CursorShape::SteadyBlock
        })
}

/// Sets the cursor shape only when it changes.
fn set_cursor_shape(
    frontend: &mut impl Frontend,
    current: &mut Option<CursorShape>,
    shape: CursorShape,
) {
    if *current == Some(shape) {
        return;
    }
    if frontend.set_cursor_shape(shape).is_ok() {
        *current = Some(shape);
    }
}

/// The window title when it differs from `last`, the one written last (`None`: not written).
fn changed_title(last: Option<&str>, app: &App) -> Option<String> {
    let now = app.title();
    (last != Some(now.as_str())).then_some(now)
}

/// Sets the window title when the workspace changed it.
fn set_title(frontend: &mut impl Frontend, last: &mut Option<String>, app: &App) {
    let Some(title) = changed_title(last.as_deref(), app) else {
        return;
    };
    if frontend.set_title(&title).is_ok() {
        *last = Some(title);
    }
}

/// What one pass of the loop does before it draws: watcher, index, search and workspace jobs,
/// timers, highlighters, terminal output, AI usage reports. Sets `app.dirty` when something must
/// be redrawn.
pub(crate) fn background_pass(app: &mut App, now: Instant) {
    app.drain_fs_events();
    app.drain_index();
    app.drain_search();
    app.drain_ws_job();
    app.tick(now);
    // A highlighter's worker published spans (or gave up) since the last pass.
    if app.syntax.take_wake() {
        app.dirty = true;
    }
    // A terminal printed, changed its title or exited since the last pass.
    if app.terminal.take_wake() {
        // Terminals and AI sessions share the wake flag (one PTY machinery).
        let terminals = crate::commands::terminal::refresh(app);
        if crate::ai::refresh(app) || terminals {
            app.dirty = true;
        }
    }
    // claude's usage reports come without output or a wake (a hidden panel, an idle agent).
    if crate::ai::read_usage(app, now) {
        app.dirty = true;
    }
    // A quiet session's screen is read for its limit or login message (AC-5, SEC-26).
    if crate::ai::limits::check(app, now) {
        app.dirty = true;
    }
    // A limit event moves a session to another account; a switch under way goes on (AC-6).
    if crate::ai::switch::tick(app, now) {
        app.dirty = true;
    }
}

/// What [`step`] tells its caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepOutcome {
    /// A command set `app.quit`: the caller ends its loop and runs [`exit`].
    pub quit: bool,
    /// Call [`step`] again within this time even without events: timers, watcher results and
    /// terminal output are picked up by the background pass.
    pub next_wake: Duration,
}

/// What [`step`] last gave the frontend, so it is given again only when it changes.
#[derive(Debug, Default)]
pub struct StepState {
    title: Option<String>,
    cursor: Option<CursorShape>,
    /// `None`: not given yet.
    ime: Option<Option<Rect>>,
}

impl StepState {
    /// The state after the frontend already shows `title` (the terminal sets it on entry).
    pub fn with_title(title: String) -> Self {
        Self {
            title: Some(title),
            ..Self::default()
        }
    }
}

/// Hands one input event to the app.
fn handle_event(app: &mut App, event: Event) {
    match event {
        Event::Key(k) => app.handle_key(k),
        Event::Paste(s) => app.handle_paste(&s),
        Event::Mouse(m) => app.handle_mouse(m),
        Event::Resize(..) => app.on_resize(),
        _ => {}
    }
}

/// The attention marks of AI sessions (`ai::attention::mark`), as (session serial, mark order).
fn attention_marks(app: &App) -> Vec<(u64, u64)> {
    app.ai
        .sessions
        .iter()
        .filter_map(|s| s.attention.map(|order| (s.serial, order)))
        .collect()
}

/// A mark in `after` that `before` did not have: a session asked for the user. Counting would
/// miss a mark cleared and another one added in the same pass.
fn new_mark(before: &[(u64, u64)], after: &[(u64, u64)]) -> bool {
    after.iter().any(|m| !before.contains(m))
}

/// One turn of any frontend's loop: handles `events` in order (the ones after a quit are left
/// unread, `events` is consumed lazily), runs the background pass at `now`, draws when something
/// changed and gives the frontend the title, cursor shape and IME area when they changed. The
/// app stays on the caller's thread; nothing here needs `Send`.
pub fn step<B, F>(
    terminal: &mut Terminal<B>,
    frontend: &mut F,
    state: &mut StepState,
    app: &mut App,
    events: impl IntoIterator<Item = Event>,
    now: Instant,
) -> anyhow::Result<StepOutcome>
where
    B: Backend + DrawnCursor,
    B::Error: Send + Sync + 'static,
    F: Frontend,
{
    for event in events {
        handle_event(app, event);
        if app.quit {
            break;
        }
    }
    let marked = attention_marks(app);
    background_pass(app, now);
    if new_mark(&marked, &attention_marks(app)) {
        frontend.request_attention();
    }
    if app.dirty {
        draw_frame(terminal, app, &recovery::GLOBAL, ui::render)?;
        set_cursor_shape(frontend, &mut state.cursor, cursor_style(app));
        let ime = terminal
            .backend()
            .drawn_cursor()
            .map(|p| Rect::new(p.x, p.y, 1, 1));
        if state.ime != Some(ime) {
            frontend.set_ime_area(ime);
            state.ime = Some(ime);
        }
        // A workspace switch, or roots added or removed, redraw: the title follows them.
        set_title(frontend, &mut state.title, app);
    }
    Ok(StepOutcome {
        quit: app.quit,
        next_wake: poll_interval(app),
    })
}

/// The terminal loop: `poll → read → step` until a command sets `app.quit`. `main` set the
/// first title on entry.
pub fn run<B, F>(terminal: &mut Terminal<B>, frontend: &mut F, app: &mut App) -> anyhow::Result<()>
where
    B: Backend + DrawnCursor,
    B::Error: Send + Sync + 'static,
    F: Frontend,
{
    let mut state = StepState::with_title(app.title());
    let mut out = step(terminal, frontend, &mut state, app, [], Instant::now())?;
    while !out.quit {
        if !event::poll(out.next_wake)? {
            out = step(terminal, frontend, &mut state, app, [], Instant::now())?;
            continue;
        }
        // Handle everything already queued before drawing again (fast typing, pastes).
        let mut failed = None;
        let mut first = true;
        let events = std::iter::from_fn(|| {
            if !std::mem::take(&mut first) {
                match event::poll(Duration::ZERO) {
                    Ok(true) => {}
                    Ok(false) => return None,
                    Err(e) => {
                        failed = Some(e);
                        return None;
                    }
                }
            }
            event::read().map_err(|e| failed = Some(e)).ok()
        });
        let result = step(terminal, frontend, &mut state, app, events, Instant::now());
        if let Some(e) = failed {
            return Err(e.into());
        }
        out = result?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyCode as K;

    fn chord(code: K, mods: KeyModifiers, enhanced: bool) -> String {
        chord_from_key(&KeyEvent::new(code, mods), enhanced)
            .unwrap()
            .to_string()
    }

    #[test]
    fn edits_are_published_before_a_frame_is_drawn() {
        let _g = recovery::tests::HOOK_TEST
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let (_d, _p, mut app, _) = crate::app_tests::app_with_file("a\n");
        app.handle_key(KeyEvent::new(K::Char('x'), KeyModifiers::NONE));
        let state = recovery::RecoveryState::new();
        let mut t = Terminal::new(ratatui::backend::TestBackend::new(20, 5)).unwrap();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            draw_frame(&mut t, &mut app, &state, |_, _| {
                panic!("render failed (expected by the test)")
            })
        }));
        assert!(r.is_err());
        let s = state.snapshot();
        assert_eq!(s.len(), 1, "the last edits are not in recovery");
        assert_eq!(s[0].text.to_string(), "xa\n");
    }

    #[test]
    fn loop_error_writes_unsaved_buffers_to_recovery() {
        let (_d, _p, mut app, _) = crate::app_tests::app_with_file("a\n");
        app.handle_key(KeyEvent::new(K::Char('x'), KeyModifiers::NONE));
        let state = tempfile::tempdir().unwrap();
        let dir = state.path().join("recovery");
        let r = finish(Err(anyhow::anyhow!("write to terminal: EIO")), &app, &dir);
        let msg = format!("{:#}", r.unwrap_err());
        let files: Vec<_> =
            std::fs::read_dir(&dir).map_or(Vec::new(), |d| d.map(|e| e.unwrap().path()).collect());
        assert_eq!(files.len(), 1, "{msg}");
        assert_eq!(std::fs::read_to_string(&files[0]).unwrap(), "xa\n");
        assert!(
            msg.contains("EIO") && msg.contains(&files[0].display().to_string()),
            "{msg}"
        );
        assert!(finish(Ok(()), &app, &dir).is_ok());
    }

    /// A replace worker blocked in I/O at exit: the terminal is restored and the unsaved
    /// buffers are in recovery before the bounded wait starts, and the exit still ends.
    #[test]
    fn exit_restores_and_writes_recovery_before_waiting_for_a_replace() {
        use crate::commands::search::{JobKind, tests::stuck_job};
        use std::time::SystemTime;
        let (_d, _p, mut app, _) = crate::app_tests::app_with_file("a\n");
        app.handle_key(KeyEvent::new(K::Char('x'), KeyModifiers::NONE));
        stuck_job(&mut app, JobKind::Apply);
        let state = tempfile::tempdir().unwrap();
        let dir = state.path().join("recovery");
        let (t0, wall0) = (Instant::now(), SystemTime::now());
        let mut restored = None;
        let wait = Duration::from_millis(500);
        let r = exit(
            Err(anyhow::anyhow!("write to terminal: EIO")),
            &mut app,
            &dir,
            &mut || restored = Some(t0.elapsed()),
            wait,
        );
        let took = t0.elapsed();
        assert!(took >= wait && took < Duration::from_secs(2), "{took:?}");
        let restored = restored.expect("terminal restored");
        assert!(
            restored < Duration::from_millis(200),
            "restored at {restored:?}"
        );
        let files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(files.len(), 1);
        let written = std::fs::metadata(&files[0]).unwrap().modified().unwrap();
        let after = written.duration_since(wall0).unwrap_or_default();
        assert!(
            after < Duration::from_millis(200),
            "recovery written {after:?} after the start"
        );
        assert!(format!("{:#}", r.unwrap_err()).contains("unsaved buffers written"));
        let msgs = app.exit_messages.join(" | ");
        assert!(msgs.contains("still writing b"), "{msgs}");
    }

    /// FR-1 quick pass: a window that outlived its terminal (SIGHUP ignored) exits with a
    /// stderr that fails every write; the exit messages are dropped and the sequence still
    /// runs to the end (the replace wait included) instead of panicking. The child's stderr is
    /// a pipe whose read end is closed before it writes (EPIPE, as EIO on a hung-up tty).
    #[cfg(unix)]
    #[test]
    fn exit_finishes_when_stderr_is_gone() {
        use std::io::Write;
        const CHILD: &str = "VR_IDE_TEST_DEAD_STDERR_CHILD";
        if std::env::var_os(CHILD).is_some() {
            use crate::commands::search::{JobKind, tests::stuck_job};
            let (_d, _p, mut app, _) = crate::app_tests::app_with_file("a\n");
            app.handle_key(KeyEvent::new(K::Char('x'), KeyModifiers::NONE));
            stuck_job(&mut app, JobKind::Apply);
            app.exit_messages.push("a message nobody can read".into());
            // Let the parent close the pipe first.
            std::thread::sleep(Duration::from_millis(300));
            let state = tempfile::tempdir().unwrap();
            let dir = state.path().join("recovery");
            let t0 = Instant::now();
            let _ = exit(
                Err(anyhow::anyhow!("the connection to the display was lost")),
                &mut app,
                &dir,
                &mut || {},
                Duration::from_millis(300),
            );
            let mut out = std::io::stdout();
            let _ = writeln!(
                out,
                "exit finished, waited {}",
                t0.elapsed().as_millis() >= 300
            );
            let _ = out.flush();
            return;
        }
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "event_loop::tests::exit_finishes_when_stderr_is_gone",
                "--test-threads=1",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        drop(child.stderr.take());
        let out = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(
            stdout.contains("exit finished, waited true"),
            "{stdout:?} {:?}",
            out.status
        );
        assert!(out.status.success(), "{:?}", out.status);
    }

    /// The window title is written again only when the workspace label changed (after a
    /// switch), never with the control characters a folder name may hold.
    #[test]
    fn the_window_title_follows_a_workspace_switch() {
        let (_d, _p, mut app, _) = crate::app_tests::app_with_file("a\n");
        let mut last = Some(app.title());
        assert_eq!(changed_title(last.as_deref(), &app), None, "unchanged");
        let other = tempfile::tempdir().unwrap();
        let odd = other.path().join("x\u{1b}]0;evil\u{7}y");
        std::fs::create_dir(&odd).unwrap();
        app.open_workspace(crate::workspace::Workspace::single(odd));
        let t = changed_title(last.as_deref(), &app).expect("written after the switch");
        assert!(t.starts_with("vr-ide — "), "{t}");
        assert!(t.ends_with("x?]0;evil?y"), "{t:?}");
        assert!(!t.chars().any(char::is_control), "{t:?}");
        last = Some(t);
        assert_eq!(changed_title(last.as_deref(), &app), None, "written once");
        assert_eq!(changed_title(None, &app).as_deref(), last.as_deref());
    }

    #[test]
    fn ctrl_s_and_ctrl_q_map_to_their_chords() {
        // Raw mode turns off XON/XOFF and ISIG, so these arrive as plain control keys.
        assert_eq!(chord(K::Char('s'), KeyModifiers::CONTROL, false), "ctrl+s");
        assert_eq!(chord(K::Char('q'), KeyModifiers::CONTROL, false), "ctrl+q");
        assert_eq!(chord(K::Char('c'), KeyModifiers::CONTROL, true), "ctrl+c");
    }

    #[test]
    fn legacy_control_bytes_map_back_to_their_keys() {
        assert_eq!(chord(K::Char('5'), KeyModifiers::CONTROL, false), "ctrl+]");
        assert_eq!(chord(K::Char('7'), KeyModifiers::CONTROL, false), "ctrl+/");
        // With the kitty protocol ctrl+5 really is ctrl+5.
        assert_eq!(chord(K::Char('5'), KeyModifiers::CONTROL, true), "ctrl+5");
        assert_eq!(chord(K::Char('/'), KeyModifiers::CONTROL, true), "ctrl+/");
    }

    #[test]
    fn shifted_symbols_with_ctrl_or_alt_name_the_shift_even_when_the_terminal_drops_it() {
        let c = KeyModifiers::CONTROL;
        let a = KeyModifiers::ALT;
        assert_eq!(chord(K::Char('!'), c, true), "ctrl+shift+1");
        assert_eq!(chord(K::Char('?'), c, true), "ctrl+shift+/");
        assert_eq!(chord(K::Char('!'), a, true), "shift+alt+1");
        assert_eq!(chord(K::Char('~'), c | a, true), "ctrl+shift+alt+`");
        // The shapes the window sends (SHIFT kept) name the same chords.
        let s = KeyModifiers::SHIFT;
        assert_eq!(chord(K::Char('~'), c | a | s, true), "ctrl+shift+alt+`");
        assert_eq!(chord(K::Char('!'), a | s, true), "shift+alt+1");
        assert_eq!(chord(K::Char('?'), c | s, true), "ctrl+shift+/");
        assert_eq!(chord(K::Char(' '), s, true), "shift+space");
        assert_eq!(chord(K::Char('?'), KeyModifiers::NONE, true), "?");
    }

    #[test]
    fn shifted_keys_use_the_unshifted_name() {
        let cs = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        assert_eq!(chord(K::Char('P'), cs, true), "ctrl+shift+p");
        assert_eq!(chord(K::Char('!'), cs, true), "ctrl+shift+1");
        assert_eq!(chord(K::BackTab, KeyModifiers::SHIFT, false), "shift+tab");
        assert_eq!(chord(K::F(1), KeyModifiers::NONE, false), "f1");
        assert_eq!(
            chord(K::Down, KeyModifiers::SHIFT | KeyModifiers::ALT, false),
            "shift+alt+down"
        );
    }

    #[test]
    fn legacy_unreachable_chords() {
        let u = |s: &str| legacy_unreachable(&s.parse().unwrap());
        assert!(u("ctrl+shift+p") && u("ctrl+[") && u("ctrl+alt+enter") && u("ctrl+shift+1"));
        assert!(!u("ctrl+s") && !u("ctrl+]") && !u("ctrl+/") && !u("f1") && !u("shift+tab"));
        assert!(!u("alt+up") && !u("ctrl+pagedown"));
        // ctrl+` arrives as NUL, which stands for it outside a focused terminal (D185).
        assert!(!u("ctrl+`") && u("ctrl+shift+`") && u("ctrl+alt+`") && !u("ctrl+alt+f"));
    }

    /// AC-14a: a legacy NUL (ctrl+space) maps to that chord; the app treats it as ctrl+`.
    #[test]
    fn legacy_nul_is_ctrl_space() {
        assert_eq!(
            chord(K::Char(' '), KeyModifiers::CONTROL, false),
            "ctrl+space"
        );
        let cs = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        assert_eq!(chord(K::Char('~'), cs, true), "ctrl+shift+`");
        assert_eq!(chord(K::Char('`'), KeyModifiers::CONTROL, true), "ctrl+`");
    }

    use crate::frontend::tests::FakeFrontend;

    type TestTerminal = Terminal<ratatui::backend::TestBackend>;

    fn test_terminal() -> TestTerminal {
        Terminal::new(ratatui::backend::TestBackend::new(60, 12)).unwrap()
    }

    fn typed(c: char) -> Event {
        Event::Key(KeyEvent::new(K::Char(c), KeyModifiers::NONE))
    }

    #[test]
    fn step_handles_events_in_order_then_draws() {
        let (_d, _p, mut app, _) = crate::app_tests::app_with_file("a\n");
        let (mut t, mut f, mut s) = (
            test_terminal(),
            FakeFrontend::default(),
            StepState::default(),
        );
        let events = [typed('x'), typed('y'), Event::Resize(60, 12), typed('z')];
        let out = step(&mut t, &mut f, &mut s, &mut app, events, Instant::now()).unwrap();
        assert_eq!(app.docs[0].buffer.text().to_string(), "xyza\n");
        assert!(!out.quit);
        assert!(!app.dirty, "drawn");
        let screen = format!("{:?}", t.backend().buffer());
        assert!(screen.contains("xyza"), "{screen}");
    }

    /// The events after the one that quits stay unread (the terminal loop reads lazily).
    #[test]
    fn step_stops_reading_events_at_a_quit() {
        let (_d, _p, mut app, _) = crate::app_tests::app_with_file("a\n");
        let (mut t, mut f, mut s) = (
            test_terminal(),
            FakeFrontend::default(),
            StepState::default(),
        );
        let quit = Event::Key(KeyEvent::new(K::Char('q'), KeyModifiers::CONTROL));
        let mut read = 0;
        let events = [quit, typed('x')].into_iter().inspect(|_| read += 1);
        let out = step(&mut t, &mut f, &mut s, &mut app, events, Instant::now()).unwrap();
        assert!(out.quit && app.quit);
        assert_eq!(read, 1, "the event after the quit was read");
        assert_eq!(app.docs[0].buffer.text().to_string(), "a\n");
    }

    #[test]
    fn step_reports_the_next_wake_from_the_poll_interval() {
        let (_d, _p, mut app, _) = crate::app_tests::app_with_file("a\n");
        let (mut t, mut f, mut s) = (
            test_terminal(),
            FakeFrontend::default(),
            StepState::default(),
        );
        let out = step(&mut t, &mut f, &mut s, &mut app, [], Instant::now()).unwrap();
        assert_eq!(out.next_wake, POLL, "no terminal is running");
        assert_eq!(out.next_wake, poll_interval(&app));
        assert!(!out.quit);
    }

    /// Title, cursor shape and IME area go to the frontend on the first draw and again only
    /// when they change; a failed title write is tried again on the next draw.
    #[test]
    fn step_gives_the_frontend_only_changed_values() {
        let (_d, _p, mut app, _) = crate::app_tests::app_with_file("a\n");
        let (mut t, mut f, mut s) = (
            test_terminal(),
            FakeFrontend::default(),
            StepState::default(),
        );
        step(&mut t, &mut f, &mut s, &mut app, [], Instant::now()).unwrap();
        assert_eq!(f.titles, [app.title()]);
        assert_eq!(
            f.shapes,
            [CursorShape::BlinkingBar],
            "edit mode types into text"
        );
        let cursor = t
            .backend()
            .drawn_cursor()
            .expect("the editor shows its cursor");
        assert_eq!(f.ime, [Some(Rect::new(cursor.x, cursor.y, 1, 1))]);

        // Redrawn with nothing changed for the frontend.
        app.dirty = true;
        step(&mut t, &mut f, &mut s, &mut app, [], Instant::now()).unwrap();
        assert_eq!((f.titles.len(), f.shapes.len(), f.ime.len()), (1, 1, 1));

        // Moving the cursor one cell: only the IME area follows.
        let right = Event::Key(KeyEvent::new(K::Right, KeyModifiers::NONE));
        step(&mut t, &mut f, &mut s, &mut app, [right], Instant::now()).unwrap();
        assert_eq!((f.titles.len(), f.shapes.len()), (1, 1));
        assert_eq!(
            f.ime.last(),
            Some(&Some(Rect::new(cursor.x + 1, cursor.y, 1, 1)))
        );

        // A workspace switch changes the title; the first write fails and is retried.
        let other = tempfile::tempdir().unwrap();
        app.open_workspace(crate::workspace::Workspace::single(
            other.path().to_path_buf(),
        ));
        f.fail_title = true;
        app.dirty = true;
        step(&mut t, &mut f, &mut s, &mut app, [], Instant::now()).unwrap();
        assert_eq!(f.titles.len(), 1);
        f.fail_title = false;
        app.dirty = true;
        step(&mut t, &mut f, &mut s, &mut app, [], Instant::now()).unwrap();
        assert_eq!(f.titles.len(), 2, "{:?} now {}", f.titles, app.title());
        assert_eq!(f.titles[1], app.title());
        app.dirty = true;
        step(&mut t, &mut f, &mut s, &mut app, [], Instant::now()).unwrap();
        assert_eq!(f.titles.len(), 2, "written once");
    }

    /// The terminal sets the first title on entry; the loop does not write it again.
    #[test]
    fn a_title_already_shown_is_not_written_again() {
        let (_d, _p, mut app, _) = crate::app_tests::app_with_file("a\n");
        let (mut t, mut f) = (test_terminal(), FakeFrontend::default());
        let mut s = StepState::with_title(app.title());
        step(&mut t, &mut f, &mut s, &mut app, [], Instant::now()).unwrap();
        assert!(f.titles.is_empty());
        assert_eq!(f.attention, 0);
    }

    /// One mark cleared and another added in the same pass still asks for attention; marks
    /// only cleared, or unchanged, do not.
    #[test]
    fn a_new_mark_is_seen_even_when_another_was_cleared() {
        assert!(new_mark(&[(1, 0)], &[(2, 0)]), "cleared one, added one");
        assert!(new_mark(&[], &[(1, 0)]));
        assert!(new_mark(&[(1, 0)], &[(1, 0), (2, 1)]));
        assert!(!new_mark(&[(1, 0), (2, 1)], &[(2, 1)]), "only cleared");
        assert!(!new_mark(&[(1, 0)], &[(1, 0)]), "unchanged");
        assert!(!new_mark(&[], &[]));
    }
}
