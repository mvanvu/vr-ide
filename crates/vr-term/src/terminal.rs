//! A shell in a PTY with an emulator fed by a reader thread.
//!
//! Threads per terminal (they end when the PTY is hung up, and at the latest when `Drop` fires
//! the wake eventfd, even if a process outside the child's group still holds the slave):
//! - reader: reads its own non-blocking copy of the PTY master, feeds the emulator in small
//!   locked slices, raises `Notify`;
//! - writer: writes queued input (keys, pastes, emulator replies; at most
//!   [`MAX_PENDING_INPUT`] bytes queued) so neither the UI nor the reader ever blocks on a
//!   program that does not read its input;
//! - waiter: waits for the child without reaping it, then reaps it under the status lock so a
//!   process-group signal can never reach a recycled pid.

use std::os::fd::{BorrowedFd, OwnedFd};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::term::{Config, Osc52, Term};
use alacritty_terminal::vte::ansi::Processor;
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};
use rustix::event::{PollFd, PollFlags, poll};
use rustix::io::Errno;

use crate::attention::{Attention, AttentionScan};
use crate::marker::MarkerScan;
use crate::snapshot;
use crate::types::{Grid, Notify, SpawnExtras, SpawnOptions, TermError, TermStatus};

/// How long a child gets to exit after SIGHUP before the group gets SIGKILL, and again after it.
pub const KILL_GRACE: Duration = Duration::from_millis(300);
/// How long `Drop` waits for the helper threads after the child is gone before detaching them.
const JOIN_WAIT: Duration = Duration::from_millis(200);
/// Bytes read from the PTY per syscall.
const READ_BUF: usize = 64 * 1024;
/// Bytes parsed per emulator lock, so a snapshot never waits long behind a flood of output.
const PARSE_SLICE: usize = 4 * 1024;
/// Input queued for a program that is not reading it; more is refused rather than buffered.
pub const MAX_PENDING_INPUT: usize = 1024 * 1024;

/// Variables that describe the terminal vr-ide itself runs in; a shell in vr-ide's terminal must
/// not see them (`tmux` would refuse to start as "nested", programs would size themselves to
/// the wrong window or pick features of the wrong emulator).
const OUTER_TERMINAL_ENV: &[&str] = &[
    "TMUX",
    "TMUX_PANE",
    "STY",
    "WINDOW",
    "TERM_PROGRAM",
    "TERM_PROGRAM_VERSION",
    "TERM_SESSION_ID",
    "TERMINAL_EMULATOR",
    "VTE_VERSION",
    "COLUMNS",
    "LINES",
    "WINDOWID",
    "KITTY_WINDOW_ID",
    "KITTY_PID",
    "KITTY_PUBLIC_KEY",
    "KITTY_LISTEN_ON",
    "KITTY_INSTALLATION_DIR",
    "WEZTERM_PANE",
    "WEZTERM_UNIX_SOCKET",
    "WEZTERM_EXECUTABLE",
    "WEZTERM_EXECUTABLE_DIR",
    "WEZTERM_CONFIG_DIR",
    "WEZTERM_CONFIG_FILE",
    "ALACRITTY_WINDOW_ID",
    "ALACRITTY_SOCKET",
    "ALACRITTY_LOG",
    "KONSOLE_VERSION",
    "KONSOLE_DBUS_SESSION",
    "KONSOLE_DBUS_SERVICE",
    "KONSOLE_DBUS_WINDOW",
    "SHELL_SESSION_ID",
    "GNOME_TERMINAL_SCREEN",
    "GNOME_TERMINAL_SERVICE",
    "TERMINATOR_UUID",
    "TERMINATOR_DBUS_NAME",
    "TERMINATOR_DBUS_PATH",
    "TILIX_ID",
    "GHOSTTY_RESOURCES_DIR",
    "GHOSTTY_BIN_DIR",
    "GHOSTTY_SHELL_FEATURES",
    "WT_SESSION",
    "WT_PROFILE_ID",
    "ITERM_SESSION_ID",
    "ITERM_PROFILE",
    "LC_TERMINAL",
    "LC_TERMINAL_VERSION",
    "VSCODE_INJECTION",
    "VSCODE_GIT_IPC_HANDLE",
];

