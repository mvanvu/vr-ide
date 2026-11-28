use super::*;

fn cell(width: u32, height: u32) -> CellMetrics {
    CellMetrics {
        width,
        height,
        baseline: 10,
        underline: 12,
        strike: 7,
        line: 1,
    }
}

fn layout() -> GridLayout {
    GridLayout {
        cols: 10,
        rows: 4,
        origin_x: 2,
        origin_y: 3,
        cell: cell(8, 16),
    }
}

#[test]
fn only_damage_is_copied_when_the_buffer_holds_a_recent_frame() {
    let now = [PixelRect::new(0, 0, 10, 16)];
    let last = [PixelRect::new(0, 32, 10, 16)];
    assert_eq!(copy_plan(1, &now, &last), Some(now.to_vec()));
    assert_eq!(
        copy_plan(2, &now, &last),
        Some(vec![now[0], last[0]]),
        "a buffer from the frame before last misses the last frame's damage too"
    );
    assert_eq!(
        copy_plan(0, &now, &last),
        None,
        "unknown contents: full copy"
    );
    assert_eq!(copy_plan(3, &now, &last), None);
    assert_eq!(copy_plan(1, &[], &last), Some(Vec::new()));
}

#[test]
fn the_first_window_size_holds_the_grid_at_scale_one() {
    let size = initial_size(cell(9, 19), 120, 36);
    assert_eq!((size.width, size.height), (1080, 684));
    let degenerate = initial_size(cell(0, 0), 2, 3);
    assert_eq!((degenerate.width, degenerate.height), (2, 3));
}

#[test]
fn the_ime_area_is_the_cursor_cell_in_pixels() {
    let l = layout();
    assert_eq!(ime_area(&l, 0, 0), PixelRect::new(2, 3, 8, 16));
    assert_eq!(ime_area(&l, 3, 2), PixelRect::new(2 + 24, 3 + 32, 8, 16));
    // A cursor past the last column stays on the grid.
    assert_eq!(ime_area(&l, 50, 1), PixelRect::new(2 + 72, 3 + 16, 8, 16));
}

#[test]
fn a_blinking_cursor_flips_and_input_shows_it_again() {
    let t0 = Instant::now();
    let mut b = Blink::default();
    b.set(false, t0);
    assert!(b.on && b.next().is_none(), "steady: shown, no wake");

    b.set(true, t0);
    assert_eq!(b.next(), Some(t0 + BLINK));
    b.tick(t0 + BLINK / 2);
    assert!(b.on, "not yet");
    b.tick(t0 + BLINK);
    assert!(!b.on, "hidden after one period");
    assert_eq!(b.next(), Some(t0 + BLINK * 2));
    b.set(true, t0 + BLINK);
    assert_eq!(b.next(), Some(t0 + BLINK * 2), "set again keeps the phase");

    let typed = t0 + BLINK + BLINK / 4;
    b.reset(typed);
    assert!(b.on, "input shows the cursor");
    assert_eq!(b.next(), Some(typed + BLINK));

    b.tick(typed + BLINK);
    assert!(!b.on);
    b.set(false, typed + BLINK);
    assert!(b.on && b.next().is_none(), "a steady shape is never hidden");
}

/// Records what the runner tells the app at the end.
#[derive(Default)]
struct EndHandler {
    failed: Vec<String>,
}

impl WindowHandler for EndHandler {
    fn step(
        &mut self,
        _: &mut Terminal<PixelBackend>,
        _: &mut dyn WindowOps,
        _: Vec<Event>,
        _: Instant,
    ) -> Step {
        unreachable!("no window in this test")
    }

    fn close_requested(&mut self) {}

    fn failed(&mut self, error: String) {
        self.failed.push(error);
    }
}

fn runner(handler: &mut EndHandler, started: bool) -> Runner<'_, EndHandler> {
    Runner {
        handler,
        options: None,
        live: None,
        start_error: None,
        started,
    }
}

/// Review blocker 1: the display going away after the window showed (Wayland flush/dispatch
/// error, X11 reset) ends the session through the app's own exit, never as a start failure
/// that would send it to the terminal.
#[test]
fn a_display_lost_after_the_start_ends_through_the_app_not_as_a_start_error() {
    let mut h = EndHandler::default();
    let lost = Err(winit::error::EventLoopError::ExitFailure(1));
    assert!(runner(&mut h, true).finish(lost).is_ok());
    assert_eq!(h.failed.len(), 1, "the app is told once: {:?}", h.failed);
    assert!(h.failed[0].contains("display"), "{:?}", h.failed);

    let mut h = EndHandler::default();
    let lost = Err(winit::error::EventLoopError::ExitFailure(1));
    assert!(
        matches!(
            runner(&mut h, false).finish(lost),
            Err(StartError::EventLoop(_))
        ),
        "before the start: the caller may still use the terminal"
    );
    assert!(h.failed.is_empty());

    let mut h = EndHandler::default();
    let mut r = runner(&mut h, false);
    r.start_error = Some(StartError::Surface("no shm".into()));
    assert!(matches!(r.finish(Ok(())), Err(StartError::Surface(_))));
    assert!(runner(&mut h, true).finish(Ok(())).is_ok());
    assert!(h.failed.is_empty());
}

/// Review major 2: the candidate window follows the cursor after anything that makes the IME
/// start over (focus, IME enabled, Esc cancelling a composition), not only when the cell moves.
#[test]
fn the_ime_area_is_sent_again_after_the_ime_starts_over() {
    let l = layout();
    let mut p = ImePlacement::default();
    assert_eq!(p.take(l), None, "no cursor yet");
    p.set_cell(Some((3, 2)));
    let area = ime_area(&l, 3, 2);
    assert_eq!(p.take(l), Some(area));
    assert_eq!(p.take(l), None, "the window has it");

    p.reset();
    assert_eq!(
        p.take(l),
        Some(area),
        "after a reset the area goes out again"
    );
    assert_eq!(p.take(l), None);

    let wider = GridLayout {
        cell: cell(10, 20),
        ..l
    };
    assert_eq!(p.take(wider), Some(ime_area(&wider, 3, 2)), "new layout");

    p.set_cell(None);
    p.reset();
    assert_eq!(p.take(wider), None, "no cursor shown: nothing to place");
}

#[test]
fn focus_and_ime_enable_start_the_ime_over() {
    use winit::event::Ime;
    assert!(resets_ime(&WindowEvent::Focused(true)));
    assert!(resets_ime(&WindowEvent::Ime(Ime::Enabled)));
    assert!(!resets_ime(&WindowEvent::Focused(false)));
    assert!(!resets_ime(&WindowEvent::Ime(Ime::Commit("a".into()))));
    assert!(!resets_ime(&WindowEvent::Ime(Ime::Disabled)));
}

/// Review (round 2): on X11 libX11 exits the process on a lost server; the registered hook
/// runs first, from the IO error handler, and nothing unwinds into C.
#[cfg(all(
    unix,
    not(any(target_os = "macos", target_os = "ios", target_os = "android"))
))]
#[test]
fn a_lost_x11_connection_runs_the_registered_hook_first() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    fn hook() {
        CALLS.fetch_add(1, Ordering::SeqCst);
    }
    assert!(on_x11_connection_lost(hook));
    assert!(!on_x11_connection_lost(hook), "once per process");
    assert_eq!(super::x11::io_error(std::ptr::null_mut()), 0);
    assert_eq!(CALLS.load(Ordering::SeqCst), 1, "the hook ran");
}
