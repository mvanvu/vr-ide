//! X11 only: libX11 ends the process (`exit(1)`) when the server connection breaks, inside
//! winit and before `run_app` can return an error, so the app's own exit never runs. libX11
//! calls an IO error handler first; the hook registered here runs in it, so the app can still
//! write its recovery files. libX11 exits after the handler returns, as before.

use std::os::raw::c_int;
use std::sync::OnceLock;

use x11_dl::xlib::{Display, Xlib};

static HOOK: OnceLock<fn()> = OnceLock::new();

/// Registers the hook (once per process); `false` when one was registered before.
pub fn set_hook(hook: fn()) -> bool {
    HOOK.set(hook).is_ok()
}

/// Installs the IO error handler; call after winit opened its X11 connection.
pub(super) fn install() {
    let Ok(xlib) = Xlib::open() else {
        return;
    };
    // SAFETY: a plain libX11 call on the main thread; the handler is a valid `extern "C"` fn
    // for the whole run.
    unsafe {
        (xlib.XSetIOErrorHandler)(Some(io_error));
    }
    // Dropping `xlib` does not unload libX11: x11-dl keeps the library in a process-wide cache.
}

/// libX11's IO error handler: runs the hook, never unwinds into C.
pub(super) extern "C" fn io_error(_: *mut Display) -> c_int {
    let _ = std::panic::catch_unwind(|| {
        if let Some(hook) = HOOK.get() {
            hook();
        }
    });
    0
}