/// Adds `len` to the queued-input count unless that passes [`MAX_PENDING_INPUT`]; on refusal
/// returns the count as it was.
fn reserve(pending: &AtomicUsize, len: usize) -> Result<(), usize> {
    pending
        .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
            (n + len <= MAX_PENDING_INPUT).then_some(n + len)
        })
        .map(|_| ())
}

pub(crate) struct Size {
    pub cols: usize,
    pub rows: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn columns(&self) -> usize {
        self.cols
    }
}

enum Input {
    Bytes(Vec<u8>),
    Stop,
}

/// Receives the emulator's events on the reader thread (inside the emulator lock).
pub(crate) struct Listener {
    title: Arc<Mutex<Option<String>>>,
    input: Sender<Input>,
    pending: Arc<AtomicUsize>,
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            Event::Title(title) => *lock(&self.title) = Some(title),
            Event::ResetTitle => *lock(&self.title) = None,
            // Replies to queries (cursor position, device attributes) go through the input queue:
            // writing here could block the reader with the emulator locked.
            // Over the cap the reply is dropped: a program that leaves its input unread cannot use
            // the answer, and queueing it would let printed queries grow memory without bound.
            Event::PtyWrite(text) if reserve(&self.pending, text.len()).is_ok() => {
                let _ = self.input.send(Input::Bytes(text.into_bytes()));
            }
            // Clipboard (OSC 52 is disabled), colour and pixel-size queries are not answered;
            // programs time out on them, as they do in terminals that lack them.
            _ => {}
        }
    }
}

/// Events kept for the UI between two takes.
const MAX_ATTENTION: usize = 64;

struct Shared {
    term: Mutex<Term<Listener>>,
    /// Snapshots waiting for the emulator lock; the reader steps aside while this is non-zero.
    snapshot_waiting: AtomicUsize,
    title: Arc<Mutex<Option<String>>>,
    status: Mutex<TermStatus>,
    status_changed: Condvar,
    /// Output (or an exit) arrived since the last snapshot; `Notify` fires only on the
    /// false → true edge.
    dirty: AtomicBool,
    /// Set by `Drop`: no more `Notify` calls for a terminal the UI already let go.
    closed: AtomicBool,
    /// The session's marker came through its own output while its child ran (sticky).
    marker_seen: AtomicBool,
    /// Scan the output for bells and notifications (AI sessions only).
    watch_attention: bool,
    /// Bells and notifications since the UI last took them (at most [`MAX_ATTENTION`]).
    attention: Mutex<Vec<Attention>>,
    notify: Notify,
}

impl Shared {
    /// Queues events and wakes the UI on the empty to non-empty edge: the output wake is
    /// coalesced and a hidden panel never takes a snapshot, which re-arms it.
    fn push_attention(&self, found: &mut Vec<Attention>) {
        let first = {
            let mut q = lock(&self.attention);
            let was_empty = q.is_empty();
            let room = MAX_ATTENTION.saturating_sub(q.len());
            q.extend(found.drain(..).take(room));
            was_empty && !q.is_empty()
        };
        if first && !self.closed.load(Ordering::SeqCst) {
            (self.notify)();
        }
    }

    fn mark_dirty(&self) {
        if !self.dirty.swap(true, Ordering::SeqCst) && !self.closed.load(Ordering::SeqCst) {
            (self.notify)();
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic on a helper thread must not take the UI down with it; the data stays usable.
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A running (or exited) program in a PTY.
pub struct Terminal {
    shared: Arc<Shared>,
    master: Option<Box<dyn MasterPty + Send>>,
    input: Sender<Input>,
    pending: Arc<AtomicUsize>,
    /// The child's pid, which is also its process group and session id (portable-pty `setsid`s).
    pid: Option<rustix::process::Pid>,
    size: Mutex<(u16, u16)>,
    threads: Vec<JoinHandle<()>>,
    /// Disconnects once every helper thread has ended.
    alive: Receiver<()>,
    /// Made readable by `Drop` so the reader and writer leave their polls and close their
    /// master fds even when something outside our process group still holds the slave.
    wake: Arc<OwnedFd>,
}

impl std::fmt::Debug for Terminal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Terminal")
            .field("pid", &self.pid())
            .field("status", &self.status())
            .finish_non_exhaustive()
    }
}

impl Terminal {
    /// Starts `opts.shell` in a new PTY session with `TERM=xterm-256color`; `COLORTERM`
    /// is left as inherited (the caller sets it through `opts.env`). `notify` is called (coalesced) whenever there is something new to draw.
    pub fn spawn(opts: SpawnOptions, notify: Notify) -> Result<Terminal, TermError> {
        Terminal::spawn_with(opts, SpawnExtras::default(), notify)
    }

