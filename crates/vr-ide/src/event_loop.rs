//! The terminal event loop and the crossterm → `KeyChord` conversion.

use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::Backend;
use vr_config::{KeyChord, Mods};

use crate::app::App;
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
            // Keymaps name the unshifted key ("ctrl+shift+1"); terminals report the shifted
            // symbol ('!'). US layout, the one VS Code's default keys assume.
            if mods.shift && (mods.ctrl || mods.alt) {
                c = unshift_us(c);
            }
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

fn unshift_us(c: char) -> char {
    const SHIFTED: &str = "!@#$%^&*()_+{}|:\"<>?~";
    const BASE: &str = "1234567890-=[]\\;',./`";
    SHIFTED
        .chars()
        .position(|s| s == c)
        .and_then(|i| BASE.chars().nth(i))
        .unwrap_or(c)
}

/// Chords a terminal without keyboard enhancement cannot deliver as themselves.
/// Legacy encoding has one control byte per letter (shift is lost), only `[ \ ] /` and space
/// among the other keys (ctrl+[ is Escape itself), and no modified Enter or Tab.
pub fn legacy_unreachable(chord: &KeyChord) -> bool {
    use vr_config::KeyCode as C;
    let m = chord.mods;
    match chord.code {
        C::Char(c) if m.ctrl => {
            (m.shift && !(c == ' '))
                || !(c.is_ascii_lowercase() || matches!(c, ' ' | '\\' | ']' | '/'))
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
    state.publish(&app.docs);
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
    let written = recovery::write_docs(&docs, dir);
    if written.is_empty() {
        return Err(e);
    }
    let list: Vec<String> = written.iter().map(|p| p.display().to_string()).collect();
    Err(e.context(format!("unsaved buffers written to {}", list.join(", "))))
}

/// Runs until a command sets `app.quit`. Draws only when something changed.
pub fn run<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> anyhow::Result<()>
where
    B::Error: Send + Sync + 'static,
{
    loop {
        app.drain_fs_events();
        app.drain_index();
        app.tick(Instant::now());
        if app.dirty {
            draw_frame(terminal, app, &recovery::GLOBAL, ui::render)?;
        }
        if app.quit {
            return Ok(());
        }
        if event::poll(POLL)? {
            // Handle everything already queued before drawing again (fast typing, pastes).
            loop {
                match event::read()? {
                    Event::Key(k) => app.handle_key(k),
                    Event::Paste(s) => app.handle_paste(&s),
                    Event::Mouse(m) => app.handle_mouse(m),
                    Event::Resize(..) => app.cursor_moved(),
                    _ => {}
                }
                if app.quit || !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
    }
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
    }
}