    /// [`Terminal::spawn`] for a session with a marker to watch for or variables to withhold.
    pub fn spawn_with(
        opts: SpawnOptions,
        extras: SpawnExtras,
        notify: Notify,
    ) -> Result<Terminal, TermError> {
        if opts.cols == 0 || opts.rows == 0 {
            return Err(TermError::BadSize {
                cols: opts.cols,
                rows: opts.rows,
            });
        }
        // portable-pty silently falls back to $HOME for a missing directory; refuse instead.
        if !opts.cwd.is_dir() {
            return Err(TermError::BadCwd(opts.cwd.clone()));
        }
        let pair = native_pty_system()
            .openpty(pty_size(opts.cols, opts.rows, 0, 0))
            .map_err(|e| TermError::Pty(e.to_string()))?;
        let mut cmd = CommandBuilder::new(&opts.shell);
        cmd.args(&opts.args);
        cmd.cwd(&opts.cwd);
        for key in OUTER_TERMINAL_ENV {
            cmd.env_remove(key);
        }
        for key in &extras.env_remove {
            cmd.env_remove(key);
        }
        cmd.env("TERM", "xterm-256color");
        for (key, value) in &opts.env {
            cmd.env(key, value);
        }
        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| TermError::Spawn {
                shell: opts.shell.display().to_string(),
                reason: e.to_string(),
            })?;
        // Only the child may hold the slave, so the reader sees EOF once the child's side closes.
        drop(pair.slave);
        let pid = child
            .process_id()
            .and_then(|p| rustix::process::Pid::from_raw(p as i32));

        let (reader, writer, wake) = match pty_io(pair.master.as_ref()) {
            Ok(io) => io,
            Err(e) => {
                if let Some(pid) = pid {
                    let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
                }
                let _ = child.wait();
                return Err(TermError::Pty(e.to_string()));
            }
        };

        let (input_tx, input_rx) = mpsc::channel();
        let pending = Arc::new(AtomicUsize::new(0));
        let title = Arc::new(Mutex::new(None));
        let listener = Listener {
            title: title.clone(),
            input: input_tx.clone(),
            pending: pending.clone(),
        };
        let config = Config {
            scrolling_history: opts.scrollback,
            osc52: Osc52::Disabled,
            ..Config::default()
        };
        let size = Size {
            cols: usize::from(opts.cols),
            rows: usize::from(opts.rows),
        };
        let shared = Arc::new(Shared {
            term: Mutex::new(Term::new(config, &size, listener)),
            snapshot_waiting: AtomicUsize::new(0),
            title,
            status: Mutex::new(TermStatus::Running),
            status_changed: Condvar::new(),
            dirty: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            marker_seen: AtomicBool::new(false),
            watch_attention: extras.attention,
            attention: Mutex::default(),
            notify,
        });

        let (alive_tx, alive) = mpsc::channel::<()>();
        let spawn_thread = |name: &str, body: Box<dyn FnOnce() + Send>| {
            let alive = alive_tx.clone();
            thread::Builder::new().name(name.into()).spawn(move || {
                let _alive = alive;
                body();
            })
        };
        // The waiter goes first: once it runs, dropping the half-built terminal below kills and
        // reaps the child like any other terminal.
        let waiter_shared = shared.clone();
        let waiter = spawn_thread(
            "vr-term-waiter",
            Box::new(move || wait_loop(child, pid, &waiter_shared)),
        );
        let waiter = match waiter {
            Ok(handle) => handle,
            Err(e) => {
                if let Some(pid) = pid {
                    use rustix::process::{Signal, WaitOptions, kill_process_group, waitpid};
                    let _ = kill_process_group(pid, Signal::KILL);
                    let _ = waitpid(Some(pid), WaitOptions::empty());
                }
                return Err(e.into());
            }
        };
        let mut terminal = Terminal {
            shared: shared.clone(),
            master: Some(pair.master),
            input: input_tx,
            pending: pending.clone(),
            pid,
            size: Mutex::new((opts.cols, opts.rows)),
            threads: vec![waiter],
            alive,
            wake: wake.clone(),
        };
        let reader_wake = wake.clone();
        let scan = extras.marker.and_then(MarkerScan::new);
        terminal.threads.push(spawn_thread(
            "vr-term-reader",
            Box::new(move || read_loop(&reader, &reader_wake, &shared, scan)),
        )?);
        terminal.threads.push(spawn_thread(
            "vr-term-writer",
            Box::new(move || write_loop(&writer, &wake, input_rx, &pending)),
        )?);
        drop(alive_tx);
        Ok(terminal)
    }

    /// The shell itself is the terminal's foreground process group (`tcgetpgrp` on the PTY
    /// equals its pid): no program it started (an editor, a pager, an agent, `sudo`) holds the
    /// keyboard. False when that cannot be told (exited, closed, no pid).
    pub fn shell_in_foreground(&self) -> bool {
        match (&self.master, self.pid) {
            (Some(m), Some(pid)) => m.process_group_leader() == Some(pid.as_raw_nonzero().get()),
            _ => false,
        }
    }

    /// The terminal's foreground process group id (`tcgetpgrp`); `None` when it cannot be told.
    pub fn foreground_pgid(&self) -> Option<u32> {
        let pgid = self.master.as_ref()?.process_group_leader()?;
        u32::try_from(pgid).ok().filter(|p| *p > 0)
    }

    /// The child's pid (also its process group id).
    pub fn pid(&self) -> Option<u32> {
        self.pid.map(|p| p.as_raw_nonzero().get() as u32)
    }

    /// Current size as (cols, rows).
    pub fn size(&self) -> (u16, u16) {
        *lock(&self.size)
    }

    /// Bytes queued for the program (typed input and emulator replies) and not yet written.
    pub fn pending_input(&self) -> usize {
        self.pending.load(Ordering::SeqCst)
    }

    /// Queues bytes for the program (use the `encode_*` helpers). Never blocks.
    pub fn write(&self, bytes: &[u8]) -> Result<(), TermError> {
        if self.status() != TermStatus::Running {
            return Err(TermError::Closed);
        }
        if bytes.is_empty() {
            return Ok(());
        }
        reserve(&self.pending, bytes.len()).map_err(TermError::InputBacklog)?;
        self.input.send(Input::Bytes(bytes.to_vec())).map_err(|_| {
            self.pending.fetch_sub(bytes.len(), Ordering::SeqCst);
            TermError::Closed
        })
    }

    /// Resizes the emulator and the PTY (the program gets SIGWINCH); the PTY reports 0 pixels.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), TermError> {
        self.resize_with_pixels(cols, rows, 0, 0)
    }

    /// Like `resize`, and the PTY reports `cols * cell_w` by `rows * cell_h` pixels (saturating).
    pub fn resize_with_pixels(
        &self,
        cols: u16,
        rows: u16,
        cell_w: u16,
        cell_h: u16,
    ) -> Result<(), TermError> {
        if cols == 0 || rows == 0 {
            return Err(TermError::BadSize { cols, rows });
        }
        let mut size = lock(&self.size);
        // Emulator first, so what the program redraws after SIGWINCH lands on the new grid.
        lock(&self.shared.term).resize(Size {
            cols: usize::from(cols),
            rows: usize::from(rows),
        });
        if let Some(master) = &self.master {
            master
                .resize(pty_size(cols, rows, cell_w, cell_h))
                .map_err(|e| TermError::Pty(e.to_string()))?;
        }
        *size = (cols, rows);
        Ok(())
    }

    /// The marker given at spawn came through this terminal's own output while its child ran.
    /// Later occurrences, and any after the child exited, are removed without effect.
    pub fn marker_seen(&self) -> bool {
        self.shared.marker_seen.load(Ordering::SeqCst)
    }

    /// The bells and notifications (OSC 9 / 777) the program sent since the last call. The
    /// queue is bounded; a flood keeps its first events.
    pub fn take_attention(&self) -> Vec<Attention> {
        std::mem::take(&mut *lock(&self.shared.attention))
    }

    /// Running, or exited with its code once the child is reaped.
    pub fn status(&self) -> TermStatus {
        *lock(&self.shared.status)
    }

    /// Waits up to `timeout` for the child to exit; returns the status at that point.
    pub fn wait_exit(&self, timeout: Duration) -> TermStatus {
        let deadline = Instant::now() + timeout;
        let mut status = lock(&self.shared.status);
        while *status == TermStatus::Running {
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            status = self
                .shared
                .status_changed
                .wait_timeout(status, deadline - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        *status
    }

    /// Sends SIGHUP to the child's process group, then SIGKILL after [`KILL_GRACE`] if it is
    /// still running, and returns once it is reaped (or after a second grace period, so at
    /// most `2 * KILL_GRACE`). A no-op once the child has exited.
    pub fn kill(&self) {
        use rustix::process::Signal;
        for signal in [Signal::HUP, Signal::KILL] {
            if !self.signal_group(signal) {
                return;
            }
            if self.wait_exit(KILL_GRACE) != TermStatus::Running {
                return;
            }
        }
    }

    /// Sends SIGHUP to the child's process group and returns at once: the caller watches
    /// [`Terminal::status`] and decides how long the child gets (a restart that must not overlap
    /// the old run). False once the child has exited.
    pub fn hang_up(&self) -> bool {
        self.signal_group(rustix::process::Signal::HUP)
    }

    /// Sends SIGKILL to the child's process group and returns at once (for a child that outlived
    /// its time after [`Terminal::hang_up`]). False once the child has exited.
    pub fn force_kill(&self) -> bool {
        self.signal_group(rustix::process::Signal::KILL)
    }

    /// Signals the child's process group while the child is unreaped; false once it has exited.
    fn signal_group(&self, signal: rustix::process::Signal) -> bool {
        // The waiter reaps under this lock, so while it reads Running the pid cannot have been
        // recycled and the group is still ours.
        let status = lock(&self.shared.status);
        if *status != TermStatus::Running {
            return false;
        }
        if let Some(pid) = self.pid {
            let _ = rustix::process::kill_process_group(pid, signal);
        }
        true
    }

    /// The emulator, taken ahead of the reader so a flood cannot hold the UI thread up.
    fn term(&self) -> MutexGuard<'_, Term<Listener>> {
        self.shared.snapshot_waiting.fetch_add(1, Ordering::SeqCst);
        let term = lock(&self.shared.term);
        self.shared.snapshot_waiting.fetch_sub(1, Ordering::SeqCst);
        term
    }

    /// Moves the view `lines` up into the history (negative: down), clamped to the history;
    /// returns the new offset. The emulator keeps a scrolled-back view on the same lines while
    /// output arrives: the offset grows with every line pushed into the history (up to the
    /// scrollback cap, where the oldest lines drop out), so new output never pulls it down.
    /// At offset 0 the view follows the output.
    pub fn scroll(&self, lines: isize) -> usize {
        // Far beyond any history, and safe from overflow inside the emulator's i32 maths.
        const FAR: isize = 1 << 30;
        let mut term = self.term();
        term.scroll_display(Scroll::Delta(lines.clamp(-FAR, FAR) as i32));
        term.grid().display_offset()
    }

    /// Back to the bottom: the view follows the output again.
    pub fn scroll_to_bottom(&self) {
        self.term().scroll_display(Scroll::Bottom);
    }

    /// The screen at the view's offset into the history (see [`Terminal::scroll`]), with
    /// cursor, title and modes. Also re-arms `Notify`.
    pub fn snapshot(&self) -> Grid {
        let term = self.term();
        self.shared.dirty.store(false, Ordering::SeqCst);
        let captured = snapshot::capture(&term);
        // The title lives beside the emulator but is written under its lock (by the listener).
        let title = lock(&self.shared.title).clone();
        drop(term);
        captured.into_grid(title)
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::SeqCst);
        self.kill();
        let _ = self.input.send(Input::Stop);
        // Wake the reader and writer: they return and close their master fds, so with ours
        // gone the master is closed and the kernel hangs up whatever still holds the slave.
        let _ = rustix::io::write(&*self.wake, &1u64.to_ne_bytes());
        self.master.take();
        // Normally both are gone within microseconds; a thread stuck anyway (never seen) is
        // detached after JOIN_WAIT instead of holding up the caller.
        let deadline = Instant::now() + JOIN_WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.alive.recv_timeout(left) {
                Ok(()) => continue,
                Err(RecvTimeoutError::Disconnected | RecvTimeoutError::Timeout) => break,
            }
        }
        for handle in self.threads.drain(..) {
            if handle.is_finished() {
                let _ = handle.join();
            }
        }
    }
}

fn pty_size(cols: u16, rows: u16, cell_w: u16, cell_h: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: cols.saturating_mul(cell_w),
        pixel_height: rows.saturating_mul(cell_h),
    }
}

/// Our own non-blocking, close-on-exec copies of the master for the reader and the writer, and
/// the wake eventfd. portable-pty's reader / writer block in read/write, which `Drop` could not
/// interrupt.
fn pty_io(master: &(dyn MasterPty + Send)) -> std::io::Result<(OwnedFd, OwnedFd, Arc<OwnedFd>)> {
    use rustix::event::{EventfdFlags, eventfd};
    use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
    use rustix::io::fcntl_dupfd_cloexec;
    let raw = master
        .as_raw_fd()
        .ok_or_else(|| std::io::Error::other("pty master has no fd"))?;
    // SAFETY: `master` owns `raw` and outlives this borrow; it is only duplicated here.
    let fd = unsafe { BorrowedFd::borrow_raw(raw) };
    // CLOEXEC: another terminal spawned later must not inherit (and so keep alive) this PTY.
    let reader = fcntl_dupfd_cloexec(fd, 0)?;
    let writer = fcntl_dupfd_cloexec(fd, 0)?;
    // O_NONBLOCK lives on the shared open file description: one call covers every copy.
    fcntl_setfl(&reader, fcntl_getfl(&reader)? | OFlags::NONBLOCK)?;
    let wake = eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK)?;
    Ok((reader, writer, Arc::new(wake)))
}

/// Waits until `fd` is ready for `events` or `wake` fires; false on wake or a poll error.
fn ready(fd: &OwnedFd, events: PollFlags, wake: &OwnedFd) -> bool {
    loop {
        let mut fds = [PollFd::new(fd, events), PollFd::new(wake, PollFlags::IN)];
        match poll(&mut fds, None) {
            Ok(_) if !fds[1].revents().is_empty() => return false,
            Ok(_) if !fds[0].revents().is_empty() => return true,
            Ok(_) | Err(Errno::INTR) => continue,
            Err(_) => return false,
        }
    }
}

fn read_loop(reader: &OwnedFd, wake: &OwnedFd, shared: &Shared, mut scan: Option<MarkerScan>) {
    let mut parser: Processor = Processor::new();
    let mut buf = vec![0u8; READ_BUF];
    let mut seen = AttentionScan::default();
    let mut found = Vec::new();
    // What is left of a read once the marker scanner took its bytes out.
    let mut kept = Vec::new();
    let mut parse = |bytes: &[u8]| {
        for slice in bytes.chunks(PARSE_SLICE) {
            while shared.snapshot_waiting.load(Ordering::SeqCst) > 0 {
                thread::yield_now();
            }
            parser.advance(&mut *lock(&shared.term), slice);
        }
    };
    loop {
        // A holder of the slave that writes faster than we parse would never let the read hit
        // EAGAIN (where the wake fd is polled), so look at the close flag on every round too.
        if shared.closed.load(Ordering::SeqCst) {
            return;
        }
        let n = match rustix::io::read(reader, &mut buf[..]) {
            Ok(n) if n > 0 => n,
            Err(Errno::INTR) => continue,
            Err(Errno::AGAIN) if ready(reader, PollFlags::IN, wake) => continue,
            // EOF, or EIO once every holder of the slave side has gone, or `Drop`'s wake.
            Ok(_) | Err(_) => break,
        };
        if shared.watch_attention {
            found.clear();
            seen.feed(&buf[..n], &mut found);
            if !found.is_empty() {
                shared.push_attention(&mut found);
            }
        }
        let Some(scan) = &mut scan else {
            parse(&buf[..n]);
            shared.mark_dirty();
            continue;
        };
        kept.clear();
        let hits = scan.feed(&buf[..n], &mut kept);
        parse(&kept);
        // Only the first marker counts, and only while the child runs: whatever still holds
        // the PTY after its exit proves nothing about the session.
        if hits > 0
            && *lock(&shared.status) == TermStatus::Running
            && !shared.marker_seen.swap(true, Ordering::SeqCst)
            && !shared.closed.load(Ordering::SeqCst)
        {
            // Not coalesced with output: the UI must hear of it even when it has not looked
            // at this terminal since the last output.
            (shared.notify)();
        }
        shared.mark_dirty();
    }
    // The start of a marker that never completed is ordinary output.
    if let Some(scan) = &mut scan
        && !shared.closed.load(Ordering::SeqCst)
    {
        kept.clear();
        scan.flush(&mut kept);
        if !kept.is_empty() {
            parse(&kept);
            shared.mark_dirty();
        }
    }
}

fn write_loop(writer: &OwnedFd, wake: &OwnedFd, rx: Receiver<Input>, pending: &AtomicUsize) {
    while let Ok(Input::Bytes(bytes)) = rx.recv() {
        let mut rest = &bytes[..];
        let ok = loop {
            if rest.is_empty() {
                break true;
            }
            match rustix::io::write(writer, rest) {
                Ok(n) => rest = &rest[n..],
                Err(Errno::INTR) => {}
                Err(Errno::AGAIN) => {
                    if !ready(writer, PollFlags::OUT, wake) {
                        break false;
                    }
                }
                Err(_) => break false,
            }
        };
        pending.fetch_sub(bytes.len(), Ordering::SeqCst);
        if !ok {
            return;
        }
    }
}

fn wait_loop(
    mut child: Box<dyn portable_pty::Child + Send + Sync>,
    pid: Option<rustix::process::Pid>,
    shared: &Shared,
) {
    use rustix::process::{WaitId, WaitIdOptions, waitid};
    // Wait without reaping: the pid (= process group) stays ours until the reap below, which
    // happens under the status lock that `signal_group` takes.
    let code = pid.and_then(|pid| {
        loop {
            match waitid(
                WaitId::Pid(pid),
                WaitIdOptions::EXITED | WaitIdOptions::NOWAIT,
            ) {
                Ok(Some(st)) => {
                    break st
                        .exit_status()
                        .or_else(|| st.terminating_signal().map(|sig| 128 + sig));
                }
                Ok(None) | Err(rustix::io::Errno::INTR) => continue,
                Err(_) => break None,
            }
        }
    });
    let mut status = lock(&shared.status);
    let reaped = child.wait();
    let code = code
        .or_else(|| reaped.ok().map(|s| s.exit_code() as i32))
        .unwrap_or(-1);
    *status = TermStatus::Exited(code);
    drop(status);
    shared.status_changed.notify_all();
    shared.mark_dirty();
}

#[cfg(test)]
mod pixel_tests {
    use super::pty_size;

    #[test]
    fn pty_size_multiplies_and_saturates() {
        let s = pty_size(80, 24, 9, 18);
        assert_eq!((s.pixel_width, s.pixel_height), (720, 432));
        let s = pty_size(80, 24, 0, 0);
        assert_eq!((s.pixel_width, s.pixel_height), (0, 0));
        let s = pty_size(1000, 1000, 200, 200);
        assert_eq!((s.pixel_width, s.pixel_height), (u16::MAX, u16::MAX));
    }
}

#[cfg(test)]
mod pixel_pty_tests {
    use super::{SpawnOptions, Terminal};
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn resize_with_pixels_reaches_the_pty_and_resize_clears_them() {
        let term = Terminal::spawn_with(
            SpawnOptions {
                shell: PathBuf::from("/bin/sh"),
                args: vec![OsString::from("-c"), OsString::from("read x")],
                cwd: PathBuf::from("/"),
                env: Vec::new(),
                cols: 80,
                rows: 24,
                scrollback: 100,
            },
            Default::default(),
            Arc::new(|| {}),
        )
        .expect("spawn /bin/sh");
        let size = |t: &Terminal| t.master.as_ref().unwrap().get_size().unwrap();
        term.resize_with_pixels(100, 30, 9, 18).unwrap();
        let s = size(&term);
        assert_eq!(
            (s.cols, s.rows, s.pixel_width, s.pixel_height),
            (100, 30, 900, 540)
        );
        term.resize(80, 24).unwrap();
        let s = size(&term);
        assert_eq!((s.pixel_width, s.pixel_height), (0, 0));
    }
}
